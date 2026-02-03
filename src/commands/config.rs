//! Config command handler.
//!
//! Displays, modifies, and resets autom8 configuration values.

use crate::config::{global_config_path, project_config_path, Config};
use crate::error::{Autom8Error, Result};
use crate::git::is_git_repo;
use crate::output::{BOLD, CYAN, GRAY, RESET, YELLOW};
use std::fs;

/// Scope for config operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigScope {
    /// Global configuration (~/.config/autom8/config.toml)
    Global,
    /// Project-specific configuration (~/.config/autom8/<project>/config.toml)
    Project,
    /// Both global and project configurations
    Both,
}

/// Display configuration values.
///
/// Shows the configuration in TOML format. When scope is `Both`, displays
/// global config first with a header, then project config with its header.
///
/// # Arguments
///
/// * `scope` - Which configuration(s) to display
///
/// # Returns
///
/// * `Ok(())` on success
/// * `Err(Autom8Error)` if the configuration cannot be read
pub fn config_display_command(scope: ConfigScope) -> Result<()> {
    match scope {
        ConfigScope::Global => display_global_config(),
        ConfigScope::Project => display_project_config(),
        ConfigScope::Both => {
            display_global_config()?;
            println!();
            display_project_config()
        }
    }
}

/// Display the global configuration.
fn display_global_config() -> Result<()> {
    println!("{BOLD}# Global config{RESET}");
    println!("{GRAY}# {}{RESET}", global_config_path()?.display());
    println!();

    let config_path = global_config_path()?;

    if !config_path.exists() {
        println!("{YELLOW}# (file does not exist, using defaults){RESET}");
        println!();
        print_config_as_toml(&Config::default());
        return Ok(());
    }

    let config = load_config_from_path(&config_path)?;
    print_config_as_toml(&config);

    Ok(())
}

/// Display the project configuration.
fn display_project_config() -> Result<()> {
    // Check if we're in a git repo - required for project config
    if !is_git_repo() {
        return Err(Autom8Error::Config(
            "Not in a git repository.\n\n\
            Project configuration requires being inside a git repository.\n\
            Run this command from within a git repository, or use --global to view global config."
                .to_string(),
        ));
    }

    println!("{BOLD}# Project config{RESET}");
    println!("{GRAY}# {}{RESET}", project_config_path()?.display());
    println!();

    let config_path = project_config_path()?;

    if !config_path.exists() {
        println!("{YELLOW}# (file does not exist, using global config or defaults){RESET}");
        println!();
        // Show what would be effective - either global config or defaults
        let effective_config = if global_config_path()?.exists() {
            load_config_from_path(&global_config_path()?)?
        } else {
            Config::default()
        };
        print_config_as_toml(&effective_config);
        return Ok(());
    }

    let config = load_config_from_path(&config_path)?;
    print_config_as_toml(&config);

    Ok(())
}

/// Load a config from a specific path without any fallback logic.
fn load_config_from_path(path: &std::path::Path) -> Result<Config> {
    let content = fs::read_to_string(path)?;
    toml::from_str(&content).map_err(|e| {
        Autom8Error::Config(format!(
            "Failed to parse config file at {:?}: {}",
            path, e
        ))
    })
}

/// Print a Config struct as valid TOML format.
fn print_config_as_toml(config: &Config) {
    println!("{CYAN}review{RESET} = {}", config.review);
    println!("{CYAN}commit{RESET} = {}", config.commit);
    println!("{CYAN}pull_request{RESET} = {}", config.pull_request);
    println!("{CYAN}worktree{RESET} = {}", config.worktree);
    println!(
        "{CYAN}worktree_path_pattern{RESET} = \"{}\"",
        config.worktree_path_pattern
    );
    println!("{CYAN}worktree_cleanup{RESET} = {}", config.worktree_cleanup);
}

/// Convert a Config to a TOML string (for testing).
pub fn config_to_toml_string(config: &Config) -> String {
    format!(
        "review = {}\n\
         commit = {}\n\
         pull_request = {}\n\
         worktree = {}\n\
         worktree_path_pattern = \"{}\"\n\
         worktree_cleanup = {}",
        config.review,
        config.commit,
        config.pull_request,
        config.worktree,
        config.worktree_path_pattern,
        config.worktree_cleanup
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // US-001: Config display tests
    // ========================================================================

    #[test]
    fn test_us001_config_to_toml_produces_valid_toml() {
        let config = Config::default();
        let toml_str = config_to_toml_string(&config);

        // Parse the generated TOML to verify it's valid
        let parsed: std::result::Result<Config, _> = toml::from_str(&toml_str);
        assert!(
            parsed.is_ok(),
            "Generated TOML should be parseable: {:?}",
            parsed.err()
        );

        // Verify the parsed config matches the original
        let parsed_config = parsed.unwrap();
        assert_eq!(parsed_config, config);
    }

    #[test]
    fn test_us001_config_to_toml_includes_all_fields() {
        let config = Config {
            review: false,
            commit: true,
            pull_request: false,
            worktree: false,
            worktree_path_pattern: "custom-{branch}".to_string(),
            worktree_cleanup: true,
        };
        let toml_str = config_to_toml_string(&config);

        assert!(toml_str.contains("review = false"));
        assert!(toml_str.contains("commit = true"));
        assert!(toml_str.contains("pull_request = false"));
        assert!(toml_str.contains("worktree = false"));
        assert!(toml_str.contains("worktree_path_pattern = \"custom-{branch}\""));
        assert!(toml_str.contains("worktree_cleanup = true"));
    }

    #[test]
    fn test_us001_config_to_toml_default_values() {
        let config = Config::default();
        let toml_str = config_to_toml_string(&config);

        // Verify default values are correct
        assert!(toml_str.contains("review = true"));
        assert!(toml_str.contains("commit = true"));
        assert!(toml_str.contains("pull_request = true"));
        assert!(toml_str.contains("worktree = true"));
        assert!(toml_str.contains("worktree_path_pattern = \"{repo}-wt-{branch}\""));
        assert!(toml_str.contains("worktree_cleanup = false"));
    }

    #[test]
    fn test_us001_config_scope_variants() {
        // Verify all scope variants exist and are distinct
        assert_ne!(ConfigScope::Global, ConfigScope::Project);
        assert_ne!(ConfigScope::Global, ConfigScope::Both);
        assert_ne!(ConfigScope::Project, ConfigScope::Both);
    }
}
