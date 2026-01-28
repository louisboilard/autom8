//! Pull request operation output.
//!
//! Output functions for PR creation, detection, and branch switching.

use super::colors::*;

/// Print a prominent success message for a created PR with its URL.
pub fn print_pr_success(url: &str) {
    println!();
    println!("{GREEN}{BOLD}╔════════════════════════════════════════════════════════╗{RESET}");
    println!("{GREEN}{BOLD}║  ✓ Pull Request Created                                ║{RESET}");
    println!("{GREEN}{BOLD}╚════════════════════════════════════════════════════════╝{RESET}");
    println!();
    println!("{GREEN}{BOLD}  {}{RESET}", url);
    println!();
}

/// Print a prominent message when a PR already exists for the branch.
pub fn print_pr_already_exists(url: &str) {
    println!();
    println!("{CYAN}{BOLD}╔════════════════════════════════════════════════════════╗{RESET}");
    println!("{CYAN}{BOLD}║  ℹ Pull Request Already Exists                         ║{RESET}");
    println!("{CYAN}{BOLD}╚════════════════════════════════════════════════════════╝{RESET}");
    println!();
    println!("{CYAN}{BOLD}  {}{RESET}", url);
    println!();
}

/// Print a skip message for PR creation with the reason.
pub fn print_pr_skipped(reason: &str) {
    println!("{GRAY}PR creation skipped: {}{RESET}", reason);
}

/// Print a prominent message when a PR description has been updated.
pub fn print_pr_updated(url: &str) {
    println!();
    println!("{GREEN}{BOLD}╔════════════════════════════════════════════════════════╗{RESET}");
    println!("{GREEN}{BOLD}║  ✓ Pull Request Updated                                ║{RESET}");
    println!("{GREEN}{BOLD}╚════════════════════════════════════════════════════════╝{RESET}");
    println!();
    println!("{GREEN}{BOLD}  {}{RESET}", url);
    println!();
}

/// Print a status message when pushing branch to remote.
pub fn print_pushing_branch(branch: &str) {
    println!("{CYAN}Pushing branch '{}'...{RESET}", branch);
}

/// Print a success message when branch push completes.
pub fn print_push_success() {
    println!("{GREEN}Branch pushed successfully.{RESET}");
}

/// Print a message when branch is already up-to-date on remote.
pub fn print_push_already_up_to_date() {
    println!("{GRAY}Branch already up-to-date on remote.{RESET}");
}

/// Print a message when no open PRs exist in the repository.
pub fn print_no_open_prs() {
    println!();
    println!("{YELLOW}No open pull requests found in this repository.{RESET}");
    println!();
    println!("{GRAY}Create a PR first with 'gh pr create' or push a branch with changes.{RESET}");
}

/// Print a message when a PR was detected for the current branch.
pub fn print_pr_detected(pr_number: u32, title: &str, branch: &str) {
    println!();
    println!(
        "{GREEN}Detected PR #{}{RESET} for branch {CYAN}{}{RESET}",
        pr_number, branch
    );
    println!("{BLUE}Title:{RESET} {}", title);
    println!();
}

/// Print a message when switching to a different branch.
pub fn print_switching_branch(from_branch: &str, to_branch: &str) {
    println!(
        "{CYAN}Switching{RESET} from {GRAY}{}{RESET} to {CYAN}{}{RESET}...",
        from_branch, to_branch
    );
}

/// Print a success message when branch switch completes.
pub fn print_branch_switched(branch: &str) {
    println!("{GREEN}Now on branch:{RESET} {}", branch);
    println!();
}

/// Format a PR for display in a selection list.
///
/// Returns a formatted string like: "#123 feature/add-auth (Add authentication)"
pub fn format_pr_for_selection(number: u32, branch: &str, title: &str) -> String {
    // Truncate title if too long
    let max_title_len = 50;
    let display_title = if title.len() > max_title_len {
        format!("{}...", &title[..max_title_len - 3])
    } else {
        title.to_string()
    };

    format!("#{} {} ({})", number, branch, display_title)
}
