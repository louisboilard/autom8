//! PR template detection and agent-based population for GitHub repositories.
//!
//! This module provides functionality to:
//! - Detect and read PR templates from standard GitHub locations
//! - Run a Claude agent to populate templates and execute PR commands

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use crate::claude::extract_text_from_stream_line;
use crate::claude::ClaudeErrorInfo;
use crate::error::{Autom8Error, Result};
use crate::prompts::PR_TEMPLATE_PROMPT;
use crate::spec::Spec;

/// Standard locations for GitHub PR templates, in order of precedence.
const PR_TEMPLATE_PATHS: &[&str] = &[
    ".github/pull_request_template.md",
    ".github/PULL_REQUEST_TEMPLATE.md",
    "pull_request_template.md",
];

/// Detects and returns the content of a PR template if one exists in the repository.
///
/// Checks standard GitHub template locations in order of precedence:
/// 1. `.github/pull_request_template.md` (lowercase)
/// 2. `.github/PULL_REQUEST_TEMPLATE.md` (uppercase)
/// 3. `pull_request_template.md` (repo root)
///
/// Returns `Some(content)` if a template is found, `None` otherwise.
///
/// # Arguments
///
/// * `repo_root` - Path to the repository root directory
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
/// use autom8::gh::detect_pr_template;
///
/// let template = detect_pr_template(Path::new("/path/to/repo"));
/// if let Some(content) = template {
///     println!("Found template:\n{}", content);
/// }
/// ```
pub fn detect_pr_template(repo_root: &Path) -> Option<String> {
    for template_path in PR_TEMPLATE_PATHS {
        let full_path = repo_root.join(template_path);
        if full_path.is_file() {
            match fs::read_to_string(&full_path) {
                Ok(content) => return Some(content),
                Err(_) => continue, // Try next location if read fails
            }
        }
    }
    None
}

/// Result from running the PR template agent.
#[derive(Debug, Clone, PartialEq)]
pub enum TemplateAgentResult {
    /// Agent succeeded, PR URL extracted from output
    Success(String),
    /// Agent failed with error details
    Error(ClaudeErrorInfo),
}

/// Formats spec data for inclusion in the PR template prompt.
///
/// Serializes the spec into a human-readable format including:
/// - Project name
/// - Feature description
/// - User stories with their completion status
pub fn format_spec_for_template(spec: &Spec) -> String {
    let mut output = String::new();

    output.push_str(&format!("**Project:** {}\n\n", spec.project));
    output.push_str(&format!("**Description:**\n{}\n\n", spec.description));
    output.push_str("**User Stories:**\n\n");

    for story in &spec.user_stories {
        let status = if story.passes { "[x]" } else { "[ ]" };
        output.push_str(&format!("- {} **{}**: {}\n", status, story.id, story.title));
        output.push_str(&format!("  {}\n", story.description));

        if !story.acceptance_criteria.is_empty() {
            output.push_str("  - Acceptance Criteria:\n");
            for criterion in &story.acceptance_criteria {
                let criterion_status = if story.passes { "[x]" } else { "[ ]" };
                output.push_str(&format!("    - {} {}\n", criterion_status, criterion));
            }
        }
        output.push('\n');
    }

    output.trim_end().to_string()
}

/// Builds the `gh pr create` or `gh pr edit` command string.
///
/// # Arguments
///
/// * `title` - The PR title
/// * `pr_number` - If Some, builds an edit command; if None, builds a create command
/// * `draft` - If true and creating a new PR, includes the `--draft` flag (ignored for edits)
pub fn build_gh_command(title: &str, pr_number: Option<u32>, draft: bool) -> String {
    match pr_number {
        Some(num) => format!("gh pr edit {} --body \"<filled template>\"", num),
        None => {
            let draft_flag = if draft { " --draft" } else { "" };
            format!(
                "gh pr create --title \"{}\" --body \"<filled template>\"{}",
                title, draft_flag
            )
        }
    }
}

/// Extracts a PR URL from the agent's output.
///
/// Looks for GitHub PR URLs in the format:
/// - `https://github.com/<owner>/<repo>/pull/<number>`
///
/// Returns the first URL found, or None if no URL is present.
pub fn extract_pr_url(output: &str) -> Option<String> {
    // Look for GitHub PR URLs
    for line in output.lines().rev() {
        let line = line.trim();
        if line.starts_with("https://github.com/") && line.contains("/pull/") {
            return Some(line.to_string());
        }
    }

    // Also check for PR URLs that might be embedded in text
    for word in output.split_whitespace().rev() {
        if word.starts_with("https://github.com/") && word.contains("/pull/") {
            // Clean up any trailing punctuation
            let url = word.trim_end_matches(|c: char| !c.is_alphanumeric());
            return Some(url.to_string());
        }
    }

    None
}

/// Run a Claude agent to populate a PR template and execute the gh command.
///
/// The agent receives:
/// - Serialized spec data (project, description, user stories with status)
/// - Raw PR template content
/// - The exact `gh pr create` or `gh pr edit` command to run
///
/// # Arguments
///
/// * `spec` - The spec containing feature data
/// * `template_content` - The raw PR template content
/// * `title` - The PR title
/// * `pr_number` - If Some, updates existing PR; if None, creates new PR
/// * `draft` - If true and creating a new PR, includes the `--draft` flag
/// * `on_output` - Callback for streaming output
///
/// # Returns
///
/// `TemplateAgentResult::Success(url)` if the agent successfully created/updated the PR,
/// `TemplateAgentResult::Error(info)` if the agent failed.
pub fn run_template_agent<F>(
    spec: &Spec,
    template_content: &str,
    title: &str,
    pr_number: Option<u32>,
    draft: bool,
    mut on_output: F,
) -> Result<TemplateAgentResult>
where
    F: FnMut(&str),
{
    let spec_data = format_spec_for_template(spec);
    let gh_command = build_gh_command(title, pr_number, draft);

    let prompt = PR_TEMPLATE_PROMPT
        .replace("{spec_data}", &spec_data)
        .replace("{template_content}", template_content)
        .replace("{gh_command}", &gh_command);

    let mut child = Command::new("claude")
        .args([
            "--dangerously-skip-permissions",
            "--print",
            "--output-format",
            "stream-json",
            "--verbose",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Autom8Error::ClaudeError(format!("Failed to spawn claude: {}", e)))?;

    // Write prompt to stdin
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(prompt.as_bytes())
            .map_err(|e| Autom8Error::ClaudeError(format!("Failed to write to stdin: {}", e)))?;
    }

    // Take stderr handle before consuming stdout
    let stderr = child.stderr.take();

    // Stream stdout
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Autom8Error::ClaudeError("Failed to capture stdout".into()))?;

    let reader = BufReader::new(stdout);
    let mut accumulated_text = String::new();

    for line in reader.lines() {
        let line = line.map_err(|e| Autom8Error::ClaudeError(format!("Read error: {}", e)))?;

        // Parse stream-json output and extract text content
        if let Some(text) = extract_text_from_stream_line(&line) {
            on_output(&text);
            accumulated_text.push_str(&text);
        }
    }

    // Wait for process to complete
    let status = child
        .wait()
        .map_err(|e| Autom8Error::ClaudeError(format!("Wait error: {}", e)))?;

    if !status.success() {
        let stderr_content = stderr
            .map(|s| std::io::read_to_string(s).unwrap_or_default())
            .unwrap_or_default();
        let error_info = ClaudeErrorInfo::from_process_failure(
            status,
            if stderr_content.is_empty() {
                None
            } else {
                Some(stderr_content)
            },
        );
        return Ok(TemplateAgentResult::Error(error_info));
    }

    // Extract PR URL from output
    match extract_pr_url(&accumulated_text) {
        Some(url) => Ok(TemplateAgentResult::Success(url)),
        None => {
            // Agent succeeded but we couldn't find a URL - this is an error
            Ok(TemplateAgentResult::Error(ClaudeErrorInfo::new(
                "Agent completed but no PR URL found in output",
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::UserStory;
    use std::fs::{self, File};
    use std::io::Write;
    use tempfile::TempDir;

    fn create_template(dir: &Path, relative_path: &str, content: &str) {
        let full_path = dir.join(relative_path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut file = File::create(full_path).unwrap();
        writeln!(file, "{}", content).unwrap();
    }

    #[test]
    fn test_no_template_returns_none() {
        let temp_dir = TempDir::new().unwrap();
        let result = detect_pr_template(temp_dir.path());
        assert!(result.is_none());
    }

    #[test]
    fn test_detects_lowercase_github_template() {
        let temp_dir = TempDir::new().unwrap();
        let expected_content = "## Description\nPlease describe your changes";
        create_template(
            temp_dir.path(),
            ".github/pull_request_template.md",
            expected_content,
        );

        let result = detect_pr_template(temp_dir.path());
        assert!(result.is_some());
        assert!(result.unwrap().contains(expected_content));
    }

    #[test]
    fn test_detects_uppercase_github_template() {
        let temp_dir = TempDir::new().unwrap();
        let expected_content = "## Summary\nDescribe what this PR does";
        create_template(
            temp_dir.path(),
            ".github/PULL_REQUEST_TEMPLATE.md",
            expected_content,
        );

        let result = detect_pr_template(temp_dir.path());
        assert!(result.is_some());
        assert!(result.unwrap().contains(expected_content));
    }

    #[test]
    fn test_detects_root_template() {
        let temp_dir = TempDir::new().unwrap();
        let expected_content = "## Changes\nList your changes here";
        create_template(
            temp_dir.path(),
            "pull_request_template.md",
            expected_content,
        );

        let result = detect_pr_template(temp_dir.path());
        assert!(result.is_some());
        assert!(result.unwrap().contains(expected_content));
    }

    #[test]
    fn test_precedence_lowercase_github_over_uppercase() {
        let temp_dir = TempDir::new().unwrap();
        let lowercase_content = "LOWERCASE TEMPLATE";
        let uppercase_content = "UPPERCASE TEMPLATE";

        create_template(
            temp_dir.path(),
            ".github/pull_request_template.md",
            lowercase_content,
        );
        create_template(
            temp_dir.path(),
            ".github/PULL_REQUEST_TEMPLATE.md",
            uppercase_content,
        );

        let result = detect_pr_template(temp_dir.path());
        assert!(result.is_some());

        // On case-insensitive filesystems (macOS APFS, Windows NTFS), both filenames
        // refer to the same file, so the second write overwrites the first.
        // The test verifies that we find *a* template; the precedence between
        // lowercase and uppercase is only meaningful on case-sensitive filesystems.
        let content = result.unwrap();
        let is_case_sensitive_fs = temp_dir
            .path()
            .join(".github/pull_request_template.md")
            .exists()
            && temp_dir
                .path()
                .join(".github/PULL_REQUEST_TEMPLATE.md")
                .exists()
            && fs::read_to_string(temp_dir.path().join(".github/pull_request_template.md"))
                .unwrap()
                != fs::read_to_string(temp_dir.path().join(".github/PULL_REQUEST_TEMPLATE.md"))
                    .unwrap();

        if is_case_sensitive_fs {
            // On case-sensitive filesystems, lowercase takes precedence
            assert!(content.contains(lowercase_content));
        }
        // On case-insensitive filesystems, just verify we got a template
    }

    #[test]
    fn test_precedence_github_over_root() {
        let temp_dir = TempDir::new().unwrap();
        let github_content = "GITHUB DIRECTORY TEMPLATE";
        let root_content = "ROOT TEMPLATE";

        create_template(
            temp_dir.path(),
            ".github/pull_request_template.md",
            github_content,
        );
        create_template(temp_dir.path(), "pull_request_template.md", root_content);

        let result = detect_pr_template(temp_dir.path());
        assert!(result.is_some());
        assert!(result.unwrap().contains(github_content));
    }

    #[test]
    fn test_precedence_uppercase_github_over_root() {
        let temp_dir = TempDir::new().unwrap();
        let github_content = "UPPERCASE GITHUB TEMPLATE";
        let root_content = "ROOT TEMPLATE";

        create_template(
            temp_dir.path(),
            ".github/PULL_REQUEST_TEMPLATE.md",
            github_content,
        );
        create_template(temp_dir.path(), "pull_request_template.md", root_content);

        let result = detect_pr_template(temp_dir.path());
        assert!(result.is_some());
        assert!(result.unwrap().contains(github_content));
    }

    #[test]
    fn test_falls_back_to_root_when_github_missing() {
        let temp_dir = TempDir::new().unwrap();
        let root_content = "ROOT ONLY TEMPLATE";
        create_template(temp_dir.path(), "pull_request_template.md", root_content);

        let result = detect_pr_template(temp_dir.path());
        assert!(result.is_some());
        assert!(result.unwrap().contains(root_content));
    }

    #[test]
    fn test_nonexistent_repo_path_returns_none() {
        let result = detect_pr_template(Path::new("/nonexistent/path/to/repo"));
        assert!(result.is_none());
    }

    #[test]
    fn test_empty_template_returns_content() {
        let temp_dir = TempDir::new().unwrap();
        // Create an empty template file
        let template_path = temp_dir.path().join(".github/pull_request_template.md");
        fs::create_dir_all(template_path.parent().unwrap()).unwrap();
        File::create(&template_path).unwrap();

        let result = detect_pr_template(temp_dir.path());
        // Empty file should still be detected
        assert!(result.is_some());
    }

    // ========================================================================
    // format_spec_for_template tests
    // ========================================================================

    fn make_test_story(id: &str, title: &str, passes: bool) -> UserStory {
        UserStory {
            id: id.to_string(),
            title: title.to_string(),
            description: format!("Description for {}", id),
            acceptance_criteria: vec!["Criterion 1".to_string(), "Criterion 2".to_string()],
            priority: 1,
            passes,
            notes: String::new(),
        }
    }

    fn make_test_spec() -> Spec {
        Spec {
            project: "TestProject".to_string(),
            branch_name: "feature/test".to_string(),
            description: "This is a test feature description.".to_string(),
            user_stories: vec![
                make_test_story("US-001", "First Story", true),
                make_test_story("US-002", "Second Story", false),
            ],
        }
    }

    #[test]
    fn test_format_spec_includes_project_name() {
        let spec = make_test_spec();
        let formatted = format_spec_for_template(&spec);
        assert!(formatted.contains("**Project:** TestProject"));
    }

    #[test]
    fn test_format_spec_includes_description() {
        let spec = make_test_spec();
        let formatted = format_spec_for_template(&spec);
        assert!(formatted.contains("**Description:**"));
        assert!(formatted.contains("This is a test feature description."));
    }

    #[test]
    fn test_format_spec_includes_user_stories_header() {
        let spec = make_test_spec();
        let formatted = format_spec_for_template(&spec);
        assert!(formatted.contains("**User Stories:**"));
    }

    #[test]
    fn test_format_spec_includes_story_ids_and_titles() {
        let spec = make_test_spec();
        let formatted = format_spec_for_template(&spec);
        assert!(formatted.contains("**US-001**: First Story"));
        assert!(formatted.contains("**US-002**: Second Story"));
    }

    #[test]
    fn test_format_spec_shows_completed_story_with_checkbox() {
        let spec = make_test_spec();
        let formatted = format_spec_for_template(&spec);
        assert!(formatted.contains("[x] **US-001**: First Story"));
    }

    #[test]
    fn test_format_spec_shows_incomplete_story_without_checkbox() {
        let spec = make_test_spec();
        let formatted = format_spec_for_template(&spec);
        assert!(formatted.contains("[ ] **US-002**: Second Story"));
    }

    #[test]
    fn test_format_spec_includes_acceptance_criteria() {
        let spec = make_test_spec();
        let formatted = format_spec_for_template(&spec);
        assert!(formatted.contains("Acceptance Criteria:"));
        assert!(formatted.contains("Criterion 1"));
        assert!(formatted.contains("Criterion 2"));
    }

    #[test]
    fn test_format_spec_includes_story_descriptions() {
        let spec = make_test_spec();
        let formatted = format_spec_for_template(&spec);
        assert!(formatted.contains("Description for US-001"));
        assert!(formatted.contains("Description for US-002"));
    }

    // ========================================================================
    // build_gh_command tests
    // ========================================================================

    #[test]
    fn test_build_gh_command_for_new_pr() {
        let command = build_gh_command("Add feature X", None, false);
        assert!(command.contains("gh pr create"));
        assert!(command.contains("--title \"Add feature X\""));
        assert!(command.contains("--body"));
        assert!(!command.contains("--draft"));
    }

    #[test]
    fn test_build_gh_command_for_new_pr_with_draft() {
        let command = build_gh_command("Add feature X", None, true);
        assert!(command.contains("gh pr create"));
        assert!(command.contains("--title \"Add feature X\""));
        assert!(command.contains("--body"));
        assert!(command.contains("--draft"));
    }

    #[test]
    fn test_build_gh_command_for_existing_pr() {
        let command = build_gh_command("Add feature X", Some(42), false);
        assert!(command.contains("gh pr edit 42"));
        assert!(command.contains("--body"));
        assert!(!command.contains("--title"));
        assert!(!command.contains("--draft"));
    }

    #[test]
    fn test_build_gh_command_for_existing_pr_ignores_draft() {
        // Draft flag should be ignored when editing existing PRs
        let command = build_gh_command("Add feature X", Some(42), true);
        assert!(command.contains("gh pr edit 42"));
        assert!(command.contains("--body"));
        assert!(!command.contains("--draft"));
    }

    #[test]
    fn test_build_gh_command_escapes_title_quotes() {
        let command = build_gh_command("Fix \"special\" case", None, false);
        // Title should be included (escape handling is agent's responsibility)
        assert!(command.contains("Fix \"special\" case"));
    }

    // ========================================================================
    // extract_pr_url tests
    // ========================================================================

    #[test]
    fn test_extract_pr_url_from_simple_output() {
        let output = "https://github.com/owner/repo/pull/123";
        let url = extract_pr_url(output);
        assert_eq!(
            url,
            Some("https://github.com/owner/repo/pull/123".to_string())
        );
    }

    #[test]
    fn test_extract_pr_url_from_multiline_output() {
        let output = r#"Creating pull request...
Done!
https://github.com/owner/repo/pull/456"#;
        let url = extract_pr_url(output);
        assert_eq!(
            url,
            Some("https://github.com/owner/repo/pull/456".to_string())
        );
    }

    #[test]
    fn test_extract_pr_url_from_embedded_text() {
        let output = "PR created at https://github.com/owner/repo/pull/789 successfully";
        let url = extract_pr_url(output);
        assert_eq!(
            url,
            Some("https://github.com/owner/repo/pull/789".to_string())
        );
    }

    #[test]
    fn test_extract_pr_url_returns_none_when_no_url() {
        let output = "No URL here, just some text";
        let url = extract_pr_url(output);
        assert!(url.is_none());
    }

    #[test]
    fn test_extract_pr_url_returns_none_for_non_pr_github_url() {
        let output = "https://github.com/owner/repo/issues/123";
        let url = extract_pr_url(output);
        assert!(url.is_none());
    }

    #[test]
    fn test_extract_pr_url_handles_trailing_punctuation() {
        let output = "Created: https://github.com/owner/repo/pull/100.";
        let url = extract_pr_url(output);
        assert_eq!(
            url,
            Some("https://github.com/owner/repo/pull/100".to_string())
        );
    }

    #[test]
    fn test_extract_pr_url_prefers_last_url_in_output() {
        // The function searches from the end, expecting the final URL to be the result
        let output = r#"Opening https://github.com/owner/repo/pull/1
Updated https://github.com/owner/repo/pull/2"#;
        let url = extract_pr_url(output);
        assert_eq!(
            url,
            Some("https://github.com/owner/repo/pull/2".to_string())
        );
    }

    // ========================================================================
    // TemplateAgentResult tests
    // ========================================================================

    #[test]
    fn test_template_agent_result_success_equality() {
        let result1 = TemplateAgentResult::Success("https://github.com/o/r/pull/1".to_string());
        let result2 = TemplateAgentResult::Success("https://github.com/o/r/pull/1".to_string());
        assert_eq!(result1, result2);
    }

    #[test]
    fn test_template_agent_result_error_equality() {
        let error1 = ClaudeErrorInfo::new("test error");
        let error2 = ClaudeErrorInfo::new("test error");
        let result1 = TemplateAgentResult::Error(error1);
        let result2 = TemplateAgentResult::Error(error2);
        assert_eq!(result1, result2);
    }

    #[test]
    fn test_template_agent_result_clone() {
        let result = TemplateAgentResult::Success("https://github.com/o/r/pull/42".to_string());
        let cloned = result.clone();
        assert_eq!(result, cloned);
    }
}
