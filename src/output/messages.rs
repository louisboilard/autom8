//! Basic message output functions.
//!
//! Provides simple error, warning, and info message display.

use super::colors::*;

/// Print an error message.
pub fn print_error(msg: &str) {
    println!("{RED}{BOLD}Error:{RESET} {}", msg);
}

/// Print a warning message.
pub fn print_warning(msg: &str) {
    println!("{YELLOW}Warning:{RESET} {}", msg);
}

/// Print an info message.
pub fn print_info(msg: &str) {
    println!("{CYAN}Info:{RESET} {}", msg);
}

/// Print worktree creation information.
pub fn print_worktree_created(path: &std::path::Path, branch: &str) {
    println!(
        "{GREEN}Worktree created:{RESET} {} (branch: {})",
        path.display(),
        branch
    );
}

/// Print worktree reuse information.
pub fn print_worktree_reused(path: &std::path::Path, branch: &str) {
    println!(
        "{CYAN}Worktree reused:{RESET} {} (branch: {})",
        path.display(),
        branch
    );
}

/// Print worktree context information.
pub fn print_worktree_context(path: &std::path::Path) {
    println!("{CYAN}Working in worktree:{RESET} {}", path.display());
}

/// Print interruption message when the user presses Ctrl+C.
pub fn print_interrupted() {
    println!();
    println!("{YELLOW}Interrupted.{RESET} Run '{CYAN}autom8 resume{RESET}' to continue.");
}

/// Print message when the run is paused (via GUI pause button or Step mode).
pub fn print_paused() {
    println!();
    println!("{YELLOW}Paused.{RESET} Run '{CYAN}autom8 resume{RESET}' to continue.");
}

/// Print message when resuming from an interrupted run.
pub fn print_resuming_interrupted(machine_state: &str) {
    println!(
        "{YELLOW}Previous run was interrupted at {BOLD}{}{RESET}{YELLOW}. Resuming...{RESET}",
        machine_state
    );
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// US-005: Verify print_resuming_interrupted doesn't panic and accepts various machine states.
    #[test]
    fn test_us005_print_resuming_interrupted_smoke() {
        // Test with various machine state strings
        print_resuming_interrupted("RunningClaude");
        print_resuming_interrupted("Reviewing");
        print_resuming_interrupted("Committing");
        print_resuming_interrupted("CreatingPR");
        // Should not panic with any input
    }

    /// US-005: Verify print_resuming_interrupted works with Debug-formatted machine states.
    #[test]
    fn test_us005_print_resuming_interrupted_debug_format() {
        // The function is called with format!("{:?}", state.machine_state)
        // which produces debug-formatted strings
        print_resuming_interrupted("Idle");
        print_resuming_interrupted("Initializing");
        print_resuming_interrupted("PickingStory");
        print_resuming_interrupted("Failed");
        // All should succeed without panic
    }
}
