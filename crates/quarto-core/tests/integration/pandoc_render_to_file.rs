/*
 * tests/integration/pandoc_render_to_file.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * P7-foundation Task 3 — `render_document_to_file`'s Pandoc(fmt) branch
 * and the stage-position invariant it relies on.
 */

//! `T3.6`, `T3.7`, `T3.8` from the P7-foundation implementation
//! companion.

use std::path::Path;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::pipeline::{build_pandoc_pipeline_stages, run_pipeline};
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};
use quarto_core::render_to_file::{RenderToFileOptions, render_document_to_file};
use quarto_core::stage::PipelineData;
use quarto_pandoc_types::block::Block;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// T3.6 (real pandoc): `render_document_to_file` with a docx `Format`
/// returns `RenderToFileResult.render_output.html.is_empty()` (Finding
/// 3's decision — no binary bytes travel through `content`), an
/// `output_path` ending `.docx`, and a non-empty file on disk. Pairing
/// the emptiness check with the on-disk non-empty check is required by
/// the task's vacuity note: `content.is_empty()` is also what an
/// unimplemented stub would return.
#[test]
fn render_document_to_file_docx_produces_real_output() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().canonicalize().unwrap();
    let input_path = project_dir.join("f.qmd");
    write(&input_path, "---\ntitle: F\n---\n\nHello docx body.\n");

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let options = RenderToFileOptions::default();

    let result = render_document_to_file(
        &input_path,
        "docx",
        &options,
        None,
        runtime,
        None,
        None,
        None,
    )
    .expect("docx render should succeed");

    assert!(
        result.render_output.html.is_empty(),
        "content must stay empty for the Pandoc leg"
    );
    assert_eq!(
        result.output_path.extension().and_then(|e| e.to_str()),
        Some("docx")
    );
    let len = std::fs::metadata(&result.output_path)
        .expect("output file should exist")
        .len();
    assert!(len > 0, "docx output must be non-empty");
}

/// T3.7 (real pandoc): the `OutputSink` / artifact path does not
/// enqueue the Pandoc leg's empty `content` as a destructive write —
/// which would silently truncate the file `PandocWriteStage` already
/// wrote. Guards `render_to_file.rs`'s `if render_format.identifier
/// .is_native()` gate around the `sink.write` call, a different line
/// than T3.6's `Pandoc(fmt)` branch itself.
#[test]
fn render_document_to_file_docx_sink_does_not_truncate_pandoc_output() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().canonicalize().unwrap();
    let input_path = project_dir.join("f.qmd");
    write(&input_path, "---\ntitle: F\n---\n\nHello docx body.\n");

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let options = RenderToFileOptions::default();

    let result = render_document_to_file(
        &input_path,
        "docx",
        &options,
        None,
        runtime,
        None,
        None,
        None,
    )
    .expect("docx render should succeed");

    let bytes = std::fs::read(&result.output_path).expect("output file should be readable");
    assert!(
        !bytes.is_empty(),
        "sink must not overwrite the real pandoc output with zero bytes"
    );
    assert_eq!(
        &bytes[..4],
        b"PK\x03\x04",
        "docx must still be a real zip after the sink runs"
    );
}

/// A relative image reference must resolve against the **document's own
/// directory**, not whatever directory the process happens to have as its
/// cwd. `PandocWriteStage` shells out to a real `pandoc` subprocess to
/// write the docx; pandoc itself (not Q2) reads the image bytes off disk
/// at that point, resolving a relative target against its own cwd unless
/// told otherwise. Reproduced via `cargo run --bin q2 -- render
/// <fixture>/all-docx.qmd --to docx`, which printed `Warning [Q-11-1]:
/// [WARNING] Could not fetch resource img/thinker.jpg: replacing image
/// with description` — the image silently drops out of every real docx
/// render that references one by a relative path, which is the common
/// case.
///
/// Revert hunk: reverting the `--resource-path` argument added to
/// `PandocWriteStage`'s `Command` in `pandoc_write.rs` reddens this test —
/// the output docx would then carry zero `word/media/` entries.
#[test]
fn render_document_to_file_docx_embeds_a_relatively_referenced_image() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().canonicalize().unwrap();

    // A minimal valid 1x1 PNG (the smallest well-known valid PNG byte
    // sequence), placed in a subdirectory so the qmd's `img/dot.png`
    // reference is genuinely relative, not accidentally already at the
    // process cwd.
    const ONE_PIXEL_PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f,
        0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    write(
        &project_dir.join("doc.qmd"),
        "---\ntitle: F\n---\n\n![alt](img/dot.png)\n",
    );
    std::fs::create_dir_all(project_dir.join("img")).unwrap();
    std::fs::write(project_dir.join("img/dot.png"), ONE_PIXEL_PNG).unwrap();

    let input_path = project_dir.join("doc.qmd");
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let options = RenderToFileOptions::default();

    let result = render_document_to_file(
        &input_path,
        "docx",
        &options,
        None,
        runtime,
        None,
        None,
        None,
    )
    .expect("docx render should succeed");

    let bytes = std::fs::read(&result.output_path).expect("output file should be readable");
    let cursor = std::io::Cursor::new(bytes);
    let zip = zip::ZipArchive::new(cursor).expect("output should be a valid ZIP archive");
    let media_entries: Vec<&str> = zip
        .file_names()
        .filter(|name| name.starts_with("word/media/"))
        .collect();
    assert!(
        !media_entries.is_empty(),
        "expected the relatively-referenced image to be embedded under word/media/, \
         found no such entry among: {:?}",
        zip.file_names().collect::<Vec<_>>()
    );
}

/// Walk a document's top-level blocks for any `Block::Custom` (the
/// typed AST variant `AstTransformsStage` creates when it desugars a
/// callout div — see `crates/quarto-pandoc-types/src/custom.rs`).
fn has_custom_node(blocks: &[Block]) -> bool {
    blocks.iter().any(|b| matches!(b, Block::Custom(_)))
}

/// Build a `RenderContext` + minimal single-file `ProjectContext` for a
/// docx-target callout fixture, shared by both halves of T3.8.
fn callout_fixture_ctx(project_dir: &Path) -> (ProjectContext, DocumentInfo, Format) {
    let input_path = project_dir.join("callout.qmd");
    write(
        &input_path,
        "---\ntitle: T\n---\n\n::: {#nte-a .callout-note}\n## Heads up\n\nBody.\n:::\n",
    );
    let project = ProjectContext {
        dir: project_dir.to_path_buf(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path(&input_path)],
        output_dir: project_dir.to_path_buf(),
        ..Default::default()
    };
    let doc = DocumentInfo::from_path(&input_path);
    (project, doc, Format::docx())
}

/// T3.8 (in-process, no pandoc subprocess needed — truncates the stage
/// list before `PandocWriteStage`): for a `Pandoc("docx")` render,
/// `UserFiltersStage::pre()` observes an AST with **no** `Block::Custom`
/// (transforms haven't run yet), and the AST immediately after
/// `AstTransformsStage` (before `UserFiltersStage::post()`) **does**
/// contain one — the callout survives as a typed `Custom` node into the
/// `post` filter position, unlike native HTML.
///
/// Revert hunk: reordering `UserFiltersStage::post()` to run *before*
/// `AstTransformsStage` would make the post-position assertion
/// (currently checking the AST right after `AstTransformsStage`) find
/// no `Block::Custom` either, reddening this test.
#[tokio::test]
async fn user_filters_pre_sees_no_custom_node_post_position_does() {
    let stages = build_pandoc_pipeline_stages();
    let pre_idx = stages
        .iter()
        .position(|s| s.name() == "user-filters-pre")
        .expect("user-filters-pre must exist in the pandoc stage list");
    let ast_transforms_idx = stages
        .iter()
        .position(|s| s.name() == "ast-transforms")
        .expect("ast-transforms must exist in the pandoc stage list");
    assert!(
        pre_idx < ast_transforms_idx,
        "user-filters-pre must run before ast-transforms"
    );

    // --- Pre position: run everything through user-filters-pre. ---
    {
        let temp = TempDir::new().unwrap();
        let project_dir = temp.path().canonicalize().unwrap();
        let (project, doc, format) = callout_fixture_ctx(&project_dir);
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        let content = std::fs::read(&doc.input).unwrap();

        let pre_stages: Vec<_> = build_pandoc_pipeline_stages()
            .into_iter()
            .take(pre_idx + 1)
            .collect();
        let (output, _diags) = run_pipeline(&content, "callout.qmd", &mut ctx, runtime, pre_stages)
            .await
            .expect("pipeline through user-filters-pre should succeed");
        let PipelineData::DocumentAst(pre_doc) = output else {
            panic!("expected DocumentAst at the pre position");
        };
        assert!(
            !has_custom_node(&pre_doc.ast.blocks),
            "pre position must not see a Custom node yet"
        );
    }

    // --- Post position: run everything through ast-transforms, one
    // stage further than the pre check, stopping before
    // user-filters-post runs. ---
    {
        let temp = TempDir::new().unwrap();
        let project_dir = temp.path().canonicalize().unwrap();
        let (project, doc, format) = callout_fixture_ctx(&project_dir);
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        let content = std::fs::read(&doc.input).unwrap();

        let post_stages: Vec<_> = build_pandoc_pipeline_stages()
            .into_iter()
            .take(ast_transforms_idx + 1)
            .collect();
        let (output, _diags) =
            run_pipeline(&content, "callout.qmd", &mut ctx, runtime, post_stages)
                .await
                .expect("pipeline through ast-transforms should succeed");
        let PipelineData::DocumentAst(post_doc) = output else {
            panic!("expected DocumentAst right after ast-transforms");
        };
        assert!(
            has_custom_node(&post_doc.ast.blocks),
            "the callout must still be a Custom node right after ast-transforms, \
             for a Pandoc(fmt) render"
        );
    }
}
