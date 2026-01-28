//! List command handler.
//!
//! Shows a tree view of all projects with their status.

use crate::error::Result;
use crate::output::print_project_tree;

/// Display a tree view of all projects.
///
/// Shows each project with its subdirectories and status indicators
/// using box-drawing characters for visual tree structure.
///
/// # Returns
///
/// * `Ok(())` on success
/// * `Err(Autom8Error)` if reading project information fails
pub fn list_command() -> Result<()> {
    let projects = crate::config::list_projects_tree()?;
    print_project_tree(&projects);
    Ok(())
}
