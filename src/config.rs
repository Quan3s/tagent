//! Paths, BYOK config and per-role model selection.
//! Nothing in this module logs or persists a key value.

use crate::keystore;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};

pub const DEFAULT_KEY_ENV: &str = "ANTHROPIC_API_KEY";
pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
pub const OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
pub const OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";
pub const GEMINI_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta/openai";

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

pub fn plugins_dir() -> Result<PathBuf> {
    Ok(data_dir()?.join("tagent/plugins"))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ProviderConfig {
    Anthropic {
        #[serde(default = "default_anthropic_key_env")]
        key_env: String,
        #[serde(default)]
        base_url: Option<String>,
    },
    OpenaiCompat {
        #[serde(default = "default_openai_key_env")]
        key_env: String,
        base_url: String,
    },
    Openrouter {
        #[serde(default = "default_openrouter_key_env")]
        key_env: String,
        #[serde(default)]
        base_url: Option<String>,
    },
    Gemini {
        #[serde(default = "default_gemini_key_env")]
        key_env: String,
        #[serde(default)]
        base_url: Option<String>,
    },
}

fn default_anthropic_key_env() -> String {
    DEFAULT_KEY_ENV.to_string()
}
fn default_openai_key_env() -> String {
    "OPENAI_API_KEY".to_string()
}
fn default_openrouter_key_env() -> String {
    "OPENROUTER_API_KEY".to_string()
}
fn default_gemini_key_env() -> String {
    "GEMINI_API_KEY".to_string()
}

impl ProviderConfig {
    /// Name of the environment variable holding the key. Only the *name* is
    /// ever exposed; the resolved secret stays inside the provider adapter.
    pub fn key_env_name(&self) -> &str {
        match self {
            Self::Anthropic { key_env, .. }
            | Self::OpenaiCompat { key_env, .. }
            | Self::Openrouter { key_env, .. }
            | Self::Gemini { key_env, .. } => key_env,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Anthropic { .. } => "anthropic",
            Self::OpenaiCompat { .. } => "openai-compat",
            Self::Openrouter { .. } => "openrouter",
            Self::Gemini { .. } => "gemini",
        }
    }

    pub fn base_url(&self) -> &str {
        match self {
            Self::Anthropic { base_url, .. } => base_url.as_deref().unwrap_or(DEFAULT_BASE_URL),
            Self::OpenaiCompat { base_url, .. } => base_url,
            Self::Openrouter { base_url, .. } => base_url.as_deref().unwrap_or(OPENROUTER_BASE_URL),
            Self::Gemini { base_url, .. } => base_url.as_deref().unwrap_or(GEMINI_BASE_URL),
        }
    }

    /// Environment first, then the local encrypted keystore.
    pub fn resolve_key(&self, provider_name: &str) -> Option<String> {
        keystore::get(provider_name, Some(self.key_env_name()))
    }

    pub fn configured(&self, provider_name: &str) -> bool {
        keystore::configured(provider_name, Some(self.key_env_name()))
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Roles {
    pub main: Option<String>,
    pub fast: Option<String>,
    pub plan: Option<String>,
    pub subagent: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Settings {
    pub theme: String,
    pub language: String,
    pub permission_mode: String,
    pub compact_threshold: u64,
    pub pricing: HashMap<String, f64>,
    pub permissions: HashMap<String, Vec<String>>,
    pub termux_api: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            language: "vi".into(),
            permission_mode: "ask".into(),
            compact_threshold: 24_000,
            pricing: HashMap::new(),
            permissions: HashMap::new(),
            termux_api: false,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub providers: HashMap<String, ProviderConfig>,
    pub roles: Roles,
    pub settings: Settings,
}

impl Config {
    /// A missing config file is fine: env-only BYOK with the built-in adapters.
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("cannot read config at {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("invalid TOML in {}", path.display()))
    }

    pub fn save(&self) -> Result<()> {
        let path = config_path()?;
        std::fs::create_dir_all(
            path.parent()
                .context("config path has no parent directory")?,
        )?;
        let text = toml::to_string_pretty(self).context("cannot serialize config")?;
        std::fs::write(&path, text)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    pub fn builtin(name: &str) -> Option<ProviderConfig> {
        match name {
            "anthropic" => Some(ProviderConfig::Anthropic {
                key_env: DEFAULT_KEY_ENV.into(),
                base_url: None,
            }),
            "openai" | "openai-compat" => Some(ProviderConfig::OpenaiCompat {
                key_env: default_openai_key_env(),
                base_url: OPENAI_BASE_URL.into(),
            }),
            "openrouter" => Some(ProviderConfig::Openrouter {
                key_env: default_openrouter_key_env(),
                base_url: None,
            }),
            "gemini" => Some(ProviderConfig::Gemini {
                key_env: default_gemini_key_env(),
                base_url: None,
            }),
            _ => None,
        }
    }

    pub fn anthropic_default() -> ProviderConfig {
        Self::builtin("anthropic").expect("builtin anthropic")
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
                        format!(
                            "role '{spec}' is not set in {} (and no provider/model given)",
                            cfg_path.display()
                        )
                    })?
                }
                other => bail!(
                    "model '{other}' must be provider/model (e.g. anthropic/<your-model>) or a role name"
                ),
            }
        };
        let (provider, model) = full
            .split_once('/')
            .context("model must be provider/model")?;
        if provider.is_empty() || model.is_empty() {
            bail!("model must be provider/model with both parts non-empty");
        }
        let cfg = self
            .providers
            .get(provider)
            .cloned()
            .or_else(|| Self::builtin(provider))
            .with_context(|| format!("unknown provider '{provider}'"))?;
        Ok((provider.to_string(), model.to_string(), cfg))
    }

    pub fn set_role(&mut self, role: &str, model: &str) -> Result<()> {
        let slot = match role {
            "main" => &mut self.roles.main,
            "fast" => &mut self.roles.fast,
            "plan" => &mut self.roles.plan,
            "subagent" => &mut self.roles.subagent,
            other => bail!("unknown role '{other}' (use main|fast|plan|subagent)"),
        };
        *slot = Some(model.to_string());
        Ok(())
    }

    /// `config get <key>` for the small set of flat settings exposed in CLI/UI.
    pub fn get(&self, key: &str) -> Result<String> {
        let value = match key {
            "settings.theme" => self.settings.theme.clone(),
            "settings.language" => self.settings.language.clone(),
            "settings.permission_mode" => self.settings.permission_mode.clone(),
            "settings.compact_threshold" => self.settings.compact_threshold.to_string(),
            "settings.permissions" => serde_json::to_string(&self.settings.permissions)?,
            "roles.main" => self.roles.main.clone().unwrap_or_default(),
            "roles.fast" => self.roles.fast.clone().unwrap_or_default(),
            "roles.plan" => self.roles.plan.clone().unwrap_or_default(),
            "roles.subagent" => self.roles.subagent.clone().unwrap_or_default(),
            other => bail!("unknown config key '{other}'"),
        };
        Ok(value)
    }

    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "settings.theme" => self.settings.theme = value.into(),
            "settings.language" => self.settings.language = value.into(),
            "settings.permission_mode" => self.settings.permission_mode = value.into(),
            "settings.compact_threshold" => {
                self.settings.compact_threshold =
                    value.parse().context("threshold must be a number")?;
            }
            "roles.main" => self.roles.main = Some(value.into()),
            "roles.fast" => self.roles.fast = Some(value.into()),
            "roles.plan" => self.roles.plan = Some(value.into()),
            "roles.subagent" => self.roles.subagent = Some(value.into()),
            other => bail!("unknown config key '{other}'"),
        }
        Ok(())
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
        assert!(Config::default().resolve("unknown/gpt-x").is_err());
    }

    #[test]
    fn parses_example_config_shape() {
        let parsed: Config = toml::from_str(
            r#"
[providers.anthropic]
type = "anthropic"
key_env = "MY_KEY"
base_url = "http://127.0.0.1:1234"

[providers.local]
type = "openai-compat"
base_url = "http://127.0.0.1:11434/v1"

[roles]
main = "anthropic/claude-sonnet-4-5"
"#,
        )
        .unwrap();
        let (_, _, pc) = parsed.resolve("main").unwrap();
        assert_eq!(pc.key_env_name(), "MY_KEY");
        assert_eq!(pc.base_url(), "http://127.0.0.1:1234");
        assert_eq!(
            parsed.resolve("local/model").unwrap().2.base_url(),
            "http://127.0.0.1:11434/v1"
        );
    }
}
