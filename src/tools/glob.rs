use super::{resolve_path, schema_object, truncate_output, Tool, ToolContext, ToolOutput};
use crate::perm::wildcard;
use anyhow::Result;
use async_trait::async_trait;
use ignore::WalkBuilder;
use serde_json::{json, Value};

pub struct GlobTool;

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str {
        "glob"
    }

    fn description(&self) -> &str {
        "Find files by path pattern using gitignore-aware traversal."
    }

    fn input_schema(&self) -> Value {
        schema_object(
            json!({
                "pattern": {"type": "string", "description": "Glob, e.g. src/**/*.rs"},
                "path": {"type": "string", "description": "Optional directory to search"}
            }),
            &["pattern"],
        )
    }

    async fn call(&self, args: &Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let Some(pattern) = args.get("pattern").and_then(Value::as_str) else {
            return Ok(ToolOutput::err("glob: missing 'pattern'"));
        };
        let root = if let Some(path) = args.get("path").and_then(Value::as_str) {
            match resolve_path(ctx, path) {
                Ok(p) => p,
                Err(e) => return Ok(ToolOutput::err(format!("glob: {e}"))),
            }
        } else {
            ctx.workspace.clone()
        };
        if !root.is_dir() {
            return Ok(ToolOutput::err("glob: path is not a directory"));
        }
        let mut paths: Vec<String> = WalkBuilder::new(&root)
            .max_depth(Some(32))
            .build()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().map(|t| t.is_file()).unwrap_or(false))
            .filter_map(|entry| {
                let rel = entry
                    .path()
                    .strip_prefix(&ctx.workspace)
                    .unwrap_or(entry.path());
                let rel = rel.to_string_lossy().replace('\\', "/");
                wildcard(pattern, &rel).then_some(rel)
            })
            .take(1000)
            .collect();
        paths.sort();
        if paths.is_empty() {
            return Ok(ToolOutput::ok("(no matches)"));
        }
        Ok(ToolOutput::ok(truncate_output(
            &paths.join("\n"),
            128 * 1024,
        )))
    }
}
