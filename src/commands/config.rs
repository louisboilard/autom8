//! Config command handler.
//!
//! Displays, modifies, and resets autom8 configuration values.

use crate::config::{
    global_config_path, load_global_config, load_project_config, project_config_path,
    save_global_config, save_project_config, validate_config, Config,
};
use crate::error::{Autom8Error, Result};
use crate::git::is_git_repo;
use crate::output::{BOLD, CYAN, GRAY, GREEN, RESET, YELLOW};
use clap::Subcommand;
use std::fs;

/// Valid configuration keys that can be set via `config set`.
pub const VALID_CONFIG_KEYS: &[&str] = &[
    "review",
    "commit",
    "pull_request",
    "worktree",
    "worktree_path_pattern",
    "worktree_cleanup",
];

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

/// Subcommands for the config command.
#[derive(Subcommand, Debug, Clone)]
pub enum ConfigSubcommand {
    /// Set a configuration value
    #[command(after_help = "EXAMPLES:
    autom8 config set review false            # Disable review step in project config
    autom8 config set --global commit true    # Enable auto-commit globally
    autom8 config set worktree_path_pattern \"{repo}-feature-{branch}\"

VALID KEYS:
    review              - Enable code review step (true/false)
    commit              - Enable auto-commit (true/false)
    pull_request        - Enable auto-PR creation (true/false, requires commit=true)
    worktree            - Enable worktree mode (true/false)
    worktree_path_pattern - Pattern for worktree directory names (string)
    worktree_cleanup    - Auto-cleanup worktrees after completion (true/false)

VALUE FORMATS:
    Boolean: true, false (case-insensitive)
    String:  Quoted or unquoted text

VALIDATION:
    - Setting pull_request=true requires commit=true
    - Invalid keys or values are rejected with an error message")]
    Set {
        /// Set in global config instead of project config
        #[arg(short, long)]
        global: bool,

        /// The configuration key to set
        key: String,

        /// The value to set
        value: String,
    },

    /// Reset configuration to default values
    #[command(after_help = "EXAMPLES:
    autom8 config reset               # Reset project config (with confirmation)
    autom8 config reset --global      # Reset global config (with confirmation)
    autom8 config reset -y            # Reset without confirmation prompt
    autom8 config reset --global -y   # Reset global config without prompting

DEFAULT VALUES:
    review              = true
    commit              = true
    pull_request        = true
    worktree            = true
    worktree_path_pattern = \"{repo}-wt-{branch}\"
    worktree_cleanup    = false

BEHAVIOR:
    - Prompts for confirmation before resetting (unless -y/--yes is used)
    - Overwrites the config file with default values
    - Displays the new configuration after reset
    - If config file doesn't exist, informs you defaults are already in use")]
    Reset {
        /// Reset global config instead of project config
        #[arg(short, long)]
        global: bool,

        /// Skip confirmation prompt
        #[arg(short, long)]
        yes: bool,
    },
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
        Autom8Error::Config(format!("Failed to parse config file at {:?}: {}", path, e))
    })
}

// ============================================================================
// Config Set Command (US-002)
// ============================================================================

/// Set a configuration value.
///
/// Sets a single configuration key to the specified value, with immediate
/// validation. Creates the config file if it doesn't exist.
///
/// # Arguments
///
/// * `key` - The configuration key to set (e.g., "review", "commit")
/// * `value` - The value to set (booleans: "true"/"false", strings as-is)
/// * `global` - If true, sets in global config; otherwise in project config
///
/// # Returns
///
/// * `Ok(())` on success
/// * `Err(Autom8Error)` if the key is invalid, value is invalid, or validation fails
pub fn config_set_command(key: &str, value: &str, global: bool) -> Result<()> {
    // Validate the key
    if !VALID_CONFIG_KEYS.contains(&key) {
        return Err(Autom8Error::Config(format!(
            "Invalid configuration key: '{}'\n\n\
            Valid keys are:\n  - {}\n\n\
            Use 'autom8 config set --help' for more information.",
            key,
            VALID_CONFIG_KEYS.join("\n  - ")
        )));
    }

    // Check if we're in a git repo for project config
    if !global && !is_git_repo() {
        return Err(Autom8Error::Config(
            "Not in a git repository.\n\n\
            Project configuration requires being inside a git repository.\n\
            Either:\n  - Run this command from within a git repository, or\n  - Use --global to set the global config."
                .to_string(),
        ));
    }

    // Load the current config (create default if doesn't exist)
    let mut config = if global {
        load_global_config()?
    } else {
        load_project_config()?
    };

    // Parse and set the value
    set_config_value(&mut config, key, value)?;

    // Validate the resulting configuration
    validate_config(&config).map_err(|e| Autom8Error::Config(e.to_string()))?;

    // Save the config
    let config_type = if global { "global" } else { "project" };
    if global {
        save_global_config(&config)?;
    } else {
        save_project_config(&config)?;
    }

    // Print confirmation
    let display_value = format_value_for_display(key, &config);
    println!(
        "{GREEN}Set {CYAN}{key}{RESET} = {display_value} in {config_type} config{RESET}"
    );

    Ok(())
}

/// Set a configuration value on a Config struct.
///
/// Parses the string value and sets the appropriate field.
fn set_config_value(config: &mut Config, key: &str, value: &str) -> Result<()> {
    match key {
        "review" => {
            config.review = parse_bool_value(value, key)?;
        }
        "commit" => {
            config.commit = parse_bool_value(value, key)?;
        }
        "pull_request" => {
            config.pull_request = parse_bool_value(value, key)?;
        }
        "worktree" => {
            config.worktree = parse_bool_value(value, key)?;
        }
        "worktree_path_pattern" => {
            config.worktree_path_pattern = value.to_string();
        }
        "worktree_cleanup" => {
            config.worktree_cleanup = parse_bool_value(value, key)?;
        }
        _ => {
            // This shouldn't happen if VALID_CONFIG_KEYS is kept in sync
            return Err(Autom8Error::Config(format!("Unknown key: {}", key)));
        }
    }
    Ok(())
}

/// Parse a boolean value from a string (case-insensitive).
fn parse_bool_value(value: &str, key: &str) -> Result<bool> {
    match value.to_lowercase().as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(Autom8Error::Config(format!(
            "Invalid value for '{}': expected boolean (true/false), got '{}'",
            key, value
        ))),
    }
}

/// Format a config value for display in the confirmation message.
fn format_value_for_display(key: &str, config: &Config) -> String {
    match key {
        "review" => config.review.to_string(),
        "commit" => config.commit.to_string(),
        "pull_request" => config.pull_request.to_string(),
        "worktree" => config.worktree.to_string(),
        "worktree_path_pattern" => format!("\"{}\"", config.worktree_path_pattern),
        "worktree_cleanup" => config.worktree_cleanup.to_string(),
        _ => "unknown".to_string(),
    }
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
    println!(
        "{CYAN}worktree_cleanup{RESET} = {}",
        config.worktree_cleanup
    );
}

/// Convert a Config to a TOML string (for testing).
#[cfg(test)]
fn config_to_toml_string(config: &Config) -> String {
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

    // ========================================================================
    // US-002: Config set tests
    // ========================================================================

    #[test]
    fn test_us002_valid_config_keys_constant() {
        // Verify all expected keys are in the VALID_CONFIG_KEYS constant
        assert!(VALID_CONFIG_KEYS.contains(&"review"));
        assert!(VALID_CONFIG_KEYS.contains(&"commit"));
        assert!(VALID_CONFIG_KEYS.contains(&"pull_request"));
        assert!(VALID_CONFIG_KEYS.contains(&"worktree"));
        assert!(VALID_CONFIG_KEYS.contains(&"worktree_path_pattern"));
        assert!(VALID_CONFIG_KEYS.contains(&"worktree_cleanup"));
        assert_eq!(VALID_CONFIG_KEYS.len(), 6, "Should have exactly 6 valid keys");
    }

    #[test]
    fn test_us002_parse_bool_value_true() {
        // Test various true spellings (case-insensitive)
        assert!(parse_bool_value("true", "test").unwrap());
        assert!(parse_bool_value("TRUE", "test").unwrap());
        assert!(parse_bool_value("True", "test").unwrap());
        assert!(parse_bool_value("tRuE", "test").unwrap());
    }

    #[test]
    fn test_us002_parse_bool_value_false() {
        // Test various false spellings (case-insensitive)
        assert!(!parse_bool_value("false", "test").unwrap());
        assert!(!parse_bool_value("FALSE", "test").unwrap());
        assert!(!parse_bool_value("False", "test").unwrap());
        assert!(!parse_bool_value("fAlSe", "test").unwrap());
    }

    #[test]
    fn test_us002_parse_bool_value_invalid() {
        // Test invalid values
        let result = parse_bool_value("yes", "review");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid value for 'review'"));
        assert!(err.contains("expected boolean"));
        assert!(err.contains("yes"));

        let result = parse_bool_value("1", "commit");
        assert!(result.is_err());

        let result = parse_bool_value("on", "worktree");
        assert!(result.is_err());

        let result = parse_bool_value("", "review");
        assert!(result.is_err());
    }

    #[test]
    fn test_us002_set_config_value_review() {
        let mut config = Config::default();
        assert!(config.review); // default is true

        set_config_value(&mut config, "review", "false").unwrap();
        assert!(!config.review);

        set_config_value(&mut config, "review", "true").unwrap();
        assert!(config.review);
    }

    #[test]
    fn test_us002_set_config_value_commit() {
        let mut config = Config::default();
        set_config_value(&mut config, "commit", "false").unwrap();
        assert!(!config.commit);
    }

    #[test]
    fn test_us002_set_config_value_pull_request() {
        let mut config = Config::default();
        set_config_value(&mut config, "pull_request", "false").unwrap();
        assert!(!config.pull_request);
    }

    #[test]
    fn test_us002_set_config_value_worktree() {
        let mut config = Config::default();
        set_config_value(&mut config, "worktree", "false").unwrap();
        assert!(!config.worktree);
    }

    #[test]
    fn test_us002_set_config_value_worktree_cleanup() {
        let mut config = Config::default();
        assert!(!config.worktree_cleanup); // default is false

        set_config_value(&mut config, "worktree_cleanup", "true").unwrap();
        assert!(config.worktree_cleanup);
    }

    #[test]
    fn test_us002_set_config_value_worktree_path_pattern() {
        let mut config = Config::default();
        let custom_pattern = "{repo}-feature-{branch}";

        set_config_value(&mut config, "worktree_path_pattern", custom_pattern).unwrap();
        assert_eq!(config.worktree_path_pattern, custom_pattern);
    }

    #[test]
    fn test_us002_set_config_value_worktree_path_pattern_with_spaces() {
        let mut config = Config::default();
        let pattern_with_spaces = "my-repo wt {branch}";

        set_config_value(&mut config, "worktree_path_pattern", pattern_with_spaces).unwrap();
        assert_eq!(config.worktree_path_pattern, pattern_with_spaces);
    }

    #[test]
    fn test_us002_format_value_for_display_boolean() {
        let mut config = Config::default();
        config.review = true;
        config.commit = false;

        assert_eq!(format_value_for_display("review", &config), "true");
        assert_eq!(format_value_for_display("commit", &config), "false");
    }

    #[test]
    fn test_us002_format_value_for_display_string() {
        let mut config = Config::default();
        config.worktree_path_pattern = "custom-{branch}".to_string();

        // String values should be quoted
        assert_eq!(
            format_value_for_display("worktree_path_pattern", &config),
            "\"custom-{branch}\""
        );
    }

    #[test]
    fn test_us002_invalid_key_rejected() {
        let mut config = Config::default();
        let result = set_config_value(&mut config, "invalid_key", "value");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unknown key"));
    }

    #[test]
    fn test_us002_all_valid_keys_settable() {
        // Verify each valid key can be set
        for key in VALID_CONFIG_KEYS.iter() {
            let mut config = Config::default();
            let value = match *key {
                "worktree_path_pattern" => "custom-pattern",
                _ => "false", // Boolean keys
            };
            let result = set_config_value(&mut config, key, value);
            assert!(result.is_ok(), "Setting key '{}' should succeed", key);
        }
    }

    #[test]
    fn test_us002_validation_enforced_pr_without_commit() {
        // Test that setting pull_request=true when commit=false would fail validation
        let mut config = Config {
            review: true,
            commit: false,
            pull_request: false,
            ..Default::default()
        };

        // Set pull_request to true
        set_config_value(&mut config, "pull_request", "true").unwrap();

        // The config should fail validation (this is what config_set_command checks)
        let validation_result = validate_config(&config);
        assert!(validation_result.is_err());
        assert!(validation_result
            .unwrap_err()
            .to_string()
            .contains("commit"));
    }

    #[test]
    fn test_us002_validation_enforced_commit_false_with_pr_true() {
        // Test that setting commit=false when pull_request=true would fail validation
        let mut config = Config {
            review: true,
            commit: true,
            pull_request: true,
            ..Default::default()
        };

        // Set commit to false
        set_config_value(&mut config, "commit", "false").unwrap();

        // The config should fail validation
        let validation_result = validate_config(&config);
        assert!(validation_result.is_err());
    }

    #[test]
    fn test_us002_valid_combinations_pass_validation() {
        // Test valid combinations
        let valid_combos = [
            (true, true, true),   // all true
            (true, true, false),  // commit true, pr false
            (true, false, false), // commit false, pr false
            (false, true, true),  // review false, others true
        ];

        for (review, commit, pull_request) in valid_combos {
            let config = Config {
                review,
                commit,
                pull_request,
                ..Default::default()
            };
            let result = validate_config(&config);
            assert!(
                result.is_ok(),
                "Config (review={}, commit={}, pull_request={}) should be valid",
                review,
                commit,
                pull_request
            );
        }
    }

    #[test]
    fn test_us002_case_insensitive_boolean_values() {
        let mut config = Config::default();

        // Test various case combinations
        set_config_value(&mut config, "review", "TRUE").unwrap();
        assert!(config.review);

        set_config_value(&mut config, "review", "False").unwrap();
        assert!(!config.review);

        set_config_value(&mut config, "worktree", "TrUe").unwrap();
        assert!(config.worktree);
    }

    #[test]
    fn test_us002_invalid_boolean_value_descriptive_error() {
        let result = parse_bool_value("yes", "review");
        assert!(result.is_err());

        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("review"),
            "Error should mention the key"
        );
        assert!(
            error_msg.contains("boolean"),
            "Error should mention expected type"
        );
        assert!(
            error_msg.contains("true/false"),
            "Error should mention valid values"
        );
        assert!(error_msg.contains("yes"), "Error should mention the invalid value");
    }
}
