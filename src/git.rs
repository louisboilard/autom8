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
        .args(["show-ref", "--verify", "--quiet", &format!("refs/heads/{}", branch)])
        .output()?;

    if local.status.success() {
        return Ok(true);
    }

    // Check remote branches
    let remote = Command::new("git")
        .args(["show-ref", "--verify", "--quiet", &format!("refs/remotes/origin/{}", branch)])
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
    let output = Command::new("git")
        .args(["checkout", branch])
        .output()?;

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
