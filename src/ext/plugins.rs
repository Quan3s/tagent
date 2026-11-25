//! Claude Code plugin manifests and marketplaces. Unknown fields are retained
//! so `ta plugin import` can report what maps and what does not.

use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::ext::frontmatter::split_tokens;

#[derive(Clone, Debug, Deserialize)]
pub struct Plugin {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    /// Author may be a string or an object; keep it opaque.
    #[serde(default)]
    pub author: Option<Value>,
    /// Directory the manifest was loaded from.
    #[serde(default)]
    pub source: PathBuf,
    /// Binaries that must be available (e.g. `node`, `uv`).
    #[serde(default, deserialize_with = "deserialize_string_list")]
    pub requires: Vec<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Marketplace {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub owner: Option<Value>,
    #[serde(default)]
    pub plugins: Vec<MarketplacePlugin>,
    /// Directory containing `marketplace.json`.
    #[serde(default)]
    pub source: PathBuf,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MarketplacePlugin {
    pub name: String,
    /// Usually a GitHub path like `owner/repo` or a relative directory.
    #[serde(default)]
    pub source: Value,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl MarketplacePlugin {
    pub fn source_path(&self) -> Option<&str> {
        self.source.as_str()
    }
}

fn default_true() -> bool {
    true
}

fn deserialize_string_list<'de, D>(de: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(de)?;
    Ok(match value {
        Value::Array(items) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        Value::String(s) => split_tokens(&s),
        _ => Vec::new(),
    })
}

impl Plugin {
    /// Load `.claude-plugin/plugin.json` (or a root `plugin.json`) from `dir`.
    pub fn load_dir(dir: &Path) -> Result<Self> {
        let manifest = dir.join(".claude-plugin/plugin.json");
        let manifest = if manifest.is_file() {
            manifest
        } else {
            dir.join("plugin.json")
        };
        if !manifest.is_file() {
            anyhow::bail!("no plugin.json in {}", dir.display());
        }
        let text = fs::read_to_string(&manifest)
            .with_context(|| format!("failed to read {}", manifest.display()))?;
        let mut plugin: Self = serde_json::from_str(&text)
            .with_context(|| format!("invalid plugin manifest {}", manifest.display()))?;
        if plugin.name.is_empty() {
            plugin.name = dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("plugin")
                .to_string();
        }
        plugin.source = dir.to_path_buf();
        Ok(plugin)
    }

    /// Paths declared by the manifest for `commands`, `agents`, `skills`, etc.
    /// Values may be strings, arrays, or objects keyed by path.
    pub fn manifest_paths(&self, key: &str) -> Vec<PathBuf> {
        let Some(value) = self.extra.get(key) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut push = |raw: &str| {
            if raw.is_empty() {
                return;
            }
            let path = Path::new(raw);
            let resolved = if path.is_absolute() {
                path.to_path_buf()
            } else {
                self.source.join(path)
            };
            if !out.contains(&resolved) {
                out.push(resolved);
            }
        };
        match value {
            Value::String(s) => push(s),
            Value::Array(items) => {
                for item in items {
                    if let Some(s) = item.as_str() {
                        push(s);
                    }
                }
            }
            Value::Object(obj) => {
                for (path, enabled) in obj {
                    if enabled.as_bool().unwrap_or(true) {
                        push(path);
                    }
                }
            }
            _ => {}
        }
        out
    }

    /// Requirements that are not on `PATH`. Useful for `ta plugin import`.
    pub fn missing_requirements(&self) -> Vec<String> {
        self.requires
            .iter()
            .filter(|name| !binary_available(name))
            .cloned()
            .collect()
    }
}

impl Marketplace {
    pub fn load(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read marketplace {}", path.display()))?;
        let mut market: Self = serde_json::from_str(&text)
            .with_context(|| format!("invalid marketplace {}", path.display()))?;
        market.source = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| path.to_path_buf());
        Ok(market)
    }
}

/// Find plugin directories (up to `max_depth` below `root`) that contain a
/// `.claude-plugin/plugin.json` or root `plugin.json`. Missing roots are fine.
pub fn find_plugin_dirs(root: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    search_plugin_dirs(root, 0, max_depth, &mut out);
    out.sort();
    out
}

fn search_plugin_dirs(dir: &Path, depth: usize, max_depth: usize, out: &mut Vec<PathBuf>) {
    if depth > max_depth || !dir.is_dir() || out.contains(&dir.to_path_buf()) {
        return;
    }
    if dir.join(".claude-plugin/plugin.json").is_file() || dir.join("plugin.json").is_file() {
        out.push(dir.to_path_buf());
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with('.') || n == "node_modules")
            .unwrap_or(false)
        {
            continue;
        }
        search_plugin_dirs(&path, depth + 1, max_depth, out);
    }
}

fn binary_available(name: &str) -> bool {
    let path = Path::new(name);
    if path.components().count() > 1 {
        return path.is_file() && is_executable(path);
    }
    let Some(path_var) = std::env::var_os("PATH") else {
        return false;
    };
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() && is_executable(&candidate) {
            return true;
        }
    }
    false
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_manifest_and_marketplace_tolerantly() {
        let root = std::env::temp_dir().join(format!("ta-plugin-test-{}", std::process::id()));
        let plugin_dir = root.join("demo");
        fs::create_dir_all(plugin_dir.join(".claude-plugin")).unwrap();
        fs::write(
            plugin_dir.join(".claude-plugin/plugin.json"),
            r#"{"name":"demo","description":"Demo","requires":"node uv","unknown":{"x":1}}"#,
        )
        .unwrap();
        let plugin = Plugin::load_dir(&plugin_dir).unwrap();
        assert_eq!(plugin.name, "demo");
        assert_eq!(plugin.requires, vec!["node", "uv"]);
        assert!(plugin.extra.contains_key("unknown"));
        assert!(plugin.enabled);

        let market_path = root.join("marketplace.json");
        fs::write(
            &market_path,
            r#"{"name":"market","owner":{"name":"x"},"plugins":[{"name":"demo","source":"./demo","extra":1}]}"#,
        )
        .unwrap();
        let market = Marketplace::load(&market_path).unwrap();
        assert_eq!(market.plugins.len(), 1);
        assert_eq!(market.plugins[0].source_path(), Some("./demo"));
        assert_eq!(find_plugin_dirs(&root, 2), vec![plugin_dir.clone()]);
        fs::remove_dir_all(root).unwrap();
    }
}
