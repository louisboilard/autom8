use crate::claude::{
    extract_decisions, extract_files_context, extract_patterns, ClaudeUsage, FileContextEntry,
};
use crate::config::{self, Config};
use crate::error::Result;
use crate::git;
use crate::knowledge::{Decision, FileChange, FileInfo, Pattern, ProjectKnowledge, StoryChanges};
use crate::worktree::{get_current_session_id, MAIN_SESSION_ID};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
    /// Path to the spec JSON file used for this session.
    /// Enables the improve command to quickly load the spec without searching.
    #[serde(default)]
    pub spec_json_path: Option<PathBuf>,
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
    /// Token usage data for this iteration
    #[serde(default)]
    pub usage: Option<ClaudeUsage>,
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
    /// Total accumulated token usage across all phases of the run.
    #[serde(default)]
    pub total_usage: Option<ClaudeUsage>,
    /// Token usage broken down by phase.
    /// Keys are story IDs (e.g., "US-001") or pseudo-phase names:
    /// - "Planning": spec generation
    /// - "US-001", "US-002", etc.: user story implementation
    /// - "Final Review": review iterations + corrections
    /// - "PR & Commit": commit generation + PR creation
    #[serde(default)]
    pub phase_usage: HashMap<String, ClaudeUsage>,
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
            total_usage: None,
            phase_usage: HashMap::new(),
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
            total_usage: None,
            phase_usage: HashMap::new(),
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
            total_usage: None,
            phase_usage: HashMap::new(),
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
            total_usage: None,
            phase_usage: HashMap::new(),
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
            total_usage: None,
            phase_usage: HashMap::new(),
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
            total_usage: None,
            phase_usage: HashMap::new(),
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
            total_usage: None,
            phase_usage: HashMap::new(),
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
            usage: None,
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

    /// Get the total run duration in seconds.
    ///
    /// Returns the time between `started_at` and `finished_at` (or now if not finished).
    pub fn run_duration_secs(&self) -> u64 {
        let end = self.finished_at.unwrap_or_else(Utc::now);
        (end - self.started_at).num_seconds().max(0) as u64
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

    /// Capture usage from a Claude call and add it to the appropriate phase.
    ///
    /// This method:
    /// 1. Adds the usage to the specified phase in `phase_usage`
    /// 2. Accumulates the usage into `total_usage`
    ///
    /// If usage is `None`, this is a no-op.
    ///
    /// # Arguments
    /// * `phase_key` - The phase identifier (e.g., "Planning", "US-001", "Final Review", "PR & Commit")
    /// * `usage` - The usage data from the Claude call, or None if not available
    pub fn capture_usage(&mut self, phase_key: &str, usage: Option<ClaudeUsage>) {
        if let Some(usage) = usage {
            // Add to phase_usage
            self.phase_usage
                .entry(phase_key.to_string())
                .and_modify(|existing| existing.add(&usage))
                .or_insert(usage.clone());

            // Accumulate into total_usage
            match &mut self.total_usage {
                Some(existing) => existing.add(&usage),
                None => self.total_usage = Some(usage),
            }
        }
    }

    /// Set usage on the current (last) iteration.
    ///
    /// This stores the usage data in the IterationRecord for per-story tracking.
    /// If usage is `None`, this is a no-op.
    pub fn set_iteration_usage(&mut self, usage: Option<ClaudeUsage>) {
        if let Some(iter) = self.iterations.last_mut() {
            iter.usage = usage;
        }
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
            spec_json_path: Some(state.spec_json_path.clone()),
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
                spec_json_path: Some(state.spec_json_path.clone()),
            }
        } else {
            SessionMetadata {
                session_id: self.session_id.clone(),
                worktree_path,
                branch_name: state.branch.clone(),
                created_at: state.started_at,
                last_active_at: Utc::now(),
                is_running,
                spec_json_path: Some(state.spec_json_path.clone()),
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

    /// Find the most recent session that worked on the specified branch.
    ///
    /// Searches all sessions in the project and returns the one with the most
    /// recent `last_active_at` timestamp that matches the branch name. This is
    /// used by the `improve` command to load accumulated knowledge from previous
    /// runs on the same branch.
    ///
    /// Both worktree sessions and main repo sessions are searched.
    ///
    /// # Arguments
    /// * `branch` - The branch name to search for
    ///
    /// # Returns
    /// * `Ok(Some(metadata))` - Found a session that worked on this branch
    /// * `Ok(None)` - No session found for this branch (graceful degradation)
    /// * `Err` - Error reading session data
    pub fn find_session_for_branch(&self, branch: &str) -> Result<Option<SessionMetadata>> {
        let sessions = self.list_sessions()?;

        // list_sessions() already returns sessions sorted by last_active_at descending,
        // so the first match is the most recent
        for session in sessions {
            if session.branch_name == branch {
                return Ok(Some(session));
            }
        }

        Ok(None)
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
    use tempfile::TempDir;

    // =========================================================================
    // RunState Creation and Transitions
    // =========================================================================

    #[test]
    fn test_run_state_creation_and_defaults() {
        let state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        assert_eq!(state.branch, "test-branch");
        assert_eq!(state.review_iteration, 0);
        assert_eq!(state.machine_state, MachineState::Initializing);
        assert_eq!(state.status, RunStatus::Running);
        assert!(state.config.is_none());
        assert!(state.session_id.is_none());

        let state_with_config = RunState::new_with_config(
            PathBuf::from("test.json"),
            "test-branch".to_string(),
            crate::config::Config::default(),
        );
        assert!(state_with_config.config.is_some());
    }

    #[test]
    fn test_state_transitions() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        // Normal workflow
        state.transition_to(MachineState::PickingStory);
        assert_eq!(state.machine_state, MachineState::PickingStory);
        assert_eq!(state.status, RunStatus::Running);

        state.transition_to(MachineState::RunningClaude);
        state.transition_to(MachineState::Reviewing);
        state.transition_to(MachineState::Correcting);
        state.transition_to(MachineState::Reviewing);
        state.review_iteration = 2;
        assert_eq!(state.review_iteration, 2);

        state.transition_to(MachineState::Committing);
        state.transition_to(MachineState::CreatingPR);
        assert_eq!(state.status, RunStatus::Running);

        state.transition_to(MachineState::Completed);
        assert_eq!(state.status, RunStatus::Completed);

        // Failed transition
        let mut failed = RunState::new(PathBuf::from("test.json"), "branch".to_string());
        failed.transition_to(MachineState::Failed);
        assert_eq!(failed.status, RunStatus::Failed);
    }

    // =========================================================================
    // Serialization
    // =========================================================================

    #[test]
    fn test_serialization_roundtrip() {
        // MachineState
        for (state, expected) in [
            (MachineState::Idle, "\"idle\""),
            (MachineState::RunningClaude, "\"running-claude\""),
            (MachineState::CreatingPR, "\"creating-pr\""),
        ] {
            let json = serde_json::to_string(&state).unwrap();
            assert_eq!(json, expected);
            assert_eq!(serde_json::from_str::<MachineState>(&json).unwrap(), state);
        }

        // RunStatus
        for (status, expected) in [
            (RunStatus::Running, "\"running\""),
            (RunStatus::Completed, "\"completed\""),
        ] {
            let json = serde_json::to_string(&status).unwrap();
            assert_eq!(json, expected);
            assert_eq!(serde_json::from_str::<RunStatus>(&json).unwrap(), status);
        }

        // Full RunState
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.start_iteration("US-001");
        state.set_work_summary(Some("Summary".to_string()));
        let json = serde_json::to_string(&state).unwrap();
        let back: RunState = serde_json::from_str(&json).unwrap();
        assert_eq!(back.branch, state.branch);
    }

    #[test]
    fn test_backwards_compatibility() {
        let legacy = r#"{"number":1,"story_id":"US-001","started_at":"2024-01-01T00:00:00Z","finished_at":null,"status":"running","output_snippet":""}"#;
        let record: IterationRecord = serde_json::from_str(legacy).unwrap();
        assert!(record.work_summary.is_none());
    }

    // =========================================================================
    // Iteration Management
    // =========================================================================

    #[test]
    fn test_iteration_management() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.start_iteration("US-001");
        assert!(state.iterations[0].work_summary.is_none());

        state.set_work_summary(Some("Feature".to_string()));
        assert_eq!(
            state.iterations[0].work_summary,
            Some("Feature".to_string())
        );

        state.set_work_summary(None);
        assert!(state.iterations[0].work_summary.is_none());

        // No crash with empty iterations
        let mut empty = RunState::new(PathBuf::from("test.json"), "branch".to_string());
        empty.set_work_summary(Some("Safe".to_string()));
    }

    // =========================================================================
    // StateManager CRUD
    // =========================================================================

    #[test]
    fn test_state_manager_save_load_clear() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        sm.save(&state).unwrap();

        assert!(temp_dir
            .path()
            .join(SESSIONS_DIR)
            .join(MAIN_SESSION_ID)
            .join(STATE_FILE)
            .exists());

        let loaded = sm.load_current().unwrap().unwrap();
        assert_eq!(loaded.branch, "test-branch");

        sm.clear_current().unwrap();
        assert!(sm.load_current().unwrap().is_none());
    }

    #[test]
    fn test_state_manager_archive() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let state = RunState::new(PathBuf::from("test.json"), "feature/test".to_string());
        sm.save(&state).unwrap();

        let archive_path = sm.archive(&state).unwrap();
        assert!(archive_path.exists());

        let archived = sm.list_archived().unwrap();
        assert_eq!(archived.len(), 1);
    }

    #[test]
    fn test_state_manager_directory_structure() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());
        sm.ensure_dirs().unwrap();

        assert!(temp_dir.path().join(RUNS_DIR).is_dir());
        assert!(temp_dir.path().join(SESSIONS_DIR).is_dir());
        // Note: SPEC_DIR is created by ensure_spec_dir(), not ensure_dirs()
    }

    // =========================================================================
    // Session Management
    // =========================================================================

    #[test]
    fn test_session_isolation() {
        let temp_dir = TempDir::new().unwrap();

        let sm1 = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "session-a".to_string(),
        );
        let sm2 = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "session-b".to_string(),
        );

        sm1.save(&RunState::new(
            PathBuf::from("a.json"),
            "branch-a".to_string(),
        ))
        .unwrap();
        sm2.save(&RunState::new(
            PathBuf::from("b.json"),
            "branch-b".to_string(),
        ))
        .unwrap();

        assert_eq!(sm1.load_current().unwrap().unwrap().branch, "branch-a");
        assert_eq!(sm2.load_current().unwrap().unwrap().branch, "branch-b");
        assert_eq!(sm1.list_sessions().unwrap().len(), 2);
    }

    #[test]
    fn test_session_metadata() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        assert!(sm.load_metadata().unwrap().is_none());

        let state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        sm.save(&state).unwrap();

        let metadata = sm.load_metadata().unwrap().unwrap();
        assert!(metadata.is_running);

        let mut completed = sm.load_current().unwrap().unwrap();
        completed.transition_to(MachineState::Completed);
        sm.save(&completed).unwrap();
        assert!(!sm.load_metadata().unwrap().unwrap().is_running);
    }

    // =========================================================================
    // LiveState
    // =========================================================================

    #[test]
    fn test_live_state() {
        let mut live = LiveState::new(MachineState::RunningClaude);
        assert!(live.is_heartbeat_fresh());

        for i in 0..60 {
            live.append_line(format!("line {}", i));
        }
        assert_eq!(live.output_lines.len(), 50); // Max 50

        live.last_heartbeat = chrono::Utc::now() - chrono::Duration::seconds(65);
        assert!(!live.is_heartbeat_fresh());
    }

    #[test]
    fn test_live_state_persistence() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        assert!(sm.load_live().is_none());

        let mut live = LiveState::new(MachineState::RunningClaude);
        live.append_line("output".to_string());
        sm.save_live(&live).unwrap();

        assert!(sm.load_live().is_some());
        sm.clear_live().unwrap();
        assert!(sm.load_live().is_none());
    }

    // =========================================================================
    // Config and Knowledge
    // =========================================================================

    #[test]
    fn test_config_preservation() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let mut config = crate::config::Config::default();
        config.review = false;
        let state =
            RunState::new_with_config(PathBuf::from("test.json"), "branch".to_string(), config);
        sm.save(&state).unwrap();

        assert!(
            !sm.load_current()
                .unwrap()
                .unwrap()
                .effective_config()
                .review
        );
    }

    #[test]
    fn test_knowledge_tracking() {
        let mut state = RunState::new(PathBuf::from("test.json"), "branch".to_string());
        state.knowledge.story_changes.push(StoryChanges {
            story_id: "US-001".to_string(),
            files_created: vec![],
            files_modified: vec![FileChange {
                path: PathBuf::from("src/main.rs"),
                additions: 10,
                deletions: 2,
                purpose: Some("Main entry point".to_string()),
                key_symbols: vec![],
            }],
            files_deleted: vec![],
            commit_hash: None,
        });
        assert!(state
            .knowledge
            .story_changes
            .iter()
            .any(|c| c.story_id == "US-001"));
    }

    // Tests for token usage fields (US-004)

    #[test]
    fn test_iteration_record_usage_initialized_as_none() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.start_iteration("US-001");
        assert!(state.iterations[0].usage.is_none());
    }

    #[test]
    fn test_iteration_record_usage_can_be_set() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.start_iteration("US-001");
        state.iterations[0].usage = Some(ClaudeUsage {
            input_tokens: 1000,
            output_tokens: 500,
            cache_read_tokens: 200,
            cache_creation_tokens: 100,
            thinking_tokens: 50,
            model: Some("claude-sonnet-4-20250514".to_string()),
        });
        assert!(state.iterations[0].usage.is_some());
        assert_eq!(
            state.iterations[0].usage.as_ref().unwrap().input_tokens,
            1000
        );
    }

    #[test]
    fn test_iteration_record_backwards_compatible_without_usage() {
        // Simulate a legacy state.json that doesn't have the usage field
        let legacy_json = r#"{
            "number": 1,
            "story_id": "US-001",
            "started_at": "2024-01-01T00:00:00Z",
            "finished_at": null,
            "status": "running",
            "output_snippet": ""
        }"#;

        let record: IterationRecord = serde_json::from_str(legacy_json).unwrap();
        assert!(record.usage.is_none());
        assert_eq!(record.story_id, "US-001");
    }

    #[test]
    fn test_run_state_total_usage_initialized_as_none() {
        let state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        assert!(state.total_usage.is_none());
    }

    #[test]
    fn test_run_state_phase_usage_initialized_empty() {
        let state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        assert!(state.phase_usage.is_empty());
    }

    #[test]
    fn test_run_state_total_usage_can_be_set() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.total_usage = Some(ClaudeUsage {
            input_tokens: 5000,
            output_tokens: 2500,
            cache_read_tokens: 1000,
            cache_creation_tokens: 500,
            thinking_tokens: 250,
            model: Some("claude-sonnet-4-20250514".to_string()),
        });
        assert!(state.total_usage.is_some());
        assert_eq!(state.total_usage.as_ref().unwrap().total_tokens(), 7500);
    }

    #[test]
    fn test_run_state_phase_usage_can_be_populated() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        // Add usage for various phases
        state.phase_usage.insert(
            "Planning".to_string(),
            ClaudeUsage {
                input_tokens: 1000,
                output_tokens: 500,
                ..Default::default()
            },
        );
        state.phase_usage.insert(
            "US-001".to_string(),
            ClaudeUsage {
                input_tokens: 2000,
                output_tokens: 1000,
                ..Default::default()
            },
        );
        state.phase_usage.insert(
            "Final Review".to_string(),
            ClaudeUsage {
                input_tokens: 500,
                output_tokens: 250,
                ..Default::default()
            },
        );
        state.phase_usage.insert(
            "PR & Commit".to_string(),
            ClaudeUsage {
                input_tokens: 300,
                output_tokens: 150,
                ..Default::default()
            },
        );

        assert_eq!(state.phase_usage.len(), 4);
        assert!(state.phase_usage.contains_key("Planning"));
        assert!(state.phase_usage.contains_key("US-001"));
        assert!(state.phase_usage.contains_key("Final Review"));
        assert!(state.phase_usage.contains_key("PR & Commit"));
    }

    #[test]
    fn test_run_state_backwards_compatible_without_usage_fields() {
        // Simulate a legacy RunState JSON without total_usage and phase_usage fields
        let legacy_json = r#"{
            "run_id": "test-run-id",
            "status": "running",
            "machine_state": "running-claude",
            "spec_json_path": "test.json",
            "branch": "test-branch",
            "current_story": "US-001",
            "iteration": 1,
            "started_at": "2024-01-01T00:00:00Z",
            "finished_at": null,
            "iterations": []
        }"#;

        let state: RunState = serde_json::from_str(legacy_json).unwrap();
        assert!(state.total_usage.is_none());
        assert!(state.phase_usage.is_empty());
        assert_eq!(state.run_id, "test-run-id");
        assert_eq!(state.branch, "test-branch");
    }

    #[test]
    fn test_iteration_record_usage_serialization_roundtrip() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.start_iteration("US-001");
        state.iterations[0].usage = Some(ClaudeUsage {
            input_tokens: 1000,
            output_tokens: 500,
            cache_read_tokens: 200,
            cache_creation_tokens: 100,
            thinking_tokens: 50,
            model: Some("claude-sonnet-4-20250514".to_string()),
        });

        // Serialize
        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("\"inputTokens\":1000"));
        assert!(json.contains("\"outputTokens\":500"));

        // Deserialize
        let deserialized: RunState = serde_json::from_str(&json).unwrap();
        assert!(deserialized.iterations[0].usage.is_some());
        let usage = deserialized.iterations[0].usage.as_ref().unwrap();
        assert_eq!(usage.input_tokens, 1000);
        assert_eq!(usage.output_tokens, 500);
        assert_eq!(usage.cache_read_tokens, 200);
        assert_eq!(usage.cache_creation_tokens, 100);
        assert_eq!(usage.thinking_tokens, 50);
        assert_eq!(usage.model, Some("claude-sonnet-4-20250514".to_string()));
    }

    #[test]
    fn test_run_state_usage_serialization_roundtrip() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.total_usage = Some(ClaudeUsage {
            input_tokens: 5000,
            output_tokens: 2500,
            ..Default::default()
        });
        state.phase_usage.insert(
            "US-001".to_string(),
            ClaudeUsage {
                input_tokens: 2000,
                output_tokens: 1000,
                ..Default::default()
            },
        );

        // Serialize
        let json = serde_json::to_string(&state).unwrap();
        // RunState uses snake_case serialization (no rename_all attribute)
        assert!(json.contains("\"total_usage\""));
        assert!(json.contains("\"phase_usage\""));

        // Deserialize
        let deserialized: RunState = serde_json::from_str(&json).unwrap();
        assert!(deserialized.total_usage.is_some());
        assert_eq!(
            deserialized.total_usage.as_ref().unwrap().input_tokens,
            5000
        );
        assert_eq!(deserialized.phase_usage.len(), 1);
        assert!(deserialized.phase_usage.contains_key("US-001"));
        assert_eq!(
            deserialized.phase_usage.get("US-001").unwrap().input_tokens,
            2000
        );
    }

    #[test]
    fn test_from_spec_constructors_initialize_usage_fields() {
        let state = RunState::from_spec(
            PathBuf::from("spec-feature.md"),
            PathBuf::from("spec-feature.json"),
        );
        assert!(state.total_usage.is_none());
        assert!(state.phase_usage.is_empty());

        let state2 = RunState::from_spec_with_config(
            PathBuf::from("spec-feature.md"),
            PathBuf::from("spec-feature.json"),
            Config::default(),
        );
        assert!(state2.total_usage.is_none());
        assert!(state2.phase_usage.is_empty());
    }

    // ======================================================================
    // Tests for US-005: capture_usage and set_iteration_usage methods
    // ======================================================================

    #[test]
    fn test_capture_usage_first_call_initializes_totals() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        let usage = ClaudeUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 10,
            cache_creation_tokens: 5,
            thinking_tokens: 3,
            model: Some("claude-sonnet-4".to_string()),
        };

        state.capture_usage("Planning", Some(usage.clone()));

        // total_usage should be set
        assert!(state.total_usage.is_some());
        let total = state.total_usage.as_ref().unwrap();
        assert_eq!(total.input_tokens, 100);
        assert_eq!(total.output_tokens, 50);
        assert_eq!(total.cache_read_tokens, 10);

        // phase_usage should have Planning entry
        assert!(state.phase_usage.contains_key("Planning"));
        let planning = state.phase_usage.get("Planning").unwrap();
        assert_eq!(planning.input_tokens, 100);
        assert_eq!(planning.output_tokens, 50);
    }

    #[test]
    fn test_capture_usage_accumulates_into_existing_phase() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        let usage1 = ClaudeUsage {
            input_tokens: 100,
            output_tokens: 50,
            ..Default::default()
        };
        let usage2 = ClaudeUsage {
            input_tokens: 200,
            output_tokens: 100,
            ..Default::default()
        };

        state.capture_usage("Final Review", Some(usage1));
        state.capture_usage("Final Review", Some(usage2));

        // Phase usage should be accumulated
        let review = state.phase_usage.get("Final Review").unwrap();
        assert_eq!(review.input_tokens, 300);
        assert_eq!(review.output_tokens, 150);

        // Total usage should also be accumulated
        let total = state.total_usage.as_ref().unwrap();
        assert_eq!(total.input_tokens, 300);
        assert_eq!(total.output_tokens, 150);
    }

    #[test]
    fn test_capture_usage_multiple_phases() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        state.capture_usage(
            "Planning",
            Some(ClaudeUsage {
                input_tokens: 1000,
                output_tokens: 500,
                ..Default::default()
            }),
        );
        state.capture_usage(
            "US-001",
            Some(ClaudeUsage {
                input_tokens: 2000,
                output_tokens: 1000,
                ..Default::default()
            }),
        );
        state.capture_usage(
            "US-002",
            Some(ClaudeUsage {
                input_tokens: 1500,
                output_tokens: 750,
                ..Default::default()
            }),
        );
        state.capture_usage(
            "Final Review",
            Some(ClaudeUsage {
                input_tokens: 500,
                output_tokens: 250,
                ..Default::default()
            }),
        );
        state.capture_usage(
            "PR & Commit",
            Some(ClaudeUsage {
                input_tokens: 300,
                output_tokens: 150,
                ..Default::default()
            }),
        );

        // Verify all phases are tracked
        assert_eq!(state.phase_usage.len(), 5);
        assert_eq!(
            state.phase_usage.get("Planning").unwrap().input_tokens,
            1000
        );
        assert_eq!(state.phase_usage.get("US-001").unwrap().input_tokens, 2000);
        assert_eq!(state.phase_usage.get("US-002").unwrap().input_tokens, 1500);
        assert_eq!(
            state.phase_usage.get("Final Review").unwrap().input_tokens,
            500
        );
        assert_eq!(
            state.phase_usage.get("PR & Commit").unwrap().input_tokens,
            300
        );

        // Verify total is sum of all phases
        let total = state.total_usage.as_ref().unwrap();
        assert_eq!(total.input_tokens, 1000 + 2000 + 1500 + 500 + 300);
        assert_eq!(total.output_tokens, 500 + 1000 + 750 + 250 + 150);
    }

    #[test]
    fn test_capture_usage_with_none_is_noop() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        state.capture_usage("Planning", None);

        // Should remain unset
        assert!(state.total_usage.is_none());
        assert!(state.phase_usage.is_empty());
    }

    #[test]
    fn test_capture_usage_none_after_some_preserves_existing() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        state.capture_usage(
            "Planning",
            Some(ClaudeUsage {
                input_tokens: 100,
                output_tokens: 50,
                ..Default::default()
            }),
        );

        // Calling with None should not change anything
        state.capture_usage("Planning", None);

        assert_eq!(state.phase_usage.get("Planning").unwrap().input_tokens, 100);
        assert_eq!(state.total_usage.as_ref().unwrap().input_tokens, 100);
    }

    #[test]
    fn test_set_iteration_usage_sets_on_current_iteration() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.start_iteration("US-001");

        let usage = ClaudeUsage {
            input_tokens: 500,
            output_tokens: 250,
            model: Some("claude-sonnet-4".to_string()),
            ..Default::default()
        };

        state.set_iteration_usage(Some(usage.clone()));

        assert!(state.iterations.last().unwrap().usage.is_some());
        let iter_usage = state.iterations.last().unwrap().usage.as_ref().unwrap();
        assert_eq!(iter_usage.input_tokens, 500);
        assert_eq!(iter_usage.output_tokens, 250);
    }

    #[test]
    fn test_set_iteration_usage_with_none_does_not_set() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        state.start_iteration("US-001");

        state.set_iteration_usage(None);

        assert!(state.iterations.last().unwrap().usage.is_none());
    }

    #[test]
    fn test_set_iteration_usage_no_iteration_is_noop() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        // No iteration started, should not panic
        state.set_iteration_usage(Some(ClaudeUsage {
            input_tokens: 100,
            ..Default::default()
        }));

        // No iterations exist
        assert!(state.iterations.is_empty());
    }

    #[test]
    fn test_capture_usage_preserves_model_from_first_call() {
        let mut state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        state.capture_usage(
            "Planning",
            Some(ClaudeUsage {
                input_tokens: 100,
                model: Some("claude-sonnet-4".to_string()),
                ..Default::default()
            }),
        );
        state.capture_usage(
            "Planning",
            Some(ClaudeUsage {
                input_tokens: 200,
                model: Some("claude-opus-4".to_string()),
                ..Default::default()
            }),
        );

        // Model should be preserved from first call (add() preserves existing model)
        let planning = state.phase_usage.get("Planning").unwrap();
        assert_eq!(planning.model, Some("claude-sonnet-4".to_string()));
    }

    // ======================================================================
    // Tests for find_session_for_branch (US-002)
    // ======================================================================

    #[test]
    fn test_find_session_for_branch_returns_none_when_no_sessions() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let result = sm.find_session_for_branch("feature/test").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_find_session_for_branch_returns_none_when_no_match() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "session-1".to_string(),
        );

        // Create a session with a different branch
        let state = RunState::new(PathBuf::from("test.json"), "feature/other".to_string());
        sm.save(&state).unwrap();

        let result = sm.find_session_for_branch("feature/test").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_find_session_for_branch_returns_matching_session() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "session-1".to_string(),
        );

        let state = RunState::new(PathBuf::from("test.json"), "feature/test".to_string());
        sm.save(&state).unwrap();

        let result = sm.find_session_for_branch("feature/test").unwrap();
        assert!(result.is_some());
        let metadata = result.unwrap();
        assert_eq!(metadata.branch_name, "feature/test");
        assert_eq!(metadata.session_id, "session-1");
    }

    #[test]
    fn test_find_session_for_branch_returns_most_recent() {
        let temp_dir = TempDir::new().unwrap();

        // Create two sessions with the same branch, different times
        let sm1 = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "session-old".to_string(),
        );
        let state1 = RunState::new(PathBuf::from("test.json"), "feature/test".to_string());
        sm1.save(&state1).unwrap();

        // Sleep briefly to ensure different timestamps
        std::thread::sleep(std::time::Duration::from_millis(10));

        let sm2 = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "session-new".to_string(),
        );
        let state2 = RunState::new(PathBuf::from("test.json"), "feature/test".to_string());
        sm2.save(&state2).unwrap();

        // Query should return the most recent session
        let result = sm1.find_session_for_branch("feature/test").unwrap();
        assert!(result.is_some());
        let metadata = result.unwrap();
        assert_eq!(metadata.session_id, "session-new");
    }

    #[test]
    fn test_find_session_for_branch_searches_all_sessions() {
        let temp_dir = TempDir::new().unwrap();

        // Create sessions with different branches
        let sm1 = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "session-a".to_string(),
        );
        sm1.save(&RunState::new(
            PathBuf::from("a.json"),
            "feature/a".to_string(),
        ))
        .unwrap();

        let sm2 = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "session-b".to_string(),
        );
        sm2.save(&RunState::new(
            PathBuf::from("b.json"),
            "feature/b".to_string(),
        ))
        .unwrap();

        let sm3 = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            MAIN_SESSION_ID.to_string(),
        );
        sm3.save(&RunState::new(
            PathBuf::from("main.json"),
            "feature/main".to_string(),
        ))
        .unwrap();

        // Query from any session manager should find the right branch
        let result_a = sm3.find_session_for_branch("feature/a").unwrap();
        assert!(result_a.is_some());
        assert_eq!(result_a.unwrap().session_id, "session-a");

        let result_b = sm1.find_session_for_branch("feature/b").unwrap();
        assert!(result_b.is_some());
        assert_eq!(result_b.unwrap().session_id, "session-b");

        let result_main = sm2.find_session_for_branch("feature/main").unwrap();
        assert!(result_main.is_some());
        assert_eq!(result_main.unwrap().session_id, MAIN_SESSION_ID);
    }

    // ======================================================================
    // Tests for spec_json_path in SessionMetadata (US-003)
    // ======================================================================

    #[test]
    fn test_session_metadata_spec_json_path_defaults_to_none() {
        // Simulate a legacy metadata JSON without spec_json_path field
        let legacy_json = r#"{
            "sessionId": "test-session",
            "worktreePath": "/path/to/worktree",
            "branchName": "feature/test",
            "createdAt": "2024-01-01T00:00:00Z",
            "lastActiveAt": "2024-01-01T01:00:00Z",
            "isRunning": false
        }"#;

        let metadata: SessionMetadata = serde_json::from_str(legacy_json).unwrap();
        assert!(metadata.spec_json_path.is_none());
        assert_eq!(metadata.session_id, "test-session");
        assert_eq!(metadata.branch_name, "feature/test");
    }

    #[test]
    fn test_session_metadata_spec_json_path_serialization_roundtrip() {
        let metadata = SessionMetadata {
            session_id: "test-session".to_string(),
            worktree_path: PathBuf::from("/path/to/worktree"),
            branch_name: "feature/test".to_string(),
            created_at: Utc::now(),
            last_active_at: Utc::now(),
            is_running: false,
            spec_json_path: Some(PathBuf::from("/path/to/spec.json")),
        };

        // Serialize
        let json = serde_json::to_string(&metadata).unwrap();
        assert!(json.contains("\"specJsonPath\""));
        assert!(json.contains("/path/to/spec.json"));

        // Deserialize
        let deserialized: SessionMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(
            deserialized.spec_json_path,
            Some(PathBuf::from("/path/to/spec.json"))
        );
    }

    #[test]
    fn test_save_metadata_populates_spec_json_path() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let state = RunState::new(
            PathBuf::from("/config/spec/spec-feature.json"),
            "feature/test".to_string(),
        );
        sm.save(&state).unwrap();

        let metadata = sm.load_metadata().unwrap().unwrap();
        assert_eq!(
            metadata.spec_json_path,
            Some(PathBuf::from("/config/spec/spec-feature.json"))
        );
    }

    #[test]
    fn test_save_metadata_updates_spec_json_path() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        // First save with one spec
        let state1 = RunState::new(PathBuf::from("spec-v1.json"), "feature/test".to_string());
        sm.save(&state1).unwrap();

        let metadata1 = sm.load_metadata().unwrap().unwrap();
        assert_eq!(
            metadata1.spec_json_path,
            Some(PathBuf::from("spec-v1.json"))
        );

        // Second save with different spec
        let state2 = RunState::new(PathBuf::from("spec-v2.json"), "feature/test".to_string());
        sm.save(&state2).unwrap();

        let metadata2 = sm.load_metadata().unwrap().unwrap();
        assert_eq!(
            metadata2.spec_json_path,
            Some(PathBuf::from("spec-v2.json"))
        );
    }

    #[test]
    fn test_find_session_for_branch_returns_spec_json_path() {
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir_and_session(
            temp_dir.path().to_path_buf(),
            "session-1".to_string(),
        );

        let state = RunState::new(
            PathBuf::from("spec-feature.json"),
            "feature/test".to_string(),
        );
        sm.save(&state).unwrap();

        let result = sm.find_session_for_branch("feature/test").unwrap();
        assert!(result.is_some());
        let metadata = result.unwrap();
        assert_eq!(
            metadata.spec_json_path,
            Some(PathBuf::from("spec-feature.json"))
        );
    }
}
