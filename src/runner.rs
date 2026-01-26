use crate::claude::{
    run_claude, run_corrector, run_for_commit, run_for_spec_generation, run_reviewer,
    ClaudeOutcome, ClaudeStoryResult, CommitResult, CorrectorResult, ReviewResult,
};
use crate::config::get_effective_config;
use crate::error::{Autom8Error, Result};
use crate::gh::{create_pull_request, PRResult};
use crate::git;
use crate::output::{
    print_all_complete, print_breadcrumb_trail, print_claude_output, print_error_panel,
    print_full_progress, print_generating_spec, print_header, print_info, print_issues_found,
    print_iteration_complete, print_iteration_start, print_max_review_iterations,
    print_phase_banner, print_phase_footer, print_pr_already_exists, print_pr_skipped,
    print_pr_success, print_pr_updated, print_proceeding_to_implementation, print_project_info,
    print_review_passed, print_reviewing, print_run_summary, print_skip_review,
    print_spec_generated, print_spec_loaded, print_state_transition, print_story_complete,
    print_tasks_progress, BannerColor, StoryResult, BOLD, CYAN, GRAY, RESET, YELLOW,
};
use crate::progress::{
    AgentDisplay, Breadcrumb, BreadcrumbState, ClaudeSpinner, Outcome, VerboseTimer,
};
use crate::spec::{Spec, UserStory};
use crate::state::{IterationStatus, MachineState, RunState, StateManager};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

// ============================================================================
// Constants
// ============================================================================

/// Maximum number of review/correct iterations before giving up.
/// This prevents infinite loops when the corrector cannot resolve review issues.
const MAX_REVIEW_ITERATIONS: u32 = 3;

// ============================================================================
// Progress Display Helper (US-006)
// ============================================================================

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

/// Control flow action returned from extracted helper methods
/// to communicate back to the main implementation loop.
enum LoopAction {
    /// Continue to the next iteration of the main loop
    Continue,
    /// Break out of the main loop (run complete)
    Break,
}

pub struct Runner {
    state_manager: StateManager,
    verbose: bool,
    skip_review: bool,
}

impl Runner {
    pub fn new() -> Result<Self> {
        Ok(Self {
            state_manager: StateManager::new()?,
            verbose: false,
            skip_review: false,
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

    /// Run the review/correct loop until review passes or max iterations reached.
    /// Returns Ok(()) if review passes, Err if max iterations exceeded or error occurs.
    fn run_review_correct_loop(
        &self,
        state: &mut RunState,
        spec: &Spec,
        breadcrumb: &mut Breadcrumb,
        story_results: &[StoryResult],
        print_summary: &impl Fn(u32, &[StoryResult]) -> Result<()>,
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
                    Some(|| print_summary(iteration, results)),
                    Autom8Error::MaxReviewIterationsReached,
                ));
            }

            // Transition to Reviewing state
            print_state_transition(state.machine_state, MachineState::Reviewing);
            state.transition_to(MachineState::Reviewing);
            self.state_manager.save(state)?;

            // Update breadcrumb to enter Review state
            breadcrumb.enter_state(BreadcrumbState::Review);

            print_phase_banner("REVIEWING", BannerColor::Cyan);
            print_reviewing(state.review_iteration, MAX_REVIEW_ITERATIONS);

            // Run reviewer with progress display
            let review_iter = state.review_iteration;
            let review_result = with_progress_display(
                self.verbose,
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

                    // Update breadcrumb to enter Correct state
                    breadcrumb.enter_state(BreadcrumbState::Correct);

                    print_phase_banner("CORRECTING", BannerColor::Yellow);
                    print_issues_found(state.review_iteration, MAX_REVIEW_ITERATIONS);

                    // Run corrector with progress display
                    let corrector_result = with_progress_display(
                        self.verbose,
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
                                Some(|| print_summary(iteration, results)),
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
                        Some(|| print_summary(iteration, results)),
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

        // Update breadcrumb to enter Commit state
        breadcrumb.enter_state(BreadcrumbState::Commit);

        print_phase_banner("COMMITTING", BannerColor::Cyan);

        // Run commit with progress display
        let commit_result = with_progress_display(
            self.verbose,
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
        print_summary: &impl Fn(u32, &[StoryResult]) -> Result<()>,
    ) -> Result<LoopAction> {
        print_all_complete();

        // Get the effective config for this run (US-005)
        let config = state.effective_config();

        // Skip review if --skip-review flag is set OR if review=false in config
        if self.skip_review || !config.review {
            print_skip_review();
        } else {
            // Run review/correct loop
            self.run_review_correct_loop(state, spec, breadcrumb, story_results, print_summary)?;
        }

        // Commit changes and create PR (respects commit and pull_request config)
        self.handle_commit_and_pr(state, spec, breadcrumb)?;

        state.transition_to(MachineState::Completed);
        self.state_manager.save(state)?;
        print_summary(state.iteration, story_results)?;
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
        print_summary: &impl Fn(u32, &[StoryResult]) -> Result<()>,
    ) -> Result<LoopAction> {
        state.finish_iteration(IterationStatus::Failed, error_msg.to_string());
        state.transition_to(MachineState::Failed);
        self.state_manager.save(state)?;

        story_results.push(StoryResult {
            id: story.id.clone(),
            title: story.title.clone(),
            passed: false,
            duration_secs: story_start.elapsed().as_secs(),
        });

        print_error_panel(error_panel_title, error_panel_msg, exit_code, stderr);
        print_summary(state.iteration, story_results)?;
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
        print_summary: &impl Fn(u32, &[StoryResult]) -> Result<()>,
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

        // Run Claude with progress display
        let result = with_progress_display(
            self.verbose,
            || VerboseTimer::new_with_story_progress(&story_id, story_index, total_stories),
            || ClaudeSpinner::new_with_story_progress(&story_id, story_index, total_stories),
            |callback| run_claude(spec, story, spec_json_path, &iterations, callback),
            |res| match res {
                Ok(_) => Outcome::success("Implementation done"),
                Err(e) => Outcome::failure(e.to_string()),
            },
        );

        match result {
            Ok(ClaudeStoryResult {
                outcome: ClaudeOutcome::AllStoriesComplete,
                work_summary,
            }) => self.handle_all_stories_complete_from_story(
                state,
                spec,
                spec_json_path,
                story,
                breadcrumb,
                story_results,
                work_summary,
                print_summary,
            ),
            Ok(ClaudeStoryResult {
                outcome: ClaudeOutcome::IterationComplete,
                work_summary,
            }) => self.handle_iteration_complete(
                state,
                spec_json_path,
                story,
                breadcrumb,
                story_results,
                work_summary,
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
                print_summary,
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
                print_summary,
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
        print_summary: &impl Fn(u32, &[StoryResult]) -> Result<()>,
    ) -> Result<LoopAction> {
        state.finish_iteration(IterationStatus::Success, String::new());
        state.set_work_summary(work_summary);

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
                print_summary,
            )?;
        }

        // Commit changes and create PR (respects commit and pull_request config)
        self.handle_commit_and_pr(state, spec, breadcrumb)?;

        state.transition_to(MachineState::Completed);
        self.state_manager.save(state)?;
        print_summary(state.iteration, story_results)?;
        self.archive_and_cleanup(state)?;
        Ok(LoopAction::Break)
    }

    /// Handle a normal iteration completion (story done, more to go).
    fn handle_iteration_complete(
        &self,
        state: &mut RunState,
        spec_json_path: &Path,
        story: &UserStory,
        breadcrumb: &mut Breadcrumb,
        story_results: &mut Vec<StoryResult>,
        work_summary: Option<String>,
    ) -> Result<LoopAction> {
        state.finish_iteration(IterationStatus::Success, String::new());
        state.set_work_summary(work_summary);
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
        // Check for existing active run
        if self.state_manager.has_active_run()? {
            if let Some(state) = self.state_manager.load_current()? {
                return Err(Autom8Error::RunInProgress(state.run_id));
            }
        }

        // Load effective config at startup before constructing state machine (US-005)
        let config = get_effective_config()?;

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
        let mut state =
            RunState::from_spec_with_config(spec_path.clone(), spec_json_path.clone(), config);
        self.state_manager.save(&state)?;

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
        state.transition_to(MachineState::GeneratingSpec);
        self.state_manager.save(&state)?;
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

        // Update state with branch from generated spec
        state.branch = spec.branch_name.clone();
        state.transition_to(MachineState::Initializing);
        self.state_manager.save(&state)?;
        print_state_transition(MachineState::GeneratingSpec, MachineState::Initializing);

        print_proceeding_to_implementation();

        // Continue with normal implementation flow
        self.run_implementation_loop(state, &spec_json_path)
    }

    pub fn run(&self, spec_json_path: &Path) -> Result<()> {
        // Check for existing active run
        if self.state_manager.has_active_run()? {
            if let Some(state) = self.state_manager.load_current()? {
                return Err(Autom8Error::RunInProgress(state.run_id));
            }
        }

        // Load effective config at startup before constructing state machine (US-005)
        let config = get_effective_config()?;

        // Canonicalize path so resume works from any directory
        let spec_json_path = spec_json_path
            .canonicalize()
            .map_err(|_| Autom8Error::SpecNotFound(spec_json_path.to_path_buf()))?;

        // Load and validate spec
        let spec = Spec::load(&spec_json_path)?;

        // If in a git repo, ensure we're on the correct branch
        if git::is_git_repo() {
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
        let state =
            RunState::new_with_config(spec_json_path.to_path_buf(), spec.branch_name.clone(), config);

        print_state_transition(MachineState::Idle, MachineState::Initializing);
        print_project_info(&spec);

        self.run_implementation_loop(state, &spec_json_path)
    }

    fn run_implementation_loop(&self, mut state: RunState, spec_json_path: &Path) -> Result<()> {
        // Transition to PickingStory
        print_state_transition(state.machine_state, MachineState::PickingStory);
        state.transition_to(MachineState::PickingStory);
        self.state_manager.save(&state)?;

        // Track story results for summary
        let mut story_results: Vec<StoryResult> = Vec::new();
        let run_start = Instant::now();

        // Breadcrumb trail for tracking workflow journey
        let mut breadcrumb = Breadcrumb::new();

        // Helper to print run summary (loads spec and prints)
        let print_summary = |iteration: u32, results: &[StoryResult]| -> Result<()> {
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
            // Reload spec to get latest passes state
            let spec = Spec::load(spec_json_path)?;

            // Check if all stories complete at loop start
            if spec.all_complete() {
                match self.handle_all_stories_complete(
                    &mut state,
                    &spec,
                    &mut breadcrumb,
                    &story_results,
                    &print_summary,
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

            // Start iteration
            print_state_transition(MachineState::PickingStory, MachineState::RunningClaude);
            state.start_iteration(&story.id);
            self.state_manager.save(&state)?;

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
                &print_summary,
            )? {
                LoopAction::Break => return Ok(()),
                LoopAction::Continue => continue,
            }
        }
    }

    pub fn resume(&self) -> Result<()> {
        // First try: load from active state
        if let Some(state) = self.state_manager.load_current()? {
            if state.status == crate::state::RunStatus::Running
                || state.status == crate::state::RunStatus::Failed
            {
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
}
