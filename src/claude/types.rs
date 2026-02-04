//! Core types for Claude operations.
//!
//! Defines error types, result enums, outcome structures, and usage tracking
//! used throughout the Claude integration.

use serde::{Deserialize, Serialize};

/// Represents token usage data from Claude CLI responses.
///
/// This struct captures detailed token consumption metrics from Claude API calls,
/// including input/output tokens, cache statistics, and model information.
/// Used for tracking resource consumption across stories and runs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeUsage {
    /// Number of input tokens consumed
    #[serde(default)]
    pub input_tokens: u64,
    /// Number of output tokens generated
    #[serde(default)]
    pub output_tokens: u64,
    /// Number of tokens read from cache
    #[serde(default)]
    pub cache_read_tokens: u64,
    /// Number of tokens written to cache
    #[serde(default)]
    pub cache_creation_tokens: u64,
    /// Number of tokens used for thinking/reasoning
    #[serde(default)]
    pub thinking_tokens: u64,
    /// The Claude model used (e.g., "claude-sonnet-4-20250514")
    #[serde(default)]
    pub model: Option<String>,
}

impl ClaudeUsage {
    /// Returns the total number of tokens (input + output).
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }

    /// Accumulates token counts from another ClaudeUsage instance.
    ///
    /// This is useful for aggregating usage across multiple Claude calls
    /// within a single phase or story.
    pub fn add(&mut self, other: &ClaudeUsage) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_read_tokens += other.cache_read_tokens;
        self.cache_creation_tokens += other.cache_creation_tokens;
        self.thinking_tokens += other.thinking_tokens;
        // For model, keep the existing value if set, otherwise take the other's value
        if self.model.is_none() {
            self.model = other.model.clone();
        }
    }
}

/// Captures detailed error information from Claude process failures.
#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeErrorInfo {
    /// Human-readable error message
    pub message: String,
    /// Exit code from the subprocess, if available
    pub exit_code: Option<i32>,
    /// Stderr output from the subprocess, if available
    pub stderr: Option<String>,
}

impl ClaudeErrorInfo {
    /// Create a new error info with just a message
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit_code: None,
            stderr: None,
        }
    }

    /// Create error info from a process exit status and stderr
    pub fn from_process_failure(status: std::process::ExitStatus, stderr: Option<String>) -> Self {
        let exit_code = status.code();
        let stderr_trimmed = stderr.as_ref().map(|s| s.trim().to_string());

        let message = match (&stderr_trimmed, exit_code) {
            (Some(err), Some(code)) if !err.is_empty() => {
                format!("Claude exited with status {}: {}", code, err)
            }
            (Some(err), None) if !err.is_empty() => {
                format!("Claude exited with error: {}", err)
            }
            (_, Some(code)) => {
                format!("Claude exited with status: {}", code)
            }
            (_, None) => {
                format!("Claude exited with status: {}", status)
            }
        };

        Self {
            message,
            exit_code,
            stderr: stderr_trimmed.filter(|s| !s.is_empty()),
        }
    }
}

impl std::fmt::Display for ClaudeErrorInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl From<ClaudeErrorInfo> for String {
    fn from(info: ClaudeErrorInfo) -> Self {
        info.message
    }
}

/// Result from running Claude on a story task
#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeStoryResult {
    pub outcome: ClaudeOutcome,
    /// Extracted work summary from Claude's output, if present
    pub work_summary: Option<String>,
    /// Full accumulated text output from Claude, used for knowledge extraction
    pub full_output: String,
    /// Token usage data from the Claude API response
    pub usage: Option<ClaudeUsage>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClaudeOutcome {
    IterationComplete,
    AllStoriesComplete,
    Error(ClaudeErrorInfo),
}

/// Legacy enum for backwards compatibility - use ClaudeStoryResult for new code
#[derive(Debug, Clone, PartialEq)]
pub enum ClaudeResult {
    IterationComplete,
    AllStoriesComplete,
    Error(ClaudeErrorInfo),
}

#[cfg(test)]
mod tests {
    use super::*;

    // ClaudeUsage tests

    #[test]
    fn test_claude_usage_default() {
        let usage = ClaudeUsage::default();
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
        assert_eq!(usage.cache_read_tokens, 0);
        assert_eq!(usage.cache_creation_tokens, 0);
        assert_eq!(usage.thinking_tokens, 0);
        assert_eq!(usage.model, None);
    }

    #[test]
    fn test_claude_usage_total_tokens() {
        let usage = ClaudeUsage {
            input_tokens: 100,
            output_tokens: 50,
            ..Default::default()
        };
        assert_eq!(usage.total_tokens(), 150);
    }

    #[test]
    fn test_claude_usage_total_tokens_zero() {
        let usage = ClaudeUsage::default();
        assert_eq!(usage.total_tokens(), 0);
    }

    #[test]
    fn test_claude_usage_add_basic() {
        let mut usage1 = ClaudeUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 25,
            cache_creation_tokens: 10,
            thinking_tokens: 5,
            model: None,
        };
        let usage2 = ClaudeUsage {
            input_tokens: 200,
            output_tokens: 100,
            cache_read_tokens: 50,
            cache_creation_tokens: 20,
            thinking_tokens: 10,
            model: Some("claude-sonnet-4-20250514".to_string()),
        };

        usage1.add(&usage2);

        assert_eq!(usage1.input_tokens, 300);
        assert_eq!(usage1.output_tokens, 150);
        assert_eq!(usage1.cache_read_tokens, 75);
        assert_eq!(usage1.cache_creation_tokens, 30);
        assert_eq!(usage1.thinking_tokens, 15);
        assert_eq!(usage1.model, Some("claude-sonnet-4-20250514".to_string()));
    }

    #[test]
    fn test_claude_usage_add_preserves_existing_model() {
        let mut usage1 = ClaudeUsage {
            model: Some("existing-model".to_string()),
            ..Default::default()
        };
        let usage2 = ClaudeUsage {
            model: Some("other-model".to_string()),
            ..Default::default()
        };

        usage1.add(&usage2);

        // Should preserve the existing model
        assert_eq!(usage1.model, Some("existing-model".to_string()));
    }

    #[test]
    fn test_claude_usage_add_takes_model_when_none() {
        let mut usage1 = ClaudeUsage::default();
        let usage2 = ClaudeUsage {
            model: Some("new-model".to_string()),
            ..Default::default()
        };

        usage1.add(&usage2);

        assert_eq!(usage1.model, Some("new-model".to_string()));
    }

    #[test]
    fn test_claude_usage_clone() {
        let usage = ClaudeUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 25,
            cache_creation_tokens: 10,
            thinking_tokens: 5,
            model: Some("test-model".to_string()),
        };
        let cloned = usage.clone();
        assert_eq!(usage.input_tokens, cloned.input_tokens);
        assert_eq!(usage.output_tokens, cloned.output_tokens);
        assert_eq!(usage.cache_read_tokens, cloned.cache_read_tokens);
        assert_eq!(usage.cache_creation_tokens, cloned.cache_creation_tokens);
        assert_eq!(usage.thinking_tokens, cloned.thinking_tokens);
        assert_eq!(usage.model, cloned.model);
    }

    #[test]
    fn test_claude_usage_serialize_deserialize() {
        let usage = ClaudeUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 25,
            cache_creation_tokens: 10,
            thinking_tokens: 5,
            model: Some("test-model".to_string()),
        };

        let json = serde_json::to_string(&usage).unwrap();
        let deserialized: ClaudeUsage = serde_json::from_str(&json).unwrap();

        assert_eq!(usage.input_tokens, deserialized.input_tokens);
        assert_eq!(usage.output_tokens, deserialized.output_tokens);
        assert_eq!(usage.cache_read_tokens, deserialized.cache_read_tokens);
        assert_eq!(
            usage.cache_creation_tokens,
            deserialized.cache_creation_tokens
        );
        assert_eq!(usage.thinking_tokens, deserialized.thinking_tokens);
        assert_eq!(usage.model, deserialized.model);
    }

    #[test]
    fn test_claude_usage_deserialize_partial() {
        // Test backward compatibility - missing fields should default to 0/None
        let json = r#"{"inputTokens": 100, "outputTokens": 50}"#;
        let usage: ClaudeUsage = serde_json::from_str(json).unwrap();

        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.cache_read_tokens, 0);
        assert_eq!(usage.cache_creation_tokens, 0);
        assert_eq!(usage.thinking_tokens, 0);
        assert_eq!(usage.model, None);
    }

    #[test]
    fn test_claude_usage_deserialize_empty() {
        // Test that an empty object deserializes with defaults
        let json = r#"{}"#;
        let usage: ClaudeUsage = serde_json::from_str(json).unwrap();

        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
        assert_eq!(usage.cache_read_tokens, 0);
        assert_eq!(usage.cache_creation_tokens, 0);
        assert_eq!(usage.thinking_tokens, 0);
        assert_eq!(usage.model, None);
    }

    // ClaudeErrorInfo tests

    #[test]
    fn test_claude_error_info_new() {
        let info = ClaudeErrorInfo::new("test error message");
        assert_eq!(info.message, "test error message");
        assert_eq!(info.exit_code, None);
        assert_eq!(info.stderr, None);
    }

    #[test]
    fn test_claude_error_info_display() {
        let info = ClaudeErrorInfo::new("test error");
        assert_eq!(format!("{}", info), "test error");
    }

    #[test]
    fn test_claude_error_info_into_string() {
        let info = ClaudeErrorInfo::new("convertible error");
        let s: String = info.into();
        assert_eq!(s, "convertible error");
    }

    #[test]
    fn test_claude_error_info_clone() {
        let info = ClaudeErrorInfo {
            message: "cloned error".to_string(),
            exit_code: Some(42),
            stderr: Some("stderr content".to_string()),
        };
        let cloned = info.clone();
        assert_eq!(info, cloned);
    }

    #[test]
    fn test_claude_error_info_equality() {
        let info1 = ClaudeErrorInfo::new("error");
        let info2 = ClaudeErrorInfo::new("error");
        let info3 = ClaudeErrorInfo::new("different");
        assert_eq!(info1, info2);
        assert_ne!(info1, info3);
    }
}
