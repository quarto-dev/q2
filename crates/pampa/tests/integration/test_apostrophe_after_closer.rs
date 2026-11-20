//! A straight apostrophe directly after the end of an inline construct is an
//! apostrophe, not a quote mark (bd-apostrophe-after-span-6l9c8c26).
//!
//! `The _book_'s premise`, `` `a.rs`'s ``, `[book](url)'s` and
//! `_books_' premises` used to be Q-2-7 / Q-2-10 parse errors (or, with a
//! second `'` later in the paragraph, a silently wrong `Quoted`). Pandoc
//! reads each `'` as an apostrophe, and so do we now.
//!
//! The scanner cannot look behind, and several closers (the `)` of a link,
//! the `}` of an attribute, the closing `$` of math) are not external
//! tokens, so the context comes from the grammar: `_inline_element` allows
//! an optional `_span_apostrophe` external token right after a
//! `_closable_inline`, and `parse_single_quote` in
//! `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c` emits it
//! when it is valid, no single-quoted span is open, and no whitespace
//! separates it from the closer. This is the sibling of GH #943 (an
//! apostrophe after a word, `test_apostrophe_after_word.rs`).

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
            Inline::Link(l) => format!("Link[{}]", show(&l.content)),
            Inline::Span(s) => format!("Span[{}]", show(&s.content)),
            Inline::Image(_) => "Image".to_string(),
            Inline::Math(m) => format!("Math({})", m.text),
            Inline::Superscript(e) => format!("Sup[{}]", show(&e.content)),
            Inline::Subscript(e) => format!("Sub[{}]", show(&e.content)),
            Inline::Strikeout(e) => format!("Strikeout[{}]", show(&e.content)),
            Inline::Cite(_) => "Cite".to_string(),
            Inline::Note(_) => "Note".to_string(),
            Inline::RawInline(_) => "RawInline".to_string(),
            other => format!("Other({other:?})"),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

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
    let pandoc = read(input).unwrap_or_else(|codes| panic!("{input:?} failed to parse: {codes:?}"));
    assert_eq!(
        show(first_inlines(&pandoc.blocks)),
        expected,
        "input: {input:?}"
    );
}

/// The unescaped form parses to exactly what the escaped form does.
#[track_caller]
fn assert_same_as_escaped(input: &str) {
    assert!(input.contains('\''), "no apostrophe in {input:?}");
    let escaped = input.replace('\'', "\\'");
    let plain = read(input).unwrap_or_else(|codes| panic!("{input:?} failed to parse: {codes:?}"));
    let esc = read(&escaped).unwrap_or_else(|codes| panic!("{escaped:?} failed: {codes:?}"));
    let shown = show(first_inlines(&plain.blocks));
    assert_eq!(shown, show(first_inlines(&esc.blocks)), "input: {input:?}");
    assert!(shown.contains('’'), "no typographic apostrophe: {shown}");
    assert!(!shown.contains("Quoted"), "unexpected quote: {shown}");
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
fn possessive_after_emphasis_and_strong() {
    assert_inlines(
        "The _book_'s premise\n",
        "Str(The) Space Emph[Str(book)] Str(’s) Space Str(premise)",
    );
    assert_inlines("*book*'s\n", "Emph[Str(book)] Str(’s)");
    assert_inlines("**book**'s\n", "Strong[Str(book)] Str(’s)");
    assert_inlines("__book__'s\n", "Strong[Str(book)] Str(’s)");
}

#[test]
fn possessive_after_a_link_and_a_span() {
    assert_inlines(
        "The [book](https://book.com)'s premise\n",
        "Str(The) Space Link[Str(book)] Str(’s) Space Str(premise)",
    );
    assert_inlines("[book]{.x}'s\n", "Span[Str(book)] Str(’s)");
    assert_inlines("[book](u){.x}'s\n", "Link[Str(book)] Str(’s)");
    assert_inlines("![alt](a.png)'s\n", "Image Str(’s)");
}

#[test]
fn possessive_after_a_code_span() {
    assert_inlines("`a.rs`'s file\n", "Code(a.rs) Str(’s) Space Str(file)");
    assert_inlines("`a.rs`{.rust}'s\n", "Code(a.rs) Str(’s)");
}

#[test]
fn every_closer_kind_matches_the_escaped_form() {
    for input in [
        "$x$'s value\n",
        "x^2^'s value\n",
        "H~2~'s value\n",
        "~~gone~~'s value\n",
        "@knuth's book\n",
        "[@knuth]'s book\n",
        "^[a note]'s value\n",
        "[>> comment]'s value\n",
        "[++ added]'s value\n",
        "[-- removed]'s value\n",
        "[!! marked]'s value\n",
        "{{< meta title >}}'s value\n",
        "<https://example.com>'s value\n",
    ] {
        assert_same_as_escaped(input);
    }
}

#[test]
fn plural_possessive_and_bare_apostrophe_after_a_closer() {
    assert_inlines(
        "The _books_' premises\n",
        "Str(The) Space Emph[Str(books)] Str(’) Space Str(premises)",
    );
    assert_inlines("_a_'\n", "Emph[Str(a)] Str(’)");
    assert_inlines("_a_'", "Emph[Str(a)] Str(’)");
    assert_inlines("(_a_'.)\n", "Str(() Emph[Str(a)] Str(’.))");
    // Followed by a non-ASCII letter.
    assert_inlines("_l_'été\n", "Emph[Str(l)] Str(’été)");
}

#[test]
fn two_possessives_no_longer_pair_into_a_quote() {
    // Before: Strong, Quoted[s and "quote"], Str(s) with no diagnostic.
    assert_inlines(
        "**book**'s and `code`'s\n",
        "Strong[Str(book)] Str(’s) Space Str(and) Space Code(code) Str(’s)",
    );
}

#[test]
fn a_quote_abutting_a_closer_reads_as_pandoc_does() {
    // Pandoc: Emph [Str "a"], Str "’b’" and Str "’b", Space, Str "c’".
    assert_inlines("_a_'b'\n", "Emph[Str(a)] Str(’b’)");
    assert_inlines(
        "_a_'b c' d\n",
        "Emph[Str(a)] Str(’b) Space Str(c’) Space Str(d)",
    );
}

#[test]
fn a_quote_abutting_a_closer_stays_reachable() {
    // An author who does want a quoted span right after a closer puts
    // something that is not a closable inline between the two. An HTML
    // comment renders as nothing in every format; a zero-width space
    // (typed as a character reference) is the other option.
    assert_inlines("_a_<!---->'b'\n", "Emph[Str(a)] RawInline Quoted[Str(b)]");
    assert_inlines(
        "_a_&#8203;'b'\n",
        "Emph[Str(a)] Str(\u{200B}) Quoted[Str(b)]",
    );
}

#[test]
fn possessive_after_a_double_quoted_span() {
    assert_inlines(
        "\"quote\"'s value\n",
        "Quoted[Str(quote)] Str(’s) Space Str(value)",
    );
}

#[test]
fn in_every_block_context() {
    assert_inlines("- _a_'s toys.\n", "Emph[Str(a)] Str(’s) Space Str(toys.)");
    assert_inlines("## _a_'s toys\n", "Emph[Str(a)] Str(’s) Space Str(toys)");
    assert_inlines("> _a_'s toys\n", "Emph[Str(a)] Str(’s) Space Str(toys)");
    assert_inlines(
        "\"the _book_'s premise\"\n",
        "Quoted[Str(the) Space Emph[Str(book)] Str(’s) Space Str(premise)]",
    );
    assert_inlines(
        "**the _book_'s premise**\n",
        "Strong[Str(the) Space Emph[Str(book)] Str(’s) Space Str(premise)]",
    );
    let pandoc = read("| a | _b_'s |\n|---|---|\n| 1 | `c`'s |\n").expect("table parses");
    assert!(
        matches!(pandoc.blocks[0], Block::Table(_)),
        "{:?}",
        pandoc.blocks
    );
}

#[test]
fn quotes_still_pair() {
    // Whitespace (or a line break) between the closer and the mark: a quote.
    assert_inlines("_a_ 'quoted'\n", "Emph[Str(a)] Space Quoted[Str(quoted)]");
    assert_inlines(
        "_a_\n'quoted'\n",
        "Emph[Str(a)] SoftBreak Quoted[Str(quoted)]",
    );
    // Inside an open single quote, the `'` after a closer is the quote's
    // closer.
    assert_inlines("'_book_'\n", "Quoted[Emph[Str(book)]]");
    assert_inlines(
        "The '_book_'s are not good\n",
        "Str(The) Space Quoted[Emph[Str(book)]] Str(s) Space Str(are) Space Str(not) Space Str(good)",
    );
}

#[test]
fn genuine_quote_errors_still_fire() {
    assert_fails_with("_a_ 'never closed\n", "Q-2-7");
    assert_fails_with("'never closed _a_ either\n", "Q-2-7");
    assert_fails_with("a ' b.\n", "Q-2-10");
    // Known limit, shared with GH #943: inside an open single quote the
    // mark after a closer is the quote's closer, so the possessive needs
    // its escape there (pandoc reads it as an apostrophe). Pinned so a
    // change to it is deliberate.
    assert_inlines(
        "'the _book_'s premise'\n",
        "Quoted[Str(the) Space Emph[Str(book)]] Str(s) Space Str(premise’)",
    );
    assert_inlines(
        "'the _book_\\'s premise'\n",
        "Quoted[Str(the) Space Emph[Str(book)] Str(’s) Space Str(premise)]",
    );
}
