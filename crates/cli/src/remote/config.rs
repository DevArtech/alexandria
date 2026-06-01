use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

const CONFIG_FILE: &str = "remote.toml";
const ENV_REMOTE: &str = "ALEXANDRIA_REMOTE";
pub const DEFAULT_TOKEN_ENV: &str = "ALEXANDRIA_MCP_TOKEN";

/// Config value meaning "use the local library" (discover `.alexandria/` from cwd).
pub const DEFAULT_LOCAL: &str = "local";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RemoteConfigFile {
    /// Default target when no `--remote` / `ALEXANDRIA_REMOTE`: `"local"` or a profile name.
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub profiles: BTreeMap<String, RemoteProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteProfile {
    pub url: String,
    #[serde(default = "default_token_env")]
    pub token_env: String,
}

fn default_token_env() -> String {
    DEFAULT_TOKEN_ENV.to_string()
}

/// Resolved connection target for a single CLI invocation.
#[derive(Debug, Clone)]
pub struct ResolvedRemote {
    pub mcp_url: String,
    pub token_env: String,
    /// Profile name if resolved from config; None when a raw URL was used.
    pub profile_name: Option<String>,
}

pub fn config_path() -> Result<PathBuf> {
    let base = dirs::config_dir().context("could not determine config directory")?;
    Ok(base.join("alexandria").join(CONFIG_FILE))
}

pub fn load_config() -> Result<RemoteConfigFile> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(RemoteConfigFile::default());
    }
    let raw = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let cfg: RemoteConfigFile =
        toml::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
    Ok(cfg)
}

pub fn save_config(cfg: &RemoteConfigFile) -> Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let raw = toml::to_string_pretty(cfg).context("serialize remote config")?;
    fs::write(&path, raw).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

/// True when `default` means use the local library (not a remote profile).
pub fn is_local_default(default: Option<&str>) -> bool {
    match default.map(str::trim) {
        None | Some("") => true,
        Some(s) => s.eq_ignore_ascii_case(DEFAULT_LOCAL),
    }
}

/// Resolution order: `--remote` flag -> `ALEXANDRIA_REMOTE` env -> `default` in config (`local` or profile).
pub fn resolve_remote(
    flag: Option<&str>,
    token_env_override: Option<&str>,
) -> Result<Option<ResolvedRemote>> {
    let source = flag.map(str::to_string).or_else(|| {
        std::env::var(ENV_REMOTE)
            .ok()
            .filter(|s| !s.trim().is_empty())
    });

    let Some(source) = source else {
        let cfg = load_config()?;
        if is_local_default(cfg.default.as_deref()) {
            return Ok(None);
        }
        let name = cfg
            .default
            .as_deref()
            .expect("non-local default checked above");
        return resolve_profile(&cfg, name, token_env_override);
    };

    let source = source.trim().to_string();
    if source.eq_ignore_ascii_case(DEFAULT_LOCAL) {
        return Ok(None);
    }

    if looks_like_url(&source) {
        let token_env = token_env_override
            .map(str::to_string)
            .unwrap_or_else(|| DEFAULT_TOKEN_ENV.to_string());
        return Ok(Some(ResolvedRemote {
            mcp_url: normalize_mcp_url(&source),
            token_env,
            profile_name: None,
        }));
    }

    let cfg = load_config()?;
    resolve_profile(&cfg, &source, token_env_override)
}

/// Persist the default target (`local` or a profile name).
pub fn set_default_target(name: &str) -> Result<()> {
    let mut cfg = load_config()?;
    if !is_local_default(Some(name)) && !cfg.profiles.contains_key(name) {
        bail!("remote profile '{name}' not found (run `alexandria remote list`)");
    }
    cfg.default = if is_local_default(Some(name)) {
        Some(DEFAULT_LOCAL.to_string())
    } else {
        Some(name.to_string())
    };
    save_config(&cfg)
}

fn resolve_profile(
    cfg: &RemoteConfigFile,
    name: &str,
    token_env_override: Option<&str>,
) -> Result<Option<ResolvedRemote>> {
    let profile = cfg.profiles.get(name).with_context(|| {
        format!("remote profile '{name}' not found (run `alexandria remote list`)")
    })?;
    let token_env = token_env_override
        .map(str::to_string)
        .unwrap_or_else(|| profile.token_env.clone());
    Ok(Some(ResolvedRemote {
        mcp_url: normalize_mcp_url(&profile.url),
        token_env,
        profile_name: Some(name.to_string()),
    }))
}

pub fn read_token(token_env: &str) -> Result<String> {
    let token = std::env::var(token_env)
        .with_context(|| format!("environment variable {token_env} is not set"))?;
    let token = token.trim().to_string();
    if token.is_empty() {
        bail!("environment variable {token_env} is empty");
    }
    Ok(token)
}

pub fn looks_like_url(s: &str) -> bool {
    let s = s.trim();
    s.starts_with("http://") || s.starts_with("https://")
}

pub fn normalize_mcp_url(url: &str) -> String {
    let url = url.trim().trim_end_matches('/');
    if url.ends_with("/mcp") {
        url.to_string()
    } else {
        format!("{url}/mcp")
    }
}

pub fn guard_local_only(remote: Option<&ResolvedRemote>, command: &str) -> Result<()> {
    if remote.is_some() {
        bail!(
            "`{command}` only runs against a local library (it touches files/index on disk). \
             Use `--local`, unset {ENV_REMOTE}, or run `alexandria remote use local`."
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_local_default_values() {
        assert!(is_local_default(None));
        assert!(is_local_default(Some("local")));
        assert!(is_local_default(Some("LOCAL")));
        assert!(!is_local_default(Some("prod")));
    }

    #[test]
    fn normalize_appends_mcp() {
        assert_eq!(
            normalize_mcp_url("http://127.0.0.1:8080"),
            "http://127.0.0.1:8080/mcp"
        );
        assert_eq!(
            normalize_mcp_url("http://127.0.0.1:8080/mcp"),
            "http://127.0.0.1:8080/mcp"
        );
    }
}
