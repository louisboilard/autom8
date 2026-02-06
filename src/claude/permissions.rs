//! Phase-aware permission configuration for Claude CLI.
//!
//! Different phases of autom8's workflow need different permissions:
//! - Story implementation: block only `git push` (safety net against accidental pushes)
//! - Review/Correction: same as story implementation (read-heavy, no special needs)
//! - Commit: allow `git add`, `git commit`
//! - PR creation: allow `git push`, `gh pr *`

/// Represents the different phases where Claude is invoked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudePhase {
    /// Story implementation phase (`RunningClaude` state).
    /// Block only `git push` to prevent accidental pushes to remote.
    StoryImplementation,

    /// Review phase (`Reviewing` state).
    /// Same as story implementation - read-heavy, no special needs.
    Review,

    /// Correction phase (fixing issues found by reviewer).
    /// Same as story implementation.
    Correction,

    /// Commit phase (`Committing` state).
    /// Allows `git add` and `git commit` operations.
    Commit,

    /// PR creation phase (`CreatingPR` state).
    /// Allows `git push` and `gh pr *` operations.
    PullRequest,
}

/// Builds Claude CLI permission arguments based on the phase.
///
/// Returns a vector of CLI arguments to be passed to the Claude subprocess.
/// All phases use `--permission-mode acceptEdits` to auto-allow file operations
/// and `--allowedTools Bash` to allow most Bash commands.
///
/// Phase-specific behavior:
/// - `StoryImplementation`, `Review`, `Correction`: block `git push *`
/// - `Commit`: allow `git add`, `git commit` (no `git push` block)
/// - `PullRequest`: allow `git push`, `gh pr *` (no restrictions)
///
/// When `all_permissions` is true, returns `--dangerously-skip-permissions` instead,
/// bypassing all permission checks (useful for CI/CD or fully trusted environments).
pub fn build_permission_args(phase: ClaudePhase, all_permissions: bool) -> Vec<&'static str> {
    // When all_permissions is enabled, bypass all permission checks
    if all_permissions {
        return vec!["--dangerously-skip-permissions"];
    }

    match phase {
        ClaudePhase::StoryImplementation | ClaudePhase::Review | ClaudePhase::Correction => {
            // Block git push during story implementation, review, and correction
            vec![
                "--permission-mode",
                "acceptEdits",
                "--allowedTools",
                "Bash",
                "--disallowedTools",
                "Bash(git push *)",
            ]
        }
        ClaudePhase::Commit => {
            // Commit phase needs git add and git commit, no push
            // Still block git push for safety
            vec![
                "--permission-mode",
                "acceptEdits",
                "--allowedTools",
                "Bash",
                "--disallowedTools",
                "Bash(git push *)",
            ]
        }
        ClaudePhase::PullRequest => {
            // PR phase needs git push and gh commands - no restrictions
            vec!["--permission-mode", "acceptEdits", "--allowedTools", "Bash"]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_story_implementation_blocks_git_push() {
        let args = build_permission_args(ClaudePhase::StoryImplementation, false);
        assert!(args.contains(&"--permission-mode"));
        assert!(args.contains(&"acceptEdits"));
        assert!(args.contains(&"--allowedTools"));
        assert!(args.contains(&"Bash"));
        assert!(args.contains(&"--disallowedTools"));
        assert!(args.contains(&"Bash(git push *)"));
    }

    #[test]
    fn test_review_phase_same_as_story_implementation() {
        let story_args = build_permission_args(ClaudePhase::StoryImplementation, false);
        let review_args = build_permission_args(ClaudePhase::Review, false);
        assert_eq!(story_args, review_args);
    }

    #[test]
    fn test_correction_phase_same_as_story_implementation() {
        let story_args = build_permission_args(ClaudePhase::StoryImplementation, false);
        let correction_args = build_permission_args(ClaudePhase::Correction, false);
        assert_eq!(story_args, correction_args);
    }

    #[test]
    fn test_commit_phase_blocks_git_push() {
        let args = build_permission_args(ClaudePhase::Commit, false);
        assert!(args.contains(&"--permission-mode"));
        assert!(args.contains(&"acceptEdits"));
        assert!(args.contains(&"--allowedTools"));
        assert!(args.contains(&"Bash"));
        assert!(args.contains(&"--disallowedTools"));
        assert!(args.contains(&"Bash(git push *)"));
    }

    #[test]
    fn test_pr_phase_allows_all_bash_commands() {
        let args = build_permission_args(ClaudePhase::PullRequest, false);
        assert!(args.contains(&"--permission-mode"));
        assert!(args.contains(&"acceptEdits"));
        assert!(args.contains(&"--allowedTools"));
        assert!(args.contains(&"Bash"));
        // PR phase should NOT have disallowedTools
        assert!(!args.contains(&"--disallowedTools"));
    }

    #[test]
    fn test_all_phases_use_accept_edits_mode() {
        for phase in [
            ClaudePhase::StoryImplementation,
            ClaudePhase::Review,
            ClaudePhase::Correction,
            ClaudePhase::Commit,
            ClaudePhase::PullRequest,
        ] {
            let args = build_permission_args(phase, false);
            assert!(
                args.contains(&"--permission-mode"),
                "Phase {:?} missing --permission-mode",
                phase
            );
            assert!(
                args.contains(&"acceptEdits"),
                "Phase {:?} missing acceptEdits",
                phase
            );
        }
    }

    #[test]
    fn test_all_phases_allow_bash() {
        for phase in [
            ClaudePhase::StoryImplementation,
            ClaudePhase::Review,
            ClaudePhase::Correction,
            ClaudePhase::Commit,
            ClaudePhase::PullRequest,
        ] {
            let args = build_permission_args(phase, false);
            assert!(
                args.contains(&"--allowedTools"),
                "Phase {:?} missing --allowedTools",
                phase
            );
            assert!(
                args.contains(&"Bash"),
                "Phase {:?} missing Bash in allowedTools",
                phase
            );
        }
    }

    #[test]
    fn test_all_permissions_uses_dangerously_skip() {
        // When all_permissions is true, should use --dangerously-skip-permissions
        for phase in [
            ClaudePhase::StoryImplementation,
            ClaudePhase::Review,
            ClaudePhase::Correction,
            ClaudePhase::Commit,
            ClaudePhase::PullRequest,
        ] {
            let args = build_permission_args(phase, true);
            assert_eq!(
                args,
                vec!["--dangerously-skip-permissions"],
                "Phase {:?} should use --dangerously-skip-permissions when all_permissions=true",
                phase
            );
        }
    }

    #[test]
    fn test_all_permissions_false_uses_phase_aware_permissions() {
        // When all_permissions is false, should NOT use --dangerously-skip-permissions
        let args = build_permission_args(ClaudePhase::StoryImplementation, false);
        assert!(
            !args.contains(&"--dangerously-skip-permissions"),
            "Should not use --dangerously-skip-permissions when all_permissions=false"
        );
        assert!(
            args.contains(&"--permission-mode"),
            "Should use phase-aware permissions when all_permissions=false"
        );
    }
}
