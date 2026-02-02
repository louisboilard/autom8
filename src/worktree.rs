//! Git worktree operations for autom8.
//!
//! This module provides functions for managing git worktrees, enabling
//! parallel execution of autom8 sessions on the same project.

use crate::error::{Autom8Error, Result};
use sha2::{Digest, Sha256};
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

/// Get the git repository name (basename of the main repo root).
///
/// This function returns the name of the git repository, which is the
/// basename of the main repository root directory. This ensures consistent
/// project identification regardless of whether you're in the main repo
/// or a linked worktree.
///
/// # Returns
/// * `Ok(Some(name))` - The repository name if in a git repository
/// * `Ok(None)` - If not in a git repository
/// * `Err` - If there's an error determining the repo name
///
/// # Example
/// ```no_run
/// use autom8::worktree::get_git_repo_name;
///
/// if let Some(name) = get_git_repo_name().expect("git error") {
///     println!("Repository: {}", name);
/// }
/// ```
pub fn get_git_repo_name() -> Result<Option<String>> {
    // First check if we're in a git repo
    let output = Command::new("git")
        .args(["rev-parse", "--git-common-dir"])
        .output()?;

    if !output.status.success() {
        // Not in a git repository - this is not an error, just means no git
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("not a git repository") {
            return Ok(None);
        }
        return Err(Autom8Error::WorktreeError(format!(
            "Failed to check git repository: {}",
            stderr.trim()
        )));
    }

    // Get the main repo root (works from both main repo and worktrees)
    let main_root = get_main_repo_root()?;

    // Extract the basename
    main_root
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| Some(s.to_string()))
        .ok_or_else(|| {
            Autom8Error::WorktreeError("Could not determine repository name from path".to_string())
        })
}

// ============================================================================
// Session Identity System (US-002)
// ============================================================================

/// Well-known session ID for the main repository.
pub const MAIN_SESSION_ID: &str = "main";

/// Generate a deterministic session ID from a worktree path.
///
/// The session ID is derived from the SHA-256 hash of the absolute path,
/// taking the first 8 characters. This ensures:
/// - Determinism: same path always produces the same ID
/// - Uniqueness: different paths produce different IDs (with high probability)
/// - Filesystem safety: only alphanumeric characters (hex digits)
/// - Readability: 8 characters is short but sufficient
///
/// # Arguments
/// * `worktree_path` - The absolute path to the worktree directory
///
/// # Returns
/// An 8-character hexadecimal string that uniquely identifies the worktree.
///
/// # Example
/// ```
/// use autom8::worktree::generate_session_id;
/// use std::path::Path;
///
/// let id = generate_session_id(Path::new("/home/user/project-feature"));
/// assert_eq!(id.len(), 8);
/// assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
/// ```
pub fn generate_session_id(worktree_path: &Path) -> String {
    let path_str = worktree_path.to_string_lossy();
    let mut hasher = Sha256::new();
    hasher.update(path_str.as_bytes());
    let result = hasher.finalize();
    // Take first 8 characters of hex representation (4 bytes = 8 hex chars)
    hex::encode(&result[..4])
}

/// Get the session ID for the current working directory.
///
/// This function determines the appropriate session ID based on the current
/// location:
/// - If in the main repository: returns the well-known "main" session ID
/// - If in a linked worktree: returns a hash-based ID from the worktree path
///
/// # Returns
/// * `Ok(String)` - The session ID for the current directory
/// * `Err` - If not in a git repository
///
/// # Example
/// ```no_run
/// use autom8::worktree::get_current_session_id;
///
/// let session_id = get_current_session_id().expect("Not in a git repo");
/// println!("Session ID: {}", session_id);
/// ```
pub fn get_current_session_id() -> Result<String> {
    // Check if we're in a linked worktree
    if let Some(worktree_root) = get_worktree_root()? {
        // In a linked worktree - generate ID from path
        Ok(generate_session_id(&worktree_root))
    } else {
        // In main repository - use well-known ID
        Ok(MAIN_SESSION_ID.to_string())
    }
}

/// Get the session ID for the main repository.
///
/// This function returns the session ID that would be used when running
/// from the main repository (not a linked worktree). This is useful for
/// operations that need to reference the main session regardless of
/// the current working directory.
///
/// # Returns
/// The well-known "main" session ID.
pub fn get_main_session_id() -> String {
    MAIN_SESSION_ID.to_string()
}

/// Get the session ID for a specific worktree path.
///
/// This is a convenience function that combines path resolution with
/// session ID generation. For the main repository path, it returns "main".
/// For linked worktree paths, it generates a hash-based ID.
///
/// # Arguments
/// * `path` - The path to resolve a session ID for
///
/// # Returns
/// * `Ok(String)` - The session ID for the given path
/// * `Err` - If the path is not in a git repository or cannot be resolved
pub fn get_session_id_for_path(path: &Path) -> Result<String> {
    // Get the absolute path
    let abs_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };

    // Get the main repo root to compare
    let main_root = get_main_repo_root()?;

    // Canonicalize both paths for reliable comparison
    let abs_canonical = abs_path.canonicalize().unwrap_or(abs_path);
    let main_canonical = main_root.canonicalize().unwrap_or(main_root);

    // Check if this is the main repo
    if abs_canonical == main_canonical {
        Ok(MAIN_SESSION_ID.to_string())
    } else {
        Ok(generate_session_id(&abs_canonical))
    }
}

// ============================================================================
// Worktree Path Generation (US-007)
// ============================================================================

/// Convert a branch name to a filesystem-safe slug.
///
/// Replaces slashes with dashes, removes unsafe characters, and normalizes
/// to produce a valid directory name component.
///
/// # Arguments
/// * `branch_name` - The git branch name to slugify
///
/// # Returns
/// A filesystem-safe version of the branch name.
///
/// # Example
/// ```
/// use autom8::worktree::slugify_branch_name;
///
/// assert_eq!(slugify_branch_name("feature/login"), "feature-login");
/// assert_eq!(slugify_branch_name("feat/add-user-auth"), "feat-add-user-auth");
/// ```
pub fn slugify_branch_name(branch_name: &str) -> String {
    branch_name
        .chars()
        .map(|c| {
            if c == '/' || c == '\\' {
                '-'
            } else if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        // Collapse multiple dashes into one
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Generate the worktree path from a pattern and parameters.
///
/// Replaces placeholders in the pattern:
/// - `{repo}` - The repository name
/// - `{branch}` - The branch name (slugified)
///
/// The worktree is created as a sibling directory of the main repository.
///
/// # Arguments
/// * `pattern` - The path pattern (e.g., "{repo}-wt-{branch}")
/// * `repo_name` - The repository name
/// * `branch_name` - The branch name (will be slugified)
///
/// # Returns
/// The generated worktree directory name.
///
/// # Example
/// ```
/// use autom8::worktree::generate_worktree_name;
///
/// let name = generate_worktree_name("{repo}-wt-{branch}", "myproject", "feature/login");
/// assert_eq!(name, "myproject-wt-feature-login");
/// ```
pub fn generate_worktree_name(pattern: &str, repo_name: &str, branch_name: &str) -> String {
    let slugified_branch = slugify_branch_name(branch_name);
    pattern
        .replace("{repo}", repo_name)
        .replace("{branch}", &slugified_branch)
}

/// Generate the full path for a worktree.
///
/// Creates the worktree as a sibling directory of the main repository.
///
/// # Arguments
/// * `pattern` - The path pattern (e.g., "{repo}-wt-{branch}")
/// * `branch_name` - The branch name (will be slugified)
///
/// # Returns
/// * `Ok(PathBuf)` - The full path where the worktree should be created
/// * `Err` - If not in a git repository
///
/// # Example
/// ```no_run
/// use autom8::worktree::generate_worktree_path;
///
/// // If main repo is at /home/user/myproject, returns:
/// // /home/user/myproject-wt-feature-login
/// let path = generate_worktree_path("{repo}-wt-{branch}", "feature/login").unwrap();
/// ```
pub fn generate_worktree_path(pattern: &str, branch_name: &str) -> Result<PathBuf> {
    let main_repo = get_main_repo_root()?;
    let repo_name = main_repo
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| {
            Autom8Error::WorktreeError("Could not determine repository name".to_string())
        })?;

    let worktree_name = generate_worktree_name(pattern, repo_name, branch_name);

    // Place worktree as sibling of main repo
    let parent = main_repo.parent().ok_or_else(|| {
        Autom8Error::WorktreeError("Could not determine repository parent directory".to_string())
    })?;

    Ok(parent.join(worktree_name))
}

/// Result of ensuring a worktree exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeResult {
    /// A new worktree was created at this path
    Created(PathBuf),
    /// An existing worktree was found and reused at this path
    Reused(PathBuf),
}

impl WorktreeResult {
    /// Get the path regardless of whether it was created or reused.
    pub fn path(&self) -> &Path {
        match self {
            WorktreeResult::Created(p) | WorktreeResult::Reused(p) => p,
        }
    }

    /// Returns true if the worktree was newly created.
    pub fn was_created(&self) -> bool {
        matches!(self, WorktreeResult::Created(_))
    }
}

/// Ensure a worktree exists for the specified branch.
///
/// If a worktree already exists for this branch, it is reused.
/// Otherwise, a new worktree is created.
///
/// # Arguments
/// * `pattern` - The path pattern for the worktree name
/// * `branch_name` - The branch to use for the worktree
///
/// # Returns
/// * `Ok(WorktreeResult::Created(path))` - A new worktree was created
/// * `Ok(WorktreeResult::Reused(path))` - An existing worktree was found
/// * `Err` - If worktree creation fails
pub fn ensure_worktree(pattern: &str, branch_name: &str) -> Result<WorktreeResult> {
    let target_path = generate_worktree_path(pattern, branch_name)?;

    // Check if a worktree already exists at this path or for this branch
    let worktrees = list_worktrees()?;
    for wt in &worktrees {
        // Check if there's a worktree at our target path
        if wt.path == target_path {
            // Verify it has the right branch
            if let Some(ref wt_branch) = wt.branch {
                if wt_branch == branch_name {
                    return Ok(WorktreeResult::Reused(target_path));
                }
            }
            // Path exists but with wrong branch - this is a conflict
            return Err(Autom8Error::WorktreeError(format!(
                "Worktree at '{}' exists but uses branch '{}', not '{}'",
                target_path.display(),
                wt.branch.as_deref().unwrap_or("(detached)"),
                branch_name
            )));
        }

        // Check if there's already a worktree for this branch at a different path
        if let Some(ref wt_branch) = wt.branch {
            if wt_branch == branch_name && !wt.is_main {
                // Branch is already checked out in a worktree - reuse it
                return Ok(WorktreeResult::Reused(wt.path.clone()));
            }
        }
    }

    // No existing worktree found - create a new one
    create_worktree(&target_path, branch_name)?;
    Ok(WorktreeResult::Created(target_path))
}

/// Get detailed information about worktree creation failure.
///
/// Provides suggestions for how to resolve common worktree creation issues.
///
/// # Arguments
/// * `error` - The error message from the failed git command
/// * `branch_name` - The branch that was being created
/// * `worktree_path` - The path where the worktree was being created
///
/// # Returns
/// A user-friendly error message with suggestions.
pub fn format_worktree_error(error: &str, branch_name: &str, worktree_path: &Path) -> String {
    let mut message = format!(
        "Failed to create worktree for branch '{}' at '{}'.\n\n",
        branch_name,
        worktree_path.display()
    );

    // Analyze the error and provide specific suggestions
    if error.contains("already checked out") {
        message.push_str("Reason: Branch is already checked out in another worktree.\n\n");
        message.push_str("Suggestions:\n");
        message.push_str("  1. Use a different branch name in your spec\n");
        message.push_str("  2. Run `git worktree list` to see existing worktrees\n");
        message
            .push_str("  3. Remove the conflicting worktree with `git worktree remove <path>`\n");
    } else if error.contains("already exists") {
        message.push_str("Reason: A directory or worktree already exists at this path.\n\n");
        message.push_str("Suggestions:\n");
        message.push_str(&format!(
            "  1. Remove the existing directory: rm -rf '{}'\n",
            worktree_path.display()
        ));
        message.push_str("  2. Use a different branch name in your spec\n");
        message.push_str("  3. Configure a different worktree_path_pattern in config\n");
    } else if error.contains("permission denied") || error.contains("Permission denied") {
        message.push_str("Reason: Insufficient permissions to create the worktree directory.\n\n");
        message.push_str("Suggestions:\n");
        message.push_str("  1. Check write permissions on the parent directory\n");
        message.push_str("  2. Run with appropriate permissions\n");
    } else {
        message.push_str(&format!("Error: {}\n\n", error));
        message.push_str("Suggestions:\n");
        message.push_str("  1. Ensure you're in a git repository\n");
        message.push_str("  2. Run `git worktree list` to check current worktrees\n");
        message.push_str("  3. Check git configuration and permissions\n");
    }

    message
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

    // ========================================================================
    // Session ID tests (US-002)
    // ========================================================================

    #[test]
    fn test_generate_session_id_returns_8_chars() {
        let path = Path::new("/home/user/project-feature");
        let id = generate_session_id(path);
        assert_eq!(id.len(), 8);
    }

    #[test]
    fn test_generate_session_id_is_hex_only() {
        let path = Path::new("/home/user/project-feature");
        let id = generate_session_id(path);
        assert!(
            id.chars().all(|c| c.is_ascii_hexdigit()),
            "Session ID should be filesystem-safe (hex only): {}",
            id
        );
    }

    #[test]
    fn test_generate_session_id_is_deterministic() {
        let path = Path::new("/home/user/project-feature");
        let id1 = generate_session_id(path);
        let id2 = generate_session_id(path);
        assert_eq!(id1, id2, "Same path should produce same session ID");
    }

    #[test]
    fn test_generate_session_id_different_paths_different_ids() {
        let path1 = Path::new("/home/user/project-feature-a");
        let path2 = Path::new("/home/user/project-feature-b");
        let id1 = generate_session_id(path1);
        let id2 = generate_session_id(path2);
        assert_ne!(
            id1, id2,
            "Different paths should produce different session IDs"
        );
    }

    #[test]
    fn test_generate_session_id_similar_paths() {
        // Test that even similar paths produce different IDs
        let path1 = Path::new("/home/user/project");
        let path2 = Path::new("/home/user/project2");
        let id1 = generate_session_id(path1);
        let id2 = generate_session_id(path2);
        assert_ne!(
            id1, id2,
            "Similar paths should produce different session IDs"
        );
    }

    #[test]
    fn test_generate_session_id_handles_path_with_spaces() {
        let path = Path::new("/home/user/my project/feature branch");
        let id = generate_session_id(path);
        assert_eq!(id.len(), 8);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_generate_session_id_handles_unicode_path() {
        let path = Path::new("/home/user/проект/фича");
        let id = generate_session_id(path);
        assert_eq!(id.len(), 8);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_main_session_id_constant() {
        assert_eq!(MAIN_SESSION_ID, "main");
        assert!(MAIN_SESSION_ID.len() <= 12);
        assert!(MAIN_SESSION_ID.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn test_get_main_session_id() {
        let id = get_main_session_id();
        assert_eq!(id, "main");
    }

    #[test]
    fn test_get_current_session_id_in_main_repo() {
        let _lock = CWD_MUTEX.lock().unwrap();

        // When running in main repo (not a linked worktree)
        if !is_in_worktree().unwrap_or(true) {
            let id = get_current_session_id().unwrap();
            assert_eq!(id, MAIN_SESSION_ID, "Main repo should return 'main' ID");
        }
    }

    #[test]
    fn test_get_current_session_id_returns_valid_id() {
        let _lock = CWD_MUTEX.lock().unwrap();

        let result = get_current_session_id();
        assert!(result.is_ok());

        let id = result.unwrap();
        // ID should be either "main" or an 8-char hex string
        assert!(
            id == MAIN_SESSION_ID || (id.len() == 8 && id.chars().all(|c| c.is_ascii_hexdigit())),
            "Session ID should be 'main' or 8 hex chars: {}",
            id
        );
    }

    #[test]
    fn test_get_current_session_id_is_stable() {
        let _lock = CWD_MUTEX.lock().unwrap();

        // Calling multiple times should return same ID
        let id1 = get_current_session_id().unwrap();
        let id2 = get_current_session_id().unwrap();
        assert_eq!(id1, id2, "Session ID should be stable across calls");
    }

    #[test]
    fn test_session_id_length_is_within_bounds() {
        // Test that all possible session IDs are 8-12 chars (per acceptance criteria)
        let main_id = get_main_session_id();
        assert!(
            main_id.len() >= 4 && main_id.len() <= 12,
            "main ID should be 4-12 chars: {} ({})",
            main_id,
            main_id.len()
        );

        let hash_id = generate_session_id(Path::new("/some/path"));
        assert!(
            hash_id.len() >= 8 && hash_id.len() <= 12,
            "hash ID should be 8-12 chars: {} ({})",
            hash_id,
            hash_id.len()
        );
    }

    #[test]
    fn test_session_id_is_filesystem_safe() {
        // All generated IDs should be safe for use in filenames
        let paths = [
            "/home/user/project",
            "/tmp/worktree-123",
            "C:\\Users\\test\\project",
            "/path/with spaces/and-dashes_underscores",
        ];

        for path in paths {
            let id = generate_session_id(Path::new(path));
            assert!(
                id.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'),
                "ID '{}' from path '{}' should be filesystem-safe",
                id,
                path
            );
        }

        // Main ID should also be safe
        assert!(MAIN_SESSION_ID
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'));
    }

    #[test]
    fn test_get_session_id_for_path_returns_main_for_main_repo() {
        let _lock = CWD_MUTEX.lock().unwrap();

        let main_root = get_main_repo_root().unwrap();
        let id = get_session_id_for_path(&main_root).unwrap();
        assert_eq!(
            id, MAIN_SESSION_ID,
            "Main repo path should return 'main' ID"
        );
    }

    #[test]
    fn test_generate_session_id_uniqueness_sample() {
        // Test a sample of paths to verify uniqueness
        let paths = [
            "/home/user/project1",
            "/home/user/project2",
            "/home/user/project3",
            "/tmp/worktree-a",
            "/tmp/worktree-b",
            "/var/lib/myproject",
            "/opt/work/feature-x",
            "/opt/work/feature-y",
        ];

        let ids: Vec<String> = paths
            .iter()
            .map(|p| generate_session_id(Path::new(p)))
            .collect();

        // Check all IDs are unique
        let unique_ids: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(
            ids.len(),
            unique_ids.len(),
            "All session IDs should be unique. IDs: {:?}",
            ids
        );
    }

    // ========================================================================
    // Git Repo Name tests (US-004)
    // ========================================================================

    #[test]
    fn test_get_git_repo_name_returns_repo_name() {
        let _lock = CWD_MUTEX.lock().unwrap();

        // When running in a git repo, should return Some(repo_name)
        let result = get_git_repo_name();
        assert!(result.is_ok());

        let name = result.unwrap();
        assert!(name.is_some(), "Should return repo name when in git repo");

        let repo_name = name.unwrap();
        assert!(!repo_name.is_empty(), "Repo name should not be empty");
        // The name should be a valid directory name (no path separators)
        assert!(
            !repo_name.contains('/') && !repo_name.contains('\\'),
            "Repo name should not contain path separators: {}",
            repo_name
        );
    }

    #[test]
    fn test_get_git_repo_name_is_consistent_with_main_repo_root() {
        let _lock = CWD_MUTEX.lock().unwrap();

        // The repo name should match the basename of get_main_repo_root()
        let main_root = get_main_repo_root().unwrap();
        let expected_name = main_root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap()
            .to_string();

        let repo_name = get_git_repo_name().unwrap().unwrap();

        assert_eq!(
            repo_name, expected_name,
            "Repo name should match main repo root basename"
        );
    }

    #[test]
    fn test_get_git_repo_name_is_stable() {
        let _lock = CWD_MUTEX.lock().unwrap();

        // Calling multiple times should return the same value
        let name1 = get_git_repo_name().unwrap();
        let name2 = get_git_repo_name().unwrap();
        assert_eq!(name1, name2, "Repo name should be stable across calls");
    }

    #[test]
    fn test_get_git_repo_name_is_filesystem_safe() {
        let _lock = CWD_MUTEX.lock().unwrap();

        let repo_name = get_git_repo_name().unwrap();
        if let Some(name) = repo_name {
            // Name should be filesystem-safe (no special characters that would break paths)
            assert!(
                !name.contains('/'),
                "Repo name should not contain forward slashes"
            );
            assert!(
                !name.contains('\\'),
                "Repo name should not contain backslashes"
            );
            assert!(!name.contains('\0'), "Repo name should not contain NUL");
        }
    }

    // ========================================================================
    // Worktree Path Generation tests (US-007)
    // ========================================================================

    #[test]
    fn test_slugify_branch_name_replaces_slashes() {
        assert_eq!(slugify_branch_name("feature/login"), "feature-login");
        assert_eq!(
            slugify_branch_name("feature/user/auth"),
            "feature-user-auth"
        );
    }

    #[test]
    fn test_slugify_branch_name_preserves_valid_chars() {
        assert_eq!(slugify_branch_name("main"), "main");
        assert_eq!(slugify_branch_name("feature-branch"), "feature-branch");
        assert_eq!(slugify_branch_name("v1.0.0"), "v1.0.0");
        assert_eq!(slugify_branch_name("release_v2"), "release_v2");
    }

    #[test]
    fn test_slugify_branch_name_collapses_multiple_dashes() {
        assert_eq!(slugify_branch_name("feature//login"), "feature-login");
        assert_eq!(slugify_branch_name("a--b---c"), "a-b-c");
    }

    #[test]
    fn test_slugify_branch_name_removes_special_chars() {
        assert_eq!(slugify_branch_name("feature@login"), "feature-login");
        assert_eq!(slugify_branch_name("branch#1"), "branch-1");
        assert_eq!(slugify_branch_name("test:name"), "test-name");
    }

    #[test]
    fn test_generate_worktree_name_default_pattern() {
        let name = generate_worktree_name("{repo}-wt-{branch}", "myproject", "feature/login");
        assert_eq!(name, "myproject-wt-feature-login");
    }

    #[test]
    fn test_generate_worktree_name_custom_pattern() {
        let name = generate_worktree_name("{repo}_worktree_{branch}", "myproject", "main");
        assert_eq!(name, "myproject_worktree_main");
    }

    #[test]
    fn test_generate_worktree_name_only_repo() {
        let name = generate_worktree_name("{repo}-dev", "myproject", "feature/test");
        assert_eq!(name, "myproject-dev");
    }

    #[test]
    fn test_generate_worktree_name_only_branch() {
        let name = generate_worktree_name("wt-{branch}", "myproject", "feature/test");
        assert_eq!(name, "wt-feature-test");
    }

    #[test]
    fn test_generate_worktree_name_complex_branch() {
        let name = generate_worktree_name(
            "{repo}-wt-{branch}",
            "autom8",
            "feature/git-worktrees/us-007",
        );
        assert_eq!(name, "autom8-wt-feature-git-worktrees-us-007");
    }

    #[test]
    fn test_generate_worktree_path_returns_sibling_of_main_repo() {
        let _lock = CWD_MUTEX.lock().unwrap();

        // This test runs in a git repo
        let result = generate_worktree_path("{repo}-wt-{branch}", "feature/test");
        assert!(result.is_ok());

        let path = result.unwrap();
        // Path should end with the generated worktree name
        let path_str = path.to_string_lossy();
        assert!(
            path_str.contains("-wt-feature-test"),
            "Path should contain worktree name: {}",
            path_str
        );

        // Path should be a sibling of the main repo (same parent directory)
        let main_repo = get_main_repo_root().unwrap();
        assert_eq!(
            path.parent(),
            main_repo.parent(),
            "Worktree should be a sibling of main repo"
        );
    }

    #[test]
    fn test_worktree_result_path_method() {
        let path = PathBuf::from("/test/path");
        let created = WorktreeResult::Created(path.clone());
        let reused = WorktreeResult::Reused(path.clone());

        assert_eq!(created.path(), &path);
        assert_eq!(reused.path(), &path);
    }

    #[test]
    fn test_worktree_result_was_created() {
        let path = PathBuf::from("/test/path");
        let created = WorktreeResult::Created(path.clone());
        let reused = WorktreeResult::Reused(path.clone());

        assert!(created.was_created());
        assert!(!reused.was_created());
    }

    #[test]
    fn test_format_worktree_error_already_checked_out() {
        let msg = format_worktree_error(
            "fatal: branch 'main' is already checked out at '/other/path'",
            "main",
            Path::new("/new/worktree"),
        );

        assert!(msg.contains("main"));
        assert!(msg.contains("already checked out"));
        assert!(msg.contains("Suggestions:"));
    }

    #[test]
    fn test_format_worktree_error_already_exists() {
        let msg = format_worktree_error(
            "fatal: '/new/worktree' already exists",
            "feature",
            Path::new("/new/worktree"),
        );

        assert!(msg.contains("already exists"));
        assert!(msg.contains("Suggestions:"));
    }

    #[test]
    fn test_format_worktree_error_permission_denied() {
        let msg = format_worktree_error(
            "error: permission denied",
            "feature",
            Path::new("/restricted/path"),
        );

        assert!(msg.contains("permissions"));
        assert!(msg.contains("Suggestions:"));
    }

    #[test]
    fn test_format_worktree_error_generic() {
        let msg = format_worktree_error("some unknown error", "feature", Path::new("/some/path"));

        assert!(msg.contains("some unknown error"));
        assert!(msg.contains("Suggestions:"));
    }
}
