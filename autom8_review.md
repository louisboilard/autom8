# Review Issues (Iteration 1/3)

## Critical

- [x] FIXED: **Test isolation bug causing flaky tests**: Tests that depend on the current working directory being a git repository (`test_capture_pre_story_state_in_git_repo`, `test_capture_and_record_workflow`, `test_capture_pre_story_state_sets_baseline_commit_on_first_call`, `test_capture_pre_story_state_preserves_baseline_on_subsequent_calls`, `test_capture_pre_story_state_baseline_persists_through_multiple_stories`) do NOT acquire the `CWD_MUTEX`, but other tests (`test_system_works_in_non_git_directory`, `test_capture_pre_story_state_no_op_in_non_git`, `test_capture_story_knowledge_uses_agent_context_only_in_non_git`) DO acquire the mutex and change the working directory to a non-git temp directory. When tests run in parallel, the git-dependent tests can fail because they're suddenly running in a non-git directory. **Location:** `src/state.rs:1517-1530` (and similar tests at lines 2072-2128)

  **Fix applied:** Created a shared `CWD_MUTEX` in a new `src/test_utils.rs` module that is used by both `src/state.rs` and `src/git.rs` tests. All tests that either depend on being in a git repo or change the cwd now acquire this shared mutex. The fix was verified by running tests multiple times with parallel execution - all tests now pass consistently.

## Test Failures

- [x] FIXED: `git::tests::test_stage_all_changes_does_not_error` - Fails when running in parallel with tests that change cwd
- [x] FIXED: `state::tests::test_capture_and_record_workflow` - Assertion failed: `captured_commit.is_some()` at `src/state.rs:1622`
- [x] FIXED: `state::tests::test_capture_pre_story_state_in_git_repo` - Assertion failed: `state.pre_story_commit.is_some()` at `src/state.rs:1525`
- [x] FIXED: `state::tests::test_capture_pre_story_state_baseline_persists_through_multiple_stories` - Assertion failed: `baseline.is_some()` at `src/state.rs:2117`
- [x] FIXED: `state::tests::test_capture_pre_story_state_sets_baseline_commit_on_first_call` - Assertion failed at `src/state.rs:2082`
- [x] FIXED: `state::tests::test_capture_pre_story_state_preserves_baseline_on_subsequent_calls` - Assertion failed: `baseline.is_some()` at `src/state.rs:2097`

**Note:** All tests pass when run with `--test-threads=1`, confirming this is a test isolation issue, not a logic bug.

## Significant

(None identified - the implementation logic appears correct)

## Minor (iteration 1 only)

- [ ] SKIPPED: **Duplicate type definitions**: There are two `Decision` types (`claude::utils::Decision` and `knowledge::Decision`) and two `Pattern` types (`claude::utils::Pattern` and `knowledge::Pattern`). While the conversion is handled correctly in `capture_story_knowledge()`, this duplication could be confusing. Consider using the same types or making the relationship more explicit. **Location:** `src/claude/utils.rs:36-52` and `src/knowledge.rs:113-142`

  **Reason for skipping:** This is a minor code organization issue, not a bug. The current approach of having separate types for parsing (utils) and storage (knowledge) with explicit conversion is actually a reasonable pattern. Refactoring this now would require changes across multiple modules and could introduce regressions without adding functional value. This can be addressed in a future cleanup PR if desired.

## Typecheck/Lint Errors

(None - `cargo clippy` and `cargo fmt --check` pass)
