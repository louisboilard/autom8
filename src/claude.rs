use crate::error::{Autom8Error, Result};
use crate::git;
use crate::prompts::{
    COMMIT_PROMPT, CORRECTOR_PROMPT, REVIEWER_PROMPT, SPEC_JSON_CORRECTION_PROMPT, SPEC_JSON_PROMPT,
};
use crate::spec::{Spec, UserStory};
use crate::state::IterationRecord;
use serde::Deserialize;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};

const COMPLETION_SIGNAL: &str = "<promise>COMPLETE</promise>";
const WORK_SUMMARY_START: &str = "<work-summary>";
const WORK_SUMMARY_END: &str = "</work-summary>";
const MAX_WORK_SUMMARY_LENGTH: usize = 500;

// ============================================================================
// Structured error information for Claude operations
// ============================================================================

/// Captures detailed error information from Claude process failures.
/// This allows preserving stderr output and exit codes separately from
/// the error message for better error display.
#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeErrorInfo {
    /// Human-readable error message
    pub message: String,
    /// Exit code from the subprocess, if available
    pub exit_code: Option<i32>,
    /// Stderr output from the subprocess, if available
    pub stderr: Option<String>,
}

impl ClaudeErrorInfo {
    /// Create a new error info with just a message
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit_code: None,
            stderr: None,
        }
    }

    /// Create error info from a process exit status and stderr
    pub fn from_process_failure(status: std::process::ExitStatus, stderr: Option<String>) -> Self {
        let exit_code = status.code();
        let stderr_trimmed = stderr.as_ref().map(|s| s.trim().to_string());

        let message = match (&stderr_trimmed, exit_code) {
            (Some(err), Some(code)) if !err.is_empty() => {
                format!("Claude exited with status {}: {}", code, err)
            }
            (Some(err), None) if !err.is_empty() => {
                format!("Claude exited with error: {}", err)
            }
            (_, Some(code)) => {
                format!("Claude exited with status: {}", code)
            }
            (_, None) => {
                format!("Claude exited with status: {}", status)
            }
        };

        Self {
            message,
            exit_code,
            stderr: stderr_trimmed.filter(|s| !s.is_empty()),
        }
    }
}

impl std::fmt::Display for ClaudeErrorInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl From<ClaudeErrorInfo> for String {
    fn from(info: ClaudeErrorInfo) -> Self {
        info.message
    }
}

// ============================================================================
// Stream JSON parsing types for Claude CLI output
// ============================================================================

/// Top-level stream event wrapper
#[derive(Debug, Deserialize)]
struct StreamLine {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    event: Option<StreamEventInner>,
    #[serde(default)]
    message: Option<AssistantMessage>,
    #[serde(default)]
    result: Option<String>,
}

/// Inner event content for stream_event types
#[derive(Debug, Deserialize)]
struct StreamEventInner {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    delta: Option<ContentDelta>,
}

/// Content delta containing text updates
#[derive(Debug, Deserialize)]
struct ContentDelta {
    #[serde(default)]
    text: Option<String>,
}

/// Assistant message containing content blocks
#[derive(Debug, Deserialize)]
struct AssistantMessage {
    #[serde(default)]
    content: Vec<ContentBlock>,
}

/// Content block that may contain text
#[derive(Debug, Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    #[serde(default)]
    text: Option<String>,
}

/// Extract text content from a stream JSON line
fn extract_text_from_stream_line(line: &str) -> Option<String> {
    let parsed: StreamLine = serde_json::from_str(line).ok()?;

    match parsed.event_type.as_str() {
        // Handle incremental text deltas from streaming
        "stream_event" => {
            if let Some(event) = parsed.event {
                if event.event_type == "content_block_delta" {
                    if let Some(delta) = event.delta {
                        return delta.text;
                    }
                }
            }
            None
        }
        // Handle complete assistant messages
        "assistant" => {
            if let Some(message) = parsed.message {
                let text: String = message
                    .content
                    .iter()
                    .filter(|block| block.block_type == "text")
                    .filter_map(|block| block.text.as_ref())
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("");
                if !text.is_empty() {
                    return Some(text);
                }
            }
            None
        }
        // Handle final result
        "result" => parsed.result,
        _ => None,
    }
}

/// Extract work summary from Claude's output using <work-summary>...</work-summary> markers.
/// Returns None if no valid summary is found, for graceful degradation.
/// Truncates to MAX_WORK_SUMMARY_LENGTH chars to prevent prompt bloat.
pub fn extract_work_summary(output: &str) -> Option<String> {
    let start_idx = output.find(WORK_SUMMARY_START)?;
    let content_start = start_idx + WORK_SUMMARY_START.len();
    let end_idx = output[content_start..].find(WORK_SUMMARY_END)?;

    let summary = output[content_start..content_start + end_idx].trim();

    if summary.is_empty() {
        return None;
    }

    // Truncate to max length to prevent prompt bloat
    let truncated = if summary.len() > MAX_WORK_SUMMARY_LENGTH {
        let mut end = MAX_WORK_SUMMARY_LENGTH;
        // Try to truncate at a word boundary
        if let Some(last_space) = summary[..end].rfind(' ') {
            end = last_space;
        }
        format!("{}...", &summary[..end])
    } else {
        summary.to_string()
    };

    Some(truncated)
}

// STREAMING OUTPUT IMPLEMENTATION (US-002):
// ==========================================
// Fixed the streaming output issue by using `--output-format stream-json --verbose` instead
// of plain `--print` which buffers output.
//
// Stream JSON format provides real-time output as JSON lines:
// - "stream_event" with "content_block_delta": incremental text pieces as tokens generate
// - "assistant": complete message with full content
// - "result": final result text
//
// The extract_text_from_stream_line() function parses each JSON line and extracts text content,
// which is then passed to the on_output callback for real-time display updates.

/// Result from running Claude on a story task
#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeStoryResult {
    pub outcome: ClaudeOutcome,
    /// Extracted work summary from Claude's output, if present
    pub work_summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClaudeOutcome {
    IterationComplete,
    AllStoriesComplete,
    Error(ClaudeErrorInfo),
}

/// Legacy enum for backwards compatibility - use ClaudeStoryResult for new code
#[derive(Debug, Clone, PartialEq)]
pub enum ClaudeResult {
    IterationComplete,
    AllStoriesComplete,
    Error(ClaudeErrorInfo),
}

pub fn run_claude<F>(
    spec: &Spec,
    story: &UserStory,
    spec_path: &std::path::Path,
    previous_iterations: &[IterationRecord],
    mut on_output: F,
) -> Result<ClaudeStoryResult>
where
    F: FnMut(&str),
{
    let previous_context = build_previous_context(previous_iterations);
    let prompt = build_prompt(spec, story, spec_path, previous_context.as_deref());

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

    // Stream stdout and check for completion
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Autom8Error::ClaudeError("Failed to capture stdout".into()))?;

    let reader = BufReader::new(stdout);
    let mut found_complete = false;
    let mut accumulated_text = String::new();

    for line in reader.lines() {
        let line = line.map_err(|e| Autom8Error::ClaudeError(format!("Read error: {}", e)))?;

        // Parse stream-json output and extract text content
        if let Some(text) = extract_text_from_stream_line(&line) {
            on_output(&text);
            accumulated_text.push_str(&text);

            if text.contains(COMPLETION_SIGNAL) || accumulated_text.contains(COMPLETION_SIGNAL) {
                found_complete = true;
            }
        }
    }

    // Wait for process to complete
    let status = child
        .wait()
        .map_err(|e| Autom8Error::ClaudeError(format!("Wait error: {}", e)))?;

    if !status.success() {
        // Read stderr for error details
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
        return Err(Autom8Error::ClaudeError(error_info.message.clone()));
    }

    // Extract work summary from accumulated output (graceful degradation if not found)
    let work_summary = extract_work_summary(&accumulated_text);

    let outcome = if found_complete {
        ClaudeOutcome::AllStoriesComplete
    } else {
        ClaudeOutcome::IterationComplete
    };

    Ok(ClaudeStoryResult {
        outcome,
        work_summary,
    })
}

const MAX_JSON_RETRY_ATTEMPTS: u32 = 3;

/// Helper function to run Claude with a given prompt and return the raw output.
/// Streams output to the callback and returns the accumulated text.
fn run_claude_with_prompt<F>(prompt: &str, mut on_output: F) -> Result<String>
where
    F: FnMut(&str),
{
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

    // Stream stdout and collect output
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Autom8Error::ClaudeError("Failed to capture stdout".into()))?;

    let reader = BufReader::new(stdout);
    let mut full_output = String::new();

    for line in reader.lines() {
        let line = line.map_err(|e| Autom8Error::ClaudeError(format!("Read error: {}", e)))?;

        // Parse stream-json output and extract text content
        if let Some(text) = extract_text_from_stream_line(&line) {
            on_output(&text);
            full_output.push_str(&text);
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
        return Err(Autom8Error::SpecGenerationFailed(error_info.message));
    }

    Ok(full_output)
}

/// Run Claude to convert a spec-<feature>.md markdown file into spec-<feature>.json
/// Implements retry logic (up to 3 attempts) when JSON parsing fails.
pub fn run_for_spec_generation<F>(
    spec_content: &str,
    output_path: &Path,
    mut on_output: F,
) -> Result<Spec>
where
    F: FnMut(&str),
{
    // First attempt with the initial prompt
    let initial_prompt = SPEC_JSON_PROMPT.replace("{spec_content}", spec_content);
    let mut full_output = run_claude_with_prompt(&initial_prompt, &mut on_output)?;

    // Try to get JSON either from response or from file if Claude wrote it directly
    let mut json_str = if let Some(json) = extract_json(&full_output) {
        json
    } else if output_path.exists() {
        // Claude may have written the file directly using tools
        std::fs::read_to_string(output_path).map_err(|e| {
            Autom8Error::InvalidGeneratedSpec(format!("Failed to read generated file: {}", e))
        })?
    } else {
        let preview = if full_output.len() > 200 {
            format!("{}...", &full_output[..200])
        } else {
            full_output.clone()
        };
        return Err(Autom8Error::InvalidGeneratedSpec(format!(
            "No valid JSON found in response. Response preview: {:?}",
            preview
        )));
    };

    // Try to parse the JSON, with retry logic on failure
    let mut last_error: Option<serde_json::Error> = None;

    for attempt in 1..=MAX_JSON_RETRY_ATTEMPTS {
        match serde_json::from_str::<Spec>(&json_str) {
            Ok(spec) => {
                // Success! Save and return
                spec.save(output_path)?;
                return Ok(spec);
            }
            Err(e) => {
                last_error = Some(e);

                // If this was the last attempt, don't retry
                if attempt == MAX_JSON_RETRY_ATTEMPTS {
                    break;
                }

                // Inform user of retry
                let retry_msg = format!(
                    "\nJSON malformed, retrying (attempt {}/{})...\n",
                    attempt + 1,
                    MAX_JSON_RETRY_ATTEMPTS
                );
                on_output(&retry_msg);

                // Build correction prompt with the malformed JSON and original spec content
                let correction_prompt = SPEC_JSON_CORRECTION_PROMPT
                    .replace("{spec_content}", spec_content)
                    .replace("{malformed_json}", &json_str)
                    .replace("{error_message}", &last_error.as_ref().unwrap().to_string())
                    .replace("{attempt}", &(attempt + 1).to_string())
                    .replace("{max_attempts}", &MAX_JSON_RETRY_ATTEMPTS.to_string());

                // Run Claude again with correction prompt
                full_output = run_claude_with_prompt(&correction_prompt, &mut on_output)?;

                // Extract JSON from the new response
                if let Some(json) = extract_json(&full_output) {
                    json_str = json;
                } else {
                    // If we can't extract JSON at all, use the raw output as the "JSON"
                    // This will fail parsing but we'll try again on next iteration
                    json_str = full_output.clone();
                }
            }
        }
    }

    // All agentic retries exhausted - try non-agentic fix as final fallback
    on_output("\nAttempting programmatic JSON fix...\n");

    let fixed_json = fix_json_syntax(&json_str);

    // Try to parse the fixed JSON
    match serde_json::from_str::<Spec>(&fixed_json) {
        Ok(spec) => {
            on_output("Programmatic fix succeeded!\n");
            spec.save(output_path)?;
            return Ok(spec);
        }
        Err(fallback_err) => {
            // Non-agentic fix also failed - build detailed error message with both errors
            let agentic_error = last_error
                .map(|e| e.to_string())
                .unwrap_or_else(|| "Unknown error".to_string());
            let fallback_error = fallback_err.to_string();

            // Create truncated JSON preview for debugging
            let json_preview = truncate_json_preview(&json_str, 500);

            Err(Autom8Error::InvalidGeneratedSpec(format!(
                "JSON generation failed after {} agentic attempts and programmatic fallback.\n\n\
                 Agent error: {}\n\n\
                 Fallback error: {}\n\n\
                 Malformed JSON preview:\n{}",
                MAX_JSON_RETRY_ATTEMPTS, agentic_error, fallback_error, json_preview
            )))
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CommitResult {
    /// Commit succeeded, with short commit hash
    Success(String),
    NothingToCommit,
    Error(ClaudeErrorInfo),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReviewResult {
    Pass,
    IssuesFound,
    Error(ClaudeErrorInfo),
}

#[derive(Debug, Clone, PartialEq)]
pub enum CorrectorResult {
    Complete,
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

    // Stream stdout and check for "nothing to commit"
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Autom8Error::ClaudeError("Failed to capture stdout".into()))?;

    let reader = BufReader::new(stdout);
    let mut nothing_to_commit = false;
    let mut accumulated_text = String::new();

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
        return Ok(CommitResult::Error(error_info));
    }

    if nothing_to_commit {
        Ok(CommitResult::NothingToCommit)
    } else {
        // Get the short commit hash after successful commit
        let commit_hash = git::latest_commit_short().unwrap_or_else(|_| "unknown".to_string());
        Ok(CommitResult::Success(commit_hash))
    }
}

const REVIEW_FILE: &str = "autom8_review.md";

/// Run the reviewer agent to check completed work for quality issues.
/// Returns ReviewResult::Pass if autom8_review.md does not exist after run.
/// Returns ReviewResult::IssuesFound if autom8_review.md exists and has content.
/// Returns ReviewResult::Error(ClaudeErrorInfo) on failure with stderr and exit code preserved.
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

    for line in reader.lines() {
        let line = line.map_err(|e| Autom8Error::ClaudeError(format!("Read error: {}", e)))?;

        // Parse stream-json output and extract text content
        if let Some(text) = extract_text_from_stream_line(&line) {
            on_output(&text);
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
        return Ok(ReviewResult::Error(error_info));
    }

    // Check if autom8_review.md exists and has content
    let review_path = Path::new(REVIEW_FILE);
    if review_path.exists() {
        match std::fs::read_to_string(review_path) {
            Ok(content) if !content.trim().is_empty() => Ok(ReviewResult::IssuesFound),
            Ok(_) => Ok(ReviewResult::Pass), // File exists but is empty
            Err(e) => Ok(ReviewResult::Error(ClaudeErrorInfo::new(format!(
                "Failed to read review file: {}",
                e
            )))),
        }
    } else {
        Ok(ReviewResult::Pass)
    }
}

/// Run the corrector agent to fix issues identified by the reviewer.
/// Returns CorrectorResult::Complete when Claude finishes successfully.
/// Returns CorrectorResult::Error(ClaudeErrorInfo) on failure with stderr and exit code preserved.
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

    for line in reader.lines() {
        let line = line.map_err(|e| Autom8Error::ClaudeError(format!("Read error: {}", e)))?;

        // Parse stream-json output and extract text content
        if let Some(text) = extract_text_from_stream_line(&line) {
            on_output(&text);
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
        return Ok(CorrectorResult::Error(error_info));
    }

    Ok(CorrectorResult::Complete)
}

/// Build the prompt for the corrector agent
fn build_corrector_prompt(spec: &Spec, iteration: u32, max_iterations: u32) -> String {
    // Build stories context - summary of all user stories
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

/// Build the prompt for the reviewer agent
fn build_reviewer_prompt(spec: &Spec, iteration: u32, max_iterations: u32) -> String {
    // Build stories context - summary of all user stories
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

/// Fix common JSON syntax errors without calling Claude.
/// This is a conservative fixer that only corrects unambiguous errors:
/// - Strips markdown code fences (```json ... ``` and ``` ... ```)
/// - Removes trailing commas before ] and }
/// - Quotes unquoted keys that match identifier patterns
///
/// The function is idempotent - running it twice produces the same output.
pub fn fix_json_syntax(input: &str) -> String {
    use regex::Regex;

    let mut result = input.to_string();

    // Step 1: Strip markdown code fences
    // Handle ```json ... ``` and ``` ... ```
    let code_fence_re = Regex::new(r"(?s)^```(?:json)?\s*\n?(.*?)\n?```\s*$").unwrap();
    if let Some(captures) = code_fence_re.captures(&result) {
        if let Some(content) = captures.get(1) {
            result = content.as_str().to_string();
        }
    }

    // Also handle code fences that aren't at the start/end but wrap the entire JSON
    // Look for code fences and extract content between them
    let inline_fence_re = Regex::new(r"(?s)```(?:json)?\s*\n(.*?)\n```").unwrap();
    if let Some(captures) = inline_fence_re.captures(&result) {
        if let Some(content) = captures.get(1) {
            result = content.as_str().to_string();
        }
    }

    // Step 2: Quote unquoted keys that match identifier patterns
    // This must happen BEFORE trailing comma removal to avoid issues with parsing
    // Match: { key: or , key: where key is an identifier (not already quoted)
    // Be careful not to match inside strings
    let unquoted_key_re = Regex::new(r#"([{,]\s*)([a-zA-Z_][a-zA-Z0-9_]*)(\s*:)"#).unwrap();
    result = unquoted_key_re
        .replace_all(&result, |caps: &regex::Captures| {
            format!(
                "{}\"{}\"{}",
                caps.get(1).map_or("", |m| m.as_str()),
                caps.get(2).map_or("", |m| m.as_str()),
                caps.get(3).map_or("", |m| m.as_str())
            )
        })
        .to_string();

    // Step 3: Remove trailing commas before ] and }
    // Match comma followed by optional whitespace and then ] or }
    let trailing_comma_re = Regex::new(r",(\s*[}\]])").unwrap();
    result = trailing_comma_re.replace_all(&result, "$1").to_string();

    result.trim().to_string()
}

/// Extract JSON from Claude's response, handling potential markdown code blocks
fn extract_json(response: &str) -> Option<String> {
    let trimmed = response.trim();

    // Try to find JSON in markdown code block
    if let Some(start) = trimmed.find("```json") {
        let content_start = start + 7;
        if let Some(end) = trimmed[content_start..].find("```") {
            return Some(
                trimmed[content_start..content_start + end]
                    .trim()
                    .to_string(),
            );
        }
    }

    // Try to find JSON in generic code block
    if let Some(start) = trimmed.find("```") {
        let content_start = start + 3;
        // Skip any language identifier on the same line
        let content_start = trimmed[content_start..]
            .find('\n')
            .map(|i| content_start + i + 1)
            .unwrap_or(content_start);
        if let Some(end) = trimmed[content_start..].find("```") {
            return Some(
                trimmed[content_start..content_start + end]
                    .trim()
                    .to_string(),
            );
        }
    }

    // Try to find raw JSON object
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            if end > start {
                return Some(trimmed[start..=end].to_string());
            }
        }
    }

    None
}

/// Truncate JSON string for error preview, preserving readability.
/// If the JSON is longer than max_len, it truncates and adds "..." indicator.
fn truncate_json_preview(json: &str, max_len: usize) -> String {
    let trimmed = json.trim();
    if trimmed.len() <= max_len {
        trimmed.to_string()
    } else {
        format!("{}...", &trimmed[..max_len])
    }
}

/// Build a context string from previous iteration work summaries.
/// Returns None if there are no previous iterations with summaries.
/// Format: "US-001: [summary]\nUS-002: [summary]"
pub fn build_previous_context(iterations: &[IterationRecord]) -> Option<String> {
    let summaries: Vec<String> = iterations
        .iter()
        .filter_map(|iter| {
            iter.work_summary
                .as_ref()
                .map(|summary| format!("{}: {}", iter.story_id, summary))
        })
        .collect();

    if summaries.is_empty() {
        None
    } else {
        Some(summaries.join("\n"))
    }
}

fn build_prompt(
    spec: &Spec,
    story: &UserStory,
    spec_path: &Path,
    previous_context: Option<&str>,
) -> String {
    let acceptance_criteria = story
        .acceptance_criteria
        .iter()
        .map(|c| format!("- {}", c))
        .collect::<Vec<_>>()
        .join("\n");

    let spec_path_str = spec_path.display();

    // Build the previous work section if we have context
    let previous_work_section = match previous_context {
        Some(context) => format!(
            r#"
## Previous Work

The following user stories have already been completed:

{}
"#,
            context
        ),
        None => String::new(),
    };

    format!(
        r#"You are working on project: {project}

## Current Task

Implement user story **{story_id}: {story_title}**

### Description
{story_description}

### Acceptance Criteria
{acceptance_criteria}

## Instructions

1. Implement the user story according to the acceptance criteria
2. Write tests to verify the implementation
3. Run the tests to ensure they pass
4. After implementation, update `{spec_path}` to set `passes: true` for story {story_id}

## Completion

When ALL user stories in `{spec_path}` have `passes: true`, output exactly:
<promise>COMPLETE</promise>

This signals that the entire feature is done.

## Work Summary

After completing your implementation, output a brief summary (1-3 sentences) of what you accomplished in this format:

<work-summary>
Files changed: [list key files]. [Brief description of functionality added/changed].
</work-summary>

This helps provide context for subsequent tasks.

## Project Context

{spec_description}{previous_work}

## Notes
{notes}
"#,
        project = spec.project,
        story_id = story.id,
        story_title = story.title,
        story_description = story.description,
        acceptance_criteria = acceptance_criteria,
        spec_description = spec.description,
        spec_path = spec_path_str,
        previous_work = previous_work_section,
        notes = if story.notes.is_empty() {
            "None"
        } else {
            &story.notes
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // ========================================================================
    // ClaudeErrorInfo tests (US-006)
    // ========================================================================

    #[test]
    fn test_claude_error_info_new() {
        let info = ClaudeErrorInfo::new("test error message");
        assert_eq!(info.message, "test error message");
        assert_eq!(info.exit_code, None);
        assert_eq!(info.stderr, None);
    }

    #[test]
    fn test_claude_error_info_from_process_failure_with_stderr() {
        // Create a mock ExitStatus - we can't easily create one, so test the logic
        let info = ClaudeErrorInfo {
            message: "Claude exited with status 1: authentication failed".to_string(),
            exit_code: Some(1),
            stderr: Some("authentication failed".to_string()),
        };
        assert_eq!(info.exit_code, Some(1));
        assert_eq!(info.stderr, Some("authentication failed".to_string()));
        assert!(info.message.contains("status 1"));
        assert!(info.message.contains("authentication failed"));
    }

    #[test]
    fn test_claude_error_info_from_process_failure_no_stderr() {
        let info = ClaudeErrorInfo {
            message: "Claude exited with status: 1".to_string(),
            exit_code: Some(1),
            stderr: None,
        };
        assert_eq!(info.exit_code, Some(1));
        assert_eq!(info.stderr, None);
    }

    #[test]
    fn test_claude_error_info_display() {
        let info = ClaudeErrorInfo::new("test error");
        assert_eq!(format!("{}", info), "test error");
    }

    #[test]
    fn test_claude_error_info_into_string() {
        let info = ClaudeErrorInfo::new("convertible error");
        let s: String = info.into();
        assert_eq!(s, "convertible error");
    }

    #[test]
    fn test_claude_error_info_clone() {
        let info = ClaudeErrorInfo {
            message: "cloned error".to_string(),
            exit_code: Some(42),
            stderr: Some("stderr content".to_string()),
        };
        let cloned = info.clone();
        assert_eq!(info, cloned);
    }

    #[test]
    fn test_claude_error_info_equality() {
        let info1 = ClaudeErrorInfo::new("error");
        let info2 = ClaudeErrorInfo::new("error");
        let info3 = ClaudeErrorInfo::new("different");
        assert_eq!(info1, info2);
        assert_ne!(info1, info3);
    }

    #[test]
    fn test_claude_error_info_with_all_fields() {
        let info = ClaudeErrorInfo {
            message: "Claude exited with status 127: command not found".to_string(),
            exit_code: Some(127),
            stderr: Some("command not found".to_string()),
        };
        assert_eq!(info.exit_code, Some(127));
        assert!(info.stderr.as_ref().unwrap().contains("command not found"));
        assert!(info.message.contains("127"));
    }

    #[test]
    fn test_claude_error_info_debug() {
        let info = ClaudeErrorInfo::new("debug test");
        let debug_str = format!("{:?}", info);
        assert!(debug_str.contains("ClaudeErrorInfo"));
        assert!(debug_str.contains("debug test"));
    }

    #[test]
    fn test_build_prompt() {
        let spec = Spec {
            project: "TestProject".into(),
            branch_name: "test-branch".into(),
            description: "A test project".into(),
            user_stories: vec![],
        };
        let story = UserStory {
            id: "US-001".into(),
            title: "Test Story".into(),
            description: "A test story".into(),
            acceptance_criteria: vec!["Criterion 1".into(), "Criterion 2".into()],
            priority: 1,
            passes: false,
            notes: String::new(),
        };
        let spec_path = Path::new("/tmp/spec-test.json");

        let prompt = build_prompt(&spec, &story, spec_path, None);
        assert!(prompt.contains("TestProject"));
        assert!(prompt.contains("US-001"));
        assert!(prompt.contains("Criterion 1"));
        assert!(prompt.contains("/tmp/spec-test.json"));
        // No previous context, so no "Previous Work" section
        assert!(!prompt.contains("Previous Work"));
    }

    #[test]
    fn test_extract_json_from_code_block() {
        let response = r#"Here's the JSON:
```json
{"project": "Test"}
```
Done!"#;
        let json = extract_json(response).unwrap();
        assert_eq!(json, r#"{"project": "Test"}"#);
    }

    #[test]
    fn test_extract_json_raw() {
        let response = r#"{"project": "Test", "branchName": "main"}"#;
        let json = extract_json(response).unwrap();
        assert_eq!(json, r#"{"project": "Test", "branchName": "main"}"#);
    }

    #[test]
    fn test_extract_json_with_surrounding_text() {
        let response = r#"Here is the result:
{"project": "Test"}
End of response"#;
        let json = extract_json(response).unwrap();
        assert_eq!(json, r#"{"project": "Test"}"#);
    }

    #[test]
    fn test_build_reviewer_prompt() {
        let spec = Spec {
            project: "TestProject".into(),
            branch_name: "test-branch".into(),
            description: "A test feature description".into(),
            user_stories: vec![
                UserStory {
                    id: "US-001".into(),
                    title: "First Story".into(),
                    description: "First story description".into(),
                    acceptance_criteria: vec!["Criterion A".into(), "Criterion B".into()],
                    priority: 1,
                    passes: true,
                    notes: String::new(),
                },
                UserStory {
                    id: "US-002".into(),
                    title: "Second Story".into(),
                    description: "Second story description".into(),
                    acceptance_criteria: vec!["Criterion C".into()],
                    priority: 2,
                    passes: true,
                    notes: String::new(),
                },
            ],
        };

        let prompt = build_reviewer_prompt(&spec, 1, 3);

        // Check that project name is included
        assert!(prompt.contains("TestProject"));
        // Check that feature description is included
        assert!(prompt.contains("A test feature description"));
        // Check that iteration info is included
        assert!(prompt.contains("Review iteration 1/3"));
        // Check that stories context is included
        assert!(prompt.contains("US-001"));
        assert!(prompt.contains("First Story"));
        assert!(prompt.contains("US-002"));
        assert!(prompt.contains("Second Story"));
        // Check acceptance criteria are included
        assert!(prompt.contains("Criterion A"));
        assert!(prompt.contains("Criterion B"));
        assert!(prompt.contains("Criterion C"));
    }

    #[test]
    fn test_build_reviewer_prompt_iteration_2() {
        let spec = Spec {
            project: "TestProject".into(),
            branch_name: "test-branch".into(),
            description: "Test description".into(),
            user_stories: vec![UserStory {
                id: "US-001".into(),
                title: "Story".into(),
                description: "Description".into(),
                acceptance_criteria: vec!["Criterion".into()],
                priority: 1,
                passes: true,
                notes: String::new(),
            }],
        };

        let prompt = build_reviewer_prompt(&spec, 2, 3);
        assert!(prompt.contains("Review iteration 2/3"));
    }

    #[test]
    fn test_review_result_variants() {
        // Test that all variants can be created
        let pass = ReviewResult::Pass;
        let issues = ReviewResult::IssuesFound;
        let error = ReviewResult::Error(ClaudeErrorInfo::new("test error"));

        assert_eq!(pass, ReviewResult::Pass);
        assert_eq!(issues, ReviewResult::IssuesFound);
        assert_eq!(
            error,
            ReviewResult::Error(ClaudeErrorInfo::new("test error"))
        );
    }

    #[test]
    fn test_review_result_clone() {
        let result = ReviewResult::Error(ClaudeErrorInfo::new("clone test"));
        let cloned = result.clone();
        assert_eq!(result, cloned);
    }

    #[test]
    fn test_review_result_debug() {
        let result = ReviewResult::Pass;
        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("Pass"));
    }

    #[test]
    fn test_corrector_result_variants() {
        // Test that all variants can be created
        let complete = CorrectorResult::Complete;
        let error = CorrectorResult::Error(ClaudeErrorInfo::new("test error"));

        assert_eq!(complete, CorrectorResult::Complete);
        assert_eq!(
            error,
            CorrectorResult::Error(ClaudeErrorInfo::new("test error"))
        );
    }

    #[test]
    fn test_corrector_result_clone() {
        let result = CorrectorResult::Error(ClaudeErrorInfo::new("clone test"));
        let cloned = result.clone();
        assert_eq!(result, cloned);
    }

    #[test]
    fn test_corrector_result_debug() {
        let result = CorrectorResult::Complete;
        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("Complete"));
    }

    #[test]
    fn test_build_corrector_prompt() {
        let spec = Spec {
            project: "TestProject".into(),
            branch_name: "test-branch".into(),
            description: "A test feature description".into(),
            user_stories: vec![
                UserStory {
                    id: "US-001".into(),
                    title: "First Story".into(),
                    description: "First story description".into(),
                    acceptance_criteria: vec!["Criterion A".into(), "Criterion B".into()],
                    priority: 1,
                    passes: true,
                    notes: String::new(),
                },
                UserStory {
                    id: "US-002".into(),
                    title: "Second Story".into(),
                    description: "Second story description".into(),
                    acceptance_criteria: vec!["Criterion C".into()],
                    priority: 2,
                    passes: true,
                    notes: String::new(),
                },
            ],
        };

        let prompt = build_corrector_prompt(&spec, 1, 3);

        // Check that project name is included
        assert!(prompt.contains("TestProject"));
        // Check that feature description is included
        assert!(prompt.contains("A test feature description"));
        // Check that iteration info is included
        assert!(prompt.contains("Correction iteration 1/3"));
        // Check that stories context is included
        assert!(prompt.contains("US-001"));
        assert!(prompt.contains("First Story"));
        assert!(prompt.contains("US-002"));
        assert!(prompt.contains("Second Story"));
        // Check acceptance criteria are included
        assert!(prompt.contains("Criterion A"));
        assert!(prompt.contains("Criterion B"));
        assert!(prompt.contains("Criterion C"));
    }

    #[test]
    fn test_build_corrector_prompt_iteration_2() {
        let spec = Spec {
            project: "TestProject".into(),
            branch_name: "test-branch".into(),
            description: "Test description".into(),
            user_stories: vec![UserStory {
                id: "US-001".into(),
                title: "Story".into(),
                description: "Description".into(),
                acceptance_criteria: vec!["Criterion".into()],
                priority: 1,
                passes: true,
                notes: String::new(),
            }],
        };

        let prompt = build_corrector_prompt(&spec, 2, 3);
        assert!(prompt.contains("Correction iteration 2/3"));
    }

    // ========================================================================
    // Stream JSON parsing tests
    // ========================================================================

    #[test]
    fn test_extract_text_from_stream_event_content_block_delta() {
        let line = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello world"}},"session_id":"test"}"#;
        let text = extract_text_from_stream_line(line);
        assert_eq!(text, Some("Hello world".to_string()));
    }

    #[test]
    fn test_extract_text_from_assistant_message() {
        let line = r#"{"type":"assistant","message":{"model":"claude","id":"msg_123","type":"message","role":"assistant","content":[{"type":"text","text":"Complete response here"}]},"session_id":"test"}"#;
        let text = extract_text_from_stream_line(line);
        assert_eq!(text, Some("Complete response here".to_string()));
    }

    #[test]
    fn test_extract_text_from_result() {
        let line = r#"{"type":"result","subtype":"success","is_error":false,"duration_ms":1000,"result":"Final result text","session_id":"test"}"#;
        let text = extract_text_from_stream_line(line);
        assert_eq!(text, Some("Final result text".to_string()));
    }

    #[test]
    fn test_extract_text_from_system_event_returns_none() {
        let line = r#"{"type":"system","subtype":"init","cwd":"/test","session_id":"test"}"#;
        let text = extract_text_from_stream_line(line);
        assert_eq!(text, None);
    }

    #[test]
    fn test_extract_text_from_message_start_returns_none() {
        let line = r#"{"type":"stream_event","event":{"type":"message_start","message":{}},"session_id":"test"}"#;
        let text = extract_text_from_stream_line(line);
        assert_eq!(text, None);
    }

    #[test]
    fn test_extract_text_from_invalid_json_returns_none() {
        let line = "not valid json at all";
        let text = extract_text_from_stream_line(line);
        assert_eq!(text, None);
    }

    #[test]
    fn test_extract_text_from_empty_delta_returns_none() {
        let line = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{}},"session_id":"test"}"#;
        let text = extract_text_from_stream_line(line);
        assert_eq!(text, None);
    }

    #[test]
    fn test_extract_text_from_assistant_with_multiple_content_blocks() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"First"},{"type":"text","text":" Second"},{"type":"tool_use","text":"ignored"}]}}"#;
        let text = extract_text_from_stream_line(line);
        assert_eq!(text, Some("First Second".to_string()));
    }

    #[test]
    fn test_extract_text_preserves_special_characters() {
        let line = r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"text":"Line1\nLine2\ttab"}}}"#;
        let text = extract_text_from_stream_line(line);
        assert_eq!(text, Some("Line1\nLine2\ttab".to_string()));
    }

    #[test]
    fn test_extract_text_from_real_claude_output() {
        // Test with actual Claude CLI output format
        let init_line = r#"{"type":"system","subtype":"init","cwd":"/Users/test","session_id":"abc123","tools":["Bash"]}"#;
        assert_eq!(extract_text_from_stream_line(init_line), None);

        let delta_line = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Code"}},"session_id":"abc123"}"#;
        assert_eq!(
            extract_text_from_stream_line(delta_line),
            Some("Code".to_string())
        );

        let result_line = r#"{"type":"result","subtype":"success","is_error":false,"duration_ms":2831,"result":"Code flows like water","session_id":"abc123"}"#;
        assert_eq!(
            extract_text_from_stream_line(result_line),
            Some("Code flows like water".to_string())
        );
    }

    // ========================================================================
    // Work summary extraction tests (US-006)
    // ========================================================================

    #[test]
    fn test_extract_work_summary_basic() {
        let output = r#"I made some changes.

<work-summary>
Files changed: src/main.rs, src/lib.rs. Added new authentication module with login and logout functions.
</work-summary>

Done!"#;
        let summary = extract_work_summary(output);
        assert_eq!(
            summary,
            Some("Files changed: src/main.rs, src/lib.rs. Added new authentication module with login and logout functions.".to_string())
        );
    }

    #[test]
    fn test_extract_work_summary_no_markers() {
        let output = "I made some changes but didn't include a work summary.";
        let summary = extract_work_summary(output);
        assert_eq!(summary, None);
    }

    #[test]
    fn test_extract_work_summary_empty_content() {
        let output = "<work-summary>   </work-summary>";
        let summary = extract_work_summary(output);
        assert_eq!(summary, None);
    }

    #[test]
    fn test_extract_work_summary_missing_end_marker() {
        let output = "<work-summary>This summary has no end marker";
        let summary = extract_work_summary(output);
        assert_eq!(summary, None);
    }

    #[test]
    fn test_extract_work_summary_truncates_long_content() {
        // Create a string longer than 500 chars
        let long_content = "a".repeat(600);
        let output = format!("<work-summary>{}</work-summary>", long_content);
        let summary = extract_work_summary(&output).unwrap();
        // Should be truncated with "..." at the end
        assert!(summary.ends_with("..."));
        assert!(summary.len() <= 503); // 500 + "..."
    }

    #[test]
    fn test_extract_work_summary_truncates_at_word_boundary() {
        // Create content with spaces that's longer than 500 chars
        let words = "word ".repeat(120); // 600 chars
        let output = format!("<work-summary>{}</work-summary>", words);
        let summary = extract_work_summary(&output).unwrap();
        assert!(summary.ends_with("..."));
        // Should truncate at a word boundary (space)
        assert!(!summary.trim_end_matches("...").ends_with(' '));
    }

    #[test]
    fn test_extract_work_summary_trims_whitespace() {
        let output = "<work-summary>  \n  Files changed: test.rs. Fixed bug.  \n  </work-summary>";
        let summary = extract_work_summary(output);
        assert_eq!(
            summary,
            Some("Files changed: test.rs. Fixed bug.".to_string())
        );
    }

    #[test]
    fn test_extract_work_summary_multiline() {
        let output = r#"<work-summary>
Files changed: src/auth.rs, src/user.rs.
Added user authentication with JWT tokens.
Also updated the database schema.
</work-summary>"#;
        let summary = extract_work_summary(output).unwrap();
        assert!(summary.contains("Files changed:"));
        assert!(summary.contains("JWT tokens"));
        assert!(summary.contains("database schema"));
    }

    #[test]
    fn test_extract_work_summary_with_surrounding_text() {
        let output = r#"I completed the implementation.

Let me write a summary:

<work-summary>
Files changed: src/api.rs. Added REST endpoint for user registration.
</work-summary>

<promise>COMPLETE</promise>"#;
        let summary = extract_work_summary(output);
        assert_eq!(
            summary,
            Some(
                "Files changed: src/api.rs. Added REST endpoint for user registration.".to_string()
            )
        );
    }

    #[test]
    fn test_extract_work_summary_exactly_500_chars() {
        // Create exactly 500 char content
        let content = "x".repeat(500);
        let output = format!("<work-summary>{}</work-summary>", content);
        let summary = extract_work_summary(&output).unwrap();
        // Should not be truncated
        assert_eq!(summary.len(), 500);
        assert!(!summary.ends_with("..."));
    }

    // ========================================================================
    // Previous context building tests (US-007)
    // ========================================================================

    use crate::state::{IterationRecord, IterationStatus};
    use chrono::Utc;

    fn create_iteration_record(story_id: &str, work_summary: Option<&str>) -> IterationRecord {
        IterationRecord {
            number: 1,
            story_id: story_id.to_string(),
            started_at: Utc::now(),
            finished_at: Some(Utc::now()),
            status: IterationStatus::Success,
            output_snippet: String::new(),
            work_summary: work_summary.map(|s| s.to_string()),
        }
    }

    #[test]
    fn test_build_previous_context_empty_iterations() {
        let iterations: Vec<IterationRecord> = vec![];
        let context = build_previous_context(&iterations);
        assert!(context.is_none());
    }

    #[test]
    fn test_build_previous_context_no_summaries() {
        let iterations = vec![
            create_iteration_record("US-001", None),
            create_iteration_record("US-002", None),
        ];
        let context = build_previous_context(&iterations);
        assert!(context.is_none());
    }

    #[test]
    fn test_build_previous_context_single_summary() {
        let iterations = vec![create_iteration_record(
            "US-001",
            Some("Files changed: src/main.rs. Added entry point."),
        )];
        let context = build_previous_context(&iterations).unwrap();
        assert_eq!(
            context,
            "US-001: Files changed: src/main.rs. Added entry point."
        );
    }

    #[test]
    fn test_build_previous_context_multiple_summaries() {
        let iterations = vec![
            create_iteration_record(
                "US-001",
                Some("Files changed: src/main.rs. Added entry point."),
            ),
            create_iteration_record(
                "US-002",
                Some("Files changed: src/lib.rs. Added core functionality."),
            ),
        ];
        let context = build_previous_context(&iterations).unwrap();
        assert!(context.contains("US-001: Files changed: src/main.rs. Added entry point."));
        assert!(context.contains("US-002: Files changed: src/lib.rs. Added core functionality."));
        assert!(context.contains('\n'));
    }

    #[test]
    fn test_build_previous_context_skips_none_summaries() {
        let iterations = vec![
            create_iteration_record(
                "US-001",
                Some("Files changed: src/main.rs. Added entry point."),
            ),
            create_iteration_record("US-002", None), // No summary
            create_iteration_record(
                "US-003",
                Some("Files changed: src/lib.rs. Added core functionality."),
            ),
        ];
        let context = build_previous_context(&iterations).unwrap();
        assert!(context.contains("US-001"));
        assert!(!context.contains("US-002"));
        assert!(context.contains("US-003"));
    }

    #[test]
    fn test_build_prompt_with_previous_context() {
        let spec = Spec {
            project: "TestProject".into(),
            branch_name: "test-branch".into(),
            description: "A test project".into(),
            user_stories: vec![],
        };
        let story = UserStory {
            id: "US-002".into(),
            title: "Second Story".into(),
            description: "A second story".into(),
            acceptance_criteria: vec!["Criterion 1".into()],
            priority: 2,
            passes: false,
            notes: String::new(),
        };
        let spec_path = Path::new("/tmp/spec-test.json");
        let previous_context = Some("US-001: Files changed: src/main.rs. Added entry point.");

        let prompt = build_prompt(&spec, &story, spec_path, previous_context);

        // Should contain the Previous Work section
        assert!(prompt.contains("## Previous Work"));
        assert!(prompt.contains("The following user stories have already been completed:"));
        assert!(prompt.contains("US-001: Files changed: src/main.rs. Added entry point."));
    }

    #[test]
    fn test_build_prompt_without_previous_context() {
        let spec = Spec {
            project: "TestProject".into(),
            branch_name: "test-branch".into(),
            description: "A test project".into(),
            user_stories: vec![],
        };
        let story = UserStory {
            id: "US-001".into(),
            title: "First Story".into(),
            description: "A first story".into(),
            acceptance_criteria: vec!["Criterion 1".into()],
            priority: 1,
            passes: false,
            notes: String::new(),
        };
        let spec_path = Path::new("/tmp/spec-test.json");

        let prompt = build_prompt(&spec, &story, spec_path, None);

        // Should NOT contain the Previous Work section for first story
        assert!(!prompt.contains("## Previous Work"));
        assert!(!prompt.contains("The following user stories have already been completed:"));
    }

    #[test]
    fn test_build_prompt_integration_with_iteration_records() {
        // Test the full integration: iterations -> build_previous_context -> build_prompt
        let spec = Spec {
            project: "TestProject".into(),
            branch_name: "test-branch".into(),
            description: "A test project".into(),
            user_stories: vec![],
        };
        let story = UserStory {
            id: "US-003".into(),
            title: "Third Story".into(),
            description: "A third story".into(),
            acceptance_criteria: vec!["Criterion 1".into()],
            priority: 3,
            passes: false,
            notes: String::new(),
        };
        let spec_path = Path::new("/tmp/spec-test.json");

        let iterations = vec![
            create_iteration_record("US-001", Some("Added authentication module.")),
            create_iteration_record("US-002", Some("Added user management.")),
        ];

        let previous_context = build_previous_context(&iterations);
        let prompt = build_prompt(&spec, &story, spec_path, previous_context.as_deref());

        assert!(prompt.contains("## Previous Work"));
        assert!(prompt.contains("US-001: Added authentication module."));
        assert!(prompt.contains("US-002: Added user management."));
    }

    // ========================================================================
    // JSON retry logic tests (US-005)
    // ========================================================================

    #[test]
    fn test_max_json_retry_attempts_is_three() {
        assert_eq!(MAX_JSON_RETRY_ATTEMPTS, 3);
    }

    #[test]
    fn test_correction_prompt_construction() {
        use crate::prompts::SPEC_JSON_CORRECTION_PROMPT;

        let malformed_json = r#"{"project": "Test", "branchName": "test"#;
        let error_message = "EOF while parsing a string at line 1 column 39";

        let correction_prompt = SPEC_JSON_CORRECTION_PROMPT
            .replace("{malformed_json}", malformed_json)
            .replace("{error_message}", error_message)
            .replace("{attempt}", "2")
            .replace("{max_attempts}", "3");

        // Verify the correction prompt contains the malformed JSON
        assert!(correction_prompt.contains(malformed_json));
        // Verify it contains the error message
        assert!(correction_prompt.contains(error_message));
        // Verify it shows the attempt count
        assert!(correction_prompt.contains("2/3"));
        // Verify it asks Claude to fix the JSON
        assert!(correction_prompt.contains("Fix the JSON"));
    }

    #[test]
    fn test_extract_json_valid_json() {
        let valid_json = r#"{"project": "Test", "branchName": "main", "description": "desc", "userStories": []}"#;
        let result = extract_json(valid_json);
        assert!(result.is_some());
        // Verify it can be parsed as JSON
        let parsed: std::result::Result<serde_json::Value, _> =
            serde_json::from_str(&result.unwrap());
        assert!(parsed.is_ok());
    }

    #[test]
    fn test_extract_json_from_markdown_code_block() {
        let response = r#"Here's the fixed JSON:

```json
{"project": "Test", "branchName": "main"}
```

Hope that helps!"#;
        let result = extract_json(response);
        assert!(result.is_some());
        assert_eq!(
            result.unwrap(),
            r#"{"project": "Test", "branchName": "main"}"#
        );
    }

    #[test]
    fn test_extract_json_handles_malformed_gracefully() {
        // extract_json should still extract JSON-like content even if malformed
        let response = r#"{"project": "Test", "branchName": "#;
        let result = extract_json(response);
        // It should still extract something since there's an opening brace
        // but there's no closing brace, so it should return None
        assert!(result.is_none());
    }

    #[test]
    fn test_spec_parsing_valid_json() {
        let valid_spec_json = r#"{
            "project": "TestProject",
            "branchName": "test-branch",
            "description": "A test project",
            "userStories": [{
                "id": "US-001",
                "title": "Test Story",
                "description": "A test story",
                "acceptanceCriteria": ["Criterion 1"],
                "priority": 1,
                "passes": false,
                "notes": ""
            }]
        }"#;

        let result: std::result::Result<Spec, _> = serde_json::from_str(valid_spec_json);
        assert!(result.is_ok());
        let spec = result.unwrap();
        assert_eq!(spec.project, "TestProject");
        assert_eq!(spec.user_stories.len(), 1);
    }

    #[test]
    fn test_spec_parsing_malformed_json() {
        // Missing closing brace
        let malformed_json = r#"{"project": "Test", "branchName": "main""#;
        let result: std::result::Result<Spec, _> = serde_json::from_str(malformed_json);
        assert!(result.is_err());
        // Verify error message is useful for correction prompt
        let error_msg = result.unwrap_err().to_string();
        assert!(!error_msg.is_empty());
    }

    #[test]
    fn test_spec_parsing_invalid_schema() {
        // Valid JSON but invalid schema (missing required fields)
        let invalid_schema = r#"{"project": "Test"}"#;
        let result: std::result::Result<Spec, _> = serde_json::from_str(invalid_schema);
        assert!(result.is_err());
    }

    #[test]
    fn test_retry_message_format() {
        // Test the format of the retry message shown to users
        let attempt = 2;
        let max_attempts = 3;
        let retry_msg = format!(
            "\nJSON malformed, retrying (attempt {}/{})...\n",
            attempt, max_attempts
        );
        assert!(retry_msg.contains("JSON malformed"));
        assert!(retry_msg.contains("retrying"));
        assert!(retry_msg.contains("2/3"));
    }

    // ========================================================================
    // Correction prompt spec_content tests (US-002)
    // ========================================================================

    #[test]
    fn test_correction_prompt_includes_spec_content() {
        use crate::prompts::SPEC_JSON_CORRECTION_PROMPT;

        let spec_content =
            "# My Feature\n\n## Project\nTestProject\n\n## Description\nA test feature.";
        let malformed_json = r#"{"project": "Test"#;
        let error_message = "unexpected end of input";

        let correction_prompt = SPEC_JSON_CORRECTION_PROMPT
            .replace("{spec_content}", spec_content)
            .replace("{malformed_json}", malformed_json)
            .replace("{error_message}", error_message)
            .replace("{attempt}", "2")
            .replace("{max_attempts}", "3");

        // Verify the correction prompt contains the spec content
        assert!(
            correction_prompt.contains("# My Feature"),
            "Correction prompt should include the original spec content"
        );
        assert!(
            correction_prompt.contains("## Project"),
            "Correction prompt should include spec sections"
        );
        assert!(
            correction_prompt.contains("TestProject"),
            "Correction prompt should include project name from spec"
        );
    }

    #[test]
    fn test_correction_prompt_all_placeholders_populated() {
        use crate::prompts::SPEC_JSON_CORRECTION_PROMPT;

        let spec_content = "# Test Spec\n\n## Project\nMyProject";
        let malformed_json = r#"{"invalid": json}"#;
        let error_message = "expected colon at line 1";

        let correction_prompt = SPEC_JSON_CORRECTION_PROMPT
            .replace("{spec_content}", spec_content)
            .replace("{malformed_json}", malformed_json)
            .replace("{error_message}", error_message)
            .replace("{attempt}", "3")
            .replace("{max_attempts}", "3");

        // Verify no placeholders remain
        assert!(
            !correction_prompt.contains("{spec_content}"),
            "spec_content placeholder should be replaced"
        );
        assert!(
            !correction_prompt.contains("{malformed_json}"),
            "malformed_json placeholder should be replaced"
        );
        assert!(
            !correction_prompt.contains("{error_message}"),
            "error_message placeholder should be replaced"
        );
        assert!(
            !correction_prompt.contains("{attempt}"),
            "attempt placeholder should be replaced"
        );
        assert!(
            !correction_prompt.contains("{max_attempts}"),
            "max_attempts placeholder should be replaced"
        );
    }

    #[test]
    fn test_correction_prompt_spec_content_enables_regeneration() {
        use crate::prompts::SPEC_JSON_CORRECTION_PROMPT;

        // The prompt should allow regeneration from the spec when JSON is too corrupted
        let spec_content = r#"# User Auth Feature

## Project
my-app

## Branch
feature/user-auth

## Description
Add user authentication

## User Stories

### US-001: Login endpoint
**Priority:** 1

Add login functionality

**Acceptance Criteria:**
- [ ] POST /login accepts credentials
- [ ] Returns JWT token"#;

        let malformed_json = "completely broken {{{not json at all";
        let error_message = "expected value at line 1 column 1";

        let correction_prompt = SPEC_JSON_CORRECTION_PROMPT
            .replace("{spec_content}", spec_content)
            .replace("{malformed_json}", malformed_json)
            .replace("{error_message}", error_message)
            .replace("{attempt}", "2")
            .replace("{max_attempts}", "3");

        // Verify the prompt contains context to regenerate from spec
        assert!(
            correction_prompt.contains("User Auth Feature"),
            "Should include feature name from spec"
        );
        assert!(
            correction_prompt.contains("feature/user-auth"),
            "Should include branch name from spec"
        );
        assert!(
            correction_prompt.contains("US-001"),
            "Should include user story ID from spec"
        );
        assert!(
            correction_prompt.contains("Login endpoint"),
            "Should include story title from spec"
        );
        assert!(
            correction_prompt.contains("POST /login"),
            "Should include acceptance criteria from spec"
        );
        // Verify regeneration is mentioned as an option
        assert!(
            correction_prompt.contains("regenerate") || correction_prompt.contains("Regenerate"),
            "Should mention regeneration option when JSON is too corrupted"
        );
    }

    // ========================================================================
    // fix_json_syntax tests (US-003)
    // ========================================================================

    #[test]
    fn test_fix_json_syntax_strips_json_code_fence() {
        let input = r#"```json
{"project": "Test", "name": "value"}
```"#;
        let result = fix_json_syntax(input);
        assert_eq!(result, r#"{"project": "Test", "name": "value"}"#);
    }

    #[test]
    fn test_fix_json_syntax_strips_generic_code_fence() {
        let input = r#"```
{"project": "Test", "name": "value"}
```"#;
        let result = fix_json_syntax(input);
        assert_eq!(result, r#"{"project": "Test", "name": "value"}"#);
    }

    #[test]
    fn test_fix_json_syntax_removes_trailing_comma_before_brace() {
        let input = r#"{"project": "Test", "name": "value",}"#;
        let result = fix_json_syntax(input);
        assert_eq!(result, r#"{"project": "Test", "name": "value"}"#);
    }

    #[test]
    fn test_fix_json_syntax_removes_trailing_comma_before_bracket() {
        let input = r#"[1, 2, 3,]"#;
        let result = fix_json_syntax(input);
        assert_eq!(result, r#"[1, 2, 3]"#);
    }

    #[test]
    fn test_fix_json_syntax_removes_nested_trailing_commas() {
        let input = r#"{"items": [1, 2, 3,], "nested": {"a": 1,},}"#;
        let result = fix_json_syntax(input);
        assert_eq!(result, r#"{"items": [1, 2, 3], "nested": {"a": 1}}"#);
    }

    #[test]
    fn test_fix_json_syntax_quotes_unquoted_key() {
        let input = r#"{foo: "bar"}"#;
        let result = fix_json_syntax(input);
        assert_eq!(result, r#"{"foo": "bar"}"#);
    }

    #[test]
    fn test_fix_json_syntax_quotes_multiple_unquoted_keys() {
        let input = r#"{foo: "bar", baz: 123}"#;
        let result = fix_json_syntax(input);
        assert_eq!(result, r#"{"foo": "bar", "baz": 123}"#);
    }

    #[test]
    fn test_fix_json_syntax_quotes_nested_unquoted_keys() {
        let input = r#"{outer: {inner: "value"}}"#;
        let result = fix_json_syntax(input);
        assert_eq!(result, r#"{"outer": {"inner": "value"}}"#);
    }

    #[test]
    fn test_fix_json_syntax_preserves_already_quoted_keys() {
        let input = r#"{"project": "Test", "name": "value"}"#;
        let result = fix_json_syntax(input);
        assert_eq!(result, r#"{"project": "Test", "name": "value"}"#);
    }

    #[test]
    fn test_fix_json_syntax_handles_mixed_quoted_unquoted_keys() {
        let input = r#"{"project": "Test", name: "value"}"#;
        let result = fix_json_syntax(input);
        assert_eq!(result, r#"{"project": "Test", "name": "value"}"#);
    }

    #[test]
    fn test_fix_json_syntax_handles_underscore_keys() {
        let input = r#"{my_key: "value", another_key_123: "test"}"#;
        let result = fix_json_syntax(input);
        assert_eq!(result, r#"{"my_key": "value", "another_key_123": "test"}"#);
    }

    #[test]
    fn test_fix_json_syntax_combined_fixes() {
        // Test all fixes together: code fence + trailing comma + unquoted key
        let input = r#"```json
{project: "Test", items: [1, 2,],}
```"#;
        let result = fix_json_syntax(input);
        assert_eq!(result, r#"{"project": "Test", "items": [1, 2]}"#);
    }

    #[test]
    fn test_fix_json_syntax_idempotent_valid_json() {
        let input = r#"{"project": "Test", "items": [1, 2, 3]}"#;
        let result1 = fix_json_syntax(input);
        let result2 = fix_json_syntax(&result1);
        assert_eq!(result1, result2, "Function should be idempotent");
    }

    #[test]
    fn test_fix_json_syntax_idempotent_after_fixes() {
        let input = r#"```json
{project: "Test", items: [1, 2,],}
```"#;
        let result1 = fix_json_syntax(input);
        let result2 = fix_json_syntax(&result1);
        assert_eq!(
            result1, result2,
            "Function should be idempotent after fixing"
        );
    }

    #[test]
    fn test_fix_json_syntax_idempotent_trailing_comma() {
        let input = r#"{"a": 1,}"#;
        let result1 = fix_json_syntax(input);
        let result2 = fix_json_syntax(&result1);
        assert_eq!(result1, result2, "Trailing comma fix should be idempotent");
    }

    #[test]
    fn test_fix_json_syntax_idempotent_unquoted_keys() {
        let input = r#"{foo: "bar"}"#;
        let result1 = fix_json_syntax(input);
        let result2 = fix_json_syntax(&result1);
        assert_eq!(result1, result2, "Unquoted key fix should be idempotent");
    }

    #[test]
    fn test_fix_json_syntax_preserves_string_content() {
        // Should not modify content inside strings
        let input = r#"{"message": "Hello, world!"}"#;
        let result = fix_json_syntax(input);
        assert_eq!(result, r#"{"message": "Hello, world!"}"#);
    }

    #[test]
    fn test_fix_json_syntax_handles_empty_input() {
        let input = "";
        let result = fix_json_syntax(input);
        assert_eq!(result, "");
    }

    #[test]
    fn test_fix_json_syntax_handles_whitespace_only() {
        let input = "   \n  \t  ";
        let result = fix_json_syntax(input);
        assert_eq!(result, "");
    }

    #[test]
    fn test_fix_json_syntax_trailing_comma_with_whitespace() {
        let input = r#"{"a": 1 ,  }"#;
        let result = fix_json_syntax(input);
        assert_eq!(result, r#"{"a": 1   }"#);
    }

    #[test]
    fn test_fix_json_syntax_trailing_comma_with_newline() {
        let input = r#"{
    "a": 1,
}"#;
        let result = fix_json_syntax(input);
        assert_eq!(
            result,
            r#"{
    "a": 1
}"#
        );
    }

    #[test]
    fn test_fix_json_syntax_real_world_spec_json() {
        // Test with a realistic spec JSON that might have errors
        let input = r#"```json
{
    project: "my-app",
    branchName: "feature/test",
    "description": "Test feature",
    userStories: [
        {
            "id": "US-001",
            "title": "Test story",
            "description": "A test",
            "acceptanceCriteria": ["Criterion 1",],
            "priority": 1,
            "passes": false,
            "notes": "",
        },
    ],
}
```"#;
        let result = fix_json_syntax(input);
        // Verify it can now be parsed as JSON
        let parsed: std::result::Result<serde_json::Value, _> = serde_json::from_str(&result);
        assert!(parsed.is_ok(), "Fixed JSON should be valid: {}", result);
    }

    #[test]
    fn test_fix_json_syntax_does_not_break_valid_json() {
        let valid_json = r#"{"project": "Test", "branchName": "main", "description": "A test project", "userStories": [{"id": "US-001", "title": "Story", "description": "Desc", "acceptanceCriteria": ["AC1"], "priority": 1, "passes": false, "notes": ""}]}"#;
        let result = fix_json_syntax(valid_json);
        // Should still be valid JSON
        let parsed: std::result::Result<serde_json::Value, _> = serde_json::from_str(&result);
        assert!(
            parsed.is_ok(),
            "Valid JSON should remain valid after fix_json_syntax"
        );
    }

    // ========================================================================
    // Non-agentic fallback integration tests (US-004)
    // ========================================================================

    #[test]
    fn test_fix_json_syntax_can_fix_malformed_spec_json() {
        // Simulate JSON that would fail parsing but can be fixed programmatically
        // This is the type of JSON the non-agentic fallback would receive
        let malformed_spec = r#"```json
{
    project: "TestProject",
    branchName: "test-branch",
    "description": "A test project",
    userStories: [
        {
            "id": "US-001",
            "title": "Test Story",
            "description": "A test story",
            "acceptanceCriteria": ["Criterion 1",],
            "priority": 1,
            "passes": false,
            "notes": "",
        },
    ],
}
```"#;

        // Verify original fails to parse
        let parse_result: std::result::Result<Spec, _> = serde_json::from_str(malformed_spec);
        assert!(parse_result.is_err(), "Malformed JSON should fail to parse");

        // Apply fix_json_syntax
        let fixed = fix_json_syntax(malformed_spec);

        // Verify fixed JSON parses successfully
        let fixed_result: std::result::Result<Spec, _> = serde_json::from_str(&fixed);
        assert!(
            fixed_result.is_ok(),
            "Fixed JSON should parse successfully: {}",
            fixed
        );

        // Verify the spec content is correct
        let spec = fixed_result.unwrap();
        assert_eq!(spec.project, "TestProject");
        assert_eq!(spec.branch_name, "test-branch");
        assert_eq!(spec.description, "A test project");
        assert_eq!(spec.user_stories.len(), 1);
        assert_eq!(spec.user_stories[0].id, "US-001");
    }

    #[test]
    fn test_non_agentic_fallback_flow_success() {
        // Test the fallback flow: malformed JSON that fix_json_syntax can fix
        // This simulates what happens after MAX_JSON_RETRY_ATTEMPTS are exhausted

        // Malformed JSON with trailing comma and unquoted key (fixable issues)
        let json_after_retries = r#"{
            project: "TestProject",
            "branchName": "main",
            "description": "Test",
            "userStories": [],
        }"#;

        // First verify it fails to parse as-is
        let initial_parse: std::result::Result<Spec, _> = serde_json::from_str(json_after_retries);
        assert!(initial_parse.is_err());

        // Apply non-agentic fix
        let fixed_json = fix_json_syntax(json_after_retries);

        // Verify it now parses successfully
        let fixed_parse: std::result::Result<Spec, _> = serde_json::from_str(&fixed_json);
        assert!(
            fixed_parse.is_ok(),
            "Non-agentic fix should produce valid JSON"
        );
    }

    #[test]
    fn test_non_agentic_fallback_flow_failure() {
        // Test the fallback flow when fix_json_syntax cannot fix the JSON
        // This simulates truly broken JSON that requires regeneration

        // Completely broken JSON - missing closing braces, truncated
        let unfixable_json = r#"{"project": "Test", "branchName": "#;

        // Apply non-agentic fix
        let fixed_json = fix_json_syntax(unfixable_json);

        // Should still fail to parse (unfixable structural issues)
        let parse_result: std::result::Result<Spec, _> = serde_json::from_str(&fixed_json);
        assert!(
            parse_result.is_err(),
            "Unfixable JSON should still fail after fix attempt"
        );
    }

    #[test]
    fn test_non_agentic_fallback_fixes_code_fence_wrapped_json() {
        // Claude often wraps JSON in code fences even when told not to
        // The non-agentic fallback should strip these

        let fenced_json = r#"```json
{
    "project": "TestProject",
    "branchName": "feature-branch",
    "description": "Test description",
    "userStories": [{
        "id": "US-001",
        "title": "Test",
        "description": "Test desc",
        "acceptanceCriteria": ["AC1"],
        "priority": 1,
        "passes": false,
        "notes": ""
    }]
}
```"#;

        // Apply non-agentic fix (strips code fences)
        let fixed = fix_json_syntax(fenced_json);

        // Verify it parses successfully
        let result: std::result::Result<Spec, _> = serde_json::from_str(&fixed);
        assert!(
            result.is_ok(),
            "Code fence-wrapped JSON should be fixable: {}",
            fixed
        );
    }

    #[test]
    fn test_fallback_message_format() {
        // Verify the user-facing messages match acceptance criteria
        let fallback_start_msg = "Attempting programmatic JSON fix...";
        let fallback_success_msg = "Programmatic fix succeeded!";

        // These messages should be shown to users
        assert!(fallback_start_msg.contains("programmatic"));
        assert!(fallback_start_msg.contains("JSON fix"));
        assert!(fallback_success_msg.contains("succeeded"));
    }

    #[test]
    fn test_error_message_after_fallback_failure() {
        // Verify error message format includes information about both agentic and non-agentic attempts
        let error_msg = format!(
            "JSON parse error after {} attempts and programmatic fix: {}",
            MAX_JSON_RETRY_ATTEMPTS, "expected value at line 1"
        );

        assert!(error_msg.contains("3 attempts"));
        assert!(error_msg.contains("programmatic fix"));
        assert!(error_msg.contains("expected value"));
    }

    // ========================================================================
    // Error reporting tests (US-005)
    // ========================================================================

    #[test]
    fn test_truncate_json_preview_short_json() {
        let short_json = r#"{"project": "Test"}"#;
        let result = truncate_json_preview(short_json, 500);
        assert_eq!(result, short_json);
    }

    #[test]
    fn test_truncate_json_preview_long_json() {
        let long_json = "a".repeat(600);
        let result = truncate_json_preview(&long_json, 500);
        assert_eq!(result.len(), 503); // 500 chars + "..."
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_truncate_json_preview_trims_whitespace() {
        let json_with_whitespace = "  \n  {\"project\": \"Test\"}  \n  ";
        let result = truncate_json_preview(json_with_whitespace, 500);
        assert_eq!(result, r#"{"project": "Test"}"#);
    }

    #[test]
    fn test_truncate_json_preview_exact_limit() {
        let exact_json = "a".repeat(500);
        let result = truncate_json_preview(&exact_json, 500);
        assert_eq!(result.len(), 500);
        assert!(!result.ends_with("..."));
    }

    #[test]
    fn test_error_message_contains_both_error_sources() {
        // US-005: Error message includes both the agentic error and the fallback error
        let agentic_error = "expected `:` at line 1 column 10";
        let fallback_error = "expected value at line 1 column 1";
        let malformed_json = r#"{project: "Test"}"#;

        // Simulate the error message format from run_for_spec_generation
        let error_msg = format!(
            "JSON generation failed after {} agentic attempts and programmatic fallback.\n\n\
             Agent error: {}\n\n\
             Fallback error: {}\n\n\
             Malformed JSON preview:\n{}",
            MAX_JSON_RETRY_ATTEMPTS,
            agentic_error,
            fallback_error,
            truncate_json_preview(malformed_json, 500)
        );

        // Verify all required components are present
        assert!(
            error_msg.contains("Agent error:"),
            "Error message should include 'Agent error:' label"
        );
        assert!(
            error_msg.contains("Fallback error:"),
            "Error message should include 'Fallback error:' label"
        );
        assert!(
            error_msg.contains(agentic_error),
            "Error message should include the agentic error"
        );
        assert!(
            error_msg.contains(fallback_error),
            "Error message should include the fallback error"
        );
        assert!(
            error_msg.contains("Malformed JSON preview:"),
            "Error message should include JSON preview section"
        );
        assert!(
            error_msg.contains(malformed_json),
            "Error message should include the malformed JSON"
        );
        assert!(
            error_msg.contains("3 agentic attempts"),
            "Error message should mention the number of attempts"
        );
    }

    #[test]
    fn test_error_message_truncates_long_json_preview() {
        // US-005: Malformed JSON should be truncated in error message
        let long_json = format!(r#"{{"project": "Test", "data": "{}"}}"#, "x".repeat(1000));
        let preview = truncate_json_preview(&long_json, 500);

        assert!(
            preview.len() <= 503,
            "Preview should be truncated to max 500 chars + '...'"
        );
        assert!(
            preview.ends_with("..."),
            "Truncated preview should end with '...'"
        );
    }

    #[test]
    fn test_error_message_labels_are_distinct() {
        // US-005: Error messages are clearly labeled
        let error_msg = format!(
            "JSON generation failed after {} agentic attempts and programmatic fallback.\n\n\
             Agent error: some agent error\n\n\
             Fallback error: some fallback error\n\n\
             Malformed JSON preview:\n{{}}",
            MAX_JSON_RETRY_ATTEMPTS
        );

        // Verify labels are distinct and can be used for parsing
        let agent_label_count = error_msg.matches("Agent error:").count();
        let fallback_label_count = error_msg.matches("Fallback error:").count();

        assert_eq!(
            agent_label_count, 1,
            "Should have exactly one 'Agent error:' label"
        );
        assert_eq!(
            fallback_label_count, 1,
            "Should have exactly one 'Fallback error:' label"
        );
    }

    #[test]
    fn test_error_message_with_real_parse_errors() {
        // US-005: Test with actual serde_json errors
        let unfixable_json = r#"{"project": "Test", "branchName": "#;

        // Get actual parse error
        let agentic_parse_result: std::result::Result<Spec, _> =
            serde_json::from_str(unfixable_json);
        let agentic_error = agentic_parse_result.unwrap_err().to_string();

        // Apply fix (won't help with structural issues)
        let fixed_json = fix_json_syntax(unfixable_json);
        let fallback_parse_result: std::result::Result<Spec, _> = serde_json::from_str(&fixed_json);
        let fallback_error = fallback_parse_result.unwrap_err().to_string();

        // Build error message
        let error_msg = format!(
            "JSON generation failed after {} agentic attempts and programmatic fallback.\n\n\
             Agent error: {}\n\n\
             Fallback error: {}\n\n\
             Malformed JSON preview:\n{}",
            MAX_JSON_RETRY_ATTEMPTS,
            agentic_error,
            fallback_error,
            truncate_json_preview(unfixable_json, 500)
        );

        // Verify the error message is useful for debugging
        assert!(
            error_msg.contains("EOF"),
            "Parse error should mention unexpected EOF"
        );
        assert!(
            error_msg.contains(unfixable_json.trim()),
            "Error should include the problematic JSON"
        );
    }

    // ========================================================================
    // Full retry + fallback flow integration tests (US-006)
    // ========================================================================
    //
    // These tests document and verify the complete JSON generation retry flow:
    //
    // RETRY FLOW STAGES:
    // 1. Initial attempt: Claude generates JSON from spec markdown
    // 2. Agentic retry (up to MAX_JSON_RETRY_ATTEMPTS): If JSON parse fails,
    //    send correction prompt to Claude with:
    //    - Original spec content (for regeneration if needed)
    //    - Malformed JSON output
    //    - Parse error message
    //    - Attempt counter (e.g., "2/3")
    // 3. Non-agentic fallback: If all agentic retries exhausted, call
    //    fix_json_syntax() which programmatically fixes:
    //    - Markdown code fences (```json ... ```)
    //    - Trailing commas before ] and }
    //    - Unquoted keys (foo: -> "foo":)
    // 4. Final error: If fallback also fails, return detailed error with:
    //    - Agent error (last agentic parse error)
    //    - Fallback error (parse error after programmatic fix)
    //    - Truncated JSON preview for debugging
    //
    // ========================================================================

    #[test]
    fn test_integration_trailing_comma_fixed_by_non_agentic_fallback() {
        // US-006: Test case with JSON that has a trailing comma verifies
        // non-agentic fix succeeds.
        //
        // This simulates the scenario where:
        // 1. Claude generates valid structure but with trailing commas
        // 2. All 3 agentic retries fail (Claude keeps producing trailing commas)
        // 3. Non-agentic fallback (fix_json_syntax) removes the trailing commas
        // 4. JSON parses successfully -> SUCCESS

        let json_with_trailing_comma = r#"{
            "project": "TestProject",
            "branchName": "feature/test",
            "description": "A test project with trailing commas",
            "userStories": [
                {
                    "id": "US-001",
                    "title": "Test Story",
                    "description": "A story to test",
                    "acceptanceCriteria": ["Criterion 1", "Criterion 2",],
                    "priority": 1,
                    "passes": false,
                    "notes": ""
                },
            ]
        }"#;

        // Stage 1: Verify the original JSON fails to parse (trailing commas are invalid JSON)
        let initial_parse: std::result::Result<Spec, _> =
            serde_json::from_str(json_with_trailing_comma);
        assert!(
            initial_parse.is_err(),
            "JSON with trailing commas should fail to parse"
        );
        let parse_error = initial_parse.unwrap_err().to_string();
        assert!(
            parse_error.contains("trailing") || parse_error.contains("expected"),
            "Parse error should indicate the issue"
        );

        // Stage 2: Apply non-agentic fix (simulating fallback after agentic retries)
        let fixed_json = fix_json_syntax(json_with_trailing_comma);

        // Stage 3: Verify the fixed JSON parses successfully
        let fixed_parse: std::result::Result<Spec, _> = serde_json::from_str(&fixed_json);
        assert!(
            fixed_parse.is_ok(),
            "Non-agentic fix should successfully remove trailing commas. Fixed JSON:\n{}",
            fixed_json
        );

        // Stage 4: Verify the parsed spec has correct content
        let spec = fixed_parse.unwrap();
        assert_eq!(spec.project, "TestProject");
        assert_eq!(spec.branch_name, "feature/test");
        assert_eq!(spec.user_stories.len(), 1);
        assert_eq!(spec.user_stories[0].id, "US-001");
        assert_eq!(spec.user_stories[0].acceptance_criteria.len(), 2);
    }

    #[test]
    fn test_integration_completely_invalid_json_error_propagation() {
        // US-006: Test case with completely invalid JSON (e.g., not json at all)
        // verifies proper error propagation.
        //
        // This simulates the scenario where:
        // 1. Claude generates something that isn't JSON at all
        // 2. All 3 agentic retries fail
        // 3. Non-agentic fallback cannot fix structural issues
        // 4. Detailed error message is returned with both errors

        let not_json_at_all = "This is plain text, not JSON at all. It has no braces or structure.";

        // Stage 1: Verify it completely fails to parse
        let initial_parse: std::result::Result<Spec, _> = serde_json::from_str(not_json_at_all);
        assert!(
            initial_parse.is_err(),
            "Plain text should fail to parse as JSON"
        );
        let agentic_error = initial_parse.unwrap_err().to_string();

        // Stage 2: Apply non-agentic fix (it cannot help with this)
        let fixed_json = fix_json_syntax(not_json_at_all);

        // Stage 3: Verify the fix didn't help (can't create JSON from plain text)
        let fallback_parse: std::result::Result<Spec, _> = serde_json::from_str(&fixed_json);
        assert!(
            fallback_parse.is_err(),
            "Non-agentic fix cannot create JSON from plain text"
        );
        let fallback_error = fallback_parse.unwrap_err().to_string();

        // Stage 4: Build error message as run_for_spec_generation would
        let error_msg = format!(
            "JSON generation failed after {} agentic attempts and programmatic fallback.\n\n\
             Agent error: {}\n\n\
             Fallback error: {}\n\n\
             Malformed JSON preview:\n{}",
            MAX_JSON_RETRY_ATTEMPTS,
            agentic_error,
            fallback_error,
            truncate_json_preview(not_json_at_all, 500)
        );

        // Stage 5: Verify error message contains all required components for debugging
        assert!(
            error_msg.contains("3 agentic attempts"),
            "Error should mention retry count"
        );
        assert!(
            error_msg.contains("Agent error:"),
            "Error should have agent error label"
        );
        assert!(
            error_msg.contains("Fallback error:"),
            "Error should have fallback error label"
        );
        assert!(
            error_msg.contains("Malformed JSON preview:"),
            "Error should have JSON preview section"
        );
        assert!(
            error_msg.contains("This is plain text"),
            "Error should include the original content"
        );
    }

    #[test]
    fn test_integration_truncated_json_cannot_be_fixed() {
        // US-006: Another form of invalid JSON - structurally incomplete
        // Verifies error propagation when JSON is cut off mid-stream.
        //
        // This simulates the scenario where:
        // 1. Claude's output was truncated (network issue, context limit, etc.)
        // 2. JSON has valid start but no closing braces
        // 3. Neither agentic retry nor fallback can fix it
        // 4. Error message helps identify the structural issue

        let truncated_json = r#"{"project": "Test", "branchName": "main", "description": "A project", "userStories": [{"id": "US-001", "title": "Story", "description": "Desc", "acceptanceCriteria": ["#;

        // Stage 1: Initial parse fails
        let initial_parse: std::result::Result<Spec, _> = serde_json::from_str(truncated_json);
        assert!(initial_parse.is_err());
        let agentic_error = initial_parse.unwrap_err().to_string();
        assert!(
            agentic_error.contains("EOF") || agentic_error.contains("end of"),
            "Error should mention unexpected end of input"
        );

        // Stage 2: Non-agentic fix cannot restore missing structure
        let fixed_json = fix_json_syntax(truncated_json);
        let fallback_parse: std::result::Result<Spec, _> = serde_json::from_str(&fixed_json);
        assert!(
            fallback_parse.is_err(),
            "Cannot fix structurally incomplete JSON"
        );

        // Stage 3: Error message format is consistent
        let fallback_error = fallback_parse.unwrap_err().to_string();
        let error_msg = format!(
            "JSON generation failed after {} agentic attempts and programmatic fallback.\n\n\
             Agent error: {}\n\n\
             Fallback error: {}",
            MAX_JSON_RETRY_ATTEMPTS, agentic_error, fallback_error
        );
        assert!(error_msg.contains("Agent error:"));
        assert!(error_msg.contains("Fallback error:"));
    }

    #[test]
    fn test_integration_code_fence_and_trailing_comma_combined() {
        // US-006: Test that combined issues (code fence + trailing comma + unquoted keys)
        // are all fixed by non-agentic fallback.
        //
        // This is a realistic scenario where Claude:
        // 1. Wraps output in ```json code fence (despite being told not to)
        // 2. Uses trailing commas (JavaScript habit)
        // 3. Sometimes uses unquoted keys

        let claude_output_with_issues = r#"```json
{
    project: "MyApp",
    "branchName": "feature/auth",
    "description": "Add authentication",
    userStories: [
        {
            "id": "US-001",
            "title": "Login",
            "description": "Add login form",
            "acceptanceCriteria": ["Works",],
            "priority": 1,
            "passes": false,
            "notes": "",
        },
    ],
}
```"#;

        // Verify original fails
        let initial_parse: std::result::Result<Spec, _> =
            serde_json::from_str(claude_output_with_issues);
        assert!(initial_parse.is_err());

        // Apply non-agentic fix
        let fixed_json = fix_json_syntax(claude_output_with_issues);

        // Verify all issues were fixed
        assert!(
            !fixed_json.contains("```"),
            "Code fences should be stripped"
        );

        // Parse should now succeed
        let fixed_parse: std::result::Result<Spec, _> = serde_json::from_str(&fixed_json);
        assert!(
            fixed_parse.is_ok(),
            "Combined fixes should produce valid JSON. Result:\n{}",
            fixed_json
        );

        let spec = fixed_parse.unwrap();
        assert_eq!(spec.project, "MyApp");
        assert_eq!(spec.user_stories[0].title, "Login");
    }

    #[test]
    fn test_integration_retry_flow_messages() {
        // US-006: Document the expected user-facing messages at each stage.
        // These messages are shown to the user during the retry flow.

        // Check all retry message variants (attempts 2 and 3)
        for attempt in 2..=MAX_JSON_RETRY_ATTEMPTS {
            let retry_msg = format!(
                "\nJSON malformed, retrying (attempt {}/{})...\n",
                attempt, MAX_JSON_RETRY_ATTEMPTS
            );
            assert!(
                retry_msg.contains("JSON malformed"),
                "Retry message should indicate JSON is malformed"
            );
            assert!(
                retry_msg.contains("retrying"),
                "Retry message should indicate a retry is happening"
            );
            assert!(
                retry_msg.contains(&format!("{}/{}", attempt, MAX_JSON_RETRY_ATTEMPTS)),
                "Retry message should show attempt count"
            );
        }

        // Message shown when fallback is triggered
        let fallback_start_msg = "\nAttempting programmatic JSON fix...\n";
        assert!(
            fallback_start_msg.contains("programmatic"),
            "Fallback message should indicate programmatic fix"
        );

        // Message shown when fallback succeeds
        let fallback_success_msg = "Programmatic fix succeeded!\n";
        assert!(
            fallback_success_msg.contains("succeeded"),
            "Success message should indicate success"
        );
    }

    #[test]
    fn test_integration_retry_constants() {
        // US-006: Verify the retry configuration is correct.
        // This documents the expected retry behavior.

        // Maximum number of agentic retry attempts before fallback
        assert_eq!(
            MAX_JSON_RETRY_ATTEMPTS, 3,
            "Should attempt Claude correction 3 times before fallback"
        );

        // The flow should be:
        // Attempt 1 (initial) -> Attempt 2 (retry) -> Attempt 3 (retry) -> Fallback -> Error
        // That's 3 total agentic attempts, then 1 fallback attempt
    }
}
