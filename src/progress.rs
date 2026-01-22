use crate::output::{DIM, GRAY, GREEN, RED, RESET, YELLOW};
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
}

impl Outcome {
    /// Create a successful outcome with a message
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: message.into(),
        }
    }

    /// Create a failed outcome with an error message
    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: message.into(),
        }
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

    pub fn new_for_prd() -> Self {
        Self::new("PRD generation")
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

        if outcome.success {
            eprintln!(
                "{GREEN}\u{2714} {} completed in {} - {}{RESET}",
                prefix, duration, outcome.message
            );
        } else {
            eprintln!(
                "{RED}\u{2718} {} failed in {} - {}{RESET}",
                prefix, duration, outcome.message
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

/// Format the display prefix based on story_id and iteration info
/// Returns format like "[US-001 2/5]", "[Review 1/3]", "[Commit]", or just the story_id
fn format_display_prefix(story_id: &str, iteration_info: &Option<IterationInfo>) -> String {
    if let Some(info) = iteration_info {
        if let Some(formatted) = info.format() {
            return formatted;
        }
    }
    // Fall back to story_id if no iteration info or invalid format
    // Special case for PRD
    if story_id == "PRD" {
        "PRD generation".to_string()
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

    pub fn new_for_prd() -> Self {
        Self::create("PRD", "PRD generation | Starting...".to_string())
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
                let truncated = truncate_activity_for_display(&activity, &prefix);

                spinner_clone.set_message(format!("{} | {} [{}]", prefix, truncated, time_str));
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
        let truncated = truncate_activity_for_display(activity, &prefix);

        self.spinner
            .set_message(format!("{} | {} [{}]", prefix, truncated, time_str));
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
        println!("{GREEN}\u{2714} {} completed in {}{RESET}", prefix, duration);
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

        if outcome.success {
            println!(
                "{GREEN}\u{2714} {} completed in {} - {}{RESET}",
                prefix, duration, outcome.message
            );
        } else {
            println!(
                "{RED}\u{2718} {} failed in {} - {}{RESET}",
                prefix, duration, outcome.message
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
            let sep_len = if fit_parts.is_empty() { 0 } else { plain_separator.len() };

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

    // ========================================================================
    // AgentDisplay trait tests (US-001)
    // ========================================================================

    #[test]
    fn test_iteration_info_new() {
        let info = IterationInfo::new(2, 5);
        assert_eq!(info.current, Some(2));
        assert_eq!(info.total, Some(5));
        assert_eq!(info.phase, None);
    }

    #[test]
    fn test_iteration_info_with_phase() {
        let info = IterationInfo::with_phase("Review", 1, 3);
        assert_eq!(info.current, Some(1));
        assert_eq!(info.total, Some(3));
        assert_eq!(info.phase, Some("Review".to_string()));
    }

    #[test]
    fn test_iteration_info_phase_only() {
        let info = IterationInfo::phase_only("Commit");
        assert_eq!(info.current, None);
        assert_eq!(info.total, None);
        assert_eq!(info.phase, Some("Commit".to_string()));
    }

    #[test]
    fn test_iteration_info_format_with_phase_and_counts() {
        let info = IterationInfo::with_phase("Review", 1, 3);
        assert_eq!(info.format(), Some("[Review 1/3]".to_string()));
    }

    #[test]
    fn test_iteration_info_format_phase_only() {
        let info = IterationInfo::phase_only("Commit");
        assert_eq!(info.format(), Some("[Commit]".to_string()));
    }

    #[test]
    fn test_iteration_info_format_counts_only() {
        let info = IterationInfo::new(2, 5);
        assert_eq!(info.format(), Some("[2/5]".to_string()));
    }

    #[test]
    fn test_iteration_info_format_default() {
        let info = IterationInfo::default();
        assert_eq!(info.format(), None);
    }

    #[test]
    fn test_outcome_success() {
        let outcome = Outcome::success("Implementation done");
        assert!(outcome.success);
        assert_eq!(outcome.message, "Implementation done");
    }

    #[test]
    fn test_outcome_failure() {
        let outcome = Outcome::failure("Build failed");
        assert!(!outcome.success);
        assert_eq!(outcome.message, "Build failed");
    }

    #[test]
    fn test_format_duration_seconds() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(30), "30s");
        assert_eq!(format_duration(59), "59s");
    }

    #[test]
    fn test_format_duration_minutes() {
        assert_eq!(format_duration(60), "1m 0s");
        assert_eq!(format_duration(90), "1m 30s");
        assert_eq!(format_duration(125), "2m 5s");
    }

    #[test]
    fn test_verbose_timer_agent_display_agent_name() {
        let mut timer = VerboseTimer::new("US-001");
        assert_eq!(timer.agent_name(), "US-001");
        timer.stop_timer();
    }

    #[test]
    fn test_verbose_timer_agent_display_elapsed_secs() {
        let mut timer = VerboseTimer::new("US-001");
        // Just created, elapsed should be small
        assert!(timer.elapsed_secs() <= 1);
        timer.stop_timer();
    }

    #[test]
    fn test_verbose_timer_agent_display_iteration_info() {
        let mut timer = VerboseTimer::new("US-001");
        assert!(timer.iteration_info().is_none());

        timer.set_iteration_info(IterationInfo::with_phase("Review", 1, 3));
        let info = timer.iteration_info().unwrap();
        assert_eq!(info.phase, Some("Review".to_string()));
        assert_eq!(info.current, Some(1));
        assert_eq!(info.total, Some(3));

        timer.stop_timer();
    }

    #[test]
    fn test_verbose_timer_agent_display_finish_with_outcome_success() {
        let mut timer = VerboseTimer::new("US-001");
        timer.finish_with_outcome(Outcome::success("All tests passed"));
        assert!(timer.stop_flag.load(Ordering::Relaxed));
        assert!(timer.timer_thread.is_none());
    }

    #[test]
    fn test_verbose_timer_agent_display_finish_with_outcome_failure() {
        let mut timer = VerboseTimer::new("US-001");
        timer.finish_with_outcome(Outcome::failure("Test failed"));
        assert!(timer.stop_flag.load(Ordering::Relaxed));
        assert!(timer.timer_thread.is_none());
    }

    #[test]
    fn test_spinner_agent_display_agent_name() {
        let mut spinner = ClaudeSpinner::new("US-002");
        assert_eq!(spinner.agent_name(), "US-002");
        spinner.stop_timer();
    }

    #[test]
    fn test_spinner_agent_display_elapsed_secs() {
        let mut spinner = ClaudeSpinner::new("US-002");
        // Just created, elapsed should be small
        assert!(spinner.elapsed_secs() <= 1);
        spinner.stop_timer();
    }

    #[test]
    fn test_spinner_agent_display_iteration_info() {
        let mut spinner = ClaudeSpinner::new("US-002");
        assert!(spinner.iteration_info().is_none());

        spinner.set_iteration_info(IterationInfo::with_phase("Correct", 2, 3));
        let info = spinner.iteration_info().unwrap();
        assert_eq!(info.phase, Some("Correct".to_string()));
        assert_eq!(info.current, Some(2));
        assert_eq!(info.total, Some(3));

        spinner.stop_timer();
    }

    #[test]
    fn test_spinner_agent_display_finish_success() {
        let mut spinner = ClaudeSpinner::new("US-002");
        AgentDisplay::finish_success(&mut spinner);
        assert!(spinner.stop_flag.load(Ordering::Relaxed));
        assert!(spinner.timer_thread.is_none());
    }

    #[test]
    fn test_spinner_agent_display_finish_error() {
        let mut spinner = ClaudeSpinner::new("US-002");
        AgentDisplay::finish_error(&mut spinner, "Test error");
        assert!(spinner.stop_flag.load(Ordering::Relaxed));
        assert!(spinner.timer_thread.is_none());
    }

    #[test]
    fn test_spinner_agent_display_finish_with_outcome_success() {
        let mut spinner = ClaudeSpinner::new("US-002");
        spinner.finish_with_outcome(Outcome::success("Implementation done"));
        assert!(spinner.stop_flag.load(Ordering::Relaxed));
        assert!(spinner.timer_thread.is_none());
    }

    #[test]
    fn test_spinner_agent_display_finish_with_outcome_failure() {
        let mut spinner = ClaudeSpinner::new("US-002");
        spinner.finish_with_outcome(Outcome::failure("Build failed"));
        assert!(spinner.stop_flag.load(Ordering::Relaxed));
        assert!(spinner.timer_thread.is_none());
    }

    #[test]
    fn test_spinner_agent_display_update() {
        let mut spinner = ClaudeSpinner::new("US-002");
        AgentDisplay::update(&mut spinner, "Working on feature");

        let stored = spinner.last_activity.lock().unwrap().clone();
        assert_eq!(stored, "Working on feature");

        spinner.stop_timer();
    }

    #[test]
    fn test_spinner_agent_display_start() {
        let mut spinner = ClaudeSpinner::new("US-002");
        // start() is a no-op for ClaudeSpinner but should not panic
        AgentDisplay::start(&mut spinner);
        spinner.stop_timer();
    }

    #[test]
    fn test_verbose_timer_agent_display_start() {
        let mut timer = VerboseTimer::new("US-002");
        // start() is a no-op for VerboseTimer but should not panic
        AgentDisplay::start(&mut timer);
        timer.stop_timer();
    }

    #[test]
    fn test_verbose_timer_agent_display_update() {
        let mut timer = VerboseTimer::new("US-002");
        // update() is a no-op for VerboseTimer but should not panic
        AgentDisplay::update(&mut timer, "Working on feature");
        timer.stop_timer();
    }

    // ========================================================================
    // US-003: Iteration and progress context display tests
    // ========================================================================

    #[test]
    fn test_format_display_prefix_with_story_progress() {
        let info = IterationInfo::with_phase("US-001", 2, 5);
        let prefix = format_display_prefix("US-001", &Some(info));
        assert_eq!(prefix, "[US-001 2/5]");
    }

    #[test]
    fn test_format_display_prefix_with_review() {
        let info = IterationInfo::with_phase("Review", 1, 3);
        let prefix = format_display_prefix("Review", &Some(info));
        assert_eq!(prefix, "[Review 1/3]");
    }

    #[test]
    fn test_format_display_prefix_with_correct() {
        let info = IterationInfo::with_phase("Correct", 1, 3);
        let prefix = format_display_prefix("Correct", &Some(info));
        assert_eq!(prefix, "[Correct 1/3]");
    }

    #[test]
    fn test_format_display_prefix_with_commit() {
        let info = IterationInfo::phase_only("Commit");
        let prefix = format_display_prefix("Commit", &Some(info));
        assert_eq!(prefix, "[Commit]");
    }

    #[test]
    fn test_format_display_prefix_without_info() {
        let prefix = format_display_prefix("US-001", &None);
        assert_eq!(prefix, "US-001");
    }

    #[test]
    fn test_format_display_prefix_prd_fallback() {
        let prefix = format_display_prefix("PRD", &None);
        assert_eq!(prefix, "PRD generation");
    }

    #[test]
    fn test_spinner_new_with_story_progress() {
        let mut spinner = ClaudeSpinner::new_with_story_progress("US-001", 2, 5);
        let info = spinner.iteration_info().unwrap();
        assert_eq!(info.phase, Some("US-001".to_string()));
        assert_eq!(info.current, Some(2));
        assert_eq!(info.total, Some(5));
        assert_eq!(info.format(), Some("[US-001 2/5]".to_string()));
        spinner.stop_timer();
    }

    #[test]
    fn test_spinner_new_for_review() {
        let mut spinner = ClaudeSpinner::new_for_review(1, 3);
        let info = spinner.iteration_info().unwrap();
        assert_eq!(info.phase, Some("Review".to_string()));
        assert_eq!(info.current, Some(1));
        assert_eq!(info.total, Some(3));
        assert_eq!(info.format(), Some("[Review 1/3]".to_string()));
        spinner.stop_timer();
    }

    #[test]
    fn test_spinner_new_for_correct() {
        let mut spinner = ClaudeSpinner::new_for_correct(2, 3);
        let info = spinner.iteration_info().unwrap();
        assert_eq!(info.phase, Some("Correct".to_string()));
        assert_eq!(info.current, Some(2));
        assert_eq!(info.total, Some(3));
        assert_eq!(info.format(), Some("[Correct 2/3]".to_string()));
        spinner.stop_timer();
    }

    #[test]
    fn test_spinner_new_for_commit_has_iteration_info() {
        let mut spinner = ClaudeSpinner::new_for_commit();
        let info = spinner.iteration_info().unwrap();
        assert_eq!(info.phase, Some("Commit".to_string()));
        assert_eq!(info.current, None);
        assert_eq!(info.total, None);
        assert_eq!(info.format(), Some("[Commit]".to_string()));
        spinner.stop_timer();
    }

    #[test]
    fn test_verbose_timer_new_with_story_progress() {
        let mut timer = VerboseTimer::new_with_story_progress("US-001", 2, 5);
        let info = timer.iteration_info().unwrap();
        assert_eq!(info.phase, Some("US-001".to_string()));
        assert_eq!(info.current, Some(2));
        assert_eq!(info.total, Some(5));
        assert_eq!(info.format(), Some("[US-001 2/5]".to_string()));
        timer.stop_timer();
    }

    #[test]
    fn test_verbose_timer_new_for_review() {
        let mut timer = VerboseTimer::new_for_review(1, 3);
        let info = timer.iteration_info().unwrap();
        assert_eq!(info.phase, Some("Review".to_string()));
        assert_eq!(info.current, Some(1));
        assert_eq!(info.total, Some(3));
        assert_eq!(info.format(), Some("[Review 1/3]".to_string()));
        timer.stop_timer();
    }

    #[test]
    fn test_verbose_timer_new_for_correct() {
        let mut timer = VerboseTimer::new_for_correct(2, 3);
        let info = timer.iteration_info().unwrap();
        assert_eq!(info.phase, Some("Correct".to_string()));
        assert_eq!(info.current, Some(2));
        assert_eq!(info.total, Some(3));
        assert_eq!(info.format(), Some("[Correct 2/3]".to_string()));
        timer.stop_timer();
    }

    #[test]
    fn test_verbose_timer_new_for_commit_has_iteration_info() {
        let mut timer = VerboseTimer::new_for_commit();
        let info = timer.iteration_info().unwrap();
        assert_eq!(info.phase, Some("Commit".to_string()));
        assert_eq!(info.current, None);
        assert_eq!(info.total, None);
        assert_eq!(info.format(), Some("[Commit]".to_string()));
        timer.stop_timer();
    }

    #[test]
    fn test_spinner_set_iteration_info_updates_shared() {
        let mut spinner = ClaudeSpinner::new("US-001");
        assert!(spinner.iteration_info().is_none());

        spinner.set_iteration_info(IterationInfo::with_phase("Review", 1, 3));

        // Check the local copy
        let info = spinner.iteration_info().unwrap();
        assert_eq!(info.format(), Some("[Review 1/3]".to_string()));

        // Check the shared copy (used by timer thread)
        let shared = spinner.iteration_info_shared.lock().unwrap().clone();
        assert!(shared.is_some());
        assert_eq!(shared.unwrap().format(), Some("[Review 1/3]".to_string()));

        spinner.stop_timer();
    }

    // ========================================================================
    // US-004: Standardized completion messages with outcomes tests
    // ========================================================================

    #[test]
    fn test_outcome_with_runner_success() {
        // Runner completion: `✓ US-001 completed in 2m 34s - Implementation done`
        let outcome = Outcome::success("Implementation done");
        assert!(outcome.success);
        assert_eq!(outcome.message, "Implementation done");
    }

    #[test]
    fn test_outcome_with_reviewer_pass() {
        // Reviewer pass: `✓ Review 1/3 passed in 45s - No issues found`
        let outcome = Outcome::success("No issues found");
        assert!(outcome.success);
        assert_eq!(outcome.message, "No issues found");
    }

    #[test]
    fn test_outcome_with_reviewer_issues() {
        // Reviewer fail: `✓ Review 1/3 completed in 1m 12s - 3 issues found`
        let outcome = Outcome::success("3 issues found");
        assert!(outcome.success);
        assert_eq!(outcome.message, "3 issues found");
    }

    #[test]
    fn test_outcome_with_corrector() {
        // Corrector completion: `✓ Correct 1/3 completed in 1m 45s - Issues addressed`
        let outcome = Outcome::success("Issues addressed");
        assert!(outcome.success);
        assert_eq!(outcome.message, "Issues addressed");
    }

    #[test]
    fn test_outcome_with_commit_hash() {
        // Commit completion: `✓ Commit completed in 12s - abc1234`
        let outcome = Outcome::success("abc1234");
        assert!(outcome.success);
        assert_eq!(outcome.message, "abc1234");
    }

    #[test]
    fn test_spinner_finish_with_outcome_runner() {
        let mut spinner = ClaudeSpinner::new_with_story_progress("US-001", 2, 5);
        // Should not panic - this tests the completion message format
        spinner.finish_with_outcome(Outcome::success("Implementation done"));
        assert!(spinner.stop_flag.load(Ordering::Relaxed));
        assert!(spinner.timer_thread.is_none());
    }

    #[test]
    fn test_spinner_finish_with_outcome_reviewer() {
        let mut spinner = ClaudeSpinner::new_for_review(1, 3);
        spinner.finish_with_outcome(Outcome::success("No issues found"));
        assert!(spinner.stop_flag.load(Ordering::Relaxed));
    }

    #[test]
    fn test_spinner_finish_with_outcome_corrector() {
        let mut spinner = ClaudeSpinner::new_for_correct(1, 3);
        spinner.finish_with_outcome(Outcome::success("Issues addressed"));
        assert!(spinner.stop_flag.load(Ordering::Relaxed));
    }

    #[test]
    fn test_spinner_finish_with_outcome_commit() {
        let mut spinner = ClaudeSpinner::new_for_commit();
        spinner.finish_with_outcome(Outcome::success("abc1234"));
        assert!(spinner.stop_flag.load(Ordering::Relaxed));
    }

    #[test]
    fn test_verbose_timer_finish_with_outcome_runner() {
        let mut timer = VerboseTimer::new_with_story_progress("US-001", 2, 5);
        timer.finish_with_outcome(Outcome::success("Implementation done"));
        assert!(timer.stop_flag.load(Ordering::Relaxed));
    }

    #[test]
    fn test_verbose_timer_finish_with_outcome_reviewer() {
        let mut timer = VerboseTimer::new_for_review(1, 3);
        timer.finish_with_outcome(Outcome::success("No issues found"));
        assert!(timer.stop_flag.load(Ordering::Relaxed));
    }

    #[test]
    fn test_verbose_timer_finish_with_outcome_corrector() {
        let mut timer = VerboseTimer::new_for_correct(1, 3);
        timer.finish_with_outcome(Outcome::success("Issues addressed"));
        assert!(timer.stop_flag.load(Ordering::Relaxed));
    }

    #[test]
    fn test_verbose_timer_finish_with_outcome_commit() {
        let mut timer = VerboseTimer::new_for_commit();
        timer.finish_with_outcome(Outcome::success("abc1234"));
        assert!(timer.stop_flag.load(Ordering::Relaxed));
    }

    #[test]
    fn test_outcome_failure_with_error() {
        let outcome = Outcome::failure("Build failed: missing dependency");
        assert!(!outcome.success);
        assert_eq!(outcome.message, "Build failed: missing dependency");
    }

    #[test]
    fn test_spinner_finish_with_outcome_failure() {
        let mut spinner = ClaudeSpinner::new_with_story_progress("US-001", 2, 5);
        spinner.finish_with_outcome(Outcome::failure("Build failed"));
        assert!(spinner.stop_flag.load(Ordering::Relaxed));
    }

    #[test]
    fn test_verbose_timer_finish_with_outcome_failure() {
        let mut timer = VerboseTimer::new_with_story_progress("US-001", 2, 5);
        timer.finish_with_outcome(Outcome::failure("Build failed"));
        assert!(timer.stop_flag.load(Ordering::Relaxed));
    }

    // ========================================================================
    // US-005: Breadcrumb trail for workflow journey tests
    // ========================================================================

    #[test]
    fn test_breadcrumb_state_display_names() {
        assert_eq!(BreadcrumbState::Story.display_name(), "Story");
        assert_eq!(BreadcrumbState::Review.display_name(), "Review");
        assert_eq!(BreadcrumbState::Correct.display_name(), "Correct");
        assert_eq!(BreadcrumbState::Commit.display_name(), "Commit");
    }

    #[test]
    fn test_breadcrumb_new_is_empty() {
        let breadcrumb = Breadcrumb::new();
        assert!(breadcrumb.is_empty());
        assert!(breadcrumb.completed_states().is_empty());
        assert!(breadcrumb.current_state().is_none());
    }

    #[test]
    fn test_breadcrumb_default_is_empty() {
        let breadcrumb = Breadcrumb::default();
        assert!(breadcrumb.is_empty());
    }

    #[test]
    fn test_breadcrumb_enter_state() {
        let mut breadcrumb = Breadcrumb::new();

        breadcrumb.enter_state(BreadcrumbState::Story);
        assert!(!breadcrumb.is_empty());
        assert_eq!(breadcrumb.current_state(), Some(&BreadcrumbState::Story));
        assert!(breadcrumb.completed_states().is_empty());
    }

    #[test]
    fn test_breadcrumb_enter_multiple_states() {
        let mut breadcrumb = Breadcrumb::new();

        breadcrumb.enter_state(BreadcrumbState::Story);
        breadcrumb.enter_state(BreadcrumbState::Review);

        assert_eq!(breadcrumb.current_state(), Some(&BreadcrumbState::Review));
        assert_eq!(breadcrumb.completed_states(), &[BreadcrumbState::Story]);
    }

    #[test]
    fn test_breadcrumb_full_workflow() {
        let mut breadcrumb = Breadcrumb::new();

        breadcrumb.enter_state(BreadcrumbState::Story);
        breadcrumb.enter_state(BreadcrumbState::Review);
        breadcrumb.enter_state(BreadcrumbState::Correct);
        breadcrumb.enter_state(BreadcrumbState::Review);
        breadcrumb.enter_state(BreadcrumbState::Commit);

        assert_eq!(breadcrumb.current_state(), Some(&BreadcrumbState::Commit));
        assert_eq!(
            breadcrumb.completed_states(),
            &[
                BreadcrumbState::Story,
                BreadcrumbState::Review,
                BreadcrumbState::Correct,
                BreadcrumbState::Review,
            ]
        );
    }

    #[test]
    fn test_breadcrumb_complete_current() {
        let mut breadcrumb = Breadcrumb::new();

        breadcrumb.enter_state(BreadcrumbState::Story);
        breadcrumb.complete_current();

        assert!(breadcrumb.current_state().is_none());
        assert_eq!(breadcrumb.completed_states(), &[BreadcrumbState::Story]);
    }

    #[test]
    fn test_breadcrumb_reset() {
        let mut breadcrumb = Breadcrumb::new();

        breadcrumb.enter_state(BreadcrumbState::Story);
        breadcrumb.enter_state(BreadcrumbState::Review);
        breadcrumb.reset();

        assert!(breadcrumb.is_empty());
        assert!(breadcrumb.completed_states().is_empty());
        assert!(breadcrumb.current_state().is_none());
    }

    #[test]
    fn test_breadcrumb_render_empty() {
        let breadcrumb = Breadcrumb::new();
        assert_eq!(breadcrumb.render(None), "");
    }

    #[test]
    fn test_breadcrumb_render_single_current() {
        let mut breadcrumb = Breadcrumb::new();
        breadcrumb.enter_state(BreadcrumbState::Story);

        let rendered = breadcrumb.render(Some(100));
        // Should contain "Journey:" prefix
        assert!(rendered.contains("Journey:"));
        // Should contain "Story" (in yellow for current)
        assert!(rendered.contains("Story"));
        // Should contain YELLOW color code
        assert!(rendered.contains(YELLOW));
    }

    #[test]
    fn test_breadcrumb_render_with_completed() {
        let mut breadcrumb = Breadcrumb::new();
        breadcrumb.enter_state(BreadcrumbState::Story);
        breadcrumb.enter_state(BreadcrumbState::Review);

        let rendered = breadcrumb.render(Some(100));
        // Should contain both states
        assert!(rendered.contains("Story"));
        assert!(rendered.contains("Review"));
        // Should contain arrow separator
        assert!(rendered.contains("→"));
        // Story should be green (completed)
        assert!(rendered.contains(GREEN));
        // Review should be yellow (current)
        assert!(rendered.contains(YELLOW));
    }

    #[test]
    fn test_breadcrumb_render_truncation() {
        let mut breadcrumb = Breadcrumb::new();
        breadcrumb.enter_state(BreadcrumbState::Story);
        breadcrumb.enter_state(BreadcrumbState::Review);
        breadcrumb.enter_state(BreadcrumbState::Correct);
        breadcrumb.enter_state(BreadcrumbState::Review);
        breadcrumb.enter_state(BreadcrumbState::Commit);

        // Very narrow width should trigger truncation
        let rendered = breadcrumb.render(Some(30));
        // Should contain ellipsis when truncated
        assert!(rendered.contains("..."));
    }

    #[test]
    fn test_breadcrumb_render_no_truncation_when_fits() {
        let mut breadcrumb = Breadcrumb::new();
        breadcrumb.enter_state(BreadcrumbState::Story);

        // Wide width should not trigger truncation
        let rendered = breadcrumb.render(Some(200));
        // Should not contain ellipsis
        assert!(!rendered.contains("..."));
    }

    #[test]
    fn test_breadcrumb_state_equality() {
        assert_eq!(BreadcrumbState::Story, BreadcrumbState::Story);
        assert_ne!(BreadcrumbState::Story, BreadcrumbState::Review);
    }

    // ========================================================================
    // US-010: ProgressContext tests for overall progress context
    // ========================================================================

    #[test]
    fn test_progress_context_new() {
        let ctx = ProgressContext::new("US-001", 2, 5);
        assert_eq!(ctx.story_id, Some("US-001".to_string()));
        assert_eq!(ctx.story_index, Some(2));
        assert_eq!(ctx.total_stories, Some(5));
        assert_eq!(ctx.current_phase, None);
    }

    #[test]
    fn test_progress_context_with_phase() {
        let ctx = ProgressContext::with_phase("US-001", 2, 5, "Review");
        assert_eq!(ctx.story_id, Some("US-001".to_string()));
        assert_eq!(ctx.story_index, Some(2));
        assert_eq!(ctx.total_stories, Some(5));
        assert_eq!(ctx.current_phase, Some("Review".to_string()));
    }

    #[test]
    fn test_progress_context_set_phase() {
        let mut ctx = ProgressContext::new("US-001", 2, 5);
        assert_eq!(ctx.current_phase, None);

        ctx.set_phase("Correct");
        assert_eq!(ctx.current_phase, Some("Correct".to_string()));
    }

    #[test]
    fn test_progress_context_format_story_progress() {
        let ctx = ProgressContext::new("US-001", 2, 5);
        assert_eq!(ctx.format_story_progress(), Some("[US-001 2/5]".to_string()));
    }

    #[test]
    fn test_progress_context_format_story_progress_default() {
        let ctx = ProgressContext::default();
        assert_eq!(ctx.format_story_progress(), None);
    }

    #[test]
    fn test_progress_context_dual_context_both_present() {
        let ctx = ProgressContext::new("US-001", 2, 5);
        let iter_info = Some(IterationInfo::with_phase("Review", 1, 3));

        let result = ctx.format_dual_context(&iter_info);
        assert_eq!(result, Some("[US-001 2/5 | Review 1/3]".to_string()));
    }

    #[test]
    fn test_progress_context_dual_context_story_only() {
        let ctx = ProgressContext::new("US-001", 2, 5);
        let iter_info: Option<IterationInfo> = None;

        let result = ctx.format_dual_context(&iter_info);
        assert_eq!(result, Some("[US-001 2/5]".to_string()));
    }

    #[test]
    fn test_progress_context_dual_context_iter_only() {
        let ctx = ProgressContext::default();
        let iter_info = Some(IterationInfo::with_phase("Review", 1, 3));

        let result = ctx.format_dual_context(&iter_info);
        assert_eq!(result, Some("[Review 1/3]".to_string()));
    }

    #[test]
    fn test_progress_context_dual_context_neither_present() {
        let ctx = ProgressContext::default();
        let iter_info: Option<IterationInfo> = None;

        let result = ctx.format_dual_context(&iter_info);
        assert_eq!(result, None);
    }

    #[test]
    fn test_progress_context_dual_context_with_correct() {
        let ctx = ProgressContext::new("US-002", 3, 10);
        let iter_info = Some(IterationInfo::with_phase("Correct", 2, 3));

        let result = ctx.format_dual_context(&iter_info);
        assert_eq!(result, Some("[US-002 3/10 | Correct 2/3]".to_string()));
    }

    #[test]
    fn test_progress_context_dual_context_with_commit() {
        let ctx = ProgressContext::new("US-001", 2, 5);
        let iter_info = Some(IterationInfo::phase_only("Commit"));

        let result = ctx.format_dual_context(&iter_info);
        assert_eq!(result, Some("[US-001 2/5 | Commit]".to_string()));
    }
}
