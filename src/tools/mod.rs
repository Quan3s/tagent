//! Built-in tool registry and executor. M0: `read`, `write`, `bash`.

pub mod bash;
pub mod read;
pub mod write;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::{
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::Duration,
};

#[derive(Clone, Debug)]
pub struct ToolContext {
    /// Workspace plus any `--add-dir` roots. Every path is confined to one of them.
    pub roots: Vec<PathBuf>,
    pub bash_timeout: Duration,
    /// Env vars removed from tool subprocesses so a tool cannot exfiltrate keys.
    pub secret_env_names: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct ToolOutput {
    pub content: String,
    pub is_error: bool,
}

impl ToolOutput {
    pub fn ok(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
        }
    }

    pub fn err(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: true,
        }
    }
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn input_schema(&self) -> Value;
    async fn call(&self, args: &Value, ctx: &ToolContext) -> Result<ToolOutput>;
}

pub fn builtin() -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(read::ReadTool),
        Arc::new(write::WriteTool),
        Arc::new(bash::BashTool),
    ]
}

pub fn schema_object(properties: Value, required: &[&str]) -> Value {
    serde_json::json!({
        "type": "object",
        "properties": properties,
        "required": required,
    })
}

/// Lexically collapse `.` and `..` before touching the filesystem.
pub fn lexical_normalize(path: impl AsRef<Path>) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.as_ref().components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Resolve a model-supplied path against the allowed roots. Rejects traversal
/// and symlinks that point outside a root, including for files not yet created.
pub fn resolve_path(ctx: &ToolContext, raw: &str) -> Result<PathBuf> {
    if raw.is_empty() {
        bail!("path is empty");
    }
    let input = Path::new(raw);
    for root in &ctx.roots {
        let base = root.canonicalize()?;
        let candidate = lexical_normalize(if input.is_absolute() {
            input.to_path_buf()
        } else {
            base.join(input)
        });
        if !candidate.starts_with(&base) {
            continue;
        }
        let checked = if candidate.exists() {
            candidate.canonicalize()?
        } else {
            let parent = candidate
                .parent()
                .context("path has no parent directory")?
                .canonicalize()?;
            let name = candidate.file_name().context("path has no file name")?;
            parent.join(name)
        };
        if checked.starts_with(&base) {
            return Ok(checked);
        }
    }
    bail!("path '{raw}' is outside the workspace")
}

pub fn truncate_output(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… [truncated {} bytes]", &text[..end], text.len() - end)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(root: PathBuf) -> ToolContext {
        ToolContext {
            roots: vec![root],
            bash_timeout: Duration::from_secs(5),
            secret_env_names: Vec::new(),
        }
    }

    #[test]
    fn traversal_is_rejected() {
        let root = std::env::temp_dir().join(format!("ta-path-test-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let c = ctx(root.clone());
        assert!(resolve_path(&c, "a.txt").is_ok());
        assert!(resolve_path(&c, "../outside.txt").is_err());
        assert!(resolve_path(&c, "/etc/passwd").is_err());
        assert!(resolve_path(&c, "a/../../b").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn truncation_respects_utf8_boundaries() {
        let s = "é".repeat(100);
        let out = truncate_output(&s, 51);
        assert!(out.ends_with("]"));
        assert!(!out.contains('\u{fffd}'));
    }
}
