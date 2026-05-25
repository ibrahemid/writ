//! Window registry: [`writ_core::preview::WindowId`] ↔ Tauri webview
//! window labels.
//!
//! The frontend stores per-window state under a numeric `WindowId`
//! (`WindowId::MAIN` is `1`; detached preview windows are assigned the
//! next monotonic value at spawn time). Tauri identifies its windows by
//! string labels (`"main"`, `"preview-2"`, …). This module owns the
//! mapping in both directions plus the focus signal that
//! `windowRegistry.focus(…)` consumes on the frontend.
//!
//! Phase 1 ships the registry shape and registers `WindowId::MAIN` at
//! startup. Phase 2 wires the detached-window spawn path that allocates
//! the next id, attaches the label, and emits the
//! `WindowOpened`/`WindowClosed`/`WindowFocusChanged` events through the
//! existing bus.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use writ_core::preview::WindowId;

/// Label Tauri uses for the always-present main webview window.
pub const MAIN_WINDOW_LABEL: &str = "main";

/// Bidirectional mapping between [`WindowId`] and Tauri webview labels.
pub struct WindowManager {
    next_id: AtomicU64,
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    id_to_label: HashMap<WindowId, String>,
    label_to_id: HashMap<String, WindowId>,
    focused: Option<WindowId>,
}

impl WindowManager {
    /// Construct a manager seeded with the main window mapping.
    pub fn with_main() -> Self {
        let mgr = Self {
            // `WindowId::MAIN` is `1`; next allocation begins at 2.
            next_id: AtomicU64::new(WindowId::MAIN.0 + 1),
            inner: Mutex::new(Inner::default()),
        };
        mgr.register(WindowId::MAIN, MAIN_WINDOW_LABEL.to_string());
        mgr.set_focused(Some(WindowId::MAIN));
        mgr
    }

    /// Allocate the next monotonic [`WindowId`].
    pub fn allocate_id(&self) -> WindowId {
        WindowId(self.next_id.fetch_add(1, Ordering::SeqCst))
    }

    /// Register a `(window_id, label)` pair. Replaces any prior mapping.
    pub fn register(&self, id: WindowId, label: String) {
        let mut inner = self.inner.lock().expect("window manager mutex");
        if let Some(prev_label) = inner.id_to_label.insert(id, label.clone()) {
            inner.label_to_id.remove(&prev_label);
        }
        inner.label_to_id.insert(label, id);
    }

    /// Remove an entry, returning the prior label.
    pub fn unregister(&self, id: WindowId) -> Option<String> {
        let mut inner = self.inner.lock().expect("window manager mutex");
        if inner.focused == Some(id) {
            inner.focused = None;
        }
        let label = inner.id_to_label.remove(&id)?;
        inner.label_to_id.remove(&label);
        Some(label)
    }

    /// Look up the label for a given id.
    pub fn label_for(&self, id: WindowId) -> Option<String> {
        let inner = self.inner.lock().expect("window manager mutex");
        inner.id_to_label.get(&id).cloned()
    }

    /// Look up the id for a given label.
    pub fn id_for(&self, label: &str) -> Option<WindowId> {
        let inner = self.inner.lock().expect("window manager mutex");
        inner.label_to_id.get(label).copied()
    }

    /// Snapshot of every registered window.
    pub fn list(&self) -> Vec<(WindowId, String)> {
        let inner = self.inner.lock().expect("window manager mutex");
        inner
            .id_to_label
            .iter()
            .map(|(id, label)| (*id, label.clone()))
            .collect()
    }

    /// Set the currently focused window.
    pub fn set_focused(&self, id: Option<WindowId>) {
        let mut inner = self.inner.lock().expect("window manager mutex");
        inner.focused = id;
    }

    /// Currently focused window, if any.
    pub fn focused(&self) -> Option<WindowId> {
        let inner = self.inner.lock().expect("window manager mutex");
        inner.focused
    }
}

impl Default for WindowManager {
    fn default() -> Self {
        Self::with_main()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_main_registers_window_id_main() {
        let m = WindowManager::with_main();
        assert_eq!(m.label_for(WindowId::MAIN), Some(MAIN_WINDOW_LABEL.into()));
        assert_eq!(m.id_for(MAIN_WINDOW_LABEL), Some(WindowId::MAIN));
        assert_eq!(m.focused(), Some(WindowId::MAIN));
    }

    #[test]
    fn allocate_id_is_monotonic_starting_after_main() {
        let m = WindowManager::with_main();
        let a = m.allocate_id();
        let b = m.allocate_id();
        assert!(a.0 > WindowId::MAIN.0);
        assert!(b.0 > a.0);
    }

    #[test]
    fn register_and_unregister_round_trip() {
        let m = WindowManager::with_main();
        let id = m.allocate_id();
        m.register(id, "preview-2".to_string());
        assert_eq!(m.label_for(id), Some("preview-2".into()));
        assert_eq!(m.id_for("preview-2"), Some(id));
        m.unregister(id);
        assert_eq!(m.label_for(id), None);
        assert_eq!(m.id_for("preview-2"), None);
    }

    #[test]
    fn register_replaces_prior_label_mapping() {
        let m = WindowManager::with_main();
        let id = m.allocate_id();
        m.register(id, "first".to_string());
        m.register(id, "second".to_string());
        assert_eq!(m.label_for(id), Some("second".into()));
        assert_eq!(m.id_for("first"), None);
        assert_eq!(m.id_for("second"), Some(id));
    }

    #[test]
    fn unregister_clears_focus_if_pointed_at_target() {
        let m = WindowManager::with_main();
        let id = m.allocate_id();
        m.register(id, "detached".to_string());
        m.set_focused(Some(id));
        assert_eq!(m.focused(), Some(id));
        m.unregister(id);
        assert_eq!(m.focused(), None);
    }

    #[test]
    fn list_returns_every_entry() {
        let m = WindowManager::with_main();
        let id = m.allocate_id();
        m.register(id, "preview-2".to_string());
        let listed: std::collections::HashMap<WindowId, String> =
            m.list().into_iter().collect();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed.get(&WindowId::MAIN).unwrap(), MAIN_WINDOW_LABEL);
        assert_eq!(listed.get(&id).unwrap(), "preview-2");
    }
}
