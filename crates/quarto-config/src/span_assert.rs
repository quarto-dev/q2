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
use quarto_source_map::{FileId, SourceContext, SourceInfo};

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
    /// A `Concat` span: spans multiple sources, so it has no single range.
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
                    "span is Concat (covers multiple sources, no single range)"
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
            } => write!(f, "range {start}..{end} is outside `{path}` (length {len})"),
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

/// Resolve a [`SourceInfo`] to the source text it covers.
pub fn resolve_span(info: &SourceInfo, ctx: &SourceContext) -> Result<ResolvedSpan, SpanProblem> {
    // Diagnose the shapes that cannot resolve *before* asking for a byte
    // range, so the caller gets the specific reason rather than a bare
    // `None`. `resolve_byte_range` folds several distinct cases together.
    match info {
        SourceInfo::Original {
            file_id,
            start_offset,
            end_offset,
        } if file_id.0 == 0 && *start_offset == 0 && *end_offset == 0 => {
            return Err(SpanProblem::SuspiciousDefault);
        }
        SourceInfo::Concat { .. } => return Err(SpanProblem::Concat),
        SourceInfo::Generated { .. } if info.invocation_anchor().is_none() => {
            return Err(SpanProblem::Generated);
        }
        _ => {}
    }

    let (file_id, start, end) = info.resolve_byte_range().ok_or(SpanProblem::Generated)?;

    let file = ctx
        .get_file(FileId(file_id))
        .ok_or(SpanProblem::UnknownFile { file_id })?;
    let content = file
        .content
        .as_deref()
        .ok_or_else(|| SpanProblem::NoContent {
            path: file.path.clone(),
        })?;

    if start > end || end > content.len() {
        return Err(SpanProblem::OutOfBounds {
            path: file.path.clone(),
            start,
            end,
            len: content.len(),
        });
    }

    let location =
        quarto_source_map::offset_to_location(content, start).ok_or(SpanProblem::OutOfBounds {
            path: file.path.clone(),
            start,
            end,
            len: content.len(),
        })?;

    Ok(ResolvedSpan {
        path: file.path.clone(),
        // `Location` is 0-indexed; rendered diagnostics are 1-indexed.
        line: location.row + 1,
        column: location.column + 1,
        text: content[start..end].to_string(),
    })
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
}
