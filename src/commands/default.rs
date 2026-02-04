//! Default command handler.
//!
//! Handles the default behavior when autom8 is run with no arguments.
//! Checks for existing state and either resumes work or starts spec creation.

use crate::error::{Autom8Error, Result};
use crate::output::{print_error, print_header, BOLD, CYAN, GRAY, GREEN, RESET, YELLOW};
use crate::prompt;
use crate::prompts;
use crate::state::{RunState, StateManager};
use crate::Runner;
use crate::SpecSnapshot;

use super::ensure_project_dir;

/// Default command when running `autom8` with no arguments.
///
/// # Workflow
///
/// 1. Check if the project is initialized. If not, prompt to initialize.
/// 2. Check for existing state indicating work in progress.
/// 3. If state exists, prompt user to resume or start fresh.
/// 4. If no state, proceed to spec creation flow.
///
/// # Arguments
///
/// * `verbose` - If true, show full Claude output during spec creation
///
/// # Returns
///
/// * `Ok(())` on success
/// * `Err(Autom8Error)` if any step fails
pub fn default_command(verbose: bool) -> Result<()> {
    // Check if this directory is already tracked by autom8
    let project_name = crate::config::current_project_name()?;
    if !crate::config::project_exists(&project_name)? {
        // Project not initialized - ask user if they want to initialize
        print_header();
        println!(
            "This directory ({CYAN}{}{RESET}) is not currently tracked by autom8.",
            project_name
        );
        println!();

        if prompt::confirm("Would you like to initialize it?", true) {
            ensure_project_dir()?;
            println!();
            println!("{GREEN}Initialized.{RESET}");
            println!();
        } else {
            println!();
            println!("Exiting.");
            return Ok(());
        }
    } else {
        // Project exists, ensure directories are set up
        ensure_project_dir()?;
    }

    let state_manager = StateManager::new()?;

    // Check for existing state file
    if let Some(state) = state_manager.load_current()? {
        // State exists - proceed to prompt user
        handle_existing_state(state, verbose)
    } else {
        // No state - proceed to spec creation
        start_spec_creation(verbose)
    }
}

/// Handle existing state file - prompt user to resume or start fresh.
///
/// # Arguments
///
/// * `state` - The existing run state
/// * `verbose` - If true, show full Claude output
///
/// # Returns
///
/// * `Ok(())` on success
/// * `Err(Autom8Error)` if any step fails
fn handle_existing_state(state: RunState, verbose: bool) -> Result<()> {
    print_header();

    // Display clear prompt with context
    println!("{YELLOW}Work in progress detected.{RESET}");
    println!();
    println!("  Branch: {CYAN}{}{RESET}", state.branch);
    if let Some(story) = &state.current_story {
        println!("  Current story: {CYAN}{}{RESET}", story);
    }
    println!();

    // Present options to the user
    let choice = prompt::select(
        "Resume or start fresh?",
        &["Resume existing work", "Start fresh", "Exit"],
        0, // Default to Resume
    );

    match choice {
        0 => {
            // Option 1: Resume - continue the existing run
            println!();
            prompt::print_action("Resuming existing work...");
            println!();

            let runner = Runner::new()?.with_verbose(verbose);
            runner.resume()
        }
        1 => {
            // Option 2: Start fresh - archive state and proceed to spec creation
            let state_manager = StateManager::new()?;

            // Archive before deleting
            let archive_path = state_manager.archive(&state)?;
            state_manager.clear_current()?;

            println!();
            println!(
                "{GREEN}Previous state archived:{RESET} {}",
                archive_path.display()
            );
            println!();

            // Proceed to spec creation
            start_spec_creation(verbose)
        }
        _ => {
            // Option 3: Exit - do nothing, exit cleanly
            println!();
            println!("Exiting.");
            Ok(())
        }
    }
}

/// Start a new spec creation session.
///
/// Spawns an interactive Claude session to help create a spec file.
/// After the session ends, detects new spec files and proceeds to implementation.
///
/// # Arguments
///
/// * `verbose` - If true, show full Claude output
///
/// # Returns
///
/// * `Ok(())` on success
/// * `Err(Autom8Error)` if spec creation or implementation fails
fn start_spec_creation(verbose: bool) -> Result<()> {
    use std::process::Command;

    print_header();

    // Print explanation of what will happen
    println!("{BOLD}Starting Spec Creation Session{RESET}");
    println!();
    println!("This will spawn an interactive Claude session to help you create a spec.");
    println!("Claude will guide you through defining your feature with questions about:");
    println!("  - Project context and tech stack");
    println!("  - Feature requirements and user stories");
    println!("  - Acceptance criteria for each story");
    println!();
    println!(
        "When your spec is ready, Claude will ask if you'd like autom8 to start implementation."
    );
    println!("Say {CYAN}yes{RESET} to hand off automatically—autom8 takes it from there.");
    println!();
    println!("{GRAY}Starting Claude...{RESET}");
    println!();

    // Take a snapshot of existing spec files before spawning Claude
    let snapshot = SpecSnapshot::capture()?;

    // Spawn interactive Claude session with the spec skill prompt
    let status = Command::new("claude")
        .arg(prompts::SPEC_SKILL_PROMPT)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status();

    match status {
        Ok(exit_status) => {
            if exit_status.success() {
                println!();
                println!("{GREEN}Claude session ended.{RESET}");
                println!();

                // Detect new spec files created during the session
                let new_files = snapshot.detect_new_files()?;

                match new_files.len() {
                    0 => {
                        print_error("No new spec files detected.");
                        println!();
                        println!("{BOLD}Possible causes:{RESET}");
                        println!("  - Claude session ended before the spec was saved");
                        println!("  - Spec was saved to an unexpected location");
                        println!("  - Claude didn't follow the spec skill instructions");
                        println!();
                        println!("{BOLD}Suggestions:{RESET}");
                        println!("  - Run {CYAN}autom8{RESET} again to start a fresh session");
                        println!("  - Or use the manual workflow:");
                        println!("      1. Run {CYAN}autom8 skill spec{RESET} to get the prompt");
                        println!("      2. Start a Claude session and paste the prompt");
                        println!("      3. Save the spec as {CYAN}spec-<feature>.md{RESET}");
                        println!("      4. Run {CYAN}autom8{RESET} to implement");
                        std::process::exit(1);
                    }
                    1 => {
                        let spec_path = &new_files[0];
                        println!("{GREEN}Detected new spec:{RESET} {}", spec_path.display());
                        println!();
                        println!("{BOLD}Proceeding to implementation...{RESET}");
                        println!();

                        // Create a new runner and run from the spec
                        let runner = Runner::new()?.with_verbose(verbose);
                        runner.run_from_spec(spec_path)
                    }
                    n => {
                        println!("{YELLOW}Detected {} new spec files:{RESET}", n);
                        println!();

                        // Build options list with file paths
                        let options: Vec<String> = new_files
                            .iter()
                            .enumerate()
                            .map(|(i, file)| {
                                let filename = file
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("spec.md");
                                format!("{}. {}", i + 1, filename)
                            })
                            .collect();
                        let option_refs: Vec<&str> = options.iter().map(|s| s.as_str()).collect();

                        let choice = prompt::select(
                            "Which spec would you like to implement?",
                            &option_refs,
                            0,
                        );

                        let selected_spec = &new_files[choice];
                        println!();
                        println!("{GREEN}Selected:{RESET} {}", selected_spec.display());
                        println!();
                        println!("{BOLD}Proceeding to implementation...{RESET}");
                        println!();

                        // Create a new runner and run from the spec
                        let runner = Runner::new()?.with_verbose(verbose);
                        runner.run_from_spec(selected_spec)
                    }
                }
            } else {
                Err(Autom8Error::ClaudeError(format!(
                    "Claude exited with status: {}",
                    exit_status
                )))
            }
        }
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                Err(Autom8Error::ClaudeError(
                    "Claude CLI not found. Please install it from https://github.com/anthropics/claude-code".to_string()
                ))
            } else {
                Err(Autom8Error::ClaudeError(format!(
                    "Failed to spawn Claude: {}",
                    e
                )))
            }
        }
    }
}
