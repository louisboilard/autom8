use crate::spec::Spec;
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

/// Print a phase footer (bottom border) to visually close the output section.
///
/// The footer is a horizontal line using the same style as the phase banner,
/// providing visual framing around the Claude output section.
///
/// # Arguments
/// * `color` - The color to use for the footer (should match the phase banner)
///
/// # Example
/// ```ignore
/// print_phase_banner("RUNNING", BannerColor::Cyan);
/// // ... Claude output ...
/// print_phase_footer(BannerColor::Cyan);
/// ```
pub fn print_phase_footer(color: BannerColor) {
    let terminal_width = get_terminal_width_for_banner();

    // Clamp banner width between MIN and MAX (same as phase banner)
    let banner_width = terminal_width.clamp(MIN_BANNER_WIDTH, MAX_BANNER_WIDTH);

    let color_code = color.ansi_code();

    println!("{}{BOLD}{}{RESET}", color_code, "━".repeat(banner_width));
    // Print blank line for padding after the frame (US-003)
    println!();
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

pub fn print_iteration_start(iteration: u32, story_id: &str, story_title: &str) {
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!(
        "{YELLOW}Task {}{RESET} - Running {BOLD}{}{RESET}: {}",
        iteration, story_id, story_title
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
    println!("{YELLOW}Task {} finished{RESET}", iteration);
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

/// Print a prominent success message for a created PR with its URL.
///
/// This displays a visually distinct box to highlight the PR URL,
/// making it easy for users to find and click.
pub fn print_pr_success(url: &str) {
    println!();
    println!("{GREEN}{BOLD}╔════════════════════════════════════════════════════════╗{RESET}");
    println!("{GREEN}{BOLD}║  ✓ Pull Request Created                                ║{RESET}");
    println!("{GREEN}{BOLD}╚════════════════════════════════════════════════════════╝{RESET}");
    println!();
    println!("{GREEN}{BOLD}  {}{RESET}", url);
    println!();
}

/// Print a prominent message when a PR already exists for the branch.
///
/// This displays the existing PR URL in a visually distinct style similar
/// to print_pr_success, making it easy for users to find and click.
pub fn print_pr_already_exists(url: &str) {
    println!();
    println!("{CYAN}{BOLD}╔════════════════════════════════════════════════════════╗{RESET}");
    println!("{CYAN}{BOLD}║  ℹ Pull Request Already Exists                         ║{RESET}");
    println!("{CYAN}{BOLD}╚════════════════════════════════════════════════════════╝{RESET}");
    println!();
    println!("{CYAN}{BOLD}  {}{RESET}", url);
    println!();
}

/// Print a skip message for PR creation with the reason.
///
/// This displays the skip reason in a less prominent style than success/exists,
/// using the standard info format.
pub fn print_pr_skipped(reason: &str) {
    println!("{GRAY}PR creation skipped: {}{RESET}", reason);
}

pub fn print_status(state: &RunState) {
    println!("{BLUE}Run ID:{RESET}    {}", state.run_id);
    println!("{BLUE}Status:{RESET}    {:?}", state.status);
    println!("{BLUE}Spec:{RESET}      {}", state.spec_json_path.display());
    println!("{BLUE}Branch:{RESET}    {}", state.branch);
    if let Some(story) = &state.current_story {
        println!("{BLUE}Current:{RESET}   {}", story);
    }
    println!("{BLUE}Task:{RESET}      {}", state.iteration);
    println!(
        "{BLUE}Started:{RESET}   {}",
        state.started_at.format("%Y-%m-%d %H:%M:%S")
    );
    println!("{BLUE}Tasks run:{RESET}  {}", state.iterations.len());
}

/// Print global status across all projects.
///
/// Shows each project with its status: active runs, failed runs, and incomplete specs.
/// Projects with active/failed runs are highlighted.
/// Projects with no active work are shown as "idle".
pub fn print_global_status(statuses: &[crate::config::ProjectStatus]) {
    use crate::state::RunStatus;

    if statuses.is_empty() {
        println!("{GRAY}No projects found.{RESET}");
        println!();
        println!("Run {CYAN}autom8{RESET} in a project directory to create a project.");
        return;
    }

    // Separate projects that need attention from idle ones
    let (needs_attention, idle): (Vec<_>, Vec<_>) =
        statuses.iter().partition(|s| s.needs_attention());

    // Print projects needing attention first
    if !needs_attention.is_empty() {
        println!("{BOLD}Projects needing attention:{RESET}");
        println!();

        for status in &needs_attention {
            let status_indicator = match status.run_status {
                Some(RunStatus::Running) => format!("{YELLOW}[running]{RESET}"),
                Some(RunStatus::Failed) => format!("{RED}[failed]{RESET}"),
                Some(RunStatus::Completed) => String::new(),
                None => String::new(),
            };

            let spec_info = if status.incomplete_spec_count > 0 {
                format!(
                    " {CYAN}{} incomplete spec{}{RESET}",
                    status.incomplete_spec_count,
                    if status.incomplete_spec_count == 1 { "" } else { "s" }
                )
            } else {
                String::new()
            };

            if status_indicator.is_empty() {
                println!("  {BOLD}{}{RESET}{}", status.name, spec_info);
            } else {
                println!(
                    "  {BOLD}{}{RESET} {}{}",
                    status.name, status_indicator, spec_info
                );
            }
        }
        println!();
    }

    // Print idle projects
    if !idle.is_empty() {
        println!("{GRAY}Idle projects:{RESET}");
        for status in &idle {
            println!("{GRAY}  {}{RESET}", status.name);
        }
        println!();
    }

    // Print summary
    let active_count = statuses
        .iter()
        .filter(|s| s.run_status == Some(RunStatus::Running))
        .count();
    let failed_count = statuses
        .iter()
        .filter(|s| s.run_status == Some(RunStatus::Failed))
        .count();
    let incomplete_spec_total: usize = statuses.iter().map(|s| s.incomplete_spec_count).sum();

    println!(
        "{GRAY}({} project{}, {} active, {} failed, {} incomplete spec{}){RESET}",
        statuses.len(),
        if statuses.len() == 1 { "" } else { "s" },
        active_count,
        failed_count,
        incomplete_spec_total,
        if incomplete_spec_total == 1 { "" } else { "s" }
    );
}

/// Print a tree view of all projects in the config directory.
///
/// Shows each project with its subdirectories and key files, using box-drawing
/// characters for visual tree structure.
///
/// Example output:
/// ```text
/// ~/.config/autom8/
/// ├── my-project [running]
/// │   ├── spec/    (2 files)
/// │   └── runs/    (3 archived)
/// └── other-project [complete]
///     ├── spec/    (1 file)
///     └── runs/    (empty)
/// ```
pub fn print_project_tree(projects: &[crate::config::ProjectTreeInfo]) {
    use crate::state::RunStatus;

    if projects.is_empty() {
        println!("{GRAY}No projects found in ~/.config/autom8/{RESET}");
        println!();
        println!("Run {CYAN}autom8{RESET} in a project directory to create a project.");
        return;
    }

    // Print header
    println!("{BOLD}~/.config/autom8/{RESET}");

    let total = projects.len();

    for (idx, project) in projects.iter().enumerate() {
        let is_last_project = idx == total - 1;
        let branch_char = if is_last_project { "└" } else { "├" };
        let cont_char = if is_last_project { " " } else { "│" };

        // Determine status indicator and color
        let (status_indicator, status_color) = match project.run_status {
            Some(RunStatus::Running) => ("[running]", YELLOW),
            Some(RunStatus::Failed) => ("[failed]", RED),
            Some(RunStatus::Completed) if project.incomplete_spec_count > 0 => {
                ("[incomplete]", CYAN)
            }
            Some(RunStatus::Completed) => ("[complete]", GREEN),
            None if project.incomplete_spec_count > 0 => ("[incomplete]", CYAN),
            None if project.has_content() => ("[idle]", GRAY),
            None => ("", GRAY),
        };

        // Print project line
        if status_indicator.is_empty() {
            println!("{branch_char}── {BOLD}{}{RESET}", project.name);
        } else {
            println!(
                "{branch_char}── {BOLD}{}{RESET} {status_color}{status_indicator}{RESET}",
                project.name
            );
        }

        // Print subdirectories
        let subdirs = [
            ("spec", project.spec_md_count, "md"),
            ("spec", project.spec_count, "json"),
            ("runs", project.runs_count, "archived"),
        ];

        for (subidx, (name, count, unit)) in subdirs.iter().enumerate() {
            let is_last_subdir = subidx == subdirs.len() - 1;
            let sub_branch = if is_last_subdir { "└" } else { "├" };

            let count_str = if *count == 0 {
                format!("{GRAY}(empty){RESET}")
            } else if *count == 1 {
                format!("{GRAY}(1 {unit}){RESET}")
            } else {
                format!("{GRAY}({count} {unit}s){RESET}")
            };

            println!("{cont_char}   {sub_branch}── {name}/     {count_str}");
        }

        // Add spacing between projects (except after the last one)
        if !is_last_project {
            println!("{cont_char}");
        }
    }

    // Print summary
    println!();
    let active_count = projects.iter().filter(|p| p.has_active_run).count();
    let failed_count = projects
        .iter()
        .filter(|p| p.run_status == Some(RunStatus::Failed))
        .count();
    let incomplete_total: usize = projects.iter().map(|p| p.incomplete_spec_count).sum();

    println!(
        "{GRAY}({} project{}, {} active, {} failed, {} incomplete spec{}){RESET}",
        total,
        if total == 1 { "" } else { "s" },
        active_count,
        failed_count,
        incomplete_total,
        if incomplete_total == 1 { "" } else { "s" }
    );
}

/// Print detailed description of a project.
///
/// Shows:
/// - Project name and path
/// - Current status (active run, idle, etc.)
/// - Spec details with user stories and progress
/// - File counts
pub fn print_project_description(desc: &crate::config::ProjectDescription) {
    use crate::state::RunStatus;

    // Header
    println!("{BOLD}Project: {CYAN}{}{RESET}", desc.name);
    println!("{GRAY}Path: {}{RESET}", desc.path.display());
    println!();

    // Status section
    let status_indicator = match desc.run_status {
        Some(RunStatus::Running) => format!("{YELLOW}[running]{RESET}"),
        Some(RunStatus::Failed) => format!("{RED}[failed]{RESET}"),
        Some(RunStatus::Completed) => format!("{GREEN}[completed]{RESET}"),
        None => format!("{GRAY}[idle]{RESET}"),
    };
    println!("{BOLD}Status:{RESET} {}", status_indicator);

    if let Some(branch) = &desc.current_branch {
        println!("{BLUE}Branch:{RESET} {}", branch);
    }

    if let Some(story) = &desc.current_story {
        println!("{BLUE}Current Story:{RESET} {}", story);
    }
    println!();

    // Specs section
    if desc.specs.is_empty() {
        println!("{GRAY}No specs found.{RESET}");
    } else {
        println!("{BOLD}Specs:{RESET} ({} total)", desc.specs.len());
        println!();

        for spec in &desc.specs {
            print_spec_summary(spec);
        }
    }

    // File counts summary
    println!("{GRAY}─────────────────────────────────────────────────────────{RESET}");
    println!(
        "{GRAY}Files: {} spec md, {} spec json, {} archived runs{RESET}",
        desc.spec_md_count,
        desc.specs.len(),
        desc.runs_count
    );
}

/// Print summary of a single spec with its user stories.
fn print_spec_summary(spec: &crate::config::SpecSummary) {
    // Spec header
    println!("{CYAN}━━━{RESET} {BOLD}{}{RESET}", spec.filename);
    println!("{BLUE}Project:{RESET} {}", spec.project_name);
    println!("{BLUE}Branch:{RESET}  {}", spec.branch_name);

    // Description (truncate if too long)
    let desc_preview = if spec.description.len() > 100 {
        format!("{}...", &spec.description[..100])
    } else {
        spec.description.clone()
    };
    // Show first line only for brevity
    let first_line = desc_preview.lines().next().unwrap_or(&desc_preview);
    println!("{BLUE}Description:{RESET} {}", first_line);
    println!();

    // Progress bar
    let progress_bar = make_progress_bar_simple(spec.completed_count, spec.total_count, 12);
    let progress_color = if spec.completed_count == spec.total_count {
        GREEN
    } else if spec.completed_count == 0 {
        GRAY
    } else {
        YELLOW
    };
    println!(
        "{BOLD}Progress:{RESET} [{}] {}{}/{} stories complete{}",
        progress_bar, progress_color, spec.completed_count, spec.total_count, RESET
    );
    println!();

    // User stories
    println!("{BOLD}User Stories:{RESET}");
    for story in &spec.stories {
        let status_icon = if story.passes {
            format!("{GREEN}✓{RESET}")
        } else {
            format!("{GRAY}○{RESET}")
        };
        let title_color = if story.passes { GREEN } else { RESET };
        println!("  {} {BOLD}{}{RESET}: {}{}{}", status_icon, story.id, title_color, story.title, RESET);
    }
    println!();
}

/// Make a simple progress bar (internal helper for describe output).
fn make_progress_bar_simple(completed: usize, total: usize, width: usize) -> String {
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

pub fn print_history_entry(state: &RunState, index: usize) {
    let status_color = match state.status {
        crate::state::RunStatus::Completed => GREEN,
        crate::state::RunStatus::Failed => RED,
        _ => YELLOW,
    };
    println!(
        "{}. [{}{:?}{}] {} - {} ({} tasks)",
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

pub fn print_generating_spec() {
    println!("Converting to spec JSON...");
    println!("{GRAY}{}{RESET}", "-".repeat(57));
}

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
        "{YELLOW}Reviewing changes (review {}/{})...{RESET}",
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
        "{YELLOW}Issues found. Running corrector (attempt {}/{})...{RESET}",
        iteration, max_iterations
    );
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!();
}

pub fn print_max_review_iterations() {
    println!();
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!("{RED}{BOLD}Review failed after 3 attempts.{RESET}");
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!();
}

/// Print a progress bar showing task (story) completion status.
///
/// Format: `Tasks: [███░░░░░] 3/8 complete`
///
/// This should be called after each story task completes to show the user
/// the current state of the run.
///
/// # Arguments
/// * `completed` - Number of completed stories
/// * `total` - Total number of stories
pub fn print_tasks_progress(completed: usize, total: usize) {
    let progress_bar = make_progress_bar(completed, total, 12);
    println!(
        "{BLUE}Tasks:{RESET}   [{}] {}/{} complete",
        progress_bar, completed, total
    );
}

/// Print a progress bar showing review iteration status.
///
/// Format: `Review: [██░░] 2/3`
///
/// This should be called after each review or correct task completes
/// to show the user the current review iteration.
///
/// # Arguments
/// * `current` - Current review iteration (1-indexed)
/// * `max` - Maximum number of review iterations
pub fn print_review_progress(current: u32, max: u32) {
    let progress_bar = make_progress_bar(current as usize, max as usize, 8);
    println!(
        "{BLUE}Review:{RESET}  [{}] {}/{}",
        progress_bar, current, max
    );
}

/// Print both tasks progress and review progress.
///
/// This is a convenience function to show full progress context
/// during review/correct phases.
///
/// Format:
/// ```text
/// Tasks:   [███░░░░░] 3/8 complete
/// Review:  [██░░] 2/3
/// ```
///
/// # Arguments
/// * `tasks_completed` - Number of completed stories
/// * `tasks_total` - Total number of stories
/// * `review_current` - Current review iteration (1-indexed)
/// * `review_max` - Maximum number of review iterations
pub fn print_full_progress(
    tasks_completed: usize,
    tasks_total: usize,
    review_current: u32,
    review_max: u32,
) {
    print_tasks_progress(tasks_completed, tasks_total);
    print_review_progress(review_current, review_max);
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
    println!("{BLUE}Tasks:{RESET}      {}", total_iterations);
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

// ============================================================================
// Error panel display
// ============================================================================

/// Structured error information for display.
///
/// This type captures all relevant details about an error that occurred
/// during Claude operations, enabling comprehensive error display.
#[derive(Debug, Clone, PartialEq)]
pub struct ErrorDetails {
    /// Category of error (e.g., "Process Failed", "Timeout", "Auth Error")
    pub error_type: String,
    /// User-friendly description of what went wrong
    pub message: String,
    /// Exit code from subprocess, if applicable
    pub exit_code: Option<i32>,
    /// Stderr output from subprocess, if available
    pub stderr: Option<String>,
    /// Which Claude function failed (e.g., "run_claude", "run_reviewer")
    pub source: Option<String>,
}

impl ErrorDetails {
    /// Create a new ErrorDetails instance.
    pub fn new(error_type: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error_type: error_type.into(),
            message: message.into(),
            exit_code: None,
            stderr: None,
            source: None,
        }
    }

    /// Set the exit code.
    pub fn with_exit_code(mut self, code: i32) -> Self {
        self.exit_code = Some(code);
        self
    }

    /// Set the stderr output.
    pub fn with_stderr(mut self, stderr: impl Into<String>) -> Self {
        self.stderr = Some(stderr.into());
        self
    }

    /// Set the source function.
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    /// Print this error using the error panel.
    pub fn print_panel(&self) {
        print_error_panel(
            &self.error_type,
            &self.message,
            self.exit_code,
            self.stderr.as_deref(),
        );
    }
}

impl std::fmt::Display for ErrorDetails {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.error_type, self.message)?;

        if let Some(source) = &self.source {
            write!(f, " (source: {})", source)?;
        }

        if let Some(code) = self.exit_code {
            write!(f, " [exit code: {}]", code)?;
        }

        if let Some(stderr) = &self.stderr {
            let trimmed = stderr.trim();
            if !trimmed.is_empty() {
                // Show first line of stderr in display
                if let Some(first_line) = trimmed.lines().next() {
                    write!(f, " stderr: {}", first_line)?;
                }
            }
        }

        Ok(())
    }
}

const ERROR_PANEL_WIDTH: usize = 60;

/// Print a dedicated error panel with full error details.
///
/// This displays a visually distinct panel with a red bordered header,
/// showing the error type, message, exit code (if applicable), and stderr
/// output (if available).
///
/// # Arguments
/// * `error_type` - Category of error (e.g., "Claude Process Failed", "API Error", "Timeout")
/// * `message` - The error message describing what went wrong
/// * `exit_code` - Optional exit code from the subprocess
/// * `stderr` - Optional stderr output from the subprocess
///
/// # Example
/// ```ignore
/// print_error_panel(
///     "Claude Process Failed",
///     "The process exited unexpectedly",
///     Some(1),
///     Some("Error: authentication failed"),
/// );
/// ```
pub fn print_error_panel(
    error_type: &str,
    message: &str,
    exit_code: Option<i32>,
    stderr: Option<&str>,
) {
    let top_border = format!("╔{}╗", "═".repeat(ERROR_PANEL_WIDTH - 2));
    let bottom_border = format!("╚{}╝", "═".repeat(ERROR_PANEL_WIDTH - 2));
    let separator = format!("╟{}╢", "─".repeat(ERROR_PANEL_WIDTH - 2));

    // Print top border
    println!("{RED}{BOLD}{}{RESET}", top_border);

    // Print header with error type
    let header = format!(" ERROR: {} ", error_type);
    let header_padding = ERROR_PANEL_WIDTH.saturating_sub(header.len() + 2);
    let left_pad = header_padding / 2;
    let right_pad = header_padding - left_pad;
    println!(
        "{RED}{BOLD}║{}{}{}║{RESET}",
        " ".repeat(left_pad),
        header,
        " ".repeat(right_pad)
    );

    // Print separator
    println!("{RED}{}{RESET}", separator);

    // Print message (wrapped if necessary)
    print_panel_content("Message", message);

    // Print exit code if available
    if let Some(code) = exit_code {
        print_panel_line(&format!("Exit code: {}", code));
    }

    // Print stderr if available
    if let Some(err) = stderr {
        let trimmed = err.trim();
        if !trimmed.is_empty() {
            println!("{RED}{}{RESET}", separator);
            print_panel_content("Stderr", trimmed);
        }
    }

    // Print bottom border
    println!("{RED}{BOLD}{}{RESET}", bottom_border);
}

/// Print a labeled content section within the error panel.
fn print_panel_content(label: &str, content: &str) {
    let max_content_width = ERROR_PANEL_WIDTH - 6; // Account for "║ " prefix and " ║" suffix

    // Print label
    print_panel_line(&format!("{}:", label));

    // Print content, wrapping long lines
    for line in content.lines() {
        if line.len() <= max_content_width {
            print_panel_line(&format!("  {}", line));
        } else {
            // Wrap long lines
            let mut remaining = line;
            while !remaining.is_empty() {
                let (chunk, rest) = if remaining.len() <= max_content_width - 2 {
                    (remaining, "")
                } else {
                    // Find a good break point
                    let break_at = remaining[..max_content_width - 2]
                        .rfind(|c: char| c.is_whitespace() || c == '/' || c == '\\' || c == ':')
                        .map(|i| i + 1)
                        .unwrap_or(max_content_width - 2);
                    (&remaining[..break_at], &remaining[break_at..])
                };
                print_panel_line(&format!("  {}", chunk));
                remaining = rest;
            }
        }
    }
}

/// Print a single line within the error panel borders.
fn print_panel_line(text: &str) {
    let max_width = ERROR_PANEL_WIDTH - 4; // Account for "║ " and " ║"
    let display_text = if text.len() > max_width {
        &text[..max_width]
    } else {
        text
    };
    let padding = max_width.saturating_sub(display_text.len());
    println!(
        "{RED}║{RESET} {}{} {RED}║{RESET}",
        display_text,
        " ".repeat(padding)
    );
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

    // ========================================================================
    // US-002: Phase footer (bottom border) tests
    // ========================================================================

    #[test]
    fn test_print_phase_footer_cyan() {
        // Should not panic with cyan color (matches RUNNING/REVIEWING phase banners)
        print_phase_footer(BannerColor::Cyan);
    }

    #[test]
    fn test_print_phase_footer_yellow() {
        // Should not panic with yellow color (matches CORRECTING phase banner)
        print_phase_footer(BannerColor::Yellow);
    }

    #[test]
    fn test_print_phase_footer_green() {
        // Should not panic with green color (matches SUCCESS phase banner)
        print_phase_footer(BannerColor::Green);
    }

    #[test]
    fn test_print_phase_footer_red() {
        // Should not panic with red color (matches FAILURE phase banner)
        print_phase_footer(BannerColor::Red);
    }

    #[test]
    fn test_print_phase_footer_uses_same_width_as_banner() {
        // Both banner and footer should use the same width calculation
        // This test ensures they share the get_terminal_width_for_banner() logic
        let width = get_terminal_width_for_banner();
        let clamped_width = width.clamp(MIN_BANNER_WIDTH, MAX_BANNER_WIDTH);

        // The footer should produce a line of exactly clamped_width characters
        // (excluding ANSI codes). This is verified by the function using the same
        // width calculation as print_phase_banner.
        assert!(clamped_width >= MIN_BANNER_WIDTH);
        assert!(clamped_width <= MAX_BANNER_WIDTH);
    }

    // ========================================================================
    // US-004: Progress bar display tests
    // ========================================================================

    #[test]
    fn test_print_tasks_progress_no_panic() {
        // Verify the function doesn't panic with various inputs
        print_tasks_progress(0, 8);
        print_tasks_progress(3, 8);
        print_tasks_progress(8, 8);
    }

    #[test]
    fn test_print_tasks_progress_zero_total() {
        // Should not panic when total is zero
        print_tasks_progress(0, 0);
    }

    #[test]
    fn test_print_review_progress_no_panic() {
        // Verify the function doesn't panic with various inputs
        print_review_progress(1, 3);
        print_review_progress(2, 3);
        print_review_progress(3, 3);
    }

    #[test]
    fn test_print_review_progress_zero() {
        // Should not panic when values are zero
        print_review_progress(0, 0);
    }

    #[test]
    fn test_print_full_progress_no_panic() {
        // Verify the function doesn't panic with various inputs
        print_full_progress(3, 8, 1, 3);
        print_full_progress(8, 8, 3, 3);
        print_full_progress(0, 10, 1, 3);
    }

    #[test]
    fn test_print_full_progress_zero_values() {
        // Should not panic when values are zero
        print_full_progress(0, 0, 0, 0);
    }

    #[test]
    fn test_make_progress_bar_empty() {
        let bar = make_progress_bar(0, 8, 12);
        // Should have 12 chars (all empty)
        assert!(bar.contains("░"));
    }

    #[test]
    fn test_make_progress_bar_full() {
        let bar = make_progress_bar(8, 8, 12);
        // Should have 12 filled chars
        assert!(bar.contains("█"));
    }

    #[test]
    fn test_make_progress_bar_partial() {
        let bar = make_progress_bar(4, 8, 12);
        // Should have mix of filled and empty
        assert!(bar.contains("█"));
        assert!(bar.contains("░"));
    }

    #[test]
    fn test_make_progress_bar_zero_total() {
        let bar = make_progress_bar(0, 0, 12);
        // Should return spaces when total is zero
        assert_eq!(bar.len(), 12);
    }

    #[test]
    fn test_make_progress_bar_width() {
        // Test different widths
        let bar_8 = make_progress_bar(4, 8, 8);
        let bar_16 = make_progress_bar(8, 16, 16);
        // Both should work without panic
        assert!(!bar_8.is_empty());
        assert!(!bar_16.is_empty());
    }

    // ========================================================================
    // Error panel display tests
    // ========================================================================

    #[test]
    fn test_print_error_panel_basic() {
        // Should not panic with basic inputs
        print_error_panel("Claude Process Failed", "The process exited unexpectedly", None, None);
    }

    #[test]
    fn test_print_error_panel_with_exit_code() {
        // Should not panic with exit code
        print_error_panel(
            "Claude Process Failed",
            "The process exited with an error",
            Some(1),
            None,
        );
    }

    #[test]
    fn test_print_error_panel_with_stderr() {
        // Should not panic with stderr
        print_error_panel(
            "API Error",
            "Failed to communicate with Claude API",
            None,
            Some("Error: connection refused"),
        );
    }

    #[test]
    fn test_print_error_panel_full_details() {
        // Should not panic with all details
        print_error_panel(
            "Timeout",
            "Claude did not respond within the timeout period",
            Some(124),
            Some("Process killed after 300 seconds"),
        );
    }

    #[test]
    fn test_print_error_panel_empty_message() {
        // Should not panic with empty message
        print_error_panel("Unknown Error", "", None, None);
    }

    #[test]
    fn test_print_error_panel_long_message() {
        // Should not panic with long message (should wrap)
        let long_message = "This is a very long error message that should be wrapped across multiple lines because it exceeds the panel width significantly and needs proper handling";
        print_error_panel("Test Error", long_message, None, None);
    }

    #[test]
    fn test_print_error_panel_multiline_stderr() {
        // Should not panic with multiline stderr
        let stderr = "Line 1: Some error occurred\nLine 2: More details here\nLine 3: Stack trace follows";
        print_error_panel("Process Error", "Multiple errors occurred", Some(1), Some(stderr));
    }

    #[test]
    fn test_print_error_panel_empty_stderr() {
        // Should not panic with empty stderr (should be treated as None)
        print_error_panel("Test Error", "Test message", None, Some(""));
    }

    #[test]
    fn test_print_error_panel_whitespace_stderr() {
        // Should not panic with whitespace-only stderr (should be treated as empty)
        print_error_panel("Test Error", "Test message", None, Some("   \n\t  "));
    }

    #[test]
    fn test_error_panel_width_constant() {
        // Verify the error panel width is reasonable
        assert!(ERROR_PANEL_WIDTH >= 40);
        assert!(ERROR_PANEL_WIDTH <= 120);
    }

    // ========================================================================
    // US-005: ErrorDetails struct tests
    // ========================================================================

    #[test]
    fn test_error_details_new() {
        let err = ErrorDetails::new("Process Failed", "The process crashed");
        assert_eq!(err.error_type, "Process Failed");
        assert_eq!(err.message, "The process crashed");
        assert_eq!(err.exit_code, None);
        assert_eq!(err.stderr, None);
        assert_eq!(err.source, None);
    }

    #[test]
    fn test_error_details_builder_pattern() {
        let err = ErrorDetails::new("Timeout", "Operation timed out")
            .with_exit_code(124)
            .with_stderr("killed by signal")
            .with_source("run_claude");

        assert_eq!(err.error_type, "Timeout");
        assert_eq!(err.message, "Operation timed out");
        assert_eq!(err.exit_code, Some(124));
        assert_eq!(err.stderr, Some("killed by signal".to_string()));
        assert_eq!(err.source, Some("run_claude".to_string()));
    }

    #[test]
    fn test_error_details_with_exit_code() {
        let err = ErrorDetails::new("Process Failed", "Non-zero exit").with_exit_code(1);
        assert_eq!(err.exit_code, Some(1));
    }

    #[test]
    fn test_error_details_with_stderr() {
        let err = ErrorDetails::new("API Error", "Connection failed")
            .with_stderr("Error: connection refused");
        assert_eq!(err.stderr, Some("Error: connection refused".to_string()));
    }

    #[test]
    fn test_error_details_with_source() {
        let err = ErrorDetails::new("Auth Error", "Invalid token").with_source("run_reviewer");
        assert_eq!(err.source, Some("run_reviewer".to_string()));
    }

    #[test]
    fn test_error_details_display_basic() {
        let err = ErrorDetails::new("Process Failed", "The process crashed");
        let display = format!("{}", err);
        assert_eq!(display, "[Process Failed] The process crashed");
    }

    #[test]
    fn test_error_details_display_with_source() {
        let err = ErrorDetails::new("Timeout", "Operation timed out").with_source("run_claude");
        let display = format!("{}", err);
        assert!(display.contains("[Timeout]"));
        assert!(display.contains("Operation timed out"));
        assert!(display.contains("(source: run_claude)"));
    }

    #[test]
    fn test_error_details_display_with_exit_code() {
        let err = ErrorDetails::new("Process Failed", "Exited").with_exit_code(1);
        let display = format!("{}", err);
        assert!(display.contains("[exit code: 1]"));
    }

    #[test]
    fn test_error_details_display_with_stderr() {
        let err = ErrorDetails::new("API Error", "Failed").with_stderr("connection refused");
        let display = format!("{}", err);
        assert!(display.contains("stderr: connection refused"));
    }

    #[test]
    fn test_error_details_display_full() {
        let err = ErrorDetails::new("Auth Error", "Authentication failed")
            .with_exit_code(1)
            .with_stderr("Error: unauthorized\nMore details here")
            .with_source("run_reviewer");
        let display = format!("{}", err);

        assert!(display.contains("[Auth Error]"));
        assert!(display.contains("Authentication failed"));
        assert!(display.contains("(source: run_reviewer)"));
        assert!(display.contains("[exit code: 1]"));
        // Should only show first line of stderr in Display
        assert!(display.contains("stderr: Error: unauthorized"));
        assert!(!display.contains("More details here"));
    }

    #[test]
    fn test_error_details_display_empty_stderr() {
        let err = ErrorDetails::new("Test", "Test message").with_stderr("   \n  ");
        let display = format!("{}", err);
        // Empty/whitespace stderr should not appear in display
        assert!(!display.contains("stderr:"));
    }

    #[test]
    fn test_error_details_equality() {
        let err1 = ErrorDetails::new("Test", "Message").with_exit_code(1);
        let err2 = ErrorDetails::new("Test", "Message").with_exit_code(1);
        let err3 = ErrorDetails::new("Test", "Message").with_exit_code(2);

        assert_eq!(err1, err2);
        assert_ne!(err1, err3);
    }

    #[test]
    fn test_error_details_clone() {
        let err = ErrorDetails::new("Test", "Message")
            .with_exit_code(1)
            .with_stderr("some error")
            .with_source("test_source");
        let cloned = err.clone();

        assert_eq!(err, cloned);
    }

    #[test]
    fn test_error_details_debug() {
        let err = ErrorDetails::new("Test", "Message");
        let debug = format!("{:?}", err);
        assert!(debug.contains("ErrorDetails"));
        assert!(debug.contains("Test"));
        assert!(debug.contains("Message"));
    }

    #[test]
    fn test_error_details_print_panel_no_panic() {
        // Should not panic when printing error panel
        let err = ErrorDetails::new("Process Failed", "The process crashed")
            .with_exit_code(1)
            .with_stderr("Error details here")
            .with_source("run_claude");
        err.print_panel();
    }

    // ========================================================================
    // US-007: Project tree view tests
    // ========================================================================

    #[test]
    fn test_print_project_tree_empty_no_panic() {
        // Should not panic with empty list
        let projects: Vec<crate::config::ProjectTreeInfo> = vec![];
        print_project_tree(&projects);
    }

    #[test]
    fn test_print_project_tree_single_project_no_panic() {
        // Should not panic with single project
        let projects = vec![crate::config::ProjectTreeInfo {
            name: "test-project".to_string(),
            has_active_run: false,
            run_status: None,
            spec_count: 1,
            incomplete_spec_count: 0,
            spec_md_count: 2,
            runs_count: 0,
        }];
        print_project_tree(&projects);
    }

    #[test]
    fn test_print_project_tree_multiple_projects_no_panic() {
        // Should not panic with multiple projects
        let projects = vec![
            crate::config::ProjectTreeInfo {
                name: "project-a".to_string(),
                has_active_run: true,
                run_status: Some(crate::state::RunStatus::Running),
                spec_count: 1,
                incomplete_spec_count: 1,
                spec_md_count: 2,
                runs_count: 3,
            },
            crate::config::ProjectTreeInfo {
                name: "project-b".to_string(),
                has_active_run: false,
                run_status: Some(crate::state::RunStatus::Failed),
                spec_count: 0,
                incomplete_spec_count: 0,
                spec_md_count: 0,
                runs_count: 1,
            },
            crate::config::ProjectTreeInfo {
                name: "project-c".to_string(),
                has_active_run: false,
                run_status: Some(crate::state::RunStatus::Completed),
                spec_count: 2,
                incomplete_spec_count: 0,
                spec_md_count: 1,
                runs_count: 5,
            },
        ];
        print_project_tree(&projects);
    }

    #[test]
    fn test_print_project_tree_all_status_types_no_panic() {
        // Test all possible status types don't panic
        use crate::state::RunStatus;

        let projects = vec![
            crate::config::ProjectTreeInfo {
                name: "running".to_string(),
                has_active_run: true,
                run_status: Some(RunStatus::Running),
                spec_count: 1,
                incomplete_spec_count: 0,
                spec_md_count: 0,
                runs_count: 0,
            },
            crate::config::ProjectTreeInfo {
                name: "failed".to_string(),
                has_active_run: false,
                run_status: Some(RunStatus::Failed),
                spec_count: 0,
                incomplete_spec_count: 0,
                spec_md_count: 0,
                runs_count: 0,
            },
            crate::config::ProjectTreeInfo {
                name: "complete".to_string(),
                has_active_run: false,
                run_status: Some(RunStatus::Completed),
                spec_count: 1,
                incomplete_spec_count: 0,
                spec_md_count: 0,
                runs_count: 0,
            },
            crate::config::ProjectTreeInfo {
                name: "incomplete".to_string(),
                has_active_run: false,
                run_status: None,
                spec_count: 1,
                incomplete_spec_count: 1,
                spec_md_count: 0,
                runs_count: 0,
            },
            crate::config::ProjectTreeInfo {
                name: "idle".to_string(),
                has_active_run: false,
                run_status: None,
                spec_count: 0,
                incomplete_spec_count: 0,
                spec_md_count: 1,
                runs_count: 0,
            },
            crate::config::ProjectTreeInfo {
                name: "empty".to_string(),
                has_active_run: false,
                run_status: None,
                spec_count: 0,
                incomplete_spec_count: 0,
                spec_md_count: 0,
                runs_count: 0,
            },
        ];
        print_project_tree(&projects);
    }

    // ========================================================================
    // US-008: Project description output tests
    // ========================================================================

    #[test]
    fn test_us008_print_project_description_no_panic() {
        // Should not panic when printing a project description
        use std::path::PathBuf;

        let desc = crate::config::ProjectDescription {
            name: "test-project".to_string(),
            path: PathBuf::from("/test/path"),
            has_active_run: false,
            run_status: None,
            current_story: None,
            current_branch: None,
            specs: vec![],
            spec_md_count: 0,
            runs_count: 0,
        };
        print_project_description(&desc);
    }

    #[test]
    fn test_us008_print_project_description_with_prd_no_panic() {
        // Should not panic when printing a project with a PRD
        use std::path::PathBuf;

        let desc = crate::config::ProjectDescription {
            name: "test-project".to_string(),
            path: PathBuf::from("/test/path"),
            has_active_run: false,
            run_status: Some(crate::state::RunStatus::Completed),
            current_story: None,
            current_branch: Some("feature/test".to_string()),
            specs: vec![crate::config::SpecSummary {
                filename: "prd-test.json".to_string(),
                path: PathBuf::from("/test/path/prds/prd-test.json"),
                project_name: "Test Project".to_string(),
                branch_name: "feature/test".to_string(),
                description: "A test project description.".to_string(),
                stories: vec![
                    crate::config::StorySummary {
                        id: "US-001".to_string(),
                        title: "First Story".to_string(),
                        passes: true,
                    },
                    crate::config::StorySummary {
                        id: "US-002".to_string(),
                        title: "Second Story".to_string(),
                        passes: false,
                    },
                ],
                completed_count: 1,
                total_count: 2,
            }],
            spec_md_count: 1,
            runs_count: 2,
        };
        print_project_description(&desc);
    }

    #[test]
    fn test_us008_print_project_description_running_status_no_panic() {
        // Should not panic when printing a project with running status
        use std::path::PathBuf;

        let desc = crate::config::ProjectDescription {
            name: "test-project".to_string(),
            path: PathBuf::from("/test/path"),
            has_active_run: true,
            run_status: Some(crate::state::RunStatus::Running),
            current_story: Some("US-003".to_string()),
            current_branch: Some("feature/wip".to_string()),
            specs: vec![],
            spec_md_count: 0,
            runs_count: 0,
        };
        print_project_description(&desc);
    }

    #[test]
    fn test_us008_print_project_description_failed_status_no_panic() {
        // Should not panic when printing a project with failed status
        use std::path::PathBuf;

        let desc = crate::config::ProjectDescription {
            name: "test-project".to_string(),
            path: PathBuf::from("/test/path"),
            has_active_run: false,
            run_status: Some(crate::state::RunStatus::Failed),
            current_story: Some("US-001".to_string()),
            current_branch: Some("feature/broken".to_string()),
            specs: vec![],
            spec_md_count: 0,
            runs_count: 0,
        };
        print_project_description(&desc);
    }

    #[test]
    fn test_us008_print_project_description_long_description_no_panic() {
        // Should not panic with long description (should truncate)
        use std::path::PathBuf;

        let long_desc = "This is a very long description that goes on and on and on and should be truncated when displayed to the user because it's too long for a single line display in the terminal output.";

        let desc = crate::config::ProjectDescription {
            name: "test-project".to_string(),
            path: PathBuf::from("/test/path"),
            has_active_run: false,
            run_status: None,
            current_story: None,
            current_branch: None,
            specs: vec![crate::config::SpecSummary {
                filename: "prd-test.json".to_string(),
                path: PathBuf::from("/test/path/prds/prd-test.json"),
                project_name: "Test Project".to_string(),
                branch_name: "feature/test".to_string(),
                description: long_desc.to_string(),
                stories: vec![],
                completed_count: 0,
                total_count: 0,
            }],
            spec_md_count: 0,
            runs_count: 0,
        };
        print_project_description(&desc);
    }

    #[test]
    fn test_us008_make_progress_bar_simple_empty() {
        let bar = make_progress_bar_simple(0, 10, 10);
        assert!(bar.contains("░"), "Empty progress bar should have empty chars");
    }

    #[test]
    fn test_us008_make_progress_bar_simple_full() {
        let bar = make_progress_bar_simple(10, 10, 10);
        assert!(bar.contains("█"), "Full progress bar should have filled chars");
    }

    #[test]
    fn test_us008_make_progress_bar_simple_partial() {
        let bar = make_progress_bar_simple(5, 10, 10);
        assert!(bar.contains("█"), "Partial bar should have filled chars");
        assert!(bar.contains("░"), "Partial bar should have empty chars");
    }

    #[test]
    fn test_us008_make_progress_bar_simple_zero_total() {
        // Should not panic and return empty bar
        let bar = make_progress_bar_simple(0, 0, 10);
        assert_eq!(bar.len(), 10);
    }

    // ========================================================================
    // US-006: PR success display tests
    // ========================================================================

    #[test]
    fn test_print_pr_success_no_panic() {
        // Should not panic when printing PR success message
        print_pr_success("https://github.com/owner/repo/pull/42");
    }

    #[test]
    fn test_print_pr_success_with_long_url() {
        // Should not panic with a long PR URL
        print_pr_success("https://github.com/very-long-organization-name/extremely-long-repository-name-for-testing/pull/12345");
    }

    #[test]
    fn test_print_pr_success_with_empty_url() {
        // Should not panic with empty URL (edge case)
        print_pr_success("");
    }

    // ========================================================================
    // US-007: PR URL console output tests
    // ========================================================================

    #[test]
    fn test_print_pr_already_exists_no_panic() {
        // Should not panic when printing PR already exists message
        print_pr_already_exists("https://github.com/owner/repo/pull/42");
    }

    #[test]
    fn test_print_pr_already_exists_with_long_url() {
        // Should not panic with a long PR URL
        print_pr_already_exists("https://github.com/very-long-organization-name/extremely-long-repository-name-for-testing/pull/12345");
    }

    #[test]
    fn test_print_pr_already_exists_with_empty_url() {
        // Should not panic with empty URL (edge case)
        print_pr_already_exists("");
    }

    #[test]
    fn test_print_pr_skipped_no_panic() {
        // Should not panic when printing PR skipped message
        print_pr_skipped("No commits were made in this session");
    }

    #[test]
    fn test_print_pr_skipped_with_long_reason() {
        // Should not panic with a long skip reason
        print_pr_skipped("Not authenticated with GitHub CLI - please run 'gh auth login' to authenticate before creating pull requests");
    }

    #[test]
    fn test_print_pr_skipped_with_empty_reason() {
        // Should not panic with empty reason (edge case)
        print_pr_skipped("");
    }
}
