//! A straight apostrophe directly after a word is an apostrophe, not a
//! quote mark (GH #943).
//!
//! Pandoc never opens a single quote right after a string and closes one
//! only inside an open single-quoted span, so a plural possessive
//! (`Dogs' toys`), a possessive before punctuation (`the cats'.`), an
//! apostrophe before a code span (`d'`Arrow``, GH #221) and a word-final
//! apostrophe at the end of a block are all `Str`s ending in U+2019 after
//! smart typography. Before, each was a Q-2-7 or Q-2-10 parse error.
//!
//! The decision is made by the tree-sitter external scanner from the start
//! of the word (it cannot look behind): see `scan_word_then_apostrophe` in
//! `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c`. A `'` with
//! whitespace before it, after a closing delimiter, or after a word the
//! scanner does not recognise (digit-initial, non-ASCII) keeps its old
//! reading, so those cases still error; they are pinned here on purpose.

use pampa::pandoc::{Block, Inline};
use pampa::readers;

fn read(input: &str) -> Result<pampa::pandoc::Pandoc, Vec<String>> {
    match readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    ) {
        Ok((pandoc, _context, _warnings)) => Ok(pandoc),
        Err(diags) => Err(diags
            .iter()
            .map(|d| d.code.clone().unwrap_or_else(|| "uncoded".to_string()))
            .collect()),
    }
}

fn show(inlines: &[Inline]) -> String {
    inlines
        .iter()
        .map(|i| match i {
            Inline::Str(s) => format!("Str({})", s.text),
            Inline::Space(_) => "Space".to_string(),
            Inline::SoftBreak(_) => "SoftBreak".to_string(),
            Inline::Code(c) => format!("Code({})", c.text),
            Inline::Emph(e) => format!("Emph[{}]", show(&e.content)),
            Inline::Strong(e) => format!("Strong[{}]", show(&e.content)),
            Inline::Quoted(q) => format!("Quoted[{}]", show(&q.content)),
            other => format!("Other({other:?})"),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The inlines of the first paragraph-like block, descending into block
/// quotes and the first item of a list.
fn first_inlines(blocks: &[Block]) -> &[Inline] {
    match &blocks[0] {
        Block::Paragraph(p) => &p.content,
        Block::Plain(p) => &p.content,
        Block::Header(h) => &h.content,
        Block::BlockQuote(q) => first_inlines(&q.content),
        Block::BulletList(l) => first_inlines(&l.content[0]),
        other => panic!("no inline content in {other:?}"),
    }
}

#[track_caller]
fn assert_inlines(input: &str, expected: &str) {
    let pandoc =
        read(input).unwrap_or_else(|codes| panic!("{input:?} failed to parse: {codes:?}"));
    assert_eq!(
        show(first_inlines(&pandoc.blocks)),
        expected,
        "input: {input:?}"
    );
}

#[track_caller]
fn assert_fails_with(input: &str, code: &str) {
    match read(input) {
        Ok(p) => panic!("{input:?} parsed but should keep its error: {:?}", p.blocks),
        Err(codes) => assert!(
            codes.iter().any(|c| c == code),
            "{input:?} should report {code}, got {codes:?}"
        ),
    }
}

#[test]
fn plural_possessive_is_an_apostrophe() {
    // The first word of a paragraph (where a pipe table could also start)
    // and a word in the middle of one.
    assert_inlines(
        "Dogs' toys and cats' toys.\n",
        "Str(Dogs’) Space Str(toys) Space Str(and) Space Str(cats’) Space Str(toys.)",
    );
    assert_inlines("It's the cats' toys.\n", "Str(It’s) Space Str(the) Space Str(cats’) Space Str(toys.)");
}

#[test]
fn apostrophe_at_end_of_block_and_before_punctuation() {
    assert_inlines("Cats'\n", "Str(Cats’)");
    assert_inlines("Cats'", "Str(Cats’)");
    assert_inlines("the cats'. Yes\n", "Str(the) Space Str(cats’.) Space Str(Yes)");
    assert_inlines("(the cats' toys)\n", "Str((the) Space Str(cats’) Space Str(toys))");
}

#[test]
fn apostrophe_before_a_code_span() {
    assert_inlines("d'`Arrow`\n", "Str(d’) Code(Arrow)");
}

#[test]
fn apostrophe_in_every_block_context() {
    assert_inlines("- Dogs' toys.\n- Cats' toys.\n", "Str(Dogs’) Space Str(toys.)");
    assert_inlines("## Dogs' toys\n", "Str(Dogs’) Space Str(toys)");
    assert_inlines("> Dogs' toys\n", "Str(Dogs’) Space Str(toys)");
    assert_inlines(
        "first line\nDogs' toys\n",
        "Str(first) Space Str(line) SoftBreak Str(Dogs’) Space Str(toys)",
    );
    assert_inlines(
        "\"Dogs' toys\"\n",
        "Quoted[Str(Dogs’) Space Str(toys)]",
    );
    assert_inlines(
        "*the dogs' toys* are **cats' toys**\n",
        "Emph[Str(the) Space Str(dogs’) Space Str(toys)] Space Str(are) Space Strong[Str(cats’) Space Str(toys)]",
    );
}

#[test]
fn apostrophe_in_a_pipe_table_header_cell() {
    let pandoc = read("Dogs' toys | count\n---|---\ncats' | 2\n").expect("table parses");
    assert!(
        matches!(pandoc.blocks[0], Block::Table(_)),
        "a header cell starting with a possessive is still a table: {:?}",
        pandoc.blocks
    );
}

#[test]
fn quotes_still_pair() {
    assert_inlines("'Hello world'\n", "Quoted[Str(Hello) Space Str(world)]");
    assert_inlines("These are 'quotes'.\n", "Str(These) Space Str(are) Space Quoted[Str(quotes)] Str(.)");
    // Inside an open single quote, the `'` after a word is the closer.
    assert_inlines("'Dogs' toys'\n", "Quoted[Str(Dogs)] Space Str(toys’)");
}

#[test]
fn quote_marks_not_after_a_word_keep_their_errors() {
    // Whitespace before the mark: still a quote that never closes.
    assert_fails_with("a ' b.\n", "Q-2-10");
    // After a closing delimiter (GH #221's other half): still an opener.
    assert_fails_with("`a.rs`'s file\n", "Q-2-7");
    // Known limit: the scanner counts open single quotes but not the
    // constructs nested inside them, so a possessive inside an emphasis
    // inside a quote is still read as the quote's closer (pandoc reads it
    // as an apostrophe). Unchanged from before GH #943.
    assert_fails_with("'the *dogs' toys* are'\n", "Q-2-10");
}
