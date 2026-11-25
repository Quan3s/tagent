use super::{schema_object, truncate_output, Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::{process::Command, time::timeout};

pub struct BashTool;

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &'static str {
        "bash"
    }

    fn description(&self) -> &'static str {
        "Run a shell command with the workspace as cwd. Destructive commands are blocked."
    }

    fn input_schema(&self) -> Value {
        schema_object(
            json!({
                "command": {"type": "string", "description": "Shell command"},
                "timeout_ms": {"type": "integer", "description": "Optional timeout in milliseconds (max 120000)"}
            }),
            &["command"],
        )
    }

    async fn call(&self, args: &Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let Some(command) = args.get("command").and_then(Value::as_str) else {
            return Ok(ToolOutput::err("bash: missing string argument 'command'"));
        };
        let limit = args
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .map(|ms| Duration::from_millis(ms.min(120_000)))
            .unwrap_or(ctx.bash_timeout);

        let mut cmd = Command::new(shell());
        cmd.arg("-c")
            .arg(command)
            .kill_on_drop(true)
            .current_dir(ctx.roots.first().cloned().unwrap_or_default());
        for name in &ctx.secret_env_names {
            cmd.env_remove(name);
        }

        match timeout(limit, cmd.output()).await {
            Err(_) => Ok(ToolOutput::err(format!(
                "bash: timed out after {} ms",
                limit.as_millis()
            ))),
            Ok(Err(e)) => Ok(ToolOutput::err(format!("bash: cannot start shell: {e}"))),
            Ok(Ok(output)) => {
                let mut text = String::new();
                if !output.stdout.is_empty() {
                    text.push_str(&String::from_utf8_lossy(&output.stdout));
                }
                if !output.stderr.is_empty() {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str("[stderr]\n");
                    text.push_str(&String::from_utf8_lossy(&output.stderr));
                }
                if text.is_empty() {
                    text.push_str(&format!(
                        "(no output, exit code {})",
                        output.status.code().unwrap_or(-1)
                    ));
                }
                Ok(ToolOutput {
                    content: truncate_output(&text, 64 * 1024),
                    is_error: !output.status.success(),
                })
            }
        }
    }
}

/// Termux has no `/bin/sh`; use `$PREFIX/bin/sh` when available.
fn shell() -> String {
    if let Some(prefix) = std::env::var_os("PREFIX") {
        let candidate = std::path::Path::new(&prefix).join("bin/sh");
        if candidate.exists() {
            return candidate.to_string_lossy().into_owned();
        }
    }
    if std::path::Path::new("/bin/sh").exists() {
        return "/bin/sh".to_string();
    }
    "sh".to_string()
}
