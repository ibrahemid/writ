use crate::watcher::change_event::ExternalChange;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

/// Domain events emitted by `writ-core`.
///
/// All variants are `Serialize` / `Deserialize` so the Tauri adapter can
/// forward them to the frontend without re-encoding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WritEvent {
    /// A buffer transitioned to [`crate::buffer::document::BufferStatus::Active`].
    ///
    /// Emitted by [`crate::buffer::manager::BufferManager`] for both
    /// scratch buffers and externally-opened files. The bridge in the
    /// host adapter translates this into a frontend-visible event.
    BufferOpened {
        /// Identifier of the buffer that became active.
        id: String,
        /// Human-readable title at the moment of opening.
        title: String,
    },
    /// The user's configuration changed. `keys` lists the dotted paths
    /// that changed, so listeners can respond selectively.
    ConfigChanged {
        /// Dotted config paths that changed (for example `editor.font_size`).
        keys: Vec<String>,
    },
    /// A buffer's backing file was modified or deleted externally.
    BufferExternal {
        /// Identifier of the buffer that observed the change.
        buffer_id: String,
        /// Nature of the external change.
        change: ExternalChange,
    },
    /// A file or directory inside the open workspace folder changed on
    /// disk. Listeners refresh the affected directory listing; the event
    /// carries no content.
    WorkspaceChanged {
        /// Absolute path of the changed entry.
        path: String,
        /// `true` when the entry no longer exists on disk.
        removed: bool,
    },
    /// A qualifying new file appeared inside the watched inbox folder
    /// (ADR-018). The frontend opens it through the normal open path.
    InboxFileArrived {
        /// Absolute path of the file that arrived.
        path: String,
    },
    /// The global toggle hotkey was pressed.
    HotkeyToggle,
    /// A native menu item was activated. Originates in the host shell;
    /// routed through the bus so the frontend has a single path to
    /// observe menu activations.
    MenuAction {
        /// Stable identifier of the menu item, matching the
        /// `command.id` registered in the frontend command palette.
        action: String,
    },
    /// A plugin-defined event payload.
    PluginEvent {
        /// Identifier of the plugin that emitted the event.
        plugin_id: String,
        /// Opaque payload; interpretation is plugin-specific.
        data: serde_json::Value,
    },
}

type Subscriber = Arc<dyn Fn(&WritEvent) + Send + Sync>;

/// Fan-out event bus for [`WritEvent`] payloads.
///
/// Subscribers are invoked synchronously in the order they subscribed.
/// The bus is internally synchronized and may be shared across threads.
pub struct EventBus {
    subscribers: Arc<Mutex<Vec<Subscriber>>>,
}

impl EventBus {
    /// Creates an empty bus with no subscribers.
    pub fn new() -> Self {
        Self {
            subscribers: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Registers a handler to be invoked for every subsequent event.
    pub fn subscribe<F>(&self, handler: F)
    where
        F: Fn(&WritEvent) + Send + Sync + 'static,
    {
        self.subscribers
            .lock()
            .unwrap_or_else(|poisoned| {
                tracing::error!(
                    location = "events::bus::subscribe",
                    "recovered poisoned mutex"
                );
                poisoned.into_inner()
            })
            .push(Arc::new(handler));
    }

    /// Delivers `event` to every current subscriber.
    ///
    /// Subscribers added after this call returns will not receive the
    /// event. Poisoned locks are recovered transparently so a panicking
    /// subscriber cannot take the bus down with it.
    pub fn emit(&self, event: WritEvent) {
        let subscribers = self
            .subscribers
            .lock()
            .unwrap_or_else(|poisoned| {
                tracing::error!(location = "events::bus::emit", "recovered poisoned mutex");
                poisoned.into_inner()
            })
            .clone();
        for subscriber in subscribers {
            subscriber(&event);
        }
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}
