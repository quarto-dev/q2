//! R4: the typst `.typ` request through `render_pandoc_request` (the code
//! behind the browser export), driven natively over a directory. Typst
//! parity with a recorded native run is in `pandoc_request_prepare.rs`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine as _;
use quarto_core::pandoc_request::render::{
    PandocRequestInput, PandocRequestOutcome, render_pandoc_request,
};
use quarto_core::pandoc_request::{PandocRequest, RequestPost};
use quarto_core::project::ProjectContext;
use quarto_system_runtime::NativeRuntime;
use serde_json::Value;

/// A directory outside `/tmp` (wasm-mode requests reject mounts under it).
pub(crate) fn scratch() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::Builder::new()
        .prefix("q2-r4-typst-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    (dir, root)
}

fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

pub(crate) fn render_with(
    path: &Path,
    format: &str,
    typst_available_fonts: Option<Vec<String>>,
) -> PandocRequestOutcome {
    let runtime = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(path, runtime.as_ref()).unwrap();
    let content = std::fs::read(path).unwrap();
    pollster::block_on(render_pandoc_request(
        PandocRequestInput {
            scope: quarto_core::pandoc_request::render::BookScope::Auto,
            captures_by_path: Default::default(),
            capture_error: None,
            hooks: None,
            path,
            content: &content,
            format,
            project: &project,
            source_date_epoch: Some(1_700_000_000),
            captures: Vec::new(),
            typst_available_fonts,
            resolver: None,
        },
        runtime,
    ))
}

pub(crate) fn render_typst(root: &Path, qmd: &str) -> PandocRequestOutcome {
    let doc = root.join("doc.qmd");
    write(&doc, qmd.as_bytes());
    render_with(&doc, "typst", None)
}

fn params(request: &PandocRequest) -> Value {
    let blob = &request.env["QUARTO_FILTER_PARAMS"];
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(blob)
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn file<'a>(request: &'a PandocRequest, suffix: &str) -> &'a [u8] {
    &request
        .files
        .iter()
        .find(|f| f.path.ends_with(suffix))
        .unwrap_or_else(|| panic!("no request file ending {suffix}"))
        .bytes
}

fn codes(out: &PandocRequestOutcome) -> Vec<&str> {
    out.diagnostics
        .iter()
        .filter_map(|d| d.code.as_deref())
        .collect()
}

#[test]
fn typst_yields_a_typ_request_with_the_template_as_files() {
    let (_guard, root) = scratch();
    let out = render_typst(&root, "---\ntitle: T\n---\n\n# Hi\n\nSome text.\n");
    assert!(out.error.is_none(), "{:?}", out.error);
    let request = out.request.expect("request");
    assert_eq!(request.writer, "typst");
    assert_eq!(request.post, RequestPost::None);
    assert_eq!(request.job_id, request.compute_job_id());
    // pandoc writes the `.typ`; the typst `Format`'s own extension is `pdf`.
    assert!(
        request.output_path.ends_with("/doc.typ"),
        "{}",
        request.output_path
    );
    // Templates are request files, not disk state: the whole vendored set.
    let dir = format!("{}/pandoc-typst-template/", request.share_root);
    let template_files: Vec<_> = request
        .files
        .iter()
        .filter(|f| f.path.starts_with(&dir))
        .map(|f| f.path.trim_start_matches(&dir).to_string())
        .collect();
    assert!(template_files.len() >= 8, "{template_files:?}");
    assert!(template_files.iter().any(|f| f == "template.typ"));
    let i = request.argv.iter().position(|a| a == "--template").unwrap();
    assert_eq!(request.argv[i + 1], format!("{dir}template.typ"));
    // Nothing the PDF compile alone reads is in a `.typ` request (D8.6).
    assert!(request.typst_available_fonts.is_none());
    assert!(params(&request).get("typst-available-fonts").is_none());
    assert!(request.resource_refs.is_empty());
}

#[test]
fn a_user_template_and_partials_replace_the_vendored_files() {
    let (_guard, root) = scratch();
    write(&root.join("my.typ"), b"// USER-TEMPLATE\n$body$\n");
    write(&root.join("p/typst-show.typ"), b"// USER-SHOW-PARTIAL\n");
    let out = render_typst(
        &root,
        "---\ntitle: T\nformat:\n  typst:\n    template: my.typ\n    template-partials:\n      - p/typst-show.typ\n---\n\n# Hi\n",
    );
    assert!(out.error.is_none(), "{:?}", out.error);
    let request = out.request.expect("request");
    assert_eq!(
        file(&request, "/pandoc-typst-template/template.typ"),
        b"// USER-TEMPLATE\n$body$\n"
    );
    assert_eq!(
        file(&request, "/pandoc-typst-template/typst-show.typ"),
        b"// USER-SHOW-PARTIAL\n"
    );
    // The vendored partials the user did not replace stay.
    assert!(
        request
            .files
            .iter()
            .any(|f| f.path.ends_with("/pandoc-typst-template/numbering.typ"))
    );
    // Only one `--template`: the user's file travels as the template itself.
    assert_eq!(
        request.argv.iter().filter(|a| *a == "--template").count(),
        1
    );
}

/// A declared partial that cannot be read is a stage error naming it (the
/// read goes through the runtime, so in the browser it is a VFS miss).
#[test]
fn a_missing_template_partial_is_an_error() {
    let (_guard, root) = scratch();
    let out = render_typst(
        &root,
        "---\ntitle: T\ntemplate-partials:\n  - nope.typ\n---\n\n# Hi\n",
    );
    assert!(out.request.is_none());
    let error = out.error.expect("error");
    assert!(error.contains("typst template partial"), "{error}");
}

#[test]
fn a_theme_file_highlight_style_reaches_argv_as_text() {
    let (_guard, root) = scratch();
    write(
        &root.join("mine.theme"),
        br##"{"background-color":"#fafafa","line-number-color":"#123456",
            "text-styles":{"Keyword":{"text-color":"#008000","bold":true}}}"##,
    );
    let out = render_typst(
        &root,
        "---\ntitle: T\nsyntax-highlighting: mine.theme\n---\n\n```python\nx = 1\n```\n",
    );
    assert!(out.error.is_none(), "{:?}", out.error);
    let request = out.request.expect("request");
    let defs = request
        .argv
        .iter()
        .find(|a| a.starts_with("highlighting-definitions="))
        .expect("definitions argv");
    assert!(defs.contains("rgb(\"#fafafa\")"));
    // Read into argv text: the theme file itself is not mounted.
    assert!(request.resource_refs.is_empty());
    assert!(!request.files.iter().any(|f| f.path.ends_with("mine.theme")));
}

#[test]
fn the_toc_depth_defaults_file_is_a_request_file() {
    let (_guard, root) = scratch();
    let out = render_typst(
        &root,
        "---\ntitle: T\ntoc: true\ntoc-depth: 2\n---\n\n# Hi\n",
    );
    assert!(out.error.is_none(), "{:?}", out.error);
    let request = out.request.expect("request");
    let yaml = file(&request, "/pandoc-typst-toc-defaults.yaml");
    assert_eq!(
        std::str::from_utf8(yaml).unwrap(),
        "toc: true\ntoc-depth: 2\n"
    );
    let i = request.argv.iter().position(|a| a == "--defaults").unwrap();
    assert!(request.argv[i + 1].ends_with("/pandoc-typst-toc-defaults.yaml"));
    assert!(request.files.iter().any(|f| f.path == request.argv[i + 1]));
}

/// The Pandoc JSON a request will hand to pandoc, as text.
fn input_json(request: &PandocRequest) -> String {
    String::from_utf8(file(request, "/pandoc-input.json").to_vec()).unwrap()
}

const STYLED_TABLE: &str = "```{=html}\n<style>td { text-align: right; color: red }</style>\n<table><tr><td>x</td></tr></table>\n```\n";

/// PR #766's Lua `inline_css` needs `quarto.config.cli_path()`, which only the
/// native CLI sets, so the browser request carries none. The Rust
/// `inline-table-css` stage does the inlining ahead of pandoc instead.
#[test]
fn a_styled_raw_html_table_is_css_inlined_without_a_cli_path() {
    let (_guard, root) = scratch();
    let out = render_typst(&root, &format!("---\ntitle: T\n---\n\n{STYLED_TABLE}"));
    assert!(out.error.is_none(), "{:?}", out.error);
    assert!(codes(&out).is_empty(), "{:?}", out.diagnostics);
    let request = out.request.expect("request");
    let json = input_json(&request);
    assert!(!json.contains("<style"), "style block left in the input");
    assert!(json.contains("text-align"), "rules not on the cells");
    assert!(params(&request).get("quarto-cli-path").is_none());
    assert!(params(&request).get("typst-path").is_none());
}

#[test]
fn the_opt_out_comment_leaves_the_table_untouched() {
    let (_guard, root) = scratch();
    let out = render_typst(
        &root,
        "---\ntitle: T\n---\n\n```{=html}\n<!--| quarto-html-table-processing: none -->\n<style>td { color: red }</style>\n<table><tr><td>x</td></tr></table>\n```\n",
    );
    assert!(out.error.is_none(), "{:?}", out.error);
    let json = input_json(&out.request.expect("request"));
    assert!(json.contains("<style"), "opted-out table was inlined");
}

#[test]
fn a_non_typst_format_is_not_css_inlined() {
    let (_guard, root) = scratch();
    let doc = root.join("doc.qmd");
    write(
        &doc,
        format!("---\ntitle: T\n---\n\n{STYLED_TABLE}").as_bytes(),
    );
    let out = render_with(&doc, "docx", None);
    assert!(out.error.is_none(), "{:?}", out.error);
    let json = input_json(&out.request.expect("request"));
    assert!(json.contains("<style"), "docx input was inlined");
}

#[test]
fn an_unstyled_html_table_is_left_alone() {
    let (_guard, root) = scratch();
    let out = render_typst(
        &root,
        "---\ntitle: T\n---\n\n```{=html}\n<table><tr><td>x</td></tr></table>\n```\n",
    );
    assert!(codes(&out).is_empty(), "{:?}", out.diagnostics);
}

#[test]
fn host_fonts_feed_the_filter_param_and_the_job_id() {
    let (_guard, root) = scratch();
    let doc = root.join("doc.qmd");
    write(&doc, b"---\ntitle: T\n---\n\n# Hi\n");
    let none = render_with(&doc, "typst", None).request.unwrap();
    let inter = render_with(&doc, "typst", Some(vec!["Inter".into()]))
        .request
        .unwrap();
    assert_eq!(
        params(&inter)["typst-available-fonts"],
        serde_json::json!(["Inter"])
    );
    assert_eq!(inter.typst_available_fonts, Some(vec!["Inter".to_string()]));
    assert_ne!(none.job_id, inter.job_id);
    assert_eq!(inter.job_id, inter.compute_job_id());
    // Not a typst param for other formats.
    let docx = render_with(&doc, "docx", Some(vec!["Inter".into()]))
        .request
        .unwrap();
    assert!(params(&docx).get("typst-available-fonts").is_none());
}

/// `pdf` is the typst request with a compile step; same argv, different job.
#[test]
fn pdf_is_the_typst_request_with_a_compile_post_step() {
    let (_guard, root) = scratch();
    let doc = root.join("doc.qmd");
    write(&doc, b"---\ntitle: T\n---\n\n# Hi\n");
    let typst = render_with(&doc, "typst", None).request.unwrap();
    let pdf = render_with(&doc, "typst-pdf", None)
        .request
        .expect("pdf request");
    assert_eq!(pdf.post, RequestPost::CompileTypst);
    assert_eq!(pdf.writer, "typst");
    assert!(pdf.output_path.ends_with("/doc.typ"), "{}", pdf.output_path);
    assert_eq!(pdf.argv, typst.argv);
    assert_ne!(pdf.job_id, typst.job_id);
    assert_eq!(pdf.job_id, pdf.compute_job_id());
    assert_eq!(
        pdf.post,
        serde_json::from_value(serde_json::json!("compile_typst")).unwrap()
    );
}

#[test]
fn the_document_date_comes_from_source_date_epoch() {
    let (_guard, root) = scratch();
    let doc = root.join("doc.qmd");
    write(&doc, b"---\ntitle: T\n---\n\n# Hi\n");
    let pdf = render_with(&doc, "typst-pdf", None).request.unwrap();
    assert_eq!(pdf.env["SOURCE_DATE_EPOCH"], "1700000000");
}

/// R4's open item, decided: pandoc never reads a typst document's images or
/// its brand's font and logo files, so only the `pdf` request (whose compile
/// reads them) carries them in `resource_refs`.
#[test]
fn images_and_brand_assets_are_mounted_for_pdf_only() {
    let (_guard, root) = scratch();
    let doc = root.join("doc.qmd");
    write(
        &doc,
        b"---\ntitle: T\nbrand:\n  typography:\n    fonts:\n      - family: Mine\n        source: file\n        files:\n          - fonts/mine.ttf\n  logo:\n    small: logo.svg\n---\n\n![fig](fig.png)\n",
    );
    write(&root.join("fig.png"), b"PNG");
    write(&root.join("fonts/mine.ttf"), b"TTF");
    write(&root.join("logo.svg"), b"<svg/>");

    let typst = render_with(&doc, "typst", None);
    assert!(typst.error.is_none(), "{:?}", typst.error);
    assert!(typst.request.unwrap().resource_refs.is_empty());

    let pdf = render_with(&doc, "typst-pdf", None);
    assert!(pdf.error.is_none(), "{:?}", pdf.error);
    let mounted: Vec<String> = pdf
        .request
        .unwrap()
        .resource_refs
        .iter()
        .map(|f| f.path.rsplit('/').next().unwrap().to_string())
        .collect();
    for want in ["fig.png", "mine.ttf", "logo.svg"] {
        assert!(
            mounted.iter().any(|m| m == want),
            "{want} not in {mounted:?}"
        );
    }
}

/// Other formats keep embedding images, whatever typst does.
#[test]
fn docx_still_mounts_its_images() {
    let (_guard, root) = scratch();
    let doc = root.join("doc.qmd");
    write(&doc, b"---\ntitle: T\n---\n\n![fig](fig.png)\n");
    write(&root.join("fig.png"), b"PNG");
    let docx = render_with(&doc, "docx", None).request.unwrap();
    assert_eq!(docx.resource_refs.len(), 1);
}
