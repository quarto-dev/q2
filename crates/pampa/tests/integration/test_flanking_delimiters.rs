//! Flanking rules for `*`, `_`, `~`, `^` (bd-star-as-str-qigl02pz,
//! bd-whitespace-flanked-delimiters-0ncy8bgq).
//!
//! A `*` / `_` run followed by whitespace cannot open emphasis and one
//! preceded by whitespace cannot close it (CommonMark §6.2 flanking); a
//! `~` / `^` only opens a sub/superscript when its closer appears before
//! the next whitespace (Pandoc's markdown rule). A delimiter that can do
//! neither is literal text, not an "unclosed" error.
//!
//! Expected inline sequences are what `pandoc -f markdown -t native`
//! produces for the same input (`-f commonmark` for the `*` closer cases
//! where Pandoc's markdown reader has its own quirk), see the plan
//! `claude-notes/plans/2026-09-25-star-as-str.md`.

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
            Inline::Emph(e) => format!("Emph[{}]", show(&e.content)),
            Inline::Strong(e) => format!("Strong[{}]", show(&e.content)),
            Inline::Subscript(e) => format!("Sub[{}]", show(&e.content)),
            Inline::Superscript(e) => format!("Sup[{}]", show(&e.content)),
            Inline::Strikeout(e) => format!("Strike[{}]", show(&e.content)),
            other => format!("Other({other:?})"),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn first_para(input: &str) -> String {
    let pandoc = read(input).unwrap_or_else(|codes| panic!("{input:?} failed to parse: {codes:?}"));
    match &pandoc.blocks[0] {
        Block::Paragraph(p) => show(&p.content),
        other => panic!("expected paragraph for {input:?}, got {other:?}"),
    }
}

#[track_caller]
fn assert_para(input: &str, expected: &str) {
    assert_eq!(first_para(input), expected, "input: {input:?}");
}

#[track_caller]
fn assert_error(input: &str, code: &str) {
    match read(input) {
        Ok(p) => panic!(
            "{input:?} parsed but should fail with {code}: {:?}",
            p.blocks
        ),
        Err(codes) => assert!(
            codes.iter().any(|c| c == code),
            "{input:?}: expected {code}, got {codes:?}"
        ),
    }
}

// --- `*`: opener side (followed by whitespace cannot open) --------------

#[test]
fn star_between_spaces_is_literal() {
    assert_para("a * b\n", "Str(a) Space Str(*) Space Str(b)");
    assert_para("a\t*\tb\n", "Str(a) Space Str(*) Space Str(b)");
}

#[test]
fn star_at_end_of_line_is_literal() {
    assert_para("foo *\n", "Str(foo) Space Str(*)");
    assert_para(
        "first *\nsecond\n",
        "Str(first) Space Str(*) SoftBreak Str(second)",
    );
    assert_para("para\n*\n", "Str(para) SoftBreak Str(*)");
}

#[test]
fn longer_star_runs_followed_by_space_are_literal() {
    assert_para("a ** b\n", "Str(a) Space Str(**) Space Str(b)");
    assert_para("foo **\n", "Str(foo) Space Str(**)");
    assert_para("a *** b\n", "Str(a) Space Str(***) Space Str(b)");
    assert_para("foo***\n", "Str(foo***)");
}

#[test]
fn star_after_punctuation_or_word_before_space_is_literal() {
    assert_para(
        "O(N * D) cost\n",
        "Str(O(N) Space Str(*) Space Str(D)) Space Str(cost)",
    );
    assert_para("Q-2-* codes\n", "Str(Q-2-*) Space Str(codes)");
    assert_para("Ts* types\n", "Str(Ts*) Space Str(types)");
    assert_para("value*\n", "Str(value*)");
}

#[test]
fn lone_star_next_to_real_emphasis() {
    assert_para("*x* * y\n", "Emph[Str(x)] Space Str(*) Space Str(y)");
    assert_para("**a * b**\n", "Strong[Str(a) Space Str(*) Space Str(b)]");
}

// --- `*`: closer side (preceded by whitespace cannot close) -------------

#[test]
fn star_preceded_by_space_does_not_close() {
    // CommonMark: the inner `*` is neither left- nor right-flanking.
    assert_para("*a * b*\n", "Emph[Str(a) Space Str(*) Space Str(b)]");
    assert_para("*a ** b*\n", "Emph[Str(a) Space Str(**) Space Str(b)]");
    assert_para(
        "a * b * c\n",
        "Str(a) Space Str(*) Space Str(b) Space Str(*) Space Str(c)",
    );
}

#[test]
fn line_start_star_opens_but_does_not_close() {
    assert_para("x\n*b* c\n", "Str(x) SoftBreak Emph[Str(b)] Space Str(c)");
}

// --- `_` ---------------------------------------------------------------

#[test]
fn underscore_flanking() {
    assert_para("a _ b\n", "Str(a) Space Str(_) Space Str(b)");
    assert_para(
        "_ a and b _\n",
        "Str(_) Space Str(a) Space Str(and) Space Str(b) Space Str(_)",
    );
    assert_para("a __ b\n", "Str(a) Space Str(__) Space Str(b)");
    assert_para("_a and b_\n", "Emph[Str(a) Space Str(and) Space Str(b)]");
}

// --- `~` and `^`: Pandoc's no-whitespace rule --------------------------

#[test]
fn tilde_without_closer_before_whitespace_is_literal() {
    assert_para("a ~ b\n", "Str(a) Space Str(~) Space Str(b)");
    assert_para(
        "a ~5 and ~10 b\n",
        "Str(a) Space Str(~5) Space Str(and) Space Str(~10) Space Str(b)",
    );
    assert_para(
        "~a and then ~b\n",
        "Str(~a) Space Str(and) Space Str(then) Space Str(~b)",
    );
    assert_para("~a and b~\n", "Str(~a) Space Str(and) Space Str(b~)");
    assert_para("a~ and ~b\n", "Str(a~) Space Str(and) Space Str(~b)");
    assert_para(
        "x ~1.5s\ny ~8x z\n",
        "Str(x) Space Str(~1.5s) SoftBreak Str(y) Space Str(~8x) Space Str(z)",
    );
}

#[test]
fn subscript_and_strikeout_still_parse() {
    assert_para("H~2~O\n", "Str(H) Sub[Str(2)] Str(O)");
    assert_para("~~gone~~ here\n", "Strike[Str(gone)] Space Str(here)");
    assert_para(
        "x ~~a b~~ y\n",
        "Str(x) Space Strike[Str(a) Space Str(b)] Space Str(y)",
    );
    // Escaped spaces are not whitespace: the closer is still "in sight",
    // and each `\ ` becomes U+00A0 as in pandoc.
    assert_para(
        "~a\\ and\\ yes~ work?\n",
        "Sub[Str(a\u{a0}and\u{a0}yes)] Space Str(work?)",
    );
    assert_para("^a\\ b^ x\n", "Sup[Str(a\u{a0}b)] Space Str(x)");
}

#[test]
fn caret_without_closer_before_whitespace_is_literal() {
    assert_para("a ^ b\n", "Str(a) Space Str(^) Space Str(b)");
    assert_para(
        "x ^2 and y ^3 z\n",
        "Str(x) Space Str(^2) Space Str(and) Space Str(y) Space Str(^3) Space Str(z)",
    );
    assert_para(
        "^a and then ^b\n",
        "Str(^a) Space Str(and) Space Str(then) Space Str(^b)",
    );
    assert_para("^a and b^\n", "Str(^a) Space Str(and) Space Str(b^)");
    assert_para("2^10 and 3^4\n", "Str(2^10) Space Str(and) Space Str(3^4)");
}

#[test]
fn superscript_still_parses() {
    assert_para("x^2^\n", "Str(x) Sup[Str(2)]");
}

// --- regressions ---------------------------------------------------------

#[test]
fn escapes_code_and_intraword_unchanged() {
    assert_para("a \\* b\n", "Str(a) Space Str(*) Space Str(b)");
    assert_para("*a* b\n", "Emph[Str(a)] Space Str(b)");
    assert_para("foo*bar*baz\n", "Str(foo) Emph[Str(bar)] Str(baz)");
}

// --- writers -------------------------------------------------------------

#[test]
fn literal_star_round_trips_through_the_qmd_writer() {
    let pandoc = read("a * b\n").expect("parse");
    let mut buf = Vec::new();
    pampa::writers::qmd::write(&pandoc, &mut buf).expect("write");
    assert_eq!(String::from_utf8(buf).unwrap().trim_end(), "a \\* b");
}

#[test]
fn sub_and_superscript_containing_a_space_round_trip_through_the_qmd_writer() {
    // A Subscript/Superscript holding a real `Space` inline can come from
    // JSON, a filter, or pandoc's commonmark_x reader. The writer must
    // escape the space (`~a\ b~`, as pandoc's markdown writer does) or the
    // reader, which now requires the closer before the next unescaped
    // whitespace, would read the result back as literal text.
    let json = r#"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[{"t":"Para","c":[{"t":"Subscript","c":[{"t":"Str","c":"a"},{"t":"Space"},{"t":"Str","c":"b"}]},{"t":"Space"},{"t":"Str","c":"x"},{"t":"Superscript","c":[{"t":"Str","c":"c"},{"t":"SoftBreak"},{"t":"Str","c":"d"}]}]}]}"#;
    let (pandoc, _ctx) = readers::json::read_completing_source_info(
        &mut json.as_bytes(),
        quarto_source_map::By::unknown(),
    )
    .expect("json");
    let mut buf = Vec::new();
    pampa::writers::qmd::write(&pandoc, &mut buf).expect("write");
    let qmd = String::from_utf8(buf).unwrap();
    assert_eq!(qmd.trim_end(), "~a\\ b~ x^c\\ d^");
    assert_para(&qmd, "Sub[Str(a\u{a0}b)] Space Str(x) Sup[Str(c\u{a0}d)]");
}

// --- tier 3 boundary: unmatched openers are still errors ------------------

#[test]
fn unmatched_left_flanking_openers_still_error() {
    assert_error("a *b c\n", "Q-2-12");
    assert_error("**bold **\n", "Q-2-13");
    assert_error("_quarto.yml and _metadata.yml\n", "Q-2-5");
}
