use super::{resolve_path, schema_object, truncate_output, Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use ignore::WalkBuilder;
use serde_json::{json, Value};

pub struct LsTool;

#[async_trait]
impl Tool for LsTool {
    fn name(&self) -> &str {
        "ls"
    }

    fn description(&self) -> &str {
        "List files and directories inside the workspace."
    }

    fn input_schema(&self) -> Value {
        schema_object(
            json!({
                "path": {"type": "string", "description": "Directory (default: workspace root)"},
                "recursive": {"type": "boolean", "default": false}
            }),
            &[],
        )
    }

    async fn call(&self, args: &Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let raw = args.get("path").and_then(Value::as_str).unwrap_or(".");
        let root = match resolve_path(ctx, raw) {
            Ok(p) => p,
            Err(e) => return Ok(ToolOutput::err(format!("ls: {e}"))),
        };
        if !root.is_dir() {
            return Ok(ToolOutput::err("ls: path is not a directory"));
        }
        let recursive = args
            .get("recursive")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let mut rows: Vec<String> = Vec::new();
        if recursive {
            for entry in WalkBuilder::new(&root)
                .max_depth(Some(12))
                .build()
                .filter_map(Result::ok)
            {
                if entry.depth() == 0 {
                    continue;
                }
                let rel = entry
                    .path()
                    .strip_prefix(&ctx.workspace)
                    .unwrap_or(entry.path());
                let suffix = if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    "/"
                } else {
                    ""
                };
                rows.push(format!("{}{}", rel.display(), suffix));
            }
        } else {
            let mut entries = match tokio::fs::read_dir(&root).await {
                Ok(e) => e,
                Err(e) => return Ok(ToolOutput::err(format!("ls: cannot read directory: {e}"))),
            };
            while let Ok(Some(entry)) = entries.next_entry().await {
                let suffix = if entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
                    "/"
                } else {
                    ""
                };
                rows.push(format!("{}{}", entry.file_name().to_string_lossy(), suffix));
            }
        }
        rows.sort();
        if rows.is_empty() {
            return Ok(ToolOutput::ok("(empty directory)"));
        }
        Ok(ToolOutput::ok(truncate_output(
            &rows.join("\n"),
            128 * 1024,
        )))
    }
}
