//! GUI application entry point.
//!
//! This module contains the eframe application setup and main window
//! configuration for the autom8 GUI.

use crate::error::{Autom8Error, Result};
use crate::gui::theme::{self, colors, Status};
use crate::gui::typography::{self, FontSize, FontWeight};
use eframe::egui;

/// Default window width in pixels.
const DEFAULT_WIDTH: f32 = 1200.0;

/// Default window height in pixels.
const DEFAULT_HEIGHT: f32 = 800.0;

/// Minimum window width in pixels.
const MIN_WIDTH: f32 = 800.0;

/// Minimum window height in pixels.
const MIN_HEIGHT: f32 = 600.0;

/// The main GUI application state.
pub struct Autom8App {
    /// Optional project filter to show only a specific project.
    project_filter: Option<String>,
}

impl Autom8App {
    /// Create a new application instance.
    ///
    /// # Arguments
    ///
    /// * `project_filter` - Optional project name to filter the view
    pub fn new(project_filter: Option<String>) -> Self {
        Self { project_filter }
    }
}

impl eframe::App for Autom8App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            // Title using custom typography with primary text color
            ui.label(
                egui::RichText::new("autom8")
                    .font(typography::font(FontSize::Title, FontWeight::SemiBold))
                    .color(colors::TEXT_PRIMARY),
            );

            ui.add_space(20.0);

            if let Some(ref filter) = self.project_filter {
                ui.label(
                    egui::RichText::new(format!("Filtering: {}", filter))
                        .font(typography::font(FontSize::Body, FontWeight::Regular))
                        .color(colors::TEXT_SECONDARY),
                );
            } else {
                ui.label(
                    egui::RichText::new("Monitoring all projects")
                        .font(typography::font(FontSize::Body, FontWeight::Regular))
                        .color(colors::TEXT_SECONDARY),
                );
            }

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(16.0);

            // Status indicators demonstration
            ui.label(
                egui::RichText::new("Status Colors")
                    .font(typography::font(FontSize::Heading, FontWeight::SemiBold))
                    .color(colors::TEXT_PRIMARY),
            );
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                Self::status_indicator(ui, Status::Running, "Running");
                ui.add_space(16.0);
                Self::status_indicator(ui, Status::Success, "Success");
                ui.add_space(16.0);
                Self::status_indicator(ui, Status::Warning, "Warning");
                ui.add_space(16.0);
                Self::status_indicator(ui, Status::Error, "Error");
                ui.add_space(16.0);
                Self::status_indicator(ui, Status::Idle, "Idle");
            });

            ui.add_space(16.0);

            ui.label(
                egui::RichText::new("Theme initialized. Ready for implementation.")
                    .font(typography::font(FontSize::Body, FontWeight::Regular))
                    .color(colors::TEXT_MUTED),
            );
        });
    }
}

impl Autom8App {
    /// Render a status indicator with a colored dot and label.
    fn status_indicator(ui: &mut egui::Ui, status: Status, label: &str) {
        ui.horizontal(|ui| {
            // Draw colored dot
            let (rect, _response) =
                ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
            ui.painter()
                .circle_filled(rect.center(), 4.0, status.color());

            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(label)
                    .font(typography::font(FontSize::Small, FontWeight::Regular))
                    .color(colors::TEXT_PRIMARY),
            );
        });
    }
}

/// Launch the native GUI application.
///
/// Opens a native window using eframe with the specified configuration.
///
/// # Arguments
///
/// * `project_filter` - Optional project name to filter the view
///
/// # Returns
///
/// * `Ok(())` when the user closes the window
/// * `Err(Autom8Error)` if the GUI fails to initialize
pub fn run_gui(project_filter: Option<String>) -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("autom8")
            .with_inner_size([DEFAULT_WIDTH, DEFAULT_HEIGHT])
            .with_min_inner_size([MIN_WIDTH, MIN_HEIGHT]),
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
            Ok(Box::new(Autom8App::new(project_filter)))
        }),
    )
    .map_err(|e| Autom8Error::GuiError(e.to_string()))
}
