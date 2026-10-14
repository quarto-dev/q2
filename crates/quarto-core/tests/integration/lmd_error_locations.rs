/*
 * tests/integration/lmd_error_locations.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * lmd errors point at the right file and line through the real pipeline
 * (bd-lmd-exploration-jqv2nbl4 spike 7), including cells that arrive via
 * `{{< include >}}`.
 */

use std::sync::Arc;

use quarto_core::format::Format;
use quarto_core::pipeline::{HtmlRenderConfig, RenderOutput, render_qmd_to_html};
use quarto_core::project::{DocumentInfo, ProjectConfig, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};

/// Write `files` into a temp dir and render `main` through the real HTML
/// pipeline.
async fn render_fixture(files: &[(&str, &str)], main: &str) -> (RenderOutput, std::path::PathBuf) {
    let tmp = tempfile::TempDir::new().unwrap();
    let project_dir = tmp.path().to_path_buf();
    for (name, content) in files {
        std::fs::write(project_dir.join(name), content).unwrap();
    }
    let main_path = project_dir.join(main);
    let content = std::fs::read(&main_path).unwrap();
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
        &content,
        &main_path.to_string_lossy(),
        &mut ctx,
        &HtmlRenderConfig::default(),
        runtime,
    )
    .await
    .expect("render completes");
    (output, project_dir)
}

/// A cell inside an included file fails: the error names the included
/// file and its line, and the diagnostic is located in that file.
#[tokio::test]
async fn an_error_in_an_included_cell_points_at_the_included_file() {
    let (output, _dir) = render_fixture(
        &[
            ("doc.qmd", "Main.\n\n{{< include inc.qmd >}}\n\nAfter.\n"),
            (
                "inc.qmd",
                "Included.\n\n```{lmd}\nlocal y = 2\nerror('inside include')\n```\n",
            ),
        ],
        "doc.qmd",
    )
    .await;
    let diag = output
        .diagnostics
        .iter()
        .find(|d| d.title.contains("lmd cell failed"))
        .unwrap_or_else(|| panic!("no lmd failure among {:?}", output.diagnostics));
    let problem = format!("{:?}", diag.problem);
    assert!(problem.contains("inc.qmd:5:"), "{problem}");
    assert!(!problem.contains("doc.qmd:"), "{problem}");
    let loc = diag.location.as_ref().expect("located");
    let mapped = loc
        .map_offset(0, &output.source_context)
        .expect("maps into a file");
    let file = output.source_context.get_file(mapped.file_id).unwrap();
    assert!(file.path.ends_with("inc.qmd"), "{}", file.path);
    assert_eq!(mapped.location.row, 4, "{mapped:?}");
}

/// A cell after a plain include in the main file keeps its own line.
#[tokio::test]
async fn a_cell_after_an_include_keeps_its_main_file_line() {
    let (output, _dir) = render_fixture(
        &[
            (
                "doc.qmd",
                "Main.\n\n{{< include inc.qmd >}}\n\n```{lmd}\nerror('in main')\n```\n",
            ),
            ("inc.qmd", "One.\n\nTwo.\n\nThree.\n"),
        ],
        "doc.qmd",
    )
    .await;
    let diag = output
        .diagnostics
        .iter()
        .find(|d| d.title.contains("lmd cell failed"))
        .unwrap_or_else(|| panic!("no lmd failure among {:?}", output.diagnostics));
    let problem = format!("{:?}", diag.problem);
    assert!(problem.contains("doc.qmd:6:"), "{problem}");
    let mapped = diag
        .location
        .as_ref()
        .unwrap()
        .map_offset(0, &output.source_context)
        .unwrap();
    let file = output.source_context.get_file(mapped.file_id).unwrap();
    assert!(file.path.ends_with("doc.qmd"), "{}", file.path);
    assert_eq!(mapped.location.row, 5, "{mapped:?}");
}
