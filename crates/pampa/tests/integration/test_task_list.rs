//! Task-list parsing and rendering (bd-obkvhlam, bd-2ydvebkc).
//!
//! `- [ ] todo` / `- [x] done` follow Pandoc's convention: the item's
//! first inline is `Str "☐"` (U+2610) or `Str "☒"` (U+2612) followed by
//! `Space`; an empty item (`- [ ] `) is `Plain [Str "☐"]`, as in Pandoc.
//! The HTML writer follows Pandoc's writer — `<ul class="task-list">` when
//! every item is a task item, and each task item's marker becomes
//! `<label><input type="checkbox" [checked=""] disabled="" />…</label>` —
//! plus two deliberate departures (GH #892): every task `<li>` carries
//! `class="task-list-item"` so CSS can hide the marker per item, and the
//! static checkbox is `disabled` (Pandoc ≥ 3.0.1 emits it enabled). The qmd
//! writer round-trips the ballot-box Str back to `[ ]` / `[x]`.

use pampa::pandoc::ast_context::ASTContext;
use pampa::pandoc::{Block, Inline};
use pampa::readers;
use pampa::writers;

fn parse_qmd(input: &str) -> (pampa::pandoc::Pandoc, ASTContext) {
    let result = readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    )
    .expect("Failed to parse QMD");
    (result.0, result.1)
}

fn to_html(input: &str) -> String {
    let (pandoc, ctx) = parse_qmd(input);
    let mut buf = Vec::new();
    writers::html::write(&pandoc, &ctx, &mut buf).expect("html write");
    String::from_utf8(buf).expect("utf8")
}

fn to_qmd(input: &str) -> String {
    let (pandoc, _ctx) = parse_qmd(input);
    let mut buf = Vec::new();
    writers::qmd::write(&pandoc, &mut buf).expect("qmd write");
    String::from_utf8(buf).expect("utf8")
}

/// First-item inlines of the first block (expected BulletList/OrderedList).
fn first_item_inlines(pandoc: &pampa::pandoc::Pandoc) -> &Vec<Inline> {
    let items = match &pandoc.blocks[0] {
        Block::BulletList(bl) => &bl.content,
        Block::OrderedList(ol) => &ol.content,
        other => panic!("Expected a list block, got {:?}", other),
    };
    match &items[0][0] {
        Block::Plain(p) => &p.content,
        Block::Paragraph(p) => &p.content,
        other => panic!("Expected Plain/Paragraph item head, got {:?}", other),
    }
}

// ============================================================================
// Reader: AST shape
// ============================================================================

#[test]
fn unchecked_item_parses_to_ballot_box_str() {
    let (pandoc, _) = parse_qmd("- [ ] todo\n");
    let inlines = first_item_inlines(&pandoc);
    match &inlines[0] {
        Inline::Str(s) => assert_eq!(s.text, "☐"),
        other => panic!("Expected Str ☐, got {:?}", other),
    }
    assert!(
        matches!(&inlines[1], Inline::Space(_)),
        "Space after marker"
    );
    match &inlines[2] {
        Inline::Str(s) => assert_eq!(s.text, "todo"),
        other => panic!("Expected Str todo, got {:?}", other),
    }
}

#[test]
fn checked_item_parses_to_crossed_ballot_box_str() {
    for src in ["- [x] done\n", "- [X] done\n"] {
        let (pandoc, _) = parse_qmd(src);
        let inlines = first_item_inlines(&pandoc);
        match &inlines[0] {
            Inline::Str(s) => assert_eq!(s.text, "☒", "for source {src:?}"),
            other => panic!("Expected Str ☒ for {src:?}, got {other:?}"),
        }
    }
}

#[test]
fn ordered_list_task_item_parses() {
    let (pandoc, _) = parse_qmd("1. [ ] todo\n");
    let inlines = first_item_inlines(&pandoc);
    match &inlines[0] {
        Inline::Str(s) => assert_eq!(s.text, "☐"),
        other => panic!("Expected Str ☐, got {:?}", other),
    }
}

#[test]
fn marker_source_info_covers_bracket_bytes() {
    // `- [ ] todo` — the marker Str's range must start at the `[` (byte 2)
    // so an interactive toggle can splice exactly those bytes.
    let (pandoc, _) = parse_qmd("- [ ] todo\n");
    let inlines = first_item_inlines(&pandoc);
    let Inline::Str(s) = &inlines[0] else {
        panic!("Expected Str ☐, got {:?}", inlines[0]);
    };
    assert_eq!(s.source_info.start_offset(), 2);
    // End covers at least the closing bracket (offset 5 = exclusive end of
    // `[ ]`); the trailing space may or may not be included by the token.
    assert!(s.source_info.end_offset() >= 5);
}

#[test]
fn non_task_brackets_unaffected() {
    // A link, a two-char span, and a mid-text span must not become tasks.
    let (pandoc, _) = parse_qmd("- [x](https://example.com)\n");
    let inlines = first_item_inlines(&pandoc);
    assert!(
        matches!(&inlines[0], Inline::Link(_)),
        "expected Link, got {:?}",
        inlines[0]
    );

    let (pandoc, _) = parse_qmd("- [xx] nope\n");
    let inlines = first_item_inlines(&pandoc);
    assert!(
        !matches!(&inlines[0], Inline::Str(s) if s.text == "☒"),
        "[xx] must not be a task marker"
    );
}

// ============================================================================
// HTML writer
// ============================================================================

#[test]
fn html_all_task_items_gets_task_list_class() {
    let html = to_html("- [ ] todo\n- [x] done\n");
    assert!(
        html.contains("<ul class=\"task-list\">"),
        "missing ul.task-list in: {html}"
    );
    assert!(
        html.contains(
            "<li class=\"task-list-item\"><label><input type=\"checkbox\" disabled=\"\" />todo</label></li>"
        ),
        "missing unchecked checkbox in: {html}"
    );
    assert!(
        html.contains(
            "<li class=\"task-list-item\"><label><input type=\"checkbox\" checked=\"\" disabled=\"\" />done</label></li>"
        ),
        "missing checked checkbox in: {html}"
    );
    assert!(!html.contains('☐'), "ballot box leaked into HTML: {html}");
    assert!(!html.contains('☒'), "ballot box leaked into HTML: {html}");
}

#[test]
fn html_mixed_list_has_no_task_list_class_but_renders_checkboxes() {
    // Pandoc: the class requires ALL items to be tasks; task items still
    // render checkboxes individually.
    let html = to_html("- [ ] todo\n- plain\n");
    assert!(
        html.contains("<ul>"),
        "mixed list must not get task-list class: {html}"
    );
    assert!(
        html.contains("<li class=\"task-list-item\"><label><input type=\"checkbox\" disabled=\"\" />"),
        "task item in mixed list still renders a checkbox and the per-item class: {html}"
    );
    assert!(
        html.contains("<li>plain</li>"),
        "plain item in mixed list gets no task-list-item class: {html}"
    );
}

#[test]
fn html_ordered_task_items_render_checkboxes_without_class() {
    let html = to_html("1. [x] done\n");
    assert!(
        html.contains("<li class=\"task-list-item\"><label><input type=\"checkbox\" checked=\"\" disabled=\"\" />"),
        "ordered task item renders checkbox and the per-item class: {html}"
    );
    assert!(
        !html.contains("class=\"task-list\""),
        "ordered lists never get the list class (Pandoc parity): {html}"
    );
}

// ============================================================================
// qmd writer round-trip
// ============================================================================

#[test]
fn qmd_round_trip_preserves_task_markers() {
    let src = "- [ ] todo\n- [x] done\n";
    let out = to_qmd(src);
    assert!(out.contains("[ ] todo"), "unchecked round-trip, got: {out}");
    assert!(out.contains("[x] done"), "checked round-trip, got: {out}");
    assert!(!out.contains('☐'), "ballot box leaked into qmd: {out}");
    assert!(!out.contains('☒'), "ballot box leaked into qmd: {out}");
}

// ============================================================================
// Empty task items (bd-2ydvebkc)
// ============================================================================

#[test]
fn empty_task_item_parses_to_lone_ballot_box_str() {
    // `- [ ] ` is what a user has typed just before the item text; Pandoc
    // reads it as `Plain [Str "☐"]` (no Space).
    for (src, glyph) in [("- [ ] \n", "☐"), ("- [x] \n- [ ] next\n", "☒")] {
        let (pandoc, _) = parse_qmd(src);
        let inlines = first_item_inlines(&pandoc);
        assert_eq!(inlines.len(), 1, "for {src:?}: {inlines:?}");
        match &inlines[0] {
            Inline::Str(s) => {
                assert_eq!(s.text, glyph, "for {src:?}");
                assert_eq!(s.source_info.start_offset(), 2, "for {src:?}");
                assert_eq!(s.source_info.end_offset(), 5, "for {src:?}");
            }
            other => panic!("Expected Str {glyph} for {src:?}, got {other:?}"),
        }
    }
}

#[test]
fn html_empty_task_item_renders_checkbox() {
    let html = to_html("- [ ] \n");
    assert!(
        html.contains("<ul class=\"task-list\">"),
        "an empty task item is still a task item: {html}"
    );
    assert!(
        html.contains(
            "<li class=\"task-list-item\"><label><input type=\"checkbox\" disabled=\"\" /></label></li>"
        ),
        "empty task item renders an empty checkbox: {html}"
    );
    assert!(!html.contains('☐'), "ballot box leaked into HTML: {html}");
}

#[test]
fn html_task_item_class_composes_with_item_attr() {
    // A trailing block attribute is hoisted onto the <li> (bd-aeyss6p5);
    // the per-item task class must compose with it, not replace it.
    let html = to_html("- [ ] todo {.extra}\n");
    assert!(
        html.contains("<li class=\"task-list-item extra\">"),
        "task-list-item composes with the item attr: {html}"
    );
}

#[test]
fn qmd_round_trip_preserves_empty_task_item() {
    let out = to_qmd("- [ ] \n- [x] done\n");
    let (pandoc, _) = parse_qmd(&out);
    let inlines = first_item_inlines(&pandoc);
    assert!(
        matches!(&inlines[0], Inline::Str(s) if s.text == "☐"),
        "empty task item must re-parse as a task item; qmd was {out:?}"
    );
}
