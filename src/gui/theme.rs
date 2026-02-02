//! Theme and color system for the GUI.
//!
//! This module provides a cohesive color system and egui Visuals configuration
//! that achieves a macOS-native aesthetic with proper semantic colors for
//! status states and UI elements.

use eframe::egui::{self, Color32, Rounding, Stroke, Style, Visuals};

/// Spacing scale for consistent layout throughout the application.
///
/// Use these constants instead of arbitrary pixel values to ensure
/// visual consistency and a cohesive rhythm across all UI elements.
pub mod spacing {
    /// Extra small spacing (4px) - tight spacing between related elements.
    pub const XS: f32 = 4.0;

    /// Small spacing (8px) - standard spacing between related elements.
    pub const SM: f32 = 8.0;

    /// Medium spacing (12px) - spacing between sections within a component.
    pub const MD: f32 = 12.0;

    /// Standard spacing (16px) - standard component padding, gaps between cards.
    pub const LG: f32 = 16.0;

    /// Large spacing (24px) - spacing between major sections.
    pub const XL: f32 = 24.0;

    /// Extra large spacing (32px) - large gaps, page-level margins.
    pub const XXL: f32 = 32.0;
}

/// Corner rounding values for consistent UI elements.
pub mod rounding {
    /// Rounding for cards and panels (8px).
    pub const CARD: f32 = 8.0;

    /// Rounding for buttons and inputs (4px).
    pub const BUTTON: f32 = 4.0;

    /// Rounding for small elements like badges (2px).
    pub const SMALL: f32 = 2.0;

    /// No rounding (0px).
    pub const NONE: f32 = 0.0;
}

/// Shadow depths for elevated surfaces.
pub mod shadow {
    use super::Color32;
    use eframe::egui::Shadow;

    /// Subtle shadow for slightly elevated elements.
    pub fn subtle() -> Shadow {
        Shadow {
            offset: [0.0, 1.0].into(),
            blur: 3.0,
            spread: 0.0,
            color: Color32::from_black_alpha(10),
        }
    }

    /// Medium shadow for cards and panels.
    pub fn medium() -> Shadow {
        Shadow {
            offset: [0.0, 2.0].into(),
            blur: 8.0,
            spread: 0.0,
            color: Color32::from_black_alpha(15),
        }
    }

    /// Elevated shadow for modals and popovers.
    pub fn elevated() -> Shadow {
        Shadow {
            offset: [0.0, 4.0].into(),
            blur: 16.0,
            spread: 0.0,
            color: Color32::from_black_alpha(20),
        }
    }
}

/// Semantic color palette for the light theme.
///
/// Colors are inspired by macOS system colors with careful attention
/// to contrast ratios for accessibility.
pub mod colors {
    use super::Color32;

    // ==========================================================================
    // Background Colors
    // ==========================================================================

    /// Primary window background - very light gray.
    /// Similar to macOS window background.
    pub const BACKGROUND: Color32 = Color32::from_rgb(246, 246, 248);

    /// Surface color for cards and panels - white.
    pub const SURFACE: Color32 = Color32::from_rgb(255, 255, 255);

    /// Elevated surface for modals and popovers.
    pub const SURFACE_ELEVATED: Color32 = Color32::from_rgb(255, 255, 255);

    /// Subtle background for hover states.
    pub const SURFACE_HOVER: Color32 = Color32::from_rgb(240, 240, 242);

    /// Background for selected/active items.
    pub const SURFACE_SELECTED: Color32 = Color32::from_rgb(232, 232, 237);

    // ==========================================================================
    // Text Colors
    // ==========================================================================

    /// Primary text color - dark gray for good contrast.
    /// WCAG AA compliant against BACKGROUND and SURFACE.
    pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(28, 28, 30);

    /// Secondary text color - medium gray for less emphasis.
    pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(99, 99, 102);

    /// Muted text color - light gray for tertiary information.
    pub const TEXT_MUTED: Color32 = Color32::from_rgb(142, 142, 147);

    /// Disabled text color.
    pub const TEXT_DISABLED: Color32 = Color32::from_rgb(174, 174, 178);

    // ==========================================================================
    // Border and Separator Colors
    // ==========================================================================

    /// Subtle border color for cards and inputs.
    pub const BORDER: Color32 = Color32::from_rgb(229, 229, 234);

    /// Stronger border for focused elements.
    pub const BORDER_FOCUSED: Color32 = Color32::from_rgb(199, 199, 204);

    /// Separator line color.
    pub const SEPARATOR: Color32 = Color32::from_rgb(229, 229, 234);

    // ==========================================================================
    // Accent Colors
    // ==========================================================================

    /// Primary accent color - blue (similar to macOS system blue).
    pub const ACCENT: Color32 = Color32::from_rgb(0, 122, 255);

    /// Accent color for hover state.
    pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0, 111, 230);

    /// Accent color for active/pressed state.
    pub const ACCENT_ACTIVE: Color32 = Color32::from_rgb(0, 100, 210);

    /// Light accent for backgrounds.
    pub const ACCENT_SUBTLE: Color32 = Color32::from_rgb(230, 244, 255);

    // ==========================================================================
    // Status Colors
    // ==========================================================================

    /// Running state - blue/cyan (indicates active work).
    pub const STATUS_RUNNING: Color32 = Color32::from_rgb(0, 149, 255);

    /// Success state - green (indicates completion).
    pub const STATUS_SUCCESS: Color32 = Color32::from_rgb(52, 199, 89);

    /// Warning state - amber (indicates attention needed).
    pub const STATUS_WARNING: Color32 = Color32::from_rgb(255, 149, 0);

    /// Error state - red (indicates failure).
    pub const STATUS_ERROR: Color32 = Color32::from_rgb(255, 59, 48);

    /// Idle state - gray (indicates inactive).
    pub const STATUS_IDLE: Color32 = Color32::from_rgb(142, 142, 147);

    // ==========================================================================
    // Status Background Colors (for badges/highlights)
    // ==========================================================================

    /// Running state background - light blue.
    pub const STATUS_RUNNING_BG: Color32 = Color32::from_rgb(230, 244, 255);

    /// Success state background - light green.
    pub const STATUS_SUCCESS_BG: Color32 = Color32::from_rgb(232, 250, 238);

    /// Warning state background - light amber.
    pub const STATUS_WARNING_BG: Color32 = Color32::from_rgb(255, 244, 230);

    /// Error state background - light red.
    pub const STATUS_ERROR_BG: Color32 = Color32::from_rgb(255, 235, 234);

    /// Idle state background - light gray.
    pub const STATUS_IDLE_BG: Color32 = Color32::from_rgb(240, 240, 242);
}

/// Status state enum for mapping to colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Running/active state.
    Running,
    /// Successful completion.
    Success,
    /// Warning/attention needed.
    Warning,
    /// Error/failure state.
    Error,
    /// Idle/inactive state.
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

    /// Returns the background color for this status.
    pub fn background_color(self) -> Color32 {
        match self {
            Status::Running => colors::STATUS_RUNNING_BG,
            Status::Success => colors::STATUS_SUCCESS_BG,
            Status::Warning => colors::STATUS_WARNING_BG,
            Status::Error => colors::STATUS_ERROR_BG,
            Status::Idle => colors::STATUS_IDLE_BG,
        }
    }
}

/// Configure egui Visuals for the light theme.
///
/// This creates a custom Visuals configuration that matches the macOS
/// aesthetic with proper colors, rounding, and shadows.
pub fn configure_visuals() -> Visuals {
    let mut visuals = Visuals::light();

    // Window and panel backgrounds
    visuals.window_fill = colors::SURFACE;
    visuals.panel_fill = colors::BACKGROUND;
    visuals.faint_bg_color = colors::SURFACE_HOVER;
    visuals.extreme_bg_color = colors::SURFACE;
    visuals.code_bg_color = Color32::from_rgb(245, 245, 247);

    // Selection colors
    visuals.selection.bg_fill = colors::ACCENT_SUBTLE;
    visuals.selection.stroke = Stroke::new(1.0, colors::ACCENT);

    // Hyperlink color
    visuals.hyperlink_color = colors::ACCENT;

    // Window shadow (for popups/menus)
    visuals.window_shadow = shadow::elevated();
    visuals.popup_shadow = shadow::medium();

    // Window stroke (border)
    visuals.window_stroke = Stroke::new(1.0, colors::BORDER);

    // Corner rounding
    visuals.window_rounding = Rounding::same(rounding::CARD);
    visuals.menu_rounding = Rounding::same(rounding::BUTTON);

    // Text cursor
    visuals.text_cursor.stroke = Stroke::new(2.0, colors::ACCENT);

    // Widget visuals
    configure_widget_visuals(&mut visuals);

    visuals
}

/// Configure widget-specific visuals.
fn configure_widget_visuals(visuals: &mut Visuals) {
    // Noninteractive widgets (labels, etc.)
    visuals.widgets.noninteractive.bg_fill = colors::SURFACE;
    visuals.widgets.noninteractive.weak_bg_fill = colors::SURFACE_HOVER;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, colors::BORDER);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, colors::TEXT_PRIMARY);
    visuals.widgets.noninteractive.rounding = Rounding::same(rounding::BUTTON);

    // Inactive widgets (not hovered, not clicked)
    visuals.widgets.inactive.bg_fill = colors::SURFACE;
    visuals.widgets.inactive.weak_bg_fill = colors::SURFACE;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, colors::BORDER);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, colors::TEXT_PRIMARY);
    visuals.widgets.inactive.rounding = Rounding::same(rounding::BUTTON);

    // Hovered widgets
    visuals.widgets.hovered.bg_fill = colors::SURFACE_HOVER;
    visuals.widgets.hovered.weak_bg_fill = colors::SURFACE_HOVER;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, colors::BORDER_FOCUSED);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.5, colors::TEXT_PRIMARY);
    visuals.widgets.hovered.rounding = Rounding::same(rounding::BUTTON);

    // Active/pressed widgets
    visuals.widgets.active.bg_fill = colors::SURFACE_SELECTED;
    visuals.widgets.active.weak_bg_fill = colors::SURFACE_SELECTED;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, colors::ACCENT);
    visuals.widgets.active.fg_stroke = Stroke::new(2.0, colors::TEXT_PRIMARY);
    visuals.widgets.active.rounding = Rounding::same(rounding::BUTTON);

    // Open widgets (combo boxes, menus when open)
    visuals.widgets.open.bg_fill = colors::SURFACE;
    visuals.widgets.open.weak_bg_fill = colors::SURFACE_HOVER;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, colors::ACCENT);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, colors::TEXT_PRIMARY);
    visuals.widgets.open.rounding = Rounding::same(rounding::BUTTON);
}

/// Configure the egui Style with additional settings.
pub fn configure_style() -> Style {
    // Get default style and modify spacing
    let default_style = Style::default();
    let mut style_spacing = default_style.spacing.clone();

    // Use our spacing scale for consistency
    style_spacing.item_spacing = egui::vec2(spacing::SM, spacing::XS);
    style_spacing.window_margin = egui::Margin::same(spacing::LG);
    style_spacing.button_padding = egui::vec2(spacing::MD, 6.0);
    style_spacing.menu_margin = egui::Margin::same(spacing::SM);
    style_spacing.indent = spacing::LG;
    style_spacing.scroll = egui::style::ScrollStyle {
        floating: true,
        bar_width: spacing::SM,
        // Smoother scroll animation
        floating_allocated_width: 0.0,
        bar_inner_margin: spacing::XS,
        bar_outer_margin: spacing::XS,
        ..Default::default()
    };
    // Ensure consistent spacing for combo boxes and menus
    style_spacing.combo_width = 100.0;

    // Modify interaction settings
    let mut interaction = default_style.interaction.clone();
    interaction.selectable_labels = true;
    interaction.multi_widget_text_select = true;

    Style {
        visuals: configure_visuals(),
        spacing: style_spacing,
        interaction,
        // animation_time uses default which provides smooth transitions
        ..Default::default()
    }
}

/// Initialize the theme for the application.
///
/// Call this during app initialization (in the CreationContext callback)
/// to apply the custom theme globally.
pub fn init(ctx: &egui::Context) {
    ctx.set_style(configure_style());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spacing_scale() {
        // Verify spacing scale values
        assert_eq!(spacing::XS, 4.0);
        assert_eq!(spacing::SM, 8.0);
        assert_eq!(spacing::MD, 12.0);
        assert_eq!(spacing::LG, 16.0);
        assert_eq!(spacing::XL, 24.0);
        assert_eq!(spacing::XXL, 32.0);
    }

    #[test]
    fn test_spacing_scale_progression() {
        // Verify spacing scale is monotonically increasing
        assert!(spacing::XS < spacing::SM);
        assert!(spacing::SM < spacing::MD);
        assert!(spacing::MD < spacing::LG);
        assert!(spacing::LG < spacing::XL);
        assert!(spacing::XL < spacing::XXL);
    }

    #[test]
    fn test_rounding_values() {
        assert_eq!(rounding::CARD, 8.0);
        assert_eq!(rounding::BUTTON, 4.0);
        assert_eq!(rounding::SMALL, 2.0);
        assert_eq!(rounding::NONE, 0.0);
    }

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
    fn test_shadows_are_subtle() {
        let subtle = shadow::subtle();
        let medium = shadow::medium();
        let elevated = shadow::elevated();

        // Verify shadow alpha values are appropriately subtle
        assert!(subtle.color.a() <= 15);
        assert!(medium.color.a() <= 20);
        assert!(elevated.color.a() <= 25);

        // Verify blur increases with elevation
        assert!(subtle.blur < medium.blur);
        assert!(medium.blur < elevated.blur);
    }

    #[test]
    fn test_colors_are_distinct() {
        // Status colors should be visually distinct
        assert_ne!(colors::STATUS_RUNNING, colors::STATUS_SUCCESS);
        assert_ne!(colors::STATUS_SUCCESS, colors::STATUS_WARNING);
        assert_ne!(colors::STATUS_WARNING, colors::STATUS_ERROR);
        assert_ne!(colors::STATUS_ERROR, colors::STATUS_IDLE);
    }

    #[test]
    fn test_text_contrast_against_background() {
        // Simplified contrast check: primary text should be much darker than background
        let text_luminance = colors::TEXT_PRIMARY.r() as u32
            + colors::TEXT_PRIMARY.g() as u32
            + colors::TEXT_PRIMARY.b() as u32;
        let bg_luminance = colors::BACKGROUND.r() as u32
            + colors::BACKGROUND.g() as u32
            + colors::BACKGROUND.b() as u32;

        // Text should be significantly darker than background
        assert!(text_luminance < bg_luminance);

        // The difference should be substantial for readability
        let contrast_diff = bg_luminance - text_luminance;
        assert!(
            contrast_diff > 400,
            "Expected contrast difference > 400, got {}",
            contrast_diff
        );
    }

    #[test]
    fn test_text_contrast_against_surface() {
        // Primary text should have good contrast against surface (white)
        let text_luminance = colors::TEXT_PRIMARY.r() as u32
            + colors::TEXT_PRIMARY.g() as u32
            + colors::TEXT_PRIMARY.b() as u32;
        let surface_luminance =
            colors::SURFACE.r() as u32 + colors::SURFACE.g() as u32 + colors::SURFACE.b() as u32;

        // Text should be significantly darker than surface
        assert!(text_luminance < surface_luminance);

        // The difference should be substantial for readability
        let contrast_diff = surface_luminance - text_luminance;
        assert!(
            contrast_diff > 500,
            "Expected contrast difference > 500, got {}",
            contrast_diff
        );
    }

    #[test]
    fn test_configure_visuals_returns_light_base() {
        let visuals = configure_visuals();

        // Should be a light theme
        assert!(!visuals.dark_mode);
    }

    #[test]
    fn test_configure_visuals_has_correct_rounding() {
        let visuals = configure_visuals();

        // Window rounding should be card rounding (8px)
        assert_eq!(visuals.window_rounding, Rounding::same(rounding::CARD));

        // Menu rounding should be button rounding (4px)
        assert_eq!(visuals.menu_rounding, Rounding::same(rounding::BUTTON));
    }

    #[test]
    fn test_configure_visuals_has_custom_colors() {
        let visuals = configure_visuals();

        // Verify custom colors are applied
        assert_eq!(visuals.window_fill, colors::SURFACE);
        assert_eq!(visuals.panel_fill, colors::BACKGROUND);
        assert_eq!(visuals.hyperlink_color, colors::ACCENT);
    }

    #[test]
    fn test_configure_style_includes_visuals() {
        let style = configure_style();

        // Style should include our custom visuals
        assert_eq!(style.visuals.window_fill, colors::SURFACE);
        assert_eq!(style.visuals.panel_fill, colors::BACKGROUND);
    }

    #[test]
    fn test_status_enum_equality() {
        assert_eq!(Status::Running, Status::Running);
        assert_ne!(Status::Running, Status::Idle);
    }

    #[test]
    fn test_status_enum_copy() {
        let status = Status::Success;
        let copied = status;
        assert_eq!(status, copied);
    }
}
