//! Terminal output formatting for autom8.
//!
//! This module provides consistent, colored terminal output for all
//! CLI operations. Functions are organized by domain:
//!
//! - [`banner`] - Phase banners and footers
//! - [`messages`] - Error, warning, and info messages
//! - [`header`] - Session headers and iteration display
//! - [`pr`] - Pull request operation output
//! - [`pr_review`] - PR review workflow output
//! - [`status`] - Project and run status display
//! - [`progress`] - Progress bars and summaries
//! - [`error`] - Error panels with detailed formatting

pub mod banner;
pub mod error;
pub mod header;
pub mod messages;
pub mod pr;
pub mod pr_review;
pub mod progress;
pub mod status;

/// ANSI color codes for terminal output.
pub mod colors {
    pub const RESET: &str = "\x1b[0m";
    pub const BOLD: &str = "\x1b[1m";
    pub const DIM: &str = "\x1b[2m";
    pub const GREEN: &str = "\x1b[32m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const BLUE: &str = "\x1b[34m";
    pub const CYAN: &str = "\x1b[36m";
    pub const RED: &str = "\x1b[31m";
    pub const GRAY: &str = "\x1b[90m";
}

// Re-export colors at module level for convenience
pub use colors::*;

// Re-export all public items for backward compatibility
pub use banner::{print_phase_banner, print_phase_footer, BannerColor};
pub use error::{print_error_panel, ErrorDetails};
pub use header::{
    print_claude_output, print_generating_spec, print_header, print_iteration_complete,
    print_iteration_start, print_proceeding_to_implementation, print_project_info,
    print_spec_generated, print_spec_loaded, print_state_transition,
};
pub use messages::{
    print_error, print_info, print_warning, print_worktree_context, print_worktree_created,
    print_worktree_reused,
};
pub use pr::{
    format_pr_for_selection, print_branch_switched, print_no_open_prs, print_pr_already_exists,
    print_pr_detected, print_pr_skipped, print_pr_success, print_pr_updated,
    print_push_already_up_to_date, print_push_success, print_pushing_branch,
    print_switching_branch,
};
pub use pr_review::{
    print_no_unresolved_comments, print_pr_comment, print_pr_comments_list, print_pr_commit_error,
    print_pr_commit_skipped_config, print_pr_commit_success, print_pr_context_error,
    print_pr_context_summary, print_pr_no_commit_no_fixes, print_pr_push_error,
    print_pr_push_skipped_config, print_pr_push_success, print_pr_push_up_to_date,
    print_pr_review_actions_summary, print_pr_review_complete_with_fixes, print_pr_review_error,
    print_pr_review_no_fixes_needed, print_pr_review_spawning, print_pr_review_start,
    print_pr_review_streaming, print_pr_review_streaming_done, print_pr_review_summary,
};
pub use progress::{
    make_progress_bar, print_all_complete, print_breadcrumb_trail, print_full_progress,
    print_issues_found, print_max_review_iterations, print_review_passed, print_review_progress,
    print_reviewing, print_run_summary, print_skip_review, print_story_complete,
    print_tasks_progress, StoryResult,
};
pub use status::{
    print_branch_context_summary, print_commit_list, print_global_status, print_history_entry,
    print_missing_spec_warning, print_project_description, print_project_tree,
    print_sessions_status, print_status,
};
