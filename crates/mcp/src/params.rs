use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RecallParams {
    pub query: String,
    #[serde(default)]
    pub budget: Option<u32>,
    #[serde(default)]
    pub audit: bool,
    #[serde(default)]
    pub high_stakes: bool,
    #[serde(default)]
    pub domain: Option<String>,
    /// Restrict to engrams in any of these collections (structured/faceted recall).
    #[serde(default)]
    pub collections: Vec<String>,
    /// Restrict to engrams carrying any of these tags (structured/faceted recall).
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MapParams {
    pub seed: String,
    #[serde(default)]
    pub depth: Option<u32>,
    #[serde(default)]
    pub rel: Vec<String>,
    #[serde(default)]
    pub budget: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SurveyParams {
    pub topic: String,
    #[serde(default)]
    pub budget: Option<u32>,
    #[serde(default)]
    pub depth: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CoverageParams {
    pub topic: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExpandParams {
    pub id: String,
    #[serde(default)]
    pub rel: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RememberParams {
    /// First line becomes the claim; remaining lines become the body.
    pub text: String,
    #[serde(default)]
    pub tier: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub collections: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub derived_from: Vec<String>,
    #[serde(default)]
    pub surface_when: Vec<String>,
    #[serde(default)]
    pub observed: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LinkParams {
    pub from: String,
    pub rel: String,
    pub to: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct IdParams {
    pub id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TimelineParams {
    #[serde(default)]
    pub since: Option<String>,
    #[serde(default)]
    pub until: Option<String>,
    #[serde(default)]
    pub tier: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ThreadsParams {
    #[serde(default)]
    pub surface_for: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MetaParams {
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub record_correction: bool,
    #[serde(default)]
    pub correction_domain: Option<String>,
    #[serde(default)]
    pub record_gap: bool,
    #[serde(default)]
    pub gap_kind: Option<String>,
    #[serde(default)]
    pub gap_confirmed: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ConsolidateParams {
    /// When true, run the fast (non-canonical) reflection pass instead of slow consolidation.
    #[serde(default)]
    pub fast: bool,
}
