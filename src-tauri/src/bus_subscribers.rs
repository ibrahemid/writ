//! Everything the application hangs on its event bus.
//!
//! [`EventBus::emit`] runs every subscriber on the emitting thread before it
//! returns, and some emitters hold the buffer store's lock while they emit: a
//! reopen of a tab whose file changed announces the change from under it. A
//! subscriber that took the store would wait on its own thread for good, with
//! every later save behind it. So no subscriber takes the store or reaches
//! anything that does; what an event needs done to the store is done by the
//! code that emits it, under the lock it holds (ADR-043 §1).
//!
//! The application subscribes here and nowhere else, so a test can hang the
//! same subscribers on a state it built, hold the store and emit every event
//! (`src-tauri/tests/event_bus_store_lock_tests.rs`).

use std::path::Path;

use tauri::{AppHandle, Manager};
use tracing::info;
use writ_core::events::bus::{EventBus, WritEvent};

use crate::events::{bus_bridge, emit_event, WritFrontendEvent};
use crate::state::AppState;

/// What the subscribers reach the running application through.
///
/// The application is the [`AppHandle`]; a test supplies a state of its own.
/// No method may take the buffer store's lock, for the reason the module
/// gives.
pub trait BusHost: Clone + Send + Sync + 'static {
    /// The state every subscriber acts on.
    fn app_state(&self) -> &AppState;
    /// Hands an event to the frontend.
    fn to_frontend(&self, event: WritFrontendEvent);
    /// Redraws the menu bar after the configuration was read again.
    fn redraw_menu_bar(&self);
    /// Tells the frontend the programs waiting for approval changed.
    fn announce_waiting_clients(&self);
}

impl BusHost for AppHandle {
    fn app_state(&self) -> &AppState {
        self.state::<AppState>().inner()
    }

    fn to_frontend(&self, event: WritFrontendEvent) {
        let _ = emit_event(self, event);
    }

    fn redraw_menu_bar(&self) {
        crate::app_menu::sync(self);
    }

    fn announce_waiting_clients(&self) {
        crate::commands::activity::announce_waiting(self);
    }
}

/// Subscribes everything the application listens for on its bus, in the
/// order it has to hear it.
pub fn attach_bus_subscribers<H: BusHost>(host: &H) {
    let bus: &EventBus = &host.app_state().event_bus;

    // An edit to config.toml from outside Writ is read back into the copy
    // every command answers from. Subscribed ahead of the bridge, so by the
    // time the frontend hears the file changed and asks for the config, the
    // answer is the file's.
    {
        let host = host.clone();
        bus.subscribe(move |event| {
            if let WritEvent::ConfigChanged { .. } = event {
                if host.app_state().reload_config_from_disk() {
                    host.redraw_menu_bar();
                }
            }
        });
    }

    {
        let host = host.clone();
        bus_bridge::attach_bridge_behind_notes_index(
            bus,
            host.app_state().notes_index.clone(),
            move |frontend_event| host.to_frontend(frontend_event),
        );
        info!("event bus bridge attached");
    }

    {
        let host = host.clone();
        bus.subscribe(move |event| {
            if let WritEvent::McpPendingChanged = event {
                host.announce_waiting_clients();
            }
        });
    }

    // Maintain the workspace file-name index from watcher events (ADR-026). A
    // separate subscriber from the frontend bridge, so both observe every
    // WorkspaceChanged.
    {
        let index = host.app_state().workspace_index.clone();
        bus.subscribe(move |event| match event {
            WritEvent::WorkspaceChanged { path, removed } => {
                crate::workspace_index::on_workspace_changed(&index, path, *removed);
            }
            WritEvent::WorkspaceSwept { .. } => {
                crate::workspace_index::on_workspace_swept(&index);
            }
            _ => {}
        });
    }

    // Keep the notes index honest when the watcher saw more change than it
    // listed (ADR-028 section 7). A file another program wrote is patched in
    // ahead of the bridge instead (`bus_bridge::attach_bridge_behind_notes_index`).
    {
        let state = host.app_state();
        let index = state.notes_index.clone();
        let cancel = state.notes_index_cancel.clone();
        let reconcile_gate = state.notes_reconcile.clone();
        let host = host.clone();
        bus.subscribe(move |event| {
            // More changed in one window than was worth listing, so the index
            // walks the folder instead of patching a path. The root is read
            // live rather than captured: moving the notes folder restarts the
            // watcher, and the walk has to follow.
            if let WritEvent::NotesSwept { .. } = event {
                let state = host.app_state();
                crate::spawn_notes_reconcile(
                    index.clone(),
                    cancel.clone(),
                    reconcile_gate.clone(),
                    state.notes_root(),
                    state.event_bus.clone(),
                );
            }
        });
    }

    // A rename the note's own first line asks for is only safe while nothing
    // outside Writ has looked at the note. The bus is where that is seen:
    // every write Writ made has already been dropped by the watcher's ignore
    // set before an event reaches here.
    {
        let watch = host.app_state().retitle_watch.clone();
        bus.subscribe(move |event| {
            if let WritEvent::NotesChanged { path, .. } = event {
                watch.changed_outside(Path::new(path));
            }
        });
    }
}
