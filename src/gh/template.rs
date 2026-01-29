//! PR template detection for GitHub repositories.
//!
//! This module provides functionality to detect and read PR templates
//! from standard GitHub template locations.

use std::fs;
use std::path::Path;

/// Standard locations for GitHub PR templates, in order of precedence.
const PR_TEMPLATE_PATHS: &[&str] = &[
    ".github/pull_request_template.md",
    ".github/PULL_REQUEST_TEMPLATE.md",
    "pull_request_template.md",
];

/// Detects and returns the content of a PR template if one exists in the repository.
///
/// Checks standard GitHub template locations in order of precedence:
/// 1. `.github/pull_request_template.md` (lowercase)
/// 2. `.github/PULL_REQUEST_TEMPLATE.md` (uppercase)
/// 3. `pull_request_template.md` (repo root)
///
/// Returns `Some(content)` if a template is found, `None` otherwise.
///
/// # Arguments
///
/// * `repo_root` - Path to the repository root directory
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
/// use autom8::gh::detect_pr_template;
///
/// let template = detect_pr_template(Path::new("/path/to/repo"));
/// if let Some(content) = template {
///     println!("Found template:\n{}", content);
/// }
/// ```
pub fn detect_pr_template(repo_root: &Path) -> Option<String> {
    for template_path in PR_TEMPLATE_PATHS {
        let full_path = repo_root.join(template_path);
        if full_path.is_file() {
            match fs::read_to_string(&full_path) {
                Ok(content) => return Some(content),
                Err(_) => continue, // Try next location if read fails
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;
    use tempfile::TempDir;

    fn create_template(dir: &Path, relative_path: &str, content: &str) {
        let full_path = dir.join(relative_path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut file = File::create(full_path).unwrap();
        writeln!(file, "{}", content).unwrap();
    }

    #[test]
    fn test_no_template_returns_none() {
        let temp_dir = TempDir::new().unwrap();
        let result = detect_pr_template(temp_dir.path());
        assert!(result.is_none());
    }

    #[test]
    fn test_detects_lowercase_github_template() {
        let temp_dir = TempDir::new().unwrap();
        let expected_content = "## Description\nPlease describe your changes";
        create_template(
            temp_dir.path(),
            ".github/pull_request_template.md",
            expected_content,
        );

        let result = detect_pr_template(temp_dir.path());
        assert!(result.is_some());
        assert!(result.unwrap().contains(expected_content));
    }

    #[test]
    fn test_detects_uppercase_github_template() {
        let temp_dir = TempDir::new().unwrap();
        let expected_content = "## Summary\nDescribe what this PR does";
        create_template(
            temp_dir.path(),
            ".github/PULL_REQUEST_TEMPLATE.md",
            expected_content,
        );

        let result = detect_pr_template(temp_dir.path());
        assert!(result.is_some());
        assert!(result.unwrap().contains(expected_content));
    }

    #[test]
    fn test_detects_root_template() {
        let temp_dir = TempDir::new().unwrap();
        let expected_content = "## Changes\nList your changes here";
        create_template(
            temp_dir.path(),
            "pull_request_template.md",
            expected_content,
        );

        let result = detect_pr_template(temp_dir.path());
        assert!(result.is_some());
        assert!(result.unwrap().contains(expected_content));
    }

    #[test]
    fn test_precedence_lowercase_github_over_uppercase() {
        let temp_dir = TempDir::new().unwrap();
        let lowercase_content = "LOWERCASE TEMPLATE";
        let uppercase_content = "UPPERCASE TEMPLATE";

        create_template(
            temp_dir.path(),
            ".github/pull_request_template.md",
            lowercase_content,
        );
        create_template(
            temp_dir.path(),
            ".github/PULL_REQUEST_TEMPLATE.md",
            uppercase_content,
        );

        let result = detect_pr_template(temp_dir.path());
        assert!(result.is_some());

        // On case-insensitive filesystems (macOS APFS, Windows NTFS), both filenames
        // refer to the same file, so the second write overwrites the first.
        // The test verifies that we find *a* template; the precedence between
        // lowercase and uppercase is only meaningful on case-sensitive filesystems.
        let content = result.unwrap();
        let is_case_sensitive_fs = temp_dir
            .path()
            .join(".github/pull_request_template.md")
            .exists()
            && temp_dir
                .path()
                .join(".github/PULL_REQUEST_TEMPLATE.md")
                .exists()
            && fs::read_to_string(temp_dir.path().join(".github/pull_request_template.md"))
                .unwrap()
                != fs::read_to_string(temp_dir.path().join(".github/PULL_REQUEST_TEMPLATE.md"))
                    .unwrap();

        if is_case_sensitive_fs {
            // On case-sensitive filesystems, lowercase takes precedence
            assert!(content.contains(lowercase_content));
        }
        // On case-insensitive filesystems, just verify we got a template
    }

    #[test]
    fn test_precedence_github_over_root() {
        let temp_dir = TempDir::new().unwrap();
        let github_content = "GITHUB DIRECTORY TEMPLATE";
        let root_content = "ROOT TEMPLATE";

        create_template(
            temp_dir.path(),
            ".github/pull_request_template.md",
            github_content,
        );
        create_template(temp_dir.path(), "pull_request_template.md", root_content);

        let result = detect_pr_template(temp_dir.path());
        assert!(result.is_some());
        assert!(result.unwrap().contains(github_content));
    }

    #[test]
    fn test_precedence_uppercase_github_over_root() {
        let temp_dir = TempDir::new().unwrap();
        let github_content = "UPPERCASE GITHUB TEMPLATE";
        let root_content = "ROOT TEMPLATE";

        create_template(
            temp_dir.path(),
            ".github/PULL_REQUEST_TEMPLATE.md",
            github_content,
        );
        create_template(temp_dir.path(), "pull_request_template.md", root_content);

        let result = detect_pr_template(temp_dir.path());
        assert!(result.is_some());
        assert!(result.unwrap().contains(github_content));
    }

    #[test]
    fn test_falls_back_to_root_when_github_missing() {
        let temp_dir = TempDir::new().unwrap();
        let root_content = "ROOT ONLY TEMPLATE";
        create_template(temp_dir.path(), "pull_request_template.md", root_content);

        let result = detect_pr_template(temp_dir.path());
        assert!(result.is_some());
        assert!(result.unwrap().contains(root_content));
    }

    #[test]
    fn test_nonexistent_repo_path_returns_none() {
        let result = detect_pr_template(Path::new("/nonexistent/path/to/repo"));
        assert!(result.is_none());
    }

    #[test]
    fn test_empty_template_returns_content() {
        let temp_dir = TempDir::new().unwrap();
        // Create an empty template file
        let template_path = temp_dir.path().join(".github/pull_request_template.md");
        fs::create_dir_all(template_path.parent().unwrap()).unwrap();
        File::create(&template_path).unwrap();

        let result = detect_pr_template(temp_dir.path());
        // Empty file should still be detected
        assert!(result.is_some());
    }
}
