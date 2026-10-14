/*
 * tests/integration/heading_id_after_expansion.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Heading ids follow the text that shortcode expansion and lmd cells
 * produce (bd-2wv8431v, bd-lmd-exploration-jqv2nbl4 spike 5).
 */

use std::sync::Arc;

use quarto_core::format::Format;
use quarto_core::pipeline::{HtmlRenderConfig, render_qmd_to_html};
use quarto_core::project::{DocumentInfo, ProjectConfig, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};

/// Render `qmd` through the real HTML pipeline and return the HTML.
async fn render_html(qmd: &str) -> String {
    let tmp = tempfile::TempDir::new().unwrap();
    let project_dir = tmp.path().to_path_buf();
    let main_path = project_dir.join("doc.qmd");
    std::fs::write(&main_path, qmd).unwrap();

    let project = ProjectContext {
        dir: project_dir.clone(),
        config: ProjectConfig::default(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path(&main_path)],
        output_dir: project_dir.clone(),
        ..Default::default()
    };
    let doc = DocumentInfo::from_path(&main_path);
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
    let runtime: Arc<dyn quarto_system_runtime::SystemRuntime> =
        Arc::new(quarto_system_runtime::NativeRuntime::new());

    let output = render_qmd_to_html(
        qmd.as_bytes(),
        &main_path.to_string_lossy(),
        &mut ctx,
        &HtmlRenderConfig::default(),
        runtime,
    )
    .await
    .expect("render succeeds");
    output.html
}

/// The `id` attributes of every heading section in document order. The
/// HTML writer puts a heading's id on the `<section>` sectionize wraps it in.
fn heading_ids(html: &str) -> Vec<String> {
    html.match_indices("<section id=\"")
        .map(|(pos, tag)| {
            let rest = &html[pos + tag.len()..];
            rest[..rest.find('"').unwrap()].to_string()
        })
        .collect()
}

/// Q1 parity (bd-2wv8431v): the id reflects the shortcode's expansion.
#[tokio::test]
async fn shortcode_expansion_is_folded_into_the_heading_id() {
    let html = render_html("---\nfoo: hello\n---\n\n## before {{< meta foo >}} after\n").await;
    assert_eq!(heading_ids(&html), vec!["before-hello-after"], "{html}");
}

/// A document heading holding an inline lmd cell gets its id from the
/// cell's value, not from the `{lmd} name` placeholder.
#[tokio::test]
async fn inline_lmd_cell_in_a_heading_yields_an_id_from_the_final_text() {
    let qmd = "```{lmd}\nname = 'world'\n```\n\n## Hello, `{lmd} name`\n";
    let html = render_html(qmd).await;
    assert_eq!(heading_ids(&html), vec!["hello-world"], "{html}");
}

/// A fragment used as a component twice yields two distinct ids derived
/// from each instance's text, not two copies of `lmd-title`.
#[tokio::test]
async fn reused_fragment_headings_get_distinct_ids_from_their_text() {
    // `pandoc.Blocks` does not nest, so the cards are appended one by one.
    let qmd = "````{lmd}\nlocal function card(title)\n  return {=:\n### `{lmd} title`\n\nBody.\n:=}\nend\nlocal out = pandoc.Blocks({})\nfor _, t in ipairs({ 'Hello', 'Bye', 'Hello' }) do out:extend(card(t)) end\nreturn out\n````\n";
    let html = render_html(qmd).await;
    assert_eq!(
        heading_ids(&html),
        vec!["hello", "bye", "hello-1"],
        "{html}"
    );
}

/// An explicit id inside a fragment is the author's and survives reuse
/// untouched, duplicates included: that is their call to make.
#[tokio::test]
async fn explicit_ids_inside_fragments_are_kept() {
    let qmd = "```{lmd}\nreturn {=:\n### Pinned {#pinned}\n:=}\n```\n\n## After\n";
    let html = render_html(qmd).await;
    assert_eq!(heading_ids(&html), vec!["pinned", "after"], "{html}");
}
