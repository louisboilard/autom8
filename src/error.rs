use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Autom8Error {
    #[error("Spec file not found: {0}\n\nThe spec file does not exist at the specified path.\n\nTo fix this:\n  1. Check that the path is correct\n  2. Run 'autom8 init' to create the project structure\n  3. Create a spec file or use 'autom8' to generate one interactively")]
    SpecNotFound(PathBuf),

    #[error("Invalid spec format: {0}\n\nThe spec file exists but cannot be parsed.\n\nTo fix this:\n  1. Ensure the file is valid JSON or Markdown\n  2. Check for syntax errors (missing commas, brackets, etc.)\n  3. See CLAUDE.md for spec format requirements")]
    InvalidSpec(String),

    #[error("No incomplete stories found in spec\n\nAll user stories in the spec have passes: true.\n\nTo continue:\n  1. Add new user stories to the spec, or\n  2. Set passes: false on stories you want to re-implement")]
    NoIncompleteStories,

    #[error("Claude process failed: {0}")]
    ClaudeError(String),

    #[error("Claude process timed out after {0} seconds")]
    ClaudeTimeout(u64),

    #[error("State file error: {0}")]
    StateError(String),

    #[error("No active run to resume\n\nNo incomplete session was found for this project.\n\nTo start a new run:\n  1. Run 'autom8 spec.json' to start from a spec file, or\n  2. Run 'autom8' to create a new spec interactively, or\n  3. Use 'autom8 status --all' to check all sessions")]
    NoActiveRun,

    #[error("Run already in progress: {0}\n\nAnother session is actively running for this project.\n\nTo resolve this:\n  1. Wait for the current run to complete, or\n  2. Use --worktree to run in a separate worktree, or\n  3. Use 'autom8 status --all' to see all sessions")]
    RunInProgress(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Git error: {0}")]
    GitError(String),

    #[error("Spec markdown file not found: {0}")]
    SpecMarkdownNotFound(PathBuf),

    #[error("Spec file is empty")]
    EmptySpec,

    #[error("Spec generation failed: {0}")]
    SpecGenerationFailed(String),

    #[error("Invalid generated spec: {0}")]
    InvalidGeneratedSpec(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Review failed after 3 iterations. Please manually review autom8_review.md for remaining issues.")]
    MaxReviewIterationsReached,

    #[error("No incomplete specs found in spec/\n\nNo spec files with incomplete user stories were found.\n\nTo start a new run:\n  1. Run 'autom8' to create a new spec interactively, or\n  2. Add a spec file to ~/.config/autom8/<project>/spec/, or\n  3. Set passes: false on stories you want to re-implement")]
    NoSpecsToResume,

    #[error("Shell completion error: {0}")]
    ShellCompletion(String),

    #[error("Worktree error: {0}")]
    WorktreeError(String),

    #[error("Branch conflict: branch '{branch}' is already in use by session '{session_id}' at {worktree_path}.\n\nThe branch is checked out in another worktree session.\n\nTo resolve this:\n  1. Wait for the other session to complete, or\n  2. Use a different branch name in your spec, or\n  3. Resume the existing session: autom8 resume --session {session_id}, or\n  4. Clean up the conflicting session: autom8 clean --session {session_id}")]
    BranchConflict {
        branch: String,
        session_id: String,
        worktree_path: std::path::PathBuf,
    },

    #[error("Signal handler error: {0}")]
    SignalHandler(String),

    #[error("Interrupted by user")]
    Interrupted,
}

pub type Result<T> = std::result::Result<T, Autom8Error>;

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // Error message format tests (US-012)
    // ========================================================================

    #[test]
    fn test_us012_branch_conflict_error_includes_what_happened() {
        let err = Autom8Error::BranchConflict {
            branch: "feature/test".to_string(),
            session_id: "abc123".to_string(),
            worktree_path: PathBuf::from("/path/to/worktree"),
        };
        let msg = err.to_string();

        // What happened
        assert!(
            msg.contains("Branch conflict"),
            "Error should describe what happened"
        );
        assert!(
            msg.contains("feature/test"),
            "Error should include branch name"
        );
    }

    #[test]
    fn test_us012_branch_conflict_error_includes_why() {
        let err = Autom8Error::BranchConflict {
            branch: "feature/test".to_string(),
            session_id: "abc123".to_string(),
            worktree_path: PathBuf::from("/path/to/worktree"),
        };
        let msg = err.to_string();

        // Why it happened
        assert!(
            msg.contains("already in use") || msg.contains("checked out"),
            "Error should explain why"
        );
    }

    #[test]
    fn test_us012_branch_conflict_error_includes_how_to_fix() {
        let err = Autom8Error::BranchConflict {
            branch: "feature/test".to_string(),
            session_id: "abc123".to_string(),
            worktree_path: PathBuf::from("/path/to/worktree"),
        };
        let msg = err.to_string();

        // How to fix - multiple options
        assert!(
            msg.contains("To resolve"),
            "Error should include resolution steps"
        );
        assert!(
            msg.contains("autom8 resume"),
            "Error should suggest resume command"
        );
        assert!(
            msg.contains("autom8 clean"),
            "Error should suggest clean command"
        );
        assert!(
            msg.contains("abc123"),
            "Error should include session ID for commands"
        );
    }

    #[test]
    fn test_us012_spec_not_found_error_includes_fix() {
        let err = Autom8Error::SpecNotFound(PathBuf::from("/missing/spec.json"));
        let msg = err.to_string();

        assert!(
            msg.contains("not found"),
            "Error should describe what happened"
        );
        assert!(msg.contains("To fix"), "Error should include fix steps");
        assert!(
            msg.contains("autom8 init") || msg.contains("init"),
            "Error should suggest init command"
        );
    }

    #[test]
    fn test_us012_no_active_run_error_includes_fix() {
        let err = Autom8Error::NoActiveRun;
        let msg = err.to_string();

        assert!(
            msg.contains("No active run"),
            "Error should describe what happened"
        );
        assert!(
            msg.contains("To start") || msg.contains("To fix"),
            "Error should include fix steps"
        );
        assert!(
            msg.contains("autom8 status"),
            "Error should suggest status command"
        );
    }

    #[test]
    fn test_us012_run_in_progress_error_includes_fix() {
        let err = Autom8Error::RunInProgress("session123".to_string());
        let msg = err.to_string();

        assert!(
            msg.contains("in progress"),
            "Error should describe what happened"
        );
        assert!(msg.contains("To resolve"), "Error should include fix steps");
        assert!(
            msg.contains("--worktree"),
            "Error should suggest worktree option"
        );
    }

    #[test]
    fn test_us012_no_incomplete_stories_error_includes_fix() {
        let err = Autom8Error::NoIncompleteStories;
        let msg = err.to_string();

        assert!(
            msg.contains("No incomplete stories"),
            "Error should describe what happened"
        );
        assert!(
            msg.contains("To continue"),
            "Error should include fix steps"
        );
        assert!(
            msg.contains("passes: false"),
            "Error should suggest how to re-run stories"
        );
    }

    #[test]
    fn test_us012_no_specs_to_resume_error_includes_fix() {
        let err = Autom8Error::NoSpecsToResume;
        let msg = err.to_string();

        assert!(
            msg.contains("No incomplete specs"),
            "Error should describe what happened"
        );
        assert!(msg.contains("To start"), "Error should include fix steps");
    }

    #[test]
    fn test_us012_invalid_spec_error_includes_fix() {
        let err = Autom8Error::InvalidSpec("missing field 'id'".to_string());
        let msg = err.to_string();

        assert!(
            msg.contains("Invalid spec format"),
            "Error should describe what happened"
        );
        assert!(msg.contains("To fix"), "Error should include fix steps");
        assert!(
            msg.contains("JSON") || msg.contains("syntax"),
            "Error should mention format"
        );
    }
}
