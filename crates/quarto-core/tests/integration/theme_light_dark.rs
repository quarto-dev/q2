//! End-to-end render tests for the Q1 light/dark theme map form
//! (bd-0pic6 epic, phase A2: dual compilation).
//!
//! `theme: {light: […], dark: […]}` must compile BOTH variants: the
//! light half into the primary theme CSS, the dark half into a
//! separate `-dark` CSS artifact. No Q-14-3 warning is emitted (the
//! interim degradation from bd-o76p01wb is retired). Each compiled
//! variant carries a `color-scheme` declaration derived from its
//! `$body-bg` darkness (plan D1a).
//!
//! Drives the real `render_to_file` API (see CLAUDE.md "End-to-end
//! verification") against tempdir-based projects, then inspects both
//! the produced CSS and the render diagnostics.

#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};
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

/// Collect `(path, contents)` for every CSS file under the resources
/// dir, so assertions can distinguish which *file* carries a marker,
/// not just whether it appears anywhere.
fn css_files(resources_dir: &Path) -> Vec<(PathBuf, String)> {
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

const LIGHT_SCSS: &str = "/*-- scss:rules --*/\n.q-light-marker { color: #123456; }\n";
const DARK_SCSS: &str = "/*-- scss:rules --*/\n.q-dark-marker { color: #654321; }\n";

fn assert_no_q14_3(diagnostics: &[quarto_error_reporting::DiagnosticMessage]) {
    assert!(
        !diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some("Q-14-3")),
        "Q-14-3 is retired: the dark half now compiles, nothing is ignored; \
         diagnostics: {:?}",
        diagnostics
    );
}

/// Project-config path: the map form in `_quarto.yml`
/// (`format.html.theme`). Both halves compile; the light and dark
/// variants land in separate CSS files with matching `color-scheme`
/// declarations.
#[test]
fn project_theme_light_dark_map_compiles_both_variants() {
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
    .expect("light/dark theme map must render");

    let files = css_files(&result.resources_dir);
    let light_file = files
        .iter()
        .find(|(_, css)| css.contains(".q-light-marker"))
        .expect("some CSS file must carry the light half's custom rule");
    let dark_file = files
        .iter()
        .find(|(_, css)| css.contains(".q-dark-marker"))
        .expect("some CSS file must carry the dark half's custom rule (dark now compiles)");
    assert_ne!(
        light_file.0, dark_file.0,
        "light and dark variants must be separate CSS files"
    );
    assert!(
        !light_file.1.contains(".q-dark-marker"),
        "light variant must not contain dark rules"
    );
    assert!(
        !dark_file.1.contains(".q-light-marker"),
        "dark variant must not contain light rules"
    );

    // D1a: each variant declares its own color-scheme, derived from
    // $body-bg darkness (cosmo → light, darkly → dark).
    assert!(
        light_file.1.contains("color-scheme:light") || light_file.1.contains("color-scheme: light"),
        "light variant CSS must declare color-scheme light"
    );
    assert!(
        dark_file.1.contains("color-scheme:dark") || dark_file.1.contains("color-scheme: dark"),
        "dark variant CSS must declare color-scheme dark"
    );

    assert_no_q14_3(&result.render_output.diagnostics);
}

/// Document-frontmatter path: the map form in the document's own
/// `format.html.theme` (single-file render, no `_quarto.yml`). The
/// single-doc dark artifact is `styles-dark.css` next to `styles.css`.
#[test]
fn frontmatter_theme_light_dark_map_compiles_both_variants() {
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

    let files = css_files(&result.resources_dir);
    let light_file = files
        .iter()
        .find(|(p, _)| p.file_name().and_then(|n| n.to_str()) == Some("styles.css"))
        .expect("single-doc light variant is styles.css");
    let dark_file = files
        .iter()
        .find(|(p, _)| p.file_name().and_then(|n| n.to_str()) == Some("styles-dark.css"))
        .expect("single-doc dark variant is styles-dark.css");
    assert!(light_file.1.contains(".q-light-marker"));
    assert!(!light_file.1.contains(".q-dark-marker"));
    assert!(dark_file.1.contains(".q-dark-marker"));
    assert!(!dark_file.1.contains(".q-light-marker"));

    assert_no_q14_3(&result.render_output.diagnostics);
}

/// A light-only map is a fully-honored (if redundant) spelling of the
/// plain form — no dark artifact is produced, no warning.
#[test]
fn light_only_theme_map_renders_without_dark_artifact() {
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

    let files = css_files(&result.resources_dir);
    assert!(
        files.iter().any(|(_, css)| css.contains(".q-light-marker")),
        "light half's custom SCSS must be in the compiled CSS"
    );
    assert!(
        !files.iter().any(|(p, _)| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            name.contains("-dark")
        }),
        "a light-only map must not produce a dark CSS artifact; files: {:?}",
        files.iter().map(|(p, _)| p).collect::<Vec<_>>()
    );

    assert_no_q14_3(&result.render_output.diagnostics);
}

/// Dark-only map: the light variant falls back to default Bootstrap,
/// the dark variant compiles the configured themes. No warning.
#[test]
fn dark_only_theme_map_compiles_dark_variant_with_default_light() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write(&root.join("dark-marker.scss"), DARK_SCSS);
    write(
        &root.join("doc.qmd"),
        "---\ntitle: Dark Only\nformat:\n  html:\n    theme:\n      dark: [darkly, dark-marker.scss]\n---\n\n# Hi\n",
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
    .expect("dark-only theme map must render");

    let files = css_files(&result.resources_dir);
    let light_file = files
        .iter()
        .find(|(p, _)| p.file_name().and_then(|n| n.to_str()) == Some("styles.css"))
        .expect("light variant (default Bootstrap) is styles.css");
    let dark_file = files
        .iter()
        .find(|(p, _)| p.file_name().and_then(|n| n.to_str()) == Some("styles-dark.css"))
        .expect("dark variant is styles-dark.css");
    assert!(
        !light_file.1.contains(".q-dark-marker"),
        "default-Bootstrap light variant must not carry dark rules"
    );
    assert!(dark_file.1.contains(".q-dark-marker"));
    assert!(
        dark_file.1.contains("color-scheme:dark") || dark_file.1.contains("color-scheme: dark"),
        "darkly-based dark variant must declare color-scheme dark"
    );

    assert_no_q14_3(&result.render_output.diagnostics);
}

/// D1a bonus: a *single* dark theme (no light/dark pair at all) gets
/// `color-scheme: dark` from the darkness sentinel, so existing
/// dark-theme users get correct native scrollbars/controls.
#[test]
fn single_dark_theme_declares_dark_color_scheme() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write(
        &root.join("doc.qmd"),
        "---\ntitle: Darkly\nformat:\n  html:\n    theme: darkly\n---\n\n# Hi\n",
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
    .expect("single dark theme must render");

    let files = css_files(&result.resources_dir);
    let styles = files
        .iter()
        .find(|(p, _)| p.file_name().and_then(|n| n.to_str()) == Some("styles.css"))
        .expect("styles.css present");
    assert!(
        styles.1.contains("color-scheme:dark") || styles.1.contains("color-scheme: dark"),
        "single dark theme must declare color-scheme dark"
    );
}
