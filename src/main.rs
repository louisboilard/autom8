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

        // No file and no command - auto-detect
        (None, None) => auto_detect_and_run(&runner),
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

/// Auto-detect PRD files and run appropriately
///
/// Looks in `~/.config/autom8/<project-name>/prds/` for incomplete PRDs.
/// Does NOT check legacy `.autom8/` or project root directories (clean break).
fn auto_detect_and_run(runner: &Runner) -> autom8::error::Result<()> {
    use autom8::prd::Prd;
    use autom8::state::StateManager;

    print_header();
    println!("{YELLOW}[detecting]{RESET} Scanning for PRD files...");
    println!();

    // Check config directory prds/ for incomplete PRDs
    let state_manager = StateManager::new()?;
    let prds_in_config = state_manager.list_prds().unwrap_or_default();
    let incomplete_prds: Vec<_> = prds_in_config
        .iter()
        .filter_map(|path| {
            Prd::load(path).ok().and_then(|prd| {
                if prd.is_incomplete() {
                    Some((path.clone(), prd))
                } else {
                    None
                }
            })
        })
        .collect();

    if !incomplete_prds.is_empty() {
        if incomplete_prds.len() == 1 {
            let (path, prd) = &incomplete_prds[0];
            let (completed, total) = prd.progress();
            prompt::print_found(
                "incomplete PRD",
                &format!("{} ({}/{})", path.display(), completed, total),
            );
            println!();

            let choice = prompt::select(
                &format!(
                    "Found incomplete PRD: {}. What would you like to do?",
                    prd.project
                ),
                &["Continue implementation", "Delete and start fresh", "Exit"],
                0,
            );

            match choice {
                0 => {
                    println!();
                    prompt::print_action(&format!("Resuming {}", prd.project));
                    println!();
                    runner.run(path)
                }
                1 => {
                    fs::remove_file(path).ok();
                    println!();
                    println!("{GREEN}Deleted{RESET} {}", path.display());
                    println!();
                    print_getting_started();
                    Ok(())
                }
                _ => {
                    println!();
                    println!("Exiting.");
                    Ok(())
                }
            }
        } else {
            // Multiple incomplete PRDs
            println!(
                "{BOLD}Found {} incomplete PRDs:{RESET}",
                incomplete_prds.len()
            );
            println!();

            let options: Vec<String> = incomplete_prds
                .iter()
                .map(|(path, prd)| {
                    let (completed, total) = prd.progress();
                    let filename = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("prd.json");
                    format!("{} - {} ({}/{})", filename, prd.project, completed, total)
                })
                .chain(std::iter::once("Exit".to_string()))
                .collect();

            let option_refs: Vec<&str> = options.iter().map(|s| s.as_str()).collect();
            let choice = prompt::select("Which PRD would you like to resume?", &option_refs, 0);

            if choice >= incomplete_prds.len() {
                println!();
                println!("Exiting.");
                return Ok(());
            }

            let (path, prd) = &incomplete_prds[choice];
            println!();
            prompt::print_action(&format!("Resuming {}", prd.project));
            println!();
            runner.run(path)
        }
    } else {
        // No incomplete PRDs found in config directory
        println!("{GRAY}No PRD files found.{RESET}");
        println!();
        print_getting_started();
        Ok(())
    }
}

fn print_getting_started() {
    println!("{BOLD}Getting Started{RESET}");
    println!();
    println!("  1. Run {CYAN}autom8 skill prd{RESET} to get the PRD creation prompt");
    println!("  2. Start a Claude session and paste the prompt");
    println!("  3. Describe your feature through the interactive Q&A");
    println!("  4. Save Claude's output as {BOLD}prd.md{RESET}");
    println!("  5. Run {CYAN}autom8{RESET} to implement your feature");
    println!();
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
