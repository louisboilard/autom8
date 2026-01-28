//! PR context gathering for reviews.

use std::process::Command;


/// A single comment on a PR
#[derive(Debug, Clone, PartialEq)]
pub struct PRComment {
    /// The author's username
    pub author: String,
    /// The comment body text
    pub body: String,
    /// The file path if this is a file comment
    pub file_path: Option<String>,
    /// The line number if applicable
    pub line: Option<u32>,
    /// Whether this is a review thread (vs conversation comment)
    pub is_review_thread: bool,
    /// The thread ID for review threads
    pub thread_id: Option<String>,
}

/// Full context about a PR for review
#[derive(Debug, Clone)]
pub struct PRContext {
    /// PR number
    pub number: u32,
    /// PR title
    pub title: String,
    /// PR body/description
    pub body: String,
    /// PR URL
    pub url: String,
    /// Unresolved comments that need attention
    pub unresolved_comments: Vec<PRComment>,
}

/// Result of gathering PR context
#[derive(Debug, Clone)]
pub enum PRContextResult {
    /// Successfully gathered PR context with unresolved comments
    Success(PRContext),
    /// PR has no unresolved comments
    NoUnresolvedComments {
        number: u32,
        title: String,
        body: String,
        url: String,
    },
    /// Error occurred during gathering
    Error(String),
}

/// Gather full context for a PR including unresolved comments
pub fn gather_pr_context(pr_number: u32) -> PRContextResult {
    // Get basic PR info
    let output = match Command::new("gh")
        .args([
            "pr",
            "view",
            &pr_number.to_string(),
            "--json",
            "title,body,url",
        ])
        .output()
    {
        Ok(o) => o,
        Err(e) => return PRContextResult::Error(format!("Failed to get PR info: {}", e)),
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return PRContextResult::Error(format!("Failed to get PR info: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = match serde_json::from_str(stdout.trim()) {
        Ok(v) => v,
        Err(e) => return PRContextResult::Error(format!("Failed to parse PR info: {}", e)),
    };

    let title = parsed
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let body = parsed
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let url = parsed
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Gather unresolved comments from review threads
    let unresolved_comments = gather_unresolved_comments(pr_number);

    if unresolved_comments.is_empty() {
        return PRContextResult::NoUnresolvedComments {
            number: pr_number,
            title,
            body,
            url,
        };
    }

    PRContextResult::Success(PRContext {
        number: pr_number,
        title,
        body,
        url,
        unresolved_comments,
    })
}

/// Gather unresolved comments from a PR
fn gather_unresolved_comments(pr_number: u32) -> Vec<PRComment> {
    let mut comments = Vec::new();

    // Get review threads
    // Note: This API call for review bodies is currently unused but kept for future use
    let _output = Command::new("gh")
        .args([
            "api",
            &format!(
                "repos/{{owner}}/{{repo}}/pulls/{}/reviews",
                pr_number
            ),
            "--jq",
            ".[].body",
        ])
        .output();

    // Get unresolved review threads using GraphQL
    let graphql_query = format!(
        r#"{{
            repository(owner: "{{owner}}", name: "{{repo}}") {{
                pullRequest(number: {}) {{
                    reviewThreads(first: 100) {{
                        nodes {{
                            id
                            isResolved
                            path
                            line
                            comments(first: 10) {{
                                nodes {{
                                    author {{ login }}
                                    body
                                }}
                            }}
                        }}
                    }}
                }}
            }}
        }}"#,
        pr_number
    );

    let output = match Command::new("gh")
        .args(["api", "graphql", "-f", &format!("query={}", graphql_query)])
        .output()
    {
        Ok(o) => o,
        Err(_) => return comments,
    };

    if !output.status.success() {
        return comments;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(stdout.trim()) {
        if let Some(threads) = parsed
            .pointer("/data/repository/pullRequest/reviewThreads/nodes")
            .and_then(|v| v.as_array())
        {
            for thread in threads {
                let is_resolved = thread
                    .get("isResolved")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);

                if is_resolved {
                    continue;
                }

                let path = thread
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let line = thread.get("line").and_then(|v| v.as_u64()).map(|n| n as u32);
                let thread_id = thread
                    .get("id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                if let Some(thread_comments) = thread
                    .pointer("/comments/nodes")
                    .and_then(|v| v.as_array())
                {
                    for comment in thread_comments {
                        let author = comment
                            .pointer("/author/login")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let body = comment
                            .get("body")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();

                        if !body.is_empty() {
                            comments.push(PRComment {
                                author,
                                body,
                                file_path: path.clone(),
                                line,
                                is_review_thread: true,
                                thread_id: thread_id.clone(),
                            });
                        }
                    }
                }
            }
        }
    }

    comments
}
