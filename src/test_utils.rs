//! Test utilities shared across modules.
//!
//! This module provides common utilities for tests, including
//! synchronization primitives for tests that modify global state.

use std::sync::Mutex;

/// Mutex to serialize tests that depend on or change the current working directory.
///
/// Tests that either:
/// - Change the current working directory (e.g., to test non-git scenarios)
/// - Depend on the current working directory being a git repo
///
/// must acquire this mutex to prevent race conditions during parallel test execution.
///
/// # Example
///
/// ```ignore
/// use crate::test_utils::CWD_MUTEX;
///
/// #[test]
/// fn test_that_changes_cwd() {
///     let _lock = CWD_MUTEX.lock().unwrap();
///     // ... test code that changes or depends on cwd ...
/// }
/// ```
pub static CWD_MUTEX: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cwd_mutex_can_be_acquired() {
        let lock = CWD_MUTEX.lock();
        assert!(lock.is_ok());
    }

    #[test]
    fn test_cwd_mutex_can_be_acquired_multiple_times_sequentially() {
        {
            let _lock = CWD_MUTEX.lock().unwrap();
        }
        {
            let _lock = CWD_MUTEX.lock().unwrap();
        }
    }
}
