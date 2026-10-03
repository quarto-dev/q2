//! R9: the whole-book pandoc request (`render_pandoc_request` for a book
//! chapter, typst/pdf/epub), driven natively with `NativeRuntime` over a
//! directory (`WasmRuntime` is wasm32-only; the wasm halves are in
//! `pandocRequest.wasm.test.ts`).
//!
//! The parity tests render the same book natively (`run_with_book_support`)
//! and through the request, replayed in real `pandoc`, and compare the
//! `.typ`; they need real `pandoc` and `typst` like the other book tests.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use quarto_core::ResourceResolverContext;
use quarto_core::engine::ExecutionPolicy;
use quarto_core::format::Format;
use quarto_core::pandoc_request::render::{
    BookScope, PandocRequestInput, PandocRequestOutcome, ResolvedBookScope, render_pandoc_request,
};
use quarto_core::pandoc_request::{PandocRequest, constants};
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, project_type_for};
use quarto_core::render_to_file::RenderToFileOptions;
use quarto_core::stage::stages::PandocWriteStage;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

const ONE_PIXEL_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
    0x42, 0x60, 0x82,
];

const REFS_BIB: &str = "@book{knuth1984,\n  author = {Knuth, Donald E.},\n  title = {The TeXbook},\n  year = {1984},\n  publisher = {Addison-Wesley}\n}\n@article{doe2020,\n  author = {Doe, Jane},\n  title = {A Paper},\n  year = {2020}\n}\n";

/// A directory outside `/tmp` (request mounts reject it).
fn scratch() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::Builder::new()
        .prefix("q2-r9-books-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    // The hub's VFS project root is the runtime's working directory, and
    // capture keys are relative to it (`sidecar_key`). Each nextest test is
    // its own process, so changing the directory is private to the test.
    std::env::set_current_dir(&root).unwrap();
    (dir, root)
}

fn write(root: &Path, rel: &str, bytes: &[u8]) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// Fixture A: the parity book. Two parts (one with an `href:` page), an
/// appendix chapter, an `index.qmd`, cross-chapter `@sec-` references and
/// links, a bibliography cited from two chapters, a subdirectory chapter with
/// local, `../` and site-root images, local and site-root images in root
/// chapters, a title-only chapter, a chapter with both `title:` and its own
/// heading, a chapter with neither, and a chapter with no `title:` and text
/// before an H2 first heading. No brand logos or custom fonts, and no code
/// cells (native would run them; the request splices captures).
fn parity_book(root: &Path) {
    write(
        root,
        "_quarto.yml",
        b"project:\n  type: book\n\nkeep-typ: true\n\nbook:\n  title: \"Parity Book\"\n  author: \"A. Author\"\n  cover-image: cover.png\n  chapters:\n    - index.qmd\n    - part: \"Part One\"\n      chapters:\n        - one.qmd\n        - sub/two.qmd\n    - part: \"Part Two\"\n      href: partpage.qmd\n      chapters:\n        - titleonly.qmd\n        - withh1.qmd\n        - neither.qmd\n        - textfirst.qmd\n  appendices:\n    - app.qmd\n\nbibliography: refs.bib\nciteproc: true\n",
    );
    write(root, "refs.bib", REFS_BIB.as_bytes());
    write(
        root,
        "index.qmd",
        b"---\ntitle: Preface\n---\n\nWelcome. See @sec-one and [chapter two](sub/two.qmd).\n",
    );
    write(
        root,
        "one.qmd",
        b"# One {#sec-one}\n\nKnuth wrote it [@knuth1984].\n\n![root local](rootlocal.png)\n\n![root site](/img/a.png)\n",
    );
    write(
        root,
        "sub/two.qmd",
        b"# Two {#sec-two}\n\nDoe and Knuth [@doe2020; @knuth1984].\n\n![local](local.png)\n\n![site root](/img/a.png)\n\n![up](../top.png)\n\nBack to @sec-one and [chapter one](../one.qmd).\n",
    );
    write(
        root,
        "partpage.qmd",
        b"---\ntitle: Part Two Page\n---\n\nIntro to part two.\n",
    );
    write(
        root,
        "titleonly.qmd",
        b"---\ntitle: Title Only Chapter\n---\n\nText with no heading.\n",
    );
    write(
        root,
        "withh1.qmd",
        b"---\ntitle: Meta Title\n---\n\n# Heading Y\n\nBody y.\n",
    );
    write(
        root,
        "neither.qmd",
        b"Just text, no title and no heading.\n",
    );
    write(
        root,
        "textfirst.qmd",
        b"Some leading text.\n\n## Early Heading\n\nBody.\n",
    );
    write(root, "app.qmd", b"# Appendix Alpha\n\nAppendix body.\n");
    for f in [
        "rootlocal.png",
        "top.png",
        "img/a.png",
        "sub/local.png",
        "cover.png",
    ] {
        write(root, f, ONE_PIXEL_PNG);
    }
}

fn input<'a>(
    runtime: &dyn SystemRuntime,
    path: &'a Path,
    content: &'a [u8],
    format: &'a str,
    project: &'a ProjectContext,
    scope: BookScope,
) -> PandocRequestInput<'a> {
    let _ = runtime;
    PandocRequestInput {
        path,
        content,
        format,
        project,
        source_date_epoch: Some(1_700_000_000),
        captures: Vec::new(),
        typst_available_fonts: None,
        // As the hub prelude installs it.
        resolver: Some(ResourceResolverContext::vfs_root(
            "/.quarto/project-artifacts",
        )),
        scope,
        captures_by_path: Default::default(),
        capture_error: None,
        hooks: None,
    }
}

fn render(path: &Path, format: &str, scope: BookScope) -> PandocRequestOutcome {
    let runtime = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(path, runtime.as_ref()).unwrap();
    let content = std::fs::read(path).unwrap();
    pollster::block_on(render_pandoc_request(
        input(runtime.as_ref(), path, &content, format, &project, scope),
        runtime,
    ))
}

fn request_of(out: PandocRequestOutcome) -> PandocRequest {
    assert!(
        out.error.is_none(),
        "{:?} {:?}",
        out.error,
        out.diagnostics.iter().map(|d| &d.title).collect::<Vec<_>>()
    );
    out.request.expect("request")
}

/// The document's text as a reader sees it: every `Str` joined, `Space`s as
/// blanks. (The input JSON stores words as separate nodes, so a phrase is not
/// a substring of it.)
fn plain_text(request: &PandocRequest) -> String {
    fn walk(v: &serde_json::Value, out: &mut String) {
        match v {
            serde_json::Value::Object(map) => {
                match map.get("t").and_then(|t| t.as_str()) {
                    Some("Str") => {
                        if let Some(text) = map.get("c").and_then(|c| c.as_str()) {
                            out.push_str(text);
                        }
                    }
                    Some("Space" | "SoftBreak") => out.push(' '),
                    _ => {}
                }
                map.values().for_each(|v| walk(v, out));
            }
            serde_json::Value::Array(items) => items.iter().for_each(|v| walk(v, out)),
            _ => {}
        }
    }
    let json: serde_json::Value = serde_json::from_str(&input_json(request)).unwrap();
    let mut out = String::new();
    walk(&json, &mut out);
    out
}

fn input_json(request: &PandocRequest) -> String {
    let file = request
        .files
        .iter()
        .find(|f| f.path.ends_with("/pandoc-input.json"))
        .expect("pandoc-input.json");
    String::from_utf8(file.bytes.clone()).unwrap()
}

// ---------------------------------------------------------------------
// Replaying a request in real pandoc
// ---------------------------------------------------------------------

/// Re-root a request built for the wasm share root (`/__q2_share__`, not
/// writable) under `new_root`, including the paths inside the filter-params
/// blob, so `PandocWriteStage::execute` can run it natively.
fn reroot(request: &mut PandocRequest, new_root: &Path) {
    use base64::Engine as _;
    let from = constants().share_root.clone();
    let to = quarto_core::pandoc_request::normalize_request_path(new_root);
    let swap = |s: &str| s.replace(&from, &to);
    request.share_root = swap(&request.share_root);
    request.share_tree_path = swap(&request.share_tree_path);
    request.json_path = swap(&request.json_path);
    request.argv = request.argv.iter().map(|a| swap(a)).collect();
    for f in &mut request.files {
        f.path = swap(&f.path);
    }
    request.dirs = request
        .dirs
        .iter()
        .map(|d| swap(d))
        .filter(|d| d != "/tmp")
        .collect();
    let env: Vec<(String, String)> = request
        .env
        .iter()
        .map(|(k, v)| {
            let v = if k == "QUARTO_FILTER_PARAMS" {
                let json = base64::engine::general_purpose::STANDARD
                    .decode(v)
                    .expect("params blob is base64");
                let text = String::from_utf8(json).unwrap();
                base64::engine::general_purpose::STANDARD.encode(swap(&text))
            } else {
                swap(v)
            };
            (k.clone(), v)
        })
        .collect();
    request.env = env.into_iter().collect();
}

/// Run `request` in real pandoc (writing `files` at their absolute paths
/// first) and return the produced output file's text.
fn replay_in_pandoc(mut request: PandocRequest, scratch_root: &Path) -> String {
    reroot(&mut request, &scratch_root.join("share"));
    let runtime = NativeRuntime::new();
    let pandoc = runtime
        .find_binary("pandoc", "QUARTO_PANDOC")
        .expect("pandoc is installed for the native replay tests");
    PandocWriteStage::new()
        .execute(&request, &pandoc)
        .expect("pandoc runs the request");
    std::fs::read_to_string(&request.output_path).expect("pandoc wrote the output")
}

/// What native `q2 render` writes for the book: the kept `.typ`.
fn native_typ(root: &Path) -> String {
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(root, runtime.as_ref()).unwrap();
    let project_type = project_type_for(&project);
    let format = Format::from_format_string("typst").unwrap();
    let options = RenderToFileOptions {
        execution_policy: ExecutionPolicy::None,
        ..Default::default()
    };
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        format,
        "typst",
        &options,
        runtime,
    );
    let summary = pollster::block_on(pipeline.run_with_book_support()).expect("native render");
    assert_eq!(summary.outputs.len(), 1, "{summary:?}");
    let typ = summary.outputs[0].output_path.with_extension("typ");
    std::fs::read_to_string(&typ).unwrap_or_else(|e| panic!("kept {}: {e}", typ.display()))
}

/// Strip what legitimately differs between the two runs: absolute paths and
/// the share root.
fn comparable(text: &str, roots: &[&Path]) -> String {
    let mut out = text.to_string();
    for root in roots {
        out = out.replace(&root.to_string_lossy().replace('\\', "/"), "<ROOT>");
    }
    out.replace(&constants().share_root, "<SHARE>")
}

#[test]
fn whole_book_typst_request_equals_native() {
    let (_g, root) = scratch();
    parity_book(&root);
    // The pin must reach native pandoc too.
    unsafe { std::env::set_var("SOURCE_DATE_EPOCH", "1700000000") };

    let native = native_typ(&root);

    let out = render(&root.join("one.qmd"), "typst", BookScope::Auto);
    let book = out.book.as_ref().expect("a book outcome").scope;
    assert_eq!(book, ResolvedBookScope::Book);
    let request = request_of(out);
    let from_request = replay_in_pandoc(request, &root);

    let a = comparable(&native, &[&root]);
    let b = comparable(&from_request, &[&root]);
    if a != b {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"));
        std::fs::write(dir.join("r9-native.typ"), &a).unwrap();
        std::fs::write(dir.join("r9-request.typ"), &b).unwrap();
        panic!("request .typ differs from native; see target/tmp/r9-native.typ and r9-request.typ");
    }
    // Not vacuous: the title headings and the part markers are in it.
    for expected in [
        "= Preface",
        "= Title Only Chapter",
        "= Meta Title",
        "#part[",
        "appendices.with",
    ] {
        assert!(a.contains(expected), "native .typ lacks {expected}");
    }
}

// ---------------------------------------------------------------------
// Request equals native, EPUB
// ---------------------------------------------------------------------

fn epub_text_entries(bytes: &[u8]) -> std::collections::BTreeMap<String, String> {
    use std::io::Read;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let uuid = regex::Regex::new(
        r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}",
    )
    .unwrap();
    let stamp = regex::Regex::new(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z").unwrap();
    let names: Vec<String> = zip.file_names().map(String::from).collect();
    let mut out = std::collections::BTreeMap::new();
    for n in names {
        let textual = [".xhtml", ".opf", ".ncx"].iter().any(|e| n.ends_with(e));
        if !textual {
            out.insert(n, String::new());
            continue;
        }
        let mut s = String::new();
        zip.by_name(&n).unwrap().read_to_string(&mut s).unwrap();
        let s = uuid.replace_all(&s, "<UUID>");
        let s = stamp.replace_all(&s, "<TIMESTAMP>");
        out.insert(n, s.into_owned());
    }
    out
}

#[test]
fn whole_book_epub_request_equals_native() {
    let (_g, root) = scratch();
    parity_book(&root);
    unsafe { std::env::set_var("SOURCE_DATE_EPOCH", "1700000000") };

    // Native.
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(&root, runtime.as_ref()).unwrap();
    let project_type = project_type_for(&project);
    let options = RenderToFileOptions {
        execution_policy: ExecutionPolicy::None,
        ..Default::default()
    };
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        Format::from_format_string("epub").unwrap(),
        "epub",
        &options,
        runtime,
    );
    let summary = pollster::block_on(pipeline.run_with_book_support()).expect("native epub");
    assert_eq!(summary.outputs.len(), 1, "{summary:?}");
    let native = epub_text_entries(&std::fs::read(&summary.outputs[0].output_path).unwrap());

    // Request, replayed.
    let out = render(&root.join("one.qmd"), "epub", BookScope::Auto);
    assert_eq!(out.book.as_ref().unwrap().scope, ResolvedBookScope::Book);
    let request = request_of(out);
    assert!(
        request
            .resource_refs
            .iter()
            .any(|f| f.path.ends_with("/cover.png")),
        "the cover is in resource_refs"
    );
    let mut request = request;
    reroot(&mut request, &root.join("share"));
    let pandoc = NativeRuntime::new()
        .find_binary("pandoc", "QUARTO_PANDOC")
        .expect("pandoc");
    PandocWriteStage::new()
        .execute(&request, &pandoc)
        .expect("pandoc runs the request");
    let from_request = epub_text_entries(&std::fs::read(&request.output_path).unwrap());

    assert_eq!(
        native.keys().collect::<Vec<_>>(),
        from_request.keys().collect::<Vec<_>>(),
        "same entries"
    );
    for (name, text) in &native {
        let other = &from_request[name];
        if text != other {
            let dir = Path::new(env!("CARGO_TARGET_TMPDIR"));
            std::fs::write(dir.join("r9-native.epub-entry"), text).unwrap();
            std::fs::write(dir.join("r9-request.epub-entry"), other).unwrap();
            panic!("EPUB entry {name} differs; see target/tmp/r9-*.epub-entry");
        }
    }
    let nav = native
        .iter()
        .find(|(n, _)| n.ends_with("nav.xhtml"))
        .map(|(_, t)| t.clone())
        .unwrap_or_default();
    assert!(
        native.iter().any(|(_, t)| t.contains("Title Only Chapter")),
        "the title headings are in the EPUB (not vacuous): {nav}"
    );
}

// ---------------------------------------------------------------------
// Scope
// ---------------------------------------------------------------------

fn small_book(root: &Path) {
    write(
        root,
        "_quarto.yml",
        b"project:\n  type: book\nbook:\n  title: B\n  chapters:\n    - index.qmd\n    - one.qmd\n    - two.qmd\n",
    );
    write(root, "index.qmd", b"# Preface\n\nHello\n");
    write(root, "one.qmd", b"# One\n\nFirst chapter.\n");
    write(root, "two.qmd", b"# Two\n\nSecond chapter.\n");
}

#[test]
fn auto_scope_on_a_chapter_is_the_whole_book_and_reports_it() {
    let (_g, root) = scratch();
    small_book(&root);
    for format in ["typst", "typst-pdf", "epub"] {
        let out = render(&root.join("one.qmd"), format, BookScope::Auto);
        let book = out.book.as_ref().expect("book info");
        assert_eq!(book.scope, ResolvedBookScope::Book, "{format}");
        assert_eq!(book.chapters, 3, "{format}");
        let request = request_of(out);
        let text = plain_text(&request);
        for chapter in ["Hello", "First chapter", "Second chapter"] {
            assert!(text.contains(chapter), "{format}: {chapter}");
        }
    }
}

#[test]
fn chapter_scope_is_the_active_chapter_alone() {
    let (_g, root) = scratch();
    small_book(&root);
    let out = render(&root.join("one.qmd"), "typst", BookScope::Chapter);
    let book = out.book.as_ref().expect("book info");
    assert_eq!(book.scope, ResolvedBookScope::Chapter);
    assert_eq!(book.chapters, 3);
    let text = plain_text(&request_of(out));
    assert!(text.contains("First chapter"));
    assert!(!text.contains("Second chapter"));
    assert!(!text.contains("Hello"));
}

#[test]
fn docx_and_pptx_on_a_chapter_stay_chapter_only_without_a_warning() {
    let (_g, root) = scratch();
    small_book(&root);
    for format in ["docx", "pptx"] {
        let out = render(&root.join("one.qmd"), format, BookScope::Auto);
        assert!(
            out.diagnostics
                .iter()
                .all(|d| d.code.as_deref() != Some("Q-5-33")),
            "{format}: {:?}",
            out.diagnostics.iter().map(|d| &d.title).collect::<Vec<_>>()
        );
        assert_eq!(
            out.book.as_ref().unwrap().scope,
            ResolvedBookScope::Chapter,
            "{format}"
        );
        let text = plain_text(&request_of(out));
        assert!(text.contains("First chapter") && !text.contains("Second chapter"));
    }
}

#[test]
fn the_same_book_requested_from_two_chapters_is_one_request() {
    let (_g, root) = scratch();
    small_book(&root);
    let a = request_of(render(&root.join("one.qmd"), "typst", BookScope::Auto));
    let b = request_of(render(&root.join("two.qmd"), "typst", BookScope::Auto));
    assert_eq!(
        a.job_id, b.job_id,
        "the active chapter is not in the request"
    );
    assert_eq!(a.argv, b.argv);
}

#[test]
fn a_single_document_has_no_book_info() {
    let (_g, root) = scratch();
    write(&root, "doc.qmd", b"# Hi\n");
    let out = render(&root.join("doc.qmd"), "typst", BookScope::Auto);
    assert!(out.book.is_none());
}

// ---------------------------------------------------------------------
// Pages that are not chapters (D-3), and chapter hrefs that need normalizing
// ---------------------------------------------------------------------

#[test]
fn a_page_in_the_project_but_not_in_the_book_renders_alone() {
    let (_g, root) = scratch();
    small_book(&root);
    write(&root, "notes.qmd", b"# Notes\n\nA page outside the book.\n");
    for format in ["typst", "typst-pdf", "epub", "docx"] {
        let out = render(&root.join("notes.qmd"), format, BookScope::Auto);
        let book = out.book.as_ref().map(|b| (b.scope, b.chapters));
        assert_eq!(book, Some((ResolvedBookScope::Chapter, 3)), "{format}");
        assert!(
            out.diagnostics.iter().all(|d| !d.title.contains("book")),
            "{format}: no mention of the book: {:?}",
            out.diagnostics.iter().map(|d| &d.title).collect::<Vec<_>>()
        );
        let request = request_of(out);
        let text = plain_text(&request);
        assert!(text.contains("A page outside the book"), "{format}");
        assert!(!text.contains("First chapter"), "{format}");
    }
}

#[test]
fn a_page_excluded_from_the_render_list_renders_alone() {
    let (_g, root) = scratch();
    write(
        &root,
        "_quarto.yml",
        b"project:\n  type: book\n  render:\n    - index.qmd\n    - one.qmd\n    - two.qmd\nbook:\n  title: B\n  chapters:\n    - index.qmd\n    - one.qmd\n    - two.qmd\n",
    );
    write(&root, "index.qmd", b"# Preface\n\nHello\n");
    write(&root, "one.qmd", b"# One\n\nFirst chapter.\n");
    write(&root, "two.qmd", b"# Two\n\nSecond chapter.\n");
    // Not in `project.render`, not a chapter.
    write(
        &root,
        "scratch.qmd",
        b"# Scratch\n\nOutside the render list.\n",
    );
    // A file under a directory that is not part of the project's inputs.
    write(&root, "drafts/idea.qmd", b"# Idea\n\nA draft.\n");
    for rel in ["scratch.qmd", "drafts/idea.qmd"] {
        for format in ["typst", "epub"] {
            let out = render(&root.join(rel), format, BookScope::Auto);
            let book = out.book.as_ref().map(|b| b.scope);
            assert_eq!(book, Some(ResolvedBookScope::Chapter), "{rel} {format}");
            assert!(
                out.diagnostics.iter().all(|d| !d.title.contains("book")),
                "{rel} {format}: {:?}",
                out.diagnostics.iter().map(|d| &d.title).collect::<Vec<_>>()
            );
            let request = request_of(out);
            let text = plain_text(&request);
            assert!(
                text.contains("Outside the render list") || text.contains("A draft"),
                "{rel} {format}"
            );
            assert!(!text.contains("First chapter"), "{rel} {format}");
        }
    }
}

#[test]
fn a_book_below_the_vfs_root_keys_its_captures_from_that_root() {
    // The hub's VFS root is the runtime's working directory (`scratch()` sets
    // it); `_quarto.yml` lives one level below it, so sidecar keys carry the
    // `book/` prefix.
    let (_g, root) = scratch();
    let book = root.join("book");
    let (_one, two, _three) = capture_book(&book);
    // `capture_book` wrote under `book/`; the cwd stays `root`.
    let blob = gz(&[capture_for(&two, "SAME", "OUT_TWO", Vec::new())]);
    let wrong = gz(&[capture_for(&two, "SAME", "OUT_WRONG", Vec::new())]);
    let out = render_with_captures(
        &book.join("one.qmd"),
        "typst",
        BookScope::Auto,
        // Keyed from the project directory, not the VFS root: not a chapter.
        &[("book/two.qmd", blob), ("two.qmd", wrong)],
        Vec::new(),
    );
    assert!(out.error.is_none(), "{:?}", out.error);
    let text = input_json(out.request.as_ref().unwrap());
    assert!(text.contains("OUT_TWO"), "keyed from the VFS root");
    assert!(
        !text.contains("OUT_WRONG"),
        "keyed from the project dir is ignored"
    );
}

#[test]
fn a_chapter_href_that_needs_normalizing_is_still_found() {
    let (_g, root) = scratch();
    write(
        &root,
        "_quarto.yml",
        b"project:\n  type: book\nbook:\n  title: B\n  chapters:\n    - index.qmd\n    - ./sub/../one.qmd\n",
    );
    write(&root, "index.qmd", b"# Preface\n\nHello\n");
    write(&root, "one.qmd", b"# One\n\nFirst chapter.\n");
    // `sub/` must exist for `./sub/../one.qmd` to resolve on disk.
    write(&root, "sub/placeholder.txt", b"x");
    // Alone (it failed with "produced no output for the active page") ...
    let alone = request_of(render(&root.join("one.qmd"), "typst", BookScope::Chapter));
    assert!(plain_text(&alone).contains("First chapter"));
    // ... and as the whole book.
    let out = render(&root.join("one.qmd"), "typst", BookScope::Auto);
    assert_eq!(out.book.as_ref().unwrap().scope, ResolvedBookScope::Book);
    assert!(plain_text(&request_of(out)).contains("Hello"));
}

// ---------------------------------------------------------------------
// A broken chapter fails the book
// ---------------------------------------------------------------------

#[test]
fn a_chapter_whose_stages_fail_fails_the_whole_render_naming_its_file() {
    let (_g, root) = scratch();
    small_book(&root);
    write(
        &root,
        "two.qmd",
        b"# Two\n\n{{< include missing-file.qmd >}}\n",
    );
    let out = render(&root.join("one.qmd"), "typst", BookScope::Auto);
    assert!(
        out.request.is_none(),
        "no request for a book with a broken chapter"
    );
    let error = out.error.as_deref().unwrap_or_default();
    let book = out.book.as_ref().expect("book info");
    assert_eq!(book.scope, ResolvedBookScope::Book);
    // Either the stages returned `Err` (named in the message, diagnostics
    // against the chapter) or the chapter finished with an error diagnostic
    // (located in the chapter's own list): both name the file.
    let located = book.chapter_diagnostics.iter().any(|c| {
        c.file == "two.qmd"
            && c.diagnostics
                .iter()
                .any(|d| d.kind == quarto_error_reporting::DiagnosticKind::Error)
    });
    assert!(
        error.contains("two.qmd") || located,
        "{error} / {:?}",
        book.chapter_diagnostics.len()
    );
}

#[test]
fn a_chapter_with_broken_front_matter_fails_before_any_chapter_renders() {
    let (_g, root) = scratch();
    small_book(&root);
    write(&root, "two.qmd", b"---\ntitle: [unclosed\n---\n\nBroken.\n");
    let out = render(&root.join("one.qmd"), "typst", BookScope::Auto);
    assert!(out.request.is_none());
    let error = out.error.as_deref().unwrap_or_default();
    assert!(error.to_lowercase().contains("front"), "{error}");
}

#[test]
fn a_warning_in_a_later_chapter_is_reported_against_that_chapter() {
    let (_g, root) = scratch();
    small_book(&root);
    // An unknown shortcode warns during the chapter's own pause.
    write(
        &root,
        "two.qmd",
        b"# Two\n\nSecond chapter {{< nosuchshortcode >}}.\n",
    );
    let out = render(&root.join("one.qmd"), "typst", BookScope::Auto);
    assert!(out.error.is_none(), "{:?}", out.error);
    let book = out.book.as_ref().unwrap();
    let chapter = book
        .chapter_diagnostics
        .iter()
        .find(|c| c.file == "two.qmd")
        .expect("a warning against two.qmd");
    assert!(
        chapter
            .diagnostics
            .iter()
            .any(|d| d.title.to_lowercase().contains("shortcode"))
    );
    assert!(
        book.chapter_diagnostics.iter().all(|c| c.file != "one.qmd"),
        "not against chapter one"
    );
}

// ---------------------------------------------------------------------
// Progress and cancel
// ---------------------------------------------------------------------

struct Hooks {
    calls: std::sync::Mutex<Vec<(usize, usize, String)>>,
    cancel_at: Option<usize>,
}

#[async_trait::async_trait(?Send)]
impl quarto_core::project::book::BookRenderHooks for Hooks {
    async fn before_chapter(
        &self,
        index: usize,
        total: usize,
        file: &str,
    ) -> Result<(), quarto_core::project::book::Cancelled> {
        self.calls
            .lock()
            .unwrap()
            .push((index, total, file.to_string()));
        if self.cancel_at == Some(index) {
            Err(quarto_core::project::book::Cancelled)
        } else {
            Ok(())
        }
    }
}

fn render_with_hooks(path: &Path, hooks: &Hooks) -> PandocRequestOutcome {
    let runtime = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(path, runtime.as_ref()).unwrap();
    let content = std::fs::read(path).unwrap();
    let mut request_input = input(
        runtime.as_ref(),
        path,
        &content,
        "typst",
        &project,
        BookScope::Auto,
    );
    request_input.hooks = Some(hooks);
    pollster::block_on(render_pandoc_request(request_input, runtime))
}

#[test]
fn progress_reports_each_file_bearing_chapter_in_book_order() {
    let (_g, root) = scratch();
    write(
        &root,
        "_quarto.yml",
        b"project:\n  type: book\nbook:\n  title: B\n  chapters:\n    - index.qmd\n    - part: P\n      chapters:\n        - one.qmd\n        - two.qmd\n",
    );
    write(&root, "index.qmd", b"# Preface\n\nHello\n");
    write(&root, "one.qmd", b"# One\n");
    write(&root, "two.qmd", b"# Two\n");
    let hooks = Hooks {
        calls: Default::default(),
        cancel_at: None,
    };
    let out = render_with_hooks(&root.join("one.qmd"), &hooks);
    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(
        hooks.calls.lock().unwrap().clone(),
        vec![
            (1, 3, "index.qmd".to_string()),
            (2, 3, "one.qmd".to_string()),
            (3, 3, "two.qmd".to_string()),
        ],
        "the part divider is skipped; no calls for the tail"
    );
}

#[test]
fn cancel_stops_the_render_with_no_request() {
    let (_g, root) = scratch();
    small_book(&root);
    let hooks = Hooks {
        calls: Default::default(),
        cancel_at: Some(2),
    };
    let out = render_with_hooks(&root.join("one.qmd"), &hooks);
    assert!(out.request.is_none());
    assert_eq!(out.error.as_deref(), Some("cancelled"));
    assert_eq!(
        hooks.calls.lock().unwrap().len(),
        2,
        "never reached chapter 3"
    );
}

// ---------------------------------------------------------------------
// Captures per chapter (D-6)
// ---------------------------------------------------------------------

use quarto_trace::{CaptureFile, EngineCapture};

fn gz(captures: &[EngineCapture]) -> Vec<u8> {
    use std::io::Write;
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(&serde_json::to_vec(captures).unwrap())
        .unwrap();
    enc.finish().unwrap()
}

/// `qmd` holds one `{r}` cell with body `cell`; a capture whose output is
/// `out` instead of the cell.
fn capture_for(qmd: &str, cell: &str, out: &str, files: Vec<CaptureFile>) -> EngineCapture {
    let src = format!("```{{r}}\n{cell}\n```");
    let result =
        format!("::: {{.cell}}\n::: {{.cell-output .cell-output-stdout}}\n{out}\n:::\n:::");
    EngineCapture {
        engine_name: "r".to_string(),
        input_qmd: qmd.to_string(),
        result: serde_json::json!({ "markdown": qmd.replace(&src, &result) }),
        files,
    }
}

fn capture_book(root: &Path) -> (String, String, String) {
    write(
        root,
        "_quarto.yml",
        b"project:\n  type: book\nbook:\n  title: B\n  chapters:\n    - index.qmd\n    - one.qmd\n    - two.qmd\n    - sub/three.qmd\n",
    );
    write(root, "index.qmd", b"# Preface\n\nHello\n");
    let one = "# One\n\nBefore.\n\n```{r}\nSAME\n```\n".to_string();
    let two = "# Two\n\nBefore.\n\n```{r}\nSAME\n```\n".to_string();
    let three = "# Three\n\nBefore.\n\n```{r}\nTHREE_SRC\n```\n".to_string();
    write(root, "one.qmd", one.as_bytes());
    write(root, "two.qmd", two.as_bytes());
    write(root, "sub/three.qmd", three.as_bytes());
    (one, two, three)
}

fn render_with_captures(
    path: &Path,
    format: &str,
    scope: BookScope,
    by_path: &[(&str, Vec<u8>)],
    active_blob: Vec<EngineCapture>,
) -> PandocRequestOutcome {
    let runtime = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(path, runtime.as_ref()).unwrap();
    let content = std::fs::read(path).unwrap();
    let mut request_input = input(runtime.as_ref(), path, &content, format, &project, scope);
    request_input.captures_by_path = by_path
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect();
    request_input.captures = active_blob;
    pollster::block_on(render_pandoc_request(request_input, runtime))
}

#[test]
fn a_chapters_capture_shows_there_and_never_in_an_identical_cell_elsewhere() {
    let (_g, root) = scratch();
    let (_one, two, _three) = capture_book(&root);
    let blob = gz(&[capture_for(&two, "SAME", "OUT_TWO", Vec::new())]);
    // Active chapter one has no capture; chapter two's shows in its place.
    let out = render_with_captures(
        &root.join("one.qmd"),
        "typst",
        BookScope::Auto,
        &[("two.qmd", blob)],
        Vec::new(),
    );
    assert!(out.error.is_none(), "{:?}", out.error);
    let request = out.request.as_ref().unwrap();
    let text = input_json(request);
    assert!(text.contains("OUT_TWO"), "chapter 2's recorded output");
    assert_eq!(
        text.matches("SAME").count(),
        1,
        "chapter 1's identical cell stays source, chapter 2's is replaced"
    );
    // One + chapter three have no result: 2 unexecuted; chapter two's is served.
    assert_eq!(out.unexecuted_cells, 2);
}

#[test]
fn engine_includes_in_a_capture_count_only_for_the_first_chapter() {
    // The merged document's metadata is seeded from the first chapter, so a
    // later chapter's engine includes never reach the request (typst and
    // epub take none from a header include in any case): this pins both.
    let (_g, root) = scratch();
    write(
        &root,
        "_quarto.yml",
        b"project:\n  type: book\nbook:\n  title: B\n  chapters:\n    - index.qmd\n    - two.qmd\n",
    );
    let index = "# Preface\n\n```{r}\nIDX\n```\n".to_string();
    let two = "# Two\n\n```{r}\nTWO\n```\n".to_string();
    write(&root, "index.qmd", index.as_bytes());
    write(&root, "two.qmd", two.as_bytes());
    let with_includes = |qmd: &str, cell: &str, marker: &str| {
        let mut capture = capture_for(qmd, cell, "OUT", Vec::new());
        capture.result["includes"] = serde_json::json!({
            "header_includes": [marker],
            "include_before": [],
            "include_after": []
        });
        gz(&[capture])
    };
    let later = render_with_captures(
        &root.join("index.qmd"),
        "typst",
        BookScope::Auto,
        &[("two.qmd", with_includes(&two, "TWO", "LATER_HEADER"))],
        Vec::new(),
    );
    let later_text = input_json(later.request.as_ref().expect("request"));
    assert!(
        !later_text.contains("LATER_HEADER"),
        "chapter 2's include leaked"
    );
    let first = render_with_captures(
        &root.join("index.qmd"),
        "typst",
        BookScope::Auto,
        &[("index.qmd", with_includes(&index, "IDX", "FIRST_HEADER"))],
        Vec::new(),
    );
    let first_text = input_json(first.request.as_ref().expect("request"));
    assert!(
        first_text.contains("FIRST_HEADER"),
        "chapter 1's include is seeded"
    );
}

#[test]
fn a_captured_figure_in_a_subdirectory_chapter_mounts_under_its_own_directory() {
    let (_g, root) = scratch();
    let (_one, _two, three) = capture_book(&root);
    let figure = CaptureFile {
        path: "doc_files/fig.png".to_string(),
        contents_base64: base64_png(),
    };
    let cap = {
        let mut c = capture_for(&three, "THREE_SRC", "OUT_THREE", vec![figure]);
        c.result = serde_json::json!({ "markdown": three.replace(
            "```{r}\nTHREE_SRC\n```",
            "::: {.cell}\n::: {.cell-output-display}\n![](doc_files/fig.png)\n:::\n:::"
        ) });
        c
    };
    let out = render_with_captures(
        &root.join("one.qmd"),
        "typst-pdf",
        BookScope::Auto,
        &[("sub/three.qmd", gz(&[cap]))],
        Vec::new(),
    );
    assert!(out.error.is_none(), "{:?}", out.error);
    let request = out.request.as_ref().unwrap();
    let refs: Vec<&str> = request
        .resource_refs
        .iter()
        .map(|f| f.path.as_str())
        .collect();
    let root_str = quarto_core::pandoc_request::normalize_request_path(&root);
    assert!(
        refs.contains(&format!("{root_str}/sub/doc_files/fig.png").as_str()),
        "{refs:?}"
    );
    assert!(
        !refs
            .iter()
            .any(|p| p == &format!("{root_str}/doc_files/fig.png")),
        "not under the root: {refs:?}"
    );
}

fn base64_png() -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(ONE_PIXEL_PNG)
}

#[test]
fn chapter_scope_prefers_the_given_blob_and_falls_back_to_the_map() {
    let (_g, root) = scratch();
    let (one, two, _three) = capture_book(&root);
    let map_one = gz(&[capture_for(&one, "SAME", "MAP_ONE", Vec::new())]);
    // Fallback: no blob given, the map's entry for the active file is used.
    let out = render_with_captures(
        &root.join("one.qmd"),
        "typst",
        BookScope::Chapter,
        &[
            ("one.qmd", map_one.clone()),
            (
                "two.qmd",
                gz(&[capture_for(&two, "SAME", "MAP_TWO", Vec::new())]),
            ),
        ],
        Vec::new(),
    );
    let text = input_json(out.request.as_ref().unwrap());
    assert!(
        text.contains("MAP_ONE") && !text.contains("MAP_TWO"),
        "{text}"
    );
    // A given blob wins over the map.
    let out = render_with_captures(
        &root.join("one.qmd"),
        "typst",
        BookScope::Chapter,
        &[("one.qmd", map_one)],
        vec![capture_for(&one, "SAME", "BLOB_ONE", Vec::new())],
    );
    let text = input_json(out.request.as_ref().unwrap());
    assert!(
        text.contains("BLOB_ONE") && !text.contains("MAP_ONE"),
        "{text}"
    );
}

#[test]
fn a_book_request_ignores_the_active_files_own_blob() {
    let (_g, root) = scratch();
    let (one, two, _three) = capture_book(&root);
    let by_path = [(
        "two.qmd",
        gz(&[capture_for(&two, "SAME", "OUT_TWO", Vec::new())]),
    )];
    let plain = request_of(render_with_captures(
        &root.join("one.qmd"),
        "typst",
        BookScope::Auto,
        &by_path,
        Vec::new(),
    ));
    let with_blob = request_of(render_with_captures(
        &root.join("one.qmd"),
        "typst",
        BookScope::Auto,
        &by_path,
        vec![capture_for(&one, "SAME", "DIFFERENT", Vec::new())],
    ));
    assert_eq!(plain.job_id, with_blob.job_id);
    assert!(!input_json(&with_blob).contains("DIFFERENT"));
    // And from the other chapter: the same request.
    let from_two = request_of(render_with_captures(
        &root.join("two.qmd"),
        "typst",
        BookScope::Auto,
        &by_path,
        Vec::new(),
    ));
    assert_eq!(plain.job_id, from_two.job_id);
}

#[test]
fn a_stale_capture_splices_what_still_matches() {
    let (_g, root) = scratch();
    let (_one, two, _three) = capture_book(&root);
    // Recorded against an older source of chapter two (different cell body).
    let old = two.replace("SAME", "OLD");
    let stale = gz(&[capture_for(&old, "OLD", "STALE_OUT", Vec::new())]);
    let out = render_with_captures(
        &root.join("one.qmd"),
        "typst",
        BookScope::Auto,
        &[("two.qmd", stale)],
        Vec::new(),
    );
    assert!(out.error.is_none(), "{:?}", out.error);
    let text = input_json(out.request.as_ref().unwrap());
    assert!(
        !text.contains("STALE_OUT"),
        "a cell that changed stays source"
    );
    assert_eq!(text.matches("SAME").count(), 2);
}

#[test]
fn a_path_key_that_is_not_a_chapter_is_ignored_and_a_corrupt_one_degrades_that_chapter() {
    let (_g, root) = scratch();
    capture_book(&root);
    let out = render_with_captures(
        &root.join("one.qmd"),
        "typst",
        BookScope::Auto,
        &[
            ("nowhere.qmd", gz(&[])),
            ("two.qmd", b"not gzip at all".to_vec()),
        ],
        Vec::new(),
    );
    assert!(
        out.error.is_none(),
        "the book still builds: {:?}",
        out.error
    );
    let book = out.book.as_ref().unwrap();
    let chapter = book
        .chapter_diagnostics
        .iter()
        .find(|c| c.file == "two.qmd")
        .expect("a diagnostic naming chapter two");
    assert!(
        chapter
            .diagnostics
            .iter()
            .any(|d| d.title.contains("two.qmd")),
        "{:?}",
        chapter
            .diagnostics
            .iter()
            .map(|d| &d.title)
            .collect::<Vec<_>>()
    );
    assert!(input_json(out.request.as_ref().unwrap()).contains("SAME"));
}

// ---------------------------------------------------------------------
// Chapter-relative resources, through the request
// ---------------------------------------------------------------------

#[test]
fn pdf_request_mounts_each_chapters_images_under_its_own_directory() {
    let (_g, root) = scratch();
    parity_book(&root);
    let out = render(&root.join("one.qmd"), "typst-pdf", BookScope::Auto);
    assert!(
        out.diagnostics
            .iter()
            .all(|d| d.code.as_deref() != Some("Q-11-1")),
        "{:?}",
        out.diagnostics.iter().map(|d| &d.title).collect::<Vec<_>>()
    );
    let request = request_of(out);
    let root_str = quarto_core::pandoc_request::normalize_request_path(&root);
    let refs: Vec<&str> = request
        .resource_refs
        .iter()
        .map(|f| f.path.as_str())
        .collect();
    for rel in ["rootlocal.png", "sub/local.png", "top.png", "img/a.png"] {
        assert!(
            refs.contains(&format!("{root_str}/{rel}").as_str()),
            "{rel} in {refs:?}"
        );
    }
}

// ---------------------------------------------------------------------
// The golden `pandoc-input.json`
// ---------------------------------------------------------------------

/// The built-in extension's template partials are named in the metadata by
/// absolute path, which differs by target: native extracts the subtree to a
/// temp directory (`.../orange-book/<file>`), the browser serves it under
/// `/__quarto_resources__/extension-subtrees/orange-book/_extensions/orange-book/`.
/// Both become `<EXT>/<file>`; the vitest applies the same normalization.
fn normalize_extension_paths(value: &mut serde_json::Value) {
    let re = regex::Regex::new(r"^/.*?/orange-book/(?:_extensions/orange-book/)?").unwrap();
    match value {
        serde_json::Value::String(text)
            if text.starts_with('/') && text.contains("/orange-book/") =>
        {
            *text = re.replace(text, "<EXT>/").into_owned();
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(normalize_extension_paths),
        serde_json::Value::Object(map) => map.values_mut().for_each(normalize_extension_paths),
        _ => {}
    }
}

/// Drops what is not part of the request's meaning and differs by target or
/// by day: every `s` (an index into the dropped `astContext` source table; it
/// shifts when any entry before it comes or goes) and the `listing-item`'s
/// `date-modified`, which native reads from the file's mtime (the browser's
/// VFS has none, and the golden would change every day). The vitest applies
/// the same normalization.
fn strip_unstable(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Array(items) => items.iter_mut().for_each(strip_unstable),
        serde_json::Value::Object(map) => {
            map.remove("s");
            map.remove("date-modified");
            map.values_mut().for_each(strip_unstable);
        }
        _ => {}
    }
}

/// The parity book's `pandoc-input.json` is recorded as a golden; the wasm
/// vitest asserts the wasm-built request equals it (after the same
/// `astContext` normalization `comparable()` applies there). Valid only
/// because the file holds no absolute path, which is asserted. Re-record with
/// `BOOK_REQUEST_GOLDEN=record`.
#[test]
fn the_parity_books_input_json_matches_its_golden_and_holds_no_absolute_path() {
    let (_g, root) = scratch();
    parity_book(&root);
    let out = render(&root.join("one.qmd"), "typst", BookScope::Auto);
    let request = request_of(out);
    let mut json: serde_json::Value = serde_json::from_str(&input_json(&request)).unwrap();
    json.as_object_mut().unwrap().remove("astContext");
    normalize_extension_paths(&mut json);
    strip_unstable(&mut json);
    let text = serde_json::to_string_pretty(&json).unwrap() + "\n";
    let root_str = root.to_string_lossy().replace('\\', "/");
    assert!(
        !text.contains(&root_str),
        "the project path leaks into the input JSON"
    );
    assert!(!text.contains(&constants().share_root));
    assert!(
        !text.contains("/var/folders") && !text.contains("/Users/") && !text.contains("/tmp/"),
        "an absolute path is left in the input JSON"
    );
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/integration/pandoc_request_books/pandoc-input.golden.json");
    if std::env::var("BOOK_REQUEST_GOLDEN").as_deref() == Ok("record") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &text).unwrap();
    }
    let golden = std::fs::read_to_string(&path)
        .expect("golden recorded with BOOK_REQUEST_GOLDEN=record")
        .replace("\r\n", "\n");
    assert_eq!(text, golden);
}

// ---------------------------------------------------------------------
// The resolver's `book` field (task 8)
// ---------------------------------------------------------------------

fn resolved_book(path: &Path) -> Option<(Vec<String>, bool)> {
    let runtime = NativeRuntime::new();
    quarto_core::pandoc_request::formats::resolve_document_formats(path, &runtime)
        .unwrap()
        .book
        .map(|b| (b.chapters, b.chapter))
}

#[test]
fn the_resolver_reports_a_book_chapter_with_the_chapter_keys_in_book_order() {
    let (_g, root) = scratch();
    parity_book(&root);
    let (chapters, chapter) = resolved_book(&root.join("sub/two.qmd")).expect("a book");
    assert!(chapter);
    // Keys are relative to the runtime's cwd (the VFS project root here): the
    // book's files in render order, `index.qmd` first, the part page and the
    // appendix included, dividers not.
    assert_eq!(
        chapters,
        [
            "index.qmd",
            "one.qmd",
            "sub/two.qmd",
            "partpage.qmd",
            "titleonly.qmd",
            "withh1.qmd",
            "neither.qmd",
            "textfirst.qmd",
            "app.qmd"
        ]
    );
}

#[test]
fn the_resolver_says_a_non_chapter_page_is_not_a_chapter_and_other_projects_have_no_book() {
    let (_g, root) = scratch();
    small_book(&root);
    write(&root, "notes.qmd", b"# Notes\n");
    let (chapters, chapter) = resolved_book(&root.join("notes.qmd")).expect("a book");
    assert!(!chapter);
    assert_eq!(chapters, ["index.qmd", "one.qmd", "two.qmd"]);

    // A website page and a single file: `null`.
    let (_g2, web) = scratch();
    write(&web, "_quarto.yml", b"project:\n  type: website\n");
    write(&web, "a.qmd", b"# A\n");
    assert_eq!(resolved_book(&web.join("a.qmd")), None);
    let (_g3, single) = scratch();
    write(&single, "a.qmd", b"# A\n");
    assert_eq!(resolved_book(&single.join("a.qmd")), None);
}

#[test]
fn the_resolver_and_the_exports_auto_scope_agree_for_every_file_of_the_book() {
    let (_g, root) = scratch();
    parity_book(&root);
    write(&root, "notes.qmd", b"# Notes\n\nOutside.\n");
    let mut files: Vec<String> = resolved_book(&root.join("one.qmd")).unwrap().0;
    files.push("notes.qmd".to_string());
    for file in files {
        let (_, says_chapter) = resolved_book(&root.join(&file)).unwrap();
        let out = render(&root.join(&file), "typst", BookScope::Auto);
        let scope = out.book.as_ref().expect("book info").scope;
        assert_eq!(
            scope == ResolvedBookScope::Book,
            says_chapter,
            "{file}: resolver says chapter={says_chapter}, export says {scope:?}"
        );
    }
}

#[test]
fn a_book_listing_a_missing_chapter_degrades_in_the_resolver_and_fails_the_export() {
    let (_g, root) = scratch();
    write(
        &root,
        "_quarto.yml",
        b"project:\n  type: book\nbook:\n  title: B\n  chapters:\n    - index.qmd\n    - missing.qmd\n",
    );
    write(&root, "index.qmd", b"# Preface\n");
    let (chapters, chapter) = resolved_book(&root.join("index.qmd")).expect("a book");
    assert!(chapters.is_empty() && !chapter);
    let out = render(&root.join("index.qmd"), "typst", BookScope::Auto);
    assert!(out.request.is_none());
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some("Q-5-35"))
            || out.error.as_deref().unwrap_or_default().contains("Q-5-35"),
        "{:?} {:?}",
        out.error,
        out.diagnostics.iter().map(|d| &d.title).collect::<Vec<_>>()
    );
}
