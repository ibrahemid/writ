use std::sync::{Arc, Mutex};
use writ_core::events::bus::{EventBus, WritEvent};

#[test]
fn subscribe_and_emit_event() {
    let bus = EventBus::new();
    let received: Arc<Mutex<Vec<WritEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let received_clone = Arc::clone(&received);

    bus.subscribe(move |event| {
        received_clone.lock().unwrap().push(event.clone());
    });

    bus.emit(WritEvent::ConfigChanged {
        keys: vec!["theme".to_string(), "font_size".to_string()],
    });

    let events = received.lock().unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        WritEvent::ConfigChanged { keys } => {
            assert_eq!(keys, &vec!["theme".to_string(), "font_size".to_string()]);
        }
        _ => panic!("unexpected event variant"),
    }
}

#[test]
fn multiple_subscribers_all_receive() {
    let bus = EventBus::new();
    let count_a: Arc<Mutex<u32>> = Arc::new(Mutex::new(0));
    let count_b: Arc<Mutex<u32>> = Arc::new(Mutex::new(0));

    let count_a_clone = Arc::clone(&count_a);
    bus.subscribe(move |_event| {
        *count_a_clone.lock().unwrap() += 1;
    });

    let count_b_clone = Arc::clone(&count_b);
    bus.subscribe(move |_event| {
        *count_b_clone.lock().unwrap() += 1;
    });

    bus.emit(WritEvent::HotkeyToggle);

    assert_eq!(*count_a.lock().unwrap(), 1);
    assert_eq!(*count_b.lock().unwrap(), 1);
}
