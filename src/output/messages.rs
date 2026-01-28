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
