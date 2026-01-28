use crate::progress::Breadcrumb;
use crate::spec::Spec;
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

/// Print a prominent message when a PR description has been updated.
///
/// This displays the PR URL in a visually distinct style similar to
/// print_pr_success, making it easy for users to find and click.
pub fn print_pr_updated(url: &str) {
    println!();
    println!("{GREEN}{BOLD}╔════════════════════════════════════════════════════════╗{RESET}");
    println!("{GREEN}{BOLD}║  ✓ Pull Request Updated                                ║{RESET}");
    println!("{GREEN}{BOLD}╚════════════════════════════════════════════════════════╝{RESET}");
    println!();
    println!("{GREEN}{BOLD}  {}{RESET}", url);
    println!();
}

/// Print a status message when pushing branch to remote.
///
/// This displays a simple status line indicating the push operation is in progress.
pub fn print_pushing_branch(branch: &str) {
    println!("{CYAN}Pushing branch '{}'...{RESET}", branch);
}

/// Print a success message when branch push completes.
pub fn print_push_success() {
    println!("{GREEN}Branch pushed successfully.{RESET}");
}

/// Print a message when branch is already up-to-date on remote.
pub fn print_push_already_up_to_date() {
    println!("{GRAY}Branch already up-to-date on remote.{RESET}");
}

// ============================================================================
// PR Detection Output (US-001: Detect PR from Current Branch)
// ============================================================================

/// Print a message when no open PRs exist in the repository.
pub fn print_no_open_prs() {
    println!();
    println!("{YELLOW}No open pull requests found in this repository.{RESET}");
    println!();
    println!("{GRAY}Create a PR first with 'gh pr create' or push a branch with changes.{RESET}");
}

/// Print a message when a PR was detected for the current branch.
pub fn print_pr_detected(pr_number: u32, title: &str, branch: &str) {
    println!();
    println!(
        "{GREEN}Detected PR #{}{RESET} for branch {CYAN}{}{RESET}",
        pr_number, branch
    );
    println!("{BLUE}Title:{RESET} {}", title);
    println!();
}

/// Print a message when switching to a different branch.
pub fn print_switching_branch(from_branch: &str, to_branch: &str) {
    println!(
        "{CYAN}Switching{RESET} from {GRAY}{}{RESET} to {CYAN}{}{RESET}...",
        from_branch, to_branch
    );
}

/// Print a success message when branch switch completes.
pub fn print_branch_switched(branch: &str) {
    println!("{GREEN}Now on branch:{RESET} {}", branch);
    println!();
}

/// Format a PR for display in a selection list.
///
/// Returns a formatted string like: "#123 feature/add-auth (Add authentication)"
pub fn format_pr_for_selection(number: u32, branch: &str, title: &str) -> String {
    // Truncate title if too long
    let max_title_len = 50;
    let display_title = if title.len() > max_title_len {
        format!("{}...", &title[..max_title_len - 3])
    } else {
        title.to_string()
    };

    format!("#{} {} ({})", number, branch, display_title)
}

// ============================================================================
// US-002: PR Context Display (Description, Comments, Reviews)
// ============================================================================

/// Print a message when no unresolved comments are found on a PR.
pub fn print_no_unresolved_comments(pr_number: u32, title: &str) {
    println!();
    println!(
        "{GREEN}PR #{}{RESET} has no unresolved comments.",
        pr_number
    );
    println!("{BLUE}Title:{RESET} {}", title);
    println!();
    println!("{GRAY}Nothing to review - all feedback has been addressed!{RESET}");
}

/// Print a summary of the PR context being analyzed.
pub fn print_pr_context_summary(pr_number: u32, title: &str, comment_count: usize) {
    println!();
    println!("{CYAN}Analyzing PR #{}{RESET}: {}", pr_number, title);
    println!(
        "{BLUE}Found:{RESET} {} unresolved comment{}",
        comment_count,
        if comment_count == 1 { "" } else { "s" }
    );
    println!();
}

/// Print a single PR comment with its context.
///
/// Displays the comment author, location (file/line if inline), and content.
pub fn print_pr_comment(
    index: usize,
    author: &str,
    body: &str,
    file_path: Option<&str>,
    line: Option<u32>,
) {
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!(
        "{YELLOW}Comment {}{RESET} by {CYAN}{}{RESET}",
        index + 1,
        author
    );

    // Show location context for inline comments
    if let Some(path) = file_path {
        if let Some(line_num) = line {
            println!("{BLUE}Location:{RESET} {}:{}", path, line_num);
        } else {
            println!("{BLUE}Location:{RESET} {}", path);
        }
    }

    println!();
    // Print the comment body, preserving formatting
    for line in body.lines() {
        println!("  {}", line);
    }
    println!();
}

/// Print the list of all unresolved comments for a PR.
pub fn print_pr_comments_list(comments: &[crate::gh::PRComment]) {
    println!("{BOLD}Unresolved Comments:{RESET}");
    println!();

    for (i, comment) in comments.iter().enumerate() {
        print_pr_comment(
            i,
            &comment.author,
            &comment.body,
            comment.file_path.as_deref(),
            comment.line,
        );
    }

    println!("{GRAY}{}{RESET}", "-".repeat(57));
}

/// Print an error message for PR context gathering failures.
pub fn print_pr_context_error(message: &str) {
    println!();
    println!("{RED}{BOLD}Failed to gather PR context:{RESET}");
    println!("{RED}  {}{RESET}", message);
    println!();
}

// ============================================================================
// US-003: Branch Context Display (Spec Warning, Commits)
// ============================================================================

const WARNING_PANEL_WIDTH: usize = 60;

/// Print a prominent warning panel for missing spec file.
///
/// This displays a visually distinct yellow warning box that makes it clear
/// the user is operating with reduced context. This is different from a
/// simple warning line - it's designed to be highly visible.
///
/// # Arguments
/// * `branch_name` - The branch name that was searched for
/// * `spec_path` - The path where the spec was expected
pub fn print_missing_spec_warning(branch_name: &str, spec_path: &str) {
    let top_border = format!("╔{}╗", "═".repeat(WARNING_PANEL_WIDTH - 2));
    let bottom_border = format!("╚{}╝", "═".repeat(WARNING_PANEL_WIDTH - 2));
    let separator = format!("╟{}╢", "─".repeat(WARNING_PANEL_WIDTH - 2));

    println!();
    println!("{YELLOW}{BOLD}{}{RESET}", top_border);

    // Print header
    let header = " ⚠  NO SPEC FILE FOUND ";
    let header_padding = WARNING_PANEL_WIDTH.saturating_sub(header.len() + 2);
    let left_pad = header_padding / 2;
    let right_pad = header_padding - left_pad;
    println!(
        "{YELLOW}{BOLD}║{}{}{}║{RESET}",
        " ".repeat(left_pad),
        header,
        " ".repeat(right_pad)
    );

    println!("{YELLOW}{}{RESET}", separator);

    // Print warning message
    print_warning_panel_line("The PR review will proceed with reduced context.");
    print_warning_panel_line("");
    print_warning_panel_line(&format!("Branch: {}", branch_name));

    // Truncate spec path if too long
    let max_path_len = WARNING_PANEL_WIDTH - 12; // "Expected: " prefix
    let display_path = if spec_path.len() > max_path_len {
        format!("...{}", &spec_path[spec_path.len() - max_path_len + 3..])
    } else {
        spec_path.to_string()
    };
    print_warning_panel_line(&format!("Expected: {}", display_path));

    println!("{YELLOW}{}{RESET}", separator);

    // Print suggestion
    print_warning_panel_line("Create a spec file to provide full context:");
    print_warning_panel_line("  autom8 --spec <spec.md>");

    println!("{YELLOW}{BOLD}{}{RESET}", bottom_border);
    println!();
}

/// Print a single line within the warning panel borders.
fn print_warning_panel_line(text: &str) {
    let max_width = WARNING_PANEL_WIDTH - 4; // Account for "║ " and " ║"
    let display_text = if text.len() > max_width {
        &text[..max_width]
    } else {
        text
    };
    let padding = max_width.saturating_sub(display_text.len());
    println!(
        "{YELLOW}║{RESET} {}{} {YELLOW}║{RESET}",
        display_text,
        " ".repeat(padding)
    );
}

/// Print a summary of the branch context being used.
pub fn print_branch_context_summary(has_spec: bool, commit_count: usize, branch_name: &str) {
    println!();
    println!("{CYAN}Branch Context:{RESET} {}", branch_name);

    if has_spec {
        println!("{GREEN}  ✓ Spec file loaded{RESET}");
    } else {
        println!("{YELLOW}  ⚠ No spec file (reduced context){RESET}");
    }

    println!(
        "{BLUE}  {} commit{} on branch{RESET}",
        commit_count,
        if commit_count == 1 { "" } else { "s" }
    );
    println!();
}

/// Print a list of commits for display.
pub fn print_commit_list(commits: &[crate::git::CommitInfo], max_display: usize) {
    if commits.is_empty() {
        println!("{GRAY}No commits found on this branch.{RESET}");
        return;
    }

    let display_count = commits.len().min(max_display);
    println!("{BOLD}Recent Commits:{RESET}");

    for commit in commits.iter().take(display_count) {
        // Truncate message if too long
        let max_msg_len = 50;
        let display_msg = if commit.message.len() > max_msg_len {
            format!("{}...", &commit.message[..max_msg_len - 3])
        } else {
            commit.message.clone()
        };

        println!("  {CYAN}{}{RESET} {}", commit.short_hash, display_msg);
    }

    if commits.len() > max_display {
        println!(
            "{GRAY}  ... and {} more commit{}{RESET}",
            commits.len() - max_display,
            if commits.len() - max_display == 1 {
                ""
            } else {
                "s"
            }
        );
    }
    println!();
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
                    if status.incomplete_spec_count == 1 {
                        ""
                    } else {
                        "s"
                    }
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
        println!(
            "  {} {BOLD}{}{RESET}: {}{}{}",
            status_icon, story.id, title_color, story.title, RESET
        );
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

#[derive(Debug, Clone)]
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

// ============================================================================
// PR Review Display Functions (US-005)
// ============================================================================

/// Print a header when starting PR review analysis.
pub fn print_pr_review_start(pr_number: u32, title: &str, comment_count: usize) {
    println!();
    println!("{CYAN}{BOLD}╔════════════════════════════════════════════════════════╗{RESET}");
    println!("{CYAN}{BOLD}║  PR Review Analysis                                    ║{RESET}");
    println!("{CYAN}{BOLD}╚════════════════════════════════════════════════════════╝{RESET}");
    println!();
    println!("{BLUE}PR #{}{RESET}: {}", pr_number, title);
    println!("{BLUE}Comments to analyze:{RESET} {}", comment_count);
    println!();
}

/// Print status when spawning the Claude agent for PR review.
pub fn print_pr_review_spawning() {
    println!("{GRAY}Spawning Claude agent for PR review...{RESET}");
    println!();
}

/// Print the PR review summary results.
///
/// Displays a formatted summary box showing:
/// - Total comments analyzed
/// - Real issues fixed
/// - Red herrings identified
/// - Legitimate suggestions (no action taken)
pub fn print_pr_review_summary(summary: &crate::claude::PRReviewSummary) {
    println!();
    println!("{GRAY}{}{RESET}", "─".repeat(57));
    println!();
    println!("{BOLD}PR Review Summary{RESET}");
    println!();

    // Use colored indicators for each category
    println!(
        "  {BLUE}Total comments analyzed:{RESET}    {}",
        summary.total_comments
    );
    println!(
        "  {GREEN}Real issues fixed:{RESET}         {}",
        summary.real_issues_fixed
    );
    println!(
        "  {YELLOW}Red herrings identified:{RESET}   {}",
        summary.red_herrings
    );
    println!(
        "  {GRAY}Legitimate suggestions:{RESET}    {}",
        summary.legitimate_suggestions
    );
    println!();
}

/// Print a success message when PR review completes with fixes made.
pub fn print_pr_review_complete_with_fixes(fixes_count: usize) {
    println!();
    println!("{GREEN}{BOLD}╔════════════════════════════════════════════════════════╗{RESET}");
    println!("{GREEN}{BOLD}║  ✓ PR Review Complete                                  ║{RESET}");
    println!("{GREEN}{BOLD}╚════════════════════════════════════════════════════════╝{RESET}");
    println!();
    println!(
        "{GREEN}Fixed {} issue{}.{RESET}",
        fixes_count,
        if fixes_count == 1 { "" } else { "s" }
    );
    println!();
}

/// Print a message when PR review completes but no fixes were needed.
pub fn print_pr_review_no_fixes_needed() {
    println!();
    println!("{CYAN}{BOLD}╔════════════════════════════════════════════════════════╗{RESET}");
    println!("{CYAN}{BOLD}║  ✓ PR Review Complete - No Fixes Needed                ║{RESET}");
    println!("{CYAN}{BOLD}╚════════════════════════════════════════════════════════╝{RESET}");
    println!();
    println!("{GRAY}All comments were either red herrings or suggestions.{RESET}");
    println!("{GRAY}No code changes were required.{RESET}");
    println!();
}

/// Print an error message when PR review fails.
pub fn print_pr_review_error(message: &str) {
    println!();
    println!("{RED}{BOLD}╔════════════════════════════════════════════════════════╗{RESET}");
    println!("{RED}{BOLD}║  ✗ PR Review Failed                                    ║{RESET}");
    println!("{RED}{BOLD}╚════════════════════════════════════════════════════════╝{RESET}");
    println!();
    println!("{RED}Error:{RESET} {}", message);
    println!();
}

/// Print a message when starting to stream Claude output for PR review.
pub fn print_pr_review_streaming() {
    println!("{GRAY}{}{RESET}", "─".repeat(57));
    println!("{CYAN}Claude Analysis:{RESET}");
    println!("{GRAY}{}{RESET}", "─".repeat(57));
    println!();
}

/// Print a footer after streaming Claude output for PR review.
pub fn print_pr_review_streaming_done() {
    println!();
    println!("{GRAY}{}{RESET}", "─".repeat(57));
}

// ============================================================================
// US-006: Commit and Push Status Display for PR Review
// ============================================================================

/// Print a message when commit is skipped due to config.
pub fn print_pr_commit_skipped_config() {
    println!("{GRAY}Commit skipped (commit disabled in config){RESET}");
}

/// Print a message when push is skipped due to config.
pub fn print_pr_push_skipped_config() {
    println!("{GRAY}Push skipped (push disabled in config){RESET}");
}

/// Print a message when no fixes were made so no commit is needed.
pub fn print_pr_no_commit_no_fixes() {
    println!("{GRAY}No commit created (no fixes were made){RESET}");
}

/// Print a success message when PR review commit is created.
pub fn print_pr_commit_success(commit_hash: &str) {
    println!(
        "{GREEN}Created commit {}{RESET} with PR review fixes",
        commit_hash
    );
}

/// Print an error message when PR review commit fails.
pub fn print_pr_commit_error(message: &str) {
    println!("{RED}Failed to create commit:{RESET} {}", message);
}

/// Print a success message when PR review push succeeds.
pub fn print_pr_push_success(branch: &str) {
    println!("{GREEN}Pushed{RESET} fixes to {CYAN}{}{RESET}", branch);
}

/// Print an error message when PR review push fails.
pub fn print_pr_push_error(message: &str) {
    println!("{RED}Failed to push:{RESET} {}", message);
}

/// Print a message when push reports already up-to-date.
pub fn print_pr_push_up_to_date() {
    println!("{GRAY}Branch already up-to-date on remote{RESET}");
}

/// Print a summary of what was done based on config.
///
/// This provides a clear overview of which actions were performed or skipped.
pub fn print_pr_review_actions_summary(
    commit_enabled: bool,
    push_enabled: bool,
    commit_made: bool,
    push_made: bool,
    no_fixes_needed: bool,
) {
    println!();
    println!("{BOLD}Actions:{RESET}");

    if no_fixes_needed {
        println!("  {GRAY}• No fixes needed - no commit created{RESET}");
        return;
    }

    // Commit status
    if !commit_enabled {
        println!("  {GRAY}• Commit: disabled in config{RESET}");
    } else if commit_made {
        println!("  {GREEN}• Commit: created{RESET}");
    } else {
        println!("  {GRAY}• Commit: no changes to commit{RESET}");
    }

    // Push status
    if !push_enabled {
        println!("  {GRAY}• Push: disabled in config{RESET}");
    } else if !commit_made {
        println!("  {GRAY}• Push: skipped (no commit){RESET}");
    } else if push_made {
        println!("  {GREEN}• Push: completed{RESET}");
    } else {
        println!("  {GRAY}• Push: already up-to-date{RESET}");
    }

    println!();
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

    #[test]
    fn test_print_phase_banner_all_colors_and_phases() {
        // Test all standard phase/color combinations don't panic
        let test_cases: &[(&str, BannerColor)] = &[
            ("RUNNING", BannerColor::Cyan),
            ("REVIEWING", BannerColor::Cyan),
            ("CORRECTING", BannerColor::Yellow),
            ("COMMITTING", BannerColor::Cyan),
            ("SUCCESS", BannerColor::Green),
            ("FAILURE", BannerColor::Red),
        ];

        for (phase_name, color) in test_cases {
            print_phase_banner(phase_name, *color);
        }
    }

    #[test]
    fn test_print_phase_banner_edge_cases() {
        // Empty name should not panic
        print_phase_banner("", BannerColor::Cyan);

        // Very long name should not panic
        print_phase_banner(
            "THIS_IS_A_VERY_LONG_PHASE_NAME_THAT_EXCEEDS_NORMAL_LENGTH",
            BannerColor::Cyan,
        );
    }

    // ========================================================================
    // US-002: Phase footer (bottom border) tests
    // ========================================================================

    #[test]
    fn test_print_phase_footer_all_colors() {
        // Test all banner colors work with footer
        for color in &[
            BannerColor::Cyan,
            BannerColor::Yellow,
            BannerColor::Green,
            BannerColor::Red,
        ] {
            print_phase_footer(*color);
        }
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
    fn test_print_progress_functions_various_inputs() {
        // Test progress display functions with various inputs including edge cases
        let task_cases: &[(usize, usize)] = &[(0, 8), (3, 8), (8, 8), (0, 0)];
        for (completed, total) in task_cases {
            print_tasks_progress(*completed, *total);
        }

        let review_cases: &[(u32, u32)] = &[(1, 3), (2, 3), (3, 3), (0, 0)];
        for (current, max) in review_cases {
            print_review_progress(*current, *max);
        }

        // Full progress combines both
        print_full_progress(3, 8, 1, 3);
        print_full_progress(8, 8, 3, 3);
        print_full_progress(0, 10, 1, 3);
        print_full_progress(0, 0, 0, 0);
    }

    #[test]
    fn test_make_progress_bar_states() {
        // Empty bar
        let empty = make_progress_bar(0, 8, 12);
        assert!(empty.contains("░"));

        // Full bar
        let full = make_progress_bar(8, 8, 12);
        assert!(full.contains("█"));

        // Partial bar
        let partial = make_progress_bar(4, 8, 12);
        assert!(partial.contains("█") && partial.contains("░"));

        // Zero total returns spaces
        let zero = make_progress_bar(0, 0, 12);
        assert_eq!(zero.len(), 12);

        // Different widths work
        assert!(!make_progress_bar(4, 8, 8).is_empty());
        assert!(!make_progress_bar(8, 16, 16).is_empty());
    }

    // ========================================================================
    // Error panel display tests
    // ========================================================================

    #[test]
    fn test_print_error_panel_various_inputs() {
        // Test error panel with various combinations of inputs
        let test_cases: &[(&str, &str, Option<i32>, Option<&str>)] = &[
            (
                "Claude Process Failed",
                "The process exited unexpectedly",
                None,
                None,
            ),
            (
                "Claude Process Failed",
                "The process exited with an error",
                Some(1),
                None,
            ),
            (
                "API Error",
                "Failed to communicate with Claude API",
                None,
                Some("Error: connection refused"),
            ),
            (
                "Timeout",
                "Claude did not respond within the timeout period",
                Some(124),
                Some("Process killed after 300 seconds"),
            ),
            ("Unknown Error", "", None, None), // empty message
            ("Test Error", "Test message", None, Some("")), // empty stderr
            ("Test Error", "Test message", None, Some("   \n\t  ")), // whitespace stderr
        ];

        for (error_type, message, exit_code, stderr) in test_cases {
            print_error_panel(error_type, message, *exit_code, *stderr);
        }

        // Long message that wraps
        let long_message = "This is a very long error message that should be wrapped across multiple lines because it exceeds the panel width significantly and needs proper handling";
        print_error_panel("Test Error", long_message, None, None);

        // Multiline stderr
        let stderr =
            "Line 1: Some error occurred\nLine 2: More details here\nLine 3: Stack trace follows";
        print_error_panel(
            "Process Error",
            "Multiple errors occurred",
            Some(1),
            Some(stderr),
        );
    }

    #[test]
    fn test_error_panel_width_constant() {
        assert!(ERROR_PANEL_WIDTH >= 40 && ERROR_PANEL_WIDTH <= 120);
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
    fn test_print_project_tree_various_inputs() {
        use crate::state::RunStatus;

        // Empty list
        print_project_tree(&[]);

        // Single project
        print_project_tree(&[crate::config::ProjectTreeInfo {
            name: "test-project".to_string(),
            has_active_run: false,
            run_status: None,
            spec_count: 1,
            incomplete_spec_count: 0,
            spec_md_count: 2,
            runs_count: 0,
            last_run_date: None,
        }]);

        // Multiple projects with all status types
        let projects = vec![
            crate::config::ProjectTreeInfo {
                name: "running".to_string(),
                has_active_run: true,
                run_status: Some(RunStatus::Running),
                spec_count: 1,
                incomplete_spec_count: 1,
                spec_md_count: 2,
                runs_count: 3,
                last_run_date: None,
            },
            crate::config::ProjectTreeInfo {
                name: "failed".to_string(),
                has_active_run: false,
                run_status: Some(RunStatus::Failed),
                spec_count: 0,
                incomplete_spec_count: 0,
                spec_md_count: 0,
                runs_count: 1,
                last_run_date: None,
            },
            crate::config::ProjectTreeInfo {
                name: "complete".to_string(),
                has_active_run: false,
                run_status: Some(RunStatus::Completed),
                spec_count: 2,
                incomplete_spec_count: 0,
                spec_md_count: 1,
                runs_count: 5,
                last_run_date: None,
            },
            crate::config::ProjectTreeInfo {
                name: "incomplete".to_string(),
                has_active_run: false,
                run_status: None,
                spec_count: 1,
                incomplete_spec_count: 1,
                spec_md_count: 0,
                runs_count: 0,
                last_run_date: None,
            },
            crate::config::ProjectTreeInfo {
                name: "idle".to_string(),
                has_active_run: false,
                run_status: None,
                spec_count: 0,
                incomplete_spec_count: 0,
                spec_md_count: 1,
                runs_count: 0,
                last_run_date: None,
            },
            crate::config::ProjectTreeInfo {
                name: "empty".to_string(),
                has_active_run: false,
                run_status: None,
                spec_count: 0,
                incomplete_spec_count: 0,
                spec_md_count: 0,
                runs_count: 0,
                last_run_date: None,
            },
        ];
        print_project_tree(&projects);
    }

    // ========================================================================
    // US-008: Project description output tests
    // ========================================================================

    #[test]
    fn test_print_project_description_various_states() {
        use std::path::PathBuf;

        // Empty project
        print_project_description(&crate::config::ProjectDescription {
            name: "test-project".to_string(),
            path: PathBuf::from("/test/path"),
            has_active_run: false,
            run_status: None,
            current_story: None,
            current_branch: None,
            specs: vec![],
            spec_md_count: 0,
            runs_count: 0,
        });

        // Project with PRD and stories (completed status)
        print_project_description(&crate::config::ProjectDescription {
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
        });

        // Running status
        print_project_description(&crate::config::ProjectDescription {
            name: "test-project".to_string(),
            path: PathBuf::from("/test/path"),
            has_active_run: true,
            run_status: Some(crate::state::RunStatus::Running),
            current_story: Some("US-003".to_string()),
            current_branch: Some("feature/wip".to_string()),
            specs: vec![],
            spec_md_count: 0,
            runs_count: 0,
        });

        // Failed status
        print_project_description(&crate::config::ProjectDescription {
            name: "test-project".to_string(),
            path: PathBuf::from("/test/path"),
            has_active_run: false,
            run_status: Some(crate::state::RunStatus::Failed),
            current_story: Some("US-001".to_string()),
            current_branch: Some("feature/broken".to_string()),
            specs: vec![],
            spec_md_count: 0,
            runs_count: 0,
        });

        // Long description (truncation test)
        let long_desc = "This is a very long description that goes on and on and on and should be truncated when displayed to the user because it's too long for a single line display in the terminal output.";
        print_project_description(&crate::config::ProjectDescription {
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
        });
    }

    #[test]
    fn test_make_progress_bar_simple_states() {
        // Empty, full, partial, and zero total
        assert!(make_progress_bar_simple(0, 10, 10).contains("░"));
        assert!(make_progress_bar_simple(10, 10, 10).contains("█"));
        let partial = make_progress_bar_simple(5, 10, 10);
        assert!(partial.contains("█") && partial.contains("░"));
        assert_eq!(make_progress_bar_simple(0, 0, 10).len(), 10);
    }

    // ========================================================================
    // PR and push output tests
    // ========================================================================

    #[test]
    fn test_pr_output_functions() {
        // PR success with various URLs
        for url in &["https://github.com/owner/repo/pull/42", "https://github.com/very-long-organization-name/extremely-long-repository-name-for-testing/pull/12345", ""] {
            print_pr_success(url);
            print_pr_already_exists(url);
        }

        // PR skipped with various reasons
        for reason in &["No commits were made in this session", "Not authenticated with GitHub CLI - please run 'gh auth login' to authenticate before creating pull requests", ""] {
            print_pr_skipped(reason);
        }
    }

    #[test]
    fn test_push_output_functions() {
        print_pushing_branch("feature/test");
        print_pushing_branch("feature/very-long-branch-name-that-describes-the-feature-in-detail");
        print_push_success();
        print_push_already_up_to_date();
    }

    // ========================================================================
    // US-001: PR Detection output tests
    // ========================================================================

    #[test]
    fn test_print_no_open_prs_does_not_panic() {
        print_no_open_prs();
    }

    #[test]
    fn test_print_pr_detected_various_inputs() {
        // Normal case
        print_pr_detected(42, "Add feature X", "feature/x");

        // Long title
        print_pr_detected(
            123,
            "This is a very long PR title that describes the changes in detail",
            "feature/long-branch-name",
        );

        // Minimal case
        print_pr_detected(1, "", "");
    }

    #[test]
    fn test_print_switching_branch_various_inputs() {
        print_switching_branch("main", "feature/test");
        print_switching_branch("feature/old", "feature/new");
        print_switching_branch("", "feature/target");
    }

    #[test]
    fn test_print_branch_switched_various_inputs() {
        print_branch_switched("feature/test");
        print_branch_switched("a-very-long-branch-name-for-testing");
        print_branch_switched("");
    }

    #[test]
    fn test_format_pr_for_selection_normal() {
        let formatted = format_pr_for_selection(42, "feature/auth", "Add authentication");
        assert!(formatted.contains("#42"));
        assert!(formatted.contains("feature/auth"));
        assert!(formatted.contains("Add authentication"));
    }

    #[test]
    fn test_format_pr_for_selection_long_title_truncation() {
        let long_title = "This is a very long PR title that definitely exceeds the maximum allowed length for display in the selection list";
        let formatted = format_pr_for_selection(99, "feature/long", long_title);

        assert!(formatted.contains("#99"));
        assert!(formatted.contains("feature/long"));
        // Title should be truncated with "..."
        assert!(formatted.contains("..."));
        // Should not contain the full title
        assert!(!formatted.contains("selection list"));
    }

    #[test]
    fn test_format_pr_for_selection_short_title() {
        let short_title = "Fix bug";
        let formatted = format_pr_for_selection(1, "fix/bug", short_title);

        assert!(formatted.contains("#1"));
        assert!(formatted.contains("fix/bug"));
        assert!(formatted.contains("Fix bug"));
        // Short title should not have ellipsis
        assert!(!formatted.contains("..."));
    }

    #[test]
    fn test_format_pr_for_selection_empty_title() {
        let formatted = format_pr_for_selection(5, "branch", "");
        assert!(formatted.contains("#5"));
        assert!(formatted.contains("branch"));
    }

    #[test]
    fn test_format_pr_for_selection_exactly_max_length() {
        // Title exactly at max length (50 chars) should not be truncated
        let title = "a".repeat(50);
        let formatted = format_pr_for_selection(10, "test", &title);

        assert!(formatted.contains("#10"));
        assert!(formatted.contains("test"));
        assert!(!formatted.contains("..."));
    }

    // ========================================================================
    // US-002: PR Context Display Tests
    // ========================================================================

    #[test]
    fn test_print_no_unresolved_comments_does_not_panic() {
        print_no_unresolved_comments(42, "Test PR");
        print_no_unresolved_comments(1, "");
        print_no_unresolved_comments(
            99999,
            "A very long PR title that might be too long to display properly",
        );
    }

    #[test]
    fn test_print_pr_context_summary_does_not_panic() {
        print_pr_context_summary(1, "Small PR", 1);
        print_pr_context_summary(42, "Test PR", 5);
        print_pr_context_summary(100, "Large PR", 100);
        print_pr_context_summary(1, "", 0);
    }

    #[test]
    fn test_print_pr_comment_inline_does_not_panic() {
        // Inline comment with file and line
        print_pr_comment(
            0,
            "reviewer",
            "This code needs improvement",
            Some("src/main.rs"),
            Some(42),
        );
    }

    #[test]
    fn test_print_pr_comment_file_without_line_does_not_panic() {
        // Comment with file but no line (file-level comment)
        print_pr_comment(
            1,
            "reviewer",
            "General file feedback",
            Some("src/lib.rs"),
            None,
        );
    }

    #[test]
    fn test_print_pr_comment_conversation_does_not_panic() {
        // Conversation comment (no file/line context)
        print_pr_comment(2, "commenter", "This is great work overall!", None, None);
    }

    #[test]
    fn test_print_pr_comment_multiline_body_does_not_panic() {
        let multiline_body = "First line\nSecond line\nThird line\n\nAfter blank line";
        print_pr_comment(3, "reviewer", multiline_body, Some("file.rs"), Some(10));
    }

    #[test]
    fn test_print_pr_comments_list_does_not_panic() {
        use crate::gh::PRComment;

        let comments = vec![
            PRComment {
                author: "user1".to_string(),
                body: "First comment".to_string(),
                file_path: Some("src/main.rs".to_string()),
                line: Some(10),
                id: 1,
                url: "url1".to_string(),
            },
            PRComment {
                author: "user2".to_string(),
                body: "Second comment".to_string(),
                file_path: None,
                line: None,
                id: 2,
                url: "url2".to_string(),
            },
        ];

        print_pr_comments_list(&comments);
    }

    #[test]
    fn test_print_pr_comments_list_empty_does_not_panic() {
        use crate::gh::PRComment;
        let empty: Vec<PRComment> = vec![];
        print_pr_comments_list(&empty);
    }

    #[test]
    fn test_print_pr_context_error_does_not_panic() {
        print_pr_context_error("Something went wrong");
        print_pr_context_error("");
        print_pr_context_error(
            "A very long error message that explains in detail what happened and why it failed",
        );
    }

    // ========================================================================
    // US-003: Branch Context Display Tests
    // ========================================================================

    #[test]
    fn test_print_missing_spec_warning_does_not_panic() {
        print_missing_spec_warning("feature/test", "/path/to/spec.json");
        print_missing_spec_warning(
            "feature/pr-review",
            "~/.config/autom8/project/spec/spec-feature-pr-review.json",
        );
        print_missing_spec_warning("", "");
    }

    #[test]
    fn test_print_missing_spec_warning_long_path_truncation() {
        // Very long path should be truncated
        let long_path = "/very/long/path/to/config/directory/that/exceeds/normal/length/spec-feature-branch.json";
        print_missing_spec_warning("feature/branch", long_path);
    }

    #[test]
    fn test_print_branch_context_summary_does_not_panic() {
        // With spec
        print_branch_context_summary(true, 5, "feature/test");
        // Without spec
        print_branch_context_summary(false, 10, "feature/other");
        // No commits
        print_branch_context_summary(true, 0, "feature/empty");
        // Single commit
        print_branch_context_summary(false, 1, "fix/bug");
    }

    #[test]
    fn test_print_commit_list_does_not_panic() {
        use crate::git::CommitInfo;

        // Empty list
        let empty: Vec<CommitInfo> = vec![];
        print_commit_list(&empty, 5);

        // Single commit
        let single = vec![CommitInfo {
            short_hash: "abc1234".to_string(),
            full_hash: "abc1234567890".to_string(),
            message: "Initial commit".to_string(),
            author: "Test Author".to_string(),
            date: "2024-01-15".to_string(),
        }];
        print_commit_list(&single, 5);

        // Multiple commits under max
        let multiple = vec![
            CommitInfo {
                short_hash: "abc1234".to_string(),
                full_hash: "abc1234567890".to_string(),
                message: "First commit".to_string(),
                author: "Author 1".to_string(),
                date: "2024-01-15".to_string(),
            },
            CommitInfo {
                short_hash: "def5678".to_string(),
                full_hash: "def5678901234".to_string(),
                message: "Second commit".to_string(),
                author: "Author 2".to_string(),
                date: "2024-01-16".to_string(),
            },
        ];
        print_commit_list(&multiple, 5);

        // More commits than max (should show "... and X more")
        let many = (0..10)
            .map(|i| CommitInfo {
                short_hash: format!("hash{:03}", i),
                full_hash: format!("fullhash{:03}", i),
                message: format!("Commit number {}", i),
                author: "Author".to_string(),
                date: "2024-01-15".to_string(),
            })
            .collect::<Vec<_>>();
        print_commit_list(&many, 3);
    }

    #[test]
    fn test_print_commit_list_long_message_truncation() {
        use crate::git::CommitInfo;

        let long_message_commit = vec![CommitInfo {
            short_hash: "abc1234".to_string(),
            full_hash: "abc1234567890".to_string(),
            message: "This is a very long commit message that should be truncated when displayed to fit within the terminal width constraints".to_string(),
            author: "Test Author".to_string(),
            date: "2024-01-15".to_string(),
        }];
        print_commit_list(&long_message_commit, 5);
    }

    #[test]
    fn test_warning_panel_width_constant() {
        assert!(WARNING_PANEL_WIDTH >= 40 && WARNING_PANEL_WIDTH <= 100);
    }

    // ========================================================================
    // US-005: PR Review Display Tests
    // ========================================================================

    #[test]
    fn test_print_pr_review_start_does_not_panic() {
        print_pr_review_start(123, "Test PR Title", 5);
        print_pr_review_start(1, "Short", 0);
        print_pr_review_start(
            999999,
            "Very Long PR Title That Should Still Display Correctly",
            100,
        );
    }

    #[test]
    fn test_print_pr_review_spawning_does_not_panic() {
        print_pr_review_spawning();
    }

    #[test]
    fn test_print_pr_review_summary_does_not_panic() {
        use crate::claude::PRReviewSummary;

        // Zero values
        let summary_zero = PRReviewSummary {
            total_comments: 0,
            real_issues_fixed: 0,
            red_herrings: 0,
            legitimate_suggestions: 0,
        };
        print_pr_review_summary(&summary_zero);

        // Normal values
        let summary_normal = PRReviewSummary {
            total_comments: 10,
            real_issues_fixed: 3,
            red_herrings: 5,
            legitimate_suggestions: 2,
        };
        print_pr_review_summary(&summary_normal);

        // Large values
        let summary_large = PRReviewSummary {
            total_comments: 1000,
            real_issues_fixed: 500,
            red_herrings: 300,
            legitimate_suggestions: 200,
        };
        print_pr_review_summary(&summary_large);
    }

    #[test]
    fn test_print_pr_review_complete_with_fixes_does_not_panic() {
        print_pr_review_complete_with_fixes(0);
        print_pr_review_complete_with_fixes(1);
        print_pr_review_complete_with_fixes(5);
        print_pr_review_complete_with_fixes(100);
    }

    #[test]
    fn test_print_pr_review_no_fixes_needed_does_not_panic() {
        print_pr_review_no_fixes_needed();
    }

    #[test]
    fn test_print_pr_review_error_does_not_panic() {
        print_pr_review_error("Simple error message");
        print_pr_review_error("A much longer error message that contains a detailed description of what went wrong during the PR review process");
        print_pr_review_error("");
    }

    #[test]
    fn test_print_pr_review_streaming_does_not_panic() {
        print_pr_review_streaming();
    }

    #[test]
    fn test_print_pr_review_streaming_done_does_not_panic() {
        print_pr_review_streaming_done();
    }

    #[test]
    fn test_pr_review_complete_with_fixes_plural_handling() {
        // Test singular/plural handling in the output
        // These just verify no panics - we can't easily test the actual output
        print_pr_review_complete_with_fixes(1); // "1 issue" (singular)
        print_pr_review_complete_with_fixes(2); // "2 issues" (plural)
    }

    // ========================================================================
    // US-006: Commit and Push Status Display Tests
    // ========================================================================

    #[test]
    fn test_print_pr_commit_skipped_config_does_not_panic() {
        print_pr_commit_skipped_config();
    }

    #[test]
    fn test_print_pr_push_skipped_config_does_not_panic() {
        print_pr_push_skipped_config();
    }

    #[test]
    fn test_print_pr_no_commit_no_fixes_does_not_panic() {
        print_pr_no_commit_no_fixes();
    }

    #[test]
    fn test_print_pr_commit_success_does_not_panic() {
        print_pr_commit_success("abc1234");
        print_pr_commit_success("");
        print_pr_commit_success("very-long-commit-hash-for-testing");
    }

    #[test]
    fn test_print_pr_commit_error_does_not_panic() {
        print_pr_commit_error("failed to commit");
        print_pr_commit_error("");
        print_pr_commit_error("a very long error message that explains in detail what went wrong");
    }

    #[test]
    fn test_print_pr_push_success_does_not_panic() {
        print_pr_push_success("feature/test");
        print_pr_push_success("main");
        print_pr_push_success("feature/very-long-branch-name-for-testing");
    }

    #[test]
    fn test_print_pr_push_error_does_not_panic() {
        print_pr_push_error("permission denied");
        print_pr_push_error("");
        print_pr_push_error("a very long error message about push failure");
    }

    #[test]
    fn test_print_pr_push_up_to_date_does_not_panic() {
        print_pr_push_up_to_date();
    }

    #[test]
    fn test_print_pr_review_actions_summary_all_cases() {
        // No fixes needed
        print_pr_review_actions_summary(true, true, false, false, true);

        // Commit disabled
        print_pr_review_actions_summary(false, true, false, false, false);

        // Push disabled
        print_pr_review_actions_summary(true, false, true, false, false);

        // Both disabled
        print_pr_review_actions_summary(false, false, false, false, false);

        // Commit made but push disabled
        print_pr_review_actions_summary(true, false, true, false, false);

        // Commit made and push made
        print_pr_review_actions_summary(true, true, true, true, false);

        // Commit made but push already up-to-date
        print_pr_review_actions_summary(true, true, true, false, false);

        // Commit enabled but nothing to commit
        print_pr_review_actions_summary(true, true, false, false, false);
    }
}
