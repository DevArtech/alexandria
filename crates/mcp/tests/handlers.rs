use alexandria_core::{build_embedder, Config, Engram, Index, Library, Status, Tier};
use alexandria_mcp::handlers::{expand, recall, remember, style, ServerState};
use alexandria_mcp::params::{ExpandParams, RecallParams, RememberParams};
use tempfile::TempDir;

fn test_state(dir: &TempDir) -> ServerState {
    let lib = Library::init(dir.path()).unwrap();
    let mut config = Config::load(dir.path()).unwrap();
    config.providers.embedder = "hash".into();
    let embedder = build_embedder(&config).unwrap();
    let index = Index::open_with_embedder(&lib, embedder).unwrap();
    ServerState {
        library: lib,
        config,
        index,
    }
}

#[test]
fn remember_recall_round_trip() {
    let dir = TempDir::new().unwrap();
    let mut state = test_state(&dir);

    let remembered = remember(
        &mut state,
        RememberParams {
            text: "Alexandria uses hybrid fused retrieval\nNot vector-only.".into(),
            tier: Some("semantic".into()),
            status: None,
            collections: vec!["core".into()],
            tags: vec![],
            sources: vec![],
            derived_from: vec![],
            surface_when: vec![],
        },
    )
    .unwrap();
    assert!(remembered.get("id").is_some());

    let recalled = recall(
        &state,
        RecallParams {
            query: "hybrid retrieval".into(),
            budget: Some(2000),
            audit: false,
            high_stakes: false,
            domain: None,
        },
    )
    .unwrap();
    let state_str = recalled.get("state").and_then(|v| v.as_str()).unwrap_or("nothing");
    assert_ne!(state_str, "nothing");
}

#[test]
fn expand_excludes_relational() {
    let dir = TempDir::new().unwrap();
    let state = test_state(&dir);

    let rel = Engram::new(
        "User prefers terse answers",
        "Never quote this body.",
        Tier::Relational,
        Status::Confirmed,
    );
    let path = state.library.write_engram(&rel).unwrap();
    state
        .index
        .upsert(&rel, &path.display().to_string())
        .unwrap();

    let recalled = recall(
        &state,
        RecallParams {
            query: "terse answers".into(),
            budget: Some(2000),
            audit: false,
            high_stakes: false,
            domain: None,
        },
    )
    .unwrap();

    // Relational should not appear in recall tree hits
    if let Some(collections) = recalled.get("tree").and_then(|t| t.get("collections")) {
        if let Some(arr) = collections.as_array() {
            for collection in arr {
                if let Some(hits) = collection.get("hits").and_then(|h| h.as_array()) {
                    for hit in hits {
                        let claim = hit.get("claim").and_then(|c| c.as_str()).unwrap_or("");
                        assert!(!claim.contains("terse answers"));
                    }
                }
            }
        }
    }

    let expanded = expand(
        &state,
        ExpandParams {
            id: rel.id.clone(),
            rel: None,
        },
    );
    assert!(expanded.is_err() || expanded.unwrap().get("id").is_none());
}

#[test]
fn style_returns_profile_without_bodies() {
    let dir = TempDir::new().unwrap();
    let state = test_state(&dir);
    let profile = style(&state).unwrap();
    assert!(profile.get("verbosity").is_some());
    assert!(profile.get("directness").is_some());
    assert!(profile.as_object().unwrap().values().all(|v| {
        !v.as_str().is_some_and(|s| s.contains("Never quote"))
    }));
}

#[test]
fn recall_state_is_valid_enum() {
    let dir = TempDir::new().unwrap();
    let mut state = test_state(&dir);
    remember(
        &mut state,
        RememberParams {
            text: "Rust is the implementation language".into(),
            tier: None,
            status: None,
            collections: vec![],
            tags: vec![],
            sources: vec![],
            derived_from: vec![],
            surface_when: vec![],
        },
    )
    .unwrap();

    let recalled = recall(
        &state,
        RecallParams {
            query: "implementation language".into(),
            budget: None,
            audit: false,
            high_stakes: false,
            domain: None,
        },
    )
    .unwrap();

    let state_str = recalled.get("state").and_then(|v| v.as_str()).unwrap();
    assert!(
        matches!(
            state_str,
            "strong_hit" | "weak_hit" | "high_confidence_gap" | "low_confidence_gap" | "nothing"
        ),
        "unexpected state: {state_str}"
    );

    assert!(recalled.get("response_mode").is_some());
}
