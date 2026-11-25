use super::{schema_object, Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::fs;

pub struct TodoTool;

#[async_trait]
impl Tool for TodoTool {
    fn name(&self) -> &str {
        "todo"
    }

    fn description(&self) -> &str {
        "Create or update the visible todo list for this session."
    }

    fn input_schema(&self) -> Value {
        schema_object(
            json!({
                "todos": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "content": {"type": "string"},
                            "status": {"type": "string", "enum": ["pending", "in_progress", "completed"]}
                        },
                        "required": ["content", "status"]
                    }
                }
            }),
            &["todos"],
        )
    }

    async fn call(&self, args: &Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let Some(todos) = args.get("todos").and_then(Value::as_array) else {
            return Ok(ToolOutput::err("todo: missing 'todos' array"));
        };
        if todos.len() > 100 {
            return Ok(ToolOutput::err("todo: too many items (max 100)"));
        }
        let dir = ctx.checkpoint_root.join("todos");
        fs::create_dir_all(&dir).await?;
        let path = dir.join(format!("{}.json", sanitize(&ctx.session_id)));
        let body = json!({"todos": todos});
        fs::write(&path, serde_json::to_string_pretty(&body)?).await?;
        Ok(ToolOutput::ok(serde_json::to_string(&body)?))
    }
}

fn sanitize(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_session_id() {
        assert_eq!(sanitize("a/b-1"), "a_b_1");
    }
}
