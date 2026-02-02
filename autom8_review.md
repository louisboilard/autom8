# Review Issues (Iteration 1/3)

## Critical

None identified.

## Significant

- [x] FIXED: **Flaky tests due to missing CWD_MUTEX lock** - Multiple tests in `src/state.rs` call `StateManager::new()` without acquiring the `CWD_MUTEX` lock. Since `StateManager::new()` internally calls `worktree::get_current_session_id()` which runs git commands dependent on the current working directory, these tests can fail intermittently when run in parallel with other tests that modify CWD.

  **Affected tests (src/state.rs):**
  - `test_state_manager_new_uses_config_directory` (line ~1492)
  - `test_state_manager_state_file_in_config_directory` (line ~1522)
  - `test_state_manager_list_specs_uses_config_directory` (line ~1547)
  - `test_clean_uses_config_directory_not_legacy_location` (line ~1660)

  **Fix:** Added `let _lock = CWD_MUTEX.lock().unwrap();` at the start of each test, following the pattern used by other tests in the same file (e.g., `test_capture_pre_story_state_in_git_repo` at line 2030).

## Minor (iteration 1 only)

None identified.

## Test Failures

- [x] FIXED: Tests are no longer flaky. All tests pass with `cargo test --all-features`. The fix for the CWD_MUTEX issue above resolves the intermittent failures.

## Typecheck/Lint Errors

None - `cargo check`, `cargo clippy`, and `cargo fmt --check` all pass.
