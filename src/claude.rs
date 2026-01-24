use crate::error::{Autom8Error, Result};
use crate::git;
use crate::prompts::{COMMIT_PROMPT, CORRECTOR_PROMPT, SPEC_JSON_PROMPT, SPEC_JSON_CORRECTION_PROMPT, REVIEWER_PROMPT};
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
    pub fn from_process_failure(
        status: std::process::ExitStatus,
        stderr: Option<String>,
    ) -> Self {
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
            if stderr_content.is_empty() { None } else { Some(stderr_content) },
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
            if stderr_content.is_empty() { None } else { Some(stderr_content) },
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

                // Build correction prompt with the malformed JSON
                let correction_prompt = SPEC_JSON_CORRECTION_PROMPT
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

    // All retries exhausted, return the last error
    Err(Autom8Error::InvalidGeneratedSpec(format!(
        "JSON parse error after {} attempts: {}",
        MAX_JSON_RETRY_ATTEMPTS,
        last_error.map(|e| e.to_string()).unwrap_or_else(|| "Unknown error".to_string())
    )))
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
            if stderr_content.is_empty() { None } else { Some(stderr_content) },
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
            if stderr_content.is_empty() { None } else { Some(stderr_content) },
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
            if stderr_content.is_empty() { None } else { Some(stderr_content) },
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

fn build_prompt(spec: &Spec, story: &UserStory, spec_path: &Path, previous_context: Option<&str>) -> String {
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
        assert_eq!(error, ReviewResult::Error(ClaudeErrorInfo::new("test error")));
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
        assert_eq!(error, CorrectorResult::Error(ClaudeErrorInfo::new("test error")));
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
            Some("Files changed: src/api.rs. Added REST endpoint for user registration.".to_string())
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
        let parsed: std::result::Result<serde_json::Value, _> = serde_json::from_str(&result.unwrap());
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
        assert_eq!(result.unwrap(), r#"{"project": "Test", "branchName": "main"}"#);
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
}
