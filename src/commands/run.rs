//! Run command handler.
//!
//! Executes the autom8 agent loop to implement spec stories.

use std::path::Path;

use crate::error::Result;
use crate::output::{print_header, GREEN, RESET};
use crate::Runner;

use super::{detect_input_type, ensure_project_dir, InputType};

/// Run implementation from a spec file.
///
/// Parses the spec file and runs the agent loop to implement
/// each user story sequentially.
///
/// # Arguments
///
/// * `verbose` - If true, show full Claude output
/// * `spec` - Path to the spec file (JSON or Markdown)
/// * `skip_review` - If true, skip the review loop
/// * `worktree` - If true, enable worktree mode (overrides config)
/// * `no_worktree` - If true, disable worktree mode (overrides config)
///
/// # Returns
///
/// * `Ok(())` on successful completion
/// * `Err(Autom8Error)` if implementation fails
pub fn run_command(
    verbose: bool,
    spec: &Path,
    skip_review: bool,
    worktree: bool,
    no_worktree: bool,
) -> Result<()> {
    ensure_project_dir()?;

    let mut runner = Runner::new()?
        .with_verbose(verbose)
        .with_skip_review(skip_review);

    // Apply worktree CLI flag override (CLI flags take precedence over config)
    if worktree {
        runner = runner.with_worktree(true);
    } else if no_worktree {
        runner = runner.with_worktree(false);
    }

    print_header();

    match detect_input_type(spec) {
        InputType::Json => runner.run(spec),
        InputType::Markdown => runner.run_from_spec(spec),
    }
}

/// Run implementation from a file argument.
///
/// Handles the positional file argument, moving the spec file to the
/// config directory if necessary before running.
///
/// # Arguments
///
/// * `runner` - The configured Runner instance
/// * `file` - Path to the spec file
///
/// # Returns
///
/// * `Ok(())` on successful completion
/// * `Err(Autom8Error)` if implementation fails
pub fn run_with_file(runner: &Runner, file: &Path) -> Result<()> {
    ensure_project_dir()?;

    // Move file to config directory if not already there
    let move_result = crate::config::move_to_config_dir(file)?;

    print_header();

    // Notify user if file was moved
    if move_result.was_moved {
        println!(
            "{GREEN}Moved{RESET} {} → {}",
            file.display(),
            move_result.dest_path.display()
        );
        println!();
    }

    // Use the destination path for processing
    match detect_input_type(&move_result.dest_path) {
        InputType::Json => runner.run(&move_result.dest_path),
        InputType::Markdown => runner.run_from_spec(&move_result.dest_path),
    }
}
