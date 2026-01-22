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
