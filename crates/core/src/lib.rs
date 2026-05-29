pub mod config;
pub mod consolidate;
pub mod engram;
pub mod error;
pub mod graph;
pub mod index;
pub mod ops;
pub mod provider;
pub mod retrieval;
pub mod store;

pub use config::Config;
pub use consolidate::{consolidate_slow, ConsolidationReport};
pub use engram::{Engram, Rel, Source, Status, Tier};
pub use error::{AlexandriaError, Result};
pub use graph::{
    compute_effective_confidence, Graph, TimelineEntry, TimelineResult, TraceNode, TraceResult,
    TraverseNode, TraverseResult,
};
pub use index::{EngramRow, Index, ReindexResult, SemanticHit};
pub use ops::{ArchiveResult, LinkResult, Ops};
pub use provider::{build_embedder, embed_sync, Embedder, HashEmbedder};
pub use retrieval::{
    escape_fts_query, CollectionNode, ContextTree, ExpandResult, RecallHit, RecallResult,
    RecallState, ResponseMode, Retrieval,
};
pub use store::{Library, ParseFailure, ScanResult};
