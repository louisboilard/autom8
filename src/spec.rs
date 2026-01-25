use crate::error::{Autom8Error, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spec {
    pub project: String,
    #[serde(default = "default_branch_name")]
    pub branch_name: String,
    pub description: String,
    pub user_stories: Vec<UserStory>,
}

fn default_branch_name() -> String {
    "autom8/feature".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserStory {
    pub id: String,
    pub title: String,
    pub description: String,
    pub acceptance_criteria: Vec<String>,
    pub priority: u32,
    pub passes: bool,
    #[serde(default)]
    pub notes: String,
}

impl Spec {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(Autom8Error::SpecNotFound(path.to_path_buf()));
        }

        let content = fs::read_to_string(path)?;
        let spec: Spec =
            serde_json::from_str(&content).map_err(|e| Autom8Error::InvalidSpec(e.to_string()))?;

        spec.validate()?;
        Ok(spec)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let content = serde_json::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        if self.project.is_empty() {
            return Err(Autom8Error::InvalidSpec("project name is required".into()));
        }
        if self.user_stories.is_empty() {
            return Err(Autom8Error::InvalidSpec(
                "at least one user story is required".into(),
            ));
        }
        for story in &self.user_stories {
            if story.id.is_empty() {
                return Err(Autom8Error::InvalidSpec("story id is required".into()));
            }
        }
        Ok(())
    }

    pub fn next_incomplete_story(&self) -> Option<&UserStory> {
        self.user_stories
            .iter()
            .filter(|s| !s.passes)
            .min_by_key(|s| s.priority)
    }

    pub fn completed_count(&self) -> usize {
        self.user_stories.iter().filter(|s| s.passes).count()
    }

    pub fn total_count(&self) -> usize {
        self.user_stories.len()
    }

    pub fn all_complete(&self) -> bool {
        self.user_stories.iter().all(|s| s.passes)
    }

    /// Returns true if spec has incomplete stories
    pub fn is_incomplete(&self) -> bool {
        !self.all_complete()
    }

    /// Returns (completed, total) story counts
    pub fn progress(&self) -> (usize, usize) {
        (self.completed_count(), self.total_count())
    }

    pub fn mark_story_complete(&mut self, story_id: &str) {
        if let Some(story) = self.user_stories.iter_mut().find(|s| s.id == story_id) {
            story.passes = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn make_story(id: &str, priority: u32, passes: bool) -> UserStory {
        UserStory {
            id: id.into(),
            title: format!("Story {}", id),
            description: format!("Description for {}", id),
            acceptance_criteria: vec!["Criteria 1".into()],
            priority,
            passes,
            notes: String::new(),
        }
    }

    fn make_spec(stories: Vec<UserStory>) -> Spec {
        Spec {
            project: "TestProject".into(),
            branch_name: "test-branch".into(),
            description: "Test description".into(),
            user_stories: stories,
        }
    }

    // ===========================================
    // Validation tests
    // ===========================================

    #[test]
    fn test_validate_empty_project_name_fails() {
        let spec = Spec {
            project: "".into(),
            branch_name: "test".into(),
            description: "Test".into(),
            user_stories: vec![make_story("US-001", 1, false)],
        };
        let result = spec.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("project name is required"));
    }

    #[test]
    fn test_validate_empty_stories_fails() {
        let spec = Spec {
            project: "Test".into(),
            branch_name: "test".into(),
            description: "Test".into(),
            user_stories: vec![],
        };
        let result = spec.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("at least one user story is required"));
    }

    #[test]
    fn test_validate_story_with_empty_id_fails() {
        let spec = Spec {
            project: "Test".into(),
            branch_name: "test".into(),
            description: "Test".into(),
            user_stories: vec![UserStory {
                id: "".into(),
                title: "Story".into(),
                description: "Desc".into(),
                acceptance_criteria: vec![],
                priority: 1,
                passes: false,
                notes: String::new(),
            }],
        };
        let result = spec.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("story id is required"));
    }

    #[test]
    fn test_validate_valid_spec_succeeds() {
        let spec = make_spec(vec![make_story("US-001", 1, false)]);
        assert!(spec.validate().is_ok());
    }

    // ===========================================
    // Load and save round-trip tests
    // ===========================================

    #[test]
    fn test_load_nonexistent_file_returns_spec_not_found() {
        let path = Path::new("/nonexistent/path/spec.json");
        let result = Spec::load(path);
        assert!(result.is_err());
        match result.unwrap_err() {
            Autom8Error::SpecNotFound(_) => {}
            e => panic!("Expected SpecNotFound, got {:?}", e),
        }
    }

    #[test]
    fn test_load_invalid_json_returns_invalid_spec() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "not valid json {{}}").unwrap();
        let result = Spec::load(file.path());
        assert!(result.is_err());
        match result.unwrap_err() {
            Autom8Error::InvalidSpec(_) => {}
            e => panic!("Expected InvalidSpec, got {:?}", e),
        }
    }

    #[test]
    fn test_save_and_load_round_trip() {
        let spec = make_spec(vec![
            make_story("US-001", 1, true),
            make_story("US-002", 2, false),
        ]);
        let file = NamedTempFile::new().unwrap();

        spec.save(file.path()).unwrap();
        let loaded = Spec::load(file.path()).unwrap();

        assert_eq!(loaded.project, spec.project);
        assert_eq!(loaded.branch_name, spec.branch_name);
        assert_eq!(loaded.description, spec.description);
        assert_eq!(loaded.user_stories.len(), 2);
        assert_eq!(loaded.user_stories[0].id, "US-001");
        assert!(loaded.user_stories[0].passes);
        assert_eq!(loaded.user_stories[1].id, "US-002");
        assert!(!loaded.user_stories[1].passes);
    }

    #[test]
    fn test_load_validates_after_parsing() {
        let mut file = NamedTempFile::new().unwrap();
        // Valid JSON but empty project name
        writeln!(
            file,
            r#"{{"project": "", "branchName": "test", "description": "Test", "userStories": [{{"id": "US-001", "title": "T", "description": "D", "acceptanceCriteria": [], "priority": 1, "passes": false}}]}}"#
        )
        .unwrap();
        let result = Spec::load(file.path());
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("project name is required"));
    }

    // ===========================================
    // next_incomplete_story tests
    // ===========================================

    #[test]
    fn test_next_incomplete_story_returns_lowest_priority_number() {
        let spec = make_spec(vec![
            make_story("US-001", 2, false),
            make_story("US-002", 1, false),
        ]);
        let next = spec.next_incomplete_story().unwrap();
        assert_eq!(next.id, "US-002"); // Lower priority number = higher priority
    }

    #[test]
    fn test_next_incomplete_story_skips_completed() {
        let spec = make_spec(vec![
            make_story("US-001", 1, true), // completed, lowest priority number
            make_story("US-002", 2, false),
            make_story("US-003", 3, false),
        ]);
        let next = spec.next_incomplete_story().unwrap();
        assert_eq!(next.id, "US-002");
    }

    #[test]
    fn test_next_incomplete_story_returns_none_when_all_complete() {
        let spec = make_spec(vec![
            make_story("US-001", 1, true),
            make_story("US-002", 2, true),
        ]);
        assert!(spec.next_incomplete_story().is_none());
    }

    #[test]
    fn test_next_incomplete_story_with_single_incomplete() {
        let spec = make_spec(vec![make_story("US-001", 5, false)]);
        let next = spec.next_incomplete_story().unwrap();
        assert_eq!(next.id, "US-001");
    }

    #[test]
    fn test_next_incomplete_story_with_same_priority_returns_first_encountered() {
        let spec = make_spec(vec![
            make_story("US-001", 1, false),
            make_story("US-002", 1, false),
        ]);
        let next = spec.next_incomplete_story().unwrap();
        // With same priority, min_by_key returns first found
        assert_eq!(next.id, "US-001");
    }

    // ===========================================
    // Completion calculation tests
    // ===========================================

    #[test]
    fn test_completed_count_with_no_complete() {
        let spec = make_spec(vec![
            make_story("US-001", 1, false),
            make_story("US-002", 2, false),
        ]);
        assert_eq!(spec.completed_count(), 0);
    }

    #[test]
    fn test_completed_count_with_some_complete() {
        let spec = make_spec(vec![
            make_story("US-001", 1, true),
            make_story("US-002", 2, false),
            make_story("US-003", 3, true),
        ]);
        assert_eq!(spec.completed_count(), 2);
    }

    #[test]
    fn test_total_count() {
        let spec = make_spec(vec![
            make_story("US-001", 1, false),
            make_story("US-002", 2, true),
            make_story("US-003", 3, false),
        ]);
        assert_eq!(spec.total_count(), 3);
    }

    #[test]
    fn test_all_complete_returns_false_when_incomplete_exists() {
        let spec = make_spec(vec![
            make_story("US-001", 1, true),
            make_story("US-002", 2, false),
        ]);
        assert!(!spec.all_complete());
    }

    #[test]
    fn test_all_complete_returns_true_when_all_done() {
        let spec = make_spec(vec![
            make_story("US-001", 1, true),
            make_story("US-002", 2, true),
        ]);
        assert!(spec.all_complete());
    }

    #[test]
    fn test_is_incomplete_inverse_of_all_complete() {
        let complete_spec = make_spec(vec![make_story("US-001", 1, true)]);
        let incomplete_spec = make_spec(vec![make_story("US-001", 1, false)]);

        assert!(!complete_spec.is_incomplete());
        assert!(incomplete_spec.is_incomplete());
    }

    #[test]
    fn test_progress_returns_completed_and_total() {
        let spec = make_spec(vec![
            make_story("US-001", 1, true),
            make_story("US-002", 2, true),
            make_story("US-003", 3, false),
            make_story("US-004", 4, false),
        ]);
        let (completed, total) = spec.progress();
        assert_eq!(completed, 2);
        assert_eq!(total, 4);
    }

    // ===========================================
    // mark_story_complete tests
    // ===========================================

    #[test]
    fn test_mark_story_complete_marks_correct_story() {
        let mut spec = make_spec(vec![
            make_story("US-001", 1, false),
            make_story("US-002", 2, false),
        ]);
        spec.mark_story_complete("US-001");
        assert!(spec.user_stories[0].passes);
        assert!(!spec.user_stories[1].passes);
    }

    #[test]
    fn test_mark_story_complete_nonexistent_id_is_noop() {
        let mut spec = make_spec(vec![make_story("US-001", 1, false)]);
        spec.mark_story_complete("US-999"); // doesn't exist
        assert!(!spec.user_stories[0].passes); // unchanged
    }

    #[test]
    fn test_mark_story_complete_already_complete_is_idempotent() {
        let mut spec = make_spec(vec![make_story("US-001", 1, true)]);
        spec.mark_story_complete("US-001");
        assert!(spec.user_stories[0].passes); // still true
    }

    // ===========================================
    // Default branch name test
    // ===========================================

    #[test]
    fn test_default_branch_name_used_when_missing() {
        let mut file = NamedTempFile::new().unwrap();
        // JSON without branchName field
        writeln!(
            file,
            r#"{{"project": "Test", "description": "Test", "userStories": [{{"id": "US-001", "title": "T", "description": "D", "acceptanceCriteria": [], "priority": 1, "passes": false}}]}}"#
        )
        .unwrap();
        let loaded = Spec::load(file.path()).unwrap();
        assert_eq!(loaded.branch_name, "autom8/feature");
    }
}
