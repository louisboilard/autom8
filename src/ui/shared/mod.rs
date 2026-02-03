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
    IterationStatus, LiveState, RunState, RunStatus, SessionMetadata, StateManager,
};
use crate::worktree::MAIN_SESSION_ID;
use std::collections::HashSet;
use std::path::PathBuf;

// ============================================================================
// Shared Data Types
// ============================================================================

/// Progress information for a run.
#[derive(Debug, Clone)]
pub struct RunProgress {
    /// Number of completed stories.
    pub completed: usize,
    /// Total number of stories.
    pub total: usize,
}

impl RunProgress {
    /// Format progress as a fraction string (e.g., "Story 2/5").
    pub fn as_fraction(&self) -> String {
        format!("Story {}/{}", self.completed + 1, self.total)
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
}
