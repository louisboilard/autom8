# PRD: Reviewer Agent

## Introduction

Add a review loop between story completion and commit to ensure code quality before committing. When all stories pass, a **Reviewer Agent** inspects the changes for bugs, missing tests, and code quality issues. If issues are found, a **Corrector Agent** addresses them. This loop repeats up to 3 times until the review passes or the run fails.

This introduces two new agents and a new state machine loop:
```
[All Stories Complete] → Reviewing → (issues found?)
                              ↓ yes              ↓ no
                         Correcting          Committing
                              ↓
                         Reviewing (iteration 2/3)
                              ↓
                         ... up to 3 iterations ...
                              ↓
                         (still failing?) → FAIL
```

## Goals

- Catch bugs, missing tests, and code quality issues before committing
- Ensure consistency with existing codebase patterns
- Provide actionable feedback via `autom8_review.md`
- Limit review loop to 3 iterations to prevent infinite loops
- Make reviewer progressively less strict on each iteration (focus on critical issues only)
- Never commit the review file (`autom8_review.md`)

## User Stories

### US-001: Add new state machine states for review loop
**Description:** As a developer, I need new states to track the review/correction cycle so the runner can orchestrate the review loop.

**Acceptance Criteria:**
- [ ] Add `Reviewing` state to `MachineState` enum
- [ ] Add `Correcting` state to `MachineState` enum
- [ ] Add `review_iteration: u32` field to `RunState` to track current review cycle (1, 2, or 3)
- [ ] State transitions: `PickingStory` (all complete) → `Reviewing` → `Correcting` → `Reviewing` ... → `Committing`
- [ ] Typecheck passes

### US-002: Create reviewer agent prompt
**Description:** As a developer, I need a prompt template for the reviewer agent that instructs Claude to review changes and write issues to a file.

**Acceptance Criteria:**
- [ ] Add `REVIEWER_PROMPT` constant to `prompts.rs`
- [ ] Prompt includes: project name, feature description, full PRD context (all stories)
- [ ] Prompt includes current iteration number and max iterations (e.g., "Review iteration 1/3")
- [ ] Prompt explicitly states: "You have a maximum of 3 review cycles. Focus on critical issues, not nitpicks."
- [ ] Prompt instructs: iteration 1 = thorough review; iteration 2 = only significant issues; iteration 3 = only blocking bugs
- [ ] Prompt instructs Claude to write issues to `autom8_review.md` (overwrite if exists)
- [ ] Prompt instructs Claude to output nothing to the file if all checks pass (delete file if it exists)
- [ ] Review criteria: bugs, missing tests, code quality, pattern consistency, no needless repetition
- [ ] Prompt instructs Claude to explicitly run the project's test suite if tests exist
- [ ] Prompt instructs Claude to run typecheck/lint commands if available (e.g., `cargo check`, `npm run typecheck`)
- [ ] Typecheck passes

### US-003: Create corrector agent prompt
**Description:** As a developer, I need a prompt template for the corrector agent that reads the review file and fixes the issues.

**Acceptance Criteria:**
- [ ] Add `CORRECTOR_PROMPT` constant to `prompts.rs`
- [ ] Prompt includes: project name, feature description, full PRD context
- [ ] Prompt instructs Claude to read `autom8_review.md` for issues to fix
- [ ] Prompt instructs Claude to fix issues it agrees with (use judgment)
- [ ] Prompt instructs Claude to annotate fixed items with "FIXED:" prefix in `autom8_review.md`
- [ ] Prompt reminds Claude this is iteration X/3, focus on the most critical fixes first
- [ ] Typecheck passes

### US-004: Implement reviewer agent runner function
**Description:** As a developer, I need a function to spawn the reviewer agent and detect whether issues were found.

**Acceptance Criteria:**
- [ ] Add `run_reviewer()` function to `claude.rs`
- [ ] Function accepts: `prd: &Prd`, `iteration: u32`, `max_iterations: u32`, `callback`
- [ ] Spawns Claude with `REVIEWER_PROMPT` (filled with context)
- [ ] Returns `ReviewResult::Pass` if `autom8_review.md` does not exist after run
- [ ] Returns `ReviewResult::IssuesFound` if `autom8_review.md` exists and has content
- [ ] Returns `ReviewResult::Error(String)` on failure
- [ ] Typecheck passes

### US-005: Implement corrector agent runner function
**Description:** As a developer, I need a function to spawn the corrector agent to fix review issues.

**Acceptance Criteria:**
- [ ] Add `run_corrector()` function to `claude.rs`
- [ ] Function accepts: `prd: &Prd`, `iteration: u32`, `callback`
- [ ] Spawns Claude with `CORRECTOR_PROMPT` (filled with context)
- [ ] Returns `CorrectorResult::Complete` when Claude finishes
- [ ] Returns `CorrectorResult::Error(String)` on failure
- [ ] Typecheck passes

### US-006: Integrate review loop into runner
**Description:** As a developer, I need the main runner to orchestrate the review/correction loop before committing.

**Acceptance Criteria:**
- [ ] After all stories complete, transition to `Reviewing` state (not directly to `Committing`)
- [ ] Call `run_reviewer()` with current iteration (starting at 1)
- [ ] If `ReviewResult::Pass`: delete `autom8_review.md` if exists, transition to `Committing`
- [ ] If `ReviewResult::IssuesFound`: transition to `Correcting`, call `run_corrector()`
- [ ] After correction: increment `review_iteration`, transition back to `Reviewing`
- [ ] If `review_iteration > 3` and still issues: return `Autom8Error::MaxReviewIterationsReached`
- [ ] Typecheck passes

### US-007: Add autom8_review.md to commit exclusion list
**Description:** As a developer, I need to ensure the review file is never committed.

**Acceptance Criteria:**
- [ ] Add `autom8_review.md` to the exclusion list in `COMMIT_PROMPT`
- [ ] Verify commit agent is instructed to never stage or commit this file
- [ ] Typecheck passes

### US-008: Add MaxReviewIterationsReached error type
**Description:** As a developer, I need a specific error type for when the review loop exceeds max iterations.

**Acceptance Criteria:**
- [ ] Add `MaxReviewIterationsReached` variant to `Autom8Error` enum
- [ ] Error message: "Review failed after 3 iterations. Please manually review autom8_review.md for remaining issues."
- [ ] Typecheck passes

### US-009: Add review result types
**Description:** As a developer, I need result enums for the reviewer and corrector agents.

**Acceptance Criteria:**
- [ ] Add `ReviewResult` enum: `Pass`, `IssuesFound`, `Error(String)`
- [ ] Add `CorrectorResult` enum: `Complete`, `Error(String)`
- [ ] Place in `claude.rs` or a new `review.rs` module
- [ ] Typecheck passes

### US-010: Update progress/output for review states
**Description:** As a developer, I need the terminal output to show review progress clearly.

**Acceptance Criteria:**
- [ ] Display "Reviewing changes (iteration 1/3)..." when entering `Reviewing` state
- [ ] Display "Review passed! Proceeding to commit." on `ReviewResult::Pass`
- [ ] Display "Issues found. Running corrector (iteration 1/3)..." when entering `Correcting` state
- [ ] Display "Review failed after 3 iterations." on max iterations error
- [ ] Typecheck passes

### US-011: Add --skip-review CLI flag
**Description:** As a user, I want to bypass the review loop when I trust my changes or need to commit quickly.

**Acceptance Criteria:**
- [ ] Add `--skip-review` flag to the `run` command in `main.rs`
- [ ] When flag is set, skip `Reviewing` state entirely and go directly to `Committing`
- [ ] Display "Skipping review (--skip-review flag set)" in terminal output
- [ ] Flag has no effect on `resume` command (respects current state)
- [ ] Typecheck passes

## Functional Requirements

- FR-1: Add `Reviewing` and `Correcting` states to state machine
- FR-2: Track review iteration count (1-3) in `RunState`
- FR-3: Reviewer agent writes issues to `autom8_review.md`, overwrites if exists
- FR-4: Reviewer agent deletes/skips file creation if no issues found
- FR-5: Corrector agent reads `autom8_review.md` and fixes issues
- FR-6: Corrector agent annotates fixed items with "FIXED:" prefix
- FR-7: Review loop: Reviewing → Correcting → Reviewing (max 3 cycles)
- FR-8: If review passes (no file), proceed to Committing
- FR-9: If 3 iterations exhausted with issues remaining, fail with specific error
- FR-10: Never commit `autom8_review.md`
- FR-11: Reviewer prompt emphasizes: iteration 1 = thorough, iteration 2 = significant only, iteration 3 = blockers only
- FR-12: Both agents receive full PRD context (project, description, all stories)
- FR-13: Reviewer explicitly runs test suite if tests exist in the project
- FR-14: Reviewer runs typecheck/lint commands if available
- FR-15: `--skip-review` flag bypasses review loop entirely, going straight to commit

## Non-Goals

- No interactive user approval during review loop
- No partial commits (either all passes or nothing commits)
- No configurable max iterations (hardcoded to 3)
- No persistence of review history across runs

## Technical Considerations

- Review file location: project root `./autom8_review.md`
- File detection: use `std::path::Path::exists()` after reviewer completes
- Prompt structure: follow existing patterns in `prompts.rs`
- State persistence: `review_iteration` must be saved to `.autom8/state.json` for resume support
- The reviewer should be instructed to actually run tests/typecheck if applicable to the project

## Success Metrics

- Review loop completes in ≤3 iterations for most features
- Catches obvious bugs before commit
- Does not block on trivial style issues
- Clear error message when review fails after max iterations

## Open Questions

None - all questions resolved.
