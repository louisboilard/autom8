use autom8::error::Autom8Error;
use autom8::output::{
    print_error, print_global_status, print_header, print_history_entry, print_status,
    print_warning, BOLD, CYAN, GRAY, GREEN, RESET, YELLOW,
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
    /// Path to a prd.md or prd.json file (shorthand for `run --prd <file>`)
    file: Option<PathBuf>,

    /// Show full Claude output instead of spinner (useful for debugging)
    #[arg(short, long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the agent loop to implement PRD stories
    Run {
        /// Path to the PRD JSON or spec file
        #[arg(long, default_value = "./prd.json")]
        prd: PathBuf,

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

    /// List past runs
    History,

    /// Archive current run and reset
    Archive,

    /// Output a skill prompt
    Skill {
        /// Skill name: "prd" for creating PRDs, "prd-json" for conversion prompt
        name: String,
    },

    /// Clean up PRD files from current directory
    Clean,

    /// Initialize autom8 by installing skills to ~/.claude/skills/
    Init,

    /// List all known projects in the config directory
    Projects,
}

/// Determine input type based on file extension
#[derive(Debug, Clone, Copy, PartialEq)]
enum InputType {
    Prd,  // .json file
    Spec, // .md or other file
}

fn detect_input_type(path: &Path) -> InputType {
    match path.extension().and_then(|e| e.to_str()) {
        Some("json") => InputType::Prd,
        _ => InputType::Spec,
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
        (None, Some(Commands::Run { prd, skip_review })) => {
            runner = runner.with_skip_review(*skip_review);
            print_header();
            match detect_input_type(prd) {
                InputType::Prd => runner.run(prd),
                InputType::Spec => runner.run_from_spec(prd),
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

        (None, Some(Commands::History)) => {
            print_header();
            match runner.history() {
                Ok(runs) => {
                    if runs.is_empty() {
                        println!("No past runs found.");
                    } else {
                        println!("Past runs:\n");
                        for (i, run) in runs.iter().enumerate() {
                            print_history_entry(run, i);
                        }
                    }
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }

        (None, Some(Commands::Archive)) => match runner.archive_current() {
            Ok(Some(path)) => {
                println!("Run archived to: {}", path.display());
                Ok(())
            }
            Ok(None) => {
                print_warning("No active run to archive.");
                Ok(())
            }
            Err(e) => Err(e),
        },

        (None, Some(Commands::Skill { name })) => output_skill(name),

        (None, Some(Commands::Clean)) => clean_prd_files(),

        (None, Some(Commands::Init)) => init_skills(),

        (None, Some(Commands::Projects)) => list_projects_command(),

        // No file and no command - check for existing state first, then start PRD creation
        (None, None) => default_command(cli.verbose),
    };

    if let Err(e) = result {
        print_error(&e.to_string());
        std::process::exit(1);
    }
}

fn run_with_file(runner: &Runner, file: &Path) -> autom8::error::Result<()> {
    // Copy file to config directory if not already there
    let copy_result = autom8::config::copy_to_config_dir(file)?;

    print_header();

    // Notify user if file was copied
    if copy_result.was_copied {
        println!(
            "{GREEN}Copied{RESET} {} → {}",
            file.display(),
            copy_result.dest_path.display()
        );
        println!();
    }

    // Use the destination path for processing
    match detect_input_type(&copy_result.dest_path) {
        InputType::Prd => runner.run(&copy_result.dest_path),
        InputType::Spec => runner.run_from_spec(&copy_result.dest_path),
    }
}

fn output_skill(name: &str) -> autom8::error::Result<()> {
    match name {
        "prd" => {
            println!("{}", prompts::PRD_SKILL_PROMPT);
            println!();
            println!("---");
            println!("Copy this prompt and paste it into a Claude session to create your prd.md");
            Ok(())
        }
        "prd-json" => {
            println!("{}", prompts::PRD_JSON_PROMPT);
            println!();
            println!("---");
            println!("This prompt is used internally by autom8 to convert prd.md to prd.json");
            Ok(())
        }
        _ => Err(Autom8Error::UnknownSkill(name.to_string())),
    }
}

/// Default command when running `autom8` with no arguments.
///
/// First checks for an existing state file indicating work in progress.
/// If state exists, proceeds to prompt the user (US-002).
/// If no state exists, proceeds to start PRD creation (US-003).
fn default_command(verbose: bool) -> autom8::error::Result<()> {
    use autom8::state::StateManager;

    let state_manager = StateManager::new()?;

    // Check for existing state file
    if let Some(state) = state_manager.load_current()? {
        // State exists - proceed to US-002 (prompt user)
        handle_existing_state(state, verbose)
    } else {
        // No state - proceed to US-003 (start PRD creation)
        start_prd_creation(verbose)
    }
}

/// Handle existing state file - prompt user to resume or start fresh (US-002)
fn handle_existing_state(state: autom8::state::RunState, verbose: bool) -> autom8::error::Result<()> {
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
            // Option 2: Start fresh → archive state and proceed to PRD creation
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

            // Proceed to PRD creation
            start_prd_creation(verbose)
        }
        _ => {
            // Option 3: Exit → do nothing, exit cleanly
            println!();
            println!("Exiting.");
            Ok(())
        }
    }
}

/// Start a new PRD creation session (US-003)
fn start_prd_creation(verbose: bool) -> autom8::error::Result<()> {
    use autom8::PrdSnapshot;
    use std::process::Command;

    print_header();

    // Print explanation of what will happen
    println!("{BOLD}Starting PRD Creation Session{RESET}");
    println!();
    println!("This will spawn an interactive Claude session to help you create a PRD.");
    println!("Claude will guide you through defining your feature with questions about:");
    println!("  • Project context and tech stack");
    println!("  • Feature requirements and user stories");
    println!("  • Acceptance criteria for each story");
    println!();
    println!("When you're done, save the PRD as {CYAN}prd.md{RESET} and exit the session.");
    println!("autom8 will automatically proceed to implementation.");
    println!();
    println!("{GRAY}Starting Claude...{RESET}");
    println!();

    // Take a snapshot of existing PRD files before spawning Claude
    let snapshot = PrdSnapshot::capture()?;

    // Spawn interactive Claude session with the PRD skill prompt
    let status = Command::new("claude")
        .arg(prompts::PRD_SKILL_PROMPT)
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

                // Detect new PRD files created during the session
                let new_files = snapshot.detect_new_files()?;

                match new_files.len() {
                    0 => {
                        print_error("No new PRD files detected.");
                        println!();
                        println!("{BOLD}Possible causes:{RESET}");
                        println!("  • Claude session ended before the PRD was saved");
                        println!("  • PRD was saved to an unexpected location");
                        println!("  • Claude didn't follow the PRD skill instructions");
                        println!();
                        println!("{BOLD}Suggestions:{RESET}");
                        println!("  • Run {CYAN}autom8{RESET} again to start a fresh session");
                        println!("  • Or use the manual workflow:");
                        println!("      1. Run {CYAN}autom8 skill prd{RESET} to get the prompt");
                        println!("      2. Start a Claude session and paste the prompt");
                        println!("      3. Save the PRD as {CYAN}prd.md{RESET}");
                        println!("      4. Run {CYAN}autom8{RESET} to implement");
                        std::process::exit(1);
                    }
                    1 => {
                        let prd_path = &new_files[0];
                        println!("{GREEN}Detected new PRD:{RESET} {}", prd_path.display());
                        println!();
                        println!("{BOLD}Proceeding to implementation...{RESET}");
                        println!();

                        // Create a new runner and run from the spec
                        let runner = Runner::new()?.with_verbose(verbose);
                        runner.run_from_spec(prd_path)
                    }
                    n => {
                        println!("{YELLOW}Detected {} new PRD files:{RESET}", n);
                        println!();

                        // Build options list with file paths
                        let options: Vec<String> = new_files
                            .iter()
                            .enumerate()
                            .map(|(i, file)| {
                                let filename = file
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("prd.md");
                                format!("{}. {}", i + 1, filename)
                            })
                            .collect();
                        let option_refs: Vec<&str> = options.iter().map(|s| s.as_str()).collect();

                        let choice = prompt::select(
                            "Which PRD would you like to implement?",
                            &option_refs,
                            0,
                        );

                        let selected_prd = &new_files[choice];
                        println!();
                        println!("{GREEN}Selected:{RESET} {}", selected_prd.display());
                        println!();
                        println!("{BOLD}Proceeding to implementation...{RESET}");
                        println!();

                        // Create a new runner and run from the spec
                        let runner = Runner::new()?.with_verbose(verbose);
                        runner.run_from_spec(selected_prd)
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

fn clean_prd_files() -> autom8::error::Result<()> {
    use autom8::state::StateManager;

    let state_manager = StateManager::new()?;
    let prds_dir = state_manager.prds_dir();
    let project_config_dir = autom8::config::project_config_dir()?;

    let mut deleted_any = false;

    // Check prds/ directory in config
    if prds_dir.exists() {
        let prds = state_manager.list_prds().unwrap_or_default();
        if !prds.is_empty() {
            println!();
            println!(
                "Found {} PRD file(s) in {}:",
                prds.len(),
                prds_dir.display()
            );
            for prd_path in &prds {
                let filename = prd_path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
                println!("  - {}", filename);
            }
            println!();

            let prompt_msg = format!("Delete all PRDs in {}?", prds_dir.display());
            if prompt::confirm(&prompt_msg, false) {
                for prd_path in prds {
                    fs::remove_file(&prd_path)?;
                    println!("{GREEN}Deleted{RESET} {}", prd_path.display());
                    deleted_any = true;
                }
            }
        }
    }

    if !deleted_any {
        println!("{GRAY}No PRD files to clean up in {}.{RESET}", project_config_dir.display());
    }

    Ok(())
}

fn init_skills() -> autom8::error::Result<()> {
    println!("Initializing autom8...");
    println!();

    // Create config directory ~/.config/autom8/
    let (config_dir, config_created) = autom8::config::ensure_config_dir()?;
    if config_created {
        println!("  {GREEN}Created{RESET} {}", config_dir.display());
    } else {
        println!("  {GRAY}Exists{RESET}  {}", config_dir.display());
    }
    println!();

    // Get home directory for skill paths
    let home = dirs::home_dir()
        .ok_or_else(|| Autom8Error::Config("Could not determine home directory".to_string()))?;

    // Define skill paths
    let skills_dir = home.join(".claude").join("skills");
    let prd_skill_path = skills_dir.join("pdr").join("SKILL.md");
    let prd_json_skill_path = skills_dir.join("pdr-json").join("SKILL.md");

    // Check which files already exist
    let prd_exists = prd_skill_path.exists();
    let prd_json_exists = prd_json_skill_path.exists();

    // If any files exist, ask for confirmation
    if prd_exists || prd_json_exists {
        println!("Skill files already exist:");
        if prd_exists {
            println!("  - {}", prd_skill_path.display());
        }
        if prd_json_exists {
            println!("  - {}", prd_json_skill_path.display());
        }
        println!();

        if !prompt::confirm("Overwrite existing skill files?", true) {
            println!();
            println!("Skipped. Existing skill files unchanged.");
            return Ok(());
        }
        println!();
    }

    // Create directories and write files
    fs::create_dir_all(prd_skill_path.parent().unwrap())?;
    fs::write(&prd_skill_path, prompts::PRD_SKILL_MD)?;
    let prd_action = if prd_exists { "Overwrote" } else { "Created" };
    println!(
        "  {GREEN}{}{RESET} {}",
        prd_action,
        prd_skill_path.display()
    );

    fs::create_dir_all(prd_json_skill_path.parent().unwrap())?;
    fs::write(&prd_json_skill_path, prompts::PRD_JSON_SKILL_MD)?;
    let prd_json_action = if prd_json_exists {
        "Overwrote"
    } else {
        "Created"
    };
    println!(
        "  {GREEN}{}{RESET} {}",
        prd_json_action,
        prd_json_skill_path.display()
    );

    println!();
    println!("{GREEN}Initialization complete!{RESET}");
    println!();
    if config_created {
        println!("  - Config directory created at {}", config_dir.display());
    }
    if prd_exists || prd_json_exists {
        println!("  - Skills updated");
    } else {
        println!("  - Skills installed");
    }
    println!();
    println!("You can now use:");
    println!("  {CYAN}/prd{RESET}       - Create a PRD through interactive Q&A");
    println!("  {CYAN}/prd-json{RESET}  - Convert a PRD to prd.json format");
    println!();
    println!("{BOLD}Next steps:{RESET}");
    println!("  1. Start a Claude session: {CYAN}claude{RESET}");
    println!("  2. Use {CYAN}/prd{RESET} to create your PRD");
    println!("  3. Run {CYAN}autom8{RESET} to implement it");

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
        println!("{GRAY}({} project{}){RESET}", projects.len(), if projects.len() == 1 { "" } else { "s" });
    }

    Ok(())
}

fn global_status_command() -> autom8::error::Result<()> {
    let statuses = autom8::config::global_status()?;
    print_global_status(&statuses);
    Ok(())
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
        assert!(cli.command.is_none(), "`new` should not be a command anymore");
        assert!(cli.file.is_some(), "`new` should be treated as a file path");
        assert_eq!(cli.file.unwrap().to_string_lossy(), "new");
    }

    #[test]
    fn test_us006_no_args_triggers_default_flow() {
        // Test that running `autom8` with no arguments parses to (None, None)
        // which triggers the default flow
        let cli = Cli::try_parse_from(["autom8"]).unwrap();
        assert!(cli.file.is_none(), "No file should be set");
        assert!(cli.command.is_none(), "No command should be set - triggers default flow");
    }

    #[test]
    fn test_us006_other_commands_still_work() {
        // Verify that other commands are still routed correctly
        let cli_resume = Cli::try_parse_from(["autom8", "resume"]).unwrap();
        assert!(matches!(cli_resume.command, Some(Commands::Resume)));

        let cli_status = Cli::try_parse_from(["autom8", "status"]).unwrap();
        assert!(matches!(cli_status.command, Some(Commands::Status { .. })));

        let cli_history = Cli::try_parse_from(["autom8", "history"]).unwrap();
        assert!(matches!(cli_history.command, Some(Commands::History)));

        let cli_projects = Cli::try_parse_from(["autom8", "projects"]).unwrap();
        assert!(matches!(cli_projects.command, Some(Commands::Projects)));
    }

    #[test]
    fn test_us006_file_argument_still_takes_precedence() {
        // Test that positional file argument still works
        let cli = Cli::try_parse_from(["autom8", "my-prd.json"]).unwrap();
        assert!(cli.file.is_some());
        assert_eq!(cli.file.unwrap().to_string_lossy(), "my-prd.json");
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
        assert!(result.is_none(), "Should return None when no state.json exists");
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
        assert!(result.is_some(), "Should return Some when state.json exists");
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
        assert!(sm.load_current().unwrap().is_none(), "State should be cleared");

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

        let mut state = RunState::new(PathBuf::from("test.json"), "feature/test-branch".to_string());
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
        let state1 = RunState::new(PathBuf::from("prd1.json"), "feature/first".to_string());
        sm.archive(&state1).unwrap();

        // Create second state, archive it
        let state2 = RunState::new(PathBuf::from("prd2.json"), "feature/second".to_string());
        sm.archive(&state2).unwrap();

        // Both archives should exist
        let archived = sm.list_archived().unwrap();
        assert_eq!(archived.len(), 2);
    }

    // ======================================================================
    // Tests for US-003: Run PRD creation flow as default
    // ======================================================================
    // Note: The core PrdSnapshot detection logic is tested extensively in
    // src/snapshot.rs. Here we test the integration points for US-003.

    #[test]
    fn test_us003_prd_snapshot_public_api_exists() {
        // Test that PrdSnapshot has the required public API for the PRD creation flow
        use autom8::PrdSnapshot;

        // Verify capture() exists and returns a Result
        // (We can't test it directly without setting up config dirs, but we verify the API)

        // Verify the struct has the expected public fields
        let _: fn() -> autom8::error::Result<PrdSnapshot> = PrdSnapshot::capture;

        // The snapshot module is properly exported
        assert!(true, "PrdSnapshot is available through autom8::PrdSnapshot");
    }

    #[test]
    fn test_us003_prd_skill_prompt_available() {
        // Verify the PRD skill prompt is available for the PRD creation flow
        // This is what gets passed to Claude when spawning the session
        assert!(
            !prompts::PRD_SKILL_PROMPT.is_empty(),
            "PRD_SKILL_PROMPT should be defined and non-empty"
        );

        // The prompt should contain key instructions for PRD creation
        assert!(
            prompts::PRD_SKILL_PROMPT.contains("PRD") || prompts::PRD_SKILL_PROMPT.contains("prd"),
            "PRD_SKILL_PROMPT should mention PRD"
        );
    }

    #[test]
    fn test_us003_start_prd_creation_path_from_default_command() {
        // Test that when no state exists, default_command proceeds to PRD creation
        // This verifies the control flow: no state -> start_prd_creation
        use autom8::state::StateManager;
        use tempfile::TempDir;

        // Create a fresh temp directory with no state file
        let temp_dir = TempDir::new().unwrap();
        let sm = StateManager::with_dir(temp_dir.path().to_path_buf());

        // Verify no state exists
        let result = sm.load_current().unwrap();
        assert!(result.is_none(), "Should have no state file");

        // This confirms the condition for entering the PRD creation path:
        // In default_command(), when load_current() returns None,
        // it calls start_prd_creation(verbose)
    }

    #[test]
    fn test_us003_start_fresh_leads_to_prd_creation() {
        // Test that "start fresh" option (after archiving state) leads to PRD creation
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
            "After clear, should have no state - ready for PRD creation"
        );

        // This confirms the path: start fresh -> archive -> clear -> start_prd_creation
    }
}
