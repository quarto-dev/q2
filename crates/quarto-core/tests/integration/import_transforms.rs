//! The import transforms (P3 T4, T5): track changes, highlights and comments become q2
//! editorial marks (I3, I4, I13); ordered-list styles are counted (I17); a pptx import gets
//! `format: revealjs` (I6).
//!
//! The oracle is the idea of P2's: the transformed AST, written as qmd and re-read, equals the
//! transformed AST (compared as pampa's native text). Shapes pandoc's docx reader gives that
//! the recordings lack are built here as pandoc JSON.

use quarto_core::import::read_pandoc_json;
use quarto_core::import::transforms::{TransformCounts, apply, editorial_marks};
use serde_json::{Value, json};

use super::import_support as support;

// ---------------------------------------------------------------------------
// Pandoc JSON builders
// ---------------------------------------------------------------------------

const DATE: &str = "2026-09-01T10:00:00Z";

fn doc(blocks: Vec<Value>) -> Value {
    json!({"pandoc-api-version": [1, 23, 1], "meta": {}, "blocks": blocks})
}

fn doc_meta(meta: Value, blocks: Vec<Value>) -> Value {
    json!({"pandoc-api-version": [1, 23, 1], "meta": meta, "blocks": blocks})
}

fn s(t: &str) -> Value {
    json!({"t": "Str", "c": t})
}

fn sp() -> Value {
    json!({"t": "Space"})
}

/// Words and spaces: `"two words"` → `Str Space Str`.
fn words(text: &str) -> Vec<Value> {
    let mut out = Vec::new();
    for (i, word) in text.split(' ').enumerate() {
        if i > 0 {
            out.push(sp());
        }
        if !word.is_empty() {
            out.push(s(word));
        }
    }
    out
}

fn para(inlines: Vec<Value>) -> Value {
    json!({"t": "Para", "c": inlines})
}

fn span(classes: &[&str], attrs: &[(&str, &str)], content: Vec<Value>) -> Value {
    json!({"t": "Span", "c": [["", classes, attrs], content]})
}

fn start(id: &str, author: &str, text: &str) -> Value {
    span(
        &["comment-start"],
        &[("id", id), ("author", author), ("date", DATE)],
        words(text),
    )
}

fn end(id: &str) -> Value {
    span(&["comment-end"], &[("id", id)], vec![])
}

/// pandoc's reply shape: the reply's end nested inside the parent's.
fn end_nested(id: &str, inner: Value) -> Value {
    span(&["comment-end"], &[("id", id)], vec![inner])
}

fn emph(content: Vec<Value>) -> Value {
    json!({"t": "Emph", "c": content})
}

fn cat(parts: Vec<Vec<Value>>) -> Vec<Value> {
    parts.into_iter().flatten().collect()
}

fn one(v: Value) -> Vec<Value> {
    vec![v]
}

// ---------------------------------------------------------------------------
// Running
// ---------------------------------------------------------------------------

struct Out {
    qmd: String,
    counts: TransformCounts,
}

#[track_caller]
fn run_with(input: &Value, resolve_notes: bool) -> Out {
    let pandoc = read_pandoc_json(&input.to_string()).expect("pandoc JSON reads");
    let (pandoc, counts) = apply(pandoc, "docx");
    let qmd = support::assert_qmd_roundtrips(&pandoc, resolve_notes);
    Out { qmd, counts }
}

/// For shapes whose qmd doesn't re-read to the same AST for reasons of their own (a figure's
/// caption, a definition list): the transform's output is checked, not the round trip.
fn run_no_oracle(input: &Value) -> Out {
    let pandoc = read_pandoc_json(&input.to_string()).expect("pandoc JSON reads");
    let (pandoc, counts) = apply(pandoc, "docx");
    Out {
        qmd: support::write_qmd(&pandoc),
        counts,
    }
}

#[track_caller]
fn run(input: &Value) -> Out {
    run_with(input, false)
}

fn body(qmd: &str) -> &str {
    qmd.trim_end()
}

const ANN: &str = r#"{author="Ann" date="2026-09-01T10:00:00Z"}"#;

// ---------------------------------------------------------------------------
// T4a: renames and paragraph marks
// ---------------------------------------------------------------------------

#[test]
fn insertions_and_deletions_keep_author_and_date_only() {
    let ins = span(
        &["insertion"],
        &[("id", "7"), ("author", "Ann"), ("date", DATE)],
        words("added text"),
    );
    let del = span(
        &["deletion"],
        &[("id", "8"), ("author", "Bob"), ("date", DATE)],
        words("removed"),
    );
    let out = run(&doc(vec![para(cat(vec![
        words("A"),
        vec![sp(), ins, sp(), del, sp()],
        words("end."),
    ]))]));
    assert_eq!(
        body(&out.qmd),
        format!("A [++ added text]{ANN} [-- removed]{{author=\"Bob\" date=\"{DATE}\"}} end.")
    );
    assert_eq!(out.counts, TransformCounts::default());
}

#[test]
fn a_highlight_becomes_a_quarto_highlight() {
    let mark = span(&["mark"], &[], words("important"));
    let out = run(&doc(vec![para(cat(vec![
        words("This is"),
        vec![sp(), mark, sp()],
        words("here."),
    ]))]));
    assert_eq!(body(&out.qmd), "This is [!! important] here.");
}

#[test]
fn a_highlight_keeps_its_other_attributes() {
    let mark = span(&["mark", "extra"], &[("k", "v")], words("x"));
    let pandoc = read_pandoc_json(&doc(vec![para(vec![mark])]).to_string()).unwrap();
    let (pandoc, _) = apply(pandoc, "docx");
    let native = support::native(&pandoc);
    assert!(
        native.contains("quarto-highlight") && native.contains("extra") && native.contains("\"k\""),
        "{native}"
    );
    assert!(!native.contains("\"mark\""), "{native}");
}

#[test]
fn paragraph_mark_changes_are_removed_and_counted() {
    let pi = span(
        &["paragraph-insertion"],
        &[("author", "Bob"), ("date", DATE)],
        vec![],
    );
    let pd = span(
        &["paragraph-deletion"],
        &[("author", "Bob"), ("date", DATE)],
        vec![],
    );
    let out = run(&doc(vec![
        para(cat(vec![words("first"), vec![pi]])),
        para(cat(vec![words("second"), vec![pd]])),
    ]));
    assert_eq!(body(&out.qmd), "first\n\nsecond");
    assert_eq!(out.counts.paragraph_marks, 2);
}

#[test]
fn a_tracked_change_inside_a_table_cell_and_a_note_is_renamed() {
    let ins = |t: &str| {
        span(
            &["insertion"],
            &[("author", "Ann"), ("date", DATE)],
            words(t),
        )
    };
    let cell = json!([["", [], []], {"t": "AlignDefault"}, 1, 1,
        [{"t": "Plain", "c": one(ins("in cell"))}]]);
    let row = json!([["", [], []], [cell]]);
    let table = json!({"t": "Table", "c": [
        ["", [], []], [null, []],
        [[{"t": "AlignDefault"}, {"t": "ColWidthDefault"}]],
        [["", [], []], [row.clone()]],
        [[["", [], []], 0, [], [row]]],
        [["", [], []], []]
    ]});
    let note = json!({"t": "Note", "c": [para(one(ins("in note")))]});
    let out = run(&doc(vec![table, para(cat(vec![words("T"), vec![note]]))]));
    assert!(out.qmd.contains("[++ in cell]"), "{}", out.qmd);
    assert!(out.qmd.contains("[++ in note]"), "{}", out.qmd);
}

#[test]
fn a_tracked_change_in_the_metadata_title_is_renamed() {
    let title = json!({"t": "MetaInlines", "c": cat(vec![
        words("Plain"),
        vec![sp(), span(&["insertion"], &[("author", "Ann"), ("date", DATE)], words("added"))],
    ])});
    let out = run(&doc_meta(
        json!({"title": title}),
        vec![para(words("body"))],
    ));
    assert!(out.qmd.contains("[++ added]"), "{}", out.qmd);
    assert!(out.qmd.contains("title:"), "{}", out.qmd);
}

// ---------------------------------------------------------------------------
// T4b: same-list comments
// ---------------------------------------------------------------------------

#[test]
fn a_comment_over_a_range_wraps_it_in_a_plain_span() {
    let inl = cat(vec![
        words("Some"),
        vec![sp(), start("0", "Ann", "Please reword this.")],
        words("a commented range"),
        vec![end("0"), sp()],
        words("follows."),
    ]);
    let out = run(&doc(vec![para(inl)]));
    assert_eq!(
        body(&out.qmd),
        format!("Some [a commented range[>> Please reword this.]{ANN}] follows.")
    );
    assert_eq!(out.counts, TransformCounts::default());
}

#[test]
fn an_empty_range_is_a_point_comment_with_no_wrapper() {
    let inl = cat(vec![
        words("Before"),
        vec![sp(), start("0", "Ann", "Point."), end("0"), sp()],
        words("after."),
    ]);
    let out = run(&doc(vec![para(inl)]));
    assert_eq!(body(&out.qmd), format!("Before [>> Point.]{ANN} after."));
}

#[test]
fn a_leading_space_of_the_range_is_hoisted_in_front_of_the_wrapper() {
    // `start, Space, range…`: the reader would trim a space just inside `[`.
    let inl = cat(vec![
        words("A"),
        vec![start("0", "Ann", "c"), sp()],
        words("range"),
        vec![end("0")],
    ]);
    let out = run(&doc(vec![para(inl)]));
    assert_eq!(body(&out.qmd), format!("A [range[>> c]{ANN}]"));
}

#[test]
fn nested_ranges_wrap_innermost_first() {
    let inl = cat(vec![
        vec![start("0", "Ann", "outer")],
        words("one"),
        vec![sp(), start("1", "Ann", "inner")],
        words("two"),
        vec![end("1"), sp()],
        words("three"),
        vec![end("0")],
    ]);
    let out = run(&doc(vec![para(inl)]));
    assert_eq!(
        body(&out.qmd),
        format!("[one [two[>> inner]{ANN}] three[>> outer]{ANN}]")
    );
    assert_eq!(out.counts, TransformCounts::default());
}

#[test]
fn a_reply_shares_its_parents_wrapper() {
    // start 0, start 1, range, end 0 [ end 1 ]  (P1 T3 (a))
    let inl = cat(vec![
        words("See"),
        vec![
            sp(),
            start("0", "Ann", "Parent."),
            start("1", "Dee", "Reply."),
        ],
        words("the range"),
        vec![end_nested("0", end("1"))],
        words(" here."),
    ]);
    let out = run(&doc(vec![para(inl)]));
    assert_eq!(
        body(&out.qmd),
        format!(
            "See [the range[>> Parent.]{ANN}[>> Reply.]{{author=\"Dee\" date=\"{DATE}\"}}] here."
        ),
        "{}",
        out.qmd
    );
    assert_eq!(out.counts, TransformCounts::default());
}

#[test]
fn two_replies_follow_in_start_order() {
    let inl = cat(vec![
        vec![
            start("0", "Ann", "P."),
            start("1", "Dee", "R1."),
            start("2", "Eve", "R2."),
        ],
        words("range"),
        vec![end_nested("0", end_nested("1", end("2")))],
    ]);
    let out = run(&doc(vec![para(inl)]));
    assert!(
        out.qmd.starts_with("[range[>> P.]")
            && out.qmd.find("R1.").unwrap() < out.qmd.find("R2.").unwrap(),
        "{}",
        out.qmd
    );
    assert_eq!(out.qmd.matches("[>>").count(), 3);
    assert_eq!(out.counts, TransformCounts::default());
}

#[test]
fn a_wrapper_followed_by_a_parenthesis_is_escaped_by_the_writer() {
    // P2 T1: `[…](see)` would re-read as a link.
    let inl = cat(vec![
        vec![start("0", "Ann", "c")],
        words("range"),
        vec![end("0"), s("(see)")],
    ]);
    let out = run(&doc(vec![para(inl)]));
    assert!(out.qmd.contains("(see)"), "{}", out.qmd);
}

#[test]
fn a_comment_in_a_table_cell_wraps_there() {
    let inl = cat(vec![
        vec![start("0", "Ann", "cell note")],
        words("cell text"),
        vec![end("0")],
    ]);
    let cell = json!([["", [], []], {"t": "AlignDefault"}, 1, 1,
        [{"t": "Plain", "c": inl}]]);
    let row = json!([["", [], []], [cell]]);
    let table = json!({"t": "Table", "c": [
        ["", [], []], [null, []],
        [[{"t": "AlignDefault"}, {"t": "ColWidthDefault"}]],
        [["", [], []], [row.clone()]],
        [[["", [], []], 0, [], [row]]],
        [["", [], []], []]
    ]});
    let out = run(&doc(vec![table]));
    assert!(out.qmd.contains("[cell text[>> cell note]"), "{}", out.qmd);
}

#[test]
fn a_comment_in_a_footnote_wraps_inside_the_note() {
    let note_inl = cat(vec![
        vec![start("0", "Ann", "note comment")],
        words("noted"),
        vec![end("0")],
    ]);
    let note = json!({"t": "Note", "c": [para(note_inl)]});
    let out = run_with(
        &doc(vec![para(cat(vec![
            words("Text"),
            vec![note],
            words(" more."),
        ]))]),
        true,
    );
    assert!(out.qmd.contains("[noted[>> note comment]"), "{}", out.qmd);
}

// ---------------------------------------------------------------------------
// T4c: the I13 fallback, orphans, metadata
// ---------------------------------------------------------------------------

#[test]
fn a_range_crossing_paragraphs_attaches_to_the_starting_paragraph() {
    let p1 = para(cat(vec![
        words("First"),
        vec![sp(), start("0", "Ann", "Across.")],
        words("tail"),
    ]));
    let p2 = para(cat(vec![
        words("head"),
        vec![end("0"), sp()],
        words("done."),
    ]));
    let out = run(&doc(vec![p1, p2]));
    assert_eq!(
        body(&out.qmd),
        format!("First tail[>> Across.]{ANN}\n\nhead  done.").replace("head  done.", "head done."),
    );
    assert_eq!(out.counts.block_comments, 1);
}

#[test]
fn a_range_starting_inside_emphasis_attaches_to_the_paragraph() {
    // P1 T3 (e): the Emph is split around the start span.
    let inl = cat(vec![
        vec![emph(cat(vec![
            words("emphasised"),
            vec![sp(), start("0", "Ann", "In emph.")],
            words("words"),
        ]))],
        words(" end"),
        vec![end("0")],
    ]);
    let out = run(&doc(vec![para(inl)]));
    assert!(
        out.qmd.contains(&format!("[>> In emph.]{ANN}")),
        "{}",
        out.qmd
    );
    assert!(!out.qmd.contains("comment-start"), "{}", out.qmd);
    assert_eq!(out.counts.block_comments, 1);
}

#[test]
fn overlapping_ranges_wrap_the_first_and_attach_the_second() {
    // start 5 … start 6 … end 5 … end 6 (P1 T3 (f))
    let inl = cat(vec![
        vec![start("5", "Ann", "First.")],
        words("aa"),
        vec![sp(), start("6", "Dee", "Second.")],
        words("bb"),
        vec![end("5"), sp()],
        words("cc"),
        vec![end("6")],
    ]);
    let out = run(&doc(vec![para(inl)]));
    assert!(out.qmd.contains("[aa"), "{}", out.qmd);
    assert!(
        out.qmd.contains("[>> First.]") && out.qmd.contains("[>> Second.]"),
        "{}",
        out.qmd
    );
    assert_eq!(out.counts.block_comments, 1, "{}", out.qmd);
    // The second is a trailing comment of the paragraph, after the wrapper.
    assert!(
        out.qmd
            .trim_end()
            .ends_with(&format!("[>> Second.]{{author=\"Dee\" date=\"{DATE}\"}}")),
        "{}",
        out.qmd
    );
}

#[test]
fn a_comment_in_a_definition_term_gets_a_new_paragraph_after_the_list() {
    let term = cat(vec![
        vec![start("0", "Ann", "On the term.")],
        words("Term"),
        // the end sits in a different block, so the pair can't wrap
    ]);
    let dl =
        json!({"t": "DefinitionList", "c": [[term, [[{"t": "Para", "c": words("Definition")}]]]]});
    let p = para(cat(vec![words("later"), vec![end("0")]]));
    let out = run_no_oracle(&doc(vec![dl, p]));
    assert!(
        out.qmd.contains(&format!("[>> On the term.]{ANN}")),
        "{}",
        out.qmd
    );
    assert!(!out.qmd.contains("comment-start"), "{}", out.qmd);
    assert_eq!(out.counts.block_comments, 1);
    // The comment is in its own paragraph, right after the list, not inside it.
    let dl_end = out.qmd.find("Definition").unwrap();
    let c = out.qmd.find("[>> On the term.]").unwrap();
    assert!(c > dl_end, "{}", out.qmd);
}

#[test]
fn a_comment_in_a_figure_caption_attaches_inside_the_caption() {
    let cap_inl = cat(vec![
        vec![start("0", "Ann", "On caption.")],
        words("Caption text"),
        vec![end("0")],
    ]);
    let fig = json!({"t": "Figure", "c": [
        ["", [], []],
        [null, [{"t": "Plain", "c": cap_inl}]],
        [{"t": "Plain", "c": [{"t": "Image", "c": [["", [], []], words("alt"), ["pic.png", ""]]}]}]
    ]});
    let out = run_no_oracle(&doc(vec![fig]));
    assert!(out.qmd.contains("[>> On caption.]"), "{}", out.qmd);
    assert!(!out.qmd.contains("comment-start"), "{}", out.qmd);
}

#[test]
fn a_comment_in_the_metadata_is_dropped_and_counted() {
    let title = json!({"t": "MetaInlines", "c": cat(vec![
        vec![start("0", "Ann", "On the title.")],
        words("Title"),
        vec![end("0")],
    ])});
    let out = run(&doc_meta(
        json!({"title": title}),
        vec![para(words("body"))],
    ));
    assert!(!out.qmd.contains("On the title"), "{}", out.qmd);
    assert!(
        out.qmd.contains("title: Title") || out.qmd.contains("title: \"Title\""),
        "{}",
        out.qmd
    );
    assert_eq!(out.counts.block_comments, 1);
}

#[test]
fn an_unmatched_start_becomes_a_point_comment_and_an_unmatched_end_vanishes() {
    let inl = cat(vec![
        words("one"),
        vec![sp(), start("0", "Ann", "Lonely."), sp()],
        words("two"),
        vec![sp(), end("9"), sp()],
        words("three"),
    ]);
    let out = run(&doc(vec![para(inl)]));
    assert!(
        out.qmd.contains(&format!("[>> Lonely.]{ANN}")),
        "{}",
        out.qmd
    );
    assert!(!out.qmd.contains("comment-"), "{}", out.qmd);
    assert_eq!(out.counts.unmatched_markers, 2);
    assert_eq!(out.counts.block_comments, 0);
}

#[test]
fn a_comment_end_nested_in_another_ends_content_is_never_lost() {
    // The reply's end is only reachable through the parent's end; its start is elsewhere.
    let p1 = para(cat(vec![vec![start("1", "Dee", "Far reply.")], words("x")]));
    let p2 = para(vec![end_nested("0", end("1"))]);
    let out = run(&doc(vec![p1, p2]));
    assert!(out.qmd.contains("Far reply."), "{}", out.qmd);
    assert!(!out.qmd.contains("comment-"), "{}", out.qmd);
    // Start 1 pairs with the nested end across paragraphs (I13); end 0 has no start.
    assert_eq!(out.counts.block_comments, 1);
    assert_eq!(out.counts.unmatched_markers, 1);
}

#[test]
fn the_transform_is_deterministic() {
    let inl = cat(vec![
        vec![start("0", "Ann", "a"), start("1", "Dee", "b")],
        words("r"),
        vec![end_nested("0", end("1"))],
    ]);
    let input = doc(vec![para(inl)]);
    let a = run(&input).qmd;
    let b = run(&input).qmd;
    assert_eq!(a, b);
}

// ---------------------------------------------------------------------------
// T5: other transforms
// ---------------------------------------------------------------------------

#[test]
fn pptx_gets_format_revealjs_unless_set() {
    let pandoc = read_pandoc_json(&support::pandoc_json("basic-pptx")).unwrap();
    let (pandoc, _) = apply(pandoc, "pptx");
    let qmd = support::write_qmd(&pandoc);
    assert!(
        qmd.starts_with("---\n") && qmd.contains("format: revealjs"),
        "{qmd}"
    );

    // Already set: untouched.
    let with_format = doc_meta(
        json!({"format": {"t": "MetaString", "c": "html"}}),
        vec![para(words("x"))],
    );
    let pandoc = read_pandoc_json(&with_format.to_string()).unwrap();
    let (pandoc, _) = apply(pandoc, "pptx");
    let qmd = support::write_qmd(&pandoc);
    assert!(
        qmd.contains("format: html") && !qmd.contains("revealjs"),
        "{qmd}"
    );

    // Other formats never get it.
    let pandoc = read_pandoc_json(&support::pandoc_json("basic-docx")).unwrap();
    let (pandoc, _) = apply(pandoc, "docx");
    assert!(!support::write_qmd(&pandoc).contains("revealjs"));
}

#[test]
fn list_styles_that_the_qmd_cannot_keep_are_counted() {
    let pandoc = read_pandoc_json(&support::pandoc_json("writer-bugs-docx")).unwrap();
    let (_, counts) = apply(pandoc, "docx");
    // The fixture has an alpha list and a roman list; its decimal lists are fine.
    assert_eq!(counts.list_styles, 2);

    let list = |style: &str, delim: &str| {
        json!({"t": "OrderedList", "c": [[3, {"t": style}, {"t": delim}],
            [[{"t": "Plain", "c": words("x")}]]]})
    };
    let cases = [
        ("Decimal", "Period", 0),
        ("Decimal", "OneParen", 0),
        ("DefaultStyle", "DefaultDelim", 0),
        ("Decimal", "TwoParens", 1),
        ("UpperAlpha", "Period", 1),
        ("LowerRoman", "OneParen", 1),
    ];
    for (style, delim, want) in cases {
        let pandoc = read_pandoc_json(&doc(vec![list(style, delim)]).to_string()).unwrap();
        let (_, counts) = apply(pandoc, "docx");
        assert_eq!(counts.list_styles, want, "{style} {delim}");
    }
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn fixture_qmd(name: &str, format: &str) -> (String, TransformCounts) {
    let pandoc = read_pandoc_json(&support::pandoc_json(name)).unwrap();
    let (pandoc, counts) = apply(pandoc, format);
    (support::write_qmd(&pandoc), counts)
}

#[test]
fn track_changes_docx_has_marks_with_author_and_date() {
    let (qmd, counts) = fixture_qmd("track-changes-docx", "docx");
    assert!(qmd.contains("[++ ") && qmd.contains("[-- "), "{qmd}");
    assert!(
        qmd.contains("author=\"") && qmd.contains("date=\""),
        "{qmd}"
    );
    assert!(
        !qmd.contains("insertion") && !qmd.contains("deletion"),
        "{qmd}"
    );
    assert!(
        !qmd.contains("comment-start") && !qmd.contains("comment-end"),
        "{qmd}"
    );
    assert_eq!(counts.paragraph_marks, 2);
    assert_eq!(counts.unmatched_markers, 0);
}

#[test]
fn highlights_docx_has_highlight_marks() {
    let (qmd, _) = fixture_qmd("highlights-docx", "docx");
    assert!(qmd.contains("[!! "), "{qmd}");
    assert!(!qmd.contains("{.mark"), "{qmd}");
}

#[test]
fn comments_edge_docx_roundtrips_and_reports() {
    let pandoc = read_pandoc_json(&support::pandoc_json("comments-edge-docx")).unwrap();
    let (pandoc, counts) = apply(pandoc, "docx");
    // The multi-paragraph comment text is joined by line breaks inside the span: it must
    // survive the qmd round trip too.
    let qmd = support::assert_qmd_roundtrips(&pandoc, true);
    assert!(
        !qmd.contains("comment-start") && !qmd.contains("comment-end"),
        "{qmd}"
    );
    assert_eq!(counts.unmatched_markers, 0, "{qmd}");
}

#[test]
fn every_fixture_transforms_and_its_marks_roundtrip() {
    for name in support::FIXTURES.iter().filter(|n| **n != "corrupt-docx") {
        let format = name.rsplit('-').next().unwrap();
        let pandoc = read_pandoc_json(&support::pandoc_json(name)).unwrap();
        let (mut pandoc, _) = apply(pandoc, format);
        // `editorial_marks` is idempotent: nothing is left for a second run.
        let mut again = TransformCounts::default();
        editorial_marks(&mut pandoc, &mut again);
        assert_eq!(again, TransformCounts::default(), "{name}");
    }
}
