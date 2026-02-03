//! GUI application entry point.
//!
//! This module contains the eframe application setup and main window
//! configuration for the autom8 GUI.

use crate::config::{list_projects_tree, ProjectTreeInfo};
use crate::error::{Autom8Error, Result};
use crate::ui::gui::components::{
    badge_background_color, format_duration, format_relative_time, format_state, state_to_color,
    truncate_with_ellipsis, MAX_BRANCH_LENGTH, MAX_TEXT_LENGTH,
};
use crate::ui::gui::theme::{self, colors, rounding, spacing};
use crate::ui::gui::typography::{self, FontSize, FontWeight};
use crate::ui::shared::{ProjectData, RunHistoryEntry, RunProgress, SessionData};
use crate::spec::Spec;
use crate::state::{MachineState, StateManager};
use crate::worktree::MAIN_SESSION_ID;
use eframe::egui::{self, Color32, Rect, Rounding, Sense, Stroke, Vec2};
use std::time::{Duration, Instant};

/// Default window width in pixels.
const DEFAULT_WIDTH: f32 = 840.0;

/// Default window height in pixels.
const DEFAULT_HEIGHT: f32 = 600.0;

/// Minimum window width in pixels.
const MIN_WIDTH: f32 = 400.0;

/// Minimum window height in pixels.
const MIN_HEIGHT: f32 = 300.0;

/// Height of the header/tab bar area (48px = 3 * LG spacing).
/// Note: Used by tests. The content header uses CONTENT_TAB_BAR_HEIGHT (36px).
#[allow(dead_code)]
const HEADER_HEIGHT: f32 = 48.0;

// ============================================================================
// Title Bar Constants (Custom Title Bar - US-002)
// ============================================================================

/// Height of the title bar area.
const TITLE_BAR_HEIGHT: f32 = 48.0;

/// Horizontal offset from the left edge for title bar content.
const TITLE_BAR_LEFT_OFFSET: f32 = 72.0;

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
// Split View Constants (Visual Polish - US-007)
// ============================================================================

/// Width of the visual divider between split panels.
const SPLIT_DIVIDER_WIDTH: f32 = 1.0;

/// Spacing around the divider (creates padding between content and divider).
const SPLIT_DIVIDER_MARGIN: f32 = 12.0; // spacing::MD

/// Minimum width for either panel in the split view.
const SPLIT_PANEL_MIN_WIDTH: f32 = 200.0;

// ============================================================================
// Sidebar Constants (Sidebar Navigation - US-003)
// ============================================================================

/// Width of the sidebar when expanded.
/// Based on Claude desktop reference (~200-220px).
const SIDEBAR_WIDTH: f32 = 220.0;

/// Width of the sidebar when collapsed (fully hidden).
/// The sidebar completely hides when collapsed, maximizing content area.
const SIDEBAR_COLLAPSED_WIDTH: f32 = 0.0;

// ============================================================================
// Sidebar Toggle Constants (Collapsible Sidebar - US-004)
// ============================================================================

/// Size of the sidebar toggle button.
const SIDEBAR_TOGGLE_SIZE: f32 = 34.0;

/// Horizontal padding before the toggle button.
const SIDEBAR_TOGGLE_PADDING: f32 = 8.0;

/// Height of each navigation item in the sidebar.
const SIDEBAR_ITEM_HEIGHT: f32 = 40.0;

/// Horizontal padding for sidebar items.
const SIDEBAR_ITEM_PADDING_H: f32 = 16.0; // spacing::LG

/// Vertical padding for sidebar items.
/// Note: Used by tests, available for future refinement.
#[allow(dead_code)]
const SIDEBAR_ITEM_PADDING_V: f32 = 8.0; // spacing::SM

/// Width of the accent bar indicator for active items.
const SIDEBAR_ACTIVE_INDICATOR_WIDTH: f32 = 3.0;

/// Corner rounding for sidebar item backgrounds.
const SIDEBAR_ITEM_ROUNDING: f32 = 6.0;

// ============================================================================
// GUI-specific Extensions
// ============================================================================

/// Extension trait for GUI-specific methods on RunHistoryEntry.
pub trait RunHistoryEntryExt {
    /// Get the status color for display (GUI-specific).
    fn status_color(&self) -> Color32;
}

impl RunHistoryEntryExt for RunHistoryEntry {
    fn status_color(&self) -> Color32 {
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

/// Unique identifier for tabs.
/// Static tabs use well-known IDs, dynamic tabs use unique generated IDs.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum TabId {
    /// The Active Runs tab (permanent).
    #[default]
    ActiveRuns,
    /// The Projects tab (permanent).
    Projects,
    /// A dynamic tab for viewing run details.
    /// Contains the run_id as identifier.
    RunDetail(String),
}

/// Information about a tab displayed in the tab bar.
#[derive(Debug, Clone)]
pub struct TabInfo {
    /// Unique identifier for this tab.
    pub id: TabId,
    /// Display label shown in the tab bar.
    pub label: String,
    /// Whether this tab can be closed (permanent tabs cannot be closed).
    pub closable: bool,
}

impl TabInfo {
    /// Create a new permanent (non-closable) tab.
    pub fn permanent(id: TabId, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            closable: false,
        }
    }

    /// Create a new closable dynamic tab.
    pub fn closable(id: TabId, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            closable: true,
        }
    }
}

/// The available tabs in the application.
/// This is kept for backward compatibility and used internally.
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

    /// Convert to TabId.
    pub fn to_tab_id(self) -> TabId {
        match self {
            Tab::ActiveRuns => TabId::ActiveRuns,
            Tab::Projects => TabId::Projects,
        }
    }
}

/// Maximum width for the tab bar scroll area.
const TAB_BAR_MAX_SCROLL_WIDTH: f32 = 800.0;

/// Width of the close button area on closable tabs.
const TAB_CLOSE_BUTTON_SIZE: f32 = 16.0;

/// Padding around the close button.
const TAB_CLOSE_PADDING: f32 = 4.0;

/// Height of the content header tab bar (only shown when dynamic tabs exist).
/// Sized to fit the text tightly without extra vertical gaps.
const CONTENT_TAB_BAR_HEIGHT: f32 = 32.0;

/// The main GUI application state.
///
/// This struct holds all UI state and loaded data, similar to the TUI's `MonitorApp`.
/// Data is refreshed at a configurable interval (default 500ms).
pub struct Autom8App {
    /// Optional project filter to show only a specific project.
    project_filter: Option<String>,
    /// Currently selected tab (legacy, for backward compatibility).
    current_tab: Tab,

    // ========================================================================
    // Dynamic Tab System
    // ========================================================================
    /// All open tabs in order. The first two are always ActiveRuns and Projects.
    tabs: Vec<TabInfo>,
    /// The currently active tab ID.
    active_tab_id: TabId,

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

    /// Cached run details for open detail tabs.
    /// Maps run_id to the full RunState for rendering detail views.
    run_detail_cache: std::collections::HashMap<String, crate::state::RunState>,

    /// Loading state for run history.
    /// True while run history is being loaded from disk.
    run_history_loading: bool,

    /// Error message if run history failed to load.
    run_history_error: Option<String>,

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

    // ========================================================================
    // Sidebar State (Collapsible Sidebar - US-004)
    // ========================================================================
    /// Whether the sidebar is collapsed.
    /// When collapsed, the sidebar is fully hidden to maximize content area.
    /// State persists during the session (not persisted across restarts).
    sidebar_collapsed: bool,
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
        // Initialize permanent tabs
        let tabs = vec![
            TabInfo::permanent(TabId::ActiveRuns, "Active Runs"),
            TabInfo::permanent(TabId::Projects, "Projects"),
        ];

        let mut app = Self {
            project_filter,
            current_tab: Tab::default(),
            tabs,
            active_tab_id: TabId::default(),
            projects: Vec::new(),
            sessions: Vec::new(),
            has_active_runs: false,
            selected_project: None,
            run_history: Vec::new(),
            run_detail_cache: std::collections::HashMap::new(),
            run_history_loading: false,
            run_history_error: None,
            initial_load_complete: false,
            last_refresh: Instant::now(),
            refresh_interval,
            sidebar_collapsed: false,
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

    // ========================================================================
    // Sidebar State (Collapsible Sidebar - US-004)
    // ========================================================================

    /// Returns whether the sidebar is collapsed.
    pub fn is_sidebar_collapsed(&self) -> bool {
        self.sidebar_collapsed
    }

    /// Sets the sidebar collapsed state.
    pub fn set_sidebar_collapsed(&mut self, collapsed: bool) {
        self.sidebar_collapsed = collapsed;
    }

    /// Toggles the sidebar collapsed state.
    pub fn toggle_sidebar(&mut self) {
        self.sidebar_collapsed = !self.sidebar_collapsed;
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
            // Deselect: clear selection, history, and error state
            self.selected_project = None;
            self.run_history.clear();
            self.run_history_loading = false;
            self.run_history_error = None;
        } else {
            // Select new project: update selection and load history
            self.selected_project = Some(project_name.to_string());
            self.load_run_history(project_name);
        }
    }

    /// Load run history for a specific project.
    /// Populates self.run_history with archived runs, sorted newest first.
    /// Sets loading and error states appropriately.
    fn load_run_history(&mut self, project_name: &str) {
        self.run_history.clear();
        self.run_history_error = None;
        self.run_history_loading = true;

        // Get the StateManager for this project
        let sm = match StateManager::for_project(project_name) {
            Ok(sm) => sm,
            Err(e) => {
                self.run_history_loading = false;
                self.run_history_error = Some(format!("Failed to access project: {}", e));
                return;
            }
        };

        // Load archived runs
        let archived = match sm.list_archived() {
            Ok(runs) => runs,
            Err(e) => {
                self.run_history_loading = false;
                self.run_history_error = Some(format!("Failed to load run history: {}", e));
                return;
            }
        };

        // Convert to RunHistoryEntry and store (already sorted newest first by list_archived)
        self.run_history = archived
            .iter()
            .map(|run| RunHistoryEntry::from_run_state(project_name.to_string(), run))
            .collect();

        self.run_history_loading = false;
    }

    /// Returns the run history for the selected project.
    pub fn run_history(&self) -> &[RunHistoryEntry] {
        &self.run_history
    }

    /// Returns whether run history is currently loading.
    pub fn is_run_history_loading(&self) -> bool {
        self.run_history_loading
    }

    /// Returns the run history error message, if any.
    pub fn run_history_error(&self) -> Option<&str> {
        self.run_history_error.as_deref()
    }

    /// Returns whether a project is currently selected.
    pub fn is_project_selected(&self, project_name: &str) -> bool {
        self.selected_project.as_deref() == Some(project_name)
    }

    // ========================================================================
    // Tab Management
    // ========================================================================

    /// Returns all open tabs.
    pub fn tabs(&self) -> &[TabInfo] {
        &self.tabs
    }

    /// Returns the currently active tab ID.
    pub fn active_tab_id(&self) -> &TabId {
        &self.active_tab_id
    }

    /// Returns the number of open tabs.
    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// Returns the number of closable (dynamic) tabs.
    pub fn closable_tab_count(&self) -> usize {
        self.tabs.iter().filter(|t| t.closable).count()
    }

    /// Set the active tab by ID.
    /// Also updates the legacy current_tab field for backward compatibility.
    pub fn set_active_tab(&mut self, tab_id: TabId) {
        // Update legacy field for backward compatibility
        match &tab_id {
            TabId::ActiveRuns => self.current_tab = Tab::ActiveRuns,
            TabId::Projects => self.current_tab = Tab::Projects,
            TabId::RunDetail(_) => {
                // Dynamic tabs don't have a legacy equivalent,
                // but we keep the last static tab for backward compat
            }
        }
        self.active_tab_id = tab_id;
    }

    /// Check if a tab with the given ID exists.
    pub fn has_tab(&self, tab_id: &TabId) -> bool {
        self.tabs.iter().any(|t| t.id == *tab_id)
    }

    /// Open a new dynamic tab for run details.
    /// If a tab with this run_id already exists, switches to it instead of creating a duplicate.
    /// Returns true if a new tab was created, false if an existing tab was activated.
    pub fn open_run_detail_tab(&mut self, run_id: &str, run_label: &str) -> bool {
        let tab_id = TabId::RunDetail(run_id.to_string());

        // Check if tab already exists
        if self.has_tab(&tab_id) {
            self.set_active_tab(tab_id);
            return false;
        }

        // Create new tab
        let tab = TabInfo::closable(tab_id.clone(), run_label);
        self.tabs.push(tab);
        self.set_active_tab(tab_id);
        true
    }

    /// Open a run detail tab from a RunHistoryEntry.
    /// Caches the run state for rendering and opens the tab.
    pub fn open_run_detail_from_entry(
        &mut self,
        entry: &RunHistoryEntry,
        run_state: Option<crate::state::RunState>,
    ) {
        let label = format!("Run - {}", entry.started_at.format("%Y-%m-%d %H:%M"));

        // Cache the run state if provided
        if let Some(state) = run_state {
            self.run_detail_cache.insert(entry.run_id.clone(), state);
        }

        self.open_run_detail_tab(&entry.run_id, &label);
    }

    /// Close a tab by ID.
    /// Returns true if the tab was closed, false if the tab doesn't exist or is not closable.
    /// If the closed tab was active, switches to the previous tab or Projects tab.
    pub fn close_tab(&mut self, tab_id: &TabId) -> bool {
        // Find the tab index
        let tab_index = match self.tabs.iter().position(|t| t.id == *tab_id) {
            Some(idx) => idx,
            None => return false,
        };

        // Check if the tab is closable
        if !self.tabs[tab_index].closable {
            return false;
        }

        // Check if this is the active tab
        let was_active = self.active_tab_id == *tab_id;

        // Remove the tab
        self.tabs.remove(tab_index);

        // Clean up cached run state if it's a run detail tab
        if let TabId::RunDetail(run_id) = tab_id {
            self.run_detail_cache.remove(run_id);
        }

        // If the closed tab was active, switch to another tab
        if was_active {
            // Try to switch to the previous tab (if it exists)
            if tab_index > 0 && tab_index <= self.tabs.len() {
                self.set_active_tab(self.tabs[tab_index - 1].id.clone());
            } else if !self.tabs.is_empty() {
                // Fall back to the first available tab or Projects tab
                self.set_active_tab(TabId::Projects);
            }
        }

        true
    }

    /// Close all closable (dynamic) tabs.
    /// Returns the number of tabs closed.
    pub fn close_all_dynamic_tabs(&mut self) -> usize {
        let to_close: Vec<TabId> = self
            .tabs
            .iter()
            .filter(|t| t.closable)
            .map(|t| t.id.clone())
            .collect();

        let count = to_close.len();
        for tab_id in to_close {
            self.close_tab(&tab_id);
        }
        count
    }

    /// Get cached run state for a run detail tab.
    pub fn get_cached_run_state(&self, run_id: &str) -> Option<&crate::state::RunState> {
        self.run_detail_cache.get(run_id)
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

        // Custom title bar area (provides draggable area for window)
        self.render_title_bar(ctx);

        // Sidebar navigation (replaces horizontal tab bar - US-003)
        // Sidebar can be collapsed via toggle button in title bar (US-004)
        let sidebar_width = if self.sidebar_collapsed {
            SIDEBAR_COLLAPSED_WIDTH
        } else {
            SIDEBAR_WIDTH
        };

        // Only show sidebar panel when not collapsed
        // When collapsed (width=0), the content area expands to fill the space
        if !self.sidebar_collapsed {
            egui::SidePanel::left("sidebar")
                .exact_width(sidebar_width)
                .resizable(false)
                .frame(
                    egui::Frame::none()
                        .fill(colors::BACKGROUND)
                        .inner_margin(egui::Margin {
                            left: spacing::MD,
                            right: spacing::MD,
                            top: spacing::LG,
                            bottom: spacing::LG,
                        })
                        .stroke(Stroke::new(1.0, colors::SEPARATOR)),
                )
                .show(ctx, |ui| {
                    self.render_sidebar(ui);
                });
        }

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
    // ========================================================================
    // Title Bar (Custom Title Bar - US-002)
    // ========================================================================

    /// Render the custom title bar area.
    ///
    /// This creates a panel at the top of the window that:
    /// - Uses the app's background color for seamless visual integration
    /// - Provides a draggable area for window movement
    /// - Contains the sidebar toggle button
    fn render_title_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("title_bar")
            .exact_height(TITLE_BAR_HEIGHT)
            .frame(
                egui::Frame::none()
                    .fill(colors::SURFACE)
                    .inner_margin(egui::Margin::ZERO),
            )
            .show(ctx, |ui| {
                // Make the entire title bar area draggable for window movement
                let title_bar_rect = ui.max_rect();
                let response = ui.interact(
                    title_bar_rect,
                    ui.id().with("title_bar_drag"),
                    Sense::click_and_drag(),
                );

                // Enable window dragging when the title bar is dragged
                if response.drag_started() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }

                // Support double-click to maximize/restore
                if response.double_clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(
                        !ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false)),
                    ));
                }

                // Position content to align with window control buttons (fixed offset from top)
                ui.add_space(5.0);
                ui.horizontal(|ui| {
                    // Left offset for title bar content
                    ui.add_space(TITLE_BAR_LEFT_OFFSET);

                    // Vertical separator between window controls and toggle button
                    let separator_height = SIDEBAR_TOGGLE_SIZE;
                    let (separator_rect, _) =
                        ui.allocate_exact_size(egui::vec2(1.0, separator_height), Sense::hover());
                    ui.painter().vline(
                        separator_rect.center().x,
                        separator_rect.y_range(),
                        Stroke::new(1.0, colors::SEPARATOR),
                    );

                    // Add some padding before the toggle button
                    ui.add_space(SIDEBAR_TOGGLE_PADDING);

                    // Sidebar toggle button
                    let toggle_response =
                        self.render_sidebar_toggle_button(ui, self.sidebar_collapsed);
                    if toggle_response.clicked() {
                        self.sidebar_collapsed = !self.sidebar_collapsed;
                    }
                });
            });
    }

    /// Render the sidebar toggle button in the title bar.
    ///
    /// The button uses a hamburger icon (☰) when collapsed (to expand)
    /// and a sidebar icon (⊏) when expanded (to collapse).
    /// Supports hover states for visual feedback.
    ///
    /// # Arguments
    /// * `ui` - The UI context
    /// * `is_collapsed` - Whether the sidebar is currently collapsed
    ///
    /// # Returns
    /// The egui Response for click detection
    fn render_sidebar_toggle_button(
        &self,
        ui: &mut egui::Ui,
        is_collapsed: bool,
    ) -> egui::Response {
        let button_size = egui::vec2(SIDEBAR_TOGGLE_SIZE, SIDEBAR_TOGGLE_SIZE);
        let (rect, response) = ui.allocate_exact_size(button_size, Sense::click());
        let is_hovered = response.hovered();

        // Draw background on hover
        if is_hovered {
            ui.painter().rect_filled(
                rect,
                Rounding::same(rounding::BUTTON),
                colors::SURFACE_HOVER,
            );
        }

        // Draw the icon
        // When collapsed: hamburger icon (three horizontal lines) to indicate "show sidebar"
        // When expanded: sidebar icon (panel + lines) to indicate "hide sidebar"
        let icon_color = if is_hovered {
            colors::TEXT_PRIMARY
        } else {
            colors::TEXT_SECONDARY
        };

        let painter = ui.painter();
        let center = rect.center();

        if is_collapsed {
            // Hamburger icon (three horizontal lines) - indicates "expand/show"
            let line_width = 12.0;
            let line_spacing = 4.0;
            let half_width = line_width / 2.0;

            for i in -1..=1 {
                let y = center.y + (i as f32) * line_spacing;
                painter.line_segment(
                    [
                        egui::pos2(center.x - half_width, y),
                        egui::pos2(center.x + half_width, y),
                    ],
                    Stroke::new(1.5, icon_color),
                );
            }
        } else {
            // Sidebar icon (left panel with lines) - indicates "collapse/hide"
            // Draw a rectangle representing the sidebar
            let icon_rect = Rect::from_center_size(center, egui::vec2(14.0, 12.0));

            // Outer frame
            painter.rect_stroke(icon_rect, Rounding::same(1.0), Stroke::new(1.5, icon_color));

            // Vertical divider (sidebar edge)
            let divider_x = icon_rect.left() + 5.0;
            painter.line_segment(
                [
                    egui::pos2(divider_x, icon_rect.top() + 1.0),
                    egui::pos2(divider_x, icon_rect.bottom() - 1.0),
                ],
                Stroke::new(1.0, icon_color),
            );

            // Content lines on the right side
            let line_start_x = divider_x + 2.0;
            let line_end_x = icon_rect.right() - 2.0;
            for i in 0..2 {
                let y = icon_rect.top() + 4.0 + (i as f32) * 4.0;
                painter.line_segment(
                    [egui::pos2(line_start_x, y), egui::pos2(line_end_x, y)],
                    Stroke::new(1.0, icon_color),
                );
            }
        }

        // Add tooltip
        let tooltip_text = if is_collapsed {
            "Show sidebar"
        } else {
            "Hide sidebar"
        };
        response.on_hover_text(tooltip_text)
    }

    // ========================================================================
    // Sidebar Navigation (US-003)
    // ========================================================================

    /// Render the sidebar navigation panel.
    ///
    /// The sidebar contains permanent navigation items (Active Runs, Projects)
    /// as a vertical list with visual indicators for the active item.
    /// A decorative animation is displayed at the bottom.
    fn render_sidebar(&mut self, ui: &mut egui::Ui) {
        // Use a layout that puts nav at top, animation at bottom
        ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
            // Add some top spacing to align with content area
            ui.add_space(spacing::SM);

            // Render permanent navigation items
            let mut tab_to_activate: Option<TabId> = None;

            // Snapshot of permanent tabs (ActiveRuns and Projects only)
            let permanent_tabs: Vec<(TabId, &'static str)> = vec![
                (TabId::ActiveRuns, "Active Runs"),
                (TabId::Projects, "Projects"),
            ];

            for (tab_id, label) in permanent_tabs {
                let is_active = self.active_tab_id == tab_id;
                if self.render_sidebar_item(ui, label, is_active) {
                    tab_to_activate = Some(tab_id);
                }
                ui.add_space(spacing::XS);
            }

            // Process tab activation after render loop
            if let Some(tab_id) = tab_to_activate {
                self.set_active_tab(tab_id);
            }

            // Fill remaining space, leaving room for animation
            let animation_height = 150.0;
            ui.add_space(ui.available_height() - animation_height);

            // Decorative animation at the bottom of sidebar
            // Uses full sidebar width, particles rise from bottom
            let sidebar_width = ui.available_width();
            super::animation::render_rising_particles(ui, sidebar_width, animation_height);
        });
    }

    /// Render a single sidebar navigation item.
    ///
    /// Returns true if the item was clicked.
    fn render_sidebar_item(&self, ui: &mut egui::Ui, label: &str, is_active: bool) -> bool {
        // Calculate item dimensions
        let available_width = ui.available_width();
        let item_size = egui::vec2(available_width, SIDEBAR_ITEM_HEIGHT);

        // Allocate space and create interaction response
        let (rect, response) = ui.allocate_exact_size(item_size, Sense::click());
        let is_hovered = response.hovered();

        // Determine background color based on state
        let bg_color = if is_active {
            colors::SURFACE_SELECTED
        } else if is_hovered {
            colors::SURFACE_HOVER
        } else {
            Color32::TRANSPARENT
        };

        // Draw background
        if bg_color != Color32::TRANSPARENT {
            ui.painter()
                .rect_filled(rect, Rounding::same(SIDEBAR_ITEM_ROUNDING), bg_color);
        }

        // Draw active indicator (accent bar on the left)
        if is_active {
            let indicator_rect = Rect::from_min_size(
                rect.min,
                egui::vec2(SIDEBAR_ACTIVE_INDICATOR_WIDTH, rect.height()),
            );
            ui.painter().rect_filled(
                indicator_rect,
                Rounding {
                    nw: SIDEBAR_ITEM_ROUNDING,
                    sw: SIDEBAR_ITEM_ROUNDING,
                    ne: 0.0,
                    se: 0.0,
                },
                colors::ACCENT,
            );
        }

        // Determine text color based on state
        let text_color = if is_active {
            colors::TEXT_PRIMARY
        } else {
            colors::TEXT_SECONDARY
        };

        // Draw text label
        let text_pos = egui::pos2(rect.left() + SIDEBAR_ITEM_PADDING_H, rect.center().y);

        ui.painter().text(
            text_pos,
            egui::Align2::LEFT_CENTER,
            label,
            typography::font(
                FontSize::Body,
                if is_active {
                    FontWeight::SemiBold
                } else {
                    FontWeight::Medium
                },
            ),
            text_color,
        );

        response.clicked()
    }

    // ========================================================================
    // Header / Tab Bar (preserved for US-005: Dynamic Tabs in Content Header)
    // ========================================================================

    /// Render the header area with tab bar.
    /// Note: Will be repurposed for US-005 (Dynamic Tabs in Content Header).
    #[allow(dead_code)]
    fn render_header(&mut self, ui: &mut egui::Ui) {
        // Use horizontal scroll for tab bar if there are many tabs
        let scroll_width = ui.available_width().min(TAB_BAR_MAX_SCROLL_WIDTH);

        ui.horizontal_centered(|ui| {
            ui.add_space(spacing::XS);

            egui::ScrollArea::horizontal()
                .max_width(scroll_width)
                .auto_shrink([false, false])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        // Collect tab actions to process after render loop
                        let mut tab_to_activate: Option<TabId> = None;
                        let mut tab_to_close: Option<TabId> = None;

                        // Clone tabs to avoid borrow issues
                        let tabs_snapshot: Vec<(TabId, String, bool)> = self
                            .tabs
                            .iter()
                            .map(|t| (t.id.clone(), t.label.clone(), t.closable))
                            .collect();

                        for (tab_id, label, closable) in &tabs_snapshot {
                            let is_active = self.active_tab_id == *tab_id;
                            let (clicked, close_clicked) =
                                self.render_dynamic_tab(ui, label, *closable, is_active);

                            if clicked {
                                tab_to_activate = Some(tab_id.clone());
                            }
                            if close_clicked {
                                tab_to_close = Some(tab_id.clone());
                            }
                            ui.add_space(spacing::XS);
                        }

                        // Process actions after render loop
                        if let Some(tab_id) = tab_to_close {
                            self.close_tab(&tab_id);
                        } else if let Some(tab_id) = tab_to_activate {
                            self.set_active_tab(tab_id);
                        }
                    });
                });
        });

        // Draw bottom border for header
        let rect = ui.max_rect();
        ui.painter().hline(
            rect.x_range(),
            rect.bottom(),
            Stroke::new(1.0, colors::BORDER),
        );
    }

    /// Render a single tab button with optional close button.
    /// Returns (tab_clicked, close_clicked).
    /// Note: Will be used for US-005 (Dynamic Tabs in Content Header).
    #[allow(dead_code)]
    fn render_dynamic_tab(
        &self,
        ui: &mut egui::Ui,
        label: &str,
        closable: bool,
        is_active: bool,
    ) -> (bool, bool) {
        // Calculate text size
        let text_galley = ui.fonts(|f| {
            f.layout_no_wrap(
                label.to_string(),
                typography::font(FontSize::Body, FontWeight::Medium),
                colors::TEXT_PRIMARY,
            )
        });
        let text_size = text_galley.size();

        // Calculate tab width including close button if closable
        let close_button_space = if closable {
            TAB_CLOSE_BUTTON_SIZE + TAB_CLOSE_PADDING
        } else {
            0.0
        };
        let tab_width = text_size.x + TAB_PADDING_H * 2.0 + close_button_space;
        let tab_size = egui::vec2(tab_width, HEADER_HEIGHT - TAB_UNDERLINE_HEIGHT);

        // Allocate space for the entire tab
        let (rect, response) = ui.allocate_exact_size(tab_size, Sense::click());

        let is_hovered = response.hovered();

        // Draw tab background on hover (subtle)
        if is_hovered && !is_active {
            ui.painter().rect_filled(
                rect,
                Rounding::same(rounding::BUTTON),
                colors::SURFACE_HOVER,
            );
        }

        // Draw text (offset left if closable to make room for close button)
        let text_color = if is_active {
            colors::TEXT_PRIMARY
        } else if is_hovered {
            colors::TEXT_SECONDARY
        } else {
            colors::TEXT_MUTED
        };

        let text_x = if closable {
            rect.left() + TAB_PADDING_H
        } else {
            rect.center().x - text_size.x / 2.0
        };
        let text_pos = egui::pos2(text_x, rect.center().y - text_size.y / 2.0);

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

        // Draw close button for closable tabs
        let mut close_clicked = false;
        if closable {
            let close_rect = Rect::from_min_size(
                egui::pos2(
                    rect.right() - TAB_PADDING_H - TAB_CLOSE_BUTTON_SIZE,
                    rect.center().y - TAB_CLOSE_BUTTON_SIZE / 2.0,
                ),
                egui::vec2(TAB_CLOSE_BUTTON_SIZE, TAB_CLOSE_BUTTON_SIZE),
            );

            // Check if mouse is over the close button
            let close_hovered = ui
                .ctx()
                .input(|i| i.pointer.hover_pos())
                .is_some_and(|pos| close_rect.contains(pos));

            // Draw close button background on hover
            if close_hovered {
                ui.painter().rect_filled(
                    close_rect,
                    Rounding::same(rounding::SMALL),
                    colors::SURFACE_HOVER,
                );
            }

            // Draw X icon
            let x_color = if close_hovered {
                colors::TEXT_PRIMARY
            } else {
                colors::TEXT_MUTED
            };
            let x_center = close_rect.center();
            let x_size = TAB_CLOSE_BUTTON_SIZE * 0.35;
            ui.painter().line_segment(
                [
                    egui::pos2(x_center.x - x_size, x_center.y - x_size),
                    egui::pos2(x_center.x + x_size, x_center.y + x_size),
                ],
                Stroke::new(1.5, x_color),
            );
            ui.painter().line_segment(
                [
                    egui::pos2(x_center.x + x_size, x_center.y - x_size),
                    egui::pos2(x_center.x - x_size, x_center.y + x_size),
                ],
                Stroke::new(1.5, x_color),
            );

            // Check for close button click
            if response.clicked() && close_hovered {
                close_clicked = true;
            }
        }

        // Draw underline indicator for active tab
        if is_active {
            let underline_rect = egui::Rect::from_min_size(
                egui::pos2(rect.left(), rect.bottom() - TAB_UNDERLINE_HEIGHT),
                egui::vec2(rect.width(), TAB_UNDERLINE_HEIGHT),
            );
            ui.painter()
                .rect_filled(underline_rect, Rounding::ZERO, colors::ACCENT);
        }

        // Tab was clicked if response.clicked() and NOT close button clicked
        let tab_clicked = response.clicked() && !close_clicked;

        (tab_clicked, close_clicked)
    }

    /// Render the content area based on the current tab.
    ///
    /// When dynamic tabs are open, a tab bar header appears at the top of the
    /// content area showing the closable tabs. Clicking a tab switches to it,
    /// and closing the last dynamic tab returns to the permanent view.
    fn render_content(&mut self, ui: &mut egui::Ui) {
        // Check if there are dynamic tabs to show the content header tab bar
        let has_dynamic_tabs = self.closable_tab_count() > 0;

        if has_dynamic_tabs {
            // Render the content header with dynamic tabs
            self.render_content_tab_bar(ui);

            // Add a subtle separator line
            let separator_rect = ui.available_rect_before_wrap();
            ui.painter().hline(
                separator_rect.x_range(),
                separator_rect.top(),
                Stroke::new(1.0, colors::SEPARATOR),
            );

            ui.add_space(spacing::SM);
        }

        // Render the main content based on the active tab
        match &self.active_tab_id {
            TabId::ActiveRuns => self.render_active_runs(ui),
            TabId::Projects => self.render_projects(ui),
            TabId::RunDetail(run_id) => {
                let run_id = run_id.clone();
                self.render_run_detail(ui, &run_id);
            }
        }
    }

    /// Render the content header tab bar with dynamic tabs only.
    ///
    /// This tab bar appears in the content area header when dynamic tabs (like
    /// Run Detail views) are open. The permanent tabs (Active Runs, Projects)
    /// are handled by the sidebar navigation, not shown here.
    ///
    /// Features:
    /// - Each tab has a close button (X)
    /// - Clicking a tab switches to that content
    /// - Closing the last dynamic tab returns to the last permanent view
    /// - Tab bar uses horizontal scrolling if many tabs are open
    fn render_content_tab_bar(&mut self, ui: &mut egui::Ui) {
        // Allocate fixed height for the tab bar
        let available_width = ui.available_width();
        let scroll_width = available_width.min(TAB_BAR_MAX_SCROLL_WIDTH);

        ui.allocate_ui_with_layout(
            egui::vec2(available_width, CONTENT_TAB_BAR_HEIGHT),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                // Collect tab actions to process after render loop
                let mut tab_to_activate: Option<TabId> = None;
                let mut tab_to_close: Option<TabId> = None;

                egui::ScrollArea::horizontal()
                    .max_width(scroll_width)
                    .auto_shrink([false, false])
                    .scroll_bar_visibility(
                        egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded,
                    )
                    .show(ui, |ui| {
                        ui.horizontal_centered(|ui| {
                            ui.add_space(spacing::XS);

                            // Only show closable (dynamic) tabs in the content header
                            let dynamic_tabs: Vec<(TabId, String)> = self
                                .tabs
                                .iter()
                                .filter(|t| t.closable)
                                .map(|t| (t.id.clone(), t.label.clone()))
                                .collect();

                            for (tab_id, label) in &dynamic_tabs {
                                let is_active = self.active_tab_id == *tab_id;
                                let (clicked, close_clicked) =
                                    self.render_content_tab(ui, label, is_active);

                                if clicked {
                                    tab_to_activate = Some(tab_id.clone());
                                }
                                if close_clicked {
                                    tab_to_close = Some(tab_id.clone());
                                }
                                ui.add_space(spacing::XS);
                            }
                        });
                    });

                // Process actions after render loop
                if let Some(tab_id) = tab_to_close {
                    self.close_tab(&tab_id);
                } else if let Some(tab_id) = tab_to_activate {
                    self.set_active_tab(tab_id);
                }
            },
        );
    }

    /// Render a single tab in the content header tab bar.
    ///
    /// Each tab displays its label and a close button (X).
    /// Returns (tab_clicked, close_clicked).
    fn render_content_tab(&self, ui: &mut egui::Ui, label: &str, is_active: bool) -> (bool, bool) {
        // Calculate text size
        let text_galley = ui.fonts(|f| {
            f.layout_no_wrap(
                label.to_string(),
                typography::font(FontSize::Body, FontWeight::Medium),
                colors::TEXT_PRIMARY,
            )
        });
        let text_size = text_galley.size();

        // Calculate tab width including close button
        let close_button_space = TAB_CLOSE_BUTTON_SIZE + TAB_CLOSE_PADDING;
        let tab_width = text_size.x + TAB_PADDING_H * 2.0 + close_button_space;
        let tab_height = CONTENT_TAB_BAR_HEIGHT - TAB_UNDERLINE_HEIGHT - spacing::XS;
        let tab_size = egui::vec2(tab_width, tab_height);

        // Allocate space for the entire tab
        let (rect, response) = ui.allocate_exact_size(tab_size, Sense::click());
        let is_hovered = response.hovered();

        // Draw tab background
        let bg_color = if is_active {
            colors::SURFACE_SELECTED
        } else if is_hovered {
            colors::SURFACE_HOVER
        } else {
            Color32::TRANSPARENT
        };

        if bg_color != Color32::TRANSPARENT {
            ui.painter()
                .rect_filled(rect, Rounding::same(rounding::BUTTON), bg_color);
        }

        // Draw text
        let text_color = if is_active {
            colors::TEXT_PRIMARY
        } else if is_hovered {
            colors::TEXT_SECONDARY
        } else {
            colors::TEXT_MUTED
        };

        let text_x = rect.left() + TAB_PADDING_H;
        let text_pos = egui::pos2(text_x, rect.center().y - text_size.y / 2.0);

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

        // Draw close button
        let close_rect = Rect::from_min_size(
            egui::pos2(
                rect.right() - TAB_PADDING_H - TAB_CLOSE_BUTTON_SIZE,
                rect.center().y - TAB_CLOSE_BUTTON_SIZE / 2.0,
            ),
            egui::vec2(TAB_CLOSE_BUTTON_SIZE, TAB_CLOSE_BUTTON_SIZE),
        );

        // Check if mouse is over the close button
        let close_hovered = ui
            .ctx()
            .input(|i| i.pointer.hover_pos())
            .is_some_and(|pos| close_rect.contains(pos));

        // Draw close button background on hover
        if close_hovered {
            ui.painter().rect_filled(
                close_rect,
                Rounding::same(rounding::SMALL),
                colors::SURFACE_HOVER,
            );
        }

        // Draw X icon
        let x_color = if close_hovered {
            colors::TEXT_PRIMARY
        } else {
            colors::TEXT_MUTED
        };
        let x_center = close_rect.center();
        let x_size = TAB_CLOSE_BUTTON_SIZE * 0.3;

        ui.painter().line_segment(
            [
                egui::pos2(x_center.x - x_size, x_center.y - x_size),
                egui::pos2(x_center.x + x_size, x_center.y + x_size),
            ],
            Stroke::new(1.5, x_color),
        );
        ui.painter().line_segment(
            [
                egui::pos2(x_center.x + x_size, x_center.y - x_size),
                egui::pos2(x_center.x - x_size, x_center.y + x_size),
            ],
            Stroke::new(1.5, x_color),
        );

        // Draw underline indicator for active tab
        if is_active {
            let underline_rect = egui::Rect::from_min_size(
                egui::pos2(rect.left(), rect.bottom()),
                egui::vec2(rect.width(), TAB_UNDERLINE_HEIGHT),
            );
            ui.painter()
                .rect_filled(underline_rect, Rounding::ZERO, colors::ACCENT);
        }

        // Close button click takes precedence over tab click
        let close_clicked = response.clicked() && close_hovered;
        let tab_clicked = response.clicked() && !close_hovered;

        (tab_clicked, close_clicked)
    }

    /// Render the run detail view for a specific run.
    fn render_run_detail(&self, ui: &mut egui::Ui, run_id: &str) {
        // Header (fixed, not scrollable)
        ui.label(
            egui::RichText::new(format!("Run Details: {}", run_id))
                .font(typography::font(FontSize::Title, FontWeight::SemiBold))
                .color(colors::TEXT_PRIMARY),
        );

        ui.add_space(spacing::MD);

        // Check if we have cached run state
        if let Some(run_state) = self.run_detail_cache.get(run_id) {
            // Render run details in a ScrollArea that fills remaining space
            self.render_run_state_details(ui, run_state);
        } else {
            // No cached state - show placeholder (also in ScrollArea for consistency)
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_space(spacing::XXL);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new("Run details not available")
                                .font(typography::font(FontSize::Heading, FontWeight::Medium))
                                .color(colors::TEXT_MUTED),
                        );

                        ui.add_space(spacing::SM);

                        ui.label(
                            egui::RichText::new(
                                "This run may have been archived or the data is unavailable.",
                            )
                            .font(typography::font(FontSize::Body, FontWeight::Regular))
                            .color(colors::TEXT_MUTED),
                        );
                    });
                });
        }
    }

    /// Render detailed information about a run state.
    fn render_run_state_details(&self, ui: &mut egui::Ui, run_state: &crate::state::RunState) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // ================================================================
                // RUN SUMMARY SECTION
                // ================================================================
                self.render_run_summary_card(ui, run_state);

                ui.add_space(spacing::LG);
                ui.separator();
                ui.add_space(spacing::MD);

                // ================================================================
                // STORIES SECTION
                // ================================================================
                ui.label(
                    egui::RichText::new("Stories")
                        .font(typography::font(FontSize::Heading, FontWeight::SemiBold))
                        .color(colors::TEXT_PRIMARY),
                );

                ui.add_space(spacing::SM);

                if run_state.iterations.is_empty() {
                    ui.label(
                        egui::RichText::new("No stories processed yet")
                            .font(typography::font(FontSize::Body, FontWeight::Regular))
                            .color(colors::TEXT_MUTED),
                    );
                } else {
                    // Group iterations by story_id while preserving order
                    let mut story_order: Vec<String> = Vec::new();
                    let mut story_iterations: std::collections::HashMap<
                        String,
                        Vec<&crate::state::IterationRecord>,
                    > = std::collections::HashMap::new();

                    for iter in &run_state.iterations {
                        if !story_iterations.contains_key(&iter.story_id) {
                            story_order.push(iter.story_id.clone());
                        }
                        story_iterations
                            .entry(iter.story_id.clone())
                            .or_default()
                            .push(iter);
                    }

                    // Render each story in order
                    for story_id in &story_order {
                        let iterations = story_iterations.get(story_id).unwrap();
                        self.render_story_detail_card(ui, story_id, iterations);
                        ui.add_space(spacing::MD);
                    }
                }
            });
    }

    /// Render the run summary card with status, timing, and metadata.
    fn render_run_summary_card(&self, ui: &mut egui::Ui, run_state: &crate::state::RunState) {
        // Status badge and run ID row
        ui.horizontal(|ui| {
            // Status badge
            let status_text = match run_state.status {
                crate::state::RunStatus::Completed => "Completed",
                crate::state::RunStatus::Failed => "Failed",
                crate::state::RunStatus::Running => "Running",
            };
            let status_color = match run_state.status {
                crate::state::RunStatus::Completed => colors::STATUS_SUCCESS,
                crate::state::RunStatus::Failed => colors::STATUS_ERROR,
                crate::state::RunStatus::Running => colors::STATUS_RUNNING,
            };

            let badge_galley = ui.fonts(|f| {
                f.layout_no_wrap(
                    status_text.to_string(),
                    typography::font(FontSize::Body, FontWeight::Medium),
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
                badge_background_color(status_color),
            );

            let text_pos = badge_rect.center() - badge_galley.rect.center().to_vec2();
            ui.painter().galley(text_pos, badge_galley, status_color);

            ui.add_space(spacing::MD);

            // Run ID (smaller, muted)
            ui.label(
                egui::RichText::new(format!(
                    "Run ID: {}",
                    &run_state.run_id[..8.min(run_state.run_id.len())]
                ))
                .font(typography::font(FontSize::Small, FontWeight::Regular))
                .color(colors::TEXT_MUTED),
            );
        });

        ui.add_space(spacing::MD);

        // Grid layout for timing information
        egui::Grid::new("run_timing_grid")
            .num_columns(2)
            .spacing([spacing::LG, spacing::XS])
            .show(ui, |ui| {
                // Start time
                ui.label(
                    egui::RichText::new("Start Time:")
                        .font(typography::font(FontSize::Body, FontWeight::Medium))
                        .color(colors::TEXT_SECONDARY),
                );
                ui.label(
                    egui::RichText::new(
                        run_state.started_at.format("%Y-%m-%d %H:%M:%S").to_string(),
                    )
                    .font(typography::font(FontSize::Body, FontWeight::Regular))
                    .color(colors::TEXT_PRIMARY),
                );
                ui.end_row();

                // End time
                ui.label(
                    egui::RichText::new("End Time:")
                        .font(typography::font(FontSize::Body, FontWeight::Medium))
                        .color(colors::TEXT_SECONDARY),
                );
                if let Some(finished) = run_state.finished_at {
                    ui.label(
                        egui::RichText::new(finished.format("%Y-%m-%d %H:%M:%S").to_string())
                            .font(typography::font(FontSize::Body, FontWeight::Regular))
                            .color(colors::TEXT_PRIMARY),
                    );
                } else {
                    ui.label(
                        egui::RichText::new("In progress...")
                            .font(typography::font(FontSize::Body, FontWeight::Regular))
                            .color(colors::STATUS_RUNNING),
                    );
                }
                ui.end_row();

                // Duration
                ui.label(
                    egui::RichText::new("Duration:")
                        .font(typography::font(FontSize::Body, FontWeight::Medium))
                        .color(colors::TEXT_SECONDARY),
                );
                let duration_str = if let Some(finished) = run_state.finished_at {
                    let duration = finished - run_state.started_at;
                    Self::format_duration_detailed(duration)
                } else {
                    let duration = chrono::Utc::now() - run_state.started_at;
                    format!("{} (ongoing)", Self::format_duration_detailed(duration))
                };
                ui.label(
                    egui::RichText::new(duration_str)
                        .font(typography::font(FontSize::Body, FontWeight::Regular))
                        .color(colors::TEXT_PRIMARY),
                );
                ui.end_row();

                // Branch
                ui.label(
                    egui::RichText::new("Branch:")
                        .font(typography::font(FontSize::Body, FontWeight::Medium))
                        .color(colors::TEXT_SECONDARY),
                );
                ui.label(
                    egui::RichText::new(&run_state.branch)
                        .font(typography::font(FontSize::Body, FontWeight::Regular))
                        .color(colors::ACCENT),
                );
                ui.end_row();

                // Story summary
                let completed_count = run_state
                    .iterations
                    .iter()
                    .filter(|i| i.status == crate::state::IterationStatus::Success)
                    .map(|i| &i.story_id)
                    .collect::<std::collections::HashSet<_>>()
                    .len();
                let total_stories = run_state
                    .iterations
                    .iter()
                    .map(|i| &i.story_id)
                    .collect::<std::collections::HashSet<_>>()
                    .len();

                if total_stories > 0 {
                    ui.label(
                        egui::RichText::new("Stories:")
                            .font(typography::font(FontSize::Body, FontWeight::Medium))
                            .color(colors::TEXT_SECONDARY),
                    );
                    ui.label(
                        egui::RichText::new(format!(
                            "{}/{} completed",
                            completed_count, total_stories
                        ))
                        .font(typography::font(FontSize::Body, FontWeight::Regular))
                        .color(colors::TEXT_PRIMARY),
                    );
                    ui.end_row();
                }
            });
    }

    /// Render a detailed card for a single story with all its iterations.
    fn render_story_detail_card(
        &self,
        ui: &mut egui::Ui,
        story_id: &str,
        iterations: &[&crate::state::IterationRecord],
    ) {
        let last_iter = iterations.last().unwrap();
        let status_color = match last_iter.status {
            crate::state::IterationStatus::Success => colors::STATUS_SUCCESS,
            crate::state::IterationStatus::Failed => colors::STATUS_ERROR,
            crate::state::IterationStatus::Running => colors::STATUS_RUNNING,
        };

        // Story card background
        let available_width = ui.available_width();
        egui::Frame::none()
            .fill(colors::SURFACE_HOVER)
            .rounding(Rounding::same(rounding::CARD))
            .inner_margin(egui::Margin::same(spacing::MD))
            .show(ui, |ui| {
                ui.set_min_width(available_width - spacing::MD * 2.0);

                // Story header row
                ui.horizontal(|ui| {
                    // Status dot
                    let (dot_rect, _) =
                        ui.allocate_exact_size(Vec2::splat(spacing::MD), Sense::hover());
                    ui.painter()
                        .circle_filled(dot_rect.center(), 5.0, status_color);

                    // Story ID
                    ui.label(
                        egui::RichText::new(story_id)
                            .font(typography::font(FontSize::Body, FontWeight::SemiBold))
                            .color(colors::TEXT_PRIMARY),
                    );

                    // Status text badge
                    let status_text = match last_iter.status {
                        crate::state::IterationStatus::Success => "Success",
                        crate::state::IterationStatus::Failed => "Failed",
                        crate::state::IterationStatus::Running => "Running",
                    };

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let badge_galley = ui.fonts(|f| {
                            f.layout_no_wrap(
                                status_text.to_string(),
                                typography::font(FontSize::Small, FontWeight::Medium),
                                status_color,
                            )
                        });
                        let badge_width = badge_galley.rect.width() + spacing::SM * 2.0;
                        let badge_height = badge_galley.rect.height() + spacing::XS * 2.0;

                        let (badge_rect, _) = ui.allocate_exact_size(
                            Vec2::new(badge_width, badge_height),
                            Sense::hover(),
                        );

                        ui.painter().rect_filled(
                            badge_rect,
                            Rounding::same(rounding::SMALL),
                            badge_background_color(status_color),
                        );

                        let text_pos = badge_rect.center() - badge_galley.rect.center().to_vec2();
                        ui.painter().galley(text_pos, badge_galley, status_color);
                    });
                });

                // Work summary if available (from the last successful iteration)
                let work_summary = iterations
                    .iter()
                    .rev()
                    .find_map(|iter| iter.work_summary.as_ref());

                if let Some(summary) = work_summary {
                    ui.add_space(spacing::SM);
                    ui.label(
                        egui::RichText::new(truncate_with_ellipsis(summary, 200))
                            .font(typography::font(FontSize::Small, FontWeight::Regular))
                            .color(colors::TEXT_SECONDARY),
                    );
                }

                // Iteration details section (shown if there are multiple iterations)
                if iterations.len() > 1 {
                    ui.add_space(spacing::SM);
                    ui.separator();
                    ui.add_space(spacing::SM);

                    ui.label(
                        egui::RichText::new(format!("Iterations ({} total)", iterations.len()))
                            .font(typography::font(FontSize::Small, FontWeight::SemiBold))
                            .color(colors::TEXT_SECONDARY),
                    );

                    ui.add_space(spacing::XS);

                    // Show each iteration in a compact format
                    for (idx, iter) in iterations.iter().enumerate() {
                        let iter_status_color = match iter.status {
                            crate::state::IterationStatus::Success => colors::STATUS_SUCCESS,
                            crate::state::IterationStatus::Failed => colors::STATUS_ERROR,
                            crate::state::IterationStatus::Running => colors::STATUS_RUNNING,
                        };

                        ui.horizontal(|ui| {
                            // Small status indicator
                            let (dot_rect, _) =
                                ui.allocate_exact_size(Vec2::splat(spacing::SM), Sense::hover());
                            ui.painter()
                                .circle_filled(dot_rect.center(), 3.0, iter_status_color);

                            // Iteration number
                            ui.label(
                                egui::RichText::new(format!("#{}", idx + 1))
                                    .font(typography::font(FontSize::Caption, FontWeight::Medium))
                                    .color(colors::TEXT_PRIMARY),
                            );

                            // Status
                            let status_str = match iter.status {
                                crate::state::IterationStatus::Success => "Success",
                                crate::state::IterationStatus::Failed => "Failed (review cycle)",
                                crate::state::IterationStatus::Running => "Running",
                            };
                            ui.label(
                                egui::RichText::new(status_str)
                                    .font(typography::font(FontSize::Caption, FontWeight::Regular))
                                    .color(iter_status_color),
                            );

                            // Duration if available
                            if let Some(finished) = iter.finished_at {
                                let duration = finished - iter.started_at;
                                let duration_str = Self::format_duration_short(duration);
                                ui.label(
                                    egui::RichText::new(format!("({})", duration_str))
                                        .font(typography::font(
                                            FontSize::Caption,
                                            FontWeight::Regular,
                                        ))
                                        .color(colors::TEXT_MUTED),
                                );
                            }
                        });
                    }
                } else {
                    // Single iteration - show duration
                    let iter = iterations[0];
                    if let Some(finished) = iter.finished_at {
                        ui.add_space(spacing::XS);
                        let duration = finished - iter.started_at;
                        ui.label(
                            egui::RichText::new(format!(
                                "Duration: {}",
                                Self::format_duration_detailed(duration)
                            ))
                            .font(typography::font(FontSize::Small, FontWeight::Regular))
                            .color(colors::TEXT_MUTED),
                        );
                    }
                }
            });
    }

    /// Format a chrono Duration as a detailed string (e.g., "1h 23m 45s").
    fn format_duration_detailed(duration: chrono::Duration) -> String {
        let total_seconds = duration.num_seconds().max(0);
        let hours = total_seconds / 3600;
        let minutes = (total_seconds % 3600) / 60;
        let seconds = total_seconds % 60;

        if hours > 0 {
            format!("{}h {}m {}s", hours, minutes, seconds)
        } else if minutes > 0 {
            format!("{}m {}s", minutes, seconds)
        } else {
            format!("{}s", seconds)
        }
    }

    /// Format a chrono Duration as a short string (e.g., "2m 30s").
    fn format_duration_short(duration: chrono::Duration) -> String {
        let total_seconds = duration.num_seconds().max(0);
        let hours = total_seconds / 3600;
        let minutes = (total_seconds % 3600) / 60;
        let seconds = total_seconds % 60;

        if hours > 0 {
            format!("{}h{}m", hours, minutes)
        } else if minutes > 0 {
            format!("{}m{}s", minutes, seconds)
        } else {
            format!("{}s", seconds)
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
        let available_height = ui.available_height();

        // Calculate panel widths: 50/50 split with divider in the middle
        // Subtract the divider width and margins from the total width
        let divider_total_width = SPLIT_DIVIDER_WIDTH + SPLIT_DIVIDER_MARGIN * 2.0;
        let panel_width =
            ((available_width - divider_total_width) / 2.0).max(SPLIT_PANEL_MIN_WIDTH);

        // We need to collect the clicked_run_id outside the closure
        let mut clicked_run_id: Option<String> = None;

        ui.horizontal(|ui| {
            // Left panel: Project list
            ui.allocate_ui_with_layout(
                Vec2::new(panel_width, available_height),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    self.render_projects_left_panel(ui);
                },
            );

            // Visual divider between panels with appropriate margin
            ui.add_space(SPLIT_DIVIDER_MARGIN);

            // Draw a custom vertical divider line using the SEPARATOR color
            let divider_rect = ui.available_rect_before_wrap();
            let divider_line_rect = Rect::from_min_size(
                divider_rect.min,
                Vec2::new(SPLIT_DIVIDER_WIDTH, available_height),
            );
            ui.painter()
                .rect_filled(divider_line_rect, Rounding::ZERO, colors::SEPARATOR);
            ui.add_space(SPLIT_DIVIDER_WIDTH);

            ui.add_space(SPLIT_DIVIDER_MARGIN);

            // Right panel: Run history for selected project
            ui.allocate_ui_with_layout(
                Vec2::new(ui.available_width(), available_height),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    clicked_run_id = self.render_projects_right_panel(ui);
                },
            );
        });

        // Handle click on run history entry - open detail tab
        if let Some(run_id) = clicked_run_id {
            // Find the entry in run_history to get the label
            if let Some(entry) = self.run_history.iter().find(|e| e.run_id == run_id) {
                let entry_clone = entry.clone();

                // Try to load the full run state for the detail view
                if let Some(ref project_name) = self.selected_project {
                    let run_state = StateManager::for_project(project_name).ok().and_then(|sm| {
                        sm.list_archived()
                            .ok()
                            .and_then(|runs| runs.into_iter().find(|r| r.run_id == run_id))
                    });

                    self.open_run_detail_from_entry(&entry_clone, run_state);
                }
            }
        }
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
    /// Returns the run_id of a clicked entry, if any.
    fn render_projects_right_panel(&self, ui: &mut egui::Ui) -> Option<String> {
        let mut clicked_run_id: Option<String> = None;

        if let Some(ref selected_name) = self.selected_project {
            // Header: Project name
            ui.label(
                egui::RichText::new(format!("Run History: {}", selected_name))
                    .font(typography::font(FontSize::Title, FontWeight::SemiBold))
                    .color(colors::TEXT_PRIMARY),
            );

            ui.add_space(spacing::MD);

            // Check for error state first
            if let Some(ref error) = self.run_history_error {
                self.render_run_history_error(ui, error);
            } else if self.run_history_loading {
                // Show loading indicator
                self.render_run_history_loading(ui);
            } else if self.run_history.is_empty() {
                // Empty state for no run history
                self.render_run_history_empty(ui);
            } else {
                // Scrollable run history list
                egui::ScrollArea::vertical()
                    .id_salt("projects_right_panel")
                    .auto_shrink([false, false])
                    .scroll_bar_visibility(
                        egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded,
                    )
                    .show(ui, |ui| {
                        for entry in &self.run_history {
                            if self.render_run_history_entry(ui, entry) {
                                clicked_run_id = Some(entry.run_id.clone());
                            }
                            ui.add_space(spacing::SM);
                        }
                    });
            }
        } else {
            // Empty state when no project is selected
            self.render_no_project_selected(ui);
        }

        clicked_run_id
    }

    /// Render loading indicator for run history.
    fn render_run_history_loading(&self, ui: &mut egui::Ui) {
        ui.add_space(spacing::LG);
        ui.vertical_centered(|ui| {
            // Custom spinner using theme accent color for visual consistency
            let spinner_size = 24.0;
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(spinner_size), egui::Sense::hover());

            if ui.is_rect_visible(rect) {
                // Draw a simple animated arc spinner in accent color
                let center = rect.center();
                let radius = spinner_size / 2.0 - 2.0;
                let time = ui.input(|i| i.time);
                let start_angle = (time * 2.0) as f32 % std::f32::consts::TAU;
                let arc_length = std::f32::consts::PI * 1.5;

                // Draw the spinner arc
                let n_points = 32;
                let points: Vec<_> = (0..=n_points)
                    .map(|i| {
                        let angle = start_angle + (i as f32 / n_points as f32) * arc_length;
                        egui::pos2(
                            center.x + radius * angle.cos(),
                            center.y + radius * angle.sin(),
                        )
                    })
                    .collect();

                ui.painter()
                    .add(egui::Shape::line(points, Stroke::new(2.5, colors::ACCENT)));

                // Request repaint for animation
                ui.ctx().request_repaint();
            }

            ui.add_space(spacing::SM);

            ui.label(
                egui::RichText::new("Loading run history...")
                    .font(typography::font(FontSize::Body, FontWeight::Regular))
                    .color(colors::TEXT_MUTED),
            );
        });
    }

    /// Render error state for run history.
    fn render_run_history_error(&self, ui: &mut egui::Ui, error: &str) {
        ui.add_space(spacing::LG);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new("Failed to load run history")
                    .font(typography::font(FontSize::Body, FontWeight::Medium))
                    .color(colors::STATUS_ERROR),
            );

            ui.add_space(spacing::XS);

            ui.label(
                egui::RichText::new(truncate_with_ellipsis(error, 60))
                    .font(typography::font(FontSize::Small, FontWeight::Regular))
                    .color(colors::TEXT_MUTED),
            );
        });
    }

    /// Render empty state when run history has no entries.
    fn render_run_history_empty(&self, ui: &mut egui::Ui) {
        ui.add_space(spacing::XXL);
        ui.vertical_centered(|ui| {
            ui.add_space(spacing::LG);

            ui.label(
                egui::RichText::new("No run history")
                    .font(typography::font(FontSize::Heading, FontWeight::Medium))
                    .color(colors::TEXT_MUTED),
            );

            ui.add_space(spacing::SM);

            ui.label(
                egui::RichText::new("Completed runs will appear here")
                    .font(typography::font(FontSize::Body, FontWeight::Regular))
                    .color(colors::TEXT_MUTED),
            );
        });
    }

    /// Render empty state when no project is selected.
    fn render_no_project_selected(&self, ui: &mut egui::Ui) {
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

    /// Render a single run history entry as a card.
    /// Returns true if the entry was clicked.
    fn render_run_history_entry(&self, ui: &mut egui::Ui, entry: &RunHistoryEntry) -> bool {
        // Card background - use consistent height from constants
        let available_width = ui.available_width();
        let card_height = 72.0; // Fixed height for history cards

        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(available_width, card_height), Sense::click());

        let is_hovered = response.hovered();

        // Draw card background with hover state - consistent with project row pattern
        // Uses SURFACE as default, SURFACE_HOVER on hover, and border feedback
        let bg_color = if is_hovered {
            colors::SURFACE_HOVER
        } else {
            colors::SURFACE
        };

        // Border changes on hover for visual feedback - consistent with project rows
        let border = if is_hovered {
            Stroke::new(1.0, colors::BORDER_FOCUSED)
        } else {
            Stroke::new(1.0, colors::BORDER)
        };

        ui.painter()
            .rect(rect, Rounding::same(rounding::CARD), bg_color, border);

        // Card content
        let inner_rect = rect.shrink(spacing::MD);
        let mut child_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner_rect)
                .layout(egui::Layout::top_down(egui::Align::LEFT)),
        );

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
                    badge_background_color(status_color),
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

        response.clicked()
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
            .id_salt("projects_left_panel")
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

// ============================================================================
// Viewport Configuration (Custom Title Bar - US-002)
// ============================================================================

/// Build the viewport configuration for the native window.
///
/// Configures a custom title bar that blends with the app's background color.
fn build_viewport() -> egui::ViewportBuilder {
    egui::ViewportBuilder::default()
        .with_title("autom8")
        .with_inner_size([DEFAULT_WIDTH, DEFAULT_HEIGHT])
        .with_min_inner_size([MIN_WIDTH, MIN_HEIGHT])
        .with_fullsize_content_view(true)
        .with_titlebar_shown(false)
        .with_title_shown(false)
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
        viewport: build_viewport(),
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
    use crate::state::SessionMetadata;
    use chrono::Utc;
    use std::path::PathBuf;

    // ========================================================================
    // App Initialization Tests
    // ========================================================================

    #[test]
    fn test_autom8_app_new_defaults_to_active_runs() {
        let app = Autom8App::new(None);
        assert_eq!(app.current_tab(), Tab::ActiveRuns);
    }

    #[test]
    fn test_autom8_app_new_with_filter() {
        let app = Autom8App::new(Some("test-project".to_string()));
        assert_eq!(app.project_filter, Some("test-project".to_string()));
    }

    #[test]
    fn test_app_with_custom_refresh_interval() {
        let interval = Duration::from_millis(100);
        let app = Autom8App::with_refresh_interval(None, interval);
        assert_eq!(app.refresh_interval(), interval);
    }

    // ========================================================================
    // Run Progress Tests
    // ========================================================================

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
        assert_eq!(
            RunProgress {
                completed: 2,
                total: 4
            }
            .as_percentage(),
            "50%"
        );
        assert_eq!(
            RunProgress {
                completed: 0,
                total: 0
            }
            .as_percentage(),
            "0%"
        );
        assert_eq!(
            RunProgress {
                completed: 5,
                total: 5
            }
            .as_percentage(),
            "100%"
        );
    }

    // ========================================================================
    // Session Data Tests
    // ========================================================================

    #[test]
    fn test_session_data_display_title() {
        let main_session = SessionData {
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
        assert_eq!(main_session.display_title(), "my-project (main)");

        let worktree_session = SessionData {
            is_main_session: false,
            metadata: SessionMetadata {
                session_id: "abc12345".to_string(),
                ..main_session.metadata.clone()
            },
            ..main_session
        };
        assert_eq!(worktree_session.display_title(), "my-project (abc12345)");
    }

    #[test]
    fn test_session_data_truncated_worktree_path() {
        let make_session = |path: &str| SessionData {
            project_name: "my-project".to_string(),
            metadata: SessionMetadata {
                session_id: "abc12345".to_string(),
                worktree_path: PathBuf::from(path),
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

        assert_eq!(make_session("project").truncated_worktree_path(), "project");

        let long_path = make_session("/Users/dev/projects/my-project-wt-feature");
        let truncated = long_path.truncated_worktree_path();
        assert!(truncated.starts_with("..."));
        assert!(truncated.contains("my-project-wt-feature"));
    }

    // ========================================================================
    // Grid Layout Tests
    // ========================================================================

    #[test]
    fn test_calculate_grid_columns() {
        assert_eq!(Autom8App::calculate_grid_columns(300.0), 2); // Very narrow
        assert_eq!(Autom8App::calculate_grid_columns(500.0), 2); // Narrow
        assert_eq!(Autom8App::calculate_grid_columns(900.0), 3); // Medium
        assert_eq!(Autom8App::calculate_grid_columns(1400.0), 4); // Wide
        assert_eq!(Autom8App::calculate_grid_columns(2000.0), 4); // Very wide (capped)
    }

    #[test]
    fn test_calculate_card_width() {
        // Normal cases
        let width_2col = Autom8App::calculate_card_width(600.0, 2);
        assert!(width_2col >= CARD_MIN_WIDTH && width_2col <= CARD_MAX_WIDTH);

        // Clamps to min
        assert_eq!(Autom8App::calculate_card_width(400.0, 4), CARD_MIN_WIDTH);

        // Clamps to max
        assert_eq!(Autom8App::calculate_card_width(1000.0, 2), CARD_MAX_WIDTH);
    }

    // ========================================================================
    // Projects View Tests
    // ========================================================================

    #[test]
    fn test_project_status_color() {
        let app = Autom8App::new(Some("nonexistent".to_string()));

        let make_project = |has_active_run, run_status, load_error| ProjectData {
            info: ProjectTreeInfo {
                name: "test".to_string(),
                has_active_run,
                run_status,
                spec_count: 1,
                incomplete_spec_count: 0,
                spec_md_count: 0,
                runs_count: 0,
                last_run_date: None,
            },
            active_run: None,
            progress: None,
            load_error,
        };

        assert_eq!(
            app.project_status_color(&make_project(
                true,
                Some(crate::state::RunStatus::Running),
                None
            )),
            colors::STATUS_RUNNING
        );
        assert_eq!(
            app.project_status_color(&make_project(false, None, None)),
            colors::STATUS_IDLE
        );
        assert_eq!(
            app.project_status_color(&make_project(false, None, Some("error".to_string()))),
            colors::STATUS_ERROR
        );
    }

    // ========================================================================
    // Project Selection Tests
    // ========================================================================

    #[test]
    fn test_toggle_project_selection() {
        let mut app = Autom8App::new(None);
        assert!(app.selected_project().is_none());

        app.toggle_project_selection("my-project");
        assert_eq!(app.selected_project(), Some("my-project"));

        // Toggle again to deselect
        app.toggle_project_selection("my-project");
        assert!(app.selected_project().is_none());

        // Select and switch
        app.toggle_project_selection("project-a");
        app.toggle_project_selection("project-b");
        assert_eq!(app.selected_project(), Some("project-b"));
    }

    // ========================================================================
    // Run History Tests
    // ========================================================================

    #[test]
    fn test_run_history_entry_from_run_state() {
        use crate::state::{IterationRecord, IterationStatus, RunState, RunStatus};

        let mut run = RunState::new(
            std::path::PathBuf::from("test.json"),
            "feature/test".to_string(),
        );
        run.status = RunStatus::Completed;
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
            finished_at: None,
            status: IterationStatus::Failed,
            output_snippet: String::new(),
            work_summary: None,
        });

        let entry = RunHistoryEntry::from_run_state("test-project".to_string(), &run);
        assert_eq!(entry.project_name, "test-project");
        assert_eq!(entry.branch, "feature/test");
        assert_eq!(entry.status, RunStatus::Completed);
        assert_eq!(entry.completed_stories, 1);
        assert_eq!(entry.total_stories, 2);
        assert_eq!(entry.story_count_text(), "1/2 stories");
        assert_eq!(entry.status_text(), "Completed");
        assert_eq!(entry.status_color(), colors::STATUS_SUCCESS);
    }

    // ========================================================================
    // Dynamic Tab System Tests
    // ========================================================================

    #[test]
    fn test_app_initial_tabs() {
        let app = Autom8App::new(None);
        assert_eq!(app.tab_count(), 2);
        assert_eq!(app.closable_tab_count(), 0);
        assert_eq!(*app.active_tab_id(), TabId::ActiveRuns);
    }

    #[test]
    fn test_app_open_and_close_tabs() {
        let mut app = Autom8App::new(None);

        // Open tabs
        assert!(app.open_run_detail_tab("run-1", "Run 1"));
        assert!(!app.open_run_detail_tab("run-1", "Run 1")); // No duplicate
        app.open_run_detail_tab("run-2", "Run 2");
        app.open_run_detail_tab("run-3", "Run 3");

        assert_eq!(app.tab_count(), 5);
        assert_eq!(app.closable_tab_count(), 3);
        assert!(app.has_tab(&TabId::RunDetail("run-1".to_string())));

        // Close one tab
        assert!(app.close_tab(&TabId::RunDetail("run-2".to_string())));
        assert_eq!(app.closable_tab_count(), 2);

        // Can't close permanent tabs
        assert!(!app.close_tab(&TabId::ActiveRuns));
        assert!(!app.close_tab(&TabId::Projects));

        // Close all dynamic tabs
        assert_eq!(app.close_all_dynamic_tabs(), 2);
        assert_eq!(app.tab_count(), 2);
    }

    #[test]
    fn test_app_close_active_tab_switches_to_previous() {
        let mut app = Autom8App::new(None);
        app.set_active_tab(TabId::Projects);
        app.open_run_detail_tab("run-123", "Run Details");

        assert_eq!(
            *app.active_tab_id(),
            TabId::RunDetail("run-123".to_string())
        );

        app.close_tab(&TabId::RunDetail("run-123".to_string()));
        assert_eq!(*app.active_tab_id(), TabId::Projects);
    }

    #[test]
    fn test_run_detail_cache() {
        use crate::state::RunState;

        let mut app = Autom8App::new(None);
        assert!(app.get_cached_run_state("run-123").is_none());

        let run = RunState::new(
            std::path::PathBuf::from("test.json"),
            "feature/test".to_string(),
        );
        let entry = RunHistoryEntry::from_run_state("test-project".to_string(), &run);
        app.open_run_detail_from_entry(&entry, Some(run.clone()));

        assert!(app.get_cached_run_state(&entry.run_id).is_some());

        app.close_tab(&TabId::RunDetail(entry.run_id.clone()));
        assert!(app.get_cached_run_state(&entry.run_id).is_none());
    }

    // ========================================================================
    // Duration Formatting Tests (app-specific format functions)
    // ========================================================================

    #[test]
    fn test_format_duration_detailed() {
        assert_eq!(
            Autom8App::format_duration_detailed(chrono::Duration::seconds(45)),
            "45s"
        );
        assert_eq!(
            Autom8App::format_duration_detailed(chrono::Duration::seconds(125)),
            "2m 5s"
        );
        assert_eq!(
            Autom8App::format_duration_detailed(chrono::Duration::seconds(3725)),
            "1h 2m 5s"
        );
        assert_eq!(
            Autom8App::format_duration_detailed(chrono::Duration::seconds(0)),
            "0s"
        );
        assert_eq!(
            Autom8App::format_duration_detailed(chrono::Duration::seconds(-100)),
            "0s"
        );
    }

    #[test]
    fn test_format_duration_short() {
        assert_eq!(
            Autom8App::format_duration_short(chrono::Duration::seconds(45)),
            "45s"
        );
        assert_eq!(
            Autom8App::format_duration_short(chrono::Duration::seconds(125)),
            "2m5s"
        );
        assert_eq!(
            Autom8App::format_duration_short(chrono::Duration::seconds(3725)),
            "1h2m"
        );
    }

    #[test]
    fn test_run_detail_tab_opens_from_history_entry() {
        use crate::state::{RunState, RunStatus};

        let mut app = Autom8App::new(None);
        let mut run = RunState::new(
            std::path::PathBuf::from("test.json"),
            "feature/test".to_string(),
        );
        run.status = RunStatus::Completed;

        let entry = RunHistoryEntry::from_run_state("test-project".to_string(), &run);
        app.open_run_detail_from_entry(&entry, Some(run.clone()));

        assert!(app.has_tab(&TabId::RunDetail(entry.run_id.clone())));
        assert_eq!(app.tab_count(), 3);
        assert_eq!(*app.active_tab_id(), TabId::RunDetail(entry.run_id.clone()));

        // Check label format
        let tab = app
            .tabs()
            .iter()
            .find(|t| t.id == TabId::RunDetail(entry.run_id.clone()))
            .unwrap();
        assert!(tab.label.starts_with("Run - "));
        assert!(tab.closable);
    }

    // ========================================================================
    // Sidebar Tests
    // ========================================================================

    #[test]
    fn test_sidebar_toggle() {
        let mut app = Autom8App::new(None);
        assert!(!app.is_sidebar_collapsed());

        app.toggle_sidebar();
        assert!(app.is_sidebar_collapsed());

        app.toggle_sidebar();
        assert!(!app.is_sidebar_collapsed());
    }
}
