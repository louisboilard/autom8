//! Run command handler.
//!
//! Executes the autom8 agent loop to implement spec stories.

use std::path::Path;

use crate::config::spec_dir;
use crate::error::Result;
use crate::output::{print_header, GREEN, RESET};
use crate::self_test::{
    cleanup_self_test, create_self_test_spec, print_cleanup_results, print_failure_details,
    SELF_TEST_SPEC_FILENAME,
};
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
/// * `self_test` - If true, run a self-test with hardcoded spec (ignores spec argument)
/// * `all_permissions` - If true, skip all permission restrictions (use --dangerously-skip-permissions)
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
    self_test: bool,
    all_permissions: bool,
) -> Result<()> {
    ensure_project_dir()?;

    // Handle self-test mode
    if self_test {
        return run_self_test(verbose, skip_review, worktree, no_worktree, all_permissions);
    }

    let mut runner = Runner::new()?
        .with_verbose(verbose)
        .with_skip_review(skip_review);

    // Apply worktree CLI flag override (CLI flags take precedence over config)
    if worktree {
        runner = runner.with_worktree(true);
    } else if no_worktree {
        runner = runner.with_worktree(false);
    }

    // Apply all_permissions CLI flag override
    if all_permissions {
        runner = runner.with_all_permissions(true);
    }

    print_header();

    match detect_input_type(spec) {
        InputType::Json => runner.run(spec),
        InputType::Markdown => runner.run_from_spec(spec),
    }
}

/// Run a self-test with the hardcoded spec.
///
/// Creates the self-test spec, saves it to the config directory, and runs
/// the normal implementation flow with commit and PR disabled. Cleans up
/// all test artifacts afterward (on both success and failure).
fn run_self_test(
    verbose: bool,
    skip_review: bool,
    worktree: bool,
    no_worktree: bool,
    all_permissions: bool,
) -> Result<()> {
    // Create and save the self-test spec to the config directory
    let spec = create_self_test_spec();
    let spec_path = spec_dir()?.join(SELF_TEST_SPEC_FILENAME);
    spec.save(&spec_path)?;

    // Configure runner with commit and PR disabled (self-test shouldn't create commits or PRs)
    let mut runner = Runner::new()?
        .with_verbose(verbose)
        .with_skip_review(skip_review)
        .with_commit(false)
        .with_pull_request(false);

    // Apply worktree CLI flag override
    if worktree {
        runner = runner.with_worktree(true);
    } else if no_worktree {
        runner = runner.with_worktree(false);
    }

    // Apply all_permissions CLI flag override
    if all_permissions {
        runner = runner.with_all_permissions(true);
    }

    print_header();

    // Run the normal implementation flow with the self-test spec
    let run_result = runner.run(&spec_path);

    // On failure, print detailed error information before cleanup
    if let Err(ref e) = run_result {
        print_failure_details(e);
    }

    // Always clean up, regardless of success or failure
    let cleanup_result = cleanup_self_test();
    print_cleanup_results(&cleanup_result);

    // Return the original run result (cleanup failures are just reported, not propagated)
    run_result
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
