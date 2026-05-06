/*
 * tests/render_page_in_project.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Phase 9 sub-phase 9.5: end-to-end native test of the same
 * code path the WASM `render_page_in_project` entry point drives
 * — `ProjectPipeline<RenderToHtmlRenderer>` with
 * `RenderMode::ActivePage(target)`.
 */

//! Integration tests for the WASM Pass-2 renderer.
//!
//! These run on **native** (using the same `NativeRuntime` the
//! CLI uses) but exercise the *exact* renderer the WASM
//! `render_page_in_project` entry point uses — `RenderToHtmlRenderer`
//! returning [`WasmPassTwoOutput`] in-memory rather than writing to
//! disk. The HTML is inspected for sidebar entries, cross-document
//! link rewriting, page-scope artifacts, and project-scope artifact
//! flush via `flush_site_libs`.
//!
//! Pinning native coverage on this code path means a regression in
//! the project-rendering machinery surfaces in `cargo nextest run`
//! before it reaches the browser. The browser smoke (sub-phase 9.5)
//! then confirms the same path works against MorphIframe + Monaco.
//!
//! See `claude-notes/plans/2026-04-27-websites-phase-9.md` §Tests
//! 11–14 (refraled around the unified entry point).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, RenderMode, project_type_for};
use quarto_core::project::pass2_renderer::{
    RenderToHtmlRenderer, RenderToPreviewAstRenderer, WasmPassTwoOutput,
};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Drive `ProjectPipeline<RenderToHtmlRenderer>` with
/// `RenderMode::ActivePage(active)`. Mirrors what the WASM
/// `render_page_in_project` entry point does, just against the
/// native filesystem instead of the in-memory VFS.
///
/// `vfs_root` is the path the WASM renderer would synthesize as
/// the artifact root. In WASM that's an absolute path under the
/// in-memory VFS (`/.quarto/project-artifacts`). On native
/// `NativeRuntime`, we point it at a real subdirectory of the
/// temp test fixture so `flush_site_libs` can actually write
/// without bumping into read-only system paths.
fn render_active_page(project_dir: &Path, active: &Path) -> WasmPassTwoOutput {
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    // Discover from the active file first — that locates the
    // `_quarto.yml` and tells us whether this is a single-file or
    // multi-file project. When it's multi-file, re-discover from
    // the project root so `project.files` contains every sibling
    // (the active-file form returns just `[active]`, which would
    // starve Pass-1 of every other file's profile and break the
    // sidebar's title resolution and the cross-doc link rewriter).
    let mut project = ProjectContext::discover(active, runtime.as_ref()).unwrap();
    if !project.is_single_file {
        project = ProjectContext::discover(&project.dir, runtime.as_ref()).unwrap();
    }
    let _ = project_dir; // canonicalization happens via `project.dir`

    let project_type = project_type_for(&project);
    let vfs_root = project.dir.join(".quarto/project-artifacts");
    let renderer = RenderToHtmlRenderer::new(&vfs_root);

    let mut pipeline = ProjectPipeline::with_renderer(
        &mut project,
        project_type,
        Format::html(),
        "html",
        runtime.clone(),
        renderer,
    )
    .with_mode(RenderMode::ActivePage(active.to_path_buf()));

    let summary = pollster::block_on(pipeline.run()).expect("pipeline run");
    assert!(
        summary.pass1_failures.is_empty(),
        "unexpected pass-1 failures: {:?}",
        summary.pass1_failures,
    );
    assert!(
        summary.pass2_failures.is_empty(),
        "unexpected pass-2 failures: {:?}",
        summary.pass2_failures,
    );
    assert_eq!(
        summary.outputs.len(),
        1,
        "ActivePage mode should produce exactly one output"
    );
    summary.outputs.into_iter().next().unwrap()
}

/// Plan test 8 (refraled): a two-file website fixture renders the
/// active page with the sibling listed in the sidebar.
#[test]
fn website_sidebar_includes_sibling_pages() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());

    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\nwebsite:\n  title: Test\n  sidebar:\n    contents:\n      - index.qmd\n      - about.qmd\n",
    );
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nHello.\n",
    );
    write(
        &project_dir.join("about.qmd"),
        "---\ntitle: About\n---\n\nAbout this site.\n",
    );

    let active = canonical(&project_dir.join("index.qmd"));
    let output = render_active_page(&project_dir, &active);

    assert!(
        output.html().contains("class=\"sidebar"),
        "rendered HTML should contain the sidebar block; got {}",
        snippet(&output.html())
    );
    assert!(
        output.html().contains(">About<") || output.html().contains(">About\n<"),
        "sidebar should reference the sibling 'About' entry; got {}",
        snippet(&output.html())
    );
    // The vfs_root resolver makes URLs absolute under the synthetic
    // root; the cross-doc link rewriter still resolves the page
    // identity to `about.html`, just prefixed with the vfs root.
    assert!(
        output.html().contains("about.html\""),
        "sidebar entry for about.qmd should rewrite to about.html; got {}",
        snippet(&output.html())
    );
}

/// Plan test 9 (refraled): editing a sibling's frontmatter title
/// changes the rendered sidebar entry on the next render. The
/// hub-client's "any-edit triggers re-render" behavior relies on
/// this.
#[test]
fn sibling_title_edit_reflects_in_sidebar() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());

    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\nwebsite:\n  sidebar:\n    contents:\n      - index.qmd\n      - about.qmd\n",
    );
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nHello.\n",
    );
    write(
        &project_dir.join("about.qmd"),
        "---\ntitle: About v1\n---\n\nFirst version.\n",
    );

    let active = canonical(&project_dir.join("index.qmd"));
    let first = render_active_page(&project_dir, &active);
    assert!(
        first.html().contains(">About v1<"),
        "first render should show 'About v1'; got {}",
        snippet(&first.html())
    );

    // Edit about.qmd's title and re-render the (unchanged) index.
    write(
        &project_dir.join("about.qmd"),
        "---\ntitle: About v2\n---\n\nFirst version.\n",
    );
    let second = render_active_page(&project_dir, &active);
    assert!(
        second.html().contains(">About v2<"),
        "second render should reflect the new sibling title; got {}",
        snippet(&second.html())
    );
    assert!(
        !second.html().contains(">About v1<"),
        "second render should *not* still show the old title; got {}",
        snippet(&second.html())
    );
}

/// Plan test 10 (refraled): single-file render (no `_quarto.yml`)
/// produces HTML with no sidebar block.
#[test]
fn single_file_no_sidebar() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());

    write(
        &project_dir.join("only.qmd"),
        "---\ntitle: Only\n---\n\nA single document.\n",
    );

    let active = canonical(&project_dir.join("only.qmd"));
    let output = render_active_page(&project_dir, &active);

    assert!(
        !output.html().contains("class=\"sidebar"),
        "single-file render should have no sidebar; got {}",
        snippet(&output.html())
    );
    assert!(
        output.html().contains("Only"),
        "rendered HTML should contain the page title; got {}",
        snippet(&output.html())
    );
}

/// Plan test 12: cross-document link rewriting. `[link](b.qmd)` in
/// the active page renders as `href="b.html"`.
#[test]
fn cross_document_link_rewrites_to_html() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());

    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\nwebsite:\n  sidebar:\n    contents:\n      - index.qmd\n      - about.qmd\n",
    );
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nSee [about](about.qmd).\n",
    );
    write(
        &project_dir.join("about.qmd"),
        "---\ntitle: About\n---\n\nA.\n",
    );

    let active = canonical(&project_dir.join("index.qmd"));
    let output = render_active_page(&project_dir, &active);

    // Under the WASM `vfs_root` resolver every internal href is
    // emitted as an absolute URL prefixed with the synthetic root
    // (`/{vfs_root}/about.html`). The cross-doc rewrite is what
    // resolves `about.qmd` → `about.html`; the prefix is the
    // resolver's job. Match on the suffix to stay agnostic to the
    // prefix while still asserting the .qmd → .html rewrite.
    assert!(
        output.html().contains("about.html\""),
        "[link](about.qmd) should rewrite to ...about.html; got {}",
        snippet(&output.html())
    );
    // And there must be NO `about.qmd` reference left in the body —
    // every internal-doc reference must have been rewritten.
    assert!(
        !output.html().contains("about.qmd\""),
        "no rewritten about.qmd should remain in body; got {}",
        snippet(&output.html())
    );
}

/// Plan test 14: per-page transforms still fire under the WASM
/// renderer — title prefix is applied (`<title>Home – Test</title>`).
#[test]
fn title_prefix_applied_in_website_render() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());

    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\nwebsite:\n  title: Test Site\n",
    );
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nHello.\n",
    );

    let active = canonical(&project_dir.join("index.qmd"));
    let output = render_active_page(&project_dir, &active);

    // Phase-7 title-prefix transform: the page title is suffixed
    // with the website title separated by an en-dash.
    assert!(
        output.html().contains("<title>Home – Test Site</title>"),
        "title prefix should be applied; got {}",
        snippet(&output.html())
    );
}

/// Plan test 15-equivalent (the 'hub-smoke' fixture): the
/// committed fixture under
/// `crates/quarto-core/tests/fixtures/websites/hub-smoke/` renders
/// the active `index.qmd` cleanly — sidebar contains all three
/// entries, cross-doc link rewrites, no failures.
#[test]
fn hub_smoke_fixture_renders_cleanly() {
    // Copy the fixture into a temp dir so canonicalize can resolve
    // it without the read-only `target/` interleaving.
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    copy_fixture("websites/hub-smoke", &project_dir);

    let active = canonical(&project_dir.join("index.qmd"));
    let output = render_active_page(&project_dir, &active);

    // Under the WASM `vfs_root` resolver, all internal hrefs
    // (sidebar, body links, page-nav, …) are emitted as absolute
    // URLs prefixed by the synthetic root. Match on the suffix to
    // stay agnostic to the prefix.
    assert!(
        output.html().contains("about.html\""),
        "sidebar should link to ...about.html; got {}",
        snippet(&output.html())
    );
    assert!(
        output.html().contains("posts/first.html\""),
        "sidebar should link to ...posts/first.html; got {}",
        snippet(&output.html())
    );

    // Cross-doc body link from index.qmd's body rewrites too —
    // expect at least two `about.html` references (one body, one
    // sidebar). Both forms now route through `page_url_for` so the
    // emitted URL is identical at both call sites; counting suffix
    // matches stays robust to the resolver flavor.
    let about_links = output.html().matches("about.html\"").count();
    assert!(
        about_links >= 2,
        "expected at least one body href + one sidebar href ending in about.html; got {} matches",
        about_links
    );
}

/// Drive `ProjectPipeline<RenderToHtmlRenderer>` like
/// `render_active_page` but tolerate Pass-1 failures so callers
/// can inspect the failure record. Returns the full
/// `ProjectRenderSummary` rather than a single output. Used by
/// the bd-mwtf regression tests.
fn run_active_page_summary(
    active: &Path,
) -> quarto_core::project::orchestrator::ProjectRenderSummary<WasmPassTwoOutput> {
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(active, runtime.as_ref()).unwrap();
    if !project.is_single_file {
        project = ProjectContext::discover(&project.dir, runtime.as_ref()).unwrap();
    }
    let project_type = project_type_for(&project);
    let vfs_root = project.dir.join(".quarto/project-artifacts");
    let renderer = RenderToHtmlRenderer::new(&vfs_root);

    let mut pipeline = ProjectPipeline::with_renderer(
        &mut project,
        project_type,
        Format::html(),
        "html",
        runtime.clone(),
        renderer,
    )
    .with_mode(RenderMode::ActivePage(active.to_path_buf()));

    pollster::block_on(pipeline.run()).expect("pipeline run")
}

/// Regression for bd-rqba: when a *sibling* page fails Pass-1
/// (parse error) but the active page itself parses fine, the
/// active-page render still succeeds and the orchestrator
/// surfaces the sibling's failure on `pass1_failures`. The
/// WASM `render_page_in_project` entry point puts these in
/// `RenderResponse.pass1_failures` so the overlay can show
/// "about.qmd failed to parse" with line/column instead of
/// just the misleading "missing document information for
/// 'about.qmd'" navigation warning.
#[test]
fn pass1_parse_error_in_sibling_surfaces_alongside_active_render() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());

    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\nwebsite:\n  sidebar:\n    contents:\n      - index.qmd\n      - about.qmd\n",
    );
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nHello.\n",
    );
    // Same Q-2-10 quote-mark error as the bd-mwtf test.
    write(
        &project_dir.join("about.qmd"),
        "---\ntitle: About\n---\n\n- Reflect changes to *other* pages' titles within the next render\n",
    );

    let active = canonical(&project_dir.join("index.qmd"));
    let summary = run_active_page_summary(&active);

    // Active page renders fine.
    assert_eq!(
        summary.outputs.len(),
        1,
        "active page should render successfully"
    );
    // Sibling pass-1 failure is reported.
    assert_eq!(
        summary.pass1_failures.len(),
        1,
        "expected exactly one Pass-1 failure for the sibling"
    );
    let failure = &summary.pass1_failures[0];
    assert_eq!(failure.input, canonical(&project_dir.join("about.qmd")));
    assert!(
        !failure.diagnostics.is_empty(),
        "sibling Pass-1 failure should carry structured diagnostics"
    );
    assert!(failure.source_context.is_some());

    // The renamed nav warning (D2) is the project-scoped warning
    // surfaced when the sidebar references the dropped sibling.
    // Confirm the new wording is in place. It rides on a per-page
    // render output's `diagnostics` *or* on `summary.project_diagnostics`
    // depending on which transform emitted it; check both.
    let active_output = &summary.outputs[0];
    let combined: Vec<String> = active_output
        .diagnostics
        .iter()
        .chain(summary.project_diagnostics.iter())
        .map(|d| d.title.clone())
        .collect();
    assert!(
        combined
            .iter()
            .any(|m| m.contains("missing document information")),
        "expected the renamed 'missing document information' warning; got: {:?}",
        combined,
    );
    assert!(
        combined
            .iter()
            .all(|m| !m.contains("references unknown document")),
        "old 'references unknown document' wording should be gone; got: {:?}",
        combined,
    );
}

/// Regression for bd-mwtf: when the active page itself fails
/// Pass-1 (parse error), the orchestrator surfaces it via
/// `pass1_failures`, with **structured** diagnostics + a
/// SourceContext attached. The hub-client's WASM entry point
/// uses these to render the parse error in the preview overlay
/// instead of falling through to the generic "no output" string.
#[test]
fn pass1_parse_error_on_active_page_carries_diagnostics() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());

    // Two-file website where `about.qmd` contains an unescaped
    // apostrophe Q2 reads as a Q-2-10 quote-mark error
    // (`pages'` in `*other* pages' titles`).
    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\nwebsite:\n  sidebar:\n    contents:\n      - index.qmd\n      - about.qmd\n",
    );
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nHello.\n",
    );
    write(
        &project_dir.join("about.qmd"),
        "---\ntitle: About\n---\n\n- Reflect changes to *other* pages' titles within the next render\n",
    );

    let active = canonical(&project_dir.join("about.qmd"));
    let summary = run_active_page_summary(&active);

    // Active page is dropped from outputs; pass-2 runs but has
    // nothing to render for the active page.
    assert_eq!(
        summary.outputs.len(),
        0,
        "outputs should be empty when the active page fails Pass-1"
    );
    assert_eq!(
        summary.pass1_failures.len(),
        1,
        "expected exactly one Pass-1 failure for about.qmd"
    );

    let failure = &summary.pass1_failures[0];
    assert_eq!(failure.input, active);
    assert!(
        !failure.diagnostics.is_empty(),
        "FileFailure.diagnostics should be populated for parse errors; \
         got error string: {}",
        failure.error,
    );
    assert!(
        failure.source_context.is_some(),
        "FileFailure.source_context should be Some for parse errors so \
         JsonDiagnostic can map offsets to line/column",
    );

    // The user-facing error string still contains the rendered
    // ariadne snippet (since `e.to_string()` for QuartoError::Parse
    // calls ParseError::Display); the CLI's text path depends on
    // this remaining intact.
    assert!(
        failure.error.contains("Q-2-10") || failure.error.to_lowercase().contains("quote"),
        "error string should mention the quote-mark diagnostic; got: {}",
        failure.error,
    );
}

/// Regression for bd-87fu: in a default project (no `_quarto.yml`
/// `type: website`, so `lib_dir == ""`), the WASM Pass-2 renderer
/// must flush Project-scope artifacts (e.g. theme CSS) so the
/// iframe post-processor can find them at the URL the rendered
/// HTML embeds.
///
/// Pre-fix: `RenderToHtmlRenderer.render` always drained Project-
/// scope artifacts into the orchestrator accumulator, but
/// `DefaultProjectType.post_render` was a no-op — so theme bytes
/// vanished. The HTML embedded a `<link>` to a VFS path with no
/// matching file.
///
/// Post-fix: `RenderToHtmlRenderer.render` mirrors the native
/// `render_document_to_file` lib_dir branch — when `lib_dir` is
/// empty, Project-scope artifacts are written in-place via the
/// per-page (vfs_root) resolver and the URL/on-disk paths
/// round-trip.
#[test]
fn default_project_theme_artifact_lands_in_vfs() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());

    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: default\n",
    );
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: T\nformat:\n  html:\n    theme: flatly\n---\n\nhi\n",
    );

    let active = canonical(&project_dir.join("index.qmd"));
    let output = render_active_page(&project_dir, &active);

    // The HTML should embed a `<link>` to a quarto theme CSS file
    // under the synthetic vfs root.
    let vfs_root = project_dir.join(".quarto/project-artifacts");
    let vfs_root_str = vfs_root.to_string_lossy().to_string();
    let needle_prefix = format!("{}/quarto/quarto-theme-", vfs_root_str);
    let theme_link = output
        .html()
        .lines()
        .filter(|line| line.contains(&needle_prefix) && line.contains(".css"))
        .next()
        .unwrap_or_else(|| {
            panic!(
                "expected a theme <link> under {}quarto/quarto-theme-…; html: {}",
                vfs_root_str,
                snippet(&output.html()),
            )
        });

    // Extract the actual CSS path from the href attribute and
    // confirm the bytes landed at that path.
    let href_start = theme_link
        .find(&needle_prefix)
        .expect("needle present (filter just confirmed it)");
    let after_prefix = &theme_link[href_start..];
    let css_end = after_prefix
        .find(".css")
        .map(|i| href_start + i + ".css".len())
        .expect("href ends with .css");
    let css_path_str = &theme_link[href_start..css_end];
    let css_path = PathBuf::from(css_path_str);

    let runtime = NativeRuntime::new();
    let bytes = runtime.file_read(&css_path).unwrap_or_else(|e| {
        panic!(
            "expected theme CSS to be flushed to {}; runtime read failed: {}",
            css_path.display(),
            e,
        )
    });
    assert!(
        !bytes.is_empty(),
        "theme CSS at {} should be non-empty",
        css_path.display(),
    );
    let css_text = std::str::from_utf8(&bytes).unwrap_or("");
    assert!(
        css_text.contains("flatly") || css_text.contains("body") || css_text.contains(":root"),
        "theme CSS at {} should look like compiled CSS; first 200 bytes: {}",
        css_path.display(),
        snippet(css_text),
    );
}

fn copy_fixture(rel: &str, dst: &Path) {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel);
    copy_dir_recursive(&src, dst);
}

fn copy_dir_recursive(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to);
        } else {
            std::fs::copy(&from, &to).unwrap();
        }
    }
}

fn snippet(s: impl AsRef<str>) -> String {
    let s = s.as_ref();
    if s.len() <= 200 {
        s.to_string()
    } else {
        format!("{}…", &s[..200])
    }
}

// ─── q2-preview Plan 1 commit 5 ─────────────────────────────────────
//
// Native E2E coverage of the orchestrator path through
// `RenderToPreviewAstRenderer`. Mirrors `render_active_page` (HTML)
// but constructs a q2-preview renderer; both renderers share
// `Output = WasmPassTwoOutput` (commit 2's enum-payload payoff), so
// the orchestrator and summary handling are identical and the only
// observable divergence is the payload variant.

/// Drive `ProjectPipeline<RenderToPreviewAstRenderer>` with
/// `RenderMode::ActivePage(active)`. Sibling of [`render_active_page`]
/// — same project discovery and orchestrator wiring; differs only
/// in the renderer choice and (consequently) the payload variant
/// of the resulting `WasmPassTwoOutput`.
fn render_active_page_preview(project_dir: &Path, active: &Path) -> WasmPassTwoOutput {
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(active, runtime.as_ref()).unwrap();
    if !project.is_single_file {
        project = ProjectContext::discover(&project.dir, runtime.as_ref()).unwrap();
    }
    let _ = project_dir;

    let project_type = project_type_for(&project);
    let vfs_root = project.dir.join(".quarto/project-artifacts");
    let renderer = RenderToPreviewAstRenderer::new(&vfs_root);

    // The orchestrator reads `format` to drive Pass-1 + Pass-2.
    // For q2-preview the format is HTML-based with
    // `pipeline_kind = Some("preview")`; that's what the renderer
    // and `AstTransformsStage` dispatch on.
    let format =
        Format::from_format_string("q2-preview").expect("q2-preview is a recognized pseudo-format");

    let mut pipeline = ProjectPipeline::with_renderer(
        &mut project,
        project_type,
        format,
        "q2-preview",
        runtime.clone(),
        renderer,
    )
    .with_mode(RenderMode::ActivePage(active.to_path_buf()));

    let summary = pollster::block_on(pipeline.run()).expect("q2-preview pipeline run");
    assert!(
        summary.pass1_failures.is_empty(),
        "unexpected pass-1 failures: {:?}",
        summary.pass1_failures,
    );
    assert!(
        summary.pass2_failures.is_empty(),
        "unexpected pass-2 failures: {:?}",
        summary.pass2_failures,
    );
    assert_eq!(
        summary.outputs.len(),
        1,
        "ActivePage mode should produce exactly one output"
    );
    summary.outputs.into_iter().next().unwrap()
}

/// Asserts the q2-preview output's payload is `Pass2Payload::AstJson`.
/// Convenience companion to [`WasmPassTwoOutput::html`] (the panicking
/// HTML accessor used by HTML tests above).
fn ast_json(output: &WasmPassTwoOutput) -> &str {
    output
        .payload
        .as_ast_json()
        .expect("q2-preview renderer must produce Pass2Payload::AstJson")
}

/// q2-preview commit 5 E2E: a website fixture with a callout and an
/// embedded image renders through `RenderToPreviewAstRenderer`,
/// producing AST JSON with:
/// - the callout encoded as `__quarto_custom_node` Div with
///   `data-custom-type=Callout` (preserves the wrapper for React);
/// - the embedded image's URL rewritten under the synthetic
///   `vfs_root` (matching the path the renderer flushes the image
///   bytes to);
/// - sidebar metadata populated via `SidebarGenerateTransform`
///   (which is in the q2-preview transform list).
///
/// This is the primary regression test for the wiring this commit
/// adds (single-doc + project-active dispatch on `pipeline_kind`).
/// It guards Plan 1's "Multi-plan contract: page-scoped image
/// artifacts" — Plan 2 will rely on the embedded URL and the VFS
/// path agreeing.
#[test]
fn website_q2_preview_renders_through_orchestrator() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());

    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\n\
         website:\n  title: Test Site\n  \
         sidebar:\n    contents:\n      - index.qmd\n      - about.qmd\n",
    );
    write(
        &project_dir.join("about.qmd"),
        "---\ntitle: About\n---\n\nAbout this site.\n",
    );

    // ResourceCollectorTransform reads the file path lazily and
    // doesn't validate image bytes, so any contents work for the
    // wiring test. The `write` helper takes &str, so use ASCII.
    write(
        &project_dir.join("hero.png"),
        "fake image bytes for q2-preview test",
    );

    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\nformat: q2-preview\n---\n\n\
         ::: {.callout-note}\n## Heads-up\n\nWelcome.\n:::\n\n\
         ![Hero](hero.png)\n",
    );

    let active = canonical(&project_dir.join("index.qmd"));
    let output = render_active_page_preview(&project_dir, &active);

    let json = ast_json(&output);
    let snip = || snippet(json);

    // Wrapper survives → React's CustomNode component (Plan 2)
    // can dispatch on type-name.
    assert!(
        json.contains("__quarto_custom_node"),
        "expected wrapper class in q2-preview AST JSON; got:\n{}",
        snip()
    );
    assert!(
        json.contains("data-custom-type"),
        "expected data-custom-type attribute; got:\n{}",
        snip()
    );
    assert!(
        json.contains("Callout"),
        "expected Callout type-name in JSON; got:\n{}",
        snip()
    );

    // ResourceCollectorTransform rewrites the image URL relative
    // to the resolver's vfs_root. The renderer flushes the bytes
    // to a path under `<project_dir>/.quarto/project-artifacts/`
    // (the native test's stand-in for the WASM VFS).
    assert!(
        json.contains("hero"),
        "expected the image filename to appear in the AST URL; got:\n{}",
        snip()
    );
    // page_artifacts carries the image as an artifact entry.
    assert!(
        !output.page_artifacts.is_empty(),
        "expected ResourceCollectorTransform to emit a page-scoped \
         artifact for hero.png; got empty page_artifacts"
    );

    // SidebarGenerateTransform is in the q2-preview transform
    // list; with a website project + sidebar config, the
    // structured `navigation.sidebar` should land in `meta`.
    // Sniff the JSON for the sibling page's title (title resolution
    // is a sidebar-generate side effect when ProjectIndex is
    // populated).
    assert!(
        json.contains("\"title\"") && json.contains("About"),
        "expected sidebar metadata to include the sibling 'About' \
         entry's title; got:\n{}",
        snip()
    );
}
