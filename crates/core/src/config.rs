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
    #[serde(default)]
    pub consolidation: ConsolidationConfig,
    #[serde(default)]
    pub shape: ShapeConfig,
    #[serde(default)]
    pub relational: RelationalConfig,
    #[serde(default)]
    pub posture: PostureConfig,
    #[serde(default)]
    pub reranker: RerankerConfig,
    #[serde(default)]
    pub calibration: CalibrationConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvidersConfig {
    #[serde(default = "default_embedder")]
    pub embedder: String,
    #[serde(default)]
    pub completer: Option<String>,
    #[serde(default)]
    pub embedding: EmbeddingConfig,
    #[serde(default)]
    pub ollama: OllamaConfig,
    #[serde(default)]
    pub openai: OpenAiConfig,
    #[serde(default)]
    pub anthropic: AnthropicConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaConfig {
    #[serde(default = "default_ollama_base_url")]
    pub base_url: String,
    #[serde(default = "default_ollama_embed_model")]
    pub embed_model: String,
    #[serde(default = "default_ollama_complete_model")]
    pub complete_model: String,
}

fn default_ollama_base_url() -> String {
    "http://localhost:11434".to_string()
}

fn default_ollama_embed_model() -> String {
    "nomic-embed-text".to_string()
}

fn default_ollama_complete_model() -> String {
    "llama3.1".to_string()
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self {
            base_url: default_ollama_base_url(),
            embed_model: default_ollama_embed_model(),
            complete_model: default_ollama_complete_model(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAiConfig {
    #[serde(default = "default_openai_base_url")]
    pub base_url: String,
    #[serde(default = "default_openai_embed_model")]
    pub embed_model: String,
    #[serde(default = "default_openai_complete_model")]
    pub complete_model: String,
    #[serde(default = "default_openai_api_key_env")]
    pub api_key_env: String,
}

fn default_openai_base_url() -> String {
    "https://api.openai.com/v1".to_string()
}

fn default_openai_embed_model() -> String {
    "text-embedding-3-small".to_string()
}

fn default_openai_complete_model() -> String {
    "gpt-4o-mini".to_string()
}

fn default_openai_api_key_env() -> String {
    "OPENAI_API_KEY".to_string()
}

impl Default for OpenAiConfig {
    fn default() -> Self {
        Self {
            base_url: default_openai_base_url(),
            embed_model: default_openai_embed_model(),
            complete_model: default_openai_complete_model(),
            api_key_env: default_openai_api_key_env(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicConfig {
    #[serde(default = "default_anthropic_base_url")]
    pub base_url: String,
    #[serde(default = "default_anthropic_complete_model")]
    pub complete_model: String,
    #[serde(default = "default_anthropic_api_key_env")]
    pub api_key_env: String,
}

fn default_anthropic_base_url() -> String {
    "https://api.anthropic.com/v1".to_string()
}

fn default_anthropic_complete_model() -> String {
    "claude-3-5-sonnet-latest".to_string()
}

fn default_anthropic_api_key_env() -> String {
    "ANTHROPIC_API_KEY".to_string()
}

impl Default for AnthropicConfig {
    fn default() -> Self {
        Self {
            base_url: default_anthropic_base_url(),
            complete_model: default_anthropic_complete_model(),
            api_key_env: default_anthropic_api_key_env(),
        }
    }
}

impl Default for ProvidersConfig {
    fn default() -> Self {
        Self {
            embedder: default_embedder(),
            completer: None,
            embedding: EmbeddingConfig::default(),
            ollama: OllamaConfig::default(),
            openai: OpenAiConfig::default(),
            anthropic: AnthropicConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    /// fastembed model name (e.g. BGESmallENV15)
    pub model: Option<String>,
}

fn default_embedder() -> String {
    "fastembed".to_string()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BudgetsConfig {
    #[serde(default = "default_recall_tokens")]
    pub default_recall_tokens: u32,
}

fn default_recall_tokens() -> u32 {
    2000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdsConfig {
    /// RRF constant k (typically 60).
    #[serde(default = "default_rrf_k")]
    pub rrf_k: u32,
    /// Fused RRF score lower bound for strong_hit (among distance-qualified hits).
    #[serde(default = "default_strong_cutoff")]
    pub strong_cutoff: f64,
    /// Fused RRF score lower bound for weak_hit (among distance-qualified hits).
    #[serde(default = "default_weak_cutoff")]
    pub weak_cutoff: f64,
    /// Minimum distinct signals (lexical + semantic) for strong_hit corroboration.
    #[serde(default = "default_min_corroborating_signals")]
    pub min_corroborating_signals: u32,
    /// Max L2 distance for top semantic hit to count as weak_hit.
    #[serde(default = "default_semantic_weak_max_distance")]
    pub semantic_weak_max_distance: f32,
    /// Max L2 distance for top semantic hit to count toward strong_hit (with corroboration).
    #[serde(default = "default_semantic_strong_max_distance")]
    pub semantic_strong_max_distance: f32,
    /// Max L2 distance for density neighborhood (high_confidence_gap).
    #[serde(default = "default_density_radius")]
    pub density_radius: f32,
    /// Min neighbors within density_radius for high_confidence_gap.
    #[serde(default = "default_density_min_count")]
    pub density_min_count: u32,
    /// Max L2 distance to nearest collection centroid for low_confidence_gap.
    #[serde(default = "default_centroid_radius")]
    pub centroid_radius: f32,
}

fn default_rrf_k() -> u32 {
    60
}

fn default_strong_cutoff() -> f64 {
    0.03
}

fn default_weak_cutoff() -> f64 {
    0.015
}

fn default_min_corroborating_signals() -> u32 {
    2
}

fn default_semantic_weak_max_distance() -> f32 {
    0.55
}

fn default_semantic_strong_max_distance() -> f32 {
    0.38
}

/// Wider than `semantic_weak_max_distance` so a dense neighborhood can exist
/// while the top hit remains beyond the relevance cutoff (high_confidence_gap).
fn default_density_radius() -> f32 {
    0.8
}

fn default_density_min_count() -> u32 {
    3
}

/// Between weak and density radii: near a collection centroid but no precise hit.
fn default_centroid_radius() -> f32 {
    0.72
}

impl Default for ThresholdsConfig {
    fn default() -> Self {
        Self {
            rrf_k: default_rrf_k(),
            strong_cutoff: default_strong_cutoff(),
            weak_cutoff: default_weak_cutoff(),
            min_corroborating_signals: default_min_corroborating_signals(),
            semantic_weak_max_distance: default_semantic_weak_max_distance(),
            semantic_strong_max_distance: default_semantic_strong_max_distance(),
            density_radius: default_density_radius(),
            density_min_count: default_density_min_count(),
            centroid_radius: default_centroid_radius(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            providers: ProvidersConfig::default(),
            budgets: BudgetsConfig {
                default_recall_tokens: default_recall_tokens(),
            },
            thresholds: ThresholdsConfig::default(),
            consolidation: ConsolidationConfig::default(),
            shape: ShapeConfig::default(),
            relational: RelationalConfig::default(),
            posture: PostureConfig::default(),
            reranker: RerankerConfig::default(),
            calibration: CalibrationConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShapeConfig {
    #[serde(default = "default_shape_enabled")]
    pub enabled: bool,
    /// RRF weight multiplier for shape signal (low-weight corroboration).
    #[serde(default = "default_shape_weight")]
    pub weight: f64,
    #[serde(default = "default_shape_max_distance")]
    pub max_distance: f32,
}

fn default_shape_enabled() -> bool {
    true
}

fn default_shape_weight() -> f64 {
    0.5
}

fn default_shape_max_distance() -> f32 {
    0.6
}

impl Default for ShapeConfig {
    fn default() -> Self {
        Self {
            enabled: default_shape_enabled(),
            weight: default_shape_weight(),
            max_distance: default_shape_max_distance(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationalConfig {
    #[serde(default = "default_relational_salience_half_life_days")]
    pub salience_half_life_days: f64,
    #[serde(default = "default_relational_min_projects")]
    pub min_projects: u32,
    #[serde(default = "default_relational_min_task_types")]
    pub min_task_types: u32,
    #[serde(default = "default_relational_min_registers")]
    pub min_registers: u32,
}

fn default_relational_salience_half_life_days() -> f64 {
    30.0
}

fn default_relational_min_projects() -> u32 {
    1
}

fn default_relational_min_task_types() -> u32 {
    1
}

fn default_relational_min_registers() -> u32 {
    1
}

impl Default for RelationalConfig {
    fn default() -> Self {
        Self {
            salience_half_life_days: default_relational_salience_half_life_days(),
            min_projects: default_relational_min_projects(),
            min_task_types: default_relational_min_task_types(),
            min_registers: default_relational_min_registers(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostureConfig {
    #[serde(default = "default_meta_reliability_threshold")]
    pub meta_reliability_threshold: f64,
}

fn default_meta_reliability_threshold() -> f64 {
    // Corrections alone floor reliability at 0.5 (penalty capped at 0.5); threshold must
    // exceed that floor so repeated corrections trigger humility and score calibration.
    0.6
}

impl Default for PostureConfig {
    fn default() -> Self {
        Self {
            meta_reliability_threshold: default_meta_reliability_threshold(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RerankerConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_reranker_model")]
    pub model: String,
    #[serde(default = "default_reranker_top_n")]
    pub top_n: u32,
}

fn default_reranker_model() -> String {
    "JINARerankerV1TurboEn".to_string()
}

fn default_reranker_top_n() -> u32 {
    20
}

impl Default for RerankerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: default_reranker_model(),
            top_n: default_reranker_top_n(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalibrationConfig {
    #[serde(default = "default_calibration_enabled")]
    pub enabled: bool,
    #[serde(default = "default_score_weight_floor")]
    pub score_weight_floor: f64,
}

fn default_calibration_enabled() -> bool {
    true
}

fn default_score_weight_floor() -> f64 {
    0.5
}

impl Default for CalibrationConfig {
    fn default() -> Self {
        Self {
            enabled: default_calibration_enabled(),
            score_weight_floor: default_score_weight_floor(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationConfig {
    /// Max semantic L2 distance to consider near-duplicates for merge.
    #[serde(default = "default_dedupe_max_distance")]
    pub dedupe_max_distance: f32,
    /// Min token overlap ratio (0..1) between claims to merge.
    #[serde(default = "default_dedupe_claim_overlap")]
    pub dedupe_claim_overlap: f64,
    /// Incoming supports required for episodic -> provisional promotion.
    #[serde(default = "default_promote_episodic_to_provisional")]
    pub promote_episodic_to_provisional: u32,
    /// Incoming supports required for provisional -> semantic promotion.
    #[serde(default = "default_promote_provisional_to_semantic")]
    pub promote_provisional_to_semantic: u32,
    /// Salience half-life in days of inactivity.
    #[serde(default = "default_salience_half_life_days")]
    pub salience_half_life_days: f64,
    /// Minimum salience after decay.
    #[serde(default = "default_salience_floor")]
    pub salience_floor: f64,
}

fn default_dedupe_max_distance() -> f32 {
    0.25
}

fn default_dedupe_claim_overlap() -> f64 {
    0.6
}

fn default_promote_episodic_to_provisional() -> u32 {
    1
}

fn default_promote_provisional_to_semantic() -> u32 {
    2
}

fn default_salience_half_life_days() -> f64 {
    30.0
}

fn default_salience_floor() -> f64 {
    0.05
}

impl Default for ConsolidationConfig {
    fn default() -> Self {
        Self {
            dedupe_max_distance: default_dedupe_max_distance(),
            dedupe_claim_overlap: default_dedupe_claim_overlap(),
            promote_episodic_to_provisional: default_promote_episodic_to_provisional(),
            promote_provisional_to_semantic: default_promote_provisional_to_semantic(),
            salience_half_life_days: default_salience_half_life_days(),
            salience_floor: default_salience_floor(),
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
        assert_eq!(loaded.providers.embedder, "fastembed");
        assert_eq!(loaded.budgets.default_recall_tokens, 2000);
        assert_eq!(loaded.thresholds.rrf_k, 60);
        assert_eq!(loaded.thresholds.semantic_weak_max_distance, 0.55);
        assert_eq!(loaded.thresholds.density_radius, 0.8);
        assert_eq!(loaded.providers.ollama.embed_model, "nomic-embed-text");
        assert_eq!(loaded.reranker.model, "JINARerankerV1TurboEn");
        assert!(loaded.calibration.enabled);
        assert_eq!(loaded.posture.meta_reliability_threshold, 0.6);
    }

    #[test]
    fn default_thresholds_allow_gap_states() {
        let t = ThresholdsConfig::default();
        assert!(
            t.density_radius > t.semantic_weak_max_distance,
            "density_radius must exceed semantic_weak_max_distance for high_confidence_gap"
        );
        assert!(
            t.centroid_radius > t.semantic_weak_max_distance,
            "centroid_radius must exceed semantic_weak_max_distance for low_confidence_gap"
        );
    }
}
