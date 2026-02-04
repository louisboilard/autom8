/// Prompt for interactive spec creation with Claude.
/// Users paste this into a Claude session to create their spec-<feature>.md file.
pub const SPEC_SKILL_PROMPT: &str = r####"# Spec Creation Assistant

You are helping the user create a specification document for a software feature. Your goal is to gather enough information to produce a well-structured spec-<feature>.md file.

## Process

1. **Understand the project context** - Ask about the existing codebase, tech stack, and what they're building
2. **Define the feature** - Get a clear description of what they want to implement
3. **Break down into user stories** - Identify discrete, implementable units of work
4. **Define acceptance criteria** - For each story, establish clear pass/fail criteria
5. **Prioritize** - Help order the stories by dependency and importance

## Questions to Ask

Start with these, then drill deeper based on responses:

- What project is this for? What's the tech stack?
- What feature or functionality do you want to add?
- Who is this feature for? What problem does it solve?
- Are there any existing patterns or conventions in the codebase I should follow?
- What are the must-have vs nice-to-have aspects?
- Are there any constraints (performance, security, compatibility)?

## Output Format

Once you have enough information, generate a spec-<feature>.md file in this format:

```markdown
# [Feature Name]

## Project
[Project name]

## Branch
[Suggested branch name, e.g., feature/user-auth]

## Description
[2-3 paragraph description of the feature, its purpose, and context]

## User Stories

### US-001: [Story Title]
**Priority:** 1

[Description of what this story accomplishes]

**Acceptance Criteria:**
- [ ] [Criterion 1]
- [ ] [Criterion 2]
- [ ] [Criterion 3]

**Notes:** [Any implementation hints or context]

### US-002: [Story Title]
**Priority:** 2

...
```

## Guidelines

- Each user story should be implementable in a single Claude session
- Acceptance criteria should be specific and testable
- Lower priority number = higher priority (1 is highest)
- Include 3-7 user stories for most features
- Stories should be ordered by dependency (prerequisites first)

## REQUIRED Save Location

**CRITICAL:** You MUST save the spec file to this exact location:

```
~/.config/autom8/<project-name>/spec/spec-[feature-name].md
```

Where:
- `<project-name>` is the basename of the current working directory (e.g., if in `/Users/alice/projects/my-app`, use `my-app`)
- `[feature-name]` is a kebab-case name for the feature (e.g., `user-authentication`, `task-priority`)

**This is NOT a suggestion - this is the required location.** The autom8 tool expects files in this directory structure. Do NOT save to the current working directory or any other location.

---

## Handoff to Implementation

After you save the spec file, follow this handoff protocol:

### Step 1: Confirm the Save
After successfully saving the spec file, confirm to the user:
```
✓ Spec saved to: ~/.config/autom8/<project>/spec/spec-<feature>.md
```

### Step 2: Ask About Implementation
Immediately after confirming the save, ask:
```
Ready for autom8 to start implementation? [Y/n]
```

### Step 3: Handle the Response

**If the user confirms** (responds with: y, yes, yeah, go, proceed, ok, sure, yep, or just presses Enter):
1. Print exactly: `Handing off to autom8...`
2. Then type `/exit` to exit the session

**If the user declines** (responds with: n, no, not yet, wait, hold on, changes, edit):
1. Ask: "What would you like to change?"
2. Help them refine the spec
3. After saving again, repeat from Step 1

### Important Notes
- Always complete the handoff protocol after saving
- The `/exit` command ends the Claude session, which signals autom8 to detect the new spec
- Be responsive to user intent - if they seem ready to proceed, treat it as confirmation
- If the response is ambiguous, ask for clarification

---

Let's begin! Tell me about the feature you want to build.
"####;

/// Prompt for committing changes after all stories are complete.
/// Placeholders: {project}, {feature_description}, {stories_summary}
pub const COMMIT_PROMPT: &str = r####"You are committing changes for a completed feature.

## Context

**Project:** {project}
**Feature:** {feature_description}

**User stories implemented:**
{stories_summary}

## Your Task

Create clean, logical git commits for the changes made to implement this feature.

## Step-by-Step Workflow

1. **Analyze changes**: Run `git status` to see all modified/new files
2. **Identify feature files**: Determine which files are part of THIS feature implementation
3. **Plan commits**: Group related changes into logical commits
4. **Commit implementation first**: Stage and commit source code changes
5. **Commit tests separately**: Stage and commit test files in their own commit(s)
6. **Verify**: Run `git status` to confirm all feature files are committed

## CRITICAL: File Selection Rules

You MUST be extremely careful about which files you commit.

### NEVER commit these (always exclude):
- `spec-*.json` - autom8 spec JSON files, not part of the feature
- `spec-*.md` - autom8 spec markdown files, not part of the feature
- `autom8_review.md` - autom8 review file, not part of the feature
- `.autom8/` - autom8 internal directory
- `tasks/` - task tracking directory
- `.env` or any credentials/secrets
- `node_modules/`, `target/`, `dist/`, `build/` - build artifacts
- Any file you did not create or modify for this feature

### ONLY commit files that:
- You created specifically for this feature
- You modified to implement this feature
- Are tests you wrote for this feature

### How to identify feature files:
- Look at the user stories above - what functionality did they require?
- Only files directly related to that functionality should be committed
- When in doubt, DO NOT commit the file

## Commit Organization

Make **multiple logical commits**, not one big commit. Examples:

**Good commit structure:**
```
1. "Add user authentication service"     (core implementation)
2. "Add login and logout API endpoints"  (related feature code)
3. "[test] Add auth service unit tests"  (tests for commit 1)
4. "[test] Add auth API integration tests" (tests for commit 2)
```

**Bad (avoid this):**
```
1. "Add feature"  (everything in one commit)
```

## Commit Message Rules

- Use imperative mood: "Add feature" not "Added feature"
- Keep under 50 characters
- No period at the end
- Be specific: "Add user login endpoint" not "Update code"
- Prefix test commits with `[test]`

## Git Commands

- Stage specific files: `git add path/to/file.rs path/to/other.rs`
- NEVER use: `git add .` or `git add -A` (too dangerous)
- Commit: `git commit -m "Your message"`
- Do NOT use `--author` flag (uses system git config automatically)
- Do NOT add any "Co-Authored-By" line to commit messages - the user must be the sole author
- Do NOT push (autom8 handles that separately)

## Error Handling

- If pre-commit hooks fail: fix the issue, re-stage files, and retry once
- If there are no changes to commit: output "Nothing to commit"
- If unsure about a file: skip it and mention it in your output

## Final Checklist

Before each commit, verify:
- [ ] Only feature-related files are staged
- [ ] No spec-*.json, spec-*.md, autom8_review.md, or .autom8/ files staged
- [ ] Commit message is clear and under 50 chars
- [ ] Tests are in separate commits with [test] prefix
"####;

/// Prompt for converting spec-<feature>.md to spec-<feature>.json (used internally by autom8).
pub const SPEC_JSON_PROMPT: &str = r####"Convert the following spec markdown document into a valid JSON format.

## Input Spec:

{spec_content}

## Output Requirements

Produce a JSON object with this exact structure:

```json
{
  "project": "Project Name",
  "branchName": "feature/branch-name",
  "description": "Feature description paragraph(s)",
  "userStories": [
    {
      "id": "US-001",
      "title": "Story title",
      "description": "What this story accomplishes",
      "acceptanceCriteria": [
        "Criterion 1",
        "Criterion 2"
      ],
      "priority": 1,
      "passes": false,
      "notes": "Implementation hints or empty string"
    }
  ]
}
```

## Rules

1. Extract the project name from the "## Project" section
2. Extract branch name from "## Branch" section (default to "autom8/feature" if not specified)
3. Extract description from "## Description" section
4. Parse each "### US-XXX" section as a user story
5. Priority should be a number (1 = highest priority)
6. All stories should have `passes: false` initially
7. Convert markdown checkbox items to plain text acceptance criteria
8. Use camelCase for JSON keys

## Output

Return ONLY the JSON object, no markdown code fences, no explanation. The output must be valid JSON that can be parsed directly.
"####;

/// Prompt for asking Claude to fix malformed JSON from a previous spec generation attempt.
/// Placeholders: {spec_content}, {malformed_json}, {error_message}, {attempt}, {max_attempts}
pub const SPEC_JSON_CORRECTION_PROMPT: &str = r####"The previous JSON generation attempt produced malformed JSON that failed to parse.

## Original Spec

This is the original spec document that the JSON should represent:

{spec_content}

## Error Details

**Parse Error:** {error_message}
**Attempt:** {attempt}/{max_attempts}

## Malformed JSON

The following JSON output failed to parse (shown as plain text):

---
{malformed_json}
---

## Common Errors to Check

Before fixing, check for these common JSON syntax errors:

1. **Trailing commas** - Remove commas before closing `]` or `}`
   - Bad: `[1, 2, 3,]` → Good: `[1, 2, 3]`
2. **Unquoted keys** - All object keys must be double-quoted
   - Bad: `{foo: "bar"}` → Good: `{"foo": "bar"}`
3. **Unclosed brackets/braces** - Ensure every `[` has `]` and every `{` has `}`
4. **Invalid escape sequences** - Use `\\n` not `\n` in strings, escape `"` as `\"`
5. **Single quotes** - JSON requires double quotes, not single quotes
   - Bad: `{'key': 'value'}` → Good: `{"key": "value"}`

## Your Task

Fix the JSON above OR regenerate it from the original spec if the JSON is too corrupted to fix. The JSON should:
1. Be syntactically correct (proper quoting, commas, brackets)
2. Preserve all the original content and meaning (or regenerate from spec if needed)
3. Follow the expected schema with these fields:
   - `project` (string)
   - `branchName` (string)
   - `description` (string)
   - `userStories` (array of objects with: id, title, description, acceptanceCriteria, priority, passes, notes)

## Expected Output Format

The output must be a valid JSON object with this exact structure:

{
  "project": "Project Name",
  "branchName": "feature/branch-name",
  "description": "Feature description paragraph(s)",
  "userStories": [
    {
      "id": "US-001",
      "title": "Story title",
      "description": "What this story accomplishes",
      "acceptanceCriteria": [
        "Criterion 1",
        "Criterion 2"
      ],
      "priority": 1,
      "passes": false,
      "notes": "Implementation hints or empty string"
    }
  ]
}

## Output

**CRITICAL: Do NOT wrap your output in markdown code fences (```json or ```). Return ONLY the raw JSON object.**

Return ONLY the corrected JSON object, no code fences, no explanation. The output must be valid JSON that can be parsed directly.
"####;

/// Prompt for the reviewer agent that checks completed work for issues.
/// Placeholders: {project}, {feature_description}, {stories_context}, {iteration}, {max_iterations}
pub const REVIEWER_PROMPT: &str = r####"You are a code reviewer checking completed feature work for quality issues.

## Context

**Project:** {project}
**Feature:** {feature_description}

**Review iteration {iteration}/{max_iterations}**

You have a maximum of 3 review cycles. Focus on critical issues, not nitpicks.

## Spec Context (All User Stories)

{stories_context}

## Review Strategy by Iteration

Your review approach MUST vary based on the iteration:

- **Iteration 1**: Thorough review - check everything comprehensively
- **Iteration 2**: Only significant issues - skip minor style/naming concerns
- **Iteration 3**: Only blocking bugs - things that would cause runtime errors or security issues

## Your Task

Review the changes made for this feature implementation.

### Step 1: Run Automated Checks

First, run any available automated checks:

1. **Run tests** (if test suite exists):
   - Rust: `cargo test`
   - Node.js: `npm test` or `yarn test`
   - Python: `pytest` or `python -m unittest`
   - Go: `go test ./...`

2. **Run typecheck/lint** (if available):
   - Rust: `cargo check` and `cargo clippy`
   - TypeScript: `npm run typecheck` or `npx tsc --noEmit`
   - Python: `mypy .` or `pyright`
   - Go: `go vet ./...`

### Step 2: Manual Code Review

Review the implementation for:

1. **Bugs**: Logic errors, off-by-one errors, null/undefined handling, race conditions
2. **Missing tests**: Features without test coverage, edge cases not tested
3. **Code quality**: Dead code, overly complex logic, unclear naming
4. **Pattern consistency**: Does the code follow existing patterns in the codebase?
5. **Needless repetition**: DRY violations, copy-pasted code that should be abstracted

### Step 3: Output Results

Based on your findings:

**If ALL checks pass** (tests pass, typecheck passes, no issues found):
- Delete `autom8_review.md` if it exists
- Output nothing to the file
- The absence of this file signals success

**If issues are found**:
- Write issues to `autom8_review.md` (overwrite if exists)
- Use this format:

```markdown
# Review Issues (Iteration {iteration}/{max_iterations})

## Critical
- [ ] Issue description with file and line reference

## Significant
- [ ] Issue description with file and line reference

## Minor (iteration 1 only)
- [ ] Issue description with file and line reference

## Test Failures
- [ ] Test name: failure reason

## Typecheck/Lint Errors
- [ ] Error message and location
```

## Important Rules

1. Do NOT include nitpicks in iteration 2 or 3
2. Be specific - include file paths and line numbers
3. Focus on the changes made for THIS feature, not pre-existing issues
4. If tests fail, include the failure output
5. If typecheck/lint fails, include the errors
"####;

/// Prompt for the PR review agent that analyzes PR comments and determines if they represent real issues.
/// Placeholders: {spec_context}, {pr_description}, {commit_history}, {unresolved_comments}
pub const PR_REVIEW_PROMPT: &str = r####"You are a PR review agent analyzing pull request feedback to determine which comments represent real issues that need to be fixed.

## Your Role

Your job is to analyze unresolved comments on this pull request and determine:
1. **Is the comment factually correct?** Does it accurately describe the code behavior or a real problem?
2. **Is this a real issue?** Is it a bug, security vulnerability, performance problem, or legitimate code quality concern?
3. **Is it a red herring?** Is the reviewer mistaken, looking at outdated code, or misunderstanding the intent?

For each comment that represents a **real issue**, you must fix it.

## Context

{spec_context}

### PR Description

{pr_description}

### Branch Commits

These commits show the changes made in this branch:

{commit_history}

### Unresolved Comments

The following comments are unresolved and require your analysis:

{unresolved_comments}

## Analysis Process

For each comment:

### Step 1: Understand the Comment
- Read the comment carefully
- Identify what the reviewer is claiming or suggesting
- Note the file and line number (if provided)

### Step 2: Verify Against the Code
- Read the relevant code to verify the claim
- Check if the code actually behaves as the reviewer describes
- Consider the context from the spec and PR description

### Step 3: Make a Determination
Classify each comment as one of:

**REAL ISSUE** - The comment identifies a genuine problem:
- Code has a bug that could cause incorrect behavior
- There's a security vulnerability
- Performance problem that matters
- Missing error handling for likely failure cases
- Violation of documented requirements (from spec)

**RED HERRING** - The comment is incorrect or not actionable:
- Reviewer misread the code
- Code is correct but reviewer misunderstands the intent
- Issue was already fixed in a later commit
- Stylistic preference with no functional impact
- Hypothetical concern that doesn't apply to this context

**LEGITIMATE SUGGESTION** - Valid improvement but not a bug:
- Better naming or code organization
- Additional test coverage
- Documentation improvement

### Step 4: Act on Real Issues
For each **REAL ISSUE**:
1. Read the affected file(s)
2. Implement the fix
3. Verify the fix addresses the concern

For **RED HERRINGS** and **LEGITIMATE SUGGESTIONS**: Take no action (the PR author can decide how to respond to reviewers).

## Output Format

For each comment, output your analysis in this format:

```
### Comment from @{author} ({location})
> {comment_text}

**Analysis:** [Your reasoning about what the comment claims and whether it's accurate]

**Verdict:** [REAL ISSUE | RED HERRING | LEGITIMATE SUGGESTION]

**Action:** [Description of fix made, or "No action required"]
```

## Important Guidelines

1. **Be thorough**: Read the actual code before making determinations. Don't guess.
2. **Trust the spec**: If the spec defines behavior, code that matches the spec is correct.
3. **Consider context**: A comment might be wrong because the reviewer looked at the wrong commit.
4. **Fix conservatively**: Only fix issues that are clearly problems. Don't refactor on suggestion.
5. **Explain your reasoning**: Your analysis helps the PR author respond to reviewers.

## Final Summary

After analyzing all comments, provide a summary:

```
## Summary

**Total comments analyzed:** X
**Real issues fixed:** Y
**Red herrings identified:** Z
**Legitimate suggestions (no action):** W
```

Begin your analysis now.
"####;

/// Prompt for the PR template population agent.
/// This agent fills in a PR template using spec data and executes the gh command.
/// Placeholders: {spec_data}, {template_content}, {gh_command}
pub const PR_TEMPLATE_PROMPT: &str = r####"You are a PR creation agent. Your task is to fill in a pull request template using the provided spec data, then execute the GitHub CLI command to create or update the PR.

## Spec Data

The following is the spec for this feature:

{spec_data}

## PR Template

Fill in the following PR template. Replace placeholder sections with appropriate content based on the spec data above:

```markdown
{template_content}
```

## Your Task

1. **Analyze the template**: Identify sections that need to be filled in (e.g., Description, Summary, Changes, etc.)

2. **Fill the template**: Replace template placeholders and sections with content derived from the spec:
   - Use the spec description for summary/description sections
   - List user stories as changes or features
   - Mark completed stories with checkboxes if the template uses them
   - Keep the template structure intact - only fill in the content areas

3. **Execute the command**: After preparing the filled template, run this exact command with the filled template as the body:

```
{gh_command}
```

The `--body` argument should contain your filled-in template content.

## Important Guidelines

- Preserve the template's original structure and section headers
- Do not add sections that don't exist in the template
- Do not remove sections from the template
- If a section doesn't apply, write "N/A" or leave a brief note
- Use markdown formatting consistent with the template

## Output

After successfully executing the `gh` command, output the PR URL that was created or updated.

If the command fails, output an error message explaining what went wrong.

Do NOT output any other content after the PR URL or error - this helps parse your output.
"####;

/// Prompt for the corrector agent that fixes issues found by the reviewer.
/// Placeholders: {project}, {feature_description}, {stories_context}, {iteration}, {max_iterations}
pub const CORRECTOR_PROMPT: &str = r####"You are a corrector agent fixing issues identified during code review.

## Context

**Project:** {project}
**Feature:** {feature_description}

**Correction iteration {iteration}/{max_iterations}**

This is your chance to fix the issues. Focus on the most critical fixes first.

## Spec Context (All User Stories)

{stories_context}

## Your Task

Read the review file and fix the issues identified by the reviewer.

### Step 1: Read the Review File

Read `autom8_review.md` to see the list of issues to address.

### Step 2: Triage Issues

Not all issues need to be fixed. Use your judgment:

- **Must fix**: Test failures, typecheck errors, bugs, security issues
- **Should fix**: Missing tests for new code, code quality issues
- **Optional**: Style suggestions, minor refactoring ideas

If you disagree with an issue, you may skip it - but be judicious. The reviewer identified these for a reason.

### Step 3: Fix Issues

Work through the issues systematically:

1. Start with critical/blocking issues (test failures, typecheck errors)
2. Then address significant bugs or missing functionality
3. Finally, handle minor issues if time permits

For each issue you fix:
- Make the necessary code changes
- Verify the fix works (run relevant tests if applicable)

### Step 4: Update the Review File

After fixing issues, update `autom8_review.md` to annotate what you fixed:

**Original:**
```markdown
- [ ] Missing null check in user_service.rs:42
```

**After fixing:**
```markdown
- [x] FIXED: Missing null check in user_service.rs:42
```

For issues you chose not to fix, add a note:
```markdown
- [ ] SKIPPED: Style suggestion - keeping current naming for consistency
```

### Step 5: Run Verification

After making fixes, run the same checks the reviewer ran:

1. **Run tests** (if test suite exists):
   - Rust: `cargo test`
   - Node.js: `npm test` or `yarn test`
   - Python: `pytest` or `python -m unittest`
   - Go: `go test ./...`

2. **Run typecheck/lint** (if available):
   - Rust: `cargo check` and `cargo clippy`
   - TypeScript: `npm run typecheck` or `npx tsc --noEmit`
   - Python: `mypy .` or `pyright`
   - Go: `go vet ./...`

## Prioritization Guidelines

Since this is iteration {iteration}/{max_iterations}, prioritize accordingly:

- **Iteration 1**: Fix all issues you can reasonably address
- **Iteration 2**: Focus only on issues that would block the feature
- **Iteration 3**: Fix ONLY test failures and typecheck errors - nothing else

## Important Rules

1. Always read `autom8_review.md` first - don't guess at issues
2. Mark each fixed issue with "FIXED:" prefix in the review file
3. Run tests after making changes to verify fixes
4. Do NOT create new issues or expand scope - only fix what's listed
5. If a fix would require significant refactoring, mark as SKIPPED with explanation
"####;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reviewer_prompt_contains_placeholders() {
        assert!(REVIEWER_PROMPT.contains("{project}"));
        assert!(REVIEWER_PROMPT.contains("{feature_description}"));
        assert!(REVIEWER_PROMPT.contains("{stories_context}"));
        assert!(REVIEWER_PROMPT.contains("{iteration}"));
        assert!(REVIEWER_PROMPT.contains("{max_iterations}"));
    }

    #[test]
    fn reviewer_prompt_contains_iteration_display() {
        assert!(REVIEWER_PROMPT.contains("Review iteration {iteration}/{max_iterations}"));
    }

    #[test]
    fn reviewer_prompt_contains_max_cycles_warning() {
        assert!(REVIEWER_PROMPT.contains("You have a maximum of 3 review cycles"));
        assert!(REVIEWER_PROMPT.contains("Focus on critical issues, not nitpicks"));
    }

    #[test]
    fn reviewer_prompt_contains_iteration_strategy() {
        assert!(REVIEWER_PROMPT.contains("Iteration 1"));
        assert!(REVIEWER_PROMPT.contains("Thorough review"));
        assert!(REVIEWER_PROMPT.contains("Iteration 2"));
        assert!(REVIEWER_PROMPT.contains("Only significant issues"));
        assert!(REVIEWER_PROMPT.contains("Iteration 3"));
        assert!(REVIEWER_PROMPT.contains("Only blocking bugs"));
    }

    #[test]
    fn reviewer_prompt_instructs_write_to_review_file() {
        assert!(REVIEWER_PROMPT.contains("autom8_review.md"));
        assert!(REVIEWER_PROMPT.contains("overwrite if exists"));
    }

    #[test]
    fn reviewer_prompt_instructs_delete_on_pass() {
        assert!(REVIEWER_PROMPT.contains("Delete `autom8_review.md` if it exists"));
        assert!(REVIEWER_PROMPT.contains("Output nothing to the file"));
    }

    #[test]
    fn reviewer_prompt_contains_review_criteria() {
        assert!(REVIEWER_PROMPT.contains("Bugs"));
        assert!(REVIEWER_PROMPT.contains("Missing tests"));
        assert!(REVIEWER_PROMPT.contains("Code quality"));
        assert!(REVIEWER_PROMPT.contains("Pattern consistency"));
        assert!(
            REVIEWER_PROMPT.contains("Needless repetition")
                || REVIEWER_PROMPT.contains("repetition")
        );
    }

    #[test]
    fn reviewer_prompt_instructs_run_tests() {
        assert!(REVIEWER_PROMPT.contains("Run tests"));
        assert!(REVIEWER_PROMPT.contains("cargo test"));
        assert!(REVIEWER_PROMPT.contains("npm test"));
    }

    #[test]
    fn reviewer_prompt_instructs_run_typecheck() {
        assert!(REVIEWER_PROMPT.contains("typecheck"));
        assert!(REVIEWER_PROMPT.contains("cargo check"));
        assert!(REVIEWER_PROMPT.contains("npm run typecheck") || REVIEWER_PROMPT.contains("tsc"));
    }

    #[test]
    fn corrector_prompt_contains_placeholders() {
        assert!(CORRECTOR_PROMPT.contains("{project}"));
        assert!(CORRECTOR_PROMPT.contains("{feature_description}"));
        assert!(CORRECTOR_PROMPT.contains("{stories_context}"));
        assert!(CORRECTOR_PROMPT.contains("{iteration}"));
        assert!(CORRECTOR_PROMPT.contains("{max_iterations}"));
    }

    #[test]
    fn corrector_prompt_contains_iteration_display() {
        assert!(CORRECTOR_PROMPT.contains("Correction iteration {iteration}/{max_iterations}"));
    }

    #[test]
    fn corrector_prompt_instructs_read_review_file() {
        assert!(CORRECTOR_PROMPT.contains("autom8_review.md"));
        assert!(CORRECTOR_PROMPT.contains("Read the review file"));
        assert!(CORRECTOR_PROMPT.contains("Read `autom8_review.md`"));
    }

    #[test]
    fn corrector_prompt_instructs_use_judgment() {
        assert!(CORRECTOR_PROMPT.contains("Use your judgment"));
        assert!(CORRECTOR_PROMPT.contains("Not all issues need to be fixed"));
    }

    #[test]
    fn corrector_prompt_instructs_fixed_prefix() {
        assert!(CORRECTOR_PROMPT.contains("FIXED:"));
        assert!(CORRECTOR_PROMPT.contains("annotate what you fixed"));
    }

    #[test]
    fn corrector_prompt_contains_iteration_prioritization() {
        assert!(CORRECTOR_PROMPT.contains("iteration {iteration}/{max_iterations}"));
        assert!(CORRECTOR_PROMPT.contains("Iteration 1"));
        assert!(CORRECTOR_PROMPT.contains("Iteration 2"));
        assert!(CORRECTOR_PROMPT.contains("Iteration 3"));
        assert!(CORRECTOR_PROMPT.contains("most critical fixes first"));
    }

    #[test]
    fn spec_skill_prompt_specifies_required_save_location() {
        // Must contain the exact save path pattern
        assert!(SPEC_SKILL_PROMPT
            .contains("~/.config/autom8/<project-name>/spec/spec-[feature-name].md"));
    }

    #[test]
    fn spec_skill_prompt_emphasizes_save_location_is_required() {
        // Must emphasize this is required, not a suggestion
        assert!(SPEC_SKILL_PROMPT.contains("REQUIRED"));
        assert!(SPEC_SKILL_PROMPT.contains("CRITICAL"));
        assert!(SPEC_SKILL_PROMPT.contains("NOT a suggestion"));
    }

    #[test]
    fn spec_skill_prompt_warns_against_wrong_location() {
        // Must explicitly warn against saving to wrong location
        assert!(SPEC_SKILL_PROMPT.contains("Do NOT save to the current working directory"));
    }

    #[test]
    fn spec_skill_prompt_uses_spec_terminology() {
        // Verify prompt uses "spec" terminology, not "PRD"
        assert!(SPEC_SKILL_PROMPT.contains("specification document"));
        assert!(SPEC_SKILL_PROMPT.contains("spec-<feature>.md"));
        assert!(!SPEC_SKILL_PROMPT.contains("PRD"));
    }

    #[test]
    fn spec_json_prompt_uses_spec_terminology() {
        // Verify JSON conversion prompt uses "spec" terminology
        assert!(SPEC_JSON_PROMPT.contains("## Input Spec:"));
        assert!(!SPEC_JSON_PROMPT.contains("PRD"));
    }

    // ========================================================================
    // JSON correction prompt tests (US-005)
    // ========================================================================

    #[test]
    fn spec_json_correction_prompt_contains_placeholders() {
        assert!(SPEC_JSON_CORRECTION_PROMPT.contains("{malformed_json}"));
        assert!(SPEC_JSON_CORRECTION_PROMPT.contains("{error_message}"));
        assert!(SPEC_JSON_CORRECTION_PROMPT.contains("{attempt}"));
        assert!(SPEC_JSON_CORRECTION_PROMPT.contains("{max_attempts}"));
    }

    #[test]
    fn spec_json_correction_prompt_explains_task() {
        assert!(SPEC_JSON_CORRECTION_PROMPT.contains("malformed JSON"));
        assert!(SPEC_JSON_CORRECTION_PROMPT.contains("failed to parse"));
        assert!(SPEC_JSON_CORRECTION_PROMPT.contains("Fix the JSON"));
    }

    #[test]
    fn spec_json_correction_prompt_describes_expected_schema() {
        assert!(SPEC_JSON_CORRECTION_PROMPT.contains("project"));
        assert!(SPEC_JSON_CORRECTION_PROMPT.contains("branchName"));
        assert!(SPEC_JSON_CORRECTION_PROMPT.contains("description"));
        assert!(SPEC_JSON_CORRECTION_PROMPT.contains("userStories"));
    }

    #[test]
    fn spec_json_correction_prompt_requests_only_json_output() {
        assert!(SPEC_JSON_CORRECTION_PROMPT.contains("Return ONLY the corrected JSON"));
        assert!(SPEC_JSON_CORRECTION_PROMPT.contains("no code fences"));
    }

    #[test]
    fn spec_json_correction_prompt_can_be_populated() {
        let populated = SPEC_JSON_CORRECTION_PROMPT
            .replace("{spec_content}", "# Test Spec\n\n## Project\ntest-project")
            .replace("{malformed_json}", r#"{"project": "test"#)
            .replace("{error_message}", "unexpected end of input")
            .replace("{attempt}", "2")
            .replace("{max_attempts}", "3");

        assert!(populated.contains(r#"{"project": "test"#));
        assert!(populated.contains("unexpected end of input"));
        assert!(populated.contains("2/3"));
        assert!(populated.contains("# Test Spec"));
    }

    // ========================================================================
    // JSON correction prompt tests for US-001 (enhanced correction prompt)
    // ========================================================================

    #[test]
    fn spec_json_correction_prompt_contains_spec_content_placeholder() {
        // US-001: Correction prompt includes the original spec markdown content
        assert!(
            SPEC_JSON_CORRECTION_PROMPT.contains("{spec_content}"),
            "Correction prompt must include {{spec_content}} placeholder for original spec"
        );
    }

    #[test]
    fn spec_json_correction_prompt_contains_common_errors_guidance() {
        // US-001: Correction prompt lists specific common errors to check
        assert!(
            SPEC_JSON_CORRECTION_PROMPT.contains("Trailing commas"),
            "Should mention trailing commas error"
        );
        assert!(
            SPEC_JSON_CORRECTION_PROMPT.contains("Unquoted keys"),
            "Should mention unquoted keys error"
        );
        assert!(
            SPEC_JSON_CORRECTION_PROMPT.contains("Unclosed brackets")
                || SPEC_JSON_CORRECTION_PROMPT.contains("Unclosed brackets/braces"),
            "Should mention unclosed brackets/braces error"
        );
        assert!(
            SPEC_JSON_CORRECTION_PROMPT.contains("Invalid escape sequences")
                || SPEC_JSON_CORRECTION_PROMPT.contains("escape sequences"),
            "Should mention invalid escape sequences error"
        );
    }

    #[test]
    fn spec_json_correction_prompt_does_not_wrap_malformed_json_in_code_fences() {
        // US-001: Correction prompt does NOT wrap the malformed JSON in markdown code fences
        // Check that the malformed JSON section uses plain text block (--- delimiters) not ```json
        assert!(
            !SPEC_JSON_CORRECTION_PROMPT.contains("```json\n{malformed_json}"),
            "Malformed JSON should not be wrapped in ```json code fences"
        );
        assert!(
            !SPEC_JSON_CORRECTION_PROMPT.contains("```\n{malformed_json}"),
            "Malformed JSON should not be wrapped in ``` code fences"
        );
        // The malformed JSON should be shown as plain text with --- delimiters
        assert!(
            SPEC_JSON_CORRECTION_PROMPT.contains("---\n{malformed_json}\n---"),
            "Malformed JSON should be wrapped in plain text block with --- delimiters"
        );
    }

    #[test]
    fn spec_json_correction_prompt_contains_valid_json_example() {
        // US-001: Correction prompt includes an example of valid JSON output
        // The example should match the structure from SPEC_JSON_PROMPT
        assert!(
            SPEC_JSON_CORRECTION_PROMPT.contains(r#""project": "Project Name""#),
            "Should include example project field"
        );
        assert!(
            SPEC_JSON_CORRECTION_PROMPT.contains(r#""branchName": "feature/branch-name""#),
            "Should include example branchName field"
        );
        assert!(
            SPEC_JSON_CORRECTION_PROMPT.contains(r#""userStories": ["#),
            "Should include example userStories array"
        );
        assert!(
            SPEC_JSON_CORRECTION_PROMPT.contains(r#""id": "US-001""#),
            "Should include example story id"
        );
        assert!(
            SPEC_JSON_CORRECTION_PROMPT.contains(r#""acceptanceCriteria": ["#),
            "Should include example acceptanceCriteria array"
        );
    }

    #[test]
    fn spec_json_correction_prompt_uses_stronger_code_fence_warning() {
        // US-001: Correction prompt uses stronger language about not using code fences
        assert!(
            SPEC_JSON_CORRECTION_PROMPT.contains("CRITICAL"),
            "Should use CRITICAL keyword for emphasis"
        );
        assert!(
            SPEC_JSON_CORRECTION_PROMPT.contains("Do NOT wrap")
                || SPEC_JSON_CORRECTION_PROMPT
                    .contains("Do NOT wrap your output in markdown code fences"),
            "Should explicitly instruct not to wrap in code fences"
        );
    }

    #[test]
    fn spec_json_correction_prompt_includes_original_spec_section() {
        // US-001: The original spec should be clearly labeled
        assert!(
            SPEC_JSON_CORRECTION_PROMPT.contains("## Original Spec")
                || SPEC_JSON_CORRECTION_PROMPT.contains("Original Spec"),
            "Should have a section for the original spec"
        );
    }

    // ========================================================================
    // PR Review Prompt tests (US-004)
    // ========================================================================

    #[test]
    fn pr_review_prompt_contains_required_placeholders() {
        // US-004: Include spec content, PR description, commits, and comments
        assert!(
            PR_REVIEW_PROMPT.contains("{spec_context}"),
            "Must include spec_context placeholder"
        );
        assert!(
            PR_REVIEW_PROMPT.contains("{pr_description}"),
            "Must include pr_description placeholder"
        );
        assert!(
            PR_REVIEW_PROMPT.contains("{commit_history}"),
            "Must include commit_history placeholder"
        );
        assert!(
            PR_REVIEW_PROMPT.contains("{unresolved_comments}"),
            "Must include unresolved_comments placeholder"
        );
    }

    #[test]
    fn pr_review_prompt_instructs_factual_correctness_check() {
        // US-004: Instruct agent to analyze each comment for factual correctness
        assert!(
            PR_REVIEW_PROMPT.contains("factually correct")
                || PR_REVIEW_PROMPT.contains("factual")
                || PR_REVIEW_PROMPT.contains("accurately describe"),
            "Should instruct agent to check factual correctness"
        );
    }

    #[test]
    fn pr_review_prompt_instructs_determine_real_issues() {
        // US-004: Instruct agent to determine if reported issues are actual bugs/problems
        assert!(
            PR_REVIEW_PROMPT.contains("real issue") || PR_REVIEW_PROMPT.contains("REAL ISSUE"),
            "Should instruct agent to identify real issues"
        );
        assert!(
            PR_REVIEW_PROMPT.contains("bug") || PR_REVIEW_PROMPT.contains("Bug"),
            "Should mention bugs as a type of real issue"
        );
    }

    #[test]
    fn pr_review_prompt_instructs_fix_confirmed_issues() {
        // US-004: Instruct agent to fix confirmed real issues
        assert!(
            PR_REVIEW_PROMPT.contains("fix") || PR_REVIEW_PROMPT.contains("Fix"),
            "Should instruct agent to fix issues"
        );
        assert!(
            PR_REVIEW_PROMPT.contains("Implement the fix")
                || PR_REVIEW_PROMPT.contains("fix it")
                || PR_REVIEW_PROMPT.contains("Action"),
            "Should provide instructions for fixing issues"
        );
    }

    #[test]
    fn pr_review_prompt_distinguishes_red_herrings() {
        // US-004: The prompt should help distinguish real issues from red herrings
        assert!(
            PR_REVIEW_PROMPT.contains("red herring") || PR_REVIEW_PROMPT.contains("RED HERRING"),
            "Should mention red herrings"
        );
    }

    #[test]
    fn pr_review_prompt_has_structured_analysis_format() {
        // US-004: Follow existing prompt patterns - should have structured sections
        assert!(
            PR_REVIEW_PROMPT.contains("## ") || PR_REVIEW_PROMPT.contains("### "),
            "Should use markdown headers for structure"
        );
        assert!(
            PR_REVIEW_PROMPT.contains("Step 1") || PR_REVIEW_PROMPT.contains("step"),
            "Should have step-by-step analysis process"
        );
    }

    #[test]
    fn pr_review_prompt_includes_verdict_categories() {
        // US-004: Should categorize each comment
        assert!(
            PR_REVIEW_PROMPT.contains("REAL ISSUE"),
            "Should have REAL ISSUE verdict category"
        );
        assert!(
            PR_REVIEW_PROMPT.contains("RED HERRING"),
            "Should have RED HERRING verdict category"
        );
        assert!(
            PR_REVIEW_PROMPT.contains("LEGITIMATE SUGGESTION")
                || PR_REVIEW_PROMPT.contains("suggestion"),
            "Should have a category for legitimate suggestions"
        );
    }

    #[test]
    fn pr_review_prompt_includes_summary_section() {
        // US-004: Should include a summary of findings
        assert!(
            PR_REVIEW_PROMPT.contains("## Summary") || PR_REVIEW_PROMPT.contains("Summary"),
            "Should include a summary section"
        );
        assert!(
            PR_REVIEW_PROMPT.contains("Total comments analyzed")
                || PR_REVIEW_PROMPT.contains("comments analyzed"),
            "Summary should count analyzed comments"
        );
    }

    #[test]
    fn pr_review_prompt_can_be_populated() {
        // US-004: Verify placeholders can be replaced
        let spec_context = "### Spec: Test Feature\n- Story 1: Implement X";
        let pr_description = "This PR adds feature X";
        let commit_history = "abc1234 - Add feature X\ndef5678 - Fix tests";
        let comments = "### Comment from @reviewer (src/main.rs:42)\n> This looks wrong";

        let populated = PR_REVIEW_PROMPT
            .replace("{spec_context}", spec_context)
            .replace("{pr_description}", pr_description)
            .replace("{commit_history}", commit_history)
            .replace("{unresolved_comments}", comments);

        assert!(populated.contains("### Spec: Test Feature"));
        assert!(populated.contains("This PR adds feature X"));
        assert!(populated.contains("abc1234 - Add feature X"));
        assert!(populated.contains("### Comment from @reviewer"));
        assert!(!populated.contains("{spec_context}"));
        assert!(!populated.contains("{pr_description}"));
        assert!(!populated.contains("{commit_history}"));
        assert!(!populated.contains("{unresolved_comments}"));
    }

    #[test]
    fn pr_review_prompt_mentions_spec_for_context() {
        // US-004: Should reference the spec for determining correct behavior
        assert!(
            PR_REVIEW_PROMPT.contains("spec") || PR_REVIEW_PROMPT.contains("Spec"),
            "Should reference the spec for context"
        );
        assert!(
            PR_REVIEW_PROMPT.contains("Trust the spec")
                || PR_REVIEW_PROMPT.contains("spec defines"),
            "Should instruct to trust the spec for determining correct behavior"
        );
    }

    #[test]
    fn pr_review_prompt_considers_comment_location() {
        // US-004: Should handle comment author and location context
        assert!(
            PR_REVIEW_PROMPT.contains("author") || PR_REVIEW_PROMPT.contains("@{author}"),
            "Should reference comment author"
        );
        assert!(
            PR_REVIEW_PROMPT.contains("location") || PR_REVIEW_PROMPT.contains("line"),
            "Should reference comment location"
        );
    }

    // ========================================================================
    // PR Template Prompt tests (US-002)
    // ========================================================================

    #[test]
    fn pr_template_prompt_contains_spec_data_placeholder() {
        assert!(
            PR_TEMPLATE_PROMPT.contains("{spec_data}"),
            "Must include spec_data placeholder"
        );
    }

    #[test]
    fn pr_template_prompt_contains_template_content_placeholder() {
        assert!(
            PR_TEMPLATE_PROMPT.contains("{template_content}"),
            "Must include template_content placeholder"
        );
    }

    #[test]
    fn pr_template_prompt_contains_gh_command_placeholder() {
        assert!(
            PR_TEMPLATE_PROMPT.contains("{gh_command}"),
            "Must include gh_command placeholder"
        );
    }

    #[test]
    fn pr_template_prompt_instructs_fill_template() {
        assert!(
            PR_TEMPLATE_PROMPT.contains("Fill") || PR_TEMPLATE_PROMPT.contains("fill"),
            "Should instruct agent to fill the template"
        );
        assert!(
            PR_TEMPLATE_PROMPT.contains("template"),
            "Should reference the template"
        );
    }

    #[test]
    fn pr_template_prompt_instructs_execute_command() {
        assert!(
            PR_TEMPLATE_PROMPT.contains("Execute") || PR_TEMPLATE_PROMPT.contains("execute"),
            "Should instruct agent to execute the command"
        );
        assert!(
            PR_TEMPLATE_PROMPT.contains("gh") || PR_TEMPLATE_PROMPT.contains("command"),
            "Should reference the gh command"
        );
    }

    #[test]
    fn pr_template_prompt_mentions_spec_data() {
        assert!(
            PR_TEMPLATE_PROMPT.contains("spec") || PR_TEMPLATE_PROMPT.contains("Spec"),
            "Should reference spec data"
        );
    }

    #[test]
    fn pr_template_prompt_mentions_pr_url_output() {
        assert!(
            PR_TEMPLATE_PROMPT.contains("PR URL") || PR_TEMPLATE_PROMPT.contains("URL"),
            "Should instruct agent to output PR URL"
        );
    }

    #[test]
    fn pr_template_prompt_instructs_preserve_structure() {
        assert!(
            PR_TEMPLATE_PROMPT.contains("structure") || PR_TEMPLATE_PROMPT.contains("Preserve"),
            "Should instruct agent to preserve template structure"
        );
    }

    #[test]
    fn pr_template_prompt_can_be_populated() {
        let spec_data = "**Project:** Test\n**Description:** Test feature";
        let template = "## Description\n\n<!-- Describe your changes -->";
        let command = "gh pr create --title \"Test\" --body \"<filled>\"";

        let populated = PR_TEMPLATE_PROMPT
            .replace("{spec_data}", spec_data)
            .replace("{template_content}", template)
            .replace("{gh_command}", command);

        assert!(populated.contains("**Project:** Test"));
        assert!(populated.contains("## Description"));
        assert!(populated.contains("gh pr create"));
        assert!(!populated.contains("{spec_data}"));
        assert!(!populated.contains("{template_content}"));
        assert!(!populated.contains("{gh_command}"));
    }
}
