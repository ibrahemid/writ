//! Files opened from the OS while the frontend reports ready are opened, not
//! parked after the drain (TS-21): the ready flag is flipped and read under
//! the `pending_opens` lock.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Barrier, Mutex};
use std::thread;

use writ_tauri_lib::startup::{mark_ready_and_drain, park_or_pass_opens};

#[test]
fn files_opened_before_ready_wait_for_the_drain() {
    let pending = Mutex::new(Vec::new());
    let ready = AtomicBool::new(false);

    assert_eq!(
        park_or_pass_opens(&pending, &ready, vec!["a.md".into()]),
        None
    );
    assert_eq!(
        park_or_pass_opens(&pending, &ready, vec!["b.md".into()]),
        None
    );

    assert_eq!(mark_ready_and_drain(&pending, &ready), vec!["a.md", "b.md"]);
    assert!(pending.lock().unwrap().is_empty());
}

#[test]
fn files_opened_after_ready_go_straight_through() {
    let pending = Mutex::new(Vec::new());
    let ready = AtomicBool::new(false);
    assert!(mark_ready_and_drain(&pending, &ready).is_empty());

    assert_eq!(
        park_or_pass_opens(&pending, &ready, vec!["c.md".into()]),
        Some(vec!["c.md".to_string()])
    );
    assert!(pending.lock().unwrap().is_empty());
}

#[test]
fn an_open_racing_the_ready_signal_is_never_left_parked() {
    for round in 0..2000 {
        let pending = Arc::new(Mutex::new(Vec::new()));
        let ready = Arc::new(AtomicBool::new(false));
        let start = Arc::new(Barrier::new(2));

        let opener = {
            let (pending, ready, start) = (pending.clone(), ready.clone(), start.clone());
            thread::spawn(move || {
                start.wait();
                park_or_pass_opens(&pending, &ready, vec![format!("{round}.md")])
            })
        };
        let drainer = {
            let (pending, ready, start) = (pending.clone(), ready.clone(), start.clone());
            thread::spawn(move || {
                start.wait();
                mark_ready_and_drain(&pending, &ready)
            })
        };

        let passed = opener.join().unwrap().unwrap_or_default();
        let drained = drainer.join().unwrap();
        assert_eq!(
            passed.len() + drained.len(),
            1,
            "round {round} lost the file"
        );
        assert!(
            pending.lock().unwrap().is_empty(),
            "round {round} parked the file"
        );
    }
}
