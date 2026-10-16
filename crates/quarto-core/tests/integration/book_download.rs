/*
 * tests/integration/book_download.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * q2-preview hub-parity Plan 10: `render_book_download`, the whole-book
 * entry behind the preview server's "Download book as X". Tests needing
 * `typst` or `pandoc` skip when absent.
 */

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use base64::Engine as _;
use quarto_core::book_download::{
    BookDownloadFailure, BookDownloadParams, BookDownloadResult, render_book_download,
};
use quarto_core::format::Format;
use quarto_core::render_to_file::TypstTarget;
use quarto_core::stage::Cancellation;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};
use quarto_trace::{CaptureFile, EngineCapture};

use tempfile::TempDir;

fn native() -> Arc<dyn SystemRuntime> {
    Arc::new(NativeRuntime::new())
}

fn have(bin: &str) -> bool {
    Command::new(bin)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

macro_rules! require {
    ($($bin:literal),+) => {
        $(if !have($bin) {
            eprintln!("SKIP: {} is required", $bin);
            return;
        })+
    };
}

fn write(path: &Path, bytes: impl AsRef<[u8]>) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// 1x1 PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0xfc, 0xcf, 0xc0, 0x50,
    0x0f, 0x00, 0x04, 0x85, 0x01, 0x80, 0x84, 0xa9, 0x8c, 0x21, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

const INDEX: &str = "---\ntitle: Home\n---\n\nWelcome to the book.\n";
const CH1: &str = "# One\n\nChapter one text.\n\n![A pixel](pixel.png)\n";
const CH2: &str = "# Two\n\nChapter two text.\n";
const CAPTURE_CH1: &str = "# One\n\nBefore.\n\n```{r}\nplot(1)\n```\n\nAfter.\n";

struct Fixture {
    _tmp: TempDir,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        Self::with_ch1(CH1)
    }

    fn with_ch1(ch1: &str) -> Self {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        write(
            &root.join("_quarto.yml"),
            "project:\n  type: book\n\nbook:\n  title: \"Small Book\"\n  author: \"A. Writer\"\n  chapters:\n    - index.qmd\n    - ch1.qmd\n    - ch2.qmd\n",
        );
        write(&root.join("index.qmd"), INDEX);
        write(&root.join("ch1.qmd"), ch1);
        write(&root.join("ch2.qmd"), CH2);
        write(&root.join("pixel.png"), PNG);
        Self { _tmp: tmp, root }
    }

    fn request_dir(&self) -> PathBuf {
        let dir = self.root.join(".quarto/preview-out/req");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}

#[allow(clippy::too_many_arguments)]
fn download(
    fx: &Fixture,
    active: &str,
    content: Option<&str>,
    format: &str,
    target: Option<TypstTarget>,
    captures: &HashMap<String, Vec<EngineCapture>>,
    cancellation: Cancellation,
) -> Result<BookDownloadResult, BookDownloadFailure> {
    let format = Format::from_format_string(format).unwrap();
    render_book_download(BookDownloadParams {
        path: &fx.root.join(active),
        content: content.map(str::to_string),
        format: &format,
        typst_target: target,
        request_dir: &fx.request_dir(),
        hub_root: &fx.root,
        captures,
        cancellation,
        child_deadline: Some(std::time::Duration::from_secs(120)),
        metadata: None,
        runtime: native(),
    })
}

fn simple(
    fx: &Fixture,
    format: &str,
    target: Option<TypstTarget>,
) -> Result<BookDownloadResult, BookDownloadFailure> {
    download(
        fx,
        "ch1.qmd",
        None,
        format,
        target,
        &HashMap::new(),
        Cancellation::new(),
    )
}

fn ok(r: Result<BookDownloadResult, BookDownloadFailure>) -> BookDownloadResult {
    r.unwrap_or_else(|e| panic!("book download failed: {e:?}"))
}

/// The user's `_book/` and the render manifests, with a planted marker each.
fn plant_user_outputs(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let planted = [
        ("_book/keep.txt", "keep"),
        (".quarto/render-manifest.json", "{\"rendered\":[]}"),
        (".quarto/llms-manifest.json", "{\"llms\":[]}"),
    ];
    planted
        .iter()
        .map(|(rel, text)| {
            write(&root.join(rel), text);
            (root.join(rel), text.as_bytes().to_vec())
        })
        .collect()
}

#[test]
fn a_typst_book_downloads_as_source() {
    require!("pandoc");
    let fx = Fixture::new();
    let planted = plant_user_outputs(&fx.root);
    let result = ok(simple(&fx, "typst", Some(TypstTarget::Source)));
    assert_eq!(result.file.extension().unwrap(), "typ");
    assert!(result.file.starts_with(fx.request_dir()));
    assert_eq!(result.chapters, 3);
    let typ = std::fs::read_to_string(&result.file).unwrap();
    for text in [
        "Welcome to the book",
        "Chapter one text",
        "Chapter two text",
    ] {
        assert!(typ.contains(text), "{text} missing from the source");
    }
    assert!(!result.file.with_extension("pdf").exists(), "no compile");
    for (path, bytes) in planted {
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "{path:?} changed");
    }
    assert_eq!(
        std::fs::read_dir(fx.root.join("_book")).unwrap().count(),
        1,
        "_book/ gained files"
    );
}

#[test]
fn request_metadata_reaches_the_book_render() {
    require!("pandoc");
    let fx = Fixture::new();
    let format = Format::from_format_string("typst").unwrap();
    let render = |metadata: Option<serde_json::Value>| {
        let result = render_book_download(BookDownloadParams {
            path: &fx.root.join("ch1.qmd"),
            content: None,
            format: &format,
            typst_target: Some(TypstTarget::Source),
            request_dir: &fx.request_dir(),
            hub_root: &fx.root,
            captures: &HashMap::new(),
            cancellation: Cancellation::new(),
            child_deadline: Some(std::time::Duration::from_secs(120)),
            metadata,
            runtime: native(),
        });
        std::fs::read_to_string(ok(result).file).unwrap()
    };
    let plain = render(None);
    assert!(plain.contains("subtitle: none"));
    assert!(!plain.contains("ZZSUB"));
    let overlaid = render(Some(serde_json::json!({"subtitle": "ZZSUB"})));
    assert!(
        overlaid.contains("subtitle: [ZZSUB]"),
        "the request's metadata is not in the book source"
    );
}

/// F8-D6: the runtime in the params carries the live preview's `--profile`
/// into the whole-book render: a profile that sets `book.title` shows in the
/// book source on the profile's runtime and on no other.
#[test]
fn the_runtime_in_the_params_carries_the_profile() {
    require!("pandoc");
    let fx = Fixture::new();
    write(
        &fx.root.join("_quarto-pt.yml"),
        "book:\n  title: \"ZZPROFILETITLE\"\n",
    );
    let format = Format::from_format_string("typst").unwrap();
    let render = |runtime: Arc<dyn SystemRuntime>| {
        let result = render_book_download(BookDownloadParams {
            path: &fx.root.join("ch1.qmd"),
            content: None,
            format: &format,
            typst_target: Some(TypstTarget::Source),
            request_dir: &fx.request_dir(),
            hub_root: &fx.root,
            captures: &HashMap::new(),
            cancellation: Cancellation::new(),
            child_deadline: Some(std::time::Duration::from_secs(120)),
            metadata: None,
            runtime,
        });
        std::fs::read_to_string(ok(result).file).unwrap()
    };
    let plain = render(native());
    assert!(plain.contains("Small Book") && !plain.contains("ZZPROFILETITLE"));
    let profiled = render(Arc::new(
        quarto_core::project::project_profile::native_runtime_for_selection(&["pt".to_string()]),
    ));
    assert!(
        profiled.contains("ZZPROFILETITLE"),
        "the profile did not reach the whole-book download"
    );
}

#[test]
fn a_typst_book_downloads_as_pdf_with_its_chapter_image() {
    require!("pandoc", "typst");
    let fx = Fixture::new();
    let planted = plant_user_outputs(&fx.root);
    let result = ok(simple(&fx, "typst", Some(TypstTarget::Pdf)));
    assert_eq!(result.file.extension().unwrap(), "pdf");
    assert!(std::fs::read(&result.file).unwrap().starts_with(b"%PDF"));
    // The image flushed into the request directory, not `_book/`.
    assert!(fx.request_dir().join("pixel.png").is_file());
    for (path, bytes) in planted {
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "{path:?} changed");
    }
    assert!(!fx.root.join("_book/pixel.png").exists());
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some("Q-21-3")),
        "typst could not find a source or image"
    );
}

#[test]
fn an_epub_book_downloads_with_its_chapter_image() {
    require!("pandoc");
    let fx = Fixture::new();
    plant_user_outputs(&fx.root);
    let result = ok(simple(&fx, "epub", None));
    assert_eq!(result.file.extension().unwrap(), "epub");
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&result.file).unwrap()).unwrap();
    let names: Vec<String> = zip.file_names().map(|n| n.unwrap().to_string()).collect();
    assert!(
        names.iter().any(|n| n.ends_with(".png")),
        "no image in the epub: {names:?}"
    );
    let mut text = String::new();
    for name in names.iter().filter(|n| n.ends_with(".xhtml")) {
        zip.by_name(name)
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
    }
    assert!(text.contains("Chapter two text"));
    assert_eq!(
        std::fs::read_dir(fx.root.join("_book")).unwrap().count(),
        1,
        "_book/ gained files"
    );
}

#[test]
fn the_active_chapter_comes_from_the_editor_and_the_others_from_disk() {
    require!("pandoc");
    let fx = Fixture::new();
    let edited = "# One\n\nEDITED unsaved sentence.\n";
    let result = ok(download(
        &fx,
        "ch1.qmd",
        Some(edited),
        "typst",
        Some(TypstTarget::Source),
        &HashMap::new(),
        Cancellation::new(),
    ));
    let typ = std::fs::read_to_string(&result.file).unwrap();
    assert!(typ.contains("EDITED unsaved sentence"));
    assert!(!typ.contains("Chapter one text"), "disk text leaked in");
    assert!(typ.contains("Chapter two text"));
    // The disk is untouched.
    assert_eq!(
        std::fs::read_to_string(fx.root.join("ch1.qmd")).unwrap(),
        CH1
    );
}

#[test]
fn an_unrecorded_chapter_counts_as_unexecuted() {
    require!("pandoc");
    let fx = Fixture::with_ch1(CAPTURE_CH1);
    let result = ok(simple(&fx, "typst", Some(TypstTarget::Source)));
    assert_eq!(result.unexecuted_cells, 1);
}

#[test]
fn a_chapters_captures_are_spliced_and_its_plot_lands_in_the_request_directory() {
    require!("pandoc");
    let fx = Fixture::with_ch1(CAPTURE_CH1);
    let plot = "ch1_files/figure-typst/cell-1.png";
    let markdown = format!(
        "# One\n\nBefore.\n\n::: {{.cell}}\n::: {{.cell-output-display}}\n![]({plot})\n:::\n:::\n\nAfter.\n"
    );
    let capture = EngineCapture {
        engine_name: "r".to_string(),
        input_qmd: CAPTURE_CH1.to_string(),
        result: serde_json::json!({ "markdown": markdown }),
        files: vec![CaptureFile {
            path: plot.to_string(),
            contents_base64: base64::engine::general_purpose::STANDARD.encode(PNG),
        }],
    };
    // Sidecar keys are hub-root-relative; the hub root is the project here.
    let captures = HashMap::from([("ch1.qmd".to_string(), vec![capture])]);
    let result = ok(download(
        &fx,
        "ch2.qmd",
        None,
        "typst",
        Some(TypstTarget::Source),
        &captures,
        Cancellation::new(),
    ));
    assert_eq!(result.unexecuted_cells, 0);
    assert_eq!(std::fs::read(fx.request_dir().join(plot)).unwrap(), PNG);
    assert!(
        !fx.root.join("ch1_files").exists(),
        "plot written beside the source"
    );
}

#[test]
fn a_cancelled_token_stops_the_download_and_a_retry_succeeds() {
    require!("pandoc");
    let fx = Fixture::new();
    let token = Cancellation::new();
    token.cancel();
    let err = download(
        &fx,
        "ch1.qmd",
        None,
        "typst",
        Some(TypstTarget::Source),
        &HashMap::new(),
        token,
    )
    .unwrap_err();
    assert!(matches!(err, BookDownloadFailure::Cancelled), "{err:?}");
    // The in-flight guard was released.
    ok(simple(&fx, "typst", Some(TypstTarget::Source)));
}

#[test]
fn a_file_outside_a_book_is_refused() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    write(&root.join("_quarto.yml"), "project:\n  title: x\n");
    write(&root.join("a.qmd"), "hello\n");
    let format = Format::from_format_string("typst").unwrap();
    let dir = root.join(".quarto/preview-out/r");
    std::fs::create_dir_all(&dir).unwrap();
    let err = render_book_download(BookDownloadParams {
        path: &root.join("a.qmd"),
        content: None,
        format: &format,
        typst_target: None,
        request_dir: &dir,
        hub_root: &root,
        captures: &HashMap::new(),
        cancellation: Cancellation::new(),
        child_deadline: None,
        metadata: None,
        runtime: native(),
    })
    .unwrap_err();
    assert!(matches!(err, BookDownloadFailure::Failed(_)), "{err:?}");
}
