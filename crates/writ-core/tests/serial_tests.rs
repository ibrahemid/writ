//! Work under one key runs one at a time, in ticket order.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use writ_core::serial::KeyedQueue;

/// Tickets taken in order 0..n, each waited out on its own thread, the later
/// ones started first: they still run in ticket order.
#[test]
fn turns_under_one_key_run_in_ticket_order_whichever_thread_waits_first() {
    let queue = KeyedQueue::new();
    let tickets: Vec<_> = (0..8).map(|_| queue.ticket("note")).collect();
    let order = Arc::new(Mutex::new(Vec::new()));
    let mut threads = Vec::new();
    for (place, ticket) in tickets.into_iter().enumerate().rev() {
        let order = Arc::clone(&order);
        threads.push(thread::spawn(move || {
            let _turn = ticket.wait();
            order.lock().unwrap().push(place);
            thread::sleep(Duration::from_millis(2));
        }));
        thread::sleep(Duration::from_millis(1));
    }
    for t in threads {
        t.join().unwrap();
    }
    assert_eq!(*order.lock().unwrap(), (0..8).collect::<Vec<_>>());
    assert_eq!(queue.outstanding("note"), 0);
}

#[test]
fn two_turns_under_one_key_never_overlap() {
    let queue = KeyedQueue::new();
    let running = Arc::new(AtomicUsize::new(0));
    let most = Arc::new(AtomicUsize::new(0));
    let threads: Vec<_> = (0..16)
        .map(|_| {
            let queue = queue.clone();
            let running = Arc::clone(&running);
            let most = Arc::clone(&most);
            thread::spawn(move || {
                let _turn = queue.turn("note");
                let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                most.fetch_max(now, Ordering::SeqCst);
                thread::sleep(Duration::from_millis(1));
                running.fetch_sub(1, Ordering::SeqCst);
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    assert_eq!(most.load(Ordering::SeqCst), 1);
}

#[test]
fn a_turn_under_one_key_does_not_hold_up_another_key() {
    let queue = KeyedQueue::new();
    let held = queue.turn("a");
    let (done, finished) = mpsc::channel();
    let other = queue.clone();
    thread::spawn(move || {
        let _turn = other.turn("b");
        done.send(()).unwrap();
    });
    finished
        .recv_timeout(Duration::from_secs(5))
        .expect("key b ran while key a was held");
    drop(held);
}

#[test]
fn a_ticket_given_up_before_its_turn_does_not_hold_up_the_ones_after_it() {
    let queue = KeyedQueue::new();
    let first = queue.turn("note");
    let given_up = queue.ticket("note");
    let last = queue.ticket("note");
    assert_eq!(queue.outstanding("note"), 3);
    drop(given_up);
    assert_eq!(queue.outstanding("note"), 2);
    let (done, finished) = mpsc::channel();
    thread::spawn(move || {
        let _turn = last.wait();
        done.send(()).unwrap();
    });
    assert!(finished.recv_timeout(Duration::from_millis(50)).is_err());
    drop(first);
    finished
        .recv_timeout(Duration::from_secs(5))
        .expect("the last ticket ran once the first turn ended");
}

#[test]
fn a_ticket_given_up_on_its_own_turn_passes_the_turn_on() {
    let queue = KeyedQueue::new();
    let given_up = queue.ticket("note");
    let next = queue.ticket("note");
    drop(given_up);
    let (done, finished) = mpsc::channel();
    thread::spawn(move || {
        let _turn = next.wait();
        done.send(()).unwrap();
    });
    finished
        .recv_timeout(Duration::from_secs(5))
        .expect("the turn passed to the next ticket");
}

#[test]
fn a_turn_that_panics_lets_the_next_one_run() {
    let queue = KeyedQueue::new();
    let panicking = queue.clone();
    let result = thread::spawn(move || {
        let _turn = panicking.turn("note");
        panic!("the work failed");
    })
    .join();
    assert!(result.is_err());
    let (done, finished) = mpsc::channel();
    let next = queue.clone();
    thread::spawn(move || {
        let _turn = next.turn("note");
        done.send(()).unwrap();
    });
    finished
        .recv_timeout(Duration::from_secs(5))
        .expect("the key was released by the unwinding turn");
    assert_eq!(queue.outstanding("note"), 0);
}

#[test]
fn a_ticket_and_its_turn_name_their_key() {
    let queue = KeyedQueue::new();
    let ticket = queue.ticket("note-1");
    assert_eq!(ticket.key(), "note-1");
    let turn = ticket.wait();
    assert_eq!(turn.key(), "note-1");
    assert_eq!(queue.outstanding("note-1"), 1);
    drop(turn);
    assert_eq!(queue.outstanding("note-1"), 0);
}
