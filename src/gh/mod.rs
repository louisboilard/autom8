//! GitHub CLI integration for PR operations.
//!
//! This module provides functions to interact with the GitHub CLI for
//! checking prerequisites and managing pull requests.
//!
//! # Modules
//!
//! - [`types`] - Core types for PR operations
//! - [`format`] - PR title and description formatting
//! - [`pr`] - PR creation and management
//! - [`detection`] - PR detection for branches
//! - [`context`] - PR context gathering for reviews
//! - [`branch`] - Branch context for PR reviews

mod branch;
mod context;
mod detection;
mod format;
mod pr;
mod template;
mod types;

// Re-export all public types and functions
pub use branch::{
    find_spec_for_branch, gather_branch_context, print_branch_context, BranchContext,
    BranchContextResult,
};
pub use context::{gather_pr_context, PRComment, PRContext, PRContextResult};
pub use detection::{
    detect_pr_for_current_branch, get_existing_pr_number, get_existing_pr_url,
    get_pr_info_for_branch, list_open_prs, pr_exists_for_branch,
};
pub use format::{format_pr_description, format_pr_title};
pub use pr::{
    create_pull_request, ensure_branch_pushed, is_gh_authenticated, is_gh_installed,
    update_pr_description,
};
pub use template::{
    build_gh_command, detect_pr_template, extract_pr_url, format_spec_for_template,
    run_template_agent, TemplateAgentResult,
};
pub use types::{PRDetectionResult, PRResult, PullRequestInfo};
