//! Thin ratatui client. The daemon and Web UI are the main surface; this is a
//! small terminal client that reuses the same agent loop via `cli::run_once`.

use crate::cli::Cli;
use anyhow::Result;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph, Wrap},
    Terminal,
};
use std::io::{self, Write};

pub async fn run(cli: &Cli) -> Result<()> {
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut history: Vec<String> = vec![
        "ta — local coding agent".into(),
        "Type a prompt, :q to quit. Web UI: ta serve".into(),
        String::new(),
    ];
    let mut draft = cli.prompt.join(" ");

    let result = loop {
        terminal.draw(|frame| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(3), Constraint::Length(3)])
                .split(frame.area());
            let body = history
                .iter()
                .map(|line| Line::from(line.clone()))
                .collect::<Vec<_>>();
            frame.render_widget(
                Paragraph::new(body)
                    .wrap(Wrap { trim: false })
                    .block(Block::default().borders(Borders::ALL).title("session")),
                chunks[0],
            );
            frame.render_widget(
                Paragraph::new(draft.clone())
                    .style(
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    )
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("prompt (stdin)"),
                    ),
                chunks[1],
            );
        })?;

        print!("ta> ");
        io::stdout().flush()?;
        let mut line = String::new();
        if io::stdin().read_line(&mut line)? == 0 {
            break Ok(());
        }
        let prompt = if draft.trim().is_empty() {
            line.trim().to_string()
        } else {
            let combined = format!("{} {}", draft.trim(), line.trim());
            draft.clear();
            combined
        };
        if prompt == ":q" || prompt == "/quit" {
            break Ok(());
        }
        if prompt.is_empty() {
            continue;
        }
        history.push(format!("> {prompt}"));
        match crate::cli::run_once(cli, prompt).await {
            Ok(text) => {
                for line in text.lines() {
                    history.push(line.to_string());
                }
            }
            Err(e) => history.push(format!("error: {e:#}")),
        }
        history.push(String::new());
    };

    let mut stdout = io::stdout();
    execute!(stdout, LeaveAlternateScreen)?;
    disable_raw_mode().ok();
    result
}
