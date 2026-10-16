/*
 * tests/integration/render_overlay.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Format-resolution epic F1 (FR12): a `RenderOverlay` applied by the code
 * that creates a render's runtime reaches every consumer of the runtime
 * layer, and the pipeline entries never wrap it themselves.
 */

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use quarto_core::engine::ExecutionPolicy;
use quarto_core::format::Format;
use quarto_core::overlay_runtime::RenderOverlay;
use quarto_core::project::ProjectContext;
use quarto_core::render_chapter_alone;
use quarto_core::render_to_file::{RenderToFileOptions, TypstTarget, render_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};
use serde_json::json;
use tempfile::TempDir;

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn root_of(tmp: &TempDir) -> PathBuf {
    tmp.path().canonicalize().unwrap()
}

fn overlaid(overlay: RenderOverlay) -> Arc<dyn SystemRuntime> {
    Arc::new(overlay).apply(Arc::new(NativeRuntime::new()))
}

fn render_html(input: &Path, runtime: Arc<dyn SystemRuntime>) -> String {
    let options = RenderToFileOptions {
        quiet: true,
        ..Default::default()
    };
    let result = render_to_file(input, "html", &options, runtime).expect("render");
    std::fs::read_to_string(result.output_path).unwrap()
}

const DOC: &str = "---\ntitle: Document title\n---\n\n## Section\n\nBody.\n";

#[test]
fn overlay_title_beats_document_and_project_titles() {
    let tmp = TempDir::new().unwrap();
    let root = root_of(&tmp);
    write(
        &root.join("_quarto.yml"),
        "project:\n  title: Project title\n",
    );
    write(&root.join("a.qmd"), DOC);

    let plain = render_html(&root.join("a.qmd"), Arc::new(NativeRuntime::new()));
    assert!(plain.contains("<title>Document title"), "{plain}");

    let html = render_html(
        &root.join("a.qmd"),
        overlaid(RenderOverlay::with_metadata(
            json!({"title": "Overlay title"}),
        )),
    );
    assert!(html.contains("<title>Overlay title"), "{html}");
    assert!(!html.contains("Document title"));
}

#[test]
fn overlay_format_map_is_flattened_for_the_format() {
    let tmp = TempDir::new().unwrap();
    let root = root_of(&tmp);
    write(&root.join("a.qmd"), DOC);

    let plain = render_html(&root.join("a.qmd"), Arc::new(NativeRuntime::new()));
    assert!(!plain.contains("id=\"TOC\"") && !plain.contains("quarto-toc"));

    let html = render_html(
        &root.join("a.qmd"),
        overlaid(RenderOverlay::with_metadata(
            json!({"format": {"html": {"toc": true}}}),
        )),
    );
    assert!(
        html.contains("id=\"TOC\"") || html.contains("quarto-toc") || html.contains("toc-"),
        "no table of contents in:\n{html}"
    );
}

#[test]
fn overlay_file_is_read_in_place_of_the_disk_file() {
    let tmp = TempDir::new().unwrap();
    let root = root_of(&tmp);
    write(&root.join("a.qmd"), DOC);
    let mut files = HashMap::new();
    files.insert(
        root.join("a.qmd"),
        b"---\ntitle: From the overlay\n---\n\nEdited.\n".to_vec(),
    );
    let html = render_html(
        &root.join("a.qmd"),
        overlaid(RenderOverlay::with_files(files)),
    );
    assert!(html.contains("<title>From the overlay"), "{html}");
    assert!(html.contains("Edited."));
    // The disk file is untouched.
    assert!(
        std::fs::read_to_string(root.join("a.qmd"))
            .unwrap()
            .contains("Document title")
    );
}

#[test]
fn the_nested_chapter_alone_pipeline_sees_the_overlay() {
    let tmp = TempDir::new().unwrap();
    let root = root_of(&tmp);
    write(
        &root.join("_quarto.yml"),
        "project:\n  type: book\nbook:\n  title: B\n  author: A\n  chapters:\n    - index.qmd\n    - one.qmd\n",
    );
    write(&root.join("index.qmd"), "---\ntitle: Index\n---\n\nHome.\n");
    write(
        &root.join("one.qmd"),
        "---\ntitle: One\n---\n\nChapter one.\n",
    );

    let runtime = overlaid(RenderOverlay::with_metadata(json!({"subtitle": "ZZSUB"})));
    let project = ProjectContext::discover(&root, runtime.as_ref()).unwrap();
    let options = RenderToFileOptions {
        execution_policy: ExecutionPolicy::None,
        typst_target: Some(TypstTarget::Source),
        ..Default::default()
    };
    let format = Format::from_format_string("typst").unwrap();
    let result = pollster::block_on(render_chapter_alone(
        &project,
        &root.join("one.qmd"),
        &format,
        &options,
        runtime,
    ))
    .expect("chapter render");
    let typ = result.output_path.with_extension("typ");
    let source = std::fs::read_to_string(&typ).unwrap();
    assert!(source.contains("ZZSUB"), "{source}");
}

/// `ProjectPipeline::new`, `render_to_file` and `render_document_to_file`
/// never wrap the runtime: the creator of the runtime does, once (F1-D4).
/// A source audit, since a wrap there would be invisible to behaviour (the
/// merge is idempotent) and so cannot be caught by a result test.
#[test]
fn pipeline_entries_never_apply_an_overlay() {
    let sources = [
        (
            "render_to_file.rs",
            include_str!("../../src/render_to_file.rs"),
        ),
        (
            "project/orchestrator.rs",
            include_str!("../../src/project/orchestrator.rs"),
        ),
        (
            "project/book/chapter.rs",
            include_str!("../../src/project/book/chapter.rs"),
        ),
        (
            "project/pass2_renderer.rs",
            include_str!("../../src/project/pass2_renderer.rs"),
        ),
    ];
    for (name, src) in sources {
        // Only the production half of the file: tests may build overlays.
        let production = src.split("#[cfg(test)]").next().unwrap();
        for needle in [".apply(", "options.overlay", "OverlayRuntime"] {
            // `.apply(` also matches unrelated methods; narrow it to overlays.
            let hit = if needle == ".apply(" {
                production
                    .lines()
                    .any(|l| l.contains(".apply(") && l.to_lowercase().contains("overlay"))
            } else {
                production.contains(needle)
            };
            assert!(!hit, "{name} must not wrap the runtime ({needle})");
        }
    }
}
