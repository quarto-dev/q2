/*
 * raw_json.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! The pampa-native `raw-json` writer (GH #11, bd-en2hvrwn).
//!
//! Contract: `raw_json::write` followed by `readers::raw_json::read` is
//! the identity (structural equality) on the pampa AST — including the
//! extensions the Pandoc-superset `-t json` format desugars or rejects
//! (standalone `Inline::Attr`, `NoteReference`, CriticMarkup inlines,
//! `Shortcode`, `CaptionBlock`) and full-fidelity `ConfigValue` metadata.
//!
//! The output deliberately does **not** look like `pandoc -t json`
//! output to machine consumers: the `pampa-json-format` marker is the
//! first key of the envelope and `pandoc-api-version` is absent.
//!
//! Implementation-wise this is a thin entry point over the shared
//! streaming JSON machinery in [`super::json`] with [`JsonConfig::raw`]
//! set: the source-info pool, `astContext` encoding, and all standard
//! node arms are the same battle-tested code path as `-t json`.
//!
//! Design + contract details:
//! `claude-notes/plans/2026-07-17-raw-json-format.md`.

use crate::pandoc::ASTContext;
use crate::pandoc::Pandoc;
use quarto_error_reporting::DiagnosticMessage;

use super::json::JsonConfig;

/// Version of the raw-json envelope, carried in the
/// `pampa-json-format.version` marker and validated by the reader.
/// Bump on any breaking change to the wire shape; the reader rejects
/// versions it does not know.
pub const RAW_JSON_FORMAT_VERSION: u64 = 1;

/// Write `pandoc` as raw-json with a custom configuration.
///
/// `config.raw` is forced on; the other fields (inline locations,
/// attribution) compose with raw mode as they do with the Pandoc-superset
/// format.
pub fn write_with_config<W: std::io::Write>(
    pandoc: &Pandoc,
    context: &ASTContext,
    writer: &mut W,
    config: &JsonConfig,
) -> Result<(), Vec<DiagnosticMessage>> {
    let raw_config = JsonConfig {
        raw: true,
        ..config.clone()
    };
    super::json::write_with_config(pandoc, context, writer, &raw_config)
}

/// Write `pandoc` as raw-json with the default configuration.
pub fn write<W: std::io::Write>(
    pandoc: &Pandoc,
    context: &ASTContext,
    writer: &mut W,
) -> Result<(), Vec<DiagnosticMessage>> {
    write_with_config(pandoc, context, writer, &JsonConfig::default())
}
