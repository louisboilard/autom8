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
    println!(
        "{YELLOW}Interrupted.{RESET} Run '{CYAN}autom8 resume{RESET}' to continue."
    );
}
