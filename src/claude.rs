use crate::error::{Autom8Error, Result};
use crate::git;
use crate::prd::{Prd, UserStory};
use crate::prompts::{COMMIT_PROMPT, CORRECTOR_PROMPT, PRD_JSON_PROMPT, REVIEWER_PROMPT};
use serde::Deserialize;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};

const COMPLETION_SIGNAL: &str = "<promise>COMPLETE</promise>";

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

#[derive(Debug, Clone, PartialEq)]
pub enum ClaudeResult {
    IterationComplete,
    AllStoriesComplete,
    Error(String),
}

pub fn run_claude<F>(
    prd: &Prd,
    story: &UserStory,
    prd_path: &std::path::Path,
    mut on_output: F,
) -> Result<ClaudeResult>
where
    F: FnMut(&str),
{
    let prompt = build_prompt(prd, story, prd_path);

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
        let error_msg = if stderr_content.is_empty() {
            format!("Claude exited with status: {}", status)
        } else {
            format!(
                "Claude exited with status {}: {}",
                status,
                stderr_content.trim()
            )
        };
        return Err(Autom8Error::ClaudeError(error_msg));
    }

    if found_complete {
        Ok(ClaudeResult::AllStoriesComplete)
    } else {
        Ok(ClaudeResult::IterationComplete)
    }
}

/// Run Claude to convert a prd.md spec into prd.json
pub fn run_for_prd_generation<F>(
    spec_content: &str,
    output_path: &Path,
    mut on_output: F,
) -> Result<Prd>
where
    F: FnMut(&str),
{
    let prompt = PRD_JSON_PROMPT.replace("{spec_content}", spec_content);

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
        let error_msg = if stderr_content.is_empty() {
            format!("Claude exited with status: {}", status)
        } else {
            format!(
                "Claude exited with status {}: {}",
                status,
                stderr_content.trim()
            )
        };
        return Err(Autom8Error::PrdGenerationFailed(error_msg));
    }

    // Try to get JSON either from response or from file if Claude wrote it directly
    let json_str = if let Some(json) = extract_json(&full_output) {
        json
    } else if output_path.exists() {
        // Claude may have written the file directly using tools
        std::fs::read_to_string(output_path).map_err(|e| {
            Autom8Error::InvalidGeneratedPrd(format!("Failed to read generated file: {}", e))
        })?
    } else {
        let preview = if full_output.len() > 200 {
            format!("{}...", &full_output[..200])
        } else {
            full_output.clone()
        };
        return Err(Autom8Error::InvalidGeneratedPrd(format!(
            "No valid JSON found in response. Response preview: {:?}",
            preview
        )));
    };

    // Parse the JSON into Prd
    let prd: Prd = serde_json::from_str(&json_str)
        .map_err(|e| Autom8Error::InvalidGeneratedPrd(format!("JSON parse error: {}", e)))?;

    // Save to output path (may overwrite if Claude already wrote it, but ensures consistent format)
    prd.save(output_path)?;

    Ok(prd)
}

#[derive(Debug, Clone, PartialEq)]
pub enum CommitResult {
    /// Commit succeeded, with short commit hash
    Success(String),
    NothingToCommit,
    Error(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReviewResult {
    Pass,
    IssuesFound,
    Error(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum CorrectorResult {
    Complete,
    Error(String),
}

/// Run Claude to commit changes after all stories are complete
pub fn run_for_commit<F>(prd: &Prd, mut on_output: F) -> Result<CommitResult>
where
    F: FnMut(&str),
{
    // Build stories summary for context
    let stories_summary = prd
        .user_stories
        .iter()
        .map(|s| format!("- {}: {}", s.id, s.title))
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = COMMIT_PROMPT
        .replace("{project}", &prd.project)
        .replace("{feature_description}", &prd.description)
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
        let error_msg = if stderr_content.is_empty() {
            format!("Claude exited with status: {}", status)
        } else {
            format!(
                "Claude exited with status {}: {}",
                status,
                stderr_content.trim()
            )
        };
        return Ok(CommitResult::Error(error_msg));
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
/// Returns ReviewResult::Error(String) on failure.
pub fn run_reviewer<F>(
    prd: &Prd,
    iteration: u32,
    max_iterations: u32,
    mut on_output: F,
) -> Result<ReviewResult>
where
    F: FnMut(&str),
{
    let prompt = build_reviewer_prompt(prd, iteration, max_iterations);

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
        let error_msg = if stderr_content.is_empty() {
            format!("Claude exited with status: {}", status)
        } else {
            format!(
                "Claude exited with status {}: {}",
                status,
                stderr_content.trim()
            )
        };
        return Ok(ReviewResult::Error(error_msg));
    }

    // Check if autom8_review.md exists and has content
    let review_path = Path::new(REVIEW_FILE);
    if review_path.exists() {
        match std::fs::read_to_string(review_path) {
            Ok(content) if !content.trim().is_empty() => Ok(ReviewResult::IssuesFound),
            Ok(_) => Ok(ReviewResult::Pass), // File exists but is empty
            Err(e) => Ok(ReviewResult::Error(format!(
                "Failed to read review file: {}",
                e
            ))),
        }
    } else {
        Ok(ReviewResult::Pass)
    }
}

/// Run the corrector agent to fix issues identified by the reviewer.
/// Returns CorrectorResult::Complete when Claude finishes successfully.
/// Returns CorrectorResult::Error(String) on failure.
pub fn run_corrector<F>(prd: &Prd, iteration: u32, mut on_output: F) -> Result<CorrectorResult>
where
    F: FnMut(&str),
{
    let max_iterations = 3;
    let prompt = build_corrector_prompt(prd, iteration, max_iterations);

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
        let error_msg = if stderr_content.is_empty() {
            format!("Claude exited with status: {}", status)
        } else {
            format!(
                "Claude exited with status {}: {}",
                status,
                stderr_content.trim()
            )
        };
        return Ok(CorrectorResult::Error(error_msg));
    }

    Ok(CorrectorResult::Complete)
}

/// Build the prompt for the corrector agent
fn build_corrector_prompt(prd: &Prd, iteration: u32, max_iterations: u32) -> String {
    // Build stories context - summary of all user stories
    let stories_context = prd
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
        .replace("{project}", &prd.project)
        .replace("{feature_description}", &prd.description)
        .replace("{stories_context}", &stories_context)
        .replace("{iteration}", &iteration.to_string())
        .replace("{max_iterations}", &max_iterations.to_string())
}

/// Build the prompt for the reviewer agent
fn build_reviewer_prompt(prd: &Prd, iteration: u32, max_iterations: u32) -> String {
    // Build stories context - summary of all user stories
    let stories_context = prd
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
        .replace("{project}", &prd.project)
        .replace("{feature_description}", &prd.description)
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

fn build_prompt(prd: &Prd, story: &UserStory, prd_path: &Path) -> String {
    let acceptance_criteria = story
        .acceptance_criteria
        .iter()
        .map(|c| format!("- {}", c))
        .collect::<Vec<_>>()
        .join("\n");

    let prd_path_str = prd_path.display();

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
4. After implementation, update `{prd_path}` to set `passes: true` for story {story_id}

## Completion

When ALL user stories in `{prd_path}` have `passes: true`, output exactly:
<promise>COMPLETE</promise>

This signals that the entire feature is done.

## Project Context

{prd_description}

## Notes
{notes}
"#,
        project = prd.project,
        story_id = story.id,
        story_title = story.title,
        story_description = story.description,
        acceptance_criteria = acceptance_criteria,
        prd_description = prd.description,
        prd_path = prd_path_str,
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

    #[test]
    fn test_build_prompt() {
        let prd = Prd {
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
        let prd_path = Path::new("/tmp/prd.json");

        let prompt = build_prompt(&prd, &story, prd_path);
        assert!(prompt.contains("TestProject"));
        assert!(prompt.contains("US-001"));
        assert!(prompt.contains("Criterion 1"));
        assert!(prompt.contains("/tmp/prd.json"));
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
        let prd = Prd {
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

        let prompt = build_reviewer_prompt(&prd, 1, 3);

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
        let prd = Prd {
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

        let prompt = build_reviewer_prompt(&prd, 2, 3);
        assert!(prompt.contains("Review iteration 2/3"));
    }

    #[test]
    fn test_review_result_variants() {
        // Test that all variants can be created
        let pass = ReviewResult::Pass;
        let issues = ReviewResult::IssuesFound;
        let error = ReviewResult::Error("test error".into());

        assert_eq!(pass, ReviewResult::Pass);
        assert_eq!(issues, ReviewResult::IssuesFound);
        assert_eq!(error, ReviewResult::Error("test error".into()));
    }

    #[test]
    fn test_review_result_clone() {
        let result = ReviewResult::Error("clone test".into());
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
        let error = CorrectorResult::Error("test error".into());

        assert_eq!(complete, CorrectorResult::Complete);
        assert_eq!(error, CorrectorResult::Error("test error".into()));
    }

    #[test]
    fn test_corrector_result_clone() {
        let result = CorrectorResult::Error("clone test".into());
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
        let prd = Prd {
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

        let prompt = build_corrector_prompt(&prd, 1, 3);

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
        let prd = Prd {
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

        let prompt = build_corrector_prompt(&prd, 2, 3);
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
}
