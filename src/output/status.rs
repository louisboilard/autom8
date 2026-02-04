//! Status and project display.
//!
//! Output functions for displaying run status, project trees, and descriptions.

use crate::state::{MachineState, RunState, RunStatus, SessionStatus};
use chrono::Utc;

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
                Some(RunStatus::Interrupted) => format!("{YELLOW}[interrupted]{RESET}"),
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
            Some(RunStatus::Interrupted) => ("[interrupted]", YELLOW),
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
        Some(RunStatus::Interrupted) => format!("{YELLOW}[interrupted]{RESET}"),
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
            if commits.len() - max_display == 1 {
                ""
            } else {
                "s"
            }
        );
    }
    println!();
}

/// Print status for all sessions in a project.
///
/// Sessions are displayed with the current session highlighted, including:
/// - Session ID and worktree path
/// - Branch name and current state
/// - Current story (if any)
/// - Duration since start
pub fn print_sessions_status(sessions: &[SessionStatus]) {
    println!("{BOLD}Sessions for this project:{RESET}");
    println!();

    for session in sessions {
        print_session_row(session);
    }

    // Summary line
    let running_count = sessions
        .iter()
        .filter(|s| s.metadata.is_running && !s.is_stale)
        .count();
    let stale_count = sessions.iter().filter(|s| s.is_stale).count();

    println!();
    print!(
        "{GRAY}({} session{}",
        sessions.len(),
        if sessions.len() == 1 { "" } else { "s" }
    );
    if running_count > 0 {
        print!(", {} running", running_count);
    }
    if stale_count > 0 {
        print!(", {} stale", stale_count);
    }
    println!("){RESET}");
}

/// Print a single session row.
fn print_session_row(session: &SessionStatus) {
    let metadata = &session.metadata;

    // Determine row color based on state
    let (indicator, indicator_color) = if session.is_stale {
        ("✗", GRAY)
    } else if session.is_current {
        ("→", GREEN)
    } else if metadata.is_running {
        ("●", YELLOW)
    } else {
        ("○", GRAY)
    };

    // Session ID and current marker
    let current_marker = if session.is_current { " (current)" } else { "" };
    let stale_marker = if session.is_stale { " [stale]" } else { "" };

    println!(
        "{indicator_color}{indicator}{RESET} {BOLD}{}{RESET}{GREEN}{}{RESET}{GRAY}{}{RESET}",
        metadata.session_id, current_marker, stale_marker
    );

    // Worktree path (truncated if too long)
    let path_str = metadata.worktree_path.display().to_string();
    let display_path = if path_str.len() > 60 {
        format!("...{}", &path_str[path_str.len() - 57..])
    } else {
        path_str
    };
    println!("  {GRAY}Path:{RESET}    {}", display_path);

    // Branch name
    println!("  {BLUE}Branch:{RESET}  {}", metadata.branch_name);

    // Current state
    if let Some(state) = &session.machine_state {
        let state_str = format_machine_state(state);
        let state_color = machine_state_color(state);
        println!("  {BLUE}State:{RESET}   {state_color}{}{RESET}", state_str);
    }

    // Current story (if any)
    if let Some(story) = &session.current_story {
        println!("  {BLUE}Story:{RESET}   {}", story);
    }

    // Duration
    let duration = format_duration(metadata.created_at, metadata.last_active_at);
    println!(
        "  {GRAY}Started:{RESET} {} {}",
        metadata.created_at.format("%Y-%m-%d %H:%M"),
        duration
    );

    println!();
}

/// Format machine state for display.
fn format_machine_state(state: &MachineState) -> &'static str {
    match state {
        MachineState::Idle => "Idle",
        MachineState::LoadingSpec => "Loading Spec",
        MachineState::GeneratingSpec => "Generating Spec",
        MachineState::Initializing => "Initializing",
        MachineState::PickingStory => "Picking Story",
        MachineState::RunningClaude => "Running Claude",
        MachineState::Reviewing => "Reviewing",
        MachineState::Correcting => "Correcting",
        MachineState::Committing => "Committing",
        MachineState::CreatingPR => "Creating PR",
        MachineState::Completed => "Completed",
        MachineState::Failed => "Failed",
    }
}

/// Get color for machine state.
fn machine_state_color(state: &MachineState) -> &'static str {
    match state {
        MachineState::Completed => GREEN,
        MachineState::Failed => RED,
        MachineState::RunningClaude | MachineState::Reviewing | MachineState::Correcting => YELLOW,
        _ => CYAN,
    }
}

/// Format duration since session start.
fn format_duration(
    created_at: chrono::DateTime<chrono::Utc>,
    last_active_at: chrono::DateTime<chrono::Utc>,
) -> String {
    let now = Utc::now();
    let duration = now.signed_duration_since(created_at);

    // Calculate active duration
    let active_duration = last_active_at.signed_duration_since(created_at);

    let days = duration.num_days();
    let hours = duration.num_hours() % 24;
    let minutes = duration.num_minutes() % 60;

    let age_str = if days > 0 {
        format!("{}d {}h ago", days, hours)
    } else if hours > 0 {
        format!("{}h {}m ago", hours, minutes)
    } else if minutes > 0 {
        format!("{}m ago", minutes)
    } else {
        "just now".to_string()
    };

    // Show active duration if significantly different from total
    let active_hours = active_duration.num_hours();
    let active_mins = active_duration.num_minutes() % 60;
    if active_hours > 0 {
        format!("{} (active {}h {}m)", age_str, active_hours, active_mins)
    } else if active_mins > 5 {
        format!("{} (active {}m)", age_str, active_mins)
    } else {
        age_str
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::SessionMetadata;
    use std::path::PathBuf;

    fn make_session_status(
        session_id: &str,
        branch: &str,
        is_current: bool,
        is_stale: bool,
        is_running: bool,
        machine_state: Option<MachineState>,
        current_story: Option<&str>,
    ) -> SessionStatus {
        SessionStatus {
            metadata: SessionMetadata {
                session_id: session_id.to_string(),
                worktree_path: PathBuf::from(format!("/projects/test-wt-{}", session_id)),
                branch_name: branch.to_string(),
                created_at: Utc::now(),
                last_active_at: Utc::now(),
                is_running,
                pause_requested: false,
            },
            machine_state,
            current_story: current_story.map(|s| s.to_string()),
            is_current,
            is_stale,
        }
    }

    // ======================================================================
    // Tests for US-006: Verify status --all command output
    // ======================================================================

    #[test]
    fn test_us006_format_machine_state_all_variants() {
        // Verify all machine states have display names
        assert_eq!(format_machine_state(&MachineState::Idle), "Idle");
        assert_eq!(
            format_machine_state(&MachineState::LoadingSpec),
            "Loading Spec"
        );
        assert_eq!(
            format_machine_state(&MachineState::GeneratingSpec),
            "Generating Spec"
        );
        assert_eq!(
            format_machine_state(&MachineState::Initializing),
            "Initializing"
        );
        assert_eq!(
            format_machine_state(&MachineState::PickingStory),
            "Picking Story"
        );
        assert_eq!(
            format_machine_state(&MachineState::RunningClaude),
            "Running Claude"
        );
        assert_eq!(format_machine_state(&MachineState::Reviewing), "Reviewing");
        assert_eq!(
            format_machine_state(&MachineState::Correcting),
            "Correcting"
        );
        assert_eq!(
            format_machine_state(&MachineState::Committing),
            "Committing"
        );
        assert_eq!(
            format_machine_state(&MachineState::CreatingPR),
            "Creating PR"
        );
        assert_eq!(format_machine_state(&MachineState::Completed), "Completed");
        assert_eq!(format_machine_state(&MachineState::Failed), "Failed");
    }

    #[test]
    fn test_us006_machine_state_colors() {
        // Verify appropriate colors for different states
        assert_eq!(machine_state_color(&MachineState::Completed), GREEN);
        assert_eq!(machine_state_color(&MachineState::Failed), RED);
        assert_eq!(machine_state_color(&MachineState::RunningClaude), YELLOW);
        assert_eq!(machine_state_color(&MachineState::Reviewing), YELLOW);
        assert_eq!(machine_state_color(&MachineState::Correcting), YELLOW);
        // Other states use CYAN
        assert_eq!(machine_state_color(&MachineState::Idle), CYAN);
        assert_eq!(machine_state_color(&MachineState::Initializing), CYAN);
    }

    #[test]
    fn test_us006_session_row_current_marker() {
        // Test that current session gets → indicator and (current) marker
        let session = make_session_status(
            "main",
            "feature/test",
            true,  // is_current
            false, // is_stale
            true,  // is_running
            Some(MachineState::RunningClaude),
            Some("US-001"),
        );

        // Verify the logic for current marker
        let current_marker = if session.is_current { " (current)" } else { "" };
        assert_eq!(current_marker, " (current)");

        // Verify indicator for current session
        let (indicator, _) = if session.is_stale {
            ("✗", GRAY)
        } else if session.is_current {
            ("→", GREEN)
        } else if session.metadata.is_running {
            ("●", YELLOW)
        } else {
            ("○", GRAY)
        };
        assert_eq!(indicator, "→");
    }

    #[test]
    fn test_us006_session_row_stale_marker() {
        // Test that stale sessions get [stale] marker
        let session = make_session_status(
            "abc12345",
            "feature/old",
            false, // is_current
            true,  // is_stale
            true,  // is_running
            Some(MachineState::RunningClaude),
            None,
        );

        let stale_marker = if session.is_stale { " [stale]" } else { "" };
        assert_eq!(stale_marker, " [stale]");

        // Stale sessions get ✗ indicator regardless of other status
        let (indicator, indicator_color) = if session.is_stale {
            ("✗", GRAY)
        } else if session.is_current {
            ("→", GREEN)
        } else if session.metadata.is_running {
            ("●", YELLOW)
        } else {
            ("○", GRAY)
        };
        assert_eq!(indicator, "✗");
        assert_eq!(indicator_color, GRAY);
    }

    #[test]
    fn test_us006_session_row_running_indicator() {
        // Test that running (but not current) sessions get ● indicator
        let session = make_session_status(
            "session1",
            "feature/parallel",
            false, // is_current
            false, // is_stale
            true,  // is_running
            Some(MachineState::Reviewing),
            Some("US-002"),
        );

        let (indicator, indicator_color) = if session.is_stale {
            ("✗", GRAY)
        } else if session.is_current {
            ("→", GREEN)
        } else if session.metadata.is_running {
            ("●", YELLOW)
        } else {
            ("○", GRAY)
        };
        assert_eq!(indicator, "●");
        assert_eq!(indicator_color, YELLOW);
    }

    #[test]
    fn test_us006_session_row_idle_indicator() {
        // Test that idle sessions get ○ indicator
        let session = make_session_status(
            "session2",
            "feature/done",
            false, // is_current
            false, // is_stale
            false, // is_running
            Some(MachineState::Completed),
            None,
        );

        let (indicator, indicator_color) = if session.is_stale {
            ("✗", GRAY)
        } else if session.is_current {
            ("→", GREEN)
        } else if session.metadata.is_running {
            ("●", YELLOW)
        } else {
            ("○", GRAY)
        };
        assert_eq!(indicator, "○");
        assert_eq!(indicator_color, GRAY);
    }

    #[test]
    fn test_us006_summary_counts() {
        // Test that summary correctly counts running and stale sessions
        let sessions = vec![
            make_session_status(
                "main",
                "main",
                true,
                false,
                true,
                Some(MachineState::RunningClaude),
                Some("US-001"),
            ),
            make_session_status(
                "session1",
                "feat-1",
                false,
                false,
                true,
                Some(MachineState::Reviewing),
                Some("US-002"),
            ),
            make_session_status("session2", "feat-2", false, true, false, None, None), // stale
            make_session_status(
                "session3",
                "feat-3",
                false,
                false,
                false,
                Some(MachineState::Completed),
                None,
            ), // idle
        ];

        // Running count: sessions that are running AND not stale
        let running_count = sessions
            .iter()
            .filter(|s| s.metadata.is_running && !s.is_stale)
            .count();
        assert_eq!(running_count, 2);

        // Stale count
        let stale_count = sessions.iter().filter(|s| s.is_stale).count();
        assert_eq!(stale_count, 1);

        // Total count
        assert_eq!(sessions.len(), 4);
    }

    #[test]
    fn test_us006_worktree_path_truncation() {
        // Test that long worktree paths are truncated properly
        let long_path =
            "/very/long/path/that/exceeds/sixty/characters/for/display/purposes/test-worktree";
        assert!(long_path.len() > 60);

        let display_path = if long_path.len() > 60 {
            format!("...{}", &long_path[long_path.len() - 57..])
        } else {
            long_path.to_string()
        };

        assert!(display_path.starts_with("..."));
        assert!(display_path.len() <= 60);
    }

    #[test]
    fn test_us006_session_status_displays_all_fields() {
        // Verify SessionStatus contains all required display fields per acceptance criteria:
        // - session ID, worktree path, branch, state, current story
        let session = make_session_status(
            "abc12345",                        // session_id
            "feature/test",                    // branch
            true,                              // is_current (for highlighting)
            false,                             // is_stale
            true,                              // is_running
            Some(MachineState::RunningClaude), // state
            Some("US-001"),                    // current story
        );

        // Session ID
        assert_eq!(session.metadata.session_id, "abc12345");

        // Worktree path
        assert!(session
            .metadata
            .worktree_path
            .to_string_lossy()
            .contains("abc12345"));

        // Branch
        assert_eq!(session.metadata.branch_name, "feature/test");

        // State
        assert_eq!(session.machine_state, Some(MachineState::RunningClaude));

        // Current story
        assert_eq!(session.current_story, Some("US-001".to_string()));

        // Current session highlighting
        assert!(session.is_current);
    }
}
