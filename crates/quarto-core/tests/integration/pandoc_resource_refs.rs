//! R2: `resource_refs` for docx. Each key of D3 gets a test: images,
//! `reference-doc`/`template`/`highlight-style`, user Lua filters with their
//! directory, the path rules and the size limits. Real files in a directory
//! outside `/tmp` (wasm-mode requests reject mounts under `/tmp`, and
//! Windows temp paths are `X:/`), read through `NativeRuntime`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use quarto_core::format::Format;
use quarto_core::pandoc_request::{PandocRequest, PrepareOptions, constants};
use quarto_core::pipeline::{build_pandoc_request_stages, run_pipeline};
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};
use quarto_error_reporting::{DiagnosticKind, DiagnosticMessage};

fn scratch() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("q2-r2-refs-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .expect("scratch dir under target/tmp")
}

/// A project at `root/proj` (the allowed root); `outside.png` lives in `root`.
struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    proj: PathBuf,
}

fn fixture() -> Fixture {
    let dir = scratch();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let proj = root.join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    Fixture {
        _dir: dir,
        root,
        proj,
    }
}

fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn wasm_opts() -> PrepareOptions {
    PrepareOptions {
        temp_root: PathBuf::from(&constants().share_root),
        source_date_epoch: Some(1_700_000_000),
        collect_resources: true,
    }
}

fn prepare(
    proj: &Path,
    qmd: &str,
    opts: Option<PrepareOptions>,
) -> (PandocRequest, Vec<DiagnosticMessage>) {
    let input = proj.join("doc.qmd");
    let project = ProjectContext {
        dir: proj.to_path_buf(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path(&input)],
        output_dir: proj.to_path_buf(),
        ..Default::default()
    };
    let doc = DocumentInfo::from_path(&input).with_output(proj.join("doc.docx"));
    let format = Format::docx();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
    ctx.prepare_options = opts;
    let runtime = Arc::new(quarto_system_runtime::NativeRuntime::new());
    let (_, diagnostics) = pollster::block_on(run_pipeline(
        qmd.as_bytes(),
        &input.to_string_lossy(),
        &mut ctx,
        runtime,
        build_pandoc_request_stages(Vec::new()),
    ))
    .expect("pipeline runs");
    let request = ctx.pandoc_request.take().expect("request");
    (request, diagnostics)
}

fn norm(p: &Path) -> String {
    quarto_core::pandoc_request::normalize_request_path(p)
}

fn ref_paths(r: &PandocRequest) -> Vec<&str> {
    r.resource_refs.iter().map(|f| f.path.as_str()).collect()
}

fn find<'a>(r: &'a PandocRequest, path: &Path) -> Option<&'a [u8]> {
    let p = norm(path);
    r.resource_refs
        .iter()
        .find(|f| f.path == p)
        .map(|f| f.bytes.as_slice())
}

fn errors(d: &[DiagnosticMessage]) -> Vec<&DiagnosticMessage> {
    d.iter()
        .filter(|m| m.kind == DiagnosticKind::Error)
        .collect()
}

#[test]
fn native_requests_carry_no_resource_refs() {
    let f = fixture();
    write(&f.proj.join("a.png"), b"PNG");
    let (r, d) = prepare(&f.proj, "![x](a.png)\n", None);
    assert!(r.resource_refs.is_empty());
    assert!(d.is_empty());
}

#[test]
fn image_bytes_are_copied_percent_decoded_and_deduped() {
    let f = fixture();
    write(&f.proj.join("img/pic 1.png"), b"ONE");
    write(&f.proj.join("two.jpg"), b"TWO");
    let qmd = "![a](img/pic%201.png)\n\n![a again](img/pic%201.png)\n\n![b](two.jpg)\n\n\
               ![remote](https://example.org/x.png)\n\n![data](data:image/png;base64,AAAA)\n";
    let (r, d) = prepare(&f.proj, qmd, Some(wasm_opts()));
    assert_eq!(find(&r, &f.proj.join("img/pic 1.png")), Some(&b"ONE"[..]));
    assert_eq!(find(&r, &f.proj.join("two.jpg")), Some(&b"TWO"[..]));
    assert_eq!(r.resource_refs.len(), 2, "{:?}", ref_paths(&r));
    assert!(errors(&d).is_empty(), "{d:?}");
}

#[test]
fn an_image_absent_from_the_snapshot_is_left_to_pandoc() {
    let f = fixture();
    let (r, d) = prepare(&f.proj, "![x](missing.png)\n", Some(wasm_opts()));
    assert!(r.resource_refs.is_empty());
    assert!(d.iter().all(|m| m.kind != DiagnosticKind::Error), "{d:?}");
}

#[test]
fn an_image_outside_the_project_is_not_mounted_and_is_reported() {
    let f = fixture();
    write(&f.root.join("outside.png"), b"OUT");
    write(&f.root.join("proj/sub/ok.png"), b"OK");
    let (r, d) = prepare(
        &f.proj,
        "![o](../outside.png)\n\n![ok](sub/ok.png)\n",
        Some(wasm_opts()),
    );
    assert_eq!(ref_paths(&r), vec![norm(&f.proj.join("sub/ok.png"))]);
    let note = d
        .iter()
        .find(|m| m.title.contains("outside the project"))
        .unwrap_or_else(|| panic!("no outside-the-project diagnostic: {d:?}"));
    assert_eq!(note.code.as_deref(), Some("Q-11-1"));
    assert!(note.title.contains("outside.png"));
}

#[test]
fn reference_doc_and_highlight_theme_are_mounted() {
    let f = fixture();
    write(&f.proj.join("ref.docx"), b"REFDOC");
    write(&f.proj.join("my.theme"), b"{}");
    let qmd = "---\nformat:\n  docx:\n    reference-doc: ref.docx\n    highlight-style: my.theme\n---\n\nHi\n";
    let (r, d) = prepare(&f.proj, qmd, Some(wasm_opts()));
    assert_eq!(find(&r, &f.proj.join("ref.docx")), Some(&b"REFDOC"[..]));
    assert_eq!(find(&r, &f.proj.join("my.theme")), Some(&b"{}"[..]));
    assert!(errors(&d).is_empty(), "{d:?}");
    // The argv names the same absolute paths the refs are mounted at.
    let i = r.argv.iter().position(|a| a == "--reference-doc").unwrap();
    assert_eq!(r.argv[i + 1], norm(&f.proj.join("ref.docx")));
}

#[test]
fn a_filter_in_a_subdirectory_mounts_its_directory_recursively() {
    let f = fixture();
    write(&f.proj.join("flt/main.lua"), b"return {}");
    write(&f.proj.join("flt/helper.lua"), b"return 1");
    write(&f.proj.join("flt/data/table.json"), b"{}");
    write(&f.proj.join("unrelated.txt"), b"no");
    let qmd = "---\nfilters:\n  - quarto\n  - flt/main.lua\n---\n\nHi\n";
    let (r, d) = prepare(&f.proj, qmd, Some(wasm_opts()));
    assert_eq!(
        ref_paths(&r),
        vec![
            norm(&f.proj.join("flt/data/table.json")),
            norm(&f.proj.join("flt/helper.lua")),
            norm(&f.proj.join("flt/main.lua")),
        ]
    );
    assert!(errors(&d).is_empty(), "{d:?}");
}

#[test]
fn a_filter_at_the_project_root_mounts_only_itself() {
    let f = fixture();
    write(&f.proj.join("root.lua"), b"return {}");
    write(&f.proj.join("sibling.lua"), b"return 1");
    write(&f.proj.join("notes.md"), b"no");
    let qmd = "---\nfilters:\n  - quarto\n  - root.lua\n---\n\nHi\n";
    let (r, _) = prepare(&f.proj, qmd, Some(wasm_opts()));
    assert_eq!(ref_paths(&r), vec![norm(&f.proj.join("root.lua"))]);
}

#[test]
fn a_json_filter_cannot_run_in_the_browser() {
    let f = fixture();
    write(&f.proj.join("flt.json"), b"#!/bin/sh\n");
    let qmd = "---\nfilters:\n  - quarto\n  - flt.json\n---\n\nHi\n";
    let (_, d) = prepare(&f.proj, qmd, Some(wasm_opts()));
    let errs = errors(&d);
    assert_eq!(errs.len(), 1, "{d:?}");
    assert!(errs[0].title.contains("flt.json"), "{}", errs[0].title);
}

#[test]
fn an_image_over_the_limit_stops_collection_with_an_error_naming_it() {
    let f = fixture();
    let big = f.proj.join("big.png");
    let file = std::fs::File::create(&big).unwrap();
    file.set_len(constants().limits.image_bytes + 1).unwrap();
    write(&f.proj.join("small.png"), b"s");
    let (r, d) = prepare(
        &f.proj,
        "![b](big.png)\n\n![s](small.png)\n",
        Some(wasm_opts()),
    );
    assert!(
        find(&r, &big).is_none(),
        "oversized image must not be copied"
    );
    let errs = errors(&d);
    assert_eq!(errs.len(), 1, "{d:?}");
    assert!(errs[0].title.contains("big.png"), "{}", errs[0].title);
    assert!(
        find(&r, &f.proj.join("small.png")).is_none(),
        "collection stops at the first limit"
    );
}

#[test]
fn mounted_bytes_count_against_the_total_limit() {
    use quarto_core::pandoc_request::{ResourceCollector, ResourceKind};

    let f = fixture();
    write(&f.proj.join("a.bin"), b"12");
    write(&f.proj.join("b.bin"), b"1");
    let runtime = quarto_system_runtime::NativeRuntime::new();
    // The request already carries all but one byte of the budget.
    let mut collector = ResourceCollector::new(
        &runtime,
        &f.proj,
        Path::new("/__q2_share__"),
        constants().limits.total_bytes - 1,
    );
    collector.add_file(&f.proj.join("a.bin"), ResourceKind::Other);
    collector.add_file(&f.proj.join("b.bin"), ResourceKind::Other);
    let (refs, d) = collector.finish();
    assert!(
        refs.is_empty(),
        "a.bin (2 bytes) does not fit; b.bin is not tried"
    );
    assert_eq!(errors(&d).len(), 1, "{d:?}");
    assert!(errors(&d)[0].title.contains("a.bin"));
}
