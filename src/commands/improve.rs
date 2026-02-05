//! Improve command handler.
//!
//! Gathers context from git and previous autom8 sessions to enable
//! follow-up work with Claude. This command auto-detects everything
//! from the current git branch.

use crate::claude::run_improve_session;
use crate::error::{Autom8Error, Result};
use crate::gh::find_spec_for_branch;
use crate::git::{self, CommitInfo, DiffEntry};
use crate::knowledge::ProjectKnowledge;
use crate::output::improve::print_context_summary;
use crate::spec::Spec;
use crate::state::StateManager;
use std::path::PathBuf;

// ============================================================================
// US-008: Command Handler
// ============================================================================

/// Run the improve command.
///
/// This command gathers context from git and previous autom8 sessions,
/// displays a summary, and spawns an interactive Claude session for follow-up work.
///
/// The workflow is:
/// 1. Check if we're in a git repo (required)
/// 2. Gather context (git, spec, session knowledge)
/// 3. Display summary to the user (including any edge case warnings)
/// 4. Build the context prompt
/// 5. Spawn interactive Claude session
///
/// Edge cases handled:
/// - Not in a git repo: shows error with suggestion
/// - On main/master branch: shows warning but proceeds
/// - No session/spec found: shows message that only git context is available
/// - No commits vs base: shows "no commits yet" in summary
///
/// # Arguments
/// * `_verbose` - Reserved for future extensibility (currently unused)
///
/// # Returns
/// * `Ok(())` - Session completed
/// * `Err` - Not in git repo, context loading failed, or Claude spawn failed
pub fn improve_command(_verbose: bool) -> Result<()> {
    // Step 0: Check if we're in a git repo (fatal error if not)
    if !git::is_git_repo() {
        return Err(Autom8Error::NotInGitRepo);
    }

    // Step 1: Gather all context layers
    let context = load_follow_up_context()?;

    // Step 2: Display summary to the user
    print_context_summary(&context);

    // Step 3: Build the context prompt
    let prompt = build_improve_prompt(&context);

    // Step 4: Spawn interactive Claude session
    run_improve_session(&prompt)?;

    Ok(())
}

// ============================================================================
// US-001: Git Context Gathering
// ============================================================================

/// Git context for the current branch.
///
/// Contains all git-related context needed for the improve command.
/// This is Layer 1 context - always available when in a git repo.
#[derive(Debug, Clone)]
pub struct GitContext {
    /// Current branch name
    pub branch_name: String,
    /// Base branch name (main/master)
    pub base_branch: String,
    /// All commits on this branch since diverging from base
    pub commits: Vec<CommitInfo>,
    /// All file changes since diverging from base
    pub diff_entries: Vec<DiffEntry>,
    /// The merge-base commit hash (common ancestor with base branch)
    pub merge_base_commit: Option<String>,
}

impl GitContext {
    /// Check if this is a feature branch (not main/master).
    pub fn is_feature_branch(&self) -> bool {
        self.branch_name != "main" && self.branch_name != "master"
    }

    /// Get the number of commits on this branch.
    pub fn commit_count(&self) -> usize {
        self.commits.len()
    }

    /// Get the total number of files changed.
    pub fn files_changed_count(&self) -> usize {
        self.diff_entries.len()
    }

    /// Get total additions across all changed files.
    pub fn total_additions(&self) -> u32 {
        self.diff_entries.iter().map(|e| e.additions).sum()
    }

    /// Get total deletions across all changed files.
    pub fn total_deletions(&self) -> u32 {
        self.diff_entries.iter().map(|e| e.deletions).sum()
    }
}

/// Gather git context for the current branch.
///
/// This function collects:
/// - Current branch name
/// - Base branch (main/master)
/// - All commits since diverging from base
/// - All file changes since merge-base
///
/// Errors are handled gracefully - if certain operations fail,
/// the function will return partial context with empty collections.
///
/// # Returns
/// * `Ok(GitContext)` - Git context for the current branch
/// * `Err` - Only if we cannot get the current branch (fatal)
pub fn gather_git_context() -> Result<GitContext> {
    // Get current branch - this is required
    let branch_name = git::current_branch()?;

    // Detect base branch (main/master) - defaults to "main" if detection fails
    let base_branch = git::detect_base_branch().unwrap_or_else(|_| "main".to_string());

    // Get merge-base commit for accurate diff calculation
    let merge_base_commit = git::get_merge_base(&base_branch).ok();

    // Get commits since base branch
    // If this fails (e.g., branch doesn't exist), return empty vec
    let commits = git::get_branch_commits(&base_branch).unwrap_or_default();

    // Get diff entries since merge-base
    // Use merge-base if available, otherwise compare against base branch directly
    let diff_entries = if let Some(ref merge_base) = merge_base_commit {
        git::get_diff_since(merge_base).unwrap_or_default()
    } else {
        // Fallback: try to diff against base branch directly
        git::get_diff_since(&base_branch).unwrap_or_default()
    };

    Ok(GitContext {
        branch_name,
        base_branch,
        commits,
        diff_entries,
        merge_base_commit,
    })
}

// ============================================================================
// US-005: Build Context Prompt
// ============================================================================

/// Build a conversational, concise prompt summarizing the loaded context.
///
/// This prompt is shown to Claude at the start of an interactive improve session.
/// It acknowledges what context is available and ends with an open question.
///
/// # Arguments
/// * `context` - The follow-up context containing git, spec, and knowledge info
///
/// # Returns
/// A formatted prompt string (target: under ~500 words)
pub fn build_improve_prompt(context: &FollowUpContext) -> String {
    let mut sections: Vec<String> = Vec::new();

    // Opening statement acknowledging branch and what's loaded
    let opening = build_opening_statement(context);
    sections.push(opening);

    // Spec summary if available
    if let Some(ref spec) = context.spec {
        sections.push(build_spec_summary(spec));
    }

    // Key decisions if available
    if let Some(ref knowledge) = context.knowledge {
        if !knowledge.decisions.is_empty() {
            sections.push(build_decisions_summary(&knowledge.decisions));
        }
    }

    // Files touched if available
    if let Some(ref knowledge) = context.knowledge {
        if !knowledge.story_changes.is_empty() {
            if let Some(files_section) = build_files_summary(&knowledge.story_changes) {
                sections.push(files_section);
            }
        }
    }

    // Work summaries if available
    if !context.work_summaries.is_empty() {
        sections.push(build_work_summaries(&context.work_summaries));
    }

    // Closing question
    sections.push("What would you like to work on?".to_string());

    sections.join("\n\n")
}

/// Build the opening statement based on available context.
fn build_opening_statement(context: &FollowUpContext) -> String {
    let branch = &context.git.branch_name;
    let level = context.richness_level();

    match level {
        3 => format!(
            "You're on branch `{}`. I've loaded the spec, session knowledge, and git history.",
            branch
        ),
        2 => {
            if context.has_spec() {
                format!(
                    "You're on branch `{}`. I've loaded the spec and git history.",
                    branch
                )
            } else {
                format!(
                    "You're on branch `{}`. I've loaded session knowledge and git history.",
                    branch
                )
            }
        }
        _ => format!(
            "You're on branch `{}`. I've loaded the git history.",
            branch
        ),
    }
}

/// Build a summary of the spec.
fn build_spec_summary(spec: &Spec) -> String {
    let (completed, total) = spec.progress();
    let status = if spec.all_complete() {
        "all complete".to_string()
    } else {
        format!("{}/{} stories complete", completed, total)
    };

    format!("**Feature:** {} ({})", spec.project, status)
}

/// Build a summary of key decisions (topic and choice only).
fn build_decisions_summary(decisions: &[crate::knowledge::Decision]) -> String {
    let mut lines = vec!["**Key decisions:**".to_string()];

    // Limit to 5 most recent decisions to keep prompt concise
    for decision in decisions.iter().take(5) {
        lines.push(format!("- {}: {}", decision.topic, decision.choice));
    }

    if decisions.len() > 5 {
        lines.push(format!("- ...and {} more", decisions.len() - 5));
    }

    lines.join("\n")
}

/// Build a summary of files touched, grouped by created/modified.
fn build_files_summary(story_changes: &[crate::knowledge::StoryChanges]) -> Option<String> {
    use std::collections::HashSet;

    let mut created: HashSet<&std::path::Path> = HashSet::new();
    let mut modified: HashSet<&std::path::Path> = HashSet::new();

    for changes in story_changes {
        for file in &changes.files_created {
            created.insert(&file.path);
        }
        for file in &changes.files_modified {
            // Don't include in modified if already in created
            if !created.contains(file.path.as_path()) {
                modified.insert(&file.path);
            }
        }
    }

    if created.is_empty() && modified.is_empty() {
        return None;
    }

    let mut lines = vec!["**Files touched:**".to_string()];

    // Sort and limit files for readability
    let mut created_vec: Vec<_> = created.iter().collect();
    created_vec.sort();

    let mut modified_vec: Vec<_> = modified.iter().collect();
    modified_vec.sort();

    // Show created files (limit to 8)
    if !created_vec.is_empty() {
        lines.push("Created:".to_string());
        for path in created_vec.iter().take(8) {
            lines.push(format!("- {}", path.display()));
        }
        if created_vec.len() > 8 {
            lines.push(format!("- ...and {} more", created_vec.len() - 8));
        }
    }

    // Show modified files (limit to 8)
    if !modified_vec.is_empty() {
        lines.push("Modified:".to_string());
        for path in modified_vec.iter().take(8) {
            lines.push(format!("- {}", path.display()));
        }
        if modified_vec.len() > 8 {
            lines.push(format!("- ...and {} more", modified_vec.len() - 8));
        }
    }

    Some(lines.join("\n"))
}

/// Build a summary of work completed in previous iterations.
fn build_work_summaries(summaries: &[String]) -> String {
    let mut lines = vec!["**Work completed:**".to_string()];

    // Limit to 5 most recent summaries
    for summary in summaries.iter().take(5) {
        // Truncate long summaries to first 100 chars
        let truncated = if summary.len() > 100 {
            format!("{}...", &summary[..97])
        } else {
            summary.clone()
        };
        lines.push(format!("- {}", truncated));
    }

    if summaries.len() > 5 {
        lines.push(format!("- ...and {} more iterations", summaries.len() - 5));
    }

    lines.join("\n")
}

// ============================================================================
// US-004: Follow-Up Context (Combined Layers)
// ============================================================================

/// Combined context for follow-up work with Claude.
///
/// This struct combines all three context layers:
/// - Layer 1 (Git): Always present - branch, commits, diff entries
/// - Layer 2 (Spec): Optional - loaded from session or by branch name
/// - Layer 3 (Knowledge): Optional - decisions, patterns, files, work summaries
///
/// All layers except git are optional and degrade gracefully when unavailable.
#[derive(Debug, Clone)]
pub struct FollowUpContext {
    /// Git context (Layer 1) - always present
    pub git: GitContext,

    /// Spec loaded from session or by branch name (Layer 2) - optional
    pub spec: Option<Spec>,

    /// Path to the spec file (if spec was loaded)
    pub spec_path: Option<PathBuf>,

    /// Project knowledge from the session (Layer 3) - optional
    pub knowledge: Option<ProjectKnowledge>,

    /// Work summaries collected from all iterations (Layer 3) - optional
    /// Each summary describes what was accomplished in an iteration.
    pub work_summaries: Vec<String>,

    /// Session ID if a matching session was found
    pub session_id: Option<String>,
}

impl FollowUpContext {
    /// Check if spec context is available.
    pub fn has_spec(&self) -> bool {
        self.spec.is_some()
    }

    /// Check if session knowledge is available.
    pub fn has_knowledge(&self) -> bool {
        self.knowledge.is_some()
    }

    /// Check if any work summaries were collected.
    pub fn has_work_summaries(&self) -> bool {
        !self.work_summaries.is_empty()
    }

    /// Get the total number of work summaries.
    pub fn work_summary_count(&self) -> usize {
        self.work_summaries.len()
    }

    /// Get context richness level (1-3) based on available layers.
    ///
    /// - Level 1: Git only
    /// - Level 2: Git + Spec
    /// - Level 3: Git + Spec + Knowledge
    pub fn richness_level(&self) -> u8 {
        let mut level = 1; // Git is always present
        if self.has_spec() {
            level += 1;
        }
        if self.has_knowledge() {
            level += 1;
        }
        level
    }
}

/// Load all context layers for follow-up work.
///
/// This function gathers context additively:
/// 1. Git context is always gathered (required)
/// 2. Spec is loaded from session's spec_json_path if available,
///    otherwise by matching branch name
/// 3. If a session is found, project knowledge and work summaries
///    are extracted from the RunState
///
/// # Returns
/// * `Ok(FollowUpContext)` - Combined context from all available layers
/// * `Err` - Only if git context gathering fails (fatal)
pub fn load_follow_up_context() -> Result<FollowUpContext> {
    // Layer 1: Git context (always required)
    let git = gather_git_context()?;

    // Try to find a matching session for this branch
    let state_manager = StateManager::new()?;
    let session_metadata = state_manager
        .find_session_for_branch(&git.branch_name)
        .ok()
        .flatten();

    let mut spec: Option<Spec> = None;
    let mut spec_path: Option<PathBuf> = None;
    let mut knowledge: Option<ProjectKnowledge> = None;
    let mut work_summaries: Vec<String> = Vec::new();
    let mut session_id: Option<String> = None;

    if let Some(ref metadata) = session_metadata {
        session_id = Some(metadata.session_id.clone());

        // Layer 2: Try to load spec from session's spec_json_path first
        if let Some(ref path) = metadata.spec_json_path {
            if path.exists() {
                if let Ok(loaded_spec) = Spec::load(path) {
                    spec = Some(loaded_spec);
                    spec_path = Some(path.clone());
                }
            }
        }

        // Layer 3: Load RunState to extract knowledge and work summaries
        let session_state_manager = StateManager::with_session(metadata.session_id.clone())?;
        if let Ok(Some(run_state)) = session_state_manager.load_current() {
            // Extract project knowledge
            knowledge = Some(run_state.knowledge.clone());

            // Extract work summaries from iterations
            work_summaries = run_state
                .iterations
                .iter()
                .filter_map(|iter| iter.work_summary.clone())
                .collect();
        }
    }

    // Layer 2 fallback: If spec wasn't loaded from session, try find_spec_for_branch
    if spec.is_none() {
        if let Ok(Some((found_spec, found_path))) = find_spec_for_branch(&git.branch_name) {
            spec = Some(found_spec);
            spec_path = Some(found_path);
        }
    }

    Ok(FollowUpContext {
        git,
        spec,
        spec_path,
        knowledge,
        work_summaries,
        session_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{DiffEntry, DiffStatus};
    use crate::knowledge::{Decision, Pattern};
    use crate::spec::UserStory;
    use std::path::PathBuf;

    // ========================================================================
    // GitContext struct tests
    // ========================================================================

    #[test]
    fn test_git_context_is_feature_branch_true() {
        let context = GitContext {
            branch_name: "feature/improve-command".to_string(),
            base_branch: "main".to_string(),
            commits: vec![],
            diff_entries: vec![],
            merge_base_commit: None,
        };

        assert!(context.is_feature_branch());
    }

    #[test]
    fn test_git_context_is_feature_branch_false_main() {
        let context = GitContext {
            branch_name: "main".to_string(),
            base_branch: "main".to_string(),
            commits: vec![],
            diff_entries: vec![],
            merge_base_commit: None,
        };

        assert!(!context.is_feature_branch());
    }

    #[test]
    fn test_git_context_is_feature_branch_false_master() {
        let context = GitContext {
            branch_name: "master".to_string(),
            base_branch: "master".to_string(),
            commits: vec![],
            diff_entries: vec![],
            merge_base_commit: None,
        };

        assert!(!context.is_feature_branch());
    }

    #[test]
    fn test_git_context_commit_count() {
        let context = GitContext {
            branch_name: "feature/test".to_string(),
            base_branch: "main".to_string(),
            commits: vec![
                crate::git::CommitInfo {
                    short_hash: "abc1234".to_string(),
                    full_hash: "abc1234567890".to_string(),
                    message: "First commit".to_string(),
                    author: "Test".to_string(),
                    date: "2024-01-15".to_string(),
                },
                crate::git::CommitInfo {
                    short_hash: "def5678".to_string(),
                    full_hash: "def5678901234".to_string(),
                    message: "Second commit".to_string(),
                    author: "Test".to_string(),
                    date: "2024-01-16".to_string(),
                },
            ],
            diff_entries: vec![],
            merge_base_commit: Some("basehash".to_string()),
        };

        assert_eq!(context.commit_count(), 2);
    }

    #[test]
    fn test_git_context_files_changed_count() {
        let context = GitContext {
            branch_name: "feature/test".to_string(),
            base_branch: "main".to_string(),
            commits: vec![],
            diff_entries: vec![
                DiffEntry {
                    path: PathBuf::from("src/lib.rs"),
                    additions: 10,
                    deletions: 5,
                    status: DiffStatus::Modified,
                },
                DiffEntry {
                    path: PathBuf::from("src/main.rs"),
                    additions: 20,
                    deletions: 0,
                    status: DiffStatus::Added,
                },
            ],
            merge_base_commit: None,
        };

        assert_eq!(context.files_changed_count(), 2);
    }

    #[test]
    fn test_git_context_total_additions() {
        let context = GitContext {
            branch_name: "feature/test".to_string(),
            base_branch: "main".to_string(),
            commits: vec![],
            diff_entries: vec![
                DiffEntry {
                    path: PathBuf::from("src/lib.rs"),
                    additions: 10,
                    deletions: 5,
                    status: DiffStatus::Modified,
                },
                DiffEntry {
                    path: PathBuf::from("src/main.rs"),
                    additions: 20,
                    deletions: 3,
                    status: DiffStatus::Modified,
                },
            ],
            merge_base_commit: None,
        };

        assert_eq!(context.total_additions(), 30);
    }

    #[test]
    fn test_git_context_total_deletions() {
        let context = GitContext {
            branch_name: "feature/test".to_string(),
            base_branch: "main".to_string(),
            commits: vec![],
            diff_entries: vec![
                DiffEntry {
                    path: PathBuf::from("src/lib.rs"),
                    additions: 10,
                    deletions: 5,
                    status: DiffStatus::Modified,
                },
                DiffEntry {
                    path: PathBuf::from("src/main.rs"),
                    additions: 20,
                    deletions: 3,
                    status: DiffStatus::Modified,
                },
            ],
            merge_base_commit: None,
        };

        assert_eq!(context.total_deletions(), 8);
    }

    #[test]
    fn test_git_context_empty() {
        let context = GitContext {
            branch_name: "feature/empty".to_string(),
            base_branch: "main".to_string(),
            commits: vec![],
            diff_entries: vec![],
            merge_base_commit: None,
        };

        assert_eq!(context.commit_count(), 0);
        assert_eq!(context.files_changed_count(), 0);
        assert_eq!(context.total_additions(), 0);
        assert_eq!(context.total_deletions(), 0);
    }

    #[test]
    fn test_git_context_clone() {
        let context = GitContext {
            branch_name: "feature/test".to_string(),
            base_branch: "main".to_string(),
            commits: vec![],
            diff_entries: vec![],
            merge_base_commit: Some("abc123".to_string()),
        };

        let cloned = context.clone();
        assert_eq!(cloned.branch_name, context.branch_name);
        assert_eq!(cloned.merge_base_commit, context.merge_base_commit);
    }

    #[test]
    fn test_git_context_debug() {
        let context = GitContext {
            branch_name: "feature/test".to_string(),
            base_branch: "main".to_string(),
            commits: vec![],
            diff_entries: vec![],
            merge_base_commit: None,
        };

        let debug = format!("{:?}", context);
        assert!(debug.contains("GitContext"));
        assert!(debug.contains("feature/test"));
    }

    // ========================================================================
    // gather_git_context tests (integration-style, run in actual git repo)
    // ========================================================================

    #[test]
    fn test_gather_git_context_returns_valid_context() {
        // This test runs in the autom8 repo, so should succeed
        let result = gather_git_context();
        assert!(result.is_ok());

        let context = result.unwrap();
        // Should have a branch name
        assert!(!context.branch_name.is_empty());
        // Should have detected a base branch
        assert!(!context.base_branch.is_empty());
    }

    #[test]
    fn test_gather_git_context_has_base_branch() {
        let result = gather_git_context();
        assert!(result.is_ok());

        let context = result.unwrap();
        // Base branch should be main or master
        assert!(
            context.base_branch == "main" || context.base_branch == "master",
            "Expected 'main' or 'master', got '{}'",
            context.base_branch
        );
    }

    // ========================================================================
    // FollowUpContext struct tests (US-004)
    // ========================================================================

    fn make_git_context() -> GitContext {
        GitContext {
            branch_name: "feature/test".to_string(),
            base_branch: "main".to_string(),
            commits: vec![],
            diff_entries: vec![],
            merge_base_commit: None,
        }
    }

    fn make_spec() -> Spec {
        Spec {
            project: "TestProject".to_string(),
            branch_name: "feature/test".to_string(),
            description: "Test description".to_string(),
            user_stories: vec![UserStory {
                id: "US-001".to_string(),
                title: "Test Story".to_string(),
                description: "Test".to_string(),
                acceptance_criteria: vec![],
                priority: 1,
                passes: false,
                notes: String::new(),
            }],
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
        knowledge.patterns.push(Pattern {
            story_id: "US-001".to_string(),
            description: "Use Result for errors".to_string(),
            example_file: None,
        });
        knowledge
    }

    #[test]
    fn test_follow_up_context_git_only() {
        let context = FollowUpContext {
            git: make_git_context(),
            spec: None,
            spec_path: None,
            knowledge: None,
            work_summaries: vec![],
            session_id: None,
        };

        assert!(!context.has_spec());
        assert!(!context.has_knowledge());
        assert!(!context.has_work_summaries());
        assert_eq!(context.richness_level(), 1);
    }

    #[test]
    fn test_follow_up_context_with_spec() {
        let context = FollowUpContext {
            git: make_git_context(),
            spec: Some(make_spec()),
            spec_path: Some(PathBuf::from("/path/to/spec.json")),
            knowledge: None,
            work_summaries: vec![],
            session_id: None,
        };

        assert!(context.has_spec());
        assert!(!context.has_knowledge());
        assert_eq!(context.richness_level(), 2);
    }

    #[test]
    fn test_follow_up_context_full() {
        let context = FollowUpContext {
            git: make_git_context(),
            spec: Some(make_spec()),
            spec_path: Some(PathBuf::from("/path/to/spec.json")),
            knowledge: Some(make_knowledge()),
            work_summaries: vec![
                "Implemented feature A".to_string(),
                "Fixed bug in module B".to_string(),
            ],
            session_id: Some("session-123".to_string()),
        };

        assert!(context.has_spec());
        assert!(context.has_knowledge());
        assert!(context.has_work_summaries());
        assert_eq!(context.work_summary_count(), 2);
        assert_eq!(context.richness_level(), 3);
    }

    #[test]
    fn test_follow_up_context_richness_level_spec_only() {
        // Git + Spec = Level 2
        let context = FollowUpContext {
            git: make_git_context(),
            spec: Some(make_spec()),
            spec_path: None,
            knowledge: None,
            work_summaries: vec![],
            session_id: None,
        };

        assert_eq!(context.richness_level(), 2);
    }

    #[test]
    fn test_follow_up_context_richness_level_knowledge_only() {
        // Git + Knowledge (no spec) = Level 2
        let context = FollowUpContext {
            git: make_git_context(),
            spec: None,
            spec_path: None,
            knowledge: Some(make_knowledge()),
            work_summaries: vec![],
            session_id: None,
        };

        assert_eq!(context.richness_level(), 2);
    }

    #[test]
    fn test_follow_up_context_work_summaries_empty() {
        let context = FollowUpContext {
            git: make_git_context(),
            spec: None,
            spec_path: None,
            knowledge: None,
            work_summaries: vec![],
            session_id: None,
        };

        assert!(!context.has_work_summaries());
        assert_eq!(context.work_summary_count(), 0);
    }

    #[test]
    fn test_follow_up_context_work_summaries_with_entries() {
        let context = FollowUpContext {
            git: make_git_context(),
            spec: None,
            spec_path: None,
            knowledge: None,
            work_summaries: vec![
                "Summary 1".to_string(),
                "Summary 2".to_string(),
                "Summary 3".to_string(),
            ],
            session_id: None,
        };

        assert!(context.has_work_summaries());
        assert_eq!(context.work_summary_count(), 3);
    }

    #[test]
    fn test_follow_up_context_clone() {
        let context = FollowUpContext {
            git: make_git_context(),
            spec: Some(make_spec()),
            spec_path: Some(PathBuf::from("/path/to/spec.json")),
            knowledge: Some(make_knowledge()),
            work_summaries: vec!["Summary".to_string()],
            session_id: Some("session-id".to_string()),
        };

        let cloned = context.clone();
        assert_eq!(cloned.git.branch_name, context.git.branch_name);
        assert_eq!(cloned.spec_path, context.spec_path);
        assert_eq!(cloned.work_summaries.len(), context.work_summaries.len());
        assert_eq!(cloned.session_id, context.session_id);
    }

    #[test]
    fn test_follow_up_context_debug() {
        let context = FollowUpContext {
            git: make_git_context(),
            spec: None,
            spec_path: None,
            knowledge: None,
            work_summaries: vec![],
            session_id: None,
        };

        let debug = format!("{:?}", context);
        assert!(debug.contains("FollowUpContext"));
        assert!(debug.contains("git"));
    }

    // ========================================================================
    // build_improve_prompt tests (US-005)
    // ========================================================================

    use crate::knowledge::{FileChange, StoryChanges};

    fn make_knowledge_with_files() -> ProjectKnowledge {
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
        knowledge.story_changes.push(StoryChanges {
            story_id: "US-001".to_string(),
            files_created: vec![FileChange {
                path: PathBuf::from("src/new_module.rs"),
                additions: 100,
                deletions: 0,
                purpose: Some("New module".to_string()),
                key_symbols: vec![],
            }],
            files_modified: vec![FileChange {
                path: PathBuf::from("src/lib.rs"),
                additions: 5,
                deletions: 0,
                purpose: None,
                key_symbols: vec![],
            }],
            files_deleted: vec![],
            commit_hash: None,
        });
        knowledge
    }

    fn make_complete_spec() -> Spec {
        Spec {
            project: "TestProject".to_string(),
            branch_name: "feature/test".to_string(),
            description: "Test description".to_string(),
            user_stories: vec![
                UserStory {
                    id: "US-001".to_string(),
                    title: "Test Story".to_string(),
                    description: "Test".to_string(),
                    acceptance_criteria: vec![],
                    priority: 1,
                    passes: true,
                    notes: String::new(),
                },
                UserStory {
                    id: "US-002".to_string(),
                    title: "Test Story 2".to_string(),
                    description: "Test 2".to_string(),
                    acceptance_criteria: vec![],
                    priority: 2,
                    passes: true,
                    notes: String::new(),
                },
            ],
        }
    }

    #[test]
    fn test_build_improve_prompt_git_only() {
        let context = FollowUpContext {
            git: make_git_context(),
            spec: None,
            spec_path: None,
            knowledge: None,
            work_summaries: vec![],
            session_id: None,
        };

        let prompt = build_improve_prompt(&context);

        // Should contain branch name
        assert!(prompt.contains("feature/test"));
        // Should mention git history
        assert!(prompt.contains("git history"));
        // Should end with question
        assert!(prompt.contains("What would you like to work on?"));
        // Should NOT contain spec or knowledge sections
        assert!(!prompt.contains("**Feature:**"));
        assert!(!prompt.contains("**Key decisions:**"));
    }

    #[test]
    fn test_build_improve_prompt_with_spec() {
        let context = FollowUpContext {
            git: make_git_context(),
            spec: Some(make_spec()),
            spec_path: None,
            knowledge: None,
            work_summaries: vec![],
            session_id: None,
        };

        let prompt = build_improve_prompt(&context);

        // Should contain branch and spec info
        assert!(prompt.contains("feature/test"));
        assert!(prompt.contains("**Feature:**"));
        assert!(prompt.contains("TestProject"));
        // Should show story count (0/1 complete)
        assert!(prompt.contains("0/1 stories complete"));
        // Should end with question
        assert!(prompt.contains("What would you like to work on?"));
    }

    #[test]
    fn test_build_improve_prompt_with_complete_spec() {
        let context = FollowUpContext {
            git: make_git_context(),
            spec: Some(make_complete_spec()),
            spec_path: None,
            knowledge: None,
            work_summaries: vec![],
            session_id: None,
        };

        let prompt = build_improve_prompt(&context);

        // Should show "all complete" status
        assert!(prompt.contains("all complete"));
    }

    #[test]
    fn test_build_improve_prompt_with_decisions() {
        let context = FollowUpContext {
            git: make_git_context(),
            spec: None,
            spec_path: None,
            knowledge: Some(make_knowledge_with_files()),
            work_summaries: vec![],
            session_id: None,
        };

        let prompt = build_improve_prompt(&context);

        // Should contain decisions section
        assert!(prompt.contains("**Key decisions:**"));
        // Should contain topic and choice
        assert!(prompt.contains("Architecture: Use modules"));
        assert!(prompt.contains("Error handling: Use thiserror"));
    }

    #[test]
    fn test_build_improve_prompt_with_files() {
        let context = FollowUpContext {
            git: make_git_context(),
            spec: None,
            spec_path: None,
            knowledge: Some(make_knowledge_with_files()),
            work_summaries: vec![],
            session_id: None,
        };

        let prompt = build_improve_prompt(&context);

        // Should contain files section
        assert!(prompt.contains("**Files touched:**"));
        // Should show created and modified separately
        assert!(prompt.contains("Created:"));
        assert!(prompt.contains("src/new_module.rs"));
        assert!(prompt.contains("Modified:"));
        assert!(prompt.contains("src/lib.rs"));
    }

    #[test]
    fn test_build_improve_prompt_with_work_summaries() {
        let context = FollowUpContext {
            git: make_git_context(),
            spec: None,
            spec_path: None,
            knowledge: None,
            work_summaries: vec![
                "Implemented user authentication".to_string(),
                "Fixed login validation bug".to_string(),
            ],
            session_id: None,
        };

        let prompt = build_improve_prompt(&context);

        // Should contain work summaries section
        assert!(prompt.contains("**Work completed:**"));
        assert!(prompt.contains("Implemented user authentication"));
        assert!(prompt.contains("Fixed login validation bug"));
    }

    #[test]
    fn test_build_improve_prompt_full_context() {
        let context = FollowUpContext {
            git: make_git_context(),
            spec: Some(make_spec()),
            spec_path: None,
            knowledge: Some(make_knowledge_with_files()),
            work_summaries: vec!["Completed initial setup".to_string()],
            session_id: Some("session-123".to_string()),
        };

        let prompt = build_improve_prompt(&context);

        // Should mention all layers loaded
        assert!(prompt.contains("spec, session knowledge, and git history"));
        // Should have all sections
        assert!(prompt.contains("**Feature:**"));
        assert!(prompt.contains("**Key decisions:**"));
        assert!(prompt.contains("**Files touched:**"));
        assert!(prompt.contains("**Work completed:**"));
        // Should end with question
        assert!(prompt.ends_with("What would you like to work on?"));
    }

    #[test]
    fn test_build_improve_prompt_limits_decisions() {
        let mut knowledge = ProjectKnowledge::default();
        // Add 7 decisions
        for i in 1..=7 {
            knowledge.decisions.push(Decision {
                story_id: format!("US-{:03}", i),
                topic: format!("Topic {}", i),
                choice: format!("Choice {}", i),
                rationale: "Rationale".to_string(),
            });
        }

        let context = FollowUpContext {
            git: make_git_context(),
            spec: None,
            spec_path: None,
            knowledge: Some(knowledge),
            work_summaries: vec![],
            session_id: None,
        };

        let prompt = build_improve_prompt(&context);

        // Should show first 5 and indicate more
        assert!(prompt.contains("Topic 1: Choice 1"));
        assert!(prompt.contains("Topic 5: Choice 5"));
        assert!(prompt.contains("...and 2 more"));
        // Should NOT show Topic 6 or 7 directly
        assert!(!prompt.contains("Topic 6: Choice 6"));
    }

    #[test]
    fn test_build_improve_prompt_truncates_long_summaries() {
        let long_summary = "A".repeat(150);
        let context = FollowUpContext {
            git: make_git_context(),
            spec: None,
            spec_path: None,
            knowledge: None,
            work_summaries: vec![long_summary],
            session_id: None,
        };

        let prompt = build_improve_prompt(&context);

        // Should contain truncated summary with ellipsis
        assert!(prompt.contains("..."));
        // Should not contain the full 150-char string
        assert!(!prompt.contains(&"A".repeat(150)));
    }

    #[test]
    fn test_build_improve_prompt_limits_work_summaries() {
        let summaries: Vec<String> = (1..=8).map(|i| format!("Summary {}", i)).collect();

        let context = FollowUpContext {
            git: make_git_context(),
            spec: None,
            spec_path: None,
            knowledge: None,
            work_summaries: summaries,
            session_id: None,
        };

        let prompt = build_improve_prompt(&context);

        // Should show first 5 and indicate more
        assert!(prompt.contains("Summary 1"));
        assert!(prompt.contains("Summary 5"));
        assert!(prompt.contains("...and 3 more iterations"));
    }

    #[test]
    fn test_build_improve_prompt_empty_knowledge_no_files_section() {
        let mut knowledge = ProjectKnowledge::default();
        knowledge.decisions.push(Decision {
            story_id: "US-001".to_string(),
            topic: "Test".to_string(),
            choice: "Test".to_string(),
            rationale: "Test".to_string(),
        });
        // No story_changes

        let context = FollowUpContext {
            git: make_git_context(),
            spec: None,
            spec_path: None,
            knowledge: Some(knowledge),
            work_summaries: vec![],
            session_id: None,
        };

        let prompt = build_improve_prompt(&context);

        // Should have decisions but no files section
        assert!(prompt.contains("**Key decisions:**"));
        assert!(!prompt.contains("**Files touched:**"));
    }

    #[test]
    fn test_build_improve_prompt_opening_level_1() {
        let context = FollowUpContext {
            git: make_git_context(),
            spec: None,
            spec_path: None,
            knowledge: None,
            work_summaries: vec![],
            session_id: None,
        };

        let prompt = build_improve_prompt(&context);
        assert!(prompt.contains("I've loaded the git history"));
    }

    #[test]
    fn test_build_improve_prompt_opening_level_2_with_spec() {
        let context = FollowUpContext {
            git: make_git_context(),
            spec: Some(make_spec()),
            spec_path: None,
            knowledge: None,
            work_summaries: vec![],
            session_id: None,
        };

        let prompt = build_improve_prompt(&context);
        assert!(prompt.contains("I've loaded the spec and git history"));
    }

    #[test]
    fn test_build_improve_prompt_opening_level_2_with_knowledge() {
        let context = FollowUpContext {
            git: make_git_context(),
            spec: None,
            spec_path: None,
            knowledge: Some(make_knowledge()),
            work_summaries: vec![],
            session_id: None,
        };

        let prompt = build_improve_prompt(&context);
        assert!(prompt.contains("I've loaded session knowledge and git history"));
    }

    #[test]
    fn test_build_improve_prompt_opening_level_3() {
        let context = FollowUpContext {
            git: make_git_context(),
            spec: Some(make_spec()),
            spec_path: None,
            knowledge: Some(make_knowledge()),
            work_summaries: vec![],
            session_id: None,
        };

        let prompt = build_improve_prompt(&context);
        assert!(prompt.contains("I've loaded the spec, session knowledge, and git history"));
    }

    // ========================================================================
    // load_follow_up_context tests (integration-style)
    // ========================================================================

    #[test]
    fn test_load_follow_up_context_succeeds() {
        // This runs in the autom8 repo, so should succeed
        let result = load_follow_up_context();
        assert!(result.is_ok());

        let context = result.unwrap();
        // Git context should always be present
        assert!(!context.git.branch_name.is_empty());
        // Richness level should be at least 1 (git)
        assert!(context.richness_level() >= 1);
    }

    #[test]
    fn test_load_follow_up_context_git_always_present() {
        let result = load_follow_up_context();
        assert!(result.is_ok());

        let context = result.unwrap();
        // All required git fields should be populated
        assert!(!context.git.branch_name.is_empty());
        assert!(!context.git.base_branch.is_empty());
        // commits and diff_entries may be empty but should not panic
        let _ = context.git.commit_count();
        let _ = context.git.files_changed_count();
    }
}
