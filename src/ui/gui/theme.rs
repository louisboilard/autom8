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

    /// Green-tinted glow shadow for completed sessions.
    ///
    /// Takes a configurable `alpha` (0.0–1.0) so callers can animate the
    /// glow intensity over time (e.g., a pulsing effect). Uses `STATUS_SUCCESS`
    /// green with zero offset for a centered, non-directional glow.
    pub fn completed_glow(alpha: f32) -> Shadow {
        let a = (alpha.clamp(0.0, 1.0) * 255.0) as u8;
        Shadow {
            offset: [0.0, 0.0].into(),
            blur: 10.0,
            spread: 0.0,
            color: Color32::from_rgba_premultiplied(
                super::colors::COMPLETED_GLOW.r(),
                super::colors::COMPLETED_GLOW.g(),
                super::colors::COMPLETED_GLOW.b(),
                a,
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

    /// Correcting state - orange (attention needed, distinct from warning amber).
    pub const STATUS_CORRECTING: Color32 = Color32::from_rgb(255, 94, 58);

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

    /// Correcting state background - light orange.
    pub const STATUS_CORRECTING_BG: Color32 = Color32::from_rgb(255, 237, 230);

    // ==========================================================================
    // Completed Session Colors
    // ==========================================================================

    /// Glow color for the pulsing border on completed sessions.
    /// Derived from `STATUS_SUCCESS` (`rgb(52, 199, 89)`).
    pub const COMPLETED_GLOW: Color32 = Color32::from_rgb(52, 199, 89);

    /// Subtle green-tinted fill for completed session card backgrounds.
    /// Blends `STATUS_SUCCESS_BG` with `SURFACE` for a warm pale green
    /// that harmonizes with the cream palette.
    pub const COMPLETED_FILL: Color32 = Color32::from_rgb(244, 252, 247);

    /// Even subtler green tint for the completed session tab background.
    /// Lower saturation than `COMPLETED_FILL` for the tab bar.
    pub const COMPLETED_TAB_FILL: Color32 = Color32::from_rgb(247, 253, 249);

    /// Green-tinted hover for completed session tabs.
    /// Blends `COMPLETED_TAB_FILL` with `SURFACE_HOVER` — slightly more
    /// saturated green than the default warm hover.
    pub const COMPLETED_TAB_HOVER: Color32 = Color32::from_rgb(240, 246, 240);

    /// Green-tinted selected/active for completed session tabs.
    /// Blends `COMPLETED_TAB_FILL` with `SURFACE_SELECTED` — the green tint
    /// remains perceptible even with the darker selection background.
    pub const COMPLETED_TAB_ACTIVE: Color32 = Color32::from_rgb(234, 242, 234);

    /// Faint green border color for the static border tint on completed sessions.
    /// Low-opacity `STATUS_SUCCESS` blended onto white.
    pub const COMPLETED_BORDER: Color32 = Color32::from_rgb(220, 243, 228);
}

// Note: The Status enum is defined in the components module to avoid duplication.
// Use `crate::ui::gui::components::Status` for status state to color mapping.

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

    #[test]
    fn test_spacing_scale() {
        // Verify spacing scale is monotonically increasing
        assert!(spacing::XS < spacing::SM);
        assert!(spacing::SM < spacing::MD);
        assert!(spacing::MD < spacing::LG);
        assert!(spacing::LG < spacing::XL);
        assert!(spacing::XL < spacing::XXL);
    }

    #[test]
    fn test_shadows() {
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

        // Verify warm tones
        let warm = shadow::shadow_warm_color();
        assert!(warm.r() > warm.g() && warm.g() > warm.b());
    }

    #[test]
    fn test_text_contrast() {
        // Primary text should have good contrast against both background and surface
        let text_lum = colors::TEXT_PRIMARY.r() as u32
            + colors::TEXT_PRIMARY.g() as u32
            + colors::TEXT_PRIMARY.b() as u32;
        let bg_lum = colors::BACKGROUND.r() as u32
            + colors::BACKGROUND.g() as u32
            + colors::BACKGROUND.b() as u32;
        let surface_lum =
            colors::SURFACE.r() as u32 + colors::SURFACE.g() as u32 + colors::SURFACE.b() as u32;

        assert!(
            bg_lum - text_lum > 400,
            "Need contrast > 400 against background"
        );
        assert!(
            surface_lum - text_lum > 500,
            "Need contrast > 500 against surface"
        );
    }

    #[test]
    fn test_configure_visuals() {
        let visuals = configure_visuals();

        assert!(!visuals.dark_mode);
        assert_eq!(visuals.window_fill, colors::SURFACE);
        assert_eq!(visuals.panel_fill, colors::BACKGROUND);
        assert_eq!(visuals.window_rounding, Rounding::same(rounding::CARD));
        assert_eq!(visuals.widgets.hovered.bg_fill, colors::SURFACE_HOVER);
        assert_eq!(visuals.widgets.active.bg_fill, colors::SURFACE_SELECTED);
        assert_eq!(visuals.selection.bg_fill, colors::ACCENT_SUBTLE);
    }

    #[test]
    fn test_configure_style() {
        let style = configure_style();

        assert!(style.animation_time > 0.0 && style.animation_time <= 0.2);
        assert!(style.spacing.scroll.floating);
        assert_eq!(style.visuals.window_fill, colors::SURFACE);
    }

    #[test]
    fn test_warm_color_palette() {
        // All these colors should have warm tones (R >= G >= B)
        let warm_colors = [
            ("BACKGROUND", colors::BACKGROUND),
            ("SURFACE_HOVER", colors::SURFACE_HOVER),
            ("SURFACE_SELECTED", colors::SURFACE_SELECTED),
            ("BORDER", colors::BORDER),
            ("SEPARATOR", colors::SEPARATOR),
        ];

        for (name, color) in warm_colors {
            assert!(
                color.r() >= color.g() && color.g() >= color.b(),
                "{} should have warm tones (R >= G >= B), got RGB({}, {}, {})",
                name,
                color.r(),
                color.g(),
                color.b()
            );
        }

        // BACKGROUND should be the specific warm cream color
        assert_eq!(colors::BACKGROUND, Color32::from_rgb(250, 249, 247));
    }

    #[test]
    fn test_completed_colors() {
        // COMPLETED_GLOW matches STATUS_SUCCESS
        assert_eq!(colors::COMPLETED_GLOW, colors::STATUS_SUCCESS);

        // COMPLETED_FILL is a warm pale green (green channel dominates slightly)
        assert!(colors::COMPLETED_FILL.g() > colors::COMPLETED_FILL.r());
        assert!(colors::COMPLETED_FILL.g() > colors::COMPLETED_FILL.b());
        // Should be lighter/subtler than STATUS_SUCCESS_BG
        assert!(colors::COMPLETED_FILL.r() >= colors::STATUS_SUCCESS_BG.r());

        // COMPLETED_TAB_FILL is even subtler than COMPLETED_FILL
        assert!(colors::COMPLETED_TAB_FILL.r() >= colors::COMPLETED_FILL.r());

        // COMPLETED_TAB_HOVER is darker than TAB_FILL but retains green tint
        assert!(colors::COMPLETED_TAB_HOVER.g() >= colors::COMPLETED_TAB_HOVER.r());
        assert!(colors::COMPLETED_TAB_HOVER.r() < colors::COMPLETED_TAB_FILL.r());

        // COMPLETED_TAB_ACTIVE is darker still but retains green tint
        assert!(colors::COMPLETED_TAB_ACTIVE.g() >= colors::COMPLETED_TAB_ACTIVE.r());
        assert!(colors::COMPLETED_TAB_ACTIVE.r() < colors::COMPLETED_TAB_HOVER.r());

        // COMPLETED_BORDER has green tint
        assert!(colors::COMPLETED_BORDER.g() > colors::COMPLETED_BORDER.r());
        assert!(colors::COMPLETED_BORDER.g() > colors::COMPLETED_BORDER.b());
    }

    #[test]
    fn test_completed_glow_shadow() {
        // Zero alpha produces transparent shadow
        let glow_zero = shadow::completed_glow(0.0);
        assert_eq!(glow_zero.color.a(), 0);
        assert_eq!(glow_zero.offset, [0.0, 0.0].into());
        assert!(glow_zero.blur >= 8.0 && glow_zero.blur <= 12.0);

        // Full alpha
        let glow_full = shadow::completed_glow(1.0);
        assert_eq!(glow_full.color.a(), 255);

        // Mid alpha
        let glow_mid = shadow::completed_glow(0.5);
        assert!(glow_mid.color.a() > 100 && glow_mid.color.a() < 140);

        // Color channels match COMPLETED_GLOW / STATUS_SUCCESS
        assert_eq!(glow_full.color.r(), colors::COMPLETED_GLOW.r());
        assert_eq!(glow_full.color.g(), colors::COMPLETED_GLOW.g());
        assert_eq!(glow_full.color.b(), colors::COMPLETED_GLOW.b());

        // Clamping works
        let glow_over = shadow::completed_glow(2.0);
        assert_eq!(glow_over.color.a(), 255);
        let glow_under = shadow::completed_glow(-1.0);
        assert_eq!(glow_under.color.a(), 0);
    }

    #[test]
    fn test_status_colors_distinct() {
        // Status colors should be visually distinct
        let status_colors = [
            colors::STATUS_RUNNING,
            colors::STATUS_SUCCESS,
            colors::STATUS_WARNING,
            colors::STATUS_ERROR,
            colors::STATUS_IDLE,
        ];

        for i in 0..status_colors.len() {
            for j in (i + 1)..status_colors.len() {
                assert_ne!(status_colors[i], status_colors[j]);
            }
        }
    }
}
