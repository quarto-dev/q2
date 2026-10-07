//! qmd writer fixes for pandoc-shaped ASTs (document import P2).
//!
//! Pandoc's readers produce AST shapes the qmd reader never does, so the
//! existing qmd → json → qmd round-trip suite can't see the writer bugs they
//! hit. Every test here follows the same oracle: pandoc-shaped JSON → pampa's
//! lenient JSON reader → `qmd::write` → the qmd reader → JSON, and the AST must
//! equal the input after the reader's normal desugaring, compared without
//! source info.

use pampa::pandoc::{ASTContext, Pandoc};
use pampa::{readers, writers};
use serde_json::{Value, json};

// ---------------------------------------------------------------------------
// Builders for pandoc-shaped JSON
// ---------------------------------------------------------------------------

pub fn doc(blocks: Vec<Value>) -> Value {
    json!({"pandoc-api-version": [1, 23, 1], "meta": {}, "blocks": blocks})
}

pub fn str_(t: &str) -> Value {
    json!({"t": "Str", "c": t})
}

pub fn space() -> Value {
    json!({"t": "Space"})
}

pub fn soft_break() -> Value {
    json!({"t": "SoftBreak"})
}

pub fn line_break() -> Value {
    json!({"t": "LineBreak"})
}

/// Inlines from a compact text: words become `Str`, a space `Space`, `\n`
/// `SoftBreak` and `\\n` (backslash then newline) `LineBreak`.
pub fn inl(text: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut Vec<Value>| {
        if !word.is_empty() {
            out.push(str_(word));
            word.clear();
        }
    };
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            ' ' => {
                flush(&mut word, &mut out);
                out.push(space());
            }
            '\n' => {
                flush(&mut word, &mut out);
                out.push(soft_break());
            }
            '\\' if chars.get(i + 1) == Some(&'\n') => {
                flush(&mut word, &mut out);
                out.push(line_break());
                i += 1;
            }
            c => word.push(c),
        }
        i += 1;
    }
    flush(&mut word, &mut out);
    out
}

pub fn para(inlines: Vec<Value>) -> Value {
    json!({"t": "Para", "c": inlines})
}

pub fn plain(inlines: Vec<Value>) -> Value {
    json!({"t": "Plain", "c": inlines})
}

pub fn para_text(text: &str) -> Value {
    para(inl(text))
}

pub fn bullet_list(items: Vec<Vec<Value>>) -> Value {
    json!({"t": "BulletList", "c": items})
}

pub fn ordered_list(start: i64, items: Vec<Vec<Value>>) -> Value {
    json!({"t": "OrderedList", "c": [[start, {"t": "Decimal"}, {"t": "Period"}], items]})
}

pub fn block_quote(blocks: Vec<Value>) -> Value {
    json!({"t": "BlockQuote", "c": blocks})
}

pub fn emph(inlines: Vec<Value>) -> Value {
    json!({"t": "Emph", "c": inlines})
}

pub fn strong(inlines: Vec<Value>) -> Value {
    json!({"t": "Strong", "c": inlines})
}

pub fn attr(id: &str, classes: &[&str], kvs: &[(&str, &str)]) -> Value {
    json!([id, classes, kvs])
}

pub fn span(a: Value, inlines: Vec<Value>) -> Value {
    json!({"t": "Span", "c": [a, inlines]})
}

pub fn note(blocks: Vec<Value>) -> Value {
    json!({"t": "Note", "c": blocks})
}

pub fn code_block(text: &str) -> Value {
    json!({"t": "CodeBlock", "c": [attr("", &[], &[]), text]})
}

// ---------------------------------------------------------------------------
// The oracle
// ---------------------------------------------------------------------------

fn read_pandoc_json(value: &Value) -> (Pandoc, ASTContext) {
    readers::json::read_completing_source_info(
        &mut value.to_string().as_bytes(),
        quarto_source_map::By::unknown(),
    )
    .expect("pandoc JSON should read")
}

/// The AST as JSON with every source-info field removed.
fn ast_json(pandoc: &Pandoc, context: &ASTContext) -> Value {
    let mut buf = Vec::new();
    writers::json::write(pandoc, context, &mut buf).expect("JSON write");
    let mut value: Value = serde_json::from_slice(&buf).expect("JSON parse");
    crate::test::remove_location_fields(&mut value);
    value
}

/// The result of one trip through the oracle.
pub struct Trip {
    pub qmd: String,
    /// The input, read by the lenient reader and re-serialized.
    pub before: Value,
    /// The qmd, re-read and serialized.
    pub after: Value,
}

pub fn trip(input: &Value) -> Trip {
    let (pandoc, context) = read_pandoc_json(input);
    let before = ast_json(&pandoc, &context);
    let mut qmd = Vec::new();
    writers::qmd::write(&pandoc, &mut qmd).expect("qmd write");
    let qmd = String::from_utf8(qmd).expect("qmd is UTF-8");
    // A qmd the reader rejects is a round-trip failure like any other, so it
    // becomes the `after` value instead of a panic: table-driven tests then
    // report every failing case together.
    let after = match readers::qmd::read(
        qmd.as_bytes(),
        false,
        "<generated>",
        &mut std::io::sink(),
        true,
        None,
    ) {
        Ok((reread, reread_context, _warnings)) => ast_json(&reread, &reread_context),
        Err(e) => json!({"blocks": format!("regenerated qmd failed to parse: {e:?}")}),
    };
    Trip { qmd, before, after }
}

/// The AST of `blocks` only (metadata and block list), for readable diffs.
fn blocks_of(v: &Value) -> &Value {
    &v["blocks"]
}

/// Pandoc-shaped JSON → qmd → re-read must give the same AST.
#[track_caller]
pub fn assert_roundtrip(input: Value) -> String {
    let t = trip(&input);
    assert_eq!(
        blocks_of(&t.before),
        blocks_of(&t.after),
        "AST changed across the qmd round trip\n--- qmd ---\n{}",
        t.qmd
    );
    t.qmd
}

/// Like [`assert_roundtrip`], for cases where the oracle allows a known
/// difference: `allow` rewrites the expected (input) AST before comparing.
#[track_caller]
pub fn assert_roundtrip_with(input: Value, allow: impl FnOnce(&mut Value)) -> String {
    let mut t = trip(&input);
    allow(&mut t.before);
    assert_eq!(
        blocks_of(&t.before),
        blocks_of(&t.after),
        "AST changed across the qmd round trip\n--- qmd ---\n{}",
        t.qmd
    );
    t.qmd
}

/// The qmd text the writer produces for `input`.
pub fn write_qmd(input: &Value) -> String {
    let (pandoc, _) = read_pandoc_json(input);
    let mut buf = Vec::new();
    writers::qmd::write(&pandoc, &mut buf).expect("qmd write");
    String::from_utf8(buf).expect("qmd is UTF-8")
}

// ---------------------------------------------------------------------------
// T1: text at a line start that re-reads as a construct
// ---------------------------------------------------------------------------

/// Texts whose first word is a block-level marker when it starts a line.
const LINE_START_TEXTS: &[&str] = &[
    "1. x", "1) x", "12. x", "- x", "+ x", "* x", "# x", "> x", ": x", "::: x", "| x", "``` x",
    "~~~ x", "--- x", "*** x", "___ x", "=== x", "1.", "-", "+", ":", ":::", "---", "===",
];

/// Round-trip `build(text)` for every line-start text; every failure is
/// reported together, so one run shows which constructs still break.
#[track_caller]
fn check_all(what: &str, texts: &[&str], build: impl Fn(&str) -> Value) {
    let failures: Vec<String> = texts
        .iter()
        .filter_map(|text| {
            let t = trip(&build(text));
            (blocks_of(&t.before) != blocks_of(&t.after))
                .then(|| format!("{text:?} -> {:?}", t.qmd))
        })
        .collect();
    assert!(
        failures.is_empty(),
        "{what}: {} of {} changed across the round trip:\n{}",
        failures.len(),
        texts.len(),
        failures.join("\n")
    );
}

#[test]
fn paragraph_start() {
    check_all("paragraph start", LINE_START_TEXTS, |t| {
        doc(vec![para_text(t)])
    });
}

#[test]
fn after_soft_break() {
    check_all("after soft break", LINE_START_TEXTS, |t| {
        doc(vec![para_text(&format!("para\n{t}"))])
    });
}

#[test]
fn after_line_break() {
    check_all("after line break", LINE_START_TEXTS, |t| {
        doc(vec![para_text(&format!("para\\\n{t}"))])
    });
}

#[test]
fn bullet_item_start() {
    check_all("bullet item start", LINE_START_TEXTS, |t| {
        doc(vec![bullet_list(vec![vec![plain(inl(t))]])])
    });
}

#[test]
fn ordered_item_start() {
    check_all("ordered item start", LINE_START_TEXTS, |t| {
        doc(vec![ordered_list(1, vec![vec![plain(inl(t))]])])
    });
}

#[test]
fn block_quote_start() {
    check_all("block quote start", LINE_START_TEXTS, |t| {
        doc(vec![block_quote(vec![para_text(t)])])
    });
}

#[test]
fn numbers_that_are_not_markers_stay_plain() {
    // Not followed by a space, so not a list marker: no escape needed.
    for text in ["1.5 x", "-5 x", "+1 x", "1.5"] {
        let qmd = assert_roundtrip(doc(vec![para_text(text)]));
        assert_eq!(qmd, format!("{text}\n"), "{text:?} was escaped needlessly");
    }
}

#[test]
fn mid_line_markers_are_not_escaped() {
    let qmd = assert_roundtrip(doc(vec![para_text("a - b + c 1. d : e")]));
    assert_eq!(qmd, "a - b + c 1. d : e\n");
}

#[test]
fn soft_break_nested_in_inline_containers() {
    let wraps: [(&str, fn(Vec<Value>) -> Value); 3] = [
        ("emph", emph),
        ("strong", strong),
        ("span", |c| span(attr("", &["k"], &[]), c)),
    ];
    for (name, wrap) in wraps {
        check_all(
            &format!("soft break inside {name}"),
            &["- x", "1. x", "+ x", "=== x", ": x", "::: x"],
            |t| doc(vec![para(vec![wrap(inl(&format!("a\n{t}")))])]),
        );
    }
}

#[test]
fn table_cell_start() {
    let cell =
        |text: &str| json!([attr("", &[], &[]), {"t": "AlignDefault"}, 1, 1, [plain(inl(text))]]);
    let row = |texts: &[&str]| {
        json!([
            attr("", &[], &[]),
            texts.iter().map(|t| cell(t)).collect::<Vec<_>>()
        ])
    };
    let table = |head: Value, body: Value| {
        json!({"t": "Table", "c": [
            attr("", &[], &[]),
            [null, []],
            [[{"t": "AlignDefault"}, {"t": "ColWidthDefault"}]],
            [attr("", &[], &[]), [head]],
            [[attr("", &[], &[]), 0, [], [body]]],
            [attr("", &[], &[]), []]
        ]})
    };
    for text in ["1. c", "- c", "+ c"] {
        assert_roundtrip(doc(vec![table(row(&["h"]), row(&[text]))]));
    }
}

#[test]
fn span_followed_by_paren_does_not_become_a_link() {
    let plain_span = span(attr("", &[], &[]), inl("range"));
    let comment = span(
        attr("", &["quarto-edit-comment"], &[("author", "A")]),
        inl("c"),
    );
    let wrapper = span(attr("", &[], &[]), vec![str_("range"), space(), comment]);
    for first in [plain_span, wrapper] {
        let input = doc(vec![para(vec![first, str_("(see)")])]);
        assert_roundtrip(input);
    }
}

#[test]
fn span_followed_by_bracket_and_brace_regression() {
    let s = || span(attr("", &[], &[]), inl("range"));
    assert_roundtrip(doc(vec![para(vec![s(), str_("[ref]")])]));
    assert_roundtrip(doc(vec![para(vec![s(), str_("{y}")])]));
}

// ---------------------------------------------------------------------------
// T4: editorial shorthand with attributes
// ---------------------------------------------------------------------------

fn author_date() -> Vec<(&'static str, &'static str)> {
    vec![("author", "Ann Author"), ("date", "2026-09-01T10:00:00Z")]
}

#[test]
fn editorial_marks_with_author_and_date() {
    for (class, marker) in [
        ("quarto-insert", "[++ "),
        ("quarto-delete", "[-- "),
        ("quarto-highlight", "[!! "),
        ("quarto-edit-comment", "[>> "),
    ] {
        let mark = span(attr("", &[class], &author_date()), inl("some text"));
        let qmd = assert_roundtrip(doc(vec![para(vec![
            str_("a"),
            space(),
            mark,
            space(),
            str_("b"),
        ])]));
        assert!(
            qmd.contains(&format!("{marker}some text]{{")),
            "{class} lost its shorthand: {qmd:?}"
        );
        assert!(
            !qmd.contains(".quarto-"),
            "{class} written generically: {qmd:?}"
        );
    }
}

#[test]
fn comment_nested_in_a_plain_span_with_attributes() {
    // The I4 shape: a commented range, then the comment.
    let comment = span(
        attr("", &["quarto-edit-comment"], &author_date()),
        inl("Please reword this."),
    );
    let mut content = inl("a commented range");
    content.push(space());
    content.push(comment);
    let wrapper = span(attr("", &[], &[]), content);
    let qmd = assert_roundtrip(doc(vec![para(vec![
        wrapper,
        str_("."),
        space(),
        str_("Next"),
    ])]));
    assert!(
        qmd.starts_with("[a commented range [>> Please reword this.]{author="),
        "{qmd:?}"
    );
}

#[test]
fn editorial_mark_with_an_id_and_an_extra_class() {
    let mark = span(
        attr("i1", &["quarto-insert", "foo"], &author_date()),
        inl("x"),
    );
    let qmd = assert_roundtrip(doc(vec![para(vec![mark])]));
    assert!(qmd.starts_with("[++ x]{#i1 .foo author="), "{qmd:?}");
}

#[test]
fn editorial_mark_class_not_first_keeps_the_generic_form() {
    let mark = span(
        attr("", &["foo", "quarto-insert"], &author_date()),
        inl("x"),
    );
    let qmd = assert_roundtrip(doc(vec![para(vec![mark])]));
    assert!(qmd.contains(".quarto-insert"), "{qmd:?}");
}

#[test]
fn two_editorial_classes_keep_the_generic_form() {
    let mark = span(attr("", &["quarto-insert", "quarto-delete"], &[]), inl("x"));
    let qmd = assert_roundtrip(doc(vec![para(vec![mark])]));
    assert!(qmd.contains(".quarto-insert"), "{qmd:?}");
}

#[test]
fn editorial_mark_without_attributes_is_unchanged() {
    let mark = span(attr("", &["quarto-insert"], &[]), inl("x"));
    let qmd = assert_roundtrip(doc(vec![para(vec![mark])]));
    assert_eq!(qmd, "[++ x]\n");
}

// ---------------------------------------------------------------------------
// T3: adjacent lists
// ---------------------------------------------------------------------------

fn bullets(texts: &[&str]) -> Value {
    bullet_list(texts.iter().map(|t| vec![plain(inl(t))]).collect())
}

#[test]
fn adjacent_bullet_lists_alternate_markers() {
    let qmd = assert_roundtrip(doc(vec![bullets(&["A"]), bullets(&["B"])]));
    assert_eq!(qmd, "* A\n\n- B\n");
}

#[test]
fn three_adjacent_bullet_lists() {
    let qmd = assert_roundtrip(doc(vec![bullets(&["A"]), bullets(&["B"]), bullets(&["C"])]));
    assert_eq!(qmd, "* A\n\n- B\n\n* C\n");
}

#[test]
fn bullet_lists_separated_by_a_paragraph_keep_the_star() {
    let qmd = assert_roundtrip(doc(vec![
        bullets(&["A"]),
        para_text("mid"),
        bullets(&["B"]),
    ]));
    assert_eq!(qmd, "* A\n\nmid\n\n* B\n");
}

#[test]
fn adjacent_bullet_lists_in_nested_containers() {
    // In a list item, a block quote and a div.
    let item = vec![plain(inl("item")), bullets(&["x"]), bullets(&["y"])];
    assert_roundtrip(doc(vec![bullet_list(vec![item.clone()])]));
    assert_roundtrip(doc(vec![block_quote(vec![
        bullets(&["x"]),
        bullets(&["y"]),
    ])]));
    let div = json!({"t": "Div", "c": [attr("", &["d"], &[]), [bullets(&["x"]), bullets(&["y"])]]});
    assert_roundtrip(doc(vec![div]));
    // A list whose last item ends in a list, followed by another list.
    let outer = bullet_list(vec![vec![plain(inl("a")), bullets(&["inner"])]]);
    assert_roundtrip(doc(vec![outer, bullets(&["next"])]));
}

#[test]
fn adjacent_bullet_lists_with_empty_items() {
    let empty = bullet_list(vec![vec![]]);
    assert_roundtrip(doc(vec![
        bullets(&["A"]),
        empty.clone(),
        empty,
        bullets(&["B"]),
    ]));
}

/// The oracle's allowance for adjacent ordered lists: the writer separates them
/// with a `<!-- -->` raw block, which re-reads as exactly that block.
fn with_list_separators(value: &mut Value) {
    match value {
        Value::Array(items) => {
            let mut out: Vec<Value> = Vec::with_capacity(items.len());
            for mut item in items.drain(..) {
                with_list_separators(&mut item);
                let is_ol = |v: &Value| v["t"] == "OrderedList";
                if out.last().is_some_and(is_ol) && is_ol(&item) {
                    out.push(json!({"t": "RawBlock", "c": ["html", "<!-- -->"]}));
                }
                out.push(item);
            }
            *items = out;
        }
        Value::Object(map) => map.values_mut().for_each(with_list_separators),
        _ => {}
    }
}

fn numbered(start: i64, texts: &[&str]) -> Value {
    ordered_list(start, texts.iter().map(|t| vec![plain(inl(t))]).collect())
}

#[test]
fn adjacent_ordered_lists_get_a_separator() {
    // Word restarting its numbering: 1, 2 then 1 again.
    let qmd = assert_roundtrip_with(
        doc(vec![numbered(1, &["a", "b"]), numbered(1, &["c"])]),
        with_list_separators,
    );
    assert_eq!(qmd, "1.  a\n2.  b\n\n<!-- -->\n\n1.  c\n");
}

#[test]
fn adjacent_ordered_lists_with_a_different_start() {
    assert_roundtrip_with(
        doc(vec![
            numbered(1, &["a"]),
            numbered(5, &["b"]),
            numbered(1, &["c"]),
        ]),
        with_list_separators,
    );
}

#[test]
fn adjacent_ordered_lists_in_nested_containers() {
    let two = || vec![numbered(1, &["x"]), numbered(1, &["y"])];
    assert_roundtrip_with(doc(vec![block_quote(two())]), with_list_separators);
    let div = json!({"t": "Div", "c": [attr("", &["d"], &[]), two()]});
    assert_roundtrip_with(doc(vec![div]), with_list_separators);
    // In a loose item (a Para first) the blank lines already keep it apart.
    let mut item = vec![para_text("item"), para_text("more")];
    item.extend(two());
    assert_roundtrip_with(
        doc(vec![bullet_list(vec![item.clone()])]),
        with_list_separators,
    );
    assert_roundtrip_with(doc(vec![ordered_list(1, vec![item])]), with_list_separators);
}

/// Every `Plain` as a `Para`, for comparisons that don't care which one a
/// list item's first block is.
fn plains_as_paras(value: &mut Value) {
    match value {
        Value::Array(items) => items.iter_mut().for_each(plains_as_paras),
        Value::Object(map) => {
            if map.get("t") == Some(&json!("Plain")) {
                map.insert("t".to_string(), json!("Para"));
            }
            map.values_mut().for_each(plains_as_paras);
        }
        _ => {}
    }
}

#[test]
fn adjacent_ordered_lists_in_a_tight_item_are_not_swallowed() {
    // A tight item has no blank line between its blocks, so the separator would
    // read back as text on the preceding list line. The writer loosens the item
    // instead (its first `Plain` becomes a `Para`), so only Plain/Para may differ.
    let mut item = vec![plain(inl("item"))];
    item.extend([numbered(1, &["x"]), numbered(1, &["y"])]);
    for outer in [
        bullet_list(vec![item.clone()]),
        ordered_list(1, vec![item.clone()]),
    ] {
        let mut t = trip(&doc(vec![outer]));
        with_list_separators(&mut t.before);
        plains_as_paras(&mut t.before);
        plains_as_paras(&mut t.after);
        assert_eq!(
            t.before["blocks"], t.after["blocks"],
            "--- qmd ---\n{}",
            t.qmd
        );
    }
}

#[test]
fn ordered_list_next_to_a_bullet_list_needs_no_separator() {
    let qmd = assert_roundtrip(doc(vec![numbered(1, &["a"]), bullets(&["b"])]));
    assert!(!qmd.contains("<!--"), "{qmd:?}");
}

#[test]
fn documents_without_the_trigger_shapes_are_written_unchanged() {
    // The pre-pass must not touch (or clone) an AST with neither a multi-block
    // note nor adjacent ordered lists: this is every AST the qmd reader makes.
    let input = doc(vec![
        para(vec![
            str_("a"),
            note(vec![para_text("single paragraph")]),
            str_("b"),
        ]),
        numbered(1, &["x"]),
        para_text("between"),
        numbered(1, &["y"]),
    ]);
    let qmd = assert_roundtrip(input);
    assert_eq!(qmd, "a^[single paragraph]b\n\n1.  x\n\nbetween\n\n1.  y\n");
}

#[test]
fn tracked_writer_gives_the_same_text_for_rewritten_documents() {
    // `write_with_source_info` (the incremental writer's whole-document path)
    // runs the same pre-pass, so the inserted blocks must not break its tiling.
    let n = note(vec![para_text("first"), para_text("second")]);
    let mut inlines = inl("Text");
    inlines.push(n);
    let input = doc(vec![
        para(inlines),
        numbered(1, &["a"]),
        numbered(1, &["b"]),
        para_text("end"),
    ]);
    let (pandoc, _) = read_pandoc_json(&input);
    let mut plain_buf = Vec::new();
    writers::qmd::write(&pandoc, &mut plain_buf).unwrap();
    let (tracked, _source_info) = writers::qmd::write_with_source_info(&pandoc).unwrap();
    assert_eq!(
        String::from_utf8(tracked).unwrap(),
        String::from_utf8(plain_buf).unwrap()
    );
}

#[test]
fn the_bullet_marker_does_not_alternate_across_list_items() {
    // Each item starts a new sequence of blocks: its nested list has no previous
    // sibling, so every one of them keeps the `*` (the shape of a list-table's
    // rows, which qmd-syntax-helper's grid-table conversion writes).
    let rows = bullet_list(vec![
        vec![bullets(&["h1", "h2"])],
        vec![bullets(&["a", "b"])],
        vec![bullets(&["c", "d"])],
    ]);
    let qmd = assert_roundtrip(doc(vec![rows]));
    assert!(
        !qmd.contains("- "),
        "a nested list took the alternate marker:\n{qmd}"
    );
    // The same holds in ordered-list items and in a nested block quote.
    let numbered_rows = ordered_list(1, vec![vec![bullets(&["a"])], vec![bullets(&["b"])]]);
    let qmd = assert_roundtrip(doc(vec![numbered_rows]));
    assert!(!qmd.contains("- "), "{qmd}");
}
