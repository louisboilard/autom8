//! Describe command handler.
//!
//! Shows detailed information about a specific project.

use crate::error::Result;
use crate::output::{print_project_description, BOLD, CYAN, GRAY, RED, RESET, YELLOW};
use crate::prompt;

/// Show detailed description of a project.
///
/// Displays project information including:
/// - Project name and path
/// - Current status (active run, idle, etc.)
/// - Spec details with user stories and progress
/// - File counts
///
/// # Arguments
///
/// * `project_name` - The project name to describe (empty string uses current directory)
///
/// # Returns
///
/// * `Ok(())` on success
/// * `Err(Autom8Error)` if reading project information fails
pub fn describe_command(project_name: &str) -> Result<()> {
    // If no project name passed, try to use current directory
    let project_name = if project_name.is_empty() {
        let current = crate::config::current_project_name()?;

        if crate::config::project_exists(&current)? {
            println!(
                "{GRAY}No project specified, using current directory: {CYAN}{}{RESET}",
                current
            );
            println!();
            current
        } else {
            // Current directory is not an autom8 project - show helpful error
            println!("{RED}No project specified.{RESET}");
            println!();
            println!(
                "The current directory {CYAN}{}{RESET} is not an autom8 project.",
                current
            );
            println!();

            // List available projects
            let projects = crate::config::list_projects()?;
            if projects.is_empty() {
                println!("{GRAY}No projects have been created yet.{RESET}");
                println!();
                println!("Run {CYAN}autom8{RESET} in a project directory to create a project.");
            } else {
                println!("{BOLD}Available projects:{RESET}");
                for project in &projects {
                    println!("  - {}", project);
                }
                println!();
                println!("Run {CYAN}autom8 describe <project-name>{RESET} to describe a project.");
            }
            return Ok(());
        }
    } else {
        project_name.to_string()
    };

    // Check if project exists
    match crate::config::get_project_description(&project_name)? {
        Some(desc) => {
            // If multiple specs exist and user might want to select one, handle that case
            if desc.specs.len() > 1 {
                // Ask user which spec to describe
                println!(
                    "{YELLOW}Multiple specs found for project '{}'{RESET}",
                    &project_name
                );
                println!();

                let options: Vec<String> = desc
                    .specs
                    .iter()
                    .map(|spec| {
                        let progress = format!("{}/{}", spec.completed_count, spec.total_count);
                        format!("{} ({})", spec.filename, progress)
                    })
                    .collect();

                // Add an "All specs" option at the beginning
                let mut all_options: Vec<&str> = vec!["Show all specs"];
                all_options.extend(options.iter().map(|s| s.as_str()));

                let choice =
                    prompt::select("Which spec would you like to describe?", &all_options, 0);

                if choice == 0 {
                    // Show all specs
                    print_project_description(&desc);
                } else {
                    // Show specific spec
                    let selected_spec = &desc.specs[choice - 1];

                    // Create a description with just the selected spec
                    let single_spec_desc = crate::config::ProjectDescription {
                        specs: vec![selected_spec.clone()],
                        ..desc
                    };
                    print_project_description(&single_spec_desc);
                }
            } else {
                // Single or no specs - just show the description
                print_project_description(&desc);
            }
            Ok(())
        }
        None => {
            // Project doesn't exist
            println!("{RED}Project '{}' not found.{RESET}", &project_name);
            println!();
            println!(
                "The project directory {CYAN}~/.config/autom8/{}{RESET} does not exist.",
                &project_name
            );
            println!();

            // List available projects
            let projects = crate::config::list_projects()?;
            if projects.is_empty() {
                println!("{GRAY}No projects have been created yet.{RESET}");
                println!();
                println!("Run {CYAN}autom8{RESET} in a project directory to create a project.");
            } else {
                println!("{BOLD}Available projects:{RESET}");
                for project in &projects {
                    println!("  - {}", project);
                }
                println!();
                println!("Run {CYAN}autom8 describe <project-name>{RESET} to describe a project.");
            }
            Ok(())
        }
    }
}
