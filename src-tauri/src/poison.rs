use std::sync::{LockResult, MutexGuard};

/// Recovers a poisoned mutex guard while emitting a `tracing::error`
/// so the condition is observable in user reports.
///
/// Recovery behavior is unchanged from the previous inline
/// `unwrap_or_else(|e| e.into_inner())` pattern: a panic that occurred
/// while another thread held the lock leaves the data in a
/// possibly-inconsistent state, but the editor continues rather than
/// cascading the panic. The added log is the contract change.
pub fn recover_poison<'a, T>(
    result: LockResult<MutexGuard<'a, T>>,
    location: &'static str,
) -> MutexGuard<'a, T> {
    result.unwrap_or_else(|poisoned| {
        tracing::error!(location = location, "recovered poisoned mutex");
        poisoned.into_inner()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

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
    fn recover_poison_is_transparent_on_a_clean_mutex() {
        let mutex = Mutex::new(42u32);
        let guard = recover_poison(mutex.lock(), "test::clean_path");
        assert_eq!(*guard, 42);
    }
}
