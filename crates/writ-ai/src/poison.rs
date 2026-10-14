//! Lock recovery that says it happened.

use std::sync::LockResult;

/// Recovers a poisoned lock guard while emitting a `tracing::error`, so the
/// condition is observable in user reports. The same rule as the app shell's:
/// a panic that occurred while another thread held the lock leaves the data
/// possibly inconsistent, but the editor continues rather than cascading it.
pub(crate) fn recover_poison<G>(result: LockResult<G>, location: &'static str) -> G {
    result.unwrap_or_else(|poisoned| {
        tracing::error!(location = location, "recovered poisoned lock");
        poisoned.into_inner()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn a_poisoned_lock_hands_back_its_data() {
        let mutex = Arc::new(Mutex::new(vec![1, 2, 3]));
        let held = mutex.clone();
        let join = std::thread::spawn(move || {
            let _guard = held.lock().unwrap();
            panic!("intentional panic to poison the mutex");
        });
        assert!(join.join().is_err());
        assert_eq!(*recover_poison(mutex.lock(), "test"), vec![1, 2, 3]);
    }
}
