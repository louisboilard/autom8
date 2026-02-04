//! Resume command handler.
//!
//! Resumes a failed or interrupted autom8 run from its last checkpoint.
//! Supports multi-session resume with --session and --list flags.

use crate::error::{Autom8Error, Result};
use crate::output::{print_header, print_sessions_status, BOLD, CYAN, GRAY, GREEN, RESET, YELLOW};
use crate::prompt;
use crate::state::{RunStatus, SessionStatus, StateManager};
use crate::worktree::is_in_worktree;
use crate::Runner;

use super::ensure_project_dir;

/// Resume an interrupted or failed run.
///
/// Behavior:
/// - `autom8 resume` (no args) resumes current session (based on CWD)
/// - `autom8 resume --session <id>` resumes specific session
/// - `autom8 resume --list` shows resumable sessions (incomplete runs)
/// - If in main repo with multiple incomplete sessions, prompt user to choose
/// - If in worktree, auto-resume that worktree's session
///
/// # Arguments
/// * `session` - Optional session ID to resume
/// * `list` - If true, list resumable sessions instead of resuming
///
/// # Returns
///
/// * `Ok(())` on successful completion
/// * `Err(Autom8Error)` if no state exists or resumption fails
pub fn resume_command(session: Option<&str>, list: bool) -> Result<()> {
    ensure_project_dir()?;
    print_header();

    let state_manager = StateManager::new()?;

    // Handle --list flag
    if list {
        return list_resumable_sessions(&state_manager);
    }

    // Handle --session <id> flag
    if let Some(session_id) = session {
        return resume_specific_session(&state_manager, session_id);
    }

    // Default behavior: auto-detect session to resume
    resume_auto_detect(&state_manager)
}

/// List all resumable sessions (incomplete runs).
fn list_resumable_sessions(state_manager: &StateManager) -> Result<()> {
    let sessions = state_manager.list_sessions_with_status()?;

    // Filter to resumable sessions (incomplete runs - either running or failed states)
    let resumable: Vec<SessionStatus> = sessions.into_iter().filter(is_resumable_session).collect();

    if resumable.is_empty() {
        println!("{GRAY}No resumable sessions found.{RESET}");
        println!();
        println!("A session is resumable when it has an incomplete run (running or failed state).");
        return Ok(());
    }

    println!("{BOLD}Resumable sessions:{RESET}");
    println!();
    print_sessions_status(&resumable);

    println!();
    println!(
        "{GRAY}Use {CYAN}autom8 resume --session <id>{GRAY} to resume a specific session.{RESET}"
    );

    Ok(())
}

/// Resume a specific session by ID.
fn resume_specific_session(state_manager: &StateManager, session_id: &str) -> Result<()> {
    // Check if the session exists
    let session_sm = state_manager
        .get_session(session_id)
        .ok_or_else(|| Autom8Error::StateError(format!("Session '{}' not found", session_id)))?;

    // Load the session metadata to check worktree path
    let metadata = session_sm.load_metadata()?.ok_or_else(|| {
        Autom8Error::StateError(format!("Session '{}' has no metadata", session_id))
    })?;

    // Check if worktree still exists
    if !metadata.worktree_path.exists() {
        return Err(Autom8Error::StateError(format!(
            "Session '{}' worktree was deleted: {}",
            session_id,
            metadata.worktree_path.display()
        )));
    }

    // Check if there's something to resume
    let state = session_sm.load_current()?;
    if state.is_none() {
        return Err(Autom8Error::StateError(format!(
            "Session '{}' has no active run",
            session_id
        )));
    }
    let state = state.unwrap();

    if state.status != RunStatus::Running
        && state.status != RunStatus::Failed
        && state.status != RunStatus::Interrupted
    {
        return Err(Autom8Error::StateError(format!(
            "Session '{}' has no resumable run (status: {:?})",
            session_id, state.status
        )));
    }

    // Change to the worktree directory before resuming
    let current_dir = std::env::current_dir()?;
    if current_dir != metadata.worktree_path {
        println!(
            "{CYAN}Changing to worktree:{RESET} {}",
            metadata.worktree_path.display()
        );
        std::env::set_current_dir(&metadata.worktree_path)?;
    }

    // Create a runner for this session and resume
    println!(
        "{YELLOW}[resume]{RESET} Resuming session {BOLD}{}{RESET} on branch {}",
        session_id, metadata.branch_name
    );
    println!();

    let runner = Runner::new()?;
    runner.resume()
}

/// Auto-detect which session to resume.
fn resume_auto_detect(state_manager: &StateManager) -> Result<()> {
    let sessions = state_manager.list_sessions_with_status()?;
    let current_session_id = state_manager.session_id();

    // Check if we're in a worktree
    let in_worktree = is_in_worktree().unwrap_or(false);

    // Filter to resumable sessions
    let resumable: Vec<&SessionStatus> = sessions
        .iter()
        .filter(|&s| is_resumable_session(s))
        .collect();

    if resumable.is_empty() {
        // No resumable sessions - fall back to smart resume (scanning for incomplete specs)
        println!(
            "{YELLOW}[resume]{RESET} No active sessions found, scanning for incomplete specs..."
        );
        println!();
        let runner = Runner::new()?;
        return runner.resume();
    }

    // If in a worktree, prefer the current session
    if in_worktree {
        // Find the current session
        if let Some(current) = resumable.iter().find(|s| s.is_current) {
            println!(
                "{GREEN}[resume]{RESET} Resuming current worktree session: {}",
                current.metadata.session_id
            );
            println!();
            let runner = Runner::new()?;
            return runner.resume();
        }
    }

    // Check if current session is resumable
    if let Some(current) = resumable
        .iter()
        .find(|s| s.metadata.session_id == current_session_id)
    {
        if current.is_current {
            println!(
                "{GREEN}[resume]{RESET} Resuming current session: {}",
                current.metadata.session_id
            );
            println!();
            let runner = Runner::new()?;
            return runner.resume();
        }
    }

    // In main repo with multiple sessions - prompt user to choose
    if resumable.len() == 1 {
        let session = resumable[0];
        return resume_session_with_change(&session.metadata);
    }

    // Multiple resumable sessions - prompt user
    println!("{BOLD}Multiple resumable sessions found:{RESET}");
    println!();

    let options: Vec<String> = resumable
        .iter()
        .map(|s| {
            let current_marker = if s.is_current { " (current)" } else { "" };
            let stale_marker = if s.is_stale { " [stale]" } else { "" };
            let state_str = s
                .machine_state
                .map(|st| format!(" - {:?}", st))
                .unwrap_or_default();
            format!(
                "{}{}{} [{}]{}",
                s.metadata.session_id,
                current_marker,
                stale_marker,
                s.metadata.branch_name,
                state_str
            )
        })
        .chain(std::iter::once("Exit".to_string()))
        .collect();

    let option_refs: Vec<&str> = options.iter().map(|s| s.as_str()).collect();
    let choice = prompt::select("Which session would you like to resume?", &option_refs, 0);

    // Handle Exit option
    if choice >= resumable.len() {
        println!();
        println!("Exiting.");
        return Ok(());
    }

    let selected = resumable[choice];

    // Check if selected session is stale
    if selected.is_stale {
        return Err(Autom8Error::StateError(format!(
            "Session '{}' worktree was deleted: {}",
            selected.metadata.session_id,
            selected.metadata.worktree_path.display()
        )));
    }

    resume_session_with_change(&selected.metadata)
}

/// Resume a session, changing to its worktree directory if needed.
fn resume_session_with_change(metadata: &crate::state::SessionMetadata) -> Result<()> {
    // Check if worktree still exists
    if !metadata.worktree_path.exists() {
        return Err(Autom8Error::StateError(format!(
            "Session '{}' worktree was deleted: {}",
            metadata.session_id,
            metadata.worktree_path.display()
        )));
    }

    // Change to the worktree directory before resuming
    let current_dir = std::env::current_dir()?;
    if current_dir != metadata.worktree_path {
        println!(
            "{CYAN}Changing to worktree:{RESET} {}",
            metadata.worktree_path.display()
        );
        std::env::set_current_dir(&metadata.worktree_path)?;
    }

    println!(
        "{YELLOW}[resume]{RESET} Resuming session {BOLD}{}{RESET} on branch {}",
        metadata.session_id, metadata.branch_name
    );
    println!();

    // Create a new runner for the new directory/session
    let runner = Runner::new()?;
    runner.resume()
}

/// Check if a session is resumable (has an incomplete run).
fn is_resumable_session(session: &SessionStatus) -> bool {
    // A session is resumable if it has a state with Running or Failed status
    // We don't need the full state here - we can check is_running from metadata
    // But for Failed status, we need to check the actual state
    if session.is_stale {
        return false; // Can't resume stale sessions (deleted worktrees)
    }

    if session.metadata.is_running {
        return true;
    }

    // Check if the machine state indicates a resumable run
    if let Some(state) = &session.machine_state {
        match state {
            crate::state::MachineState::Completed => false,
            crate::state::MachineState::Idle => false,
            _ => true, // Any other state is resumable
        }
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{MachineState, SessionMetadata, SessionStatus};
    use chrono::Utc;
    use std::path::PathBuf;

    fn make_session(
        session_id: &str,
        is_running: bool,
        machine_state: Option<MachineState>,
        is_stale: bool,
    ) -> SessionStatus {
        SessionStatus {
            metadata: SessionMetadata {
                session_id: session_id.to_string(),
                worktree_path: PathBuf::from("/tmp/test"),
                branch_name: "feature/test".to_string(),
                created_at: Utc::now(),
                last_active_at: Utc::now(),
                is_running,
                pause_requested: false,
            },
            machine_state,
            current_story: None,
            is_current: false,
            is_stale,
        }
    }

    #[test]
    fn test_us010_is_resumable_running_session() {
        let session = make_session("test", true, Some(MachineState::RunningClaude), false);
        assert!(is_resumable_session(&session));
    }

    #[test]
    fn test_us010_is_resumable_failed_session() {
        let session = make_session("test", false, Some(MachineState::Failed), false);
        assert!(is_resumable_session(&session));
    }

    #[test]
    fn test_us010_is_not_resumable_completed_session() {
        let session = make_session("test", false, Some(MachineState::Completed), false);
        assert!(!is_resumable_session(&session));
    }

    #[test]
    fn test_us010_is_not_resumable_stale_session() {
        let session = make_session("test", true, Some(MachineState::RunningClaude), true);
        assert!(!is_resumable_session(&session));
    }

    #[test]
    fn test_us010_is_not_resumable_idle_session() {
        let session = make_session("test", false, Some(MachineState::Idle), false);
        assert!(!is_resumable_session(&session));
    }

    #[test]
    fn test_us010_is_resumable_reviewing_session() {
        let session = make_session("test", false, Some(MachineState::Reviewing), false);
        assert!(is_resumable_session(&session));
    }

    #[test]
    fn test_us010_is_not_resumable_no_state() {
        let session = make_session("test", false, None, false);
        assert!(!is_resumable_session(&session));
    }
}
