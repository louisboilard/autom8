//! GUI application entry point.
//!
//! This module contains the eframe application setup and main window
//! configuration for the autom8 GUI.

use crate::config::{list_projects_tree, ProjectTreeInfo};
use crate::error::{Autom8Error, Result};
use crate::gui::theme::{self, colors, rounding};
use crate::gui::typography::{self, FontSize, FontWeight};
use crate::spec::Spec;
use crate::state::{LiveState, MachineState, RunState, SessionMetadata, StateManager};
use crate::worktree::MAIN_SESSION_ID;
use chrono::{DateTime, Utc};
use eframe::egui::{self, Color32, Rounding, Sense, Stroke};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Default window width in pixels.
const DEFAULT_WIDTH: f32 = 1200.0;

/// Default window height in pixels.
const DEFAULT_HEIGHT: f32 = 800.0;

/// Minimum window width in pixels.
const MIN_WIDTH: f32 = 800.0;

/// Minimum window height in pixels.
const MIN_HEIGHT: f32 = 600.0;

/// Height of the header/tab bar area.
const HEADER_HEIGHT: f32 = 48.0;

/// Horizontal padding within the header.
const HEADER_PADDING_H: f32 = 16.0;

/// Tab indicator underline height.
const TAB_UNDERLINE_HEIGHT: f32 = 2.0;

/// Tab horizontal padding.
const TAB_PADDING_H: f32 = 16.0;

/// Space between tabs.
const TAB_SPACING: f32 = 4.0;

/// Default refresh interval for data loading (500ms for GUI, less aggressive than TUI).
pub const DEFAULT_REFRESH_INTERVAL_MS: u64 = 500;

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

// ============================================================================
// Time Formatting Utilities
// ============================================================================

/// Format a duration from a start time as a human-readable string (e.g., "5m 32s", "1h 5m").
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

/// Format a timestamp as a relative time string (e.g., "2h ago", "3d ago").
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

/// Format a machine state as a human-readable string.
pub fn format_state(state: MachineState) -> &'static str {
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
            last_refresh: Instant::now(),
            refresh_interval,
        };
        // Initial data load
        app.refresh_data();
        app
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
        let tree_infos = match list_projects_tree() {
            Ok(infos) => infos,
            Err(_) => Vec::new(), // Continue with empty list on error
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
                    Spec::load(&run.spec_json_path).ok().map(|spec| RunProgress {
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
                    .inner_margin(egui::Margin::symmetric(HEADER_PADDING_H, 0.0)),
            )
            .show(ctx, |ui| {
                self.render_header(ui);
            });

        // Content area fills remaining space
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(colors::BACKGROUND)
                    .inner_margin(egui::Margin::same(16.0)),
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
            ui.add_space(TAB_SPACING);

            for tab in Tab::all() {
                let is_active = *tab == self.current_tab;
                if self.render_tab(ui, *tab, is_active) {
                    self.current_tab = *tab;
                }
                ui.add_space(TAB_SPACING);
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
    fn render_content(&self, ui: &mut egui::Ui) {
        match self.current_tab {
            Tab::ActiveRuns => self.render_active_runs(ui),
            Tab::Projects => self.render_projects(ui),
        }
    }

    /// Render the Active Runs view.
    fn render_active_runs(&self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new("Active Runs")
                    .font(typography::font(FontSize::Title, FontWeight::SemiBold))
                    .color(colors::TEXT_PRIMARY),
            );

            ui.add_space(8.0);

            if let Some(ref filter) = self.project_filter {
                ui.label(
                    egui::RichText::new(format!("Filtering by project: {}", filter))
                        .font(typography::font(FontSize::Body, FontWeight::Regular))
                        .color(colors::TEXT_SECONDARY),
                );
            }

            ui.add_space(16.0);

            if self.sessions.is_empty() {
                ui.label(
                    egui::RichText::new("No active runs.")
                        .font(typography::font(FontSize::Body, FontWeight::Regular))
                        .color(colors::TEXT_MUTED),
                );
            } else {
                // Show count of active sessions
                ui.label(
                    egui::RichText::new(format!("{} active session(s)", self.sessions.len()))
                        .font(typography::font(FontSize::Body, FontWeight::Regular))
                        .color(colors::TEXT_SECONDARY),
                );

                ui.add_space(8.0);

                // List sessions (placeholder - will be expanded in future stories)
                for session in &self.sessions {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(session.display_title())
                                .font(typography::font(FontSize::Body, FontWeight::Medium))
                                .color(colors::TEXT_PRIMARY),
                        );

                        if let Some(ref run) = session.run {
                            ui.label(
                                egui::RichText::new(format_state(run.machine_state))
                                    .font(typography::font(FontSize::Caption, FontWeight::Regular))
                                    .color(colors::TEXT_MUTED),
                            );
                        }

                        if let Some(ref error) = session.load_error {
                            ui.label(
                                egui::RichText::new(error)
                                    .font(typography::font(FontSize::Caption, FontWeight::Regular))
                                    .color(colors::STATUS_ERROR),
                            );
                        }
                    });
                }
            }
        });
    }

    /// Render the Projects view.
    fn render_projects(&self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new("Projects")
                    .font(typography::font(FontSize::Title, FontWeight::SemiBold))
                    .color(colors::TEXT_PRIMARY),
            );

            ui.add_space(8.0);

            if let Some(ref filter) = self.project_filter {
                ui.label(
                    egui::RichText::new(format!("Filtering by project: {}", filter))
                        .font(typography::font(FontSize::Body, FontWeight::Regular))
                        .color(colors::TEXT_SECONDARY),
                );
            }

            ui.add_space(16.0);

            if self.projects.is_empty() {
                ui.label(
                    egui::RichText::new("No projects found.")
                        .font(typography::font(FontSize::Body, FontWeight::Regular))
                        .color(colors::TEXT_MUTED),
                );
            } else {
                // Show count of projects
                ui.label(
                    egui::RichText::new(format!("{} project(s)", self.projects.len()))
                        .font(typography::font(FontSize::Body, FontWeight::Regular))
                        .color(colors::TEXT_SECONDARY),
                );

                ui.add_space(8.0);

                // List projects (placeholder - will be expanded in future stories)
                for project in &self.projects {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(&project.info.name)
                                .font(typography::font(FontSize::Body, FontWeight::Medium))
                                .color(colors::TEXT_PRIMARY),
                        );

                        let status = project.info.status_label();
                        let status_color = match status {
                            "running" => colors::STATUS_RUNNING,
                            "failed" => colors::STATUS_ERROR,
                            _ => colors::TEXT_MUTED,
                        };

                        ui.label(
                            egui::RichText::new(status)
                                .font(typography::font(FontSize::Caption, FontWeight::Regular))
                                .color(status_color),
                        );

                        if let Some(ref error) = project.load_error {
                            ui.label(
                                egui::RichText::new(error)
                                    .font(typography::font(FontSize::Caption, FontWeight::Regular))
                                    .color(colors::STATUS_ERROR),
                            );
                        }
                    });
                }
            }
        });
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
        assert!(app.projects().is_empty() || !app.projects().iter().any(|p| p.info.name == "nonexistent-project"));
        assert!(app.sessions().is_empty() || !app.sessions().iter().any(|s| s.project_name == "nonexistent-project"));
    }

    #[test]
    fn test_app_has_active_runs_initially_false() {
        let app = Autom8App::new(Some("nonexistent-project".to_string()));
        // With a nonexistent filter, there should be no active runs
        assert!(!app.has_active_runs() || app.sessions().is_empty());
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
        assert_eq!(format_state(MachineState::GeneratingSpec), "Generating Spec");
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
}
