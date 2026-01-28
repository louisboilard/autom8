//! PR detection for branches.

use std::process::Command;

use crate::error::Result;
use crate::git;

use super::types::{PRDetectionResult, PullRequestInfo};

/// Check if a pull request already exists for the given branch
pub fn pr_exists_for_branch(branch: &str) -> Result<bool> {
    let output = Command::new("gh")
        .args(["pr", "list", "--head", branch, "--json", "number"])
        .output()?;

    if !output.status.success() {
        return Ok(false);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();

    Ok(trimmed != "[]" && !trimmed.is_empty())
}

/// Get the URL of an existing pull request for the given branch
pub fn get_existing_pr_url(branch: &str) -> Result<Option<String>> {
    let output = Command::new("gh")
        .args(["pr", "list", "--head", branch, "--json", "url"])
        .output()?;

    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();

    if trimmed == "[]" || trimmed.is_empty() {
        return Ok(None);
    }

    let parsed: std::result::Result<Vec<serde_json::Value>, _> = serde_json::from_str(trimmed);

    match parsed {
        Ok(prs) if !prs.is_empty() => {
            if let Some(url) = prs[0].get("url").and_then(|v| v.as_str()) {
                Ok(Some(url.to_string()))
            } else {
                Ok(None)
            }
        }
        _ => Ok(None),
    }
}

/// Get the PR number for an existing pull request for the given branch
pub fn get_existing_pr_number(branch: &str) -> Result<Option<u32>> {
    let output = Command::new("gh")
        .args(["pr", "list", "--head", branch, "--json", "number"])
        .output()?;

    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();

    if trimmed == "[]" || trimmed.is_empty() {
        return Ok(None);
    }

    let parsed: std::result::Result<Vec<serde_json::Value>, _> = serde_json::from_str(trimmed);

    match parsed {
        Ok(prs) if !prs.is_empty() => {
            if let Some(number) = prs[0].get("number").and_then(|v| v.as_u64()) {
                Ok(Some(number as u32))
            } else {
                Ok(None)
            }
        }
        _ => Ok(None),
    }
}

/// Detect the PR for the current branch
pub fn detect_pr_for_current_branch() -> Result<PRDetectionResult> {
    let branch = match git::current_branch() {
        Ok(b) => b,
        Err(e) => return Ok(PRDetectionResult::Error(e.to_string())),
    };

    if branch == "main" || branch == "master" {
        return Ok(PRDetectionResult::OnMainBranch);
    }

    match get_pr_info_for_branch(&branch) {
        Ok(Some(info)) => Ok(PRDetectionResult::Found(info)),
        Ok(None) => Ok(PRDetectionResult::NoPRForBranch(branch)),
        Err(e) => Ok(PRDetectionResult::Error(e.to_string())),
    }
}

/// Get PR info for a specific branch
pub fn get_pr_info_for_branch(branch: &str) -> Result<Option<PullRequestInfo>> {
    let output = Command::new("gh")
        .args([
            "pr",
            "list",
            "--head",
            branch,
            "--json",
            "number,title,headRefName,url",
        ])
        .output()?;

    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();

    if trimmed == "[]" || trimmed.is_empty() {
        return Ok(None);
    }

    let parsed: std::result::Result<Vec<serde_json::Value>, _> = serde_json::from_str(trimmed);

    match parsed {
        Ok(prs) if !prs.is_empty() => {
            let pr = &prs[0];
            let number = pr.get("number").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let title = pr
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let head_branch = pr
                .get("headRefName")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let url = pr
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            Ok(Some(PullRequestInfo {
                number,
                title,
                head_branch,
                url,
            }))
        }
        _ => Ok(None),
    }
}

/// List all open PRs in the repository
pub fn list_open_prs() -> Result<Vec<PullRequestInfo>> {
    let output = Command::new("gh")
        .args([
            "pr",
            "list",
            "--state",
            "open",
            "--json",
            "number,title,headRefName,url",
        ])
        .output()?;

    if !output.status.success() {
        return Ok(vec![]);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();

    if trimmed == "[]" || trimmed.is_empty() {
        return Ok(vec![]);
    }

    let parsed: std::result::Result<Vec<serde_json::Value>, _> = serde_json::from_str(trimmed);

    match parsed {
        Ok(prs) => {
            let infos: Vec<PullRequestInfo> = prs
                .iter()
                .filter_map(|pr| {
                    let number = pr.get("number").and_then(|v| v.as_u64())? as u32;
                    let title = pr.get("title").and_then(|v| v.as_str())?.to_string();
                    let head_branch = pr.get("headRefName").and_then(|v| v.as_str())?.to_string();
                    let url = pr.get("url").and_then(|v| v.as_str())?.to_string();

                    Some(PullRequestInfo {
                        number,
                        title,
                        head_branch,
                        url,
                    })
                })
                .collect();
            Ok(infos)
        }
        Err(_) => Ok(vec![]),
    }
}
