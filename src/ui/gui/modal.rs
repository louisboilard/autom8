//! Reusable modal dialog component for the GUI.
//!
//! This module provides a generic modal dialog component that can be used
//! for confirmation dialogs, alerts, and other modal interactions.
//!
//! # Features
//!
//! - Semi-transparent backdrop that captures clicks
//! - Centered dialog with configurable title and message
//! - Cancel and confirm buttons with customizable labels and colors
//! - Escape key dismisses the modal
//! - Follows the application theme (colors, spacing, typography, shadows)
//!
//! # Example
//!
//! ```ignore
//! use crate::ui::gui::modal::{Modal, ModalAction, ModalButton};
//!
//! let modal = Modal::new("Confirm Delete")
//!     .message("Are you sure you want to delete this item?")
//!     .cancel_button(ModalButton::default())
//!     .confirm_button(
//!         ModalButton::new("Delete")
//!             .color(colors::STATUS_ERROR)
//!     );
//!
//! let action = modal.show(ctx);
//! match action {
//!     ModalAction::Confirmed => { /* handle confirm */ }
//!     ModalAction::Cancelled => { /* handle cancel */ }
//!     ModalAction::None => { /* still open */ }
//! }
//! ```

use eframe::egui::{self, Color32, Key, Order, Pos2, Rounding, Sense, Stroke};

use crate::ui::gui::theme::{colors, rounding, shadow, spacing};
use crate::ui::gui::typography::{self, FontSize, FontWeight};

// ============================================================================
// Constants
// ============================================================================

/// Default width of the modal dialog.
const DIALOG_WIDTH: f32 = 400.0;

/// Padding inside the dialog. Uses spacing::XL for consistency with theme.
const DIALOG_PADDING: f32 = spacing::XL;

/// Height of the action buttons.
const BUTTON_HEIGHT: f32 = 36.0;

/// Width of the action buttons.
const BUTTON_WIDTH: f32 = 100.0;

/// Gap between buttons. Uses spacing::MD for consistency with theme.
const BUTTON_GAP: f32 = spacing::MD;

/// Backdrop opacity (0-255).
const BACKDROP_ALPHA: u8 = 128;

// ============================================================================
// Modal Button Configuration
// ============================================================================

/// Configuration for a modal button.
#[derive(Debug, Clone)]
pub struct ModalButton {
    /// The label text displayed on the button.
    pub label: String,
    /// The background color of the button.
    pub fill_color: Color32,
    /// The text color of the button.
    pub text_color: Color32,
    /// Optional border stroke.
    pub stroke: Option<Stroke>,
}

impl ModalButton {
    /// Create a new modal button with the given label.
    ///
    /// Default styling is a primary button with accent color.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            fill_color: colors::ACCENT,
            text_color: Color32::WHITE,
            stroke: None,
        }
    }

    /// Set the background fill color.
    pub fn color(mut self, color: Color32) -> Self {
        self.fill_color = color;
        self
    }

    /// Set the text color.
    pub fn text_color(mut self, color: Color32) -> Self {
        self.text_color = color;
        self
    }

    /// Add a border stroke.
    pub fn stroke(mut self, stroke: Stroke) -> Self {
        self.stroke = Some(stroke);
        self
    }

    /// Create a secondary/cancel style button.
    ///
    /// Uses a neutral background with border.
    pub fn secondary(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            fill_color: colors::SURFACE_ELEVATED,
            text_color: colors::TEXT_PRIMARY,
            stroke: Some(Stroke::new(1.0, colors::BORDER)),
        }
    }

    /// Create a destructive/danger style button.
    ///
    /// Uses red background with white text.
    pub fn destructive(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            fill_color: colors::STATUS_ERROR,
            text_color: Color32::WHITE,
            stroke: None,
        }
    }
}

impl Default for ModalButton {
    fn default() -> Self {
        Self::secondary("Cancel")
    }
}

// ============================================================================
// Modal Action Result
// ============================================================================

/// The result of showing a modal dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalAction {
    /// The user confirmed the action.
    Confirmed,
    /// The user cancelled the action (button click, backdrop click, or Escape key).
    Cancelled,
    /// No action taken; the modal is still open.
    None,
}

impl ModalAction {
    /// Returns true if the user confirmed.
    pub fn is_confirmed(&self) -> bool {
        matches!(self, ModalAction::Confirmed)
    }

    /// Returns true if the user cancelled.
    pub fn is_cancelled(&self) -> bool {
        matches!(self, ModalAction::Cancelled)
    }

    /// Returns true if the modal is still open (no action taken).
    pub fn is_open(&self) -> bool {
        matches!(self, ModalAction::None)
    }
}

// ============================================================================
// Modal Component
// ============================================================================

/// A reusable modal dialog component.
///
/// The modal renders with a semi-transparent backdrop that captures clicks,
/// a centered dialog box with title, message, and action buttons.
///
/// # Usage
///
/// Create a modal with [`Modal::new`], configure it with builder methods,
/// then call [`Modal::show`] to render it and get the user's action.
#[derive(Debug, Clone)]
pub struct Modal {
    /// Unique identifier for the modal (used for egui Area IDs).
    id: String,
    /// The title displayed at the top of the modal.
    title: String,
    /// The message body displayed below the title.
    message: String,
    /// Configuration for the cancel button.
    /// When None, only the confirm button is shown (for result/info modals).
    cancel_button: Option<ModalButton>,
    /// Configuration for the confirm button.
    confirm_button: ModalButton,
    /// Width of the dialog.
    width: f32,
}

impl Modal {
    /// Create a new modal with the given title.
    ///
    /// The modal is created with default settings:
    /// - Default "Cancel" button (secondary style)
    /// - Default "Confirm" button (primary style)
    /// - Standard dialog width
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            id: "modal".to_string(),
            title: title.into(),
            message: String::new(),
            cancel_button: Some(ModalButton::secondary("Cancel")),
            confirm_button: ModalButton::new("Confirm"),
            width: DIALOG_WIDTH,
        }
    }

    /// Set a unique ID for the modal.
    ///
    /// Use this when you need to have multiple modals that might be shown
    /// in the same context.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    /// Set the message body of the modal.
    pub fn message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }

    /// Set the cancel button configuration.
    pub fn cancel_button(mut self, button: ModalButton) -> Self {
        self.cancel_button = Some(button);
        self
    }

    /// Remove the cancel button (for single-button modals).
    ///
    /// US-007: Result modals only need an OK button to dismiss.
    pub fn no_cancel_button(mut self) -> Self {
        self.cancel_button = None;
        self
    }

    /// Set the confirm button configuration.
    pub fn confirm_button(mut self, button: ModalButton) -> Self {
        self.confirm_button = button;
        self
    }

    /// Set the dialog width.
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// Show the modal and return the user's action.
    ///
    /// Returns:
    /// - [`ModalAction::Confirmed`] if the confirm button was clicked
    /// - [`ModalAction::Cancelled`] if the cancel button was clicked,
    ///   the backdrop was clicked, or the Escape key was pressed
    /// - [`ModalAction::None`] if the modal is still open
    pub fn show(&self, ctx: &egui::Context) -> ModalAction {
        let mut action = ModalAction::None;

        // Render backdrop
        self.render_backdrop(ctx, &mut action);

        // Render dialog
        self.render_dialog(ctx, &mut action);

        // Handle Escape key
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            action = ModalAction::Cancelled;
        }

        action
    }

    /// Render the semi-transparent backdrop.
    fn render_backdrop(&self, ctx: &egui::Context, action: &mut ModalAction) {
        let screen_rect = ctx.screen_rect();

        egui::Area::new(egui::Id::new(format!("{}_backdrop", self.id)))
            .order(Order::Foreground)
            .fixed_pos(Pos2::ZERO)
            .show(ctx, |ui| {
                // Draw semi-transparent backdrop
                ui.painter().rect_filled(
                    screen_rect,
                    Rounding::ZERO,
                    Color32::from_rgba_unmultiplied(0, 0, 0, BACKDROP_ALPHA),
                );

                // Capture clicks on backdrop
                let (_, response) = ui.allocate_exact_size(screen_rect.size(), Sense::click());
                if response.clicked() {
                    *action = ModalAction::Cancelled;
                }
            });
    }

    /// Render the dialog box.
    fn render_dialog(&self, ctx: &egui::Context, action: &mut ModalAction) {
        let screen_rect = ctx.screen_rect();

        // Calculate dialog position (centered)
        let dialog_x = (screen_rect.width() - self.width) / 2.0;

        // Estimate dialog height for vertical centering
        // Title + message + button row + padding
        let estimated_height = 200.0;
        let dialog_y = (screen_rect.height() - estimated_height) / 2.0;

        let dialog_pos = Pos2::new(dialog_x, dialog_y);

        egui::Area::new(egui::Id::new(format!("{}_dialog", self.id)))
            .order(Order::Foreground)
            .fixed_pos(dialog_pos)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(colors::SURFACE)
                    .rounding(Rounding::same(rounding::CARD))
                    .shadow(shadow::elevated())
                    .stroke(Stroke::new(1.0, colors::BORDER))
                    .inner_margin(egui::Margin::same(DIALOG_PADDING))
                    .show(ui, |ui| {
                        let inner_width = self.width - 2.0 * DIALOG_PADDING;
                        ui.set_min_width(inner_width);
                        ui.set_max_width(inner_width);

                        // Title
                        ui.label(
                            egui::RichText::new(&self.title)
                                .font(typography::font(FontSize::Heading, FontWeight::SemiBold))
                                .color(colors::TEXT_PRIMARY),
                        );

                        ui.add_space(spacing::MD);

                        // Message
                        if !self.message.is_empty() {
                            ui.label(
                                egui::RichText::new(&self.message)
                                    .font(typography::font(FontSize::Body, FontWeight::Regular))
                                    .color(colors::TEXT_SECONDARY),
                            );
                        }

                        ui.add_space(spacing::XL);

                        // Button row (right-aligned)
                        ui.horizontal(|ui| {
                            // Calculate button layout based on whether cancel button exists
                            let button_count = if self.cancel_button.is_some() { 2 } else { 1 };
                            let total_button_width = button_count as f32 * BUTTON_WIDTH
                                + (button_count - 1) as f32 * BUTTON_GAP;
                            let available = ui.available_width() - total_button_width;
                            ui.add_space(available.max(0.0));

                            // Cancel button (optional)
                            if let Some(cancel_btn) = &self.cancel_button {
                                let cancel_response = self.render_button(ui, cancel_btn);
                                if cancel_response.clicked() {
                                    *action = ModalAction::Cancelled;
                                }
                                ui.add_space(BUTTON_GAP);
                            }

                            // Confirm button
                            let confirm_response = self.render_button(ui, &self.confirm_button);
                            if confirm_response.clicked() {
                                *action = ModalAction::Confirmed;
                            }
                        });
                    });
            });
    }

    /// Render a single button and return its response.
    fn render_button(&self, ui: &mut egui::Ui, button: &ModalButton) -> egui::Response {
        let mut btn = egui::Button::new(
            egui::RichText::new(&button.label)
                .font(typography::font(FontSize::Body, FontWeight::Medium))
                .color(button.text_color),
        )
        .fill(button.fill_color)
        .rounding(Rounding::same(rounding::BUTTON));

        if let Some(stroke) = button.stroke {
            btn = btn.stroke(stroke);
        }

        ui.add_sized([BUTTON_WIDTH, BUTTON_HEIGHT], btn)
    }
}

// ============================================================================
// Convenience Functions
// ============================================================================

/// Show a simple confirmation dialog.
///
/// This is a convenience function for common confirmation patterns.
///
/// # Arguments
///
/// * `ctx` - The egui context
/// * `id` - Unique identifier for the modal
/// * `title` - The dialog title
/// * `message` - The dialog message
/// * `confirm_label` - Label for the confirm button
/// * `destructive` - If true, the confirm button will be styled as destructive (red)
///
/// # Returns
///
/// The user's action (confirmed, cancelled, or none if still open).
pub fn confirmation_dialog(
    ctx: &egui::Context,
    id: &str,
    title: &str,
    message: &str,
    confirm_label: &str,
    destructive: bool,
) -> ModalAction {
    let confirm_button = if destructive {
        ModalButton::destructive(confirm_label)
    } else {
        ModalButton::new(confirm_label)
    };

    Modal::new(title)
        .id(id)
        .message(message)
        .confirm_button(confirm_button)
        .show(ctx)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modal_button_new() {
        let button = ModalButton::new("Test");
        assert_eq!(button.label, "Test");
        assert_eq!(button.fill_color, colors::ACCENT);
        assert_eq!(button.text_color, Color32::WHITE);
        assert!(button.stroke.is_none());
    }

    #[test]
    fn test_modal_button_color() {
        let button = ModalButton::new("Test").color(colors::STATUS_SUCCESS);
        assert_eq!(button.fill_color, colors::STATUS_SUCCESS);
    }

    #[test]
    fn test_modal_button_text_color() {
        let button = ModalButton::new("Test").text_color(colors::TEXT_PRIMARY);
        assert_eq!(button.text_color, colors::TEXT_PRIMARY);
    }

    #[test]
    fn test_modal_button_stroke() {
        let stroke = Stroke::new(2.0, colors::BORDER);
        let button = ModalButton::new("Test").stroke(stroke);
        assert_eq!(button.stroke, Some(stroke));
    }

    #[test]
    fn test_modal_button_secondary() {
        let button = ModalButton::secondary("Cancel");
        assert_eq!(button.label, "Cancel");
        assert_eq!(button.fill_color, colors::SURFACE_ELEVATED);
        assert_eq!(button.text_color, colors::TEXT_PRIMARY);
        assert!(button.stroke.is_some());
    }

    #[test]
    fn test_modal_button_destructive() {
        let button = ModalButton::destructive("Delete");
        assert_eq!(button.label, "Delete");
        assert_eq!(button.fill_color, colors::STATUS_ERROR);
        assert_eq!(button.text_color, Color32::WHITE);
        assert!(button.stroke.is_none());
    }

    #[test]
    fn test_modal_button_default() {
        let button = ModalButton::default();
        assert_eq!(button.label, "Cancel");
        assert_eq!(button.fill_color, colors::SURFACE_ELEVATED);
    }

    #[test]
    fn test_modal_action_is_confirmed() {
        assert!(ModalAction::Confirmed.is_confirmed());
        assert!(!ModalAction::Cancelled.is_confirmed());
        assert!(!ModalAction::None.is_confirmed());
    }

    #[test]
    fn test_modal_action_is_cancelled() {
        assert!(!ModalAction::Confirmed.is_cancelled());
        assert!(ModalAction::Cancelled.is_cancelled());
        assert!(!ModalAction::None.is_cancelled());
    }

    #[test]
    fn test_modal_action_is_open() {
        assert!(!ModalAction::Confirmed.is_open());
        assert!(!ModalAction::Cancelled.is_open());
        assert!(ModalAction::None.is_open());
    }

    #[test]
    fn test_modal_new() {
        let modal = Modal::new("Test Title");
        assert_eq!(modal.title, "Test Title");
        assert_eq!(modal.message, "");
        assert_eq!(modal.id, "modal");
        assert_eq!(modal.width, DIALOG_WIDTH);
    }

    #[test]
    fn test_modal_id() {
        let modal = Modal::new("Test").id("custom_id");
        assert_eq!(modal.id, "custom_id");
    }

    #[test]
    fn test_modal_message() {
        let modal = Modal::new("Test").message("Test message body");
        assert_eq!(modal.message, "Test message body");
    }

    #[test]
    fn test_modal_cancel_button() {
        let modal = Modal::new("Test").cancel_button(ModalButton::new("Back"));
        assert_eq!(modal.cancel_button.as_ref().unwrap().label, "Back");
    }

    #[test]
    fn test_modal_no_cancel_button() {
        let modal = Modal::new("Test").no_cancel_button();
        assert!(modal.cancel_button.is_none());
    }

    #[test]
    fn test_modal_confirm_button() {
        let modal = Modal::new("Test").confirm_button(ModalButton::destructive("Delete"));
        assert_eq!(modal.confirm_button.label, "Delete");
        assert_eq!(modal.confirm_button.fill_color, colors::STATUS_ERROR);
    }

    #[test]
    fn test_modal_width() {
        let modal = Modal::new("Test").width(500.0);
        assert_eq!(modal.width, 500.0);
    }

    #[test]
    fn test_modal_builder_chain() {
        let modal = Modal::new("Confirm Delete")
            .id("delete_modal")
            .message("Are you sure?")
            .cancel_button(ModalButton::secondary("No"))
            .confirm_button(ModalButton::destructive("Yes, Delete"))
            .width(450.0);

        assert_eq!(modal.title, "Confirm Delete");
        assert_eq!(modal.id, "delete_modal");
        assert_eq!(modal.message, "Are you sure?");
        assert_eq!(modal.cancel_button.as_ref().unwrap().label, "No");
        assert_eq!(modal.confirm_button.label, "Yes, Delete");
        assert_eq!(modal.width, 450.0);
    }
}
