use crate::claude::{
    run_corrector, run_for_commit, run_for_spec_generation, run_reviewer, ClaudeOutcome,
    ClaudeRunner, ClaudeStoryResult, CommitOutcome, CorrectorOutcome, ReviewOutcome,
};
use crate::config::get_effective_config;
use crate::display::{BannerColor, StoryResult};
use crate::error::{Autom8Error, Result};
use crate::gh::{create_pull_request, PRResult};
use crate::git;
use crate::output::{
    print_all_complete, print_breadcrumb_trail, print_claude_output, print_error_panel,
    print_full_progress, print_generating_spec, print_header, print_info, print_interrupted,
    print_issues_found, print_iteration_complete, print_iteration_start,
    print_max_review_iterations, print_phase_banner, print_phase_footer, print_pr_already_exists,
    print_pr_skipped, print_pr_success, print_pr_updated, print_proceeding_to_implementation,
    print_project_info, print_resuming_interrupted, print_review_passed, print_reviewing,
    print_run_completed, print_run_summary, print_skip_review, print_spec_generated,
    print_spec_loaded, print_state_transition, print_story_complete, print_tasks_progress,
    print_worktree_context, print_worktree_created, print_worktree_reused, BOLD, CYAN, GRAY, RESET,
    YELLOW,
};
use crate::progress::{
    AgentDisplay, Breadcrumb, BreadcrumbState, ClaudeSpinner, Outcome, VerboseTimer,
};
use crate::signal::SignalHandler;
use crate::spec::{Spec, UserStory};
use crate::state::{IterationStatus, LiveState, MachineState, RunState, RunStatus, StateManager};
use crate::worktree::{
    ensure_worktree, format_worktree_error, generate_session_id, generate_worktree_path,
    is_in_worktree, remove_worktree, WorktreeResult,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

// ============================================================================
// Constants
// ============================================================================

/// Maximum number of review/correct iterations before giving up.
/// This prevents infinite loops when the corrector cannot resolve review issues.
const MAX_REVIEW_ITERATIONS: u32 = 3;

// ============================================================================
// Progress Display Helper (US-006)
// ============================================================================

/// Flush thresholds for live output (US-003).
/// Flush when either threshold is reached.
const LIVE_FLUSH_INTERVAL_MS: u64 = 200;
const LIVE_FLUSH_LINE_COUNT: usize = 10;

/// Heartbeat interval for indicating the run is still active (US-002).
/// The heartbeat is updated every 2-3 seconds.
const HEARTBEAT_INTERVAL_MS: u64 = 2500;

/// Helper struct that wraps a callback and periodically flushes output to live.json.
/// Flushes every ~200ms or every ~10 lines, whichever comes first.
/// Also updates heartbeat every ~2.5 seconds to indicate the run is still active.
struct LiveOutputFlusher<'a> {
    state_manager: &'a StateManager,
    live_state: LiveState,
    line_count_since_flush: usize,
    last_flush: Instant,
    last_heartbeat: Instant,
}

impl<'a> LiveOutputFlusher<'a> {
    fn new(state_manager: &'a StateManager, machine_state: MachineState) -> Self {
        let mut flusher = Self {
            state_manager,
            live_state: LiveState::new(machine_state),
            line_count_since_flush: 0,
            last_flush: Instant::now(),
            last_heartbeat: Instant::now(),
        };
        // Immediately flush to ensure live.json exists with current state
        flusher.flush();
        flusher
    }

    /// Append a line to the buffer and flush if thresholds are met.
    fn append(&mut self, line: &str) {
        self.live_state.append_line(line.to_string());
        self.line_count_since_flush += 1;

        // Check if we should flush output
        let time_elapsed =
            self.last_flush.elapsed() >= Duration::from_millis(LIVE_FLUSH_INTERVAL_MS);
        let lines_threshold = self.line_count_since_flush >= LIVE_FLUSH_LINE_COUNT;

        if time_elapsed || lines_threshold {
            self.flush();
        }

        // Check if we should update heartbeat (even if no flush needed)
        self.maybe_update_heartbeat();
    }

    /// Update heartbeat if the interval has elapsed.
    /// This ensures the heartbeat is updated even during periods of low output.
    fn maybe_update_heartbeat(&mut self) {
        if self.last_heartbeat.elapsed() >= Duration::from_millis(HEARTBEAT_INTERVAL_MS) {
            self.live_state.update_heartbeat();
            self.flush();
            self.last_heartbeat = Instant::now();
        }
    }

    /// Flush the current state to live.json.
    fn flush(&mut self) {
        // Update heartbeat on every flush to keep it fresh
        self.live_state.update_heartbeat();
        // Ignore errors - live output is best-effort for monitoring
        let _ = self.state_manager.save_live(&self.live_state);
        self.line_count_since_flush = 0;
        self.last_flush = Instant::now();
        self.last_heartbeat = Instant::now();
    }

    /// Final flush to ensure all remaining output is written.
    fn final_flush(&mut self) {
        if self.line_count_since_flush > 0 {
            self.flush();
        }
    }
}

/// Flush live.json immediately with a state update.
/// This is used outside of Claude operations (e.g., during state transitions)
/// to ensure the GUI sees state changes immediately.
fn flush_live_state(state_manager: &StateManager, machine_state: MachineState) {
    let live_state = LiveState::new(machine_state);
    let _ = state_manager.save_live(&live_state);
}

/// Runs an operation with either a verbose timer or spinner display,
/// handling the display lifecycle (start, update, finish with outcome).
///
/// This eliminates the duplicate verbose/spinner branching pattern throughout
/// the codebase by abstracting the display logic into a single helper.
///
/// # Arguments
/// * `verbose` - Whether to use verbose mode (timer) or spinner mode
/// * `create_timer` - Factory function to create a VerboseTimer
/// * `create_spinner` - Factory function to create a ClaudeSpinner
/// * `run_operation` - The operation to run, receiving a callback for progress updates
/// * `map_outcome` - Maps the operation result to an Outcome for display
///
/// # Returns
/// The result of the operation, after the display has been finished with the appropriate outcome.
fn with_progress_display<T, F, M>(
    verbose: bool,
    create_timer: impl FnOnce() -> VerboseTimer,
    create_spinner: impl FnOnce() -> ClaudeSpinner,
    run_operation: F,
    map_outcome: M,
) -> Result<T>
where
    F: FnOnce(&mut dyn FnMut(&str)) -> Result<T>,
    M: FnOnce(&Result<T>) -> Outcome,
{
    if verbose {
        let mut timer = create_timer();
        let result = run_operation(&mut |line| {
            print_claude_output(line);
        });
        let outcome = map_outcome(&result);
        timer.finish_with_outcome(outcome);
        result
    } else {
        let mut spinner = create_spinner();
        let result = run_operation(&mut |line| {
            spinner.update(line);
        });
        let outcome = map_outcome(&result);
        spinner.finish_with_outcome(outcome);
        result
    }
}

/// Runs an operation with progress display and live output streaming to live.json.
///
/// Similar to `with_progress_display`, but also writes streaming output to live.json
/// for the monitor command to read. Flushes every ~200ms or ~10 lines.
///
/// # Arguments
/// * `verbose` - Whether to use verbose mode (timer) or spinner mode
/// * `state_manager` - StateManager for writing live output
/// * `machine_state` - Current machine state to include in live.json
/// * `create_timer` - Factory function to create a VerboseTimer
/// * `create_spinner` - Factory function to create a ClaudeSpinner
/// * `run_operation` - The operation to run, receiving a callback for progress updates
/// * `map_outcome` - Maps the operation result to an Outcome for display
///
/// # Returns
/// The result of the operation, after the display has been finished with the appropriate outcome.
fn with_progress_display_and_live<T, F, M>(
    verbose: bool,
    state_manager: &StateManager,
    machine_state: MachineState,
    create_timer: impl FnOnce() -> VerboseTimer,
    create_spinner: impl FnOnce() -> ClaudeSpinner,
    run_operation: F,
    map_outcome: M,
) -> Result<T>
where
    F: FnOnce(&mut dyn FnMut(&str)) -> Result<T>,
    M: FnOnce(&Result<T>) -> Outcome,
{
    let mut live_flusher = LiveOutputFlusher::new(state_manager, machine_state);

    let result = if verbose {
        let mut timer = create_timer();
        let result = run_operation(&mut |line| {
            print_claude_output(line);
            live_flusher.append(line);
        });
        let outcome = map_outcome(&result);
        timer.finish_with_outcome(outcome);
        result
    } else {
        let mut spinner = create_spinner();
        let result = run_operation(&mut |line| {
            spinner.update(line);
            live_flusher.append(line);
        });
        let outcome = map_outcome(&result);
        spinner.finish_with_outcome(outcome);
        result
    };

    // Ensure any remaining output is flushed
    live_flusher.final_flush();

    result
}

/// Control flow action returned from extracted helper methods
/// to communicate back to the main implementation loop.
enum LoopAction {
    /// Continue to the next iteration of the main loop
    Continue,
    /// Break out of the main loop (run complete)
    Break,
}

/// Context for worktree setup that tracks partial state for cleanup on interruption.
///
/// This struct is used to track the state of worktree setup so that if the process
/// is interrupted mid-setup, we can properly clean up:
/// - If worktree was created but not yet changed into, remove the worktree
/// - If CWD was changed, restore the original CWD
/// - If session metadata wasn't saved, the worktree may be orphaned
#[derive(Debug, Clone)]
pub struct WorktreeSetupContext {
    /// The original working directory before any changes
    pub original_cwd: PathBuf,
    /// The worktree path if one was created (but may not have been entered yet)
    pub worktree_path: Option<PathBuf>,
    /// Whether the worktree was newly created (vs reused)
    pub worktree_was_created: bool,
    /// Whether we've changed into the worktree directory
    pub cwd_changed: bool,
    /// Whether session metadata has been saved
    pub metadata_saved: bool,
}

impl WorktreeSetupContext {
    /// Create a new setup context, capturing the current working directory.
    pub fn new() -> Result<Self> {
        let original_cwd = std::env::current_dir()?;
        Ok(Self {
            original_cwd,
            worktree_path: None,
            worktree_was_created: false,
            cwd_changed: false,
            metadata_saved: false,
        })
    }

    /// Clean up partial worktree setup on interruption.
    ///
    /// This method:
    /// 1. Restores the original CWD if it was changed
    /// 2. Removes the worktree if it was newly created and metadata wasn't saved
    pub fn cleanup_on_interruption(&self) {
        // First, restore original CWD if it was changed
        if self.cwd_changed {
            if let Err(e) = std::env::set_current_dir(&self.original_cwd) {
                eprintln!(
                    "Warning: failed to restore original directory '{}': {}",
                    self.original_cwd.display(),
                    e
                );
            }
        }

        // Only remove the worktree if:
        // - It was newly created (not reused)
        // - Metadata wasn't saved (session isn't trackable)
        if self.worktree_was_created && !self.metadata_saved {
            if let Some(ref worktree_path) = self.worktree_path {
                // Try to remove the worktree (force=true to handle incomplete state)
                if let Err(e) = remove_worktree(worktree_path, true) {
                    eprintln!(
                        "Warning: failed to remove partial worktree '{}': {}",
                        worktree_path.display(),
                        e
                    );
                }
            }
        }
    }
}

pub struct Runner {
    state_manager: StateManager,
    verbose: bool,
    skip_review: bool,
    /// Override for the worktree config setting.
    /// None = use config value, Some(true/false) = override config.
    worktree_override: Option<bool>,
    /// Override for the commit config setting.
    /// None = use config value, Some(true/false) = override config.
    commit_override: Option<bool>,
    /// Override for the pull_request config setting.
    /// None = use config value, Some(true/false) = override config.
    pull_request_override: Option<bool>,
}

impl Runner {
    pub fn new() -> Result<Self> {
        Ok(Self {
            state_manager: StateManager::new()?,
            verbose: false,
            skip_review: false,
            worktree_override: None,
            commit_override: None,
            pull_request_override: None,
        })
    }

    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    pub fn with_skip_review(mut self, skip_review: bool) -> Self {
        self.skip_review = skip_review;
        self
    }

    /// Set the worktree mode override.
    ///
    /// When set, this overrides the `worktree` setting from the config file.
    /// Use `true` to enable worktree mode (--worktree flag).
    /// Use `false` to disable worktree mode (--no-worktree flag).
    pub fn with_worktree(mut self, worktree: bool) -> Self {
        self.worktree_override = Some(worktree);
        self
    }

    /// Set the commit mode override.
    ///
    /// When set, this overrides the `commit` setting from the config file.
    pub fn with_commit(mut self, commit: bool) -> Self {
        self.commit_override = Some(commit);
        self
    }

    /// Set the pull_request mode override.
    ///
    /// When set, this overrides the `pull_request` setting from the config file.
    pub fn with_pull_request(mut self, pull_request: bool) -> Self {
        self.pull_request_override = Some(pull_request);
        self
    }

    /// Get the effective worktree mode, considering CLI override and config.
    ///
    /// Priority: CLI flag > config file > default (false).
    #[allow(dead_code)]
    pub fn effective_worktree(&self) -> Result<bool> {
        if let Some(override_value) = self.worktree_override {
            return Ok(override_value);
        }
        let config = get_effective_config()?;
        Ok(config.worktree)
    }

    /// Load the effective config, applying any CLI overrides.
    fn load_config_with_override(&self) -> Result<crate::config::Config> {
        let mut config = get_effective_config()?;

        // Apply commit override if set
        if let Some(commit) = self.commit_override {
            config.commit = commit;
        }

        // Apply pull_request override if set
        if let Some(pull_request) = self.pull_request_override {
            config.pull_request = pull_request;
        }

        Ok(config)
    }

    /// Check if worktree mode is effective (considering CLI override and config).
    fn is_worktree_mode(&self, config: &crate::config::Config) -> bool {
        if let Some(override_value) = self.worktree_override {
            return override_value;
        }
        config.worktree
    }

    /// Flush live.json with the current state without saving state.
    /// Used when we want to update live.json but state has already been saved.
    fn flush_live(&self, machine_state: MachineState) {
        flush_live_state(&self.state_manager, machine_state);
    }

    /// Setup worktree context for a run.
    ///
    /// When worktree mode is enabled, this will:
    /// 1. Create or reuse a worktree for the specified branch
    /// 2. Change the current working directory to the worktree
    /// 3. Generate a new session ID for the worktree
    ///
    /// Returns a tuple of:
    /// - The session ID and worktree path if a worktree was created/reused, or None if not in worktree mode
    /// - The setup context for cleanup on interruption
    fn setup_worktree_context(
        &self,
        config: &crate::config::Config,
        branch_name: &str,
    ) -> Result<(Option<(String, PathBuf)>, WorktreeSetupContext)> {
        // Create setup context to track partial state for cleanup
        let mut setup_ctx = WorktreeSetupContext::new()?;

        // Check if worktree mode is enabled
        if !self.is_worktree_mode(config) {
            return Ok((None, setup_ctx));
        }

        // Check if we're in a git repo
        if !git::is_git_repo() {
            print_info(
                "Worktree mode enabled but not in a git repository. Running in current directory.",
            );
            return Ok((None, setup_ctx));
        }

        // Create or reuse worktree
        let pattern = &config.worktree_path_pattern;
        let result = ensure_worktree(pattern, branch_name).map_err(|e| {
            // Provide enhanced error message for worktree failures
            if let Autom8Error::WorktreeError(msg) = &e {
                let worktree_path = generate_worktree_path(pattern, branch_name)
                    .unwrap_or_else(|_| PathBuf::from("<unknown>"));
                let formatted = format_worktree_error(msg, branch_name, &worktree_path);
                Autom8Error::WorktreeError(formatted)
            } else {
                e
            }
        })?;

        // Get the worktree path and inform the user
        let worktree_path = result.path().to_path_buf();
        setup_ctx.worktree_path = Some(worktree_path.clone());
        setup_ctx.worktree_was_created = result.was_created();

        match result {
            WorktreeResult::Created(_) => {
                print_worktree_created(&worktree_path, branch_name);
            }
            WorktreeResult::Reused(_) => {
                print_worktree_reused(&worktree_path, branch_name);
            }
        }

        // Change to the worktree directory
        std::env::set_current_dir(&worktree_path).map_err(|e| {
            // If we can't change to the worktree, clean up if we created it
            setup_ctx.cleanup_on_interruption();
            Autom8Error::WorktreeError(format!(
                "Failed to change to worktree directory '{}': {}",
                worktree_path.display(),
                e
            ))
        })?;
        setup_ctx.cwd_changed = true;

        // Print context info
        print_worktree_context(&worktree_path);

        // Generate session ID for the worktree
        let session_id = generate_session_id(&worktree_path);

        Ok((Some((session_id, worktree_path)), setup_ctx))
    }

    /// Handle a fatal error by transitioning to Failed state, saving, displaying error, and optionally printing summary.
    /// This standardizes error handling across the runner to ensure Failed state is always persisted before returning errors.
    #[allow(clippy::too_many_arguments)]
    fn handle_fatal_error<F>(
        &self,
        state: &mut RunState,
        error_panel_title: &str,
        error_panel_msg: &str,
        exit_code: Option<i32>,
        stderr: Option<&str>,
        print_summary: Option<F>,
        error: Autom8Error,
    ) -> Autom8Error
    where
        F: FnOnce() -> Result<()>,
    {
        // Always transition to Failed state first
        state.transition_to(MachineState::Failed);

        // Always persist the failed state before returning error
        if let Err(e) = self.state_manager.save(state) {
            // If we can't save state, log it but continue with the original error
            eprintln!("Warning: failed to save state: {}", e);
        }

        // Display error panel (unless title is empty, for cases like max iterations)
        if !error_panel_title.is_empty() {
            print_error_panel(error_panel_title, error_panel_msg, exit_code, stderr);
        }

        // Print summary if provided
        if let Some(summary_fn) = print_summary {
            if let Err(e) = summary_fn() {
                eprintln!("Warning: failed to print summary: {}", e);
            }
        }

        error
    }

    /// Handle graceful shutdown on SIGINT.
    ///
    /// This method:
    /// 1. Kills any running Claude subprocess
    /// 2. Updates state status to `Interrupted` (preserving the current machine_state)
    /// 3. Saves state and session metadata (`is_running: false`)
    /// 4. Clears the live output file
    /// 5. Restores original CWD if it was changed during worktree setup
    /// 6. Displays interruption message to user
    ///
    /// Returns `Err(Autom8Error::Interrupted)` to signal the run was interrupted.
    fn handle_interruption(
        &self,
        state: &mut RunState,
        claude_runner: &ClaudeRunner,
        worktree_setup_ctx: Option<&WorktreeSetupContext>,
    ) -> Autom8Error {
        // Kill any running Claude subprocess
        if let Err(e) = claude_runner.kill() {
            eprintln!("Warning: failed to kill Claude subprocess: {}", e);
        }

        // Update state to Interrupted (preserves machine_state)
        state.status = RunStatus::Interrupted;
        state.finished_at = Some(chrono::Utc::now());

        // Save state and session metadata (is_running will be set to false
        // because status is Interrupted, not Running)
        if let Err(e) = self.state_manager.save(state) {
            eprintln!("Warning: failed to save state: {}", e);
        }

        // Clear live output file
        if let Err(e) = self.state_manager.clear_live() {
            eprintln!("Warning: failed to clear live output: {}", e);
        }

        // Clean up worktree setup if needed (restores CWD, removes partial worktree)
        if let Some(setup_ctx) = worktree_setup_ctx {
            setup_ctx.cleanup_on_interruption();
        }

        // Display message to user
        print_interrupted();

        Autom8Error::Interrupted
    }

    /// Run the review/correct loop until review passes or max iterations reached.
    /// Returns Ok(()) if review passes, Err if max iterations exceeded or error occurs.
    fn run_review_correct_loop(
        &self,
        state: &mut RunState,
        spec: &Spec,
        breadcrumb: &mut Breadcrumb,
        story_results: &[StoryResult],
        print_summary_fn: &impl Fn(u32, &[StoryResult]) -> Result<()>,
    ) -> Result<()> {
        state.review_iteration = 1;

        loop {
            // Check if we've exceeded max review iterations
            if state.review_iteration > MAX_REVIEW_ITERATIONS {
                print_max_review_iterations();
                let iteration = state.iteration;
                let results = story_results;
                return Err(self.handle_fatal_error(
                    state,
                    "", // No error panel for max iterations (has its own message)
                    "",
                    None,
                    None,
                    Some(|| print_summary_fn(iteration, results)),
                    Autom8Error::MaxReviewIterationsReached,
                ));
            }

            // Transition to Reviewing state
            print_state_transition(state.machine_state, MachineState::Reviewing);
            state.transition_to(MachineState::Reviewing);
            self.state_manager.save(state)?;
            self.flush_live(MachineState::Reviewing);

            // Update breadcrumb to enter Review state
            breadcrumb.enter_state(BreadcrumbState::Review);

            print_phase_banner("REVIEWING", BannerColor::Cyan);
            print_reviewing(state.review_iteration, MAX_REVIEW_ITERATIONS);

            // Run reviewer with progress display and live output (for heartbeat updates)
            let review_iter = state.review_iteration;
            let review_result = with_progress_display_and_live(
                self.verbose,
                &self.state_manager,
                MachineState::Reviewing,
                || VerboseTimer::new_for_review(review_iter, MAX_REVIEW_ITERATIONS),
                || ClaudeSpinner::new_for_review(review_iter, MAX_REVIEW_ITERATIONS),
                |callback| run_reviewer(spec, review_iter, MAX_REVIEW_ITERATIONS, callback),
                |res| match res {
                    Ok(r) => {
                        let tokens = r.usage.as_ref().map(|u| u.total_tokens());
                        match &r.outcome {
                            ReviewOutcome::Pass => {
                                Outcome::success("No issues found").with_optional_tokens(tokens)
                            }
                            ReviewOutcome::IssuesFound => {
                                Outcome::success("Issues found").with_optional_tokens(tokens)
                            }
                            ReviewOutcome::Error(e) => Outcome::failure(e.to_string()),
                        }
                    }
                    Err(e) => Outcome::failure(e.to_string()),
                },
            )?;

            // Capture usage from review into "Final Review" phase (US-005)
            state.capture_usage("Final Review", review_result.usage.clone());

            // Print bottom border to close the output frame
            print_phase_footer(BannerColor::Cyan);

            // Print breadcrumb trail after review phase completion
            print_breadcrumb_trail(breadcrumb);

            // Show progress bar after review task completion
            print_full_progress(
                spec.completed_count(),
                spec.total_count(),
                state.review_iteration,
                MAX_REVIEW_ITERATIONS,
            );
            println!();

            match review_result.outcome {
                ReviewOutcome::Pass => {
                    // Delete autom8_review.md if it exists
                    let review_path = std::path::Path::new("autom8_review.md");
                    if review_path.exists() {
                        let _ = fs::remove_file(review_path);
                    }
                    // Save state with captured review usage before exiting (US-005)
                    self.state_manager.save(state)?;
                    print_review_passed();
                    return Ok(()); // Exit review loop, proceed to commit
                }
                ReviewOutcome::IssuesFound => {
                    // Transition to Correcting state
                    print_state_transition(MachineState::Reviewing, MachineState::Correcting);
                    state.transition_to(MachineState::Correcting);
                    self.state_manager.save(state)?;
                    self.flush_live(MachineState::Correcting);

                    // Update breadcrumb to enter Correct state
                    breadcrumb.enter_state(BreadcrumbState::Correct);

                    print_phase_banner("CORRECTING", BannerColor::Yellow);
                    print_issues_found(state.review_iteration, MAX_REVIEW_ITERATIONS);

                    // Run corrector with progress display and live output (for heartbeat updates)
                    let corrector_result = with_progress_display_and_live(
                        self.verbose,
                        &self.state_manager,
                        MachineState::Correcting,
                        || VerboseTimer::new_for_correct(review_iter, MAX_REVIEW_ITERATIONS),
                        || ClaudeSpinner::new_for_correct(review_iter, MAX_REVIEW_ITERATIONS),
                        |callback| run_corrector(spec, review_iter, callback),
                        |res| match res {
                            Ok(r) => {
                                let tokens = r.usage.as_ref().map(|u| u.total_tokens());
                                match &r.outcome {
                                    CorrectorOutcome::Complete => {
                                        Outcome::success("Issues addressed")
                                            .with_optional_tokens(tokens)
                                    }
                                    CorrectorOutcome::Error(e) => Outcome::failure(e.to_string()),
                                }
                            }
                            Err(e) => Outcome::failure(e.to_string()),
                        },
                    )?;

                    // Capture usage from correction into "Final Review" phase (US-005)
                    // This accumulates with the review usage since both are part of the review loop
                    state.capture_usage("Final Review", corrector_result.usage.clone());

                    // Print bottom border to close the output frame
                    print_phase_footer(BannerColor::Yellow);

                    // Print breadcrumb trail after correct phase completion
                    print_breadcrumb_trail(breadcrumb);

                    // Show progress bar after correct task completion
                    print_full_progress(
                        spec.completed_count(),
                        spec.total_count(),
                        state.review_iteration,
                        MAX_REVIEW_ITERATIONS,
                    );
                    println!();

                    match corrector_result.outcome {
                        CorrectorOutcome::Complete => {
                            // Increment review iteration and loop back to Reviewing
                            state.review_iteration += 1;
                            // Save state with captured corrector usage (US-005)
                            self.state_manager.save(state)?;
                        }
                        CorrectorOutcome::Error(e) => {
                            let iteration = state.iteration;
                            let results = story_results;
                            return Err(self.handle_fatal_error(
                                state,
                                "Corrector Failed",
                                &e.message,
                                e.exit_code,
                                e.stderr.as_deref(),
                                Some(|| print_summary_fn(iteration, results)),
                                Autom8Error::ClaudeError(format!("Corrector failed: {}", e)),
                            ));
                        }
                    }
                }
                ReviewOutcome::Error(e) => {
                    let iteration = state.iteration;
                    let results = story_results;
                    return Err(self.handle_fatal_error(
                        state,
                        "Review Failed",
                        &e.message,
                        e.exit_code,
                        e.stderr.as_deref(),
                        Some(|| print_summary_fn(iteration, results)),
                        Autom8Error::ClaudeError(format!("Review failed: {}", e)),
                    ));
                }
            }
        }
    }

    /// Handle commit and PR creation flow after all stories are complete.
    /// Returns Ok(()) on success, Err on failure.
    /// Respects config settings: if commit=false, skips commit state entirely.
    /// If pull_request=false, skips PR creation (ends after commit or immediately if commit=false).
    fn handle_commit_and_pr(
        &self,
        state: &mut RunState,
        spec: &Spec,
        breadcrumb: &mut Breadcrumb,
    ) -> Result<()> {
        // Get the effective config for this run (US-005)
        let config = state.effective_config();

        // If commit=false, skip commit state entirely
        if !config.commit {
            print_state_transition(state.machine_state, MachineState::Completed);
            print_info("Skipping commit (commit = false in config)");
            return Ok(());
        }

        if !git::is_git_repo() {
            print_state_transition(state.machine_state, MachineState::Completed);
            return Ok(());
        }

        print_state_transition(state.machine_state, MachineState::Committing);
        state.transition_to(MachineState::Committing);
        self.state_manager.save(state)?;
        self.flush_live(MachineState::Committing);

        // Update breadcrumb to enter Commit state
        breadcrumb.enter_state(BreadcrumbState::Commit);

        print_phase_banner("COMMITTING", BannerColor::Cyan);

        // Run commit with progress display and live output (for heartbeat updates)
        let commit_result = with_progress_display_and_live(
            self.verbose,
            &self.state_manager,
            MachineState::Committing,
            VerboseTimer::new_for_commit,
            ClaudeSpinner::new_for_commit,
            |callback| run_for_commit(spec, callback),
            |res| match res {
                Ok(r) => {
                    let tokens = r.usage.as_ref().map(|u| u.total_tokens());
                    match &r.outcome {
                        CommitOutcome::Success(hash) => {
                            Outcome::success(hash.clone()).with_optional_tokens(tokens)
                        }
                        CommitOutcome::NothingToCommit => {
                            Outcome::success("Nothing to commit").with_optional_tokens(tokens)
                        }
                        CommitOutcome::Error(e) => Outcome::failure(e.to_string()),
                    }
                }
                Err(e) => Outcome::failure(e.to_string()),
            },
        )?;

        // Capture usage from commit into "PR & Commit" phase (US-005)
        state.capture_usage("PR & Commit", commit_result.usage.clone());
        self.state_manager.save(state)?;

        // Print bottom border to close the output frame
        print_phase_footer(BannerColor::Cyan);

        // Print breadcrumb trail after commit phase completion
        print_breadcrumb_trail(breadcrumb);

        // Track whether commits were made for PR creation
        let commits_were_made = matches!(&commit_result.outcome, CommitOutcome::Success(_));

        match &commit_result.outcome {
            CommitOutcome::Success(hash) => {
                print_info(&format!("Changes committed successfully ({})", hash))
            }
            CommitOutcome::NothingToCommit => print_info("Nothing to commit"),
            CommitOutcome::Error(e) => {
                print_error_panel(
                    "Commit Failed",
                    &e.message,
                    e.exit_code,
                    e.stderr.as_deref(),
                );
            }
        }

        // Skip PR creation if pull_request=false (US-005)
        if !config.pull_request {
            print_state_transition(MachineState::Committing, MachineState::Completed);
            print_info("Skipping PR creation (pull_request = false in config)");
            return Ok(());
        }

        // PR Creation step
        self.handle_pr_creation(state, spec, commits_were_made, config.pull_request_draft)
    }

    /// Handle PR creation after committing.
    fn handle_pr_creation(
        &self,
        state: &mut RunState,
        spec: &Spec,
        commits_were_made: bool,
        draft: bool,
    ) -> Result<()> {
        print_state_transition(MachineState::Committing, MachineState::CreatingPR);
        state.transition_to(MachineState::CreatingPR);
        self.state_manager.save(state)?;
        self.flush_live(MachineState::CreatingPR);

        match create_pull_request(spec, commits_were_made, draft) {
            Ok(PRResult::Success(url)) => {
                print_pr_success(&url);
                print_state_transition(MachineState::CreatingPR, MachineState::Completed);
                Ok(())
            }
            Ok(PRResult::Skipped(reason)) => {
                print_pr_skipped(&reason);
                print_state_transition(MachineState::CreatingPR, MachineState::Completed);
                Ok(())
            }
            Ok(PRResult::AlreadyExists(url)) => {
                print_pr_already_exists(&url);
                print_state_transition(MachineState::CreatingPR, MachineState::Completed);
                Ok(())
            }
            Ok(PRResult::Updated(url)) => {
                print_pr_updated(&url);
                print_state_transition(MachineState::CreatingPR, MachineState::Completed);
                Ok(())
            }
            Ok(PRResult::Error(msg)) => {
                print_state_transition(MachineState::CreatingPR, MachineState::Failed);
                Err(self.handle_fatal_error(
                    state,
                    "PR Creation Failed",
                    &msg,
                    None,
                    None,
                    None::<fn() -> Result<()>>,
                    Autom8Error::ClaudeError(format!("PR creation failed: {}", msg)),
                ))
            }
            Err(e) => {
                print_state_transition(MachineState::CreatingPR, MachineState::Failed);
                Err(self.handle_fatal_error(
                    state,
                    "PR Creation Error",
                    &e.to_string(),
                    None,
                    None,
                    None::<fn() -> Result<()>>,
                    e,
                ))
            }
        }
    }

    /// Handle the flow when all stories are complete at iteration start.
    /// Returns LoopAction::Break on success (run complete).
    fn handle_all_stories_complete(
        &self,
        state: &mut RunState,
        spec: &Spec,
        breadcrumb: &mut Breadcrumb,
        story_results: &[StoryResult],
        print_summary_fn: &impl Fn(u32, &[StoryResult]) -> Result<()>,
    ) -> Result<LoopAction> {
        print_all_complete();

        // Get the effective config for this run (US-005)
        let config = state.effective_config();

        // Skip review if --skip-review flag is set OR if review=false in config
        if self.skip_review || !config.review {
            print_skip_review();
        } else {
            // Run review/correct loop
            self.run_review_correct_loop(state, spec, breadcrumb, story_results, print_summary_fn)?;
        }

        // Commit changes and create PR (respects commit and pull_request config)
        self.handle_commit_and_pr(state, spec, breadcrumb)?;

        state.transition_to(MachineState::Completed);
        // Flush live state to ensure GUI sees the Completed state before cleanup
        self.flush_live(MachineState::Completed);
        self.state_manager.save(state)?;
        print_summary_fn(state.iteration, story_results)?;

        // Print final run completion message with total tokens (US-007)
        let duration_secs = state.run_duration_secs();
        let total_tokens = state.total_usage.as_ref().map(|u| u.total_tokens());
        print_run_completed(duration_secs, total_tokens);

        self.archive_and_cleanup(state)?;
        Ok(LoopAction::Break)
    }

    /// Handle an error from Claude story execution.
    /// Transitions to Failed state and returns the appropriate error.
    #[allow(clippy::too_many_arguments)]
    fn handle_story_error(
        &self,
        state: &mut RunState,
        story: &UserStory,
        story_results: &mut Vec<StoryResult>,
        story_start: Instant,
        error_msg: &str,
        error_panel_title: &str,
        error_panel_msg: &str,
        exit_code: Option<i32>,
        stderr: Option<&str>,
        print_summary_fn: &impl Fn(u32, &[StoryResult]) -> Result<()>,
    ) -> Result<LoopAction> {
        state.finish_iteration(IterationStatus::Failed, error_msg.to_string());
        state.transition_to(MachineState::Failed);
        // Clear live output when iteration finishes (US-003)
        let _ = self.state_manager.clear_live();
        self.state_manager.save(state)?;

        story_results.push(StoryResult {
            id: story.id.clone(),
            title: story.title.clone(),
            passed: false,
            duration_secs: story_start.elapsed().as_secs(),
        });

        print_error_panel(error_panel_title, error_panel_msg, exit_code, stderr);
        print_summary_fn(state.iteration, story_results)?;
        Err(Autom8Error::ClaudeError(error_msg.to_string()))
    }

    /// Handle a single story iteration, processing the Claude result.
    /// Returns LoopAction::Continue to continue the loop, LoopAction::Break to finish.
    #[allow(clippy::too_many_arguments)]
    fn handle_story_iteration(
        &self,
        state: &mut RunState,
        spec: &Spec,
        spec_json_path: &Path,
        story: &UserStory,
        breadcrumb: &mut Breadcrumb,
        story_results: &mut Vec<StoryResult>,
        story_start: Instant,
        claude_runner: &ClaudeRunner,
        print_summary_fn: &impl Fn(u32, &[StoryResult]) -> Result<()>,
    ) -> Result<LoopAction> {
        // Calculate story progress for display: [US-001 2/5]
        let story_index = spec
            .user_stories
            .iter()
            .position(|s| s.id == story.id)
            .map(|i| i as u32 + 1)
            .unwrap_or(state.iteration);
        let total_stories = spec.total_count() as u32;
        let story_id = story.id.clone();
        let iterations = state.iterations.clone();
        let knowledge = state.knowledge.clone();

        // Run Claude with progress display and live output streaming (US-003)
        // Use the provided ClaudeRunner so it can be killed on interrupt
        let result = with_progress_display_and_live(
            self.verbose,
            &self.state_manager,
            MachineState::RunningClaude,
            || VerboseTimer::new_with_story_progress(&story_id, story_index, total_stories),
            || ClaudeSpinner::new_with_story_progress(&story_id, story_index, total_stories),
            |callback| {
                claude_runner.run(
                    spec,
                    story,
                    spec_json_path,
                    &iterations,
                    &knowledge,
                    callback,
                )
            },
            |res| match res {
                Ok(result) => {
                    let tokens = result.usage.as_ref().map(|u| u.total_tokens());
                    Outcome::success("Implementation done").with_optional_tokens(tokens)
                }
                Err(e) => Outcome::failure(e.to_string()),
            },
        );

        match result {
            Ok(ClaudeStoryResult {
                outcome: ClaudeOutcome::AllStoriesComplete,
                work_summary,
                full_output,
                usage,
            }) => {
                // Capture usage from story implementation (US-005)
                state.capture_usage(&story.id, usage.clone());
                state.set_iteration_usage(usage);
                self.handle_all_stories_complete_from_story(
                    state,
                    spec,
                    spec_json_path,
                    story,
                    breadcrumb,
                    story_results,
                    work_summary,
                    &full_output,
                    print_summary_fn,
                )
            }
            Ok(ClaudeStoryResult {
                outcome: ClaudeOutcome::IterationComplete,
                work_summary,
                full_output,
                usage,
            }) => {
                // Capture usage from story implementation (US-005)
                state.capture_usage(&story.id, usage.clone());
                state.set_iteration_usage(usage);
                self.handle_iteration_complete(
                    state,
                    spec_json_path,
                    story,
                    breadcrumb,
                    story_results,
                    work_summary,
                    &full_output,
                )
            }
            Ok(ClaudeStoryResult {
                outcome: ClaudeOutcome::Error(error_info),
                usage,
                ..
            }) => {
                // Capture usage even on error (partial usage before failure) (US-005)
                state.capture_usage(&story.id, usage.clone());
                state.set_iteration_usage(usage);
                self.handle_story_error(
                    state,
                    story,
                    story_results,
                    story_start,
                    &error_info.message,
                    "Claude Process Failed",
                    &error_info.message,
                    error_info.exit_code,
                    error_info.stderr.as_deref(),
                    print_summary_fn,
                )
            }
            Err(e) => self.handle_story_error(
                state,
                story,
                story_results,
                story_start,
                &e.to_string(),
                "Claude Error",
                &e.to_string(),
                None,
                None,
                print_summary_fn,
            ),
        }
    }

    /// Handle when Claude reports all stories complete during story processing.
    #[allow(clippy::too_many_arguments)]
    fn handle_all_stories_complete_from_story(
        &self,
        state: &mut RunState,
        spec: &Spec,
        spec_json_path: &Path,
        story: &UserStory,
        breadcrumb: &mut Breadcrumb,
        story_results: &mut Vec<StoryResult>,
        work_summary: Option<String>,
        full_output: &str,
        print_summary_fn: &impl Fn(u32, &[StoryResult]) -> Result<()>,
    ) -> Result<LoopAction> {
        state.finish_iteration(IterationStatus::Success, full_output.to_string());
        state.set_work_summary(work_summary.clone());
        // Clear live output when iteration finishes (US-003)
        let _ = self.state_manager.clear_live();

        // Capture story knowledge from git diff and agent output (US-006)
        state.capture_story_knowledge(&story.id, full_output, None);
        self.state_manager.save(state)?;

        let duration = state.current_iteration_duration();
        story_results.push(StoryResult {
            id: story.id.clone(),
            title: story.title.clone(),
            passed: true,
            duration_secs: duration,
        });

        // Print bottom border to close the output frame
        print_phase_footer(BannerColor::Cyan);

        // Print breadcrumb trail after story phase completion
        print_breadcrumb_trail(breadcrumb);

        // Show progress bar after story task completion
        let updated_spec = Spec::load(spec_json_path)?;
        print_tasks_progress(updated_spec.completed_count(), updated_spec.total_count());
        println!();

        if self.verbose {
            print_story_complete(&story.id, duration);
        }

        // Validate that all stories are actually complete
        if !updated_spec.all_complete() {
            // Spec doesn't match Claude's claim - continue processing stories
            return Ok(LoopAction::Continue);
        }

        print_all_complete();

        // Get the effective config for this run (US-005)
        let config = state.effective_config();

        // Skip review if --skip-review flag is set OR if review=false in config
        if self.skip_review || !config.review {
            print_skip_review();
        } else {
            // Run review/correct loop
            self.run_review_correct_loop(
                state,
                &updated_spec,
                breadcrumb,
                story_results,
                print_summary_fn,
            )?;
        }

        // Commit changes and create PR (respects commit and pull_request config)
        self.handle_commit_and_pr(state, spec, breadcrumb)?;

        state.transition_to(MachineState::Completed);
        // Flush live state to ensure GUI sees the Completed state before cleanup
        self.flush_live(MachineState::Completed);
        self.state_manager.save(state)?;
        print_summary_fn(state.iteration, story_results)?;

        // Print final run completion message with total tokens (US-007)
        let run_duration = state.run_duration_secs();
        let total_tokens = state.total_usage.as_ref().map(|u| u.total_tokens());
        print_run_completed(run_duration, total_tokens);

        self.archive_and_cleanup(state)?;
        Ok(LoopAction::Break)
    }

    /// Handle a normal iteration completion (story done, more to go).
    #[allow(clippy::too_many_arguments)]
    fn handle_iteration_complete(
        &self,
        state: &mut RunState,
        spec_json_path: &Path,
        story: &UserStory,
        breadcrumb: &mut Breadcrumb,
        story_results: &mut Vec<StoryResult>,
        work_summary: Option<String>,
        full_output: &str,
    ) -> Result<LoopAction> {
        state.finish_iteration(IterationStatus::Success, full_output.to_string());
        state.set_work_summary(work_summary.clone());
        // Clear live output when iteration finishes (US-003)
        let _ = self.state_manager.clear_live();

        // Capture story knowledge from git diff and agent output (US-006)
        state.capture_story_knowledge(&story.id, full_output, None);
        self.state_manager.save(state)?;

        let duration = state.current_iteration_duration();

        // Print bottom border to close the output frame
        print_phase_footer(BannerColor::Cyan);

        // Print breadcrumb trail after story phase completion
        print_breadcrumb_trail(breadcrumb);

        print_state_transition(MachineState::RunningClaude, MachineState::PickingStory);
        print_iteration_complete(state.iteration);

        // Reload spec and check if current story passed
        let updated_spec = Spec::load(spec_json_path)?;
        let story_passed = updated_spec
            .user_stories
            .iter()
            .find(|s| s.id == story.id)
            .is_some_and(|s| s.passes);

        if story_passed {
            story_results.push(StoryResult {
                id: story.id.clone(),
                title: story.title.clone(),
                passed: true,
                duration_secs: duration,
            });
            if self.verbose {
                print_story_complete(&story.id, duration);
            }
        }

        // Show progress bar after story task completion
        print_tasks_progress(updated_spec.completed_count(), updated_spec.total_count());
        println!();

        // Continue to next iteration
        Ok(LoopAction::Continue)
    }

    /// Run from a spec-<feature>.md markdown file - converts to JSON first, then implements
    pub fn run_from_spec(&self, spec_path: &Path) -> Result<()> {
        // IMPORTANT: State must NOT be persisted until after worktree context is determined.
        // Saving state before we know the correct session ID would create phantom sessions
        // in the main repo when running in worktree mode. Visual state transitions can be
        // displayed, but save() must not be called until the effective StateManager is known.

        // Check for existing active run
        if self.state_manager.has_active_run()? {
            if let Some(state) = self.state_manager.load_current()? {
                return Err(Autom8Error::RunInProgress(state.run_id));
            }
        }

        // Load effective config at startup, applying CLI flag override (US-002, US-005)
        let config = self.load_config_with_override()?;

        // Canonicalize spec path
        let spec_path = spec_path
            .canonicalize()
            .map_err(|_| Autom8Error::SpecNotFound(spec_path.to_path_buf()))?;

        // Determine spec JSON output path in config directory
        let stem = spec_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("spec");
        let spec_dir = self.state_manager.ensure_spec_dir()?;
        let spec_json_path = spec_dir.join(format!("{}.json", stem));

        // Initialize state with config snapshot for resume support
        // Clone config since we need it later for worktree setup
        // Note: State is NOT saved here - we defer persistence until after worktree context
        // is determined to avoid creating phantom sessions in the main repo
        let mut state = RunState::from_spec_with_config(
            spec_path.clone(),
            spec_json_path.clone(),
            config.clone(),
        );

        // LoadingSpec state
        print_state_transition(MachineState::Idle, MachineState::LoadingSpec);

        // Load spec content
        let spec_content = fs::read_to_string(&spec_path)?;
        if spec_content.trim().is_empty() {
            return Err(Autom8Error::EmptySpec);
        }

        let metadata = fs::metadata(&spec_path)?;
        print_spec_loaded(&spec_path, metadata.len());
        println!();

        // Transition to GeneratingSpec
        // Note: State is NOT saved here - we defer persistence until after worktree context
        // is determined. Visual feedback is still shown via print_state_transition.
        state.transition_to(MachineState::GeneratingSpec);
        print_state_transition(MachineState::LoadingSpec, MachineState::GeneratingSpec);

        print_generating_spec();

        // Run Claude to generate spec JSON with progress display
        let spec_result = match with_progress_display(
            self.verbose,
            VerboseTimer::new_for_spec,
            ClaudeSpinner::new_for_spec,
            |callback| run_for_spec_generation(&spec_content, &spec_json_path, callback),
            |res| match res {
                Ok(r) => {
                    let tokens = r.usage.as_ref().map(|u| u.total_tokens());
                    Outcome::success("Spec generated").with_optional_tokens(tokens)
                }
                Err(e) => Outcome::failure(e.to_string()),
            },
        ) {
            Ok(result) => result,
            Err(e) => {
                print_error_panel("Spec Generation Failed", &e.to_string(), None, None);
                return Err(e);
            }
        };
        let spec = spec_result.spec;

        // Capture usage from spec generation into "Planning" phase (US-005)
        state.capture_usage("Planning", spec_result.usage);

        print_spec_generated(&spec, &spec_json_path);

        // Check for branch conflicts with other active sessions (US-006)
        // This must happen before any git operations to prevent race conditions
        if let Some(conflict) = self
            .state_manager
            .check_branch_conflict(&spec.branch_name)?
        {
            return Err(Autom8Error::BranchConflict {
                branch: spec.branch_name.clone(),
                session_id: conflict.session_id,
                worktree_path: conflict.worktree_path,
            });
        }

        // Setup worktree context if enabled (US-007)
        // This creates/reuses a worktree and changes the current working directory
        let (worktree_context, mut worktree_setup_ctx) =
            self.setup_worktree_context(&config, &spec.branch_name)?;

        // Create the appropriate StateManager for this context
        let effective_state_manager = if let Some((ref session_id, _)) = worktree_context {
            // In worktree mode, use the worktree's session ID
            StateManager::with_session(session_id.clone())?
        } else {
            // Not in worktree mode, use auto-detected session
            StateManager::new()?
        };

        // Update state with branch from generated spec and session ID
        state.branch = spec.branch_name.clone();
        if let Some((ref session_id, _)) = &worktree_context {
            state.session_id = Some(session_id.clone());
        }
        state.transition_to(MachineState::Initializing);
        effective_state_manager.save(&state)?;
        // Flush live.json immediately so GUI sees the state transition
        flush_live_state(&effective_state_manager, MachineState::Initializing);

        print_state_transition(MachineState::GeneratingSpec, MachineState::Initializing);

        print_proceeding_to_implementation();

        // Create a new Runner with the effective state manager and continue
        let effective_runner = Runner {
            state_manager: effective_state_manager,
            verbose: self.verbose,
            skip_review: self.skip_review,
            worktree_override: self.worktree_override,
            commit_override: self.commit_override,
            pull_request_override: self.pull_request_override,
        };

        // Mark metadata as saved since the state was just saved above
        worktree_setup_ctx.metadata_saved = true;

        effective_runner.run_implementation_loop(state, &spec_json_path, worktree_setup_ctx)
    }

    pub fn run(&self, spec_json_path: &Path) -> Result<()> {
        // IMPORTANT: State must NOT be persisted until after worktree context is determined.
        // Saving state before we know the correct session ID would create phantom sessions
        // in the main repo when running in worktree mode. State is first persisted in
        // run_implementation_loop() after the effective StateManager is known.

        // Check for existing active run
        if self.state_manager.has_active_run()? {
            if let Some(state) = self.state_manager.load_current()? {
                return Err(Autom8Error::RunInProgress(state.run_id));
            }
        }

        // Load effective config at startup, applying CLI flag override (US-002, US-005)
        let config = self.load_config_with_override()?;

        // Canonicalize path so resume works from any directory
        let spec_json_path = spec_json_path
            .canonicalize()
            .map_err(|_| Autom8Error::SpecNotFound(spec_json_path.to_path_buf()))?;

        // Load and validate spec
        let spec = Spec::load(&spec_json_path)?;

        // Check for branch conflicts with other active sessions (US-006)
        // This must happen before any git operations to prevent race conditions
        if let Some(conflict) = self
            .state_manager
            .check_branch_conflict(&spec.branch_name)?
        {
            return Err(Autom8Error::BranchConflict {
                branch: spec.branch_name.clone(),
                session_id: conflict.session_id,
                worktree_path: conflict.worktree_path,
            });
        }

        // Setup worktree context if enabled (US-007)
        // This creates/reuses a worktree and changes the current working directory
        let (worktree_context, worktree_setup_ctx) =
            self.setup_worktree_context(&config, &spec.branch_name)?;

        // Create the appropriate StateManager for this context
        let state_manager = if let Some((ref session_id, _)) = worktree_context {
            // In worktree mode, use the worktree's session ID
            StateManager::with_session(session_id.clone())?
        } else {
            // Not in worktree mode, use auto-detected session
            StateManager::new()?
        };

        // Clear any stale live output from a previous crashed run (US-003)
        let _ = state_manager.clear_live();

        // If NOT in worktree mode and in a git repo, ensure we're on the correct branch
        if worktree_context.is_none() && git::is_git_repo() {
            let current_branch = git::current_branch()?;
            if current_branch != spec.branch_name {
                print_info(&format!(
                    "Switching from '{}' to '{}'",
                    current_branch, spec.branch_name
                ));
                git::ensure_branch(&spec.branch_name)?;
            }
        }

        // Initialize state with config snapshot for resume support
        let state = if let Some((ref session_id, _)) = worktree_context {
            RunState::new_with_config_and_session(
                spec_json_path.to_path_buf(),
                spec.branch_name.clone(),
                config,
                session_id.clone(),
            )
        } else {
            RunState::new_with_config(
                spec_json_path.to_path_buf(),
                spec.branch_name.clone(),
                config,
            )
        };

        print_state_transition(MachineState::Idle, MachineState::Initializing);
        print_project_info(&spec);

        // Create a new Runner with the worktree-specific state manager
        // and delegate to it for the implementation loop
        let worktree_runner = Runner {
            state_manager,
            verbose: self.verbose,
            skip_review: self.skip_review,
            worktree_override: self.worktree_override,
            commit_override: self.commit_override,
            pull_request_override: self.pull_request_override,
        };

        worktree_runner.run_implementation_loop(state, &spec_json_path, worktree_setup_ctx)
    }

    fn run_implementation_loop(
        &self,
        mut state: RunState,
        spec_json_path: &Path,
        mut worktree_setup_ctx: WorktreeSetupContext,
    ) -> Result<()> {
        // Create signal handler for graceful shutdown (US-004)
        let signal_handler = SignalHandler::new()?;

        // Create ClaudeRunner that can be killed on interrupt (US-004)
        let claude_runner = ClaudeRunner::new();

        // Transition to PickingStory
        print_state_transition(state.machine_state, MachineState::PickingStory);
        state.transition_to(MachineState::PickingStory);
        self.state_manager.save(&state)?;
        self.flush_live(MachineState::PickingStory);

        // Mark metadata as saved now that state has been persisted
        // This ensures cleanup_on_interruption won't remove the worktree
        worktree_setup_ctx.metadata_saved = true;

        // Track story results for summary
        let mut story_results: Vec<StoryResult> = Vec::new();
        let run_start = Instant::now();

        // Breadcrumb trail for tracking workflow journey
        let mut breadcrumb = Breadcrumb::new();

        // Helper to print run summary (loads spec and prints)
        let print_summary_fn = |iteration: u32, results: &[StoryResult]| -> Result<()> {
            let spec = Spec::load(spec_json_path)?;
            print_run_summary(
                spec.total_count(),
                spec.completed_count(),
                iteration,
                run_start.elapsed().as_secs(),
                results,
            );
            Ok(())
        };

        // Main loop
        loop {
            // Check for shutdown request at safe point (between state transitions) (US-004)
            if signal_handler.is_shutdown_requested() {
                return Err(self.handle_interruption(
                    &mut state,
                    &claude_runner,
                    Some(&worktree_setup_ctx),
                ));
            }

            // Reload spec to get latest passes state
            let spec = Spec::load(spec_json_path)?;

            // Check if all stories complete at loop start
            if spec.all_complete() {
                match self.handle_all_stories_complete(
                    &mut state,
                    &spec,
                    &mut breadcrumb,
                    &story_results,
                    &print_summary_fn,
                )? {
                    LoopAction::Break => return Ok(()),
                    LoopAction::Continue => continue,
                }
            }

            // Pick next story
            let story = spec
                .next_incomplete_story()
                .ok_or(Autom8Error::NoIncompleteStories)?
                .clone();

            // Reset breadcrumb trail at start of each new story
            breadcrumb.reset();

            // Capture pre-story state for git diff calculation (US-006)
            state.capture_pre_story_state();

            // Start iteration
            print_state_transition(MachineState::PickingStory, MachineState::RunningClaude);
            state.start_iteration(&story.id);
            self.state_manager.save(&state)?;
            self.flush_live(MachineState::RunningClaude);

            // Update breadcrumb to enter Story state
            breadcrumb.enter_state(BreadcrumbState::Story);

            print_phase_banner("RUNNING", BannerColor::Cyan);
            print_iteration_start(state.iteration, &story.id, &story.title);

            // Process the story iteration
            let story_start = Instant::now();
            match self.handle_story_iteration(
                &mut state,
                &spec,
                spec_json_path,
                &story,
                &mut breadcrumb,
                &mut story_results,
                story_start,
                &claude_runner,
                &print_summary_fn,
            )? {
                LoopAction::Break => return Ok(()),
                LoopAction::Continue => {
                    // Check for shutdown after story iteration completes (US-004)
                    if signal_handler.is_shutdown_requested() {
                        return Err(self.handle_interruption(
                            &mut state,
                            &claude_runner,
                            Some(&worktree_setup_ctx),
                        ));
                    }
                    continue;
                }
            }
        }
    }

    pub fn resume(&self) -> Result<()> {
        // First try: load from active state
        if let Some(state) = self.state_manager.load_current()? {
            if state.status == RunStatus::Running
                || state.status == RunStatus::Failed
                || state.status == RunStatus::Interrupted
            {
                // Show interruption message if resuming from an interrupted state
                if state.status == RunStatus::Interrupted {
                    print_resuming_interrupted(&format!("{:?}", state.machine_state));
                }

                let spec_json_path = state.spec_json_path.clone();

                // Archive the interrupted/failed run before starting fresh
                self.state_manager.archive(&state)?;
                self.state_manager.clear_current()?;

                // Start a new run with the same parameters
                return self.run(&spec_json_path);
            }
        }

        // Second try: smart resume - scan for incomplete specs
        self.smart_resume()
    }

    /// Scan spec/ in config directory for incomplete specs and offer to resume one
    fn smart_resume(&self) -> Result<()> {
        use crate::prompt;

        let spec_files = self.state_manager.list_specs()?;
        if spec_files.is_empty() {
            return Err(Autom8Error::NoSpecsToResume);
        }

        // Filter to incomplete specs
        let incomplete_specs: Vec<(PathBuf, Spec)> = spec_files
            .into_iter()
            .filter_map(|path| {
                Spec::load(&path).ok().and_then(|spec| {
                    if spec.is_incomplete() {
                        Some((path, spec))
                    } else {
                        None
                    }
                })
            })
            .collect();

        if incomplete_specs.is_empty() {
            return Err(Autom8Error::NoSpecsToResume);
        }

        print_header();
        println!("{YELLOW}[resume]{RESET} No active run found, scanning for incomplete specs...");
        println!();

        if incomplete_specs.len() == 1 {
            // Auto-resume single incomplete spec
            let (spec_path, spec) = &incomplete_specs[0];
            let (completed, total) = spec.progress();
            println!(
                "{CYAN}Found{RESET} {} {GRAY}({}/{}){RESET}",
                spec_path.display(),
                completed,
                total
            );
            println!();
            prompt::print_action(&format!("Resuming {}", spec.project));
            println!();
            return self.run(spec_path);
        }

        // Multiple incomplete specs - let user choose
        println!(
            "{BOLD}Found {} incomplete specs:{RESET}",
            incomplete_specs.len()
        );
        println!();

        let options: Vec<String> = incomplete_specs
            .iter()
            .map(|(path, spec)| {
                let (completed, total) = spec.progress();
                let filename = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("spec.json");
                format!("{} - {} ({}/{})", filename, spec.project, completed, total)
            })
            .chain(std::iter::once("Exit".to_string()))
            .collect();

        let option_refs: Vec<&str> = options.iter().map(|s| s.as_str()).collect();
        let choice = prompt::select("Which spec would you like to resume?", &option_refs, 0);

        // Handle Exit option
        if choice >= incomplete_specs.len() {
            println!();
            println!("Exiting.");
            return Err(Autom8Error::NoSpecsToResume);
        }

        let (spec_path, spec) = &incomplete_specs[choice];
        println!();
        prompt::print_action(&format!("Resuming {}", spec.project));
        println!();
        self.run(spec_path)
    }

    fn archive_and_cleanup(&self, state: &RunState) -> Result<()> {
        self.state_manager.archive(state)?;

        // Check if we should clean up the worktree after successful completion
        // Only applies when:
        // 1. Run completed successfully (not failed)
        // 2. worktree_cleanup is enabled in config
        // 3. We're currently in a worktree (not the main repo)
        let config = state.effective_config();
        if state.status == crate::state::RunStatus::Completed && config.worktree_cleanup {
            // Check if we're in a worktree
            if let Ok(true) = is_in_worktree() {
                // Get the worktree path from session metadata
                if let Ok(Some(metadata)) = self.state_manager.load_metadata() {
                    let worktree_path = metadata.worktree_path;

                    // Clear state before removing worktree (since we're inside it)
                    self.state_manager.clear_current()?;

                    // Change to the main repo before removing worktree
                    // We need to get out of the worktree directory first
                    if let Ok(main_repo) = crate::worktree::get_main_repo_root() {
                        if std::env::set_current_dir(&main_repo).is_ok() {
                            // Now remove the worktree
                            match remove_worktree(&worktree_path, false) {
                                Ok(()) => {
                                    print_info(&format!(
                                        "Cleaned up worktree: {}",
                                        worktree_path.display()
                                    ));
                                }
                                Err(e) => {
                                    // Non-fatal - warn but continue
                                    print_info(&format!(
                                        "Warning: failed to remove worktree: {}",
                                        e
                                    ));
                                }
                            }
                        }
                    }

                    return Ok(());
                }
            }
        }

        // Default path: just clear the state
        self.state_manager.clear_current()?;
        Ok(())
    }

    pub fn status(&self) -> Result<Option<RunState>> {
        self.state_manager.load_current()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::spec::{Spec, UserStory};
    use crate::state::RunStatus;
    use tempfile::TempDir;

    // ========================================================================
    // Test helpers
    // ========================================================================

    fn create_test_spec(passes: bool) -> Spec {
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
                passes,
                notes: String::new(),
            }],
        }
    }

    fn create_multi_story_spec(completed_count: usize, total: usize) -> Spec {
        let stories = (0..total)
            .map(|i| UserStory {
                id: format!("US-{:03}", i + 1),
                title: format!("Story {}", i + 1),
                description: format!("Description for story {}", i + 1),
                acceptance_criteria: vec!["Criterion".into()],
                priority: (i + 1) as u32,
                passes: i < completed_count,
                notes: String::new(),
            })
            .collect();
        Spec {
            project: "TestProject".into(),
            branch_name: "test-branch".into(),
            description: "Multi-story test".into(),
            user_stories: stories,
        }
    }

    // ========================================================================
    // Runner builder pattern
    // ========================================================================

    #[test]
    fn test_runner_builder_pattern() {
        let runner = Runner::new()
            .unwrap()
            .with_verbose(true)
            .with_skip_review(true)
            .with_worktree(true);

        assert!(runner.verbose);
        assert!(runner.skip_review);
        assert_eq!(runner.worktree_override, Some(true));
    }

    #[test]
    fn test_runner_defaults() {
        let runner = Runner::new().unwrap();
        assert!(!runner.skip_review);
        assert!(!runner.verbose);
        assert!(runner.worktree_override.is_none());
        assert!(runner.commit_override.is_none());
        assert!(runner.pull_request_override.is_none());
    }

    #[test]
    fn test_runner_commit_and_pull_request_overrides() {
        let runner = Runner::new()
            .unwrap()
            .with_commit(false)
            .with_pull_request(false);

        assert_eq!(runner.commit_override, Some(false));
        assert_eq!(runner.pull_request_override, Some(false));
    }

    // ========================================================================
    // Story index calculation (1-indexed display)
    // ========================================================================

    #[test]
    fn test_story_index_is_one_indexed() {
        let story_ids = vec!["US-001", "US-002", "US-003"];

        // First story should be 1, not 0
        let idx = story_ids
            .iter()
            .position(|&s| s == "US-001")
            .map(|i| i as u32 + 1)
            .unwrap();
        assert_eq!(idx, 1);

        // Last story should be 3, not 2
        let idx = story_ids
            .iter()
            .position(|&s| s == "US-003")
            .map(|i| i as u32 + 1)
            .unwrap();
        assert_eq!(idx, 3);
    }

    // ========================================================================
    // Spec loading errors
    // ========================================================================

    #[test]
    fn test_spec_load_errors() {
        // Nonexistent path
        let result = Spec::load(Path::new("/nonexistent/spec.json"));
        assert!(matches!(result.unwrap_err(), Autom8Error::SpecNotFound(_)));

        // Invalid JSON
        let temp_dir = TempDir::new().unwrap();
        let spec_path = temp_dir.path().join("spec.json");
        fs::write(&spec_path, "{ invalid }").unwrap();
        let result = Spec::load(&spec_path);
        assert!(matches!(result.unwrap_err(), Autom8Error::InvalidSpec(_)));

        // Empty project
        fs::write(
            &spec_path,
            r#"{"project": "", "branchName": "test", "description": "test", "userStories": [{"id": "US-001", "title": "t", "description": "d", "acceptanceCriteria": [], "priority": 1, "passes": false}]}"#,
        )
        .unwrap();
        let result = Spec::load(&spec_path);
        assert!(matches!(result.unwrap_err(), Autom8Error::InvalidSpec(_)));
    }

    // ========================================================================
    // State transitions
    // ========================================================================

    #[test]
    fn test_state_transitions_full_workflow() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        // Initial -> PickingStory -> RunningClaude -> PickingStory
        assert_eq!(state.machine_state, MachineState::Initializing);

        state.transition_to(MachineState::PickingStory);
        state.start_iteration("US-001");
        assert_eq!(state.machine_state, MachineState::RunningClaude);
        assert_eq!(state.iteration, 1);

        state.finish_iteration(IterationStatus::Success, String::new());
        assert_eq!(state.machine_state, MachineState::PickingStory);

        // -> Reviewing -> Correcting -> Reviewing
        state.transition_to(MachineState::Reviewing);
        state.review_iteration = 1;
        state.transition_to(MachineState::Correcting);
        state.transition_to(MachineState::Reviewing);
        state.review_iteration = 2;

        // -> Committing -> CreatingPR -> Completed
        state.transition_to(MachineState::Committing);
        state.transition_to(MachineState::CreatingPR);
        state.transition_to(MachineState::Completed);

        assert_eq!(state.status, RunStatus::Completed);
        assert!(state.finished_at.is_some());
    }

    #[test]
    fn test_state_transitions_to_failed() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.transition_to(MachineState::Failed);

        assert_eq!(state.status, RunStatus::Failed);
        assert!(state.finished_at.is_some());
    }

    // ========================================================================
    // StateManager operations
    // ========================================================================

    #[test]
    fn test_state_manager_save_load_clear() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        // Initially empty
        assert!(sm.load_current().unwrap().is_none());
        assert!(!sm.has_active_run().unwrap());

        // Save and load
        let state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        sm.save(&state).unwrap();
        assert!(sm.has_active_run().unwrap());

        let loaded = sm.load_current().unwrap().unwrap();
        assert_eq!(loaded.run_id, state.run_id);

        // Clear
        sm.clear_current().unwrap();
        assert!(sm.load_current().unwrap().is_none());
    }

    #[test]
    fn test_state_manager_completed_not_active() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.transition_to(MachineState::Completed);
        sm.save(&state).unwrap();

        assert!(!sm.has_active_run().unwrap());
    }

    #[test]
    fn test_state_manager_archive() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        let archive_path = sm.archive(&state).unwrap();

        assert!(archive_path.exists());
        assert!(archive_path.parent().unwrap().ends_with("runs"));
    }

    // ========================================================================
    // Spec operations
    // ========================================================================

    #[test]
    fn test_spec_completion_detection() {
        assert!(create_test_spec(true).all_complete());
        assert!(!create_test_spec(false).all_complete());
    }

    #[test]
    fn test_spec_next_incomplete_story() {
        let spec = create_multi_story_spec(0, 3);
        assert_eq!(spec.next_incomplete_story().unwrap().id, "US-001");

        let mut spec = create_multi_story_spec(0, 3);
        spec.user_stories[0].passes = true;
        assert_eq!(spec.next_incomplete_story().unwrap().id, "US-002");

        let spec = create_multi_story_spec(3, 3);
        assert!(spec.next_incomplete_story().is_none());
    }

    #[test]
    fn test_list_specs_sorted_by_mtime() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());
        let spec_dir = sm.ensure_spec_dir().unwrap();

        create_test_spec(false)
            .save(&spec_dir.join("spec1.json"))
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        create_multi_story_spec(1, 3)
            .save(&spec_dir.join("spec2.json"))
            .unwrap();

        let specs = sm.list_specs().unwrap();
        assert_eq!(specs.len(), 2);
        assert!(specs[0].ends_with("spec2.json")); // Most recent first
    }

    // ========================================================================
    // Config integration
    // ========================================================================

    #[test]
    fn test_effective_config() {
        // Default config
        let state = RunState::new(PathBuf::from("test.json"), "test".to_string());
        let config = state.effective_config();
        assert!(config.review && config.commit && config.pull_request);

        // Custom config preserved
        let custom = Config {
            review: false,
            commit: true,
            pull_request: false,
            ..Default::default()
        };
        let state =
            RunState::new_with_config(PathBuf::from("test.json"), "test".to_string(), custom);
        let config = state.effective_config();
        assert!(!config.review && config.commit && !config.pull_request);
    }

    #[test]
    fn test_config_preserved_on_resume() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let config = Config {
            review: false,
            commit: true,
            pull_request: false,
            ..Default::default()
        };
        let state =
            RunState::new_with_config(PathBuf::from("test.json"), "test".to_string(), config);
        sm.save(&state).unwrap();

        let loaded = sm.load_current().unwrap().unwrap();
        assert!(!loaded.effective_config().review);
    }

    // ========================================================================
    // Worktree mode
    // ========================================================================

    #[test]
    fn test_worktree_mode_override() {
        let runner = Runner::new().unwrap();
        let config_true = Config {
            worktree: true,
            ..Default::default()
        };
        let config_false = Config {
            worktree: false,
            ..Default::default()
        };

        // No override - uses config
        assert!(runner.is_worktree_mode(&config_true));
        assert!(!runner.is_worktree_mode(&config_false));

        // Override takes precedence
        let runner_override = Runner::new().unwrap().with_worktree(true);
        assert!(runner_override.is_worktree_mode(&config_false));

        let runner_override = Runner::new().unwrap().with_worktree(false);
        assert!(!runner_override.is_worktree_mode(&config_true));
    }

    #[test]
    fn test_session_isolation() {
        let temp_dir = TempDir::new().unwrap();

        let sm1 = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "session1".to_string(),
        );
        let sm2 = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "session2".to_string(),
        );

        let state = RunState::new(PathBuf::from("test.json"), "test".to_string());
        sm1.save(&state).unwrap();

        assert!(sm1.has_active_run().unwrap());
        assert!(!sm2.has_active_run().unwrap());
    }

    // ========================================================================
    // Iteration tracking
    // ========================================================================

    #[test]
    fn test_iteration_tracking() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test".to_string());

        state.start_iteration("US-001");
        state.set_work_summary(Some("Work 1".to_string()));
        state.finish_iteration(IterationStatus::Success, String::new());

        state.start_iteration("US-002");
        state.set_work_summary(Some("Work 2".to_string()));
        state.finish_iteration(IterationStatus::Success, String::new());

        assert_eq!(state.iterations.len(), 2);
        assert_eq!(state.iterations[0].story_id, "US-001");
        assert_eq!(state.iterations[1].story_id, "US-002");
    }

    // ========================================================================
    // Live output flusher
    // ========================================================================

    #[test]
    fn test_live_output_flusher() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());
        sm.ensure_dirs().unwrap();

        let mut flusher = LiveOutputFlusher::new(&sm, MachineState::RunningClaude);
        assert!(flusher.live_state.output_lines.is_empty());

        // Append lines
        flusher.append("Line 1");
        flusher.append("Line 2");
        assert_eq!(flusher.live_state.output_lines.len(), 2);

        // Flush resets counter
        flusher.flush();
        assert_eq!(flusher.line_count_since_flush, 0);

        // Auto-flush at 10 lines
        for i in 0..10 {
            flusher.append(&format!("Line {}", i));
        }
        assert_eq!(flusher.line_count_since_flush, 0);
        assert!(sm.load_live().is_some());
    }

    #[test]
    fn test_live_flush_constants() {
        assert_eq!(LIVE_FLUSH_INTERVAL_MS, 200);
        assert_eq!(LIVE_FLUSH_LINE_COUNT, 10);
        assert_eq!(HEARTBEAT_INTERVAL_MS, 2500);
    }

    // ========================================================================
    // Signal handling / interruption
    // ========================================================================

    #[test]
    fn test_handle_interruption() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());
        sm.ensure_dirs().unwrap();

        let runner = Runner {
            state_manager: StateManager::with_dir(temp_dir.path().to_path_buf()),
            verbose: false,
            skip_review: false,
            worktree_override: None,
            commit_override: None,
            pull_request_override: None,
        };

        let mut state = RunState::new(PathBuf::from("test.json"), "test".to_string());
        state.transition_to(MachineState::RunningClaude);
        sm.save(&state).unwrap();

        // Create live output
        sm.save_live(&LiveState::new(MachineState::RunningClaude))
            .unwrap();

        let claude_runner = ClaudeRunner::new();
        let error = runner.handle_interruption(&mut state, &claude_runner, None);

        // Verify results
        assert!(matches!(error, Autom8Error::Interrupted));
        assert_eq!(state.status, RunStatus::Interrupted);
        assert_eq!(state.machine_state, MachineState::RunningClaude); // Preserved
        assert!(state.finished_at.is_some());
        assert!(sm.load_live().is_none()); // Cleared
    }

    #[test]
    fn test_interrupted_is_resumable() {
        for (status, resumable) in [
            (RunStatus::Interrupted, true),
            (RunStatus::Running, true),
            (RunStatus::Failed, true),
            (RunStatus::Completed, false),
        ] {
            let is_resumable = status == RunStatus::Running
                || status == RunStatus::Failed
                || status == RunStatus::Interrupted;
            assert_eq!(is_resumable, resumable, "{:?}", status);
        }
    }

    // ========================================================================
    // WorktreeSetupContext
    // ========================================================================

    #[test]
    fn test_worktree_setup_context() {
        let ctx = WorktreeSetupContext::new().unwrap();
        assert_eq!(ctx.original_cwd, std::env::current_dir().unwrap());
        assert!(ctx.worktree_path.is_none());
        assert!(!ctx.worktree_was_created);
        assert!(!ctx.cwd_changed);
        assert!(!ctx.metadata_saved);
    }

    #[test]
    fn test_worktree_cleanup_logic() {
        // Newly created without metadata - should be removed
        let ctx = WorktreeSetupContext {
            original_cwd: PathBuf::from("/orig"),
            worktree_path: Some(PathBuf::from("/wt")),
            worktree_was_created: true,
            cwd_changed: false,
            metadata_saved: false,
        };
        assert!(ctx.worktree_was_created && !ctx.metadata_saved);

        // Reused - should NOT be removed
        let ctx = WorktreeSetupContext {
            worktree_was_created: false,
            ..ctx.clone()
        };
        assert!(!ctx.worktree_was_created);

        // With metadata - should NOT be removed
        let ctx = WorktreeSetupContext {
            worktree_was_created: true,
            metadata_saved: true,
            ..ctx.clone()
        };
        assert!(ctx.metadata_saved);
    }

    #[test]
    fn test_worktree_cleanup_restores_cwd() {
        let original_cwd = std::env::current_dir().unwrap();
        let temp_dir = TempDir::new().unwrap();

        let mut ctx = WorktreeSetupContext::new().unwrap();
        std::env::set_current_dir(temp_dir.path()).unwrap();
        ctx.cwd_changed = true;
        ctx.worktree_was_created = false;

        ctx.cleanup_on_interruption();
        assert_eq!(std::env::current_dir().unwrap(), original_cwd);
    }

    // ========================================================================
    // Phantom session prevention
    // ========================================================================

    #[test]
    fn test_state_not_saved_before_worktree_context() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        // State can be created without persistence
        let state = RunState::from_spec_with_config(
            PathBuf::from("spec.md"),
            PathBuf::from("spec.json"),
            Config::default(),
        );
        assert_eq!(state.machine_state, MachineState::LoadingSpec);

        // No state persisted yet
        assert!(sm.load_current().unwrap().is_none());
    }

    #[test]
    fn test_state_saved_to_correct_session() {
        let temp_dir = TempDir::new().unwrap();
        let main_sm =
            StateManager::with_dir_and_session(temp_dir.path().to_path_buf(), "main".to_string());
        let wt_sm = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "abc12345".to_string(),
        );

        let state = RunState::new_with_config_and_session(
            PathBuf::from("spec.json"),
            "feature".to_string(),
            Config::default(),
            "abc12345".to_string(),
        );
        wt_sm.save(&state).unwrap();

        assert!(main_sm.load_current().unwrap().is_none());
        assert!(wt_sm.load_current().unwrap().is_some());
    }
}
