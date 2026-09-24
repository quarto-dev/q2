/*
 * test_fenced_div_sigils.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Fenced-div "sigils": the token right after `::: ` that turns a fenced div
 * opener into a different construct (`::: ^id` note definitions). See
 * `parse_fenced_div_sigil` in tree-sitter-qmd's scanner.c.
 */

use pampa::pandoc::Block;
use pampa::readers;

fn parse(input: &str) -> Vec<Block> {
    let (pandoc, _context, _warnings) = readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    )
    .unwrap_or_else(|diags| {
        panic!(
            "expected a clean parse, got: {:?}",
            diags
                .iter()
                .map(|d| (d.code.clone(), d.problem.clone()))
                .collect::<Vec<_>>()
        )
    });
    pandoc.blocks
}

fn note_definition_ids(blocks: &[Block]) -> Vec<String> {
    blocks
        .iter()
        .filter_map(|b| match b {
            Block::NoteDefinitionFencedBlock(n) => Some(n.id.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn note_definition_id_lf() {
    let blocks = parse("::: ^foo\n\nBody.\n\n:::\n");
    assert_eq!(note_definition_ids(&blocks), vec!["foo".to_string()]);
}

/// With CRLF line endings the id used to run on to the `\r`, yielding
/// `"foo\r"`, which then never matched its `[^foo]` reference.
#[test]
fn note_definition_id_crlf_excludes_carriage_return() {
    let blocks = parse("::: ^foo\r\n\r\nBody.\r\n\r\n:::\r\n");
    assert_eq!(note_definition_ids(&blocks), vec!["foo".to_string()]);
}
