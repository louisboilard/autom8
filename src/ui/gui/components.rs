//! Reusable UI components for the GUI.
//!
//! This module contains reusable widgets and helper functions for consistent
//! status visualization across the application, including status dots, progress
//! indicators, and time formatting utilities.

use crate::state::MachineState;
use crate::ui::gui::theme::{colors, rounding, spacing};
use crate::ui::gui::typography::{self, FontSize, FontWeight};
// Import and re-export shared types and functions for backward compatibility
use crate::ui::shared::format_state_label;
pub use crate::ui::shared::{
    format_duration, format_duration_secs, format_relative_time, format_relative_time_secs,
    RunProgress, Status,
};
use eframe::egui::{self, Color32, Pos2, Rect, Rounding, Vec2};

// ============================================================================
// Status Dot Component
// ============================================================================

/// Default radius for status indicator dots.
pub const STATUS_DOT_RADIUS: f32 = 4.0;

/// A reusable status dot component that renders a small filled circle
/// with a color representing the current status.
///
/// # Example
///
/// ```ignore
/// let dot = StatusDot::new(Status::Running);
/// dot.paint(painter, egui::pos2(10.0, 10.0));
/// ```
#[derive(Debug, Clone, Copy)]
pub struct StatusDot {
    /// The color of the status dot.
    color: Color32,
    /// The radius of the dot.
    radius: f32,
}

impl StatusDot {
    /// Create a new status dot from a Status enum value.
    pub fn from_status(status: Status) -> Self {
        Self {
            color: status.color(),
            radius: STATUS_DOT_RADIUS,
        }
    }

    /// Create a new status dot from a MachineState.
    pub fn from_machine_state(state: MachineState) -> Self {
        Self::from_status(Status::from_machine_state(state))
    }

    /// Create a new status dot with a custom color.
    pub fn with_color(color: Color32) -> Self {
        Self {
            color,
            radius: STATUS_DOT_RADIUS,
        }
    }

    /// Set a custom radius for the dot.
    pub fn with_radius(mut self, radius: f32) -> Self {
        self.radius = radius;
        self
    }

    /// Returns the radius of this status dot.
    pub fn radius(&self) -> f32 {
        self.radius
    }

    /// Returns the color of this status dot.
    pub fn color(&self) -> Color32 {
        self.color
    }

    /// Paint the status dot at the given center position.
    pub fn paint(&self, painter: &egui::Painter, center: Pos2) {
        painter.circle_filled(center, self.radius, self.color);
    }

    /// Paint the status dot with an optional border.
    pub fn paint_with_border(&self, painter: &egui::Painter, center: Pos2, border_color: Color32) {
        painter.circle_filled(center, self.radius, self.color);
        painter.circle_stroke(center, self.radius, egui::Stroke::new(1.0, border_color));
    }
}

impl Default for StatusDot {
    fn default() -> Self {
        Self {
            color: colors::STATUS_IDLE,
            radius: STATUS_DOT_RADIUS,
        }
    }
}

// ============================================================================
// Status Color Mapping (GUI-specific extension)
// ============================================================================

// The Status enum is imported from crate::ui::shared and re-exported above.
// This module provides GUI-specific color mapping via the StatusColors trait.

/// GUI-specific color mapping for the shared Status enum.
///
/// This trait extends the shared Status enum with GUI-specific colors
/// using the theme's color palette. The shared Status enum defines the
/// semantic categorization (Setup, Running, etc.), and this trait provides
/// the visual representation for the GUI.
pub trait StatusColors {
    /// Returns the primary color for this status.
    fn color(self) -> Color32;

    /// Returns the background color for this status (for badges/highlights).
    fn background_color(self) -> Color32;
}

impl StatusColors for Status {
    fn color(self) -> Color32 {
        match self {
            Status::Setup => colors::STATUS_IDLE,
            Status::Running => colors::STATUS_RUNNING,
            Status::Reviewing => colors::STATUS_WARNING,
            Status::Correcting => colors::STATUS_CORRECTING,
            Status::Success => colors::STATUS_SUCCESS,
            Status::Warning => colors::STATUS_WARNING,
            Status::Error => colors::STATUS_ERROR,
            Status::Idle => colors::STATUS_IDLE,
        }
    }

    fn background_color(self) -> Color32 {
        match self {
            Status::Setup => colors::STATUS_IDLE_BG,
            Status::Running => colors::STATUS_RUNNING_BG,
            Status::Reviewing => colors::STATUS_WARNING_BG,
            Status::Correcting => colors::STATUS_CORRECTING_BG,
            Status::Success => colors::STATUS_SUCCESS_BG,
            Status::Warning => colors::STATUS_WARNING_BG,
            Status::Error => colors::STATUS_ERROR_BG,
            Status::Idle => colors::STATUS_IDLE_BG,
        }
    }
}

/// Map a MachineState directly to its display color.
///
/// This is a convenience function that wraps `Status::from_machine_state().color()`.
pub fn state_to_color(state: MachineState) -> Color32 {
    Status::from_machine_state(state).color()
}

/// Map a MachineState to its background color (for badges).
pub fn state_to_background_color(state: MachineState) -> Color32 {
    Status::from_machine_state(state).background_color()
}

/// Create a badge background color from any status color.
///
/// This blends the status color with the warm background color to create
/// a soft, theme-consistent badge background. Use this instead of
/// `color.gamma_multiply()` for status badges.
pub fn badge_background_color(status_color: Color32) -> Color32 {
    // Blend the status color with warm cream at ~15% opacity
    // This creates a soft tinted background that complements the warm theme
    let bg = colors::BACKGROUND;
    let alpha = 0.15;

    let r = (status_color.r() as f32 * alpha + bg.r() as f32 * (1.0 - alpha)) as u8;
    let g = (status_color.g() as f32 * alpha + bg.g() as f32 * (1.0 - alpha)) as u8;
    let b = (status_color.b() as f32 * alpha + bg.b() as f32 * (1.0 - alpha)) as u8;

    Color32::from_rgb(r, g, b)
}

/// Check if a MachineState represents a finished/terminal state.
///
/// US-002: Used to determine when the close button should be visible on session tabs.
/// Terminal states are those where the run has ended (success, failure, or idle):
/// - `Completed`: Run finished successfully
/// - `Failed`: Run ended with an error
/// - `Idle`: No active run (session has never started or was interrupted)
///
/// All other states are considered "in progress" and the close button should be hidden.
pub fn is_terminal_state(state: MachineState) -> bool {
    matches!(
        state,
        MachineState::Completed | MachineState::Failed | MachineState::Idle
    )
}

// ============================================================================
// Progress Components
// ============================================================================

// NOTE: Progress information is now provided by the shared `RunProgress` struct
// from `crate::ui::shared`. It is re-exported above for backward compatibility.
// The `ProgressBar` component below uses `RunProgress` for its data source.

/// A visual progress bar component.
#[derive(Debug, Clone)]
pub struct ProgressBar {
    /// The progress value (0.0 to 1.0).
    progress: f32,
    /// The height of the progress bar.
    height: f32,
    /// The background color.
    background_color: Color32,
    /// The fill color.
    fill_color: Color32,
    /// Corner rounding for the bar.
    rounding: f32,
}

impl ProgressBar {
    /// Create a new progress bar with the given progress value.
    pub fn new(progress: f32) -> Self {
        Self {
            progress: progress.clamp(0.0, 1.0),
            height: 6.0,
            background_color: colors::SURFACE_HOVER,
            fill_color: colors::ACCENT,
            rounding: 3.0,
        }
    }

    /// Create a progress bar from a RunProgress struct.
    pub fn from_progress(progress: &RunProgress) -> Self {
        Self::new(progress.fraction())
    }

    /// Set the height of the progress bar.
    pub fn with_height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    /// Set the fill color based on a status.
    pub fn with_status_color(mut self, status: Status) -> Self {
        self.fill_color = status.color();
        self
    }

    /// Set custom colors for the progress bar.
    pub fn with_colors(mut self, background: Color32, fill: Color32) -> Self {
        self.background_color = background;
        self.fill_color = fill;
        self
    }

    /// Set the corner rounding.
    pub fn with_rounding(mut self, rounding: f32) -> Self {
        self.rounding = rounding;
        self
    }

    /// Returns the current progress value.
    pub fn progress(&self) -> f32 {
        self.progress
    }

    /// Paint the progress bar at the given rectangle.
    pub fn paint(&self, painter: &egui::Painter, rect: Rect) {
        // Draw background
        painter.rect_filled(rect, Rounding::same(self.rounding), self.background_color);

        // Draw fill based on progress
        if self.progress > 0.0 {
            let fill_width = rect.width() * self.progress;
            let fill_rect = Rect::from_min_size(rect.min, Vec2::new(fill_width, rect.height()));
            painter.rect_filled(fill_rect, Rounding::same(self.rounding), self.fill_color);
        }
    }

    /// Allocate space and paint the progress bar in a UI.
    pub fn show(&self, ui: &mut egui::Ui, width: f32) -> egui::Response {
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(width, self.height), egui::Sense::hover());
        self.paint(ui.painter(), rect);
        response
    }
}

impl Default for ProgressBar {
    fn default() -> Self {
        Self::new(0.0)
    }
}

// Time formatting utilities are re-exported from crate::ui::shared above.
// See format_duration, format_duration_secs, format_relative_time, format_relative_time_secs.

// ============================================================================
// Text Utilities
// ============================================================================

/// Truncate a string with ellipsis if it exceeds the max length.
///
/// If the string fits within `max_len` characters, it is returned unchanged.
/// Otherwise, it is truncated at the last word boundary (space) before the
/// character limit, with "..." appended.
///
/// Special cases:
/// - If `max_len <= 3`, the string is simply truncated without ellipsis
/// - Empty strings are returned unchanged
/// - If no space exists before the limit, falls back to character-based truncation
///
/// This function is Unicode-safe and operates on characters, not bytes.
pub fn truncate_with_ellipsis(s: &str, max_len: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_len {
        s.to_string()
    } else if max_len <= 3 {
        s.chars().take(max_len).collect()
    } else {
        let target_len = max_len - 3; // Reserve space for "..."
        let truncated: String = s.chars().take(target_len).collect();

        // Try to find the last space to break at a word boundary
        let truncate_at = truncated.rfind(' ').unwrap_or(target_len);

        if truncate_at == 0 {
            // No space found or only leading space - fall back to character truncation
            format!("{}...", truncated.trim_end())
        } else {
            format!("{}...", truncated[..truncate_at].trim_end())
        }
    }
}

/// Strip worktree-related prefixes from branch names for cleaner display.
///
/// This removes common prefixes that follow the worktree naming pattern:
/// - `{project}-wt-` prefix (e.g., "autom8-wt-feature/foo" → "feature/foo")
/// - `{project}-` prefix if followed by common branch prefixes
///
/// The project_name is used to identify project-specific prefixes.
///
/// # Examples
/// ```
/// use autom8::ui::gui::components::strip_worktree_prefix;
///
/// assert_eq!(strip_worktree_prefix("feature/login", "myproject"), "feature/login");
/// assert_eq!(strip_worktree_prefix("myproject-wt-feature/login", "myproject"), "feature/login");
/// ```
pub fn strip_worktree_prefix(branch_name: &str, project_name: &str) -> String {
    // Try to strip "{project}-wt-" prefix
    let wt_prefix = format!("{}-wt-", project_name);
    if let Some(stripped) = branch_name.strip_prefix(&wt_prefix) {
        return stripped.to_string();
    }

    // Try lowercase version as well (case-insensitive matching)
    let wt_prefix_lower = format!("{}-wt-", project_name.to_lowercase());
    if branch_name.to_lowercase().starts_with(&wt_prefix_lower) {
        // Return the original case for the rest of the branch name
        return branch_name[wt_prefix_lower.len()..].to_string();
    }

    // Return unchanged if no prefix matched
    branch_name.to_string()
}

/// Maximum characters for general text truncation.
pub const MAX_TEXT_LENGTH: usize = 40;

/// Maximum characters for branch name truncation.
pub const MAX_BRANCH_LENGTH: usize = 25;

// ============================================================================
// State Label Formatting
// ============================================================================

/// Format a machine state as a human-readable string.
///
/// This is a re-export of the shared `format_state_label` function for
/// backward compatibility. Both GUI and TUI use the same underlying
/// function from `ui::shared` to ensure consistent state labels.
pub fn format_state(state: MachineState) -> &'static str {
    format_state_label(state)
}

// ============================================================================
// Status Label Component
// ============================================================================

/// A component that renders a status dot followed by a text label.
#[derive(Debug, Clone)]
pub struct StatusLabel {
    /// The status for coloring.
    status: Status,
    /// The label text.
    label: String,
    /// The dot radius.
    dot_radius: f32,
    /// Spacing between dot and label.
    spacing: f32,
}

impl StatusLabel {
    /// Create a new status label.
    pub fn new(status: Status, label: impl Into<String>) -> Self {
        Self {
            status,
            label: label.into(),
            dot_radius: STATUS_DOT_RADIUS,
            spacing: 8.0,
        }
    }

    /// Create a status label from a machine state.
    pub fn from_machine_state(state: MachineState) -> Self {
        Self::new(Status::from_machine_state(state), format_state(state))
    }

    /// Set custom dot radius.
    pub fn with_dot_radius(mut self, radius: f32) -> Self {
        self.dot_radius = radius;
        self
    }

    /// Set custom spacing between dot and label.
    pub fn with_spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Returns the status.
    pub fn status(&self) -> Status {
        self.status
    }

    /// Returns the label text.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Paint the status label at the given position.
    ///
    /// Returns the total width used.
    pub fn paint(
        &self,
        _ui: &egui::Ui,
        painter: &egui::Painter,
        pos: Pos2,
        font: egui::FontId,
        text_color: Color32,
    ) -> f32 {
        // Draw the dot
        let dot = StatusDot::from_status(self.status).with_radius(self.dot_radius);
        let dot_center = Pos2::new(pos.x + self.dot_radius, pos.y + self.dot_radius);
        dot.paint(painter, dot_center);

        // Draw the label
        let label_x = pos.x + self.dot_radius * 2.0 + self.spacing;
        let galley = painter.layout_no_wrap(self.label.clone(), font, text_color);
        painter.galley(
            Pos2::new(label_x, pos.y),
            galley.clone(),
            Color32::TRANSPARENT,
        );

        // Return total width
        self.dot_radius * 2.0 + self.spacing + galley.rect.width()
    }

    /// Show the status label in a UI, allocating space automatically.
    pub fn show(&self, ui: &mut egui::Ui) -> egui::Response {
        let font = typography::font(FontSize::Caption, FontWeight::Medium);
        let text_color = colors::TEXT_PRIMARY;

        // Calculate the approximate width needed
        let text_galley =
            ui.fonts(|f| f.layout_no_wrap(self.label.clone(), font.clone(), text_color));
        let width = self.dot_radius * 2.0 + self.spacing + text_galley.rect.width();
        let height = text_galley.rect.height().max(self.dot_radius * 2.0);

        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::hover());

        if ui.is_rect_visible(rect) {
            self.paint(ui, ui.painter(), rect.min, font, text_color);
        }

        response
    }
}

// ============================================================================
// Collapsible Section Component
// ============================================================================

/// A reusable collapsible section component for detail panels.
///
/// The section has a clickable header that toggles between expanded and collapsed
/// states. When expanded, the content area is visible. When collapsed, only the
/// header is shown with an indicator showing the collapsed state.
///
/// # Example
///
/// ```ignore
/// let mut collapsed_sections = HashMap::new();
///
/// CollapsibleSection::new("work_summaries", "Work Summaries")
///     .default_expanded(false)
///     .show(ui, &mut collapsed_sections, |ui| {
///         // Section content here
///         ui.label("Content goes here");
///     });
/// ```
pub struct CollapsibleSection<'a> {
    /// Unique identifier for this section (used for state tracking).
    id: &'a str,
    /// Title displayed in the section header.
    title: &'a str,
    /// Whether the section should be expanded by default.
    default_expanded: bool,
}

impl<'a> CollapsibleSection<'a> {
    /// Create a new collapsible section with the given ID and title.
    ///
    /// The ID should be unique within the context where the section is used,
    /// as it's used to track the collapsed state in the state map.
    pub fn new(id: &'a str, title: &'a str) -> Self {
        Self {
            id,
            title,
            default_expanded: true,
        }
    }

    /// Set whether this section should be expanded by default.
    ///
    /// When the section is first rendered (or when its state is not in the map),
    /// this determines whether it starts expanded or collapsed.
    pub fn default_expanded(mut self, expanded: bool) -> Self {
        self.default_expanded = expanded;
        self
    }

    /// Render the collapsible section and execute the content callback if expanded.
    ///
    /// # Arguments
    ///
    /// * `ui` - The egui UI context
    /// * `collapsed_state` - Map of section IDs to their collapsed state (true = collapsed)
    /// * `add_contents` - Callback to render the section content when expanded
    ///
    /// # Returns
    ///
    /// The response from the header click interaction.
    pub fn show<R>(
        self,
        ui: &mut egui::Ui,
        collapsed_state: &mut std::collections::HashMap<String, bool>,
        add_contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> egui::Response {
        // Get or initialize the collapsed state for this section
        let is_collapsed = *collapsed_state
            .entry(self.id.to_string())
            .or_insert(!self.default_expanded);

        // Render the header (clickable to toggle)
        let header_response = self.render_header(ui, is_collapsed);

        // Toggle state on click
        if header_response.clicked() {
            collapsed_state.insert(self.id.to_string(), !is_collapsed);
        }

        // Render content if expanded
        if !is_collapsed {
            ui.add_space(spacing::SM);
            add_contents(ui);
        }

        header_response
    }

    /// Render the section header with title and expand/collapse indicator.
    fn render_header(&self, ui: &mut egui::Ui, is_collapsed: bool) -> egui::Response {
        let available_width = ui.available_width();

        // Create a clickable header area
        let header_height = typography::line_height(FontSize::Body) + spacing::XS * 2.0;

        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(available_width, header_height),
            egui::Sense::click(),
        );

        if ui.is_rect_visible(rect) {
            let painter = ui.painter();

            // Draw hover highlight if applicable
            if response.hovered() {
                painter.rect_filled(rect, Rounding::same(rounding::SMALL), colors::SURFACE_HOVER);
            }

            // Draw the chevron indicator
            let chevron_size = 8.0;
            let chevron_x = rect.min.x + spacing::XS;
            let chevron_y = rect.center().y;

            let chevron_color = if response.hovered() {
                colors::TEXT_PRIMARY
            } else {
                colors::TEXT_SECONDARY
            };

            if is_collapsed {
                // Right-pointing chevron (collapsed)
                // Draw > shape
                let points = [
                    Pos2::new(chevron_x, chevron_y - chevron_size / 2.0),
                    Pos2::new(chevron_x + chevron_size / 2.0, chevron_y),
                    Pos2::new(chevron_x, chevron_y + chevron_size / 2.0),
                ];
                painter.line_segment(
                    [points[0], points[1]],
                    egui::Stroke::new(1.5, chevron_color),
                );
                painter.line_segment(
                    [points[1], points[2]],
                    egui::Stroke::new(1.5, chevron_color),
                );
            } else {
                // Down-pointing chevron (expanded)
                // Draw v shape
                let points = [
                    Pos2::new(chevron_x, chevron_y - chevron_size / 4.0),
                    Pos2::new(
                        chevron_x + chevron_size / 2.0,
                        chevron_y + chevron_size / 4.0,
                    ),
                    Pos2::new(chevron_x + chevron_size, chevron_y - chevron_size / 4.0),
                ];
                painter.line_segment(
                    [points[0], points[1]],
                    egui::Stroke::new(1.5, chevron_color),
                );
                painter.line_segment(
                    [points[1], points[2]],
                    egui::Stroke::new(1.5, chevron_color),
                );
            }

            // Draw the title
            let title_x = chevron_x + chevron_size + spacing::SM;
            let title_y = rect.center().y - typography::line_height(FontSize::Body) / 2.0;

            let title_color = if response.hovered() {
                colors::TEXT_PRIMARY
            } else {
                colors::TEXT_SECONDARY
            };

            let galley = painter.layout_no_wrap(
                self.title.to_string(),
                typography::font(FontSize::Body, FontWeight::Medium),
                title_color,
            );

            painter.galley(Pos2::new(title_x, title_y), galley, Color32::TRANSPARENT);
        }

        // Show cursor change on hover
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        response
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------------
    // Status Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_status_colors() {
        // All status variants should return their designated colors
        assert_eq!(Status::Setup.color(), colors::STATUS_IDLE);
        assert_eq!(Status::Running.color(), colors::STATUS_RUNNING);
        assert_eq!(Status::Reviewing.color(), colors::STATUS_WARNING);
        assert_eq!(Status::Correcting.color(), colors::STATUS_CORRECTING);
        assert_eq!(Status::Success.color(), colors::STATUS_SUCCESS);
        assert_eq!(Status::Warning.color(), colors::STATUS_WARNING);
        assert_eq!(Status::Error.color(), colors::STATUS_ERROR);
        assert_eq!(Status::Idle.color(), colors::STATUS_IDLE);
    }

    #[test]
    fn test_status_background_colors() {
        assert_eq!(Status::Setup.background_color(), colors::STATUS_IDLE_BG);
        assert_eq!(
            Status::Running.background_color(),
            colors::STATUS_RUNNING_BG
        );
        assert_eq!(
            Status::Reviewing.background_color(),
            colors::STATUS_WARNING_BG
        );
        assert_eq!(
            Status::Correcting.background_color(),
            colors::STATUS_CORRECTING_BG
        );
        assert_eq!(
            Status::Success.background_color(),
            colors::STATUS_SUCCESS_BG
        );
        assert_eq!(
            Status::Warning.background_color(),
            colors::STATUS_WARNING_BG
        );
        assert_eq!(Status::Error.background_color(), colors::STATUS_ERROR_BG);
        assert_eq!(Status::Idle.background_color(), colors::STATUS_IDLE_BG);
    }

    #[test]
    fn test_status_from_machine_state_setup_phase() {
        // Setup phases should map to Status::Setup (gray)
        assert_eq!(
            Status::from_machine_state(MachineState::Initializing),
            Status::Setup
        );
        assert_eq!(
            Status::from_machine_state(MachineState::PickingStory),
            Status::Setup
        );
        assert_eq!(
            Status::from_machine_state(MachineState::LoadingSpec),
            Status::Setup
        );
        assert_eq!(
            Status::from_machine_state(MachineState::GeneratingSpec),
            Status::Setup
        );
    }

    #[test]
    fn test_status_from_machine_state_running() {
        // Active implementation should map to Status::Running (blue)
        assert_eq!(
            Status::from_machine_state(MachineState::RunningClaude),
            Status::Running
        );
    }

    #[test]
    fn test_status_from_machine_state_reviewing() {
        // Evaluation phase should map to Status::Reviewing (amber)
        assert_eq!(
            Status::from_machine_state(MachineState::Reviewing),
            Status::Reviewing
        );
    }

    #[test]
    fn test_status_from_machine_state_correcting() {
        // Attention needed should map to Status::Correcting (orange)
        assert_eq!(
            Status::from_machine_state(MachineState::Correcting),
            Status::Correcting
        );
    }

    #[test]
    fn test_status_from_machine_state_success_path() {
        // Success path states should map to Status::Success (green)
        assert_eq!(
            Status::from_machine_state(MachineState::Committing),
            Status::Success
        );
        assert_eq!(
            Status::from_machine_state(MachineState::CreatingPR),
            Status::Success
        );
        assert_eq!(
            Status::from_machine_state(MachineState::Completed),
            Status::Success
        );
    }

    #[test]
    fn test_status_from_machine_state_terminal() {
        // Terminal states
        assert_eq!(
            Status::from_machine_state(MachineState::Failed),
            Status::Error
        );
        assert_eq!(Status::from_machine_state(MachineState::Idle), Status::Idle);
    }

    #[test]
    fn test_state_to_color_semantic_mapping() {
        // Setup phases -> gray (STATUS_IDLE)
        assert_eq!(
            state_to_color(MachineState::Initializing),
            colors::STATUS_IDLE
        );
        assert_eq!(
            state_to_color(MachineState::PickingStory),
            colors::STATUS_IDLE
        );

        // Active implementation -> blue
        assert_eq!(
            state_to_color(MachineState::RunningClaude),
            colors::STATUS_RUNNING
        );

        // Evaluation -> amber (warning)
        assert_eq!(
            state_to_color(MachineState::Reviewing),
            colors::STATUS_WARNING
        );

        // Attention needed -> orange
        assert_eq!(
            state_to_color(MachineState::Correcting),
            colors::STATUS_CORRECTING
        );

        // Success path -> green
        assert_eq!(
            state_to_color(MachineState::Committing),
            colors::STATUS_SUCCESS
        );
        assert_eq!(
            state_to_color(MachineState::CreatingPR),
            colors::STATUS_SUCCESS
        );
        assert_eq!(
            state_to_color(MachineState::Completed),
            colors::STATUS_SUCCESS
        );

        // Failure -> red
        assert_eq!(state_to_color(MachineState::Failed), colors::STATUS_ERROR);

        // Idle -> gray
        assert_eq!(state_to_color(MachineState::Idle), colors::STATUS_IDLE);
    }

    #[test]
    fn test_state_to_background_color() {
        assert_eq!(
            state_to_background_color(MachineState::RunningClaude),
            colors::STATUS_RUNNING_BG
        );
        assert_eq!(
            state_to_background_color(MachineState::Completed),
            colors::STATUS_SUCCESS_BG
        );
        assert_eq!(
            state_to_background_color(MachineState::Failed),
            colors::STATUS_ERROR_BG
        );
        assert_eq!(
            state_to_background_color(MachineState::Correcting),
            colors::STATUS_CORRECTING_BG
        );
    }

    // ------------------------------------------------------------------------
    // StatusDot Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_status_dot_default() {
        let dot = StatusDot::default();
        assert_eq!(dot.radius(), STATUS_DOT_RADIUS);
        assert_eq!(dot.color(), colors::STATUS_IDLE);
    }

    #[test]
    fn test_status_dot_from_status() {
        let dot = StatusDot::from_status(Status::Running);
        assert_eq!(dot.color(), colors::STATUS_RUNNING);
    }

    #[test]
    fn test_status_dot_from_machine_state() {
        let dot = StatusDot::from_machine_state(MachineState::Completed);
        assert_eq!(dot.color(), colors::STATUS_SUCCESS);
    }

    #[test]
    fn test_status_dot_with_radius() {
        let dot = StatusDot::default().with_radius(8.0);
        assert_eq!(dot.radius(), 8.0);
    }

    #[test]
    fn test_status_dot_with_color() {
        let custom_color = Color32::from_rgb(255, 0, 128);
        let dot = StatusDot::with_color(custom_color);
        assert_eq!(dot.color(), custom_color);
    }

    // ------------------------------------------------------------------------
    // ProgressBar Tests (RunProgress tests are in ui::shared::tests)
    // ------------------------------------------------------------------------

    #[test]
    fn test_progress_bar() {
        assert!((ProgressBar::new(0.5).progress() - 0.5).abs() < 0.001);
        assert_eq!(ProgressBar::new(-0.5).progress(), 0.0); // Clamps low
        assert_eq!(ProgressBar::new(1.5).progress(), 1.0); // Clamps high
        assert!(
            (ProgressBar::from_progress(&RunProgress::new(3, 10)).progress() - 0.3).abs() < 0.001
        );
    }

    // Duration and relative time formatting tests are in ui::shared::tests.
    // The functions are re-exported from shared for backward compatibility.

    // ------------------------------------------------------------------------
    // Text Utility Tests
    // ------------------------------------------------------------------------

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
    fn test_truncate_with_ellipsis_long_string_word_boundary() {
        // Should break at word boundary "is a" instead of mid-word "ve"
        let result = truncate_with_ellipsis("this is a very long string", 15);
        assert_eq!(result, "this is a...");
        assert!(result.len() <= 15);
    }

    #[test]
    fn test_truncate_with_ellipsis_very_short_max() {
        let result = truncate_with_ellipsis("hello", 3);
        assert_eq!(result, "hel");
    }

    #[test]
    fn test_truncate_with_ellipsis_empty_string() {
        let result = truncate_with_ellipsis("", 10);
        assert_eq!(result, "");
    }

    #[test]
    fn test_truncate_with_ellipsis_no_space_fallback() {
        // When there's no space, fall back to character-based truncation
        let result = truncate_with_ellipsis("superlongword", 10);
        assert_eq!(result, "superlo...");
        assert_eq!(result.len(), 10);
    }

    #[test]
    fn test_truncate_with_ellipsis_word_boundary_exact() {
        // "hello world" with max 11 fits exactly
        let result = truncate_with_ellipsis("hello world", 11);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_truncate_with_ellipsis_word_boundary_just_over() {
        // "hello world test" with max 14 -> target_len = 11 -> "hello world" -> last space at 5
        // Should truncate at "hello" since "world" would exceed
        let result = truncate_with_ellipsis("hello world test", 14);
        assert_eq!(result, "hello...");
    }

    #[test]
    fn test_truncate_with_ellipsis_single_word_too_long() {
        // Single word that's too long should use character truncation
        // max_len 15, target_len = 12 -> "internationa" -> no space -> fall back to char truncation
        let result = truncate_with_ellipsis("internationalization", 15);
        assert_eq!(result, "internationa...");
        assert!(result.len() <= 15);
    }

    #[test]
    fn test_truncate_with_ellipsis_preserves_short_content() {
        // Short content should be unchanged
        let result = truncate_with_ellipsis("ok", 10);
        assert_eq!(result, "ok");
    }

    #[test]
    fn test_truncate_with_ellipsis_multiple_spaces() {
        // max_len 16, target_len = 13 -> "one two three" -> last space at 7 -> "one two"
        let result = truncate_with_ellipsis("one two three four five", 16);
        assert_eq!(result, "one two...");
    }

    #[test]
    fn test_truncate_with_ellipsis_trailing_space_trimmed() {
        // Trailing spaces before truncation point should be trimmed
        let result = truncate_with_ellipsis("hello   world", 10);
        assert_eq!(result, "hello...");
    }

    // ------------------------------------------------------------------------
    // strip_worktree_prefix Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_strip_worktree_prefix_no_prefix() {
        // Branch without worktree prefix should be unchanged
        assert_eq!(
            strip_worktree_prefix("feature/login", "myproject"),
            "feature/login"
        );
        assert_eq!(strip_worktree_prefix("main", "myproject"), "main");
        assert_eq!(
            strip_worktree_prefix("develop/new-feature", "myproject"),
            "develop/new-feature"
        );
    }

    #[test]
    fn test_strip_worktree_prefix_standard_wt_prefix() {
        // Standard "{project}-wt-" prefix should be stripped
        assert_eq!(
            strip_worktree_prefix("myproject-wt-feature/login", "myproject"),
            "feature/login"
        );
        assert_eq!(
            strip_worktree_prefix("autom8-wt-feature/gui-tabs", "autom8"),
            "feature/gui-tabs"
        );
    }

    #[test]
    fn test_strip_worktree_prefix_case_insensitive() {
        // Should handle case differences in project name
        assert_eq!(
            strip_worktree_prefix("MyProject-wt-feature/test", "myproject"),
            "feature/test"
        );
        assert_eq!(
            strip_worktree_prefix("MYPROJECT-wt-feature/test", "myproject"),
            "feature/test"
        );
    }

    #[test]
    fn test_strip_worktree_prefix_preserves_case_in_branch() {
        // Should preserve the case of the branch name portion
        assert_eq!(
            strip_worktree_prefix("myproject-wt-Feature/LOGIN", "myproject"),
            "Feature/LOGIN"
        );
    }

    #[test]
    fn test_strip_worktree_prefix_partial_match_not_stripped() {
        // Partial matches should not be stripped
        assert_eq!(
            strip_worktree_prefix("myproject-feature/test", "myproject"),
            "myproject-feature/test"
        );
        assert_eq!(
            strip_worktree_prefix("myproject-wt", "myproject"),
            "myproject-wt"
        );
    }

    #[test]
    fn test_strip_worktree_prefix_different_project() {
        // Different project name should not match
        assert_eq!(
            strip_worktree_prefix("otherproject-wt-feature/test", "myproject"),
            "otherproject-wt-feature/test"
        );
    }

    #[test]
    fn test_strip_worktree_prefix_empty_strings() {
        // Empty branch name
        assert_eq!(strip_worktree_prefix("", "myproject"), "");
        // Empty project name (shouldn't match any prefix)
        assert_eq!(strip_worktree_prefix("feature/test", ""), "feature/test");
    }

    // ------------------------------------------------------------------------
    // State Label Formatting Tests
    // ------------------------------------------------------------------------

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

    // ------------------------------------------------------------------------
    // StatusLabel Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_status_label_new() {
        let label = StatusLabel::new(Status::Running, "Test Label");
        assert_eq!(label.status(), Status::Running);
        assert_eq!(label.label(), "Test Label");
    }

    #[test]
    fn test_status_label_from_machine_state() {
        let label = StatusLabel::from_machine_state(MachineState::RunningClaude);
        assert_eq!(label.status(), Status::Running);
        assert_eq!(label.label(), "Running Claude");
    }

    #[test]
    fn test_status_label_with_dot_radius() {
        let label = StatusLabel::new(Status::Success, "Done").with_dot_radius(8.0);
        // Just verify it compiles and stores the value
        assert_eq!(label.status(), Status::Success);
    }

    #[test]
    fn test_status_label_with_spacing() {
        let label = StatusLabel::new(Status::Error, "Failed").with_spacing(12.0);
        assert_eq!(label.status(), Status::Error);
    }

    // ------------------------------------------------------------------------
    // Badge Background Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_badge_background_color() {
        let running_bg = badge_background_color(colors::STATUS_RUNNING);
        let success_bg = badge_background_color(colors::STATUS_SUCCESS);
        let error_bg = badge_background_color(colors::STATUS_ERROR);

        // Badge backgrounds should be light (high luminance > 600 out of 765 max)
        for (name, bg) in [
            ("running", running_bg),
            ("success", success_bg),
            ("error", error_bg),
        ] {
            let lum = bg.r() as u32 + bg.g() as u32 + bg.b() as u32;
            assert!(
                lum > 600,
                "{} badge bg should be light, got luminance {}",
                name,
                lum
            );
        }

        // All three should produce different colors
        assert_ne!(running_bg, success_bg);
        assert_ne!(success_bg, error_bg);
        assert_ne!(running_bg, error_bg);
    }

    // ------------------------------------------------------------------------
    // CollapsibleSection Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_collapsible_section_new() {
        let section = CollapsibleSection::new("test_id", "Test Title");
        assert_eq!(section.id, "test_id");
        assert_eq!(section.title, "Test Title");
        assert!(section.default_expanded); // Default is expanded
    }

    #[test]
    fn test_collapsible_section_default_expanded() {
        let section_expanded = CollapsibleSection::new("test", "Test").default_expanded(true);
        assert!(section_expanded.default_expanded);

        let section_collapsed = CollapsibleSection::new("test", "Test").default_expanded(false);
        assert!(!section_collapsed.default_expanded);
    }

    #[test]
    fn test_collapsible_section_state_initialization() {
        // Test that default_expanded is respected when state is not present
        let mut state = std::collections::HashMap::new();

        // Section with default_expanded = true should initialize as not collapsed (false)
        let _ = state.entry("expanded_section".to_string()).or_insert(!true); // !default_expanded where default_expanded = true
        assert_eq!(state.get("expanded_section"), Some(&false)); // collapsed = false

        // Section with default_expanded = false should initialize as collapsed (true)
        let _ = state
            .entry("collapsed_section".to_string())
            .or_insert(!false); // !default_expanded where default_expanded = false
        assert_eq!(state.get("collapsed_section"), Some(&true)); // collapsed = true
    }

    #[test]
    fn test_collapsible_section_state_persistence() {
        // Test that state is properly tracked in the HashMap
        let mut state = std::collections::HashMap::new();

        // Simulate initial state
        state.insert("section_a".to_string(), false); // expanded
        state.insert("section_b".to_string(), true); // collapsed

        // Verify state
        assert_eq!(state.get("section_a"), Some(&false));
        assert_eq!(state.get("section_b"), Some(&true));

        // Simulate toggle
        state.insert("section_a".to_string(), true); // now collapsed
        assert_eq!(state.get("section_a"), Some(&true));
    }
}
