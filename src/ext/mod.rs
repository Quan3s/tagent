//! Extension compatibility layer for the `ta` coding agent.
//!
//! Loads Claude Code plugins, commands, skills, agents, hooks and MCP servers,
//! plus read-only adapters for other agents' instruction files. Nothing is
//! executed at load time; hooks and plugin scripts are opt-in at call time.

pub mod adapters;
pub mod agents;
pub mod commands;
pub mod frontmatter;
pub mod hooks;
pub mod mcp;
pub mod plugins;
pub mod skills;

pub use adapters::discover_instruction_files;
pub use adapters::project_instructions;
pub use adapters::project_instructions as read_project_instructions;
pub use agents::AgentMeta;
pub use commands::Command;
pub use frontmatter::Frontmatter;
pub use hooks::{Hook, HookEvent, HookOutcome, HookRunner};
pub use mcp::{HttpMcpClient, McpClient, McpServer, McpTransport, StdioMcpClient};
pub use plugins::{Marketplace, MarketplacePlugin, Plugin};
pub use skills::{LoadedSkill, SkillMeta};

use anyhow::{anyhow, Result};
use std::{
    collections::HashSet,
    env,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
pub struct Registry {
    pub plugins: Vec<Plugin>,
    pub commands: Vec<Command>,
    pub skills: Vec<SkillMeta>,
    pub agents: Vec<AgentMeta>,
    pub hooks: Vec<Hook>,
    pub mcp_servers: Vec<McpServer>,
    /// Marketplaces found next to plugins or at project/user level.
    pub marketplaces: Vec<Marketplace>,
    /// Root used to resolve `@file` references and instruction adapters.
    pub workspace: PathBuf,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            plugins: Vec::new(),
            commands: Vec::new(),
            skills: Vec::new(),
            agents: Vec::new(),
            hooks: Vec::new(),
            mcp_servers: Vec::new(),
            marketplaces: Vec::new(),
            workspace: PathBuf::new(),
        }
    }
}

#[derive(Default)]
struct Seen {
    plugins: HashSet<String>,
    commands: HashSet<String>,
    skills: HashSet<String>,
    agents: HashSet<String>,
    hooks: HashSet<(HookEvent, String)>,
    mcp_servers: HashSet<String>,
    marketplaces: HashSet<PathBuf>,
    plugin_dirs: HashSet<PathBuf>,
}

impl Registry {
    /// Scan workspace `.claude/`, `$HOME/.claude/`, `<workspace>/.ta/plugins/`
    /// and `$XDG_DATA_HOME/tagent/plugins/`. Missing directories are ignored.
    pub fn load(workspace: &Path) -> Result<Self> {
        let home = env::var_os("HOME").map(PathBuf::from);
        let data_home = env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| home.as_ref().map(|h| h.join(".local/share")));
        Self::load_with_roots(workspace, home.as_deref(), data_home.as_deref())
    }

    /// Explicit-root variant of [`Registry::load`], useful for embedding and
    /// hermetic tests.
    pub fn load_with_roots(
        workspace: &Path,
        home: Option<&Path>,
        data_home: Option<&Path>,
    ) -> Result<Self> {
        let mut registry = Self {
            workspace: workspace.to_path_buf(),
            ..Self::default()
        };
        let mut seen = Seen::default();

        load_root(&mut registry, &workspace.join(".claude"), None, &mut seen);
        load_mcp_file(&mut registry, &workspace.join(".mcp.json"), &mut seen);
        load_marketplace(
            &mut registry,
            &workspace.join(".claude-plugin/marketplace.json"),
            &mut seen,
        );

        if let Some(home) = home {
            load_root(&mut registry, &home.join(".claude"), None, &mut seen);
            load_marketplace(
                &mut registry,
                &home.join(".claude-plugin/marketplace.json"),
                &mut seen,
            );
        }

        let mut plugin_dirs = plugins::find_plugin_dirs(&workspace.join(".ta/plugins"), 3);
        if let Some(data_home) = data_home {
            plugin_dirs.extend(plugins::find_plugin_dirs(
                &data_home.join("tagent/plugins"),
                3,
            ));
        }
        for dir in plugin_dirs {
            if !seen.plugin_dirs.insert(dir.clone()) {
                continue;
            }
            load_plugin(&mut registry, &dir, &mut seen);
        }
        Ok(registry)
    }

    /// Read the full body of a skill discovered at load time.
    pub fn load_skill(&self, name: &str) -> Result<LoadedSkill> {
        let meta = self
            .skills
            .iter()
            .find(|skill| skill.name == name || skill.name.rsplit(':').next() == Some(name))
            .ok_or_else(|| anyhow!("unknown skill '{}'", name))?;
        LoadedSkill::read(&meta.path)
    }

    /// Render a slash command template with the given argument string.
    pub fn render_command(&self, name: &str, args: &str) -> Result<String> {
        let name = name.trim_start_matches('/');
        let command = self
            .commands
            .iter()
            .find(|command| command.name == name)
            .or_else(|| {
                self.commands
                    .iter()
                    .find(|command| command.name.rsplit(':').next() == Some(name))
            })
            .ok_or_else(|| anyhow!("unknown command '{}'", name))?;
        command.render(args, &self.workspace)
    }

    /// Contents of recognized `AGENTS.md`, `GEMINI.md`, Cursor, Cline,
    /// Copilot, OpenCode and Gemini instruction files. Never executed.
    pub fn project_instructions(&self) -> Vec<String> {
        adapters::project_instructions(&self.workspace)
    }

    pub fn enabled_mcp_servers(&self) -> Vec<McpServer> {
        self.mcp_servers
            .iter()
            .filter(|server| server.enabled)
            .cloned()
            .collect()
    }

    /// Plugins whose declared `requires` binaries are missing from `PATH`.
    pub fn missing_requirements(&self) -> Vec<(String, Vec<String>)> {
        self.plugins
            .iter()
            .filter_map(|plugin| {
                let missing = plugin.missing_requirements();
                if missing.is_empty() {
                    None
                } else {
                    Some((plugin.name.clone(), missing))
                }
            })
            .collect()
    }
}

/// `mcp__server__tool` for permission rules and tool registration.
pub fn mcp_tool_name(server: &str, tool: &str) -> String {
    format!("mcp__{}__{}", sanitize(server), sanitize(tool))
}

/// `plugin__tool` for plugin script tools. The plugin name is intentionally
/// not part of the namespace (per the compatibility plan).
pub fn plugin_tool_name(plugin: &str, tool: &str) -> String {
    let _ = plugin;
    format!("plugin__{}", sanitize(tool))
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn load_root(registry: &mut Registry, root: &Path, prefix: Option<&str>, seen: &mut Seen) {
    if !root.is_dir() {
        return;
    }
    for command in commands::load_commands(&root.join("commands"), prefix).unwrap_or_default() {
        if seen.commands.insert(command.name.clone()) {
            registry.commands.push(command);
        }
    }
    for agent in agents::load_agents(&root.join("agents")).unwrap_or_default() {
        if seen.agents.insert(agent.name.clone()) {
            registry.agents.push(agent);
        }
    }
    for skill in skills::load_skills(&root.join("skills")).unwrap_or_default() {
        if seen.skills.insert(skill.name.clone()) {
            registry.skills.push(skill);
        }
    }
    for path in [root.join("hooks/hooks.json"), root.join("settings.json")] {
        let parsed = if path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n == "settings.json")
            .unwrap_or(false)
        {
            hooks::hooks_from_settings(&path)
        } else {
            hooks::load_hooks(&path)
        };
        for hook in parsed.unwrap_or_default() {
            if seen.hooks.insert((hook.event, hook.command.clone())) {
                registry.hooks.push(hook);
            }
        }
    }
    load_mcp_file(registry, &root.join(".mcp.json"), seen);
}

fn load_plugin(registry: &mut Registry, dir: &Path, seen: &mut Seen) {
    let Ok(plugin) = Plugin::load_dir(dir) else {
        return;
    };
    if !seen.plugins.insert(plugin.name.clone()) {
        return;
    }
    let prefix = plugin.name.clone();
    load_root(registry, dir, Some(&prefix), seen);

    for path in plugin.manifest_paths("commands") {
        if path.is_dir() {
            for command in commands::load_commands(&path, Some(&prefix)).unwrap_or_default() {
                push_command(registry, command, seen);
            }
        } else if is_markdown(&path) {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("command");
            if let Ok(command) = commands::Command::parse(&path, format!("{}:{}", prefix, stem)) {
                push_command(registry, command, seen);
            }
        }
    }
    for path in plugin.manifest_paths("agents") {
        if path.is_dir() {
            for agent in agents::load_agents(&path).unwrap_or_default() {
                if seen.agents.insert(agent.name.clone()) {
                    registry.agents.push(agent);
                }
            }
        } else if is_markdown(&path) {
            if let Ok(agent) = agents::AgentMeta::parse(&path) {
                if seen.agents.insert(agent.name.clone()) {
                    registry.agents.push(agent);
                }
            }
        }
    }
    for path in plugin.manifest_paths("skills") {
        if path.join("SKILL.md").is_file() || path.is_dir() {
            for skill in skills::load_skills(&path).unwrap_or_default() {
                if seen.skills.insert(skill.name.clone()) {
                    registry.skills.push(skill);
                }
            }
        } else if path.file_name().and_then(|n| n.to_str()) == Some("SKILL.md") {
            if let Ok(loaded) = LoadedSkill::read(&path) {
                let meta = SkillMeta {
                    name: loaded.name,
                    description: loaded.description,
                    path: loaded.path,
                };
                if seen.skills.insert(meta.name.clone()) {
                    registry.skills.push(meta);
                }
            }
        }
    }
    for path in plugin.manifest_paths("hooks") {
        let file = if path.is_dir() {
            path.join("hooks.json")
        } else {
            path
        };
        for hook in hooks::load_hooks(&file).unwrap_or_default() {
            if seen.hooks.insert((hook.event, hook.command.clone())) {
                registry.hooks.push(hook);
            }
        }
    }
    if let Some(servers) = plugin.extra.get("mcpServers") {
        if let Ok(servers) = mcp::parse_servers_map(servers) {
            push_servers(registry, servers, seen);
        }
    }
    load_mcp_file(registry, &dir.join(".mcp.json"), seen);

    load_marketplace(registry, &dir.join(".claude-plugin/marketplace.json"), seen);
    load_marketplace(registry, &dir.join("marketplace.json"), seen);
    registry.plugins.push(plugin);
}

fn push_command(registry: &mut Registry, command: Command, seen: &mut Seen) {
    if seen.commands.insert(command.name.clone()) {
        registry.commands.push(command);
    }
}

fn load_mcp_file(registry: &mut Registry, path: &Path, seen: &mut Seen) {
    if let Ok(servers) = mcp::load_mcp_file(path) {
        push_servers(registry, servers, seen);
    }
}

fn push_servers(registry: &mut Registry, servers: Vec<McpServer>, seen: &mut Seen) {
    for server in servers {
        if seen.mcp_servers.insert(server.name.clone()) {
            registry.mcp_servers.push(server);
        }
    }
}

fn load_marketplace(registry: &mut Registry, path: &Path, seen: &mut Seen) {
    if !path.is_file() {
        return;
    }
    if let Ok(marketplace) = Marketplace::load(path) {
        if seen.marketplaces.insert(marketplace.source.clone()) {
            registry.marketplaces.push(marketplace);
        }
    }
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("md"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(label: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("ta-ext-{}-{}", label, std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write(path: &Path, text: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, text).unwrap();
    }

    #[test]
    fn loads_workspace_home_and_plugins() {
        let tmp = TempDir::new("registry");
        let ws = tmp.0.join("workspace");
        let home = tmp.0.join("home");
        let data = tmp.0.join("data");
        fs::create_dir_all(&ws).unwrap();

        write(
            &ws.join(".claude/commands/hello.md"),
            "---\ndescription: Greet\nallowed-tools: Bash(git status:*) Read\nargument-hint: <who>\n---\nHi $1 / $ARGUMENTS\n!`printf shell`\n@note.txt\n",
        );
        write(&ws.join(".claude/commands/note.txt"), "FILE-BODY");
        write(
            &ws.join(".claude/skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: Demo skill\n---\nFull instructions.\n",
        );
        write(
            &ws.join(".claude/agents/reviewer.md"),
            "---\ndescription: Reviews\ntools: Read\n---\nReview.\n",
        );
        write(
            &ws.join(".claude/hooks/hooks.json"),
            r#"{"UserPromptSubmit":[{"hooks":[{"command":"cat","description":"observe"}]}]}"#,
        );
        write(
            &ws.join(".mcp.json"),
            r#"{"mcpServers":{"local":{"command":"cat","args":[]}}}"#,
        );
        write(
            &ws.join(".claude-plugin/marketplace.json"),
            r#"{"name":"market","plugins":[{"name":"demo","source":"./demo"}]}"#,
        );
        write(&ws.join("AGENTS.md"), "workspace agents");
        write(&ws.join(".clinerules"), "cline rules");
        write(
            &ws.join(".ta/plugins/demo/.claude-plugin/plugin.json"),
            r#"{"name":"demo","description":"Demo plugin"}"#,
        );
        write(
            &ws.join(".ta/plugins/demo/commands/greet.md"),
            "---\ndescription: Plugin greet\n---\nHello!\n",
        );
        write(
            &home.join(".claude/commands/home.md"),
            "---\ndescription: Home command\n---\nHome.\n",
        );
        write(
            &data.join("tagent/plugins/ext/plugin.json"),
            r#"{"name":"ext"}"#,
        );

        let registry = Registry::load_with_roots(&ws, Some(&home), Some(&data)).unwrap();
        assert!(registry.commands.iter().any(|c| c.name == "hello"));
        assert!(registry.commands.iter().any(|c| c.name == "home"));
        assert!(registry.commands.iter().any(|c| c.name == "demo:greet"));
        assert_eq!(registry.plugins.len(), 2);
        assert_eq!(registry.skills.len(), 1);
        assert_eq!(registry.agents.len(), 1);
        assert_eq!(registry.hooks.len(), 1);
        assert_eq!(registry.hooks[0].event, HookEvent::UserPromptSubmit);
        assert_eq!(registry.mcp_servers.len(), 1);
        assert_eq!(registry.marketplaces.len(), 1);

        let skill = registry.load_skill("demo").unwrap();
        assert!(skill.body.contains("Full instructions."));
        assert_eq!(skill.path, ws.join(".claude/skills/demo"));

        let rendered = registry.render_command("hello", "Ada").unwrap();
        assert!(rendered.contains("Hi Ada / Ada"), "{rendered}");
        assert!(rendered.contains("shell"), "{rendered}");
        assert!(rendered.contains("FILE-BODY"), "{rendered}");

        let instructions = registry.project_instructions().join("\n");
        assert!(instructions.contains("workspace agents"));
        assert!(instructions.contains("cline rules"));

        assert_eq!(mcp_tool_name("srv", "read file"), "mcp__srv__read_file");
        assert_eq!(plugin_tool_name("demo", "lint"), "plugin__lint");
    }
}
