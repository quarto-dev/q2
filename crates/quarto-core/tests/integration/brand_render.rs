//! End-to-end render test for `_brand.yml` integration.
//!
//! Drives the real `render_to_file` API against a tempdir-based
//! project that has `_quarto.yml`, `_brand.yml`, and an `index.qmd`,
//! then inspects the produced theme CSS for brand-derived rules.
//!
//! This is the test that would have caught the
//! `CompileThemeCssStage`-doesn't-fire incident (see CLAUDE.md
//! "End-to-end verification") for brand. Pure unit tests in
//! `quarto-sass` can't see whether the pipeline actually wired
//! `ThemeConfig::resolve` correctly; this test can.

#![cfg(not(target_arch = "wasm32"))]

use std::path::Path;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};
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

#[test]
fn brand_yml_renders_palette_and_primary_into_theme_css() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write(
        &root.join("_quarto.yml"),
        "project:\n  type: default\nformat:\n  html:\n    theme:\n      - cosmo\n      - brand\nbrand: _brand.yml\n",
    );
    write(
        &root.join("_brand.yml"),
        "color:\n  palette:\n    brand-blue: \"#0066cc\"\n  primary: brand-blue\n  foreground: \"#222\"\n",
    );
    write(
        &root.join("index.qmd"),
        "---\ntitle: Brand Test\n---\n\n# Hello brand\n",
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
    .expect("render_to_file");

    let css = read_css(&result.resources_dir);
    assert!(
        css.contains("--brand-brand-blue: #0066cc"),
        "expected --brand-brand-blue CSS custom property in:\n{}",
        &css[..css.len().min(500)]
    );
    assert!(
        css.contains("--bs-body-color: #222"),
        "expected `foreground: #222` mapped to --bs-body-color via name map"
    );
    // primary → brand-blue → #0066cc; Bootstrap derives an RGB
    // tuple from $primary which gives us a fingerprint for the
    // resolution chain that's stable across SCSS versions.
    assert!(
        css.contains("0,102,204") || css.contains("0, 102, 204"),
        "expected primary color RGB derivation in CSS"
    );
}

#[test]
fn brand_only_no_theme_key_still_renders_brand_layer() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write(
        &root.join("_quarto.yml"),
        "project:\n  type: default\nbrand: _brand.yml\n",
    );
    write(
        &root.join("_brand.yml"),
        "color:\n  primary: \"#ff6600\"\n  background: \"#fffaf0\"\n",
    );
    write(
        &root.join("doc.qmd"),
        "---\nformat: html\n---\n\n# Brand only\n",
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
    .expect("render_to_file");

    let css = read_css(&result.resources_dir);
    assert!(
        css.contains("#fffaf0"),
        "expected brand background color in CSS"
    );
    assert!(
        css.contains("255,102,0") || css.contains("255, 102, 0"),
        "expected primary color #ff6600 in CSS"
    );
}

/// Q2 is intentionally **stricter** than Q1 about missing brand
/// configuration: `theme: [..., brand, ...]` without a `brand:` key
/// (or a discoverable `_brand.yml` referenced via that key) must be a
/// hard error, not a silent fallback to default CSS.
///
/// Q1 auto-discovers `_brand.yml` from the project root; Q2 doesn't.
/// The reasoning is that an unstyled page is a worse failure mode
/// than a clear "you asked for brand but didn't say where it is"
/// diagnostic — see CLAUDE.md "Debugging Approach" / "End-to-end
/// verification". We can revisit auto-discovery once we have proper
/// source-location diagnostics that point at the offending YAML.
#[test]
fn theme_brand_without_brand_key_fails_loudly() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write(
        &root.join("_quarto.yml"),
        "project:\n  type: default\nformat:\n  html:\n    theme:\n      - cosmo\n      - brand\n",
    );
    // Note: a `_brand.yml` is present on disk, but no `brand:` key
    // points at it. Q2 must refuse to render rather than silently
    // ignore the `brand` token.
    write(&root.join("_brand.yml"), "color:\n  primary: \"#00ff00\"\n");
    write(
        &root.join("doc.qmd"),
        "---\ntitle: Loud Failure\n---\n\n# Hi\n",
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
    );

    let err = result.expect_err("render should fail when `brand` in theme has no `brand:` key");
    let msg = err.to_string();
    assert!(
        msg.to_lowercase().contains("brand"),
        "error message should mention brand, got: {msg}"
    );
}
