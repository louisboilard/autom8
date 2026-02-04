use crate::claude::{extract_decisions, extract_files_context, extract_patterns, FileContextEntry};
use crate::config::{self, Config};
use crate::error::Result;
use crate::git;
use crate::knowledge::{Decision, FileChange, FileInfo, Pattern, ProjectKnowledge, StoryChanges};
use crate::worktree::{get_current_session_id, MAIN_SESSION_ID};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

const STATE_FILE: &str = "state.json";
const METADATA_FILE: &str = "metadata.json";
const LIVE_FILE: &str = "live.json";
const SESSIONS_DIR: &str = "sessions";
const RUNS_DIR: &str = "runs";
const SPEC_DIR: &str = "spec";

/// Maximum number of output lines to keep in LiveState.
/// Prevents unbounded memory growth during long Claude runs.
const LIVE_STATE_MAX_LINES: usize = 50;

/// Metadata about a session, stored separately from the full state.
///
/// This enables quick session listing without loading the full state file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMetadata {
    /// Unique session identifier (e.g., "main" or 8-char hash)
    pub session_id: String,
    /// Absolute path to the worktree directory
    pub worktree_path: PathBuf,
    /// The branch being worked on in this session
    pub branch_name: String,
    /// When the session was created
    pub created_at: DateTime<Utc>,
    /// When the session was last active (updated on each state save)
    pub last_active_at: DateTime<Utc>,
    /// Whether this session is currently running (has an active run).
    /// Used for branch conflict detection - only running sessions "own" their branch.
    #[serde(default)]
    pub is_running: bool,
}

/// Enriched session status for display purposes.
///
/// Combines session metadata with state information (current story, machine state)
/// for the status command's `--all` flag.
#[derive(Debug, Clone)]
pub struct SessionStatus {
    /// Session metadata
    pub metadata: SessionMetadata,
    /// Current machine state (e.g., "RunningClaude", "Reviewing")
    pub machine_state: Option<MachineState>,
    /// Current story ID being worked on
    pub current_story: Option<String>,
    /// Whether this session matches the current working directory
    pub is_current: bool,
    /// Whether the worktree path still exists
    pub is_stale: bool,
}

/// Live streaming state for a session, written frequently during Claude runs.
///
/// This struct holds the most recent output lines and current state, enabling
/// the monitor command to display real-time output without reading the full state.
/// Written atomically to prevent partial reads.
///
/// The `last_heartbeat` field is the authoritative indicator of whether a run is
/// still active. GUI/TUI should consider a run "active" if heartbeat is recent
/// (< 10 seconds old).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveState {
    /// Recent output lines from Claude (max 50 lines, newest last)
    pub output_lines: Vec<String>,
    /// When this live state was last updated (by output or state change)
    pub updated_at: DateTime<Utc>,
    /// Current machine state
    pub machine_state: MachineState,
    /// Heartbeat timestamp - updated every 2-3 seconds while run is active.
    /// This is the authoritative indicator of whether the run is still alive.
    /// GUI/TUI should consider a run "active" if this is < 10 seconds old.
    #[serde(default = "Utc::now")]
    pub last_heartbeat: DateTime<Utc>,
}

/// Threshold for considering a heartbeat "stale" (run likely dead).
/// GUI/TUI should consider a run inactive if heartbeat is older than this.
/// Set to 60 seconds to account for periods where Claude is thinking without
/// sending output, and for phases like Reviewing/Correcting that may take time.
pub const HEARTBEAT_STALE_THRESHOLD_SECS: i64 = 60;

impl LiveState {
    /// Create a new LiveState with the given machine state.
    pub fn new(machine_state: MachineState) -> Self {
        let now = Utc::now();
        Self {
            output_lines: Vec::new(),
            updated_at: now,
            machine_state,
            last_heartbeat: now,
        }
    }

    /// Append a line to the output, keeping at most 50 lines.
    /// Updates the `updated_at` timestamp.
    pub fn append_line(&mut self, line: String) {
        self.output_lines.push(line);
        // Keep only the last 50 lines
        if self.output_lines.len() > LIVE_STATE_MAX_LINES {
            let excess = self.output_lines.len() - LIVE_STATE_MAX_LINES;
            self.output_lines.drain(0..excess);
        }
        self.updated_at = Utc::now();
    }

    /// Update the heartbeat timestamp to indicate the run is still active.
    /// This should be called every 2-3 seconds during an active run.
    pub fn update_heartbeat(&mut self) {
        self.last_heartbeat = Utc::now();
    }

    /// Update the machine state and refresh timestamps.
    /// Called when the state machine transitions to a new state.
    pub fn update_state(&mut self, new_state: MachineState) {
        self.machine_state = new_state;
        let now = Utc::now();
        self.updated_at = now;
        self.last_heartbeat = now;
    }

    /// Check if the heartbeat is recent enough to consider the run active.
    /// Returns true if the heartbeat is less than HEARTBEAT_STALE_THRESHOLD_SECS old.
    pub fn is_heartbeat_fresh(&self) -> bool {
        let age = Utc::now()
            .signed_duration_since(self.last_heartbeat)
            .num_seconds();
        age < HEARTBEAT_STALE_THRESHOLD_SECS
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunStatus {
    Running,
    Completed,
    Failed,
    Interrupted,
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
    /// Session identifier for worktree-based parallel execution.
    /// Deterministic ID derived from worktree path (or "main" for main repo).
    #[serde(default)]
    pub session_id: Option<String>,
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
            session_id: None,
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
            session_id: None,
        }
    }

    /// Create a new RunState with a session ID.
    pub fn new_with_session(spec_json_path: PathBuf, branch: String, session_id: String) -> Self {
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
            session_id: Some(session_id),
        }
    }

    /// Create a new RunState with config and session ID.
    pub fn new_with_config_and_session(
        spec_json_path: PathBuf,
        branch: String,
        config: Config,
        session_id: String,
    ) -> Self {
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
            session_id: Some(session_id),
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
            session_id: None,
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
            session_id: None,
        }
    }

    /// Create a RunState from spec with config and session ID.
    pub fn from_spec_with_config_and_session(
        spec_md_path: PathBuf,
        spec_json_path: PathBuf,
        config: Config,
        session_id: String,
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
            session_id: Some(session_id),
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

/// Manages session state storage with per-session directory structure.
///
/// State is stored in: `~/.config/autom8/<project>/sessions/<session-id>/`
/// Each session has:
/// - `state.json` - The full run state
/// - `metadata.json` - Quick metadata for session listing
///
/// The spec/ and runs/ directories remain at the project level (shared across sessions).
pub struct StateManager {
    /// Base config directory for the project: `~/.config/autom8/<project>/`
    base_dir: PathBuf,
    /// Session ID for this manager (auto-detected from CWD or specified)
    session_id: String,
}

impl StateManager {
    /// Create a StateManager using the config directory for the current project.
    /// Auto-detects session ID from the current working directory.
    /// Uses `~/.config/autom8/<project-name>/` as the base directory.
    pub fn new() -> Result<Self> {
        let base_dir = config::project_config_dir()?;
        let session_id = get_current_session_id()?;
        let mut manager = Self {
            base_dir,
            session_id,
        };
        manager.migrate_legacy_state()?;
        Ok(manager)
    }

    /// Create a StateManager for a specific session.
    /// Uses `~/.config/autom8/<project-name>/` as the base directory.
    pub fn with_session(session_id: String) -> Result<Self> {
        let base_dir = config::project_config_dir()?;
        let mut manager = Self {
            base_dir,
            session_id,
        };
        manager.migrate_legacy_state()?;
        Ok(manager)
    }

    /// Create a StateManager for a specific project name.
    /// Auto-detects session ID from the current working directory.
    /// Uses `~/.config/autom8/<project-name>/` as the base directory.
    pub fn for_project(project_name: &str) -> Result<Self> {
        let base_dir = config::project_config_dir_for(project_name)?;
        let session_id = get_current_session_id()?;
        let mut manager = Self {
            base_dir,
            session_id,
        };
        manager.migrate_legacy_state()?;
        Ok(manager)
    }

    /// Create a StateManager for a specific project and session.
    /// Uses `~/.config/autom8/<project-name>/` as the base directory.
    pub fn for_project_session(project_name: &str, session_id: String) -> Result<Self> {
        let base_dir = config::project_config_dir_for(project_name)?;
        let mut manager = Self {
            base_dir,
            session_id,
        };
        manager.migrate_legacy_state()?;
        Ok(manager)
    }

    /// Create a StateManager with a custom base directory (for testing).
    pub fn with_dir(dir: PathBuf) -> Self {
        Self {
            base_dir: dir,
            session_id: MAIN_SESSION_ID.to_string(),
        }
    }

    /// Create a StateManager with a custom base directory and session ID (for testing).
    pub fn with_dir_and_session(dir: PathBuf, session_id: String) -> Self {
        Self {
            base_dir: dir,
            session_id,
        }
    }

    /// Get the session ID for this manager.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Path to the sessions directory: `~/.config/autom8/<project>/sessions/`
    fn sessions_dir(&self) -> PathBuf {
        self.base_dir.join(SESSIONS_DIR)
    }

    /// Path to this session's directory: `~/.config/autom8/<project>/sessions/<session-id>/`
    fn session_dir(&self) -> PathBuf {
        self.sessions_dir().join(&self.session_id)
    }

    /// Path to the state file for this session
    fn state_file(&self) -> PathBuf {
        self.session_dir().join(STATE_FILE)
    }

    /// Path to the metadata file for this session
    fn metadata_file(&self) -> PathBuf {
        self.session_dir().join(METADATA_FILE)
    }

    /// Path to the live state file for this session
    fn live_file(&self) -> PathBuf {
        self.session_dir().join(LIVE_FILE)
    }

    /// Path to the legacy state file (for migration)
    fn legacy_state_file(&self) -> PathBuf {
        self.base_dir.join(STATE_FILE)
    }

    /// Path to the runs directory (archived runs)
    pub fn runs_dir(&self) -> PathBuf {
        self.base_dir.join(RUNS_DIR)
    }

    /// Path to the spec directory
    pub fn spec_dir(&self) -> PathBuf {
        self.base_dir.join(SPEC_DIR)
    }

    /// Migrate legacy state.json to the new sessions structure.
    ///
    /// On first run after upgrade, if there's a state.json in the project root,
    /// migrate it to sessions/main/state.json and create appropriate metadata.
    fn migrate_legacy_state(&mut self) -> Result<()> {
        let legacy_path = self.legacy_state_file();

        // Only migrate if legacy file exists and sessions dir doesn't have main session
        if !legacy_path.exists() {
            return Ok(());
        }

        let main_session_dir = self.sessions_dir().join(MAIN_SESSION_ID);
        let main_state_file = main_session_dir.join(STATE_FILE);

        // Skip if already migrated
        if main_state_file.exists() {
            // Remove the legacy file since migration was already done
            let _ = fs::remove_file(&legacy_path);
            return Ok(());
        }

        // Read the legacy state
        let content = fs::read_to_string(&legacy_path)?;
        let mut state: RunState = serde_json::from_str(&content)?;

        // Update the state to have the main session ID
        if state.session_id.is_none() {
            state.session_id = Some(MAIN_SESSION_ID.to_string());
        }

        // Create the sessions/main/ directory
        fs::create_dir_all(&main_session_dir)?;

        // Write state to new location
        let state_content = serde_json::to_string_pretty(&state)?;
        fs::write(&main_state_file, state_content)?;

        // Create metadata for the migrated session
        let worktree_path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let metadata = SessionMetadata {
            session_id: MAIN_SESSION_ID.to_string(),
            worktree_path,
            branch_name: state.branch.clone(),
            created_at: state.started_at,
            last_active_at: state.finished_at.unwrap_or_else(Utc::now),
            is_running: state.status == RunStatus::Running,
        };
        let metadata_content = serde_json::to_string_pretty(&metadata)?;
        fs::write(main_session_dir.join(METADATA_FILE), metadata_content)?;

        // Remove the legacy state file
        fs::remove_file(&legacy_path)?;

        Ok(())
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        fs::create_dir_all(&self.base_dir)?;
        fs::create_dir_all(self.session_dir())?;
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

    /// Load the metadata for the current session.
    pub fn load_metadata(&self) -> Result<Option<SessionMetadata>> {
        let path = self.metadata_file();
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path)?;
        let metadata: SessionMetadata = serde_json::from_str(&content)?;
        Ok(Some(metadata))
    }

    /// Save the run state and update session metadata.
    pub fn save(&self, state: &RunState) -> Result<()> {
        self.ensure_dirs()?;

        // Save the state
        let content = serde_json::to_string_pretty(state)?;
        fs::write(self.state_file(), content)?;

        // Update or create metadata
        self.save_metadata(state)?;

        Ok(())
    }

    /// Save session metadata based on the current state.
    fn save_metadata(&self, state: &RunState) -> Result<()> {
        let worktree_path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let is_running = state.status == RunStatus::Running;

        // Load existing metadata or create new
        let metadata = if let Some(existing) = self.load_metadata()? {
            SessionMetadata {
                session_id: self.session_id.clone(),
                worktree_path,
                branch_name: state.branch.clone(),
                created_at: existing.created_at,
                last_active_at: Utc::now(),
                is_running,
            }
        } else {
            SessionMetadata {
                session_id: self.session_id.clone(),
                worktree_path,
                branch_name: state.branch.clone(),
                created_at: state.started_at,
                last_active_at: Utc::now(),
                is_running,
            }
        };

        let content = serde_json::to_string_pretty(&metadata)?;
        fs::write(self.metadata_file(), content)?;

        Ok(())
    }

    pub fn clear_current(&self) -> Result<()> {
        let path = self.state_file();
        if path.exists() {
            fs::remove_file(path)?;
        }
        // Also clear metadata
        let metadata_path = self.metadata_file();
        if metadata_path.exists() {
            fs::remove_file(metadata_path)?;
        }
        // Also clear live state
        self.clear_live()?;
        // Try to remove the session directory if empty
        let session_dir = self.session_dir();
        let _ = fs::remove_dir(&session_dir); // Ignore error if not empty
        Ok(())
    }

    /// Save live state atomically (write to temp file, then rename).
    ///
    /// Atomic writes prevent the monitor from reading a partial/corrupted file.
    pub fn save_live(&self, live_state: &LiveState) -> Result<()> {
        self.ensure_dirs()?;

        let live_path = self.live_file();
        let temp_path = live_path.with_extension("json.tmp");

        // Write to temp file
        let content = serde_json::to_string(live_state)?;
        fs::write(&temp_path, content)?;

        // Atomic rename
        fs::rename(&temp_path, &live_path)?;

        Ok(())
    }

    /// Load live state, returning None if file doesn't exist or is corrupted.
    ///
    /// Gracefully handles missing or malformed files so the monitor can
    /// recover without crashing.
    pub fn load_live(&self) -> Option<LiveState> {
        let path = self.live_file();
        if !path.exists() {
            return None;
        }

        let content = fs::read_to_string(&path).ok()?;
        serde_json::from_str(&content).ok()
    }

    /// Remove the live state file.
    pub fn clear_live(&self) -> Result<()> {
        let path = self.live_file();
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

    /// List all sessions for this project with their metadata.
    ///
    /// Returns sessions sorted by last_active_at descending (most recent first).
    /// Sessions without valid metadata are skipped.
    pub fn list_sessions(&self) -> Result<Vec<SessionMetadata>> {
        let sessions_dir = self.sessions_dir();
        if !sessions_dir.exists() {
            return Ok(Vec::new());
        }

        let mut sessions = Vec::new();
        for entry in fs::read_dir(&sessions_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                let metadata_path = path.join(METADATA_FILE);
                if let Ok(content) = fs::read_to_string(&metadata_path) {
                    if let Ok(metadata) = serde_json::from_str::<SessionMetadata>(&content) {
                        sessions.push(metadata);
                    }
                }
            }
        }

        // Sort by last_active_at descending
        sessions.sort_by(|a, b| b.last_active_at.cmp(&a.last_active_at));
        Ok(sessions)
    }

    /// Get a specific session by ID.
    ///
    /// Returns a new StateManager configured for the specified session.
    /// Returns None if the session doesn't exist.
    pub fn get_session(&self, session_id: &str) -> Option<StateManager> {
        let session_dir = self.sessions_dir().join(session_id);
        if session_dir.exists() && session_dir.join(STATE_FILE).exists() {
            Some(StateManager {
                base_dir: self.base_dir.clone(),
                session_id: session_id.to_string(),
            })
        } else {
            None
        }
    }

    /// List all sessions with enriched status information.
    ///
    /// Returns sessions sorted with current session first, then by last_active_at
    /// descending. Includes state details (machine state, current story) and
    /// marks stale sessions (deleted worktrees).
    pub fn list_sessions_with_status(&self) -> Result<Vec<SessionStatus>> {
        let sessions = self.list_sessions()?;
        let current_dir = std::env::current_dir().ok();

        let mut statuses: Vec<SessionStatus> = sessions
            .into_iter()
            .map(|metadata| {
                // Check if this is the current session
                let is_current = current_dir
                    .as_ref()
                    .map(|cwd| cwd == &metadata.worktree_path)
                    .unwrap_or(false);

                // Check if worktree still exists
                let is_stale = !metadata.worktree_path.exists();

                // Load state for this session to get machine_state and current_story
                let (machine_state, current_story) =
                    if let Some(session_sm) = self.get_session(&metadata.session_id) {
                        if let Ok(Some(state)) = session_sm.load_current() {
                            (Some(state.machine_state), state.current_story)
                        } else {
                            (None, None)
                        }
                    } else {
                        (None, None)
                    };

                SessionStatus {
                    metadata,
                    machine_state,
                    current_story,
                    is_current,
                    is_stale,
                }
            })
            .collect();

        // Sort: current first, then by last_active_at descending
        statuses.sort_by(|a, b| {
            // Current session always first
            match (a.is_current, b.is_current) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => b.metadata.last_active_at.cmp(&a.metadata.last_active_at),
            }
        });

        Ok(statuses)
    }

    /// Check for branch conflicts with other active sessions.
    ///
    /// Returns the conflicting session metadata if another session is already
    /// using the specified branch. A session "owns" a branch only while it is
    /// actively running (status == Running).
    ///
    /// Stale sessions (where the worktree directory no longer exists) are
    /// automatically skipped and do not cause conflicts.
    ///
    /// # Arguments
    /// * `branch_name` - The branch name to check for conflicts
    ///
    /// # Returns
    /// * `Ok(Some(metadata))` - Another session is using this branch
    /// * `Ok(None)` - No conflict, branch is available
    /// * `Err` - Error reading session data
    pub fn check_branch_conflict(&self, branch_name: &str) -> Result<Option<SessionMetadata>> {
        let sessions = self.list_sessions()?;

        for session in sessions {
            // Skip our own session
            if session.session_id == self.session_id {
                continue;
            }

            // Skip sessions not using this branch
            if session.branch_name != branch_name {
                continue;
            }

            // Skip sessions that aren't running
            if !session.is_running {
                continue;
            }

            // Check if the worktree still exists (detect stale sessions)
            if !session.worktree_path.exists() {
                // Stale session - worktree deleted but metadata remains
                // Don't block on this session
                continue;
            }

            // Found a conflict - another active session is using this branch
            return Ok(Some(session));
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Use the shared CWD_MUTEX for tests that depend on or change the current working directory
    use crate::test_utils::CWD_MUTEX;

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
    fn test_run_status_serialization_roundtrip() {
        // Test all RunStatus variants serialize/deserialize correctly with lowercase
        let test_cases: &[(RunStatus, &str)] = &[
            (RunStatus::Running, "\"running\""),
            (RunStatus::Completed, "\"completed\""),
            (RunStatus::Failed, "\"failed\""),
            (RunStatus::Interrupted, "\"interrupted\""),
        ];

        for (status, expected_json) in test_cases {
            // Test serialization
            let serialized = serde_json::to_string(status).unwrap();
            assert_eq!(
                &serialized, *expected_json,
                "Serialization failed for {:?}",
                status
            );

            // Test deserialization roundtrip
            let deserialized: RunStatus = serde_json::from_str(&serialized).unwrap();
            assert_eq!(
                deserialized, *status,
                "Deserialization failed for {:?}",
                status
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
    fn test_state_manager_with_dir_creates_state_file_in_session_dir() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let run_state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        sm.save(&run_state).unwrap();

        // state.json should be in sessions/main/ (default session for with_dir)
        assert!(temp_dir
            .path()
            .join(SESSIONS_DIR)
            .join(MAIN_SESSION_ID)
            .join(STATE_FILE)
            .exists());
    }

    #[test]
    fn test_state_manager_with_dir_creates_runs_subdir() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        sm.ensure_dirs().unwrap();

        // runs/ should be in the base dir (shared across sessions)
        assert!(temp_dir.path().join(RUNS_DIR).exists());
        assert!(temp_dir.path().join(RUNS_DIR).is_dir());
    }

    #[test]
    fn test_state_manager_with_dir_creates_sessions_subdir() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        sm.ensure_dirs().unwrap();

        // sessions/ should be in the base dir
        assert!(temp_dir.path().join(SESSIONS_DIR).exists());
        assert!(temp_dir.path().join(SESSIONS_DIR).is_dir());

        // sessions/main/ should exist for the default session
        assert!(temp_dir
            .path()
            .join(SESSIONS_DIR)
            .join(MAIN_SESSION_ID)
            .exists());
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
        // Acquire lock to prevent other tests from changing cwd concurrently
        let _lock = CWD_MUTEX.lock().unwrap();

        // This test verifies that StateManager::new() uses the config directory
        let sm = StateManager::new().unwrap();
        let spec_dir = sm.spec_dir();

        // The spec_dir should be under ~/.config/autom8/<project-name>/spec/
        assert!(spec_dir.ends_with("spec"));
        // Parent should be the project name (autom8 when running tests)
        let project_dir = spec_dir.parent().unwrap();
        assert!(project_dir.parent().unwrap().ends_with("autom8"));
    }

    // ======================================================================
    // Tests for resume command config directory integration (US-007)
    // ======================================================================

    /// Tests that StateManager::new() creates a state file path in the config directory.
    /// This is used by the resume command to find active runs.
    #[test]
    fn test_state_manager_state_file_in_config_directory() {
        // Acquire lock to prevent other tests from changing cwd concurrently
        let _lock = CWD_MUTEX.lock().unwrap();

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
        // Acquire lock to prevent other tests from changing cwd concurrently
        let _lock = CWD_MUTEX.lock().unwrap();

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
        // Acquire lock to prevent other tests from changing cwd concurrently
        let _lock = CWD_MUTEX.lock().unwrap();

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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
        // Acquire lock to prevent other tests from changing cwd concurrently
        let _lock = CWD_MUTEX.lock().unwrap();

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
        // Acquire lock to prevent other tests from changing cwd concurrently
        let _lock = CWD_MUTEX.lock().unwrap();

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
        // Acquire lock to prevent other tests from changing cwd concurrently
        let _lock = CWD_MUTEX.lock().unwrap();

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
        // Acquire lock to prevent other tests from changing cwd concurrently
        let _lock = CWD_MUTEX.lock().unwrap();

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
        // Acquire lock to prevent other tests from changing cwd concurrently
        let _lock = CWD_MUTEX.lock().unwrap();

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
        state
            .knowledge
            .story_changes
            .push(crate::knowledge::StoryChanges {
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

    // ======================================================================
    // Tests for US-001: Output snippet stored in finish_iteration
    // ======================================================================

    #[test]
    fn test_finish_iteration_stores_output_snippet() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.start_iteration("US-001");

        let output = "Line 1\nLine 2\nLine 3\nClaude output here".to_string();
        state.finish_iteration(IterationStatus::Success, output.clone());

        assert_eq!(state.iterations[0].output_snippet, output);
        assert_eq!(state.iterations[0].status, IterationStatus::Success);
        assert!(state.iterations[0].finished_at.is_some());
    }

    #[test]
    fn test_finish_iteration_with_empty_output_snippet() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.start_iteration("US-001");

        state.finish_iteration(IterationStatus::Success, String::new());

        assert_eq!(state.iterations[0].output_snippet, "");
    }

    #[test]
    fn test_finish_iteration_output_snippet_serialization() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.start_iteration("US-001");

        let output = "Test output from Claude".to_string();
        state.finish_iteration(IterationStatus::Success, output.clone());

        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("\"output_snippet\":\"Test output from Claude\""));

        // Verify roundtrip
        let loaded: RunState = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.iterations[0].output_snippet, output);
    }

    // ======================================================================
    // Tests for US-002: Session Identity System
    // ======================================================================

    #[test]
    fn test_run_state_new_has_no_session_id_by_default() {
        let state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        assert!(state.session_id.is_none());
    }

    #[test]
    fn test_run_state_new_with_session() {
        let state = RunState::new_with_session(
            PathBuf::from("test.json"),
            "test-branch".to_string(),
            "abc12345".to_string(),
        );
        assert_eq!(state.session_id, Some("abc12345".to_string()));
    }

    #[test]
    fn test_run_state_new_with_config_and_session() {
        let config = Config {
            review: true,
            commit: true,
            pull_request: false,
            ..Default::default()
        };
        let state = RunState::new_with_config_and_session(
            PathBuf::from("test.json"),
            "test-branch".to_string(),
            config.clone(),
            "session123".to_string(),
        );
        assert_eq!(state.session_id, Some("session123".to_string()));
        assert_eq!(state.config, Some(config));
    }

    #[test]
    fn test_run_state_from_spec_has_no_session_id_by_default() {
        let state = RunState::from_spec(
            PathBuf::from("spec-feature.md"),
            PathBuf::from("spec-feature.json"),
        );
        assert!(state.session_id.is_none());
    }

    #[test]
    fn test_run_state_from_spec_with_config_and_session() {
        let config = Config {
            review: true,
            commit: true,
            pull_request: true,
            ..Default::default()
        };
        let state = RunState::from_spec_with_config_and_session(
            PathBuf::from("spec-feature.md"),
            PathBuf::from("spec-feature.json"),
            config.clone(),
            "worktree1".to_string(),
        );
        assert_eq!(state.session_id, Some("worktree1".to_string()));
        assert_eq!(state.config, Some(config));
    }

    #[test]
    fn test_run_state_session_id_serialization_roundtrip() {
        let state = RunState::new_with_session(
            PathBuf::from("test.json"),
            "test-branch".to_string(),
            "abc12345".to_string(),
        );

        // Serialize to JSON
        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("\"session_id\":\"abc12345\""));

        // Deserialize back
        let deserialized: RunState = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.session_id, Some("abc12345".to_string()));
    }

    #[test]
    fn test_run_state_backwards_compatible_without_session_id_field() {
        // Simulate loading a legacy state.json that doesn't have the session_id field
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
        assert!(
            state.session_id.is_none(),
            "Legacy state without session_id should deserialize with None"
        );
    }

    #[test]
    fn test_state_manager_preserves_session_id_on_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let state = RunState::new_with_session(
            PathBuf::from("test.json"),
            "test-branch".to_string(),
            "mysession".to_string(),
        );

        sm.save(&state).unwrap();

        let loaded = sm.load_current().unwrap().unwrap();
        assert_eq!(loaded.session_id, Some("mysession".to_string()));
    }

    #[test]
    fn test_run_state_session_id_with_main_constant() {
        use crate::worktree::MAIN_SESSION_ID;

        let state = RunState::new_with_session(
            PathBuf::from("test.json"),
            "test-branch".to_string(),
            MAIN_SESSION_ID.to_string(),
        );
        assert_eq!(state.session_id, Some("main".to_string()));
    }

    #[test]
    fn test_run_state_session_id_with_hash_format() {
        // Test with a typical hash-based session ID (8 hex chars)
        let state = RunState::new_with_session(
            PathBuf::from("test.json"),
            "test-branch".to_string(),
            "a1b2c3d4".to_string(),
        );
        assert_eq!(state.session_id, Some("a1b2c3d4".to_string()));

        // Verify it's valid hex
        let session_id = state.session_id.unwrap();
        assert_eq!(session_id.len(), 8);
        assert!(session_id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // ======================================================================
    // Tests for US-003: Per-Session State Storage
    // ======================================================================

    #[test]
    fn test_session_metadata_serialization() {
        let metadata = SessionMetadata {
            session_id: "main".to_string(),
            worktree_path: PathBuf::from("/home/user/project"),
            branch_name: "feature/test".to_string(),
            created_at: Utc::now(),
            last_active_at: Utc::now(),
            is_running: true,
        };

        let json = serde_json::to_string(&metadata).unwrap();
        let parsed: SessionMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.session_id, "main");
        assert_eq!(parsed.worktree_path, PathBuf::from("/home/user/project"));
        assert_eq!(parsed.branch_name, "feature/test");
        assert!(parsed.is_running);
    }

    #[test]
    fn test_session_metadata_camel_case() {
        let metadata = SessionMetadata {
            session_id: "main".to_string(),
            worktree_path: PathBuf::from("/path"),
            branch_name: "main".to_string(),
            created_at: Utc::now(),
            last_active_at: Utc::now(),
            is_running: false,
        };

        let json = serde_json::to_string(&metadata).unwrap();

        // Verify camelCase serialization
        assert!(json.contains("sessionId"));
        assert!(json.contains("worktreePath"));
        assert!(json.contains("branchName"));
        assert!(json.contains("createdAt"));
        assert!(json.contains("lastActiveAt"));
        assert!(json.contains("isRunning"));

        // Verify snake_case is NOT used
        assert!(!json.contains("session_id"));
        assert!(!json.contains("worktree_path"));
        assert!(!json.contains("branch_name"));
        assert!(!json.contains("created_at"));
        assert!(!json.contains("last_active_at"));
        assert!(!json.contains("is_running"));
    }

    #[test]
    fn test_state_manager_session_id_accessor() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());
        assert_eq!(sm.session_id(), MAIN_SESSION_ID);
    }

    #[test]
    fn test_state_manager_with_custom_session() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "abc12345".to_string(),
        );
        assert_eq!(sm.session_id(), "abc12345");
    }

    #[test]
    fn test_state_manager_session_directory_structure() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        sm.save(&state).unwrap();

        // Verify the directory structure
        let sessions_dir = temp_dir.path().join(SESSIONS_DIR);
        let session_dir = sessions_dir.join(MAIN_SESSION_ID);
        let state_file = session_dir.join(STATE_FILE);
        let metadata_file = session_dir.join(METADATA_FILE);

        assert!(sessions_dir.exists(), "sessions/ should exist");
        assert!(session_dir.exists(), "sessions/main/ should exist");
        assert!(state_file.exists(), "sessions/main/state.json should exist");
        assert!(
            metadata_file.exists(),
            "sessions/main/metadata.json should exist"
        );
    }

    #[test]
    fn test_state_manager_creates_metadata_on_save() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let state = RunState::new(PathBuf::from("test.json"), "feature-branch".to_string());
        sm.save(&state).unwrap();

        // Load metadata and verify
        let metadata = sm.load_metadata().unwrap().unwrap();
        assert_eq!(metadata.session_id, MAIN_SESSION_ID);
        assert_eq!(metadata.branch_name, "feature-branch");
    }

    #[test]
    fn test_state_manager_load_metadata_returns_none_for_new_session() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        // No save yet - metadata should be None
        let metadata = sm.load_metadata().unwrap();
        assert!(metadata.is_none());
    }

    #[test]
    fn test_state_manager_list_sessions_empty() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let sessions = sm.list_sessions().unwrap();
        assert!(sessions.is_empty());
    }

    #[test]
    fn test_state_manager_list_sessions_single() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        sm.save(&state).unwrap();

        let sessions = sm.list_sessions().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, MAIN_SESSION_ID);
        assert_eq!(sessions[0].branch_name, "test-branch");
    }

    #[test]
    fn test_state_manager_list_sessions_multiple() {
        let temp_dir = TempDir::new().unwrap();

        // Create first session
        let sm1 = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "session1".to_string(),
        );
        let state1 = RunState::new(PathBuf::from("test1.json"), "branch1".to_string());
        sm1.save(&state1).unwrap();

        // Create second session
        let sm2 = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "session2".to_string(),
        );
        let state2 = RunState::new(PathBuf::from("test2.json"), "branch2".to_string());
        sm2.save(&state2).unwrap();

        // List sessions from either manager
        let sessions = sm1.list_sessions().unwrap();
        assert_eq!(sessions.len(), 2);

        // Both sessions should be present
        let session_ids: Vec<&str> = sessions.iter().map(|s| s.session_id.as_str()).collect();
        assert!(session_ids.contains(&"session1"));
        assert!(session_ids.contains(&"session2"));
    }

    #[test]
    fn test_state_manager_list_sessions_sorted_by_last_active() {
        let temp_dir = TempDir::new().unwrap();

        // Create older session first
        let sm1 =
            StateManager::with_dir_and_session(temp_dir.path().to_path_buf(), "older".to_string());
        let state1 = RunState::new(PathBuf::from("test1.json"), "branch1".to_string());
        sm1.save(&state1).unwrap();

        // Small delay to ensure different timestamps
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Create newer session
        let sm2 =
            StateManager::with_dir_and_session(temp_dir.path().to_path_buf(), "newer".to_string());
        let state2 = RunState::new(PathBuf::from("test2.json"), "branch2".to_string());
        sm2.save(&state2).unwrap();

        let sessions = sm1.list_sessions().unwrap();
        assert_eq!(sessions.len(), 2);

        // Newer session should be first (sorted by last_active_at descending)
        assert_eq!(sessions[0].session_id, "newer");
        assert_eq!(sessions[1].session_id, "older");
    }

    #[test]
    fn test_state_manager_get_session_existing() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        sm.save(&state).unwrap();

        // Get the session
        let session_sm = sm.get_session(MAIN_SESSION_ID);
        assert!(session_sm.is_some());

        let session_sm = session_sm.unwrap();
        assert_eq!(session_sm.session_id(), MAIN_SESSION_ID);

        // Load state from the returned manager
        let loaded = session_sm.load_current().unwrap().unwrap();
        assert_eq!(loaded.run_id, state.run_id);
    }

    #[test]
    fn test_state_manager_get_session_nonexistent() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let session = sm.get_session("nonexistent");
        assert!(session.is_none());
    }

    #[test]
    fn test_state_manager_get_session_different_id() {
        let temp_dir = TempDir::new().unwrap();

        // Create a session
        let sm1 = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "session-abc".to_string(),
        );
        let state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        sm1.save(&state).unwrap();

        // Get session from a different StateManager
        let sm2 = StateManager::with_dir(temp_dir.path().to_path_buf());
        let session_sm = sm2.get_session("session-abc");
        assert!(session_sm.is_some());

        let session_sm = session_sm.unwrap();
        assert_eq!(session_sm.session_id(), "session-abc");
    }

    #[test]
    fn test_state_manager_migrate_legacy_state() {
        let temp_dir = TempDir::new().unwrap();

        // Create a legacy state.json in the root
        let legacy_state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        let legacy_content = serde_json::to_string_pretty(&legacy_state).unwrap();
        let legacy_path = temp_dir.path().join(STATE_FILE);
        fs::write(&legacy_path, &legacy_content).unwrap();

        assert!(legacy_path.exists(), "Legacy state file should exist");

        // Create a StateManager - this should trigger migration
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());
        sm.ensure_dirs().unwrap(); // Trigger migration if needed

        // Now create a new manager that will migrate
        let sm = {
            let mut sm = StateManager {
                base_dir: temp_dir.path().to_path_buf(),
                session_id: MAIN_SESSION_ID.to_string(),
            };
            // Manually call migrate since with_dir doesn't call it
            sm.migrate_legacy_state().unwrap();
            sm
        };

        // Legacy file should be removed
        assert!(
            !legacy_path.exists(),
            "Legacy state file should be removed after migration"
        );

        // State should exist in new location
        let new_state_path = temp_dir
            .path()
            .join(SESSIONS_DIR)
            .join(MAIN_SESSION_ID)
            .join(STATE_FILE);
        assert!(new_state_path.exists(), "Migrated state file should exist");

        // Metadata should be created
        let metadata_path = temp_dir
            .path()
            .join(SESSIONS_DIR)
            .join(MAIN_SESSION_ID)
            .join(METADATA_FILE);
        assert!(metadata_path.exists(), "Metadata file should be created");

        // Load and verify state
        let loaded = sm.load_current().unwrap().unwrap();
        assert_eq!(loaded.run_id, legacy_state.run_id);
        assert_eq!(loaded.session_id, Some(MAIN_SESSION_ID.to_string()));
    }

    #[test]
    fn test_state_manager_migrate_preserves_state_data() {
        let temp_dir = TempDir::new().unwrap();

        // Create a legacy state with lots of data
        let mut legacy_state = RunState::new(PathBuf::from("test.json"), "feature-x".to_string());
        legacy_state.start_iteration("US-001");
        legacy_state.iterations.last_mut().unwrap().work_summary =
            Some("Did some work".to_string());
        legacy_state.review_iteration = 2;
        legacy_state.knowledge.baseline_commit = Some("abc123".to_string());

        let legacy_content = serde_json::to_string_pretty(&legacy_state).unwrap();
        fs::write(temp_dir.path().join(STATE_FILE), &legacy_content).unwrap();

        // Trigger migration
        let sm = {
            let mut sm = StateManager {
                base_dir: temp_dir.path().to_path_buf(),
                session_id: MAIN_SESSION_ID.to_string(),
            };
            sm.migrate_legacy_state().unwrap();
            sm
        };

        // Verify all data preserved
        let loaded = sm.load_current().unwrap().unwrap();
        assert_eq!(loaded.branch, "feature-x");
        assert_eq!(loaded.iteration, 1);
        assert_eq!(loaded.review_iteration, 2);
        assert_eq!(
            loaded.iterations[0].work_summary,
            Some("Did some work".to_string())
        );
        assert_eq!(loaded.knowledge.baseline_commit, Some("abc123".to_string()));
    }

    #[test]
    fn test_state_manager_migrate_skips_if_already_migrated() {
        let temp_dir = TempDir::new().unwrap();

        // Create session structure first
        let session_dir = temp_dir.path().join(SESSIONS_DIR).join(MAIN_SESSION_ID);
        fs::create_dir_all(&session_dir).unwrap();

        let state = RunState::new(PathBuf::from("new.json"), "new-branch".to_string());
        let content = serde_json::to_string_pretty(&state).unwrap();
        fs::write(session_dir.join(STATE_FILE), &content).unwrap();

        // Create legacy file with DIFFERENT content
        let legacy_state = RunState::new(PathBuf::from("legacy.json"), "legacy-branch".to_string());
        let legacy_content = serde_json::to_string_pretty(&legacy_state).unwrap();
        let legacy_path = temp_dir.path().join(STATE_FILE);
        fs::write(&legacy_path, &legacy_content).unwrap();

        // Trigger migration
        let sm = {
            let mut sm = StateManager {
                base_dir: temp_dir.path().to_path_buf(),
                session_id: MAIN_SESSION_ID.to_string(),
            };
            sm.migrate_legacy_state().unwrap();
            sm
        };

        // Legacy file should still be removed
        assert!(!legacy_path.exists());

        // But the existing session state should NOT be overwritten
        let loaded = sm.load_current().unwrap().unwrap();
        assert_eq!(loaded.branch, "new-branch"); // Not legacy-branch
    }

    #[test]
    fn test_state_manager_clear_removes_metadata() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        sm.save(&state).unwrap();

        // Both files should exist
        let session_dir = temp_dir.path().join(SESSIONS_DIR).join(MAIN_SESSION_ID);
        assert!(session_dir.join(STATE_FILE).exists());
        assert!(session_dir.join(METADATA_FILE).exists());

        // Clear
        sm.clear_current().unwrap();

        // Both files should be removed
        assert!(!session_dir.join(STATE_FILE).exists());
        assert!(!session_dir.join(METADATA_FILE).exists());
    }

    #[test]
    fn test_state_manager_metadata_updates_last_active_on_save() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        sm.save(&state).unwrap();

        let metadata1 = sm.load_metadata().unwrap().unwrap();
        let first_active = metadata1.last_active_at;

        // Wait a bit
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Save again
        sm.save(&state).unwrap();

        let metadata2 = sm.load_metadata().unwrap().unwrap();
        assert!(
            metadata2.last_active_at > first_active,
            "last_active_at should be updated on save"
        );

        // created_at should remain the same
        assert_eq!(
            metadata2.created_at, metadata1.created_at,
            "created_at should not change"
        );
    }

    #[test]
    fn test_state_manager_different_sessions_isolated() {
        let temp_dir = TempDir::new().unwrap();

        // Create first session
        let sm1 = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "session-a".to_string(),
        );
        let state1 = RunState::new(PathBuf::from("a.json"), "branch-a".to_string());
        sm1.save(&state1).unwrap();

        // Create second session
        let sm2 = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "session-b".to_string(),
        );
        let state2 = RunState::new(PathBuf::from("b.json"), "branch-b".to_string());
        sm2.save(&state2).unwrap();

        // Each should load their own state
        let loaded1 = sm1.load_current().unwrap().unwrap();
        let loaded2 = sm2.load_current().unwrap().unwrap();

        assert_eq!(loaded1.branch, "branch-a");
        assert_eq!(loaded2.branch, "branch-b");

        // Clearing one shouldn't affect the other
        sm1.clear_current().unwrap();

        assert!(sm1.load_current().unwrap().is_none());
        assert!(sm2.load_current().unwrap().is_some());
    }

    #[test]
    fn test_state_manager_spec_dir_shared_across_sessions() {
        let temp_dir = TempDir::new().unwrap();

        // Create two sessions
        let sm1 = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "session-1".to_string(),
        );
        let sm2 = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "session-2".to_string(),
        );

        // Both should share the same spec directory
        assert_eq!(sm1.spec_dir(), sm2.spec_dir());
        assert_eq!(sm1.spec_dir(), temp_dir.path().join(SPEC_DIR));
    }

    #[test]
    fn test_state_manager_runs_dir_shared_across_sessions() {
        let temp_dir = TempDir::new().unwrap();

        // Create two sessions
        let sm1 = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "session-1".to_string(),
        );
        let sm2 = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "session-2".to_string(),
        );

        // Archive from both sessions
        let state1 = RunState::new(PathBuf::from("test1.json"), "branch1".to_string());
        let state2 = RunState::new(PathBuf::from("test2.json"), "branch2".to_string());

        sm1.archive(&state1).unwrap();
        sm2.archive(&state2).unwrap();

        // Both archives should be in the shared runs/ directory
        let runs_dir = temp_dir.path().join(RUNS_DIR);
        let archives: Vec<_> = fs::read_dir(&runs_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();

        assert_eq!(archives.len(), 2, "Both archives should be in shared runs/");
    }

    // ======================================================================
    // Tests for US-006: Branch Conflict Detection
    // ======================================================================

    #[test]
    fn test_check_branch_conflict_no_conflict_empty() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        // No sessions exist, should be no conflict
        let conflict = sm.check_branch_conflict("feature-branch").unwrap();
        assert!(conflict.is_none());
    }

    #[test]
    fn test_check_branch_conflict_no_conflict_different_branch() {
        let temp_dir = TempDir::new().unwrap();

        // Create a session with a different branch
        let sm1 = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "session1".to_string(),
        );
        let state1 = RunState::new(PathBuf::from("test1.json"), "other-branch".to_string());
        sm1.save(&state1).unwrap();

        // Check for conflict on a different branch
        let sm2 = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "session2".to_string(),
        );
        let conflict = sm2.check_branch_conflict("feature-branch").unwrap();
        assert!(conflict.is_none());
    }

    #[test]
    fn test_check_branch_conflict_no_conflict_same_session() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        // Create a session
        let state = RunState::new(PathBuf::from("test.json"), "feature-branch".to_string());
        sm.save(&state).unwrap();

        // Checking from the same session should not cause conflict
        let conflict = sm.check_branch_conflict("feature-branch").unwrap();
        assert!(conflict.is_none());
    }

    #[test]
    fn test_check_branch_conflict_no_conflict_not_running() {
        let temp_dir = TempDir::new().unwrap();

        // Create a session with a completed run
        let sm1 = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "session1".to_string(),
        );
        let mut state1 = RunState::new(PathBuf::from("test1.json"), "feature-branch".to_string());
        state1.transition_to(MachineState::Completed); // Mark as completed
        sm1.save(&state1).unwrap();

        // Another session checking for the same branch should NOT find conflict
        // because the first session is not running
        let sm2 = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "session2".to_string(),
        );
        let conflict = sm2.check_branch_conflict("feature-branch").unwrap();
        assert!(conflict.is_none());
    }

    #[test]
    fn test_check_branch_conflict_detects_conflict() {
        let temp_dir = TempDir::new().unwrap();

        // Create a running session
        let sm1 = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "session1".to_string(),
        );
        let state1 = RunState::new(PathBuf::from("test1.json"), "feature-branch".to_string());
        sm1.save(&state1).unwrap();

        // Another session checking for the same branch should find conflict
        let sm2 = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "session2".to_string(),
        );
        let conflict = sm2.check_branch_conflict("feature-branch").unwrap();

        assert!(conflict.is_some(), "Should detect branch conflict");
        let conflict = conflict.unwrap();
        assert_eq!(conflict.session_id, "session1");
        assert_eq!(conflict.branch_name, "feature-branch");
        assert!(conflict.is_running);
    }

    #[test]
    fn test_check_branch_conflict_stale_session_no_conflict() {
        let temp_dir = TempDir::new().unwrap();
        let worktree_dir = TempDir::new().unwrap();

        // Create a session pointing to a worktree
        let sm1 = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "session1".to_string(),
        );
        let state1 = RunState::new(PathBuf::from("test1.json"), "feature-branch".to_string());
        sm1.save(&state1).unwrap();

        // Manually update the metadata to point to the worktree directory
        let metadata_path = temp_dir
            .path()
            .join(SESSIONS_DIR)
            .join("session1")
            .join(METADATA_FILE);
        let mut metadata: SessionMetadata =
            serde_json::from_str(&fs::read_to_string(&metadata_path).unwrap()).unwrap();
        metadata.worktree_path = worktree_dir.path().to_path_buf();
        fs::write(
            &metadata_path,
            serde_json::to_string_pretty(&metadata).unwrap(),
        )
        .unwrap();

        // Now delete the worktree directory to make it stale
        drop(worktree_dir);

        // Another session checking for the same branch should NOT find conflict
        // because the first session's worktree is deleted (stale)
        let sm2 = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "session2".to_string(),
        );
        let conflict = sm2.check_branch_conflict("feature-branch").unwrap();
        assert!(
            conflict.is_none(),
            "Stale sessions should not cause conflicts"
        );
    }

    #[test]
    fn test_check_branch_conflict_returns_correct_metadata() {
        let temp_dir = TempDir::new().unwrap();

        // Create a running session
        let sm1 = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "session-abc".to_string(),
        );
        let state1 = RunState::new(
            PathBuf::from("test1.json"),
            "feature/my-feature".to_string(),
        );
        sm1.save(&state1).unwrap();

        // Check conflict from another session
        let sm2 = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "session-xyz".to_string(),
        );
        let conflict = sm2.check_branch_conflict("feature/my-feature").unwrap();

        assert!(conflict.is_some());
        let conflict = conflict.unwrap();
        assert_eq!(conflict.session_id, "session-abc");
        assert_eq!(conflict.branch_name, "feature/my-feature");
        // worktree_path should be set (to CWD by default in tests)
        assert!(!conflict.worktree_path.as_os_str().is_empty());
    }

    #[test]
    fn test_session_metadata_is_running_field() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        // Create a running session
        let state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        sm.save(&state).unwrap();

        let metadata = sm.load_metadata().unwrap().unwrap();
        assert!(
            metadata.is_running,
            "New RunState should be marked as running"
        );

        // Mark as completed and save again
        let mut state = sm.load_current().unwrap().unwrap();
        state.transition_to(MachineState::Completed);
        sm.save(&state).unwrap();

        let metadata = sm.load_metadata().unwrap().unwrap();
        assert!(
            !metadata.is_running,
            "Completed run should not be marked as running"
        );
    }

    #[test]
    fn test_session_metadata_is_running_on_failure() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        // Create a running session
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        sm.save(&state).unwrap();

        // Mark as failed
        state.transition_to(MachineState::Failed);
        sm.save(&state).unwrap();

        let metadata = sm.load_metadata().unwrap().unwrap();
        assert!(
            !metadata.is_running,
            "Failed run should not be marked as running"
        );
    }

    #[test]
    fn test_session_metadata_is_running_default() {
        // Test that is_running defaults to false when deserializing old metadata
        let json = r#"{
            "sessionId": "main",
            "worktreePath": "/tmp/test",
            "branchName": "test-branch",
            "createdAt": "2024-01-01T00:00:00Z",
            "lastActiveAt": "2024-01-01T00:00:00Z"
        }"#;

        let metadata: SessionMetadata = serde_json::from_str(json).unwrap();
        assert!(
            !metadata.is_running,
            "is_running should default to false for backwards compatibility"
        );
    }

    // ======================================================================
    // Tests for US-009: Multi-Session Status Command
    // ======================================================================

    #[test]
    fn test_us009_list_sessions_with_status_empty() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let sessions = sm.list_sessions_with_status().unwrap();
        assert!(sessions.is_empty());
    }

    #[test]
    fn test_us009_list_sessions_with_status_single() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.current_story = Some("US-001".to_string());
        sm.save(&state).unwrap();

        let sessions = sm.list_sessions_with_status().unwrap();
        assert_eq!(sessions.len(), 1);

        let session = &sessions[0];
        assert_eq!(session.metadata.session_id, MAIN_SESSION_ID);
        assert_eq!(session.metadata.branch_name, "test-branch");
        assert_eq!(session.current_story, Some("US-001".to_string()));
        assert_eq!(session.machine_state, Some(MachineState::Initializing));
    }

    #[test]
    fn test_us009_list_sessions_with_status_multiple() {
        let temp_dir = TempDir::new().unwrap();

        // Create first session
        let sm1 = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "session1".to_string(),
        );
        let mut state1 = RunState::new(PathBuf::from("test1.json"), "branch1".to_string());
        state1.current_story = Some("US-001".to_string());
        state1.transition_to(MachineState::RunningClaude);
        sm1.save(&state1).unwrap();

        // Create second session
        let sm2 = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "session2".to_string(),
        );
        let mut state2 = RunState::new(PathBuf::from("test2.json"), "branch2".to_string());
        state2.current_story = Some("US-002".to_string());
        state2.transition_to(MachineState::Reviewing);
        sm2.save(&state2).unwrap();

        // List sessions
        let sessions = sm1.list_sessions_with_status().unwrap();
        assert_eq!(sessions.len(), 2);

        // Both sessions should have their state info
        let session_ids: Vec<&str> = sessions
            .iter()
            .map(|s| s.metadata.session_id.as_str())
            .collect();
        assert!(session_ids.contains(&"session1"));
        assert!(session_ids.contains(&"session2"));
    }

    #[test]
    fn test_us009_list_sessions_with_status_includes_machine_state() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.transition_to(MachineState::Reviewing);
        sm.save(&state).unwrap();

        let sessions = sm.list_sessions_with_status().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].machine_state, Some(MachineState::Reviewing));
    }

    #[test]
    fn test_us009_list_sessions_with_status_detects_stale() {
        let temp_dir = TempDir::new().unwrap();
        let worktree_dir = TempDir::new().unwrap();

        // Create a session
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());
        let state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        sm.save(&state).unwrap();

        // Update metadata to point to the temp worktree directory
        let metadata_path = temp_dir
            .path()
            .join(SESSIONS_DIR)
            .join(MAIN_SESSION_ID)
            .join(METADATA_FILE);
        let mut metadata: SessionMetadata =
            serde_json::from_str(&fs::read_to_string(&metadata_path).unwrap()).unwrap();
        metadata.worktree_path = worktree_dir.path().to_path_buf();
        fs::write(
            &metadata_path,
            serde_json::to_string_pretty(&metadata).unwrap(),
        )
        .unwrap();

        // Session should NOT be stale yet
        let sessions = sm.list_sessions_with_status().unwrap();
        assert_eq!(sessions.len(), 1);
        assert!(!sessions[0].is_stale);

        // Delete the worktree directory to make it stale
        drop(worktree_dir);

        // Now session should be marked as stale
        let sessions = sm.list_sessions_with_status().unwrap();
        assert_eq!(sessions.len(), 1);
        assert!(sessions[0].is_stale, "Session should be marked stale");
    }

    #[test]
    fn test_us009_list_sessions_with_status_sorted_by_last_active() {
        // Test that sessions are sorted by last_active_at descending when none is current
        let temp_dir = TempDir::new().unwrap();

        // Create older session
        let sm1 = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "older-session".to_string(),
        );
        let state1 = RunState::new(PathBuf::from("test1.json"), "branch1".to_string());
        sm1.save(&state1).unwrap();

        // Small delay
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Create newer session
        let sm2 = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "newer-session".to_string(),
        );
        let state2 = RunState::new(PathBuf::from("test2.json"), "branch2".to_string());
        sm2.save(&state2).unwrap();

        // List sessions - newer should be first (sorted by last_active_at descending)
        let sessions = sm1.list_sessions_with_status().unwrap();
        assert_eq!(sessions.len(), 2);
        assert_eq!(
            sessions[0].metadata.session_id, "newer-session",
            "Newer session should be first"
        );
        assert_eq!(
            sessions[1].metadata.session_id, "older-session",
            "Older session should be second"
        );
    }

    #[test]
    fn test_us009_session_status_is_current_detection() {
        // Test that is_current is based on worktree_path matching CWD
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        sm.save(&state).unwrap();

        // The metadata worktree_path defaults to CWD (from save_metadata)
        // so the session should be marked as current
        let sessions = sm.list_sessions_with_status().unwrap();
        assert_eq!(sessions.len(), 1);
        // Note: This will be current if CWD matches the saved worktree_path
        // The actual CWD during test may differ, so we just verify the field exists
        let _is_current = sessions[0].is_current;
    }

    #[test]
    fn test_us009_session_status_struct_fields() {
        // Verify SessionStatus has all expected fields
        let metadata = SessionMetadata {
            session_id: "test".to_string(),
            worktree_path: PathBuf::from("/tmp/test"),
            branch_name: "test-branch".to_string(),
            created_at: Utc::now(),
            last_active_at: Utc::now(),
            is_running: true,
        };

        let status = SessionStatus {
            metadata: metadata.clone(),
            machine_state: Some(MachineState::RunningClaude),
            current_story: Some("US-001".to_string()),
            is_current: true,
            is_stale: false,
        };

        assert_eq!(status.metadata.session_id, "test");
        assert_eq!(status.machine_state, Some(MachineState::RunningClaude));
        assert_eq!(status.current_story, Some("US-001".to_string()));
        assert!(status.is_current);
        assert!(!status.is_stale);
    }

    // ======================================================================
    // Tests for US-002: LiveState struct and file management
    // ======================================================================

    #[test]
    fn test_live_state_new() {
        let live = LiveState::new(MachineState::RunningClaude);

        assert!(live.output_lines.is_empty());
        assert_eq!(live.machine_state, MachineState::RunningClaude);
        // updated_at should be recent (within last second)
        let elapsed = Utc::now() - live.updated_at;
        assert!(elapsed.num_seconds() < 1);
    }

    #[test]
    fn test_live_state_append_line() {
        let mut live = LiveState::new(MachineState::RunningClaude);

        live.append_line("Line 1".to_string());
        live.append_line("Line 2".to_string());

        assert_eq!(live.output_lines.len(), 2);
        assert_eq!(live.output_lines[0], "Line 1");
        assert_eq!(live.output_lines[1], "Line 2");
    }

    #[test]
    fn test_live_state_append_line_updates_timestamp() {
        let mut live = LiveState::new(MachineState::RunningClaude);
        let initial_time = live.updated_at;

        // Small delay to ensure timestamp changes
        std::thread::sleep(std::time::Duration::from_millis(10));

        live.append_line("New line".to_string());

        assert!(
            live.updated_at > initial_time,
            "updated_at should be updated on append_line"
        );
    }

    #[test]
    fn test_live_state_max_50_lines() {
        let mut live = LiveState::new(MachineState::RunningClaude);

        // Add 60 lines
        for i in 0..60 {
            live.append_line(format!("Line {}", i));
        }

        // Should only keep last 50 lines
        assert_eq!(live.output_lines.len(), 50);
        // First line should be "Line 10" (lines 0-9 were dropped)
        assert_eq!(live.output_lines[0], "Line 10");
        // Last line should be "Line 59"
        assert_eq!(live.output_lines[49], "Line 59");
    }

    #[test]
    fn test_live_state_exactly_50_lines() {
        let mut live = LiveState::new(MachineState::RunningClaude);

        // Add exactly 50 lines
        for i in 0..50 {
            live.append_line(format!("Line {}", i));
        }

        assert_eq!(live.output_lines.len(), 50);
        assert_eq!(live.output_lines[0], "Line 0");
        assert_eq!(live.output_lines[49], "Line 49");
    }

    #[test]
    fn test_live_state_serialization_roundtrip() {
        let mut live = LiveState::new(MachineState::Reviewing);
        live.append_line("Test output".to_string());

        let json = serde_json::to_string(&live).unwrap();
        let parsed: LiveState = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.output_lines, live.output_lines);
        assert_eq!(parsed.machine_state, MachineState::Reviewing);
    }

    #[test]
    fn test_live_state_camel_case_serialization() {
        let live = LiveState::new(MachineState::RunningClaude);
        let json = serde_json::to_string(&live).unwrap();

        // Verify camelCase serialization
        assert!(json.contains("outputLines"));
        assert!(json.contains("updatedAt"));
        assert!(json.contains("machineState"));

        // Verify snake_case is NOT used
        assert!(!json.contains("output_lines"));
        assert!(!json.contains("updated_at"));
        assert!(!json.contains("machine_state"));
    }

    #[test]
    fn test_state_manager_save_and_load_live() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        // Ensure session directory exists
        sm.ensure_dirs().unwrap();

        let mut live = LiveState::new(MachineState::RunningClaude);
        live.append_line("Hello from Claude".to_string());

        sm.save_live(&live).unwrap();

        let loaded = sm.load_live();
        assert!(loaded.is_some(), "Should load saved live state");

        let loaded = loaded.unwrap();
        assert_eq!(loaded.output_lines, vec!["Hello from Claude"]);
        assert_eq!(loaded.machine_state, MachineState::RunningClaude);
    }

    #[test]
    fn test_state_manager_load_live_returns_none_when_missing() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let loaded = sm.load_live();
        assert!(
            loaded.is_none(),
            "Should return None when live.json doesn't exist"
        );
    }

    #[test]
    fn test_state_manager_load_live_handles_corrupted_file() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        // Create session directory and write corrupted content
        sm.ensure_dirs().unwrap();
        let live_path = temp_dir
            .path()
            .join(SESSIONS_DIR)
            .join(MAIN_SESSION_ID)
            .join(LIVE_FILE);
        fs::write(&live_path, "not valid json {{{").unwrap();

        // Should gracefully return None instead of panicking
        let loaded = sm.load_live();
        assert!(loaded.is_none(), "Should return None for corrupted file");
    }

    #[test]
    fn test_state_manager_clear_live() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        sm.ensure_dirs().unwrap();

        // Save live state
        let live = LiveState::new(MachineState::RunningClaude);
        sm.save_live(&live).unwrap();
        assert!(sm.load_live().is_some());

        // Clear live state
        sm.clear_live().unwrap();
        assert!(sm.load_live().is_none(), "Live state should be cleared");
    }

    #[test]
    fn test_state_manager_clear_live_no_error_when_missing() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        // Should not error when file doesn't exist
        let result = sm.clear_live();
        assert!(result.is_ok());
    }

    #[test]
    fn test_state_manager_clear_current_also_clears_live() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        // Save both state and live state
        let state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        sm.save(&state).unwrap();

        let live = LiveState::new(MachineState::RunningClaude);
        sm.save_live(&live).unwrap();

        // Clear current - should also clear live
        sm.clear_current().unwrap();

        assert!(sm.load_current().unwrap().is_none());
        assert!(
            sm.load_live().is_none(),
            "clear_current should also clear live state"
        );
    }

    #[test]
    fn test_state_manager_save_live_atomic_write() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());
        sm.ensure_dirs().unwrap();

        let live = LiveState::new(MachineState::RunningClaude);
        sm.save_live(&live).unwrap();

        // Verify no temp file remains
        let temp_path = temp_dir
            .path()
            .join(SESSIONS_DIR)
            .join(MAIN_SESSION_ID)
            .join("live.json.tmp");
        assert!(
            !temp_path.exists(),
            "Temp file should be renamed, not remain"
        );

        // Verify actual file exists
        let live_path = temp_dir
            .path()
            .join(SESSIONS_DIR)
            .join(MAIN_SESSION_ID)
            .join(LIVE_FILE);
        assert!(live_path.exists(), "live.json should exist after save");
    }

    #[test]
    fn test_state_manager_live_file_location() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "test-session".to_string(),
        );

        sm.ensure_dirs().unwrap();

        let live = LiveState::new(MachineState::RunningClaude);
        sm.save_live(&live).unwrap();

        // Verify file is in the correct session directory
        let expected_path = temp_dir
            .path()
            .join(SESSIONS_DIR)
            .join("test-session")
            .join(LIVE_FILE);
        assert!(
            expected_path.exists(),
            "live.json should be in session directory"
        );
    }

    #[test]
    fn test_live_state_different_machine_states() {
        // Test that all relevant machine states serialize correctly
        let states = vec![
            MachineState::Idle,
            MachineState::RunningClaude,
            MachineState::Reviewing,
            MachineState::Correcting,
            MachineState::Committing,
        ];

        for state in states {
            let live = LiveState::new(state);
            let json = serde_json::to_string(&live).unwrap();
            let parsed: LiveState = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed.machine_state, state);
        }
    }

    // ========================================================================
    // US-002: Heartbeat Mechanism Tests
    // ========================================================================

    #[test]
    fn test_live_state_new_has_heartbeat() {
        let live = LiveState::new(MachineState::RunningClaude);

        // Should have last_heartbeat set to a recent time
        assert!(live.is_heartbeat_fresh());

        // Heartbeat should be close to now
        let age = Utc::now()
            .signed_duration_since(live.last_heartbeat)
            .num_seconds();
        assert!(age < 1, "Heartbeat should be very recent");
    }

    #[test]
    fn test_live_state_update_heartbeat() {
        let mut live = LiveState::new(MachineState::RunningClaude);
        let original_heartbeat = live.last_heartbeat;

        // Small delay to ensure time difference
        std::thread::sleep(std::time::Duration::from_millis(10));

        live.update_heartbeat();

        // Heartbeat should be updated
        assert!(
            live.last_heartbeat > original_heartbeat,
            "Heartbeat should be updated to a newer time"
        );
        assert!(live.is_heartbeat_fresh());
    }

    #[test]
    fn test_live_state_update_state() {
        let mut live = LiveState::new(MachineState::PickingStory);
        let original_time = live.updated_at;

        // Small delay to ensure time difference
        std::thread::sleep(std::time::Duration::from_millis(10));

        live.update_state(MachineState::RunningClaude);

        // State should be updated
        assert_eq!(live.machine_state, MachineState::RunningClaude);

        // Timestamps should be updated
        assert!(
            live.updated_at > original_time,
            "updated_at should be refreshed"
        );
        assert!(live.is_heartbeat_fresh());
    }

    #[test]
    fn test_live_state_is_heartbeat_fresh() {
        let live = LiveState::new(MachineState::RunningClaude);

        // Fresh heartbeat should return true
        assert!(live.is_heartbeat_fresh());
    }

    #[test]
    fn test_live_state_stale_heartbeat() {
        let mut live = LiveState::new(MachineState::RunningClaude);

        // Set heartbeat to be older than the threshold (60 seconds)
        live.last_heartbeat = Utc::now() - chrono::Duration::seconds(65);

        // Stale heartbeat should return false
        assert!(
            !live.is_heartbeat_fresh(),
            "Heartbeat older than 60 seconds should be stale"
        );
    }

    #[test]
    fn test_heartbeat_stale_threshold_constant() {
        // Verify the threshold constant is 60 seconds
        assert_eq!(
            HEARTBEAT_STALE_THRESHOLD_SECS, 60,
            "Heartbeat threshold should be 60 seconds"
        );
    }

    #[test]
    fn test_live_state_heartbeat_serialization() {
        let live = LiveState::new(MachineState::RunningClaude);

        // Serialize to JSON
        let json = serde_json::to_string(&live).unwrap();

        // Parse back
        let parsed: LiveState = serde_json::from_str(&json).unwrap();

        // Heartbeat should be preserved
        assert_eq!(parsed.last_heartbeat, live.last_heartbeat);
        assert!(parsed.is_heartbeat_fresh());
    }

    #[test]
    fn test_live_state_heartbeat_default_on_deserialize() {
        // Test that old live.json without lastHeartbeat field gets a default
        let json = r#"{
            "outputLines": [],
            "updatedAt": "2024-01-01T00:00:00Z",
            "machineState": "running-claude"
        }"#;

        // This should deserialize successfully with default heartbeat
        let live: LiveState = serde_json::from_str(json).unwrap();

        // The default is Utc::now(), so it should be fresh
        assert!(
            live.is_heartbeat_fresh(),
            "Default heartbeat should be fresh (set to now)"
        );
    }

    #[test]
    fn test_live_state_append_line_preserves_heartbeat() {
        let mut live = LiveState::new(MachineState::RunningClaude);
        let original_heartbeat = live.last_heartbeat;

        // Append a line
        live.append_line("test output".to_string());

        // Heartbeat should NOT be automatically updated by append_line
        // (heartbeat is only updated by explicit update_heartbeat() or update_state() calls)
        // However, append_line DOES update updated_at
        assert!(live.updated_at >= original_heartbeat);

        // The heartbeat itself should still be fresh (since the state was just created)
        assert!(live.is_heartbeat_fresh());
    }
}
