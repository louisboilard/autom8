//! Projects command handler.
//!
//! Lists all known projects in the autom8 config directory.

use crate::error::Result;
use crate::output::{BOLD, CYAN, GRAY, RESET};

/// List all known projects.
///
/// Displays all projects that have been initialized in the
/// `~/.config/autom8/` directory.
///
/// # Returns
///
/// * `Ok(())` on success
/// * `Err(Autom8Error)` if reading the config directory fails
pub fn projects_command() -> Result<()> {
    let projects = crate::config::list_projects()?;

    if projects.is_empty() {
        println!("{GRAY}No projects found.{RESET}");
        println!();
        println!("Run {CYAN}autom8{RESET} in a project directory to create a project.");
    } else {
        println!("{BOLD}Known projects:{RESET}");
        println!();
        for project in &projects {
            println!("  {}", project);
        }
        println!();
        println!(
            "{GRAY}({} project{}){RESET}",
            projects.len(),
            if projects.len() == 1 { "" } else { "s" }
        );
    }

    Ok(())
}
