//! Shared data types and logic for UI modules.
//!
//! This module contains common data structures used by both the GUI and TUI,
//! such as run progress, project data, session data, and run history entries.
//!
//! These types are framework-agnostic and can be used by any UI implementation.

use crate::config::{list_projects_tree, ProjectTreeInfo};
use crate::error::Result;
use crate::spec::Spec;
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
    // Load projects (returns error if config directory is inaccessible)
    let tree_infos = list_projects_tree()?;

    // Filter by project if specified
    let filtered: Vec<_> = if let Some(filter) = project_filter {
        tree_infos
            .into_iter()
            .filter(|p| p.name == filter)
            .collect()
    } else {
        tree_infos
    };

    // Collect project data including active runs and progress
    let projects: Vec<ProjectData> = filtered.iter().map(load_project_data).collect();

    // Collect sessions for Active Runs view
    let sessions = load_sessions(&filtered, project_filter);

    // Determine if there are active runs
    let has_active_runs = !sessions.is_empty();

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
/// Collects all running sessions across all projects, filtering out
/// stale sessions (where the worktree no longer exists).
fn load_sessions(
    project_infos: &[ProjectTreeInfo],
    project_filter: Option<&str>,
) -> Vec<SessionData> {
    let mut sessions: Vec<SessionData> = Vec::new();

    // Get all project names to check
    let project_names: Vec<_> = if let Some(filter) = project_filter {
        vec![filter.to_string()]
    } else {
        project_infos.iter().map(|p| p.name.clone()).collect()
    };

    for project_name in project_names {
        // Get the StateManager for this project
        let sm = match StateManager::for_project(&project_name) {
            Ok(sm) => sm,
            Err(_) => continue, // Skip projects we can't access
        };

        // List all sessions for this project
        let project_sessions = match sm.list_sessions() {
            Ok(s) => s,
            Err(_) => continue, // Skip if we can't list sessions
        };

        // Process each session
        for metadata in project_sessions {
            // Skip non-running sessions
            if !metadata.is_running {
                continue;
            }

            // Check if worktree was deleted (stale session)
            let is_stale = !metadata.worktree_path.exists();

            // Determine if this is the main session
            let is_main_session = metadata.session_id == MAIN_SESSION_ID;

            // For stale sessions, set error and skip state loading
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
                });
                continue;
            }

            // Load the run state and live output for this session
            let (run, load_error, live_output) =
                if let Some(session_sm) = sm.get_session(&metadata.session_id) {
                    match session_sm.load_current() {
                        Ok(run) => {
                            // Load live output (gracefully returns None if missing/corrupted)
                            let live = session_sm.load_live();
                            (run, None, live)
                        }
                        Err(e) => (None, Some(format!("Corrupted state: {}", e)), None),
                    }
                } else {
                    (None, Some("Session not found".to_string()), None)
                };

            // Load spec to get progress information
            let progress = run.as_ref().and_then(|r| {
                Spec::load(&r.spec_json_path).ok().map(|spec| RunProgress {
                    completed: spec.completed_count(),
                    total: spec.total_count(),
                })
            });

            sessions.push(SessionData {
                project_name: project_name.clone(),
                metadata,
                run,
                progress,
                load_error,
                is_main_session,
                is_stale: false,
                live_output,
            });
        }
    }

    // Sort sessions by last_active_at descending
    sessions.sort_by(|a, b| b.metadata.last_active_at.cmp(&a.metadata.last_active_at));

    sessions
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

    // Already sorted by list_archived, but ensure newest first
    history.sort_by(|a, b| b.started_at.cmp(&a.started_at));

    Ok(history)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_progress_as_fraction() {
        // Normal case: working on story 2 of 5
        let progress = RunProgress {
            completed: 1,
            total: 5,
        };
        assert_eq!(progress.as_fraction(), "Story 2/5");

        // First story case
        let first = RunProgress {
            completed: 0,
            total: 3,
        };
        assert_eq!(first.as_fraction(), "Story 1/3");
    }

    #[test]
    fn test_run_progress_as_percentage() {
        // Normal case
        let progress = RunProgress {
            completed: 2,
            total: 5,
        };
        assert_eq!(progress.as_percentage(), "40%");

        // Complete case
        let complete = RunProgress {
            completed: 5,
            total: 5,
        };
        assert_eq!(complete.as_percentage(), "100%");
    }

    #[test]
    fn test_run_progress_as_percentage_zero_total() {
        let progress = RunProgress {
            completed: 0,
            total: 0,
        };
        assert_eq!(progress.as_percentage(), "0%");
    }

    #[test]
    fn test_run_progress_as_fraction_edge_cases() {
        // Edge case: 0/0 (empty spec) - should show "Story 0/0"
        let empty = RunProgress {
            completed: 0,
            total: 0,
        };
        assert_eq!(empty.as_fraction(), "Story 0/0");

        // Edge case: 0/5 (not started) - should show "Story 1/5"
        let not_started = RunProgress {
            completed: 0,
            total: 5,
        };
        assert_eq!(not_started.as_fraction(), "Story 1/5");

        // Edge case: 5/5 (completed) - should show "Story 5/5", NOT "Story 6/5"
        let complete = RunProgress {
            completed: 5,
            total: 5,
        };
        assert_eq!(complete.as_fraction(), "Story 5/5");

        // Edge case: 7/7 (completed, different total) - should show "Story 7/7"
        let complete_7 = RunProgress {
            completed: 7,
            total: 7,
        };
        assert_eq!(complete_7.as_fraction(), "Story 7/7");

        // Normal case: in progress
        let in_progress = RunProgress {
            completed: 3,
            total: 7,
        };
        assert_eq!(in_progress.as_fraction(), "Story 4/7");

        // Boundary: one before completion - should show next story
        let almost_done = RunProgress {
            completed: 4,
            total: 5,
        };
        assert_eq!(almost_done.as_fraction(), "Story 5/5");
    }

    #[test]
    fn test_run_progress_new() {
        let progress = RunProgress::new(3, 7);
        assert_eq!(progress.completed, 3);
        assert_eq!(progress.total, 7);
    }

    #[test]
    fn test_run_progress_fraction() {
        // Normal case
        assert!((RunProgress::new(2, 5).fraction() - 0.4).abs() < 0.001);

        // Zero total
        assert_eq!(RunProgress::new(0, 0).fraction(), 0.0);

        // Complete
        assert!((RunProgress::new(5, 5).fraction() - 1.0).abs() < 0.001);

        // Not started
        assert_eq!(RunProgress::new(0, 5).fraction(), 0.0);
    }

    #[test]
    fn test_run_progress_as_simple_fraction() {
        assert_eq!(RunProgress::new(2, 5).as_simple_fraction(), "2/5");
        assert_eq!(RunProgress::new(0, 3).as_simple_fraction(), "0/3");
        assert_eq!(RunProgress::new(5, 5).as_simple_fraction(), "5/5");
        assert_eq!(RunProgress::new(0, 0).as_simple_fraction(), "0/0");
    }

    #[test]
    fn test_run_progress_as_story_fraction_alias() {
        // as_story_fraction should produce identical results to as_fraction
        let progress = RunProgress::new(3, 7);
        assert_eq!(progress.as_story_fraction(), progress.as_fraction());

        let complete = RunProgress::new(5, 5);
        assert_eq!(complete.as_story_fraction(), complete.as_fraction());
    }

    #[test]
    fn test_run_history_entry_status_text() {
        use chrono::Utc;

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

    #[test]
    fn test_session_data_display_title_main() {
        use chrono::Utc;
        use std::path::PathBuf;

        let session = SessionData {
            project_name: "my-project".to_string(),
            metadata: SessionMetadata {
                session_id: "main".to_string(),
                worktree_path: PathBuf::from("/path/to/repo"),
                branch_name: "main".to_string(),
                created_at: Utc::now(),
                last_active_at: Utc::now(),
                is_running: false,
            },
            run: None,
            progress: None,
            load_error: None,
            is_main_session: true,
            is_stale: false,
            live_output: None,
        };
        assert_eq!(session.display_title(), "my-project (main)");
    }

    #[test]
    fn test_session_data_display_title_worktree() {
        use chrono::Utc;
        use std::path::PathBuf;

        let session = SessionData {
            project_name: "my-project".to_string(),
            metadata: SessionMetadata {
                session_id: "abc12345".to_string(),
                worktree_path: PathBuf::from("/path/to/worktree"),
                branch_name: "feature/test".to_string(),
                created_at: Utc::now(),
                last_active_at: Utc::now(),
                is_running: false,
            },
            run: None,
            progress: None,
            load_error: None,
            is_main_session: false,
            is_stale: false,
            live_output: None,
        };
        assert_eq!(session.display_title(), "my-project (abc12345)");
    }

    #[test]
    fn test_session_data_truncated_worktree_path_short() {
        use chrono::Utc;
        use std::path::PathBuf;

        let session = SessionData {
            project_name: "test".to_string(),
            metadata: SessionMetadata {
                session_id: "test".to_string(),
                worktree_path: PathBuf::from("repo"),
                branch_name: "main".to_string(),
                created_at: Utc::now(),
                last_active_at: Utc::now(),
                is_running: false,
            },
            run: None,
            progress: None,
            load_error: None,
            is_main_session: false,
            is_stale: false,
            live_output: None,
        };
        assert_eq!(session.truncated_worktree_path(), "repo");
    }

    #[test]
    fn test_session_data_truncated_worktree_path_long() {
        use chrono::Utc;
        use std::path::PathBuf;

        let session = SessionData {
            project_name: "test".to_string(),
            metadata: SessionMetadata {
                session_id: "test".to_string(),
                worktree_path: PathBuf::from("/home/user/projects/repo"),
                branch_name: "main".to_string(),
                created_at: Utc::now(),
                last_active_at: Utc::now(),
                is_running: false,
            },
            run: None,
            progress: None,
            load_error: None,
            is_main_session: false,
            is_stale: false,
            live_output: None,
        };
        assert_eq!(session.truncated_worktree_path(), ".../projects/repo");
    }

    // ========================================================================
    // US-002: Heartbeat Freshness Tests
    // ========================================================================

    #[test]
    fn test_session_data_has_fresh_heartbeat_no_live_output() {
        use chrono::Utc;
        use std::path::PathBuf;

        let session = SessionData {
            project_name: "test".to_string(),
            metadata: SessionMetadata {
                session_id: "test".to_string(),
                worktree_path: PathBuf::from("/path"),
                branch_name: "main".to_string(),
                created_at: Utc::now(),
                last_active_at: Utc::now(),
                is_running: true,
            },
            run: None,
            progress: None,
            load_error: None,
            is_main_session: true,
            is_stale: false,
            live_output: None, // No live output
        };

        // Without live output, heartbeat is considered not fresh
        assert!(!session.has_fresh_heartbeat());
    }

    #[test]
    fn test_session_data_has_fresh_heartbeat_with_fresh_live() {
        use chrono::Utc;
        use std::path::PathBuf;

        let session = SessionData {
            project_name: "test".to_string(),
            metadata: SessionMetadata {
                session_id: "test".to_string(),
                worktree_path: PathBuf::from("/path"),
                branch_name: "main".to_string(),
                created_at: Utc::now(),
                last_active_at: Utc::now(),
                is_running: true,
            },
            run: None,
            progress: None,
            load_error: None,
            is_main_session: true,
            is_stale: false,
            live_output: Some(LiveState::new(crate::state::MachineState::RunningClaude)),
        };

        // Fresh live output should mean fresh heartbeat
        assert!(session.has_fresh_heartbeat());
    }

    #[test]
    fn test_session_data_is_actively_running_no_live_output() {
        use chrono::Utc;
        use std::path::PathBuf;

        let session = SessionData {
            project_name: "test".to_string(),
            metadata: SessionMetadata {
                session_id: "test".to_string(),
                worktree_path: PathBuf::from("/path"),
                branch_name: "main".to_string(),
                created_at: Utc::now(),
                last_active_at: Utc::now(),
                is_running: true,
            },
            run: None,
            progress: None,
            load_error: None,
            is_main_session: true,
            is_stale: false,
            live_output: None, // No live output yet
        };

        // Without live output but is_running=true, trust is_running (run may have just started)
        assert!(session.is_actively_running());
    }

    #[test]
    fn test_session_data_is_actively_running_stale() {
        use chrono::Utc;
        use std::path::PathBuf;

        let session = SessionData {
            project_name: "test".to_string(),
            metadata: SessionMetadata {
                session_id: "test".to_string(),
                worktree_path: PathBuf::from("/deleted/path"),
                branch_name: "main".to_string(),
                created_at: Utc::now(),
                last_active_at: Utc::now(),
                is_running: true,
            },
            run: None,
            progress: None,
            load_error: None,
            is_main_session: false,
            is_stale: true, // Worktree deleted
            live_output: Some(LiveState::new(crate::state::MachineState::RunningClaude)),
        };

        // Stale sessions are never actively running
        assert!(!session.is_actively_running());
    }

    #[test]
    fn test_session_data_appears_stuck_fresh_heartbeat() {
        use chrono::Utc;
        use std::path::PathBuf;

        let session = SessionData {
            project_name: "test".to_string(),
            metadata: SessionMetadata {
                session_id: "test".to_string(),
                worktree_path: PathBuf::from("/path"),
                branch_name: "main".to_string(),
                created_at: Utc::now(),
                last_active_at: Utc::now(),
                is_running: true,
            },
            run: None,
            progress: None,
            load_error: None,
            is_main_session: true,
            is_stale: false,
            live_output: Some(LiveState::new(crate::state::MachineState::RunningClaude)),
        };

        // Fresh heartbeat means not stuck
        assert!(!session.appears_stuck());
    }

    #[test]
    fn test_session_data_appears_stuck_stale_heartbeat() {
        use chrono::Utc;
        use std::path::PathBuf;

        let mut live = LiveState::new(crate::state::MachineState::RunningClaude);
        // Set heartbeat to be 65 seconds ago (stale, threshold is 60s)
        live.last_heartbeat = Utc::now() - chrono::Duration::seconds(65);

        let session = SessionData {
            project_name: "test".to_string(),
            metadata: SessionMetadata {
                session_id: "test".to_string(),
                worktree_path: PathBuf::from("/path"),
                branch_name: "main".to_string(),
                created_at: Utc::now(),
                last_active_at: Utc::now(),
                is_running: true,
            },
            run: None,
            progress: None,
            load_error: None,
            is_main_session: true,
            is_stale: false,
            live_output: Some(live),
        };

        // Stale heartbeat while is_running=true means appears stuck
        assert!(session.appears_stuck());
    }

    #[test]
    fn test_session_data_appears_stuck_not_running() {
        use chrono::Utc;
        use std::path::PathBuf;

        let mut live = LiveState::new(crate::state::MachineState::Completed);
        // Stale heartbeat
        live.last_heartbeat = Utc::now() - chrono::Duration::seconds(15);

        let session = SessionData {
            project_name: "test".to_string(),
            metadata: SessionMetadata {
                session_id: "test".to_string(),
                worktree_path: PathBuf::from("/path"),
                branch_name: "main".to_string(),
                created_at: Utc::now(),
                last_active_at: Utc::now(),
                is_running: false, // Not running
            },
            run: None,
            progress: None,
            load_error: None,
            is_main_session: true,
            is_stale: false,
            live_output: Some(live),
        };

        // Not running sessions can't be stuck
        assert!(!session.appears_stuck());
    }

    // ========================================================================
    // US-005: Shared Status Utilities Tests
    // ========================================================================

    #[test]
    fn test_status_from_machine_state_setup_phases() {
        // All setup phases should map to Status::Setup
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
        assert_eq!(
            Status::from_machine_state(MachineState::GeneratingSpec),
            Status::Setup
        );
    }

    #[test]
    fn test_status_from_machine_state_running() {
        assert_eq!(
            Status::from_machine_state(MachineState::RunningClaude),
            Status::Running
        );
    }

    #[test]
    fn test_status_from_machine_state_reviewing() {
        assert_eq!(
            Status::from_machine_state(MachineState::Reviewing),
            Status::Reviewing
        );
    }

    #[test]
    fn test_status_from_machine_state_correcting() {
        assert_eq!(
            Status::from_machine_state(MachineState::Correcting),
            Status::Correcting
        );
    }

    #[test]
    fn test_status_from_machine_state_success_path() {
        // All success path states should map to Status::Success
        assert_eq!(
            Status::from_machine_state(MachineState::Committing),
            Status::Success
        );
        assert_eq!(
            Status::from_machine_state(MachineState::CreatingPR),
            Status::Success
        );
        assert_eq!(
            Status::from_machine_state(MachineState::Completed),
            Status::Success
        );
    }

    #[test]
    fn test_status_from_machine_state_terminal() {
        assert_eq!(
            Status::from_machine_state(MachineState::Failed),
            Status::Error
        );
        assert_eq!(Status::from_machine_state(MachineState::Idle), Status::Idle);
    }

    #[test]
    fn test_format_state_label_all_states() {
        // Verify all state labels are correctly formatted
        assert_eq!(format_state_label(MachineState::Idle), "Idle");
        assert_eq!(
            format_state_label(MachineState::LoadingSpec),
            "Loading Spec"
        );
        assert_eq!(
            format_state_label(MachineState::GeneratingSpec),
            "Generating Spec"
        );
        assert_eq!(
            format_state_label(MachineState::Initializing),
            "Initializing"
        );
        assert_eq!(
            format_state_label(MachineState::PickingStory),
            "Picking Story"
        );
        assert_eq!(
            format_state_label(MachineState::RunningClaude),
            "Running Claude"
        );
        assert_eq!(format_state_label(MachineState::Reviewing), "Reviewing");
        assert_eq!(format_state_label(MachineState::Correcting), "Correcting");
        assert_eq!(format_state_label(MachineState::Committing), "Committing");
        assert_eq!(format_state_label(MachineState::CreatingPR), "Creating PR");
        assert_eq!(format_state_label(MachineState::Completed), "Completed");
        assert_eq!(format_state_label(MachineState::Failed), "Failed");
    }

    #[test]
    fn test_format_duration_secs() {
        // Seconds only
        assert_eq!(format_duration_secs(0), "0s");
        assert_eq!(format_duration_secs(30), "30s");
        assert_eq!(format_duration_secs(59), "59s");

        // Minutes and seconds
        assert_eq!(format_duration_secs(60), "1m 0s");
        assert_eq!(format_duration_secs(125), "2m 5s");
        assert_eq!(format_duration_secs(3599), "59m 59s");

        // Hours and minutes
        assert_eq!(format_duration_secs(3600), "1h 0m");
        assert_eq!(format_duration_secs(7265), "2h 1m");
    }

    #[test]
    fn test_format_relative_time_secs() {
        // Just now (< 1 minute)
        assert_eq!(format_relative_time_secs(0), "just now");
        assert_eq!(format_relative_time_secs(59), "just now");

        // Minutes
        assert_eq!(format_relative_time_secs(60), "1m ago");
        assert_eq!(format_relative_time_secs(300), "5m ago");

        // Hours
        assert_eq!(format_relative_time_secs(3600), "1h ago");
        assert_eq!(format_relative_time_secs(7200), "2h ago");

        // Days
        assert_eq!(format_relative_time_secs(86400), "1d ago");
        assert_eq!(format_relative_time_secs(172800), "2d ago");
    }
}
