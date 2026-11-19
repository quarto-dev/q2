//! Quarto Language Server Protocol implementation.
//!
//! This crate provides the LSP server for Quarto documents, wrapping
//! a project-aware `quarto-lsp-project` workspace with `lsp-server` (JSON-RPC over stdio).
//!
//! # Architecture
//!
//! ```text
//!  client ──JSON-RPC/stdio──▶ quarto-lsp (lsp-server loop: server.rs)
//!                               │ Input / queries        ▲ Output
//!                               ▼                        │
//!                     quarto-lsp-project::Workspace (sans-IO sessions)
//!                               │
//!                     quarto-lsp-core (per-document analysis)
//! ```
//!
//! `server.rs` is a thin host: it translates LSP messages to workspace
//! inputs and queries, publishes diagnostics, watches project roots, and
//! runs background jobs on rayon. All analysis state lives in the
//! [`quarto_lsp_project::Workspace`] (epic bd-3f6ijnw9, C3).
//!
//! # Usage
//!
//! The LSP server is invoked via the `quarto lsp` subcommand:
//!
//! ```bash
//! quarto lsp
//! ```
//!
//! Or programmatically:
//!
//! ```rust,ignore
//! quarto_lsp::run_server()?;
//! ```

pub mod capabilities;
pub mod convert;
pub mod preview;
pub mod semantic_tokens;
pub mod server;
pub mod stdio;

pub use server::run_server;
