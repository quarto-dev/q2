//! End-to-end render tests for the Q1 light/dark theme map form
//! (bd-o76p01wb interim behavior).
//!
//! `theme: {light: […], dark: […]}` must render using only the
//! `light:` half and emit a single Q-14-3 warning that the `dark:`
//! half is ignored. Full dual-theme support (compile both variants +
//! toggle) is tracked separately (bd-0pic6).
//!
//! Drives the real `render_to_file` API (see CLAUDE.md "End-to-end
//! verification") against tempdir-based projects, then inspects both
//! the produced CSS and the render diagnostics.

#![cfg(not(target_arch = "wasm32"))]

use std::path::Path;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};
use quarto_error_reporting::DiagnosticKind;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn read_css(resources_dir: &Path) -> String {
    let mut combined = String::new();
    for entry in walkdir::WalkDir::new(resources_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) == Some("css") {
            combined.push_str(&std::fs::read_to_string(p).unwrap());
            combined.push('\n');
        }
    }
    combined
}

const LIGHT_SCSS: &str = "/*-- scss:rules --*/\n.q-light-marker { color: #123456; }\n";
const DARK_SCSS: &str = "/*-- scss:rules --*/\n.q-dark-marker { color: #654321; }\n";

/// Project-config path: the map form in `_quarto.yml`
/// (`format.html.theme`), the way the posit-docs extension ships it.
#[test]
fn project_theme_light_dark_map_renders_light_half_with_q_14_3_warning() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write(
        &root.join("_quarto.yml"),
        "project:\n  type: default\nformat:\n  html:\n    theme:\n      light: [cosmo, light-marker.scss]\n      dark: [darkly, dark-marker.scss]\n",
    );
    write(&root.join("light-marker.scss"), LIGHT_SCSS);
    write(&root.join("dark-marker.scss"), DARK_SCSS);
    write(
        &root.join("index.qmd"),
        "---\ntitle: Light/Dark Map\n---\n\n# Hello\n",
    );

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result = render_to_file(
        &root.join("index.qmd"),
        "html",
        &RenderToFileOptions {
            quiet: true,
            ..Default::default()
        },
        runtime,
    )
    .expect("light/dark theme map must render, not fail with Q-14-1");

    // Only the light half is compiled into the page CSS.
    let css = read_css(&result.resources_dir);
    assert!(
        css.contains(".q-light-marker"),
        "light half's custom SCSS must be in the compiled CSS"
    );
    assert!(
        !css.contains(".q-dark-marker"),
        "dark half must be ignored in the interim behavior"
    );

    // The degradation is loud: exactly one Q-14-3 warning.
    let diagnostics = &result.render_output.diagnostics;
    let q14_3: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code.as_deref() == Some("Q-14-3"))
        .collect();
    assert_eq!(
        q14_3.len(),
        1,
        "expected exactly one Q-14-3 warning, diagnostics: {:?}",
        diagnostics
    );
    assert_eq!(q14_3[0].kind, DiagnosticKind::Warning);
    assert!(
        q14_3[0].location.is_some(),
        "Q-14-3 should carry a source location pointing at the dark entry"
    );
}

/// Document-frontmatter path: the map form in the document's own
/// `format.html.theme` (single-file render, no `_quarto.yml`).
#[test]
fn frontmatter_theme_light_dark_map_renders_light_half_with_q_14_3_warning() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write(&root.join("light-marker.scss"), LIGHT_SCSS);
    write(&root.join("dark-marker.scss"), DARK_SCSS);
    write(
        &root.join("doc.qmd"),
        "---\ntitle: Frontmatter Map\nformat:\n  html:\n    theme:\n      light: [cosmo, light-marker.scss]\n      dark: [dark-marker.scss]\n---\n\n# Hi\n",
    );

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result = render_to_file(
        &root.join("doc.qmd"),
        "html",
        &RenderToFileOptions {
            quiet: true,
            ..Default::default()
        },
        runtime,
    )
    .expect("frontmatter light/dark theme map must render");

    let css = read_css(&result.resources_dir);
    assert!(css.contains(".q-light-marker"));
    assert!(!css.contains(".q-dark-marker"));

    let q14_3: Vec<_> = result
        .render_output
        .diagnostics
        .iter()
        .filter(|d| d.code.as_deref() == Some("Q-14-3"))
        .collect();
    assert_eq!(q14_3.len(), 1, "expected exactly one Q-14-3 warning");
}

/// A light-only map is a fully-honored (if redundant) spelling — no
/// warning (D6 in the plan).
#[test]
fn light_only_theme_map_renders_without_warning() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write(
        &root.join("_quarto.yml"),
        "project:\n  type: default\nformat:\n  html:\n    theme:\n      light: [cosmo, light-marker.scss]\n",
    );
    write(&root.join("light-marker.scss"), LIGHT_SCSS);
    write(
        &root.join("index.qmd"),
        "---\ntitle: Light Only\n---\n\n# Hello\n",
    );

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result = render_to_file(
        &root.join("index.qmd"),
        "html",
        &RenderToFileOptions {
            quiet: true,
            ..Default::default()
        },
        runtime,
    )
    .expect("light-only theme map must render");

    let css = read_css(&result.resources_dir);
    assert!(css.contains(".q-light-marker"));

    assert!(
        !result
            .render_output
            .diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some("Q-14-3")),
        "light-only map ignores nothing, so it must not warn"
    );
}
