/*
 * tests/integration/equation_numbering_pipeline.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * End-to-end tests for format-specific equation numbering (bd-vlhi2zkj).
 */

//! Numbered display equations (`$$…$$ {#eq-x}`) reach the writer with
//! their number encoded the way the selected math engine needs:
//!
//! - `\tag{N}` inside the TeX for MathJax / KaTeX (the default),
//! - ` \qquad(N)` inside the TeX for engines that only read math,
//! - a sibling `span.quarto-eq-number` outside the math for MathML.
//!
//! `CrossrefRenderTransform` records the number as the reserved
//! `quarto-eq-number` attribute on the equation span and leaves the TeX
//! alone; `EquationNumberStage`, which runs *after* user post filters,
//! picks the encoding and removes the attribute. Lua post filters can
//! therefore read, rewrite or delete the number; pre filters never see
//! it (crossref has not run yet). Plan:
//! `claude-notes/plans/2026-09-21-equation-numbering-and-mathml.md`.

use std::path::Path;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

/// Substring of the MathJax inline config block `MathJsStage` emits.
const MATHJAX_CONFIG_SENTINEL: &str = "window.MathJax";

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// Render `doc.qmd` (plus any `extra` sibling files) to `format` and
/// return the HTML.
fn render_with(qmd: &str, format: &str, extra: &[(&str, &str)]) -> String {
    let temp = TempDir::new().unwrap();
    for (name, contents) in extra {
        write_file(&temp.path().join(name), contents);
    }
    let qmd_path = temp.path().join("doc.qmd");
    write_file(&qmd_path, qmd);
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result = render_to_file(&qmd_path, format, &RenderToFileOptions::default(), runtime)
        .expect("render");
    std::fs::read_to_string(&result.output_path).expect("read output")
}

fn render(qmd: &str, format: &str) -> String {
    render_with(qmd, format, &[])
}

/// One labelled equation under the given front matter.
fn labelled_doc(front_matter: &str) -> String {
    format!(
        "---\ntitle: Labelled\n{front_matter}---\n\n$$E = mc^2$$ {{#eq-einstein}}\n\nSee @eq-einstein.\n"
    )
}

// ── Encodings ───────────────────────────────────────────────────────────

/// Default engine (MathJax): the number is a `\tag{N}` inside the TeX,
/// the reserved attribute never reaches the HTML, and the crossref link
/// still resolves.
#[test]
fn default_method_encodes_tag_inside_tex() {
    let html = render(&labelled_doc(""), "html");
    assert!(
        html.contains("\\[E = mc^2\\tag{1}\\]"),
        "expected \\tag{{1}} appended to the display math; got:\n{html}"
    );
    assert!(
        !html.contains("quarto-eq-number"),
        "the reserved attribute must be consumed before the writer; got:\n{html}"
    );
    assert!(html.contains(MATHJAX_CONFIG_SENTINEL));
    assert!(
        html.contains("Equation\u{a0}1"),
        "the @eq-einstein reference must still render its number"
    );
}

#[test]
fn katex_method_encodes_tag_inside_tex() {
    let html = render(&labelled_doc("html-math-method: katex\n"), "html");
    assert!(html.contains("\\[E = mc^2\\tag{1}\\]"), "got:\n{html}");
    assert!(!html.contains("quarto-eq-number"));
}

#[test]
fn object_form_method_encodes_tag_inside_tex() {
    let html = render(
        &labelled_doc(
            "html-math-method:\n  method: mathjax\n  url: https://example.invalid/mj.js\n",
        ),
        "html",
    );
    assert!(html.contains("\\[E = mc^2\\tag{1}\\]"), "got:\n{html}");
}

#[test]
fn revealjs_encodes_tag_inside_tex() {
    let html = render(&labelled_doc(""), "revealjs");
    assert!(html.contains("\\[E = mc^2\\tag{1}\\]"), "got:\n{html}");
    assert!(!html.contains("quarto-eq-number"));
}

/// `plain` loads no engine, so `\tag` would be a bare TeX command in the
/// page. Quarto 1's encoding for engines that only read math is
/// ` \qquad(N)`.
#[test]
fn plain_method_encodes_qquad_inside_tex() {
    let html = render(&labelled_doc("html-math-method: plain\n"), "html");
    assert!(
        html.contains("\\[E = mc^2 \\qquad(1)\\]"),
        "expected ` \\qquad(1)` appended; got:\n{html}"
    );
    assert!(!html.contains("\\tag{"));
    assert!(!html.contains("quarto-eq-number"));
    assert!(!html.contains(MATHJAX_CONFIG_SENTINEL));
}

/// Unknown method strings are treated like `plain` (no engine we know
/// of will read `\tag`).
#[test]
fn unknown_method_encodes_qquad_inside_tex() {
    let html = render(&labelled_doc("html-math-method: gladtex\n"), "html");
    assert!(html.contains("\\[E = mc^2 \\qquad(1)\\]"), "got:\n{html}");
}

/// `mathml`: the TeX is untouched (the MathML stage, bd-3evfzwal, will
/// convert it later) and the number is a sibling label outside the math,
/// with a modifier class on the equation span for the CSS that lays the
/// two out.
#[test]
fn mathml_method_places_number_as_sibling_label() {
    let html = render(&labelled_doc("html-math-method: mathml\n"), "html");
    assert!(
        html.contains("\\[E = mc^2\\]"),
        "the math text must be untouched; got:\n{html}"
    );
    assert!(
        html.contains("<span class=\"quarto-eq-number\">(1)</span>"),
        "expected the sibling label; got:\n{html}"
    );
    assert!(
        html.contains("class=\"quarto-math-with-attribute quarto-eq-sibling-number\""),
        "expected the modifier class on the equation span; got:\n{html}"
    );
    assert!(!html.contains("\\tag{"));
    assert!(!html.contains("quarto-eq-number=\""));
}

/// The sibling label follows the math inside the equation span, so CSS
/// can lay them out as one row.
#[test]
fn mathml_sibling_label_is_inside_the_equation_span() {
    let html = render(&labelled_doc("html-math-method: mathml\n"), "html");
    let start = html.find("id=\"eq-einstein\"").expect("equation span");
    let math_end = html[start..].find("\\]</span>").expect("math span end") + start;
    let label = html[start..].find("quarto-eq-number\">(1)").expect("label") + start;
    assert!(
        label > math_end,
        "label must come after the math span; got:\n{}",
        &html[start..]
    );
    let close = html[label..]
        .find("</span></span>")
        .expect("both spans close")
        + label;
    assert!(
        !html[label..close].contains("<span"),
        "nothing else between the label and the equation span's close"
    );
}

/// Unlabelled display math carries no number in any encoding.
#[test]
fn unlabelled_display_math_is_untouched() {
    for method in [
        "",
        "html-math-method: plain\n",
        "html-math-method: mathml\n",
    ] {
        let html = render(
            &format!("---\ntitle: Unlabelled\n{method}---\n\n$$x^2$$\n"),
            "html",
        );
        assert!(
            html.contains("\\[x^2\\]"),
            "method {method:?}: got:\n{html}"
        );
        assert!(!html.contains("\\tag{"));
        assert!(!html.contains("qquad"));
        assert!(!html.contains("quarto-eq-number"));
    }
}

/// Numbers stay sequential across several equations.
#[test]
fn several_equations_number_sequentially() {
    let html = render(
        "---\ntitle: Three\n---\n\n$$a$$ {#eq-a}\n\n$$b$$ {#eq-b}\n\n$$c$$ {#eq-c}\n",
        "html",
    );
    for (text, n) in [("a", 1), ("b", 2), ("c", 3)] {
        assert!(
            html.contains(&format!("\\[{text}\\tag{{{n}}}\\]")),
            "expected {text} tagged {n}; got:\n{html}"
        );
    }
}

// ── The Lua escape hatch ───────────────────────────────────────────────

const RENUMBER_LUA: &str = r#"
function Span(el)
  if el.attributes["quarto-eq-number"] then
    el.attributes["quarto-eq-number"] = "A"
    return el
  end
end
"#;

const UNNUMBER_LUA: &str = r#"
function Span(el)
  if el.attributes["quarto-eq-number"] then
    el.attributes["quarto-eq-number"] = nil
    return el
  end
end
"#;

/// A filter that marks a span whenever it sees the attribute; used to
/// pin *when* the attribute is visible.
const WITNESS_LUA: &str = r#"
function Span(el)
  if el.attributes["quarto-eq-number"] then
    el.classes:insert("saw-eq-number")
    return el
  end
end
"#;

/// A post filter (listed after the `quarto` sentinel) can rewrite the
/// number: the encoding then carries the rewritten label.
#[test]
fn post_filter_can_rewrite_the_number() {
    let html = render_with(
        &labelled_doc("filters:\n  - quarto\n  - renumber.lua\n"),
        "html",
        &[("renumber.lua", RENUMBER_LUA)],
    );
    assert!(
        html.contains("\\[E = mc^2\\tag{A}\\]"),
        "expected the filter's label in the tag; got:\n{html}"
    );
    assert!(!html.contains("\\tag{1}"));
}

/// Deleting the attribute in a post filter suppresses the number
/// entirely; the equation itself still renders.
#[test]
fn post_filter_can_delete_the_number() {
    let html = render_with(
        &labelled_doc("filters:\n  - quarto\n  - unnumber.lua\n"),
        "html",
        &[("unnumber.lua", UNNUMBER_LUA)],
    );
    assert!(html.contains("\\[E = mc^2\\]"), "got:\n{html}");
    assert!(!html.contains("\\tag{"));
    assert!(!html.contains("quarto-eq-number"));
}

/// The attribute exists from crossref-render onwards, so a *post* filter
/// sees it and a *pre* filter (the default position, before the
/// `quarto` sentinel) does not.
#[test]
fn attribute_is_visible_to_post_filters_only() {
    let post = render_with(
        &labelled_doc("filters:\n  - quarto\n  - witness.lua\n"),
        "html",
        &[("witness.lua", WITNESS_LUA)],
    );
    assert!(
        post.contains("saw-eq-number"),
        "a post filter must see quarto-eq-number; got:\n{post}"
    );

    let pre = render_with(
        &labelled_doc("filters:\n  - witness.lua\n"),
        "html",
        &[("witness.lua", WITNESS_LUA)],
    );
    assert!(
        !pre.contains("saw-eq-number"),
        "a pre filter runs before crossref-render and must not see it; got:\n{pre}"
    );
    // The number still lands either way.
    assert!(post.contains("\\tag{1}"));
    assert!(pre.contains("\\tag{1}"));
}
