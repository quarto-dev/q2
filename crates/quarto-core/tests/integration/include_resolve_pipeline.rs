/*
 * tests/include_resolve_pipeline.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Pipeline integration tests for IncludeResolveStage.
 */

//! End-to-end integration tests for `include-in-header` /
//! `include-before-body` / `include-after-body` (`bd-8kp3`).
//!
//! Two contracts are pinned here:
//!
//! 1. **File-slot include dependencies land in `profile.includes`.**
//!    `IncludeResolveStage` runs before `DocumentProfileStage`, so any
//!    file referenced by `include-in-header: foo.html` is recorded
//!    on the AST's `recorded_includes` side-channel and drained into
//!    the profile by `DocumentProfileStage`. `bd-r82e` cache
//!    invalidation hashes `profile.includes`, so a change to `foo.html`
//!    triggers a re-render.
//!
//! 2. **All three slots reach the rendered HTML.**
//!    A real CLI-equivalent render (`render_qmd_to_html`) of a
//!    fixture exercising bare-string paths, `{file:..}`, and
//!    `{text:..}` smart-include forms produces HTML where each
//!    include appears in the right slot (inside `<head>`,
//!    immediately after `<body>`, immediately before `</body>`).

use std::path::PathBuf;
use std::sync::Arc;

use quarto_core::format::Format;
use quarto_core::pipeline::{HtmlRenderConfig, build_html_pipeline_stages, render_qmd_to_html};
use quarto_core::project::{DocumentInfo, ProjectConfig, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};
use quarto_core::stage::{
    LoadedSource, Pipeline, PipelineData, PipelineDataKind, PipelineStage, StageContext,
};

fn position_of(stages: &[Box<dyn PipelineStage>], target: &str) -> Option<usize> {
    stages.iter().position(|s| s.name() == target)
}

fn make_project(dir: &std::path::Path) -> ProjectContext {
    ProjectContext {
        dir: dir.to_path_buf(),
        config: ProjectConfig::default(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path(dir.join("test.qmd"))],
        output_dir: dir.to_path_buf(),

        ..Default::default()
    }
}

fn make_document(dir: &std::path::Path) -> DocumentInfo {
    DocumentInfo::from_path(dir.join("test.qmd"))
}

fn make_stage_context(dir: &std::path::Path) -> StageContext {
    let runtime = Arc::new(quarto_system_runtime::NativeRuntime::new());
    let format = Format::html();
    StageContext::new(runtime, format, make_project(dir), make_document(dir))
        .expect("stage context")
}

/// Run the pipeline up to and including `DocumentProfileStage`,
/// returning the `DocumentAtProfile`. The profile's `includes` set
/// here reflects whatever `IncludeResolveStage` recorded.
async fn run_head_pipeline(
    qmd_path: &std::path::Path,
    content: &[u8],
    project_dir: &std::path::Path,
) -> quarto_core::stage::DocumentAtProfile {
    let full = build_html_pipeline_stages();
    let checkpoint =
        position_of(&full, "document-profile").expect("document-profile stage present");
    let head: Vec<Box<dyn PipelineStage>> = full.into_iter().take(checkpoint + 1).collect();

    let pipeline = Pipeline::new(head).expect("head pipeline valid");
    let mut ctx = make_stage_context(project_dir);
    let input =
        PipelineData::LoadedSource(LoadedSource::new(qmd_path.to_path_buf(), content.to_vec()));
    let out = pipeline
        .run(input, &mut ctx)
        .await
        .expect("head pipeline run");
    assert_eq!(out.kind(), PipelineDataKind::AtProfile);
    out.into_at_profile().unwrap()
}

#[tokio::test]
async fn file_slot_include_lands_in_profile_includes() {
    // Real on-disk fixture: a project dir containing an extra.html
    // referenced by `include-in-header`. The runtime can canonicalize
    // and read it.
    let tmp = tempfile::TempDir::new().unwrap();
    let project_dir = tmp.path().to_path_buf();

    let extra_html = "<style>span.x { color: red; }</style>\n";
    std::fs::write(project_dir.join("extra.html"), extra_html).unwrap();

    let qmd = "---\n\
         title: With include\n\
         include-in-header: extra.html\n\
         ---\n\
         \n\
         Body.\n"
        .to_string();
    let qmd_path = project_dir.join("test.qmd");
    std::fs::write(&qmd_path, &qmd).unwrap();

    let bundle = run_head_pipeline(&qmd_path, qmd.as_bytes(), &project_dir).await;
    let profile = &bundle.profile;

    assert_eq!(
        profile.includes.len(),
        1,
        "file-slot include must reach profile.includes for cache-key invalidation; got {:?}",
        profile.includes
    );

    // The recorded path is a canonicalized absolute reference to
    // extra.html. We don't pin the exact form (canonicalize differs
    // per platform for tmp paths) but it must end with the file
    // name the user wrote.
    let entry = &profile.includes[0];
    assert!(
        entry
            .path
            .file_name()
            .is_some_and(|n| n == std::ffi::OsStr::new("extra.html")),
        "expected entry for extra.html, got {:?}",
        entry.path
    );

    // The hash matches the bytes we wrote. Any change to extra.html
    // triggers a different hash, which triggers cache invalidation.
    use quarto_core::document_profile::IncludeEntry;
    assert_eq!(
        entry.content_hash,
        IncludeEntry::hash_bytes(extra_html.as_bytes())
    );
}

#[tokio::test]
async fn three_slots_reach_rendered_html_via_full_pipeline() {
    // Fixture with all three keys exercising all three forms:
    //
    //   include-in-header:    bare string path
    //   include-before-body:  {file: ...}     object form
    //   include-after-body:   {text: ...}     literal form
    let tmp = tempfile::TempDir::new().unwrap();
    let project_dir = tmp.path().to_path_buf();

    std::fs::write(
        project_dir.join("head.html"),
        "<meta name=\"q2-include\" content=\"in-head\">\n",
    )
    .unwrap();
    std::fs::write(
        project_dir.join("before.html"),
        "<aside class=\"q2-banner\">BEFORE</aside>\n",
    )
    .unwrap();

    let qmd = "---\n\
        title: Includes Smoke\n\
        include-in-header: head.html\n\
        include-before-body:\n  file: before.html\n\
        include-after-body:\n  text: \"<script>console.log('AFTER')</script>\"\n\
        ---\n\
        \n\
        Body.\n";
    let qmd_path = project_dir.join("test.qmd");
    std::fs::write(&qmd_path, qmd).unwrap();

    let project = make_project(&project_dir);
    let doc = make_document(&project_dir);
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

    let runtime: Arc<dyn quarto_system_runtime::SystemRuntime> =
        Arc::new(quarto_system_runtime::NativeRuntime::new());
    let config = HtmlRenderConfig::default();

    let output = render_qmd_to_html(
        qmd.as_bytes(),
        &qmd_path.to_string_lossy(),
        &mut ctx,
        &config,
        runtime,
    )
    .await
    .expect("render");
    let html = output.html;

    // Slot 1: in-header content lives inside <head>.
    let head_close = html.find("</head>").expect("</head>");
    let in_header = html
        .find("<meta name=\"q2-include\"")
        .expect("in-header content reaches output");
    assert!(
        in_header < head_close,
        "include-in-header must appear inside <head>:\n{}",
        html
    );

    // Slot 2: before-body content sits between <body> and the body
    // text. Its position must precede "Body.".
    let body_text = html.find("Body.").expect("body text");
    let before_body = html.find("BEFORE").expect("before-body content");
    assert!(
        before_body < body_text,
        "include-before-body must appear before body text:\n{}",
        html
    );

    // Slot 3: after-body content sits after the body text and
    // before </body>.
    let after_body = html.find("AFTER").expect("after-body content");
    let body_close = html.find("</body>").expect("</body>");
    assert!(
        body_text < after_body && after_body < body_close,
        "include-after-body must appear after body text and before </body>:\n{}",
        html
    );
}

/// `text:` holding block-level markdown — a fenced ```{=html} block
/// or two HTML paragraphs — must reach `<head>` (bd-include-in-header-text-blocks-ins2v6za).
/// Before the fix both spellings were dropped with Q-5-5.
#[tokio::test]
async fn text_block_markdown_reaches_head_via_full_pipeline() {
    let tmp = tempfile::TempDir::new().unwrap();
    let project_dir = tmp.path().to_path_buf();

    // A raw string, not `\`-continued lines: the block scalar's
    // indentation is significant.
    let qmd = r#"---
title: Block Includes
include-in-header:
  - text: |
      ```{=html}
      <style type="text/css">
        .marker-fence { color: rebeccapurple; }
      </style>
      ```
  - text: |
      <meta name="marker-para-one" content="1">

      <meta name="marker-para-two" content="2">
---

Body.
"#;
    let qmd_path = project_dir.join("test.qmd");
    std::fs::write(&qmd_path, qmd).unwrap();

    let project = make_project(&project_dir);
    let doc = make_document(&project_dir);
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

    let runtime: Arc<dyn quarto_system_runtime::SystemRuntime> =
        Arc::new(quarto_system_runtime::NativeRuntime::new());
    let config = HtmlRenderConfig::default();

    let output = render_qmd_to_html(
        qmd.as_bytes(),
        &qmd_path.to_string_lossy(),
        &mut ctx,
        &config,
        runtime,
    )
    .await
    .expect("render");
    let html = output.html;

    let head_close = html.find("</head>").expect("</head>");
    for marker in ["marker-fence", "marker-para-one", "marker-para-two"] {
        let pos = html
            .find(marker)
            .unwrap_or_else(|| panic!("{marker} must reach the output:\n{html}"));
        assert!(pos < head_close, "{marker} must be inside <head>:\n{html}");
    }
    let fence_open = html.find("```");
    assert!(
        fence_open.is_none(),
        "fence markers must not leak into the output:\n{html}"
    );
}

// Suppress unused-warning when only one test runs.
#[allow(dead_code)]
fn _silence_unused() {
    let _ = PathBuf::new();
}
