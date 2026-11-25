//! Claude Code style slash commands (`commands/*.md`) and template rendering.

use anyhow::{bail, Context, Result};
use regex::Regex;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
};

use crate::ext::frontmatter;

#[derive(Clone, Debug)]
pub struct Command {
    pub name: String,
    pub description: String,
    pub allowed_tools: Vec<String>,
    pub argument_hint: Option<String>,
    pub model: Option<String>,
    pub body: String,
    pub source: PathBuf,
}

impl Command {
    /// Parse a single command markdown file. `name` normally comes from the
    /// file stem; frontmatter `name` may override it.
    pub fn parse(path: &Path, name: impl Into<String>) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read command {}", path.display()))?;
        Self::parse_text(&text, name, path)
    }

    pub fn parse_text(text: &str, name: impl Into<String>, source: &Path) -> Result<Self> {
        let fm = frontmatter::parse(text)?;
        let tools = fm.effective_tools();
        Ok(Self {
            name: fm.name.clone().unwrap_or_else(|| name.into()),
            description: fm.description,
            allowed_tools: tools,
            argument_hint: fm.argument_hint,
            model: fm.model,
            body: fm.body,
            source: source.to_path_buf(),
        })
    }

    /// Render this command's template with `args`.
    pub fn render(&self, args: &str, workspace: &Path) -> Result<String> {
        render_template(&self.body, args, self.source.parent(), workspace)
    }
}

/// Load `commands/**/*.md`. Nested paths are joined with `:` (Claude Code
/// subdirectories), and an optional prefix namespaces plugin commands as
/// `plugin:command`.
pub fn load_commands(dir: &Path, prefix: Option<&str>) -> Result<Vec<Command>> {
    let mut out = Vec::new();
    if !dir.is_dir() {
        return Ok(out);
    }
    walk_commands(dir, dir, prefix, 0, &mut out)?;
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

fn walk_commands(
    root: &Path,
    dir: &Path,
    prefix: Option<&str>,
    depth: usize,
    out: &mut Vec<Command>,
) -> Result<()> {
    if depth > 4 {
        return Ok(());
    }
    let mut entries: Vec<_> = fs::read_dir(dir)
        .with_context(|| format!("failed to list {}", dir.display()))?
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            if path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with('.'))
                .unwrap_or(false)
            {
                continue;
            }
            walk_commands(root, &path, prefix, depth + 1, out)?;
            continue;
        }
        if !is_markdown(&path) {
            continue;
        }
        let relative = path.strip_prefix(root).unwrap_or(&path);
        let mut parts: Vec<String> = relative
            .components()
            .filter_map(|c| c.as_os_str().to_str())
            .map(str::to_string)
            .collect();
        if let Some(last) = parts.last_mut() {
            if last.ends_with(".md") {
                last.truncate(last.len() - 3);
            }
        }
        let mut name_parts: Vec<String> = Vec::new();
        if let Some(prefix) = prefix {
            name_parts.push(prefix.to_string());
        }
        name_parts.extend(parts);
        let name = name_parts.join(":");
        if name.is_empty() {
            continue;
        }
        out.push(Command::parse(&path, name)?);
    }
    Ok(())
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"))
        .unwrap_or(false)
}

/// Expand `$ARGUMENTS`, `$1`..`$9`, `` !`cmd` `` and `@file` in a template.
///
/// `@file` paths are resolved relative to the template's own directory, the
/// enclosing plugin root, and the workspace (in that order). Shell commands
/// run through `sh -c` with no privileged environment injection.
pub fn render_template(
    template: &str,
    args: &str,
    base: Option<&Path>,
    workspace: &Path,
) -> Result<String> {
    let plugin_root = base.and_then(find_plugin_root);
    let mut text = expand_claude_vars(template, workspace, plugin_root.as_deref());

    text = replace_arguments(&text, args);

    // `@file` references are read-only. Do this before shell substitution so
    // that file contents containing `@` are not expanded again.
    let file_re = Regex::new(r#"@([A-Za-z0-9_./~\-]+)"#)?;
    let mut file_error: Option<String> = None;
    text = file_re
        .replace_all(&text, |caps: &regex::Captures| {
            let reference = &caps[1];
            match read_reference(reference, base, plugin_root.as_deref(), workspace) {
                Ok(contents) => contents,
                Err(err) => {
                    if file_error.is_none() {
                        file_error = Some(format!("@{}: {}", reference, err));
                    }
                    caps[0].to_string()
                }
            }
        })
        .into_owned();
    if let Some(err) = file_error {
        bail!("{}", err);
    }

    let shell_re = Regex::new(r#"!\x60([^\x60]*)\x60"#)?;
    let mut shell_error: Option<String> = None;
    text = shell_re
        .replace_all(&text, |caps: &regex::Captures| {
            let command = &caps[1];
            match shell_output(command) {
                Ok(output) => output,
                Err(err) => {
                    if shell_error.is_none() {
                        shell_error = Some(format!("shell substitution failed: {}", err));
                    }
                    caps[0].to_string()
                }
            }
        })
        .into_owned();
    if let Some(err) = shell_error {
        bail!("{}", err);
    }
    Ok(text)
}

fn replace_arguments(text: &str, args: &str) -> String {
    let positional = frontmatter::split_args(args);
    let mut out = text.replace("$ARGUMENTS", args.trim());
    for index in (1..=9).rev() {
        let value = positional.get(index - 1).map(String::as_str).unwrap_or("");
        out = out.replace(&format!("${}", index), value);
    }
    out
}

fn expand_claude_vars(text: &str, workspace: &Path, plugin_root: Option<&Path>) -> String {
    let mut out = text.replace(
        "${CLAUDE_PLUGIN_ROOT}",
        &path_text(plugin_root.unwrap_or(workspace)),
    );
    out = out.replace(
        "$CLAUDE_PLUGIN_ROOT",
        &path_text(plugin_root.unwrap_or(workspace)),
    );
    out = out.replace("${TA_WORKSPACE}", &path_text(workspace));
    out
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn read_reference(
    reference: &str,
    base: Option<&Path>,
    plugin_root: Option<&Path>,
    workspace: &Path,
) -> Result<String> {
    let raw = Path::new(reference);
    let mut candidates = Vec::new();
    if raw.is_absolute() {
        candidates.push(raw.to_path_buf());
    } else {
        if let Some(base) = base {
            candidates.push(base.join(raw));
        }
        if let Some(plugin_root) = plugin_root {
            candidates.push(plugin_root.join(raw));
        }
        candidates.push(workspace.join(raw));
    }
    for candidate in candidates {
        if candidate.is_file() {
            return fs::read_to_string(&candidate)
                .with_context(|| format!("failed to read {}", candidate.display()));
        }
        if candidate.is_dir() {
            let mut names: Vec<String> = fs::read_dir(&candidate)
                .with_context(|| format!("failed to list {}", candidate.display()))?
                .filter_map(Result::ok)
                .filter_map(|e| e.file_name().into_string().ok())
                .collect();
            names.sort();
            return Ok(names.join("\n"));
        }
    }
    bail!("file not found: {}", reference)
}

fn shell_output(command: &str) -> Result<String> {
    let output = ProcessCommand::new("sh")
        .arg("-c")
        .arg(command)
        .output()
        .with_context(|| format!("failed to run shell substitution `{}`", command))?;
    if !output.status.success() {
        bail!(
            "shell substitution `{}` exited with status {}",
            command,
            output.status
        );
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text.trim_end_matches(['\n', '\r']).to_string())
}

/// Walk up from `base` looking for a `.claude-plugin/plugin.json` manifest.
pub fn find_plugin_root(base: &Path) -> Option<PathBuf> {
    let mut dir = base.to_path_buf();
    loop {
        if dir.join(".claude-plugin/plugin.json").is_file() || dir.join("plugin.json").is_file() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "ta-ext-command-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn renders_arguments_shell_and_file_references() {
        let tmp = TempDir::new();
        fs::write(tmp.0.join("note.txt"), "FILE-BODY").unwrap();
        let cmd = Command::parse_text(
            "---\ndescription: t\nallowed-tools: Bash(git status:*) Read\n---\nHi $1 / $ARGUMENTS\n!`printf hi`\n@note.txt\n",
            "hello",
            &tmp.0.join("commands/hello.md"),
        )
        .unwrap();
        assert_eq!(cmd.allowed_tools, vec!["Bash(git status:*)", "Read"]);
        let rendered = cmd.render("world", &tmp.0).unwrap();
        assert!(rendered.contains("Hi world / world"), "{rendered}");
        assert!(rendered.contains("hi"), "{rendered}");
        assert!(rendered.contains("FILE-BODY"), "{rendered}");
    }

    #[test]
    fn nested_command_names_use_colons() {
        let tmp = TempDir::new();
        let dir = tmp.0.join("commands");
        fs::create_dir_all(dir.join("git")).unwrap();
        fs::write(
            dir.join("git/commit.md"),
            "---\ndescription: commit\n---\nbody\n",
        )
        .unwrap();
        let loaded = load_commands(&dir, Some("demo")).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "demo:git:commit");
    }
}
