//! bd-knitr-inline-r-eats-fence-2ofk91x1: an R **display** fence — a fence
//! carrying a language but no braces — must render as a highlighted,
//! non-executed code block in a document the knitr engine runs, exactly as
//! Quarto 1 does.
//!
//! Before the fix it was a fatal parse error that cost the whole page. Two
//! stages compounded: the qmd writer collapses `` ``` r ``, `` ```{.r} `` and
//! `` ```r `` into the single spelling `` ```r ``, and the knitr inline-R
//! preprocessor's `` `r\s+([^`]+)` `` then anchored on that fence's *third*
//! backtick, let `\s+` eat the newline, and swallowed the block body up to
//! the closing fence.
//!
//! **Why this lives at the render level rather than in `preprocess.rs`.** The
//! unit tests there pin the regex against the collapsed spelling directly.
//! They cannot see stage 1 — that all three author spellings *become* that
//! one spelling is a property of the writer, and only a real render exercises
//! writer and preprocessor together. This file therefore renders each
//! spelling through `render_document_to_file`, the entry `q2 render` itself
//! uses, and asserts on the HTML.
//!
//! Every fixture carries an executable `` ```{r} `` cell: the knitr engine
//! must actually run for the bug to be reachable at all.
//!
//! Tests skip when knitr isn't installed.

#![cfg(not(target_arch = "wasm32"))]

use std::path::Path;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::ProjectContext;
use quarto_core::engine::EngineRegistry;
use quarto_core::render_to_file::{RenderToFileOptions, render_document_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn knitr_available() -> bool {
    EngineRegistry::default()
        .get("knitr")
        .is_some_and(|e| e.is_available())
}

/// A document with one executable R cell followed by a display fence written
/// as `fence` (the whole opening line, e.g. ``"``` r"``).
fn doc_with_display_fence(fence: &str) -> String {
    format!(
        "---\ntitle: Display fence\nengine: knitr\n---\n\nBefore.\n\n\
         ```{{r}}\ncat(paste0(\"O\", \"UT\"), \"\\n\")\n```\n\n\
         {fence}\npak::pak(c(\"usethis\", \"cli\"))\n```\n\nAfter.\n"
    )
}

/// Render `content` to HTML through the real render path and return the HTML.
/// Panics with the render error on failure — which is the assertion for the
/// whole-page-loss symptom.
fn render_html(tmp: &TempDir, name: &str, content: &str) -> String {
    let input = tmp.path().join(name);
    std::fs::write(&input, content).unwrap();

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&input, runtime.as_ref())
        .expect("project discovery for the display-fence fixture");

    let result = render_document_to_file(
        &input,
        "html",
        &RenderToFileOptions::default(),
        Some(&project),
        runtime.clone(),
        None,
        None,
        None,
    )
    .unwrap_or_else(|e| panic!("render must succeed, but the display fence made it fail: {e}"));

    read_html(&result.output_path)
}

fn read_html(path: &Path) -> String {
    std::fs::read_to_string(path).expect("read rendered HTML")
}

/// Assert the display fence survived as a *non-executed*, highlighted block,
/// and that the executable cell next to it still ran.
fn assert_fence_rendered(html: &str, spelling: &str) {
    // The engine ran: `OUT` exists only at runtime (the source says
    // `paste0("O", "UT")`), so this cannot be satisfied by an echo.
    assert!(
        html.contains("OUT"),
        "{spelling}: the executable cell must still run"
    );
    // The display block is highlighted as R, i.e. the highlighter resolved
    // the language rather than the block arriving mangled.
    assert!(
        html.contains("sourceCode r"),
        "{spelling}: display fence must render as a highlighted R block; html:\n{html}"
    );
    // Its body survived intact.
    assert!(
        html.contains("pak"),
        "{spelling}: display fence body must survive"
    );
    // The smoking gun of the old failure, in case it ever returns in a
    // non-fatal form: the wrapper must never appear in the output.
    assert!(
        !html.contains("QuartoInlineRender"),
        "{spelling}: the inline-R wrapper must not reach the output"
    );
}

/// The spelling Pandoc and Quarto 1 accept, and the one knitr's own `.Rmd`
/// output writes: three backticks, a space, the language.
#[test]
fn display_fence_with_space_renders() {
    if !knitr_available() {
        eprintln!("SKIP: knitr not available — display_fence_with_space_renders");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let html = render_html(&tmp, "space.qmd", &doc_with_display_fence("``` r"));
    assert_fence_rendered(&html, "``` r");
}

/// The no-space spelling — the one every other spelling collapses to.
#[test]
fn display_fence_without_space_renders() {
    if !knitr_available() {
        eprintln!("SKIP: knitr not available — display_fence_without_space_renders");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let html = render_html(&tmp, "nospace.qmd", &doc_with_display_fence("```r"));
    assert_fence_rendered(&html, "```r");
}

/// The canonical Pandoc attribute spelling.
#[test]
fn display_fence_with_pandoc_attr_renders() {
    if !knitr_available() {
        eprintln!("SKIP: knitr not available — display_fence_with_pandoc_attr_renders");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let html = render_html(&tmp, "attr.qmd", &doc_with_display_fence("```{.r}"));
    assert_fence_rendered(&html, "```{.r}");
}

/// A four-backtick fence, which the writer emits when the body contains a
/// backtick. knitr's own lookbehinds are anchored to a line's first two
/// backticks and so miss this shape entirely; our prefix guard is not
/// anchored and covers it like any other fence.
#[test]
fn four_backtick_display_fence_renders() {
    if !knitr_available() {
        eprintln!("SKIP: knitr not available — four_backtick_display_fence_renders");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let content = "---\ntitle: Wide fence\nengine: knitr\n---\n\n\
                   ```{r}\ncat(paste0(\"O\", \"UT\"), \"\\n\")\n```\n\n\
                   ````r\nx <- `y`\n````\n\nAfter.\n";
    let html = render_html(&tmp, "wide.qmd", content);
    assert!(html.contains("OUT"), "the executable cell must still run");
    // Without this the test passes even if the fence body were swallowed:
    // "renders at all" and "wrapper absent" are both satisfied by an empty
    // block. Assert on the backtick *inside* the body — the thing that makes
    // the writer widen the fence in the first place, and the byte the old
    // pattern used as its closing delimiter. (Not on `x <- `: the highlighter
    // splits that across spans.)
    assert!(
        html.contains("sourceCode r"),
        "the wide fence must render as a highlighted R block; html:\n{html}"
    );
    assert!(
        html.contains("`y`"),
        "the wide fence's body, backtick and all, must survive; html:\n{html}"
    );
    assert!(
        !html.contains("QuartoInlineRender"),
        "the inline-R wrapper must not reach the output"
    );
}

/// A fence spelling inside a front-matter scalar. This one is mid-line, so
/// knitr's line-anchored lookbehinds would still eat it; the non-backtick
/// prefix guard is what rejects it.
#[test]
fn fence_spelling_in_yaml_scalar_renders() {
    if !knitr_available() {
        eprintln!("SKIP: knitr not available — fence_spelling_in_yaml_scalar_renders");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let content = "---\ntitle: \"In the title: ```r blocks\"\nengine: knitr\n---\n\n\
                   ```{r}\ncat(paste0(\"O\", \"UT\"), \"\\n\")\n```\n\nProse.\n";
    let html = render_html(&tmp, "yamltitle.qmd", content);
    assert!(html.contains("OUT"), "the executable cell must still run");
    // knitr re-scans the string we hand it, and its lookbehinds check for
    // adjacent backticks rather than escaped ones — so it would not reject
    // this scalar on its own. Pin the surviving text, or the two assertions
    // either side of this one would still hold if knitr mangled the title.
    assert!(
        html.contains("```r blocks"),
        "the title scalar must survive intact; html:\n{html}"
    );
    assert!(
        !html.contains("QuartoInlineRender"),
        "the inline-R wrapper must not reach the output"
    );
}

/// The guard must not cost us a real inline expression sharing a document
/// with a display fence — the case that makes a naive "skip everything near a
/// backtick" fix wrong.
#[test]
fn inline_r_still_evaluated_next_to_a_display_fence() {
    if !knitr_available() {
        eprintln!("SKIP: knitr not available — inline_r_still_evaluated_next_to_a_display_fence");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let content = "---\ntitle: Both\nengine: knitr\n---\n\n\
                   ```{r}\nvalue <- 6 * 7\n```\n\n\
                   ``` r\ninstall.packages(\"cli\")\n```\n\n\
                   The answer is `r value`.\n";
    let html = render_html(&tmp, "both.qmd", content);
    assert!(
        html.contains("42"),
        "the inline expression must still be evaluated; html:\n{html}"
    );
    assert!(
        html.contains("sourceCode r"),
        "the display fence must still render as an R block"
    );
}
