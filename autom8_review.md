# Review Issues (Iteration 1/3)

## Critical

None identified.

## Significant

- [ ] **Flaky tests due to missing CWD_MUTEX lock** - Multiple tests in `src/state.rs` call `StateManager::new()` without acquiring the `CWD_MUTEX` lock. Since `StateManager::new()` internally calls `worktree::get_current_session_id()` which runs git commands dependent on the current working directory, these tests can fail intermittently when run in parallel with other tests that modify CWD.

  **Affected tests (src/state.rs):**
  - `test_state_manager_new_uses_config_directory` (line ~1492)
  - `test_state_manager_state_file_in_config_directory` (line ~1522)
  - `test_state_manager_list_specs_uses_config_directory` (line ~1547)
  - `test_clean_uses_config_directory_not_legacy_location` (line ~1660)

  **Fix:** Add `let _lock = CWD_MUTEX.lock().unwrap();` at the start of each test, following the pattern used by other tests in the same file (e.g., `test_capture_pre_story_state_in_git_repo` at line 2030).

## Minor (iteration 1 only)

None identified.

## Test Failures

Tests are flaky due to the issue described above. When run in parallel, different tests fail on different runs:
- `config::tests::test_list_projects_tree_real_config` - panics when CWD is changed by another test
- `state::tests::test_state_manager_list_specs_uses_config_directory` - fails when git can't read CWD

Running with `cargo test -- --test-threads=1` or running individual tests in isolation passes consistently.

## Typecheck/Lint Errors

None - `cargo check`, `cargo clippy`, and `cargo fmt --check` all pass.
