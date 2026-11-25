use super::{resolve_path, schema_object, truncate_output, Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::fs;

pub struct ReadTool;

#[async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        "Read a UTF-8 text file inside the workspace. Output is capped at 256 KiB."
    }

    fn input_schema(&self) -> Value {
        schema_object(
            json!({"path": {"type": "string", "description": "Relative file path"}}),
            &["path"],
        )
    }

    async fn call(&self, args: &Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let Some(path) = args.get("path").and_then(Value::as_str) else {
            return Ok(ToolOutput::err("read: missing string argument 'path'"));
        };
        let resolved = match resolve_path(ctx, path) {
            Ok(p) => p,
            Err(e) => return Ok(ToolOutput::err(format!("read: {e}"))),
        };
        match fs::read(&resolved).await {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes);
                Ok(ToolOutput::ok(truncate_output(&text, 256 * 1024)))
            }
            Err(e) => Ok(ToolOutput::err(format!("read: cannot read {path}: {e}"))),
        }
    }
}
