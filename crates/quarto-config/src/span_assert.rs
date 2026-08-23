/*
 * quarto-config/src/span_assert.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Test-only helpers for asserting that a SourceInfo points where it should.
 */

//! Resolve a [`SourceInfo`] to the concrete source text it claims to cover,
//! so tests can assert on *where a diagnostic points* rather than only on
//! which code it carries.
//!
//! # Why this exists
//!
//! At the time this module was added, `crates/` held 154 assertions on a
//! diagnostic's `code` and roughly a dozen that touched its location. That
//! asymmetry is why bd-9yh3pzfu survived: `Q-12-7`'s caret underlined an
//! unrelated sibling key for as long as the diagnostic existed, and the
//! test covering it —
//!
//! ```ignore
//! assert_eq!(diags[0].code.as_deref(), Some("Q-12-7"));
//! ```
//!
//! — passed the whole time. A code-only assertion cannot fail on a wrong
//! span.
//!
//! # Why it refuses to be lenient
//!
//! [`SourceInfo`]'s `Default` is `Original { file_id: FileId(0), 0..0 }` —
//! a well-formed span that is indistinguishable downstream from a genuine
//! location at the first byte of file 0. A helper that quietly rendered it
//! as `line 1, column 1` would reproduce, inside the test suite, exactly
//! the failure mode the suite is meant to catch. So [`resolve_span`]
//! reports it as [`SpanProblem::SuspiciousDefault`] instead.
//!
//! See `claude-notes/scratch/2026-08-06-memo-quarto-source-map-default-sourceinfo.md`
//! for the upstream fix that would make that check unnecessary.
//!
//! # Fixtures must come from real text
//!
//! Most existing config tests build values with `SourceInfo::for_test()`.
//! Those spans are synthetic, so a wrong output span is indistinguishable
//! from a right one — the bug is invisible by construction. Tests that mean
//! to assert on spans must parse real source text; [`context_for`] builds
//! the matching [`SourceContext`].

use quarto_error_reporting::DiagnosticMessage;
use quarto_source_map::{FileId, MappedLocation, SourceContext, SourceInfo};

/// A [`SourceInfo`] resolved against a [`SourceContext`], in the terms a
/// reader of the rendered diagnostic sees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSpan {
    /// Path as registered in the [`SourceContext`].
    pub path: String,
    /// 1-based line of the span's start, matching rendered output.
    pub line: usize,
    /// 1-based column of the span's start, matching rendered output.
    pub column: usize,
    /// The source bytes the span actually covers — what gets underlined.
    pub text: String,
}

/// Why a [`SourceInfo`] could not be resolved to concrete source text.
///
/// Every variant is a distinct diagnosis rather than a generic failure:
/// when a span assertion fails, the reason is usually the interesting part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpanProblem {
    /// The diagnostic carried no location at all.
    NoLocation,
    /// `Original { file_id: FileId(0), 0..0 }` — almost certainly a
    /// defaulted `SourceInfo` rather than a real span into file 0.
    SuspiciousDefault,
    /// A `Generated` span with no invocation anchor: synthesized content
    /// with no source preimage to point at.
    Generated,
    /// A **gappy** `Concat` (or a `Substring`/`Generated` chain bottoming
    /// out in one): its pieces do not tile the source without gaps, so
    /// there is no single contiguous range to report. A gap-free `Concat`
    /// does *not* hit this — [`resolve_span`] resolves it piecewise (via
    /// `map_offset` on each end) to the hull those pieces tile out. Before
    /// piecewise resolution landed, this variant covered every `Concat`;
    /// it is now reserved for genuinely gappy ones.
    Concat,
    /// The file is not registered in the supplied [`SourceContext`].
    UnknownFile { file_id: usize },
    /// The file is registered but its content was not retained.
    NoContent { path: String },
    /// The byte range falls outside the file's content.
    OutOfBounds {
        path: String,
        start: usize,
        end: usize,
        len: usize,
    },
}

impl std::fmt::Display for SpanProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpanProblem::NoLocation => write!(f, "diagnostic carries no location"),
            SpanProblem::SuspiciousDefault => write!(
                f,
                "span is Original {{ FileId(0), 0..0 }} — a defaulted SourceInfo, \
                 not a real location (see span_assert module docs)"
            ),
            SpanProblem::Generated => write!(
                f,
                "span is Generated with no invocation anchor (synthesized, no source preimage)"
            ),
            SpanProblem::Concat => {
                write!(
                    f,
                    "span is a gappy Concat (pieces don't tile the source without \
                     gaps, so there is no single hull range)"
                )
            }
            SpanProblem::UnknownFile { file_id } => {
                write!(
                    f,
                    "FileId({file_id}) is not registered in the SourceContext"
                )
            }
            SpanProblem::NoContent { path } => {
                write!(f, "file `{path}` is registered without content")
            }
            SpanProblem::OutOfBounds {
                path,
                start,
                end,
                len,
            } => write!(
                f,
                "range {start}..{end} (end may be approximate — derived from \
                 arithmetic, not independently resolved, when only the start \
                 of the span resolved) is outside `{path}` (length {len})"
            ),
        }
    }
}

/// Build a [`SourceContext`] for text parsed via `quarto_yaml::parse_file`
/// under the same `filename`.
///
/// `quarto-yaml` derives its `FileId` by hashing the filename
/// (`quarto_yaml::file_id_for_filename`) rather than allocating
/// sequentially, so a context built with `add_file` would not resolve those
/// spans. This registers the file under the id the parser will actually
/// use.
pub fn context_for(filename: &str, content: &str) -> SourceContext {
    let mut ctx = SourceContext::new();
    let file_id = quarto_yaml::file_id_for_filename(filename);
    ctx.add_file_with_id(file_id, filename.to_string(), Some(content.to_string()));
    ctx
}

/// Peel off `Generated` wrappers to find the node whose own offset space we
/// should walk with `map_offset`/`length` — `Generated` itself has none (its
/// `map_offset` always returns `None`; see the module-level API in
/// `quarto-source-map`). Mirrors the recursion `resolve_byte_range` used to
/// do implicitly via `invocation_anchor().resolve_byte_range()`: each layer
/// must carry an `Invocation` anchor, or we refuse rather than guess.
fn peel_generated(info: &SourceInfo) -> Result<&SourceInfo, SpanProblem> {
    let mut current = info;
    while let SourceInfo::Generated { .. } = current {
        current = current.invocation_anchor().ok_or(SpanProblem::Generated)?;
    }
    Ok(current)
}

/// Whether a `Concat`'s pieces tile the source without gaps: for every
/// adjacent pair, the previous piece's own source end coincides with the
/// next piece's own source start (same file, same offset).
///
/// Deliberately measured via each piece's **own full extent** —
/// `piece.source_info.map_offset(0)` / `map_offset(piece.source_info.length())`
/// — rather than the concat's declared per-piece `length` field. A piece's
/// declared content length and its `source_info`'s own span length can
/// differ (a decoded escape folding two source bytes into one content
/// byte); walking a "local offset" derived from the declared length would
/// silently misalign the piece's own coordinate space, exactly the
/// mistake `Concat::map_offset`'s exclusive-end handling already has to
/// route around for the *last* piece — this applies the same fix to every
/// boundary, not just the last one.
///
/// This function answers the question for *whatever slice of pieces it is
/// handed*, so it is only as broad as its caller makes it. [`is_gapless`]
/// hands it every piece of a bare `Concat`, but for a `Substring` it hands
/// it only the pieces that substring's own content sub-range touches (see
/// [`pieces_touching`]) — a gap elsewhere in the same `Concat` is then
/// invisible, which is correct, because no hull over that sub-range spans
/// it.
///
/// Two imprecisions remain in the **refusing** direction — they can report
/// a gap-free span as gappy, never the reverse:
///
/// - a *partially* covered piece that is itself a nested `Concat` is
///   recursed into in **full**, not narrowed to the overlapping part;
/// - the first and last selected pieces are checked in full even when the
///   query covers only part of them. That costs nothing *when the piece's
///   own extent is contiguous* — which holds when its `source_info` is
///   `Original`, `Generated`, or a `Concat` (recursed into just above) —
///   because then only the boundaries *between* selected pieces can fail.
///
/// And one known gap in the **accepting** direction — a wrong `Ok`:
///
/// - a piece whose `source_info` is a `Substring` over a gappy `Concat`
///   is **not** checked internally. The `if let` above matches a *bare*
///   `Concat` only, so such a piece skips the recursion, and its own
///   `map_offset(0)`/`map_offset(length())` straddle the gap — the
///   endpoints this loop then measures neighbours against already span
///   source bytes belonging to no piece. Measured on the module's own
///   fixture: a single-piece `Concat` wrapping
///   `Substring(Concat[(6..10, 4), (12..15, 3)], 0, 7)` resolves to
///   `Ok("6789ABCDE")` — nine source bytes for seven content bytes,
///   silently including the gap's `"AB"`; the honest content is
///   `"6789CDE"`. Tracked as bd-qnubn7s0. This predates the
///   sub-range narrowing (the loop body is unchanged), but the narrowing
///   makes it *more reachable*: a top-level `Substring` over a gappy
///   `Concat` used to be refused wholesale, and now resolves whenever the
///   touched pieces pass — so such a piece among them can produce the
///   `Ok`.
///
/// This is the piecewise replacement for the contiguity check
/// `SourceInfo::preimage_in` used to make for `Concat`. `resolve_span`
/// deliberately does not call `preimage_in`: as of quarto-source-map 0.1.3
/// it refuses a `Substring` over a `Concat` parent outright (see the
/// design memo this task's brief cites), which is exactly the shape this
/// module must resolve.
fn concat_pieces_are_contiguous(
    pieces: &[quarto_source_map::SourcePiece],
    ctx: &SourceContext,
) -> bool {
    let mut prev_end: Option<MappedLocation> = None;
    for piece in pieces {
        if let SourceInfo::Concat { pieces: nested } = &piece.source_info
            && !concat_pieces_are_contiguous(nested, ctx)
        {
            return false;
        }
        let Some(this_start) = piece.source_info.map_offset(0, ctx) else {
            return false;
        };
        if let Some(prev) = &prev_end
            && (prev.file_id != this_start.file_id
                || prev.location.offset != this_start.location.offset)
        {
            return false;
        }
        let Some(this_end) = piece
            .source_info
            .map_offset(piece.source_info.length(), ctx)
        else {
            return false;
        };
        prev_end = Some(this_end);
    }
    true
}

/// The pieces a hull over the content sub-range `[start, end]` actually
/// depends on, as a sub-slice of `pieces`.
///
/// Selection is the one place in this module where the concat's declared
/// per-piece `length` field is the right number: it is a *content* length
/// being compared against *content* offsets, which is exactly what it is.
/// No source position is derived from it — positions come from
/// `map_offset` in [`concat_pieces_are_contiguous`].
///
/// The interval is deliberately **closed**, not half-open.
/// `Concat::map_offset` resolves an offset that lands exactly on a piece
/// boundary inside the *next* piece (offset 0 of it), falling back to the
/// last piece's own end only when the offset is the concat's total length.
/// So a query ending exactly on a boundary still reads a position out of
/// the following piece, and a gap at that boundary would silently widen
/// the hull. Selecting by a half-open `[start, end)` would drop that piece
/// and miss the gap.
fn pieces_touching(
    pieces: &[quarto_source_map::SourcePiece],
    start: usize,
    end: usize,
) -> &[quarto_source_map::SourcePiece] {
    let Some(first) = pieces
        .iter()
        .position(|p| p.offset_in_concat + p.length > start)
    else {
        return &[];
    };
    let Some(last) = pieces.iter().rposition(|p| p.offset_in_concat <= end) else {
        return &[];
    };
    if last < first {
        return &[];
    }
    &pieces[first..=last]
}

/// Whether `info` (or, for a `Substring`, its parent chain) is gap-free
/// **over the sub-range the query actually reads** — see
/// [`concat_pieces_are_contiguous`] for what gap-free means and why it's
/// measured that way, and [`pieces_touching`] for which pieces count.
/// `Original`/`Generated` are trivially gap-free (a single atom, by
/// construction).
fn is_gapless(info: &SourceInfo, ctx: &SourceContext) -> bool {
    is_gapless_over(info, None, ctx)
}

/// [`is_gapless`]'s worker. `queried` is the content sub-range, in
/// `info`'s **own** content coordinate space, that the caller's hull
/// reads; `None` means the whole of `info`.
///
/// A `Substring` layer translates that range into its parent's content
/// space (`parent_offset = substring.start_offset + own_offset`, the same
/// arithmetic `SourceInfo::map_offset` does) and clamps it to the
/// substring's own extent, so nested `Substring` chains narrow correctly
/// rather than losing track of where they sit in the root `Concat`.
fn is_gapless_over(
    info: &SourceInfo,
    queried: Option<(usize, usize)>,
    ctx: &SourceContext,
) -> bool {
    match info {
        SourceInfo::Original { .. } | SourceInfo::Generated { .. } => true,
        SourceInfo::Substring {
            parent,
            start_offset,
            end_offset,
        } => {
            let (start, end) = match queried {
                None => (*start_offset, *end_offset),
                Some((qs, qe)) => (
                    start_offset.saturating_add(qs).min(*end_offset),
                    start_offset.saturating_add(qe).min(*end_offset),
                ),
            };
            is_gapless_over(parent, Some((start, end)), ctx)
        }
        SourceInfo::Concat { pieces } => match queried {
            // Kept distinct from `Some((0, length))` on purpose: it is
            // what makes "a bare `Concat` keeps its pre-narrowing
            // behaviour" exactly true, including for degenerate
            // zero-length pieces. Don't collapse the two arms.
            None => concat_pieces_are_contiguous(pieces, ctx),
            Some((start, end)) => {
                concat_pieces_are_contiguous(pieces_touching(pieces, start, end), ctx)
            }
        },
    }
}

/// Resolve a [`SourceInfo`] to the source text it covers.
///
/// Resolution is **piecewise**: both ends of the span are located
/// independently via `map_offset(0)` / `map_offset(length())`, for every
/// shape. That is what lets a gap-free `Concat` — and a `Substring` whose
/// parent is one, the shape a diagnostic carries once content provenance
/// is threaded — resolve to the hull those pieces tile out, instead of
/// refusing outright. A **gappy** `Concat` still refuses
/// ([`SpanProblem::Concat`]): its pieces don't tile a single range, so
/// there is nothing honest to report.
pub fn resolve_span(info: &SourceInfo, ctx: &SourceContext) -> Result<ResolvedSpan, SpanProblem> {
    // Diagnose the shapes that cannot resolve *before* asking for a byte
    // range, so the caller gets the specific reason rather than a bare
    // `None`.
    if let SourceInfo::Original {
        file_id,
        start_offset,
        end_offset,
    } = info
        && file_id.0 == 0
        && *start_offset == 0
        && *end_offset == 0
    {
        return Err(SpanProblem::SuspiciousDefault);
    }
    if matches!(info, SourceInfo::Generated { .. }) && info.invocation_anchor().is_none() {
        return Err(SpanProblem::Generated);
    }

    // `Generated` has no offset space of its own; walk to the node whose
    // space we actually resolve (its `Invocation` anchor, possibly several
    // layers deep).
    let effective = peel_generated(info)?;
    let length = effective.length();

    match (
        effective.map_offset(0, ctx),
        effective.map_offset(length, ctx),
    ) {
        (Some(start_mapped), Some(end_mapped)) => {
            // Gap-free is required for a single hull to exist at all:
            // either the pieces don't tile without gaps, or (defensively)
            // the two ends landed in different files despite the
            // piecewise check below passing — both mean there's no single
            // range to report.
            if start_mapped.file_id != end_mapped.file_id || !is_gapless(effective, ctx) {
                return Err(SpanProblem::Concat);
            }

            let file = ctx
                .get_file(start_mapped.file_id)
                .ok_or(SpanProblem::UnknownFile {
                    file_id: start_mapped.file_id.0,
                })?;
            let content = file
                .content
                .as_deref()
                .ok_or_else(|| SpanProblem::NoContent {
                    path: file.path.clone(),
                })?;

            let start = start_mapped.location.offset;
            let end = end_mapped.location.offset;

            if start > end || end > content.len() {
                return Err(SpanProblem::OutOfBounds {
                    path: file.path.clone(),
                    start,
                    end,
                    len: content.len(),
                });
            }

            Ok(ResolvedSpan {
                path: file.path.clone(),
                // `Location` is 0-indexed; rendered diagnostics are 1-indexed.
                line: start_mapped.location.row + 1,
                column: start_mapped.location.column + 1,
                text: content[start..end].to_string(),
            })
        }
        (Some(start_mapped), None) => {
            // The start resolved but the end didn't: `map_offset` defers
            // its bounds check to `FileInformation::offset_to_location`,
            // which reports an out-of-bounds offset as a bare `None`
            // rather than saying how far out of bounds it is. We already
            // know the file (from the start); report the attempted end via
            // plain offset arithmetic — `length` past the start we did
            // resolve — rather than reaching for a second resolution
            // mechanism to recover the exact number.
            let file = ctx
                .get_file(start_mapped.file_id)
                .ok_or(SpanProblem::UnknownFile {
                    file_id: start_mapped.file_id.0,
                })?;
            let content = file
                .content
                .as_deref()
                .ok_or_else(|| SpanProblem::NoContent {
                    path: file.path.clone(),
                })?;
            Err(SpanProblem::OutOfBounds {
                path: file.path.clone(),
                start: start_mapped.location.offset,
                end: start_mapped.location.offset + length,
                len: content.len(),
            })
        }
        (None, _) => {
            // The *start* didn't resolve either. `map_offset`'s bounds
            // check treats "no such offset" (past EOF) exactly like "no
            // such file": both come back as a bare `None`, with no way to
            // tell which happened from the `Option` alone.
            // `resolve_byte_range` still answers this for
            // `Original`/`Substring` chains -- it does no bounds checking
            // at all, so it never fails just because an offset is out of
            // range. Used here purely to recover the raw numbers for a
            // diagnosis, not as the resolution mechanism: it still refuses
            // `Concat`, which falls through to the file-registration
            // checks below exactly as before.
            if let Some((file_id, start, end)) = effective.resolve_byte_range() {
                let file = ctx
                    .get_file(FileId(file_id))
                    .ok_or(SpanProblem::UnknownFile { file_id })?;
                let content = file
                    .content
                    .as_deref()
                    .ok_or_else(|| SpanProblem::NoContent {
                        path: file.path.clone(),
                    })?;
                return Err(SpanProblem::OutOfBounds {
                    path: file.path.clone(),
                    start,
                    end,
                    len: content.len(),
                });
            }

            // `resolve_byte_range` refused too (a `Concat`-rooted chain):
            // find out why via the file registration (structural — doesn't
            // need `ctx`) rather than via the offset, since `map_offset`
            // gives no reason.
            match effective.root_file_id() {
                Some(file_id) => match ctx.get_file(file_id) {
                    None => Err(SpanProblem::UnknownFile { file_id: file_id.0 }),
                    Some(file) if file.content.is_none() => Err(SpanProblem::NoContent {
                        path: file.path.clone(),
                    }),
                    Some(_) => Err(SpanProblem::Generated),
                },
                None => Err(SpanProblem::Generated),
            }
        }
    }
}

/// Resolve the location a [`DiagnosticMessage`] points at.
pub fn resolve_diagnostic_span(
    diag: &DiagnosticMessage,
    ctx: &SourceContext,
) -> Result<ResolvedSpan, SpanProblem> {
    let info = diag.location.as_ref().ok_or(SpanProblem::NoLocation)?;
    resolve_span(info, ctx)
}

/// Assert that `diag` underlines exactly `expected`.
///
/// Panics with the resolved location (or the specific [`SpanProblem`]) so a
/// failure says what the diagnostic *did* point at, not merely that it
/// mismatched.
#[track_caller]
pub fn assert_diagnostic_underlines(diag: &DiagnosticMessage, ctx: &SourceContext, expected: &str) {
    match resolve_diagnostic_span(diag, ctx) {
        Ok(span) if span.text == expected => {}
        Ok(span) => panic!(
            "diagnostic [{}] underlines the wrong text\n  \
             expected: {expected:?}\n  \
             actual:   {:?}\n  \
             at:       {}:{}:{}",
            diag.code.as_deref().unwrap_or("<no code>"),
            span.text,
            span.path,
            span.line,
            span.column,
        ),
        Err(problem) => panic!(
            "diagnostic [{}] has no usable span: {problem}\n  expected it to underline {expected:?}",
            diag.code.as_deref().unwrap_or("<no code>"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const YAML: &str = "listing:\n    sort: false\n    template: t.ejs\n";

    fn fixture() -> (quarto_yaml::YamlWithSourceInfo, SourceContext) {
        let parsed = quarto_yaml::parse_file(YAML, "fixture.yml").expect("valid yaml");
        (parsed, context_for("fixture.yml", YAML))
    }

    #[test]
    fn resolves_a_real_scalar_span_to_its_text() {
        let (parsed, ctx) = fixture();
        let listing = parsed.get_hash_value("listing").expect("listing key");
        let template = listing.get_hash_value("template").expect("template key");

        let span = resolve_span(&template.source_info, &ctx).expect("resolvable");
        assert_eq!(span.text, "t.ejs");
        assert_eq!(span.path, "fixture.yml");
        // Third line of YAML, 1-based.
        assert_eq!(span.line, 3);
    }

    #[test]
    fn line_and_column_are_one_based() {
        let (parsed, ctx) = fixture();
        // `listing` is the very first byte of the file.
        let span = resolve_span(&parsed.get_hash_value("listing").unwrap().source_info, &ctx)
            .expect("resolvable");
        assert!(
            span.line >= 1 && span.column >= 1,
            "expected 1-based line/column, got {}:{}",
            span.line,
            span.column
        );
    }

    #[test]
    fn defaulted_source_info_is_reported_not_rendered() {
        // The whole point: a defaulted SourceInfo must not resolve to a
        // plausible-looking "file 0, line 1". If this ever starts passing
        // as Ok(..), the helper has stopped catching the bug class it
        // exists for.
        let ctx = SourceContext::new();
        let bogus = SourceInfo::Original {
            file_id: FileId(0),
            start_offset: 0,
            end_offset: 0,
        };
        assert_eq!(
            resolve_span(&bogus, &ctx),
            Err(SpanProblem::SuspiciousDefault)
        );
    }

    #[test]
    fn unregistered_file_is_reported() {
        let (parsed, _) = fixture();
        let empty = SourceContext::new();
        let listing = parsed.get_hash_value("listing").expect("listing key");
        assert!(matches!(
            resolve_span(&listing.source_info, &empty),
            Err(SpanProblem::UnknownFile { .. })
        ));
    }

    #[test]
    fn generated_with_no_invocation_anchor_is_reported() {
        // Pure synthesis (e.g. a sectionize wrapper): no source preimage to
        // point at, so this must refuse rather than fall through to
        // `resolve_byte_range`'s bare `None`.
        let ctx = SourceContext::new();
        let synthetic = SourceInfo::for_test();
        assert_eq!(resolve_span(&synthetic, &ctx), Err(SpanProblem::Generated));
    }

    #[test]
    fn no_content_is_reported() {
        let mut ctx = SourceContext::new();
        let file_id = quarto_yaml::file_id_for_filename("no-content.yml");
        ctx.add_file_with_id(file_id, "no-content.yml".to_string(), None);
        let info = SourceInfo::Original {
            file_id,
            start_offset: 0,
            end_offset: 3,
        };
        assert!(matches!(
            resolve_span(&info, &ctx),
            Err(SpanProblem::NoContent { .. })
        ));
    }

    #[test]
    fn out_of_bounds_is_reported() {
        let (_parsed, ctx) = fixture();
        let file_id = quarto_yaml::file_id_for_filename("fixture.yml");
        let info = SourceInfo::Original {
            file_id,
            start_offset: 0,
            end_offset: YAML.len() + 1000,
        };
        assert!(matches!(
            resolve_span(&info, &ctx),
            Err(SpanProblem::OutOfBounds { .. })
        ));
    }

    #[test]
    fn out_of_bounds_start_is_reported_not_generated() {
        // Regression: when the *start* offset alone is past EOF (not just
        // the end), `map_offset(0)` itself returns `None` -- landing in the
        // `(None, _)` arm rather than the `(Some(start), None)` arm that
        // `out_of_bounds_is_reported` above exercises. These are two
        // independent code paths to the same SpanProblem; before the fix
        // this one fell through to `SpanProblem::Generated`, which is
        // exactly the misdiagnosis this module's docs warn against: it
        // sends a reader hunting for a filter-created node that was never
        // there, when the span was simply a bad offset into a real file.
        let (_parsed, ctx) = fixture();
        let file_id = quarto_yaml::file_id_for_filename("fixture.yml");
        let info = SourceInfo::Original {
            file_id,
            start_offset: YAML.len() + 1000,
            end_offset: YAML.len() + 1005,
        };
        assert!(matches!(
            resolve_span(&info, &ctx),
            Err(SpanProblem::OutOfBounds { .. })
        ));
    }

    // -------------------------------------------------------------------
    // `peel_generated`: walking through a `Generated` wrapper to the node
    // whose offset space `map_offset`/`length` actually operate over.
    // `generated_with_no_invocation_anchor_is_reported` above exercises
    // `resolve_span`'s *early guard* (the outer node itself has no
    // anchor) -- these exercise `peel_generated` itself, which only runs
    // once the outer node already has an anchor.
    // -------------------------------------------------------------------

    fn generated_with_anchor(anchor: SourceInfo) -> SourceInfo {
        let mut generated = SourceInfo::generated(quarto_source_map::By::test_scaffold());
        generated.append_anchor(
            quarto_source_map::AnchorRole::Invocation,
            std::sync::Arc::new(anchor),
        );
        generated
    }

    #[test]
    fn generated_with_a_valid_anchor_resolves_through_it() {
        let (parsed, ctx) = fixture();
        let listing = parsed.get_hash_value("listing").expect("listing key");
        let generated = generated_with_anchor(listing.source_info.clone());

        let span = resolve_span(&generated, &ctx)
            .expect("Generated with a real Invocation anchor should resolve through it");
        assert_eq!(span.path, "fixture.yml");
    }

    #[test]
    fn generated_nested_without_an_anchor_at_the_inner_layer_is_reported() {
        // Outer has a real Invocation anchor (so `resolve_span`'s early
        // guard does not fire) that points at *another* `Generated` node
        // with no anchor of its own -- this is `peel_generated`'s own
        // multi-layer refusal, not the early guard's.
        let ctx = SourceContext::new();
        let inner = SourceInfo::for_test(); // Generated { from: [] }
        let outer = generated_with_anchor(inner);
        assert_eq!(resolve_span(&outer, &ctx), Err(SpanProblem::Generated));
    }

    // -------------------------------------------------------------------
    // Piecewise resolution of `Concat` and `Substring{parent: Concat}`.
    //
    // Shapes modeled on the design memo's measured A/B/C example (see this
    // task's brief): three real files' worth of content is unnecessary —
    // one real file with several `Original` spans into it, concatenated,
    // reproduces the same piece shapes (including a piece whose declared
    // concat-length differs from its own source span length, and a gap
    // between two pieces).
    // -------------------------------------------------------------------

    const CONCAT_CONTENT: &str = "0123456789ABCDEF";

    fn concat_fixture() -> (FileId, SourceContext) {
        let filename = "concat-fixture.txt";
        let file_id = quarto_yaml::file_id_for_filename(filename);
        (file_id, context_for(filename, CONCAT_CONTENT))
    }

    fn original(file_id: FileId, start: usize, end: usize) -> SourceInfo {
        SourceInfo::Original {
            file_id,
            start_offset: start,
            end_offset: end,
        }
    }

    #[test]
    fn gap_free_concat_resolves_to_its_hull() {
        let (fid, ctx) = concat_fixture();
        // A = concat[(Original(1,3), 2), (Original(3,5), 1), (Original(5,6), 1)]
        // Piece 1 declares content-length 1 over a 2-byte source span (a
        // folded piece) -- deliberately, per the design memo -- and the
        // pieces still tile the source without gaps: 1..3, 3..5, 5..6.
        let a = SourceInfo::concat(vec![
            (original(fid, 1, 3), 2),
            (original(fid, 3, 5), 1),
            (original(fid, 5, 6), 1),
        ]);

        // Sanity per the measured numbers this task's brief cites.
        assert_eq!(a.map_offset(0, &ctx).unwrap().location.offset, 1);
        assert_eq!(a.map_offset(4, &ctx).unwrap().location.offset, 6);

        let span = resolve_span(&a, &ctx).expect("gap-free Concat should resolve");
        assert_eq!(span.text, &CONCAT_CONTENT[1..6]);
        assert_eq!(span.line, 1);
        // Offset 1 into an all-ASCII single-line file -> 0-based column 1,
        // 1-based column 2.
        assert_eq!(span.column, 2);
    }

    #[test]
    fn substring_over_whole_gap_free_concat_resolves_to_the_same_hull() {
        let (fid, ctx) = concat_fixture();
        let a = SourceInfo::concat(vec![
            (original(fid, 1, 3), 2),
            (original(fid, 3, 5), 1),
            (original(fid, 5, 6), 1),
        ]);
        // C = substring(A, 0, 4): the whole content of A, length 4. This is
        // the shape a diagnostic carries after the threading phase (a
        // Substring extracted from a Concat-backed scalar). Before
        // piecewise resolution this reported `SpanProblem::Generated`.
        let c = SourceInfo::substring(a, 0, 4);

        let span = resolve_span(&c, &ctx)
            .expect("Substring{parent: Concat} over the whole content should resolve");
        assert_eq!(span.text, &CONCAT_CONTENT[1..6]);
        assert_eq!(span.line, 1);
        assert_eq!(span.column, 2);
    }

    #[test]
    fn gappy_concat_is_reported_not_guessed() {
        let (fid, ctx) = concat_fixture();
        // B = concat[(Original(6,10), 4), (Original(12,15), 3)] -- a gap at
        // 10..12 the source bytes in between aren't part of either piece.
        let b = SourceInfo::concat(vec![(original(fid, 6, 10), 4), (original(fid, 12, 15), 3)]);

        assert_eq!(resolve_span(&b, &ctx), Err(SpanProblem::Concat));
    }

    /// The narrowing (Plan 3 Phase 6c): a `Substring` that lies wholly
    /// inside **one** piece of an otherwise-gappy `Concat` resolves, because
    /// no hull over its sub-range crosses the gap. This is the shape a span
    /// inside a single `#|` cell-option line has — every multi-line options
    /// block is gappy (each line's `#| ` marker sits between the pieces).
    /// Reverting [`is_gapless`] to whole-`Concat` contiguity reddens this.
    #[test]
    fn substring_inside_one_piece_of_a_gappy_concat_resolves() {
        let (fid, ctx) = concat_fixture();
        // Same gappy B as above: content [0,4) -> source 6..10,
        // content [4,7) -> source 12..15, with 10..12 in neither.
        let b = SourceInfo::concat(vec![(original(fid, 6, 10), 4), (original(fid, 12, 15), 3)]);
        // Content 1..3 is strictly inside the first piece: source 7..9.
        let inside = SourceInfo::substring(b, 1, 3);

        let span = resolve_span(&inside, &ctx)
            .expect("a sub-range inside one piece never crosses the gap");
        assert_eq!(span.text, &CONCAT_CONTENT[7..9]);
    }

    /// The closed-interval half of the narrowing. A sub-range that stops
    /// exactly **on** a piece boundary still reads its end position out of
    /// the *following* piece (`Concat::map_offset` maps a boundary offset to
    /// offset 0 of the next piece), so the gap at that boundary is inside
    /// the hull and the span must still be refused. Selecting pieces by a
    /// half-open `[start, end)` would drop the second piece and report
    /// `Ok` with `CONCAT_CONTENT[6..12]` — six source bytes for four
    /// content bytes.
    #[test]
    fn substring_ending_on_a_gappy_piece_boundary_is_still_refused() {
        let (fid, ctx) = concat_fixture();
        let b = SourceInfo::concat(vec![(original(fid, 6, 10), 4), (original(fid, 12, 15), 3)]);
        // Content 0..4 == exactly the first piece, ending on the boundary.
        let up_to_boundary = SourceInfo::substring(b, 0, 4);

        assert_eq!(
            resolve_span(&up_to_boundary, &ctx),
            Err(SpanProblem::Concat)
        );
    }
}
