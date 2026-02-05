//! Interactive Claude session for the improve command.
//!
//! This module provides functions for spawning an interactive Claude session
//! with context from previous autom8 runs. Unlike other Claude modules that
//! capture output, this module hands off control to an interactive session.

use std::process::{Command, Stdio};

use crate::error::{Autom8Error, Result};

/// Result of an interactive improve session.
#[derive(Debug)]
pub struct ImproveSessionResult {
    /// Whether the session completed successfully
    pub success: bool,
    /// Exit code if available
    pub exit_code: Option<i32>,
}

/// Spawn an interactive Claude session with the given prompt.
///
/// This function hands off control to Claude for an interactive session.
/// The prompt is passed as the first argument, and all I/O is inherited
/// from the parent process.
///
/// # Arguments
/// * `prompt` - The context prompt to inject into the Claude session
///
/// # Returns
/// * `Ok(ImproveSessionResult)` - Session completed (check `success` for exit status)
/// * `Err` - Failed to spawn Claude (e.g., not installed)
pub fn run_improve_session(prompt: &str) -> Result<ImproveSessionResult> {
    let status = Command::new("claude")
        .arg(prompt)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Autom8Error::ClaudeNotFound
            } else {
                Autom8Error::ClaudeSpawnError(e.to_string())
            }
        })?;

    Ok(ImproveSessionResult {
        success: status.success(),
        exit_code: status.code(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_improve_session_result_debug() {
        let result = ImproveSessionResult {
            success: true,
            exit_code: Some(0),
        };
        let debug = format!("{:?}", result);
        assert!(debug.contains("ImproveSessionResult"));
        assert!(debug.contains("success: true"));
    }

    #[test]
    fn test_improve_session_result_success_false() {
        let result = ImproveSessionResult {
            success: false,
            exit_code: Some(1),
        };
        assert!(!result.success);
        assert_eq!(result.exit_code, Some(1));
    }

    #[test]
    fn test_improve_session_result_no_exit_code() {
        // When killed by signal, exit_code may be None
        let result = ImproveSessionResult {
            success: false,
            exit_code: None,
        };
        assert!(!result.success);
        assert!(result.exit_code.is_none());
    }
}
