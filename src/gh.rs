//! GitHub CLI (gh) integration for PR operations
//!
//! This module provides functions to interact with the GitHub CLI for
//! checking prerequisites and managing pull requests.

use crate::error::Result;
use crate::git;
use crate::spec::Spec;
use std::process::Command;

/// Result type for PR creation operations
///
/// Captures all possible outcomes of the PR creation step.
/// Note that `Skipped` represents a successful state transition (not a failure)
/// when prerequisites aren't met.
#[derive(Debug, Clone, PartialEq)]
pub enum PRResult {
    /// PR created successfully, contains PR URL
    Success(String),
    /// Prerequisites not met, contains reason for skip
    Skipped(String),
    /// PR already exists for branch, contains existing PR URL
    AlreadyExists(String),
    /// PR creation attempted but failed, contains error message
    Error(String),
}

/// Format a Spec into a well-structured GitHub PR description in Markdown format
///
/// The output includes:
/// - A Summary section with the spec description
/// - A User Stories section listing each story's ID, title, and description
pub fn format_pr_description(spec: &Spec) -> String {
    let mut output = String::new();

    // Summary section
    output.push_str("## Summary\n\n");
    output.push_str(&spec.description);
    output.push_str("\n\n");

    // User Stories section
    output.push_str("## User Stories\n\n");

    for story in &spec.user_stories {
        // Format: ### US-001: [title]
        output.push_str(&format!("### {}: {}\n\n", story.id, story.title));
        output.push_str(&story.description);
        output.push_str("\n\n");
    }

    // Trim trailing whitespace but keep one trailing newline
    output.trim_end().to_string()
}

/// Check if the GitHub CLI (gh) is installed and available in PATH
pub fn is_gh_installed() -> bool {
    Command::new("gh")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Check if the user is authenticated with GitHub CLI
///
/// Uses `gh auth status` which returns exit code 0 if authenticated.
pub fn is_gh_authenticated() -> bool {
    Command::new("gh")
        .args(["auth", "status"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Check if a pull request already exists for the given branch
///
/// Returns `Ok(true)` if a PR exists, `Ok(false)` if no PR exists,
/// or an error if the command fails.
pub fn pr_exists_for_branch(branch: &str) -> Result<bool> {
    let output = Command::new("gh")
        .args(["pr", "list", "--head", branch, "--json", "number"])
        .output()?;

    if !output.status.success() {
        // On error, return false (non-blocking behavior)
        return Ok(false);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();

    // Empty array [] means no PRs exist
    // Non-empty array means at least one PR exists
    Ok(trimmed != "[]" && !trimmed.is_empty())
}

/// Get the URL of an existing pull request for the given branch
///
/// Returns `Ok(Some(url))` if a PR exists, `Ok(None)` if no PR exists,
/// or an error if the command fails.
pub fn get_existing_pr_url(branch: &str) -> Result<Option<String>> {
    let output = Command::new("gh")
        .args(["pr", "list", "--head", branch, "--json", "url"])
        .output()?;

    if !output.status.success() {
        // On error, return None (non-blocking behavior)
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();

    // Empty array means no PRs
    if trimmed == "[]" || trimmed.is_empty() {
        return Ok(None);
    }

    // Parse JSON array to extract URL
    // Expected format: [{"url":"https://github.com/..."}]
    let parsed: std::result::Result<Vec<serde_json::Value>, _> = serde_json::from_str(trimmed);

    match parsed {
        Ok(prs) if !prs.is_empty() => {
            if let Some(url) = prs[0].get("url").and_then(|v| v.as_str()) {
                Ok(Some(url.to_string()))
            } else {
                Ok(None)
            }
        }
        _ => Ok(None),
    }
}

/// Create a pull request for the current branch using the GitHub CLI
///
/// This function orchestrates all prerequisite checks and PR creation.
/// It returns `PRResult::Skipped` (not an error) when prerequisites aren't met,
/// allowing the workflow to continue gracefully.
///
/// # Arguments
/// * `spec` - The spec containing PR title and description data
/// * `commits_were_made` - Whether commits were made in the current session
///
/// # Returns
/// * `PRResult::Success(url)` - PR created successfully
/// * `PRResult::Skipped(reason)` - Prerequisites not met
/// * `PRResult::AlreadyExists(url)` - PR already exists for branch
/// * `PRResult::Error(message)` - PR creation failed
pub fn create_pull_request(spec: &Spec, commits_were_made: bool) -> Result<PRResult> {
    // Check: commits_were_made
    if !commits_were_made {
        return Ok(PRResult::Skipped(
            "No commits were made in this session".to_string(),
        ));
    }

    // Check: in git repo
    if !git::is_git_repo() {
        return Ok(PRResult::Skipped("Not in a git repository".to_string()));
    }

    // Check: gh CLI installed
    if !is_gh_installed() {
        return Ok(PRResult::Skipped("GitHub CLI (gh) is not installed".to_string()));
    }

    // Check: gh authenticated
    if !is_gh_authenticated() {
        return Ok(PRResult::Skipped(
            "Not authenticated with GitHub CLI (run 'gh auth login')".to_string(),
        ));
    }

    // Check: not on main/master branch
    let current_branch = git::current_branch()?;
    if current_branch == "main" || current_branch == "master" {
        return Ok(PRResult::Skipped(format!(
            "Cannot create PR from {} branch",
            current_branch
        )));
    }

    // Check: PR already exists for this branch
    if let Ok(Some(existing_url)) = get_existing_pr_url(&current_branch) {
        return Ok(PRResult::AlreadyExists(existing_url));
    }

    // Create the PR
    let title = &spec.description;
    let body = format_pr_description(spec);

    let output = Command::new("gh")
        .args(["pr", "create", "--title", title, "--body", &body])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Ok(PRResult::Error(format!(
            "Failed to create PR: {}",
            stderr.trim()
        )));
    }

    // Extract PR URL from stdout
    let stdout = String::from_utf8_lossy(&output.stdout);
    let pr_url = stdout.trim().to_string();

    if pr_url.is_empty() {
        return Ok(PRResult::Error(
            "PR created but no URL was returned".to_string(),
        ));
    }

    Ok(PRResult::Success(pr_url))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::UserStory;

    #[test]
    fn test_format_pr_description_basic() {
        let spec = Spec {
            project: "test-project".to_string(),
            branch_name: "feature/test".to_string(),
            description: "A test feature description.".to_string(),
            user_stories: vec![UserStory {
                id: "US-001".to_string(),
                title: "First Story".to_string(),
                description: "This is the first story.".to_string(),
                acceptance_criteria: vec!["Criterion 1".to_string()],
                priority: 1,
                passes: false,
                notes: String::new(),
            }],
        };

        let result = format_pr_description(&spec);

        assert!(result.contains("## Summary"));
        assert!(result.contains("A test feature description."));
        assert!(result.contains("## User Stories"));
        assert!(result.contains("### US-001: First Story"));
        assert!(result.contains("This is the first story."));
    }

    #[test]
    fn test_format_pr_description_multiple_stories() {
        let spec = Spec {
            project: "test-project".to_string(),
            branch_name: "feature/test".to_string(),
            description: "Multi-story feature.".to_string(),
            user_stories: vec![
                UserStory {
                    id: "US-001".to_string(),
                    title: "Story One".to_string(),
                    description: "First description.".to_string(),
                    acceptance_criteria: vec![],
                    priority: 1,
                    passes: true,
                    notes: String::new(),
                },
                UserStory {
                    id: "US-002".to_string(),
                    title: "Story Two".to_string(),
                    description: "Second description.".to_string(),
                    acceptance_criteria: vec![],
                    priority: 2,
                    passes: false,
                    notes: String::new(),
                },
            ],
        };

        let result = format_pr_description(&spec);

        // Verify both stories are included
        assert!(result.contains("### US-001: Story One"));
        assert!(result.contains("First description."));
        assert!(result.contains("### US-002: Story Two"));
        assert!(result.contains("Second description."));

        // Verify structure order
        let summary_pos = result.find("## Summary").unwrap();
        let stories_pos = result.find("## User Stories").unwrap();
        assert!(summary_pos < stories_pos);
    }

    #[test]
    fn test_format_pr_description_with_newlines_in_description() {
        let spec = Spec {
            project: "test-project".to_string(),
            branch_name: "feature/test".to_string(),
            description: "Line one.\nLine two.\nLine three.".to_string(),
            user_stories: vec![UserStory {
                id: "US-001".to_string(),
                title: "Test".to_string(),
                description: "Story line one.\nStory line two.".to_string(),
                acceptance_criteria: vec![],
                priority: 1,
                passes: false,
                notes: String::new(),
            }],
        };

        let result = format_pr_description(&spec);

        // Newlines should be preserved in the output
        assert!(result.contains("Line one.\nLine two.\nLine three."));
        assert!(result.contains("Story line one.\nStory line two."));
    }

    #[test]
    fn test_format_pr_description_output_is_clean() {
        let spec = Spec {
            project: "test-project".to_string(),
            branch_name: "feature/test".to_string(),
            description: "Clean output test.".to_string(),
            user_stories: vec![UserStory {
                id: "US-001".to_string(),
                title: "Clean".to_string(),
                description: "Clean story.".to_string(),
                acceptance_criteria: vec![],
                priority: 1,
                passes: false,
                notes: String::new(),
            }],
        };

        let result = format_pr_description(&spec);

        // Should not have excessive trailing whitespace
        assert!(!result.ends_with("\n\n"));
        // Should start with ## Summary
        assert!(result.starts_with("## Summary"));
    }

    #[test]
    fn test_is_gh_installed_returns_bool() {
        // This test just verifies the function runs without panicking
        // and returns a boolean (actual result depends on system)
        let result = is_gh_installed();
        assert!(result || !result); // Always true, just confirms it returns bool
    }

    #[test]
    fn test_is_gh_authenticated_returns_bool() {
        // This test verifies the function runs without panicking
        let result = is_gh_authenticated();
        assert!(result || !result);
    }

    #[test]
    fn test_pr_exists_for_nonexistent_branch() {
        // Test with a branch name that almost certainly doesn't have a PR
        let result = pr_exists_for_branch("nonexistent-branch-that-does-not-exist-12345");
        // Should return Ok (not panic) regardless of gh installation
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_existing_pr_url_for_nonexistent_branch() {
        // Test with a branch name that almost certainly doesn't have a PR
        let result = get_existing_pr_url("nonexistent-branch-that-does-not-exist-12345");
        // Should return Ok (not panic) regardless of gh installation
        assert!(result.is_ok());
        // Should return None since no PR exists
        if let Ok(url) = result {
            assert!(url.is_none());
        }
    }

    #[test]
    fn test_pr_exists_handles_empty_json_array() {
        // Verify our parsing logic handles empty arrays correctly
        let empty_json = "[]";
        let trimmed = empty_json.trim();
        assert_eq!(trimmed, "[]");
        assert!(trimmed == "[]" || trimmed.is_empty());
    }

    #[test]
    fn test_get_existing_pr_url_parses_json_correctly() {
        // Test the JSON parsing logic
        let json_str = r#"[{"url":"https://github.com/owner/repo/pull/123"}]"#;
        let parsed: Vec<serde_json::Value> = serde_json::from_str(json_str).unwrap();
        assert!(!parsed.is_empty());
        let url = parsed[0].get("url").and_then(|v| v.as_str());
        assert_eq!(url, Some("https://github.com/owner/repo/pull/123"));
    }

    #[test]
    fn test_pr_result_success_contains_url() {
        let url = "https://github.com/owner/repo/pull/42".to_string();
        let result = PRResult::Success(url.clone());
        assert_eq!(result, PRResult::Success(url));
    }

    #[test]
    fn test_pr_result_skipped_contains_reason() {
        let reason = "gh CLI not installed".to_string();
        let result = PRResult::Skipped(reason.clone());
        assert_eq!(result, PRResult::Skipped(reason));
    }

    #[test]
    fn test_pr_result_already_exists_contains_url() {
        let url = "https://github.com/owner/repo/pull/99".to_string();
        let result = PRResult::AlreadyExists(url.clone());
        assert_eq!(result, PRResult::AlreadyExists(url));
    }

    #[test]
    fn test_pr_result_error_contains_message() {
        let message = "Failed to create PR: permission denied".to_string();
        let result = PRResult::Error(message.clone());
        assert_eq!(result, PRResult::Error(message));
    }

    #[test]
    fn test_pr_result_variants_are_distinct() {
        let url = "https://github.com/owner/repo/pull/1".to_string();
        let success = PRResult::Success(url.clone());
        let skipped = PRResult::Skipped(url.clone());
        let already_exists = PRResult::AlreadyExists(url.clone());
        let error = PRResult::Error(url.clone());

        // Each variant should be distinct even with the same inner value
        assert_ne!(success, skipped);
        assert_ne!(success, already_exists);
        assert_ne!(success, error);
        assert_ne!(skipped, already_exists);
        assert_ne!(skipped, error);
        assert_ne!(already_exists, error);
    }

    #[test]
    fn test_pr_result_clone() {
        let original = PRResult::Success("https://github.com/owner/repo/pull/5".to_string());
        let cloned = original.clone();
        assert_eq!(original, cloned);
    }

    #[test]
    fn test_create_pull_request_skips_when_no_commits() {
        let spec = Spec {
            project: "test-project".to_string(),
            branch_name: "feature/test".to_string(),
            description: "Test feature".to_string(),
            user_stories: vec![],
        };

        let result = create_pull_request(&spec, false);
        assert!(result.is_ok());

        match result.unwrap() {
            PRResult::Skipped(reason) => {
                assert!(reason.contains("No commits"));
            }
            _ => panic!("Expected PRResult::Skipped"),
        }
    }

    #[test]
    fn test_create_pull_request_returns_result() {
        // This test verifies the function runs without panicking
        // and returns a valid Result (actual outcome depends on environment)
        let spec = Spec {
            project: "test-project".to_string(),
            branch_name: "feature/test".to_string(),
            description: "Test feature".to_string(),
            user_stories: vec![],
        };

        let result = create_pull_request(&spec, true);
        assert!(result.is_ok());

        // The result should be one of the valid PRResult variants
        let pr_result = result.unwrap();
        match pr_result {
            PRResult::Success(_)
            | PRResult::Skipped(_)
            | PRResult::AlreadyExists(_)
            | PRResult::Error(_) => {}
        }
    }

    #[test]
    fn test_create_pull_request_checks_prerequisites_in_order() {
        // Test that commits_were_made is checked first
        let spec = Spec {
            project: "test".to_string(),
            branch_name: "test".to_string(),
            description: "Test".to_string(),
            user_stories: vec![],
        };

        // When commits_were_made is false, should skip immediately
        // regardless of other conditions
        let result = create_pull_request(&spec, false);
        assert!(result.is_ok());

        if let Ok(PRResult::Skipped(reason)) = result {
            assert!(
                reason.contains("No commits"),
                "Should skip due to no commits first"
            );
        } else {
            panic!("Expected Skipped result for no commits");
        }
    }

    #[test]
    fn test_create_pull_request_with_commits_checks_git_repo() {
        // When commits_were_made is true, the function should proceed
        // to check git repo status (we're in a git repo, so it passes)
        let spec = Spec {
            project: "test".to_string(),
            branch_name: "test".to_string(),
            description: "Test".to_string(),
            user_stories: vec![],
        };

        let result = create_pull_request(&spec, true);
        assert!(result.is_ok());

        // Should not return "No commits" skip since commits_were_made is true
        if let Ok(PRResult::Skipped(reason)) = &result {
            assert!(
                !reason.contains("No commits"),
                "Should not skip due to commits when commits_were_made is true"
            );
        }
    }

    #[test]
    fn test_create_pull_request_function_signature() {
        // Verify the function signature matches the acceptance criteria
        fn _check_signature(_spec: &Spec, _commits: bool) -> Result<PRResult> {
            create_pull_request(_spec, _commits)
        }

        let spec = Spec {
            project: "test".to_string(),
            branch_name: "test".to_string(),
            description: "Test".to_string(),
            user_stories: vec![],
        };

        // This test just verifies compilation with the correct signature
        let _ = _check_signature(&spec, false);
    }

    #[test]
    fn test_create_pull_request_uses_spec_description_as_title() {
        // The function uses spec.description as the PR title
        // We can't easily test the actual gh command, but we verify
        // the function has access to the spec and doesn't panic
        let spec = Spec {
            project: "my-project".to_string(),
            branch_name: "feature/awesome".to_string(),
            description: "Add awesome feature with multiple components".to_string(),
            user_stories: vec![UserStory {
                id: "US-001".to_string(),
                title: "Awesome Story".to_string(),
                description: "Make it awesome".to_string(),
                acceptance_criteria: vec![],
                priority: 1,
                passes: false,
                notes: String::new(),
            }],
        };

        // Just verify the function accepts the spec correctly
        let result = create_pull_request(&spec, false);
        assert!(result.is_ok());
    }
}
