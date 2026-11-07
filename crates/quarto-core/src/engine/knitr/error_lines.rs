/*
 * engine/knitr/error_lines.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! EXR-11 Phase 11D: map knitr's "Quitting from lines X-Y" back to the document.
//!
//! The line numbers knitr reports are lines of the text q2 handed to R, which differs
//! from the text the stage gave the engine (inline-R resolution, `quarto-unit-id` and
//! `label:` option lines) and from the author's file (the stage serializes the AST, and
//! included files are spliced in). Two steps undo that:
//!
//! 1. [`map_to_engine_lines`] relocates the range onto the engine's input by chunk: the
//!    k-th chunk of one text is the k-th chunk of the other, and a line inside a chunk
//!    keeps its distance from the first code line, so option lines q2 added do not
//!    shift it.
//! 2. [`engine_lines_to_document`] resolves those lines through the stage's source map.
//!
//! The unit id is deliberately not the knitr label: labels name output files.

use std::ops::RangeInclusive;
use std::path::Path;

use crate::engine::context::ExecutionContext;
use crate::engine::units::{CellUnit, Unit, scan_units};

/// One chunk's lines, 1-based and inclusive.
struct ChunkLines {
    opener: usize,
    /// First line after the opener and its leading `#|` option lines.
    body: usize,
    closer: usize,
}

fn line_of(text: &str, offset: usize) -> usize {
    text[..offset.min(text.len())].matches('\n').count() + 1
}

fn chunks(text: &str) -> Vec<ChunkLines> {
    let lines: Vec<&str> = text.split('\n').collect();
    scan_units(text)
        .into_iter()
        .filter_map(|u| match u {
            Unit::Cell(c) => Some(chunk_lines(text, &lines, &c)),
            Unit::Inline(_) => None,
        })
        .collect()
}

fn chunk_lines(text: &str, lines: &[&str], c: &CellUnit) -> ChunkLines {
    let opener = line_of(text, c.start);
    // `end` may sit after the closer's trailing newline; the closer is the last
    // non-blank line at or before it.
    let mut closer = line_of(text, c.end.saturating_sub(1)).min(lines.len());
    while closer > opener && lines[closer - 1].trim().is_empty() {
        closer -= 1;
    }
    let mut body = opener + 1;
    while body < closer {
        let l = lines[body - 1].trim_start_matches(['>', ' ']).trim_start();
        if l.starts_with("#|") {
            body += 1;
        } else {
            break;
        }
    }
    ChunkLines {
        opener,
        body,
        closer,
    }
}

/// Relocate `lines` (in the text handed to R, `r_text`) onto `engine_text`. `None` when
/// the range is not inside a chunk, or the two texts disagree on the chunk count.
pub(super) fn map_to_engine_lines(
    engine_text: &str,
    r_text: &str,
    lines: RangeInclusive<usize>,
) -> Option<RangeInclusive<usize>> {
    let from = chunks(r_text);
    let to = chunks(engine_text);
    if from.len() != to.len() {
        return None;
    }
    let k = from
        .iter()
        .position(|c| c.opener <= *lines.start() && *lines.start() <= c.closer)?;
    let (a, b) = (&from[k], &to[k]);
    let map = |l: usize| -> usize {
        if l < a.body {
            b.opener
        } else {
            (b.body + (l - a.body)).min(b.closer)
        }
    };
    let start = map(*lines.start());
    let end = map((*lines.end()).min(a.closer)).max(start);
    Some(start..=end)
}

/// EXR-12: relocate one line of the text handed to R onto `engine_text` when it holds an
/// inline span (a `` `r `` code span; the rewrite never changes a line count). Lines of
/// chunks are [`map_to_engine_lines`]'s; between chunks the only edits are inside chunks, so
/// the line keeps its distance from the closer of the last chunk above it. `None` for a line
/// with no span, or when the two texts do not hold the same chunks.
pub(super) fn map_span_line_to_engine_line(
    engine_text: &str,
    r_text: &str,
    line: usize,
) -> Option<usize> {
    if !r_text
        .split('\n')
        .nth(line.checked_sub(1)?)?
        .contains("`r ")
    {
        return None;
    }
    let (from, to) = (chunks(r_text), chunks(engine_text));
    if from.len() != to.len() {
        return None;
    }
    Some(match from.iter().rposition(|c| c.closer < line) {
        Some(k) => to[k].closer + (line - from[k].closer),
        None => line,
    })
}

/// Resolve `lines` of the engine's input to lines of the document's own file, through the
/// stage's source map. `None` when a line maps into another file (an include) or nowhere.
pub(super) fn engine_lines_to_document(
    ctx: &ExecutionContext,
    engine_text: &str,
    lines: RangeInclusive<usize>,
) -> Option<RangeInclusive<usize>> {
    let row = |line: usize| -> Option<usize> {
        let offset: usize = engine_text
            .split_inclusive('\n')
            .take(line.checked_sub(1)?)
            .map(str::len)
            .sum();
        let mapped = ctx.source_info.map_offset(offset, &ctx.source_context)?;
        let file = ctx.source_context.get_file(mapped.file_id)?;
        same_file(Path::new(&file.path), &ctx.source_path).then_some(mapped.location.row + 1)
    };
    Some(row(*lines.start())?..=row(*lines.end())?)
}

fn same_file(a: &Path, b: &Path) -> bool {
    a == b
        || match (a.canonicalize(), b.canonicalize()) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENGINE: &str = "Intro\n\n```{r}\nx <- 1\nstop('a')\n```\n\nMid\n\n```{r}\n#| echo: false\ny\nstop('b')\n```\n";

    #[test]
    fn a_span_line_keeps_its_distance_from_the_chunk_above() {
        let engine = "Top `r a`\n\n```{r}\nx\n```\n\nMid `r b`\n";
        let r = "Top `r a`\n\n```{r}\n#| quarto-unit-id: u\n#| label: l\nx\n```\n\nMid `r b`\n";
        assert_eq!(map_span_line_to_engine_line(engine, r, 1), Some(1));
        assert_eq!(map_span_line_to_engine_line(engine, r, 9), Some(7));
        // A line with no span, or one the texts disagree about, does not map.
        assert_eq!(map_span_line_to_engine_line(engine, r, 2), None);
        assert_eq!(map_span_line_to_engine_line(engine, "`r z`\n", 1), None);
    }

    #[test]
    fn identical_texts_map_to_themselves() {
        assert_eq!(map_to_engine_lines(ENGINE, ENGINE, 3..=6), Some(3..=6));
        assert_eq!(map_to_engine_lines(ENGINE, ENGINE, 13..=14), Some(13..=14));
    }

    #[test]
    fn added_option_lines_do_not_shift_body_lines() {
        // q2 inserted an id line and a label line into both chunks
        let r = "Intro\n\n```{r}\n#| quarto-unit-id: a\n#| label: fig-x\nx <- 1\nstop('a')\n```\n\nMid\n\n```{r}\n#| quarto-unit-id: b\n#| echo: false\ny\nstop('b')\n```\n";
        // `stop('a')` is line 7 in r, line 5 in the engine text
        assert_eq!(map_to_engine_lines(ENGINE, r, 7..=7), Some(5..=5));
        // `stop('b')` is line 16 in r, line 13 in the engine text
        assert_eq!(map_to_engine_lines(ENGINE, r, 16..=16), Some(13..=13));
    }

    #[test]
    fn a_whole_chunk_range_maps_to_the_whole_chunk() {
        let r = "Intro\n\n```{r}\n#| quarto-unit-id: a\nx <- 1\nstop('a')\n```\n\nMid\n\n```{r}\n#| quarto-unit-id: b\n#| echo: false\ny\nstop('b')\n```\n";
        assert_eq!(map_to_engine_lines(ENGINE, r, 3..=7), Some(3..=6));
    }

    #[test]
    fn a_range_outside_every_chunk_does_not_map() {
        assert_eq!(map_to_engine_lines(ENGINE, ENGINE, 1..=1), None);
    }

    #[test]
    fn disagreeing_chunk_counts_do_not_map() {
        assert_eq!(map_to_engine_lines(ENGINE, "```{r}\nx\n```\n", 2..=2), None);
    }
}
