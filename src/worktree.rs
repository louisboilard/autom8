//! Git worktree operations for autom8.
//!
//! This module provides functions for managing git worktrees, enabling
//! parallel execution of autom8 sessions on the same project.

use crate::error::{Autom8Error, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Information about a git worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeInfo {
    /// Absolute path to the worktree directory
    pub path: PathBuf,
    /// The branch checked out in this worktree (None for detached HEAD)
    pub branch: Option<String>,
    /// The current commit hash
    pub commit: String,
    /// Whether this is the main worktree (the original repo)
    pub is_main: bool,
    /// Whether this worktree is bare (no working directory)
    pub is_bare: bool,
    /// Whether the worktree is currently locked
    pub is_locked: bool,
    /// Whether the worktree is prunable (missing directory)
    pub is_prunable: bool,
}

impl WorktreeInfo {
    /// Parse a single worktree from porcelain output lines.
    ///
    /// The porcelain format outputs one attribute per line, with worktrees
    /// separated by blank lines.
    fn from_porcelain_lines(lines: &[&str]) -> Option<Self> {
        let mut path: Option<PathBuf> = None;
        let mut branch: Option<String> = None;
        let mut commit: Option<String> = None;
        let mut is_bare = false;
        let mut is_locked = false;
        let mut is_prunable = false;

        for line in lines {
            if let Some(rest) = line.strip_prefix("worktree ") {
                path = Some(PathBuf::from(rest));
            } else if let Some(rest) = line.strip_prefix("HEAD ") {
                commit = Some(rest.to_string());
            } else if let Some(rest) = line.strip_prefix("branch ") {
                // Branch is in format "refs/heads/branch-name"
                let branch_name = rest.strip_prefix("refs/heads/").unwrap_or(rest).to_string();
                branch = Some(branch_name);
            } else if *line == "bare" {
                is_bare = true;
            } else if *line == "detached" {
                // Detached HEAD - branch remains None
            } else if line.starts_with("locked") {
                is_locked = true;
            } else if line.starts_with("prunable") {
                is_prunable = true;
            }
        }

        let path = path?;
        let commit = commit?;

        // The first worktree listed is always the main worktree
        // We'll set this properly in list_worktrees()
        Some(WorktreeInfo {
            path,
            branch,
            commit,
            is_main: false,
            is_bare,
            is_locked,
            is_prunable,
        })
    }
}

/// List all worktrees for the current repository.
///
/// Returns information about each worktree including path, branch, and commit.
/// The main repository is always included in the list with `is_main: true`.
///
/// # Returns
/// * `Ok(Vec<WorktreeInfo>)` - List of worktrees (always has at least one - the main repo)
/// * `Err` - If not in a git repository or git command fails
pub fn list_worktrees() -> Result<Vec<WorktreeInfo>> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Autom8Error::WorktreeError(format!(
            "Failed to list worktrees: {}",
            stderr.trim()
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let worktrees = parse_worktree_list_porcelain(&stdout)?;

    Ok(worktrees)
}

/// Parse the output of `git worktree list --porcelain`.
///
/// The porcelain format is machine-readable with one attribute per line,
/// and worktrees separated by blank lines.
fn parse_worktree_list_porcelain(output: &str) -> Result<Vec<WorktreeInfo>> {
    let mut worktrees = Vec::new();
    let mut current_lines: Vec<&str> = Vec::new();
    let mut is_first = true;

    for line in output.lines() {
        if line.is_empty() {
            // End of a worktree block
            if !current_lines.is_empty() {
                if let Some(mut wt) = WorktreeInfo::from_porcelain_lines(&current_lines) {
                    // First worktree in the list is always the main worktree
                    wt.is_main = is_first;
                    is_first = false;
                    worktrees.push(wt);
                }
                current_lines.clear();
            }
        } else {
            current_lines.push(line);
        }
    }

    // Don't forget the last worktree (output may not end with blank line)
    if !current_lines.is_empty() {
        if let Some(mut wt) = WorktreeInfo::from_porcelain_lines(&current_lines) {
            wt.is_main = is_first;
            worktrees.push(wt);
        }
    }

    Ok(worktrees)
}

/// Create a new worktree at the specified path for the given branch.
///
/// If the branch already exists, it will be checked out in the new worktree.
/// If the branch doesn't exist, it will be created from the current HEAD.
///
/// # Arguments
/// * `path` - The path where the worktree should be created
/// * `branch` - The branch name to checkout or create
///
/// # Returns
/// * `Ok(())` - Worktree created successfully
/// * `Err` - If creation fails (e.g., branch already checked out elsewhere)
pub fn create_worktree<P: AsRef<Path>>(path: P, branch: &str) -> Result<()> {
    let path = path.as_ref();

    // First, check if branch exists
    let branch_exists = Command::new("git")
        .args([
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{}", branch),
        ])
        .output()?
        .status
        .success();

    let output = if branch_exists {
        // Branch exists, just add worktree
        Command::new("git")
            .args(["worktree", "add", path.to_string_lossy().as_ref(), branch])
            .output()?
    } else {
        // Create new branch with -b flag
        Command::new("git")
            .args([
                "worktree",
                "add",
                "-b",
                branch,
                path.to_string_lossy().as_ref(),
            ])
            .output()?
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Autom8Error::WorktreeError(format!(
            "Failed to create worktree at '{}' for branch '{}': {}",
            path.display(),
            branch,
            stderr.trim()
        )));
    }

    Ok(())
}

/// Remove a worktree at the specified path.
///
/// By default, this will fail if the worktree has uncommitted changes.
/// Use `force: true` to remove even with uncommitted changes.
///
/// # Arguments
/// * `path` - The path of the worktree to remove
/// * `force` - If true, remove even if the worktree has uncommitted changes
///
/// # Returns
/// * `Ok(())` - Worktree removed successfully
/// * `Err` - If removal fails
pub fn remove_worktree<P: AsRef<Path>>(path: P, force: bool) -> Result<()> {
    let path = path.as_ref();
    let path_str = path.to_string_lossy();

    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(path_str.as_ref());

    let output = Command::new("git").args(&args).output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Autom8Error::WorktreeError(format!(
            "Failed to remove worktree at '{}': {}",
            path.display(),
            stderr.trim()
        )));
    }

    Ok(())
}

/// Get the worktree root for the current directory.
///
/// If the current directory is inside a linked worktree (not the main repo),
/// returns the root path of that worktree. Returns None if in the main repo.
///
/// # Returns
/// * `Ok(Some(path))` - The worktree root if in a linked worktree
/// * `Ok(None)` - If in the main repository (not a linked worktree)
/// * `Err` - If not in a git repository
pub fn get_worktree_root() -> Result<Option<PathBuf>> {
    // git rev-parse --git-common-dir returns the .git dir of the main repo
    // git rev-parse --git-dir returns the .git dir of the current worktree
    // If they're different, we're in a linked worktree

    let git_dir_output = Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .output()?;

    if !git_dir_output.status.success() {
        let stderr = String::from_utf8_lossy(&git_dir_output.stderr);
        return Err(Autom8Error::WorktreeError(format!(
            "Failed to get git directory: {}",
            stderr.trim()
        )));
    }

    let git_dir = String::from_utf8_lossy(&git_dir_output.stdout)
        .trim()
        .to_string();

    // In a linked worktree, git-dir points to .git/worktrees/<name>
    // The gitdir file inside contains the path we need to check
    if git_dir.contains("/worktrees/") || git_dir.contains("\\worktrees\\") {
        // We're in a worktree - get the toplevel
        let toplevel_output = Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .output()?;

        if !toplevel_output.status.success() {
            let stderr = String::from_utf8_lossy(&toplevel_output.stderr);
            return Err(Autom8Error::WorktreeError(format!(
                "Failed to get worktree root: {}",
                stderr.trim()
            )));
        }

        let toplevel = String::from_utf8_lossy(&toplevel_output.stdout)
            .trim()
            .to_string();
        return Ok(Some(PathBuf::from(toplevel)));
    }

    Ok(None)
}

/// Get the main repository root (works from any worktree).
///
/// Returns the path to the main repository, regardless of whether
/// the current directory is in the main repo or a linked worktree.
///
/// # Returns
/// * `Ok(path)` - The main repository root path
/// * `Err` - If not in a git repository
pub fn get_main_repo_root() -> Result<PathBuf> {
    // git rev-parse --git-common-dir gives us the path to the main .git directory
    let output = Command::new("git")
        .args(["rev-parse", "--git-common-dir"])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Autom8Error::WorktreeError(format!(
            "Failed to get main repo root: {}",
            stderr.trim()
        )));
    }

    let git_common_dir = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // The common dir is the .git directory - we want its parent
    let git_path = PathBuf::from(&git_common_dir);

    // Handle both .git file (in worktrees) and .git directory cases
    // Also handle absolute vs relative paths
    let main_repo_path = if git_path.is_absolute() {
        git_path.parent().map(|p| p.to_path_buf())
    } else {
        // Relative path - resolve it
        let current_dir = std::env::current_dir()?;
        let absolute_git = current_dir.join(&git_path);
        absolute_git
            .canonicalize()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
    };

    main_repo_path.ok_or_else(|| {
        Autom8Error::WorktreeError("Failed to determine main repository root".to_string())
    })
}

/// Check if the current working directory is inside a linked worktree.
///
/// Returns true if the CWD is inside a linked worktree (not the main repository).
///
/// # Returns
/// * `Ok(true)` - CWD is inside a linked worktree
/// * `Ok(false)` - CWD is inside the main repository
/// * `Err` - If not in a git repository
pub fn is_in_worktree() -> Result<bool> {
    Ok(get_worktree_root()?.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Use the shared CWD_MUTEX for tests that depend on current working directory
    use crate::test_utils::CWD_MUTEX;

    // ========================================================================
    // WorktreeInfo struct tests
    // ========================================================================

    #[test]
    fn test_worktree_info_creation() {
        let info = WorktreeInfo {
            path: PathBuf::from("/path/to/worktree"),
            branch: Some("feature/test".to_string()),
            commit: "abc1234567890".to_string(),
            is_main: false,
            is_bare: false,
            is_locked: false,
            is_prunable: false,
        };

        assert_eq!(info.path, PathBuf::from("/path/to/worktree"));
        assert_eq!(info.branch, Some("feature/test".to_string()));
        assert_eq!(info.commit, "abc1234567890");
        assert!(!info.is_main);
        assert!(!info.is_bare);
        assert!(!info.is_locked);
        assert!(!info.is_prunable);
    }

    #[test]
    fn test_worktree_info_clone() {
        let original = WorktreeInfo {
            path: PathBuf::from("/path/to/worktree"),
            branch: Some("main".to_string()),
            commit: "abc1234".to_string(),
            is_main: true,
            is_bare: false,
            is_locked: false,
            is_prunable: false,
        };

        let cloned = original.clone();
        assert_eq!(original, cloned);
    }

    #[test]
    fn test_worktree_info_equality() {
        let wt1 = WorktreeInfo {
            path: PathBuf::from("/path1"),
            branch: Some("main".to_string()),
            commit: "abc".to_string(),
            is_main: true,
            is_bare: false,
            is_locked: false,
            is_prunable: false,
        };

        let wt2 = WorktreeInfo {
            path: PathBuf::from("/path1"),
            branch: Some("main".to_string()),
            commit: "abc".to_string(),
            is_main: true,
            is_bare: false,
            is_locked: false,
            is_prunable: false,
        };

        let wt3 = WorktreeInfo {
            path: PathBuf::from("/path2"),
            branch: Some("feature".to_string()),
            commit: "def".to_string(),
            is_main: false,
            is_bare: false,
            is_locked: false,
            is_prunable: false,
        };

        assert_eq!(wt1, wt2);
        assert_ne!(wt1, wt3);
    }

    #[test]
    fn test_worktree_info_debug() {
        let info = WorktreeInfo {
            path: PathBuf::from("/test/path"),
            branch: Some("test-branch".to_string()),
            commit: "abc1234".to_string(),
            is_main: false,
            is_bare: false,
            is_locked: false,
            is_prunable: false,
        };

        let debug = format!("{:?}", info);
        assert!(debug.contains("WorktreeInfo"));
        assert!(debug.contains("test-branch"));
    }

    #[test]
    fn test_worktree_info_detached_head() {
        let info = WorktreeInfo {
            path: PathBuf::from("/path/to/worktree"),
            branch: None, // Detached HEAD
            commit: "abc1234567890".to_string(),
            is_main: false,
            is_bare: false,
            is_locked: false,
            is_prunable: false,
        };

        assert!(info.branch.is_none());
    }

    // ========================================================================
    // Porcelain parsing tests
    // ========================================================================

    #[test]
    fn test_parse_porcelain_single_worktree() {
        let output = "worktree /home/user/project\nHEAD abc1234567890abcdef1234567890abcdef12345678\nbranch refs/heads/main\n\n";

        let worktrees = parse_worktree_list_porcelain(output).unwrap();
        assert_eq!(worktrees.len(), 1);

        let wt = &worktrees[0];
        assert_eq!(wt.path, PathBuf::from("/home/user/project"));
        assert_eq!(wt.branch, Some("main".to_string()));
        assert_eq!(wt.commit, "abc1234567890abcdef1234567890abcdef12345678");
        assert!(wt.is_main);
        assert!(!wt.is_bare);
    }

    #[test]
    fn test_parse_porcelain_multiple_worktrees() {
        let output = concat!(
            "worktree /home/user/project\n",
            "HEAD abc1234567890abcdef1234567890abcdef12345678\n",
            "branch refs/heads/main\n",
            "\n",
            "worktree /home/user/project-feature\n",
            "HEAD def5678901234abcdef5678901234abcdef56789012\n",
            "branch refs/heads/feature/test\n",
            "\n"
        );

        let worktrees = parse_worktree_list_porcelain(output).unwrap();
        assert_eq!(worktrees.len(), 2);

        // First worktree is main
        assert!(worktrees[0].is_main);
        assert_eq!(worktrees[0].branch, Some("main".to_string()));

        // Second worktree is not main
        assert!(!worktrees[1].is_main);
        assert_eq!(worktrees[1].branch, Some("feature/test".to_string()));
    }

    #[test]
    fn test_parse_porcelain_detached_head() {
        let output = "worktree /home/user/project\nHEAD abc1234567890abcdef1234567890abcdef12345678\ndetached\n\n";

        let worktrees = parse_worktree_list_porcelain(output).unwrap();
        assert_eq!(worktrees.len(), 1);

        let wt = &worktrees[0];
        assert!(wt.branch.is_none()); // Detached HEAD
    }

    #[test]
    fn test_parse_porcelain_bare_repo() {
        let output = "worktree /home/user/project.git\nHEAD abc1234567890abcdef1234567890abcdef12345678\nbare\n\n";

        let worktrees = parse_worktree_list_porcelain(output).unwrap();
        assert_eq!(worktrees.len(), 1);

        let wt = &worktrees[0];
        assert!(wt.is_bare);
    }

    #[test]
    fn test_parse_porcelain_locked_worktree() {
        let output = "worktree /home/user/project\nHEAD abc1234567890abcdef1234567890abcdef12345678\nbranch refs/heads/main\nlocked\n\n";

        let worktrees = parse_worktree_list_porcelain(output).unwrap();
        assert_eq!(worktrees.len(), 1);

        let wt = &worktrees[0];
        assert!(wt.is_locked);
    }

    #[test]
    fn test_parse_porcelain_locked_with_reason() {
        let output = "worktree /home/user/project\nHEAD abc1234567890abcdef1234567890abcdef12345678\nbranch refs/heads/main\nlocked reason for locking\n\n";

        let worktrees = parse_worktree_list_porcelain(output).unwrap();
        assert_eq!(worktrees.len(), 1);

        let wt = &worktrees[0];
        assert!(wt.is_locked);
    }

    #[test]
    fn test_parse_porcelain_prunable_worktree() {
        let output = "worktree /home/user/project\nHEAD abc1234567890abcdef1234567890abcdef12345678\nbranch refs/heads/main\nprunable\n\n";

        let worktrees = parse_worktree_list_porcelain(output).unwrap();
        assert_eq!(worktrees.len(), 1);

        let wt = &worktrees[0];
        assert!(wt.is_prunable);
    }

    #[test]
    fn test_parse_porcelain_no_trailing_newline() {
        let output = "worktree /home/user/project\nHEAD abc1234567890abcdef1234567890abcdef12345678\nbranch refs/heads/main";

        let worktrees = parse_worktree_list_porcelain(output).unwrap();
        assert_eq!(worktrees.len(), 1);
    }

    #[test]
    fn test_parse_porcelain_empty_output() {
        let output = "";

        let worktrees = parse_worktree_list_porcelain(output).unwrap();
        assert!(worktrees.is_empty());
    }

    #[test]
    fn test_parse_porcelain_path_with_spaces() {
        let output = "worktree /home/user/my project/repo\nHEAD abc1234567890abcdef1234567890abcdef12345678\nbranch refs/heads/main\n\n";

        let worktrees = parse_worktree_list_porcelain(output).unwrap();
        assert_eq!(worktrees.len(), 1);
        assert_eq!(
            worktrees[0].path,
            PathBuf::from("/home/user/my project/repo")
        );
    }

    #[test]
    fn test_from_porcelain_lines_missing_path() {
        let lines = vec!["HEAD abc1234", "branch refs/heads/main"];
        let result = WorktreeInfo::from_porcelain_lines(&lines);
        assert!(result.is_none());
    }

    #[test]
    fn test_from_porcelain_lines_missing_commit() {
        let lines = vec!["worktree /path/to/repo", "branch refs/heads/main"];
        let result = WorktreeInfo::from_porcelain_lines(&lines);
        assert!(result.is_none());
    }

    // ========================================================================
    // Integration tests (run against actual git repo)
    // ========================================================================

    #[test]
    fn test_list_worktrees_returns_at_least_one() {
        let _lock = CWD_MUTEX.lock().unwrap();

        // This test runs in a git repo, so should return at least one worktree
        let result = list_worktrees();
        assert!(result.is_ok());

        let worktrees = result.unwrap();
        assert!(!worktrees.is_empty());

        // First worktree should be marked as main
        assert!(worktrees[0].is_main);
    }

    #[test]
    fn test_list_worktrees_main_has_valid_fields() {
        let _lock = CWD_MUTEX.lock().unwrap();

        let worktrees = list_worktrees().unwrap();
        let main_wt = &worktrees[0];

        // Path should exist and not be empty
        assert!(!main_wt.path.as_os_str().is_empty());

        // Commit should be a valid hex string (40 chars)
        assert_eq!(main_wt.commit.len(), 40);
        assert!(main_wt.commit.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_get_main_repo_root_returns_path() {
        let _lock = CWD_MUTEX.lock().unwrap();

        let result = get_main_repo_root();
        assert!(result.is_ok());

        let path = result.unwrap();
        assert!(!path.as_os_str().is_empty());
        // The path should be a directory that exists
        assert!(path.exists());
    }

    #[test]
    fn test_is_in_worktree_returns_bool() {
        let _lock = CWD_MUTEX.lock().unwrap();

        let result = is_in_worktree();
        assert!(result.is_ok());
        // Result could be true or false depending on where we're running
    }

    #[test]
    fn test_get_worktree_root_returns_valid_result() {
        let _lock = CWD_MUTEX.lock().unwrap();

        let result = get_worktree_root();
        assert!(result.is_ok());
        // Result could be Some(path) or None depending on where we're running
    }
}
