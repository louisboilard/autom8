//! PR creation and management.

use std::process::Command;

use crate::error::Result;
use crate::git::{self, PushResult};
use crate::output::{print_push_already_up_to_date, print_push_success, print_pushing_branch};
use crate::spec::Spec;

use super::detection::{get_existing_pr_number, get_existing_pr_url, pr_exists_for_branch};
use super::format::{format_pr_description, format_pr_title};
use super::types::PRResult;

/// Check if the GitHub CLI (gh) is installed and available in PATH
pub fn is_gh_installed() -> bool {
    Command::new("gh")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Check if the user is authenticated with GitHub CLI
pub fn is_gh_authenticated() -> bool {
    Command::new("gh")
        .args(["auth", "status"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Update the description of an existing pull request
pub fn update_pr_description(spec: &Spec, pr_number: u32) -> Result<PRResult> {
    let body = format_pr_description(spec);

    let output = Command::new("gh")
        .args(["pr", "edit", &pr_number.to_string(), "--body", &body])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Ok(PRResult::Error(format!(
            "Failed to update PR: {}",
            stderr.trim()
        )));
    }

    let url_output = Command::new("gh")
        .args(["pr", "view", &pr_number.to_string(), "--json", "url"])
        .output()?;

    if url_output.status.success() {
        let stdout = String::from_utf8_lossy(&url_output.stdout);
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(stdout.trim()) {
            if let Some(url) = parsed.get("url").and_then(|v| v.as_str()) {
                return Ok(PRResult::Updated(url.to_string()));
            }
        }
    }

    Ok(PRResult::Updated(format!("PR #{}", pr_number)))
}

/// Create a pull request for the current branch using the GitHub CLI
pub fn create_pull_request(spec: &Spec, commits_were_made: bool) -> Result<PRResult> {
    if !commits_were_made {
        return Ok(PRResult::Skipped(
            "No commits were made in this session".to_string(),
        ));
    }

    if !git::is_git_repo() {
        return Ok(PRResult::Skipped("Not in a git repository".to_string()));
    }

    if !is_gh_installed() {
        return Ok(PRResult::Skipped(
            "GitHub CLI (gh) not installed. Install from https://cli.github.com".to_string(),
        ));
    }

    if !is_gh_authenticated() {
        return Ok(PRResult::Skipped(
            "Not authenticated with GitHub CLI. Run 'gh auth login' first".to_string(),
        ));
    }

    let branch = match git::current_branch() {
        Ok(b) => b,
        Err(e) => {
            return Ok(PRResult::Error(format!(
                "Failed to get current branch: {}",
                e
            )))
        }
    };

    if branch == "main" || branch == "master" {
        return Ok(PRResult::Skipped(format!(
            "Cannot create PR from {} branch",
            branch
        )));
    }

    // Check if PR already exists
    match pr_exists_for_branch(&branch)? {
        true => {
            // PR exists - update description instead
            if let Some(pr_number) = get_existing_pr_number(&branch)? {
                return update_pr_description(spec, pr_number);
            } else if let Some(url) = get_existing_pr_url(&branch)? {
                return Ok(PRResult::AlreadyExists(url));
            }
            return Ok(PRResult::AlreadyExists(format!("PR exists for {}", branch)));
        }
        false => {}
    }

    // Ensure branch is pushed
    let push_result = ensure_branch_pushed(&branch)?;
    match push_result {
        PushResult::Error(e) => {
            return Ok(PRResult::Error(format!("Failed to push branch: {}", e)));
        }
        _ => {}
    }

    // Create the PR
    let title = format_pr_title(spec);
    let body = format_pr_description(spec);

    let output = Command::new("gh")
        .args(["pr", "create", "--title", &title, "--body", &body])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Ok(PRResult::Error(format!(
            "Failed to create PR: {}",
            stderr.trim()
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let url = stdout.trim().to_string();

    Ok(PRResult::Success(url))
}

/// Ensure the current branch is pushed to the remote
pub fn ensure_branch_pushed(branch: &str) -> Result<PushResult> {
    print_pushing_branch(branch);
    let result = git::push_branch(branch)?;

    match &result {
        PushResult::Success => print_push_success(),
        PushResult::AlreadyUpToDate => print_push_already_up_to_date(),
        PushResult::Error(_) => {}
    }

    Ok(result)
}
