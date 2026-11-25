//! Paths and BYOK config. Only the Anthropic adapter exists in M0.
//! Nothing in this module logs or persists a key value.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::{collections::HashMap, path::PathBuf};

pub const DEFAULT_KEY_ENV: &str = "ANTHROPIC_API_KEY";
pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";

pub fn home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set (Termux always provides it)")
}

pub fn config_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(dir));
    }
    Ok(home()?.join(".config"))
}

pub fn data_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_DATA_HOME") {
        return Ok(PathBuf::from(dir));
    }
    Ok(home()?.join(".local/share"))
}

pub fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("tagent/config.toml"))
}

pub fn sessions_dir() -> Result<PathBuf> {
    Ok(data_dir()?.join("tagent/sessions"))
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type")]
pub enum ProviderConfig {
    #[serde(rename = "anthropic")]
    Anthropic {
        key_env: String,
        #[serde(default)]
        base_url: Option<String>,
    },
}

impl ProviderConfig {
    /// Name of the environment variable holding the key. Only the *name* is
    /// ever exposed; the resolved secret stays inside `provider::anthropic`.
    pub fn key_env_name(&self) -> &str {
        match self {
            Self::Anthropic { key_env, .. } => key_env,
        }
    }

    pub fn base_url(&self) -> &str {
        match self {
            Self::Anthropic { base_url, .. } => base_url.as_deref().unwrap_or(DEFAULT_BASE_URL),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Roles {
    pub main: Option<String>,
    pub fast: Option<String>,
    pub plan: Option<String>,
    pub subagent: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub providers: HashMap<String, ProviderConfig>,
    #[serde(default)]
    pub roles: Roles,
}

impl Config {
    /// A missing config file is fine in M0: env-only BYOK with the Anthropic
    /// provider under the name `anthropic`.
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("cannot read config at {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("invalid TOML in {}", path.display()))
    }

    pub fn anthropic_default() -> ProviderConfig {
        ProviderConfig::Anthropic {
            key_env: DEFAULT_KEY_ENV.to_string(),
            base_url: None,
        }
    }

    /// Resolve `provider/model` (or a bare role name from `[roles]`).
    pub fn resolve(&self, spec: &str) -> Result<(String, String, ProviderConfig)> {
        let full = if spec.contains('/') {
            spec.to_string()
        } else {
            let cfg_path = config_path()?;
            match spec {
                "main" | "fast" | "plan" | "subagent" => {
                    let role = match spec {
                        "main" => &self.roles.main,
                        "fast" => &self.roles.fast,
                        "plan" => &self.roles.plan,
                        _ => &self.roles.subagent,
                    };
                    role.clone().with_context(|| {
                        format!("role '{spec}' is not set in {} (and no provider/model given)", cfg_path.display())
                    })?
                }
                other => bail!(
                    "model '{other}' must be provider/model (e.g. anthropic/claude-sonnet-4-5) or a role name"
                ),
            }
        };
        let (provider, model) = full
            .split_once('/')
            .context("model must be provider/model")?;
        if provider.is_empty() || model.is_empty() {
            bail!("model must be provider/model with both parts non-empty");
        }
        let cfg = match self.providers.get(provider) {
            Some(c) => c.clone(),
            None if provider == "anthropic" => Self::anthropic_default(),
            None => bail!("unknown provider '{provider}' (M0 supports only 'anthropic')"),
        };
        Ok((provider.to_string(), model.to_string(), cfg))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_builtin_anthropic_without_config() {
        let cfg = Config::default();
        let (p, m, pc) = cfg.resolve("anthropic/claude-sonnet-4-5").unwrap();
        assert_eq!(p, "anthropic");
        assert_eq!(m, "claude-sonnet-4-5");
        assert_eq!(pc.key_env_name(), DEFAULT_KEY_ENV);
        assert_eq!(pc.base_url(), DEFAULT_BASE_URL);
    }

    #[test]
    fn rejects_bad_model_specs() {
        assert!(Config::default().resolve("claude").is_err());
        assert!(Config::default().resolve("openai/gpt-x").is_err());
    }

    #[test]
    fn parses_example_config_shape() {
        let parsed: Config = toml::from_str(
            r#"
[providers.anthropic]
type = "anthropic"
key_env = "MY_KEY"
base_url = "http://127.0.0.1:1234"

[roles]
main = "anthropic/claude-sonnet-4-5"
"#,
        )
        .unwrap();
        let (_, _, pc) = parsed.resolve("main").unwrap();
        assert_eq!(pc.key_env_name(), "MY_KEY");
        assert_eq!(pc.base_url(), "http://127.0.0.1:1234");
    }
}
