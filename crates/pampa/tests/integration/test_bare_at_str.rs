//! Bare `@` that cannot be a citation typo is literal text
//! (bd-bare-at-literal-w3ytmu8e).
//!
//! A standalone `@` (whitespace or line start before it; whitespace, end of
//! line, end of file or a quote after it) and an `@` inside or at the end of
//! a word are literal text. Before, the first failed the document with an
//! uncoded parse error and `word@word` silently became `Str "word"` plus a
//! bogus `Cite`. An `@` next to citation-like punctuation (`@-foo`, `@,`,
//! `(see @)`, `-@`, `@@`) keeps its error on purpose: it is more likely a
//! mistyped citation than prose.
//!
//! Expected inline sequences are what `pandoc -f markdown -t native`
//! produces for the same input; see the plan
//! `claude-notes/plans/2026-09-25-bare-at-as-str.md`.

use pampa::pandoc::{Block, CitationMode, Inline};
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
            Inline::Strong(e) => format!("Strong[{}]", show(&e.content)),
            Inline::Quoted(q) => format!("Quoted[{}]", show(&q.content)),
            Inline::Cite(c) => {
                let mode = match c.citations[0].mode {
                    CitationMode::AuthorInText => "text",
                    CitationMode::SuppressAuthor => "suppress",
                    CitationMode::NormalCitation => "normal",
                };
                format!("Cite({} {mode})", c.citations[0].id)
            }
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
        Block::OrderedList(l) => first_inlines(&l.content[0]),
        other => panic!("no inline content in {other:?}"),
    }
}

fn parse_ok(input: &str) -> pampa::pandoc::Pandoc {
    read(input).unwrap_or_else(|codes| panic!("{input:?} failed to parse: {codes:?}"))
}

#[track_caller]
fn assert_inlines(input: &str, expected: &str) {
    let pandoc = parse_ok(input);
    assert_eq!(
        show(first_inlines(&pandoc.blocks)),
        expected,
        "input: {input:?}"
    );
}

#[track_caller]
fn assert_fails(input: &str) {
    if let Ok(p) = read(input) {
        panic!("{input:?} parsed but should keep its error: {:?}", p.blocks);
    }
}

// --- standalone `@` --------------------------------------------------------

#[test]
fn standalone_at_is_literal() {
    assert_inlines("a @ b\n", "Str(a) Space Str(@) Space Str(b)");
    assert_inlines("@\n", "Str(@)");
    assert_inlines("@ b\n", "Str(@) Space Str(b)");
    assert_inlines("a @\n", "Str(a) Space Str(@)");
    assert_inlines("a @", "Str(a) Space Str(@)");
    assert_inlines("a\t@\tb\n", "Str(a) Space Str(@) Space Str(b)");
    assert_inlines("a  @  b\n", "Str(a) Space Str(@) Space Str(b)");
    assert_inlines(
        "x @ y @ z\n",
        "Str(x) Space Str(@) Space Str(y) Space Str(@) Space Str(z)",
    );
    assert_inlines("Q&A @ 3pm\n", "Str(Q&A) Space Str(@) Space Str(3pm)");
}

#[test]
fn standalone_at_at_line_boundaries() {
    assert_inlines(
        "first @\nsecond\n",
        "Str(first) Space Str(@) SoftBreak Str(second)",
    );
    assert_inlines(
        "text\n@ more\n",
        "Str(text) SoftBreak Str(@) Space Str(more)",
    );
    assert_inlines("para\n@\n", "Str(para) SoftBreak Str(@)");
}

#[test]
fn plan_header_idiom() {
    assert_inlines(
        "**Branch:** `main` @ `6bee9ebe` (investigation)\n",
        "Strong[Str(Branch:)] Space Code(main) Space Str(@) Space Code(6bee9ebe) \
         Space Str((investigation))",
    );
    assert_inlines(
        "on `main` @\n`6bee9ebe`\n",
        "Str(on) Space Code(main) Space Str(@) SoftBreak Code(6bee9ebe)",
    );
}

#[test]
fn standalone_at_before_a_quote() {
    assert_inlines(
        "a @\"quoted\" b\n",
        "Str(a) Space Str(@) Quoted[Str(quoted)] Space Str(b)",
    );
    assert_inlines(
        "a @'x' b\n",
        "Str(a) Space Str(@) Quoted[Str(x)] Space Str(b)",
    );
}

/// A container prefix or continuation indent swallows the whitespace before
/// the first inline token of a line; that position still counts as line
/// start (the scanner's line-content anchor).
#[test]
fn at_first_on_a_line_after_a_container_prefix() {
    let expected = "Str(@) Space Str(b)";
    assert_inlines("> @ b\n", expected);
    assert_inlines("> > @ b\n", expected);
    // No space after the marker: content starts right at the `@`.
    assert_inlines(">@ b\n", expected);
    assert_inlines("- @ b\n", expected);
    assert_inlines("1. @ b\n", expected);
    assert_inlines("- > @ b\n", expected);
    assert_inlines("# @ b\n", expected);
    assert_inlines("> a\n> @ b\n", "Str(a) SoftBreak Str(@) Space Str(b)");
    assert_inlines("- a\n  @ b\n", "Str(a) SoftBreak Str(@) Space Str(b)");
    assert_inlines(
        "text\n  @ more\n",
        "Str(text) SoftBreak Str(@) Space Str(more)",
    );
}

/// A literal-Str token at the start of a heading folds the space after the
/// `#` into its range; the heading must not start with the resulting Space.
/// Pre-existing for `<` (bd-j9cf) and `*` (bd-star-as-str-qigl02pz).
#[test]
fn heading_starting_with_a_literal_str_has_no_leading_space() {
    assert_inlines("# @ b\n", "Str(@) Space Str(b)");
    assert_inlines("# < b\n", "Str(<) Space Str(b)");
    assert_inlines("# * b\n", "Str(*) Space Str(b)");
    assert_inlines("## @\n", "Str(@)");
}

#[test]
fn line_content_anchor_does_not_leak_past_a_token() {
    // The `@` is one character after the content start, glued to `(`.
    assert_fails("> (@ b\n");
    assert_fails("- (@ b\n");
    assert_fails("text\n  (@ b\n");
    assert_fails(">(@ b\n");
}

// --- `@` inside or at the end of a word ------------------------------------

#[test]
fn word_internal_at_is_part_of_the_word() {
    assert_inlines("user@example.com\n", "Str(user@example.com)");
    assert_inlines("foo@bar\n", "Str(foo@bar)");
    assert_inlines(
        "with mermaid@11 and std@0.224.0\n",
        "Str(with) Space Str(mermaid@11) Space Str(and) Space Str(std@0.224.0)",
    );
    assert_inlines("café@x\n", "Str(café@x)");
}

#[test]
fn word_final_at_is_part_of_the_word() {
    assert_inlines("a@ b\n", "Str(a@) Space Str(b)");
    assert_inlines("a@\n", "Str(a@)");
    assert_inlines(
        "a@. and b@, c\n",
        "Str(a@.) Space Str(and) Space Str(b@,) Space Str(c)",
    );
}

#[test]
fn word_glued_braced_key_is_a_brace_error() {
    // Was `Str "Smith"` + a braced Cite; now `Smith@` is a word and the
    // brace gets the ordinary bare-brace diagnostic.
    match read("see Smith@{key} here\n") {
        Ok(p) => panic!("parsed: {:?}", p.blocks),
        Err(codes) => assert!(codes.iter().any(|c| c == "Q-2-41"), "got {codes:?}"),
    }
}

// --- citations are unchanged -------------------------------------------------

#[test]
fn citations_still_parse() {
    assert_inlines(
        "see @ref for details\n",
        "Str(see) Space Cite(ref text) Space Str(for) Space Str(details)",
    );
    assert_inlines("@123\n", "Cite(123 text)");
    assert_inlines("@_foo\n", "Cite(_foo text)");
    assert_inlines(
        "@types/node is great\n",
        "Cite(types/node text) Space Str(is) Space Str(great)",
    );
    assert_inlines("@foo. x\n", "Cite(foo text) Str(.) Space Str(x)");
    assert_inlines(
        "see (@foo) here\n",
        "Str(see) Space Str(() Cite(foo text) Str()) Space Str(here)",
    );
    assert_inlines("Hi(@cite)\n", "Str(Hi() Cite(cite text) Str())");
    assert_inlines(
        "a \"@foo\" b\n",
        "Str(a) Space Quoted[Cite(foo text)] Space Str(b)",
    );
    assert_inlines(
        "a -@ref b\n",
        "Str(a) Space Cite(ref suppress) Space Str(b)",
    );
    assert_inlines(
        "a @{https://example.com} b\n",
        "Str(a) Space Cite(https://example.com text) Space Str(b)",
    );
    assert_inlines("\\@ b\n", "Str(@) Space Str(b)");
}

// --- likely citation typos keep their error ----------------------------------

/// These report an uncoded parse error today; bd-2o8rq2xj gives them a
/// Q-code and a `\@` hint, and should tighten these assertions to it.
#[test]
fn citation_typos_keep_their_error() {
    for input in [
        "@-foo\n",
        "a @- b\n",
        "@:foo\n",
        "@~ x\n",
        "@< x\n",
        "a @, b\n",
        "a @. b\n",
        "(see @)\n",
        "[see @]\n",
        "a (@) b\n",
        "a -@ b\n",
        "-@\n",
        "a @@ b\n",
        "@@\n",
        "@@foo\n",
        "a @} b\n",
        "@{foo\n",
        "@{weird key}\n",
        "@{}\n",
        "x ~@ y\n",
        "(@ b\n",
        "**@**\n",
        "@é\n",
    ] {
        assert_fails(input);
    }
}

// --- source ranges and the qmd writer ----------------------------------------

#[track_caller]
fn assert_str_range(input: &str, text: &str, start: usize, end: usize) {
    let pandoc = parse_ok(input);
    let s = first_inlines(&pandoc.blocks)
        .iter()
        .find_map(|i| match i {
            Inline::Str(s) if s.text == text => Some(s),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no Str({text}) in {input:?}"));
    assert_eq!(
        (s.source_info.start_offset(), s.source_info.end_offset()),
        (start, end),
        "range of Str({text}) in {input:?}"
    );
}

#[test]
fn literal_at_source_ranges_exclude_the_whitespace_before_it() {
    // The scanner folds the whitespace before the `@` into its token; the
    // reader splits it back out into a Space, so the Str covers just `@`.
    assert_str_range("a @ b\n", "@", 2, 3);
    assert_str_range("first @\nsecond\n", "@", 6, 7);
    assert_str_range("> a\n> @ b\n", "@", 6, 7);
    assert_str_range("text\n  @ more\n", "@", 7, 8);
    assert_str_range("user@example.com\n", "user@example.com", 0, 16);
}

#[test]
fn literal_at_round_trips_through_the_qmd_writer() {
    for (input, written) in [
        ("a @ b\n", "a \\@ b"),
        ("user@example.com\n", "user\\@example.com"),
        ("a@ b\n", "a\\@ b"),
    ] {
        let pandoc = parse_ok(input);
        let mut buf = Vec::new();
        pampa::writers::qmd::write(&pandoc, &mut buf).expect("write");
        let qmd = String::from_utf8(buf).unwrap();
        assert_eq!(qmd.trim_end(), written, "writing {input:?}");
        assert_eq!(
            show(first_inlines(&parse_ok(&qmd).blocks)),
            show(first_inlines(&pandoc.blocks)),
            "re-reading {qmd:?}"
        );
    }
}
