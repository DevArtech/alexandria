use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{AlexandriaError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Working,
    Episodic,
    Provisional,
    Semantic,
    Procedural,
    Relational,
}

impl Tier {
    pub fn dir_name(self) -> Option<&'static str> {
        match self {
            Tier::Working => None,
            Tier::Episodic => Some("episodic"),
            Tier::Provisional => Some("provisional"),
            Tier::Semantic => Some("semantic"),
            Tier::Procedural => Some("procedural"),
            Tier::Relational => Some("relational"),
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "working" => Ok(Tier::Working),
            "episodic" => Ok(Tier::Episodic),
            "provisional" => Ok(Tier::Provisional),
            "semantic" => Ok(Tier::Semantic),
            "procedural" => Ok(Tier::Procedural),
            "relational" => Ok(Tier::Relational),
            _ => Err(AlexandriaError::InvalidEngram(format!("unknown tier: {s}"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Confirmed,
    Provisional,
    UnresolvedByDesign,
    Superseded,
    Archived,
}

impl Status {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "confirmed" => Ok(Status::Confirmed),
            "provisional" => Ok(Status::Provisional),
            "unresolved_by_design" => Ok(Status::UnresolvedByDesign),
            "superseded" => Ok(Status::Superseded),
            "archived" => Ok(Status::Archived),
            _ => Err(AlexandriaError::InvalidEngram(format!("unknown status: {s}"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rel {
    Supports,
    Refines,
    DependsOn,
    CausedBy,
    ConflictsConfirmed,
    TensionPossible,
    ContextQualified,
    Coexists,
    Supersedes,
    SupersededBy,
    AspectOf,
    SameEpisode,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Source {
    pub kind: String,
    pub r#ref: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Link {
    pub rel: Rel,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngramFrontmatter {
    pub id: String,
    pub tier: Tier,
    pub status: Status,
    pub claim: String,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
    pub last_touched: DateTime<Utc>,
    #[serde(default)]
    pub source: Vec<Source>,
    #[serde(default = "default_confidence")]
    pub confidence: f64,
    #[serde(default = "default_salience")]
    pub salience: f64,
    #[serde(default)]
    pub collections: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub links: Vec<Link>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_when: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_policy: Option<String>,
}

fn default_confidence() -> f64 {
    0.9
}

fn default_salience() -> f64 {
    0.7
}

#[derive(Debug, Clone, PartialEq)]
pub struct Engram {
    pub id: String,
    pub tier: Tier,
    pub status: Status,
    pub claim: String,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
    pub last_touched: DateTime<Utc>,
    pub source: Vec<Source>,
    pub confidence: f64,
    pub salience: f64,
    pub collections: Vec<String>,
    pub tags: Vec<String>,
    pub links: Vec<Link>,
    pub embedding_ref: Option<String>,
    pub shape_ref: Option<String>,
    pub surface_when: Option<Vec<String>>,
    pub output_policy: Option<String>,
    pub body: String,
}

impl Engram {
    pub fn generate_id(claim: &str, created: &DateTime<Utc>) -> String {
        let mut hasher = Sha256::new();
        hasher.update(claim.as_bytes());
        hasher.update(created.to_rfc3339().as_bytes());
        let hash = hasher.finalize();
        format!("eng_{}", hex::encode(&hash[..8]))
    }

    pub fn new(
        claim: impl Into<String>,
        body: impl Into<String>,
        tier: Tier,
        status: Status,
    ) -> Self {
        let now = Utc::now();
        let claim = claim.into();
        let id = Self::generate_id(&claim, &now);
        Self {
            id,
            tier,
            status,
            claim,
            created: now,
            updated: now,
            last_touched: now,
            source: Vec::new(),
            confidence: default_confidence(),
            salience: default_salience(),
            collections: Vec::new(),
            tags: Vec::new(),
            links: Vec::new(),
            embedding_ref: None,
            shape_ref: None,
            surface_when: None,
            output_policy: if tier == Tier::Relational {
                Some("generation_only".to_string())
            } else {
                None
            },
            body: body.into(),
        }
    }

    pub fn to_frontmatter(&self) -> EngramFrontmatter {
        EngramFrontmatter {
            id: self.id.clone(),
            tier: self.tier,
            status: self.status,
            claim: self.claim.clone(),
            created: self.created,
            updated: self.updated,
            last_touched: self.last_touched,
            source: self.source.clone(),
            confidence: self.confidence,
            salience: self.salience,
            collections: self.collections.clone(),
            tags: self.tags.clone(),
            links: self.links.clone(),
            embedding_ref: self.embedding_ref.clone(),
            shape_ref: self.shape_ref.clone(),
            surface_when: self.surface_when.clone(),
            output_policy: self.output_policy.clone(),
        }
    }

    pub fn from_frontmatter(fm: EngramFrontmatter, body: String) -> Self {
        Self {
            id: fm.id,
            tier: fm.tier,
            status: fm.status,
            claim: fm.claim,
            created: fm.created,
            updated: fm.updated,
            last_touched: fm.last_touched,
            source: fm.source,
            confidence: fm.confidence,
            salience: fm.salience,
            collections: fm.collections,
            tags: fm.tags,
            links: fm.links,
            embedding_ref: fm.embedding_ref,
            shape_ref: fm.shape_ref,
            surface_when: fm.surface_when,
            output_policy: fm.output_policy,
            body,
        }
    }

    pub fn serialize(&self) -> Result<String> {
        let yaml = serde_yaml::to_string(&self.to_frontmatter())
            .map_err(|e| AlexandriaError::InvalidEngram(e.to_string()))?;
        let body = self.body.trim();
        if body.is_empty() {
            Ok(format!("---\n{yaml}---\n"))
        } else {
            Ok(format!("---\n{yaml}---\n\n{body}\n"))
        }
    }

    pub fn parse(content: &str) -> Result<Self> {
        let content = content.trim();
        if !content.starts_with("---") {
            return Err(AlexandriaError::InvalidEngram(
                "missing YAML frontmatter".into(),
            ));
        }
        let rest = &content[3..];
        let end = rest
            .find("\n---")
            .ok_or_else(|| AlexandriaError::InvalidEngram("unclosed frontmatter".into()))?;
        let yaml = rest[..end].trim_start_matches('\n');
        let body = rest[end + 4..].trim_start_matches('\n').trim().to_string();
        let fm: EngramFrontmatter = serde_yaml::from_str(yaml)
            .map_err(|e| AlexandriaError::InvalidEngram(e.to_string()))?;
        Ok(Self::from_frontmatter(fm, body))
    }

    pub fn estimate_tokens(text: &str) -> u32 {
        ((text.chars().count() as f64) / 4.0).ceil() as u32
    }
}

// hex encoding helper (avoid extra dep for just 6 chars)
mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_is_stable_for_same_input() {
        let created = Utc::now();
        let a = Engram::generate_id("test claim", &created);
        let b = Engram::generate_id("test claim", &created);
        assert_eq!(a, b);
        assert!(a.starts_with("eng_"));
        assert_eq!(a.len(), 20); // eng_ + 16 hex (8 bytes)
    }

    #[test]
    fn frontmatter_round_trip() {
        let engram = Engram::new(
            "Alexandria uses hybrid fused retrieval",
            "Vector-only retrieval fails on exact recall.",
            Tier::Semantic,
            Status::Confirmed,
        );
        let serialized = engram.serialize().unwrap();
        let parsed = Engram::parse(&serialized).unwrap();
        assert_eq!(parsed.id, engram.id);
        assert_eq!(parsed.claim, engram.claim);
        assert_eq!(parsed.tier, Tier::Semantic);
        assert_eq!(parsed.body.trim(), engram.body.trim());
    }

    #[test]
    fn relational_gets_output_policy() {
        let engram = Engram::new("prefers terse answers", "", Tier::Relational, Status::Confirmed);
        assert_eq!(engram.output_policy.as_deref(), Some("generation_only"));
    }
}
