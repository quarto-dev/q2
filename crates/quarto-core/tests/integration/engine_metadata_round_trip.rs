//! Project metadata on pages that run code (bd-8svy3p89; plan
//! `claude-notes/plans/2026-11-10-engine-metadata-round-trip.md`).
//!
//! The engine stage sends the merged metadata to the engine as qmd front
//! matter and reads the output back. Before the three-way metadata
//! reconcile, the read-back metadata replaced the real metadata: project
//! strings came back as markdown, marked paths lost their project-relative
//! base (their spans lay in the intermediate file, in the document's
//! directory), and `_a_b` read as emphasis. Each test renders a page in
//! `sub/` that runs a cell, through `render_to_file` with a `FixtureEngine`
//! registry, so no Python or R is needed.

#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use quarto_core::engine::{EngineRegistry, FixtureEngine};
use quarto_core::{RenderToFileOptions, RenderToFileResult, render_to_file};
use quarto_system_runtime::NativeRuntime;

const RESULT: &str = "EXECUTED-RESULT-MARKER";
const ENGINE_DOC: &str =
    "---\ntitle: Engine doc\nengine: fixture-a\n---\n\n```{fixture-a}\nx\n```\n";

fn write(dir: &Path, name: &str, contents: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, contents).unwrap();
    path
}

fn render(input: &Path, format: &str) -> Result<RenderToFileResult, String> {
    let mut registry = EngineRegistry::new();
    registry.register(Arc::new(FixtureEngine::with_results(
        "fixture-a",
        vec![RESULT.to_string()],
    )));
    let options = RenderToFileOptions {
        engine_registry_override: Some(Arc::new(registry)),
        quiet: true,
        ..Default::default()
    };
    render_to_file(input, format, &options, Arc::new(NativeRuntime::new()))
        .map_err(|e| format!("{e:?}"))
}

/// Every stylesheet the page links, concatenated.
fn linked_css(result: &RenderToFileResult, html: &str) -> String {
    let page_dir = result.output_path.parent().unwrap();
    html.split("href=\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .filter(|href| href.ends_with(".css"))
        .filter_map(|href| std::fs::read_to_string(page_dir.join(href)).ok())
        .collect()
}

/// bd-08hfh3ut: a project `brand:` file reaches a page in a
/// subdirectory that runs code. The reparsed `brand:` value's span lay in
/// the intermediate (`sub/doc.fixture-a.rmarkdown`), so the brand path
/// resolved against `sub/`.
#[test]
fn project_brand_reaches_an_engine_page_in_a_subdirectory() {
    let temp = tempfile::TempDir::new().unwrap();
    let dir = quarto_system_runtime::canonicalize(temp.path()).unwrap();
    write(
        &dir,
        "_quarto.yml",
        "project:\n  type: default\nbrand: _brand.yml\n",
    );
    write(&dir, "_brand.yml", "color:\n  background: \"#ffffff\"\n");
    let page = write(&dir, "sub/doc.qmd", ENGINE_DOC);

    // Before the fix: Q-14-6, the brand file looked up as `sub/_brand.yml`.
    let result = render(&page, "html").expect("render succeeds");
    let html = std::fs::read_to_string(&result.output_path).unwrap();
    assert!(html.contains(RESULT), "the cell executed");
}

/// bd-bfp32yw9 and the investigation's subdirectory repro: project
/// `include-in-header`, `css:` and a custom theme layer reach a page in a
/// subdirectory that runs code. The header was missing from every engine
/// page; the `css:` link resolved against the document's directory.
#[test]
fn project_format_paths_reach_an_engine_page_in_a_subdirectory() {
    let temp = tempfile::TempDir::new().unwrap();
    let dir = quarto_system_runtime::canonicalize(temp.path()).unwrap();
    write(
        &dir,
        "_quarto.yml",
        "project:\n  type: default\nformat:\n  html:\n    \
         theme: [cosmo, custom.scss]\n    include-in-header: header.html\n    \
         css: styles.css\n",
    );
    write(
        &dir,
        "custom.scss",
        "/*-- scss:rules --*/\n.custom-theme-marker { color: red; }\n",
    );
    write(
        &dir,
        "header.html",
        "<meta name=\"custom-header-marker\">\n",
    );
    write(&dir, "styles.css", ".styles-marker {}\n");
    let page = write(&dir, "sub/doc.qmd", ENGINE_DOC);

    let result = render(&page, "html").expect("render succeeds");
    let html = std::fs::read_to_string(&result.output_path).unwrap();
    assert!(html.contains(RESULT), "the cell executed");
    assert!(
        html.contains("custom-header-marker"),
        "include-in-header reached the page: {html}"
    );
    assert!(
        html.contains("href=\"../styles.css\""),
        "css: resolves against the project root: {html}"
    );
    assert!(
        linked_css(&result, &html).contains("custom-theme-marker"),
        "the custom theme layer reached the theme CSS"
    );
}

/// GH #1152: an extension's theme partial with a leading underscore
/// (`_colors.scss`, rebased to `_extensions/acme/demo/_colors.scss`, two
/// underscores) on a page that runs code.
#[test]
fn extension_theme_partial_reaches_an_engine_page() {
    let temp = tempfile::TempDir::new().unwrap();
    let dir = quarto_system_runtime::canonicalize(temp.path()).unwrap();
    write(
        &dir,
        "_quarto.yml",
        "project:\n  type: default\nformat: demo-html\n",
    );
    write(
        &dir,
        "_extensions/acme/demo/_extension.yml",
        "title: demo\ncontributes:\n  formats:\n    html:\n      \
         theme: [main.scss, _colors.scss]\n",
    );
    write(
        &dir,
        "_extensions/acme/demo/_colors.scss",
        "/*-- scss:defaults --*/\n$accent: #447099;\n",
    );
    write(
        &dir,
        "_extensions/acme/demo/main.scss",
        "/*-- scss:rules --*/\n.accent-marker { color: $accent; }\n",
    );
    let page = write(&dir, "sub/doc.qmd", ENGINE_DOC);

    // Before the fix: Q-14-4 on `extensions/acme/demo/colors.scss`.
    let result = render(&page, "demo-html").expect("render succeeds");
    let html = std::fs::read_to_string(&result.output_path).unwrap();
    assert!(html.contains(RESULT), "the cell executed");
    let css = linked_css(&result, &html);
    assert!(
        css.contains(".accent-marker") && css.contains("#447099"),
        "both theme files reached the theme CSS"
    );
}
