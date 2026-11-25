//! Local BYOK key storage. Keys are never logged, returned in full, or written
//! as plaintext. Order: environment variable → Termux keystore (best effort)
//! → encrypted file with mode 0600.

use crate::config;
use anyhow::{bail, Context, Result};
use ring::{
    aead::{Aad, LessSafeKey, Nonce, UnboundKey, CHACHA20_POLY1305, NONCE_LEN},
    rand::{SecureRandom, SystemRandom},
};
use std::{os::unix::fs::PermissionsExt, path::PathBuf};

fn keys_dir() -> Result<PathBuf> {
    Ok(config::data_dir()?.join("tagent/keys"))
}

fn master_path() -> Result<PathBuf> {
    Ok(keys_dir()?.join("master.key"))
}

fn key_path(provider: &str) -> Result<PathBuf> {
    if provider.is_empty() || provider.contains('/') || provider.contains('\\') {
        bail!("invalid provider name");
    }
    Ok(keys_dir()?.join(format!("{provider}.key")))
}

fn restrict(path: &PathBuf) -> Result<()> {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("cannot restrict {}", path.display()))
}

fn load_or_create_master() -> Result<[u8; 32]> {
    let dir = keys_dir()?;
    std::fs::create_dir_all(&dir)?;
    let path = master_path()?;
    if path.exists() {
        let bytes = std::fs::read(&path)?;
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("master key is corrupt"))?;
        return Ok(arr);
    }
    let mut key = [0u8; 32];
    SystemRandom::new()
        .fill(&mut key)
        .map_err(|_| anyhow::anyhow!("cannot generate master key"))?;
    std::fs::write(&path, key)?;
    restrict(&path)?;
    Ok(key)
}

fn cipher_key() -> Result<LessSafeKey> {
    let master = load_or_create_master()?;
    let key = UnboundKey::new(&CHACHA20_POLY1305, &master)
        .map_err(|_| anyhow::anyhow!("invalid master key"))?;
    Ok(LessSafeKey::new(key))
}

/// Store a key locally. Callers must never print the `key` argument.
pub fn set(provider: &str, key: &str) -> Result<()> {
    if key.trim().is_empty() {
        bail!("key is empty");
    }
    let cipher = cipher_key()?;
    let mut nonce_bytes = [0u8; NONCE_LEN];
    SystemRandom::new()
        .fill(&mut nonce_bytes)
        .map_err(|_| anyhow::anyhow!("cannot generate nonce"))?;
    let nonce = Nonce::assume_unique_for_key(nonce_bytes);
    let mut output = nonce_bytes.to_vec();
    output.extend_from_slice(key.as_bytes());
    cipher
        .seal_in_place_append_tag(nonce, Aad::from(provider.as_bytes()), &mut output)
        .map_err(|_| anyhow::anyhow!("encryption failed"))?;

    let path = key_path(provider)?;
    std::fs::write(&path, output)?;
    restrict(&path)?;

    // TODO(verify): exact Termux:API keystore CLI syntax on-device. The
    // encrypted 0600 file above is the authoritative local store for now.
    Ok(())
}

/// Fetch a key without ever exposing it to logs. Returns `None` if unset.
pub fn get(provider: &str, env_name: Option<&str>) -> Option<String> {
    if let Some(name) = env_name {
        if let Ok(value) = std::env::var(name) {
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    let path = key_path(provider).ok()?;
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() <= NONCE_LEN {
        return None;
    }
    let nonce_bytes: [u8; NONCE_LEN] = bytes[..NONCE_LEN].try_into().ok()?;
    let cipher = cipher_key().ok()?;
    let nonce = Nonce::assume_unique_for_key(nonce_bytes);
    let mut sealed = bytes[NONCE_LEN..].to_vec();
    let plain = cipher
        .open_in_place(nonce, Aad::from(provider.as_bytes()), &mut sealed)
        .ok()?;
    String::from_utf8(plain.to_vec()).ok()
}

pub fn remove(provider: &str) -> Result<()> {
    let path = key_path(provider)?;
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

pub fn configured(provider: &str, env_name: Option<&str>) -> bool {
    if let Some(name) = env_name {
        if std::env::var_os(name).is_some() {
            return true;
        }
    }
    key_path(provider).map(|p| p.exists()).unwrap_or(false)
}

pub fn list(env_names: &[(String, Option<String>)]) -> Vec<(String, bool)> {
    env_names
        .iter()
        .map(|(name, env)| (name.clone(), configured(name, env.as_deref())))
        .collect()
}
