use super::{resolve_path, schema_object, truncate_output, Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use ignore::WalkBuilder;
use regex::RegexBuilder;
use serde_json::{json, Value};
use std::path::PathBuf;

pub struct GrepTool;

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        "Search file contents with a regular expression."
    }

    fn input_schema(&self) -> Value {
        schema_object(
            json!({
                "pattern": {"type": "string", "description": "Regular expression"},
                "path": {"type": "string", "description": "Optional file or directory"},
                "case_insensitive": {"type": "boolean", "default": false},
                "max_results": {"type": "integer", "default": 200}
            }),
            &["pattern"],
        )
    }

    async fn call(&self, args: &Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let Some(pattern) = args.get("pattern").and_then(Value::as_str) else {
            return Ok(ToolOutput::err("grep: missing 'pattern'"));
        };
        let insensitive = args
            .get("case_insensitive")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let max = args
            .get("max_results")
            .and_then(Value::as_u64)
            .unwrap_or(200)
            .min(2000) as usize;
        let re = match RegexBuilder::new(pattern)
            .case_insensitive(insensitive)
            .build()
        {
            Ok(r) => r,
            Err(e) => return Ok(ToolOutput::err(format!("grep: invalid regex: {e}"))),
        };
        let target: PathBuf = if let Some(path) = args.get("path").and_then(Value::as_str) {
            match resolve_path(ctx, path) {
                Ok(p) => p,
                Err(e) => return Ok(ToolOutput::err(format!("grep: {e}"))),
            }
        } else {
            ctx.workspace.clone()
        };
        let mut files: Vec<PathBuf> = if target.is_file() {
            vec![target]
        } else {
            WalkBuilder::new(&target)
                .max_depth(Some(32))
                .build()
                .filter_map(Result::ok)
                .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
                .map(|e| e.into_path())
                .collect()
        };
        files.sort();
        let mut out = String::new();
        let mut count = 0usize;
        for file in files {
            let Ok(bytes) = tokio::fs::read(&file).await else {
                continue;
            };
            if bytes.contains(&0) {
                continue;
            }
            let text = String::from_utf8_lossy(&bytes);
            let rel = file.strip_prefix(&ctx.workspace).unwrap_or(&file);
            for (index, line) in text.lines().enumerate() {
                if re.is_match(line) {
                    out.push_str(&format!("{}:{}:{}\n", rel.display(), index + 1, line));
                    count += 1;
                    if count >= max {
                        break;
                    }
                }
            }
            if count >= max {
                break;
            }
        }
        if out.is_empty() {
            return Ok(ToolOutput::ok("(no matches)"));
        }
        Ok(ToolOutput::ok(truncate_output(&out, 256 * 1024)))
    }
}
