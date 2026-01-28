//! PR review analysis.
//!
//! Analyzes PR comments and fixes real issues while ignoring red herrings.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use crate::error::{Autom8Error, Result};
use crate::gh::{BranchContext, PRContext};
use crate::prompts::PR_REVIEW_PROMPT;

use super::stream::extract_text_from_stream_line;
use super::types::ClaudeErrorInfo;

/// Summary of the PR review analysis
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PRReviewSummary {
    /// Total number of comments analyzed
    pub total_comments: usize,
    /// Number of real issues that were fixed
    pub real_issues_fixed: usize,
    /// Number of red herrings identified
    pub red_herrings: usize,
    /// Number of legitimate suggestions (no action taken)
    pub legitimate_suggestions: usize,
}

impl PRReviewSummary {
    /// Parse summary from Claude's output text.
    pub fn parse_from_output(output: &str) -> Self {
        let mut summary = PRReviewSummary::default();

        // Look for summary section
        if let Some(summary_start) = output.find("## Summary") {
            let summary_text = &output[summary_start..];

            summary.total_comments = parse_summary_number(summary_text, "total comments analyzed");
            summary.real_issues_fixed = parse_summary_number(summary_text, "real issues fixed");
            summary.red_herrings = parse_summary_number(summary_text, "red herrings identified");
            summary.legitimate_suggestions =
                parse_summary_number(summary_text, "legitimate suggestions");
        }

        summary
    }
}

/// Parse a number from summary text matching a pattern like "**Label:** X"
fn parse_summary_number(text: &str, label: &str) -> usize {
    let label_lower = label.to_lowercase();
    for line in text.lines() {
        let line_lower = line.to_lowercase();
        if line_lower.contains(&label_lower) {
            for word in line.split_whitespace() {
                if let Ok(num) = word
                    .trim_matches(|c: char| !c.is_ascii_digit())
                    .parse::<usize>()
                {
                    return num;
                }
            }
        }
    }
    0
}

/// Result from running the PR review agent
#[derive(Debug, Clone, PartialEq)]
pub enum PRReviewResult {
    /// Review completed successfully with summary of findings
    Complete(PRReviewSummary),
    /// Review completed but no fixes were needed
    NoFixesNeeded(PRReviewSummary),
    /// Error occurred during review
    Error(ClaudeErrorInfo),
}

/// Run the PR review agent to analyze PR comments and fix real issues.
pub fn run_pr_review<F>(
    pr_context: &PRContext,
    branch_context: &BranchContext,
    mut on_output: F,
) -> Result<PRReviewResult>
where
    F: FnMut(&str),
{
    let prompt = build_pr_review_prompt(pr_context, branch_context);

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

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(prompt.as_bytes())
            .map_err(|e| Autom8Error::ClaudeError(format!("Failed to write to stdin: {}", e)))?;
    }

    let stderr = child.stderr.take();

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Autom8Error::ClaudeError("Failed to capture stdout".into()))?;

    let reader = BufReader::new(stdout);
    let mut accumulated_text = String::new();

    for line in reader.lines() {
        let line = line.map_err(|e| Autom8Error::ClaudeError(format!("Read error: {}", e)))?;

        if let Some(text) = extract_text_from_stream_line(&line) {
            on_output(&text);
            accumulated_text.push_str(&text);
        }
    }

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
        return Ok(PRReviewResult::Error(error_info));
    }

    let summary = PRReviewSummary::parse_from_output(&accumulated_text);

    if summary.real_issues_fixed > 0 {
        Ok(PRReviewResult::Complete(summary))
    } else {
        Ok(PRReviewResult::NoFixesNeeded(summary))
    }
}

fn build_pr_review_prompt(pr_context: &PRContext, branch_context: &BranchContext) -> String {
    // Build spec context section
    let spec_context = match &branch_context.spec {
        Some(spec) => {
            let stories = spec
                .user_stories
                .iter()
                .map(|s| {
                    let criteria = s
                        .acceptance_criteria
                        .iter()
                        .map(|c| format!("  - {}", c))
                        .collect::<Vec<_>>()
                        .join("\n");
                    format!(
                        "### {}: {}\n{}\n\n**Acceptance Criteria:**\n{}",
                        s.id, s.title, s.description, criteria
                    )
                })
                .collect::<Vec<_>>()
                .join("\n\n");

            format!(
                "### Spec: {}\n\n**Description:**\n{}\n\n**User Stories:**\n\n{}",
                spec.project, spec.description, stories
            )
        }
        None => format!(
            "*No spec file found for branch `{}`*\n\nThe review will proceed with reduced context.",
            branch_context.branch_name
        ),
    };

    // Build commit history section
    let commit_history = if branch_context.commits.is_empty() {
        "No commits found specific to this branch.".to_string()
    } else {
        branch_context
            .commits
            .iter()
            .map(|c| format!("{} - {} ({})", c.short_hash, c.message, c.author))
            .collect::<Vec<_>>()
            .join("\n")
    };

    // Build unresolved comments section
    let unresolved_comments = pr_context
        .unresolved_comments
        .iter()
        .enumerate()
        .map(|(i, comment)| {
            let location = match (&comment.file_path, comment.line) {
                (Some(path), Some(line)) => format!("{}:{}", path, line),
                (Some(path), None) => path.clone(),
                _ => "PR conversation".to_string(),
            };

            format!(
                "### Comment {} from @{} ({})\n\n> {}\n",
                i + 1,
                comment.author,
                location,
                comment.body.lines().collect::<Vec<_>>().join("\n> ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    PR_REVIEW_PROMPT
        .replace("{spec_context}", &spec_context)
        .replace("{pr_description}", &pr_context.body)
        .replace("{commit_history}", &commit_history)
        .replace("{unresolved_comments}", &unresolved_comments)
}
