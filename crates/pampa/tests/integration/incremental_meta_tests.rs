/*
 * incremental_meta_tests.rs
 *
 * The incremental QMD writer's handling of a changed front matter: only
 * the top-level keys whose values changed are rewritten; every other line
 * (other keys, comments, blank lines, key order) is kept byte for byte.
 *
 * Copyright (c) 2026 Posit, PBC
 */

use pampa::pandoc::Pandoc;
use pampa::writers;
use quarto_ast_reconcile::compute_reconciliation;
use quarto_pandoc_types::{ConfigMapEntry, ConfigValueKind};
use std::io::Cursor;

// =============================================================================
// Helpers
// =============================================================================

fn parse_qmd(input: &str) -> Pandoc {
    pampa::readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    )
    .expect("Failed to parse QMD")
    .0
}

fn entries(ast: &mut Pandoc) -> &mut Vec<ConfigMapEntry> {
    match &mut ast.meta.value {
        ConfigValueKind::Map(entries) => entries,
        _ => panic!("metadata is not a map"),
    }
}

/// The single entry of the front matter `yaml`, as the reader parses it.
fn entry(yaml: &str) -> ConfigMapEntry {
    let mut ast = parse_qmd(&format!("---\n{yaml}\n---\n"));
    entries(&mut ast).remove(0)
}

/// Set a top-level key to the entry parsed from `yaml`, in place, or add
/// it at the end.
fn set(ast: &mut Pandoc, yaml: &str) {
    let new = entry(yaml);
    let entries = entries(ast);
    match entries.iter_mut().find(|e| e.key == new.key) {
        Some(existing) => *existing = new,
        None => entries.push(new),
    }
}

fn remove(ast: &mut Pandoc, key: &str) {
    entries(ast).retain(|e| e.key != key);
}

fn write(original_qmd: &str, new_ast: &Pandoc) -> String {
    let original_ast = parse_qmd(original_qmd);
    let plan = compute_reconciliation(&original_ast, new_ast);
    writers::incremental::incremental_write(original_qmd, &original_ast, new_ast, &plan)
        .expect("incremental_write failed")
}

/// As the WASM `incremental_write_qmd` sees an edit: the new AST has been
/// through JSON, which sorts the metadata keys and loses their positions.
fn write_via_json(original_qmd: &str, new_ast: &Pandoc) -> String {
    let mut buf = Vec::new();
    let context = pampa::pandoc::ASTContext::default();
    writers::json::write(new_ast, &context, &mut buf).expect("Failed to write JSON");
    let from_json = pampa::readers::json::read(&mut Cursor::new(buf))
        .expect("Failed to read JSON")
        .0;
    write(original_qmd, &from_json)
}

/// Apply `edit` to the parse of `original` and write it back, checking
/// that the direct and the JSON paths agree.
fn edited(original: &str, edit: impl Fn(&mut Pandoc)) -> String {
    let mut ast = parse_qmd(original);
    edit(&mut ast);
    let direct = write(original, &ast);
    let via_json = write_via_json(original, &ast);
    assert_eq!(direct, via_json, "direct and JSON paths differ");
    direct
}

const DOC: &str = "\
---
# About the report
title:   Quarterly report   # working title
subtitle: \"Third quarter\"

author:
  - name: Ana Silva
    affiliations: [{ref: posit}]
date: 2026-11-12
# The end
---

Body   text.
";

// =============================================================================
// Changing a value
// =============================================================================

#[test]
fn changed_value_rewrites_only_its_line() {
    let result = edited(DOC, |ast| set(ast, "date: 2026-12-01"));
    assert_eq!(result, DOC.replace("date: 2026-11-12", "date: 2026-12-01"));
}

#[test]
fn changed_text_value_rewrites_only_its_line() {
    let result = edited(DOC, |ast| set(ast, "subtitle: Fourth quarter"));
    assert_eq!(
        result,
        DOC.replace("subtitle: \"Third quarter\"", "subtitle: Fourth quarter")
    );
}

#[test]
fn rewritten_line_loses_its_own_formatting_only() {
    let result = edited(DOC, |ast| set(ast, "title: Annual report"));
    assert_eq!(
        result,
        DOC.replace(
            "title:   Quarterly report   # working title",
            "title: Annual report"
        )
    );
}

#[test]
fn changed_nested_value_rewrites_its_whole_entry() {
    let result = edited(DOC, |ast| set(ast, "author:\n  - name: Ben Okafor"));
    assert_eq!(
        result,
        DOC.replace(
            "author:\n  - name: Ana Silva\n    affiliations: [{ref: posit}]\n",
            "author:\n  - name: Ben Okafor\n"
        )
    );
}

#[test]
fn several_changes_at_once() {
    let result = edited(DOC, |ast| {
        set(ast, "subtitle: Fourth quarter");
        set(ast, "date: 2026-12-01");
    });
    assert_eq!(
        result,
        DOC.replace("subtitle: \"Third quarter\"", "subtitle: Fourth quarter")
            .replace("date: 2026-11-12", "date: 2026-12-01")
    );
}

// =============================================================================
// Removing and adding keys
// =============================================================================

#[test]
fn removed_key_removes_only_its_lines() {
    let result = edited(DOC, |ast| remove(ast, "subtitle"));
    assert_eq!(result, DOC.replace("subtitle: \"Third quarter\"\n", ""));
}

#[test]
fn removed_nested_key_removes_all_its_lines() {
    let result = edited(DOC, |ast| remove(ast, "author"));
    assert_eq!(
        result,
        DOC.replace(
            "author:\n  - name: Ana Silva\n    affiliations: [{ref: posit}]\n",
            ""
        )
    );
}

#[test]
fn added_key_goes_after_the_last_key() {
    let result = edited(DOC, |ast| set(ast, "lang: en"));
    assert_eq!(
        result,
        DOC.replace("date: 2026-11-12\n", "date: 2026-11-12\nlang: en\n")
    );
}

#[test]
fn sequence_at_the_key_indent_belongs_to_its_key() {
    let doc = "---\nauthor:\n- Ana\n- Ben\ntitle: Old\n---\n\nBody.\n";
    assert_eq!(
        edited(doc, |ast| set(ast, "title: New")),
        doc.replace("Old", "New")
    );
    assert_eq!(
        edited(doc, |ast| remove(ast, "author")),
        "---\ntitle: Old\n---\n\nBody.\n"
    );
}

// =============================================================================
// Multi-line text
// =============================================================================

const ABSTRACT_DOC: &str = "\
---
title: Report
abstract: |
  First paragraph.

  Second paragraph.
author: Ana
---

Body.
";

#[test]
fn block_text_is_written_as_a_literal_block() {
    let result = edited(ABSTRACT_DOC, |ast| {
        set(
            ast,
            "abstract: |\n  First paragraph.\n\n  Second one.\n\n  Third.",
        )
    });
    assert_eq!(
        result,
        ABSTRACT_DOC.replace("Second paragraph.\n", "Second one.\n\n  Third.\n")
    );
}

#[test]
fn single_line_text_replacing_a_block_is_one_line() {
    let result = edited(ABSTRACT_DOC, |ast| set(ast, "abstract: Just this."));
    assert_eq!(
        result,
        ABSTRACT_DOC.replace(
            "abstract: |\n  First paragraph.\n\n  Second paragraph.\n",
            "abstract: Just this.\n"
        )
    );
}

// =============================================================================
// Shapes of document
// =============================================================================

#[test]
fn front_matter_without_a_body() {
    let doc = "---\n# note\ntitle: Old\nauthor: Ana\n---\n";
    assert_eq!(
        edited(doc, |ast| set(ast, "title: New")),
        doc.replace("Old", "New")
    );
}

#[test]
fn body_edits_keep_the_front_matter() {
    let mut ast = parse_qmd(DOC);
    set(&mut ast, "date: 2026-12-01");
    if let pampa::pandoc::Block::Paragraph(ref mut p) = ast.blocks[0] {
        p.content = vec![pampa::pandoc::Inline::Str(pampa::pandoc::Str {
            text: "Changed.".to_string(),
            source_info: quarto_source_map::SourceInfo::for_test(),
        })];
    }
    let result = write(DOC, &ast);
    assert_eq!(
        result,
        DOC.replace("date: 2026-11-12", "date: 2026-12-01")
            .replace("Body   text.", "Changed.")
    );
}

#[test]
fn result_reads_back_as_the_new_metadata() {
    let result = edited(DOC, |ast| {
        set(ast, "title: \"Costs: up *again*\"");
        remove(ast, "subtitle");
        set(ast, "lang: en");
    });
    let mut reread = parse_qmd(&result);
    let keys: Vec<_> = entries(&mut reread).iter().map(|e| e.key.clone()).collect();
    assert_eq!(keys, ["title", "author", "date", "lang"]);
}

// =============================================================================
// Text that looks like markdown structure
// =============================================================================

/// The plain text of the `title` in `qmd`.
fn title_text(qmd: &str) -> String {
    let mut ast = parse_qmd(qmd);
    let title = entries(&mut ast)
        .iter()
        .find(|e| e.key == "title")
        .expect("no title")
        .clone();
    match title.value.value {
        ConfigValueKind::PandocInlines(inlines) => inlines
            .iter()
            .map(|inline| match inline {
                pampa::pandoc::Inline::Str(s) => s.text.clone(),
                pampa::pandoc::Inline::Space(_) => " ".to_string(),
                other => panic!("unexpected inline {other:?}"),
            })
            .collect(),
        other => panic!("title is not inlines: {other:?}"),
    }
}

#[test]
fn text_starting_like_a_block_reads_back_as_text() {
    let doc = "---\ntitle: Old\n---\n\nBody.\n";
    for text in [
        "- dash", "+ plus", "1. one", "1) one", "::: div", "> quote", "# hash",
    ] {
        let mut ast = parse_qmd(doc);
        let words: Vec<pampa::pandoc::Inline> = text
            .split(' ')
            .flat_map(|word| {
                [
                    pampa::pandoc::Inline::Space(pampa::pandoc::Space {
                        source_info: quarto_source_map::SourceInfo::for_test(),
                    }),
                    pampa::pandoc::Inline::Str(pampa::pandoc::Str {
                        text: word.to_string(),
                        source_info: quarto_source_map::SourceInfo::for_test(),
                    }),
                ]
            })
            .skip(1)
            .collect();
        entries(&mut ast)[0].value.value = ConfigValueKind::PandocInlines(words);
        let result = write(doc, &ast);
        assert_eq!(title_text(&result), text, "written as:\n{result}");
    }
}

// =============================================================================
// Front matter the reader couldn't read
// =============================================================================

#[test]
fn unreadable_front_matter_is_reported_alongside_the_ast() {
    let broken = "---\ntitle: \"Quarterly report\nsubtitle: Third\n---\n\nBody.\n";
    let (ast, _context, reported) =
        pampa::wasm_entry_points::qmd_to_pandoc_with_diagnostics(broken.as_bytes())
            .expect("the document still reads");
    // The AST has none of the metadata...
    assert!(matches!(&ast.meta.value, ConfigValueKind::Map(entries) if entries.is_empty()));
    // ...and the reader says why.
    assert!(
        reported.iter().any(|d| d.code.as_deref() == Some("Q-1-1")),
        "reported: {reported:?}"
    );

    let fine = "---\ntitle: Quarterly report\n---\n\nBody.\n";
    let (_, _, reported) =
        pampa::wasm_entry_points::qmd_to_pandoc_with_diagnostics(fine.as_bytes()).unwrap();
    assert!(reported.is_empty());
}
