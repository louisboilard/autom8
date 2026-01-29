//! Utility functions for Claude operations.
//!
//! Provides helper functions for JSON fixing, context building, and output parsing.

use crate::state::IterationRecord;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const WORK_SUMMARY_START: &str = "<work-summary>";
const WORK_SUMMARY_END: &str = "</work-summary>";
const FILES_CONTEXT_START: &str = "<files-context>";
const FILES_CONTEXT_END: &str = "</files-context>";
const DECISIONS_START: &str = "<decisions>";
const DECISIONS_END: &str = "</decisions>";
const PATTERNS_START: &str = "<patterns>";
const PATTERNS_END: &str = "</patterns>";

/// A file context entry extracted from agent output.
/// Contains semantic information about a file the agent worked with.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FileContextEntry {
    /// Path to the file
    pub path: PathBuf,
    /// Brief description of the file's purpose
    pub purpose: String,
    /// Key symbols (functions, types, constants) in this file
    pub key_symbols: Vec<String>,
}

/// A decision extracted from agent output.
/// Represents an architectural or implementation choice made by the agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    /// The topic or area this decision relates to
    pub topic: String,
    /// The choice that was made
    pub choice: String,
    /// Why this choice was made
    pub rationale: String,
}

/// A pattern extracted from agent output.
/// Represents a coding pattern or convention established by the agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Pattern {
    /// Description of the pattern
    pub description: String,
}
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

/// Extract files context from Claude's output using `<files-context>...</files-context>` markers.
/// Returns empty Vec if no valid context is found (graceful degradation).
///
/// Expected format inside tags (one entry per line):
/// `path | purpose | [symbol1, symbol2]`
///
/// Example:
/// ```text
/// <files-context>
/// src/main.rs | Application entry point | [main, run]
/// src/lib.rs | Library exports | []
/// </files-context>
/// ```
pub fn extract_files_context(output: &str) -> Vec<FileContextEntry> {
    let Some(start_idx) = output.find(FILES_CONTEXT_START) else {
        return Vec::new();
    };
    let content_start = start_idx + FILES_CONTEXT_START.len();
    let Some(end_idx) = output[content_start..].find(FILES_CONTEXT_END) else {
        return Vec::new();
    };

    let content = output[content_start..content_start + end_idx].trim();
    if content.is_empty() {
        return Vec::new();
    }

    content
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }

            let parts: Vec<&str> = line.splitn(3, '|').collect();
            if parts.len() < 2 {
                return None;
            }

            let path = PathBuf::from(parts[0].trim());
            let purpose = parts[1].trim().to_string();
            let key_symbols = if parts.len() >= 3 {
                parse_symbol_list(parts[2].trim())
            } else {
                Vec::new()
            };

            Some(FileContextEntry {
                path,
                purpose,
                key_symbols,
            })
        })
        .collect()
}

/// Extract decisions from Claude's output using `<decisions>...</decisions>` markers.
/// Returns empty Vec if no valid decisions are found (graceful degradation).
///
/// Expected format inside tags (one entry per line):
/// `topic | choice | rationale`
///
/// Example:
/// ```text
/// <decisions>
/// Error handling | thiserror crate | Provides clean derive macros
/// Database | SQLite | Embedded, no setup required
/// </decisions>
/// ```
pub fn extract_decisions(output: &str) -> Vec<Decision> {
    let Some(start_idx) = output.find(DECISIONS_START) else {
        return Vec::new();
    };
    let content_start = start_idx + DECISIONS_START.len();
    let Some(end_idx) = output[content_start..].find(DECISIONS_END) else {
        return Vec::new();
    };

    let content = output[content_start..content_start + end_idx].trim();
    if content.is_empty() {
        return Vec::new();
    }

    content
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }

            let parts: Vec<&str> = line.splitn(3, '|').collect();
            if parts.len() < 3 {
                return None;
            }

            Some(Decision {
                topic: parts[0].trim().to_string(),
                choice: parts[1].trim().to_string(),
                rationale: parts[2].trim().to_string(),
            })
        })
        .collect()
}

/// Extract patterns from Claude's output using `<patterns>...</patterns>` markers.
/// Returns empty Vec if no valid patterns are found (graceful degradation).
///
/// Expected format inside tags (one pattern description per line):
///
/// Example:
/// ```text
/// <patterns>
/// Use Result<T, Error> for all fallible operations
/// Prefer explicit error types over Box<dyn Error>
/// </patterns>
/// ```
pub fn extract_patterns(output: &str) -> Vec<Pattern> {
    let Some(start_idx) = output.find(PATTERNS_START) else {
        return Vec::new();
    };
    let content_start = start_idx + PATTERNS_START.len();
    let Some(end_idx) = output[content_start..].find(PATTERNS_END) else {
        return Vec::new();
    };

    let content = output[content_start..content_start + end_idx].trim();
    if content.is_empty() {
        return Vec::new();
    }

    content
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }

            Some(Pattern {
                description: line.to_string(),
            })
        })
        .collect()
}

/// Parse a symbol list in the format `[symbol1, symbol2]` or `[]`.
fn parse_symbol_list(input: &str) -> Vec<String> {
    let trimmed = input.trim();

    // Handle empty brackets or missing brackets
    if trimmed.is_empty() || trimmed == "[]" {
        return Vec::new();
    }

    // Strip brackets if present
    let inner = if trimmed.starts_with('[') && trimmed.ends_with(']') {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };

    if inner.is_empty() {
        return Vec::new();
    }

    inner
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
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

    // ===========================================
    // extract_files_context tests
    // ===========================================

    #[test]
    fn test_extract_files_context_basic() {
        let output = r#"Here's what I did:

<files-context>
src/main.rs | Application entry point | [main, run]
src/lib.rs | Library exports | [Config, Runner]
</files-context>

Done!"#;
        let entries = extract_files_context(output);
        assert_eq!(entries.len(), 2);

        assert_eq!(entries[0].path, PathBuf::from("src/main.rs"));
        assert_eq!(entries[0].purpose, "Application entry point");
        assert_eq!(entries[0].key_symbols, vec!["main", "run"]);

        assert_eq!(entries[1].path, PathBuf::from("src/lib.rs"));
        assert_eq!(entries[1].purpose, "Library exports");
        assert_eq!(entries[1].key_symbols, vec!["Config", "Runner"]);
    }

    #[test]
    fn test_extract_files_context_empty_symbols() {
        let output = r#"<files-context>
src/lib.rs | Library exports | []
</files-context>"#;
        let entries = extract_files_context(output);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].key_symbols.is_empty());
    }

    #[test]
    fn test_extract_files_context_no_symbols_field() {
        let output = r#"<files-context>
src/main.rs | Application entry point
</files-context>"#;
        let entries = extract_files_context(output);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, PathBuf::from("src/main.rs"));
        assert_eq!(entries[0].purpose, "Application entry point");
        assert!(entries[0].key_symbols.is_empty());
    }

    #[test]
    fn test_extract_files_context_missing_tags() {
        let output = "No files context here";
        let entries = extract_files_context(output);
        assert!(entries.is_empty());
    }

    #[test]
    fn test_extract_files_context_empty_content() {
        let output = r#"<files-context>
</files-context>"#;
        let entries = extract_files_context(output);
        assert!(entries.is_empty());
    }

    #[test]
    fn test_extract_files_context_whitespace_only() {
        let output = r#"<files-context>



</files-context>"#;
        let entries = extract_files_context(output);
        assert!(entries.is_empty());
    }

    #[test]
    fn test_extract_files_context_single_symbol() {
        let output = r#"<files-context>
src/config.rs | Configuration | [Config]
</files-context>"#;
        let entries = extract_files_context(output);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].key_symbols, vec!["Config"]);
    }

    #[test]
    fn test_extract_files_context_unclosed_tag() {
        let output = r#"<files-context>
src/main.rs | Entry point | [main]
"#;
        let entries = extract_files_context(output);
        assert!(entries.is_empty());
    }

    #[test]
    fn test_extract_files_context_invalid_line() {
        let output = r#"<files-context>
src/main.rs | Entry point | [main]
invalid line without pipes
src/lib.rs | Library | [mod]
</files-context>"#;
        let entries = extract_files_context(output);
        assert_eq!(entries.len(), 2);
    }

    // ===========================================
    // extract_decisions tests
    // ===========================================

    #[test]
    fn test_extract_decisions_basic() {
        let output = r#"Here's what I decided:

<decisions>
Error handling | thiserror crate | Provides clean derive macros
Database | SQLite | Embedded, no setup required
</decisions>

Done!"#;
        let decisions = extract_decisions(output);
        assert_eq!(decisions.len(), 2);

        assert_eq!(decisions[0].topic, "Error handling");
        assert_eq!(decisions[0].choice, "thiserror crate");
        assert_eq!(decisions[0].rationale, "Provides clean derive macros");

        assert_eq!(decisions[1].topic, "Database");
        assert_eq!(decisions[1].choice, "SQLite");
        assert_eq!(decisions[1].rationale, "Embedded, no setup required");
    }

    #[test]
    fn test_extract_decisions_missing_tags() {
        let output = "No decisions here";
        let decisions = extract_decisions(output);
        assert!(decisions.is_empty());
    }

    #[test]
    fn test_extract_decisions_empty_content() {
        let output = r#"<decisions>
</decisions>"#;
        let decisions = extract_decisions(output);
        assert!(decisions.is_empty());
    }

    #[test]
    fn test_extract_decisions_incomplete_line() {
        let output = r#"<decisions>
Error handling | thiserror
Database | SQLite | Embedded
</decisions>"#;
        let decisions = extract_decisions(output);
        // Only second line is valid (has all 3 parts)
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].topic, "Database");
    }

    #[test]
    fn test_extract_decisions_with_pipes_in_rationale() {
        let output = r#"<decisions>
Separator | Pipe char | Use | for separating values in output
</decisions>"#;
        let decisions = extract_decisions(output);
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].topic, "Separator");
        assert_eq!(decisions[0].choice, "Pipe char");
        // splitn(3, '|') means rationale captures everything after 2nd pipe
        assert_eq!(
            decisions[0].rationale,
            "Use | for separating values in output"
        );
    }

    #[test]
    fn test_extract_decisions_unclosed_tag() {
        let output = r#"<decisions>
Topic | Choice | Rationale
"#;
        let decisions = extract_decisions(output);
        assert!(decisions.is_empty());
    }

    #[test]
    fn test_extract_decisions_whitespace_handling() {
        let output = r#"<decisions>
   Topic   |   Choice   |   Rationale with spaces
</decisions>"#;
        let decisions = extract_decisions(output);
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].topic, "Topic");
        assert_eq!(decisions[0].choice, "Choice");
        assert_eq!(decisions[0].rationale, "Rationale with spaces");
    }

    // ===========================================
    // extract_patterns tests
    // ===========================================

    #[test]
    fn test_extract_patterns_basic() {
        let output = r#"Here are the patterns:

<patterns>
Use Result<T, Error> for all fallible operations
Prefer explicit error types over Box<dyn Error>
Use snake_case for function names
</patterns>

Done!"#;
        let patterns = extract_patterns(output);
        assert_eq!(patterns.len(), 3);

        assert_eq!(
            patterns[0].description,
            "Use Result<T, Error> for all fallible operations"
        );
        assert_eq!(
            patterns[1].description,
            "Prefer explicit error types over Box<dyn Error>"
        );
        assert_eq!(patterns[2].description, "Use snake_case for function names");
    }

    #[test]
    fn test_extract_patterns_missing_tags() {
        let output = "No patterns here";
        let patterns = extract_patterns(output);
        assert!(patterns.is_empty());
    }

    #[test]
    fn test_extract_patterns_empty_content() {
        let output = r#"<patterns>
</patterns>"#;
        let patterns = extract_patterns(output);
        assert!(patterns.is_empty());
    }

    #[test]
    fn test_extract_patterns_with_blank_lines() {
        let output = r#"<patterns>
Pattern one

Pattern two

</patterns>"#;
        let patterns = extract_patterns(output);
        assert_eq!(patterns.len(), 2);
        assert_eq!(patterns[0].description, "Pattern one");
        assert_eq!(patterns[1].description, "Pattern two");
    }

    #[test]
    fn test_extract_patterns_single_pattern() {
        let output = r#"<patterns>
Single pattern here
</patterns>"#;
        let patterns = extract_patterns(output);
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].description, "Single pattern here");
    }

    #[test]
    fn test_extract_patterns_unclosed_tag() {
        let output = r#"<patterns>
Pattern one
"#;
        let patterns = extract_patterns(output);
        assert!(patterns.is_empty());
    }

    #[test]
    fn test_extract_patterns_whitespace_trimmed() {
        let output = r#"<patterns>
   Pattern with leading/trailing spaces
</patterns>"#;
        let patterns = extract_patterns(output);
        assert_eq!(patterns.len(), 1);
        assert_eq!(
            patterns[0].description,
            "Pattern with leading/trailing spaces"
        );
    }

    // ===========================================
    // parse_symbol_list tests
    // ===========================================

    #[test]
    fn test_parse_symbol_list_basic() {
        let symbols = parse_symbol_list("[foo, bar, baz]");
        assert_eq!(symbols, vec!["foo", "bar", "baz"]);
    }

    #[test]
    fn test_parse_symbol_list_empty_brackets() {
        let symbols = parse_symbol_list("[]");
        assert!(symbols.is_empty());
    }

    #[test]
    fn test_parse_symbol_list_empty_string() {
        let symbols = parse_symbol_list("");
        assert!(symbols.is_empty());
    }

    #[test]
    fn test_parse_symbol_list_whitespace() {
        let symbols = parse_symbol_list("  [  foo  ,  bar  ]  ");
        assert_eq!(symbols, vec!["foo", "bar"]);
    }

    #[test]
    fn test_parse_symbol_list_no_brackets() {
        let symbols = parse_symbol_list("foo, bar");
        assert_eq!(symbols, vec!["foo", "bar"]);
    }

    #[test]
    fn test_parse_symbol_list_single_symbol() {
        let symbols = parse_symbol_list("[Config]");
        assert_eq!(symbols, vec!["Config"]);
    }

    // ===========================================
    // Serialization tests for new types
    // ===========================================

    #[test]
    fn test_file_context_entry_serialization() {
        let entry = FileContextEntry {
            path: PathBuf::from("src/main.rs"),
            purpose: "Entry point".to_string(),
            key_symbols: vec!["main".to_string()],
        };

        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("keySymbols"));

        let deserialized: FileContextEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.path, PathBuf::from("src/main.rs"));
        assert_eq!(deserialized.purpose, "Entry point");
        assert_eq!(deserialized.key_symbols, vec!["main"]);
    }

    #[test]
    fn test_decision_serialization() {
        let decision = Decision {
            topic: "DB".to_string(),
            choice: "SQLite".to_string(),
            rationale: "Simple".to_string(),
        };

        let json = serde_json::to_string(&decision).unwrap();
        let deserialized: Decision = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.topic, "DB");
        assert_eq!(deserialized.choice, "SQLite");
        assert_eq!(deserialized.rationale, "Simple");
    }

    #[test]
    fn test_pattern_serialization() {
        let pattern = Pattern {
            description: "Use Result for errors".to_string(),
        };

        let json = serde_json::to_string(&pattern).unwrap();
        let deserialized: Pattern = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.description, "Use Result for errors");
    }

    // ===========================================
    // Integration tests with combined output
    // ===========================================

    #[test]
    fn test_extract_all_context_types() {
        let output = r#"I've completed the implementation.

<work-summary>
Files changed: src/main.rs, src/lib.rs. Added authentication module.
</work-summary>

<files-context>
src/main.rs | Application entry point | [main]
src/auth.rs | Authentication logic | [authenticate, verify]
</files-context>

<decisions>
Auth method | JWT | Stateless, scalable
</decisions>

<patterns>
Use Result<T, AuthError> for auth operations
</patterns>

Done!"#;

        let summary = extract_work_summary(output);
        assert!(summary.is_some());
        assert!(summary.unwrap().contains("authentication module"));

        let files = extract_files_context(output);
        assert_eq!(files.len(), 2);

        let decisions = extract_decisions(output);
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].topic, "Auth method");

        let patterns = extract_patterns(output);
        assert_eq!(patterns.len(), 1);
        assert!(patterns[0].description.contains("AuthError"));
    }

    #[test]
    fn test_extract_from_output_with_no_context() {
        let output = "Just some regular output with no special tags";

        let summary = extract_work_summary(output);
        assert!(summary.is_none());

        let files = extract_files_context(output);
        assert!(files.is_empty());

        let decisions = extract_decisions(output);
        assert!(decisions.is_empty());

        let patterns = extract_patterns(output);
        assert!(patterns.is_empty());
    }
}
