//! Progress display and run summary.
//!
//! Provides progress bars, story completion tracking, and run summaries.

use crate::progress::Breadcrumb;

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
