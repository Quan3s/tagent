//! `ta` command line surface: headless runs, the thin TUI, and management commands.

use crate::{
    config::{self, Config, ProviderConfig},
    core::{self, RunOptions},
    events::{Event, Role},
    ext::{HookRunner, Registry},
    keystore,
    perm::{self, Mode},
    provider::{self, DynProvider},
    redact,
    session::SessionStore,
};
use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use serde_json::{json, Value};
use std::{
    io::{self, IsTerminal, Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    str::FromStr,
    sync::{Arc, Mutex},
};

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum OutputFormat {
    Text,
    Json,
    StreamJson,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Start the local Web UI
    Serve {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 7777)]
        port: u16,
        #[arg(long)]
        token: Option<String>,
    },
    /// Manage BYOK credentials: auth add|ls|rm
    Auth { args: Vec<String> },
    /// Select models per role: model ls|set
    Model { args: Vec<String> },
    /// Claude Code plugins: plugin ls|add|rm|import|enable|disable
    Plugin { args: Vec<String> },
    /// Agent Skills: skill ls|add|info
    Skill { args: Vec<String> },
    /// Subagents: agent ls|run
    Agent { args: Vec<String> },
    /// MCP servers: mcp ls|add|rm
    Mcp { args: Vec<String> },
    /// Environment checks
    Doctor,
    /// Read/edit config: config get|set|edit
    Config { args: Vec<String> },
}

#[derive(Debug, Parser)]
#[command(
    name = "ta",
    version,
    bin_name = "ta",
    about = "Small BYOK coding agent for Termux"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Prompt for a one-shot run
    pub prompt: Vec<String>,

    #[arg(short = 'm', long, value_name = "provider/model")]
    pub model: Option<String>,
    #[arg(short = 'p', long, help = "Headless/print mode")]
    pub print: bool,
    #[arg(long, value_enum, default_value = "text")]
    pub output_format: OutputFormat,
    #[arg(long, default_value = "ask")]
    pub permission_mode: String,
    #[arg(long, visible_alias = "allowedTools", value_name = "Tool(pattern)")]
    pub allowed_tools: Vec<String>,
    #[arg(long, visible_alias = "disallowedTools", value_name = "Tool(pattern)")]
    pub disallowed_tools: Vec<String>,
    #[arg(long, value_name = "PATH")]
    pub add_dir: Vec<PathBuf>,
    #[arg(long, value_name = "PATH")]
    pub plugin_dir: Vec<PathBuf>,
    #[arg(long, value_name = "NAME")]
    pub agent: Option<String>,
    #[arg(long, value_name = "FILE")]
    pub mcp_config: Option<PathBuf>,
    #[arg(long)]
    pub system_prompt: Option<String>,
    #[arg(long)]
    pub append_system_prompt: Option<String>,
    #[arg(long)]
    pub max_turns: Option<usize>,
    #[arg(long, value_name = "USD")]
    pub budget: Option<f64>,
    #[arg(short = 'c', long = "continue")]
    pub continue_session: bool,
    #[arg(short = 'r', long, value_name = "ID")]
    pub resume: Option<String>,
    #[arg(long)]
    pub compact: bool,
    #[arg(long)]
    pub verbose: bool,
    #[arg(long)]
    pub debug: bool,
    #[arg(long)]
    pub no_color: bool,
}

impl Cli {
    pub async fn run(mut self) -> Result<ExitCode> {
        if let Some(command) = self.command.take() {
            return dispatch(&self, command).await;
        }
        let headless = self.print || !io::stdin().is_terminal();
        if !headless {
            crate::tui::run(&self).await?;
            return Ok(ExitCode::SUCCESS);
        }
        let mut prompt = self.prompt.join(" ");
        if prompt.trim().is_empty() {
            let mut buffer = String::new();
            io::stdin()
                .read_to_string(&mut buffer)
                .context("cannot read prompt from stdin")?;
            prompt = buffer;
        }
        if prompt.trim().is_empty() {
            bail!("no prompt provided (pass a prompt, pipe stdin, or use -p \"prompt\")");
        }
        let text = run_once(&self, prompt).await?;
        if matches!(self.output_format, OutputFormat::Text) && text.is_empty() {
            eprintln!("ta: model returned no text");
        }
        Ok(ExitCode::SUCCESS)
    }
}

/// Execute one agent turn and return the assistant's text according to the
/// selected output format. Shared by headless mode and the thin TUI.
pub async fn run_once(cli: &Cli, prompt: String) -> Result<String> {
    let config = Config::load()?;
    let spec = cli
        .model
        .clone()
        .or_else(|| config.roles.main.clone())
        .context("no model: pass -m provider/model or set [roles].main in the config")?;
    let (provider_name, provider_config) = {
        let full = if spec.contains('/') {
            spec.clone()
        } else {
            config.resolve(&spec).map(|_| spec.clone())?
        };
        let (name, _, pc) = config.resolve(&full)?;
        (name, pc)
    };
    let provider = build_provider(&provider_name, &provider_config)?;

    let workspace = std::env::current_dir().context("cannot determine workspace")?;
    let mut roots = vec![workspace.clone()];
    for dir in &cli.add_dir {
        roots.push(
            dir.canonicalize()
                .with_context(|| format!("cannot resolve --add-dir {}", dir.display()))?,
        );
    }
    for dir in &cli.plugin_dir {
        let _ = dir.canonicalize();
    }

    let allowed = cli
        .allowed_tools
        .iter()
        .chain(
            config
                .settings
                .permissions
                .get("allow")
                .into_iter()
                .flatten(),
        )
        .cloned()
        .collect::<Vec<_>>();
    let denied = cli
        .disallowed_tools
        .iter()
        .chain(
            config
                .settings
                .permissions
                .get("deny")
                .into_iter()
                .flatten(),
        )
        .cloned()
        .collect::<Vec<_>>();
    let permission = perm::Engine::new(Mode::from_str(&cli.permission_mode)?, allowed, denied)?;

    let session_id = if let Some(id) = &cli.resume {
        Some(id.clone())
    } else if cli.continue_session {
        Some(
            SessionStore::latest(config::sessions_dir()?)?
                .context("no session to continue")?
                .id,
        )
    } else {
        None
    };

    let key_envs: Vec<String> = config
        .providers
        .values()
        .map(|p| p.key_env_name().to_string())
        .collect();
    let secrets = redact::collect_secret_values(&key_envs);
    let events = Arc::new(Mutex::new(Vec::<Event>::new()));
    let events_for_sink = events.clone();
    let format = cli.output_format;
    let verbose = cli.verbose || cli.debug;
    let sink: Box<dyn FnMut(Event) + Send> = Box::new(move |event| {
        match format {
            OutputFormat::Text => print_text_event(&event),
            OutputFormat::Json => {
                if let Ok(mut collected) = events_for_sink.lock() {
                    collected.push(event.clone());
                }
            }
            OutputFormat::StreamJson => {
                if let Ok(line) = serde_json::to_string(&event) {
                    println!("{line}");
                }
            }
        }
        if verbose {
            eprintln!("[event] {}", event.kind());
        }
    });

    let registry = Registry::load(&workspace)?;
    let skill_prompt = registry
        .skills
        .iter()
        .map(|s| format!("- {}: {}", s.name, s.description))
        .collect::<Vec<_>>()
        .join("\n");
    let mut system_prompt = cli.system_prompt.clone();
    if let Some(agent_name) = &cli.agent {
        let agent = registry
            .agents
            .iter()
            .find(|a| &a.name == agent_name)
            .with_context(|| format!("unknown agent '{agent_name}'"))?;
        let extra = format!(
            "You are the '{name}' subagent.\n\n{body}",
            name = agent.name,
            body = agent.body
        );
        system_prompt = Some(match system_prompt {
            Some(existing) => format!("{existing}\n\n{extra}"),
            None => extra,
        });
    }
    let mut project_instructions = registry.project_instructions();
    if let Some(path) = &cli.mcp_config {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read --mcp-config {}", path.display()))?;
        project_instructions.push(format!("MCP config:\n{text}"));
    }

    let result = core::run(RunOptions {
        workspace,
        roots,
        prompt,
        model: spec,
        provider: DynProvider::from(provider),
        permission,
        max_turns: cli.max_turns.unwrap_or(core::DEFAULT_MAX_TURNS),
        system_prompt,
        append_system_prompt: cli.append_system_prompt.clone(),
        session: session_id,
        session_root: None,
        sink,
        secrets: secrets.clone(),
        secret_env_names: key_envs,
        permission_reply: None,
        interrupt: None,
        record_sink: None,
        pricing: config.settings.pricing.clone(),
        compact_threshold: if cli.compact {
            Some(1)
        } else {
            Some(config.settings.compact_threshold)
        },
        checkpoints: true,
        project_instructions,
        skill_prompt: Some(skill_prompt),
        allowed_tools: None,
        hooks: Some(Arc::new(HookRunner::new(registry.hooks.clone()))),
        extra_tools: Vec::new(),
        depth: 0,
    })
    .await?;

    if let Some(budget) = cli.budget {
        if result.cost_usd > budget {
            bail!("cost ${:.4} exceeds budget ${budget:.4}", result.cost_usd);
        }
    }
    if matches!(format, OutputFormat::Json) {
        let collected = events
            .lock()
            .map_err(|e| anyhow::anyhow!("event lock poisoned: {e}"))?;
        let text: String = collected
            .iter()
            .filter_map(|e| match e {
                Event::MessageDelta {
                    role: Role::Assistant,
                    text,
                } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        let summary = serde_json::json!({
            "session_id": result.session_id,
            "text": text,
            "turns": result.turns,
            "usage": {
                "input_tokens": result.usage.input_tokens,
                "output_tokens": result.usage.output_tokens,
                "cost_usd": result.cost_usd,
            },
            "events": collected.len(),
        });
        println!("{}", serde_json::to_string(&summary)?);
        return Ok(text);
    }
    Ok(result.final_text)
}

fn build_provider(name: &str, cfg: &ProviderConfig) -> Result<DynProvider> {
    provider::build(name, cfg)
}

fn print_text_event(event: &Event) {
    match event {
        Event::MessageDelta {
            role: Role::Assistant,
            text,
        } => {
            print!("{text}");
            let _ = io::stdout().flush();
        }
        Event::ThinkingDelta { text } => {
            if !text.trim().is_empty() {
                eprintln!("  [thinking] {}", text.trim());
            }
        }
        Event::ToolStart { name, args, .. } => {
            eprintln!(
                "  > {name} {}",
                serde_json::to_string(args).unwrap_or_default()
            );
        }
        Event::ToolResult {
            name,
            output,
            is_error,
            elapsed_ms,
            ..
        } => {
            let mark = if *is_error { "!" } else { "ok" };
            eprintln!(
                "  {mark} {name} ({elapsed_ms} ms): {}",
                output.lines().next().unwrap_or("")
            );
        }
        Event::PermissionRequest { tool, decision, .. } => {
            eprintln!("  permission {tool}: {decision}")
        }
        Event::Error { message } => eprintln!("ta: error: {message}"),
        Event::Done {
            session_id, reason, ..
        } => {
            eprintln!("\n[session {session_id}, {reason}]");
        }
        _ => {}
    }
}

async fn dispatch(cli: &Cli, command: Command) -> Result<ExitCode> {
    match command {
        Command::Serve { host, port, token } => {
            crate::serve::run(
                &host,
                port,
                token,
                std::env::current_dir().context("cannot determine workspace")?,
            )
            .await?;
        }
        Command::Auth { args } => auth(&args)?,
        Command::Model { args } => model(&args)?,
        Command::Plugin { args } => plugin(cli, &args)?,
        Command::Skill { args } => skill(cli, &args)?,
        Command::Agent { args } => agent(cli, &args).await?,
        Command::Mcp { args } => mcp(&args)?,
        Command::Doctor => doctor()?,
        Command::Config { args } => config_command(&args)?,
    }
    Ok(ExitCode::SUCCESS)
}

fn auth(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("add") => {
            let provider = args.get(1).context("usage: ta auth add <provider> [key]")?;
            let key = if let Some(key) = args.get(2) {
                key.clone()
            } else {
                eprint!("API key for {provider}: ");
                io::stderr().flush()?;
                let mut line = String::new();
                io::stdin().read_line(&mut line)?;
                line.trim().to_string()
            };
            keystore::set(provider, &key)?;
            eprintln!("ta: stored key for {provider} (value hidden)");
        }
        Some("ls") | None => {
            for name in ["anthropic", "openai", "openrouter", "gemini"] {
                let configured = Config::builtin(name)
                    .map(|p| p.configured(name))
                    .unwrap_or(false);
                println!(
                    "{name}\t{}",
                    if configured { "configured" } else { "missing" }
                );
            }
        }
        Some("rm") => {
            let provider = args.get(1).context("usage: ta auth rm <provider>")?;
            keystore::remove(provider)?;
            eprintln!("ta: removed key for {provider}");
        }
        Some(other) => bail!("unknown auth command '{other}' (use add|ls|rm)"),
    }
    Ok(())
}

fn model(args: &[String]) -> Result<()> {
    let mut config = Config::load()?;
    match args.first().map(String::as_str) {
        Some("ls") | None => {
            println!("main\t{}", config.roles.main.as_deref().unwrap_or("-"));
            println!("fast\t{}", config.roles.fast.as_deref().unwrap_or("-"));
            println!("plan\t{}", config.roles.plan.as_deref().unwrap_or("-"));
            println!(
                "subagent\t{}",
                config.roles.subagent.as_deref().unwrap_or("-")
            );
        }
        Some("set") => {
            let role = args
                .get(1)
                .context("usage: ta model set <role> <provider/model>")?;
            let value = args
                .get(2)
                .context("usage: ta model set <role> <provider/model>")?;
            config.set_role(role, value)?;
            config.save()?;
            eprintln!("ta: {role} = {value}");
        }
        Some(other) => bail!("unknown model command '{other}' (use ls|set)"),
    }
    Ok(())
}

fn plugin(cli: &Cli, args: &[String]) -> Result<()> {
    let workspace = std::env::current_dir()?;
    let registry = Registry::load(&workspace)?;
    match args.first().map(String::as_str) {
        Some("ls") | None => {
            for p in &registry.plugins {
                println!(
                    "{}\t{}\tmissing: {:?}",
                    p.name,
                    p.description.as_deref().unwrap_or(""),
                    p.missing_requirements()
                );
            }
            if registry.plugins.is_empty() {
                eprintln!("ta: no plugins found in .claude/, ~/.claude/ or data dir");
            }
        }
        Some("add") => {
            let source = args.get(1).context("usage: ta plugin add <repo|path>")?;
            install_plugin(source)?;
        }
        Some("import") => {
            let path = args.get(1).context("usage: ta plugin import <path>")?;
            let imported = Registry::load(Path::new(path))?;
            println!(
                "mapped: {} commands, {} skills, {} agents, {} hooks, {} MCP servers",
                imported.commands.len(),
                imported.skills.len(),
                imported.agents.len(),
                imported.hooks.len(),
                imported.mcp_servers.len()
            );
        }
        Some("enable") | Some("disable") => {
            let name = args
                .get(1)
                .context("usage: ta plugin enable|disable <name>")?;
            let enabled = args[0] == "enable";
            crate::serve::set_plugin_state_cli(name, enabled)?;
            eprintln!(
                "ta: plugin {name} {}",
                if enabled { "enabled" } else { "disabled" }
            );
        }
        Some("rm") => {
            let name = args.get(1).context("usage: ta plugin rm <name>")?;
            let dir = config::plugins_dir()?.join(name);
            if dir.exists() {
                std::fs::remove_dir_all(dir)?;
                eprintln!("ta: removed plugin {name}");
            } else {
                bail!("plugin '{name}' is not installed in the data directory");
            }
        }
        Some("update") => {
            eprintln!("ta: plugin update is not implemented; remove and add the plugin again");
        }
        Some(other) => bail!("unknown plugin command '{other}'"),
    }
    let _ = cli;
    Ok(())
}

fn install_plugin(source: &str) -> Result<()> {
    let dir = config::plugins_dir()?;
    std::fs::create_dir_all(&dir)?;
    if let Ok(path) = Path::new(source).canonicalize() {
        let name = path
            .file_name()
            .context("plugin path has no name")?
            .to_string_lossy();
        let target = dir.join(name.as_ref());
        if target.exists() {
            bail!("plugin already installed");
        }
        copy_dir(&path, &target)?;
        eprintln!("ta: installed plugin {name}");
        return Ok(());
    }
    let status = std::process::Command::new("git")
        .args(["clone", "--depth", "1", source])
        .current_dir(&dir)
        .status()?;
    if !status.success() {
        bail!("git clone failed for {source}");
    }
    eprintln!("ta: cloned plugin from {source}");
    Ok(())
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn skill(_cli: &Cli, args: &[String]) -> Result<()> {
    let registry = Registry::load(&std::env::current_dir()?)?;
    match args.first().map(String::as_str) {
        Some("ls") | None => {
            for s in &registry.skills {
                println!("{}\t{}", s.name, s.description);
            }
        }
        Some("info") => {
            let name = args.get(1).context("usage: ta skill info <name>")?;
            let loaded = registry.load_skill(name)?;
            println!("# {}\n\n{}", loaded.name, loaded.body);
        }
        Some("add") => {
            let path = args
                .get(1)
                .context("usage: ta skill add <path-to-SKILL.md>")?;
            let source = Path::new(path).canonicalize()?;
            let name = source
                .parent()
                .and_then(Path::file_name)
                .context("skill path has no directory name")?;
            let dir = config::data_dir()?.join("tagent/skills").join(name);
            std::fs::create_dir_all(&dir)?;
            std::fs::copy(&source, dir.join("SKILL.md"))?;
            eprintln!("ta: installed skill {}", name.to_string_lossy());
        }
        Some(other) => bail!("unknown skill command '{other}' (use ls|add|info)"),
    }
    Ok(())
}

async fn agent(cli: &Cli, args: &[String]) -> Result<()> {
    let registry = Registry::load(&std::env::current_dir()?)?;
    match args.first().map(String::as_str) {
        Some("ls") | None => {
            for a in &registry.agents {
                println!("{}\t{}", a.name, a.description);
            }
        }
        Some("run") => {
            let name = args.get(1).context("usage: ta agent run <name> <prompt>")?;
            if !registry.agents.iter().any(|a| &a.name == name) {
                bail!("unknown agent '{name}'");
            }
            let agent_body = registry
                .agents
                .iter()
                .find(|a| &a.name == name)
                .map(|a| a.body.clone())
                .unwrap_or_default();
            let prompt = format!(
                "[agent:{name}] {body}\n\n{task}",
                body = agent_body,
                task = args[2..].join(" ")
            );
            if args[2..].join(" ").trim().is_empty() {
                bail!("agent run needs a prompt");
            }
            run_once(cli, prompt).await?;
        }
        Some(other) => bail!("unknown agent command '{other}' (use ls|run)"),
    }
    Ok(())
}

fn mcp(args: &[String]) -> Result<()> {
    let path = std::env::current_dir()?.join(".mcp.json");
    let mut config: Value = if path.exists() {
        serde_json::from_str(&std::fs::read_to_string(&path)?)?
    } else {
        json!({"mcpServers": {}})
    };
    let servers = config
        .as_object_mut()
        .context(".mcp.json is not an object")?
        .entry("mcpServers")
        .or_insert_with(|| json!({}));
    match args.first().map(String::as_str) {
        Some("ls") | None => {
            for (name, value) in servers.as_object().context("mcpServers is not an object")? {
                println!("{name}\t{}", value);
            }
        }
        Some("add") => {
            let name = args
                .get(1)
                .context("usage: ta mcp add <name> <command> [args...]")?;
            let command = args
                .get(2)
                .context("usage: ta mcp add <name> <command> [args...]")?;
            let extra: Vec<String> = args[3..].to_vec();
            servers[name] = json!({"command": command, "args": extra});
            std::fs::write(&path, serde_json::to_string_pretty(&config)?)?;
            eprintln!("ta: added MCP server {name}");
        }
        Some("rm") => {
            let name = args.get(1).context("usage: ta mcp rm <name>")?;
            if let Some(map) = servers.as_object_mut() {
                map.remove(name);
            }
            std::fs::write(&path, serde_json::to_string_pretty(&config)?)?;
            eprintln!("ta: removed MCP server {name}");
        }
        Some(other) => bail!("unknown mcp command '{other}' (use ls|add|rm)"),
    }
    Ok(())
}

fn doctor() -> Result<()> {
    let binaries = [
        ("git", "git"),
        ("ripgrep", "rg"),
        ("node", "node"),
        ("python3", "python3"),
        ("termux-api", "termux-battery-status"),
    ];
    let home = config::home()?;
    let prefix = std::env::var_os("PREFIX").map(PathBuf::from);
    for (label, bin) in binaries {
        let found = which(bin, prefix.as_deref()).is_some();
        println!("{}\t{}", label, if found { "ok" } else { "missing" });
    }
    let sessions = config::sessions_dir()?;
    let storage_ok = std::fs::create_dir_all(&sessions).is_ok();
    println!("storage\t{}", if storage_ok { "ok" } else { "missing" });
    println!("home\t{}", home.display());
    let registry = Registry::load(&std::env::current_dir()?)?;
    for (name, missing) in registry.missing_requirements() {
        println!("plugin-requires\t{name}\tmissing: {}", missing.join(", "));
    }
    Ok(())
}

fn which(bin: &str, prefix: Option<&Path>) -> Option<PathBuf> {
    if let Some(prefix) = prefix {
        let candidate = prefix.join("bin").join(bin);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(bin);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn config_command(args: &[String]) -> Result<()> {
    let mut config = Config::load()?;
    match args.first().map(String::as_str) {
        Some("get") => {
            let key = args.get(1).context("usage: ta config get <key>")?;
            println!("{}", config.get(key)?);
        }
        Some("set") => {
            let key = args.get(1).context("usage: ta config set <key> <value>")?;
            let value = args.get(2).context("usage: ta config set <key> <value>")?;
            config.set(key, value)?;
            config.save()?;
            eprintln!("ta: {key} = {value}");
        }
        Some("edit") => {
            let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".into());
            let path = config::config_path()?;
            std::fs::create_dir_all(path.parent().context("missing config parent")?)?;
            let status = std::process::Command::new(editor).arg(&path).status()?;
            if !status.success() {
                bail!("editor exited with an error");
            }
        }
        Some(other) => bail!("unknown config command '{other}' (use get|set|edit)"),
        None => bail!("usage: ta config get|set|edit …"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_claude_compatible_flags() {
        let cli = Cli::try_parse_from([
            "ta",
            "-p",
            "-m",
            "anthropic/example",
            "--permission-mode",
            "auto-edit",
            "--allowedTools",
            "Bash(git:*)",
            "--max-turns",
            "3",
            "hello",
            "world",
        ])
        .unwrap();
        assert!(cli.print);
        assert_eq!(cli.prompt.join(" "), "hello world");
        assert_eq!(cli.permission_mode, "auto-edit");
        assert_eq!(cli.max_turns, Some(3));
        assert_eq!(cli.allowed_tools, vec!["Bash(git:*)"]);
    }

    #[test]
    fn parses_output_formats() {
        let cli =
            Cli::try_parse_from(["ta", "-p", "--output-format", "stream-json", "hi"]).unwrap();
        assert!(matches!(cli.output_format, OutputFormat::StreamJson));
    }
}
