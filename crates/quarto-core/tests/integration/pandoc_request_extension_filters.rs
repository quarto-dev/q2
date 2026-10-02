//! R9 task 1a: the built-in extension subtrees (the default `orange-book`
//! book extension) are mounted into a typst/pdf request, so pandoc can load
//! the extension's filter. Native twin of the wasm vitest.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use quarto_core::ResourceResolverContext;
use quarto_core::extension::builtin_extension_subtree_roots;
use quarto_core::format::Format;
use quarto_core::pandoc_request::render::{
    PandocRequestInput, PandocRequestOutcome, render_pandoc_request,
};
use quarto_core::pandoc_request::{PandocRequest, PrepareOptions, RequestPost, constants};
use quarto_core::pipeline::{build_pandoc_pipeline_stages, run_pipeline};
use quarto_core::project::orchestrator::project_type_for;
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext, RenderOptions};
use quarto_core::stage::stages::{PandocPrepareStage, PandocWriteStage};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

/// A directory outside `/tmp`: `validate_mounts` rejects mounts under it, and
/// native extracts the subtrees to `/tmp` on Linux.
fn scratch() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::Builder::new()
        .prefix("q2-r9-extfilter-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    (dir, root)
}

fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn copy_tree(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let to = dst.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &to);
        } else {
            std::fs::copy(entry.path(), to).unwrap();
        }
    }
}

/// Point `QUARTO_EXTENSION_SUBTREES_DIR` at a copy of the extracted subtrees
/// under the scratch root. Each nextest test is its own process, so setting
/// the environment is safe.
fn relocate_subtrees() -> (tempfile::TempDir, PathBuf) {
    let runtime = NativeRuntime::new();
    let extracted = builtin_extension_subtree_roots(&runtime);
    let src = extracted.first().expect("an extracted subtree root");
    // Outside the project root, as the extracted subtrees are.
    let (guard, root) = scratch();
    let dst = root.join("subtrees");
    copy_tree(src, &dst);
    unsafe { std::env::set_var("QUARTO_EXTENSION_SUBTREES_DIR", &dst) };
    (guard, dst)
}

fn render(path: &Path, format: &str) -> PandocRequestOutcome {
    let runtime = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(path, runtime.as_ref()).unwrap();
    let content = std::fs::read(path).unwrap();
    pollster::block_on(render_pandoc_request(
        PandocRequestInput {
            path,
            content: &content,
            format,
            project: &project,
            source_date_epoch: Some(1_700_000_000),
            captures: Vec::new(),
            typst_available_fonts: None,
            resolver: Some(ResourceResolverContext::vfs_root(
                "/.quarto/project-artifacts",
            )),
        },
        runtime,
    ))
}

fn book_project(root: &Path) {
    write(
        &root.join("_quarto.yml"),
        b"project:\n  type: book\nbook:\n  title: B\n  chapters:\n    - index.qmd\n    - one.qmd\n",
    );
    write(&root.join("index.qmd"), b"# Preface\n\nHello\n");
    write(&root.join("one.qmd"), b"# One\n\nFirst chapter.\n");
}

/// The decoded `QUARTO_FILTER_PARAMS` blob.
fn params(request: &PandocRequest) -> serde_json::Value {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&request.env["QUARTO_FILTER_PARAMS"])
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

/// The orange-book filter entry points of the request, as written in
/// `QUARTO_FILTER_PARAMS`.
fn filter_entry_points(request: &PandocRequest) -> Vec<String> {
    params(request)["quarto-filters"]["entryPoints"]
        .as_array()
        .expect("entryPoints")
        .iter()
        .map(|e| e["path"].as_str().unwrap().to_string())
        .collect()
}

fn file_paths(request: &PandocRequest) -> Vec<&str> {
    request.files.iter().map(|f| f.path.as_str()).collect()
}

#[test]
fn the_built_in_book_filter_and_its_directory_are_mounted_as_files() {
    let (_g, root) = scratch();
    let (_sub, _subtrees) = relocate_subtrees();
    book_project(&root);
    let out = render(&root.join("one.qmd"), "typst");
    assert!(out.error.is_none(), "{:?}", out.error);
    assert!(
        out.diagnostics
            .iter()
            .all(|d| d.code.as_deref() != Some("Q-11-1")),
        "no outside-the-project warning: {:?}",
        out.diagnostics.iter().map(|d| &d.title).collect::<Vec<_>>()
    );
    let request = out.request.expect("request");

    // The filter and a sibling of it (the whole directory) are in `files`.
    let files = file_paths(&request);
    assert!(
        files
            .iter()
            .any(|p| p.ends_with("/orange-book/orange-book.lua")),
        "{files:?}"
    );
    assert!(
        files
            .iter()
            .any(|p| p.ends_with("/orange-book/_extension.yml")),
        "the directory, not just the filter: {files:?}"
    );
    // Never in `resource_refs`: the host validator rejects those outside the
    // project root.
    assert!(
        request
            .resource_refs
            .iter()
            .all(|f| !f.path.contains("/orange-book/")),
        "{:?}",
        request
            .resource_refs
            .iter()
            .map(|f| &f.path)
            .collect::<Vec<_>>()
    );
    // Every entry point names a mounted file.
    let entries = filter_entry_points(&request);
    assert!(!entries.is_empty());
    for entry in &entries {
        assert!(
            files.contains(&entry.as_str()),
            "entry point {entry} is not a mounted file: {files:?}"
        );
    }
}

#[test]
fn mounted_files_count_against_the_total_limit() {
    // The filter directory is about 650 KB, so it is in the request's size.
    let (_g, root) = scratch();
    let (_sub, _subtrees) = relocate_subtrees();
    book_project(&root);
    let request = render(&root.join("one.qmd"), "typst").request.unwrap();
    let ext: usize = request
        .files
        .iter()
        .filter(|f| f.path.contains("/orange-book/"))
        .map(|f| f.bytes.len())
        .sum();
    assert!(ext > 100_000, "the directory mount is substantial: {ext}");
    assert!(
        (ext as u64) < constants().limits.total_bytes,
        "and well inside the limit"
    );
}

/// The prepared request for `doc_path`, built the way native builds it but
/// with a writable `temp_root`: `PandocWriteStage::execute` writes
/// `request.files` at their absolute paths and the wasm share root is not
/// writable.
fn prepared_request(doc_path: &Path, temp_root: &Path) -> PandocRequest {
    let runtime = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(doc_path, runtime.as_ref()).unwrap();
    let format = Format::from_format_string("typst").unwrap();
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
        temp_root: temp_root.to_path_buf(),
        source_date_epoch: Some(1_700_000_000),
        collect_resources: true,
        typst_available_fonts: None,
        post: RequestPost::None,
    });
    let mut stages = build_pandoc_pipeline_stages(format.identifier);
    let write_at = stages
        .iter()
        .position(|s| s.name() == "pandoc-write")
        .expect("pandoc-write stage");
    stages.truncate(write_at);
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

#[test]
fn real_pandoc_loads_the_filter_from_the_request_alone() {
    let (_g, root) = scratch();
    let (_sub, subtrees) = relocate_subtrees();
    book_project(&root);
    let temp_root = root.join("tmp-share");
    let request = prepared_request(&root.join("one.qmd"), &temp_root);
    assert!(
        file_paths(&request)
            .iter()
            .any(|p| p.ends_with("/orange-book/orange-book.lua")),
        "the filter travels in the request"
    );

    // Remove the extracted subtree: pandoc can only find the filter if the
    // request carries it (`execute` writes `files` at their absolute paths).
    std::fs::remove_dir_all(&subtrees).unwrap();
    assert!(!subtrees.join("orange-book/orange-book.lua").exists());

    let runtime = NativeRuntime::new();
    let pandoc = runtime
        .find_binary("pandoc", "QUARTO_PANDOC")
        .expect("pandoc is installed for the native replay tests");
    PandocWriteStage::new()
        .execute(&request, &pandoc)
        .expect("pandoc runs the book filter from the mounted copy");
    let typ = std::fs::read_to_string(&request.output_path).unwrap();
    assert!(typ.contains("First chapter"), "{typ}");
}
