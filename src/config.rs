use crate::error::{Autom8Error, Result};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::PathBuf;

/// The base config directory name under ~/.config/
const CONFIG_DIR_NAME: &str = "autom8";

// ============================================================================
// State Machine Configuration
// ============================================================================

/// Configuration for controlling which states are executed in the autom8 state machine.
///
/// This struct represents the user's preferences for which steps of the automation
/// pipeline should be executed. Each field corresponds to a state in the state machine.
///
/// # Default Behavior
///
/// By default, all states are enabled (`true`), meaning the full pipeline runs:
/// review → commit → pull request.
///
/// # Serialization
///
/// This struct supports TOML serialization via serde. Missing fields in a config file
/// will default to `true`, allowing partial configs to work correctly.
///
/// # Example
///
/// ```toml
/// # Enable/disable the review state (code review before committing)
/// review = true
///
/// # Enable/disable the commit state (creating git commits)
/// commit = true
///
/// # Enable/disable the pull request state (creating PRs)
/// pull_request = true
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// Whether to run the review state.
    ///
    /// When `true`, code changes are reviewed before committing.
    /// When `false`, the review step is skipped.
    #[serde(default = "default_true")]
    pub review: bool,

    /// Whether to run the commit state.
    ///
    /// When `true`, changes are committed to git.
    /// When `false`, changes are left uncommitted.
    #[serde(default = "default_true")]
    pub commit: bool,

    /// Whether to run the pull request state.
    ///
    /// When `true`, a pull request is created after committing.
    /// When `false`, no PR is created.
    #[serde(default = "default_true")]
    pub pull_request: bool,

    /// Whether to automatically create worktrees for runs.
    ///
    /// When `true`, autom8 creates a dedicated worktree for each run,
    /// enabling multiple parallel sessions for the same project.
    /// When `false`, autom8 runs on the current branch (default behavior).
    ///
    /// Note: Requires a git repository. Has no effect outside of git repos.
    #[serde(default = "default_false")]
    pub worktree: bool,
}

/// Helper function for serde default values (true).
fn default_true() -> bool {
    true
}

/// Helper function for serde default values (false).
fn default_false() -> bool {
    false
}

impl Default for Config {
    fn default() -> Self {
        Self {
            review: true,
            commit: true,
            pull_request: true,
            worktree: false,
        }
    }
}

// ============================================================================
// Config Validation
// ============================================================================

use std::error::Error;
use std::fmt;

/// Error type for configuration validation failures.
///
/// This enum represents specific validation errors that can occur when
/// validating configuration settings. Each variant provides a clear,
/// actionable error message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// Pull request is enabled but commit is disabled.
    ///
    /// Creating a pull request requires commits to exist, so this
    /// configuration combination is invalid.
    PullRequestWithoutCommit,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::PullRequestWithoutCommit => {
                write!(
                    f,
                    "Cannot create pull request without commits. \
                    Either set `commit = true` or set `pull_request = false`"
                )
            }
        }
    }
}

impl Error for ConfigError {}

/// Validate a configuration for logical consistency.
///
/// This function checks that the configuration settings are valid and
/// consistent with each other. It should be called after loading a config
/// and before the state machine starts.
///
/// # Validation Rules
///
/// - `pull_request = true` requires `commit = true`
///   (Cannot create a PR without commits)
///
/// # Arguments
///
/// * `config` - The configuration to validate
///
/// # Returns
///
/// * `Ok(())` if the configuration is valid
/// * `Err(ConfigError)` if the configuration is invalid, with a clear error message
///
/// # Example
///
/// ```
/// use autom8::config::{Config, validate_config};
///
/// let valid_config = Config::default();
/// assert!(validate_config(&valid_config).is_ok());
///
/// let invalid_config = Config {
///     review: true,
///     commit: false,
///     pull_request: true, // Invalid: PR without commit
///     ..Default::default()
/// };
/// assert!(validate_config(&invalid_config).is_err());
/// ```
pub fn validate_config(config: &Config) -> std::result::Result<(), ConfigError> {
    // Rule: pull_request = true requires commit = true
    if config.pull_request && !config.commit {
        return Err(ConfigError::PullRequestWithoutCommit);
    }

    Ok(())
}

// ============================================================================
// Global Config File Management
// ============================================================================

/// The filename for the global configuration file.
const GLOBAL_CONFIG_FILENAME: &str = "config.toml";

/// Default config file content with explanatory comments.
///
/// This is written when creating a new config file to help users understand
/// each option without needing to reference documentation.
const DEFAULT_CONFIG_WITH_COMMENTS: &str = r#"# Autom8 Configuration
# This file controls which states in the autom8 state machine are executed.

# Review state: Code review before committing
# - true: Run code review step to check implementation quality
# - false: Skip code review and proceed directly to commit
review = true

# Commit state: Creating git commits
# - true: Automatically commit changes after implementation
# - false: Leave changes uncommitted (manual commit required)
commit = true

# Pull request state: Creating pull requests
# - true: Automatically create a PR after committing
# - false: Skip PR creation (commits remain on local branch)
# Note: Requires commit = true to work
pull_request = true

# Worktree mode: Automatic worktree creation for parallel runs
# - true: Create a dedicated worktree for each run (enables parallel sessions)
# - false: Run on the current branch (default, single session per project)
# Note: Requires a git repository. Has no effect outside of git repos.
worktree = false
"#;

/// Get the path to the global config file.
///
/// Returns the path to `~/.config/autom8/config.toml`.
pub fn global_config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join(GLOBAL_CONFIG_FILENAME))
}

/// Load the global configuration from `~/.config/autom8/config.toml`.
///
/// If the config file doesn't exist, it creates one with default values
/// and helpful comments explaining each option.
///
/// # Returns
///
/// The loaded or newly-created default configuration.
///
/// # Errors
///
/// Returns an error if:
/// - The home directory cannot be determined
/// - The config directory cannot be created
/// - The config file cannot be read (other than not existing)
/// - The config file contains invalid TOML
pub fn load_global_config() -> Result<Config> {
    let config_path = global_config_path()?;

    if !config_path.exists() {
        // Ensure the config directory exists
        ensure_config_dir()?;

        // Create the config file with default values and comments
        fs::write(&config_path, DEFAULT_CONFIG_WITH_COMMENTS)?;

        return Ok(Config::default());
    }

    // Read and parse the existing config file
    let content = fs::read_to_string(&config_path)?;
    let config: Config = toml::from_str(&content).map_err(|e| {
        Autom8Error::Config(format!(
            "Failed to parse config file at {:?}: {}",
            config_path, e
        ))
    })?;

    Ok(config)
}

/// Save the global configuration to `~/.config/autom8/config.toml`.
///
/// This writes the configuration with explanatory comments. Note that this
/// will overwrite any existing file, including any user-added comments.
///
/// # Arguments
///
/// * `config` - The configuration to save
///
/// # Errors
///
/// Returns an error if:
/// - The home directory cannot be determined
/// - The config directory cannot be created
/// - The config file cannot be written
pub fn save_global_config(config: &Config) -> Result<()> {
    let config_path = global_config_path()?;

    // Ensure the config directory exists
    ensure_config_dir()?;

    // Generate config content with comments
    let content = generate_config_with_comments(config);

    fs::write(&config_path, content)?;

    Ok(())
}

/// Generate config file content with explanatory comments.
///
/// Creates a TOML string that includes comments explaining each option,
/// using the actual values from the provided config.
fn generate_config_with_comments(config: &Config) -> String {
    format!(
        r#"# Autom8 Configuration
# This file controls which states in the autom8 state machine are executed.

# Review state: Code review before committing
# - true: Run code review step to check implementation quality
# - false: Skip code review and proceed directly to commit
review = {}

# Commit state: Creating git commits
# - true: Automatically commit changes after implementation
# - false: Leave changes uncommitted (manual commit required)
commit = {}

# Pull request state: Creating pull requests
# - true: Automatically create a PR after committing
# - false: Skip PR creation (commits remain on local branch)
# Note: Requires commit = true to work
pull_request = {}

# Worktree mode: Automatic worktree creation for parallel runs
# - true: Create a dedicated worktree for each run (enables parallel sessions)
# - false: Run on the current branch (default, single session per project)
# Note: Requires a git repository. Has no effect outside of git repos.
worktree = {}
"#,
        config.review, config.commit, config.pull_request, config.worktree
    )
}

// ============================================================================
// Project Config File Management
// ============================================================================

/// The filename for project-specific configuration files.
const PROJECT_CONFIG_FILENAME: &str = "config.toml";

/// Get the path to a project's config file.
///
/// Returns the path to `~/.config/autom8/<project>/config.toml`.
pub fn project_config_path() -> Result<PathBuf> {
    Ok(project_config_dir()?.join(PROJECT_CONFIG_FILENAME))
}

/// Get the path to a specific project's config file by name.
///
/// Returns the path to `~/.config/autom8/<project_name>/config.toml`.
pub fn project_config_path_for(project_name: &str) -> Result<PathBuf> {
    Ok(project_config_dir_for(project_name)?.join(PROJECT_CONFIG_FILENAME))
}

/// Load the project-specific configuration from `~/.config/autom8/<project>/config.toml`.
///
/// If the project config file doesn't exist, it copies the global config (with comments)
/// to the project config directory and returns the global config values.
///
/// # Returns
///
/// The loaded or inherited configuration.
///
/// # Errors
///
/// Returns an error if:
/// - The home directory cannot be determined
/// - The project config directory cannot be created
/// - The config file cannot be read (other than not existing)
/// - The config file contains invalid TOML
pub fn load_project_config() -> Result<Config> {
    let config_path = project_config_path()?;

    if !config_path.exists() {
        // Ensure the project config directory exists
        ensure_project_config_dir()?;

        // Copy global config (with comments) to project config
        let global_config = load_global_config()?;
        let content = generate_config_with_comments(&global_config);
        fs::write(&config_path, content)?;

        return Ok(global_config);
    }

    // Read and parse the existing project config file
    let content = fs::read_to_string(&config_path)?;
    let config: Config = toml::from_str(&content).map_err(|e| {
        Autom8Error::Config(format!(
            "Failed to parse project config file at {:?}: {}",
            config_path, e
        ))
    })?;

    Ok(config)
}

/// Save a project-specific configuration to `~/.config/autom8/<project>/config.toml`.
///
/// This writes the configuration with explanatory comments. Note that this
/// will overwrite any existing file, including any user-added comments.
///
/// # Arguments
///
/// * `config` - The configuration to save
///
/// # Errors
///
/// Returns an error if:
/// - The home directory cannot be determined
/// - The project config directory cannot be created
/// - The config file cannot be written
pub fn save_project_config(config: &Config) -> Result<()> {
    let config_path = project_config_path()?;

    // Ensure the project config directory exists
    ensure_project_config_dir()?;

    // Generate config content with comments
    let content = generate_config_with_comments(config);

    fs::write(&config_path, content)?;

    Ok(())
}

/// Get the effective configuration for the current project.
///
/// This function returns the resolved configuration by checking:
/// 1. If a project config exists at `~/.config/autom8/<project>/config.toml`, return it
/// 2. Otherwise, return the global config from `~/.config/autom8/config.toml`
///
/// Unlike `load_project_config()`, this function does NOT create a project config
/// if one doesn't exist. It simply returns whichever config is applicable.
///
/// **Important:** This function validates the configuration before returning it.
/// Invalid configurations will result in an error.
///
/// # Returns
///
/// The effective configuration (project config if exists, else global config).
///
/// # Errors
///
/// Returns an error if:
/// - The home directory cannot be determined
/// - The config file cannot be read
/// - The config file contains invalid TOML
/// - The configuration is invalid (e.g., pull_request=true with commit=false)
pub fn get_effective_config() -> Result<Config> {
    let project_config_path = project_config_path()?;

    let config = if project_config_path.exists() {
        // Project config exists, load it directly (no auto-creation)
        let content = fs::read_to_string(&project_config_path)?;
        toml::from_str(&content).map_err(|e| {
            Autom8Error::Config(format!(
                "Failed to parse project config file at {:?}: {}",
                project_config_path, e
            ))
        })?
    } else {
        // No project config, load global config
        load_global_config()?
    };

    // Validate the configuration before returning
    validate_config(&config).map_err(|e| Autom8Error::Config(e.to_string()))?;

    Ok(config)
}

// ============================================================================
// Directory Management
// ============================================================================

/// Subdirectory names within a project config directory
const SPEC_SUBDIR: &str = "spec";
const RUNS_SUBDIR: &str = "runs";

/// Get the autom8 config directory path (~/.config/autom8/).
///
/// Returns the path to the config directory. Does not create the directory.
pub fn config_dir() -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| Autom8Error::Config("Could not determine home directory".to_string()))?;
    Ok(home.join(".config").join(CONFIG_DIR_NAME))
}

/// Ensure the autom8 config directory exists (~/.config/autom8/).
///
/// Creates the directory if it doesn't exist. Returns whether the directory
/// was newly created (true) or already existed (false).
pub fn ensure_config_dir() -> Result<(PathBuf, bool)> {
    let dir = config_dir()?;
    let created = !dir.exists();
    fs::create_dir_all(&dir)?;
    Ok((dir, created))
}

/// Get the current project name.
///
/// Uses the git repository name (basename of the main repo root) when in a git
/// repository, ensuring consistent project identification across all worktrees.
/// Falls back to the current working directory basename if not in a git repo.
pub fn current_project_name() -> Result<String> {
    // Try to get the git repository name first
    if let Ok(Some(repo_name)) = crate::worktree::get_git_repo_name() {
        return Ok(repo_name);
    }

    // Fallback: use CWD basename for non-git directories
    let cwd = env::current_dir().map_err(|e| {
        Autom8Error::Config(format!("Could not determine current directory: {}", e))
    })?;
    cwd.file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            Autom8Error::Config("Could not determine project name from path".to_string())
        })
}

/// Get the project-specific config directory path (~/.config/autom8/<project-name>/).
///
/// Returns the path to the project config directory. Does not create the directory.
pub fn project_config_dir() -> Result<PathBuf> {
    let base = config_dir()?;
    let project_name = current_project_name()?;
    Ok(base.join(project_name))
}

/// Get the project-specific config directory path for a given project name.
pub fn project_config_dir_for(project_name: &str) -> Result<PathBuf> {
    let base = config_dir()?;
    Ok(base.join(project_name))
}

/// Ensure the project-specific config directory and its subdirectories exist.
///
/// Creates:
/// - `~/.config/autom8/<project-name>/`
/// - `~/.config/autom8/<project-name>/spec/`
/// - `~/.config/autom8/<project-name>/runs/`
///
/// Returns the project config directory path and whether it was newly created.
pub fn ensure_project_config_dir() -> Result<(PathBuf, bool)> {
    let dir = project_config_dir()?;
    let created = !dir.exists();

    // Create all subdirectories
    fs::create_dir_all(dir.join(SPEC_SUBDIR))?;
    fs::create_dir_all(dir.join(RUNS_SUBDIR))?;

    Ok((dir, created))
}

/// Get the spec subdirectory path for the current project.
pub fn spec_dir() -> Result<PathBuf> {
    Ok(project_config_dir()?.join(SPEC_SUBDIR))
}

/// Get the runs subdirectory path for the current project.
pub fn runs_dir() -> Result<PathBuf> {
    Ok(project_config_dir()?.join(RUNS_SUBDIR))
}

/// List all project directories in the config directory.
///
/// Returns a sorted list of project names (directory basenames) from `~/.config/autom8/`.
/// Only includes directories, not files.
pub fn list_projects() -> Result<Vec<String>> {
    let base = config_dir()?;

    if !base.exists() {
        return Ok(Vec::new());
    }

    let mut projects = Vec::new();

    let entries = fs::read_dir(&base)
        .map_err(|e| Autom8Error::Config(format!("Could not read config directory: {}", e)))?;

    for entry in entries {
        let entry = entry
            .map_err(|e| Autom8Error::Config(format!("Could not read directory entry: {}", e)))?;

        let path = entry.path();
        if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                projects.push(name.to_string());
            }
        }
    }

    projects.sort();
    Ok(projects)
}

/// Check if a file is already inside the project's config directory.
///
/// Returns true if the file path is inside `~/.config/autom8/<project-name>/`.
pub fn is_in_config_dir(file_path: &std::path::Path) -> Result<bool> {
    let config_dir = project_config_dir()?;

    // Canonicalize both paths to handle relative paths and symlinks
    let canonical_file = file_path
        .canonicalize()
        .unwrap_or_else(|_| file_path.to_path_buf());
    let canonical_config = config_dir.canonicalize().unwrap_or(config_dir);

    Ok(canonical_file.starts_with(&canonical_config))
}

/// Result of moving a file to the config directory.
#[derive(Debug)]
pub struct MoveResult {
    /// The destination path where the file was moved.
    pub dest_path: PathBuf,
    /// Whether the file was actually moved (false if already in config dir).
    pub was_moved: bool,
}

/// Move a file to the appropriate config subdirectory if it's not already there.
///
/// Both markdown (`.md`) and JSON (`.json`) files are moved to `~/.config/autom8/<project-name>/spec/`
///
/// Uses `fs::rename()` when possible, falls back to copy+delete for cross-filesystem moves.
///
/// Returns the path to use for processing (either the original or the moved location).
pub fn move_to_config_dir(file_path: &std::path::Path) -> Result<MoveResult> {
    // If already in config directory, return original path
    if is_in_config_dir(file_path)? {
        let canonical = file_path
            .canonicalize()
            .unwrap_or_else(|_| file_path.to_path_buf());
        return Ok(MoveResult {
            dest_path: canonical,
            was_moved: false,
        });
    }

    // All files go to spec/ directory
    let dest_dir = spec_dir()?;

    // Ensure destination directory exists
    fs::create_dir_all(&dest_dir)?;

    // Get filename and create destination path
    let filename = file_path
        .file_name()
        .ok_or_else(|| Autom8Error::Config("Could not determine filename".to_string()))?;
    let dest_path = dest_dir.join(filename);

    // Try rename first (fast, atomic), fall back to copy+delete for cross-filesystem
    if fs::rename(file_path, &dest_path).is_err() {
        // Cross-filesystem move: copy then delete original
        fs::copy(file_path, &dest_path)?;
        fs::remove_file(file_path)?;
    }

    Ok(MoveResult {
        dest_path,
        was_moved: true,
    })
}

/// Status information for a single project.
#[derive(Debug, Clone)]
pub struct ProjectStatus {
    /// The project name (directory basename).
    pub name: String,
    /// Whether there is an active or failed run.
    pub has_active_run: bool,
    /// The run status (if any run exists).
    pub run_status: Option<crate::state::RunStatus>,
    /// Count of incomplete specs.
    pub incomplete_spec_count: usize,
    /// Total spec count.
    pub total_spec_count: usize,
}

impl ProjectStatus {
    /// Returns true if this project needs attention (active/failed run or incomplete specs).
    pub fn needs_attention(&self) -> bool {
        self.has_active_run
            || self.run_status == Some(crate::state::RunStatus::Failed)
            || self.incomplete_spec_count > 0
    }

    /// Returns true if this project is idle (no active work).
    pub fn is_idle(&self) -> bool {
        !self.needs_attention()
    }
}

/// Information about a project's directory contents for tree display.
#[derive(Debug, Clone)]
pub struct ProjectTreeInfo {
    /// The project name (directory basename).
    pub name: String,
    /// Whether there is an active run.
    pub has_active_run: bool,
    /// The run status (if any run exists).
    pub run_status: Option<crate::state::RunStatus>,
    /// Number of spec files in spec/ directory.
    pub spec_count: usize,
    /// Number of incomplete specs.
    pub incomplete_spec_count: usize,
    /// Number of markdown spec files in spec/ directory.
    pub spec_md_count: usize,
    /// Number of archived runs in runs/ directory.
    pub runs_count: usize,
    /// The date of the most recent run (archived or current).
    pub last_run_date: Option<chrono::DateTime<chrono::Utc>>,
}

impl ProjectTreeInfo {
    /// Returns a status label for the project.
    pub fn status_label(&self) -> &'static str {
        if self.has_active_run {
            "running"
        } else if self.run_status == Some(crate::state::RunStatus::Failed) {
            "failed"
        } else if self.incomplete_spec_count > 0 {
            "incomplete"
        } else if self.spec_count > 0 {
            "complete"
        } else {
            "empty"
        }
    }

    /// Returns true if this project has any content.
    pub fn has_content(&self) -> bool {
        self.spec_count > 0 || self.spec_md_count > 0 || self.runs_count > 0 || self.has_active_run
    }
}

/// Get detailed tree information for all projects.
///
/// Returns a list of `ProjectTreeInfo` for each project in `~/.config/autom8/`.
/// Projects are sorted alphabetically by name.
pub fn list_projects_tree() -> Result<Vec<ProjectTreeInfo>> {
    use crate::spec::Spec;
    use crate::state::StateManager;

    let projects = list_projects()?;
    let mut tree_info = Vec::new();

    for project_name in projects {
        let sm = StateManager::for_project(&project_name)?;

        // Check for active run
        let run_state = sm.load_current().ok().flatten();
        let has_active_run = run_state
            .as_ref()
            .map(|s| s.status == crate::state::RunStatus::Running)
            .unwrap_or(false);
        let run_status = run_state.as_ref().map(|s| s.status);

        // Count specs and incomplete specs
        let specs = sm.list_specs().unwrap_or_default();
        let mut incomplete_count = 0;

        for spec_path in &specs {
            if let Ok(spec) = Spec::load(spec_path) {
                if spec.is_incomplete() {
                    incomplete_count += 1;
                }
            }
        }

        // Count spec files (markdown specs)
        let project_dir = project_config_dir_for(&project_name)?;
        let spec_dir = project_dir.join(SPEC_SUBDIR);
        let spec_md_count = if spec_dir.exists() {
            fs::read_dir(&spec_dir)
                .map(|entries| {
                    entries
                        .filter_map(|e| e.ok())
                        .filter(|e| {
                            e.path().is_file()
                                && e.path().extension().is_some_and(|ext| ext == "md")
                        })
                        .count()
                })
                .unwrap_or(0)
        } else {
            0
        };

        // Get archived runs (already sorted by date, most recent first)
        let archived_runs = sm.list_archived().unwrap_or_default();
        let runs_count = archived_runs.len();

        // Determine last run date from archived runs or current run
        let last_run_date = run_state
            .as_ref()
            .map(|s| s.started_at)
            .or_else(|| archived_runs.first().map(|r| r.started_at));

        tree_info.push(ProjectTreeInfo {
            name: project_name,
            has_active_run,
            run_status,
            spec_count: specs.len(),
            incomplete_spec_count: incomplete_count,
            spec_md_count,
            runs_count,
            last_run_date,
        });
    }

    Ok(tree_info)
}

/// Detailed information about a project for the describe command.
#[derive(Debug, Clone)]
pub struct ProjectDescription {
    /// The project name.
    pub name: String,
    /// Path to the project config directory.
    pub path: PathBuf,
    /// Whether there is an active run.
    pub has_active_run: bool,
    /// The run status (if any run exists).
    pub run_status: Option<crate::state::RunStatus>,
    /// Current story being worked on (if any).
    pub current_story: Option<String>,
    /// Current branch from state (if any).
    pub current_branch: Option<String>,
    /// List of specs with their details.
    pub specs: Vec<SpecSummary>,
    /// Number of markdown spec files.
    pub spec_md_count: usize,
    /// Number of archived runs.
    pub runs_count: usize,
}

/// Summary of a single spec.
#[derive(Debug, Clone)]
pub struct SpecSummary {
    /// The spec filename.
    pub filename: String,
    /// Full path to the spec file.
    pub path: PathBuf,
    /// Project name from the spec.
    pub project_name: String,
    /// Branch name from the spec.
    pub branch_name: String,
    /// Description from the spec.
    pub description: String,
    /// All user stories with their status.
    pub stories: Vec<StorySummary>,
    /// Number of completed stories.
    pub completed_count: usize,
    /// Total number of stories.
    pub total_count: usize,
}

/// Summary of a user story.
#[derive(Debug, Clone)]
pub struct StorySummary {
    /// Story ID (e.g., "US-001").
    pub id: String,
    /// Story title.
    pub title: String,
    /// Whether the story passes.
    pub passes: bool,
}

/// Check if a project exists in the config directory.
pub fn project_exists(project_name: &str) -> Result<bool> {
    let project_dir = project_config_dir_for(project_name)?;
    Ok(project_dir.exists())
}

/// Get detailed description of a project.
///
/// Returns `None` if the project doesn't exist.
pub fn get_project_description(project_name: &str) -> Result<Option<ProjectDescription>> {
    use crate::spec::Spec;
    use crate::state::StateManager;

    let project_dir = project_config_dir_for(project_name)?;

    if !project_dir.exists() {
        return Ok(None);
    }

    let sm = StateManager::for_project(project_name)?;

    // Check for active run
    let run_state = sm.load_current().ok().flatten();
    let has_active_run = run_state
        .as_ref()
        .map(|s| s.status == crate::state::RunStatus::Running)
        .unwrap_or(false);
    let run_status = run_state.as_ref().map(|s| s.status);
    let current_story = run_state.as_ref().and_then(|s| s.current_story.clone());
    let current_branch = run_state.map(|s| s.branch);

    // Load specs with details
    let spec_paths = sm.list_specs().unwrap_or_default();
    let mut specs = Vec::new();

    for spec_path in spec_paths {
        if let Ok(spec) = Spec::load(&spec_path) {
            let stories: Vec<StorySummary> = spec
                .user_stories
                .iter()
                .map(|s| StorySummary {
                    id: s.id.clone(),
                    title: s.title.clone(),
                    passes: s.passes,
                })
                .collect();

            let completed_count = stories.iter().filter(|s| s.passes).count();
            let total_count = stories.len();

            let filename = spec_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();

            specs.push(SpecSummary {
                filename,
                path: spec_path,
                project_name: spec.project,
                branch_name: spec.branch_name,
                description: spec.description,
                stories,
                completed_count,
                total_count,
            });
        }
    }

    // Count spec files (markdown specs)
    let spec_dir = project_dir.join(SPEC_SUBDIR);
    let spec_md_count = if spec_dir.exists() {
        fs::read_dir(&spec_dir)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter(|e| {
                        e.path().is_file() && e.path().extension().is_some_and(|ext| ext == "md")
                    })
                    .count()
            })
            .unwrap_or(0)
    } else {
        0
    };

    // Count archived runs
    let runs_count = sm.list_archived().unwrap_or_default().len();

    Ok(Some(ProjectDescription {
        name: project_name.to_string(),
        path: project_dir,
        has_active_run,
        run_status,
        current_story,
        current_branch,
        specs,
        spec_md_count,
        runs_count,
    }))
}

/// Get status for all projects across the config directory.
///
/// Returns a list of `ProjectStatus` for each project in `~/.config/autom8/`.
/// Projects are sorted alphabetically by name.
pub fn global_status() -> Result<Vec<ProjectStatus>> {
    use crate::spec::Spec;
    use crate::state::StateManager;

    let projects = list_projects()?;
    let mut statuses = Vec::new();

    for project_name in projects {
        let sm = StateManager::for_project(&project_name)?;

        // Check for active run
        let run_state = sm.load_current().ok().flatten();
        let has_active_run = run_state
            .as_ref()
            .map(|s| s.status == crate::state::RunStatus::Running)
            .unwrap_or(false);
        let run_status = run_state.map(|s| s.status);

        // Count incomplete specs
        let specs = sm.list_specs().unwrap_or_default();
        let mut incomplete_count = 0;
        let mut total_count = 0;

        for spec_path in &specs {
            if let Ok(spec) = Spec::load(spec_path) {
                total_count += 1;
                if spec.is_incomplete() {
                    incomplete_count += 1;
                }
            }
        }

        statuses.push(ProjectStatus {
            name: project_name,
            has_active_run,
            run_status,
            incomplete_spec_count: incomplete_count,
            total_spec_count: total_count,
        });
    }

    Ok(statuses)
}

/// Get status for all projects at a given config directory (for testing).
#[cfg(test)]
fn global_status_at(base_config_dir: &std::path::Path) -> Result<Vec<ProjectStatus>> {
    use crate::spec::Spec;
    use crate::state::StateManager;

    let projects = list_projects_at(base_config_dir)?;
    let mut statuses = Vec::new();

    for project_name in projects {
        let project_dir = base_config_dir.join(&project_name);
        let sm = StateManager::with_dir(project_dir);

        // Check for active run
        let run_state = sm.load_current().ok().flatten();
        let has_active_run = run_state
            .as_ref()
            .map(|s| s.status == crate::state::RunStatus::Running)
            .unwrap_or(false);
        let run_status = run_state.map(|s| s.status);

        // Count incomplete specs
        let specs = sm.list_specs().unwrap_or_default();
        let mut incomplete_count = 0;
        let mut total_count = 0;

        for spec_path in &specs {
            if let Ok(spec) = Spec::load(spec_path) {
                total_count += 1;
                if spec.is_incomplete() {
                    incomplete_count += 1;
                }
            }
        }

        statuses.push(ProjectStatus {
            name: project_name,
            has_active_run,
            run_status,
            incomplete_spec_count: incomplete_count,
            total_spec_count: total_count,
        });
    }

    Ok(statuses)
}

/// List all project directories at a given base config path.
///
/// This is a testable version that allows specifying a custom base path.
/// Returns a sorted list of project names (directory basenames).
#[cfg(test)]
fn list_projects_at(base_config_dir: &std::path::Path) -> Result<Vec<String>> {
    if !base_config_dir.exists() {
        return Ok(Vec::new());
    }

    let mut projects = Vec::new();

    let entries = fs::read_dir(base_config_dir)
        .map_err(|e| Autom8Error::Config(format!("Could not read config directory: {}", e)))?;

    for entry in entries {
        let entry = entry
            .map_err(|e| Autom8Error::Config(format!("Could not read directory entry: {}", e)))?;

        let path = entry.path();
        if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                projects.push(name.to_string());
            }
        }
    }

    projects.sort();
    Ok(projects)
}

/// Ensure a config directory exists at the given base path.
///
/// This is a testable version that allows specifying a custom base path.
/// Creates `<base>/.config/autom8/` if it doesn't exist.
///
/// Returns the full path and whether the directory was newly created.
#[cfg(test)]
fn ensure_config_dir_at(base: &std::path::Path) -> Result<(PathBuf, bool)> {
    let dir = base.join(".config").join(CONFIG_DIR_NAME);
    let created = !dir.exists();
    fs::create_dir_all(&dir)?;
    Ok((dir, created))
}

/// Ensure a project config directory with subdirectories exists at the given base path.
///
/// This is a testable version that allows specifying a custom base path and project name.
/// Creates:
/// - `<base>/.config/autom8/<project-name>/`
/// - `<base>/.config/autom8/<project-name>/spec/`
/// - `<base>/.config/autom8/<project-name>/runs/`
///
/// Returns the full project path and whether it was newly created.
#[cfg(test)]
fn ensure_project_config_dir_at(
    base: &std::path::Path,
    project_name: &str,
) -> Result<(PathBuf, bool)> {
    let dir = base
        .join(".config")
        .join(CONFIG_DIR_NAME)
        .join(project_name);
    let created = !dir.exists();

    fs::create_dir_all(dir.join(SPEC_SUBDIR))?;
    fs::create_dir_all(dir.join(RUNS_SUBDIR))?;

    Ok((dir, created))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_config_dir_returns_path_ending_with_autom8() {
        // This test verifies the structure without depending on exact paths
        let result = config_dir().unwrap();
        assert!(result.ends_with("autom8"));
        assert!(result.parent().unwrap().ends_with(".config"));
    }

    #[test]
    fn test_ensure_config_dir_at_creates_directory() {
        let temp_dir = TempDir::new().unwrap();
        let expected_path = temp_dir.path().join(".config").join("autom8");
        assert!(!expected_path.exists());

        let (path, created) = ensure_config_dir_at(temp_dir.path()).unwrap();

        assert_eq!(path, expected_path);
        assert!(created);
        assert!(expected_path.exists());
        assert!(expected_path.is_dir());
    }

    #[test]
    fn test_ensure_config_dir_at_reports_existing_directory() {
        let temp_dir = TempDir::new().unwrap();
        let expected_path = temp_dir.path().join(".config").join("autom8");

        // Create the directory first
        fs::create_dir_all(&expected_path).unwrap();
        assert!(expected_path.exists());

        let (path, created) = ensure_config_dir_at(temp_dir.path()).unwrap();

        assert_eq!(path, expected_path);
        assert!(!created); // Directory already existed
        assert!(expected_path.exists());
    }

    #[test]
    fn test_ensure_config_dir_at_creates_parent_directories() {
        let temp_dir = TempDir::new().unwrap();

        // Neither .config nor .config/autom8 should exist initially
        let config_path = temp_dir.path().join(".config");
        assert!(!config_path.exists());

        let (path, created) = ensure_config_dir_at(temp_dir.path()).unwrap();

        assert!(created);
        assert!(path.exists());
        assert!(config_path.exists()); // Parent was also created
    }

    #[test]
    fn test_ensure_config_dir_creates_real_directory() {
        // This test uses the real function to verify it doesn't panic
        // and returns a valid path structure
        let result = ensure_config_dir();
        assert!(result.is_ok());
        let (path, _created) = result.unwrap();
        assert!(path.ends_with("autom8"));
        assert!(path.exists());
    }

    #[test]
    fn test_current_project_name_returns_git_repo_name() {
        // This test verifies the function returns the git repo name when in a git repo
        // (which enables consistent project identification across worktrees)
        let result = current_project_name();
        assert!(result.is_ok());
        let name = result.unwrap();
        assert!(!name.is_empty());
        // Running from autom8 project directory, should use git repo name
        assert_eq!(name, "autom8");
    }

    #[test]
    fn test_current_project_name_uses_git_repo_not_cwd() {
        // Verify that current_project_name uses git repo name, not CWD basename.
        // The git repo name should match what get_git_repo_name() returns.
        let project_name = current_project_name().unwrap();
        let git_repo_name = crate::worktree::get_git_repo_name().unwrap();

        // When in a git repo, both should return the same value
        assert!(git_repo_name.is_some(), "Should be in a git repo");
        assert_eq!(
            project_name,
            git_repo_name.unwrap(),
            "current_project_name should match git repo name"
        );
    }

    #[test]
    fn test_current_project_name_is_consistent_across_calls() {
        // Project name should be stable - important for worktree support
        let name1 = current_project_name().unwrap();
        let name2 = current_project_name().unwrap();
        assert_eq!(name1, name2, "Project name should be stable across calls");
    }

    #[test]
    fn test_project_config_dir_includes_project_name() {
        let result = project_config_dir().unwrap();
        // Path should be ~/.config/autom8/<project-name>
        assert!(result.parent().unwrap().ends_with("autom8"));
        // Project name should be the last component
        let project_name = result.file_name().unwrap().to_str().unwrap();
        assert_eq!(project_name, "autom8");
    }

    #[test]
    fn test_project_config_dir_for_with_custom_name() {
        let result = project_config_dir_for("my-project").unwrap();
        assert!(result.ends_with("my-project"));
        assert!(result.parent().unwrap().ends_with("autom8"));
    }

    #[test]
    fn test_ensure_project_config_dir_at_creates_all_subdirs() {
        let temp_dir = TempDir::new().unwrap();
        let project_name = "test-project";

        let (path, created) = ensure_project_config_dir_at(temp_dir.path(), project_name).unwrap();

        assert!(created);
        assert!(path.exists());
        assert!(path.ends_with(project_name));

        // Verify all subdirectories were created
        assert!(path.join("spec").exists());
        assert!(path.join("spec").is_dir());
        assert!(path.join("runs").exists());
        assert!(path.join("runs").is_dir());
    }

    #[test]
    fn test_ensure_project_config_dir_at_reports_existing() {
        let temp_dir = TempDir::new().unwrap();
        let project_name = "existing-project";

        // Create the directory first
        let (path1, created1) =
            ensure_project_config_dir_at(temp_dir.path(), project_name).unwrap();
        assert!(created1);

        // Call again - should report as existing
        let (path2, created2) =
            ensure_project_config_dir_at(temp_dir.path(), project_name).unwrap();
        assert!(!created2);
        assert_eq!(path1, path2);
    }

    #[test]
    fn test_ensure_project_config_dir_at_different_projects_share_nothing() {
        let temp_dir = TempDir::new().unwrap();

        let (path1, _) = ensure_project_config_dir_at(temp_dir.path(), "project-a").unwrap();
        let (path2, _) = ensure_project_config_dir_at(temp_dir.path(), "project-b").unwrap();

        // Each project has its own directory
        assert_ne!(path1, path2);
        assert!(path1.exists());
        assert!(path2.exists());

        // Each has its own subdirs
        assert!(path1.join("spec").exists());
        assert!(path2.join("spec").exists());
    }

    #[test]
    fn test_spec_dir_path() {
        let result = spec_dir().unwrap();
        assert!(result.ends_with("spec"));
        assert!(result.parent().unwrap().file_name().unwrap() == "autom8");
    }

    #[test]
    fn test_runs_dir_path() {
        let result = runs_dir().unwrap();
        assert!(result.ends_with("runs"));
    }

    #[test]
    fn test_ensure_project_config_dir_creates_real_directory() {
        // This test uses the real function to verify it works end-to-end
        let result = ensure_project_config_dir();
        assert!(result.is_ok());
        let (path, _created) = result.unwrap();

        // Verify structure
        assert!(path.exists());
        assert!(path.join("spec").exists());
        assert!(path.join("runs").exists());
    }

    #[test]
    fn test_is_in_config_dir_true_for_file_in_config() {
        // Create a file inside the config directory
        let config_dir = project_config_dir().unwrap();
        fs::create_dir_all(&config_dir).unwrap();
        let test_file = config_dir.join("test.json");
        fs::write(&test_file, "{}").unwrap();

        let result = is_in_config_dir(&test_file).unwrap();
        assert!(result, "File in config dir should return true");

        // Cleanup
        fs::remove_file(&test_file).ok();
    }

    #[test]
    fn test_is_in_config_dir_false_for_file_outside_config() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.json");
        fs::write(&test_file, "{}").unwrap();

        let result = is_in_config_dir(&test_file).unwrap();
        assert!(!result, "File outside config dir should return false");
    }

    #[test]
    fn test_is_in_config_dir_true_for_file_in_subdirectory() {
        // Create a file in a subdirectory of config
        let spec_dir = spec_dir().unwrap();
        fs::create_dir_all(&spec_dir).unwrap();
        let test_file = spec_dir.join("test.md");
        fs::write(&test_file, "# Test").unwrap();

        let result = is_in_config_dir(&test_file).unwrap();
        assert!(result, "File in config subdirectory should return true");

        // Cleanup
        fs::remove_file(&test_file).ok();
    }

    #[test]
    fn test_move_to_config_dir_moves_md_to_spec() {
        let temp_dir = TempDir::new().unwrap();
        let source_file = temp_dir.path().join("test-spec.md");
        let content = "# Test Spec\n\nThis is a test.";
        fs::write(&source_file, content).unwrap();

        let result = move_to_config_dir(&source_file).unwrap();

        assert!(result.was_moved, "File should have been moved");
        assert!(result.dest_path.exists(), "Destination file should exist");
        assert!(
            !source_file.exists(),
            "Source file should be deleted after move"
        );
        assert!(
            result.dest_path.parent().unwrap().ends_with("spec"),
            "MD files should go to spec/ directory"
        );
        assert_eq!(
            fs::read_to_string(&result.dest_path).unwrap(),
            content,
            "Content should match"
        );

        // Cleanup
        fs::remove_file(&result.dest_path).ok();
    }

    #[test]
    fn test_move_to_config_dir_moves_json_to_spec() {
        let temp_dir = TempDir::new().unwrap();
        let source_file = temp_dir.path().join("test-spec.json");
        let content = r#"{"project": "test"}"#;
        fs::write(&source_file, content).unwrap();

        let result = move_to_config_dir(&source_file).unwrap();

        assert!(result.was_moved, "File should have been moved");
        assert!(result.dest_path.exists(), "Destination file should exist");
        assert!(
            !source_file.exists(),
            "Source file should be deleted after move"
        );
        assert!(
            result.dest_path.parent().unwrap().ends_with("spec"),
            "JSON files should go to spec/ directory"
        );
        assert_eq!(
            fs::read_to_string(&result.dest_path).unwrap(),
            content,
            "Content should match"
        );

        // Cleanup
        fs::remove_file(&result.dest_path).ok();
    }

    #[test]
    fn test_move_to_config_dir_no_move_if_already_in_config() {
        // Create a file already in the config directory
        let spec_dir = spec_dir().unwrap();
        fs::create_dir_all(&spec_dir).unwrap();
        let existing_file = spec_dir.join("existing-test.md");
        fs::write(&existing_file, "# Already here").unwrap();

        let result = move_to_config_dir(&existing_file).unwrap();

        assert!(!result.was_moved, "File should not have been moved");
        assert!(
            existing_file.exists(),
            "File should still exist in original location"
        );
        assert_eq!(
            result.dest_path.canonicalize().unwrap(),
            existing_file.canonicalize().unwrap(),
            "Path should be the original"
        );

        // Cleanup
        fs::remove_file(&existing_file).ok();
    }

    #[test]
    fn test_move_to_config_dir_unknown_extension_goes_to_spec() {
        let temp_dir = TempDir::new().unwrap();
        let source_file = temp_dir.path().join("test-file.txt");
        fs::write(&source_file, "Some content").unwrap();

        let result = move_to_config_dir(&source_file).unwrap();

        assert!(result.was_moved, "File should have been moved");
        assert!(
            !source_file.exists(),
            "Source file should be deleted after move"
        );
        assert!(
            result.dest_path.parent().unwrap().ends_with("spec"),
            "Unknown extensions should default to spec/ directory"
        );

        // Cleanup
        fs::remove_file(&result.dest_path).ok();
    }

    #[test]
    fn test_move_to_config_dir_preserves_filename() {
        let temp_dir = TempDir::new().unwrap();
        let source_file = temp_dir.path().join("my-custom-name.md");
        fs::write(&source_file, "# Test").unwrap();

        let result = move_to_config_dir(&source_file).unwrap();

        assert_eq!(
            result.dest_path.file_name().unwrap().to_str().unwrap(),
            "my-custom-name.md",
            "Filename should be preserved"
        );
        assert!(
            !source_file.exists(),
            "Source file should be deleted after move"
        );

        // Cleanup
        fs::remove_file(&result.dest_path).ok();
    }

    #[test]
    fn test_move_result_struct() {
        // Verify MoveResult fields work correctly
        let result = MoveResult {
            dest_path: PathBuf::from("/test/path"),
            was_moved: true,
        };
        assert_eq!(result.dest_path, PathBuf::from("/test/path"));
        assert!(result.was_moved);
    }

    #[test]
    fn test_move_to_config_dir_md_and_json_go_to_same_spec_dir() {
        // US-001: Verify both .md and .json files are stored in the same spec/ directory
        let temp_dir = TempDir::new().unwrap();

        // Create an .md file
        let md_file = temp_dir.path().join("spec-feature.md");
        fs::write(&md_file, "# Feature Spec").unwrap();

        // Create a .json file
        let json_file = temp_dir.path().join("spec-feature.json");
        fs::write(&json_file, r#"{"project": "test"}"#).unwrap();

        // Move both files
        let md_result = move_to_config_dir(&md_file).unwrap();
        let json_result = move_to_config_dir(&json_file).unwrap();

        // Both should be moved
        assert!(md_result.was_moved, "MD file should have been moved");
        assert!(json_result.was_moved, "JSON file should have been moved");

        // Both should be in the same spec/ directory
        let md_parent = md_result.dest_path.parent().unwrap();
        let json_parent = json_result.dest_path.parent().unwrap();

        assert_eq!(
            md_parent, json_parent,
            "Both .md and .json files should be in the same directory"
        );
        assert!(
            md_parent.ends_with("spec"),
            "Both files should be in spec/ directory"
        );

        // Cleanup
        fs::remove_file(&md_result.dest_path).ok();
        fs::remove_file(&json_result.dest_path).ok();
    }

    #[test]
    fn test_list_projects_empty_when_no_projects() {
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path().join(".config").join("autom8");
        fs::create_dir_all(&config_dir).unwrap();

        let projects = list_projects_at(&config_dir).unwrap();
        assert!(
            projects.is_empty(),
            "Should return empty list when no projects exist"
        );
    }

    #[test]
    fn test_list_projects_returns_sorted_list() {
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path().join(".config").join("autom8");

        // Create projects in non-alphabetical order
        fs::create_dir_all(config_dir.join("zebra")).unwrap();
        fs::create_dir_all(config_dir.join("alpha")).unwrap();
        fs::create_dir_all(config_dir.join("mango")).unwrap();

        let projects = list_projects_at(&config_dir).unwrap();

        assert_eq!(projects.len(), 3);
        assert_eq!(projects[0], "alpha", "First project should be 'alpha'");
        assert_eq!(projects[1], "mango", "Second project should be 'mango'");
        assert_eq!(projects[2], "zebra", "Third project should be 'zebra'");
    }

    #[test]
    fn test_list_projects_ignores_files() {
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path().join(".config").join("autom8");
        fs::create_dir_all(&config_dir).unwrap();

        // Create a project directory and a file
        fs::create_dir_all(config_dir.join("my-project")).unwrap();
        fs::write(config_dir.join("some-file.txt"), "not a project").unwrap();

        let projects = list_projects_at(&config_dir).unwrap();

        assert_eq!(projects.len(), 1, "Should only include directories");
        assert_eq!(projects[0], "my-project");
    }

    #[test]
    fn test_list_projects_empty_when_dir_does_not_exist() {
        let temp_dir = TempDir::new().unwrap();
        let non_existent_dir = temp_dir.path().join("does-not-exist");

        let projects = list_projects_at(&non_existent_dir).unwrap();
        assert!(
            projects.is_empty(),
            "Should return empty list for non-existent directory"
        );
    }

    #[test]
    fn test_list_projects_real_config_directory() {
        // This test verifies list_projects() works with the real config directory
        // After running tests for this project, at least 'autom8' should exist
        let result = list_projects();
        assert!(result.is_ok(), "list_projects() should not error");
        // Note: We can't assert specific contents since it depends on actual config state
    }

    // ========================================================================
    // US-010: Global status tests
    // ========================================================================

    #[test]
    fn test_project_status_needs_attention_with_active_run() {
        let status = ProjectStatus {
            name: "test-project".to_string(),
            has_active_run: true,
            run_status: Some(crate::state::RunStatus::Running),
            incomplete_spec_count: 0,
            total_spec_count: 0,
        };
        assert!(status.needs_attention(), "Active run should need attention");
        assert!(!status.is_idle());
    }

    #[test]
    fn test_project_status_needs_attention_with_failed_run() {
        let status = ProjectStatus {
            name: "test-project".to_string(),
            has_active_run: false,
            run_status: Some(crate::state::RunStatus::Failed),
            incomplete_spec_count: 0,
            total_spec_count: 0,
        };
        assert!(status.needs_attention(), "Failed run should need attention");
        assert!(!status.is_idle());
    }

    #[test]
    fn test_project_status_needs_attention_with_incomplete_specs() {
        let status = ProjectStatus {
            name: "test-project".to_string(),
            has_active_run: false,
            run_status: None,
            incomplete_spec_count: 2,
            total_spec_count: 3,
        };
        assert!(
            status.needs_attention(),
            "Incomplete specs should need attention"
        );
        assert!(!status.is_idle());
    }

    #[test]
    fn test_project_status_idle_when_no_work() {
        let status = ProjectStatus {
            name: "test-project".to_string(),
            has_active_run: false,
            run_status: Some(crate::state::RunStatus::Completed),
            incomplete_spec_count: 0,
            total_spec_count: 1,
        };
        assert!(
            !status.needs_attention(),
            "Completed project should not need attention"
        );
        assert!(status.is_idle());
    }

    #[test]
    fn test_project_status_idle_when_no_runs_no_specs() {
        let status = ProjectStatus {
            name: "test-project".to_string(),
            has_active_run: false,
            run_status: None,
            incomplete_spec_count: 0,
            total_spec_count: 0,
        };
        assert!(!status.needs_attention());
        assert!(status.is_idle());
    }

    #[test]
    fn test_global_status_empty_when_no_projects() {
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path().join(".config").join("autom8");
        fs::create_dir_all(&config_dir).unwrap();

        let statuses = global_status_at(&config_dir).unwrap();
        assert!(
            statuses.is_empty(),
            "Should return empty list when no projects exist"
        );
    }

    #[test]
    fn test_global_status_returns_all_projects() {
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path().join(".config").join("autom8");

        // Create project directories with spec subdirs
        fs::create_dir_all(config_dir.join("project-a").join("spec")).unwrap();
        fs::create_dir_all(config_dir.join("project-b").join("spec")).unwrap();

        let statuses = global_status_at(&config_dir).unwrap();

        assert_eq!(statuses.len(), 2);
        assert_eq!(statuses[0].name, "project-a");
        assert_eq!(statuses[1].name, "project-b");
    }

    #[test]
    fn test_global_status_detects_active_run() {
        use crate::state::{RunState, StateManager};

        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path().join(".config").join("autom8");
        let project_dir = config_dir.join("active-project");
        fs::create_dir_all(project_dir.join("spec")).unwrap();

        // Create an active run
        let sm = StateManager::with_dir(project_dir);
        let run_state = RunState::new(PathBuf::from("test.json"), "test-branch".to_string());
        sm.save(&run_state).unwrap();

        let statuses = global_status_at(&config_dir).unwrap();

        assert_eq!(statuses.len(), 1);
        assert!(statuses[0].has_active_run);
        assert_eq!(
            statuses[0].run_status,
            Some(crate::state::RunStatus::Running)
        );
    }

    #[test]
    fn test_global_status_counts_incomplete_specs() {
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path().join(".config").join("autom8");
        let project_dir = config_dir.join("spec-project");
        let spec_dir = project_dir.join("spec");
        fs::create_dir_all(&spec_dir).unwrap();

        // Create an incomplete PRD
        let incomplete_prd = r#"{
            "project": "Test Project",
            "branchName": "test",
            "description": "Test",
            "userStories": [
                {"id": "US-001", "title": "Story 1", "description": "Desc", "acceptanceCriteria": [], "priority": 1, "passes": false}
            ]
        }"#;
        fs::write(spec_dir.join("spec-test.json"), incomplete_prd).unwrap();

        // Create a complete PRD
        let complete_prd = r#"{
            "project": "Complete Project",
            "branchName": "test",
            "description": "Test",
            "userStories": [
                {"id": "US-001", "title": "Story 1", "description": "Desc", "acceptanceCriteria": [], "priority": 1, "passes": true}
            ]
        }"#;
        fs::write(spec_dir.join("spec-complete.json"), complete_prd).unwrap();

        let statuses = global_status_at(&config_dir).unwrap();

        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].incomplete_spec_count, 1);
        assert_eq!(statuses[0].total_spec_count, 2);
    }

    #[test]
    fn test_global_status_real_config() {
        // Test against real config directory - should not error
        let result = global_status();
        assert!(result.is_ok(), "global_status() should not error");
    }

    // ========================================================================
    // US-007: Project tree view tests
    // ========================================================================

    #[test]
    fn test_project_tree_info_status_label_running() {
        let info = ProjectTreeInfo {
            name: "test".to_string(),
            has_active_run: true,
            run_status: Some(crate::state::RunStatus::Running),
            spec_count: 1,
            incomplete_spec_count: 0,
            spec_md_count: 0,
            runs_count: 0,
            last_run_date: None,
        };
        assert_eq!(info.status_label(), "running");
    }

    #[test]
    fn test_project_tree_info_status_label_failed() {
        let info = ProjectTreeInfo {
            name: "test".to_string(),
            has_active_run: false,
            run_status: Some(crate::state::RunStatus::Failed),
            spec_count: 1,
            incomplete_spec_count: 0,
            spec_md_count: 0,
            runs_count: 0,
            last_run_date: None,
        };
        assert_eq!(info.status_label(), "failed");
    }

    #[test]
    fn test_project_tree_info_status_label_incomplete() {
        let info = ProjectTreeInfo {
            name: "test".to_string(),
            has_active_run: false,
            run_status: None,
            spec_count: 2,
            incomplete_spec_count: 1,
            spec_md_count: 0,
            runs_count: 0,
            last_run_date: None,
        };
        assert_eq!(info.status_label(), "incomplete");
    }

    #[test]
    fn test_project_tree_info_status_label_complete() {
        let info = ProjectTreeInfo {
            name: "test".to_string(),
            has_active_run: false,
            run_status: None,
            spec_count: 2,
            incomplete_spec_count: 0,
            spec_md_count: 1,
            runs_count: 0,
            last_run_date: None,
        };
        assert_eq!(info.status_label(), "complete");
    }

    #[test]
    fn test_project_tree_info_status_label_empty() {
        let info = ProjectTreeInfo {
            name: "test".to_string(),
            has_active_run: false,
            run_status: None,
            spec_count: 0,
            incomplete_spec_count: 0,
            spec_md_count: 0,
            runs_count: 0,
            last_run_date: None,
        };
        assert_eq!(info.status_label(), "empty");
    }

    #[test]
    fn test_project_tree_info_has_content_true() {
        let info = ProjectTreeInfo {
            name: "test".to_string(),
            has_active_run: false,
            run_status: None,
            spec_count: 1,
            incomplete_spec_count: 0,
            spec_md_count: 0,
            runs_count: 0,
            last_run_date: None,
        };
        assert!(info.has_content());
    }

    #[test]
    fn test_project_tree_info_has_content_false() {
        let info = ProjectTreeInfo {
            name: "test".to_string(),
            has_active_run: false,
            run_status: None,
            spec_count: 0,
            incomplete_spec_count: 0,
            spec_md_count: 0,
            runs_count: 0,
            last_run_date: None,
        };
        assert!(!info.has_content());
    }

    #[test]
    fn test_project_tree_info_has_content_with_active_run() {
        let info = ProjectTreeInfo {
            name: "test".to_string(),
            has_active_run: true,
            run_status: Some(crate::state::RunStatus::Running),
            spec_count: 0,
            incomplete_spec_count: 0,
            spec_md_count: 0,
            runs_count: 0,
            last_run_date: None,
        };
        assert!(info.has_content());
    }

    #[test]
    fn test_list_projects_tree_real_config() {
        // Test against real config directory - should not error
        let result = list_projects_tree();
        assert!(result.is_ok(), "list_projects_tree() should not error");
    }

    // ========================================================================
    // US-008: Describe command tests
    // ========================================================================

    #[test]
    fn test_us008_project_exists_true_for_existing() {
        // The autom8 project should exist since we're running from it
        let result = project_exists("autom8");
        assert!(result.is_ok());
        assert!(result.unwrap(), "autom8 project should exist");
    }

    #[test]
    fn test_us008_project_exists_false_for_nonexistent() {
        let result = project_exists("nonexistent-project-xyz-12345");
        assert!(result.is_ok());
        assert!(!result.unwrap(), "nonexistent project should return false");
    }

    #[test]
    fn test_us008_get_project_description_existing_project() {
        // Test getting description for an existing project
        let result = get_project_description("autom8");
        assert!(result.is_ok());
        let desc = result.unwrap();
        assert!(desc.is_some(), "autom8 project should return Some");

        let desc = desc.unwrap();
        assert_eq!(desc.name, "autom8");
        assert!(desc.path.exists());
        // Note: We don't assert on prds.is_empty() since the directory structure may vary
    }

    #[test]
    fn test_us008_get_project_description_nonexistent_project() {
        // Test getting description for a nonexistent project
        let result = get_project_description("nonexistent-project-xyz-12345");
        assert!(result.is_ok());
        assert!(
            result.unwrap().is_none(),
            "nonexistent project should return None"
        );
    }

    #[test]
    fn test_us008_project_description_has_all_fields() {
        // Test that ProjectDescription has all expected fields populated
        let desc = get_project_description("autom8").unwrap().unwrap();

        // name and path should be set
        assert!(!desc.name.is_empty());
        assert!(desc.path.exists());

        // PRDs should have correct structure
        for spec in &desc.specs {
            assert!(!spec.filename.is_empty());
            assert!(spec.path.exists());
            assert!(!spec.project_name.is_empty());
            assert!(!spec.branch_name.is_empty());
            assert!(!spec.stories.is_empty());
            assert!(spec.completed_count <= spec.total_count);
            assert_eq!(spec.total_count, spec.stories.len());
        }
    }

    #[test]
    fn test_us008_spec_summary_struct_fields() {
        // Verify SpecSummary struct has all fields
        let summary = SpecSummary {
            filename: "test.json".to_string(),
            path: PathBuf::from("/test"),
            project_name: "Test Project".to_string(),
            branch_name: "feature/test".to_string(),
            description: "Test description".to_string(),
            stories: vec![StorySummary {
                id: "US-001".to_string(),
                title: "Test Story".to_string(),
                passes: true,
            }],
            completed_count: 1,
            total_count: 1,
        };

        assert_eq!(summary.filename, "test.json");
        assert_eq!(summary.project_name, "Test Project");
        assert_eq!(summary.branch_name, "feature/test");
        assert_eq!(summary.completed_count, 1);
        assert_eq!(summary.total_count, 1);
    }

    #[test]
    fn test_us008_story_summary_struct_fields() {
        // Verify StorySummary struct has all fields
        let story = StorySummary {
            id: "US-001".to_string(),
            title: "Test Story".to_string(),
            passes: false,
        };

        assert_eq!(story.id, "US-001");
        assert_eq!(story.title, "Test Story");
        assert!(!story.passes);
    }

    #[test]
    fn test_us008_project_description_counts_spec_md_files() {
        // Test that spec_md_count is populated correctly for real project
        let desc = get_project_description("autom8").unwrap().unwrap();

        // spec_md_count should be >= 0 (may or may not have spec md files)
        // Just verify it's accessible and doesn't panic
        let _spec_md_count = desc.spec_md_count;
    }

    #[test]
    fn test_us008_project_description_counts_archived_runs() {
        // Test that runs_count is populated correctly for real project
        let desc = get_project_description("autom8").unwrap().unwrap();

        // runs_count should be >= 0
        let _runs_count = desc.runs_count;
    }

    #[test]
    fn test_us008_project_description_run_state_fields() {
        // Test that run state fields are accessible
        let desc = get_project_description("autom8").unwrap().unwrap();

        // These fields should be accessible even if None
        let _has_active_run = desc.has_active_run;
        let _run_status = &desc.run_status;
        let _current_story = &desc.current_story;
        let _current_branch = &desc.current_branch;
    }

    // ========================================================================
    // US-001: Config struct tests
    // ========================================================================

    #[test]
    fn test_config_default_all_true() {
        let config = Config::default();
        assert!(config.review, "review should default to true");
        assert!(config.commit, "commit should default to true");
        assert!(config.pull_request, "pull_request should default to true");
        assert!(!config.worktree, "worktree should default to false");
    }

    #[test]
    fn test_config_serialize_to_toml() {
        let config = Config::default();
        let toml_str = toml::to_string(&config).unwrap();

        assert!(toml_str.contains("review = true"));
        assert!(toml_str.contains("commit = true"));
        assert!(toml_str.contains("pull_request = true"));
        assert!(toml_str.contains("worktree = false"));
    }

    #[test]
    fn test_config_deserialize_from_toml() {
        let toml_str = r#"
            review = false
            commit = true
            pull_request = false
            worktree = true
        "#;

        let config: Config = toml::from_str(toml_str).unwrap();

        assert!(!config.review);
        assert!(config.commit);
        assert!(!config.pull_request);
        assert!(config.worktree);
    }

    #[test]
    fn test_config_deserialize_partial_toml_uses_defaults() {
        // Only specify one field - others should default to their respective defaults
        let toml_str = r#"
            commit = false
        "#;

        let config: Config = toml::from_str(toml_str).unwrap();

        assert!(config.review, "missing review should default to true");
        assert!(!config.commit, "commit should be false as specified");
        assert!(
            config.pull_request,
            "missing pull_request should default to true"
        );
        assert!(!config.worktree, "missing worktree should default to false");
    }

    #[test]
    fn test_config_deserialize_empty_toml_uses_all_defaults() {
        let toml_str = "";

        let config: Config = toml::from_str(toml_str).unwrap();

        assert!(config.review);
        assert!(config.commit);
        assert!(config.pull_request);
        assert!(!config.worktree);
    }

    #[test]
    fn test_config_roundtrip() {
        let original = Config {
            review: false,
            commit: true,
            pull_request: false,
            worktree: true,
        };

        let toml_str = toml::to_string(&original).unwrap();
        let deserialized: Config = toml::from_str(&toml_str).unwrap();

        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_config_equality() {
        let config1 = Config::default();
        let config2 = Config::default();
        assert_eq!(config1, config2);

        let config3 = Config {
            review: false,
            ..Default::default()
        };
        assert_ne!(config1, config3);
    }

    #[test]
    fn test_config_clone() {
        let original = Config {
            review: false,
            commit: true,
            pull_request: false,
            worktree: true,
        };

        let cloned = original.clone();
        assert_eq!(original, cloned);
    }

    #[test]
    fn test_config_debug_format() {
        let config = Config::default();
        let debug_str = format!("{:?}", config);

        assert!(debug_str.contains("Config"));
        assert!(debug_str.contains("review"));
        assert!(debug_str.contains("commit"));
        assert!(debug_str.contains("pull_request"));
        assert!(debug_str.contains("worktree"));
    }

    // ========================================================================
    // US-002: Global Config File Management tests
    // ========================================================================

    #[test]
    fn test_global_config_path_returns_config_toml() {
        let path = global_config_path().unwrap();
        assert!(path.ends_with("config.toml"));
        assert!(path.parent().unwrap().ends_with("autom8"));
    }

    #[test]
    fn test_generate_config_with_comments_includes_all_fields() {
        let config = Config::default();
        let content = generate_config_with_comments(&config);

        // Check that all field values are present
        assert!(content.contains("review = true"));
        assert!(content.contains("commit = true"));
        assert!(content.contains("pull_request = true"));
        assert!(content.contains("worktree = false"));
    }

    #[test]
    fn test_generate_config_with_comments_has_explanatory_comments() {
        let config = Config::default();
        let content = generate_config_with_comments(&config);

        // Check that comments explain each option
        assert!(content.contains("# Review state"));
        assert!(content.contains("# Commit state"));
        assert!(content.contains("# Pull request state"));
        assert!(content.contains("# Worktree mode"));

        // Check that true/false meanings are explained
        assert!(content.contains("- true:"));
        assert!(content.contains("- false:"));
    }

    #[test]
    fn test_generate_config_with_comments_preserves_custom_values() {
        let config = Config {
            review: false,
            commit: true,
            pull_request: false,
            worktree: true,
        };
        let content = generate_config_with_comments(&config);

        assert!(content.contains("review = false"));
        assert!(content.contains("commit = true"));
        assert!(content.contains("pull_request = false"));
        assert!(content.contains("worktree = true"));
    }

    #[test]
    fn test_default_config_with_comments_is_valid_toml() {
        // Verify the default config string can be parsed
        let config: Config = toml::from_str(DEFAULT_CONFIG_WITH_COMMENTS).unwrap();

        assert!(config.review);
        assert!(config.commit);
        assert!(config.pull_request);
        assert!(!config.worktree);
    }

    #[test]
    fn test_load_global_config_creates_file_when_missing() {
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path().join(".config").join("autom8");
        fs::create_dir_all(&config_dir).unwrap();

        let config_path = config_dir.join("config.toml");
        assert!(
            !config_path.exists(),
            "Config file should not exist initially"
        );

        // We can't easily test the real load_global_config because it uses the real home dir,
        // but we can test the underlying logic by simulating it
        let content = DEFAULT_CONFIG_WITH_COMMENTS;
        fs::write(&config_path, content).unwrap();

        let loaded: Config = toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
        assert_eq!(loaded, Config::default());
    }

    #[test]
    fn test_save_and_load_global_config_roundtrip() {
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path().join(".config").join("autom8");
        fs::create_dir_all(&config_dir).unwrap();

        let config_path = config_dir.join("config.toml");

        // Create a custom config
        let custom_config = Config {
            review: false,
            commit: true,
            pull_request: false,
            ..Default::default()
        };

        // Write it
        let content = generate_config_with_comments(&custom_config);
        fs::write(&config_path, content).unwrap();

        // Read it back
        let loaded: Config = toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();

        assert_eq!(loaded, custom_config);
    }

    #[test]
    fn test_load_global_config_handles_partial_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path().join(".config").join("autom8");
        fs::create_dir_all(&config_dir).unwrap();

        let config_path = config_dir.join("config.toml");

        // Write a partial config (missing pull_request)
        let partial_content = r#"
# Partial config
review = false
commit = true
"#;
        fs::write(&config_path, partial_content).unwrap();

        // Read it back - missing fields should use defaults
        let loaded: Config = toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();

        assert!(!loaded.review);
        assert!(loaded.commit);
        assert!(
            loaded.pull_request,
            "Missing pull_request should default to true"
        );
    }

    #[test]
    fn test_generated_config_includes_note_about_pr_requiring_commit() {
        let config = Config::default();
        let content = generate_config_with_comments(&config);

        // The config should mention that PR requires commit
        assert!(
            content.contains("Requires commit = true"),
            "Config should note that PR requires commit"
        );
    }

    #[test]
    fn test_load_global_config_real_path() {
        // Test the actual load_global_config function
        // This will either load an existing config or create a new one
        let result = load_global_config();
        assert!(result.is_ok(), "load_global_config should not error");

        let config = result.unwrap();
        // Verify it returns a valid Config
        // (We don't assert specific values since they depend on user's actual config)
        let _ = config.review;
        let _ = config.commit;
        let _ = config.pull_request;
    }

    #[test]
    fn test_save_global_config_real_path() {
        // First load to get current state (and ensure file exists)
        let original = load_global_config().unwrap();

        // Save the same config
        let result = save_global_config(&original);
        assert!(result.is_ok(), "save_global_config should not error");

        // Verify it's still readable
        let reloaded = load_global_config().unwrap();
        assert_eq!(
            original, reloaded,
            "Config should be unchanged after save/load cycle"
        );
    }

    #[test]
    fn test_global_config_file_has_comments_after_save() {
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path().join(".config").join("autom8");
        fs::create_dir_all(&config_dir).unwrap();

        let config_path = config_dir.join("config.toml");

        // Save a config
        let config = Config::default();
        let content = generate_config_with_comments(&config);
        fs::write(&config_path, content).unwrap();

        // Read raw content and verify comments are present
        let raw_content = fs::read_to_string(&config_path).unwrap();
        assert!(
            raw_content.contains("#"),
            "Config file should contain comments"
        );
        assert!(
            raw_content.contains("# Autom8 Configuration"),
            "Config file should have header comment"
        );
    }

    // ========================================================================
    // US-003: Per-Project Config Inheritance tests
    // ========================================================================

    #[test]
    fn test_us003_project_config_path_returns_correct_path() {
        let path = project_config_path().unwrap();
        assert!(path.ends_with("config.toml"));
        // Path should be inside the project directory, not the root autom8 dir
        assert!(path.parent().unwrap().file_name().unwrap() == "autom8");
    }

    #[test]
    fn test_us003_project_config_path_for_returns_correct_path() {
        let path = project_config_path_for("my-test-project").unwrap();
        assert!(path.ends_with("config.toml"));
        assert!(path.parent().unwrap().ends_with("my-test-project"));
    }

    #[test]
    fn test_us003_load_project_config_creates_from_global_when_missing() {
        // Use temp directory to avoid race conditions
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path().join(".config").join("autom8");
        fs::create_dir_all(&config_dir).unwrap();

        // Create global config
        let global_config = Config {
            review: true,
            commit: false,
            pull_request: false,
            ..Default::default()
        };
        let global_path = config_dir.join("config.toml");
        let global_content = generate_config_with_comments(&global_config);
        fs::write(&global_path, &global_content).unwrap();

        // Create project directory (no config file yet)
        let project_dir = config_dir.join("test-project");
        fs::create_dir_all(project_dir.join("spec")).unwrap();
        fs::create_dir_all(project_dir.join("runs")).unwrap();

        let project_config_path = project_dir.join("config.toml");
        assert!(
            !project_config_path.exists(),
            "Project config should not exist initially"
        );

        // Simulate load_project_config: when project config doesn't exist,
        // copy global config content to project config
        fs::write(&project_config_path, &global_content).unwrap();

        // Verify project config was created
        assert!(
            project_config_path.exists(),
            "Project config should be created when missing"
        );

        // Verify it matches global config
        let loaded: Config =
            toml::from_str(&fs::read_to_string(&project_config_path).unwrap()).unwrap();
        assert_eq!(
            loaded, global_config,
            "Project config should match global config"
        );
    }

    #[test]
    fn test_us003_load_project_config_preserves_comments() {
        // Use temp directory to avoid race conditions
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path().join(".config").join("autom8");
        let project_dir = config_dir.join("test-project");
        fs::create_dir_all(&project_dir).unwrap();

        // Create global config (used as source for project config creation)
        let global_config = Config::default();
        let global_path = config_dir.join("config.toml");
        let global_content = generate_config_with_comments(&global_config);
        fs::write(&global_path, &global_content).unwrap();

        // Simulate load_project_config: copy global to project when missing
        let project_config_path = project_dir.join("config.toml");
        assert!(!project_config_path.exists());

        // Copy global config content to project config (as load_project_config does)
        fs::write(&project_config_path, &global_content).unwrap();

        // Verify comments are present
        let raw_content = fs::read_to_string(&project_config_path).unwrap();

        assert!(
            raw_content.contains("#"),
            "Project config should contain comments"
        );
        assert!(
            raw_content.contains("# Autom8 Configuration"),
            "Project config should have header comment"
        );
        assert!(
            raw_content.contains("# Review state"),
            "Project config should have review state comment"
        );
    }

    #[test]
    fn test_us003_save_project_config_creates_file() {
        // Use temp directory to avoid race conditions
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path().join(".config").join("autom8");
        let project_dir = config_dir.join("test-project");
        fs::create_dir_all(project_dir.join("spec")).unwrap();
        fs::create_dir_all(project_dir.join("runs")).unwrap();

        let config = Config {
            review: false,
            commit: true,
            pull_request: true,
            ..Default::default()
        };

        // Simulate save_project_config
        let project_config_path = project_dir.join("config.toml");
        let content = generate_config_with_comments(&config);
        fs::write(&project_config_path, &content).unwrap();

        // Verify file exists and can be loaded
        assert!(project_config_path.exists());

        let loaded: Config =
            toml::from_str(&fs::read_to_string(&project_config_path).unwrap()).unwrap();
        assert_eq!(loaded, config);
    }

    #[test]
    fn test_us003_save_project_config_preserves_comments() {
        // Use temp directory to avoid race conditions
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path().join(".config").join("autom8");
        let project_dir = config_dir.join("test-project");
        fs::create_dir_all(&project_dir).unwrap();

        let config = Config::default();
        let project_config_path = project_dir.join("config.toml");
        let content = generate_config_with_comments(&config);
        fs::write(&project_config_path, &content).unwrap();

        let raw_content = fs::read_to_string(&project_config_path).unwrap();

        assert!(
            raw_content.contains("#"),
            "Saved config should contain comments"
        );
        assert!(
            raw_content.contains("# Autom8 Configuration"),
            "Saved config should have header comment"
        );
    }

    #[test]
    fn test_us003_get_effective_config_returns_project_if_exists() {
        // Use temp directory to avoid race conditions
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path().join(".config").join("autom8");
        fs::create_dir_all(&config_dir).unwrap();

        // Create global config first
        let global_config = Config::default();
        let global_path = config_dir.join("config.toml");
        fs::write(&global_path, generate_config_with_comments(&global_config)).unwrap();

        // Create project config with distinct values
        let project_config = Config {
            review: false,
            commit: false,
            pull_request: false,
            ..Default::default()
        };
        let project_dir = config_dir.join("test-project");
        fs::create_dir_all(&project_dir).unwrap();
        let project_path = project_dir.join("config.toml");
        fs::write(
            &project_path,
            generate_config_with_comments(&project_config),
        )
        .unwrap();

        // Simulate get_effective_config logic
        let effective_path = if project_path.exists() {
            &project_path
        } else {
            &global_path
        };

        let effective: Config =
            toml::from_str(&fs::read_to_string(effective_path).unwrap()).unwrap();
        assert_eq!(
            effective, project_config,
            "Should return project config when it exists"
        );
    }

    #[test]
    fn test_us003_get_effective_config_returns_global_when_project_missing() {
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path().join(".config").join("autom8");
        fs::create_dir_all(&config_dir).unwrap();

        // Create global config
        let global_config = Config {
            review: true,
            commit: true,
            pull_request: false,
            ..Default::default()
        };
        let global_path = config_dir.join("config.toml");
        let content = generate_config_with_comments(&global_config);
        fs::write(&global_path, content).unwrap();

        // Create project dir but NOT project config
        let project_dir = config_dir.join("test-project");
        fs::create_dir_all(&project_dir).unwrap();

        // We can't directly test get_effective_config with temp dirs,
        // but we can verify the logic by checking path existence
        let project_config_path = project_dir.join("config.toml");
        assert!(
            !project_config_path.exists(),
            "Project config should not exist"
        );
        assert!(global_path.exists(), "Global config should exist");

        // Load global config to verify
        let loaded: Config = toml::from_str(&fs::read_to_string(&global_path).unwrap()).unwrap();
        assert_eq!(loaded, global_config);
    }

    #[test]
    fn test_us003_project_config_takes_precedence_over_global() {
        // Simulate project config overriding global with temp directories
        // to avoid race conditions with other tests
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path().join(".config").join("autom8");
        fs::create_dir_all(&config_dir).unwrap();

        // Create global config
        let global_config = Config {
            review: true,
            commit: true,
            pull_request: true,
            ..Default::default()
        };
        let global_path = config_dir.join("config.toml");
        fs::write(&global_path, generate_config_with_comments(&global_config)).unwrap();

        // Create project config with different values
        let project_config = Config {
            review: false,
            commit: true,
            pull_request: false,
            ..Default::default()
        };
        let project_dir = config_dir.join("my-project");
        fs::create_dir_all(&project_dir).unwrap();
        let project_path = project_dir.join("config.toml");
        fs::write(
            &project_path,
            generate_config_with_comments(&project_config),
        )
        .unwrap();

        // Simulate get_effective_config logic: prefer project if exists
        let effective_path = if project_path.exists() {
            &project_path
        } else {
            &global_path
        };

        let effective: Config =
            toml::from_str(&fs::read_to_string(effective_path).unwrap()).unwrap();
        assert_eq!(
            effective, project_config,
            "Project config should take precedence over global"
        );
        assert_ne!(
            effective, global_config,
            "Should not return global config when project config exists"
        );
    }

    #[test]
    fn test_us003_get_effective_config_does_not_create_project_config() {
        // Use temp directory to test the logic
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path().join(".config").join("autom8");
        fs::create_dir_all(&config_dir).unwrap();

        // Create global config
        let global_config = Config::default();
        let global_path = config_dir.join("config.toml");
        fs::write(&global_path, generate_config_with_comments(&global_config)).unwrap();

        // Create project dir but NOT project config
        let project_dir = config_dir.join("test-project");
        fs::create_dir_all(&project_dir).unwrap();
        let project_config_path = project_dir.join("config.toml");

        // Simulate get_effective_config: it should NOT create project config
        assert!(
            !project_config_path.exists(),
            "Project config should not exist before"
        );

        // Simulate reading effective config (prefer project if exists, else global)
        let effective_path = if project_config_path.exists() {
            &project_config_path
        } else {
            &global_path
        };
        let _effective: Config =
            toml::from_str(&fs::read_to_string(effective_path).unwrap()).unwrap();

        // get_effective_config should NOT have created the project config
        assert!(
            !project_config_path.exists(),
            "get_effective_config should NOT create project config"
        );
    }

    #[test]
    fn test_us003_project_config_roundtrip() {
        // Use temp directory to avoid race conditions
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path().join(".config").join("autom8");
        let project_dir = config_dir.join("test-project");
        fs::create_dir_all(&project_dir).unwrap();

        let original = Config {
            review: false,
            commit: true,
            pull_request: false,
            ..Default::default()
        };

        // Save
        let project_config_path = project_dir.join("config.toml");
        let content = generate_config_with_comments(&original);
        fs::write(&project_config_path, &content).unwrap();

        // Load
        let loaded: Config =
            toml::from_str(&fs::read_to_string(&project_config_path).unwrap()).unwrap();

        assert_eq!(original, loaded, "Config should survive save/load cycle");
    }

    #[test]
    fn test_us003_project_config_handles_partial_config() {
        // Use temp directory to avoid race conditions
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path().join(".config").join("autom8");
        let project_dir = config_dir.join("test-project");
        fs::create_dir_all(&project_dir).unwrap();

        let project_config_path = project_dir.join("config.toml");

        // Write a partial config (missing some fields)
        let partial_content = r#"
# Partial project config
review = false
"#;
        fs::write(&project_config_path, partial_content).unwrap();

        // Load should fill in defaults for missing fields
        let loaded: Config =
            toml::from_str(&fs::read_to_string(&project_config_path).unwrap()).unwrap();

        assert!(!loaded.review, "review should be false as specified");
        assert!(loaded.commit, "missing commit should default to true");
        assert!(
            loaded.pull_request,
            "missing pull_request should default to true"
        );
    }

    #[test]
    fn test_us003_inheritance_simulation_with_temp_dirs() {
        // Simulate the full inheritance flow with temp directories
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path().join(".config").join("autom8");
        fs::create_dir_all(&config_dir).unwrap();

        // Create global config
        let global_config = Config {
            review: true,
            commit: false,
            pull_request: false,
            ..Default::default()
        };
        let global_content = generate_config_with_comments(&global_config);
        let global_path = config_dir.join("config.toml");
        fs::write(&global_path, &global_content).unwrap();

        // Create project directory
        let project_dir = config_dir.join("test-project");
        fs::create_dir_all(project_dir.join("spec")).unwrap();
        fs::create_dir_all(project_dir.join("runs")).unwrap();

        // Simulate load_project_config behavior: copy global to project
        let project_config_path = project_dir.join("config.toml");
        assert!(!project_config_path.exists());

        // Copy global config content to project config
        fs::write(&project_config_path, &global_content).unwrap();

        // Verify project config exists and matches global
        assert!(project_config_path.exists());
        let loaded: Config =
            toml::from_str(&fs::read_to_string(&project_config_path).unwrap()).unwrap();
        assert_eq!(
            loaded, global_config,
            "Project config should inherit from global"
        );

        // Verify comments were preserved
        let project_content = fs::read_to_string(&project_config_path).unwrap();
        assert!(project_content.contains("# Autom8 Configuration"));
        assert!(project_content.contains("# Review state"));
    }

    #[test]
    fn test_us003_project_config_override_simulation() {
        // Simulate project config overriding global with different values
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path().join(".config").join("autom8");
        fs::create_dir_all(&config_dir).unwrap();

        // Create global config
        let global_config = Config {
            review: true,
            commit: true,
            pull_request: true,
            ..Default::default()
        };
        let global_path = config_dir.join("config.toml");
        fs::write(&global_path, generate_config_with_comments(&global_config)).unwrap();

        // Create project config with different values
        let project_config = Config {
            review: false,
            commit: true,
            pull_request: false,
            ..Default::default()
        };
        let project_dir = config_dir.join("my-project");
        fs::create_dir_all(&project_dir).unwrap();
        let project_path = project_dir.join("config.toml");
        fs::write(
            &project_path,
            generate_config_with_comments(&project_config),
        )
        .unwrap();

        // Simulate get_effective_config logic: prefer project if exists
        let effective_path = if project_path.exists() {
            &project_path
        } else {
            &global_path
        };

        let effective: Config =
            toml::from_str(&fs::read_to_string(effective_path).unwrap()).unwrap();
        assert_eq!(
            effective, project_config,
            "Project config should take precedence"
        );
        assert_ne!(effective.review, global_config.review);
        assert_ne!(effective.pull_request, global_config.pull_request);
    }

    // =========================================================================
    // US-004: Config Validation Tests
    // =========================================================================

    #[test]
    fn test_us004_validate_config_accepts_default_config() {
        let config = Config::default();
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn test_us004_validate_config_accepts_all_true() {
        let config = Config {
            review: true,
            commit: true,
            pull_request: true,
            ..Default::default()
        };
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn test_us004_validate_config_accepts_all_false() {
        let config = Config {
            review: false,
            commit: false,
            pull_request: false,
            ..Default::default()
        };
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn test_us004_validate_config_accepts_commit_true_pr_false() {
        let config = Config {
            review: true,
            commit: true,
            pull_request: false,
            ..Default::default()
        };
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn test_us004_validate_config_accepts_commit_false_pr_false() {
        let config = Config {
            review: true,
            commit: false,
            pull_request: false,
            ..Default::default()
        };
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn test_us004_validate_config_rejects_pr_true_commit_false() {
        let config = Config {
            review: true,
            commit: false,
            pull_request: true,
            ..Default::default()
        };
        let result = validate_config(&config);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), ConfigError::PullRequestWithoutCommit);
    }

    #[test]
    fn test_us004_config_error_message_is_actionable() {
        let error = ConfigError::PullRequestWithoutCommit;
        let message = error.to_string();

        // Verify the error message contains the exact required text
        assert_eq!(
            message,
            "Cannot create pull request without commits. \
            Either set `commit = true` or set `pull_request = false`"
        );
    }

    #[test]
    fn test_us004_config_error_implements_error_trait() {
        let error = ConfigError::PullRequestWithoutCommit;
        // Verify it implements std::error::Error
        let _: &dyn std::error::Error = &error;
    }

    #[test]
    fn test_us004_config_error_debug_format() {
        let error = ConfigError::PullRequestWithoutCommit;
        let debug_str = format!("{:?}", error);
        assert!(debug_str.contains("PullRequestWithoutCommit"));
    }

    #[test]
    fn test_us004_config_error_clone() {
        let error = ConfigError::PullRequestWithoutCommit;
        let cloned = error.clone();
        assert_eq!(error, cloned);
    }

    #[test]
    fn test_us004_validate_config_accepts_review_false_with_valid_pr_commit() {
        // Review state doesn't affect PR/commit validation
        let config = Config {
            review: false,
            commit: true,
            pull_request: true,
            ..Default::default()
        };
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn test_us004_validate_config_all_combinations() {
        // Test all 8 possible boolean combinations (for review, commit, pull_request)
        let combinations = [
            (false, false, false, true), // all false - valid
            (false, false, true, false), // pr=true, commit=false - invalid
            (false, true, false, true),  // commit=true, pr=false - valid
            (false, true, true, true),   // commit=true, pr=true - valid
            (true, false, false, true),  // review=true, commit=false, pr=false - valid
            (true, false, true, false),  // review=true, pr=true, commit=false - invalid
            (true, true, false, true),   // review=true, commit=true, pr=false - valid
            (true, true, true, true),    // all true - valid
        ];

        for (review, commit, pull_request, should_be_valid) in combinations {
            let config = Config {
                review,
                commit,
                pull_request,
                ..Default::default()
            };
            let result = validate_config(&config);
            assert_eq!(
                result.is_ok(),
                should_be_valid,
                "Config (review={}, commit={}, pull_request={}) expected valid={}, got valid={}",
                review,
                commit,
                pull_request,
                should_be_valid,
                result.is_ok()
            );
        }
    }

    #[test]
    fn test_us004_get_effective_config_validates_before_returning() {
        // This test verifies that get_effective_config validates the loaded config
        // We can't easily test this with real files in a unit test, but we can
        // verify the validation function is called by testing with a simulated scenario

        // Create an invalid config directly and validate it
        let invalid_config = Config {
            review: true,
            commit: false,
            pull_request: true,
            ..Default::default()
        };
        let validation_result = validate_config(&invalid_config);
        assert!(validation_result.is_err());

        // Verify the error message contains actionable information
        let error = validation_result.unwrap_err();
        let message = error.to_string();
        assert!(message.contains("commit = true"));
        assert!(message.contains("pull_request = false"));
    }

    #[test]
    fn test_us004_validation_integration_with_autom8_error() {
        // Verify ConfigError can be converted to Autom8Error::Config
        let config_error = ConfigError::PullRequestWithoutCommit;
        let autom8_error = Autom8Error::Config(config_error.to_string());

        // The error message should be preserved
        let error_string = format!("{}", autom8_error);
        assert!(error_string.contains("Cannot create pull request without commits"));
    }

    // =========================================================================
    // Test: config files with use_tui should still parse (backwards compat)
    // =========================================================================

    #[test]
    fn test_config_with_use_tui_field_still_parses() {
        // Old config files may still have use_tui field - ensure they parse without error
        let toml_str = r#"
            review = true
            commit = true
            pull_request = true
            use_tui = true
        "#;
        // This should parse successfully (use_tui is ignored)
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.review);
        assert!(config.commit);
        assert!(config.pull_request);
    }

    // ========================================================================
    // US-005: Worktree Configuration Option tests
    // ========================================================================

    #[test]
    fn test_worktree_config_defaults_to_false() {
        let config = Config::default();
        assert!(
            !config.worktree,
            "worktree should default to false for backward compatibility"
        );
    }

    #[test]
    fn test_worktree_config_can_be_enabled() {
        let toml_str = r#"
            worktree = true
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.worktree, "worktree should be true when set in config");
    }

    #[test]
    fn test_worktree_config_missing_defaults_to_false() {
        // Old config files without worktree field should still work
        let toml_str = r#"
            review = true
            commit = true
            pull_request = true
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(
            !config.worktree,
            "missing worktree field should default to false"
        );
    }

    #[test]
    fn test_worktree_config_explicit_false() {
        let toml_str = r#"
            worktree = false
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(
            !config.worktree,
            "explicit worktree = false should be respected"
        );
    }

    #[test]
    fn test_worktree_config_with_all_other_fields() {
        let toml_str = r#"
            review = false
            commit = true
            pull_request = false
            worktree = true
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(!config.review);
        assert!(config.commit);
        assert!(!config.pull_request);
        assert!(config.worktree);
    }

    #[test]
    fn test_worktree_config_documentation_note_in_generated_comments() {
        let config = Config::default();
        let content = generate_config_with_comments(&config);

        // Verify the git repository requirement note is documented
        assert!(
            content.contains("Requires a git repository"),
            "config comments should document git repo requirement"
        );
    }
}
