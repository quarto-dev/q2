/*
 * engine/content_processors/percent.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * `percent` content processor.
 */

//! `percent` content processor.
//!
//! Rust port of `ts-packages/quarto-api/src/jupyter/percent-script.ts`
//! (itself a rewrite of Q1 `core/jupyter/percent.ts`), parameterized by
//! `(comment_open, fence_language)` so one implementation serves every
//! percent-format language (jupyter's `.py/.jl/.r/.q`, and any TS engine —
//! julia, marimo — that names this processor).
//!
//! **Scope cut (documented, not silently dropped):** Q1's raw-cell path
//! dispatches on `format:`/`raw_mimetype:` metadata (`mdRawOutput`/
//! `mdFormatOutput` in `to-markdown.ts`) to wrap raw content in a
//! `{=format}` fenced block. This port treats every raw cell as plain
//! comment-stripped (or triple-quote) content, matching the markdown-cell
//! path — the same simplification the plan's own "Boundaries known now"
//! section already models for block-comment languages and YAML front
//! matter. A raw cell's `format`/`raw_mimetype` attribute, if present, is
//! parsed but currently unused.

use std::ops::Range;
use std::path::Path;

use regex::Regex;

use super::line_writer::{LineSpan, Writer, scan_lines, trim_blank_lines};
use super::{ContentProcessor, Converted, ProcessorContext, ProcessorError, ProcessorParams};

pub struct Percent;

impl ContentProcessor for Percent {
    fn sniff(&self, _path: &Path, content: &str, params: &ProcessorParams) -> bool {
        let ProcessorParams::Percent { comment_open, .. } = params else {
            return false;
        };
        sniff_pattern(comment_open).is_match(content)
    }

    fn convert(
        &self,
        _path: &Path,
        content: &str,
        params: &ProcessorParams,
        _ctx: &ProcessorContext,
    ) -> Result<Converted, ProcessorError> {
        let ProcessorParams::Percent {
            comment_open,
            fence_language,
        } = params
        else {
            return Err(ProcessorError::Other(
                "percent processor requires Percent params".to_string(),
            ));
        };
        Ok(convert_percent(content, comment_open, fence_language))
    }
}

/// `^\s*{comment}\s*%%+\s+\[(markdown|raw)\]` (multiline) — a bare `# %%`
/// code marker is NOT enough; detection requires a markdown/raw marker
/// (Q1-faithful, see `percent-script.ts`'s own "DETECTION" doc comment).
fn sniff_pattern(comment_open: &str) -> Regex {
    Regex::new(&format!(
        r"(?m)^\s*{}\s*%%+\s+\[(markdown|raw)\]",
        regex::escape(comment_open)
    ))
    .expect("sniff_pattern must compile")
}

/// `^\s*{comment}\s*%%+\s*(?:\[(markdown|raw)\])?\s*(.*)?$`
fn header_pattern(comment_open: &str) -> Regex {
    Regex::new(&format!(
        r"^\s*{}\s*%%+\s*(?:\[(markdown|raw)\])?\s*(.*)?$",
        regex::escape(comment_open)
    ))
    .expect("header_pattern must compile")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CellKind {
    Code,
    Markdown,
    Raw,
}

struct Header {
    kind: CellKind,
    /// Parsed `key=value` attributes from the header's trailing text, in
    /// source order. Empty when the header carries no attributes.
    metadata: Vec<(String, String)>,
}

struct Cell {
    header: Header,
    header_span: LineSpan,
    /// Every body line up to (not including) the next header, BEFORE
    /// `trimEmptyLines` — needed so leading/trailing blanks that get
    /// dropped from rendering still get a deletion piece (provenance must
    /// tile the whole file, not just the rendered subset).
    body: Vec<LineSpan>,
}

/// Parse `attribs` (the header's trailing text) into ordered `key=value`
/// pairs. Port of `pandocAttrKeyvalueFromText(text, " ")` restricted to the
/// space-separated form percent headers use — quoted values may contain
/// spaces; an optional leading bare title token (no `=`) is skipped, per
/// `parsePercentAttribs`'s `match(/[\w-]+=.*$/)`.
fn parse_percent_attribs(attribs: &str) -> Vec<(String, String)> {
    let Some(start) = Regex::new(r"[\w-]+=")
        .unwrap()
        .find(attribs)
        .map(|m| m.start())
    else {
        return Vec::new();
    };
    let kv_text = &attribs[start..];

    // Replace spaces outside double-quotes with newlines, then split.
    let mut converted = String::with_capacity(kv_text.len());
    let mut in_quotes = false;
    for ch in kv_text.chars() {
        if ch == '"' {
            in_quotes = !in_quotes;
            converted.push(ch);
        } else if ch == ' ' && !in_quotes {
            converted.push('\n');
        } else {
            converted.push(ch);
        }
    }

    converted
        .trim()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let line = line.trim();
            match line.split_once('=') {
                Some((k, v)) => {
                    let v = v.trim_matches('"');
                    (k.to_string(), v.to_string())
                }
                None => (line.to_string(), String::new()),
            }
        })
        .collect()
}

fn parse_header(comment_open: &str, text: &str, span: &LineSpan) -> Option<Header> {
    let line = &text[span.content_range()];
    let caps = header_pattern(comment_open).captures(line)?;
    let kind = match caps.get(1).map(|m| m.as_str()) {
        Some("markdown") => CellKind::Markdown,
        Some("raw") => CellKind::Raw,
        _ => CellKind::Code,
    };
    let attribs_text = caps.get(2).map_or("", |m| m.as_str());
    Some(Header {
        kind,
        metadata: parse_percent_attribs(attribs_text),
    })
}

fn split_cells(content: &str, comment_open: &str, range: Range<usize>) -> Vec<Cell> {
    let mut cells: Vec<Cell> = Vec::new();
    for span in scan_lines(content, range) {
        if let Some(header) = parse_header(comment_open, content, &span) {
            cells.push(Cell {
                header,
                header_span: span,
                body: Vec::new(),
            });
        } else if let Some(cell) = cells.last_mut() {
            cell.body.push(span);
        }
        // Lines before the first header are dropped (Q1-faithful: `activeCell()?.lines.push`
        // is a no-op when `cells` is empty).
    }
    cells
}

fn is_triple_quote(content: &str, span: &LineSpan) -> bool {
    Regex::new(r#"^"{3,}\s*$"#)
        .unwrap()
        .is_match(content[span.content_range()].trim_start())
}

/// Render one cell's kept lines (the `trim_blank_lines` sub-range), joined
/// by a synthetic `"\n"` — mirrors `cellLines.join("\n")`. `strip_prefix`
/// controls whether each line's comment marker is removed (markdown/raw,
/// non-triple-quote) or kept as-is (code, and triple-quote interiors).
fn write_cell_lines(w: &mut Writer, lines: &[LineSpan], comment_open: &str, strip_prefix: bool) {
    let prefix_re =
        Regex::new(&format!(r"^{}\s?", regex::escape(comment_open))).expect("prefix regex");
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            w.synthetic("\n");
        }
        let content_range = line.content_range();
        if strip_prefix {
            let text = &w.content[content_range.clone()];
            let prefix_len = prefix_re.find(text).map_or(0, |m| m.end());
            let prefix_end = content_range.start + prefix_len;
            if prefix_len > 0 {
                w.delete(content_range.start..prefix_end);
            }
            w.keep(prefix_end..content_range.end);
        } else {
            w.keep(content_range);
        }
        w.delete(line.newline_range());
    }
}

fn write_metadata_options(w: &mut Writer, comment_open: &str, metadata: &[(String, String)]) {
    for (key, value) in metadata {
        w.synthetic(&format!("{comment_open}| {key}: {value}\n"));
    }
}

fn convert_percent(content: &str, comment_open: &str, fence_language: &str) -> Converted {
    // Q1 trims the whole file before splitting into lines; leading/trailing
    // whitespace outside any cell has no output, so it is simply excluded
    // from the walked range (never referenced by any piece).
    let trimmed_start = content.len() - content.trim_start().len();
    let trimmed_end = content.trim_end().len();
    let range = if trimmed_start >= trimmed_end {
        0..0
    } else {
        trimmed_start..trimmed_end
    };

    let cells = split_cells(content, comment_open, range.clone());

    let mut w = Writer::new(content, range.start);
    for cell in &cells {
        w.delete(cell.header_span.whole_range());

        // `trim_empty_lines` decides which body lines are rendered, but
        // EVERY body line's source bytes must be accounted for — a leading/
        // trailing blank line dropped from rendering still needs a
        // deletion piece so the provenance builder's tiling stays
        // contiguous across the whole file.
        let Some(kept_range) = trim_blank_lines(content, &cell.body) else {
            for line in &cell.body {
                w.delete(line.whole_range());
            }
            continue;
        };
        for line in &cell.body[..kept_range.start] {
            w.delete(line.whole_range());
        }
        let kept = &cell.body[kept_range.clone()];

        match cell.header.kind {
            CellKind::Code => {
                w.synthetic(&format!("```{{{fence_language}}}\n"));
                write_metadata_options(&mut w, comment_open, &cell.header.metadata);
                if !cell.header.metadata.is_empty() && !kept.is_empty() {
                    w.synthetic("\n");
                }
                write_cell_lines(&mut w, kept, comment_open, false);
                if !kept.is_empty() {
                    w.synthetic("\n");
                }
                w.synthetic("```\n\n");
            }
            CellKind::Markdown | CellKind::Raw => {
                if kept.len() > 2
                    && is_triple_quote(content, &kept[0])
                    && is_triple_quote(content, &kept[kept.len() - 1])
                {
                    w.delete(kept[0].whole_range());
                    write_cell_lines(&mut w, &kept[1..kept.len() - 1], comment_open, false);
                    w.delete(kept[kept.len() - 1].whole_range());
                    w.synthetic("\n\n");
                } else {
                    write_cell_lines(&mut w, kept, comment_open, true);
                    w.synthetic("\n\n");
                }
            }
        }

        for line in &cell.body[kept_range.end..] {
            w.delete(line.whole_range());
        }
    }

    let (markdown, source_info) = w.finish();
    Converted {
        markdown,
        source_info,
        files: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::ORIGINAL_FILE_ID;
    use super::*;

    const PY: &str = "#";

    fn percent_of(content: &str) -> Converted {
        convert_percent(content, PY, "python")
    }

    // --- T-percent (Phase 3): sniff. ---

    #[test]
    fn sniff_requires_markdown_or_raw_marker() {
        let params = ProcessorParams::Percent {
            comment_open: "#".to_string(),
            fence_language: "python".to_string(),
        };
        assert!(
            !Percent.sniff(Path::new("x.py"), "# %%\nprint(1)\n", &params),
            "bare code marker alone must not be detected"
        );
        assert!(Percent.sniff(Path::new("x.py"), "# %% [markdown]\n# hi\n", &params));
        assert!(Percent.sniff(Path::new("x.py"), "# %% [raw]\nhi\n", &params));
    }

    // --- T-percent: cell-kind conversion. ---

    #[test]
    fn markdown_cell_strips_comment_prefix() {
        let out = percent_of("# %% [markdown]\n# hello\n# world\n").markdown;
        assert_eq!(out, "hello\nworld\n\n");
    }

    #[test]
    fn markdown_cell_triple_quote_kept_verbatim() {
        let out = percent_of("# %% [markdown]\n\"\"\"\nhello *world*\n\"\"\"\n").markdown;
        assert_eq!(out, "hello *world*\n\n");
    }

    #[test]
    fn code_cell_fences_with_language() {
        let out = percent_of("# %%\nprint(1)\nprint(2)\n").markdown;
        assert_eq!(out, "```{python}\nprint(1)\nprint(2)\n```\n\n");
    }

    #[test]
    fn code_cell_with_attribs_emits_pipe_options() {
        let out = percent_of("# %% label=fig-1\nplot()\n").markdown;
        assert_eq!(out, "```{python}\n#| label: fig-1\n\nplot()\n```\n\n");
    }

    #[test]
    fn raw_cell_strips_comment_prefix_like_markdown() {
        let out = percent_of("# %% [raw]\n# <div>hi</div>\n").markdown;
        assert_eq!(out, "<div>hi</div>\n\n");
    }

    #[test]
    fn multiple_cells_concatenate_in_order() {
        let out = percent_of("# %% [markdown]\n# title\n# %%\nprint(1)\n# %% [markdown]\n# done\n")
            .markdown;
        assert_eq!(out, "title\n\n```{python}\nprint(1)\n```\n\ndone\n\n");
    }

    #[test]
    fn code_only_script_with_no_markdown_cell_still_converts() {
        // Detection (sniff) requires a markdown/raw marker, but convert()
        // itself is unconditional once a caller has decided to invoke it.
        let out = percent_of("# %%\nprint(1)\n").markdown;
        assert_eq!(out, "```{python}\nprint(1)\n```\n\n");
    }

    #[test]
    fn blank_lines_at_cell_boundaries_are_trimmed_from_output() {
        let out = percent_of("# %% [markdown]\n\n# hello\n\n\n# %%\nprint(1)\n").markdown;
        assert_eq!(out, "hello\n\n```{python}\nprint(1)\n```\n\n");
    }

    // --- T-percent-src: SourceInfo maps output positions to the original file. ---

    #[test]
    fn source_info_maps_markdown_cell_to_original_line() {
        let content = "# %% [markdown]\n# hello\n# world\n";
        let converted = percent_of(content);
        assert_eq!(converted.markdown, "hello\nworld\n\n");

        let mut ctx = quarto_source_map::SourceContext::new();
        ctx.add_file_with_id(
            ORIGINAL_FILE_ID,
            "x.py".to_string(),
            Some(content.to_string()),
        );

        // Byte 0 of the output ("h" of "hello") must resolve to the "h" in
        // "# hello" (line 1, right after the stripped "# " prefix).
        let mapped = converted
            .source_info
            .map_offset(0, &ctx)
            .expect("must resolve");
        assert_eq!(mapped.location.row, 1);
        assert_eq!(mapped.location.column, 2);

        // "world" starts at output byte 6 ("hello\n" = 6 bytes).
        let mapped = converted
            .source_info
            .map_offset(6, &ctx)
            .expect("must resolve");
        assert_eq!(mapped.location.row, 2);
        assert_eq!(mapped.location.column, 2);
    }

    #[test]
    fn source_info_maps_code_cell_body_to_original_line() {
        let content = "# %%\nprint(1)\nprint(2)\n";
        let converted = percent_of(content);
        assert_eq!(
            converted.markdown,
            "```{python}\nprint(1)\nprint(2)\n```\n\n"
        );

        let mut ctx = quarto_source_map::SourceContext::new();
        ctx.add_file_with_id(
            ORIGINAL_FILE_ID,
            "x.py".to_string(),
            Some(content.to_string()),
        );

        // "print(1)" starts right after the synthetic "```{python}\n" (12 bytes).
        let mapped = converted
            .source_info
            .map_offset(12, &ctx)
            .expect("must resolve");
        assert_eq!(mapped.location.row, 1);
        assert_eq!(mapped.location.column, 0);

        // "print(2)" starts after "```{python}\nprint(1)\n" (12 + 9 = 21 bytes).
        let mapped = converted
            .source_info
            .map_offset(21, &ctx)
            .expect("must resolve");
        assert_eq!(mapped.location.row, 2);
        assert_eq!(mapped.location.column, 0);
    }

    #[test]
    fn source_info_synthetic_fence_line_resolves_to_its_zero_width_anchor() {
        // A synthetic insertion (fence line, no source bytes) is a
        // zero-width `ProvenanceBuilder` piece anchored at the exact
        // source position it was inserted at — not "no location" (that
        // would need a `Generated` node, which this builder never emits).
        // Byte 0 is the synthetic "`" of "```{python}", inserted right
        // after "# %%\n" (5 bytes) — so it anchors to row 1, col 0 (the
        // start of the next real line, "print(1)").
        let content = "# %%\nprint(1)\n";
        let converted = percent_of(content);
        let mut ctx = quarto_source_map::SourceContext::new();
        ctx.add_file_with_id(
            ORIGINAL_FILE_ID,
            "x.py".to_string(),
            Some(content.to_string()),
        );
        let mapped = converted
            .source_info
            .map_offset(0, &ctx)
            .expect("a zero-width anchor still resolves to a position");
        assert_eq!(mapped.location.row, 1);
        assert_eq!(mapped.location.column, 0);
    }
}
