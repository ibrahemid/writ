//! Bridge between the in-process `writ-core` event bus and the
//! Tauri-frontend event channel.
//!
//! The bus carries [`WritEvent`] payloads from code that has no
//! Tauri dependency (writ-core `BufferManager`, the watcher in
//! `src-tauri/watcher/handler.rs`, the menu callback in `lib.rs`).
//! This module subscribes once at app startup, maps each supported
//! variant to its [`WritFrontendEvent`] counterpart, and hands the
//! result to the supplied emit closure. Splitting the emit side from
//! the translation side keeps the bridge unit-testable without an
//! `AppHandle`.
//!
//! Variants without a frontend mapping are intentionally dropped:
//! [`WritEvent::HotkeyToggle`] and [`WritEvent::PluginEvent`] are
//! domain-only signals with no current frontend consumer.

use writ_core::events::bus::{EventBus, WritEvent};
use writ_core::watcher::change_event::ExternalChange;

use crate::events::WritFrontendEvent;

/// Subscribes a bridge handler to `bus`.
pub fn attach_bridge<F>(bus: &EventBus, on_emit: F)
where
    F: Fn(WritFrontendEvent) + Send + Sync + 'static,
{
    bus.subscribe(move |event| {
        if let Some(frontend_event) = translate(event) {
            on_emit(frontend_event);
        }
    });
}

fn translate(event: &WritEvent) -> Option<WritFrontendEvent> {
    match event {
        WritEvent::BufferOpened { id, title } => Some(WritFrontendEvent::BufferOpened {
            id: id.clone(),
            title: title.clone(),
        }),
        WritEvent::ConfigChanged { keys } => Some(WritFrontendEvent::ConfigChanged {
            keys: keys.clone(),
        }),
        WritEvent::BufferExternal { buffer_id, change } => Some(WritFrontendEvent::BufferExternal {
            buffer_id: buffer_id.clone(),
            change: match change {
                ExternalChange::Modified => "modified".to_string(),
                ExternalChange::Deleted => "deleted".to_string(),
            },
        }),
        WritEvent::MenuAction { action } => Some(WritFrontendEvent::MenuAction {
            action: action.clone(),
        }),
        WritEvent::WorkspaceChanged { path, removed } => {
            Some(WritFrontendEvent::WorkspaceChanged {
                path: path.clone(),
                removed: *removed,
            })
        }
        WritEvent::InboxFileArrived { path } => Some(WritFrontendEvent::InboxFileArrived {
            path: path.clone(),
        }),
        WritEvent::HotkeyToggle | WritEvent::PluginEvent { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn capture(bus: &EventBus) -> Arc<Mutex<Vec<WritFrontendEvent>>> {
        let captured: Arc<Mutex<Vec<WritFrontendEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = captured.clone();
        attach_bridge(bus, move |event| {
            captured_clone.lock().unwrap().push(event);
        });
        captured
    }

    #[test]
    fn bridge_translates_buffer_opened_to_frontend_event() {
        let bus = EventBus::new();
        let captured = capture(&bus);

        bus.emit(WritEvent::BufferOpened {
            id: "id-1".to_string(),
            title: "draft".to_string(),
        });

        let events = captured.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            WritFrontendEvent::BufferOpened {
                id: "id-1".to_string(),
                title: "draft".to_string(),
            }
        );
    }

    #[test]
    fn bridge_translates_config_changed_to_frontend_event() {
        let bus = EventBus::new();
        let captured = capture(&bus);

        bus.emit(WritEvent::ConfigChanged {
            keys: vec!["theme".to_string(), "font_size".to_string()],
        });

        let events = captured.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            WritFrontendEvent::ConfigChanged {
                keys: vec!["theme".to_string(), "font_size".to_string()],
            }
        );
    }

    #[test]
    fn bridge_translates_buffer_external_modified_to_frontend_event() {
        let bus = EventBus::new();
        let captured = capture(&bus);

        bus.emit(WritEvent::BufferExternal {
            buffer_id: "draft-1.txt".to_string(),
            change: ExternalChange::Modified,
        });

        let events = captured.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            WritFrontendEvent::BufferExternal {
                buffer_id: "draft-1.txt".to_string(),
                change: "modified".to_string(),
            }
        );
    }

    #[test]
    fn bridge_translates_buffer_external_deleted_to_frontend_event() {
        let bus = EventBus::new();
        let captured = capture(&bus);

        bus.emit(WritEvent::BufferExternal {
            buffer_id: "draft-2.txt".to_string(),
            change: ExternalChange::Deleted,
        });

        let events = captured.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            WritFrontendEvent::BufferExternal {
                buffer_id: "draft-2.txt".to_string(),
                change: "deleted".to_string(),
            }
        );
    }

    #[test]
    fn bridge_translates_menu_action_to_frontend_event() {
        let bus = EventBus::new();
        let captured = capture(&bus);

        bus.emit(WritEvent::MenuAction {
            action: "file.open".to_string(),
        });

        let events = captured.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            WritFrontendEvent::MenuAction {
                action: "file.open".to_string(),
            }
        );
    }

    #[test]
    fn bridge_translates_workspace_changed_to_frontend_event() {
        let bus = EventBus::new();
        let received = capture(&bus);

        bus.emit(WritEvent::WorkspaceChanged {
            path: "/ws/src/main.rs".to_string(),
            removed: false,
        });

        let events = received.lock().unwrap();
        assert_eq!(
            events.as_slice(),
            [WritFrontendEvent::WorkspaceChanged {
                path: "/ws/src/main.rs".to_string(),
                removed: false,
            }]
        );
    }

    #[test]
    fn bridge_translates_inbox_file_arrived_to_frontend_event() {
        let bus = EventBus::new();
        let received = capture(&bus);

        bus.emit(WritEvent::InboxFileArrived {
            path: "/inbox/report.md".to_string(),
        });

        let events = received.lock().unwrap();
        assert_eq!(
            events.as_slice(),
            [WritFrontendEvent::InboxFileArrived {
                path: "/inbox/report.md".to_string(),
            }]
        );
    }

    #[test]
    fn bridge_drops_events_without_a_frontend_mapping() {
        let bus = EventBus::new();
        let captured = capture(&bus);

        bus.emit(WritEvent::HotkeyToggle);
        bus.emit(WritEvent::PluginEvent {
            plugin_id: "plg".to_string(),
            data: serde_json::Value::Null,
        });

        assert!(captured.lock().unwrap().is_empty());
    }
}
