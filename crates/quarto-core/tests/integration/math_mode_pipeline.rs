/*
 * tests/math_mode_pipeline.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Integration tests for bd-w5ov: math-mode rendering for HTML output.
 */

//! End-to-end integration tests for [`MathJsStage`].
//!
//! These tests drive a real `render_to_file` (single-doc) or
//! `ProjectPipeline` (website) render and assert behavior of the
//! generated HTML. We check:
//!
//! - When the source contains `$x$` / `$$y$$` / `$$z$$ {#eq-z}`, the
//!   rendered HTML includes the MathJax inline config block AND a
//!   `<script defer src=...>` pointing at the default jsDelivr CDN.
//! - When the source contains no math, neither block appears.
//! - When `html-math-method: katex` is set, the page references the
//!   default KaTeX CDN; no MathJax URL is present.
//! - When `html-math-method: { method: ..., url: ... }` is set, the
//!   user URL is honored verbatim.
//! - In a multi-page website, only the math-bearing page emits the
//!   math blocks; the math-free page does not.
//!
//! Math-mode is CDN-default (matches Pandoc / Quarto 1), so unlike
//! Bootstrap there are no on-disk assets to assert against.

use std::path::Path;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, project_type_for};
use quarto_core::render_to_file::{RenderToFileOptions, render_document_to_file, render_to_file};
use quarto_core::stage::stages::{DEFAULT_KATEX_URL_BASE, DEFAULT_MATHJAX_URL};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn html_format() -> Format {
    Format::html()
}

fn runtime_arc() -> Arc<dyn SystemRuntime> {
    Arc::new(NativeRuntime::new())
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

/// Sentinel substring our MathJax inline config block must contain.
/// Used to assert "the math stage populated `meta.math`" without
/// hardcoding the entire config text.
const MATHJAX_CONFIG_SENTINEL: &str = "window.MathJax";

/// Sentinel substring KaTeX init code must contain so it shows up in
/// the rendered HTML.
const KATEX_AUTO_RENDER_SENTINEL: &str = "renderMathInElement";

// ── Single-doc tests ────────────────────────────────────────────────────

/// `html-math-method` is an HTML option. A document that selects `mathml`
/// and is rendered to docx (a common shape: the key sits in shared
/// metadata of a project that renders both) must reach pandoc with its
/// `Inline::Math` intact and come out with a native OMML equation,
/// numbered by the vendored `crossref/equations.lua`. Before `MathMlStage`
/// was gated on the format it converted the math to a `RawInline` on the
/// Pandoc leg too, and that Lua filter crashed on it (pandoc exit 83).
/// Requires a real `pandoc` on `PATH`, like every Pandoc-leg test.
#[test]
fn mathml_method_is_ignored_for_docx() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().canonicalize().unwrap();
    let qmd_path = project_dir.join("doc.qmd");
    write_file(
        &qmd_path,
        "---\ntitle: Math\nhtml-math-method: mathml\n---\n\n$$\nE = mc^2\n$$ {#eq-e}\n\nSee @eq-e.\n",
    );

    let result = render_document_to_file(
        &qmd_path,
        "docx",
        &RenderToFileOptions::default(),
        None,
        runtime_arc(),
        None,
        None,
        Some("docx"),
    )
    .expect("docx render must succeed with html-math-method: mathml");
    let bytes = std::fs::read(&result.output_path).unwrap();
    let extraction = quarto_ooxml_extract::extract_docx(&bytes).expect("extract docx");

    assert_eq!(
        extraction.math.len(),
        1,
        "exactly one OMML equation expected; got {extraction}"
    );
    let math = &extraction.math[0];
    assert!(
        math.contains("mc") && math.contains("(1)"),
        "the equation must be native OMML, numbered by the vendored Lua: {math:?}"
    );
    let body: String = extraction
        .paragraphs
        .iter()
        .map(|p| p.text.as_str())
        .collect();
    assert!(
        !body.contains("<math"),
        "no MathML may leak into the docx body: {body}"
    );
}

/// Default config + a doc with inline math → both the MathJax inline
/// config block and the CDN loader land in the rendered HTML.
#[test]
fn single_doc_with_math_emits_mathjax_config_and_loader() {
    let temp = TempDir::new().unwrap();
    let qmd_path = temp.path().join("doc.qmd");
    write_file(
        &qmd_path,
        "---\ntitle: Math Test\n---\n\nThe equation $E = mc^2$ is famous.\n",
    );

    let runtime = runtime_arc();
    let options = RenderToFileOptions::default();
    let result =
        render_to_file(&qmd_path, "html", &options, runtime).expect("single-doc render failed");
    let html = read(&result.output_path);

    assert!(
        html.contains(MATHJAX_CONFIG_SENTINEL),
        "rendered HTML must contain MathJax inline config block (`{}`); got {} bytes",
        MATHJAX_CONFIG_SENTINEL,
        html.len()
    );
    assert!(
        html.contains(DEFAULT_MATHJAX_URL),
        "rendered HTML must reference default MathJax CDN (`{}`)",
        DEFAULT_MATHJAX_URL
    );
}

/// Display math (`$$y$$`) triggers the same injection as inline math.
#[test]
fn single_doc_with_display_math_emits_mathjax() {
    let temp = TempDir::new().unwrap();
    let qmd_path = temp.path().join("doc.qmd");
    write_file(
        &qmd_path,
        "---\ntitle: Math Test\n---\n\n$$y = mx + b$$\n\nThe end.\n",
    );

    let runtime = runtime_arc();
    let options = RenderToFileOptions::default();
    let result = render_to_file(&qmd_path, "html", &options, runtime).expect("render");
    let html = read(&result.output_path);

    assert!(html.contains(MATHJAX_CONFIG_SENTINEL));
    assert!(html.contains(DEFAULT_MATHJAX_URL));
}

/// Labelled equation (`$$x = y$$ {#eq-z}`) — the math stage must
/// detect the equation through the `CustomNode("Equation")` wrapper
/// inserted by `EquationLabelTransform`. Also verifies that
/// `CrossrefRenderTransform` injected `\tag{N}` so the equation
/// number renders.
#[test]
fn labelled_equation_emits_mathjax_and_tag() {
    let temp = TempDir::new().unwrap();
    let qmd_path = temp.path().join("doc.qmd");
    write_file(
        &qmd_path,
        "---\ntitle: Labelled\n---\n\n$$E = mc^2$$ {#eq-einstein}\n\nSee @eq-einstein.\n",
    );

    let runtime = runtime_arc();
    let options = RenderToFileOptions::default();
    let result = render_to_file(&qmd_path, "html", &options, runtime).expect("render");
    let html = read(&result.output_path);

    assert!(
        html.contains(MATHJAX_CONFIG_SENTINEL),
        "labelled equation must trigger MathJax injection"
    );
    assert!(
        html.contains("\\tag{1}"),
        "CrossrefRenderTransform must have appended \\tag{{1}} for the labelled equation"
    );
}

/// A math-free document: neither the MathJax config nor any loader
/// URL appears.
#[test]
fn single_doc_without_math_omits_mathjax() {
    let temp = TempDir::new().unwrap();
    let qmd_path = temp.path().join("doc.qmd");
    write_file(
        &qmd_path,
        "---\ntitle: No Math\n---\n\nJust plain text. No equations.\n",
    );

    let runtime = runtime_arc();
    let options = RenderToFileOptions::default();
    let result = render_to_file(&qmd_path, "html", &options, runtime).expect("render");
    let html = read(&result.output_path);

    assert!(
        !html.contains(MATHJAX_CONFIG_SENTINEL),
        "math-free doc must not emit MathJax config block"
    );
    assert!(
        !html.contains(DEFAULT_MATHJAX_URL),
        "math-free doc must not reference MathJax CDN"
    );
    assert!(
        !html.contains("cdn.jsdelivr.net/npm/mathjax"),
        "math-free doc must not link any mathjax CDN"
    );
}

/// `html-math-method: katex` → KaTeX CDN, no MathJax URL.
#[test]
fn katex_method_uses_katex_cdn() {
    let temp = TempDir::new().unwrap();
    let qmd_path = temp.path().join("doc.qmd");
    write_file(
        &qmd_path,
        "---\ntitle: KaTeX\nhtml-math-method: katex\n---\n\n$x = y$\n",
    );

    let runtime = runtime_arc();
    let options = RenderToFileOptions::default();
    let result = render_to_file(&qmd_path, "html", &options, runtime).expect("render");
    let html = read(&result.output_path);

    assert!(
        html.contains("katex"),
        "katex method must reference katex assets in HTML"
    );
    assert!(
        html.contains(DEFAULT_KATEX_URL_BASE),
        "katex method must reference default KaTeX CDN base ({}); html: {}...",
        DEFAULT_KATEX_URL_BASE,
        &html[..html.len().min(2000)]
    );
    assert!(
        html.contains(KATEX_AUTO_RENDER_SENTINEL),
        "katex method must include auto-render init code (`{}`)",
        KATEX_AUTO_RENDER_SENTINEL
    );
    assert!(
        !html.contains(DEFAULT_MATHJAX_URL),
        "katex method must NOT reference MathJax CDN"
    );
}

/// User-supplied URL via `html-math-method: { method: mathjax, url: ... }`
/// is honored verbatim; default CDN is replaced.
#[test]
fn custom_url_overrides_default_cdn() {
    let temp = TempDir::new().unwrap();
    let qmd_path = temp.path().join("doc.qmd");
    let custom = "https://example.com/my/mathjax/loader.js";
    write_file(
        &qmd_path,
        &format!(
            "---\ntitle: Custom URL\nhtml-math-method:\n  method: mathjax\n  url: \"{custom}\"\n---\n\n$x^2$\n"
        ),
    );

    let runtime = runtime_arc();
    let options = RenderToFileOptions::default();
    let result = render_to_file(&qmd_path, "html", &options, runtime).expect("render");
    let html = read(&result.output_path);

    assert!(
        html.contains(custom),
        "custom MathJax URL must appear in rendered HTML"
    );
    assert!(
        !html.contains(DEFAULT_MATHJAX_URL),
        "default MathJax CDN must be replaced by the user URL"
    );
}

// ── Multi-page website tests ────────────────────────────────────────────

fn render_website(fixture: impl FnOnce(&Path)) -> std::path::PathBuf {
    let temp = TempDir::new().unwrap();
    let project_dir = temp
        .path()
        .canonicalize()
        .unwrap_or_else(|_| temp.path().to_path_buf());
    std::mem::forget(temp);
    fixture(&project_dir);

    let runtime = runtime_arc();
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();
    let options = RenderToFileOptions::default();
    let project_type = project_type_for(&project);
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        html_format(),
        "html",
        &options,
        runtime.clone(),
    );
    let summary = pollster::block_on(pipeline.run()).expect("project render");
    assert!(
        !summary.has_failures(),
        "project render reported failures: {:?}",
        summary
    );
    project_dir
}

/// Two-page site where one page has math and one doesn't. Only the
/// math-bearing page should reference MathJax.
#[test]
fn website_only_math_page_references_mathjax() {
    let project_dir = render_website(|dir| {
        write_file(&dir.join("_quarto.yml"), "project:\n  type: website\n");
        write_file(
            &dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nNothing here.\n",
        );
        write_file(
            &dir.join("equations.qmd"),
            "---\ntitle: Equations\n---\n\nWe have $x + 1 = y$.\n",
        );
    });

    let site = project_dir.join("_site");
    let index_html = read(&site.join("index.html"));
    let eq_html = read(&site.join("equations.html"));

    assert!(
        eq_html.contains(MATHJAX_CONFIG_SENTINEL),
        "math-bearing page must emit MathJax config"
    );
    assert!(
        eq_html.contains(DEFAULT_MATHJAX_URL),
        "math-bearing page must reference MathJax CDN"
    );

    assert!(
        !index_html.contains(MATHJAX_CONFIG_SENTINEL),
        "math-free page must NOT emit MathJax config"
    );
    assert!(
        !index_html.contains(DEFAULT_MATHJAX_URL),
        "math-free page must NOT reference MathJax CDN"
    );
}

// ── html-math-method: mathml (bd-3evfzwal) ──────────────────────────────

/// Render `doc.qmd` with the given body under `html-math-method: mathml`
/// and return the HTML plus the render diagnostics.
fn render_mathml(body: &str) -> (String, Vec<quarto_error_reporting::DiagnosticMessage>) {
    let temp = TempDir::new().unwrap();
    let qmd_path = temp.path().join("doc.qmd");
    write_file(
        &qmd_path,
        &format!("---\ntitle: MathML\nhtml-math-method: mathml\n---\n\n{body}\n"),
    );
    let runtime = runtime_arc();
    let options = RenderToFileOptions::default();
    let result = render_to_file(&qmd_path, "html", &options, runtime).expect("render");
    let html = read(&result.output_path);
    (html, result.render_output.diagnostics)
}

/// Every expression converts: native `<math>` in the page, the TeX
/// delimiters gone, no JS engine loaded, and a numbered equation's label
/// sits outside the math.
#[test]
fn mathml_method_emits_native_mathml_and_no_loader() {
    let (html, diagnostics) = render_mathml(
        "Inline $x^2$ and display:\n\n$$\\frac{a}{b}$$\n\n$$E = mc^2$$ {#eq-e}\n\nSee @eq-e.",
    );
    assert!(
        html.contains(r#"<span class="math inline"><math xmlns="http://www.w3.org/1998/Math/MathML"><semantics><mrow><msup><mi>x</mi><mn>2</mn></msup></mrow>"#),
        "inline math must be native MathML inside the usual span; got:\n{html}"
    );
    assert!(
        html.contains(r#"<span class="math display"><math xmlns="http://www.w3.org/1998/Math/MathML" display="block"><semantics><mrow><mfrac><mi>a</mi><mi>b</mi></mfrac></mrow>"#),
        "display math must be block MathML; got:\n{html}"
    );
    assert!(
        html.contains(r#"<annotation encoding="application/x-tex">E = mc^2</annotation></semantics></math></span><span class="quarto-eq-number">(1)</span></span>"#),
        "the numbered equation keeps its TeX untouched and carries the label as a sibling; got:\n{html}"
    );
    assert!(
        !html.contains("\\("),
        "no inline TeX delimiters may remain:\n{html}"
    );
    assert!(
        !html.contains("\\["),
        "no display TeX delimiters may remain:\n{html}"
    );
    assert!(!html.contains("\\tag{"));
    assert!(
        !html.contains(MATHJAX_CONFIG_SENTINEL),
        "no MathJax when everything converted"
    );
    assert!(!html.contains("cdn.jsdelivr.net/npm/mathjax"));
    assert!(!html.contains("cdn.jsdelivr.net/npm/katex"));
    assert!(
        html.contains("Equation\u{a0}1"),
        "the crossref link still resolves"
    );
    let math_codes: Vec<&str> = diagnostics
        .iter()
        .filter_map(|d| d.code.as_deref())
        .filter(|c| c.starts_with("Q-22-"))
        .collect();
    assert!(
        math_codes.is_empty(),
        "no math diagnostics expected, got {math_codes:?}"
    );
}

/// An expression the converter rejects stays TeX, and MathJax is loaded
/// for the page so the reader still sees it rendered (plan decision 2).
/// The author gets a `Q-22-1` *warning* pointing at the command.
#[test]
fn mathml_method_falls_back_to_mathjax_for_an_unconvertible_expression() {
    let (html, diagnostics) = render_mathml("Good $a^2$ and bad $x + \\bogus y$.");
    assert!(
        html.contains("<mrow><msup><mi>a</mi><mn>2</mn></msup></mrow>"),
        "the convertible expression is MathML; got:\n{html}"
    );
    assert!(
        html.contains(r#"<span class="math inline">\(x + \bogus y\)</span>"#),
        "the unconvertible expression stays TeX for MathJax; got:\n{html}"
    );
    assert!(
        html.contains(MATHJAX_CONFIG_SENTINEL) && html.contains(DEFAULT_MATHJAX_URL),
        "MathJax must be loaded for the leftover expression; got:\n{html}"
    );
    // …and must not re-typeset the TeX inside the converted math's
    // `<annotation>` (MathJax's default skip list covers it; ours must too).
    assert!(
        html.contains("'annotation', 'annotation-xml'"),
        "the MathJax skip list must include annotation elements; got:\n{html}"
    );
    let bogus: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code.as_deref() == Some("Q-22-1"))
        .collect();
    assert_eq!(
        bogus.len(),
        1,
        "one unknown-command diagnostic; got {diagnostics:?}"
    );
    assert_eq!(
        bogus[0].kind,
        quarto_error_reporting::DiagnosticKind::Warning,
        "the page still renders (MathJax), so this is a warning, not an error"
    );
    assert!(
        bogus[0].location.is_some(),
        "the diagnostic points into the .qmd"
    );
}

/// No math: neither `<math>` nor a loader.
#[test]
fn mathml_method_without_math_emits_nothing() {
    let (html, _) = render_mathml("Just prose.");
    assert!(!html.contains("<math"));
    assert!(!html.contains(MATHJAX_CONFIG_SENTINEL));
}

/// Math is converted wherever it sits: headers, list items, table cells,
/// and footnotes.
#[test]
fn mathml_method_reaches_math_in_every_container() {
    let (html, _) = render_mathml(
        "# Header $h^2$\n\n- item $i^2$\n\n| a |\n|---|\n| $t^2$ |\n\nText^[note $n^2$]",
    );
    for var in ["h", "i", "t", "n"] {
        assert!(
            html.contains(&format!("<msup><mi>{var}</mi><mn>2</mn></msup>")),
            "math in a container ({var}) must be converted; got:\n{html}"
        );
    }
    assert!(!html.contains("\\("));
    assert!(!html.contains(MATHJAX_CONFIG_SENTINEL));
}

/// Two-page site: the mathml page gets `<math>` and no loader; the
/// math-free page gets neither.
#[test]
fn website_mathml_page_has_math_and_no_loader() {
    let project_dir = render_website(|dir| {
        write_file(
            &dir.join("_quarto.yml"),
            "project:\n  type: website\nformat:\n  html:\n    html-math-method: mathml\n",
        );
        write_file(
            &dir.join("index.qmd"),
            "---\ntitle: Home\n---\n\nNothing here.\n",
        );
        write_file(
            &dir.join("equations.qmd"),
            "---\ntitle: Equations\n---\n\nWe have $x + 1 = y$.\n",
        );
    });
    let site = project_dir.join("_site");
    let index_html = read(&site.join("index.html"));
    let eq_html = read(&site.join("equations.html"));
    assert!(eq_html.contains("<math xmlns="), "got:\n{eq_html}");
    assert!(!eq_html.contains(MATHJAX_CONFIG_SENTINEL));
    assert!(!index_html.contains("<math"));
    assert!(!index_html.contains(MATHJAX_CONFIG_SENTINEL));
}
