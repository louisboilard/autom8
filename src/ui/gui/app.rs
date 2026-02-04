//! GUI application entry point.
//!
//! This module contains the eframe application setup and main window
//! configuration for the autom8 GUI.

use crate::claude::ClaudeUsage;
use crate::error::{Autom8Error, Result};
use crate::state::{IterationStatus, MachineState, SessionStatus, StateManager};
use crate::ui::gui::components::{
    badge_background_color, format_duration, format_relative_time, format_state, state_to_color,
    strip_worktree_prefix, truncate_with_ellipsis, CollapsibleSection, MAX_BRANCH_LENGTH,
};
use crate::ui::gui::config::{
    BoolFieldChanges, ConfigBoolField, ConfigEditorActions, ConfigScope, ConfigTabState,
    ConfigTextField, TextFieldChanges, CONFIG_SCOPE_ROW_HEIGHT, CONFIG_SCOPE_ROW_PADDING_H,
    CONFIG_SCOPE_ROW_PADDING_V,
};
use crate::ui::gui::modal::{Modal, ModalAction, ModalButton};
use crate::ui::gui::theme::{self, colors, rounding, spacing};
use crate::ui::gui::typography::{self, FontSize, FontWeight};
use crate::ui::shared::{
    load_project_run_history, load_ui_data, ProjectData, RunHistoryEntry, SessionData,
};
use eframe::egui::{self, Color32, Key, Order, Pos2, Rect, Rounding, Sense, Stroke, Vec2};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Default window width in pixels.
const DEFAULT_WIDTH: f32 = 1200.0;

/// Default window height in pixels.
const DEFAULT_HEIGHT: f32 = 800.0;

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
// Active Runs View Constants
// ============================================================================

/// Number of output lines to display when getting output for a session.
/// Used by `get_output_for_session()` to limit output from both live and iteration sources.
const OUTPUT_LINES_TO_SHOW: usize = 50;

/// Freshness threshold for live output in seconds.
/// Live output older than this is considered stale and we fall back to iteration output.
/// This matches the TUI's behavior for consistent user experience.
const LIVE_OUTPUT_FRESHNESS_SECS: i64 = 5;

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

/// Breakpoint width below which the expanded session view switches from
/// side-by-side panels to vertically stacked panels. This ensures content
/// remains readable at narrow window widths.
const RESPONSIVE_STACK_BREAKPOINT: f32 = 800.0;

/// Minimum width for each panel in the expanded session view when
/// in side-by-side layout. Prevents content from becoming unreadable.
const EXPANDED_PANEL_MIN_WIDTH: f32 = 300.0;

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

/// Size of the sidebar mascot icon in pixels (US-005).
/// Sized for strong visual presence in the sidebar.
const SIDEBAR_ICON_SIZE: f32 = 120.0;

// ============================================================================
// Context Menu Constants (Right-Click Context Menu - US-002)
// ============================================================================

/// Minimum width for the context menu (US-001).
const CONTEXT_MENU_MIN_WIDTH: f32 = 100.0;

/// Maximum width for the context menu (US-001).
const CONTEXT_MENU_MAX_WIDTH: f32 = 300.0;

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
    /// Whether the item is currently hovered (only true for enabled items).
    hovered: bool,
    /// Whether the item is hovered regardless of enabled state (US-006: for tooltips).
    hovered_raw: bool,
    /// The screen-space rect of the item (for positioning submenus).
    rect: Rect,
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Calculate the final menu width from a measured text width (US-001).
///
/// Applies padding and clamping to the max text width to get the final menu width.
/// This is separated from the text measurement for testability.
///
/// # Arguments
/// * `max_text_width` - The maximum text width among all menu item labels
/// * `has_submenu` - Whether any items have submenus (adds extra space for arrow)
fn calculate_menu_width_from_text_width(max_text_width: f32) -> f32 {
    // Total width = text width + left padding (24px) + right padding (24px)
    // The padding is ~48px total (CONTEXT_MENU_PADDING_H * 4)
    let padding = CONTEXT_MENU_PADDING_H * 2.0 + CONTEXT_MENU_PADDING_H * 2.0;
    let calculated_width = max_text_width + padding;

    // Clamp to min/max bounds
    calculated_width.clamp(CONTEXT_MENU_MIN_WIDTH, CONTEXT_MENU_MAX_WIDTH)
}

/// Calculate the dynamic width for a context menu based on its items (US-001).
///
/// The width is determined by:
/// 1. Measuring the text width of each label using the Body font
/// 2. Adding horizontal padding (24px each side = 48px total)
/// 3. Adding submenu arrow space for items with submenus (arrow size + padding)
/// 4. Clamping to min/max bounds (100px-300px)
fn calculate_context_menu_width(ctx: &egui::Context, items: &[ContextMenuItem]) -> f32 {
    let font_id = typography::font(FontSize::Body, FontWeight::Regular);

    let max_text_width = items
        .iter()
        .filter_map(|item| {
            match item {
                ContextMenuItem::Action { label, .. } => {
                    // Measure text width using egui's font system
                    let galley = ctx.fonts(|fonts| {
                        fonts.layout_no_wrap(label.clone(), font_id.clone(), Color32::WHITE)
                    });
                    Some(galley.rect.width())
                }
                ContextMenuItem::Submenu { label, .. } => {
                    // Submenu items need extra space for the arrow indicator
                    let galley = ctx.fonts(|fonts| {
                        fonts.layout_no_wrap(label.clone(), font_id.clone(), Color32::WHITE)
                    });
                    // Add space for the arrow: arrow_size + padding between text and arrow
                    Some(galley.rect.width() + CONTEXT_MENU_ARROW_SIZE + CONTEXT_MENU_PADDING_H)
                }
                ContextMenuItem::Separator => None, // Separators don't contribute to width
            }
        })
        .fold(0.0_f32, |max, width| max.max(width));

    calculate_menu_width_from_text_width(max_text_width)
}

// ============================================================================
// Output Display Helpers (US-002: Improve Output Display Update Mechanism)
// ============================================================================

/// Result of determining which output to display for a session.
///
/// This enum represents the source of output to display in the session card,
/// providing clear semantics for the rendering code.
#[derive(Debug, Clone, PartialEq)]
enum OutputSource {
    /// Fresh live output from Claude (within freshness threshold).
    /// Contains the lines to display (already limited to OUTPUT_LINES_TO_SHOW).
    Live(Vec<String>),
    /// Archived iteration output (fallback when live output is stale).
    /// Contains the lines to display (already limited to OUTPUT_LINES_TO_SHOW).
    Iteration(Vec<String>),
    /// Status message fallback when no output is available.
    /// Contains a descriptive message based on the machine state (e.g., "Waiting for output...").
    StatusMessage(String),
    /// No live output data available (session may not be actively running).
    NoData,
}

/// Get the appropriate output to display for a session.
///
/// This function implements intelligent output source selection, matching
/// the TUI's `get_output_snippet` behavior for consistent user experience.
///
/// ## Priority (matches TUI implementation):
/// 1. **Fresh live output** - If `machine_state == RunningClaude` AND live output exists
///    AND output is fresh (< 5 seconds old) AND `output_lines` is not empty
/// 2. **Iteration output** - If the latest iteration has an `output_snippet`
/// 3. **Status message** - Fallback based on the current machine state
///
/// ## Freshness Check:
/// Live output is considered "fresh" if updated within `LIVE_OUTPUT_FRESHNESS_SECS` (5 seconds).
/// This prevents showing stale output when Claude pauses or transitions between phases,
/// providing a smoother experience without flickering or jarring transitions.
///
/// ## Arguments
/// * `session` - The session data to get output for
///
/// ## Returns
/// An `OutputSource` variant indicating which output to display and its content.
fn get_output_for_session(session: &SessionData) -> OutputSource {
    // Get the machine state (default to Idle if no run)
    let machine_state = session
        .run
        .as_ref()
        .map(|r| r.machine_state)
        .unwrap_or(MachineState::Idle);

    // Priority 1: Check for fresh live output when Claude is running
    if machine_state == MachineState::RunningClaude {
        if let Some(ref live) = session.live_output {
            // Check if live output is fresh (within freshness threshold)
            let age = chrono::Utc::now().signed_duration_since(live.updated_at);
            if age.num_seconds() < LIVE_OUTPUT_FRESHNESS_SECS && !live.output_lines.is_empty() {
                // Take last OUTPUT_LINES_TO_SHOW lines from live output
                let take_count = OUTPUT_LINES_TO_SHOW.min(live.output_lines.len());
                let start = live.output_lines.len().saturating_sub(take_count);
                let lines: Vec<String> = live.output_lines[start..].to_vec();
                return OutputSource::Live(lines);
            }
        }
    }

    // Priority 2: Get output from the current or last iteration
    if let Some(ref run) = session.run {
        if let Some(iter) = run.iterations.last() {
            if !iter.output_snippet.is_empty() {
                // Take last OUTPUT_LINES_TO_SHOW lines of output
                let lines: Vec<String> = iter
                    .output_snippet
                    .lines()
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .take(OUTPUT_LINES_TO_SHOW)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .map(|s| s.to_string())
                    .collect();
                return OutputSource::Iteration(lines);
            }
        }
    }

    // Priority 3: Fall back to status message based on machine state
    // If there's no live output at all, show NoData (session not actively running)
    if session.live_output.is_none() {
        return OutputSource::NoData;
    }

    // Fall back to status message for other states
    let message = match machine_state {
        MachineState::Idle => "Waiting to start...",
        MachineState::LoadingSpec => "Loading spec file...",
        MachineState::GeneratingSpec => "Generating spec from markdown...",
        MachineState::Initializing => "Initializing run...",
        MachineState::PickingStory => "Selecting next story...",
        // US-004: For RunningClaude, show "Waiting" only when there's no iteration output
        // (i.e., we got here because there's no output at all to show).
        // This ensures previous iteration output remains visible until new output arrives.
        MachineState::RunningClaude => "Waiting for output...",
        MachineState::Reviewing => "Reviewing changes...",
        MachineState::Correcting => "Applying corrections...",
        MachineState::Committing => "Committing changes...",
        MachineState::CreatingPR => "Creating pull request...",
        MachineState::Completed => "Run completed successfully!",
        MachineState::Failed => "Run failed.",
    };
    OutputSource::StatusMessage(message.to_string())
}

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

/// Format session status data as plain text lines for display.
///
/// US-002: This replaces the CLI output formatting from status.rs for GUI display.
/// Shows: session ID, branch, state, current story, started time.
fn format_sessions_as_text(sessions: &[SessionStatus]) -> Vec<String> {
    let mut lines = Vec::new();

    if sessions.is_empty() {
        lines.push("No sessions found for this project.".to_string());
        return lines;
    }

    lines.push("Sessions for this project:".to_string());
    lines.push(String::new());

    for session in sessions {
        let metadata = &session.metadata;

        // Session indicator based on state
        let indicator = if session.is_stale {
            "✗"
        } else if session.is_current {
            "→"
        } else if metadata.is_running {
            "●"
        } else {
            "○"
        };

        // Build session header line
        let mut header = format!("{} {}", indicator, metadata.session_id);
        if session.is_current {
            header.push_str(" (current)");
        }
        if session.is_stale {
            header.push_str(" [stale]");
        }
        lines.push(header);

        // Branch
        lines.push(format!("  Branch:  {}", metadata.branch_name));

        // State
        if let Some(state) = &session.machine_state {
            let state_str = format_machine_state_text(state);
            lines.push(format!("  State:   {}", state_str));
        }

        // Current story (if any)
        if let Some(story) = &session.current_story {
            lines.push(format!("  Story:   {}", story));
        }

        // Started time
        lines.push(format!(
            "  Started: {}",
            metadata.created_at.format("%Y-%m-%d %H:%M")
        ));

        lines.push(String::new());
    }

    // Summary line
    let running_count = sessions
        .iter()
        .filter(|s| s.metadata.is_running && !s.is_stale)
        .count();
    let stale_count = sessions.iter().filter(|s| s.is_stale).count();

    let mut summary = format!(
        "({} session{}",
        sessions.len(),
        if sessions.len() == 1 { "" } else { "s" }
    );
    if running_count > 0 {
        summary.push_str(&format!(", {} running", running_count));
    }
    if stale_count > 0 {
        summary.push_str(&format!(", {} stale", stale_count));
    }
    summary.push(')');
    lines.push(summary);

    lines
}

/// Format machine state for text display.
fn format_machine_state_text(state: &MachineState) -> &'static str {
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

/// Format resume session information as plain text lines for display.
///
/// US-005: Shows session info instead of spawning subprocess.
/// Info includes: session ID, branch, worktree path, current state.
/// Shows message with instructions on how to resume in terminal.
fn format_resume_info_as_text(session: &ResumableSessionInfo) -> Vec<String> {
    let mut lines = Vec::new();

    lines.push("Resume Session Information".to_string());
    lines.push(String::new());
    lines.push(format!("Session ID:    {}", session.session_id));
    lines.push(format!("Branch:        {}", session.branch_name));
    lines.push(format!(
        "Worktree Path: {}",
        session.worktree_path.display()
    ));
    lines.push(format!(
        "Current State: {}",
        format_machine_state_text(&session.machine_state)
    ));
    lines.push(String::new());
    lines.push(format!(
        "To resume, run `autom8 resume --session {}` in terminal",
        session.session_id
    ));

    lines
}

/// Format project description as plain text lines for display.
///
/// US-003: This replaces the CLI output formatting from print_project_description() for GUI display.
/// Shows: project name, path, status, specs with progress, file counts.
fn format_project_description_as_text(desc: &crate::config::ProjectDescription) -> Vec<String> {
    use crate::state::RunStatus;

    let mut lines = Vec::new();

    // Project header
    lines.push(format!("Project: {}", desc.name));
    lines.push(format!("Path: {}", desc.path.display()));
    lines.push(String::new());

    // Status
    let status_text = match desc.run_status {
        Some(RunStatus::Running) => "[running]",
        Some(RunStatus::Failed) => "[failed]",
        Some(RunStatus::Interrupted) => "[interrupted]",
        Some(RunStatus::Completed) => "[completed]",
        None => "[idle]",
    };
    lines.push(format!("Status: {}", status_text));

    // Branch (if any)
    if let Some(branch) = &desc.current_branch {
        lines.push(format!("Branch: {}", branch));
    }

    // Current story (if any)
    if let Some(story) = &desc.current_story {
        lines.push(format!("Current Story: {}", story));
    }
    lines.push(String::new());

    // Specs
    if desc.specs.is_empty() {
        lines.push("No specs found.".to_string());
    } else {
        lines.push(format!("Specs: ({} total)", desc.specs.len()));
        lines.push(String::new());

        for spec in &desc.specs {
            lines.extend(format_spec_summary_as_text(spec));
        }
    }

    // File counts summary
    lines.push("─────────────────────────────────────────────────────────".to_string());
    lines.push(format!(
        "Files: {} spec md, {} spec json, {} archived runs",
        desc.spec_md_count,
        desc.specs.len(),
        desc.runs_count
    ));

    lines
}

/// Format a single spec summary as plain text lines.
///
/// Shows full details (with user stories) only for the active spec.
/// All other specs (including when no spec is active) show condensed view.
fn format_spec_summary_as_text(spec: &crate::config::SpecSummary) -> Vec<String> {
    let mut lines = Vec::new();

    // Show "(Active)" indicator for the active spec
    let active_label = if spec.is_active { " (Active)" } else { "" };
    lines.push(format!("━━━ {}{}", spec.filename, active_label));

    // Only show full details for the active spec
    // All other specs (or when no spec is active) show condensed view
    if !spec.is_active {
        let desc_preview = if spec.description.len() > 80 {
            format!("{}...", &spec.description[..80])
        } else {
            spec.description.clone()
        };
        let first_line = desc_preview.lines().next().unwrap_or(&desc_preview);
        lines.push(first_line.to_string());
        lines.push(format!(
            "({}/{} stories complete)",
            spec.completed_count, spec.total_count
        ));
        lines.push(String::new());
        return lines;
    }

    // Full display for active spec only
    lines.push(format!("Project: {}", spec.project_name));
    lines.push(format!("Branch:  {}", spec.branch_name));

    // Description preview (first line, truncated to 100 chars)
    let desc_preview = if spec.description.len() > 100 {
        format!("{}...", &spec.description[..100])
    } else {
        spec.description.clone()
    };
    let first_line = desc_preview.lines().next().unwrap_or(&desc_preview);
    lines.push(format!("Description: {}", first_line));
    lines.push(String::new());

    // Progress bar (simple text version)
    let progress_bar = make_progress_bar_text(spec.completed_count, spec.total_count, 12);
    lines.push(format!(
        "Progress: [{}] {}/{} stories complete",
        progress_bar, spec.completed_count, spec.total_count
    ));
    lines.push(String::new());

    // User stories
    lines.push("User Stories:".to_string());
    for story in &spec.stories {
        let status_icon = if story.passes { "✓" } else { "○" };
        lines.push(format!("  {} {}: {}", status_icon, story.id, story.title));
    }
    lines.push(String::new());

    lines
}

/// Create a simple text progress bar.
fn make_progress_bar_text(completed: usize, total: usize, width: usize) -> String {
    if total == 0 {
        return " ".repeat(width);
    }
    let filled = (completed * width) / total;
    let empty = width - filled;
    format!("{}{}", "█".repeat(filled), "░".repeat(empty))
}

/// Format cleanup summary as plain text lines for display.
///
/// US-004: This formats CleanupSummary from direct clean operations for GUI display.
/// Shows: sessions removed, worktrees removed, bytes freed, skipped sessions, errors.
fn format_cleanup_summary_as_text(
    summary: &crate::commands::CleanupSummary,
    operation: &str,
) -> Vec<String> {
    use crate::commands::format_bytes_display;

    let mut lines = Vec::new();

    lines.push(format!("Cleanup Operation: {}", operation));
    lines.push(String::new());

    // Results section
    if summary.sessions_removed == 0 && summary.worktrees_removed == 0 {
        lines.push("No sessions or worktrees were removed.".to_string());
    } else {
        let freed_str = format_bytes_display(summary.bytes_freed);
        lines.push(format!(
            "Removed {} session{}, {} worktree{}, freed {}",
            summary.sessions_removed,
            if summary.sessions_removed == 1 {
                ""
            } else {
                "s"
            },
            summary.worktrees_removed,
            if summary.worktrees_removed == 1 {
                ""
            } else {
                "s"
            },
            freed_str
        ));
    }

    // Skipped sessions
    if !summary.sessions_skipped.is_empty() {
        lines.push(String::new());
        lines.push(format!(
            "Skipped {} session{}:",
            summary.sessions_skipped.len(),
            if summary.sessions_skipped.len() == 1 {
                ""
            } else {
                "s"
            }
        ));
        for skipped in &summary.sessions_skipped {
            lines.push(format!("  - {}: {}", skipped.session_id, skipped.reason));
        }
    }

    // Errors
    if !summary.errors.is_empty() {
        lines.push(String::new());
        lines.push("Errors during cleanup:".to_string());
        for error in &summary.errors {
            lines.push(format!("  - {}", error));
        }
    }

    lines
}

/// Format data cleanup summary as plain text lines for display.
///
/// US-003: This formats DataCleanupSummary from clean_data_direct() for GUI display.
/// Shows: specs removed, runs removed, bytes freed, errors.
fn format_data_cleanup_summary_as_text(
    summary: &crate::commands::DataCleanupSummary,
) -> Vec<String> {
    use crate::commands::format_bytes_display;

    let mut lines = Vec::new();

    lines.push("Cleanup Operation: Clean Data".to_string());
    lines.push(String::new());

    // Results section
    if summary.specs_removed == 0 && summary.runs_removed == 0 {
        lines.push("No specs or runs were removed.".to_string());
    } else {
        let freed_str = format_bytes_display(summary.bytes_freed);
        lines.push(format!(
            "Removed {} spec{}, {} run{}, freed {}",
            summary.specs_removed,
            if summary.specs_removed == 1 { "" } else { "s" },
            summary.runs_removed,
            if summary.runs_removed == 1 { "" } else { "s" },
            freed_str
        ));
    }

    // Errors
    if !summary.errors.is_empty() {
        lines.push(String::new());
        lines.push("Errors during cleanup:".to_string());
        for error in &summary.errors {
            lines.push(format!("  - {}", error));
        }
    }

    lines
}

/// Format removal summary as plain text lines for display.
///
/// US-004: This formats RemovalSummary from remove_project_direct() for GUI display.
/// Shows: worktrees removed, config deleted, bytes freed, skipped worktrees, errors.
fn format_removal_summary_as_text(
    summary: &crate::commands::RemovalSummary,
    project_name: &str,
) -> Vec<String> {
    use crate::commands::format_bytes_display;

    let mut lines = Vec::new();

    lines.push(format!("Remove Project: {}", project_name));
    lines.push(String::new());

    // Results section
    if summary.worktrees_removed == 0 && !summary.config_deleted {
        if summary.errors.is_empty() {
            lines.push("Nothing was removed.".to_string());
        } else {
            lines.push("Failed to remove project.".to_string());
        }
    } else {
        let freed_str = format_bytes_display(summary.bytes_freed);
        let mut results = Vec::new();

        if summary.worktrees_removed > 0 {
            results.push(format!(
                "{} worktree{}",
                summary.worktrees_removed,
                if summary.worktrees_removed == 1 {
                    ""
                } else {
                    "s"
                }
            ));
        }

        if summary.config_deleted {
            results.push("config directory".to_string());
        }

        lines.push(format!("Removed: {}", results.join(", ")));
        lines.push(format!("Freed: {}", freed_str));
    }

    // Skipped worktrees
    if !summary.worktrees_skipped.is_empty() {
        lines.push(String::new());
        lines.push(format!(
            "Skipped {} worktree{} (active runs):",
            summary.worktrees_skipped.len(),
            if summary.worktrees_skipped.len() == 1 {
                ""
            } else {
                "s"
            }
        ));
        for skipped in &summary.worktrees_skipped {
            lines.push(format!(
                "  - {}: {}",
                skipped.path.display(),
                skipped.reason
            ));
        }
    }

    // Errors
    if !summary.errors.is_empty() {
        lines.push(String::new());
        lines.push("Errors during removal:".to_string());
        for error in &summary.errors {
            lines.push(format!("  - {}", error));
        }
    }

    // Success message
    if summary.errors.is_empty() && (summary.worktrees_removed > 0 || summary.config_deleted) {
        lines.push(String::new());
        lines.push(format!(
            "Project '{}' has been removed from autom8.",
            project_name
        ));
    }

    lines
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
        /// Optional hint/tooltip shown when disabled (US-006).
        hint: Option<String>,
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
            hint: None,
        }
    }

    /// Create a disabled submenu item with an optional hint/tooltip (US-006).
    pub fn submenu_disabled(
        label: impl Into<String>,
        id: impl Into<String>,
        hint: impl Into<String>,
    ) -> Self {
        Self::Submenu {
            label: label.into(),
            id: id.into(),
            enabled: false,
            items: Vec::new(),
            hint: Some(hint.into()),
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
    /// Clean data (specs and archived runs) for the project.
    CleanData,
    /// Remove the project from autom8 entirely.
    RemoveProject,
}

/// Information about a resumable session for display in the context menu.
/// This is a simplified view of SessionStatus for the GUI.
#[derive(Debug, Clone)]
pub struct ResumableSessionInfo {
    /// The session ID (e.g., "main" or 8-char hash).
    pub session_id: String,
    /// The branch name being worked on.
    pub branch_name: String,
    /// The worktree path where this session is running.
    pub worktree_path: std::path::PathBuf,
    /// The current machine state (e.g., RunningClaude, Reviewing).
    pub machine_state: MachineState,
}

impl ResumableSessionInfo {
    /// Create a new resumable session info.
    pub fn new(
        session_id: impl Into<String>,
        branch_name: impl Into<String>,
        worktree_path: std::path::PathBuf,
        machine_state: MachineState,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            branch_name: branch_name.into(),
            worktree_path,
            machine_state,
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
/// Contains counts for worktrees, orphaned sessions, specs, and runs.
#[derive(Debug, Clone, Default)]
pub struct CleanableInfo {
    /// Number of cleanable worktrees (non-main sessions with existing worktrees and no active runs).
    /// US-006: Counts any worktree that can be cleaned, not just completed sessions.
    pub cleanable_worktrees: usize,
    /// Number of orphaned sessions (worktree deleted but session state remains).
    pub orphaned_sessions: usize,
    /// Number of cleanable spec files (pairs of .json/.md counted as 1).
    /// US-002: Specs used by active sessions are excluded.
    pub cleanable_specs: usize,
    /// Number of cleanable archived runs in the runs/ directory.
    /// US-002: Runs used by active sessions are excluded.
    pub cleanable_runs: usize,
}

impl CleanableInfo {
    /// Returns true if there's anything to clean.
    /// US-002: Now also considers specs and runs.
    pub fn has_cleanable(&self) -> bool {
        self.cleanable_worktrees > 0
            || self.orphaned_sessions > 0
            || self.cleanable_specs > 0
            || self.cleanable_runs > 0
    }
}

/// Check if a session is cleanable.
///
/// US-006: Updated to consider any non-running session as cleanable.
/// A session is cleanable if it doesn't have an active run (is_running=false).
/// This makes the Clean menu more useful by enabling it for any worktree.
#[allow(dead_code)] // Keep for potential future use and tests
fn is_cleanable_session(session: &SessionStatus) -> bool {
    // US-006: Simply check if the session has an active run
    // Any session without an active run can be cleaned
    !session.metadata.is_running
}

/// Count cleanable spec files in the spec directory.
///
/// US-002: Spec files come in pairs (.json and .md with the same base name).
/// Each pair is counted as a single spec, not duplicated.
/// Specs whose paths are in `active_spec_paths` are excluded.
fn count_cleanable_specs(
    spec_dir: &std::path::Path,
    active_spec_paths: &std::collections::HashSet<std::path::PathBuf>,
) -> usize {
    if !spec_dir.exists() {
        return 0;
    }

    // Collect all .json spec files (we use .json as the canonical file for counting)
    let mut cleanable_count = 0;

    if let Ok(entries) = std::fs::read_dir(spec_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                // Check if this spec is used by an active session
                if !active_spec_paths.contains(&path) {
                    cleanable_count += 1;
                }
            }
        }
    }

    cleanable_count
}

/// Count cleanable archived run files in the runs directory.
///
/// US-002: All files in the runs/ directory are considered cleanable.
fn count_cleanable_runs(runs_dir: &std::path::Path) -> usize {
    if !runs_dir.exists() {
        return 0;
    }

    std::fs::read_dir(runs_dir)
        .map(|entries| entries.filter_map(|e| e.ok()).count())
        .unwrap_or(0)
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
    /// Project was successfully removed (US-005: remove from sidebar).
    ProjectRemoved { project_name: String },
    /// Cleanup operation completed with result (US-007: show result modal).
    CleanupCompleted { result: CleanupResult },
}

// ============================================================================
// Confirmation Dialog Types (US-004)
// ============================================================================

/// Type of clean operation pending confirmation.
#[derive(Debug, Clone, PartialEq)]
pub enum PendingCleanOperation {
    /// Clean worktrees for a project.
    Worktrees { project_name: String },
    /// Clean orphaned sessions for a project.
    Orphaned { project_name: String },
    /// Clean data (specs and archived runs) for a project.
    Data {
        project_name: String,
        specs_count: usize,
        runs_count: usize,
    },
    /// Remove a project from autom8 entirely.
    RemoveProject { project_name: String },
}

impl PendingCleanOperation {
    /// Get the title for the confirmation dialog.
    fn title(&self) -> &'static str {
        match self {
            Self::Worktrees { .. } => "Clean Worktrees",
            Self::Orphaned { .. } => "Clean Orphaned Sessions",
            // US-004: Modal title is "Clean Project Data"
            Self::Data { .. } => "Clean Project Data",
            Self::RemoveProject { .. } => "Remove Project",
        }
    }

    /// Get the label for the confirm button.
    /// US-004: Data cleanup uses "Delete" as the destructive action label.
    fn confirm_button_label(&self) -> &'static str {
        match self {
            Self::Data { .. } => "Delete",
            _ => "Confirm",
        }
    }

    /// Get the message for the confirmation dialog.
    fn message(&self) -> String {
        match self {
            Self::Worktrees { project_name } => {
                format!(
                    "This will remove completed worktrees and their session state for '{}'.\n\n\
                     Are you sure you want to continue?",
                    project_name
                )
            }
            Self::Orphaned { project_name } => {
                format!(
                    "This will remove session state for orphaned sessions (where the worktree \
                     has been deleted) for '{}'.\n\n\
                     Are you sure you want to continue?",
                    project_name
                )
            }
            Self::Data {
                project_name,
                specs_count,
                runs_count,
            } => {
                // US-004: List archived runs first, then specs (per acceptance criteria)
                let mut items = Vec::new();
                if *runs_count > 0 {
                    items.push(format!(
                        "{} archived run{}",
                        runs_count,
                        if *runs_count == 1 { "" } else { "s" }
                    ));
                }
                if *specs_count > 0 {
                    items.push(format!(
                        "{} spec{}",
                        specs_count,
                        if *specs_count == 1 { "" } else { "s" }
                    ));
                }
                let items_str = items.join(", ");
                format!(
                    "This will delete {} for '{}'.\n\n\
                     Are you sure you want to continue?",
                    items_str, project_name
                )
            }
            Self::RemoveProject { project_name } => {
                format!(
                    "This will remove all worktrees (except those with active runs) and delete \
                     the autom8 configuration for '{}'.\n\n\
                     This cannot be undone.",
                    project_name
                )
            }
        }
    }

    /// Get the project name.
    fn project_name(&self) -> &str {
        match self {
            Self::Worktrees { project_name }
            | Self::Orphaned { project_name }
            | Self::Data { project_name, .. }
            | Self::RemoveProject { project_name } => project_name,
        }
    }
}

// ============================================================================
// Result Modal Types (US-007)
// ============================================================================

/// Result of a cleanup operation to display in a modal.
///
/// US-007: After clean or remove operations complete, show a result summary modal.
/// This enum stores the summary data from the cleanup operation so it can be
/// displayed in a modal after the operation completes.
#[derive(Debug, Clone)]
pub enum CleanupResult {
    /// Result from a worktree cleanup operation.
    Worktrees {
        project_name: String,
        worktrees_removed: usize,
        sessions_removed: usize,
        bytes_freed: u64,
        skipped_count: usize,
        error_count: usize,
    },
    /// Result from an orphaned session cleanup operation.
    Orphaned {
        project_name: String,
        sessions_removed: usize,
        bytes_freed: u64,
        error_count: usize,
    },
    /// Result from a project removal operation.
    RemoveProject {
        project_name: String,
        worktrees_removed: usize,
        config_deleted: bool,
        bytes_freed: u64,
        skipped_count: usize,
        error_count: usize,
    },
    /// Result from a data cleanup operation (specs and archived runs).
    Data {
        project_name: String,
        specs_removed: usize,
        runs_removed: usize,
        bytes_freed: u64,
        error_count: usize,
    },
}

impl CleanupResult {
    /// Get the title for the result modal.
    pub fn title(&self) -> &'static str {
        match self {
            Self::Worktrees { .. } => "Cleanup Complete",
            Self::Orphaned { .. } => "Cleanup Complete",
            Self::Data { .. } => "Cleanup Complete",
            Self::RemoveProject { .. } => "Project Removed",
        }
    }

    /// Get the message for the result modal.
    pub fn message(&self) -> String {
        use crate::commands::format_bytes_display;

        match self {
            Self::Worktrees {
                worktrees_removed,
                sessions_removed,
                bytes_freed,
                skipped_count,
                error_count,
                ..
            } => {
                let mut parts = Vec::new();

                if *worktrees_removed > 0 || *sessions_removed > 0 {
                    let freed = format_bytes_display(*bytes_freed);
                    parts.push(format!(
                        "Removed {} worktree{} and {} session{}, freed {}.",
                        worktrees_removed,
                        if *worktrees_removed == 1 { "" } else { "s" },
                        sessions_removed,
                        if *sessions_removed == 1 { "" } else { "s" },
                        freed
                    ));
                } else {
                    parts.push("No worktrees or sessions were removed.".to_string());
                }

                if *skipped_count > 0 {
                    parts.push(format!(
                        "{} session{} skipped (active runs or uncommitted changes).",
                        skipped_count,
                        if *skipped_count == 1 {
                            " was"
                        } else {
                            "s were"
                        }
                    ));
                }

                if *error_count > 0 {
                    parts.push(format!(
                        "{} error{} occurred. Check the command output tab for details.",
                        error_count,
                        if *error_count == 1 { "" } else { "s" }
                    ));
                }

                parts.join("\n\n")
            }
            Self::Orphaned {
                sessions_removed,
                bytes_freed,
                error_count,
                ..
            } => {
                let mut parts = Vec::new();

                if *sessions_removed > 0 {
                    let freed = format_bytes_display(*bytes_freed);
                    parts.push(format!(
                        "Removed {} orphaned session{}, freed {}.",
                        sessions_removed,
                        if *sessions_removed == 1 { "" } else { "s" },
                        freed
                    ));
                } else {
                    parts.push("No orphaned sessions were found.".to_string());
                }

                if *error_count > 0 {
                    parts.push(format!(
                        "{} error{} occurred. Check the command output tab for details.",
                        error_count,
                        if *error_count == 1 { "" } else { "s" }
                    ));
                }

                parts.join("\n\n")
            }
            Self::RemoveProject {
                project_name,
                worktrees_removed,
                config_deleted,
                bytes_freed,
                skipped_count,
                error_count,
            } => {
                let mut parts = Vec::new();

                if *config_deleted {
                    let freed = format_bytes_display(*bytes_freed);
                    let mut summary = format!("Project '{}' has been removed.", project_name);
                    if *worktrees_removed > 0 {
                        summary.push_str(&format!(
                            "\n\nRemoved {} worktree{}, freed {}.",
                            worktrees_removed,
                            if *worktrees_removed == 1 { "" } else { "s" },
                            freed
                        ));
                    }
                    parts.push(summary);
                } else {
                    parts.push(format!(
                        "Failed to fully remove project '{}'.",
                        project_name
                    ));
                }

                if *skipped_count > 0 {
                    parts.push(format!(
                        "{} worktree{} skipped (active runs).",
                        skipped_count,
                        if *skipped_count == 1 {
                            " was"
                        } else {
                            "s were"
                        }
                    ));
                }

                if *error_count > 0 {
                    parts.push(format!(
                        "{} error{} occurred. Check the command output tab for details.",
                        error_count,
                        if *error_count == 1 { "" } else { "s" }
                    ));
                }

                parts.join("\n\n")
            }
            Self::Data {
                specs_removed,
                runs_removed,
                bytes_freed,
                error_count,
                ..
            } => {
                let mut parts = Vec::new();

                if *specs_removed > 0 || *runs_removed > 0 {
                    let freed = format_bytes_display(*bytes_freed);
                    let mut items = Vec::new();
                    if *specs_removed > 0 {
                        items.push(format!(
                            "{} spec{}",
                            specs_removed,
                            if *specs_removed == 1 { "" } else { "s" }
                        ));
                    }
                    if *runs_removed > 0 {
                        items.push(format!(
                            "{} archived run{}",
                            runs_removed,
                            if *runs_removed == 1 { "" } else { "s" }
                        ));
                    }
                    parts.push(format!("Removed {}, freed {}.", items.join(" and "), freed));
                } else {
                    parts.push("No data was removed.".to_string());
                }

                if *error_count > 0 {
                    parts.push(format!(
                        "{} error{} occurred. Check the command output tab for details.",
                        error_count,
                        if *error_count == 1 { "" } else { "s" }
                    ));
                }

                parts.join("\n\n")
            }
        }
    }

    /// Returns true if the operation had errors.
    pub fn has_errors(&self) -> bool {
        match self {
            Self::Worktrees { error_count, .. }
            | Self::Orphaned { error_count, .. }
            | Self::RemoveProject { error_count, .. }
            | Self::Data { error_count, .. } => *error_count > 0,
        }
    }
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
    /// The Config tab (permanent).
    Config,
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
    /// View of configuration settings.
    Config,
}

impl Tab {
    /// Returns the display label for this tab.
    pub fn label(self) -> &'static str {
        match self {
            Tab::ActiveRuns => "Active Runs",
            Tab::Projects => "Projects",
            Tab::Config => "Config",
        }
    }

    /// Returns all available tabs.
    pub fn all() -> &'static [Tab] {
        &[Tab::ActiveRuns, Tab::Projects, Tab::Config]
    }

    /// Convert to TabId.
    pub fn to_tab_id(self) -> TabId {
        match self {
            Tab::ActiveRuns => TabId::ActiveRuns,
            Tab::Projects => TabId::Projects,
            Tab::Config => TabId::Config,
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

// ============================================================================
// Story Progress Types (US-002: Story Progress Timeline)
// ============================================================================

/// Status of a story in the story progress timeline.
#[derive(Debug, Clone, Copy, PartialEq)]
enum StoryStatus {
    /// Story has been completed successfully.
    Completed,
    /// Story is currently being worked on.
    Active,
    /// Story is pending (not yet started).
    Pending,
    /// Story failed during implementation.
    Failed,
}

impl StoryStatus {
    /// Returns the color for this story status.
    fn color(self) -> Color32 {
        match self {
            StoryStatus::Completed => colors::STATUS_SUCCESS,
            StoryStatus::Active => colors::STATUS_RUNNING,
            StoryStatus::Pending => colors::TEXT_MUTED,
            StoryStatus::Failed => colors::STATUS_ERROR,
        }
    }

    /// Returns the background color for this story status.
    fn background(self) -> Color32 {
        match self {
            StoryStatus::Completed => colors::STATUS_SUCCESS_BG,
            StoryStatus::Active => colors::STATUS_RUNNING_BG,
            StoryStatus::Pending => colors::SURFACE_HOVER,
            StoryStatus::Failed => colors::STATUS_ERROR_BG,
        }
    }

    /// Returns the status indicator text.
    fn indicator(self) -> &'static str {
        match self {
            StoryStatus::Completed => "\u{2713}", // ✓ checkmark
            StoryStatus::Active => "\u{25CF}",    // ● filled circle
            StoryStatus::Pending => "\u{25CB}",   // ○ empty circle
            StoryStatus::Failed => "\u{2717}",    // ✗ X mark
        }
    }
}

/// A story item for display in the story progress timeline.
#[derive(Debug, Clone)]
struct StoryItem {
    /// Story ID (e.g., "US-001").
    id: String,
    /// Story title.
    title: String,
    /// Current status of the story.
    status: StoryStatus,
}

/// Load story items from a session's cached user stories and run state.
///
/// Returns a list of story items ordered by status: Active first, then Completed,
/// then Failed, then Pending. The current story is marked as Active, completed
/// stories are marked based on the spec's `passes` field, and the rest are Pending.
///
/// This function uses cached user stories from `SessionData` to avoid file I/O
/// on every render frame. The cache is populated during `load_ui_data()`.
fn load_story_items(session: &SessionData) -> Vec<StoryItem> {
    let Some(ref run) = session.run else {
        return Vec::new();
    };

    // Use cached user stories instead of loading from disk
    let Some(ref user_stories) = session.cached_user_stories else {
        return Vec::new();
    };

    let current_story_id = run.current_story.as_deref();

    // Build set of failed story IDs from iterations
    let failed_stories: std::collections::HashSet<&str> = run
        .iterations
        .iter()
        .filter(|iter| iter.status == IterationStatus::Failed)
        .map(|iter| iter.story_id.as_str())
        .collect();

    // Build story items from cached user stories
    let mut items: Vec<StoryItem> = user_stories
        .iter()
        .map(|story| {
            let status = if Some(story.id.as_str()) == current_story_id {
                StoryStatus::Active
            } else if story.passes {
                StoryStatus::Completed
            } else if failed_stories.contains(story.id.as_str()) {
                StoryStatus::Failed
            } else {
                StoryStatus::Pending
            };

            StoryItem {
                id: story.id.clone(),
                title: story.title.clone(),
                status,
            }
        })
        .collect();

    // Sort: Active first, then by original priority (which is the order in spec)
    // Actually, acceptance criteria says "most recent/current at the top"
    // so we put active first, then completed (most recently), then pending
    items.sort_by(|a, b| {
        let order = |s: &StoryItem| match s.status {
            StoryStatus::Active => 0,
            StoryStatus::Completed => 1,
            StoryStatus::Failed => 2,
            StoryStatus::Pending => 3,
        };
        order(a).cmp(&order(b))
    });

    items
}

/// A work summary item for display in the work summaries section.
#[derive(Debug, Clone)]
struct WorkSummaryItem {
    /// Iteration number (1-based).
    iteration: u32,
    /// Story ID this iteration worked on.
    story_id: String,
    /// The work summary text.
    summary: String,
}

/// Load work summary items from a session's run state.
///
/// Returns a list of work summary items ordered by most recent first.
/// Only includes iterations that have a work_summary.
fn load_work_summaries(session: &SessionData) -> Vec<WorkSummaryItem> {
    let Some(ref run) = session.run else {
        return Vec::new();
    };

    // Filter to iterations with work summaries and map to WorkSummaryItem
    let mut items: Vec<WorkSummaryItem> = run
        .iterations
        .iter()
        .filter_map(|iter| {
            iter.work_summary.as_ref().map(|summary| WorkSummaryItem {
                iteration: iter.number,
                story_id: iter.story_id.clone(),
                summary: summary.clone(),
            })
        })
        .collect();

    // Sort by iteration number descending (most recent first)
    items.sort_by(|a, b| b.iteration.cmp(&a.iteration));

    items
}

/// Format a number with thousands separators (e.g., 1234567 -> "1,234,567").
fn format_number_with_commas(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

/// Token usage entry for a single phase.
#[derive(Debug, Clone)]
struct PhaseUsageEntry {
    /// Phase name (e.g., "US-001", "Planning", "Final Review").
    phase: String,
    /// Usage data for this phase.
    usage: ClaudeUsage,
}

/// Token usage data for display.
#[derive(Debug, Clone)]
struct TokenUsageData {
    /// Total usage across all phases.
    total: ClaudeUsage,
    /// Per-phase breakdown.
    phases: Vec<PhaseUsageEntry>,
}

/// Load token usage data from a session's run state.
///
/// Returns Some(TokenUsageData) if the session has usage data, None otherwise.
fn load_token_usage(session: &SessionData) -> Option<TokenUsageData> {
    let run = session.run.as_ref()?;
    let total = run.total_usage.clone()?;

    // Only return data if there are actual tokens used
    if total.total_tokens() == 0 {
        return None;
    }

    // Collect phase usage entries
    let mut phases: Vec<PhaseUsageEntry> = run
        .phase_usage
        .iter()
        .map(|(phase, usage)| PhaseUsageEntry {
            phase: phase.clone(),
            usage: usage.clone(),
        })
        .collect();

    // Sort phases in a logical order: Planning first, then US-* in order, then others
    phases.sort_by(|a, b| {
        let order = |p: &str| -> (u8, u32) {
            if p == "Planning" {
                (0, 0)
            } else if let Some(num) = p.strip_prefix("US-") {
                if let Ok(n) = num.parse::<u32>() {
                    (1, n)
                } else {
                    (2, 0)
                }
            } else if p == "Final Review" {
                (3, 0)
            } else if p == "PR & Commit" {
                (4, 0)
            } else {
                (5, 0)
            }
        };
        order(&a.phase).cmp(&order(&b.phase))
    });

    Some(TokenUsageData { total, phases })
}

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
    /// The previously active tab ID (for returning after closing a tab).
    /// Cleared if the previous tab itself is closed.
    previous_tab_id: Option<TabId>,

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
    // Active Runs Tab State (Tab Bar - US-001, US-003)
    // ========================================================================
    /// Session ID of the currently selected session tab in the Active Runs view.
    /// Using ID instead of index is more robust when sessions change order.
    /// None means no session is selected (will auto-select first if available).
    selected_session_id: Option<String>,

    /// Session IDs that have open tabs in the Active Runs view (US-003).
    /// Tracks which sessions are currently visible as tabs.
    /// Tabs auto-open for new sessions unless manually closed.
    open_session_tabs: std::collections::HashSet<String>,

    /// Session IDs that have been manually closed (US-003).
    /// These sessions won't auto-reopen when refreshed.
    /// Cleared when a session completes and is no longer active.
    closed_session_tabs: std::collections::HashSet<String>,

    // ========================================================================
    // Config Tab State
    // ========================================================================
    /// State for the Config tab (scope selection, cached configs, etc.).
    /// Public for test access.
    pub config_state: ConfigTabState,

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

    // ========================================================================
    // Confirmation Dialog State (US-004)
    // ========================================================================
    /// Pending clean operation awaiting user confirmation.
    /// When Some, a confirmation dialog is displayed.
    pending_clean_confirmation: Option<PendingCleanOperation>,

    // ========================================================================
    // Result Modal State (US-007)
    // ========================================================================
    /// Cleanup result to display in a modal after operation completes.
    /// When Some, a result modal is displayed with the cleanup summary.
    pending_result_modal: Option<CleanupResult>,

    // ========================================================================
    // Detail Panel Section State (Active Runs Detail Panel - US-005)
    // ========================================================================
    /// Collapsed state for collapsible sections in the detail panel.
    /// Maps section ID to collapsed state (true = collapsed, false = expanded).
    /// State persists during the session but not across restarts.
    section_collapsed_state: std::collections::HashMap<String, bool>,
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
            TabInfo::permanent(TabId::Config, "Config"),
        ];

        // Create channel for command execution messages
        let (command_tx, command_rx) = std::sync::mpsc::channel();

        let mut app = Self {
            current_tab: Tab::default(),
            tabs,
            active_tab_id: TabId::default(),
            previous_tab_id: None,
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
            selected_session_id: None,
            open_session_tabs: std::collections::HashSet::new(),
            closed_session_tabs: std::collections::HashSet::new(),
            config_state: ConfigTabState::new(),
            context_menu: None,
            command_executions: std::collections::HashMap::new(),
            command_rx,
            command_tx,
            pending_clean_confirmation: None,
            pending_result_modal: None,
            section_collapsed_state: std::collections::HashMap::new(),
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
    // Config Tab State (delegating to ConfigTabState)
    // ========================================================================

    /// Returns the currently selected config scope.
    pub fn selected_config_scope(&self) -> &ConfigScope {
        self.config_state.selected_scope()
    }

    /// Sets the selected config scope.
    pub fn set_selected_config_scope(&mut self, scope: ConfigScope) {
        self.config_state.set_selected_scope(scope);
    }

    /// Returns the cached list of project names for config scope selection.
    pub fn config_scope_projects(&self) -> &[String] {
        self.config_state.scope_projects()
    }

    /// Returns whether a project has its own config file.
    pub fn project_has_config(&self, project_name: &str) -> bool {
        self.config_state.project_has_config(project_name)
    }

    /// Refresh the config scope data (project list and config file status).
    fn refresh_config_scope_data(&mut self) {
        self.config_state.refresh_scope_data();
    }

    /// Returns the cached global config, if loaded.
    pub fn cached_global_config(&self) -> Option<&crate::config::Config> {
        self.config_state.cached_global_config()
    }

    /// Returns the global config error, if any.
    pub fn global_config_error(&self) -> Option<&str> {
        self.config_state.global_config_error()
    }

    /// Returns the cached project config for a specific project, if loaded.
    pub fn cached_project_config(&self, project_name: &str) -> Option<&crate::config::Config> {
        self.config_state.cached_project_config(project_name)
    }

    /// Returns the project config error, if any.
    pub fn project_config_error(&self) -> Option<&str> {
        self.config_state.project_config_error()
    }

    /// Create a project config file from the current global configuration.
    fn create_project_config_from_global(
        &mut self,
        project_name: &str,
    ) -> std::result::Result<(), String> {
        self.config_state
            .create_project_config_from_global(project_name)
    }

    /// Apply boolean field changes to the config and save immediately (US-006).
    fn apply_config_bool_changes(
        &mut self,
        is_global: bool,
        project_name: Option<&str>,
        changes: &[(ConfigBoolField, bool)],
    ) {
        self.config_state
            .apply_bool_changes(is_global, project_name, changes);
    }

    /// Apply text field changes to the config (US-007).
    fn apply_config_text_changes(
        &mut self,
        is_global: bool,
        project_name: Option<&str>,
        changes: &[(ConfigTextField, String)],
    ) {
        self.config_state
            .apply_text_changes(is_global, project_name, changes);
    }

    /// Reset config to application defaults (US-009).
    fn reset_config_to_defaults(&mut self, is_global: bool, project_name: Option<&str>) {
        self.config_state.reset_to_defaults(is_global, project_name);
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
            .filter_map(|s| {
                // machine_state is required for ResumableSessionInfo, and is_resumable_session
                // already ensures it exists and is not Idle/Completed
                let machine_state = s.machine_state?;
                Some(ResumableSessionInfo::new(
                    s.metadata.session_id,
                    s.metadata.branch_name,
                    s.metadata.worktree_path,
                    machine_state,
                ))
            })
            .collect()
    }

    /// Get a specific resumable session by ID.
    ///
    /// US-005: Used to look up session details when user clicks a resume option.
    fn get_resumable_session_by_id(
        &self,
        project_name: &str,
        session_id: &str,
    ) -> Option<ResumableSessionInfo> {
        self.get_resumable_sessions(project_name)
            .into_iter()
            .find(|s| s.session_id == session_id)
    }

    /// Get cleanable session information for a project.
    ///
    /// US-006: Updated to count any worktrees that can be cleaned, not just completed sessions.
    /// US-002: Now also counts cleanable specs and runs.
    ///
    /// Returns counts for:
    /// - cleanable_worktrees: non-main sessions with existing worktrees and no active runs
    /// - orphaned_sessions: sessions where the worktree was deleted but state remains
    /// - cleanable_specs: spec files (pairs counted as 1) not used by active sessions
    /// - cleanable_runs: archived run files in runs/ directory
    ///
    /// Safety: Sessions with active runs (is_running=true) are NOT counted as cleanable.
    /// Specs used by active sessions are excluded from the count.
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

        // US-002: Collect spec paths used by active (running) sessions
        let mut active_spec_paths: std::collections::HashSet<std::path::PathBuf> =
            std::collections::HashSet::new();

        for session in &sessions {
            // Skip main session - it's not a worktree created by autom8
            if session.metadata.session_id == "main" {
                // Still need to check if main session has active run with spec
                if session.metadata.is_running {
                    if let Some(session_sm) = sm.get_session(&session.metadata.session_id) {
                        if let Ok(Some(state)) = session_sm.load_current() {
                            active_spec_paths.insert(state.spec_json_path.clone());
                            if let Some(md_path) = &state.spec_md_path {
                                active_spec_paths.insert(md_path.clone());
                            }
                        }
                    }
                }
                continue;
            }

            if session.is_stale {
                // Orphaned session: worktree was deleted
                info.orphaned_sessions += 1;
            } else if !session.metadata.is_running {
                // US-006: Count any worktree that exists and doesn't have an active run
                // The actual clean operation will also skip active runs
                info.cleanable_worktrees += 1;
            } else {
                // US-002: Session is running - collect its spec paths to exclude
                if let Some(session_sm) = sm.get_session(&session.metadata.session_id) {
                    if let Ok(Some(state)) = session_sm.load_current() {
                        active_spec_paths.insert(state.spec_json_path.clone());
                        if let Some(md_path) = &state.spec_md_path {
                            active_spec_paths.insert(md_path.clone());
                        }
                    }
                }
            }
        }

        // US-002: Count cleanable specs (pairs counted as 1)
        info.cleanable_specs = count_cleanable_specs(&sm.spec_dir(), &active_spec_paths);

        // US-002: Count cleanable runs
        info.cleanable_runs = count_cleanable_runs(&sm.runs_dir());

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
            // Nothing to clean - disabled menu item with tooltip hint (US-006)
            ContextMenuItem::submenu_disabled("Clean", "clean", "Nothing to clean")
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

            // US-003: Add Data option when specs or runs exist
            let data_count = cleanable_info.cleanable_specs + cleanable_info.cleanable_runs;
            if data_count > 0 {
                let label = format!("Data ({})", data_count);
                submenu_items.push(ContextMenuItem::action(label, ContextMenuAction::CleanData));
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
            ContextMenuItem::Separator,
            ContextMenuItem::action("Remove Project", ContextMenuAction::RemoveProject),
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
    /// Tracks the previous tab for returning after closing the new tab.
    pub fn set_active_tab(&mut self, tab_id: TabId) {
        // Store current tab as previous before switching (if different)
        if self.active_tab_id != tab_id {
            self.previous_tab_id = Some(self.active_tab_id.clone());
        }

        // Update legacy field for backward compatibility
        match &tab_id {
            TabId::ActiveRuns => self.current_tab = Tab::ActiveRuns,
            TabId::Projects => self.current_tab = Tab::Projects,
            TabId::Config => self.current_tab = Tab::Config,
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

    /// Load status for a project by calling the data layer directly.
    /// Opens a new command output tab and populates it with session data.
    ///
    /// US-002: Replaces subprocess spawning with direct StateManager calls.
    pub fn spawn_status_command(&mut self, project_name: &str) {
        // Open the tab first to get the cache key
        let id = self.open_command_output_tab(project_name, "status");
        let cache_key = id.cache_key();
        let tx = self.command_tx.clone();
        let project = project_name.to_string();

        std::thread::spawn(move || {
            // Call data layer directly instead of spawning subprocess
            match StateManager::for_project(&project) {
                Ok(state_manager) => {
                    match state_manager.list_sessions_with_status() {
                        Ok(sessions) => {
                            // Format session data as plain text
                            let lines = format_sessions_as_text(&sessions);
                            for line in lines {
                                let _ = tx.send(CommandMessage::Stdout {
                                    cache_key: cache_key.clone(),
                                    line,
                                });
                            }
                            let _ = tx.send(CommandMessage::Completed {
                                cache_key,
                                exit_code: 0,
                            });
                        }
                        Err(e) => {
                            let _ = tx.send(CommandMessage::Failed {
                                cache_key,
                                error: format!("Failed to list sessions: {}", e),
                            });
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(CommandMessage::Failed {
                        cache_key,
                        error: format!("Failed to load project: {}", e),
                    });
                }
            }
        });
    }

    /// Get project description and display in a command output tab.
    ///
    /// US-003: Calls data layer directly instead of spawning subprocess.
    /// Opens a new command output tab and formats ProjectDescription as plain text.
    pub fn spawn_describe_command(&mut self, project_name: &str) {
        // Open the tab first to get the cache key
        let id = self.open_command_output_tab(project_name, "describe");
        let cache_key = id.cache_key();
        let tx = self.command_tx.clone();
        let project = project_name.to_string();

        std::thread::spawn(move || {
            // Call data layer directly instead of spawning subprocess
            match crate::config::get_project_description(&project) {
                Ok(Some(desc)) => {
                    // Format project description as plain text
                    let lines = format_project_description_as_text(&desc);
                    for line in lines {
                        let _ = tx.send(CommandMessage::Stdout {
                            cache_key: cache_key.clone(),
                            line,
                        });
                    }
                    let _ = tx.send(CommandMessage::Completed {
                        cache_key,
                        exit_code: 0,
                    });
                }
                Ok(None) => {
                    let _ = tx.send(CommandMessage::Stdout {
                        cache_key: cache_key.clone(),
                        line: format!("Project '{}' not found.", project),
                    });
                    let _ = tx.send(CommandMessage::Completed {
                        cache_key,
                        exit_code: 1,
                    });
                }
                Err(e) => {
                    let _ = tx.send(CommandMessage::Failed {
                        cache_key,
                        error: format!("Failed to get project description: {}", e),
                    });
                }
            }
        });
    }

    /// Show resume session information in the output tab.
    ///
    /// US-005: Shows session info instead of spawning subprocess.
    /// Info includes: session ID, branch, worktree path, current state.
    /// Shows message with instructions on how to resume in terminal.
    pub fn show_resume_info(&mut self, project_name: &str, session_id: &str) {
        // Open the tab first to get the cache key
        let id = self.open_command_output_tab(project_name, "resume");
        let cache_key = id.cache_key();
        let tx = self.command_tx.clone();

        // Look up the session info
        match self.get_resumable_session_by_id(project_name, session_id) {
            Some(session) => {
                // Format session info as plain text
                let lines = format_resume_info_as_text(&session);
                for line in lines {
                    let _ = tx.send(CommandMessage::Stdout {
                        cache_key: cache_key.clone(),
                        line,
                    });
                }
                let _ = tx.send(CommandMessage::Completed {
                    cache_key,
                    exit_code: 0,
                });
            }
            None => {
                let _ = tx.send(CommandMessage::Stdout {
                    cache_key: cache_key.clone(),
                    line: format!("Session '{}' not found or no longer resumable.", session_id),
                });
                let _ = tx.send(CommandMessage::Completed {
                    cache_key,
                    exit_code: 1,
                });
            }
        }
    }

    /// Clean completed/failed sessions with worktrees by calling the data layer directly.
    /// Opens a new command output tab and populates it with cleanup results.
    ///
    /// US-004: Replaces subprocess spawning with direct clean_worktrees_direct() call.
    /// Note: The clean operation respects safety filters - only Completed/Failed/Interrupted
    /// sessions are cleaned, not Running/InProgress ones.
    pub fn spawn_clean_worktrees_command(&mut self, project_name: &str) {
        // Open the tab first to get the cache key
        let id = self.open_command_output_tab(project_name, "clean-worktrees");
        let cache_key = id.cache_key();
        let tx = self.command_tx.clone();
        let project = project_name.to_string();

        std::thread::spawn(move || {
            use crate::commands::{clean_worktrees_direct, DirectCleanOptions};

            // Call data layer directly instead of spawning subprocess
            let options = DirectCleanOptions {
                worktrees: true,
                force: false,
            };

            match clean_worktrees_direct(&project, options) {
                Ok(summary) => {
                    // Format cleanup summary as plain text
                    let lines = format_cleanup_summary_as_text(&summary, "Clean Worktrees");
                    for line in lines {
                        let _ = tx.send(CommandMessage::Stdout {
                            cache_key: cache_key.clone(),
                            line,
                        });
                    }
                    let exit_code = if summary.errors.is_empty() { 0 } else { 1 };
                    let _ = tx.send(CommandMessage::Completed {
                        cache_key,
                        exit_code,
                    });

                    // US-007: Send cleanup result for modal display
                    let _ = tx.send(CommandMessage::CleanupCompleted {
                        result: CleanupResult::Worktrees {
                            project_name: project,
                            worktrees_removed: summary.worktrees_removed,
                            sessions_removed: summary.sessions_removed,
                            bytes_freed: summary.bytes_freed,
                            skipped_count: summary.sessions_skipped.len(),
                            error_count: summary.errors.len(),
                        },
                    });
                }
                Err(e) => {
                    let _ = tx.send(CommandMessage::Failed {
                        cache_key,
                        error: format!("Failed to clean sessions: {}", e),
                    });
                }
            }
        });
    }

    /// Clean orphaned sessions by calling the data layer directly.
    /// Orphaned sessions are those where the worktree has been deleted but the
    /// session state remains.
    /// Opens a new command output tab and populates it with cleanup results.
    ///
    /// US-004: Replaces subprocess spawning with direct clean_orphaned_direct() call.
    pub fn spawn_clean_orphaned_command(&mut self, project_name: &str) {
        // Open the tab first to get the cache key
        let id = self.open_command_output_tab(project_name, "clean-orphaned");
        let cache_key = id.cache_key();
        let tx = self.command_tx.clone();
        let project = project_name.to_string();

        std::thread::spawn(move || {
            use crate::commands::clean_orphaned_direct;

            // Call data layer directly instead of spawning subprocess
            match clean_orphaned_direct(&project) {
                Ok(summary) => {
                    // Format cleanup summary as plain text
                    let lines = format_cleanup_summary_as_text(&summary, "Clean Orphaned");
                    for line in lines {
                        let _ = tx.send(CommandMessage::Stdout {
                            cache_key: cache_key.clone(),
                            line,
                        });
                    }
                    let exit_code = if summary.errors.is_empty() { 0 } else { 1 };
                    let _ = tx.send(CommandMessage::Completed {
                        cache_key,
                        exit_code,
                    });

                    // US-007: Send cleanup result for modal display
                    let _ = tx.send(CommandMessage::CleanupCompleted {
                        result: CleanupResult::Orphaned {
                            project_name: project,
                            sessions_removed: summary.sessions_removed,
                            bytes_freed: summary.bytes_freed,
                            error_count: summary.errors.len(),
                        },
                    });
                }
                Err(e) => {
                    let _ = tx.send(CommandMessage::Failed {
                        cache_key,
                        error: format!("Failed to clean orphaned sessions: {}", e),
                    });
                }
            }
        });
    }

    /// Clean data (specs and archived runs) for a project by calling the data layer directly.
    /// Opens a new command output tab and populates it with cleanup results.
    ///
    /// US-003: Implements the clean data action for specs and archived runs.
    pub fn spawn_clean_data_command(&mut self, project_name: &str) {
        // Open the tab first to get the cache key
        let id = self.open_command_output_tab(project_name, "clean-data");
        let cache_key = id.cache_key();
        let tx = self.command_tx.clone();
        let project = project_name.to_string();

        std::thread::spawn(move || {
            use crate::commands::clean_data_direct;

            // Call data layer directly instead of spawning subprocess
            match clean_data_direct(&project) {
                Ok(summary) => {
                    // Format cleanup summary as plain text
                    let lines = format_data_cleanup_summary_as_text(&summary);
                    for line in lines {
                        let _ = tx.send(CommandMessage::Stdout {
                            cache_key: cache_key.clone(),
                            line,
                        });
                    }
                    let exit_code = if summary.errors.is_empty() { 0 } else { 1 };
                    let _ = tx.send(CommandMessage::Completed {
                        cache_key,
                        exit_code,
                    });

                    // US-007: Send cleanup result for modal display
                    let _ = tx.send(CommandMessage::CleanupCompleted {
                        result: CleanupResult::Data {
                            project_name: project,
                            specs_removed: summary.specs_removed,
                            runs_removed: summary.runs_removed,
                            bytes_freed: summary.bytes_freed,
                            error_count: summary.errors.len(),
                        },
                    });
                }
                Err(e) => {
                    let _ = tx.send(CommandMessage::Failed {
                        cache_key,
                        error: format!("Failed to clean data: {}", e),
                    });
                }
            }
        });
    }

    /// Remove a project from autom8 entirely by calling the data layer directly.
    /// This removes all worktrees (except active runs), session state, specs, and project configuration.
    /// Opens a new command output tab and populates it with removal results.
    ///
    /// US-004: Implements the actual removal logic using remove_project_direct().
    pub fn spawn_remove_project_command(&mut self, project_name: &str) {
        // Open the tab first to get the cache key
        let id = self.open_command_output_tab(project_name, "remove-project");
        let cache_key = id.cache_key();
        let tx = self.command_tx.clone();
        let project = project_name.to_string();

        std::thread::spawn(move || {
            use crate::commands::remove_project_direct;

            match remove_project_direct(&project) {
                Ok(summary) => {
                    // Format removal summary as plain text
                    let lines = format_removal_summary_as_text(&summary, &project);
                    for line in lines {
                        let _ = tx.send(CommandMessage::Stdout {
                            cache_key: cache_key.clone(),
                            line,
                        });
                    }
                    let exit_code = if summary.errors.is_empty() { 0 } else { 1 };
                    let _ = tx.send(CommandMessage::Completed {
                        cache_key: cache_key.clone(),
                        exit_code,
                    });

                    // US-005: Remove project from sidebar after successful removal.
                    // Only remove if config was deleted (project fully removed).
                    // If removal fails entirely, keep project in sidebar.
                    if summary.config_deleted {
                        let _ = tx.send(CommandMessage::ProjectRemoved {
                            project_name: project.clone(),
                        });
                    }

                    // US-007: Send cleanup result for modal display
                    let _ = tx.send(CommandMessage::CleanupCompleted {
                        result: CleanupResult::RemoveProject {
                            project_name: project,
                            worktrees_removed: summary.worktrees_removed,
                            config_deleted: summary.config_deleted,
                            bytes_freed: summary.bytes_freed,
                            skipped_count: summary.worktrees_skipped.len(),
                            error_count: summary.errors.len(),
                        },
                    });
                }
                Err(e) => {
                    let _ = tx.send(CommandMessage::Failed {
                        cache_key,
                        error: format!("Failed to remove project: {}", e),
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
                CommandMessage::ProjectRemoved { project_name } => {
                    // US-005: Remove project from sidebar after successful removal.
                    self.remove_project_from_sidebar(&project_name);
                }
                CommandMessage::CleanupCompleted { result } => {
                    // US-007: Show result modal after cleanup operation completes.
                    self.pending_result_modal = Some(result);
                    // US-005: Refresh data immediately after cleanup to update UI
                    // (e.g., cleanable counts in menu).
                    self.refresh_data();
                }
            }
        }
    }

    /// Remove a project from the sidebar projects list.
    ///
    /// US-005: Called after a project is successfully removed via remove_project_direct().
    /// Removes the project from the in-memory list so it disappears from the sidebar.
    fn remove_project_from_sidebar(&mut self, project_name: &str) {
        self.projects.retain(|p| p.info.name != project_name);
    }

    /// Close a tab by ID.
    /// Returns true if the tab was closed, false if the tab doesn't exist or is not closable.
    /// If the closed tab was active, switches to the previous tab (if available and still open),
    /// otherwise falls back to adjacent tab logic or Projects tab.
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

        // Clear previous_tab_id if the previous tab itself is being closed
        if self.previous_tab_id.as_ref() == Some(tab_id) {
            self.previous_tab_id = None;
        }

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
            // Try to switch to the previous tab (if it exists and is still open)
            if let Some(prev_id) = self.previous_tab_id.take() {
                if self.has_tab(&prev_id) {
                    self.set_active_tab(prev_id);
                    return true;
                }
            }

            // Fall back to adjacent tab logic
            if tab_index > 0 && tab_index <= self.tabs.len() {
                self.set_active_tab(self.tabs[tab_index - 1].id.clone());
            } else if !self.tabs.is_empty() {
                // Fall back to Projects tab
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

        // US-003: Sync open_session_tabs with current sessions
        self.sync_session_tabs();
    }

    /// Sync session tabs with current session list (US-003).
    ///
    /// This method:
    /// 1. Auto-opens tabs for new sessions (unless manually closed)
    /// 2. Removes tabs for sessions that no longer exist
    /// 3. Clears closed_session_tabs for sessions that are gone (so they can reopen later)
    fn sync_session_tabs(&mut self) {
        // Get current session IDs
        let current_session_ids: std::collections::HashSet<String> = self
            .sessions
            .iter()
            .map(|s| s.metadata.session_id.clone())
            .collect();

        // Auto-open tabs for new sessions (if not manually closed)
        for session_id in &current_session_ids {
            if !self.closed_session_tabs.contains(session_id) {
                self.open_session_tabs.insert(session_id.clone());
            }
        }

        // Remove tabs for sessions that no longer exist
        self.open_session_tabs
            .retain(|id| current_session_ids.contains(id));

        // Clear closed_session_tabs for sessions that are gone
        // This allows the tab to auto-open if the session appears again later
        self.closed_session_tabs
            .retain(|id| current_session_ids.contains(id));
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

        // Render confirmation dialog if pending (must be after context menu to appear on top)
        self.render_confirmation_dialog(ctx);

        // Render result modal if cleanup operation completed (US-007)
        self.render_result_modal(ctx);
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

        // Calculate menu dimensions (US-001: Dynamic width based on content)
        let menu_width = calculate_context_menu_width(ctx, &menu_state.items);
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
                        ui.set_max_width(menu_width);

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
                                    hint,
                                } => {
                                    // Render submenu trigger with arrow indicator
                                    let response =
                                        self.render_context_menu_item(ui, label, *enabled, true);
                                    if response.hovered && *enabled && !items.is_empty() {
                                        // Track this submenu as hovered for rendering
                                        hovered_submenu =
                                            Some((id.clone(), items.clone(), response.rect));
                                    }
                                    // US-006: Show tooltip when hovering a disabled submenu with a hint
                                    if response.hovered_raw && !*enabled {
                                        if let Some(hint_text) = hint {
                                            egui::show_tooltip_at_pointer(
                                                ui.ctx(),
                                                ui.layer_id(),
                                                egui::Id::new("submenu_hint").with(id),
                                                |ui| {
                                                    ui.label(hint_text);
                                                },
                                            );
                                        }
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
                // Calculate submenu dimensions (US-001: Dynamic width for submenus too)
                let submenu_width = calculate_context_menu_width(ctx, &submenu_items);
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
                if submenu_pos.x + submenu_width > screen_rect.max.x - spacing::SM {
                    // Position to the left of the main menu instead
                    submenu_pos.x = menu_pos.x - submenu_width - CONTEXT_MENU_SUBMENU_GAP;
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
                    Vec2::new(submenu_width, submenu_height),
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
                                ui.set_min_width(submenu_width);
                                ui.set_max_width(submenu_width);

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
                                                Vec2::new(submenu_width, 1.0),
                                            );
                                            ui.painter().rect_filled(
                                                separator_rect,
                                                Rounding::ZERO,
                                                colors::SEPARATOR,
                                            );
                                            ui.allocate_space(Vec2::new(submenu_width, 1.0));
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
                    // US-005: Show session info in output tab instead of spawning subprocess
                    if let Some(id) = session_id {
                        self.show_resume_info(&project_name, &id);
                    }
                    // If session_id is None, the menu item should have been disabled,
                    // so this case shouldn't happen in normal operation
                }
                ContextMenuAction::CleanWorktrees => {
                    // US-004: Show confirmation dialog before executing clean operation
                    self.pending_clean_confirmation = Some(PendingCleanOperation::Worktrees {
                        project_name: project_name.clone(),
                    });
                }
                ContextMenuAction::CleanOrphaned => {
                    // US-004: Show confirmation dialog before executing clean operation
                    self.pending_clean_confirmation = Some(PendingCleanOperation::Orphaned {
                        project_name: project_name.clone(),
                    });
                }
                ContextMenuAction::CleanData => {
                    // US-003: Show confirmation dialog before cleaning data
                    let cleanable_info = self.get_cleanable_info(&project_name);
                    self.pending_clean_confirmation = Some(PendingCleanOperation::Data {
                        project_name: project_name.clone(),
                        specs_count: cleanable_info.cleanable_specs,
                        runs_count: cleanable_info.cleanable_runs,
                    });
                }
                ContextMenuAction::RemoveProject => {
                    // US-002: Show confirmation dialog before removing project (modal implemented in US-003)
                    self.pending_clean_confirmation = Some(PendingCleanOperation::RemoveProject {
                        project_name: project_name.clone(),
                    });
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
            hovered_raw: response.hovered(),
            rect: screen_item_rect,
        }
    }

    // ========================================================================
    // Confirmation Dialog (US-004)
    // ========================================================================

    /// Render the confirmation dialog overlay for clean operations.
    ///
    /// This method renders a modal dialog when `pending_clean_confirmation` is Some.
    /// Uses the reusable Modal component with a semi-transparent backdrop and
    /// Cancel/Confirm buttons.
    fn render_confirmation_dialog(&mut self, ctx: &egui::Context) {
        // Early return if no confirmation is pending
        let pending = match &self.pending_clean_confirmation {
            Some(op) => op.clone(),
            None => return,
        };

        // Create the modal using the reusable component
        // US-004: Data cleanup uses "Delete" button, others use "Confirm"
        let modal = Modal::new(pending.title())
            .id("clean_confirmation")
            .message(pending.message())
            .cancel_button(ModalButton::secondary("Cancel"))
            .confirm_button(ModalButton::destructive(pending.confirm_button_label()));

        // Show the modal and handle the action
        match modal.show(ctx) {
            ModalAction::Confirmed => {
                // Execute the clean operation
                let project_name = pending.project_name().to_string();
                match pending {
                    PendingCleanOperation::Worktrees { .. } => {
                        self.spawn_clean_worktrees_command(&project_name);
                    }
                    PendingCleanOperation::Orphaned { .. } => {
                        self.spawn_clean_orphaned_command(&project_name);
                    }
                    PendingCleanOperation::Data { .. } => {
                        // US-003: Clean specs and archived runs
                        self.spawn_clean_data_command(&project_name);
                    }
                    PendingCleanOperation::RemoveProject { .. } => {
                        // US-004: Remove project entirely (worktrees + config)
                        self.spawn_remove_project_command(&project_name);
                    }
                }
                self.pending_clean_confirmation = None;
            }
            ModalAction::Cancelled => {
                self.pending_clean_confirmation = None;
            }
            ModalAction::None => {
                // Modal is still open, do nothing
            }
        }
    }

    // ========================================================================
    // Result Modal (US-007)
    // ========================================================================

    /// Render the result modal overlay after cleanup operations.
    ///
    /// US-007: After clean or remove operations complete, show a result summary modal.
    /// This method renders a modal dialog when `pending_result_modal` is Some.
    /// Uses the reusable Modal component with a single "OK" button to dismiss.
    fn render_result_modal(&mut self, ctx: &egui::Context) {
        // Early return if no result is pending
        let result = match &self.pending_result_modal {
            Some(r) => r.clone(),
            None => return,
        };

        // Create the modal using the reusable component
        // US-007: Result modals only have a single "OK" button (no cancel)
        let modal = Modal::new(result.title())
            .id("cleanup_result")
            .message(result.message())
            .no_cancel_button()
            .confirm_button(ModalButton::new("OK"));

        // Show the modal and handle dismissal
        // OK button or backdrop click/Escape all dismiss the modal
        match modal.show(ctx) {
            ModalAction::Confirmed | ModalAction::Cancelled => {
                self.pending_result_modal = None;
            }
            ModalAction::None => {
                // Modal is still open, do nothing
            }
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

            // Snapshot of permanent tabs (ActiveRuns, Projects, and Config)
            let permanent_tabs: Vec<(TabId, &'static str)> = vec![
                (TabId::ActiveRuns, "Active Runs"),
                (TabId::Projects, "Projects"),
                (TabId::Config, "Config"),
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

            // Fill remaining space, leaving room for icon and animation
            // Icon positioned higher in the sidebar for better visual balance
            let animation_height = 150.0;
            let icon_section_height = SIDEBAR_ICON_SIZE + spacing::LG * 2.0; // Icon + generous padding
            ui.add_space(ui.available_height() - animation_height - icon_section_height);

            // Decorative mascot icon (US-005)
            // Centered between the tabs and the animation
            ui.add_space(spacing::LG);
            ui.horizontal(|ui| {
                let sidebar_width = ui.available_width();
                let icon_offset = (sidebar_width - SIDEBAR_ICON_SIZE) / 2.0;
                ui.add_space(icon_offset);
                ui.add(
                    egui::Image::new(egui::include_image!("../../../assets/icon.png"))
                        .fit_to_exact_size(egui::vec2(SIDEBAR_ICON_SIZE, SIDEBAR_ICON_SIZE)),
                );
            });
            ui.add_space(spacing::LG);

            // Decorative animation at the bottom of sidebar
            // Uses full sidebar width, particles rise from bottom
            let sidebar_width = ui.available_width();
            super::animation::render_rising_particles(ui, sidebar_width, animation_height);

            // Schedule next animation frame (handles all animations)
            super::animation::schedule_frame(ui.ctx());
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
            TabId::Config => self.render_config(ui),
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

    /// Render the Config view with split-panel layout.
    ///
    /// Uses the same split-panel pattern as the Projects tab:
    /// - Left panel: Scope selector (Global + projects)
    /// - Right panel: Config editor for the selected scope
    fn render_config(&mut self, ui: &mut egui::Ui) {
        // Refresh config scope data before rendering
        self.refresh_config_scope_data();

        // Track actions that need to be processed after rendering (US-005, US-006)
        let mut editor_actions = ConfigEditorActions::default();

        // Use horizontal layout for split view
        let available_width = ui.available_width();
        let available_height = ui.available_height();

        // Calculate panel widths: 50/50 split with divider in the middle
        // Subtract the divider width and margins from the total width
        let divider_total_width = SPLIT_DIVIDER_WIDTH + SPLIT_DIVIDER_MARGIN * 2.0;
        let panel_width =
            ((available_width - divider_total_width) / 2.0).max(SPLIT_PANEL_MIN_WIDTH);

        ui.horizontal(|ui| {
            // Left panel: Scope selector
            ui.allocate_ui_with_layout(
                Vec2::new(panel_width, available_height),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    self.render_config_left_panel(ui);
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

            // Right panel: Config editor for selected scope
            // Returns actions including create project config (US-005) and bool changes (US-006)
            let actions_response = ui.allocate_ui_with_layout(
                Vec2::new(ui.available_width(), available_height),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| self.render_config_right_panel(ui),
            );

            editor_actions = actions_response.inner;
        });

        // Process the create config action outside of the closure (US-005)
        if let Some(project_name) = editor_actions.create_project_config {
            if let Err(e) = self.create_project_config_from_global(&project_name) {
                self.config_state.project_config_error = Some(e);
            }
        }

        // Process boolean field changes (US-006)
        if !editor_actions.bool_changes.is_empty() {
            self.apply_config_bool_changes(
                editor_actions.is_global,
                editor_actions.project_name.as_deref(),
                &editor_actions.bool_changes,
            );
        }

        // Process text field changes (US-007)
        if !editor_actions.text_changes.is_empty() {
            self.apply_config_text_changes(
                editor_actions.is_global,
                editor_actions.project_name.as_deref(),
                &editor_actions.text_changes,
            );
        }

        // Process reset to defaults action (US-009)
        if editor_actions.reset_to_defaults {
            self.reset_config_to_defaults(
                editor_actions.is_global,
                editor_actions.project_name.as_deref(),
            );
        }
    }

    /// Render the left panel of the Config view (scope selector).
    ///
    /// Shows "Global" at the top, followed by all discovered projects.
    /// Projects without their own config file are shown greyed out with "(global)" suffix.
    fn render_config_left_panel(&mut self, ui: &mut egui::Ui) {
        // Header section
        ui.label(
            egui::RichText::new("Scope")
                .font(typography::font(FontSize::Title, FontWeight::SemiBold))
                .color(colors::TEXT_PRIMARY),
        );

        ui.add_space(spacing::SM);

        // Scrollable scope list
        egui::ScrollArea::vertical()
            .id_salt("config_scope_list")
            .auto_shrink([false, false])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
            .show(ui, |ui| {
                // Global scope item (always first, always has config)
                if self.render_config_scope_item(ui, ConfigScope::Global, true) {
                    self.config_state.selected_scope = ConfigScope::Global;
                }

                ui.add_space(spacing::SM);

                // Project scope items
                let projects: Vec<String> = self.config_state.scope_projects.clone();
                for project in projects {
                    let has_config = self.project_has_config(&project);
                    let scope = ConfigScope::Project(project.clone());
                    if self.render_config_scope_item(ui, scope.clone(), has_config) {
                        self.config_state.selected_scope = scope;
                    }
                    ui.add_space(spacing::XS);
                }
            });
    }

    /// Render a single config scope item in the scope selector.
    ///
    /// Returns true if the item was clicked.
    fn render_config_scope_item(
        &self,
        ui: &mut egui::Ui,
        scope: ConfigScope,
        has_config: bool,
    ) -> bool {
        let is_selected = self.config_state.selected_scope == scope;

        // Determine display text and styling
        let (display_text, text_color) = match &scope {
            ConfigScope::Global => ("Global".to_string(), colors::TEXT_PRIMARY),
            ConfigScope::Project(name) => {
                if has_config {
                    (name.clone(), colors::TEXT_PRIMARY)
                } else {
                    // Projects without config file: greyed out with "(global)" suffix
                    (format!("{} (global)", name), colors::TEXT_MUTED)
                }
            }
        };

        // Allocate space for the row
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), CONFIG_SCOPE_ROW_HEIGHT),
            Sense::click(),
        );

        // Draw background on hover or selection
        if ui.is_rect_visible(rect) {
            let bg_color = if is_selected {
                colors::SURFACE_SELECTED
            } else if response.hovered() {
                colors::SURFACE_HOVER
            } else {
                Color32::TRANSPARENT
            };

            ui.painter()
                .rect_filled(rect, Rounding::same(SIDEBAR_ITEM_ROUNDING), bg_color);

            // Draw selection indicator on the left edge for selected items
            if is_selected {
                let indicator_rect = Rect::from_min_size(
                    rect.min,
                    Vec2::new(SIDEBAR_ACTIVE_INDICATOR_WIDTH, rect.height()),
                );
                ui.painter().rect_filled(
                    indicator_rect,
                    Rounding::same(SIDEBAR_ACTIVE_INDICATOR_WIDTH / 2.0),
                    colors::ACCENT,
                );
            }

            // Draw the scope name with appropriate styling
            let text_rect = rect.shrink2(Vec2::new(
                CONFIG_SCOPE_ROW_PADDING_H
                    + (if is_selected {
                        SIDEBAR_ACTIVE_INDICATOR_WIDTH + 4.0
                    } else {
                        0.0
                    }),
                CONFIG_SCOPE_ROW_PADDING_V,
            ));

            let font_weight = if is_selected {
                FontWeight::SemiBold
            } else {
                FontWeight::Regular
            };

            ui.painter().text(
                text_rect.left_center(),
                egui::Align2::LEFT_CENTER,
                &display_text,
                typography::font(FontSize::Body, font_weight),
                text_color,
            );
        }

        response.clicked()
    }

    /// Render the right panel of the Config view (config editor).
    ///
    /// Shows the config editor for the currently selected scope.
    /// For US-003: Global Config Editor with all 6 fields grouped logically.
    /// For US-005: Returns project name if "Create Project Config" button was clicked.
    /// For US-006: Returns boolean field changes for immediate save.
    ///
    /// # Returns
    ///
    /// `ConfigEditorActions` containing any actions that need to be processed:
    /// - `create_project_config`: Project name if "Create Project Config" was clicked
    /// - `bool_changes`: Vector of (field, new_value) for toggled boolean fields
    fn render_config_right_panel(&self, ui: &mut egui::Ui) -> ConfigEditorActions {
        let mut actions = ConfigEditorActions::default();

        // Header showing the selected scope with tooltip for config path
        let (header_text, tooltip_text) = match &self.config_state.selected_scope {
            ConfigScope::Global => {
                actions.is_global = true;
                let path = crate::config::global_config_path()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| "~/.config/autom8/config.toml".to_string());
                ("Global Config".to_string(), path)
            }
            ConfigScope::Project(name) => {
                actions.is_global = false;
                actions.project_name = Some(name.clone());
                let path = crate::config::project_config_path_for(name)
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| format!("~/.config/autom8/{}/config.toml", name));
                if self.project_has_config(name) {
                    (format!("Project Config: {}", name), path)
                } else {
                    (format!("Project Config: {} (using global)", name), path)
                }
            }
        };

        // Header with tooltip
        let header_response = ui.label(
            egui::RichText::new(&header_text)
                .font(typography::font(FontSize::Title, FontWeight::SemiBold))
                .color(colors::TEXT_PRIMARY),
        );
        header_response.on_hover_text(&tooltip_text);

        ui.add_space(spacing::MD);

        // Render content based on scope
        match &self.config_state.selected_scope {
            ConfigScope::Global => {
                let (bool_changes, text_changes, reset_clicked) =
                    self.render_global_config_editor(ui);
                actions.bool_changes = bool_changes;
                actions.text_changes = text_changes;
                actions.reset_to_defaults = reset_clicked;
            }
            ConfigScope::Project(name) => {
                // Project config editor (US-004, US-007, US-009)
                // Check if the project has its own config file
                if self.project_has_config(name) {
                    let (bool_changes, text_changes, reset_clicked) =
                        self.render_project_config_editor(ui, name);
                    actions.bool_changes = bool_changes;
                    actions.text_changes = text_changes;
                    actions.reset_to_defaults = reset_clicked;
                } else {
                    // Project doesn't have its own config - show message and button (US-005)
                    let project_name = name.clone();
                    egui::ScrollArea::vertical()
                        .id_salt("config_editor")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.add_space(spacing::XXL);
                            ui.vertical_centered(|ui| {
                                // Information message
                                ui.label(
                                    egui::RichText::new(
                                        "This project does not have a config file.\nIt uses the global configuration.",
                                    )
                                    .font(typography::font(FontSize::Body, FontWeight::Regular))
                                    .color(colors::TEXT_MUTED),
                                );

                                ui.add_space(spacing::LG);

                                // Create Project Config button (US-005)
                                if self.render_create_config_button(ui) {
                                    actions.create_project_config = Some(project_name.clone());
                                }
                            });
                        });
                }
            }
        }

        // Show "Changes take effect on next run" notice if config was recently modified (US-006)
        if self.config_state.last_modified.is_some() {
            ui.add_space(spacing::MD);
            ui.label(
                egui::RichText::new("Changes take effect on next run")
                    .font(typography::font(FontSize::Small, FontWeight::Regular))
                    .color(colors::TEXT_MUTED),
            );
        }

        actions
    }

    /// Render the "Create Project Config" button (US-005).
    ///
    /// Returns true if the button was clicked.
    fn render_create_config_button(&self, ui: &mut egui::Ui) -> bool {
        let button_text = "Create Project Config";
        let text_galley = ui.fonts(|f| {
            f.layout_no_wrap(
                button_text.to_string(),
                typography::font(FontSize::Body, FontWeight::Medium),
                colors::TEXT_PRIMARY,
            )
        });
        let text_size = text_galley.size();

        // Button dimensions with padding
        let button_padding_h = spacing::LG;
        let button_padding_v = spacing::SM;
        let button_size = Vec2::new(
            text_size.x + button_padding_h * 2.0,
            text_size.y + button_padding_v * 2.0,
        );

        // Allocate space and get response
        let (rect, response) = ui.allocate_exact_size(button_size, Sense::click());
        let is_hovered = response.hovered();

        // Draw button background
        let bg_color = if is_hovered {
            colors::ACCENT
        } else {
            colors::ACCENT_SUBTLE
        };
        ui.painter()
            .rect_filled(rect, Rounding::same(rounding::BUTTON), bg_color);

        // Draw button text
        let text_color = if is_hovered {
            colors::TEXT_PRIMARY
        } else {
            colors::ACCENT
        };
        let text_pos = rect.center() - text_size / 2.0;
        ui.painter().galley(
            text_pos,
            ui.fonts(|f| {
                f.layout_no_wrap(
                    button_text.to_string(),
                    typography::font(FontSize::Body, FontWeight::Medium),
                    text_color,
                )
            }),
            text_color,
        );

        response.clicked()
    }

    /// Render the "Reset to Defaults" button (US-009).
    ///
    /// Styled as a secondary/subtle action - uses muted colors and smaller weight.
    /// Returns true if the button was clicked.
    fn render_reset_to_defaults_button(&self, ui: &mut egui::Ui) -> bool {
        let button_text = "Reset to Defaults";
        let text_galley = ui.fonts(|f| {
            f.layout_no_wrap(
                button_text.to_string(),
                typography::font(FontSize::Small, FontWeight::Regular),
                colors::TEXT_MUTED,
            )
        });
        let text_size = text_galley.size();

        // Button dimensions with modest padding (subtle button)
        let button_padding_h = spacing::MD;
        let button_padding_v = spacing::XS;
        let button_size = Vec2::new(
            text_size.x + button_padding_h * 2.0,
            text_size.y + button_padding_v * 2.0,
        );

        // Allocate space and get response
        let (rect, response) = ui.allocate_exact_size(button_size, Sense::click());
        let is_hovered = response.hovered();

        // Draw subtle button background (only visible on hover)
        if is_hovered {
            ui.painter()
                .rect_filled(rect, Rounding::same(rounding::BUTTON), colors::SURFACE);
        }

        // Draw button text (slightly brighter on hover)
        let text_color = if is_hovered {
            colors::TEXT_SECONDARY
        } else {
            colors::TEXT_MUTED
        };
        let text_pos = rect.center() - text_size / 2.0;
        ui.painter().galley(
            text_pos,
            ui.fonts(|f| {
                f.layout_no_wrap(
                    button_text.to_string(),
                    typography::font(FontSize::Small, FontWeight::Regular),
                    text_color,
                )
            }),
            text_color,
        );

        response.clicked()
    }

    /// Render the global config editor with all fields (US-003, US-006, US-009).
    ///
    /// Displays all 6 config fields grouped logically:
    /// - Pipeline group: review, commit, pull_request
    /// - Worktree group: worktree, worktree_path_pattern, worktree_cleanup
    ///
    /// Boolean fields are rendered as interactive toggle switches (US-006).
    /// Text fields are rendered as editable inputs with real-time validation (US-007).
    /// Includes "Reset to Defaults" button at the bottom (US-009).
    /// Returns tuples of (bool_changes, text_changes, reset_clicked) to be processed by the caller.
    fn render_global_config_editor(
        &self,
        ui: &mut egui::Ui,
    ) -> (BoolFieldChanges, TextFieldChanges, bool) {
        let mut bool_changes: Vec<(ConfigBoolField, bool)> = Vec::new();
        let mut text_changes: Vec<(ConfigTextField, String)> = Vec::new();
        let mut reset_clicked = false;

        // Show error if config failed to load
        if let Some(error) = &self.config_state.global_config_error {
            ui.add_space(spacing::MD);
            ui.label(
                egui::RichText::new(error)
                    .font(typography::font(FontSize::Body, FontWeight::Regular))
                    .color(colors::STATUS_ERROR),
            );
            return (bool_changes, text_changes, reset_clicked);
        }

        // Show loading state or editor
        let Some(config) = &self.config_state.cached_global_config else {
            ui.add_space(spacing::MD);
            ui.label(
                egui::RichText::new("Loading configuration...")
                    .font(typography::font(FontSize::Body, FontWeight::Regular))
                    .color(colors::TEXT_MUTED),
            );
            return (bool_changes, text_changes, reset_clicked);
        };

        // Create mutable copies of boolean fields for toggle interaction (US-006)
        let mut review = config.review;
        let mut commit = config.commit;
        let mut pull_request = config.pull_request;
        let mut worktree = config.worktree;
        let mut worktree_cleanup = config.worktree_cleanup;

        // Create mutable copy of text field for editing (US-007)
        let mut worktree_path_pattern = config.worktree_path_pattern.clone();

        // ScrollArea for config fields
        egui::ScrollArea::vertical()
            .id_salt("config_editor")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Pipeline Settings Group
                self.render_config_group_header(ui, "Pipeline");
                ui.add_space(spacing::SM);

                if self.render_config_bool_field(
                    ui,
                    "review",
                    &mut review,
                    "Code review before committing. When enabled, changes are reviewed for quality before being committed.",
                ) {
                    bool_changes.push((ConfigBoolField::Review, review));
                }

                ui.add_space(spacing::SM);

                // Commit toggle - when disabling commit while pull_request is true,
                // cascade by also disabling pull_request (US-008)
                if self.render_config_bool_field(
                    ui,
                    "commit",
                    &mut commit,
                    "Automatic git commits. When enabled, changes are automatically committed after implementation.",
                ) {
                    bool_changes.push((ConfigBoolField::Commit, commit));
                    // Cascade: if commit is now false and pull_request was true, disable pull_request too
                    if !commit && pull_request {
                        pull_request = false;
                        bool_changes.push((ConfigBoolField::PullRequest, false));
                    }
                }

                ui.add_space(spacing::SM);

                // Pull request toggle - disabled when commit is false (US-008)
                // Shows tooltip explaining why it's disabled
                if self.render_config_bool_field_with_disabled(
                    ui,
                    "pull_request",
                    &mut pull_request,
                    "Automatic PR creation. When enabled, a pull request is created after committing. Requires commit to be enabled.",
                    !commit, // disabled when commit is false
                    Some("Pull requests require commits to be enabled"),
                ) {
                    bool_changes.push((ConfigBoolField::PullRequest, pull_request));
                }

                ui.add_space(spacing::XL);

                // Worktree Settings Group
                self.render_config_group_header(ui, "Worktree");
                ui.add_space(spacing::SM);

                if self.render_config_bool_field(
                    ui,
                    "worktree",
                    &mut worktree,
                    "Automatic worktree creation. When enabled, creates a dedicated worktree for each run, enabling parallel sessions.",
                ) {
                    bool_changes.push((ConfigBoolField::Worktree, worktree));
                }

                ui.add_space(spacing::SM);

                // Editable text field with real-time validation (US-007)
                if let Some(new_value) = self.render_config_text_field(
                    ui,
                    "worktree_path_pattern",
                    &mut worktree_path_pattern,
                    "Pattern for worktree directory names. Placeholders: {repo} = repository name, {branch} = branch name.",
                ) {
                    text_changes.push((ConfigTextField::WorktreePathPattern, new_value));
                }

                ui.add_space(spacing::SM);

                if self.render_config_bool_field(
                    ui,
                    "worktree_cleanup",
                    &mut worktree_cleanup,
                    "Automatic worktree cleanup. When enabled, removes worktrees after successful completion. Failed runs keep their worktrees.",
                ) {
                    bool_changes.push((ConfigBoolField::WorktreeCleanup, worktree_cleanup));
                }

                // Add some padding before the reset button
                ui.add_space(spacing::XXL);

                // Reset to Defaults button (US-009)
                // Styled as a secondary/subtle action at the bottom of the editor
                if self.render_reset_to_defaults_button(ui) {
                    reset_clicked = true;
                }

                // Add some padding at the bottom
                ui.add_space(spacing::XL);
            });

        (bool_changes, text_changes, reset_clicked)
    }

    /// Render the project config editor with all fields (US-004, US-006, US-007, US-008, US-009).
    ///
    /// Uses the same field layout and controls as the global config editor.
    /// The UI is identical but operates on the project-specific config file.
    /// Boolean fields are rendered as interactive toggle switches (US-006).
    /// Text fields are rendered as editable inputs with real-time validation (US-007).
    /// Includes "Reset to Defaults" button at the bottom (US-009).
    fn render_project_config_editor(
        &self,
        ui: &mut egui::Ui,
        project_name: &str,
    ) -> (BoolFieldChanges, TextFieldChanges, bool) {
        let mut bool_changes: Vec<(ConfigBoolField, bool)> = Vec::new();
        let mut text_changes: Vec<(ConfigTextField, String)> = Vec::new();
        let mut reset_clicked = false;

        // Show error if config failed to load
        if let Some(error) = &self.config_state.project_config_error {
            ui.add_space(spacing::MD);
            ui.label(
                egui::RichText::new(error)
                    .font(typography::font(FontSize::Body, FontWeight::Regular))
                    .color(colors::STATUS_ERROR),
            );
            return (bool_changes, text_changes, reset_clicked);
        }

        // Show loading state or editor
        let Some(config) = self.cached_project_config(project_name) else {
            ui.add_space(spacing::MD);
            ui.label(
                egui::RichText::new("Loading configuration...")
                    .font(typography::font(FontSize::Body, FontWeight::Regular))
                    .color(colors::TEXT_MUTED),
            );
            return (bool_changes, text_changes, reset_clicked);
        };

        // Create mutable copies of boolean fields for toggle interaction (US-006)
        let mut review = config.review;
        let mut commit = config.commit;
        let mut pull_request = config.pull_request;
        let mut worktree = config.worktree;
        let mut worktree_cleanup = config.worktree_cleanup;

        // Create mutable copy of text field for editing (US-007)
        let mut worktree_path_pattern = config.worktree_path_pattern.clone();

        // ScrollArea for config fields
        egui::ScrollArea::vertical()
            .id_salt("project_config_editor")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Pipeline Settings Group
                self.render_config_group_header(ui, "Pipeline");
                ui.add_space(spacing::SM);

                if self.render_config_bool_field(
                    ui,
                    "review",
                    &mut review,
                    "Code review before committing. When enabled, changes are reviewed for quality before being committed.",
                ) {
                    bool_changes.push((ConfigBoolField::Review, review));
                }

                ui.add_space(spacing::SM);

                // Commit toggle - when disabling commit while pull_request is true,
                // cascade by also disabling pull_request (US-008)
                if self.render_config_bool_field(
                    ui,
                    "commit",
                    &mut commit,
                    "Automatic git commits. When enabled, changes are automatically committed after implementation.",
                ) {
                    bool_changes.push((ConfigBoolField::Commit, commit));
                    // Cascade: if commit is now false and pull_request was true, disable pull_request too
                    if !commit && pull_request {
                        pull_request = false;
                        bool_changes.push((ConfigBoolField::PullRequest, false));
                    }
                }

                ui.add_space(spacing::SM);

                // Pull request toggle - disabled when commit is false (US-008)
                // Shows tooltip explaining why it's disabled
                if self.render_config_bool_field_with_disabled(
                    ui,
                    "pull_request",
                    &mut pull_request,
                    "Automatic PR creation. When enabled, a pull request is created after committing. Requires commit to be enabled.",
                    !commit, // disabled when commit is false
                    Some("Pull requests require commits to be enabled"),
                ) {
                    bool_changes.push((ConfigBoolField::PullRequest, pull_request));
                }

                ui.add_space(spacing::XL);

                // Worktree Settings Group
                self.render_config_group_header(ui, "Worktree");
                ui.add_space(spacing::SM);

                if self.render_config_bool_field(
                    ui,
                    "worktree",
                    &mut worktree,
                    "Automatic worktree creation. When enabled, creates a dedicated worktree for each run, enabling parallel sessions.",
                ) {
                    bool_changes.push((ConfigBoolField::Worktree, worktree));
                }

                ui.add_space(spacing::SM);

                // Editable text field with real-time validation (US-007)
                if let Some(new_value) = self.render_config_text_field(
                    ui,
                    "worktree_path_pattern",
                    &mut worktree_path_pattern,
                    "Pattern for worktree directory names. Placeholders: {repo} = repository name, {branch} = branch name.",
                ) {
                    text_changes.push((ConfigTextField::WorktreePathPattern, new_value));
                }

                ui.add_space(spacing::SM);

                if self.render_config_bool_field(
                    ui,
                    "worktree_cleanup",
                    &mut worktree_cleanup,
                    "Automatic worktree cleanup. When enabled, removes worktrees after successful completion. Failed runs keep their worktrees.",
                ) {
                    bool_changes.push((ConfigBoolField::WorktreeCleanup, worktree_cleanup));
                }

                // Add some padding before the reset button
                ui.add_space(spacing::XXL);

                // Reset to Defaults button (US-009)
                // Styled as a secondary/subtle action at the bottom of the editor
                if self.render_reset_to_defaults_button(ui) {
                    reset_clicked = true;
                }

                // Add some padding at the bottom
                ui.add_space(spacing::XL);
            });

        (bool_changes, text_changes, reset_clicked)
    }

    /// Render a config group header.
    fn render_config_group_header(&self, ui: &mut egui::Ui, title: &str) {
        ui.label(
            egui::RichText::new(title)
                .font(typography::font(FontSize::Heading, FontWeight::SemiBold))
                .color(colors::TEXT_PRIMARY),
        );
    }

    /// Render a boolean config field with an interactive toggle switch (US-006, US-008).
    ///
    /// Displays the field with a toggle switch (not a checkbox) that can be clicked
    /// to change the value. The toggle provides visual feedback matching the app's style.
    /// Returns `true` if the toggle was clicked (value changed).
    ///
    /// # Arguments
    ///
    /// * `ui` - The egui UI context
    /// * `name` - The field name to display
    /// * `value` - The current boolean value (mutable reference for toggle_value)
    /// * `help_text` - Descriptive help text shown below the field
    ///
    /// # Returns
    ///
    /// `true` if the toggle was clicked and the value changed, `false` otherwise.
    fn render_config_bool_field(
        &self,
        ui: &mut egui::Ui,
        name: &str,
        value: &mut bool,
        help_text: &str,
    ) -> bool {
        self.render_config_bool_field_with_disabled(ui, name, value, help_text, false, None)
    }

    /// Render a boolean config field with optional disabled state and tooltip (US-008).
    ///
    /// When disabled, the toggle is greyed out, non-interactive, and shows a tooltip
    /// explaining why. This is used for validation constraints like `pull_request`
    /// requiring `commit` to be enabled.
    ///
    /// # Arguments
    ///
    /// * `ui` - The egui UI context
    /// * `name` - The field name to display
    /// * `value` - The current boolean value (mutable reference for toggle_value)
    /// * `help_text` - Descriptive help text shown below the field
    /// * `disabled` - If true, the toggle is greyed out and non-interactive
    /// * `disabled_tooltip` - Tooltip text shown when hovering over a disabled toggle
    ///
    /// # Returns
    ///
    /// `true` if the toggle was clicked and the value changed, `false` otherwise.
    fn render_config_bool_field_with_disabled(
        &self,
        ui: &mut egui::Ui,
        name: &str,
        value: &mut bool,
        help_text: &str,
        disabled: bool,
        disabled_tooltip: Option<&str>,
    ) -> bool {
        let original_value = *value;

        ui.horizontal(|ui| {
            // Field name - use disabled color if disabled
            let text_color = if disabled {
                colors::TEXT_DISABLED
            } else {
                colors::TEXT_PRIMARY
            };
            ui.label(
                egui::RichText::new(name)
                    .font(typography::font(FontSize::Body, FontWeight::Medium))
                    .color(text_color),
            );

            ui.add_space(spacing::SM);

            // Interactive toggle switch (US-006) or disabled toggle (US-008)
            if disabled {
                let response = ui.add(Self::toggle_switch_disabled(*value));
                // Show tooltip on hover when disabled (US-008)
                if let Some(tooltip) = disabled_tooltip {
                    response.on_hover_text(tooltip);
                }
            } else {
                ui.add(Self::toggle_switch(value));
            }
        });

        // Help text below the field - use disabled color if disabled
        let help_color = if disabled {
            colors::TEXT_DISABLED
        } else {
            colors::TEXT_MUTED
        };
        ui.label(
            egui::RichText::new(help_text)
                .font(typography::font(FontSize::Small, FontWeight::Regular))
                .color(help_color),
        );

        // Return whether the value changed
        *value != original_value
    }

    /// Create an iOS/macOS style toggle switch widget (US-006).
    ///
    /// This creates a toggle switch that looks like a slider/pill shape rather
    /// than a checkbox, matching modern UI conventions.
    fn toggle_switch(on: &mut bool) -> impl egui::Widget + '_ {
        move |ui: &mut egui::Ui| -> egui::Response {
            // Toggle dimensions - slightly smaller than standard for config fields
            let desired_size = Vec2::new(36.0, 20.0);

            // Allocate space and handle interaction
            let (rect, mut response) = ui.allocate_exact_size(desired_size, Sense::click());

            // Handle click
            if response.clicked() {
                *on = !*on;
                response.mark_changed();
            }

            // Draw the toggle
            if ui.is_rect_visible(rect) {
                let how_on = ui.ctx().animate_bool_responsive(response.id, *on);
                let visuals = ui.style().interact_selectable(&response, *on);

                // Background pill shape
                let rect = rect.expand(visuals.expansion);
                let radius = 0.5 * rect.height();

                // Use accent color when on, muted when off
                let bg_color = if *on {
                    colors::ACCENT_SUBTLE
                } else {
                    colors::SURFACE_HOVER
                };
                ui.painter()
                    .rect_filled(rect, Rounding::same(radius), bg_color);

                // Border
                let border_color = if *on { colors::ACCENT } else { colors::BORDER };
                ui.painter().rect_stroke(
                    rect,
                    Rounding::same(radius),
                    Stroke::new(1.0, border_color),
                );

                // Circle knob
                let circle_x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), how_on);
                let center = egui::pos2(circle_x, rect.center().y);
                let knob_radius = radius * 0.75;

                // Knob shadow for depth
                ui.painter().circle_filled(
                    center + egui::vec2(0.5, 0.5),
                    knob_radius,
                    Color32::from_black_alpha(30),
                );

                // Knob
                ui.painter()
                    .circle_filled(center, knob_radius, colors::TEXT_PRIMARY);
            }

            response
        }
    }

    /// Create a disabled iOS/macOS style toggle switch widget (US-008).
    ///
    /// This creates a non-interactive toggle that displays the current value
    /// but cannot be clicked. It uses greyed-out colors to indicate the disabled state.
    /// Used for validation constraints (e.g., pull_request requires commit to be enabled).
    fn toggle_switch_disabled(on: bool) -> impl egui::Widget {
        move |ui: &mut egui::Ui| -> egui::Response {
            // Toggle dimensions - same as regular toggle
            let desired_size = Vec2::new(36.0, 20.0);

            // Allocate space but with hover sense only (no click)
            // This allows the tooltip to work
            let (rect, response) = ui.allocate_exact_size(desired_size, Sense::hover());

            // Draw the toggle in disabled state
            if ui.is_rect_visible(rect) {
                // Animate based on current value (but won't change)
                let how_on = ui.ctx().animate_bool_responsive(response.id, on);

                // Background pill shape
                let radius = 0.5 * rect.height();

                // Use very muted colors for disabled state
                let bg_color = colors::SURFACE_HOVER;
                ui.painter()
                    .rect_filled(rect, Rounding::same(radius), bg_color);

                // Border - use disabled/muted color
                ui.painter().rect_stroke(
                    rect,
                    Rounding::same(radius),
                    Stroke::new(1.0, colors::BORDER),
                );

                // Circle knob - positioned based on value but greyed out
                let circle_x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), how_on);
                let center = egui::pos2(circle_x, rect.center().y);
                let knob_radius = radius * 0.75;

                // No shadow for disabled state (flatter appearance)

                // Knob - use disabled color
                ui.painter()
                    .circle_filled(center, knob_radius, colors::TEXT_DISABLED);
            }

            response
        }
    }

    /// Render a text config field with label, editable input, and help text (US-007).
    ///
    /// The text input allows inline editing with real-time validation.
    /// For `worktree_path_pattern`, warns if `{repo}` or `{branch}` placeholders are missing.
    /// Invalid patterns are still saved (warning only, not blocking).
    ///
    /// Returns `Some(new_value)` if the text was changed, `None` otherwise.
    fn render_config_text_field(
        &self,
        ui: &mut egui::Ui,
        name: &str,
        value: &mut String,
        help_text: &str,
    ) -> Option<String> {
        let mut changed_value: Option<String> = None;

        ui.horizontal(|ui| {
            // Field name
            ui.label(
                egui::RichText::new(name)
                    .font(typography::font(FontSize::Body, FontWeight::Medium))
                    .color(colors::TEXT_PRIMARY),
            );

            ui.add_space(spacing::SM);

            // Editable text input (US-007)
            let text_edit = egui::TextEdit::singleline(value)
                .font(typography::mono(FontSize::Body))
                .text_color(colors::TEXT_SECONDARY)
                .desired_width(250.0);

            let response = ui.add(text_edit);
            if response.changed() {
                changed_value = Some(value.clone());
            }
        });

        // Help text below the field
        ui.label(
            egui::RichText::new(help_text)
                .font(typography::font(FontSize::Small, FontWeight::Regular))
                .color(colors::TEXT_MUTED),
        );

        // Real-time validation for worktree_path_pattern (US-007)
        if name == "worktree_path_pattern" {
            let mut warnings: Vec<&str> = Vec::new();

            if !value.contains("{repo}") {
                warnings.push("Missing {repo} placeholder");
            }
            if !value.contains("{branch}") {
                warnings.push("Missing {branch} placeholder");
            }

            // Display validation warnings in amber/warning color
            if !warnings.is_empty() {
                ui.add_space(spacing::XS);
                for warning in warnings {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("⚠")
                                .font(typography::font(FontSize::Small, FontWeight::Regular))
                                .color(colors::STATUS_WARNING),
                        );
                        ui.add_space(spacing::XS);
                        ui.label(
                            egui::RichText::new(warning)
                                .font(typography::font(FontSize::Small, FontWeight::Regular))
                                .color(colors::STATUS_WARNING),
                        );
                    });
                }
            }
        }

        changed_value
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

                // Token usage section
                ui.label(
                    egui::RichText::new("Total Tokens:")
                        .font(typography::font(FontSize::Body, FontWeight::Medium))
                        .color(colors::TEXT_SECONDARY),
                );
                if let Some(ref usage) = run_state.total_usage {
                    let total = Self::format_tokens(usage.total_tokens());
                    let input = Self::format_tokens(usage.input_tokens);
                    let output = Self::format_tokens(usage.output_tokens);
                    ui.label(
                        egui::RichText::new(format!("{} ({} in / {} out)", total, input, output))
                            .font(typography::font(FontSize::Body, FontWeight::Regular))
                            .color(colors::TEXT_PRIMARY),
                    );
                } else {
                    ui.label(
                        egui::RichText::new("N/A")
                            .font(typography::font(FontSize::Body, FontWeight::Regular))
                            .color(colors::TEXT_MUTED),
                    );
                }
                ui.end_row();

                // Cache stats (only if we have usage data)
                if let Some(ref usage) = run_state.total_usage {
                    if usage.cache_read_tokens > 0 || usage.cache_creation_tokens > 0 {
                        ui.label(
                            egui::RichText::new("Cache:")
                                .font(typography::font(FontSize::Body, FontWeight::Medium))
                                .color(colors::TEXT_SECONDARY),
                        );
                        let cache_read = Self::format_tokens(usage.cache_read_tokens);
                        let cache_created = Self::format_tokens(usage.cache_creation_tokens);
                        ui.label(
                            egui::RichText::new(format!(
                                "{} read / {} created",
                                cache_read, cache_created
                            ))
                            .font(typography::font(FontSize::Body, FontWeight::Regular))
                            .color(colors::TEXT_PRIMARY),
                        );
                        ui.end_row();
                    }

                    // Model name
                    if let Some(ref model) = usage.model {
                        ui.label(
                            egui::RichText::new("Model:")
                                .font(typography::font(FontSize::Body, FontWeight::Medium))
                                .color(colors::TEXT_SECONDARY),
                        );
                        ui.label(
                            egui::RichText::new(model)
                                .font(typography::font(FontSize::Body, FontWeight::Regular))
                                .color(colors::TEXT_PRIMARY),
                        );
                        ui.end_row();
                    }
                }
            });

        // Pseudo-phase breakdown (only show phases that have usage data)
        let pseudo_phases = ["Planning", "Final Review", "PR & Commit"];
        let has_pseudo_phase_usage = pseudo_phases
            .iter()
            .any(|phase| run_state.phase_usage.contains_key(*phase));

        if has_pseudo_phase_usage {
            ui.add_space(spacing::SM);

            ui.label(
                egui::RichText::new("Phase Breakdown")
                    .font(typography::font(FontSize::Small, FontWeight::SemiBold))
                    .color(colors::TEXT_SECONDARY),
            );

            ui.add_space(spacing::XS);

            egui::Grid::new("phase_usage_grid")
                .num_columns(2)
                .spacing([spacing::LG, spacing::XS])
                .show(ui, |ui| {
                    for phase in pseudo_phases {
                        if let Some(usage) = run_state.phase_usage.get(phase) {
                            ui.label(
                                egui::RichText::new(format!("{}:", phase))
                                    .font(typography::font(FontSize::Small, FontWeight::Regular))
                                    .color(colors::TEXT_SECONDARY),
                            );
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} tokens",
                                    Self::format_tokens(usage.total_tokens())
                                ))
                                .font(typography::font(FontSize::Small, FontWeight::Regular))
                                .color(colors::TEXT_PRIMARY),
                            );
                            ui.end_row();
                        }
                    }
                });
        }
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

                            // Token usage for this iteration (if available)
                            if let Some(ref usage) = iter.usage {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "• {} tokens",
                                        Self::format_tokens(usage.total_tokens())
                                    ))
                                    .font(typography::font(FontSize::Caption, FontWeight::Regular))
                                    .color(colors::TEXT_MUTED),
                                );
                            }
                        });
                    }
                } else {
                    // Single iteration - show duration and tokens
                    let iter = iterations[0];
                    ui.add_space(spacing::XS);

                    // Build the info string with duration and tokens
                    let mut info_parts = Vec::new();

                    if let Some(finished) = iter.finished_at {
                        let duration = finished - iter.started_at;
                        info_parts.push(format!(
                            "Duration: {}",
                            Self::format_duration_detailed(duration)
                        ));
                    }

                    if let Some(ref usage) = iter.usage {
                        info_parts.push(format!(
                            "Tokens: {}",
                            Self::format_tokens(usage.total_tokens())
                        ));
                    }

                    if !info_parts.is_empty() {
                        ui.label(
                            egui::RichText::new(info_parts.join(" • "))
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

    /// Format a token count with thousands separators (e.g., 1234567 -> "1,234,567").
    fn format_tokens(tokens: u64) -> String {
        let s = tokens.to_string();
        let mut result = String::new();
        for (i, c) in s.chars().rev().enumerate() {
            if i > 0 && i % 3 == 0 {
                result.push(',');
            }
            result.push(c);
        }
        result.chars().rev().collect()
    }

    /// Render the Active Runs view with tab bar for session selection.
    ///
    /// When there are active sessions, displays a horizontal tab bar where each
    /// session gets its own tab (labeled with branch name). Clicking a tab switches
    /// to that session's expanded view with full output display.
    ///
    /// US-003: Tabs have close buttons and follow a lifecycle:
    /// - Tabs auto-appear for new active runs
    /// - Tabs persist after completion until manually closed
    /// - Closing all tabs returns to empty state
    fn render_active_runs(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            // Header section with consistent spacing
            ui.label(
                egui::RichText::new("Active Runs")
                    .font(typography::font(FontSize::Title, FontWeight::SemiBold))
                    .color(colors::TEXT_PRIMARY),
            );

            ui.add_space(spacing::SM);

            // US-003: Filter sessions to only those with open tabs
            let visible_sessions: Vec<(usize, &SessionData)> = self
                .sessions
                .iter()
                .enumerate()
                .filter(|(_, s)| self.open_session_tabs.contains(&s.metadata.session_id))
                .collect();

            // Empty state if no sessions or all tabs closed
            if visible_sessions.is_empty() {
                self.render_empty_active_runs(ui);
            } else {
                // Ensure valid selection: auto-select first visible session if none selected
                // or if current selection is not visible (US-005: robust to session order changes)
                let current_selection_valid = self.selected_session_id.as_ref().is_some_and(|id| {
                    visible_sessions
                        .iter()
                        .any(|(_, s)| s.metadata.session_id == *id)
                });

                if !current_selection_valid {
                    // Select first visible session by ID
                    self.selected_session_id = visible_sessions
                        .first()
                        .map(|(_, s)| s.metadata.session_id.clone());
                }

                // Render tab bar for session selection
                self.render_active_session_tab_bar(ui);

                ui.add_space(spacing::SM);

                // Render expanded view for selected session (find by ID, robust to reordering)
                if let Some(ref selected_id) = self.selected_session_id {
                    if let Some(session) = self
                        .sessions
                        .iter()
                        .find(|s| s.metadata.session_id == *selected_id)
                    {
                        // Clone session data to avoid borrow issues
                        let session = session.clone();
                        self.render_expanded_session_view(ui, &session);
                    }
                }
            }
        });
    }

    /// Render the horizontal tab bar for active session selection.
    ///
    /// Each session gets a tab with its branch name (worktree prefix stripped).
    /// The tab bar scrolls horizontally if there are many tabs (US-005).
    ///
    /// US-003: Tabs have close buttons. Only sessions with open tabs are shown.
    /// US-005: Uses session ID instead of index for robust selection during rapid changes.
    fn render_active_session_tab_bar(&mut self, ui: &mut egui::Ui) {
        let available_width = ui.available_width();
        let scroll_width = available_width.min(TAB_BAR_MAX_SCROLL_WIDTH);

        // Track which tab to select and which to close (by session ID for robustness)
        let mut tab_to_select: Option<String> = None;
        let mut tab_to_close: Option<String> = None;

        // Collect visible sessions (those with open tabs)
        let visible_sessions: Vec<(String, String)> = self
            .sessions
            .iter()
            .filter(|s| self.open_session_tabs.contains(&s.metadata.session_id))
            .map(|s| {
                let branch_label = strip_worktree_prefix(&s.metadata.branch_name, &s.project_name);
                (s.metadata.session_id.clone(), branch_label)
            })
            .collect();

        ui.allocate_ui_with_layout(
            egui::vec2(available_width, CONTENT_TAB_BAR_HEIGHT),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                egui::ScrollArea::horizontal()
                    .max_width(scroll_width)
                    .auto_shrink([false, false])
                    .scroll_bar_visibility(
                        egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded,
                    )
                    .show(ui, |ui| {
                        ui.horizontal_centered(|ui| {
                            ui.add_space(spacing::XS);

                            for (session_id, branch_label) in &visible_sessions {
                                let is_active = self
                                    .selected_session_id
                                    .as_ref()
                                    .is_some_and(|id| id == session_id);

                                let (tab_clicked, close_clicked) =
                                    self.render_active_session_tab(ui, branch_label, is_active);

                                if tab_clicked {
                                    tab_to_select = Some(session_id.clone());
                                }
                                if close_clicked {
                                    tab_to_close = Some(session_id.clone());
                                }
                                ui.add_space(spacing::XS);
                            }
                        });
                    });
            },
        );

        // Apply selection after render loop (by ID, robust to reordering)
        if let Some(session_id) = tab_to_select {
            self.selected_session_id = Some(session_id);
        }

        // US-003: Handle tab close
        if let Some(session_id) = tab_to_close {
            self.close_session_tab(&session_id);
        }
    }

    /// Close a session tab (US-003).
    ///
    /// Removes the tab from the open set and adds to closed set.
    /// If the closed tab was selected, clears selection (render_active_runs will auto-select).
    fn close_session_tab(&mut self, session_id: &str) {
        // Remove from open tabs
        self.open_session_tabs.remove(session_id);

        // Add to closed tabs (prevents auto-reopen)
        self.closed_session_tabs.insert(session_id.to_string());

        // If the closed tab was selected, clear selection
        // (render_active_runs will auto-select the first available)
        if self
            .selected_session_id
            .as_ref()
            .is_some_and(|id| id == session_id)
        {
            self.selected_session_id = None;
        }
    }

    /// Render a single tab in the active session tab bar.
    ///
    /// US-003: Tabs have close buttons (X) that are always visible.
    /// Returns (tab_clicked, close_clicked).
    fn render_active_session_tab(
        &self,
        ui: &mut egui::Ui,
        label: &str,
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

        // Calculate tab dimensions including close button
        let close_button_space = TAB_CLOSE_BUTTON_SIZE + TAB_CLOSE_PADDING;
        let tab_width = text_size.x + TAB_PADDING_H * 2.0 + close_button_space;
        let tab_height = CONTENT_TAB_BAR_HEIGHT - TAB_UNDERLINE_HEIGHT - spacing::XS;
        let tab_size = egui::vec2(tab_width, tab_height);

        // Allocate space for the tab
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

        // US-003: Draw close button
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

    /// Render the expanded view for a selected session.
    ///
    /// This view takes most of the available space and shows:
    /// - Session metadata (project, branch, state, progress) at top
    /// - Large output section filling remaining space
    ///
    /// Styled to match RunDetail view with Title-sized header and consistent padding.
    fn render_expanded_session_view(&mut self, ui: &mut egui::Ui, session: &SessionData) {
        let available_width = ui.available_width();
        let available_height = ui.available_height();

        // Use the full available area with padding matching RunDetail style
        let content_padding = spacing::LG;
        let content_width = available_width - content_padding * 2.0;

        // Determine if we should use stacked (vertical) or side-by-side (horizontal) layout
        // Stack vertically when content area is too narrow for two readable panels
        let panel_gap = spacing::LG; // Gap between panels
        let use_stacked_layout = content_width < RESPONSIVE_STACK_BREAKPOINT;

        // Calculate panel widths based on layout mode
        let panel_width = if use_stacked_layout {
            // In stacked mode, each panel gets the full content width
            content_width
        } else {
            // In side-by-side mode, split 50/50 with gap, ensuring minimum width
            ((content_width - panel_gap) / 2.0).max(EXPANDED_PANEL_MIN_WIDTH)
        };

        ui.allocate_ui_with_layout(
            egui::vec2(available_width, available_height),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| {
                ui.add_space(content_padding);

                ui.horizontal(|ui| {
                    ui.add_space(content_padding);

                    ui.vertical(|ui| {
                        ui.set_width(content_width);

                        // === HEADER: Branch name (primary identifier) with session badge ===
                        let branch_display = strip_worktree_prefix(
                            &session.metadata.branch_name,
                            &session.project_name,
                        );

                        ui.horizontal(|ui| {
                            // Branch name as the prominent title (matches RunDetail header size)
                            ui.label(
                                egui::RichText::new(&branch_display)
                                    .font(typography::font(FontSize::Title, FontWeight::SemiBold))
                                    .color(colors::TEXT_PRIMARY),
                            );

                            ui.add_space(spacing::MD);

                            // Session badge
                            let badge_text = if session.is_main_session {
                                "main"
                            } else {
                                &session.metadata.session_id
                            };
                            let badge_color = if session.is_main_session {
                                colors::ACCENT
                            } else {
                                colors::TEXT_SECONDARY
                            };
                            let badge_bg = if session.is_main_session {
                                colors::ACCENT_SUBTLE
                            } else {
                                colors::SURFACE_HOVER
                            };

                            egui::Frame::none()
                                .fill(badge_bg)
                                .rounding(rounding::SMALL)
                                .inner_margin(egui::Margin::symmetric(spacing::SM, spacing::XS))
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(badge_text)
                                            .font(typography::font(
                                                FontSize::Small,
                                                FontWeight::Medium,
                                            ))
                                            .color(badge_color),
                                    );
                                });
                        });

                        ui.add_space(spacing::XS);

                        // === PROJECT NAME (secondary info) ===
                        ui.label(
                            egui::RichText::new(&session.project_name)
                                .font(typography::font(FontSize::Body, FontWeight::Regular))
                                .color(colors::TEXT_MUTED),
                        );

                        ui.add_space(spacing::MD);

                        // === STATUS ROW ===
                        let appears_stuck = session.appears_stuck();
                        let (state, state_color) = if let Some(ref run) = session.run {
                            let base_color = state_to_color(run.machine_state);
                            let color = if appears_stuck {
                                colors::STATUS_WARNING
                            } else {
                                base_color
                            };
                            (run.machine_state, color)
                        } else {
                            (MachineState::Idle, colors::STATUS_IDLE)
                        };

                        ui.horizontal(|ui| {
                            // Status dot
                            let dot_size = 8.0;
                            let (rect, _) = ui.allocate_exact_size(
                                egui::vec2(dot_size, dot_size),
                                Sense::hover(),
                            );
                            ui.painter()
                                .circle_filled(rect.center(), dot_size / 2.0, state_color);

                            ui.add_space(spacing::SM);

                            // State text
                            let state_text = if appears_stuck {
                                format!("{} (Not responding)", format_state(state))
                            } else {
                                format_state(state).to_string()
                            };
                            ui.label(
                                egui::RichText::new(state_text)
                                    .font(typography::font(FontSize::Body, FontWeight::Medium))
                                    .color(colors::TEXT_PRIMARY),
                            );

                            // Progress info
                            if let Some(ref progress) = session.progress {
                                ui.add_space(spacing::MD);
                                ui.label(
                                    egui::RichText::new(progress.as_fraction())
                                        .font(typography::font(FontSize::Body, FontWeight::Regular))
                                        .color(colors::TEXT_SECONDARY),
                                );

                                // Current story
                                if let Some(ref run) = session.run {
                                    if let Some(ref story_id) = run.current_story {
                                        ui.add_space(spacing::SM);
                                        ui.label(
                                            egui::RichText::new(story_id)
                                                .font(typography::font(
                                                    FontSize::Body,
                                                    FontWeight::Regular,
                                                ))
                                                .color(colors::TEXT_MUTED),
                                        );
                                    }
                                }
                            }

                            // Duration
                            if let Some(ref run) = session.run {
                                ui.add_space(spacing::MD);
                                ui.label(
                                    egui::RichText::new(format_duration(run.started_at))
                                        .font(typography::font(FontSize::Body, FontWeight::Regular))
                                        .color(colors::TEXT_MUTED),
                                );
                            }
                        });

                        ui.add_space(spacing::LG);

                        // === RESPONSIVE SPLIT PANEL LAYOUT ===
                        // Calculate available space for panels
                        let available_panel_height = ui.available_height() - content_padding;

                        // In stacked mode, use a scrollable vertical layout with fixed-height sections
                        // In side-by-side mode, use horizontal layout with panels filling available height
                        if use_stacked_layout {
                            // === STACKED LAYOUT (Vertical) ===
                            // Wrap both panels in a vertical scroll area
                            egui::ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    // Fixed heights for stacked mode panels
                                    let stacked_output_height = 300.0;
                                    let stacked_section_height = 150.0;

                                    // === OUTPUT PANEL (Stacked) ===
                                    ui.vertical(|ui| {
                                        ui.set_width(panel_width);

                                        // Section header
                                        ui.label(
                                            egui::RichText::new("Output")
                                                .font(typography::font(FontSize::Body, FontWeight::Medium))
                                                .color(colors::TEXT_SECONDARY),
                                        );

                                        ui.add_space(spacing::SM);

                                        egui::Frame::none()
                                            .fill(colors::SURFACE_HOVER)
                                            .rounding(rounding::CARD)
                                            .inner_margin(egui::Margin::same(spacing::MD))
                                            .show(ui, |ui| {
                                                ui.set_min_height(stacked_output_height);
                                                ui.set_max_height(stacked_output_height);
                                                ui.set_width(panel_width - spacing::MD * 2.0);

                                                egui::ScrollArea::vertical()
                                                    .auto_shrink([false, false])
                                                    .stick_to_bottom(true)
                                                    .show(ui, |ui| {
                                                        let output_source = get_output_for_session(session);
                                                        Self::render_output_content(ui, &output_source);
                                                    });
                                            });
                                    });

                                    // Gap between stacked panels
                                    ui.add_space(panel_gap);

                                    // === DETAIL SECTIONS (Stacked) ===
                                    ui.vertical(|ui| {
                                        ui.set_width(panel_width);

                                        // Load data once for display
                                        let story_items = load_story_items(session);
                                        let work_summaries = load_work_summaries(session);
                                        let token_usage = load_token_usage(session);

                                        let section_gap = spacing::MD;

                                        // === Stories Section (Collapsible) ===
                                        CollapsibleSection::new("stories", "Stories")
                                            .default_expanded(true)
                                            .show(ui, &mut self.section_collapsed_state, |ui| {
                                                egui::Frame::none()
                                                    .fill(colors::SURFACE_HOVER)
                                                    .rounding(rounding::CARD)
                                                    .inner_margin(egui::Margin::same(spacing::MD))
                                                    .show(ui, |ui| {
                                                        ui.set_min_height(stacked_section_height);
                                                        ui.set_max_height(stacked_section_height);
                                                        ui.set_width(panel_width - spacing::MD * 2.0);

                                                        egui::ScrollArea::vertical()
                                                            .auto_shrink([false, false])
                                                            .show(ui, |ui| {
                                                                Self::render_story_items_content(ui, &story_items);
                                                            });
                                                    });
                                            });

                                        ui.add_space(section_gap);

                                        // === Work Summaries Section (Collapsible) ===
                                        CollapsibleSection::new("work_summaries", "Work Summaries")
                                            .default_expanded(true)
                                            .show(ui, &mut self.section_collapsed_state, |ui| {
                                                egui::Frame::none()
                                                    .fill(colors::SURFACE_HOVER)
                                                    .rounding(rounding::CARD)
                                                    .inner_margin(egui::Margin::same(spacing::MD))
                                                    .show(ui, |ui| {
                                                        ui.set_min_height(stacked_section_height);
                                                        ui.set_max_height(stacked_section_height);
                                                        ui.set_width(panel_width - spacing::MD * 2.0);

                                                        egui::ScrollArea::vertical()
                                                            .auto_shrink([false, false])
                                                            .show(ui, |ui| {
                                                                Self::render_work_summaries_content(ui, &work_summaries);
                                                            });
                                                    });
                                            });

                                        // === Token Usage Section (Collapsible) ===
                                        if let Some(ref usage_data) = token_usage {
                                            ui.add_space(section_gap);

                                            CollapsibleSection::new("token_usage", "Token Usage")
                                                .default_expanded(true)
                                                .show(ui, &mut self.section_collapsed_state, |ui| {
                                                    egui::Frame::none()
                                                        .fill(colors::SURFACE_HOVER)
                                                        .rounding(rounding::CARD)
                                                        .inner_margin(egui::Margin::same(spacing::MD))
                                                        .show(ui, |ui| {
                                                            ui.set_min_height(stacked_section_height);
                                                            ui.set_max_height(stacked_section_height);
                                                            ui.set_width(panel_width - spacing::MD * 2.0);

                                                            egui::ScrollArea::vertical()
                                                                .auto_shrink([false, false])
                                                                .show(ui, |ui| {
                                                                    Self::render_token_usage_content(ui, usage_data);
                                                                });
                                                        });
                                                });
                                        }
                                    });

                                    // Bottom padding
                                    ui.add_space(content_padding);
                                });
                        } else {
                            // === SIDE-BY-SIDE LAYOUT (Horizontal) ===
                            let panel_height = available_panel_height;

                            ui.horizontal(|ui| {
                                // === LEFT PANEL: Output ===
                                ui.vertical(|ui| {
                                    ui.set_width(panel_width);

                                    // Section header
                                    ui.label(
                                        egui::RichText::new("Output")
                                            .font(typography::font(FontSize::Body, FontWeight::Medium))
                                            .color(colors::TEXT_SECONDARY),
                                    );

                                    ui.add_space(spacing::SM);

                                    // Output card - fills remaining panel height
                                    let output_card_height = panel_height
                                        - typography::line_height(FontSize::Body)
                                        - spacing::SM;

                                    egui::Frame::none()
                                        .fill(colors::SURFACE_HOVER)
                                        .rounding(rounding::CARD)
                                        .inner_margin(egui::Margin::same(spacing::MD))
                                        .show(ui, |ui| {
                                            ui.set_min_height(output_card_height);
                                            ui.set_max_height(output_card_height);
                                            ui.set_width(panel_width - spacing::MD * 2.0);

                                            egui::ScrollArea::vertical()
                                                .auto_shrink([false, false])
                                                .stick_to_bottom(true)
                                                .show(ui, |ui| {
                                                    let output_source = get_output_for_session(session);
                                                    Self::render_output_content(ui, &output_source);
                                                });
                                        });
                                });

                                // Gap between panels
                                ui.add_space(panel_gap);

                            // === RIGHT PANEL: Detail sections ===
                            ui.vertical(|ui| {
                                ui.set_width(panel_width);

                                // Load data once for display
                                let story_items = load_story_items(session);
                                let work_summaries = load_work_summaries(session);
                                let token_usage = load_token_usage(session);

                                // Calculate heights for sections
                                // Each section has: header + spacing + card
                                let section_header_height =
                                    typography::line_height(FontSize::Body) + spacing::SM;
                                let section_gap = spacing::MD;

                                // Determine number of sections (token usage may be hidden)
                                let num_sections = if token_usage.is_some() { 3.0 } else { 2.0 };
                                let num_gaps = if token_usage.is_some() { 2.0 } else { 1.0 };

                                // Split remaining height evenly between sections
                                let available_for_sections = panel_height
                                    - section_header_height * num_sections
                                    - section_gap * num_gaps;
                                let section_card_height =
                                    (available_for_sections / num_sections).max(60.0);

                                // === Stories Section (Collapsible) ===
                                CollapsibleSection::new("stories", "Stories")
                                    .default_expanded(true)
                                    .show(ui, &mut self.section_collapsed_state, |ui| {
                                        egui::Frame::none()
                                            .fill(colors::SURFACE_HOVER)
                                            .rounding(rounding::CARD)
                                            .inner_margin(egui::Margin::same(spacing::MD))
                                            .show(ui, |ui| {
                                                ui.set_min_height(section_card_height);
                                                ui.set_max_height(section_card_height);
                                                ui.set_width(panel_width - spacing::MD * 2.0);

                                                egui::ScrollArea::vertical()
                                                    .auto_shrink([false, false])
                                                    .show(ui, |ui| {
                                                        Self::render_story_items_content(ui, &story_items);
                                                    });
                                            });
                                    });

                                // Gap between sections
                                ui.add_space(section_gap);

                                // === Work Summaries Section (Collapsible) ===
                                CollapsibleSection::new("work_summaries", "Work Summaries")
                                    .default_expanded(true)
                                    .show(ui, &mut self.section_collapsed_state, |ui| {
                                        egui::Frame::none()
                                            .fill(colors::SURFACE_HOVER)
                                            .rounding(rounding::CARD)
                                            .inner_margin(egui::Margin::same(spacing::MD))
                                            .show(ui, |ui| {
                                                ui.set_min_height(section_card_height);
                                                ui.set_max_height(section_card_height);
                                                ui.set_width(panel_width - spacing::MD * 2.0);

                                                egui::ScrollArea::vertical()
                                                    .auto_shrink([false, false])
                                                    .show(ui, |ui| {
                                                        Self::render_work_summaries_content(ui, &work_summaries);
                                                    });
                                            });
                                    });

                                // === Token Usage Section (Collapsible) ===
                                if let Some(ref usage_data) = token_usage {
                                    ui.add_space(section_gap);

                                    CollapsibleSection::new("token_usage", "Token Usage")
                                        .default_expanded(true)
                                        .show(ui, &mut self.section_collapsed_state, |ui| {
                                            egui::Frame::none()
                                                .fill(colors::SURFACE_HOVER)
                                                .rounding(rounding::CARD)
                                                .inner_margin(egui::Margin::same(spacing::MD))
                                                .show(ui, |ui| {
                                                    ui.set_min_height(section_card_height);
                                                    ui.set_max_height(section_card_height);
                                                    ui.set_width(panel_width - spacing::MD * 2.0);

                                                    egui::ScrollArea::vertical()
                                                        .auto_shrink([false, false])
                                                        .show(ui, |ui| {
                                                            Self::render_token_usage_content(ui, usage_data);
                                                        });
                                                });
                                        });
                                }
                            });
                        });
                        } // End of else block (side-by-side layout)
                    });
                });
            },
        );
    }

    /// Render story items content (extracted for reuse in both layouts).
    fn render_story_items_content(ui: &mut egui::Ui, story_items: &[StoryItem]) {
        if story_items.is_empty() {
            ui.label(
                egui::RichText::new("No stories found")
                    .font(typography::font(FontSize::Body, FontWeight::Regular))
                    .color(colors::TEXT_DISABLED),
            );
        } else {
            for (index, story) in story_items.iter().enumerate() {
                if index > 0 {
                    ui.add_space(spacing::SM);
                }

                let is_active = story.status == StoryStatus::Active;

                egui::Frame::none()
                    .fill(story.status.background())
                    .rounding(rounding::SMALL)
                    .inner_margin(egui::Margin::symmetric(spacing::SM, spacing::XS))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(story.status.indicator())
                                    .font(typography::font(FontSize::Body, FontWeight::Medium))
                                    .color(story.status.color()),
                            );

                            ui.add_space(spacing::SM);

                            let id_weight = if is_active {
                                FontWeight::SemiBold
                            } else {
                                FontWeight::Medium
                            };
                            let id_color = if is_active {
                                colors::ACCENT
                            } else {
                                colors::TEXT_PRIMARY
                            };
                            ui.label(
                                egui::RichText::new(&story.id)
                                    .font(typography::font(FontSize::Small, id_weight))
                                    .color(id_color),
                            );
                        });

                        let title_weight = if is_active {
                            FontWeight::Medium
                        } else {
                            FontWeight::Regular
                        };
                        let title_color = if is_active {
                            colors::TEXT_PRIMARY
                        } else {
                            colors::TEXT_SECONDARY
                        };
                        ui.label(
                            egui::RichText::new(&story.title)
                                .font(typography::font(FontSize::Small, title_weight))
                                .color(title_color),
                        );
                    });
            }
        }
    }

    /// Render work summaries content (extracted for reuse in both layouts).
    fn render_work_summaries_content(ui: &mut egui::Ui, work_summaries: &[WorkSummaryItem]) {
        if work_summaries.is_empty() {
            ui.label(
                egui::RichText::new("No summaries yet")
                    .font(typography::font(FontSize::Body, FontWeight::Regular))
                    .color(colors::TEXT_DISABLED),
            );
        } else {
            for (index, summary_item) in work_summaries.iter().enumerate() {
                if index > 0 {
                    ui.add_space(spacing::SM);
                }

                egui::Frame::none()
                    .fill(colors::SURFACE)
                    .rounding(rounding::SMALL)
                    .inner_margin(egui::Margin::symmetric(spacing::SM, spacing::XS))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!(
                                    "Iteration {}",
                                    summary_item.iteration
                                ))
                                .font(typography::font(FontSize::Small, FontWeight::SemiBold))
                                .color(colors::TEXT_PRIMARY),
                            );

                            ui.add_space(spacing::SM);

                            ui.label(
                                egui::RichText::new(&summary_item.story_id)
                                    .font(typography::font(FontSize::Small, FontWeight::Regular))
                                    .color(colors::TEXT_MUTED),
                            );
                        });

                        ui.label(
                            egui::RichText::new(&summary_item.summary)
                                .font(typography::font(FontSize::Small, FontWeight::Regular))
                                .color(colors::TEXT_SECONDARY),
                        );
                    });
            }
        }
    }

    /// Render token usage content (extracted for reuse in both layouts).
    fn render_token_usage_content(ui: &mut egui::Ui, usage_data: &TokenUsageData) {
        // Total Usage Summary
        egui::Frame::none()
            .fill(colors::SURFACE)
            .rounding(rounding::SMALL)
            .inner_margin(egui::Margin::symmetric(spacing::SM, spacing::XS))
            .show(ui, |ui| {
                // Header: Total tokens
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Total")
                            .font(typography::font(FontSize::Small, FontWeight::SemiBold))
                            .color(colors::TEXT_PRIMARY),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format_number_with_commas(
                                usage_data.total.total_tokens(),
                            ))
                            .font(typography::font(FontSize::Small, FontWeight::SemiBold))
                            .color(colors::ACCENT),
                        );
                    });
                });

                ui.add_space(spacing::XS);

                // Input tokens
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("  Input")
                            .font(typography::font(FontSize::Small, FontWeight::Regular))
                            .color(colors::TEXT_SECONDARY),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format_number_with_commas(
                                usage_data.total.input_tokens,
                            ))
                            .font(typography::font(FontSize::Small, FontWeight::Regular))
                            .color(colors::TEXT_SECONDARY),
                        );
                    });
                });

                // Output tokens
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("  Output")
                            .font(typography::font(FontSize::Small, FontWeight::Regular))
                            .color(colors::TEXT_SECONDARY),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format_number_with_commas(
                                usage_data.total.output_tokens,
                            ))
                            .font(typography::font(FontSize::Small, FontWeight::Regular))
                            .color(colors::TEXT_SECONDARY),
                        );
                    });
                });

                // Cache read tokens (if non-zero)
                if usage_data.total.cache_read_tokens > 0 {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("  Cache Read")
                                .font(typography::font(FontSize::Small, FontWeight::Regular))
                                .color(colors::TEXT_MUTED),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                egui::RichText::new(format_number_with_commas(
                                    usage_data.total.cache_read_tokens,
                                ))
                                .font(typography::font(FontSize::Small, FontWeight::Regular))
                                .color(colors::TEXT_MUTED),
                            );
                        });
                    });
                }

                // Cache creation tokens (if non-zero)
                if usage_data.total.cache_creation_tokens > 0 {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("  Cache Creation")
                                .font(typography::font(FontSize::Small, FontWeight::Regular))
                                .color(colors::TEXT_MUTED),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                egui::RichText::new(format_number_with_commas(
                                    usage_data.total.cache_creation_tokens,
                                ))
                                .font(typography::font(FontSize::Small, FontWeight::Regular))
                                .color(colors::TEXT_MUTED),
                            );
                        });
                    });
                }

                // Thinking tokens (if non-zero)
                if usage_data.total.thinking_tokens > 0 {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("  Thinking")
                                .font(typography::font(FontSize::Small, FontWeight::Regular))
                                .color(colors::TEXT_MUTED),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                egui::RichText::new(format_number_with_commas(
                                    usage_data.total.thinking_tokens,
                                ))
                                .font(typography::font(FontSize::Small, FontWeight::Regular))
                                .color(colors::TEXT_MUTED),
                            );
                        });
                    });
                }
            });

        // Per-Phase Breakdown (only show if there are multiple phases)
        if usage_data.phases.len() > 1 {
            ui.add_space(spacing::SM);

            ui.label(
                egui::RichText::new("By Phase")
                    .font(typography::font(FontSize::Small, FontWeight::Medium))
                    .color(colors::TEXT_MUTED),
            );

            ui.add_space(spacing::XS);

            for phase_entry in &usage_data.phases {
                egui::Frame::none()
                    .fill(colors::SURFACE)
                    .rounding(rounding::SMALL)
                    .inner_margin(egui::Margin::symmetric(spacing::SM, spacing::XS))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(&phase_entry.phase)
                                    .font(typography::font(FontSize::Small, FontWeight::Medium))
                                    .color(colors::TEXT_PRIMARY),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(format_number_with_commas(
                                            phase_entry.usage.total_tokens(),
                                        ))
                                        .font(typography::font(
                                            FontSize::Small,
                                            FontWeight::Regular,
                                        ))
                                        .color(colors::TEXT_SECONDARY),
                                    );
                                },
                            );
                        });
                    });
                ui.add_space(spacing::XS);
            }
        }
    }

    /// Render output content from an OutputSource (extracted for reuse in both layouts).
    fn render_output_content(ui: &mut egui::Ui, output_source: &OutputSource) {
        match output_source {
            OutputSource::Live(lines) | OutputSource::Iteration(lines) => {
                for line in lines {
                    ui.label(
                        egui::RichText::new(line.trim())
                            .font(typography::mono(FontSize::Small))
                            .color(colors::TEXT_SECONDARY),
                    );
                }
            }
            OutputSource::StatusMessage(message) => {
                ui.label(
                    egui::RichText::new(message)
                        .font(typography::mono(FontSize::Small))
                        .color(colors::TEXT_DISABLED),
                );
            }
            OutputSource::NoData => {
                ui.label(
                    egui::RichText::new("No live output")
                        .font(typography::mono(FontSize::Small))
                        .color(colors::TEXT_DISABLED),
                );
            }
        }
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

/// Load the application window icon from the embedded PNG asset.
///
/// The icon is embedded at compile time from `assets/icon.png` and decoded
/// to RGBA pixel data for use with the window manager.
///
/// # Returns
///
/// `IconData` containing the RGBA pixels and dimensions for the window icon.
/// Returns `None` if the icon fails to load (graceful degradation to default).
fn load_window_icon() -> Option<Arc<egui::IconData>> {
    // Embed the icon PNG at compile time
    let icon_bytes = include_bytes!("../../../assets/icon.png");

    // Use eframe's built-in PNG decoder for proper icon loading
    match eframe::icon_data::from_png_bytes(icon_bytes) {
        Ok(icon_data) => Some(Arc::new(icon_data)),
        Err(_) => {
            // Graceful degradation: return None to use default icon
            None
        }
    }
}

/// Build the viewport configuration for the native window.
///
/// Configures a custom title bar that blends with the app's background color,
/// and sets the application window icon (US-006).
fn build_viewport() -> egui::ViewportBuilder {
    let mut builder = egui::ViewportBuilder::default()
        .with_title("autom8")
        .with_inner_size([DEFAULT_WIDTH, DEFAULT_HEIGHT])
        .with_min_inner_size([MIN_WIDTH, MIN_HEIGHT])
        .with_fullsize_content_view(true)
        .with_titlebar_shown(false)
        .with_title_shown(false);

    // Set the window/dock icon if loading succeeds
    if let Some(icon) = load_window_icon() {
        builder = builder.with_icon(icon);
    }

    builder
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
            // Initialize image loaders for embedded images (US-005)
            egui_extras::install_image_loaders(&cc.egui_ctx);
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
    use chrono::Utc;
    use std::path::PathBuf;

    // =========================================================================
    // Test Helpers
    // =========================================================================

    fn make_test_session_data(
        run: Option<crate::state::RunState>,
        live_output: Option<crate::state::LiveState>,
    ) -> SessionData {
        use crate::state::SessionMetadata;

        SessionData {
            project_name: "test-project".to_string(),
            metadata: SessionMetadata {
                session_id: "main".to_string(),
                worktree_path: PathBuf::from("/test/path"),
                branch_name: "test-branch".to_string(),
                created_at: Utc::now(),
                last_active_at: Utc::now(),
                is_running: true,
            },
            run,
            progress: None,
            load_error: None,
            is_main_session: true,
            is_stale: false,
            live_output,
            cached_user_stories: None,
        }
    }

    fn make_test_run_state(machine_state: MachineState) -> crate::state::RunState {
        crate::state::RunState {
            run_id: "test-run".to_string(),
            status: crate::state::RunStatus::Running,
            machine_state,
            spec_json_path: PathBuf::from("/test/spec.json"),
            spec_md_path: None,
            branch: "test-branch".to_string(),
            current_story: None,
            iteration: 1,
            review_iteration: 0,
            started_at: Utc::now(),
            finished_at: None,
            iterations: vec![],
            config: None,
            knowledge: Default::default(),
            pre_story_commit: None,
            session_id: Some("main".to_string()),
            total_usage: None,
            phase_usage: std::collections::HashMap::new(),
        }
    }

    // =========================================================================
    // App Initialization
    // =========================================================================

    #[test]
    fn test_app_initialization() {
        let app = Autom8App::new();
        assert_eq!(app.current_tab(), Tab::ActiveRuns);
        assert_eq!(app.tab_count(), 3); // ActiveRuns, Projects, Config

        let interval = Duration::from_millis(100);
        let app2 = Autom8App::with_refresh_interval(interval);
        assert_eq!(app2.refresh_interval(), interval);
    }

    // =========================================================================
    // Tab System
    // =========================================================================

    #[test]
    fn test_tab_open_close() {
        let mut app = Autom8App::new();

        // Open tabs
        assert!(app.open_run_detail_tab("run-1", "Run 1"));
        assert!(!app.open_run_detail_tab("run-1", "Run 1")); // No duplicate
        app.open_run_detail_tab("run-2", "Run 2");

        assert_eq!(app.tab_count(), 5); // 3 permanent + 2 dynamic
        assert_eq!(app.closable_tab_count(), 2);

        // Close dynamic tab
        assert!(app.close_tab(&TabId::RunDetail("run-1".to_string())));
        assert_eq!(app.closable_tab_count(), 1);

        // Can't close permanent tabs
        assert!(!app.close_tab(&TabId::ActiveRuns));
        assert!(!app.close_tab(&TabId::Config));
    }

    // =========================================================================
    // Run History
    // =========================================================================

    #[test]
    fn test_run_history_entry_creation() {
        use crate::state::{IterationRecord, IterationStatus, RunState, RunStatus};

        let mut run = RunState::new(PathBuf::from("test.json"), "feature/test".to_string());
        run.status = RunStatus::Completed;
        run.iterations.push(IterationRecord {
            number: 1,
            story_id: "US-001".to_string(),
            started_at: Utc::now(),
            finished_at: Some(Utc::now()),
            status: IterationStatus::Success,
            output_snippet: String::new(),
            work_summary: None,
            usage: None,
        });
        run.iterations.push(IterationRecord {
            number: 2,
            story_id: "US-002".to_string(),
            started_at: Utc::now(),
            finished_at: None,
            status: IterationStatus::Failed,
            output_snippet: String::new(),
            work_summary: None,
            usage: None,
        });

        let entry = RunHistoryEntry::from_run_state("test-project".to_string(), &run);
        assert_eq!(entry.branch, "feature/test");
        assert_eq!(entry.completed_stories, 1);
        assert_eq!(entry.total_stories, 2);
        assert_eq!(entry.story_count_text(), "1/2 stories");
        assert_eq!(entry.status_color(), colors::STATUS_SUCCESS);
    }

    // =========================================================================
    // Config Scope
    // =========================================================================

    #[test]
    fn test_config_scope_display() {
        assert_eq!(ConfigScope::Global.display_name(), "Global");
        assert_eq!(
            ConfigScope::Project("my-project".to_string()).display_name(),
            "my-project"
        );
        assert!(ConfigScope::Global.is_global());
        assert!(!ConfigScope::Project("test".to_string()).is_global());
    }

    // =========================================================================
    // Output Source Priority
    // =========================================================================

    #[test]
    fn test_output_source_fresh_live_preferred() {
        let mut live = crate::state::LiveState::new(MachineState::RunningClaude);
        live.output_lines = vec!["Line 1".to_string(), "Line 2".to_string()];

        let run = make_test_run_state(MachineState::RunningClaude);
        let session = make_test_session_data(Some(run), Some(live));
        let output = get_output_for_session(&session);

        assert!(matches!(output, OutputSource::Live(_)));
        if let OutputSource::Live(lines) = output {
            assert_eq!(lines.len(), 2);
        }
    }

    #[test]
    fn test_output_source_no_live_returns_no_data() {
        let run = make_test_run_state(MachineState::RunningClaude);
        let session = make_test_session_data(Some(run), None);
        let output = get_output_for_session(&session);

        assert!(matches!(output, OutputSource::NoData));
    }

    #[test]
    fn test_output_source_enum_variants() {
        let live = OutputSource::Live(vec!["test".to_string()]);
        let iter = OutputSource::Iteration(vec!["test".to_string()]);
        let status = OutputSource::StatusMessage("test".to_string());
        let no_data = OutputSource::NoData;

        assert_ne!(live, iter);
        assert_ne!(status.clone(), no_data.clone());
        assert_eq!(status, OutputSource::StatusMessage("test".to_string()));
    }

    /// US-004: Tests that iteration output is preserved when live output is empty.
    /// This ensures "Waiting for output..." doesn't replace valid previous output.
    #[test]
    fn test_iteration_output_preserved_when_live_empty() {
        use crate::state::{IterationRecord, IterationStatus, LiveState};

        // Create a run state with RunningClaude state and iteration with output
        let mut run = make_test_run_state(MachineState::RunningClaude);
        run.iterations.push(IterationRecord {
            number: 1,
            story_id: "US-001".to_string(),
            started_at: Utc::now(),
            finished_at: None,
            status: IterationStatus::Running,
            output_snippet: "Previous iteration output\nLine 2\nLine 3".to_string(),
            work_summary: None,
            usage: None,
        });

        // Create live output with EMPTY output_lines (new invocation just started)
        let live = LiveState {
            output_lines: vec![], // Empty - this is the bug scenario
            updated_at: Utc::now(),
            machine_state: MachineState::RunningClaude,
            last_heartbeat: Utc::now(),
        };

        let session = make_test_session_data(Some(run), Some(live));

        let output = get_output_for_session(&session);

        // Should fall through to iteration output, NOT return "Waiting for output..."
        match output {
            OutputSource::Iteration(lines) => {
                assert!(!lines.is_empty());
                assert!(lines
                    .iter()
                    .any(|l| l.contains("Previous iteration output")));
            }
            OutputSource::StatusMessage(msg) => {
                // This is the bug we're fixing - it should NOT show "Waiting for output..."
                panic!(
                    "Bug: Should have shown iteration output, not status message: {}",
                    msg
                );
            }
            other => panic!("Unexpected output source: {:?}", other),
        }
    }

    /// US-004: When there's no iteration output AND no live output, show status message.
    #[test]
    fn test_waiting_shown_only_when_no_output() {
        use crate::state::LiveState;

        // Create a run state with RunningClaude state but NO iterations
        let run = make_test_run_state(MachineState::RunningClaude);

        // Create live output with empty output_lines
        let live = LiveState {
            output_lines: vec![],
            updated_at: Utc::now(),
            machine_state: MachineState::RunningClaude,
            last_heartbeat: Utc::now(),
        };

        let session = make_test_session_data(Some(run), Some(live));

        let output = get_output_for_session(&session);

        // With no iteration output and no live output, should show status message
        match output {
            OutputSource::StatusMessage(msg) => {
                assert_eq!(msg, "Waiting for output...");
            }
            other => panic!("Expected StatusMessage, got {:?}", other),
        }
    }

    // =========================================================================
    // format_number_with_commas Tests
    // =========================================================================

    #[test]
    fn test_format_number_with_commas_zero() {
        assert_eq!(format_number_with_commas(0), "0");
    }

    #[test]
    fn test_format_number_with_commas_small_numbers() {
        assert_eq!(format_number_with_commas(1), "1");
        assert_eq!(format_number_with_commas(12), "12");
        assert_eq!(format_number_with_commas(123), "123");
        assert_eq!(format_number_with_commas(999), "999");
    }

    #[test]
    fn test_format_number_with_commas_thousands() {
        assert_eq!(format_number_with_commas(1000), "1,000");
        assert_eq!(format_number_with_commas(1234), "1,234");
        assert_eq!(format_number_with_commas(9999), "9,999");
    }

    #[test]
    fn test_format_number_with_commas_millions() {
        assert_eq!(format_number_with_commas(1000000), "1,000,000");
        assert_eq!(format_number_with_commas(1234567), "1,234,567");
        assert_eq!(format_number_with_commas(9999999), "9,999,999");
    }

    #[test]
    fn test_format_number_with_commas_large_numbers() {
        assert_eq!(format_number_with_commas(1000000000), "1,000,000,000");
        assert_eq!(format_number_with_commas(123456789012), "123,456,789,012");
    }

    #[test]
    fn test_format_number_with_commas_boundary_values() {
        // Test values at boundaries between thousand groups
        assert_eq!(format_number_with_commas(100), "100");
        assert_eq!(format_number_with_commas(1000), "1,000");
        assert_eq!(format_number_with_commas(10000), "10,000");
        assert_eq!(format_number_with_commas(100000), "100,000");
    }
}
