use crate::claude::{
    run_claude, run_corrector, run_for_commit, run_for_prd_generation, run_reviewer, ClaudeOutcome,
    ClaudeStoryResult, CommitResult, CorrectorResult, ReviewResult,
};
use crate::error::{Autom8Error, Result};
use crate::git;
use crate::output::{
    print_all_complete, print_breadcrumb_trail, print_claude_output, print_error_panel,
    print_full_progress, print_generating_prd, print_header, print_info, print_issues_found,
    print_iteration_complete, print_iteration_start, print_max_review_iterations,
    print_phase_banner, print_phase_footer, print_prd_generated, print_proceeding_to_implementation,
    print_project_info, print_review_passed, print_reviewing, print_run_summary, print_skip_review,
    print_spec_loaded, print_state_transition, print_story_complete, print_tasks_progress,
    BannerColor, StoryResult, BOLD, CYAN, GRAY, RESET, YELLOW,
};
use crate::prd::Prd;
use crate::progress::{AgentDisplay, Breadcrumb, BreadcrumbState, ClaudeSpinner, Outcome, VerboseTimer};
use crate::state::{IterationStatus, MachineState, RunState, StateManager};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub struct Runner {
    state_manager: StateManager,
    verbose: bool,
    skip_review: bool,
}

impl Runner {
    pub fn new() -> Self {
        Self {
            state_manager: StateManager::new(),
            verbose: false,
            skip_review: false,
        }
    }

    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    pub fn with_skip_review(mut self, skip_review: bool) -> Self {
        self.skip_review = skip_review;
        self
    }

    /// Run from a prd.md spec file - converts to JSON first, then implements
    pub fn run_from_spec(&self, spec_path: &Path) -> Result<()> {
        // Check for existing active run
        if self.state_manager.has_active_run()? {
            if let Some(state) = self.state_manager.load_current()? {
                return Err(Autom8Error::RunInProgress(state.run_id));
            }
        }

        // Canonicalize spec path
        let spec_path = spec_path
            .canonicalize()
            .map_err(|_| Autom8Error::SpecNotFound(spec_path.to_path_buf()))?;

        // Determine PRD output path in .autom8/prds/ directory
        let stem = spec_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("prd");
        let prds_dir = self.state_manager.ensure_prds_dir()?;
        let prd_path = prds_dir.join(format!("{}.json", stem));

        // Initialize state
        let mut state = RunState::from_spec(spec_path.clone(), prd_path.clone());
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

        // Transition to GeneratingPrd
        state.transition_to(MachineState::GeneratingPrd);
        self.state_manager.save(&state)?;
        print_state_transition(MachineState::LoadingSpec, MachineState::GeneratingPrd);

        print_generating_prd();

        // Run Claude to generate PRD
        let verbose = self.verbose;
        let prd = if verbose {
            let mut timer = VerboseTimer::new_for_prd();
            let result = run_for_prd_generation(&spec_content, &prd_path, |line| {
                print_claude_output(line);
            });
            match &result {
                Ok(_) => timer.finish_success(),
                Err(e) => timer.finish_error(&e.to_string()),
            }
            match result {
                Ok(prd) => prd,
                Err(e) => {
                    print_error_panel(
                        "PRD Generation Failed",
                        &e.to_string(),
                        None,
                        None,
                    );
                    return Err(e);
                }
            }
        } else {
            let prd_start = Instant::now();
            let mut spinner = ClaudeSpinner::new_for_prd();
            let result = run_for_prd_generation(&spec_content, &prd_path, |line| {
                spinner.update(line);
            });
            match &result {
                Ok(_) => spinner.finish_success(prd_start.elapsed().as_secs()),
                Err(e) => spinner.finish_error(&e.to_string()),
            }
            match result {
                Ok(prd) => prd,
                Err(e) => {
                    print_error_panel(
                        "PRD Generation Failed",
                        &e.to_string(),
                        None,
                        None,
                    );
                    return Err(e);
                }
            }
        };

        print_prd_generated(&prd, &prd_path);

        // Update state with branch from generated PRD
        state.branch = prd.branch_name.clone();
        state.transition_to(MachineState::Initializing);
        self.state_manager.save(&state)?;
        print_state_transition(MachineState::GeneratingPrd, MachineState::Initializing);

        print_proceeding_to_implementation();

        // Continue with normal implementation flow
        self.run_implementation_loop(state, &prd_path)
    }

    pub fn run(&self, prd_path: &Path) -> Result<()> {
        // Check for existing active run
        if self.state_manager.has_active_run()? {
            if let Some(state) = self.state_manager.load_current()? {
                return Err(Autom8Error::RunInProgress(state.run_id));
            }
        }

        // Canonicalize path so resume works from any directory
        let prd_path = prd_path
            .canonicalize()
            .map_err(|_| Autom8Error::PrdNotFound(prd_path.to_path_buf()))?;

        // Load and validate PRD
        let prd = Prd::load(&prd_path)?;

        // If in a git repo, ensure we're on the correct branch
        if git::is_git_repo() {
            let current_branch = git::current_branch()?;
            if current_branch != prd.branch_name {
                print_info(&format!(
                    "Switching from '{}' to '{}'",
                    current_branch, prd.branch_name
                ));
                git::ensure_branch(&prd.branch_name)?;
            }
        }

        // Initialize state
        let state = RunState::new(prd_path.to_path_buf(), prd.branch_name.clone());

        print_state_transition(MachineState::Idle, MachineState::Initializing);
        print_project_info(&prd);

        self.run_implementation_loop(state, &prd_path)
    }

    fn run_implementation_loop(&self, mut state: RunState, prd_path: &Path) -> Result<()> {
        // Transition to PickingStory
        print_state_transition(state.machine_state, MachineState::PickingStory);
        state.transition_to(MachineState::PickingStory);
        self.state_manager.save(&state)?;

        // Track story results for summary
        let mut story_results: Vec<StoryResult> = Vec::new();
        let run_start = Instant::now();

        // Breadcrumb trail for tracking workflow journey
        let mut breadcrumb = Breadcrumb::new();

        // Helper to print run summary (loads PRD and prints)
        let print_summary = |iteration: u32, results: &[StoryResult]| -> Result<()> {
            let prd = Prd::load(prd_path)?;
            print_run_summary(
                prd.total_count(),
                prd.completed_count(),
                iteration,
                run_start.elapsed().as_secs(),
                results,
            );
            Ok(())
        };

        // Main loop
        loop {
            // Reload PRD to get latest passes state
            let prd = Prd::load(prd_path)?;

            // Check if all stories complete
            if prd.all_complete() {
                print_all_complete();

                // Skip review if --skip-review flag is set
                if self.skip_review {
                    print_skip_review();
                } else {
                    // Run review loop before committing
                    const MAX_REVIEW_ITERATIONS: u32 = 3;
                    state.review_iteration = 1;

                    loop {
                        // Check if we've exceeded max review iterations
                        if state.review_iteration > MAX_REVIEW_ITERATIONS {
                            state.transition_to(MachineState::Failed);
                            self.state_manager.save(&state)?;
                            print_max_review_iterations();
                            print_summary(state.iteration, &story_results)?;
                            return Err(Autom8Error::MaxReviewIterationsReached);
                        }

                        // Transition to Reviewing state
                        print_state_transition(state.machine_state, MachineState::Reviewing);
                        state.transition_to(MachineState::Reviewing);
                        self.state_manager.save(&state)?;

                        // Update breadcrumb to enter Review state
                        breadcrumb.enter_state(BreadcrumbState::Review);

                        print_phase_banner("REVIEWING", BannerColor::Cyan);
                        print_reviewing(state.review_iteration, MAX_REVIEW_ITERATIONS);

                        // Run reviewer
                        let verbose = self.verbose;
                        let review_result = if verbose {
                            let mut timer = VerboseTimer::new_for_review(
                                state.review_iteration,
                                MAX_REVIEW_ITERATIONS,
                            );
                            let res = run_reviewer(
                                &prd,
                                state.review_iteration,
                                MAX_REVIEW_ITERATIONS,
                                |line| {
                                    print_claude_output(line);
                                },
                            );
                            match &res {
                                Ok(ReviewResult::Pass) => {
                                    timer.finish_with_outcome(Outcome::success("No issues found"))
                                }
                                Ok(ReviewResult::IssuesFound) => {
                                    timer.finish_with_outcome(Outcome::success("Issues found"))
                                }
                                Ok(ReviewResult::Error(e)) => {
                                    timer.finish_with_outcome(Outcome::failure(e.clone()))
                                }
                                Err(e) => {
                                    timer.finish_with_outcome(Outcome::failure(e.to_string()))
                                }
                            }
                            res?
                        } else {
                            let mut spinner = ClaudeSpinner::new_for_review(
                                state.review_iteration,
                                MAX_REVIEW_ITERATIONS,
                            );
                            let res = run_reviewer(
                                &prd,
                                state.review_iteration,
                                MAX_REVIEW_ITERATIONS,
                                |line| {
                                    spinner.update(line);
                                },
                            );
                            match &res {
                                Ok(ReviewResult::Pass) => {
                                    spinner.finish_with_outcome(Outcome::success("No issues found"))
                                }
                                Ok(ReviewResult::IssuesFound) => {
                                    spinner.finish_with_outcome(Outcome::success("Issues found"))
                                }
                                Ok(ReviewResult::Error(e)) => {
                                    spinner.finish_with_outcome(Outcome::failure(e.clone()))
                                }
                                Err(e) => {
                                    spinner.finish_with_outcome(Outcome::failure(e.to_string()))
                                }
                            }
                            res?
                        };

                        // Print bottom border to close the output frame
                        print_phase_footer(BannerColor::Cyan);

                        // Print breadcrumb trail after review phase completion
                        print_breadcrumb_trail(&breadcrumb);

                        // Show progress bar after review task completion
                        print_full_progress(
                            prd.completed_count(),
                            prd.total_count(),
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
                                break; // Exit review loop, proceed to commit
                            }
                            ReviewResult::IssuesFound => {
                                // Transition to Correcting state
                                print_state_transition(
                                    MachineState::Reviewing,
                                    MachineState::Correcting,
                                );
                                state.transition_to(MachineState::Correcting);
                                self.state_manager.save(&state)?;

                                // Update breadcrumb to enter Correct state
                                breadcrumb.enter_state(BreadcrumbState::Correct);

                                print_phase_banner("CORRECTING", BannerColor::Yellow);
                                print_issues_found(state.review_iteration, MAX_REVIEW_ITERATIONS);

                                // Run corrector
                                let corrector_result = if verbose {
                                    let mut timer = VerboseTimer::new_for_correct(
                                        state.review_iteration,
                                        MAX_REVIEW_ITERATIONS,
                                    );
                                    let res = run_corrector(&prd, state.review_iteration, |line| {
                                        print_claude_output(line);
                                    });
                                    match &res {
                                        Ok(CorrectorResult::Complete) => {
                                            timer.finish_with_outcome(Outcome::success("Issues addressed"))
                                        }
                                        Ok(CorrectorResult::Error(e)) => {
                                            timer.finish_with_outcome(Outcome::failure(e.clone()))
                                        }
                                        Err(e) => {
                                            timer.finish_with_outcome(Outcome::failure(e.to_string()))
                                        }
                                    }
                                    res?
                                } else {
                                    let mut spinner = ClaudeSpinner::new_for_correct(
                                        state.review_iteration,
                                        MAX_REVIEW_ITERATIONS,
                                    );
                                    let res = run_corrector(&prd, state.review_iteration, |line| {
                                        spinner.update(line);
                                    });
                                    match &res {
                                        Ok(CorrectorResult::Complete) => {
                                            spinner.finish_with_outcome(Outcome::success("Issues addressed"))
                                        }
                                        Ok(CorrectorResult::Error(e)) => {
                                            spinner.finish_with_outcome(Outcome::failure(e.clone()))
                                        }
                                        Err(e) => {
                                            spinner.finish_with_outcome(Outcome::failure(e.to_string()))
                                        }
                                    }
                                    res?
                                };

                                // Print bottom border to close the output frame
                                print_phase_footer(BannerColor::Yellow);

                                // Print breadcrumb trail after correct phase completion
                                print_breadcrumb_trail(&breadcrumb);

                                // Show progress bar after correct task completion
                                print_full_progress(
                                    prd.completed_count(),
                                    prd.total_count(),
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
                                        state.transition_to(MachineState::Failed);
                                        self.state_manager.save(&state)?;
                                        print_error_panel(
                                            "Corrector Failed",
                                            &e.message,
                                            e.exit_code,
                                            e.stderr.as_deref(),
                                        );
                                        print_summary(state.iteration, &story_results)?;
                                        return Err(Autom8Error::ClaudeError(format!(
                                            "Corrector failed: {}",
                                            e
                                        )));
                                    }
                                }
                            }
                            ReviewResult::Error(e) => {
                                state.transition_to(MachineState::Failed);
                                self.state_manager.save(&state)?;
                                print_error_panel(
                                    "Review Failed",
                                    &e.message,
                                    e.exit_code,
                                    e.stderr.as_deref(),
                                );
                                print_summary(state.iteration, &story_results)?;
                                return Err(Autom8Error::ClaudeError(format!(
                                    "Review failed: {}",
                                    e
                                )));
                            }
                        }
                    }
                }

                // Commit changes if in git repo
                if git::is_git_repo() {
                    print_state_transition(state.machine_state, MachineState::Committing);
                    state.transition_to(MachineState::Committing);
                    self.state_manager.save(&state)?;

                    // Update breadcrumb to enter Commit state
                    breadcrumb.enter_state(BreadcrumbState::Commit);

                    print_phase_banner("COMMITTING", BannerColor::Cyan);

                    let verbose = self.verbose;
                    let commit_result = if verbose {
                        let mut timer = VerboseTimer::new_for_commit();
                        let res = run_for_commit(&prd, |line| {
                            print_claude_output(line);
                        });
                        match &res {
                            Ok(CommitResult::Success(hash)) => {
                                timer.finish_with_outcome(Outcome::success(hash.clone()))
                            }
                            Ok(CommitResult::NothingToCommit) => {
                                timer.finish_with_outcome(Outcome::success("Nothing to commit"))
                            }
                            Ok(CommitResult::Error(e)) => {
                                timer.finish_with_outcome(Outcome::failure(e.clone()))
                            }
                            Err(e) => {
                                timer.finish_with_outcome(Outcome::failure(e.to_string()))
                            }
                        }
                        res?
                    } else {
                        let mut spinner = ClaudeSpinner::new_for_commit();
                        let res = run_for_commit(&prd, |line| {
                            spinner.update(line);
                        });
                        match &res {
                            Ok(CommitResult::Success(hash)) => {
                                spinner.finish_with_outcome(Outcome::success(hash.clone()))
                            }
                            Ok(CommitResult::NothingToCommit) => {
                                spinner.finish_with_outcome(Outcome::success("Nothing to commit"))
                            }
                            Ok(CommitResult::Error(e)) => {
                                spinner.finish_with_outcome(Outcome::failure(e.clone()))
                            }
                            Err(e) => {
                                spinner.finish_with_outcome(Outcome::failure(e.to_string()))
                            }
                        }
                        res?
                    };

                    // Print bottom border to close the output frame
                    print_phase_footer(BannerColor::Cyan);

                    // Print breadcrumb trail after commit phase completion
                    print_breadcrumb_trail(&breadcrumb);

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

                    print_state_transition(MachineState::Committing, MachineState::Completed);
                } else {
                    print_state_transition(state.machine_state, MachineState::Completed);
                }

                state.transition_to(MachineState::Completed);
                self.state_manager.save(&state)?;
                print_summary(state.iteration, &story_results)?;
                self.archive_and_cleanup(&state)?;
                return Ok(());
            }

            // Pick next story
            let story = prd
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

            // Run Claude with spinner or verbose output
            let story_start = Instant::now();
            let verbose = self.verbose;
            // Calculate story progress for display: [US-001 2/5]
            let story_index = prd
                .user_stories
                .iter()
                .position(|s| s.id == story.id)
                .map(|i| i as u32 + 1)
                .unwrap_or(state.iteration);
            let total_stories = prd.total_count() as u32;
            let result = if verbose {
                let mut timer =
                    VerboseTimer::new_with_story_progress(&story.id, story_index, total_stories);
                let res = run_claude(&prd, &story, prd_path, &state.iterations, |line| {
                    print_claude_output(line);
                });
                match &res {
                    Ok(_) => timer.finish_with_outcome(Outcome::success("Implementation done")),
                    Err(e) => timer.finish_with_outcome(Outcome::failure(e.to_string())),
                }
                res
            } else {
                let mut spinner =
                    ClaudeSpinner::new_with_story_progress(&story.id, story_index, total_stories);
                let res = run_claude(&prd, &story, prd_path, &state.iterations, |line| {
                    spinner.update(line);
                });
                match &res {
                    Ok(_) => spinner.finish_with_outcome(Outcome::success("Implementation done")),
                    Err(e) => spinner.finish_with_outcome(Outcome::failure(e.to_string())),
                }
                res
            };

            match result {
                Ok(ClaudeStoryResult {
                    outcome: ClaudeOutcome::AllStoriesComplete,
                    work_summary,
                }) => {
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
                    print_breadcrumb_trail(&breadcrumb);

                    // Show progress bar after story task completion
                    // Reload PRD to verify actual completion state
                    let updated_prd = Prd::load(prd_path)?;
                    print_tasks_progress(updated_prd.completed_count(), updated_prd.total_count());
                    println!();

                    if verbose {
                        print_story_complete(&story.id, duration);
                    }

                    // Validate that all stories are actually complete
                    // Claude may output COMPLETE prematurely before updating the PRD
                    if !updated_prd.all_complete() {
                        // PRD doesn't match Claude's claim - continue processing stories
                        continue;
                    }

                    print_all_complete();

                    // Skip review if --skip-review flag is set
                    if self.skip_review {
                        print_skip_review();
                    } else {
                        // Run review loop before committing
                        const MAX_REVIEW_ITERATIONS: u32 = 3;
                        state.review_iteration = 1;

                        loop {
                            // Check if we've exceeded max review iterations
                            if state.review_iteration > MAX_REVIEW_ITERATIONS {
                                state.transition_to(MachineState::Failed);
                                self.state_manager.save(&state)?;
                                print_max_review_iterations();
                                print_summary(state.iteration, &story_results)?;
                                return Err(Autom8Error::MaxReviewIterationsReached);
                            }

                            // Transition to Reviewing state
                            print_state_transition(state.machine_state, MachineState::Reviewing);
                            state.transition_to(MachineState::Reviewing);
                            self.state_manager.save(&state)?;

                            // Update breadcrumb to enter Review state
                            breadcrumb.enter_state(BreadcrumbState::Review);

                            print_phase_banner("REVIEWING", BannerColor::Cyan);
                            print_reviewing(state.review_iteration, MAX_REVIEW_ITERATIONS);

                            // Run reviewer
                            let review_result = if verbose {
                                let mut timer = VerboseTimer::new_for_review(
                                    state.review_iteration,
                                    MAX_REVIEW_ITERATIONS,
                                );
                                let res = run_reviewer(
                                    &prd,
                                    state.review_iteration,
                                    MAX_REVIEW_ITERATIONS,
                                    |line| {
                                        print_claude_output(line);
                                    },
                                );
                                match &res {
                                    Ok(ReviewResult::Pass) => {
                                        timer.finish_with_outcome(Outcome::success("No issues found"))
                                    }
                                    Ok(ReviewResult::IssuesFound) => {
                                        timer.finish_with_outcome(Outcome::success("Issues found"))
                                    }
                                    Ok(ReviewResult::Error(e)) => {
                                        timer.finish_with_outcome(Outcome::failure(e.clone()))
                                    }
                                    Err(e) => {
                                        timer.finish_with_outcome(Outcome::failure(e.to_string()))
                                    }
                                }
                                res?
                            } else {
                                let mut spinner = ClaudeSpinner::new_for_review(
                                    state.review_iteration,
                                    MAX_REVIEW_ITERATIONS,
                                );
                                let res = run_reviewer(
                                    &prd,
                                    state.review_iteration,
                                    MAX_REVIEW_ITERATIONS,
                                    |line| {
                                        spinner.update(line);
                                    },
                                );
                                match &res {
                                    Ok(ReviewResult::Pass) => {
                                        spinner.finish_with_outcome(Outcome::success("No issues found"))
                                    }
                                    Ok(ReviewResult::IssuesFound) => {
                                        spinner.finish_with_outcome(Outcome::success("Issues found"))
                                    }
                                    Ok(ReviewResult::Error(e)) => {
                                        spinner.finish_with_outcome(Outcome::failure(e.clone()))
                                    }
                                    Err(e) => {
                                        spinner.finish_with_outcome(Outcome::failure(e.to_string()))
                                    }
                                }
                                res?
                            };

                            // Print bottom border to close the output frame
                            print_phase_footer(BannerColor::Cyan);

                            // Print breadcrumb trail after review phase completion
                            print_breadcrumb_trail(&breadcrumb);

                            // Show progress bar after review task completion
                            // Use updated_prd which was loaded earlier in this block
                            print_full_progress(
                                updated_prd.completed_count(),
                                updated_prd.total_count(),
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
                                    break; // Exit review loop, proceed to commit
                                }
                                ReviewResult::IssuesFound => {
                                    // Transition to Correcting state
                                    print_state_transition(
                                        MachineState::Reviewing,
                                        MachineState::Correcting,
                                    );
                                    state.transition_to(MachineState::Correcting);
                                    self.state_manager.save(&state)?;

                                    // Update breadcrumb to enter Correct state
                                    breadcrumb.enter_state(BreadcrumbState::Correct);

                                    print_phase_banner("CORRECTING", BannerColor::Yellow);
                                    print_issues_found(
                                        state.review_iteration,
                                        MAX_REVIEW_ITERATIONS,
                                    );

                                    // Run corrector
                                    let corrector_result = if verbose {
                                        let mut timer = VerboseTimer::new_for_correct(
                                            state.review_iteration,
                                            MAX_REVIEW_ITERATIONS,
                                        );
                                        let res =
                                            run_corrector(&prd, state.review_iteration, |line| {
                                                print_claude_output(line);
                                            });
                                        match &res {
                                            Ok(CorrectorResult::Complete) => {
                                                timer.finish_with_outcome(Outcome::success("Issues addressed"))
                                            }
                                            Ok(CorrectorResult::Error(e)) => {
                                                timer.finish_with_outcome(Outcome::failure(e.clone()))
                                            }
                                            Err(e) => {
                                                timer.finish_with_outcome(Outcome::failure(e.to_string()))
                                            }
                                        }
                                        res?
                                    } else {
                                        let mut spinner = ClaudeSpinner::new_for_correct(
                                            state.review_iteration,
                                            MAX_REVIEW_ITERATIONS,
                                        );
                                        let res =
                                            run_corrector(&prd, state.review_iteration, |line| {
                                                spinner.update(line);
                                            });
                                        match &res {
                                            Ok(CorrectorResult::Complete) => {
                                                spinner.finish_with_outcome(Outcome::success("Issues addressed"))
                                            }
                                            Ok(CorrectorResult::Error(e)) => {
                                                spinner.finish_with_outcome(Outcome::failure(e.clone()))
                                            }
                                            Err(e) => {
                                                spinner.finish_with_outcome(Outcome::failure(e.to_string()))
                                            }
                                        }
                                        res?
                                    };

                                    // Print bottom border to close the output frame
                                    print_phase_footer(BannerColor::Yellow);

                                    // Print breadcrumb trail after correct phase completion
                                    print_breadcrumb_trail(&breadcrumb);

                                    // Show progress bar after correct task completion
                                    print_full_progress(
                                        updated_prd.completed_count(),
                                        updated_prd.total_count(),
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
                                            state.transition_to(MachineState::Failed);
                                            self.state_manager.save(&state)?;
                                            print_error_panel(
                                                "Corrector Failed",
                                                &e.message,
                                                e.exit_code,
                                                e.stderr.as_deref(),
                                            );
                                            print_summary(state.iteration, &story_results)?;
                                            return Err(Autom8Error::ClaudeError(format!(
                                                "Corrector failed: {}",
                                                e
                                            )));
                                        }
                                    }
                                }
                                ReviewResult::Error(e) => {
                                    state.transition_to(MachineState::Failed);
                                    self.state_manager.save(&state)?;
                                    print_error_panel(
                                        "Review Failed",
                                        &e.message,
                                        e.exit_code,
                                        e.stderr.as_deref(),
                                    );
                                    print_summary(state.iteration, &story_results)?;
                                    return Err(Autom8Error::ClaudeError(format!(
                                        "Review failed: {}",
                                        e
                                    )));
                                }
                            }
                        }
                    }

                    // Commit changes if in git repo
                    if git::is_git_repo() {
                        print_state_transition(state.machine_state, MachineState::Committing);
                        state.transition_to(MachineState::Committing);
                        self.state_manager.save(&state)?;

                        // Update breadcrumb to enter Commit state
                        breadcrumb.enter_state(BreadcrumbState::Commit);

                        print_phase_banner("COMMITTING", BannerColor::Cyan);

                        let commit_result = if verbose {
                            let mut timer = VerboseTimer::new_for_commit();
                            let res = run_for_commit(&prd, |line| {
                                print_claude_output(line);
                            });
                            match &res {
                                Ok(CommitResult::Success(hash)) => {
                                    timer.finish_with_outcome(Outcome::success(hash.clone()))
                                }
                                Ok(CommitResult::NothingToCommit) => {
                                    timer.finish_with_outcome(Outcome::success("Nothing to commit"))
                                }
                                Ok(CommitResult::Error(e)) => {
                                    timer.finish_with_outcome(Outcome::failure(e.clone()))
                                }
                                Err(e) => {
                                    timer.finish_with_outcome(Outcome::failure(e.to_string()))
                                }
                            }
                            res?
                        } else {
                            let mut spinner = ClaudeSpinner::new_for_commit();
                            let res = run_for_commit(&prd, |line| {
                                spinner.update(line);
                            });
                            match &res {
                                Ok(CommitResult::Success(hash)) => {
                                    spinner.finish_with_outcome(Outcome::success(hash.clone()))
                                }
                                Ok(CommitResult::NothingToCommit) => {
                                    spinner.finish_with_outcome(Outcome::success("Nothing to commit"))
                                }
                                Ok(CommitResult::Error(e)) => {
                                    spinner.finish_with_outcome(Outcome::failure(e.clone()))
                                }
                                Err(e) => {
                                    spinner.finish_with_outcome(Outcome::failure(e.to_string()))
                                }
                            }
                            res?
                        };

                        // Print bottom border to close the output frame
                        print_phase_footer(BannerColor::Cyan);

                        // Print breadcrumb trail after commit phase completion
                        print_breadcrumb_trail(&breadcrumb);

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

                        print_state_transition(MachineState::Committing, MachineState::Completed);
                    } else {
                        print_state_transition(state.machine_state, MachineState::Completed);
                    }

                    state.transition_to(MachineState::Completed);
                    self.state_manager.save(&state)?;
                    print_summary(state.iteration, &story_results)?;
                    self.archive_and_cleanup(&state)?;
                    return Ok(());
                }
                Ok(ClaudeStoryResult {
                    outcome: ClaudeOutcome::IterationComplete,
                    work_summary,
                }) => {
                    state.finish_iteration(IterationStatus::Success, String::new());
                    state.set_work_summary(work_summary);
                    self.state_manager.save(&state)?;

                    let duration = state.current_iteration_duration();

                    // Print bottom border to close the output frame
                    print_phase_footer(BannerColor::Cyan);

                    // Print breadcrumb trail after story phase completion
                    print_breadcrumb_trail(&breadcrumb);

                    print_state_transition(MachineState::RunningClaude, MachineState::PickingStory);
                    print_iteration_complete(state.iteration);

                    // Reload PRD and check if current story passed
                    let updated_prd = Prd::load(prd_path)?;
                    let story_passed = updated_prd
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
                        if verbose {
                            print_story_complete(&story.id, duration);
                        }
                    }

                    // Show progress bar after story task completion
                    print_tasks_progress(updated_prd.completed_count(), updated_prd.total_count());
                    println!();

                    // Continue to next iteration
                }
                Ok(ClaudeStoryResult {
                    outcome: ClaudeOutcome::Error(error_info),
                    ..
                }) => {
                    state.finish_iteration(IterationStatus::Failed, error_info.to_string());
                    state.transition_to(MachineState::Failed);
                    self.state_manager.save(&state)?;

                    story_results.push(StoryResult {
                        id: story.id.clone(),
                        title: story.title.clone(),
                        passed: false,
                        duration_secs: story_start.elapsed().as_secs(),
                    });

                    print_error_panel(
                        "Claude Process Failed",
                        &error_info.message,
                        error_info.exit_code,
                        error_info.stderr.as_deref(),
                    );
                    print_summary(state.iteration, &story_results)?;
                    return Err(Autom8Error::ClaudeError(error_info.message));
                }
                Err(e) => {
                    state.finish_iteration(IterationStatus::Failed, e.to_string());
                    state.transition_to(MachineState::Failed);
                    self.state_manager.save(&state)?;

                    story_results.push(StoryResult {
                        id: story.id.clone(),
                        title: story.title.clone(),
                        passed: false,
                        duration_secs: story_start.elapsed().as_secs(),
                    });

                    print_error_panel(
                        "Claude Error",
                        &e.to_string(),
                        None,
                        None,
                    );
                    print_summary(state.iteration, &story_results)?;
                    return Err(e);
                }
            }
        }
    }

    pub fn resume(&self) -> Result<()> {
        // First try: load from active state
        if let Some(state) = self.state_manager.load_current()? {
            if state.status == crate::state::RunStatus::Running
                || state.status == crate::state::RunStatus::Failed
            {
                let prd_path = state.prd_path.clone();

                // Archive the interrupted/failed run before starting fresh
                self.state_manager.archive(&state)?;
                self.state_manager.clear_current()?;

                // Start a new run with the same parameters
                return self.run(&prd_path);
            }
        }

        // Second try: smart resume - scan for incomplete PRDs
        self.smart_resume()
    }

    /// Scan .autom8/prds/ for incomplete PRDs and offer to resume one
    fn smart_resume(&self) -> Result<()> {
        use crate::prompt;

        let prd_files = self.state_manager.list_prds()?;
        if prd_files.is_empty() {
            return Err(Autom8Error::NoPrdsToResume);
        }

        // Filter to incomplete PRDs
        let incomplete_prds: Vec<(PathBuf, Prd)> = prd_files
            .into_iter()
            .filter_map(|path| {
                Prd::load(&path).ok().and_then(|prd| {
                    if prd.is_incomplete() {
                        Some((path, prd))
                    } else {
                        None
                    }
                })
            })
            .collect();

        if incomplete_prds.is_empty() {
            return Err(Autom8Error::NoPrdsToResume);
        }

        print_header();
        println!("{YELLOW}[resume]{RESET} No active run found, scanning for incomplete PRDs...");
        println!();

        if incomplete_prds.len() == 1 {
            // Auto-resume single incomplete PRD
            let (prd_path, prd) = &incomplete_prds[0];
            let (completed, total) = prd.progress();
            println!(
                "{CYAN}Found{RESET} {} {GRAY}({}/{}){RESET}",
                prd_path.display(),
                completed,
                total
            );
            println!();
            prompt::print_action(&format!("Resuming {}", prd.project));
            println!();
            return self.run(prd_path);
        }

        // Multiple incomplete PRDs - let user choose
        println!(
            "{BOLD}Found {} incomplete PRDs:{RESET}",
            incomplete_prds.len()
        );
        println!();

        let options: Vec<String> = incomplete_prds
            .iter()
            .map(|(path, prd)| {
                let (completed, total) = prd.progress();
                let filename = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("prd.json");
                format!("{} - {} ({}/{})", filename, prd.project, completed, total)
            })
            .chain(std::iter::once("Exit".to_string()))
            .collect();

        let option_refs: Vec<&str> = options.iter().map(|s| s.as_str()).collect();
        let choice = prompt::select("Which PRD would you like to resume?", &option_refs, 0);

        // Handle Exit option
        if choice >= incomplete_prds.len() {
            println!();
            println!("Exiting.");
            return Err(Autom8Error::NoPrdsToResume);
        }

        let (prd_path, prd) = &incomplete_prds[choice];
        println!();
        prompt::print_action(&format!("Resuming {}", prd.project));
        println!();
        self.run(prd_path)
    }

    fn archive_and_cleanup(&self, state: &RunState) -> Result<()> {
        self.state_manager.archive(state)?;
        self.state_manager.clear_current()?;
        Ok(())
    }

    pub fn status(&self) -> Result<Option<RunState>> {
        self.state_manager.load_current()
    }

    pub fn history(&self) -> Result<Vec<RunState>> {
        self.state_manager.list_archived()
    }

    pub fn archive_current(&self) -> Result<Option<std::path::PathBuf>> {
        if let Some(state) = self.state_manager.load_current()? {
            let path = self.state_manager.archive(&state)?;
            self.state_manager.clear_current()?;
            Ok(Some(path))
        } else {
            Ok(None)
        }
    }
}

impl Default for Runner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runner_skip_review_defaults_to_false() {
        let runner = Runner::new();
        assert!(!runner.skip_review);
    }

    #[test]
    fn test_runner_with_skip_review_true() {
        let runner = Runner::new().with_skip_review(true);
        assert!(runner.skip_review);
    }

    #[test]
    fn test_runner_with_skip_review_false() {
        let runner = Runner::new().with_skip_review(false);
        assert!(!runner.skip_review);
    }

    #[test]
    fn test_runner_builder_pattern_preserves_skip_review() {
        let runner = Runner::new().with_verbose(true).with_skip_review(true);
        assert!(runner.skip_review);
        assert!(runner.verbose);
    }

    /// Tests that story_index calculation produces 1-indexed values.
    /// The formula: position().map(|i| i as u32 + 1).unwrap_or(state.iteration)
    /// must produce 1-indexed display values like [US-001 1/8], not [US-001 0/8].
    #[test]
    fn test_story_index_calculation_is_one_indexed() {
        // Simulate the story_index calculation from runner.rs:557-562
        let story_ids = vec!["US-001", "US-002", "US-003", "US-004", "US-005", "US-006", "US-007", "US-008"];

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
        assert_eq!(state.iteration, 1, "After first start_iteration, iteration should be 1");

        // Simulate fallback scenario where position lookup fails
        let story_ids: Vec<&str> = vec!["US-001", "US-002"];
        let unknown_story = "US-UNKNOWN";
        let story_index = story_ids
            .iter()
            .position(|&s| s == unknown_story)
            .map(|i| i as u32 + 1)
            .unwrap_or(state.iteration);

        // The fallback should use state.iteration which is 1 (1-indexed)
        assert_eq!(story_index, 1, "Fallback should use 1-indexed state.iteration");

        // After second iteration
        state.finish_iteration(crate::state::IterationStatus::Success, String::new());
        state.start_iteration("US-002");
        assert_eq!(state.iteration, 2, "After second start_iteration, iteration should be 2");
    }
}
