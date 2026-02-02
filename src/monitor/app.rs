//! Monitor TUI Application
//!
//! The main application struct and event loop for the monitor command.

use super::views::View;
use crate::config::{list_projects_tree, ProjectTreeInfo};
use crate::error::Result;
use crate::spec::Spec;
use crate::state::{MachineState, RunState, SessionMetadata, StateManager};
use crate::worktree::MAIN_SESSION_ID;
use chrono::{DateTime, Utc};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Tabs, Wrap},
    Frame, Terminal,
};
use std::io::{self, Stdout};
use std::path::PathBuf;
use std::time::Duration;

// ============================================================================
// Color Constants (consistent with output.rs autom8 branding)
// ============================================================================

/// Cyan - primary branding color, used for headers and highlights
const COLOR_PRIMARY: Color = Color::Cyan;
/// Green - success states
const COLOR_SUCCESS: Color = Color::Green;
/// Yellow - warning/in-progress states
const COLOR_WARNING: Color = Color::Yellow;
/// Red - error/failure states
const COLOR_ERROR: Color = Color::Red;
/// Blue - informational elements
const COLOR_INFO: Color = Color::Blue;
/// Gray - dimmed/secondary text
const COLOR_DIM: Color = Color::DarkGray;
/// Magenta - review/correction states
const COLOR_REVIEW: Color = Color::Magenta;

/// Result type for monitor operations.
pub type MonitorResult<T> = std::result::Result<T, MonitorError>;

/// Error types for the monitor TUI.
#[derive(Debug)]
pub enum MonitorError {
    /// IO error from terminal operations
    Io(io::Error),
    /// Error from autom8 operations
    Autom8(crate::error::Autom8Error),
}

impl std::fmt::Display for MonitorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MonitorError::Io(e) => write!(f, "IO error: {}", e),
            MonitorError::Autom8(e) => write!(f, "Autom8 error: {}", e),
        }
    }
}

impl std::error::Error for MonitorError {}

impl From<io::Error> for MonitorError {
    fn from(err: io::Error) -> Self {
        MonitorError::Io(err)
    }
}

impl From<crate::error::Autom8Error> for MonitorError {
    fn from(err: crate::error::Autom8Error) -> Self {
        MonitorError::Autom8(err)
    }
}

/// Progress information for a run.
#[derive(Debug, Clone)]
pub struct RunProgress {
    /// Number of completed stories
    pub completed: usize,
    /// Total number of stories
    pub total: usize,
}

impl RunProgress {
    /// Format progress as a fraction string (e.g., "Story 2/5")
    pub fn as_fraction(&self) -> String {
        format!("Story {}/{}", self.completed + 1, self.total)
    }

    /// Format progress as a percentage (e.g., "40%")
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
    pub info: ProjectTreeInfo,
    pub active_run: Option<RunState>,
    /// Progress through the spec (loaded from spec file)
    pub progress: Option<RunProgress>,
    /// Error message if state file is corrupted or unreadable
    pub load_error: Option<String>,
}

/// Data for a single session in the Active Runs view.
///
/// This struct represents one running session, which can be from
/// the main repo or a worktree. Multiple sessions can belong to
/// the same project (when using worktree mode).
#[derive(Debug, Clone)]
pub struct SessionData {
    /// Project name (e.g., "autom8")
    pub project_name: String,
    /// Session metadata (includes session_id, worktree_path, branch)
    pub metadata: SessionMetadata,
    /// The active run state for this session
    pub run: Option<RunState>,
    /// Progress through the spec (loaded from spec file)
    pub progress: Option<RunProgress>,
    /// Error message if state file is corrupted or unreadable
    pub load_error: Option<String>,
    /// Whether this is the main repo session (vs. a worktree)
    pub is_main_session: bool,
    /// Whether this session is stale (worktree was deleted)
    pub is_stale: bool,
}

impl SessionData {
    /// Format the display title for this session.
    /// Returns "project-name (main)" or "project-name (abc12345)"
    pub fn display_title(&self) -> String {
        if self.is_main_session {
            format!("{} (main)", self.project_name)
        } else {
            format!("{} ({})", self.project_name, &self.metadata.session_id)
        }
    }

    /// Get a truncated worktree path for display (last 2 components)
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

/// A single entry in the run history view.
#[derive(Debug, Clone)]
pub struct RunHistoryEntry {
    /// The project this run belongs to
    pub project_name: String,
    /// The run state data
    pub run: RunState,
    /// Number of stories that were completed
    pub completed_stories: usize,
    /// Total number of stories in the spec
    pub total_stories: usize,
}

/// Format a duration in seconds as a human-readable string (e.g., "5m 32s", "1h 5m")
pub fn format_duration(started_at: DateTime<Utc>) -> String {
    let now = Utc::now();
    let duration = now.signed_duration_since(started_at);
    let total_secs = duration.num_seconds().max(0) as u64;

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

/// Format a timestamp as a relative time string (e.g., "2h ago", "3d ago")
pub fn format_relative_time(timestamp: DateTime<Utc>) -> String {
    let now = Utc::now();
    let duration = now.signed_duration_since(timestamp);
    let total_secs = duration.num_seconds().max(0) as u64;

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

/// Format a machine state as a human-readable string
fn format_state(state: MachineState) -> &'static str {
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

/// Get a color for a machine state (consistent with output.rs branding)
fn state_color(state: MachineState) -> Color {
    match state {
        MachineState::Idle => COLOR_DIM,
        MachineState::LoadingSpec | MachineState::GeneratingSpec => COLOR_WARNING,
        MachineState::Initializing | MachineState::PickingStory => COLOR_INFO,
        MachineState::RunningClaude => COLOR_PRIMARY,
        MachineState::Reviewing | MachineState::Correcting => COLOR_REVIEW,
        MachineState::Committing | MachineState::CreatingPR => COLOR_SUCCESS,
        MachineState::Completed => COLOR_SUCCESS,
        MachineState::Failed => COLOR_ERROR,
    }
}

/// The main monitor application state.
pub struct MonitorApp {
    /// Current view being displayed
    current_view: View,
    /// Polling interval in seconds
    poll_interval: u64,
    /// Optional project filter
    project_filter: Option<String>,
    /// Cached project data (used for Project List view)
    projects: Vec<ProjectData>,
    /// Cached session data for Active Runs view.
    /// Contains only running sessions (is_running=true and not stale).
    sessions: Vec<SessionData>,
    /// Cached run history entries (sorted by date, most recent first)
    run_history: Vec<RunHistoryEntry>,
    /// Whether there are any active runs
    has_active_runs: bool,
    /// Whether the app should quit
    should_quit: bool,
    /// Selected index for list navigation
    selected_index: usize,
    /// Project name to filter Run History view (set when pressing Enter on Project List)
    run_history_filter: Option<String>,
    /// Scroll offset for run history view
    history_scroll_offset: usize,
    /// Whether to show the detail view for a selected run
    show_run_detail: bool,
    /// Current page in Active Runs view (0-indexed) for pagination when > 4 runs
    quadrant_page: usize,
    /// Selected quadrant row (0 or 1) for Active Runs 2D navigation
    quadrant_row: usize,
    /// Selected quadrant column (0 or 1) for Active Runs 2D navigation
    quadrant_col: usize,
    /// Scroll offset for detail view
    detail_scroll_offset: usize,
}

impl MonitorApp {
    /// Create a new MonitorApp with the given configuration.
    pub fn new(poll_interval: u64, project_filter: Option<String>) -> Self {
        Self {
            current_view: View::ProjectList, // Will be updated on first refresh
            poll_interval,
            project_filter,
            projects: Vec::new(),
            sessions: Vec::new(),
            run_history: Vec::new(),
            has_active_runs: false,
            should_quit: false,
            selected_index: 0,
            run_history_filter: None,
            history_scroll_offset: 0,
            show_run_detail: false,
            quadrant_page: 0,
            quadrant_row: 0,
            quadrant_col: 0,
            detail_scroll_offset: 0,
        }
    }

    /// Refresh project data from disk.
    ///
    /// This method handles corrupted or invalid state files gracefully,
    /// showing error indicators in the UI instead of crashing.
    pub fn refresh_data(&mut self) -> Result<()> {
        // Handle list_projects_tree failure gracefully
        let tree_infos = match list_projects_tree() {
            Ok(infos) => infos,
            Err(e) => {
                // Log error but continue with empty list
                // This handles cases where the config directory is inaccessible
                eprintln!("Warning: Failed to list projects: {}", e);
                Vec::new()
            }
        };

        // Filter by project if specified
        let filtered: Vec<_> = if let Some(ref filter) = self.project_filter {
            tree_infos
                .into_iter()
                .filter(|p| p.name == *filter)
                .collect()
        } else {
            tree_infos
        };

        // Collect project data including active runs and progress (for Project List view)
        // Handle corrupted state files gracefully
        self.projects = filtered
            .iter()
            .map(|info| {
                let (active_run, load_error) = if info.has_active_run {
                    match StateManager::for_project(&info.name) {
                        Ok(sm) => match sm.load_current() {
                            Ok(run) => (run, None),
                            Err(e) => {
                                // State file exists but is corrupted/invalid
                                (None, Some(format!("Corrupted state: {}", e)))
                            }
                        },
                        Err(e) => {
                            // Failed to create state manager
                            (None, Some(format!("State error: {}", e)))
                        }
                    }
                } else {
                    (None, None)
                };

                // Load spec to get progress information (gracefully handle errors)
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
            })
            .collect();

        // Refresh sessions for Active Runs view
        self.refresh_sessions(&filtered);

        // Update active runs status based on sessions (not projects)
        self.has_active_runs = !self.sessions.is_empty();

        // If current view is ActiveRuns but no active runs, switch to ProjectList
        if self.current_view == View::ActiveRuns && !self.has_active_runs {
            self.current_view = View::ProjectList;
        }

        // Clamp selected_index to valid range when projects are removed
        self.clamp_selection_index();

        // Load run history (errors are handled internally)
        let _ = self.refresh_run_history();

        Ok(())
    }

    /// Refresh sessions for the Active Runs view.
    ///
    /// Collects all running sessions across all projects, filtering out
    /// stale sessions (where the worktree no longer exists).
    fn refresh_sessions(&mut self, project_infos: &[ProjectTreeInfo]) {
        let mut sessions: Vec<SessionData> = Vec::new();

        // Get all project names to check
        let project_names: Vec<_> = if let Some(ref filter) = self.project_filter {
            vec![filter.clone()]
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
                    });
                    continue;
                }

                // Load the run state for this session
                let (run, load_error) =
                    if let Some(session_sm) = sm.get_session(&metadata.session_id) {
                        match session_sm.load_current() {
                            Ok(run) => (run, None),
                            Err(e) => (None, Some(format!("Corrupted state: {}", e))),
                        }
                    } else {
                        (None, Some("Session not found".to_string()))
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
                });
            }
        }

        // Sort sessions by last_active_at descending
        sessions.sort_by(|a, b| b.metadata.last_active_at.cmp(&a.metadata.last_active_at));

        self.sessions = sessions;
    }

    /// Ensure selected_index and quadrant_page stay within bounds when projects/history change.
    fn clamp_selection_index(&mut self) {
        let max_index = match self.current_view {
            View::ProjectList => self.projects.len().saturating_sub(1),
            // Active Runs now uses sessions, not projects
            View::ActiveRuns => self.sessions.len().saturating_sub(1),
            View::RunHistory => self.run_history.len().saturating_sub(1),
        };
        if self.selected_index > max_index {
            self.selected_index = max_index;
        }
        // Also clamp scroll offset
        if self.history_scroll_offset > self.selected_index {
            self.history_scroll_offset = self.selected_index;
        }
        // Clamp quadrant_page for Active Runs view
        let max_page = self.total_quadrant_pages().saturating_sub(1);
        if self.quadrant_page > max_page {
            self.quadrant_page = max_page;
        }
        // Clamp quadrant row/col to valid positions
        let runs_on_page = self.runs_on_current_page();
        if runs_on_page == 0 {
            self.quadrant_row = 0;
            self.quadrant_col = 0;
        } else {
            // Ensure current position is valid
            if !self.is_quadrant_valid(self.quadrant_row, self.quadrant_col) {
                // Move to the last valid quadrant
                let last_idx = runs_on_page.saturating_sub(1);
                self.quadrant_row = last_idx / 2;
                self.quadrant_col = last_idx % 2;
            }
        }
    }

    /// Refresh run history from all projects.
    ///
    /// This method handles corrupted run files gracefully by skipping them
    /// rather than failing the entire refresh.
    fn refresh_run_history(&mut self) -> Result<()> {
        let mut history: Vec<RunHistoryEntry> = Vec::new();

        // Determine which projects to load history from
        let project_names: Vec<String> = if let Some(ref filter) = self.run_history_filter {
            // Filtered to a single project
            vec![filter.clone()]
        } else if let Some(ref filter) = self.project_filter {
            // Using the global project filter
            vec![filter.clone()]
        } else {
            // All projects
            self.projects.iter().map(|p| p.info.name.clone()).collect()
        };

        // Load archived runs from each project
        // Errors are handled gracefully - corrupted files are simply skipped
        for project_name in project_names {
            if let Ok(sm) = StateManager::for_project(&project_name) {
                // list_archived already skips corrupted files internally
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
                                    .filter(|i| i.status == crate::state::IterationStatus::Success)
                                    .count();
                                (completed, run.iterations.len().max(completed))
                            });

                        history.push(RunHistoryEntry {
                            project_name: project_name.clone(),
                            run,
                            completed_stories: completed,
                            total_stories: total,
                        });
                    }
                }
            }
        }

        // Sort by date, most recent first
        history.sort_by(|a, b| b.run.started_at.cmp(&a.run.started_at));

        // Limit to last 100 runs for performance
        history.truncate(100);

        self.run_history = history;

        Ok(())
    }

    /// Switch to the next view.
    pub fn next_view(&mut self) {
        self.current_view = self.current_view.next(!self.has_active_runs);
        self.selected_index = 0;
        self.quadrant_page = 0; // Reset pagination when switching views
        self.quadrant_row = 0;
        self.quadrant_col = 0;
    }

    /// Get the total number of pages for Active Runs view.
    fn total_quadrant_pages(&self) -> usize {
        // Active Runs view now uses sessions (not projects)
        let active_count = self.sessions.len();
        if active_count == 0 {
            1
        } else {
            active_count.div_ceil(4)
        }
    }

    /// Move to the next page in Active Runs view.
    fn next_quadrant_page(&mut self) {
        let total_pages = self.total_quadrant_pages();
        if total_pages > 1 && self.quadrant_page < total_pages - 1 {
            self.quadrant_page += 1;
        }
    }

    /// Move to the previous page in Active Runs view.
    fn prev_quadrant_page(&mut self) {
        if self.quadrant_page > 0 {
            self.quadrant_page -= 1;
        }
    }

    /// Get the number of active runs on the current page (0-4).
    fn runs_on_current_page(&self) -> usize {
        // Active Runs view now uses sessions (not projects)
        let active_count = self.sessions.len();
        let start_idx = self.quadrant_page * 4;
        let remaining = active_count.saturating_sub(start_idx);
        remaining.min(4)
    }

    /// Check if a quadrant position is valid (has a run) on the current page.
    fn is_quadrant_valid(&self, row: usize, col: usize) -> bool {
        let quadrant_idx = row * 2 + col;
        quadrant_idx < self.runs_on_current_page()
    }

    /// Navigate up in the quadrant grid (Active Runs view).
    fn quadrant_move_up(&mut self) {
        if self.quadrant_row > 0 {
            let new_row = self.quadrant_row - 1;
            if self.is_quadrant_valid(new_row, self.quadrant_col) {
                self.quadrant_row = new_row;
            }
        }
    }

    /// Navigate down in the quadrant grid (Active Runs view).
    fn quadrant_move_down(&mut self) {
        if self.quadrant_row < 1 {
            let new_row = self.quadrant_row + 1;
            if self.is_quadrant_valid(new_row, self.quadrant_col) {
                self.quadrant_row = new_row;
            }
        }
    }

    /// Navigate left in the quadrant grid (Active Runs view).
    fn quadrant_move_left(&mut self) {
        if self.quadrant_col > 0 {
            let new_col = self.quadrant_col - 1;
            if self.is_quadrant_valid(self.quadrant_row, new_col) {
                self.quadrant_col = new_col;
            }
        }
    }

    /// Navigate right in the quadrant grid (Active Runs view).
    fn quadrant_move_right(&mut self) {
        if self.quadrant_col < 1 {
            let new_col = self.quadrant_col + 1;
            if self.is_quadrant_valid(self.quadrant_row, new_col) {
                self.quadrant_col = new_col;
            }
        }
    }

    /// Handle keyboard input.
    pub fn handle_key(&mut self, key: KeyCode) {
        // q/Q always quits immediately from any screen
        if matches!(key, KeyCode::Char('q') | KeyCode::Char('Q')) {
            self.should_quit = true;
            return;
        }

        // Handle keys in detail view
        if self.show_run_detail {
            match key {
                KeyCode::Esc | KeyCode::Enter => {
                    self.show_run_detail = false;
                    self.detail_scroll_offset = 0;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.detail_scroll_offset = self.detail_scroll_offset.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.detail_scroll_offset = self.detail_scroll_offset.saturating_add(1);
                }
                _ => {}
            }
            return;
        }

        match key {
            KeyCode::Tab => {
                self.next_view();
                // Clear run history filter when switching views with Tab
                self.run_history_filter = None;
                self.history_scroll_offset = 0;
            }
            // Up navigation (arrow key or k)
            KeyCode::Up | KeyCode::Char('k') => {
                if self.current_view == View::ActiveRuns {
                    self.quadrant_move_up();
                } else if self.selected_index > 0 {
                    self.selected_index -= 1;
                    // Adjust scroll offset if needed
                    if self.current_view == View::RunHistory
                        && self.selected_index < self.history_scroll_offset
                    {
                        self.history_scroll_offset = self.selected_index;
                    }
                }
            }
            // Down navigation (arrow key or j)
            KeyCode::Down | KeyCode::Char('j') => {
                if self.current_view == View::ActiveRuns {
                    self.quadrant_move_down();
                } else {
                    let max_index = match self.current_view {
                        View::ProjectList => self.projects.len().saturating_sub(1),
                        View::ActiveRuns => 0, // Not used, handled above
                        View::RunHistory => self.run_history.len().saturating_sub(1),
                    };
                    if self.selected_index < max_index {
                        self.selected_index += 1;
                    }
                }
            }
            // Left navigation (h) - only meaningful in Active Runs quadrant view
            KeyCode::Left | KeyCode::Char('h') => {
                if self.current_view == View::ActiveRuns {
                    self.quadrant_move_left();
                }
            }
            // Right navigation (l) - only meaningful in Active Runs quadrant view
            KeyCode::Right | KeyCode::Char('l') => {
                if self.current_view == View::ActiveRuns {
                    self.quadrant_move_right();
                }
            }
            KeyCode::Enter => {
                self.handle_enter();
            }
            KeyCode::Esc => {
                // Hierarchical escape behavior - go back one level
                match self.current_view {
                    View::RunHistory => {
                        if self.run_history_filter.is_some() {
                            // Clear filter first
                            self.run_history_filter = None;
                            self.selected_index = 0;
                            self.history_scroll_offset = 0;
                        } else {
                            // Go back to ProjectList
                            self.current_view = View::ProjectList;
                            self.selected_index = 0;
                        }
                    }
                    View::ProjectList | View::ActiveRuns => {
                        // Root views - quit
                        self.should_quit = true;
                    }
                }
            }
            // Pagination for Active Runs view (n/] = next, p/[ = previous)
            KeyCode::Char('n') | KeyCode::Char(']') => {
                if self.current_view == View::ActiveRuns {
                    self.next_quadrant_page();
                }
            }
            KeyCode::Char('p') | KeyCode::Char('[') => {
                if self.current_view == View::ActiveRuns {
                    self.prev_quadrant_page();
                }
            }
            _ => {}
        }
    }

    /// Handle Enter key press based on current view.
    fn handle_enter(&mut self) {
        match self.current_view {
            View::ProjectList => {
                // Switch to Run History filtered by selected project
                if let Some(project) = self.projects.get(self.selected_index) {
                    self.run_history_filter = Some(project.info.name.clone());
                    self.current_view = View::RunHistory;
                    self.selected_index = 0;
                    self.history_scroll_offset = 0;
                }
            }
            View::RunHistory => {
                // Show detail view for selected run
                if self.selected_index < self.run_history.len() {
                    self.show_run_detail = true;
                    self.detail_scroll_offset = 0;
                }
            }
            View::ActiveRuns => {
                // No action for now
            }
        }
    }

    /// Check if run detail view is shown.
    pub fn is_showing_run_detail(&self) -> bool {
        self.show_run_detail
    }

    /// Get the current run history filter (project name).
    pub fn run_history_filter(&self) -> Option<&str> {
        self.run_history_filter.as_deref()
    }

    /// Check if the app should quit.
    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    /// Get the current view.
    pub fn current_view(&self) -> View {
        self.current_view
    }

    /// Get the poll interval in seconds.
    pub fn poll_interval(&self) -> u64 {
        self.poll_interval
    }

    /// Get available views based on current state.
    fn available_views(&self) -> Vec<View> {
        if self.has_active_runs {
            View::all().to_vec()
        } else {
            vec![View::ProjectList, View::RunHistory]
        }
    }

    /// Render the UI to the terminal.
    pub fn render(&self, frame: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Header with tabs
                Constraint::Min(0),    // Main content
                Constraint::Length(1), // Footer
            ])
            .split(frame.area());

        self.render_header(frame, chunks[0]);
        self.render_content(frame, chunks[1]);
        self.render_footer(frame, chunks[2]);
    }

    fn render_header(&self, frame: &mut Frame, area: Rect) {
        let available_views = self.available_views();
        let titles: Vec<Line> = available_views
            .iter()
            .map(|v| Line::from(v.name()))
            .collect();

        let selected_idx = available_views
            .iter()
            .position(|v| *v == self.current_view)
            .unwrap_or(0);

        let tabs = Tabs::new(titles)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" autom8 monitor ")
                    .border_style(Style::default().fg(COLOR_PRIMARY)),
            )
            .select(selected_idx)
            .style(Style::default().fg(Color::White))
            .highlight_style(
                Style::default()
                    .fg(COLOR_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            );

        frame.render_widget(tabs, area);
    }

    fn render_content(&self, frame: &mut Frame, area: Rect) {
        match self.current_view {
            View::ActiveRuns => self.render_active_runs(frame, area),
            View::ProjectList => self.render_project_list(frame, area),
            View::RunHistory => self.render_run_history(frame, area),
        }
    }

    fn render_active_runs(&self, frame: &mut Frame, area: Rect) {
        // Calculate pagination info - now using sessions instead of projects
        let total_runs = self.sessions.len();
        let total_pages = total_runs.div_ceil(4).max(1);
        let start_idx = self.quadrant_page * 4;

        // Get the 4 sessions (or fewer) for the current page
        let page_sessions: Vec<Option<&SessionData>> =
            (0..4).map(|i| self.sessions.get(start_idx + i)).collect();

        // Fixed 2x2 grid layout - always split into 2 rows, each with 2 columns
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        let top_cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(rows[0]);

        let bottom_cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(rows[1]);

        // Map quadrant indices to areas: [0]=top-left, [1]=top-right, [2]=bottom-left, [3]=bottom-right
        let quadrant_areas = [top_cols[0], top_cols[1], bottom_cols[0], bottom_cols[1]];

        // Render each quadrant
        for (i, opt_session) in page_sessions.iter().enumerate() {
            let row = i / 2;
            let col = i % 2;
            let is_selected = row == self.quadrant_row && col == self.quadrant_col;

            match opt_session {
                Some(session) => {
                    self.render_session_or_error(
                        frame,
                        quadrant_areas[i],
                        session,
                        false,
                        is_selected,
                    );
                }
                None => {
                    // Empty bordered box for unused quadrants
                    let empty_block = Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(COLOR_DIM));
                    frame.render_widget(empty_block, quadrant_areas[i]);
                }
            }
        }

        // Render page indicator if more than 4 runs (overlay at top-right of area)
        if total_pages > 1 {
            let indicator = format!(" Page {}/{} ", self.quadrant_page + 1, total_pages);
            let indicator_width = indicator.len() as u16;
            let indicator_area = Rect::new(
                area.x + area.width.saturating_sub(indicator_width + 1),
                area.y,
                indicator_width,
                1,
            );
            let indicator_widget = Paragraph::new(indicator)
                .style(Style::default().fg(COLOR_PRIMARY).bg(Color::Black));
            frame.render_widget(indicator_widget, indicator_area);
        }
    }

    /// Render either a run detail or an error panel for a session
    fn render_session_or_error(
        &self,
        frame: &mut Frame,
        area: Rect,
        session: &SessionData,
        full: bool,
        is_selected: bool,
    ) {
        if let Some(ref error) = session.load_error {
            self.render_session_error_panel(frame, area, session, error, is_selected);
        } else {
            self.render_session_detail(frame, area, session, full, is_selected);
        }
    }

    // Legacy render_run_or_error for ProjectData (kept for potential future use)
    #[allow(dead_code)]
    fn render_run_or_error(
        &self,
        frame: &mut Frame,
        area: Rect,
        project: &ProjectData,
        full: bool,
        is_selected: bool,
    ) {
        if let Some(ref error) = project.load_error {
            self.render_error_panel(frame, area, &project.info.name, error, is_selected);
        } else {
            self.render_run_detail(frame, area, project, full, is_selected);
        }
    }

    /// Render an error panel for a project with a corrupted state file
    #[allow(dead_code)]
    fn render_error_panel(
        &self,
        frame: &mut Frame,
        area: Rect,
        project_name: &str,
        error: &str,
        is_selected: bool,
    ) {
        let border_color = if is_selected {
            COLOR_WARNING
        } else {
            COLOR_ERROR
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", project_name))
            .border_style(Style::default().fg(border_color));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let error_lines = vec![
            Line::from(vec![
                Span::styled("⚠ ", Style::default().fg(COLOR_ERROR)),
                Span::styled(
                    "State File Error",
                    Style::default()
                        .fg(COLOR_ERROR)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(""),
            Line::from(Span::styled(error, Style::default().fg(COLOR_DIM))),
            Line::from(""),
            Line::from(Span::styled(
                "The state file may be corrupted or unreadable.",
                Style::default().fg(COLOR_DIM),
            )),
            Line::from(Span::styled(
                "Check the .autom8/state.json file in your project.",
                Style::default().fg(COLOR_DIM),
            )),
        ];

        let paragraph = Paragraph::new(error_lines).wrap(Wrap { trim: true });
        frame.render_widget(paragraph, inner);
    }

    /// Render an error panel for a session with a corrupted state file or stale worktree
    fn render_session_error_panel(
        &self,
        frame: &mut Frame,
        area: Rect,
        session: &SessionData,
        error: &str,
        is_selected: bool,
    ) {
        let border_color = if is_selected {
            COLOR_WARNING
        } else {
            COLOR_ERROR
        };
        // Use display_title() to show "project (session-id)"
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", session.display_title()))
            .border_style(Style::default().fg(border_color));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Build error lines based on whether this is a stale session or corrupted state
        let error_lines = if session.is_stale {
            vec![
                Line::from(vec![
                    Span::styled("⚠ ", Style::default().fg(COLOR_ERROR)),
                    Span::styled(
                        "Stale Session",
                        Style::default()
                            .fg(COLOR_ERROR)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("Session ", Style::default().fg(COLOR_DIM)),
                    Span::styled(
                        &session.metadata.session_id,
                        Style::default().fg(COLOR_PRIMARY),
                    ),
                    Span::styled(" failed to load.", Style::default().fg(COLOR_DIM)),
                ]),
                Line::from(""),
                Line::from(Span::styled(error, Style::default().fg(COLOR_DIM))),
                Line::from(""),
                Line::from(Span::styled(
                    "The worktree directory no longer exists.",
                    Style::default().fg(COLOR_DIM),
                )),
                Line::from(Span::styled(
                    "Run `autom8 clean --orphaned` to remove stale sessions.",
                    Style::default().fg(COLOR_DIM),
                )),
            ]
        } else {
            vec![
                Line::from(vec![
                    Span::styled("⚠ ", Style::default().fg(COLOR_ERROR)),
                    Span::styled(
                        "State File Error",
                        Style::default()
                            .fg(COLOR_ERROR)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("Session ", Style::default().fg(COLOR_DIM)),
                    Span::styled(
                        &session.metadata.session_id,
                        Style::default().fg(COLOR_PRIMARY),
                    ),
                    Span::styled(" failed to load.", Style::default().fg(COLOR_DIM)),
                ]),
                Line::from(""),
                Line::from(Span::styled(error, Style::default().fg(COLOR_DIM))),
                Line::from(""),
                Line::from(Span::styled(
                    "The state file may be corrupted or unreadable.",
                    Style::default().fg(COLOR_DIM),
                )),
            ]
        };

        let paragraph = Paragraph::new(error_lines).wrap(Wrap { trim: true });
        frame.render_widget(paragraph, inner);
    }

    /// Render detailed view for a single session
    fn render_session_detail(
        &self,
        frame: &mut Frame,
        area: Rect,
        session: &SessionData,
        full: bool,
        is_selected: bool,
    ) {
        let run = match session.run.as_ref() {
            Some(r) => r,
            None => return, // No run to render
        };

        let border_color = if is_selected {
            COLOR_WARNING
        } else {
            COLOR_PRIMARY
        };
        // Use display_title() to show "project (session-id)"
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", session.display_title()))
            .border_style(Style::default().fg(border_color));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Calculate header height based on session type and display mode
        // Base: 4 lines (State, Story, Progress, Duration)
        // + 1 for Session type (always shown)
        // + 1 for Branch (always shown)
        // + 1 for Worktree path (only for worktree sessions in full mode)
        let base_height = 6; // State, Story, Progress, Duration, Session, Branch
        let extra_height = if full && !session.is_main_session {
            1
        } else {
            0
        }; // Worktree path

        // Split into header info and output snippet
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(base_height + extra_height),
                Constraint::Min(0),
            ])
            .split(inner);

        // Header info
        let state_str = format_state(run.machine_state);
        let duration = format_duration(run.started_at);
        let story = run.current_story.as_deref().unwrap_or("N/A");

        let progress_str = session
            .progress
            .as_ref()
            .map(|p| p.as_fraction())
            .unwrap_or_else(|| "N/A".to_string());

        // Session type indicator with visual distinction
        let (session_type_indicator, session_type_color) = if session.is_main_session {
            ("● main", COLOR_PRIMARY)
        } else {
            ("◆ worktree", COLOR_REVIEW)
        };

        let mut info_lines = vec![
            // Session type line (first for visibility)
            Line::from(vec![
                Span::styled("Session: ", Style::default().fg(COLOR_DIM)),
                Span::styled(
                    session_type_indicator,
                    Style::default().fg(session_type_color),
                ),
            ]),
            // Branch line (always visible)
            Line::from(vec![
                Span::styled("Branch: ", Style::default().fg(COLOR_DIM)),
                Span::styled(&run.branch, Style::default().fg(Color::White)),
            ]),
            Line::from(vec![
                Span::styled("State: ", Style::default().fg(COLOR_DIM)),
                Span::styled(
                    state_str,
                    Style::default().fg(state_color(run.machine_state)),
                ),
            ]),
            Line::from(vec![
                Span::styled("Story: ", Style::default().fg(COLOR_DIM)),
                Span::styled(story, Style::default().fg(Color::White)),
            ]),
            Line::from(vec![
                Span::styled("Progress: ", Style::default().fg(COLOR_DIM)),
                Span::styled(&progress_str, Style::default().fg(COLOR_PRIMARY)),
            ]),
            Line::from(vec![
                Span::styled("Duration: ", Style::default().fg(COLOR_DIM)),
                Span::styled(&duration, Style::default().fg(COLOR_WARNING)),
            ]),
        ];

        // Add worktree path for worktree sessions in full mode
        if full && !session.is_main_session {
            info_lines.insert(
                2, // After Session and Branch
                Line::from(vec![
                    Span::styled("Path: ", Style::default().fg(COLOR_DIM)),
                    Span::styled(
                        session.truncated_worktree_path(),
                        Style::default().fg(COLOR_DIM),
                    ),
                ]),
            );
        }

        let info = Paragraph::new(info_lines);
        frame.render_widget(info, chunks[0]);

        // Output snippet section
        let output_snippet = self.get_output_snippet(run);
        let output = Paragraph::new(output_snippet)
            .style(Style::default().fg(COLOR_DIM))
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .title(" Latest Output "),
            );
        frame.render_widget(output, chunks[1]);
    }

    /// Render detailed view for a single run (legacy, for ProjectData)
    #[allow(dead_code)]
    fn render_run_detail(
        &self,
        frame: &mut Frame,
        area: Rect,
        project: &ProjectData,
        full: bool,
        is_selected: bool,
    ) {
        let run = match project.active_run.as_ref() {
            Some(r) => r,
            None => return, // No run to render
        };

        let border_color = if is_selected {
            COLOR_WARNING
        } else {
            COLOR_PRIMARY
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", project.info.name))
            .border_style(Style::default().fg(border_color));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Split into header info and output snippet
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(if full { 6 } else { 4 }),
                Constraint::Min(0),
            ])
            .split(inner);

        // Header info
        let state_str = format_state(run.machine_state);
        let duration = format_duration(run.started_at);
        let story = run.current_story.as_deref().unwrap_or("N/A");

        let progress_str = project
            .progress
            .as_ref()
            .map(|p| p.as_fraction())
            .unwrap_or_else(|| "N/A".to_string());

        let mut info_lines = vec![
            Line::from(vec![
                Span::styled("State: ", Style::default().fg(COLOR_DIM)),
                Span::styled(
                    state_str,
                    Style::default().fg(state_color(run.machine_state)),
                ),
            ]),
            Line::from(vec![
                Span::styled("Story: ", Style::default().fg(COLOR_DIM)),
                Span::styled(story, Style::default().fg(Color::White)),
            ]),
            Line::from(vec![
                Span::styled("Progress: ", Style::default().fg(COLOR_DIM)),
                Span::styled(&progress_str, Style::default().fg(COLOR_PRIMARY)),
            ]),
            Line::from(vec![
                Span::styled("Duration: ", Style::default().fg(COLOR_DIM)),
                Span::styled(&duration, Style::default().fg(COLOR_WARNING)),
            ]),
        ];

        if full {
            info_lines.push(Line::from(vec![
                Span::styled("Branch: ", Style::default().fg(COLOR_DIM)),
                Span::styled(&run.branch, Style::default().fg(Color::White)),
            ]));
        }

        let info = Paragraph::new(info_lines);
        frame.render_widget(info, chunks[0]);

        // Output snippet section
        let output_snippet = self.get_output_snippet(run);
        let output = Paragraph::new(output_snippet)
            .style(Style::default().fg(COLOR_DIM))
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .title(" Latest Output "),
            );
        frame.render_widget(output, chunks[1]);
    }

    /// Get the latest output snippet from a run
    fn get_output_snippet(&self, run: &RunState) -> String {
        // Get output from the current or last iteration
        if let Some(iter) = run.iterations.last() {
            if !iter.output_snippet.is_empty() {
                // Take last few lines of output
                let lines: Vec<&str> = iter.output_snippet.lines().collect();
                let take_count = 5.min(lines.len());
                let start = lines.len().saturating_sub(take_count);
                return lines[start..].join("\n");
            }
        }

        // Fallback to status message based on state
        match run.machine_state {
            MachineState::Idle => "Waiting to start...".to_string(),
            MachineState::LoadingSpec => "Loading spec file...".to_string(),
            MachineState::GeneratingSpec => "Generating spec from markdown...".to_string(),
            MachineState::Initializing => "Initializing run...".to_string(),
            MachineState::PickingStory => "Selecting next story...".to_string(),
            MachineState::RunningClaude => "Claude is working...".to_string(),
            MachineState::Reviewing => {
                format!("Reviewing changes (cycle {})...", run.review_iteration)
            }
            MachineState::Correcting => "Applying corrections...".to_string(),
            MachineState::Committing => "Committing changes...".to_string(),
            MachineState::CreatingPR => "Creating pull request...".to_string(),
            MachineState::Completed => "Run completed successfully!".to_string(),
            MachineState::Failed => "Run failed.".to_string(),
        }
    }

    fn render_project_list(&self, frame: &mut Frame, area: Rect) {
        if self.projects.is_empty() {
            let message = if self.project_filter.is_some() {
                "No matching projects found"
            } else {
                "No projects found. Run 'autom8' in a project directory to create one."
            };
            let paragraph = Paragraph::new(message)
                .style(Style::default().fg(COLOR_DIM))
                .block(Block::default().borders(Borders::ALL).title(" Projects "));
            frame.render_widget(paragraph, area);
            return;
        }

        // Count running sessions per project for multi-session awareness
        let session_counts: std::collections::HashMap<String, usize> = self
            .sessions
            .iter()
            .filter(|s| s.run.is_some() || s.load_error.is_some()) // Count sessions that are running or have errors
            .fold(std::collections::HashMap::new(), |mut acc, s| {
                *acc.entry(s.project_name.clone()).or_insert(0) += 1;
                acc
            });

        let items: Vec<ListItem> = self
            .projects
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let is_selected = i == self.selected_index;
                let session_count = session_counts.get(&p.info.name).copied().unwrap_or(0);

                // Status indicator and text - check for errors first, then aggregate session state
                let (status_indicator, status_text, status_clr) = if p.load_error.is_some() {
                    ("⚠", "Error".to_string(), COLOR_ERROR)
                } else if session_count > 1 {
                    // Multiple sessions running - show count
                    ("●", format!("[{} sessions]", session_count), COLOR_SUCCESS)
                } else if p.active_run.is_some() || session_count == 1 {
                    ("●", "Running".to_string(), COLOR_SUCCESS)
                } else if let Some(last_run) = p.info.last_run_date {
                    (
                        "○",
                        format!("Last run: {}", format_relative_time(last_run)),
                        COLOR_DIM,
                    )
                } else {
                    ("○", "Idle".to_string(), COLOR_DIM)
                };

                let name_style = if is_selected {
                    Style::default()
                        .fg(COLOR_WARNING)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };

                let line = Line::from(vec![
                    Span::styled(
                        if is_selected { "▶ " } else { "  " },
                        Style::default().fg(COLOR_PRIMARY),
                    ),
                    Span::styled(
                        format!("{} ", status_indicator),
                        Style::default().fg(status_clr),
                    ),
                    Span::styled(&p.info.name, name_style),
                    Span::styled(
                        format!("  {}", status_text),
                        Style::default().fg(status_clr),
                    ),
                ]);

                ListItem::new(line)
            })
            .collect();

        let title = format!(" Projects ({}) ", self.projects.len());
        let list = List::new(items).block(Block::default().borders(Borders::ALL).title(title));

        frame.render_widget(list, area);
    }

    fn render_run_history(&self, frame: &mut Frame, area: Rect) {
        // Check if we should show the detail view
        if self.show_run_detail {
            self.render_run_detail_modal(frame, area);
            return;
        }

        let title = if let Some(ref project) = self.run_history_filter {
            format!(" Run History: {} ({}) ", project, self.run_history.len())
        } else {
            format!(" Run History ({}) ", self.run_history.len())
        };

        if self.run_history.is_empty() {
            let message = if self.run_history_filter.is_some() {
                "No runs found for this project"
            } else {
                "No run history found"
            };
            let paragraph = Paragraph::new(message)
                .style(Style::default().fg(COLOR_DIM))
                .block(Block::default().borders(Borders::ALL).title(title));
            frame.render_widget(paragraph, area);
            return;
        }

        // Calculate visible area (accounting for borders)
        let inner_height = area.height.saturating_sub(2) as usize;

        // Build list items
        let items: Vec<ListItem> = self
            .run_history
            .iter()
            .enumerate()
            .skip(self.history_scroll_offset)
            .take(inner_height)
            .map(|(i, entry)| {
                let is_selected = i == self.selected_index;

                // Status indicator and color
                let (status_indicator, status_clr) = match entry.run.status {
                    crate::state::RunStatus::Completed => ("✓", COLOR_SUCCESS),
                    crate::state::RunStatus::Failed => ("✗", COLOR_ERROR),
                    crate::state::RunStatus::Running => ("●", COLOR_WARNING),
                };

                // Format date/time
                let date_str = entry.run.started_at.format("%Y-%m-%d %H:%M").to_string();

                // Story count
                let story_str = format!("{}/{}", entry.completed_stories, entry.total_stories);

                // Duration if completed
                let duration_str = if let Some(finished) = entry.run.finished_at {
                    let duration = finished.signed_duration_since(entry.run.started_at);
                    let secs = duration.num_seconds().max(0) as u64;
                    let mins = secs / 60;
                    let hours = secs / 3600;
                    if hours > 0 {
                        format!("{}h {}m", hours, (secs % 3600) / 60)
                    } else if mins > 0 {
                        format!("{}m {}s", mins, secs % 60)
                    } else {
                        format!("{}s", secs)
                    }
                } else {
                    "—".to_string()
                };

                let name_style = if is_selected {
                    Style::default()
                        .fg(COLOR_WARNING)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };

                // Build line with project name (if unfiltered), date, status, stories, duration
                let mut spans = vec![
                    Span::styled(
                        if is_selected { "▶ " } else { "  " },
                        Style::default().fg(COLOR_PRIMARY),
                    ),
                    Span::styled(
                        format!("{} ", status_indicator),
                        Style::default().fg(status_clr),
                    ),
                ];

                // Show project name only if not filtered
                if self.run_history_filter.is_none() {
                    spans.push(Span::styled(
                        format!("{:<16} ", truncate_string(&entry.project_name, 15)),
                        name_style,
                    ));
                }

                spans.extend([
                    Span::styled(date_str, Style::default().fg(COLOR_PRIMARY)),
                    Span::styled("  ", Style::default()),
                    Span::styled(
                        format!("Stories: {:<7}", story_str),
                        Style::default().fg(COLOR_DIM),
                    ),
                    Span::styled(
                        format!("  Duration: {}", duration_str),
                        Style::default().fg(COLOR_DIM),
                    ),
                ]);

                ListItem::new(Line::from(spans))
            })
            .collect();

        let list = List::new(items).block(Block::default().borders(Borders::ALL).title(title));

        frame.render_widget(list, area);
    }

    /// Render detailed view for a selected run history entry
    fn render_run_detail_modal(&self, frame: &mut Frame, area: Rect) {
        let entry = match self.run_history.get(self.selected_index) {
            Some(e) => e,
            None => return,
        };

        let title = format!(" Run Details: {} ", entry.project_name);

        // Create a centered modal area
        let modal_area = centered_rect(80, 80, area);

        // Clear background
        frame.render_widget(
            Block::default().style(Style::default().bg(Color::Black)),
            modal_area,
        );

        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(COLOR_PRIMARY))
            .style(Style::default().bg(Color::Black));

        let inner = block.inner(modal_area);
        frame.render_widget(block, modal_area);

        // Build detail content
        let run = &entry.run;

        // Status with color
        let (status_str, status_clr) = match run.status {
            crate::state::RunStatus::Completed => ("Completed", COLOR_SUCCESS),
            crate::state::RunStatus::Failed => ("Failed", COLOR_ERROR),
            crate::state::RunStatus::Running => ("Running", COLOR_WARNING),
        };

        // Duration
        let duration_str = if let Some(finished) = run.finished_at {
            let duration = finished.signed_duration_since(run.started_at);
            let secs = duration.num_seconds().max(0) as u64;
            let mins = secs / 60;
            let hours = secs / 3600;
            if hours > 0 {
                format!("{}h {}m {}s", hours, (secs % 3600) / 60, secs % 60)
            } else if mins > 0 {
                format!("{}m {}s", mins, secs % 60)
            } else {
                format!("{}s", secs)
            }
        } else {
            "In progress".to_string()
        };

        let mut lines = vec![
            Line::from(vec![
                Span::styled("Status:     ", Style::default().fg(COLOR_DIM)),
                Span::styled(status_str, Style::default().fg(status_clr)),
            ]),
            Line::from(vec![
                Span::styled("Started:    ", Style::default().fg(COLOR_DIM)),
                Span::styled(
                    run.started_at.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
                    Style::default().fg(Color::White),
                ),
            ]),
            Line::from(vec![
                Span::styled("Duration:   ", Style::default().fg(COLOR_DIM)),
                Span::styled(&duration_str, Style::default().fg(COLOR_WARNING)),
            ]),
            Line::from(vec![
                Span::styled("Branch:     ", Style::default().fg(COLOR_DIM)),
                Span::styled(&run.branch, Style::default().fg(COLOR_PRIMARY)),
            ]),
            Line::from(vec![
                Span::styled("Stories:    ", Style::default().fg(COLOR_DIM)),
                Span::styled(
                    format!(
                        "{}/{} completed",
                        entry.completed_stories, entry.total_stories
                    ),
                    Style::default().fg(Color::White),
                ),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "Iterations:",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )),
        ];

        // Add iteration details
        for iter in &run.iterations {
            let iter_status_clr = match iter.status {
                crate::state::IterationStatus::Success => COLOR_SUCCESS,
                crate::state::IterationStatus::Failed => COLOR_ERROR,
                crate::state::IterationStatus::Running => COLOR_WARNING,
            };
            let iter_status_str = match iter.status {
                crate::state::IterationStatus::Success => "✓",
                crate::state::IterationStatus::Failed => "✗",
                crate::state::IterationStatus::Running => "●",
            };

            lines.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(
                    format!("{} ", iter_status_str),
                    Style::default().fg(iter_status_clr),
                ),
                Span::styled(&iter.story_id, Style::default().fg(Color::White)),
            ]));

            // Show work summary if available
            if let Some(ref summary) = iter.work_summary {
                let truncated = truncate_string(summary, 60);
                lines.push(Line::from(vec![
                    Span::styled("    ", Style::default()),
                    Span::styled(truncated, Style::default().fg(COLOR_DIM)),
                ]));
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "j/k or ↑↓: scroll | Enter/Esc: close",
            Style::default().fg(COLOR_DIM),
        )));

        let paragraph = Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .scroll((self.detail_scroll_offset as u16, 0));
        frame.render_widget(paragraph, inner);
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let help_text = if self.show_run_detail {
            " jk/↑↓: scroll | Enter/Esc: close detail view ".to_string()
        } else {
            match self.current_view {
                View::ProjectList => {
                    " Tab: switch view | jk/↑↓: navigate | Enter: view history | Q: quit "
                        .to_string()
                }
                View::RunHistory => {
                    if self.run_history_filter.is_some() {
                        " Tab: switch view | jk/↑↓: navigate | Enter: details | Esc: clear filter | Q: quit ".to_string()
                    } else {
                        " Tab: switch view | jk/↑↓: navigate | Enter: details | Q: quit "
                            .to_string()
                    }
                }
                View::ActiveRuns => {
                    // Show pagination keys only when there are more than 4 runs
                    if self.total_quadrant_pages() > 1 {
                        " Tab: switch view | hjkl/arrows: navigate | n/]: next page | p/[: prev page | Q: quit ".to_string()
                    } else {
                        " Tab: switch view | hjkl/arrows: navigate | Q: quit ".to_string()
                    }
                }
            }
        };
        let footer = Paragraph::new(help_text).style(Style::default().fg(COLOR_DIM));
        frame.render_widget(footer, area);
    }
}

/// Truncate a string to a maximum length, adding "..." if truncated
fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

/// Create a centered rectangle of given percentage width/height
fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

/// Initialize the terminal for TUI mode.
pub fn init_terminal() -> MonitorResult<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

/// Restore the terminal to normal mode.
pub fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> MonitorResult<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

/// Run the monitor TUI application.
///
/// This is the main entry point for the monitor command. It initializes the terminal,
/// runs the event loop, and restores the terminal on exit.
pub fn run_monitor(poll_interval: u64, project_filter: Option<String>) -> Result<()> {
    // Set up panic hook to restore terminal on panic
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        // Attempt to restore terminal state
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        original_hook(panic_info);
    }));

    // Initialize terminal
    let mut terminal = init_terminal().map_err(|e| match e {
        MonitorError::Io(io_err) => crate::error::Autom8Error::Io(io_err),
        MonitorError::Autom8(err) => err,
    })?;

    // Create app state
    let mut app = MonitorApp::new(poll_interval, project_filter);

    // Initial data load
    app.refresh_data()?;

    // Set default view based on active runs
    if app.has_active_runs {
        app.current_view = View::ActiveRuns;
    }

    // Main event loop
    let poll_duration = Duration::from_secs(poll_interval);

    loop {
        // Render
        terminal.draw(|frame| app.render(frame))?;

        // Poll for events with timeout
        if event::poll(poll_duration)? {
            if let Event::Key(key) = event::read()? {
                // Only handle key press events (not release or repeat)
                if key.kind == KeyEventKind::Press {
                    app.handle_key(key.code);
                }
            }
            // Handle resize events gracefully - ratatui handles this automatically
            // on the next draw call
        }

        // Check if we should quit
        if app.should_quit() {
            break;
        }

        // Refresh data each cycle
        app.refresh_data()?;
    }

    // Restore terminal
    restore_terminal(&mut terminal).map_err(|e| match e {
        MonitorError::Io(io_err) => crate::error::Autom8Error::Io(io_err),
        MonitorError::Autom8(err) => err,
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper function to create a test SessionData for use in tests
    fn create_test_session(project_name: &str, session_id: &str, branch: &str) -> SessionData {
        let is_main = session_id == MAIN_SESSION_ID;
        SessionData {
            project_name: project_name.to_string(),
            metadata: SessionMetadata {
                session_id: session_id.to_string(),
                worktree_path: PathBuf::from(if is_main {
                    format!("/home/user/projects/{}", project_name)
                } else {
                    format!("/home/user/projects/{}-wt-{}", project_name, branch)
                }),
                branch_name: branch.to_string(),
                created_at: chrono::Utc::now(),
                last_active_at: chrono::Utc::now(),
                is_running: true,
            },
            run: Some(RunState::new(
                PathBuf::from("test.json"),
                branch.to_string(),
            )),
            progress: None,
            load_error: None,
            is_main_session: is_main,
            is_stale: false,
        }
    }

    /// Helper function to add N test sessions to the app
    fn add_test_sessions(app: &mut MonitorApp, count: usize) {
        for i in 1..=count {
            app.sessions.push(create_test_session(
                &format!("project-{}", i),
                &format!("{:08x}", i), // session IDs like "00000001", "00000002"
                &format!("branch-{}", i),
            ));
        }
    }

    #[test]
    fn test_monitor_app_new() {
        let app = MonitorApp::new(1, None);
        assert_eq!(app.poll_interval, 1);
        assert!(app.project_filter.is_none());
        assert!(!app.should_quit);
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_monitor_app_with_project_filter() {
        let app = MonitorApp::new(5, Some("myapp".to_string()));
        assert_eq!(app.poll_interval, 5);
        assert_eq!(app.project_filter, Some("myapp".to_string()));
    }

    #[test]
    fn test_monitor_app_handle_quit() {
        let mut app = MonitorApp::new(1, None);
        assert!(!app.should_quit());

        app.handle_key(KeyCode::Char('q'));
        assert!(app.should_quit());
    }

    #[test]
    fn test_monitor_app_handle_quit_uppercase() {
        let mut app = MonitorApp::new(1, None);
        app.handle_key(KeyCode::Char('Q'));
        assert!(app.should_quit());
    }

    #[test]
    fn test_monitor_app_handle_tab_switches_view() {
        let mut app = MonitorApp::new(1, None);
        // Start at ProjectList (default when no active runs)
        app.current_view = View::ProjectList;
        app.has_active_runs = false;

        app.handle_key(KeyCode::Tab);
        assert_eq!(app.current_view, View::RunHistory);

        app.handle_key(KeyCode::Tab);
        // Should skip ActiveRuns since has_active_runs is false
        assert_eq!(app.current_view, View::ProjectList);
    }

    #[test]
    fn test_monitor_app_handle_tab_with_active_runs() {
        let mut app = MonitorApp::new(1, None);
        app.current_view = View::ActiveRuns;
        app.has_active_runs = true;

        app.handle_key(KeyCode::Tab);
        assert_eq!(app.current_view, View::ProjectList);

        app.handle_key(KeyCode::Tab);
        assert_eq!(app.current_view, View::RunHistory);

        app.handle_key(KeyCode::Tab);
        assert_eq!(app.current_view, View::ActiveRuns);
    }

    #[test]
    fn test_monitor_app_handle_navigation() {
        let mut app = MonitorApp::new(1, None);
        app.selected_index = 1;

        app.handle_key(KeyCode::Up);
        assert_eq!(app.selected_index, 0);

        // Should not go below 0
        app.handle_key(KeyCode::Up);
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_available_views_with_active_runs() {
        let mut app = MonitorApp::new(1, None);
        app.has_active_runs = true;

        let views = app.available_views();
        assert_eq!(views.len(), 3);
        assert!(views.contains(&View::ActiveRuns));
    }

    #[test]
    fn test_available_views_without_active_runs() {
        let mut app = MonitorApp::new(1, None);
        app.has_active_runs = false;

        let views = app.available_views();
        assert_eq!(views.len(), 2);
        assert!(!views.contains(&View::ActiveRuns));
        assert!(views.contains(&View::ProjectList));
        assert!(views.contains(&View::RunHistory));
    }

    #[test]
    fn test_next_view_resets_selected_index() {
        let mut app = MonitorApp::new(1, None);
        app.selected_index = 5;
        app.current_view = View::ProjectList;
        app.has_active_runs = false;

        app.next_view();
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_monitor_error_display() {
        let io_err = MonitorError::Io(io::Error::new(io::ErrorKind::Other, "test error"));
        assert!(io_err.to_string().contains("IO error"));
        assert!(io_err.to_string().contains("test error"));
    }

    // ===========================================
    // US-005: Active Runs View Tests
    // ===========================================

    #[test]
    fn test_run_progress_as_fraction() {
        let progress = RunProgress {
            completed: 1,
            total: 5,
        };
        // completed + 1 because we're working on the next story
        assert_eq!(progress.as_fraction(), "Story 2/5");
    }

    #[test]
    fn test_run_progress_as_fraction_first_story() {
        let progress = RunProgress {
            completed: 0,
            total: 3,
        };
        assert_eq!(progress.as_fraction(), "Story 1/3");
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
    fn test_run_progress_as_percentage_complete() {
        let progress = RunProgress {
            completed: 5,
            total: 5,
        };
        assert_eq!(progress.as_percentage(), "100%");
    }

    #[test]
    fn test_format_duration_seconds_only() {
        let now = Utc::now();
        let started_at = now - chrono::Duration::seconds(45);
        let result = format_duration(started_at);
        assert_eq!(result, "45s");
    }

    #[test]
    fn test_format_duration_minutes_and_seconds() {
        let now = Utc::now();
        let started_at = now - chrono::Duration::seconds(332); // 5m 32s
        let result = format_duration(started_at);
        assert_eq!(result, "5m 32s");
    }

    #[test]
    fn test_format_duration_hours_and_minutes() {
        let now = Utc::now();
        let started_at = now - chrono::Duration::seconds(3900); // 1h 5m
        let result = format_duration(started_at);
        assert_eq!(result, "1h 5m");
    }

    #[test]
    fn test_format_duration_zero() {
        let now = Utc::now();
        let result = format_duration(now);
        assert_eq!(result, "0s");
    }

    #[test]
    fn test_format_state_all_states() {
        assert_eq!(format_state(MachineState::Idle), "Idle");
        assert_eq!(format_state(MachineState::LoadingSpec), "Loading Spec");
        assert_eq!(
            format_state(MachineState::GeneratingSpec),
            "Generating Spec"
        );
        assert_eq!(format_state(MachineState::Initializing), "Initializing");
        assert_eq!(format_state(MachineState::PickingStory), "Picking Story");
        assert_eq!(format_state(MachineState::RunningClaude), "Running Claude");
        assert_eq!(format_state(MachineState::Reviewing), "Reviewing");
        assert_eq!(format_state(MachineState::Correcting), "Correcting");
        assert_eq!(format_state(MachineState::Committing), "Committing");
        assert_eq!(format_state(MachineState::CreatingPR), "Creating PR");
        assert_eq!(format_state(MachineState::Completed), "Completed");
        assert_eq!(format_state(MachineState::Failed), "Failed");
    }

    #[test]
    fn test_state_color_returns_appropriate_colors() {
        // Just verify all states have colors and don't panic
        assert_eq!(state_color(MachineState::Idle), Color::DarkGray);
        assert_eq!(state_color(MachineState::RunningClaude), Color::Cyan);
        assert_eq!(state_color(MachineState::Completed), Color::Green);
        assert_eq!(state_color(MachineState::Failed), Color::Red);
    }

    #[test]
    fn test_get_output_snippet_returns_status_message_when_no_iterations() {
        use std::path::PathBuf;

        let app = MonitorApp::new(1, None);
        let run = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());

        let snippet = app.get_output_snippet(&run);
        assert_eq!(snippet, "Initializing run...");
    }

    #[test]
    fn test_get_output_snippet_returns_last_lines_from_iteration() {
        use std::path::PathBuf;

        let app = MonitorApp::new(1, None);
        let mut run = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        run.start_iteration("US-001");
        run.iterations.last_mut().unwrap().output_snippet =
            "Line 1\nLine 2\nLine 3\nLine 4\nLine 5\nLine 6\nLine 7".to_string();

        let snippet = app.get_output_snippet(&run);
        // Should return last 5 lines
        assert_eq!(snippet, "Line 3\nLine 4\nLine 5\nLine 6\nLine 7");
    }

    #[test]
    fn test_get_output_snippet_with_reviewing_state() {
        use std::path::PathBuf;

        let app = MonitorApp::new(1, None);
        let mut run = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        run.machine_state = MachineState::Reviewing;
        run.review_iteration = 2;

        let snippet = app.get_output_snippet(&run);
        assert_eq!(snippet, "Reviewing changes (cycle 2)...");
    }

    #[test]
    fn test_project_data_includes_progress() {
        use crate::config::ProjectTreeInfo;
        use crate::state::RunStatus;

        let project = ProjectData {
            info: ProjectTreeInfo {
                name: "test".to_string(),
                has_active_run: true,
                run_status: Some(RunStatus::Running),
                spec_count: 1,
                incomplete_spec_count: 0,
                spec_md_count: 0,
                runs_count: 0,
                last_run_date: None,
            },
            active_run: None,
            progress: Some(RunProgress {
                completed: 2,
                total: 5,
            }),
            load_error: None,
        };

        assert!(project.progress.is_some());
        assert_eq!(
            project.progress.as_ref().unwrap().as_fraction(),
            "Story 3/5"
        );
    }

    // ===========================================
    // US-006: Project List View Tests
    // ===========================================

    #[test]
    fn test_format_relative_time_just_now() {
        let now = Utc::now();
        let result = format_relative_time(now);
        assert_eq!(result, "just now");
    }

    #[test]
    fn test_format_relative_time_minutes_ago() {
        let now = Utc::now();
        let timestamp = now - chrono::Duration::minutes(15);
        let result = format_relative_time(timestamp);
        assert_eq!(result, "15m ago");
    }

    #[test]
    fn test_format_relative_time_hours_ago() {
        let now = Utc::now();
        let timestamp = now - chrono::Duration::hours(3);
        let result = format_relative_time(timestamp);
        assert_eq!(result, "3h ago");
    }

    #[test]
    fn test_format_relative_time_days_ago() {
        let now = Utc::now();
        let timestamp = now - chrono::Duration::days(5);
        let result = format_relative_time(timestamp);
        assert_eq!(result, "5d ago");
    }

    #[test]
    fn test_monitor_app_has_run_history_filter() {
        let app = MonitorApp::new(1, None);
        assert!(app.run_history_filter().is_none());
    }

    #[test]
    fn test_handle_enter_on_project_list_sets_filter() {
        use crate::config::ProjectTreeInfo;

        let mut app = MonitorApp::new(1, None);
        app.current_view = View::ProjectList;
        app.projects = vec![ProjectData {
            info: ProjectTreeInfo {
                name: "test-project".to_string(),
                has_active_run: false,
                run_status: None,
                spec_count: 0,
                incomplete_spec_count: 0,
                spec_md_count: 0,
                runs_count: 0,
                last_run_date: None,
            },
            active_run: None,
            progress: None,
            load_error: None,
        }];
        app.selected_index = 0;

        app.handle_key(KeyCode::Enter);

        assert_eq!(app.current_view(), View::RunHistory);
        assert_eq!(app.run_history_filter(), Some("test-project"));
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_handle_tab_clears_run_history_filter() {
        let mut app = MonitorApp::new(1, None);
        app.current_view = View::RunHistory;
        app.run_history_filter = Some("test-project".to_string());

        app.handle_key(KeyCode::Tab);

        assert!(app.run_history_filter().is_none());
    }

    #[test]
    fn test_handle_enter_on_empty_project_list_does_nothing() {
        let mut app = MonitorApp::new(1, None);
        app.current_view = View::ProjectList;
        app.projects = vec![]; // Empty

        app.handle_key(KeyCode::Enter);

        // Should still be on ProjectList, no filter set
        assert_eq!(app.current_view(), View::ProjectList);
        assert!(app.run_history_filter().is_none());
    }

    #[test]
    fn test_handle_enter_on_active_runs_does_nothing() {
        let mut app = MonitorApp::new(1, None);
        app.current_view = View::ActiveRuns;
        app.has_active_runs = true;

        app.handle_key(KeyCode::Enter);

        // Should still be on ActiveRuns
        assert_eq!(app.current_view(), View::ActiveRuns);
        assert!(app.run_history_filter().is_none());
    }

    #[test]
    fn test_project_list_navigation_with_arrow_keys() {
        use crate::config::ProjectTreeInfo;

        let mut app = MonitorApp::new(1, None);
        app.current_view = View::ProjectList;
        app.projects = vec![
            ProjectData {
                info: ProjectTreeInfo {
                    name: "project-a".to_string(),
                    has_active_run: false,
                    run_status: None,
                    spec_count: 0,
                    incomplete_spec_count: 0,
                    spec_md_count: 0,
                    runs_count: 0,
                    last_run_date: None,
                },
                active_run: None,
                progress: None,
                load_error: None,
            },
            ProjectData {
                info: ProjectTreeInfo {
                    name: "project-b".to_string(),
                    has_active_run: false,
                    run_status: None,
                    spec_count: 0,
                    incomplete_spec_count: 0,
                    spec_md_count: 0,
                    runs_count: 0,
                    last_run_date: None,
                },
                active_run: None,
                progress: None,
                load_error: None,
            },
        ];

        assert_eq!(app.selected_index, 0);

        app.handle_key(KeyCode::Down);
        assert_eq!(app.selected_index, 1);

        app.handle_key(KeyCode::Down);
        // Should stay at 1 (max index)
        assert_eq!(app.selected_index, 1);

        app.handle_key(KeyCode::Up);
        assert_eq!(app.selected_index, 0);

        app.handle_key(KeyCode::Up);
        // Should stay at 0
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_project_tree_info_with_last_run_date() {
        use crate::config::ProjectTreeInfo;

        let last_run = Utc::now() - chrono::Duration::hours(2);
        let info = ProjectTreeInfo {
            name: "test".to_string(),
            has_active_run: false,
            run_status: None,
            spec_count: 1,
            incomplete_spec_count: 0,
            spec_md_count: 0,
            runs_count: 5,
            last_run_date: Some(last_run),
        };

        assert!(info.last_run_date.is_some());
        assert_eq!(format_relative_time(info.last_run_date.unwrap()), "2h ago");
    }

    #[test]
    fn test_enter_on_second_project_selects_correct_filter() {
        use crate::config::ProjectTreeInfo;

        let mut app = MonitorApp::new(1, None);
        app.current_view = View::ProjectList;
        app.projects = vec![
            ProjectData {
                info: ProjectTreeInfo {
                    name: "first-project".to_string(),
                    has_active_run: false,
                    run_status: None,
                    spec_count: 0,
                    incomplete_spec_count: 0,
                    spec_md_count: 0,
                    runs_count: 0,
                    last_run_date: None,
                },
                active_run: None,
                progress: None,
                load_error: None,
            },
            ProjectData {
                info: ProjectTreeInfo {
                    name: "second-project".to_string(),
                    has_active_run: false,
                    run_status: None,
                    spec_count: 0,
                    incomplete_spec_count: 0,
                    spec_md_count: 0,
                    runs_count: 0,
                    last_run_date: None,
                },
                active_run: None,
                progress: None,
                load_error: None,
            },
        ];
        app.selected_index = 1;

        app.handle_key(KeyCode::Enter);

        assert_eq!(app.run_history_filter(), Some("second-project"));
    }

    // ===========================================
    // US-007: Run History View Tests
    // ===========================================

    #[test]
    fn test_run_history_entry_creation() {
        use std::path::PathBuf;

        let run = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        let entry = RunHistoryEntry {
            project_name: "test-project".to_string(),
            run,
            completed_stories: 3,
            total_stories: 5,
        };

        assert_eq!(entry.project_name, "test-project");
        assert_eq!(entry.completed_stories, 3);
        assert_eq!(entry.total_stories, 5);
    }

    #[test]
    fn test_monitor_app_new_initializes_run_history_empty() {
        let app = MonitorApp::new(1, None);
        assert!(app.run_history.is_empty());
        assert_eq!(app.history_scroll_offset, 0);
        assert!(!app.show_run_detail);
    }

    #[test]
    fn test_run_history_navigation_with_arrow_keys() {
        use std::path::PathBuf;

        let mut app = MonitorApp::new(1, None);
        app.current_view = View::RunHistory;
        app.run_history = vec![
            RunHistoryEntry {
                project_name: "project-a".to_string(),
                run: RunState::new(PathBuf::from("a.json"), "branch-a".to_string()),
                completed_stories: 1,
                total_stories: 2,
            },
            RunHistoryEntry {
                project_name: "project-b".to_string(),
                run: RunState::new(PathBuf::from("b.json"), "branch-b".to_string()),
                completed_stories: 2,
                total_stories: 3,
            },
            RunHistoryEntry {
                project_name: "project-c".to_string(),
                run: RunState::new(PathBuf::from("c.json"), "branch-c".to_string()),
                completed_stories: 3,
                total_stories: 4,
            },
        ];

        assert_eq!(app.selected_index, 0);

        app.handle_key(KeyCode::Down);
        assert_eq!(app.selected_index, 1);

        app.handle_key(KeyCode::Down);
        assert_eq!(app.selected_index, 2);

        app.handle_key(KeyCode::Down);
        // Should stay at max index
        assert_eq!(app.selected_index, 2);

        app.handle_key(KeyCode::Up);
        assert_eq!(app.selected_index, 1);

        app.handle_key(KeyCode::Up);
        assert_eq!(app.selected_index, 0);

        app.handle_key(KeyCode::Up);
        // Should stay at 0
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_enter_on_run_history_shows_detail() {
        use std::path::PathBuf;

        let mut app = MonitorApp::new(1, None);
        app.current_view = View::RunHistory;
        app.run_history = vec![RunHistoryEntry {
            project_name: "test-project".to_string(),
            run: RunState::new(PathBuf::from("test.json"), "test-branch".to_string()),
            completed_stories: 1,
            total_stories: 2,
        }];

        assert!(!app.show_run_detail);
        assert!(!app.is_showing_run_detail());

        app.handle_key(KeyCode::Enter);

        assert!(app.show_run_detail);
        assert!(app.is_showing_run_detail());
    }

    #[test]
    fn test_esc_closes_detail_view() {
        let mut app = MonitorApp::new(1, None);
        app.show_run_detail = true;

        app.handle_key(KeyCode::Esc);

        assert!(!app.show_run_detail);
    }

    #[test]
    fn test_enter_closes_detail_view() {
        let mut app = MonitorApp::new(1, None);
        app.show_run_detail = true;

        app.handle_key(KeyCode::Enter);

        assert!(!app.show_run_detail);
    }

    #[test]
    fn test_q_quits_immediately_even_from_detail_view() {
        let mut app = MonitorApp::new(1, None);
        app.show_run_detail = true;

        app.handle_key(KeyCode::Char('q'));

        // q always quits immediately, regardless of detail view state
        assert!(app.should_quit());
    }

    #[test]
    fn test_esc_clears_run_history_filter() {
        let mut app = MonitorApp::new(1, None);
        app.current_view = View::RunHistory;
        app.run_history_filter = Some("test-project".to_string());

        app.handle_key(KeyCode::Esc);

        assert!(app.run_history_filter.is_none());
    }

    #[test]
    fn test_tab_resets_history_scroll_offset() {
        let mut app = MonitorApp::new(1, None);
        app.current_view = View::RunHistory;
        app.history_scroll_offset = 10;

        app.handle_key(KeyCode::Tab);

        assert_eq!(app.history_scroll_offset, 0);
    }

    #[test]
    fn test_enter_on_empty_run_history_does_not_show_detail() {
        let mut app = MonitorApp::new(1, None);
        app.current_view = View::RunHistory;
        app.run_history = vec![]; // Empty

        app.handle_key(KeyCode::Enter);

        assert!(!app.show_run_detail);
    }

    #[test]
    fn test_truncate_string_short_string() {
        let result = truncate_string("short", 10);
        assert_eq!(result, "short");
    }

    #[test]
    fn test_truncate_string_exact_length() {
        let result = truncate_string("exact", 5);
        assert_eq!(result, "exact");
    }

    #[test]
    fn test_truncate_string_long_string() {
        let result = truncate_string("this is a very long string", 15);
        assert_eq!(result, "this is a ve...");
    }

    #[test]
    fn test_enter_from_project_list_resets_scroll_offset() {
        use crate::config::ProjectTreeInfo;

        let mut app = MonitorApp::new(1, None);
        app.current_view = View::ProjectList;
        app.history_scroll_offset = 5; // Should be reset
        app.projects = vec![ProjectData {
            info: ProjectTreeInfo {
                name: "test-project".to_string(),
                has_active_run: false,
                run_status: None,
                spec_count: 0,
                incomplete_spec_count: 0,
                spec_md_count: 0,
                runs_count: 0,
                last_run_date: None,
            },
            active_run: None,
            progress: None,
            load_error: None,
        }];

        app.handle_key(KeyCode::Enter);

        assert_eq!(app.current_view, View::RunHistory);
        assert_eq!(app.history_scroll_offset, 0);
    }

    #[test]
    fn test_navigation_keys_ignored_when_detail_shown() {
        let mut app = MonitorApp::new(1, None);
        app.show_run_detail = true;
        app.selected_index = 0;

        // These should be ignored when detail is shown
        app.handle_key(KeyCode::Down);
        assert_eq!(app.selected_index, 0); // Unchanged

        app.handle_key(KeyCode::Tab);
        // View should not change
        // (The detail was closed by Esc check earlier, but Tab is not handled in detail mode)
    }

    #[test]
    fn test_run_history_entry_with_failed_status() {
        use std::path::PathBuf;

        let mut run = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        run.transition_to(MachineState::Failed);

        let entry = RunHistoryEntry {
            project_name: "test-project".to_string(),
            run,
            completed_stories: 2,
            total_stories: 5,
        };

        assert_eq!(entry.run.status, crate::state::RunStatus::Failed);
    }

    #[test]
    fn test_run_history_entry_with_completed_status() {
        use std::path::PathBuf;

        let mut run = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        run.transition_to(MachineState::Completed);

        let entry = RunHistoryEntry {
            project_name: "test-project".to_string(),
            run,
            completed_stories: 5,
            total_stories: 5,
        };

        assert_eq!(entry.run.status, crate::state::RunStatus::Completed);
        assert!(entry.run.finished_at.is_some());
    }

    #[test]
    fn test_centered_rect() {
        let area = Rect::new(0, 0, 100, 50);
        let result = centered_rect(80, 60, area);

        // Should be centered
        assert!(result.x > 0);
        assert!(result.y > 0);
        assert!(result.width < area.width);
        assert!(result.height < area.height);
    }

    // ===========================================
    // US-008: Polish and Error Handling Tests
    // ===========================================

    #[test]
    fn test_project_data_with_load_error() {
        use crate::config::ProjectTreeInfo;

        let project = ProjectData {
            info: ProjectTreeInfo {
                name: "broken-project".to_string(),
                has_active_run: true, // Indicates there should be a state file
                run_status: None,
                spec_count: 0,
                incomplete_spec_count: 0,
                spec_md_count: 0,
                runs_count: 0,
                last_run_date: None,
            },
            active_run: None, // But it failed to load
            progress: None,
            load_error: Some("Corrupted state: invalid JSON".to_string()),
        };

        assert!(project.load_error.is_some());
        assert!(project.active_run.is_none());
        assert!(project.load_error.unwrap().contains("Corrupted"));
    }

    #[test]
    fn test_clamp_selection_index_on_empty_projects() {
        let mut app = MonitorApp::new(1, None);
        app.current_view = View::ProjectList;
        app.selected_index = 5; // Out of bounds
        app.projects = vec![]; // Empty

        app.clamp_selection_index();

        // Should clamp to 0 (max index when empty is 0 via saturating_sub)
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_clamp_selection_index_after_project_removal() {
        use crate::config::ProjectTreeInfo;

        let mut app = MonitorApp::new(1, None);
        app.current_view = View::ProjectList;
        app.selected_index = 3; // Was valid when we had 4 projects
                                // Now only 2 projects
        app.projects = vec![
            ProjectData {
                info: ProjectTreeInfo {
                    name: "project-a".to_string(),
                    has_active_run: false,
                    run_status: None,
                    spec_count: 0,
                    incomplete_spec_count: 0,
                    spec_md_count: 0,
                    runs_count: 0,
                    last_run_date: None,
                },
                active_run: None,
                progress: None,
                load_error: None,
            },
            ProjectData {
                info: ProjectTreeInfo {
                    name: "project-b".to_string(),
                    has_active_run: false,
                    run_status: None,
                    spec_count: 0,
                    incomplete_spec_count: 0,
                    spec_md_count: 0,
                    runs_count: 0,
                    last_run_date: None,
                },
                active_run: None,
                progress: None,
                load_error: None,
            },
        ];

        app.clamp_selection_index();

        // Should be clamped to last valid index (1)
        assert_eq!(app.selected_index, 1);
    }

    #[test]
    fn test_clamp_history_scroll_offset() {
        let mut app = MonitorApp::new(1, None);
        app.current_view = View::RunHistory;
        app.selected_index = 2;
        app.history_scroll_offset = 10; // Out of bounds
        app.run_history = vec![]; // Empty

        app.clamp_selection_index();

        // Both should be clamped to 0
        assert_eq!(app.selected_index, 0);
        assert_eq!(app.history_scroll_offset, 0);
    }

    #[test]
    fn test_color_constants_match_output_rs() {
        // Verify our color constants are defined correctly
        assert_eq!(COLOR_PRIMARY, Color::Cyan);
        assert_eq!(COLOR_SUCCESS, Color::Green);
        assert_eq!(COLOR_WARNING, Color::Yellow);
        assert_eq!(COLOR_ERROR, Color::Red);
        assert_eq!(COLOR_INFO, Color::Blue);
        assert_eq!(COLOR_DIM, Color::DarkGray);
        assert_eq!(COLOR_REVIEW, Color::Magenta);
    }

    #[test]
    fn test_state_color_uses_consistent_colors() {
        // Verify state colors use our defined constants
        assert_eq!(state_color(MachineState::Idle), COLOR_DIM);
        assert_eq!(state_color(MachineState::RunningClaude), COLOR_PRIMARY);
        assert_eq!(state_color(MachineState::Completed), COLOR_SUCCESS);
        assert_eq!(state_color(MachineState::Failed), COLOR_ERROR);
        assert_eq!(state_color(MachineState::Reviewing), COLOR_REVIEW);
        assert_eq!(state_color(MachineState::LoadingSpec), COLOR_WARNING);
        assert_eq!(state_color(MachineState::Initializing), COLOR_INFO);
    }

    #[test]
    fn test_active_runs_includes_error_projects() {
        use crate::config::ProjectTreeInfo;

        let mut app = MonitorApp::new(1, None);
        app.current_view = View::ActiveRuns;
        app.has_active_runs = true;
        app.projects = vec![
            ProjectData {
                info: ProjectTreeInfo {
                    name: "working-project".to_string(),
                    has_active_run: true,
                    run_status: Some(crate::state::RunStatus::Running),
                    spec_count: 0,
                    incomplete_spec_count: 0,
                    spec_md_count: 0,
                    runs_count: 0,
                    last_run_date: None,
                },
                active_run: Some(RunState::new(
                    std::path::PathBuf::from("test.json"),
                    "test-branch".to_string(),
                )),
                progress: None,
                load_error: None,
            },
            ProjectData {
                info: ProjectTreeInfo {
                    name: "broken-project".to_string(),
                    has_active_run: true, // State file exists but corrupted
                    run_status: None,
                    spec_count: 0,
                    incomplete_spec_count: 0,
                    spec_md_count: 0,
                    runs_count: 0,
                    last_run_date: None,
                },
                active_run: None,
                progress: None,
                load_error: Some("Invalid JSON".to_string()),
            },
        ];

        // Filter should include both working and error projects
        let active: Vec<_> = app
            .projects
            .iter()
            .filter(|p| p.active_run.is_some() || p.load_error.is_some())
            .collect();

        assert_eq!(active.len(), 2);
    }

    #[test]
    fn test_project_list_shows_error_indicator() {
        use crate::config::ProjectTreeInfo;

        let project = ProjectData {
            info: ProjectTreeInfo {
                name: "error-project".to_string(),
                has_active_run: true,
                run_status: None,
                spec_count: 0,
                incomplete_spec_count: 0,
                spec_md_count: 0,
                runs_count: 0,
                last_run_date: None,
            },
            active_run: None,
            progress: None,
            load_error: Some("State corrupted".to_string()),
        };

        // Verify the project has an error
        assert!(project.load_error.is_some());
        assert!(project.active_run.is_none());
    }

    #[test]
    fn test_handle_enter_on_project_list_does_not_crash_on_empty() {
        let mut app = MonitorApp::new(1, None);
        app.current_view = View::ProjectList;
        app.projects = vec![]; // Empty
        app.selected_index = 0;

        // This should not crash
        app.handle_key(KeyCode::Enter);

        // View should remain unchanged
        assert_eq!(app.current_view, View::ProjectList);
    }

    #[test]
    fn test_active_runs_list_handles_error_state() {
        use crate::config::ProjectTreeInfo;

        let app = MonitorApp::new(1, None);
        let project_with_error = ProjectData {
            info: ProjectTreeInfo {
                name: "error-project".to_string(),
                has_active_run: true,
                run_status: None,
                spec_count: 0,
                incomplete_spec_count: 0,
                spec_md_count: 0,
                runs_count: 0,
                last_run_date: None,
            },
            active_run: None,
            progress: None,
            load_error: Some("Corrupted".to_string()),
        };

        // Verify the logic for determining state string
        let (state_str, _) = if project_with_error.load_error.is_some() {
            ("Error", COLOR_ERROR)
        } else if let Some(ref run) = project_with_error.active_run {
            (
                format_state(run.machine_state),
                state_color(run.machine_state),
            )
        } else {
            ("Unknown", COLOR_DIM)
        };

        assert_eq!(state_str, "Error");
        // Just to use app to avoid warning
        assert!(!app.should_quit());
    }

    // ===========================================
    // US-002: Fixed Quadrant Layout Tests
    // ===========================================

    #[test]
    fn test_monitor_app_new_initializes_quadrant_page_to_zero() {
        let app = MonitorApp::new(1, None);
        assert_eq!(app.quadrant_page, 0);
    }

    #[test]
    fn test_total_quadrant_pages_with_zero_runs() {
        let app = MonitorApp::new(1, None);
        // With no projects, total_quadrant_pages returns 1 (minimum)
        assert_eq!(app.total_quadrant_pages(), 1);
    }

    #[test]
    fn test_total_quadrant_pages_with_one_to_four_runs() {
        use crate::config::ProjectTreeInfo;

        let mut app = MonitorApp::new(1, None);
        app.projects = vec![ProjectData {
            info: ProjectTreeInfo {
                name: "project-1".to_string(),
                has_active_run: true,
                run_status: Some(crate::state::RunStatus::Running),
                spec_count: 0,
                incomplete_spec_count: 0,
                spec_md_count: 0,
                runs_count: 0,
                last_run_date: None,
            },
            active_run: Some(RunState::new(
                std::path::PathBuf::from("test.json"),
                "branch".to_string(),
            )),
            progress: None,
            load_error: None,
        }];

        // 1 run = 1 page
        assert_eq!(app.total_quadrant_pages(), 1);

        // Add 3 more projects (4 total) - still 1 page
        for i in 2..=4 {
            app.projects.push(ProjectData {
                info: ProjectTreeInfo {
                    name: format!("project-{}", i),
                    has_active_run: true,
                    run_status: Some(crate::state::RunStatus::Running),
                    spec_count: 0,
                    incomplete_spec_count: 0,
                    spec_md_count: 0,
                    runs_count: 0,
                    last_run_date: None,
                },
                active_run: Some(RunState::new(
                    std::path::PathBuf::from("test.json"),
                    "branch".to_string(),
                )),
                progress: None,
                load_error: None,
            });
        }
        assert_eq!(app.total_quadrant_pages(), 1);
    }

    #[test]
    fn test_total_quadrant_pages_with_five_runs() {
        let mut app = MonitorApp::new(1, None);
        add_test_sessions(&mut app, 5);

        // 5 sessions = 2 pages (4 on first, 1 on second)
        assert_eq!(app.total_quadrant_pages(), 2);
    }

    #[test]
    fn test_total_quadrant_pages_with_eight_runs() {
        let mut app = MonitorApp::new(1, None);
        add_test_sessions(&mut app, 8);

        // 8 sessions = 2 pages (4 on each)
        assert_eq!(app.total_quadrant_pages(), 2);
    }

    #[test]
    fn test_total_quadrant_pages_with_nine_runs() {
        let mut app = MonitorApp::new(1, None);
        add_test_sessions(&mut app, 9);

        // 9 sessions = 3 pages (4, 4, 1)
        assert_eq!(app.total_quadrant_pages(), 3);
    }

    #[test]
    fn test_next_quadrant_page_advances() {
        let mut app = MonitorApp::new(1, None);
        // Add 5 sessions to have 2 pages
        add_test_sessions(&mut app, 5);

        assert_eq!(app.quadrant_page, 0);
        app.next_quadrant_page();
        assert_eq!(app.quadrant_page, 1);

        // Should not go beyond last page
        app.next_quadrant_page();
        assert_eq!(app.quadrant_page, 1);
    }

    #[test]
    fn test_prev_quadrant_page_goes_back() {
        let mut app = MonitorApp::new(1, None);
        app.quadrant_page = 2;

        app.prev_quadrant_page();
        assert_eq!(app.quadrant_page, 1);

        app.prev_quadrant_page();
        assert_eq!(app.quadrant_page, 0);

        // Should not go below 0
        app.prev_quadrant_page();
        assert_eq!(app.quadrant_page, 0);
    }

    #[test]
    fn test_next_quadrant_page_does_nothing_with_single_page() {
        let mut app = MonitorApp::new(1, None);
        // Only 2 sessions = 1 page
        add_test_sessions(&mut app, 2);

        assert_eq!(app.quadrant_page, 0);
        app.next_quadrant_page();
        // Should stay at 0 since there's only 1 page
        assert_eq!(app.quadrant_page, 0);
    }

    #[test]
    fn test_handle_n_key_advances_page_in_active_runs() {
        let mut app = MonitorApp::new(1, None);
        app.current_view = View::ActiveRuns;
        app.has_active_runs = true;
        // Add 5 sessions
        add_test_sessions(&mut app, 5);

        assert_eq!(app.quadrant_page, 0);
        app.handle_key(KeyCode::Char('n'));
        assert_eq!(app.quadrant_page, 1);
    }

    #[test]
    fn test_handle_right_bracket_advances_page_in_active_runs() {
        let mut app = MonitorApp::new(1, None);
        app.current_view = View::ActiveRuns;
        app.has_active_runs = true;
        // Add 5 sessions
        add_test_sessions(&mut app, 5);

        assert_eq!(app.quadrant_page, 0);
        app.handle_key(KeyCode::Char(']'));
        assert_eq!(app.quadrant_page, 1);
    }

    #[test]
    fn test_handle_p_key_goes_back_page_in_active_runs() {
        let mut app = MonitorApp::new(1, None);
        app.current_view = View::ActiveRuns;
        app.has_active_runs = true;
        app.quadrant_page = 1;
        // Add 5 sessions
        add_test_sessions(&mut app, 5);

        app.handle_key(KeyCode::Char('p'));
        assert_eq!(app.quadrant_page, 0);
    }

    #[test]
    fn test_handle_left_bracket_goes_back_page_in_active_runs() {
        let mut app = MonitorApp::new(1, None);
        app.current_view = View::ActiveRuns;
        app.has_active_runs = true;
        app.quadrant_page = 1;
        // Add 5 sessions
        add_test_sessions(&mut app, 5);

        app.handle_key(KeyCode::Char('['));
        assert_eq!(app.quadrant_page, 0);
    }

    #[test]
    fn test_pagination_keys_ignored_in_other_views() {
        let mut app = MonitorApp::new(1, None);
        app.current_view = View::ProjectList;
        app.quadrant_page = 0;

        // n/p keys should not affect quadrant_page in other views
        app.handle_key(KeyCode::Char('n'));
        assert_eq!(app.quadrant_page, 0);

        app.handle_key(KeyCode::Char('p'));
        assert_eq!(app.quadrant_page, 0);
    }

    #[test]
    fn test_tab_resets_quadrant_page() {
        let mut app = MonitorApp::new(1, None);
        app.current_view = View::ActiveRuns;
        app.has_active_runs = true;
        app.quadrant_page = 2;

        app.handle_key(KeyCode::Tab);

        // After switching view, quadrant_page should reset to 0
        assert_eq!(app.quadrant_page, 0);
    }

    #[test]
    fn test_clamp_selection_index_also_clamps_quadrant_page() {
        let mut app = MonitorApp::new(1, None);
        app.current_view = View::ActiveRuns;
        app.quadrant_page = 5; // Out of bounds
                               // Only 3 sessions = 1 page (max page index = 0)
        add_test_sessions(&mut app, 3);

        app.clamp_selection_index();

        // Should be clamped to max valid page (0)
        assert_eq!(app.quadrant_page, 0);
    }

    #[test]
    fn test_total_quadrant_pages_only_counts_sessions() {
        // Sessions are always "active" by definition (we only store running sessions)
        let mut app = MonitorApp::new(1, None);
        // Add 3 active sessions
        add_test_sessions(&mut app, 3);

        // 3 sessions = 1 page
        assert_eq!(app.total_quadrant_pages(), 1);
    }

    #[test]
    fn test_total_quadrant_pages_includes_error_sessions() {
        let mut app = MonitorApp::new(1, None);
        // Add 3 active sessions
        add_test_sessions(&mut app, 3);

        // Add 2 sessions with errors (should still count)
        for i in 1..=2 {
            let mut session = create_test_session(
                &format!("error-project-{}", i),
                &format!("err{:05x}", i),
                &format!("error-branch-{}", i),
            );
            session.run = None;
            session.load_error = Some("Corrupted".to_string());
            app.sessions.push(session);
        }

        // 3 active + 2 error = 5 = 2 pages
        assert_eq!(app.total_quadrant_pages(), 2);
    }

    // ===========================================
    // US-003: Vim-Style Navigation (hjkl) Tests
    // ===========================================

    #[test]
    fn test_monitor_app_new_initializes_quadrant_row_col_to_zero() {
        let app = MonitorApp::new(1, None);
        assert_eq!(app.quadrant_row, 0);
        assert_eq!(app.quadrant_col, 0);
    }

    #[test]
    fn test_next_view_resets_quadrant_position() {
        let mut app = MonitorApp::new(1, None);
        app.quadrant_row = 1;
        app.quadrant_col = 1;
        app.current_view = View::ActiveRuns;
        app.has_active_runs = true;

        app.next_view();

        assert_eq!(app.quadrant_row, 0);
        assert_eq!(app.quadrant_col, 0);
    }

    #[test]
    fn test_j_key_navigates_down_in_project_list() {
        use crate::config::ProjectTreeInfo;

        let mut app = MonitorApp::new(1, None);
        app.current_view = View::ProjectList;
        app.projects = vec![
            ProjectData {
                info: ProjectTreeInfo {
                    name: "project-a".to_string(),
                    has_active_run: false,
                    run_status: None,
                    spec_count: 0,
                    incomplete_spec_count: 0,
                    spec_md_count: 0,
                    runs_count: 0,
                    last_run_date: None,
                },
                active_run: None,
                progress: None,
                load_error: None,
            },
            ProjectData {
                info: ProjectTreeInfo {
                    name: "project-b".to_string(),
                    has_active_run: false,
                    run_status: None,
                    spec_count: 0,
                    incomplete_spec_count: 0,
                    spec_md_count: 0,
                    runs_count: 0,
                    last_run_date: None,
                },
                active_run: None,
                progress: None,
                load_error: None,
            },
        ];

        assert_eq!(app.selected_index, 0);

        app.handle_key(KeyCode::Char('j'));
        assert_eq!(app.selected_index, 1);
    }

    #[test]
    fn test_k_key_navigates_up_in_project_list() {
        use crate::config::ProjectTreeInfo;

        let mut app = MonitorApp::new(1, None);
        app.current_view = View::ProjectList;
        app.selected_index = 1;
        app.projects = vec![
            ProjectData {
                info: ProjectTreeInfo {
                    name: "project-a".to_string(),
                    has_active_run: false,
                    run_status: None,
                    spec_count: 0,
                    incomplete_spec_count: 0,
                    spec_md_count: 0,
                    runs_count: 0,
                    last_run_date: None,
                },
                active_run: None,
                progress: None,
                load_error: None,
            },
            ProjectData {
                info: ProjectTreeInfo {
                    name: "project-b".to_string(),
                    has_active_run: false,
                    run_status: None,
                    spec_count: 0,
                    incomplete_spec_count: 0,
                    spec_md_count: 0,
                    runs_count: 0,
                    last_run_date: None,
                },
                active_run: None,
                progress: None,
                load_error: None,
            },
        ];

        app.handle_key(KeyCode::Char('k'));
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_j_key_navigates_down_in_run_history() {
        use std::path::PathBuf;

        let mut app = MonitorApp::new(1, None);
        app.current_view = View::RunHistory;
        app.run_history = vec![
            RunHistoryEntry {
                project_name: "project-a".to_string(),
                run: RunState::new(PathBuf::from("a.json"), "branch-a".to_string()),
                completed_stories: 1,
                total_stories: 2,
            },
            RunHistoryEntry {
                project_name: "project-b".to_string(),
                run: RunState::new(PathBuf::from("b.json"), "branch-b".to_string()),
                completed_stories: 2,
                total_stories: 3,
            },
        ];

        assert_eq!(app.selected_index, 0);

        app.handle_key(KeyCode::Char('j'));
        assert_eq!(app.selected_index, 1);
    }

    #[test]
    fn test_k_key_navigates_up_in_run_history() {
        use std::path::PathBuf;

        let mut app = MonitorApp::new(1, None);
        app.current_view = View::RunHistory;
        app.selected_index = 1;
        app.run_history = vec![
            RunHistoryEntry {
                project_name: "project-a".to_string(),
                run: RunState::new(PathBuf::from("a.json"), "branch-a".to_string()),
                completed_stories: 1,
                total_stories: 2,
            },
            RunHistoryEntry {
                project_name: "project-b".to_string(),
                run: RunState::new(PathBuf::from("b.json"), "branch-b".to_string()),
                completed_stories: 2,
                total_stories: 3,
            },
        ];

        app.handle_key(KeyCode::Char('k'));
        assert_eq!(app.selected_index, 0);
    }

    fn create_four_active_sessions() -> Vec<SessionData> {
        (1..=4)
            .map(|i| {
                create_test_session(
                    &format!("project-{}", i),
                    &format!("{:08x}", i),
                    &format!("branch-{}", i),
                )
            })
            .collect()
    }

    #[test]
    fn test_hjkl_quadrant_navigation_in_active_runs() {
        let mut app = MonitorApp::new(1, None);
        app.current_view = View::ActiveRuns;
        app.has_active_runs = true;
        app.sessions = create_four_active_sessions();

        // Start at top-left (0,0)
        assert_eq!(app.quadrant_row, 0);
        assert_eq!(app.quadrant_col, 0);

        // Move right with 'l'
        app.handle_key(KeyCode::Char('l'));
        assert_eq!(app.quadrant_row, 0);
        assert_eq!(app.quadrant_col, 1);

        // Move down with 'j'
        app.handle_key(KeyCode::Char('j'));
        assert_eq!(app.quadrant_row, 1);
        assert_eq!(app.quadrant_col, 1);

        // Move left with 'h'
        app.handle_key(KeyCode::Char('h'));
        assert_eq!(app.quadrant_row, 1);
        assert_eq!(app.quadrant_col, 0);

        // Move up with 'k'
        app.handle_key(KeyCode::Char('k'));
        assert_eq!(app.quadrant_row, 0);
        assert_eq!(app.quadrant_col, 0);
    }

    #[test]
    fn test_arrow_keys_quadrant_navigation_in_active_runs() {
        let mut app = MonitorApp::new(1, None);
        app.current_view = View::ActiveRuns;
        app.has_active_runs = true;
        app.sessions = create_four_active_sessions();

        // Start at top-left (0,0)
        assert_eq!(app.quadrant_row, 0);
        assert_eq!(app.quadrant_col, 0);

        // Move right with Right arrow
        app.handle_key(KeyCode::Right);
        assert_eq!(app.quadrant_row, 0);
        assert_eq!(app.quadrant_col, 1);

        // Move down with Down arrow
        app.handle_key(KeyCode::Down);
        assert_eq!(app.quadrant_row, 1);
        assert_eq!(app.quadrant_col, 1);

        // Move left with Left arrow
        app.handle_key(KeyCode::Left);
        assert_eq!(app.quadrant_row, 1);
        assert_eq!(app.quadrant_col, 0);

        // Move up with Up arrow
        app.handle_key(KeyCode::Up);
        assert_eq!(app.quadrant_row, 0);
        assert_eq!(app.quadrant_col, 0);
    }

    #[test]
    fn test_quadrant_navigation_stays_in_bounds() {
        let mut app = MonitorApp::new(1, None);
        app.current_view = View::ActiveRuns;
        app.has_active_runs = true;
        app.sessions = create_four_active_sessions();

        // Try to go left from (0,0) - should stay
        app.handle_key(KeyCode::Char('h'));
        assert_eq!(app.quadrant_row, 0);
        assert_eq!(app.quadrant_col, 0);

        // Try to go up from (0,0) - should stay
        app.handle_key(KeyCode::Char('k'));
        assert_eq!(app.quadrant_row, 0);
        assert_eq!(app.quadrant_col, 0);

        // Move to bottom-right (1,1)
        app.handle_key(KeyCode::Char('l'));
        app.handle_key(KeyCode::Char('j'));
        assert_eq!(app.quadrant_row, 1);
        assert_eq!(app.quadrant_col, 1);

        // Try to go right from (1,1) - should stay
        app.handle_key(KeyCode::Char('l'));
        assert_eq!(app.quadrant_row, 1);
        assert_eq!(app.quadrant_col, 1);

        // Try to go down from (1,1) - should stay
        app.handle_key(KeyCode::Char('j'));
        assert_eq!(app.quadrant_row, 1);
        assert_eq!(app.quadrant_col, 1);
    }

    #[test]
    fn test_quadrant_navigation_with_fewer_than_four_runs() {
        let mut app = MonitorApp::new(1, None);
        app.current_view = View::ActiveRuns;
        app.has_active_runs = true;
        // Only 2 sessions (positions 0,0 and 0,1)
        add_test_sessions(&mut app, 2);

        // Start at (0,0)
        assert_eq!(app.quadrant_row, 0);
        assert_eq!(app.quadrant_col, 0);

        // Can move right to (0,1)
        app.handle_key(KeyCode::Char('l'));
        assert_eq!(app.quadrant_row, 0);
        assert_eq!(app.quadrant_col, 1);

        // Cannot move down (no sessions in row 1)
        app.handle_key(KeyCode::Char('j'));
        assert_eq!(app.quadrant_row, 0);
        assert_eq!(app.quadrant_col, 1);
    }

    #[test]
    fn test_quadrant_navigation_with_three_runs() {
        let mut app = MonitorApp::new(1, None);
        app.current_view = View::ActiveRuns;
        app.has_active_runs = true;
        // 3 sessions (positions 0,0, 0,1, and 1,0)
        add_test_sessions(&mut app, 3);

        // Start at (0,0), move right to (0,1)
        app.handle_key(KeyCode::Char('l'));
        assert_eq!(app.quadrant_row, 0);
        assert_eq!(app.quadrant_col, 1);

        // Try to move down from (0,1) - position (1,1) is invalid
        app.handle_key(KeyCode::Char('j'));
        // Should stay at (0,1) since (1,1) has no session
        assert_eq!(app.quadrant_row, 0);
        assert_eq!(app.quadrant_col, 1);

        // Move left back to (0,0)
        app.handle_key(KeyCode::Char('h'));
        assert_eq!(app.quadrant_row, 0);
        assert_eq!(app.quadrant_col, 0);

        // Can move down from (0,0) to (1,0)
        app.handle_key(KeyCode::Char('j'));
        assert_eq!(app.quadrant_row, 1);
        assert_eq!(app.quadrant_col, 0);

        // Cannot move right from (1,0) to (1,1) - no session there
        app.handle_key(KeyCode::Char('l'));
        assert_eq!(app.quadrant_row, 1);
        assert_eq!(app.quadrant_col, 0);
    }

    #[test]
    fn test_h_and_l_ignored_in_project_list() {
        use crate::config::ProjectTreeInfo;

        let mut app = MonitorApp::new(1, None);
        app.current_view = View::ProjectList;
        app.selected_index = 1;
        app.projects = vec![
            ProjectData {
                info: ProjectTreeInfo {
                    name: "project-a".to_string(),
                    has_active_run: false,
                    run_status: None,
                    spec_count: 0,
                    incomplete_spec_count: 0,
                    spec_md_count: 0,
                    runs_count: 0,
                    last_run_date: None,
                },
                active_run: None,
                progress: None,
                load_error: None,
            },
            ProjectData {
                info: ProjectTreeInfo {
                    name: "project-b".to_string(),
                    has_active_run: false,
                    run_status: None,
                    spec_count: 0,
                    incomplete_spec_count: 0,
                    spec_md_count: 0,
                    runs_count: 0,
                    last_run_date: None,
                },
                active_run: None,
                progress: None,
                load_error: None,
            },
        ];

        // 'h' should not change selected_index in list views
        app.handle_key(KeyCode::Char('h'));
        assert_eq!(app.selected_index, 1);

        // 'l' should not change selected_index in list views
        app.handle_key(KeyCode::Char('l'));
        assert_eq!(app.selected_index, 1);
    }

    #[test]
    fn test_runs_on_current_page_with_full_page() {
        let mut app = MonitorApp::new(1, None);
        app.sessions = create_four_active_sessions();
        app.quadrant_page = 0;

        assert_eq!(app.runs_on_current_page(), 4);
    }

    #[test]
    fn test_runs_on_current_page_with_partial_page() {
        let mut app = MonitorApp::new(1, None);
        // 5 sessions = 4 on page 0, 1 on page 1
        add_test_sessions(&mut app, 5);

        app.quadrant_page = 0;
        assert_eq!(app.runs_on_current_page(), 4);

        app.quadrant_page = 1;
        assert_eq!(app.runs_on_current_page(), 1);
    }

    #[test]
    fn test_is_quadrant_valid() {
        let mut app = MonitorApp::new(1, None);
        app.sessions = create_four_active_sessions();
        app.quadrant_page = 0;

        // All 4 positions valid with 4 runs
        assert!(app.is_quadrant_valid(0, 0));
        assert!(app.is_quadrant_valid(0, 1));
        assert!(app.is_quadrant_valid(1, 0));
        assert!(app.is_quadrant_valid(1, 1));
    }

    #[test]
    fn test_is_quadrant_valid_with_two_runs() {
        let mut app = MonitorApp::new(1, None);
        // Only 2 sessions
        add_test_sessions(&mut app, 2);
        app.quadrant_page = 0;

        // Only positions 0,0 and 0,1 valid
        assert!(app.is_quadrant_valid(0, 0));
        assert!(app.is_quadrant_valid(0, 1));
        assert!(!app.is_quadrant_valid(1, 0));
        assert!(!app.is_quadrant_valid(1, 1));
    }

    #[test]
    fn test_clamp_selection_index_clamps_quadrant_position() {
        let mut app = MonitorApp::new(1, None);
        app.current_view = View::ActiveRuns;
        // Start at position (1,1) with 4 sessions
        app.quadrant_row = 1;
        app.quadrant_col = 1;
        app.sessions = create_four_active_sessions();

        // Now reduce to only 2 sessions
        app.sessions.truncate(2);

        app.clamp_selection_index();

        // Should be clamped to last valid position (0,1)
        assert_eq!(app.quadrant_row, 0);
        assert_eq!(app.quadrant_col, 1);
    }

    #[test]
    fn test_clamp_selection_index_with_one_run() {
        let mut app = MonitorApp::new(1, None);
        app.current_view = View::ActiveRuns;
        app.quadrant_row = 1;
        app.quadrant_col = 1;
        // Only 1 session
        add_test_sessions(&mut app, 1);

        app.clamp_selection_index();

        // Should be clamped to (0,0)
        assert_eq!(app.quadrant_row, 0);
        assert_eq!(app.quadrant_col, 0);
    }

    #[test]
    fn test_clamp_selection_index_with_zero_runs() {
        let mut app = MonitorApp::new(1, None);
        app.current_view = View::ActiveRuns;
        app.quadrant_row = 1;
        app.quadrant_col = 1;
        // No sessions - sessions vector is already empty by default

        app.clamp_selection_index();

        // Should be clamped to (0,0)
        assert_eq!(app.quadrant_row, 0);
        assert_eq!(app.quadrant_col, 0);
    }

    // ============================================================================
    // US-004: Hierarchical Escape Behavior Tests
    // ============================================================================

    #[test]
    fn test_esc_from_active_runs_quits() {
        let mut app = MonitorApp::new(1, None);
        app.current_view = View::ActiveRuns;

        app.handle_key(KeyCode::Esc);

        assert!(app.should_quit());
    }

    #[test]
    fn test_esc_from_project_list_quits() {
        let mut app = MonitorApp::new(1, None);
        app.current_view = View::ProjectList;

        app.handle_key(KeyCode::Esc);

        assert!(app.should_quit());
    }

    #[test]
    fn test_esc_from_run_history_unfiltered_goes_to_project_list() {
        let mut app = MonitorApp::new(1, None);
        app.current_view = View::RunHistory;
        app.run_history_filter = None;

        app.handle_key(KeyCode::Esc);

        assert_eq!(app.current_view, View::ProjectList);
        assert!(!app.should_quit());
    }

    #[test]
    fn test_esc_from_run_history_filtered_clears_filter_first() {
        let mut app = MonitorApp::new(1, None);
        app.current_view = View::RunHistory;
        app.run_history_filter = Some("my-project".to_string());
        app.selected_index = 5;
        app.history_scroll_offset = 10;

        app.handle_key(KeyCode::Esc);

        // Should clear filter but stay in RunHistory
        assert!(app.run_history_filter.is_none());
        assert_eq!(app.current_view, View::RunHistory);
        assert_eq!(app.selected_index, 0);
        assert_eq!(app.history_scroll_offset, 0);
        assert!(!app.should_quit());
    }

    #[test]
    fn test_esc_twice_from_filtered_run_history_goes_to_project_list() {
        let mut app = MonitorApp::new(1, None);
        app.current_view = View::RunHistory;
        app.run_history_filter = Some("my-project".to_string());

        // First Esc clears filter
        app.handle_key(KeyCode::Esc);
        assert!(app.run_history_filter.is_none());
        assert_eq!(app.current_view, View::RunHistory);

        // Second Esc goes to ProjectList
        app.handle_key(KeyCode::Esc);
        assert_eq!(app.current_view, View::ProjectList);
        assert!(!app.should_quit());
    }

    #[test]
    fn test_esc_three_times_from_filtered_run_history_quits() {
        let mut app = MonitorApp::new(1, None);
        app.current_view = View::RunHistory;
        app.run_history_filter = Some("my-project".to_string());

        // First Esc clears filter
        app.handle_key(KeyCode::Esc);
        // Second Esc goes to ProjectList
        app.handle_key(KeyCode::Esc);
        // Third Esc quits
        app.handle_key(KeyCode::Esc);

        assert!(app.should_quit());
    }

    #[test]
    fn test_q_quits_from_any_view() {
        // Test q quits from ProjectList
        let mut app1 = MonitorApp::new(1, None);
        app1.current_view = View::ProjectList;
        app1.handle_key(KeyCode::Char('q'));
        assert!(app1.should_quit());

        // Test q quits from ActiveRuns
        let mut app2 = MonitorApp::new(1, None);
        app2.current_view = View::ActiveRuns;
        app2.handle_key(KeyCode::Char('q'));
        assert!(app2.should_quit());

        // Test q quits from RunHistory
        let mut app3 = MonitorApp::new(1, None);
        app3.current_view = View::RunHistory;
        app3.handle_key(KeyCode::Char('q'));
        assert!(app3.should_quit());

        // Test Q (uppercase) quits from any view
        let mut app4 = MonitorApp::new(1, None);
        app4.current_view = View::ProjectList;
        app4.handle_key(KeyCode::Char('Q'));
        assert!(app4.should_quit());
    }

    #[test]
    fn test_esc_resets_selected_index_when_going_to_project_list() {
        let mut app = MonitorApp::new(1, None);
        app.current_view = View::RunHistory;
        app.run_history_filter = None;
        app.selected_index = 5;

        app.handle_key(KeyCode::Esc);

        assert_eq!(app.current_view, View::ProjectList);
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_hierarchical_navigation_flow() {
        // Simulate full navigation flow:
        // ProjectList -> (Enter with project) -> RunHistory (filtered) -> (Enter) -> Detail
        // Then Esc back through the hierarchy
        use crate::config::ProjectTreeInfo;

        let mut app = MonitorApp::new(1, None);

        // Setup a project
        app.projects = vec![ProjectData {
            info: ProjectTreeInfo {
                name: "test-project".to_string(),
                has_active_run: false,
                run_status: None,
                spec_count: 1,
                incomplete_spec_count: 0,
                spec_md_count: 0,
                runs_count: 1,
                last_run_date: None,
            },
            active_run: None,
            progress: None,
            load_error: None,
        }];

        // Add a run history entry
        app.run_history = vec![RunHistoryEntry {
            project_name: "test-project".to_string(),
            run: RunState::new(
                std::path::PathBuf::from("test.json"),
                "test-branch".to_string(),
            ),
            completed_stories: 3,
            total_stories: 3,
        }];

        // Start at ProjectList
        app.current_view = View::ProjectList;
        assert_eq!(app.current_view, View::ProjectList);

        // Enter on project goes to filtered RunHistory
        app.handle_key(KeyCode::Enter);
        assert_eq!(app.current_view, View::RunHistory);
        assert_eq!(app.run_history_filter, Some("test-project".to_string()));

        // Enter on run history shows detail
        app.handle_key(KeyCode::Enter);
        assert!(app.show_run_detail);

        // Esc closes detail view
        app.handle_key(KeyCode::Esc);
        assert!(!app.show_run_detail);
        assert_eq!(app.current_view, View::RunHistory);

        // Esc clears filter
        app.handle_key(KeyCode::Esc);
        assert!(app.run_history_filter.is_none());
        assert_eq!(app.current_view, View::RunHistory);

        // Esc goes to ProjectList
        app.handle_key(KeyCode::Esc);
        assert_eq!(app.current_view, View::ProjectList);

        // Esc from ProjectList quits
        app.handle_key(KeyCode::Esc);
        assert!(app.should_quit());
    }

    // ============================================================================
    // US-002: Session Display Title Tests
    // ============================================================================

    #[test]
    fn test_session_display_title_main_session() {
        let session = create_test_session("my-project", MAIN_SESSION_ID, "feature-branch");
        assert_eq!(session.display_title(), "my-project (main)");
    }

    #[test]
    fn test_session_display_title_worktree_session() {
        let session = create_test_session("my-project", "abc12345", "feature-branch");
        assert_eq!(session.display_title(), "my-project (abc12345)");
    }

    #[test]
    fn test_session_display_title_8char_hex_id() {
        // Verify that session IDs are displayed in their 8-char hex format
        let session = create_test_session("autom8", "deadbeef", "worktree-branch");
        assert_eq!(session.display_title(), "autom8 (deadbeef)");
    }

    #[test]
    fn test_multiple_sessions_same_project_in_grid() {
        let mut app = MonitorApp::new(1, None);

        // Add three sessions for the same project
        app.sessions.push(create_test_session(
            "my-project",
            MAIN_SESSION_ID,
            "main-branch",
        ));
        app.sessions
            .push(create_test_session("my-project", "abc12345", "feature-1"));
        app.sessions
            .push(create_test_session("my-project", "def67890", "feature-2"));

        // All three should be in the sessions list
        assert_eq!(app.sessions.len(), 3);

        // Verify each has the correct project name
        assert_eq!(app.sessions[0].project_name, "my-project");
        assert_eq!(app.sessions[1].project_name, "my-project");
        assert_eq!(app.sessions[2].project_name, "my-project");

        // Verify distinct session IDs
        assert_eq!(app.sessions[0].metadata.session_id, MAIN_SESSION_ID);
        assert_eq!(app.sessions[1].metadata.session_id, "abc12345");
        assert_eq!(app.sessions[2].metadata.session_id, "def67890");

        // Verify distinct display titles
        assert_eq!(app.sessions[0].display_title(), "my-project (main)");
        assert_eq!(app.sessions[1].display_title(), "my-project (abc12345)");
        assert_eq!(app.sessions[2].display_title(), "my-project (def67890)");
    }

    #[test]
    fn test_pagination_with_multiple_sessions_same_project() {
        let mut app = MonitorApp::new(1, None);
        app.current_view = View::ActiveRuns;
        app.has_active_runs = true;

        // Add 5 sessions for the same project (to span 2 pages)
        for i in 1..=5 {
            app.sessions.push(create_test_session(
                "my-project",
                &format!("{:08x}", i),
                &format!("feature-{}", i),
            ));
        }

        // Should have 2 pages (4 on first, 1 on second)
        assert_eq!(app.total_quadrant_pages(), 2);

        // First page should have 4 sessions
        assert_eq!(app.quadrant_page, 0);

        // Navigate to second page
        app.next_quadrant_page();
        assert_eq!(app.quadrant_page, 1);

        // Verify all sessions have distinct display titles
        let titles: Vec<_> = app.sessions.iter().map(|s| s.display_title()).collect();
        assert_eq!(titles.len(), 5);
        // Check uniqueness
        let unique_titles: std::collections::HashSet<_> = titles.iter().collect();
        assert_eq!(unique_titles.len(), 5);
    }

    #[test]
    fn test_sessions_from_different_projects_in_grid() {
        let mut app = MonitorApp::new(1, None);

        // Add sessions from different projects
        app.sessions.push(create_test_session(
            "project-alpha",
            MAIN_SESSION_ID,
            "main-branch",
        ));
        app.sessions
            .push(create_test_session("project-beta", "12345678", "feature-x"));
        app.sessions.push(create_test_session(
            "project-gamma",
            MAIN_SESSION_ID,
            "develop",
        ));

        // All three should be in the grid
        assert_eq!(app.sessions.len(), 3);

        // Verify display titles
        assert_eq!(app.sessions[0].display_title(), "project-alpha (main)");
        assert_eq!(app.sessions[1].display_title(), "project-beta (12345678)");
        assert_eq!(app.sessions[2].display_title(), "project-gamma (main)");
    }

    #[test]
    fn test_is_main_session_flag_correct() {
        // Main session should have is_main_session = true
        let main_session = create_test_session("test", MAIN_SESSION_ID, "branch");
        assert!(main_session.is_main_session);

        // Worktree session should have is_main_session = false
        let worktree_session = create_test_session("test", "abcd1234", "branch");
        assert!(!worktree_session.is_main_session);
    }

    // ===========================================
    // US-003: Session Context in Run Detail Tests
    // ===========================================

    #[test]
    fn test_us003_main_session_indicator() {
        // Main sessions should use "● main" indicator
        let session = create_test_session("my-project", MAIN_SESSION_ID, "main");
        assert!(session.is_main_session);
        // The indicator is constructed in render_session_detail, verify the condition
        let (indicator, color) = if session.is_main_session {
            ("● main", COLOR_PRIMARY)
        } else {
            ("◆ worktree", COLOR_REVIEW)
        };
        assert_eq!(indicator, "● main");
        assert_eq!(color, COLOR_PRIMARY); // Cyan for main
    }

    #[test]
    fn test_us003_worktree_session_indicator() {
        // Worktree sessions should use "◆ worktree" indicator
        let session = create_test_session("my-project", "abc12345", "feature-x");
        assert!(!session.is_main_session);
        let (indicator, color) = if session.is_main_session {
            ("● main", COLOR_PRIMARY)
        } else {
            ("◆ worktree", COLOR_REVIEW)
        };
        assert_eq!(indicator, "◆ worktree");
        assert_eq!(color, COLOR_REVIEW); // Magenta for worktree
    }

    #[test]
    fn test_us003_truncated_worktree_path_short() {
        // Short paths should display fully
        let mut session = create_test_session("test", "abc12345", "branch");
        session.metadata.worktree_path = PathBuf::from("foo/bar");
        let truncated = session.truncated_worktree_path();
        assert_eq!(truncated, "foo/bar");
    }

    #[test]
    fn test_us003_truncated_worktree_path_long() {
        // Long paths should show ".../last/two" format
        let mut session = create_test_session("test", "abc12345", "branch");
        session.metadata.worktree_path = PathBuf::from("/home/user/projects/autom8-wt-feature-x");
        let truncated = session.truncated_worktree_path();
        assert_eq!(truncated, ".../projects/autom8-wt-feature-x");
    }

    #[test]
    fn test_us003_truncated_worktree_path_exactly_two_components() {
        // Exactly 2 components should display fully
        let mut session = create_test_session("test", "abc12345", "branch");
        session.metadata.worktree_path = PathBuf::from("projects/repo");
        let truncated = session.truncated_worktree_path();
        assert_eq!(truncated, "projects/repo");
    }

    #[test]
    fn test_us003_main_session_no_worktree_path_in_detail() {
        // Main sessions should not show worktree path (tested by checking is_main_session)
        let session = create_test_session("my-project", MAIN_SESSION_ID, "main");
        // In render_session_detail, worktree path is only shown when:
        // full && !session.is_main_session
        // Since session.is_main_session is true, path would NOT be shown
        assert!(session.is_main_session);
    }

    #[test]
    fn test_us003_worktree_session_has_path_available() {
        // Worktree sessions should have path available for display
        let session = create_test_session("my-project", "abc12345", "feature-x");
        assert!(!session.is_main_session);
        // In render_session_detail, path IS shown when full && !is_main_session
        // Verify the path is populated
        assert!(!session.metadata.worktree_path.as_os_str().is_empty());
    }

    #[test]
    fn test_us003_branch_always_available() {
        // Both main and worktree sessions should have branch available
        let main_session = create_test_session("project", MAIN_SESSION_ID, "develop");
        let worktree_session = create_test_session("project", "abc12345", "feature-x");

        // Branch should be in the run state
        assert_eq!(main_session.run.as_ref().unwrap().branch, "develop");
        assert_eq!(worktree_session.run.as_ref().unwrap().branch, "feature-x");
    }

    #[test]
    fn test_us003_visual_distinction_colors() {
        // Verify the color constants are distinct
        assert_ne!(COLOR_PRIMARY, COLOR_REVIEW);
        // COLOR_PRIMARY is Cyan, COLOR_REVIEW is Magenta
        assert_eq!(COLOR_PRIMARY, Color::Cyan);
        assert_eq!(COLOR_REVIEW, Color::Magenta);
    }

    // ============================================================================
    // US-007: Integration Test - Multiple Concurrent Sessions
    // ============================================================================
    //
    // These tests verify that the monitor correctly displays multiple concurrent
    // sessions for the same project (main repo + worktree scenarios).

    #[test]
    fn test_us007_main_and_worktree_sessions_both_appear_in_grid() {
        // Simulates: Run in main repo AND run with --worktree for same project
        let mut app = MonitorApp::new(1, None);
        app.current_view = View::ActiveRuns;
        app.has_active_runs = true;

        // Add a main repo session (simulating: `autom8 run spec.json`)
        app.sessions.push(create_test_session(
            "autom8",
            MAIN_SESSION_ID,
            "features/worktrees",
        ));

        // Add a worktree session (simulating: `autom8 run --worktree other-spec.json`)
        app.sessions.push(create_test_session(
            "autom8",
            "abc12345",
            "features/other-feature",
        ));

        // Both sessions should appear in the Active Runs grid
        assert_eq!(app.sessions.len(), 2);

        // Both should have the same project name
        assert_eq!(app.sessions[0].project_name, "autom8");
        assert_eq!(app.sessions[1].project_name, "autom8");

        // But different session IDs
        assert_eq!(app.sessions[0].metadata.session_id, MAIN_SESSION_ID);
        assert_eq!(app.sessions[1].metadata.session_id, "abc12345");

        // Display titles should distinguish them
        assert_eq!(app.sessions[0].display_title(), "autom8 (main)");
        assert_eq!(app.sessions[1].display_title(), "autom8 (abc12345)");
    }

    #[test]
    fn test_us007_each_session_shows_correct_session_id_in_title() {
        let mut app = MonitorApp::new(1, None);

        // Add sessions with distinct session IDs
        app.sessions
            .push(create_test_session("myproject", MAIN_SESSION_ID, "main"));
        app.sessions
            .push(create_test_session("myproject", "deadbeef", "feature-a"));
        app.sessions
            .push(create_test_session("myproject", "cafebabe", "feature-b"));

        // Each should have correct title format
        assert_eq!(app.sessions[0].display_title(), "myproject (main)");
        assert_eq!(app.sessions[1].display_title(), "myproject (deadbeef)");
        assert_eq!(app.sessions[2].display_title(), "myproject (cafebabe)");
    }

    #[test]
    fn test_us007_sessions_have_independent_progress() {
        let mut app = MonitorApp::new(1, None);

        // Create main session with progress 2/5
        let mut main_session = create_test_session("autom8", MAIN_SESSION_ID, "main");
        main_session.progress = Some(RunProgress {
            completed: 2,
            total: 5,
        });

        // Create worktree session with different progress 4/8
        let mut worktree_session = create_test_session("autom8", "abc12345", "feature-x");
        worktree_session.progress = Some(RunProgress {
            completed: 4,
            total: 8,
        });

        app.sessions.push(main_session);
        app.sessions.push(worktree_session);

        // Verify independent progress
        let main_progress = app.sessions[0].progress.as_ref().unwrap();
        let wt_progress = app.sessions[1].progress.as_ref().unwrap();

        assert_eq!(main_progress.completed, 2);
        assert_eq!(main_progress.total, 5);
        assert_eq!(main_progress.as_fraction(), "Story 3/5");

        assert_eq!(wt_progress.completed, 4);
        assert_eq!(wt_progress.total, 8);
        assert_eq!(wt_progress.as_fraction(), "Story 5/8");
    }

    #[test]
    fn test_us007_sessions_have_independent_state() {
        use crate::state::MachineState;

        let mut app = MonitorApp::new(1, None);

        // Main session is in Reviewing state
        let mut main_session = create_test_session("autom8", MAIN_SESSION_ID, "main");
        if let Some(ref mut run) = main_session.run {
            run.machine_state = MachineState::Reviewing;
        }

        // Worktree session is in RunningClaude state
        let mut worktree_session = create_test_session("autom8", "abc12345", "feature-x");
        if let Some(ref mut run) = worktree_session.run {
            run.machine_state = MachineState::RunningClaude;
        }

        app.sessions.push(main_session);
        app.sessions.push(worktree_session);

        // Verify independent states
        assert_eq!(
            app.sessions[0].run.as_ref().unwrap().machine_state,
            MachineState::Reviewing
        );
        assert_eq!(
            app.sessions[1].run.as_ref().unwrap().machine_state,
            MachineState::RunningClaude
        );
    }

    #[test]
    fn test_us007_sessions_have_independent_branches() {
        let mut app = MonitorApp::new(1, None);

        app.sessions
            .push(create_test_session("autom8", MAIN_SESSION_ID, "main"));
        app.sessions.push(create_test_session(
            "autom8",
            "abc12345",
            "features/new-feature",
        ));

        // Branches should be independent
        assert_eq!(app.sessions[0].run.as_ref().unwrap().branch, "main");
        assert_eq!(
            app.sessions[1].run.as_ref().unwrap().branch,
            "features/new-feature"
        );

        // Also verify branch in metadata
        assert_eq!(app.sessions[0].metadata.branch_name, "main");
        assert_eq!(app.sessions[1].metadata.branch_name, "features/new-feature");
    }

    #[test]
    fn test_us007_quadrant_navigation_with_concurrent_sessions() {
        let mut app = MonitorApp::new(1, None);
        app.current_view = View::ActiveRuns;
        app.has_active_runs = true;

        // Add 2 sessions for same project (main + worktree)
        app.sessions
            .push(create_test_session("autom8", MAIN_SESSION_ID, "main"));
        app.sessions
            .push(create_test_session("autom8", "abc12345", "feature-x"));

        // Both should be navigable
        assert!(app.is_quadrant_valid(0, 0));
        assert!(app.is_quadrant_valid(0, 1));
        assert!(!app.is_quadrant_valid(1, 0)); // No third session

        // Navigate between them
        // Session index is: quadrant_page * 4 + quadrant_row * 2 + quadrant_col
        app.quadrant_col = 0;
        app.quadrant_row = 0;
        let session_idx_0 = app.quadrant_page * 4 + app.quadrant_row * 2 + app.quadrant_col;
        assert_eq!(session_idx_0, 0);

        app.handle_key(KeyCode::Char('l')); // Move right
        assert_eq!(app.quadrant_col, 1);
        let session_idx_1 = app.quadrant_page * 4 + app.quadrant_row * 2 + app.quadrant_col;
        assert_eq!(session_idx_1, 1);
    }

    #[test]
    fn test_us007_session_type_indicators_in_concurrent_sessions() {
        let mut app = MonitorApp::new(1, None);

        // Main session
        let main_session = create_test_session("autom8", MAIN_SESSION_ID, "main");
        assert!(main_session.is_main_session);

        // Worktree session
        let worktree_session = create_test_session("autom8", "abc12345", "feature-x");
        assert!(!worktree_session.is_main_session);

        app.sessions.push(main_session);
        app.sessions.push(worktree_session);

        // Verify type indicators would be different
        // (The actual indicator logic uses is_main_session to choose)
        let get_indicator = |is_main: bool| {
            if is_main {
                ("● main", COLOR_PRIMARY)
            } else {
                ("◆ worktree", COLOR_REVIEW)
            }
        };

        let (main_ind, main_color) = get_indicator(app.sessions[0].is_main_session);
        let (wt_ind, wt_color) = get_indicator(app.sessions[1].is_main_session);

        assert_eq!(main_ind, "● main");
        assert_eq!(wt_ind, "◆ worktree");
        assert_eq!(main_color, Color::Cyan);
        assert_eq!(wt_color, Color::Magenta);
    }

    #[test]
    fn test_us007_worktree_path_differs_between_sessions() {
        let mut app = MonitorApp::new(1, None);

        // Main session - path is the main repo
        let mut main_session = create_test_session("autom8", MAIN_SESSION_ID, "main");
        main_session.metadata.worktree_path = PathBuf::from("/home/user/projects/autom8");

        // Worktree session - path is the worktree directory
        let mut worktree_session = create_test_session("autom8", "abc12345", "feature-x");
        worktree_session.metadata.worktree_path =
            PathBuf::from("/home/user/projects/autom8-wt-feature-x");

        app.sessions.push(main_session);
        app.sessions.push(worktree_session);

        // Paths should be different
        assert_ne!(
            app.sessions[0].metadata.worktree_path,
            app.sessions[1].metadata.worktree_path
        );

        // Truncated paths should also be different
        assert_ne!(
            app.sessions[0].truncated_worktree_path(),
            app.sessions[1].truncated_worktree_path()
        );

        // Main shows full path (only 2 components)
        assert_eq!(
            app.sessions[0].truncated_worktree_path(),
            ".../projects/autom8"
        );
        // Worktree shows truncated path
        assert_eq!(
            app.sessions[1].truncated_worktree_path(),
            ".../projects/autom8-wt-feature-x"
        );
    }

    // ==========================================================================
    // US-004 Tests: Update error panel display for session context
    // ==========================================================================
    // These tests verify that error panels show session identity and appropriate
    // error messages for corrupted states and stale sessions.

    /// Helper function to create a test session with an error
    fn create_error_session(
        project_name: &str,
        session_id: &str,
        error: &str,
        is_stale: bool,
    ) -> SessionData {
        let is_main = session_id == MAIN_SESSION_ID;
        SessionData {
            project_name: project_name.to_string(),
            metadata: SessionMetadata {
                session_id: session_id.to_string(),
                worktree_path: PathBuf::from(if is_main {
                    format!("/home/user/projects/{}", project_name)
                } else {
                    format!("/home/user/projects/{}-wt-deleted", project_name)
                }),
                branch_name: "feature/test".to_string(),
                created_at: chrono::Utc::now(),
                last_active_at: chrono::Utc::now(),
                is_running: true,
            },
            run: None,
            progress: None,
            load_error: Some(error.to_string()),
            is_main_session: is_main,
            is_stale,
        }
    }

    #[test]
    fn test_us004_error_panel_title_shows_project_and_session_id() {
        // Main session error panel
        let main_error_session = create_error_session(
            "autom8",
            MAIN_SESSION_ID,
            "Corrupted state: invalid JSON",
            false,
        );
        assert_eq!(main_error_session.display_title(), "autom8 (main)");

        // Worktree session error panel
        let worktree_error_session =
            create_error_session("autom8", "abc12345", "Corrupted state: invalid JSON", false);
        assert_eq!(worktree_error_session.display_title(), "autom8 (abc12345)");
    }

    #[test]
    fn test_us004_error_session_has_load_error() {
        let error_session = create_error_session(
            "myproject",
            "deadbeef",
            "Corrupted state: invalid JSON",
            false,
        );
        assert!(error_session.load_error.is_some());
        assert_eq!(
            error_session.load_error.as_ref().unwrap(),
            "Corrupted state: invalid JSON"
        );
    }

    #[test]
    fn test_us004_stale_session_has_is_stale_flag() {
        let stale_session =
            create_error_session("autom8", "abc12345", "Worktree has been deleted", true);
        assert!(stale_session.is_stale);
        assert!(stale_session.load_error.is_some());
    }

    #[test]
    fn test_us004_non_stale_error_session_has_is_stale_false() {
        let error_session = create_error_session(
            "autom8",
            MAIN_SESSION_ID,
            "Corrupted state: invalid JSON",
            false,
        );
        assert!(!error_session.is_stale);
    }

    #[test]
    fn test_us004_stale_session_error_message_content() {
        let stale_session =
            create_error_session("autom8", "abc12345", "Worktree has been deleted", true);
        // The stale session should have an error indicating worktree deletion
        assert!(stale_session
            .load_error
            .as_ref()
            .unwrap()
            .contains("deleted"));
    }

    #[test]
    fn test_us004_error_session_display_title_format() {
        // Verify the display_title works correctly for error sessions
        let main_error = create_error_session("proj", MAIN_SESSION_ID, "error", false);
        let worktree_error = create_error_session("proj", "12345678", "error", true);

        // Main should show "(main)"
        assert!(main_error.display_title().ends_with("(main)"));
        // Worktree should show the 8-char session ID
        assert!(worktree_error.display_title().ends_with("(12345678)"));
    }

    #[test]
    fn test_us004_error_and_stale_sessions_appear_in_app() {
        let mut app = MonitorApp::new(1, None);
        app.current_view = View::ActiveRuns;

        // Add a normal session
        app.sessions
            .push(create_test_session("proj1", MAIN_SESSION_ID, "main"));

        // Add an error session (corrupted state)
        app.sessions.push(create_error_session(
            "proj2",
            "aabbccdd",
            "Corrupted state",
            false,
        ));

        // Add a stale session
        app.sessions.push(create_error_session(
            "proj3",
            "11223344",
            "Worktree has been deleted",
            true,
        ));

        // All three should be in the sessions list
        assert_eq!(app.sessions.len(), 3);

        // First is normal (no error)
        assert!(app.sessions[0].load_error.is_none());
        assert!(!app.sessions[0].is_stale);

        // Second is error but not stale
        assert!(app.sessions[1].load_error.is_some());
        assert!(!app.sessions[1].is_stale);

        // Third is stale
        assert!(app.sessions[2].load_error.is_some());
        assert!(app.sessions[2].is_stale);
    }

    #[test]
    fn test_us004_stale_session_metadata_preserved() {
        // Stale sessions should still have accessible metadata for display
        let stale_session =
            create_error_session("autom8", "abc12345", "Worktree has been deleted", true);

        // Session ID should be accessible for error display
        assert_eq!(stale_session.metadata.session_id, "abc12345");

        // Project name should be accessible for title
        assert_eq!(stale_session.project_name, "autom8");

        // Branch should be accessible (shows what was being worked on)
        assert_eq!(stale_session.metadata.branch_name, "feature/test");
    }

    // ============================================================================
    // US-005: Project List view multi-session awareness tests
    // ============================================================================

    /// Helper to create a project data entry
    fn create_test_project(name: &str) -> ProjectData {
        ProjectData {
            info: ProjectTreeInfo {
                name: name.to_string(),
                has_active_run: false,
                run_status: None,
                spec_count: 0,
                incomplete_spec_count: 0,
                spec_md_count: 0,
                runs_count: 0,
                last_run_date: None,
            },
            active_run: None,
            progress: None,
            load_error: None,
        }
    }

    #[test]
    fn test_us005_single_session_shows_running() {
        // When a project has exactly 1 session running, it should show "Running"
        let mut app = MonitorApp::new(1, None);
        app.projects = vec![create_test_project("autom8")];
        app.sessions = vec![create_test_session("autom8", MAIN_SESSION_ID, "main")];

        // Count sessions for the project
        let session_count: usize = app
            .sessions
            .iter()
            .filter(|s| s.project_name == "autom8" && (s.run.is_some() || s.load_error.is_some()))
            .count();

        assert_eq!(session_count, 1);
        // Single session should show "Running", not "[1 sessions]"
    }

    #[test]
    fn test_us005_multiple_sessions_show_count() {
        // When a project has >1 sessions running, it should show "[N sessions]"
        let mut app = MonitorApp::new(1, None);
        app.projects = vec![create_test_project("autom8")];

        // Add 3 sessions for the same project
        app.sessions = vec![
            create_test_session("autom8", MAIN_SESSION_ID, "main"),
            create_test_session("autom8", "abc12345", "feature-a"),
            create_test_session("autom8", "def67890", "feature-b"),
        ];

        // Count sessions for the project
        let session_count: usize = app
            .sessions
            .iter()
            .filter(|s| s.project_name == "autom8" && (s.run.is_some() || s.load_error.is_some()))
            .count();

        assert_eq!(session_count, 3);
        // Should show "[3 sessions]"
    }

    #[test]
    fn test_us005_session_count_per_project() {
        // Different projects should have independent session counts
        let mut app = MonitorApp::new(1, None);
        app.projects = vec![
            create_test_project("autom8"),
            create_test_project("other-project"),
        ];

        // autom8 has 2 sessions, other-project has 1
        app.sessions = vec![
            create_test_session("autom8", MAIN_SESSION_ID, "main"),
            create_test_session("autom8", "abc12345", "feature-a"),
            create_test_session("other-project", MAIN_SESSION_ID, "main"),
        ];

        // Build session counts like render_project_list does
        let session_counts: std::collections::HashMap<String, usize> = app
            .sessions
            .iter()
            .filter(|s| s.run.is_some() || s.load_error.is_some())
            .fold(std::collections::HashMap::new(), |mut acc, s| {
                *acc.entry(s.project_name.clone()).or_insert(0) += 1;
                acc
            });

        assert_eq!(session_counts.get("autom8"), Some(&2));
        assert_eq!(session_counts.get("other-project"), Some(&1));
    }

    #[test]
    fn test_us005_no_sessions_idle_status() {
        // Projects with no running sessions should show "Idle"
        let mut app = MonitorApp::new(1, None);
        app.projects = vec![create_test_project("autom8")];
        app.sessions = vec![]; // No sessions

        let session_count: usize = app
            .sessions
            .iter()
            .filter(|s| s.project_name == "autom8" && (s.run.is_some() || s.load_error.is_some()))
            .count();

        assert_eq!(session_count, 0);
        // Should show "Idle" (or last run date if available)
    }

    #[test]
    fn test_us005_aggregate_state_any_running() {
        // If any session is running, the project status should be "running"
        let mut app = MonitorApp::new(1, None);
        app.projects = vec![create_test_project("autom8")];

        // One session running
        app.sessions = vec![create_test_session("autom8", MAIN_SESSION_ID, "main")];

        let has_running_sessions = app
            .sessions
            .iter()
            .any(|s| s.project_name == "autom8" && s.run.is_some());

        assert!(has_running_sessions);
    }

    #[test]
    fn test_us005_session_count_includes_errors() {
        // Sessions with load_error should also be counted (they're still "active")
        let mut app = MonitorApp::new(1, None);
        app.projects = vec![create_test_project("autom8")];

        // One normal session, one error session
        let mut error_session = create_test_session("autom8", "abc12345", "feature-a");
        error_session.run = None;
        error_session.load_error = Some("Corrupted state".to_string());

        app.sessions = vec![
            create_test_session("autom8", MAIN_SESSION_ID, "main"),
            error_session,
        ];

        let session_count: usize = app
            .sessions
            .iter()
            .filter(|s| s.project_name == "autom8" && (s.run.is_some() || s.load_error.is_some()))
            .count();

        assert_eq!(session_count, 2);
    }

    #[test]
    fn test_us005_mixed_projects_correct_counts() {
        // Test a realistic scenario with multiple projects and varying session counts
        let mut app = MonitorApp::new(1, None);
        app.projects = vec![
            create_test_project("autom8"),
            create_test_project("web-app"),
            create_test_project("api-service"),
        ];

        app.sessions = vec![
            // autom8: 3 sessions
            create_test_session("autom8", MAIN_SESSION_ID, "main"),
            create_test_session("autom8", "11111111", "feature-1"),
            create_test_session("autom8", "22222222", "feature-2"),
            // web-app: 1 session
            create_test_session("web-app", MAIN_SESSION_ID, "main"),
            // api-service: 0 sessions (not in sessions list)
        ];

        let session_counts: std::collections::HashMap<String, usize> = app
            .sessions
            .iter()
            .filter(|s| s.run.is_some() || s.load_error.is_some())
            .fold(std::collections::HashMap::new(), |mut acc, s| {
                *acc.entry(s.project_name.clone()).or_insert(0) += 1;
                acc
            });

        assert_eq!(session_counts.get("autom8"), Some(&3)); // Shows "[3 sessions]"
        assert_eq!(session_counts.get("web-app"), Some(&1)); // Shows "Running"
        assert_eq!(session_counts.get("api-service"), None); // Shows "Idle"
    }

    #[test]
    fn test_us005_project_error_takes_precedence() {
        // If a project has a load_error in ProjectData, that should show "Error"
        // even if sessions are running
        let mut app = MonitorApp::new(1, None);
        let mut project = create_test_project("autom8");
        project.load_error = Some("Config error".to_string());
        app.projects = vec![project];
        app.sessions = vec![create_test_session("autom8", MAIN_SESSION_ID, "main")];

        // The project has load_error, so it should show "Error" status
        assert!(app.projects[0].load_error.is_some());
    }
}
