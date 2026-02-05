//! Claude CLI permission configuration.
//!
//! This module provides functions for building permission arguments
//! for the Claude CLI, replacing `--dangerously-skip-permissions` with
//! a more granular allowlist/denylist approach.

use std::path::Path;

/// Default tools that are broadly allowed (no path restrictions).
const DEFAULT_ALLOWED_TOOLS: &[&str] = &[
    "Bash",      // All bash commands (language-agnostic)
    "Read",      // Read any file (permissive reads)
    "Glob",      // File pattern matching
    "Grep",      // Content search
    "LSP",       // Language server protocol
    "WebFetch",  // Web requests
    "WebSearch", // Web search
];

/// Default dangerous patterns to block.
const DEFAULT_DISALLOWED_TOOLS: &[&str] = &[
    "Bash(rm -rf *)",
    "Bash(sudo *)",
    "Bash(chmod 777 *)",
    "Bash(git push --force *)",
    "Bash(curl * | sh)",
    "Bash(curl * | bash)",
    "Bash(wget * | sh)",
    "Bash(wget * | bash)",
];

/// Build the `--allowedTools` argument value for Claude CLI.
///
/// This includes:
/// - Broad tool access (Bash, Read, Glob, Grep, etc.)
/// - Restricted Edit/Write access to project directories and config
///
/// # Arguments
///
/// * `project_dir` - The root directory of the project being worked on
///
/// # Returns
///
/// A comma-separated string of allowed tools suitable for `--allowedTools`.
pub fn build_allowed_tools(project_dir: &Path) -> String {
    let mut tools: Vec<String> = DEFAULT_ALLOWED_TOOLS
        .iter()
        .map(|s| s.to_string())
        .collect();

    // Get parent directory for worktree support
    let parent_dir = project_dir.parent().unwrap_or(project_dir);

    // Add restricted Edit permissions (project, parent for worktrees, config)
    tools.push(format!("Edit(//{}/**)", project_dir.display()));
    tools.push(format!("Edit(//{}/**)", parent_dir.display()));
    tools.push("Edit(~/.config/autom8/**)".to_string());

    // Add restricted Write permissions (same directories)
    tools.push(format!("Write(//{}/**)", project_dir.display()));
    tools.push(format!("Write(//{}/**)", parent_dir.display()));
    tools.push("Write(~/.config/autom8/**)".to_string());

    tools.join(",")
}

/// Build the `--disallowedTools` argument value for Claude CLI.
///
/// This blocks dangerous command patterns that could cause harm.
///
/// # Returns
///
/// A comma-separated string of disallowed tools suitable for `--disallowedTools`.
pub fn build_disallowed_tools() -> String {
    DEFAULT_DISALLOWED_TOOLS.join(",")
}

/// Build all permission-related arguments for Claude CLI invocation.
///
/// Returns a vector of argument pairs that can be passed to `Command::args()`.
///
/// # Arguments
///
/// * `project_dir` - The root directory of the project being worked on
///
/// # Returns
///
/// A vector of strings: `["--allowedTools", "<tools>", "--disallowedTools", "<patterns>"]`
pub fn build_permission_args(project_dir: &Path) -> Vec<String> {
    vec![
        "--allowedTools".to_string(),
        build_allowed_tools(project_dir),
        "--disallowedTools".to_string(),
        build_disallowed_tools(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_build_allowed_tools_includes_default_tools() {
        let project_dir = PathBuf::from("/Users/test/projects/myproject");
        let allowed = build_allowed_tools(&project_dir);

        assert!(allowed.contains("Bash"));
        assert!(allowed.contains("Read"));
        assert!(allowed.contains("Glob"));
        assert!(allowed.contains("Grep"));
        assert!(allowed.contains("LSP"));
        assert!(allowed.contains("WebFetch"));
    }

    #[test]
    fn test_build_allowed_tools_includes_project_edit() {
        let project_dir = PathBuf::from("/Users/test/projects/myproject");
        let allowed = build_allowed_tools(&project_dir);

        assert!(allowed.contains("Edit(///Users/test/projects/myproject/**)"));
    }

    #[test]
    fn test_build_allowed_tools_includes_parent_edit() {
        let project_dir = PathBuf::from("/Users/test/projects/myproject");
        let allowed = build_allowed_tools(&project_dir);

        // Parent directory for worktree support
        assert!(allowed.contains("Edit(///Users/test/projects/**)"));
    }

    #[test]
    fn test_build_allowed_tools_includes_config_edit() {
        let project_dir = PathBuf::from("/Users/test/projects/myproject");
        let allowed = build_allowed_tools(&project_dir);

        assert!(allowed.contains("Edit(~/.config/autom8/**)"));
    }

    #[test]
    fn test_build_allowed_tools_includes_write_permissions() {
        let project_dir = PathBuf::from("/Users/test/projects/myproject");
        let allowed = build_allowed_tools(&project_dir);

        assert!(allowed.contains("Write(///Users/test/projects/myproject/**)"));
        assert!(allowed.contains("Write(///Users/test/projects/**)"));
        assert!(allowed.contains("Write(~/.config/autom8/**)"));
    }

    #[test]
    fn test_build_disallowed_tools_includes_dangerous_patterns() {
        let disallowed = build_disallowed_tools();

        assert!(disallowed.contains("Bash(rm -rf *)"));
        assert!(disallowed.contains("Bash(sudo *)"));
        assert!(disallowed.contains("Bash(chmod 777 *)"));
        assert!(disallowed.contains("Bash(git push --force *)"));
        assert!(disallowed.contains("Bash(curl * | sh)"));
        assert!(disallowed.contains("Bash(wget * | sh)"));
    }

    #[test]
    fn test_build_permission_args_returns_four_elements() {
        let project_dir = PathBuf::from("/Users/test/projects/myproject");
        let args = build_permission_args(&project_dir);

        assert_eq!(args.len(), 4);
        assert_eq!(args[0], "--allowedTools");
        assert_eq!(args[2], "--disallowedTools");
    }

    #[test]
    fn test_build_permission_args_handles_root_project() {
        // Edge case: project at filesystem root
        let project_dir = PathBuf::from("/myproject");
        let args = build_permission_args(&project_dir);

        // Should not panic, and should have valid args
        assert_eq!(args.len(), 4);
        assert!(args[1].contains("Edit(///myproject/**)"));
    }
}
