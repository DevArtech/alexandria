use alexandria_core::{Config, Engram, Index, Library, RecallState, Retrieval, Status, Tier};
use tempfile::TempDir;

#[test]
fn init_remember_recall_flow() {
    let dir = TempDir::new().unwrap();
    let lib = Library::init(dir.path()).unwrap();

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

    let index = Index::open(&lib).unwrap();
    index.upsert(&e1, &p1.display().to_string()).unwrap();
    index.upsert(&e2, &p2.display().to_string()).unwrap();

    let config = Config::load(dir.path()).unwrap();
    let retrieval = Retrieval::new(&index, &config);
    let result = retrieval.recall("retrieval hybrid", Some(2000)).unwrap();

    assert!(!result.engrams.is_empty());
    assert_ne!(result.state, RecallState::Nothing);
    assert!(result
        .engrams
        .iter()
        .any(|h| h.claim.contains("hybrid fused retrieval")));
}

#[test]
fn reindex_rebuilds_from_text_after_db_deleted() {
    let dir = TempDir::new().unwrap();
    let lib = Library::init(dir.path()).unwrap();

    let engram = Engram::new("reindex test claim", "body", Tier::Semantic, Status::Confirmed);
    let path = lib.write_engram(&engram).unwrap();

    let index = Index::open(&lib).unwrap();
    index.upsert(&engram, &path.display().to_string()).unwrap();

    let db_path = lib.index_path();
    assert!(db_path.exists());
    std::fs::remove_file(&db_path).unwrap();

    let index2 = Index::open(&lib).unwrap();
    let result = index2.reindex(&lib).unwrap();
    assert_eq!(result.indexed, 1);
    assert!(result.parse_failures.is_empty());

    let config = Config::load(dir.path()).unwrap();
    let retrieval = Retrieval::new(&index2, &config);
    let recall = retrieval.recall("reindex test", Some(2000)).unwrap();
    assert_eq!(recall.engrams.len(), 1);
    assert_eq!(recall.engrams[0].id, engram.id);
}

#[test]
fn reindex_reports_parse_failures() {
    let dir = TempDir::new().unwrap();
    let lib = Library::init(dir.path()).unwrap();
    std::fs::write(
        lib.root.join("semantic/corrupt.md"),
        "no frontmatter here\n",
    )
    .unwrap();

    let index = Index::open(&lib).unwrap();
    let result = index.reindex(&lib).unwrap();
    assert_eq!(result.indexed, 0);
    assert_eq!(result.parse_failures.len(), 1);
}
