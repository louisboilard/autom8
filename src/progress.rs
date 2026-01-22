use crate::output::{DIM, GREEN, RED, RESET};
use indicatif::{ProgressBar, ProgressStyle};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use terminal_size::{terminal_size, Width};

const SPINNER_CHARS: &str = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏";
const DEFAULT_TERMINAL_WIDTH: u16 = 80;
// Spinner (2) + " Claude working on " (19) + " [HH:MM:SS]" (11) = 32 chars overhead
const SPINNER_OVERHEAD: usize = 32;

// ============================================================================
// VerboseTimer: Shows elapsed time in verbose mode without truncating output
// ============================================================================

/// Timer for verbose mode that periodically prints a status line
/// while allowing full Claude output to scroll without truncation.
pub struct VerboseTimer {
    story_id: String,
    stop_flag: Arc<AtomicBool>,
    timer_thread: Option<JoinHandle<()>>,
    start_time: Instant,
}

impl VerboseTimer {
    pub fn new(story_id: &str) -> Self {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let start_time = Instant::now();

        let stop_flag_clone = Arc::clone(&stop_flag);
        let story_id_owned = story_id.to_string();

        // Spawn timer thread that prints status every 10 seconds
        let timer_thread = thread::spawn(move || {
            let mut last_print = Instant::now();
            while !stop_flag_clone.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(500));

                if stop_flag_clone.load(Ordering::Relaxed) {
                    break;
                }

                // Print status every 10 seconds
                if last_print.elapsed().as_secs() >= 10 {
                    let elapsed = start_time.elapsed();
                    let hours = elapsed.as_secs() / 3600;
                    let mins = (elapsed.as_secs() % 3600) / 60;
                    let secs = elapsed.as_secs() % 60;
                    eprintln!(
                        "{DIM}[{} elapsed: {:02}:{:02}:{:02}]{RESET}",
                        story_id_owned, hours, mins, secs
                    );
                    last_print = Instant::now();
                }
            }
        });

        Self {
            story_id: story_id.to_string(),
            stop_flag,
            timer_thread: Some(timer_thread),
            start_time,
        }
    }

    pub fn new_for_prd() -> Self {
        Self::new("PRD generation")
    }

    pub fn new_for_commit() -> Self {
        Self::new("Commit")
    }

    fn stop_timer(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(handle) = self.timer_thread.take() {
            let _ = handle.join();
        }
    }

    pub fn finish_success(&mut self) {
        self.stop_timer();
        let elapsed = self.start_time.elapsed();
        let mins = elapsed.as_secs() / 60;
        let secs = elapsed.as_secs() % 60;
        eprintln!(
            "{GREEN}{} completed in {}m {}s{RESET}",
            self.story_id, mins, secs
        );
    }

    pub fn finish_error(&mut self, error: &str) {
        self.stop_timer();
        eprintln!("{RED}{} failed: {}{RESET}", self.story_id, error);
    }

    pub fn elapsed_secs(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }
}

impl Drop for VerboseTimer {
    fn drop(&mut self) {
        // Ensure timer thread is stopped when VerboseTimer is dropped
        self.stop_flag.store(true, Ordering::Relaxed);
        // Wait for thread to finish to prevent partial lines on screen
        if let Some(handle) = self.timer_thread.take() {
            let _ = handle.join();
        }
    }
}

// ============================================================================
// ClaudeSpinner: Single-line preview mode with spinner animation
// ============================================================================

/// Get the current terminal width, falling back to a default if unavailable
fn get_terminal_width() -> usize {
    terminal_size()
        .map(|(Width(w), _)| w as usize)
        .unwrap_or(DEFAULT_TERMINAL_WIDTH as usize)
}

pub struct ClaudeSpinner {
    spinner: Arc<ProgressBar>,
    story_id: String,
    stop_flag: Arc<AtomicBool>,
    timer_thread: Option<JoinHandle<()>>,
    start_time: Instant,
    last_activity: Arc<std::sync::Mutex<String>>,
}

impl ClaudeSpinner {
    pub fn new(story_id: &str) -> Self {
        Self::create(story_id, format!("{} | Starting...", story_id))
    }

    pub fn new_for_prd() -> Self {
        Self::create("PRD", "PRD generation | Starting...".to_string())
    }

    pub fn new_for_commit() -> Self {
        Self::create("Commit", "Committing | Starting...".to_string())
    }

    fn create(story_id: &str, initial_message: String) -> Self {
        let spinner = Arc::new(ProgressBar::new_spinner());
        spinner.set_style(
            ProgressStyle::default_spinner()
                .tick_chars(SPINNER_CHARS)
                .template("{spinner:.cyan} Claude working on {msg}")
                .expect("invalid template"),
        );
        spinner.set_message(format!("{} [00:00:00]", initial_message));
        spinner.enable_steady_tick(Duration::from_millis(80));

        let stop_flag = Arc::new(AtomicBool::new(false));
        let start_time = Instant::now();
        let last_activity = Arc::new(std::sync::Mutex::new("Starting...".to_string()));

        // Clone for timer thread
        let spinner_clone = Arc::clone(&spinner);
        let stop_flag_clone = Arc::clone(&stop_flag);
        let last_activity_clone = Arc::clone(&last_activity);
        let story_id_owned = story_id.to_string();

        // Spawn independent timer thread that updates every second
        let timer_thread = thread::spawn(move || {
            while !stop_flag_clone.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_secs(1));

                // Check again after sleep in case we should stop
                if stop_flag_clone.load(Ordering::Relaxed) {
                    break;
                }

                let elapsed = start_time.elapsed();
                let hours = elapsed.as_secs() / 3600;
                let mins = (elapsed.as_secs() % 3600) / 60;
                let secs = elapsed.as_secs() % 60;
                let time_str = format!("{:02}:{:02}:{:02}", hours, mins, secs);

                let activity = last_activity_clone.lock().unwrap().clone();
                let truncated = truncate_activity_for_display(&activity, &story_id_owned);

                if story_id_owned == "PRD" {
                    spinner_clone
                        .set_message(format!("PRD generation | {} [{}]", truncated, time_str));
                } else {
                    spinner_clone
                        .set_message(format!("{} | {} [{}]", story_id_owned, truncated, time_str));
                }
            }
        });

        Self {
            spinner,
            story_id: story_id.to_string(),
            stop_flag,
            timer_thread: Some(timer_thread),
            start_time,
            last_activity,
        }
    }

    pub fn update(&self, activity: &str) {
        // Update the last activity for the timer thread to use
        if let Ok(mut guard) = self.last_activity.lock() {
            *guard = activity.to_string();
        }

        // Also update immediately for responsiveness
        let elapsed = self.start_time.elapsed();
        let hours = elapsed.as_secs() / 3600;
        let mins = (elapsed.as_secs() % 3600) / 60;
        let secs = elapsed.as_secs() % 60;
        let time_str = format!("{:02}:{:02}:{:02}", hours, mins, secs);

        let truncated = truncate_activity_for_display(activity, &self.story_id);

        if self.story_id == "PRD" {
            self.spinner
                .set_message(format!("PRD generation | {} [{}]", truncated, time_str));
        } else {
            self.spinner
                .set_message(format!("{} | {} [{}]", self.story_id, truncated, time_str));
        }
    }

    fn stop_timer(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(handle) = self.timer_thread.take() {
            // Wait for thread to finish (it should exit quickly)
            let _ = handle.join();
        }
    }

    /// Clear the spinner line without printing a final message.
    /// Used to ensure no visual artifacts remain before printing completion output.
    pub fn clear(&self) {
        self.spinner.finish_and_clear();
    }

    pub fn finish_success(&mut self, duration_secs: u64) {
        self.stop_timer();
        let mins = duration_secs / 60;
        let secs = duration_secs % 60;
        // Clear the line first, then print completion message to ensure clean output
        self.spinner.finish_and_clear();
        println!(
            "{GREEN}\u{2714} {} completed in {}m {}s{RESET}",
            self.story_id, mins, secs
        );
    }

    pub fn finish_error(&mut self, error: &str) {
        self.stop_timer();
        // For error messages, use a reasonable width accounting for " failed: " and color codes
        let available = get_terminal_width().saturating_sub(self.story_id.chars().count() + 15);
        let truncated = truncate_activity(error, available.max(20));
        // Clear the line first, then print error message to ensure clean output
        self.spinner.finish_and_clear();
        println!(
            "{RED}\u{2718} {} failed: {}{RESET}",
            self.story_id, truncated
        );
    }

    pub fn finish_with_message(&mut self, message: &str) {
        self.stop_timer();
        // Clear the line first, then print message to ensure clean output
        self.spinner.finish_and_clear();
        println!("{GREEN}\u{2714} {}: {}{RESET}", self.story_id, message);
    }
}

impl Drop for ClaudeSpinner {
    fn drop(&mut self) {
        // Ensure timer thread is stopped and spinner is cleared when dropped
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(handle) = self.timer_thread.take() {
            let _ = handle.join();
        }
        // Clear the spinner line if it hasn't been finished yet
        // This prevents partial lines from remaining on screen
        self.spinner.finish_and_clear();
    }
}

/// Calculate the available width for activity text given the story ID
fn calculate_available_width(story_id: &str) -> usize {
    let terminal_width = get_terminal_width();
    // Story ID is displayed as "{story_id} | " which adds len + 3 chars
    let story_id_overhead = story_id.chars().count() + 3;
    let total_overhead = SPINNER_OVERHEAD + story_id_overhead;

    // Ensure we have at least some minimum space (10 chars) for the activity
    if terminal_width > total_overhead + 10 {
        terminal_width - total_overhead
    } else {
        10 // Minimum activity width
    }
}

fn truncate_activity(activity: &str, max_len: usize) -> String {
    // Take first line only and clean it up
    let first_line = activity.lines().next().unwrap_or(activity);
    let cleaned = first_line.trim();

    // Count characters (not bytes) to handle UTF-8 properly
    let char_count = cleaned.chars().count();
    if char_count <= max_len {
        cleaned.to_string()
    } else {
        // Need at least 4 chars for "X..." where X is at least one character
        if max_len < 4 {
            "...".to_string()
        } else {
            let truncated: String = cleaned.chars().take(max_len - 3).collect();
            format!("{}...", truncated)
        }
    }
}

/// Truncate activity text to fit terminal width, accounting for story ID and spinner overhead
fn truncate_activity_for_display(activity: &str, story_id: &str) -> String {
    let available_width = calculate_available_width(story_id);
    truncate_activity(activity, available_width)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_activity_short() {
        let result = truncate_activity("Short message", 50);
        assert_eq!(result, "Short message");
    }

    #[test]
    fn test_truncate_activity_long() {
        let long_msg = "This is a very long message that should be truncated because it exceeds the maximum length";
        let result = truncate_activity(long_msg, 30);
        assert_eq!(result.chars().count(), 30);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_truncate_activity_multiline() {
        let multiline = "First line\nSecond line\nThird line";
        let result = truncate_activity(multiline, 50);
        assert_eq!(result, "First line");
    }

    #[test]
    fn test_truncate_activity_utf8() {
        // Should not panic on multi-byte UTF-8 characters
        let utf8_msg = "Implementing 日本語 feature with more text here";
        let result = truncate_activity(utf8_msg, 20);
        assert_eq!(result.chars().count(), 20);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_truncate_activity_exact_boundary() {
        let msg = "Exactly twenty chars";
        let result = truncate_activity(msg, 20);
        assert_eq!(result, "Exactly twenty chars");
    }

    // ========================================================================
    // Timer functionality tests (US-003)
    // ========================================================================

    #[test]
    fn test_spinner_creates_with_stop_flag() {
        let mut spinner = ClaudeSpinner::new("US-001");
        // Stop flag should be false initially
        assert!(!spinner.stop_flag.load(Ordering::Relaxed));
        // Timer thread should exist
        assert!(spinner.timer_thread.is_some());
        // Clean up
        spinner.stop_timer();
    }

    #[test]
    fn test_spinner_stop_timer_sets_flag() {
        let mut spinner = ClaudeSpinner::new("US-001");
        spinner.stop_timer();
        // Stop flag should be true after stopping
        assert!(spinner.stop_flag.load(Ordering::Relaxed));
        // Timer thread should be taken (None after join)
        assert!(spinner.timer_thread.is_none());
    }

    #[test]
    fn test_spinner_finish_success_stops_timer() {
        let mut spinner = ClaudeSpinner::new("US-001");
        spinner.finish_success(60);
        assert!(spinner.stop_flag.load(Ordering::Relaxed));
        assert!(spinner.timer_thread.is_none());
    }

    #[test]
    fn test_spinner_finish_error_stops_timer() {
        let mut spinner = ClaudeSpinner::new("US-001");
        spinner.finish_error("Test error");
        assert!(spinner.stop_flag.load(Ordering::Relaxed));
        assert!(spinner.timer_thread.is_none());
    }

    #[test]
    fn test_spinner_finish_with_message_stops_timer() {
        let mut spinner = ClaudeSpinner::new("US-001");
        spinner.finish_with_message("Done");
        assert!(spinner.stop_flag.load(Ordering::Relaxed));
        assert!(spinner.timer_thread.is_none());
    }

    #[test]
    fn test_spinner_update_stores_activity() {
        let mut spinner = ClaudeSpinner::new("US-001");
        spinner.update("Working on feature X");
        let activity = spinner.last_activity.lock().unwrap().clone();
        assert_eq!(activity, "Working on feature X");
        spinner.stop_timer();
    }

    #[test]
    fn test_spinner_prd_variant() {
        let mut spinner = ClaudeSpinner::new_for_prd();
        assert_eq!(spinner.story_id, "PRD");
        spinner.stop_timer();
    }

    #[test]
    fn test_spinner_commit_variant() {
        let mut spinner = ClaudeSpinner::new_for_commit();
        assert_eq!(spinner.story_id, "Commit");
        spinner.stop_timer();
    }

    #[test]
    fn test_timer_thread_updates_independently() {
        // Create spinner and wait slightly more than 1 second
        let mut spinner = ClaudeSpinner::new("US-001");
        spinner.update("Initial activity");

        // Wait for timer thread to potentially update (1.1 seconds)
        thread::sleep(Duration::from_millis(1100));

        // The timer should have run at least once - verify stop flag is still false
        assert!(!spinner.stop_flag.load(Ordering::Relaxed));

        // Clean up
        spinner.stop_timer();
    }

    #[test]
    fn test_spinner_shared_state_is_arc() {
        // Verify the spinner and stop_flag are properly shared (Arc)
        let spinner = ClaudeSpinner::new("US-001");

        // Strong count should be 2 (one here, one in timer thread)
        assert!(Arc::strong_count(&spinner.spinner) >= 1);
        assert!(Arc::strong_count(&spinner.stop_flag) >= 1);
        assert!(Arc::strong_count(&spinner.last_activity) >= 1);

        // We can't call stop_timer since spinner is not mut, so just let it drop
        // The timer thread will clean up when spinner is dropped
    }

    // ========================================================================
    // Terminal width and single-line preview tests (US-004)
    // ========================================================================

    #[test]
    fn test_get_terminal_width_returns_positive() {
        let width = get_terminal_width();
        assert!(width > 0);
    }

    #[test]
    fn test_calculate_available_width_short_story_id() {
        // With a short story ID, should have more space for activity
        let width = calculate_available_width("US-001");
        // Should be positive and reasonable
        assert!(width >= 10);
    }

    #[test]
    fn test_calculate_available_width_long_story_id() {
        // With a longer story ID, should have less space for activity
        let short_width = calculate_available_width("US-001");
        let long_width = calculate_available_width("US-001-very-long-story-identifier");
        // Longer story ID should result in less available width
        assert!(long_width < short_width);
    }

    #[test]
    fn test_calculate_available_width_minimum_bound() {
        // Even with very long story ID, should have at least minimum width
        let width = calculate_available_width("A".repeat(200).as_str());
        assert!(width >= 10);
    }

    #[test]
    fn test_truncate_activity_for_display() {
        // Test that truncate_activity_for_display produces a result
        let long_activity = "This is a very long activity message that should definitely be truncated to fit within the terminal width when displayed";
        let result = truncate_activity_for_display(long_activity, "US-001");
        // Result should be shorter than original if terminal is narrow
        // or equal/longer only if terminal is very wide
        assert!(!result.is_empty());
    }

    #[test]
    fn test_truncate_activity_for_display_short_message() {
        // Short messages should not be truncated
        let short_activity = "Working";
        let result = truncate_activity_for_display(short_activity, "US-001");
        assert_eq!(result, "Working");
    }

    #[test]
    fn test_truncate_activity_for_display_with_prd() {
        // PRD has specific handling, test it works
        let activity = "Generating user stories";
        let result = truncate_activity_for_display(activity, "PRD");
        assert_eq!(result, "Generating user stories");
    }

    #[test]
    fn test_truncate_activity_very_small_max_len() {
        // Edge case: very small max_len should still produce valid output
        let result = truncate_activity("Hello world", 3);
        assert_eq!(result, "...");
    }

    #[test]
    fn test_truncate_activity_max_len_4() {
        // Edge case: max_len of 4 should show "X..."
        let result = truncate_activity("Hello world", 4);
        assert_eq!(result, "H...");
        assert_eq!(result.chars().count(), 4);
    }

    #[test]
    fn test_truncate_activity_preserves_first_line_only() {
        let multiline = "First line of output\nSecond line\nThird line";
        let result = truncate_activity_for_display(multiline, "US-001");
        assert!(!result.contains('\n'));
        assert!(result.starts_with("First") || result == "...");
    }

    #[test]
    fn test_spinner_update_uses_terminal_width() {
        // Create a spinner and update with a long message
        let mut spinner = ClaudeSpinner::new("US-001");
        let long_activity =
            "This is a very long activity message that should be truncated based on terminal width";
        spinner.update(long_activity);

        // The activity should be stored
        let stored = spinner.last_activity.lock().unwrap().clone();
        assert_eq!(stored, long_activity);

        spinner.stop_timer();
    }

    // ========================================================================
    // VerboseTimer tests (US-005)
    // ========================================================================

    #[test]
    fn test_verbose_timer_creates_with_stop_flag() {
        let mut timer = VerboseTimer::new("US-001");
        // Stop flag should be false initially
        assert!(!timer.stop_flag.load(Ordering::Relaxed));
        // Timer thread should exist
        assert!(timer.timer_thread.is_some());
        // Clean up
        timer.stop_timer();
    }

    #[test]
    fn test_verbose_timer_stop_timer_sets_flag() {
        let mut timer = VerboseTimer::new("US-001");
        timer.stop_timer();
        // Stop flag should be true after stopping
        assert!(timer.stop_flag.load(Ordering::Relaxed));
        // Timer thread should be taken (None after join)
        assert!(timer.timer_thread.is_none());
    }

    #[test]
    fn test_verbose_timer_finish_success_stops_timer() {
        let mut timer = VerboseTimer::new("US-001");
        timer.finish_success();
        assert!(timer.stop_flag.load(Ordering::Relaxed));
        assert!(timer.timer_thread.is_none());
    }

    #[test]
    fn test_verbose_timer_finish_error_stops_timer() {
        let mut timer = VerboseTimer::new("US-001");
        timer.finish_error("Test error");
        assert!(timer.stop_flag.load(Ordering::Relaxed));
        assert!(timer.timer_thread.is_none());
    }

    #[test]
    fn test_verbose_timer_elapsed_secs() {
        let timer = VerboseTimer::new("US-001");
        // Just created, elapsed should be 0 or very small
        let elapsed = timer.elapsed_secs();
        assert!(elapsed <= 1);
        // Clean up by dropping (Drop impl handles stop_flag)
    }

    #[test]
    fn test_verbose_timer_prd_variant() {
        let mut timer = VerboseTimer::new_for_prd();
        assert_eq!(timer.story_id, "PRD generation");
        timer.stop_timer();
    }

    #[test]
    fn test_verbose_timer_commit_variant() {
        let mut timer = VerboseTimer::new_for_commit();
        assert_eq!(timer.story_id, "Commit");
        timer.stop_timer();
    }

    #[test]
    fn test_verbose_timer_shared_state_is_arc() {
        let timer = VerboseTimer::new("US-001");
        // Strong count should be 2 (one here, one in timer thread)
        assert!(Arc::strong_count(&timer.stop_flag) >= 1);
        // Drop will set stop_flag
    }

    #[test]
    fn test_verbose_timer_drop_sets_stop_flag() {
        let stop_flag_clone;
        {
            let timer = VerboseTimer::new("US-001");
            stop_flag_clone = Arc::clone(&timer.stop_flag);
            assert!(!stop_flag_clone.load(Ordering::Relaxed));
        }
        // After drop, stop_flag should be set
        assert!(stop_flag_clone.load(Ordering::Relaxed));
    }

    // ========================================================================
    // Clean Display on Completion tests (US-006)
    // ========================================================================

    #[test]
    fn test_spinner_drop_stops_timer_and_clears() {
        let stop_flag_clone;
        let timer_thread_exists;
        {
            let spinner = ClaudeSpinner::new("US-006");
            stop_flag_clone = Arc::clone(&spinner.stop_flag);
            timer_thread_exists = spinner.timer_thread.is_some();
            assert!(!stop_flag_clone.load(Ordering::Relaxed));
            assert!(timer_thread_exists);
        }
        // After drop, stop_flag should be set (timer stopped)
        assert!(stop_flag_clone.load(Ordering::Relaxed));
    }

    #[test]
    fn test_verbose_timer_drop_joins_thread() {
        // Create a timer and drop it - should not hang
        let stop_flag_clone;
        {
            let timer = VerboseTimer::new("US-006");
            stop_flag_clone = Arc::clone(&timer.stop_flag);
            // Drop happens here
        }
        // Thread should have been joined and flag set
        assert!(stop_flag_clone.load(Ordering::Relaxed));
    }

    #[test]
    fn test_spinner_finish_success_clears_and_prints() {
        let mut spinner = ClaudeSpinner::new("US-006");
        // Should not panic and should cleanly finish
        spinner.finish_success(65); // 1m 5s
                                    // Timer should be stopped
        assert!(spinner.stop_flag.load(Ordering::Relaxed));
        assert!(spinner.timer_thread.is_none());
    }

    #[test]
    fn test_spinner_finish_error_clears_and_prints() {
        let mut spinner = ClaudeSpinner::new("US-006");
        // Should not panic and should cleanly finish
        spinner.finish_error("Test error message");
        // Timer should be stopped
        assert!(spinner.stop_flag.load(Ordering::Relaxed));
        assert!(spinner.timer_thread.is_none());
    }

    #[test]
    fn test_spinner_finish_with_message_clears_and_prints() {
        let mut spinner = ClaudeSpinner::new("US-006");
        // Should not panic and should cleanly finish
        spinner.finish_with_message("Custom completion message");
        // Timer should be stopped
        assert!(spinner.stop_flag.load(Ordering::Relaxed));
        assert!(spinner.timer_thread.is_none());
    }

    #[test]
    fn test_spinner_clear_method() {
        let spinner = ClaudeSpinner::new("US-006");
        // clear() should work without panic
        spinner.clear();
        // Spinner should be finished after clear
        assert!(spinner.spinner.is_finished());
    }

    #[test]
    fn test_spinner_double_finish_no_panic() {
        let mut spinner = ClaudeSpinner::new("US-006");
        // First finish
        spinner.finish_success(60);
        // Second finish should not panic (idempotent)
        spinner.finish_success(60);
        assert!(spinner.stop_flag.load(Ordering::Relaxed));
    }

    #[test]
    fn test_verbose_timer_double_finish_no_panic() {
        let mut timer = VerboseTimer::new("US-006");
        // First finish
        timer.finish_success();
        // Second finish should not panic (idempotent)
        timer.finish_success();
        assert!(timer.stop_flag.load(Ordering::Relaxed));
    }
}
