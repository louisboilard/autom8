//! Phase banner display.
//!
//! Provides visual phase indicators for the autom8 workflow.

use terminal_size::{terminal_size, Width};

use super::colors::*;

const DEFAULT_TERMINAL_WIDTH: u16 = 80;
const MIN_BANNER_WIDTH: usize = 20;
const MAX_BANNER_WIDTH: usize = 80;

/// Color options for phase banners.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BannerColor {
    /// Cyan - used for starting a phase
    Cyan,
    /// Green - used for successful completion
    Green,
    /// Red - used for failure
    Red,
    /// Yellow - used for correction/warning phases
    Yellow,
}

impl BannerColor {
    /// Get the ANSI color code for this banner color.
    pub fn ansi_code(&self) -> &'static str {
        match self {
            BannerColor::Cyan => CYAN,
            BannerColor::Green => GREEN,
            BannerColor::Red => RED,
            BannerColor::Yellow => YELLOW,
        }
    }
}

/// Get the current terminal width for banner display.
fn get_terminal_width_for_banner() -> usize {
    terminal_size()
        .map(|(Width(w), _)| w as usize)
        .unwrap_or(DEFAULT_TERMINAL_WIDTH as usize)
}

/// Print a color-coded phase banner.
///
/// Banner format: `━━━ PHASE_NAME ━━━` with appropriate color.
/// The banner width adapts to terminal width (clamped between MIN and MAX).
///
/// # Arguments
///
/// * `phase_name` - The name of the phase (e.g., "RUNNING", "REVIEWING")
/// * `color` - The color to use for the banner
pub fn print_phase_banner(phase_name: &str, color: BannerColor) {
    let terminal_width = get_terminal_width_for_banner();

    // Clamp banner width between MIN and MAX
    let banner_width = terminal_width.clamp(MIN_BANNER_WIDTH, MAX_BANNER_WIDTH);

    // Calculate padding: " PHASE_NAME " has phase_name.len() + 2 spaces
    let phase_with_spaces = format!(" {} ", phase_name);
    let phase_len = phase_with_spaces.chars().count();

    // Calculate how many ━ characters we need on each side
    let remaining = banner_width.saturating_sub(phase_len);
    let left_padding = remaining / 2;
    let right_padding = remaining - left_padding;

    let color_code = color.ansi_code();

    println!(
        "{}{BOLD}{}{}{}{}",
        color_code,
        "━".repeat(left_padding),
        phase_with_spaces,
        "━".repeat(right_padding),
        RESET
    );
}

/// Print a phase footer (bottom border) to visually close the output section.
///
/// The footer is a horizontal line using the same style as the phase banner,
/// providing visual framing around the Claude output section.
///
/// # Arguments
///
/// * `color` - The color to use for the footer (should match the phase banner)
pub fn print_phase_footer(color: BannerColor) {
    let terminal_width = get_terminal_width_for_banner();

    // Clamp banner width between MIN and MAX (same as phase banner)
    let banner_width = terminal_width.clamp(MIN_BANNER_WIDTH, MAX_BANNER_WIDTH);

    let color_code = color.ansi_code();

    println!("{}{BOLD}{}{RESET}", color_code, "━".repeat(banner_width));
    // Print blank line for padding after the frame
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_banner_color_ansi_codes() {
        assert_eq!(BannerColor::Cyan.ansi_code(), CYAN);
        assert_eq!(BannerColor::Green.ansi_code(), GREEN);
        assert_eq!(BannerColor::Red.ansi_code(), RED);
        assert_eq!(BannerColor::Yellow.ansi_code(), YELLOW);
    }

    #[test]
    fn test_banner_color_equality() {
        assert_eq!(BannerColor::Cyan, BannerColor::Cyan);
        assert_ne!(BannerColor::Cyan, BannerColor::Green);
    }

    #[test]
    fn test_get_terminal_width_returns_valid_width() {
        let width = get_terminal_width_for_banner();
        assert!(width >= MIN_BANNER_WIDTH);
    }

    #[test]
    fn test_banner_width_clamping() {
        assert!(MIN_BANNER_WIDTH < MAX_BANNER_WIDTH);
        assert_eq!(MIN_BANNER_WIDTH, 20);
        assert_eq!(MAX_BANNER_WIDTH, 80);
    }

    #[test]
    fn test_print_phase_banner_all_colors_and_phases() {
        let test_cases: &[(&str, BannerColor)] = &[
            ("RUNNING", BannerColor::Cyan),
            ("REVIEWING", BannerColor::Cyan),
            ("CORRECTING", BannerColor::Yellow),
            ("COMMITTING", BannerColor::Cyan),
            ("SUCCESS", BannerColor::Green),
            ("FAILURE", BannerColor::Red),
        ];

        for (phase_name, color) in test_cases {
            print_phase_banner(phase_name, *color);
        }
    }

    #[test]
    fn test_print_phase_banner_edge_cases() {
        // Empty name should not panic
        print_phase_banner("", BannerColor::Cyan);

        // Very long name should not panic
        print_phase_banner(
            "THIS_IS_A_VERY_LONG_PHASE_NAME_THAT_EXCEEDS_NORMAL_LENGTH",
            BannerColor::Cyan,
        );
    }
}
