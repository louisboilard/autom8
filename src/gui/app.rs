//! GUI application entry point.
//!
//! This module contains the eframe application setup and main window
//! configuration for the autom8 GUI.

use crate::error::{Autom8Error, Result};
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
            ui.heading("autom8");

            ui.add_space(20.0);

            if let Some(ref filter) = self.project_filter {
                ui.label(format!("Filtering: {}", filter));
            } else {
                ui.label("Monitoring all projects");
            }

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(10.0);

            ui.label("GUI initialized successfully. Ready for implementation.");
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
        Box::new(|_cc| Ok(Box::new(Autom8App::new(project_filter)))),
    )
    .map_err(|e| Autom8Error::GuiError(e.to_string()))
}
