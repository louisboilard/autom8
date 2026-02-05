use crate::output::{DIM, GRAY, GREEN, RED, RESET, YELLOW};
use indicatif::{ProgressBar, ProgressStyle};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use terminal_size::{terminal_size, Width};

const SPINNER_CHARS: &str = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏";
const DEFAULT_TERMINAL_WIDTH: u16 = 80;
/// Fixed width for activity text to prevent timer position jumping (US-002)
pub const ACTIVITY_TEXT_WIDTH: usize = 40;

// ============================================================================
// AgentDisplay: Unified trait/interface for all agent displays
// ============================================================================

/// Information about the current iteration or progress context
#[derive(Debug, Clone, Default)]
pub struct IterationInfo {
    /// Current iteration number (1-indexed)
    pub current: Option<u32>,
    /// Total number of iterations (if known)
    pub total: Option<u32>,
    /// Phase identifier (e.g., "Review", "Correct", "Commit")
    pub phase: Option<String>,
}

// ============================================================================
// ProgressContext: Overall progress context for unified display (US-010)
// ============================================================================

/// Overall progress context holding story progress and current phase information.
///
/// This struct tracks the current story position within the total stories,
/// and can be combined with iteration info for dual-context display.
///
/// # Display Format
/// During review/correct, shows both story progress and iteration:
/// `[US-001 2/5 | Review 1/3]`
#[derive(Debug, Clone, Default)]
pub struct ProgressContext {
    /// Current story index (1-indexed)
    pub story_index: Option<u32>,
    /// Total number of stories
    pub total_stories: Option<u32>,
    /// Current story ID (e.g., "US-001")
    pub story_id: Option<String>,
    /// Current phase name
    pub current_phase: Option<String>,
}

impl ProgressContext {
    /// Create a new ProgressContext with story progress
    pub fn new(story_id: &str, story_index: u32, total_stories: u32) -> Self {
        Self {
            story_index: Some(story_index),
            total_stories: Some(total_stories),
            story_id: Some(story_id.to_string()),
            current_phase: None,
        }
    }

    /// Create a ProgressContext with phase information
    pub fn with_phase(story_id: &str, story_index: u32, total_stories: u32, phase: &str) -> Self {
        Self {
            story_index: Some(story_index),
            total_stories: Some(total_stories),
            story_id: Some(story_id.to_string()),
            current_phase: Some(phase.to_string()),
        }
    }

    /// Set the current phase
    pub fn set_phase(&mut self, phase: &str) {
        self.current_phase = Some(phase.to_string());
    }

    /// Format the story progress part: `[US-001 2/5]`
    pub fn format_story_progress(&self) -> Option<String> {
        match (&self.story_id, self.story_index, self.total_stories) {
            (Some(id), Some(idx), Some(total)) => Some(format!("[{} {}/{}]", id, idx, total)),
            _ => None,
        }
    }

    /// Format a dual-context display combining story progress and iteration info.
    ///
    /// Returns format like `[US-001 2/5 | Review 1/3]` when both contexts are present,
    /// or just the story progress or iteration info if only one is available.
    pub fn format_dual_context(&self, iteration_info: &Option<IterationInfo>) -> Option<String> {
        let story_part = self.format_story_progress();
        let iter_part = iteration_info.as_ref().and_then(|info| info.format());

        match (story_part, iter_part) {
            (Some(story), Some(iter)) => {
                // Remove brackets and combine: "[US-001 2/5]" + "[Review 1/3]" -> "[US-001 2/5 | Review 1/3]"
                let story_inner = story.trim_start_matches('[').trim_end_matches(']');
                let iter_inner = iter.trim_start_matches('[').trim_end_matches(']');
                Some(format!("[{} | {}]", story_inner, iter_inner))
            }
            (Some(story), None) => Some(story),
            (None, Some(iter)) => Some(iter),
            (None, None) => None,
        }
    }
}

impl IterationInfo {
    /// Create a new IterationInfo with current and total iteration counts
    pub fn new(current: u32, total: u32) -> Self {
        Self {
            current: Some(current),
            total: Some(total),
            phase: None,
        }
    }

    /// Create a new IterationInfo with phase and iteration counts
    pub fn with_phase(phase: &str, current: u32, total: u32) -> Self {
        Self {
            current: Some(current),
            total: Some(total),
            phase: Some(phase.to_string()),
        }
    }

    /// Create an IterationInfo with just a phase name (no iteration counts)
    pub fn phase_only(phase: &str) -> Self {
        Self {
            current: None,
            total: None,
            phase: Some(phase.to_string()),
        }
    }

    /// Format the iteration info as a string for display
    /// Returns format like "[Review 1/3]" or "[Commit]" or "[2/5]"
    pub fn format(&self) -> Option<String> {
        match (&self.phase, self.current, self.total) {
            (Some(phase), Some(curr), Some(tot)) => Some(format!("[{} {}/{}]", phase, curr, tot)),
            (Some(phase), None, None) => Some(format!("[{}]", phase)),
            (None, Some(curr), Some(tot)) => Some(format!("[{}/{}]", curr, tot)),
            _ => None,
        }
    }
}

/// Outcome information for agent completion
#[derive(Debug, Clone)]
pub struct Outcome {
    /// Whether the operation was successful
    pub success: bool,
    /// Brief description of the outcome (e.g., "3 issues found", "abc1234")
    pub message: String,
    /// Optional token count to display (always shown if present, not gated by verbose)
    pub tokens: Option<u64>,
}

impl Outcome {
    /// Create a successful outcome with a message
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: message.into(),
            tokens: None,
        }
    }

    /// Create a failed outcome with an error message
    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: message.into(),
            tokens: None,
        }
    }

    /// Add token count to this outcome
    pub fn with_tokens(mut self, tokens: u64) -> Self {
        self.tokens = Some(tokens);
        self
    }

    /// Add optional token count to this outcome (no-op if None)
    pub fn with_optional_tokens(mut self, tokens: Option<u64>) -> Self {
        self.tokens = tokens;
        self
    }
}

/// Common interface for agent display components.
///
/// This trait defines a unified contract for how all agents report their status,
/// ensuring consistent display across Runner, Reviewer, Corrector, and Commit phases.
///
/// All implementors should provide:
/// - Agent name identification
/// - Elapsed time tracking
/// - Activity preview updates
/// - Iteration/progress context
pub trait AgentDisplay {
    /// Start the display for an agent operation.
    /// Called when the agent begins its work.
    fn start(&mut self);

    /// Update the display with current activity information.
    ///
    /// # Arguments
    /// * `activity` - Brief description of current activity (will be truncated if too long)
    fn update(&mut self, activity: &str);

    /// Mark the operation as successfully completed.
    /// Stops any timers and displays a success message.
    fn finish_success(&mut self);

    /// Mark the operation as failed.
    /// Stops any timers and displays an error message.
    ///
    /// # Arguments
    /// * `error` - Description of what went wrong
    fn finish_error(&mut self, error: &str);

    /// Mark the operation as completed with a specific outcome.
    /// Allows for more detailed completion information than simple success/failure.
    ///
    /// # Arguments
    /// * `outcome` - The outcome information including success status and message
    fn finish_with_outcome(&mut self, outcome: Outcome);

    /// Get the agent's display name
    fn agent_name(&self) -> &str;

    /// Get the elapsed time in seconds since the operation started
    fn elapsed_secs(&self) -> u64;

    /// Get the current iteration information, if any
    fn iteration_info(&self) -> Option<&IterationInfo>;

    /// Set the iteration information for progress context
    fn set_iteration_info(&mut self, info: IterationInfo);
}

/// Extension trait for using AgentDisplay in a type-erased context
pub trait AgentDisplayExt: AgentDisplay {
    /// Finish with success (type-erased version that can be called on &mut dyn AgentDisplay)
    fn complete_success(&mut self) {
        AgentDisplay::finish_success(self);
    }

    /// Finish with error (type-erased version)
    fn complete_error(&mut self, error: &str) {
        AgentDisplay::finish_error(self, error);
    }

    /// Finish with outcome (type-erased version)
    fn complete_with_outcome(&mut self, outcome: Outcome) {
        AgentDisplay::finish_with_outcome(self, outcome);
    }
}

impl<T: AgentDisplay> AgentDisplayExt for T {}

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
    iteration_info: Option<IterationInfo>,
    started: bool,
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
            iteration_info: None,
            started: true,
        }
    }

    /// Create a new timer for a story with iteration context
    pub fn new_with_story_progress(story_id: &str, current: u32, total: u32) -> Self {
        let mut timer = Self::new(story_id);
        timer.iteration_info = Some(IterationInfo::with_phase(story_id, current, total));
        timer
    }

    /// Create a new timer for review with iteration context
    pub fn new_for_review(current: u32, total: u32) -> Self {
        let mut timer = Self::new("Review");
        timer.iteration_info = Some(IterationInfo::with_phase("Review", current, total));
        timer
    }

    /// Create a new timer for correction with iteration context
    pub fn new_for_correct(current: u32, total: u32) -> Self {
        let mut timer = Self::new("Correct");
        timer.iteration_info = Some(IterationInfo::with_phase("Correct", current, total));
        timer
    }

    pub fn new_for_spec() -> Self {
        Self::new("Spec generation")
    }

    /// Create a new timer for commit
    pub fn new_for_commit() -> Self {
        let mut timer = Self::new("Commit");
        timer.iteration_info = Some(IterationInfo::phase_only("Commit"));
        timer
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
        let duration = format_duration(elapsed.as_secs());
        let prefix = format_display_prefix(&self.story_id, &self.iteration_info);
        eprintln!("{GREEN}{} completed in {}{RESET}", prefix, duration);
    }

    pub fn finish_error(&mut self, error: &str) {
        self.stop_timer();
        let prefix = format_display_prefix(&self.story_id, &self.iteration_info);
        eprintln!("{RED}{} failed: {}{RESET}", prefix, error);
    }

    pub fn elapsed_secs(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }
}

impl AgentDisplay for VerboseTimer {
    fn start(&mut self) {
        // VerboseTimer starts automatically on creation, so this is a no-op
        // but we mark it as started for consistency
        self.started = true;
    }

    fn update(&mut self, _activity: &str) {
        // VerboseTimer doesn't truncate output, so update is a no-op
        // The full output scrolls naturally in verbose mode
    }

    fn finish_success(&mut self) {
        // Delegate to the inherent method
        VerboseTimer::finish_success(self);
    }

    fn finish_error(&mut self, error: &str) {
        // Delegate to the inherent method
        VerboseTimer::finish_error(self, error);
    }

    fn finish_with_outcome(&mut self, outcome: Outcome) {
        self.stop_timer();
        let elapsed = self.start_time.elapsed();
        let duration = format_duration(elapsed.as_secs());
        let prefix = format_display_prefix(&self.story_id, &self.iteration_info);

        // Build the token suffix if tokens are present
        let token_suffix = outcome
            .tokens
            .map(|t| format!(" - {} tokens", format_tokens(t)))
            .unwrap_or_default();

        if outcome.success {
            eprintln!(
                "{GREEN}\u{2714} {} completed in {} - {}{}{RESET}",
                prefix, duration, outcome.message, token_suffix
            );
        } else {
            eprintln!(
                "{RED}\u{2718} {} failed in {} - {}{}{RESET}",
                prefix, duration, outcome.message, token_suffix
            );
        }
    }

    fn agent_name(&self) -> &str {
        &self.story_id
    }

    fn elapsed_secs(&self) -> u64 {
        VerboseTimer::elapsed_secs(self)
    }

    fn iteration_info(&self) -> Option<&IterationInfo> {
        self.iteration_info.as_ref()
    }

    fn set_iteration_info(&mut self, info: IterationInfo) {
        self.iteration_info = Some(info);
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
// Helper functions
// ============================================================================

/// Format duration in a human-readable way: "Xs" for <60s, "Xm Ys" for >=60s
pub fn format_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else {
        let mins = secs / 60;
        let remaining_secs = secs % 60;
        format!("{}m {}s", mins, remaining_secs)
    }
}

/// Format token count with thousands separators (e.g., 1,234,567)
pub fn format_tokens(tokens: u64) -> String {
    let s = tokens.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    let chars: Vec<char> = s.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if i > 0 && (chars.len() - i).is_multiple_of(3) == 0 {
            result.push(',');
        }
        result.push(*c);
    }
    result
}

/// Format the display prefix based on story_id and iteration info
/// Returns format like "[US-001 2/5]", "[Review 1/3]", "[Commit]", or just the story_id
fn format_display_prefix(story_id: &str, iteration_info: &Option<IterationInfo>) -> String {
    if let Some(info) = iteration_info {
        if let Some(formatted) = info.format() {
            return formatted;
        }
    }
    // Fall back to story_id if no iteration info or invalid format
    // Special case for Spec
    if story_id == "Spec" {
        "Spec generation".to_string()
    } else {
        story_id.to_string()
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
    iteration_info: Option<IterationInfo>,
    iteration_info_shared: Arc<std::sync::Mutex<Option<IterationInfo>>>,
}

impl ClaudeSpinner {
    pub fn new(story_id: &str) -> Self {
        Self::create(story_id, format!("{} | Starting...", story_id))
    }

    /// Create a new spinner for a story with iteration context
    /// Display format: `[US-001 2/5] | activity [HH:MM:SS]`
    pub fn new_with_story_progress(story_id: &str, current: u32, total: u32) -> Self {
        let info = IterationInfo::with_phase(story_id, current, total);
        let prefix = info.format().unwrap_or_else(|| story_id.to_string());
        Self::create_with_iteration(story_id, format!("{} | Starting...", prefix), Some(info))
    }

    /// Create a new spinner for review with iteration context
    /// Display format: `[Review 1/3] | activity [HH:MM:SS]`
    pub fn new_for_review(current: u32, total: u32) -> Self {
        let info = IterationInfo::with_phase("Review", current, total);
        let prefix = info.format().unwrap_or_else(|| "Review".to_string());
        Self::create_with_iteration("Review", format!("{} | Starting...", prefix), Some(info))
    }

    /// Create a new spinner for correction with iteration context
    /// Display format: `[Correct 1/3] | activity [HH:MM:SS]`
    pub fn new_for_correct(current: u32, total: u32) -> Self {
        let info = IterationInfo::with_phase("Correct", current, total);
        let prefix = info.format().unwrap_or_else(|| "Correct".to_string());
        Self::create_with_iteration("Correct", format!("{} | Starting...", prefix), Some(info))
    }

    pub fn new_for_spec() -> Self {
        Self::create("Spec", "Spec generation | Starting...".to_string())
    }

    /// Create a new spinner for commit
    /// Display format: `[Commit] | activity [HH:MM:SS]`
    pub fn new_for_commit() -> Self {
        let info = IterationInfo::phase_only("Commit");
        let prefix = info.format().unwrap_or_else(|| "Commit".to_string());
        Self::create_with_iteration("Commit", format!("{} | Starting...", prefix), Some(info))
    }

    fn create(story_id: &str, initial_message: String) -> Self {
        Self::create_with_iteration(story_id, initial_message, None)
    }

    fn create_with_iteration(
        story_id: &str,
        initial_message: String,
        iteration_info: Option<IterationInfo>,
    ) -> Self {
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
        let iteration_info_shared = Arc::new(std::sync::Mutex::new(iteration_info.clone()));

        // Clone for timer thread
        let spinner_clone = Arc::clone(&spinner);
        let stop_flag_clone = Arc::clone(&stop_flag);
        let last_activity_clone = Arc::clone(&last_activity);
        let iteration_info_clone = Arc::clone(&iteration_info_shared);
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
                let iter_info = iteration_info_clone.lock().unwrap().clone();
                let prefix = format_display_prefix(&story_id_owned, &iter_info);
                let fixed_activity = fixed_width_activity(&activity);

                spinner_clone
                    .set_message(format!("{} | {} [{}]", prefix, fixed_activity, time_str));
            }
        });

        Self {
            spinner,
            story_id: story_id.to_string(),
            stop_flag,
            timer_thread: Some(timer_thread),
            start_time,
            last_activity,
            iteration_info,
            iteration_info_shared,
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

        let iter_info = self.iteration_info_shared.lock().unwrap().clone();
        let prefix = format_display_prefix(&self.story_id, &iter_info);
        let fixed_activity = fixed_width_activity(activity);

        self.spinner
            .set_message(format!("{} | {} [{}]", prefix, fixed_activity, time_str));
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
        let duration = format_duration(duration_secs);
        let prefix = format_display_prefix(&self.story_id, &self.iteration_info);
        // Clear the line first, then print completion message to ensure clean output
        self.spinner.finish_and_clear();
        println!(
            "{GREEN}\u{2714} {} completed in {}{RESET}",
            prefix, duration
        );
    }

    pub fn finish_error(&mut self, error: &str) {
        self.stop_timer();
        let prefix = format_display_prefix(&self.story_id, &self.iteration_info);
        // For error messages, use a reasonable width accounting for " failed: " and color codes
        let available = get_terminal_width().saturating_sub(prefix.chars().count() + 15);
        let truncated = truncate_activity(error, available.max(20));
        // Clear the line first, then print error message to ensure clean output
        self.spinner.finish_and_clear();
        println!("{RED}\u{2718} {} failed: {}{RESET}", prefix, truncated);
    }

    pub fn finish_with_message(&mut self, message: &str) {
        self.stop_timer();
        let prefix = format_display_prefix(&self.story_id, &self.iteration_info);
        // Clear the line first, then print message to ensure clean output
        self.spinner.finish_and_clear();
        println!("{GREEN}\u{2714} {}: {}{RESET}", prefix, message);
    }

    pub fn elapsed_secs(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }
}

impl AgentDisplay for ClaudeSpinner {
    fn start(&mut self) {
        // ClaudeSpinner starts automatically on creation, so this is a no-op
    }

    fn update(&mut self, activity: &str) {
        // Delegate to the inherent method (note: takes &self, not &mut self)
        ClaudeSpinner::update(self, activity);
    }

    fn finish_success(&mut self) {
        // Use internal elapsed time for trait implementation
        let elapsed = self.start_time.elapsed().as_secs();
        ClaudeSpinner::finish_success(self, elapsed);
    }

    fn finish_error(&mut self, error: &str) {
        // Delegate to the inherent method
        ClaudeSpinner::finish_error(self, error);
    }

    fn finish_with_outcome(&mut self, outcome: Outcome) {
        self.stop_timer();
        let elapsed = self.start_time.elapsed();
        let duration = format_duration(elapsed.as_secs());
        let prefix = format_display_prefix(&self.story_id, &self.iteration_info);
        self.spinner.finish_and_clear();

        // Build the token suffix if tokens are present
        let token_suffix = outcome
            .tokens
            .map(|t| format!(" - {} tokens", format_tokens(t)))
            .unwrap_or_default();

        if outcome.success {
            println!(
                "{GREEN}\u{2714} {} completed in {} - {}{}{RESET}",
                prefix, duration, outcome.message, token_suffix
            );
        } else {
            println!(
                "{RED}\u{2718} {} failed in {} - {}{}{RESET}",
                prefix, duration, outcome.message, token_suffix
            );
        }
    }

    fn agent_name(&self) -> &str {
        &self.story_id
    }

    fn elapsed_secs(&self) -> u64 {
        ClaudeSpinner::elapsed_secs(self)
    }

    fn iteration_info(&self) -> Option<&IterationInfo> {
        self.iteration_info.as_ref()
    }

    fn set_iteration_info(&mut self, info: IterationInfo) {
        self.iteration_info = Some(info.clone());
        // Also update the shared version for the timer thread
        if let Ok(mut guard) = self.iteration_info_shared.lock() {
            *guard = Some(info);
        }
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

/// Create a fixed-width activity text string (US-002).
///
/// - Truncates text longer than ACTIVITY_TEXT_WIDTH with "..." suffix
/// - Pads text shorter than ACTIVITY_TEXT_WIDTH with trailing spaces
/// - This ensures the timer position remains fixed regardless of activity text content
fn fixed_width_activity(activity: &str) -> String {
    // Take first line only and clean it up
    let first_line = activity.lines().next().unwrap_or(activity);
    let cleaned = first_line.trim();

    let char_count = cleaned.chars().count();

    if char_count > ACTIVITY_TEXT_WIDTH {
        // Truncate with "..." suffix
        let truncated: String = cleaned.chars().take(ACTIVITY_TEXT_WIDTH - 3).collect();
        format!("{}...", truncated)
    } else {
        // Pad with spaces to reach fixed width
        format!("{:width$}", cleaned, width = ACTIVITY_TEXT_WIDTH)
    }
}

// ============================================================================
// Breadcrumb: Track workflow journey through states
// ============================================================================

/// Represents a single state in the workflow journey
#[derive(Debug, Clone, PartialEq)]
pub enum BreadcrumbState {
    Story,
    Review,
    Correct,
    Commit,
}

impl BreadcrumbState {
    /// Get the display name for this state
    pub fn display_name(&self) -> &'static str {
        match self {
            BreadcrumbState::Story => "Story",
            BreadcrumbState::Review => "Review",
            BreadcrumbState::Correct => "Correct",
            BreadcrumbState::Commit => "Commit",
        }
    }
}

/// Tracks the workflow journey through different states.
///
/// Displays a breadcrumb trail showing completed and current states:
/// `Journey: Story → Review → Correct → Review`
///
/// - Completed states are shown in green
/// - Current state is shown in yellow
/// - Future states are not shown
#[derive(Debug, Clone, Default)]
pub struct Breadcrumb {
    /// History of states visited (completed states)
    completed: Vec<BreadcrumbState>,
    /// Current state being processed (if any)
    current: Option<BreadcrumbState>,
}

impl Breadcrumb {
    /// Create a new empty breadcrumb trail
    pub fn new() -> Self {
        Self {
            completed: Vec::new(),
            current: None,
        }
    }

    /// Reset the breadcrumb trail (used at start of each new story)
    pub fn reset(&mut self) {
        self.completed.clear();
        self.current = None;
    }

    /// Enter a new state, marking any current state as completed
    pub fn enter_state(&mut self, state: BreadcrumbState) {
        // Mark current state as completed if it exists
        if let Some(current) = self.current.take() {
            self.completed.push(current);
        }
        self.current = Some(state);
    }

    /// Mark current state as completed without entering a new one
    pub fn complete_current(&mut self) {
        if let Some(current) = self.current.take() {
            self.completed.push(current);
        }
    }

    /// Get the list of completed states
    pub fn completed_states(&self) -> &[BreadcrumbState] {
        &self.completed
    }

    /// Get the current state if any
    pub fn current_state(&self) -> Option<&BreadcrumbState> {
        self.current.as_ref()
    }

    /// Check if the breadcrumb trail is empty
    pub fn is_empty(&self) -> bool {
        self.completed.is_empty() && self.current.is_none()
    }

    /// Render the breadcrumb trail as a colored string.
    ///
    /// Format: `Journey: Story → Review → Correct → Review`
    /// - Completed states in green
    /// - Current state in yellow
    /// - Uses `→` separator in gray/dim color
    /// - Truncates with `...` if too long for terminal
    pub fn render(&self, max_width: Option<usize>) -> String {
        if self.is_empty() {
            return String::new();
        }

        let max_width = max_width.unwrap_or_else(get_terminal_width);
        let separator = format!("{GRAY} → {RESET}");
        let prefix = format!("{DIM}Journey:{RESET} ");

        // Build the trail parts
        let mut parts: Vec<String> = Vec::new();

        // Add completed states in green
        for state in &self.completed {
            parts.push(format!("{GREEN}{}{RESET}", state.display_name()));
        }

        // Add current state in yellow
        if let Some(current) = &self.current {
            parts.push(format!("{YELLOW}{}{RESET}", current.display_name()));
        }

        // Calculate plain text length for truncation (without ANSI codes)
        let plain_prefix = "Journey: ";
        let plain_separator = " → ";
        let plain_parts: Vec<&str> = self
            .completed
            .iter()
            .map(|s| s.display_name())
            .chain(self.current.iter().map(|s| s.display_name()))
            .collect();
        let plain_trail = plain_parts.join(plain_separator);
        let plain_full = format!("{}{}", plain_prefix, plain_trail);
        let plain_len = plain_full.chars().count();

        // If the trail fits, return it
        if plain_len <= max_width {
            return format!("{}{}", prefix, parts.join(&separator));
        }

        // Need to truncate - show as many recent states as possible with "..."
        let ellipsis = "...";
        let available = max_width.saturating_sub(plain_prefix.len() + ellipsis.len() + 4); // 4 for " → " after ellipsis

        // Start from the end and work backwards to fit as many states as possible
        let mut fit_parts: Vec<String> = Vec::new();
        let mut fit_plain_parts: Vec<&str> = Vec::new();
        let mut current_len: usize = 0;

        // First, always include current state if it exists
        if let Some(current) = &self.current {
            fit_parts.push(format!("{YELLOW}{}{RESET}", current.display_name()));
            fit_plain_parts.push(current.display_name());
            current_len = current.display_name().chars().count();
        }

        // Add completed states from most recent to oldest until we run out of space
        for state in self.completed.iter().rev() {
            let state_len = state.display_name().chars().count();
            let sep_len = if fit_parts.is_empty() {
                0
            } else {
                plain_separator.len()
            };

            if current_len + sep_len + state_len <= available {
                fit_parts.insert(0, format!("{GREEN}{}{RESET}", state.display_name()));
                fit_plain_parts.insert(0, state.display_name());
                current_len += sep_len + state_len;
            } else {
                break;
            }
        }

        // If we couldn't fit all states, prepend ellipsis
        if fit_plain_parts.len() < plain_parts.len() {
            format!(
                "{}{DIM}...{RESET}{}{}",
                prefix,
                separator,
                fit_parts.join(&separator)
            )
        } else {
            format!("{}{}", prefix, fit_parts.join(&separator))
        }
    }

    /// Print the breadcrumb trail to stdout if it's not empty
    pub fn print(&self) {
        if !self.is_empty() {
            println!("{}", self.render(None));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // Core text truncation tests
    // ========================================================================

    #[test]
    fn test_truncate_activity() {
        // Short - no truncation
        assert_eq!(truncate_activity("Short message", 50), "Short message");

        // Long - truncation with ellipsis
        let long_msg = "This is a very long message that should be truncated";
        let result = truncate_activity(long_msg, 30);
        assert_eq!(result.chars().count(), 30);
        assert!(result.ends_with("..."));

        // Multiline - only first line
        assert_eq!(
            truncate_activity("First line\nSecond line", 50),
            "First line"
        );

        // UTF-8 handling
        let utf8_msg = "Implementing 日本語 feature with more text here";
        let result = truncate_activity(utf8_msg, 20);
        assert_eq!(result.chars().count(), 20);
    }

    #[test]
    fn test_fixed_width_activity() {
        // Short text - padded
        let result = fixed_width_activity("Working");
        assert_eq!(result.chars().count(), ACTIVITY_TEXT_WIDTH);
        assert!(result.starts_with("Working"));

        // Long text - truncated
        let long_msg = "This is a very long message that exceeds forty characters limit";
        let result = fixed_width_activity(long_msg);
        assert_eq!(result.chars().count(), ACTIVITY_TEXT_WIDTH);
        assert!(result.ends_with("..."));

        // Empty - all spaces
        let result = fixed_width_activity("");
        assert_eq!(result.chars().count(), ACTIVITY_TEXT_WIDTH);
        assert!(result.chars().all(|c| c == ' '));
    }

    // ========================================================================
    // Duration formatting
    // ========================================================================

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(59), "59s");
        assert_eq!(format_duration(60), "1m 0s");
        assert_eq!(format_duration(125), "2m 5s");
    }

    // ========================================================================
    // IterationInfo formatting
    // ========================================================================

    #[test]
    fn test_iteration_info_format() {
        // With phase and counts
        let info = IterationInfo::with_phase("Review", 1, 3);
        assert_eq!(info.format(), Some("[Review 1/3]".to_string()));

        // Phase only
        let info = IterationInfo::phase_only("Commit");
        assert_eq!(info.format(), Some("[Commit]".to_string()));

        // Counts only
        let info = IterationInfo::new(2, 5);
        assert_eq!(info.format(), Some("[2/5]".to_string()));

        // Default - None
        assert_eq!(IterationInfo::default().format(), None);
    }

    // ========================================================================
    // Display prefix formatting
    // ========================================================================

    #[test]
    fn test_format_display_prefix() {
        // With iteration info
        let info = IterationInfo::with_phase("Review", 1, 3);
        assert_eq!(format_display_prefix("Review", &Some(info)), "[Review 1/3]");

        // Without info - falls back to story_id
        assert_eq!(format_display_prefix("US-001", &None), "US-001");

        // Spec special case
        assert_eq!(format_display_prefix("Spec", &None), "Spec generation");
    }

    // ========================================================================
    // ProgressContext dual-context formatting
    // ========================================================================

    #[test]
    fn test_progress_context_dual_context() {
        let ctx = ProgressContext::new("US-001", 2, 5);

        // Both present
        let iter_info = Some(IterationInfo::with_phase("Review", 1, 3));
        assert_eq!(
            ctx.format_dual_context(&iter_info),
            Some("[US-001 2/5 | Review 1/3]".to_string())
        );

        // Story only
        assert_eq!(
            ctx.format_dual_context(&None),
            Some("[US-001 2/5]".to_string())
        );

        // Neither
        let empty_ctx = ProgressContext::default();
        assert_eq!(empty_ctx.format_dual_context(&None), None);
    }

    // ========================================================================
    // Breadcrumb state management
    // ========================================================================

    #[test]
    fn test_breadcrumb_workflow() {
        let mut breadcrumb = Breadcrumb::new();
        assert!(breadcrumb.is_empty());

        // Enter states
        breadcrumb.enter_state(BreadcrumbState::Story);
        assert_eq!(breadcrumb.current_state(), Some(&BreadcrumbState::Story));
        assert!(breadcrumb.completed_states().is_empty());

        breadcrumb.enter_state(BreadcrumbState::Review);
        assert_eq!(breadcrumb.current_state(), Some(&BreadcrumbState::Review));
        assert_eq!(breadcrumb.completed_states(), &[BreadcrumbState::Story]);

        // Reset
        breadcrumb.reset();
        assert!(breadcrumb.is_empty());
    }

    #[test]
    fn test_breadcrumb_render() {
        let mut breadcrumb = Breadcrumb::new();
        breadcrumb.enter_state(BreadcrumbState::Story);
        breadcrumb.enter_state(BreadcrumbState::Review);

        let rendered = breadcrumb.render(Some(100));
        assert!(rendered.contains("Journey:"));
        assert!(rendered.contains("Story"));
        assert!(rendered.contains("Review"));
        assert!(rendered.contains("→"));
    }

    // ========================================================================
    // Spinner lifecycle (minimal - just verify cleanup works)
    // ========================================================================

    #[test]
    fn test_spinner_lifecycle() {
        let mut spinner = ClaudeSpinner::new("US-001");
        assert!(!spinner.stop_flag.load(Ordering::Relaxed));

        spinner.update("Working");
        let activity = spinner.last_activity.lock().unwrap().clone();
        assert_eq!(activity, "Working");

        spinner.stop_timer();
        assert!(spinner.stop_flag.load(Ordering::Relaxed));
    }

    #[test]
    fn test_verbose_timer_lifecycle() {
        let mut timer = VerboseTimer::new("US-001");
        assert!(!timer.stop_flag.load(Ordering::Relaxed));

        timer.stop_timer();
        assert!(timer.stop_flag.load(Ordering::Relaxed));
    }

    // ========================================================================
    // Drop cleanup verification
    // ========================================================================

    #[test]
    fn test_drop_stops_timer() {
        let stop_flag_clone;
        {
            let spinner = ClaudeSpinner::new("test");
            stop_flag_clone = Arc::clone(&spinner.stop_flag);
            assert!(!stop_flag_clone.load(Ordering::Relaxed));
        }
        assert!(stop_flag_clone.load(Ordering::Relaxed));
    }

    // ========================================================================
    // US-007: Token formatting and display tests
    // ========================================================================

    #[test]
    fn test_format_tokens_zero() {
        assert_eq!(format_tokens(0), "0");
    }

    #[test]
    fn test_format_tokens_small() {
        assert_eq!(format_tokens(1), "1");
        assert_eq!(format_tokens(12), "12");
        assert_eq!(format_tokens(123), "123");
    }

    #[test]
    fn test_format_tokens_thousands() {
        assert_eq!(format_tokens(1000), "1,000");
        assert_eq!(format_tokens(1234), "1,234");
        assert_eq!(format_tokens(12345), "12,345");
        assert_eq!(format_tokens(123456), "123,456");
    }

    #[test]
    fn test_format_tokens_millions() {
        assert_eq!(format_tokens(1000000), "1,000,000");
        assert_eq!(format_tokens(1234567), "1,234,567");
        assert_eq!(format_tokens(12345678), "12,345,678");
    }

    #[test]
    fn test_format_tokens_large() {
        assert_eq!(format_tokens(123456789), "123,456,789");
        assert_eq!(format_tokens(1234567890), "1,234,567,890");
    }

    #[test]
    fn test_format_tokens_boundary_cases() {
        assert_eq!(format_tokens(999), "999");
        assert_eq!(format_tokens(1000), "1,000");
        assert_eq!(format_tokens(9999), "9,999");
        assert_eq!(format_tokens(10000), "10,000");
        assert_eq!(format_tokens(99999), "99,999");
        assert_eq!(format_tokens(100000), "100,000");
    }

    #[test]
    fn test_outcome_with_tokens() {
        let outcome = Outcome::success("Done").with_tokens(45678);
        assert!(outcome.success);
        assert_eq!(outcome.message, "Done");
        assert_eq!(outcome.tokens, Some(45678));
    }

    #[test]
    fn test_outcome_with_optional_tokens_some() {
        let outcome = Outcome::success("Done").with_optional_tokens(Some(12345));
        assert_eq!(outcome.tokens, Some(12345));
    }

    #[test]
    fn test_outcome_with_optional_tokens_none() {
        let outcome = Outcome::success("Done").with_optional_tokens(None);
        assert_eq!(outcome.tokens, None);
    }

    #[test]
    fn test_outcome_default_no_tokens() {
        let outcome = Outcome::success("Done");
        assert_eq!(outcome.tokens, None);

        let outcome_fail = Outcome::failure("Error");
        assert_eq!(outcome_fail.tokens, None);
    }

    #[test]
    fn test_outcome_failure_with_tokens() {
        let outcome = Outcome::failure("Build failed").with_tokens(1000);
        assert!(!outcome.success);
        assert_eq!(outcome.message, "Build failed");
        assert_eq!(outcome.tokens, Some(1000));
    }
}
