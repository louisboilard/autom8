# PRD: Unified Agent Display System

## Introduction

Standardize the UI/UX across all agent phases (Runner, Reviewer, Corrector, Commit) to provide a consistent, clear, and informative user experience. Currently, the Runner agent has rich display features (timers, progress, duration) while Reviewer and Corrector have minimal displays. This feature unifies the visual language, making state transitions obvious and providing consistent context throughout the entire workflow.

## Goals

- Establish visual consistency across all agent phases (same timing display, completion format, transition indicators)
- Make state transitions immediately obvious with color-coded banners
- Always show elapsed time, current activity, iteration context, and overall progress
- Provide meaningful completion summaries with pass/fail, duration, and brief outcomes
- Display a persistent breadcrumb trail showing the user's journey through states

## User Stories

### US-001: Create unified display trait/interface for all agents
**Description:** As a developer, I need a common interface for agent display so all agents report status consistently.

**Acceptance Criteria:**
- [ ] Create `AgentDisplay` trait in `progress.rs` with methods: `start()`, `update()`, `finish_success()`, `finish_error()`, `finish_with_outcome()`
- [ ] Trait defines common display contract: agent name, elapsed time, activity preview, iteration info
- [ ] Both `VerboseTimer` and `ClaudeSpinner` implement this trait
- [ ] Typecheck passes (`cargo check`)

### US-002: Implement color-coded state transition banners
**Description:** As a user, I want clear visual separation when the system changes phases so I always know what's happening.

**Acceptance Criteria:**
- [ ] Add `print_phase_banner(phase_name, color)` function to `output.rs`
- [ ] Banner format: `━━━ PHASE_NAME ━━━` with appropriate color (cyan for start, green for success, red for failure)
- [ ] Banner width adapts to terminal width (not too wide, not too narrow)
- [ ] Banners used for: `RUNNING`, `REVIEWING`, `CORRECTING`, `COMMITTING`
- [ ] Banners are visually distinct but not overly tall (single line with box-drawing characters)
- [ ] Typecheck passes

### US-003: Add iteration and progress context to all agent displays
**Description:** As a user, I want to see my current position in the workflow (which story, which review iteration, overall progress) at all times.

**Acceptance Criteria:**
- [ ] Spinner/timer prefix shows: `[US-001 2/5]` for story progress or `[Review 1/3]` for review iterations
- [ ] Format: `[{identifier} {current}/{total}]` consistently across all agents
- [ ] Runner shows: `[US-001 2/5]` (story 2 of 5)
- [ ] Reviewer shows: `[Review 1/3]` (review iteration 1 of max 3)
- [ ] Corrector shows: `[Correct 1/3]` (correction iteration 1 of max 3)
- [ ] Commit shows: `[Commit]` (no iteration needed)
- [ ] Typecheck passes

### US-004: Standardize completion messages with outcomes
**Description:** As a user, I want each phase completion to show pass/fail status, duration, and a brief outcome description.

**Acceptance Criteria:**
- [ ] All agents show completion in format: `✓ {Phase} completed in {duration} - {outcome}` or `✗ {Phase} failed in {duration} - {error}`
- [ ] Runner completion: `✓ US-001 completed in 2m 34s - Implementation done`
- [ ] Reviewer pass: `✓ Review 1/3 passed in 45s - No issues found`
- [ ] Reviewer fail: `✓ Review 1/3 completed in 1m 12s - 3 issues found`
- [ ] Corrector completion: `✓ Correct 1/3 completed in 1m 45s - Issues addressed`
- [ ] Commit completion: `✓ Commit completed in 12s - abc1234`
- [ ] Duration format is consistent: `Xs` for <60s, `Xm Ys` for >=60s
- [ ] Typecheck passes

### US-005: Implement breadcrumb trail for workflow journey
**Description:** As a user, I want to see a compact trail of states I've passed through so I understand the workflow progression.

**Acceptance Criteria:**
- [ ] Add `Breadcrumb` struct to track state history
- [ ] Trail displayed after each phase completion: `Journey: Story → Review → Correct → Review`
- [ ] Trail uses `→` separator and dim/gray color to not distract
- [ ] Trail resets at start of each new story
- [ ] Completed states shown in green, current state in yellow, future states not shown
- [ ] Trail displayed on single line, truncated with `...` if too long for terminal
- [ ] Typecheck passes

### US-006: Update Runner to use unified display system
**Description:** As a developer, I need to refactor Runner to use the new unified display components.

**Acceptance Criteria:**
- [ ] Runner uses `print_phase_banner("RUNNING", Color::Cyan)` at story start
- [ ] Runner spinner/timer shows iteration context `[US-001 2/5]`
- [ ] Runner completion uses standardized outcome format
- [ ] Runner updates breadcrumb trail
- [ ] Existing verbose/preview mode behavior preserved
- [ ] Typecheck passes

### US-007: Update Reviewer to use unified display system
**Description:** As a developer, I need to refactor Reviewer to use the new unified display components.

**Acceptance Criteria:**
- [ ] Reviewer uses `print_phase_banner("REVIEWING", Color::Cyan)` at start
- [ ] Reviewer spinner/timer shows `[Review 1/3]` with elapsed time
- [ ] Reviewer completion shows: duration + outcome (pass/issues found count)
- [ ] Reviewer updates breadcrumb trail with "Review" state
- [ ] Works correctly in both verbose and preview modes
- [ ] Typecheck passes

### US-008: Update Corrector to use unified display system
**Description:** As a developer, I need to refactor Corrector to use the new unified display components.

**Acceptance Criteria:**
- [ ] Corrector uses `print_phase_banner("CORRECTING", Color::Yellow)` at start
- [ ] Corrector spinner/timer shows `[Correct 1/3]` with elapsed time
- [ ] Corrector completion shows: duration + "Issues addressed" outcome
- [ ] Corrector updates breadcrumb trail with "Correct" state
- [ ] Works correctly in both verbose and preview modes
- [ ] Typecheck passes

### US-009: Update Commit phase to use unified display system
**Description:** As a developer, I need to refactor Commit phase to use the new unified display components.

**Acceptance Criteria:**
- [ ] Commit uses `print_phase_banner("COMMITTING", Color::Cyan)` at start
- [ ] Commit spinner/timer shows `[Commit]` with elapsed time
- [ ] Commit completion shows: duration + short commit hash as outcome
- [ ] Commit updates breadcrumb trail with "Commit" state
- [ ] Works correctly in both verbose and preview modes
- [ ] Typecheck passes

### US-010: Add overall progress context to displays
**Description:** As a user, I want to see overall progress (e.g., "Story 2/5") alongside the current operation.

**Acceptance Criteria:**
- [ ] Add `ProgressContext` struct holding: current story index, total stories, current phase
- [ ] Context passed to all display components
- [ ] During review/correct, show both story progress and iteration: `[US-001 2/5 | Review 1/3]`
- [ ] Progress context visible in both verbose timer output and spinner display
- [ ] Typecheck passes

## Functional Requirements

- FR-1: All agent phases (Runner, Reviewer, Corrector, Commit) must use the same display components and formatting
- FR-2: State transitions must be marked with color-coded single-line banners using box-drawing characters
- FR-3: All running agents must display: elapsed time (updating), current activity preview, iteration context
- FR-4: All agent completions must display: pass/fail indicator, duration, brief outcome description
- FR-5: A breadcrumb trail must show the journey through states, updated after each phase
- FR-6: Duration must be formatted consistently: seconds only for <60s, minutes and seconds for >=60s
- FR-7: Display must work correctly in both verbose mode (full output) and preview mode (single-line spinner)
- FR-8: Terminal width must be respected; long content must be truncated appropriately
- FR-9: Color scheme must be consistent: cyan for starting, green for success, red for failure, yellow for in-progress/warning

## Non-Goals

- No changes to the actual agent logic or Claude prompts
- No changes to the state machine or state persistence
- No audio or system notifications
- No interactive UI elements (remains a passive display)
- No configuration options for display preferences (keep it simple)
- No changes to the JSON output format or logging

## Technical Considerations

- **Existing Infrastructure:** Build on existing `VerboseTimer`, `ClaudeSpinner`, and `output.rs` utilities
- **Thread Safety:** Breadcrumb and progress context must be thread-safe (`Arc<Mutex<>>`) as spinners run on background threads
- **Terminal Compatibility:** Use only ANSI codes already proven to work in the codebase
- **Backwards Compatibility:** Preserve existing `--verbose` flag behavior
- **Box-Drawing Characters:** Use Unicode box-drawing (`━`, `┃`, `┏`, `┓`) that render well in most terminals

## Success Metrics

- All four agent phases display with identical formatting patterns
- State transitions are immediately visible without reading logs
- User can identify current phase, iteration, and progress at a glance
- Completion messages consistently show duration and outcome
- Breadcrumb trail accurately reflects the journey through states

## Open Questions

- Should the breadcrumb trail persist across stories, showing the entire run history, or reset per story?
- Should there be a summary breadcrumb at the very end showing all stories' journeys?
- For very long review/correct cycles (multiple iterations), should the trail collapse repeated states (e.g., `Review ×2 → Correct ×2`)?
