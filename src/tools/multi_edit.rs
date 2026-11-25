use super::{resolve_path, Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::fs;

pub struct MultiEditTool;

#[async_trait]
impl Tool for MultiEditTool {
    fn name(&self) -> &str {
        "multi_edit"
    }

    fn description(&self) -> &str {
        "Apply several exact string replacements, grouped by file."
    }

    fn input_schema(&self) -> Value {
        super::schema_object(
            json!({
                "edits": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "path": {"type": "string"},
                            "old_string": {"type": "string"},
                            "new_string": {"type": "string"},
                            "replace_all": {"type": "boolean"}
                        },
                        "required": ["path", "old_string", "new_string"]
                    }
                }
            }),
            &["edits"],
        )
    }

    async fn call(&self, args: &Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let Some(edits) = args.get("edits").and_then(Value::as_array) else {
            return Ok(ToolOutput::err("multi_edit: missing 'edits' array"));
        };
        if edits.is_empty() {
            return Ok(ToolOutput::err("multi_edit: no edits supplied"));
        }
        let mut by_path: Vec<(String, Vec<&Value>)> = Vec::new();
        for edit in edits {
            let Some(path) = edit.get("path").and_then(Value::as_str) else {
                return Ok(ToolOutput::err("multi_edit: an edit is missing 'path'"));
            };
            if let Some(entry) = by_path.iter_mut().find(|(p, _)| p == path) {
                entry.1.push(edit);
            } else {
                by_path.push((path.to_string(), vec![edit]));
            }
        }
        let mut report = String::new();
        for (path, items) in by_path {
            let resolved = match resolve_path(ctx, &path) {
                Ok(p) => p,
                Err(e) => return Ok(ToolOutput::err(format!("multi_edit: {e}"))),
            };
            let mut text = fs::read_to_string(&resolved)
                .await
                .map_err(|e| anyhow::anyhow!("cannot read {path}: {e}"))?;
            for item in &items {
                let old = item.get("old_string").and_then(Value::as_str).unwrap_or("");
                let new = item.get("new_string").and_then(Value::as_str).unwrap_or("");
                if old.is_empty() {
                    return Ok(ToolOutput::err("multi_edit: empty old_string"));
                }
                let count = text.matches(old).count();
                if count == 0 {
                    return Ok(ToolOutput::err(format!(
                        "multi_edit: old string not found in {path}"
                    )));
                }
                if count > 1
                    && !item
                        .get("replace_all")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                {
                    return Ok(ToolOutput::err(format!(
                        "multi_edit: old string appears {count} times in {path}"
                    )));
                }
                text = text.replacen(old, new, 1);
            }
            fs::write(&resolved, text)
                .await
                .map_err(|e| anyhow::anyhow!("cannot write {path}: {e}"))?;
            report.push_str(&format!("edited {path} ({} replacements)\n", items.len()));
        }
        Ok(ToolOutput::ok(report.trim_end()))
    }
}
