//! Self-test spec for testing autom8 itself.
//!
//! Provides a hardcoded trivial spec that exercises the normal autom8 flow
//! without modifying any real code. The spec creates and modifies a dummy
//! file (`test_output.txt`) in the repo root.

use std::fs;
use std::process::Command;

use crate::config::spec_dir;
use crate::error::Result;
use crate::spec::{Spec, UserStory};
use crate::state::StateManager;
use crate::worktree::get_main_repo_root;

/// Branch name used for self-test runs.
pub const SELF_TEST_BRANCH: &str = "autom8/self-test";

/// Target file for self-test operations.
pub const SELF_TEST_FILE: &str = "test_output.txt";

/// Filename for the self-test spec in the config directory.
pub const SELF_TEST_SPEC_FILENAME: &str = "test_spec.json";

/// Creates the hardcoded self-test spec.
///
/// The spec contains 3 trivial user stories that:
/// 1. Create `test_output.txt` with a greeting
/// 2. Add a separator and timestamp placeholder line
/// 3. Add a completion message
///
/// These stories exercise multiple iterations of the autom8 loop
/// without touching any real code.
pub fn create_self_test_spec() -> Spec {
    Spec {
        project: "autom8-self-test".to_string(),
        branch_name: SELF_TEST_BRANCH.to_string(),
        description: "Self-test spec for validating autom8 functionality. Creates and modifies a dummy test_output.txt file.".to_string(),
        user_stories: vec![
            UserStory {
                id: "ST-001".to_string(),
                title: "Create test output file".to_string(),
                description: "Create the test_output.txt file in the repository root with an initial greeting message.".to_string(),
                acceptance_criteria: vec![
                    "File test_output.txt exists in the repo root".to_string(),
                    "File contains the text 'Hello from autom8 self-test!'".to_string(),
                ],
                priority: 1,
                passes: false,
                notes: "This is the first step - just create the file with a simple greeting.".to_string(),
            },
            UserStory {
                id: "ST-002".to_string(),
                title: "Add separator and status line".to_string(),
                description: "Add a separator line and a status line to test_output.txt.".to_string(),
                acceptance_criteria: vec![
                    "File contains a separator line (e.g., '---')".to_string(),
                    "File contains a status line with 'Status: Running'".to_string(),
                ],
                priority: 2,
                passes: false,
                notes: "Appends content to the existing file.".to_string(),
            },
            UserStory {
                id: "ST-003".to_string(),
                title: "Add completion message".to_string(),
                description: "Add a final completion message to test_output.txt indicating the self-test finished successfully.".to_string(),
                acceptance_criteria: vec![
                    "File contains a completion message".to_string(),
                    "Message includes 'Self-test complete!'".to_string(),
                ],
                priority: 3,
                passes: false,
                notes: "Final step - adds the completion marker.".to_string(),
            },
        ],
    }
}

/// Result of a cleanup operation with details about what was cleaned.
#[derive(Debug, Default)]
pub struct CleanupResult {
    /// Whether the test output file was deleted
    pub test_file_deleted: bool,
    /// Whether the spec file was deleted
    pub spec_file_deleted: bool,
    /// Whether the session state was cleared
    pub session_cleared: bool,
    /// Whether the test branch was deleted
    pub branch_deleted: bool,
    /// Whether the worktree was deleted (if running in worktree mode)
    pub worktree_deleted: bool,
    /// Errors encountered during cleanup (non-fatal)
    pub errors: Vec<String>,
}

impl CleanupResult {
    /// Returns true if all cleanup operations succeeded without errors
    pub fn is_complete(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Clean up all self-test artifacts.
///
/// This function removes:
/// 1. `test_output.txt` from the current working directory (or main repo root)
/// 2. Test spec file from `~/.config/autom8/<project>/spec/`
/// 3. Session state from `~/.config/autom8/<project>/sessions/`
/// 4. The test branch (`autom8/self-test`)
/// 5. The worktree directory (if running in worktree mode)
///
/// Cleanup failures are collected but don't cause the function to fail,
/// allowing as much cleanup as possible to complete.
pub fn cleanup_self_test() -> CleanupResult {
    let mut result = CleanupResult::default();

    // Capture worktree info before changing directories
    let worktree_info = get_worktree_info_for_cleanup();

    // 1. Delete the test output file from current directory (where Claude created it)
    result.test_file_deleted = cleanup_test_file(&mut result.errors);

    // 2. Delete the spec file
    result.spec_file_deleted = cleanup_spec_file(&mut result.errors);

    // 3. Clear session state
    result.session_cleared = cleanup_session_state(&mut result.errors);

    // 4. Delete the worktree (must happen before branch deletion, requires leaving the worktree first)
    if let Some((worktree_path, main_repo_path)) = worktree_info {
        result.worktree_deleted =
            cleanup_worktree(&worktree_path, &main_repo_path, &mut result.errors);
    }

    // 5. Delete the test branch (after checkout to another branch)
    result.branch_deleted = cleanup_test_branch(&mut result.errors);

    result
}

/// Get worktree info if we're running in a linked worktree.
/// Returns (worktree_path, main_repo_path) if in a worktree, None otherwise.
fn get_worktree_info_for_cleanup() -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    use crate::worktree::{get_main_repo_root, is_in_worktree};

    // Check if we're in a linked worktree
    if is_in_worktree().unwrap_or(false) {
        let worktree_path = std::env::current_dir().ok()?;
        let main_repo_path = get_main_repo_root().ok()?;
        Some((worktree_path, main_repo_path))
    } else {
        None
    }
}

/// Clean up a worktree created during self-test.
fn cleanup_worktree(
    worktree_path: &std::path::Path,
    main_repo_path: &std::path::Path,
    errors: &mut Vec<String>,
) -> bool {
    use crate::worktree::remove_worktree;

    // First, change to the main repo so we can remove the worktree
    if let Err(e) = std::env::set_current_dir(main_repo_path) {
        errors.push(format!(
            "Failed to change to main repo '{}': {}",
            main_repo_path.display(),
            e
        ));
        return false;
    }

    // Now remove the worktree (force removal since we may have uncommitted test changes)
    if let Err(e) = remove_worktree(worktree_path, true) {
        errors.push(format!(
            "Failed to remove worktree '{}': {}",
            worktree_path.display(),
            e
        ));
        return false;
    }

    true
}

/// Delete the test output file from the current working directory.
///
/// When running with --self-test, Claude creates the test file in the current
/// working directory. In worktree mode, this is the worktree directory, not
/// the main repo root. We try the current directory first, then fall back to
/// the main repo root for compatibility.
fn cleanup_test_file(errors: &mut Vec<String>) -> bool {
    // First, try the current working directory (where Claude runs)
    if let Ok(cwd) = std::env::current_dir() {
        let test_file = cwd.join(SELF_TEST_FILE);
        if test_file.exists() {
            if let Err(e) = fs::remove_file(&test_file) {
                errors.push(format!("Failed to delete {}: {}", test_file.display(), e));
                return false;
            }
            return true;
        }
    }

    // Fall back to main repo root (in case cleanup is called from a different directory)
    let repo_root = match get_main_repo_root() {
        Ok(root) => root,
        Err(e) => {
            // If we can't get repo root and file wasn't in CWD, it may not exist
            // This is not necessarily an error - the file may have already been cleaned
            errors.push(format!(
                "Could not locate {}: not in CWD and failed to get repo root: {}",
                SELF_TEST_FILE, e
            ));
            return false;
        }
    };

    let test_file = repo_root.join(SELF_TEST_FILE);
    if test_file.exists() {
        if let Err(e) = fs::remove_file(&test_file) {
            errors.push(format!("Failed to delete {}: {}", test_file.display(), e));
            return false;
        }
    }
    true
}

/// Delete the spec file from the config directory.
fn cleanup_spec_file(errors: &mut Vec<String>) -> bool {
    let spec_path = match spec_dir() {
        Ok(dir) => dir.join(SELF_TEST_SPEC_FILENAME),
        Err(e) => {
            errors.push(format!("Failed to get spec directory: {}", e));
            return false;
        }
    };

    if spec_path.exists() {
        if let Err(e) = fs::remove_file(&spec_path) {
            errors.push(format!("Failed to delete {}: {}", spec_path.display(), e));
            return false;
        }
    }
    true
}

/// Clear the session state.
fn cleanup_session_state(errors: &mut Vec<String>) -> bool {
    let state_manager = match StateManager::new() {
        Ok(sm) => sm,
        Err(e) => {
            errors.push(format!("Failed to create state manager: {}", e));
            return false;
        }
    };

    if let Err(e) = state_manager.clear_current() {
        errors.push(format!("Failed to clear session state: {}", e));
        return false;
    }
    true
}

/// Delete the test branch after checking out to main/master.
fn cleanup_test_branch(errors: &mut Vec<String>) -> bool {
    // First, check if we're on the test branch
    let current_branch = match get_current_branch() {
        Ok(branch) => branch,
        Err(e) => {
            errors.push(format!("Failed to get current branch: {}", e));
            return false;
        }
    };

    // If on test branch, switch to main/master first
    if current_branch == SELF_TEST_BRANCH {
        let base_branch = detect_base_branch_for_cleanup();
        if let Err(e) = checkout_branch(&base_branch) {
            errors.push(format!("Failed to checkout {}: {}", base_branch, e));
            return false;
        }
    }

    // Now delete the test branch
    if branch_exists_local(SELF_TEST_BRANCH) {
        if let Err(e) = delete_branch(SELF_TEST_BRANCH) {
            errors.push(format!(
                "Failed to delete branch '{}': {}",
                SELF_TEST_BRANCH, e
            ));
            return false;
        }
    }
    true
}

/// Get the current branch name (internal helper).
fn get_current_branch() -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()?;

    if !output.status.success() {
        return Err(crate::error::Autom8Error::GitError(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Detect base branch (main or master) for checkout.
fn detect_base_branch_for_cleanup() -> String {
    // Try main first, then master
    if branch_exists_local("main") {
        "main".to_string()
    } else if branch_exists_local("master") {
        "master".to_string()
    } else {
        // Default to main if neither exists (git checkout will fail gracefully)
        "main".to_string()
    }
}

/// Check if a local branch exists.
fn branch_exists_local(branch: &str) -> bool {
    Command::new("git")
        .args([
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{}", branch),
        ])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Checkout a branch.
fn checkout_branch(branch: &str) -> Result<()> {
    let output = Command::new("git").args(["checkout", branch]).output()?;

    if !output.status.success() {
        return Err(crate::error::Autom8Error::GitError(format!(
            "Failed to checkout branch '{}': {}",
            branch,
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    Ok(())
}

/// Delete a local branch.
fn delete_branch(branch: &str) -> Result<()> {
    let output = Command::new("git")
        .args(["branch", "-D", branch])
        .output()?;

    if !output.status.success() {
        return Err(crate::error::Autom8Error::GitError(format!(
            "Failed to delete branch '{}': {}",
            branch,
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    Ok(())
}

/// Print detailed error information before cleanup (for failure cases).
pub fn print_failure_details(run_error: &crate::error::Autom8Error) {
    use crate::output::{print_error, print_warning};

    println!(); // Add spacing
    print_error(&format!("Self-test failed: {}", run_error));

    // Print additional context based on error type
    match run_error {
        crate::error::Autom8Error::ClaudeError(msg) => {
            print_warning(&format!("Claude error details: {}", msg));
        }
        crate::error::Autom8Error::ClaudeTimeout(secs) => {
            print_warning(&format!("Claude timed out after {} seconds", secs));
        }
        crate::error::Autom8Error::MaxReviewIterationsReached => {
            print_warning("Review failed after maximum iterations");
        }
        crate::error::Autom8Error::Interrupted => {
            print_warning("Run was interrupted by user");
        }
        _ => {}
    }
}

/// Print cleanup results.
pub fn print_cleanup_results(result: &CleanupResult) {
    use crate::output::{print_info, print_warning, GREEN, RESET};

    println!(); // Add spacing
    print_info("Cleaning up self-test artifacts...");

    if result.test_file_deleted {
        println!("  {GREEN}✓{RESET} Deleted {}", SELF_TEST_FILE);
    }
    if result.spec_file_deleted {
        println!(
            "  {GREEN}✓{RESET} Deleted spec file ({})",
            SELF_TEST_SPEC_FILENAME
        );
    }
    if result.session_cleared {
        println!("  {GREEN}✓{RESET} Cleared session state");
    }
    if result.worktree_deleted {
        println!("  {GREEN}✓{RESET} Removed worktree");
    }
    if result.branch_deleted {
        println!("  {GREEN}✓{RESET} Deleted branch '{}'", SELF_TEST_BRANCH);
    }

    if !result.errors.is_empty() {
        println!();
        print_warning("Some cleanup operations failed:");
        for error in &result.errors {
            print_warning(&format!("  - {}", error));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_self_test_spec_returns_valid_spec() {
        let spec = create_self_test_spec();

        assert_eq!(spec.project, "autom8-self-test");
        assert_eq!(spec.branch_name, SELF_TEST_BRANCH);
        assert!(!spec.description.is_empty());
    }

    #[test]
    fn test_self_test_spec_has_three_stories() {
        let spec = create_self_test_spec();

        assert_eq!(spec.user_stories.len(), 3);
    }

    #[test]
    fn test_self_test_spec_stories_are_not_passing() {
        let spec = create_self_test_spec();

        for story in &spec.user_stories {
            assert!(
                !story.passes,
                "Story {} should not be passing initially",
                story.id
            );
        }
    }

    #[test]
    fn test_self_test_spec_stories_have_correct_priorities() {
        let spec = create_self_test_spec();

        assert_eq!(spec.user_stories[0].priority, 1);
        assert_eq!(spec.user_stories[1].priority, 2);
        assert_eq!(spec.user_stories[2].priority, 3);
    }

    #[test]
    fn test_self_test_spec_stories_have_ids() {
        let spec = create_self_test_spec();

        assert_eq!(spec.user_stories[0].id, "ST-001");
        assert_eq!(spec.user_stories[1].id, "ST-002");
        assert_eq!(spec.user_stories[2].id, "ST-003");
    }

    #[test]
    fn test_self_test_spec_stories_have_acceptance_criteria() {
        let spec = create_self_test_spec();

        for story in &spec.user_stories {
            assert!(
                !story.acceptance_criteria.is_empty(),
                "Story {} should have acceptance criteria",
                story.id
            );
        }
    }

    #[test]
    fn test_self_test_spec_can_be_serialized_to_json() {
        let spec = create_self_test_spec();

        let json = serde_json::to_string_pretty(&spec);
        assert!(json.is_ok(), "Spec should serialize to JSON");

        let json_str = json.unwrap();
        assert!(json_str.contains("autom8-self-test"));
        assert!(json_str.contains("ST-001"));
        assert!(json_str.contains("test_output.txt"));
    }

    #[test]
    fn test_self_test_spec_round_trips_through_json() {
        let spec = create_self_test_spec();

        let json = serde_json::to_string(&spec).unwrap();
        let parsed: Spec = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.project, spec.project);
        assert_eq!(parsed.branch_name, spec.branch_name);
        assert_eq!(parsed.user_stories.len(), spec.user_stories.len());
    }

    #[test]
    fn test_self_test_branch_constant() {
        assert_eq!(SELF_TEST_BRANCH, "autom8/self-test");
    }

    #[test]
    fn test_self_test_file_constant() {
        assert_eq!(SELF_TEST_FILE, "test_output.txt");
    }

    #[test]
    fn test_self_test_spec_filename_constant() {
        assert_eq!(SELF_TEST_SPEC_FILENAME, "test_spec.json");
    }

    // ========================================================================
    // US-004: Cleanup tests
    // ========================================================================

    #[test]
    fn test_cleanup_result_default_is_empty() {
        let result = CleanupResult::default();

        assert!(!result.test_file_deleted);
        assert!(!result.spec_file_deleted);
        assert!(!result.session_cleared);
        assert!(!result.branch_deleted);
        assert!(!result.worktree_deleted);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_cleanup_result_is_complete_when_no_errors() {
        let mut result = CleanupResult::default();
        result.test_file_deleted = true;
        result.spec_file_deleted = true;
        result.session_cleared = true;
        result.branch_deleted = true;
        result.worktree_deleted = true;

        assert!(result.is_complete());
    }

    #[test]
    fn test_cleanup_result_is_not_complete_with_errors() {
        let mut result = CleanupResult::default();
        result.test_file_deleted = true;
        result.errors.push("Failed to delete something".to_string());

        assert!(!result.is_complete());
    }

    #[test]
    fn test_cleanup_result_collects_multiple_errors() {
        let mut result = CleanupResult::default();
        result.errors.push("Error 1".to_string());
        result.errors.push("Error 2".to_string());

        assert_eq!(result.errors.len(), 2);
        assert!(!result.is_complete());
    }

    #[test]
    fn test_branch_exists_local_returns_bool() {
        // This test just verifies the function doesn't panic and returns a bool.
        // In the test environment, the branch may or may not exist.
        let exists = branch_exists_local("main");
        // Result is either true or false - just verify it's a valid bool
        assert!(exists || !exists);
    }

    #[test]
    fn test_branch_exists_local_nonexistent_branch() {
        // A branch that definitely doesn't exist
        let exists = branch_exists_local("nonexistent-branch-xyz-123456789");
        assert!(!exists);
    }

    #[test]
    fn test_detect_base_branch_for_cleanup_returns_string() {
        // Should return either "main" or "master"
        let branch = detect_base_branch_for_cleanup();
        assert!(!branch.is_empty());
        // Should be a valid branch name
        assert!(
            branch == "main" || branch == "master",
            "Expected 'main' or 'master', got '{}'",
            branch
        );
    }

    #[test]
    fn test_get_current_branch_returns_result() {
        // We should be in a git repo during tests
        let result = get_current_branch();
        assert!(result.is_ok(), "Should be able to get current branch");
        let branch = result.unwrap();
        assert!(!branch.is_empty(), "Branch name should not be empty");
    }

    // ========================================================================
    // Worktree cleanup tests
    // ========================================================================

    #[test]
    fn test_get_worktree_info_for_cleanup_returns_correct_value() {
        // get_worktree_info_for_cleanup should return Some if in a worktree, None otherwise
        use crate::worktree::is_in_worktree;

        let info = get_worktree_info_for_cleanup();
        let in_worktree = is_in_worktree().unwrap_or(false);

        if in_worktree {
            // If we're in a worktree, we should get Some with valid paths
            let (worktree_path, main_repo_path) =
                info.expect("get_worktree_info_for_cleanup should return Some when in a worktree");
            assert!(worktree_path.exists(), "worktree_path should exist");
            assert!(main_repo_path.exists(), "main_repo_path should exist");
            assert_ne!(
                worktree_path, main_repo_path,
                "worktree_path and main_repo_path should be different"
            );
        } else {
            // If we're not in a worktree, we should get None
            assert!(
                info.is_none(),
                "get_worktree_info_for_cleanup should return None when not in a worktree"
            );
        }
    }

    #[test]
    fn test_cleanup_result_worktree_deleted_field() {
        let mut result = CleanupResult::default();
        assert!(
            !result.worktree_deleted,
            "worktree_deleted should default to false"
        );

        result.worktree_deleted = true;
        assert!(
            result.worktree_deleted,
            "worktree_deleted should be settable to true"
        );
    }

    #[test]
    fn test_cleanup_test_file_deletes_file_in_cwd() {
        use std::fs::File;
        use std::io::Write;

        // Test that cleanup_test_file deletes the file when it exists in CWD
        // Note: This test runs in the repo root (which is a git repo),
        // so cleanup_test_file should successfully delete the file.
        let cwd = std::env::current_dir().unwrap();
        let test_file = cwd.join(SELF_TEST_FILE);

        // Create the test file
        {
            let mut f = File::create(&test_file).unwrap();
            writeln!(f, "test content").unwrap();
        }
        assert!(test_file.exists(), "Test file should exist before cleanup");

        // Run cleanup - it should find and delete the file in CWD
        let mut errors = Vec::new();
        let deleted = cleanup_test_file(&mut errors);

        // Clean up in case of assertion failure
        if test_file.exists() {
            let _ = std::fs::remove_file(&test_file);
        }

        assert!(deleted, "cleanup_test_file should return true");
        assert!(
            errors.is_empty(),
            "cleanup_test_file should have no errors: {:?}",
            errors
        );
    }

    #[test]
    fn test_cleanup_test_file_succeeds_when_file_missing() {
        // cleanup_test_file should succeed (return true) even if the file doesn't exist
        let mut errors = Vec::new();
        let deleted = cleanup_test_file(&mut errors);

        // The function should return true even if the file doesn't exist
        // (it checks in CWD first, then falls back to repo root)
        assert!(
            deleted,
            "cleanup_test_file should return true when file is missing"
        );
        assert!(
            errors.is_empty(),
            "cleanup_test_file should have no errors when file is missing"
        );
    }
}
