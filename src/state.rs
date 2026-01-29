use crate::claude::{extract_decisions, extract_files_context, extract_patterns, FileContextEntry};
use crate::config::{self, Config};
use crate::error::Result;
use crate::git;
use crate::knowledge::{Decision, FileChange, FileInfo, Pattern, ProjectKnowledge, StoryChanges};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

const STATE_FILE: &str = "state.json";
const RUNS_DIR: &str = "runs";
const SPEC_DIR: &str = "spec";

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
    GeneratingSpec,
    Initializing,
    PickingStory,
    RunningClaude,
    Reviewing,
    Correcting,
    Committing,
    #[serde(rename = "creating-pr")]
    CreatingPR,
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
    pub spec_json_path: PathBuf,
    #[serde(default)]
    pub spec_md_path: Option<PathBuf>,
    pub branch: String,
    pub current_story: Option<String>,
    pub iteration: u32,
    /// Tracks the current review cycle (1, 2, or 3) during the review loop
    #[serde(default)]
    pub review_iteration: u32,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub iterations: Vec<IterationRecord>,
    /// Configuration snapshot taken at run start.
    /// This ensures resumed runs use the same config they started with.
    #[serde(default)]
    pub config: Option<Config>,
    /// Cumulative project knowledge tracked across agent runs.
    /// Contains file info, decisions, patterns, and story changes.
    #[serde(default)]
    pub knowledge: ProjectKnowledge,
    /// Git commit hash captured before starting each story.
    /// Used to calculate diffs for what changed during the story.
    #[serde(default)]
    pub pre_story_commit: Option<String>,
}

impl RunState {
    pub fn new(spec_json_path: PathBuf, branch: String) -> Self {
        Self {
            run_id: Uuid::new_v4().to_string(),
            status: RunStatus::Running,
            machine_state: MachineState::Initializing,
            spec_json_path,
            spec_md_path: None,
            branch,
            current_story: None,
            iteration: 0,
            review_iteration: 0,
            started_at: Utc::now(),
            finished_at: None,
            iterations: Vec::new(),
            config: None,
            knowledge: ProjectKnowledge::default(),
            pre_story_commit: None,
        }
    }

    /// Create a new RunState with a config snapshot.
    pub fn new_with_config(spec_json_path: PathBuf, branch: String, config: Config) -> Self {
        Self {
            run_id: Uuid::new_v4().to_string(),
            status: RunStatus::Running,
            machine_state: MachineState::Initializing,
            spec_json_path,
            spec_md_path: None,
            branch,
            current_story: None,
            iteration: 0,
            review_iteration: 0,
            started_at: Utc::now(),
            finished_at: None,
            iterations: Vec::new(),
            config: Some(config),
            knowledge: ProjectKnowledge::default(),
            pre_story_commit: None,
        }
    }

    pub fn from_spec(spec_md_path: PathBuf, spec_json_path: PathBuf) -> Self {
        Self {
            run_id: Uuid::new_v4().to_string(),
            status: RunStatus::Running,
            machine_state: MachineState::LoadingSpec,
            spec_json_path,
            spec_md_path: Some(spec_md_path),
            branch: String::new(), // Will be set after spec generation
            current_story: None,
            iteration: 0,
            review_iteration: 0,
            started_at: Utc::now(),
            finished_at: None,
            iterations: Vec::new(),
            config: None,
            knowledge: ProjectKnowledge::default(),
            pre_story_commit: None,
        }
    }

    /// Create a RunState from spec with a config snapshot.
    pub fn from_spec_with_config(
        spec_md_path: PathBuf,
        spec_json_path: PathBuf,
        config: Config,
    ) -> Self {
        Self {
            run_id: Uuid::new_v4().to_string(),
            status: RunStatus::Running,
            machine_state: MachineState::LoadingSpec,
            spec_json_path,
            spec_md_path: Some(spec_md_path),
            branch: String::new(), // Will be set after spec generation
            current_story: None,
            iteration: 0,
            review_iteration: 0,
            started_at: Utc::now(),
            finished_at: None,
            iterations: Vec::new(),
            config: Some(config),
            knowledge: ProjectKnowledge::default(),
            pre_story_commit: None,
        }
    }

    /// Get the effective config for this run.
    /// Returns the stored config if available, otherwise the default.
    pub fn effective_config(&self) -> Config {
        self.config.clone().unwrap_or_default()
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

    /// Capture the current HEAD commit before starting a story.
    ///
    /// This stores the commit hash so we can later calculate what changed
    /// during the story implementation. For non-git projects, this is a no-op.
    ///
    /// On the first call (when `baseline_commit` is not set), this also captures
    /// the baseline commit for the entire run. This is used to track which files
    /// autom8 touched vs external changes (US-010).
    pub fn capture_pre_story_state(&mut self) {
        if git::is_git_repo() {
            if let Ok(head) = git::get_head_commit() {
                // Capture baseline commit on first story (US-010)
                if self.knowledge.baseline_commit.is_none() {
                    self.knowledge.baseline_commit = Some(head.clone());
                }
                self.pre_story_commit = Some(head);
            }
        }
    }

    /// Record changes made during a story and update project knowledge.
    ///
    /// This method:
    /// 1. Captures the git diff since `pre_story_commit`
    /// 2. Creates a `StoryChanges` record
    /// 3. Adds it to the project knowledge
    ///
    /// For non-git projects or if `pre_story_commit` is not set, this creates
    /// an empty `StoryChanges` record.
    ///
    /// # Arguments
    /// * `story_id` - The ID of the story that was just implemented
    /// * `commit_hash` - Optional commit hash if the changes were committed
    pub fn record_story_changes(&mut self, story_id: &str, commit_hash: Option<String>) {
        let mut files_created = Vec::new();
        let mut files_modified = Vec::new();
        let mut files_deleted = Vec::new();

        // If we have a pre-story commit, calculate the diff
        if let Some(ref base_commit) = self.pre_story_commit {
            if git::is_git_repo() {
                if let Ok(entries) = git::get_diff_since(base_commit) {
                    for entry in entries {
                        let file_change = FileChange {
                            path: entry.path.clone(),
                            additions: entry.additions,
                            deletions: entry.deletions,
                            purpose: None,
                            key_symbols: Vec::new(),
                        };

                        match entry.status {
                            git::DiffStatus::Added => files_created.push(file_change),
                            git::DiffStatus::Modified => files_modified.push(file_change),
                            git::DiffStatus::Deleted => files_deleted.push(entry.path),
                        }
                    }
                }
            }
        }

        let story_changes = StoryChanges {
            story_id: story_id.to_string(),
            files_created,
            files_modified,
            files_deleted,
            commit_hash,
        };

        self.knowledge.story_changes.push(story_changes);

        // Clear pre_story_commit after recording
        self.pre_story_commit = None;
    }

    /// Capture story knowledge after agent completion.
    ///
    /// This method combines two sources of truth:
    /// 1. Git diff data for empirical knowledge of what files were created/modified
    /// 2. Agent-provided semantic information (files context, decisions, patterns)
    ///
    /// The method:
    /// - Gets git diff since `pre_story_commit` (if available)
    /// - Filters changes to only include files autom8 touched (see US-010)
    /// - Extracts structured context from the agent's output
    /// - Creates a `StoryChanges` record combining both sources
    /// - Merges file info into the `knowledge.files` registry
    /// - Appends decisions and patterns to knowledge
    ///
    /// For non-git projects, only agent-provided context is used.
    ///
    /// # Arguments
    /// * `story_id` - The ID of the story that was just implemented
    /// * `agent_output` - The full output from the Claude agent
    /// * `commit_hash` - Optional commit hash if the changes were committed
    pub fn capture_story_knowledge(
        &mut self,
        story_id: &str,
        agent_output: &str,
        commit_hash: Option<String>,
    ) {
        // Extract structured context from agent output
        let files_context = extract_files_context(agent_output);
        let agent_decisions = extract_decisions(agent_output);
        let agent_patterns = extract_patterns(agent_output);

        // Build a map of agent-provided context for enriching git diff data
        let context_by_path: std::collections::HashMap<PathBuf, &FileContextEntry> = files_context
            .iter()
            .map(|fc| (fc.path.clone(), fc))
            .collect();

        let mut files_created = Vec::new();
        let mut files_modified = Vec::new();
        let mut files_deleted: Vec<PathBuf> = Vec::new();

        // If we have a pre-story commit, calculate the diff
        if let Some(ref base_commit) = self.pre_story_commit {
            if git::is_git_repo() {
                if let Ok(all_entries) = git::get_diff_since(base_commit) {
                    // Filter to only include changes autom8 made (US-010)
                    let entries = self.knowledge.filter_our_changes(&all_entries);

                    for entry in entries {
                        // Enrich with agent-provided context if available
                        let (purpose, key_symbols) = context_by_path
                            .get(&entry.path)
                            .map(|fc| (Some(fc.purpose.clone()), fc.key_symbols.clone()))
                            .unwrap_or((None, Vec::new()));

                        let file_change = FileChange {
                            path: entry.path.clone(),
                            additions: entry.additions,
                            deletions: entry.deletions,
                            purpose,
                            key_symbols,
                        };

                        match entry.status {
                            git::DiffStatus::Added => files_created.push(file_change),
                            git::DiffStatus::Modified => files_modified.push(file_change),
                            git::DiffStatus::Deleted => files_deleted.push(entry.path),
                        }
                    }
                }
            }
        }

        // For non-git projects or when no diff available, use agent context directly
        if files_created.is_empty() && files_modified.is_empty() && files_deleted.is_empty() {
            // Create file changes from agent context only
            for fc in &files_context {
                // We can't know from agent context alone if a file was created vs modified,
                // so we treat them as modified (safer assumption)
                files_modified.push(FileChange {
                    path: fc.path.clone(),
                    additions: 0,
                    deletions: 0,
                    purpose: Some(fc.purpose.clone()),
                    key_symbols: fc.key_symbols.clone(),
                });
            }
        }

        // Create and add story changes
        let story_changes = StoryChanges {
            story_id: story_id.to_string(),
            files_created: files_created.clone(),
            files_modified: files_modified.clone(),
            files_deleted: files_deleted.clone(),
            commit_hash,
        };
        self.knowledge.story_changes.push(story_changes);

        // Merge file info into the files registry
        for change in files_created.iter().chain(files_modified.iter()) {
            let file_info = self
                .knowledge
                .files
                .entry(change.path.clone())
                .or_insert_with(|| FileInfo {
                    purpose: change.purpose.clone().unwrap_or_default(),
                    key_symbols: Vec::new(),
                    touched_by: Vec::new(),
                    line_count: 0,
                });

            // Update purpose if we have a new one
            if let Some(ref purpose) = change.purpose {
                file_info.purpose = purpose.clone();
            }

            // Merge key symbols (avoid duplicates)
            for symbol in &change.key_symbols {
                if !file_info.key_symbols.contains(symbol) {
                    file_info.key_symbols.push(symbol.clone());
                }
            }

            // Add story to touched_by if not already present
            if !file_info.touched_by.contains(&story_id.to_string()) {
                file_info.touched_by.push(story_id.to_string());
            }

            // Update line count if available
            if change.additions > 0 {
                file_info.line_count = file_info.line_count.saturating_add(change.additions);
                file_info.line_count = file_info.line_count.saturating_sub(change.deletions);
            }
        }

        // Remove deleted files from registry
        for deleted_path in &files_deleted {
            self.knowledge.files.remove(deleted_path);
        }

        // Append decisions to knowledge
        for agent_decision in agent_decisions {
            self.knowledge.decisions.push(Decision {
                story_id: story_id.to_string(),
                topic: agent_decision.topic,
                choice: agent_decision.choice,
                rationale: agent_decision.rationale,
            });
        }

        // Append patterns to knowledge
        for agent_pattern in agent_patterns {
            self.knowledge.patterns.push(Pattern {
                story_id: story_id.to_string(),
                description: agent_pattern.description,
                example_file: None, // Agent doesn't provide example file in pattern output
            });
        }

        // Clear pre_story_commit after recording
        self.pre_story_commit = None;
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

    /// Path to the spec directory
    pub fn spec_dir(&self) -> PathBuf {
        self.base_dir.join(SPEC_DIR)
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        fs::create_dir_all(&self.base_dir)?;
        fs::create_dir_all(self.runs_dir())?;
        Ok(())
    }

    /// Ensure spec directory exists
    pub fn ensure_spec_dir(&self) -> Result<PathBuf> {
        let dir = self.spec_dir();
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// List all spec JSON files in the config directory's spec/, sorted by modification time (newest first)
    pub fn list_specs(&self) -> Result<Vec<PathBuf>> {
        let spec_dir = self.spec_dir();
        if !spec_dir.exists() {
            return Ok(Vec::new());
        }

        let mut specs: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();
        for entry in fs::read_dir(&spec_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json") {
                if let Ok(metadata) = entry.metadata() {
                    if let Ok(mtime) = metadata.modified() {
                        specs.push((path, mtime));
                    }
                }
            }
        }

        // Sort by modification time, newest first
        specs.sort_by(|a, b| b.1.cmp(&a.1));
        Ok(specs.into_iter().map(|(p, _)| p).collect())
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
    use std::sync::Mutex;

    // Mutex to serialize tests that change the current directory
    // This prevents race conditions when multiple tests try to change cwd concurrently
    static CWD_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn test_run_state_has_review_iteration() {
        let state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        assert_eq!(state.review_iteration, 0);
    }

    #[test]
    fn test_run_state_from_spec_has_review_iteration() {
        let state = RunState::from_spec(
            PathBuf::from("spec-feature.md"),
            PathBuf::from("spec-feature.json"),
        );
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
    fn test_machine_state_serialization_roundtrip() {
        // Test all MachineState variants serialize/deserialize correctly with kebab-case
        let test_cases: &[(MachineState, &str)] = &[
            (MachineState::Idle, "\"idle\""),
            (MachineState::LoadingSpec, "\"loading-spec\""),
            (MachineState::GeneratingSpec, "\"generating-spec\""),
            (MachineState::Initializing, "\"initializing\""),
            (MachineState::PickingStory, "\"picking-story\""),
            (MachineState::RunningClaude, "\"running-claude\""),
            (MachineState::Reviewing, "\"reviewing\""),
            (MachineState::Correcting, "\"correcting\""),
            (MachineState::Committing, "\"committing\""),
            (MachineState::CreatingPR, "\"creating-pr\""),
            (MachineState::Completed, "\"completed\""),
            (MachineState::Failed, "\"failed\""),
        ];

        for (state, expected_json) in test_cases {
            // Test serialization
            let serialized = serde_json::to_string(state).unwrap();
            assert_eq!(
                &serialized, *expected_json,
                "Serialization failed for {:?}",
                state
            );

            // Test deserialization roundtrip
            let deserialized: MachineState = serde_json::from_str(&serialized).unwrap();
            assert_eq!(
                deserialized, *state,
                "Deserialization failed for {:?}",
                state
            );
        }
    }

    #[test]
    fn test_transition_to_creating_pr() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.transition_to(MachineState::CreatingPR);
        assert_eq!(state.machine_state, MachineState::CreatingPR);
        assert_eq!(state.status, RunStatus::Running); // Should remain running, not completed
    }

    #[test]
    fn test_creating_pr_state_workflow_position() {
        // Test that CreatingPR can transition from Committing and to Completed
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        state.transition_to(MachineState::Committing);
        assert_eq!(state.machine_state, MachineState::Committing);

        state.transition_to(MachineState::CreatingPR);
        assert_eq!(state.machine_state, MachineState::CreatingPR);
        assert_eq!(state.status, RunStatus::Running);

        state.transition_to(MachineState::Completed);
        assert_eq!(state.machine_state, MachineState::Completed);
        assert_eq!(state.status, RunStatus::Completed);
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
    fn test_state_manager_with_dir_creates_spec_subdir() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let spec_dir = sm.ensure_spec_dir().unwrap();

        // spec/ should be in the base dir
        assert_eq!(spec_dir, temp_dir.path().join(SPEC_DIR));
        assert!(spec_dir.exists());
        assert!(spec_dir.is_dir());
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
    fn test_state_manager_list_specs_empty() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let specs = sm.list_specs().unwrap();
        assert!(specs.is_empty());
    }

    #[test]
    fn test_state_manager_list_specs_finds_json_files() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let spec_dir = sm.ensure_spec_dir().unwrap();

        // Create a test spec file
        let spec_content = r#"{"project": "test", "branchName": "test", "userStories": []}"#;
        fs::write(spec_dir.join("test.json"), spec_content).unwrap();

        let specs = sm.list_specs().unwrap();
        assert_eq!(specs.len(), 1);
        assert!(specs[0].ends_with("test.json"));
    }

    #[test]
    fn test_state_manager_new_uses_config_directory() {
        // This test verifies that StateManager::new() uses the config directory
        let sm = StateManager::new().unwrap();
        let spec_dir = sm.spec_dir();

        // The spec_dir should be under ~/.config/autom8/<project-name>/spec/
        assert!(spec_dir.ends_with("spec"));
        // Parent should be the project name (autom8 when running tests)
        let project_dir = spec_dir.parent().unwrap();
        assert!(project_dir.parent().unwrap().ends_with("autom8"));
    }

    #[test]
    fn test_state_manager_for_project() {
        let sm = StateManager::for_project("test-project").unwrap();
        let spec_dir = sm.spec_dir();

        // The spec_dir should be under ~/.config/autom8/test-project/spec/
        assert!(spec_dir.ends_with("spec"));
        let project_dir = spec_dir.parent().unwrap();
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
        assert!(
            sm.save(&state).is_ok(),
            "Should save state to config directory"
        );

        // Load should find the state in config directory
        let loaded = sm.load_current().unwrap();
        assert!(loaded.is_some(), "Should load state from config directory");
        assert_eq!(loaded.unwrap().run_id, state.run_id);

        // Cleanup
        sm.clear_current().unwrap();
    }

    /// Tests that smart_resume (via list_specs) scans the config directory spec/.
    /// This verifies the path: ~/.config/autom8/<project-name>/spec/
    #[test]
    fn test_state_manager_list_specs_uses_config_directory() {
        let sm = StateManager::new().unwrap();
        let spec_dir = sm.spec_dir();

        // Verify path structure
        let path_str = spec_dir.to_string_lossy();
        assert!(
            path_str.contains(".config/autom8/") || path_str.contains(".config\\autom8\\"),
            "spec_dir should be in ~/.config/autom8/: got {}",
            path_str
        );
        assert!(
            spec_dir.ends_with("spec"),
            "spec_dir should end with 'spec'"
        );

        // list_specs should work (even if empty)
        let specs = sm.list_specs().unwrap();
        assert!(
            specs.is_empty()
                || specs
                    .iter()
                    .all(|p| p.to_string_lossy().contains(".config/autom8/")),
            "All specs should be in config directory"
        );
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
        assert!(
            !sm.has_active_run().unwrap(),
            "Should have no active run initially"
        );

        // Save a running state
        let state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        sm.save(&state).unwrap();

        // Now has_active_run should return true
        assert!(sm.has_active_run().unwrap(), "Should detect active run");

        // Clear and verify
        sm.clear_current().unwrap();
        assert!(
            !sm.has_active_run().unwrap(),
            "Should have no active run after clear"
        );
    }

    /// Tests that clean command can list and delete specs from config directory.
    /// This verifies that files in ~/.config/autom8/<project-name>/spec/ can be:
    /// 1. Listed via list_specs()
    /// 2. Deleted via standard fs::remove_file()
    #[test]
    fn test_clean_command_operates_on_config_directory() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        // Create spec directory structure
        let spec_dir = temp_dir.path().join("spec");
        std::fs::create_dir_all(&spec_dir).unwrap();

        // Create test spec files
        let spec1 = spec_dir.join("spec-feature1.json");
        let spec2 = spec_dir.join("spec-feature2.json");
        std::fs::write(&spec1, r#"{"project": "test1"}"#).unwrap();
        std::fs::write(&spec2, r#"{"project": "test2"}"#).unwrap();

        // Verify list_specs finds the files
        let specs = sm.list_specs().unwrap();
        assert_eq!(specs.len(), 2, "Should find 2 spec files");

        // Verify spec_dir points to config directory structure
        assert_eq!(sm.spec_dir(), spec_dir);

        // Clean (delete) the files - simulating what clean_spec_files does
        for spec_path in &specs {
            std::fs::remove_file(spec_path).unwrap();
        }

        // Verify files are gone
        let specs_after = sm.list_specs().unwrap();
        assert!(specs_after.is_empty(), "All spec files should be deleted");
        assert!(!spec1.exists(), "spec-feature1.json should be deleted");
        assert!(!spec2.exists(), "spec-feature2.json should be deleted");
    }

    /// Tests that clean command no longer operates on legacy .autom8/ location.
    /// Spec files should ONLY be found in the config directory.
    #[test]
    fn test_clean_uses_config_directory_not_legacy_location() {
        let sm = StateManager::new().unwrap();
        let spec_dir = sm.spec_dir();

        // spec_dir should NOT point to .autom8/spec/ in current directory
        let path_str = spec_dir.to_string_lossy();
        assert!(
            !path_str.starts_with(".autom8/") && !path_str.contains("/.autom8/spec"),
            "spec_dir should not reference legacy .autom8/ location: got {}",
            path_str
        );

        // Should be in ~/.config/autom8/
        assert!(
            path_str.contains(".config/autom8/") || path_str.contains(".config\\autom8\\"),
            "spec_dir should be in config directory: got {}",
            path_str
        );
    }

    // ======================================================================
    // Tests for US-005: Config integration with RunState
    // ======================================================================

    #[test]
    fn test_run_state_new_has_no_config_by_default() {
        let state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        assert!(state.config.is_none());
    }

    #[test]
    fn test_run_state_new_with_config_stores_config() {
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
        assert!(state.config.is_some());
        assert_eq!(state.config.unwrap(), config);
    }

    #[test]
    fn test_run_state_from_spec_has_no_config_by_default() {
        let state = RunState::from_spec(
            PathBuf::from("spec-feature.md"),
            PathBuf::from("spec-feature.json"),
        );
        assert!(state.config.is_none());
    }

    #[test]
    fn test_run_state_from_spec_with_config_stores_config() {
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
        assert!(state.config.is_some());
        assert_eq!(state.config.unwrap(), config);
    }

    #[test]
    fn test_effective_config_returns_stored_config_when_present() {
        let config = Config {
            review: false,
            commit: false,
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
    fn test_effective_config_returns_default_when_no_config() {
        let state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        let effective = state.effective_config();
        // Default config has review, commit, pull_request enabled
        assert!(effective.review);
        assert!(effective.commit);
        assert!(effective.pull_request);
    }

    #[test]
    fn test_run_state_config_serialization_roundtrip() {
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

        // Serialize to JSON
        let json = serde_json::to_string(&state).unwrap();

        // Deserialize back
        let deserialized: RunState = serde_json::from_str(&json).unwrap();

        // Verify config is preserved
        assert!(deserialized.config.is_some());
        assert_eq!(deserialized.config.unwrap(), config);
    }

    #[test]
    fn test_run_state_backwards_compatible_without_config_field() {
        // Simulate loading a legacy state.json that doesn't have the config field
        let legacy_json = r#"{
            "run_id": "test-123",
            "status": "running",
            "machine_state": "initializing",
            "spec_json_path": "test.json",
            "branch": "test-branch",
            "current_story": null,
            "iteration": 0,
            "review_iteration": 0,
            "started_at": "2024-01-01T00:00:00Z",
            "finished_at": null,
            "iterations": []
        }"#;

        let state: RunState = serde_json::from_str(legacy_json).unwrap();

        // Config should default to None
        assert!(state.config.is_none());

        // effective_config() should return default (all true)
        let effective = state.effective_config();
        assert!(effective.review);
        assert!(effective.commit);
        assert!(effective.pull_request);
    }

    #[test]
    fn test_run_state_config_with_review_false() {
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
        assert!(!effective.review);
        assert!(effective.commit);
        assert!(effective.pull_request);
    }

    #[test]
    fn test_run_state_config_with_commit_false() {
        let config = Config {
            review: true,
            commit: false,
            pull_request: false,
        };
        let state = RunState::new_with_config(
            PathBuf::from("test.json"),
            "test-branch".to_string(),
            config,
        );
        let effective = state.effective_config();
        assert!(effective.review);
        assert!(!effective.commit);
        assert!(!effective.pull_request);
    }

    #[test]
    fn test_run_state_config_with_pull_request_false() {
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
        assert!(effective.review);
        assert!(effective.commit);
        assert!(!effective.pull_request);
    }

    #[test]
    fn test_state_manager_preserves_config_on_save_and_load() {
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

        sm.save(&state).unwrap();

        let loaded = sm.load_current().unwrap().unwrap();
        assert!(loaded.config.is_some());
        assert_eq!(loaded.config.unwrap(), config);
    }

    // ======================================================================
    // Tests for US-003: ProjectKnowledge integration with RunState
    // ======================================================================

    #[test]
    fn test_run_state_has_knowledge_field() {
        let state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        // Knowledge should be initialized as default (empty)
        assert!(state.knowledge.files.is_empty());
        assert!(state.knowledge.decisions.is_empty());
        assert!(state.knowledge.patterns.is_empty());
        assert!(state.knowledge.story_changes.is_empty());
        assert!(state.knowledge.baseline_commit.is_none());
    }

    #[test]
    fn test_run_state_has_pre_story_commit_field() {
        let state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        assert!(state.pre_story_commit.is_none());
    }

    #[test]
    fn test_run_state_new_with_config_has_knowledge() {
        let config = Config {
            review: true,
            commit: true,
            pull_request: true,
        };
        let state = RunState::new_with_config(
            PathBuf::from("test.json"),
            "test-branch".to_string(),
            config,
        );
        assert!(state.knowledge.files.is_empty());
        assert!(state.pre_story_commit.is_none());
    }

    #[test]
    fn test_run_state_from_spec_has_knowledge() {
        let state = RunState::from_spec(
            PathBuf::from("spec-feature.md"),
            PathBuf::from("spec-feature.json"),
        );
        assert!(state.knowledge.files.is_empty());
        assert!(state.pre_story_commit.is_none());
    }

    #[test]
    fn test_run_state_from_spec_with_config_has_knowledge() {
        let config = Config {
            review: true,
            commit: true,
            pull_request: true,
        };
        let state = RunState::from_spec_with_config(
            PathBuf::from("spec-feature.md"),
            PathBuf::from("spec-feature.json"),
            config,
        );
        assert!(state.knowledge.files.is_empty());
        assert!(state.pre_story_commit.is_none());
    }

    #[test]
    fn test_run_state_backwards_compatible_without_knowledge_field() {
        // Simulate loading a legacy state.json that doesn't have knowledge or pre_story_commit fields
        let legacy_json = r#"{
            "run_id": "test-123",
            "status": "running",
            "machine_state": "initializing",
            "spec_json_path": "test.json",
            "branch": "test-branch",
            "current_story": null,
            "iteration": 0,
            "review_iteration": 0,
            "started_at": "2024-01-01T00:00:00Z",
            "finished_at": null,
            "iterations": []
        }"#;

        let state: RunState = serde_json::from_str(legacy_json).unwrap();

        // Knowledge should default to empty
        assert!(state.knowledge.files.is_empty());
        assert!(state.knowledge.decisions.is_empty());
        assert!(state.knowledge.patterns.is_empty());
        assert!(state.knowledge.story_changes.is_empty());
        assert!(state.knowledge.baseline_commit.is_none());

        // pre_story_commit should default to None
        assert!(state.pre_story_commit.is_none());
    }

    #[test]
    fn test_run_state_knowledge_serialization_roundtrip() {
        use crate::knowledge::{Decision, Pattern};

        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        // Add some knowledge
        state.knowledge.baseline_commit = Some("abc123".to_string());
        state.knowledge.decisions.push(Decision {
            story_id: "US-001".to_string(),
            topic: "Architecture".to_string(),
            choice: "Use modules".to_string(),
            rationale: "Better organization".to_string(),
        });
        state.knowledge.patterns.push(Pattern {
            story_id: "US-001".to_string(),
            description: "Use Result for errors".to_string(),
            example_file: Some(PathBuf::from("src/lib.rs")),
        });
        state.pre_story_commit = Some("def456".to_string());

        // Serialize to JSON
        let json = serde_json::to_string(&state).unwrap();

        // Deserialize back
        let deserialized: RunState = serde_json::from_str(&json).unwrap();

        // Verify knowledge is preserved
        assert_eq!(
            deserialized.knowledge.baseline_commit,
            Some("abc123".to_string())
        );
        assert_eq!(deserialized.knowledge.decisions.len(), 1);
        assert_eq!(deserialized.knowledge.decisions[0].story_id, "US-001");
        assert_eq!(deserialized.knowledge.patterns.len(), 1);
        assert_eq!(deserialized.pre_story_commit, Some("def456".to_string()));
    }

    #[test]
    fn test_capture_pre_story_state_in_git_repo() {
        // This test runs in a git repo, so capture_pre_story_state should set pre_story_commit
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        assert!(state.pre_story_commit.is_none());

        state.capture_pre_story_state();

        // In a git repo, this should capture the HEAD commit
        assert!(state.pre_story_commit.is_some());
        let commit = state.pre_story_commit.unwrap();
        // Should be a 40-character hex hash
        assert_eq!(commit.len(), 40);
        assert!(commit.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_record_story_changes_creates_story_changes_entry() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        // Record changes without pre_story_commit (simulates non-git or no prior capture)
        state.record_story_changes("US-001", None);

        assert_eq!(state.knowledge.story_changes.len(), 1);
        let changes = &state.knowledge.story_changes[0];
        assert_eq!(changes.story_id, "US-001");
        assert!(changes.files_created.is_empty());
        assert!(changes.files_modified.is_empty());
        assert!(changes.files_deleted.is_empty());
        assert!(changes.commit_hash.is_none());
    }

    #[test]
    fn test_record_story_changes_with_commit_hash() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        state.record_story_changes("US-001", Some("abc1234".to_string()));

        assert_eq!(state.knowledge.story_changes.len(), 1);
        let changes = &state.knowledge.story_changes[0];
        assert_eq!(changes.commit_hash, Some("abc1234".to_string()));
    }

    #[test]
    fn test_record_story_changes_clears_pre_story_commit() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.pre_story_commit = Some("abc123".to_string());

        state.record_story_changes("US-001", None);

        // pre_story_commit should be cleared after recording
        assert!(state.pre_story_commit.is_none());
    }

    #[test]
    fn test_record_story_changes_multiple_stories() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        state.record_story_changes("US-001", Some("commit1".to_string()));
        state.record_story_changes("US-002", Some("commit2".to_string()));
        state.record_story_changes("US-003", None);

        assert_eq!(state.knowledge.story_changes.len(), 3);
        assert_eq!(state.knowledge.story_changes[0].story_id, "US-001");
        assert_eq!(state.knowledge.story_changes[1].story_id, "US-002");
        assert_eq!(state.knowledge.story_changes[2].story_id, "US-003");
    }

    #[test]
    fn test_state_manager_preserves_knowledge_on_save_and_load() {
        use crate::knowledge::Decision;

        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.knowledge.baseline_commit = Some("baseline123".to_string());
        state.knowledge.decisions.push(Decision {
            story_id: "US-001".to_string(),
            topic: "Test".to_string(),
            choice: "Option A".to_string(),
            rationale: "Because".to_string(),
        });
        state.pre_story_commit = Some("pre123".to_string());

        sm.save(&state).unwrap();

        let loaded = sm.load_current().unwrap().unwrap();
        assert_eq!(
            loaded.knowledge.baseline_commit,
            Some("baseline123".to_string())
        );
        assert_eq!(loaded.knowledge.decisions.len(), 1);
        assert_eq!(loaded.pre_story_commit, Some("pre123".to_string()));
    }

    #[test]
    fn test_capture_and_record_workflow() {
        // Test the typical workflow: capture -> implement -> record
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        // Step 1: Capture pre-story state
        state.capture_pre_story_state();

        // In a git repo, pre_story_commit should be set
        let captured_commit = state.pre_story_commit.clone();
        assert!(captured_commit.is_some());

        // Step 2: (Implementation happens here - we simulate no changes for test)

        // Step 3: Record story changes
        state.record_story_changes("US-001", Some("new_commit_hash".to_string()));

        // Verify: pre_story_commit cleared, story_changes recorded
        assert!(state.pre_story_commit.is_none());
        assert_eq!(state.knowledge.story_changes.len(), 1);
        assert_eq!(
            state.knowledge.story_changes[0].commit_hash,
            Some("new_commit_hash".to_string())
        );
    }

    // ======================================================================
    // Tests for US-006: capture_story_knowledge
    // ======================================================================

    #[test]
    fn test_capture_story_knowledge_extracts_files_context() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        let agent_output = r#"I implemented the feature.

<files-context>
src/main.rs | Application entry point | [main, run]
src/lib.rs | Library exports | [Config, Runner]
</files-context>

Done!"#;

        state.capture_story_knowledge("US-001", agent_output, None);

        // Should have created story changes
        assert_eq!(state.knowledge.story_changes.len(), 1);
        assert_eq!(state.knowledge.story_changes[0].story_id, "US-001");

        // Files should be merged into the registry
        assert_eq!(state.knowledge.files.len(), 2);
        let main_info = state
            .knowledge
            .files
            .get(&PathBuf::from("src/main.rs"))
            .unwrap();
        assert_eq!(main_info.purpose, "Application entry point");
        assert_eq!(main_info.key_symbols, vec!["main", "run"]);
        assert_eq!(main_info.touched_by, vec!["US-001"]);

        let lib_info = state
            .knowledge
            .files
            .get(&PathBuf::from("src/lib.rs"))
            .unwrap();
        assert_eq!(lib_info.purpose, "Library exports");
        assert_eq!(lib_info.key_symbols, vec!["Config", "Runner"]);
    }

    #[test]
    fn test_capture_story_knowledge_extracts_decisions() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        let agent_output = r#"I made some decisions.

<decisions>
Error handling | thiserror crate | Provides clean derive macros
Database | SQLite | Embedded, no setup required
</decisions>

Done!"#;

        state.capture_story_knowledge("US-001", agent_output, None);

        // Should have extracted decisions
        assert_eq!(state.knowledge.decisions.len(), 2);

        assert_eq!(state.knowledge.decisions[0].story_id, "US-001");
        assert_eq!(state.knowledge.decisions[0].topic, "Error handling");
        assert_eq!(state.knowledge.decisions[0].choice, "thiserror crate");
        assert_eq!(
            state.knowledge.decisions[0].rationale,
            "Provides clean derive macros"
        );

        assert_eq!(state.knowledge.decisions[1].story_id, "US-001");
        assert_eq!(state.knowledge.decisions[1].topic, "Database");
        assert_eq!(state.knowledge.decisions[1].choice, "SQLite");
    }

    #[test]
    fn test_capture_story_knowledge_extracts_patterns() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        let agent_output = r#"I established some patterns.

<patterns>
Use Result<T, Error> for all fallible operations
Prefer explicit error types over Box<dyn Error>
</patterns>

Done!"#;

        state.capture_story_knowledge("US-001", agent_output, None);

        // Should have extracted patterns
        assert_eq!(state.knowledge.patterns.len(), 2);

        assert_eq!(state.knowledge.patterns[0].story_id, "US-001");
        assert_eq!(
            state.knowledge.patterns[0].description,
            "Use Result<T, Error> for all fallible operations"
        );

        assert_eq!(state.knowledge.patterns[1].story_id, "US-001");
        assert_eq!(
            state.knowledge.patterns[1].description,
            "Prefer explicit error types over Box<dyn Error>"
        );
    }

    #[test]
    fn test_capture_story_knowledge_with_empty_output() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        // Empty output should still create a story changes entry
        state.capture_story_knowledge("US-001", "", None);

        assert_eq!(state.knowledge.story_changes.len(), 1);
        assert_eq!(state.knowledge.story_changes[0].story_id, "US-001");
        assert!(state.knowledge.story_changes[0].files_created.is_empty());
        assert!(state.knowledge.story_changes[0].files_modified.is_empty());
        assert!(state.knowledge.decisions.is_empty());
        assert!(state.knowledge.patterns.is_empty());
    }

    #[test]
    fn test_capture_story_knowledge_with_commit_hash() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        state.capture_story_knowledge("US-001", "Some output", Some("abc123def".to_string()));

        assert_eq!(state.knowledge.story_changes.len(), 1);
        assert_eq!(
            state.knowledge.story_changes[0].commit_hash,
            Some("abc123def".to_string())
        );
    }

    #[test]
    fn test_capture_story_knowledge_clears_pre_story_commit() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.pre_story_commit = Some("old_commit".to_string());

        state.capture_story_knowledge("US-001", "", None);

        // pre_story_commit should be cleared after capture
        assert!(state.pre_story_commit.is_none());
    }

    #[test]
    fn test_capture_story_knowledge_merges_files_across_stories() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        // First story touches main.rs
        let output1 = r#"<files-context>
src/main.rs | Entry point | [main]
</files-context>"#;
        state.capture_story_knowledge("US-001", output1, None);

        // Second story also touches main.rs and adds lib.rs
        let output2 = r#"<files-context>
src/main.rs | Entry point with new feature | [main, new_feature]
src/lib.rs | Library | [lib_fn]
</files-context>"#;
        state.capture_story_knowledge("US-002", output2, None);

        // Should have 2 files in registry
        assert_eq!(state.knowledge.files.len(), 2);

        // main.rs should have both stories in touched_by
        let main_info = state
            .knowledge
            .files
            .get(&PathBuf::from("src/main.rs"))
            .unwrap();
        assert_eq!(main_info.touched_by, vec!["US-001", "US-002"]);

        // Symbols should be merged (no duplicates)
        assert!(main_info.key_symbols.contains(&"main".to_string()));
        assert!(main_info.key_symbols.contains(&"new_feature".to_string()));

        // Purpose should be updated to latest
        assert_eq!(main_info.purpose, "Entry point with new feature");
    }

    #[test]
    fn test_capture_story_knowledge_full_workflow() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        let agent_output = r#"I implemented the authentication feature.

<work-summary>
Files changed: src/auth.rs, src/main.rs. Added JWT authentication module.
</work-summary>

<files-context>
src/auth.rs | JWT authentication module | [authenticate, verify_token]
src/main.rs | Application entry | [main]
</files-context>

<decisions>
Auth method | JWT | Stateless, scalable, well-supported
</decisions>

<patterns>
Use Result<T, AuthError> for auth operations
</patterns>

Done!"#;

        state.capture_story_knowledge("US-001", agent_output, Some("commit123".to_string()));

        // Verify story changes
        assert_eq!(state.knowledge.story_changes.len(), 1);
        assert_eq!(state.knowledge.story_changes[0].story_id, "US-001");
        assert_eq!(
            state.knowledge.story_changes[0].commit_hash,
            Some("commit123".to_string())
        );

        // Verify files registry
        assert_eq!(state.knowledge.files.len(), 2);
        let auth_info = state
            .knowledge
            .files
            .get(&PathBuf::from("src/auth.rs"))
            .unwrap();
        assert_eq!(auth_info.purpose, "JWT authentication module");

        // Verify decisions
        assert_eq!(state.knowledge.decisions.len(), 1);
        assert_eq!(state.knowledge.decisions[0].topic, "Auth method");

        // Verify patterns
        assert_eq!(state.knowledge.patterns.len(), 1);
        assert!(state.knowledge.patterns[0]
            .description
            .contains("AuthError"));
    }

    #[test]
    fn test_capture_story_knowledge_graceful_with_no_context() {
        // Test graceful degradation when agent provides no structured context
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        let agent_output = r#"I implemented the feature but didn't provide any structured context.
Just plain text output without any special tags."#;

        state.capture_story_knowledge("US-001", agent_output, None);

        // Should still create story changes (empty)
        assert_eq!(state.knowledge.story_changes.len(), 1);
        assert_eq!(state.knowledge.story_changes[0].story_id, "US-001");

        // No files, decisions, or patterns
        assert!(state.knowledge.files.is_empty());
        assert!(state.knowledge.decisions.is_empty());
        assert!(state.knowledge.patterns.is_empty());
    }

    #[test]
    fn test_capture_story_knowledge_multiple_stories_accumulate() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        // Story 1
        let output1 = r#"<decisions>
Database | SQLite | Simple
</decisions>"#;
        state.capture_story_knowledge("US-001", output1, None);

        // Story 2
        let output2 = r#"<decisions>
Cache | Redis | Fast
</decisions>"#;
        state.capture_story_knowledge("US-002", output2, None);

        // Story 3
        let output3 = r#"<patterns>
Use connection pooling
</patterns>"#;
        state.capture_story_knowledge("US-003", output3, None);

        // Should have accumulated all knowledge
        assert_eq!(state.knowledge.story_changes.len(), 3);
        assert_eq!(state.knowledge.decisions.len(), 2);
        assert_eq!(state.knowledge.patterns.len(), 1);
    }

    // ======================================================================
    // Tests for US-009: Non-git project support
    // ======================================================================

    /// Integration test that verifies the system works in a non-git directory.
    /// This test:
    /// 1. Creates a temporary directory (not a git repo)
    /// 2. Changes to that directory
    /// 3. Verifies all git-dependent operations work without errors
    /// 4. Restores the original directory
    #[test]
    fn test_system_works_in_non_git_directory() {
        use std::env;

        // Acquire lock to prevent other tests from changing cwd concurrently
        let _lock = CWD_MUTEX.lock().unwrap();

        let temp_dir = TempDir::new().unwrap();
        let original_dir = env::current_dir().unwrap();

        // Change to the temp directory (not a git repo)
        env::set_current_dir(temp_dir.path()).unwrap();

        // Verify we're not in a git repo
        assert!(
            !git::is_git_repo(),
            "Temp directory should not be a git repo"
        );

        // Test 1: capture_pre_story_state should be a no-op
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.capture_pre_story_state();
        assert!(
            state.pre_story_commit.is_none(),
            "pre_story_commit should be None in non-git directory"
        );

        // Test 2: record_story_changes should work (creates empty changes)
        state.record_story_changes("US-001", Some("fake-commit".to_string()));
        assert_eq!(state.knowledge.story_changes.len(), 1);
        assert!(state.knowledge.story_changes[0].files_created.is_empty());
        assert!(state.knowledge.story_changes[0].files_modified.is_empty());
        assert!(state.knowledge.story_changes[0].files_deleted.is_empty());

        // Test 3: capture_story_knowledge should work with agent context only
        let agent_output = r#"<files-context>
src/main.rs | Application entry | [main]
</files-context>
<decisions>
Framework | Actix | Good performance
</decisions>
<patterns>
Use async/await for all IO
</patterns>"#;
        state.capture_story_knowledge("US-002", agent_output, None);

        // Should have used agent context
        assert_eq!(state.knowledge.story_changes.len(), 2);
        assert_eq!(state.knowledge.files.len(), 1);
        assert_eq!(state.knowledge.decisions.len(), 1);
        assert_eq!(state.knowledge.patterns.len(), 1);

        // The file should be recorded from agent context (as modified since we can't determine)
        let changes = &state.knowledge.story_changes[1];
        assert_eq!(changes.files_modified.len(), 1);
        assert_eq!(changes.files_modified[0].path, PathBuf::from("src/main.rs"));

        // Test 4: git diff functions should return empty results
        let diff = git::get_diff_since("any-commit").unwrap();
        assert!(
            diff.is_empty(),
            "get_diff_since should return empty in non-git"
        );

        let uncommitted = git::get_uncommitted_changes().unwrap();
        assert!(
            uncommitted.is_empty(),
            "get_uncommitted_changes should return empty in non-git"
        );

        let new_files = git::get_new_files_since("any-commit").unwrap();
        assert!(
            new_files.is_empty(),
            "get_new_files_since should return empty in non-git"
        );

        // Restore original directory
        env::set_current_dir(original_dir).unwrap();
    }

    #[test]
    fn test_capture_pre_story_state_no_op_in_non_git() {
        use std::env;

        // Acquire lock to prevent other tests from changing cwd concurrently
        let _lock = CWD_MUTEX.lock().unwrap();

        let temp_dir = TempDir::new().unwrap();
        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(temp_dir.path()).unwrap();

        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.capture_pre_story_state();

        // Should remain None - no error, just a no-op
        assert!(state.pre_story_commit.is_none());

        env::set_current_dir(original_dir).unwrap();
    }

    #[test]
    fn test_capture_story_knowledge_uses_agent_context_only_in_non_git() {
        use std::env;

        // Acquire lock to prevent other tests from changing cwd concurrently
        let _lock = CWD_MUTEX.lock().unwrap();

        let temp_dir = TempDir::new().unwrap();
        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(temp_dir.path()).unwrap();

        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        // Even with pre_story_commit set (which shouldn't happen, but test defensively)
        state.pre_story_commit = Some("fake-commit".to_string());

        let agent_output = r#"<files-context>
src/lib.rs | Library module | [Config]
</files-context>"#;
        state.capture_story_knowledge("US-001", agent_output, None);

        // Should use agent context only (git operations return empty)
        assert_eq!(state.knowledge.story_changes.len(), 1);
        assert_eq!(state.knowledge.files.len(), 1);

        // File comes from agent context, recorded as modified
        let changes = &state.knowledge.story_changes[0];
        assert_eq!(changes.files_modified.len(), 1);
        assert_eq!(changes.files_modified[0].path, PathBuf::from("src/lib.rs"));

        // pre_story_commit should be cleared
        assert!(state.pre_story_commit.is_none());

        env::set_current_dir(original_dir).unwrap();
    }

    // ======================================================================
    // Tests for US-010: Filter changes to only autom8-related files
    // ======================================================================

    #[test]
    fn test_capture_pre_story_state_sets_baseline_commit_on_first_call() {
        // This test runs in a git repo
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        // Initially baseline_commit should be None
        assert!(state.knowledge.baseline_commit.is_none());

        // First call should set both pre_story_commit and baseline_commit
        state.capture_pre_story_state();

        assert!(state.pre_story_commit.is_some());
        assert!(state.knowledge.baseline_commit.is_some());

        // They should be the same on first call
        assert_eq!(state.pre_story_commit, state.knowledge.baseline_commit);
    }

    #[test]
    fn test_capture_pre_story_state_preserves_baseline_on_subsequent_calls() {
        // This test runs in a git repo
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        // First call
        state.capture_pre_story_state();
        let baseline = state.knowledge.baseline_commit.clone();
        assert!(baseline.is_some());

        // Clear pre_story_commit to simulate finishing a story
        state.pre_story_commit = None;

        // Second call should set pre_story_commit but not change baseline_commit
        state.capture_pre_story_state();

        assert!(state.pre_story_commit.is_some());
        assert_eq!(state.knowledge.baseline_commit, baseline);
    }

    #[test]
    fn test_capture_pre_story_state_baseline_persists_through_multiple_stories() {
        // This test runs in a git repo
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        // Story 1 start
        state.capture_pre_story_state();
        let baseline = state.knowledge.baseline_commit.clone();
        assert!(baseline.is_some());

        // Story 1 complete
        state.capture_story_knowledge("US-001", "", None);

        // Story 2 start
        state.capture_pre_story_state();

        // Baseline should still be the original
        assert_eq!(state.knowledge.baseline_commit, baseline);
        assert!(state.pre_story_commit.is_some());
    }

    #[test]
    fn test_filter_our_changes_integration_with_capture_story_knowledge() {
        // This test verifies that capture_story_knowledge properly uses filter_our_changes
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        // Simulate first story that touched src/a.rs
        state.knowledge.story_changes.push(crate::knowledge::StoryChanges {
            story_id: "US-001".to_string(),
            files_created: vec![crate::knowledge::FileChange {
                path: PathBuf::from("src/a.rs"),
                additions: 100,
                deletions: 0,
                purpose: None,
                key_symbols: vec![],
            }],
            files_modified: vec![],
            files_deleted: vec![],
            commit_hash: None,
        });

        // Verify our_files returns the file
        let our_files = state.knowledge.our_files();
        assert!(our_files.contains(&PathBuf::from("src/a.rs")));

        // Verify filter works as expected
        let entries = vec![
            git::DiffEntry {
                path: PathBuf::from("src/a.rs"),
                additions: 10,
                deletions: 5,
                status: git::DiffStatus::Modified,
            },
            git::DiffEntry {
                path: PathBuf::from("external.txt"),
                additions: 20,
                deletions: 0,
                status: git::DiffStatus::Modified,
            },
        ];

        let filtered = state.knowledge.filter_our_changes(&entries);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].path, PathBuf::from("src/a.rs"));
    }

    #[test]
    fn test_baseline_commit_not_set_in_non_git_directory() {
        use std::env;

        // Acquire lock to prevent other tests from changing cwd concurrently
        let _lock = CWD_MUTEX.lock().unwrap();

        let temp_dir = TempDir::new().unwrap();
        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(temp_dir.path()).unwrap();

        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.capture_pre_story_state();

        // In non-git directory, neither should be set
        assert!(state.pre_story_commit.is_none());
        assert!(state.knowledge.baseline_commit.is_none());

        env::set_current_dir(original_dir).unwrap();
    }
}
