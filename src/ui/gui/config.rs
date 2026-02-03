//! Config Tab module for the GUI.
//!
//! This module contains the types, state, and logic for the Config tab,
//! which allows users to view and edit both global and project-specific
//! configuration settings.

use std::collections::HashMap;
use std::time::Instant;

// ============================================================================
// Config Scope Types (Config Tab - US-002)
// ============================================================================

/// Represents the scope of configuration being edited.
///
/// The Config tab supports editing both global configuration and
/// per-project configuration. This enum represents which scope is
/// currently selected.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ConfigScope {
    /// Global configuration (`~/.config/autom8/config.toml`).
    /// This is the default selection when the Config tab is opened.
    #[default]
    Global,
    /// Project-specific configuration (`~/.config/autom8/<project>/config.toml`).
    /// Contains the project name.
    Project(String),
}

impl ConfigScope {
    /// Returns the display name for this scope.
    pub fn display_name(&self) -> &str {
        match self {
            ConfigScope::Global => "Global",
            ConfigScope::Project(name) => name,
        }
    }

    /// Returns whether this scope is the global scope.
    pub fn is_global(&self) -> bool {
        matches!(self, ConfigScope::Global)
    }
}

// ============================================================================
// Config Field Change Types (Config Tab - US-006)
// ============================================================================

/// Represents a change to a boolean config field (US-006).
///
/// When a toggle is clicked, the render method returns this change to indicate
/// which field was modified and its new value. The change is then processed
/// by the parent method which has mutable access to save the config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigBoolField {
    /// The `review` field.
    Review,
    /// The `commit` field.
    Commit,
    /// The `pull_request` field.
    PullRequest,
    /// The `worktree` field.
    Worktree,
    /// The `worktree_cleanup` field.
    WorktreeCleanup,
}

/// Identifier for text config fields (US-007).
///
/// Used to track which text field changed when processing editor actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigTextField {
    /// The `worktree_path_pattern` field.
    WorktreePathPattern,
}

/// Type alias for a collection of boolean field changes (US-006).
pub type BoolFieldChanges = Vec<(ConfigBoolField, bool)>;

/// Type alias for a collection of text field changes (US-007).
pub type TextFieldChanges = Vec<(ConfigTextField, String)>;

/// Actions that can be returned from config editor rendering (US-006, US-007, US-009).
///
/// This struct collects all actions that require mutation, allowing the
/// render methods to remain `&self` while the parent processes mutations.
#[derive(Debug, Default)]
pub struct ConfigEditorActions {
    /// If set, create a project config from global (US-005).
    pub create_project_config: Option<String>,
    /// Boolean field changes with (field, new_value) (US-006).
    pub bool_changes: Vec<(ConfigBoolField, bool)>,
    /// Text field changes with (field, new_value) (US-007).
    pub text_changes: Vec<(ConfigTextField, String)>,
    /// Whether we're editing global (true) or project (false) config.
    pub is_global: bool,
    /// Project name if editing project config.
    pub project_name: Option<String>,
    /// If true, reset the config to defaults (US-009).
    pub reset_to_defaults: bool,
}

// ============================================================================
// Config Scope Constants (Config Tab - US-002)
// ============================================================================

/// Height of each row in the config scope list.
pub const CONFIG_SCOPE_ROW_HEIGHT: f32 = 44.0;

/// Horizontal padding within config scope rows (uses MD from spacing scale).
pub const CONFIG_SCOPE_ROW_PADDING_H: f32 = 12.0; // spacing::MD

/// Vertical padding within config scope rows (uses SM from spacing scale).
pub const CONFIG_SCOPE_ROW_PADDING_V: f32 = 8.0; // spacing::SM

// ============================================================================
// Config Tab State
// ============================================================================

/// State for the Config tab.
///
/// This struct holds all the state needed for the Config tab, including
/// the currently selected scope, cached configurations, and UI state.
#[derive(Debug, Default)]
pub struct ConfigTabState {
    /// Currently selected config scope in the Config tab.
    /// Defaults to Global when the Config tab is first opened.
    pub selected_scope: ConfigScope,

    /// Cached list of project names for the config scope selector.
    /// Loaded from `~/.config/autom8/*/` directories.
    pub scope_projects: Vec<String>,

    /// Cached information about which projects have their own config file.
    /// Maps project name to whether it has a `config.toml` file.
    pub scope_has_config: HashMap<String, bool>,

    /// Cached global configuration for editing.
    /// Loaded via `config::load_global_config()` when Global scope is selected.
    pub cached_global_config: Option<crate::config::Config>,

    /// Error message if global config failed to load.
    pub global_config_error: Option<String>,

    /// Cached project configuration for editing.
    /// Loaded when a project with its own config file is selected.
    /// Key is the project name, value is the loaded config.
    pub cached_project_config: Option<(String, crate::config::Config)>,

    /// Error message if project config failed to load.
    pub project_config_error: Option<String>,

    /// Timestamp of the last config modification.
    /// Used to show the "Changes take effect on next run" notice.
    /// Set to Some(Instant) when a config field is modified, cleared after timeout.
    pub last_modified: Option<Instant>,
}

impl ConfigTabState {
    /// Create a new ConfigTabState with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the currently selected config scope.
    pub fn selected_scope(&self) -> &ConfigScope {
        &self.selected_scope
    }

    /// Sets the selected config scope.
    pub fn set_selected_scope(&mut self, scope: ConfigScope) {
        self.selected_scope = scope;
    }

    /// Returns the cached list of project names for config scope selection.
    pub fn scope_projects(&self) -> &[String] {
        &self.scope_projects
    }

    /// Returns whether a project has its own config file.
    pub fn project_has_config(&self, project_name: &str) -> bool {
        self.scope_has_config
            .get(project_name)
            .copied()
            .unwrap_or(false)
    }

    /// Refresh the config scope data (project list and config file status).
    /// Called when the Config tab is rendered or data needs to be refreshed.
    pub fn refresh_scope_data(&mut self) {
        // Load project list from config directory
        if let Ok(projects) = crate::config::list_projects() {
            self.scope_projects = projects;

            // Check which projects have their own config file
            self.scope_has_config.clear();
            for project in &self.scope_projects {
                if let Ok(config_path) = crate::config::project_config_path_for(project) {
                    self.scope_has_config
                        .insert(project.clone(), config_path.exists());
                }
            }
        }

        // Load global config when Global scope is selected
        if self.selected_scope.is_global() && self.cached_global_config.is_none() {
            self.load_global_config();
        }

        // Load project config when a project scope is selected (US-004)
        if let ConfigScope::Project(project_name) = &self.selected_scope {
            // Only load if not already cached for this project
            let needs_load = match &self.cached_project_config {
                Some((cached_name, _)) => cached_name != project_name,
                None => self.project_has_config(project_name),
            };
            if needs_load {
                let project_name = project_name.clone();
                self.load_project_config(&project_name);
            }
        }
    }

    /// Load the global configuration from disk.
    /// Called when Global scope is selected in the Config tab.
    pub fn load_global_config(&mut self) {
        match crate::config::load_global_config() {
            Ok(config) => {
                self.cached_global_config = Some(config);
                self.global_config_error = None;
            }
            Err(e) => {
                self.cached_global_config = None;
                self.global_config_error = Some(format!("Failed to load config: {}", e));
            }
        }
    }

    /// Returns the cached global config, if loaded.
    pub fn cached_global_config(&self) -> Option<&crate::config::Config> {
        self.cached_global_config.as_ref()
    }

    /// Returns the global config error, if any.
    pub fn global_config_error(&self) -> Option<&str> {
        self.global_config_error.as_deref()
    }

    /// Returns the cached project config for a specific project, if loaded.
    pub fn cached_project_config(&self, project_name: &str) -> Option<&crate::config::Config> {
        self.cached_project_config
            .as_ref()
            .filter(|(name, _)| name == project_name)
            .map(|(_, config)| config)
    }

    /// Returns the project config error, if any.
    pub fn project_config_error(&self) -> Option<&str> {
        self.project_config_error.as_deref()
    }

    /// Load project configuration for a specific project.
    pub fn load_project_config(&mut self, project_name: &str) {
        // Get the config file path for this project
        let config_path = match crate::config::project_config_path_for(project_name) {
            Ok(path) => path,
            Err(e) => {
                self.cached_project_config = None;
                self.project_config_error = Some(format!("Failed to get config path: {}", e));
                return;
            }
        };

        // Check if the config file exists
        if !config_path.exists() {
            self.cached_project_config = None;
            self.project_config_error = None;
            return;
        }

        // Read and parse the config file
        match std::fs::read_to_string(&config_path) {
            Ok(content) => match toml::from_str::<crate::config::Config>(&content) {
                Ok(config) => {
                    self.cached_project_config = Some((project_name.to_string(), config));
                    self.project_config_error = None;
                }
                Err(e) => {
                    self.cached_project_config = None;
                    self.project_config_error = Some(format!("Failed to parse config: {}", e));
                }
            },
            Err(e) => {
                self.cached_project_config = None;
                self.project_config_error = Some(format!("Failed to read config: {}", e));
            }
        }
    }

    /// Create a project config from the global config (US-005).
    ///
    /// Copies the global configuration values to create a new project-specific
    /// config file, then updates the UI state to reflect the new config.
    pub fn create_project_config_from_global(&mut self, project_name: &str) -> Result<(), String> {
        // Get the global config values (or defaults if not loaded)
        let global_config = self.cached_global_config.clone().unwrap_or_default();

        // Save as project config
        if let Err(e) = crate::config::save_project_config_for(project_name, &global_config) {
            return Err(format!("Failed to create project config: {}", e));
        }

        // Update our state to reflect the new config
        self.scope_has_config.insert(project_name.to_string(), true);
        self.cached_project_config = Some((project_name.to_string(), global_config));
        self.project_config_error = None;

        // Update modification timestamp to show notice
        self.last_modified = Some(Instant::now());

        Ok(())
    }

    /// Apply boolean field changes to the config (US-006).
    pub fn apply_bool_changes(
        &mut self,
        is_global: bool,
        project_name: Option<&str>,
        changes: &[(ConfigBoolField, bool)],
    ) {
        // Early return if no changes
        if changes.is_empty() {
            return;
        }

        // Get mutable reference to the appropriate config
        let config = if is_global {
            self.cached_global_config.as_mut()
        } else {
            // For project config, check that the project name matches
            match (&mut self.cached_project_config, project_name) {
                (Some((cached_name, config)), Some(project)) if cached_name == project => {
                    Some(config)
                }
                _ => None,
            }
        };

        let Some(config) = config else {
            return;
        };

        // Apply each change
        for (field, value) in changes {
            match field {
                ConfigBoolField::Review => config.review = *value,
                ConfigBoolField::Commit => config.commit = *value,
                ConfigBoolField::PullRequest => config.pull_request = *value,
                ConfigBoolField::Worktree => config.worktree = *value,
                ConfigBoolField::WorktreeCleanup => config.worktree_cleanup = *value,
            }
        }

        // Save the config
        let save_result = if is_global {
            crate::config::save_global_config(config)
        } else if let Some(project) = project_name {
            crate::config::save_project_config_for(project, config)
        } else {
            return;
        };

        if let Err(e) = save_result {
            if is_global {
                self.global_config_error = Some(format!("Failed to save config: {}", e));
            } else {
                self.project_config_error = Some(format!("Failed to save config: {}", e));
            }
        } else {
            // Update modification timestamp to show notice
            self.last_modified = Some(Instant::now());
        }
    }

    /// Apply text field changes to the config (US-007).
    pub fn apply_text_changes(
        &mut self,
        is_global: bool,
        project_name: Option<&str>,
        changes: &[(ConfigTextField, String)],
    ) {
        // Early return if no changes
        if changes.is_empty() {
            return;
        }

        // Get mutable reference to the appropriate config
        let config = if is_global {
            self.cached_global_config.as_mut()
        } else {
            // For project config, check that the project name matches
            match (&mut self.cached_project_config, project_name) {
                (Some((cached_name, config)), Some(project)) if cached_name == project => {
                    Some(config)
                }
                _ => None,
            }
        };

        let Some(config) = config else {
            return;
        };

        // Apply each change
        for (field, value) in changes {
            match field {
                ConfigTextField::WorktreePathPattern => {
                    config.worktree_path_pattern = value.clone();
                }
            }
        }

        // Save the config
        let save_result = if is_global {
            crate::config::save_global_config(config)
        } else if let Some(project) = project_name {
            crate::config::save_project_config_for(project, config)
        } else {
            return;
        };

        if let Err(e) = save_result {
            if is_global {
                self.global_config_error = Some(format!("Failed to save config: {}", e));
            } else {
                self.project_config_error = Some(format!("Failed to save config: {}", e));
            }
        } else {
            // Update modification timestamp to show notice
            self.last_modified = Some(Instant::now());
        }
    }

    /// Reset config to application defaults (US-009).
    ///
    /// Replaces the current config with `Config::default()` values:
    /// - review = true
    /// - commit = true
    /// - pull_request = true
    /// - worktree = true
    /// - worktree_path_pattern = "{repo}-wt-{branch}"
    /// - worktree_cleanup = false
    ///
    /// The config is saved immediately and the UI updates to reflect the new values.
    pub fn reset_to_defaults(&mut self, is_global: bool, project_name: Option<&str>) {
        let default_config = crate::config::Config::default();

        if is_global {
            // Reset global config
            self.cached_global_config = Some(default_config.clone());

            // Save to disk
            if let Err(e) = crate::config::save_global_config(&default_config) {
                self.global_config_error = Some(format!("Failed to save config: {}", e));
            } else {
                // Update modification timestamp to show notice
                self.last_modified = Some(Instant::now());
            }
        } else if let Some(project) = project_name {
            // Reset project config
            self.cached_project_config = Some((project.to_string(), default_config.clone()));

            // Save to disk
            if let Err(e) = crate::config::save_project_config_for(project, &default_config) {
                self.project_config_error = Some(format!("Failed to save config: {}", e));
            } else {
                // Update modification timestamp to show notice
                self.last_modified = Some(Instant::now());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // Config Tab Tests (US-001)
    // ========================================================================

    #[test]
    fn test_config_scope_enum_global_default() {
        let scope = ConfigScope::default();
        assert!(matches!(scope, ConfigScope::Global));
    }

    #[test]
    fn test_config_scope_enum_display_names() {
        assert_eq!(ConfigScope::Global.display_name(), "Global");
        assert_eq!(
            ConfigScope::Project("my-project".to_string()).display_name(),
            "my-project"
        );
    }

    #[test]
    fn test_config_scope_is_global() {
        assert!(ConfigScope::Global.is_global());
        assert!(!ConfigScope::Project("test".to_string()).is_global());
    }

    #[test]
    fn test_config_scope_equality() {
        assert_eq!(ConfigScope::Global, ConfigScope::Global);
        assert_eq!(
            ConfigScope::Project("a".to_string()),
            ConfigScope::Project("a".to_string())
        );
        assert_ne!(
            ConfigScope::Project("a".to_string()),
            ConfigScope::Project("b".to_string())
        );
        assert_ne!(ConfigScope::Global, ConfigScope::Project("a".to_string()));
    }

    #[test]
    fn test_config_scope_constants_exist() {
        // Verify constants are accessible and have reasonable values
        assert!(CONFIG_SCOPE_ROW_HEIGHT > 0.0);
        assert!(CONFIG_SCOPE_ROW_PADDING_H > 0.0);
        assert!(CONFIG_SCOPE_ROW_PADDING_V > 0.0);
    }

    // ========================================================================
    // ConfigTabState Tests (US-003)
    // ========================================================================

    #[test]
    fn test_config_tab_state_default() {
        let state = ConfigTabState::new();
        assert!(matches!(state.selected_scope, ConfigScope::Global));
        assert!(state.scope_projects.is_empty());
        assert!(state.scope_has_config.is_empty());
        assert!(state.cached_global_config.is_none());
        assert!(state.global_config_error.is_none());
        assert!(state.cached_project_config.is_none());
        assert!(state.project_config_error.is_none());
        assert!(state.last_modified.is_none());
    }

    #[test]
    fn test_config_tab_state_set_selected_scope() {
        let mut state = ConfigTabState::new();
        state.set_selected_scope(ConfigScope::Project("test-project".to_string()));
        assert!(matches!(
            state.selected_scope(),
            ConfigScope::Project(name) if name == "test-project"
        ));
    }

    #[test]
    fn test_config_tab_state_project_has_config() {
        let mut state = ConfigTabState::new();
        state.scope_has_config.insert("project-a".to_string(), true);
        state
            .scope_has_config
            .insert("project-b".to_string(), false);

        assert!(state.project_has_config("project-a"));
        assert!(!state.project_has_config("project-b"));
        assert!(!state.project_has_config("project-c")); // Not in map
    }

    // ========================================================================
    // Config Field Change Tests (US-006)
    // ========================================================================

    #[test]
    fn test_config_bool_field_enum_variants() {
        // Verify all variants can be created
        let _ = ConfigBoolField::Review;
        let _ = ConfigBoolField::Commit;
        let _ = ConfigBoolField::PullRequest;
        let _ = ConfigBoolField::Worktree;
        let _ = ConfigBoolField::WorktreeCleanup;
    }

    #[test]
    fn test_config_editor_actions_default() {
        let actions = ConfigEditorActions::default();
        assert!(actions.create_project_config.is_none());
        assert!(actions.bool_changes.is_empty());
        assert!(actions.text_changes.is_empty());
        assert!(!actions.is_global);
        assert!(actions.project_name.is_none());
        assert!(!actions.reset_to_defaults);
    }
}
