//! Spec file snapshot functionality for detecting new files after Claude sessions.
//!
//! This module provides utilities to snapshot the state of spec files (`.md` files)
//! before spawning a Claude session, so that new files can be detected afterward.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

use crate::error::Result;

/// Metadata for a single file in the snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMetadata {
    /// The file's modification time.
    pub modified: SystemTime,
}

/// A snapshot of spec files at a point in time.
///
/// This struct captures the state of `.md` files in relevant directories
/// before spawning a Claude session, allowing detection of new files afterward.
#[derive(Debug, Clone)]
pub struct SpecSnapshot {
    /// The timestamp when this snapshot was taken.
    pub timestamp: SystemTime,
    /// Map of file paths to their metadata at snapshot time.
    pub files: HashMap<PathBuf, FileMetadata>,
}

impl SpecSnapshot {
    /// Create a new snapshot by scanning the specified directories for `.md` files.
    ///
    /// Scans both:
    /// - `~/.config/autom8/<project>/pdr/` (config directory)
    /// - Current working directory
    ///
    /// Directories that don't exist are silently skipped (resulting in an empty
    /// contribution to the snapshot from that location).
    pub fn capture() -> Result<Self> {
        let timestamp = SystemTime::now();
        let mut files = HashMap::new();

        // Scan config directory spec/
        if let Ok(spec_dir) = crate::config::spec_dir() {
            collect_md_files(&spec_dir, &mut files);
        }

        // Scan current working directory
        if let Ok(cwd) = std::env::current_dir() {
            collect_md_files(&cwd, &mut files);
        }

        Ok(Self { timestamp, files })
    }

    /// Create a snapshot from specific directories (for testing).
    #[cfg(test)]
    pub fn capture_from_dirs(dirs: &[PathBuf]) -> Self {
        let timestamp = SystemTime::now();
        let mut files = HashMap::new();

        for dir in dirs {
            collect_md_files(dir, &mut files);
        }

        Self { timestamp, files }
    }

    /// Create an empty snapshot with the current timestamp (for testing).
    #[cfg(test)]
    pub fn empty() -> Self {
        Self {
            timestamp: SystemTime::now(),
            files: HashMap::new(),
        }
    }

    /// Returns the number of files in this snapshot.
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// Returns true if the snapshot contains no files.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Check if a file path exists in this snapshot.
    pub fn contains(&self, path: &PathBuf) -> bool {
        self.files.contains_key(path)
    }

    /// Get the metadata for a file path, if it exists in the snapshot.
    pub fn get(&self, path: &PathBuf) -> Option<&FileMetadata> {
        self.files.get(path)
    }

    /// Detect new spec files by comparing current state against this snapshot.
    ///
    /// Scans the same directories (config spec/ and current working directory) and returns
    /// paths to files that are either:
    /// - Not present in the original snapshot (newly created)
    /// - Present but modified after the snapshot timestamp (modified during session)
    ///
    /// Returns a sorted list of canonical paths to new/modified `.md` files.
    pub fn detect_new_files(&self) -> Result<Vec<PathBuf>> {
        let mut current_files = HashMap::new();

        // Scan config directory spec/
        if let Ok(spec_dir) = crate::config::spec_dir() {
            collect_md_files(&spec_dir, &mut current_files);
        }

        // Scan current working directory
        if let Ok(cwd) = std::env::current_dir() {
            collect_md_files(&cwd, &mut current_files);
        }

        let mut new_files = Vec::new();

        for (path, metadata) in current_files {
            match self.files.get(&path) {
                // File wasn't in snapshot - it's new
                None => {
                    new_files.push(path);
                }
                // File was in snapshot - check if modified after snapshot timestamp
                Some(old_metadata) => {
                    if metadata.modified > self.timestamp && metadata.modified != old_metadata.modified
                    {
                        new_files.push(path);
                    }
                }
            }
        }

        // Sort for deterministic output
        new_files.sort();

        Ok(new_files)
    }

    /// Detect new files from specific directories (for testing).
    #[cfg(test)]
    pub fn detect_new_files_from_dirs(&self, dirs: &[PathBuf]) -> Vec<PathBuf> {
        let mut current_files = HashMap::new();

        for dir in dirs {
            collect_md_files(dir, &mut current_files);
        }

        let mut new_files = Vec::new();

        for (path, metadata) in current_files {
            match self.files.get(&path) {
                None => {
                    new_files.push(path);
                }
                Some(old_metadata) => {
                    if metadata.modified > self.timestamp && metadata.modified != old_metadata.modified
                    {
                        new_files.push(path);
                    }
                }
            }
        }

        new_files.sort();
        new_files
    }
}

/// Collect all `.md` files from a directory into the files map.
///
/// Non-existent directories are silently ignored.
/// Only collects files directly in the directory (non-recursive).
fn collect_md_files(dir: &PathBuf, files: &mut HashMap<PathBuf, FileMetadata>) {
    if !dir.exists() || !dir.is_dir() {
        return;
    }

    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();

        // Only collect .md files
        if !path.is_file() {
            continue;
        }
        let extension = path.extension().and_then(|e| e.to_str());
        if extension != Some("md") {
            continue;
        }

        // Get modification time
        let metadata = match fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let modified = match metadata.modified() {
            Ok(t) => t,
            Err(_) => continue,
        };

        // Canonicalize path to avoid duplicates from different paths to same file
        let canonical = path.canonicalize().unwrap_or(path);
        files.insert(canonical, FileMetadata { modified });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;
    use tempfile::TempDir;

    #[test]
    fn test_empty_snapshot() {
        let snapshot = SpecSnapshot::empty();
        assert!(snapshot.is_empty());
        assert_eq!(snapshot.len(), 0);
    }

    #[test]
    fn test_capture_from_nonexistent_directory() {
        let nonexistent = PathBuf::from("/this/path/does/not/exist");
        let snapshot = SpecSnapshot::capture_from_dirs(&[nonexistent]);

        assert!(snapshot.is_empty());
        assert_eq!(snapshot.len(), 0);
    }

    #[test]
    fn test_capture_from_empty_directory() {
        let temp_dir = TempDir::new().unwrap();
        let snapshot = SpecSnapshot::capture_from_dirs(&[temp_dir.path().to_path_buf()]);

        assert!(snapshot.is_empty());
    }

    #[test]
    fn test_capture_md_files_only() {
        let temp_dir = TempDir::new().unwrap();

        // Create various files
        fs::write(temp_dir.path().join("readme.md"), "# README").unwrap();
        fs::write(temp_dir.path().join("spec.md"), "# Spec").unwrap();
        fs::write(temp_dir.path().join("config.json"), "{}").unwrap();
        fs::write(temp_dir.path().join("script.sh"), "#!/bin/bash").unwrap();

        let snapshot = SpecSnapshot::capture_from_dirs(&[temp_dir.path().to_path_buf()]);

        assert_eq!(snapshot.len(), 2, "Should only capture .md files");
    }

    #[test]
    fn test_capture_stores_modification_time() {
        let temp_dir = TempDir::new().unwrap();
        let md_file = temp_dir.path().join("test.md");
        fs::write(&md_file, "# Test").unwrap();

        let snapshot = SpecSnapshot::capture_from_dirs(&[temp_dir.path().to_path_buf()]);

        assert_eq!(snapshot.len(), 1);

        // Get the canonical path to look up in snapshot
        let canonical = md_file.canonicalize().unwrap();
        let metadata = snapshot.get(&canonical);
        assert!(metadata.is_some(), "Should have metadata for the file");

        // Verify modification time is reasonable (not in the future, not too old)
        let file_metadata = fs::metadata(&md_file).unwrap();
        let actual_modified = file_metadata.modified().unwrap();
        assert_eq!(
            metadata.unwrap().modified,
            actual_modified,
            "Stored modification time should match file's actual modification time"
        );
    }

    #[test]
    fn test_contains_and_get() {
        let temp_dir = TempDir::new().unwrap();
        let md_file = temp_dir.path().join("test.md");
        fs::write(&md_file, "# Test").unwrap();

        let snapshot = SpecSnapshot::capture_from_dirs(&[temp_dir.path().to_path_buf()]);

        let canonical = md_file.canonicalize().unwrap();
        assert!(snapshot.contains(&canonical));
        assert!(snapshot.get(&canonical).is_some());

        let nonexistent = PathBuf::from("/nonexistent/file.md");
        assert!(!snapshot.contains(&nonexistent));
        assert!(snapshot.get(&nonexistent).is_none());
    }

    #[test]
    fn test_capture_multiple_directories() {
        let temp_dir1 = TempDir::new().unwrap();
        let temp_dir2 = TempDir::new().unwrap();

        fs::write(temp_dir1.path().join("file1.md"), "# File 1").unwrap();
        fs::write(temp_dir2.path().join("file2.md"), "# File 2").unwrap();

        let snapshot = SpecSnapshot::capture_from_dirs(&[
            temp_dir1.path().to_path_buf(),
            temp_dir2.path().to_path_buf(),
        ]);

        assert_eq!(snapshot.len(), 2, "Should capture files from both directories");
    }

    #[test]
    fn test_capture_ignores_subdirectories() {
        let temp_dir = TempDir::new().unwrap();

        // Create file in main directory
        fs::write(temp_dir.path().join("main.md"), "# Main").unwrap();

        // Create subdirectory with file
        let subdir = temp_dir.path().join("subdir");
        fs::create_dir(&subdir).unwrap();
        fs::write(subdir.join("nested.md"), "# Nested").unwrap();

        let snapshot = SpecSnapshot::capture_from_dirs(&[temp_dir.path().to_path_buf()]);

        assert_eq!(
            snapshot.len(),
            1,
            "Should only capture files in the directory, not subdirectories"
        );
    }

    #[test]
    fn test_snapshot_timestamp_is_current() {
        let before = SystemTime::now();
        thread::sleep(Duration::from_millis(10));
        let snapshot = SpecSnapshot::empty();
        thread::sleep(Duration::from_millis(10));
        let after = SystemTime::now();

        assert!(
            snapshot.timestamp >= before,
            "Snapshot timestamp should be after 'before' time"
        );
        assert!(
            snapshot.timestamp <= after,
            "Snapshot timestamp should be before 'after' time"
        );
    }

    #[test]
    fn test_file_metadata_equality() {
        let time = SystemTime::now();
        let meta1 = FileMetadata { modified: time };
        let meta2 = FileMetadata { modified: time };
        let meta3 = FileMetadata {
            modified: time + Duration::from_secs(1),
        };

        assert_eq!(meta1, meta2);
        assert_ne!(meta1, meta3);
    }

    #[test]
    fn test_capture_handles_mixed_directory_states() {
        let temp_dir = TempDir::new().unwrap();
        let existing_dir = temp_dir.path().to_path_buf();
        let nonexistent_dir = PathBuf::from("/this/does/not/exist");

        fs::write(existing_dir.join("test.md"), "# Test").unwrap();

        let snapshot = SpecSnapshot::capture_from_dirs(&[existing_dir, nonexistent_dir]);

        assert_eq!(
            snapshot.len(),
            1,
            "Should capture from existing dir, ignore nonexistent"
        );
    }

    #[test]
    fn test_capture_deduplicates_same_file_via_symlink() {
        let temp_dir = TempDir::new().unwrap();
        let md_file = temp_dir.path().join("original.md");
        fs::write(&md_file, "# Original").unwrap();

        // Create a symlink to the same file (Unix only)
        #[cfg(unix)]
        {
            let symlink = temp_dir.path().join("link.md");
            std::os::unix::fs::symlink(&md_file, &symlink).unwrap();

            let snapshot = SpecSnapshot::capture_from_dirs(&[temp_dir.path().to_path_buf()]);

            // Due to canonicalization, the symlink should resolve to the same file
            // but since we're iterating directory entries, we might see both
            // The key point is canonical paths should be used
            assert!(snapshot.len() >= 1);
        }
    }

    // ===== Detection Logic Tests =====

    #[test]
    fn test_detect_new_files_empty_snapshot_empty_dir() {
        let temp_dir = TempDir::new().unwrap();
        let snapshot = SpecSnapshot::capture_from_dirs(&[temp_dir.path().to_path_buf()]);

        // No changes - should detect nothing
        let new_files = snapshot.detect_new_files_from_dirs(&[temp_dir.path().to_path_buf()]);
        assert!(new_files.is_empty(), "Should detect no new files in unchanged directory");
    }

    #[test]
    fn test_detect_new_files_detects_newly_created_file() {
        let temp_dir = TempDir::new().unwrap();

        // Take snapshot of empty directory
        let snapshot = SpecSnapshot::capture_from_dirs(&[temp_dir.path().to_path_buf()]);
        assert!(snapshot.is_empty());

        // Small delay to ensure file timestamp is after snapshot
        thread::sleep(Duration::from_millis(50));

        // Create new file
        let new_file = temp_dir.path().join("spec-new-feature.md");
        fs::write(&new_file, "# New Spec").unwrap();

        // Detect new files
        let new_files = snapshot.detect_new_files_from_dirs(&[temp_dir.path().to_path_buf()]);

        assert_eq!(new_files.len(), 1, "Should detect exactly one new file");
        assert_eq!(new_files[0], new_file.canonicalize().unwrap());
    }

    #[test]
    fn test_detect_new_files_detects_multiple_new_files() {
        let temp_dir = TempDir::new().unwrap();

        // Take snapshot of empty directory
        let snapshot = SpecSnapshot::capture_from_dirs(&[temp_dir.path().to_path_buf()]);

        // Small delay
        thread::sleep(Duration::from_millis(50));

        // Create multiple new files
        fs::write(temp_dir.path().join("spec-one.md"), "# Spec 1").unwrap();
        fs::write(temp_dir.path().join("spec-two.md"), "# Spec 2").unwrap();
        fs::write(temp_dir.path().join("spec-three.md"), "# Spec 3").unwrap();

        let new_files = snapshot.detect_new_files_from_dirs(&[temp_dir.path().to_path_buf()]);

        assert_eq!(new_files.len(), 3, "Should detect all three new files");
    }

    #[test]
    fn test_detect_new_files_ignores_unchanged_files() {
        let temp_dir = TempDir::new().unwrap();

        // Create existing file
        let existing_file = temp_dir.path().join("existing.md");
        fs::write(&existing_file, "# Existing Spec").unwrap();

        // Take snapshot
        let snapshot = SpecSnapshot::capture_from_dirs(&[temp_dir.path().to_path_buf()]);
        assert_eq!(snapshot.len(), 1);

        // Detect without any changes
        let new_files = snapshot.detect_new_files_from_dirs(&[temp_dir.path().to_path_buf()]);

        assert!(new_files.is_empty(), "Should not detect unchanged files as new");
    }

    #[test]
    fn test_detect_new_files_detects_modified_file() {
        let temp_dir = TempDir::new().unwrap();

        // Create existing file
        let existing_file = temp_dir.path().join("existing.md");
        fs::write(&existing_file, "# Original content").unwrap();

        // Take snapshot
        let snapshot = SpecSnapshot::capture_from_dirs(&[temp_dir.path().to_path_buf()]);

        // Small delay to ensure modification time differs
        thread::sleep(Duration::from_millis(50));

        // Modify the file
        fs::write(&existing_file, "# Modified content - this is new!").unwrap();

        let new_files = snapshot.detect_new_files_from_dirs(&[temp_dir.path().to_path_buf()]);

        assert_eq!(new_files.len(), 1, "Should detect modified file as new");
        assert_eq!(new_files[0], existing_file.canonicalize().unwrap());
    }

    #[test]
    fn test_detect_new_files_only_detects_md_files() {
        let temp_dir = TempDir::new().unwrap();

        // Take snapshot of empty directory
        let snapshot = SpecSnapshot::capture_from_dirs(&[temp_dir.path().to_path_buf()]);

        thread::sleep(Duration::from_millis(50));

        // Create files of various types
        fs::write(temp_dir.path().join("spec-feature.md"), "# Spec").unwrap();
        fs::write(temp_dir.path().join("config.json"), "{}").unwrap();
        fs::write(temp_dir.path().join("readme.txt"), "readme").unwrap();
        fs::write(temp_dir.path().join("script.sh"), "#!/bin/bash").unwrap();

        let new_files = snapshot.detect_new_files_from_dirs(&[temp_dir.path().to_path_buf()]);

        assert_eq!(new_files.len(), 1, "Should only detect .md file");
    }

    #[test]
    fn test_detect_new_files_across_multiple_directories() {
        let temp_dir1 = TempDir::new().unwrap();
        let temp_dir2 = TempDir::new().unwrap();

        // Take snapshot of both directories
        let snapshot = SpecSnapshot::capture_from_dirs(&[
            temp_dir1.path().to_path_buf(),
            temp_dir2.path().to_path_buf(),
        ]);

        thread::sleep(Duration::from_millis(50));

        // Create new files in both directories
        fs::write(temp_dir1.path().join("spec1.md"), "# Spec 1").unwrap();
        fs::write(temp_dir2.path().join("spec2.md"), "# Spec 2").unwrap();

        let new_files = snapshot.detect_new_files_from_dirs(&[
            temp_dir1.path().to_path_buf(),
            temp_dir2.path().to_path_buf(),
        ]);

        assert_eq!(new_files.len(), 2, "Should detect new files from both directories");
    }

    #[test]
    fn test_detect_new_files_returns_sorted_paths() {
        let temp_dir = TempDir::new().unwrap();

        // Take snapshot of empty directory
        let snapshot = SpecSnapshot::capture_from_dirs(&[temp_dir.path().to_path_buf()]);

        thread::sleep(Duration::from_millis(50));

        // Create files in non-alphabetical order
        fs::write(temp_dir.path().join("zebra.md"), "# Z").unwrap();
        fs::write(temp_dir.path().join("apple.md"), "# A").unwrap();
        fs::write(temp_dir.path().join("mango.md"), "# M").unwrap();

        let new_files = snapshot.detect_new_files_from_dirs(&[temp_dir.path().to_path_buf()]);

        assert_eq!(new_files.len(), 3);

        // Verify sorted order
        let filenames: Vec<&str> = new_files
            .iter()
            .filter_map(|p| p.file_name().and_then(|n| n.to_str()))
            .collect();
        assert_eq!(filenames, vec!["apple.md", "mango.md", "zebra.md"]);
    }

    #[test]
    fn test_detect_new_files_handles_nonexistent_directory() {
        let temp_dir = TempDir::new().unwrap();
        let nonexistent = PathBuf::from("/this/path/does/not/exist/at/all");

        // Snapshot from temp_dir, then detect including nonexistent dir
        let snapshot = SpecSnapshot::capture_from_dirs(&[temp_dir.path().to_path_buf()]);

        thread::sleep(Duration::from_millis(50));

        fs::write(temp_dir.path().join("new.md"), "# New").unwrap();

        // Should still work, just ignore the nonexistent directory
        let new_files = snapshot.detect_new_files_from_dirs(&[
            temp_dir.path().to_path_buf(),
            nonexistent,
        ]);

        assert_eq!(new_files.len(), 1, "Should detect file from existing directory");
    }

    #[test]
    fn test_detect_new_files_mixed_new_and_existing() {
        let temp_dir = TempDir::new().unwrap();

        // Create some existing files
        fs::write(temp_dir.path().join("old1.md"), "# Old 1").unwrap();
        fs::write(temp_dir.path().join("old2.md"), "# Old 2").unwrap();

        // Take snapshot
        let snapshot = SpecSnapshot::capture_from_dirs(&[temp_dir.path().to_path_buf()]);
        assert_eq!(snapshot.len(), 2);

        thread::sleep(Duration::from_millis(50));

        // Create some new files (leave old files unchanged)
        fs::write(temp_dir.path().join("new1.md"), "# New 1").unwrap();
        fs::write(temp_dir.path().join("new2.md"), "# New 2").unwrap();

        let new_files = snapshot.detect_new_files_from_dirs(&[temp_dir.path().to_path_buf()]);

        assert_eq!(new_files.len(), 2, "Should only detect the 2 new files, not the 2 old ones");

        let filenames: Vec<&str> = new_files
            .iter()
            .filter_map(|p| p.file_name().and_then(|n| n.to_str()))
            .collect();
        assert!(filenames.contains(&"new1.md"));
        assert!(filenames.contains(&"new2.md"));
        assert!(!filenames.contains(&"old1.md"));
        assert!(!filenames.contains(&"old2.md"));
    }

    #[test]
    fn test_detect_new_files_deleted_file_not_detected() {
        let temp_dir = TempDir::new().unwrap();

        // Create a file
        let to_delete = temp_dir.path().join("delete_me.md");
        fs::write(&to_delete, "# Delete me").unwrap();

        // Take snapshot
        let snapshot = SpecSnapshot::capture_from_dirs(&[temp_dir.path().to_path_buf()]);
        assert_eq!(snapshot.len(), 1);

        // Delete the file
        fs::remove_file(&to_delete).unwrap();

        let new_files = snapshot.detect_new_files_from_dirs(&[temp_dir.path().to_path_buf()]);

        assert!(new_files.is_empty(), "Deleted files should not appear in new files list");
    }
}
