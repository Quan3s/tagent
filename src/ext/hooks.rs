//! Hook parsing and execution. Hooks receive JSON on stdin; exit 0 allows,
//! exit 2 blocks with an optional JSON/plain-text reason.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command as ProcessCommand, Stdio},
};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub enum HookEvent {
    #[serde(rename = "PreToolUse", alias = "pre_tool_use")]
    PreToolUse,
    #[serde(rename = "PostToolUse", alias = "post_tool_use")]
    PostToolUse,
    #[serde(rename = "UserPromptSubmit", alias = "user_prompt_submit")]
    UserPromptSubmit,
    #[serde(rename = "SessionStart", alias = "session_start")]
    SessionStart,
    #[serde(rename = "Stop")]
    Stop,
    #[serde(rename = "SubagentStop", alias = "subagent_stop")]
    SubagentStop,
}

impl HookEvent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PreToolUse => "PreToolUse",
            Self::PostToolUse => "PostToolUse",
            Self::UserPromptSubmit => "UserPromptSubmit",
            Self::SessionStart => "SessionStart",
            Self::Stop => "Stop",
            Self::SubagentStop => "SubagentStop",
        }
    }

    /// Accept the exact Claude Code names plus snake/kebab case variants.
    pub fn parse(input: &str) -> Option<Self> {
        let normalized: String = input
            .chars()
            .filter(|c| *c != '_' && *c != '-' && !c.is_whitespace())
            .collect();
        let lowered = normalized.to_ascii_lowercase();
        [
            Self::PreToolUse,
            Self::PostToolUse,
            Self::UserPromptSubmit,
            Self::SessionStart,
            Self::Stop,
            Self::SubagentStop,
        ]
        .into_iter()
        .find(|event| event.as_str().to_ascii_lowercase() == lowered)
    }
}

impl std::str::FromStr for HookEvent {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        Self::parse(s).ok_or_else(|| anyhow::anyhow!("unknown hook event '{}'", s))
    }
}

impl std::fmt::Display for HookEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug)]
pub struct Hook {
    pub event: HookEvent,
    pub command: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug)]
pub struct HookOutcome {
    pub allowed: bool,
    pub reason: Option<String>,
    /// Parsed JSON from stdout, when stdout is valid JSON.
    pub json: Option<Value>,
    pub stdout: String,
    pub stderr: String,
}

impl HookOutcome {
    /// Interpret a hook process result without running anything.
    pub fn from_exit(exit_code: i32, stdout: &str, stderr: &str) -> Result<Self> {
        let json = serde_json::from_str::<Value>(stdout.trim()).ok();
        match exit_code {
            0 => {
                let decision = json
                    .as_ref()
                    .and_then(|v| v.get("decision").or_else(|| v.get("status")))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                let blocked = matches!(decision.as_str(), "block" | "deny" | "denied");
                Ok(Self {
                    allowed: !blocked,
                    reason: if blocked {
                        json_reason(json.as_ref()).or_else(|| non_empty(stdout))
                    } else {
                        None
                    },
                    json,
                    stdout: stdout.to_string(),
                    stderr: stderr.to_string(),
                })
            }
            2 => Ok(Self {
                allowed: false,
                reason: json_reason(json.as_ref())
                    .or_else(|| non_empty(stdout))
                    .or_else(|| Some("blocked by hook".to_string())),
                json,
                stdout: stdout.to_string(),
                stderr: stderr.to_string(),
            }),
            other => bail!("hook exited with status {}", other),
        }
    }
}

fn json_reason(value: Option<&Value>) -> Option<String> {
    let value = value?;
    for key in ["reason", "error", "message", "feedback"] {
        if let Some(reason) = value.get(key).and_then(Value::as_str) {
            if !reason.trim().is_empty() {
                return Some(reason.trim().to_string());
            }
        }
    }
    None
}

fn non_empty(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[derive(Clone, Debug, Default)]
pub struct HookRunner {
    pub hooks: Vec<Hook>,
}

impl HookRunner {
    pub fn new(hooks: Vec<Hook>) -> Self {
        Self { hooks }
    }

    /// Run all hooks matching `event`, stopping at the first block.
    pub fn run(&self, event: HookEvent, mut payload: Value) -> Result<HookOutcome> {
        if let Some(obj) = payload.as_object_mut() {
            obj.entry("hook_event_name")
                .or_insert_with(|| Value::String(event.as_str().to_string()));
        }
        let mut last = HookOutcome {
            allowed: true,
            reason: None,
            json: None,
            stdout: String::new(),
            stderr: String::new(),
        };
        for hook in self.hooks.iter().filter(|h| h.event == event) {
            let output = ProcessCommand::new("sh")
                .arg("-c")
                .arg(&hook.command)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .with_context(|| format!("failed to spawn hook: {}", hook.command))?;
            let mut child = output;
            {
                let mut stdin = child.stdin.take().context("hook stdin was not piped")?;
                serde_json::to_writer(&mut stdin, &payload)?;
                stdin.write_all(b"\n")?;
            }
            let output = child
                .wait_with_output()
                .with_context(|| format!("failed to wait for hook: {}", hook.command))?;
            let code = output
                .status
                .code()
                .context("hook terminated without an exit status")?;
            let outcome = HookOutcome::from_exit(
                code,
                &String::from_utf8_lossy(&output.stdout),
                &String::from_utf8_lossy(&output.stderr),
            )?;
            if !outcome.allowed {
                return Ok(outcome);
            }
            last = outcome;
        }
        Ok(last)
    }
}

/// Parse `hooks/hooks.json`. Supports both Claude Code's event-keyed object
/// and a simple array of `{ event, command, description }` entries.
pub fn parse_hooks(value: &Value) -> Vec<Hook> {
    let mut out = Vec::new();
    match value {
        Value::Array(items) => {
            for item in items {
                out.extend(parse_entry(None, item));
            }
        }
        Value::Object(obj) => {
            if let Some(entries) = obj.get("hooks") {
                out.extend(parse_hooks(entries));
            }
            for (key, entry) in obj {
                if key.eq_ignore_ascii_case("hooks") {
                    continue;
                }
                if let Some(event) = HookEvent::parse(key) {
                    out.extend(parse_entry(Some(event), entry));
                }
            }
        }
        _ => {}
    }
    out
}

fn parse_entry(event: Option<HookEvent>, value: &Value) -> Vec<Hook> {
    let mut out = Vec::new();
    match value {
        Value::String(command) => {
            if let Some(event) = event {
                out.push(Hook {
                    event,
                    command: command.clone(),
                    description: None,
                });
            }
        }
        Value::Array(items) => {
            for item in items {
                out.extend(parse_entry(event, item));
            }
        }
        Value::Object(obj) => {
            let event = event.or_else(|| {
                obj.get("event")
                    .or_else(|| obj.get("name"))
                    .and_then(Value::as_str)
                    .and_then(HookEvent::parse)
            });
            if let Some(command) = obj.get("command").and_then(Value::as_str) {
                if let Some(event) = event {
                    out.push(Hook {
                        event,
                        command: command.to_string(),
                        description: obj
                            .get("description")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                    });
                }
            }
            if let Some(nested) = obj.get("hooks") {
                out.extend(parse_entry(event, nested));
            }
        }
        _ => {}
    }
    out
}

/// Load a hooks file if it exists. Missing or malformed files are tolerated.
pub fn load_hooks(path: &Path) -> Result<Vec<Hook>> {
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read hooks {}", path.display()))?;
    let value: Value = serde_json::from_str(&text)
        .with_context(|| format!("invalid hooks file {}", path.display()))?;
    Ok(parse_hooks(&value))
}

/// Extract hooks from a Claude Code `settings.json` (`hooks` key).
pub fn hooks_from_settings(path: &Path) -> Result<Vec<Hook>> {
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read settings {}", path.display()))?;
    let value: Value = serde_json::from_str(&text)
        .with_context(|| format!("invalid settings file {}", path.display()))?;
    Ok(value.get("hooks").map(parse_hooks).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hook_outcomes() {
        let allowed = HookOutcome::from_exit(0, "{\"continue\":true}", "").unwrap();
        assert!(allowed.allowed);
        assert!(allowed.reason.is_none());

        let blocked =
            HookOutcome::from_exit(2, "{\"decision\":\"block\",\"reason\":\"nope\"}", "").unwrap();
        assert!(!blocked.allowed);
        assert_eq!(blocked.reason.as_deref(), Some("nope"));

        let plain = HookOutcome::from_exit(2, "not allowed", "").unwrap();
        assert_eq!(plain.reason.as_deref(), Some("not allowed"));

        assert!(HookOutcome::from_exit(1, "", "boom").is_err());
    }

    #[test]
    fn parses_both_hook_shapes() {
        let value = serde_json::json!({
            "PreToolUse": [{ "hooks": [{ "command": "cat", "description": "check" }] }],
            "hooks": [{ "event": "Stop", "command": "printf bye" }]
        });
        let hooks = parse_hooks(&value);
        assert_eq!(hooks.len(), 2);
        assert!(hooks
            .iter()
            .any(|h| h.event == HookEvent::PreToolUse && h.command == "cat"));
        assert!(hooks.iter().any(|h| h.event == HookEvent::Stop));
    }

    #[test]
    fn runs_a_builtin_shell_hook() {
        let hook = Hook {
            event: HookEvent::UserPromptSubmit,
            command: "cat".to_string(),
            description: None,
        };
        let outcome = HookRunner::new(vec![hook])
            .run(
                HookEvent::UserPromptSubmit,
                serde_json::json!({"prompt": "hello"}),
            )
            .unwrap();
        assert!(outcome.allowed);
        let value: Value = serde_json::from_str(outcome.stdout.trim()).unwrap();
        assert_eq!(value["prompt"], "hello");
        assert_eq!(value["hook_event_name"], "UserPromptSubmit");
    }
}
