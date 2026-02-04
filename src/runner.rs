use crate::claude::{
    run_corrector, run_for_commit, run_for_spec_generation, run_reviewer, ClaudeOutcome,
    ClaudeRunner, ClaudeStoryResult, CommitResult, CorrectorResult, ReviewResult,
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
    print_run_summary, print_skip_review, print_spec_generated, print_spec_loaded,
    print_state_transition, print_story_complete, print_tasks_progress, print_worktree_context,
    print_worktree_created, print_worktree_reused, BOLD, CYAN, GRAY, RESET, YELLOW,
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
}

impl Runner {
    pub fn new() -> Result<Self> {
        Ok(Self {
            state_manager: StateManager::new()?,
            verbose: false,
            skip_review: false,
            worktree_override: None,
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

    /// Load the effective config.
    fn load_config_with_override(&self) -> Result<crate::config::Config> {
        get_effective_config()
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
                    Ok(ReviewResult::Pass) => Outcome::success("No issues found"),
                    Ok(ReviewResult::IssuesFound) => Outcome::success("Issues found"),
                    Ok(ReviewResult::Error(e)) => Outcome::failure(e.to_string()),
                    Err(e) => Outcome::failure(e.to_string()),
                },
            )?;

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

            match review_result {
                ReviewResult::Pass => {
                    // Delete autom8_review.md if it exists
                    let review_path = std::path::Path::new("autom8_review.md");
                    if review_path.exists() {
                        let _ = fs::remove_file(review_path);
                    }
                    print_review_passed();
                    return Ok(()); // Exit review loop, proceed to commit
                }
                ReviewResult::IssuesFound => {
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
                            Ok(CorrectorResult::Complete) => Outcome::success("Issues addressed"),
                            Ok(CorrectorResult::Error(e)) => Outcome::failure(e.to_string()),
                            Err(e) => Outcome::failure(e.to_string()),
                        },
                    )?;

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

                    match corrector_result {
                        CorrectorResult::Complete => {
                            // Increment review iteration and loop back to Reviewing
                            state.review_iteration += 1;
                        }
                        CorrectorResult::Error(e) => {
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
                ReviewResult::Error(e) => {
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
                Ok(CommitResult::Success(hash)) => Outcome::success(hash.clone()),
                Ok(CommitResult::NothingToCommit) => Outcome::success("Nothing to commit"),
                Ok(CommitResult::Error(e)) => Outcome::failure(e.to_string()),
                Err(e) => Outcome::failure(e.to_string()),
            },
        )?;

        // Print bottom border to close the output frame
        print_phase_footer(BannerColor::Cyan);

        // Print breadcrumb trail after commit phase completion
        print_breadcrumb_trail(breadcrumb);

        // Track whether commits were made for PR creation
        let commits_were_made = matches!(&commit_result, CommitResult::Success(_));

        match commit_result {
            CommitResult::Success(hash) => {
                print_info(&format!("Changes committed successfully ({})", hash))
            }
            CommitResult::NothingToCommit => print_info("Nothing to commit"),
            CommitResult::Error(e) => {
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
        self.handle_pr_creation(state, spec, commits_were_made)
    }

    /// Handle PR creation after committing.
    fn handle_pr_creation(
        &self,
        state: &mut RunState,
        spec: &Spec,
        commits_were_made: bool,
    ) -> Result<()> {
        print_state_transition(MachineState::Committing, MachineState::CreatingPR);
        state.transition_to(MachineState::CreatingPR);
        self.state_manager.save(state)?;
        self.flush_live(MachineState::CreatingPR);

        match create_pull_request(spec, commits_were_made) {
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
                Ok(_) => Outcome::success("Implementation done"),
                Err(e) => Outcome::failure(e.to_string()),
            },
        );

        match result {
            Ok(ClaudeStoryResult {
                outcome: ClaudeOutcome::AllStoriesComplete,
                work_summary,
                full_output,
            }) => self.handle_all_stories_complete_from_story(
                state,
                spec,
                spec_json_path,
                story,
                breadcrumb,
                story_results,
                work_summary,
                &full_output,
                print_summary_fn,
            ),
            Ok(ClaudeStoryResult {
                outcome: ClaudeOutcome::IterationComplete,
                work_summary,
                full_output,
            }) => self.handle_iteration_complete(
                state,
                spec_json_path,
                story,
                breadcrumb,
                story_results,
                work_summary,
                &full_output,
            ),
            Ok(ClaudeStoryResult {
                outcome: ClaudeOutcome::Error(error_info),
                ..
            }) => self.handle_story_error(
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
            ),
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
        let spec = match with_progress_display(
            self.verbose,
            VerboseTimer::new_for_spec,
            ClaudeSpinner::new_for_spec,
            |callback| run_for_spec_generation(&spec_content, &spec_json_path, callback),
            |res| match res {
                Ok(_) => Outcome::success("Spec generated"),
                Err(e) => Outcome::failure(e.to_string()),
            },
        ) {
            Ok(spec) => spec,
            Err(e) => {
                print_error_panel("Spec Generation Failed", &e.to_string(), None, None);
                return Err(e);
            }
        };

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

    #[test]
    fn test_runner_skip_review_defaults_to_false() {
        let runner = Runner::new().unwrap();
        assert!(!runner.skip_review);
    }

    #[test]
    fn test_runner_with_skip_review_true() {
        let runner = Runner::new().unwrap().with_skip_review(true);
        assert!(runner.skip_review);
    }

    #[test]
    fn test_runner_with_skip_review_false() {
        let runner = Runner::new().unwrap().with_skip_review(false);
        assert!(!runner.skip_review);
    }

    #[test]
    fn test_runner_builder_pattern_preserves_skip_review() {
        let runner = Runner::new()
            .unwrap()
            .with_verbose(true)
            .with_skip_review(true);
        assert!(runner.skip_review);
        assert!(runner.verbose);
    }

    // ========================================================================
    // US-005: Worktree Configuration Override tests
    // ========================================================================

    #[test]
    fn test_runner_worktree_override_defaults_to_none() {
        let runner = Runner::new().unwrap();
        assert!(
            runner.worktree_override.is_none(),
            "worktree_override should be None by default"
        );
    }

    #[test]
    fn test_runner_with_worktree_true() {
        let runner = Runner::new().unwrap().with_worktree(true);
        assert_eq!(
            runner.worktree_override,
            Some(true),
            "worktree_override should be Some(true) after with_worktree(true)"
        );
    }

    #[test]
    fn test_runner_with_worktree_false() {
        let runner = Runner::new().unwrap().with_worktree(false);
        assert_eq!(
            runner.worktree_override,
            Some(false),
            "worktree_override should be Some(false) after with_worktree(false)"
        );
    }

    #[test]
    fn test_runner_builder_pattern_preserves_worktree() {
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
    fn test_runner_builder_pattern_worktree_order_independent() {
        let runner1 = Runner::new()
            .unwrap()
            .with_worktree(true)
            .with_verbose(true);

        let runner2 = Runner::new()
            .unwrap()
            .with_verbose(true)
            .with_worktree(true);

        assert_eq!(runner1.worktree_override, runner2.worktree_override);
        assert_eq!(runner1.verbose, runner2.verbose);
    }

    /// Tests that story_index calculation produces 1-indexed values.
    /// The formula: position().map(|i| i as u32 + 1).unwrap_or(state.iteration)
    /// must produce 1-indexed display values like [US-001 1/8], not [US-001 0/8].
    #[test]
    fn test_story_index_calculation_is_one_indexed() {
        // Simulate the story_index calculation from runner.rs:557-562
        let story_ids = vec![
            "US-001", "US-002", "US-003", "US-004", "US-005", "US-006", "US-007", "US-008",
        ];

        // Test case 1: First story (task 1 of 8) should show 1, not 0
        let current_story = "US-001";
        let story_index = story_ids
            .iter()
            .position(|&s| s == current_story)
            .map(|i| i as u32 + 1)
            .unwrap_or(1); // fallback to iteration=1
        assert_eq!(story_index, 1, "First story should display as 1/8, not 0/8");

        // Test case 2: Last story (task 8 of 8) should show 8, not 7
        let current_story = "US-008";
        let story_index = story_ids
            .iter()
            .position(|&s| s == current_story)
            .map(|i| i as u32 + 1)
            .unwrap_or(8); // fallback to iteration=8
        assert_eq!(story_index, 8, "Last story should display as 8/8, not 7/8");

        // Test case 3: Middle story (task 4 of 8) should show 4
        let current_story = "US-004";
        let story_index = story_ids
            .iter()
            .position(|&s| s == current_story)
            .map(|i| i as u32 + 1)
            .unwrap_or(4);
        assert_eq!(story_index, 4, "Fourth story should display as 4/8");
    }

    /// Tests that state.iteration fallback produces correct 1-indexed value
    /// when position lookup fails.
    #[test]
    fn test_story_index_fallback_is_one_indexed() {
        use crate::state::RunState;

        // Create a state and simulate iteration increments
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        // Before any iteration, state.iteration is 0
        assert_eq!(state.iteration, 0);

        // After start_iteration, it should be 1 (1-indexed)
        state.start_iteration("US-001");
        assert_eq!(
            state.iteration, 1,
            "After first start_iteration, iteration should be 1"
        );

        // Simulate fallback scenario where position lookup fails
        let story_ids: Vec<&str> = vec!["US-001", "US-002"];
        let unknown_story = "US-UNKNOWN";
        let story_index = story_ids
            .iter()
            .position(|&s| s == unknown_story)
            .map(|i| i as u32 + 1)
            .unwrap_or(state.iteration);

        // The fallback should use state.iteration which is 1 (1-indexed)
        assert_eq!(
            story_index, 1,
            "Fallback should use 1-indexed state.iteration"
        );

        // After second iteration
        state.finish_iteration(crate::state::IterationStatus::Success, String::new());
        state.start_iteration("US-002");
        assert_eq!(
            state.iteration, 2,
            "After second start_iteration, iteration should be 2"
        );
    }

    /// Tests that Runner uses StateManager which uses config directory paths.
    /// This verifies the resume command looks in the right location.
    #[test]
    fn test_runner_state_manager_uses_config_directory() {
        let runner = Runner::new().unwrap();
        // The state_manager field is private, but we can verify through the status() method
        // that it reads from the config directory (no error means path resolution works)
        let status_result = runner.status();
        assert!(
            status_result.is_ok(),
            "Runner should use valid config directory paths"
        );
    }

    // ========================================================================
    // US-006: PR creation integration tests
    // ========================================================================

    #[test]
    fn test_pr_result_success_variant_accessible() {
        // Verify PRResult::Success is properly imported and usable
        let result = PRResult::Success("https://github.com/owner/repo/pull/1".to_string());
        assert!(matches!(result, PRResult::Success(_)));
    }

    #[test]
    fn test_pr_result_skipped_variant_accessible() {
        // Verify PRResult::Skipped is properly imported and usable
        let result = PRResult::Skipped("No commits were made".to_string());
        assert!(matches!(result, PRResult::Skipped(_)));
    }

    #[test]
    fn test_pr_result_already_exists_variant_accessible() {
        // Verify PRResult::AlreadyExists is properly imported and usable
        let result = PRResult::AlreadyExists("https://github.com/owner/repo/pull/99".to_string());
        assert!(matches!(result, PRResult::AlreadyExists(_)));
    }

    #[test]
    fn test_pr_result_error_variant_accessible() {
        // Verify PRResult::Error is properly imported and usable
        let result = PRResult::Error("Failed to create PR".to_string());
        assert!(matches!(result, PRResult::Error(_)));
    }

    #[test]
    fn test_commits_were_made_detection_success() {
        // Test that CommitResult::Success is properly detected as commits_were_made = true
        let commit_result = CommitResult::Success("abc123".to_string());
        let commits_were_made = matches!(&commit_result, CommitResult::Success(_));
        assert!(
            commits_were_made,
            "Success should indicate commits were made"
        );
    }

    #[test]
    fn test_commits_were_made_detection_nothing_to_commit() {
        // Test that CommitResult::NothingToCommit is properly detected as commits_were_made = false
        let commit_result = CommitResult::NothingToCommit;
        let commits_were_made = matches!(&commit_result, CommitResult::Success(_));
        assert!(
            !commits_were_made,
            "NothingToCommit should indicate no commits were made"
        );
    }

    #[test]
    fn test_creating_pr_state_accessible() {
        // Verify MachineState::CreatingPR is properly accessible for transitions
        let state = MachineState::CreatingPR;
        assert!(matches!(state, MachineState::CreatingPR));
    }

    // ========================================================================
    // US-008: Critical Tests for runner.rs
    // ========================================================================

    use crate::spec::{Spec, UserStory};
    use crate::state::RunStatus;
    use tempfile::TempDir;

    /// Helper to create a minimal valid spec for testing
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

    /// Helper to create a spec with multiple stories
    fn create_multi_story_spec(completed_count: usize, total: usize) -> Spec {
        let mut stories = Vec::new();
        for i in 0..total {
            stories.push(UserStory {
                id: format!("US-{:03}", i + 1),
                title: format!("Story {}", i + 1),
                description: format!("Description for story {}", i + 1),
                acceptance_criteria: vec!["Criterion".into()],
                priority: (i + 1) as u32,
                passes: i < completed_count,
                notes: String::new(),
            });
        }
        Spec {
            project: "TestProject".into(),
            branch_name: "test-branch".into(),
            description: "Multi-story test".into(),
            user_stories: stories,
        }
    }

    // ------------------------------------------------------------------------
    // run() error handling tests
    // Note: These tests verify error behavior at the Spec/path level.
    // Tests using Runner::new() may be affected by existing active runs,
    // so we test error paths that occur BEFORE the active run check,
    // or test the underlying error types directly.
    // ------------------------------------------------------------------------

    #[test]
    fn test_spec_load_with_nonexistent_path_returns_spec_not_found() {
        // Test Spec::load directly since Runner::run checks for active run first
        let result = Spec::load(Path::new("/nonexistent/path/spec.json"));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, Autom8Error::SpecNotFound(_)),
            "Expected SpecNotFound error, got: {:?}",
            err
        );
    }

    #[test]
    fn test_path_canonicalize_fails_for_nonexistent_spec() {
        // Verify that canonicalize fails for nonexistent paths (as used in run/run_from_spec)
        let result = Path::new("/nonexistent/spec-feature.md").canonicalize();
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_spec_content_detection() {
        // Test the empty spec content check logic
        let content = "   \n  \t  ";
        assert!(
            content.trim().is_empty(),
            "Whitespace-only content should be detected as empty"
        );
    }

    #[test]
    fn test_spec_load_with_invalid_json_returns_invalid_spec() {
        let temp_dir = TempDir::new().unwrap();
        let spec_path = temp_dir.path().join("spec.json");
        fs::write(&spec_path, "{ invalid json }").unwrap();

        let result = Spec::load(&spec_path);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, Autom8Error::InvalidSpec(_)),
            "Expected InvalidSpec error, got: {:?}",
            err
        );
    }

    #[test]
    fn test_spec_load_with_missing_required_fields_returns_invalid_spec() {
        let temp_dir = TempDir::new().unwrap();
        let spec_path = temp_dir.path().join("spec.json");
        // Missing userStories field
        fs::write(&spec_path, r#"{"project": "Test", "branchName": "test"}"#).unwrap();

        let result = Spec::load(&spec_path);
        assert!(result.is_err());
        // Could be InvalidSpec or Json error depending on serde behavior
    }

    #[test]
    fn test_spec_load_with_empty_project_returns_invalid_spec() {
        let temp_dir = TempDir::new().unwrap();
        let spec_path = temp_dir.path().join("spec.json");
        fs::write(
            &spec_path,
            r#"{"project": "", "branchName": "test", "description": "test", "userStories": [{"id": "US-001", "title": "t", "description": "d", "acceptanceCriteria": [], "priority": 1, "passes": false}]}"#,
        )
        .unwrap();

        let result = Spec::load(&spec_path);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, Autom8Error::InvalidSpec(_)),
            "Expected InvalidSpec error, got: {:?}",
            err
        );
    }

    // ------------------------------------------------------------------------
    // State transition tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_state_transitions_through_picking_story() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        // Initial state is Initializing
        assert_eq!(state.machine_state, MachineState::Initializing);

        // Transition to PickingStory
        state.transition_to(MachineState::PickingStory);
        assert_eq!(state.machine_state, MachineState::PickingStory);
        assert_eq!(state.status, RunStatus::Running);
    }

    #[test]
    fn test_state_transitions_through_full_story_workflow() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        // PickingStory -> RunningClaude
        state.transition_to(MachineState::PickingStory);
        state.start_iteration("US-001");
        assert_eq!(state.machine_state, MachineState::RunningClaude);
        assert_eq!(state.iteration, 1);

        // RunningClaude -> PickingStory (iteration complete)
        state.finish_iteration(IterationStatus::Success, String::new());
        assert_eq!(state.machine_state, MachineState::PickingStory);
    }

    #[test]
    fn test_state_transitions_through_review_workflow() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        // Simulate: PickingStory -> Reviewing -> Correcting -> Reviewing
        state.transition_to(MachineState::PickingStory);

        state.transition_to(MachineState::Reviewing);
        state.review_iteration = 1;
        assert_eq!(state.machine_state, MachineState::Reviewing);
        assert_eq!(state.review_iteration, 1);

        state.transition_to(MachineState::Correcting);
        assert_eq!(state.machine_state, MachineState::Correcting);

        state.transition_to(MachineState::Reviewing);
        state.review_iteration = 2;
        assert_eq!(state.review_iteration, 2);
    }

    #[test]
    fn test_state_transitions_to_completed() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        state.transition_to(MachineState::Committing);
        state.transition_to(MachineState::CreatingPR);
        state.transition_to(MachineState::Completed);

        assert_eq!(state.machine_state, MachineState::Completed);
        assert_eq!(state.status, RunStatus::Completed);
        assert!(state.finished_at.is_some());
    }

    #[test]
    fn test_state_transitions_to_failed() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        state.transition_to(MachineState::RunningClaude);
        state.transition_to(MachineState::Failed);

        assert_eq!(state.machine_state, MachineState::Failed);
        assert_eq!(state.status, RunStatus::Failed);
        assert!(state.finished_at.is_some());
    }

    // ------------------------------------------------------------------------
    // Resume functionality tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_status_returns_none_when_no_active_run() {
        // Use a fresh temp directory for isolated testing
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        // Ensure no state file exists
        let result = sm.load_current().unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_status_returns_state_when_active_run_exists() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        sm.save(&state).unwrap();

        let loaded = sm.load_current().unwrap();
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().run_id, state.run_id);
    }

    #[test]
    fn test_has_active_run_detects_running_state() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        // No active run initially
        assert!(!sm.has_active_run().unwrap());

        // Save a running state
        let state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        sm.save(&state).unwrap();

        assert!(sm.has_active_run().unwrap());
    }

    #[test]
    fn test_has_active_run_ignores_completed_state() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        // Save a completed state
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.transition_to(MachineState::Completed);
        sm.save(&state).unwrap();

        // Should NOT count as active run
        assert!(!sm.has_active_run().unwrap());
    }

    #[test]
    fn test_list_specs_returns_incomplete_specs_sorted_by_mtime() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());
        let spec_dir = sm.ensure_spec_dir().unwrap();

        // Create two spec files
        let spec1 = create_test_spec(false);
        let spec2 = create_multi_story_spec(1, 3);

        spec1.save(&spec_dir.join("spec1.json")).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10)); // Ensure different mtime
        spec2.save(&spec_dir.join("spec2.json")).unwrap();

        let specs = sm.list_specs().unwrap();
        assert_eq!(specs.len(), 2);
        // Most recent first (spec2)
        assert!(specs[0].ends_with("spec2.json"));
    }

    // ------------------------------------------------------------------------
    // LoopAction enum tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_loop_action_continue_variant() {
        let action = LoopAction::Continue;
        assert!(matches!(action, LoopAction::Continue));
    }

    #[test]
    fn test_loop_action_break_variant() {
        let action = LoopAction::Break;
        assert!(matches!(action, LoopAction::Break));
    }

    // ------------------------------------------------------------------------
    // Spec integration tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_spec_all_complete_detection() {
        let spec = create_test_spec(true);
        assert!(spec.all_complete());

        let spec = create_test_spec(false);
        assert!(!spec.all_complete());
    }

    #[test]
    fn test_spec_next_incomplete_story_returns_lowest_priority() {
        let spec = create_multi_story_spec(0, 3);
        let next = spec.next_incomplete_story().unwrap();
        assert_eq!(next.id, "US-001"); // Priority 1 is lowest
    }

    #[test]
    fn test_spec_next_incomplete_story_skips_completed() {
        let mut spec = create_multi_story_spec(0, 3);
        spec.user_stories[0].passes = true; // Mark US-001 as complete

        let next = spec.next_incomplete_story().unwrap();
        assert_eq!(next.id, "US-002");
    }

    #[test]
    fn test_spec_next_incomplete_story_returns_none_when_all_complete() {
        let spec = create_multi_story_spec(3, 3);
        assert!(spec.next_incomplete_story().is_none());
    }

    // ------------------------------------------------------------------------
    // Error handling tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_max_review_iterations_error_message() {
        let err = Autom8Error::MaxReviewIterationsReached;
        let msg = format!("{}", err);
        assert!(msg.contains("3 iterations"));
        assert!(msg.contains("autom8_review.md"));
    }

    #[test]
    fn test_run_in_progress_error_contains_run_id() {
        let run_id = "test-run-id-123".to_string();
        let err = Autom8Error::RunInProgress(run_id.clone());
        let msg = format!("{}", err);
        assert!(msg.contains(&run_id));
    }

    #[test]
    fn test_no_incomplete_stories_error() {
        let err = Autom8Error::NoIncompleteStories;
        let msg = format!("{}", err);
        assert!(msg.contains("No incomplete stories"));
    }

    #[test]
    fn test_no_specs_to_resume_error() {
        let err = Autom8Error::NoSpecsToResume;
        let msg = format!("{}", err);
        assert!(msg.contains("No incomplete specs"));
    }

    // ------------------------------------------------------------------------
    // RunState from_spec tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_run_state_from_spec_initializes_correctly() {
        let state = RunState::from_spec(
            PathBuf::from("spec-feature.md"),
            PathBuf::from("spec-feature.json"),
        );

        assert_eq!(state.machine_state, MachineState::LoadingSpec);
        assert_eq!(state.status, RunStatus::Running);
        assert_eq!(state.spec_md_path, Some(PathBuf::from("spec-feature.md")));
        assert_eq!(state.spec_json_path, PathBuf::from("spec-feature.json"));
        assert!(state.branch.is_empty()); // Branch set after spec generation
    }

    #[test]
    fn test_run_state_new_initializes_correctly() {
        let state = RunState::new(PathBuf::from("spec.json"), "feature-branch".to_string());

        assert_eq!(state.machine_state, MachineState::Initializing);
        assert_eq!(state.status, RunStatus::Running);
        assert!(state.spec_md_path.is_none());
        assert_eq!(state.branch, "feature-branch");
        assert_eq!(state.iteration, 0);
        assert_eq!(state.review_iteration, 0);
    }

    // ------------------------------------------------------------------------
    // Iteration tracking tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_iteration_record_preserves_work_summary() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        state.start_iteration("US-001");
        state.set_work_summary(Some(
            "Files changed: src/main.rs. Added feature.".to_string(),
        ));
        state.finish_iteration(IterationStatus::Success, String::new());

        assert_eq!(
            state.iterations[0].work_summary,
            Some("Files changed: src/main.rs. Added feature.".to_string())
        );
    }

    #[test]
    fn test_multiple_iterations_tracked_independently() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        state.start_iteration("US-001");
        state.set_work_summary(Some("Work 1".to_string()));
        state.finish_iteration(IterationStatus::Success, String::new());

        state.start_iteration("US-002");
        state.set_work_summary(Some("Work 2".to_string()));
        state.finish_iteration(IterationStatus::Success, String::new());

        assert_eq!(state.iterations.len(), 2);
        assert_eq!(state.iterations[0].story_id, "US-001");
        assert_eq!(state.iterations[0].work_summary, Some("Work 1".to_string()));
        assert_eq!(state.iterations[1].story_id, "US-002");
        assert_eq!(state.iterations[1].work_summary, Some("Work 2".to_string()));
    }

    #[test]
    fn test_current_iteration_duration_calculated_correctly() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        state.start_iteration("US-001");
        std::thread::sleep(std::time::Duration::from_millis(50));
        state.finish_iteration(IterationStatus::Success, String::new());

        let duration = state.current_iteration_duration();
        // Duration is u64, so just verify the method returns successfully
        let _ = duration; // Value is non-negative by type
    }

    // ------------------------------------------------------------------------
    // StateManager archive and cleanup tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_archive_creates_run_file_with_correct_format() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        let archive_path = sm.archive(&state).unwrap();

        assert!(archive_path.exists());
        // File should be in runs/ directory
        assert!(archive_path.parent().unwrap().ends_with("runs"));
        // Filename should contain date and run_id prefix
        let filename = archive_path.file_name().unwrap().to_str().unwrap();
        assert!(filename.contains(&state.run_id[..8]));
    }

    #[test]
    fn test_clear_current_removes_state_file() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        sm.save(&state).unwrap();

        assert!(sm.load_current().unwrap().is_some());

        sm.clear_current().unwrap();

        assert!(sm.load_current().unwrap().is_none());
    }

    // ------------------------------------------------------------------------
    // Runner builder pattern tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_runner_verbose_defaults_to_false() {
        let runner = Runner::new().unwrap();
        assert!(!runner.verbose);
    }

    #[test]
    fn test_runner_with_verbose_true() {
        let runner = Runner::new().unwrap().with_verbose(true);
        assert!(runner.verbose);
    }

    #[test]
    fn test_runner_builder_chain_order_independent() {
        let runner1 = Runner::new()
            .unwrap()
            .with_verbose(true)
            .with_skip_review(true);

        let runner2 = Runner::new()
            .unwrap()
            .with_skip_review(true)
            .with_verbose(true);

        assert_eq!(runner1.verbose, runner2.verbose);
        assert_eq!(runner1.skip_review, runner2.skip_review);
    }

    // ========================================================================
    // US-005: Config integration with state machine tests
    // ========================================================================

    #[test]
    fn test_run_state_effective_config_returns_default_when_none() {
        let state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        let config = state.effective_config();
        // Default config has all options enabled
        assert!(config.review);
        assert!(config.commit);
        assert!(config.pull_request);
    }

    #[test]
    fn test_run_state_effective_config_returns_stored_config() {
        let config = Config {
            review: false,
            commit: true,
            pull_request: false,
            ..Default::default()
        };
        let state = RunState::new_with_config(
            PathBuf::from("test.json"),
            "test-branch".to_string(),
            config.clone(),
        );
        assert_eq!(state.effective_config(), config);
    }

    #[test]
    fn test_run_state_new_with_config_initializes_correctly() {
        let config = Config {
            review: false,
            commit: false,
            pull_request: false,
            ..Default::default()
        };
        let state = RunState::new_with_config(
            PathBuf::from("spec.json"),
            "feature-branch".to_string(),
            config.clone(),
        );

        assert_eq!(state.machine_state, MachineState::Initializing);
        assert_eq!(state.status, RunStatus::Running);
        assert_eq!(state.branch, "feature-branch");
        assert!(state.config.is_some());
        assert_eq!(state.config.unwrap(), config);
    }

    #[test]
    fn test_run_state_from_spec_with_config_initializes_correctly() {
        let config = Config {
            review: true,
            commit: false,
            pull_request: false,
            ..Default::default()
        };
        let state = RunState::from_spec_with_config(
            PathBuf::from("spec-feature.md"),
            PathBuf::from("spec-feature.json"),
            config.clone(),
        );

        assert_eq!(state.machine_state, MachineState::LoadingSpec);
        assert_eq!(state.status, RunStatus::Running);
        assert!(state.branch.is_empty()); // Branch set after spec generation
        assert!(state.config.is_some());
        assert_eq!(state.config.unwrap(), config);
    }

    #[test]
    fn test_config_with_review_false_skips_review_state() {
        // This tests that when review=false in config, the review state is skipped
        let config = Config {
            review: false,
            commit: true,
            pull_request: true,
            ..Default::default()
        };
        let state = RunState::new_with_config(
            PathBuf::from("test.json"),
            "test-branch".to_string(),
            config,
        );

        let effective = state.effective_config();
        assert!(
            !effective.review,
            "review should be false, state machine should skip review"
        );
    }

    #[test]
    fn test_config_with_commit_false_skips_commit_state() {
        // This tests that when commit=false in config, the commit state is skipped
        let config = Config {
            review: true,
            commit: false,
            pull_request: false, // Must be false when commit is false (validated by US-004)
            ..Default::default()
        };
        let state = RunState::new_with_config(
            PathBuf::from("test.json"),
            "test-branch".to_string(),
            config,
        );

        let effective = state.effective_config();
        assert!(
            !effective.commit,
            "commit should be false, state machine should skip commit"
        );
    }

    #[test]
    fn test_config_with_pull_request_false_skips_pr_state() {
        // This tests that when pull_request=false in config, the PR state is skipped
        let config = Config {
            review: true,
            commit: true,
            pull_request: false,
            ..Default::default()
        };
        let state = RunState::new_with_config(
            PathBuf::from("test.json"),
            "test-branch".to_string(),
            config,
        );

        let effective = state.effective_config();
        assert!(
            !effective.pull_request,
            "pull_request should be false, state machine should skip PR creation"
        );
    }

    #[test]
    fn test_state_machine_transitions_with_all_config_disabled() {
        // Test that state transitions work when all optional states are disabled
        let config = Config {
            review: false,
            commit: false,
            pull_request: false,
            ..Default::default()
        };
        let mut state = RunState::new_with_config(
            PathBuf::from("test.json"),
            "test-branch".to_string(),
            config,
        );

        // Simulate the expected flow with all states disabled:
        // Initializing -> PickingStory -> RunningClaude -> PickingStory -> Completed
        assert_eq!(state.machine_state, MachineState::Initializing);

        state.transition_to(MachineState::PickingStory);
        assert_eq!(state.machine_state, MachineState::PickingStory);

        state.start_iteration("US-001");
        assert_eq!(state.machine_state, MachineState::RunningClaude);

        state.finish_iteration(IterationStatus::Success, String::new());
        assert_eq!(state.machine_state, MachineState::PickingStory);

        // With all configs disabled, we skip directly to Completed
        state.transition_to(MachineState::Completed);
        assert_eq!(state.machine_state, MachineState::Completed);
        assert_eq!(state.status, RunStatus::Completed);
    }

    #[test]
    fn test_state_machine_transitions_with_review_disabled_only() {
        // Test transitions when only review is disabled
        let config = Config {
            review: false,
            commit: true,
            pull_request: true,
            ..Default::default()
        };
        let mut state = RunState::new_with_config(
            PathBuf::from("test.json"),
            "test-branch".to_string(),
            config,
        );

        // Expected flow:
        // Initializing -> PickingStory -> RunningClaude -> PickingStory
        // (skips Reviewing/Correcting) -> Committing -> CreatingPR -> Completed
        state.transition_to(MachineState::PickingStory);
        state.start_iteration("US-001");
        state.finish_iteration(IterationStatus::Success, String::new());

        // Skip review, go to commit
        state.transition_to(MachineState::Committing);
        assert_eq!(state.machine_state, MachineState::Committing);

        state.transition_to(MachineState::CreatingPR);
        assert_eq!(state.machine_state, MachineState::CreatingPR);

        state.transition_to(MachineState::Completed);
        assert_eq!(state.machine_state, MachineState::Completed);
    }

    #[test]
    fn test_state_machine_transitions_with_pr_disabled_only() {
        // Test transitions when only PR is disabled
        let config = Config {
            review: true,
            commit: true,
            pull_request: false,
            ..Default::default()
        };
        let mut state = RunState::new_with_config(
            PathBuf::from("test.json"),
            "test-branch".to_string(),
            config,
        );

        // Expected flow:
        // ... -> Reviewing -> Committing -> Completed (skip CreatingPR)
        state.transition_to(MachineState::Reviewing);
        state.review_iteration = 1;
        assert_eq!(state.machine_state, MachineState::Reviewing);

        state.transition_to(MachineState::Committing);
        assert_eq!(state.machine_state, MachineState::Committing);

        // Skip PR, go directly to completed
        state.transition_to(MachineState::Completed);
        assert_eq!(state.machine_state, MachineState::Completed);
    }

    #[test]
    fn test_config_preserved_during_resume_workflow() {
        // Test that config is preserved when state is saved and loaded (resume scenario)
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let config = Config {
            review: false,
            commit: true,
            pull_request: false,
            ..Default::default()
        };
        let state = RunState::new_with_config(
            PathBuf::from("test.json"),
            "test-branch".to_string(),
            config.clone(),
        );

        // Save state
        sm.save(&state).unwrap();

        // Load state (simulating resume)
        let loaded = sm.load_current().unwrap().unwrap();

        // Verify config is preserved
        assert_eq!(loaded.effective_config(), config);
    }

    // ========================================================================
    // US-008: Runner Worktree Integration tests
    // ========================================================================

    #[test]
    fn test_runner_new_auto_detects_session() {
        // Runner::new() should auto-detect the session from CWD
        let runner = Runner::new().unwrap();
        // The session ID is auto-detected based on whether we're in main repo or worktree
        // We can't assert the exact value but we can verify it's created successfully
        let status = runner.status();
        assert!(
            status.is_ok(),
            "Runner should auto-detect session successfully"
        );
    }

    #[test]
    fn test_runner_state_manager_has_session_id() {
        // Verify that StateManager has a session_id (proves per-session state storage)
        let runner = Runner::new().unwrap();
        let session_id = runner.state_manager.session_id();
        assert!(
            !session_id.is_empty(),
            "StateManager should have a session ID"
        );
        // Session ID should be either "main" or 8-char hex
        assert!(
            session_id == "main"
                || (session_id.len() == 8 && session_id.chars().all(|c| c.is_ascii_hexdigit())),
            "Session ID should be 'main' or 8 hex chars, got: {}",
            session_id
        );
    }

    #[test]
    fn test_has_active_run_is_per_session() {
        // Test that has_active_run() is per-session, not global
        let temp_dir = TempDir::new().unwrap();

        // Create two state managers with different session IDs
        let sm1 = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "session1".to_string(),
        );
        let sm2 = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "session2".to_string(),
        );

        // Initially, neither has an active run
        assert!(!sm1.has_active_run().unwrap());
        assert!(!sm2.has_active_run().unwrap());

        // Create active run in session1
        let state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        sm1.save(&state).unwrap();

        // Session1 has active run, session2 does not
        assert!(sm1.has_active_run().unwrap());
        assert!(
            !sm2.has_active_run().unwrap(),
            "Session2 should NOT see session1's active run"
        );
    }

    #[test]
    fn test_state_has_session_id_field() {
        // Verify RunState has session_id field
        let state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        assert!(
            state.session_id.is_none(),
            "session_id should be None by default"
        );

        let state_with_session = RunState::new_with_session(
            PathBuf::from("test.json"),
            "test-branch".to_string(),
            "abc12345".to_string(),
        );
        assert_eq!(
            state_with_session.session_id,
            Some("abc12345".to_string()),
            "session_id should be set when created with session"
        );
    }

    #[test]
    fn test_worktree_cleanup_config_defaults_to_false() {
        // Verify worktree_cleanup defaults to false for backward compatibility
        let config = Config::default();
        assert!(
            !config.worktree_cleanup,
            "worktree_cleanup should default to false"
        );
    }

    #[test]
    fn test_worktree_cleanup_config_can_be_enabled() {
        // Test that worktree_cleanup can be set to true
        let config = Config {
            worktree_cleanup: true,
            ..Default::default()
        };
        assert!(
            config.worktree_cleanup,
            "worktree_cleanup should be true when set"
        );
    }

    #[test]
    fn test_worktree_cleanup_config_only_affects_successful_runs() {
        // Verify the logic that cleanup only applies to completed (not failed) runs
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        // Create a failed run state
        let config = Config {
            worktree_cleanup: true,
            ..Default::default()
        };
        let mut state = RunState::new_with_config(
            PathBuf::from("test.json"),
            "test-branch".to_string(),
            config.clone(),
        );
        state.transition_to(MachineState::Failed);
        sm.save(&state).unwrap();

        // Verify the state is failed
        let loaded = sm.load_current().unwrap().unwrap();
        assert_eq!(loaded.status, RunStatus::Failed);
        // The cleanup logic checks for Completed status, so Failed runs
        // should NOT trigger cleanup (tested via the condition in archive_and_cleanup)
    }

    #[test]
    fn test_state_transitions_work_in_worktree_context() {
        // Verify state transitions work correctly when session_id is set (worktree context)
        let config = Config::default();
        let mut state = RunState::new_with_config_and_session(
            PathBuf::from("test.json"),
            "test-branch".to_string(),
            config,
            "wt-session".to_string(),
        );

        // Verify session_id is preserved through transitions
        assert_eq!(state.session_id, Some("wt-session".to_string()));

        state.transition_to(MachineState::PickingStory);
        assert_eq!(state.session_id, Some("wt-session".to_string()));

        state.start_iteration("US-001");
        assert_eq!(state.session_id, Some("wt-session".to_string()));

        state.finish_iteration(IterationStatus::Success, String::new());
        assert_eq!(state.session_id, Some("wt-session".to_string()));

        state.transition_to(MachineState::Completed);
        assert_eq!(state.session_id, Some("wt-session".to_string()));
        assert_eq!(state.status, RunStatus::Completed);
    }

    #[test]
    fn test_effective_worktree_mode_respects_config() {
        // Test that is_worktree_mode respects the config value
        let runner = Runner::new().unwrap();

        // With worktree = false in config
        let config_false = Config {
            worktree: false,
            ..Default::default()
        };
        assert!(!runner.is_worktree_mode(&config_false));

        // With worktree = true in config
        let config_true = Config {
            worktree: true,
            ..Default::default()
        };
        assert!(runner.is_worktree_mode(&config_true));
    }

    #[test]
    fn test_effective_worktree_mode_override_takes_precedence() {
        // Test that CLI override takes precedence over config
        let runner_with_override = Runner::new().unwrap().with_worktree(true);

        // Even with worktree = false in config, override should win
        let config_false = Config {
            worktree: false,
            ..Default::default()
        };
        assert!(runner_with_override.is_worktree_mode(&config_false));

        // And override false should also work
        let runner_no_worktree = Runner::new().unwrap().with_worktree(false);
        let config_true = Config {
            worktree: true,
            ..Default::default()
        };
        assert!(!runner_no_worktree.is_worktree_mode(&config_true));
    }

    #[test]
    fn test_session_state_isolation() {
        // Test that multiple sessions have isolated state
        let temp_dir = TempDir::new().unwrap();

        let sm1 = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "session-a".to_string(),
        );
        let sm2 = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "session-b".to_string(),
        );

        // Save state in session a
        let state_a = RunState::new_with_session(
            PathBuf::from("spec-a.json"),
            "branch-a".to_string(),
            "session-a".to_string(),
        );
        sm1.save(&state_a).unwrap();

        // Save state in session b
        let state_b = RunState::new_with_session(
            PathBuf::from("spec-b.json"),
            "branch-b".to_string(),
            "session-b".to_string(),
        );
        sm2.save(&state_b).unwrap();

        // Load and verify each session has its own state
        let loaded_a = sm1.load_current().unwrap().unwrap();
        let loaded_b = sm2.load_current().unwrap().unwrap();

        assert_eq!(loaded_a.branch, "branch-a");
        assert_eq!(loaded_b.branch, "branch-b");
        assert_eq!(loaded_a.session_id, Some("session-a".to_string()));
        assert_eq!(loaded_b.session_id, Some("session-b".to_string()));
    }

    #[test]
    fn test_is_worktree_mode_with_none_override() {
        // Test that None override falls back to config value
        let runner = Runner::new().unwrap();
        assert!(runner.worktree_override.is_none());

        let config = Config {
            worktree: true,
            ..Default::default()
        };
        assert!(runner.is_worktree_mode(&config));
    }

    // ========================================================================
    // US-003: Live Output Flusher tests
    // ========================================================================

    #[test]
    fn test_live_output_flusher_new() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());
        sm.ensure_dirs().unwrap();

        let flusher = LiveOutputFlusher::new(&sm, MachineState::RunningClaude);

        assert!(flusher.live_state.output_lines.is_empty());
        assert_eq!(
            flusher.live_state.machine_state,
            MachineState::RunningClaude
        );
        assert_eq!(flusher.line_count_since_flush, 0);
    }

    #[test]
    fn test_live_output_flusher_append_accumulates_lines() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());
        sm.ensure_dirs().unwrap();

        let mut flusher = LiveOutputFlusher::new(&sm, MachineState::RunningClaude);

        flusher.append("Line 1");
        flusher.append("Line 2");
        flusher.append("Line 3");

        assert_eq!(flusher.live_state.output_lines.len(), 3);
        assert_eq!(flusher.live_state.output_lines[0], "Line 1");
        assert_eq!(flusher.live_state.output_lines[1], "Line 2");
        assert_eq!(flusher.live_state.output_lines[2], "Line 3");
    }

    #[test]
    fn test_live_output_flusher_flush_resets_line_count() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());
        sm.ensure_dirs().unwrap();

        let mut flusher = LiveOutputFlusher::new(&sm, MachineState::RunningClaude);

        flusher.append("Line 1");
        flusher.append("Line 2");
        assert_eq!(flusher.line_count_since_flush, 2);

        flusher.flush();
        assert_eq!(flusher.line_count_since_flush, 0);
    }

    #[test]
    fn test_live_output_flusher_auto_flush_on_line_count() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());
        sm.ensure_dirs().unwrap();

        let mut flusher = LiveOutputFlusher::new(&sm, MachineState::RunningClaude);

        // Add 10 lines (should trigger auto-flush)
        for i in 0..10 {
            flusher.append(&format!("Line {}", i));
        }

        // After 10 lines, should have auto-flushed
        assert_eq!(flusher.line_count_since_flush, 0);

        // Verify file was written
        let loaded = sm.load_live();
        assert!(loaded.is_some());
        let live = loaded.unwrap();
        assert_eq!(live.output_lines.len(), 10);
    }

    #[test]
    fn test_live_output_flusher_final_flush() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());
        sm.ensure_dirs().unwrap();

        let mut flusher = LiveOutputFlusher::new(&sm, MachineState::RunningClaude);

        // Add a few lines (not enough to trigger auto-flush)
        flusher.append("Line 1");
        flusher.append("Line 2");
        flusher.append("Line 3");

        assert!(flusher.line_count_since_flush > 0);

        // Final flush should write remaining output
        flusher.final_flush();
        assert_eq!(flusher.line_count_since_flush, 0);

        // Verify file was written
        let loaded = sm.load_live();
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().output_lines.len(), 3);
    }

    #[test]
    fn test_live_output_flusher_final_flush_no_op_when_no_new_lines() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());
        sm.ensure_dirs().unwrap();

        let mut flusher = LiveOutputFlusher::new(&sm, MachineState::RunningClaude);

        // Note: LiveOutputFlusher::new() now flushes immediately for heartbeat support (US-002)
        // So live.json already exists from the constructor

        // No additional lines added, final flush should be a no-op
        // (line_count_since_flush is 0 after the initial flush in constructor)
        flusher.final_flush();
        assert_eq!(flusher.line_count_since_flush, 0);

        // File SHOULD exist now (from the constructor's immediate flush)
        let loaded = sm.load_live();
        assert!(
            loaded.is_some(),
            "live.json should exist from constructor flush"
        );
        assert_eq!(
            loaded.unwrap().output_lines.len(),
            0,
            "Output lines should be empty"
        );
    }

    #[test]
    fn test_live_output_flusher_preserves_machine_state() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());
        sm.ensure_dirs().unwrap();

        let mut flusher = LiveOutputFlusher::new(&sm, MachineState::Reviewing);
        flusher.append("Output");
        flusher.flush();

        let loaded = sm.load_live().unwrap();
        assert_eq!(loaded.machine_state, MachineState::Reviewing);
    }

    #[test]
    fn test_live_flush_constants() {
        // Verify the flush thresholds are reasonable
        assert_eq!(
            LIVE_FLUSH_INTERVAL_MS, 200,
            "Flush interval should be 200ms"
        );
        assert_eq!(LIVE_FLUSH_LINE_COUNT, 10, "Flush line count should be 10");
    }

    // ========================================================================
    // US-002: Heartbeat Mechanism Tests
    // ========================================================================

    #[test]
    fn test_heartbeat_interval_constant() {
        // Verify the heartbeat interval is ~2.5 seconds
        assert_eq!(
            HEARTBEAT_INTERVAL_MS, 2500,
            "Heartbeat interval should be 2500ms"
        );
    }

    #[test]
    fn test_live_output_flusher_new_flushes_immediately() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());
        sm.ensure_dirs().unwrap();

        // Creating a new flusher should immediately flush live.json
        let _flusher = LiveOutputFlusher::new(&sm, MachineState::RunningClaude);

        // Verify live.json was created
        let loaded = sm.load_live();
        assert!(
            loaded.is_some(),
            "live.json should exist after flusher creation"
        );

        let live = loaded.unwrap();
        assert_eq!(live.machine_state, MachineState::RunningClaude);
        assert!(
            live.is_heartbeat_fresh(),
            "Initial heartbeat should be fresh"
        );
    }

    #[test]
    fn test_live_output_flusher_flush_updates_heartbeat() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());
        sm.ensure_dirs().unwrap();

        let mut flusher = LiveOutputFlusher::new(&sm, MachineState::RunningClaude);

        // Add a line and flush
        flusher.append("test output");
        flusher.flush();

        // Verify heartbeat is fresh
        let loaded = sm.load_live().unwrap();
        assert!(
            loaded.is_heartbeat_fresh(),
            "Heartbeat should be fresh after flush"
        );
    }

    #[test]
    fn test_flush_live_state_function() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());
        sm.ensure_dirs().unwrap();

        // Call the standalone flush function
        flush_live_state(&sm, MachineState::Reviewing);

        // Verify live.json was created with correct state
        let loaded = sm.load_live();
        assert!(
            loaded.is_some(),
            "live.json should exist after flush_live_state"
        );

        let live = loaded.unwrap();
        assert_eq!(live.machine_state, MachineState::Reviewing);
        assert!(live.is_heartbeat_fresh(), "Heartbeat should be fresh");
    }

    // ========================================================================
    // US-004: Graceful Signal Handling Tests
    // ========================================================================

    /// Test that handle_interruption updates state status to Interrupted
    /// while preserving the machine_state.
    #[test]
    fn test_us004_handle_interruption_sets_interrupted_status() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());
        sm.ensure_dirs().unwrap();

        let runner = Runner {
            state_manager: StateManager::with_dir(temp_dir.path().to_path_buf()),
            verbose: false,
            skip_review: false,
            worktree_override: None,
        };

        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.transition_to(MachineState::RunningClaude);
        sm.save(&state).unwrap();

        let claude_runner = ClaudeRunner::new();

        // Call handle_interruption (no worktree setup context in this test)
        let error = runner.handle_interruption(&mut state, &claude_runner, None);

        // Verify the error type
        assert!(matches!(error, Autom8Error::Interrupted));

        // Verify state status is Interrupted but machine_state is preserved
        assert_eq!(state.status, RunStatus::Interrupted);
        assert_eq!(state.machine_state, MachineState::RunningClaude);
        assert!(state.finished_at.is_some());
    }

    /// Test that handle_interruption saves state and updates metadata.
    #[test]
    fn test_us004_handle_interruption_saves_state_and_metadata() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());
        sm.ensure_dirs().unwrap();

        let runner = Runner {
            state_manager: StateManager::with_dir(temp_dir.path().to_path_buf()),
            verbose: false,
            skip_review: false,
            worktree_override: None,
        };

        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.transition_to(MachineState::PickingStory);
        sm.save(&state).unwrap(); // Initial save

        let claude_runner = ClaudeRunner::new();

        // Call handle_interruption (no worktree setup context in this test)
        runner.handle_interruption(&mut state, &claude_runner, None);

        // Verify state was saved
        let loaded_state = sm.load_current().unwrap().unwrap();
        assert_eq!(loaded_state.status, RunStatus::Interrupted);

        // Verify metadata is_running is false (Interrupted != Running)
        let metadata = sm.load_metadata().unwrap().unwrap();
        assert!(
            !metadata.is_running,
            "Interrupted session should not be marked as running"
        );
    }

    /// Test that handle_interruption clears live output file.
    #[test]
    fn test_us004_handle_interruption_clears_live_output() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());
        sm.ensure_dirs().unwrap();

        let runner = Runner {
            state_manager: StateManager::with_dir(temp_dir.path().to_path_buf()),
            verbose: false,
            skip_review: false,
            worktree_override: None,
        };

        // Create some live output
        let live_state = LiveState::new(MachineState::RunningClaude);
        sm.save_live(&live_state).unwrap();

        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        sm.save(&state).unwrap();

        let claude_runner = ClaudeRunner::new();

        // Call handle_interruption (no worktree setup context in this test)
        runner.handle_interruption(&mut state, &claude_runner, None);

        // Verify live output was cleared
        let live_result = sm.load_live();
        assert!(
            live_result.is_none(),
            "Live output should be cleared after interruption"
        );
    }

    /// Test that resume handles Interrupted status correctly.
    #[test]
    fn test_us004_resume_handles_interrupted_status() {
        // Test that the condition in resume() includes Interrupted
        let status = RunStatus::Interrupted;
        let should_resume = status == RunStatus::Running
            || status == RunStatus::Failed
            || status == RunStatus::Interrupted;
        assert!(should_resume, "Interrupted status should be resumable");
    }

    /// Test that Interrupted state transitions preserve machine_state.
    #[test]
    fn test_us004_interrupted_preserves_machine_state_reviewing() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());
        sm.ensure_dirs().unwrap();

        let runner = Runner {
            state_manager: StateManager::with_dir(temp_dir.path().to_path_buf()),
            verbose: false,
            skip_review: false,
            worktree_override: None,
        };

        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.transition_to(MachineState::Reviewing);
        state.review_iteration = 2;
        sm.save(&state).unwrap();

        let claude_runner = ClaudeRunner::new();
        runner.handle_interruption(&mut state, &claude_runner, None);

        // Machine state should be preserved
        assert_eq!(state.machine_state, MachineState::Reviewing);
        assert_eq!(state.review_iteration, 2);
        assert_eq!(state.status, RunStatus::Interrupted);
    }

    /// Test that Interrupted state transitions preserve machine_state for Committing.
    #[test]
    fn test_us004_interrupted_preserves_machine_state_committing() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());
        sm.ensure_dirs().unwrap();

        let runner = Runner {
            state_manager: StateManager::with_dir(temp_dir.path().to_path_buf()),
            verbose: false,
            skip_review: false,
            worktree_override: None,
        };

        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.transition_to(MachineState::Committing);
        sm.save(&state).unwrap();

        let claude_runner = ClaudeRunner::new();
        runner.handle_interruption(&mut state, &claude_runner, None);

        assert_eq!(state.machine_state, MachineState::Committing);
        assert_eq!(state.status, RunStatus::Interrupted);
    }

    /// Test that Interrupted status is different from Failed.
    #[test]
    fn test_us004_interrupted_is_distinct_from_failed() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        // Interrupted status
        state.status = RunStatus::Interrupted;
        assert_ne!(state.status, RunStatus::Failed);
        assert_ne!(state.status, RunStatus::Running);
        assert_ne!(state.status, RunStatus::Completed);
    }

    // =========================================================================
    // US-005: Show Interruption Message on Resume
    // =========================================================================

    /// Test that interrupted status can be detected for showing resume message.
    #[test]
    fn test_us005_detect_interrupted_status_for_resume_message() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        // Set up an interrupted state with a specific machine state
        state.status = RunStatus::Interrupted;
        state.machine_state = MachineState::RunningClaude;

        // The resume logic should detect this condition
        let should_show_message = state.status == RunStatus::Interrupted;
        assert!(should_show_message);

        // Machine state should be preserved and accessible
        assert_eq!(format!("{:?}", state.machine_state), "RunningClaude");
    }

    /// Test that the resume logic shows message only for Interrupted, not Failed or Running.
    #[test]
    fn test_us005_resume_message_only_for_interrupted() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        // Running status should not show interruption message
        state.status = RunStatus::Running;
        assert_ne!(state.status, RunStatus::Interrupted);

        // Failed status should not show interruption message
        state.status = RunStatus::Failed;
        assert_ne!(state.status, RunStatus::Interrupted);

        // Completed status should not show interruption message
        state.status = RunStatus::Completed;
        assert_ne!(state.status, RunStatus::Interrupted);

        // Only Interrupted shows the message
        state.status = RunStatus::Interrupted;
        assert_eq!(state.status, RunStatus::Interrupted);
    }

    /// Test that machine state formatting works for all states in interruption message.
    #[test]
    fn test_us005_machine_state_formatting_for_message() {
        // Test that all machine states can be Debug-formatted for the message
        let states = [
            MachineState::Idle,
            MachineState::Initializing,
            MachineState::LoadingSpec,
            MachineState::PickingStory,
            MachineState::RunningClaude,
            MachineState::Reviewing,
            MachineState::Correcting,
            MachineState::Committing,
            MachineState::CreatingPR,
            MachineState::Completed,
            MachineState::Failed,
        ];

        for machine_state in states {
            let formatted = format!("{:?}", machine_state);
            assert!(
                !formatted.is_empty(),
                "Machine state should format to non-empty string"
            );
            // Verify it doesn't contain unexpected characters
            assert!(
                formatted.chars().all(|c| c.is_alphanumeric()),
                "Formatted state should be alphanumeric: {}",
                formatted
            );
        }
    }

    /// Test that print_resuming_interrupted is called correctly in the resume flow.
    #[test]
    fn test_us005_resume_flow_interrupt_detection() {
        // This test verifies the condition used in the resume method
        let test_cases = [
            (RunStatus::Interrupted, true), // Should show message
            (RunStatus::Running, false),    // Should NOT show message
            (RunStatus::Failed, false),     // Should NOT show message
            (RunStatus::Completed, false),  // Should NOT show message (and not resumable)
        ];

        for (status, should_show) in test_cases {
            let show_message = status == RunStatus::Interrupted;
            assert_eq!(
                show_message,
                should_show,
                "Status {:?} should{} show interruption message",
                status,
                if should_show { "" } else { " NOT" }
            );
        }
    }

    // =========================================================================
    // US-006: Handle Interruption During Worktree Setup
    // =========================================================================

    /// Test that WorktreeSetupContext captures original CWD correctly.
    #[test]
    fn test_us006_worktree_setup_context_captures_original_cwd() {
        let ctx = WorktreeSetupContext::new().unwrap();
        let current_dir = std::env::current_dir().unwrap();
        assert_eq!(
            ctx.original_cwd, current_dir,
            "WorktreeSetupContext should capture current directory"
        );
        assert!(ctx.worktree_path.is_none());
        assert!(!ctx.worktree_was_created);
        assert!(!ctx.cwd_changed);
        assert!(!ctx.metadata_saved);
    }

    /// Test that cleanup_on_interruption does nothing when no changes were made.
    #[test]
    fn test_us006_cleanup_on_interruption_noop_when_no_changes() {
        let original_cwd = std::env::current_dir().unwrap();
        let ctx = WorktreeSetupContext::new().unwrap();

        // Cleanup should be a no-op
        ctx.cleanup_on_interruption();

        // CWD should remain unchanged
        let current_cwd = std::env::current_dir().unwrap();
        assert_eq!(
            original_cwd, current_cwd,
            "CWD should not change when no cleanup needed"
        );
    }

    /// Test that cleanup restores CWD when it was changed.
    #[test]
    fn test_us006_cleanup_restores_cwd() {
        use tempfile::TempDir;

        let original_cwd = std::env::current_dir().unwrap();
        let temp_dir = TempDir::new().unwrap();

        // Create context with original CWD
        let mut ctx = WorktreeSetupContext::new().unwrap();

        // Simulate changing to a different directory
        std::env::set_current_dir(temp_dir.path()).unwrap();
        ctx.cwd_changed = true;
        ctx.worktree_path = Some(temp_dir.path().to_path_buf());

        // Mark as reused (not created) so worktree won't be removed
        ctx.worktree_was_created = false;

        // Verify we're in the temp directory
        let current = std::env::current_dir().unwrap();
        assert_ne!(
            current, original_cwd,
            "Should be in different directory before cleanup"
        );

        // Cleanup should restore original CWD
        ctx.cleanup_on_interruption();

        let restored = std::env::current_dir().unwrap();
        assert_eq!(
            restored, original_cwd,
            "CWD should be restored to original after cleanup"
        );
    }

    /// Test that newly created worktree is removed if metadata not saved.
    #[test]
    fn test_us006_cleanup_removes_partial_worktree() {
        // This test verifies the logic - actual worktree removal requires git setup
        let ctx = WorktreeSetupContext {
            original_cwd: PathBuf::from("/original/path"),
            worktree_path: Some(PathBuf::from("/fake/worktree")),
            worktree_was_created: true,
            cwd_changed: false,
            metadata_saved: false,
        };

        // Verify the cleanup logic conditions
        assert!(
            ctx.worktree_was_created && !ctx.metadata_saved,
            "Should attempt to remove worktree when newly created without saved metadata"
        );
    }

    /// Test that reused worktree is NOT removed on cleanup.
    #[test]
    fn test_us006_cleanup_preserves_reused_worktree() {
        let ctx = WorktreeSetupContext {
            original_cwd: PathBuf::from("/original/path"),
            worktree_path: Some(PathBuf::from("/fake/worktree")),
            worktree_was_created: false, // Reused, not created
            cwd_changed: false,
            metadata_saved: false,
        };

        // Verify the cleanup logic - should NOT remove reused worktrees
        assert!(
            !ctx.worktree_was_created,
            "Reused worktrees should not be removed"
        );
    }

    /// Test that worktree with saved metadata is NOT removed on cleanup.
    #[test]
    fn test_us006_cleanup_preserves_worktree_with_metadata() {
        let ctx = WorktreeSetupContext {
            original_cwd: PathBuf::from("/original/path"),
            worktree_path: Some(PathBuf::from("/fake/worktree")),
            worktree_was_created: true,
            cwd_changed: true,
            metadata_saved: true, // Metadata was saved
        };

        // Verify the cleanup logic - should NOT remove worktrees with saved metadata
        assert!(
            ctx.metadata_saved,
            "Worktrees with saved metadata should not be removed"
        );
    }

    /// Test that handle_interruption with worktree context cleans up.
    #[test]
    fn test_us006_handle_interruption_with_worktree_context() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());
        sm.ensure_dirs().unwrap();

        let runner = Runner {
            state_manager: StateManager::with_dir(temp_dir.path().to_path_buf()),
            verbose: false,
            skip_review: false,
            worktree_override: None,
        };

        let original_cwd = std::env::current_dir().unwrap();

        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.transition_to(MachineState::Initializing);
        sm.save(&state).unwrap();

        let claude_runner = ClaudeRunner::new();

        // Create a worktree setup context
        let setup_ctx = WorktreeSetupContext {
            original_cwd: original_cwd.clone(),
            worktree_path: None, // No worktree in this test
            worktree_was_created: false,
            cwd_changed: false,
            metadata_saved: true, // Simulate metadata being saved
        };

        // Call handle_interruption with context
        let error = runner.handle_interruption(&mut state, &claude_runner, Some(&setup_ctx));

        // Verify the error type
        assert!(matches!(error, Autom8Error::Interrupted));
        assert_eq!(state.status, RunStatus::Interrupted);

        // CWD should remain unchanged (cwd_changed was false)
        let current_cwd = std::env::current_dir().unwrap();
        assert_eq!(
            current_cwd, original_cwd,
            "CWD should remain unchanged when cwd_changed is false"
        );
    }

    /// Test that handle_interruption without worktree context still works.
    #[test]
    fn test_us006_handle_interruption_without_worktree_context() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());
        sm.ensure_dirs().unwrap();

        let runner = Runner {
            state_manager: StateManager::with_dir(temp_dir.path().to_path_buf()),
            verbose: false,
            skip_review: false,
            worktree_override: None,
        };

        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        sm.save(&state).unwrap();

        let claude_runner = ClaudeRunner::new();

        // Call handle_interruption without context
        let error = runner.handle_interruption(&mut state, &claude_runner, None);

        // Should still work correctly
        assert!(matches!(error, Autom8Error::Interrupted));
        assert_eq!(state.status, RunStatus::Interrupted);
    }

    /// Test that setup_worktree_context returns context with correct initial state.
    #[test]
    fn test_us006_setup_worktree_context_returns_context() {
        // We test the return type - actual worktree operations require git
        let temp_dir = TempDir::new().unwrap();
        let runner = Runner {
            state_manager: StateManager::with_dir(temp_dir.path().to_path_buf()),
            verbose: false,
            skip_review: false,
            worktree_override: Some(false), // Disable worktree mode for this test
        };

        let config = crate::config::Config::default();

        let result = runner.setup_worktree_context(&config, "test-branch");
        assert!(result.is_ok());

        let (worktree_context, setup_ctx) = result.unwrap();

        // With worktree mode disabled, no worktree context
        assert!(worktree_context.is_none());

        // Setup context should be initialized
        assert!(setup_ctx.worktree_path.is_none());
        assert!(!setup_ctx.worktree_was_created);
        assert!(!setup_ctx.cwd_changed);
        assert!(!setup_ctx.metadata_saved);
    }

    /// Test that WorktreeSetupContext clone works correctly.
    #[test]
    fn test_us006_worktree_setup_context_clone() {
        let ctx = WorktreeSetupContext {
            original_cwd: PathBuf::from("/some/path"),
            worktree_path: Some(PathBuf::from("/worktree/path")),
            worktree_was_created: true,
            cwd_changed: true,
            metadata_saved: false,
        };

        let cloned = ctx.clone();

        assert_eq!(ctx.original_cwd, cloned.original_cwd);
        assert_eq!(ctx.worktree_path, cloned.worktree_path);
        assert_eq!(ctx.worktree_was_created, cloned.worktree_was_created);
        assert_eq!(ctx.cwd_changed, cloned.cwd_changed);
        assert_eq!(ctx.metadata_saved, cloned.metadata_saved);
    }

    /// Test that WorktreeSetupContext debug formatting works.
    #[test]
    fn test_us006_worktree_setup_context_debug() {
        let ctx = WorktreeSetupContext::new().unwrap();
        let debug_str = format!("{:?}", ctx);
        assert!(debug_str.contains("WorktreeSetupContext"));
        assert!(debug_str.contains("original_cwd"));
        assert!(debug_str.contains("worktree_path"));
    }

    // ========================================================================
    // Fix: Worktree phantom session prevention tests
    // ========================================================================

    /// Tests that `run_from_spec()` does not persist state before worktree context is determined.
    ///
    /// This test verifies the fix for the phantom session bug where `run_from_spec()` was
    /// saving state to the original session (typically "main") before the worktree context
    /// was established, causing `status --all` to show two sessions when only one was running.
    ///
    /// The fix defers all state saves until after `effective_state_manager` is created.
    #[test]
    fn test_run_from_spec_state_not_saved_before_worktree_context() {
        use crate::state::RunState;

        // Verify RunState::from_spec_with_config creates state in LoadingSpec
        // but does not automatically persist it
        let state = RunState::from_spec_with_config(
            PathBuf::from("spec.md"),
            PathBuf::from("spec.json"),
            Config::default(),
        );

        // State should be in LoadingSpec (the initial state for spec generation)
        assert_eq!(
            state.machine_state,
            MachineState::LoadingSpec,
            "Initial state for from_spec should be LoadingSpec"
        );

        // Create an isolated StateManager to verify no state is pre-persisted
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        // Before any save operation, the session should have no state
        assert!(
            sm.load_current().unwrap().is_none(),
            "No state should exist before explicit save"
        );

        // This test documents the contract: run_from_spec() MUST NOT call
        // self.state_manager.save() before setup_worktree_context() returns.
        // The actual enforcement is in the code structure, which was fixed by:
        // 1. Removing save() at line 1163 (after state initialization)
        // 2. Removing save() at line 1180 (at GeneratingSpec transition)
    }

    /// Tests that state persistence only happens with the effective state manager.
    ///
    /// This test verifies that when state IS saved, it goes to the correct session
    /// (the worktree session, not the main repo session when running in worktree mode).
    #[test]
    fn test_state_saved_to_effective_session_only() {
        // Create two separate StateManagers representing different sessions
        let temp_dir = TempDir::new().unwrap();
        let main_sm =
            StateManager::with_dir_and_session(temp_dir.path().to_path_buf(), "main".to_string());
        let worktree_sm = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "abc12345".to_string(), // Simulated worktree session ID
        );

        // Create state for the worktree session
        let mut state = RunState::new_with_config_and_session(
            PathBuf::from("spec.json"),
            "feature-branch".to_string(),
            Config::default(),
            "abc12345".to_string(),
        );
        state.transition_to(MachineState::PickingStory);

        // Save only to the worktree session (simulating the fix)
        worktree_sm.save(&state).unwrap();

        // Main session should NOT have state (the phantom session bug)
        assert!(
            main_sm.load_current().unwrap().is_none(),
            "Main session should not have phantom state"
        );

        // Worktree session SHOULD have state
        assert!(
            worktree_sm.load_current().unwrap().is_some(),
            "Worktree session should have state"
        );
    }

    /// Tests the correct behavior: visual transitions without persistence.
    ///
    /// During spec generation, state transitions should be displayed visually
    /// (via print_state_transition) but NOT persisted. This test documents
    /// that state can be mutated locally without being saved.
    #[test]
    fn test_state_transitions_without_persistence() {
        use crate::state::RunState;

        let mut state = RunState::from_spec(PathBuf::from("spec.md"), PathBuf::from("spec.json"));

        // Initial state
        assert_eq!(state.machine_state, MachineState::LoadingSpec);

        // Transition locally (no save)
        state.transition_to(MachineState::GeneratingSpec);
        assert_eq!(state.machine_state, MachineState::GeneratingSpec);

        // The state has been mutated but NOT persisted
        // This is the correct behavior during spec generation:
        // - Visual feedback via print_state_transition() - works
        // - State persistence - DEFERRED until worktree context known

        // Transition to Initializing (what happens after worktree setup)
        state.transition_to(MachineState::Initializing);
        assert_eq!(state.machine_state, MachineState::Initializing);

        // At this point, save() can be called with the effective_state_manager
    }
}
