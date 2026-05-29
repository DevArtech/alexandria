use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AlexandriaError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub providers: ProvidersConfig,
    #[serde(default)]
    pub budgets: BudgetsConfig,
    #[serde(default)]
    pub thresholds: ThresholdsConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProvidersConfig {
    #[serde(default = "default_embedder")]
    pub embedder: String,
    #[serde(default)]
    pub completer: Option<String>,
}

fn default_embedder() -> String {
    "none".to_string()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BudgetsConfig {
    #[serde(default = "default_recall_tokens")]
    pub default_recall_tokens: u32,
}

fn default_recall_tokens() -> u32 {
    2000
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ThresholdsConfig {
    /// FTS5 bm25 upper bound for strong_hit (more negative = better; score must be <= this).
    #[serde(default = "default_strong_cutoff")]
    pub strong_cutoff: f64,
    /// FTS5 bm25 upper bound for weak_hit (must be > strong_cutoff, closer to zero).
    #[serde(default = "default_weak_cutoff")]
    pub weak_cutoff: f64,
}

fn default_strong_cutoff() -> f64 {
    -1.0
}

fn default_weak_cutoff() -> f64 {
    1.0
}

impl Default for Config {
    fn default() -> Self {
        Self {
            providers: ProvidersConfig {
                embedder: default_embedder(),
                completer: None,
            },
            budgets: BudgetsConfig {
                default_recall_tokens: default_recall_tokens(),
            },
            thresholds: ThresholdsConfig {
                strong_cutoff: default_strong_cutoff(),
                weak_cutoff: default_weak_cutoff(),
            },
        }
    }
}

impl Config {
    pub fn path(library_root: &Path) -> PathBuf {
        library_root.join(".alexandria").join("config.toml")
    }

    pub fn load(library_root: &Path) -> Result<Self> {
        let path = Self::path(library_root);
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(&path)?;
        toml::from_str(&content)
            .map_err(|e| AlexandriaError::Config(format!("failed to parse config: {e}")))
    }

    pub fn write_default(library_root: &Path) -> Result<()> {
        let path = Self::path(library_root);
        if path.exists() {
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(&Self::default())
            .map_err(|e| AlexandriaError::Config(e.to_string()))?;
        fs::write(path, content)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn default_config_round_trip() {
        let dir = TempDir::new().unwrap();
        Config::write_default(dir.path()).unwrap();
        let loaded = Config::load(dir.path()).unwrap();
        assert_eq!(loaded.providers.embedder, "none");
        assert_eq!(loaded.budgets.default_recall_tokens, 2000);
    }
}
