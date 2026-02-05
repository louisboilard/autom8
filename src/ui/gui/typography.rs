//! Typography configuration for the GUI.
//!
//! This module provides custom font embedding and type scale definitions
//! for consistent visual hierarchy throughout the application.

use eframe::egui::{self, FontData, FontDefinitions, FontFamily, FontId, TextStyle};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Embedded Geist Sans Regular font data.
const GEIST_REGULAR: &[u8] = include_bytes!("fonts/Geist-Regular.ttf");

/// Embedded Geist Sans Medium font data.
const GEIST_MEDIUM: &[u8] = include_bytes!("fonts/Geist-Medium.ttf");

/// Embedded Geist Sans SemiBold font data.
const GEIST_SEMIBOLD: &[u8] = include_bytes!("fonts/Geist-SemiBold.ttf");

/// Embedded Geist Mono Regular font data.
const GEIST_MONO_REGULAR: &[u8] = include_bytes!("fonts/GeistMono-Regular.ttf");

/// Font family identifier for Geist Sans Regular.
pub const FAMILY_GEIST_REGULAR: &str = "Geist-Regular";

/// Font family identifier for Geist Sans Medium.
pub const FAMILY_GEIST_MEDIUM: &str = "Geist-Medium";

/// Font family identifier for Geist Sans SemiBold.
pub const FAMILY_GEIST_SEMIBOLD: &str = "Geist-SemiBold";

/// Font family identifier for Geist Mono.
pub const FAMILY_GEIST_MONO: &str = "Geist-Mono";

/// Standard font sizes for the type scale.
///
/// This defines a consistent set of sizes used throughout the application
/// to maintain visual hierarchy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FontSize {
    /// Extra small text for captions or tertiary information (10px).
    Caption,
    /// Small text for labels and secondary information (12px).
    Small,
    /// Standard body text size (14px).
    Body,
    /// Slightly larger text for emphasis (16px).
    Large,
    /// Section headings (18px).
    Heading,
    /// Page or view titles (24px).
    Title,
    /// Large display text (32px).
    Display,
}

impl FontSize {
    /// Returns the pixel size for this font size.
    pub fn pixels(self) -> f32 {
        match self {
            FontSize::Caption => 10.0,
            FontSize::Small => 12.0,
            FontSize::Body => 14.0,
            FontSize::Large => 16.0,
            FontSize::Heading => 18.0,
            FontSize::Title => 24.0,
            FontSize::Display => 32.0,
        }
    }
}

/// Font weight variants available in the application.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FontWeight {
    /// Regular weight (400).
    Regular,
    /// Medium weight (500).
    Medium,
    /// SemiBold weight (600).
    SemiBold,
}

impl FontWeight {
    /// Returns the FontFamily for this weight.
    pub fn family(self) -> FontFamily {
        match self {
            FontWeight::Regular => FontFamily::Name(FAMILY_GEIST_REGULAR.into()),
            FontWeight::Medium => FontFamily::Name(FAMILY_GEIST_MEDIUM.into()),
            FontWeight::SemiBold => FontFamily::Name(FAMILY_GEIST_SEMIBOLD.into()),
        }
    }
}

/// Create a FontId with the specified size and weight.
pub fn font(size: FontSize, weight: FontWeight) -> FontId {
    FontId::new(size.pixels(), weight.family())
}

/// Create a FontId for monospace text at the specified size.
pub fn mono(size: FontSize) -> FontId {
    FontId::new(size.pixels(), FontFamily::Name(FAMILY_GEIST_MONO.into()))
}

/// Returns the approximate line height for a given font size.
///
/// Uses a standard 1.4x multiplier on the font pixel size to account
/// for line spacing.
pub fn line_height(size: FontSize) -> f32 {
    size.pixels() * 1.4
}

/// Create FontDefinitions with embedded Geist fonts.
///
/// This function configures egui to use the Geist font family as the default
/// for all text rendering. It sets up:
/// - Geist Sans (Regular, Medium, SemiBold) as custom font families
/// - Geist Mono as the monospace font
/// - Default text styles mapped to appropriate sizes and weights
pub fn configure_fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();

    // Insert font data wrapped in Arc
    fonts.font_data.insert(
        FAMILY_GEIST_REGULAR.to_owned(),
        Arc::new(FontData::from_static(GEIST_REGULAR)),
    );
    fonts.font_data.insert(
        FAMILY_GEIST_MEDIUM.to_owned(),
        Arc::new(FontData::from_static(GEIST_MEDIUM)),
    );
    fonts.font_data.insert(
        FAMILY_GEIST_SEMIBOLD.to_owned(),
        Arc::new(FontData::from_static(GEIST_SEMIBOLD)),
    );
    fonts.font_data.insert(
        FAMILY_GEIST_MONO.to_owned(),
        Arc::new(FontData::from_static(GEIST_MONO_REGULAR)),
    );

    // Create custom font families
    fonts.families.insert(
        FontFamily::Name(FAMILY_GEIST_REGULAR.into()),
        vec![FAMILY_GEIST_REGULAR.to_owned()],
    );
    fonts.families.insert(
        FontFamily::Name(FAMILY_GEIST_MEDIUM.into()),
        vec![FAMILY_GEIST_MEDIUM.to_owned()],
    );
    fonts.families.insert(
        FontFamily::Name(FAMILY_GEIST_SEMIBOLD.into()),
        vec![FAMILY_GEIST_SEMIBOLD.to_owned()],
    );
    fonts.families.insert(
        FontFamily::Name(FAMILY_GEIST_MONO.into()),
        vec![FAMILY_GEIST_MONO.to_owned()],
    );

    // Set Geist Regular as the default proportional font
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, FAMILY_GEIST_REGULAR.to_owned());

    // Set Geist Mono as the default monospace font
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, FAMILY_GEIST_MONO.to_owned());

    fonts
}

/// Configure default text styles with the custom type scale.
///
/// This maps egui's built-in TextStyle variants to appropriate font sizes
/// and weights from our type system.
pub fn configure_text_styles(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();

    // Map TextStyles to our type scale
    let mut text_styles = BTreeMap::new();
    text_styles.insert(
        TextStyle::Small,
        FontId::new(FontSize::Small.pixels(), FontFamily::Proportional),
    );
    text_styles.insert(
        TextStyle::Body,
        FontId::new(FontSize::Body.pixels(), FontFamily::Proportional),
    );
    text_styles.insert(
        TextStyle::Button,
        FontId::new(FontSize::Body.pixels(), FontWeight::Medium.family()),
    );
    text_styles.insert(
        TextStyle::Heading,
        FontId::new(FontSize::Heading.pixels(), FontWeight::SemiBold.family()),
    );
    text_styles.insert(
        TextStyle::Monospace,
        FontId::new(FontSize::Body.pixels(), FontFamily::Monospace),
    );

    style.text_styles = text_styles;
    ctx.set_style(style);
}

/// Initialize typography for the application.
///
/// Call this during app initialization (in the CreationContext callback)
/// to set up custom fonts and text styles.
pub fn init(ctx: &egui::Context) {
    ctx.set_fonts(configure_fonts());
    configure_text_styles(ctx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_font_size_pixels() {
        assert_eq!(FontSize::Caption.pixels(), 10.0);
        assert_eq!(FontSize::Small.pixels(), 12.0);
        assert_eq!(FontSize::Body.pixels(), 14.0);
        assert_eq!(FontSize::Large.pixels(), 16.0);
        assert_eq!(FontSize::Heading.pixels(), 18.0);
        assert_eq!(FontSize::Title.pixels(), 24.0);
        assert_eq!(FontSize::Display.pixels(), 32.0);
    }

    #[test]
    fn test_font_weight_family() {
        assert_eq!(
            FontWeight::Regular.family(),
            FontFamily::Name(FAMILY_GEIST_REGULAR.into())
        );
        assert_eq!(
            FontWeight::Medium.family(),
            FontFamily::Name(FAMILY_GEIST_MEDIUM.into())
        );
        assert_eq!(
            FontWeight::SemiBold.family(),
            FontFamily::Name(FAMILY_GEIST_SEMIBOLD.into())
        );
    }

    #[test]
    fn test_font_helper() {
        let font_id = font(FontSize::Body, FontWeight::Regular);
        assert_eq!(font_id.size, 14.0);
        assert_eq!(
            font_id.family,
            FontFamily::Name(FAMILY_GEIST_REGULAR.into())
        );
    }

    #[test]
    fn test_mono_helper() {
        let font_id = mono(FontSize::Body);
        assert_eq!(font_id.size, 14.0);
        assert_eq!(font_id.family, FontFamily::Name(FAMILY_GEIST_MONO.into()));
    }

    #[test]
    fn test_configure_fonts_has_all_families() {
        let fonts = configure_fonts();

        // Check that all font data is loaded
        assert!(fonts.font_data.contains_key(FAMILY_GEIST_REGULAR));
        assert!(fonts.font_data.contains_key(FAMILY_GEIST_MEDIUM));
        assert!(fonts.font_data.contains_key(FAMILY_GEIST_SEMIBOLD));
        assert!(fonts.font_data.contains_key(FAMILY_GEIST_MONO));

        // Check that custom families are defined
        assert!(fonts
            .families
            .contains_key(&FontFamily::Name(FAMILY_GEIST_REGULAR.into())));
        assert!(fonts
            .families
            .contains_key(&FontFamily::Name(FAMILY_GEIST_MEDIUM.into())));
        assert!(fonts
            .families
            .contains_key(&FontFamily::Name(FAMILY_GEIST_SEMIBOLD.into())));
        assert!(fonts
            .families
            .contains_key(&FontFamily::Name(FAMILY_GEIST_MONO.into())));

        // Check that default families have Geist fonts as primary
        let proportional = fonts.families.get(&FontFamily::Proportional).unwrap();
        assert_eq!(proportional.first().unwrap(), FAMILY_GEIST_REGULAR);

        let monospace = fonts.families.get(&FontFamily::Monospace).unwrap();
        assert_eq!(monospace.first().unwrap(), FAMILY_GEIST_MONO);
    }
}
