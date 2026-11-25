//! Agent loop: context → stream → tool call → permission → execute → repeat.

use crate::{
    events::Event,
    perm::{Decision, Engine},
    provider::{CompletionRequest, ContentBlock, DynProvider, Message, ProviderEvent, Usage},
    redact::redact_event,
    session::{self, SessionStore},
    tools::{self, ToolContext, ToolOutput},
};
use anyhow::{Context, Result};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

pub const DEFAULT_MAX_TOKENS: u32 = 4096;
pub const DEFAULT_MAX_TURNS: usize = 8;

const DEFAULT_SYSTEM: &str = "You are ta, a coding agent running locally in Termux. \
Use the provided tools to inspect and change files. Be concise and precise. \
Never reveal API keys or other secrets, even if a tool output contains them.";

pub struct RunOptions {
    pub workspace: PathBuf,
    pub roots: Vec<PathBuf>,
    pub prompt: String,
    pub model: String,
    pub provider: DynProvider,
    pub permission: Engine,
    pub max_turns: usize,
    pub system_prompt: Option<String>,
    pub append_system_prompt: Option<String>,
    /// Existing session id to resume; `None` starts a new session.
    pub session: Option<String>,
    pub session_root: Option<PathBuf>,
    pub sink: Box<dyn FnMut(Event) + Send>,
    pub secrets: Vec<String>,
    pub secret_env_names: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct RunResult {
    pub session_id: String,
    pub final_text: String,
    pub usage: Usage,
    pub turns: u32,
}

pub async fn run(mut opts: RunOptions) -> Result<RunResult> {
    let root = match opts.session_root.clone() {
        Some(root) => root,
        None => crate::config::sessions_dir()?,
    };
    let mut store = match &opts.session {
        Some(id) => SessionStore::load(root, id)?,
        None => SessionStore::create_in(root)?,
    };

    let records = store.records()?;
    let mut messages = session::messages(&records);
    session::push_user_text(&mut messages, &opts.prompt);

    let system = build_system(&opts).await;
    let tools = tools::builtin();
    let tool_specs: Vec<_> = tools
        .iter()
        .map(|t| crate::provider::ToolSpec {
            name: t.name().to_string(),
            description: t.description().to_string(),
            input_schema: t.input_schema(),
        })
        .collect();
    let ctx = ToolContext {
        roots: opts.roots.clone(),
        bash_timeout: Duration::from_secs(30),
        secret_env_names: opts.secret_env_names.clone(),
    };

    let mut total_usage = Usage::default();
    let mut final_text = String::new();
    let mut turns = 0u32;

    for turn in 1..=opts.max_turns {
        turns = turn as u32;
        let request = CompletionRequest {
            model: opts.model.clone(),
            system: system.clone(),
            messages: messages.clone(),
            tools: tool_specs.clone(),
            max_tokens: DEFAULT_MAX_TOKENS,
        };

        let mut delta_text = String::new();
        let mut calls: Vec<(String, String, serde_json::Value)> = Vec::new();
        let mut usage = Usage::default();
        let mut stream_error: Option<anyhow::Error> = None;
        let mut push = |event: Event| {
            if let Err(e) = emit(&mut store, &mut opts.sink, &opts.secrets, event) {
                stream_error = Some(e);
            }
        };

        opts.provider
            .complete(request, &mut |event| match event {
                ProviderEvent::TextDelta(text) => {
                    delta_text.push_str(&text);
                    push(Event::MessageDelta {
                        role: crate::events::Role::Assistant,
                        text,
                    });
                }
                ProviderEvent::ToolUseStart { .. } => {}
                ProviderEvent::ToolUse { id, name, input } => {
                    calls.push((id.clone(), name.clone(), input.clone()));
                    push(Event::ToolStart {
                        id,
                        name,
                        args: input,
                    });
                }
                ProviderEvent::MessageStop {
                    stop_reason: _,
                    usage: u,
                } => {
                    usage = u;
                }
            })
            .await
            .context("model call failed")?;
        if let Some(e) = stream_error {
            return Err(e);
        }

        total_usage.input_tokens += usage.input_tokens;
        total_usage.output_tokens += usage.output_tokens;
        if !delta_text.is_empty() {
            final_text = delta_text.clone();
        }

        let mut assistant_content: Vec<ContentBlock> = Vec::new();
        if !delta_text.is_empty() {
            assistant_content.push(ContentBlock::Text { text: delta_text });
        }
        for (id, name, input) in &calls {
            assistant_content.push(ContentBlock::ToolUse {
                id: id.clone(),
                name: name.clone(),
                input: input.clone(),
            });
        }
        if !assistant_content.is_empty() {
            messages.push(Message {
                role: crate::provider::Role::Assistant,
                content: assistant_content,
            });
        }

        if calls.is_empty() {
            break;
        }

        let mut results: Vec<ContentBlock> = Vec::new();
        for (id, name, args) in calls {
            let target = args
                .get("path")
                .or_else(|| args.get("command"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let decision = opts.permission.decide(&name, &target);
            emit(
                &mut store,
                &mut opts.sink,
                &opts.secrets,
                Event::PermissionRequest {
                    tool: name.clone(),
                    target,
                    decision: match &decision {
                        Decision::Allow => "allow".into(),
                        Decision::Ask => "ask".into(),
                        Decision::Deny(_) => "deny".into(),
                    },
                },
            )?;

            let started = Instant::now();
            let output = match decision {
                Decision::Deny(reason) => ToolOutput::err(format!("permission denied: {reason}")),
                Decision::Ask => ToolOutput::err(
                    "permission required; rerun with --permission-mode auto-edit/yolo or --allowedTools (headless cannot prompt)",
                ),
                Decision::Allow => match tools.iter().find(|t| t.name() == name) {
                    Some(tool) => match tool.call(&args, &ctx).await {
                        Ok(out) => out,
                        Err(e) => ToolOutput::err(format!("{name} failed: {e}")),
                    },
                    None => ToolOutput::err(format!("unknown tool '{name}'")),
                },
            };
            let elapsed_ms = started.elapsed().as_millis() as u64;
            emit(
                &mut store,
                &mut opts.sink,
                &opts.secrets,
                Event::ToolResult {
                    id: id.clone(),
                    name: name.clone(),
                    output: output.content.clone(),
                    is_error: output.is_error,
                    elapsed_ms,
                },
            )?;
            results.push(ContentBlock::ToolResult {
                tool_use_id: id,
                content: output.content,
                is_error: output.is_error,
            });
        }
        messages.push(Message {
            role: crate::provider::Role::User,
            content: results,
        });
    }

    let session_id = store.id.clone();
    emit(
        &mut store,
        &mut opts.sink,
        &opts.secrets,
        Event::Usage {
            input_tokens: total_usage.input_tokens,
            output_tokens: total_usage.output_tokens,
        },
    )?;
    let reason = if turns as usize >= opts.max_turns {
        "max_turns"
    } else {
        "end_turn"
    };
    emit(
        &mut store,
        &mut opts.sink,
        &opts.secrets,
        Event::Done {
            reason: reason.into(),
            session_id: session_id.clone(),
            turns,
        },
    )?;

    Ok(RunResult {
        session_id,
        final_text,
        usage: total_usage,
        turns,
    })
}

fn emit(
    store: &mut SessionStore,
    sink: &mut dyn FnMut(Event),
    secrets: &[String],
    event: Event,
) -> Result<()> {
    let event = redact_event(event, secrets);
    store.append(event.clone())?;
    sink(event);
    Ok(())
}

async fn build_system(opts: &RunOptions) -> String {
    let mut system = opts
        .system_prompt
        .clone()
        .unwrap_or_else(|| DEFAULT_SYSTEM.to_string());
    if let Some(extra) = &opts.append_system_prompt {
        system.push_str("\n\n");
        system.push_str(extra);
    }
    // Project memory is part of the context, not a plugin loader (M4).
    for name in ["AGENTS.md", "CLAUDE.md"] {
        if let Ok(text) = tokio::fs::read_to_string(opts.workspace.join(name)).await {
            system.push_str("\n\n# ");
            system.push_str(name);
            system.push_str("\n");
            let truncated: String = text.chars().take(32_000).collect();
            system.push_str(&truncated);
        }
    }
    system
}

/// Small helper used by the CLI to expose a provider as `DynProvider`.
pub fn shared<P: crate::provider::Provider + 'static>(provider: P) -> DynProvider {
    Arc::new(provider)
}
