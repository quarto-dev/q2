//! Q-5-37 end-to-end (bd-yl1bpj82): a Q1-style brand file that no
//! `brand:` key references warns per document, decided on the *merged*
//! metadata — so a `brand:` declared in `_metadata.yml`, an active
//! profile, or front matter counts.

#![cfg(not(target_arch = "wasm32"))]

use std::path::Path;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

const BRAND: &str = "color:\n  primary: \"#0066cc\"\n";
const PROJECT: &str = "project:\n  type: default\n";

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// Render `<root>/<doc>` and return how many Q-5-37 diagnostics it carried.
fn q537_count(root: &Path, doc: &str) -> usize {
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result = render_to_file(
        &root.join(doc),
        "html",
        &RenderToFileOptions {
            quiet: true,
            ..Default::default()
        },
        runtime,
    )
    .expect("render_to_file");
    result
        .render_output
        .diagnostics
        .iter()
        .filter(|d| d.code.as_deref() == Some("Q-5-37"))
        .count()
}

fn project(config: &str, brand_file: Option<&str>) -> TempDir {
    let dir = TempDir::new().unwrap();
    write(&dir.path().join("_quarto.yml"), config);
    if let Some(rel) = brand_file {
        write(&dir.path().join(rel), BRAND);
    }
    write(&dir.path().join("index.qmd"), "---\ntitle: T\n---\n\nHi\n");
    dir
}

#[test]
fn warns_for_each_candidate_file_when_nothing_references_it() {
    for file in [
        "_brand.yml",
        "_brand.yaml",
        "_brand/_brand.yml",
        "_brand/_brand.yaml",
    ] {
        let dir = project(PROJECT, Some(file));
        assert_eq!(q537_count(dir.path(), "index.qmd"), 1, "{file}");
    }
}

#[test]
fn silent_without_a_brand_file() {
    let dir = project(PROJECT, None);
    assert_eq!(q537_count(dir.path(), "index.qmd"), 0);
}

#[test]
fn silent_when_quarto_yml_declares_brand() {
    let dir = project(&format!("{PROJECT}brand: _brand.yml\n"), Some("_brand.yml"));
    assert_eq!(q537_count(dir.path(), "index.qmd"), 0);
}

#[test]
fn silent_when_format_html_declares_brand() {
    let dir = project(
        &format!("{PROJECT}format:\n  html:\n    brand: _brand.yml\n"),
        Some("_brand.yml"),
    );
    assert_eq!(q537_count(dir.path(), "index.qmd"), 0);
}

#[test]
fn silent_when_metadata_yml_declares_brand() {
    // The case a project-config-only check would get wrong.
    let dir = project(PROJECT, Some("_brand.yml"));
    write(
        &dir.path().join("docs/_metadata.yml"),
        "brand: _brand.yml\n",
    );
    write(
        &dir.path().join("docs/page.qmd"),
        "---\ntitle: P\n---\n\nHi\n",
    );
    assert_eq!(q537_count(dir.path(), "docs/page.qmd"), 0);
    // A document outside that directory does not inherit it.
    assert_eq!(q537_count(dir.path(), "index.qmd"), 1);
}

#[test]
fn silent_when_front_matter_declares_brand() {
    let dir = project(PROJECT, Some("_brand.yml"));
    write(
        &dir.path().join("index.qmd"),
        "---\ntitle: T\nbrand: _brand.yml\n---\n\nHi\n",
    );
    assert_eq!(q537_count(dir.path(), "index.qmd"), 0);
}

#[test]
fn diagnostics_off_suppresses_it() {
    let dir = project(
        &format!("{PROJECT}diagnostics:\n  Q-5-37: off\n"),
        Some("_brand.yml"),
    );
    assert_eq!(q537_count(dir.path(), "index.qmd"), 0);
}
