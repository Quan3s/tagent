use super::{resolve_path, Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::fs;

pub struct EditTool;

#[async_trait]
impl Tool for EditTool {
    fn name(&self) -> &str {
        "edit"
    }

    fn description(&self) -> &str {
        "Replace an exact string in a file. Fails if the old string is missing or ambiguous."
    }

    fn input_schema(&self) -> Value {
        super::schema_object(
            json!({
                "path": {"type": "string", "description": "Relative file path"},
                "old_string": {"type": "string"},
                "new_string": {"type": "string"},
                "replace_all": {"type": "boolean", "default": false}
            }),
            &["path", "old_string", "new_string"],
        )
    }

    async fn call(&self, args: &Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let Some(path) = args.get("path").and_then(Value::as_str) else {
            return Ok(ToolOutput::err("edit: missing 'path'"));
        };
        let Some(old) = args.get("old_string").and_then(Value::as_str) else {
            return Ok(ToolOutput::err("edit: missing 'old_string'"));
        };
        let new = args.get("new_string").and_then(Value::as_str).unwrap_or("");
        if old.is_empty() {
            return Ok(ToolOutput::err("edit: 'old_string' must not be empty"));
        }
        let resolved = match resolve_path(ctx, path) {
            Ok(p) => p,
            Err(e) => return Ok(ToolOutput::err(format!("edit: {e}"))),
        };
        let text = match fs::read_to_string(&resolved).await {
            Ok(t) => t,
            Err(e) => return Ok(ToolOutput::err(format!("edit: cannot read {path}: {e}"))),
        };
        let count = text.matches(old).count();
        if count == 0 {
            return Ok(ToolOutput::err(format!(
                "edit: old string not found in {path}"
            )));
        }
        let replace_all = args
            .get("replace_all")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if count > 1 && !replace_all {
            return Ok(ToolOutput::err(format!(
                "edit: old string appears {count} times in {path}; set replace_all=true"
            )));
        }
        let updated = if replace_all {
            text.replace(old, new)
        } else {
            text.replacen(old, new, 1)
        };
        match fs::write(&resolved, updated).await {
            Ok(()) => Ok(ToolOutput::ok(format!(
                " edited {path}: replaced {count} occurrence(s)"
            ))),
            Err(e) => Ok(ToolOutput::err(format!("edit: cannot write {path}: {e}"))),
        }
    }
}
