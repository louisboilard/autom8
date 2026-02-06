//! Display output types and adapters for autom8.
//!
//! This module provides the `DisplayAdapter` trait for abstracting display output,
//! along with concrete implementations for CLI and TUI contexts.
//!
//! The `DisplayAdapter` trait enables polymorphic display handling, allowing
//! different contexts (CLI, TUI) to provide appropriate user interactions.

use crate::claude::PermissionResult;
use crate::output::{BOLD, CYAN, GRAY, RESET, YELLOW};
use serde_json::Value;
use std::io::{self, Write};

// Re-export types from output module
pub use crate::output::{BannerColor, StoryResult};

// ============================================================================
// DisplayAdapter Trait
// ============================================================================

/// Trait for abstracting display output in different contexts.
///
/// This trait allows the runner to display output and interact with users
/// in a context-appropriate way (CLI vs TUI vs tests).
pub trait DisplayAdapter {
    /// Prompt the user for permission when a blocked operation is attempted.
    ///
    /// This is called when Claude tries to use a tool that was blocked by
    /// the permission system (e.g., `git push` during story implementation).
    ///
    /// # Arguments
    /// * `tool_name` - The name of the tool being requested (e.g., "Bash")
    /// * `tool_input` - The tool's input parameters (e.g., {"command": "git push origin main"})
    ///
    /// # Returns
    /// * `PermissionResult::Allow(None)` - User approved the operation
    /// * `PermissionResult::Allow(Some(value))` - User approved with modified input
    /// * `PermissionResult::Deny(reason)` - User denied the operation
    fn prompt_permission(&self, tool_name: &str, tool_input: &Value) -> PermissionResult;
}

// ============================================================================
// CliDisplay
// ============================================================================

/// CLI display adapter for interactive terminal output.
///
/// This implementation prompts users via stdin/stdout for permission decisions.
#[derive(Debug, Default)]
pub struct CliDisplay;

impl CliDisplay {
    /// Create a new CLI display adapter.
    pub fn new() -> Self {
        Self
    }

    /// Format the tool input for display.
    ///
    /// Extracts the command from Bash tool input, or pretty-prints JSON for other tools.
    fn format_tool_input(&self, tool_name: &str, tool_input: &Value) -> String {
        if tool_name == "Bash" {
            // Extract command from Bash tool input
            if let Some(command) = tool_input.get("command").and_then(|v| v.as_str()) {
                return format!("    {}", command);
            }
        }
        // Fall back to pretty-printed JSON for other tools
        serde_json::to_string_pretty(tool_input).unwrap_or_else(|_| tool_input.to_string())
    }

    /// Get a human-readable description of why an operation might be blocked.
    fn get_block_reason(&self, tool_name: &str, tool_input: &Value) -> &'static str {
        if tool_name == "Bash" {
            if let Some(command) = tool_input.get("command").and_then(|v| v.as_str()) {
                if command.contains("git push") {
                    return "This was blocked because it affects the remote repository.";
                }
            }
        }
        "This operation was blocked by the permission policy."
    }
}

impl DisplayAdapter for CliDisplay {
    fn prompt_permission(&self, tool_name: &str, tool_input: &Value) -> PermissionResult {
        // Format the operation being attempted
        let formatted_input = self.format_tool_input(tool_name, tool_input);
        let block_reason = self.get_block_reason(tool_name, tool_input);

        // Display warning header
        println!();
        println!(
            "{YELLOW}⚠️  Claude wants to use {BOLD}{}{RESET}{YELLOW}:{RESET}",
            tool_name
        );
        println!();
        println!("{}", formatted_input);
        println!();
        println!("{GRAY}{}{RESET}", block_reason);

        // Prompt for confirmation (default: allow on Enter)
        print!("{CYAN}Allow this operation?{RESET} {GRAY}[Y/n]:{RESET} ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            return PermissionResult::Deny("Failed to read user input".to_string());
        }

        match input.trim().to_lowercase().as_str() {
            "n" | "no" => PermissionResult::Deny("User declined the operation".to_string()),
            _ => PermissionResult::Allow(None), // Empty, "y", "yes", or anything else = allow
        }
    }
}

// ============================================================================
// TuiDisplay
// ============================================================================

/// TUI display adapter for the terminal UI monitor.
///
/// Note: The TUI is currently a read-only monitoring interface that doesn't
/// run Claude directly. This implementation provides a fallback that denies
/// all permission requests, as interactive prompts aren't supported in the
/// monitoring context.
///
/// In the future, if the TUI supports running Claude directly, this could
/// be extended to show a modal dialog for permission prompts.
#[derive(Debug, Default)]
pub struct TuiDisplay;

impl TuiDisplay {
    /// Create a new TUI display adapter.
    pub fn new() -> Self {
        Self
    }
}

impl DisplayAdapter for TuiDisplay {
    fn prompt_permission(&self, tool_name: &str, _tool_input: &Value) -> PermissionResult {
        // TUI is a monitoring-only interface, so we can't prompt interactively.
        // Deny by default with a clear message.
        PermissionResult::Deny(format!(
            "Operation '{}' was blocked. Interactive prompts not supported in TUI monitoring mode.",
            tool_name
        ))
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_cli_display_format_bash_command() {
        let display = CliDisplay::new();
        let input = json!({"command": "git push origin main"});
        let formatted = display.format_tool_input("Bash", &input);
        assert_eq!(formatted, "    git push origin main");
    }

    #[test]
    fn test_cli_display_format_bash_no_command() {
        let display = CliDisplay::new();
        let input = json!({"other": "value"});
        let formatted = display.format_tool_input("Bash", &input);
        // Falls back to JSON
        assert!(formatted.contains("other"));
        assert!(formatted.contains("value"));
    }

    #[test]
    fn test_cli_display_format_other_tool() {
        let display = CliDisplay::new();
        let input = json!({"file": "test.rs", "content": "code"});
        let formatted = display.format_tool_input("Write", &input);
        // Should be pretty-printed JSON
        assert!(formatted.contains("file"));
        assert!(formatted.contains("test.rs"));
    }

    #[test]
    fn test_cli_display_block_reason_git_push() {
        let display = CliDisplay::new();
        let input = json!({"command": "git push origin main"});
        let reason = display.get_block_reason("Bash", &input);
        assert!(reason.contains("remote repository"));
    }

    #[test]
    fn test_cli_display_block_reason_other() {
        let display = CliDisplay::new();
        let input = json!({"command": "rm -rf /"});
        let reason = display.get_block_reason("Bash", &input);
        assert!(reason.contains("blocked by the permission policy"));
    }

    #[test]
    fn test_cli_display_block_reason_non_bash() {
        let display = CliDisplay::new();
        let input = json!({"file": "test.rs"});
        let reason = display.get_block_reason("Write", &input);
        assert!(reason.contains("blocked by the permission policy"));
    }

    #[test]
    fn test_tui_display_denies_by_default() {
        let display = TuiDisplay::new();
        let input = json!({"command": "git push"});
        let result = display.prompt_permission("Bash", &input);
        match result {
            PermissionResult::Deny(msg) => {
                assert!(msg.contains("Bash"));
                assert!(msg.contains("not supported"));
            }
            PermissionResult::Allow(_) => panic!("TuiDisplay should deny by default"),
        }
    }

    #[test]
    fn test_cli_display_default() {
        let display = CliDisplay::default();
        // Just verify it constructs without panic
        let _ = display.format_tool_input("Bash", &json!({}));
    }

    #[test]
    fn test_tui_display_default() {
        let display = TuiDisplay::default();
        // Just verify it constructs without panic
        let result = display.prompt_permission("Test", &json!({}));
        assert!(matches!(result, PermissionResult::Deny(_)));
    }
}
