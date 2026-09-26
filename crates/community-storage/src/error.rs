use community_core::CommunityError;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("integrity: {0}")]
    Integrity(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid event: {0}")]
    InvalidEvent(String),
    #[error("sequence conflict: {0}")]
    SequenceConflict(String),
    #[error("policy: {0}")]
    Policy(String),
    #[error("encryption: {0}")]
    Encryption(String),
    #[error("object tombstoned: {0}")]
    Tombstoned(String),
    #[error("schema: {0}")]
    Schema(String),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Other(String),
}

impl From<StorageError> for CommunityError {
    fn from(e: StorageError) -> Self {
        match e {
            StorageError::Integrity(s) | StorageError::InvalidEvent(s) => {
                CommunityError::Security(s)
            }
            StorageError::Encryption(s) => CommunityError::Security(s),
            other => CommunityError::Config(other.to_string()),
        }
    }
}

pub type Result<T> = std::result::Result<T, StorageError>;
