use std::io::{self, Write};

use crate::output::{BOLD, CYAN, GRAY, GREEN, RESET, YELLOW};

/// Ask a yes/no question and return the user's choice
pub fn confirm(question: &str, default: bool) -> bool {
    let hint = if default { "[Y/n]" } else { "[y/N]" };
    print!("{CYAN}?{RESET} {} {GRAY}{}{RESET} ", question, hint);
    io::stdout().flush().unwrap();

    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() {
        return default;
    }

    match input.trim().to_lowercase().as_str() {
        "y" | "yes" => true,
        "n" | "no" => false,
        "" => default,
        _ => default,
    }
}

/// Ask user to select from a list of options
/// Returns the index of the selected option (0-based)
pub fn select(question: &str, options: &[&str], default: usize) -> usize {
    println!("{CYAN}?{RESET} {}", question);
    println!();

    for (i, option) in options.iter().enumerate() {
        let marker = if i == default {
            format!("{GREEN}>{RESET}")
        } else {
            " ".to_string()
        };
        let num = i + 1;
        println!("  {} {BOLD}{}{RESET}. {}", marker, num, option);
    }

    loop {
        println!();
        print!("{GRAY}Enter choice [{}]:{RESET} ", default + 1);
        io::stdout().flush().unwrap();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            return default;
        }

        let trimmed = input.trim();

        // Empty input = use default
        if trimmed.is_empty() {
            return default;
        }

        // Try to parse as number
        match trimmed.parse::<usize>() {
            Ok(n) if n >= 1 && n <= options.len() => return n - 1,
            _ => {
                println!(
                    "{YELLOW}Please enter a number between 1 and {}{RESET}",
                    options.len()
                );
            }
        }
    }
}

/// Print a status message with a state indicator
pub fn print_status(state: &str, message: &str) {
    println!("{YELLOW}[{}]{RESET} {}", state, message);
}

/// Print a found file message
pub fn print_found(file_type: &str, path: &str) {
    println!("{GREEN}Found{RESET} {} at {BOLD}{}{RESET}", file_type, path);
}

/// Print info about what will happen
pub fn print_action(message: &str) {
    println!("{CYAN}→{RESET} {}", message);
}
