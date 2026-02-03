//! GUI application entry point.
//!
//! This module contains the eframe application setup and main window
//! configuration for the autom8 GUI.

use crate::error::{Autom8Error, Result};
use crate::state::{MachineState, SessionStatus, StateManager};
use crate::ui::gui::components::{
    badge_background_color, format_duration, format_relative_time, format_state, state_to_color,
    truncate_with_ellipsis, MAX_BRANCH_LENGTH, MAX_TEXT_LENGTH,
};
use crate::ui::gui::theme::{self, colors, rounding, spacing};
use crate::ui::gui::typography::{self, FontSize, FontWeight};
use crate::ui::shared::{
    load_project_run_history, load_ui_data, ProjectData, RunHistoryEntry, SessionData,
};
use eframe::egui::{self, Color32, Key, Order, Pos2, Rect, Rounding, Sense, Stroke, Vec2};
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
/// Cards should take approximately 50% of available width.
const CARD_MIN_WIDTH: f32 = 400.0;

/// Maximum width for a card in the grid layout.
/// Allows cards to grow larger for better content display.
const CARD_MAX_WIDTH: f32 = 800.0;

/// Spacing between cards in the grid (uses XL from spacing scale for larger cards).
const CARD_SPACING: f32 = 24.0; // spacing::XL

/// Internal padding for cards (uses XL from spacing scale for larger cards).
const CARD_PADDING: f32 = 20.0; // Between LG and XL

/// Minimum height for a card.
/// Cards should take approximately 50% of available height.
const CARD_MIN_HEIGHT: f32 = 320.0;

/// Number of output lines to display in session cards.
/// Increased for better monitoring of streaming output.
const OUTPUT_LINES_TO_SHOW: usize = 12;

/// Maximum number of columns in the grid layout (2x2 grid for 1/4 screen each).
const MAX_GRID_COLUMNS: usize = 2;

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
// Context Menu Constants (Right-Click Context Menu - US-002)
// ============================================================================

/// Minimum width for the context menu.
const CONTEXT_MENU_MIN_WIDTH: f32 = 160.0;

/// Height of each menu item.
const CONTEXT_MENU_ITEM_HEIGHT: f32 = 32.0;

/// Horizontal padding for menu items.
const CONTEXT_MENU_PADDING_H: f32 = 12.0; // spacing::MD

/// Vertical padding for menu items.
const CONTEXT_MENU_PADDING_V: f32 = 6.0;

/// Size of the submenu arrow indicator.
const CONTEXT_MENU_ARROW_SIZE: f32 = 8.0;

/// Offset from cursor for menu positioning.
const CONTEXT_MENU_CURSOR_OFFSET: f32 = 2.0;

/// Horizontal gap between submenu and parent menu.
const CONTEXT_MENU_SUBMENU_GAP: f32 = 2.0;

/// Response from rendering a context menu item.
///
/// Contains information about user interaction with the item.
struct ContextMenuItemResponse {
    /// Whether the item was clicked.
    clicked: bool,
    /// Whether the item is currently hovered.
    hovered: bool,
    /// The screen-space rect of the item (for positioning submenus).
    rect: Rect,
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Check if a session is resumable.
///
/// A session is resumable if:
/// - It's not stale (worktree still exists)
/// - It's marked as running, OR
/// - It has a machine state that's not Idle or Completed
fn is_resumable_session(session: &SessionStatus) -> bool {
    // Can't resume stale sessions (deleted worktrees)
    if session.is_stale {
        return false;
    }

    // Running sessions are resumable
    if session.metadata.is_running {
        return true;
    }

    // Check if the machine state indicates a resumable run
    if let Some(state) = &session.machine_state {
        match state {
            MachineState::Completed | MachineState::Idle => false,
            _ => true, // Any other state is resumable
        }
    } else {
        false
    }
}

// ============================================================================
// Context Menu Types (Right-Click Context Menu - US-002)
// ============================================================================

/// Menu item in the context menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextMenuItem {
    /// A simple action item.
    Action {
        /// Display label for the menu item.
        label: String,
        /// Unique identifier for the action.
        action: ContextMenuAction,
        /// Whether the item is enabled.
        enabled: bool,
    },
    /// A separator line between items.
    Separator,
    /// An item that opens a submenu.
    Submenu {
        /// Display label for the submenu trigger.
        label: String,
        /// Unique identifier for the submenu.
        id: String,
        /// Whether the submenu is enabled.
        enabled: bool,
        /// Items in the submenu (built lazily when opened).
        items: Vec<ContextMenuItem>,
    },
}

impl ContextMenuItem {
    /// Create a new action menu item.
    pub fn action(label: impl Into<String>, action: ContextMenuAction) -> Self {
        Self::Action {
            label: label.into(),
            action,
            enabled: true,
        }
    }

    /// Create a disabled action menu item.
    pub fn action_disabled(label: impl Into<String>, action: ContextMenuAction) -> Self {
        Self::Action {
            label: label.into(),
            action,
            enabled: false,
        }
    }

    /// Create a separator.
    pub fn separator() -> Self {
        Self::Separator
    }

    /// Create a submenu item.
    pub fn submenu(
        label: impl Into<String>,
        id: impl Into<String>,
        items: Vec<ContextMenuItem>,
    ) -> Self {
        let items_vec = items;
        Self::Submenu {
            label: label.into(),
            id: id.into(),
            enabled: !items_vec.is_empty(),
            items: items_vec,
        }
    }

    /// Create a disabled submenu item.
    pub fn submenu_disabled(label: impl Into<String>, id: impl Into<String>) -> Self {
        Self::Submenu {
            label: label.into(),
            id: id.into(),
            enabled: false,
            items: Vec::new(),
        }
    }
}

/// Actions that can be triggered from the context menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextMenuAction {
    /// Run the status command for the project.
    Status,
    /// Run the describe command for the project.
    Describe,
    /// Resume a specific session (with session ID).
    Resume(Option<String>),
    /// Clean worktrees for the project.
    CleanWorktrees,
    /// Clean orphaned sessions for the project.
    CleanOrphaned,
}

/// Information about a resumable session for display in the context menu.
/// This is a simplified view of SessionStatus for the GUI.
#[derive(Debug, Clone)]
pub struct ResumableSessionInfo {
    /// The session ID (e.g., "main" or 8-char hash).
    pub session_id: String,
    /// The branch name being worked on.
    pub branch_name: String,
}

impl ResumableSessionInfo {
    /// Create a new resumable session info.
    pub fn new(session_id: impl Into<String>, branch_name: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            branch_name: branch_name.into(),
        }
    }

    /// Returns a truncated version of the session ID (first 8 chars).
    pub fn truncated_id(&self) -> &str {
        if self.session_id.len() > 8 {
            &self.session_id[..8]
        } else {
            &self.session_id
        }
    }

    /// Returns the menu label for this session.
    /// Format: "branch-name (session-id-truncated)"
    pub fn menu_label(&self) -> String {
        format!("{} ({})", self.branch_name, self.truncated_id())
    }
}

/// Information about cleanable sessions for the Clean context menu.
/// Contains counts for worktrees and orphaned sessions.
#[derive(Debug, Clone, Default)]
pub struct CleanableInfo {
    /// Number of cleanable worktrees (Completed/Failed/Interrupted sessions with existing worktrees).
    pub cleanable_worktrees: usize,
    /// Number of orphaned sessions (worktree deleted but session state remains).
    pub orphaned_sessions: usize,
}

impl CleanableInfo {
    /// Returns true if there's anything to clean.
    pub fn has_cleanable(&self) -> bool {
        self.cleanable_worktrees > 0 || self.orphaned_sessions > 0
    }
}

/// Check if a session is cleanable (Completed, Failed, or Interrupted).
///
/// A session is cleanable if it's NOT running or in-progress.
/// Running/InProgress sessions should be preserved for safety.
fn is_cleanable_session(session: &SessionStatus) -> bool {
    // Can't clean sessions that are actively running
    if session.metadata.is_running {
        return false;
    }

    // Check machine state to determine if cleanable
    if let Some(state) = &session.machine_state {
        match state {
            // Cleanable states: completed work
            MachineState::Completed | MachineState::Failed | MachineState::Idle => true,
            // Running states: do not clean
            MachineState::Initializing
            | MachineState::LoadingSpec
            | MachineState::GeneratingSpec
            | MachineState::PickingStory
            | MachineState::RunningClaude
            | MachineState::Reviewing
            | MachineState::Correcting
            | MachineState::Committing
            | MachineState::CreatingPR => false,
        }
    } else {
        // No machine state - treat as cleanable (likely orphaned or corrupted)
        true
    }
}

/// State for the context menu overlay.
#[derive(Debug, Clone)]
pub struct ContextMenuState {
    /// Screen position where the menu should appear.
    pub position: Pos2,
    /// Name of the project this menu is for.
    pub project_name: String,
    /// The menu items to display.
    pub items: Vec<ContextMenuItem>,
    /// Currently open submenu ID (if any).
    pub open_submenu: Option<String>,
    /// Position of the open submenu (if any).
    pub submenu_position: Option<Pos2>,
}

impl ContextMenuState {
    /// Create a new context menu state.
    pub fn new(position: Pos2, project_name: String, items: Vec<ContextMenuItem>) -> Self {
        Self {
            position,
            project_name,
            items,
            open_submenu: None,
            submenu_position: None,
        }
    }

    /// Open a submenu at the given position.
    pub fn open_submenu(&mut self, id: String, position: Pos2) {
        self.open_submenu = Some(id);
        self.submenu_position = Some(position);
    }

    /// Close any open submenu.
    pub fn close_submenu(&mut self) {
        self.open_submenu = None;
        self.submenu_position = None;
    }
}

/// Result of a project row interaction.
/// Contains information about both left-click and right-click events.
#[derive(Debug, Clone, Default)]
pub struct ProjectRowInteraction {
    /// True if the row was left-clicked (select project).
    pub clicked: bool,
    /// If right-clicked, contains the screen position for context menu.
    pub right_click_pos: Option<Pos2>,
}

impl ProjectRowInteraction {
    /// Create a new interaction with no events.
    pub fn none() -> Self {
        Self::default()
    }

    /// Create a left-click interaction.
    pub fn click() -> Self {
        Self {
            clicked: true,
            right_click_pos: None,
        }
    }

    /// Create a right-click interaction at the given position.
    pub fn right_click(pos: Pos2) -> Self {
        Self {
            clicked: false,
            right_click_pos: Some(pos),
        }
    }
}

// ============================================================================
// Command Output Types (Command Output Tab - US-007)
// ============================================================================

/// Status of a command execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandStatus {
    /// Command is currently running.
    Running,
    /// Command completed successfully (exit code 0).
    Completed,
    /// Command failed (non-zero exit code or error).
    Failed,
}

/// Identifier for a command output, used for tab matching and cache lookup.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CommandOutputId {
    /// Name of the project the command was run for.
    pub project: String,
    /// Name of the command (e.g., "status", "describe").
    pub command: String,
    /// Unique identifier for this command execution (UUID).
    pub id: String,
}

impl CommandOutputId {
    /// Create a new command output ID.
    pub fn new(project: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            project: project.into(),
            command: command.into(),
            id: uuid::Uuid::new_v4().to_string(),
        }
    }

    /// Create a command output ID with a specific ID (for testing).
    #[cfg(test)]
    pub fn with_id(
        project: impl Into<String>,
        command: impl Into<String>,
        id: impl Into<String>,
    ) -> Self {
        Self {
            project: project.into(),
            command: command.into(),
            id: id.into(),
        }
    }

    /// Returns the cache key for this command output.
    pub fn cache_key(&self) -> String {
        format!("{}:{}:{}", self.project, self.command, self.id)
    }

    /// Returns the tab label for this command output.
    pub fn tab_label(&self) -> String {
        // Capitalize first letter of command
        let command_display = if self.command.is_empty() {
            "Command".to_string()
        } else {
            let mut chars = self.command.chars();
            match chars.next() {
                None => "Command".to_string(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        };
        format!("{}: {}", command_display, self.project)
    }
}

/// State of a command execution for display in a tab.
#[derive(Debug, Clone)]
pub struct CommandExecution {
    /// The command output identifier.
    pub id: CommandOutputId,
    /// Current status of the command.
    pub status: CommandStatus,
    /// Lines of stdout output.
    pub stdout: Vec<String>,
    /// Lines of stderr output.
    pub stderr: Vec<String>,
    /// Exit code if the command has finished.
    pub exit_code: Option<i32>,
    /// Whether auto-scroll is enabled (scroll to bottom on new output).
    pub auto_scroll: bool,
}

impl CommandExecution {
    /// Create a new command execution in the running state.
    pub fn new(id: CommandOutputId) -> Self {
        Self {
            id,
            status: CommandStatus::Running,
            stdout: Vec::new(),
            stderr: Vec::new(),
            exit_code: None,
            auto_scroll: true,
        }
    }

    /// Add a line to stdout.
    pub fn add_stdout(&mut self, line: String) {
        self.stdout.push(line);
    }

    /// Add a line to stderr.
    pub fn add_stderr(&mut self, line: String) {
        self.stderr.push(line);
    }

    /// Mark the command as completed with the given exit code.
    pub fn complete(&mut self, exit_code: i32) {
        self.exit_code = Some(exit_code);
        self.status = if exit_code == 0 {
            CommandStatus::Completed
        } else {
            CommandStatus::Failed
        };
    }

    /// Mark the command as failed (e.g., spawn error).
    pub fn fail(&mut self, error_message: String) {
        self.stderr.push(error_message);
        self.status = CommandStatus::Failed;
    }

    /// Returns true if the command is still running.
    pub fn is_running(&self) -> bool {
        self.status == CommandStatus::Running
    }

    /// Returns true if the command has completed (successfully or not).
    pub fn is_finished(&self) -> bool {
        self.status != CommandStatus::Running
    }

    /// Returns the combined output (stdout + stderr interleaved would require timestamps,
    /// so we return stdout followed by stderr).
    pub fn combined_output(&self) -> Vec<&str> {
        let mut output: Vec<&str> = self.stdout.iter().map(|s| s.as_str()).collect();
        if !self.stderr.is_empty() {
            output.extend(self.stderr.iter().map(|s| s.as_str()));
        }
        output
    }
}

// ============================================================================
// Command Message Types (for async command execution)
// ============================================================================

/// Message sent from background command execution threads to the UI.
#[derive(Debug, Clone)]
pub enum CommandMessage {
    /// A line of stdout output.
    Stdout { cache_key: String, line: String },
    /// A line of stderr output.
    Stderr { cache_key: String, line: String },
    /// Command completed with exit code.
    Completed { cache_key: String, exit_code: i32 },
    /// Command failed to spawn or encountered an error.
    Failed { cache_key: String, error: String },
}

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
            crate::state::RunStatus::Interrupted => colors::STATUS_WARNING,
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
    /// A dynamic tab for viewing command output.
    /// Contains the cache key (project:command:id) as identifier.
    CommandOutput(String),
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

    // ========================================================================
    // Context Menu State (Right-Click Context Menu - US-002)
    // ========================================================================
    /// State for the right-click context menu overlay.
    /// When Some, a context menu is displayed at the specified position.
    /// Only one context menu can be open at a time.
    context_menu: Option<ContextMenuState>,

    // ========================================================================
    // Command Execution State (Command Output Tab - US-007)
    // ========================================================================
    /// Cached command executions for open command output tabs.
    /// Maps cache_key (project:command:id) to the command execution state.
    command_executions: std::collections::HashMap<String, CommandExecution>,

    // ========================================================================
    // Command Channel (for async command execution - US-003)
    // ========================================================================
    /// Receiver for command execution messages from background threads.
    /// The sender is cloned and moved to each background thread.
    command_rx: std::sync::mpsc::Receiver<CommandMessage>,
    /// Sender for command execution messages.
    /// Cloned for each background command thread.
    command_tx: std::sync::mpsc::Sender<CommandMessage>,
}

impl Default for Autom8App {
    fn default() -> Self {
        Self::new()
    }
}

impl Autom8App {
    /// Create a new application instance.
    pub fn new() -> Self {
        Self::with_refresh_interval(Duration::from_millis(DEFAULT_REFRESH_INTERVAL_MS))
    }

    /// Create a new application instance with a custom refresh interval.
    ///
    /// # Arguments
    ///
    /// * `refresh_interval` - How often to refresh data from disk
    pub fn with_refresh_interval(refresh_interval: Duration) -> Self {
        // Initialize permanent tabs
        let tabs = vec![
            TabInfo::permanent(TabId::ActiveRuns, "Active Runs"),
            TabInfo::permanent(TabId::Projects, "Projects"),
        ];

        // Create channel for command execution messages
        let (command_tx, command_rx) = std::sync::mpsc::channel();

        let mut app = Self {
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
            context_menu: None,
            command_executions: std::collections::HashMap::new(),
            command_rx,
            command_tx,
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

    // ========================================================================
    // Context Menu State (Right-Click Context Menu - US-002)
    // ========================================================================

    /// Returns whether the context menu is currently open.
    pub fn is_context_menu_open(&self) -> bool {
        self.context_menu.is_some()
    }

    /// Returns a reference to the context menu state, if open.
    pub fn context_menu(&self) -> Option<&ContextMenuState> {
        self.context_menu.as_ref()
    }

    /// Open the context menu for a project at the given position.
    pub fn open_context_menu(&mut self, position: Pos2, project_name: String) {
        // Build the menu items for this project
        let items = self.build_context_menu_items(&project_name);

        self.context_menu = Some(ContextMenuState::new(position, project_name, items));
    }

    /// Close the context menu.
    pub fn close_context_menu(&mut self) {
        self.context_menu = None;
    }

    /// Get resumable sessions for a project.
    ///
    /// Queries the StateManager for sessions that can be resumed:
    /// - Session is not stale (worktree still exists)
    /// - Session is_running, OR
    /// - Session has a machine state that's not Idle/Completed
    ///
    /// Returns sessions sorted by last_active_at descending.
    fn get_resumable_sessions(&self, project_name: &str) -> Vec<ResumableSessionInfo> {
        // Try to get the state manager for this project
        let sm = match StateManager::for_project(project_name) {
            Ok(sm) => sm,
            Err(_) => return Vec::new(),
        };

        // Get all sessions with status
        let sessions = match sm.list_sessions_with_status() {
            Ok(sessions) => sessions,
            Err(_) => return Vec::new(),
        };

        // Filter to resumable sessions and convert to ResumableSessionInfo
        sessions
            .into_iter()
            .filter(is_resumable_session)
            .map(|s| ResumableSessionInfo::new(s.metadata.session_id, s.metadata.branch_name))
            .collect()
    }

    /// Get cleanable session information for a project.
    ///
    /// Returns counts for:
    /// - cleanable_worktrees: sessions with Completed/Failed/Interrupted status
    ///   where the worktree still exists
    /// - orphaned_sessions: sessions where the worktree was deleted but state remains
    ///
    /// Safety: Running/InProgress sessions are NOT counted as cleanable.
    fn get_cleanable_info(&self, project_name: &str) -> CleanableInfo {
        // Try to get the state manager for this project
        let sm = match StateManager::for_project(project_name) {
            Ok(sm) => sm,
            Err(_) => return CleanableInfo::default(),
        };

        // Get all sessions with status
        let sessions = match sm.list_sessions_with_status() {
            Ok(sessions) => sessions,
            Err(_) => return CleanableInfo::default(),
        };

        let mut info = CleanableInfo::default();

        for session in sessions {
            if session.is_stale {
                // Orphaned session: worktree was deleted
                info.orphaned_sessions += 1;
            } else if is_cleanable_session(&session) {
                // Cleanable worktree: not running/in-progress
                info.cleanable_worktrees += 1;
            }
        }

        info
    }

    /// Build the context menu items for a project.
    /// This creates the menu structure with Status, Describe, Resume, and Clean options.
    fn build_context_menu_items(&self, project_name: &str) -> Vec<ContextMenuItem> {
        // Get resumable sessions for this project
        let resumable_sessions = self.get_resumable_sessions(project_name);

        // Build the Resume menu item based on number of sessions
        let resume_item = match resumable_sessions.len() {
            0 => {
                // No resumable sessions - disabled menu item
                ContextMenuItem::action_disabled("Resume", ContextMenuAction::Resume(None))
            }
            1 => {
                // Single session - direct action with branch name
                let session = &resumable_sessions[0];
                let label = format!("Resume ({})", session.branch_name);
                ContextMenuItem::action(
                    label,
                    ContextMenuAction::Resume(Some(session.session_id.clone())),
                )
            }
            _ => {
                // Multiple sessions - submenu
                let submenu_items: Vec<ContextMenuItem> = resumable_sessions
                    .iter()
                    .map(|session| {
                        ContextMenuItem::action(
                            session.menu_label(),
                            ContextMenuAction::Resume(Some(session.session_id.clone())),
                        )
                    })
                    .collect();
                ContextMenuItem::submenu("Resume", "resume", submenu_items)
            }
        };

        // Get cleanable info for this project (US-006)
        let cleanable_info = self.get_cleanable_info(project_name);

        // Build the Clean menu item based on cleanable info
        let clean_item = if !cleanable_info.has_cleanable() {
            // Nothing to clean - disabled menu item with tooltip hint
            ContextMenuItem::submenu_disabled("Clean", "clean")
        } else {
            // Build submenu with only applicable options (showing counts)
            let mut submenu_items = Vec::new();

            if cleanable_info.cleanable_worktrees > 0 {
                let label = format!("Worktrees ({})", cleanable_info.cleanable_worktrees);
                submenu_items.push(ContextMenuItem::action(
                    label,
                    ContextMenuAction::CleanWorktrees,
                ));
            }

            if cleanable_info.orphaned_sessions > 0 {
                let label = format!("Orphaned ({})", cleanable_info.orphaned_sessions);
                submenu_items.push(ContextMenuItem::action(
                    label,
                    ContextMenuAction::CleanOrphaned,
                ));
            }

            ContextMenuItem::submenu("Clean", "clean", submenu_items)
        };

        vec![
            ContextMenuItem::action("Status", ContextMenuAction::Status),
            ContextMenuItem::action("Describe", ContextMenuAction::Describe),
            ContextMenuItem::Separator,
            resume_item,
            ContextMenuItem::Separator,
            clean_item,
        ]
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

        // Use shared function to load run history
        match load_project_run_history(project_name) {
            Ok(history) => {
                self.run_history = history;
            }
            Err(e) => {
                self.run_history_error = Some(format!("Failed to load run history: {}", e));
            }
        }

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
            TabId::RunDetail(_) | TabId::CommandOutput(_) => {
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

    /// Open a new command output tab.
    /// Creates a new CommandExecution and opens a tab for it.
    /// Returns the CommandOutputId for the new execution (to be used for updates).
    pub fn open_command_output_tab(&mut self, project: &str, command: &str) -> CommandOutputId {
        let id = CommandOutputId::new(project, command);
        let cache_key = id.cache_key();
        let tab_id = TabId::CommandOutput(cache_key.clone());
        let label = id.tab_label();

        // Create the command execution
        let execution = CommandExecution::new(id.clone());
        self.command_executions.insert(cache_key, execution);

        // Create and activate the tab
        let tab = TabInfo::closable(tab_id.clone(), label);
        self.tabs.push(tab);
        self.set_active_tab(tab_id);

        id
    }

    /// Get a command execution by cache key.
    pub fn get_command_execution(&self, cache_key: &str) -> Option<&CommandExecution> {
        self.command_executions.get(cache_key)
    }

    /// Get a mutable command execution by cache key.
    pub fn get_command_execution_mut(&mut self, cache_key: &str) -> Option<&mut CommandExecution> {
        self.command_executions.get_mut(cache_key)
    }

    /// Update a command execution with new stdout output.
    pub fn add_command_stdout(&mut self, cache_key: &str, line: String) {
        if let Some(exec) = self.command_executions.get_mut(cache_key) {
            exec.add_stdout(line);
        }
    }

    /// Update a command execution with new stderr output.
    pub fn add_command_stderr(&mut self, cache_key: &str, line: String) {
        if let Some(exec) = self.command_executions.get_mut(cache_key) {
            exec.add_stderr(line);
        }
    }

    /// Mark a command execution as completed.
    pub fn complete_command(&mut self, cache_key: &str, exit_code: i32) {
        if let Some(exec) = self.command_executions.get_mut(cache_key) {
            exec.complete(exit_code);
        }
    }

    /// Mark a command execution as failed.
    pub fn fail_command(&mut self, cache_key: &str, error_message: String) {
        if let Some(exec) = self.command_executions.get_mut(cache_key) {
            exec.fail(error_message);
        }
    }

    /// Spawn the `autom8 status --project <name>` command in a background thread.
    /// Opens a new command output tab and streams output to it.
    pub fn spawn_status_command(&mut self, project_name: &str) {
        // Open the tab first to get the cache key
        let id = self.open_command_output_tab(project_name, "status");
        let cache_key = id.cache_key();
        let tx = self.command_tx.clone();
        let project = project_name.to_string();

        std::thread::spawn(move || {
            use std::io::{BufRead, BufReader};
            use std::process::{Command, Stdio};

            // Spawn the autom8 status command (--all shows all sessions for the project)
            let result = Command::new("autom8")
                .arg("status")
                .arg("--all")
                .arg("--project")
                .arg(&project)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn();

            match result {
                Ok(mut child) => {
                    // Read stdout in a separate thread
                    let stdout = child.stdout.take();
                    let stderr = child.stderr.take();
                    let tx_stdout = tx.clone();
                    let tx_stderr = tx.clone();
                    let cache_key_stdout = cache_key.clone();
                    let cache_key_stderr = cache_key.clone();

                    let stdout_handle = std::thread::spawn(move || {
                        if let Some(stdout) = stdout {
                            let reader = BufReader::new(stdout);
                            for line in reader.lines().map_while(|r| r.ok()) {
                                let _ = tx_stdout.send(CommandMessage::Stdout {
                                    cache_key: cache_key_stdout.clone(),
                                    line,
                                });
                            }
                        }
                    });

                    let stderr_handle = std::thread::spawn(move || {
                        if let Some(stderr) = stderr {
                            let reader = BufReader::new(stderr);
                            for line in reader.lines().map_while(|r| r.ok()) {
                                let _ = tx_stderr.send(CommandMessage::Stderr {
                                    cache_key: cache_key_stderr.clone(),
                                    line,
                                });
                            }
                        }
                    });

                    // Wait for output threads to finish
                    let _ = stdout_handle.join();
                    let _ = stderr_handle.join();

                    // Wait for the child process to exit
                    match child.wait() {
                        Ok(status) => {
                            let exit_code = status.code().unwrap_or(-1);
                            let _ = tx.send(CommandMessage::Completed {
                                cache_key,
                                exit_code,
                            });
                        }
                        Err(e) => {
                            let _ = tx.send(CommandMessage::Failed {
                                cache_key,
                                error: format!("Failed to wait for process: {}", e),
                            });
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(CommandMessage::Failed {
                        cache_key,
                        error: format!("Failed to spawn autom8: {}", e),
                    });
                }
            }
        });
    }

    /// Spawn the `autom8 describe --project <name>` command in a background thread.
    /// Opens a new command output tab and streams output to it.
    pub fn spawn_describe_command(&mut self, project_name: &str) {
        // Open the tab first to get the cache key
        let id = self.open_command_output_tab(project_name, "describe");
        let cache_key = id.cache_key();
        let tx = self.command_tx.clone();
        let project = project_name.to_string();

        std::thread::spawn(move || {
            use std::io::{BufRead, BufReader};
            use std::process::{Command, Stdio};

            // Spawn the autom8 describe command (project name is a positional argument)
            let result = Command::new("autom8")
                .arg("describe")
                .arg(&project)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn();

            match result {
                Ok(mut child) => {
                    // Read stdout in a separate thread
                    let stdout = child.stdout.take();
                    let stderr = child.stderr.take();
                    let tx_stdout = tx.clone();
                    let tx_stderr = tx.clone();
                    let cache_key_stdout = cache_key.clone();
                    let cache_key_stderr = cache_key.clone();

                    let stdout_handle = std::thread::spawn(move || {
                        if let Some(stdout) = stdout {
                            let reader = BufReader::new(stdout);
                            for line in reader.lines().map_while(|r| r.ok()) {
                                let _ = tx_stdout.send(CommandMessage::Stdout {
                                    cache_key: cache_key_stdout.clone(),
                                    line,
                                });
                            }
                        }
                    });

                    let stderr_handle = std::thread::spawn(move || {
                        if let Some(stderr) = stderr {
                            let reader = BufReader::new(stderr);
                            for line in reader.lines().map_while(|r| r.ok()) {
                                let _ = tx_stderr.send(CommandMessage::Stderr {
                                    cache_key: cache_key_stderr.clone(),
                                    line,
                                });
                            }
                        }
                    });

                    // Wait for output threads to finish
                    let _ = stdout_handle.join();
                    let _ = stderr_handle.join();

                    // Wait for the child process to exit
                    match child.wait() {
                        Ok(status) => {
                            let exit_code = status.code().unwrap_or(-1);
                            let _ = tx.send(CommandMessage::Completed {
                                cache_key,
                                exit_code,
                            });
                        }
                        Err(e) => {
                            let _ = tx.send(CommandMessage::Failed {
                                cache_key,
                                error: format!("Failed to wait for process: {}", e),
                            });
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(CommandMessage::Failed {
                        cache_key,
                        error: format!("Failed to spawn autom8: {}", e),
                    });
                }
            }
        });
    }

    /// Spawn the `autom8 clean --worktrees --project <name>` command in a background thread.
    /// Opens a new command output tab and streams output to it.
    /// Note: The clean command respects safety filters - only Completed/Failed/Interrupted sessions
    /// are cleaned, not Running/InProgress ones.
    pub fn spawn_clean_worktrees_command(&mut self, project_name: &str) {
        // Open the tab first to get the cache key
        let id = self.open_command_output_tab(project_name, "clean-worktrees");
        let cache_key = id.cache_key();
        let tx = self.command_tx.clone();
        let project = project_name.to_string();

        std::thread::spawn(move || {
            use std::io::{BufRead, BufReader};
            use std::process::{Command, Stdio};

            // Spawn the autom8 clean --worktrees command
            let result = Command::new("autom8")
                .arg("clean")
                .arg("--worktrees")
                .arg("--project")
                .arg(&project)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn();

            match result {
                Ok(mut child) => {
                    // Read stdout in a separate thread
                    let stdout = child.stdout.take();
                    let stderr = child.stderr.take();
                    let tx_stdout = tx.clone();
                    let tx_stderr = tx.clone();
                    let cache_key_stdout = cache_key.clone();
                    let cache_key_stderr = cache_key.clone();

                    let stdout_handle = std::thread::spawn(move || {
                        if let Some(stdout) = stdout {
                            let reader = BufReader::new(stdout);
                            for line in reader.lines().map_while(|r| r.ok()) {
                                let _ = tx_stdout.send(CommandMessage::Stdout {
                                    cache_key: cache_key_stdout.clone(),
                                    line,
                                });
                            }
                        }
                    });

                    let stderr_handle = std::thread::spawn(move || {
                        if let Some(stderr) = stderr {
                            let reader = BufReader::new(stderr);
                            for line in reader.lines().map_while(|r| r.ok()) {
                                let _ = tx_stderr.send(CommandMessage::Stderr {
                                    cache_key: cache_key_stderr.clone(),
                                    line,
                                });
                            }
                        }
                    });

                    // Wait for output threads to finish
                    let _ = stdout_handle.join();
                    let _ = stderr_handle.join();

                    // Wait for the child process to exit
                    match child.wait() {
                        Ok(status) => {
                            let exit_code = status.code().unwrap_or(-1);
                            let _ = tx.send(CommandMessage::Completed {
                                cache_key,
                                exit_code,
                            });
                        }
                        Err(e) => {
                            let _ = tx.send(CommandMessage::Failed {
                                cache_key,
                                error: format!("Failed to wait for process: {}", e),
                            });
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(CommandMessage::Failed {
                        cache_key,
                        error: format!("Failed to spawn autom8: {}", e),
                    });
                }
            }
        });
    }

    /// Spawn the `autom8 clean --orphaned --project <name>` command in a background thread.
    /// Opens a new command output tab and streams output to it.
    pub fn spawn_clean_orphaned_command(&mut self, project_name: &str) {
        // Open the tab first to get the cache key
        let id = self.open_command_output_tab(project_name, "clean-orphaned");
        let cache_key = id.cache_key();
        let tx = self.command_tx.clone();
        let project = project_name.to_string();

        std::thread::spawn(move || {
            use std::io::{BufRead, BufReader};
            use std::process::{Command, Stdio};

            // Spawn the autom8 clean --orphaned command
            let result = Command::new("autom8")
                .arg("clean")
                .arg("--orphaned")
                .arg("--project")
                .arg(&project)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn();

            match result {
                Ok(mut child) => {
                    // Read stdout in a separate thread
                    let stdout = child.stdout.take();
                    let stderr = child.stderr.take();
                    let tx_stdout = tx.clone();
                    let tx_stderr = tx.clone();
                    let cache_key_stdout = cache_key.clone();
                    let cache_key_stderr = cache_key.clone();

                    let stdout_handle = std::thread::spawn(move || {
                        if let Some(stdout) = stdout {
                            let reader = BufReader::new(stdout);
                            for line in reader.lines().map_while(|r| r.ok()) {
                                let _ = tx_stdout.send(CommandMessage::Stdout {
                                    cache_key: cache_key_stdout.clone(),
                                    line,
                                });
                            }
                        }
                    });

                    let stderr_handle = std::thread::spawn(move || {
                        if let Some(stderr) = stderr {
                            let reader = BufReader::new(stderr);
                            for line in reader.lines().map_while(|r| r.ok()) {
                                let _ = tx_stderr.send(CommandMessage::Stderr {
                                    cache_key: cache_key_stderr.clone(),
                                    line,
                                });
                            }
                        }
                    });

                    // Wait for output threads to finish
                    let _ = stdout_handle.join();
                    let _ = stderr_handle.join();

                    // Wait for the child process to exit
                    match child.wait() {
                        Ok(status) => {
                            let exit_code = status.code().unwrap_or(-1);
                            let _ = tx.send(CommandMessage::Completed {
                                cache_key,
                                exit_code,
                            });
                        }
                        Err(e) => {
                            let _ = tx.send(CommandMessage::Failed {
                                cache_key,
                                error: format!("Failed to wait for process: {}", e),
                            });
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(CommandMessage::Failed {
                        cache_key,
                        error: format!("Failed to spawn autom8: {}", e),
                    });
                }
            }
        });
    }

    /// Poll for command execution messages and update state.
    /// This should be called in the update loop to process messages from background threads.
    fn poll_command_messages(&mut self) {
        // Process all pending messages (non-blocking)
        while let Ok(msg) = self.command_rx.try_recv() {
            match msg {
                CommandMessage::Stdout { cache_key, line } => {
                    self.add_command_stdout(&cache_key, line);
                }
                CommandMessage::Stderr { cache_key, line } => {
                    self.add_command_stderr(&cache_key, line);
                }
                CommandMessage::Completed {
                    cache_key,
                    exit_code,
                } => {
                    self.complete_command(&cache_key, exit_code);
                }
                CommandMessage::Failed { cache_key, error } => {
                    self.fail_command(&cache_key, error);
                }
            }
        }
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

        // Clean up command execution state if it's a command output tab
        if let TabId::CommandOutput(cache_key) = tab_id {
            self.command_executions.remove(cache_key);
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

        // Use shared data loading function (swallow errors, use defaults)
        // No project filter - always show all projects
        let ui_data = load_ui_data(None).unwrap_or_default();

        self.projects = ui_data.projects;
        self.sessions = ui_data.sessions;
        self.has_active_runs = ui_data.has_active_runs;
    }
}

impl eframe::App for Autom8App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Refresh data from disk if interval has elapsed
        self.maybe_refresh();

        // Poll for command execution messages from background threads
        self.poll_command_messages();

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

        // Handle global keyboard shortcuts for context menu
        if self.context_menu.is_some() {
            // Close context menu on Escape key
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                self.close_context_menu();
            }
        }

        // Render context menu overlay (must be after content to appear on top)
        self.render_context_menu(ctx);
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

    // ========================================================================
    // Context Menu Rendering (Right-Click Context Menu - US-002)
    // ========================================================================

    /// Render the context menu overlay.
    ///
    /// This method renders the context menu as a floating panel at the stored position.
    /// The menu is rendered on top of all other content using `Order::Foreground`.
    /// Handles click-outside-to-close and menu item interactions.
    /// Also handles submenu rendering for items like Resume and Clean (US-005, US-006).
    fn render_context_menu(&mut self, ctx: &egui::Context) {
        // Early return if no context menu is open
        let menu_state = match &self.context_menu {
            Some(state) => state.clone(),
            None => return,
        };

        // Get screen rect for bounds checking
        let screen_rect = ctx.screen_rect();

        // Calculate menu dimensions
        let menu_width = CONTEXT_MENU_MIN_WIDTH;
        let item_count = menu_state
            .items
            .iter()
            .filter(|item| !matches!(item, ContextMenuItem::Separator))
            .count();
        let separator_count = menu_state
            .items
            .iter()
            .filter(|item| matches!(item, ContextMenuItem::Separator))
            .count();
        let menu_height = (item_count as f32 * CONTEXT_MENU_ITEM_HEIGHT)
            + (separator_count as f32 * (spacing::SM + 1.0))
            + (CONTEXT_MENU_PADDING_V * 2.0);

        // Constrain menu position within window bounds
        let mut menu_pos = menu_state.position;
        menu_pos.x += CONTEXT_MENU_CURSOR_OFFSET;
        menu_pos.y += CONTEXT_MENU_CURSOR_OFFSET;

        // Ensure menu doesn't go off the right edge
        if menu_pos.x + menu_width > screen_rect.max.x - spacing::SM {
            menu_pos.x = screen_rect.max.x - menu_width - spacing::SM;
        }

        // Ensure menu doesn't go off the bottom edge
        if menu_pos.y + menu_height > screen_rect.max.y - spacing::SM {
            menu_pos.y = screen_rect.max.y - menu_height - spacing::SM;
        }

        // Ensure menu doesn't go off the left or top edge
        menu_pos.x = menu_pos.x.max(spacing::SM);
        menu_pos.y = menu_pos.y.max(spacing::SM);

        // Track if we should close the menu
        let mut should_close = false;
        let mut selected_action: Option<ContextMenuAction> = None;

        // Track submenu hover state: (submenu_id, items, trigger_rect)
        let mut hovered_submenu: Option<(String, Vec<ContextMenuItem>, Rect)> = None;

        // Check for click outside the menu
        let pointer_pos = ctx.input(|i| i.pointer.hover_pos());
        let primary_clicked = ctx.input(|i| i.pointer.primary_clicked());

        // Render the main menu using an Area overlay
        egui::Area::new(egui::Id::new("context_menu"))
            .order(Order::Foreground)
            .fixed_pos(menu_pos)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(colors::SURFACE)
                    .rounding(Rounding::same(rounding::CARD))
                    .shadow(crate::ui::gui::theme::shadow::elevated())
                    .stroke(Stroke::new(1.0, colors::BORDER))
                    .inner_margin(egui::Margin::symmetric(0.0, CONTEXT_MENU_PADDING_V))
                    .show(ui, |ui| {
                        ui.set_min_width(menu_width);

                        for item in &menu_state.items {
                            match item {
                                ContextMenuItem::Action {
                                    label,
                                    action,
                                    enabled,
                                } => {
                                    let response =
                                        self.render_context_menu_item(ui, label, *enabled, false);
                                    if response.clicked {
                                        selected_action = Some(action.clone());
                                        should_close = true;
                                    }
                                }
                                ContextMenuItem::Separator => {
                                    ui.add_space(spacing::XS);
                                    let rect = ui.available_rect_before_wrap();
                                    let separator_rect =
                                        Rect::from_min_size(rect.min, Vec2::new(menu_width, 1.0));
                                    ui.painter().rect_filled(
                                        separator_rect,
                                        Rounding::ZERO,
                                        colors::SEPARATOR,
                                    );
                                    ui.allocate_space(Vec2::new(menu_width, 1.0));
                                    ui.add_space(spacing::XS);
                                }
                                ContextMenuItem::Submenu {
                                    label,
                                    id,
                                    enabled,
                                    items,
                                } => {
                                    // Render submenu trigger with arrow indicator
                                    let response =
                                        self.render_context_menu_item(ui, label, *enabled, true);
                                    if response.hovered && *enabled && !items.is_empty() {
                                        // Track this submenu as hovered for rendering
                                        hovered_submenu =
                                            Some((id.clone(), items.clone(), response.rect));
                                    }
                                }
                            }
                        }
                    });
            });

        // Calculate the main menu rect for click-outside detection
        let menu_rect = Rect::from_min_size(menu_pos, Vec2::new(menu_width, menu_height));

        // Track submenu rect for click-outside detection
        let mut submenu_rect: Option<Rect> = None;

        // Render submenu if one is hovered or already open
        // Priority: currently hovered submenu > previously open submenu
        let submenu_to_render = if let Some((id, items, trigger_rect)) = hovered_submenu {
            // Update the open_submenu state with the hovered submenu
            if let Some(menu) = &mut self.context_menu {
                let submenu_pos = Pos2::new(
                    menu_pos.x + menu_width + CONTEXT_MENU_SUBMENU_GAP,
                    trigger_rect.min.y,
                );
                menu.open_submenu(id.clone(), submenu_pos);
            }
            Some((items, trigger_rect))
        } else if let (Some(open_id), Some(open_pos)) =
            (&menu_state.open_submenu, menu_state.submenu_position)
        {
            // Find the items for the currently open submenu
            let items = menu_state.items.iter().find_map(|item| {
                if let ContextMenuItem::Submenu { id, items, .. } = item {
                    if id == open_id {
                        return Some(items.clone());
                    }
                }
                None
            });
            // Find the trigger rect (approximate from stored position)
            let trigger_rect = Rect::from_min_size(
                Pos2::new(menu_pos.x, open_pos.y),
                Vec2::new(menu_width, CONTEXT_MENU_ITEM_HEIGHT),
            );
            items.map(|i| (i, trigger_rect))
        } else {
            // No submenu to render, close any open submenu
            if let Some(menu) = &mut self.context_menu {
                menu.close_submenu();
            }
            None
        };

        // Render the submenu if we have one
        if let Some((submenu_items, trigger_rect)) = submenu_to_render {
            if !submenu_items.is_empty() {
                // Calculate submenu dimensions
                let submenu_item_count = submenu_items
                    .iter()
                    .filter(|item| !matches!(item, ContextMenuItem::Separator))
                    .count();
                let submenu_separator_count = submenu_items
                    .iter()
                    .filter(|item| matches!(item, ContextMenuItem::Separator))
                    .count();
                let submenu_height = (submenu_item_count as f32 * CONTEXT_MENU_ITEM_HEIGHT)
                    + (submenu_separator_count as f32 * (spacing::SM + 1.0))
                    + (CONTEXT_MENU_PADDING_V * 2.0);

                // Position submenu to the right of the main menu
                let mut submenu_pos = Pos2::new(
                    menu_pos.x + menu_width + CONTEXT_MENU_SUBMENU_GAP,
                    trigger_rect.min.y - CONTEXT_MENU_PADDING_V,
                );

                // Ensure submenu doesn't go off the right edge
                if submenu_pos.x + menu_width > screen_rect.max.x - spacing::SM {
                    // Position to the left of the main menu instead
                    submenu_pos.x = menu_pos.x - menu_width - CONTEXT_MENU_SUBMENU_GAP;
                }

                // Ensure submenu doesn't go off the bottom edge
                if submenu_pos.y + submenu_height > screen_rect.max.y - spacing::SM {
                    submenu_pos.y = screen_rect.max.y - submenu_height - spacing::SM;
                }

                // Ensure submenu doesn't go off the top edge
                submenu_pos.y = submenu_pos.y.max(spacing::SM);

                // Store submenu rect for click-outside detection
                submenu_rect = Some(Rect::from_min_size(
                    submenu_pos,
                    Vec2::new(menu_width, submenu_height),
                ));

                // Render the submenu
                egui::Area::new(egui::Id::new("context_submenu"))
                    .order(Order::Foreground)
                    .fixed_pos(submenu_pos)
                    .show(ctx, |ui| {
                        egui::Frame::none()
                            .fill(colors::SURFACE)
                            .rounding(Rounding::same(rounding::CARD))
                            .shadow(crate::ui::gui::theme::shadow::elevated())
                            .stroke(Stroke::new(1.0, colors::BORDER))
                            .inner_margin(egui::Margin::symmetric(0.0, CONTEXT_MENU_PADDING_V))
                            .show(ui, |ui| {
                                ui.set_min_width(menu_width);

                                for item in &submenu_items {
                                    match item {
                                        ContextMenuItem::Action {
                                            label,
                                            action,
                                            enabled,
                                        } => {
                                            let response = self.render_context_menu_item(
                                                ui, label, *enabled, false,
                                            );
                                            if response.clicked {
                                                selected_action = Some(action.clone());
                                                should_close = true;
                                            }
                                        }
                                        ContextMenuItem::Separator => {
                                            ui.add_space(spacing::XS);
                                            let rect = ui.available_rect_before_wrap();
                                            let separator_rect = Rect::from_min_size(
                                                rect.min,
                                                Vec2::new(menu_width, 1.0),
                                            );
                                            ui.painter().rect_filled(
                                                separator_rect,
                                                Rounding::ZERO,
                                                colors::SEPARATOR,
                                            );
                                            ui.allocate_space(Vec2::new(menu_width, 1.0));
                                            ui.add_space(spacing::XS);
                                        }
                                        ContextMenuItem::Submenu { .. } => {
                                            // Nested submenus not supported (not needed for current use cases)
                                        }
                                    }
                                }
                            });
                    });
            }
        }

        // Check if click was outside both the menu and submenu areas
        if primary_clicked {
            if let Some(pos) = pointer_pos {
                let in_menu = menu_rect.contains(pos);
                let in_submenu = submenu_rect.map(|r| r.contains(pos)).unwrap_or(false);
                if !in_menu && !in_submenu {
                    should_close = true;
                }
            }
        }

        // Handle the selected action
        if let Some(action) = selected_action {
            let project_name = menu_state.project_name.clone();
            match action {
                ContextMenuAction::Status => {
                    // Spawn the status command (US-003)
                    self.spawn_status_command(&project_name);
                }
                ContextMenuAction::Describe => {
                    // Spawn the describe command (US-004)
                    self.spawn_describe_command(&project_name);
                }
                ContextMenuAction::Resume(session_id) => {
                    // TODO: Implement resume integration
                    // For now, log to console as a placeholder
                    if let Some(id) = session_id {
                        eprintln!(
                            "Resume not implemented yet (project: {}, session: {})",
                            project_name, id
                        );
                    } else {
                        eprintln!("Resume not implemented yet (project: {})", project_name);
                    }
                }
                ContextMenuAction::CleanWorktrees => {
                    // Spawn the clean --worktrees command (US-006)
                    self.spawn_clean_worktrees_command(&project_name);
                }
                ContextMenuAction::CleanOrphaned => {
                    // Spawn the clean --orphaned command (US-006)
                    self.spawn_clean_orphaned_command(&project_name);
                }
            }
        }

        // Close the menu if needed
        if should_close {
            self.close_context_menu();
        }
    }

    /// Render a single context menu item.
    ///
    /// Returns a `ContextMenuItemResponse` with click/hover state and item rect.
    fn render_context_menu_item(
        &self,
        ui: &mut egui::Ui,
        label: &str,
        enabled: bool,
        has_submenu: bool,
    ) -> ContextMenuItemResponse {
        let item_size = Vec2::new(ui.available_width(), CONTEXT_MENU_ITEM_HEIGHT);
        let (rect, response) = ui.allocate_exact_size(item_size, Sense::click());

        let is_hovered = response.hovered() && enabled;
        let painter = ui.painter();

        // Draw hover background
        if is_hovered {
            painter.rect_filled(rect, Rounding::ZERO, colors::SURFACE_HOVER);
        }

        // Calculate text position with padding
        let text_x = rect.min.x + CONTEXT_MENU_PADDING_H;
        let text_color = if enabled {
            colors::TEXT_PRIMARY
        } else {
            colors::TEXT_DISABLED
        };

        // Draw label
        let galley = painter.layout_no_wrap(
            label.to_string(),
            typography::font(FontSize::Body, FontWeight::Regular),
            text_color,
        );
        let text_y = rect.center().y - galley.rect.height() / 2.0;
        painter.galley(Pos2::new(text_x, text_y), galley, Color32::TRANSPARENT);

        // Draw submenu arrow indicator if this item has a submenu
        if has_submenu {
            let arrow_x = rect.max.x - CONTEXT_MENU_PADDING_H - CONTEXT_MENU_ARROW_SIZE;
            let arrow_y = rect.center().y;
            let arrow_color = if enabled {
                colors::TEXT_SECONDARY
            } else {
                colors::TEXT_DISABLED
            };

            // Draw a simple right-pointing chevron
            let arrow_points = [
                Pos2::new(arrow_x, arrow_y - CONTEXT_MENU_ARROW_SIZE / 2.0),
                Pos2::new(arrow_x + CONTEXT_MENU_ARROW_SIZE / 2.0, arrow_y),
                Pos2::new(arrow_x, arrow_y + CONTEXT_MENU_ARROW_SIZE / 2.0),
            ];
            painter.line_segment(
                [arrow_points[0], arrow_points[1]],
                Stroke::new(1.5, arrow_color),
            );
            painter.line_segment(
                [arrow_points[1], arrow_points[2]],
                Stroke::new(1.5, arrow_color),
            );
        }

        // Convert local rect to screen rect for submenu positioning
        let screen_rect = ui.clip_rect();
        let screen_item_rect = Rect::from_min_max(
            Pos2::new(screen_rect.min.x, rect.min.y),
            Pos2::new(screen_rect.min.x + ui.available_width(), rect.max.y),
        );

        ContextMenuItemResponse {
            clicked: response.clicked() && enabled,
            hovered: is_hovered,
            rect: screen_item_rect,
        }
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
            TabId::CommandOutput(cache_key) => {
                let cache_key = cache_key.clone();
                self.render_command_output(ui, &cache_key);
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

    /// Render the command output view for a specific command execution.
    fn render_command_output(&self, ui: &mut egui::Ui, cache_key: &str) {
        // Get the command execution state
        let execution = match self.command_executions.get(cache_key) {
            Some(exec) => exec,
            None => {
                // No execution found - show placeholder
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add_space(spacing::XXL);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new("Command output not available")
                                    .font(typography::font(FontSize::Heading, FontWeight::Medium))
                                    .color(colors::TEXT_MUTED),
                            );
                        });
                    });
                return;
            }
        };

        // Header with command info
        self.render_command_output_header(ui, execution);

        ui.add_space(spacing::MD);

        // Output content with auto-scroll
        self.render_command_output_content(ui, execution, cache_key);
    }

    /// Render the header for command output (status indicator, project, command).
    fn render_command_output_header(&self, ui: &mut egui::Ui, execution: &CommandExecution) {
        ui.horizontal(|ui| {
            // Status badge
            let (status_text, status_color) = match execution.status {
                CommandStatus::Running => ("Running", colors::STATUS_RUNNING),
                CommandStatus::Completed => ("Completed", colors::STATUS_SUCCESS),
                CommandStatus::Failed => ("Failed", colors::STATUS_ERROR),
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

            // Spinner for running state
            if execution.status == CommandStatus::Running {
                self.render_inline_spinner(ui);
                ui.add_space(spacing::SM);
            }

            // Title: "Command: project"
            ui.label(
                egui::RichText::new(execution.id.tab_label())
                    .font(typography::font(FontSize::Title, FontWeight::SemiBold))
                    .color(colors::TEXT_PRIMARY),
            );
        });

        // Show exit code if completed
        if let Some(exit_code) = execution.exit_code {
            ui.add_space(spacing::SM);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Exit code:")
                        .font(typography::font(FontSize::Body, FontWeight::Medium))
                        .color(colors::TEXT_SECONDARY),
                );
                ui.add_space(spacing::XS);

                let exit_color = if exit_code == 0 {
                    colors::STATUS_SUCCESS
                } else {
                    colors::STATUS_ERROR
                };
                ui.label(
                    egui::RichText::new(exit_code.to_string())
                        .font(typography::mono(FontSize::Body))
                        .color(exit_color),
                );
            });
        }
    }

    /// Render an inline spinner for loading states.
    fn render_inline_spinner(&self, ui: &mut egui::Ui) {
        let spinner_size = 16.0;
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(spinner_size), Sense::hover());

        if ui.is_rect_visible(rect) {
            let center = rect.center();
            let radius = spinner_size / 2.0 - 2.0;
            let time = ui.input(|i| i.time);
            let start_angle = (time * 2.0) as f32 % std::f32::consts::TAU;
            let arc_length = std::f32::consts::PI * 1.5;

            let n_points = 32;
            let points: Vec<_> = (0..=n_points)
                .map(|i| {
                    let angle = start_angle + arc_length * (i as f32 / n_points as f32);
                    egui::pos2(
                        center.x + radius * angle.cos(),
                        center.y + radius * angle.sin(),
                    )
                })
                .collect();

            ui.painter()
                .add(egui::Shape::line(points, Stroke::new(2.0, colors::ACCENT)));

            // Request repaint for animation
            ui.ctx().request_repaint();
        }
    }

    /// Render the command output content in a scrollable area.
    fn render_command_output_content(
        &self,
        ui: &mut egui::Ui,
        execution: &CommandExecution,
        _cache_key: &str,
    ) {
        // Calculate a unique ID for scroll state
        let scroll_id = egui::Id::new("command_output_scroll").with(execution.id.cache_key());

        // Build scroll area - auto-scroll to bottom when auto_scroll is enabled
        let scroll_area = egui::ScrollArea::vertical()
            .id_salt(scroll_id)
            .auto_shrink([false, false])
            .stick_to_bottom(execution.auto_scroll);

        // If running, request repaint to show spinner animation
        if execution.is_running() {
            ui.ctx().request_repaint();
        }

        scroll_area.show(ui, |ui| {
            // Background for output area
            let available_rect = ui.available_rect_before_wrap();
            ui.painter().rect_filled(
                available_rect,
                Rounding::same(rounding::BUTTON),
                colors::SURFACE_HOVER,
            );

            ui.add_space(spacing::SM);

            egui::Frame::none()
                .inner_margin(spacing::MD)
                .show(ui, |ui| {
                    // Render stdout
                    if !execution.stdout.is_empty() {
                        for line in &execution.stdout {
                            // Use selectable_label for copy/paste support
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(line)
                                        .font(typography::mono(FontSize::Small))
                                        .color(colors::TEXT_PRIMARY),
                                )
                                .selectable(true)
                                .wrap_mode(egui::TextWrapMode::Wrap),
                            );
                        }
                    }

                    // Render stderr (in error color)
                    if !execution.stderr.is_empty() {
                        if !execution.stdout.is_empty() {
                            ui.add_space(spacing::SM);
                            ui.separator();
                            ui.add_space(spacing::SM);
                            ui.label(
                                egui::RichText::new("Errors:")
                                    .font(typography::font(FontSize::Small, FontWeight::Medium))
                                    .color(colors::STATUS_ERROR),
                            );
                            ui.add_space(spacing::XS);
                        }

                        for line in &execution.stderr {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(line)
                                        .font(typography::mono(FontSize::Small))
                                        .color(colors::STATUS_ERROR),
                                )
                                .selectable(true)
                                .wrap_mode(egui::TextWrapMode::Wrap),
                            );
                        }
                    }

                    // Show "no output yet" if empty and still running
                    if execution.stdout.is_empty()
                        && execution.stderr.is_empty()
                        && execution.is_running()
                    {
                        ui.label(
                            egui::RichText::new("Waiting for output...")
                                .font(typography::font(FontSize::Body, FontWeight::Regular))
                                .color(colors::TEXT_MUTED)
                                .italics(),
                        );
                    }

                    // Show completion message if no output and completed
                    if execution.stdout.is_empty()
                        && execution.stderr.is_empty()
                        && execution.is_finished()
                    {
                        ui.label(
                            egui::RichText::new("Command completed with no output.")
                                .font(typography::font(FontSize::Body, FontWeight::Regular))
                                .color(colors::TEXT_MUTED)
                                .italics(),
                        );
                    }
                });
        });
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
                crate::state::RunStatus::Interrupted => "Interrupted",
            };
            let status_color = match run_state.status {
                crate::state::RunStatus::Completed => colors::STATUS_SUCCESS,
                crate::state::RunStatus::Failed => colors::STATUS_ERROR,
                crate::state::RunStatus::Running => colors::STATUS_RUNNING,
                crate::state::RunStatus::Interrupted => colors::STATUS_WARNING,
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
    /// Always returns at most 2 columns for a 2x2 grid layout where each card
    /// takes approximately 1/4 of the screen.
    fn calculate_grid_columns(available_width: f32) -> usize {
        // Calculate how many cards fit, accounting for spacing
        let card_with_spacing = CARD_MIN_WIDTH + CARD_SPACING;
        let columns = ((available_width + CARD_SPACING) / card_with_spacing).floor() as usize;

        // Clamp to range: minimum 1, maximum 2 (for 2x2 grid of 1/4 screen cards)
        columns.clamp(1, MAX_GRID_COLUMNS)
    }

    /// Calculate the card width for the current number of columns.
    /// Accounts for edge spacing and inter-card spacing.
    fn calculate_card_width(available_width: f32, columns: usize) -> f32 {
        // Total spacing: edges (left + right) + between cards
        let total_spacing = CARD_SPACING * (columns as f32 + 1.0);
        let card_width = (available_width - total_spacing) / columns as f32;

        // Clamp to min/max bounds
        card_width.clamp(CARD_MIN_WIDTH, CARD_MAX_WIDTH)
    }

    /// Render the sessions in a responsive grid layout.
    /// Cards are sized to approximately 50% width and 50% height, creating a 2x2 visible grid.
    /// When more than 4 sessions exist, the content scrolls vertically.
    fn render_sessions_grid(&self, ui: &mut egui::Ui) {
        let available_width = ui.available_width();
        let available_height = ui.available_height();
        let columns = Self::calculate_grid_columns(available_width);

        // Calculate card dimensions based on available space
        let card_width = Self::calculate_card_width(available_width, columns);

        // Calculate height: 2 rows visible with spacing (edge spacing + inter-row spacing)
        let total_v_spacing = CARD_SPACING * 3.0; // Top + between rows + bottom
        let card_height = ((available_height - total_v_spacing) / 2.0).max(CARD_MIN_HEIGHT);

        // Calculate total width of card row for centering
        let row_width = (card_width * columns as f32) + (CARD_SPACING * (columns as f32 - 1.0));
        let h_offset = ((available_width - row_width) / 2.0).max(0.0);

        // Scrollable area for the grid with smooth scrolling
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
            .show(ui, |ui| {
                // Add top spacing
                ui.add_space(CARD_SPACING);

                // Create rows of cards with consistent spacing, centered horizontally
                let mut session_iter = self.sessions.iter().peekable();
                while session_iter.peek().is_some() {
                    ui.horizontal(|ui| {
                        // Add horizontal offset for centering
                        ui.add_space(h_offset);

                        for i in 0..columns {
                            if let Some(session) = session_iter.next() {
                                self.render_session_card(ui, session, card_width, card_height);
                                // Add spacing between cards (but not after the last one in row)
                                if i < columns - 1 {
                                    ui.add_space(CARD_SPACING);
                                }
                            }
                        }
                    });
                    ui.add_space(CARD_SPACING);
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
    /// - Output section: Last OUTPUT_LINES_TO_SHOW lines of Claude output in monospace font
    fn render_session_card(
        &self,
        ui: &mut egui::Ui,
        session: &SessionData,
        card_width: f32,
        card_height: f32,
    ) {
        // Define card dimensions
        let card_size = Vec2::new(card_width, card_height);

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

        // Collect interactions to handle after rendering (to avoid borrow issues)
        let mut interactions: Vec<(String, ProjectRowInteraction)> = Vec::new();

        egui::ScrollArea::vertical()
            .id_salt("projects_left_panel")
            .auto_shrink([false, false])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
            .show(ui, |ui| {
                for (idx, project_name) in project_names.iter().enumerate() {
                    let project = &self.projects[idx];
                    let is_selected = selected.as_deref() == Some(project_name.as_str());
                    let interaction = self.render_project_row(ui, project, is_selected);
                    if interaction.clicked || interaction.right_click_pos.is_some() {
                        interactions.push((project_name.clone(), interaction));
                    }
                    ui.add_space(spacing::XS);
                }
            });

        // Handle interactions after rendering
        for (project_name, interaction) in interactions {
            if interaction.clicked {
                // Left-click: toggle selection and load history
                self.toggle_project_selection(&project_name);
            } else if let Some(pos) = interaction.right_click_pos {
                // Right-click: open context menu
                self.open_context_menu(pos, project_name);
            }
        }
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
    /// Returns interaction information (left-click and right-click).
    fn render_project_row(
        &self,
        ui: &mut egui::Ui,
        project: &ProjectData,
        is_selected: bool,
    ) -> ProjectRowInteraction {
        let row_size = Vec2::new(ui.available_width(), PROJECT_ROW_HEIGHT);

        // Allocate space for the row with click interaction (both primary and secondary)
        let (rect, response) = ui.allocate_exact_size(row_size, Sense::click());

        // Skip if not visible (optimization for scrolling)
        if !ui.is_rect_visible(rect) {
            return ProjectRowInteraction::none();
        }

        let painter = ui.painter();
        let is_hovered = response.hovered();
        let was_clicked = response.clicked();
        let was_secondary_clicked = response.secondary_clicked();

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

        // Return interaction info
        if was_secondary_clicked {
            // Right-click: return position for context menu
            // Use the pointer position if available, otherwise center of the row
            let menu_pos = ui
                .ctx()
                .input(|i| i.pointer.hover_pos())
                .unwrap_or(rect.center());
            ProjectRowInteraction::right_click(menu_pos)
        } else if was_clicked {
            ProjectRowInteraction::click()
        } else {
            ProjectRowInteraction::none()
        }
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
/// # Returns
///
/// * `Ok(())` when the user closes the window
/// * `Err(Autom8Error)` if the GUI fails to initialize
pub fn run_gui() -> Result<()> {
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
            Ok(Box::new(Autom8App::new()))
        }),
    )
    .map_err(|e| Autom8Error::GuiError(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProjectTreeInfo;
    use chrono::Utc;

    // ========================================================================
    // App Initialization Tests
    // ========================================================================

    #[test]
    fn test_autom8_app_new_defaults_to_active_runs() {
        let app = Autom8App::new();
        assert_eq!(app.current_tab(), Tab::ActiveRuns);
    }

    #[test]
    fn test_app_with_custom_refresh_interval() {
        let interval = Duration::from_millis(100);
        let app = Autom8App::with_refresh_interval(interval);
        assert_eq!(app.refresh_interval(), interval);
    }

    // ========================================================================
    // Grid Layout Tests
    // ========================================================================

    #[test]
    fn test_calculate_grid_columns() {
        // Cards take ~50% of width, so max 2 columns for 2x2 grid layout
        assert_eq!(Autom8App::calculate_grid_columns(300.0), 1); // Very narrow - single column
        assert_eq!(Autom8App::calculate_grid_columns(500.0), 1); // Narrow - single column
        assert_eq!(Autom8App::calculate_grid_columns(900.0), 2); // Medium - 2 columns
        assert_eq!(Autom8App::calculate_grid_columns(1400.0), 2); // Wide - capped at 2
        assert_eq!(Autom8App::calculate_grid_columns(2000.0), 2); // Very wide - capped at 2
    }

    #[test]
    fn test_calculate_card_width() {
        // With new formula: (available - (columns+1)*spacing) / columns
        // For 2 columns: (width - 3*24) / 2 = (width - 72) / 2

        // Normal cases - cards should be within bounds
        // 1200 - 72 = 1128 / 2 = 564, within 400-800 range
        let width_2col = Autom8App::calculate_card_width(1200.0, 2);
        assert!(width_2col >= CARD_MIN_WIDTH && width_2col <= CARD_MAX_WIDTH);

        // Clamps to min when width is too small
        // 600 - 72 = 528 / 2 = 264, should clamp to 400
        assert_eq!(Autom8App::calculate_card_width(600.0, 2), CARD_MIN_WIDTH);

        // Clamps to max when width is very large
        // 2000 - 72 = 1928 / 2 = 964, should clamp to 800
        assert_eq!(Autom8App::calculate_card_width(2000.0, 2), CARD_MAX_WIDTH);

        // Single column case
        // 900 - 48 = 852 / 1 = 852, should clamp to 800
        assert_eq!(Autom8App::calculate_card_width(900.0, 1), CARD_MAX_WIDTH);
    }

    // ========================================================================
    // Projects View Tests
    // ========================================================================

    #[test]
    fn test_project_status_color() {
        let app = Autom8App::new();

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
        let mut app = Autom8App::new();
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
        let app = Autom8App::new();
        assert_eq!(app.tab_count(), 2);
        assert_eq!(app.closable_tab_count(), 0);
        assert_eq!(*app.active_tab_id(), TabId::ActiveRuns);
    }

    #[test]
    fn test_app_open_and_close_tabs() {
        let mut app = Autom8App::new();

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
        let mut app = Autom8App::new();
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

        let mut app = Autom8App::new();
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

        let mut app = Autom8App::new();
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
        let mut app = Autom8App::new();
        assert!(!app.is_sidebar_collapsed());

        app.toggle_sidebar();
        assert!(app.is_sidebar_collapsed());

        app.toggle_sidebar();
        assert!(!app.is_sidebar_collapsed());
    }

    // ========================================================================
    // Context Menu Tests (Right-Click Context Menu - US-002)
    // ========================================================================

    #[test]
    fn test_context_menu_state_creation() {
        let pos = Pos2::new(100.0, 200.0);
        let items = vec![
            ContextMenuItem::action("Status", ContextMenuAction::Status),
            ContextMenuItem::separator(),
            ContextMenuItem::action("Describe", ContextMenuAction::Describe),
        ];

        let state = ContextMenuState::new(pos, "test-project".to_string(), items.clone());

        assert_eq!(state.position, pos);
        assert_eq!(state.project_name, "test-project");
        assert_eq!(state.items.len(), 3);
        assert!(state.open_submenu.is_none());
        assert!(state.submenu_position.is_none());
    }

    #[test]
    fn test_context_menu_submenu_open_close() {
        let pos = Pos2::new(100.0, 200.0);
        let items = vec![ContextMenuItem::action("Test", ContextMenuAction::Status)];
        let mut state = ContextMenuState::new(pos, "test-project".to_string(), items);

        // Open a submenu
        let submenu_pos = Pos2::new(260.0, 220.0);
        state.open_submenu("clean".to_string(), submenu_pos);
        assert_eq!(state.open_submenu, Some("clean".to_string()));
        assert_eq!(state.submenu_position, Some(submenu_pos));

        // Close submenu
        state.close_submenu();
        assert!(state.open_submenu.is_none());
        assert!(state.submenu_position.is_none());
    }

    #[test]
    fn test_context_menu_item_creation() {
        // Test action item
        let action = ContextMenuItem::action("Status", ContextMenuAction::Status);
        match action {
            ContextMenuItem::Action {
                label,
                action: act,
                enabled,
            } => {
                assert_eq!(label, "Status");
                assert_eq!(act, ContextMenuAction::Status);
                assert!(enabled);
            }
            _ => panic!("Expected Action variant"),
        }

        // Test disabled action item
        let disabled = ContextMenuItem::action_disabled("Resume", ContextMenuAction::Resume(None));
        match disabled {
            ContextMenuItem::Action { enabled, .. } => {
                assert!(!enabled);
            }
            _ => panic!("Expected Action variant"),
        }

        // Test separator
        let sep = ContextMenuItem::separator();
        assert!(matches!(sep, ContextMenuItem::Separator));

        // Test submenu with items
        let submenu = ContextMenuItem::submenu(
            "Clean",
            "clean",
            vec![ContextMenuItem::action(
                "Worktrees",
                ContextMenuAction::CleanWorktrees,
            )],
        );
        match submenu {
            ContextMenuItem::Submenu {
                label,
                id,
                enabled,
                items,
            } => {
                assert_eq!(label, "Clean");
                assert_eq!(id, "clean");
                assert!(enabled); // Has items, so enabled
                assert_eq!(items.len(), 1);
            }
            _ => panic!("Expected Submenu variant"),
        }

        // Test disabled submenu (no items)
        let disabled_submenu = ContextMenuItem::submenu_disabled("Empty", "empty");
        match disabled_submenu {
            ContextMenuItem::Submenu { enabled, items, .. } => {
                assert!(!enabled);
                assert!(items.is_empty());
            }
            _ => panic!("Expected Submenu variant"),
        }
    }

    #[test]
    fn test_app_context_menu_open_close() {
        let mut app = Autom8App::new();

        // Initially no context menu
        assert!(!app.is_context_menu_open());
        assert!(app.context_menu().is_none());

        // Open context menu
        let pos = Pos2::new(150.0, 300.0);
        app.open_context_menu(pos, "my-project".to_string());

        assert!(app.is_context_menu_open());
        let menu = app.context_menu().unwrap();
        assert_eq!(menu.position, pos);
        assert_eq!(menu.project_name, "my-project");

        // Close context menu
        app.close_context_menu();
        assert!(!app.is_context_menu_open());
        assert!(app.context_menu().is_none());
    }

    #[test]
    fn test_app_only_one_context_menu_at_a_time() {
        let mut app = Autom8App::new();

        // Open first context menu
        app.open_context_menu(Pos2::new(100.0, 100.0), "project-a".to_string());
        assert_eq!(app.context_menu().unwrap().project_name, "project-a");

        // Open second context menu - should replace the first
        app.open_context_menu(Pos2::new(200.0, 200.0), "project-b".to_string());
        assert_eq!(app.context_menu().unwrap().project_name, "project-b");

        // Only one context menu should be open
        assert!(app.is_context_menu_open());
    }

    #[test]
    fn test_build_context_menu_items() {
        let app = Autom8App::new();
        let items = app.build_context_menu_items("test-project");

        // Should have Status, Describe, separator, Resume, separator, Clean
        assert_eq!(items.len(), 6);

        // Check first item is Status
        match &items[0] {
            ContextMenuItem::Action {
                label,
                action,
                enabled,
            } => {
                assert_eq!(label, "Status");
                assert_eq!(action, &ContextMenuAction::Status);
                assert!(enabled);
            }
            _ => panic!("Expected Status action"),
        }

        // Check second item is Describe
        match &items[1] {
            ContextMenuItem::Action {
                label,
                action,
                enabled,
            } => {
                assert_eq!(label, "Describe");
                assert_eq!(action, &ContextMenuAction::Describe);
                assert!(enabled);
            }
            _ => panic!("Expected Describe action"),
        }

        // Check separators
        assert!(matches!(&items[2], ContextMenuItem::Separator));
        assert!(matches!(&items[4], ContextMenuItem::Separator));

        // Check Resume is disabled (no resumable sessions for test-project)
        match &items[3] {
            ContextMenuItem::Action {
                label,
                enabled,
                action,
            } => {
                assert_eq!(label, "Resume");
                assert_eq!(action, &ContextMenuAction::Resume(None));
                assert!(!enabled, "Resume should be disabled when no sessions");
            }
            _ => panic!("Expected Resume action"),
        }

        // Check Clean submenu is disabled (placeholder)
        match &items[5] {
            ContextMenuItem::Submenu { label, enabled, .. } => {
                assert_eq!(label, "Clean");
                assert!(!enabled);
            }
            _ => panic!("Expected Clean submenu"),
        }
    }

    #[test]
    fn test_project_row_interaction() {
        // Test none
        let none = ProjectRowInteraction::none();
        assert!(!none.clicked);
        assert!(none.right_click_pos.is_none());

        // Test click
        let click = ProjectRowInteraction::click();
        assert!(click.clicked);
        assert!(click.right_click_pos.is_none());

        // Test right-click
        let pos = Pos2::new(100.0, 200.0);
        let right_click = ProjectRowInteraction::right_click(pos);
        assert!(!right_click.clicked);
        assert_eq!(right_click.right_click_pos, Some(pos));
    }

    #[test]
    fn test_context_menu_constants() {
        // Verify constants are reasonable values
        assert!(CONTEXT_MENU_MIN_WIDTH >= 100.0);
        assert!(CONTEXT_MENU_ITEM_HEIGHT >= 24.0);
        assert!(CONTEXT_MENU_PADDING_H > 0.0);
        assert!(CONTEXT_MENU_PADDING_V >= 0.0);
        assert!(CONTEXT_MENU_ARROW_SIZE > 0.0);
    }

    // ========================================================================
    // Command Output Tab Tests (US-007)
    // ========================================================================

    #[test]
    fn test_command_output_id_creation() {
        let id = CommandOutputId::new("my-project", "status");

        assert_eq!(id.project, "my-project");
        assert_eq!(id.command, "status");
        assert!(!id.id.is_empty()); // UUID should be generated
    }

    #[test]
    fn test_command_output_id_with_id() {
        let id = CommandOutputId::with_id("my-project", "describe", "test-id-123");

        assert_eq!(id.project, "my-project");
        assert_eq!(id.command, "describe");
        assert_eq!(id.id, "test-id-123");
    }

    #[test]
    fn test_command_output_id_cache_key() {
        let id = CommandOutputId::with_id("my-project", "status", "abc123");
        assert_eq!(id.cache_key(), "my-project:status:abc123");
    }

    #[test]
    fn test_command_output_id_tab_label() {
        let id = CommandOutputId::with_id("my-project", "status", "test");
        assert_eq!(id.tab_label(), "Status: my-project");

        let id2 = CommandOutputId::with_id("another-project", "describe", "test");
        assert_eq!(id2.tab_label(), "Describe: another-project");

        let id3 = CommandOutputId::with_id("project", "", "test");
        assert_eq!(id3.tab_label(), "Command: project");
    }

    #[test]
    fn test_command_execution_creation() {
        let id = CommandOutputId::with_id("project", "status", "id1");
        let exec = CommandExecution::new(id.clone());

        assert_eq!(exec.id, id);
        assert_eq!(exec.status, CommandStatus::Running);
        assert!(exec.stdout.is_empty());
        assert!(exec.stderr.is_empty());
        assert!(exec.exit_code.is_none());
        assert!(exec.auto_scroll);
    }

    #[test]
    fn test_command_execution_add_output() {
        let id = CommandOutputId::with_id("project", "status", "id1");
        let mut exec = CommandExecution::new(id);

        exec.add_stdout("line 1".to_string());
        exec.add_stdout("line 2".to_string());
        exec.add_stderr("error 1".to_string());

        assert_eq!(exec.stdout.len(), 2);
        assert_eq!(exec.stderr.len(), 1);
        assert_eq!(exec.stdout[0], "line 1");
        assert_eq!(exec.stderr[0], "error 1");
    }

    #[test]
    fn test_command_execution_complete_success() {
        let id = CommandOutputId::with_id("project", "status", "id1");
        let mut exec = CommandExecution::new(id);

        assert!(exec.is_running());
        assert!(!exec.is_finished());

        exec.complete(0);

        assert!(!exec.is_running());
        assert!(exec.is_finished());
        assert_eq!(exec.status, CommandStatus::Completed);
        assert_eq!(exec.exit_code, Some(0));
    }

    #[test]
    fn test_command_execution_complete_failure() {
        let id = CommandOutputId::with_id("project", "status", "id1");
        let mut exec = CommandExecution::new(id);

        exec.complete(1);

        assert!(!exec.is_running());
        assert!(exec.is_finished());
        assert_eq!(exec.status, CommandStatus::Failed);
        assert_eq!(exec.exit_code, Some(1));
    }

    #[test]
    fn test_command_execution_fail() {
        let id = CommandOutputId::with_id("project", "status", "id1");
        let mut exec = CommandExecution::new(id);

        exec.fail("Command not found".to_string());

        assert!(!exec.is_running());
        assert!(exec.is_finished());
        assert_eq!(exec.status, CommandStatus::Failed);
        assert!(exec.exit_code.is_none());
        assert_eq!(exec.stderr.len(), 1);
        assert_eq!(exec.stderr[0], "Command not found");
    }

    #[test]
    fn test_command_execution_combined_output() {
        let id = CommandOutputId::with_id("project", "status", "id1");
        let mut exec = CommandExecution::new(id);

        exec.add_stdout("out1".to_string());
        exec.add_stdout("out2".to_string());
        exec.add_stderr("err1".to_string());

        let combined = exec.combined_output();
        assert_eq!(combined.len(), 3);
        assert_eq!(combined[0], "out1");
        assert_eq!(combined[1], "out2");
        assert_eq!(combined[2], "err1");
    }

    #[test]
    fn test_app_open_command_output_tab() {
        let mut app = Autom8App::new();

        let id = app.open_command_output_tab("my-project", "status");

        // Check tab was created
        let tab_id = TabId::CommandOutput(id.cache_key());
        assert!(app.has_tab(&tab_id));
        assert_eq!(*app.active_tab_id(), tab_id);

        // Check execution was created
        let exec = app.get_command_execution(&id.cache_key());
        assert!(exec.is_some());
        assert_eq!(exec.unwrap().status, CommandStatus::Running);

        // Check tab label
        let tab = app.tabs().iter().find(|t| t.id == tab_id).unwrap();
        assert!(tab.label.starts_with("Status: "));
        assert!(tab.closable);
    }

    #[test]
    fn test_app_multiple_command_output_tabs() {
        let mut app = Autom8App::new();

        let id1 = app.open_command_output_tab("project-a", "status");
        let id2 = app.open_command_output_tab("project-b", "describe");
        let id3 = app.open_command_output_tab("project-a", "status"); // Same command, new tab

        // Each should create a unique tab
        assert_eq!(app.closable_tab_count(), 3);

        // All cache keys should be unique
        assert_ne!(id1.cache_key(), id2.cache_key());
        assert_ne!(id1.cache_key(), id3.cache_key());
        assert_ne!(id2.cache_key(), id3.cache_key());
    }

    #[test]
    fn test_app_command_output_update_methods() {
        let mut app = Autom8App::new();

        let id = app.open_command_output_tab("project", "status");
        let cache_key = id.cache_key();

        // Add stdout
        app.add_command_stdout(&cache_key, "output line 1".to_string());
        app.add_command_stdout(&cache_key, "output line 2".to_string());

        // Add stderr
        app.add_command_stderr(&cache_key, "warning line".to_string());

        // Verify updates
        let exec = app.get_command_execution(&cache_key).unwrap();
        assert_eq!(exec.stdout.len(), 2);
        assert_eq!(exec.stderr.len(), 1);

        // Complete the command
        app.complete_command(&cache_key, 0);

        let exec = app.get_command_execution(&cache_key).unwrap();
        assert_eq!(exec.status, CommandStatus::Completed);
        assert_eq!(exec.exit_code, Some(0));
    }

    #[test]
    fn test_app_command_output_fail_method() {
        let mut app = Autom8App::new();

        let id = app.open_command_output_tab("project", "status");
        let cache_key = id.cache_key();

        app.fail_command(&cache_key, "spawn error".to_string());

        let exec = app.get_command_execution(&cache_key).unwrap();
        assert_eq!(exec.status, CommandStatus::Failed);
        assert_eq!(exec.stderr.len(), 1);
    }

    #[test]
    fn test_app_close_command_output_tab_cleans_up() {
        let mut app = Autom8App::new();

        let id = app.open_command_output_tab("project", "status");
        let cache_key = id.cache_key();
        let tab_id = TabId::CommandOutput(cache_key.clone());

        // Verify tab and execution exist
        assert!(app.has_tab(&tab_id));
        assert!(app.get_command_execution(&cache_key).is_some());

        // Close the tab
        assert!(app.close_tab(&tab_id));

        // Verify cleanup
        assert!(!app.has_tab(&tab_id));
        assert!(app.get_command_execution(&cache_key).is_none());
    }

    #[test]
    fn test_command_status_enum() {
        assert_eq!(CommandStatus::Running, CommandStatus::Running);
        assert_ne!(CommandStatus::Running, CommandStatus::Completed);
        assert_ne!(CommandStatus::Running, CommandStatus::Failed);
        assert_ne!(CommandStatus::Completed, CommandStatus::Failed);
    }

    #[test]
    fn test_tab_id_command_output_variant() {
        let cache_key = "project:status:abc123".to_string();
        let tab_id = TabId::CommandOutput(cache_key.clone());

        // Test equality
        assert_eq!(tab_id, TabId::CommandOutput(cache_key.clone()));
        assert_ne!(tab_id, TabId::ActiveRuns);
        assert_ne!(tab_id, TabId::Projects);
        assert_ne!(tab_id, TabId::RunDetail("abc".to_string()));

        // Test hash
        let mut set = std::collections::HashSet::new();
        set.insert(tab_id.clone());
        assert!(set.contains(&TabId::CommandOutput(cache_key)));
    }

    // ========================================================================
    // Command Message and Channel Tests (US-003)
    // ========================================================================

    #[test]
    fn test_command_message_stdout() {
        let msg = CommandMessage::Stdout {
            cache_key: "test:status:123".to_string(),
            line: "output line".to_string(),
        };
        if let CommandMessage::Stdout { cache_key, line } = msg {
            assert_eq!(cache_key, "test:status:123");
            assert_eq!(line, "output line");
        } else {
            panic!("Expected Stdout variant");
        }
    }

    #[test]
    fn test_command_message_stderr() {
        let msg = CommandMessage::Stderr {
            cache_key: "test:status:123".to_string(),
            line: "error line".to_string(),
        };
        if let CommandMessage::Stderr { cache_key, line } = msg {
            assert_eq!(cache_key, "test:status:123");
            assert_eq!(line, "error line");
        } else {
            panic!("Expected Stderr variant");
        }
    }

    #[test]
    fn test_command_message_completed() {
        let msg = CommandMessage::Completed {
            cache_key: "test:status:123".to_string(),
            exit_code: 0,
        };
        if let CommandMessage::Completed {
            cache_key,
            exit_code,
        } = msg
        {
            assert_eq!(cache_key, "test:status:123");
            assert_eq!(exit_code, 0);
        } else {
            panic!("Expected Completed variant");
        }
    }

    #[test]
    fn test_command_message_failed() {
        let msg = CommandMessage::Failed {
            cache_key: "test:status:123".to_string(),
            error: "spawn error".to_string(),
        };
        if let CommandMessage::Failed { cache_key, error } = msg {
            assert_eq!(cache_key, "test:status:123");
            assert_eq!(error, "spawn error");
        } else {
            panic!("Expected Failed variant");
        }
    }

    #[test]
    fn test_poll_command_messages_stdout() {
        let mut app = Autom8App::new();

        // Open a command output tab first
        let id = app.open_command_output_tab("project", "status");
        let cache_key = id.cache_key();

        // Send a message through the channel
        app.command_tx
            .send(CommandMessage::Stdout {
                cache_key: cache_key.clone(),
                line: "test output".to_string(),
            })
            .unwrap();

        // Poll for messages
        app.poll_command_messages();

        // Verify the stdout was added
        let exec = app.get_command_execution(&cache_key).unwrap();
        assert_eq!(exec.stdout.len(), 1);
        assert_eq!(exec.stdout[0], "test output");
    }

    #[test]
    fn test_poll_command_messages_stderr() {
        let mut app = Autom8App::new();

        let id = app.open_command_output_tab("project", "status");
        let cache_key = id.cache_key();

        app.command_tx
            .send(CommandMessage::Stderr {
                cache_key: cache_key.clone(),
                line: "error output".to_string(),
            })
            .unwrap();

        app.poll_command_messages();

        let exec = app.get_command_execution(&cache_key).unwrap();
        assert_eq!(exec.stderr.len(), 1);
        assert_eq!(exec.stderr[0], "error output");
    }

    #[test]
    fn test_poll_command_messages_completed() {
        let mut app = Autom8App::new();

        let id = app.open_command_output_tab("project", "status");
        let cache_key = id.cache_key();

        app.command_tx
            .send(CommandMessage::Completed {
                cache_key: cache_key.clone(),
                exit_code: 0,
            })
            .unwrap();

        app.poll_command_messages();

        let exec = app.get_command_execution(&cache_key).unwrap();
        assert_eq!(exec.status, CommandStatus::Completed);
        assert_eq!(exec.exit_code, Some(0));
    }

    #[test]
    fn test_poll_command_messages_failed() {
        let mut app = Autom8App::new();

        let id = app.open_command_output_tab("project", "status");
        let cache_key = id.cache_key();

        app.command_tx
            .send(CommandMessage::Failed {
                cache_key: cache_key.clone(),
                error: "spawn error".to_string(),
            })
            .unwrap();

        app.poll_command_messages();

        let exec = app.get_command_execution(&cache_key).unwrap();
        assert_eq!(exec.status, CommandStatus::Failed);
        assert_eq!(exec.stderr.len(), 1);
        assert_eq!(exec.stderr[0], "spawn error");
    }

    #[test]
    fn test_poll_command_messages_multiple() {
        let mut app = Autom8App::new();

        let id = app.open_command_output_tab("project", "status");
        let cache_key = id.cache_key();

        // Send multiple messages
        app.command_tx
            .send(CommandMessage::Stdout {
                cache_key: cache_key.clone(),
                line: "line 1".to_string(),
            })
            .unwrap();
        app.command_tx
            .send(CommandMessage::Stdout {
                cache_key: cache_key.clone(),
                line: "line 2".to_string(),
            })
            .unwrap();
        app.command_tx
            .send(CommandMessage::Stderr {
                cache_key: cache_key.clone(),
                line: "error".to_string(),
            })
            .unwrap();
        app.command_tx
            .send(CommandMessage::Completed {
                cache_key: cache_key.clone(),
                exit_code: 1,
            })
            .unwrap();

        // Poll should process all messages
        app.poll_command_messages();

        let exec = app.get_command_execution(&cache_key).unwrap();
        assert_eq!(exec.stdout.len(), 2);
        assert_eq!(exec.stderr.len(), 1);
        assert_eq!(exec.status, CommandStatus::Failed); // exit code 1
        assert_eq!(exec.exit_code, Some(1));
    }

    #[test]
    fn test_poll_command_messages_ignores_unknown_cache_key() {
        let mut app = Autom8App::new();

        // Send message for a cache key that doesn't exist
        app.command_tx
            .send(CommandMessage::Stdout {
                cache_key: "nonexistent:key:123".to_string(),
                line: "should be ignored".to_string(),
            })
            .unwrap();

        // This should not panic
        app.poll_command_messages();

        // Verify no command execution was created
        assert!(app.get_command_execution("nonexistent:key:123").is_none());
    }

    #[test]
    fn test_spawn_status_command_creates_tab() {
        let mut app = Autom8App::new();

        // Note: spawn_status_command will actually try to spawn autom8,
        // which may not be in PATH during tests. We test that the tab
        // and execution are created correctly. The background thread
        // will send a Failed message if autom8 isn't found.
        app.spawn_status_command("test-project");

        // Check that a command output tab was created
        assert_eq!(app.closable_tab_count(), 1);

        // Find the tab
        let tab = app
            .tabs()
            .iter()
            .find(|t| matches!(&t.id, TabId::CommandOutput(_)));
        assert!(tab.is_some());

        let tab = tab.unwrap();
        assert!(tab.label.contains("test-project"));
        assert!(tab.label.starts_with("Status:"));
        assert!(tab.closable);

        // Check that a command execution was created
        if let TabId::CommandOutput(cache_key) = &tab.id {
            let exec = app.get_command_execution(cache_key);
            assert!(exec.is_some());
            // Initially should be running (thread spawned)
            assert_eq!(exec.unwrap().status, CommandStatus::Running);
        } else {
            panic!("Expected CommandOutput tab");
        }
    }

    #[test]
    fn test_status_tab_label_format() {
        // Per US-003: Tab title format: "Status: {project-name}"
        let id = CommandOutputId::new("my-awesome-project", "status");
        let label = id.tab_label();
        assert_eq!(label, "Status: my-awesome-project");
    }

    #[test]
    fn test_spawn_describe_command_creates_tab() {
        let mut app = Autom8App::new();

        // Note: spawn_describe_command will actually try to spawn autom8,
        // which may not be in PATH during tests. We test that the tab
        // and execution are created correctly. The background thread
        // will send a Failed message if autom8 isn't found.
        app.spawn_describe_command("test-project");

        // Check that a command output tab was created
        assert_eq!(app.closable_tab_count(), 1);

        // Find the tab
        let tab = app
            .tabs()
            .iter()
            .find(|t| matches!(&t.id, TabId::CommandOutput(_)));
        assert!(tab.is_some());

        let tab = tab.unwrap();
        assert!(tab.label.contains("test-project"));
        assert!(tab.label.starts_with("Describe:"));
        assert!(tab.closable);

        // Check that a command execution was created
        if let TabId::CommandOutput(cache_key) = &tab.id {
            let exec = app.get_command_execution(cache_key);
            assert!(exec.is_some());
            // Initially should be running (thread spawned)
            assert_eq!(exec.unwrap().status, CommandStatus::Running);
        } else {
            panic!("Expected CommandOutput tab");
        }
    }

    #[test]
    fn test_describe_tab_label_format() {
        // Per US-004: Tab title format: "Describe: {project-name}"
        let id = CommandOutputId::new("my-awesome-project", "describe");
        let label = id.tab_label();
        assert_eq!(label, "Describe: my-awesome-project");
    }

    // ========================================================================
    // US-005: Resume Menu Tests
    // ========================================================================

    #[test]
    fn test_resumable_session_info_new() {
        let info = ResumableSessionInfo::new("abc12345", "feature/test");
        assert_eq!(info.session_id, "abc12345");
        assert_eq!(info.branch_name, "feature/test");
    }

    #[test]
    fn test_resumable_session_info_truncated_id_short() {
        // Session ID <= 8 chars should not be truncated
        let info = ResumableSessionInfo::new("main", "main");
        assert_eq!(info.truncated_id(), "main");

        let info = ResumableSessionInfo::new("abcd1234", "test");
        assert_eq!(info.truncated_id(), "abcd1234");
    }

    #[test]
    fn test_resumable_session_info_truncated_id_long() {
        // Session ID > 8 chars should be truncated to first 8
        let info = ResumableSessionInfo::new("abcd12345678", "test");
        assert_eq!(info.truncated_id(), "abcd1234");

        let info = ResumableSessionInfo::new("very-long-session-id-here", "test");
        assert_eq!(info.truncated_id(), "very-lon");
    }

    #[test]
    fn test_resumable_session_info_menu_label() {
        // Format: "branch-name (session-id-truncated)"
        let info = ResumableSessionInfo::new("main", "main");
        assert_eq!(info.menu_label(), "main (main)");

        let info = ResumableSessionInfo::new("abc12345", "feature/login");
        assert_eq!(info.menu_label(), "feature/login (abc12345)");

        // Long session ID should be truncated in label
        let info = ResumableSessionInfo::new("abcd12345678", "feature/test");
        assert_eq!(info.menu_label(), "feature/test (abcd1234)");
    }

    #[test]
    fn test_is_resumable_session_stale() {
        // Stale sessions are not resumable
        let session = SessionStatus {
            metadata: crate::state::SessionMetadata {
                session_id: "test".to_string(),
                worktree_path: std::path::PathBuf::from("/tmp/test"),
                branch_name: "test-branch".to_string(),
                created_at: chrono::Utc::now(),
                last_active_at: chrono::Utc::now(),
                is_running: true,
            },
            machine_state: Some(MachineState::RunningClaude),
            current_story: None,
            is_current: false,
            is_stale: true, // Stale!
        };
        assert!(!is_resumable_session(&session));
    }

    #[test]
    fn test_is_resumable_session_running() {
        // Running sessions are resumable
        let session = SessionStatus {
            metadata: crate::state::SessionMetadata {
                session_id: "test".to_string(),
                worktree_path: std::path::PathBuf::from("/tmp/test"),
                branch_name: "test-branch".to_string(),
                created_at: chrono::Utc::now(),
                last_active_at: chrono::Utc::now(),
                is_running: true, // Running
            },
            machine_state: Some(MachineState::RunningClaude),
            current_story: None,
            is_current: false,
            is_stale: false,
        };
        assert!(is_resumable_session(&session));
    }

    #[test]
    fn test_is_resumable_session_idle_not_resumable() {
        // Idle sessions are not resumable
        let session = SessionStatus {
            metadata: crate::state::SessionMetadata {
                session_id: "test".to_string(),
                worktree_path: std::path::PathBuf::from("/tmp/test"),
                branch_name: "test-branch".to_string(),
                created_at: chrono::Utc::now(),
                last_active_at: chrono::Utc::now(),
                is_running: false,
            },
            machine_state: Some(MachineState::Idle),
            current_story: None,
            is_current: false,
            is_stale: false,
        };
        assert!(!is_resumable_session(&session));
    }

    #[test]
    fn test_is_resumable_session_completed_not_resumable() {
        // Completed sessions are not resumable
        let session = SessionStatus {
            metadata: crate::state::SessionMetadata {
                session_id: "test".to_string(),
                worktree_path: std::path::PathBuf::from("/tmp/test"),
                branch_name: "test-branch".to_string(),
                created_at: chrono::Utc::now(),
                last_active_at: chrono::Utc::now(),
                is_running: false,
            },
            machine_state: Some(MachineState::Completed),
            current_story: None,
            is_current: false,
            is_stale: false,
        };
        assert!(!is_resumable_session(&session));
    }

    #[test]
    fn test_is_resumable_session_other_states_resumable() {
        // Other states (like Reviewing, Committing) are resumable
        let states = vec![
            MachineState::RunningClaude,
            MachineState::Reviewing,
            MachineState::Correcting,
            MachineState::Committing,
            MachineState::CreatingPR,
            MachineState::LoadingSpec,
            MachineState::GeneratingSpec,
            MachineState::PickingStory,
            MachineState::Failed,
            MachineState::Initializing,
        ];

        for state in states {
            let session = SessionStatus {
                metadata: crate::state::SessionMetadata {
                    session_id: "test".to_string(),
                    worktree_path: std::path::PathBuf::from("/tmp/test"),
                    branch_name: "test-branch".to_string(),
                    created_at: chrono::Utc::now(),
                    last_active_at: chrono::Utc::now(),
                    is_running: false,
                },
                machine_state: Some(state.clone()),
                current_story: None,
                is_current: false,
                is_stale: false,
            };
            assert!(
                is_resumable_session(&session),
                "State {:?} should be resumable",
                state
            );
        }
    }

    #[test]
    fn test_is_resumable_session_no_machine_state() {
        // Sessions with no machine state are not resumable
        let session = SessionStatus {
            metadata: crate::state::SessionMetadata {
                session_id: "test".to_string(),
                worktree_path: std::path::PathBuf::from("/tmp/test"),
                branch_name: "test-branch".to_string(),
                created_at: chrono::Utc::now(),
                last_active_at: chrono::Utc::now(),
                is_running: false,
            },
            machine_state: None, // No machine state
            current_story: None,
            is_current: false,
            is_stale: false,
        };
        assert!(!is_resumable_session(&session));
    }

    #[test]
    fn test_build_context_menu_no_resumable_sessions() {
        // Test with a non-existent project (will have no sessions)
        let app = Autom8App::new();
        let items = app.build_context_menu_items("nonexistent-project-12345");

        // Should have Status, Describe, separator, Resume (disabled), separator, Clean
        assert_eq!(items.len(), 6);

        // Resume should be disabled with no session ID
        match &items[3] {
            ContextMenuItem::Action {
                label,
                action,
                enabled,
            } => {
                assert_eq!(label, "Resume");
                assert_eq!(action, &ContextMenuAction::Resume(None));
                assert!(!enabled, "Resume should be disabled when no sessions");
            }
            _ => panic!("Expected Resume action"),
        }
    }

    #[test]
    fn test_get_resumable_sessions_nonexistent_project() {
        // Non-existent project should return empty vec
        let app = Autom8App::new();
        let sessions = app.get_resumable_sessions("nonexistent-project-xyz123");
        assert!(sessions.is_empty());
    }

    #[test]
    fn test_resume_action_contains_session_id() {
        // Verify that Resume action can hold session ID
        let action = ContextMenuAction::Resume(Some("abc12345".to_string()));
        if let ContextMenuAction::Resume(Some(id)) = action {
            assert_eq!(id, "abc12345");
        } else {
            panic!("Expected Resume action with session ID");
        }

        let action_none = ContextMenuAction::Resume(None);
        assert!(matches!(action_none, ContextMenuAction::Resume(None)));
    }

    // =========================================================================
    // US-006 Clean Menu Tests
    // =========================================================================

    #[test]
    fn test_us006_cleanable_info_default() {
        // Default CleanableInfo should have zero counts
        let info = CleanableInfo::default();
        assert_eq!(info.cleanable_worktrees, 0);
        assert_eq!(info.orphaned_sessions, 0);
        assert!(!info.has_cleanable());
    }

    #[test]
    fn test_us006_cleanable_info_has_cleanable() {
        // Test has_cleanable() with various combinations
        let mut info = CleanableInfo::default();
        assert!(!info.has_cleanable(), "Empty should have nothing cleanable");

        info.cleanable_worktrees = 1;
        assert!(info.has_cleanable(), "Should have cleanable with worktrees");

        info.cleanable_worktrees = 0;
        info.orphaned_sessions = 1;
        assert!(info.has_cleanable(), "Should have cleanable with orphaned");

        info.cleanable_worktrees = 2;
        info.orphaned_sessions = 3;
        assert!(
            info.has_cleanable(),
            "Should have cleanable with both types"
        );
    }

    #[test]
    fn test_us006_get_cleanable_info_nonexistent_project() {
        // Non-existent project should return empty CleanableInfo
        let app = Autom8App::new();
        let info = app.get_cleanable_info("nonexistent-project-xyz123");
        assert_eq!(info.cleanable_worktrees, 0);
        assert_eq!(info.orphaned_sessions, 0);
        assert!(!info.has_cleanable());
    }

    #[test]
    fn test_us006_clean_menu_disabled_when_nothing_to_clean() {
        // Test with a non-existent project (will have no sessions)
        let app = Autom8App::new();
        let items = app.build_context_menu_items("nonexistent-project-12345");

        // Find the Clean menu item (last item)
        let clean_item = items.last().expect("Should have Clean item");

        match clean_item {
            ContextMenuItem::Submenu {
                label,
                id,
                enabled,
                items,
            } => {
                assert_eq!(label, "Clean");
                assert_eq!(id, "clean");
                assert!(!enabled, "Clean should be disabled when nothing to clean");
                assert!(
                    items.is_empty(),
                    "Disabled Clean should have no submenu items"
                );
            }
            _ => panic!("Expected Clean to be a Submenu"),
        }
    }

    #[test]
    fn test_us006_clean_action_variants() {
        // Verify CleanWorktrees and CleanOrphaned actions exist and are distinct
        let worktrees = ContextMenuAction::CleanWorktrees;
        let orphaned = ContextMenuAction::CleanOrphaned;

        assert_ne!(worktrees, orphaned, "Actions should be distinct");
        assert!(
            matches!(worktrees, ContextMenuAction::CleanWorktrees),
            "Should be CleanWorktrees"
        );
        assert!(
            matches!(orphaned, ContextMenuAction::CleanOrphaned),
            "Should be CleanOrphaned"
        );
    }

    #[test]
    fn test_us006_clean_menu_item_with_worktrees_action() {
        // Test creating a Clean submenu item with Worktrees action
        let submenu_items = vec![ContextMenuItem::action(
            "Worktrees (3)",
            ContextMenuAction::CleanWorktrees,
        )];
        let clean_submenu = ContextMenuItem::submenu("Clean", "clean", submenu_items);

        match clean_submenu {
            ContextMenuItem::Submenu {
                label,
                id,
                enabled,
                items,
            } => {
                assert_eq!(label, "Clean");
                assert_eq!(id, "clean");
                assert!(enabled, "Clean should be enabled with items");
                assert_eq!(items.len(), 1);

                // Verify the submenu item
                match &items[0] {
                    ContextMenuItem::Action {
                        label,
                        action,
                        enabled,
                    } => {
                        assert_eq!(label, "Worktrees (3)");
                        assert_eq!(action, &ContextMenuAction::CleanWorktrees);
                        assert!(*enabled);
                    }
                    _ => panic!("Expected Action item"),
                }
            }
            _ => panic!("Expected Submenu"),
        }
    }

    #[test]
    fn test_us006_clean_menu_item_with_orphaned_action() {
        // Test creating a Clean submenu item with Orphaned action
        let submenu_items = vec![ContextMenuItem::action(
            "Orphaned (5)",
            ContextMenuAction::CleanOrphaned,
        )];
        let clean_submenu = ContextMenuItem::submenu("Clean", "clean", submenu_items);

        match clean_submenu {
            ContextMenuItem::Submenu { items, .. } => {
                assert_eq!(items.len(), 1);

                // Verify the submenu item
                match &items[0] {
                    ContextMenuItem::Action { label, action, .. } => {
                        assert_eq!(label, "Orphaned (5)");
                        assert_eq!(action, &ContextMenuAction::CleanOrphaned);
                    }
                    _ => panic!("Expected Action item"),
                }
            }
            _ => panic!("Expected Submenu"),
        }
    }

    #[test]
    fn test_us006_clean_menu_with_both_options() {
        // Test creating a Clean submenu with both Worktrees and Orphaned options
        let submenu_items = vec![
            ContextMenuItem::action("Worktrees (2)", ContextMenuAction::CleanWorktrees),
            ContextMenuItem::action("Orphaned (1)", ContextMenuAction::CleanOrphaned),
        ];
        let clean_submenu = ContextMenuItem::submenu("Clean", "clean", submenu_items);

        match clean_submenu {
            ContextMenuItem::Submenu { enabled, items, .. } => {
                assert!(enabled, "Clean should be enabled with items");
                assert_eq!(items.len(), 2);

                // Verify both items
                match &items[0] {
                    ContextMenuItem::Action { action, .. } => {
                        assert_eq!(action, &ContextMenuAction::CleanWorktrees);
                    }
                    _ => panic!("Expected CleanWorktrees action"),
                }

                match &items[1] {
                    ContextMenuItem::Action { action, .. } => {
                        assert_eq!(action, &ContextMenuAction::CleanOrphaned);
                    }
                    _ => panic!("Expected CleanOrphaned action"),
                }
            }
            _ => panic!("Expected Submenu"),
        }
    }

    #[test]
    fn test_us006_spawn_clean_worktrees_command_creates_tab() {
        let mut app = Autom8App::new();

        // Note: spawn_clean_worktrees_command will actually try to spawn autom8,
        // but we're just testing that a tab is created
        let initial_tab_count = app.tab_count();

        app.spawn_clean_worktrees_command("test-project");

        // Should have created a new tab
        assert_eq!(app.tab_count(), initial_tab_count + 1);

        // Tab should be for clean-worktrees command
        let tab = app.tabs().last().unwrap();
        assert!(tab.closable, "Command output tab should be closable");
        assert!(
            tab.label.contains("Clean-worktrees"),
            "Tab label should contain 'Clean-worktrees'"
        );
    }

    #[test]
    fn test_us006_spawn_clean_orphaned_command_creates_tab() {
        let mut app = Autom8App::new();

        // Note: spawn_clean_orphaned_command will actually try to spawn autom8,
        // but we're just testing that a tab is created
        let initial_tab_count = app.tab_count();

        app.spawn_clean_orphaned_command("test-project");

        // Should have created a new tab
        assert_eq!(app.tab_count(), initial_tab_count + 1);

        // Tab should be for clean-orphaned command
        let tab = app.tabs().last().unwrap();
        assert!(tab.closable, "Command output tab should be closable");
        assert!(
            tab.label.contains("Clean-orphaned"),
            "Tab label should contain 'Clean-orphaned'"
        );
    }

    #[test]
    fn test_us006_is_cleanable_session_helper() {
        use crate::state::{SessionMetadata, SessionStatus};
        use std::path::PathBuf;

        // Create a test session metadata
        let metadata = SessionMetadata {
            session_id: "test123".to_string(),
            worktree_path: PathBuf::from("/tmp/test"),
            branch_name: "feature/test".to_string(),
            created_at: chrono::Utc::now(),
            last_active_at: chrono::Utc::now(),
            is_running: false,
        };

        // Test Completed state - should be cleanable
        let completed_session = SessionStatus {
            metadata: metadata.clone(),
            machine_state: Some(MachineState::Completed),
            current_story: None,
            is_current: false,
            is_stale: false,
        };
        assert!(
            is_cleanable_session(&completed_session),
            "Completed session should be cleanable"
        );

        // Test Failed state - should be cleanable
        let failed_session = SessionStatus {
            metadata: metadata.clone(),
            machine_state: Some(MachineState::Failed),
            current_story: None,
            is_current: false,
            is_stale: false,
        };
        assert!(
            is_cleanable_session(&failed_session),
            "Failed session should be cleanable"
        );

        // Test RunningClaude state - should NOT be cleanable (safety)
        let running_session = SessionStatus {
            metadata: metadata.clone(),
            machine_state: Some(MachineState::RunningClaude),
            current_story: None,
            is_current: false,
            is_stale: false,
        };
        assert!(
            !is_cleanable_session(&running_session),
            "Running session should NOT be cleanable"
        );

        // Test session with is_running = true - should NOT be cleanable
        let mut metadata_running = metadata.clone();
        metadata_running.is_running = true;
        let is_running_session = SessionStatus {
            metadata: metadata_running,
            machine_state: Some(MachineState::Completed), // Even if state says completed
            current_story: None,
            is_current: false,
            is_stale: false,
        };
        assert!(
            !is_cleanable_session(&is_running_session),
            "Session with is_running=true should NOT be cleanable"
        );
    }
}
