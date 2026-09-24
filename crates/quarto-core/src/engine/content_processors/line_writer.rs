/*
 * engine/content_processors/line_writer.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Shared line-scanning + provenance-tracking helpers for the percent and
 * spin content processors.
 */

//! Shared building blocks for converting a line-oriented source format
//! (percent scripts, `knitr::spin` scripts) to markdown while building an
//! exact `SourceInfo` back into the original bytes.

use std::ops::Range;

use quarto_source_map::{ProvenanceBuilder, SourceInfo};

use super::ORIGINAL_FILE_ID;

/// One physical line, in byte offsets into the original content.
/// `content_end` excludes the line terminator; `full_end` includes it
/// (`full_end == content_end` for a final line with no trailing newline).
#[derive(Debug, Clone, Copy)]
pub(super) struct LineSpan {
    pub start: usize,
    pub content_end: usize,
    pub full_end: usize,
}

impl LineSpan {
    pub fn content_range(&self) -> Range<usize> {
        self.start..self.content_end
    }

    pub fn newline_range(&self) -> Range<usize> {
        self.content_end..self.full_end
    }

    pub fn whole_range(&self) -> Range<usize> {
        self.start..self.full_end
    }
}

/// Scan `text` into `LineSpan`s over `range`, splitting on `\r\n`, `\r`, or
/// `\n` (matching `ts-packages/quarto-api/src/text/index.ts`'s `lines()`).
pub(super) fn scan_lines(text: &str, range: Range<usize>) -> Vec<LineSpan> {
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let mut start = range.start;
    let mut i = range.start;
    while i < range.end {
        match bytes[i] {
            b'\n' => {
                spans.push(LineSpan {
                    start,
                    content_end: i,
                    full_end: i + 1,
                });
                i += 1;
                start = i;
            }
            b'\r' => {
                let full_end = if i + 1 < range.end && bytes[i + 1] == b'\n' {
                    i + 2
                } else {
                    i + 1
                };
                spans.push(LineSpan {
                    start,
                    content_end: i,
                    full_end,
                });
                i = full_end;
                start = i;
            }
            _ => i += 1,
        }
    }
    if start < range.end || spans.is_empty() {
        spans.push(LineSpan {
            start,
            content_end: range.end,
            full_end: range.end,
        });
    }
    spans
}

/// A whitespace-only (or empty) line.
pub(super) fn is_blank(text: &str, span: &LineSpan) -> bool {
    text[span.content_range()].trim().is_empty()
}

/// The trimmed sub-range of `lines` with leading/trailing blank lines
/// excluded (`trimEmptyLines`/`strip_white`, both "trim both ends" by
/// default). `None` when every line is blank.
pub(super) fn trim_blank_lines(content: &str, lines: &[LineSpan]) -> Option<Range<usize>> {
    let first = lines.iter().position(|l| !is_blank(content, l))?;
    let last = lines.iter().rposition(|l| !is_blank(content, l))?;
    Some(first..last + 1)
}

/// A running (source cursor, output builder, provenance builder) triple —
/// every push keeps the provenance builder's source-side tiling exactly in
/// sync with what gets appended to `out`.
pub(super) struct Writer<'a> {
    pub content: &'a str,
    out: String,
    pb: ProvenanceBuilder,
    cursor: usize,
}

impl<'a> Writer<'a> {
    pub fn new(content: &'a str, anchor: usize) -> Self {
        Self {
            content,
            out: String::new(),
            pb: ProvenanceBuilder::in_file(ORIGINAL_FILE_ID, anchor),
            cursor: anchor,
        }
    }

    /// Delete a source range (e.g. a header line, a stripped prefix, an
    /// original newline) with no corresponding output.
    pub fn delete(&mut self, range: Range<usize>) {
        debug_assert_eq!(
            self.cursor, range.start,
            "provenance must tile contiguously"
        );
        self.pb.replacement(range.clone(), 0);
        self.cursor = range.end;
    }

    /// Copy a source range's bytes verbatim into the output.
    pub fn keep(&mut self, range: Range<usize>) {
        debug_assert_eq!(
            self.cursor, range.start,
            "provenance must tile contiguously"
        );
        self.out.push_str(&self.content[range.clone()]);
        self.pb.verbatim(range.clone());
        self.cursor = range.end;
    }

    /// Insert synthetic text with no corresponding source bytes (fence
    /// lines, `#|` option lines, blank-line separators).
    pub fn synthetic(&mut self, text: &str) {
        self.out.push_str(text);
        self.pb.replacement(self.cursor..self.cursor, text.len());
    }

    pub fn finish(self) -> (String, SourceInfo) {
        (self.out, self.pb.finish())
    }
}
