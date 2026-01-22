use crate::output::{GREEN, RED, RESET};
use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

const SPINNER_CHARS: &str = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏";

pub struct ClaudeSpinner {
    spinner: ProgressBar,
    story_id: String,
}

impl ClaudeSpinner {
    pub fn new(story_id: &str) -> Self {
        let spinner = ProgressBar::new_spinner();
        spinner.set_style(
            ProgressStyle::default_spinner()
                .tick_chars(SPINNER_CHARS)
                .template("{spinner:.cyan} Claude working on {msg}")
                .expect("invalid template"),
        );
        spinner.set_message(format!("{} | Starting... [00:00:00]", story_id));
        spinner.enable_steady_tick(Duration::from_millis(80));

        Self {
            spinner,
            story_id: story_id.to_string(),
        }
    }

    pub fn new_for_prd() -> Self {
        let spinner = ProgressBar::new_spinner();
        spinner.set_style(
            ProgressStyle::default_spinner()
                .tick_chars(SPINNER_CHARS)
                .template("{spinner:.cyan} Claude working on {msg}")
                .expect("invalid template"),
        );
        spinner.set_message("PRD generation | Starting... [00:00:00]");
        spinner.enable_steady_tick(Duration::from_millis(80));

        Self {
            spinner,
            story_id: "PRD".to_string(),
        }
    }

    pub fn new_for_commit() -> Self {
        let spinner = ProgressBar::new_spinner();
        spinner.set_style(
            ProgressStyle::default_spinner()
                .tick_chars(SPINNER_CHARS)
                .template("{spinner:.cyan} Claude working on {msg}")
                .expect("invalid template"),
        );
        spinner.set_message("Committing | Starting... [00:00:00]");
        spinner.enable_steady_tick(Duration::from_millis(80));

        Self {
            spinner,
            story_id: "Commit".to_string(),
        }
    }

    pub fn update(&self, activity: &str) {
        let elapsed = self.spinner.elapsed();
        let hours = elapsed.as_secs() / 3600;
        let mins = (elapsed.as_secs() % 3600) / 60;
        let secs = elapsed.as_secs() % 60;
        let time_str = format!("{:02}:{:02}:{:02}", hours, mins, secs);

        // Truncate activity to fit on one line (max ~50 chars)
        let truncated = truncate_activity(activity, 50);

        if self.story_id == "PRD" {
            self.spinner
                .set_message(format!("PRD generation | {} [{}]", truncated, time_str));
        } else {
            self.spinner
                .set_message(format!("{} | {} [{}]", self.story_id, truncated, time_str));
        }
    }

    pub fn finish_success(&self, duration_secs: u64) {
        let mins = duration_secs / 60;
        let secs = duration_secs % 60;
        self.spinner.finish_with_message(format!(
            "{GREEN}{} completed in {}m {}s{RESET}",
            self.story_id, mins, secs
        ));
    }

    pub fn finish_error(&self, error: &str) {
        let truncated = truncate_activity(error, 60);
        self.spinner.finish_with_message(format!(
            "{RED}{} failed: {}{RESET}",
            self.story_id, truncated
        ));
    }

    pub fn finish_with_message(&self, message: &str) {
        self.spinner.finish_with_message(format!(
            "{GREEN}{}: {}{RESET}",
            self.story_id, message
        ));
    }
}

fn truncate_activity(activity: &str, max_len: usize) -> String {
    // Take first line only and clean it up
    let first_line = activity.lines().next().unwrap_or(activity);
    let cleaned = first_line.trim();

    // Count characters (not bytes) to handle UTF-8 properly
    let char_count = cleaned.chars().count();
    if char_count <= max_len {
        cleaned.to_string()
    } else {
        let truncated: String = cleaned.chars().take(max_len - 3).collect();
        format!("{}...", truncated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_activity_short() {
        let result = truncate_activity("Short message", 50);
        assert_eq!(result, "Short message");
    }

    #[test]
    fn test_truncate_activity_long() {
        let long_msg = "This is a very long message that should be truncated because it exceeds the maximum length";
        let result = truncate_activity(long_msg, 30);
        assert_eq!(result.chars().count(), 30);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_truncate_activity_multiline() {
        let multiline = "First line\nSecond line\nThird line";
        let result = truncate_activity(multiline, 50);
        assert_eq!(result, "First line");
    }

    #[test]
    fn test_truncate_activity_utf8() {
        // Should not panic on multi-byte UTF-8 characters
        let utf8_msg = "Implementing 日本語 feature with more text here";
        let result = truncate_activity(utf8_msg, 20);
        assert_eq!(result.chars().count(), 20);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_truncate_activity_exact_boundary() {
        let msg = "Exactly twenty chars";
        let result = truncate_activity(msg, 20);
        assert_eq!(result, "Exactly twenty chars");
    }
}
