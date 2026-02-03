//! Theme and color system for the GUI.
//!
//! This module provides a cohesive color system and egui Visuals configuration
//! that achieves a warm, approachable aesthetic inspired by the Claude desktop
//! application. Uses a warm beige/cream palette with proper semantic colors
//! for status states and UI elements.

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
///
/// Shadows use warm-tinted colors (slight brown/amber tint) to complement
/// the warm cream color palette, avoiding harsh pure-black shadows.
pub mod shadow {
    use super::Color32;
    use eframe::egui::Shadow;

    /// Warm shadow base color - a very subtle warm brown.
    /// Uses RGB values that lean warm (R > G > B) even at low alpha.
    /// This creates softer shadows that complement the warm palette.
    const SHADOW_WARM: Color32 = Color32::from_rgba_premultiplied(40, 30, 20, 255);

    /// Subtle shadow for slightly elevated elements.
    pub fn subtle() -> Shadow {
        Shadow {
            offset: [0.0, 1.0].into(),
            blur: 3.0,
            spread: 0.0,
            // Warm brown shadow at low opacity
            color: Color32::from_rgba_premultiplied(
                SHADOW_WARM.r(),
                SHADOW_WARM.g(),
                SHADOW_WARM.b(),
                12,
            ),
        }
    }

    /// Medium shadow for cards and panels.
    pub fn medium() -> Shadow {
        Shadow {
            offset: [0.0, 2.0].into(),
            blur: 8.0,
            spread: 0.0,
            // Warm brown shadow at medium opacity
            color: Color32::from_rgba_premultiplied(
                SHADOW_WARM.r(),
                SHADOW_WARM.g(),
                SHADOW_WARM.b(),
                18,
            ),
        }
    }

    /// Elevated shadow for modals and popovers.
    pub fn elevated() -> Shadow {
        Shadow {
            offset: [0.0, 4.0].into(),
            blur: 16.0,
            spread: 0.0,
            // Warm brown shadow at higher opacity
            color: Color32::from_rgba_premultiplied(
                SHADOW_WARM.r(),
                SHADOW_WARM.g(),
                SHADOW_WARM.b(),
                24,
            ),
        }
    }

    /// Returns the warm shadow base color for testing.
    #[cfg(test)]
    pub fn shadow_warm_color() -> Color32 {
        SHADOW_WARM
    }
}

/// Semantic color palette for the light theme.
///
/// Colors use a warm beige/cream palette inspired by the Claude desktop
/// application, with careful attention to contrast ratios for accessibility.
pub mod colors {
    use super::Color32;

    // ==========================================================================
    // Background Colors
    // ==========================================================================

    /// Primary window background - warm cream.
    /// Inspired by Claude's warm, approachable aesthetic (~#FAF9F7).
    pub const BACKGROUND: Color32 = Color32::from_rgb(250, 249, 247);

    /// Surface color for cards and panels - white.
    pub const SURFACE: Color32 = Color32::from_rgb(255, 255, 255);

    /// Elevated surface for modals and popovers.
    pub const SURFACE_ELEVATED: Color32 = Color32::from_rgb(255, 255, 255);

    /// Subtle background for hover states - warm beige tint.
    pub const SURFACE_HOVER: Color32 = Color32::from_rgb(245, 243, 239);

    /// Background for selected/active items - warm beige.
    pub const SURFACE_SELECTED: Color32 = Color32::from_rgb(238, 235, 229);

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

    /// Subtle border color for cards and inputs - warm gray.
    pub const BORDER: Color32 = Color32::from_rgb(232, 229, 222);

    /// Stronger border for focused elements - warm gray.
    pub const BORDER_FOCUSED: Color32 = Color32::from_rgb(205, 200, 190);

    /// Separator line color - warm gray.
    pub const SEPARATOR: Color32 = Color32::from_rgb(232, 229, 222);

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

    /// Idle state background - warm light beige.
    pub const STATUS_IDLE_BG: Color32 = Color32::from_rgb(245, 243, 239);
}

// Note: The Status enum is defined in the components module to avoid duplication.
// Use `crate::gui::components::Status` for status state to color mapping.

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
    visuals.code_bg_color = Color32::from_rgb(248, 246, 242);

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
        // Enable smooth animations for hover/click state transitions.
        // This affects widget state changes (hover, press, etc.) with a
        // ~100ms ease-in animation for polished visual feedback.
        // Note: egui doesn't support panel appearance/disappearance animations,
        // but scroll areas and widget state changes will be smoothly animated.
        animation_time: 0.1,
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
    use crate::gui::components::Status;

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
        // Status enum is defined in components module
        assert_eq!(Status::Running.color(), colors::STATUS_RUNNING);
        assert_eq!(Status::Success.color(), colors::STATUS_SUCCESS);
        assert_eq!(Status::Warning.color(), colors::STATUS_WARNING);
        assert_eq!(Status::Error.color(), colors::STATUS_ERROR);
        assert_eq!(Status::Idle.color(), colors::STATUS_IDLE);
    }

    #[test]
    fn test_status_background_colors() {
        // Status enum is defined in components module
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
        assert!(elevated.color.a() <= 30);

        // Verify blur increases with elevation
        assert!(subtle.blur < medium.blur);
        assert!(medium.blur < elevated.blur);
    }

    #[test]
    fn test_shadows_use_warm_tones() {
        // Verify shadow colors have warm tones (R > G > B)
        let warm = shadow::shadow_warm_color();
        assert!(
            warm.r() > warm.g() && warm.g() > warm.b(),
            "Shadow base color should be warm (R > G > B), got RGB({}, {}, {})",
            warm.r(),
            warm.g(),
            warm.b()
        );
    }

    #[test]
    fn test_shadows_have_consistent_warmth() {
        // All shadow levels should use the same warm base color
        let subtle = shadow::subtle();
        let medium = shadow::medium();
        let elevated = shadow::elevated();

        // Extract RGB ratios (alpha varies, but RGB ratios should match)
        let warm = shadow::shadow_warm_color();

        // Verify each shadow uses the warm base color RGB values
        assert_eq!(subtle.color.r(), warm.r());
        assert_eq!(subtle.color.g(), warm.g());
        assert_eq!(subtle.color.b(), warm.b());

        assert_eq!(medium.color.r(), warm.r());
        assert_eq!(medium.color.g(), warm.g());
        assert_eq!(medium.color.b(), warm.b());

        assert_eq!(elevated.color.r(), warm.r());
        assert_eq!(elevated.color.g(), warm.g());
        assert_eq!(elevated.color.b(), warm.b());
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
        // Status enum is defined in components module
        assert_eq!(Status::Running, Status::Running);
        assert_ne!(Status::Running, Status::Idle);
    }

    #[test]
    fn test_status_enum_copy() {
        // Status enum is defined in components module
        let status = Status::Success;
        let copied = status;
        assert_eq!(status, copied);
    }

    // ========================================================================
    // Visual Polish Tests (US-007)
    // ========================================================================

    #[test]
    fn test_animation_time_configured() {
        let style = configure_style();
        // Animation time should be configured for smooth transitions
        assert!(
            style.animation_time > 0.0,
            "Animation time should be positive"
        );
        assert!(
            style.animation_time <= 0.2,
            "Animation time should not be too long (responsive feel)"
        );
    }

    #[test]
    fn test_widget_hover_visuals_configured() {
        let visuals = configure_visuals();
        // Hovered widgets should have distinct styling
        assert_eq!(
            visuals.widgets.hovered.bg_fill,
            colors::SURFACE_HOVER,
            "Hovered widgets should use SURFACE_HOVER"
        );
        assert_eq!(
            visuals.widgets.hovered.bg_stroke.color,
            colors::BORDER_FOCUSED,
            "Hovered widgets should have focused border"
        );
    }

    #[test]
    fn test_widget_active_visuals_configured() {
        let visuals = configure_visuals();
        // Active/pressed widgets should have distinct styling
        assert_eq!(
            visuals.widgets.active.bg_fill,
            colors::SURFACE_SELECTED,
            "Active widgets should use SURFACE_SELECTED"
        );
        assert_eq!(
            visuals.widgets.active.bg_stroke.color,
            colors::ACCENT,
            "Active widgets should have accent border"
        );
    }

    #[test]
    fn test_spacing_scale_consistency() {
        // Verify spacing values follow a consistent scale
        // Each step should be roughly double or 1.5x the previous
        assert!(spacing::SM >= spacing::XS * 1.5);
        assert!(spacing::MD >= spacing::SM * 1.25);
        assert!(spacing::LG >= spacing::MD * 1.25);
        assert!(spacing::XL >= spacing::LG * 1.25);
    }

    #[test]
    fn test_scroll_style_configured() {
        let style = configure_style();
        // Scroll bars should have floating style for modern look
        assert!(
            style.spacing.scroll.floating,
            "Scroll bars should use floating style"
        );
        assert!(
            style.spacing.scroll.bar_width >= 6.0 && style.spacing.scroll.bar_width <= 12.0,
            "Scroll bar width should be moderate"
        );
    }

    #[test]
    fn test_selection_colors_configured() {
        let visuals = configure_visuals();
        // Selection should use accent colors
        assert_eq!(
            visuals.selection.bg_fill,
            colors::ACCENT_SUBTLE,
            "Selection background should use ACCENT_SUBTLE"
        );
    }

    // ========================================================================
    // Warm Color Palette Tests (US-001)
    // ========================================================================

    #[test]
    fn test_background_is_warm_cream() {
        // BACKGROUND should be warm cream (~#FAF9F7), not cool gray
        // Warm colors have R >= G >= B
        let bg = colors::BACKGROUND;
        assert!(
            bg.r() >= bg.g() && bg.g() >= bg.b(),
            "BACKGROUND should have warm tones (R >= G >= B), got RGB({}, {}, {})",
            bg.r(),
            bg.g(),
            bg.b()
        );
        // Should be close to #FAF9F7 (250, 249, 247)
        assert_eq!(bg, Color32::from_rgb(250, 249, 247));
    }

    #[test]
    fn test_surface_hover_is_warm() {
        // SURFACE_HOVER should use warm tones
        let hover = colors::SURFACE_HOVER;
        assert!(
            hover.r() >= hover.g() && hover.g() >= hover.b(),
            "SURFACE_HOVER should have warm tones (R >= G >= B), got RGB({}, {}, {})",
            hover.r(),
            hover.g(),
            hover.b()
        );
    }

    #[test]
    fn test_surface_selected_is_warm() {
        // SURFACE_SELECTED should use warm tones
        let selected = colors::SURFACE_SELECTED;
        assert!(
            selected.r() >= selected.g() && selected.g() >= selected.b(),
            "SURFACE_SELECTED should have warm tones (R >= G >= B), got RGB({}, {}, {})",
            selected.r(),
            selected.g(),
            selected.b()
        );
    }

    #[test]
    fn test_borders_are_warm_gray() {
        // BORDER and SEPARATOR should use warm gray tones
        let border = colors::BORDER;
        assert!(
            border.r() >= border.g() && border.g() >= border.b(),
            "BORDER should have warm tones (R >= G >= B), got RGB({}, {}, {})",
            border.r(),
            border.g(),
            border.b()
        );

        let separator = colors::SEPARATOR;
        assert!(
            separator.r() >= separator.g() && separator.g() >= separator.b(),
            "SEPARATOR should have warm tones (R >= G >= B), got RGB({}, {}, {})",
            separator.r(),
            separator.g(),
            separator.b()
        );
    }

    #[test]
    fn test_status_colors_unchanged() {
        // Status colors should remain unchanged for clarity
        assert_eq!(
            colors::STATUS_RUNNING,
            Color32::from_rgb(0, 149, 255),
            "STATUS_RUNNING should remain blue"
        );
        assert_eq!(
            colors::STATUS_SUCCESS,
            Color32::from_rgb(52, 199, 89),
            "STATUS_SUCCESS should remain green"
        );
        assert_eq!(
            colors::STATUS_WARNING,
            Color32::from_rgb(255, 149, 0),
            "STATUS_WARNING should remain amber"
        );
        assert_eq!(
            colors::STATUS_ERROR,
            Color32::from_rgb(255, 59, 48),
            "STATUS_ERROR should remain red"
        );
    }

    #[test]
    fn test_text_colors_high_contrast() {
        // Text colors should remain high-contrast for readability
        // TEXT_PRIMARY should be dark
        let text = colors::TEXT_PRIMARY;
        let luminance = (text.r() as u32 + text.g() as u32 + text.b() as u32) / 3;
        assert!(
            luminance < 50,
            "TEXT_PRIMARY should be dark for readability, got luminance {}",
            luminance
        );
    }

    #[test]
    fn test_warm_palette_harmony() {
        // Verify the warm colors form a cohesive palette
        // Darker warm colors should still be warm
        let colors_to_check = [
            ("BACKGROUND", colors::BACKGROUND),
            ("SURFACE_HOVER", colors::SURFACE_HOVER),
            ("SURFACE_SELECTED", colors::SURFACE_SELECTED),
            ("BORDER", colors::BORDER),
            ("BORDER_FOCUSED", colors::BORDER_FOCUSED),
        ];

        for (name, color) in colors_to_check {
            // For warm colors, the difference between R and B should be positive
            let warmth = color.r() as i32 - color.b() as i32;
            assert!(
                warmth >= 0,
                "{} should have warm tones (R >= B), got warmth diff {}",
                name,
                warmth
            );
        }
    }
}
