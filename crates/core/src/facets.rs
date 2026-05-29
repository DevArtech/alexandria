//! Facet detection: match query tokens against known collections and tags.

use std::collections::HashSet;
use std::sync::OnceLock;

use serde::Serialize;

use crate::error::Result;
use crate::index::Index;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FacetKind {
    Collection,
    Tag,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DetectedFacet {
    pub kind: FacetKind,
    pub name: String,
    pub count: usize,
}

static FACET_STOPWORDS: OnceLock<HashSet<String>> = OnceLock::new();

fn stopwords() -> &'static HashSet<String> {
    FACET_STOPWORDS.get_or_init(|| {
        include_str!("stopwords_en.txt")
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.to_ascii_lowercase())
            .collect()
    })
}

fn is_stopword(token: &str) -> bool {
    stopwords().contains(&token.to_ascii_lowercase())
}

/// Tokenize a query into normalized lowercase tokens (alphanumeric segments),
/// excluding English stop words.
pub fn tokenize_query(query: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for ch in query.chars() {
        if ch.is_alphanumeric() || ch == '_' || ch == '-' || ch == '/' {
            current.push(ch.to_ascii_lowercase());
        } else if !current.is_empty() {
            if current.len() >= 2 && !is_stopword(&current) {
                tokens.push(std::mem::take(&mut current));
            } else {
                current.clear();
            }
        }
    }
    if !current.is_empty() && current.len() >= 2 && !is_stopword(&current) {
        tokens.push(current);
    }
    tokens
}

fn normalize_facet_name(name: &str) -> String {
    name.to_ascii_lowercase()
}

fn facet_segment_tokens(facet_name: &str) -> Vec<String> {
    normalize_facet_name(facet_name)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| s.len() >= 2)
        .map(String::from)
        .collect()
}

/// Whole-token match only — no substring containment (avoids "and" ⊂ "alexandria").
fn facet_matches_tokens(facet_name: &str, tokens: &[String]) -> bool {
    let normalized = normalize_facet_name(facet_name);
    let segments = facet_segment_tokens(facet_name);

    tokens.iter().any(|token| {
        *token == normalized || segments.iter().any(|seg| seg == token)
    })
}

/// Detect collections and tags whose names overlap with query tokens.
pub fn detect_facets(index: &Index, query: &str) -> Result<Vec<DetectedFacet>> {
    let tokens = tokenize_query(query);
    if tokens.is_empty() {
        return Ok(Vec::new());
    }

    let mut detected = Vec::new();

    for (name, count) in index.list_collections()? {
        if facet_matches_tokens(&name, &tokens) {
            detected.push(DetectedFacet {
                kind: FacetKind::Collection,
                name,
                count,
            });
        }
    }

    for (name, count) in index.list_tags()? {
        if facet_matches_tokens(&name, &tokens) {
            detected.push(DetectedFacet {
                kind: FacetKind::Tag,
                name,
                count,
            });
        }
    }

    detected.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
    Ok(detected)
}

/// Extract collection and tag names from detected facets.
pub fn facets_to_filters(facets: &[DetectedFacet]) -> (Vec<String>, Vec<String>) {
    let mut collections = Vec::new();
    let mut tags = Vec::new();
    for facet in facets {
        match facet.kind {
            FacetKind::Collection => collections.push(facet.name.clone()),
            FacetKind::Tag => tags.push(facet.name.clone()),
        }
    }
    (collections, tags)
}

/// Dominant facet for domain/meta-memory lookup (highest count).
pub fn dominant_facet(facets: &[DetectedFacet]) -> Option<&DetectedFacet> {
    facets.first()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_splits_on_punctuation() {
        let tokens = tokenize_query("auth in project X");
        assert!(tokens.contains(&"auth".to_string()));
        assert!(tokens.contains(&"project".to_string()));
        assert!(!tokens.contains(&"in".to_string()));
    }

    #[test]
    fn facet_matches_hatco() {
        assert!(facet_matches_tokens("hatco", &tokenize_query("tell me about Hatco")));
        assert!(facet_matches_tokens(
            "meridian/cartographer",
            &tokenize_query("cartographer agent")
        ));
    }

    #[test]
    fn stopword_does_not_match_substring_of_facet() {
        let tokens = tokenize_query("cats and dogs");
        assert!(!tokens.contains(&"and".to_string()));
        assert!(!facet_matches_tokens("alexandria", &tokens));
    }

    #[test]
    fn whole_token_match_required() {
        assert!(!facet_matches_tokens("hatco", &tokenize_query("that")));
    }
}
