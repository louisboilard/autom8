use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Autom8Error {
    #[error("Spec file not found: {0}")]
    SpecNotFound(PathBuf),

    #[error("Invalid spec format: {0}")]
    InvalidSpec(String),

    #[error("No incomplete stories found in spec")]
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

    #[error("No incomplete specs found in spec/")]
    NoSpecsToResume,

    #[error("Shell completion error: {0}")]
    ShellCompletion(String),
}

pub type Result<T> = std::result::Result<T, Autom8Error>;
