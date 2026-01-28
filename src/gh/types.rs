//! Core types for GitHub PR operations.

/// Information about an open pull request
#[derive(Debug, Clone, PartialEq)]
pub struct PullRequestInfo {
    /// PR number
    pub number: u32,
    /// PR title
    pub title: String,
    /// Head branch name (the source branch)
    pub head_branch: String,
    /// PR URL
    pub url: String,
}

/// Result of attempting to detect a PR for the current branch
#[derive(Debug, Clone, PartialEq)]
pub enum PRDetectionResult {
    /// Found a PR for the current branch
    Found(PullRequestInfo),
    /// Current branch is main/master with no PR
    OnMainBranch,
    /// On a feature branch but no open PR exists for it
    NoPRForBranch(String),
    /// Error occurred during detection
    Error(String),
}

/// Result type for PR creation operations
#[derive(Debug, Clone, PartialEq)]
pub enum PRResult {
    /// PR created successfully, contains PR URL
    Success(String),
    /// Prerequisites not met, contains reason for skip
    Skipped(String),
    /// PR already exists for branch, contains existing PR URL
    AlreadyExists(String),
    /// PR description updated successfully, contains PR URL
    Updated(String),
    /// PR creation attempted but failed, contains error message
    Error(String),
}
