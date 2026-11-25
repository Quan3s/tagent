//! Subagent definitions (`agents/*.md`).

use anyhow::{Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::ext::frontmatter;

#[derive(Clone, Debug)]
pub struct AgentMeta {
    pub name: String,
    pub description: String,
    pub tools: Vec<String>,
    pub body: String,
    pub path: PathBuf,
}

impl AgentMeta {
    pub fn parse(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read agent {}", path.display()))?;
        let fm = frontmatter::parse(&text)?;
        let tools = fm.effective_tools();
        let name = fm
            .name
            .or_else(|| {
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .map(str::to_string)
            })
            .unwrap_or_default();
        Ok(Self {
            name,
            description: fm.description,
            tools,
            body: fm.body,
            path: path.to_path_buf(),
        })
    }
}

pub fn load_agents(root: &Path) -> Result<Vec<AgentMeta>> {
    let mut out = Vec::new();
    if !root.is_dir() {
        return Ok(out);
    }
    let mut entries: Vec<_> = fs::read_dir(root)
        .with_context(|| format!("failed to list {}", root.display()))?
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let is_md = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("md"))
            .unwrap_or(false);
        if path.is_file() && is_md {
            out.push(AgentMeta::parse(&path)?);
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_agent_frontmatter() {
        let root = std::env::temp_dir().join(format!("ta-agent-test-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let file = root.join("reviewer.md");
        fs::write(&file, "---\nname: reviewer\ndescription: Reviews code\ntools: Read, Grep\n---\nReview carefully.\n").unwrap();
        let agents = load_agents(&root).unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].name, "reviewer");
        assert_eq!(agents[0].tools, vec!["Read", "Grep"]);
        assert!(agents[0].body.contains("Review carefully."));
        fs::remove_dir_all(root).unwrap();
    }
}
