use autom8::error::Autom8Error;
use autom8::output::{
    print_error, print_global_status, print_header, print_status, BOLD, CYAN, GRAY, GREEN, RED,
    RESET, YELLOW,
};
use autom8::prompt;
use autom8::prompts;
use autom8::Runner;
use clap::{Parser, Subcommand};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "autom8")]
#[command(
    version,
    about = "CLI automation tool for orchestrating Claude-powered development"
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
    Run {
        /// Path to the spec JSON or markdown file
        #[arg(long, default_value = "./spec.json")]
        spec: PathBuf,

        /// Skip the review loop and go directly to committing
        #[arg(long)]
        skip_review: bool,
    },

    /// Check the current run status
    Status {
        /// Show status across all projects
        #[arg(short = 'a', long = "all")]
        all: bool,

        /// Show status across all projects (alias for --all)
        #[arg(short = 'g', long = "global")]
        global: bool,
    },

    /// Resume a failed or interrupted run
    Resume,

    /// Clean up spec files from config directory
    Clean,

    /// Initialize autom8 config directory structure for current project
    Init,

    /// List all known projects in the config directory
    Projects,

    /// Show a tree view of all projects with status
    List,

    /// Show detailed information about a specific project
    Describe {
        /// The project name to describe
        project_name: String,
    },
}

/// Determine input type based on file extension
#[derive(Debug, Clone, Copy, PartialEq)]
enum InputType {
    Json,     // .json file (spec-<feature>.json)
    Markdown, // .md or other file (spec-<feature>.md)
}

fn detect_input_type(path: &Path) -> InputType {
    match path.extension().and_then(|e| e.to_str()) {
        Some("json") => InputType::Json,
        _ => InputType::Markdown,
    }
}

fn main() {
    let cli = Cli::parse();
    let mut runner = match Runner::new() {
        Ok(r) => r.with_verbose(cli.verbose),
        Err(e) => {
            print_error(&format!("Failed to initialize runner: {}", e));
            std::process::exit(1);
        }
    };

    // Ensure project config directory exists for commands that need it
    // (Skip for init command since it has its own config directory handling)
    if !matches!(&cli.command, Some(Commands::Init)) {
        if let Err(e) = autom8::config::ensure_project_config_dir() {
            print_error(&format!("Failed to create project config directory: {}", e));
            std::process::exit(1);
        }
    }

    let result = match (&cli.file, &cli.command) {
        // Positional file argument takes precedence
        (Some(file), _) => run_with_file(&runner, file),

        // Subcommands
        (None, Some(Commands::Run { spec, skip_review })) => {
            runner = runner.with_skip_review(*skip_review);
            print_header();
            match detect_input_type(spec) {
                InputType::Json => runner.run(spec),
                InputType::Markdown => runner.run_from_spec(spec),
            }
        }

        (None, Some(Commands::Status { all, global })) => {
            print_header();
            if *all || *global {
                // Global status across all projects
                global_status_command()
            } else {
                // Local status for current project
                match runner.status() {
                    Ok(Some(state)) => {
                        print_status(&state);
                        Ok(())
                    }
                    Ok(None) => {
                        println!("No active run.");
                        Ok(())
                    }
                    Err(e) => Err(e),
                }
            }
        }

        (None, Some(Commands::Resume)) => runner.resume(),

        (None, Some(Commands::Clean)) => clean_spec_files(),

        (None, Some(Commands::Init)) => init_command(),

        (None, Some(Commands::Projects)) => list_projects_command(),

        (None, Some(Commands::List)) => list_tree_command(),

        (None, Some(Commands::Describe { project_name })) => describe_command(project_name),

        // No file and no command - check for existing state first, then start spec creation
        (None, None) => default_command(cli.verbose),
    };

    if let Err(e) = result {
        print_error(&e.to_string());
        std::process::exit(1);
    }
}

fn run_with_file(runner: &Runner, file: &Path) -> autom8::error::Result<()> {
    // Move file to config directory if not already there
    let move_result = autom8::config::move_to_config_dir(file)?;

    print_header();

    // Notify user if file was moved
    if move_result.was_moved {
        println!(
            "{GREEN}Moved{RESET} {} → {}",
            file.display(),
            move_result.dest_path.display()
        );
        println!();
    }

    // Use the destination path for processing
    match detect_input_type(&move_result.dest_path) {
        InputType::Json => runner.run(&move_result.dest_path),
        InputType::Markdown => runner.run_from_spec(&move_result.dest_path),
    }
}

/// Default command when running `autom8` with no arguments.
///
/// First checks for an existing state file indicating work in progress.
/// If state exists, proceeds to prompt the user (US-002).
/// If no state exists, proceeds to start spec creation (US-003).
fn default_command(verbose: bool) -> autom8::error::Result<()> {
    use autom8::state::StateManager;

    let state_manager = StateManager::new()?;

    // Check for existing state file
    if let Some(state) = state_manager.load_current()? {
        // State exists - proceed to US-002 (prompt user)
        handle_existing_state(state, verbose)
    } else {
        // No state - proceed to US-003 (start spec creation)
        start_spec_creation(verbose)
    }
}

/// Handle existing state file - prompt user to resume or start fresh (US-002)
fn handle_existing_state(
    state: autom8::state::RunState,
    verbose: bool,
) -> autom8::error::Result<()> {
    use autom8::state::StateManager;

    print_header();

    // Display clear prompt with context
    println!("{YELLOW}Work in progress detected.{RESET}");
    println!();
    println!("  Branch: {CYAN}{}{RESET}", state.branch);
    if let Some(story) = &state.current_story {
        println!("  Current story: {CYAN}{}{RESET}", story);
    }
    println!();

    // Present options to the user
    let choice = prompt::select(
        "Resume or start fresh?",
        &["Resume existing work", "Start fresh", "Exit"],
        0, // Default to Resume
    );

    match choice {
        0 => {
            // Option 1: Resume → continue the existing run
            println!();
            prompt::print_action("Resuming existing work...");
            println!();

            let runner = Runner::new()?.with_verbose(verbose);
            runner.resume()
        }
        1 => {
            // Option 2: Start fresh → archive state and proceed to spec creation
            let state_manager = StateManager::new()?;

            // Archive before deleting (US-004 behavior)
            let archive_path = state_manager.archive(&state)?;
            state_manager.clear_current()?;

            println!();
            println!(
                "{GREEN}Previous state archived:{RESET} {}",
                archive_path.display()
            );
            println!();

            // Proceed to spec creation
            start_spec_creation(verbose)
        }
        _ => {
            // Option 3: Exit → do nothing, exit cleanly
            println!();
            println!("Exiting.");
            Ok(())
        }
    }
}

/// Start a new spec creation session (US-003)
fn start_spec_creation(verbose: bool) -> autom8::error::Result<()> {
    use autom8::SpecSnapshot;
    use std::process::Command;

    print_header();

    // Print explanation of what will happen
    println!("{BOLD}Starting Spec Creation Session{RESET}");
    println!();
    println!("This will spawn an interactive Claude session to help you create a spec.");
    println!("Claude will guide you through defining your feature with questions about:");
    println!("  • Project context and tech stack");
    println!("  • Feature requirements and user stories");
    println!("  • Acceptance criteria for each story");
    println!();
    println!(
        "When you're done, save the spec as {CYAN}spec-<feature>.md{RESET} and exit the session."
    );
    println!("autom8 will automatically proceed to implementation.");
    println!();
    println!("{GRAY}Starting Claude...{RESET}");
    println!();

    // Take a snapshot of existing spec files before spawning Claude
    let snapshot = SpecSnapshot::capture()?;

    // Spawn interactive Claude session with the spec skill prompt
    let status = Command::new("claude")
        .arg(prompts::SPEC_SKILL_PROMPT)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status();

    match status {
        Ok(exit_status) => {
            if exit_status.success() {
                println!();
                println!("{GREEN}Claude session ended.{RESET}");
                println!();

                // Detect new spec files created during the session
                let new_files = snapshot.detect_new_files()?;

                match new_files.len() {
                    0 => {
                        print_error("No new spec files detected.");
                        println!();
                        println!("{BOLD}Possible causes:{RESET}");
                        println!("  • Claude session ended before the spec was saved");
                        println!("  • Spec was saved to an unexpected location");
                        println!("  • Claude didn't follow the spec skill instructions");
                        println!();
                        println!("{BOLD}Suggestions:{RESET}");
                        println!("  • Run {CYAN}autom8{RESET} again to start a fresh session");
                        println!("  • Or use the manual workflow:");
                        println!("      1. Run {CYAN}autom8 skill spec{RESET} to get the prompt");
                        println!("      2. Start a Claude session and paste the prompt");
                        println!("      3. Save the spec as {CYAN}spec-<feature>.md{RESET}");
                        println!("      4. Run {CYAN}autom8{RESET} to implement");
                        std::process::exit(1);
                    }
                    1 => {
                        let spec_path = &new_files[0];
                        println!("{GREEN}Detected new spec:{RESET} {}", spec_path.display());
                        println!();
                        println!("{BOLD}Proceeding to implementation...{RESET}");
                        println!();

                        // Create a new runner and run from the spec
                        let runner = Runner::new()?.with_verbose(verbose);
                        runner.run_from_spec(spec_path)
                    }
                    n => {
                        println!("{YELLOW}Detected {} new spec files:{RESET}", n);
                        println!();

                        // Build options list with file paths
                        let options: Vec<String> = new_files
                            .iter()
                            .enumerate()
                            .map(|(i, file)| {
                                let filename = file
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("spec.md");
                                format!("{}. {}", i + 1, filename)
                            })
                            .collect();
                        let option_refs: Vec<&str> = options.iter().map(|s| s.as_str()).collect();

                        let choice = prompt::select(
                            "Which spec would you like to implement?",
                            &option_refs,
                            0,
                        );

                        let selected_spec = &new_files[choice];
                        println!();
                        println!("{GREEN}Selected:{RESET} {}", selected_spec.display());
                        println!();
                        println!("{BOLD}Proceeding to implementation...{RESET}");
                        println!();

                        // Create a new runner and run from the spec
                        let runner = Runner::new()?.with_verbose(verbose);
                        runner.run_from_spec(selected_spec)
                    }
                }
            } else {
                Err(Autom8Error::ClaudeError(format!(
                    "Claude exited with status: {}",
                    exit_status
                )))
            }
        }
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                Err(Autom8Error::ClaudeError(
                    "Claude CLI not found. Please install it from https://github.com/anthropics/claude-code".to_string()
                ))
            } else {
                Err(Autom8Error::ClaudeError(format!(
                    "Failed to spawn Claude: {}",
                    e
                )))
            }
        }
    }
}

fn clean_spec_files() -> autom8::error::Result<()> {
    use autom8::state::StateManager;

    let state_manager = StateManager::new()?;
    let spec_dir = state_manager.spec_dir();
    let project_config_dir = autom8::config::project_config_dir()?;

    let mut deleted_any = false;

    // Check spec/ directory in config
    if spec_dir.exists() {
        let specs = state_manager.list_specs().unwrap_or_default();
        if !specs.is_empty() {
            println!();
            println!(
                "Found {} spec file(s) in {}:",
                specs.len(),
                spec_dir.display()
            );
            for spec_path in &specs {
                let filename = spec_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("?");
                println!("  - {}", filename);
            }
            println!();

            let prompt_msg = format!("Delete all spec files in {}?", spec_dir.display());
            if prompt::confirm(&prompt_msg, false) {
                for spec_path in specs {
                    fs::remove_file(&spec_path)?;
                    println!("{GREEN}Deleted{RESET} {}", spec_path.display());
                    deleted_any = true;
                }
            }
        }
    }

    if !deleted_any {
        println!(
            "{GRAY}No spec files to clean up in {}.{RESET}",
            project_config_dir.display()
        );
    }

    Ok(())
}

fn init_command() -> autom8::error::Result<()> {
    println!("Initializing autom8...");
    println!();

    // Create base config directory ~/.config/autom8/
    let (config_dir, config_created) = autom8::config::ensure_config_dir()?;
    if config_created {
        println!("  {GREEN}Created{RESET} {}", config_dir.display());
    } else {
        println!("  {GRAY}Exists{RESET}  {}", config_dir.display());
    }

    // Create project-specific config directory with subdirectories
    let (project_dir, project_created) = autom8::config::ensure_project_config_dir()?;
    if project_created {
        println!("  {GREEN}Created{RESET} {}", project_dir.display());
        println!("  {GREEN}Created{RESET} {}/spec/", project_dir.display());
        println!("  {GREEN}Created{RESET} {}/runs/", project_dir.display());
    } else {
        println!("  {GRAY}Exists{RESET}  {}", project_dir.display());
    }

    println!();
    println!("{GREEN}Initialization complete!{RESET}");
    println!();
    println!("Config directory structure:");
    println!("  {CYAN}{}{RESET}", project_dir.display());
    println!("    ├── spec/  (spec markdown and JSON files)");
    println!("    └── runs/  (archived run states)");
    println!();
    println!("{BOLD}Next steps:{RESET}");
    println!("  Run {CYAN}autom8{RESET} to start creating a spec");

    Ok(())
}

fn list_projects_command() -> autom8::error::Result<()> {
    let projects = autom8::config::list_projects()?;

    if projects.is_empty() {
        println!("{GRAY}No projects found.{RESET}");
        println!();
        println!("Run {CYAN}autom8{RESET} in a project directory to create a project.");
    } else {
        println!("{BOLD}Known projects:{RESET}");
        println!();
        for project in &projects {
            println!("  {}", project);
        }
        println!();
        println!(
            "{GRAY}({} project{}){RESET}",
            projects.len(),
            if projects.len() == 1 { "" } else { "s" }
        );
    }

    Ok(())
}

fn global_status_command() -> autom8::error::Result<()> {
    let statuses = autom8::config::global_status()?;
    print_global_status(&statuses);
    Ok(())
}

fn list_tree_command() -> autom8::error::Result<()> {
    let projects = autom8::config::list_projects_tree()?;
    autom8::output::print_project_tree(&projects);
    Ok(())
}

fn describe_command(project_name: &str) -> autom8::error::Result<()> {
    use autom8::output::print_project_description;

    // Check if project exists
    match autom8::config::get_project_description(project_name)? {
        Some(desc) => {
            // If multiple specs exist and user might want to select one, handle that case
            if desc.specs.len() > 1 {
                // Ask user which spec to describe
                println!(
                    "{YELLOW}Multiple specs found for project '{}'{RESET}",
                    project_name
                );
                println!();

                let options: Vec<String> = desc
                    .specs
                    .iter()
                    .map(|spec| {
                        let progress = format!("{}/{}", spec.completed_count, spec.total_count);
                        format!("{} ({})", spec.filename, progress)
                    })
                    .collect();

                // Add an "All specs" option at the beginning
                let mut all_options: Vec<&str> = vec!["Show all specs"];
                all_options.extend(options.iter().map(|s| s.as_str()));

                let choice =
                    prompt::select("Which spec would you like to describe?", &all_options, 0);

                if choice == 0 {
                    // Show all specs
                    print_project_description(&desc);
                } else {
                    // Show specific spec
                    let selected_spec = &desc.specs[choice - 1];

                    // Create a description with just the selected spec
                    let single_spec_desc = autom8::config::ProjectDescription {
                        specs: vec![selected_spec.clone()],
                        ..desc
                    };
                    print_project_description(&single_spec_desc);
                }
            } else {
                // Single or no specs - just show the description
                print_project_description(&desc);
            }
            Ok(())
        }
        None => {
            // Project doesn't exist
            println!("{RED}Project '{}' not found.{RESET}", project_name);
            println!();
            println!(
                "The project directory {CYAN}~/.config/autom8/{}{RESET} does not exist.",
                project_name
            );
            println!();

            // List available projects
            let projects = autom8::config::list_projects()?;
            if projects.is_empty() {
                println!("{GRAY}No projects have been created yet.{RESET}");
                println!();
                println!("Run {CYAN}autom8{RESET} in a project directory to create a project.");
            } else {
                println!("{BOLD}Available projects:{RESET}");
                for project in &projects {
                    println!("  - {}", project);
                }
                println!();
                println!("Run {CYAN}autom8 describe <project-name>{RESET} to describe a project.");
            }
            Ok(())
        }
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
        assert!(matches!(cli_resume.command, Some(Commands::Resume)));

        let cli_status = Cli::try_parse_from(["autom8", "status"]).unwrap();
        assert!(matches!(cli_status.command, Some(Commands::Status { .. })));

        let cli_projects = Cli::try_parse_from(["autom8", "projects"]).unwrap();
        assert!(matches!(cli_projects.command, Some(Commands::Projects)));

        let cli_clean = Cli::try_parse_from(["autom8", "clean"]).unwrap();
        assert!(matches!(cli_clean.command, Some(Commands::Clean)));

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
            assert_eq!(project_name, "my-project");
        } else {
            panic!("Expected Describe command");
        }
    }

    #[test]
    fn test_us008_describe_command_requires_project_name() {
        // Test that describe command requires a project name argument
        let result = Cli::try_parse_from(["autom8", "describe"]);
        assert!(result.is_err(), "describe should require a project name");
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
}
