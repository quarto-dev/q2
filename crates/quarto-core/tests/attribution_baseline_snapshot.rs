//! Phase 0 — HTML off-path baseline snapshot.
//!
//! Pins the rendered HTML body of a small attribution-free document
//! so that any unintended change to the writer (e.g. accidentally
//! emitting `data-attr-*` when no provider is installed) shows up
//! immediately as a snapshot diff.
//!
//! This test is **GREEN immediately and stays green**: it asserts
//! existing behaviour and is the regression guard the plan's "byte-
//! identical when off" promise leans on. As Phase 4c lands, the
//! attribution-render transform will be registered in the pipeline,
//! but the off-path (no provider installed) must continue to produce
//! exactly this snapshot.

use std::sync::Arc;

use quarto_core::pipeline::{HtmlRenderConfig, render_qmd_to_html};
use quarto_core::project::{DocumentInfo, ProjectConfig, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};
use quarto_core::{Format, QuartoError};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

const FIXTURE: &str = "# Hello, world\n\nThis is a paragraph.\n";

#[tokio::test]
async fn attribution_off_html_baseline() -> Result<(), QuartoError> {
    let dir = std::env::temp_dir().join("attribution-baseline-snapshot");
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let qmd_path = dir.join("doc.qmd");
    std::fs::write(&qmd_path, FIXTURE).expect("write fixture");

    let project = ProjectContext {
        dir: dir.clone(),
        config: ProjectConfig::default(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path(qmd_path.clone())],
        output_dir: dir.clone(),
    };
    let doc = DocumentInfo::from_path(qmd_path.clone());
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let config = HtmlRenderConfig::default();

    let output = render_qmd_to_html(
        FIXTURE.as_bytes(),
        &qmd_path.to_string_lossy(),
        &mut ctx,
        &config,
        runtime,
    )
    .await?;

    // We snapshot just the body — the surrounding template includes
    // many platform-dependent paths (CSS hash filenames, dist
    // directories) that would create noisy diffs.
    let body_marker = output.html.find("<body").expect("<body marker");
    let body_close = output.html.find("</body>").expect("</body marker");
    let body = &output.html[body_marker..body_close + "</body>".len()];

    // Sanity: in the off-path, the body must NOT contain any
    // attribution-related markup. This is the property the snapshot
    // re-asserts mechanically across the corpus.
    assert!(
        !body.contains("data-attr-"),
        "off-path HTML must contain no data-attr-* attributes; body:\n{body}"
    );

    insta::assert_snapshot!("attribution_off_baseline", body);
    Ok(())
}
