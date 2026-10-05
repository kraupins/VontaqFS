use std::io;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("PATH_INVALID: {0}")]
    PathInvalid(String),
    #[error("PATH_CONFLICT: {0}")]
    PathConflict(String),
    #[error("NOT_FOUND: {0}")]
    NotFound(String),
    #[error("CONFLICT: {0}")]
    Conflict(String),
    #[error("STORAGE_UNAVAILABLE: {0}")]
    StorageUnavailable(String),
    #[error("DISK_SPACE_LOW: {0}")]
    DiskSpaceLow(String),
    #[error("STORAGE_CORRUPT: {0}")]
    StorageCorrupt(String),
    #[error("STORAGE_SCHEMA_UNSUPPORTED: registry schema version {0}")]
    SchemaUnsupported(i64),
    #[error("REQUEST_INVALID: {0}")]
    RequestInvalid(String),
    #[error("INTERNAL_ERROR: {0}")]
    Internal(String),
    #[error("OPERATION_CANCELLED: operation cancelled")]
    OperationCancelled,
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type StorageResult<T> = Result<T, StorageError>;
