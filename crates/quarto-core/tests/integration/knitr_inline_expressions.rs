//! bd-inline-r-brace-spelling-not-evaluated-lk9s3iwe: Quarto's two inline
//! expression spellings, through the real render path.
//!
//! `` `{r} expr` `` is the cross-engine brace spelling quarto.org documents;
//! `` `r expr` `` is knitr's native rmarkdown spelling. Both evaluate, and
//! they insert the resulting value differently on purpose — the brace form
//! escapes markdown specials in the value, the classic form does not.
//! `docs/computations/inline-code.qmd` on quarto.org states the relationship
//! as an equivalence: `` `r x` `` == `` `{r} I(x)` ``.
//!
//! **Why this lives at the render level rather than in `preprocess.rs`.** The
//! unit tests there pin the rewrite the preprocessor performs. They cannot
//! see what R does with it — whether `.QuartoInlineRender` actually escapes,
//! whether an unwrapped expression actually reaches knitr's inline hook,
//! whether an expression inside an attribute value survives serialization.
//! Only a real render, through the entry `q2 render` itself uses, answers
//! those.
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

fn render_html(tmp: &TempDir, name: &str, content: &str) -> String {
    let input = tmp.path().join(name);
    std::fs::write(&input, content).unwrap();

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&input, runtime.as_ref())
        .expect("project discovery for the inline-expression fixture");

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
    .unwrap_or_else(|e| panic!("render must succeed: {e}"));

    read_html(&result.output_path)
}

fn read_html(path: &Path) -> String {
    std::fs::read_to_string(path).expect("read rendered HTML")
}

/// A document whose setup cell defines `version`, `star` and `b`, followed by
/// `body`.
fn doc(body: &str) -> String {
    format!(
        "---\ntitle: Inline expressions\nengine: knitr\n---\n\n\
         ```{{r}}\n#| echo: false\n\
         version <- paste0(\"2026.0\", \"8.1\")\n\
         star <- \"*emph*\"\n\
         b <- \"**bold**\"\n```\n\n\
         {body}\n"
    )
}

/// The three positions the strand measured, in the brace spelling. The
/// attribute cases are the ones that motivated it: nothing about them is
/// visible to a text diff of the rendered page.
#[test]
fn brace_spelling_is_evaluated_in_prose_and_attributes() {
    if !knitr_available() {
        eprintln!(
            "SKIP: knitr not available — brace_spelling_is_evaluated_in_prose_and_attributes"
        );
        return;
    }
    let tmp = TempDir::new().unwrap();
    let html = render_html(
        &tmp,
        "brace.qmd",
        &doc("Prose: `{r} version`.\n\n\
             ::: {#hero data-version=\"`{r} version`\"}\n\
             Hero body.\n\
             :::\n\n\
             [link](https://example.com \"`{r} version`\")\n"),
    );

    // `version` is assembled at runtime from two string halves, so a literal
    // echo of the source could not produce it.
    assert!(
        html.contains("Prose: 2026.08.1"),
        "brace spelling must be evaluated in prose; html:\n{html}"
    );
    assert!(
        html.contains(r#"data-version="2026.08.1""#),
        "brace spelling must be evaluated in a fenced-div attribute value; html:\n{html}"
    );
    assert!(
        html.contains(r#"title="2026.08.1""#),
        "brace spelling must be evaluated in a link title; html:\n{html}"
    );
    assert!(
        !html.contains("{r}"),
        "no brace expression may survive into the output; html:\n{html}"
    );
    assert!(
        !html.contains("QuartoInlineRender"),
        "the wrapper must not reach the output"
    );
}

/// The classic spelling in the same three positions — the control that proves
/// the spelling, not the position, was the variable.
#[test]
fn classic_spelling_is_evaluated_in_prose_and_attributes() {
    if !knitr_available() {
        eprintln!(
            "SKIP: knitr not available — classic_spelling_is_evaluated_in_prose_and_attributes"
        );
        return;
    }
    let tmp = TempDir::new().unwrap();
    let html = render_html(
        &tmp,
        "classic.qmd",
        &doc("Prose: `r version`.\n\n\
             ::: {#hero data-version=\"`r version`\"}\n\
             Hero body.\n\
             :::\n\n\
             [link](https://example.org \"`r version`\")\n"),
    );

    assert!(
        html.contains("Prose: 2026.08.1"),
        "classic spelling must be evaluated in prose; html:\n{html}"
    );
    assert!(
        html.contains(r#"data-version="2026.08.1""#),
        "classic spelling must be evaluated in a fenced-div attribute value; html:\n{html}"
    );
    assert!(
        html.contains(r#"title="2026.08.1""#),
        "classic spelling must be evaluated in a link title; html:\n{html}"
    );
}

/// The brace spelling escapes markdown specials in the value. This is the
/// half of the contract that `.QuartoInlineRender` implements.
#[test]
fn brace_spelling_escapes_markdown_in_the_value() {
    if !knitr_available() {
        eprintln!("SKIP: knitr not available — brace_spelling_escapes_markdown_in_the_value");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let html = render_html(&tmp, "escape.qmd", &doc("Value: `{r} star`."));

    assert!(
        html.contains("Value: *emph*"),
        "the brace spelling must insert the value as literal text; html:\n{html}"
    );
    assert!(
        !html.contains("<em>emph</em>"),
        "the brace spelling must not let the value's markdown be interpreted; html:\n{html}"
    );
}

/// The classic spelling inserts the value as live markdown. This is knitr's
/// own documented behaviour, and q2 gets it by not wrapping the expression.
#[test]
fn classic_spelling_inserts_the_value_as_markdown() {
    if !knitr_available() {
        eprintln!("SKIP: knitr not available — classic_spelling_inserts_the_value_as_markdown");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let html = render_html(&tmp, "markdown.qmd", &doc("Value: `r star`."));

    assert!(
        html.contains("<em>emph</em>"),
        "the classic spelling must insert the value as markdown; html:\n{html}"
    );
}

/// The documented equivalence, both sides in one render:
/// `` `r x` `` == `` `{r} I(x)` ``.
#[test]
fn as_is_makes_the_two_spellings_equivalent() {
    if !knitr_available() {
        eprintln!("SKIP: knitr not available — as_is_makes_the_two_spellings_equivalent");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let html = render_html(
        &tmp,
        "asis.qmd",
        &doc("Native: `r b`.\n\nWrapped: `{r} I(b)`.\n"),
    );

    assert!(
        html.contains("Native: <strong>bold</strong>"),
        "the classic spelling must render the value's markdown; html:\n{html}"
    );
    assert!(
        html.contains("Wrapped: <strong>bold</strong>"),
        "I() must opt the brace spelling into markdown; html:\n{html}"
    );
}

/// The two spellings render `NULL` differently, and both halves are pinned
/// here because the classic half changed with this fix.
///
/// The classic spelling reaches knitr's default inline hook, which yields
/// `paste(as.character(NULL), collapse = ", ")` — the empty string. The brace
/// spelling reaches `.QuartoInlineRender`, whose first branch turns `NULL`
/// into the literal text `NULL`. Quarto 1 splits exactly the same way; both
/// spans below were checked against `quarto` 99.9.9 and match byte for byte.
///
/// This also covers the reason the classic spelling is not wrapped as
/// `.QuartoInlineRender(I(expr))`: `I(NULL)` is an error in R, so that
/// spelling of the documented equivalence would fail the whole render here.
#[test]
fn null_values_render_per_spelling() {
    if !knitr_available() {
        eprintln!("SKIP: knitr not available — null_values_render_per_spelling");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let html = render_html(
        &tmp,
        "null.qmd",
        &doc("Classic: [`r NULL`]{.classic}. Brace: [`{r} NULL`]{.brace}.\n"),
    );

    assert!(
        html.contains(r#"<span class="classic"></span>"#),
        "the classic spelling must render NULL as the empty string; html:\n{html}"
    );
    assert!(
        html.contains(r#"<span class="brace">NULL</span>"#),
        "the brace spelling must render NULL as the literal text NULL; html:\n{html}"
    );
}

/// A `` ```{r} `` opener carrying a trailing space is the brace branch's worst
/// fence shape: the space satisfies the pattern's `[ \t]` separator, so only
/// the prefix guard stands between it and a match that swallows the block body
/// (bd-knitr-inline-r-eats-fence-2ofk91x1).
///
/// **This test pins the chain, not the guard alone — read this before
/// "strengthening" it.** Two layers upstream mean the shape cannot actually
/// reach `resolve_inline_r_expressions` in production:
///
/// - A *top-level* cell can't carry the space at all. `write_codeblock`
///   (`crates/pampa/src/writers/qmd.rs`) regenerates the fence from the block's
///   attributes and ends it with a bare `writeln!`, so trailing whitespace is
///   gone before this pass runs. A fixture that puts the space on a real cell
///   tests nothing.
/// - Inside a display block the bytes *are* written through verbatim, but
///   `engine::nested_cell_mask::mask` rewrites the opener to
///   `` ```{.r q2-nested-executable} `` before serialization, so the literal
///   `{r}` the pattern needs is no longer there.
///
/// Established by mutation, not by inspection: weakening the prefix guard to
/// `(^|[^\\])` leaves this test green, because the mask intercepts first.
/// Weakening the guard *and* disabling the mask turns the block into
/// `` ```r .QuartoInlineRender(SENTINEL)``` `` and reddens it. (Disabling the
/// mask alone reddens it differently — knitr then executes the nested cell and
/// the render dies, which is the defect the mask exists for.)
///
/// So what this pins end-to-end is that a documented `` ```{r} `` inside a
/// display block survives unexecuted and unwrapped. The prefix guard's own
/// teeth for this shape are at unit level, in
/// `test_executable_cell_fence_with_trailing_space_not_matched`.
#[test]
fn nested_executable_fence_is_not_eaten_by_the_inline_pass() {
    if !knitr_available() {
        eprintln!(
            "SKIP: knitr not available — nested_executable_fence_is_not_eaten_by_the_inline_pass"
        );
        return;
    }
    let tmp = TempDir::new().unwrap();
    // Note the trailing space after `{r}` on the inner fence: that is the
    // byte under test. `SENTINEL` stands in for the block body — if a match
    // anchored on the inner fence, everything from there to the next backtick
    // would be replaced by the wrapper and the sentinel would vanish.
    let content = "---\ntitle: Trailing space\nengine: knitr\n---\n\n\
                   ```{r}\ncat(paste0(\"O\", \"UT\"), \"\\n\")\n```\n\n\
                   ````markdown\n```{r} \nSENTINEL\n```\n````\n\n\
                   After.\n";
    let html = render_html(&tmp, "trailing.qmd", content);

    assert!(
        html.contains("OUT"),
        "the executable cell must still run; html:\n{html}"
    );
    assert!(
        html.contains("SENTINEL"),
        "the display block body must survive intact; html:\n{html}"
    );
    assert!(
        !html.contains("QuartoInlineRender"),
        "the wrapper must not reach the output; html:\n{html}"
    );
    assert!(html.contains("After."), "the page must survive intact");
}

/// The guard must not cost a real brace expression that shares a document
/// with a display fence — the render-level counterpart of
/// `test_inline_r_still_matched_next_to_a_fence`. Without a case in this
/// spelling, no render-level test covers the only spelling the pass rewrites:
/// `knitr_display_fence.rs`'s `inline_r_still_evaluated_next_to_a_display_fence`
/// uses the classic spelling, which this pass now leaves alone, so it would
/// pass even if the rewrite were a no-op.
#[test]
fn brace_expression_still_evaluated_next_to_a_display_fence() {
    if !knitr_available() {
        eprintln!(
            "SKIP: knitr not available — brace_expression_still_evaluated_next_to_a_display_fence"
        );
        return;
    }
    let tmp = TempDir::new().unwrap();
    let html = render_html(
        &tmp,
        "beside.qmd",
        &doc("``` r\ninstall.packages(\"cli\")\n```\n\nThe version is `{r} version`.\n"),
    );

    assert!(
        html.contains("The version is 2026.08.1"),
        "the brace expression must still be evaluated; html:\n{html}"
    );
    assert!(
        html.contains("sourceCode r"),
        "the display fence must still render as a highlighted R block; html:\n{html}"
    );
    assert!(
        !html.contains("QuartoInlineRender"),
        "the wrapper must not reach the output; html:\n{html}"
    );
}
