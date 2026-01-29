# Review Issues (Iteration 1/3)

## Critical

- [ ] **Test isolation bug causing flaky tests**: Tests that depend on the current working directory being a git repository (`test_capture_pre_story_state_in_git_repo`, `test_capture_and_record_workflow`, `test_capture_pre_story_state_sets_baseline_commit_on_first_call`, `test_capture_pre_story_state_preserves_baseline_on_subsequent_calls`, `test_capture_pre_story_state_baseline_persists_through_multiple_stories`) do NOT acquire the `CWD_MUTEX`, but other tests (`test_system_works_in_non_git_directory`, `test_capture_pre_story_state_no_op_in_non_git`, `test_capture_story_knowledge_uses_agent_context_only_in_non_git`) DO acquire the mutex and change the working directory to a non-git temp directory. When tests run in parallel, the git-dependent tests can fail because they're suddenly running in a non-git directory. **Location:** `src/state.rs:1517-1530` (and similar tests at lines 2072-2128)

## Test Failures

- [ ] `git::tests::test_stage_all_changes_does_not_error` - Fails when running in parallel with tests that change cwd
- [ ] `state::tests::test_capture_and_record_workflow` - Assertion failed: `captured_commit.is_some()` at `src/state.rs:1622`
- [ ] `state::tests::test_capture_pre_story_state_in_git_repo` - Assertion failed: `state.pre_story_commit.is_some()` at `src/state.rs:1525`
- [ ] `state::tests::test_capture_pre_story_state_baseline_persists_through_multiple_stories` - Assertion failed: `baseline.is_some()` at `src/state.rs:2117`
- [ ] `state::tests::test_capture_pre_story_state_sets_baseline_commit_on_first_call` - Assertion failed at `src/state.rs:2082`
- [ ] `state::tests::test_capture_pre_story_state_preserves_baseline_on_subsequent_calls` - Assertion failed: `baseline.is_some()` at `src/state.rs:2097`

**Note:** All tests pass when run with `--test-threads=1`, confirming this is a test isolation issue, not a logic bug.

## Significant

(None identified - the implementation logic appears correct)

## Minor (iteration 1 only)

- [ ] **Duplicate type definitions**: There are two `Decision` types (`claude::utils::Decision` and `knowledge::Decision`) and two `Pattern` types (`claude::utils::Pattern` and `knowledge::Pattern`). While the conversion is handled correctly in `capture_story_knowledge()`, this duplication could be confusing. Consider using the same types or making the relationship more explicit. **Location:** `src/claude/utils.rs:36-52` and `src/knowledge.rs:113-142`

## Typecheck/Lint Errors

(None - `cargo clippy` and `cargo fmt --check` pass)
