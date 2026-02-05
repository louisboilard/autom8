//! Display functions for the improve command.
//!
//! Provides formatted output for showing loaded context before spawning Claude.

use crate::commands::FollowUpContext;
use crate::git::CommitInfo;

use super::colors::*;

/// Print the "CONTEXT LOADED" banner.
///
/// Shows a prominent banner indicating context has been gathered.
pub fn print_context_banner() {
    println!();
    println!("{CYAN}{BOLD}━━━ CONTEXT LOADED ━━━{RESET}");
    println!();
}

/// Print the branch name.
///
/// # Arguments
/// * `branch_name` - The current git branch name
pub fn print_branch_info(branch_name: &str) {
    println!("{BLUE}Branch:{RESET}  {BOLD}{}{RESET}", branch_name);
}

/// Print git history summary.
///
/// Shows "{N} commits" with an abbreviated list of recent commits.
///
/// # Arguments
/// * `commits` - List of commits on this branch
/// * `max_display` - Maximum number of commits to display individually
pub fn print_git_history_summary(commits: &[CommitInfo], max_display: usize) {
    let count = commits.len();
    let commit_word = if count == 1 { "commit" } else { "commits" };
    println!(
        "{BLUE}History:{RESET} {BOLD}{} {}{RESET}",
        count, commit_word
    );

    if !commits.is_empty() {
        let display_count = commits.len().min(max_display);
        for commit in commits.iter().take(display_count) {
            // Truncate message to fit on one line
            let max_msg_len = 45;
            let display_msg = if commit.message.len() > max_msg_len {
                format!("{}...", &commit.message[..max_msg_len - 3])
            } else {
                commit.message.clone()
            };
            println!(
                "         {GRAY}{}{RESET} {}",
                commit.short_hash, display_msg
            );
        }

        if commits.len() > max_display {
            println!(
                "         {GRAY}... and {} more{RESET}",
                commits.len() - max_display
            );
        }
    }
}

/// Print files changed summary.
///
/// Shows "{N} files, +{adds} -{dels}" format.
///
/// # Arguments
/// * `file_count` - Number of files changed
/// * `additions` - Total lines added
/// * `deletions` - Total lines deleted
pub fn print_files_changed_summary(file_count: usize, additions: u32, deletions: u32) {
    let file_word = if file_count == 1 { "file" } else { "files" };
    println!(
        "{BLUE}Changed:{RESET} {}{} {}{RESET}, {GREEN}+{}{RESET} {RED}-{}{RESET}",
        BOLD, file_count, file_word, additions, deletions
    );
}

/// Print spec info if loaded.
///
/// Shows the spec filename and story count.
///
/// # Arguments
/// * `filename` - The spec file name (not full path)
/// * `completed` - Number of completed stories
/// * `total` - Total number of stories
pub fn print_spec_info(filename: &str, completed: usize, total: usize) {
    let status = if completed == total {
        format!("{GREEN}all complete{RESET}")
    } else {
        format!("{}/{}", completed, total)
    };
    println!("{BLUE}Spec:{RESET}    {} ({})", filename, status);
}

/// Print session knowledge info if loaded.
///
/// Shows decision count and pattern count.
///
/// # Arguments
/// * `decision_count` - Number of decisions recorded
/// * `pattern_count` - Number of patterns recorded
pub fn print_session_knowledge_info(decision_count: usize, pattern_count: usize) {
    let decision_word = if decision_count == 1 {
        "decision"
    } else {
        "decisions"
    };
    let pattern_word = if pattern_count == 1 {
        "pattern"
    } else {
        "patterns"
    };
    println!(
        "{BLUE}Session:{RESET} {} {}, {} {}",
        decision_count, decision_word, pattern_count, pattern_word
    );
}

/// Print "Spawning Claude with context..." message.
///
/// Called just before handing off to the interactive Claude session.
pub fn print_spawning_claude() {
    println!();
    println!("{CYAN}Spawning Claude with context...{RESET}");
    println!();
}

// ============================================================================
// US-009: Edge Case Display Functions
// ============================================================================

/// Print a warning when on main/master branch.
///
/// This warns the user that no feature-specific context is available
/// but still allows the command to proceed.
pub fn print_main_branch_warning() {
    println!(
        "{YELLOW}Note:{RESET} You're on the main branch. No feature context is available, \
but you can still work with Claude."
    );
    println!();
}

/// Print a message when only git context is available.
///
/// This is shown when no session and no spec were found for the branch.
pub fn print_git_only_context() {
    println!("{GRAY}No spec or session found for this branch. Using git context only.{RESET}");
}

/// Print "no commits yet" indicator for branches with no commits vs base.
///
/// This is shown instead of the commit list when a branch has no commits.
pub fn print_no_commits_yet() {
    println!("{BLUE}History:{RESET} {GRAY}no commits yet{RESET}");
}

/// Print a complete context summary for the improve command.
///
/// This is the main entry point for displaying the context summary.
/// It shows:
/// - CONTEXT LOADED banner
/// - Branch name
/// - Warning if on main/master branch
/// - Git history summary (or "no commits yet")
/// - Files changed summary
/// - Spec info (if loaded)
/// - Session knowledge (if loaded)
/// - Message if only git context available
///
/// # Arguments
/// * `context` - The follow-up context containing all loaded information
pub fn print_context_summary(context: &FollowUpContext) {
    print_context_banner();

    // Always show branch
    print_branch_info(&context.git.branch_name);

    // Show warning if on main/master branch
    if !context.git.is_feature_branch() {
        print_main_branch_warning();
    }

    // Show git history - use special message for "no commits yet"
    if context.git.commits.is_empty() && context.git.is_feature_branch() {
        // Feature branch with no commits yet
        print_no_commits_yet();
    } else {
        // Normal case: show commit history (may be empty for main branch)
        print_git_history_summary(&context.git.commits, 3);
    }

    // Always show files changed (may be zeros)
    print_files_changed_summary(
        context.git.files_changed_count(),
        context.git.total_additions(),
        context.git.total_deletions(),
    );

    // Show spec info if loaded
    if let Some(ref spec) = context.spec {
        let filename = context
            .spec_path
            .as_ref()
            .and_then(|p: &std::path::PathBuf| p.file_name())
            .map(|n: &std::ffi::OsStr| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "spec.json".to_string());

        let (completed, total): (usize, usize) = spec.progress();
        print_spec_info(&filename, completed, total);
    }

    // Show session knowledge if loaded
    if let Some(ref knowledge) = context.knowledge {
        print_session_knowledge_info(knowledge.decisions.len(), knowledge.patterns.len());
    }

    // Show message if only git context is available (no spec and no session)
    if context.spec.is_none() && context.knowledge.is_none() && context.git.is_feature_branch() {
        print_git_only_context();
    }

    print_spawning_claude();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::GitContext;
    use crate::git::{DiffEntry, DiffStatus};
    use crate::knowledge::{Decision, Pattern, ProjectKnowledge};
    use crate::spec::{Spec, UserStory};
    use std::path::PathBuf;

    fn make_commit(hash: &str, message: &str) -> CommitInfo {
        CommitInfo {
            short_hash: hash.to_string(),
            full_hash: format!("{}1234567890abcdef", hash),
            message: message.to_string(),
            author: "Test Author".to_string(),
            date: "2024-01-15".to_string(),
        }
    }

    fn make_git_context() -> GitContext {
        GitContext {
            branch_name: "feature/improve-command".to_string(),
            base_branch: "main".to_string(),
            commits: vec![
                make_commit("abc1234", "Add context loading"),
                make_commit("def5678", "Implement display functions"),
                make_commit("ghi9012", "Fix edge cases"),
            ],
            diff_entries: vec![
                DiffEntry {
                    path: PathBuf::from("src/commands/improve.rs"),
                    additions: 150,
                    deletions: 10,
                    status: DiffStatus::Added,
                },
                DiffEntry {
                    path: PathBuf::from("src/output/improve.rs"),
                    additions: 80,
                    deletions: 0,
                    status: DiffStatus::Added,
                },
            ],
            merge_base_commit: Some("basehash".to_string()),
        }
    }

    fn make_spec() -> Spec {
        Spec {
            project: "TestProject".to_string(),
            branch_name: "feature/test".to_string(),
            description: "Test description".to_string(),
            user_stories: vec![
                UserStory {
                    id: "US-001".to_string(),
                    title: "Story 1".to_string(),
                    description: "Test".to_string(),
                    acceptance_criteria: vec![],
                    priority: 1,
                    passes: true,
                    notes: String::new(),
                },
                UserStory {
                    id: "US-002".to_string(),
                    title: "Story 2".to_string(),
                    description: "Test 2".to_string(),
                    acceptance_criteria: vec![],
                    priority: 2,
                    passes: false,
                    notes: String::new(),
                },
            ],
        }
    }

    fn make_knowledge() -> ProjectKnowledge {
        let mut knowledge = ProjectKnowledge::default();
        knowledge.decisions.push(Decision {
            story_id: "US-001".to_string(),
            topic: "Architecture".to_string(),
            choice: "Use modules".to_string(),
            rationale: "Better organization".to_string(),
        });
        knowledge.decisions.push(Decision {
            story_id: "US-002".to_string(),
            topic: "Error handling".to_string(),
            choice: "Use thiserror".to_string(),
            rationale: "Clean error types".to_string(),
        });
        knowledge.patterns.push(Pattern {
            story_id: "US-001".to_string(),
            description: "Use Result for errors".to_string(),
            example_file: None,
        });
        knowledge
    }

    // ========================================================================
    // Unit tests for individual display functions
    // ========================================================================

    #[test]
    fn test_print_context_banner() {
        // Should not panic
        print_context_banner();
    }

    #[test]
    fn test_print_branch_info() {
        // Should not panic
        print_branch_info("feature/test-branch");
    }

    #[test]
    fn test_print_git_history_summary_empty() {
        // Empty commits should not panic
        print_git_history_summary(&[], 3);
    }

    #[test]
    fn test_print_git_history_summary_few_commits() {
        let commits = vec![
            make_commit("abc1234", "First commit"),
            make_commit("def5678", "Second commit"),
        ];
        print_git_history_summary(&commits, 5);
    }

    #[test]
    fn test_print_git_history_summary_many_commits() {
        let commits = vec![
            make_commit("abc1234", "Commit 1"),
            make_commit("def5678", "Commit 2"),
            make_commit("ghi9012", "Commit 3"),
            make_commit("jkl3456", "Commit 4"),
            make_commit("mno7890", "Commit 5"),
        ];
        // Should only show 3 and indicate more
        print_git_history_summary(&commits, 3);
    }

    #[test]
    fn test_print_git_history_summary_long_message() {
        let commits = vec![make_commit(
            "abc1234",
            "This is a very long commit message that exceeds the maximum display length and should be truncated",
        )];
        print_git_history_summary(&commits, 3);
    }

    #[test]
    fn test_print_git_history_summary_single_commit() {
        let commits = vec![make_commit("abc1234", "Single commit")];
        // "commit" should be singular
        print_git_history_summary(&commits, 3);
    }

    #[test]
    fn test_print_files_changed_summary_zero() {
        print_files_changed_summary(0, 0, 0);
    }

    #[test]
    fn test_print_files_changed_summary_single_file() {
        // "file" should be singular
        print_files_changed_summary(1, 50, 10);
    }

    #[test]
    fn test_print_files_changed_summary_multiple_files() {
        print_files_changed_summary(5, 200, 50);
    }

    #[test]
    fn test_print_spec_info_partial() {
        print_spec_info("spec-feature.json", 3, 5);
    }

    #[test]
    fn test_print_spec_info_all_complete() {
        print_spec_info("spec-feature.json", 5, 5);
    }

    #[test]
    fn test_print_spec_info_none_complete() {
        print_spec_info("spec-feature.json", 0, 3);
    }

    #[test]
    fn test_print_session_knowledge_info_empty() {
        print_session_knowledge_info(0, 0);
    }

    #[test]
    fn test_print_session_knowledge_info_singular() {
        // "decision" and "pattern" should be singular
        print_session_knowledge_info(1, 1);
    }

    #[test]
    fn test_print_session_knowledge_info_plural() {
        print_session_knowledge_info(5, 3);
    }

    #[test]
    fn test_print_spawning_claude() {
        // Should not panic
        print_spawning_claude();
    }

    // ========================================================================
    // Integration tests for print_context_summary
    // ========================================================================

    #[test]
    fn test_print_context_summary_git_only() {
        let context = FollowUpContext {
            git: make_git_context(),
            spec: None,
            spec_path: None,
            knowledge: None,
            work_summaries: vec![],
            session_id: None,
        };

        // Should not panic and show git info only
        print_context_summary(&context);
    }

    #[test]
    fn test_print_context_summary_with_spec() {
        let context = FollowUpContext {
            git: make_git_context(),
            spec: Some(make_spec()),
            spec_path: Some(PathBuf::from("/path/to/spec-test.json")),
            knowledge: None,
            work_summaries: vec![],
            session_id: None,
        };

        print_context_summary(&context);
    }

    #[test]
    fn test_print_context_summary_with_knowledge() {
        let context = FollowUpContext {
            git: make_git_context(),
            spec: None,
            spec_path: None,
            knowledge: Some(make_knowledge()),
            work_summaries: vec![],
            session_id: None,
        };

        print_context_summary(&context);
    }

    #[test]
    fn test_print_context_summary_full() {
        let context = FollowUpContext {
            git: make_git_context(),
            spec: Some(make_spec()),
            spec_path: Some(PathBuf::from("/config/spec-improve.json")),
            knowledge: Some(make_knowledge()),
            work_summaries: vec![
                "Implemented context loading".to_string(),
                "Added display functions".to_string(),
            ],
            session_id: Some("session-123".to_string()),
        };

        print_context_summary(&context);
    }

    #[test]
    fn test_print_context_summary_spec_without_path() {
        // Should use default filename when path is None
        let context = FollowUpContext {
            git: make_git_context(),
            spec: Some(make_spec()),
            spec_path: None, // No path
            knowledge: None,
            work_summaries: vec![],
            session_id: None,
        };

        print_context_summary(&context);
    }

    #[test]
    fn test_print_context_summary_empty_git() {
        // Git with no commits or changes
        let context = FollowUpContext {
            git: GitContext {
                branch_name: "main".to_string(),
                base_branch: "main".to_string(),
                commits: vec![],
                diff_entries: vec![],
                merge_base_commit: None,
            },
            spec: None,
            spec_path: None,
            knowledge: None,
            work_summaries: vec![],
            session_id: None,
        };

        print_context_summary(&context);
    }

    #[test]
    fn test_print_context_summary_complete_spec() {
        // Spec with all stories complete
        let mut spec = make_spec();
        for story in &mut spec.user_stories {
            story.passes = true;
        }

        let context = FollowUpContext {
            git: make_git_context(),
            spec: Some(spec),
            spec_path: Some(PathBuf::from("/path/to/complete-spec.json")),
            knowledge: None,
            work_summaries: vec![],
            session_id: None,
        };

        print_context_summary(&context);
    }

    // ========================================================================
    // US-009: Edge case display function tests
    // ========================================================================

    #[test]
    fn test_print_main_branch_warning() {
        // Should not panic
        print_main_branch_warning();
    }

    #[test]
    fn test_print_git_only_context() {
        // Should not panic
        print_git_only_context();
    }

    #[test]
    fn test_print_no_commits_yet() {
        // Should not panic
        print_no_commits_yet();
    }

    #[test]
    fn test_print_context_summary_main_branch_shows_warning() {
        // On main branch with no spec/knowledge - should show main branch warning
        let context = FollowUpContext {
            git: GitContext {
                branch_name: "main".to_string(),
                base_branch: "main".to_string(),
                commits: vec![],
                diff_entries: vec![],
                merge_base_commit: None,
            },
            spec: None,
            spec_path: None,
            knowledge: None,
            work_summaries: vec![],
            session_id: None,
        };

        // Should not panic - warning will be printed
        print_context_summary(&context);
    }

    #[test]
    fn test_print_context_summary_master_branch_shows_warning() {
        // On master branch - should show main branch warning
        let context = FollowUpContext {
            git: GitContext {
                branch_name: "master".to_string(),
                base_branch: "master".to_string(),
                commits: vec![],
                diff_entries: vec![],
                merge_base_commit: None,
            },
            spec: None,
            spec_path: None,
            knowledge: None,
            work_summaries: vec![],
            session_id: None,
        };

        // Should not panic - warning will be printed
        print_context_summary(&context);
    }

    #[test]
    fn test_print_context_summary_feature_branch_no_commits() {
        // Feature branch with no commits yet
        let context = FollowUpContext {
            git: GitContext {
                branch_name: "feature/new-feature".to_string(),
                base_branch: "main".to_string(),
                commits: vec![], // No commits yet
                diff_entries: vec![],
                merge_base_commit: None,
            },
            spec: None,
            spec_path: None,
            knowledge: None,
            work_summaries: vec![],
            session_id: None,
        };

        // Should not panic - "no commits yet" and "git only" messages will be printed
        print_context_summary(&context);
    }

    #[test]
    fn test_print_context_summary_feature_branch_git_only() {
        // Feature branch with commits but no spec/session
        let context = FollowUpContext {
            git: make_git_context(), // Has commits
            spec: None,
            spec_path: None,
            knowledge: None,
            work_summaries: vec![],
            session_id: None,
        };

        // Should not panic - "git only" message will be printed
        print_context_summary(&context);
    }

    #[test]
    fn test_print_context_summary_feature_branch_with_spec_no_git_only_message() {
        // Feature branch with spec - should NOT show "git only" message
        let context = FollowUpContext {
            git: make_git_context(),
            spec: Some(make_spec()),
            spec_path: Some(PathBuf::from("/path/to/spec.json")),
            knowledge: None, // No knowledge but has spec
            work_summaries: vec![],
            session_id: None,
        };

        // Should not panic - should NOT show "git only" message since spec is present
        print_context_summary(&context);
    }

    #[test]
    fn test_print_context_summary_main_branch_no_git_only_message() {
        // Main branch with no spec/knowledge - should NOT show "git only" message
        // because the "main branch warning" is sufficient
        let context = FollowUpContext {
            git: GitContext {
                branch_name: "main".to_string(),
                base_branch: "main".to_string(),
                commits: vec![],
                diff_entries: vec![],
                merge_base_commit: None,
            },
            spec: None,
            spec_path: None,
            knowledge: None,
            work_summaries: vec![],
            session_id: None,
        };

        // Should not panic
        print_context_summary(&context);
    }
}
