use alexandria_core::{
    build_embedder, Config, Engram, Graph, Index, Library, Ops, RecallState, Rel, Retrieval,
    Source, Status, Tier,
};
use tempfile::TempDir;

fn test_config(dir: &TempDir) -> Config {
    let mut config = Config::load(dir.path()).unwrap();
    config.providers.embedder = "hash".into();
    config
}

fn open_index(lib: &Library, config: &Config) -> Index {
    let embedder = build_embedder(config).unwrap();
    Index::open_with_embedder(lib, embedder).unwrap()
}

#[test]
fn init_remember_recall_flow() {
    let dir = TempDir::new().unwrap();
    let lib = Library::init(dir.path()).unwrap();
    let config = test_config(&dir);

    let e1 = Engram::new(
        "Alexandria uses hybrid fused retrieval",
        "Vector-only retrieval fails on exact recall.",
        Tier::Semantic,
        Status::Confirmed,
    );
    let e2 = Engram::new(
        "Rust is the target runtime for Alexandria",
        "Single binary, local-first.",
        Tier::Semantic,
        Status::Confirmed,
    );

    let p1 = lib.write_engram(&e1).unwrap();
    let p2 = lib.write_engram(&e2).unwrap();

    let index = open_index(&lib, &config);
    index.upsert(&e1, &p1.display().to_string()).unwrap();
    index.upsert(&e2, &p2.display().to_string()).unwrap();

    let retrieval = Retrieval::new(&index, &config);
    let result = retrieval.recall("retrieval hybrid", Some(2000)).unwrap();

    let hits: Vec<_> = result
        .tree
        .collections
        .iter()
        .flat_map(|c| c.hits.iter())
        .collect();
    assert!(!hits.is_empty() || result.state == RecallState::HighConfidenceGap);
    if !hits.is_empty() {
        assert!(hits.iter().any(|h| h.claim.contains("hybrid fused retrieval")));
    }
}

#[test]
fn reindex_rebuilds_from_text_after_db_deleted() {
    let dir = TempDir::new().unwrap();
    let lib = Library::init(dir.path()).unwrap();
    let config = test_config(&dir);

    let engram = Engram::new("reindex test claim", "body", Tier::Semantic, Status::Confirmed);
    let path = lib.write_engram(&engram).unwrap();

    let index = open_index(&lib, &config);
    index.upsert(&engram, &path.display().to_string()).unwrap();

    let db_path = lib.index_path();
    assert!(db_path.exists());
    std::fs::remove_file(&db_path).unwrap();

    let index2 = open_index(&lib, &config);
    let result = index2.reindex(&lib).unwrap();
    assert_eq!(result.indexed, 1);
    assert!(result.parse_failures.is_empty());

    let retrieval = Retrieval::new(&index2, &config);
    let recall = retrieval.recall("reindex test", Some(2000)).unwrap();
    let hits: Vec<_> = recall
        .tree
        .collections
        .iter()
        .flat_map(|c| c.hits.iter())
        .collect();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, engram.id);
}

#[test]
fn reindex_reports_parse_failures() {
    let dir = TempDir::new().unwrap();
    let lib = Library::init(dir.path()).unwrap();
    let config = test_config(&dir);
    std::fs::write(
        lib.root.join("semantic/corrupt.md"),
        "no frontmatter here\n",
    )
    .unwrap();

    let index = open_index(&lib, &config);
    let result = index.reindex(&lib).unwrap();
    assert_eq!(result.indexed, 0);
    assert_eq!(result.parse_failures.len(), 1);
}

#[test]
fn embedder_change_invalidates_vec_index() {
    let dir = TempDir::new().unwrap();
    let lib = Library::init(dir.path()).unwrap();
    let config = test_config(&dir);

    let engram = Engram::new("embedder invalidation", "body", Tier::Semantic, Status::Confirmed);
    let path = lib.write_engram(&engram).unwrap();

    let index = open_index(&lib, &config);
    index.upsert(&engram, &path.display().to_string()).unwrap();

    index
        .connection()
        .execute(
            "UPDATE index_meta SET value = 'stale:embedder' WHERE key = 'embedder_id'",
            [],
        )
        .unwrap();

    let index2 = open_index(&lib, &config);
    assert_eq!(index2.embedder_id().unwrap(), "hash:v1");
    index2.reindex(&lib).unwrap();

    let recall = Retrieval::new(&index2, &config)
        .recall("embedder invalidation", Some(2000))
        .unwrap();
    let hits: Vec<_> = recall
        .tree
        .collections
        .iter()
        .flat_map(|c| c.hits.iter())
        .collect();
    assert!(!hits.is_empty());
}

#[test]
fn expand_and_relational_suppression() {
    let dir = TempDir::new().unwrap();
    let lib = Library::init(dir.path()).unwrap();
    let config = test_config(&dir);
    let index = open_index(&lib, &config);

    let rel = Engram::new("prefers terse", "body", Tier::Relational, Status::Confirmed);
    let p = lib.write_engram(&rel).unwrap();
    index.upsert(&rel, &p.display().to_string()).unwrap();

    let retrieval = Retrieval::new(&index, &config);
    assert!(retrieval.expand(&rel.id, None).is_err());
}

#[test]
fn reindex_rebuilds_sources_table() {
    let dir = TempDir::new().unwrap();
    let lib = Library::init(dir.path()).unwrap();
    let config = test_config(&dir);

    let mut engram = Engram::new("derived fact", "body", Tier::Semantic, Status::Confirmed);
    engram.source.push(Source {
        kind: "conversation".into(),
        r#ref: "conv_2026".into(),
    });
    let path = lib.write_engram(&engram).unwrap();

    let index = open_index(&lib, &config);
    index.upsert(&engram, &path.display().to_string()).unwrap();

    std::fs::remove_file(lib.index_path()).unwrap();
    let index2 = open_index(&lib, &config);
    index2.reindex(&lib).unwrap();

    let count: i64 = index2
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM sources WHERE engram_id = ?1",
            [&engram.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);

    let graph = Graph::new(&index2);
    let trace = graph.trace(&engram.id).unwrap();
    assert_eq!(trace.nodes.len(), 1);
    assert_eq!(trace.nodes[0].source_kind, "conversation");
}

#[test]
fn link_and_trace_flow() {
    let dir = TempDir::new().unwrap();
    let lib = Library::init(dir.path()).unwrap();
    let config = test_config(&dir);
    let index = open_index(&lib, &config);

    let a = Engram::new("parent", "body", Tier::Semantic, Status::Confirmed);
    let b = Engram::new("child", "body", Tier::Semantic, Status::Confirmed);
    let pa = lib.write_engram(&a).unwrap();
    let pb = lib.write_engram(&b).unwrap();
    index.upsert(&a, &pa.display().to_string()).unwrap();
    index.upsert(&b, &pb.display().to_string()).unwrap();

    let ops = Ops::new(&lib, &index);
    ops.link(&a.id, Rel::Supports, &b.id).unwrap();

    let graph = Graph::new(&index);
    let walk = graph
        .traverse(&a.id, Some(&[Rel::Supports]), 1)
        .unwrap();
    assert_eq!(walk.nodes.len(), 1);
    assert_eq!(walk.nodes[0].id, b.id);
}
