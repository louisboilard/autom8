//! Display abstraction layer for autom8.
//!
//! This module provides a trait-based abstraction that decouples the runner from
//! specific display implementations. This enables the TUI to be completely isolated—
//! when disabled, no TUI code is loaded or referenced by the runner.
//!
//! # Architecture
//!
//! The `DisplayAdapter` trait defines all display operations needed by the runner.
//! Two implementations are provided:
//!
//! - `CliDisplay`: Uses the existing `output.rs` functions for CLI output
//! - Future: `TuiDisplay`: Will use Ratatui for rich terminal interface
//!
//! The runner accepts a `Box<dyn DisplayAdapter>` which allows runtime switching
//! between implementations based on the `use_tui` configuration.

use crate::output::{
    print_all_complete, print_breadcrumb_trail, print_claude_output, print_error_panel,
    print_full_progress, print_generating_spec, print_header, print_info, print_issues_found,
    print_iteration_complete, print_iteration_start, print_max_review_iterations,
    print_phase_banner, print_phase_footer, print_pr_already_exists, print_pr_skipped,
    print_pr_success, print_pr_updated, print_proceeding_to_implementation, print_project_info,
    print_push_already_up_to_date, print_push_success, print_pushing_branch, print_review_passed,
    print_reviewing, print_run_summary, print_skip_review, print_spec_generated, print_spec_loaded,
    print_state_transition, print_story_complete, print_tasks_progress,
};
// Re-export types needed by display operations
pub use crate::output::{BannerColor, StoryResult};
use crate::progress::Breadcrumb;
use crate::spec::Spec;
use crate::state::MachineState;
use std::path::Path;

/// Trait defining all display operations needed by the runner.
///
/// This abstraction allows different display implementations (CLI, TUI) to be
/// used interchangeably without changing the runner's logic.
///
/// # Implementation Notes
///
/// - All methods have default implementations that do nothing (no-op)
/// - `CliDisplay` overrides these with the existing `output.rs` functions
/// - Future `TuiDisplay` will override with Ratatui-based rendering
pub trait DisplayAdapter: Send {
    // ========================================================================
    // Header and project info
    // ========================================================================

    /// Print the application header banner.
    fn header(&self) {}

    /// Print project information from the spec.
    fn project_info(&self, _spec: &Spec) {}

    // ========================================================================
    // State transitions
    // ========================================================================

    /// Print a state transition message.
    fn state_transition(&self, _from: MachineState, _to: MachineState) {}

    // ========================================================================
    // Phase banners
    // ========================================================================

    /// Print a phase banner (e.g., "RUNNING", "REVIEWING").
    fn phase_banner(&self, _phase_name: &str, _color: BannerColor) {}

    /// Print a phase footer (bottom border).
    fn phase_footer(&self, _color: BannerColor) {}

    // ========================================================================
    // Story progress
    // ========================================================================

    /// Print iteration start information.
    fn iteration_start(&self, _iteration: u32, _story_id: &str, _story_title: &str) {}

    /// Print story completion information.
    fn story_complete(&self, _story_id: &str, _duration_secs: u64) {}

    /// Print iteration completion information.
    fn iteration_complete(&self, _iteration: u32) {}

    /// Print "all stories complete" message.
    fn all_complete(&self) {}

    // ========================================================================
    // Claude output streaming
    // ========================================================================

    /// Print a line of Claude output.
    fn claude_output(&self, _line: &str) {}

    // ========================================================================
    // Progress bars
    // ========================================================================

    /// Print tasks progress bar.
    fn tasks_progress(&self, _completed: usize, _total: usize) {}

    /// Print full progress (tasks + review).
    fn full_progress(
        &self,
        _tasks_completed: usize,
        _tasks_total: usize,
        _review_current: u32,
        _review_max: u32,
    ) {
    }

    // ========================================================================
    // Breadcrumb trail
    // ========================================================================

    /// Print the breadcrumb trail showing workflow journey.
    fn breadcrumb_trail(&self, _breadcrumb: &Breadcrumb) {}

    // ========================================================================
    // Review/correct phase messages
    // ========================================================================

    /// Print "reviewing" message.
    fn reviewing(&self, _iteration: u32, _max_iterations: u32) {}

    /// Print "review passed" message.
    fn review_passed(&self) {}

    /// Print "issues found" message.
    fn issues_found(&self, _iteration: u32, _max_iterations: u32) {}

    /// Print "skip review" message.
    fn skip_review(&self) {}

    /// Print "max review iterations reached" message.
    fn max_review_iterations(&self) {}

    // ========================================================================
    // Error display
    // ========================================================================

    /// Print an error panel with details.
    fn error_panel(
        &self,
        _error_type: &str,
        _message: &str,
        _exit_code: Option<i32>,
        _stderr: Option<&str>,
    ) {
    }

    // ========================================================================
    // Completion summary
    // ========================================================================

    /// Print the run summary.
    fn run_summary(
        &self,
        _total_stories: usize,
        _completed_stories: usize,
        _total_iterations: u32,
        _total_duration_secs: u64,
        _story_results: &[StoryResult],
    ) {
    }

    // ========================================================================
    // Spec operations
    // ========================================================================

    /// Print spec loaded message.
    fn spec_loaded(&self, _path: &Path, _size_bytes: u64) {}

    /// Print "generating spec" message.
    fn generating_spec(&self) {}

    /// Print spec generated success message.
    fn spec_generated(&self, _spec: &Spec, _output_path: &Path) {}

    /// Print "proceeding to implementation" message.
    fn proceeding_to_implementation(&self) {}

    // ========================================================================
    // PR and push operations
    // ========================================================================

    /// Print PR success message.
    fn pr_success(&self, _url: &str) {}

    /// Print PR already exists message.
    fn pr_already_exists(&self, _url: &str) {}

    /// Print PR skipped message.
    fn pr_skipped(&self, _reason: &str) {}

    /// Print PR updated message.
    fn pr_updated(&self, _url: &str) {}

    /// Print pushing branch message.
    fn pushing_branch(&self, _branch: &str) {}

    /// Print push success message.
    fn push_success(&self) {}

    /// Print push already up-to-date message.
    fn push_already_up_to_date(&self) {}

    // ========================================================================
    // Info/warning messages
    // ========================================================================

    /// Print an info message.
    fn info(&self, _msg: &str) {}

    // ========================================================================
    // Newline helper
    // ========================================================================

    /// Print a blank line (for spacing).
    fn newline(&self) {}
}

/// CLI display implementation using existing `output.rs` functions.
///
/// This implementation provides the same behavior as the current CLI output,
/// ensuring backwards compatibility when TUI mode is disabled.
#[derive(Debug, Default)]
pub struct CliDisplay;

impl CliDisplay {
    /// Create a new CLI display adapter.
    pub fn new() -> Self {
        Self
    }
}

impl DisplayAdapter for CliDisplay {
    // ========================================================================
    // Header and project info
    // ========================================================================

    fn header(&self) {
        print_header();
    }

    fn project_info(&self, spec: &Spec) {
        print_project_info(spec);
    }

    // ========================================================================
    // State transitions
    // ========================================================================

    fn state_transition(&self, from: MachineState, to: MachineState) {
        print_state_transition(from, to);
    }

    // ========================================================================
    // Phase banners
    // ========================================================================

    fn phase_banner(&self, phase_name: &str, color: BannerColor) {
        print_phase_banner(phase_name, color);
    }

    fn phase_footer(&self, color: BannerColor) {
        print_phase_footer(color);
    }

    // ========================================================================
    // Story progress
    // ========================================================================

    fn iteration_start(&self, iteration: u32, story_id: &str, story_title: &str) {
        print_iteration_start(iteration, story_id, story_title);
    }

    fn story_complete(&self, story_id: &str, duration_secs: u64) {
        print_story_complete(story_id, duration_secs);
    }

    fn iteration_complete(&self, iteration: u32) {
        print_iteration_complete(iteration);
    }

    fn all_complete(&self) {
        print_all_complete();
    }

    // ========================================================================
    // Claude output streaming
    // ========================================================================

    fn claude_output(&self, line: &str) {
        print_claude_output(line);
    }

    // ========================================================================
    // Progress bars
    // ========================================================================

    fn tasks_progress(&self, completed: usize, total: usize) {
        print_tasks_progress(completed, total);
    }

    fn full_progress(
        &self,
        tasks_completed: usize,
        tasks_total: usize,
        review_current: u32,
        review_max: u32,
    ) {
        print_full_progress(tasks_completed, tasks_total, review_current, review_max);
    }

    // ========================================================================
    // Breadcrumb trail
    // ========================================================================

    fn breadcrumb_trail(&self, breadcrumb: &Breadcrumb) {
        print_breadcrumb_trail(breadcrumb);
    }

    // ========================================================================
    // Review/correct phase messages
    // ========================================================================

    fn reviewing(&self, iteration: u32, max_iterations: u32) {
        print_reviewing(iteration, max_iterations);
    }

    fn review_passed(&self) {
        print_review_passed();
    }

    fn issues_found(&self, iteration: u32, max_iterations: u32) {
        print_issues_found(iteration, max_iterations);
    }

    fn skip_review(&self) {
        print_skip_review();
    }

    fn max_review_iterations(&self) {
        print_max_review_iterations();
    }

    // ========================================================================
    // Error display
    // ========================================================================

    fn error_panel(
        &self,
        error_type: &str,
        message: &str,
        exit_code: Option<i32>,
        stderr: Option<&str>,
    ) {
        print_error_panel(error_type, message, exit_code, stderr);
    }

    // ========================================================================
    // Completion summary
    // ========================================================================

    fn run_summary(
        &self,
        total_stories: usize,
        completed_stories: usize,
        total_iterations: u32,
        total_duration_secs: u64,
        story_results: &[StoryResult],
    ) {
        print_run_summary(
            total_stories,
            completed_stories,
            total_iterations,
            total_duration_secs,
            story_results,
        );
    }

    // ========================================================================
    // Spec operations
    // ========================================================================

    fn spec_loaded(&self, path: &Path, size_bytes: u64) {
        print_spec_loaded(path, size_bytes);
    }

    fn generating_spec(&self) {
        print_generating_spec();
    }

    fn spec_generated(&self, spec: &Spec, output_path: &Path) {
        print_spec_generated(spec, output_path);
    }

    fn proceeding_to_implementation(&self) {
        print_proceeding_to_implementation();
    }

    // ========================================================================
    // PR and push operations
    // ========================================================================

    fn pr_success(&self, url: &str) {
        print_pr_success(url);
    }

    fn pr_already_exists(&self, url: &str) {
        print_pr_already_exists(url);
    }

    fn pr_skipped(&self, reason: &str) {
        print_pr_skipped(reason);
    }

    fn pr_updated(&self, url: &str) {
        print_pr_updated(url);
    }

    fn pushing_branch(&self, branch: &str) {
        print_pushing_branch(branch);
    }

    fn push_success(&self) {
        print_push_success();
    }

    fn push_already_up_to_date(&self) {
        print_push_already_up_to_date();
    }

    // ========================================================================
    // Info/warning messages
    // ========================================================================

    fn info(&self, msg: &str) {
        print_info(msg);
    }

    // ========================================================================
    // Newline helper
    // ========================================================================

    fn newline(&self) {
        println!();
    }
}

/// Create a display adapter based on the configuration.
///
/// Returns `CliDisplay` when TUI is disabled, or `TuiDisplay` when TUI mode is enabled.
/// When TUI mode is enabled, the TUI render loop is automatically started.
pub fn create_display(use_tui: bool) -> Box<dyn DisplayAdapter> {
    if use_tui {
        let mut tui = crate::tui::TuiDisplay::new();
        // Start the TUI render loop - this initializes the terminal and spawns
        // the render thread. The TUI will be stopped automatically when dropped.
        if let Err(e) = tui.start() {
            eprintln!(
                "Warning: Failed to start TUI mode: {}. Falling back to CLI.",
                e
            );
            return Box::new(CliDisplay::new());
        }
        Box::new(tui)
    } else {
        Box::new(CliDisplay::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{Spec, UserStory};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Test display adapter that tracks method calls for verification.
    struct MockDisplay {
        state_transition_count: Arc<AtomicUsize>,
        phase_banner_count: Arc<AtomicUsize>,
        claude_output_count: Arc<AtomicUsize>,
        error_panel_count: Arc<AtomicUsize>,
    }

    impl MockDisplay {
        fn new() -> Self {
            Self {
                state_transition_count: Arc::new(AtomicUsize::new(0)),
                phase_banner_count: Arc::new(AtomicUsize::new(0)),
                claude_output_count: Arc::new(AtomicUsize::new(0)),
                error_panel_count: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    impl DisplayAdapter for MockDisplay {
        fn state_transition(&self, _from: MachineState, _to: MachineState) {
            self.state_transition_count.fetch_add(1, Ordering::SeqCst);
        }

        fn phase_banner(&self, _phase_name: &str, _color: BannerColor) {
            self.phase_banner_count.fetch_add(1, Ordering::SeqCst);
        }

        fn claude_output(&self, _line: &str) {
            self.claude_output_count.fetch_add(1, Ordering::SeqCst);
        }

        fn error_panel(
            &self,
            _error_type: &str,
            _message: &str,
            _exit_code: Option<i32>,
            _stderr: Option<&str>,
        ) {
            self.error_panel_count.fetch_add(1, Ordering::SeqCst);
        }
    }

    // ========================================================================
    // DisplayAdapter trait tests
    // ========================================================================

    #[test]
    fn test_display_adapter_default_implementations_are_noop() {
        // A minimal struct that relies on default implementations
        struct NoopDisplay;
        impl DisplayAdapter for NoopDisplay {}

        let display = NoopDisplay;

        // All default implementations should do nothing (no panic)
        display.header();
        display.state_transition(MachineState::Idle, MachineState::Initializing);
        display.phase_banner("TEST", BannerColor::Cyan);
        display.phase_footer(BannerColor::Cyan);
        display.iteration_start(1, "US-001", "Test Story");
        display.story_complete("US-001", 60);
        display.iteration_complete(1);
        display.all_complete();
        display.claude_output("test line");
        display.tasks_progress(1, 5);
        display.full_progress(1, 5, 1, 3);
        display.reviewing(1, 3);
        display.review_passed();
        display.issues_found(1, 3);
        display.skip_review();
        display.max_review_iterations();
        display.error_panel("Test Error", "Test message", Some(1), Some("stderr"));
        display.info("test info");
        display.newline();
    }

    #[test]
    fn test_mock_display_tracks_state_transitions() {
        let display = MockDisplay::new();
        let count = display.state_transition_count.clone();

        assert_eq!(count.load(Ordering::SeqCst), 0);

        display.state_transition(MachineState::Idle, MachineState::Initializing);
        assert_eq!(count.load(Ordering::SeqCst), 1);

        display.state_transition(MachineState::Initializing, MachineState::PickingStory);
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_mock_display_tracks_phase_banners() {
        let display = MockDisplay::new();
        let count = display.phase_banner_count.clone();

        assert_eq!(count.load(Ordering::SeqCst), 0);

        display.phase_banner("RUNNING", BannerColor::Cyan);
        assert_eq!(count.load(Ordering::SeqCst), 1);

        display.phase_banner("REVIEWING", BannerColor::Yellow);
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_mock_display_tracks_claude_output() {
        let display = MockDisplay::new();
        let count = display.claude_output_count.clone();

        assert_eq!(count.load(Ordering::SeqCst), 0);

        display.claude_output("Line 1");
        display.claude_output("Line 2");
        display.claude_output("Line 3");

        assert_eq!(count.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn test_mock_display_tracks_error_panels() {
        let display = MockDisplay::new();
        let count = display.error_panel_count.clone();

        assert_eq!(count.load(Ordering::SeqCst), 0);

        display.error_panel("Claude Error", "Process failed", Some(1), None);
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    // ========================================================================
    // CliDisplay tests
    // ========================================================================

    #[test]
    fn test_cli_display_new() {
        let display = CliDisplay::new();
        // Should create successfully
        let _ = display;
    }

    #[test]
    fn test_cli_display_default() {
        let display = CliDisplay::default();
        // Should create successfully
        let _ = display;
    }

    #[test]
    fn test_cli_display_implements_display_adapter() {
        fn accepts_display_adapter(_: &dyn DisplayAdapter) {}

        let display = CliDisplay::new();
        accepts_display_adapter(&display);
    }

    #[test]
    fn test_cli_display_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<CliDisplay>();
    }

    #[test]
    fn test_create_display_returns_box_dyn_display_adapter() {
        let display = create_display(false);
        // Should be usable as Box<dyn DisplayAdapter>
        display.header();
        display.newline();
    }

    #[test]
    fn test_create_display_with_tui_false_returns_cli_display() {
        let display = create_display(false);
        // Currently always returns CliDisplay
        display.info("test");
    }

    #[test]
    fn test_create_display_with_tui_true_returns_tui_display() {
        // TUI mode returns TuiDisplay
        let display = create_display(true);
        display.info("test");
    }

    // ========================================================================
    // CliDisplay method coverage tests (verify no panic)
    // ========================================================================

    fn create_test_spec() -> Spec {
        Spec {
            project: "TestProject".into(),
            branch_name: "test-branch".into(),
            description: "A test project".into(),
            user_stories: vec![UserStory {
                id: "US-001".into(),
                title: "Test Story".into(),
                description: "A test story".into(),
                acceptance_criteria: vec!["Test criterion".into()],
                priority: 1,
                passes: false,
                notes: String::new(),
            }],
        }
    }

    #[test]
    fn test_cli_display_header_no_panic() {
        let display = CliDisplay::new();
        display.header();
    }

    #[test]
    fn test_cli_display_project_info_no_panic() {
        let display = CliDisplay::new();
        let spec = create_test_spec();
        display.project_info(&spec);
    }

    #[test]
    fn test_cli_display_state_transition_no_panic() {
        let display = CliDisplay::new();
        display.state_transition(MachineState::Idle, MachineState::Initializing);
    }

    #[test]
    fn test_cli_display_phase_banner_no_panic() {
        let display = CliDisplay::new();
        display.phase_banner("RUNNING", BannerColor::Cyan);
        display.phase_banner("REVIEWING", BannerColor::Yellow);
        display.phase_banner("CORRECTING", BannerColor::Yellow);
        display.phase_banner("COMMITTING", BannerColor::Cyan);
    }

    #[test]
    fn test_cli_display_phase_footer_no_panic() {
        let display = CliDisplay::new();
        display.phase_footer(BannerColor::Cyan);
        display.phase_footer(BannerColor::Yellow);
        display.phase_footer(BannerColor::Green);
        display.phase_footer(BannerColor::Red);
    }

    #[test]
    fn test_cli_display_iteration_start_no_panic() {
        let display = CliDisplay::new();
        display.iteration_start(1, "US-001", "Test Story");
    }

    #[test]
    fn test_cli_display_story_complete_no_panic() {
        let display = CliDisplay::new();
        display.story_complete("US-001", 120);
    }

    #[test]
    fn test_cli_display_iteration_complete_no_panic() {
        let display = CliDisplay::new();
        display.iteration_complete(1);
    }

    #[test]
    fn test_cli_display_all_complete_no_panic() {
        let display = CliDisplay::new();
        display.all_complete();
    }

    #[test]
    fn test_cli_display_claude_output_no_panic() {
        let display = CliDisplay::new();
        display.claude_output("Test output line");
    }

    #[test]
    fn test_cli_display_tasks_progress_no_panic() {
        let display = CliDisplay::new();
        display.tasks_progress(3, 8);
    }

    #[test]
    fn test_cli_display_full_progress_no_panic() {
        let display = CliDisplay::new();
        display.full_progress(3, 8, 1, 3);
    }

    #[test]
    fn test_cli_display_breadcrumb_trail_no_panic() {
        use crate::progress::{Breadcrumb, BreadcrumbState};

        let display = CliDisplay::new();
        let mut breadcrumb = Breadcrumb::new();
        breadcrumb.enter_state(BreadcrumbState::Story);
        display.breadcrumb_trail(&breadcrumb);
    }

    #[test]
    fn test_cli_display_reviewing_no_panic() {
        let display = CliDisplay::new();
        display.reviewing(1, 3);
    }

    #[test]
    fn test_cli_display_review_passed_no_panic() {
        let display = CliDisplay::new();
        display.review_passed();
    }

    #[test]
    fn test_cli_display_issues_found_no_panic() {
        let display = CliDisplay::new();
        display.issues_found(1, 3);
    }

    #[test]
    fn test_cli_display_skip_review_no_panic() {
        let display = CliDisplay::new();
        display.skip_review();
    }

    #[test]
    fn test_cli_display_max_review_iterations_no_panic() {
        let display = CliDisplay::new();
        display.max_review_iterations();
    }

    #[test]
    fn test_cli_display_error_panel_no_panic() {
        let display = CliDisplay::new();
        display.error_panel(
            "Claude Error",
            "Process failed",
            Some(1),
            Some("stderr output"),
        );
        display.error_panel("API Error", "Connection refused", None, None);
    }

    #[test]
    fn test_cli_display_run_summary_no_panic() {
        let display = CliDisplay::new();
        let results = vec![StoryResult {
            id: "US-001".to_string(),
            title: "Test Story".to_string(),
            passed: true,
            duration_secs: 120,
        }];
        display.run_summary(5, 5, 5, 600, &results);
    }

    #[test]
    fn test_cli_display_spec_loaded_no_panic() {
        let display = CliDisplay::new();
        display.spec_loaded(Path::new("spec-test.md"), 1024);
    }

    #[test]
    fn test_cli_display_generating_spec_no_panic() {
        let display = CliDisplay::new();
        display.generating_spec();
    }

    #[test]
    fn test_cli_display_spec_generated_no_panic() {
        let display = CliDisplay::new();
        let spec = create_test_spec();
        display.spec_generated(&spec, Path::new("spec-test.json"));
    }

    #[test]
    fn test_cli_display_proceeding_to_implementation_no_panic() {
        let display = CliDisplay::new();
        display.proceeding_to_implementation();
    }

    #[test]
    fn test_cli_display_pr_success_no_panic() {
        let display = CliDisplay::new();
        display.pr_success("https://github.com/owner/repo/pull/1");
    }

    #[test]
    fn test_cli_display_pr_already_exists_no_panic() {
        let display = CliDisplay::new();
        display.pr_already_exists("https://github.com/owner/repo/pull/1");
    }

    #[test]
    fn test_cli_display_pr_skipped_no_panic() {
        let display = CliDisplay::new();
        display.pr_skipped("No commits were made");
    }

    #[test]
    fn test_cli_display_pr_updated_no_panic() {
        let display = CliDisplay::new();
        display.pr_updated("https://github.com/owner/repo/pull/1");
    }

    #[test]
    fn test_cli_display_pushing_branch_no_panic() {
        let display = CliDisplay::new();
        display.pushing_branch("feature/test");
    }

    #[test]
    fn test_cli_display_push_success_no_panic() {
        let display = CliDisplay::new();
        display.push_success();
    }

    #[test]
    fn test_cli_display_push_already_up_to_date_no_panic() {
        let display = CliDisplay::new();
        display.push_already_up_to_date();
    }

    #[test]
    fn test_cli_display_info_no_panic() {
        let display = CliDisplay::new();
        display.info("Test info message");
    }

    #[test]
    fn test_cli_display_newline_no_panic() {
        let display = CliDisplay::new();
        display.newline();
    }

    // ========================================================================
    // Box<dyn DisplayAdapter> tests
    // ========================================================================

    #[test]
    fn test_boxed_display_adapter_is_object_safe() {
        let display: Box<dyn DisplayAdapter> = Box::new(CliDisplay::new());
        display.header();
        display.state_transition(MachineState::Idle, MachineState::Initializing);
    }

    #[test]
    fn test_boxed_display_adapter_can_be_stored() {
        struct Container {
            display: Box<dyn DisplayAdapter>,
        }

        let container = Container {
            display: Box::new(CliDisplay::new()),
        };
        container.display.info("test");
    }

    #[test]
    fn test_boxed_display_adapter_can_be_swapped() {
        let mut display: Box<dyn DisplayAdapter> = Box::new(CliDisplay::new());
        display.info("first");

        // Swap to different instance (same type for now, but demonstrates the pattern)
        display = Box::new(CliDisplay::default());
        display.info("second");
    }
}
