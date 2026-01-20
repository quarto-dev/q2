//! Quarto Language Server Protocol implementation.
//!
//! This crate provides the LSP server for Quarto documents, wrapping
//! `quarto-lsp-core` with the tower-lsp framework.
//!
//! # Architecture
//!
//! ```text
//! ┌───────────────────────────────────────────────────────────────┐
//! │                         quarto-lsp                             │
//! │   tower-lsp wrapper, JSON-RPC/stdio, `quarto lsp` command     │
//! │                                                                │
//! │  ┌─────────────┐  ┌─────────────┐  ┌───────────────────────┐  │
//! │  │  server.rs  │  │ convert.rs  │  │    capabilities.rs    │  │
//! │  │ LanguageServer│ │Core ↔ LSP  │  │  Capability negotiation│  │
//! │  └──────┬──────┘  └──────┬──────┘  └───────────────────────┘  │
//! │         │                │                                     │
//! │         └────────────────┴──────────────────┐                  │
//! │                                             │                  │
//! │  ┌──────────────────────────────────────────▼───────────────┐  │
//! │  │                    quarto-lsp-core                        │  │
//! │  │         (Transport-agnostic analysis logic)               │  │
//! │  └───────────────────────────────────────────────────────────┘  │
//! └───────────────────────────────────────────────────────────────┘
//! ```
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
//! quarto_lsp::run_server().await;
//! ```

pub mod capabilities;
pub mod convert;
pub mod server;

pub use server::run_server;
