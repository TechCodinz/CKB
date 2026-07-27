//! Error types for CKB Core

use thiserror::Error;

#[derive(Error, Debug)]
pub enum CkbError {
    #[error("Failed to parse file: {path}")]
    ParseError {
        path: String,
        #[source]
        source: anyhow::Error,
    },

    #[error("Unsupported file type: {extension}")]
    UnsupportedFileType { extension: String },

    #[error("Graph operation failed: {0}")]
    GraphError(String),

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Node not found: {0}")]
    NodeNotFound(String),

    #[error("Snapshot not found: {0}")]
    SnapshotNotFound(String),

    #[error("Analysis failed: {0}")]
    AnalysisError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Serialization(#[from] serde_json::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type CkbResult<T> = Result<T, CkbError>;
