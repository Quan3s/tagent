//! `ta` command line surface. M0 implements headless `-p`; the remaining
//! subcommands are parsed but report the milestone that implements them.

use crate::{
    config::{self, Config},
    core::{self, RunOptions},
    events::{Event, Role},
    perm::{self, Mode},
    provider::anthropic::Anthropic,
    redact,
    session::SessionStore,
};
use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use std::{
    io::{self, IsTerminal, Read, Write},
    path::PathBuf,
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
    /// Start the local Web UI (M1)
    Serve {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 7777)]
        port: u16,
        #[arg(long)]
        token: Option<String>,
    },
    /// Manage BYOK credentials (M3)
    Auth { args: Vec<String> },
    /// Select models per role (M3)
    Model { args: Vec<String> },
    /// Claude Code plugins (M4)
    Plugin { args: Vec<String> },
    /// Agent Skills (M4)
    Skill { args: Vec<String> },
    /// Subagents (M4/M5)
    Agent { args: Vec<String> },
    /// MCP servers (M5)
    Mcp { args: Vec<String> },
    /// Environment checks (M6)
    Doctor,
    /// Read/edit config (M3)
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
    pub async fn run(self) -> Result<ExitCode> {
        if let Some(command) = &self.command {
            return stub(command);
        }

        let headless = self.print || !io::stdin().is_terminal();
        if !headless {
            eprintln!("ta: the interactive TUI is M6; use `ta -p \"prompt\"` for now");
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
        if self.budget.is_some() {
            eprintln!("ta: --budget is accepted but cost tracking lands in M3");
        }
        if self.compact {
            eprintln!("ta: --compact is accepted but context compaction lands in M3");
        }

        let config = Config::load()?;
        let spec = self
            .model
            .clone()
            .or_else(|| config.roles.main.clone())
            .context("no model: pass -m provider/model or set [roles].main in the config")?;
        let (_, _, provider_config) = config.resolve(&spec)?;
        let key_env = provider_config.key_env_name().to_string();
        let key = std::env::var(&key_env)
            .with_context(|| format!("set {key_env} in your environment before running ta"))?;
        let provider = Anthropic::new(key, provider_config.base_url())?;

        let workspace = std::env::current_dir().context("cannot determine workspace")?;
        let mut roots = vec![workspace.clone()];
        for dir in &self.add_dir {
            roots.push(
                dir.canonicalize()
                    .with_context(|| format!("cannot resolve --add-dir {}", dir.display()))?,
            );
        }

        let permission = perm::Engine::new(
            Mode::from_str(&self.permission_mode)?,
            self.allowed_tools.clone(),
            self.disallowed_tools.clone(),
        )?;

        let session_id = if let Some(id) = &self.resume {
            Some(id.clone())
        } else if self.continue_session {
            let dir = config::sessions_dir()?;
            Some(
                SessionStore::latest(dir)?
                    .context("no session to continue")?
                    .id,
            )
        } else {
            None
        };

        let secrets = redact::collect_secret_values(&[key_env.clone()]);
        let events = Arc::new(Mutex::new(Vec::<Event>::new()));
        let events_for_sink = events.clone();
        let format = self.output_format;
        let verbose = self.verbose || self.debug;
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

        let result = core::run(RunOptions {
            workspace,
            roots,
            prompt,
            model: spec,
            provider: core::shared(provider),
            permission,
            max_turns: self.max_turns.unwrap_or(core::DEFAULT_MAX_TURNS),
            system_prompt: self.system_prompt,
            append_system_prompt: self.append_system_prompt,
            session: session_id,
            session_root: None,
            sink,
            secrets,
            secret_env_names: vec![key_env],
        })
        .await?;

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
                },
                "events": collected.len(),
            });
            println!("{}", serde_json::to_string(&summary)?);
        }
        Ok(ExitCode::SUCCESS)
    }
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
        Event::ToolStart { name, args, .. } => {
            eprintln!(
                "  · {name} {}",
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
            let mark = if *is_error { "!" } else { "✓" };
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

fn stub(command: &Command) -> Result<ExitCode> {
    let (name, milestone) = match command {
        Command::Serve { .. } => ("serve", "M1"),
        Command::Auth { .. } => ("auth", "M3"),
        Command::Model { .. } => ("model", "M3"),
        Command::Plugin { .. } => ("plugin", "M4"),
        Command::Skill { .. } => ("skill", "M4"),
        Command::Agent { .. } => ("agent", "M4/M5"),
        Command::Mcp { .. } => ("mcp", "M5"),
        Command::Doctor => ("doctor", "M6"),
        Command::Config { .. } => ("config", "M3"),
    };
    eprintln!("ta: `{name}` is implemented in {milestone}; this build is M0 (headless -p)");
    Ok(ExitCode::SUCCESS)
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
