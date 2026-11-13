//! The directory a document's code runs in (GH #1202, bd-tkqk1vwz).
//!
//! Quarto 1 runs a project document's cells in the document's own
//! directory, and in the project root only under `project: execute-dir:
//! project`. Driven through `render_to_file` with a file-backed
//! `FixtureEngine`, which reads its `results` file relative to the
//! execution `cwd`: each candidate directory holds a results file with a
//! different marker, so the rendered HTML names the directory that won.

#![cfg(not(target_arch = "wasm32"))]

use std::path::Path;
use std::sync::Arc;

use quarto_core::engine::{EngineRegistry, FixtureEngine};
use quarto_core::{RenderToFileOptions, render_to_file};
use quarto_system_runtime::NativeRuntime;

const PAGE: &str = "---\ntitle: Page\nengine:\n  fixture-a:\n    results: !str results.json\n---\n\n```{fixture-a}\nseed-cell-source\n```\n";

fn render_page(project_yml: &str) -> String {
    let temp = tempfile::TempDir::new().unwrap();
    let root = quarto_system_runtime::canonicalize(temp.path()).unwrap();
    let sub = root.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(root.join("_quarto.yml"), project_yml).unwrap();
    std::fs::write(root.join("results.json"), r#"["RAN-IN-PROJECT-ROOT"]"#).unwrap();
    std::fs::write(sub.join("results.json"), r#"["RAN-IN-DOCUMENT-DIR"]"#).unwrap();
    let page = sub.join("page.qmd");
    std::fs::write(&page, PAGE).unwrap();

    let mut registry = EngineRegistry::new();
    registry.register(Arc::new(FixtureEngine::new("fixture-a")));
    let options = RenderToFileOptions {
        engine_registry_override: Some(Arc::new(registry)),
        quiet: true,
        ..Default::default()
    };
    let result = render_to_file(&page, "html", &options, Arc::new(NativeRuntime::new()))
        .expect("render succeeds");
    read(&result.output_path)
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).expect("output written")
}

#[test]
fn project_document_runs_in_its_own_directory_by_default() {
    let html = render_page("project:\n  type: default\n");
    assert!(html.contains("RAN-IN-DOCUMENT-DIR"), "{html}");
}

#[test]
fn execute_dir_file_runs_in_the_documents_directory() {
    let html = render_page("project:\n  type: default\n  execute-dir: file\n");
    assert!(html.contains("RAN-IN-DOCUMENT-DIR"), "{html}");
}

#[test]
fn execute_dir_project_runs_in_the_project_root() {
    let html = render_page("project:\n  type: default\n  execute-dir: project\n");
    assert!(html.contains("RAN-IN-PROJECT-ROOT"), "{html}");
}
