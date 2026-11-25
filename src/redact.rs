//! Defence in depth: never let a BYOK secret reach an event, a session file,
//! stdout or an error message. The provider adapter already keeps the key out
//! of logs; this module catches secrets echoed back by tools (e.g. `env`).

use crate::events::Event;
use serde_json::Value;

/// Collect values of suspicious env vars plus explicitly configured key envs.
pub fn collect_secret_values(extra_env_names: &[String]) -> Vec<String> {
    let mut names: Vec<String> = extra_env_names.to_vec();
    {
        for (name, _) in std::env::vars() {
            let upper = name.to_ascii_uppercase();
            if ["KEY", "TOKEN", "SECRET", "PASSWORD", "CREDENTIAL"]
                .iter()
                .any(|needle| upper.contains(needle))
            {
                names.push(name);
            }
        }
    }
    names.sort();
    names.dedup();
    names
        .into_iter()
        .filter_map(|name| std::env::var(&name).ok())
        .filter(|value| value.len() >= 6)
        .collect()
}

pub fn redact(text: &str, secrets: &[String]) -> String {
    if secrets.is_empty() || text.is_empty() {
        return text.to_string();
    }
    let mut out = text.to_string();
    // Longest first so a short key that prefixes a longer one does not leave
    // a partial secret behind.
    let mut sorted = secrets.to_vec();
    sorted.sort_by_key(|s| std::cmp::Reverse(s.len()));
    for secret in sorted {
        out = out.replace(&secret, "[REDACTED]");
    }
    out
}

pub fn redact_event(event: Event, secrets: &[String]) -> Event {
    match event {
        Event::MessageDelta { role, text } => Event::MessageDelta {
            role,
            text: redact(&text, secrets),
        },
        Event::ToolResult {
            id,
            name,
            output,
            is_error,
            elapsed_ms,
        } => Event::ToolResult {
            id,
            name,
            output: redact(&output, secrets),
            is_error,
            elapsed_ms,
        },
        Event::ToolStart { id, name, args } => Event::ToolStart {
            id,
            name,
            args: redact_value(args, secrets),
        },
        Event::Error { message } => Event::Error {
            message: redact(&message, secrets),
        },
        Event::PermissionRequest {
            tool,
            target,
            decision,
        } => Event::PermissionRequest {
            tool,
            target: redact(&target, secrets),
            decision,
        },
        other => other,
    }
}

fn redact_value(value: Value, secrets: &[String]) -> Value {
    match value {
        Value::String(s) => Value::String(redact(&s, secrets)),
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|v| redact_value(v, secrets))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| (k, redact_value(v, secrets)))
                .collect(),
        ),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_all_secret_occurrences() {
        let out = redact("key=abc12345 and abc12345", &["abc12345".into()]);
        assert_eq!(out, "key=[REDACTED] and [REDACTED]");
    }

    #[test]
    fn redacts_nested_tool_args() {
        let event = Event::ToolStart {
            id: "t".into(),
            name: "bash".into(),
            args: serde_json::json!({"command": "echo abc12345", "n": [1, "abc12345"]}),
        };
        let out = redact_event(event, &["abc12345".into()]);
        let value = serde_json::to_value(out).unwrap();
        assert!(!serde_json::to_string(&value).unwrap().contains("abc12345"));
    }
}
