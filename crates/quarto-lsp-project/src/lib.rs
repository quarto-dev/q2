//! Project-aware analysis sessions for the Quarto language server
//! (epic bd-3f6ijnw9, child C3).
//!
//! A [`Workspace`] holds one session per project (or per lone document)
//! and every client's open buffers. It is a **sans-IO state machine with a
//! typed query API**, not an LSP server:
//!
//! - a host feeds it [`Input`]s (buffer events, file changes from a feeder,
//!   finished background [`Job`]s) and carries out its [`Output`]s (push
//!   diagnostics, watch a root, run a job);
//! - queries ([`Workspace::document`], [`Workspace::document_symbols`])
//!   are plain method calls that analyse on demand;
//! - all file access goes through the session's
//!   [`quarto_core::session_runtime::SessionRuntime`]: open buffers over a
//!   base layer over the inner runtime.
//!
//! `quarto-lsp` is the native LSP host (JSON-RPC over stdio). A server
//! manager, the preview, or the browser can host the same `Workspace`
//! without speaking LSP (plan: "C3 design", decision R2).
//!
//! Caching is **pull-validated**: a document's result keeps the read log of
//! the run that produced it, and is reused iff every file in that log still
//! has the version it saw. That covers includes, `_metadata.yml` layers,
//! unsaved config edits and files that appear, with no dependency graph.
//!
//! Plan: `claude-notes/plans/2026-11-16-lsp-document-profile-requests.md`.

mod session;
mod uri;
mod workspace;

pub use session::{DocumentResult, SessionId};
pub use uri::DocUri;
pub use workspace::{
    BaseLoader, ClientEvent, ClientId, FileChange, Input, Job, JobOutput, Output, Workspace,
    WorkspaceConfig,
};
