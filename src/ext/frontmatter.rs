//! Minimal YAML frontmatter parsing shared by commands, skills and agents.
//!
//! Claude Code markdown files start with a `---` delimited YAML block. The
//! parser is deliberately tolerant: missing frontmatter yields empty metadata
//! and unknown keys are kept in `extra` so other agents can interpret them.

use anyhow::{Context, Result};
use serde_json::{Map, Value};

#[derive(Clone, Debug, Default)]
pub struct Frontmatter {
    pub name: Option<String>,
    pub description: String,
    /// `allowed-tools` (Claude Code) or `tools` (other agents).
    pub allowed_tools: Vec<String>,
    pub tools: Vec<String>,
    pub argument_hint: Option<String>,
    pub model: Option<String>,
    /// Markdown after the closing `---`.
    pub body: String,
    pub extra: Map<String, Value>,
}

impl Frontmatter {
    /// Combined tool restriction list, preferring `allowed-tools` when both
    /// spellings are present.
    pub fn effective_tools(&self) -> Vec<String> {
        if self.allowed_tools.is_empty() {
            self.tools.clone()
        } else {
            self.allowed_tools.clone()
        }
    }
}

/// Split a markdown document into its YAML block and body.
fn split(input: &str) -> (&str, Option<&str>) {
    let text = input.strip_prefix('\u{feff}').unwrap_or(input);
    if !text.starts_with("---") {
        return ("", Some(text));
    }
    let rest = match text[3..]
        .strip_prefix("\r\n")
        .or_else(|| text[3..].strip_prefix('\n'))
    {
        Some(rest) => rest,
        None => return ("", Some(text)),
    };
    let mut offset = 0usize;
    for line in rest.split_inclusive('\n') {
        let trimmed = line.trim_end_matches('\n').trim_end_matches('\r').trim();
        let next = offset + line.len();
        if trimmed == "---" {
            return (&rest[..offset], Some(&rest[next..]));
        }
        offset = next;
    }
    ("", Some(text))
}

/// Parse a markdown document's frontmatter and body.
pub fn parse(input: &str) -> Result<Frontmatter> {
    let (yaml, body) = split(input);
    let mut out = Frontmatter {
        body: body.unwrap_or_default().to_string(),
        ..Frontmatter::default()
    };
    if yaml.trim().is_empty() {
        return Ok(out);
    }

    let parsed: serde_yaml::Value =
        serde_yaml::from_str(yaml).context("invalid YAML frontmatter")?;
    let value = serde_json::to_value(parsed).context("unsupported YAML frontmatter value")?;
    let obj = match value.as_object() {
        Some(obj) => obj.clone(),
        None => return Ok(out),
    };

    out.name = take_string(&obj, &["name"]);
    out.description = take_string(&obj, &["description", "desc", "about"]).unwrap_or_default();
    out.allowed_tools = take_list(&obj, &["allowed-tools", "allowed_tools", "allowedTools"]);
    out.tools = take_list(&obj, &["tools"]);
    out.argument_hint = take_string(&obj, &["argument-hint", "argument_hint", "argumentHint"]);
    out.model = take_string(&obj, &["model"]);

    for (key, value) in obj {
        if !matches!(
            key.as_str(),
            "name"
                | "description"
                | "desc"
                | "about"
                | "allowed-tools"
                | "allowed_tools"
                | "allowedTools"
                | "tools"
                | "argument-hint"
                | "argument_hint"
                | "argumentHint"
                | "model"
        ) {
            out.extra.insert(key, value);
        }
    }
    Ok(out)
}

fn take_string(obj: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    for key in keys {
        match obj.get(*key) {
            Some(Value::String(s)) => return Some(s.trim().to_string()),
            Some(Value::Number(n)) => return Some(n.to_string()),
            Some(Value::Bool(b)) => return Some(b.to_string()),
            _ => {}
        }
    }
    None
}

fn take_list(obj: &Map<String, Value>, keys: &[&str]) -> Vec<String> {
    for key in keys {
        match obj.get(*key) {
            Some(Value::Array(items)) => {
                let list: Vec<String> = items
                    .iter()
                    .filter_map(|item| match item {
                        Value::String(s) => Some(s.trim().to_string()),
                        Value::Number(n) => Some(n.to_string()),
                        _ => None,
                    })
                    .filter(|s| !s.is_empty())
                    .collect();
                if !list.is_empty() {
                    return list;
                }
            }
            Some(Value::String(s)) => {
                let list = split_tokens(s);
                if !list.is_empty() {
                    return list;
                }
            }
            _ => {}
        }
    }
    Vec::new()
}

/// Split a tool list on whitespace/commas, but not inside `Tool(pattern)` or
/// quotes. `Bash(git status:*) Read` becomes two tools.
pub fn split_tokens(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    for ch in input.chars() {
        if let Some(q) = quote {
            current.push(ch);
            if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' => {
                quote = Some(ch);
                current.push(ch);
            }
            '(' | '[' | '{' => {
                depth += 1;
                current.push(ch);
            }
            ')' | ']' | '}' => {
                depth -= 1;
                current.push(ch);
            }
            c if depth == 0 && (c.is_whitespace() || c == ',') => {
                if !current.is_empty() {
                    out.push(current.trim().to_string());
                    current.clear();
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        out.push(current.trim().to_string());
    }
    out
}

/// Shell-like argument tokenization used for `$1`, `$2`, ... substitution.
pub fn split_args(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    for ch in input.chars() {
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            } else {
                current.push(ch);
            }
            continue;
        }
        match ch {
            '\'' | '"' => {
                quote = Some(ch);
                started = true;
            }
            '\\' => started = true,
            c if c.is_whitespace() => {
                if started {
                    out.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            _ => {
                current.push(ch);
                started = true;
            }
        }
    }
    if started {
        out.push(current);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_aliases_and_tool_patterns_with_spaces() {
        let doc = "---\ndescription: Say hello\nallowed-tools: Bash(git status:*) Read\nargument-hint: <name>\nmodel: fast\ncustom: keep\n---\n\nBody text.\n";
        let fm = parse(doc).unwrap();
        assert_eq!(fm.description, "Say hello");
        assert_eq!(fm.allowed_tools, vec!["Bash(git status:*)", "Read"]);
        assert_eq!(fm.argument_hint.as_deref(), Some("<name>"));
        assert_eq!(fm.model.as_deref(), Some("fast"));
        assert_eq!(fm.extra.get("custom").unwrap(), "keep");
        assert_eq!(fm.body.trim(), "Body text.");
    }

    #[test]
    fn missing_frontmatter_is_tolerated() {
        let fm = parse("Just a body.\n").unwrap();
        assert!(fm.description.is_empty());
        assert_eq!(fm.body, "Just a body.\n");
    }
}
