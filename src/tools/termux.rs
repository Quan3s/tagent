use super::{schema_object, Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::{io::AsyncWriteExt, process::Command};

pub struct TermuxTool;

#[async_trait]
impl Tool for TermuxTool {
    fn name(&self) -> &str {
        "termux"
    }

    fn description(&self) -> &str {
        "Optional Termux:API action: notify, clipboard, open or battery."
    }

    fn input_schema(&self) -> Value {
        schema_object(
            json!({
                "action": {"type": "string", "enum": ["notify", "clipboard", "open", "battery"]},
                "title": {"type": "string"},
                "message": {"type": "string"},
                "text": {"type": "string"},
                "url": {"type": "string"}
            }),
            &["action"],
        )
    }

    async fn call(&self, args: &Value, _ctx: &ToolContext) -> Result<ToolOutput> {
        let action = args.get("action").and_then(Value::as_str).unwrap_or("");
        let bin = |name: &str| -> String {
            if let Some(prefix) = std::env::var_os("PREFIX") {
                let candidate = std::path::Path::new(&prefix).join("bin").join(name);
                if candidate.exists() {
                    return candidate.to_string_lossy().into_owned();
                }
            }
            format!("termux-{name}")
        };
        match action {
            "notify" => {
                let title = args.get("title").and_then(Value::as_str).unwrap_or("ta");
                let message = args.get("message").and_then(Value::as_str).unwrap_or("");
                run(
                    Command::new(bin("notification")).args(["-t", title, "-m", message]),
                    None,
                )
                .await
            }
            "clipboard" => {
                let text = args.get("text").and_then(Value::as_str).unwrap_or("");
                run(Command::new(bin("clipboard")).arg("set"), Some(text)).await
            }
            "open" => {
                let url = args.get("url").and_then(Value::as_str).unwrap_or("");
                if !url.starts_with("http://") && !url.starts_with("https://") {
                    return Ok(ToolOutput::err("termux: only http(s) URLs can be opened"));
                }
                run(Command::new(bin("open")).arg(url), None).await
            }
            "battery" => run(&mut Command::new(bin("battery")), None).await,
            other => Ok(ToolOutput::err(format!(
                "termux: unknown action '{other}' (use notify|clipboard|open|battery)"
            ))),
        }
    }
}

async fn run(cmd: &mut Command, stdin_text: Option<&str>) -> Result<ToolOutput> {
    cmd.kill_on_drop(true);
    if stdin_text.is_some() {
        cmd.stdin(std::process::Stdio::piped());
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return Ok(ToolOutput::err(format!(
                "termux: cannot start Termux:API command (is the app installed?): {e}"
            )))
        }
    };
    if let Some(text) = stdin_text {
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(text.as_bytes()).await?;
        }
    }
    match child.wait_with_output().await {
        Ok(output) => Ok(ToolOutput {
            content: String::from_utf8_lossy(&output.stdout).trim().to_string(),
            is_error: !output.status.success(),
        }),
        Err(e) => Ok(ToolOutput::err(format!("termux: command failed: {e}"))),
    }
}
