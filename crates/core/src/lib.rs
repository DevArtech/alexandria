pub mod config;
pub mod consolidate;
pub mod engram;
pub mod error;
pub mod graph;
pub mod index;
pub mod meta;
pub mod ops;
pub mod provider;
pub mod retrieval;
pub mod shape;
pub mod store;
pub mod style;
pub mod threads;

pub use config::Config;
pub use consolidate::{
    consolidate_fast, consolidate_slow, ConsolidationReport, FastReflectionReport,
};
pub use engram::{Engram, Rel, Source, Status, Tier};
pub use error::{AlexandriaError, Result};
pub use graph::{
    compute_effective_confidence, Graph, TimelineEntry, TimelineResult, TraceNode, TraceResult,
    TraverseNode, TraverseResult,
};
pub use index::{EngramRow, Index, ReindexResult, SemanticHit};
pub use meta::{
    append_meta_event, load_meta_events, meta_report, record_correction, record_gap_outcome,
    record_promotion_reversal, rebuild_meta_index, MetaLogEvent, MetaReport,
};
pub use ops::{ArchiveResult, LinkResult, Ops};
pub use provider::{
    build_completer, build_embedder, build_embedder_with_dim_hint, build_reranker, embed_sync,
    predict_embedder_id, Completer, Embedder, HashEmbedder, Prompt, Reranker,
};
pub use retrieval::{
    escape_fts_query, CollectionNode, ContextTree, ExpandResult, RecallHit, RecallOptions,
    RecallResult, RecallState, ResponseMode, Retrieval,
};
pub use shape::extract_shape_summary_heuristic;
pub use store::{Library, ParseFailure, ScanResult};
pub use style::{style_profile, StyleProfile};
pub use threads::{list_threads, ThreadEntry, ThreadsResult};
