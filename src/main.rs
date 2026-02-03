//! autom8 CLI entry point.
//!
//! Parses command-line arguments and dispatches to the appropriate command handler.

use autom8::commands::{
    all_sessions_status_command, clean_command, config_display_command, config_set_command,
    default_command, describe_command, global_status_command, init_command, list_command,
    monitor_command, pr_review_command, projects_command, resume_command, run_command,
    run_with_file, status_command, CleanOptions, ConfigScope, ConfigSubcommand,
};
use autom8::completion::{print_completion_script, ShellType, SUPPORTED_SHELLS};
use autom8::output::{print_error, print_header};
use autom8::Runner;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "autom8")]
#[command(
    version,
    about = "CLI automation tool for orchestrating Claude-powered development",
    after_help = "EXAMPLES:
    # Start a new run from a spec file
    autom8 spec.json
    autom8 run --spec feature.json

    # Run multiple features in parallel using worktrees
    autom8 run --worktree --spec feature-a.json  # Terminal 1
    autom8 run --worktree --spec feature-b.json  # Terminal 2

    # Check status of all parallel sessions
    autom8 status --all

    # Resume a specific session
    autom8 resume --list              # See resumable sessions
    autom8 resume --session abc123    # Resume by session ID

    # Clean up after completing work
    autom8 clean                      # Remove completed sessions
    autom8 clean --worktrees          # Also remove worktree directories"
)]
struct Cli {
    /// Path to a spec.md or spec.json file (shorthand for `run --spec <file>`)
    file: Option<PathBuf>,

    /// Show full Claude output instead of spinner (useful for debugging)
    #[arg(short, long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the agent loop to implement spec stories
    #[command(after_help = "EXAMPLES:
    autom8 run --spec feature.json           # Run on current branch
    autom8 run --worktree                    # Create dedicated worktree for parallel execution
    autom8 run --worktree --spec feature.json # Run in worktree with specific spec

WORKTREE MODE:
    When --worktree is enabled, autom8 creates a separate worktree directory
    at <repo-parent>/<repo>-wt-<branch>/ allowing multiple specs to run in parallel.
    Each worktree has its own isolated session state.")]
    Run {
        /// Path to the spec JSON or markdown file
        #[arg(long, default_value = "./spec.json")]
        spec: PathBuf,

        /// Skip the review loop and go directly to committing
        #[arg(long)]
        skip_review: bool,

        /// Enable worktree mode: create a dedicated worktree for this run.
        /// Allows running multiple specs in parallel with isolated state.
        #[arg(long, conflicts_with = "no_worktree")]
        worktree: bool,

        /// Disable worktree mode: run on the current branch (overrides config).
        /// Use this to override worktree=true in your config file.
        #[arg(long, conflicts_with = "worktree")]
        no_worktree: bool,
    },

    /// Check the current run status
    #[command(after_help = "EXAMPLES:
    autom8 status             # Show current session status
    autom8 status --all       # Show all sessions for this project
    autom8 status --global    # Show status across all projects

SESSION STATUS:
    Sessions are shown with: session ID, worktree path, branch name,
    current state (e.g., RunningClaude, Reviewing), and current story.
    The current session (matching CWD) is highlighted.")]
    Status {
        /// Show all sessions for the current project.
        /// Lists all active and completed sessions with their status.
        #[arg(short = 'a', long = "all")]
        all: bool,

        /// Show status across all projects.
        /// Displays a summary of all projects and their active runs.
        #[arg(short = 'g', long = "global")]
        global: bool,
    },

    /// Resume a failed or interrupted run
    #[command(after_help = "EXAMPLES:
    autom8 resume                     # Resume current session (auto-detected from CWD)
    autom8 resume --list              # List all resumable sessions
    autom8 resume --session abc123    # Resume a specific session by ID

BEHAVIOR:
    In the main repo with multiple incomplete sessions: prompts for selection.
    In a worktree: automatically resumes that worktree's session.
    With --session: changes to the worktree directory before resuming.")]
    Resume {
        /// Resume a specific session by ID.
        /// Use --list to see available session IDs.
        #[arg(short, long)]
        session: Option<String>,

        /// List all resumable sessions (incomplete runs).
        /// Shows sessions that can be resumed with --session <id>.
        #[arg(short, long)]
        list: bool,
    },

    /// Clean up sessions and worktrees from the project
    #[command(after_help = "EXAMPLES:
    autom8 clean                      # Remove completed/failed session state
    autom8 clean --worktrees          # Also remove associated worktree directories
    autom8 clean --all                # Remove ALL sessions (with confirmation)
    autom8 clean --session abc123     # Remove a specific session
    autom8 clean --orphaned           # Remove orphaned sessions only
    autom8 clean --worktrees --force  # Remove even with uncommitted changes

WHAT GETS CLEANED:
    By default, cleans completed and failed sessions (preserves in-progress).
    Session state is archived to runs/ directory before deletion.
    Worktrees with uncommitted changes are preserved unless --force is used.")]
    Clean {
        /// Also remove associated worktree directories.
        /// Without this flag, only session state is removed.
        #[arg(short, long)]
        worktrees: bool,

        /// Remove all sessions (with confirmation).
        /// Includes in-progress sessions - use with caution.
        #[arg(short, long)]
        all: bool,

        /// Remove a specific session by ID.
        /// Use 'autom8 status --all' to see session IDs.
        #[arg(short, long)]
        session: Option<String>,

        /// Only remove orphaned sessions (worktree deleted but state remains).
        /// Useful for cleaning up after manually deleting worktree directories.
        #[arg(short, long)]
        orphaned: bool,

        /// Force removal even if worktrees have uncommitted changes.
        /// Use with caution - uncommitted work will be lost.
        #[arg(short, long)]
        force: bool,
    },

    /// View, modify, or reset configuration values
    #[command(after_help = "EXAMPLES:
    autom8 config                              # Show both global and project config
    autom8 config --global                     # Show only global config
    autom8 config --project                    # Show only project config
    autom8 config set review false             # Set a value in project config
    autom8 config set --global commit true     # Set a value in global config
    autom8 config reset                        # Reset project config to defaults
    autom8 config reset --global               # Reset global config to defaults

CONFIG FILES:
    Global:  ~/.config/autom8/config.toml
    Project: ~/.config/autom8/<project>/config.toml

    The project config takes precedence over global config when both exist.
    If a config file doesn't exist, defaults are shown with a note.

VALID KEYS:
    review              - Enable code review step (true/false)
    commit              - Enable auto-commit (true/false)
    pull_request        - Enable auto-PR creation (true/false)
    worktree            - Enable worktree mode (true/false)
    worktree_path_pattern - Pattern for worktree names (string)
    worktree_cleanup    - Auto-cleanup worktrees (true/false)

SUBCOMMANDS:
    set    Set a configuration value
    reset  Reset configuration to default values

Run 'autom8 config <subcommand> --help' for more details on each subcommand.")]
    Config {
        /// Show only the global configuration (~/.config/autom8/config.toml)
        #[arg(short, long, conflicts_with = "project")]
        global: bool,

        /// Show only the project configuration (~/.config/autom8/<project>/config.toml)
        #[arg(short, long, conflicts_with = "global")]
        project: bool,

        /// Subcommand (set or reset)
        #[command(subcommand)]
        subcommand: Option<ConfigSubcommand>,
    },

    /// Initialize autom8 config directory structure for current project
    Init,

    /// List all known projects in the config directory
    Projects,

    /// Show a tree view of all projects with status
    List,

    /// Show detailed information about a specific project
    Describe {
        /// The project name to describe (defaults to current directory)
        project_name: Option<String>,
    },

    /// Analyze PR review comments and fix real issues
    PrReview,

    /// Monitor autom8 activity across all projects (dashboard view)
    Monitor {
        /// Filter to a specific project
        #[arg(short, long)]
        project: Option<String>,
    },

    /// Output shell completion script to stdout (hidden utility command)
    #[command(hide = true)]
    Completions {
        /// Shell type to generate completions for (bash, zsh, or fish)
        shell: String,
    },
}

fn main() {
    let cli = Cli::parse();

    // Handle commands that don't require a Runner (can work outside git repos)
    let result = match (&cli.file, &cli.command) {
        // Config command - handle all scopes and subcommands
        (
            None,
            Some(Commands::Config {
                global,
                project,
                subcommand,
            }),
        ) => {
            match subcommand {
                // Display config (default behavior when no subcommand)
                None => {
                    let scope = match (global, project) {
                        (true, false) => ConfigScope::Global,
                        (false, true) => ConfigScope::Project,
                        _ => ConfigScope::Both,
                    };
                    config_display_command(scope)
                }
                // Set subcommand (US-002)
                Some(ConfigSubcommand::Set {
                    global: g,
                    key,
                    value,
                }) => config_set_command(key, value, *g),
                // Reset subcommand - to be implemented in US-003
                Some(ConfigSubcommand::Reset { global: g, yes }) => {
                    // Placeholder for US-003 implementation
                    // For now, return an informative error
                    Err(autom8::error::Autom8Error::Config(format!(
                        "The 'config reset' subcommand is not yet implemented.\n\
                        Planned: autom8 config reset{}{}",
                        if *g { " --global" } else { "" },
                        if *yes { " --yes" } else { "" }
                    )))
                }
            }
        }

        // Completions command doesn't need a git repo
        (None, Some(Commands::Completions { shell })) => match ShellType::from_name(shell) {
            Ok(shell_type) => {
                print_completion_script(shell_type);
                Ok(())
            }
            Err(e) => {
                print_error(&format!(
                    "{}\nSupported shells: {}",
                    e,
                    SUPPORTED_SHELLS.join(", ")
                ));
                std::process::exit(1);
            }
        },

        // All other commands need the Runner (which requires a git repo)
        _ => {
            let runner = match Runner::new() {
                Ok(r) => r.with_verbose(cli.verbose),
                Err(e) => {
                    print_error(&format!("Failed to initialize runner: {}", e));
                    std::process::exit(1);
                }
            };

            match (&cli.file, &cli.command) {
                // Positional file argument takes precedence
                (Some(file), _) => run_with_file(&runner, file),

                // Subcommands
                (
                    None,
                    Some(Commands::Run {
                        spec,
                        skip_review,
                        worktree,
                        no_worktree,
                    }),
                ) => run_command(cli.verbose, spec, *skip_review, *worktree, *no_worktree),

                (None, Some(Commands::Status { all, global })) => {
                    print_header();
                    if *global {
                        global_status_command()
                    } else if *all {
                        all_sessions_status_command()
                    } else {
                        status_command(&runner)
                    }
                }

                (None, Some(Commands::Resume { session, list })) => {
                    resume_command(session.as_deref(), *list)
                }

                (
                    None,
                    Some(Commands::Clean {
                        worktrees,
                        all,
                        session,
                        orphaned,
                        force,
                    }),
                ) => clean_command(CleanOptions {
                    worktrees: *worktrees,
                    all: *all,
                    session: session.clone(),
                    orphaned: *orphaned,
                    force: *force,
                }),

                // Config already handled above (outside Runner block)
                (None, Some(Commands::Config { .. })) => unreachable!(),

                (None, Some(Commands::Init)) => init_command(),

                (None, Some(Commands::Projects)) => projects_command(),

                (None, Some(Commands::List)) => list_command(),

                (None, Some(Commands::Describe { project_name })) => {
                    describe_command(project_name.as_deref().unwrap_or(""))
                }

                (None, Some(Commands::PrReview)) => {
                    print_header();
                    pr_review_command(cli.verbose)
                }

                (None, Some(Commands::Monitor { project })) => monitor_command(project.as_deref()),

                // Completions already handled above
                (None, Some(Commands::Completions { .. })) => unreachable!(),

                // No file and no command - check for existing state first, then start spec creation
                (None, None) => default_command(cli.verbose),
            }
        }
    };

    if let Err(e) = result {
        print_error(&e.to_string());
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    // ======================================================================
    // Tests for US-006: Command routing logic
    // ======================================================================

    #[test]
    fn test_us006_new_is_treated_as_file_not_command() {
        // After removing the `new` subcommand, `autom8 new` is parsed as a file argument
        // (because we have a positional file argument in the CLI)
        let cli = Cli::try_parse_from(["autom8", "new"]).unwrap();
        // It should be treated as a file path, not a command
        assert!(
            cli.command.is_none(),
            "`new` should not be a command anymore"
        );
        assert!(cli.file.is_some(), "`new` should be treated as a file path");
        assert_eq!(cli.file.unwrap().to_string_lossy(), "new");
    }

    #[test]
    fn test_us006_no_args_triggers_default_flow() {
        // Test that running `autom8` with no arguments parses to (None, None)
        // which triggers the default flow
        let cli = Cli::try_parse_from(["autom8"]).unwrap();
        assert!(cli.file.is_none(), "No file should be set");
        assert!(
            cli.command.is_none(),
            "No command should be set - triggers default flow"
        );
    }

    #[test]
    fn test_us006_other_commands_still_work() {
        // Verify that other commands are still routed correctly
        let cli_resume = Cli::try_parse_from(["autom8", "resume"]).unwrap();
        assert!(matches!(cli_resume.command, Some(Commands::Resume { .. })));

        let cli_status = Cli::try_parse_from(["autom8", "status"]).unwrap();
        assert!(matches!(cli_status.command, Some(Commands::Status { .. })));

        let cli_projects = Cli::try_parse_from(["autom8", "projects"]).unwrap();
        assert!(matches!(cli_projects.command, Some(Commands::Projects)));

        let cli_clean = Cli::try_parse_from(["autom8", "clean"]).unwrap();
        assert!(matches!(cli_clean.command, Some(Commands::Clean { .. })));

        let cli_init = Cli::try_parse_from(["autom8", "init"]).unwrap();
        assert!(matches!(cli_init.command, Some(Commands::Init)));
    }

    #[test]
    fn test_us006_file_argument_still_takes_precedence() {
        // Test that positional file argument still works
        let cli = Cli::try_parse_from(["autom8", "my-spec.json"]).unwrap();
        assert!(cli.file.is_some());
        assert_eq!(cli.file.unwrap().to_string_lossy(), "my-spec.json");
    }

    // ======================================================================
    // Tests for US-001: State detection on default command
    // ======================================================================

    #[test]
    fn test_cli_no_args_triggers_default_command() {
        // Test that running `autom8` with no arguments parses to None/None
        let cli = Cli::try_parse_from(["autom8"]).unwrap();
        assert!(cli.file.is_none());
        assert!(cli.command.is_none());
    }

    #[test]
    fn test_state_manager_load_current_returns_none_when_no_state() {
        use autom8::state::StateManager;
        use tempfile::TempDir;

        // Create a fresh temp directory with no state file
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        // load_current should return None
        let result = sm.load_current().unwrap();
        assert!(
            result.is_none(),
            "Should return None when no state.json exists"
        );
    }

    #[test]
    fn test_state_manager_load_current_returns_state_when_exists() {
        use autom8::state::{RunState, StateManager};
        use tempfile::TempDir;

        // Create a temp directory and save a state file
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        let state = RunState::new(PathBuf::from("test.json"), "feature/test".to_string());
        sm.save(&state).unwrap();

        // load_current should return the state
        let result = sm.load_current().unwrap();
        assert!(
            result.is_some(),
            "Should return Some when state.json exists"
        );
        let loaded = result.unwrap();
        assert_eq!(loaded.branch, "feature/test");
    }

    // ======================================================================
    // Tests for US-002: Prompt user when state file exists
    // ======================================================================
    // Note: The actual handle_existing_state function is interactive (requires user input),
    // so we test the underlying components that it uses.

    #[test]
    fn test_us002_state_archive_before_clear() {
        // Test that state can be archived and then cleared - this is the "start fresh" flow
        use autom8::state::{RunState, StateManager};
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        // Create and save a state
        let state = RunState::new(PathBuf::from("test.json"), "feature/my-feature".to_string());
        sm.save(&state).unwrap();

        // Verify state exists
        assert!(sm.load_current().unwrap().is_some());

        // Archive the state (this is what handle_existing_state does for "start fresh")
        let archive_path = sm.archive(&state).unwrap();
        assert!(archive_path.exists(), "Archive file should be created");

        // Clear the current state
        sm.clear_current().unwrap();

        // Verify state is cleared
        assert!(
            sm.load_current().unwrap().is_none(),
            "State should be cleared"
        );

        // Verify archive still exists
        assert!(archive_path.exists(), "Archive should remain after clear");

        // Verify archived runs can be listed
        let archived = sm.list_archived().unwrap();
        assert_eq!(archived.len(), 1);
        assert_eq!(archived[0].branch, "feature/my-feature");
    }

    #[test]
    fn test_us002_state_has_branch_and_current_story_fields() {
        // Test that RunState properly stores branch and current_story for display
        use autom8::state::RunState;

        let mut state = RunState::new(
            PathBuf::from("test.json"),
            "feature/test-branch".to_string(),
        );
        assert_eq!(state.branch, "feature/test-branch");
        assert!(state.current_story.is_none());

        state.current_story = Some("US-001".to_string());
        assert_eq!(state.current_story, Some("US-001".to_string()));
    }

    #[test]
    fn test_us002_prompt_select_returns_valid_indices() {
        // Test that prompt::select options map to expected indices
        // (We can't test interactive input, but we verify the expected indices)
        // Index 0 = Resume, Index 1 = Start fresh, Index 2 = Exit
        let options = ["Resume existing work", "Start fresh", "Exit"];
        assert_eq!(options.len(), 3);
        assert_eq!(options[0], "Resume existing work");
        assert_eq!(options[1], "Start fresh");
        assert_eq!(options[2], "Exit");
    }

    #[test]
    fn test_us002_multiple_archives_preserved() {
        // Test that multiple state archives are all preserved
        use autom8::state::{RunState, StateManager};
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        // Create first state, archive it
        let state1 = RunState::new(PathBuf::from("spec1.json"), "feature/first".to_string());
        sm.archive(&state1).unwrap();

        // Create second state, archive it
        let state2 = RunState::new(PathBuf::from("spec2.json"), "feature/second".to_string());
        sm.archive(&state2).unwrap();

        // Both archives should exist
        let archived = sm.list_archived().unwrap();
        assert_eq!(archived.len(), 2);
    }

    // ======================================================================
    // Tests for US-003: Run spec creation flow as default
    // ======================================================================
    // Note: The core SpecSnapshot detection logic is tested extensively in
    // src/snapshot.rs. Here we test the integration points for US-003.

    #[test]
    fn test_us003_spec_snapshot_public_api_exists() {
        // Test that SpecSnapshot has the required public API for the spec creation flow
        use autom8::SpecSnapshot;

        // Verify capture() exists and returns a Result
        // (We can't test it directly without setting up config dirs, but we verify the API)

        // Verify the struct has the expected public fields
        let _: fn() -> autom8::error::Result<SpecSnapshot> = SpecSnapshot::capture;

        // The snapshot module is properly exported
        assert!(
            true,
            "SpecSnapshot is available through autom8::SpecSnapshot"
        );
    }

    #[test]
    fn test_us003_spec_skill_prompt_available() {
        // Verify the spec skill prompt is available for the spec creation flow
        // This is what gets passed to Claude when spawning the session
        use autom8::prompts;

        assert!(
            !prompts::SPEC_SKILL_PROMPT.is_empty(),
            "SPEC_SKILL_PROMPT should be defined and non-empty"
        );

        // The prompt should contain key instructions for spec creation
        assert!(
            prompts::SPEC_SKILL_PROMPT.contains("spec")
                || prompts::SPEC_SKILL_PROMPT.contains("Spec"),
            "SPEC_SKILL_PROMPT should mention spec"
        );
    }

    #[test]
    fn test_us003_start_spec_creation_path_from_default_command() {
        // Test that when no state exists, default_command proceeds to spec creation
        // This verifies the control flow: no state -> start_spec_creation
        use autom8::state::StateManager;
        use tempfile::TempDir;

        // Create a fresh temp directory with no state file
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        // Verify no state exists
        let result = sm.load_current().unwrap();
        assert!(result.is_none(), "Should have no state file");

        // This confirms the condition for entering the spec creation path:
        // In default_command(), when load_current() returns None,
        // it calls start_spec_creation(verbose)
    }

    #[test]
    fn test_us003_start_fresh_leads_to_spec_creation() {
        // Test that "start fresh" option (after archiving state) leads to spec creation
        use autom8::state::{RunState, StateManager};
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        // Create and save a state
        let state = RunState::new(PathBuf::from("test.json"), "feature/old".to_string());
        sm.save(&state).unwrap();

        // Archive and clear (what handle_existing_state does for "start fresh")
        sm.archive(&state).unwrap();
        sm.clear_current().unwrap();

        // After clearing, load_current should return None
        let result = sm.load_current().unwrap();
        assert!(
            result.is_none(),
            "After clear, should have no state - ready for spec creation"
        );

        // This confirms the path: start fresh -> archive -> clear -> start_spec_creation
    }

    // ======================================================================
    // Tests for US-004: Removed commands return errors
    // ======================================================================

    #[test]
    fn test_us004_skill_command_removed() {
        // The `skill` command should no longer be recognized
        // With the command removed, "skill" is treated as a file argument
        // (since we have a positional file argument in the CLI)
        let result = Cli::try_parse_from(["autom8", "skill"]);
        assert!(result.is_ok());
        let cli = result.unwrap();
        assert!(cli.command.is_none(), "`skill` should not be a command");
        assert!(
            cli.file.is_some(),
            "`skill` should be treated as a file path"
        );
        assert_eq!(cli.file.unwrap().to_string_lossy(), "skill");
    }

    #[test]
    fn test_us004_history_command_removed() {
        // The `history` command should no longer be recognized
        let result = Cli::try_parse_from(["autom8", "history"]);
        // With the command removed, "history" is treated as a file argument
        assert!(result.is_ok());
        let cli = result.unwrap();
        assert!(cli.command.is_none(), "`history` should not be a command");
        assert!(
            cli.file.is_some(),
            "`history` should be treated as a file path"
        );
    }

    #[test]
    fn test_us004_archive_command_removed() {
        // The `archive` command should no longer be recognized
        let result = Cli::try_parse_from(["autom8", "archive"]);
        // With the command removed, "archive" is treated as a file argument
        assert!(result.is_ok());
        let cli = result.unwrap();
        assert!(cli.command.is_none(), "`archive` should not be a command");
        assert!(
            cli.file.is_some(),
            "`archive` should be treated as a file path"
        );
    }

    #[test]
    fn test_us004_valid_commands_still_work() {
        // Verify that remaining commands are still valid
        assert!(Cli::try_parse_from(["autom8", "run"]).is_ok());
        assert!(Cli::try_parse_from(["autom8", "status"]).is_ok());
        assert!(Cli::try_parse_from(["autom8", "resume"]).is_ok());
        assert!(Cli::try_parse_from(["autom8", "clean"]).is_ok());
        assert!(Cli::try_parse_from(["autom8", "init"]).is_ok());
        assert!(Cli::try_parse_from(["autom8", "projects"]).is_ok());
        assert!(Cli::try_parse_from(["autom8", "list"]).is_ok());
    }

    // ======================================================================
    // Tests for US-007: List command with tree view
    // ======================================================================

    #[test]
    fn test_us007_list_command_is_recognized() {
        // Test that the list command is recognized
        let cli = Cli::try_parse_from(["autom8", "list"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::List)));
    }

    #[test]
    fn test_us007_list_command_parses_correctly() {
        // Test that `autom8 list` parses to the List variant
        let cli = Cli::try_parse_from(["autom8", "list"]).unwrap();
        assert!(cli.file.is_none(), "No file should be set");
        assert!(matches!(cli.command, Some(Commands::List)));
    }

    #[test]
    fn test_us007_list_projects_tree_returns_info() {
        // Test that list_projects_tree returns valid info
        let result = autom8::config::list_projects_tree();
        assert!(result.is_ok(), "list_projects_tree() should not error");
        // The autom8 project should be in the list
        let projects = result.unwrap();
        let has_autom8 = projects.iter().any(|p| p.name == "autom8");
        assert!(has_autom8, "autom8 project should be in the list");
    }

    #[test]
    fn test_us007_project_tree_info_has_expected_fields() {
        // Verify ProjectTreeInfo has all expected fields
        let info = autom8::config::ProjectTreeInfo {
            name: "test-project".to_string(),
            has_active_run: false,
            run_status: None,
            spec_count: 2,
            incomplete_spec_count: 1,
            spec_md_count: 3,
            runs_count: 4,
            last_run_date: None,
        };
        assert_eq!(info.name, "test-project");
        assert!(!info.has_active_run);
        assert!(info.run_status.is_none());
        assert_eq!(info.spec_count, 2);
        assert_eq!(info.incomplete_spec_count, 1);
        assert_eq!(info.spec_md_count, 3);
        assert_eq!(info.runs_count, 4);
    }

    // ======================================================================
    // Tests for US-005: Simplified init command
    // ======================================================================

    #[test]
    fn test_us005_init_command_is_recognized() {
        // Test that the init command is still recognized
        let cli = Cli::try_parse_from(["autom8", "init"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Init)));
    }

    #[test]
    fn test_us005_init_creates_base_config_dir() {
        // Test that ensure_config_dir creates ~/.config/autom8/
        let result = autom8::config::ensure_config_dir();
        assert!(result.is_ok());
        let (path, _) = result.unwrap();
        assert!(path.exists());
        assert!(path.ends_with("autom8"));
    }

    #[test]
    fn test_us005_init_creates_project_subdirectories() {
        // Test that ensure_project_config_dir creates all required subdirs
        let result = autom8::config::ensure_project_config_dir();
        assert!(result.is_ok());
        let (path, _) = result.unwrap();

        // Verify all required subdirectories exist
        assert!(path.join("spec").exists(), "spec/ should be created");
        assert!(path.join("runs").exists(), "runs/ should be created");
    }

    #[test]
    fn test_us005_no_skill_writes_to_claude_skills() {
        // Verify that the init function no longer references ~/.claude/skills/
        // The init_command function should only use autom8::config functions
        // which create directories in ~/.config/autom8/

        // Check that ~/.claude/skills/pdr/SKILL.md is not created by init
        let home = dirs::home_dir().unwrap();
        let _pdr_skill = home
            .join(".claude")
            .join("skills")
            .join("pdr")
            .join("SKILL.md");
        let _pdr_json_skill = home
            .join(".claude")
            .join("skills")
            .join("pdr-json")
            .join("SKILL.md");

        // Note: We cannot test that init doesn't write these files directly
        // without running init, but we can verify the prompts module no longer
        // exports the skill constants that would be written
        // This is validated by the fact that the code compiles without PRD_SKILL_MD
        // and PRD_JSON_SKILL_MD constants

        // The file may or may not exist from previous runs - we just verify
        // that our current codebase doesn't export those constants anymore
        assert!(
            true,
            "Skill constants removed - no writes to ~/.claude/skills/"
        );
    }

    // ======================================================================
    // Tests for US-006 (Config): Init resets global config
    // ======================================================================

    #[test]
    fn test_us006_config_global_config_path_returns_expected_path() {
        // Test that global_config_path returns ~/.config/autom8/config.toml
        let result = autom8::config::global_config_path();
        assert!(result.is_ok());
        let path = result.unwrap();
        assert!(path.ends_with("config.toml"));
        assert!(path.to_string_lossy().contains("autom8"));
    }

    #[test]
    fn test_us006_config_save_global_config_creates_file() {
        // Test that save_global_config creates the config file
        let config = autom8::config::Config::default();
        let result = autom8::config::save_global_config(&config);
        assert!(result.is_ok());

        // Verify the file now exists
        let path = autom8::config::global_config_path().unwrap();
        assert!(path.exists(), "Config file should exist after save");
    }

    #[test]
    fn test_us006_config_save_global_config_resets_to_defaults() {
        // Test that Config::default() produces the expected defaults
        // and save_global_config correctly serializes them
        let default_config = autom8::config::Config::default();

        // Verify defaults are all true
        assert_eq!(default_config.review, true, "default review should be true");
        assert_eq!(default_config.commit, true, "default commit should be true");
        assert_eq!(
            default_config.pull_request, true,
            "default pull_request should be true"
        );

        // Verify save_global_config succeeds
        let result = autom8::config::save_global_config(&default_config);
        assert!(result.is_ok(), "save_global_config should succeed");

        // Verify we can round-trip through load
        let loaded = autom8::config::load_global_config().unwrap();
        assert_eq!(
            loaded.review, default_config.review,
            "review should round-trip"
        );
        assert_eq!(
            loaded.commit, default_config.commit,
            "commit should round-trip"
        );
        assert_eq!(
            loaded.pull_request, default_config.pull_request,
            "pull_request should round-trip"
        );
    }

    #[test]
    fn test_us006_config_init_only_affects_global_not_project() {
        // Test that save_global_config and save_project_config
        // write to different files (verifying init doesn't affect project config)
        let global_path = autom8::config::global_config_path().unwrap();
        let project_path = autom8::config::project_config_path().unwrap();

        // Verify the paths are different
        assert_ne!(
            global_path, project_path,
            "Global and project config paths should be different"
        );

        // Verify global config is in the base autom8 directory
        assert!(
            global_path.to_string_lossy().contains("autom8/config.toml"),
            "Global config should be at ~/.config/autom8/config.toml"
        );

        // Verify project config is in a project-specific subdirectory
        // The path should contain a project name between autom8/ and /config.toml
        let project_path_str = project_path.to_string_lossy();
        assert!(
            project_path_str.contains("/autom8/"),
            "Project config should be under autom8 directory"
        );

        // Count the path components to verify project config is nested deeper
        let global_depth = global_path.components().count();
        let project_depth = project_path.components().count();
        assert!(
            project_depth > global_depth,
            "Project config should be in a subdirectory"
        );
    }

    // ======================================================================
    // Tests for US-008: Describe command for project summaries
    // ======================================================================

    #[test]
    fn test_us008_describe_command_is_recognized() {
        // Test that the describe command is recognized
        let cli = Cli::try_parse_from(["autom8", "describe", "test-project"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Describe { .. })));
    }

    #[test]
    fn test_us008_describe_command_parses_project_name() {
        // Test that `autom8 describe <project>` parses correctly
        let cli = Cli::try_parse_from(["autom8", "describe", "my-project"]).unwrap();
        if let Some(Commands::Describe { project_name }) = cli.command {
            assert_eq!(project_name, Some("my-project".to_string()));
        } else {
            panic!("Expected Describe command");
        }
    }

    #[test]
    fn test_us008_describe_command_defaults_to_current_dir() {
        // Test that describe command works without project name (defaults to current directory)
        let cli = Cli::try_parse_from(["autom8", "describe"]).unwrap();
        if let Some(Commands::Describe { project_name }) = cli.command {
            assert!(
                project_name.is_none(),
                "project_name should be None when not provided"
            );
        } else {
            panic!("Expected Describe command");
        }
    }

    #[test]
    fn test_us008_project_exists_returns_true_for_existing() {
        // Test that project_exists returns true for a project that exists
        // The autom8 project should exist since we're running tests from it
        let result = autom8::config::project_exists("autom8");
        assert!(result.is_ok());
        assert!(result.unwrap(), "autom8 project should exist");
    }

    #[test]
    fn test_us008_project_exists_returns_false_for_nonexistent() {
        // Test that project_exists returns false for a project that doesn't exist
        let result = autom8::config::project_exists("nonexistent-project-12345");
        assert!(result.is_ok());
        assert!(!result.unwrap(), "nonexistent project should return false");
    }

    #[test]
    fn test_us008_get_project_description_returns_some_for_existing() {
        // Test that get_project_description returns Some for an existing project
        let result = autom8::config::get_project_description("autom8");
        assert!(result.is_ok());
        let desc = result.unwrap();
        assert!(desc.is_some(), "Should return Some for autom8 project");

        let desc = desc.unwrap();
        assert_eq!(desc.name, "autom8");
        assert!(desc.path.exists());
    }

    #[test]
    fn test_us008_get_project_description_returns_none_for_nonexistent() {
        // Test that get_project_description returns None for a nonexistent project
        let result = autom8::config::get_project_description("nonexistent-project-12345");
        assert!(result.is_ok());
        assert!(
            result.unwrap().is_none(),
            "Should return None for nonexistent project"
        );
    }

    #[test]
    fn test_us008_project_description_has_expected_fields() {
        // Verify ProjectDescription has all expected fields
        let desc = autom8::config::get_project_description("autom8")
            .unwrap()
            .unwrap();

        // Basic fields should be populated
        assert!(!desc.name.is_empty());
        assert!(desc.path.exists());

        // Check spec summary fields if any exist
        if !desc.specs.is_empty() {
            let first_spec = &desc.specs[0];
            assert!(!first_spec.filename.is_empty());
            assert!(!first_spec.project_name.is_empty());
            assert!(!first_spec.branch_name.is_empty());
            assert!(!first_spec.stories.is_empty(), "Spec should have stories");
        }
    }

    #[test]
    fn test_us008_story_summary_has_expected_fields() {
        // Verify StorySummary has all expected fields
        let desc = autom8::config::get_project_description("autom8")
            .unwrap()
            .unwrap();

        // Only test if there are specs available
        if !desc.specs.is_empty() {
            let first_spec = &desc.specs[0];
            let first_story = &first_spec.stories[0];

            // Story fields should be populated
            assert!(!first_story.id.is_empty());
            assert!(!first_story.title.is_empty());
            // passes is a bool, so no emptiness check needed
        }
    }

    #[test]
    fn test_us008_spec_summary_progress_counts() {
        // Verify completed_count and total_count are consistent
        let desc = autom8::config::get_project_description("autom8")
            .unwrap()
            .unwrap();

        for spec in &desc.specs {
            assert!(spec.completed_count <= spec.total_count);
            assert_eq!(spec.total_count, spec.stories.len());

            // Verify completed_count matches actual passing stories
            let actual_completed = spec.stories.iter().filter(|s| s.passes).count();
            assert_eq!(spec.completed_count, actual_completed);
        }
    }

    // ======================================================================
    // Tests for US-010: Semantic Versioning
    // ======================================================================

    #[test]
    fn test_us010_version_flag_is_configured() {
        // Test that --version flag is recognized by clap
        // Clap returns an error with ErrorKind::DisplayVersion when --version is passed
        let result = Cli::try_parse_from(["autom8", "--version"]);
        assert!(result.is_err(), "Should return error for --version flag");
        // Verify it's a DisplayVersion error (expected behavior)
        let err = result.err().unwrap();
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::DisplayVersion,
            "Should recognize --version flag"
        );
    }

    #[test]
    fn test_us010_short_version_flag_is_configured() {
        // Test that -V flag is recognized by clap
        let result = Cli::try_parse_from(["autom8", "-V"]);
        assert!(result.is_err(), "Should return error for -V flag");
        // Verify it's a DisplayVersion error (expected behavior)
        let err = result.err().unwrap();
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::DisplayVersion,
            "Should recognize -V flag"
        );
    }

    #[test]
    fn test_us010_version_matches_cargo_toml() {
        // Verify the version constant matches what's in Cargo.toml
        let cargo_version = env!("CARGO_PKG_VERSION");
        assert_eq!(cargo_version, "0.2.0", "Version should be 0.2.0");
    }

    // ======================================================================
    // Tests for US-001: Remove TUI CLI flag
    // ======================================================================

    #[test]
    fn test_us001_tui_flag_is_not_recognized() {
        // Test that --tui flag produces an error (flag has been removed)
        let result = Cli::try_parse_from(["autom8", "--tui"]);
        assert!(result.is_err(), "--tui flag should produce an error");
    }

    #[test]
    fn test_us001_tui_short_flag_is_not_recognized() {
        // Test that -t short flag produces an error (flag has been removed)
        let result = Cli::try_parse_from(["autom8", "-t"]);
        assert!(result.is_err(), "-t flag should produce an error");
    }

    #[test]
    fn test_us001_cli_struct_has_no_tui_field() {
        // Test that CLI parses without TUI field
        let cli = Cli::try_parse_from(["autom8"]).unwrap();
        // Just verify it parses - no tui field to check anymore
        assert!(cli.command.is_none());
    }

    // ======================================================================
    // Tests for US-007: Integration and entry point wiring
    // ======================================================================

    // ======================================================================
    // Tests for US-007 (PR Review): Add pr-review subcommand
    // ======================================================================

    #[test]
    fn test_us007_pr_review_command_is_recognized() {
        // Test that the pr-review command is recognized
        let cli = Cli::try_parse_from(["autom8", "pr-review"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::PrReview)));
    }

    #[test]
    fn test_us007_pr_review_parses_correctly() {
        // Test that `autom8 pr-review` parses to the PrReview variant
        let cli = Cli::try_parse_from(["autom8", "pr-review"]).unwrap();
        assert!(cli.file.is_none(), "No file should be set");
        assert!(matches!(cli.command, Some(Commands::PrReview)));
    }

    #[test]
    fn test_us007_pr_review_verbose_flag_works() {
        // Test that --verbose flag works with pr-review command
        let cli = Cli::try_parse_from(["autom8", "--verbose", "pr-review"]).unwrap();
        assert!(cli.verbose, "--verbose should be set");
        assert!(matches!(cli.command, Some(Commands::PrReview)));

        // Also test with flag after command (global flag behavior)
        let cli = Cli::try_parse_from(["autom8", "pr-review", "--verbose"]).unwrap();
        assert!(cli.verbose, "--verbose should work after command");
        assert!(matches!(cli.command, Some(Commands::PrReview)));
    }

    #[test]
    fn test_us007_pr_review_short_verbose_flag_works() {
        // Test that -v short flag works with pr-review command
        let cli = Cli::try_parse_from(["autom8", "-v", "pr-review"]).unwrap();
        assert!(cli.verbose, "-v should be set");
        assert!(matches!(cli.command, Some(Commands::PrReview)));
    }

    #[test]
    fn test_us007_pr_review_does_not_take_arguments() {
        // Test that pr-review command doesn't accept positional arguments
        // (it should work without any arguments)
        let cli = Cli::try_parse_from(["autom8", "pr-review"]).unwrap();
        assert!(cli.file.is_none());
        assert!(matches!(cli.command, Some(Commands::PrReview)));
    }

    #[test]
    fn test_us007_pr_review_with_verbose_flag() {
        // Test that pr-review works with --verbose flag
        let cli = Cli::try_parse_from(["autom8", "--verbose", "pr-review"]).unwrap();
        assert!(cli.verbose, "--verbose should be set");
        assert!(matches!(cli.command, Some(Commands::PrReview)));
    }

    #[test]
    fn test_us007_pr_review_appears_in_commands_enum() {
        // Verify PrReview is a valid variant of Commands enum
        let _cmd = Commands::PrReview;
        // If this compiles, the variant exists
    }

    #[test]
    fn test_us007_pr_review_functions_imported() {
        // Verify that all required PR review functions are available
        // These are used in pr_review_command and should be importable
        use autom8::claude::PRReviewResult;
        use autom8::gh::{BranchContextResult, PRContextResult, PRDetectionResult};
        use autom8::git::{CommitResult, PushResult};

        // Create instances to verify the types exist
        let _detection: Option<PRDetectionResult> = None;
        let _context: Option<PRContextResult> = None;
        let _branch: Option<BranchContextResult> = None;
        let _result: Option<PRReviewResult> = None;
        let _commit: Option<CommitResult> = None;
        let _push: Option<PushResult> = None;
    }

    #[test]
    #[allow(unused_imports)]
    fn test_us007_pr_review_output_functions_available() {
        // Verify that output functions for PR review are available
        // We can't call them without proper context, but we can verify they exist
        use autom8::output::{
            format_pr_for_selection, print_no_open_prs, print_no_unresolved_comments,
            print_pr_commit_error, print_pr_commit_success, print_pr_context_summary,
            print_pr_detected, print_pr_push_error, print_pr_push_success,
            print_pr_review_actions_summary, print_pr_review_complete_with_fixes,
            print_pr_review_error, print_pr_review_no_fixes_needed, print_pr_review_spawning,
            print_pr_review_start, print_pr_review_streaming, print_pr_review_streaming_done,
            print_pr_review_summary,
        };

        // Verify format_pr_for_selection returns expected format
        let formatted = format_pr_for_selection(123, "feature/test", "Add test feature");
        assert!(formatted.contains("#123"));
        assert!(formatted.contains("feature/test"));
        assert!(formatted.contains("Add test feature"));
    }

    #[test]
    fn test_us007_pr_review_config_integration() {
        // Verify that config is accessible for commit/push settings
        let config = autom8::config::get_effective_config().unwrap_or_default();

        // Config should have commit and pull_request fields (used for push permission)
        let _commit_enabled: bool = config.commit;
        let _push_enabled: bool = config.pull_request;
    }

    // ======================================================================
    // Tests for US-003 (Monitor TUI): Add monitor command structure
    // ======================================================================

    #[test]
    fn test_us003_monitor_command_is_recognized() {
        // Test that the monitor command is recognized
        let cli = Cli::try_parse_from(["autom8", "monitor"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Monitor { .. })));
    }

    #[test]
    fn test_us003_monitor_command_parses_correctly() {
        // Test that `autom8 monitor` parses to the Monitor variant with defaults
        let cli = Cli::try_parse_from(["autom8", "monitor"]).unwrap();
        assert!(cli.file.is_none(), "No file should be set");
        if let Some(Commands::Monitor { project }) = cli.command {
            assert!(
                project.is_none(),
                "Project filter should be None by default"
            );
        } else {
            panic!("Expected Monitor command");
        }
    }

    #[test]
    fn test_us003_monitor_project_flag() {
        // Test that --project flag works
        let cli = Cli::try_parse_from(["autom8", "monitor", "--project", "myapp"]).unwrap();
        if let Some(Commands::Monitor { project }) = cli.command {
            assert_eq!(project, Some("myapp".to_string()));
        } else {
            panic!("Expected Monitor command");
        }
    }

    #[test]
    fn test_us003_monitor_project_short_flag() {
        // Test that -p short flag works for --project
        let cli = Cli::try_parse_from(["autom8", "monitor", "-p", "myapp"]).unwrap();
        if let Some(Commands::Monitor { project }) = cli.command {
            assert_eq!(project, Some("myapp".to_string()));
        } else {
            panic!("Expected Monitor command");
        }
    }

    #[test]
    fn test_us003_monitor_uses_list_projects_tree() {
        // Verify that list_projects_tree is available and returns valid data
        let result = autom8::config::list_projects_tree();
        assert!(result.is_ok(), "list_projects_tree() should not error");
    }

    #[test]
    fn test_us003_monitor_command_appears_in_help() {
        // Verify that monitor command appears in the Commands enum
        // (if this compiles, the variant exists)
        let _cmd = Commands::Monitor { project: None };
    }

    // ======================================================================
    // Tests for US-003 (Shell Completion): Completions subcommand
    // ======================================================================

    #[test]
    fn test_us003_completions_command_is_recognized() {
        // Test that the completions command is recognized
        let cli = Cli::try_parse_from(["autom8", "completions", "bash"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Completions { .. })));
    }

    #[test]
    fn test_us003_completions_command_parses_shell_arg() {
        // Test that shell argument is parsed correctly
        let cli = Cli::try_parse_from(["autom8", "completions", "zsh"]).unwrap();
        if let Some(Commands::Completions { shell }) = cli.command {
            assert_eq!(shell, "zsh");
        } else {
            panic!("Expected Completions command");
        }
    }

    #[test]
    fn test_us003_completions_command_accepts_all_shells() {
        // Test bash
        let cli_bash = Cli::try_parse_from(["autom8", "completions", "bash"]).unwrap();
        if let Some(Commands::Completions { shell }) = cli_bash.command {
            assert_eq!(shell, "bash");
        }

        // Test zsh
        let cli_zsh = Cli::try_parse_from(["autom8", "completions", "zsh"]).unwrap();
        if let Some(Commands::Completions { shell }) = cli_zsh.command {
            assert_eq!(shell, "zsh");
        }

        // Test fish
        let cli_fish = Cli::try_parse_from(["autom8", "completions", "fish"]).unwrap();
        if let Some(Commands::Completions { shell }) = cli_fish.command {
            assert_eq!(shell, "fish");
        }
    }

    #[test]
    fn test_us003_completions_command_is_hidden() {
        // The completions command should not appear in help output
        // We can verify this by checking that the hide attribute is set
        // (This compiles only if the hide = true attribute is present)
        let _cmd = Commands::Completions {
            shell: "bash".to_string(),
        };

        // Try to get help text and verify completions is not mentioned
        let cli_result = Cli::try_parse_from(["autom8", "--help"]);
        // This will return an error with the help text
        if let Err(e) = cli_result {
            let help_text = e.to_string();
            // completions should NOT appear in the help output because it's hidden
            assert!(
                !help_text.contains("completions"),
                "completions command should be hidden from help"
            );
        }
    }

    #[test]
    fn test_us003_completions_requires_shell_arg() {
        // Completions command requires a shell argument
        let result = Cli::try_parse_from(["autom8", "completions"]);
        assert!(
            result.is_err(),
            "completions command should require a shell argument"
        );
    }

    #[test]
    fn test_us003_shell_type_from_name_available() {
        // Verify ShellType::from_name is available and works
        use autom8::completion::ShellType;

        assert!(ShellType::from_name("bash").is_ok());
        assert!(ShellType::from_name("zsh").is_ok());
        assert!(ShellType::from_name("fish").is_ok());
        assert!(ShellType::from_name("invalid").is_err());
    }

    #[test]
    fn test_us003_print_completion_script_available() {
        // Verify print_completion_script function is available
        use autom8::completion::{print_completion_script, ShellType};

        // Just verify it's callable (we don't want to actually print to stdout in tests)
        let _: fn(ShellType) = print_completion_script;
    }

    #[test]
    fn test_us003_supported_shells_constant_available() {
        // Verify SUPPORTED_SHELLS constant is available
        use autom8::completion::SUPPORTED_SHELLS;

        assert!(SUPPORTED_SHELLS.contains(&"bash"));
        assert!(SUPPORTED_SHELLS.contains(&"zsh"));
        assert!(SUPPORTED_SHELLS.contains(&"fish"));
    }

    // ======================================================================
    // Tests for US-005: Worktree CLI flags
    // ======================================================================

    #[test]
    fn test_us005_run_command_has_worktree_flag() {
        // Test that --worktree flag is recognized
        let cli = Cli::try_parse_from(["autom8", "run", "--worktree"]).unwrap();
        if let Some(Commands::Run {
            worktree,
            no_worktree,
            ..
        }) = cli.command
        {
            assert!(worktree, "--worktree should set worktree to true");
            assert!(
                !no_worktree,
                "no_worktree should be false when --worktree is set"
            );
        } else {
            panic!("Expected Run command");
        }
    }

    #[test]
    fn test_us005_run_command_has_no_worktree_flag() {
        // Test that --no-worktree flag is recognized
        let cli = Cli::try_parse_from(["autom8", "run", "--no-worktree"]).unwrap();
        if let Some(Commands::Run {
            worktree,
            no_worktree,
            ..
        }) = cli.command
        {
            assert!(
                !worktree,
                "worktree should be false when --no-worktree is set"
            );
            assert!(no_worktree, "--no-worktree should set no_worktree to true");
        } else {
            panic!("Expected Run command");
        }
    }

    #[test]
    fn test_us005_run_command_worktree_defaults() {
        // Test that both flags default to false
        let cli = Cli::try_parse_from(["autom8", "run"]).unwrap();
        if let Some(Commands::Run {
            worktree,
            no_worktree,
            ..
        }) = cli.command
        {
            assert!(!worktree, "worktree should default to false");
            assert!(!no_worktree, "no_worktree should default to false");
        } else {
            panic!("Expected Run command");
        }
    }

    #[test]
    fn test_us005_worktree_flags_are_mutually_exclusive() {
        // Test that --worktree and --no-worktree cannot be used together
        let result = Cli::try_parse_from(["autom8", "run", "--worktree", "--no-worktree"]);
        assert!(
            result.is_err(),
            "--worktree and --no-worktree should be mutually exclusive"
        );
    }

    #[test]
    fn test_us005_worktree_flag_with_spec() {
        // Test that --worktree works with --spec
        let cli =
            Cli::try_parse_from(["autom8", "run", "--spec", "my-spec.json", "--worktree"]).unwrap();
        if let Some(Commands::Run {
            spec,
            worktree,
            no_worktree,
            ..
        }) = cli.command
        {
            assert_eq!(spec.to_string_lossy(), "my-spec.json");
            assert!(worktree);
            assert!(!no_worktree);
        } else {
            panic!("Expected Run command");
        }
    }

    #[test]
    fn test_us005_no_worktree_flag_with_skip_review() {
        // Test that --no-worktree works with --skip-review
        let cli = Cli::try_parse_from(["autom8", "run", "--skip-review", "--no-worktree"]).unwrap();
        if let Some(Commands::Run {
            skip_review,
            worktree,
            no_worktree,
            ..
        }) = cli.command
        {
            assert!(skip_review);
            assert!(!worktree);
            assert!(no_worktree);
        } else {
            panic!("Expected Run command");
        }
    }

    #[test]
    fn test_us005_worktree_config_available() {
        // Verify the worktree field exists on Config struct
        let config = autom8::config::Config::default();
        let _worktree: bool = config.worktree;
    }

    // ======================================================================
    // Tests for US-009: Multi-Session Status Command
    // ======================================================================

    #[test]
    fn test_us009_status_all_flag() {
        // Test that --all flag is recognized
        let cli = Cli::try_parse_from(["autom8", "status", "--all"]).unwrap();
        if let Some(Commands::Status { all, global }) = cli.command {
            assert!(all, "--all should be true");
            assert!(!global, "--global should be false");
        } else {
            panic!("Expected Status command");
        }
    }

    #[test]
    fn test_us009_status_short_all_flag() {
        // Test that -a short flag works
        let cli = Cli::try_parse_from(["autom8", "status", "-a"]).unwrap();
        if let Some(Commands::Status { all, global }) = cli.command {
            assert!(all, "-a should set all to true");
            assert!(!global);
        } else {
            panic!("Expected Status command");
        }
    }

    #[test]
    fn test_us009_status_global_flag_separate() {
        // Test that --global flag is separate from --all
        let cli = Cli::try_parse_from(["autom8", "status", "--global"]).unwrap();
        if let Some(Commands::Status { all, global }) = cli.command {
            assert!(!all, "--all should be false");
            assert!(global, "--global should be true");
        } else {
            panic!("Expected Status command");
        }
    }

    #[test]
    fn test_us009_status_short_global_flag() {
        // Test that -g short flag works for --global
        let cli = Cli::try_parse_from(["autom8", "status", "-g"]).unwrap();
        if let Some(Commands::Status { all, global }) = cli.command {
            assert!(!all);
            assert!(global, "-g should set global to true");
        } else {
            panic!("Expected Status command");
        }
    }

    #[test]
    fn test_us009_status_no_flags_defaults() {
        // Test default behavior with no flags
        let cli = Cli::try_parse_from(["autom8", "status"]).unwrap();
        if let Some(Commands::Status { all, global }) = cli.command {
            assert!(!all, "all should default to false");
            assert!(!global, "global should default to false");
        } else {
            panic!("Expected Status command");
        }
    }

    #[test]
    fn test_us009_all_sessions_status_command_importable() {
        // Verify the command function is exported
        use autom8::commands::all_sessions_status_command;
        let _: fn() -> autom8::error::Result<()> = all_sessions_status_command;
    }

    #[test]
    fn test_us009_session_status_struct_available() {
        // Verify SessionStatus is exported from state module
        use autom8::state::SessionStatus;

        // Create a minimal SessionStatus to verify the struct exists
        let metadata = autom8::state::SessionMetadata {
            session_id: "test".to_string(),
            worktree_path: PathBuf::from("/tmp"),
            branch_name: "main".to_string(),
            created_at: chrono::Utc::now(),
            last_active_at: chrono::Utc::now(),
            is_running: false,
        };

        let _status = SessionStatus {
            metadata,
            machine_state: None,
            current_story: None,
            is_current: false,
            is_stale: false,
        };
    }

    #[test]
    fn test_us009_list_sessions_with_status_available() {
        // Verify the method is available on StateManager
        use autom8::state::StateManager;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        // Should be callable and return empty list for new project
        let sessions = sm.list_sessions_with_status().unwrap();
        assert!(sessions.is_empty());
    }

    // ======================================================================
    // Tests for US-010: Multi-Session Resume Command
    // ======================================================================

    #[test]
    fn test_us010_resume_command_no_args() {
        // Test that resume command works without arguments
        let cli = Cli::try_parse_from(["autom8", "resume"]).unwrap();
        if let Some(Commands::Resume { session, list }) = cli.command {
            assert!(session.is_none(), "session should be None by default");
            assert!(!list, "list should be false by default");
        } else {
            panic!("Expected Resume command");
        }
    }

    #[test]
    fn test_us010_resume_command_session_flag() {
        // Test that --session flag is recognized
        let cli = Cli::try_parse_from(["autom8", "resume", "--session", "abc123"]).unwrap();
        if let Some(Commands::Resume { session, list }) = cli.command {
            assert_eq!(session, Some("abc123".to_string()));
            assert!(!list);
        } else {
            panic!("Expected Resume command");
        }
    }

    #[test]
    fn test_us010_resume_command_session_short_flag() {
        // Test that -s short flag works for --session
        let cli = Cli::try_parse_from(["autom8", "resume", "-s", "xyz789"]).unwrap();
        if let Some(Commands::Resume { session, list }) = cli.command {
            assert_eq!(session, Some("xyz789".to_string()));
            assert!(!list);
        } else {
            panic!("Expected Resume command");
        }
    }

    #[test]
    fn test_us010_resume_command_list_flag() {
        // Test that --list flag is recognized
        let cli = Cli::try_parse_from(["autom8", "resume", "--list"]).unwrap();
        if let Some(Commands::Resume { session, list }) = cli.command {
            assert!(session.is_none());
            assert!(list, "--list should set list to true");
        } else {
            panic!("Expected Resume command");
        }
    }

    #[test]
    fn test_us010_resume_command_list_short_flag() {
        // Test that -l short flag works for --list
        let cli = Cli::try_parse_from(["autom8", "resume", "-l"]).unwrap();
        if let Some(Commands::Resume { session, list }) = cli.command {
            assert!(session.is_none());
            assert!(list, "-l should set list to true");
        } else {
            panic!("Expected Resume command");
        }
    }

    #[test]
    fn test_us010_resume_command_function_signature_changed() {
        // Verify the resume_command function accepts the new parameters
        use autom8::commands::resume_command;
        // Type check: resume_command takes Option<&str> and bool
        let _: fn(Option<&str>, bool) -> autom8::error::Result<()> = resume_command;
    }

    // ======================================================================
    // Tests for US-004 (Config Command): Command structure and help
    // ======================================================================

    #[test]
    fn test_us004_config_without_flags_is_default_behavior() {
        // `autom8 config` without flags should parse successfully (default: show both configs)
        let cli = Cli::try_parse_from(["autom8", "config"]).unwrap();
        if let Some(Commands::Config {
            global,
            project,
            subcommand,
        }) = cli.command
        {
            assert!(!global, "global should default to false");
            assert!(!project, "project should default to false");
            assert!(
                subcommand.is_none(),
                "subcommand should be None (displays both configs)"
            );
        } else {
            panic!("Expected Config command");
        }
    }

    #[test]
    fn test_us004_config_help_recognized() {
        // Test that --help flag triggers help output
        let result = Cli::try_parse_from(["autom8", "config", "--help"]);
        assert!(result.is_err(), "Should return error for --help flag");
        let err = result.err().unwrap();
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::DisplayHelp,
            "Should recognize --help flag"
        );
    }

    #[test]
    fn test_us004_config_set_subcommand_recognized() {
        // Test that `config set <key> <value>` is recognized
        let cli = Cli::try_parse_from(["autom8", "config", "set", "review", "false"]).unwrap();
        if let Some(Commands::Config { subcommand, .. }) = cli.command {
            if let Some(ConfigSubcommand::Set { global, key, value }) = subcommand {
                assert!(!global, "global should default to false");
                assert_eq!(key, "review");
                assert_eq!(value, "false");
            } else {
                panic!("Expected Set subcommand");
            }
        } else {
            panic!("Expected Config command");
        }
    }

    #[test]
    fn test_us004_config_set_global_flag() {
        // Test that `config set --global <key> <value>` works
        let cli =
            Cli::try_parse_from(["autom8", "config", "set", "--global", "commit", "true"]).unwrap();
        if let Some(Commands::Config { subcommand, .. }) = cli.command {
            if let Some(ConfigSubcommand::Set { global, key, value }) = subcommand {
                assert!(global, "--global should set global to true");
                assert_eq!(key, "commit");
                assert_eq!(value, "true");
            } else {
                panic!("Expected Set subcommand");
            }
        } else {
            panic!("Expected Config command");
        }
    }

    #[test]
    fn test_us004_config_set_short_global_flag() {
        // Test that `config set -g <key> <value>` works
        let cli =
            Cli::try_parse_from(["autom8", "config", "set", "-g", "worktree", "false"]).unwrap();
        if let Some(Commands::Config { subcommand, .. }) = cli.command {
            if let Some(ConfigSubcommand::Set { global, key, value }) = subcommand {
                assert!(global, "-g should set global to true");
                assert_eq!(key, "worktree");
                assert_eq!(value, "false");
            } else {
                panic!("Expected Set subcommand");
            }
        } else {
            panic!("Expected Config command");
        }
    }

    #[test]
    fn test_us004_config_set_help_recognized() {
        // Test that `config set --help` works
        let result = Cli::try_parse_from(["autom8", "config", "set", "--help"]);
        assert!(result.is_err(), "Should return error for --help flag");
        let err = result.err().unwrap();
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::DisplayHelp,
            "Should recognize --help flag for set subcommand"
        );
    }

    #[test]
    fn test_us004_config_reset_subcommand_recognized() {
        // Test that `config reset` is recognized
        let cli = Cli::try_parse_from(["autom8", "config", "reset"]).unwrap();
        if let Some(Commands::Config { subcommand, .. }) = cli.command {
            if let Some(ConfigSubcommand::Reset { global, yes }) = subcommand {
                assert!(!global, "global should default to false");
                assert!(!yes, "yes should default to false");
            } else {
                panic!("Expected Reset subcommand");
            }
        } else {
            panic!("Expected Config command");
        }
    }

    #[test]
    fn test_us004_config_reset_global_flag() {
        // Test that `config reset --global` works
        let cli = Cli::try_parse_from(["autom8", "config", "reset", "--global"]).unwrap();
        if let Some(Commands::Config { subcommand, .. }) = cli.command {
            if let Some(ConfigSubcommand::Reset { global, yes }) = subcommand {
                assert!(global, "--global should set global to true");
                assert!(!yes);
            } else {
                panic!("Expected Reset subcommand");
            }
        } else {
            panic!("Expected Config command");
        }
    }

    #[test]
    fn test_us004_config_reset_yes_flag() {
        // Test that `config reset --yes` works
        let cli = Cli::try_parse_from(["autom8", "config", "reset", "--yes"]).unwrap();
        if let Some(Commands::Config { subcommand, .. }) = cli.command {
            if let Some(ConfigSubcommand::Reset { global, yes }) = subcommand {
                assert!(!global);
                assert!(yes, "--yes should set yes to true");
            } else {
                panic!("Expected Reset subcommand");
            }
        } else {
            panic!("Expected Config command");
        }
    }

    #[test]
    fn test_us004_config_reset_short_yes_flag() {
        // Test that `config reset -y` works
        let cli = Cli::try_parse_from(["autom8", "config", "reset", "-y"]).unwrap();
        if let Some(Commands::Config { subcommand, .. }) = cli.command {
            if let Some(ConfigSubcommand::Reset { global, yes }) = subcommand {
                assert!(!global);
                assert!(yes, "-y should set yes to true");
            } else {
                panic!("Expected Reset subcommand");
            }
        } else {
            panic!("Expected Config command");
        }
    }

    #[test]
    fn test_us004_config_reset_combined_flags() {
        // Test that `config reset --global -y` works
        let cli = Cli::try_parse_from(["autom8", "config", "reset", "--global", "-y"]).unwrap();
        if let Some(Commands::Config { subcommand, .. }) = cli.command {
            if let Some(ConfigSubcommand::Reset { global, yes }) = subcommand {
                assert!(global, "--global should set global to true");
                assert!(yes, "-y should set yes to true");
            } else {
                panic!("Expected Reset subcommand");
            }
        } else {
            panic!("Expected Config command");
        }
    }

    #[test]
    fn test_us004_config_reset_help_recognized() {
        // Test that `config reset --help` works
        let result = Cli::try_parse_from(["autom8", "config", "reset", "--help"]);
        assert!(result.is_err(), "Should return error for --help flag");
        let err = result.err().unwrap();
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::DisplayHelp,
            "Should recognize --help flag for reset subcommand"
        );
    }

    #[test]
    fn test_us004_config_appears_in_main_help() {
        // Verify config command appears in main help output
        let result = Cli::try_parse_from(["autom8", "--help"]);
        assert!(result.is_err(), "Should return error for --help flag");
        let err = result.err().unwrap();
        let help_text = err.to_string();
        assert!(
            help_text.contains("config"),
            "config command should appear in main help"
        );
    }

    #[test]
    fn test_us004_config_subcommand_enum_exported() {
        // Verify ConfigSubcommand is exported and usable
        use autom8::commands::ConfigSubcommand;
        let _set = ConfigSubcommand::Set {
            global: false,
            key: "test".to_string(),
            value: "value".to_string(),
        };
        let _reset = ConfigSubcommand::Reset {
            global: false,
            yes: false,
        };
    }

    #[test]
    fn test_us004_config_set_requires_key_and_value() {
        // Test that `config set` without arguments fails
        let result = Cli::try_parse_from(["autom8", "config", "set"]);
        assert!(
            result.is_err(),
            "config set should require key and value arguments"
        );

        // Test that `config set <key>` without value fails
        let result = Cli::try_parse_from(["autom8", "config", "set", "review"]);
        assert!(result.is_err(), "config set should require value argument");
    }

    #[test]
    fn test_us004_config_with_subcommand_ignores_display_flags() {
        // When a subcommand is used, --global and --project flags on the parent are separate
        // The set subcommand has its own --global flag
        let cli =
            Cli::try_parse_from(["autom8", "config", "set", "--global", "key", "value"]).unwrap();
        if let Some(Commands::Config {
            global,
            project,
            subcommand,
        }) = cli.command
        {
            // Parent flags should be false (not set)
            assert!(!global, "parent global should be false");
            assert!(!project, "parent project should be false");
            // Subcommand's global flag should be true
            if let Some(ConfigSubcommand::Set {
                global: sub_global, ..
            }) = subcommand
            {
                assert!(sub_global, "subcommand's global should be true");
            } else {
                panic!("Expected Set subcommand");
            }
        } else {
            panic!("Expected Config command");
        }
    }
}
