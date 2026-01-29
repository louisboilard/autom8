//! Project knowledge tracking for cumulative context across agent runs.
//!
//! This module provides data structures for tracking what agents learn and
//! accomplish during implementation runs. The knowledge is accumulated across
//! multiple story implementations and can be injected into subsequent agent
//! prompts to provide richer context.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Cumulative project knowledge tracked across agent runs.
///
/// This struct combines two sources of truth:
/// - Git diff data for empirical knowledge of file changes
/// - Agent-provided semantic information about decisions and patterns
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProjectKnowledge {
    /// Known files and their metadata, keyed by path
    pub files: HashMap<PathBuf, FileInfo>,

    /// Architectural and implementation decisions made during the run
    pub decisions: Vec<Decision>,

    /// Code patterns established or discovered during the run
    pub patterns: Vec<Pattern>,

    /// Changes made for each completed story
    pub story_changes: Vec<StoryChanges>,

    /// The baseline commit hash when the run started (for git diff calculations)
    pub baseline_commit: Option<String>,
}

/// Metadata about a known file in the project.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    /// Brief description of the file's purpose
    pub purpose: String,

    /// Key symbols (functions, types, constants) defined in this file
    pub key_symbols: Vec<String>,

    /// IDs of stories that have touched this file
    pub touched_by: Vec<String>,

    /// Number of lines in the file
    pub line_count: u32,
}

/// An architectural or implementation decision made during the run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    /// The story ID that made this decision
    pub story_id: String,

    /// The topic or area this decision relates to
    pub topic: String,

    /// The choice that was made
    pub choice: String,

    /// Why this choice was made
    pub rationale: String,
}

/// A code pattern established or discovered during the run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pattern {
    /// The story ID that established this pattern
    pub story_id: String,

    /// Description of the pattern
    pub description: String,

    /// An example file that demonstrates this pattern
    pub example_file: Option<PathBuf>,
}

/// Changes made while implementing a specific story.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoryChanges {
    /// The story ID these changes belong to
    pub story_id: String,

    /// Files created during this story
    pub files_created: Vec<FileChange>,

    /// Files modified during this story
    pub files_modified: Vec<FileChange>,

    /// Files deleted during this story
    pub files_deleted: Vec<PathBuf>,

    /// The commit hash for these changes (if committed)
    pub commit_hash: Option<String>,
}

/// Information about a file change (creation or modification).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    /// Path to the changed file
    pub path: PathBuf,

    /// Number of lines added
    pub additions: u32,

    /// Number of lines deleted
    pub deletions: u32,

    /// Brief description of the file's purpose (agent-provided)
    pub purpose: Option<String>,

    /// Key symbols added or modified (agent-provided)
    pub key_symbols: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===========================================
    // ProjectKnowledge tests
    // ===========================================

    #[test]
    fn test_project_knowledge_default() {
        let knowledge = ProjectKnowledge::default();
        assert!(knowledge.files.is_empty());
        assert!(knowledge.decisions.is_empty());
        assert!(knowledge.patterns.is_empty());
        assert!(knowledge.story_changes.is_empty());
        assert!(knowledge.baseline_commit.is_none());
    }

    #[test]
    fn test_project_knowledge_debug_impl() {
        let knowledge = ProjectKnowledge::default();
        let debug_str = format!("{:?}", knowledge);
        assert!(debug_str.contains("ProjectKnowledge"));
    }

    #[test]
    fn test_project_knowledge_clone() {
        let mut knowledge = ProjectKnowledge::default();
        knowledge.baseline_commit = Some("abc123".to_string());
        let cloned = knowledge.clone();
        assert_eq!(cloned.baseline_commit, Some("abc123".to_string()));
    }

    #[test]
    fn test_project_knowledge_serialization_roundtrip() {
        let mut knowledge = ProjectKnowledge::default();
        knowledge.baseline_commit = Some("abc123".to_string());
        knowledge.decisions.push(Decision {
            story_id: "US-001".to_string(),
            topic: "Architecture".to_string(),
            choice: "Use modules".to_string(),
            rationale: "Better organization".to_string(),
        });

        let json = serde_json::to_string(&knowledge).unwrap();
        let deserialized: ProjectKnowledge = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.baseline_commit, Some("abc123".to_string()));
        assert_eq!(deserialized.decisions.len(), 1);
        assert_eq!(deserialized.decisions[0].story_id, "US-001");
    }

    #[test]
    fn test_project_knowledge_camel_case_serialization() {
        let knowledge = ProjectKnowledge {
            baseline_commit: Some("abc".to_string()),
            story_changes: vec![StoryChanges {
                story_id: "US-001".to_string(),
                files_created: vec![],
                files_modified: vec![],
                files_deleted: vec![],
                commit_hash: None,
            }],
            ..Default::default()
        };

        let json = serde_json::to_string(&knowledge).unwrap();
        assert!(json.contains("baselineCommit"));
        assert!(json.contains("storyChanges"));
        assert!(json.contains("storyId"));
        assert!(json.contains("filesCreated"));
        assert!(json.contains("filesModified"));
        assert!(json.contains("filesDeleted"));
        assert!(json.contains("commitHash"));
    }

    // ===========================================
    // FileInfo tests
    // ===========================================

    #[test]
    fn test_file_info_creation() {
        let file_info = FileInfo {
            purpose: "Main entry point".to_string(),
            key_symbols: vec!["main".to_string(), "run".to_string()],
            touched_by: vec!["US-001".to_string()],
            line_count: 150,
        };

        assert_eq!(file_info.purpose, "Main entry point");
        assert_eq!(file_info.key_symbols.len(), 2);
        assert_eq!(file_info.touched_by.len(), 1);
        assert_eq!(file_info.line_count, 150);
    }

    #[test]
    fn test_file_info_serialization() {
        let file_info = FileInfo {
            purpose: "Test file".to_string(),
            key_symbols: vec!["test_fn".to_string()],
            touched_by: vec!["US-001".to_string()],
            line_count: 50,
        };

        let json = serde_json::to_string(&file_info).unwrap();
        assert!(json.contains("keySymbols"));
        assert!(json.contains("touchedBy"));
        assert!(json.contains("lineCount"));

        let deserialized: FileInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.purpose, "Test file");
        assert_eq!(deserialized.line_count, 50);
    }

    // ===========================================
    // Decision tests
    // ===========================================

    #[test]
    fn test_decision_creation() {
        let decision = Decision {
            story_id: "US-001".to_string(),
            topic: "Database".to_string(),
            choice: "SQLite".to_string(),
            rationale: "Simple, embedded, no setup".to_string(),
        };

        assert_eq!(decision.story_id, "US-001");
        assert_eq!(decision.topic, "Database");
        assert_eq!(decision.choice, "SQLite");
        assert_eq!(decision.rationale, "Simple, embedded, no setup");
    }

    #[test]
    fn test_decision_serialization() {
        let decision = Decision {
            story_id: "US-002".to_string(),
            topic: "Auth".to_string(),
            choice: "JWT".to_string(),
            rationale: "Stateless".to_string(),
        };

        let json = serde_json::to_string(&decision).unwrap();
        assert!(json.contains("storyId"));

        let deserialized: Decision = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.story_id, "US-002");
    }

    // ===========================================
    // Pattern tests
    // ===========================================

    #[test]
    fn test_pattern_creation_with_example() {
        let pattern = Pattern {
            story_id: "US-001".to_string(),
            description: "Use Result<T, Error> for all fallible operations".to_string(),
            example_file: Some(PathBuf::from("src/runner.rs")),
        };

        assert_eq!(pattern.story_id, "US-001");
        assert!(pattern.example_file.is_some());
    }

    #[test]
    fn test_pattern_creation_without_example() {
        let pattern = Pattern {
            story_id: "US-001".to_string(),
            description: "Use snake_case for function names".to_string(),
            example_file: None,
        };

        assert!(pattern.example_file.is_none());
    }

    #[test]
    fn test_pattern_serialization() {
        let pattern = Pattern {
            story_id: "US-001".to_string(),
            description: "Test pattern".to_string(),
            example_file: Some(PathBuf::from("src/lib.rs")),
        };

        let json = serde_json::to_string(&pattern).unwrap();
        assert!(json.contains("storyId"));
        assert!(json.contains("exampleFile"));

        let deserialized: Pattern = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.example_file, Some(PathBuf::from("src/lib.rs")));
    }

    // ===========================================
    // StoryChanges tests
    // ===========================================

    #[test]
    fn test_story_changes_creation() {
        let changes = StoryChanges {
            story_id: "US-001".to_string(),
            files_created: vec![FileChange {
                path: PathBuf::from("src/new.rs"),
                additions: 100,
                deletions: 0,
                purpose: Some("New module".to_string()),
                key_symbols: vec!["NewStruct".to_string()],
            }],
            files_modified: vec![FileChange {
                path: PathBuf::from("src/lib.rs"),
                additions: 5,
                deletions: 0,
                purpose: None,
                key_symbols: vec![],
            }],
            files_deleted: vec![PathBuf::from("src/old.rs")],
            commit_hash: Some("def456".to_string()),
        };

        assert_eq!(changes.story_id, "US-001");
        assert_eq!(changes.files_created.len(), 1);
        assert_eq!(changes.files_modified.len(), 1);
        assert_eq!(changes.files_deleted.len(), 1);
        assert!(changes.commit_hash.is_some());
    }

    #[test]
    fn test_story_changes_without_commit() {
        let changes = StoryChanges {
            story_id: "US-001".to_string(),
            files_created: vec![],
            files_modified: vec![],
            files_deleted: vec![],
            commit_hash: None,
        };

        assert!(changes.commit_hash.is_none());
    }

    #[test]
    fn test_story_changes_serialization() {
        let changes = StoryChanges {
            story_id: "US-001".to_string(),
            files_created: vec![],
            files_modified: vec![],
            files_deleted: vec![],
            commit_hash: Some("abc".to_string()),
        };

        let json = serde_json::to_string(&changes).unwrap();
        assert!(json.contains("storyId"));
        assert!(json.contains("filesCreated"));
        assert!(json.contains("filesModified"));
        assert!(json.contains("filesDeleted"));
        assert!(json.contains("commitHash"));
    }

    // ===========================================
    // FileChange tests
    // ===========================================

    #[test]
    fn test_file_change_creation() {
        let change = FileChange {
            path: PathBuf::from("src/test.rs"),
            additions: 50,
            deletions: 10,
            purpose: Some("Test utilities".to_string()),
            key_symbols: vec!["test_helper".to_string(), "setup".to_string()],
        };

        assert_eq!(change.path, PathBuf::from("src/test.rs"));
        assert_eq!(change.additions, 50);
        assert_eq!(change.deletions, 10);
        assert!(change.purpose.is_some());
        assert_eq!(change.key_symbols.len(), 2);
    }

    #[test]
    fn test_file_change_minimal() {
        let change = FileChange {
            path: PathBuf::from("src/lib.rs"),
            additions: 1,
            deletions: 0,
            purpose: None,
            key_symbols: vec![],
        };

        assert!(change.purpose.is_none());
        assert!(change.key_symbols.is_empty());
    }

    #[test]
    fn test_file_change_serialization() {
        let change = FileChange {
            path: PathBuf::from("src/test.rs"),
            additions: 10,
            deletions: 5,
            purpose: Some("Test".to_string()),
            key_symbols: vec!["sym".to_string()],
        };

        let json = serde_json::to_string(&change).unwrap();
        assert!(json.contains("keySymbols"));

        let deserialized: FileChange = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.additions, 10);
        assert_eq!(deserialized.deletions, 5);
    }

    // ===========================================
    // Integration tests
    // ===========================================

    #[test]
    fn test_project_knowledge_with_files() {
        let mut knowledge = ProjectKnowledge::default();

        knowledge.files.insert(
            PathBuf::from("src/main.rs"),
            FileInfo {
                purpose: "Application entry point".to_string(),
                key_symbols: vec!["main".to_string()],
                touched_by: vec!["US-001".to_string()],
                line_count: 100,
            },
        );

        assert_eq!(knowledge.files.len(), 1);
        let file_info = knowledge.files.get(&PathBuf::from("src/main.rs")).unwrap();
        assert_eq!(file_info.purpose, "Application entry point");
    }

    #[test]
    fn test_full_knowledge_serialization_roundtrip() {
        let mut knowledge = ProjectKnowledge::default();
        knowledge.baseline_commit = Some("baseline123".to_string());

        knowledge.files.insert(
            PathBuf::from("src/lib.rs"),
            FileInfo {
                purpose: "Library root".to_string(),
                key_symbols: vec!["mod".to_string()],
                touched_by: vec!["US-001".to_string(), "US-002".to_string()],
                line_count: 50,
            },
        );

        knowledge.decisions.push(Decision {
            story_id: "US-001".to_string(),
            topic: "Error handling".to_string(),
            choice: "thiserror crate".to_string(),
            rationale: "Clean error types".to_string(),
        });

        knowledge.patterns.push(Pattern {
            story_id: "US-001".to_string(),
            description: "Use ? operator for error propagation".to_string(),
            example_file: Some(PathBuf::from("src/runner.rs")),
        });

        knowledge.story_changes.push(StoryChanges {
            story_id: "US-001".to_string(),
            files_created: vec![FileChange {
                path: PathBuf::from("src/knowledge.rs"),
                additions: 200,
                deletions: 0,
                purpose: Some("Knowledge tracking".to_string()),
                key_symbols: vec!["ProjectKnowledge".to_string()],
            }],
            files_modified: vec![FileChange {
                path: PathBuf::from("src/lib.rs"),
                additions: 1,
                deletions: 0,
                purpose: None,
                key_symbols: vec![],
            }],
            files_deleted: vec![],
            commit_hash: Some("commit123".to_string()),
        });

        // Serialize
        let json = serde_json::to_string_pretty(&knowledge).unwrap();

        // Deserialize
        let deserialized: ProjectKnowledge = serde_json::from_str(&json).unwrap();

        // Verify all fields preserved
        assert_eq!(
            deserialized.baseline_commit,
            Some("baseline123".to_string())
        );
        assert_eq!(deserialized.files.len(), 1);
        assert_eq!(deserialized.decisions.len(), 1);
        assert_eq!(deserialized.patterns.len(), 1);
        assert_eq!(deserialized.story_changes.len(), 1);

        // Verify nested fields
        let file_info = deserialized
            .files
            .get(&PathBuf::from("src/lib.rs"))
            .unwrap();
        assert_eq!(file_info.touched_by.len(), 2);

        let story_changes = &deserialized.story_changes[0];
        assert_eq!(story_changes.files_created.len(), 1);
        assert_eq!(story_changes.files_created[0].additions, 200);
    }
}
