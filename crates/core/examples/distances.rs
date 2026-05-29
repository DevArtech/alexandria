// Throwaway diagnostic: prints semantic KNN distances for a query against a library.
// Usage: cargo run -p alexandria-core --example distances -- <library_path> "<query>"
use alexandria_core::{Config, Index, Library};

fn main() {
    let lib_path = std::env::args().nth(1).expect("library path");
    let query = std::env::args().nth(2).expect("query");
    let lib = Library::discover(Some(std::path::Path::new(&lib_path))).unwrap();
    let config = Config::load(&lib.root).unwrap();
    println!("embedder = {}", config.providers.embedder);
    let index = Index::open(&lib, &config).unwrap();
    let qv = index.embed_query(&query).unwrap();
    let hits = index.semantic_knn(&qv, 50).unwrap();
    println!("distance   id                        claim");
    for h in hits {
        let claim: String = h.claim.chars().take(60).collect();
        println!("{:.4}    {:24}  {}", h.distance, h.id, claim);
    }
}
