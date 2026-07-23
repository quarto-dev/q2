//! End-to-end tests for AST diff annotation: parse two qmd documents,
//! annotate the diff with `quarto_ast_reconcile::annotate_diff`, and write
//! the annotated AST back to qmd. Added/removed blocks must render as
//! `::: {.added}` / `::: {.removed}` fenced divs, and added/removed inlines
//! as `[++ ...]` / `[-- ...]` editorial marks.

use pampa::wasm_entry_points::qmd_to_pandoc;
use quarto_ast_reconcile::annotate_diff;
use quarto_pandoc_types::Pandoc;

fn parse(qmd: &str) -> Pandoc {
    let (pandoc, _context) = qmd_to_pandoc(qmd.as_bytes()).expect("qmd should parse");
    pandoc
}

fn diff_to_qmd(before: &str, after: &str) -> String {
    let before_ast = parse(before);
    let after_ast = parse(after);
    let annotated = annotate_diff(&before_ast, &after_ast);
    let mut buf: Vec<u8> = Vec::new();
    pampa::writers::qmd::write(&annotated, &mut buf).expect("annotated AST should write as qmd");
    String::from_utf8(buf).expect("qmd output should be utf-8")
}

#[test]
fn added_block_renders_as_added_div() {
    let out = diff_to_qmd(
        "# Title\n\nUnchanged paragraph.\n",
        "# Title\n\nUnchanged paragraph.\n\nNew paragraph.\n",
    );

    assert!(
        out.contains("::: {.added}"),
        "expected ::: {{.added}} div in output:\n{out}"
    );
    assert!(
        out.contains("New paragraph."),
        "added content must appear in output:\n{out}"
    );
    assert!(
        !out.contains("::: {.removed}"),
        "nothing was removed:\n{out}"
    );
}

#[test]
fn removed_block_renders_as_removed_div() {
    let out = diff_to_qmd("First.\n\nOld paragraph.\n\nLast.\n", "First.\n\nLast.\n");

    assert!(
        out.contains("::: {.removed}"),
        "expected ::: {{.removed}} div in output:\n{out}"
    );
    assert!(
        out.contains("Old paragraph."),
        "removed content must still appear (inside the .removed div):\n{out}"
    );
    assert!(!out.contains("::: {.added}"), "nothing was added:\n{out}");
}

#[test]
fn changed_word_renders_as_inline_marks() {
    let out = diff_to_qmd("The cat sat on the mat.\n", "The dog sat on the mat.\n");

    assert!(
        out.contains("[-- cat]"),
        "expected [-- cat] delete mark in output:\n{out}"
    );
    assert!(
        out.contains("[++ dog]"),
        "expected [++ dog] insert mark in output:\n{out}"
    );
    // Block-level wrappers must NOT appear for a pure inline edit.
    assert!(
        !out.contains("::: {.added}") && !out.contains("::: {.removed}"),
        "inline edit must not produce block-level divs:\n{out}"
    );
}

#[test]
fn identical_documents_round_trip_without_annotations() {
    let doc = "# Title\n\nSome *emphasised* text.\n";
    let out = diff_to_qmd(doc, doc);

    for marker in ["::: {.added}", "::: {.removed}", "[++", "[--"] {
        assert!(
            !out.contains(marker),
            "identical docs must not contain {marker}:\n{out}"
        );
    }
    assert!(
        out.contains("Some *emphasised* text."),
        "content survives:\n{out}"
    );
}

#[test]
fn newly_added_list_is_wrapped_whole() {
    let out = diff_to_qmd("# hi\n", "# hi\n\n- my bullet point\n");

    // The entire BulletList is new content: the .added div must wrap the
    // list, not appear inside a kept list's item.
    assert!(
        !out.contains("* :::") && !out.contains("- :::"),
        "the list marker must be INSIDE the .added div, not outside:\n{out}"
    );
    assert!(
        out.contains("::: {.added}"),
        "expected ::: {{.added}} div:\n{out}"
    );
}

#[test]
fn list_item_content_added_from_empty_marker_wraps_bullet() {
    // Snapshot had a bare `- ` marker; the typed content is new. The bullet
    // must live INSIDE the .added div, not outside it.
    let out = diff_to_qmd("# hi\n\n- \n", "# hi\n\n- my bullet point\n");

    assert!(
        !out.contains("* :::") && !out.contains("- :::"),
        "list marker must be inside the .added div:\n{out}"
    );
    assert!(
        out.contains("::: {.added}"),
        "expected ::: {{.added}} div:\n{out}"
    );
    assert!(out.contains("my bullet point"), "content survives:\n{out}");
}

#[test]
fn item_added_to_existing_list_wraps_bullet() {
    let out = diff_to_qmd("- one\n- two\n", "- one\n- two\n- three\n");

    assert!(
        !out.contains("* :::") && !out.contains("- :::"),
        "list marker must be inside the .added div:\n{out}"
    );
    assert!(
        out.contains("::: {.added}"),
        "expected ::: {{.added}} div:\n{out}"
    );
    assert!(out.contains("three"), "added item survives:\n{out}");
}

#[test]
fn item_removed_from_existing_list_wraps_bullet() {
    let out = diff_to_qmd("- one\n- two\n- three\n", "- one\n- three\n");

    assert!(
        !out.contains("* :::") && !out.contains("- :::"),
        "list marker must be inside the .removed div:\n{out}"
    );
    assert!(
        out.contains("::: {.removed}"),
        "expected ::: {{.removed}} div:\n{out}"
    );
    assert!(
        out.contains("two"),
        "removed item survives inside the div:\n{out}"
    );
}
