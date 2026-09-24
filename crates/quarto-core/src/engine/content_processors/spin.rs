/*
 * engine/content_processors/spin.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * `spin` content processor.
 */

//! `spin` content processor.
//!
//! Native reimplementation of `knitr::spin`'s `qmd`-format branch (read in
//! full this session from `~/src/knitr/R/spin.R`), targeting byte-identical
//! output to the real `knitr::spin(format = "qmd")` — see the committed
//! golden corpus at `tests/fixtures/spin-goldens/`.
//!
//! **`matchable`** (is a `#'`/chunk-marker line a real top-level R token, or
//! text inside a string/comment?) is computed via `tree-sitter-r`: parse the
//! (comment-stripped) file; on a parse error, knitr's own fallback applies
//! (every line matchable); otherwise a line is matchable iff some AST node
//! starts at column 0 on it (mirrors R's own `getParseData()$col1 == 1`).
//!
//! **Scope** (mirrors the plan's own "Scope for this plan"): the `qmd`/`Rmd`
//! markdown branch only. `matchable` gates ONLY the outer doc-vs-code
//! classification — confirmed against a real `knitr::spin` run
//! (`string-embedded-marker` golden) that the *internal* chunk-delimiter
//! (`rc`)/pipe-comment detection inside an already-code-classified block is
//! a **pure textual regex match, not gated by `matchable` at all** — a
//! surprising nuance this session's golden generation caught. Inline
//! `{{ expr }}` expansion is out of scope (not ported).

use std::path::Path;

use regex::Regex;

use super::line_writer::{LineSpan, Writer, scan_lines, trim_blank_lines};
use super::{ContentProcessor, Converted, ProcessorContext, ProcessorError, ProcessorParams};

pub struct Spin;

impl ContentProcessor for Spin {
    fn sniff(&self, _path: &Path, content: &str, _params: &ProcessorParams) -> bool {
        // The roxygen `#' ---` ... `#' ---` YAML header is what identifies a
        // spinnable file (matches knitr's own detection convention).
        Regex::new(r"(?m)^\s*#'\s*---[\s\S]+?^\s*#'\s*---")
            .unwrap()
            .is_match(content)
    }

    fn convert(
        &self,
        _path: &Path,
        content: &str,
        _params: &ProcessorParams,
        _ctx: &ProcessorContext,
    ) -> Result<Converted, ProcessorError> {
        convert_spin(content)
    }
}

/// `doc = "^#+'[ ]?"` — one-or-more `#`, a `'`, an optional single space.
fn doc_pattern() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^#+'[ ]?").unwrap())
}

/// `rc = "^(#|--)+(\+| %%| ----+| @knitr)(.*?)\s*-*\s*$"`. Group 1 is the
/// captured "options text" (R's group 3 — groups 1/2 of the original are
/// non-capturing here since only the options text is ever extracted).
fn chunk_delim_pattern() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(?:#|--)+(?:\+| %%| ----+| @knitr)(.*?)\s*-*\s*$").unwrap())
}

/// Block-comment delimiters: `comment = c("^[# ]*/[*]", "^.*[*]/ *$")`.
fn comment_start_pattern() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[# ]*/\*").unwrap())
}

fn comment_end_pattern() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^.*\*/ *$").unwrap())
}

/// Native, zero-load `matchable` check (Plan 7b Phase 0 spike, formalized):
/// a line is matchable iff some tree-sitter-r node starts at column 0 on
/// it — mirrors R's own `getParseData()$col1 == 1`. A parse error falls
/// back to knitr's own behavior: every line matchable (`spin.R:73`).
fn matchable_lines(text: &str, n_lines: usize) -> Vec<bool> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_r::LANGUAGE.into())
        .expect("tree-sitter-r grammar must load");
    let Some(tree) = parser.parse(text, None) else {
        return vec![true; n_lines];
    };
    if tree.root_node().has_error() {
        return vec![true; n_lines];
    }
    let mut matchable = vec![false; n_lines];
    visit_all(tree.root_node(), &mut |node| {
        let pos = node.start_position();
        if pos.column == 0 && pos.row < n_lines {
            matchable[pos.row] = true;
        }
    });
    matchable
}

fn visit_all<'a>(node: tree_sitter::Node<'a>, f: &mut impl FnMut(tree_sitter::Node<'a>)) {
    f(node);
    for i in 0..node.child_count() as u32 {
        if let Some(child) = node.child(i) {
            visit_all(child, f);
        }
    }
}

/// Longest run of consecutive backticks anywhere in `text`; fence length is
/// `max(longest_run + 1, 3)` (`.fmt.rmd`).
fn fence_len(text: &str) -> usize {
    let mut longest = 0usize;
    let mut current = 0usize;
    for b in text.bytes() {
        if b == b'`' {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    (longest + 1).max(3)
}

/// Every line in the file, annotated with whether it survives block-comment
/// removal. Removed lines still appear (so the caller can `delete()` their
/// source range for provenance tiling) but are excluded from `matchable`
/// and from doc/code classification.
struct Line {
    span: LineSpan,
    removed: bool,
}

/// Apply `comment = c("^[# ]*/[*]", "^.*[*]/ *$")`: pair up start/end
/// delimiter lines in order and mark every line in each inclusive range as
/// removed. Mismatched start/end counts is a hard error (Q1/knitr-faithful:
/// `stopifnot` in `spin.R`).
fn mark_removed_comment_lines(content: &str, lines: &mut [Line]) -> Result<(), ProcessorError> {
    let starts: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| comment_start_pattern().is_match(&content[l.span.content_range()]))
        .map(|(i, _)| i)
        .collect();
    let ends: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| comment_end_pattern().is_match(&content[l.span.content_range()]))
        .map(|(i, _)| i)
        .collect();
    if starts.len() != ends.len() {
        return Err(ProcessorError::Other(
            "spin: block comments must be paired start/end delimiters".to_string(),
        ));
    }
    for (&s, &e) in starts.iter().zip(ends.iter()) {
        for line in &mut lines[s..=e] {
            line.removed = true;
        }
    }
    Ok(())
}

fn convert_spin(content: &str) -> Result<Converted, ProcessorError> {
    let mut lines: Vec<Line> = scan_lines(content, 0..content.len())
        .into_iter()
        .map(|span| Line {
            span,
            removed: false,
        })
        .collect();
    mark_removed_comment_lines(content, &mut lines)?;

    let active: Vec<&Line> = lines.iter().filter(|l| !l.removed).collect();
    let active_text: String = active
        .iter()
        .map(|l| &content[l.span.content_range()])
        .collect::<Vec<_>>()
        .join("\n");
    let matchable = matchable_lines(&active_text, active.len());
    let backtick_fence = "`".repeat(fence_len(&active_text));
    let p1 = format!("{backtick_fence}{{r");
    let p2 = "}";
    let p3 = &backtick_fence;

    // Classify each ACTIVE line as doc (matchable && doc-pattern) or code,
    // then group consecutive same-classification lines (`rle`).
    let is_doc: Vec<bool> = active
        .iter()
        .zip(matchable.iter())
        .map(|(l, &m)| m && doc_pattern().is_match(&content[l.span.content_range()]))
        .collect();

    let mut w = Writer::new(content, 0);
    // `flush_idx` walks the FULL, unfiltered `lines` in lockstep with the
    // active-line processing below, so a removed block-comment line — which
    // can fall *inside* a doc/code group's active-line span, not only at
    // group boundaries — gets deleted at exactly the right point in the
    // sequential source walk. Every active-line write below is preceded by
    // a `flush_removed(..., line.span.start)` call for this reason.
    let mut flush_idx = 0usize;
    let mut active_idx = 0usize;
    while active_idx < active.len() {
        let mut end = active_idx + 1;
        while end < active.len() && is_doc[end] == is_doc[active_idx] {
            end += 1;
        }
        if is_doc[active_idx] {
            write_doc_block(&mut w, &lines, &mut flush_idx, &active[active_idx..end]);
        } else {
            write_code_block(
                &mut w,
                &lines,
                &mut flush_idx,
                &active[active_idx..end],
                &p1,
                p2,
                p3,
            );
        }
        active_idx = end;
    }
    // Trailing removed lines after the last active line.
    let eof = lines.last().map_or(0, |l| l.span.full_end);
    flush_removed(&mut w, &lines, &mut flush_idx, eof);

    let (markdown, source_info) = w.finish();
    Ok(Converted {
        markdown,
        source_info,
        files: Vec::new(),
    })
}

/// Delete every line in `lines[*flush_idx..]` whose start precedes
/// `target_start` — the removed block-comment lines sitting between the
/// previous and next active line (§ `convert_spin`'s `flush_idx` doc).
fn flush_removed(w: &mut Writer, lines: &[Line], flush_idx: &mut usize, target_start: usize) {
    while *flush_idx < lines.len() && lines[*flush_idx].span.start < target_start {
        if lines[*flush_idx].removed {
            w.delete(lines[*flush_idx].span.whole_range());
        }
        *flush_idx += 1;
    }
}

/// A doc block: strip the `doc` prefix from each line, no block wrapping.
fn write_doc_block(w: &mut Writer, lines: &[Line], flush_idx: &mut usize, active: &[&Line]) {
    for line in active {
        flush_removed(w, lines, flush_idx, line.span.start);
        let range = line.span.content_range();
        let text = &w.content[range.clone()];
        let prefix_len = doc_pattern().find(text).map_or(0, |m| m.end());
        let prefix_end = range.start + prefix_len;
        if prefix_len > 0 {
            w.delete(range.start..prefix_end);
        }
        w.keep(prefix_end..range.end);
        w.delete(line.span.newline_range());
        w.synthetic("\n");
    }
}

/// A code block: `strip_white`, chunk-delimiter detection + fence
/// insertion, then the block-level blank-line wrap.
fn write_code_block(
    w: &mut Writer,
    lines: &[Line],
    flush_idx: &mut usize,
    active: &[&Line],
    p1: &str,
    p2: &str,
    p3: &str,
) {
    let spans: Vec<LineSpan> = active.iter().map(|l| l.span).collect();
    let Some(kept_range) = trim_blank_lines(w.content, &spans) else {
        for line in active {
            flush_removed(w, lines, flush_idx, line.span.start);
            w.delete(line.span.whole_range());
        }
        return;
    };
    for line in &active[..kept_range.start] {
        flush_removed(w, lines, flush_idx, line.span.start);
        w.delete(line.span.whole_range());
    }
    let kept = &active[kept_range.clone()];

    // j1: lines matching the chunk-delimiter regex (rc) — a PURE textual
    // match over the kept lines' content, deliberately NOT gated by
    // `matchable` (see the module doc's "surprising nuance").
    let texts: Vec<&str> = kept
        .iter()
        .map(|l| &w.content[l.span.content_range()])
        .collect();
    let j1: Vec<usize> = texts
        .iter()
        .enumerate()
        .filter(|(_, t)| chunk_delim_pattern().is_match(t))
        .map(|(i, _)| i)
        .collect();
    // j2: starts of maximal `#| ` runs, excluding any that is j1's line + 1.
    let j2: Vec<usize> = pipe_comment_starts(&texts)
        .into_iter()
        .filter(|i| !j1.iter().any(|&j| j + 1 == *i))
        .collect();
    let mut j3: Vec<usize> = j1.iter().chain(j2.iter()).copied().collect();
    j3.sort_unstable();
    j3.dedup();

    w.synthetic("\n");
    for (i, line) in kept.iter().enumerate() {
        flush_removed(w, lines, flush_idx, line.span.start);
        let is_j1 = j1.contains(&i);
        let is_j2 = j2.contains(&i);
        let needs_close_before = j3.contains(&i) && i > 0;
        if needs_close_before {
            w.synthetic(&format!("{p3}\n"));
        }
        if i == 0 && !is_j1 && !is_j2 {
            w.synthetic(&format!("{p1}{p2}\n"));
        }
        if is_j1 {
            let caps = chunk_delim_pattern().captures(texts[i]).unwrap();
            let options = caps.get(1).map_or("", |m| m.as_str());
            w.delete(line.span.whole_range());
            w.synthetic(&format!("{p1}{options}{p2}\n"));
        } else if is_j2 {
            w.synthetic(&format!("{p1}{p2}\n"));
            w.keep(line.span.content_range());
            w.delete(line.span.newline_range());
            w.synthetic("\n");
        } else {
            w.keep(line.span.content_range());
            w.delete(line.span.newline_range());
            w.synthetic("\n");
        }
    }
    w.synthetic(&format!("{p3}\n"));
    w.synthetic("\n");

    for line in &active[kept_range.end..] {
        flush_removed(w, lines, flush_idx, line.span.start);
        w.delete(line.span.whole_range());
    }
}

/// `pipe_comment_start`: the start index of each maximal run of
/// `startsWith(x, "#| ")` lines.
fn pipe_comment_starts(texts: &[&str]) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut run_start: Option<usize> = None;
    for (i, t) in texts.iter().enumerate() {
        let is_pipe = t.starts_with("#| ");
        match (is_pipe, run_start) {
            (true, None) => run_start = Some(i),
            (true, Some(_)) => {}
            (false, Some(s)) => {
                starts.push(s);
                run_start = None;
            }
            (false, None) => {}
        }
    }
    if let Some(s) = run_start {
        starts.push(s);
    }
    starts
}

#[cfg(test)]
mod tests {
    use super::super::ORIGINAL_FILE_ID;
    use super::*;

    fn golden(name: &str) -> (String, String) {
        let base =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/spin-goldens");
        let r = std::fs::read_to_string(base.join(format!("{name}.R"))).unwrap();
        let qmd = std::fs::read_to_string(base.join(format!("{name}.qmd"))).unwrap();
        (r, qmd)
    }

    fn assert_golden(name: &str) {
        let (r_src, expected) = golden(name);
        let converted = Spin
            .convert(
                Path::new(&format!("{name}.R")),
                &r_src,
                &ProcessorParams::Spin,
                &ProcessorContext {
                    runtime: std::sync::Arc::new(quarto_system_runtime::NativeRuntime::new()),
                },
            )
            .unwrap_or_else(|e| panic!("{name}: convert failed: {e}"));
        assert_eq!(
            converted.markdown, expected,
            "{name}: converted output must byte-match the committed knitr golden"
        );
    }

    // --- T-spin-golden (Phase 4): committed knitr golden corpus. ---

    #[test]
    fn golden_basic() {
        assert_golden("basic");
    }

    #[test]
    fn golden_chunk_options_plus() {
        assert_golden("chunk-options-plus");
    }

    #[test]
    fn golden_percent_delim() {
        assert_golden("percent-delim");
    }

    #[test]
    fn golden_dash_delim() {
        assert_golden("dash-delim");
    }

    #[test]
    fn golden_knitr_delim() {
        assert_golden("knitr-delim");
    }

    #[test]
    fn golden_pipe_options() {
        assert_golden("pipe-options");
    }

    #[test]
    fn golden_bare_code() {
        assert_golden("bare-code");
    }

    #[test]
    fn golden_multi_chunk_block() {
        assert_golden("multi-chunk-block");
    }

    #[test]
    fn golden_backticks() {
        assert_golden("backticks");
    }

    #[test]
    fn golden_string_embedded_marker() {
        assert_golden("string-embedded-marker");
    }

    #[test]
    fn golden_block_comment() {
        assert_golden("block-comment");
    }

    #[test]
    fn golden_parse_error_fallback() {
        assert_golden("parse-error-fallback");
    }

    // --- T-spin-matchable (Phase 4). ---

    #[test]
    fn matchable_excludes_string_embedded_marker() {
        let text = "x <- \"a\n#' fake marker inside string\nc\"\n#' real marker\n";
        let m = matchable_lines(text, 4);
        assert!(!m[1], "line inside the string must not be matchable");
        assert!(m[3], "the real top-level comment must be matchable");
    }

    #[test]
    fn matchable_falls_back_to_all_true_on_parse_error() {
        let text = "#' marker\nthis is not { valid R (((\n";
        let m = matchable_lines(text, 2);
        assert!(
            m.iter().all(|&x| x),
            "a parse error must fall back to all-matchable"
        );
    }

    // --- SourceInfo across inserted fences + stripped prefixes. ---

    #[test]
    fn source_info_maps_doc_line_to_original() {
        let content = "#' hello\n#' world\n";
        let converted = Spin
            .convert(
                Path::new("x.R"),
                content,
                &ProcessorParams::Spin,
                &ProcessorContext {
                    runtime: std::sync::Arc::new(quarto_system_runtime::NativeRuntime::new()),
                },
            )
            .unwrap();
        assert_eq!(converted.markdown, "hello\nworld\n");

        let mut ctx = quarto_source_map::SourceContext::new();
        ctx.add_file_with_id(
            ORIGINAL_FILE_ID,
            "x.R".to_string(),
            Some(content.to_string()),
        );
        let mapped = converted.source_info.map_offset(0, &ctx).unwrap();
        assert_eq!(mapped.location.row, 0);
        assert_eq!(mapped.location.column, 3);

        let mapped = converted.source_info.map_offset(6, &ctx).unwrap();
        assert_eq!(mapped.location.row, 1);
        assert_eq!(mapped.location.column, 3);
    }

    #[test]
    fn source_info_maps_code_line_to_original() {
        let content = "#' doc\n1 + 1\n";
        let converted = Spin
            .convert(
                Path::new("x.R"),
                content,
                &ProcessorParams::Spin,
                &ProcessorContext {
                    runtime: std::sync::Arc::new(quarto_system_runtime::NativeRuntime::new()),
                },
            )
            .unwrap();
        // "doc\n\n```{r}\n1 + 1\n```\n\n"
        let mut ctx = quarto_source_map::SourceContext::new();
        ctx.add_file_with_id(
            ORIGINAL_FILE_ID,
            "x.R".to_string(),
            Some(content.to_string()),
        );
        let code_offset = converted.markdown.find("1 + 1").unwrap();
        let mapped = converted.source_info.map_offset(code_offset, &ctx).unwrap();
        assert_eq!(mapped.location.row, 1);
        assert_eq!(mapped.location.column, 0);
    }
}
