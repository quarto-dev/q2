//! End-to-end tests for `source: file` brand fonts (bd-ve916wr8).
//!
//! A local font named in `_brand.yml` must be *published* — as a
//! project-scope artifact in a `fonts/` directory beside whichever
//! theme CSS references it — and the `@font-face` URL the theme CSS
//! carries must resolve from that CSS's own directory. Before this
//! work, nothing copied the file and the URL was document-relative
//! while the CSS lived in `site_libs/quarto/` (website) or
//! `{stem}_files/` (single doc), so every local brand font 404'd.
//!
//! Every test here drives the real render entry points
//! (`render_to_file` / `ProjectPipeline`), not `brand_to_layers`,
//! because the defects lived in the plumbing between the SCSS layer
//! and the output tree. Evidence for the original defects:
//! `claude-notes/plans/brand-file-fonts-website-investigation/NOTES.md`.

#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, ProjectRenderSummary, project_type_for};
use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};
use quarto_error_reporting::DiagnosticMessage;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

// ── helpers ─────────────────────────────────────────────────────────

const LIGHT_BYTES: &[u8] = b"wOF2-light-placeholder";
const DARK_BYTES: &[u8] = b"wOF2-dark-placeholder-different-length";

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn write(path: &Path, contents: &str) {
    write_bytes(path, contents.as_bytes());
}

fn write_bytes(path: &Path, contents: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

/// A `_brand.yml` declaring one `source: file` font whose single file
/// entry is `path` (with weight/style, the shape a real brand uses).
fn brand_yaml(path: &str) -> String {
    format!(
        "typography:\n  fonts:\n    - family: EB Garamond\n      source: file\n      files:\n        - path: {path}\n          weight: 400\n          style: normal\n  base: EB Garamond\n"
    )
}

const WEBSITE_CONFIG: &str = "project:\n  type: website\n  output-dir: _site\nbrand: _brand.yml\nformat:\n  html:\n    theme: [brand]\n";

/// Drive a fixture through `ProjectPipeline`; the caller decides
/// whether failures are expected.
fn try_render_project(
    fixture: impl FnOnce(&Path),
) -> (PathBuf, quarto_core::error::Result<ProjectRenderSummary>) {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    fixture(&project_dir);

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();
    let options = RenderToFileOptions {
        quiet: true,
        ..Default::default()
    };
    let project_type = project_type_for(&project);
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        Format::html(),
        "html",
        &options,
        runtime.clone(),
    );
    let result = pollster::block_on(pipeline.run());
    // Leak the temp dir so the test can inspect files after this
    // function returns (cleanup happens at process exit).
    std::mem::forget(temp);
    (project_dir, result)
}

fn render_project(fixture: impl FnOnce(&Path)) -> (PathBuf, ProjectRenderSummary) {
    let (dir, result) = try_render_project(fixture);
    let summary = result.expect("pipeline");
    assert!(
        summary.pass1_failures.is_empty() && summary.pass2_failures.is_empty(),
        "unexpected failures: pass1={:?} pass2={:?}",
        summary.pass1_failures,
        summary.pass2_failures
    );
    (dir, summary)
}

/// Every diagnostic raised while rendering any page of the project.
fn page_diagnostics(summary: &ProjectRenderSummary) -> Vec<&DiagnosticMessage> {
    summary
        .outputs
        .iter()
        .flat_map(|o| o.render_output.diagnostics.iter())
        .collect()
}

fn html_for_stem(summary: &ProjectRenderSummary, stem: &str) -> String {
    let out = summary
        .outputs
        .iter()
        .find(|o| o.output_path.file_stem().and_then(|s| s.to_str()) == Some(stem))
        .unwrap_or_else(|| panic!("no output for stem '{stem}'"));
    read(&out.output_path)
}

/// The theme bundles in `<dir>` (filenames matching
/// `quarto-theme-*.css`, light or dark).
fn theme_css_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("quarto-theme-") && n.ends_with(".css"))
        })
        .collect();
    files.sort();
    files
}

/// The `@font-face` block(s) in a compiled (minified) theme CSS.
fn font_face_blocks(css: &str) -> Vec<String> {
    css.match_indices("@font-face")
        .map(|(i, _)| {
            let end = css[i..].find('}').map_or(css.len(), |e| i + e + 1);
            css[i..end].to_string()
        })
        .collect()
}

fn single_doc_render(
    root: &Path,
    input: &str,
    format: &str,
) -> quarto_core::render_to_file::RenderToFileResult {
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    render_to_file(
        &root.join(input),
        format,
        &RenderToFileOptions {
            quiet: true,
            ..Default::default()
        },
        runtime,
    )
    .expect("render_to_file")
}

// ── website ─────────────────────────────────────────────────────────

/// The headline case: pages at two depths share ONE theme bundle in
/// `site_libs/quarto/`, its `@font-face` URL is the constant
/// `fonts/<basename>`, and the file is right there next to it.
#[test]
fn website_file_font_published_beside_theme_css_for_every_page_depth() {
    let (project_dir, summary) = render_project(|dir| {
        write(&dir.join("_quarto.yml"), WEBSITE_CONFIG);
        write(
            &dir.join("_brand.yml"),
            &brand_yaml("assets/EBGaramond.woff2"),
        );
        write_bytes(&dir.join("assets/EBGaramond.woff2"), LIGHT_BYTES);
        write(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nHello.\n");
        write(
            &dir.join("posts/one.qmd"),
            "---\ntitle: One\n---\n\nPost.\n",
        );
    });

    let lib = project_dir.join("_site/site_libs/quarto");
    let bundles = theme_css_files(&lib);
    assert_eq!(
        bundles.len(),
        1,
        "exactly one theme bundle for both depths; got {bundles:?}"
    );
    let bundle_name = bundles[0]
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    for stem in ["index", "one"] {
        let html = html_for_stem(&summary, stem);
        assert!(
            html.contains(&format!("site_libs/quarto/{bundle_name}")),
            "{stem}.html must link the shared bundle {bundle_name}:\n{html}"
        );
    }

    let css = read(&bundles[0]);
    let blocks = font_face_blocks(&css);
    assert_eq!(blocks.len(), 1, "one @font-face block:\n{css}");
    assert!(
        blocks[0].contains(r#"url("fonts/EBGaramond.woff2")"#),
        "URL must be the constant fonts/<basename>, resolvable from the bundle's own directory:\n{}",
        blocks[0]
    );
    assert!(
        blocks[0].contains(r#"format("woff2")"#),
        "format() hint derived from the extension:\n{}",
        blocks[0]
    );
    assert!(
        !css.contains("assets/EBGaramond"),
        "the brand-relative source path must not leak into the CSS:\n{css}"
    );

    let published = lib.join("fonts/EBGaramond.woff2");
    assert_eq!(
        std::fs::read(&published).ok().as_deref(),
        Some(LIGHT_BYTES),
        "font bytes published at {}",
        published.display()
    );
    assert!(
        !project_dir.join("_site/assets/EBGaramond.woff2").exists(),
        "no source-mirrored copy (fonts are artifacts, not resources)"
    );
}

/// A project whose only page is nested: the URL must not depend on
/// the document's depth (the cache-aliasing regression — the first
/// page to compile a brand used to pin a `pathdiff(document_dir,
/// brand_dir)` prefix for every later page).
#[test]
fn website_font_url_does_not_depend_on_document_depth() {
    let (project_dir, _summary) = render_project(|dir| {
        write(&dir.join("_quarto.yml"), WEBSITE_CONFIG);
        write(
            &dir.join("_brand.yml"),
            &brand_yaml("assets/EBGaramond.woff2"),
        );
        write_bytes(&dir.join("assets/EBGaramond.woff2"), LIGHT_BYTES);
        write(
            &dir.join("posts/deep/one.qmd"),
            "---\ntitle: One\n---\n\nPost.\n",
        );
    });

    let lib = project_dir.join("_site/site_libs/quarto");
    let bundles = theme_css_files(&lib);
    assert_eq!(bundles.len(), 1, "got {bundles:?}");
    let css = read(&bundles[0]);
    assert!(
        css.contains(r#"url("fonts/EBGaramond.woff2")"#),
        "constant URL regardless of page depth:\n{}",
        font_face_blocks(&css).join("\n")
    );
    assert!(
        !css.contains("../"),
        "no document-relative climbing in the bundle:\n{}",
        font_face_blocks(&css).join("\n")
    );
    assert!(lib.join("fonts/EBGaramond.woff2").exists());
}

/// A leading `/` names the project root (path-resolution contract,
/// decision 3). The file is found there and published like any other.
#[test]
fn website_rooted_font_path_resolves_against_project_root() {
    let (project_dir, _summary) = render_project(|dir| {
        write(&dir.join("_quarto.yml"), WEBSITE_CONFIG);
        write(&dir.join("_brand.yml"), &brand_yaml("/assets/Rooted.woff2"));
        write_bytes(&dir.join("assets/Rooted.woff2"), LIGHT_BYTES);
        write(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nHello.\n");
    });

    let lib = project_dir.join("_site/site_libs/quarto");
    let css = read(&theme_css_files(&lib)[0]);
    assert!(
        css.contains(r#"url("fonts/Rooted.woff2")"#),
        "rooted path publishes by basename:\n{}",
        font_face_blocks(&css).join("\n")
    );
    assert_eq!(
        std::fs::read(lib.join("fonts/Rooted.woff2"))
            .ok()
            .as_deref(),
        Some(LIGHT_BYTES)
    );
}

/// A brand file in a subdirectory: font paths are relative to the
/// brand file, not the project root.
#[test]
fn website_font_path_is_relative_to_the_brand_file() {
    let (project_dir, _summary) = render_project(|dir| {
        write(
            &dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\nbrand: brand/_brand.yml\nformat:\n  html:\n    theme: [brand]\n",
        );
        write(
            &dir.join("brand/_brand.yml"),
            &brand_yaml("fonts/Nested.woff2"),
        );
        write_bytes(&dir.join("brand/fonts/Nested.woff2"), LIGHT_BYTES);
        write(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nHello.\n");
    });

    let lib = project_dir.join("_site/site_libs/quarto");
    assert_eq!(
        std::fs::read(lib.join("fonts/Nested.woff2"))
            .ok()
            .as_deref(),
        Some(LIGHT_BYTES)
    );
}

/// Light and dark brands with distinct files: both published, no
/// conflict, each bundle references its own.
#[test]
fn website_light_and_dark_brands_publish_distinct_fonts() {
    let (project_dir, _summary) = render_project(|dir| {
        write(
            &dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\nbrand:\n  light: light/_brand.yml\n  dark: dark/_brand.yml\nformat:\n  html:\n    theme: [brand]\n",
        );
        write(&dir.join("light/_brand.yml"), &brand_yaml("Light.woff2"));
        write_bytes(&dir.join("light/Light.woff2"), LIGHT_BYTES);
        write(&dir.join("dark/_brand.yml"), &brand_yaml("Dark.woff2"));
        write_bytes(&dir.join("dark/Dark.woff2"), DARK_BYTES);
        write(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nHello.\n");
    });

    let lib = project_dir.join("_site/site_libs/quarto");
    assert_eq!(
        std::fs::read(lib.join("fonts/Light.woff2")).ok().as_deref(),
        Some(LIGHT_BYTES)
    );
    assert_eq!(
        std::fs::read(lib.join("fonts/Dark.woff2")).ok().as_deref(),
        Some(DARK_BYTES)
    );
    let bundles = theme_css_files(&lib);
    assert_eq!(bundles.len(), 2, "light + dark bundles; got {bundles:?}");
    let all: String = bundles.iter().map(|p| read(p)).collect();
    assert!(all.contains(r#"url("fonts/Light.woff2")"#), "{all}");
    assert!(all.contains(r#"url("fonts/Dark.woff2")"#), "{all}");
}

// ── collisions (decision 2: an error, never a silent overwrite) ─────

/// Two documents whose brands publish different bytes under the same
/// `fonts/<basename>`: the project render fails, and the message names
/// both source files so the user knows what to rename.
#[test]
fn website_same_font_name_different_bytes_across_documents_is_an_error() {
    let (_project_dir, result) = try_render_project(|dir| {
        write(
            &dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\nformat:\n  html:\n    theme: [brand]\n",
        );
        write(&dir.join("a/_brand.yml"), &brand_yaml("Regular.woff2"));
        write_bytes(&dir.join("a/Regular.woff2"), LIGHT_BYTES);
        write(&dir.join("b/_brand.yml"), &brand_yaml("Regular.woff2"));
        write_bytes(&dir.join("b/Regular.woff2"), DARK_BYTES);
        write(
            &dir.join("a.qmd"),
            "---\ntitle: A\nbrand: a/_brand.yml\n---\n\nA.\n",
        );
        write(
            &dir.join("b.qmd"),
            "---\ntitle: B\nbrand: b/_brand.yml\n---\n\nB.\n",
        );
    });

    let message = match result {
        Err(e) => e.to_string(),
        Ok(summary) => {
            assert!(
                summary.has_failures(),
                "expected the render to fail on the font-name collision"
            );
            summary
                .pass1_failures
                .iter()
                .chain(&summary.pass2_failures)
                .map(|f| {
                    let details: Vec<String> = f
                        .diagnostics
                        .iter()
                        .map(|d| format!("{:?} {}", d.code, d.title))
                        .collect();
                    format!("{} {}", f.error, details.join(" "))
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
    };
    assert!(
        message.contains("Q-14-10"),
        "collision diagnostic carries its catalog code:\n{message}"
    );
    assert!(
        message.contains("Regular.woff2"),
        "names the colliding published name:\n{message}"
    );
    for source in ["a/Regular.woff2", "b/Regular.woff2"] {
        assert!(
            message.contains(source),
            "names both source files ({source}):\n{message}"
        );
    }
}

/// The same collision inside ONE document: light and dark brands
/// each ship `Regular.woff2` with different bytes.
#[test]
fn website_same_font_name_different_bytes_across_light_dark_is_an_error() {
    let (_project_dir, result) = try_render_project(|dir| {
        write(
            &dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\nbrand:\n  light: light/_brand.yml\n  dark: dark/_brand.yml\nformat:\n  html:\n    theme: [brand]\n",
        );
        write(&dir.join("light/_brand.yml"), &brand_yaml("Regular.woff2"));
        write_bytes(&dir.join("light/Regular.woff2"), LIGHT_BYTES);
        write(&dir.join("dark/_brand.yml"), &brand_yaml("Regular.woff2"));
        write_bytes(&dir.join("dark/Regular.woff2"), DARK_BYTES);
        write(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nHello.\n");
    });

    let message = match result {
        Err(e) => e.to_string(),
        Ok(summary) => {
            assert!(summary.has_failures(), "expected a failure");
            summary
                .pass1_failures
                .iter()
                .chain(&summary.pass2_failures)
                .map(|f| {
                    let details: Vec<String> = f
                        .diagnostics
                        .iter()
                        .map(|d| format!("{:?} {}", d.code, d.title))
                        .collect();
                    format!("{} {}", f.error, details.join(" "))
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
    };
    assert!(message.contains("Q-14-10"), "{message}");
    for source in ["light/Regular.woff2", "dark/Regular.woff2"] {
        assert!(message.contains(source), "names {source}:\n{message}");
    }
}

/// Identical bytes under the same name from two brands are not a
/// collision — they dedupe (same rule as every other project artifact).
#[test]
fn website_same_font_name_identical_bytes_dedupes() {
    let (project_dir, _summary) = render_project(|dir| {
        write(
            &dir.join("_quarto.yml"),
            "project:\n  type: website\n  output-dir: _site\nformat:\n  html:\n    theme: [brand]\n",
        );
        write(&dir.join("a/_brand.yml"), &brand_yaml("Regular.woff2"));
        write_bytes(&dir.join("a/Regular.woff2"), LIGHT_BYTES);
        write(&dir.join("b/_brand.yml"), &brand_yaml("Regular.woff2"));
        write_bytes(&dir.join("b/Regular.woff2"), LIGHT_BYTES);
        write(
            &dir.join("a.qmd"),
            "---\ntitle: A\nbrand: a/_brand.yml\n---\n\nA.\n",
        );
        write(
            &dir.join("b.qmd"),
            "---\ntitle: B\nbrand: b/_brand.yml\n---\n\nB.\n",
        );
    });
    assert_eq!(
        std::fs::read(project_dir.join("_site/site_libs/quarto/fonts/Regular.woff2"))
            .ok()
            .as_deref(),
        Some(LIGHT_BYTES)
    );
}

// ── diagnostics ─────────────────────────────────────────────────────

/// A font file that does not exist: warn (naming the brand file and
/// the path), keep rendering, publish nothing. The `@font-face` still
/// references it — a visibly missing font beats a silently absent one,
/// matching the favicon / navbar-logo rule.
#[test]
fn website_missing_font_file_warns_and_continues() {
    let (project_dir, summary) = render_project(|dir| {
        write(&dir.join("_quarto.yml"), WEBSITE_CONFIG);
        write(&dir.join("_brand.yml"), &brand_yaml("assets/Missing.woff2"));
        write(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nHello.\n");
    });

    let lib = project_dir.join("_site/site_libs/quarto");
    assert!(
        !lib.join("fonts/Missing.woff2").exists(),
        "nothing published for a missing source"
    );
    let css = read(&theme_css_files(&lib)[0]);
    assert!(
        css.contains(r#"url("fonts/Missing.woff2")"#),
        "the @font-face is still emitted:\n{css}"
    );

    let warnings: Vec<&DiagnosticMessage> = page_diagnostics(&summary)
        .into_iter()
        .filter(|d| d.code.as_deref() == Some("Q-14-9"))
        .collect();
    assert_eq!(
        warnings.len(),
        1,
        "one Q-14-9 warning; got: {:?}",
        page_diagnostics(&summary)
            .iter()
            .map(|d| format!("{:?} {}", d.code, d.title))
            .collect::<Vec<_>>()
    );
    let text = format!("{:?}", warnings[0]);
    assert!(
        text.contains("assets/Missing.woff2"),
        "names the path:\n{text}"
    );
    assert!(text.contains("_brand.yml"), "names the brand file:\n{text}");
    assert!(text.contains("EB Garamond"), "names the family:\n{text}");
}

/// `format:` / `display:` on a file entry (unsettled in the brand.yml
/// spec) warn once and are ignored — the render, the @font-face and
/// the published file are all unaffected.
#[test]
fn website_unknown_key_on_font_file_entry_warns_once_and_is_ignored() {
    let (project_dir, summary) = render_project(|dir| {
        write(&dir.join("_quarto.yml"), WEBSITE_CONFIG);
        write(
            &dir.join("_brand.yml"),
            "typography:\n  fonts:\n    - family: EB Garamond\n      source: file\n      files:\n        - path: assets/EBGaramond.woff2\n          weight: 400\n          format: woff2\n          display: swap\n  base: EB Garamond\n",
        );
        write_bytes(&dir.join("assets/EBGaramond.woff2"), LIGHT_BYTES);
        write(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nHello.\n");
    });

    let lib = project_dir.join("_site/site_libs/quarto");
    assert!(lib.join("fonts/EBGaramond.woff2").exists());
    let css = read(&theme_css_files(&lib)[0]);
    assert!(css.contains(r#"url("fonts/EBGaramond.woff2")"#), "{css}");
    assert!(
        !css.contains("swap"),
        "the ignored display: value must not reach the CSS:\n{}",
        font_face_blocks(&css).join("\n")
    );

    let warnings: Vec<&DiagnosticMessage> = page_diagnostics(&summary)
        .into_iter()
        .filter(|d| d.code.as_deref() == Some("Q-14-11"))
        .collect();
    assert_eq!(
        warnings.len(),
        1,
        "exactly one Q-14-11 warning for the entry; got: {:?}",
        page_diagnostics(&summary)
            .iter()
            .map(|d| format!("{:?} {}", d.code, d.title))
            .collect::<Vec<_>>()
    );
    let text = format!("{:?}", warnings[0]);
    assert!(text.contains("format"), "names the ignored key:\n{text}");
    assert!(text.contains("display"), "names the ignored key:\n{text}");
    assert!(text.contains("EB Garamond"), "names the family:\n{text}");
}

// ── single document ─────────────────────────────────────────────────

/// Single-doc theme CSS lives in `{stem}_files/styles.css`, so the
/// font goes to `{stem}_files/fonts/` and the same constant URL works.
#[test]
fn single_doc_file_font_published_in_files_dir() {
    let temp = TempDir::new().unwrap();
    let root = canonical(temp.path());
    write(
        &root.join("_brand.yml"),
        &brand_yaml("assets/EBGaramond.woff2"),
    );
    write_bytes(&root.join("assets/EBGaramond.woff2"), LIGHT_BYTES);
    write(
        &root.join("doc.qmd"),
        "---\ntitle: Single\nbrand: _brand.yml\nformat:\n  html:\n    theme: [brand]\n---\n\nHello.\n",
    );

    let result = single_doc_render(&root, "doc.qmd", "html");
    let css = read(&result.resources_dir.join("styles.css"));
    let blocks = font_face_blocks(&css);
    assert_eq!(blocks.len(), 1, "{css}");
    assert!(
        blocks[0].contains(r#"url("fonts/EBGaramond.woff2") format("woff2")"#),
        "constant URL, resolvable from doc_files/:\n{}",
        blocks[0]
    );
    assert_eq!(
        std::fs::read(result.resources_dir.join("fonts/EBGaramond.woff2"))
            .ok()
            .as_deref(),
        Some(LIGHT_BYTES),
        "font published under {}",
        result.resources_dir.display()
    );
}

/// Reveal's theme CSS is a third location (`revealjs/` under the lib
/// dir); its fonts go to `revealjs/fonts/` so the constant URL still
/// resolves.
#[test]
fn single_doc_revealjs_file_font_published_beside_reveal_theme() {
    let temp = TempDir::new().unwrap();
    let root = canonical(temp.path());
    write(
        &root.join("_brand.yml"),
        &brand_yaml("assets/EBGaramond.woff2"),
    );
    write_bytes(&root.join("assets/EBGaramond.woff2"), LIGHT_BYTES);
    write(
        &root.join("deck.qmd"),
        "---\ntitle: Deck\nbrand: _brand.yml\nformat: revealjs\n---\n\n## Slide\n\nHello.\n",
    );

    let result = single_doc_render(&root, "deck.qmd", "revealjs");
    let reveal_dir = result.resources_dir.join("revealjs");
    let theme_css: Vec<PathBuf> = std::fs::read_dir(&reveal_dir)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", reveal_dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("theme-") && n.ends_with(".css"))
        })
        .collect();
    assert_eq!(
        theme_css.len(),
        1,
        "one compiled reveal theme; got {theme_css:?}"
    );
    let css = read(&theme_css[0]);
    assert!(
        css.contains(r#"url("fonts/EBGaramond.woff2")"#),
        "constant URL in the reveal theme:\n{}",
        font_face_blocks(&css).join("\n")
    );
    assert_eq!(
        std::fs::read(reveal_dir.join("fonts/EBGaramond.woff2"))
            .ok()
            .as_deref(),
        Some(LIGHT_BYTES),
        "font published beside the reveal theme"
    );
}
