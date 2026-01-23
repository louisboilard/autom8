use crate::error::Result;
use crate::state::{RunState, StateManager};
use std::path::PathBuf;

pub struct ArchiveManager {
    state_manager: StateManager,
}

impl ArchiveManager {
    pub fn new() -> Result<Self> {
        Ok(Self {
            state_manager: StateManager::new()?,
        })
    }

    pub fn archive_current_run(&self) -> Result<Option<PathBuf>> {
        if let Some(state) = self.state_manager.load_current()? {
            let path = self.state_manager.archive(&state)?;
            self.state_manager.clear_current()?;
            Ok(Some(path))
        } else {
            Ok(None)
        }
    }

    pub fn list_runs(&self) -> Result<Vec<RunState>> {
        self.state_manager.list_archived()
    }

    pub fn get_run_stats(&self) -> Result<ArchiveStats> {
        let runs = self.list_runs()?;

        let total_runs = runs.len();
        let completed_runs = runs
            .iter()
            .filter(|r| r.status == crate::state::RunStatus::Completed)
            .count();
        let failed_runs = runs
            .iter()
            .filter(|r| r.status == crate::state::RunStatus::Failed)
            .count();
        let total_iterations: usize = runs.iter().map(|r| r.iterations.len()).sum();

        Ok(ArchiveStats {
            total_runs,
            completed_runs,
            failed_runs,
            total_iterations,
        })
    }
}


#[derive(Debug)]
pub struct ArchiveStats {
    pub total_runs: usize,
    pub completed_runs: usize,
    pub failed_runs: usize,
    pub total_iterations: usize,
}
