//! Shared data types and logic for UI modules.
//!
//! This module contains common data structures used by both the GUI and TUI,
//! such as run progress, project data, session data, and run history entries.
//!
//! These types are framework-agnostic and can be used by any UI implementation.

use crate::config::ProjectTreeInfo;
use crate::state::{
    IterationStatus, LiveState, RunState, RunStatus, SessionMetadata,
};
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_progress_as_fraction() {
        let progress = RunProgress {
            completed: 1,
            total: 5,
        };
        assert_eq!(progress.as_fraction(), "Story 2/5");
    }

    #[test]
    fn test_run_progress_as_percentage() {
        let progress = RunProgress {
            completed: 2,
            total: 5,
        };
        assert_eq!(progress.as_percentage(), "40%");
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
