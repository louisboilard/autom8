//! Progress display and run summary.
//!
//! Provides progress bars, story completion tracking, and run summaries.

use crate::progress::{format_tokens, Breadcrumb};

use super::colors::*;

/// Result information for a completed story.
#[derive(Debug, Clone)]
pub struct StoryResult {
    pub id: String,
    pub title: String,
    pub passed: bool,
    pub duration_secs: u64,
}

/// Make a progress bar string.
pub fn make_progress_bar(completed: usize, total: usize, width: usize) -> String {
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

/// Print story complete message.
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

/// Print all complete message.
pub fn print_all_complete() {
    println!();
    println!("{GREEN}{BOLD}All stories completed!{RESET}");
    println!();
}

/// Print the final run completion message with duration and optional token count.
///
/// Format: `✓ Run completed in 1h 2m 26s - 1,234,567 total tokens`
///
/// If total_tokens is None, the token portion is omitted:
/// Format: `✓ Run completed in 1h 2m 26s`
pub fn print_run_completed(duration_secs: u64, total_tokens: Option<u64>) {
    let duration = format_run_duration(duration_secs);
    let token_suffix = total_tokens
        .map(|t| format!(" - {} total tokens", format_tokens(t)))
        .unwrap_or_default();

    println!();
    println!(
        "{GREEN}\u{2714} Run completed in {}{}{RESET}",
        duration, token_suffix
    );
    println!();
}

/// Format duration for run completion: "Xh Ym Zs" for hours, "Xm Ys" for minutes, "Xs" for seconds
fn format_run_duration(secs: u64) -> String {
    if secs >= 3600 {
        let hours = secs / 3600;
        let mins = (secs % 3600) / 60;
        let remaining_secs = secs % 60;
        format!("{}h {}m {}s", hours, mins, remaining_secs)
    } else if secs >= 60 {
        let mins = secs / 60;
        let remaining_secs = secs % 60;
        format!("{}m {}s", mins, remaining_secs)
    } else {
        format!("{}s", secs)
    }
}

/// Print reviewing message.
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

/// Print skip review message.
pub fn print_skip_review() {
    println!();
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!("{YELLOW}Skipping review (--skip-review flag set){RESET}");
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!();
}

/// Print review passed message.
pub fn print_review_passed() {
    println!();
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!("{GREEN}{BOLD}Review passed! Proceeding to commit.{RESET}");
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!();
}

/// Print issues found message.
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

/// Print max review iterations message.
pub fn print_max_review_iterations() {
    println!();
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!("{RED}{BOLD}Review failed after 3 attempts.{RESET}");
    println!("{GRAY}{}{RESET}", "-".repeat(57));
    println!();
}

/// Print a progress bar showing task (story) completion status.
pub fn print_tasks_progress(completed: usize, total: usize) {
    let progress_bar = make_progress_bar(completed, total, 12);
    println!(
        "{BLUE}Tasks:{RESET}   [{}] {}/{} complete",
        progress_bar, completed, total
    );
}

/// Print a progress bar showing review iteration status.
pub fn print_review_progress(current: u32, max: u32) {
    let progress_bar = make_progress_bar(current as usize, max as usize, 8);
    println!(
        "{BLUE}Review:{RESET}  [{}] {}/{}",
        progress_bar, current, max
    );
}

/// Print both tasks progress and review progress.
pub fn print_full_progress(
    tasks_completed: usize,
    tasks_total: usize,
    review_current: u32,
    review_max: u32,
) {
    print_tasks_progress(tasks_completed, tasks_total);
    print_review_progress(review_current, review_max);
}

/// Print run summary.
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
pub fn print_breadcrumb_trail(breadcrumb: &Breadcrumb) {
    breadcrumb.print();
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // US-007: Run duration formatting tests
    // ========================================================================

    #[test]
    fn test_format_run_duration_seconds() {
        assert_eq!(format_run_duration(0), "0s");
        assert_eq!(format_run_duration(1), "1s");
        assert_eq!(format_run_duration(30), "30s");
        assert_eq!(format_run_duration(59), "59s");
    }

    #[test]
    fn test_format_run_duration_minutes() {
        assert_eq!(format_run_duration(60), "1m 0s");
        assert_eq!(format_run_duration(90), "1m 30s");
        assert_eq!(format_run_duration(125), "2m 5s");
        assert_eq!(format_run_duration(3599), "59m 59s");
    }

    #[test]
    fn test_format_run_duration_hours() {
        assert_eq!(format_run_duration(3600), "1h 0m 0s");
        assert_eq!(format_run_duration(3661), "1h 1m 1s");
        assert_eq!(format_run_duration(7326), "2h 2m 6s");
        assert_eq!(format_run_duration(3723), "1h 2m 3s");
    }

    #[test]
    fn test_format_run_duration_large() {
        // 10 hours, 30 minutes, 45 seconds
        assert_eq!(format_run_duration(37845), "10h 30m 45s");
    }
}
