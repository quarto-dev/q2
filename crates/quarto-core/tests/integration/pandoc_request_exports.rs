//! R2: the logic behind the `wasm-quarto-hub-client` exports, driven natively
//! (`WasmRuntime` is wasm32-only): `render_pandoc_request`, the format
//! resolver and table. The wasm wrappers are covered by the hub-client
//! vitest (`pandocRequest.wasm.test.ts`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use quarto_core::ResourceResolverContext;
use quarto_core::pandoc_request::formats::{
    FormatClass, FormatSource, PANDOC_FORMATS, resolve_document_formats,
};
use quarto_core::pandoc_request::render::{
    PandocRequestInput, PandocRequestOutcome, render_pandoc_request,
};
use quarto_core::project::ProjectContext;
use quarto_error_reporting::DiagnosticKind;
use quarto_system_runtime::NativeRuntime;

/// A directory outside `/tmp` (wasm-mode requests reject mounts under it).
fn scratch() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::Builder::new()
        .prefix("q2-r2-exports-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    (dir, root)
}

fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn render(
    path: &Path,
    format: &str,
    resolver: Option<ResourceResolverContext>,
) -> PandocRequestOutcome {
    let runtime = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(path, runtime.as_ref()).unwrap();
    let content = std::fs::read(path).unwrap();
    pollster::block_on(render_pandoc_request(
        PandocRequestInput {
            attribution: None,
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
            typst_available_fonts: None,
            resolver,
        },
        runtime,
    ))
}

const VFS_ROOT: &str = "/.quarto/project-artifacts";

#[test]
fn a_single_document_yields_a_docx_request_with_its_image() {
    let (_guard, root) = scratch();
    let doc = root.join("doc.qmd");
    write(&doc, b"---\ntitle: T\n---\n\n# Hi\n\n![fig](fig.png)\n");
    write(&root.join("fig.png"), b"PNGBYTES");
    let out = render(&doc, "docx", None);
    assert!(out.error.is_none(), "{:?}", out.error);
    let request = out.request.expect("request");
    assert_eq!(request.writer, "docx");
    assert_eq!(request.job_id, request.compute_job_id());
    assert_eq!(request.env["SOURCE_DATE_EPOCH"], "1700000000");
    assert!(request.dirs.iter().any(|d| d == "/tmp"));
    assert_eq!(request.resource_refs.len(), 1);
    assert_eq!(request.resource_refs[0].bytes, b"PNGBYTES");
}

/// The hub prelude installs the vfs-root resolver and native pandoc rendering
/// does not (`render_to_file.rs`); for a pandoc format the request must be
/// identical either way: the resolver only decides where HTML artifacts and
/// link targets land, and the Pandoc-hybrid prefix drops every HTML stage.
#[test]
fn the_vfs_root_resolver_does_not_change_a_pandoc_request() {
    let (_guard, root) = scratch();
    let doc = root.join("doc.qmd");
    // `link-format` is the one place `link_rewrite` consults the resolver
    // (`page_url_for`); include it, with ordinary, external and anchor links.
    write(
        &doc,
        b"---\ntitle: T\n---\n\nSee [other](other.qmd), [llms](other.qmd){link-format=\"llms\"}, \
          [web](https://x.org/a.html), [anchor](#hi).\n\n# Hi {#hi}\n\n![fig](fig.png)\n",
    );
    write(&root.join("other.qmd"), b"# Other\n");
    write(&root.join("fig.png"), b"PNG");
    let plain = render(&doc, "docx", None);
    let vfs = render(
        &doc,
        "docx",
        Some(ResourceResolverContext::vfs_root(VFS_ROOT)),
    );
    let titles = |o: &PandocRequestOutcome| -> Vec<String> {
        o.diagnostics.iter().map(|d| d.title.clone()).collect()
    };
    assert_eq!(titles(&plain), titles(&vfs));
    let (plain, vfs) = (plain.request.expect("plain"), vfs.request.expect("vfs"));
    assert_eq!(plain.job_id, vfs.job_id);
    assert_eq!(plain, vfs);
}

#[test]
fn a_document_with_errors_returns_no_request() {
    let (_guard, root) = scratch();
    let doc = root.join("doc.qmd");
    write(
        &doc,
        b"---\nformat:\n  docx:\n    reference-doc: nope.docx\n---\n\nHi\n",
    );
    let out = render(&doc, "docx", None);
    assert!(out.request.is_none());
    assert!(out.error.is_some());
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.kind == DiagnosticKind::Error && d.code.as_deref() == Some("Q-5-30")),
        "{:?}",
        out.diagnostics
    );
}

#[test]
fn unsupported_formats_say_why() {
    let (_guard, root) = scratch();
    let doc = root.join("doc.qmd");
    write(&doc, b"# Hi\n");
    for (format, needle) in [
        ("latex", "cannot be rendered by pandoc"),
        ("html", "cannot be rendered by pandoc"),
        ("nonsense", "cannot be rendered by pandoc"),
    ] {
        let out = render(&doc, format, None);
        assert!(out.request.is_none(), "{format}");
        assert!(
            out.error.as_deref().unwrap_or("").contains(needle),
            "{format}: {:?}",
            out.error
        );
    }
}

#[test]
fn typst_available_fonts_are_echoed() {
    let (_guard, root) = scratch();
    let doc = root.join("doc.qmd");
    write(&doc, b"# Hi\n");
    let runtime = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&doc, runtime.as_ref()).unwrap();
    let out = pollster::block_on(render_pandoc_request(
        PandocRequestInput {
            attribution: None,
            scope: quarto_core::pandoc_request::render::BookScope::Auto,
            captures_by_path: Default::default(),
            capture_error: None,
            hooks: None,
            path: &doc,
            content: b"# Hi\n",
            format: "docx",
            project: &project,
            source_date_epoch: None,
            captures: Vec::new(),
            typst_available_fonts: Some(vec!["Inter".into()]),
            resolver: None,
        },
        runtime,
    ));
    let request = out.request.unwrap();
    assert_eq!(request.typst_available_fonts, Some(vec!["Inter".into()]));
    assert_eq!(request.job_id, request.compute_job_id());
}

// --- resolver and table ----------------------------------------------------

fn resolved(path: &Path) -> (FormatSource, Vec<(String, FormatClass)>) {
    let r = resolve_document_formats(path, &NativeRuntime::new()).unwrap();
    (
        r.source,
        r.formats.into_iter().map(|f| (f.key, f.class)).collect(),
    )
}

#[test]
fn the_resolver_takes_the_first_key_of_a_format_map() {
    let (_guard, root) = scratch();
    let doc = root.join("doc.qmd");
    write(
        &doc,
        b"---\nformat:\n  docx: default\n  html: default\n---\n",
    );
    let (source, formats) = resolved(&doc);
    assert_eq!(source, FormatSource::Document);
    assert_eq!(formats[0], ("docx".to_string(), FormatClass::Download));
    assert_eq!(formats[1], ("html".to_string(), FormatClass::Preview));
}

#[test]
fn a_quarto_yml_format_applies_when_the_document_names_none() {
    let (_guard, root) = scratch();
    write(
        &root.join("_quarto.yml"),
        b"project:\n  type: default\nformat:\n  pptx: default\n",
    );
    let bare = root.join("bare.qmd");
    write(&bare, b"# Hi\n");
    let own = root.join("own.qmd");
    write(&own, b"---\nformat: docx\n---\n");
    let (source, formats) = resolved(&bare);
    assert_eq!(source, FormatSource::Project);
    assert_eq!(formats[0], ("pptx".to_string(), FormatClass::Download));
    let (source, formats) = resolved(&own);
    assert_eq!(source, FormatSource::Document, "the document wins");
    assert_eq!(formats[0].0, "docx");
}

#[test]
fn an_unknown_format_is_neither_and_no_format_is_html() {
    let (_guard, root) = scratch();
    let odd = root.join("odd.qmd");
    write(&odd, b"---\nformat: nonsense\n---\n");
    assert_eq!(resolved(&odd).1[0].1, FormatClass::Neither);
    let none = root.join("none.qmd");
    write(&none, b"# Hi\n");
    let (source, formats) = resolved(&none);
    assert_eq!(source, FormatSource::Default);
    assert_eq!(formats, vec![("html".to_string(), FormatClass::Preview)]);
}

/// The table is what the render function accepts: every `available` row
/// renders (a hidden row is accepted but not offered).
#[test]
fn the_table_agrees_with_what_render_pandoc_request_accepts() {
    let (_guard, root) = scratch();
    let doc = root.join("doc.qmd");
    write(&doc, b"# Hi\n");
    for info in PANDOC_FORMATS {
        let out = render(&doc, info.key, None);
        assert_eq!(out.request.is_some(), info.available, "{}", info.key);
    }
}

/// The hub passes the active file's authorship with a docx download, so a comment or tracked
/// change carries its author instead of pandoc's `unknown`.
fn render_with_attribution(path: &Path, qmd: &str) -> PandocRequestOutcome {
    let json = serde_json::json!({
        "runs": [{ "start": 0, "end": qmd.len(), "actor": "bear", "time": 1_700_000_000 }],
        "identities": { "bear": { "name": "Kind Bear", "color": "#ff0000" } }
    })
    .to_string();
    let runtime = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(path, runtime.as_ref()).unwrap();
    pollster::block_on(render_pandoc_request(
        PandocRequestInput {
            attribution: Some(Arc::new(
                quarto_core::attribution::PreBuiltAttributionProvider::new(json),
            )),
            scope: quarto_core::pandoc_request::render::BookScope::Chapter,
            captures_by_path: Default::default(),
            capture_error: None,
            hooks: None,
            path,
            content: qmd.as_bytes(),
            format: "docx",
            project: &project,
            source_date_epoch: Some(1_700_000_000),
            captures: Vec::new(),
            typst_available_fonts: None,
            resolver: None,
        },
        runtime,
    ))
}

fn ast_json(outcome: PandocRequestOutcome) -> String {
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    let request = outcome.request.expect("request");
    String::from_utf8(request.files[0].bytes.clone()).unwrap()
}

#[test]
fn a_docx_request_stamps_the_blamed_author_on_a_comment() {
    let (_guard, root) = scratch();
    let doc = root.join("doc.qmd");
    let qmd = "---\ntitle: T\n---\n\nSome [range [>> why?]] here.\n";
    write(&doc, qmd.as_bytes());
    let json = ast_json(render_with_attribution(&doc, qmd));
    assert!(json.contains(r#"["author","Kind Bear"]"#));
    assert!(!json.contains(r#"["author","unknown"]"#));
}

#[test]
fn a_docx_request_without_attribution_leaves_the_author_unknown() {
    let (_guard, root) = scratch();
    let doc = root.join("doc.qmd");
    write(
        &doc,
        b"---\ntitle: T\n---\n\nSome [range [>> why?]] here.\n",
    );
    let json = ast_json(render(&doc, "docx", None));
    assert!(json.contains(r#"["author","unknown"]"#));
}

#[test]
fn a_project_page_docx_request_stamps_the_blamed_author() {
    let (_guard, root) = scratch();
    write(&root.join("_quarto.yml"), b"project:\n  type: default\n");
    let doc = root.join("doc.qmd");
    let qmd = "---\ntitle: T\n---\n\nSome [range [>> why?]] here.\n";
    write(&doc, qmd.as_bytes());
    let json = ast_json(render_with_attribution(&doc, qmd));
    assert!(json.contains(r#"["author","Kind Bear"]"#));
}
