# autom8

A CLI automation tool for orchestrating Claude-powered development. Define your feature requirements in a PRD, and let autom8 drive Claude through iterative implementation of each user story.

## Overview

autom8 bridges the gap between product requirements and working code. You describe what you want to build, and autom8 orchestrates Claude to implement it story by story, tracking progress and managing state throughout.

### Key Features

- **PRD-driven development** - Define features as structured user stories with acceptance criteria
- **Iterative implementation** - Claude implements one story at a time, marking progress as it goes
- **State persistence** - Interrupt and resume runs at any time
- **Git integration** - Automatic branch management for feature development
- **Progress tracking** - Visual feedback on implementation progress

## Installation

```bash
cargo install --path .
```

Requires the `claude` CLI to be installed and configured.

## Quick Start

### 1. Install skills (one-time setup)

```bash
autom8 init
```

This installs the `/pdr` skill to `~/.claude/skills/` so Claude knows how to create PRDs.

### 2. Create your PRD

```bash
claude
```

Then in the Claude session, use the skill:

```
/pdr
```

Claude will ask you questions about your feature and generate a `prd.md` file.

### 3. Run autom8

```bash
autom8 prd.md
```

### 4. Watch it work

autom8 will:
1. Convert your `prd.md` to structured `prd.json`
2. Pick the highest-priority incomplete story
3. Run Claude to implement it
4. Review the implementation and fix any issues
5. Commit when all stories pass

## Workflow

```
┌─────────────────────────────────────────────────────────────┐
│ 1. One-time setup                                           │
│    $ autom8 init                                            │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│ 2. Create prd.md interactively with Claude                  │
│    $ claude                                                 │
│    > /pdr                                                   │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│ 3. Run autom8                                               │
│    $ autom8 prd.md                                          │
│                                                             │
│    - Converts prd.md → prd.json                             │
│    - Iterates through user stories                          │
│    - Claude implements each story                           │
│    - Reviews implementation, fixes issues                   │
│    - Commits all changes when feature is complete           │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│ 4. Feature complete!                                        │
│    All user stories implemented and passing                 │
└─────────────────────────────────────────────────────────────┘
```

## State Machine

autom8 uses a state machine to track progress through the implementation process:

```mermaid
stateDiagram-v2
    [*] --> Idle

    Idle --> LoadingSpec: autom8 prd.md
    Idle --> Initializing: autom8 prd.json

    LoadingSpec --> GeneratingPrd: Spec loaded
    GeneratingPrd --> Initializing: PRD generated

    Initializing --> PickingStory: Ready

    PickingStory --> RunningClaude: Story selected
    PickingStory --> Reviewing: All stories pass

    RunningClaude --> PickingStory: Iteration complete
    RunningClaude --> Failed: Error

    Reviewing --> Correcting: Issues found
    Reviewing --> Committing: No issues
    Reviewing --> Failed: Max review iterations

    Correcting --> Reviewing: Corrections applied

    Committing --> Completed: Commit done
    Committing --> Failed: Commit error

    Completed --> [*]
    Failed --> [*]
```

### State Descriptions

| State | Description |
|-------|-------------|
| `idle` | Initial state, no active run |
| `loading-spec` | Loading and validating prd.md file |
| `generating-prd` | Claude converting prd.md → prd.json |
| `initializing` | Loading PRD, setting up git branch |
| `picking-story` | Selecting next incomplete user story |
| `running-claude` | Claude implementing current story |
| `reviewing` | Claude reviewing completed implementation |
| `correcting` | Claude fixing issues found during review |
| `committing` | Claude committing changes for completed feature |
| `completed` | All user stories pass |
| `failed` | Error occurred, run stopped |

## CLI Commands

```bash
autom8 init               # Install skills to ~/.claude/skills/ (one-time setup)
autom8                    # Auto-detect and run (interactive)
autom8 <file>             # Run with specific prd.md or prd.json
autom8 run --prd <file>   # Explicit run command
autom8 run --skip-review  # Skip the review loop
autom8 status             # Check current run status
autom8 resume             # Resume a failed/interrupted run
autom8 history            # List past runs
autom8 archive            # Archive current run and reset
autom8 clean              # Delete prd.md and prd.json
autom8 skill prd          # Output PRD creation prompt
autom8 skill prd-json     # Output PRD→JSON conversion prompt
```

## PRD Format

### Markdown (prd.md)

```markdown
# Feature Name

## Project
my-project

## Branch
feature/my-feature

## Description
A description of what this feature does and why it's needed.

## User Stories

### US-001: First Story
**Priority:** 1

Description of what this story accomplishes.

**Acceptance Criteria:**
- [ ] First criterion
- [ ] Second criterion

**Notes:** Implementation hints

### US-002: Second Story
**Priority:** 2
...
```

### JSON (prd.json)

```json
{
  "project": "my-project",
  "branchName": "feature/my-feature",
  "description": "A description of what this feature does...",
  "userStories": [
    {
      "id": "US-001",
      "title": "First Story",
      "description": "Description of what this story accomplishes",
      "acceptanceCriteria": ["First criterion", "Second criterion"],
      "priority": 1,
      "passes": false,
      "notes": "Implementation hints"
    }
  ]
}
```

## How It Works

1. **Story Selection**: autom8 picks the highest-priority story where `passes: false`

2. **Claude Execution**: Spawns Claude with a prompt containing:
   - Project context
   - Story details and acceptance criteria
   - Instructions to implement, test, and mark complete

3. **Completion Detection**: Claude updates `prd.json` setting `passes: true` when a story's acceptance criteria are met

4. **Iteration**: Process repeats until all stories pass or max iterations reached

5. **Review Loop**: When all stories pass, Claude reviews the implementation:
   - Checks for issues, edge cases, and code quality
   - If issues are found, enters correction mode to fix them
   - Review/correct cycles up to 3 times before failing

6. **Committing**: When review passes, Claude commits changes (only files it modified, excluding prd.json and .autom8/)

## State Persistence

Run state is saved to `.autom8/state.json`, allowing you to:
- Interrupt with Ctrl+C and resume later
- Check progress with `autom8 status`
- Review history with `autom8 history`

Completed runs are archived to `.autom8/runs/`.

## Configuration

### Max Iterations

Limit the number of Claude invocations (default: 10):

```bash
autom8 run --prd prd.json --max_iterations 20
```

### Git Integration

If running in a git repository, autom8 will:
- Check out or create the branch specified in `branchName`
- Allow Claude to commit changes as it implements

## Example Session

```
$ autom8 prd.md

+---------------------------------------------------------+
|  autom8 v0.1.0                                          |
+---------------------------------------------------------+

[state] idle -> loading-spec
Spec: ./prd.md (1.2 KB)

[state] loading-spec -> generating-prd
Converting to prd.json...
Claude is working...

PRD Generated Successfully
Project: my-api
Stories: 3
  - US-001: Set up project structure
  - US-002: Implement user endpoint
  - US-003: Add authentication

Saved: .autom8/prds/prd.json

[state] generating-prd -> initializing
Proceeding to implementation...

[state] initializing -> picking-story
Project: my-api
Branch:  feature/my-api
Stories: [░░░░░░░░░░░░] 0/3 complete

[state] picking-story -> running-claude
Iteration 1/10 - Running US-001: Set up project structure
Claude is working...

[state] running-claude -> picking-story
Story US-001 complete!

... (more iterations) ...

[state] picking-story -> reviewing
All stories complete! Running review...
Review 1/3 - Checking implementation...

[state] reviewing -> committing
Review passed! Committing changes...

[state] committing -> completed
Feature complete!
```

## License

MIT
