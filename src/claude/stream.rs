//! JSON stream parsing for Claude CLI output.
//!
//! Handles parsing of newline-delimited JSON from Claude CLI's
//! stream-json output format.

use serde::Deserialize;
use std::collections::HashMap;

use super::types::ClaudeUsage;

/// Top-level stream event wrapper
#[derive(Debug, Deserialize)]
struct StreamLine {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    event: Option<StreamEventInner>,
    #[serde(default)]
    message: Option<AssistantMessage>,
    #[serde(default)]
    result: Option<String>,
}

/// Inner event content for stream_event types
#[derive(Debug, Deserialize)]
struct StreamEventInner {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    delta: Option<ContentDelta>,
}

/// Content delta containing text updates
#[derive(Debug, Deserialize)]
struct ContentDelta {
    #[serde(default)]
    text: Option<String>,
}

/// Assistant message containing content blocks
#[derive(Debug, Deserialize)]
struct AssistantMessage {
    #[serde(default)]
    content: Vec<ContentBlock>,
}

/// Content block that may contain text
#[derive(Debug, Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    #[serde(default)]
    text: Option<String>,
}

/// Result event from Claude CLI containing usage metadata.
///
/// This struct deserializes the final `result` event from Claude CLI's
/// stream-json output, which contains token usage information.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResultEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    usage: Option<ResultUsage>,
    #[serde(default)]
    model_usage: Option<HashMap<String, ModelUsageEntry>>,
}

/// Usage statistics from the result event.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ResultUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
}

/// Per-model usage entry from modelUsage.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ModelUsageEntry {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    #[serde(default)]
    thinking_tokens: u64,
}

/// Extract usage data from a result event stream line.
///
/// Parses the final `result` event from Claude CLI output and extracts
/// token usage information. When multiple models are present in `modelUsage`,
/// tokens are summed across all models but only the primary (first) model
/// name is stored.
///
/// # Arguments
/// * `line` - A JSON line from Claude CLI's stream-json output
///
/// # Returns
/// * `Some(ClaudeUsage)` if the line is a result event with usage data
/// * `None` if the line is not a result event or has no usage data
pub fn extract_usage_from_result_line(line: &str) -> Option<ClaudeUsage> {
    let parsed: ResultEvent = serde_json::from_str(line).ok()?;

    // Only process result events
    if parsed.event_type != "result" {
        return None;
    }

    // Prefer modelUsage if available (more detailed, includes thinking tokens)
    if let Some(model_usage) = parsed.model_usage {
        if !model_usage.is_empty() {
            let mut usage = ClaudeUsage::default();

            // Get the primary model name (first key)
            let primary_model = model_usage.keys().next().cloned();
            usage.model = primary_model;

            // Sum tokens across all models
            for entry in model_usage.values() {
                usage.input_tokens += entry.input_tokens;
                usage.output_tokens += entry.output_tokens;
                usage.cache_read_tokens += entry.cache_read_input_tokens;
                usage.cache_creation_tokens += entry.cache_creation_input_tokens;
                usage.thinking_tokens += entry.thinking_tokens;
            }

            return Some(usage);
        }
    }

    // Fall back to top-level usage if modelUsage is not available
    if let Some(result_usage) = parsed.usage {
        return Some(ClaudeUsage {
            input_tokens: result_usage.input_tokens,
            output_tokens: result_usage.output_tokens,
            cache_read_tokens: result_usage.cache_read_input_tokens,
            cache_creation_tokens: result_usage.cache_creation_input_tokens,
            thinking_tokens: 0,
            model: None,
        });
    }

    None
}

/// Extract text content from a stream JSON line
pub fn extract_text_from_stream_line(line: &str) -> Option<String> {
    let parsed: StreamLine = serde_json::from_str(line).ok()?;

    match parsed.event_type.as_str() {
        // Handle incremental text deltas from streaming
        "stream_event" => {
            if let Some(event) = parsed.event {
                if event.event_type == "content_block_delta" {
                    if let Some(delta) = event.delta {
                        return delta.text;
                    }
                }
            }
            None
        }
        // Handle complete assistant messages
        "assistant" => {
            if let Some(message) = parsed.message {
                let text: String = message
                    .content
                    .iter()
                    .filter(|block| block.block_type == "text")
                    .filter_map(|block| block.text.as_ref())
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("");
                if !text.is_empty() {
                    return Some(text);
                }
            }
            None
        }
        // Handle final result
        "result" => parsed.result,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_text_from_stream_event_content_block_delta() {
        let line = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello world"}},"session_id":"test"}"#;
        let text = extract_text_from_stream_line(line);
        assert_eq!(text, Some("Hello world".to_string()));
    }

    #[test]
    fn test_extract_text_from_assistant_message() {
        let line = r#"{"type":"assistant","message":{"model":"claude","id":"msg_123","type":"message","role":"assistant","content":[{"type":"text","text":"Complete response here"}]},"session_id":"test"}"#;
        let text = extract_text_from_stream_line(line);
        assert_eq!(text, Some("Complete response here".to_string()));
    }

    #[test]
    fn test_extract_text_from_result() {
        let line = r#"{"type":"result","subtype":"success","is_error":false,"duration_ms":1000,"result":"Final result text","session_id":"test"}"#;
        let text = extract_text_from_stream_line(line);
        assert_eq!(text, Some("Final result text".to_string()));
    }

    #[test]
    fn test_extract_text_from_system_event_returns_none() {
        let line = r#"{"type":"system","subtype":"init","cwd":"/test","session_id":"test"}"#;
        let text = extract_text_from_stream_line(line);
        assert_eq!(text, None);
    }

    #[test]
    fn test_extract_text_from_invalid_json_returns_none() {
        let line = "not valid json at all";
        let text = extract_text_from_stream_line(line);
        assert_eq!(text, None);
    }

    // Usage extraction tests

    #[test]
    fn test_extract_usage_from_result_with_model_usage() {
        let line = r#"{"type":"result","subtype":"success","is_error":false,"duration_ms":1000,"result":"done","usage":{"input_tokens":100,"output_tokens":50},"modelUsage":{"claude-sonnet-4-20250514":{"inputTokens":100,"outputTokens":50,"cacheReadInputTokens":25,"cacheCreationInputTokens":10,"thinkingTokens":5}}}"#;
        let usage = extract_usage_from_result_line(line);
        assert!(usage.is_some());
        let usage = usage.unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.cache_read_tokens, 25);
        assert_eq!(usage.cache_creation_tokens, 10);
        assert_eq!(usage.thinking_tokens, 5);
        assert_eq!(usage.model, Some("claude-sonnet-4-20250514".to_string()));
    }

    #[test]
    fn test_extract_usage_from_result_with_multiple_models() {
        // When multiple models are present, tokens should be summed
        let line = r#"{"type":"result","subtype":"success","modelUsage":{"claude-sonnet-4-20250514":{"inputTokens":100,"outputTokens":50,"cacheReadInputTokens":25,"cacheCreationInputTokens":10,"thinkingTokens":5},"claude-haiku-3-20240307":{"inputTokens":200,"outputTokens":100,"cacheReadInputTokens":50,"cacheCreationInputTokens":20,"thinkingTokens":0}}}"#;
        let usage = extract_usage_from_result_line(line);
        assert!(usage.is_some());
        let usage = usage.unwrap();
        // Sum of both models
        assert_eq!(usage.input_tokens, 300);
        assert_eq!(usage.output_tokens, 150);
        assert_eq!(usage.cache_read_tokens, 75);
        assert_eq!(usage.cache_creation_tokens, 30);
        assert_eq!(usage.thinking_tokens, 5);
        // Model should be one of the keys (we just take the first)
        assert!(usage.model.is_some());
    }

    #[test]
    fn test_extract_usage_from_result_fallback_to_usage() {
        // When modelUsage is not present, fall back to top-level usage
        let line = r#"{"type":"result","subtype":"success","usage":{"inputTokens":100,"outputTokens":50,"cacheReadInputTokens":25,"cacheCreationInputTokens":10}}"#;
        let usage = extract_usage_from_result_line(line);
        assert!(usage.is_some());
        let usage = usage.unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.cache_read_tokens, 25);
        assert_eq!(usage.cache_creation_tokens, 10);
        assert_eq!(usage.thinking_tokens, 0);
        assert_eq!(usage.model, None);
    }

    #[test]
    fn test_extract_usage_from_result_with_missing_fields() {
        // Missing fields should default to 0
        let line = r#"{"type":"result","subtype":"success","modelUsage":{"claude-sonnet-4-20250514":{"inputTokens":100,"outputTokens":50}}}"#;
        let usage = extract_usage_from_result_line(line);
        assert!(usage.is_some());
        let usage = usage.unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.cache_read_tokens, 0);
        assert_eq!(usage.cache_creation_tokens, 0);
        assert_eq!(usage.thinking_tokens, 0);
        assert_eq!(usage.model, Some("claude-sonnet-4-20250514".to_string()));
    }

    #[test]
    fn test_extract_usage_from_result_with_empty_model_usage() {
        // Empty modelUsage should fall back to top-level usage
        let line = r#"{"type":"result","subtype":"success","usage":{"inputTokens":100,"outputTokens":50},"modelUsage":{}}"#;
        let usage = extract_usage_from_result_line(line);
        assert!(usage.is_some());
        let usage = usage.unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
    }

    #[test]
    fn test_extract_usage_from_non_result_event() {
        let line = r#"{"type":"system","subtype":"init","cwd":"/test"}"#;
        let usage = extract_usage_from_result_line(line);
        assert!(usage.is_none());
    }

    #[test]
    fn test_extract_usage_from_assistant_event() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hello"}]}}"#;
        let usage = extract_usage_from_result_line(line);
        assert!(usage.is_none());
    }

    #[test]
    fn test_extract_usage_from_invalid_json() {
        let line = "not valid json";
        let usage = extract_usage_from_result_line(line);
        assert!(usage.is_none());
    }

    #[test]
    fn test_extract_usage_from_result_no_usage_data() {
        // Result event without any usage data
        let line = r#"{"type":"result","subtype":"success","result":"done"}"#;
        let usage = extract_usage_from_result_line(line);
        assert!(usage.is_none());
    }

    #[test]
    fn test_extract_usage_from_result_with_only_zeros() {
        // Result with all zeros should still return valid ClaudeUsage
        let line = r#"{"type":"result","subtype":"success","modelUsage":{"claude-sonnet-4-20250514":{"inputTokens":0,"outputTokens":0}}}"#;
        let usage = extract_usage_from_result_line(line);
        assert!(usage.is_some());
        let usage = usage.unwrap();
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
        assert_eq!(usage.total_tokens(), 0);
    }
}
