//! Main Claude runner for story implementation.
//!
//! Handles running Claude to implement individual user stories.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};

use crate::error::{Autom8Error, Result};
use crate::knowledge::ProjectKnowledge;
use crate::spec::{Spec, UserStory};
use crate::state::IterationRecord;

use super::control::{ControlRequest, ControlResponse, PermissionResult};
use super::permissions::{build_permission_args, ClaudePhase};
use super::stream::{extract_text_from_stream_line, extract_usage_from_result_line};
use super::types::{ClaudeErrorInfo, ClaudeOutcome, ClaudeStoryResult, ClaudeUsage};
use super::utils::{build_knowledge_context, build_previous_context, extract_work_summary};

const COMPLETION_SIGNAL: &str = "<promise>COMPLETE</promise>";

/// Manages a running Claude subprocess, allowing it to be killed for cleanup.
///
/// The `ClaudeRunner` stores the child process handle in a thread-safe manner,
/// allowing the `kill()` method to be called from a signal handler while the
/// main thread is reading output.
#[derive(Clone)]
pub struct ClaudeRunner {
    child: Arc<Mutex<Option<Child>>>,
}

impl ClaudeRunner {
    /// Creates a new `ClaudeRunner` with no active subprocess.
    pub fn new() -> Self {
        Self {
            child: Arc::new(Mutex::new(None)),
        }
    }

    /// Kills the subprocess if it is running.
    ///
    /// This method:
    /// - Terminates the subprocess using SIGKILL
    /// - Closes stdin/stdout/stderr handles by dropping the Child
    /// - Is safe to call multiple times or when no subprocess is running
    ///
    /// Returns `Ok(true)` if a process was killed, `Ok(false)` if no process was running.
    pub fn kill(&self) -> Result<bool> {
        let mut child_guard = self.child.lock().map_err(|e| {
            Autom8Error::ClaudeError(format!("Failed to acquire lock for kill: {}", e))
        })?;

        if let Some(mut child) = child_guard.take() {
            // Kill the process
            if let Err(e) = child.kill() {
                // Process may have already exited - not an error
                if e.kind() != std::io::ErrorKind::InvalidInput {
                    return Err(Autom8Error::ClaudeError(format!(
                        "Failed to kill Claude subprocess: {}",
                        e
                    )));
                }
            }
            // Wait for the process to fully terminate to avoid zombie
            let _ = child.wait();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Returns true if the runner currently has an active subprocess.
    pub fn is_running(&self) -> bool {
        self.child
            .lock()
            .map(|guard| guard.is_some())
            .unwrap_or(false)
    }

    /// Stores a child process handle in the runner.
    fn set_child(&self, child: Child) -> Result<()> {
        let mut child_guard = self.child.lock().map_err(|e| {
            Autom8Error::ClaudeError(format!("Failed to acquire lock for set_child: {}", e))
        })?;
        *child_guard = Some(child);
        Ok(())
    }

    /// Takes the child process out of the runner, returning it.
    /// Used when the process completes normally.
    fn take_child(&self) -> Result<Option<Child>> {
        let mut child_guard = self.child.lock().map_err(|e| {
            Autom8Error::ClaudeError(format!("Failed to acquire lock for take_child: {}", e))
        })?;
        Ok(child_guard.take())
    }
}

impl Default for ClaudeRunner {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse a line for control_request events.
///
/// Returns Some(ControlRequest) if the line contains a valid control request.
fn parse_control_request(line: &str) -> Option<ControlRequest> {
    let request: ControlRequest = serde_json::from_str(line).ok()?;
    if request.is_tool_use_request() {
        Some(request)
    } else {
        None
    }
}

/// Send a control response to stdin.
fn send_control_response(stdin: &mut ChildStdin, response: &ControlResponse) -> Result<()> {
    let json = serde_json::to_string(response)
        .map_err(|e| Autom8Error::ClaudeError(format!("Failed to serialize response: {}", e)))?;
    writeln!(stdin, "{}", json)
        .map_err(|e| Autom8Error::ClaudeError(format!("Failed to write response: {}", e)))?;
    stdin
        .flush()
        .map_err(|e| Autom8Error::ClaudeError(format!("Failed to flush stdin: {}", e)))?;
    Ok(())
}

/// Build the JSON user message format for stream-json input.
///
/// When using `--input-format stream-json`, prompts must be sent as JSON messages.
fn build_json_user_message(prompt: &str) -> String {
    serde_json::json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": prompt
        }
    })
    .to_string()
}

impl ClaudeRunner {
    /// Runs Claude to implement a user story.
    ///
    /// This method stores the child process handle internally, allowing
    /// `kill()` to be called from another thread (e.g., a signal handler)
    /// to terminate the subprocess.
    ///
    /// # Arguments
    ///
    /// * `spec` - The spec containing project information
    /// * `story` - The user story to implement
    /// * `spec_path` - Path to the spec JSON file
    /// * `previous_iterations` - Previous iteration records for context
    /// * `knowledge` - Project knowledge for context
    /// * `all_permissions` - If true, use --dangerously-skip-permissions instead of phase-aware permissions
    /// * `on_output` - Callback for streaming output
    /// * `on_permission` - Callback for handling permission requests (tool_name, tool_input) -> PermissionResult
    #[allow(clippy::too_many_arguments)]
    pub fn run<F, P>(
        &self,
        spec: &Spec,
        story: &UserStory,
        spec_path: &Path,
        previous_iterations: &[IterationRecord],
        knowledge: &ProjectKnowledge,
        all_permissions: bool,
        mut on_output: F,
        mut on_permission: P,
    ) -> Result<ClaudeStoryResult>
    where
        F: FnMut(&str),
        P: FnMut(&str, &serde_json::Value) -> PermissionResult,
    {
        let previous_context = build_previous_context(previous_iterations);
        let knowledge_context = build_knowledge_context(knowledge);
        let prompt = build_prompt(
            spec,
            story,
            spec_path,
            knowledge_context.as_deref(),
            previous_context.as_deref(),
        );

        let permission_args =
            build_permission_args(ClaudePhase::StoryImplementation, all_permissions);
        let mut args: Vec<&str> = permission_args;
        // Add bidirectional communication flags for permission prompts
        args.extend([
            "--print",
            "--output-format",
            "stream-json",
            "--verbose",
            "--input-format",
            "stream-json",
            "--permission-prompt-tool",
            "stdio",
        ]);

        let mut child = Command::new("claude")
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Autom8Error::ClaudeError(format!("Failed to spawn claude: {}", e)))?;

        // Take stdin handle - keep it open for permission responses
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| Autom8Error::ClaudeError("Failed to capture stdin".into()))?;

        // Send prompt as JSON user message format
        let json_message = build_json_user_message(&prompt);
        writeln!(stdin, "{}", json_message)
            .map_err(|e| Autom8Error::ClaudeError(format!("Failed to write to stdin: {}", e)))?;
        stdin
            .flush()
            .map_err(|e| Autom8Error::ClaudeError(format!("Failed to flush stdin: {}", e)))?;

        // Take stderr handle before storing child
        let stderr = child.stderr.take();

        // Take stdout handle before storing child
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Autom8Error::ClaudeError("Failed to capture stdout".into()))?;

        // Store the child so kill() can access it
        self.set_child(child)?;

        // Stream stdout and check for completion
        let reader = BufReader::new(stdout);
        let mut found_complete = false;
        let mut accumulated_text = String::new();
        let mut usage: Option<ClaudeUsage> = None;

        for line in reader.lines() {
            let line = line.map_err(|e| Autom8Error::ClaudeError(format!("Read error: {}", e)))?;

            // Check for control_request events (permission prompts)
            if let Some(control_request) = parse_control_request(&line) {
                // Invoke the on_permission callback
                let result = on_permission(
                    &control_request.request.tool_name,
                    &control_request.request.input,
                );

                // If permission was denied, error out immediately
                // (Claude Code may stall waiting after a deny, so we abort the run)
                if let PermissionResult::Deny(ref reason) = result {
                    return Err(Autom8Error::PermissionDenied {
                        tool_name: control_request.request.tool_name.clone(),
                        reason: reason.clone(),
                    });
                }

                // Send the allow response back to Claude
                let response = ControlResponse::from_result(&control_request.request_id, result);
                send_control_response(&mut stdin, &response)?;
                continue;
            }

            // Parse stream-json output and extract text content
            if let Some(text) = extract_text_from_stream_line(&line) {
                on_output(&text);
                accumulated_text.push_str(&text);

                if text.contains(COMPLETION_SIGNAL) || accumulated_text.contains(COMPLETION_SIGNAL)
                {
                    found_complete = true;
                }
            }

            // Try to extract usage from result events
            if let Some(line_usage) = extract_usage_from_result_line(&line) {
                usage = Some(line_usage);
            }
        }

        // Drop stdin to signal end of input
        drop(stdin);

        // Take the child back to wait for completion
        let child = self.take_child()?;

        if let Some(mut child) = child {
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
        }

        // Extract work summary from accumulated output
        let work_summary = extract_work_summary(&accumulated_text);

        let outcome = if found_complete {
            ClaudeOutcome::AllStoriesComplete
        } else {
            ClaudeOutcome::IterationComplete
        };

        Ok(ClaudeStoryResult {
            outcome,
            work_summary,
            full_output: accumulated_text,
            usage,
        })
    }
}

/// Convenience function that creates a `ClaudeRunner` and runs Claude.
///
/// This maintains backwards compatibility with existing code that uses the
/// standalone function. For new code that needs to kill the subprocess
/// (e.g., on signal handling), use `ClaudeRunner` directly.
///
/// # Arguments
///
/// * `spec` - The spec containing project information
/// * `story` - The user story to implement
/// * `spec_path` - Path to the spec JSON file
/// * `previous_iterations` - Previous iteration records for context
/// * `knowledge` - Project knowledge for context
/// * `all_permissions` - If true, use --dangerously-skip-permissions instead of phase-aware permissions
/// * `on_output` - Callback for streaming output
/// * `on_permission` - Callback for handling permission requests (tool_name, tool_input) -> PermissionResult
#[allow(clippy::too_many_arguments)]
pub fn run_claude<F, P>(
    spec: &Spec,
    story: &UserStory,
    spec_path: &Path,
    previous_iterations: &[IterationRecord],
    knowledge: &ProjectKnowledge,
    all_permissions: bool,
    on_output: F,
    on_permission: P,
) -> Result<ClaudeStoryResult>
where
    F: FnMut(&str),
    P: FnMut(&str, &serde_json::Value) -> PermissionResult,
{
    let runner = ClaudeRunner::new();
    runner.run(
        spec,
        story,
        spec_path,
        previous_iterations,
        knowledge,
        all_permissions,
        on_output,
        on_permission,
    )
}

fn build_prompt(
    spec: &Spec,
    story: &UserStory,
    spec_path: &Path,
    knowledge_context: Option<&str>,
    previous_context: Option<&str>,
) -> String {
    let acceptance_criteria = story
        .acceptance_criteria
        .iter()
        .map(|c| format!("- {}", c))
        .collect::<Vec<_>>()
        .join("\n");

    let spec_path_str = spec_path.display();

    // Build the project knowledge section if we have context
    let knowledge_section = match knowledge_context {
        Some(context) => format!(
            r#"
## Project Knowledge

{}
"#,
            context
        ),
        None => String::new(),
    };

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
2. Write tests to verify the implementation if useful
3. Run the related tests to ensure they pass
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

## Structured Context (Optional)

If helpful for future agents, include any of these optional context blocks:

**Files worked with** (key files and their purpose):
```
<files-context>
path/to/file.rs | Brief purpose description | [key_symbol1, key_symbol2]
</files-context>
```

**Architectural decisions** (when you made significant choices):
```
<decisions>
topic | choice made | rationale
</decisions>
```

**Patterns established** (conventions future agents should follow):
```
<patterns>
Description of pattern or convention
</patterns>
```

These are optional - only include them when they add value for subsequent work.

## Project Context

{spec_description}{knowledge}{previous_work}

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
        knowledge = knowledge_section,
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

        let prompt = build_prompt(&spec, &story, spec_path, None, None);
        assert!(prompt.contains("TestProject"));
        assert!(prompt.contains("US-001"));
        assert!(prompt.contains("Criterion 1"));
        assert!(!prompt.contains("Previous Work"));
        assert!(!prompt.contains("Project Knowledge"));
    }

    #[test]
    fn test_build_prompt_includes_structured_context_section() {
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
            acceptance_criteria: vec!["Test criterion".into()],
            priority: 1,
            passes: false,
            notes: String::new(),
        };
        let spec_path = Path::new("/tmp/spec-test.json");

        let prompt = build_prompt(&spec, &story, spec_path, None, None);
        assert!(prompt.contains("## Structured Context (Optional)"));
    }

    #[test]
    fn test_build_prompt_includes_files_context_instructions() {
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
            acceptance_criteria: vec!["Test criterion".into()],
            priority: 1,
            passes: false,
            notes: String::new(),
        };
        let spec_path = Path::new("/tmp/spec-test.json");

        let prompt = build_prompt(&spec, &story, spec_path, None, None);
        assert!(prompt.contains("<files-context>"));
        assert!(prompt.contains("</files-context>"));
        assert!(prompt.contains("path/to/file.rs | Brief purpose description"));
    }

    #[test]
    fn test_build_prompt_includes_decisions_instructions() {
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
            acceptance_criteria: vec!["Test criterion".into()],
            priority: 1,
            passes: false,
            notes: String::new(),
        };
        let spec_path = Path::new("/tmp/spec-test.json");

        let prompt = build_prompt(&spec, &story, spec_path, None, None);
        assert!(prompt.contains("<decisions>"));
        assert!(prompt.contains("</decisions>"));
        assert!(prompt.contains("topic | choice made | rationale"));
    }

    #[test]
    fn test_build_prompt_includes_patterns_instructions() {
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
            acceptance_criteria: vec!["Test criterion".into()],
            priority: 1,
            passes: false,
            notes: String::new(),
        };
        let spec_path = Path::new("/tmp/spec-test.json");

        let prompt = build_prompt(&spec, &story, spec_path, None, None);
        assert!(prompt.contains("<patterns>"));
        assert!(prompt.contains("</patterns>"));
    }

    #[test]
    fn test_build_prompt_structured_context_is_optional() {
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
            acceptance_criteria: vec!["Test criterion".into()],
            priority: 1,
            passes: false,
            notes: String::new(),
        };
        let spec_path = Path::new("/tmp/spec-test.json");

        let prompt = build_prompt(&spec, &story, spec_path, None, None);
        // Instructions should make it clear that context is optional
        assert!(prompt.contains("Optional"));
        assert!(prompt.contains("optional"));
        assert!(prompt.contains("only include them when they add value"));
    }

    #[test]
    fn test_build_prompt_with_empty_knowledge_no_section() {
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
            acceptance_criteria: vec!["Test criterion".into()],
            priority: 1,
            passes: false,
            notes: String::new(),
        };
        let spec_path = Path::new("/tmp/spec-test.json");

        // With None knowledge context, no Project Knowledge section should appear
        let prompt = build_prompt(&spec, &story, spec_path, None, None);
        assert!(!prompt.contains("## Project Knowledge"));
    }

    #[test]
    fn test_build_prompt_with_knowledge_context() {
        let spec = Spec {
            project: "TestProject".into(),
            branch_name: "test-branch".into(),
            description: "A test project".into(),
            user_stories: vec![],
        };
        let story = UserStory {
            id: "US-002".into(),
            title: "Second Story".into(),
            description: "A second test story".into(),
            acceptance_criteria: vec!["Test criterion".into()],
            priority: 2,
            passes: false,
            notes: String::new(),
        };
        let spec_path = Path::new("/tmp/spec-test.json");

        let knowledge_context = r#"## Files Modified in This Run

| Path | Purpose | Key Symbols | Stories |
|------|---------|-------------|---------|
| s/main.rs | Entry point | main | US-001 |

## Architectural Decisions

- **Database**: SQLite — Embedded, no setup"#;

        let prompt = build_prompt(&spec, &story, spec_path, Some(knowledge_context), None);

        // Should include the Project Knowledge section
        assert!(prompt.contains("## Project Knowledge"));
        assert!(prompt.contains("## Files Modified in This Run"));
        assert!(prompt.contains("s/main.rs"));
        assert!(prompt.contains("## Architectural Decisions"));
        assert!(prompt.contains("SQLite"));
    }

    #[test]
    fn test_build_prompt_knowledge_appears_before_previous_work() {
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
            acceptance_criteria: vec!["Test criterion".into()],
            priority: 3,
            passes: false,
            notes: String::new(),
        };
        let spec_path = Path::new("/tmp/spec-test.json");

        let knowledge_context = "Files modified: src/main.rs";
        let previous_context = "US-001: Added feature X\nUS-002: Added feature Y";

        let prompt = build_prompt(
            &spec,
            &story,
            spec_path,
            Some(knowledge_context),
            Some(previous_context),
        );

        // Both sections should exist
        assert!(prompt.contains("## Project Knowledge"));
        assert!(prompt.contains("## Previous Work"));

        // Knowledge should appear before Previous Work
        let knowledge_pos = prompt.find("## Project Knowledge").unwrap();
        let previous_work_pos = prompt.find("## Previous Work").unwrap();
        assert!(
            knowledge_pos < previous_work_pos,
            "Project Knowledge section should appear before Previous Work section"
        );
    }

    #[test]
    fn test_build_prompt_with_previous_work_only() {
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
            acceptance_criteria: vec!["Test criterion".into()],
            priority: 2,
            passes: false,
            notes: String::new(),
        };
        let spec_path = Path::new("/tmp/spec-test.json");

        let previous_context = "US-001: Added authentication module";

        let prompt = build_prompt(&spec, &story, spec_path, None, Some(previous_context));

        // Should include Previous Work but not Project Knowledge
        assert!(!prompt.contains("## Project Knowledge"));
        assert!(prompt.contains("## Previous Work"));
        assert!(prompt.contains("US-001: Added authentication module"));
    }

    #[test]
    fn test_build_prompt_with_both_knowledge_and_previous_work() {
        let spec = Spec {
            project: "TestProject".into(),
            branch_name: "test-branch".into(),
            description: "A test project".into(),
            user_stories: vec![],
        };
        let story = UserStory {
            id: "US-003".into(),
            title: "Third Story".into(),
            description: "Build on previous work".into(),
            acceptance_criteria: vec!["Test criterion".into()],
            priority: 3,
            passes: false,
            notes: String::new(),
        };
        let spec_path = Path::new("/tmp/spec-test.json");

        let knowledge_context = r#"## Files Modified

| Path | Purpose |
|------|---------|
| s/auth.rs | Authentication |"#;
        let previous_context = "US-001: Added auth\nUS-002: Added config";

        let prompt = build_prompt(
            &spec,
            &story,
            spec_path,
            Some(knowledge_context),
            Some(previous_context),
        );

        // Both sections should exist
        assert!(prompt.contains("## Project Knowledge"));
        assert!(prompt.contains("## Previous Work"));
        assert!(prompt.contains("s/auth.rs"));
        assert!(prompt.contains("US-001: Added auth"));
        assert!(prompt.contains("US-002: Added config"));
    }

    #[test]
    fn test_build_prompt_knowledge_section_structure() {
        let spec = Spec {
            project: "TestProject".into(),
            branch_name: "test-branch".into(),
            description: "A test project".into(),
            user_stories: vec![],
        };
        let story = UserStory {
            id: "US-002".into(),
            title: "Test Story".into(),
            description: "Test description".into(),
            acceptance_criteria: vec!["Test".into()],
            priority: 1,
            passes: false,
            notes: String::new(),
        };
        let spec_path = Path::new("/tmp/spec-test.json");

        let knowledge_context = "Test knowledge content";

        let prompt = build_prompt(&spec, &story, spec_path, Some(knowledge_context), None);

        // The knowledge section should have the ## Project Knowledge header
        // followed by the content
        assert!(prompt.contains("## Project Knowledge\n\nTest knowledge content"));
    }

    #[test]
    fn test_build_json_user_message() {
        let message = build_json_user_message("Hello, Claude!");
        let parsed: serde_json::Value = serde_json::from_str(&message).unwrap();

        assert_eq!(parsed["type"], "user");
        assert_eq!(parsed["message"]["role"], "user");
        assert_eq!(parsed["message"]["content"], "Hello, Claude!");
    }

    #[test]
    fn test_build_json_user_message_with_multiline() {
        let prompt = "Line 1\nLine 2\nLine 3";
        let message = build_json_user_message(prompt);
        let parsed: serde_json::Value = serde_json::from_str(&message).unwrap();

        assert_eq!(parsed["message"]["content"], "Line 1\nLine 2\nLine 3");
    }

    #[test]
    fn test_build_json_user_message_with_special_chars() {
        let prompt = r#"Test with "quotes" and \backslashes\"#;
        let message = build_json_user_message(prompt);
        let parsed: serde_json::Value = serde_json::from_str(&message).unwrap();

        // Verify it round-trips correctly
        assert_eq!(
            parsed["message"]["content"].as_str().unwrap(),
            r#"Test with "quotes" and \backslashes\"#
        );
    }

    #[test]
    fn test_parse_control_request_valid() {
        let line = r#"{"type":"control_request","request_id":"req-123","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"git push"}}}"#;
        let request = parse_control_request(line);

        assert!(request.is_some());
        let request = request.unwrap();
        assert_eq!(request.request_id, "req-123");
        assert_eq!(request.request.tool_name, "Bash");
    }

    #[test]
    fn test_parse_control_request_wrong_type() {
        // Not a control_request
        let line = r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"text":"hello"}}}"#;
        let request = parse_control_request(line);
        assert!(request.is_none());
    }

    #[test]
    fn test_parse_control_request_wrong_subtype() {
        // control_request but not can_use_tool
        let line = r#"{"type":"control_request","request_id":"req-123","request":{"subtype":"other_subtype","tool_name":"Bash","input":{}}}"#;
        let request = parse_control_request(line);
        assert!(request.is_none());
    }

    #[test]
    fn test_parse_control_request_invalid_json() {
        let line = "not valid json";
        let request = parse_control_request(line);
        assert!(request.is_none());
    }

    #[test]
    fn test_parse_control_request_text_output() {
        // Normal text output should not parse as control request
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Working on task..."}]}}"#;
        let request = parse_control_request(line);
        assert!(request.is_none());
    }
}
