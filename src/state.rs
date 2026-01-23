use crate::config;
use crate::error::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

const STATE_FILE: &str = "state.json";
const RUNS_DIR: &str = "runs";
const PRDS_DIR: &str = "prds";

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunStatus {
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MachineState {
    Idle,
    LoadingSpec,
    GeneratingPrd,
    Initializing,
    PickingStory,
    RunningClaude,
    Reviewing,
    Correcting,
    Committing,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IterationRecord {
    pub number: u32,
    pub story_id: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub status: IterationStatus,
    pub output_snippet: String,
    /// Summary of what was accomplished in this iteration, for cross-task context
    #[serde(default)]
    pub work_summary: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IterationStatus {
    Running,
    Success,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunState {
    pub run_id: String,
    pub status: RunStatus,
    pub machine_state: MachineState,
    pub prd_path: PathBuf,
    #[serde(default)]
    pub spec_path: Option<PathBuf>,
    pub branch: String,
    pub current_story: Option<String>,
    pub iteration: u32,
    /// Tracks the current review cycle (1, 2, or 3) during the review loop
    #[serde(default)]
    pub review_iteration: u32,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub iterations: Vec<IterationRecord>,
}

impl RunState {
    pub fn new(prd_path: PathBuf, branch: String) -> Self {
        Self {
            run_id: Uuid::new_v4().to_string(),
            status: RunStatus::Running,
            machine_state: MachineState::Initializing,
            prd_path,
            spec_path: None,
            branch,
            current_story: None,
            iteration: 0,
            review_iteration: 0,
            started_at: Utc::now(),
            finished_at: None,
            iterations: Vec::new(),
        }
    }

    pub fn from_spec(spec_path: PathBuf, prd_path: PathBuf) -> Self {
        Self {
            run_id: Uuid::new_v4().to_string(),
            status: RunStatus::Running,
            machine_state: MachineState::LoadingSpec,
            prd_path,
            spec_path: Some(spec_path),
            branch: String::new(), // Will be set after PRD generation
            current_story: None,
            iteration: 0,
            review_iteration: 0,
            started_at: Utc::now(),
            finished_at: None,
            iterations: Vec::new(),
        }
    }

    pub fn transition_to(&mut self, state: MachineState) {
        self.machine_state = state;
        match state {
            MachineState::Completed => {
                self.status = RunStatus::Completed;
                self.finished_at = Some(Utc::now());
            }
            MachineState::Failed => {
                self.status = RunStatus::Failed;
                self.finished_at = Some(Utc::now());
            }
            _ => {}
        }
    }

    pub fn start_iteration(&mut self, story_id: &str) {
        self.iteration += 1;
        self.current_story = Some(story_id.to_string());
        self.machine_state = MachineState::RunningClaude;

        self.iterations.push(IterationRecord {
            number: self.iteration,
            story_id: story_id.to_string(),
            started_at: Utc::now(),
            finished_at: None,
            status: IterationStatus::Running,
            output_snippet: String::new(),
            work_summary: None,
        });
    }

    pub fn finish_iteration(&mut self, status: IterationStatus, output_snippet: String) {
        if let Some(iter) = self.iterations.last_mut() {
            iter.finished_at = Some(Utc::now());
            iter.status = status;
            iter.output_snippet = output_snippet;
        }
        self.machine_state = MachineState::PickingStory;
    }

    /// Set the work summary on the current (last) iteration
    pub fn set_work_summary(&mut self, summary: Option<String>) {
        if let Some(iter) = self.iterations.last_mut() {
            iter.work_summary = summary;
        }
    }

    pub fn current_iteration_duration(&self) -> u64 {
        if let Some(iter) = self.iterations.last() {
            let end = iter.finished_at.unwrap_or_else(Utc::now);
            (end - iter.started_at).num_seconds().max(0) as u64
        } else {
            0
        }
    }
}

pub struct StateManager {
    base_dir: PathBuf,
}

impl StateManager {
    /// Create a StateManager using the config directory for the current project.
    /// Uses `~/.config/autom8/<project-name>/` as the base directory.
    pub fn new() -> Result<Self> {
        let base_dir = config::project_config_dir()?;
        Ok(Self { base_dir })
    }

    /// Create a StateManager for a specific project name.
    /// Uses `~/.config/autom8/<project-name>/` as the base directory.
    pub fn for_project(project_name: &str) -> Result<Self> {
        let base_dir = config::project_config_dir_for(project_name)?;
        Ok(Self { base_dir })
    }

    /// Create a StateManager with a custom base directory (for testing).
    pub fn with_dir(dir: PathBuf) -> Self {
        Self { base_dir: dir }
    }

    fn state_file(&self) -> PathBuf {
        self.base_dir.join(STATE_FILE)
    }

    fn runs_dir(&self) -> PathBuf {
        self.base_dir.join(RUNS_DIR)
    }

    /// Path to the prds directory
    pub fn prds_dir(&self) -> PathBuf {
        self.base_dir.join(PRDS_DIR)
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        fs::create_dir_all(&self.base_dir)?;
        fs::create_dir_all(self.runs_dir())?;
        Ok(())
    }

    /// Ensure prds directory exists
    pub fn ensure_prds_dir(&self) -> Result<PathBuf> {
        let dir = self.prds_dir();
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// List all PRD files in the config directory's prds/, sorted by modification time (newest first)
    pub fn list_prds(&self) -> Result<Vec<PathBuf>> {
        let prds_dir = self.prds_dir();
        if !prds_dir.exists() {
            return Ok(Vec::new());
        }

        let mut prds: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();
        for entry in fs::read_dir(&prds_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json") {
                if let Ok(metadata) = entry.metadata() {
                    if let Ok(mtime) = metadata.modified() {
                        prds.push((path, mtime));
                    }
                }
            }
        }

        // Sort by modification time, newest first
        prds.sort_by(|a, b| b.1.cmp(&a.1));
        Ok(prds.into_iter().map(|(p, _)| p).collect())
    }

    pub fn load_current(&self) -> Result<Option<RunState>> {
        let path = self.state_file();
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path)?;
        let state: RunState = serde_json::from_str(&content)?;
        Ok(Some(state))
    }

    pub fn save(&self, state: &RunState) -> Result<()> {
        self.ensure_dirs()?;
        let content = serde_json::to_string_pretty(state)?;
        fs::write(self.state_file(), content)?;
        Ok(())
    }

    pub fn clear_current(&self) -> Result<()> {
        let path = self.state_file();
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    pub fn archive(&self, state: &RunState) -> Result<PathBuf> {
        self.ensure_dirs()?;
        let filename = format!(
            "{}_{}.json",
            state.started_at.format("%Y%m%d_%H%M%S"),
            &state.run_id[..8]
        );
        let archive_path = self.runs_dir().join(filename);
        let content = serde_json::to_string_pretty(state)?;
        fs::write(&archive_path, content)?;
        Ok(archive_path)
    }

    pub fn list_archived(&self) -> Result<Vec<RunState>> {
        let runs_dir = self.runs_dir();
        if !runs_dir.exists() {
            return Ok(Vec::new());
        }

        let mut runs = Vec::new();
        for entry in fs::read_dir(runs_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json") {
                if let Ok(content) = fs::read_to_string(&path) {
                    if let Ok(state) = serde_json::from_str::<RunState>(&content) {
                        runs.push(state);
                    }
                }
            }
        }

        runs.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        Ok(runs)
    }

    pub fn has_active_run(&self) -> Result<bool> {
        if let Some(state) = self.load_current()? {
            Ok(state.status == RunStatus::Running)
        } else {
            Ok(false)
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_machine_state_reviewing_exists() {
        let state = MachineState::Reviewing;
        assert_eq!(state, MachineState::Reviewing);
    }

    #[test]
    fn test_machine_state_correcting_exists() {
        let state = MachineState::Correcting;
        assert_eq!(state, MachineState::Correcting);
    }

    #[test]
    fn test_run_state_has_review_iteration() {
        let state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        assert_eq!(state.review_iteration, 0);
    }

    #[test]
    fn test_run_state_from_spec_has_review_iteration() {
        let state = RunState::from_spec(PathBuf::from("spec.md"), PathBuf::from("prd.json"));
        assert_eq!(state.review_iteration, 0);
    }

    #[test]
    fn test_transition_to_reviewing() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.transition_to(MachineState::Reviewing);
        assert_eq!(state.machine_state, MachineState::Reviewing);
        assert_eq!(state.status, RunStatus::Running);
    }

    #[test]
    fn test_transition_to_correcting() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.transition_to(MachineState::Correcting);
        assert_eq!(state.machine_state, MachineState::Correcting);
        assert_eq!(state.status, RunStatus::Running);
    }

    #[test]
    fn test_review_loop_state_transitions() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        // Simulate: PickingStory (all complete) → Reviewing → Correcting → Reviewing → Committing
        state.transition_to(MachineState::PickingStory);
        assert_eq!(state.machine_state, MachineState::PickingStory);

        state.transition_to(MachineState::Reviewing);
        state.review_iteration = 1;
        assert_eq!(state.machine_state, MachineState::Reviewing);
        assert_eq!(state.review_iteration, 1);

        state.transition_to(MachineState::Correcting);
        assert_eq!(state.machine_state, MachineState::Correcting);

        state.transition_to(MachineState::Reviewing);
        state.review_iteration = 2;
        assert_eq!(state.machine_state, MachineState::Reviewing);
        assert_eq!(state.review_iteration, 2);

        state.transition_to(MachineState::Committing);
        assert_eq!(state.machine_state, MachineState::Committing);
    }

    #[test]
    fn test_machine_state_serialization() {
        // Test that new states serialize correctly with kebab-case
        let reviewing = serde_json::to_string(&MachineState::Reviewing).unwrap();
        assert_eq!(reviewing, "\"reviewing\"");

        let correcting = serde_json::to_string(&MachineState::Correcting).unwrap();
        assert_eq!(correcting, "\"correcting\"");
    }

    #[test]
    fn test_machine_state_deserialization() {
        let reviewing: MachineState = serde_json::from_str("\"reviewing\"").unwrap();
        assert_eq!(reviewing, MachineState::Reviewing);

        let correcting: MachineState = serde_json::from_str("\"correcting\"").unwrap();
        assert_eq!(correcting, MachineState::Correcting);
    }

    #[test]
    fn test_run_state_review_iteration_serialization() {
        let state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("\"review_iteration\":0"));
    }

    #[test]
    fn test_iteration_record_work_summary_initialized_as_none() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.start_iteration("US-001");
        assert!(state.iterations[0].work_summary.is_none());
    }

    #[test]
    fn test_iteration_record_work_summary_can_be_set() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.start_iteration("US-001");
        state.iterations[0].work_summary = Some("Added authentication module".to_string());
        assert_eq!(
            state.iterations[0].work_summary,
            Some("Added authentication module".to_string())
        );
    }

    #[test]
    fn test_iteration_record_work_summary_serialization() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.start_iteration("US-001");
        state.iterations[0].work_summary = Some("Implemented feature X".to_string());

        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("\"work_summary\":\"Implemented feature X\""));
    }

    #[test]
    fn test_iteration_record_work_summary_none_serialization() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.start_iteration("US-001");

        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("\"work_summary\":null"));
    }

    #[test]
    fn test_iteration_record_backwards_compatible_without_work_summary() {
        // Simulate a legacy state.json that doesn't have the work_summary field
        let legacy_json = r#"{
            "number": 1,
            "story_id": "US-001",
            "started_at": "2024-01-01T00:00:00Z",
            "finished_at": null,
            "status": "running",
            "output_snippet": ""
        }"#;

        let record: IterationRecord = serde_json::from_str(legacy_json).unwrap();
        assert!(record.work_summary.is_none());
        assert_eq!(record.story_id, "US-001");
    }

    #[test]
    fn test_set_work_summary_on_current_iteration() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.start_iteration("US-001");
        assert!(state.iterations[0].work_summary.is_none());

        state.set_work_summary(Some("Added new feature X".to_string()));
        assert_eq!(
            state.iterations[0].work_summary,
            Some("Added new feature X".to_string())
        );
    }

    #[test]
    fn test_set_work_summary_none() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.start_iteration("US-001");
        state.iterations[0].work_summary = Some("Existing summary".to_string());

        state.set_work_summary(None);
        assert!(state.iterations[0].work_summary.is_none());
    }

    #[test]
    fn test_set_work_summary_no_iterations() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        // No iterations started yet
        state.set_work_summary(Some("Should not crash".to_string()));
        // Should not panic, just do nothing
        assert!(state.iterations.is_empty());
    }

    // StateManager tests using with_dir for testability
    use tempfile::TempDir;

    #[test]
    fn test_state_manager_with_dir_creates_state_file_in_base_dir() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let run_state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        sm.save(&run_state).unwrap();

        // state.json should be in the base dir
        assert!(temp_dir.path().join(STATE_FILE).exists());
    }

    #[test]
    fn test_state_manager_with_dir_creates_runs_subdir() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        sm.ensure_dirs().unwrap();

        // runs/ should be in the base dir
        assert!(temp_dir.path().join(RUNS_DIR).exists());
        assert!(temp_dir.path().join(RUNS_DIR).is_dir());
    }

    #[test]
    fn test_state_manager_with_dir_creates_prds_subdir() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let prds_dir = sm.ensure_prds_dir().unwrap();

        // prds/ should be in the base dir
        assert_eq!(prds_dir, temp_dir.path().join(PRDS_DIR));
        assert!(prds_dir.exists());
        assert!(prds_dir.is_dir());
    }

    #[test]
    fn test_state_manager_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let mut run_state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        run_state.start_iteration("US-001");

        sm.save(&run_state).unwrap();

        let loaded = sm.load_current().unwrap();
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.run_id, run_state.run_id);
        assert_eq!(loaded.branch, "test-branch");
        assert_eq!(loaded.iteration, 1);
    }

    #[test]
    fn test_state_manager_clear_current() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let run_state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        sm.save(&run_state).unwrap();
        assert!(sm.load_current().unwrap().is_some());

        sm.clear_current().unwrap();
        assert!(sm.load_current().unwrap().is_none());
    }

    #[test]
    fn test_state_manager_archive() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let run_state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        let archive_path = sm.archive(&run_state).unwrap();

        assert!(archive_path.exists());
        assert!(archive_path.starts_with(temp_dir.path().join(RUNS_DIR)));
    }

    #[test]
    fn test_state_manager_list_archived() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        // No archived runs initially
        let runs = sm.list_archived().unwrap();
        assert!(runs.is_empty());

        // Archive a run
        let run_state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        sm.archive(&run_state).unwrap();

        let runs = sm.list_archived().unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].run_id, run_state.run_id);
    }

    #[test]
    fn test_state_manager_has_active_run() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        // No active run initially
        assert!(!sm.has_active_run().unwrap());

        // Save a running state
        let run_state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        assert_eq!(run_state.status, RunStatus::Running);
        sm.save(&run_state).unwrap();

        assert!(sm.has_active_run().unwrap());
    }

    #[test]
    fn test_state_manager_list_prds_empty() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let prds = sm.list_prds().unwrap();
        assert!(prds.is_empty());
    }

    #[test]
    fn test_state_manager_list_prds_finds_json_files() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let prds_dir = sm.ensure_prds_dir().unwrap();

        // Create a test PRD file
        let prd_content = r#"{"project": "test", "branchName": "test", "userStories": []}"#;
        fs::write(prds_dir.join("test.json"), prd_content).unwrap();

        let prds = sm.list_prds().unwrap();
        assert_eq!(prds.len(), 1);
        assert!(prds[0].ends_with("test.json"));
    }

    #[test]
    fn test_state_manager_new_uses_config_directory() {
        // This test verifies that StateManager::new() uses the config directory
        let sm = StateManager::new().unwrap();
        let prds_dir = sm.prds_dir();

        // The prds_dir should be under ~/.config/autom8/<project-name>/prds/
        assert!(prds_dir.ends_with("prds"));
        // Parent should be the project name (autom8 when running tests)
        let project_dir = prds_dir.parent().unwrap();
        assert!(project_dir.parent().unwrap().ends_with("autom8"));
    }

    #[test]
    fn test_state_manager_for_project() {
        let sm = StateManager::for_project("test-project").unwrap();
        let prds_dir = sm.prds_dir();

        // The prds_dir should be under ~/.config/autom8/test-project/prds/
        assert!(prds_dir.ends_with("prds"));
        let project_dir = prds_dir.parent().unwrap();
        assert!(project_dir.ends_with("test-project"));
    }

    // ======================================================================
    // Tests for resume command config directory integration (US-007)
    // ======================================================================

    /// Tests that StateManager::new() creates a state file path in the config directory.
    /// This is used by the resume command to find active runs.
    #[test]
    fn test_state_manager_state_file_in_config_directory() {
        let sm = StateManager::new().unwrap();

        // save and load operations use state_file() which should be in config dir
        // We verify the structure: ~/.config/autom8/<project-name>/state.json
        let state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        // Save should work (creates in config directory)
        assert!(sm.save(&state).is_ok(), "Should save state to config directory");

        // Load should find the state in config directory
        let loaded = sm.load_current().unwrap();
        assert!(loaded.is_some(), "Should load state from config directory");
        assert_eq!(loaded.unwrap().run_id, state.run_id);

        // Cleanup
        sm.clear_current().unwrap();
    }

    /// Tests that smart_resume (via list_prds) scans the config directory prds/.
    /// This verifies the path: ~/.config/autom8/<project-name>/prds/
    #[test]
    fn test_state_manager_list_prds_uses_config_directory() {
        let sm = StateManager::new().unwrap();
        let prds_dir = sm.prds_dir();

        // Verify path structure
        let path_str = prds_dir.to_string_lossy();
        assert!(
            path_str.contains(".config/autom8/") || path_str.contains(".config\\autom8\\"),
            "prds_dir should be in ~/.config/autom8/: got {}",
            path_str
        );
        assert!(prds_dir.ends_with("prds"), "prds_dir should end with 'prds'");

        // list_prds should work (even if empty)
        let prds = sm.list_prds().unwrap();
        assert!(prds.is_empty() || prds.iter().all(|p| p.to_string_lossy().contains(".config/autom8/")),
            "All PRDs should be in config directory");
    }

    /// Tests that archived runs (used by resume history) are stored in config directory.
    #[test]
    fn test_state_manager_archive_uses_config_directory() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        let archive_path = sm.archive(&state).unwrap();

        // Archive should be in runs/ subdirectory
        assert!(archive_path.starts_with(temp_dir.path().join("runs")));

        // list_archived should find it
        let archived = sm.list_archived().unwrap();
        assert_eq!(archived.len(), 1);
        assert_eq!(archived[0].run_id, state.run_id);
    }

    /// Tests has_active_run() which is used by resume to check for ongoing work.
    #[test]
    fn test_state_manager_has_active_run_for_resume() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        // No active run initially
        assert!(!sm.has_active_run().unwrap(), "Should have no active run initially");

        // Save a running state
        let state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        sm.save(&state).unwrap();

        // Now has_active_run should return true
        assert!(sm.has_active_run().unwrap(), "Should detect active run");

        // Clear and verify
        sm.clear_current().unwrap();
        assert!(!sm.has_active_run().unwrap(), "Should have no active run after clear");
    }

    /// Tests that clean command can list and delete PRDs from config directory.
    /// This verifies that files in ~/.config/autom8/<project-name>/prds/ can be:
    /// 1. Listed via list_prds()
    /// 2. Deleted via standard fs::remove_file()
    #[test]
    fn test_clean_command_operates_on_config_directory() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        // Create prds directory structure
        let prds_dir = temp_dir.path().join("prds");
        std::fs::create_dir_all(&prds_dir).unwrap();

        // Create test PRD files
        let prd1 = prds_dir.join("prd-feature1.json");
        let prd2 = prds_dir.join("prd-feature2.json");
        std::fs::write(&prd1, r#"{"project": "test1"}"#).unwrap();
        std::fs::write(&prd2, r#"{"project": "test2"}"#).unwrap();

        // Verify list_prds finds the files
        let prds = sm.list_prds().unwrap();
        assert_eq!(prds.len(), 2, "Should find 2 PRD files");

        // Verify prds_dir points to config directory structure
        assert_eq!(sm.prds_dir(), prds_dir);

        // Clean (delete) the files - simulating what clean_prd_files does
        for prd_path in &prds {
            std::fs::remove_file(prd_path).unwrap();
        }

        // Verify files are gone
        let prds_after = sm.list_prds().unwrap();
        assert!(prds_after.is_empty(), "All PRD files should be deleted");
        assert!(!prd1.exists(), "prd-feature1.json should be deleted");
        assert!(!prd2.exists(), "prd-feature2.json should be deleted");
    }

    /// Tests that clean command no longer operates on legacy .autom8/ location.
    /// PRD files should ONLY be found in the config directory.
    #[test]
    fn test_clean_uses_config_directory_not_legacy_location() {
        let sm = StateManager::new().unwrap();
        let prds_dir = sm.prds_dir();

        // prds_dir should NOT point to .autom8/prds/ in current directory
        let path_str = prds_dir.to_string_lossy();
        assert!(
            !path_str.starts_with(".autom8/") && !path_str.contains("/.autom8/prds"),
            "prds_dir should not reference legacy .autom8/ location: got {}",
            path_str
        );

        // Should be in ~/.config/autom8/
        assert!(
            path_str.contains(".config/autom8/") || path_str.contains(".config\\autom8\\"),
            "prds_dir should be in config directory: got {}",
            path_str
        );
    }
}
