//! Commit message generation.
//!
//! Handles running Claude to create semantic commit messages.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use crate::error::{Autom8Error, Result};
use crate::git;
use crate::prompts::COMMIT_PROMPT;
use crate::spec::Spec;

use super::permissions::build_permission_args;
use super::stream::{extract_text_from_stream_line, extract_usage_from_result_line};
use super::types::{ClaudeErrorInfo, ClaudeUsage};

/// Result from running Claude for commit.
#[derive(Debug, Clone)]
pub struct CommitResult {
    pub outcome: CommitOutcome,
    /// Token usage data from the Claude API response
    pub usage: Option<ClaudeUsage>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CommitOutcome {
    /// Commit succeeded, with short commit hash
    Success(String),
    NothingToCommit,
    Error(ClaudeErrorInfo),
}

/// Run Claude to commit changes after all stories are complete
pub fn run_for_commit<F>(spec: &Spec, mut on_output: F) -> Result<CommitResult>
where
    F: FnMut(&str),
{
    // Build stories summary for context
    let stories_summary = spec
        .user_stories
        .iter()
        .map(|s| format!("- {}: {}", s.id, s.title))
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = COMMIT_PROMPT
        .replace("{project}", &spec.project)
        .replace("{feature_description}", &spec.description)
        .replace("{stories_summary}", &stories_summary);

    // Get project directory for permission configuration
    let project_dir = std::env::current_dir()
        .map_err(|e| Autom8Error::ClaudeError(format!("Failed to get current dir: {}", e)))?;
    let permission_args = build_permission_args(&project_dir);

    let mut child = Command::new("claude")
        .args(&permission_args)
        .args(["--print", "--output-format", "stream-json", "--verbose"])
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

    // Stream stdout and check for "nothing to commit"
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Autom8Error::ClaudeError("Failed to capture stdout".into()))?;

    let reader = BufReader::new(stdout);
    let mut nothing_to_commit = false;
    let mut accumulated_text = String::new();
    let mut usage: Option<ClaudeUsage> = None;

    for line in reader.lines() {
        let line = line.map_err(|e| Autom8Error::ClaudeError(format!("Read error: {}", e)))?;

        // Parse stream-json output and extract text content
        if let Some(text) = extract_text_from_stream_line(&line) {
            on_output(&text);
            accumulated_text.push_str(&text);

            if text.to_lowercase().contains("nothing to commit")
                || accumulated_text
                    .to_lowercase()
                    .contains("nothing to commit")
            {
                nothing_to_commit = true;
            }
        }

        // Try to extract usage from result events
        if let Some(line_usage) = extract_usage_from_result_line(&line) {
            usage = Some(line_usage);
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
        return Ok(CommitResult {
            outcome: CommitOutcome::Error(error_info),
            usage,
        });
    }

    let outcome = if nothing_to_commit {
        CommitOutcome::NothingToCommit
    } else {
        // Get the short commit hash after successful commit
        let commit_hash = git::latest_commit_short().unwrap_or_else(|_| "unknown".to_string());
        CommitOutcome::Success(commit_hash)
    };

    Ok(CommitResult { outcome, usage })
}
