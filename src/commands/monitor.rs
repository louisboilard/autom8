//! Monitor command handler.
//!
//! Launches a TUI dashboard to display real-time status of autom8
//! activity across all projects.

use crate::error::Result;
use crate::ui::tui::app::run_monitor;

/// Launch the monitor TUI dashboard.
///
/// Displays real-time status of autom8 activity across all projects
/// in a terminal user interface. Refresh interval is hardcoded to 100ms
/// for responsive UI updates.
///
/// # Arguments
///
/// * `project_filter` - Optional project name to filter the view
///
/// # Returns
///
/// * `Ok(())` when the user exits the TUI
/// * `Err(Autom8Error)` if the TUI fails to initialize
pub fn monitor_command(project_filter: Option<&str>) -> Result<()> {
    run_monitor(project_filter.map(|s| s.to_string()))
}
