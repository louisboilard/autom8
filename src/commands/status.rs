//! Status command handler.
//!
//! Displays the current run status for a project or across all projects.

use crate::error::Result;
use crate::output::{print_global_status, print_sessions_status, print_status};
use crate::state::StateManager;
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

/// Display status for all sessions in the current project.
///
/// Shows a list of all sessions (worktrees) for the project, including:
/// - Session ID and worktree path
/// - Branch name and current state
/// - Current story (if any)
/// - Started time / duration
///
/// Sessions are sorted with the current session first, then by last active time.
/// Stale sessions (deleted worktrees) are marked accordingly.
///
/// # Returns
///
/// * `Ok(())` on success
/// * `Err(Autom8Error)` if reading session data fails
pub fn all_sessions_status_command() -> Result<()> {
    ensure_project_dir()?;

    let state_manager = StateManager::new()?;
    let sessions = state_manager.list_sessions_with_status()?;

    if sessions.is_empty() {
        println!("No sessions found for this project.");
        println!();
        println!("Run `autom8 run` to start a session.");
        return Ok(());
    }

    print_sessions_status(&sessions);
    Ok(())
}
