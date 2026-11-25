//! Event protocol shared by the agent loop, sessions and (later) the Web UI.
//!
//! Every persisted record carries a monotonically increasing `id`, so a client
//! can resume a stream with `last_event_id` (M1) or replay a whole session (M0).

pub use crate::provider::Role;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Event names follow the protocol in `docs/PLAN.md` §3.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Event {
    #[serde(rename = "message.delta")]
    MessageDelta { role: Role, text: String },
    #[serde(rename = "thinking.delta")]
    ThinkingDelta { text: String },
    #[serde(rename = "tool.start")]
    ToolStart {
        #[serde(rename = "tool_use_id")]
        id: String,
        name: String,
        args: Value,
    },
    #[serde(rename = "tool.result")]
    ToolResult {
        #[serde(rename = "tool_use_id")]
        id: String,
        name: String,
        output: String,
        is_error: bool,
        elapsed_ms: u64,
    },
    #[serde(rename = "permission.request")]
    PermissionRequest {
        tool: String,
        target: String,
        decision: String,
    },
    #[serde(rename = "usage")]
    Usage {
        input_tokens: u64,
        output_tokens: u64,
    },
    #[serde(rename = "error")]
    Error { message: String },
    #[serde(rename = "done")]
    Done {
        reason: String,
        session_id: String,
        turns: u32,
    },
}

impl Event {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::MessageDelta { .. } => "message.delta",
            Self::ThinkingDelta { .. } => "thinking.delta",
            Self::ToolStart { .. } => "tool.start",
            Self::ToolResult { .. } => "tool.result",
            Self::PermissionRequest { .. } => "permission.request",
            Self::Usage { .. } => "usage",
            Self::Error { .. } => "error",
            Self::Done { .. } => "done",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record {
    pub id: u64,
    pub ts: u64,
    #[serde(flatten)]
    pub event: Event,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_type_names_match_protocol() {
        let e = Event::MessageDelta {
            role: Role::Assistant,
            text: "hi".into(),
        };
        assert_eq!(serde_json::to_value(&e).unwrap()["type"], "message.delta");

        let e = Event::ToolResult {
            id: "t1".into(),
            name: "read".into(),
            output: "x".into(),
            is_error: false,
            elapsed_ms: 3,
        };
        assert_eq!(e.kind(), "tool.result");
        assert_eq!(serde_json::to_value(&e).unwrap()["type"], "tool.result");
    }
}
