//! Main Claude runner for story implementation.
//!
//! Handles running Claude to implement individual user stories.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use crate::error::{Autom8Error, Result};
use crate::knowledge::ProjectKnowledge;
use crate::spec::{Spec, UserStory};
use crate::state::IterationRecord;

use super::stream::extract_text_from_stream_line;
use super::types::{ClaudeErrorInfo, ClaudeOutcome, ClaudeStoryResult, ProcessInfo};
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
    /// Process metadata (PID and spawn timestamp), cleared when process terminates
    process_info: Arc<Mutex<Option<ProcessInfo>>>,
    /// Tracks whether the process is currently paused (SIGSTOP sent)
    is_paused: Arc<Mutex<bool>>,
}

impl ClaudeRunner {
    /// Creates a new `ClaudeRunner` with no active subprocess.
    pub fn new() -> Self {
        Self {
            child: Arc::new(Mutex::new(None)),
            process_info: Arc::new(Mutex::new(None)),
            is_paused: Arc::new(Mutex::new(false)),
        }
    }

    /// Returns the PID of the running subprocess, if any.
    ///
    /// Returns `None` if no subprocess is currently running or if
    /// the process has terminated.
    pub fn pid(&self) -> Option<u32> {
        self.process_info
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(|info| info.pid))
    }

    /// Returns a clone of the current process info, if any.
    ///
    /// Returns `None` if no subprocess is currently running or if
    /// the process has terminated.
    pub fn process_info(&self) -> Option<ProcessInfo> {
        self.process_info
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }

    /// Returns whether the subprocess is currently paused.
    ///
    /// Returns `true` if `pause()` was called and `resume()` has not been called since.
    /// Returns `false` if no process is running, or the process is not paused.
    pub fn is_paused(&self) -> bool {
        self.is_paused.lock().map(|guard| *guard).unwrap_or(false)
    }

    /// Pauses the subprocess by sending SIGSTOP (Unix only).
    ///
    /// This method:
    /// - Sends SIGSTOP to the subprocess to pause execution
    /// - Updates internal pause state tracking
    /// - Is safe to call when already paused (no-op, returns Ok(true))
    ///
    /// Returns `Ok(true)` if the process is now paused (or was already paused),
    /// `Ok(false)` if no process is running.
    /// On non-Unix platforms, returns an error.
    #[cfg(unix)]
    pub fn pause(&self) -> Result<bool> {
        let child_guard = self.child.lock().map_err(|e| {
            Autom8Error::ClaudeError(format!("Failed to acquire lock for pause: {}", e))
        })?;

        if let Some(ref child) = *child_guard {
            let pid = child.id() as libc::pid_t;

            // Check if already paused
            if self.is_paused() {
                return Ok(true);
            }

            // Send SIGSTOP to pause the process
            let result = unsafe { libc::kill(pid, libc::SIGSTOP) };

            if result == 0 {
                // Update pause state
                if let Ok(mut paused_guard) = self.is_paused.lock() {
                    *paused_guard = true;
                }
                Ok(true)
            } else {
                let err = std::io::Error::last_os_error();
                Err(Autom8Error::ClaudeError(format!(
                    "Failed to pause Claude subprocess: {}",
                    err
                )))
            }
        } else {
            Ok(false)
        }
    }

    /// Pauses the subprocess (non-Unix stub).
    ///
    /// On non-Unix platforms, this operation is not supported.
    #[cfg(not(unix))]
    pub fn pause(&self) -> Result<bool> {
        Err(Autom8Error::ClaudeError(
            "Pause is not supported on this platform".to_string(),
        ))
    }

    /// Resumes the subprocess by sending SIGCONT (Unix only).
    ///
    /// This method:
    /// - Sends SIGCONT to the subprocess to resume execution
    /// - Updates internal pause state tracking
    /// - Is safe to call when not paused (no-op, returns Ok(true))
    ///
    /// Returns `Ok(true)` if the process is now running (or was already running),
    /// `Ok(false)` if no process is running.
    /// On non-Unix platforms, returns an error.
    #[cfg(unix)]
    pub fn resume(&self) -> Result<bool> {
        let child_guard = self.child.lock().map_err(|e| {
            Autom8Error::ClaudeError(format!("Failed to acquire lock for resume: {}", e))
        })?;

        if let Some(ref child) = *child_guard {
            let pid = child.id() as libc::pid_t;

            // Check if not paused
            if !self.is_paused() {
                return Ok(true);
            }

            // Send SIGCONT to resume the process
            let result = unsafe { libc::kill(pid, libc::SIGCONT) };

            if result == 0 {
                // Update pause state
                if let Ok(mut paused_guard) = self.is_paused.lock() {
                    *paused_guard = false;
                }
                Ok(true)
            } else {
                let err = std::io::Error::last_os_error();
                Err(Autom8Error::ClaudeError(format!(
                    "Failed to resume Claude subprocess: {}",
                    err
                )))
            }
        } else {
            Ok(false)
        }
    }

    /// Resumes the subprocess (non-Unix stub).
    ///
    /// On non-Unix platforms, this operation is not supported.
    #[cfg(not(unix))]
    pub fn resume(&self) -> Result<bool> {
        Err(Autom8Error::ClaudeError(
            "Resume is not supported on this platform".to_string(),
        ))
    }

    /// Kills the subprocess if it is running.
    ///
    /// This method:
    /// - Terminates the subprocess using SIGKILL
    /// - Closes stdin/stdout/stderr handles by dropping the Child
    /// - Clears the stored process info (PID and spawn time)
    /// - Is safe to call multiple times or when no subprocess is running
    ///
    /// Returns `Ok(true)` if a process was killed, `Ok(false)` if no process was running.
    pub fn kill(&self) -> Result<bool> {
        let mut child_guard = self.child.lock().map_err(|e| {
            Autom8Error::ClaudeError(format!("Failed to acquire lock for kill: {}", e))
        })?;

        if let Some(mut child) = child_guard.take() {
            // Clear process info since process is being terminated
            self.clear_process_info();

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

    /// Stores a child process handle in the runner and records process metadata.
    fn set_child(&self, child: Child) -> Result<()> {
        // Capture PID before storing the child
        let pid = child.id();

        let mut child_guard = self.child.lock().map_err(|e| {
            Autom8Error::ClaudeError(format!("Failed to acquire lock for set_child: {}", e))
        })?;
        *child_guard = Some(child);

        // Store process info
        let mut info_guard = self.process_info.lock().map_err(|e| {
            Autom8Error::ClaudeError(format!("Failed to acquire lock for process_info: {}", e))
        })?;
        *info_guard = Some(ProcessInfo::new(pid));

        Ok(())
    }

    /// Clears the stored process info and resets pause state.
    fn clear_process_info(&self) {
        if let Ok(mut guard) = self.process_info.lock() {
            *guard = None;
        }
        if let Ok(mut guard) = self.is_paused.lock() {
            *guard = false;
        }
    }

    /// Takes the child process out of the runner, returning it.
    /// Used when the process completes normally. Also clears process info.
    fn take_child(&self) -> Result<Option<Child>> {
        let mut child_guard = self.child.lock().map_err(|e| {
            Autom8Error::ClaudeError(format!("Failed to acquire lock for take_child: {}", e))
        })?;

        // Clear process info since process is being taken (will complete/terminate)
        self.clear_process_info();

        Ok(child_guard.take())
    }
}

impl Default for ClaudeRunner {
    fn default() -> Self {
        Self::new()
    }
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
    /// * `on_output` - Callback for streaming output
    pub fn run<F>(
        &self,
        spec: &Spec,
        story: &UserStory,
        spec_path: &Path,
        previous_iterations: &[IterationRecord],
        knowledge: &ProjectKnowledge,
        mut on_output: F,
    ) -> Result<ClaudeStoryResult>
    where
        F: FnMut(&str),
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

        // Write prompt to stdin - take and drop stdin handle to close it
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(prompt.as_bytes()).map_err(|e| {
                Autom8Error::ClaudeError(format!("Failed to write to stdin: {}", e))
            })?;
            // stdin is dropped here, closing the handle
        }

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

        for line in reader.lines() {
            let line = line.map_err(|e| Autom8Error::ClaudeError(format!("Read error: {}", e)))?;

            // Parse stream-json output and extract text content
            if let Some(text) = extract_text_from_stream_line(&line) {
                on_output(&text);
                accumulated_text.push_str(&text);

                if text.contains(COMPLETION_SIGNAL) || accumulated_text.contains(COMPLETION_SIGNAL)
                {
                    found_complete = true;
                }
            }
        }

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
        })
    }
}

/// Convenience function that creates a `ClaudeRunner` and runs Claude.
///
/// This maintains backwards compatibility with existing code that uses the
/// standalone function. For new code that needs to kill the subprocess
/// (e.g., on signal handling), use `ClaudeRunner` directly.
pub fn run_claude<F>(
    spec: &Spec,
    story: &UserStory,
    spec_path: &Path,
    previous_iterations: &[IterationRecord],
    knowledge: &ProjectKnowledge,
    on_output: F,
) -> Result<ClaudeStoryResult>
where
    F: FnMut(&str),
{
    let runner = ClaudeRunner::new();
    runner.run(
        spec,
        story,
        spec_path,
        previous_iterations,
        knowledge,
        on_output,
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
    fn test_claude_runner_new() {
        let runner = ClaudeRunner::new();
        assert!(!runner.is_running());
    }

    #[test]
    fn test_claude_runner_default() {
        let runner = ClaudeRunner::default();
        assert!(!runner.is_running());
    }

    #[test]
    fn test_claude_runner_kill_no_process() {
        let runner = ClaudeRunner::new();
        // kill() should return Ok(false) when no process is running
        let result = runner.kill();
        assert!(result.is_ok());
        assert!(!result.unwrap()); // false = no process was killed
    }

    #[test]
    fn test_claude_runner_kill_terminates_subprocess() {
        use std::thread;
        use std::time::Duration;

        let runner = ClaudeRunner::new();

        // Spawn a long-running process (sleep for 60 seconds)
        let child = Command::new("sleep")
            .arg("60")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to spawn sleep process");

        let pid = child.id();

        // Store the child in the runner
        runner.set_child(child).expect("Failed to set child");
        assert!(runner.is_running());

        // Kill the process
        let result = runner.kill();
        assert!(result.is_ok());
        assert!(result.unwrap()); // true = process was killed

        // Verify process is no longer running
        assert!(!runner.is_running());

        // Give the OS a moment to clean up
        thread::sleep(Duration::from_millis(50));

        // Verify the process is actually terminated by checking if kill would fail
        // (sending signal 0 to check if process exists)
        #[cfg(unix)]
        {
            let status = Command::new("kill").args(["-0", &pid.to_string()]).status();
            // Should fail because process no longer exists
            assert!(status.is_ok());
            assert!(!status.unwrap().success());
        }
    }

    #[test]
    fn test_claude_runner_clone_shares_state() {
        let runner1 = ClaudeRunner::new();
        let runner2 = runner1.clone();

        // Spawn a process and store in runner1
        let child = Command::new("sleep")
            .arg("60")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to spawn sleep process");

        runner1.set_child(child).expect("Failed to set child");

        // Both runners should see it as running
        assert!(runner1.is_running());
        assert!(runner2.is_running());

        // Kill via runner2
        let result = runner2.kill();
        assert!(result.is_ok());
        assert!(result.unwrap());

        // Both should now show not running
        assert!(!runner1.is_running());
        assert!(!runner2.is_running());
    }

    #[test]
    fn test_claude_runner_pid_none_when_no_process() {
        let runner = ClaudeRunner::new();
        assert!(runner.pid().is_none());
    }

    #[test]
    fn test_claude_runner_pid_available_after_spawn() {
        let runner = ClaudeRunner::new();

        // Spawn a process
        let child = Command::new("sleep")
            .arg("60")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to spawn sleep process");

        let expected_pid = child.id();

        // Store the child
        runner.set_child(child).expect("Failed to set child");

        // PID should now be available
        let pid = runner.pid();
        assert!(pid.is_some());
        assert_eq!(pid.unwrap(), expected_pid);

        // Clean up
        runner.kill().expect("Failed to kill process");
    }

    #[test]
    fn test_claude_runner_pid_cleared_after_kill() {
        let runner = ClaudeRunner::new();

        // Spawn a process
        let child = Command::new("sleep")
            .arg("60")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to spawn sleep process");

        runner.set_child(child).expect("Failed to set child");
        assert!(runner.pid().is_some());

        // Kill the process
        runner.kill().expect("Failed to kill process");

        // PID should be cleared
        assert!(runner.pid().is_none());
    }

    #[test]
    fn test_claude_runner_process_info_available_after_spawn() {
        let runner = ClaudeRunner::new();

        // Spawn a process
        let child = Command::new("sleep")
            .arg("60")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to spawn sleep process");

        let expected_pid = child.id();
        let before_spawn = chrono::Utc::now();

        runner.set_child(child).expect("Failed to set child");

        let after_spawn = chrono::Utc::now();

        // Process info should be available
        let info = runner.process_info();
        assert!(info.is_some());

        let info = info.unwrap();
        assert_eq!(info.pid, expected_pid);
        // Spawn time should be between before and after
        assert!(info.spawn_time >= before_spawn);
        assert!(info.spawn_time <= after_spawn);

        // Clean up
        runner.kill().expect("Failed to kill process");
    }

    #[test]
    fn test_claude_runner_process_info_cleared_after_kill() {
        let runner = ClaudeRunner::new();

        // Spawn a process
        let child = Command::new("sleep")
            .arg("60")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to spawn sleep process");

        runner.set_child(child).expect("Failed to set child");
        assert!(runner.process_info().is_some());

        // Kill the process
        runner.kill().expect("Failed to kill process");

        // Process info should be cleared
        assert!(runner.process_info().is_none());
    }

    #[test]
    fn test_claude_runner_pid_cleared_after_take_child() {
        let runner = ClaudeRunner::new();

        // Spawn a process
        let child = Command::new("sleep")
            .arg("60")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to spawn sleep process");

        runner.set_child(child).expect("Failed to set child");
        assert!(runner.pid().is_some());

        // Take the child (simulates normal completion)
        let taken = runner.take_child().expect("Failed to take child");
        assert!(taken.is_some());

        // PID should be cleared
        assert!(runner.pid().is_none());

        // Clean up the taken child
        let mut child = taken.unwrap();
        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn test_claude_runner_clone_shares_pid_state() {
        let runner1 = ClaudeRunner::new();
        let runner2 = runner1.clone();

        // Spawn a process and store in runner1
        let child = Command::new("sleep")
            .arg("60")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to spawn sleep process");

        let expected_pid = child.id();

        runner1.set_child(child).expect("Failed to set child");

        // Both runners should see the same PID
        assert_eq!(runner1.pid(), Some(expected_pid));
        assert_eq!(runner2.pid(), Some(expected_pid));

        // Kill via runner2
        runner2.kill().expect("Failed to kill process");

        // Both should now show no PID
        assert!(runner1.pid().is_none());
        assert!(runner2.pid().is_none());
    }

    #[test]
    fn test_claude_runner_is_paused_false_when_no_process() {
        let runner = ClaudeRunner::new();
        assert!(!runner.is_paused());
    }

    #[test]
    fn test_claude_runner_is_paused_false_after_spawn() {
        let runner = ClaudeRunner::new();

        // Spawn a process
        let child = Command::new("sleep")
            .arg("60")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to spawn sleep process");

        runner.set_child(child).expect("Failed to set child");

        // Process should not be paused after spawn
        assert!(!runner.is_paused());

        // Clean up
        runner.kill().expect("Failed to kill process");
    }

    #[cfg(unix)]
    #[test]
    fn test_claude_runner_pause_no_process() {
        let runner = ClaudeRunner::new();
        // pause() should return Ok(false) when no process is running
        let result = runner.pause();
        assert!(result.is_ok());
        assert!(!result.unwrap()); // false = no process to pause
    }

    #[cfg(unix)]
    #[test]
    fn test_claude_runner_resume_no_process() {
        let runner = ClaudeRunner::new();
        // resume() should return Ok(false) when no process is running
        let result = runner.resume();
        assert!(result.is_ok());
        assert!(!result.unwrap()); // false = no process to resume
    }

    #[cfg(unix)]
    #[test]
    fn test_claude_runner_pause_and_resume() {
        let runner = ClaudeRunner::new();

        // Spawn a process
        let child = Command::new("sleep")
            .arg("60")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to spawn sleep process");

        runner.set_child(child).expect("Failed to set child");
        assert!(!runner.is_paused());

        // Pause the process
        let result = runner.pause();
        assert!(result.is_ok());
        assert!(result.unwrap()); // true = process was paused
        assert!(runner.is_paused());

        // Resume the process
        let result = runner.resume();
        assert!(result.is_ok());
        assert!(result.unwrap()); // true = process was resumed
        assert!(!runner.is_paused());

        // Clean up
        runner.kill().expect("Failed to kill process");
    }

    #[cfg(unix)]
    #[test]
    fn test_claude_runner_pause_already_paused() {
        let runner = ClaudeRunner::new();

        // Spawn a process
        let child = Command::new("sleep")
            .arg("60")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to spawn sleep process");

        runner.set_child(child).expect("Failed to set child");

        // Pause the process
        let result = runner.pause();
        assert!(result.is_ok());
        assert!(result.unwrap());
        assert!(runner.is_paused());

        // Pause again - should be a no-op but return Ok(true)
        let result = runner.pause();
        assert!(result.is_ok());
        assert!(result.unwrap()); // true = process is paused (already was)
        assert!(runner.is_paused());

        // Clean up
        runner.kill().expect("Failed to kill process");
    }

    #[cfg(unix)]
    #[test]
    fn test_claude_runner_resume_not_paused() {
        let runner = ClaudeRunner::new();

        // Spawn a process
        let child = Command::new("sleep")
            .arg("60")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to spawn sleep process");

        runner.set_child(child).expect("Failed to set child");
        assert!(!runner.is_paused());

        // Resume without pausing first - should be a no-op but return Ok(true)
        let result = runner.resume();
        assert!(result.is_ok());
        assert!(result.unwrap()); // true = process is running (already was)
        assert!(!runner.is_paused());

        // Clean up
        runner.kill().expect("Failed to kill process");
    }

    #[cfg(unix)]
    #[test]
    fn test_claude_runner_pause_state_cleared_on_kill() {
        let runner = ClaudeRunner::new();

        // Spawn a process
        let child = Command::new("sleep")
            .arg("60")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to spawn sleep process");

        runner.set_child(child).expect("Failed to set child");

        // Pause the process
        runner.pause().expect("Failed to pause");
        assert!(runner.is_paused());

        // Kill the process
        runner.kill().expect("Failed to kill process");

        // Pause state should be cleared
        assert!(!runner.is_paused());
    }

    #[cfg(unix)]
    #[test]
    fn test_claude_runner_pause_state_cleared_on_take_child() {
        let runner = ClaudeRunner::new();

        // Spawn a process
        let child = Command::new("sleep")
            .arg("60")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to spawn sleep process");

        runner.set_child(child).expect("Failed to set child");

        // Pause the process
        runner.pause().expect("Failed to pause");
        assert!(runner.is_paused());

        // Take the child (simulates normal completion)
        let taken = runner.take_child().expect("Failed to take child");
        assert!(taken.is_some());

        // Pause state should be cleared
        assert!(!runner.is_paused());

        // Clean up the taken child
        let mut child = taken.unwrap();
        let _ = child.kill();
        let _ = child.wait();
    }

    #[cfg(unix)]
    #[test]
    fn test_claude_runner_clone_shares_pause_state() {
        let runner1 = ClaudeRunner::new();
        let runner2 = runner1.clone();

        // Spawn a process
        let child = Command::new("sleep")
            .arg("60")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to spawn sleep process");

        runner1.set_child(child).expect("Failed to set child");

        // Both should show not paused
        assert!(!runner1.is_paused());
        assert!(!runner2.is_paused());

        // Pause via runner1
        runner1.pause().expect("Failed to pause");

        // Both should show paused
        assert!(runner1.is_paused());
        assert!(runner2.is_paused());

        // Resume via runner2
        runner2.resume().expect("Failed to resume");

        // Both should show not paused
        assert!(!runner1.is_paused());
        assert!(!runner2.is_paused());

        // Clean up
        runner1.kill().expect("Failed to kill process");
    }
}
