//! PR creation and management.

use std::process::Command;

use crate::error::Result;
use crate::git::{self, PushResult};
use crate::output::{
    print_push_already_up_to_date, print_push_success, print_pushing_branch, print_warning,
};
use crate::spec::Spec;

use super::detection::{get_existing_pr_number, get_existing_pr_url, pr_exists_for_branch};
use super::format::{format_pr_description, format_pr_title};
use super::template::{detect_pr_template, run_template_agent, TemplateAgentResult};
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
    // Check for PR template in the repository
    let repo_root = std::env::current_dir().unwrap_or_default();
    if let Some(template_content) = detect_pr_template(&repo_root) {
        // Template found - use agent path
        let title = format_pr_title(spec);
        // Draft flag is not applicable when updating existing PRs
        match run_template_agent(
            spec,
            &template_content,
            &title,
            Some(pr_number),
            false,
            |_| {},
        ) {
            Ok(TemplateAgentResult::Success(url)) => {
                return Ok(PRResult::Updated(url));
            }
            Ok(TemplateAgentResult::Error(error_info)) => {
                // Agent failed - fall back to generated description
                print_warning(&format!(
                    "Template agent failed ({}), using generated description",
                    error_info.message
                ));
            }
            Err(e) => {
                // Agent error - fall back to generated description
                print_warning(&format!(
                    "Template agent error ({}), using generated description",
                    e
                ));
            }
        }
    }

    // No template or agent failed - use current generated description path
    update_pr_description_direct(spec, pr_number)
}

/// Update PR description directly using generated format (internal fallback)
fn update_pr_description_direct(spec: &Spec, pr_number: u32) -> Result<PRResult> {
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
pub fn create_pull_request(spec: &Spec, commits_were_made: bool, draft: bool) -> Result<PRResult> {
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
    if pr_exists_for_branch(&branch)? {
        // PR exists - update description instead
        if let Some(pr_number) = get_existing_pr_number(&branch)? {
            return update_pr_description(spec, pr_number);
        } else if let Some(url) = get_existing_pr_url(&branch)? {
            return Ok(PRResult::AlreadyExists(url));
        }
        return Ok(PRResult::AlreadyExists(format!("PR exists for {}", branch)));
    }

    // Ensure branch is pushed
    let push_result = ensure_branch_pushed(&branch)?;
    if let PushResult::Error(e) = push_result {
        return Ok(PRResult::Error(format!("Failed to push branch: {}", e)));
    }

    // Check for PR template in the repository (after prerequisites pass)
    let repo_root = std::env::current_dir().unwrap_or_default();
    if let Some(template_content) = detect_pr_template(&repo_root) {
        // Template found - use agent path
        let title = format_pr_title(spec);
        match run_template_agent(spec, &template_content, &title, None, draft, |_| {}) {
            Ok(TemplateAgentResult::Success(url)) => {
                return Ok(PRResult::Success(url));
            }
            Ok(TemplateAgentResult::Error(error_info)) => {
                // Agent failed - fall back to generated description
                print_warning(&format!(
                    "Template agent failed ({}), using generated description",
                    error_info.message
                ));
            }
            Err(e) => {
                // Agent error - fall back to generated description
                print_warning(&format!(
                    "Template agent error ({}), using generated description",
                    e
                ));
            }
        }
    }

    // No template or agent failed - use current generated description path
    create_pull_request_direct(spec, draft)
}

#[cfg(test)]
fn build_pr_create_args<'a>(title: &'a str, body: &'a str, draft: bool) -> Vec<&'a str> {
    let mut args = vec!["pr", "create", "--title", title, "--body", body];
    if draft {
        args.push("--draft");
    }
    args
}

/// Create PR directly using generated format (internal fallback)
fn create_pull_request_direct(spec: &Spec, draft: bool) -> Result<PRResult> {
    let title = format_pr_title(spec);
    let body = format_pr_description(spec);

    let mut args = vec!["pr", "create", "--title", &title, "--body", &body];
    if draft {
        args.push("--draft");
    }

    let output = Command::new("gh").args(&args).output()?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::UserStory;

    fn make_test_spec() -> Spec {
        Spec {
            project: "TestProject".to_string(),
            branch_name: "feature/test".to_string(),
            description: "Test feature description.".to_string(),
            user_stories: vec![UserStory {
                id: "US-001".to_string(),
                title: "Test Story".to_string(),
                description: "Test story description".to_string(),
                acceptance_criteria: vec!["Criterion 1".to_string()],
                priority: 1,
                passes: true,
                notes: String::new(),
            }],
        }
    }

    // ========================================================================
    // PRResult variant tests
    // ========================================================================

    #[test]
    fn test_pr_result_success_variant() {
        let result = PRResult::Success("https://github.com/owner/repo/pull/1".to_string());
        assert!(matches!(result, PRResult::Success(_)));
    }

    #[test]
    fn test_pr_result_updated_variant() {
        let result = PRResult::Updated("https://github.com/owner/repo/pull/1".to_string());
        assert!(matches!(result, PRResult::Updated(_)));
    }

    #[test]
    fn test_pr_result_already_exists_variant() {
        let result = PRResult::AlreadyExists("https://github.com/owner/repo/pull/1".to_string());
        assert!(matches!(result, PRResult::AlreadyExists(_)));
    }

    #[test]
    fn test_pr_result_skipped_variant() {
        let result = PRResult::Skipped("reason".to_string());
        assert!(matches!(result, PRResult::Skipped(_)));
    }

    #[test]
    fn test_pr_result_error_variant() {
        let result = PRResult::Error("error message".to_string());
        assert!(matches!(result, PRResult::Error(_)));
    }

    // ========================================================================
    // create_pull_request prerequisite tests
    // ========================================================================

    #[test]
    fn test_create_pr_skips_when_no_commits() {
        let spec = make_test_spec();
        let result = create_pull_request(&spec, false, false);
        assert!(result.is_ok());
        match result.unwrap() {
            PRResult::Skipped(msg) => {
                assert!(msg.contains("No commits"));
            }
            _ => panic!("Expected Skipped result"),
        }
    }

    // ========================================================================
    // Template integration behavior tests (unit tests without mocking)
    // ========================================================================

    #[test]
    fn test_detect_pr_template_integration_no_template_in_test_dir() {
        // When running tests, there's no PR template in the repo root (or temp dir)
        // This verifies the integration path where no template is found
        use tempfile::TempDir;
        let temp_dir = TempDir::new().unwrap();
        let result = detect_pr_template(temp_dir.path());
        assert!(result.is_none());
    }

    #[test]
    fn test_detect_pr_template_integration_with_template() {
        use std::fs;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let github_dir = temp_dir.path().join(".github");
        fs::create_dir_all(&github_dir).unwrap();
        fs::write(
            github_dir.join("pull_request_template.md"),
            "## Description\n\nPlease describe your changes.",
        )
        .unwrap();

        let result = detect_pr_template(temp_dir.path());
        assert!(result.is_some());
        assert!(result.unwrap().contains("Description"));
    }

    // ========================================================================
    // Direct function tests (internal fallback functions)
    // ========================================================================

    #[test]
    fn test_create_pull_request_direct_builds_correct_command_args() {
        // This test verifies the direct function exists and can format title/body
        // We can't actually run the gh command in unit tests, but we verify
        // the function compiles and the format functions work correctly
        let spec = make_test_spec();
        let title = format_pr_title(&spec);
        let body = format_pr_description(&spec);

        assert!(title.contains("TestProject"));
        assert!(body.contains("Summary"));
        assert!(body.contains("Test feature description"));
    }

    #[test]
    fn test_update_pr_description_direct_builds_correct_command_args() {
        // Verify the format functions work correctly for updates
        let spec = make_test_spec();
        let body = format_pr_description(&spec);

        assert!(body.contains("Summary"));
        assert!(body.contains("US-001"));
        assert!(body.contains("Test Story"));
    }

    // ========================================================================
    // Draft flag tests
    // ========================================================================

    #[test]
    fn test_build_pr_create_args_without_draft() {
        let title = "Test PR Title";
        let body = "Test PR body content";
        let args = build_pr_create_args(title, body, false);

        assert_eq!(args, vec!["pr", "create", "--title", title, "--body", body]);
        assert!(!args.contains(&"--draft"));
    }

    #[test]
    fn test_build_pr_create_args_with_draft() {
        let title = "Test PR Title";
        let body = "Test PR body content";
        let args = build_pr_create_args(title, body, true);

        assert_eq!(
            args,
            vec!["pr", "create", "--title", title, "--body", body, "--draft"]
        );
        assert!(args.contains(&"--draft"));
    }

    // ========================================================================
    // gh CLI detection tests
    // ========================================================================

    #[test]
    fn test_is_gh_installed_returns_bool() {
        // This test just verifies the function doesn't panic
        // Result depends on whether gh is installed in the test environment
        let _ = is_gh_installed();
    }

    #[test]
    fn test_is_gh_authenticated_returns_bool() {
        // This test just verifies the function doesn't panic
        // Result depends on whether gh is authenticated in the test environment
        let _ = is_gh_authenticated();
    }
}
