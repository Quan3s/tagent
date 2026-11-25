//! Provider-neutral message/tool schema. M0 ships exactly one adapter: Anthropic.

pub mod anthropic;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use serde_json::Value;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

impl Message {
    pub fn user_text(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: vec![ContentBlock::Text { text: text.into() }],
        }
    }

    pub fn assistant_text(text: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: vec![ContentBlock::Text { text: text.into() }],
        }
    }
}

#[derive(Clone, Debug)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Clone, Debug)]
pub struct CompletionRequest {
    pub model: String,
    pub system: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSpec>,
    pub max_tokens: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ProviderEvent {
    TextDelta(String),
    ToolUseStart {
        id: String,
        name: String,
    },
    /// Emitted once the streamed JSON input object is complete.
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    MessageStop {
        stop_reason: Option<String>,
        usage: Usage,
    },
}

#[async_trait]
pub trait Provider: Send + Sync {
    async fn complete(
        &self,
        req: CompletionRequest,
        emit: &mut (dyn FnMut(ProviderEvent) + Send),
    ) -> Result<()>;
}

pub type DynProvider = Arc<dyn Provider>;
