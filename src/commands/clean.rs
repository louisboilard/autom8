//! Clean command handler.
//!
//! Removes spec files from the project's config directory.

use std::fs;

use crate::error::Result;
use crate::output::{GREEN, GRAY, RESET};
use crate::prompt;
use crate::state::StateManager;

use super::ensure_project_dir;

/// Clean up spec files from the config directory.
///
/// Lists all spec files in the project's spec directory and prompts
/// the user for confirmation before deleting them.
///
/// # Returns
///
/// * `Ok(())` on success (even if no files were deleted)
/// * `Err(Autom8Error)` if file operations fail
pub fn clean_command() -> Result<()> {
    ensure_project_dir()?;

    let state_manager = StateManager::new()?;
    let spec_dir = state_manager.spec_dir();
    let project_config_dir = crate::config::project_config_dir()?;

    let mut deleted_any = false;

    // Check spec/ directory in config
    if spec_dir.exists() {
        let specs = state_manager.list_specs().unwrap_or_default();
        if !specs.is_empty() {
            println!();
            println!(
                "Found {} spec file(s) in {}:",
                specs.len(),
                spec_dir.display()
            );
            for spec_path in &specs {
                let filename = spec_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("?");
                println!("  - {}", filename);
            }
            println!();

            let prompt_msg = format!("Delete all spec files in {}?", spec_dir.display());
            if prompt::confirm(&prompt_msg, false) {
                for spec_path in specs {
                    fs::remove_file(&spec_path)?;
                    println!("{GREEN}Deleted{RESET} {}", spec_path.display());
                    deleted_any = true;
                }
            }
        }
    }

    if !deleted_any {
        println!(
            "{GRAY}No spec files to clean up in {}.{RESET}",
            project_config_dir.display()
        );
    }

    Ok(())
}
