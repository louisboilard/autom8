/// Full SKILL.md content for the PRD skill (installed to ~/.claude/skills/pdr/)
pub const PRD_SKILL_MD: &str = include_str!("../pdr/SKILL.md");

/// Full SKILL.md content for the prd-json skill (installed to ~/.claude/skills/pdr-json/)
pub const PRD_JSON_SKILL_MD: &str = include_str!("../pdr_json/SKILL.md");

/// Prompt for interactive PRD creation with Claude.
/// Users paste this into a Claude session to create their prd.md file.
pub const PRD_SKILL_PROMPT: &str = r####"# PRD Creation Assistant

You are helping the user create a Product Requirements Document (PRD) for a software feature. Your goal is to gather enough information to produce a well-structured prd.md file.

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

Once you have enough information, generate a prd.md file in this format:

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
- `prd.json` - autom8 state file, not part of the feature
- `prd.md` - autom8 spec file, not part of the feature
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
- Do NOT push (autom8 handles that separately)

## Error Handling

- If pre-commit hooks fail: fix the issue, re-stage files, and retry once
- If there are no changes to commit: output "Nothing to commit"
- If unsure about a file: skip it and mention it in your output

## Final Checklist

Before each commit, verify:
- [ ] Only feature-related files are staged
- [ ] No prd.json, prd.md, or .autom8/ files staged
- [ ] Commit message is clear and under 50 chars
- [ ] Tests are in separate commits with [test] prefix
"####;

/// Prompt for converting prd.md to prd.json (used internally by autom8).
pub const PRD_JSON_PROMPT: &str = r####"Convert the following PRD markdown document into a valid JSON format.

## Input PRD:

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
