# Review Issues (Iteration 1/3)

## Critical

- [x] FIXED: **US-003: Missing scrolling in detail view** - `src/monitor/app.rs:545-554`
  The acceptance criteria states "hjkl work for scrolling when viewing detailed output". However, when `show_run_detail` is true, the code only handles Esc/Enter keys and returns early, ignoring j/k for scrolling. The detail modal (`render_run_detail_modal`) also lacks scroll offset state and scrolling logic. Implementation needed:
  1. Add a `detail_scroll_offset: usize` field to `MonitorApp`
  2. Handle `KeyCode::Up | KeyCode::Char('k')` and `KeyCode::Down | KeyCode::Char('j')` in the detail view branch to adjust the scroll offset
  3. Apply the scroll offset in `render_run_detail_modal()` when rendering the paragraph content

  **Fix applied:**
  - Added `detail_scroll_offset: usize` field to `MonitorApp` struct
  - Added j/k and arrow key handling in detail view to adjust scroll offset
  - Applied scroll offset to Paragraph with `.scroll((detail_scroll_offset, 0))`
  - Reset scroll offset when opening/closing detail view
  - Updated footer help text to show scroll keys

## Significant

None found.

## Minor (iteration 1 only)

None found.

## Test Failures

None - all 811 tests pass.

## Typecheck/Lint Errors

None - `cargo check`, `cargo clippy -- -D warnings`, and `cargo fmt --check` all pass.
