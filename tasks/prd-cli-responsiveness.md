# PRD: CLI Responsiveness Fixes

## Introduction

Fix the CLI's broken responsiveness features. The application previously implemented several UX improvements—a spinner, a real-time timer, single-line output preview, and verbose mode—but currently **only the spinner animation works**. The timer appears frozen, the single-line preview doesn't show Claude's output, and verbose mode likely doesn't work either. This PRD focuses on diagnosing and fixing these broken features so users can see progress and know the application isn't hanging.

## Problem Statement

Current broken behavior:
- **Timer:** Displays but appears frozen (doesn't update in real-time)
- **Single-line preview:** Was supposed to show Claude's current activity but shows nothing
- **Verbose mode:** Likely also broken (not showing full Claude output)
- **Spinner:** Only working component (animation runs)

Root cause hypothesis (from codebase exploration):
- Timer only updates when `update()` is called, which depends on receiving output from Claude
- If Claude's output isn't being captured/streamed correctly, both the timer and preview fail
- The streaming callback chain may be broken somewhere between `claude.rs` and `runner.rs`

## Goals

- Fix the timer to update every second independently of Claude's output
- Fix the single-line preview to show Claude's most recent output
- Fix verbose mode to show full scrolling output from Claude
- Diagnose and fix the underlying streaming/callback issue
- Keep the UI clean (single line in non-verbose mode, no clutter)

## User Stories

### US-001: Diagnose Streaming Issue
**Description:** As a developer, I need to understand why Claude's output isn't reaching the display callbacks so that I can fix the broken features.

**Acceptance Criteria:**
- [ ] Identify where in the callback chain output is lost (`claude.rs` -> `runner.rs` -> `progress.rs`)
- [ ] Determine if the issue is with subprocess stdout capture, line reading, or callback invocation
- [ ] Document the root cause
- [ ] Typecheck/lint passes (`cargo check`, `cargo clippy`)

### US-002: Fix Output Streaming
**Description:** As a user, I want Claude's output to be properly captured and streamed so that the preview and verbose features work.

**Acceptance Criteria:**
- [ ] Claude's stdout is properly captured line-by-line
- [ ] Each line triggers the `on_output` callback
- [ ] Callbacks receive the actual output content (not empty strings)
- [ ] Streaming works for all Claude operations (story implementation, PRD generation, commits)
- [ ] Typecheck/lint passes (`cargo check`, `cargo clippy`)

### US-003: Fix Real-Time Timer
**Description:** As a user, I want to see the elapsed time updating every second so that I know the application hasn't frozen.

**Acceptance Criteria:**
- [ ] Timer updates every second regardless of whether Claude produces output
- [ ] Timer uses an independent update mechanism (not tied to output callbacks)
- [ ] Timer display format remains `HH:MM:SS`
- [ ] Timer thread/task is properly cleaned up when Claude finishes
- [ ] Typecheck/lint passes (`cargo check`, `cargo clippy`)

### US-004: Fix Single-Line Output Preview
**Description:** As a user, I want to see a single line showing Claude's most recent output so that I understand what it's currently doing.

**Acceptance Criteria:**
- [ ] Most recent line of Claude's output is displayed
- [ ] Only one line shown at a time (replaces previous)
- [ ] Long lines are truncated with ellipsis to fit terminal width
- [ ] Preview updates in real-time as new output arrives
- [ ] Typecheck/lint passes (`cargo check`, `cargo clippy`)

### US-005: Fix Verbose Mode
**Description:** As a user running with `--verbose`, I want to see full Claude output scrolling in real-time.

**Acceptance Criteria:**
- [ ] Verbose mode flag (`--verbose` or `-v`) is properly detected
- [ ] In verbose mode, full output scrolls without truncation
- [ ] In non-verbose mode, single-line preview with truncation is shown
- [ ] Both modes show real-time timer
- [ ] Typecheck/lint passes (`cargo check`, `cargo clippy`)

### US-006: Clean Display on Completion
**Description:** As a user, I want the display to finish cleanly when Claude completes.

**Acceptance Criteria:**
- [ ] Timer stops and shows final elapsed time
- [ ] Preview line is cleared before completion message
- [ ] No visual artifacts or partial lines left on screen
- [ ] Typecheck/lint passes (`cargo check`, `cargo clippy`)

## Functional Requirements

- FR-1: Diagnose and fix the stdout streaming pipeline from Claude subprocess to display callbacks
- FR-2: Implement independent timer updates (every 1 second) using a separate thread or async task
- FR-3: Ensure `on_output` callback receives and processes each line of Claude's output
- FR-4: Truncate preview lines exceeding terminal width with "..." ellipsis
- FR-5: Detect terminal width dynamically
- FR-6: Support verbose mode showing full output vs non-verbose showing single-line preview
- FR-7: Clean up timer mechanism and display state when Claude process completes

## Non-Goals

- No new CLI flags (use existing `--verbose`)
- No different spinner styles based on activity state
- No horizontal scrolling animation for long text
- No parsing or categorizing Claude's output (just show raw lines)
- No activity history or logging

## Technical Considerations

### Key Files (from exploration)
| File | Purpose | Likely Issues |
|------|---------|---------------|
| `src/progress.rs` | Spinner, timer display, `update()` method | Timer tied to output callbacks |
| `src/claude.rs` | Subprocess spawning, stdout streaming | May not be capturing output correctly |
| `src/runner.rs` | Orchestration, callback integration | Callbacks may not be wired correctly |
| `src/output.rs` | CLI output formatting | May need changes for preview display |

### Debugging Steps
1. Add logging to verify Claude subprocess stdout is being read
2. Verify `on_output` callback is being invoked with content
3. Check if `--print` flag to Claude CLI produces expected output format
4. Trace the callback chain from `claude.rs` through `runner.rs` to `progress.rs`

### Timer Fix Approach
- Spawn independent thread that updates spinner message every second
- Use `Arc<AtomicBool>` or channel to signal thread termination
- `indicatif::ProgressBar` is `Send + Sync`, safe for cross-thread updates

### Dependencies
- `indicatif` 0.17 (already present)
- May need `terminal_size` crate for width detection

## Success Metrics

- Timer updates every second without freezing
- Single-line preview shows Claude's actual output in real-time
- Verbose mode shows full scrolling output
- No regression in spinner animation
- Clean completion without visual artifacts

## Open Questions

- Is the `--print` flag to Claude CLI producing the expected streaming output?
- Are there buffering issues with the subprocess stdout pipe?
- Should we strip ANSI codes from preview lines for cleaner display?
