//! Status command handler.
//!
//! Displays the current run status for a project or across all projects.

use crate::error::Result;
use crate::output::{print_global_status, print_status};
use crate::Runner;

use super::ensure_project_dir;

/// Display status for the current project.
///
/// Shows the current run state including branch, current story,
/// iteration count, and timestamps.
///
/// # Arguments
///
/// * `runner` - The Runner instance to query status from
///
/// # Returns
///
/// * `Ok(())` on success
/// * `Err(Autom8Error)` if reading state fails
pub fn status_command(runner: &Runner) -> Result<()> {
    ensure_project_dir()?;

    match runner.status() {
        Ok(Some(state)) => {
            print_status(&state);
            Ok(())
        }
        Ok(None) => {
            println!("No active run.");
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// Display status across all projects.
///
/// Shows a summary of all projects with their run status,
/// highlighting those that need attention (active or failed runs).
///
/// # Returns
///
/// * `Ok(())` on success
/// * `Err(Autom8Error)` if reading project statuses fails
pub fn global_status_command() -> Result<()> {
    let statuses = crate::config::global_status()?;
    print_global_status(&statuses);
    Ok(())
}
