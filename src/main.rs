use autom8::error::Autom8Error;
use autom8::output::{
    print_error, print_header, print_history_entry, print_status, print_warning, BOLD, CYAN, GRAY,
    GREEN, RESET, YELLOW,
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

        /// Maximum number of iterations
        #[arg(long, default_value = "10")]
        max_iterations: u32,

        /// Skip the review loop and go directly to committing
        #[arg(long)]
        skip_review: bool,
    },

    /// Check the current run status
    Status,

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
    let mut runner = Runner::new().with_verbose(cli.verbose);

    let result = match (&cli.file, &cli.command) {
        // Positional file argument takes precedence
        (Some(file), _) => run_with_file(&runner, file),

        // Subcommands
        (
            None,
            Some(Commands::Run {
                prd,
                max_iterations,
                skip_review,
            }),
        ) => {
            runner = runner.with_skip_review(*skip_review);
            print_header();
            match detect_input_type(prd) {
                InputType::Prd => runner.run(prd, *max_iterations),
                InputType::Spec => runner.run_from_spec(prd, *max_iterations),
            }
        }

        (None, Some(Commands::Status)) => {
            print_header();
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

        // No file and no command - auto-detect
        (None, None) => auto_detect_and_run(&runner),
    };

    if let Err(e) = result {
        print_error(&e.to_string());
        std::process::exit(1);
    }
}

fn run_with_file(runner: &Runner, file: &Path) -> autom8::error::Result<()> {
    const DEFAULT_MAX_ITERATIONS: u32 = 10;

    print_header();
    match detect_input_type(file) {
        InputType::Prd => runner.run(file, DEFAULT_MAX_ITERATIONS),
        InputType::Spec => runner.run_from_spec(file, DEFAULT_MAX_ITERATIONS),
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
fn auto_detect_and_run(runner: &Runner) -> autom8::error::Result<()> {
    const DEFAULT_MAX_ITERATIONS: u32 = 10;

    let prd_json = Path::new("./prd.json");
    let prd_md = Path::new("./prd.md");

    let json_exists = prd_json.exists();
    let md_exists = prd_md.exists();

    print_header();
    println!("{YELLOW}[detecting]{RESET} Scanning for PRD files...");
    println!();

    match (json_exists, md_exists) {
        // Both files exist
        (true, true) => {
            prompt::print_found("prd.json", "./prd.json");
            prompt::print_found("prd.md", "./prd.md");
            println!();

            let choice = prompt::select(
                "Found existing PRD files. What would you like to do?",
                &[
                    "Continue with existing prd.json (resume implementation)",
                    "Regenerate prd.json from prd.md (start fresh)",
                    "Clean up and start over (delete both files)",
                    "Exit",
                ],
                0,
            );

            match choice {
                0 => {
                    println!();
                    prompt::print_action("Continuing with existing prd.json");
                    println!();
                    runner.run(prd_json, DEFAULT_MAX_ITERATIONS)
                }
                1 => {
                    println!();
                    prompt::print_action("Regenerating prd.json from prd.md");
                    // Delete existing prd.json first
                    fs::remove_file(prd_json).ok();
                    println!();
                    runner.run_from_spec(prd_md, DEFAULT_MAX_ITERATIONS)
                }
                2 => {
                    clean_prd_files()?;
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
        }

        // Only prd.json exists
        (true, false) => {
            prompt::print_found("prd.json", "./prd.json");
            println!();

            let choice = prompt::select(
                "Found existing prd.json. What would you like to do?",
                &["Continue implementation", "Delete and start fresh", "Exit"],
                0,
            );

            match choice {
                0 => {
                    println!();
                    prompt::print_action("Starting implementation from prd.json");
                    println!();
                    runner.run(prd_json, DEFAULT_MAX_ITERATIONS)
                }
                1 => {
                    fs::remove_file(prd_json).ok();
                    println!();
                    println!("{GREEN}Deleted{RESET} prd.json");
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
        }

        // Only prd.md exists
        (false, true) => {
            prompt::print_found("prd.md", "./prd.md");
            println!();

            let choice = prompt::select(
                "Found prd.md spec file. What would you like to do?",
                &[
                    "Convert to prd.json and start implementation",
                    "Delete and start fresh",
                    "Exit",
                ],
                0,
            );

            match choice {
                0 => {
                    println!();
                    prompt::print_action(
                        "Converting prd.md to prd.json and starting implementation",
                    );
                    println!();
                    runner.run_from_spec(prd_md, DEFAULT_MAX_ITERATIONS)
                }
                1 => {
                    fs::remove_file(prd_md).ok();
                    println!();
                    println!("{GREEN}Deleted{RESET} prd.md");
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
        }

        // No PRD files found
        (false, false) => {
            println!("{GRAY}No PRD files found in current directory.{RESET}");
            println!();
            print_getting_started();
            Ok(())
        }
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
    let prd_json = Path::new("./prd.json");
    let prd_md = Path::new("./prd.md");

    let mut deleted_any = false;

    if prd_json.exists() {
        fs::remove_file(prd_json)?;
        println!("{GREEN}Deleted{RESET} prd.json");
        deleted_any = true;
    }

    if prd_md.exists() {
        fs::remove_file(prd_md)?;
        println!("{GREEN}Deleted{RESET} prd.md");
        deleted_any = true;
    }

    if !deleted_any {
        println!("{GRAY}No PRD files to clean up.{RESET}");
    }

    Ok(())
}

fn init_skills() -> autom8::error::Result<()> {
    println!("Initializing autom8 skills...");
    println!();

    // Get home directory
    let home = dirs::home_dir().ok_or_else(|| {
        Autom8Error::Config("Could not determine home directory".to_string())
    })?;

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
    println!("  {GREEN}{}{RESET} {}", prd_action, prd_skill_path.display());

    fs::create_dir_all(prd_json_skill_path.parent().unwrap())?;
    fs::write(&prd_json_skill_path, prompts::PRD_JSON_SKILL_MD)?;
    let prd_json_action = if prd_json_exists { "Overwrote" } else { "Created" };
    println!(
        "  {GREEN}{}{RESET} {}",
        prd_json_action,
        prd_json_skill_path.display()
    );

    println!();
    if prd_exists || prd_json_exists {
        println!("Skills updated!");
    } else {
        println!("Skills installed! You can now use:");
        println!("  {CYAN}/prd{RESET}       - Create a PRD through interactive Q&A");
        println!("  {CYAN}/prd-json{RESET}  - Convert a PRD to prd.json format");
        println!();
        println!("{BOLD}Next steps:{RESET}");
        println!("  1. Start a Claude session: {CYAN}claude{RESET}");
        println!("  2. Use {CYAN}/prd{RESET} to create your PRD");
        println!("  3. Run {CYAN}autom8{RESET} to implement it");
    }

    Ok(())
}
