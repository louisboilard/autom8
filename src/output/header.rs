//! Header and iteration display.
//!
//! Provides session headers and iteration progress display.

use crate::spec::Spec;
use crate::state::MachineState;

use super::colors::*;
use super::progress::make_progress_bar;

/// Print the autom8 header banner.
pub fn print_header() {
    println!("{CYAN}{BOLD}");
    println!("+---------------------------------------------------------+");
    println!(
        "|  autom8 v{}                                          |",
        env!("CARGO_PKG_VERSION")
    );
    println!("+---------------------------------------------------------+");
    println!("{RESET}");
}

/// Print project information from a spec.
pub fn print_project_info(spec: &Spec) {
    let completed = spec.completed_count();
    let total = spec.total_count();
    let progress_bar = make_progress_bar(completed, total, 12);

    println!("{BLUE}Project:{RESET} {}", spec.project);
    println!("{BLUE}Branch:{RESET}  {}", spec.branch_name);
    println!(
        "{BLUE}Stories:{RESET} [{}] {}/{} complete",
        progress_bar, completed, total
    );
    println!();
}

/// Print iteration start header.
pub fn print_iteration_start(iteration: u32, story_id: &str, story_title: &str) {
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!(
        "{YELLOW}Task {}{RESET} - Running {BOLD}{}{RESET}: {}",
        iteration, story_id, story_title
    );
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!();
}

/// Print Claude output line.
pub fn print_claude_output(line: &str) {
    println!("{GRAY}{}{RESET}", line);
}

/// Print iteration complete message.
pub fn print_iteration_complete(iteration: u32) {
    println!();
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!("{YELLOW}Task {} finished{RESET}", iteration);
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!();
}

/// Print state transition for debugging.
pub fn print_state_transition(from: MachineState, to: MachineState) {
    println!(
        "{CYAN}[state]{RESET} {} -> {}",
        state_to_display(from),
        state_to_display(to)
    );
}

fn state_to_display(state: MachineState) -> &'static str {
    match state {
        MachineState::Idle => "idle",
        MachineState::LoadingSpec => "loading-spec",
        MachineState::GeneratingSpec => "generating-spec",
        MachineState::Initializing => "initializing",
        MachineState::PickingStory => "picking-story",
        MachineState::RunningClaude => "running-claude",
        MachineState::Reviewing => "reviewing",
        MachineState::Correcting => "correcting",
        MachineState::Committing => "committing",
        MachineState::CreatingPR => "creating-pr",
        MachineState::Completed => "completed",
        MachineState::Failed => "failed",
    }
}

/// Print spec loaded message.
pub fn print_spec_loaded(path: &std::path::Path, size_bytes: u64) {
    let size_str = if size_bytes >= 1024 {
        format!("{:.1} KB", size_bytes as f64 / 1024.0)
    } else {
        format!("{} B", size_bytes)
    };
    println!("{BLUE}Spec:{RESET} {} ({})", path.display(), size_str);
}

/// Print generating spec message.
pub fn print_generating_spec() {
    println!("Converting to spec JSON...");
    println!("{GRAY}{}{RESET}", "-".repeat(57));
}

/// Print spec generated success message.
pub fn print_spec_generated(spec: &Spec, output_path: &std::path::Path) {
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!();
    println!("{GREEN}{BOLD}Spec Generated Successfully{RESET}");
    println!("{BLUE}Project:{RESET} {}", spec.project);
    println!("{BLUE}Stories:{RESET} {}", spec.total_count());
    for story in &spec.user_stories {
        println!("  - {}: {}", story.id, story.title);
    }
    println!();
    println!("{BLUE}Saved:{RESET} {}", output_path.display());
    println!();
}

/// Print proceeding to implementation message.
pub fn print_proceeding_to_implementation() {
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!("Proceeding to implementation...");
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!();
}
