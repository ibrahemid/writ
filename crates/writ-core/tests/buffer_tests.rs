use std::sync::{Arc, Mutex};
use writ_core::buffer::{BufferManager, BufferStatus};
use writ_core::events::bus::{EventBus, WritEvent};

#[test]
fn create_buffer_assigns_uuid_and_timestamp() {
    let mut manager = BufferManager::new();
    let buf = manager.create_buffer(None).unwrap();

    assert!(!buf.id.is_empty());
    assert!(buf.title.starts_with("writ-"));
    assert_eq!(buf.status, BufferStatus::Active);
    assert_eq!(buf.tab_order, 0);
}

#[test]
fn create_buffer_with_title() {
    let mut manager = BufferManager::new();
    let buf = manager.create_buffer(Some("my-note".to_string())).unwrap();

    assert_eq!(buf.title, "my-note");
}

#[test]
fn close_buffer_moves_to_history() {
    let mut manager = BufferManager::new();
    let buf = manager.create_buffer(None).unwrap();

    manager.close_buffer(&buf.id).unwrap();
    let closed = manager.get_buffer(&buf.id).unwrap();

    assert_eq!(closed.status, BufferStatus::History);
    assert!(closed.closed_at.is_some());
}

#[test]
fn restore_buffer_moves_to_active() {
    let mut manager = BufferManager::new();
    let buf = manager.create_buffer(None).unwrap();
    manager.close_buffer(&buf.id).unwrap();

    manager.restore_buffer(&buf.id).unwrap();
    let restored = manager.get_buffer(&buf.id).unwrap();

    assert_eq!(restored.status, BufferStatus::Active);
    assert!(restored.closed_at.is_none());
}

#[test]
fn delete_buffer_removes_it() {
    let mut manager = BufferManager::new();
    let buf = manager.create_buffer(None).unwrap();
    let id = buf.id.clone();

    manager.delete_buffer(&id).unwrap();

    assert!(manager.get_buffer(&id).is_err());
}

#[test]
fn list_active_buffers_excludes_history() {
    let mut manager = BufferManager::new();
    let a = manager.create_buffer(Some("a".to_string())).unwrap();
    let b = manager.create_buffer(Some("b".to_string())).unwrap();

    manager.close_buffer(&a.id).unwrap();

    let active = manager.list_active();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, b.id);
}

#[test]
fn list_history_excludes_active() {
    let mut manager = BufferManager::new();
    let a = manager.create_buffer(Some("a".to_string())).unwrap();
    let _b = manager.create_buffer(Some("b".to_string())).unwrap();

    manager.close_buffer(&a.id).unwrap();

    let history = manager.list_history();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].id, a.id);
}

#[test]
fn close_nonexistent_buffer_returns_error() {
    let mut manager = BufferManager::new();
    let result = manager.close_buffer("nonexistent-id");

    assert!(result.is_err());
}

#[test]
fn tab_order_increments_on_create() {
    let mut manager = BufferManager::new();
    let first = manager.create_buffer(None).unwrap();
    let second = manager.create_buffer(None).unwrap();

    assert_eq!(first.tab_order, 0);
    assert_eq!(second.tab_order, 1);
}

#[test]
fn reorder_tabs_updates_order() {
    let mut manager = BufferManager::new();
    let a = manager.create_buffer(Some("a".to_string())).unwrap();
    let b = manager.create_buffer(Some("b".to_string())).unwrap();

    manager.reorder_tabs(&[b.id.clone(), a.id.clone()]).unwrap();

    let reordered_b = manager.get_buffer(&b.id).unwrap();
    let reordered_a = manager.get_buffer(&a.id).unwrap();

    assert_eq!(reordered_b.tab_order, 0);
    assert_eq!(reordered_a.tab_order, 1);
}

#[test]
fn open_external_file_sets_source_path() {
    let mut manager = BufferManager::new();
    let path = "/home/user/notes/todo.md".to_string();
    let buf = manager.open_external(path.clone()).unwrap();

    assert_eq!(buf.source_path, Some(path));
    assert_eq!(buf.title, "todo.md");
    assert_eq!(buf.filename, "todo.md");
}

#[test]
fn create_buffer_publishes_buffer_opened_when_bus_attached() {
    let bus = Arc::new(EventBus::new());
    let received: Arc<Mutex<Vec<WritEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let received_clone = received.clone();
    bus.subscribe(move |event| {
        received_clone.lock().unwrap().push(event.clone());
    });

    let mut manager = BufferManager::new().with_event_bus(bus.clone());
    let doc = manager.create_buffer(Some("draft".to_string())).unwrap();

    let events = received.lock().unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        WritEvent::BufferOpened { id, title } => {
            assert_eq!(id, &doc.id);
            assert_eq!(title, "draft");
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn open_external_publishes_buffer_opened_when_bus_attached() {
    let bus = Arc::new(EventBus::new());
    let received: Arc<Mutex<Vec<WritEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let received_clone = received.clone();
    bus.subscribe(move |event| {
        received_clone.lock().unwrap().push(event.clone());
    });

    let mut manager = BufferManager::new().with_event_bus(bus.clone());
    let doc = manager
        .open_external("/tmp/example/notes.md".to_string())
        .unwrap();

    let events = received.lock().unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        WritEvent::BufferOpened { id, title } => {
            assert_eq!(id, &doc.id);
            assert_eq!(title, "notes.md");
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn buffer_manager_without_event_bus_does_not_panic_or_publish() {
    let mut manager = BufferManager::new();
    let buf = manager.create_buffer(None).unwrap();
    assert!(!buf.id.is_empty());
}
