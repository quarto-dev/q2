//! R7 stage 0-1: pandoc requests for documents inside a `_quarto.yml`
//! project (and the active page of a book), driven natively
//! (`WasmRuntime` is wasm32-only; the wasm wrapper is covered by the
//! hub-client vitest).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use quarto_core::ResourceResolverContext;
use quarto_core::format::Format;
use quarto_core::pandoc_request::render::{
    PandocRequestInput, PandocRequestOutcome, render_pandoc_request,
};
use quarto_core::pandoc_request::{PandocRequest, PrepareOptions, constants};
use quarto_core::pipeline::{build_pandoc_pipeline_stages, run_pipeline};
use quarto_core::project::orchestrator::project_type_for;
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext, RenderOptions};
use quarto_core::stage::stages::PandocPrepareStage;
use quarto_system_runtime::NativeRuntime;
use serde_json::Value;

/// A directory outside `/tmp` (wasm-mode requests reject mounts under it).
fn scratch() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::Builder::new()
        .prefix("q2-r7-projects-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    // Extension bundles (orange-book) extract into the process temp dir, `/tmp` on Linux, which
    // wasm-mode requests reject; keep it under the target dir too.
    // SAFETY: one test per process under nextest.
    unsafe { std::env::set_var("TMPDIR", env!("CARGO_TARGET_TMPDIR")) };
    (dir, root)
}

fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn render(path: &Path, format: &str) -> PandocRequestOutcome {
    let runtime = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(path, runtime.as_ref()).unwrap();
    let content = std::fs::read(path).unwrap();
    pollster::block_on(render_pandoc_request(
        PandocRequestInput {
            attribution: None,
            scope: quarto_core::pandoc_request::render::BookScope::Chapter,
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
            // As the hub prelude installs it.
            resolver: Some(ResourceResolverContext::vfs_root(
                "/.quarto/project-artifacts",
            )),
        },
        runtime,
    ))
}

fn request_of(out: PandocRequestOutcome) -> PandocRequest {
    assert!(out.error.is_none(), "{:?}", out.error);
    out.request.expect("request")
}

/// The `Image` targets of the request's `pandoc-input.json`, in document order.
fn image_targets(request: &PandocRequest) -> Vec<String> {
    let input = request
        .files
        .iter()
        .find(|f| f.path.ends_with("/pandoc-input.json"))
        .expect("pandoc-input.json");
    let json: Value = serde_json::from_slice(&input.bytes).unwrap();
    let mut out = Vec::new();
    fn walk(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::Object(map) => {
                if map.get("t").and_then(Value::as_str) == Some("Image")
                    && let Some(target) = map["c"][2][0].as_str()
                {
                    out.push(target.to_string());
                }
                map.values().for_each(|v| walk(v, out));
            }
            Value::Array(items) => items.iter().for_each(|v| walk(v, out)),
            _ => {}
        }
    }
    walk(&json, &mut out);
    out
}

fn ref_paths(request: &PandocRequest) -> Vec<String> {
    request
        .resource_refs
        .iter()
        .map(|f| f.path.clone())
        .collect()
}

/// What native does for the same document: the real pre-write pipeline over
/// the project's website-mode resolver (`render_to_file.rs`), with
/// `PandocPrepareStage` in place of `PandocWriteStage`.
fn native_request(doc_path: &Path, format_key: &str) -> PandocRequest {
    let runtime = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(doc_path, runtime.as_ref()).unwrap();
    let format = Format::from_format_string(format_key).unwrap();
    let rel = doc_path.strip_prefix(&project.dir).unwrap();
    let output_path = project
        .output_dir
        .join(rel)
        .with_extension(&format.output_extension);
    let doc = DocumentInfo::from_path(doc_path).with_output(output_path.clone());
    let binaries = BinaryDependencies::new();
    let mut ctx =
        RenderContext::new(&project, &doc, &format, &binaries).with_options(RenderOptions {
            verbose: false,
            execute: false,
            use_freeze: false,
            output_path: Some(output_path.clone()),
        });
    ctx.resource_resolver = Some(ResourceResolverContext::website(
        &project.output_dir,
        &output_path,
        project_type_for(&project).lib_dir(),
        doc_path.file_stem().unwrap().to_string_lossy(),
    ));
    ctx.prepare_options = Some(PrepareOptions {
        temp_root: PathBuf::from(&constants().share_root),
        source_date_epoch: Some(1_700_000_000),
        collect_resources: true,
        typst_available_fonts: None,
        post: quarto_core::pandoc_request::RequestPost::None,
    });
    let mut stages = build_pandoc_pipeline_stages(format.identifier);
    stages.pop();
    stages.push(Box::new(PandocPrepareStage::new()));
    let content = std::fs::read(doc_path).unwrap();
    let source_name = doc_path.to_string_lossy();
    pollster::block_on(run_pipeline(
        &content,
        &source_name,
        &mut ctx,
        runtime,
        stages,
    ))
    .expect("native pipeline runs");
    ctx.pandoc_request.take().expect("request")
}

const DEFAULT_PROJECT: &str = "project:\n  type: default\n";

#[test]
fn a_project_with_format_html_still_yields_a_docx_request() {
    let (_guard, root) = scratch();
    write(
        &root.join("_quarto.yml"),
        b"project:\n  type: default\nformat:\n  html:\n    toc: true\n",
    );
    let doc = root.join("doc.qmd");
    write(&doc, b"---\ntitle: T\n---\n\n# Hi\n");
    let request = request_of(render(&doc, "docx"));
    assert_eq!(request.writer, "docx");
    assert!(
        request.argv.iter().all(|a| a != "--toc"),
        "the html format's options must not leak into docx: {:?}",
        request.argv
    );
}

#[test]
fn a_document_that_names_html_in_a_project_with_html_still_yields_docx() {
    // Re-detection from the document's own `format:` must not override the
    // requested format either.
    let (_guard, root) = scratch();
    write(&root.join("_quarto.yml"), DEFAULT_PROJECT.as_bytes());
    let doc = root.join("sub/doc.qmd");
    write(&doc, b"---\nformat: html\n---\n\n# Hi\n");
    assert_eq!(request_of(render(&doc, "docx")).writer, "docx");
}

#[test]
fn project_level_reference_doc_resolves_against_the_declaring_file() {
    let (_guard, root) = scratch();
    write(
        &root.join("_quarto.yml"),
        b"project:\n  type: default\nformat:\n  docx:\n    reference-doc: ref.docx\n",
    );
    write(&root.join("ref.docx"), b"REFDOCX");
    // A same-named decoy next to the document: resolving against the
    // document's directory would pick this one (or nothing).
    write(&root.join("sub/ref.docx"), b"DECOY");
    let doc = root.join("sub/doc.qmd");
    write(&doc, b"---\ntitle: T\n---\n\n# Hi\n");
    let request = request_of(render(&doc, "docx"));

    let declared = format!("{}/ref.docx", root.to_string_lossy().replace('\\', "/"));
    let mounted = request
        .resource_refs
        .iter()
        .find(|f| f.path.ends_with("/ref.docx"))
        .unwrap_or_else(|| panic!("ref.docx not mounted: {:?}", ref_paths(&request)));
    assert!(
        mounted.path == declared
            || mounted
                .path
                .ends_with(&declared[declared.find('/').unwrap()..]),
        "{} vs {declared}",
        mounted.path
    );
    assert_eq!(
        mounted.bytes, b"REFDOCX",
        "the project's file, not the decoy"
    );
    assert_eq!(request.resource_refs.len(), 1, "{:?}", ref_paths(&request));
    assert!(
        request.argv.iter().any(|a| a.ends_with("/ref.docx")),
        "{:?}",
        request.argv
    );
}

#[test]
fn a_document_level_reference_doc_resolves_against_the_document() {
    let (_guard, root) = scratch();
    write(
        &root.join("_quarto.yml"),
        b"project:\n  type: default\nformat:\n  docx:\n    reference-doc: ref.docx\n",
    );
    write(&root.join("ref.docx"), b"REFDOCX");
    write(&root.join("sub/own.docx"), b"OWNDOCX");
    let doc = root.join("sub/doc.qmd");
    write(
        &doc,
        b"---\nformat:\n  docx:\n    reference-doc: own.docx\n---\n\n# Hi\n",
    );
    let request = request_of(render(&doc, "docx"));
    let own = request
        .resource_refs
        .iter()
        .find(|f| f.path.ends_with("/own.docx"))
        .unwrap_or_else(|| panic!("own.docx not mounted: {:?}", ref_paths(&request)));
    assert_eq!(own.bytes, b"OWNDOCX");
}

fn assert_images_match_native(root: &Path, project_yml: &str) {
    write(&root.join("_quarto.yml"), project_yml.as_bytes());
    write(&root.join("img/a.png"), b"A");
    write(&root.join("img/c.png"), b"C");
    write(&root.join("sub/pic.png"), b"P");
    let doc = root.join("sub/page.qmd");
    write(
        &doc,
        b"---\ntitle: T\n---\n\n![a](/img/a.png)\n\n![b](pic.png)\n\n![c](../img/c.png)\n",
    );
    let ours = request_of(render(&doc, "docx"));
    let native = native_request(&doc, "docx");
    assert_eq!(
        image_targets(&ours),
        image_targets(&native),
        "image targets must equal native's"
    );
    // Not vacuous: the site-root image is no longer written as `/img/a.png`.
    assert_eq!(
        image_targets(&ours),
        vec!["../img/a.png", "pic.png", "../img/c.png"]
    );
    // And every one of them is mounted, from its resolved location.
    let refs = ref_paths(&ours);
    for name in ["img/a.png", "sub/pic.png", "img/c.png"] {
        assert!(
            refs.iter().any(|p| p.ends_with(name)),
            "{name} not mounted: {refs:?}"
        );
    }
    assert_eq!(refs.len(), 3, "{refs:?}");
}

#[test]
fn image_targets_equal_native_when_output_dir_is_the_project_dir() {
    let (_guard, root) = scratch();
    assert_images_match_native(&root, DEFAULT_PROJECT);
}

#[test]
fn image_targets_equal_native_when_output_dir_differs() {
    let (_guard, root) = scratch();
    assert_images_match_native(&root, "project:\n  type: default\n  output-dir: _out\n");
}

#[test]
fn a_website_project_document_yields_a_request() {
    let (_guard, root) = scratch();
    write(
        &root.join("_quarto.yml"),
        b"project:\n  type: website\nwebsite:\n  title: W\n  navbar:\n    left:\n      - index.qmd\n",
    );
    write(&root.join("index.qmd"), b"---\ntitle: Home\n---\n\nHome\n");
    let doc = root.join("about.qmd");
    write(&doc, b"---\ntitle: About\n---\n\nAbout [home](index.qmd)\n");
    let request = request_of(render(&doc, "docx"));
    assert_eq!(request.writer, "docx");
}

#[test]
fn a_document_with_errors_in_a_project_yields_no_request() {
    let (_guard, root) = scratch();
    write(&root.join("_quarto.yml"), DEFAULT_PROJECT.as_bytes());
    let doc = root.join("doc.qmd");
    write(&doc, b"---\ntitle: [unclosed\n---\n\n# Hi\n");
    let out = render(&doc, "docx");
    assert!(out.request.is_none());
    assert!(out.error.is_some());
    assert!(
        !out.diagnostics.is_empty(),
        "the parse error must reach the host as diagnostics"
    );
}

#[test]
fn project_sibling_with_errors_does_not_block_the_active_page() {
    let (_guard, root) = scratch();
    write(&root.join("_quarto.yml"), DEFAULT_PROJECT.as_bytes());
    write(
        &root.join("broken.qmd"),
        b"---\ntitle: [unclosed\n---\n\nx\n",
    );
    let doc = root.join("ok.qmd");
    write(&doc, b"---\ntitle: T\n---\n\n# Hi\n");
    assert_eq!(request_of(render(&doc, "docx")).writer, "docx");
}

fn book_project(root: &Path) {
    write(
        &root.join("_quarto.yml"),
        b"project:\n  type: book\nbook:\n  title: B\n  chapters:\n    - index.qmd\n    - one.qmd\n    - two.qmd\n",
    );
    write(&root.join("index.qmd"), b"# Preface\n\nHello\n");
    // Single-token markers: pandoc JSON splits prose into `Str` /
    // `Space` nodes, so a multi-word phrase never appears verbatim in
    // the body.
    write(&root.join("one.qmd"), b"# One\n\nFirstChapterBody.\n");
    write(&root.join("two.qmd"), b"# Two\n\nSecondChapterBody.\n");
}

#[test]
fn a_book_chapter_renders_alone_as_the_active_page() {
    // No native baseline: native renders a whole book or nothing for these
    // formats. Interim: typst (and so pdf) and epub consolidate a book
    // natively, so they should become a whole-book download (R7 stages 2-3,
    // own plan); until then the active page renders alone, as for any
    // project document.
    let (_guard, root) = scratch();
    book_project(&root);
    let doc = root.join("one.qmd");
    let request = request_of(render(&doc, "typst"));
    assert_eq!(request.writer, "typst");
    let input = request
        .files
        .iter()
        .find(|f| f.path.ends_with("/pandoc-input.json"))
        .unwrap();
    let json = String::from_utf8_lossy(&input.bytes);
    assert!(json.contains("FirstChapterBody"), "the active chapter");
    assert!(!json.contains("SecondChapterBody"), "no other chapter");
    assert!(!json.contains("Hello"), "no other chapter");
}

#[test]
fn a_book_chapter_downloads_as_docx_and_pptx_without_native_rejection_or_warning() {
    // Native refuses docx/pptx for a book (Q-5-33: no single-file book
    // merge). For these formats, which have no single-file book, a chapter
    // download is the active page alone, like the html preview: allowed,
    // silently (decided with Gordon). This is the settled behavior, unlike
    // the typst case above, which is interim.
    let (_guard, root) = scratch();
    book_project(&root);
    let doc = root.join("one.qmd");
    for format in ["docx", "pptx"] {
        let out = render(&doc, format);
        assert!(
            out.diagnostics
                .iter()
                .all(|d| d.code.as_deref() != Some("Q-5-33")),
            "{format}: {:?}",
            out.diagnostics
        );
        assert!(
            out.diagnostics.is_empty(),
            "{format}: {:?}",
            out.diagnostics
        );
        let request = request_of(out);
        assert_eq!(request.writer, format);
        let input = request
            .files
            .iter()
            .find(|f| f.path.ends_with("/pandoc-input.json"))
            .unwrap();
        let json = String::from_utf8_lossy(&input.bytes);
        assert!(json.contains("FirstChapterBody"), "{format}");
        assert!(!json.contains("SecondChapterBody"), "{format}");
    }
}
