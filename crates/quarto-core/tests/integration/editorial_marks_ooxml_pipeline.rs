/*
 * tests/integration/editorial_marks_ooxml_pipeline.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Document-import P6 (I23): the editorial-marks export transform inside the real docx stage
 * list. In-process, no pandoc subprocess: the stage list is cut after `ast-transforms`, as
 * `pandoc_render_to_file.rs` T3.8 does, and the AST pandoc would receive is inspected.
 */

use std::path::Path;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::ast_walk::for_each_inline_list_mut;
use quarto_core::attribution::PreBuiltAttributionProvider;
use quarto_core::format::{Format, FormatIdentifier};
use quarto_core::pipeline::{build_pandoc_pipeline_stages, run_pipeline};
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};
use quarto_core::stage::PipelineData;
use quarto_pandoc_types::block::Block;
use quarto_pandoc_types::inline::Inline;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

/// Run the docx stage list through `ast-transforms` on `qmd` and return the AST pandoc would
/// receive, with the names of the stages that ran.
async fn ast_for_pandoc(
    dir: &Path,
    qmd: &str,
    provider: Option<Arc<dyn quarto_core::attribution::AttributionSourceProvider>>,
) -> (Pandoc, Vec<String>) {
    let input_path = dir.join("doc.qmd");
    std::fs::write(&input_path, qmd).unwrap();
    let project = ProjectContext {
        dir: dir.to_path_buf(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path(&input_path)],
        output_dir: dir.to_path_buf(),
        ..Default::default()
    };
    let doc = DocumentInfo::from_path(&input_path);
    let format = Format::docx();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
    ctx.attribution_provider = provider;
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());

    let stages = build_pandoc_pipeline_stages(FormatIdentifier::Docx);
    let idx = stages
        .iter()
        .position(|s| s.name() == "ast-transforms")
        .expect("ast-transforms in the pandoc stage list");
    let names: Vec<String> = stages
        .iter()
        .take(idx + 1)
        .map(|s| s.name().to_string())
        .collect();
    let stages: Vec<_> = stages.into_iter().take(idx + 1).collect();
    let content = std::fs::read(&input_path).unwrap();
    let (output, _diagnostics) = run_pipeline(&content, "doc.qmd", &mut ctx, runtime, stages)
        .await
        .expect("pipeline through ast-transforms");
    let PipelineData::DocumentAst(doc) = output else {
        panic!("expected a DocumentAst after ast-transforms");
    };
    (doc.ast, names)
}

/// `(first class, author)` of every span with a classes, anywhere in the document (custom
/// nodes' slots included).
fn spans(ast: &mut Pandoc) -> Vec<(String, Option<String>)> {
    let mut found = Vec::new();
    for_each_inline_list_mut(&mut ast.blocks, &mut |list| {
        for inline in list.iter() {
            if let Inline::Span(s) = inline
                && let Some(first) = s.attr.1.first()
            {
                found.push((first.clone(), s.attr.2.get("author").cloned()));
            }
        }
    });
    found
}

fn count(spans: &[(String, Option<String>)], class: &str) -> usize {
    spans.iter().filter(|(c, _)| c == class).count()
}

const WITH_CONTAINERS: &str = "---\ntitle: T\n---\n\n\
Top [++ added] text.\n\n\
::: {.callout-note}\n## Heads up\n\nIn a callout [-- gone] and [>> a comment].\n:::\n\n\
::: {.panel-tabset}\n## One\n\nIn a tab [!! marked].\n:::\n";

#[tokio::test]
async fn marks_inside_a_callout_and_a_tabset_are_converted_where_pandoc_receives_them() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path().canonicalize().unwrap();
    let (mut ast, _) = ast_for_pandoc(&dir, WITH_CONTAINERS, None).await;

    // The renderer transforms are excluded for docx, so the containers are still custom
    // nodes: the marks inside them must have been reached through the slots.
    let customs = ast
        .blocks
        .iter()
        .filter(|b| matches!(b, Block::Custom(_)))
        .count();
    assert!(
        customs >= 2,
        "callout and tabset should still be custom nodes, got {customs}"
    );

    let found = spans(&mut ast);
    assert_eq!(count(&found, "insertion"), 1, "{found:?}");
    assert_eq!(count(&found, "deletion"), 1, "{found:?}");
    assert_eq!(count(&found, "mark"), 1, "{found:?}");
    assert_eq!(count(&found, "comment-start"), 1, "{found:?}");
    assert_eq!(count(&found, "comment-end"), 1, "{found:?}");
    for leftover in [
        "quarto-insert",
        "quarto-delete",
        "quarto-highlight",
        "quarto-edit-comment",
    ] {
        assert_eq!(
            count(&found, leftover),
            0,
            "{leftover} left behind: {found:?}"
        );
    }
}

#[tokio::test]
async fn the_html_stage_list_leaves_marks_alone() {
    // For contrast: the same document through the HTML stage list keeps its marks, since
    // the transform gates itself on the docx and pptx identifiers.
    let temp = TempDir::new().unwrap();
    let dir = temp.path().canonicalize().unwrap();
    let input_path = dir.join("doc.qmd");
    std::fs::write(&input_path, "Top [++ added] text.\n").unwrap();
    let project = ProjectContext {
        dir: dir.clone(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path(&input_path)],
        output_dir: dir.clone(),
        ..Default::default()
    };
    let doc = DocumentInfo::from_path(&input_path);
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let stages = quarto_core::pipeline::build_html_pipeline_stages();
    let idx = stages
        .iter()
        .position(|s| s.name() == "ast-transforms")
        .expect("ast-transforms");
    let stages: Vec<_> = stages.into_iter().take(idx + 1).collect();
    let content = std::fs::read(&input_path).unwrap();
    let (output, _) = run_pipeline(&content, "doc.qmd", &mut ctx, runtime, stages)
        .await
        .unwrap();
    let PipelineData::DocumentAst(mut doc) = output else {
        panic!("a DocumentAst");
    };
    let found = spans(&mut doc.ast);
    assert_eq!(count(&found, "quarto-insert"), 1, "{found:?}");
    assert_eq!(count(&found, "insertion"), 0, "{found:?}");
}

/// T5's pipeline-level case: the attribution data reaches this transform in a real docx
/// stage list, because `attribution-generate` runs before `ast-transforms`.
#[tokio::test]
async fn attribution_stamps_marks_in_the_real_docx_stage_list() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path().canonicalize().unwrap();
    let qmd = "---\ntitle: T\n---\n\nIt was [++ added] here and [>> why?] too.\n";
    let json = serde_json::json!({
        "runs": [{ "start": 0, "end": qmd.len(), "actor": "alice@example.com", "time": 1_700_000_000 }],
        "identities": { "alice@example.com": { "name": "Alice", "color": "#ff0000" } }
    })
    .to_string();
    let provider: Arc<dyn quarto_core::attribution::AttributionSourceProvider> =
        Arc::new(PreBuiltAttributionProvider::new(json));
    let (mut ast, stage_names) = ast_for_pandoc(&dir, qmd, Some(provider)).await;

    let position = |name: &str| stage_names.iter().position(|n| n == name);
    assert!(
        position("attribution-generate").expect("attribution-generate ran")
            < position("ast-transforms").expect("ast-transforms ran"),
        "stage order: {stage_names:?}"
    );

    let found = spans(&mut ast);
    assert!(
        found.contains(&("insertion".to_string(), Some("Alice".to_string()))),
        "the insertion carries the blamed author: {found:?}"
    );
    assert!(
        found.contains(&("comment-start".to_string(), Some("Alice".to_string()))),
        "the comment carries the blamed author: {found:?}"
    );
}

/// T5's end-to-end case with real blame data: a checked-in `git blame --porcelain` recording
/// through `attribution_from_porcelain`, as `attribution_gitblame.rs` does, so no live git
/// repository is needed. The default identity name is the email's local part.
#[tokio::test]
async fn real_blame_data_stamps_the_local_part_of_the_committers_email() {
    use quarto_core::attribution::attribution_from_porcelain;
    use quarto_core::transform::AstTransform;
    use quarto_core::transforms::EditorialMarksOoxmlTransform;

    // `multi-commit.porcelain` blames a four-line file: lines 1-2 are alice's, 3-4 are bob's.
    let porcelain = include_str!("../fixtures/attribution-blame/multi-commit.porcelain");
    let source = "[-- line1]\n世界\n[++ line3]\n[>> line4]\n";
    let data = attribution_from_porcelain(porcelain, source).expect("blame data");

    let mut sink = Vec::<u8>::new();
    let (mut ast, _ctx, _warnings) =
        pampa::readers::qmd::read(source.as_bytes(), false, "doc.qmd", &mut sink, true, None)
            .expect("parse");

    let temp = TempDir::new().unwrap();
    let dir = temp.path().canonicalize().unwrap();
    let input_path = dir.join("doc.qmd");
    let project = ProjectContext {
        dir: dir.clone(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path(&input_path)],
        output_dir: dir.clone(),
        ..Default::default()
    };
    let doc = DocumentInfo::from_path(&input_path);
    let format = Format::docx();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
    ctx.attribution_data = Some(Arc::new(data));
    EditorialMarksOoxmlTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .unwrap();

    let found = spans(&mut ast);
    assert!(
        found.contains(&("deletion".to_string(), Some("alice".to_string()))),
        "{found:?}"
    );
    assert!(
        found.contains(&("insertion".to_string(), Some("bob".to_string()))),
        "{found:?}"
    );
    assert!(
        found.contains(&("comment-start".to_string(), Some("bob".to_string()))),
        "{found:?}"
    );
}
