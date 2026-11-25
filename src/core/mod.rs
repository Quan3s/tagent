//! Agent loop: context → stream → tool call → permission → execute → repeat.

use crate::{
    checkpoint, cost,
    events::{Event, Record},
    perm::{Decision, Engine},
    provider::{CompletionRequest, ContentBlock, DynProvider, Message, ProviderEvent, Usage},
    redact::{self, redact_event},
    session::{self, SessionStore},
    tools::{self, ToolContext, ToolOutput},
};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, oneshot};

pub const DEFAULT_MAX_TOKENS: u32 = 4096;
pub const DEFAULT_MAX_TURNS: usize = 8;
pub const DEFAULT_COMPACT_THRESHOLD: u64 = 24_000;

const DEFAULT_SYSTEM: &str = "You are ta, a coding agent running locally in Termux. \
Use the provided tools to inspect and change files. Be concise and precise. \
Never reveal API keys or other secrets, even if a tool output contains them.";

#[derive(Clone)]
struct SubagentContext {
    workspace: PathBuf,
    roots: Vec<PathBuf>,
    session_root: Option<PathBuf>,
    model: String,
    permission_mode: crate::perm::Mode,
    secrets: Vec<String>,
    secret_env_names: Vec<String>,
    pricing: HashMap<String, f64>,
    depth: u8,
}

impl From<&RunOptions> for SubagentContext {
    fn from(opts: &RunOptions) -> Self {
        Self {
            workspace: opts.workspace.clone(),
            roots: opts.roots.clone(),
            session_root: opts.session_root.clone(),
            model: opts.model.clone(),
            permission_mode: opts.permission.mode(),
            secrets: opts.secrets.clone(),
            secret_env_names: opts.secret_env_names.clone(),
            pricing: opts.pricing.clone(),
            depth: opts.depth,
        }
    }
}

#[derive(Default)]
struct TurnState {
    delta_text: String,
    calls: Vec<(String, String, Value)>,
    usage: Usage,
}

fn collect_provider_event(state: &mut TurnState, event: ProviderEvent) -> Vec<Event> {
    match event {
        ProviderEvent::TextDelta(text) => {
            state.delta_text.push_str(&text);
            vec![Event::MessageDelta {
                role: crate::events::Role::Assistant,
                text,
            }]
        }
        ProviderEvent::ThinkingDelta(text) => vec![Event::ThinkingDelta { text }],
        ProviderEvent::ToolUseStart { .. } => Vec::new(),
        ProviderEvent::ToolUse { id, name, input } => {
            state.calls.push((id.clone(), name.clone(), input.clone()));
            vec![Event::ToolStart {
                id,
                name,
                args: input,
            }]
        }
        ProviderEvent::MessageStop { usage, .. } => {
            state.usage = usage;
            Vec::new()
        }
    }
}

/// A reply from the Web UI to an interactive permission request.
#[derive(Clone, Debug)]
pub struct PermissionReply {
    pub id: String,
    pub allow: bool,
    pub always: bool,
    pub edited: Option<String>,
}

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
    /// Interactive permission replies. Headless runs leave this as `None`.
    pub permission_reply: Option<mpsc::Receiver<PermissionReply>>,
    /// Cancels the run when the client sends `interrupt` or closes the tab.
    pub interrupt: Option<oneshot::Receiver<()>>,
    /// Receives persisted records with their monotonically increasing ids.
    pub record_sink: Option<Box<dyn FnMut(Record) + Send>>,
    /// Per-model USD prices (per million tokens) used for cost events.
    pub pricing: HashMap<String, f64>,
    /// Compact the rebuilt context when it exceeds this many bytes.
    pub compact_threshold: Option<u64>,
    /// Snapshot files before edit tools so `/undo` can restore them.
    pub checkpoints: bool,
    /// Read-only instructions and skill metadata injected into the system prompt.
    pub project_instructions: Vec<String>,
    pub skill_prompt: Option<String>,
    /// Restrict the exposed tools (used by subagents and plugin policies).
    pub allowed_tools: Option<Vec<String>>,
    /// Hooks loaded from `.claude/hooks.json` and settings.
    pub hooks: Option<Arc<crate::ext::HookRunner>>,
    /// Dynamically registered tools (MCP, plugin scripts).
    pub extra_tools: Vec<Arc<dyn tools::Tool>>,
    /// Internal recursion guard for `task`.
    pub depth: u8,
}

#[derive(Clone, Debug)]
pub struct RunResult {
    pub session_id: String,
    pub final_text: String,
    pub usage: Usage,
    pub cost_usd: f64,
    pub turns: u32,
}

pub async fn run(mut opts: RunOptions) -> Result<RunResult> {
    let root = match opts.session_root.clone() {
        Some(root) => root,
        None => crate::config::sessions_dir()?,
    };
    let mut store = match &opts.session {
        Some(id) => SessionStore::load(root.clone(), id)?,
        None => SessionStore::create_in(root.clone())?,
    };
    std::fs::create_dir_all(&root)?;

    let records = store.records()?;
    let mut messages = session::messages(&records);
    if let Some(threshold) = opts.compact_threshold {
        messages = compact_messages(messages, threshold);
    }
    // Persist the user's prompt as the first event of this turn. This makes a
    // resumed session replayable purely from its JSONL file.
    emit(
        &mut store,
        &mut opts.sink,
        &mut None,
        &opts.secrets,
        Event::MessageDelta {
            role: crate::events::Role::User,
            text: opts.prompt.clone(),
        },
    )?;
    session::push_user_text(&mut messages, &opts.prompt);

    let secrets = opts.secrets.clone();
    if let Some(hooks) = &opts.hooks {
        use crate::ext::HookEvent;
        let outcome = hooks.run(
            HookEvent::UserPromptSubmit,
            json!({"prompt": opts.prompt, "session_id": store.id}),
        )?;
        if !outcome.allowed {
            bail!(
                "UserPromptSubmit hook blocked the prompt: {}",
                outcome.reason.unwrap_or_default()
            );
        }
    }
    let system = build_system(
        opts.system_prompt.as_deref(),
        opts.append_system_prompt.as_deref(),
        &opts.project_instructions,
        opts.skill_prompt.as_deref(),
    );
    let mut tools = tools::builtin();
    tools.extend(opts.extra_tools.iter().cloned());
    let tool_specs: Vec<_> = tools
        .iter()
        .filter(|t| {
            opts.allowed_tools
                .as_ref()
                .map(|names| names.iter().any(|n| n.eq_ignore_ascii_case(t.name())))
                .unwrap_or(true)
        })
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
        session_id: store.id.clone(),
        checkpoints: opts.checkpoints,
        checkpoint_root: checkpoint::checkpoint_dir()?,
        workspace: opts.workspace.clone(),
    };

    let mut total_usage = Usage::default();
    let mut final_text = String::new();
    let mut turns = 0u32;
    let mut reason = "end_turn".to_string();
    let mut permission_rx = opts.permission_reply.take();
    let mut interrupt_rx = opts.interrupt.take();
    let mut record_sink = opts.record_sink.take();
    let mut permission_counter = 0u64;

    for turn in 1..=opts.max_turns {
        turns = turn as u32;
        if interrupt_pending(&mut interrupt_rx) {
            reason = "interrupted".into();
            break;
        }
        let request = CompletionRequest {
            model: opts.model.clone(),
            system: system.clone(),
            messages: messages.clone(),
            tools: tool_specs.clone(),
            max_tokens: DEFAULT_MAX_TOKENS,
        };

        let mut turn_state = TurnState::default();
        let mut stream_error: Option<anyhow::Error> = None;
        let mut interrupted = false;
        let mut handler = |event: ProviderEvent| {
            for event in collect_provider_event(&mut turn_state, event) {
                if let Err(e) = emit(
                    &mut store,
                    &mut opts.sink,
                    &mut record_sink,
                    &secrets,
                    event,
                ) {
                    stream_error = Some(e);
                }
            }
        };

        let mut complete = opts.provider.complete(request, &mut handler);
        let stream_result = match interrupt_rx.as_mut() {
            Some(rx) => {
                tokio::select! {
                    result = &mut complete => result,
                    _ = rx => {
                        interrupted = true;
                        Ok(())
                    }
                }
            }
            None => (&mut complete).await,
        };
        drop(complete);
        drop(handler);
        stream_result.context("model call failed")?;
        if let Some(e) = stream_error {
            return Err(e);
        }
        if interrupted {
            reason = "interrupted".into();
            break;
        }

        let TurnState {
            delta_text,
            calls,
            usage,
        } = turn_state;
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
        for (id, name, mut args) in calls {
            if interrupt_pending(&mut interrupt_rx) {
                reason = "interrupted".into();
                break;
            }
            let mut target = tool_target(&name, &args);
            let mut decision = opts.permission.decide(&name, &target);

            if matches!(decision, Decision::Ask) && permission_rx.is_some() {
                permission_counter += 1;
                let perm_id = format!(
                    "perm-{}-{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos(),
                    permission_counter
                );
                emit(
                    &mut store,
                    &mut opts.sink,
                    &mut record_sink,
                    &secrets,
                    Event::PermissionRequest {
                        tool: name.clone(),
                        target: target.clone(),
                        decision: "ask".into(),
                        id: Some(perm_id.clone()),
                    },
                )?;
                let reply = wait_for_reply(permission_rx.as_mut(), &perm_id).await;
                match reply {
                    Some(reply) if reply.allow => {
                        if let Some(edited) = reply.edited.filter(|s| !s.trim().is_empty()) {
                            apply_edited(&mut args, &edited);
                            target = tool_target(&name, &args);
                            decision = opts.permission.decide(&name, &target);
                        } else {
                            decision = Decision::Allow;
                        }
                        if reply.always && matches!(decision, Decision::Allow) {
                            opts.permission.allow(&name, &target);
                        }
                    }
                    _ => decision = Decision::Deny("user denied".into()),
                }
            } else {
                emit(
                    &mut store,
                    &mut opts.sink,
                    &mut record_sink,
                    &secrets,
                    Event::PermissionRequest {
                        tool: name.clone(),
                        target: target.clone(),
                        decision: match &decision {
                            Decision::Allow => "allow".into(),
                            Decision::Ask => "ask".into(),
                            Decision::Deny(_) => "deny".into(),
                        },
                        id: None,
                    },
                )?;
            }

            let started = Instant::now();
            let before = if opts.checkpoints && is_edit_tool(&name) {
                checkpoint_files(&ctx, &args)
            } else {
                Vec::new()
            };
            let mut hook_block: Option<String> = None;
            if let Some(hooks) = &opts.hooks {
                use crate::ext::HookEvent;
                let outcome = hooks.run(
                    HookEvent::PreToolUse,
                    json!({"tool": name, "args": args, "session_id": ctx.session_id}),
                )?;
                if !outcome.allowed {
                    hook_block = Some(outcome.reason.unwrap_or_default());
                }
            }
            let mut output = if let Some(reason) = hook_block {
                ToolOutput::err(format!("PreToolUse hook blocked {name}: {reason}"))
            } else {
                match decision {
                Decision::Deny(reason) => ToolOutput::err(format!("permission denied: {reason}")),
                Decision::Ask => ToolOutput::err(
                    "permission required; rerun with --permission-mode auto-edit/yolo or --allowedTools (headless cannot prompt)",
                ),
                Decision::Allow => {
                    if name.eq_ignore_ascii_case("ask_user") {
                        let prompt = args
                            .get("prompt")
                            .and_then(Value::as_str)
                            .unwrap_or("Continue?");
                        permission_counter += 1;
                        let ask_id = format!("ask-{permission_counter}");
                        emit(
                            &mut store,
                            &mut opts.sink,
                            &mut record_sink,
                            &secrets,
                            Event::PermissionRequest {
                                tool: name.clone(),
                                target: prompt.to_string(),
                                decision: "ask".into(),
                                id: Some(ask_id.clone()),
                            },
                        )?;
                        match wait_for_reply(permission_rx.as_mut(), &ask_id).await {
                            Some(reply) => ToolOutput::ok(
                                reply.edited.filter(|s| !s.is_empty()).unwrap_or_else(|| {
                                    if reply.allow { "yes".into() } else { "no".into() }
                                }),
                            ),
                            None => ToolOutput::ok("no"),
                        }
                    } else if is_task_tool(&name) && opts.depth < 2 {
                        run_subagent(SubagentContext::from(&opts), &args, &mut store, &mut record_sink).await?
                    } else {
                        match tools.iter().find(|t| t.name().eq_ignore_ascii_case(&name)) {
                            Some(tool) => match tool.call(&args, &ctx).await {
                                Ok(out) => out,
                                Err(e) => ToolOutput::err(format!("{name} failed: {e}")),
                            },
                            None => ToolOutput::err(format!("unknown tool '{name}'")),
                        }
                    }
                }
            }
            };
            if let Some(hooks) = &opts.hooks {
                use crate::ext::HookEvent;
                let outcome = hooks.run(
                    HookEvent::PostToolUse,
                    json!({
                        "tool": name,
                        "args": args,
                        "output": output.content,
                        "is_error": output.is_error
                    }),
                )?;
                if !outcome.allowed && !output.is_error {
                    output = ToolOutput::err(format!(
                        "PostToolUse hook blocked {name}: {}",
                        outcome.reason.unwrap_or_default()
                    ));
                }
            }
            let elapsed_ms = started.elapsed().as_millis() as u64;

            if !before.is_empty() && !output.is_error && is_edit_tool(&name) {
                if let Some((path, diff_text)) = checkpoint::diff_latest(
                    &ctx.checkpoint_root,
                    &ctx.session_id,
                    &before,
                    &ctx.workspace,
                    &target,
                )
                .with_context(|| format!("cannot build diff for {target}"))
                .ok()
                .flatten()
                {
                    emit(
                        &mut store,
                        &mut opts.sink,
                        &mut record_sink,
                        &secrets,
                        Event::Diff {
                            path,
                            diff: diff_text,
                        },
                    )?;
                }
            }
            if name.eq_ignore_ascii_case("todo") && !output.is_error {
                if let Ok(items) = serde_json::from_str::<Value>(&output.content) {
                    if let Some(list) = items.get("todos").cloned() {
                        let items = match list {
                            Value::Array(values) => values,
                            other => vec![other],
                        };
                        emit(
                            &mut store,
                            &mut opts.sink,
                            &mut record_sink,
                            &secrets,
                            Event::TodoUpdate { items },
                        )?;
                    }
                }
            }

            let safe_output = redact::redact(&output.content, &secrets);
            emit(
                &mut store,
                &mut opts.sink,
                &mut record_sink,
                &secrets,
                Event::ToolResult {
                    id: id.clone(),
                    name: name.clone(),
                    output: output.content,
                    is_error: output.is_error,
                    elapsed_ms,
                },
            )?;
            results.push(ContentBlock::ToolResult {
                tool_use_id: id,
                content: safe_output,
                is_error: output.is_error,
            });
        }
        messages.push(Message {
            role: crate::provider::Role::User,
            content: results,
        });
        if reason == "interrupted" {
            break;
        }
    }

    if let Some(hooks) = &opts.hooks {
        use crate::ext::HookEvent;
        let _ = hooks.run(
            HookEvent::Stop,
            json!({"session_id": store.id, "reason": reason}),
        );
    }
    let session_id = store.id.clone();
    let cost_usd = cost::estimate(&opts.model, total_usage, &opts.pricing);
    emit(
        &mut store,
        &mut opts.sink,
        &mut record_sink,
        &secrets,
        Event::Usage {
            input_tokens: total_usage.input_tokens,
            output_tokens: total_usage.output_tokens,
            cost_usd: Some(cost_usd),
        },
    )?;
    emit(
        &mut store,
        &mut opts.sink,
        &mut record_sink,
        &secrets,
        Event::Done {
            reason: reason.clone(),
            session_id: session_id.clone(),
            turns,
        },
    )?;

    Ok(RunResult {
        session_id,
        final_text,
        usage: total_usage,
        cost_usd,
        turns,
    })
}

async fn wait_for_reply(
    rx: Option<&mut mpsc::Receiver<PermissionReply>>,
    id: &str,
) -> Option<PermissionReply> {
    let rx = rx?;
    loop {
        let reply = tokio::time::timeout(Duration::from_secs(600), rx.recv())
            .await
            .ok()??;
        if reply.id == id {
            return Some(reply);
        }
    }
}

fn interrupt_pending(rx: &mut Option<oneshot::Receiver<()>>) -> bool {
    if let Some(rx) = rx.as_mut() {
        matches!(
            rx.try_recv(),
            Ok(_) | Err(oneshot::error::TryRecvError::Closed)
        )
    } else {
        false
    }
}

fn tool_target(name: &str, args: &Value) -> String {
    let target = [
        "path", "command", "pattern", "query", "url", "prompt", "agent",
    ]
    .into_iter()
    .find_map(|key| args.get(key).and_then(Value::as_str))
    .unwrap_or("");
    format!("{name}: {target}").chars().take(512).collect()
}

fn apply_edited(args: &mut Value, edited: &str) {
    for key in ["command", "content", "path", "prompt"] {
        if args.get(key).is_some() {
            args[key] = json!(edited);
            return;
        }
    }
    args["command"] = json!(edited);
}

fn is_edit_tool(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "write" | "edit" | "multi_edit" | "notebook_edit"
    )
}

fn is_task_tool(name: &str) -> bool {
    name.eq_ignore_ascii_case("task") || name.eq_ignore_ascii_case("agent")
}

fn checkpoint_files(ctx: &ToolContext, args: &Value) -> Vec<PathBuf> {
    let mut raw: Vec<String> = Vec::new();
    if let Some(path) = args.get("path").and_then(Value::as_str) {
        raw.push(path.to_string());
    }
    if let Some(items) = args.get("edits").and_then(Value::as_array) {
        for item in items {
            if let Some(path) = item.get("path").and_then(Value::as_str) {
                raw.push(path.to_string());
            }
        }
    }
    raw.into_iter()
        .filter_map(|p| tools::resolve_path(ctx, &p).ok())
        .collect()
}

async fn run_subagent(
    parent: SubagentContext,
    args: &Value,
    _store: &mut SessionStore,
    _record_sink: &mut Option<Box<dyn FnMut(Record) + Send>>,
) -> Result<ToolOutput> {
    let agent = args
        .get("agent")
        .and_then(Value::as_str)
        .unwrap_or("default");
    let prompt = args
        .get("prompt")
        .and_then(Value::as_str)
        .context("task: missing string argument 'prompt'")?;
    let tools_allowed = args
        .get("tools")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_else(|| vec!["read".into(), "glob".into(), "grep".into(), "ls".into()]);
    let (_, _, provider_config) = crate::config::Config::load()?
        .resolve("subagent")
        .or_else(|_| crate::config::Config::load()?.resolve(&parent.model))?;
    let provider =
        crate::provider::build(&provider_config.type_name().to_string(), &provider_config)?;
    let events = Arc::new(std::sync::Mutex::new(Vec::<Event>::new()));
    let collected = events.clone();
    let sink: Box<dyn FnMut(Event) + Send> = Box::new(move |event| {
        if let Ok(mut list) = collected.lock() {
            list.push(event);
        }
    });
    let result = tokio::time::timeout(
        Duration::from_secs(180),
        Box::pin(run(RunOptions {
            workspace: parent.workspace.clone(),
            roots: parent.roots.clone(),
            prompt: format!("[{agent}] {prompt}"),
            model: "subagent".into(),
            provider,
            permission: Engine::with_mode(parent.permission_mode),
            max_turns: 4,
            system_prompt: Some(format!(
                "You are the '{agent}' subagent. Work only with the tools provided to you and report a concise result."
            )),
            append_system_prompt: None,
            session: None,
            session_root: parent.session_root.clone(),
            sink,
            secrets: parent.secrets.clone(),
            secret_env_names: parent.secret_env_names.clone(),
            permission_reply: None,
            interrupt: None,
            record_sink: None,
            pricing: parent.pricing.clone(),
            compact_threshold: Some(DEFAULT_COMPACT_THRESHOLD),
            checkpoints: false,
            project_instructions: Vec::new(),
            skill_prompt: None,
            allowed_tools: Some(tools_allowed),
            hooks: None,
            extra_tools: Vec::new(),
            depth: parent.depth + 1,
        })),
    )
    .await
    .context("subagent timed out")?
    .context("subagent failed")?;
    let text = events
        .lock()
        .map_err(|e| anyhow::anyhow!("subagent event lock poisoned: {e}"))?
        .iter()
        .filter_map(|e| match e {
            Event::MessageDelta {
                role: crate::events::Role::Assistant,
                text,
            } => Some(text.as_str()),
            _ => None,
        })
        .collect::<String>();
    let _ = result;
    Ok(ToolOutput::ok(redact::redact(&text, &parent.secrets)))
}

fn compact_messages(messages: Vec<Message>, threshold: u64) -> Vec<Message> {
    let approx: u64 = messages
        .iter()
        .map(|m| {
            serde_json::to_string(m)
                .map(|s| s.len() as u64)
                .unwrap_or(0)
        })
        .sum();
    if approx <= threshold || messages.len() <= 8 {
        return messages;
    }
    let keep = 6usize.max(messages.len() / 3);
    let start = messages.len() - keep;
    let mut summary = String::from("Summary of earlier conversation (context compacted):\n");
    for message in &messages[..start] {
        for block in &message.content {
            match block {
                ContentBlock::Text { text } => {
                    summary.push_str(&text.chars().take(300).collect::<String>());
                    summary.push('\n');
                }
                ContentBlock::ToolUse { name, input, .. } => {
                    summary.push_str(&format!(
                        "called {name} with {}\n",
                        serde_json::to_string(input).unwrap_or_default()
                    ));
                }
                ContentBlock::ToolResult {
                    content, is_error, ..
                } => {
                    summary.push_str(if *is_error { "error: " } else { "result: " });
                    summary.push_str(&content.chars().take(300).collect::<String>());
                    summary.push('\n');
                }
            }
        }
    }
    summary = summary.chars().take(8000).collect();
    let mut out = vec![Message::user_text(summary)];
    out.extend(messages[start..].iter().cloned());
    out
}

fn emit(
    store: &mut SessionStore,
    sink: &mut dyn FnMut(Event),
    record_sink: &mut Option<Box<dyn FnMut(Record) + Send>>,
    secrets: &[String],
    event: Event,
) -> Result<()> {
    let event = redact_event(event, secrets);
    let record = store.append(event.clone())?;
    if let Some(rs) = record_sink.as_mut() {
        rs(record);
    }
    sink(event);
    Ok(())
}

fn build_system(
    system_prompt: Option<&str>,
    append_system_prompt: Option<&str>,
    project_instructions: &[String],
    skill_prompt: Option<&str>,
) -> String {
    let mut system = system_prompt.unwrap_or(DEFAULT_SYSTEM).to_string();
    if let Some(extra) = append_system_prompt {
        system.push_str("\n\n");
        system.push_str(extra);
    }
    for text in project_instructions {
        system.push_str("\n\n# Project instructions\n");
        let truncated: String = text.chars().take(16_000).collect();
        system.push_str(&truncated);
    }
    if let Some(skills) = skill_prompt {
        system.push_str("\n\n# Available skills\n");
        system.push_str(skills);
    }
    system
}

/// Small helper used by the CLI to expose a provider as `DynProvider`.
pub fn shared<P: crate::provider::Provider + 'static>(provider: P) -> DynProvider {
    Arc::new(provider)
}
