//! Permission engine. Session rules beat project rules, which beat user rules.
//! M0 loads session rules from CLI flags; the file-based tiers arrive with
//! config/settings in M3.

use anyhow::{bail, Result};
use std::{fmt, str::FromStr};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Ask,
    AutoEdit,
    Plan,
    Yolo,
}

impl FromStr for Mode {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "ask" => Ok(Self::Ask),
            "auto-edit" => Ok(Self::AutoEdit),
            "plan" => Ok(Self::Plan),
            "yolo" => Ok(Self::Yolo),
            other => bail!("unknown permission mode '{other}' (use ask|auto-edit|plan|yolo)"),
        }
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Ask => "ask",
            Self::AutoEdit => "auto-edit",
            Self::Plan => "plan",
            Self::Yolo => "yolo",
        })
    }
}

#[derive(Clone, Debug)]
pub enum Rule {
    /// Whole tool name, e.g. `read`.
    Tool(String),
    /// `Tool(pattern)`, e.g. `Bash(git:*)`, `Edit(src/**)`.
    Scoped { tool: String, pattern: String },
}

impl Rule {
    pub fn parse(input: &str) -> Result<Self> {
        let input = input.trim();
        if let Some((tool, rest)) = input.split_once('(') {
            if !rest.ends_with(')') {
                bail!("invalid permission rule '{input}' (missing ')')");
            }
            let pattern = &rest[..rest.len() - 1];
            if tool.is_empty() || pattern.is_empty() {
                bail!("invalid permission rule '{input}'");
            }
            Ok(Self::Scoped {
                tool: tool.to_string(),
                pattern: pattern.to_string(),
            })
        } else if input.is_empty() {
            bail!("empty permission rule")
        } else {
            Ok(Self::Tool(input.to_string()))
        }
    }

    fn matches(&self, tool: &str, target: &str) -> bool {
        match self {
            Self::Tool(name) => name.eq_ignore_ascii_case(tool),
            Self::Scoped {
                tool: name,
                pattern,
            } => name.eq_ignore_ascii_case(tool) && wildcard(pattern, target),
        }
    }
}

#[derive(Clone, Copy)]
enum GlobToken {
    /// Any run of characters except `/`.
    Star,
    /// Any run of characters, including `/`.
    DoubleStar,
    /// Literal character.
    Char(char),
    /// Claude-style command prefix separator: `:` or one whitespace character.
    Colon,
}

fn tokenize_glob(pattern: &str) -> Vec<GlobToken> {
    let mut tokens = Vec::new();
    let mut chars = pattern.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '*' => {
                if chars.peek() == Some(&'*') {
                    chars.next();
                    tokens.push(GlobToken::DoubleStar);
                } else {
                    tokens.push(GlobToken::Star);
                }
            }
            ':' => tokens.push(GlobToken::Colon),
            other => tokens.push(GlobToken::Char(other)),
        }
    }
    tokens
}

/// Glob matcher for permission scopes. `*` does not cross `/`; `**` does.
pub fn wildcard(pattern: &str, text: &str) -> bool {
    let tokens = tokenize_glob(pattern);
    let t: Vec<char> = text.chars().collect();
    let mut prev = vec![false; t.len() + 1];
    prev[0] = true;

    for token in tokens {
        let mut next = vec![false; t.len() + 1];
        match token {
            GlobToken::Star => {
                next[0] = prev[0];
                for i in 1..=t.len() {
                    next[i] = prev[i] || (t[i - 1] != '/' && next[i - 1]);
                }
            }
            GlobToken::DoubleStar => {
                next[0] = prev[0];
                for i in 1..=t.len() {
                    next[i] = prev[i] || next[i - 1];
                }
            }
            GlobToken::Char(ch) => {
                for i in 1..=t.len() {
                    next[i] = prev[i - 1] && t[i - 1] == ch;
                }
            }
            GlobToken::Colon => {
                for i in 1..=t.len() {
                    let matched = t[i - 1] == ':' || t[i - 1].is_whitespace();
                    next[i] = prev[i - 1] && matched;
                }
            }
        }
        prev = next;
    }
    prev[t.len()]
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    /// Headless M0 cannot prompt: an Ask becomes a denied call with this reason.
    Ask,
    Deny(String),
}

#[derive(Clone, Debug)]
pub struct Engine {
    pub mode: Mode,
    pub session_allow: Vec<Rule>,
    pub session_deny: Vec<Rule>,
    // Project/user tiers are intentionally empty until M3. Keeping the fields
    // makes the precedence explicit and testable.
    pub project_allow: Vec<Rule>,
    pub project_deny: Vec<Rule>,
    pub user_allow: Vec<Rule>,
    pub user_deny: Vec<Rule>,
}

impl Default for Engine {
    fn default() -> Self {
        Self {
            mode: Mode::Ask,
            session_allow: Vec::new(),
            session_deny: Vec::new(),
            project_allow: Vec::new(),
            project_deny: Vec::new(),
            user_allow: Vec::new(),
            user_deny: Vec::new(),
        }
    }
}

impl Engine {
    pub fn new(mode: Mode, allowed: Vec<String>, disallowed: Vec<String>) -> Result<Self> {
        let mut engine = Self {
            session_allow: allowed
                .iter()
                .map(|s| Rule::parse(s))
                .collect::<Result<_>>()?,
            session_deny: disallowed
                .iter()
                .map(|s| Rule::parse(s))
                .collect::<Result<_>>()?,
            ..Self::default()
        };
        engine.mode = mode;
        Ok(engine)
    }

    pub fn with_mode(mode: Mode) -> Self {
        let mut engine = Self::default();
        engine.mode = mode;
        engine
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Add a session-scoped allow rule, used by "always allow" replies.
    pub fn allow(&mut self, tool: &str, target: &str) {
        let rule = if target.is_empty() {
            Rule::Tool(tool.to_string())
        } else {
            Rule::Scoped {
                tool: tool.to_string(),
                pattern: target.to_string(),
            }
        };
        self.session_allow.push(rule);
    }

    pub fn decide(&self, tool: &str, target: &str) -> Decision {
        // Hard safety floor: dangerous shell commands stay blocked in every mode.
        if tool.eq_ignore_ascii_case("bash") {
            if let Some(why) = dangerous_bash(target) {
                return Decision::Deny(format!("blocked dangerous command: {why}"));
            }
        }

        let deny = self
            .session_deny
            .iter()
            .chain(&self.project_deny)
            .chain(&self.user_deny)
            .any(|r| r.matches(tool, target));
        if deny {
            return Decision::Deny("denied by a rule".into());
        }

        let allowed = self
            .session_allow
            .iter()
            .chain(&self.project_allow)
            .chain(&self.user_allow)
            .any(|r| r.matches(tool, target));
        if allowed {
            return Decision::Allow;
        }

        match self.mode {
            Mode::Yolo => Decision::Allow,
            Mode::AutoEdit if is_edit_tool(tool) => Decision::Allow,
            Mode::Plan if !is_read_tool(tool) => {
                Decision::Deny("plan mode only allows read-only tools".into())
            }
            _ if is_read_tool(tool) => Decision::Allow,
            _ => Decision::Ask,
        }
    }
}

fn is_read_tool(tool: &str) -> bool {
    matches!(
        tool.to_ascii_lowercase().as_str(),
        "read" | "glob" | "grep" | "ls"
    )
}

fn is_edit_tool(tool: &str) -> bool {
    matches!(
        tool.to_ascii_lowercase().as_str(),
        "write" | "edit" | "multi_edit"
    )
}

fn dangerous_bash(command: &str) -> Option<&'static str> {
    let lower = command.to_ascii_lowercase();
    [
        "rm -rf /",
        "rm -fr /",
        "mkfs",
        "dd if=",
        "chmod -r 777 /",
        "shutdown",
        "reboot",
        ":(){:|:&};:",
    ]
    .into_iter()
    .find(|needle| lower.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_matches_claude_style_rules() {
        assert!(wildcard("git:*", "git status"));
        assert!(!wildcard("git:*", "gitk status"));
        assert!(wildcard("src/**", "src/a/b.rs"));
        assert!(wildcard("src/*.rs", "src/main.rs"));
        assert!(!wildcard("src/*.rs", "src/nested/main.rs"));
    }

    #[test]
    fn precedence_and_modes() {
        let engine = Engine::new(
            Mode::Ask,
            vec!["Bash(git:*)".into()],
            vec!["Bash(git push:*)".into()],
        )
        .unwrap();
        assert_eq!(engine.decide("bash", "git status"), Decision::Allow);
        assert_eq!(
            engine.decide("bash", "git push origin"),
            Decision::Deny("denied by a rule".into())
        );
        assert_eq!(engine.decide("read", "a.txt"), Decision::Allow);
        assert_eq!(engine.decide("write", "a.txt"), Decision::Ask);

        let auto = Engine::with_mode(Mode::AutoEdit);
        assert_eq!(auto.decide("write", "a.txt"), Decision::Allow);
        assert_eq!(auto.decide("bash", "ls"), Decision::Ask);

        let plan = Engine::with_mode(Mode::Plan);
        assert_eq!(plan.decide("read", "a.txt"), Decision::Allow);
        assert_eq!(plan.decide("write", "a.txt").is_deny(), true);
    }

    #[test]
    fn dangerous_commands_are_always_denied() {
        for mode in [Mode::Ask, Mode::AutoEdit, Mode::Plan, Mode::Yolo] {
            let d = Engine::with_mode(mode).decide("bash", "sudo rm -rf /");
            assert!(matches!(d, Decision::Deny(_)), "{mode}: {d:?}");
        }
    }

    impl Decision {
        fn is_deny(&self) -> bool {
            matches!(self, Self::Deny(_))
        }
    }
}
