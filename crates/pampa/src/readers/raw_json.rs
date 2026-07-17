/*
 * raw_json.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! The pampa-native `raw-json` reader (GH #11, bd-en2hvrwn) — the
//! inverse of [`crate::writers::raw_json`].
//!
//! Validates the `pampa-json-format` envelope marker (version-checked;
//! Pandoc-style JSON is rejected with a pointer at `-f json`), then reads
//! the document with the shared JSON machinery in raw mode: the full
//! extension vocabulary is accepted and metadata is decoded as an
//! order-preserving config-value node.
//!
//! Always strict about `s:` source references — raw-json is q2-internal,
//! and every writer-produced node carries one. There is no completing
//! variant; JSON from outside the q2 source-tracking world goes through
//! the Pandoc-superset reader instead.

use crate::pandoc::ASTContext;
use crate::pandoc::Pandoc;

use super::json::{JsonReadError, read_raw_pandoc};

/// Read a raw-json document.
pub fn read<R: std::io::Read>(reader: &mut R) -> Result<(Pandoc, ASTContext), JsonReadError> {
    let value: serde_json::Value =
        serde_json::from_reader(reader).map_err(JsonReadError::InvalidJson)?;
    read_raw_pandoc(&value)
}

/// Read a raw-json document from an already-parsed [`serde_json::Value`].
pub fn read_value(value: &serde_json::Value) -> Result<(Pandoc, ASTContext), JsonReadError> {
    read_raw_pandoc(value)
}
