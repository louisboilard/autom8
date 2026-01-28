//! PR Review command handler.
//!
//! Analyzes unresolved PR review comments and uses Claude to fix
//! legitimate issues while ignoring stylistic preferences.

use crate::claude::{run_pr_review, PRReviewResult};
use crate::error::{Autom8Error, Result};
use crate::gh::{
    detect_pr_for_current_branch, gather_branch_context, gather_pr_context, list_open_prs,
    print_branch_context, BranchContextResult, PRContextResult, PRDetectionResult,
};
use crate::git::{checkout, commit_and_push_pr_fixes, current_branch, CommitResult, PushResult};
use crate::output::{
    format_pr_for_selection, print_branch_switched, print_error, print_no_open_prs,
    print_no_unresolved_comments, print_pr_commit_error, print_pr_commit_success,
    print_pr_context_summary, print_pr_detected, print_pr_push_error, print_pr_push_success,
    print_pr_push_up_to_date, print_pr_review_actions_summary,
    print_pr_review_complete_with_fixes, print_pr_review_error, print_pr_review_no_fixes_needed,
    print_pr_review_spawning, print_pr_review_start, print_pr_review_streaming,
    print_pr_review_streaming_done, print_pr_review_summary, print_switching_branch, BOLD, RESET,
};
use crate::prompt;

use super::ensure_project_dir;

/// Execute the PR review workflow.
///
/// # Workflow
///
/// 1. Detect or select a PR to review
/// 2. Gather PR context (unresolved comments)
/// 3. Gather branch context (commits, spec if available)
/// 4. Spawn Claude to analyze and fix issues
/// 5. Commit and push fixes if changes were made
///
/// # Arguments
///
/// * `verbose` - If true, show full Claude output instead of spinner
///
/// # Returns
///
/// * `Ok(())` on success
/// * `Err(Autom8Error)` if any step fails
pub fn pr_review_command(verbose: bool) -> Result<()> {
    ensure_project_dir()?;

    // Step 1: Detect PR for current branch
    let pr_info = match detect_pr_for_current_branch()? {
        PRDetectionResult::Found(info) => {
            print_pr_detected(info.number, &info.title, &info.head_branch);
            info
        }
        PRDetectionResult::OnMainBranch | PRDetectionResult::NoPRForBranch(_) => {
            // No PR for current branch - list open PRs and prompt user to select
            let open_prs = list_open_prs()?;

            if open_prs.is_empty() {
                print_no_open_prs();
                return Ok(());
            }

            // Build selection options
            let options: Vec<String> = open_prs
                .iter()
                .map(|pr| format_pr_for_selection(pr.number, &pr.head_branch, &pr.title))
                .collect();
            let option_refs: Vec<&str> = options.iter().map(|s| s.as_str()).collect();

            println!();
            println!("{BOLD}Select a PR to review:{RESET}");
            let choice = prompt::select("", &option_refs, 0);

            let selected_pr = &open_prs[choice];

            // Switch to the selected branch
            let current = current_branch()?;
            if current != selected_pr.head_branch {
                print_switching_branch(&current, &selected_pr.head_branch);
                checkout(&selected_pr.head_branch)?;
                print_branch_switched(&selected_pr.head_branch);
            }

            selected_pr.clone()
        }
        PRDetectionResult::Error(msg) => {
            print_error(&msg);
            return Err(Autom8Error::GitError(msg));
        }
    };

    // Step 2: Gather PR context (description, comments)
    let pr_context = match gather_pr_context(pr_info.number) {
        PRContextResult::Success(context) => {
            print_pr_context_summary(
                context.number,
                &context.title,
                context.unresolved_comments.len(),
            );
            context
        }
        PRContextResult::NoUnresolvedComments {
            number,
            title,
            body: _,
            url: _,
        } => {
            print_no_unresolved_comments(number, &title);
            return Ok(());
        }
        PRContextResult::Error(msg) => {
            print_error(&msg);
            return Err(Autom8Error::GitError(msg));
        }
    };

    // Step 3: Gather branch context (spec, commits)
    let branch_context = match gather_branch_context(true) {
        BranchContextResult::SuccessWithSpec(context) => {
            print_branch_context(&context);
            context
        }
        BranchContextResult::SuccessNoSpec(context) => {
            // Warning already printed by gather_branch_context when show_warning=true
            print_branch_context(&context);
            context
        }
        BranchContextResult::Error(msg) => {
            print_error(&msg);
            return Err(Autom8Error::GitError(msg));
        }
    };

    // Step 4: Spawn Claude agent for PR review
    print_pr_review_start(
        pr_context.number,
        &pr_context.title,
        pr_context.unresolved_comments.len(),
    );
    print_pr_review_spawning();
    print_pr_review_streaming();

    let review_result = run_pr_review(&pr_context, &branch_context, |text| {
        if verbose {
            print!("{}", text);
        }
    })?;

    print_pr_review_streaming_done();

    // Step 5: Handle results and commit/push if configured
    let config = crate::config::get_effective_config().unwrap_or_default();

    match review_result {
        PRReviewResult::Complete(summary) => {
            print_pr_review_summary(&summary);
            print_pr_review_complete_with_fixes(summary.real_issues_fixed);

            // Commit and push fixes
            let (commit_result, push_result) =
                commit_and_push_pr_fixes(pr_context.number, config.commit, config.pull_request)?;

            let commit_made = matches!(&commit_result, Some(CommitResult::Success(_)));
            let push_made = matches!(&push_result, Some(PushResult::Success));

            // Print individual status messages
            if let Some(ref result) = commit_result {
                match result {
                    CommitResult::Success(hash) => print_pr_commit_success(hash),
                    CommitResult::NothingToCommit => {}
                    CommitResult::Error(msg) => print_pr_commit_error(msg),
                }
            }

            if let Some(ref result) = push_result {
                match result {
                    PushResult::Success => print_pr_push_success(&branch_context.branch_name),
                    PushResult::AlreadyUpToDate => print_pr_push_up_to_date(),
                    PushResult::Error(msg) => print_pr_push_error(msg),
                }
            }

            // Print summary of actions taken
            print_pr_review_actions_summary(
                config.commit,
                config.pull_request,
                commit_made,
                push_made,
                false,
            );
        }
        PRReviewResult::NoFixesNeeded(summary) => {
            print_pr_review_summary(&summary);
            print_pr_review_no_fixes_needed();

            // Print summary indicating no fixes were needed
            print_pr_review_actions_summary(config.commit, config.pull_request, false, false, true);
        }
        PRReviewResult::Error(error_info) => {
            print_pr_review_error(&error_info.message);
            return Err(Autom8Error::ClaudeError(error_info.message));
        }
    }

    Ok(())
}
