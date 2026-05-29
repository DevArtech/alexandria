use thiserror::Error;

#[derive(Debug, Error)]
pub enum AlexandriaError {
    #[error("library not found: no .alexandria/ directory in {0} or ancestors")]
    LibraryNotFound(String),

    #[error("library already initialized at {0}")]
    LibraryAlreadyExists(String),

    #[error("engram not found: {0}")]
    EngramNotFound(String),

    #[error("invalid engram: {0}")]
    InvalidEngram(String),

    #[error("engram id collision: {id} already exists at {path} (existing claim: {existing_claim})")]
    IdCollision {
        id: String,
        path: String,
        existing_claim: String,
    },

    #[error("tier {0} cannot be persisted")]
    EphemeralTier(String),

    #[error("config error: {0}")]
    Config(String),

    #[error("provider error: {0}")]
    Provider(String),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, AlexandriaError>;
