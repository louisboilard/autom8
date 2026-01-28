//! Error panel display.
//!
//! Provides detailed error display with formatted panels.

use super::colors::*;

const ERROR_PANEL_WIDTH: usize = 60;

/// Structured error information for display.
#[derive(Debug, Clone, PartialEq)]
pub struct ErrorDetails {
    /// Category of error (e.g., "Process Failed", "Timeout", "Auth Error")
    pub error_type: String,
    /// User-friendly description of what went wrong
    pub message: String,
    /// Exit code from subprocess, if applicable
    pub exit_code: Option<i32>,
    /// Stderr output from subprocess, if available
    pub stderr: Option<String>,
    /// Which Claude function failed
    pub source: Option<String>,
}

impl ErrorDetails {
    /// Create a new ErrorDetails instance.
    pub fn new(error_type: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error_type: error_type.into(),
            message: message.into(),
            exit_code: None,
            stderr: None,
            source: None,
        }
    }

    /// Set the exit code.
    pub fn with_exit_code(mut self, code: i32) -> Self {
        self.exit_code = Some(code);
        self
    }

    /// Set the stderr output.
    pub fn with_stderr(mut self, stderr: impl Into<String>) -> Self {
        self.stderr = Some(stderr.into());
        self
    }

    /// Set the source function.
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    /// Print this error using the error panel.
    pub fn print_panel(&self) {
        print_error_panel(
            &self.error_type,
            &self.message,
            self.exit_code,
            self.stderr.as_deref(),
        );
    }
}

impl std::fmt::Display for ErrorDetails {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.error_type, self.message)?;

        if let Some(source) = &self.source {
            write!(f, " (source: {})", source)?;
        }

        if let Some(code) = self.exit_code {
            write!(f, " [exit code: {}]", code)?;
        }

        if let Some(stderr) = &self.stderr {
            let trimmed = stderr.trim();
            if !trimmed.is_empty() {
                if let Some(first_line) = trimmed.lines().next() {
                    write!(f, " stderr: {}", first_line)?;
                }
            }
        }

        Ok(())
    }
}

/// Print a dedicated error panel with full error details.
pub fn print_error_panel(
    error_type: &str,
    message: &str,
    exit_code: Option<i32>,
    stderr: Option<&str>,
) {
    let top_border = format!("╔{}╗", "═".repeat(ERROR_PANEL_WIDTH - 2));
    let bottom_border = format!("╚{}╝", "═".repeat(ERROR_PANEL_WIDTH - 2));
    let separator = format!("╟{}╢", "─".repeat(ERROR_PANEL_WIDTH - 2));

    println!("{RED}{BOLD}{}{RESET}", top_border);

    let header = format!(" ERROR: {} ", error_type);
    let header_padding = ERROR_PANEL_WIDTH.saturating_sub(header.len() + 2);
    let left_pad = header_padding / 2;
    let right_pad = header_padding - left_pad;
    println!(
        "{RED}{BOLD}║{}{}{}║{RESET}",
        " ".repeat(left_pad),
        header,
        " ".repeat(right_pad)
    );

    println!("{RED}{}{RESET}", separator);

    print_panel_content("Message", message);

    if let Some(code) = exit_code {
        print_panel_line(&format!("Exit code: {}", code));
    }

    if let Some(err) = stderr {
        let trimmed = err.trim();
        if !trimmed.is_empty() {
            println!("{RED}{}{RESET}", separator);
            print_panel_content("Stderr", trimmed);
        }
    }

    println!("{RED}{BOLD}{}{RESET}", bottom_border);
}

fn print_panel_content(label: &str, content: &str) {
    let max_content_width = ERROR_PANEL_WIDTH - 6;

    print_panel_line(&format!("{}:", label));

    for line in content.lines() {
        if line.len() <= max_content_width {
            print_panel_line(&format!("  {}", line));
        } else {
            let mut remaining = line;
            while !remaining.is_empty() {
                let (chunk, rest) = if remaining.len() <= max_content_width - 2 {
                    (remaining, "")
                } else {
                    let break_at = remaining[..max_content_width - 2]
                        .rfind(|c: char| c.is_whitespace() || c == '/' || c == '\\' || c == ':')
                        .map(|i| i + 1)
                        .unwrap_or(max_content_width - 2);
                    (&remaining[..break_at], &remaining[break_at..])
                };
                print_panel_line(&format!("  {}", chunk));
                remaining = rest;
            }
        }
    }
}

fn print_panel_line(text: &str) {
    let max_width = ERROR_PANEL_WIDTH - 4;
    let display_text = if text.len() > max_width {
        &text[..max_width]
    } else {
        text
    };
    let padding = max_width.saturating_sub(display_text.len());
    println!(
        "{RED}║{RESET} {}{} {RED}║{RESET}",
        display_text,
        " ".repeat(padding)
    );
}
