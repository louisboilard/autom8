//! Code review and correction.
//!
//! Handles reviewing completed work and correcting issues.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use crate::error::{Autom8Error, Result};
use crate::prompts::{CORRECTOR_PROMPT, REVIEWER_PROMPT};
use crate::spec::Spec;

use super::stream::{extract_text_from_stream_line, extract_usage_from_result_line};
use super::types::{ClaudeErrorInfo, ClaudeUsage};

const REVIEW_FILE: &str = "autom8_review.md";

/// Result from running the reviewer.
#[derive(Debug, Clone)]
pub struct ReviewResult {
    pub outcome: ReviewOutcome,
    /// Token usage data from the Claude API response
    pub usage: Option<ClaudeUsage>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReviewOutcome {
    Pass,
    IssuesFound,
    Error(ClaudeErrorInfo),
}

/// Result from running the corrector.
#[derive(Debug, Clone)]
pub struct CorrectorResult {
    pub outcome: CorrectorOutcome,
    /// Token usage data from the Claude API response
    pub usage: Option<ClaudeUsage>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CorrectorOutcome {
    Complete,
    Error(ClaudeErrorInfo),
}

/// Run the reviewer agent to check completed work for quality issues.
pub fn run_reviewer<F>(
    spec: &Spec,
    iteration: u32,
    max_iterations: u32,
    mut on_output: F,
) -> Result<ReviewResult>
where
    F: FnMut(&str),
{
    let prompt = build_reviewer_prompt(spec, iteration, max_iterations);

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
    let mut usage: Option<ClaudeUsage> = None;

    for line in reader.lines() {
        let line = line.map_err(|e| Autom8Error::ClaudeError(format!("Read error: {}", e)))?;

        if let Some(text) = extract_text_from_stream_line(&line) {
            on_output(&text);
        }

        // Try to extract usage from result events
        if let Some(line_usage) = extract_usage_from_result_line(&line) {
            usage = Some(line_usage);
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
        return Ok(ReviewResult {
            outcome: ReviewOutcome::Error(error_info),
            usage,
        });
    }

    // Check if autom8_review.md exists and has content
    let review_path = Path::new(REVIEW_FILE);
    let outcome = if review_path.exists() {
        match std::fs::read_to_string(review_path) {
            Ok(content) if !content.trim().is_empty() => ReviewOutcome::IssuesFound,
            Ok(_) => ReviewOutcome::Pass,
            Err(e) => ReviewOutcome::Error(ClaudeErrorInfo::new(format!(
                "Failed to read review file: {}",
                e
            ))),
        }
    } else {
        ReviewOutcome::Pass
    };

    Ok(ReviewResult { outcome, usage })
}

/// Run the corrector agent to fix issues identified by the reviewer.
pub fn run_corrector<F>(spec: &Spec, iteration: u32, mut on_output: F) -> Result<CorrectorResult>
where
    F: FnMut(&str),
{
    let max_iterations = 3;
    let prompt = build_corrector_prompt(spec, iteration, max_iterations);

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
    let mut usage: Option<ClaudeUsage> = None;

    for line in reader.lines() {
        let line = line.map_err(|e| Autom8Error::ClaudeError(format!("Read error: {}", e)))?;

        if let Some(text) = extract_text_from_stream_line(&line) {
            on_output(&text);
        }

        // Try to extract usage from result events
        if let Some(line_usage) = extract_usage_from_result_line(&line) {
            usage = Some(line_usage);
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
        return Ok(CorrectorResult {
            outcome: CorrectorOutcome::Error(error_info),
            usage,
        });
    }

    Ok(CorrectorResult {
        outcome: CorrectorOutcome::Complete,
        usage,
    })
}

fn build_reviewer_prompt(spec: &Spec, iteration: u32, max_iterations: u32) -> String {
    let stories_context = spec
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

    REVIEWER_PROMPT
        .replace("{project}", &spec.project)
        .replace("{feature_description}", &spec.description)
        .replace("{stories_context}", &stories_context)
        .replace("{iteration}", &iteration.to_string())
        .replace("{max_iterations}", &max_iterations.to_string())
}

fn build_corrector_prompt(spec: &Spec, iteration: u32, max_iterations: u32) -> String {
    let stories_context = spec
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

    CORRECTOR_PROMPT
        .replace("{project}", &spec.project)
        .replace("{feature_description}", &spec.description)
        .replace("{stories_context}", &stories_context)
        .replace("{iteration}", &iteration.to_string())
        .replace("{max_iterations}", &max_iterations.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::UserStory;

    #[test]
    fn test_review_outcome_variants() {
        let pass = ReviewOutcome::Pass;
        let issues = ReviewOutcome::IssuesFound;
        let error = ReviewOutcome::Error(ClaudeErrorInfo::new("test error"));

        assert_eq!(pass, ReviewOutcome::Pass);
        assert_eq!(issues, ReviewOutcome::IssuesFound);
        assert_eq!(
            error,
            ReviewOutcome::Error(ClaudeErrorInfo::new("test error"))
        );
    }

    #[test]
    fn test_review_result_with_usage() {
        let usage = ClaudeUsage {
            input_tokens: 100,
            output_tokens: 50,
            ..Default::default()
        };
        let result = ReviewResult {
            outcome: ReviewOutcome::Pass,
            usage: Some(usage.clone()),
        };
        assert!(matches!(result.outcome, ReviewOutcome::Pass));
        assert!(result.usage.is_some());
        assert_eq!(result.usage.unwrap().input_tokens, 100);
    }

    #[test]
    fn test_review_result_without_usage() {
        let result = ReviewResult {
            outcome: ReviewOutcome::IssuesFound,
            usage: None,
        };
        assert!(matches!(result.outcome, ReviewOutcome::IssuesFound));
        assert!(result.usage.is_none());
    }

    #[test]
    fn test_corrector_outcome_variants() {
        let complete = CorrectorOutcome::Complete;
        let error = CorrectorOutcome::Error(ClaudeErrorInfo::new("test error"));

        assert_eq!(complete, CorrectorOutcome::Complete);
        assert_eq!(
            error,
            CorrectorOutcome::Error(ClaudeErrorInfo::new("test error"))
        );
    }

    #[test]
    fn test_corrector_result_with_usage() {
        let usage = ClaudeUsage {
            input_tokens: 200,
            output_tokens: 100,
            ..Default::default()
        };
        let result = CorrectorResult {
            outcome: CorrectorOutcome::Complete,
            usage: Some(usage.clone()),
        };
        assert!(matches!(result.outcome, CorrectorOutcome::Complete));
        assert!(result.usage.is_some());
        assert_eq!(result.usage.unwrap().input_tokens, 200);
    }

    #[test]
    fn test_corrector_result_without_usage() {
        let result = CorrectorResult {
            outcome: CorrectorOutcome::Complete,
            usage: None,
        };
        assert!(matches!(result.outcome, CorrectorOutcome::Complete));
        assert!(result.usage.is_none());
    }

    #[test]
    fn test_build_reviewer_prompt() {
        let spec = Spec {
            project: "TestProject".into(),
            branch_name: "test-branch".into(),
            description: "A test feature description".into(),
            user_stories: vec![UserStory {
                id: "US-001".into(),
                title: "First Story".into(),
                description: "First story description".into(),
                acceptance_criteria: vec!["Criterion A".into()],
                priority: 1,
                passes: true,
                notes: String::new(),
            }],
        };

        let prompt = build_reviewer_prompt(&spec, 1, 3);
        assert!(prompt.contains("TestProject"));
        assert!(prompt.contains("Review iteration 1/3"));
        assert!(prompt.contains("US-001"));
    }
}
