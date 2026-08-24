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

// ── unified light/dark brand (bd-unified-brand-split-ep49amad, GH #580) ──

/// Collect `(path, contents)` for every CSS file under the resources
/// dir, so assertions can distinguish which *file* carries a value.
fn css_files_by_path(resources_dir: &Path) -> Vec<(std::path::PathBuf, String)> {
    walkdir::WalkDir::new(resources_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("css"))
        .map(|e| {
            let p = e.path().to_path_buf();
            let contents = std::fs::read_to_string(&p).unwrap();
            (p, contents)
        })
        .collect()
}

/// The exact reproduction from GH #580: a single-file render whose
/// front matter names a `_brand.yml` that uses per-color `{light:,
/// dark:}` values. The light value must land in the light stylesheet
/// and the dark value in the dark one, matching what the two-file
/// `brand: {light:, dark:}` form already does.
#[test]
fn unified_brand_light_dark_renders_both_stylesheets() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write(
        &root.join("_brand.yml"),
        "color:\n  background:\n    light: \"#b22221\"\n    dark: \"#22b221\"\n",
    );
    write(
        &root.join("index.qmd"),
        "---\nformat: html\nbrand: _brand.yml\n---\n\nHello.\n",
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
    .expect("a unified light/dark brand must render (GH #580)");

    let files = css_files_by_path(&result.resources_dir);
    let light = files
        .iter()
        .find(|(_, css)| css.contains("--bs-body-bg: #b22221"))
        .expect("some CSS file must carry the light background");
    let dark = files
        .iter()
        .find(|(_, css)| css.contains("--bs-body-bg: #22b221"))
        .expect("some CSS file must carry the dark background");
    assert_ne!(
        light.0, dark.0,
        "light and dark variants must be separate CSS files"
    );

    // The dual-variant machinery must fully engage: attributed links,
    // the trailing light-copy link (light is the author default for a
    // unified brand), and the color-mode runtime.
    let html = std::fs::read_to_string(&result.output_path).unwrap();
    assert!(
        html.contains(r#"class="quarto-color-scheme" id="quarto-bootstrap""#),
        "attributed light link must be present"
    );
    assert!(
        html.contains(r#"class="quarto-color-scheme quarto-color-alternate""#),
        "attributed dark (alternate) link must be present"
    );
    assert!(
        html.contains(r#"class="quarto-color-scheme-extra""#),
        "light is the author default → trailing light-copy link"
    );
    assert!(
        html.contains(r#"<meta name="color-scheme" content="light">"#),
        "author-default-light meta hint"
    );
    assert!(
        html.contains(r#"data-author-prefers-dark="false""#),
        "color-mode script must be configured for light default"
    );
}

/// Project-config shape from docs/guides/authoring/brand.qmd: the
/// brand is named in `_quarto.yml`, and typography colors carry
/// pairs too. Each hex must land only in its own variant's CSS.
#[test]
fn unified_brand_typography_colors_split_per_variant() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write(
        &root.join("_quarto.yml"),
        "project:\n  type: default\nbrand: _brand.yml\n",
    );
    write(
        &root.join("_brand.yml"),
        concat!(
            "color:\n",
            "  background:\n",
            "    light: \"#fefefd\"\n",
            "    dark: \"#333231\"\n",
            "typography:\n",
            "  headings:\n",
            "    color:\n",
            "      light: \"#111143\"\n",
            "      dark: \"#d0d0fe\"\n",
        ),
    );
    write(
        &root.join("index.qmd"),
        "---\nformat: html\n---\n\n# Heading\n",
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
    .expect("project-level unified brand must render");

    let files = css_files_by_path(&result.resources_dir);
    let light = files
        .iter()
        .find(|(_, css)| css.contains("#fefefd"))
        .expect("light background present in some CSS file");
    let dark = files
        .iter()
        .find(|(_, css)| css.contains("#333231"))
        .expect("dark background present in some CSS file");
    assert_ne!(light.0, dark.0);
    assert!(
        light.1.contains("#111143") && !light.1.contains("#d0d0fe"),
        "light headings color only in the light variant"
    );
    assert!(
        dark.1.contains("#d0d0fe") && !dark.1.contains("#111143"),
        "dark headings color only in the dark variant"
    );
}

/// A unified brand with no dark values must NOT enable dark mode:
/// single stylesheet, no attributed links, no color-mode runtime
/// (Q1's `enablesDarkMode` counts only actual `dark:` keys).
#[test]
fn unified_brand_all_plain_stays_single_variant() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write(
        &root.join("_brand.yml"),
        "color:\n  background: \"#fefefd\"\n  primary: \"#3366c1\"\n",
    );
    write(
        &root.join("index.qmd"),
        "---\nformat: html\nbrand: _brand.yml\n---\n\nHello.\n",
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
    .expect("plain brand must render");

    let files = css_files_by_path(&result.resources_dir);
    assert!(
        !files
            .iter()
            .any(|(p, _)| p.file_name().and_then(|n| n.to_str()) == Some("styles-dark.css")),
        "no dark values → no dark stylesheet"
    );
    let html = std::fs::read_to_string(&result.output_path).unwrap();
    assert!(
        !html.contains("quarto-color-scheme"),
        "no dark values → plain, attribute-free links"
    );
    assert!(
        !html.contains("quarto-color-mode"),
        "no dark values → no color-mode runtime"
    );
}

/// The navbar dark-mode toggle must appear when the ONLY dark signal
/// is unified brand content (no `theme: {light:, dark:}` pair) — the
/// toggle decision may not be re-derived from config alone.
#[test]
fn unified_brand_enables_navbar_toggle() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write(
        &root.join("_quarto.yml"),
        concat!(
            "project:\n",
            "  type: website\n",
            "website:\n",
            "  navbar:\n",
            "    left:\n",
            "      - href: index.qmd\n",
            "        text: Home\n",
            "brand: _brand.yml\n",
        ),
    );
    write(
        &root.join("_brand.yml"),
        "color:\n  background:\n    light: \"#fefefd\"\n    dark: \"#333231\"\n",
    );
    write(
        &root.join("index.qmd"),
        "---\ntitle: Home\n---\n\n# Hello\n",
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
    .expect("website with unified brand must render");
    let html = std::fs::read_to_string(&result.output_path).unwrap();
    assert!(
        html.contains(r#"class="quarto-color-scheme-toggle"#),
        "navbar must carry the dark-mode toggle when brand content enables dark mode"
    );
}
