/*
 * table_caption_provenance.rs
 *
 * bd-t3enk8gq: the caption-extended table span must be built with a
 * same-file, preimage-based hull — not by pairing raw `start_offset()` /
 * `end_offset()` values (parent-relative for Substrings, sentinel 0 for
 * Concat/Generated) with `root_file_id().unwrap_or(FileId(0))`.
 *
 * The reachable symptom: parsing with a `parent_source_info` (a public
 * `qmd::read` parameter) makes every node a Substring whose offsets are
 * relative to the parsed buffer. The manual span construction in
 * section.rs / pipe_table.rs stamped those buffer-relative offsets into a
 * file-absolute `Original`, mis-anchoring the table by the parent's start
 * offset.
 *
 * Copyright (c) 2026 Posit, PBC
 */

use quarto_source_map::{FileId, SourceInfo};

/// Parse `input` as a substring of a larger fictional parent document that
/// starts at byte `parent_start` of file 42.
fn parse_with_parent(input: &str, parent_start: usize) -> pampa::pandoc::Pandoc {
    let parent = SourceInfo::original(FileId(42), parent_start, parent_start + input.len());
    let (ast, _ctx, warnings) = pampa::readers::qmd::read(
        input.as_bytes(),
        false,
        "child-buffer.qmd",
        &mut std::io::sink(),
        true,
        Some(parent),
    )
    .expect("parse failed");
    assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    ast
}

fn find_table(ast: &pampa::pandoc::Pandoc) -> &pampa::pandoc::Block {
    ast.blocks
        .iter()
        .find(|b| matches!(b, pampa::pandoc::Block::Table(_)))
        .expect("table block expected")
}

const PARENT_START: usize = 1000;

/// Standalone caption (blank line between table and `: caption`) — the
/// caption-attach pass in section.rs extends the table span.
#[test]
fn standalone_caption_table_span_resolves_into_parent_file() {
    let input = "| a | b |\n|---|---|\n| 1 | 2 |\n\n: My caption\n";
    let ast = parse_with_parent(input, PARENT_START);
    let table = find_table(&ast);

    let (fid, start, end) = table
        .source_info()
        .resolve_byte_range()
        .expect("table span must resolve to a byte range");
    assert_eq!(fid, 42, "table span must stay in the parent's file");
    assert_eq!(
        start, PARENT_START,
        "table start must be absolute in the parent file, not buffer-relative"
    );
    assert_eq!(
        end,
        PARENT_START + input.len(), // caption span includes the trailing newline
        "caption-extended end must be absolute in the parent file"
    );
}

/// Adjacent caption (`: caption` directly after the table) — the span is
/// extended during pipe-table conversion in pipe_table.rs.
#[test]
fn adjacent_caption_table_span_resolves_into_parent_file() {
    let input = "| a | b |\n|---|---|\n| 1 | 2 |\n: My caption\n";
    let ast = parse_with_parent(input, PARENT_START);
    let table = find_table(&ast);

    let (fid, start, _end) = table
        .source_info()
        .resolve_byte_range()
        .expect("table span must resolve to a byte range");
    assert_eq!(fid, 42, "table span must stay in the parent's file");
    assert_eq!(
        start, PARENT_START,
        "table start must be absolute in the parent file, not buffer-relative"
    );
}

/// Control: with no parent context the span is absolute in the parsed file
/// and includes the caption.
#[test]
fn caption_table_span_without_parent_is_unchanged() {
    let input = "| a | b |\n|---|---|\n| 1 | 2 |\n\n: My caption\n";
    let (ast, _ctx, _warnings) = pampa::readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    )
    .expect("parse failed");
    let table = find_table(&ast);
    let (fid, start, end) = table
        .source_info()
        .resolve_byte_range()
        .expect("table span must resolve");
    assert_eq!(fid, 0);
    assert_eq!(start, 0);
    assert_eq!(
        end,
        input.len(),
        "caption span includes the trailing newline"
    );
}
