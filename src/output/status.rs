//! Status and project display.
//!
//! Output functions for displaying run status, project trees, and descriptions.

use crate::state::{RunState, RunStatus};

use super::colors::*;

const WARNING_PANEL_WIDTH: usize = 60;

/// Print current run status.
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
pub fn print_global_status(statuses: &[crate::config::ProjectStatus]) {
    if statuses.is_empty() {
        println!("{GRAY}No projects found.{RESET}");
        println!();
        println!("Run {CYAN}autom8{RESET} in a project directory to create a project.");
        return;
    }

    let (needs_attention, idle): (Vec<_>, Vec<_>) =
        statuses.iter().partition(|s| s.needs_attention());

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

    if !idle.is_empty() {
        println!("{GRAY}Idle projects:{RESET}");
        for status in &idle {
            println!("{GRAY}  {}{RESET}", status.name);
        }
        println!();
    }

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
pub fn print_project_tree(projects: &[crate::config::ProjectTreeInfo]) {
    if projects.is_empty() {
        println!("{GRAY}No projects found in ~/.config/autom8/{RESET}");
        println!();
        println!("Run {CYAN}autom8{RESET} in a project directory to create a project.");
        return;
    }

    println!("{BOLD}~/.config/autom8/{RESET}");

    let total = projects.len();

    for (idx, project) in projects.iter().enumerate() {
        let is_last_project = idx == total - 1;
        let branch_char = if is_last_project { "└" } else { "├" };
        let cont_char = if is_last_project { " " } else { "│" };

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

        if status_indicator.is_empty() {
            println!("{branch_char}── {BOLD}{}{RESET}", project.name);
        } else {
            println!(
                "{branch_char}── {BOLD}{}{RESET} {status_color}{status_indicator}{RESET}",
                project.name
            );
        }

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

        if !is_last_project {
            println!("{cont_char}");
        }
    }

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
pub fn print_project_description(desc: &crate::config::ProjectDescription) {
    println!("{BOLD}Project: {CYAN}{}{RESET}", desc.name);
    println!("{GRAY}Path: {}{RESET}", desc.path.display());
    println!();

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

    if desc.specs.is_empty() {
        println!("{GRAY}No specs found.{RESET}");
    } else {
        println!("{BOLD}Specs:{RESET} ({} total)", desc.specs.len());
        println!();

        for spec in &desc.specs {
            print_spec_summary(spec);
        }
    }

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
    println!("{CYAN}━━━{RESET} {BOLD}{}{RESET}", spec.filename);
    println!("{BLUE}Project:{RESET} {}", spec.project_name);
    println!("{BLUE}Branch:{RESET}  {}", spec.branch_name);

    let desc_preview = if spec.description.len() > 100 {
        format!("{}...", &spec.description[..100])
    } else {
        spec.description.clone()
    };
    let first_line = desc_preview.lines().next().unwrap_or(&desc_preview);
    println!("{BLUE}Description:{RESET} {}", first_line);
    println!();

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

/// Print history entry.
pub fn print_history_entry(state: &RunState, index: usize) {
    let status_color = match state.status {
        RunStatus::Completed => GREEN,
        RunStatus::Failed => RED,
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

/// Print a prominent warning panel for missing spec file.
pub fn print_missing_spec_warning(branch_name: &str, spec_path: &str) {
    let top_border = format!("╔{}╗", "═".repeat(WARNING_PANEL_WIDTH - 2));
    let bottom_border = format!("╚{}╝", "═".repeat(WARNING_PANEL_WIDTH - 2));
    let separator = format!("╟{}╢", "─".repeat(WARNING_PANEL_WIDTH - 2));

    println!();
    println!("{YELLOW}{BOLD}{}{RESET}", top_border);

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

    print_warning_panel_line("The PR review will proceed with reduced context.");
    print_warning_panel_line("");
    print_warning_panel_line(&format!("Branch: {}", branch_name));

    let max_path_len = WARNING_PANEL_WIDTH - 12;
    let display_path = if spec_path.len() > max_path_len {
        format!("...{}", &spec_path[spec_path.len() - max_path_len + 3..])
    } else {
        spec_path.to_string()
    };
    print_warning_panel_line(&format!("Expected: {}", display_path));

    println!("{YELLOW}{}{RESET}", separator);

    print_warning_panel_line("Create a spec file to provide full context:");
    print_warning_panel_line("  autom8 --spec <spec.md>");

    println!("{YELLOW}{BOLD}{}{RESET}", bottom_border);
    println!();
}

fn print_warning_panel_line(text: &str) {
    let max_width = WARNING_PANEL_WIDTH - 4;
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
            if commits.len() - max_display == 1 { "" } else { "s" }
        );
    }
    println!();
}
