//! Claude CLI integration for autom8.
//!
//! This module provides functions for interacting with the Claude CLI
//! to implement user stories, review code, and handle PR comments.
//!
//! # Modules
//!
//! - [`types`] - Core result types and error handling
//! - [`stream`] - JSON stream parsing for Claude CLI output
//! - [`runner`] - Main story implementation runner
//! - [`spec`] - Spec generation from markdown
//! - [`review`] - Code review and correction
//! - [`commit`] - Commit message generation
//! - [`pr_review`] - PR review analysis
//! - [`utils`] - Utility functions

mod commit;
mod pr_review;
mod review;
mod runner;
mod spec;
mod stream;
mod types;
mod utils;

// Re-export all public types and functions
pub use commit::{run_for_commit, CommitResult};
pub use pr_review::{run_pr_review, PRReviewResult, PRReviewSummary};
pub use review::{run_corrector, run_reviewer, CorrectorResult, ReviewResult};
pub use runner::run_claude;
pub use spec::run_for_spec_generation;
pub use stream::extract_text_from_stream_line;
pub use types::{ClaudeErrorInfo, ClaudeOutcome, ClaudeResult, ClaudeStoryResult};
pub use utils::{
    build_previous_context, extract_decisions, extract_files_context, extract_patterns,
    extract_work_summary, fix_json_syntax, Decision, FileContextEntry, Pattern,
};
