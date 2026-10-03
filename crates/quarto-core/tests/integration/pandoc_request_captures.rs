//! R3: cached engine results in a pandoc-request render. The browser has no
//! engines, so a code cell shows a result only when a capture holds one;
//! `PandocRequestOutcome::unexecuted_cells` counts the cells that do not.
//! `NativeRuntime` over a directory outside `/tmp`, like
//! `pandoc_request_exports.rs`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine as _;
use quarto_core::pandoc_request::render::{
    PandocRequestInput, PandocRequestOutcome, render_pandoc_request,
};
use quarto_core::project::ProjectContext;
use quarto_system_runtime::NativeRuntime;
use quarto_trace::{CaptureFile, EngineCapture};

fn scratch() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::Builder::new()
        .prefix("q2-r3-captures-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    (dir, root)
}

fn render(path: &Path, qmd: &str, captures: Vec<EngineCapture>) -> PandocRequestOutcome {
    render_as(path, qmd, captures, "docx")
}

fn render_as(
    path: &Path,
    qmd: &str,
    captures: Vec<EngineCapture>,
    format: &str,
) -> PandocRequestOutcome {
    std::fs::write(path, qmd).unwrap();
    let runtime = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(path, runtime.as_ref()).unwrap();
    pollster::block_on(render_pandoc_request(
        PandocRequestInput {
            scope: quarto_core::pandoc_request::render::BookScope::Auto,
            captures_by_path: Default::default(),
            capture_error: None,
            hooks: None,
            path,
            content: qmd.as_bytes(),
            format,
            project: &project,
            source_date_epoch: Some(1_700_000_000),
            captures,
            typst_available_fonts: None,
            resolver: None,
        },
        runtime,
    ))
}

/// Everything the request carries as text: the input JSON holds the AST.
fn request_text(out: &PandocRequestOutcome) -> String {
    let request = out.request.as_ref().expect("request");
    request
        .files
        .iter()
        .map(|f| String::from_utf8_lossy(&f.bytes).into_owned())
        .collect()
}

const DOC: &str = "---\ntitle: T\n---\n\nBefore.\n\n```{r}\nSRC_R\n```\n\nMiddle.\n\n\
                   ```{python}\nSRC_PY\n```\n\nAfter.\n";

/// A knitr capture for `DOC`'s `{r}` cell only: its output replaces the cell.
fn r_capture(figure: Option<&CaptureFile>) -> EngineCapture {
    let figure_md = if figure.is_some() {
        "::: {.cell-output-display}\n![](doc_files/fig.png)\n:::\n"
    } else {
        ""
    };
    let markdown = format!(
        "---\ntitle: T\n---\n\nBefore.\n\n::: {{.cell}}\n::: {{.cell-output .cell-output-stdout}}\n\
         OUT_R\n:::\n{figure_md}:::\n\nMiddle.\n\n```{{python}}\nSRC_PY\n```\n\nAfter.\n"
    );
    EngineCapture {
        engine_name: "r".to_string(),
        input_qmd: DOC.to_string(),
        result: serde_json::json!({ "markdown": markdown }),
        files: figure.cloned().into_iter().collect(),
    }
}

#[test]
fn without_a_capture_every_cell_is_unexecuted_and_renders_as_source() {
    let (_guard, root) = scratch();
    let out = render(&root.join("doc.qmd"), DOC, Vec::new());
    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(out.unexecuted_cells, 2);
    let text = request_text(&out);
    assert!(text.contains("SRC_R") && text.contains("SRC_PY"), "{text}");
    assert!(!text.contains("OUT_R"));
}

#[test]
fn a_capture_replaces_its_cells_and_the_rest_are_counted() {
    let (_guard, root) = scratch();
    let out = render(&root.join("doc.qmd"), DOC, vec![r_capture(None)]);
    assert!(out.error.is_none(), "{:?}", out.error);
    // The `{r}` cell shows its result; the `{python}` cell has none.
    assert_eq!(out.unexecuted_cells, 1);
    let text = request_text(&out);
    assert!(text.contains("OUT_R"), "{text}");
    assert!(!text.contains("SRC_R"), "{text}");
    assert!(text.contains("SRC_PY"), "{text}");
}

#[test]
fn a_document_without_cells_counts_none() {
    let (_guard, root) = scratch();
    let out = render(&root.join("doc.qmd"), "# Hi\n\nText.\n", Vec::new());
    assert_eq!(out.unexecuted_cells, 0);
}

#[test]
fn cells_nested_in_containers_are_counted() {
    let (_guard, root) = scratch();
    let qmd = "::: {.callout-note}\n```{r}\n1\n```\n:::\n\n- item\n\n  ```{python}\n  2\n  ```\n";
    let out = render(&root.join("doc.qmd"), qmd, Vec::new());
    assert_eq!(out.unexecuted_cells, 2);
}

/// A captured figure is materialized next to the document, so `resource_refs`
/// finds it and the request carries its bytes.
#[test]
fn a_captured_figure_reaches_the_request_as_a_resource() {
    let (_guard, root) = scratch();
    let figure = CaptureFile {
        path: "doc_files/fig.png".to_string(),
        contents_base64: base64::engine::general_purpose::STANDARD.encode(b"FIGBYTES"),
    };
    let out = render(&root.join("doc.qmd"), DOC, vec![r_capture(Some(&figure))]);
    assert!(out.error.is_none(), "{:?}", out.error);
    let request = out.request.as_ref().expect("request");
    let refs: Vec<_> = request
        .resource_refs
        .iter()
        .filter(|r| r.path.ends_with("doc_files/fig.png"))
        .collect();
    assert_eq!(refs.len(), 1, "{:?}", request.resource_refs);
    assert_eq!(refs[0].bytes, b"FIGBYTES");
}

/// R8: HTML a capture splices in (a gt/pandas table) is CSS-inlined for typst
/// too, because the stage runs after the splice.
#[test]
fn a_captured_styled_table_is_css_inlined_for_typst() {
    let (_guard, root) = scratch();
    let markdown = "---\ntitle: T\n---\n\n::: {.cell}\n::: {.cell-output-display}\n\
        ```{=html}\n<style>td { text-align: right }</style>\n\
        <table><tr><td>TBL</td></tr></table>\n```\n:::\n:::\n";
    let capture = EngineCapture {
        engine_name: "r".to_string(),
        input_qmd: "---\ntitle: T\n---\n\n```{r}\nSRC_R\n```\n".to_string(),
        result: serde_json::json!({ "markdown": markdown }),
        files: Vec::new(),
    };
    let qmd = "---\ntitle: T\n---\n\n```{r}\nSRC_R\n```\n";
    let out = render_as(&root.join("doc.qmd"), qmd, vec![capture], "typst");
    assert!(out.error.is_none(), "{:?}", out.error);
    let request = out.request.as_ref().expect("request");
    let input = request
        .files
        .iter()
        .find(|f| f.path.ends_with("/pandoc-input.json"))
        .expect("pandoc-input.json");
    let json = String::from_utf8_lossy(&input.bytes);
    assert!(json.contains("TBL"), "capture not spliced: {json}");
    assert!(!json.contains("<style"), "style block left in the input");
    assert!(json.contains("text-align"), "rules not on the cell");
}
