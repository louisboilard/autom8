//! Spec generation from markdown.
//!
//! Converts markdown spec files to JSON format using Claude.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use crate::error::{Autom8Error, Result};
use crate::prompts::{SPEC_JSON_CORRECTION_PROMPT, SPEC_JSON_PROMPT};
use crate::spec::Spec;

use super::stream::extract_text_from_stream_line;
use super::types::ClaudeErrorInfo;
use super::utils::{extract_json, fix_json_syntax, truncate_json_preview};

const MAX_JSON_RETRY_ATTEMPTS: u32 = 3;

/// Run Claude to convert a spec-<feature>.md markdown file into spec-<feature>.json
/// Implements retry logic (up to 3 attempts) when JSON parsing fails.
pub fn run_for_spec_generation<F>(
    spec_content: &str,
    output_path: &Path,
    mut on_output: F,
) -> Result<Spec>
where
    F: FnMut(&str),
{
    // First attempt with the initial prompt
    let initial_prompt = SPEC_JSON_PROMPT.replace("{spec_content}", spec_content);
    let mut full_output = run_claude_with_prompt(&initial_prompt, &mut on_output)?;

    // Try to get JSON either from response or from file if Claude wrote it directly
    let mut json_str = if let Some(json) = extract_json(&full_output) {
        json
    } else if output_path.exists() {
        // Claude may have written the file directly using tools
        std::fs::read_to_string(output_path).map_err(|e| {
            Autom8Error::InvalidGeneratedSpec(format!("Failed to read generated file: {}", e))
        })?
    } else {
        let preview = if full_output.len() > 200 {
            format!("{}...", &full_output[..200])
        } else {
            full_output.clone()
        };
        return Err(Autom8Error::InvalidGeneratedSpec(format!(
            "No valid JSON found in response. Response preview: {:?}",
            preview
        )));
    };

    // Try to parse the JSON, with retry logic on failure
    let mut last_error: Option<serde_json::Error> = None;

    for attempt in 1..=MAX_JSON_RETRY_ATTEMPTS {
        match serde_json::from_str::<Spec>(&json_str) {
            Ok(spec) => {
                spec.save(output_path)?;
                return Ok(spec);
            }
            Err(e) => {
                last_error = Some(e);

                if attempt == MAX_JSON_RETRY_ATTEMPTS {
                    break;
                }

                let retry_msg = format!(
                    "\nJSON malformed, retrying (attempt {}/{})...\n",
                    attempt + 1,
                    MAX_JSON_RETRY_ATTEMPTS
                );
                on_output(&retry_msg);

                let correction_prompt = SPEC_JSON_CORRECTION_PROMPT
                    .replace("{spec_content}", spec_content)
                    .replace("{malformed_json}", &json_str)
                    .replace("{error_message}", &last_error.as_ref().unwrap().to_string())
                    .replace("{attempt}", &(attempt + 1).to_string())
                    .replace("{max_attempts}", &MAX_JSON_RETRY_ATTEMPTS.to_string());

                full_output = run_claude_with_prompt(&correction_prompt, &mut on_output)?;

                if let Some(json) = extract_json(&full_output) {
                    json_str = json;
                } else {
                    json_str = full_output.clone();
                }
            }
        }
    }

    // All agentic retries exhausted - try non-agentic fix as final fallback
    on_output("\nAttempting programmatic JSON fix...\n");

    let fixed_json = fix_json_syntax(&json_str);

    match serde_json::from_str::<Spec>(&fixed_json) {
        Ok(spec) => {
            on_output("Programmatic fix succeeded!\n");
            spec.save(output_path)?;
            Ok(spec)
        }
        Err(fallback_err) => {
            let agentic_error = last_error
                .map(|e| e.to_string())
                .unwrap_or_else(|| "Unknown error".to_string());
            let fallback_error = fallback_err.to_string();

            let json_preview = truncate_json_preview(&json_str, 500);

            Err(Autom8Error::InvalidGeneratedSpec(format!(
                "JSON generation failed after {} agentic attempts and programmatic fallback.\n\n\
                 Agent error: {}\n\n\
                 Fallback error: {}\n\n\
                 Malformed JSON preview:\n{}",
                MAX_JSON_RETRY_ATTEMPTS, agentic_error, fallback_error, json_preview
            )))
        }
    }
}

/// Helper function to run Claude with a given prompt and return the raw output.
fn run_claude_with_prompt<F>(prompt: &str, mut on_output: F) -> Result<String>
where
    F: FnMut(&str),
{
    let mut child = Command::new("claude")
        .args([
            "--dangerously-skip-permissions",
            "--print",
            "--output-format",
            "stream-json",
            "--verbose",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Autom8Error::ClaudeError(format!("Failed to spawn claude: {}", e)))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(prompt.as_bytes())
            .map_err(|e| Autom8Error::ClaudeError(format!("Failed to write to stdin: {}", e)))?;
    }

    let stderr = child.stderr.take();

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Autom8Error::ClaudeError("Failed to capture stdout".into()))?;

    let reader = BufReader::new(stdout);
    let mut full_output = String::new();

    for line in reader.lines() {
        let line = line.map_err(|e| Autom8Error::ClaudeError(format!("Read error: {}", e)))?;

        if let Some(text) = extract_text_from_stream_line(&line) {
            on_output(&text);
            full_output.push_str(&text);
        }
    }

    let status = child
        .wait()
        .map_err(|e| Autom8Error::ClaudeError(format!("Wait error: {}", e)))?;

    if !status.success() {
        let stderr_content = stderr
            .map(|s| std::io::read_to_string(s).unwrap_or_default())
            .unwrap_or_default();
        let error_info = ClaudeErrorInfo::from_process_failure(
            status,
            if stderr_content.is_empty() {
                None
            } else {
                Some(stderr_content)
            },
        );
        return Err(Autom8Error::SpecGenerationFailed(error_info.message));
    }

    Ok(full_output)
}
