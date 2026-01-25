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
fn checkout(branch: &str) -> Result<()> {
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
}
