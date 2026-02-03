//! GUI command handler.
//!
//! Launches a native GUI window using eframe/egui to display
//! real-time status of autom8 activity across all projects.

use crate::error::Result;
use crate::ui::gui::app::run_gui;

/// Launch the native GUI application.
///
/// Opens a native window displaying real-time status of autom8 activity
/// across all projects in a graphical user interface.
///
/// # Returns
///
/// * `Ok(())` when the user closes the window
/// * `Err(Autom8Error)` if the GUI fails to initialize
pub fn gui_command() -> Result<()> {
    run_gui()
}
