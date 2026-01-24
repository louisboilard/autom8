use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Autom8Error {
    #[error("PRD file not found: {0}")]
    PrdNotFound(PathBuf),

    #[error("Invalid PRD format: {0}")]
    InvalidPrd(String),

    #[error("No incomplete stories found in PRD")]
    NoIncompleteStories,

    #[error("Claude process failed: {0}")]
    ClaudeError(String),

    #[error("Claude process timed out after {0} seconds")]
    ClaudeTimeout(u64),

    #[error("State file error: {0}")]
    StateError(String),

    #[error("No active run to resume")]
    NoActiveRun,

    #[error("Run already in progress: {0}")]
    RunInProgress(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Git error: {0}")]
    GitError(String),

    #[error("Spec file not found: {0}")]
    SpecNotFound(PathBuf),

    #[error("Spec file is empty")]
    EmptySpec,

    #[error("PRD generation failed: {0}")]
    PrdGenerationFailed(String),

    #[error("Invalid generated PRD: {0}")]
    InvalidGeneratedPrd(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Review failed after 3 iterations. Please manually review autom8_review.md for remaining issues.")]
    MaxReviewIterationsReached,

    #[error("No incomplete PRDs found in .autom8/prds/")]
    NoPrdsToResume,
}

pub type Result<T> = std::result::Result<T, Autom8Error>;
