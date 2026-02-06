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
// US-002: Git Diff Capture Functions
// ============================================================================

/// Status of a file in a git diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffStatus {
    /// File was newly created
    Added,
    /// File was modified
    Modified,
    /// File was deleted
    Deleted,
}

/// A single entry from a git diff operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffEntry {
    /// Path to the changed file
    pub path: std::path::PathBuf,
    /// Number of lines added
    pub additions: u32,
    /// Number of lines deleted
    pub deletions: u32,
    /// The type of change (Added, Modified, Deleted)
    pub status: DiffStatus,
}

impl DiffEntry {
    /// Parse a single line of `git diff --numstat` output.
    ///
    /// Format: "additions\tdeletions\tfilepath"
    /// Binary files show "-" for additions/deletions.
    ///
    /// Returns None if the line cannot be parsed.
    pub fn from_numstat_line(line: &str) -> Option<Self> {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() != 3 {
            return None;
        }

        let additions = parts[0].parse().unwrap_or(0);
        let deletions = parts[1].parse().unwrap_or(0);
        let path = std::path::PathBuf::from(parts[2]);

        // Determine status based on additions/deletions
        // This is a heuristic - for truly accurate status we'd need --name-status
        let status = if deletions == 0 && additions > 0 {
            // Could be new file or modification - we'll refine this with --name-status
            DiffStatus::Modified
        } else if additions == 0 && deletions > 0 {
            // Could be deleted or just lines removed - we'll refine this
            DiffStatus::Modified
        } else {
            DiffStatus::Modified
        };

        Some(DiffEntry {
            path,
            additions,
            deletions,
            status,
        })
    }

    /// Parse a line from `git diff --name-status` output.
    ///
    /// Format: "status\tfilepath" where status is A, M, D, R, C, etc.
    ///
    /// Returns the path and status if parseable.
    fn parse_name_status_line(line: &str) -> Option<(std::path::PathBuf, DiffStatus)> {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.is_empty() {
            return None;
        }

        let status_char = parts[0].chars().next()?;
        let status = match status_char {
            'A' => DiffStatus::Added,
            'D' => DiffStatus::Deleted,
            'M' | 'R' | 'C' | 'T' => DiffStatus::Modified,
            _ => DiffStatus::Modified,
        };

        // For rename/copy, the path is in parts[2], otherwise parts[1]
        let path = if status_char == 'R' || status_char == 'C' {
            parts.get(2).map(|p| std::path::PathBuf::from(*p))?
        } else {
            parts.get(1).map(|p| std::path::PathBuf::from(*p))?
        };

        Some((path, status))
    }
}

/// Get the full commit hash of HEAD.
///
/// # Returns
/// * `Ok(String)` - The full 40-character commit hash
/// * `Err` - If the git command fails (e.g., not in a git repo)
pub fn get_head_commit() -> Result<String> {
    let output = Command::new("git").args(["rev-parse", "HEAD"]).output()?;

    if !output.status.success() {
        return Err(Autom8Error::GitError(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Get file changes since a specific commit.
///
/// Uses `git diff --numstat` combined with `--name-status` to get accurate
/// file change information including additions, deletions, and change type.
///
/// # Arguments
/// * `base_commit` - The commit hash to compare against (e.g., "abc1234" or "HEAD~5")
///
/// # Returns
/// * `Ok(Vec<DiffEntry>)` - List of file changes (empty if no changes or not a git repo)
/// * `Err` - Only on IO errors, not on git command failures
pub fn get_diff_since(base_commit: &str) -> Result<Vec<DiffEntry>> {
    // First, check if we're in a git repo
    if !is_git_repo() {
        return Ok(Vec::new());
    }

    // Get numstat for additions/deletions
    let numstat_output = Command::new("git")
        .args(["diff", "--numstat", base_commit])
        .output()?;

    // Get name-status for accurate status info
    let name_status_output = Command::new("git")
        .args(["diff", "--name-status", base_commit])
        .output()?;

    // If either command fails, return empty (graceful degradation)
    if !numstat_output.status.success() || !name_status_output.status.success() {
        return Ok(Vec::new());
    }

    // Build a map of path -> status from name-status output
    let name_status_stdout = String::from_utf8_lossy(&name_status_output.stdout);
    let status_map: std::collections::HashMap<std::path::PathBuf, DiffStatus> = name_status_stdout
        .lines()
        .filter_map(DiffEntry::parse_name_status_line)
        .collect();

    // Parse numstat output and apply accurate status
    let numstat_stdout = String::from_utf8_lossy(&numstat_output.stdout);
    let entries: Vec<DiffEntry> = numstat_stdout
        .lines()
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let mut entry = DiffEntry::from_numstat_line(line)?;
            // Override status with accurate info from name-status
            if let Some(status) = status_map.get(&entry.path) {
                entry.status = status.clone();
            }
            Some(entry)
        })
        .collect();

    Ok(entries)
}

/// Get uncommitted changes in the working directory.
///
/// This includes both staged and unstaged changes. Uses `git diff HEAD --numstat`
/// to compare the working directory against HEAD.
///
/// # Returns
/// * `Ok(Vec<DiffEntry>)` - List of uncommitted changes (empty if clean or not a git repo)
/// * `Err` - Only on IO errors
pub fn get_uncommitted_changes() -> Result<Vec<DiffEntry>> {
    // First, check if we're in a git repo
    if !is_git_repo() {
        return Ok(Vec::new());
    }

    // Get numstat for additions/deletions (comparing HEAD to working directory)
    let numstat_output = Command::new("git")
        .args(["diff", "HEAD", "--numstat"])
        .output()?;

    // Get name-status for accurate status info
    let name_status_output = Command::new("git")
        .args(["diff", "HEAD", "--name-status"])
        .output()?;

    // If either command fails, return empty (graceful degradation)
    if !numstat_output.status.success() || !name_status_output.status.success() {
        return Ok(Vec::new());
    }

    // Build a map of path -> status from name-status output
    let name_status_stdout = String::from_utf8_lossy(&name_status_output.stdout);
    let status_map: std::collections::HashMap<std::path::PathBuf, DiffStatus> = name_status_stdout
        .lines()
        .filter_map(DiffEntry::parse_name_status_line)
        .collect();

    // Parse numstat output and apply accurate status
    let numstat_stdout = String::from_utf8_lossy(&numstat_output.stdout);
    let entries: Vec<DiffEntry> = numstat_stdout
        .lines()
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let mut entry = DiffEntry::from_numstat_line(line)?;
            if let Some(status) = status_map.get(&entry.path) {
                entry.status = status.clone();
            }
            Some(entry)
        })
        .collect();

    Ok(entries)
}

/// Get newly created files since a specific commit.
///
/// Returns only files that were added (not modified or deleted).
///
/// # Arguments
/// * `base_commit` - The commit hash to compare against
///
/// # Returns
/// * `Ok(Vec<PathBuf>)` - List of newly created file paths (empty if none or not a git repo)
/// * `Err` - Only on IO errors
pub fn get_new_files_since(base_commit: &str) -> Result<Vec<std::path::PathBuf>> {
    // First, check if we're in a git repo
    if !is_git_repo() {
        return Ok(Vec::new());
    }

    // Get name-status with diff-filter=A (added files only)
    let output = Command::new("git")
        .args(["diff", "--name-only", "--diff-filter=A", base_commit])
        .output()?;

    // If command fails, return empty (graceful degradation)
    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let files: Vec<std::path::PathBuf> = stdout
        .lines()
        .filter(|line| !line.is_empty())
        .map(std::path::PathBuf::from)
        .collect();

    Ok(files)
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

// ============================================================================
// US-001: Merge Base Detection (improve command support)
// ============================================================================

/// Get the merge-base commit between the current branch and base branch.
///
/// The merge-base is the most recent common ancestor between two branches.
/// This is useful for getting accurate diffs of what changed on a feature branch.
///
/// # Arguments
/// * `base_branch` - The base branch to compare against (e.g., "main" or "master")
///
/// # Returns
/// * `Ok(String)` - The full commit hash of the merge-base
/// * `Err` - If the git command fails or branches don't share history
pub fn get_merge_base(base_branch: &str) -> Result<String> {
    let output = Command::new("git")
        .args(["merge-base", base_branch, "HEAD"])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Autom8Error::GitError(format!(
            "Failed to find merge-base with '{}': {}",
            base_branch,
            stderr.trim()
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Get the merge-base commit, auto-detecting the base branch.
///
/// Convenience function that combines `detect_base_branch` and `get_merge_base`.
///
/// # Returns
/// * `Ok(String)` - The full commit hash of the merge-base
/// * `Err` - If the git command fails
pub fn get_merge_base_auto() -> Result<String> {
    let base_branch = detect_base_branch()?;
    get_merge_base(&base_branch)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // DiffEntry parsing tests - these test actual parsing logic
    // ========================================================================

    #[test]
    fn test_diff_entry_from_numstat_line_basic() {
        let line = "10\t5\tsrc/lib.rs";
        let entry = DiffEntry::from_numstat_line(line);

        assert!(entry.is_some());
        let entry = entry.unwrap();
        assert_eq!(entry.path, std::path::PathBuf::from("src/lib.rs"));
        assert_eq!(entry.additions, 10);
        assert_eq!(entry.deletions, 5);
    }

    #[test]
    fn test_diff_entry_from_numstat_line_binary_file() {
        // Binary files show "-" for additions/deletions
        let line = "-\t-\timage.png";
        let entry = DiffEntry::from_numstat_line(line).unwrap();

        assert_eq!(entry.path, std::path::PathBuf::from("image.png"));
        assert_eq!(entry.additions, 0);
        assert_eq!(entry.deletions, 0);
    }

    #[test]
    fn test_diff_entry_from_numstat_line_path_with_spaces() {
        let line = "5\t3\tpath/to/my file.rs";
        let entry = DiffEntry::from_numstat_line(line).unwrap();

        assert_eq!(entry.path, std::path::PathBuf::from("path/to/my file.rs"));
    }

    #[test]
    fn test_diff_entry_from_numstat_line_invalid() {
        assert!(DiffEntry::from_numstat_line("10\t5").is_none());
        assert!(DiffEntry::from_numstat_line("").is_none());
    }

    #[test]
    fn test_diff_entry_parse_name_status_variants() {
        // Added
        let (path, status) = DiffEntry::parse_name_status_line("A\tsrc/new_file.rs").unwrap();
        assert_eq!(path, std::path::PathBuf::from("src/new_file.rs"));
        assert_eq!(status, DiffStatus::Added);

        // Modified
        let (path, status) = DiffEntry::parse_name_status_line("M\tsrc/changed.rs").unwrap();
        assert_eq!(path, std::path::PathBuf::from("src/changed.rs"));
        assert_eq!(status, DiffStatus::Modified);

        // Deleted
        let (path, status) = DiffEntry::parse_name_status_line("D\tsrc/removed.rs").unwrap();
        assert_eq!(path, std::path::PathBuf::from("src/removed.rs"));
        assert_eq!(status, DiffStatus::Deleted);

        // Renamed (returns new path)
        let (path, status) =
            DiffEntry::parse_name_status_line("R100\told_name.rs\tnew_name.rs").unwrap();
        assert_eq!(path, std::path::PathBuf::from("new_name.rs"));
        assert_eq!(status, DiffStatus::Modified);

        // Invalid
        assert!(DiffEntry::parse_name_status_line("").is_none());
    }

    // ========================================================================
    // Logic tests - test actual behavior without side effects
    // ========================================================================

    #[test]
    fn test_commit_and_push_with_commit_disabled_returns_none() {
        // When commit is disabled, should return (None, None) without doing anything
        let result = commit_and_push_pr_fixes(123, false, false);
        assert!(result.is_ok());
        let (commit_result, push_result) = result.unwrap();
        assert!(commit_result.is_none());
        assert!(push_result.is_none());
    }
}
