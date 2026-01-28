//! Resume command handler.
//!
//! Resumes a failed or interrupted autom8 run from its last checkpoint.

use crate::error::Result;
use crate::Runner;

use super::ensure_project_dir;

/// Resume an interrupted or failed run.
///
/// Loads the saved state from `.autom8/state.json` and continues
/// execution from where it left off.
///
/// # Returns
///
/// * `Ok(())` on successful completion
/// * `Err(Autom8Error)` if no state exists or resumption fails
pub fn resume_command(runner: &Runner) -> Result<()> {
    ensure_project_dir()?;
    runner.resume()
}
