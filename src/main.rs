//! autom8 CLI entry point.
//!
//! Parses command-line arguments and dispatches to the appropriate command handler.

use autom8::commands::{
    all_sessions_status_command, clean_command, config_display_command, config_reset_command,
    config_set_command, default_command, describe_command, global_status_command, gui_command,
    improve_command, init_command, list_command, monitor_command, pr_review_command,
    projects_command, resume_command, run_command, run_with_file, status_command, CleanOptions,
    ConfigScope, ConfigSubcommand,
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
        #[arg(long, default_value = "./spec.json", conflicts_with = "self_test")]
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

        /// Run a self-test with a hardcoded trivial spec to verify autom8 functionality.
        /// Bypasses the normal spec file requirement and cleans up all artifacts after completion.
        #[arg(long, conflicts_with = "spec")]
        self_test: bool,
    },

    /// Check the current run status
    #[command(after_help = "EXAMPLES:
    autom8 status             # Show current session status
    autom8 status --all       # Show all sessions for this project
    autom8 status --global    # Show status across all projects
    autom8 status --project myapp --all  # Show all sessions for a specific project

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

        /// Target project name.
        /// If not specified, uses the current directory to determine the project.
        #[arg(short, long)]
        project: Option<String>,
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
    autom8 clean --project myapp      # Clean a specific project by name

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

        /// Target project name.
        /// If not specified, uses the current directory to determine the project.
        #[arg(short, long)]
        project: Option<String>,
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
    Monitor,

    /// Launch the native GUI to monitor autom8 activity
    Gui,

    /// Continue iterating on a feature with Claude using context from previous runs
    #[command(after_help = "EXAMPLES:
    autom8 improve                  # Gather context and spawn Claude session
    autom8 improve -v               # Same, with verbose output (future use)

BEHAVIOR:
    The improve command auto-detects everything from the current git branch:
    1. Gathers git context (branch, commits, file changes)
    2. Loads spec if found (from session or by branch name)
    3. Extracts session knowledge (decisions, patterns, files, work summaries)
    4. Displays a brief summary of loaded context
    5. Spawns an interactive Claude session with the context

    Context is gathered additively - git context is always available,
    spec and session knowledge are included when a matching session exists.")]
    Improve,

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
                // Reset subcommand (US-003)
                Some(ConfigSubcommand::Reset { global: g, yes }) => config_reset_command(*g, *yes),
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
                        self_test,
                    }),
                ) => run_command(
                    cli.verbose,
                    spec,
                    *skip_review,
                    *worktree,
                    *no_worktree,
                    *self_test,
                ),

                (
                    None,
                    Some(Commands::Status {
                        all,
                        global,
                        project,
                    }),
                ) => {
                    print_header();
                    if *global {
                        global_status_command()
                    } else if *all {
                        all_sessions_status_command(project.as_deref())
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
                        project,
                    }),
                ) => clean_command(CleanOptions {
                    worktrees: *worktrees,
                    all: *all,
                    session: session.clone(),
                    orphaned: *orphaned,
                    force: *force,
                    project: project.clone(),
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

                (None, Some(Commands::Monitor)) => monitor_command(),

                (None, Some(Commands::Gui)) => gui_command(),

                (None, Some(Commands::Improve)) => improve_command(cli.verbose),

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

    // =========================================================================
    // CLI parsing tests - pure logic, no side effects
    // =========================================================================

    #[test]
    fn test_default_flow_and_file_argument() {
        // No args triggers default flow
        let cli = Cli::try_parse_from(["autom8"]).unwrap();
        assert!(cli.file.is_none());
        assert!(cli.command.is_none());

        // File argument is recognized
        let cli = Cli::try_parse_from(["autom8", "my-spec.json"]).unwrap();
        assert_eq!(cli.file.unwrap().to_string_lossy(), "my-spec.json");
        assert!(cli.command.is_none());
    }

    #[test]
    fn test_all_commands_recognized() {
        let commands = [
            "run",
            "resume",
            "status",
            "clean",
            "init",
            "projects",
            "list",
            "describe",
            "config",
            "monitor",
            "gui",
            "improve",
            "pr-review",
        ];

        for cmd in commands {
            let result = Cli::try_parse_from(["autom8", cmd]);
            assert!(result.is_ok(), "Command '{}' should parse", cmd);
        }

        // Completions requires shell arg
        assert!(Cli::try_parse_from(["autom8", "completions", "bash"]).is_ok());
    }

    #[test]
    fn test_version_flag() {
        for flag in ["--version", "-V"] {
            let result = Cli::try_parse_from(["autom8", flag]);
            assert!(result.is_err());
            assert_eq!(
                result.err().unwrap().kind(),
                clap::error::ErrorKind::DisplayVersion
            );
        }
        assert_eq!(env!("CARGO_PKG_VERSION"), "0.2.0");
    }

    #[test]
    fn test_removed_flags_error() {
        // --tui flag removed
        assert!(Cli::try_parse_from(["autom8", "--tui"]).is_err());
        assert!(Cli::try_parse_from(["autom8", "-t"]).is_err());

        // --project flag removed from monitor and gui
        assert!(Cli::try_parse_from(["autom8", "monitor", "--project", "x"]).is_err());
        assert!(Cli::try_parse_from(["autom8", "gui", "-p", "x"]).is_err());
    }

    // =========================================================================
    // State management tests
    // =========================================================================

    #[test]
    fn test_state_manager_load_save_clear() {
        use autom8::state::{RunState, StateManager};
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        // No state initially
        assert!(sm.load_current().unwrap().is_none());

        // Save and load
        let state = RunState::new(PathBuf::from("test.json"), "feature/test".to_string());
        sm.save(&state).unwrap();
        let loaded = sm.load_current().unwrap().unwrap();
        assert_eq!(loaded.branch, "feature/test");

        // Clear
        sm.clear_current(false).unwrap();
        assert!(sm.load_current().unwrap().is_none());
    }

    #[test]
    fn test_state_archive_workflow() {
        use autom8::state::{RunState, StateManager};
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        // Archive multiple states
        let state1 = RunState::new(PathBuf::from("spec1.json"), "feature/first".to_string());
        let state2 = RunState::new(PathBuf::from("spec2.json"), "feature/second".to_string());

        let archive1 = sm.archive(&state1).unwrap();
        let archive2 = sm.archive(&state2).unwrap();

        assert!(archive1.exists());
        assert!(archive2.exists());

        let archived = sm.list_archived().unwrap();
        assert_eq!(archived.len(), 2);
    }

    #[test]
    fn test_run_state_fields() {
        use autom8::state::RunState;

        let mut state = RunState::new(PathBuf::from("test.json"), "feature/test".to_string());
        assert_eq!(state.branch, "feature/test");
        assert!(state.current_story.is_none());

        state.current_story = Some("US-001".to_string());
        assert_eq!(state.current_story, Some("US-001".to_string()));
    }

    // =========================================================================
    // Config tests
    // =========================================================================

    #[test]
    fn test_config_defaults_and_paths() {
        let default_config = autom8::config::Config::default();
        assert!(default_config.review);
        assert!(default_config.commit);
        assert!(default_config.pull_request);
        assert!(default_config.worktree);

        let global_path = autom8::config::global_config_path().unwrap();
        let project_path = autom8::config::project_config_path().unwrap();
        assert!(global_path.ends_with("config.toml"));
        assert_ne!(global_path, project_path);
    }

    #[test]
    fn test_config_command_parsing() {
        // Default (no subcommand)
        let cli = Cli::try_parse_from(["autom8", "config"]).unwrap();
        if let Some(Commands::Config { subcommand, .. }) = cli.command {
            assert!(subcommand.is_none());
        }

        // Set subcommand with global flag
        let cli =
            Cli::try_parse_from(["autom8", "config", "set", "-g", "review", "false"]).unwrap();
        if let Some(Commands::Config { subcommand, .. }) = cli.command {
            if let Some(ConfigSubcommand::Set { global, key, value }) = subcommand {
                assert!(global);
                assert_eq!(key, "review");
                assert_eq!(value, "false");
            }
        }

        // Set requires key and value
        assert!(Cli::try_parse_from(["autom8", "config", "set"]).is_err());
        assert!(Cli::try_parse_from(["autom8", "config", "set", "review"]).is_err());
    }

    #[test]
    fn test_worktree_flags_mutual_exclusivity() {
        let cli = Cli::try_parse_from(["autom8", "run", "--worktree"]).unwrap();
        if let Some(Commands::Run {
            worktree,
            no_worktree,
            ..
        }) = cli.command
        {
            assert!(worktree);
            assert!(!no_worktree);
        }

        let cli = Cli::try_parse_from(["autom8", "run", "--no-worktree"]).unwrap();
        if let Some(Commands::Run {
            worktree,
            no_worktree,
            ..
        }) = cli.command
        {
            assert!(!worktree);
            assert!(no_worktree);
        }

        // Cannot use both together
        assert!(Cli::try_parse_from(["autom8", "run", "--worktree", "--no-worktree"]).is_err());
    }

    #[test]
    fn test_self_test_flag() {
        let cli = Cli::try_parse_from(["autom8", "run", "--self-test"]).unwrap();
        if let Some(Commands::Run { self_test, .. }) = cli.command {
            assert!(self_test);
        }

        // --self-test conflicts with --spec
        assert!(
            Cli::try_parse_from(["autom8", "run", "--self-test", "--spec", "test.json"]).is_err()
        );
    }

    #[test]
    fn test_status_command_flags() {
        let cli = Cli::try_parse_from(["autom8", "status", "-a", "--project", "myproj"]).unwrap();
        if let Some(Commands::Status {
            all,
            global,
            project,
        }) = cli.command
        {
            assert!(all);
            assert!(!global);
            assert_eq!(project, Some("myproj".to_string()));
        }

        let cli = Cli::try_parse_from(["autom8", "status", "-g"]).unwrap();
        if let Some(Commands::Status { global, .. }) = cli.command {
            assert!(global);
        }
    }

    #[test]
    fn test_resume_command_flags() {
        let cli = Cli::try_parse_from(["autom8", "resume", "-s", "abc123", "-l"]).unwrap();
        if let Some(Commands::Resume { session, list }) = cli.command {
            assert_eq!(session, Some("abc123".to_string()));
            assert!(list);
        }
    }

    #[test]
    fn test_describe_command() {
        let cli = Cli::try_parse_from(["autom8", "describe", "my-project"]).unwrap();
        if let Some(Commands::Describe { project_name }) = cli.command {
            assert_eq!(project_name, Some("my-project".to_string()));
        }

        let cli = Cli::try_parse_from(["autom8", "describe"]).unwrap();
        if let Some(Commands::Describe { project_name }) = cli.command {
            assert!(project_name.is_none());
        }
    }
}
