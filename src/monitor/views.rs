//! View definitions for the Monitor TUI.
//!
//! The monitor supports three views:
//! - Active Runs: Shows real-time status of running autom8 processes
//! - Project List: Shows all projects with their status
//! - Run History: Shows past runs across projects

use std::fmt;

/// The available views in the monitor TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// Shows real-time status of active autom8 runs.
    /// This view is hidden if no runs are active.
    ActiveRuns,
    /// Shows all projects with their status.
    /// This is the default view when no runs are active.
    ProjectList,
    /// Shows past runs across all projects (or filtered to one project).
    RunHistory,
}

impl View {
    /// Returns the display name for this view.
    pub fn name(&self) -> &'static str {
        match self {
            View::ActiveRuns => "Active Runs",
            View::ProjectList => "Projects",
            View::RunHistory => "Run History",
        }
    }

    /// Get all views in order.
    pub fn all() -> &'static [View] {
        &[View::ActiveRuns, View::ProjectList, View::RunHistory]
    }

    /// Get the next view in the cycle, optionally skipping ActiveRuns.
    pub fn next(&self, skip_active_runs: bool) -> View {
        let views = View::all();
        let current_idx = views.iter().position(|v| v == self).unwrap_or(0);
        let mut next_idx = (current_idx + 1) % views.len();

        // Skip ActiveRuns if requested
        if skip_active_runs && views[next_idx] == View::ActiveRuns {
            next_idx = (next_idx + 1) % views.len();
        }

        views[next_idx]
    }

    /// Get the default view based on whether there are active runs.
    pub fn default_view(has_active_runs: bool) -> View {
        if has_active_runs {
            View::ActiveRuns
        } else {
            View::ProjectList
        }
    }
}

impl fmt::Display for View {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_view_name() {
        assert_eq!(View::ActiveRuns.name(), "Active Runs");
        assert_eq!(View::ProjectList.name(), "Projects");
        assert_eq!(View::RunHistory.name(), "Run History");
    }

    #[test]
    fn test_view_all() {
        let all = View::all();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0], View::ActiveRuns);
        assert_eq!(all[1], View::ProjectList);
        assert_eq!(all[2], View::RunHistory);
    }

    #[test]
    fn test_view_next_without_skip() {
        assert_eq!(View::ActiveRuns.next(false), View::ProjectList);
        assert_eq!(View::ProjectList.next(false), View::RunHistory);
        assert_eq!(View::RunHistory.next(false), View::ActiveRuns);
    }

    #[test]
    fn test_view_next_with_skip_active_runs() {
        // From ActiveRuns, should go to ProjectList (normal)
        assert_eq!(View::ActiveRuns.next(true), View::ProjectList);
        // From ProjectList, should skip ActiveRuns and go to RunHistory, then cycle
        assert_eq!(View::ProjectList.next(true), View::RunHistory);
        // From RunHistory, should skip ActiveRuns and go to ProjectList
        assert_eq!(View::RunHistory.next(true), View::ProjectList);
    }

    #[test]
    fn test_default_view_with_active_runs() {
        assert_eq!(View::default_view(true), View::ActiveRuns);
    }

    #[test]
    fn test_default_view_without_active_runs() {
        assert_eq!(View::default_view(false), View::ProjectList);
    }

    #[test]
    fn test_view_display() {
        assert_eq!(format!("{}", View::ActiveRuns), "Active Runs");
        assert_eq!(format!("{}", View::ProjectList), "Projects");
        assert_eq!(format!("{}", View::RunHistory), "Run History");
    }
}
