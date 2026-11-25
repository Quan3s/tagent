//! Agent Skills (`skills/*/SKILL.md`). Only name and description go into the
//! system prompt; the full body is loaded on activation.

use anyhow::{bail, Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::ext::frontmatter;

#[derive(Clone, Debug)]
pub struct SkillMeta {
    pub name: String,
    pub description: String,
    /// Directory containing `SKILL.md`, so supplementary files can be found.
    pub path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct LoadedSkill {
    pub name: String,
    pub description: String,
    /// Full markdown instructions (everything after the YAML frontmatter).
    pub body: String,
    /// Directory containing `SKILL.md`.
    pub path: PathBuf,
}

impl LoadedSkill {
    /// Read a skill from either its directory or a direct `SKILL.md` path.
    pub fn read(path: &Path) -> Result<Self> {
        let file = if path.is_dir() {
            path.join("SKILL.md")
        } else {
            path.to_path_buf()
        };
        if !file.is_file() {
            bail!("no SKILL.md at {}", path.display());
        }
        let text = fs::read_to_string(&file)
            .with_context(|| format!("failed to read {}", file.display()))?;
        let fm = frontmatter::parse(&text)?;
        let name = fm
            .name
            .or_else(|| {
                file.parent()
                    .and_then(Path::file_name)
                    .and_then(|n| n.to_str())
                    .map(str::to_string)
            })
            .unwrap_or_default();
        Ok(Self {
            name,
            description: fm.description,
            body: fm.body,
            path: file
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| path.to_path_buf()),
        })
    }
}

/// Scan `<root>/<skill>/SKILL.md` directories.
pub fn load_skills(root: &Path) -> Result<Vec<SkillMeta>> {
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
        let dir = entry.path();
        let skill_file = if dir.is_dir() {
            dir.join("SKILL.md")
        } else {
            continue;
        };
        if !skill_file.is_file() {
            continue;
        }
        let loaded = LoadedSkill::read(&dir)?;
        out.push(SkillMeta {
            name: loaded.name,
            description: loaded.description,
            path: dir,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_skill_body_and_directory() {
        let root = std::env::temp_dir().join(format!("ta-skill-test-{}", std::process::id()));
        let dir = root.join("demo");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo skill\n---\nDo the thing.\n",
        )
        .unwrap();
        let skills = load_skills(&root).unwrap();
        assert_eq!(skills.len(), 1);
        let loaded = LoadedSkill::read(&skills[0].path).unwrap();
        assert_eq!(loaded.name, "demo");
        assert_eq!(loaded.description, "Demo skill");
        assert_eq!(loaded.body.trim(), "Do the thing.");
        assert_eq!(loaded.path, dir);
        fs::remove_dir_all(root).unwrap();
    }
}
