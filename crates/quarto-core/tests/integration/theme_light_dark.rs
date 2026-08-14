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

/// A3: the emitted `<link>` tags carry Q1's classes/id/data-mode, in
/// the FOUC-safe order (light, dark, trailing light copy for an
/// author-default-light pair), plus the `<meta name="color-scheme">`
/// pre-CSS paint hint (D1a).
#[test]
fn light_dark_map_emits_attributed_links_and_meta() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write(&root.join("light-marker.scss"), LIGHT_SCSS);
    write(&root.join("dark-marker.scss"), DARK_SCSS);
    write(
        &root.join("doc.qmd"),
        "---\ntitle: Attributed Links\nformat:\n  html:\n    theme:\n      light: [cosmo, light-marker.scss]\n      dark: [darkly, dark-marker.scss]\n---\n\n# Hi\n",
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
    .expect("must render");
    let html = std::fs::read_to_string(&result.output_path).unwrap();

    // Light link: primary color-scheme sheet, mode from the compiled
    // CSS's darkness sentinel.
    let light = html
        .find(r#"<link rel="stylesheet" href="doc_files/styles.css" class="quarto-color-scheme" id="quarto-bootstrap" data-mode="light">"#)
        .expect("attributed light link");
    // Dark link: alternate sheet.
    let dark = html
        .find(r#"<link rel="stylesheet" href="doc_files/styles-dark.css" class="quarto-color-scheme quarto-color-alternate" id="quarto-bootstrap" data-mode="dark">"#)
        .expect("attributed dark link");
    // Trailing light copy (author default is light): re-links the SAME
    // file so no-JS/first-paint lands on the default variant.
    let extra = html
        .find(r#"<link rel="stylesheet" href="doc_files/styles.css" class="quarto-color-scheme-extra" id="quarto-bootstrap" data-mode="light">"#)
        .expect("trailing light-copy link");
    assert!(
        light < dark && dark < extra,
        "link order must be light ({light}), dark ({dark}), extra ({extra})"
    );

    // D1a: pre-CSS paint hint matches the author default.
    assert!(
        html.contains(r#"<meta name="color-scheme" content="light">"#),
        "author-default-light pair must emit the light color-scheme meta"
    );
}

/// A3: author-default-dark (dark listed first) — two links only
/// (light, dark; the enabled-last dark wins pre-JS), dark meta.
#[test]
fn dark_first_map_emits_dark_default_links_and_meta() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write(
        &root.join("doc.qmd"),
        "---\ntitle: Dark Default\nformat:\n  html:\n    theme:\n      dark: darkly\n      light: cosmo\n---\n\n# Hi\n",
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
    .expect("must render");
    let html = std::fs::read_to_string(&result.output_path).unwrap();

    let light = html
        .find(r#"class="quarto-color-scheme" id="quarto-bootstrap" data-mode="light""#)
        .expect("light link present");
    let dark = html
        .find(r#"class="quarto-color-scheme quarto-color-alternate" id="quarto-bootstrap" data-mode="dark""#)
        .expect("dark link present");
    assert!(light < dark, "light link first, dark (default) last");
    assert!(
        !html.contains(r#"class="quarto-color-scheme-extra""#),
        "author-default-dark must not emit the trailing-copy link \
         (the inline runtime's source mentioning the class is fine)"
    );
    assert!(
        html.contains(r#"<meta name="color-scheme" content="dark">"#),
        "author-default-dark pair must emit the dark color-scheme meta"
    );
}

/// A3 + D1a: `respect-user-color-scheme: true` makes the pre-CSS
/// paint hint offer both schemes so the UA picks per
/// `prefers-color-scheme` (author-default first).
#[test]
fn respect_user_color_scheme_emits_dual_meta() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write(
        &root.join("doc.qmd"),
        "---\ntitle: Respect\nformat:\n  html:\n    respect-user-color-scheme: true\n    theme:\n      light: cosmo\n      dark: darkly\n---\n\n# Hi\n",
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
    .expect("must render");
    let html = std::fs::read_to_string(&result.output_path).unwrap();
    assert!(
        html.contains(r#"<meta name="color-scheme" content="light dark">"#),
        "respect-user-color-scheme must offer both schemes, author default first"
    );
}

/// A3 back-compat: without a dark variant, links stay exactly as
/// before — no classes, no id, no data-mode, no meta tag. (The
/// phase5 golden-hash baseline guards this at the byte level; this
/// assertion documents it at the feature level.)
#[test]
fn single_variant_links_stay_plain() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write(
        &root.join("doc.qmd"),
        "---\ntitle: Plain\nformat:\n  html:\n    theme: cosmo\n---\n\n# Hi\n",
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
    .expect("must render");
    let html = std::fs::read_to_string(&result.output_path).unwrap();
    assert!(
        html.contains(r#"<link rel="stylesheet" href="doc_files/styles.css">"#),
        "single-variant link must stay attribute-free"
    );
    assert!(!html.contains("quarto-color-scheme"));
    assert!(!html.contains(r#"<meta name="color-scheme""#));
}

/// A4: the color-mode runtime is injected as an inline synchronous
/// script at the very top of `<body>` (before any paintable content —
/// the FOUC hard constraint), configured via data attributes.
#[test]
fn light_dark_map_injects_color_mode_script_at_top_of_body() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write(
        &root.join("doc.qmd"),
        "---\ntitle: Toggle\nformat:\n  html:\n    theme:\n      light: cosmo\n      dark: darkly\n---\n\n# Hi\n",
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
    .expect("must render");
    let html = std::fs::read_to_string(&result.output_path).unwrap();

    let body = html.find("<body").expect("body tag");
    let script = html
        .find(r#"<script id="quarto-color-mode" data-author-prefers-dark="false" data-respect-user-color-scheme="false">"#)
        .expect("inline color-mode script with config data attributes");
    assert!(script > body, "script must be inside body");
    // Nothing paintable between <body ...> and the script: only the
    // body tag itself (and whitespace) may precede it.
    let between = &html[body..script];
    let after_tag = &between[between.find('>').unwrap() + 1..];
    assert!(
        after_tag.trim().is_empty(),
        "color-mode script must be the first thing in <body>, found: {after_tag:?}"
    );
    assert!(
        html.contains("window.quartoToggleColorScheme"),
        "toggle entry point must be defined inline"
    );
    // Author default is light → body baked light.
    assert!(html.contains(r#"quarto-light""#) || html.contains(r#"quarto-light "#));
}

/// A4: author-default-dark bakes `quarto-dark` on `<body>` and tells
/// the runtime the author prefers dark.
#[test]
fn dark_first_map_bakes_dark_body_class() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write(
        &root.join("doc.qmd"),
        "---\ntitle: Dark Default\nformat:\n  html:\n    theme:\n      dark: darkly\n      light: cosmo\n---\n\n# Hi\n",
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
    .expect("must render");
    let html = std::fs::read_to_string(&result.output_path).unwrap();

    let body_tag_end = html
        .find("<body")
        .and_then(|i| html[i..].find('>').map(|j| i + j))
        .unwrap();
    let body_tag = &html[html.find("<body").unwrap()..=body_tag_end];
    assert!(
        body_tag.contains("quarto-dark"),
        "author-default-dark must bake quarto-dark on body, got: {body_tag}"
    );
    assert!(!body_tag.contains("quarto-light"));
    assert!(html.contains(r#"data-author-prefers-dark="true""#));
}

/// A4: `respect-user-color-scheme: true` reaches the runtime config.
#[test]
fn respect_user_color_scheme_reaches_script_config() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write(
        &root.join("doc.qmd"),
        "---\ntitle: Respect\nformat:\n  html:\n    respect-user-color-scheme: true\n    theme:\n      light: cosmo\n      dark: darkly\n---\n\n# Hi\n",
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
    .expect("must render");
    let html = std::fs::read_to_string(&result.output_path).unwrap();
    assert!(html.contains(r#"data-respect-user-color-scheme="true""#));
}

/// A4: no dark variant → no runtime script (byte-identity with the
/// pre-feature output is separately guarded by the golden-hash test).
#[test]
fn single_variant_has_no_color_mode_script() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write(
        &root.join("doc.qmd"),
        "---\ntitle: Plain\nformat:\n  html:\n    theme: cosmo\n---\n\n# Hi\n",
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
    .expect("must render");
    let html = std::fs::read_to_string(&result.output_path).unwrap();
    assert!(!html.contains("quarto-color-mode"));
    assert!(!html.contains("quartoToggleColorScheme"));
}

/// A4: a website navbar grows the dark-mode toggle (Q1's
/// `quarto-navbar-tools` slot) when a dark variant exists — and only
/// then.
#[test]
fn website_navbar_gets_dark_toggle_only_with_dark_variant() {
    let render_site = |theme_yaml: &str| -> String {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        write(
            &root.join("_quarto.yml"),
            &format!(
                "project:\n  type: website\nwebsite:\n  navbar:\n    left:\n      - href: index.qmd\n        text: Home\nformat:\n  html:\n{theme_yaml}"
            ),
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
        .expect("website page must render");
        std::fs::read_to_string(&result.output_path).unwrap()
    };

    let with_dark = render_site("    theme:\n      light: cosmo\n      dark: darkly\n");
    assert!(
        with_dark.contains(r#"class="quarto-color-scheme-toggle"#),
        "navbar must carry the dark-mode toggle when a dark variant exists"
    );
    assert!(
        with_dark.contains("window.quartoToggleColorScheme(); return false;"),
        "toggle anchor must invoke the runtime entry point"
    );

    let without_dark = render_site("    theme: cosmo\n");
    assert!(
        !without_dark.contains("quarto-color-scheme-toggle"),
        "no dark variant → no toggle"
    );
}

/// Phase B: `highlight-style: a11y` selects the variant-matching
/// palette in each compile — a11y-light colors in the light CSS,
/// a11y-dark colors (and its code-block background) in the dark CSS.
#[test]
fn highlight_style_a11y_selects_palette_per_variant() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write(
        &root.join("doc.qmd"),
        "---\ntitle: HL\nformat:\n  html:\n    theme:\n      light: cosmo\n      dark: darkly\nhighlight-style: a11y\n---\n\n```python\nprint('hi')\n```\n",
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
    .expect("must render");

    let files = css_files(&result.resources_dir);
    let light = files
        .iter()
        .find(|(p, _)| p.file_name().and_then(|n| n.to_str()) == Some("styles.css"))
        .expect("light css");
    let dark = files
        .iter()
        .find(|(p, _)| p.file_name().and_then(|n| n.to_str()) == Some("styles-dark.css"))
        .expect("dark css");
    assert!(
        light.1.contains("#d91e18"),
        "light variant must use the a11y-light keyword color"
    );
    assert!(
        !light.1.contains("#859900"),
        "light variant must not keep the solarized keyword color"
    );
    assert!(
        dark.1.contains("#ffa07a"),
        "dark variant must use the a11y-dark keyword color"
    );
    assert!(
        dark.1.contains("#2b2b2b"),
        "dark variant must apply a11y-dark's code-block background"
    );

    assert!(
        !result
            .render_output
            .diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some("Q-14-5")),
        "known adaptive style must not warn"
    );
}

/// Phase B: a single dark built-in theme resolves the adaptive name
/// to the dark palette (Q1's sentinel-driven behavior, approximated
/// statically via BuiltInTheme::is_dark).
#[test]
fn highlight_style_adaptive_follows_single_dark_theme() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write(
        &root.join("doc.qmd"),
        "---\ntitle: HL\nformat:\n  html:\n    theme: darkly\nhighlight-style: a11y\n---\n\n```python\nprint('hi')\n```\n",
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
    .expect("must render");

    let files = css_files(&result.resources_dir);
    let styles = files
        .iter()
        .find(|(p, _)| p.file_name().and_then(|n| n.to_str()) == Some("styles.css"))
        .expect("styles.css");
    assert!(
        styles.1.contains("#ffa07a"),
        "theme: darkly + a11y must select the a11y-dark palette"
    );
}

/// Phase B: `highlight-style` must apply even with NO theme configured
/// (the default-Bootstrap fast path must not bypass palette
/// selection).
#[test]
fn highlight_style_applies_without_theme() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write(
        &root.join("doc.qmd"),
        "---\ntitle: HL\nhighlight-style: a11y\n---\n\n```python\nprint('hi')\n```\n",
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
    .expect("must render");

    let files = css_files(&result.resources_dir);
    assert!(
        files.iter().any(|(_, css)| css.contains("#d91e18")),
        "a11y-light palette must apply to the default-Bootstrap compile"
    );
}

/// Phase B: an unknown highlight-style warns (Q-14-5) and falls back
/// to the default palette.
#[test]
fn unknown_highlight_style_warns_and_uses_default() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write(
        &root.join("doc.qmd"),
        "---\ntitle: HL\nformat:\n  html:\n    theme: cosmo\nhighlight-style: nosuchstyle\n---\n\n```python\nprint('hi')\n```\n",
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
    .expect("unknown highlight-style must not fail the render");

    let files = css_files(&result.resources_dir);
    assert!(
        files.iter().any(|(_, css)| css.contains("#859900")),
        "unknown style must fall back to the default (solarized) palette"
    );

    let q14_5: Vec<_> = result
        .render_output
        .diagnostics
        .iter()
        .filter(|d| d.code.as_deref() == Some("Q-14-5"))
        .collect();
    assert_eq!(
        q14_5.len(),
        1,
        "expected exactly one Q-14-5 warning, diagnostics: {:?}",
        result.render_output.diagnostics
    );
    assert!(q14_5[0].location.is_some(), "warning carries a location");
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
