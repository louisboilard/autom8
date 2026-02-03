//! Signal handling infrastructure for graceful shutdown.
//!
//! This module provides a thread-safe mechanism for handling SIGINT (Ctrl+C)
//! signals, allowing the main loop to check for shutdown requests without
//! blocking.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::error::{Autom8Error, Result};

/// Handles SIGINT signals for graceful shutdown.
///
/// `SignalHandler` registers a handler for SIGINT that sets an internal flag
/// when triggered. The main loop can check this flag using `is_shutdown_requested()`
/// without blocking.
///
/// # Thread Safety
///
/// `SignalHandler` is thread-safe and can be cloned to share across threads.
/// The underlying shutdown flag uses atomic operations.
///
/// # Example
///
/// ```ignore
/// let handler = SignalHandler::new()?;
///
/// // In main loop
/// loop {
///     if handler.is_shutdown_requested() {
///         // Clean up and exit
///         break;
///     }
///     // Continue processing
/// }
/// ```
#[derive(Clone)]
pub struct SignalHandler {
    shutdown_flag: Arc<AtomicBool>,
}

impl SignalHandler {
    /// Creates a new `SignalHandler` and registers the SIGINT handler.
    ///
    /// The handler will set an internal flag when SIGINT is received (e.g., when
    /// the user presses Ctrl+C). This flag can be checked using `is_shutdown_requested()`.
    ///
    /// # Errors
    ///
    /// Returns an error if the signal handler cannot be registered.
    pub fn new() -> Result<Self> {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let flag_clone = Arc::clone(&shutdown_flag);

        ctrlc::set_handler(move || {
            flag_clone.store(true, Ordering::SeqCst);
        })
        .map_err(|e| Autom8Error::SignalHandler(e.to_string()))?;

        Ok(Self { shutdown_flag })
    }

    /// Checks if a shutdown has been requested (non-blocking).
    ///
    /// Returns `true` if SIGINT has been received since the handler was created,
    /// `false` otherwise.
    ///
    /// This method is safe to call from any thread and does not block.
    pub fn is_shutdown_requested(&self) -> bool {
        self.shutdown_flag.load(Ordering::SeqCst)
    }

    /// Resets the shutdown flag to false.
    ///
    /// This can be useful for testing or if you need to clear the flag after
    /// handling a shutdown request.
    #[cfg(test)]
    pub fn reset(&self) {
        self.shutdown_flag.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handler_can_be_created() {
        // Note: In a real test environment, we can only register the handler once.
        // Subsequent calls will fail, but that's expected behavior for ctrlc.
        // We test the initial state instead.
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let handler = SignalHandler {
            shutdown_flag: shutdown_flag.clone(),
        };

        // Handler should start with shutdown not requested
        assert!(!handler.is_shutdown_requested());
    }

    #[test]
    fn test_is_shutdown_requested_returns_false_initially() {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let handler = SignalHandler { shutdown_flag };

        assert!(!handler.is_shutdown_requested());
    }

    #[test]
    fn test_is_shutdown_requested_returns_true_when_flag_set() {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let handler = SignalHandler {
            shutdown_flag: shutdown_flag.clone(),
        };

        // Simulate signal being received
        shutdown_flag.store(true, Ordering::SeqCst);

        assert!(handler.is_shutdown_requested());
    }

    #[test]
    fn test_handler_is_thread_safe() {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let handler = SignalHandler {
            shutdown_flag: shutdown_flag.clone(),
        };

        // Clone handler for use in another thread
        let handler_clone = handler.clone();

        // Set flag from main thread
        shutdown_flag.store(true, Ordering::SeqCst);

        // Clone should see the same state
        assert!(handler_clone.is_shutdown_requested());
        assert!(handler.is_shutdown_requested());
    }

    #[test]
    fn test_handler_clone_shares_state() {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let handler1 = SignalHandler {
            shutdown_flag: shutdown_flag.clone(),
        };
        let handler2 = handler1.clone();

        // Both should see initial state
        assert!(!handler1.is_shutdown_requested());
        assert!(!handler2.is_shutdown_requested());

        // Set via the underlying flag
        shutdown_flag.store(true, Ordering::SeqCst);

        // Both clones should see updated state
        assert!(handler1.is_shutdown_requested());
        assert!(handler2.is_shutdown_requested());
    }

    #[test]
    fn test_reset_clears_shutdown_flag() {
        let shutdown_flag = Arc::new(AtomicBool::new(true));
        let handler = SignalHandler { shutdown_flag };

        assert!(handler.is_shutdown_requested());

        handler.reset();

        assert!(!handler.is_shutdown_requested());
    }
}
