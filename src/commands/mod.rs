//! CLI command handlers for autom8.
//!
//! This module contains the implementation of all CLI subcommands.
//! Each command has its own module with handler functions.
//!
//! # Commands
//!
//! - [`run`] - Run implementation from a spec file
//! - [`status`] - Check current run status
//! - [`resume`] - Resume an interrupted run
//! - [`clean`] - Clean up spec files
//! - [`init`] - Initialize project config
//! - [`projects`] - List known projects
//! - [`list`] - Tree view of projects
//! - [`describe`] - Show project details
//! - [`pr_review`] - Analyze and fix PR review comments
//! - [`monitor`] - TUI dashboard
//! - [`default`] - Interactive spec creation flow

mod clean;
mod default;
mod describe;
mod init;
mod list;
mod monitor;
mod pr_review;
mod projects;
mod resume;
mod run;
mod status;

pub use clean::clean_command;
pub use default::default_command;
pub use describe::describe_command;
pub use init::init_command;
pub use list::list_command;
pub use monitor::monitor_command;
pub use pr_review::pr_review_command;
pub use projects::projects_command;
pub use resume::resume_command;
pub use run::{run_command, run_with_file};
pub use status::{all_sessions_status_command, global_status_command, status_command};

use std::path::Path;

/// Input type based on file extension.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputType {
    /// JSON spec file (spec-<feature>.json)
    Json,
    /// Markdown spec file (spec-<feature>.md)
    Markdown,
}

/// Detect input type based on file extension.
pub fn detect_input_type(path: &Path) -> InputType {
    match path.extension().and_then(|e| e.to_str()) {
        Some("json") => InputType::Json,
        _ => InputType::Markdown,
    }
}

/// Ensure project config directory exists.
///
/// Used by commands that need project-specific configuration.
///
/// # Errors
///
/// Returns an error if the directory cannot be created.
pub fn ensure_project_dir() -> crate::error::Result<()> {
    crate::config::ensure_project_config_dir()?;
    Ok(())
}
