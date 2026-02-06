//! Control protocol types for Claude CLI permission handling.
//!
//! When running Claude with permission restrictions, the CLI may emit control
//! requests asking for approval to use blocked tools. This module provides
//! serde types for parsing these requests and generating responses.
//!
//! # Message Formats
//!
//! Incoming request:
//! ```json
//! {"type":"control_request","request_id":"...","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"git push"}}}
//! ```
//!
//! Allow response:
//! ```json
//! {"type":"control_response","response":{"subtype":"success","request_id":"...","response":{"behavior":"allow","updatedInput":{...}}}}
//! ```
//!
//! Deny response:
//! ```json
//! {"type":"control_response","response":{"subtype":"success","request_id":"...","response":{"behavior":"deny","message":"User declined"}}}
//! ```

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Incoming control request from Claude CLI.
///
/// This is emitted when Claude attempts to use a tool that requires permission.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControlRequest {
    /// Always "control_request" for incoming requests.
    #[serde(rename = "type")]
    pub message_type: String,
    /// Unique identifier for this request (used in response).
    pub request_id: String,
    /// The permission request details.
    pub request: ToolUseRequest,
}

/// Details of a tool use permission request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolUseRequest {
    /// Request subtype, typically "can_use_tool".
    pub subtype: String,
    /// Name of the tool being requested (e.g., "Bash").
    pub tool_name: String,
    /// Tool input parameters (e.g., {"command": "git push"}).
    pub input: Value,
}

/// Response to a control request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControlResponse {
    /// Always "control_response" for outgoing responses.
    #[serde(rename = "type")]
    pub message_type: String,
    /// The response wrapper.
    pub response: ResponseWrapper,
}

/// Wrapper for the actual response content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseWrapper {
    /// Response subtype, typically "success".
    pub subtype: String,
    /// ID of the request being responded to.
    pub request_id: String,
    /// The permission decision.
    pub response: PermissionDecision,
}

/// The permission decision for a tool use request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "behavior")]
pub enum PermissionDecision {
    /// Allow the tool use, optionally with modified input.
    #[serde(rename = "allow")]
    Allow {
        /// Modified tool input, if any. Uses camelCase per spec.
        #[serde(rename = "updatedInput", skip_serializing_if = "Option::is_none")]
        updated_input: Option<Value>,
    },
    /// Deny the tool use with an explanatory message.
    #[serde(rename = "deny")]
    Deny {
        /// Message explaining why the request was denied.
        message: String,
    },
}

/// Result of a permission prompt shown to the user.
#[derive(Debug, Clone, PartialEq)]
pub enum PermissionResult {
    /// User allowed the operation, optionally with modified input.
    Allow(Option<Value>),
    /// User denied the operation with a reason.
    Deny(String),
}

impl ControlRequest {
    /// Check if this is a tool use permission request.
    pub fn is_tool_use_request(&self) -> bool {
        self.message_type == "control_request" && self.request.subtype == "can_use_tool"
    }
}

impl ControlResponse {
    /// Create an "allow" response for the given request.
    pub fn allow(request_id: &str, updated_input: Option<Value>) -> Self {
        Self {
            message_type: "control_response".to_string(),
            response: ResponseWrapper {
                subtype: "success".to_string(),
                request_id: request_id.to_string(),
                response: PermissionDecision::Allow { updated_input },
            },
        }
    }

    /// Create a "deny" response for the given request.
    pub fn deny(request_id: &str, message: impl Into<String>) -> Self {
        Self {
            message_type: "control_response".to_string(),
            response: ResponseWrapper {
                subtype: "success".to_string(),
                request_id: request_id.to_string(),
                response: PermissionDecision::Deny {
                    message: message.into(),
                },
            },
        }
    }

    /// Create a response from a PermissionResult.
    pub fn from_result(request_id: &str, result: PermissionResult) -> Self {
        match result {
            PermissionResult::Allow(updated_input) => Self::allow(request_id, updated_input),
            PermissionResult::Deny(message) => Self::deny(request_id, message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ControlRequest tests

    #[test]
    fn test_deserialize_control_request() {
        let json = r#"{"type":"control_request","request_id":"req-123","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"git push"}}}"#;
        let request: ControlRequest = serde_json::from_str(json).unwrap();

        assert_eq!(request.message_type, "control_request");
        assert_eq!(request.request_id, "req-123");
        assert_eq!(request.request.subtype, "can_use_tool");
        assert_eq!(request.request.tool_name, "Bash");
        assert_eq!(request.request.input, json!({"command": "git push"}));
    }

    #[test]
    fn test_serialize_control_request() {
        let request = ControlRequest {
            message_type: "control_request".to_string(),
            request_id: "req-456".to_string(),
            request: ToolUseRequest {
                subtype: "can_use_tool".to_string(),
                tool_name: "Bash".to_string(),
                input: json!({"command": "git push origin main"}),
            },
        };

        let json = serde_json::to_string(&request).unwrap();
        let parsed: Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["type"], "control_request");
        assert_eq!(parsed["request_id"], "req-456");
        assert_eq!(parsed["request"]["subtype"], "can_use_tool");
        assert_eq!(parsed["request"]["tool_name"], "Bash");
        assert_eq!(
            parsed["request"]["input"]["command"],
            "git push origin main"
        );
    }

    #[test]
    fn test_is_tool_use_request() {
        let request = ControlRequest {
            message_type: "control_request".to_string(),
            request_id: "req-123".to_string(),
            request: ToolUseRequest {
                subtype: "can_use_tool".to_string(),
                tool_name: "Bash".to_string(),
                input: json!({}),
            },
        };
        assert!(request.is_tool_use_request());
    }

    #[test]
    fn test_is_tool_use_request_wrong_type() {
        let request = ControlRequest {
            message_type: "other_type".to_string(),
            request_id: "req-123".to_string(),
            request: ToolUseRequest {
                subtype: "can_use_tool".to_string(),
                tool_name: "Bash".to_string(),
                input: json!({}),
            },
        };
        assert!(!request.is_tool_use_request());
    }

    #[test]
    fn test_is_tool_use_request_wrong_subtype() {
        let request = ControlRequest {
            message_type: "control_request".to_string(),
            request_id: "req-123".to_string(),
            request: ToolUseRequest {
                subtype: "other_subtype".to_string(),
                tool_name: "Bash".to_string(),
                input: json!({}),
            },
        };
        assert!(!request.is_tool_use_request());
    }

    // ControlResponse tests - Allow

    #[test]
    fn test_serialize_allow_response_without_updated_input() {
        let response = ControlResponse::allow("req-123", None);
        let json = serde_json::to_string(&response).unwrap();
        let parsed: Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["type"], "control_response");
        assert_eq!(parsed["response"]["subtype"], "success");
        assert_eq!(parsed["response"]["request_id"], "req-123");
        assert_eq!(parsed["response"]["response"]["behavior"], "allow");
        // updatedInput should be absent when None
        assert!(parsed["response"]["response"].get("updatedInput").is_none());
    }

    #[test]
    fn test_serialize_allow_response_with_updated_input() {
        let response =
            ControlResponse::allow("req-456", Some(json!({"command": "git push --dry-run"})));
        let json = serde_json::to_string(&response).unwrap();
        let parsed: Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["type"], "control_response");
        assert_eq!(parsed["response"]["subtype"], "success");
        assert_eq!(parsed["response"]["request_id"], "req-456");
        assert_eq!(parsed["response"]["response"]["behavior"], "allow");
        assert_eq!(
            parsed["response"]["response"]["updatedInput"]["command"],
            "git push --dry-run"
        );
    }

    #[test]
    fn test_deserialize_allow_response() {
        let json = r#"{"type":"control_response","response":{"subtype":"success","request_id":"req-789","response":{"behavior":"allow","updatedInput":{"command":"modified"}}}}"#;
        let response: ControlResponse = serde_json::from_str(json).unwrap();

        assert_eq!(response.message_type, "control_response");
        assert_eq!(response.response.subtype, "success");
        assert_eq!(response.response.request_id, "req-789");
        match response.response.response {
            PermissionDecision::Allow { updated_input } => {
                assert_eq!(updated_input, Some(json!({"command": "modified"})));
            }
            _ => panic!("Expected Allow variant"),
        }
    }

    // ControlResponse tests - Deny

    #[test]
    fn test_serialize_deny_response() {
        let response = ControlResponse::deny("req-123", "User declined");
        let json = serde_json::to_string(&response).unwrap();
        let parsed: Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["type"], "control_response");
        assert_eq!(parsed["response"]["subtype"], "success");
        assert_eq!(parsed["response"]["request_id"], "req-123");
        assert_eq!(parsed["response"]["response"]["behavior"], "deny");
        assert_eq!(parsed["response"]["response"]["message"], "User declined");
    }

    #[test]
    fn test_deserialize_deny_response() {
        let json = r#"{"type":"control_response","response":{"subtype":"success","request_id":"req-abc","response":{"behavior":"deny","message":"Operation not allowed"}}}"#;
        let response: ControlResponse = serde_json::from_str(json).unwrap();

        assert_eq!(response.message_type, "control_response");
        assert_eq!(response.response.subtype, "success");
        assert_eq!(response.response.request_id, "req-abc");
        match response.response.response {
            PermissionDecision::Deny { message } => {
                assert_eq!(message, "Operation not allowed");
            }
            _ => panic!("Expected Deny variant"),
        }
    }

    // ControlResponse::from_result tests

    #[test]
    fn test_from_result_allow() {
        let result = PermissionResult::Allow(None);
        let response = ControlResponse::from_result("req-test", result);

        assert_eq!(response.response.request_id, "req-test");
        match response.response.response {
            PermissionDecision::Allow { updated_input } => {
                assert!(updated_input.is_none());
            }
            _ => panic!("Expected Allow variant"),
        }
    }

    #[test]
    fn test_from_result_allow_with_input() {
        let result = PermissionResult::Allow(Some(json!({"modified": true})));
        let response = ControlResponse::from_result("req-test", result);

        match response.response.response {
            PermissionDecision::Allow { updated_input } => {
                assert_eq!(updated_input, Some(json!({"modified": true})));
            }
            _ => panic!("Expected Allow variant"),
        }
    }

    #[test]
    fn test_from_result_deny() {
        let result = PermissionResult::Deny("Not authorized".to_string());
        let response = ControlResponse::from_result("req-test", result);

        assert_eq!(response.response.request_id, "req-test");
        match response.response.response {
            PermissionDecision::Deny { message } => {
                assert_eq!(message, "Not authorized");
            }
            _ => panic!("Expected Deny variant"),
        }
    }

    // Round-trip tests

    #[test]
    fn test_control_request_roundtrip() {
        let original = ControlRequest {
            message_type: "control_request".to_string(),
            request_id: "roundtrip-test".to_string(),
            request: ToolUseRequest {
                subtype: "can_use_tool".to_string(),
                tool_name: "Bash".to_string(),
                input: json!({"command": "git push", "timeout": 30}),
            },
        };

        let json = serde_json::to_string(&original).unwrap();
        let deserialized: ControlRequest = serde_json::from_str(&json).unwrap();

        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_allow_response_roundtrip() {
        let original = ControlResponse::allow("roundtrip-allow", Some(json!({"test": "data"})));

        let json = serde_json::to_string(&original).unwrap();
        let deserialized: ControlResponse = serde_json::from_str(&json).unwrap();

        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_deny_response_roundtrip() {
        let original = ControlResponse::deny("roundtrip-deny", "Test denial reason");

        let json = serde_json::to_string(&original).unwrap();
        let deserialized: ControlResponse = serde_json::from_str(&json).unwrap();

        assert_eq!(original, deserialized);
    }

    // PermissionResult tests

    #[test]
    fn test_permission_result_allow_clone() {
        let result = PermissionResult::Allow(Some(json!({"key": "value"})));
        let cloned = result.clone();
        assert_eq!(result, cloned);
    }

    #[test]
    fn test_permission_result_deny_clone() {
        let result = PermissionResult::Deny("reason".to_string());
        let cloned = result.clone();
        assert_eq!(result, cloned);
    }

    #[test]
    fn test_permission_result_equality() {
        let allow1 = PermissionResult::Allow(None);
        let allow2 = PermissionResult::Allow(None);
        let deny1 = PermissionResult::Deny("msg".to_string());
        let deny2 = PermissionResult::Deny("msg".to_string());

        assert_eq!(allow1, allow2);
        assert_eq!(deny1, deny2);
        assert_ne!(allow1, deny1);
    }
}
