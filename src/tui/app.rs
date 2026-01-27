//! TUI application state management.
//!
//! This module contains the `TuiApp` struct which holds all state needed
//! for rendering the TUI. The state is updated by `TuiDisplay` methods
//! and read by the rendering code in `ui.rs`.

use crate::display::StoryResult;
use crate::state::MachineState;
use std::collections::VecDeque;
use std::time::Instant;

/// Maximum number of output lines to keep in the buffer.
/// This prevents unbounded memory growth for long-running sessions.
const MAX_OUTPUT_LINES: usize = 1000;

/// TUI application state.
///
/// This struct holds all the state needed to render the TUI, including:
/// - Project and spec information
/// - Current state machine state
/// - Story progress
/// - Claude output buffer
/// - Error information
/// - Run summary
///
/// All fields are private and accessed through getter methods to ensure
/// encapsulation and thread safety (when wrapped in a Mutex).
#[derive(Debug)]
pub struct TuiApp {
    // ========================================================================
    // Project info
    // ========================================================================
    project_name: String,
    spec_name: String,

    // ========================================================================
    // State machine
    // ========================================================================
    state: MachineState,
    phase: String,

    // ========================================================================
    // Story progress
    // ========================================================================
    current_story_id: Option<String>,
    current_story_title: Option<String>,
    last_completed_story_id: Option<String>,
    iteration: u32,
    total_stories: usize,
    completed_stories: usize,

    // ========================================================================
    // Review progress
    // ========================================================================
    review_current: u32,
    review_max: u32,
    review_passed: bool,
    review_skipped: bool,
    issues_found: bool,
    max_review_reached: bool,

    // ========================================================================
    // Output buffer
    // ========================================================================
    output_lines: VecDeque<String>,
    info_messages: Vec<String>,
    breadcrumb: String,

    // ========================================================================
    // Error state
    // ========================================================================
    error: Option<ErrorInfo>,

    // ========================================================================
    // Run summary
    // ========================================================================
    run_summary: Option<RunSummary>,
    all_complete: bool,

    // ========================================================================
    // PR/Push status
    // ========================================================================
    pr_status: Option<(String, Option<String>)>, // (status, optional url)
    push_status: Option<String>,

    // ========================================================================
    // Timing
    // ========================================================================
    start_time: Instant,

    // ========================================================================
    // Exit control
    // ========================================================================
    /// Flag to signal the TUI should exit (set when run completes or user quits).
    should_exit: bool,
    /// Optional message to display when exiting (e.g., user quit early).
    exit_message: Option<String>,
}

/// Error information for display.
#[derive(Debug, Clone)]
pub struct ErrorInfo {
    pub error_type: String,
    pub message: String,
    pub exit_code: Option<i32>,
    pub stderr: Option<String>,
}

/// Run summary information.
#[derive(Debug, Clone)]
pub struct RunSummary {
    pub total_stories: usize,
    pub completed_stories: usize,
    pub total_iterations: u32,
    pub total_duration_secs: u64,
    pub story_results: Vec<StoryResult>,
}

impl TuiApp {
    /// Create a new TUI application state with default values.
    pub fn new() -> Self {
        Self {
            project_name: String::new(),
            spec_name: String::new(),
            state: MachineState::Idle,
            phase: String::new(),
            current_story_id: None,
            current_story_title: None,
            last_completed_story_id: None,
            iteration: 0,
            total_stories: 0,
            completed_stories: 0,
            review_current: 0,
            review_max: 0,
            review_passed: false,
            review_skipped: false,
            issues_found: false,
            max_review_reached: false,
            output_lines: VecDeque::new(),
            info_messages: Vec::new(),
            breadcrumb: String::new(),
            error: None,
            run_summary: None,
            all_complete: false,
            pr_status: None,
            push_status: None,
            start_time: Instant::now(),
            should_exit: false,
            exit_message: None,
        }
    }

    // ========================================================================
    // Project info setters
    // ========================================================================

    /// Set the project name.
    pub fn set_project_name(&mut self, name: &str) {
        self.project_name = name.to_string();
    }

    /// Set the spec name (usually the branch name).
    pub fn set_spec_name(&mut self, name: &str) {
        self.spec_name = name.to_string();
    }

    // ========================================================================
    // State machine setters
    // ========================================================================

    /// Set the current state machine state.
    pub fn set_state(&mut self, state: MachineState) {
        self.state = state;
    }

    /// Set the current phase name (e.g., "RUNNING", "REVIEWING").
    pub fn set_phase(&mut self, phase: &str) {
        self.phase = phase.to_string();
    }

    // ========================================================================
    // Story progress setters
    // ========================================================================

    /// Set the current story being processed.
    pub fn set_current_story(&mut self, id: &str, title: &str) {
        self.current_story_id = Some(id.to_string());
        self.current_story_title = Some(title.to_string());
    }

    /// Set the current iteration number.
    pub fn set_iteration(&mut self, iteration: u32) {
        self.iteration = iteration;
    }

    /// Set the total number of stories.
    pub fn set_total_stories(&mut self, total: usize) {
        self.total_stories = total;
    }

    /// Set the number of completed stories.
    pub fn set_completed_stories(&mut self, completed: usize) {
        self.completed_stories = completed;
    }

    /// Increment the completed stories count by 1.
    pub fn increment_completed_stories(&mut self) {
        self.completed_stories += 1;
    }

    /// Mark a specific story as complete.
    ///
    /// Note: This does NOT increment `completed_stories`. Progress tracking
    /// is handled by `set_completed_stories` which receives authoritative
    /// counts from the runner via `tasks_progress` / `full_progress`.
    pub fn mark_story_complete(&mut self, story_id: &str) {
        // Store the completed story ID for potential display purposes
        // The actual progress count is set via set_completed_stories
        self.last_completed_story_id = Some(story_id.to_string());
    }

    /// Set the all stories complete flag.
    pub fn set_all_complete(&mut self) {
        self.all_complete = true;
    }

    // ========================================================================
    // Review progress setters
    // ========================================================================

    /// Set the review progress.
    pub fn set_review_progress(&mut self, current: u32, max: u32) {
        self.review_current = current;
        self.review_max = max;
    }

    /// Mark the review as passed.
    pub fn set_review_passed(&mut self) {
        self.review_passed = true;
    }

    /// Mark the review as skipped.
    pub fn set_review_skipped(&mut self) {
        self.review_skipped = true;
    }

    /// Mark that issues were found during review.
    pub fn set_issues_found(&mut self) {
        self.issues_found = true;
    }

    /// Mark that max review iterations were reached.
    pub fn set_max_review_iterations_reached(&mut self) {
        self.max_review_reached = true;
    }

    // ========================================================================
    // Output buffer methods
    // ========================================================================

    /// Append a line to the output buffer.
    ///
    /// Automatically trims old lines when the buffer exceeds `MAX_OUTPUT_LINES`.
    pub fn append_output(&mut self, line: &str) {
        self.output_lines.push_back(line.to_string());
        while self.output_lines.len() > MAX_OUTPUT_LINES {
            self.output_lines.pop_front();
        }
    }

    /// Append an info message.
    pub fn append_info(&mut self, msg: &str) {
        self.info_messages.push(msg.to_string());
    }

    /// Set the breadcrumb trail string.
    pub fn set_breadcrumb(&mut self, breadcrumb: String) {
        self.breadcrumb = breadcrumb;
    }

    // ========================================================================
    // Error methods
    // ========================================================================

    /// Set error information.
    pub fn set_error(
        &mut self,
        error_type: &str,
        message: &str,
        exit_code: Option<i32>,
        stderr: Option<&str>,
    ) {
        self.error = Some(ErrorInfo {
            error_type: error_type.to_string(),
            message: message.to_string(),
            exit_code,
            stderr: stderr.map(String::from),
        });
    }

    // ========================================================================
    // Run summary methods
    // ========================================================================

    /// Set the run summary.
    pub fn set_run_summary(
        &mut self,
        total_stories: usize,
        completed_stories: usize,
        total_iterations: u32,
        total_duration_secs: u64,
        story_results: &[StoryResult],
    ) {
        self.run_summary = Some(RunSummary {
            total_stories,
            completed_stories,
            total_iterations,
            total_duration_secs,
            story_results: story_results.to_vec(),
        });
    }

    // ========================================================================
    // PR/Push status methods
    // ========================================================================

    /// Set the PR status.
    pub fn set_pr_status(&mut self, status: &str, url: Option<&str>) {
        self.pr_status = Some((status.to_string(), url.map(String::from)));
    }

    /// Set the push status.
    pub fn set_push_status(&mut self, status: &str) {
        self.push_status = Some(status.to_string());
    }

    // ========================================================================
    // Getters
    // ========================================================================

    /// Get the project name.
    pub fn project_name(&self) -> &str {
        &self.project_name
    }

    /// Get the spec name.
    pub fn spec_name(&self) -> &str {
        &self.spec_name
    }

    /// Get the current state.
    pub fn state(&self) -> MachineState {
        self.state
    }

    /// Get the current phase.
    pub fn phase(&self) -> &str {
        &self.phase
    }

    /// Get the current story ID.
    pub fn current_story_id(&self) -> Option<&str> {
        self.current_story_id.as_deref()
    }

    /// Get the current story title.
    pub fn current_story_title(&self) -> Option<&str> {
        self.current_story_title.as_deref()
    }

    /// Get the current iteration.
    pub fn iteration(&self) -> u32 {
        self.iteration
    }

    /// Get the total number of stories.
    pub fn total_stories(&self) -> usize {
        self.total_stories
    }

    /// Get the number of completed stories.
    pub fn completed_stories(&self) -> usize {
        self.completed_stories
    }

    /// Get the current review iteration.
    pub fn review_current(&self) -> u32 {
        self.review_current
    }

    /// Get the max review iterations.
    pub fn review_max(&self) -> u32 {
        self.review_max
    }

    /// Check if review passed.
    pub fn review_passed(&self) -> bool {
        self.review_passed
    }

    /// Check if review was skipped.
    pub fn review_skipped(&self) -> bool {
        self.review_skipped
    }

    /// Check if issues were found.
    pub fn issues_found(&self) -> bool {
        self.issues_found
    }

    /// Check if max review iterations were reached.
    pub fn max_review_reached(&self) -> bool {
        self.max_review_reached
    }

    /// Get the output lines as a joined string.
    pub fn output(&self) -> String {
        self.output_lines
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Get the output lines iterator.
    pub fn output_lines(&self) -> impl Iterator<Item = &str> {
        self.output_lines.iter().map(|s| s.as_str())
    }

    /// Get the info messages.
    pub fn info_messages(&self) -> &[String] {
        &self.info_messages
    }

    /// Get the breadcrumb trail.
    pub fn breadcrumb(&self) -> &str {
        &self.breadcrumb
    }

    /// Check if there is an error.
    pub fn has_error(&self) -> bool {
        self.error.is_some()
    }

    /// Get the error information.
    pub fn error(&self) -> Option<&ErrorInfo> {
        self.error.as_ref()
    }

    /// Get the run summary.
    pub fn run_summary(&self) -> Option<&RunSummary> {
        self.run_summary.as_ref()
    }

    /// Check if all stories are complete.
    pub fn all_complete(&self) -> bool {
        self.all_complete
    }

    /// Get the PR status.
    pub fn pr_status(&self) -> Option<(&str, Option<&str>)> {
        self.pr_status
            .as_ref()
            .map(|(s, u)| (s.as_str(), u.as_deref()))
    }

    /// Get the push status.
    pub fn push_status(&self) -> Option<&str> {
        self.push_status.as_deref()
    }

    /// Get the elapsed time since the app was created.
    pub fn elapsed_secs(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }

    // ========================================================================
    // Exit control methods
    // ========================================================================

    /// Signal that the TUI should exit.
    pub fn request_exit(&mut self) {
        self.should_exit = true;
    }

    /// Signal that the TUI should exit with a message.
    pub fn request_exit_with_message(&mut self, message: &str) {
        self.should_exit = true;
        self.exit_message = Some(message.to_string());
    }

    /// Check if the TUI should exit.
    pub fn should_exit(&self) -> bool {
        self.should_exit
    }

    /// Get the exit message, if any.
    pub fn exit_message(&self) -> Option<&str> {
        self.exit_message.as_deref()
    }

    /// Check if the run is complete (either all stories done or failed).
    ///
    /// Returns true if the state is Completed, Failed, or if all_complete is set.
    pub fn is_run_complete(&self) -> bool {
        matches!(self.state, MachineState::Completed | MachineState::Failed)
            || self.all_complete
            || self.run_summary.is_some()
    }
}

impl Default for TuiApp {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // Creation tests
    // ========================================================================

    #[test]
    fn test_tui_app_new() {
        let app = TuiApp::new();
        assert!(app.project_name.is_empty());
        assert!(app.spec_name.is_empty());
        assert_eq!(app.state, MachineState::Idle);
        assert_eq!(app.iteration, 0);
        assert_eq!(app.total_stories, 0);
        assert_eq!(app.completed_stories, 0);
    }

    #[test]
    fn test_tui_app_default() {
        let app = TuiApp::default();
        assert!(app.project_name.is_empty());
    }

    // ========================================================================
    // Project info tests
    // ========================================================================

    #[test]
    fn test_set_project_name() {
        let mut app = TuiApp::new();
        app.set_project_name("MyProject");
        assert_eq!(app.project_name(), "MyProject");
    }

    #[test]
    fn test_set_spec_name() {
        let mut app = TuiApp::new();
        app.set_spec_name("feature-branch");
        assert_eq!(app.spec_name(), "feature-branch");
    }

    // ========================================================================
    // State machine tests
    // ========================================================================

    #[test]
    fn test_set_state() {
        let mut app = TuiApp::new();
        app.set_state(MachineState::RunningClaude);
        assert_eq!(app.state(), MachineState::RunningClaude);
    }

    #[test]
    fn test_set_phase() {
        let mut app = TuiApp::new();
        app.set_phase("RUNNING");
        assert_eq!(app.phase(), "RUNNING");
    }

    // ========================================================================
    // Story progress tests
    // ========================================================================

    #[test]
    fn test_set_current_story() {
        let mut app = TuiApp::new();
        app.set_current_story("US-001", "Test Story");
        assert_eq!(app.current_story_id(), Some("US-001"));
        assert_eq!(app.current_story_title(), Some("Test Story"));
    }

    #[test]
    fn test_set_iteration() {
        let mut app = TuiApp::new();
        app.set_iteration(5);
        assert_eq!(app.iteration(), 5);
    }

    #[test]
    fn test_set_total_stories() {
        let mut app = TuiApp::new();
        app.set_total_stories(10);
        assert_eq!(app.total_stories(), 10);
    }

    #[test]
    fn test_set_completed_stories() {
        let mut app = TuiApp::new();
        app.set_completed_stories(3);
        assert_eq!(app.completed_stories(), 3);
    }

    #[test]
    fn test_increment_completed_stories() {
        let mut app = TuiApp::new();
        app.set_completed_stories(3);
        app.increment_completed_stories();
        assert_eq!(app.completed_stories(), 4);
    }

    #[test]
    fn test_mark_story_complete_stores_id() {
        let mut app = TuiApp::new();
        app.set_completed_stories(2);
        app.mark_story_complete("US-003");
        // mark_story_complete should NOT increment the counter
        // (progress is tracked via set_completed_stories from tasks_progress)
        assert_eq!(app.completed_stories(), 2);
        // But it should store the story ID
        assert_eq!(app.last_completed_story_id, Some("US-003".to_string()));
    }

    #[test]
    fn test_set_all_complete() {
        let mut app = TuiApp::new();
        assert!(!app.all_complete());
        app.set_all_complete();
        assert!(app.all_complete());
    }

    // ========================================================================
    // Review progress tests
    // ========================================================================

    #[test]
    fn test_set_review_progress() {
        let mut app = TuiApp::new();
        app.set_review_progress(2, 3);
        assert_eq!(app.review_current(), 2);
        assert_eq!(app.review_max(), 3);
    }

    #[test]
    fn test_set_review_passed() {
        let mut app = TuiApp::new();
        assert!(!app.review_passed());
        app.set_review_passed();
        assert!(app.review_passed());
    }

    #[test]
    fn test_set_review_skipped() {
        let mut app = TuiApp::new();
        assert!(!app.review_skipped());
        app.set_review_skipped();
        assert!(app.review_skipped());
    }

    #[test]
    fn test_set_issues_found() {
        let mut app = TuiApp::new();
        assert!(!app.issues_found());
        app.set_issues_found();
        assert!(app.issues_found());
    }

    #[test]
    fn test_set_max_review_iterations_reached() {
        let mut app = TuiApp::new();
        assert!(!app.max_review_reached());
        app.set_max_review_iterations_reached();
        assert!(app.max_review_reached());
    }

    // ========================================================================
    // Output buffer tests
    // ========================================================================

    #[test]
    fn test_append_output() {
        let mut app = TuiApp::new();
        app.append_output("Line 1");
        app.append_output("Line 2");
        let output = app.output();
        assert!(output.contains("Line 1"));
        assert!(output.contains("Line 2"));
    }

    #[test]
    fn test_append_output_trims_old_lines() {
        let mut app = TuiApp::new();
        // Add more than MAX_OUTPUT_LINES
        for i in 0..MAX_OUTPUT_LINES + 100 {
            app.append_output(&format!("Line {}", i));
        }
        // Should only have MAX_OUTPUT_LINES
        assert_eq!(app.output_lines.len(), MAX_OUTPUT_LINES);
        // First line should be gone
        assert!(!app.output().contains("Line 0"));
        // Last line should be present
        assert!(app
            .output()
            .contains(&format!("Line {}", MAX_OUTPUT_LINES + 99)));
    }

    #[test]
    fn test_output_lines_iterator() {
        let mut app = TuiApp::new();
        app.append_output("Line 1");
        app.append_output("Line 2");
        let lines: Vec<_> = app.output_lines().collect();
        assert_eq!(lines, vec!["Line 1", "Line 2"]);
    }

    #[test]
    fn test_append_info() {
        let mut app = TuiApp::new();
        app.append_info("Info 1");
        app.append_info("Info 2");
        assert_eq!(app.info_messages().len(), 2);
        assert_eq!(app.info_messages()[0], "Info 1");
    }

    #[test]
    fn test_set_breadcrumb() {
        let mut app = TuiApp::new();
        app.set_breadcrumb("Story > Review > Correct".to_string());
        assert_eq!(app.breadcrumb(), "Story > Review > Correct");
    }

    // ========================================================================
    // Error tests
    // ========================================================================

    #[test]
    fn test_set_error() {
        let mut app = TuiApp::new();
        assert!(!app.has_error());
        app.set_error("TestError", "Error message", Some(1), Some("stderr"));
        assert!(app.has_error());
        let error = app.error().unwrap();
        assert_eq!(error.error_type, "TestError");
        assert_eq!(error.message, "Error message");
        assert_eq!(error.exit_code, Some(1));
        assert_eq!(error.stderr, Some("stderr".to_string()));
    }

    #[test]
    fn test_error_without_optional_fields() {
        let mut app = TuiApp::new();
        app.set_error("TestError", "Error message", None, None);
        let error = app.error().unwrap();
        assert!(error.exit_code.is_none());
        assert!(error.stderr.is_none());
    }

    // ========================================================================
    // Run summary tests
    // ========================================================================

    #[test]
    fn test_set_run_summary() {
        let mut app = TuiApp::new();
        assert!(app.run_summary().is_none());

        let results = vec![StoryResult {
            id: "US-001".to_string(),
            title: "Test".to_string(),
            passed: true,
            duration_secs: 60,
        }];
        app.set_run_summary(5, 5, 5, 300, &results);

        let summary = app.run_summary().unwrap();
        assert_eq!(summary.total_stories, 5);
        assert_eq!(summary.completed_stories, 5);
        assert_eq!(summary.total_iterations, 5);
        assert_eq!(summary.total_duration_secs, 300);
        assert_eq!(summary.story_results.len(), 1);
    }

    // ========================================================================
    // PR/Push status tests
    // ========================================================================

    #[test]
    fn test_set_pr_status_with_url() {
        let mut app = TuiApp::new();
        app.set_pr_status("created", Some("https://github.com/pr/1"));
        let (status, url) = app.pr_status().unwrap();
        assert_eq!(status, "created");
        assert_eq!(url, Some("https://github.com/pr/1"));
    }

    #[test]
    fn test_set_pr_status_without_url() {
        let mut app = TuiApp::new();
        app.set_pr_status("skipped", None);
        let (status, url) = app.pr_status().unwrap();
        assert_eq!(status, "skipped");
        assert!(url.is_none());
    }

    #[test]
    fn test_set_push_status() {
        let mut app = TuiApp::new();
        app.set_push_status("success");
        assert_eq!(app.push_status(), Some("success"));
    }

    // ========================================================================
    // Timing tests
    // ========================================================================

    #[test]
    fn test_elapsed_secs() {
        let app = TuiApp::new();
        // Just verify it returns a value (timing is inherently imprecise in tests)
        let _ = app.elapsed_secs();
    }

    // ========================================================================
    // Exit control tests (US-006)
    // ========================================================================

    #[test]
    fn test_should_exit_defaults_to_false() {
        let app = TuiApp::new();
        assert!(!app.should_exit());
    }

    #[test]
    fn test_request_exit_sets_flag() {
        let mut app = TuiApp::new();
        app.request_exit();
        assert!(app.should_exit());
    }

    #[test]
    fn test_request_exit_with_message() {
        let mut app = TuiApp::new();
        app.request_exit_with_message("Test message");
        assert!(app.should_exit());
        assert_eq!(app.exit_message(), Some("Test message"));
    }

    #[test]
    fn test_exit_message_defaults_to_none() {
        let app = TuiApp::new();
        assert!(app.exit_message().is_none());
    }

    #[test]
    fn test_is_run_complete_idle_state() {
        let app = TuiApp::new();
        assert!(!app.is_run_complete());
    }

    #[test]
    fn test_is_run_complete_running_state() {
        let mut app = TuiApp::new();
        app.set_state(MachineState::RunningClaude);
        assert!(!app.is_run_complete());
    }

    #[test]
    fn test_is_run_complete_completed_state() {
        let mut app = TuiApp::new();
        app.set_state(MachineState::Completed);
        assert!(app.is_run_complete());
    }

    #[test]
    fn test_is_run_complete_failed_state() {
        let mut app = TuiApp::new();
        app.set_state(MachineState::Failed);
        assert!(app.is_run_complete());
    }

    #[test]
    fn test_is_run_complete_all_complete_flag() {
        let mut app = TuiApp::new();
        app.set_all_complete();
        assert!(app.is_run_complete());
    }

    #[test]
    fn test_is_run_complete_with_run_summary() {
        let mut app = TuiApp::new();
        app.set_run_summary(5, 5, 5, 300, &[]);
        assert!(app.is_run_complete());
    }
}
