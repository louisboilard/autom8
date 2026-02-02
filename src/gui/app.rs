//! GUI application entry point.
//!
//! This module contains the eframe application setup and main window
//! configuration for the autom8 GUI.

use crate::error::{Autom8Error, Result};
use crate::gui::theme::{self, colors, rounding};
use crate::gui::typography::{self, FontSize, FontWeight};
use eframe::egui::{self, Color32, Rounding, Sense, Stroke};

/// Default window width in pixels.
const DEFAULT_WIDTH: f32 = 1200.0;

/// Default window height in pixels.
const DEFAULT_HEIGHT: f32 = 800.0;

/// Minimum window width in pixels.
const MIN_WIDTH: f32 = 800.0;

/// Minimum window height in pixels.
const MIN_HEIGHT: f32 = 600.0;

/// Height of the header/tab bar area.
const HEADER_HEIGHT: f32 = 48.0;

/// Horizontal padding within the header.
const HEADER_PADDING_H: f32 = 16.0;

/// Tab indicator underline height.
const TAB_UNDERLINE_HEIGHT: f32 = 2.0;

/// Tab horizontal padding.
const TAB_PADDING_H: f32 = 16.0;

/// Space between tabs.
const TAB_SPACING: f32 = 4.0;

/// The available tabs in the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    /// View of currently active runs.
    #[default]
    ActiveRuns,
    /// View of projects.
    Projects,
}

impl Tab {
    /// Returns the display label for this tab.
    pub fn label(self) -> &'static str {
        match self {
            Tab::ActiveRuns => "Active Runs",
            Tab::Projects => "Projects",
        }
    }

    /// Returns all available tabs.
    pub fn all() -> &'static [Tab] {
        &[Tab::ActiveRuns, Tab::Projects]
    }
}

/// The main GUI application state.
pub struct Autom8App {
    /// Optional project filter to show only a specific project.
    project_filter: Option<String>,
    /// Currently selected tab.
    current_tab: Tab,
}

impl Autom8App {
    /// Create a new application instance.
    ///
    /// # Arguments
    ///
    /// * `project_filter` - Optional project name to filter the view
    pub fn new(project_filter: Option<String>) -> Self {
        Self {
            project_filter,
            current_tab: Tab::default(),
        }
    }

    /// Returns the currently selected tab.
    pub fn current_tab(&self) -> Tab {
        self.current_tab
    }
}

impl eframe::App for Autom8App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Header with tab bar
        egui::TopBottomPanel::top("header")
            .exact_height(HEADER_HEIGHT)
            .frame(egui::Frame::none().fill(colors::SURFACE).inner_margin(
                egui::Margin::symmetric(HEADER_PADDING_H, 0.0),
            ))
            .show(ctx, |ui| {
                self.render_header(ui);
            });

        // Content area fills remaining space
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(colors::BACKGROUND)
                    .inner_margin(egui::Margin::same(16.0)),
            )
            .show(ctx, |ui| {
                self.render_content(ui);
            });
    }
}

impl Autom8App {
    /// Render the header area with tab bar.
    fn render_header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            ui.add_space(TAB_SPACING);

            for tab in Tab::all() {
                let is_active = *tab == self.current_tab;
                if self.render_tab(ui, *tab, is_active) {
                    self.current_tab = *tab;
                }
                ui.add_space(TAB_SPACING);
            }
        });

        // Draw bottom border for header
        let rect = ui.max_rect();
        ui.painter().hline(
            rect.x_range(),
            rect.bottom(),
            Stroke::new(1.0, colors::BORDER),
        );
    }

    /// Render a single tab button. Returns true if clicked.
    fn render_tab(&self, ui: &mut egui::Ui, tab: Tab, is_active: bool) -> bool {
        let label = tab.label();

        // Calculate tab size
        let text_galley = ui.fonts(|f| {
            f.layout_no_wrap(
                label.to_string(),
                typography::font(FontSize::Body, FontWeight::Medium),
                colors::TEXT_PRIMARY,
            )
        });
        let text_size = text_galley.size();
        let tab_size = egui::vec2(
            text_size.x + TAB_PADDING_H * 2.0,
            HEADER_HEIGHT - TAB_UNDERLINE_HEIGHT,
        );

        // Allocate space for the tab
        let (rect, response) = ui.allocate_exact_size(tab_size, Sense::click());

        // Determine visual state
        let is_hovered = response.hovered();

        // Draw tab background on hover (subtle)
        if is_hovered && !is_active {
            ui.painter().rect_filled(
                rect,
                Rounding::same(rounding::BUTTON),
                colors::SURFACE_HOVER,
            );
        }

        // Draw text
        let text_color = if is_active {
            colors::TEXT_PRIMARY
        } else if is_hovered {
            colors::TEXT_SECONDARY
        } else {
            colors::TEXT_MUTED
        };

        let text_pos = egui::pos2(
            rect.center().x - text_size.x / 2.0,
            rect.center().y - text_size.y / 2.0,
        );

        ui.painter().galley(
            text_pos,
            ui.fonts(|f| {
                f.layout_no_wrap(
                    label.to_string(),
                    typography::font(
                        FontSize::Body,
                        if is_active {
                            FontWeight::SemiBold
                        } else {
                            FontWeight::Medium
                        },
                    ),
                    text_color,
                )
            }),
            Color32::TRANSPARENT,
        );

        // Draw underline indicator for active tab
        if is_active {
            let underline_rect = egui::Rect::from_min_size(
                egui::pos2(rect.left(), rect.bottom() - TAB_UNDERLINE_HEIGHT),
                egui::vec2(rect.width(), TAB_UNDERLINE_HEIGHT),
            );
            ui.painter().rect_filled(
                underline_rect,
                Rounding::ZERO,
                colors::ACCENT,
            );
        }

        response.clicked()
    }

    /// Render the content area based on the current tab.
    fn render_content(&self, ui: &mut egui::Ui) {
        match self.current_tab {
            Tab::ActiveRuns => self.render_active_runs(ui),
            Tab::Projects => self.render_projects(ui),
        }
    }

    /// Render the Active Runs view.
    fn render_active_runs(&self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new("Active Runs")
                    .font(typography::font(FontSize::Title, FontWeight::SemiBold))
                    .color(colors::TEXT_PRIMARY),
            );

            ui.add_space(8.0);

            if let Some(ref filter) = self.project_filter {
                ui.label(
                    egui::RichText::new(format!("Filtering by project: {}", filter))
                        .font(typography::font(FontSize::Body, FontWeight::Regular))
                        .color(colors::TEXT_SECONDARY),
                );
            }

            ui.add_space(16.0);

            // Placeholder content
            ui.label(
                egui::RichText::new("No active runs.")
                    .font(typography::font(FontSize::Body, FontWeight::Regular))
                    .color(colors::TEXT_MUTED),
            );
        });
    }

    /// Render the Projects view.
    fn render_projects(&self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new("Projects")
                    .font(typography::font(FontSize::Title, FontWeight::SemiBold))
                    .color(colors::TEXT_PRIMARY),
            );

            ui.add_space(8.0);

            if let Some(ref filter) = self.project_filter {
                ui.label(
                    egui::RichText::new(format!("Filtering by project: {}", filter))
                        .font(typography::font(FontSize::Body, FontWeight::Regular))
                        .color(colors::TEXT_SECONDARY),
                );
            }

            ui.add_space(16.0);

            // Placeholder content
            ui.label(
                egui::RichText::new("No projects found.")
                    .font(typography::font(FontSize::Body, FontWeight::Regular))
                    .color(colors::TEXT_MUTED),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tab_default_is_active_runs() {
        assert_eq!(Tab::default(), Tab::ActiveRuns);
    }

    #[test]
    fn test_tab_labels() {
        assert_eq!(Tab::ActiveRuns.label(), "Active Runs");
        assert_eq!(Tab::Projects.label(), "Projects");
    }

    #[test]
    fn test_tab_all_returns_all_tabs() {
        let all = Tab::all();
        assert_eq!(all.len(), 2);
        assert!(all.contains(&Tab::ActiveRuns));
        assert!(all.contains(&Tab::Projects));
    }

    #[test]
    fn test_tab_equality() {
        assert_eq!(Tab::ActiveRuns, Tab::ActiveRuns);
        assert_eq!(Tab::Projects, Tab::Projects);
        assert_ne!(Tab::ActiveRuns, Tab::Projects);
    }

    #[test]
    fn test_tab_copy() {
        let tab = Tab::Projects;
        let copied = tab;
        assert_eq!(tab, copied);
    }

    #[test]
    fn test_autom8_app_new_defaults_to_active_runs() {
        let app = Autom8App::new(None);
        assert_eq!(app.current_tab(), Tab::ActiveRuns);
    }

    #[test]
    fn test_autom8_app_new_with_filter() {
        let app = Autom8App::new(Some("test-project".to_string()));
        assert_eq!(app.project_filter, Some("test-project".to_string()));
        assert_eq!(app.current_tab(), Tab::ActiveRuns);
    }

    #[test]
    fn test_autom8_app_new_without_filter() {
        let app = Autom8App::new(None);
        assert_eq!(app.project_filter, None);
    }

    #[test]
    fn test_header_height_is_reasonable() {
        assert!(HEADER_HEIGHT >= 40.0);
        assert!(HEADER_HEIGHT <= 60.0);
    }

    #[test]
    fn test_tab_underline_is_subtle() {
        assert!(TAB_UNDERLINE_HEIGHT >= 1.0);
        assert!(TAB_UNDERLINE_HEIGHT <= 4.0);
    }
}
