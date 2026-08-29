use std::sync::LockResult;

/// Recovers a poisoned lock guard while emitting a `tracing::error`
/// so the condition is observable in user reports.
///
/// Generic over the guard, so a `Mutex`, a `RwLock` read and a `RwLock` write
/// all recover the same way and log the same line.
///
/// Recovery behavior is unchanged from the previous inline
/// `unwrap_or_else(|e| e.into_inner())` pattern: a panic that occurred
/// while another thread held the lock leaves the data in a
/// possibly-inconsistent state, but the editor continues rather than
/// cascading the panic. The added log is the contract change.
pub fn recover_poison<G>(result: LockResult<G>, location: &'static str) -> G {
    result.unwrap_or_else(|poisoned| {
        tracing::error!(location = location, "recovered poisoned lock");
        poisoned.into_inner()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex, RwLock};

    #[test]
    fn recover_poison_returns_inner_data_after_panicked_holder() {
        let mutex = Arc::new(Mutex::new(vec![1, 2, 3]));
        let mutex_clone = mutex.clone();
        let join = std::thread::spawn(move || {
            let _guard = mutex_clone.lock().unwrap();
            panic!("intentional panic to poison the mutex");
        });
        assert!(join.join().is_err());

        let guard = recover_poison(mutex.lock(), "test::poison_recovery");
        assert_eq!(*guard, vec![1, 2, 3]);
    }

    #[test]
    fn recover_poison_returns_inner_data_after_a_poisoned_read_lock() {
        let lock = Arc::new(RwLock::new(vec![1, 2, 3]));
        let clone = lock.clone();
        let join = std::thread::spawn(move || {
            let _guard = clone.write().unwrap();
            panic!("intentional panic to poison the lock");
        });
        assert!(join.join().is_err());

        let guard = recover_poison(lock.read(), "test::rwlock_read");
        assert_eq!(*guard, vec![1, 2, 3]);
    }

    #[test]
    fn recover_poison_returns_inner_data_after_a_poisoned_write_lock() {
        let lock = Arc::new(RwLock::new(String::from("~/Writ")));
        let clone = lock.clone();
        let join = std::thread::spawn(move || {
            let _guard = clone.write().unwrap();
            panic!("intentional panic to poison the lock");
        });
        assert!(join.join().is_err());

        let mut guard = recover_poison(lock.write(), "test::rwlock_write");
        *guard = String::from("~/Notes");
        assert_eq!(*guard, "~/Notes");
    }

    #[test]
    fn recover_poison_is_transparent_on_a_clean_mutex() {
        let mutex = Mutex::new(42u32);
        let guard = recover_poison(mutex.lock(), "test::clean_path");
        assert_eq!(*guard, 42);
    }
}
