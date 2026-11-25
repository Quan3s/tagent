use super::{resolve_path, schema_object, Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::fs;

pub struct WriteTool;

#[async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &str {
        "write"
    }

    fn description(&self) -> &str {
        "Create or overwrite a UTF-8 text file inside the workspace."
    }

    fn input_schema(&self) -> Value {
        schema_object(
            json!({
                "path": {"type": "string", "description": "Relative file path"},
                "content": {"type": "string", "description": "Full file contents"}
            }),
            &["path", "content"],
        )
    }

    async fn call(&self, args: &Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let Some(path) = args.get("path").and_then(Value::as_str) else {
            return Ok(ToolOutput::err("write: missing string argument 'path'"));
        };
        let Some(content) = args.get("content").and_then(Value::as_str) else {
            return Ok(ToolOutput::err("write: missing string argument 'content'"));
        };
        let resolved = match resolve_path(ctx, path) {
            Ok(p) => p,
            Err(e) => return Ok(ToolOutput::err(format!("write: {e}"))),
        };
        if let Some(parent) = resolved.parent() {
            if let Err(e) = fs::create_dir_all(parent).await {
                return Ok(ToolOutput::err(format!("write: cannot create parent: {e}")));
            }
        }
        match fs::write(&resolved, content).await {
            Ok(()) => Ok(ToolOutput::ok(format!(
                "wrote {} bytes to {path}",
                content.len()
            ))),
            Err(e) => Ok(ToolOutput::err(format!("write: cannot write {path}: {e}"))),
        }
    }
}
