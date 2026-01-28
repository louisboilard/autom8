//! Utility functions for Claude operations.
//!
//! Provides helper functions for JSON fixing, context building, and output parsing.

use crate::state::IterationRecord;

const WORK_SUMMARY_START: &str = "<work-summary>";
const WORK_SUMMARY_END: &str = "</work-summary>";
const MAX_WORK_SUMMARY_LENGTH: usize = 500;

/// Extract work summary from Claude's output using <work-summary>...</work-summary> markers.
/// Returns None if no valid summary is found, for graceful degradation.
/// Truncates to MAX_WORK_SUMMARY_LENGTH chars to prevent prompt bloat.
pub fn extract_work_summary(output: &str) -> Option<String> {
    let start_idx = output.find(WORK_SUMMARY_START)?;
    let content_start = start_idx + WORK_SUMMARY_START.len();
    let end_idx = output[content_start..].find(WORK_SUMMARY_END)?;

    let summary = output[content_start..content_start + end_idx].trim();

    if summary.is_empty() {
        return None;
    }

    // Truncate to max length to prevent prompt bloat
    let truncated = if summary.len() > MAX_WORK_SUMMARY_LENGTH {
        let mut end = MAX_WORK_SUMMARY_LENGTH;
        // Try to truncate at a word boundary
        if let Some(last_space) = summary[..end].rfind(' ') {
            end = last_space;
        }
        format!("{}...", &summary[..end])
    } else {
        summary.to_string()
    };

    Some(truncated)
}

/// Build a context string from previous iteration work summaries.
/// Returns None if there are no previous iterations with summaries.
/// Format: "US-001: [summary]\nUS-002: [summary]"
pub fn build_previous_context(iterations: &[IterationRecord]) -> Option<String> {
    let summaries: Vec<String> = iterations
        .iter()
        .filter_map(|iter| {
            iter.work_summary
                .as_ref()
                .map(|summary| format!("{}: {}", iter.story_id, summary))
        })
        .collect();

    if summaries.is_empty() {
        None
    } else {
        Some(summaries.join("\n"))
    }
}

/// Fix common JSON syntax errors without calling Claude.
/// This is a conservative fixer that only corrects unambiguous errors:
/// - Strips markdown code fences (```json ... ``` and ``` ... ```)
/// - Removes trailing commas before ] and }
/// - Quotes unquoted keys that match identifier patterns
///
/// The function is idempotent - running it twice produces the same output.
pub fn fix_json_syntax(input: &str) -> String {
    use regex::Regex;

    let mut result = input.to_string();

    // Step 1: Strip markdown code fences
    let code_fence_re = Regex::new(r"(?s)^```(?:json)?\s*\n?(.*?)\n?```\s*$").unwrap();
    if let Some(captures) = code_fence_re.captures(&result) {
        if let Some(content) = captures.get(1) {
            result = content.as_str().to_string();
        }
    }

    // Also handle code fences that aren't at the start/end but wrap the entire JSON
    let inline_fence_re = Regex::new(r"(?s)```(?:json)?\s*\n(.*?)\n```").unwrap();
    if let Some(captures) = inline_fence_re.captures(&result) {
        if let Some(content) = captures.get(1) {
            result = content.as_str().to_string();
        }
    }

    // Step 2: Quote unquoted keys that match identifier patterns
    let unquoted_key_re = Regex::new(r#"([{,]\s*)([a-zA-Z_][a-zA-Z0-9_]*)(\s*:)"#).unwrap();
    result = unquoted_key_re
        .replace_all(&result, |caps: &regex::Captures| {
            format!(
                "{}\"{}\"{}",
                caps.get(1).map_or("", |m| m.as_str()),
                caps.get(2).map_or("", |m| m.as_str()),
                caps.get(3).map_or("", |m| m.as_str())
            )
        })
        .to_string();

    // Step 3: Remove trailing commas before ] and }
    let trailing_comma_re = Regex::new(r",(\s*[}\]])").unwrap();
    result = trailing_comma_re.replace_all(&result, "$1").to_string();

    result.trim().to_string()
}

/// Extract JSON from Claude's response, handling potential markdown code blocks
pub fn extract_json(response: &str) -> Option<String> {
    let trimmed = response.trim();

    // Try to find JSON in markdown code block
    if let Some(start) = trimmed.find("```json") {
        let content_start = start + 7;
        if let Some(end) = trimmed[content_start..].find("```") {
            return Some(
                trimmed[content_start..content_start + end]
                    .trim()
                    .to_string(),
            );
        }
    }

    // Try to find JSON in generic code block
    if let Some(start) = trimmed.find("```") {
        let content_start = start + 3;
        let content_start = trimmed[content_start..]
            .find('\n')
            .map(|i| content_start + i + 1)
            .unwrap_or(content_start);
        if let Some(end) = trimmed[content_start..].find("```") {
            return Some(
                trimmed[content_start..content_start + end]
                    .trim()
                    .to_string(),
            );
        }
    }

    // Try to find raw JSON object
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            if end > start {
                return Some(trimmed[start..=end].to_string());
            }
        }
    }

    None
}

/// Truncate JSON string for error preview, preserving readability.
pub fn truncate_json_preview(json: &str, max_len: usize) -> String {
    let trimmed = json.trim();
    if trimmed.len() <= max_len {
        trimmed.to_string()
    } else {
        format!("{}...", &trimmed[..max_len])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_work_summary_basic() {
        let output = r#"I made some changes.

<work-summary>
Files changed: src/main.rs, src/lib.rs. Added new authentication module.
</work-summary>

Done!"#;
        let summary = extract_work_summary(output);
        assert!(summary.is_some());
        assert!(summary.unwrap().contains("Files changed"));
    }

    #[test]
    fn test_extract_work_summary_missing() {
        let output = "No summary here";
        let summary = extract_work_summary(output);
        assert!(summary.is_none());
    }

    #[test]
    fn test_extract_json_from_code_block() {
        let response = r#"Here's the JSON:
```json
{"project": "Test"}
```
Done!"#;
        let json = extract_json(response).unwrap();
        assert_eq!(json, r#"{"project": "Test"}"#);
    }

    #[test]
    fn test_extract_json_raw() {
        let response = r#"{"project": "Test", "branchName": "main"}"#;
        let json = extract_json(response).unwrap();
        assert_eq!(json, r#"{"project": "Test", "branchName": "main"}"#);
    }
}
