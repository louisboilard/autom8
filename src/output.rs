use crate::prd::Prd;
use crate::progress::Breadcrumb;
use crate::state::{MachineState, RunState};
use terminal_size::{terminal_size, Width};

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

// ============================================================================
// Phase banner display
// ============================================================================

/// Color options for phase banners
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BannerColor {
    /// Cyan - used for starting a phase
    Cyan,
    /// Green - used for successful completion
    Green,
    /// Red - used for failure
    Red,
    /// Yellow - used for correction/warning phases
    Yellow,
}

impl BannerColor {
    /// Get the ANSI color code for this banner color
    fn ansi_code(&self) -> &'static str {
        match self {
            BannerColor::Cyan => CYAN,
            BannerColor::Green => GREEN,
            BannerColor::Red => RED,
            BannerColor::Yellow => YELLOW,
        }
    }
}

const DEFAULT_TERMINAL_WIDTH: u16 = 80;
const MIN_BANNER_WIDTH: usize = 20;
const MAX_BANNER_WIDTH: usize = 80;

/// Get the current terminal width for banner display
fn get_terminal_width_for_banner() -> usize {
    terminal_size()
        .map(|(Width(w), _)| w as usize)
        .unwrap_or(DEFAULT_TERMINAL_WIDTH as usize)
}

/// Print a color-coded phase banner.
///
/// Banner format: `━━━ PHASE_NAME ━━━` with appropriate color.
/// The banner width adapts to terminal width (clamped between MIN and MAX).
///
/// # Arguments
/// * `phase_name` - The name of the phase (e.g., "RUNNING", "REVIEWING")
/// * `color` - The color to use for the banner
///
/// # Example
/// ```ignore
/// print_phase_banner("RUNNING", BannerColor::Cyan);
/// // Output: ━━━━━━━━━━━━━━━━━ RUNNING ━━━━━━━━━━━━━━━━━
/// ```
pub fn print_phase_banner(phase_name: &str, color: BannerColor) {
    let terminal_width = get_terminal_width_for_banner();

    // Clamp banner width between MIN and MAX
    let banner_width = terminal_width.clamp(MIN_BANNER_WIDTH, MAX_BANNER_WIDTH);

    // Calculate padding: " PHASE_NAME " has phase_name.len() + 2 spaces
    let phase_with_spaces = format!(" {} ", phase_name);
    let phase_len = phase_with_spaces.chars().count();

    // Calculate how many ━ characters we need on each side
    let remaining = banner_width.saturating_sub(phase_len);
    let left_padding = remaining / 2;
    let right_padding = remaining - left_padding;

    let color_code = color.ansi_code();

    println!(
        "{}{BOLD}{}{}{}{}",
        color_code,
        "━".repeat(left_padding),
        phase_with_spaces,
        "━".repeat(right_padding),
        RESET
    );
}

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
        MachineState::Reviewing => "reviewing",
        MachineState::Correcting => "correcting",
        MachineState::Committing => "committing",
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

pub fn print_reviewing(iteration: u32, max_iterations: u32) {
    println!();
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!(
        "{YELLOW}Reviewing changes (iteration {}/{})...{RESET}",
        iteration, max_iterations
    );
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!();
}

pub fn print_skip_review() {
    println!();
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!("{YELLOW}Skipping review (--skip-review flag set){RESET}");
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!();
}

pub fn print_review_passed() {
    println!();
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!("{GREEN}{BOLD}Review passed! Proceeding to commit.{RESET}");
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!();
}

pub fn print_issues_found(iteration: u32, max_iterations: u32) {
    println!();
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!(
        "{YELLOW}Issues found. Running corrector (iteration {}/{})...{RESET}",
        iteration, max_iterations
    );
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!();
}

pub fn print_max_review_iterations() {
    println!();
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!("{RED}{BOLD}Review failed after 3 iterations.{RESET}");
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!();
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

/// Print a breadcrumb trail showing the workflow journey.
///
/// This displays the trail of states the workflow has passed through,
/// showing completed states in green and the current state in yellow.
///
/// Format: `Journey: Story → Review → Correct → Review`
///
/// The trail is automatically truncated if it's too long for the terminal.
pub fn print_breadcrumb_trail(breadcrumb: &Breadcrumb) {
    breadcrumb.print();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_banner_color_ansi_codes() {
        assert_eq!(BannerColor::Cyan.ansi_code(), CYAN);
        assert_eq!(BannerColor::Green.ansi_code(), GREEN);
        assert_eq!(BannerColor::Red.ansi_code(), RED);
        assert_eq!(BannerColor::Yellow.ansi_code(), YELLOW);
    }

    #[test]
    fn test_banner_color_equality() {
        assert_eq!(BannerColor::Cyan, BannerColor::Cyan);
        assert_ne!(BannerColor::Cyan, BannerColor::Green);
    }

    #[test]
    fn test_get_terminal_width_returns_valid_width() {
        let width = get_terminal_width_for_banner();
        // Should return something reasonable, either terminal width or default
        assert!(width >= MIN_BANNER_WIDTH);
    }

    #[test]
    fn test_banner_width_clamping() {
        // Test that banner width is clamped correctly
        // Since we can't easily mock terminal width, we test the constants
        assert!(MIN_BANNER_WIDTH < MAX_BANNER_WIDTH);
        assert_eq!(MIN_BANNER_WIDTH, 20);
        assert_eq!(MAX_BANNER_WIDTH, 80);
    }

    // Test that print_phase_banner doesn't panic for various inputs
    #[test]
    fn test_print_phase_banner_running() {
        // This test verifies the function doesn't panic
        print_phase_banner("RUNNING", BannerColor::Cyan);
    }

    #[test]
    fn test_print_phase_banner_reviewing() {
        print_phase_banner("REVIEWING", BannerColor::Cyan);
    }

    #[test]
    fn test_print_phase_banner_correcting() {
        print_phase_banner("CORRECTING", BannerColor::Yellow);
    }

    #[test]
    fn test_print_phase_banner_committing() {
        print_phase_banner("COMMITTING", BannerColor::Cyan);
    }

    #[test]
    fn test_print_phase_banner_success() {
        print_phase_banner("SUCCESS", BannerColor::Green);
    }

    #[test]
    fn test_print_phase_banner_failure() {
        print_phase_banner("FAILURE", BannerColor::Red);
    }

    #[test]
    fn test_print_phase_banner_empty_name() {
        // Should not panic even with empty name
        print_phase_banner("", BannerColor::Cyan);
    }

    #[test]
    fn test_print_phase_banner_long_name() {
        // Should not panic with a very long name
        print_phase_banner("THIS_IS_A_VERY_LONG_PHASE_NAME_THAT_EXCEEDS_NORMAL_LENGTH", BannerColor::Cyan);
    }
}
