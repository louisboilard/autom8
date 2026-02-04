//! Core types for Claude operations.
//!
//! Defines error types, result enums, and outcome structures used
//! throughout the Claude integration.

use chrono::{DateTime, Utc};

/// Holds metadata about a running Claude process.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessInfo {
    /// Process ID of the spawned Claude CLI subprocess
    pub pid: u32,
    /// Timestamp when the process was spawned
    pub spawn_time: DateTime<Utc>,
}

impl ProcessInfo {
    /// Creates a new ProcessInfo with the given PID and current timestamp.
    pub fn new(pid: u32) -> Self {
        Self {
            pid,
            spawn_time: Utc::now(),
        }
    }

    /// Creates a new ProcessInfo with a specific timestamp (useful for testing).
    pub fn with_timestamp(pid: u32, spawn_time: DateTime<Utc>) -> Self {
        Self { pid, spawn_time }
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

    #[test]
    fn test_process_info_new() {
        let pid = 12345;
        let before = Utc::now();
        let info = ProcessInfo::new(pid);
        let after = Utc::now();

        assert_eq!(info.pid, pid);
        assert!(info.spawn_time >= before);
        assert!(info.spawn_time <= after);
    }

    #[test]
    fn test_process_info_with_timestamp() {
        let pid = 54321;
        let timestamp = Utc::now();
        let info = ProcessInfo::with_timestamp(pid, timestamp);

        assert_eq!(info.pid, pid);
        assert_eq!(info.spawn_time, timestamp);
    }

    #[test]
    fn test_process_info_clone() {
        let info = ProcessInfo::new(99999);
        let cloned = info.clone();

        assert_eq!(info.pid, cloned.pid);
        assert_eq!(info.spawn_time, cloned.spawn_time);
    }

    #[test]
    fn test_process_info_equality() {
        let timestamp = Utc::now();
        let info1 = ProcessInfo::with_timestamp(100, timestamp);
        let info2 = ProcessInfo::with_timestamp(100, timestamp);
        let info3 = ProcessInfo::with_timestamp(200, timestamp);

        assert_eq!(info1, info2);
        assert_ne!(info1, info3);
    }

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
