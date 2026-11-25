//! OpenAI-compatible Chat Completions adapter. Also used for OpenRouter and
//! Gemini's documented OpenAI compatibility endpoint.

use super::{
    ApiKey, CompletionRequest, ContentBlock, Message, Provider, ProviderEvent, Role, Usage,
};
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
use std::{collections::HashMap, fmt};

pub struct OpenAiCompat {
    client: Client,
    base_url: String,
    key: ApiKey,
}

impl fmt::Debug for OpenAiCompat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAiCompat")
            .field("base_url", &self.base_url)
            .field("key", &self.key)
            .finish()
    }
}

impl OpenAiCompat {
    pub fn new(api_key: String, base_url: &str) -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .build()
                .context("cannot build HTTP client")?,
            base_url: base_url.trim_end_matches('/').to_string(),
            key: ApiKey::new(api_key)?,
        })
    }
}

#[derive(Default)]
struct ToolAcc {
    id: String,
    name: String,
    arguments: String,
    emitted: bool,
}

#[async_trait]
impl Provider for OpenAiCompat {
    async fn complete(
        &self,
        req: CompletionRequest,
        emit: &mut (dyn FnMut(ProviderEvent) + Send),
    ) -> Result<()> {
        let url = format!("{}/chat/completions", self.base_url);
        let mut messages = vec![json!({"role": "system", "content": req.system})];
        messages.extend(to_wire_messages(&req.messages));
        let body = json!({
            "model": req.model,
            "max_tokens": req.max_tokens,
            "messages": messages,
            "tools": req.tools.iter().map(|t| json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.input_schema,
                }
            })).collect::<Vec<_>>(),
            "stream": true,
        });

        let response = self
            .client
            .post(&url)
            .header("authorization", format!("Bearer {}", self.key.0))
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("request to provider failed")?;
        if response.status() != StatusCode::OK {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            bail!(
                "provider returned HTTP {}: {}",
                status.as_u16(),
                truncate(&text, 1024)
            );
        }

        let mut usage = Usage::default();
        let mut tools: HashMap<u64, ToolAcc> = HashMap::new();
        let mut stop_reason: Option<String> = None;
        let mut buffer = String::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("failed while reading provider stream")?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));
            buffer = buffer.replace("\r\n", "\n");
            while let Some(pos) = buffer.find("\n\n") {
                let block: String = buffer.drain(..pos + 2).collect();
                handle_block(&block, &mut tools, &mut usage, &mut stop_reason, emit)?;
            }
        }
        if !buffer.trim().is_empty() {
            handle_block(&buffer, &mut tools, &mut usage, &mut stop_reason, emit)?;
        }
        emit_pending_tools(&mut tools, emit)?;
        emit(ProviderEvent::MessageStop {
            stop_reason: stop_reason.or_else(|| Some("end_turn".into())),
            usage,
        });
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
    let mut out = Vec::new();
    for message in messages {
        let role = match message.role {
            Role::User => "user",
            Role::Assistant => "assistant",
        };
        let mut text = String::new();
        let mut calls = Vec::new();
        let mut tool_results = Vec::new();
        for block in &message.content {
            match block {
                ContentBlock::Text { text: t } => text.push_str(t),
                ContentBlock::ToolUse { id, name, input } => calls.push(json!({
                    "id": id,
                    "type": "function",
                    "function": {"name": name, "arguments": serde_json::to_string(input).unwrap_or_else(|_| "{}".into())},
                })),
                ContentBlock::ToolResult { tool_use_id, content, is_error } => {
                    let content = if *is_error {
                        format!("[tool error] {content}")
                    } else {
                        content.clone()
                    };
                    tool_results.push(json!({
                        "role": "tool",
                        "tool_call_id": tool_use_id,
                        "content": content,
                    }));
                }
            }
        }
        if !calls.is_empty() || !text.is_empty() {
            let mut value = json!({"role": role, "content": if text.is_empty() { " ".to_string() } else { text }});
            if !calls.is_empty() {
                value["tool_calls"] = json!(calls);
            }
            out.push(value);
        }
        out.extend(tool_results);
    }
    out
}

fn handle_block(
    block: &str,
    tools: &mut HashMap<u64, ToolAcc>,
    usage: &mut Usage,
    stop_reason: &mut Option<String>,
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
    if let Some(u) = value.get("usage") {
        usage.input_tokens = u
            .get("prompt_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(usage.input_tokens);
        usage.output_tokens = u
            .get("completion_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(usage.output_tokens);
    }
    let Some(choice) = value.get("choices").and_then(|v| v.get(0)) else {
        return Ok(());
    };
    let delta = choice.get("delta").unwrap_or(&Value::Null);
    if let Some(text) = delta.get("content").and_then(Value::as_str) {
        if !text.is_empty() {
            emit(ProviderEvent::TextDelta(text.to_string()));
        }
    }
    if let Some(text) = delta
        .get("reasoning_content")
        .and_then(Value::as_str)
        .or_else(|| delta.get("reasoning").and_then(Value::as_str))
    {
        if !text.is_empty() {
            emit(ProviderEvent::ThinkingDelta(text.to_string()));
        }
    }
    if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
        for call in calls {
            let index = call.get("index").and_then(Value::as_u64).unwrap_or(0);
            let entry = tools.entry(index).or_default();
            if let Some(id) = call.get("id").and_then(Value::as_str) {
                if !id.is_empty() && entry.id.is_empty() {
                    entry.id = id.to_string();
                }
            }
            if let Some(function) = call.get("function") {
                if let Some(name) = function.get("name").and_then(Value::as_str) {
                    if !name.is_empty() && entry.name.is_empty() {
                        entry.name = name.to_string();
                        emit(ProviderEvent::ToolUseStart {
                            id: entry.id.clone(),
                            name: entry.name.clone(),
                        });
                    }
                }
                if let Some(part) = function.get("arguments").and_then(Value::as_str) {
                    entry.arguments.push_str(part);
                }
            }
        }
    }
    if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
        *stop_reason = Some(reason.to_string());
        if reason == "tool_calls" {
            emit_pending_tools(tools, emit)?;
        }
    }
    Ok(())
}

fn emit_pending_tools(
    tools: &mut HashMap<u64, ToolAcc>,
    emit: &mut dyn FnMut(ProviderEvent),
) -> Result<()> {
    let mut indices: Vec<u64> = tools.keys().copied().collect();
    indices.sort_unstable();
    for index in indices {
        if let Some(tool) = tools.get_mut(&index) {
            if tool.emitted || tool.name.is_empty() {
                continue;
            }
            let input = if tool.arguments.trim().is_empty() {
                json!({})
            } else {
                serde_json::from_str(&tool.arguments)
                    .context("provider streamed invalid tool arguments")?
            };
            tool.emitted = true;
            emit(ProviderEvent::ToolUse {
                id: tool.id.clone(),
                name: tool.name.clone(),
                input,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_assistant_tool_calls_and_results() {
        let messages = vec![
            Message::user_text("read it"),
            Message {
                role: Role::Assistant,
                content: vec![ContentBlock::ToolUse {
                    id: "call_1".into(),
                    name: "read".into(),
                    input: json!({"path": "a.txt"}),
                }],
            },
            Message {
                role: Role::User,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "call_1".into(),
                    content: "body".into(),
                    is_error: false,
                }],
            },
        ];
        let wire = to_wire_messages(&messages);
        assert_eq!(wire[0]["role"], "user");
        assert_eq!(wire[1]["tool_calls"][0]["id"], "call_1");
        assert_eq!(wire[2]["role"], "tool");
        assert_eq!(wire[2]["tool_call_id"], "call_1");
    }

    #[test]
    fn parses_streamed_tool_call_fragments() {
        let mut tools = HashMap::new();
        let mut usage = Usage::default();
        let mut reason = None;
        let mut events = Vec::new();
        let mut sink = |e| events.push(e);
        let first = json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_1",
                        "function": {"name": "read", "arguments": "{\"path\":\""}
                    }]
                }
            }]
        })
        .to_string();
        let second = json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "function": {"arguments": "a.txt\"}"}
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        })
        .to_string();
        handle_block(
            &format!("data: {first}"),
            &mut tools,
            &mut usage,
            &mut reason,
            &mut sink,
        )
        .unwrap();
        handle_block(
            &format!("data: {second}"),
            &mut tools,
            &mut usage,
            &mut reason,
            &mut sink,
        )
        .unwrap();
        assert!(events
            .iter()
            .any(|e| matches!(e, ProviderEvent::ToolUseStart { name, .. } if name == "read")));
        assert!(events.iter().any(
            |e| matches!(e, ProviderEvent::ToolUse { input, .. } if input["path"] == "a.txt")
        ));
    }
}
