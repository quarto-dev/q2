//! quarto-hub: Automerge-based collaborative editing infrastructure for Quarto projects
//!
//! This crate provides:
//! - A collaborative editing server for Quarto projects
//! - Automerge-based CRDT document management
//! - WebSocket sync protocol for real-time collaboration
//! - REST API for document operations

pub mod context;
pub mod discovery;
pub mod error;
pub mod index;
pub mod peer;
pub mod server;
pub mod storage;

pub use context::HubContext;
pub use error::{Error, Result};
pub use index::IndexDocument;
pub use storage::{HubStorageConfig, StorageManager, CURRENT_HUB_VERSION};
