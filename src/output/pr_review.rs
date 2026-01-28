//! PR review workflow output.
//!
//! Output functions for the PR review analysis and fix workflow.

use super::colors::*;

/// Print a message when no unresolved comments are found on a PR.
pub fn print_no_unresolved_comments(pr_number: u32, title: &str) {
    println!();
    println!(
        "{GREEN}PR #{}{RESET} has no unresolved comments.",
        pr_number
    );
    println!("{BLUE}Title:{RESET} {}", title);
    println!();
    println!("{GRAY}Nothing to review - all feedback has been addressed!{RESET}");
}

/// Print a summary of the PR context being analyzed.
pub fn print_pr_context_summary(pr_number: u32, title: &str, comment_count: usize) {
    println!();
    println!("{CYAN}Analyzing PR #{}{RESET}: {}", pr_number, title);
    println!(
        "{BLUE}Found:{RESET} {} unresolved comment{}",
        comment_count,
        if comment_count == 1 { "" } else { "s" }
    );
    println!();
}

/// Print a single PR comment with its context.
pub fn print_pr_comment(
    index: usize,
    author: &str,
    body: &str,
    file_path: Option<&str>,
    line: Option<u32>,
) {
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!(
        "{YELLOW}Comment {}{RESET} by {CYAN}{}{RESET}",
        index + 1,
        author
    );

    if let Some(path) = file_path {
        if let Some(line_num) = line {
            println!("{BLUE}Location:{RESET} {}:{}", path, line_num);
        } else {
            println!("{BLUE}Location:{RESET} {}", path);
        }
    }

    println!();
    for line in body.lines() {
        println!("  {}", line);
    }
    println!();
}

/// Print the list of all unresolved comments for a PR.
pub fn print_pr_comments_list(comments: &[crate::gh::PRComment]) {
    println!("{BOLD}Unresolved Comments:{RESET}");
    println!();

    for (i, comment) in comments.iter().enumerate() {
        print_pr_comment(
            i,
            &comment.author,
            &comment.body,
            comment.file_path.as_deref(),
            comment.line,
        );
    }

    println!("{GRAY}{}{RESET}", "-".repeat(57));
}

/// Print an error message for PR context gathering failures.
pub fn print_pr_context_error(message: &str) {
    println!();
    println!("{RED}{BOLD}Failed to gather PR context:{RESET}");
    println!("{RED}  {}{RESET}", message);
    println!();
}

/// Print a header when starting PR review analysis.
pub fn print_pr_review_start(pr_number: u32, title: &str, comment_count: usize) {
    println!();
    println!("{CYAN}{BOLD}╔════════════════════════════════════════════════════════╗{RESET}");
    println!("{CYAN}{BOLD}║  PR Review Analysis                                    ║{RESET}");
    println!("{CYAN}{BOLD}╚════════════════════════════════════════════════════════╝{RESET}");
    println!();
    println!("{BLUE}PR #{}{RESET}: {}", pr_number, title);
    println!("{BLUE}Comments to analyze:{RESET} {}", comment_count);
    println!();
}

/// Print status when spawning the Claude agent for PR review.
pub fn print_pr_review_spawning() {
    println!("{GRAY}Spawning Claude agent for PR review...{RESET}");
    println!();
}

/// Print the PR review summary results.
pub fn print_pr_review_summary(summary: &crate::claude::PRReviewSummary) {
    println!();
    println!("{GRAY}{}{RESET}", "─".repeat(57));
    println!();
    println!("{BOLD}PR Review Summary{RESET}");
    println!();

    println!(
        "  {BLUE}Total comments analyzed:{RESET}    {}",
        summary.total_comments
    );
    println!(
        "  {GREEN}Real issues fixed:{RESET}         {}",
        summary.real_issues_fixed
    );
    println!(
        "  {YELLOW}Red herrings identified:{RESET}   {}",
        summary.red_herrings
    );
    println!(
        "  {GRAY}Legitimate suggestions:{RESET}    {}",
        summary.legitimate_suggestions
    );
    println!();
}

/// Print a success message when PR review completes with fixes made.
pub fn print_pr_review_complete_with_fixes(fixes_count: usize) {
    println!();
    println!("{GREEN}{BOLD}╔════════════════════════════════════════════════════════╗{RESET}");
    println!("{GREEN}{BOLD}║  ✓ PR Review Complete                                  ║{RESET}");
    println!("{GREEN}{BOLD}╚════════════════════════════════════════════════════════╝{RESET}");
    println!();
    println!(
        "{GREEN}Fixed {} issue{}.{RESET}",
        fixes_count,
        if fixes_count == 1 { "" } else { "s" }
    );
    println!();
}

/// Print a message when PR review completes but no fixes were needed.
pub fn print_pr_review_no_fixes_needed() {
    println!();
    println!("{CYAN}{BOLD}╔════════════════════════════════════════════════════════╗{RESET}");
    println!("{CYAN}{BOLD}║  ✓ PR Review Complete - No Fixes Needed                ║{RESET}");
    println!("{CYAN}{BOLD}╚════════════════════════════════════════════════════════╝{RESET}");
    println!();
    println!("{GRAY}All comments were either red herrings or suggestions.{RESET}");
    println!("{GRAY}No code changes were required.{RESET}");
    println!();
}

/// Print an error message when PR review fails.
pub fn print_pr_review_error(message: &str) {
    println!();
    println!("{RED}{BOLD}╔════════════════════════════════════════════════════════╗{RESET}");
    println!("{RED}{BOLD}║  ✗ PR Review Failed                                    ║{RESET}");
    println!("{RED}{BOLD}╚════════════════════════════════════════════════════════╝{RESET}");
    println!();
    println!("{RED}Error:{RESET} {}", message);
    println!();
}

/// Print a message when starting to stream Claude output for PR review.
pub fn print_pr_review_streaming() {
    println!("{GRAY}{}{RESET}", "─".repeat(57));
    println!("{CYAN}Claude Analysis:{RESET}");
    println!("{GRAY}{}{RESET}", "─".repeat(57));
    println!();
}

/// Print a footer after streaming Claude output for PR review.
pub fn print_pr_review_streaming_done() {
    println!();
    println!("{GRAY}{}{RESET}", "─".repeat(57));
}

/// Print a message when commit is skipped due to config.
pub fn print_pr_commit_skipped_config() {
    println!("{GRAY}Commit skipped (commit disabled in config){RESET}");
}

/// Print a message when push is skipped due to config.
pub fn print_pr_push_skipped_config() {
    println!("{GRAY}Push skipped (push disabled in config){RESET}");
}

/// Print a message when no fixes were made so no commit is needed.
pub fn print_pr_no_commit_no_fixes() {
    println!("{GRAY}No commit created (no fixes were made){RESET}");
}

/// Print a success message when PR review commit is created.
pub fn print_pr_commit_success(commit_hash: &str) {
    println!(
        "{GREEN}Created commit {}{RESET} with PR review fixes",
        commit_hash
    );
}

/// Print an error message when PR review commit fails.
pub fn print_pr_commit_error(message: &str) {
    println!("{RED}Failed to create commit:{RESET} {}", message);
}

/// Print a success message when PR review push succeeds.
pub fn print_pr_push_success(branch: &str) {
    println!("{GREEN}Pushed{RESET} fixes to {CYAN}{}{RESET}", branch);
}

/// Print an error message when PR review push fails.
pub fn print_pr_push_error(message: &str) {
    println!("{RED}Failed to push:{RESET} {}", message);
}

/// Print a message when push reports already up-to-date.
pub fn print_pr_push_up_to_date() {
    println!("{GRAY}Branch already up-to-date on remote{RESET}");
}

/// Print a summary of what was done based on config.
pub fn print_pr_review_actions_summary(
    commit_enabled: bool,
    push_enabled: bool,
    commit_made: bool,
    push_made: bool,
    no_fixes_needed: bool,
) {
    println!();
    println!("{BOLD}Actions:{RESET}");

    if no_fixes_needed {
        println!("  {GRAY}• No fixes needed - no commit created{RESET}");
        return;
    }

    if !commit_enabled {
        println!("  {GRAY}• Commit: disabled in config{RESET}");
    } else if commit_made {
        println!("  {GREEN}• Commit: created{RESET}");
    } else {
        println!("  {GRAY}• Commit: no changes to commit{RESET}");
    }

    if !push_enabled {
        println!("  {GRAY}• Push: disabled in config{RESET}");
    } else if !commit_made {
        println!("  {GRAY}• Push: skipped (no commit){RESET}");
    } else if push_made {
        println!("  {GREEN}• Push: completed{RESET}");
    } else {
        println!("  {GRAY}• Push: already up-to-date{RESET}");
    }

    println!();
}
