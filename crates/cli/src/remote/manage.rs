use anyhow::{bail, Result};

use super::config::{
    config_path, is_local_default, load_config, looks_like_url, normalize_mcp_url, save_config,
    set_default_target, RemoteProfile, DEFAULT_LOCAL, DEFAULT_TOKEN_ENV,
};
use crate::OutputFormat;

pub fn add(
    name: String,
    url: String,
    token_env: Option<String>,
    set_default: bool,
    format: OutputFormat,
) -> Result<()> {
    if looks_like_url(&name) {
        bail!("profile name must not look like a URL; use a short name such as `prod` or `local`");
    }
    let mut cfg = load_config()?;
    let profile = RemoteProfile {
        url: url.trim().to_string(),
        token_env: token_env
            .unwrap_or_else(|| DEFAULT_TOKEN_ENV.to_string())
            .trim()
            .to_string(),
    };
    cfg.profiles.insert(name.clone(), profile);
    if set_default {
        cfg.default = Some(name.clone());
    }
    save_config(&cfg)?;

    match format {
        OutputFormat::Human => {
            println!("Added remote profile '{name}'");
            println!("  url: {}", normalize_mcp_url(&cfg.profiles[&name].url));
            println!("  token_env: {}", cfg.profiles[&name].token_env);
            if cfg.default.as_deref() == Some(name.as_str()) {
                println!("  (default — used automatically without --remote)");
            }
            println!("Config: {}", config_path()?.display());
        }
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "profile": name,
                    "mcp_url": normalize_mcp_url(&cfg.profiles[&name].url),
                    "token_env": cfg.profiles[&name].token_env,
                    "default": cfg.default,
                    "config_path": config_path()?.display().to_string(),
                }))?
            );
        }
    }
    Ok(())
}

pub fn list(format: OutputFormat) -> Result<()> {
    let cfg = load_config()?;
    let path = config_path()?;

    match format {
        OutputFormat::Human => {
            println!("config: {}", path.display());
            print_default_line(&cfg);
            if cfg.profiles.is_empty() {
                println!("(no profiles — run `alexandria remote add <name> --url <https://...>`)");
                return Ok(());
            }
            for (name, p) in &cfg.profiles {
                let mark = if is_active_default(&cfg, name) {
                    " *"
                } else {
                    ""
                };
                println!(
                    "  {name}{mark}: {} (token: {})",
                    normalize_mcp_url(&p.url),
                    p.token_env
                );
            }
        }
        OutputFormat::Json => {
            let profiles: serde_json::Map<String, serde_json::Value> = cfg
                .profiles
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        serde_json::json!({
                            "url": v.url,
                            "mcp_url": normalize_mcp_url(&v.url),
                            "token_env": v.token_env,
                        }),
                    )
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "config_path": path.display().to_string(),
                    "default": cfg.default,
                    "profiles": profiles,
                }))?
            );
        }
    }
    Ok(())
}

pub fn remove(name: String, format: OutputFormat) -> Result<()> {
    let mut cfg = load_config()?;
    if cfg.profiles.remove(&name).is_none() {
        bail!("remote profile '{name}' not found");
    }
    if cfg.default.as_deref() == Some(name.as_str()) {
        cfg.default = Some(DEFAULT_LOCAL.to_string());
    }
    save_config(&cfg)?;

    match format {
        OutputFormat::Human => println!("Removed remote profile '{name}'"),
        OutputFormat::Json => println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "removed": name,
                "default": cfg.default,
            }))?
        ),
    }
    Ok(())
}

pub fn set_default(name: String, format: OutputFormat) -> Result<()> {
    set_default_target(&name)?;
    let stored = if is_local_default(Some(&name)) {
        DEFAULT_LOCAL.to_string()
    } else {
        name.clone()
    };

    match format {
        OutputFormat::Human => {
            if is_local_default(Some(&name)) {
                println!("Default is now local (discover .alexandria/ from cwd)");
            } else {
                println!(
                    "Default is now remote profile '{name}' (used automatically without --remote)"
                );
                let cfg = load_config()?;
                if let Some(p) = cfg.profiles.get(&name) {
                    println!("  url: {}", normalize_mcp_url(&p.url));
                }
            }
            println!("Config: {}", config_path()?.display());
        }
        OutputFormat::Json => println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "default": stored }))?
        ),
    }
    Ok(())
}

fn is_active_default(cfg: &super::config::RemoteConfigFile, profile_name: &str) -> bool {
    cfg.default.as_deref() == Some(profile_name)
}

fn print_default_line(cfg: &super::config::RemoteConfigFile) {
    if is_local_default(cfg.default.as_deref()) {
        println!("default: local (use library on disk — no --remote needed)");
        return;
    }
    if let Some(ref name) = cfg.default {
        if let Some(p) = cfg.profiles.get(name) {
            println!(
                "default: {name} → {} (used automatically without --remote)",
                normalize_mcp_url(&p.url)
            );
        } else {
            println!("default: {name} (profile missing — run `alexandria remote use local`)");
        }
    }
}
