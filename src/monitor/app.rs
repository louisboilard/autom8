//! Monitor TUI Application
//!
//! The main application struct and event loop for the monitor command.

use super::views::View;
use crate::config::{list_projects_tree, ProjectTreeInfo};
use crate::error::Result;
use crate::spec::Spec;
use crate::state::{MachineState, RunState, StateManager};
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

/// Get a color for a machine state
fn state_color(state: MachineState) -> Color {
    match state {
        MachineState::Idle => Color::DarkGray,
        MachineState::LoadingSpec | MachineState::GeneratingSpec => Color::Yellow,
        MachineState::Initializing | MachineState::PickingStory => Color::Blue,
        MachineState::RunningClaude => Color::Cyan,
        MachineState::Reviewing | MachineState::Correcting => Color::Magenta,
        MachineState::Committing | MachineState::CreatingPR => Color::Green,
        MachineState::Completed => Color::Green,
        MachineState::Failed => Color::Red,
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
    /// Cached project data
    projects: Vec<ProjectData>,
    /// Whether there are any active runs
    has_active_runs: bool,
    /// Whether the app should quit
    should_quit: bool,
    /// Selected index for list navigation
    selected_index: usize,
    /// Project name to filter Run History view (set when pressing Enter on Project List)
    run_history_filter: Option<String>,
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
            run_history_filter: None,
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

        // Collect project data including active runs and progress
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
                    info,
                    active_run,
                    progress,
                }
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
                // Clear run history filter when switching views with Tab
                self.run_history_filter = None;
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
            KeyCode::Enter => {
                self.handle_enter();
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
                }
            }
            View::ActiveRuns | View::RunHistory => {
                // No action for now in other views
            }
        }
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

        match active.len() {
            1 => {
                // Full screen for single run
                self.render_run_detail(frame, area, active[0], true);
            }
            2 => {
                // Vertical split (side by side) for two runs
                let chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(area);

                self.render_run_detail(frame, chunks[0], active[0], false);
                self.render_run_detail(frame, chunks[1], active[1], false);
            }
            _ => {
                // 3+ runs: list on left, detail on right
                let chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
                    .split(area);

                // Render list on left
                self.render_active_runs_list(frame, chunks[0], &active);

                // Render detail for selected run on right
                if let Some(selected) = active.get(self.selected_index) {
                    self.render_run_detail(frame, chunks[1], selected, true);
                }
            }
        }
    }

    /// Render the list view for 3+ active runs
    fn render_active_runs_list(&self, frame: &mut Frame, area: Rect, active: &[&ProjectData]) {
        let items: Vec<ListItem> = active
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let run = p.active_run.as_ref().unwrap();
                let state_str = format_state(run.machine_state);

                let line = Line::from(vec![
                    Span::styled(
                        if i == self.selected_index {
                            "▶ "
                        } else {
                            "  "
                        },
                        Style::default().fg(Color::Cyan),
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
                    Span::styled(
                        format!(" ({})", state_str),
                        Style::default().fg(state_color(run.machine_state)),
                    ),
                ]);

                ListItem::new(line)
            })
            .collect();

        let title = format!(" Runs ({}) ", active.len());
        let list = List::new(items).block(Block::default().borders(Borders::ALL).title(title));

        frame.render_widget(list, area);
    }

    /// Render detailed view for a single run
    fn render_run_detail(&self, frame: &mut Frame, area: Rect, project: &ProjectData, full: bool) {
        let run = project.active_run.as_ref().unwrap();

        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", project.info.name));

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
                Span::styled("State: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    state_str,
                    Style::default().fg(state_color(run.machine_state)),
                ),
            ]),
            Line::from(vec![
                Span::styled("Story: ", Style::default().fg(Color::DarkGray)),
                Span::styled(story, Style::default().fg(Color::White)),
            ]),
            Line::from(vec![
                Span::styled("Progress: ", Style::default().fg(Color::DarkGray)),
                Span::styled(&progress_str, Style::default().fg(Color::Cyan)),
            ]),
            Line::from(vec![
                Span::styled("Duration: ", Style::default().fg(Color::DarkGray)),
                Span::styled(&duration, Style::default().fg(Color::Yellow)),
            ]),
        ];

        if full {
            info_lines.push(Line::from(vec![
                Span::styled("Branch: ", Style::default().fg(Color::DarkGray)),
                Span::styled(&run.branch, Style::default().fg(Color::White)),
            ]));
        }

        let info = Paragraph::new(info_lines);
        frame.render_widget(info, chunks[0]);

        // Output snippet section
        let output_snippet = self.get_output_snippet(run);
        let output = Paragraph::new(output_snippet)
            .style(Style::default().fg(Color::DarkGray))
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
                let is_selected = i == self.selected_index;

                // Status indicator and text
                let (status_indicator, status_text, status_color) = if p.active_run.is_some() {
                    ("●", "Running".to_string(), Color::Green)
                } else if let Some(last_run) = p.info.last_run_date {
                    (
                        "○",
                        format!("Last run: {}", format_relative_time(last_run)),
                        Color::DarkGray,
                    )
                } else {
                    ("○", "Idle".to_string(), Color::DarkGray)
                };

                let name_style = if is_selected {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };

                let line = Line::from(vec![
                    Span::styled(
                        if is_selected { "▶ " } else { "  " },
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::styled(
                        format!("{} ", status_indicator),
                        Style::default().fg(status_color),
                    ),
                    Span::styled(&p.info.name, name_style),
                    Span::styled(
                        format!("  {}", status_text),
                        Style::default().fg(status_color),
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
        // Placeholder for run history - will be implemented in US-007
        let title = if let Some(ref project) = self.run_history_filter {
            format!(" Run History: {} ", project)
        } else {
            " Run History ".to_string()
        };

        let message = if self.run_history_filter.is_some() {
            "Run history for this project (coming soon)"
        } else {
            "Run history view (coming soon)"
        };

        let paragraph = Paragraph::new(message)
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::ALL).title(title));
        frame.render_widget(paragraph, area);
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let help_text = match self.current_view {
            View::ProjectList => {
                " Tab: switch view | ↑↓: navigate | Enter: view history | Q: quit "
            }
            _ => " Tab: switch view | ↑↓: navigate | Q: quit ",
        };
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
            },
        ];
        app.selected_index = 1;

        app.handle_key(KeyCode::Enter);

        assert_eq!(app.run_history_filter(), Some("second-project"));
    }
}
