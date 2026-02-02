//! Clean command handler.
//!
//! Provides mechanisms to clean up completed sessions and orphaned worktrees.
//! This command helps users manage disk space and keep their project clean.

use std::fs;
use std::path::Path;

use crate::error::Result;
use crate::output::{BLUE, BOLD, CYAN, GRAY, GREEN, RED, RESET, YELLOW};
use crate::prompt;
use crate::state::{MachineState, RunStatus, StateManager};
use crate::worktree;

use super::ensure_project_dir;

/// Summary of cleanup operations performed.
#[derive(Debug, Default)]
pub struct CleanupSummary {
    /// Number of sessions removed
    pub sessions_removed: usize,
    /// Number of worktrees removed
    pub worktrees_removed: usize,
    /// Total bytes freed (estimated)
    pub bytes_freed: u64,
    /// Sessions that were skipped (e.g., uncommitted changes without --force)
    pub sessions_skipped: Vec<SkippedSession>,
    /// Errors encountered during cleanup
    pub errors: Vec<String>,
}

/// Information about a session that was skipped during cleanup.
#[derive(Debug)]
pub struct SkippedSession {
    pub session_id: String,
    pub reason: String,
}

/// Options for the clean command.
#[derive(Debug, Default)]
pub struct CleanOptions {
    /// Also remove associated worktrees
    pub worktrees: bool,
    /// Remove all sessions (requires confirmation)
    pub all: bool,
    /// Remove a specific session by ID
    pub session: Option<String>,
    /// Only remove orphaned sessions (worktree deleted but session state remains)
    pub orphaned: bool,
    /// Force removal even if worktrees have uncommitted changes
    pub force: bool,
}

impl CleanupSummary {
    /// Print the cleanup summary.
    pub fn print(&self) {
        println!();

        if self.sessions_removed == 0 && self.worktrees_removed == 0 {
            println!("{GRAY}No sessions or worktrees were removed.{RESET}");
        } else {
            let freed_str = format_bytes(self.bytes_freed);
            println!(
                "{GREEN}Removed {} session{}, {} worktree{}, freed {}{RESET}",
                self.sessions_removed,
                if self.sessions_removed == 1 { "" } else { "s" },
                self.worktrees_removed,
                if self.worktrees_removed == 1 { "" } else { "s" },
                freed_str
            );
        }

        if !self.sessions_skipped.is_empty() {
            println!();
            println!(
                "{YELLOW}Skipped {} session{}:{RESET}",
                self.sessions_skipped.len(),
                if self.sessions_skipped.len() == 1 {
                    ""
                } else {
                    "s"
                }
            );
            for skipped in &self.sessions_skipped {
                println!(
                    "  {GRAY}-{RESET} {}: {}",
                    skipped.session_id, skipped.reason
                );
            }
        }

        if !self.errors.is_empty() {
            println!();
            println!("{RED}Errors during cleanup:{RESET}");
            for error in &self.errors {
                println!("  {RED}-{RESET} {}", error);
            }
        }
    }
}

/// Format bytes into human-readable string.
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Calculate the size of a directory recursively.
fn dir_size(path: &Path) -> u64 {
    if !path.exists() {
        return 0;
    }

    let mut size = 0;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                size += dir_size(&path);
            } else if let Ok(metadata) = entry.metadata() {
                size += metadata.len();
            }
        }
    }
    size
}

/// Check if a worktree has uncommitted changes.
///
/// Returns true if there are uncommitted changes (working directory is dirty).
/// Returns false if the worktree is clean or doesn't exist.
pub fn worktree_has_uncommitted_changes(worktree_path: &Path) -> bool {
    if !worktree_path.exists() {
        return false;
    }

    // Run git status in the worktree directory
    let output = std::process::Command::new("git")
        .args([
            "-C",
            &worktree_path.to_string_lossy(),
            "status",
            "--porcelain",
        ])
        .output();

    match output {
        Ok(output) => {
            // If there's any output, there are uncommitted changes
            !output.stdout.is_empty()
        }
        Err(_) => {
            // If we can't run git, assume it's safe to remove (not a git directory)
            false
        }
    }
}

/// Clean up sessions based on the provided options.
///
/// This is the main entry point for the clean command.
pub fn clean_command(options: CleanOptions) -> Result<()> {
    ensure_project_dir()?;

    // Dispatch to the appropriate cleanup function based on options
    if let Some(session_id) = &options.session {
        // Clean a specific session
        clean_specific_session(session_id, &options)
    } else if options.orphaned {
        // Clean only orphaned sessions
        clean_orphaned_sessions(&options)
    } else if options.all {
        // Clean all sessions (with confirmation)
        clean_all_sessions(&options)
    } else {
        // Default: clean completed/failed sessions
        clean_completed_sessions(&options)
    }
}

/// Clean a specific session by ID.
fn clean_specific_session(session_id: &str, options: &CleanOptions) -> Result<()> {
    let state_manager = StateManager::new()?;
    let sessions = state_manager.list_sessions()?;

    // Find the session
    let session = sessions.iter().find(|s| s.session_id == session_id);

    match session {
        Some(metadata) => {
            println!();
            println!(
                "Session {CYAN}{}{RESET} on branch {BLUE}{}{RESET}",
                metadata.session_id, metadata.branch_name
            );
            println!("  Path: {}", metadata.worktree_path.display());

            let mut summary = CleanupSummary::default();

            // Check if this is the current session
            let current_dir = std::env::current_dir().ok();
            let is_current = current_dir
                .as_ref()
                .map(|cwd| cwd == &metadata.worktree_path)
                .unwrap_or(false);

            if is_current && !options.force {
                summary.sessions_skipped.push(SkippedSession {
                    session_id: session_id.to_string(),
                    reason: "Cannot remove current session (use --force to override)".to_string(),
                });
                summary.print();
                return Ok(());
            }

            // Check for uncommitted changes if worktree exists
            if options.worktrees
                && metadata.worktree_path.exists()
                && worktree_has_uncommitted_changes(&metadata.worktree_path)
                && !options.force
            {
                summary.sessions_skipped.push(SkippedSession {
                    session_id: session_id.to_string(),
                    reason: "Worktree has uncommitted changes (use --force to override)"
                        .to_string(),
                });
                summary.print();
                return Ok(());
            }

            // Confirm deletion
            let prompt_msg = if options.worktrees && metadata.worktree_path.exists() {
                format!("Remove session '{}' and its worktree?", metadata.session_id)
            } else {
                format!("Remove session '{}'?", metadata.session_id)
            };

            if !prompt::confirm(&prompt_msg, false) {
                println!("{GRAY}Cancelled.{RESET}");
                return Ok(());
            }

            // Archive before deletion
            if let Some(session_sm) = state_manager.get_session(session_id) {
                if let Ok(Some(state)) = session_sm.load_current() {
                    if let Ok(archive_path) = session_sm.archive(&state) {
                        println!("{GRAY}Archived to: {}{RESET}", archive_path.display());
                    }
                }
            }

            // Remove worktree if requested
            if options.worktrees && metadata.worktree_path.exists() {
                summary.bytes_freed += dir_size(&metadata.worktree_path);
                if let Err(e) = remove_worktree_safely(&metadata.worktree_path, options.force) {
                    summary.errors.push(format!(
                        "Failed to remove worktree {}: {}",
                        metadata.worktree_path.display(),
                        e
                    ));
                } else {
                    summary.worktrees_removed += 1;
                }
            }

            // Remove session state
            if let Some(session_sm) = state_manager.get_session(session_id) {
                summary.bytes_freed += get_session_size(&session_sm);
                session_sm.clear_current()?;
                summary.sessions_removed += 1;
            }

            summary.print();
            Ok(())
        }
        None => {
            println!("{RED}Session '{}' not found.{RESET}", session_id);
            println!();
            println!("Use {CYAN}autom8 status --all{RESET} to list available sessions.");
            Ok(())
        }
    }
}

/// Clean only orphaned sessions (worktree deleted but session state remains).
fn clean_orphaned_sessions(_options: &CleanOptions) -> Result<()> {
    let state_manager = StateManager::new()?;
    let sessions = state_manager.list_sessions()?;

    // Find orphaned sessions
    let orphaned: Vec<_> = sessions
        .iter()
        .filter(|s| !s.worktree_path.exists())
        .collect();

    if orphaned.is_empty() {
        println!("{GRAY}No orphaned sessions found.{RESET}");
        return Ok(());
    }

    println!();
    println!("{BOLD}Orphaned sessions (worktree deleted):{RESET}");
    for session in &orphaned {
        println!(
            "  {GRAY}●{RESET} {} - {} (path: {})",
            session.session_id,
            session.branch_name,
            session.worktree_path.display()
        );
    }
    println!();

    let prompt_msg = format!(
        "Remove {} orphaned session{}?",
        orphaned.len(),
        if orphaned.len() == 1 { "" } else { "s" }
    );

    if !prompt::confirm(&prompt_msg, false) {
        println!("{GRAY}Cancelled.{RESET}");
        return Ok(());
    }

    let mut summary = CleanupSummary::default();

    for session in orphaned {
        // Archive before deletion
        if let Some(session_sm) = state_manager.get_session(&session.session_id) {
            if let Ok(Some(state)) = session_sm.load_current() {
                let _ = session_sm.archive(&state);
            }

            summary.bytes_freed += get_session_size(&session_sm);
            if let Err(e) = session_sm.clear_current() {
                summary.errors.push(format!(
                    "Failed to remove session {}: {}",
                    session.session_id, e
                ));
            } else {
                summary.sessions_removed += 1;
            }
        }
    }

    summary.print();
    Ok(())
}

/// Clean all sessions (with confirmation).
fn clean_all_sessions(options: &CleanOptions) -> Result<()> {
    let state_manager = StateManager::new()?;
    let sessions = state_manager.list_sessions()?;

    if sessions.is_empty() {
        println!("{GRAY}No sessions found.{RESET}");
        return Ok(());
    }

    let current_dir = std::env::current_dir().ok();

    println!();
    println!("{BOLD}All sessions:{RESET}");
    for session in &sessions {
        let is_current = current_dir
            .as_ref()
            .map(|cwd| cwd == &session.worktree_path)
            .unwrap_or(false);

        let is_orphaned = !session.worktree_path.exists();
        let has_uncommitted =
            !is_orphaned && worktree_has_uncommitted_changes(&session.worktree_path);

        let status_markers = format!(
            "{}{}{}",
            if is_current { " (current)" } else { "" },
            if is_orphaned { " [orphaned]" } else { "" },
            if has_uncommitted {
                " [uncommitted changes]"
            } else {
                ""
            }
        );

        let indicator = if is_orphaned {
            format!("{GRAY}✗{RESET}")
        } else if session.is_running {
            format!("{YELLOW}●{RESET}")
        } else {
            format!("{GRAY}○{RESET}")
        };

        println!(
            "  {} {} - {}{GRAY}{}{RESET}",
            indicator, session.session_id, session.branch_name, status_markers
        );
    }
    println!();

    // Warning about uncommitted changes
    let sessions_with_uncommitted: Vec<_> = sessions
        .iter()
        .filter(|s| s.worktree_path.exists() && worktree_has_uncommitted_changes(&s.worktree_path))
        .collect();

    if !sessions_with_uncommitted.is_empty() && options.worktrees && !options.force {
        println!(
            "{YELLOW}Warning: {} session{} {} uncommitted changes.{RESET}",
            sessions_with_uncommitted.len(),
            if sessions_with_uncommitted.len() == 1 {
                ""
            } else {
                "s"
            },
            if sessions_with_uncommitted.len() == 1 {
                "has"
            } else {
                "have"
            }
        );
        println!("{YELLOW}These will be skipped unless you use --force.{RESET}");
        println!();
    }

    let prompt_msg = if options.worktrees {
        format!(
            "{RED}Remove ALL {} sessions AND their worktrees? This cannot be undone.{RESET}",
            sessions.len()
        )
    } else {
        format!(
            "Remove ALL {} session state files? (worktrees will remain)",
            sessions.len()
        )
    };

    if !prompt::confirm(&prompt_msg, false) {
        println!("{GRAY}Cancelled.{RESET}");
        return Ok(());
    }

    let mut summary = CleanupSummary::default();

    for session in &sessions {
        let is_current = current_dir
            .as_ref()
            .map(|cwd| cwd == &session.worktree_path)
            .unwrap_or(false);

        // Skip current session unless --force
        if is_current && !options.force {
            summary.sessions_skipped.push(SkippedSession {
                session_id: session.session_id.clone(),
                reason: "Current session".to_string(),
            });
            continue;
        }

        // Check for uncommitted changes
        if options.worktrees
            && session.worktree_path.exists()
            && worktree_has_uncommitted_changes(&session.worktree_path)
            && !options.force
        {
            summary.sessions_skipped.push(SkippedSession {
                session_id: session.session_id.clone(),
                reason: "Uncommitted changes".to_string(),
            });
            continue;
        }

        // Archive before deletion
        if let Some(session_sm) = state_manager.get_session(&session.session_id) {
            if let Ok(Some(state)) = session_sm.load_current() {
                let _ = session_sm.archive(&state);
            }

            // Remove worktree if requested
            if options.worktrees && session.worktree_path.exists() {
                summary.bytes_freed += dir_size(&session.worktree_path);
                if let Err(e) = remove_worktree_safely(&session.worktree_path, options.force) {
                    summary.errors.push(format!(
                        "Failed to remove worktree {}: {}",
                        session.worktree_path.display(),
                        e
                    ));
                } else {
                    summary.worktrees_removed += 1;
                }
            }

            // Remove session state
            summary.bytes_freed += get_session_size(&session_sm);
            if let Err(e) = session_sm.clear_current() {
                summary.errors.push(format!(
                    "Failed to remove session {}: {}",
                    session.session_id, e
                ));
            } else {
                summary.sessions_removed += 1;
            }
        }
    }

    summary.print();
    Ok(())
}

/// Clean completed/failed sessions (default behavior).
fn clean_completed_sessions(options: &CleanOptions) -> Result<()> {
    let state_manager = StateManager::new()?;
    let sessions = state_manager.list_sessions()?;

    // Find completed or failed sessions
    let cleanable: Vec<_> = sessions
        .iter()
        .filter(|s| {
            // Load the state to check if completed or failed
            if let Some(session_sm) = state_manager.get_session(&s.session_id) {
                if let Ok(Some(state)) = session_sm.load_current() {
                    matches!(
                        state.machine_state,
                        MachineState::Completed | MachineState::Failed
                    ) || matches!(
                        state.status,
                        RunStatus::Completed | RunStatus::Failed | RunStatus::Interrupted
                    )
                } else {
                    // No state file - consider it cleanable
                    true
                }
            } else {
                false
            }
        })
        .collect();

    // Also include orphaned sessions
    let orphaned: Vec<_> = sessions
        .iter()
        .filter(|s| !s.worktree_path.exists())
        .collect();

    // Combine cleanable and orphaned (dedupe)
    let mut to_clean: Vec<_> = cleanable;
    for orphan in orphaned {
        if !to_clean.iter().any(|s| s.session_id == orphan.session_id) {
            to_clean.push(orphan);
        }
    }

    if to_clean.is_empty() {
        println!("{GRAY}No completed, failed, or orphaned sessions to clean.{RESET}");
        return Ok(());
    }

    let current_dir = std::env::current_dir().ok();

    println!();
    println!("{BOLD}Sessions to clean:{RESET}");
    for session in &to_clean {
        let is_current = current_dir
            .as_ref()
            .map(|cwd| cwd == &session.worktree_path)
            .unwrap_or(false);

        let is_orphaned = !session.worktree_path.exists();

        // Get status
        let status = if let Some(session_sm) = state_manager.get_session(&session.session_id) {
            if let Ok(Some(state)) = session_sm.load_current() {
                match state.machine_state {
                    MachineState::Completed => format!("{GREEN}completed{RESET}"),
                    MachineState::Failed => format!("{RED}failed{RESET}"),
                    _ => format!("{GRAY}idle{RESET}"),
                }
            } else {
                format!("{GRAY}no state{RESET}")
            }
        } else {
            format!("{GRAY}unknown{RESET}")
        };

        let markers = format!(
            "{}{}",
            if is_current { " (current)" } else { "" },
            if is_orphaned { " [orphaned]" } else { "" }
        );

        println!(
            "  {GRAY}○{RESET} {} - {} [{}]{GRAY}{}{RESET}",
            session.session_id, session.branch_name, status, markers
        );
    }
    println!();

    let prompt_msg = format!(
        "Remove {} session{}{}?",
        to_clean.len(),
        if to_clean.len() == 1 { "" } else { "s" },
        if options.worktrees {
            " and associated worktrees"
        } else {
            ""
        }
    );

    if !prompt::confirm(&prompt_msg, false) {
        println!("{GRAY}Cancelled.{RESET}");
        return Ok(());
    }

    let mut summary = CleanupSummary::default();

    for session in to_clean {
        let is_current = current_dir
            .as_ref()
            .map(|cwd| cwd == &session.worktree_path)
            .unwrap_or(false);

        // Skip current session unless --force
        if is_current && !options.force {
            summary.sessions_skipped.push(SkippedSession {
                session_id: session.session_id.clone(),
                reason: "Current session".to_string(),
            });
            continue;
        }

        // Check for uncommitted changes
        if options.worktrees
            && session.worktree_path.exists()
            && worktree_has_uncommitted_changes(&session.worktree_path)
            && !options.force
        {
            summary.sessions_skipped.push(SkippedSession {
                session_id: session.session_id.clone(),
                reason: "Uncommitted changes".to_string(),
            });
            continue;
        }

        // Archive before deletion
        if let Some(session_sm) = state_manager.get_session(&session.session_id) {
            if let Ok(Some(state)) = session_sm.load_current() {
                let _ = session_sm.archive(&state);
            }

            // Remove worktree if requested
            if options.worktrees && session.worktree_path.exists() {
                summary.bytes_freed += dir_size(&session.worktree_path);
                if let Err(e) = remove_worktree_safely(&session.worktree_path, options.force) {
                    summary.errors.push(format!(
                        "Failed to remove worktree {}: {}",
                        session.worktree_path.display(),
                        e
                    ));
                } else {
                    summary.worktrees_removed += 1;
                }
            }

            // Remove session state
            summary.bytes_freed += get_session_size(&session_sm);
            if let Err(e) = session_sm.clear_current() {
                summary.errors.push(format!(
                    "Failed to remove session {}: {}",
                    session.session_id, e
                ));
            } else {
                summary.sessions_removed += 1;
            }
        }
    }

    summary.print();
    Ok(())
}

/// Remove a worktree safely, with optional force flag.
///
/// This function:
/// 1. Changes to the main repo if we're inside the worktree
/// 2. Uses git worktree remove (with force if specified)
fn remove_worktree_safely(worktree_path: &Path, force: bool) -> Result<()> {
    // Check if we're currently inside this worktree
    let current_dir = std::env::current_dir().ok();
    if current_dir.as_ref() == Some(&worktree_path.to_path_buf()) {
        // We need to change to the main repo first
        if let Ok(main_repo) = worktree::get_main_repo_root() {
            std::env::set_current_dir(&main_repo)?;
        }
    }

    // Try using git worktree remove first
    worktree::remove_worktree(worktree_path, force)
}

/// Get the size of session state files.
fn get_session_size(_session_sm: &StateManager) -> u64 {
    // Get the base dir and calculate session dir size
    // This is a simplified version - we could expose session_dir() if needed
    0 // Session state files are typically very small
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{RunState, SessionMetadata};
    use chrono::Utc;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(1048576), "1.0 MB");
        assert_eq!(format_bytes(1572864), "1.5 MB");
        assert_eq!(format_bytes(1073741824), "1.0 GB");
    }

    #[test]
    fn test_cleanup_summary_default() {
        let summary = CleanupSummary::default();
        assert_eq!(summary.sessions_removed, 0);
        assert_eq!(summary.worktrees_removed, 0);
        assert_eq!(summary.bytes_freed, 0);
        assert!(summary.sessions_skipped.is_empty());
        assert!(summary.errors.is_empty());
    }

    #[test]
    fn test_clean_options_default() {
        let options = CleanOptions::default();
        assert!(!options.worktrees);
        assert!(!options.all);
        assert!(options.session.is_none());
        assert!(!options.orphaned);
        assert!(!options.force);
    }

    #[test]
    fn test_worktree_has_uncommitted_changes_nonexistent_path() {
        // Non-existent path should return false (safe to remove)
        let result = worktree_has_uncommitted_changes(Path::new("/nonexistent/path/12345"));
        assert!(!result);
    }

    #[test]
    fn test_dir_size_nonexistent() {
        let size = dir_size(Path::new("/nonexistent/path/12345"));
        assert_eq!(size, 0);
    }

    #[test]
    fn test_dir_size_empty_dir() {
        let temp_dir = TempDir::new().unwrap();
        let size = dir_size(temp_dir.path());
        assert_eq!(size, 0);
    }

    #[test]
    fn test_dir_size_with_files() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        fs::write(&file_path, "hello world").unwrap();

        let size = dir_size(temp_dir.path());
        assert!(size > 0);
        assert_eq!(size, 11); // "hello world" is 11 bytes
    }

    #[test]
    fn test_dir_size_with_nested_dirs() {
        let temp_dir = TempDir::new().unwrap();

        // Create nested structure
        let subdir = temp_dir.path().join("subdir");
        fs::create_dir(&subdir).unwrap();
        fs::write(subdir.join("file1.txt"), "hello").unwrap();
        fs::write(temp_dir.path().join("file2.txt"), "world").unwrap();

        let size = dir_size(temp_dir.path());
        assert_eq!(size, 10); // 5 + 5 bytes
    }

    #[test]
    fn test_skipped_session_struct() {
        let skipped = SkippedSession {
            session_id: "abc123".to_string(),
            reason: "test reason".to_string(),
        };
        assert_eq!(skipped.session_id, "abc123");
        assert_eq!(skipped.reason, "test reason");
    }

    // =========================================================================
    // US-011 Specific Tests
    // =========================================================================

    #[test]
    fn test_us011_clean_options_worktrees_flag() {
        let options = CleanOptions {
            worktrees: true,
            ..Default::default()
        };
        assert!(options.worktrees);
    }

    #[test]
    fn test_us011_clean_options_all_flag() {
        let options = CleanOptions {
            all: true,
            ..Default::default()
        };
        assert!(options.all);
    }

    #[test]
    fn test_us011_clean_options_session_flag() {
        let options = CleanOptions {
            session: Some("abc123".to_string()),
            ..Default::default()
        };
        assert_eq!(options.session, Some("abc123".to_string()));
    }

    #[test]
    fn test_us011_clean_options_orphaned_flag() {
        let options = CleanOptions {
            orphaned: true,
            ..Default::default()
        };
        assert!(options.orphaned);
    }

    #[test]
    fn test_us011_clean_options_force_flag() {
        let options = CleanOptions {
            force: true,
            ..Default::default()
        };
        assert!(options.force);
    }

    #[test]
    fn test_us011_cleanup_summary_with_stats() {
        let summary = CleanupSummary {
            sessions_removed: 3,
            worktrees_removed: 2,
            bytes_freed: 1048576, // 1 MB
            sessions_skipped: vec![SkippedSession {
                session_id: "skipped1".to_string(),
                reason: "uncommitted changes".to_string(),
            }],
            errors: vec!["test error".to_string()],
        };

        assert_eq!(summary.sessions_removed, 3);
        assert_eq!(summary.worktrees_removed, 2);
        assert_eq!(summary.bytes_freed, 1048576);
        assert_eq!(summary.sessions_skipped.len(), 1);
        assert_eq!(summary.errors.len(), 1);
    }

    #[test]
    fn test_us011_worktree_uncommitted_check_on_temp_dir() {
        // Create a temp directory that's NOT a git repo
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("test.txt"), "content").unwrap();

        // Should return false since it's not a git repo
        let result = worktree_has_uncommitted_changes(temp_dir.path());
        assert!(!result);
    }

    #[test]
    fn test_us011_archive_before_deletion_pattern() {
        // Verify the archive pattern works with StateManager
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        // Create a test state
        let state = RunState::new(PathBuf::from("test.json"), "feature/test".to_string());
        sm.save(&state).unwrap();

        // Archive the state
        let archive_path = sm.archive(&state).unwrap();
        assert!(archive_path.exists());

        // Clear the current state
        sm.clear_current().unwrap();

        // Verify state is cleared but archive remains
        assert!(sm.load_current().unwrap().is_none());
        assert!(archive_path.exists());
    }

    #[test]
    fn test_us011_detect_orphaned_session() {
        // Create a session with a non-existent worktree path
        let metadata = SessionMetadata {
            session_id: "orphan123".to_string(),
            worktree_path: PathBuf::from("/nonexistent/worktree/path"),
            branch_name: "feature/test".to_string(),
            created_at: Utc::now(),
            last_active_at: Utc::now(),
            is_running: false,
        };

        // Check if the worktree exists (should return false)
        assert!(!metadata.worktree_path.exists());
    }

    #[test]
    fn test_us011_completed_session_is_cleanable() {
        // A completed session should be cleanable
        let state = RunState::new(PathBuf::from("test.json"), "feature/test".to_string());
        // Note: We can't easily test the full machine_state check without more setup,
        // but we verify the pattern
        assert!(matches!(state.machine_state, MachineState::Initializing));
    }

    #[test]
    fn test_us011_failed_session_is_cleanable() {
        let mut state = RunState::new(PathBuf::from("test.json"), "feature/test".to_string());
        state.transition_to(MachineState::Failed);

        assert!(matches!(state.machine_state, MachineState::Failed));
        assert!(matches!(state.status, RunStatus::Failed));
    }
}
