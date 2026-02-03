//! GUI command handler.
//!
//! Launches a native GUI window using eframe/egui to display
//! real-time status of autom8 activity across all projects.

use crate::error::Result;
use crate::gui::app::run_gui;

/// Launch the native GUI application.
///
/// Opens a native window displaying real-time status of autom8 activity
/// across all projects in a graphical user interface.
///
/// # Arguments
///
/// * `project_filter` - Optional project name to filter the view
///
/// # Returns
///
/// * `Ok(())` when the user closes the window
/// * `Err(Autom8Error)` if the GUI fails to initialize
pub fn gui_command(project_filter: Option<&str>) -> Result<()> {
    run_gui(project_filter.map(|s| s.to_string()))
}
