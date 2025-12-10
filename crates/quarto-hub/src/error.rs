//! Error types for quarto-hub

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Hub is already running (lockfile held by another process)")]
    HubAlreadyRunning,

    #[error("Project directory not found: {0}")]
    ProjectNotFound(PathBuf),

    #[error("Failed to create hub directory: {0}")]
    CreateHubDir(#[source] std::io::Error),

    #[error("Failed to acquire lockfile: {0}")]
    LockfileAcquire(#[source] std::io::Error),

    #[error("Failed to parse hub config: {0}")]
    ConfigParse(String),

    #[error(
        "Hub storage version {found} is newer than supported version {supported}. Please upgrade quarto-hub."
    )]
    ConfigVersionTooNew { found: u32, supported: u32 },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Server error: {0}")]
    Server(String),

    #[error("Automerge error: {0}")]
    Automerge(String),

    #[error("Index document error: {0}")]
    IndexDocument(String),

    #[error("Sync state error: {0}")]
    SyncState(String),

    #[error("Sync error: {0}")]
    Sync(String),
}

pub type Result<T> = std::result::Result<T, Error>;
