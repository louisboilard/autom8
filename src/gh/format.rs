//! PR title and description formatting.

use crate::spec::Spec;

/// Maximum length for PR titles (GitHub standard)
const PR_TITLE_MAX_LENGTH: usize = 72;

/// Format a Spec into a concise PR title
pub fn format_pr_title(spec: &Spec) -> String {
    let first_part = extract_first_line_or_sentence(&spec.description);

    let title = if spec.project.is_empty() {
        first_part
    } else {
        format!("[{}] {}", spec.project, first_part)
    };

    truncate_with_ellipsis(&title, PR_TITLE_MAX_LENGTH)
}

/// Extract the first line or first sentence from text
fn extract_first_line_or_sentence(text: &str) -> String {
    if let Some(newline_pos) = text.find('\n') {
        let first_line = text[..newline_pos].trim();
        if !first_line.is_empty() {
            return first_line.to_string();
        }
    }

    for (i, c) in text.char_indices() {
        if c == '.' || c == '!' || c == '?' {
            let next_idx = i + c.len_utf8();
            if next_idx >= text.len() || text[next_idx..].starts_with(' ') {
                let sentence = text[..=i].trim();
                if !sentence.is_empty() {
                    return sentence.to_string();
                }
            }
        }
    }

    text.trim().to_string()
}

/// Truncate a string to a maximum length, adding "..." if truncated
fn truncate_with_ellipsis(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        return text.to_string();
    }

    let target_len = max_len - 3;
    let truncate_at = text[..target_len].rfind(' ').unwrap_or(target_len);

    format!("{}...", text[..truncate_at].trim_end())
}

/// Format a Spec into a well-structured GitHub PR description in Markdown format
pub fn format_pr_description(spec: &Spec) -> String {
    let mut output = String::new();

    output.push_str("## Summary\n\n");
    output.push_str(&spec.description);
    output.push_str("\n\n");

    let completed: Vec<_> = spec.user_stories.iter().filter(|s| s.passes).collect();
    let incomplete: Vec<_> = spec.user_stories.iter().filter(|s| !s.passes).collect();

    if completed.is_empty() {
        output.push_str("## Changes\n\n");
        for story in &spec.user_stories {
            format_story(&mut output, story);
        }
    } else {
        output.push_str("## Completed\n\n");
        for story in &completed {
            format_story(&mut output, story);
        }

        if !incomplete.is_empty() {
            output.push_str("## Remaining\n\n");
            for story in &incomplete {
                format_story(&mut output, story);
            }
        }
    }

    output.trim_end().to_string()
}

/// Format a single user story for the PR description
fn format_story(output: &mut String, story: &crate::spec::UserStory) {
    output.push_str(&format!("### {}: {}\n\n", story.id, story.title));
    output.push_str(&story.description);
    output.push_str("\n\n");

    if !story.acceptance_criteria.is_empty() {
        output.push_str("**Acceptance Criteria:**\n\n");
        let checkbox = if story.passes { "[x]" } else { "[ ]" };
        for criterion in &story.acceptance_criteria {
            output.push_str(&format!("- {} {}\n", checkbox, criterion));
        }
        output.push('\n');
    }

    if !story.notes.is_empty() {
        output.push_str("**Notes:**\n\n");
        output.push_str(&story.notes);
        output.push_str("\n\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_pr_title_simple() {
        let spec = Spec {
            project: "TestApp".into(),
            description: "Add user authentication.".into(),
            branch_name: "feature/auth".into(),
            user_stories: vec![],
        };
        let title = format_pr_title(&spec);
        assert_eq!(title, "[TestApp] Add user authentication.");
    }

    #[test]
    fn test_format_pr_title_truncation() {
        let spec = Spec {
            project: "TestApp".into(),
            description: "This is a very long description that exceeds the maximum GitHub title length and should be truncated.".into(),
            branch_name: "feature/test".into(),
            user_stories: vec![],
        };
        let title = format_pr_title(&spec);
        assert!(title.len() <= 72);
        assert!(title.ends_with("..."));
    }
}
