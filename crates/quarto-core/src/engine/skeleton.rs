//! KNR-6: the skeleton a whole-document engine (knitr) returns, and the marker finder that reads it.
//!
//! Once the flip lands, knitr's chunk hook emits a *marker* where a structured chunk (or a brace
//! span) belongs, instead of the finished markdown, so knitr's own joining builds the document
//! around the markers and Rust presents each unit over its marker. A marker is
//! `<!-- q2:<nonce>:<unit-id> -->`. The nonce is made per run by Rust and handed to R, so user
//! text that merely looks like a marker is never mistaken for one.
//!
//! [`rerange_onto_markers`] finds this run's markers and re-ranges the document's units onto
//! them: the result is what `present_document` takes as its units, with the skeleton as its base
//! text. A unit with no marker is dropped, so it presents nothing (what `include=FALSE` needs).

use std::collections::HashMap;
use std::path::PathBuf;

use quarto_pandoc_types::ConfigValue;

use crate::engine::units::Unit;
use crate::presenter::document::{PresentError, PresentNote, present_document};
use crate::presenter::figures::FigureSink;
use crate::presenter::inline::InlineTarget;
use crate::record::{EngineRecord, stored_markdown};

/// The marker text for `id` under `nonce`.
pub fn marker(nonce: &str, id: &str) -> String {
    format!("<!-- q2:{nonce}:{id} -->")
}

/// A fresh nonce for one run: 128 random bits as hex. Native only: only the knitr engine, which
/// does not run in WASM, makes one; replay reads the nonce from the capture.
#[cfg(not(target_arch = "wasm32"))]
pub fn new_nonce() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// Re-range `units` onto the markers of `skeleton` that carry `nonce`.
///
/// Each unit with an id and a marker keeps every field it had (its code, its container prefix
/// and indent, so its author text is unchanged) except its range, which becomes the marker's:
/// the start is the first byte of the marker and the end the byte after its closing `-->`, so
/// the newline after the marker stays in the skeleton. A unit with no id or no marker is
/// dropped. When an id has several markers (it should not), the first one wins.
pub fn rerange_onto_markers(skeleton: &str, nonce: &str, units: &[Unit]) -> Vec<Unit> {
    let mut out = Vec::with_capacity(units.len());
    for unit in units {
        let id = match unit {
            Unit::Cell(c) => c.id.as_ref(),
            Unit::Inline(i) => i.id.as_ref(),
        };
        let Some(id) = id else { continue };
        let needle = marker(nonce, &id.0);
        let Some(start) = skeleton.find(&needle) else {
            continue;
        };
        let end = start + needle.len();
        let mut unit = unit.clone();
        match &mut unit {
            Unit::Cell(c) => {
                c.start = start;
                c.end = end;
            }
            Unit::Inline(i) => {
                i.start = start;
                i.end = end;
            }
        }
        out.push(unit);
    }
    out
}

/// The markdown R shipped for each unit: a chunk's finished markdown and a brace span's
/// finished fragment, keyed by unit id.
#[derive(Debug, Default)]
pub struct Shipped<'a> {
    pub chunks: HashMap<&'a str, &'a str>,
    pub spans: HashMap<&'a str, &'a str>,
}

/// Why a unit rendered from its shipped markdown instead of the presenter (KNR-6 6B). Counted
/// per render, logged at debug level, never shown to users; KNR-8's trigger reads the counts.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FallbackCounts {
    /// The run built no record (a count mismatch or a malformed one): every unit.
    pub no_record: usize,
    /// The record was fine but presenting it failed (invalid cell options, a front matter
    /// value that cannot hold an inline result): every unit.
    pub present_failed: usize,
    /// The adapter made a fallback cell for a chunk it cannot map (an `Other` piece, a gif or
    /// svg-less image, an unreadable file, a path outside the document directory).
    pub unmapped: usize,
    /// The presenter left a unit in place (a stored fallback for another format, or a span
    /// with no value the target can use).
    pub unplaced: usize,
}

impl FallbackCounts {
    pub fn total(&self) -> usize {
        self.no_record + self.present_failed + self.unmapped + self.unplaced
    }
}

/// What presenting a skeleton produced.
#[derive(Debug)]
pub struct SkeletonPresented {
    /// The final document text.
    pub markdown: String,
    /// The skeleton with the shipped markdown already swapped in for every marker the presenter
    /// could not fill, so presenting it again (replay) reproduces `markdown`.
    pub skeleton: String,
    pub files_written: Vec<PathBuf>,
    pub fallbacks: FallbackCounts,
    /// What the presenter left in place or could not write (e.g. an unusable image,
    /// `Q-18-11`), for the caller to report.
    pub notes: Vec<PresentNote>,
    /// The units re-ranged onto their markers: the indexing `notes` refer to.
    pub ranged_units: Vec<Unit>,
}

/// Replace every marker of `nonce` still in `text` with that unit's shipped markdown. A chunk
/// marker is a whole line (an indent, the marker, a newline) standing for the markdown the hook
/// would have returned, which already carries its own indent and leading blank line, so the line
/// is replaced wholesale; a span marker is replaced in place. A marker whose unit shipped
/// nothing is left alone (it cannot be told from user text, and would show as an HTML comment).
/// Returns the unit ids replaced.
pub fn substitute_markers(text: &str, nonce: &str, shipped: &Shipped<'_>) -> (String, Vec<String>) {
    let opener = format!("<!-- q2:{nonce}:");
    let mut out = String::with_capacity(text.len());
    let mut replaced = Vec::new();
    let mut rest = 0;
    while let Some(found) = text[rest..].find(&opener) {
        let start = rest + found;
        let id_start = start + opener.len();
        let Some(close) = text[id_start..].find(" -->") else {
            break;
        };
        let id = &text[id_start..id_start + close];
        let end = id_start + close + " -->".len();
        let chunk = shipped.chunks.get(id);
        let span = shipped.spans.get(id);
        match (chunk, span) {
            (Some(markdown), _) => {
                let line_start = text[..start].rfind('\n').map_or(0, |n| n + 1);
                let prefix_ok = text[line_start..start]
                    .chars()
                    .all(|c| c == ' ' || c == '\t' || c == '>');
                let from = if prefix_ok { line_start } else { start };
                let to = if text[end..].starts_with('\n') {
                    end + 1
                } else {
                    end
                };
                out.push_str(&text[rest..from.max(rest)]);
                out.push_str(markdown);
                rest = to;
            }
            (None, Some(fragment)) => {
                out.push_str(&text[rest..start]);
                out.push_str(fragment);
                rest = end;
            }
            (None, None) => {
                out.push_str(&text[rest..end]);
                rest = end;
                continue;
            }
        }
        replaced.push(id.to_string());
    }
    out.push_str(&text[rest..]);
    (out, replaced)
}

/// Present `skeleton` (the text knitr returned, with this run's markers) from `record`.
///
/// Units are re-ranged onto their markers and presented over the skeleton; whatever marker is
/// left (no record, a failed present, a unit the presenter left in place) is then filled from
/// the shipped markdown. The render never fails here.
#[allow(clippy::too_many_arguments)]
pub fn present_skeleton(
    skeleton: &str,
    nonce: &str,
    record: Option<&EngineRecord>,
    units: &[Unit],
    scope: Option<&ConfigValue>,
    format: &str,
    shipped: &Shipped<'_>,
    fig: &mut dyn FigureSink,
) -> SkeletonPresented {
    let mut fallbacks = FallbackCounts::default();
    let mut files_written = Vec::new();
    let mut notes = Vec::new();
    let mut ranged_units = Vec::new();
    let mut presented = skeleton.to_string();
    // Why a marker the presenter left would be left: the whole document has no usable record,
    // or presenting it failed. With a usable record, a marker left is one `present_document`
    // reported as a note.
    enum Whole {
        NoRecord,
        PresentFailed,
        Presented,
    }
    let whole = match record {
        None => Whole::NoRecord,
        Some(record) => {
            let ranged = rerange_onto_markers(skeleton, nonce, units);
            // Chunks the adapter could not map ship their markdown inside the record.
            fallbacks.unmapped = ranged
                .iter()
                // Only chunks: a brace span's plain value is stored as verbatim markdown by design
                // (KNR-4), which is a typed value, not a fallback.
                .filter_map(|unit| match unit {
                    Unit::Cell(c) => c.id.as_ref(),
                    Unit::Inline(_) => None,
                })
                .filter_map(|id| {
                    record
                        .cells
                        .iter()
                        .find(|cell| cell.metadata.quarto_cell_id == id.0)
                })
                .filter(|cell| stored_markdown(cell).is_some())
                .count();
            match present_document(
                &record.cells,
                skeleton,
                &ranged,
                scope,
                InlineTarget::from_format(format),
                format,
                Some(&record.engine),
                fig,
            ) {
                Ok(result) => {
                    presented = result.markdown;
                    files_written = result.files_written;
                    notes = result.notes.clone();
                    ranged_units = ranged;
                    fallbacks.unplaced = result
                        .notes
                        .iter()
                        .filter(|n| {
                            matches!(
                                n,
                                PresentNote::InlineNoUsableValue { .. }
                                    | PresentNote::MarkdownFormatMismatch { .. }
                            )
                        })
                        .count();
                    Whole::Presented
                }
                Err(
                    PresentError::InvalidCellOptions { .. } | PresentError::FrontMatterValue { .. },
                ) => Whole::PresentFailed,
            }
        }
    };
    let (markdown, replaced) = substitute_markers(&presented, nonce, shipped);
    match whole {
        Whole::NoRecord => fallbacks.no_record = replaced.len(),
        Whole::PresentFailed => fallbacks.present_failed = replaced.len(),
        Whole::Presented => {}
    }
    // The replay skeleton: the same substitution, applied to the original skeleton for just
    // the units that fell back.
    let only: Shipped<'_> = Shipped {
        chunks: shipped
            .chunks
            .iter()
            .filter(|(id, _)| replaced.iter().any(|r| r == **id))
            .map(|(k, v)| (*k, *v))
            .collect(),
        spans: shipped
            .spans
            .iter()
            .filter(|(id, _)| replaced.iter().any(|r| r == **id))
            .map(|(k, v)| (*k, *v))
            .collect(),
    };
    let (skeleton_out, _) = substitute_markers(skeleton, nonce, &only);
    SkeletonPresented {
        markdown,
        skeleton: skeleton_out,
        files_written,
        fallbacks,
        notes,
        ranged_units,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::units::{UnitId, scan_units};

    const DOC: &str = "Before.\n\n```{r}\n1 + 1\n```\n\nAfter `{r} 2` end.\n";

    fn with_ids(doc: &str) -> Vec<Unit> {
        let mut units = scan_units(doc);
        for (n, unit) in units.iter_mut().enumerate() {
            let id = Some(UnitId(format!("u{n}")));
            match unit {
                Unit::Cell(c) => c.id = id,
                Unit::Inline(i) => i.id = id,
            }
        }
        units
    }

    fn range(unit: &Unit) -> (usize, usize) {
        match unit {
            Unit::Cell(c) => (c.start, c.end),
            Unit::Inline(i) => (i.start, i.end),
        }
    }

    #[test]
    fn units_move_onto_their_markers() {
        let skeleton = format!(
            "Before.\n\n{}\n\nAfter {} end.\n",
            marker("n1", "u0"),
            marker("n1", "u1")
        );
        let out = rerange_onto_markers(&skeleton, "n1", &with_ids(DOC));
        assert_eq!(out.len(), 2);
        let (s, e) = range(&out[0]);
        assert_eq!(&skeleton[s..e], "<!-- q2:n1:u0 -->");
        let (s, e) = range(&out[1]);
        assert_eq!(&skeleton[s..e], "<!-- q2:n1:u1 -->");
    }

    #[test]
    fn a_unit_with_no_marker_is_dropped() {
        // `include=FALSE` leaves no marker, so the unit presents nothing.
        let skeleton = format!("Before.\n\n{}\n", marker("n1", "u1"));
        let out = rerange_onto_markers(&skeleton, "n1", &with_ids(DOC));
        assert_eq!(out.len(), 1);
        assert!(matches!(out[0], Unit::Inline(_)));
    }

    #[test]
    fn a_marker_with_another_nonce_is_user_text_and_is_ignored() {
        let skeleton = format!("{}\n", marker("someone-elses", "u0"));
        assert!(rerange_onto_markers(&skeleton, "n1", &with_ids(DOC)).is_empty());
    }

    #[test]
    fn a_unit_keeps_its_other_fields() {
        let units = with_ids("- item\n\n  ```{r}\n  1\n  ```\n");
        let Unit::Cell(before) = &units[0] else {
            panic!("a cell")
        };
        let skeleton = format!("- item\n\n  {}\n", marker("n", "u0"));
        let out = rerange_onto_markers(&skeleton, "n", &units);
        let Unit::Cell(after) = &out[0] else {
            panic!("a cell")
        };
        assert_eq!(
            (
                &after.code,
                after.indent,
                &after.container_prefix,
                &after.id
            ),
            (
                &before.code,
                before.indent,
                &before.container_prefix,
                &before.id
            )
        );
        assert_ne!((after.start, after.end), (before.start, before.end));
    }

    fn shipped<'a>(chunks: &[(&'a str, &'a str)], spans: &[(&'a str, &'a str)]) -> Shipped<'a> {
        Shipped {
            chunks: chunks.iter().copied().collect(),
            spans: spans.iter().copied().collect(),
        }
    }

    #[test]
    fn a_left_chunk_marker_is_replaced_by_its_whole_line() {
        let text = format!("Before.\n{}\nAfter.\n", marker("n", "c1"));
        let (out, replaced) = substitute_markers(
            &text,
            "n",
            &shipped(&[("c1", "\n::: {.cell}\nbody\n:::\n")], &[]),
        );
        assert_eq!(out, "Before.\n\n::: {.cell}\nbody\n:::\nAfter.\n");
        assert_eq!(replaced, ["c1"]);
    }

    #[test]
    fn a_chunk_markers_indent_goes_with_its_line() {
        // The shipped markdown carries its own indent, as the hook's output does.
        let text = format!("- item\n\n  {}\n", marker("n", "c1"));
        let (out, _) = substitute_markers(
            &text,
            "n",
            &shipped(&[("c1", "\n  ::: {.cell}\n  x\n  :::\n")], &[]),
        );
        assert_eq!(out, "- item\n\n\n  ::: {.cell}\n  x\n  :::\n");
    }

    #[test]
    fn a_left_span_marker_is_replaced_in_place() {
        let text = format!("A {} b.\n", marker("n", "s1"));
        let (out, replaced) = substitute_markers(&text, "n", &shipped(&[], &[("s1", "**4**")]));
        assert_eq!(out, "A **4** b.\n");
        assert_eq!(replaced, ["s1"]);
    }

    #[test]
    fn foreign_and_unshipped_markers_are_left_alone() {
        let text = format!("{}\n{}\n", marker("other", "c1"), marker("n", "c2"));
        let (out, replaced) = substitute_markers(&text, "n", &shipped(&[("c1", "X")], &[]));
        assert_eq!(out, text);
        assert!(replaced.is_empty());
    }

    // `tempfile` is a native-only dev-dependency.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn with_no_record_every_marker_falls_back_and_is_counted() {
        let skeleton = format!("A.\n{}\nB {} c.\n", marker("n", "c1"), marker("n", "s1"));
        let shipped = shipped(&[("c1", "\nCHUNK\n")], &[("s1", "SPAN")]);
        let dir = tempfile::tempdir().unwrap();
        let mut fig = crate::presenter::figures::FsFigureSink::new(&dir.path().join("d.qmd"));
        let out = present_skeleton(&skeleton, "n", None, &[], None, "html", &shipped, &mut fig);
        assert_eq!(out.markdown, "A.\n\nCHUNK\nB SPAN c.\n");
        assert_eq!(out.fallbacks.no_record, 2);
        assert_eq!(out.fallbacks.total(), 2);
        // The replay skeleton already holds the shipped text.
        assert_eq!(out.skeleton, out.markdown);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn nonces_are_distinct_hex() {
        let (a, b) = (new_nonce(), new_nonce());
        assert_ne!(a, b);
        assert_eq!(a.len(), 32);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
    }
}
