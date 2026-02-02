//! GUI application entry point.
//!
//! This module contains the eframe application setup and main window
//! configuration for the autom8 GUI.

use crate::config::{list_projects_tree, ProjectTreeInfo};
use crate::error::{Autom8Error, Result};
use crate::gui::components::{
    format_duration, format_relative_time, format_state, state_to_color, truncate_with_ellipsis,
    MAX_BRANCH_LENGTH, MAX_TEXT_LENGTH,
};
use crate::gui::theme::{self, colors, rounding, spacing};
use crate::gui::typography::{self, FontSize, FontWeight};
use crate::spec::Spec;
use crate::state::{LiveState, MachineState, RunState, SessionMetadata, StateManager};
use crate::worktree::MAIN_SESSION_ID;
use eframe::egui::{self, Color32, Rect, Rounding, Sense, Stroke, Vec2};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Default window width in pixels.
const DEFAULT_WIDTH: f32 = 700.0;

/// Default window height in pixels.
const DEFAULT_HEIGHT: f32 = 500.0;

/// Minimum window width in pixels.
const MIN_WIDTH: f32 = 400.0;

/// Minimum window height in pixels.
const MIN_HEIGHT: f32 = 300.0;

/// Height of the header/tab bar area (48px = 3 * LG spacing).
const HEADER_HEIGHT: f32 = 48.0;

/// Tab indicator underline height.
const TAB_UNDERLINE_HEIGHT: f32 = 2.0;

/// Tab horizontal padding (uses LG from spacing scale).
const TAB_PADDING_H: f32 = 16.0; // spacing::LG

/// Default refresh interval for data loading (500ms for GUI, less aggressive than TUI).
pub const DEFAULT_REFRESH_INTERVAL_MS: u64 = 500;

// ============================================================================
// Grid Layout Constants (using spacing scale)
// ============================================================================

/// Minimum width for a card in the grid layout.
const CARD_MIN_WIDTH: f32 = 280.0;

/// Maximum width for a card in the grid layout.
const CARD_MAX_WIDTH: f32 = 400.0;

/// Spacing between cards in the grid (uses LG from spacing scale).
const CARD_SPACING: f32 = 16.0; // spacing::LG

/// Internal padding for cards (uses LG from spacing scale).
const CARD_PADDING: f32 = 16.0; // spacing::LG

/// Minimum height for a card.
const CARD_MIN_HEIGHT: f32 = 240.0;

/// Number of output lines to display in session cards.
const OUTPUT_LINES_TO_SHOW: usize = 5;

// MAX_TEXT_LENGTH and MAX_BRANCH_LENGTH are imported from components module.

// ============================================================================
// Projects View Constants (using spacing scale)
// ============================================================================

/// Height of each row in the project list.
const PROJECT_ROW_HEIGHT: f32 = 56.0;

/// Horizontal padding within project rows (uses MD from spacing scale).
const PROJECT_ROW_PADDING_H: f32 = 12.0; // spacing::MD

/// Vertical padding within project rows (uses MD from spacing scale).
const PROJECT_ROW_PADDING_V: f32 = 12.0; // spacing::MD

/// Size of the status indicator dot in the project list.
const PROJECT_STATUS_DOT_RADIUS: f32 = 5.0;

// ============================================================================
// Data Layer Types
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
/// Represents an archived run for a project, displayed in the right panel
/// when a project is selected.
#[derive(Debug, Clone)]
pub struct RunHistoryEntry {
    /// The run ID.
    pub run_id: String,
    /// When the run started.
    pub started_at: chrono::DateTime<chrono::Utc>,
    /// When the run finished (if completed).
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    /// The run status (completed/failed).
    pub status: crate::state::RunStatus,
    /// Number of completed stories.
    pub completed_stories: usize,
    /// Total number of stories in the spec.
    pub total_stories: usize,
    /// Branch name for this run.
    pub branch: String,
}

impl RunHistoryEntry {
    /// Create a RunHistoryEntry from a RunState.
    pub fn from_run_state(run: &RunState) -> Self {
        // Count completed stories by looking at iterations with status Completed
        let completed_stories = run
            .iterations
            .iter()
            .filter(|i| i.status == crate::state::IterationStatus::Success)
            .map(|i| &i.story_id)
            .collect::<std::collections::HashSet<_>>()
            .len();

        // Total stories is harder to determine from archived state
        // Use the iteration count as a proxy (each story should have at least one iteration)
        let story_ids: std::collections::HashSet<_> =
            run.iterations.iter().map(|i| &i.story_id).collect();
        let total_stories = story_ids.len().max(1);

        Self {
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
            crate::state::RunStatus::Completed => "Completed",
            crate::state::RunStatus::Failed => "Failed",
            crate::state::RunStatus::Running => "Running",
        }
    }

    /// Get the status color for display.
    pub fn status_color(&self) -> Color32 {
        match self.status {
            crate::state::RunStatus::Completed => colors::STATUS_SUCCESS,
            crate::state::RunStatus::Failed => colors::STATUS_ERROR,
            crate::state::RunStatus::Running => colors::STATUS_RUNNING,
        }
    }
}

// Time formatting utilities (format_duration, format_relative_time) and
// text utilities (truncate_with_ellipsis, format_state) are now in the
// components module and re-exported for use here.

// ============================================================================
// Tab Types
// ============================================================================

/// The available tabs in the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    /// View of currently active runs.
    #[default]
    ActiveRuns,
    /// View of projects.
    Projects,
}

impl Tab {
    /// Returns the display label for this tab.
    pub fn label(self) -> &'static str {
        match self {
            Tab::ActiveRuns => "Active Runs",
            Tab::Projects => "Projects",
        }
    }

    /// Returns all available tabs.
    pub fn all() -> &'static [Tab] {
        &[Tab::ActiveRuns, Tab::Projects]
    }
}

/// The main GUI application state.
///
/// This struct holds all UI state and loaded data, similar to the TUI's `MonitorApp`.
/// Data is refreshed at a configurable interval (default 500ms).
pub struct Autom8App {
    /// Optional project filter to show only a specific project.
    project_filter: Option<String>,
    /// Currently selected tab.
    current_tab: Tab,

    // ========================================================================
    // Data Layer
    // ========================================================================
    /// Cached project data (used for Project List view).
    projects: Vec<ProjectData>,
    /// Cached session data for Active Runs view.
    /// Contains only running sessions (is_running=true and not stale).
    sessions: Vec<SessionData>,
    /// Whether there are any active runs.
    has_active_runs: bool,

    // ========================================================================
    // Selection State
    // ========================================================================
    /// Currently selected project name in the Projects tab.
    /// Used for the master-detail split view.
    selected_project: Option<String>,

    // ========================================================================
    // Run History Cache
    // ========================================================================
    /// Cached run history for the selected project.
    /// Loaded when a project is selected, cleared when deselected.
    run_history: Vec<RunHistoryEntry>,

    // ========================================================================
    // Loading State
    // ========================================================================
    /// Whether the initial data load has completed.
    /// Used to show a brief loading state on first render.
    initial_load_complete: bool,

    // ========================================================================
    // Refresh Timing
    // ========================================================================
    /// Time of the last data refresh.
    last_refresh: Instant,
    /// Refresh interval for data loading.
    refresh_interval: Duration,
}

impl Autom8App {
    /// Create a new application instance.
    ///
    /// # Arguments
    ///
    /// * `project_filter` - Optional project name to filter the view
    pub fn new(project_filter: Option<String>) -> Self {
        Self::with_refresh_interval(
            project_filter,
            Duration::from_millis(DEFAULT_REFRESH_INTERVAL_MS),
        )
    }

    /// Create a new application instance with a custom refresh interval.
    ///
    /// # Arguments
    ///
    /// * `project_filter` - Optional project name to filter the view
    /// * `refresh_interval` - How often to refresh data from disk
    pub fn with_refresh_interval(
        project_filter: Option<String>,
        refresh_interval: Duration,
    ) -> Self {
        let mut app = Self {
            project_filter,
            current_tab: Tab::default(),
            projects: Vec::new(),
            sessions: Vec::new(),
            has_active_runs: false,
            selected_project: None,
            run_history: Vec::new(),
            initial_load_complete: false,
            last_refresh: Instant::now(),
            refresh_interval,
        };
        // Initial data load
        app.refresh_data();
        app.initial_load_complete = true;
        app
    }

    /// Returns whether the initial data load has completed.
    pub fn is_initial_load_complete(&self) -> bool {
        self.initial_load_complete
    }

    /// Returns the currently selected tab.
    pub fn current_tab(&self) -> Tab {
        self.current_tab
    }

    /// Returns the loaded projects.
    pub fn projects(&self) -> &[ProjectData] {
        &self.projects
    }

    /// Returns the active sessions.
    pub fn sessions(&self) -> &[SessionData] {
        &self.sessions
    }

    /// Returns whether there are any active runs.
    pub fn has_active_runs(&self) -> bool {
        self.has_active_runs
    }

    /// Returns the current refresh interval.
    pub fn refresh_interval(&self) -> Duration {
        self.refresh_interval
    }

    /// Sets the refresh interval.
    pub fn set_refresh_interval(&mut self, interval: Duration) {
        self.refresh_interval = interval;
    }

    /// Returns the currently selected project name.
    pub fn selected_project(&self) -> Option<&str> {
        self.selected_project.as_deref()
    }

    /// Toggles the selection of a project.
    /// If the project is already selected, it becomes deselected.
    /// If a different project is selected, it becomes the new selection.
    /// Also loads/clears run history for the selected project.
    pub fn toggle_project_selection(&mut self, project_name: &str) {
        if self.selected_project.as_deref() == Some(project_name) {
            // Deselect: clear selection and history
            self.selected_project = None;
            self.run_history.clear();
        } else {
            // Select new project: update selection and load history
            self.selected_project = Some(project_name.to_string());
            self.load_run_history(project_name);
        }
    }

    /// Load run history for a specific project.
    /// Populates self.run_history with archived runs, sorted newest first.
    fn load_run_history(&mut self, project_name: &str) {
        self.run_history.clear();

        // Get the StateManager for this project
        let sm = match StateManager::for_project(project_name) {
            Ok(sm) => sm,
            Err(_) => return, // Can't load history
        };

        // Load archived runs
        let archived = match sm.list_archived() {
            Ok(runs) => runs,
            Err(_) => return, // Can't load history
        };

        // Convert to RunHistoryEntry and store (already sorted newest first by list_archived)
        self.run_history = archived
            .iter()
            .map(RunHistoryEntry::from_run_state)
            .collect();
    }

    /// Returns the run history for the selected project.
    pub fn run_history(&self) -> &[RunHistoryEntry] {
        &self.run_history
    }

    /// Returns whether a project is currently selected.
    pub fn is_project_selected(&self, project_name: &str) -> bool {
        self.selected_project.as_deref() == Some(project_name)
    }

    // ========================================================================
    // Data Loading
    // ========================================================================

    /// Refresh data from disk if the refresh interval has elapsed.
    ///
    /// This method is called on every frame and only performs actual
    /// file I/O when the refresh interval has passed.
    pub fn maybe_refresh(&mut self) {
        if self.last_refresh.elapsed() >= self.refresh_interval {
            self.refresh_data();
        }
    }

    /// Refresh all data from disk.
    ///
    /// Loads project and session data, handling errors gracefully.
    /// Missing or corrupted files are captured as `load_error` strings
    /// rather than causing failures.
    pub fn refresh_data(&mut self) {
        self.last_refresh = Instant::now();

        // Load projects (handles errors gracefully)
        let tree_infos = list_projects_tree().unwrap_or_default();

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
            .iter()
            .map(|info| {
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
            })
            .collect();

        // Refresh sessions for Active Runs view
        self.refresh_sessions(&filtered);

        // Update active runs status based on sessions
        self.has_active_runs = !self.sessions.is_empty();
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

        self.sessions = sessions;
    }
}

impl eframe::App for Autom8App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Refresh data from disk if interval has elapsed
        self.maybe_refresh();

        // Request repaint at refresh interval to ensure timely updates
        ctx.request_repaint_after(self.refresh_interval);

        // Header with tab bar
        egui::TopBottomPanel::top("header")
            .exact_height(HEADER_HEIGHT)
            .frame(
                egui::Frame::none()
                    .fill(colors::SURFACE)
                    .inner_margin(egui::Margin::symmetric(spacing::LG, 0.0)),
            )
            .show(ctx, |ui| {
                self.render_header(ui);
            });

        // Content area fills remaining space
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(colors::BACKGROUND)
                    .inner_margin(egui::Margin::same(spacing::LG)),
            )
            .show(ctx, |ui| {
                self.render_content(ui);
            });
    }
}

impl Autom8App {
    /// Render the header area with tab bar.
    fn render_header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            ui.add_space(spacing::XS);

            for tab in Tab::all() {
                let is_active = *tab == self.current_tab;
                if self.render_tab(ui, *tab, is_active) {
                    self.current_tab = *tab;
                }
                ui.add_space(spacing::XS);
            }
        });

        // Draw bottom border for header
        let rect = ui.max_rect();
        ui.painter().hline(
            rect.x_range(),
            rect.bottom(),
            Stroke::new(1.0, colors::BORDER),
        );
    }

    /// Render a single tab button. Returns true if clicked.
    fn render_tab(&self, ui: &mut egui::Ui, tab: Tab, is_active: bool) -> bool {
        let label = tab.label();

        // Calculate tab size
        let text_galley = ui.fonts(|f| {
            f.layout_no_wrap(
                label.to_string(),
                typography::font(FontSize::Body, FontWeight::Medium),
                colors::TEXT_PRIMARY,
            )
        });
        let text_size = text_galley.size();
        let tab_size = egui::vec2(
            text_size.x + TAB_PADDING_H * 2.0,
            HEADER_HEIGHT - TAB_UNDERLINE_HEIGHT,
        );

        // Allocate space for the tab
        let (rect, response) = ui.allocate_exact_size(tab_size, Sense::click());

        // Determine visual state
        let is_hovered = response.hovered();

        // Draw tab background on hover (subtle)
        if is_hovered && !is_active {
            ui.painter().rect_filled(
                rect,
                Rounding::same(rounding::BUTTON),
                colors::SURFACE_HOVER,
            );
        }

        // Draw text
        let text_color = if is_active {
            colors::TEXT_PRIMARY
        } else if is_hovered {
            colors::TEXT_SECONDARY
        } else {
            colors::TEXT_MUTED
        };

        let text_pos = egui::pos2(
            rect.center().x - text_size.x / 2.0,
            rect.center().y - text_size.y / 2.0,
        );

        ui.painter().galley(
            text_pos,
            ui.fonts(|f| {
                f.layout_no_wrap(
                    label.to_string(),
                    typography::font(
                        FontSize::Body,
                        if is_active {
                            FontWeight::SemiBold
                        } else {
                            FontWeight::Medium
                        },
                    ),
                    text_color,
                )
            }),
            Color32::TRANSPARENT,
        );

        // Draw underline indicator for active tab
        if is_active {
            let underline_rect = egui::Rect::from_min_size(
                egui::pos2(rect.left(), rect.bottom() - TAB_UNDERLINE_HEIGHT),
                egui::vec2(rect.width(), TAB_UNDERLINE_HEIGHT),
            );
            ui.painter()
                .rect_filled(underline_rect, Rounding::ZERO, colors::ACCENT);
        }

        response.clicked()
    }

    /// Render the content area based on the current tab.
    fn render_content(&mut self, ui: &mut egui::Ui) {
        match self.current_tab {
            Tab::ActiveRuns => self.render_active_runs(ui),
            Tab::Projects => self.render_projects(ui),
        }
    }

    /// Render the Active Runs view.
    fn render_active_runs(&self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            // Header section with consistent spacing
            ui.label(
                egui::RichText::new("Active Runs")
                    .font(typography::font(FontSize::Title, FontWeight::SemiBold))
                    .color(colors::TEXT_PRIMARY),
            );

            ui.add_space(spacing::SM);

            if let Some(ref filter) = self.project_filter {
                ui.label(
                    egui::RichText::new(format!("Filtering by project: {}", filter))
                        .font(typography::font(FontSize::Body, FontWeight::Regular))
                        .color(colors::TEXT_SECONDARY),
                );
                ui.add_space(spacing::SM);
            }

            // Empty state or grid layout
            if self.sessions.is_empty() {
                self.render_empty_active_runs(ui);
            } else {
                self.render_sessions_grid(ui);
            }
        });
    }

    /// Render the empty state for Active Runs view.
    fn render_empty_active_runs(&self, ui: &mut egui::Ui) {
        ui.add_space(spacing::XXL);

        // Center the empty state message
        ui.vertical_centered(|ui| {
            ui.add_space(spacing::XXL + spacing::LG);

            ui.label(
                egui::RichText::new("No active runs")
                    .font(typography::font(FontSize::Heading, FontWeight::Medium))
                    .color(colors::TEXT_MUTED),
            );

            ui.add_space(spacing::SM);

            ui.label(
                egui::RichText::new("Run autom8 to start implementing a feature")
                    .font(typography::font(FontSize::Body, FontWeight::Regular))
                    .color(colors::TEXT_MUTED),
            );
        });
    }

    /// Calculate the number of grid columns based on available width.
    fn calculate_grid_columns(available_width: f32) -> usize {
        // Calculate how many cards fit, accounting for spacing
        let card_with_spacing = CARD_MIN_WIDTH + CARD_SPACING;
        let columns = ((available_width + CARD_SPACING) / card_with_spacing).floor() as usize;

        // Clamp to reasonable range: minimum 2, maximum 4
        columns.clamp(2, 4)
    }

    /// Calculate the card width for the current number of columns.
    fn calculate_card_width(available_width: f32, columns: usize) -> f32 {
        // Total spacing between cards
        let total_spacing = CARD_SPACING * (columns as f32 - 1.0);
        let card_width = (available_width - total_spacing) / columns as f32;

        // Clamp to min/max bounds
        card_width.clamp(CARD_MIN_WIDTH, CARD_MAX_WIDTH)
    }

    /// Render the sessions in a responsive grid layout.
    fn render_sessions_grid(&self, ui: &mut egui::Ui) {
        // Scrollable area for the grid with smooth scrolling
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
            .show(ui, |ui| {
                let available_width = ui.available_width();
                let columns = Self::calculate_grid_columns(available_width);
                let card_width = Self::calculate_card_width(available_width, columns);

                // Create rows of cards with consistent spacing
                let mut session_iter = self.sessions.iter().peekable();
                while session_iter.peek().is_some() {
                    ui.horizontal(|ui| {
                        for _ in 0..columns {
                            if let Some(session) = session_iter.next() {
                                self.render_session_card(ui, session, card_width);
                                ui.add_space(spacing::LG);
                            }
                        }
                    });
                    ui.add_space(spacing::LG);
                }
            });
    }

    /// Render a single session card.
    ///
    /// The card displays:
    /// - Header: Project name, session badge (main/worktree), branch name
    /// - Status row: Colored indicator dot with state label
    /// - Progress row: Story progress (e.g., "Story 2 of 5"), current story ID
    /// - Duration row: Time elapsed since run started
    /// - Output section: Last 5 lines of Claude output in monospace font
    fn render_session_card(&self, ui: &mut egui::Ui, session: &SessionData, card_width: f32) {
        // Define card dimensions
        let card_size = Vec2::new(card_width, CARD_MIN_HEIGHT);

        // Allocate space for the card
        let (rect, _response) = ui.allocate_exact_size(card_size, Sense::hover());

        // Skip if not visible (optimization for scrolling)
        if !ui.is_rect_visible(rect) {
            return;
        }

        // Draw card background with elevation
        let card_rect = rect;
        let painter = ui.painter();

        // Shadow for subtle elevation
        let shadow = theme::shadow::subtle();
        let shadow_rect = Rect::from_min_size(
            card_rect.min + shadow.offset,
            card_rect.size() + Vec2::splat(shadow.spread * 2.0),
        );
        painter.rect_filled(
            shadow_rect.expand(shadow.blur / 2.0),
            Rounding::same(rounding::CARD),
            shadow.color,
        );

        // Card background
        painter.rect(
            card_rect,
            Rounding::same(rounding::CARD),
            colors::SURFACE,
            Stroke::new(1.0, colors::BORDER),
        );

        // Draw card content
        let content_rect = card_rect.shrink(CARD_PADDING);
        let mut cursor_y = content_rect.min.y;
        let content_width = content_rect.width();

        // ====================================================================
        // HEADER ROW: Project name and session badge
        // ====================================================================
        let project_name =
            truncate_with_ellipsis(&session.project_name, MAX_TEXT_LENGTH.saturating_sub(10));
        let project_galley = painter.layout_no_wrap(
            project_name,
            typography::font(FontSize::Body, FontWeight::SemiBold),
            colors::TEXT_PRIMARY,
        );
        painter.galley(
            egui::pos2(content_rect.min.x, cursor_y),
            project_galley.clone(),
            Color32::TRANSPARENT,
        );

        // Session badge (main/worktree ID) - positioned after project name
        let badge_text = if session.is_main_session {
            "main".to_string()
        } else {
            session.metadata.session_id.clone()
        };
        let badge_padding_h = 6.0; // Inner padding for badge
        let badge_padding_v = 2.0; // Inner padding for badge
        let badge_galley = painter.layout_no_wrap(
            badge_text,
            typography::font(FontSize::Caption, FontWeight::Medium),
            if session.is_main_session {
                colors::ACCENT
            } else {
                colors::TEXT_SECONDARY
            },
        );
        let badge_x = content_rect.min.x + project_galley.rect.width() + 8.0;
        let badge_bg_rect = Rect::from_min_size(
            egui::pos2(badge_x, cursor_y),
            egui::vec2(
                badge_galley.rect.width() + badge_padding_h * 2.0,
                badge_galley.rect.height() + badge_padding_v * 2.0,
            ),
        );
        let badge_bg_color = if session.is_main_session {
            colors::ACCENT_SUBTLE
        } else {
            colors::SURFACE_HOVER
        };
        painter.rect_filled(
            badge_bg_rect,
            Rounding::same(rounding::SMALL),
            badge_bg_color,
        );
        painter.galley(
            egui::pos2(badge_x + badge_padding_h, cursor_y + badge_padding_v),
            badge_galley,
            Color32::TRANSPARENT,
        );
        cursor_y += project_galley.rect.height() + spacing::XS;

        // Branch name row
        let branch_text = truncate_with_ellipsis(&session.metadata.branch_name, MAX_BRANCH_LENGTH);
        let branch_galley = painter.layout_no_wrap(
            branch_text,
            typography::font(FontSize::Caption, FontWeight::Regular),
            colors::TEXT_MUTED,
        );
        painter.galley(
            egui::pos2(content_rect.min.x, cursor_y),
            branch_galley.clone(),
            Color32::TRANSPARENT,
        );
        cursor_y += branch_galley.rect.height() + spacing::SM;

        // ====================================================================
        // STATUS ROW: Colored indicator dot with state label
        // ====================================================================
        let (state, state_color) = if let Some(ref run) = session.run {
            (run.machine_state, state_to_color(run.machine_state))
        } else {
            (MachineState::Idle, colors::STATUS_IDLE)
        };

        // Status dot
        let dot_radius = 4.0;
        let dot_center = egui::pos2(
            content_rect.min.x + dot_radius,
            cursor_y + FontSize::Caption.pixels() / 2.0,
        );
        painter.circle_filled(dot_center, dot_radius, state_color);

        // State text
        let state_text = format_state(state);
        let state_galley = painter.layout_no_wrap(
            state_text.to_string(),
            typography::font(FontSize::Caption, FontWeight::Medium),
            colors::TEXT_PRIMARY,
        );
        painter.galley(
            egui::pos2(
                content_rect.min.x + dot_radius * 2.0 + spacing::SM,
                cursor_y,
            ),
            state_galley.clone(),
            Color32::TRANSPARENT,
        );
        cursor_y += state_galley.rect.height() + spacing::XS;

        // ====================================================================
        // ERROR MESSAGE (if present)
        // ====================================================================
        if let Some(ref error) = session.load_error {
            let error_text = truncate_with_ellipsis(error, MAX_TEXT_LENGTH);
            let error_galley = painter.layout_no_wrap(
                error_text,
                typography::font(FontSize::Caption, FontWeight::Regular),
                colors::STATUS_ERROR,
            );
            painter.galley(
                egui::pos2(content_rect.min.x, cursor_y),
                error_galley.clone(),
                Color32::TRANSPARENT,
            );
            cursor_y += error_galley.rect.height() + spacing::XS;
        }

        // ====================================================================
        // PROGRESS ROW: Story progress and current story ID
        // ====================================================================
        if let Some(ref progress) = session.progress {
            let progress_text = progress.as_fraction();
            let progress_galley = painter.layout_no_wrap(
                progress_text,
                typography::font(FontSize::Caption, FontWeight::Regular),
                colors::TEXT_SECONDARY,
            );
            painter.galley(
                egui::pos2(content_rect.min.x, cursor_y),
                progress_galley.clone(),
                Color32::TRANSPARENT,
            );

            // Current story ID (if available)
            if let Some(ref run) = session.run {
                if let Some(ref story_id) = run.current_story {
                    let story_text = truncate_with_ellipsis(story_id, 15);
                    let story_galley = painter.layout_no_wrap(
                        story_text,
                        typography::font(FontSize::Caption, FontWeight::Regular),
                        colors::TEXT_MUTED,
                    );
                    painter.galley(
                        egui::pos2(
                            content_rect.min.x + progress_galley.rect.width() + spacing::MD,
                            cursor_y,
                        ),
                        story_galley,
                        Color32::TRANSPARENT,
                    );
                }
            }
            cursor_y += progress_galley.rect.height() + spacing::XS;
        }

        // ====================================================================
        // DURATION ROW: Time elapsed since run started
        // ====================================================================
        if let Some(ref run) = session.run {
            let duration_text = format_duration(run.started_at);
            let duration_galley = painter.layout_no_wrap(
                duration_text,
                typography::font(FontSize::Caption, FontWeight::Regular),
                colors::TEXT_MUTED,
            );
            painter.galley(
                egui::pos2(content_rect.min.x, cursor_y),
                duration_galley.clone(),
                Color32::TRANSPARENT,
            );
            cursor_y += duration_galley.rect.height() + spacing::SM;
        }

        // ====================================================================
        // OUTPUT SECTION: Last 5 lines of Claude output in monospace
        // ====================================================================
        // Draw a subtle separator line
        let separator_y = cursor_y;
        painter.hline(
            content_rect.x_range(),
            separator_y,
            Stroke::new(1.0, colors::BORDER),
        );
        cursor_y += spacing::SM;

        // Output section background
        let output_rect = Rect::from_min_max(
            egui::pos2(content_rect.min.x, cursor_y),
            egui::pos2(content_rect.max.x, content_rect.max.y),
        );
        painter.rect_filled(
            output_rect,
            Rounding::same(rounding::SMALL),
            colors::SURFACE_HOVER,
        );

        // Output lines with consistent padding
        let output_padding = 6.0; // Inner padding for output section
        let mut output_y = cursor_y + output_padding;
        let line_height = FontSize::Caption.pixels() + 2.0;
        let max_output_chars = ((content_width - output_padding * 2.0) / 6.0) as usize; // Approx chars per line

        if let Some(ref live_output) = session.live_output {
            // Get last OUTPUT_LINES_TO_SHOW lines
            let lines: Vec<_> = live_output
                .output_lines
                .iter()
                .rev()
                .take(OUTPUT_LINES_TO_SHOW)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();

            if lines.is_empty() {
                // No output yet
                let no_output_galley = painter.layout_no_wrap(
                    "Waiting for output...".to_string(),
                    typography::mono(FontSize::Caption),
                    colors::TEXT_DISABLED,
                );
                painter.galley(
                    egui::pos2(content_rect.min.x + output_padding, output_y),
                    no_output_galley,
                    Color32::TRANSPARENT,
                );
            } else {
                for line in lines {
                    let line_text = truncate_with_ellipsis(line.trim(), max_output_chars);
                    let line_galley = painter.layout_no_wrap(
                        line_text,
                        typography::mono(FontSize::Caption),
                        colors::TEXT_SECONDARY,
                    );
                    painter.galley(
                        egui::pos2(content_rect.min.x + output_padding, output_y),
                        line_galley,
                        Color32::TRANSPARENT,
                    );
                    output_y += line_height;

                    // Stop if we exceed the output area
                    if output_y > content_rect.max.y - output_padding {
                        break;
                    }
                }
            }
        } else {
            // No live output available
            let no_output_galley = painter.layout_no_wrap(
                "No live output".to_string(),
                typography::mono(FontSize::Caption),
                colors::TEXT_DISABLED,
            );
            painter.galley(
                egui::pos2(content_rect.min.x + output_padding, output_y),
                no_output_galley,
                Color32::TRANSPARENT,
            );
        }
    }

    // state_to_color is now imported from the components module.

    /// Render the Projects view with split layout.
    /// Left half shows the compact project list, right half is reserved for detail panel.
    fn render_projects(&mut self, ui: &mut egui::Ui) {
        // Use horizontal layout for split view
        let available_width = ui.available_width();
        let left_panel_width = (available_width / 2.0).max(200.0);

        ui.horizontal(|ui| {
            // Left panel: Project list (takes half the width)
            ui.allocate_ui_with_layout(
                Vec2::new(left_panel_width, ui.available_height()),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    self.render_projects_left_panel(ui);
                },
            );

            // Vertical separator between panels
            ui.add_space(spacing::SM);
            ui.separator();
            ui.add_space(spacing::SM);

            // Right panel: Reserved for future detail view (US-003)
            ui.allocate_ui_with_layout(
                Vec2::new(ui.available_width(), ui.available_height()),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    self.render_projects_right_panel(ui);
                },
            );
        });
    }

    /// Render the left panel of the Projects view (project list).
    fn render_projects_left_panel(&mut self, ui: &mut egui::Ui) {
        // Header section with consistent spacing
        ui.label(
            egui::RichText::new("Projects")
                .font(typography::font(FontSize::Title, FontWeight::SemiBold))
                .color(colors::TEXT_PRIMARY),
        );

        ui.add_space(spacing::SM);

        if let Some(ref filter) = self.project_filter {
            ui.label(
                egui::RichText::new(format!("Filtering by project: {}", filter))
                    .font(typography::font(FontSize::Body, FontWeight::Regular))
                    .color(colors::TEXT_SECONDARY),
            );
            ui.add_space(spacing::SM);
        }

        // Empty state or list
        if self.projects.is_empty() {
            self.render_empty_projects(ui);
        } else {
            self.render_projects_list(ui);
        }
    }

    /// Render the right panel of the Projects view.
    /// Shows hint text when no project is selected, or run history when selected.
    fn render_projects_right_panel(&self, ui: &mut egui::Ui) {
        if let Some(ref selected_name) = self.selected_project {
            // Header: Project name
            ui.label(
                egui::RichText::new(format!("Run History: {}", selected_name))
                    .font(typography::font(FontSize::Title, FontWeight::SemiBold))
                    .color(colors::TEXT_PRIMARY),
            );

            ui.add_space(spacing::MD);

            if self.run_history.is_empty() {
                // Empty state for no run history
                ui.add_space(spacing::LG);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("No run history")
                            .font(typography::font(FontSize::Body, FontWeight::Medium))
                            .color(colors::TEXT_MUTED),
                    );

                    ui.add_space(spacing::XS);

                    ui.label(
                        egui::RichText::new("Completed runs will appear here")
                            .font(typography::font(FontSize::Small, FontWeight::Regular))
                            .color(colors::TEXT_MUTED),
                    );
                });
            } else {
                // Scrollable run history list
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
                    .show(ui, |ui| {
                        for entry in &self.run_history {
                            self.render_run_history_entry(ui, entry);
                            ui.add_space(spacing::SM);
                        }
                    });
            }
        } else {
            // Empty state when no project is selected
            ui.add_space(spacing::XXL);
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new("Select a project")
                        .font(typography::font(FontSize::Heading, FontWeight::Medium))
                        .color(colors::TEXT_MUTED),
                );

                ui.add_space(spacing::SM);

                ui.label(
                    egui::RichText::new("Click on a project to view its run history")
                        .font(typography::font(FontSize::Body, FontWeight::Regular))
                        .color(colors::TEXT_MUTED),
                );
            });
        }
    }

    /// Render a single run history entry as a card.
    fn render_run_history_entry(&self, ui: &mut egui::Ui, entry: &RunHistoryEntry) {
        // Card background
        let available_width = ui.available_width();
        let card_height = 72.0; // Fixed height for history cards

        let (rect, _response) =
            ui.allocate_exact_size(Vec2::new(available_width, card_height), Sense::hover());

        // Draw card background
        ui.painter().rect_filled(
            rect,
            Rounding::same(rounding::CARD),
            colors::SURFACE_HOVER,
        );

        // Card content
        let inner_rect = rect.shrink(spacing::MD);
        let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(inner_rect).layout(egui::Layout::top_down(egui::Align::LEFT)));

        // Top row: Date/time and status
        child_ui.horizontal(|ui| {
            // Date/time (left)
            let datetime_text = entry.started_at.format("%Y-%m-%d %H:%M").to_string();
            ui.label(
                egui::RichText::new(datetime_text)
                    .font(typography::font(FontSize::Body, FontWeight::Medium))
                    .color(colors::TEXT_PRIMARY),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Status badge (right)
                let status_color = entry.status_color();
                let status_text = entry.status_text();

                // Draw status badge background
                let badge_galley = ui.fonts(|f| {
                    f.layout_no_wrap(
                        status_text.to_string(),
                        typography::font(FontSize::Small, FontWeight::Medium),
                        colors::TEXT_PRIMARY,
                    )
                });
                let badge_width = badge_galley.rect.width() + spacing::MD * 2.0;
                let badge_height = badge_galley.rect.height() + spacing::XS * 2.0;

                let (badge_rect, _) =
                    ui.allocate_exact_size(Vec2::new(badge_width, badge_height), Sense::hover());

                ui.painter().rect_filled(
                    badge_rect,
                    Rounding::same(rounding::SMALL),
                    status_color.gamma_multiply(0.2),
                );

                // Center the text in the badge
                let text_pos = badge_rect.center() - badge_galley.rect.center().to_vec2();
                ui.painter().galley(text_pos, badge_galley, status_color);
            });
        });

        child_ui.add_space(spacing::XS);

        // Bottom row: Story count and branch
        child_ui.horizontal(|ui| {
            // Story count
            ui.label(
                egui::RichText::new(entry.story_count_text())
                    .font(typography::font(FontSize::Small, FontWeight::Regular))
                    .color(colors::TEXT_SECONDARY),
            );

            ui.add_space(spacing::MD);

            // Branch name (truncated)
            let branch_display = truncate_with_ellipsis(&entry.branch, MAX_BRANCH_LENGTH);
            ui.label(
                egui::RichText::new(format!("⎇ {}", branch_display))
                    .font(typography::font(FontSize::Small, FontWeight::Regular))
                    .color(colors::TEXT_MUTED),
            );
        });
    }

    /// Render the empty state for Projects view.
    fn render_empty_projects(&self, ui: &mut egui::Ui) {
        ui.add_space(spacing::XXL);

        // Center the empty state message
        ui.vertical_centered(|ui| {
            ui.add_space(spacing::XXL + spacing::LG);

            ui.label(
                egui::RichText::new("No projects found")
                    .font(typography::font(FontSize::Heading, FontWeight::Medium))
                    .color(colors::TEXT_MUTED),
            );

            ui.add_space(spacing::SM);

            ui.label(
                egui::RichText::new("Projects will appear here after running autom8")
                    .font(typography::font(FontSize::Body, FontWeight::Regular))
                    .color(colors::TEXT_MUTED),
            );
        });
    }

    /// Render the projects list with scrolling.
    fn render_projects_list(&mut self, ui: &mut egui::Ui) {
        // Clone project names to avoid borrow issues when handling clicks
        let project_names: Vec<String> =
            self.projects.iter().map(|p| p.info.name.clone()).collect();
        let selected = self.selected_project.clone();

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
            .show(ui, |ui| {
                for (idx, project_name) in project_names.iter().enumerate() {
                    let project = &self.projects[idx];
                    let is_selected = selected.as_deref() == Some(project_name.as_str());
                    let clicked = self.render_project_row(ui, project, is_selected);
                    if clicked {
                        self.toggle_project_selection(project_name);
                    }
                    ui.add_space(spacing::XS);
                }
            });
    }

    /// Count active sessions for a given project name.
    fn count_active_sessions_for_project(&self, project_name: &str) -> usize {
        self.sessions
            .iter()
            .filter(|s| s.project_name == project_name && !s.is_stale)
            .count()
    }

    /// Get the project status indicator color.
    /// Green = running, Red = error, Gray = idle
    fn project_status_color(&self, project: &ProjectData) -> Color32 {
        if let Some(ref error) = project.load_error {
            // Has an error
            if !error.is_empty() {
                return colors::STATUS_ERROR;
            }
        }

        if project.info.has_active_run {
            colors::STATUS_RUNNING
        } else {
            colors::STATUS_IDLE
        }
    }

    /// Get the status text for a project.
    /// Returns "Running", "N sessions active", "Idle", or "Last run: X ago"
    fn project_status_text(&self, project: &ProjectData) -> String {
        // Check for errors first
        if let Some(ref error) = project.load_error {
            if !error.is_empty() {
                return truncate_with_ellipsis(error, 30);
            }
        }

        // Count active sessions for this project
        let active_count = self.count_active_sessions_for_project(&project.info.name);

        if active_count > 1 {
            format!("{} sessions active", active_count)
        } else if project.info.has_active_run || active_count == 1 {
            "Running".to_string()
        } else if let Some(last_run) = project.info.last_run_date {
            format!("Last run: {}", format_relative_time(last_run))
        } else {
            "Idle".to_string()
        }
    }

    /// Render a single project row.
    /// Returns true if the row was clicked.
    fn render_project_row(
        &self,
        ui: &mut egui::Ui,
        project: &ProjectData,
        is_selected: bool,
    ) -> bool {
        let row_size = Vec2::new(ui.available_width(), PROJECT_ROW_HEIGHT);

        // Allocate space for the row with click interaction
        let (rect, response) = ui.allocate_exact_size(row_size, Sense::click());

        // Skip if not visible (optimization for scrolling)
        if !ui.is_rect_visible(rect) {
            return false;
        }

        let painter = ui.painter();
        let is_hovered = response.hovered();
        let was_clicked = response.clicked();

        // Draw row background with hover and selected states
        let bg_color = if is_selected {
            colors::SURFACE_SELECTED
        } else if is_hovered {
            colors::SURFACE_HOVER
        } else {
            colors::SURFACE
        };

        // Use accent color border for selected state, stronger border for hover
        let border_color = if is_selected {
            colors::ACCENT
        } else if is_hovered {
            colors::BORDER_FOCUSED
        } else {
            colors::BORDER
        };

        let border_width = if is_selected { 2.0 } else { 1.0 };

        painter.rect(
            rect,
            Rounding::same(rounding::BUTTON),
            bg_color,
            Stroke::new(border_width, border_color),
        );

        // Content layout within the row
        let content_rect = rect.shrink2(Vec2::new(PROJECT_ROW_PADDING_H, PROJECT_ROW_PADDING_V));
        let mut cursor_x = content_rect.min.x;
        let center_y = content_rect.center().y;

        // ====================================================================
        // STATUS INDICATOR DOT
        // ====================================================================
        let status_color = self.project_status_color(project);
        let dot_center = egui::pos2(cursor_x + PROJECT_STATUS_DOT_RADIUS, center_y);
        painter.circle_filled(dot_center, PROJECT_STATUS_DOT_RADIUS, status_color);
        cursor_x += PROJECT_STATUS_DOT_RADIUS * 2.0 + spacing::MD;

        // ====================================================================
        // PROJECT NAME
        // ====================================================================
        let name_text = truncate_with_ellipsis(&project.info.name, 30);
        let name_galley = painter.layout_no_wrap(
            name_text,
            typography::font(FontSize::Body, FontWeight::SemiBold),
            colors::TEXT_PRIMARY,
        );
        let name_y = center_y - name_galley.rect.height() / 2.0 - 6.0;
        painter.galley(
            egui::pos2(cursor_x, name_y),
            name_galley.clone(),
            Color32::TRANSPARENT,
        );

        // ====================================================================
        // STATUS TEXT (below project name)
        // ====================================================================
        let status_text = self.project_status_text(project);
        let status_text_color = if project.load_error.is_some() {
            colors::STATUS_ERROR
        } else if project.info.has_active_run
            || self.count_active_sessions_for_project(&project.info.name) > 0
        {
            colors::STATUS_RUNNING
        } else {
            colors::TEXT_MUTED
        };
        let status_galley = painter.layout_no_wrap(
            status_text,
            typography::font(FontSize::Caption, FontWeight::Regular),
            status_text_color,
        );
        let status_y = name_y + name_galley.rect.height() + spacing::XS;
        painter.galley(
            egui::pos2(cursor_x, status_y),
            status_galley,
            Color32::TRANSPARENT,
        );

        // ====================================================================
        // LAST ACTIVITY (right-aligned)
        // ====================================================================
        if let Some(last_run) = project.info.last_run_date {
            let activity_text = format_relative_time(last_run);
            let activity_galley = painter.layout_no_wrap(
                activity_text,
                typography::font(FontSize::Caption, FontWeight::Regular),
                colors::TEXT_MUTED,
            );
            let activity_x = content_rect.max.x - activity_galley.rect.width();
            let activity_y = center_y - activity_galley.rect.height() / 2.0;
            painter.galley(
                egui::pos2(activity_x, activity_y),
                activity_galley,
                Color32::TRANSPARENT,
            );
        }

        was_clicked
    }
}

/// Launch the native GUI application.
///
/// Opens a native window using eframe with the specified configuration.
///
/// # Arguments
///
/// * `project_filter` - Optional project name to filter the view
///
/// # Returns
///
/// * `Ok(())` when the user closes the window
/// * `Err(Autom8Error)` if the GUI fails to initialize
pub fn run_gui(project_filter: Option<String>) -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("autom8")
            .with_inner_size([DEFAULT_WIDTH, DEFAULT_HEIGHT])
            .with_min_inner_size([MIN_WIDTH, MIN_HEIGHT]),
        ..Default::default()
    };

    eframe::run_native(
        "autom8",
        options,
        Box::new(|cc| {
            // Initialize custom typography (fonts and text styles)
            typography::init(&cc.egui_ctx);
            // Initialize theme (colors, visuals, and style)
            theme::init(&cc.egui_ctx);
            Ok(Box::new(Autom8App::new(project_filter)))
        }),
    )
    .map_err(|e| Autom8Error::GuiError(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_tab_default_is_active_runs() {
        assert_eq!(Tab::default(), Tab::ActiveRuns);
    }

    #[test]
    fn test_tab_labels() {
        assert_eq!(Tab::ActiveRuns.label(), "Active Runs");
        assert_eq!(Tab::Projects.label(), "Projects");
    }

    #[test]
    fn test_tab_all_returns_all_tabs() {
        let all = Tab::all();
        assert_eq!(all.len(), 2);
        assert!(all.contains(&Tab::ActiveRuns));
        assert!(all.contains(&Tab::Projects));
    }

    #[test]
    fn test_tab_equality() {
        assert_eq!(Tab::ActiveRuns, Tab::ActiveRuns);
        assert_eq!(Tab::Projects, Tab::Projects);
        assert_ne!(Tab::ActiveRuns, Tab::Projects);
    }

    #[test]
    fn test_tab_copy() {
        let tab = Tab::Projects;
        let copied = tab;
        assert_eq!(tab, copied);
    }

    #[test]
    fn test_autom8_app_new_defaults_to_active_runs() {
        let app = Autom8App::new(None);
        assert_eq!(app.current_tab(), Tab::ActiveRuns);
    }

    #[test]
    fn test_autom8_app_new_with_filter() {
        let app = Autom8App::new(Some("test-project".to_string()));
        assert_eq!(app.project_filter, Some("test-project".to_string()));
        assert_eq!(app.current_tab(), Tab::ActiveRuns);
    }

    #[test]
    fn test_autom8_app_new_without_filter() {
        let app = Autom8App::new(None);
        assert_eq!(app.project_filter, None);
    }

    #[test]
    fn test_header_height_is_reasonable() {
        assert!(HEADER_HEIGHT >= 40.0);
        assert!(HEADER_HEIGHT <= 60.0);
    }

    #[test]
    fn test_tab_underline_is_subtle() {
        assert!(TAB_UNDERLINE_HEIGHT >= 1.0);
        assert!(TAB_UNDERLINE_HEIGHT <= 4.0);
    }

    // ========================================================================
    // Data Layer Tests
    // ========================================================================

    #[test]
    fn test_default_refresh_interval() {
        assert_eq!(DEFAULT_REFRESH_INTERVAL_MS, 500);
    }

    #[test]
    fn test_app_with_custom_refresh_interval() {
        let interval = Duration::from_millis(100);
        let app = Autom8App::with_refresh_interval(None, interval);
        assert_eq!(app.refresh_interval(), interval);
    }

    #[test]
    fn test_app_set_refresh_interval() {
        let mut app = Autom8App::new(None);
        let new_interval = Duration::from_millis(1000);
        app.set_refresh_interval(new_interval);
        assert_eq!(app.refresh_interval(), new_interval);
    }

    #[test]
    fn test_app_initializes_with_empty_data() {
        let app = Autom8App::new(Some("nonexistent-project".to_string()));
        assert!(
            app.projects().is_empty()
                || !app
                    .projects()
                    .iter()
                    .any(|p| p.info.name == "nonexistent-project")
        );
        assert!(
            app.sessions().is_empty()
                || !app
                    .sessions()
                    .iter()
                    .any(|s| s.project_name == "nonexistent-project")
        );
    }

    #[test]
    fn test_app_has_active_runs_initially_false() {
        let app = Autom8App::new(Some("nonexistent-project".to_string()));
        // With a nonexistent filter, there should be no active runs
        assert!(!app.has_active_runs() || app.sessions().is_empty());
    }

    #[test]
    fn test_app_initial_load_complete() {
        let app = Autom8App::new(Some("nonexistent-project".to_string()));
        // After creation, initial load should be complete
        assert!(app.is_initial_load_complete());
    }

    #[test]
    fn test_run_progress_as_fraction() {
        let progress = RunProgress {
            completed: 1,
            total: 5,
        };
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
            total: 4,
        };
        assert_eq!(progress.as_percentage(), "50%");
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
        let started_at = Utc::now() - chrono::Duration::seconds(30);
        let formatted = format_duration(started_at);
        assert!(formatted.ends_with('s'));
        assert!(!formatted.contains('m'));
    }

    #[test]
    fn test_format_duration_minutes_and_seconds() {
        let started_at = Utc::now() - chrono::Duration::seconds(125); // 2m 5s
        let formatted = format_duration(started_at);
        assert!(formatted.contains('m'));
        assert!(formatted.contains('s'));
    }

    #[test]
    fn test_format_duration_hours_and_minutes() {
        let started_at = Utc::now() - chrono::Duration::seconds(3700); // 1h 1m 40s
        let formatted = format_duration(started_at);
        assert!(formatted.contains('h'));
        assert!(formatted.contains('m'));
        assert!(!formatted.contains('s'));
    }

    #[test]
    fn test_format_relative_time_just_now() {
        let timestamp = Utc::now() - chrono::Duration::seconds(30);
        let formatted = format_relative_time(timestamp);
        assert_eq!(formatted, "just now");
    }

    #[test]
    fn test_format_relative_time_minutes() {
        let timestamp = Utc::now() - chrono::Duration::minutes(5);
        let formatted = format_relative_time(timestamp);
        assert!(formatted.contains("5m ago"));
    }

    #[test]
    fn test_format_relative_time_hours() {
        let timestamp = Utc::now() - chrono::Duration::hours(3);
        let formatted = format_relative_time(timestamp);
        assert!(formatted.contains("3h ago"));
    }

    #[test]
    fn test_format_relative_time_days() {
        let timestamp = Utc::now() - chrono::Duration::days(2);
        let formatted = format_relative_time(timestamp);
        assert!(formatted.contains("2d ago"));
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
    fn test_session_data_display_title_main() {
        let session = SessionData {
            project_name: "my-project".to_string(),
            metadata: SessionMetadata {
                session_id: "main".to_string(),
                worktree_path: PathBuf::from("/path/to/project"),
                branch_name: "feature/test".to_string(),
                created_at: Utc::now(),
                last_active_at: Utc::now(),
                is_running: true,
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
        let session = SessionData {
            project_name: "my-project".to_string(),
            metadata: SessionMetadata {
                session_id: "abc12345".to_string(),
                worktree_path: PathBuf::from("/path/to/project-wt-feature"),
                branch_name: "feature/test".to_string(),
                created_at: Utc::now(),
                last_active_at: Utc::now(),
                is_running: true,
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
        let session = SessionData {
            project_name: "my-project".to_string(),
            metadata: SessionMetadata {
                session_id: "abc12345".to_string(),
                worktree_path: PathBuf::from("project"),
                branch_name: "feature/test".to_string(),
                created_at: Utc::now(),
                last_active_at: Utc::now(),
                is_running: true,
            },
            run: None,
            progress: None,
            load_error: None,
            is_main_session: false,
            is_stale: false,
            live_output: None,
        };
        assert_eq!(session.truncated_worktree_path(), "project");
    }

    #[test]
    fn test_session_data_truncated_worktree_path_long() {
        let session = SessionData {
            project_name: "my-project".to_string(),
            metadata: SessionMetadata {
                session_id: "abc12345".to_string(),
                worktree_path: PathBuf::from("/Users/dev/projects/my-project-wt-feature"),
                branch_name: "feature/test".to_string(),
                created_at: Utc::now(),
                last_active_at: Utc::now(),
                is_running: true,
            },
            run: None,
            progress: None,
            load_error: None,
            is_main_session: false,
            is_stale: false,
            live_output: None,
        };
        let truncated = session.truncated_worktree_path();
        assert!(truncated.starts_with("..."));
        assert!(truncated.contains("my-project-wt-feature"));
    }

    // ========================================================================
    // Grid Layout Tests
    // ========================================================================

    #[test]
    fn test_calculate_grid_columns_narrow_window() {
        // Narrow window should show 2 columns (minimum)
        let columns = Autom8App::calculate_grid_columns(500.0);
        assert_eq!(columns, 2);
    }

    #[test]
    fn test_calculate_grid_columns_medium_window() {
        // Medium window around 900px should show 3 columns
        let columns = Autom8App::calculate_grid_columns(900.0);
        assert_eq!(columns, 3);
    }

    #[test]
    fn test_calculate_grid_columns_wide_window() {
        // Wide window should show 4 columns (maximum)
        let columns = Autom8App::calculate_grid_columns(1400.0);
        assert_eq!(columns, 4);
    }

    #[test]
    fn test_calculate_grid_columns_very_narrow() {
        // Very narrow window should still show minimum 2 columns
        let columns = Autom8App::calculate_grid_columns(300.0);
        assert_eq!(columns, 2);
    }

    #[test]
    fn test_calculate_grid_columns_very_wide() {
        // Very wide window should cap at 4 columns
        let columns = Autom8App::calculate_grid_columns(2000.0);
        assert_eq!(columns, 4);
    }

    #[test]
    fn test_calculate_card_width_two_columns() {
        // With 2 columns and 600px width, cards should be reasonable size
        let card_width = Autom8App::calculate_card_width(600.0, 2);
        // (600 - 16) / 2 = 292
        assert!(card_width >= CARD_MIN_WIDTH);
        assert!(card_width <= CARD_MAX_WIDTH);
    }

    #[test]
    fn test_calculate_card_width_four_columns() {
        // With 4 columns and 1200px width
        let card_width = Autom8App::calculate_card_width(1200.0, 4);
        // (1200 - 48) / 4 = 288
        assert!(card_width >= CARD_MIN_WIDTH);
        assert!(card_width <= CARD_MAX_WIDTH);
    }

    #[test]
    fn test_calculate_card_width_clamps_to_min() {
        // Very narrow width should clamp to minimum card width
        let card_width = Autom8App::calculate_card_width(400.0, 4);
        assert_eq!(card_width, CARD_MIN_WIDTH);
    }

    #[test]
    fn test_calculate_card_width_clamps_to_max() {
        // Very wide with few columns should clamp to maximum
        let card_width = Autom8App::calculate_card_width(1000.0, 2);
        // (1000 - 16) / 2 = 492, which exceeds max of 400
        assert_eq!(card_width, CARD_MAX_WIDTH);
    }

    #[test]
    fn test_grid_constants_are_reasonable() {
        assert!(
            CARD_MIN_WIDTH > 200.0,
            "Cards should be at least 200px wide"
        );
        assert!(
            CARD_MAX_WIDTH > CARD_MIN_WIDTH,
            "Max width should exceed min width"
        );
        assert!(CARD_SPACING >= 8.0, "Cards need adequate spacing");
        assert!(CARD_PADDING >= 12.0, "Cards need internal padding");
        assert!(CARD_MIN_HEIGHT >= 80.0, "Cards need minimum height");
    }

    #[test]
    fn test_state_to_color_running_states() {
        // state_to_color is now a standalone function from components module
        assert_eq!(
            state_to_color(MachineState::RunningClaude),
            colors::STATUS_RUNNING
        );
        assert_eq!(
            state_to_color(MachineState::Reviewing),
            colors::STATUS_RUNNING
        );
        assert_eq!(
            state_to_color(MachineState::Correcting),
            colors::STATUS_RUNNING
        );
        assert_eq!(
            state_to_color(MachineState::Committing),
            colors::STATUS_RUNNING
        );
        assert_eq!(
            state_to_color(MachineState::CreatingPR),
            colors::STATUS_RUNNING
        );
        assert_eq!(
            state_to_color(MachineState::Initializing),
            colors::STATUS_RUNNING
        );
        assert_eq!(
            state_to_color(MachineState::PickingStory),
            colors::STATUS_RUNNING
        );
        assert_eq!(
            state_to_color(MachineState::LoadingSpec),
            colors::STATUS_RUNNING
        );
        assert_eq!(
            state_to_color(MachineState::GeneratingSpec),
            colors::STATUS_RUNNING
        );
    }

    #[test]
    fn test_state_to_color_terminal_states() {
        // state_to_color is now a standalone function from components module
        assert_eq!(
            state_to_color(MachineState::Completed),
            colors::STATUS_SUCCESS
        );
        assert_eq!(state_to_color(MachineState::Failed), colors::STATUS_ERROR);
        assert_eq!(state_to_color(MachineState::Idle), colors::STATUS_IDLE);
    }

    // ========================================================================
    // Session Card Tests
    // ========================================================================

    #[test]
    fn test_truncate_with_ellipsis_short_string() {
        let result = truncate_with_ellipsis("short", 10);
        assert_eq!(result, "short");
    }

    #[test]
    fn test_truncate_with_ellipsis_exact_length() {
        let result = truncate_with_ellipsis("exactly10!", 10);
        assert_eq!(result, "exactly10!");
    }

    #[test]
    fn test_truncate_with_ellipsis_long_string() {
        let result = truncate_with_ellipsis("this is a very long string", 15);
        assert_eq!(result, "this is a ve...");
        assert_eq!(result.len(), 15);
    }

    #[test]
    fn test_truncate_with_ellipsis_very_short_max() {
        // When max_len <= 3, just truncate without ellipsis
        let result = truncate_with_ellipsis("hello", 3);
        assert_eq!(result, "hel");
    }

    #[test]
    fn test_truncate_with_ellipsis_empty_string() {
        let result = truncate_with_ellipsis("", 10);
        assert_eq!(result, "");
    }

    #[test]
    fn test_truncate_with_ellipsis_max_text_length() {
        let long_text = "a".repeat(50);
        let result = truncate_with_ellipsis(&long_text, MAX_TEXT_LENGTH);
        assert_eq!(result.len(), MAX_TEXT_LENGTH);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_truncate_with_ellipsis_max_branch_length() {
        let long_branch = "feature/very-long-branch-name-that-exceeds-limit";
        let result = truncate_with_ellipsis(long_branch, MAX_BRANCH_LENGTH);
        assert_eq!(result.len(), MAX_BRANCH_LENGTH);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_card_constants_for_session_card() {
        // Card height should be sufficient for all content
        assert!(
            CARD_MIN_HEIGHT >= 200.0,
            "Card height should accommodate header, status, progress, duration, and output"
        );

        // Output lines count should be reasonable
        assert!(OUTPUT_LINES_TO_SHOW >= 3 && OUTPUT_LINES_TO_SHOW <= 10);

        // Max text length for truncation
        assert!(MAX_TEXT_LENGTH >= 20 && MAX_TEXT_LENGTH <= 60);

        // Max branch length for truncation
        assert!(MAX_BRANCH_LENGTH >= 15 && MAX_BRANCH_LENGTH <= 40);
    }

    #[test]
    fn test_session_data_with_live_output() {
        let live_output = LiveState {
            output_lines: vec![
                "Line 1".to_string(),
                "Line 2".to_string(),
                "Line 3".to_string(),
            ],
            updated_at: Utc::now(),
            machine_state: MachineState::RunningClaude,
        };

        let session = SessionData {
            project_name: "test-project".to_string(),
            metadata: SessionMetadata {
                session_id: "main".to_string(),
                worktree_path: PathBuf::from("/path/to/project"),
                branch_name: "feature/test".to_string(),
                created_at: Utc::now(),
                last_active_at: Utc::now(),
                is_running: true,
            },
            run: None,
            progress: Some(RunProgress {
                completed: 2,
                total: 5,
            }),
            load_error: None,
            is_main_session: true,
            is_stale: false,
            live_output: Some(live_output),
        };

        assert!(session.live_output.is_some());
        assert_eq!(session.live_output.as_ref().unwrap().output_lines.len(), 3);
    }

    #[test]
    fn test_session_data_with_run_state() {
        let run = RunState::new(PathBuf::from("/spec.json"), "feature/test".to_string());

        let session = SessionData {
            project_name: "test-project".to_string(),
            metadata: SessionMetadata {
                session_id: "abc12345".to_string(),
                worktree_path: PathBuf::from("/path/to/worktree"),
                branch_name: "feature/test".to_string(),
                created_at: Utc::now(),
                last_active_at: Utc::now(),
                is_running: true,
            },
            run: Some(run),
            progress: Some(RunProgress {
                completed: 1,
                total: 3,
            }),
            load_error: None,
            is_main_session: false,
            is_stale: false,
            live_output: None,
        };

        assert!(session.run.is_some());
        assert!(!session.is_main_session);
        assert_eq!(
            session.progress.as_ref().unwrap().as_fraction(),
            "Story 2/3"
        );
    }

    #[test]
    fn test_session_data_with_error() {
        let session = SessionData {
            project_name: "test-project".to_string(),
            metadata: SessionMetadata {
                session_id: "main".to_string(),
                worktree_path: PathBuf::from("/deleted/path"),
                branch_name: "feature/broken".to_string(),
                created_at: Utc::now(),
                last_active_at: Utc::now(),
                is_running: true,
            },
            run: None,
            progress: None,
            load_error: Some("Corrupted state file".to_string()),
            is_main_session: true,
            is_stale: true,
            live_output: None,
        };

        assert!(session.load_error.is_some());
        assert!(session.is_stale);
        assert_eq!(session.load_error.as_ref().unwrap(), "Corrupted state file");
    }

    #[test]
    fn test_long_branch_name_truncation() {
        let branch = "feature/US-007-implement-session-card-component-with-all-details";
        let truncated = truncate_with_ellipsis(branch, MAX_BRANCH_LENGTH);
        assert!(truncated.len() <= MAX_BRANCH_LENGTH);
        assert!(truncated.ends_with("..."));
    }

    #[test]
    fn test_long_project_name_truncation() {
        let project = "my-very-long-project-name-that-exceeds-limits";
        let truncated = truncate_with_ellipsis(project, MAX_TEXT_LENGTH.saturating_sub(10));
        assert!(truncated.len() <= MAX_TEXT_LENGTH.saturating_sub(10));
    }

    // ========================================================================
    // Projects View Tests
    // ========================================================================

    #[test]
    fn test_project_row_constants() {
        // Row height should accommodate two lines of text plus padding
        assert!(
            PROJECT_ROW_HEIGHT >= 40.0 && PROJECT_ROW_HEIGHT <= 80.0,
            "Row height should be reasonable for two-line content"
        );

        // Padding values should be reasonable
        assert!(PROJECT_ROW_PADDING_H >= 8.0 && PROJECT_ROW_PADDING_H <= 20.0);
        assert!(PROJECT_ROW_PADDING_V >= 8.0 && PROJECT_ROW_PADDING_V <= 20.0);

        // Status dot should be visible but not too large
        assert!(PROJECT_STATUS_DOT_RADIUS >= 3.0 && PROJECT_STATUS_DOT_RADIUS <= 8.0);
    }

    #[test]
    fn test_count_active_sessions_for_project_empty() {
        let app = Autom8App::new(Some("nonexistent".to_string()));
        let count = app.count_active_sessions_for_project("test-project");
        assert_eq!(count, 0);
    }

    #[test]
    fn test_project_status_color_running() {
        let app = Autom8App::new(Some("nonexistent".to_string()));
        let project = ProjectData {
            info: ProjectTreeInfo {
                name: "test".to_string(),
                has_active_run: true,
                run_status: Some(crate::state::RunStatus::Running),
                spec_count: 1,
                incomplete_spec_count: 0,
                spec_md_count: 0,
                runs_count: 0,
                last_run_date: None,
            },
            active_run: None,
            progress: None,
            load_error: None,
        };
        assert_eq!(app.project_status_color(&project), colors::STATUS_RUNNING);
    }

    #[test]
    fn test_project_status_color_idle() {
        let app = Autom8App::new(Some("nonexistent".to_string()));
        let project = ProjectData {
            info: ProjectTreeInfo {
                name: "test".to_string(),
                has_active_run: false,
                run_status: None,
                spec_count: 1,
                incomplete_spec_count: 0,
                spec_md_count: 0,
                runs_count: 1,
                last_run_date: Some(Utc::now()),
            },
            active_run: None,
            progress: None,
            load_error: None,
        };
        assert_eq!(app.project_status_color(&project), colors::STATUS_IDLE);
    }

    #[test]
    fn test_project_status_color_error() {
        let app = Autom8App::new(Some("nonexistent".to_string()));
        let project = ProjectData {
            info: ProjectTreeInfo {
                name: "test".to_string(),
                has_active_run: false,
                run_status: None,
                spec_count: 1,
                incomplete_spec_count: 0,
                spec_md_count: 0,
                runs_count: 0,
                last_run_date: None,
            },
            active_run: None,
            progress: None,
            load_error: Some("State corrupted".to_string()),
        };
        assert_eq!(app.project_status_color(&project), colors::STATUS_ERROR);
    }

    #[test]
    fn test_project_status_text_running() {
        let app = Autom8App::new(Some("nonexistent".to_string()));
        let project = ProjectData {
            info: ProjectTreeInfo {
                name: "test".to_string(),
                has_active_run: true,
                run_status: Some(crate::state::RunStatus::Running),
                spec_count: 1,
                incomplete_spec_count: 0,
                spec_md_count: 0,
                runs_count: 0,
                last_run_date: None,
            },
            active_run: None,
            progress: None,
            load_error: None,
        };
        assert_eq!(app.project_status_text(&project), "Running");
    }

    #[test]
    fn test_project_status_text_idle() {
        let app = Autom8App::new(Some("nonexistent".to_string()));
        let project = ProjectData {
            info: ProjectTreeInfo {
                name: "test".to_string(),
                has_active_run: false,
                run_status: None,
                spec_count: 1,
                incomplete_spec_count: 0,
                spec_md_count: 0,
                runs_count: 0,
                last_run_date: None,
            },
            active_run: None,
            progress: None,
            load_error: None,
        };
        assert_eq!(app.project_status_text(&project), "Idle");
    }

    #[test]
    fn test_project_status_text_with_last_run() {
        let app = Autom8App::new(Some("nonexistent".to_string()));
        let last_run = Utc::now() - chrono::Duration::hours(2);
        let project = ProjectData {
            info: ProjectTreeInfo {
                name: "test".to_string(),
                has_active_run: false,
                run_status: None,
                spec_count: 1,
                incomplete_spec_count: 0,
                spec_md_count: 0,
                runs_count: 1,
                last_run_date: Some(last_run),
            },
            active_run: None,
            progress: None,
            load_error: None,
        };
        let status = app.project_status_text(&project);
        assert!(status.starts_with("Last run:"));
        assert!(status.contains("2h ago"));
    }

    #[test]
    fn test_project_status_text_with_error() {
        let app = Autom8App::new(Some("nonexistent".to_string()));
        let project = ProjectData {
            info: ProjectTreeInfo {
                name: "test".to_string(),
                has_active_run: false,
                run_status: None,
                spec_count: 1,
                incomplete_spec_count: 0,
                spec_md_count: 0,
                runs_count: 0,
                last_run_date: None,
            },
            active_run: None,
            progress: None,
            load_error: Some("Corrupted state file".to_string()),
        };
        let status = app.project_status_text(&project);
        assert!(status.contains("Corrupted"));
    }

    #[test]
    fn test_project_data_fields() {
        let project = ProjectData {
            info: ProjectTreeInfo {
                name: "my-project".to_string(),
                has_active_run: false,
                run_status: None,
                spec_count: 3,
                incomplete_spec_count: 1,
                spec_md_count: 2,
                runs_count: 5,
                last_run_date: Some(Utc::now()),
            },
            active_run: None,
            progress: Some(RunProgress {
                completed: 2,
                total: 5,
            }),
            load_error: None,
        };

        assert_eq!(project.info.name, "my-project");
        assert_eq!(project.info.spec_count, 3);
        assert!(project.progress.is_some());
        assert!(project.load_error.is_none());
    }

    #[test]
    fn test_project_tree_info_status_labels() {
        // Running status
        let running = ProjectTreeInfo {
            name: "test".to_string(),
            has_active_run: true,
            run_status: Some(crate::state::RunStatus::Running),
            spec_count: 1,
            incomplete_spec_count: 0,
            spec_md_count: 0,
            runs_count: 0,
            last_run_date: None,
        };
        assert_eq!(running.status_label(), "running");

        // Failed status
        let failed = ProjectTreeInfo {
            name: "test".to_string(),
            has_active_run: false,
            run_status: Some(crate::state::RunStatus::Failed),
            spec_count: 1,
            incomplete_spec_count: 0,
            spec_md_count: 0,
            runs_count: 0,
            last_run_date: None,
        };
        assert_eq!(failed.status_label(), "failed");

        // Incomplete status
        let incomplete = ProjectTreeInfo {
            name: "test".to_string(),
            has_active_run: false,
            run_status: None,
            spec_count: 1,
            incomplete_spec_count: 1,
            spec_md_count: 0,
            runs_count: 0,
            last_run_date: None,
        };
        assert_eq!(incomplete.status_label(), "incomplete");
    }

    #[test]
    fn test_projects_empty_state_message() {
        let app = Autom8App::new(Some("definitely-not-a-project".to_string()));
        // With an impossible filter, projects should be empty
        // The empty state message should be "No projects found"
        assert!(
            app.projects().is_empty()
                || !app
                    .projects()
                    .iter()
                    .any(|p| p.info.name == "definitely-not-a-project")
        );
    }

    // ========================================================================
    // Project Selection Tests (US-002)
    // ========================================================================

    #[test]
    fn test_selected_project_initially_none() {
        let app = Autom8App::new(None);
        assert!(app.selected_project().is_none());
    }

    #[test]
    fn test_toggle_project_selection_select() {
        let mut app = Autom8App::new(None);
        assert!(app.selected_project().is_none());

        app.toggle_project_selection("my-project");
        assert_eq!(app.selected_project(), Some("my-project"));
    }

    #[test]
    fn test_toggle_project_selection_deselect() {
        let mut app = Autom8App::new(None);
        app.toggle_project_selection("my-project");
        assert_eq!(app.selected_project(), Some("my-project"));

        // Toggle again to deselect
        app.toggle_project_selection("my-project");
        assert!(app.selected_project().is_none());
    }

    #[test]
    fn test_toggle_project_selection_switch() {
        let mut app = Autom8App::new(None);
        app.toggle_project_selection("project-a");
        assert_eq!(app.selected_project(), Some("project-a"));

        // Select a different project
        app.toggle_project_selection("project-b");
        assert_eq!(app.selected_project(), Some("project-b"));
    }

    #[test]
    fn test_is_project_selected_true() {
        let mut app = Autom8App::new(None);
        app.toggle_project_selection("test-project");
        assert!(app.is_project_selected("test-project"));
    }

    #[test]
    fn test_is_project_selected_false() {
        let mut app = Autom8App::new(None);
        app.toggle_project_selection("project-a");
        assert!(!app.is_project_selected("project-b"));
    }

    #[test]
    fn test_is_project_selected_none() {
        let app = Autom8App::new(None);
        assert!(!app.is_project_selected("any-project"));
    }

    #[test]
    fn test_selected_project_empty_string() {
        let mut app = Autom8App::new(None);
        app.toggle_project_selection("");
        assert_eq!(app.selected_project(), Some(""));

        // Toggle again to deselect
        app.toggle_project_selection("");
        assert!(app.selected_project().is_none());
    }

    // ========================================================================
    // Run History Tests (US-003)
    // ========================================================================

    #[test]
    fn test_run_history_initially_empty() {
        let app = Autom8App::new(None);
        assert!(app.run_history().is_empty());
    }

    #[test]
    fn test_run_history_entry_from_run_state() {
        use crate::state::{RunState, RunStatus};

        let mut run = RunState::new(
            std::path::PathBuf::from("test.json"),
            "feature/test".to_string(),
        );
        run.status = RunStatus::Completed;

        let entry = RunHistoryEntry::from_run_state(&run);
        assert_eq!(entry.run_id, run.run_id);
        assert_eq!(entry.branch, "feature/test");
        assert_eq!(entry.status, RunStatus::Completed);
        assert_eq!(entry.completed_stories, 0);
    }

    #[test]
    fn test_run_history_entry_story_count_text() {
        use crate::state::{IterationRecord, IterationStatus, RunState, RunStatus};
        use chrono::Utc;

        let mut run = RunState::new(
            std::path::PathBuf::from("test.json"),
            "feature/test".to_string(),
        );
        run.status = RunStatus::Completed;

        // Add some iterations
        run.iterations.push(IterationRecord {
            number: 1,
            story_id: "US-001".to_string(),
            started_at: Utc::now(),
            finished_at: Some(Utc::now()),
            status: IterationStatus::Success,
            output_snippet: String::new(),
            work_summary: None,
        });
        run.iterations.push(IterationRecord {
            number: 2,
            story_id: "US-002".to_string(),
            started_at: Utc::now(),
            finished_at: Some(Utc::now()),
            status: IterationStatus::Success,
            output_snippet: String::new(),
            work_summary: None,
        });
        run.iterations.push(IterationRecord {
            number: 3,
            story_id: "US-003".to_string(),
            started_at: Utc::now(),
            finished_at: None,
            status: IterationStatus::Failed,
            output_snippet: String::new(),
            work_summary: None,
        });

        let entry = RunHistoryEntry::from_run_state(&run);
        assert_eq!(entry.completed_stories, 2);
        assert_eq!(entry.total_stories, 3);
        assert_eq!(entry.story_count_text(), "2/3 stories");
    }

    #[test]
    fn test_run_history_entry_status_text() {
        use crate::state::{RunState, RunStatus};

        let mut run = RunState::new(
            std::path::PathBuf::from("test.json"),
            "feature/test".to_string(),
        );

        run.status = RunStatus::Completed;
        let entry = RunHistoryEntry::from_run_state(&run);
        assert_eq!(entry.status_text(), "Completed");

        run.status = RunStatus::Failed;
        let entry = RunHistoryEntry::from_run_state(&run);
        assert_eq!(entry.status_text(), "Failed");

        run.status = RunStatus::Running;
        let entry = RunHistoryEntry::from_run_state(&run);
        assert_eq!(entry.status_text(), "Running");
    }

    #[test]
    fn test_run_history_entry_status_color() {
        use crate::gui::theme::colors;
        use crate::state::{RunState, RunStatus};

        let mut run = RunState::new(
            std::path::PathBuf::from("test.json"),
            "feature/test".to_string(),
        );

        run.status = RunStatus::Completed;
        let entry = RunHistoryEntry::from_run_state(&run);
        assert_eq!(entry.status_color(), colors::STATUS_SUCCESS);

        run.status = RunStatus::Failed;
        let entry = RunHistoryEntry::from_run_state(&run);
        assert_eq!(entry.status_color(), colors::STATUS_ERROR);

        run.status = RunStatus::Running;
        let entry = RunHistoryEntry::from_run_state(&run);
        assert_eq!(entry.status_color(), colors::STATUS_RUNNING);
    }

    #[test]
    fn test_run_history_cleared_on_deselect() {
        let mut app = Autom8App::new(None);

        // Select a project (won't have history, but exercises the code path)
        app.toggle_project_selection("nonexistent-project");
        assert_eq!(app.selected_project(), Some("nonexistent-project"));

        // Deselect - history should be cleared
        app.toggle_project_selection("nonexistent-project");
        assert!(app.selected_project().is_none());
        assert!(app.run_history().is_empty());
    }

    #[test]
    fn test_run_history_cleared_on_switch_project() {
        let mut app = Autom8App::new(None);

        // Select first project
        app.toggle_project_selection("project-a");
        assert_eq!(app.selected_project(), Some("project-a"));

        // Switch to different project
        app.toggle_project_selection("project-b");
        assert_eq!(app.selected_project(), Some("project-b"));
        // History would be reloaded for the new project
    }
}
