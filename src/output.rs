use crate::prd::Prd;
use crate::state::{MachineState, RunState};

// ANSI color codes
pub const RESET: &str = "\x1b[0m";
pub const BOLD: &str = "\x1b[1m";
pub const DIM: &str = "\x1b[2m";
pub const GREEN: &str = "\x1b[32m";
pub const YELLOW: &str = "\x1b[33m";
pub const BLUE: &str = "\x1b[34m";
pub const CYAN: &str = "\x1b[36m";
pub const RED: &str = "\x1b[31m";
pub const GRAY: &str = "\x1b[90m";

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

pub fn print_project_info(prd: &Prd) {
    let completed = prd.completed_count();
    let total = prd.total_count();
    let progress_bar = make_progress_bar(completed, total, 12);

    println!("{BLUE}Project:{RESET} {}", prd.project);
    println!("{BLUE}Branch:{RESET}  {}", prd.branch_name);
    println!(
        "{BLUE}Stories:{RESET} [{}] {}/{} complete",
        progress_bar, completed, total
    );
    println!();
}

pub fn print_iteration_start(
    iteration: u32,
    max_iterations: u32,
    story_id: &str,
    story_title: &str,
) {
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!(
        "{YELLOW}Iteration {}/{}{RESET} - Running {BOLD}{}{RESET}: {}",
        iteration, max_iterations, story_id, story_title
    );
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!();
}

pub fn print_claude_output(line: &str) {
    println!("{GRAY}{}{RESET}", line);
}

pub fn print_story_complete(story_id: &str, duration_secs: u64) {
    let mins = duration_secs / 60;
    let secs = duration_secs % 60;
    println!();
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!(
        "{GREEN}{BOLD}{} completed{RESET} in {}m {}s",
        story_id, mins, secs
    );
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!();
}

pub fn print_iteration_complete(iteration: u32) {
    println!();
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!("{YELLOW}Iteration {} finished{RESET}", iteration);
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!();
}

pub fn print_all_complete() {
    println!();
    println!("{GREEN}{BOLD}All stories completed!{RESET}");
    println!();
}

pub fn print_error(msg: &str) {
    println!("{RED}{BOLD}Error:{RESET} {}", msg);
}

pub fn print_warning(msg: &str) {
    println!("{YELLOW}Warning:{RESET} {}", msg);
}

pub fn print_info(msg: &str) {
    println!("{CYAN}Info:{RESET} {}", msg);
}

pub fn print_status(state: &RunState) {
    println!("{BLUE}Run ID:{RESET}    {}", state.run_id);
    println!("{BLUE}Status:{RESET}    {:?}", state.status);
    println!("{BLUE}PRD:{RESET}       {}", state.prd_path.display());
    println!("{BLUE}Branch:{RESET}    {}", state.branch);
    if let Some(story) = &state.current_story {
        println!("{BLUE}Current:{RESET}   {}", story);
    }
    println!(
        "{BLUE}Iteration:{RESET} {}/{}",
        state.iteration, state.max_iterations
    );
    println!(
        "{BLUE}Started:{RESET}   {}",
        state.started_at.format("%Y-%m-%d %H:%M:%S")
    );
    println!("{BLUE}Iterations run:{RESET} {}", state.iterations.len());
}

pub fn print_history_entry(state: &RunState, index: usize) {
    let status_color = match state.status {
        crate::state::RunStatus::Completed => GREEN,
        crate::state::RunStatus::Failed => RED,
        _ => YELLOW,
    };
    println!(
        "{}. [{}{:?}{}] {} - {} ({} iterations)",
        index + 1,
        status_color,
        state.status,
        RESET,
        state.started_at.format("%Y-%m-%d %H:%M"),
        state.branch,
        state.iterations.len()
    );
}

fn make_progress_bar(completed: usize, total: usize, width: usize) -> String {
    if total == 0 {
        return " ".repeat(width);
    }
    let filled = (completed * width) / total;
    let empty = width - filled;
    format!(
        "{GREEN}{}{RESET}{GRAY}{}{RESET}",
        "█".repeat(filled),
        "░".repeat(empty)
    )
}

fn state_to_display(state: MachineState) -> &'static str {
    match state {
        MachineState::Idle => "idle",
        MachineState::LoadingSpec => "loading-spec",
        MachineState::GeneratingPrd => "generating-prd",
        MachineState::Initializing => "initializing",
        MachineState::PickingStory => "picking-story",
        MachineState::RunningClaude => "running-claude",
        MachineState::Completed => "completed",
        MachineState::Failed => "failed",
    }
}

pub fn print_state_transition(from: MachineState, to: MachineState) {
    println!(
        "{CYAN}[state]{RESET} {} -> {}",
        state_to_display(from),
        state_to_display(to)
    );
}

pub fn print_spec_loaded(path: &std::path::Path, size_bytes: u64) {
    let size_str = if size_bytes >= 1024 {
        format!("{:.1} KB", size_bytes as f64 / 1024.0)
    } else {
        format!("{} B", size_bytes)
    };
    println!("{BLUE}Spec:{RESET} {} ({})", path.display(), size_str);
}

pub fn print_generating_prd() {
    println!("Converting to prd.json...");
    println!("{GRAY}{}{RESET}", "-".repeat(57));
}

pub fn print_prd_generated(prd: &Prd, output_path: &std::path::Path) {
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!();
    println!("{GREEN}{BOLD}PRD Generated Successfully{RESET}");
    println!("{BLUE}Project:{RESET} {}", prd.project);
    println!("{BLUE}Stories:{RESET} {}", prd.total_count());
    for story in &prd.user_stories {
        println!("  - {}: {}", story.id, story.title);
    }
    println!();
    println!("{BLUE}Saved:{RESET} {}", output_path.display());
    println!();
}

pub fn print_proceeding_to_implementation() {
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!("Proceeding to implementation...");
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!();
}

pub struct StoryResult {
    pub id: String,
    pub title: String,
    pub passed: bool,
    pub duration_secs: u64,
}

pub fn print_run_summary(
    total_stories: usize,
    completed_stories: usize,
    total_iterations: u32,
    total_duration_secs: u64,
    story_results: &[StoryResult],
) {
    let hours = total_duration_secs / 3600;
    let mins = (total_duration_secs % 3600) / 60;
    let secs = total_duration_secs % 60;

    println!();
    println!("{CYAN}{BOLD}Run Summary{RESET}");
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!(
        "{BLUE}Stories:{RESET}    {}/{} completed",
        completed_stories, total_stories
    );
    println!("{BLUE}Iterations:{RESET} {}", total_iterations);
    println!(
        "{BLUE}Total time:{RESET} {:02}:{:02}:{:02}",
        hours, mins, secs
    );
    println!();

    if !story_results.is_empty() {
        println!("{BOLD}Per-story breakdown:{RESET}");
        for result in story_results {
            let status = if result.passed {
                format!("{GREEN}PASS{RESET}")
            } else {
                format!("{RED}FAIL{RESET}")
            };
            let story_mins = result.duration_secs / 60;
            let story_secs = result.duration_secs % 60;
            println!(
                "  [{}] {}: {} ({}m {}s)",
                status, result.id, result.title, story_mins, story_secs
            );
        }
        println!();
    }
    println!("{GRAY}{}{RESET}", "-".repeat(57));
}
