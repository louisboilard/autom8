//! JSON stream parsing for Claude CLI output.
//!
//! Handles parsing of newline-delimited JSON from Claude CLI's
//! stream-json output format.

use serde::Deserialize;

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
}
