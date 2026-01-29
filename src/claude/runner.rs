//! Main Claude runner for story implementation.
//!
//! Handles running Claude to implement individual user stories.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use crate::error::{Autom8Error, Result};
use crate::knowledge::ProjectKnowledge;
use crate::spec::{Spec, UserStory};
use crate::state::IterationRecord;

use super::stream::extract_text_from_stream_line;
use super::types::{ClaudeErrorInfo, ClaudeOutcome, ClaudeStoryResult};
use super::utils::{build_knowledge_context, build_previous_context, extract_work_summary};

const COMPLETION_SIGNAL: &str = "<promise>COMPLETE</promise>";

pub fn run_claude<F>(
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
}
