/*
 * attribution/resolve.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Chain-resolve a [`SourceInfo`] to a `(file_id, start, end)` byte
//! range in the root source file.
//!
//! Re-exports [`SourceInfo::resolve_byte_range`](quarto_source_map::SourceInfo::resolve_byte_range)
//! as a free function so callers that came in via the attribution
//! crate path (and the Lua host binding in `pampa`) read it from a
//! consistent location. The implementation lives in
//! [`quarto_source_map`] because it's a pure utility on `SourceInfo`
//! and `pampa` would otherwise have to duplicate it (no cross-crate
//! `pampa → quarto-core` dependency).

use quarto_source_map::SourceInfo;

/// Chain-resolve a [`SourceInfo`] to `(file_id, start_offset,
/// end_offset)` in the root file without requiring a
/// [`SourceContext`](quarto_source_map::SourceContext).
///
/// Returns `None` for `Concat` and `FilterProvenance` — these don't
/// map cleanly to a single contiguous byte range in v1, and any node
/// originating from a Lua filter has no source-file provenance to
/// attribute against.
pub fn resolve_byte_range(si: &SourceInfo) -> Option<(usize, usize, usize)> {
    si.resolve_byte_range()
}
