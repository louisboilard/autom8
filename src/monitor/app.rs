//! Monitor TUI Application
//!
//! The main application struct and event loop for the monitor command.

use super::views::View;
use crate::config::{list_projects_tree, ProjectTreeInfo};
use crate::error::Result;
use crate::state::{RunState, StateManager};
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
    widgets::{Block, Borders, List, ListItem, Paragraph, Tabs},
    Frame, Terminal,
};
use std::io::{self, Stdout};
use std::time::Duration;

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

/// Data collected from a single project for display.
#[derive(Debug, Clone)]
pub struct ProjectData {
    pub info: ProjectTreeInfo,
    pub active_run: Option<RunState>,
}

/// The main monitor application state.
pub struct MonitorApp {
    /// Current view being displayed
    current_view: View,
    /// Polling interval in seconds
    poll_interval: u64,
    /// Optional project filter
    project_filter: Option<String>,
    /// Cached project data
    projects: Vec<ProjectData>,
    /// Whether there are any active runs
    has_active_runs: bool,
    /// Whether the app should quit
    should_quit: bool,
    /// Selected index for list navigation
    selected_index: usize,
}

impl MonitorApp {
    /// Create a new MonitorApp with the given configuration.
    pub fn new(poll_interval: u64, project_filter: Option<String>) -> Self {
        Self {
            current_view: View::ProjectList, // Will be updated on first refresh
            poll_interval,
            project_filter,
            projects: Vec::new(),
            has_active_runs: false,
            should_quit: false,
            selected_index: 0,
        }
    }

    /// Refresh project data from disk.
    pub fn refresh_data(&mut self) -> Result<()> {
        let tree_infos = list_projects_tree()?;

        // Filter by project if specified
        let filtered: Vec<_> = if let Some(ref filter) = self.project_filter {
            tree_infos
                .into_iter()
                .filter(|p| p.name == *filter)
                .collect()
        } else {
            tree_infos
        };

        // Collect project data including active runs
        self.projects = filtered
            .into_iter()
            .map(|info| {
                let active_run = if info.has_active_run {
                    StateManager::for_project(&info.name)
                        .ok()
                        .and_then(|sm| sm.load_current().ok().flatten())
                } else {
                    None
                };
                ProjectData { info, active_run }
            })
            .collect();

        // Update active runs status
        self.has_active_runs = self.projects.iter().any(|p| p.active_run.is_some());

        // If current view is ActiveRuns but no active runs, switch to ProjectList
        if self.current_view == View::ActiveRuns && !self.has_active_runs {
            self.current_view = View::ProjectList;
        }

        Ok(())
    }

    /// Switch to the next view.
    pub fn next_view(&mut self) {
        self.current_view = self.current_view.next(!self.has_active_runs);
        self.selected_index = 0;
    }

    /// Handle keyboard input.
    pub fn handle_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::Char('q') | KeyCode::Char('Q') => {
                self.should_quit = true;
            }
            KeyCode::Tab => {
                self.next_view();
            }
            KeyCode::Up => {
                if self.selected_index > 0 {
                    self.selected_index -= 1;
                }
            }
            KeyCode::Down => {
                let max_index = match self.current_view {
                    View::ProjectList => self.projects.len().saturating_sub(1),
                    View::ActiveRuns => self
                        .projects
                        .iter()
                        .filter(|p| p.active_run.is_some())
                        .count()
                        .saturating_sub(1),
                    View::RunHistory => 0, // TODO: Implement run history navigation
                };
                if self.selected_index < max_index {
                    self.selected_index += 1;
                }
            }
            _ => {}
        }
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
                    .title(" autom8 monitor "),
            )
            .select(selected_idx)
            .style(Style::default().fg(Color::White))
            .highlight_style(
                Style::default()
                    .fg(Color::Cyan)
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
        let active: Vec<_> = self
            .projects
            .iter()
            .filter(|p| p.active_run.is_some())
            .collect();

        if active.is_empty() {
            let message = Paragraph::new("No active runs")
                .style(Style::default().fg(Color::DarkGray))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Active Runs "),
                );
            frame.render_widget(message, area);
            return;
        }

        let items: Vec<ListItem> = active
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let run = p.active_run.as_ref().unwrap();
                let status = format!(
                    "{}: {} - {:?} (Story: {})",
                    p.info.name,
                    run.branch,
                    run.machine_state,
                    run.current_story.as_deref().unwrap_or("N/A")
                );
                let style = if i == self.selected_index {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                ListItem::new(status).style(style)
            })
            .collect();

        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Active Runs "),
        );

        frame.render_widget(list, area);
    }

    fn render_project_list(&self, frame: &mut Frame, area: Rect) {
        if self.projects.is_empty() {
            let message = if self.project_filter.is_some() {
                "No matching projects found"
            } else {
                "No projects found. Run 'autom8' in a project directory to create one."
            };
            let paragraph = Paragraph::new(message)
                .style(Style::default().fg(Color::DarkGray))
                .block(Block::default().borders(Borders::ALL).title(" Projects "));
            frame.render_widget(paragraph, area);
            return;
        }

        let items: Vec<ListItem> = self
            .projects
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let status_indicator = if p.active_run.is_some() { "●" } else { "○" };
                let status_color = if p.active_run.is_some() {
                    Color::Green
                } else {
                    Color::DarkGray
                };

                let specs_info = if p.info.incomplete_spec_count > 0 {
                    format!(
                        " ({} specs, {} incomplete)",
                        p.info.spec_count, p.info.incomplete_spec_count
                    )
                } else if p.info.spec_count > 0 {
                    format!(" ({} specs)", p.info.spec_count)
                } else {
                    String::new()
                };

                let line = Line::from(vec![
                    Span::styled(
                        format!("{} ", status_indicator),
                        Style::default().fg(status_color),
                    ),
                    Span::styled(
                        &p.info.name,
                        if i == self.selected_index {
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::White)
                        },
                    ),
                    Span::styled(specs_info, Style::default().fg(Color::DarkGray)),
                ]);

                ListItem::new(line)
            })
            .collect();

        let title = format!(" Projects ({}) ", self.projects.len());
        let list = List::new(items).block(Block::default().borders(Borders::ALL).title(title));

        frame.render_widget(list, area);
    }

    fn render_run_history(&self, frame: &mut Frame, area: Rect) {
        // Placeholder for run history - will be implemented in US-007
        let message = Paragraph::new("Run history view (coming soon)")
            .style(Style::default().fg(Color::DarkGray))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Run History "),
            );
        frame.render_widget(message, area);
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let help_text = " Tab: switch view | ↑↓: navigate | Q: quit ";
        let footer = Paragraph::new(help_text).style(Style::default().fg(Color::DarkGray));
        frame.render_widget(footer, area);
    }
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
}
