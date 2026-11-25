//! Anthropic Messages API adapter (BYOK, rustls only, streamed SSE).
//!
//! The key is read from the configured environment variable and is only ever
//! placed in a request header. It is never cloned into an event, a log line or
//! an error message.

use super::{CompletionRequest, ContentBlock, Message, Provider, ProviderEvent, Role, Usage};
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
use std::{collections::HashMap, fmt};

/// Debug/Display never expose the wrapped value.
struct ApiKey(String);

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey([redacted])")
    }
}

pub struct Anthropic {
    client: Client,
    base_url: String,
    key: ApiKey,
}

impl fmt::Debug for Anthropic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Anthropic")
            .field("base_url", &self.base_url)
            .field("key", &self.key)
            .finish()
    }
}

impl Anthropic {
    pub fn new(api_key: String, base_url: &str) -> Result<Self> {
        if api_key.trim().is_empty() {
            bail!("API key is empty; set the key environment variable from your config");
        }
        Ok(Self {
            // default-features = false + `rustls-tls`: no system OpenSSL needed.
            client: Client::builder()
                .build()
                .context("cannot build HTTP client")?,
            base_url: base_url.trim_end_matches('/').to_string(),
            key: ApiKey(api_key),
        })
    }

    pub fn api_key_name(&self) -> &'static str {
        "x-api-key"
    }
}

#[async_trait]
impl Provider for Anthropic {
    async fn complete(
        &self,
        req: CompletionRequest,
        emit: &mut (dyn FnMut(ProviderEvent) + Send),
    ) -> Result<()> {
        let url = format!("{}/v1/messages", self.base_url);
        let body = json!({
            "model": req.model,
            "max_tokens": req.max_tokens,
            "system": req.system,
            "messages": to_wire_messages(&req.messages),
            "tools": req.tools.iter().map(|t| json!({
                "name": t.name,
                "description": t.description,
                "input_schema": t.input_schema,
            })).collect::<Vec<_>>(),
            "stream": true,
        });

        let response = self
            .client
            .post(&url)
            .header("x-api-key", &self.key.0)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("request to provider failed")?;

        if response.status() != StatusCode::OK {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!(
                "provider returned HTTP {}: {}",
                status.as_u16(),
                truncate(&body, 1024)
            );
        }

        let mut state = StreamState::default();
        let mut buffer = String::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("failed while reading provider stream")?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));
            buffer = buffer.replace("\r\n", "\n");
            while let Some(pos) = buffer.find("\n\n") {
                let block: String = buffer.drain(..pos + 2).collect();
                handle_block(&block, &mut state, emit)?;
            }
        }
        if !buffer.trim().is_empty() {
            handle_block(&buffer, &mut state, emit)?;
        }
        if !state.stopped {
            emit(ProviderEvent::MessageStop {
                stop_reason: None,
                usage: state.usage,
            });
        }
        Ok(())
    }
}

fn truncate(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((byte, _)) => &s[..byte],
        None => s,
    }
}

fn to_wire_messages(messages: &[Message]) -> Vec<Value> {
    messages
        .iter()
        .map(|m| {
            let mut content: Vec<Value> = m
                .content
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text } if text.is_empty() => None,
                    ContentBlock::Text { text } => Some(json!({"type": "text", "text": text})),
                    ContentBlock::ToolUse { id, name, input } => {
                        Some(json!({"type": "tool_use", "id": id, "name": name, "input": input}))
                    }
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    } => Some(json!({
                        "type": "tool_result",
                        "tool_use_id": tool_use_id,
                        "content": content,
                        "is_error": *is_error,
                    })),
                })
                .collect();
            if content.is_empty() {
                content.push(json!({"type": "text", "text": " "}));
            }
            let role = match m.role {
                Role::User => "user",
                Role::Assistant => "assistant",
            };
            json!({"role": role, "content": content})
        })
        .collect()
}

#[derive(Default)]
struct ToolAcc {
    id: String,
    name: String,
    json: String,
}

#[derive(Default)]
struct StreamState {
    usage: Usage,
    stop_reason: Option<String>,
    stopped: bool,
    tools: HashMap<u64, ToolAcc>,
}

/// Parse one SSE frame (text between blank lines). Public to tests so the
/// conformance cases do not need a network server.
fn handle_block(
    block: &str,
    state: &mut StreamState,
    emit: &mut dyn FnMut(ProviderEvent),
) -> Result<()> {
    let data = block
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(str::trim_start)
        .collect::<Vec<_>>()
        .join("\n");
    if data.is_empty() || data == "[DONE]" {
        return Ok(());
    }
    let value: Value = serde_json::from_str(&data).context("invalid SSE payload from provider")?;
    handle_event(value, state, emit)
}

fn handle_event(
    value: Value,
    state: &mut StreamState,
    emit: &mut dyn FnMut(ProviderEvent),
) -> Result<()> {
    let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
    match kind {
        "message_start" => {
            if let Some(n) = value
                .pointer("/message/usage/input_tokens")
                .and_then(Value::as_u64)
            {
                state.usage.input_tokens = n;
            }
        }
        "content_block_start" => {
            let index = value.get("index").and_then(Value::as_u64).unwrap_or(0);
            if let Some(block) = value.get("content_block") {
                if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                    let id = block
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    let name = block
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    state.tools.insert(
                        index,
                        ToolAcc {
                            id: id.clone(),
                            name: name.clone(),
                            json: String::new(),
                        },
                    );
                    emit(ProviderEvent::ToolUseStart { id, name });
                }
            }
        }
        "content_block_delta" => {
            let index = value.get("index").and_then(Value::as_u64).unwrap_or(0);
            match value.pointer("/delta/type").and_then(Value::as_str) {
                Some("text_delta") => {
                    if let Some(text) = value.pointer("/delta/text").and_then(Value::as_str) {
                        emit(ProviderEvent::TextDelta(text.to_string()));
                    }
                }
                Some("input_json_delta") => {
                    if let Some(part) = value.pointer("/delta/partial_json").and_then(Value::as_str)
                    {
                        if let Some(tool) = state.tools.get_mut(&index) {
                            tool.json.push_str(part);
                        }
                    }
                }
                _ => {}
            }
        }
        "content_block_stop" => {
            let index = value.get("index").and_then(Value::as_u64).unwrap_or(0);
            if let Some(tool) = state.tools.remove(&index) {
                let input = if tool.json.trim().is_empty() {
                    json!({})
                } else {
                    serde_json::from_str(&tool.json)
                        .context("provider streamed an invalid tool input object")?
                };
                emit(ProviderEvent::ToolUse {
                    id: tool.id,
                    name: tool.name,
                    input,
                });
            }
        }
        "message_delta" => {
            if let Some(reason) = value.pointer("/delta/stop_reason").and_then(Value::as_str) {
                state.stop_reason = Some(reason.to_string());
            }
            if let Some(n) = value
                .pointer("/usage/output_tokens")
                .and_then(Value::as_u64)
            {
                state.usage.output_tokens = n;
            }
        }
        "message_stop" => {
            state.stopped = true;
            emit(ProviderEvent::MessageStop {
                stop_reason: state.stop_reason.take(),
                usage: state.usage,
            });
        }
        "error" => {
            let kind = value
                .pointer("/error/type")
                .and_then(Value::as_str)
                .unwrap_or("unknown_error");
            let message = value
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("provider error");
            bail!("provider stream error: {kind}: {}", truncate(message, 512));
        }
        // `ping` and future event types are intentionally ignored.
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(raw: &str) -> (Vec<ProviderEvent>, StreamState) {
        let mut events = Vec::new();
        let mut state = StreamState::default();
        for block in raw.split("\n\n") {
            if !block.trim().is_empty() {
                handle_block(block, &mut state, &mut |e| events.push(e)).unwrap();
            }
        }
        (events, state)
    }

    #[test]
    fn parses_text_and_tool_use_stream() {
        let raw = concat!(
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":42}}}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"tu_1\",\"name\":\"read\",\"input\":{}}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"path\\\":\\\"a.txt\\\"}\"}}\n\n",
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":7}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        );
        let (events, state) = run(raw);
        assert_eq!(events[0], ProviderEvent::TextDelta("Hello".into()));
        assert_eq!(
            events[1],
            ProviderEvent::ToolUseStart {
                id: "tu_1".into(),
                name: "read".into()
            }
        );
        match &events[2] {
            ProviderEvent::ToolUse { id, name, input } => {
                assert_eq!(id, "tu_1");
                assert_eq!(name, "read");
                assert_eq!(input["path"], "a.txt");
            }
            other => panic!("unexpected event: {other:?}"),
        }
        assert_eq!(state.usage.input_tokens, 42);
        assert_eq!(state.usage.output_tokens, 7);
        assert!(state.stopped);
    }

    #[test]
    fn redacted_debug_never_contains_key() {
        let provider = Anthropic::new(
            "sk-ant-secret-value".into(),
            crate::config::DEFAULT_BASE_URL,
        )
        .unwrap();
        let text = format!("{provider:?}");
        assert!(!text.contains("sk-ant"));
        assert!(text.contains("redacted"));
    }

    #[test]
    fn serializes_message_shape() {
        let messages = vec![
            Message::user_text("read it"),
            Message {
                role: Role::Assistant,
                content: vec![ContentBlock::ToolUse {
                    id: "tu_1".into(),
                    name: "read".into(),
                    input: json!({"path": "a.txt"}),
                }],
            },
            Message {
                role: Role::User,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "tu_1".into(),
                    content: "body".into(),
                    is_error: false,
                }],
            },
        ];
        let wire = to_wire_messages(&messages);
        assert_eq!(wire[0]["role"], "user");
        assert_eq!(wire[1]["content"][0]["type"], "tool_use");
        assert_eq!(wire[2]["content"][0]["type"], "tool_result");
    }
}
