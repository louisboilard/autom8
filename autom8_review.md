# Review Issues (Iteration 2/3)

## Critical

None identified.

## Significant

- [x] FIXED: **Flaky tests in config.rs due to missing CWD_MUTEX lock** - Several tests in `src/config.rs` call functions that internally use `StateManager::for_project()`, which calls `worktree::get_current_session_id()`. This function depends on the current working directory. Without acquiring `CWD_MUTEX`, these tests can fail intermittently when run in parallel with other tests that modify CWD.

  **Observed failure:**
  ```
  thread 'config::tests::test_list_projects_tree_real_config' panicked at src/config.rs:1974:9:
  list_projects_tree() should not error
  ```

  **Affected tests (src/config.rs):**
  - `test_list_projects_tree_real_config` (line 1971) - calls `list_projects_tree()`
  - `test_global_status_real_config` (line 1840) - calls `global_status()`
  - `test_us008_get_project_description_existing_project` (line 1997) - calls `get_project_description("autom8")`
  - `test_us008_project_description_has_all_fields` (line 2022) - calls `get_project_description("autom8")`
  - `test_us008_project_description_counts_spec_md_files` (line 2082) - calls `get_project_description("autom8")`
  - `test_us008_project_description_counts_archived_runs` (line 2092) - calls `get_project_description("autom8")`
  - `test_us008_project_description_run_state_fields` (line 2101) - calls `get_project_description("autom8")`

  **Fix:** Added `use crate::test_utils::CWD_MUTEX;` to the config.rs tests module, and added `let _lock = CWD_MUTEX.lock().unwrap();` at the start of each affected test, following the pattern used in state.rs and worktree.rs tests.

## Test Failures

- [x] FIXED: Intermittent failure in `test_list_projects_tree_real_config` due to missing CWD_MUTEX (see above)

## Typecheck/Lint Errors

None - `cargo check`, `cargo clippy`, and `cargo fmt --check` all pass.
