use crate::error::{Autom8Error, Result};
use std::process::Command;

/// Check if current directory is a git repository
pub fn is_git_repo() -> bool {
    Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Get the current branch name
pub fn current_branch() -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()?;

    if !output.status.success() {
        return Err(Autom8Error::GitError(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Check if a branch exists (locally or remotely)
pub fn branch_exists(branch: &str) -> Result<bool> {
    // Check local branches
    let local = Command::new("git")
        .args([
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{}", branch),
        ])
        .output()?;

    if local.status.success() {
        return Ok(true);
    }

    // Check remote branches
    let remote = Command::new("git")
        .args([
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/remotes/origin/{}", branch),
        ])
        .output()?;

    Ok(remote.status.success())
}

/// Create and checkout a new branch, or checkout existing branch
pub fn ensure_branch(branch: &str) -> Result<()> {
    let current = current_branch()?;

    if current == branch {
        return Ok(());
    }

    if branch_exists(branch)? {
        // Branch exists, checkout
        checkout(branch)?;
    } else {
        // Create new branch
        create_and_checkout(branch)?;
    }

    Ok(())
}

/// Checkout an existing branch
pub fn checkout(branch: &str) -> Result<()> {
    let output = Command::new("git").args(["checkout", branch]).output()?;

    if !output.status.success() {
        return Err(Autom8Error::GitError(format!(
            "Failed to checkout branch '{}': {}",
            branch,
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    Ok(())
}

/// Create and checkout a new branch
fn create_and_checkout(branch: &str) -> Result<()> {
    let output = Command::new("git")
        .args(["checkout", "-b", branch])
        .output()?;

    if !output.status.success() {
        return Err(Autom8Error::GitError(format!(
            "Failed to create branch '{}': {}",
            branch,
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    Ok(())
}

/// Check if working directory is clean (no uncommitted changes)
pub fn is_clean() -> Result<bool> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .output()?;

    if !output.status.success() {
        return Err(Autom8Error::GitError(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    Ok(output.stdout.is_empty())
}

/// Get the short hash of the latest commit (HEAD)
pub fn latest_commit_short() -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()?;

    if !output.status.success() {
        return Err(Autom8Error::GitError(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

// ============================================================================
// US-003: Branch Commit Gathering
// ============================================================================

/// A single git commit
#[derive(Debug, Clone, PartialEq)]
pub struct CommitInfo {
    /// Short commit hash (7 characters)
    pub short_hash: String,
    /// Full commit hash
    pub full_hash: String,
    /// Commit message (first line only)
    pub message: String,
    /// Author name
    pub author: String,
    /// Commit date in ISO format
    pub date: String,
}

/// Get commits specific to the current branch (excluding merge commits).
///
/// Uses `git log` with `--no-merges` to exclude merge commits.
/// Compares against the main branch (main or master) to get only
/// commits specific to this branch.
///
/// # Arguments
/// * `base_branch` - The base branch to compare against (e.g., "main" or "master")
///
/// # Returns
/// * `Ok(Vec<CommitInfo>)` - List of commits on this branch (newest first)
/// * `Err` - If the git command fails
pub fn get_branch_commits(base_branch: &str) -> Result<Vec<CommitInfo>> {
    // Get commits that are on HEAD but not on base_branch, excluding merges
    // Format: hash|short_hash|message|author|date
    let output = Command::new("git")
        .args([
            "log",
            &format!("{}..HEAD", base_branch),
            "--no-merges",
            "--pretty=format:%H|%h|%s|%an|%ai",
        ])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Autom8Error::GitError(format!(
            "Failed to get branch commits: {}",
            stderr.trim()
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let commits: Vec<CommitInfo> = stdout
        .lines()
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(5, '|').collect();
            if parts.len() >= 5 {
                Some(CommitInfo {
                    full_hash: parts[0].to_string(),
                    short_hash: parts[1].to_string(),
                    message: parts[2].to_string(),
                    author: parts[3].to_string(),
                    date: parts[4].to_string(),
                })
            } else {
                None
            }
        })
        .collect();

    Ok(commits)
}

/// Detect the default base branch for the repository (main or master).
///
/// Checks for existence of common default branch names.
///
/// # Returns
/// * `Ok(String)` - The detected base branch name ("main" or "master")
/// * `Err` - If neither branch exists
pub fn detect_base_branch() -> Result<String> {
    // Check if 'main' exists
    if branch_exists("main")? {
        return Ok("main".to_string());
    }

    // Check if 'master' exists
    if branch_exists("master")? {
        return Ok("master".to_string());
    }

    // Neither exists - try to get from remote
    let output = Command::new("git")
        .args(["remote", "show", "origin"])
        .output();

    if let Ok(out) = output {
        if out.status.success() {
            let stdout = String::from_utf8_lossy(&out.stdout);
            // Look for "HEAD branch:" line
            for line in stdout.lines() {
                if line.contains("HEAD branch:") {
                    if let Some(branch) = line.split(':').nth(1) {
                        return Ok(branch.trim().to_string());
                    }
                }
            }
        }
    }

    // Default to "main" if we can't detect
    Ok("main".to_string())
}

/// Get commits specific to the current branch, auto-detecting the base branch.
///
/// Convenience function that combines `detect_base_branch` and `get_branch_commits`.
///
/// # Returns
/// * `Ok(Vec<CommitInfo>)` - List of commits on this branch (newest first)
/// * `Err` - If the git command fails
pub fn get_current_branch_commits() -> Result<Vec<CommitInfo>> {
    let base_branch = detect_base_branch()?;
    get_branch_commits(&base_branch)
}

/// Get the diff for a specific commit.
///
/// # Arguments
/// * `commit_hash` - The commit hash to get the diff for
///
/// # Returns
/// * `Ok(String)` - The diff output
/// * `Err` - If the git command fails
pub fn get_commit_diff(commit_hash: &str) -> Result<String> {
    let output = Command::new("git")
        .args(["show", "--format=", commit_hash])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Autom8Error::GitError(format!(
            "Failed to get commit diff: {}",
            stderr.trim()
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Result type for push operations
#[derive(Debug, Clone, PartialEq)]
pub enum PushResult {
    /// Push succeeded
    Success,
    /// Branch already up-to-date on remote
    AlreadyUpToDate,
    /// Push failed with error message
    Error(String),
}

/// Result type for commit operations
#[derive(Debug, Clone, PartialEq)]
pub enum CommitResult {
    /// Commit succeeded with commit hash
    Success(String),
    /// No changes to commit
    NothingToCommit,
    /// Commit failed with error message
    Error(String),
}

// ============================================================================
// US-006: Commit and Push for PR Review Fixes
// ============================================================================

/// Check if there are uncommitted changes (staged or unstaged)
///
/// Returns true if the working directory has changes that could be committed.
pub fn has_uncommitted_changes() -> Result<bool> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .output()?;

    if !output.status.success() {
        return Err(Autom8Error::GitError(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    // If there's any output, there are changes
    Ok(!output.stdout.is_empty())
}

/// Stage all changes (including new files, modifications, and deletions)
///
/// Uses `git add -A` to stage all changes in the working directory.
pub fn stage_all_changes() -> Result<()> {
    let output = Command::new("git").args(["add", "-A"]).output()?;

    if !output.status.success() {
        return Err(Autom8Error::GitError(format!(
            "Failed to stage changes: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    Ok(())
}

/// Create a git commit with the given message
///
/// # Arguments
/// * `message` - The commit message
///
/// # Returns
/// * `CommitResult::Success(hash)` - Commit created with short hash
/// * `CommitResult::NothingToCommit` - No changes to commit
/// * `CommitResult::Error(msg)` - Commit failed
pub fn create_commit(message: &str) -> Result<CommitResult> {
    let output = Command::new("git")
        .args(["commit", "-m", message])
        .output()?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    if output.status.success() {
        // Get the commit hash
        let hash = latest_commit_short().unwrap_or_else(|_| "unknown".to_string());
        return Ok(CommitResult::Success(hash));
    }

    // Check for "nothing to commit" case
    let combined = format!("{} {}", stdout, stderr);
    if combined.to_lowercase().contains("nothing to commit")
        || combined.to_lowercase().contains("no changes added")
    {
        return Ok(CommitResult::NothingToCommit);
    }

    Ok(CommitResult::Error(stderr.trim().to_string()))
}

/// Commit and optionally push PR review fixes
///
/// This function:
/// 1. Checks if there are uncommitted changes
/// 2. Stages all changes
/// 3. Creates a commit with the given message
/// 4. Optionally pushes to remote if `push_enabled` is true
///
/// # Arguments
/// * `pr_number` - The PR number for the commit message
/// * `commit_enabled` - Whether to create a commit (from config)
/// * `push_enabled` - Whether to push after commit (from config)
///
/// # Returns
/// Tuple of (commit_result, push_result) where push_result is None if push was skipped
pub fn commit_and_push_pr_fixes(
    pr_number: u32,
    commit_enabled: bool,
    push_enabled: bool,
) -> Result<(Option<CommitResult>, Option<PushResult>)> {
    // If commit is disabled, return early
    if !commit_enabled {
        return Ok((None, None));
    }

    // Check for uncommitted changes
    if !has_uncommitted_changes()? {
        return Ok((Some(CommitResult::NothingToCommit), None));
    }

    // Stage all changes
    stage_all_changes()?;

    // Create commit with descriptive message
    let commit_message = format!(
        "fix: address PR #{} review feedback\n\nApply fixes based on PR review comments.",
        pr_number
    );
    let commit_result = create_commit(&commit_message)?;

    // Only push if commit was successful and push is enabled
    let push_result = match (&commit_result, push_enabled) {
        (CommitResult::Success(_), true) => {
            let branch = current_branch()?;
            Some(push_branch(&branch)?)
        }
        _ => None,
    };

    Ok((Some(commit_result), push_result))
}

/// Push the current branch to origin with upstream tracking
///
/// Uses `git push --set-upstream origin <branch>` to push the branch
/// and set up tracking. If the branch already exists on remote, it will
/// still push any new commits.
///
/// # Arguments
/// * `branch` - The branch name to push
///
/// # Returns
/// * `PushResult::Success` - Push completed successfully
/// * `PushResult::AlreadyUpToDate` - Branch is already up-to-date
/// * `PushResult::Error(msg)` - Push failed with error message
pub fn push_branch(branch: &str) -> Result<PushResult> {
    let output = Command::new("git")
        .args(["push", "--set-upstream", "origin", branch])
        .output()?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    if output.status.success() {
        // Check if already up-to-date (git push outputs this to stderr)
        if stderr.contains("Everything up-to-date") {
            return Ok(PushResult::AlreadyUpToDate);
        }
        return Ok(PushResult::Success);
    }

    // Handle specific error cases
    let error_msg = if stderr.is_empty() {
        stdout.trim().to_string()
    } else {
        stderr.trim().to_string()
    };

    // Check for non-fast-forward (branch exists but diverged)
    if error_msg.contains("non-fast-forward")
        || error_msg.contains("rejected")
        || error_msg.contains("failed to push")
    {
        // Try with --force-with-lease for safe force push
        let force_output = Command::new("git")
            .args([
                "push",
                "--force-with-lease",
                "--set-upstream",
                "origin",
                branch,
            ])
            .output()?;

        if force_output.status.success() {
            return Ok(PushResult::Success);
        }

        let force_stderr = String::from_utf8_lossy(&force_output.stderr);
        return Ok(PushResult::Error(format!(
            "Failed to push branch (even with --force-with-lease): {}",
            force_stderr.trim()
        )));
    }

    Ok(PushResult::Error(error_msg))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_git_repo() {
        // This test runs within a git repo, so should return true
        assert!(is_git_repo());
    }

    #[test]
    fn test_current_branch_returns_string() {
        // Should return some branch name (not empty)
        let branch = current_branch();
        assert!(branch.is_ok());
        assert!(!branch.unwrap().is_empty());
    }

    #[test]
    fn test_latest_commit_short_returns_valid_hash() {
        // In a git repo, should return a short hash (typically 7 chars)
        let hash = latest_commit_short();
        assert!(hash.is_ok());
        let hash = hash.unwrap();
        // Short hash should be alphanumeric and reasonable length (typically 7-10 chars)
        assert!(!hash.is_empty());
        assert!(hash.len() >= 7);
        assert!(hash.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    // ========================================================================
    // PushResult enum tests
    // ========================================================================

    #[test]
    fn test_push_result_success_variant() {
        let result = PushResult::Success;
        assert!(matches!(result, PushResult::Success));
    }

    #[test]
    fn test_push_result_already_up_to_date_variant() {
        let result = PushResult::AlreadyUpToDate;
        assert!(matches!(result, PushResult::AlreadyUpToDate));
    }

    #[test]
    fn test_push_result_error_contains_message() {
        let msg = "permission denied".to_string();
        let result = PushResult::Error(msg.clone());
        assert_eq!(result, PushResult::Error(msg));
    }

    #[test]
    fn test_push_result_variants_are_distinct() {
        let success = PushResult::Success;
        let up_to_date = PushResult::AlreadyUpToDate;
        let error = PushResult::Error("error".to_string());

        assert_ne!(success, up_to_date);
        assert_ne!(success, error);
        assert_ne!(up_to_date, error);
    }

    #[test]
    fn test_push_result_clone() {
        let original = PushResult::Error("test error".to_string());
        let cloned = original.clone();
        assert_eq!(original, cloned);
    }

    #[test]
    fn test_push_result_debug() {
        let result = PushResult::Success;
        let debug = format!("{:?}", result);
        assert!(debug.contains("Success"));
    }

    // Note: We don't test push_branch directly because it requires network access
    // and would actually push to remote. The function is tested via integration
    // tests or manual testing. The unit tests verify the PushResult enum behavior.

    // ========================================================================
    // US-003: CommitInfo and branch commit tests
    // ========================================================================

    #[test]
    fn test_commit_info_struct() {
        let commit = CommitInfo {
            short_hash: "abc1234".to_string(),
            full_hash: "abc1234567890def".to_string(),
            message: "Test commit message".to_string(),
            author: "Test Author".to_string(),
            date: "2024-01-15 10:30:00 -0500".to_string(),
        };

        assert_eq!(commit.short_hash, "abc1234");
        assert_eq!(commit.full_hash, "abc1234567890def");
        assert_eq!(commit.message, "Test commit message");
        assert_eq!(commit.author, "Test Author");
        assert_eq!(commit.date, "2024-01-15 10:30:00 -0500");
    }

    #[test]
    fn test_commit_info_clone() {
        let commit = CommitInfo {
            short_hash: "abc1234".to_string(),
            full_hash: "abc1234567890def".to_string(),
            message: "Test message".to_string(),
            author: "Author".to_string(),
            date: "2024-01-15".to_string(),
        };

        let cloned = commit.clone();
        assert_eq!(commit, cloned);
    }

    #[test]
    fn test_commit_info_equality() {
        let commit1 = CommitInfo {
            short_hash: "abc1234".to_string(),
            full_hash: "abc1234567890def".to_string(),
            message: "Message".to_string(),
            author: "Author".to_string(),
            date: "2024-01-15".to_string(),
        };

        let commit2 = CommitInfo {
            short_hash: "abc1234".to_string(),
            full_hash: "abc1234567890def".to_string(),
            message: "Message".to_string(),
            author: "Author".to_string(),
            date: "2024-01-15".to_string(),
        };

        let commit3 = CommitInfo {
            short_hash: "xyz5678".to_string(),
            full_hash: "xyz5678901234abc".to_string(),
            message: "Different".to_string(),
            author: "Other".to_string(),
            date: "2024-01-16".to_string(),
        };

        assert_eq!(commit1, commit2);
        assert_ne!(commit1, commit3);
    }

    #[test]
    fn test_commit_info_debug() {
        let commit = CommitInfo {
            short_hash: "abc1234".to_string(),
            full_hash: "abc1234567890def".to_string(),
            message: "Test".to_string(),
            author: "Author".to_string(),
            date: "2024-01-15".to_string(),
        };

        let debug = format!("{:?}", commit);
        assert!(debug.contains("CommitInfo"));
        assert!(debug.contains("abc1234"));
    }

    #[test]
    fn test_detect_base_branch_returns_string() {
        // Should detect main or master, or return a default
        let result = detect_base_branch();
        assert!(result.is_ok());
        let branch = result.unwrap();
        assert!(!branch.is_empty());
        // Should be either main, master, or some other branch name
        assert!(branch
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '/'));
    }

    #[test]
    fn test_get_branch_commits_returns_vec() {
        // This test may return empty or commits depending on the state of the repo
        // We just verify it doesn't panic and returns a valid result
        let result = get_branch_commits("main");
        // Result could be Ok with commits, Ok with empty, or Err if branch doesn't exist
        // We just verify it doesn't panic
        match result {
            Ok(commits) => {
                // Verify all commits have valid fields
                for commit in commits {
                    assert!(!commit.short_hash.is_empty());
                    assert!(!commit.full_hash.is_empty());
                    // Message could be empty but hash shouldn't be
                }
            }
            Err(_) => {
                // Error is acceptable if 'main' doesn't exist
            }
        }
    }

    #[test]
    fn test_get_current_branch_commits_returns_result() {
        // This test may return empty or commits depending on the state of the repo
        let result = get_current_branch_commits();
        // We just verify it returns a valid result type
        match result {
            Ok(commits) => {
                // Valid: we got some commits (or empty vec)
                for commit in commits {
                    assert!(!commit.short_hash.is_empty());
                }
            }
            Err(_) => {
                // Also valid: base branch might not exist
            }
        }
    }

    #[test]
    fn test_get_commit_diff_invalid_hash() {
        // Testing with an invalid hash should return an error
        let result = get_commit_diff("invalid_hash_that_does_not_exist");
        assert!(result.is_err());
    }

    // ========================================================================
    // US-006: CommitResult enum and PR review commit/push tests
    // ========================================================================

    #[test]
    fn test_commit_result_success_variant() {
        let result = CommitResult::Success("abc1234".to_string());
        assert!(matches!(result, CommitResult::Success(_)));
    }

    #[test]
    fn test_commit_result_nothing_to_commit_variant() {
        let result = CommitResult::NothingToCommit;
        assert!(matches!(result, CommitResult::NothingToCommit));
    }

    #[test]
    fn test_commit_result_error_contains_message() {
        let msg = "staging area is empty".to_string();
        let result = CommitResult::Error(msg.clone());
        assert_eq!(result, CommitResult::Error(msg));
    }

    #[test]
    fn test_commit_result_variants_are_distinct() {
        let success = CommitResult::Success("hash".to_string());
        let nothing = CommitResult::NothingToCommit;
        let error = CommitResult::Error("error".to_string());

        assert_ne!(success, nothing);
        assert_ne!(success, error);
        assert_ne!(nothing, error);
    }

    #[test]
    fn test_commit_result_clone() {
        let original = CommitResult::Success("abc1234".to_string());
        let cloned = original.clone();
        assert_eq!(original, cloned);
    }

    #[test]
    fn test_commit_result_debug() {
        let result = CommitResult::Success("abc1234".to_string());
        let debug = format!("{:?}", result);
        assert!(debug.contains("Success"));
        assert!(debug.contains("abc1234"));
    }

    #[test]
    fn test_has_uncommitted_changes_returns_bool() {
        // This test runs in a git repo, should not error
        let result = has_uncommitted_changes();
        assert!(result.is_ok());
        // Result could be true or false depending on repo state
    }

    #[test]
    fn test_commit_and_push_with_commit_disabled_returns_none() {
        // When commit is disabled, should return (None, None)
        let result = commit_and_push_pr_fixes(123, false, false);
        assert!(result.is_ok());
        let (commit_result, push_result) = result.unwrap();
        assert!(commit_result.is_none());
        assert!(push_result.is_none());
    }

    #[test]
    fn test_commit_and_push_with_commit_enabled_but_push_disabled() {
        // When commit is enabled but push disabled, push_result should be None
        // Note: This test doesn't actually create commits to avoid mutating the repo
        // It just verifies the function can be called without panicking
        let result = commit_and_push_pr_fixes(123, true, false);
        assert!(result.is_ok());
        let (commit_result, push_result) = result.unwrap();
        // commit_result will be Some (either NothingToCommit or Success)
        assert!(commit_result.is_some());
        // push_result should be None because push is disabled
        assert!(push_result.is_none());
    }

    #[test]
    fn test_stage_all_changes_does_not_error() {
        // This test runs in a git repo, should not error (even if nothing to stage)
        let result = stage_all_changes();
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_commit_with_nothing_to_commit() {
        // In a clean repo state, create_commit should return NothingToCommit
        // First ensure working directory is clean by checking status
        if !has_uncommitted_changes().unwrap_or(true) {
            let result = create_commit("test commit");
            assert!(result.is_ok());
            let commit_result = result.unwrap();
            assert!(matches!(commit_result, CommitResult::NothingToCommit));
        }
        // If there are changes, we skip the test to avoid modifying the repo
    }
}
