//! autom8 - CLI tool for orchestrating Claude-powered development.
//!
//! autom8 bridges the gap between product requirements (specs) and working code
//! by driving Claude through iterative implementation of user stories.
//!
//! # Core Workflow
//!
//! 1. Define features as structured user stories with acceptance criteria
//! 2. autom8 orchestrates Claude to implement each story
//! 3. Reviews for quality and iterates as needed
//! 4. Commits changes and creates GitHub PRs
//!
//! # Modules
//!
//! - [`commands`] - CLI command handlers
//! - [`runner`] - Main orchestration loop
//! - [`claude`] - Claude CLI integration
//! - [`gh`] - GitHub CLI integration
//! - [`output`] - Terminal output formatting
//! - [`state`] - State machine and persistence
//! - [`config`] - Configuration management
//! - [`spec`] - Spec/user story structures

pub mod claude;
pub mod commands;
pub mod completion;
pub mod config;
pub mod display;
pub mod error;
pub mod gh;
pub mod git;
pub mod gui;
pub mod knowledge;
pub mod monitor;
pub mod output;
pub mod progress;
pub mod prompt;
pub mod prompts;
pub mod runner;
pub mod signal;
pub mod snapshot;
pub mod spec;
pub mod state;
#[cfg(test)]
pub mod test_utils;
pub mod worktree;

pub use display::{BannerColor, StoryResult};
pub use error::{Autom8Error, Result};
pub use progress::{Breadcrumb, BreadcrumbState, ProgressContext};
pub use runner::Runner;
pub use snapshot::{FileMetadata, SpecSnapshot};
pub use spec::Spec;
pub use state::{MachineState, RunState, RunStatus, SessionMetadata, SessionStatus, StateManager};
