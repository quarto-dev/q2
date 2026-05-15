/*
 * attribution/mode.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! `AttributionMode` enum (CLI/YAML opt-in signal) and the
//! `(cli, yaml) → resolved` resolution function pinned by Phase 0
//! test #9b.
//!
//! Lives in `quarto-core` so CLI (`quarto`) and YAML
//! (`quarto-yaml-validation` reachable from a document's merged
//! metadata) both depend on the same type. The `clap::ValueEnum`
//! derive is gated behind the `clap` cargo feature, enabled by the
//! `quarto` CLI crate; other consumers (including the WASM build)
//! keep clap out of their dep tree.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[serde(rename_all = "kebab-case")]
pub enum AttributionMode {
    Off,
    Git,
}

/// Resolve CLI override and document/project YAML into a single
/// effective mode.
///
/// Rules (pinned by Phase 0 test #9b):
/// - CLI value, if `Some`, always wins (including `Off` — the escape
///   hatch over `attribution: git` in project YAML).
/// - Otherwise the YAML value, if `Some`.
/// - Otherwise `None` (off by default; unflagged behaviour).
pub fn resolve_attribution_mode(
    cli: Option<AttributionMode>,
    yaml: Option<AttributionMode>,
) -> Option<AttributionMode> {
    cli.or(yaml)
}
