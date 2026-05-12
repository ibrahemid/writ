//! Bridge between the in-process `writ-core` event bus and the
//! Tauri-frontend event channel.
//!
//! The bus carries [`WritEvent`] payloads from core code that has no
//! Tauri dependency. This module subscribes once at app startup, maps
//! each supported variant to its [`WritFrontendEvent`] counterpart,
//! and hands the result to the supplied emit closure. Splitting the
//! emit side from the translation side keeps the bridge unit-testable
//! without an `AppHandle`.

use writ_core::events::bus::{EventBus, WritEvent};

use crate::events::WritFrontendEvent;

/// Subscribes a bridge handler to `bus`.
///
/// Whenever a [`WritEvent`] arrives that has a frontend mapping, the
/// translated [`WritFrontendEvent`] is passed to `on_emit`. Variants
/// without a current mapping are dropped silently; the bridge is
/// additive and does not enforce coverage.
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
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn bridge_translates_buffer_opened_to_frontend_event() {
        let bus = EventBus::new();
        let captured: Arc<Mutex<Vec<WritFrontendEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = captured.clone();

        attach_bridge(&bus, move |event| {
            captured_clone.lock().unwrap().push(event);
        });

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
    fn bridge_drops_events_without_a_frontend_mapping() {
        let bus = EventBus::new();
        let captured: Arc<Mutex<Vec<WritFrontendEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = captured.clone();

        attach_bridge(&bus, move |event| {
            captured_clone.lock().unwrap().push(event);
        });

        bus.emit(WritEvent::HotkeyToggle);
        bus.emit(WritEvent::PluginEvent {
            plugin_id: "plg".to_string(),
            data: serde_json::Value::Null,
        });

        assert!(captured.lock().unwrap().is_empty());
    }
}
