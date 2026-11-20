/*
 * incremental_meta_tests.rs
 *
 * The incremental QMD writer's handling of a changed front matter.
 *
 * Copyright (c) 2026 Posit, PBC
 */

use pampa::pandoc::Pandoc;
use pampa::writers;
use quarto_ast_reconcile::compute_reconciliation;
use quarto_pandoc_types::{ConfigMapEntry, ConfigValueKind};

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

fn write(original_qmd: &str, new_ast: &Pandoc) -> String {
    let original_ast = parse_qmd(original_qmd);
    let plan = compute_reconciliation(&original_ast, new_ast);
    writers::incremental::incremental_write(original_qmd, &original_ast, new_ast, &plan)
        .expect("incremental_write failed")
}

/// Plain-text inlines for `text`: its words as `Str`s, with a `Space`
/// between each two.
fn words(text: &str) -> Vec<pampa::pandoc::Inline> {
    text.split(' ')
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
        .collect()
}

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

// =============================================================================
// Text that looks like markdown structure
// =============================================================================

#[test]
fn text_starting_like_a_block_reads_back_as_text() {
    let doc = "---\ntitle: Old\n---\n\nBody.\n";
    for text in [
        "- dash", "+ plus", "1. one", "1) one", "::: div", "> quote", "# hash",
    ] {
        let mut ast = parse_qmd(doc);
        entries(&mut ast)[0].value.value = ConfigValueKind::PandocInlines(words(text));
        let result = write(doc, &ast);
        assert_eq!(title_text(&result), text, "written as:\n{result}");
    }
}
