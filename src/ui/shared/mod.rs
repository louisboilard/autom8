//! Shared data types and logic for UI modules.
//!
//! This module contains common data structures used by both the GUI and TUI,
//! such as run progress, project data, session data, and run history entries.
//!
//! These types are framework-agnostic and can be used by any UI implementation.

use crate::config::{list_projects_tree, ProjectTreeInfo};
use crate::error::Result;
use crate::spec::{Spec, UserStory};
use crate::state::{
    IterationStatus, LiveState, MachineState, RunState, RunStatus, SessionMetadata, StateManager,
};
use crate::worktree::MAIN_SESSION_ID;
use chrono::{DateTime, Utc};
use std::collections::HashSet;
use std::path::PathBuf;

// ============================================================================
// Shared Status Types and Functions
// ============================================================================

/// Semantic status states for consistent status determination across UIs.
///
/// This enum represents the semantic meaning of a run's status, abstracting
/// away the underlying MachineState details. Both GUI and TUI should use
/// these states to ensure consistent behavior.
///
/// The status values are:
/// - `Setup`: Gray - setup/initialization phases (Initializing, PickingStory, LoadingSpec, GeneratingSpec)
/// - `Running`: Blue - active implementation work (RunningClaude)
/// - `Reviewing`: Amber - evaluation phases (Reviewing)
/// - `Correcting`: Orange - attention needed, fixes in progress (Correcting)
/// - `Success`: Green - success path (Committing, CreatingPR, Completed)
/// - `Error`: Red - failure states (Failed)
/// - `Warning`: Amber - general warnings (e.g., stuck sessions)
/// - `Idle`: Gray - inactive state (Idle)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Setup/initialization state - displayed in gray.
    Setup,
    /// Active implementation work - displayed in blue.
    Running,
    /// Evaluation/review phase - displayed in amber.
    Reviewing,
    /// Attention needed, fixes in progress - displayed in orange.
    Correcting,
    /// Success path (committing, PR, completed) - displayed in green.
    Success,
    /// Warning/attention needed - displayed in amber.
    Warning,
    /// Error/failure state - displayed in red.
    Error,
    /// Idle/inactive state - displayed in gray.
    Idle,
}

impl Status {
    /// Convert a MachineState to the appropriate Status.
    ///
    /// Color mapping follows semantic meaning for state phases:
    /// - Setup phases (Initializing, PickingStory, LoadingSpec, GeneratingSpec): Gray
    /// - Active implementation (RunningClaude): Blue
    /// - Evaluation phase (Reviewing): Amber
    /// - Attention needed (Correcting): Orange
    /// - Success path (Committing, CreatingPR, Completed): Green
    /// - Failure (Failed): Red
    /// - Inactive (Idle): Gray
    pub fn from_machine_state(state: MachineState) -> Self {
        match state {
            // Setup phases - gray (preparation work)
            MachineState::Initializing
            | MachineState::PickingStory
            | MachineState::LoadingSpec
            | MachineState::GeneratingSpec => Status::Setup,

            // Active implementation - blue
            MachineState::RunningClaude => Status::Running,

            // Evaluation phase - amber
            MachineState::Reviewing => Status::Reviewing,

            // Attention needed - orange
            MachineState::Correcting => Status::Correcting,

            // Success path - green
            MachineState::Committing | MachineState::CreatingPR | MachineState::Completed => {
                Status::Success
            }

            // Failure - red
            MachineState::Failed => Status::Error,

            // Inactive - gray
            MachineState::Idle => Status::Idle,
        }
    }
}

/// Format a MachineState as a human-readable label.
///
/// This is the canonical state label formatting used by both GUI and TUI
/// to ensure consistent state display across the application.
pub fn format_state_label(state: MachineState) -> &'static str {
    match state {
        MachineState::Idle => "Idle",
        MachineState::LoadingSpec => "Loading Spec",
        MachineState::GeneratingSpec => "Generating Spec",
        MachineState::Initializing => "Initializing",
        MachineState::PickingStory => "Picking Story",
        MachineState::RunningClaude => "Running Claude",
        MachineState::Reviewing => "Reviewing",
        MachineState::Correcting => "Correcting",
        MachineState::Committing => "Committing",
        MachineState::CreatingPR => "Creating PR",
        MachineState::Completed => "Completed",
        MachineState::Failed => "Failed",
    }
}

/// Format a duration from a start time as a human-readable string.
///
/// Examples: "5s", "2m 30s", "1h 5m"
///
/// This is the canonical duration formatting used by both GUI and TUI.
pub fn format_duration(started_at: DateTime<Utc>) -> String {
    let now = Utc::now();
    let duration = now.signed_duration_since(started_at);
    format_duration_secs(duration.num_seconds().max(0) as u64)
}

/// Format a duration in seconds as a human-readable string.
///
/// - Durations under 1 minute show only seconds
/// - Durations between 1-60 minutes show minutes and seconds
/// - Durations over 1 hour show hours and minutes (no seconds)
pub fn format_duration_secs(total_secs: u64) -> String {
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, seconds)
    } else {
        format!("{}s", seconds)
    }
}

/// Format a timestamp as a relative time string.
///
/// Examples: "just now", "5m ago", "2h ago", "3d ago"
///
/// This is the canonical relative time formatting used by both GUI and TUI.
pub fn format_relative_time(timestamp: DateTime<Utc>) -> String {
    let now = Utc::now();
    let duration = now.signed_duration_since(timestamp);
    format_relative_time_secs(duration.num_seconds().max(0) as u64)
}

/// Format a relative time from seconds ago.
pub fn format_relative_time_secs(total_secs: u64) -> String {
    let minutes = total_secs / 60;
    let hours = total_secs / 3600;
    let days = total_secs / 86400;

    if days > 0 {
        format!("{}d ago", days)
    } else if hours > 0 {
        format!("{}h ago", hours)
    } else if minutes > 0 {
        format!("{}m ago", minutes)
    } else {
        "just now".to_string()
    }
}

// ============================================================================
// Shared Data Types
// ============================================================================

/// Progress information for a run.
///
/// This is the canonical progress struct used by both GUI and TUI.
/// It provides methods for calculating and formatting progress values.
#[derive(Debug, Clone, Copy)]
pub struct RunProgress {
    /// Number of completed stories.
    pub completed: usize,
    /// Total number of stories.
    pub total: usize,
}

impl RunProgress {
    /// Create a new RunProgress instance.
    pub fn new(completed: usize, total: usize) -> Self {
        Self { completed, total }
    }

    /// Calculate the progress as a fraction between 0.0 and 1.0.
    pub fn fraction(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (self.completed as f32) / (self.total as f32)
        }
    }

    /// Format progress as a story fraction string (e.g., "Story 2/5").
    /// The current story number is completed + 1 (1-indexed), but capped at total
    /// to avoid displaying impossible values like "Story 8/7" at completion.
    pub fn as_fraction(&self) -> String {
        let current = if self.completed < self.total {
            self.completed + 1
        } else {
            self.total
        };
        format!("Story {}/{}", current, self.total)
    }

    /// Alias for `as_fraction()` for clarity when the story context is explicit.
    pub fn as_story_fraction(&self) -> String {
        self.as_fraction()
    }

    /// Format progress as a simple fraction (e.g., "2/5").
    pub fn as_simple_fraction(&self) -> String {
        format!("{}/{}", self.completed, self.total)
    }

    /// Format progress as a percentage (e.g., "40%").
    pub fn as_percentage(&self) -> String {
        if self.total == 0 {
            return "0%".to_string();
        }
        let pct = (self.completed * 100) / self.total;
        format!("{}%", pct)
    }
}

/// Data collected from a single project for display.
#[derive(Debug, Clone)]
pub struct ProjectData {
    /// Project metadata from the tree.
    pub info: ProjectTreeInfo,
    /// The active run state (if any).
    pub active_run: Option<RunState>,
    /// Progress through the spec (loaded from spec file).
    pub progress: Option<RunProgress>,
    /// Error message if state file is corrupted or unreadable.
    pub load_error: Option<String>,
}

/// Data for a single session in the Active Runs view.
///
/// This struct represents one running session, which can be from
/// the main repo or a worktree. Multiple sessions can belong to
/// the same project (when using worktree mode).
#[derive(Debug, Clone)]
pub struct SessionData {
    /// Project name (e.g., "autom8").
    pub project_name: String,
    /// Session metadata (includes session_id, worktree_path, branch).
    pub metadata: SessionMetadata,
    /// The active run state for this session.
    pub run: Option<RunState>,
    /// Progress through the spec (loaded from spec file).
    pub progress: Option<RunProgress>,
    /// Error message if state file is corrupted or unreadable.
    pub load_error: Option<String>,
    /// Whether this is the main repo session (vs. a worktree).
    pub is_main_session: bool,
    /// Whether this session is stale (worktree was deleted).
    pub is_stale: bool,
    /// Live output state for streaming Claude output (from live.json).
    pub live_output: Option<LiveState>,
    /// Cached user stories from the spec (to avoid file I/O on every render frame).
    /// This is populated during `load_sessions()` and should be used by `load_story_items()`.
    pub cached_user_stories: Option<Vec<UserStory>>,
}

impl SessionData {
    /// Format the display title for this session.
    /// Returns "project-name (main)" or "project-name (abc12345)".
    pub fn display_title(&self) -> String {
        if self.is_main_session {
            format!("{} (main)", self.project_name)
        } else {
            format!("{} ({})", self.project_name, &self.metadata.session_id)
        }
    }

    /// Check if this session has a fresh heartbeat (run is actively progressing).
    ///
    /// A session is considered "alive" if:
    /// - It has live output data AND
    /// - The heartbeat is recent (< 10 seconds old)
    ///
    /// This is the authoritative check for whether a run is actively progressing.
    /// The GUI/TUI should use this to determine if a run is truly active,
    /// rather than just checking `is_running` from metadata.
    pub fn has_fresh_heartbeat(&self) -> bool {
        self.live_output
            .as_ref()
            .map(|live| live.is_heartbeat_fresh())
            .unwrap_or(false)
    }

    /// Check if this session should be considered actively running.
    ///
    /// A session is actively running if:
    /// - It's not stale (worktree exists) AND
    /// - It's marked as running AND
    /// - It either has a fresh heartbeat OR there's no live data yet (run just started)
    ///
    /// This provides a lenient check that accounts for runs that just started
    /// and haven't written live.json yet.
    pub fn is_actively_running(&self) -> bool {
        if self.is_stale || !self.metadata.is_running {
            return false;
        }

        // If we have live output, check the heartbeat
        // If no live output yet, trust the is_running flag (run may have just started)
        self.live_output
            .as_ref()
            .map(|live| live.is_heartbeat_fresh())
            .unwrap_or(true) // Trust is_running if no live data yet
    }

    /// Check if this session appears to be stuck (marked as running but heartbeat is stale).
    ///
    /// This helps identify crashed or stuck runs that need user intervention.
    /// Returns true if:
    /// - Session is marked as running AND
    /// - Live output exists AND
    /// - Heartbeat is stale (> 10 seconds old)
    pub fn appears_stuck(&self) -> bool {
        if !self.metadata.is_running || self.is_stale {
            return false;
        }

        // If we have live output and heartbeat is stale, session appears stuck
        self.live_output
            .as_ref()
            .map(|live| !live.is_heartbeat_fresh())
            .unwrap_or(false) // No live output = can't determine stuck state
    }

    /// Get a truncated worktree path for display (last 2 components).
    pub fn truncated_worktree_path(&self) -> String {
        let path = &self.metadata.worktree_path;
        let components: Vec<_> = path.components().collect();
        if components.len() <= 2 {
            path.display().to_string()
        } else {
            let last_two: PathBuf = components[components.len() - 2..].iter().collect();
            format!(".../{}", last_two.display())
        }
    }
}

/// Data for a single entry in the run history panel.
///
/// Represents an archived run for a project, displayed in the history view.
/// This is the canonical definition used by both GUI and TUI.
#[derive(Debug, Clone)]
pub struct RunHistoryEntry {
    /// The project this run belongs to (used by TUI for grouping).
    pub project_name: String,
    /// The run ID.
    pub run_id: String,
    /// When the run started.
    pub started_at: chrono::DateTime<chrono::Utc>,
    /// When the run finished (if completed).
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    /// The run status (completed/failed/running).
    pub status: RunStatus,
    /// Number of completed stories.
    pub completed_stories: usize,
    /// Total number of stories in the spec.
    pub total_stories: usize,
    /// Branch name for this run.
    pub branch: String,
}

impl RunHistoryEntry {
    /// Create a RunHistoryEntry from a RunState with explicit story counts.
    ///
    /// Use this constructor when you have the story counts already computed.
    pub fn new(
        project_name: String,
        run: &RunState,
        completed_stories: usize,
        total_stories: usize,
    ) -> Self {
        Self {
            project_name,
            run_id: run.run_id.clone(),
            started_at: run.started_at,
            finished_at: run.finished_at,
            status: run.status,
            completed_stories,
            total_stories,
            branch: run.branch.clone(),
        }
    }

    /// Create a RunHistoryEntry from a RunState, computing story counts from iterations.
    ///
    /// This method computes completed/total stories by analyzing the run's iterations.
    /// Use `new()` if you already have the story counts.
    pub fn from_run_state(project_name: String, run: &RunState) -> Self {
        // Count completed stories by looking at iterations with status Success
        let completed_stories = run
            .iterations
            .iter()
            .filter(|i| i.status == IterationStatus::Success)
            .map(|i| &i.story_id)
            .collect::<HashSet<_>>()
            .len();

        // Total stories is harder to determine from archived state
        // Use the iteration count as a proxy (each story should have at least one iteration)
        let story_ids: HashSet<_> = run.iterations.iter().map(|i| &i.story_id).collect();
        let total_stories = story_ids.len().max(1);

        Self {
            project_name,
            run_id: run.run_id.clone(),
            started_at: run.started_at,
            finished_at: run.finished_at,
            status: run.status,
            completed_stories,
            total_stories,
            branch: run.branch.clone(),
        }
    }

    /// Format the story count as "X/Y stories".
    pub fn story_count_text(&self) -> String {
        format!("{}/{} stories", self.completed_stories, self.total_stories)
    }

    /// Format the run status as a display string.
    pub fn status_text(&self) -> &'static str {
        match self.status {
            RunStatus::Completed => "Completed",
            RunStatus::Failed => "Failed",
            RunStatus::Running => "Running",
            RunStatus::Interrupted => "Interrupted",
        }
    }
}

// ============================================================================
// Shared Data Loading
// ============================================================================

/// Result of loading UI data from disk.
///
/// This struct contains all the data needed to populate the UI views,
/// including projects, sessions, run history, and status flags.
#[derive(Debug, Clone, Default)]
pub struct UiData {
    /// List of projects with their active run state.
    pub projects: Vec<ProjectData>,
    /// List of active sessions across all projects.
    pub sessions: Vec<SessionData>,
    /// Whether there are any active runs.
    pub has_active_runs: bool,
}

/// Options for loading run history.
#[derive(Debug, Clone, Default)]
pub struct RunHistoryOptions {
    /// Filter to a specific project (overrides project_filter from UiData load).
    pub project_filter: Option<String>,
    /// Maximum number of entries to return.
    pub max_entries: Option<usize>,
}

/// Result of loading run history.
#[derive(Debug, Clone, Default)]
pub struct RunHistoryData {
    /// List of run history entries.
    pub entries: Vec<RunHistoryEntry>,
    /// Full RunState objects for detail views (keyed by run_id).
    /// Only populated when `include_full_state` is true.
    pub run_states: std::collections::HashMap<String, RunState>,
}

/// Load UI data from disk.
///
/// This function consolidates the data loading logic used by both GUI and TUI.
/// It loads the project tree, filters by project name if specified, loads
/// active run states and session information.
///
/// # Arguments
/// * `project_filter` - Optional project name to filter results
///
/// # Returns
/// * `Result<UiData>` - The loaded data or an error
///
/// # Error Handling
/// This function returns `Result` and lets callers decide how to handle errors.
/// For GUI (which swallows errors), call `.unwrap_or_default()`.
/// For TUI (which propagates errors), use the `?` operator.
pub fn load_ui_data(project_filter: Option<&str>) -> Result<UiData> {
    // Load sessions first - this is critical for Active Runs detection
    // and doesn't depend on git or StateManager
    let sessions = load_sessions(project_filter);

    // Determine if there are active runs
    let has_active_runs = !sessions.is_empty();

    // Load projects for the Projects view (may fail if git issues, but that's ok)
    // This uses StateManager which can spawn git subprocesses
    let projects = match list_projects_tree() {
        Ok(tree_infos) => {
            let filtered: Vec<_> = if let Some(filter) = project_filter {
                tree_infos
                    .into_iter()
                    .filter(|p| p.name == filter)
                    .collect()
            } else {
                tree_infos
            };
            filtered.iter().map(load_project_data).collect()
        }
        Err(_) => {
            // If project loading fails (e.g., git subprocess issues),
            // return empty projects but still return sessions
            Vec::new()
        }
    };

    Ok(UiData {
        projects,
        sessions,
        has_active_runs,
    })
}

/// Load project data for a single project.
fn load_project_data(info: &ProjectTreeInfo) -> ProjectData {
    let (active_run, load_error) = if info.has_active_run {
        match StateManager::for_project(&info.name) {
            Ok(sm) => match sm.load_current() {
                Ok(run) => (run, None),
                Err(e) => (None, Some(format!("Corrupted state: {}", e))),
            },
            Err(e) => (None, Some(format!("State error: {}", e))),
        }
    } else {
        (None, None)
    };

    // Load spec to get progress information
    let progress = active_run.as_ref().and_then(|run| {
        Spec::load(&run.spec_json_path)
            .ok()
            .map(|spec| RunProgress {
                completed: spec.completed_count(),
                total: spec.total_count(),
            })
    });

    ProjectData {
        info: info.clone(),
        active_run,
        progress,
        load_error,
    }
}

/// Load sessions for the Active Runs view.
///
/// Directly reads session metadata files from disk without going through
/// StateManager, avoiding git subprocess spawning and other overhead.
/// This makes session detection reliable regardless of where the UI runs from.
fn load_sessions(project_filter: Option<&str>) -> Vec<SessionData> {
    let mut sessions: Vec<SessionData> = Vec::new();

    // Get the base config directory (~/.config/autom8/)
    let base_dir = match crate::config::config_dir() {
        Ok(dir) => dir,
        Err(_) => return sessions,
    };

    if !base_dir.exists() {
        return sessions;
    }

    // List all project directories
    let project_dirs = match std::fs::read_dir(&base_dir) {
        Ok(entries) => entries,
        Err(_) => return sessions,
    };

    for entry in project_dirs.filter_map(|e| e.ok()) {
        let project_path = entry.path();
        if !project_path.is_dir() {
            continue;
        }

        let project_name = match project_path.file_name().and_then(|n| n.to_str()) {
            Some(name) => name.to_string(),
            None => continue,
        };

        // Apply project filter if specified
        if let Some(filter) = project_filter {
            if project_name != filter {
                continue;
            }
        }

        // Look for sessions directory
        let sessions_dir = project_path.join("sessions");
        if !sessions_dir.exists() {
            continue;
        }

        // List all session directories
        let session_dirs = match std::fs::read_dir(&sessions_dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for session_entry in session_dirs.filter_map(|e| e.ok()) {
            let session_path = session_entry.path();
            if !session_path.is_dir() {
                continue;
            }

            // Read metadata.json directly
            let metadata_path = session_path.join("metadata.json");
            let metadata: SessionMetadata = match std::fs::read_to_string(&metadata_path) {
                Ok(content) => match serde_json::from_str(&content) {
                    Ok(m) => m,
                    Err(_) => continue, // Skip malformed metadata
                },
                Err(_) => continue, // Skip if can't read
            };

            // Skip non-running sessions
            if !metadata.is_running {
                continue;
            }

            // Check if worktree was deleted (stale session)
            let is_stale = !metadata.worktree_path.exists();
            let is_main_session = metadata.session_id == MAIN_SESSION_ID;

            // For stale sessions, add with error and skip state loading
            if is_stale {
                sessions.push(SessionData {
                    project_name: project_name.clone(),
                    metadata,
                    run: None,
                    progress: None,
                    load_error: Some("Worktree has been deleted".to_string()),
                    is_main_session,
                    is_stale: true,
                    live_output: None,
                    cached_user_stories: None,
                });
                continue;
            }

            // Read state.json directly
            let state_path = session_path.join("state.json");
            let (run, load_error): (Option<RunState>, Option<String>) =
                match std::fs::read_to_string(&state_path) {
                    Ok(content) => match serde_json::from_str(&content) {
                        Ok(state) => (Some(state), None),
                        Err(e) => (None, Some(format!("Corrupted state: {}", e))),
                    },
                    Err(_) => (None, Some("State file not found".to_string())),
                };

            // Read live.json directly (optional, for output display)
            let live_path = session_path.join("live.json");
            let live_output: Option<LiveState> = std::fs::read_to_string(&live_path)
                .ok()
                .and_then(|content| serde_json::from_str(&content).ok());

            // Load spec to get progress information and cache user stories
            let (progress, cached_user_stories) = run
                .as_ref()
                .and_then(|r| Spec::load(&r.spec_json_path).ok())
                .map(|spec| {
                    let progress = RunProgress {
                        completed: spec.completed_count(),
                        total: spec.total_count(),
                    };
                    (Some(progress), Some(spec.user_stories))
                })
                .unwrap_or((None, None));

            sessions.push(SessionData {
                project_name: project_name.clone(),
                metadata,
                run,
                progress,
                load_error,
                is_main_session,
                is_stale: false,
                live_output,
                cached_user_stories,
            });
        }
    }

    // Sort sessions by last_active_at descending
    sessions.sort_by(|a, b| b.metadata.last_active_at.cmp(&a.metadata.last_active_at));

    sessions
}

/// Load a single session by project name and session ID.
///
/// Unlike `load_sessions`, this does NOT filter by `is_running`, so it can
/// retrieve sessions that have completed (where `is_running` became false).
/// This is useful for updating the GUI's `seen_sessions` cache when a run
/// completes and disappears from the running sessions list.
///
/// # Arguments
/// * `project_name` - The project to look in
/// * `session_id` - The session ID to load
///
/// # Returns
/// * `Option<SessionData>` - The session data if found, None otherwise
pub fn load_session_by_id(project_name: &str, session_id: &str) -> Option<SessionData> {
    // Get the base config directory (~/.config/autom8/)
    let base_dir = crate::config::config_dir().ok()?;
    let session_path = base_dir
        .join(project_name)
        .join("sessions")
        .join(session_id);

    if !session_path.is_dir() {
        return None;
    }

    // Read metadata.json directly
    let metadata_path = session_path.join("metadata.json");
    let metadata: SessionMetadata = std::fs::read_to_string(&metadata_path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())?;

    // Check if worktree was deleted (stale session)
    let is_stale = !metadata.worktree_path.exists();
    let is_main_session = metadata.session_id == MAIN_SESSION_ID;

    // Read state.json directly
    let state_path = session_path.join("state.json");
    let (run, load_error): (Option<RunState>, Option<String>) =
        match std::fs::read_to_string(&state_path) {
            Ok(content) => match serde_json::from_str(&content) {
                Ok(state) => (Some(state), None),
                Err(e) => (None, Some(format!("Corrupted state: {}", e))),
            },
            Err(_) => (None, Some("State file not found".to_string())),
        };

    // Read live.json directly (optional, for output display)
    let live_path = session_path.join("live.json");
    let live_output: Option<LiveState> = std::fs::read_to_string(&live_path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok());

    // Load spec to get progress information and cache user stories
    let (progress, cached_user_stories) = run
        .as_ref()
        .and_then(|r| Spec::load(&r.spec_json_path).ok())
        .map(|spec| {
            let progress = RunProgress {
                completed: spec.completed_count(),
                total: spec.total_count(),
            };
            (Some(progress), Some(spec.user_stories))
        })
        .unwrap_or((None, None));

    Some(SessionData {
        project_name: project_name.to_string(),
        metadata,
        run,
        progress,
        load_error,
        is_main_session,
        is_stale,
        live_output,
        cached_user_stories,
    })
}

/// Load an archived run by run_id from a project's runs directory.
///
/// This is useful for retrieving the final state of a run after it completes
/// and the session files have been cleaned up.
///
/// # Arguments
/// * `project_name` - The project to look in
/// * `run_id` - The run ID to find
///
/// # Returns
/// * `Option<RunState>` - The archived run state if found
pub fn load_archived_run(project_name: &str, run_id: &str) -> Option<RunState> {
    let sm = StateManager::for_project(project_name).ok()?;
    let archived = sm.list_archived().ok()?;
    archived.into_iter().find(|r| r.run_id == run_id)
}

/// Load run history for display.
///
/// This function loads archived runs and converts them to RunHistoryEntry format.
/// Optionally populates a cache of full RunState objects for detail views.
///
/// # Arguments
/// * `projects` - List of projects to load history from
/// * `options` - Options controlling filtering and limits
/// * `include_full_state` - Whether to include full RunState objects in the result
///
/// # Returns
/// * `Result<RunHistoryData>` - The loaded history data
pub fn load_run_history(
    projects: &[ProjectData],
    options: &RunHistoryOptions,
    include_full_state: bool,
) -> Result<RunHistoryData> {
    let mut history: Vec<RunHistoryEntry> = Vec::new();
    let mut run_states: std::collections::HashMap<String, RunState> =
        std::collections::HashMap::new();

    // Determine which projects to load history from
    let project_names: Vec<String> = if let Some(ref filter) = options.project_filter {
        vec![filter.clone()]
    } else {
        projects.iter().map(|p| p.info.name.clone()).collect()
    };

    // Load archived runs from each project
    for project_name in project_names {
        if let Ok(sm) = StateManager::for_project(&project_name) {
            if let Ok(archived) = sm.list_archived() {
                for run in archived {
                    // Try to load the spec to get story counts
                    let (completed, total) = Spec::load(&run.spec_json_path)
                        .map(|spec| (spec.completed_count(), spec.total_count()))
                        .unwrap_or_else(|_| {
                            // Fallback: count from iterations
                            let completed = run
                                .iterations
                                .iter()
                                .filter(|i| i.status == IterationStatus::Success)
                                .count();
                            (completed, run.iterations.len().max(completed))
                        });

                    // Cache the full run state if requested
                    if include_full_state {
                        run_states.insert(run.run_id.clone(), run.clone());
                    }

                    history.push(RunHistoryEntry::new(
                        project_name.clone(),
                        &run,
                        completed,
                        total,
                    ));
                }
            }
        }
    }

    // Sort by date, most recent first
    history.sort_by(|a, b| b.started_at.cmp(&a.started_at));

    // Apply limit if specified
    if let Some(max) = options.max_entries {
        history.truncate(max);
    }

    Ok(RunHistoryData {
        entries: history,
        run_states,
    })
}

/// Load run history for a single project (simplified API for GUI).
///
/// This is a convenience wrapper around `load_run_history` for cases where
/// you want to load history for a single project and don't need full state.
///
/// # Arguments
/// * `project_name` - The project to load history from
///
/// # Returns
/// * `Result<Vec<RunHistoryEntry>>` - The loaded history entries
pub fn load_project_run_history(project_name: &str) -> Result<Vec<RunHistoryEntry>> {
    let mut history: Vec<RunHistoryEntry> = Vec::new();

    let sm = StateManager::for_project(project_name)?;
    let archived = sm.list_archived()?;

    for run in archived {
        history.push(RunHistoryEntry::from_run_state(
            project_name.to_string(),
            &run,
        ));
    }

    // Sort with running sessions at top, then by date descending
    history.sort_by(|a, b| {
        // First priority: running status at top
        let a_running = matches!(a.status, RunStatus::Running);
        let b_running = matches!(b.status, RunStatus::Running);

        match (a_running, b_running) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            // Both same category: sort by started_at descending (newest first)
            _ => b.started_at.cmp(&a.started_at),
        }
    });

    Ok(history)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::path::PathBuf;

    // =========================================================================
    // RunProgress Tests
    // =========================================================================

    #[test]
    fn test_run_progress_formatting() {
        // Fraction display
        assert_eq!(RunProgress::new(1, 5).as_fraction(), "Story 2/5");
        assert_eq!(RunProgress::new(0, 5).as_fraction(), "Story 1/5");
        assert_eq!(RunProgress::new(5, 5).as_fraction(), "Story 5/5"); // Completed
        assert_eq!(RunProgress::new(0, 0).as_fraction(), "Story 0/0"); // Empty

        // Percentage display
        assert_eq!(RunProgress::new(2, 5).as_percentage(), "40%");
        assert_eq!(RunProgress::new(5, 5).as_percentage(), "100%");
        assert_eq!(RunProgress::new(0, 0).as_percentage(), "0%");

        // Numeric fraction
        assert!((RunProgress::new(2, 5).fraction() - 0.4).abs() < 0.001);
        assert_eq!(RunProgress::new(0, 0).fraction(), 0.0);

        // Simple fraction
        assert_eq!(RunProgress::new(2, 5).as_simple_fraction(), "2/5");
    }

    // =========================================================================
    // SessionData Tests
    // =========================================================================

    fn make_test_session(is_main: bool, is_running: bool, is_stale: bool) -> SessionData {
        SessionData {
            project_name: "test-project".to_string(),
            metadata: SessionMetadata {
                session_id: if is_main { "main" } else { "abc123" }.to_string(),
                worktree_path: PathBuf::from("/path/to/repo"),
                branch_name: "test-branch".to_string(),
                created_at: Utc::now(),
                last_active_at: Utc::now(),
                is_running,
            },
            run: None,
            progress: None,
            load_error: None,
            is_main_session: is_main,
            is_stale,
            live_output: None,
            cached_user_stories: None,
        }
    }

    #[test]
    fn test_session_data_display_and_paths() {
        let main = make_test_session(true, false, false);
        assert_eq!(main.display_title(), "test-project (main)");

        let worktree = make_test_session(false, false, false);
        assert_eq!(worktree.display_title(), "test-project (abc123)");

        // Truncated path (short)
        let mut short_path = make_test_session(false, false, false);
        short_path.metadata.worktree_path = PathBuf::from("repo");
        assert_eq!(short_path.truncated_worktree_path(), "repo");

        // Truncated path (long)
        let mut long_path = make_test_session(false, false, false);
        long_path.metadata.worktree_path = PathBuf::from("/home/user/projects/repo");
        assert_eq!(long_path.truncated_worktree_path(), ".../projects/repo");
    }

    #[test]
    fn test_session_heartbeat_and_status() {
        // No live output = no fresh heartbeat
        let no_live = make_test_session(true, true, false);
        assert!(!no_live.has_fresh_heartbeat());
        assert!(no_live.is_actively_running()); // Trust is_running

        // Fresh live output
        let mut fresh = make_test_session(true, true, false);
        fresh.live_output = Some(LiveState::new(MachineState::RunningClaude));
        assert!(fresh.has_fresh_heartbeat());
        assert!(!fresh.appears_stuck());

        // Stale session never actively running
        let stale = make_test_session(false, true, true);
        assert!(!stale.is_actively_running());

        // Stale heartbeat = appears stuck
        let mut stuck = make_test_session(true, true, false);
        let mut stale_live = LiveState::new(MachineState::RunningClaude);
        stale_live.last_heartbeat = Utc::now() - chrono::Duration::seconds(65);
        stuck.live_output = Some(stale_live);
        assert!(stuck.appears_stuck());

        // Not running = not stuck
        let not_running = make_test_session(true, false, false);
        assert!(!not_running.appears_stuck());
    }

    // =========================================================================
    // Status Mapping Tests
    // =========================================================================

    #[test]
    fn test_status_from_machine_state() {
        // Setup phases
        assert_eq!(
            Status::from_machine_state(MachineState::Initializing),
            Status::Setup
        );
        assert_eq!(
            Status::from_machine_state(MachineState::PickingStory),
            Status::Setup
        );
        assert_eq!(
            Status::from_machine_state(MachineState::LoadingSpec),
            Status::Setup
        );

        // Work phases
        assert_eq!(
            Status::from_machine_state(MachineState::RunningClaude),
            Status::Running
        );
        assert_eq!(
            Status::from_machine_state(MachineState::Reviewing),
            Status::Reviewing
        );
        assert_eq!(
            Status::from_machine_state(MachineState::Correcting),
            Status::Correcting
        );

        // Success phases
        assert_eq!(
            Status::from_machine_state(MachineState::Committing),
            Status::Success
        );
        assert_eq!(
            Status::from_machine_state(MachineState::Completed),
            Status::Success
        );

        // Terminal
        assert_eq!(
            Status::from_machine_state(MachineState::Failed),
            Status::Error
        );
        assert_eq!(Status::from_machine_state(MachineState::Idle), Status::Idle);
    }

    // =========================================================================
    // Duration Formatting Tests
    // =========================================================================

    #[test]
    fn test_duration_formatting() {
        assert_eq!(format_duration_secs(30), "30s");
        assert_eq!(format_duration_secs(125), "2m 5s");
        assert_eq!(format_duration_secs(3600), "1h 0m");
        assert_eq!(format_duration_secs(7265), "2h 1m");
    }

    #[test]
    fn test_relative_time_formatting() {
        assert_eq!(format_relative_time_secs(30), "just now");
        assert_eq!(format_relative_time_secs(300), "5m ago");
        assert_eq!(format_relative_time_secs(3600), "1h ago");
        assert_eq!(format_relative_time_secs(86400), "1d ago");
    }

    // =========================================================================
    // Run History Entry Tests
    // =========================================================================

    #[test]
    fn test_run_history_entry() {
        let entry = RunHistoryEntry {
            project_name: "test-project".to_string(),
            run_id: "test-run".to_string(),
            started_at: Utc::now(),
            finished_at: None,
            status: RunStatus::Completed,
            completed_stories: 3,
            total_stories: 5,
            branch: "feature/test".to_string(),
        };
        assert_eq!(entry.status_text(), "Completed");
        assert_eq!(entry.story_count_text(), "3/5 stories");
    }

    fn make_history_entry(run_id: &str, status: RunStatus, age_secs: i64) -> RunHistoryEntry {
        RunHistoryEntry {
            project_name: "test".to_string(),
            run_id: run_id.to_string(),
            started_at: Utc::now() - chrono::Duration::seconds(age_secs),
            finished_at: None,
            status,
            completed_stories: 0,
            total_stories: 5,
            branch: "test".to_string(),
        }
    }

    #[test]
    fn test_run_history_sorting() {
        let mut history = vec![
            make_history_entry("completed-old", RunStatus::Completed, 60),
            make_history_entry("running", RunStatus::Running, 3600),
            make_history_entry("completed-new", RunStatus::Completed, 0),
        ];

        history.sort_by(|a, b| {
            let a_running = matches!(a.status, RunStatus::Running);
            let b_running = matches!(b.status, RunStatus::Running);
            match (a_running, b_running) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => b.started_at.cmp(&a.started_at),
            }
        });

        // Running first, then by date
        assert_eq!(history[0].run_id, "running");
        assert_eq!(history[1].run_id, "completed-new");
        assert_eq!(history[2].run_id, "completed-old");
    }
}
