//! Monitor TUI Application
//!
//! The main application struct and event loop for the monitor command.

use super::views::View;
use crate::error::Result;
use crate::state::{LiveState, MachineState, RunState};
use crate::ui::shared::{
    format_duration, format_relative_time, format_state_label, load_run_history, load_ui_data,
    ProjectData, RunHistoryEntry, RunHistoryOptions, SessionData, Status,
};
use chrono::Utc;
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
use std::time::Duration;

// ============================================================================
// Color Constants (consistent with output.rs autom8 branding)
// ============================================================================

/// Cyan - primary branding color, used for headers and highlights
const COLOR_PRIMARY: Color = Color::Cyan;
/// Green - success states
const COLOR_SUCCESS: Color = Color::Green;
/// Yellow - warning/in-progress states (reviewing, general warnings)
const COLOR_WARNING: Color = Color::Yellow;
/// Red - error/failure states
const COLOR_ERROR: Color = Color::Red;
/// Gray - dimmed/secondary text (idle, setup phases)
const COLOR_DIM: Color = Color::DarkGray;
/// Magenta - review/correction states (attention needed)
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

// Data types (RunProgress, ProjectData, SessionData, RunHistoryEntry) are imported
// from crate::ui::shared.
// Time formatting (format_duration, format_relative_time) and state labels
// (format_state_label) are also imported from crate::ui::shared for consistency.

/// Get a color for a machine state using the shared Status enum.
///
/// This uses the shared Status enum to ensure consistent color mapping
/// between GUI and TUI. The Status enum groups MachineState values into
/// semantic categories, and we map those to terminal colors here.
fn state_color(state: MachineState) -> Color {
    match Status::from_machine_state(state) {
        Status::Setup => COLOR_DIM,         // Gray - setup/initialization
        Status::Running => COLOR_PRIMARY,   // Cyan - active implementation
        Status::Reviewing => COLOR_WARNING, // Yellow/Amber - evaluation
        Status::Correcting => COLOR_REVIEW, // Magenta - attention needed
        Status::Success => COLOR_SUCCESS,   // Green - success path
        Status::Warning => COLOR_WARNING,   // Yellow - general warnings
        Status::Error => COLOR_ERROR,       // Red - failure
        Status::Idle => COLOR_DIM,          // Gray - inactive
    }
}

/// The main monitor application state.
pub struct MonitorApp {
    /// Current view being displayed
    current_view: View,
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
    /// Cache of full RunState objects for detail view (keyed by run_id)
    run_state_cache: std::collections::HashMap<String, RunState>,
}

impl Default for MonitorApp {
    fn default() -> Self {
        Self::new()
    }
}

impl MonitorApp {
    /// Create a new MonitorApp.
    pub fn new() -> Self {
        Self {
            current_view: View::ProjectList, // Will be updated on first refresh
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
            run_state_cache: std::collections::HashMap::new(),
        }
    }

    /// Refresh project data from disk.
    ///
    /// This method handles corrupted or invalid state files gracefully,
    /// showing error indicators in the UI instead of crashing.
    pub fn refresh_data(&mut self) -> Result<()> {
        // Use shared data loading function
        // TUI handles errors gracefully - log and continue with defaults
        let ui_data = match load_ui_data(None) {
            Ok(data) => data,
            Err(e) => {
                // Log error but continue with empty data
                eprintln!("Warning: Failed to load UI data: {}", e);
                Default::default()
            }
        };

        self.projects = ui_data.projects;
        self.sessions = ui_data.sessions;
        self.has_active_runs = ui_data.has_active_runs;

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
        // Use run_history_filter if set (from selecting a project in Project List)
        let options = RunHistoryOptions {
            project_filter: self.run_history_filter.clone(),
            max_entries: Some(100), // Limit to last 100 runs for performance
        };

        // Use shared function to load run history
        // TUI needs full state for detail view, so request it
        let history_data = load_run_history(&self.projects, &options, true).unwrap_or_default();

        // Update the run state cache with the full states
        self.run_state_cache = history_data.run_states;

        self.run_history = history_data.entries;

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
        // Check if session appears stuck (heartbeat is stale)
        let appears_stuck = session.appears_stuck();

        let state_str = if appears_stuck {
            format!("{} (Not responding)", format_state_label(run.machine_state))
        } else {
            format_state_label(run.machine_state).to_string()
        };
        let state_color_value = if appears_stuck {
            COLOR_WARNING
        } else {
            state_color(run.machine_state)
        };

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
                Span::styled(&state_str, Style::default().fg(state_color_value)),
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

        // Output snippet section (prefer live output when available)
        let output_snippet = self.get_output_snippet(run, session.live_output.as_ref());
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

    /// Staleness threshold for live output (5 seconds)
    const LIVE_OUTPUT_STALE_SECONDS: i64 = 5;

    /// Get the latest output snippet from a run, preferring live output when available and fresh.
    ///
    /// Priority:
    /// 1. If state is RunningClaude and live_output exists and is fresh (<5 seconds old), use live output
    /// 2. Otherwise, if iteration has output_snippet, use that (last 5 lines)
    /// 3. Fallback to status message based on machine state
    fn get_output_snippet(&self, run: &RunState, live_output: Option<&LiveState>) -> String {
        // Check for fresh live output when Claude is running
        if run.machine_state == MachineState::RunningClaude {
            if let Some(live) = live_output {
                // Check if live output is fresh (within 5 seconds)
                let age = Utc::now().signed_duration_since(live.updated_at);
                if age.num_seconds() < Self::LIVE_OUTPUT_STALE_SECONDS
                    && !live.output_lines.is_empty()
                {
                    // Take last 5 lines from live output (consistent with iteration output)
                    let take_count = 5.min(live.output_lines.len());
                    let start = live.output_lines.len().saturating_sub(take_count);
                    return live.output_lines[start..].join("\n");
                }
            }
        }

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
            let message = "No projects found. Run 'autom8' in a project directory to create one.";
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
                let (status_indicator, status_clr) = match entry.status {
                    crate::state::RunStatus::Completed => ("✓", COLOR_SUCCESS),
                    crate::state::RunStatus::Failed => ("✗", COLOR_ERROR),
                    crate::state::RunStatus::Running => ("●", COLOR_WARNING),
                    crate::state::RunStatus::Interrupted => ("⚠", COLOR_WARNING),
                };

                // Format date/time
                let date_str = entry.started_at.format("%Y-%m-%d %H:%M").to_string();

                // Story count
                let story_str = format!("{}/{}", entry.completed_stories, entry.total_stories);

                // Duration if completed
                let duration_str = if let Some(finished) = entry.finished_at {
                    let duration = finished.signed_duration_since(entry.started_at);
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

        // Get the full run state from cache (needed for iterations)
        let run_state = self.run_state_cache.get(&entry.run_id);

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

        // Build detail content using entry fields
        // Status with color
        let (status_str, status_clr) = match entry.status {
            crate::state::RunStatus::Completed => ("Completed", COLOR_SUCCESS),
            crate::state::RunStatus::Failed => ("Failed", COLOR_ERROR),
            crate::state::RunStatus::Running => ("Running", COLOR_WARNING),
            crate::state::RunStatus::Interrupted => ("Interrupted", COLOR_WARNING),
        };

        // Duration
        let duration_str = if let Some(finished) = entry.finished_at {
            let duration = finished.signed_duration_since(entry.started_at);
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
                    entry.started_at.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
                    Style::default().fg(Color::White),
                ),
            ]),
            Line::from(vec![
                Span::styled("Duration:   ", Style::default().fg(COLOR_DIM)),
                Span::styled(&duration_str, Style::default().fg(COLOR_WARNING)),
            ]),
            Line::from(vec![
                Span::styled("Branch:     ", Style::default().fg(COLOR_DIM)),
                Span::styled(&entry.branch, Style::default().fg(COLOR_PRIMARY)),
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

        // Add iteration details from cached run state (if available)
        if let Some(run) = run_state {
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
        } else {
            lines.push(Line::from(Span::styled(
                "  (iteration details not available)",
                Style::default().fg(COLOR_DIM),
            )));
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
///
/// The refresh interval is hardcoded to 100ms for responsive UI updates.
pub fn run_monitor() -> Result<()> {
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
    let mut app = MonitorApp::new();

    // Initial data load
    app.refresh_data()?;

    // Set default view based on active runs
    if app.has_active_runs {
        app.current_view = View::ActiveRuns;
    }

    // Main event loop - hardcoded to 100ms for responsive UI
    let poll_duration = Duration::from_millis(100);

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
