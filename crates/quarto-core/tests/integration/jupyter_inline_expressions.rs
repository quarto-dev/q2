//! EXR-3 Phase 3: Jupyter inline expressions (`` `{python} expr` ``) through the real
//! render path, against a real kernel.
//!
//! Modeled on `knitr_inline_expressions.rs`. Every test skips (with a message) when no
//! `jupyter` runtime or Python kernelspec is available, so CI without ipykernel stays green; put a venv
//! with ipykernel on `PATH` (and its `share/jupyter` in `JUPYTER_PATH`) to run them locally.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::ProjectContext;
use quarto_core::engine::EngineRegistry;
use quarto_core::engine::jupyter::list_kernelspecs;
use quarto_core::render_to_file::{RenderToFileOptions, render_document_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn python_kernel_available() -> bool {
    EngineRegistry::default()
        .get("jupyter")
        .is_some_and(|e| e.is_available())
        && tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(list_kernelspecs())
            .iter()
            .any(|s| s.language.eq_ignore_ascii_case("python"))
}

/// Skip the test (return from it) when no Python kernel is installed.
macro_rules! require_kernel {
    ($name:literal) => {
        if !python_kernel_available() {
            eprintln!("SKIP: no jupyter runtime or Python kernelspec — {}", $name);
            return;
        }
    };
}

/// Render `content` as `name` in `dir`, returning the HTML or the error text.
fn try_render_in(dir: &std::path::Path, name: &str, content: &str) -> Result<String, String> {
    let input = dir.join(name);
    std::fs::write(&input, content).unwrap();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&input, runtime.as_ref()).expect("project discovery");
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
    .map_err(|e| e.to_string())?;
    Ok(std::fs::read_to_string(result.output_path).expect("read rendered HTML"))
}

fn try_render(content: &str) -> Result<String, String> {
    let tmp = TempDir::new().unwrap();
    try_render_in(tmp.path(), "doc.qmd", content)
}

fn render(content: &str) -> String {
    try_render(content).unwrap_or_else(|e| panic!("render must succeed: {e}"))
}

fn render_err(content: &str) -> String {
    match try_render(content) {
        Ok(html) => panic!("render must fail; html:\n{html}"),
        Err(e) => e,
    }
}

/// A document with a python cell defining a few names, followed by `body`.
fn doc(body: &str) -> String {
    format!(
        "---\ntitle: Inline\njupyter: python3\n---\n\n\
         ```{{python}}\n#| echo: false\n\
         from IPython.display import HTML, Markdown\n\
         x = 21\n\
         s = \"a*b_c\"\n\
         ```\n\n{body}\n"
    )
}

#[test]
fn jupyter_inline_in_prose() {
    require_kernel!("jupyter_inline_in_prose");
    let html = render(&doc("Answer: `{python} x * 2`."));
    assert!(html.contains("Answer: 42."), "{html}");
}

#[test]
fn jupyter_inline_sees_state_from_earlier_cell() {
    require_kernel!("jupyter_inline_sees_state_from_earlier_cell");
    let html = render(&doc("Twice: `{python} x + x`."));
    assert!(html.contains("Twice: 42."), "{html}");
}

#[test]
fn jupyter_inline_in_attribute_value() {
    require_kernel!("jupyter_inline_in_attribute_value");
    let html = render(&doc(
        "::: {#hero data-v=\"`{python} x`\"}\nBody.\n:::\n\n[l](https://example.com \"`{python} x`\")\n",
    ));
    assert!(html.contains(r#"data-v="21""#), "{html}");
    assert!(html.contains(r#"title="21""#), "{html}");
}

#[test]
fn jupyter_inline_markdown_value() {
    require_kernel!("jupyter_inline_markdown_value");
    let html = render(&doc("Value: `{python} Markdown('**b**')`."));
    assert!(html.contains("<strong>b</strong>"), "{html}");
}

#[test]
fn jupyter_inline_html_value() {
    require_kernel!("jupyter_inline_html_value");
    let html = render(&doc("Value: `{python} HTML('<b class=\"v\">h</b>')`."));
    assert!(html.contains(r#"<b class="v">h</b>"#), "{html}");
}

#[test]
fn jupyter_inline_string_is_escaped() {
    require_kernel!("jupyter_inline_string_is_escaped");
    let html = render(&doc("Value: `{python} s`."));
    assert!(html.contains("Value: a*b_c."), "{html}");
    assert!(!html.contains("<em>"), "{html}");
}

#[test]
fn jupyter_inline_none_renders_as_text() {
    require_kernel!("jupyter_inline_none_renders_as_text");
    let html = render(&doc("Value: `{python} None`."));
    assert!(html.contains("Value: None."), "{html}");
}

#[test]
fn jupyter_inline_statement_is_an_error() {
    require_kernel!("jupyter_inline_statement_is_an_error");
    let err = render_err(&doc("Bad: `{python} y = 1`."));
    assert!(err.contains("inline expression"), "{err}");
    assert!(err.contains("SyntaxError"), "{err}");
}

#[test]
fn jupyter_inline_empty_string_leaves_source() {
    require_kernel!("jupyter_inline_empty_string_leaves_source");
    let html = render(&doc("Empty: `{python} \"\"`."));
    assert!(html.contains("{python}"), "{html}");
}

#[test]
fn jupyter_inline_error_true_is_ignored() {
    require_kernel!("jupyter_inline_error_true_is_ignored");
    let content = "---\ntitle: t\njupyter: python3\nexecute:\n  error: true\n---\n\n\
         ```{python}\nx = 1\n```\n\nBad: `{python} undefined_name`.\n";
    let err = render_err(content);
    assert!(err.contains("NameError"), "{err}");
}

#[test]
fn jupyter_inline_ignores_eval_false() {
    require_kernel!("jupyter_inline_ignores_eval_false");
    let content = "---\ntitle: t\njupyter: python3\nexecute:\n  eval: false\n---\n\n\
                   Sum: `{python} 1 + 2`.\n";
    let html = render(content);
    assert!(html.contains("Sum: 3."), "{html}");
}

#[test]
fn jupyter_inline_capitalized_language_is_evaluated() {
    require_kernel!("jupyter_inline_capitalized_language_is_evaluated");
    let html = render(&doc("Cap: `{Python} x`."));
    assert!(html.contains("Cap: 21."), "{html}");
}

// ── where spans are and are not evaluated ───────────────────────────────

#[test]
fn jupyter_inline_in_any_fence_stays_literal() {
    require_kernel!("jupyter_inline_in_any_fence_stays_literal");
    let html = render(&doc(
        "```text\n`{python} 111`\n```\n\n```r\n`{python} 222`\n```\n\n\
         ````markdown\n```{python}\n`{python} 333`\n```\n````\n",
    ));
    for n in ["111", "222", "333"] {
        assert!(html.contains(&format!("{{python}} {n}")), "{n}: {html}");
    }
}

#[test]
fn jupyter_inline_in_display_fence_is_not_run() {
    require_kernel!("jupyter_inline_in_display_fence_is_not_run");
    let html = render(&doc("````markdown\nShown: `{python} x`\n````\n"));
    assert!(html.contains("{python} x"), "{html}");
}

#[test]
fn jupyter_inline_in_ceded_block_is_not_run() {
    require_kernel!("jupyter_inline_in_ceded_block_is_not_run");
    // The `{r}` fence makes knitr a declared engine, and a declared engine that cannot
    // run is an error. Skip where R is absent rather than stubbing the engine.
    if !EngineRegistry::default()
        .get("knitr")
        .is_some_and(|e| e.is_available())
    {
        eprintln!("SKIP: no Rscript — jupyter_inline_in_ceded_block_is_not_run");
        return;
    }
    // A `{python}` span inside a braced `{r}` fence: literal (divergence 8).
    let html = render(&doc("```{r}\n#| eval: false\n`{python} 777`\n```\n"));
    assert!(!html.contains(">777<"), "{html}");
    assert!(html.contains("{python} 777"), "{html}");
}

#[test]
fn jupyter_inline_in_html_comment_is_not_run() {
    require_kernel!("jupyter_inline_in_html_comment_is_not_run");
    let html = render(&doc("<!-- inline `{python} undefined_one` -->\n\n\
         <!--\nmulti `{python} undefined_two`\n-->\n\n\
         Text <!-- `{python} undefined_three` --> after.\n"));
    assert!(html.contains("after."), "{html}");
}

#[test]
fn jupyter_inline_unowned_language_untouched() {
    require_kernel!("jupyter_inline_unowned_language_untouched");
    let html = render(&doc("R span: `{r} 1 + 1`, python span: `{python} x`."));
    assert!(html.contains("{r} 1 + 1"), "{html}");
    assert!(html.contains("python span: 21."), "{html}");
}

// ── span-only documents ─────────────────────────────────────────────────

#[test]
fn jupyter_span_only_document_without_engine_key_stays_literal() {
    require_kernel!("jupyter_span_only_document_without_engine_key_stays_literal");
    let html = render("---\ntitle: t\n---\n\nSum: `{python} 1 + 2`.\n");
    assert!(html.contains("{python} 1 + 2"), "{html}");
}

#[test]
fn jupyter_span_only_document_with_jupyter_key_is_evaluated() {
    require_kernel!("jupyter_span_only_document_with_jupyter_key_is_evaluated");
    let html = render("---\ntitle: t\njupyter: python3\n---\n\nSum: `{python} 1 + 2`.\n");
    assert!(html.contains("Sum: 3."), "{html}");
}

#[test]
fn jupyter_engine_declared_spans_only_without_kernel_errors() {
    require_kernel!("jupyter_engine_declared_spans_only_without_kernel_errors");
    let err = render_err("---\ntitle: t\nengine: jupyter\n---\n\nSum: `{python} 1 + 2`.\n");
    assert!(err.contains("`jupyter:` kernel"), "{err}");
    // No spans: nothing to evaluate, not an error.
    let html = render("---\ntitle: t\nengine: jupyter\n---\n\nNo spans.\n");
    assert!(html.contains("No spans."), "{html}");
}

// ── front matter ────────────────────────────────────────────────────────

#[test]
fn jupyter_inline_in_front_matter_is_evaluated() {
    require_kernel!("jupyter_inline_in_front_matter_is_evaluated");
    let html = render("---\ntitle: \"T `{python} 1 + 1` end\"\njupyter: python3\n---\n\nBody.\n");
    assert!(html.contains("<title>T 2 end</title>"), "{html}");
}

/// A markdown value built without a setup cell (a front matter span runs before any cell).
const MD: &str = r#"__import__(\"IPython.display\", fromlist=[\"Markdown\"]).Markdown"#;

#[test]
fn jupyter_inline_in_front_matter_with_quotes_and_backslashes_stays_valid_yaml() {
    require_kernel!("jupyter_inline_in_front_matter_with_quotes_and_backslashes_stays_valid_yaml");
    // The expression has double quotes (the writer escapes them) and the value has a
    // backslash (the re-escape must keep the YAML valid and the markdown parse yields one).
    let html = render(&format!(
        "---\ntitle: \"T `{{python}} {MD}(\\\"q\\\" + chr(92) * 2 + \\\"z*\\\")` end\"\njupyter: python3\n---\n\nBody.\n"
    ));
    assert!(html.contains("<title>T q\\z* end</title>"), "{html}");
}

#[test]
fn jupyter_inline_in_front_matter_with_newline_is_fatal_error() {
    require_kernel!("jupyter_inline_in_front_matter_with_newline_is_fatal_error");
    // A Python `str` never carries a raw newline into the page (Q1 renders its repr), so
    // the newline has to come from a markdown value.
    let err = render_err(&format!(
        "---\ntitle: \"T `{{python}} {MD}(chr(97) + chr(10) + chr(98))` end\"\njupyter: python3\n---\n\nBody.\n"
    ));
    assert!(err.contains("front matter key `title`"), "{err}");
    assert!(err.contains(".qmd:2:"), "{err}");
}

#[test]
fn jupyter_inline_in_project_metadata_is_not_evaluated() {
    require_kernel!("jupyter_inline_in_project_metadata_is_not_evaluated");
    let tmp = TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("_quarto.yml"),
        "project:\n  type: default\nsubtitle: \"S `{python} 1 + 1`\"\n",
    )
    .unwrap();
    let html = try_render_in(tmp.path(), "doc.qmd", &doc("Body.")).expect("render succeeds");
    assert!(!html.contains("S 2"), "{html}");
}

/// Lines of rendered HTML that carry the span under test (see the fixture README).
fn span_lines(html: &str) -> String {
    let mut out: Vec<&str> = html
        .lines()
        .filter(|l| {
            l.starts_with("<p>Before")
                || l.starts_with("<p>Outside")
                || l.contains("span inside an r fence")
        })
        .collect();
    out.push("");
    out.join("\n")
}

#[test]
fn q1_equivalence_fixtures() {
    require_kernel!("q1_equivalence_fixtures");
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jupyter-inline");
    let cases = [
        "none",
        "markdown",
        "html",
        "plain-value",
        "string-value",
        "eval-false",
        "empty-string",
        "r-fence",
    ];
    for case in cases {
        let qmd = std::fs::read_to_string(dir.join(format!("{case}.qmd"))).unwrap();
        let q2_path = dir.join(format!("{case}.q2.html"));
        let expected_path = if q2_path.exists() {
            q2_path
        } else {
            dir.join(format!("{case}.q1.html"))
        };
        let expected = std::fs::read_to_string(expected_path).unwrap();
        let html = try_render(&qmd).unwrap_or_else(|e| panic!("{case}: render failed: {e}"));
        assert_eq!(
            span_lines(&html).replace("\r\n", "\n"),
            expected.replace("\r\n", "\n"),
            "fixture {case}"
        );
    }
}

#[test]
fn q1_equivalence_statement_fails_in_both() {
    require_kernel!("q1_equivalence_statement_fails_in_both");
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jupyter-inline");
    let qmd = std::fs::read_to_string(dir.join("statement.qmd")).unwrap();
    let q1 = std::fs::read_to_string(dir.join("statement.q1.txt")).unwrap();
    let err = render_err(&qmd);
    assert!(err.contains("Q-18-10"), "{err}");
    // Q1's error class and message appear in q2's too.
    assert!(err.contains(q1.split(" (").next().unwrap()), "{err}");
}
