//! quarto-hub: Automerge-based collaborative editing infrastructure for Quarto projects
//!
//! This crate provides:
//! - A collaborative editing server for Quarto projects
//! - Automerge-based CRDT document management
//! - WebSocket sync protocol for real-time collaboration
//! - REST API for document operations

pub mod admin;
pub mod auth;
pub mod context;
/// File discovery moved to `quarto-project-files` (bd-n0uth2zt) so the
/// language server classifies project files exactly as the hub does.
pub mod discovery {
    pub use quarto_project_files::discovery::*;
}
pub mod error;
pub mod git_epochs;
pub mod index;
pub mod login_state;
pub mod peer;
pub mod resource;
pub mod revocation;
pub mod server;
pub mod session;
pub mod storage;
pub mod sync;
pub mod sync_state;
pub mod unix_socket;
pub mod upstream;
pub mod watch;

// Test-only modules
#[cfg(test)]
mod automerge_api_tests;

pub use context::HubContext;
pub use error::{Error, Result};
pub use index::IndexDocument;
pub use storage::{
    CURRENT_HUB_VERSION, HubStorageConfig, StorageManager, default_standalone_data_dir,
};
