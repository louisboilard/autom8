use crate::error::{Autom8Error, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prd {
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

impl Prd {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(Autom8Error::PrdNotFound(path.to_path_buf()));
        }

        let content = fs::read_to_string(path)?;
        let prd: Prd = serde_json::from_str(&content)
            .map_err(|e| Autom8Error::InvalidPrd(e.to_string()))?;

        prd.validate()?;
        Ok(prd)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let content = serde_json::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        if self.project.is_empty() {
            return Err(Autom8Error::InvalidPrd("project name is required".into()));
        }
        if self.user_stories.is_empty() {
            return Err(Autom8Error::InvalidPrd("at least one user story is required".into()));
        }
        for story in &self.user_stories {
            if story.id.is_empty() {
                return Err(Autom8Error::InvalidPrd("story id is required".into()));
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

    pub fn mark_story_complete(&mut self, story_id: &str) {
        if let Some(story) = self.user_stories.iter_mut().find(|s| s.id == story_id) {
            story.passes = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_next_incomplete_story() {
        let prd = Prd {
            project: "Test".into(),
            branch_name: "test".into(),
            description: "Test".into(),
            user_stories: vec![
                UserStory {
                    id: "US-001".into(),
                    title: "First".into(),
                    description: "First story".into(),
                    acceptance_criteria: vec![],
                    priority: 2,
                    passes: false,
                    notes: String::new(),
                },
                UserStory {
                    id: "US-002".into(),
                    title: "Second".into(),
                    description: "Second story".into(),
                    acceptance_criteria: vec![],
                    priority: 1,
                    passes: false,
                    notes: String::new(),
                },
            ],
        };

        let next = prd.next_incomplete_story().unwrap();
        assert_eq!(next.id, "US-002"); // Lower priority number = higher priority
    }
}
