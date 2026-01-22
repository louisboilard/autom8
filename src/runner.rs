use crate::claude::{run_claude, run_for_prd_generation, ClaudeResult};
use crate::error::{Autom8Error, Result};
use crate::git;
use crate::output::{
    print_all_complete, print_error, print_generating_prd, print_info,
    print_iteration_complete, print_iteration_start, print_prd_generated,
    print_proceeding_to_implementation, print_project_info, print_spec_loaded,
    print_state_transition, print_story_complete,
};
use crate::prd::Prd;
use crate::state::{IterationStatus, MachineState, RunState, StateManager};
use std::fs;
use std::path::Path;

pub struct Runner {
    state_manager: StateManager,
}

impl Runner {
    pub fn new() -> Self {
        Self {
            state_manager: StateManager::new(),
        }
    }

    /// Run from a prd.md spec file - converts to JSON first, then implements
    pub fn run_from_spec(&self, spec_path: &Path, max_iterations: u32) -> Result<()> {
        // Check for existing active run
        if self.state_manager.has_active_run()? {
            if let Some(state) = self.state_manager.load_current()? {
                return Err(Autom8Error::RunInProgress(state.run_id));
            }
        }

        // Canonicalize spec path
        let spec_path = spec_path.canonicalize().map_err(|_| {
            Autom8Error::SpecNotFound(spec_path.to_path_buf())
        })?;

        // Determine PRD output path (same directory as spec)
        let prd_path = spec_path
            .parent()
            .unwrap_or(Path::new("."))
            .join("prd.json");

        // Initialize state
        let mut state = RunState::from_spec(
            spec_path.clone(),
            prd_path.clone(),
            max_iterations,
        );
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
        let prd = run_for_prd_generation(&spec_content, &prd_path)?;

        print_prd_generated(&prd, &prd_path);

        // Update state with branch from generated PRD
        state.branch = prd.branch_name.clone();
        state.transition_to(MachineState::Initializing);
        self.state_manager.save(&state)?;
        print_state_transition(MachineState::GeneratingPrd, MachineState::Initializing);

        print_proceeding_to_implementation();

        // Continue with normal implementation flow
        self.run_implementation_loop(state, &prd_path, max_iterations)
    }

    pub fn run(&self, prd_path: &Path, max_iterations: u32) -> Result<()> {
        // Check for existing active run
        if self.state_manager.has_active_run()? {
            if let Some(state) = self.state_manager.load_current()? {
                return Err(Autom8Error::RunInProgress(state.run_id));
            }
        }

        // Canonicalize path so resume works from any directory
        let prd_path = prd_path.canonicalize().map_err(|_| {
            Autom8Error::PrdNotFound(prd_path.to_path_buf())
        })?;

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
        let state = RunState::new(
            prd_path.to_path_buf(),
            prd.branch_name.clone(),
            max_iterations,
        );

        print_state_transition(MachineState::Idle, MachineState::Initializing);
        print_project_info(&prd);

        self.run_implementation_loop(state, &prd_path, max_iterations)
    }

    fn run_implementation_loop(
        &self,
        mut state: RunState,
        prd_path: &Path,
        max_iterations: u32,
    ) -> Result<()> {
        // Transition to PickingStory
        print_state_transition(state.machine_state, MachineState::PickingStory);
        state.transition_to(MachineState::PickingStory);
        self.state_manager.save(&state)?;

        // Main loop
        loop {
            // Reload PRD to get latest passes state
            let prd = Prd::load(prd_path)?;

            // Check if all stories complete
            if prd.all_complete() {
                print_state_transition(state.machine_state, MachineState::Completed);
                state.transition_to(MachineState::Completed);
                self.state_manager.save(&state)?;
                print_all_complete();
                self.archive_and_cleanup(&state)?;
                return Ok(());
            }

            // Check iteration limit
            if state.iteration >= max_iterations {
                state.transition_to(MachineState::Failed);
                self.state_manager.save(&state)?;
                return Err(Autom8Error::MaxIterationsReached(max_iterations));
            }

            // Pick next story
            let story = prd
                .next_incomplete_story()
                .ok_or(Autom8Error::NoIncompleteStories)?
                .clone();

            // Start iteration
            print_state_transition(MachineState::PickingStory, MachineState::RunningClaude);
            state.start_iteration(&story.id);
            self.state_manager.save(&state)?;

            print_iteration_start(state.iteration, max_iterations, &story.id, &story.title);

            // Run Claude
            let result = run_claude(&prd, &story, prd_path);

            match result {
                Ok(ClaudeResult::AllStoriesComplete) => {
                    state.finish_iteration(IterationStatus::Success, String::new());
                    print_state_transition(MachineState::RunningClaude, MachineState::Completed);
                    state.transition_to(MachineState::Completed);
                    self.state_manager.save(&state)?;

                    let duration = state.current_iteration_duration();
                    print_story_complete(&story.id, duration);
                    print_all_complete();

                    self.archive_and_cleanup(&state)?;
                    return Ok(());
                }
                Ok(ClaudeResult::IterationComplete) => {
                    state.finish_iteration(IterationStatus::Success, String::new());
                    self.state_manager.save(&state)?;

                    let duration = state.current_iteration_duration();
                    print_state_transition(MachineState::RunningClaude, MachineState::PickingStory);
                    print_iteration_complete(state.iteration);

                    // Reload PRD and check if current story passed
                    let updated_prd = Prd::load(prd_path)?;
                    if updated_prd
                        .user_stories
                        .iter()
                        .find(|s| s.id == story.id)
                        .is_some_and(|s| s.passes)
                    {
                        print_story_complete(&story.id, duration);
                    }

                    // Continue to next iteration
                }
                Ok(ClaudeResult::Error(msg)) => {
                    state.finish_iteration(IterationStatus::Failed, msg.clone());
                    state.transition_to(MachineState::Failed);
                    self.state_manager.save(&state)?;

                    print_error(&msg);
                    return Err(Autom8Error::ClaudeError(msg));
                }
                Err(e) => {
                    state.finish_iteration(IterationStatus::Failed, e.to_string());
                    state.transition_to(MachineState::Failed);
                    self.state_manager.save(&state)?;

                    print_error(&e.to_string());
                    return Err(e);
                }
            }
        }
    }

    pub fn resume(&self) -> Result<()> {
        let state = self
            .state_manager
            .load_current()?
            .ok_or(Autom8Error::NoActiveRun)?;

        if state.status != crate::state::RunStatus::Running
            && state.status != crate::state::RunStatus::Failed
        {
            return Err(Autom8Error::NoActiveRun);
        }

        let prd_path = state.prd_path.clone();
        let max_iterations = state.max_iterations;

        // Archive the interrupted/failed run before starting fresh
        self.state_manager.archive(&state)?;
        self.state_manager.clear_current()?;

        // Start a new run with the same parameters
        self.run(&prd_path, max_iterations)
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
