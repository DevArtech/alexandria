pub mod config;
pub mod engram;
pub mod error;
pub mod index;
pub mod provider;
pub mod retrieval;
pub mod store;

pub use config::Config;
pub use engram::{Engram, Rel, Status, Tier};
pub use error::{AlexandriaError, Result};
pub use index::{EngramRow, Index, ReindexResult, SemanticHit};
pub use provider::{build_embedder, embed_sync, Embedder, HashEmbedder};
pub use retrieval::{
    escape_fts_query, CollectionNode, ContextTree, ExpandResult, RecallHit, RecallResult,
    RecallState, ResponseMode, Retrieval,
};
pub use store::{Library, ParseFailure, ScanResult};
