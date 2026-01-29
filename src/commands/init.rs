//! Init command handler.
//!
//! Initializes the autom8 config directory structure for a project.

use crate::completion;
use crate::error::Result;
use crate::output::{BOLD, CYAN, GRAY, GREEN, RESET, YELLOW};

/// Initialize autom8 configuration for the current project.
///
/// Creates the following directory structure:
/// - `~/.config/autom8/` - Base config directory
/// - `~/.config/autom8/config.toml` - Global configuration
/// - `~/.config/autom8/<project>/` - Project-specific directory
/// - `~/.config/autom8/<project>/spec/` - Spec files
/// - `~/.config/autom8/<project>/runs/` - Archived run states
///
/// # Returns
///
/// * `Ok(())` on success
/// * `Err(Autom8Error)` if directory creation fails
pub fn init_command() -> Result<()> {
    println!("Initializing autom8...");
    println!();

    // Create base config directory ~/.config/autom8/
    let (config_dir, config_created) = crate::config::ensure_config_dir()?;
    if config_created {
        println!("  {GREEN}Created{RESET} {}", config_dir.display());
    } else {
        println!("  {GRAY}Exists{RESET}  {}", config_dir.display());
    }

    // Reset global config file to defaults
    let global_config_path = crate::config::global_config_path()?;
    crate::config::save_global_config(&crate::config::Config::default())?;
    println!(
        "Created default configuration at {}",
        global_config_path.display()
    );

    // Create project-specific config directory with subdirectories
    let (project_dir, project_created) = crate::config::ensure_project_config_dir()?;
    if project_created {
        println!("  {GREEN}Created{RESET} {}", project_dir.display());
        println!("  {GREEN}Created{RESET} {}/spec/", project_dir.display());
        println!("  {GREEN}Created{RESET} {}/runs/", project_dir.display());
    } else {
        println!("  {GRAY}Exists{RESET}  {}", project_dir.display());
    }

    println!();
    println!("{GREEN}Initialization complete!{RESET}");
    println!();
    println!("Config directory structure:");
    println!("  {CYAN}{}{RESET}", project_dir.display());
    println!("    ├── spec/  (spec markdown and JSON files)");
    println!("    └── runs/  (archived run states)");
    // Install shell completions
    println!();
    println!("{BOLD}Shell completions:{RESET}");
    match completion::install_completions() {
        Ok(result) => {
            println!(
                "  {GREEN}Installed{RESET} {} completions to {}",
                result.shell,
                result.path.display()
            );
            if let Some(instructions) = result.setup_instructions {
                println!();
                println!("{YELLOW}Note:{RESET} {}", instructions);
            }
        }
        Err(e) => {
            // Don't fail init for completion errors - just inform the user
            let msg = e.to_string();
            if msg.contains("Unsupported shell") {
                println!("  {YELLOW}Skipped{RESET} Shell completions not available for your shell");
                println!("         Supported shells: bash, zsh, fish");
            } else if msg.contains("$SHELL") {
                println!("  {YELLOW}Skipped{RESET} Could not detect shell ($SHELL not set)");
            } else {
                println!(
                    "  {YELLOW}Warning{RESET} Could not install completions: {}",
                    e
                );
            }
        }
    }

    println!();
    println!("{BOLD}Next steps:{RESET}");
    println!("  Run {CYAN}autom8{RESET} to start creating a spec");

    Ok(())
}
