//! Append-only JSONL sessions. One record per event; `id` is monotonic so a
//! client can resume from `last_event_id` without replaying the whole stream.

use crate::{
    config::sessions_dir,
    events::{Event, Record, Role},
    provider::{ContentBlock, Message},
};
use anyhow::{bail, Context, Result};
use std::{
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub struct SessionStore {
    pub id: String,
    path: PathBuf,
    last_id: u64,
}

impl SessionStore {
    pub fn create() -> Result<Self> {
        Self::create_in(sessions_dir()?)
    }

    pub fn create_in(root: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&root)?;
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let id = format!("{nanos}-{}", std::process::id());
        Self::new(root, id)
    }

    pub fn new(root: PathBuf, id: impl Into<String>) -> Result<Self> {
        std::fs::create_dir_all(&root)?;
        let id = id.into();
        if id.contains('/') || id.contains('\\') || id == "." || id == ".." {
            bail!("invalid session id");
        }
        let path = root.join(format!("{id}.jsonl"));
        if path.exists() {
            let existing = Self::load(root, &id)?;
            return Ok(existing);
        }
        OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)?;
        Ok(Self {
            id,
            path,
            last_id: 0,
        })
    }

    pub fn load(root: PathBuf, id: &str) -> Result<Self> {
        if id.contains('/') || id.contains('\\') || id == "." || id == ".." {
            bail!("invalid session id");
        }
        let path = root.join(format!("{id}.jsonl"));
        if !path.exists() {
            bail!("session '{id}' not found");
        }
        let mut last_id = 0;
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let record: Record = serde_json::from_str(line).context("corrupt session record")?;
            last_id = last_id.max(record.id);
        }
        Ok(Self {
            id: id.to_string(),
            path,
            last_id,
        })
    }

    pub fn latest(root: PathBuf) -> Result<Option<Self>> {
        if !root.exists() {
            return Ok(None);
        }
        let mut files: Vec<(SystemTime, String, PathBuf)> = Vec::new();
        for entry in std::fs::read_dir(&root)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                let modified = entry.metadata()?.modified().unwrap_or(UNIX_EPOCH);
                let id = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .to_string();
                files.push((modified, id, path));
            }
        }
        // Filesystem mtimes can share a coarse timestamp; the id ends in a
        // nanosecond stamp, so it breaks ties deterministically.
        files.sort_by(|(ta, ia, _), (tb, ib, _)| ta.cmp(tb).then_with(|| ia.cmp(ib)));
        let Some((_, _, path)) = files.pop() else {
            return Ok(None);
        };
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .context("invalid session file name")?;
        Ok(Some(Self::load(root, id)?))
    }

    pub fn last_id(&self) -> u64 {
        self.last_id
    }

    pub fn records(&self) -> Result<Vec<Record>> {
        let text = std::fs::read_to_string(&self.path)?;
        let mut records = Vec::new();
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            records.push(serde_json::from_str(line).context("corrupt session record")?);
        }
        Ok(records)
    }

    pub fn append(&mut self, event: Event) -> Result<Record> {
        let record = Record {
            id: self.last_id + 1,
            ts: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            event,
        };
        let mut file = OpenOptions::new().append(true).open(&self.path)?;
        serde_json::to_writer(&mut file, &record)?;
        file.write_all(b"\n")?;
        file.flush()?;
        self.last_id = record.id;
        Ok(record)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Rebuild the provider message list from protocol events.
pub fn messages(records: &[Record]) -> Vec<Message> {
    let mut out: Vec<Message> = Vec::new();
    let mut current: Option<Role> = None;

    for record in records {
        match &record.event {
            Event::MessageDelta { role, text } => {
                if current != Some(*role) {
                    out.push(Message {
                        role: *role,
                        content: Vec::new(),
                    });
                    current = Some(*role);
                }
                if let Some(last) = out.last_mut() {
                    match last.content.last_mut() {
                        Some(ContentBlock::Text { text: existing }) => existing.push_str(text),
                        _ => last.content.push(ContentBlock::Text { text: text.clone() }),
                    }
                }
            }
            Event::ToolStart { id, name, args } => {
                if current != Some(Role::Assistant) {
                    out.push(Message {
                        role: Role::Assistant,
                        content: Vec::new(),
                    });
                    current = Some(Role::Assistant);
                }
                if let Some(last) = out.last_mut() {
                    last.content.push(ContentBlock::ToolUse {
                        id: id.clone(),
                        name: name.clone(),
                        input: args.clone(),
                    });
                }
            }
            Event::ToolResult {
                id,
                output,
                is_error,
                ..
            } => {
                if current != Some(Role::User) {
                    out.push(Message {
                        role: Role::User,
                        content: Vec::new(),
                    });
                    current = Some(Role::User);
                }
                if let Some(last) = out.last_mut() {
                    last.content.push(ContentBlock::ToolResult {
                        tool_use_id: id.clone(),
                        content: output.clone(),
                        is_error: *is_error,
                    });
                }
            }
            _ => {}
        }
    }
    out
}

pub fn push_user_text(messages: &mut Vec<Message>, text: &str) {
    if let Some(last) = messages.last_mut() {
        if last.role == Role::User {
            if let Some(ContentBlock::Text { text: existing }) = last.content.last_mut() {
                existing.push('\n');
                existing.push_str(text);
                return;
            }
        }
    }
    messages.push(Message::user_text(text));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn root(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("ta-session-{tag}-{}", std::process::id()))
    }

    #[test]
    fn append_load_and_replay() {
        let dir = root("replay");
        let _ = std::fs::remove_dir_all(&dir);
        let mut store = SessionStore::create_in(dir.clone()).unwrap();
        store
            .append(Event::MessageDelta {
                role: Role::User,
                text: "hello".into(),
            })
            .unwrap();
        store
            .append(Event::MessageDelta {
                role: Role::Assistant,
                text: "sure".into(),
            })
            .unwrap();
        store
            .append(Event::ToolStart {
                id: "t1".into(),
                name: "read".into(),
                args: json!({"path":"a.txt"}),
            })
            .unwrap();
        store
            .append(Event::ToolResult {
                id: "t1".into(),
                name: "read".into(),
                output: "body".into(),
                is_error: false,
                elapsed_ms: 1,
            })
            .unwrap();

        let loaded = SessionStore::load(dir.clone(), &store.id).unwrap();
        assert_eq!(loaded.last_id(), 4);
        let msgs = messages(&loaded.records().unwrap());
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].role, Role::User);
        assert!(msgs[1]
            .content
            .iter()
            .any(|block| matches!(block, ContentBlock::ToolUse { .. })));
        assert!(matches!(
            msgs[2].content[0],
            ContentBlock::ToolResult { .. }
        ));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn latest_picks_most_recent() {
        let dir = root("latest");
        let _ = std::fs::remove_dir_all(&dir);
        let a = SessionStore::create_in(dir.clone()).unwrap();
        let b = SessionStore::create_in(dir.clone()).unwrap();
        let latest = SessionStore::latest(dir.clone()).unwrap().unwrap();
        assert_eq!(latest.id, b.id);
        assert_ne!(latest.id, a.id);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
