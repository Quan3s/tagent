//! Provider-neutral message/tool schema and adapters.

pub mod anthropic;
pub mod openai;

use crate::config::ProviderConfig;
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use serde_json::Value;
use std::{fmt, sync::Arc};

/// Debug/Display never expose the wrapped value.
pub struct ApiKey(pub(crate) String);

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey([redacted])")
    }
}

impl ApiKey {
    pub fn new(value: String) -> anyhow::Result<Self> {
        if value.trim().is_empty() {
            anyhow::bail!(
                "API key is empty; set the key environment variable or run `ta auth add`"
            );
        }
        Ok(Self(value))
    }
}

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
    ThinkingDelta(String),
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

/// Build a provider from config, resolving the BYOK key without logging it.
pub fn build(name: &str, cfg: &ProviderConfig) -> Result<DynProvider> {
    let key = match cfg.resolve_key(name) {
        Some(key) => key,
        None => {
            // Local OpenAI-compatible servers (Ollama, llama.cpp) need no key.
            let local = cfg.base_url().starts_with("http://127.0.0.1")
                || cfg.base_url().starts_with("http://localhost")
                || cfg.base_url().starts_with("http://[::1]");
            if local {
                "local".to_string()
            } else {
                return Err(anyhow::anyhow!(
                    "no API key configured for provider '{name}' (run `ta auth add {name}` or set {})",
                    cfg.key_env_name()
                ));
            }
        }
    };
    match cfg {
        ProviderConfig::Anthropic { .. } => {
            Ok(Arc::new(anthropic::Anthropic::new(key, cfg.base_url())?))
        }
        ProviderConfig::OpenaiCompat { .. }
        | ProviderConfig::Openrouter { .. }
        | ProviderConfig::Gemini { .. } => {
            Ok(Arc::new(openai::OpenAiCompat::new(key, cfg.base_url())?))
        }
    }
}
