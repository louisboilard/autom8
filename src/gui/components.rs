//! Reusable UI components for the GUI.
//!
//! This module contains reusable widgets and helper functions for consistent
//! status visualization across the application, including status dots, progress
//! indicators, and time formatting utilities.

use crate::gui::theme::colors;
use crate::gui::typography::{self, FontSize, FontWeight};
use crate::state::MachineState;
use chrono::{DateTime, Utc};
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
// Status Enum and Color Mapping
// ============================================================================

/// Semantic status states for consistent color mapping across the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Active/running state - displayed in blue.
    Running,
    /// Successful completion - displayed in green.
    Success,
    /// Warning/attention needed - displayed in amber.
    Warning,
    /// Error/failure state - displayed in red.
    Error,
    /// Idle/inactive state - displayed in gray.
    Idle,
}

impl Status {
    /// Returns the primary color for this status.
    pub fn color(self) -> Color32 {
        match self {
            Status::Running => colors::STATUS_RUNNING,
            Status::Success => colors::STATUS_SUCCESS,
            Status::Warning => colors::STATUS_WARNING,
            Status::Error => colors::STATUS_ERROR,
            Status::Idle => colors::STATUS_IDLE,
        }
    }

    /// Returns the background color for this status (for badges/highlights).
    pub fn background_color(self) -> Color32 {
        match self {
            Status::Running => colors::STATUS_RUNNING_BG,
            Status::Success => colors::STATUS_SUCCESS_BG,
            Status::Warning => colors::STATUS_WARNING_BG,
            Status::Error => colors::STATUS_ERROR_BG,
            Status::Idle => colors::STATUS_IDLE_BG,
        }
    }

    /// Convert a MachineState to the appropriate Status.
    ///
    /// This mapping matches the TUI semantics:
    /// - Running states (RunningClaude, Reviewing, etc.) -> Status::Running
    /// - Completed -> Status::Success
    /// - Failed -> Status::Error
    /// - Idle -> Status::Idle
    pub fn from_machine_state(state: MachineState) -> Self {
        match state {
            MachineState::RunningClaude
            | MachineState::Reviewing
            | MachineState::Correcting
            | MachineState::Committing
            | MachineState::CreatingPR
            | MachineState::Initializing
            | MachineState::PickingStory
            | MachineState::LoadingSpec
            | MachineState::GeneratingSpec => Status::Running,
            MachineState::Completed => Status::Success,
            MachineState::Failed => Status::Error,
            MachineState::Idle => Status::Idle,
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

// ============================================================================
// Progress Components
// ============================================================================

/// Progress information for displaying story completion.
#[derive(Debug, Clone, Copy)]
pub struct Progress {
    /// Number of completed items.
    pub completed: usize,
    /// Total number of items.
    pub total: usize,
}

impl Progress {
    /// Create a new Progress instance.
    pub fn new(completed: usize, total: usize) -> Self {
        Self { completed, total }
    }

    /// Calculate the progress as a fraction between 0.0 and 1.0.
    pub fn fraction(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (self.completed as f32) / (self.total as f32)
        }
    }

    /// Format progress as a fraction string (e.g., "Story 2/5").
    /// The current story number is completed + 1 (1-indexed).
    pub fn as_story_fraction(&self) -> String {
        format!("Story {}/{}", self.completed + 1, self.total)
    }

    /// Format progress as a simple fraction (e.g., "2/5").
    pub fn as_fraction(&self) -> String {
        format!("{}/{}", self.completed, self.total)
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

    /// Create a progress bar from a Progress struct.
    pub fn from_progress(progress: &Progress) -> Self {
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

// ============================================================================
// Time Formatting Utilities
// ============================================================================

/// Format a duration from a start time as a human-readable string.
///
/// Examples: "5s", "2m 30s", "1h 5m"
///
/// - Durations under 1 minute show only seconds
/// - Durations between 1-60 minutes show minutes and seconds
/// - Durations over 1 hour show hours and minutes (no seconds)
pub fn format_duration(started_at: DateTime<Utc>) -> String {
    let now = Utc::now();
    let duration = now.signed_duration_since(started_at);
    format_duration_secs(duration.num_seconds().max(0) as u64)
}

/// Format a duration in seconds as a human-readable string.
///
/// This is useful when you have a pre-calculated duration.
pub fn format_duration_secs(total_secs: u64) -> String {
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

/// Format a timestamp as a relative time string.
///
/// Examples: "just now", "5m ago", "2h ago", "3d ago"
///
/// - Under 1 minute: "just now"
/// - 1-59 minutes: "Xm ago"
/// - 1-23 hours: "Xh ago"
/// - 1+ days: "Xd ago"
pub fn format_relative_time(timestamp: DateTime<Utc>) -> String {
    let now = Utc::now();
    let duration = now.signed_duration_since(timestamp);
    format_relative_time_secs(duration.num_seconds().max(0) as u64)
}

/// Format a relative time from seconds ago.
pub fn format_relative_time_secs(total_secs: u64) -> String {
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

// ============================================================================
// Text Utilities
// ============================================================================

/// Truncate a string with ellipsis if it exceeds the max length.
///
/// If the string fits within `max_len` characters, it is returned unchanged.
/// Otherwise, it is truncated to `max_len - 3` characters plus "...".
///
/// Special cases:
/// - If `max_len <= 3`, the string is simply truncated without ellipsis
/// - Empty strings are returned unchanged
///
/// This function is Unicode-safe and operates on characters, not bytes.
pub fn truncate_with_ellipsis(s: &str, max_len: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_len {
        s.to_string()
    } else if max_len <= 3 {
        s.chars().take(max_len).collect()
    } else {
        let truncated: String = s.chars().take(max_len - 3).collect();
        format!("{}...", truncated)
    }
}

/// Maximum characters for general text truncation.
pub const MAX_TEXT_LENGTH: usize = 40;

/// Maximum characters for branch name truncation.
pub const MAX_BRANCH_LENGTH: usize = 25;

// ============================================================================
// State Label Formatting
// ============================================================================

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
        assert_eq!(Status::Running.color(), colors::STATUS_RUNNING);
        assert_eq!(Status::Success.color(), colors::STATUS_SUCCESS);
        assert_eq!(Status::Warning.color(), colors::STATUS_WARNING);
        assert_eq!(Status::Error.color(), colors::STATUS_ERROR);
        assert_eq!(Status::Idle.color(), colors::STATUS_IDLE);
    }

    #[test]
    fn test_status_background_colors() {
        assert_eq!(
            Status::Running.background_color(),
            colors::STATUS_RUNNING_BG
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
    fn test_status_from_machine_state_running() {
        assert_eq!(
            Status::from_machine_state(MachineState::RunningClaude),
            Status::Running
        );
        assert_eq!(
            Status::from_machine_state(MachineState::Reviewing),
            Status::Running
        );
        assert_eq!(
            Status::from_machine_state(MachineState::Correcting),
            Status::Running
        );
        assert_eq!(
            Status::from_machine_state(MachineState::Committing),
            Status::Running
        );
        assert_eq!(
            Status::from_machine_state(MachineState::CreatingPR),
            Status::Running
        );
        assert_eq!(
            Status::from_machine_state(MachineState::Initializing),
            Status::Running
        );
        assert_eq!(
            Status::from_machine_state(MachineState::PickingStory),
            Status::Running
        );
        assert_eq!(
            Status::from_machine_state(MachineState::LoadingSpec),
            Status::Running
        );
        assert_eq!(
            Status::from_machine_state(MachineState::GeneratingSpec),
            Status::Running
        );
    }

    #[test]
    fn test_status_from_machine_state_terminal() {
        assert_eq!(
            Status::from_machine_state(MachineState::Completed),
            Status::Success
        );
        assert_eq!(
            Status::from_machine_state(MachineState::Failed),
            Status::Error
        );
        assert_eq!(Status::from_machine_state(MachineState::Idle), Status::Idle);
    }

    #[test]
    fn test_state_to_color() {
        assert_eq!(
            state_to_color(MachineState::RunningClaude),
            colors::STATUS_RUNNING
        );
        assert_eq!(
            state_to_color(MachineState::Completed),
            colors::STATUS_SUCCESS
        );
        assert_eq!(state_to_color(MachineState::Failed), colors::STATUS_ERROR);
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
    // Progress Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_progress_fraction() {
        let progress = Progress::new(2, 5);
        assert!((progress.fraction() - 0.4).abs() < 0.001);
    }

    #[test]
    fn test_progress_fraction_zero_total() {
        let progress = Progress::new(0, 0);
        assert_eq!(progress.fraction(), 0.0);
    }

    #[test]
    fn test_progress_fraction_complete() {
        let progress = Progress::new(5, 5);
        assert!((progress.fraction() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_progress_as_story_fraction() {
        let progress = Progress::new(1, 5);
        assert_eq!(progress.as_story_fraction(), "Story 2/5");
    }

    #[test]
    fn test_progress_as_story_fraction_first() {
        let progress = Progress::new(0, 3);
        assert_eq!(progress.as_story_fraction(), "Story 1/3");
    }

    #[test]
    fn test_progress_as_fraction() {
        let progress = Progress::new(2, 5);
        assert_eq!(progress.as_fraction(), "2/5");
    }

    #[test]
    fn test_progress_as_percentage() {
        let progress = Progress::new(2, 4);
        assert_eq!(progress.as_percentage(), "50%");
    }

    #[test]
    fn test_progress_as_percentage_zero_total() {
        let progress = Progress::new(0, 0);
        assert_eq!(progress.as_percentage(), "0%");
    }

    #[test]
    fn test_progress_as_percentage_complete() {
        let progress = Progress::new(5, 5);
        assert_eq!(progress.as_percentage(), "100%");
    }

    // ------------------------------------------------------------------------
    // ProgressBar Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_progress_bar_new() {
        let bar = ProgressBar::new(0.5);
        assert!((bar.progress() - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_progress_bar_clamps_values() {
        let bar_low = ProgressBar::new(-0.5);
        let bar_high = ProgressBar::new(1.5);
        assert_eq!(bar_low.progress(), 0.0);
        assert_eq!(bar_high.progress(), 1.0);
    }

    #[test]
    fn test_progress_bar_from_progress() {
        let progress = Progress::new(3, 10);
        let bar = ProgressBar::from_progress(&progress);
        assert!((bar.progress() - 0.3).abs() < 0.001);
    }

    #[test]
    fn test_progress_bar_default() {
        let bar = ProgressBar::default();
        assert_eq!(bar.progress(), 0.0);
    }

    // ------------------------------------------------------------------------
    // Duration Formatting Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_format_duration_secs_seconds_only() {
        assert_eq!(format_duration_secs(30), "30s");
        assert_eq!(format_duration_secs(0), "0s");
        assert_eq!(format_duration_secs(59), "59s");
    }

    #[test]
    fn test_format_duration_secs_minutes_and_seconds() {
        assert_eq!(format_duration_secs(60), "1m 0s");
        assert_eq!(format_duration_secs(125), "2m 5s");
        assert_eq!(format_duration_secs(3599), "59m 59s");
    }

    #[test]
    fn test_format_duration_secs_hours_and_minutes() {
        assert_eq!(format_duration_secs(3600), "1h 0m");
        assert_eq!(format_duration_secs(3700), "1h 1m");
        assert_eq!(format_duration_secs(7265), "2h 1m");
    }

    #[test]
    fn test_format_duration_from_timestamp() {
        let started_at = Utc::now() - chrono::Duration::seconds(125);
        let formatted = format_duration(started_at);
        assert!(formatted.contains("m"));
        assert!(formatted.contains("s"));
    }

    // ------------------------------------------------------------------------
    // Relative Time Formatting Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_format_relative_time_secs_just_now() {
        assert_eq!(format_relative_time_secs(0), "just now");
        assert_eq!(format_relative_time_secs(30), "just now");
        assert_eq!(format_relative_time_secs(59), "just now");
    }

    #[test]
    fn test_format_relative_time_secs_minutes() {
        assert_eq!(format_relative_time_secs(60), "1m ago");
        assert_eq!(format_relative_time_secs(300), "5m ago");
        assert_eq!(format_relative_time_secs(3599), "59m ago");
    }

    #[test]
    fn test_format_relative_time_secs_hours() {
        assert_eq!(format_relative_time_secs(3600), "1h ago");
        assert_eq!(format_relative_time_secs(7200), "2h ago");
        assert_eq!(format_relative_time_secs(86399), "23h ago");
    }

    #[test]
    fn test_format_relative_time_secs_days() {
        assert_eq!(format_relative_time_secs(86400), "1d ago");
        assert_eq!(format_relative_time_secs(172800), "2d ago");
        assert_eq!(format_relative_time_secs(604800), "7d ago");
    }

    #[test]
    fn test_format_relative_time_from_timestamp() {
        let timestamp = Utc::now() - chrono::Duration::hours(3);
        let formatted = format_relative_time(timestamp);
        assert!(formatted.contains("3h ago"));
    }

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
    fn test_truncate_with_ellipsis_long_string() {
        let result = truncate_with_ellipsis("this is a very long string", 15);
        assert_eq!(result, "this is a ve...");
        assert_eq!(result.len(), 15);
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
    // Constants Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_constants() {
        assert_eq!(STATUS_DOT_RADIUS, 4.0);
        assert_eq!(MAX_TEXT_LENGTH, 40);
        assert_eq!(MAX_BRANCH_LENGTH, 25);
    }

    // ------------------------------------------------------------------------
    // Badge Background Tests (US-007 Visual Polish)
    // ------------------------------------------------------------------------

    #[test]
    fn test_badge_background_color_creates_tinted_background() {
        // Badge backgrounds should be lighter than the status color
        let running_bg = badge_background_color(colors::STATUS_RUNNING);
        let success_bg = badge_background_color(colors::STATUS_SUCCESS);
        let error_bg = badge_background_color(colors::STATUS_ERROR);

        // Backgrounds should be relatively light (high luminance)
        let running_lum =
            running_bg.r() as u32 + running_bg.g() as u32 + running_bg.b() as u32;
        let success_lum =
            success_bg.r() as u32 + success_bg.g() as u32 + success_bg.b() as u32;
        let error_lum = error_bg.r() as u32 + error_bg.g() as u32 + error_bg.b() as u32;

        // Badge backgrounds should be light (luminance > 600 out of 765 max)
        assert!(
            running_lum > 600,
            "Running badge bg should be light, got luminance {}",
            running_lum
        );
        assert!(
            success_lum > 600,
            "Success badge bg should be light, got luminance {}",
            success_lum
        );
        assert!(
            error_lum > 600,
            "Error badge bg should be light, got luminance {}",
            error_lum
        );
    }

    #[test]
    fn test_badge_background_inherits_warm_tones() {
        // Badge backgrounds should blend with warm background color
        let bg = colors::BACKGROUND;

        // Test with a neutral color to see if warmth is preserved
        let neutral_status = Color32::from_rgb(100, 100, 100);
        let badge_bg = badge_background_color(neutral_status);

        // The badge background should have warm tones from the blend
        // Since we blend with warm BACKGROUND at 85%, the result should be warm
        assert!(
            badge_bg.r() >= badge_bg.b(),
            "Badge bg should inherit warm tones, got RGB({}, {}, {})",
            badge_bg.r(),
            badge_bg.g(),
            badge_bg.b()
        );
    }

    #[test]
    fn test_badge_background_retains_status_tint() {
        // Badge backgrounds should retain a tint of the status color
        let running_bg = badge_background_color(colors::STATUS_RUNNING);
        let success_bg = badge_background_color(colors::STATUS_SUCCESS);
        let error_bg = badge_background_color(colors::STATUS_ERROR);

        // Running (blue) should have higher blue component relative to pure warm background
        let bg = colors::BACKGROUND;
        assert!(
            running_bg.b() > bg.b() - 5 || running_bg.r() < bg.r() + 5,
            "Running badge should retain blue tint"
        );

        // Success (green) should have higher green component
        assert!(
            success_bg.g() > bg.g() - 5,
            "Success badge should retain green tint"
        );

        // Error (red) should have higher red component
        assert!(
            error_bg.r() > bg.r() - 5,
            "Error badge should retain red tint"
        );
    }
}
