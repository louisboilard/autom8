use crate::error::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

const STATE_DIR: &str = ".autom8";
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
    pub fn new() -> Self {
        Self {
            base_dir: PathBuf::from(STATE_DIR),
        }
    }

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

    /// List all PRD files in .autom8/prds/, sorted by modification time (newest first)
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

impl Default for StateManager {
    fn default() -> Self {
        Self::new()
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
}
