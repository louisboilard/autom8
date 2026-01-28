//! Branch context for PR reviews.

use std::path::PathBuf;

use crate::error::Result;
use crate::git::{self, CommitInfo};
use crate::output::{CYAN, GRAY, GREEN, RESET, YELLOW};
use crate::spec::Spec;
use crate::state::StateManager;

/// Context about the current branch for PR reviews
#[derive(Debug, Clone)]
pub struct BranchContext {
    /// The branch name
    pub branch_name: String,
    /// The spec file if available
    pub spec: Option<Spec>,
    /// Path to the spec file if found
    pub spec_path: Option<PathBuf>,
    /// Recent commits on this branch
    pub commits: Vec<CommitInfo>,
}

/// Result of gathering branch context
#[derive(Debug, Clone)]
pub enum BranchContextResult {
    /// Successfully gathered context with spec
    SuccessWithSpec(BranchContext),
    /// Successfully gathered context but no spec found
    SuccessNoSpec(BranchContext),
    /// Error occurred during gathering
    Error(String),
}

/// Gather context about the current branch for PR review
pub fn gather_branch_context(show_warning: bool) -> BranchContextResult {
    let branch_name = match git::current_branch() {
        Ok(b) => b,
        Err(e) => return BranchContextResult::Error(format!("Failed to get current branch: {}", e)),
    };

    // Try to find spec for this branch
    let (spec, spec_path) = match find_spec_for_branch(&branch_name) {
        Ok(Some((s, p))) => (Some(s), Some(p)),
        Ok(None) => {
            if show_warning {
                println!(
                    "{YELLOW}Warning: No spec file found for branch '{}'{RESET}",
                    branch_name
                );
                println!("{GRAY}PR review will proceed with reduced context.{RESET}");
                println!();
            }
            (None, None)
        }
        Err(e) => {
            if show_warning {
                println!(
                    "{YELLOW}Warning: Failed to load spec: {}{RESET}",
                    e
                );
            }
            (None, None)
        }
    };

    // Get recent commits on this branch
    let commits = git::get_branch_commits(&branch_name).unwrap_or_default();

    let context = BranchContext {
        branch_name,
        spec,
        spec_path,
        commits,
    };

    if context.spec.is_some() {
        BranchContextResult::SuccessWithSpec(context)
    } else {
        BranchContextResult::SuccessNoSpec(context)
    }
}

/// Find the spec file for a given branch
pub fn find_spec_for_branch(branch: &str) -> Result<Option<(Spec, PathBuf)>> {
    let state_manager = StateManager::new()?;
    let specs = state_manager.list_specs()?;

    // First, try to find a spec that matches the branch name
    for spec_path in &specs {
        if let Ok(spec) = Spec::load(spec_path) {
            if spec.branch_name == branch {
                return Ok(Some((spec, spec_path.clone())));
            }
        }
    }

    // If no match found, try to infer from branch name
    // e.g., branch "feature/add-auth" might have spec "spec-add-auth.json"
    let branch_suffix = branch
        .strip_prefix("feature/")
        .or_else(|| branch.strip_prefix("feat/"))
        .unwrap_or(branch);

    for spec_path in &specs {
        let filename = spec_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        // Check if spec filename matches branch suffix
        if filename.ends_with(branch_suffix)
            || filename.replace("spec-", "").contains(branch_suffix)
        {
            if let Ok(spec) = Spec::load(spec_path) {
                return Ok(Some((spec, spec_path.clone())));
            }
        }
    }

    Ok(None)
}

/// Print a summary of the branch context
pub fn print_branch_context(context: &BranchContext) {
    println!("{CYAN}Branch:{RESET} {}", context.branch_name);

    if context.spec.is_some() {
        println!("{GREEN}  ✓ Spec file loaded{RESET}");
    } else {
        println!("{YELLOW}  ⚠ No spec file (reduced context){RESET}");
    }

    if !context.commits.is_empty() {
        println!(
            "{GRAY}  {} commit{} on branch{RESET}",
            context.commits.len(),
            if context.commits.len() == 1 { "" } else { "s" }
        );
    }
    println!();
}
