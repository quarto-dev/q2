//! A hub peer that answers document-structure questions over LSP.
//!
//! `q2 hub-lsp` joins a hub's sync websocket as a samod client peer, follows
//! the projects its LSP client names as workspace folders, and answers
//! custom requests such as `quarto/projectComments` over stdio. See
//! `claude-notes/plans/2026-11-09-hub-lsp-analysis-peer.md`.

pub mod comments;
pub mod server;
pub mod workspace;
