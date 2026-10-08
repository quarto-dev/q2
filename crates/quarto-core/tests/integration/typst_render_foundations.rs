/*
 * tests/integration/typst_render_foundations.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * q2-preview hub-parity Plan 6: typst render foundations (`typst_target`,
 * request directory, captures). Tests needing `typst` skip when absent.
 */

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn have(bin: &str) -> bool {
    Command::new(bin)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// 6.2 characterisation: typst refuses a source outside its `--root`, so
/// an OS-temp output directory fails with `Q-21-3`. The same render into a
/// request directory succeeds (`typst_renders_into_a_request_directory_and_cleans_up`).
#[test]
fn typst_into_os_temp_dir_fails_on_root() {
    if !have("typst") || !have("pandoc") {
        eprintln!("SKIP: typst and pandoc are required");
        return;
    }
    let proj = TempDir::new().unwrap();
    let os = TempDir::new().unwrap();
    let root = proj.path().canonicalize().unwrap();
    write(&root.join("_quarto.yml"), "project:\n  title: x\n");
    write(&root.join("a.qmd"), "---\ntitle: T\n---\n\nHello\n");
    let options = RenderToFileOptions {
        output_dir: Some(os.path().join("out")),
        quiet: true,
        ..Default::default()
    };
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let err = render_to_file(&root.join("a.qmd"), "typst", &options, runtime)
        .expect_err("typst outside its root must fail today");
    assert!(err.to_string().contains("Q-21-3") || format!("{err:?}").contains("Q-21-3"));
}

use quarto_core::render_to_file::TypstTarget;

/// Render `a.qmd` (front matter `fm`) in a fresh project with `target`
/// and return the file names written to the output directory.
fn render_with_target(fm: &str, target: Option<TypstTarget>) -> Vec<String> {
    let proj = TempDir::new().unwrap();
    let root = proj.path().canonicalize().unwrap();
    write(&root.join("_quarto.yml"), "project:\n  title: x\n");
    write(
        &root.join("a.qmd"),
        &format!("---\ntitle: T\n{fm}---\n\nHello\n"),
    );
    let out = root.join(".quarto/preview-out/t");
    let options = RenderToFileOptions {
        output_dir: Some(out.clone()),
        quiet: true,
        typst_target: target,
        ..Default::default()
    };
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    render_to_file(&root.join("a.qmd"), "typst", &options, runtime)
        .unwrap_or_else(|e| panic!("render failed: {e}"));
    let mut names: Vec<String> = std::fs::read_dir(&out)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("a."))
        .collect();
    names.sort();
    names
}

macro_rules! require_typst {
    () => {
        if !have("typst") || !have("pandoc") {
            eprintln!("SKIP: typst and pandoc are required");
            return;
        }
    };
}

#[test]
fn typst_target_unset_follows_output_ext() {
    require_typst!();
    assert_eq!(render_with_target("", None), ["a.pdf"]);
    assert_eq!(render_with_target("output-ext: typ\n", None), ["a.typ"]);
}

#[test]
fn typst_target_pdf_overrides_output_ext() {
    require_typst!();
    let t = Some(TypstTarget::Pdf);
    assert_eq!(render_with_target("output-ext: typ\n", t), ["a.pdf"]);
    assert_eq!(render_with_target("output-ext: typst\n", t), ["a.pdf"]);
}

#[test]
fn typst_target_source_writes_source_and_skips_compile() {
    require_typst!();
    let t = Some(TypstTarget::Source);
    assert_eq!(render_with_target("", t), ["a.typ"]);
    assert_eq!(render_with_target("output-ext: pdf\n", t), ["a.typ"]);
    assert_eq!(render_with_target("output-ext: typ\n", t), ["a.typ"]);
    assert_eq!(render_with_target("output-ext: typst\n", t), ["a.typst"]);
}

// ---- capture plots (6.4, E10) -------------------------------------------

use base64::Engine as _;
use quarto_trace::{CaptureFile, EngineCapture};

/// 1x1 PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0xfc, 0xcf, 0xc0, 0x50,
    0x0f, 0x00, 0x04, 0x85, 0x01, 0x80, 0x84, 0xa9, 0x8c, 0x21, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

const CAPTURE_DOC: &str = "---\ntitle: T\n---\n\nBefore.\n\n```{r}\nplot(1)\n```\n\nAfter.\n";

fn plot_capture(plot_path: &str) -> EngineCapture {
    let markdown = format!(
        "---\ntitle: T\n---\n\nBefore.\n\n::: {{.cell}}\n::: {{.cell-output-display}}\n\
         ![]({plot_path})\n:::\n:::\n\nAfter.\n"
    );
    EngineCapture {
        engine_name: "r".to_string(),
        input_qmd: CAPTURE_DOC.to_string(),
        result: serde_json::json!({ "markdown": markdown }),
        files: vec![CaptureFile {
            path: plot_path.to_string(),
            contents_base64: base64::engine::general_purpose::STANDARD.encode(PNG),
        }],
    }
}

/// A capture plot is written into the request directory (never beside the
/// source) and found by typst, including at a nested path.
#[test]
fn capture_plots_land_in_the_request_directory_and_typst_finds_them() {
    require_typst!();
    for plot in ["plot.png", "doc_files/figure-typst/cell-1.png"] {
        let proj = TempDir::new().unwrap();
        let root = proj.path().canonicalize().unwrap();
        write(&root.join("_quarto.yml"), "project:\n  title: x\n");
        write(&root.join("doc.qmd"), CAPTURE_DOC);
        let req = root.join(".quarto/preview-out/t");
        let options = RenderToFileOptions {
            output_dir: Some(req.clone()),
            quiet: true,
            execution_policy: quarto_core::engine::ExecutionPolicy::None,
            captures: vec![plot_capture(plot)],
            capture_materialize_dir: Some(req.clone()),
            ..Default::default()
        };
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        let result = render_to_file(&root.join("doc.qmd"), "typst", &options, runtime)
            .unwrap_or_else(|e| panic!("render with plot {plot} failed: {e}"));
        let bad: Vec<_> = result
            .render_output
            .diagnostics
            .iter()
            .filter(|d| matches!(d.code.as_deref(), Some("Q-5-6" | "Q-11-1")))
            .collect();
        assert!(bad.is_empty(), "resource diagnostics for {plot}: {bad:?}");
        assert!(req.join("doc.pdf").is_file());
        assert_eq!(
            std::fs::read(req.join(plot)).unwrap(),
            PNG,
            "{plot} in request dir"
        );
        assert!(
            !root.join(plot).exists(),
            "{plot} must not be written beside the source"
        );
        assert!(!root.join("doc_files").exists());
    }
}

/// pandoc finds a capture plot in the request directory through the extra
/// `--resource-path`, in an OS temp request directory (non-typst formats).
#[test]
fn capture_plots_reach_a_docx_from_the_request_directory() {
    if !have("pandoc") {
        eprintln!("SKIP: pandoc is required");
        return;
    }
    let proj = TempDir::new().unwrap();
    let os = TempDir::new().unwrap();
    let root = proj.path().canonicalize().unwrap();
    write(&root.join("_quarto.yml"), "project:\n  title: x\n");
    write(&root.join("doc.qmd"), CAPTURE_DOC);
    let req = os.path().canonicalize().unwrap().join("req");
    let options = RenderToFileOptions {
        output_dir: Some(req.clone()),
        quiet: true,
        execution_policy: quarto_core::engine::ExecutionPolicy::None,
        captures: vec![plot_capture("doc_files/cell-1.png")],
        capture_materialize_dir: Some(req.clone()),
        ..Default::default()
    };
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    render_to_file(&root.join("doc.qmd"), "docx", &options, runtime).unwrap();
    let file = std::fs::File::open(req.join("doc.docx")).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let has_png = (0..zip.len()).any(|i| {
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut zip.by_index(i).unwrap(), &mut buf).unwrap();
        buf == PNG
    });
    assert!(has_png, "docx has no embedded plot");
    assert!(!root.join("doc_files").exists());
}

// ---- unexecuted-cell count (6.5) ----------------------------------------

fn render_docx_unexecuted(
    qmd: &str,
    captures: Vec<EngineCapture>,
    policy: quarto_core::engine::ExecutionPolicy,
) -> usize {
    let proj = TempDir::new().unwrap();
    let root = proj.path().canonicalize().unwrap();
    write(&root.join("_quarto.yml"), "project:\n  title: x\n");
    write(&root.join("doc.qmd"), qmd);
    let options = RenderToFileOptions {
        output_dir: Some(root.join(".quarto/preview-out/t")),
        quiet: true,
        execution_policy: policy,
        captures,
        ..Default::default()
    };
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    render_to_file(&root.join("doc.qmd"), "docx", &options, runtime)
        .unwrap()
        .unexecuted_cells
}

#[test]
fn unexecuted_cells_are_counted_under_policy_none() {
    if !have("pandoc") {
        eprintln!("SKIP: pandoc is required");
        return;
    }
    use quarto_core::engine::ExecutionPolicy;
    let two = "---\ntitle: T\n---\n\n```{r}\n1\n```\n\n```{python}\n2\n```\n";
    assert_eq!(
        render_docx_unexecuted(two, Vec::new(), ExecutionPolicy::None),
        2
    );
    // A document without cells counts none.
    assert_eq!(
        render_docx_unexecuted("# Hi\n", Vec::new(), ExecutionPolicy::None),
        0
    );
    // The capture covers the `{r}` cell only.
    let input = two.to_string();
    let capture = EngineCapture {
        engine_name: "r".to_string(),
        input_qmd: input,
        result: serde_json::json!({ "markdown":
            "---\ntitle: T\n---\n\n::: {.cell}\n::: {.cell-output .cell-output-stdout}\nOUT\n:::\n:::\n\n```{python}\n2\n```\n" }),
        files: Vec::new(),
    };
    assert_eq!(
        render_docx_unexecuted(two, vec![capture], ExecutionPolicy::None),
        1
    );
}

// ---- request directory (6.7-6.9, E9) ------------------------------------

use quarto_core::preview_out::PreviewOutDir;

/// Render `input` to typst into a fresh [`PreviewOutDir`], handing the guard
/// to `check` (which runs while the directory exists) and returning the
/// directory's path so the caller can assert it is gone afterwards.
fn in_request_dir(
    input: &Path,
    source: Option<&str>,
    check: impl FnOnce(&quarto_core::Result<quarto_core::render_to_file::RenderToFileResult>, &Path),
) -> std::path::PathBuf {
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let dir = PreviewOutDir::create(input, runtime.as_ref()).unwrap();
    let options = RenderToFileOptions {
        output_dir: Some(dir.path().to_path_buf()),
        quiet: true,
        source_override: source.map(|s| s.as_bytes().to_vec()),
        ..Default::default()
    };
    let result = render_document_to_file(
        input,
        "typst",
        &options,
        Some(dir.project()),
        runtime,
        None,
        None,
        None,
    );
    check(&result, dir.path());
    dir.path().to_path_buf()
}

use quarto_core::render_to_file::render_document_to_file;

#[test]
fn typst_renders_into_a_request_directory_and_cleans_up() {
    require_typst!();
    let proj = TempDir::new().unwrap();
    let root = proj.path().canonicalize().unwrap();
    write(&root.join("_quarto.yml"), "project:\n  title: x\n");
    write(&root.join("a.qmd"), "---\ntitle: T\n---\n\nHello\n");
    let gone = in_request_dir(&root.join("a.qmd"), None, |r, dir| {
        r.as_ref().unwrap_or_else(|e| panic!("render failed: {e}"));
        assert!(dir.join("a.pdf").is_file());
    });
    assert!(
        !gone.exists(),
        "request directory must be removed after success"
    );
}

#[test]
fn the_request_directory_is_removed_after_an_error_and_an_abort() {
    require_typst!();
    let proj = TempDir::new().unwrap();
    let root = proj.path().canonicalize().unwrap();
    write(&root.join("_quarto.yml"), "project:\n  title: x\n");
    write(&root.join("a.qmd"), "---\ntitle: T\n---\n\nHello\n");
    // Error: a raw typst block that does not compile.
    let broken = "---\ntitle: T\n---\n\n```{=typst}\n#unknown-function()\n```\n";
    let gone = in_request_dir(&root.join("a.qmd"), Some(broken), |r, _| {
        assert!(r.is_err(), "a broken typst document must fail");
    });
    assert!(
        !gone.exists(),
        "request directory must be removed after an error"
    );
    // Abort: the guard drops during unwinding.
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let dir = PreviewOutDir::create(&root.join("a.qmd"), runtime.as_ref()).unwrap();
    let path = dir.path().to_path_buf();
    let aborted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _held = dir;
        panic!("abort mid-render");
    }));
    assert!(aborted.is_err());
    assert!(
        !path.exists(),
        "request directory must be removed after an abort"
    );
}

/// A subfolder document, a nested-`_quarto.yml` document and a document in
/// a `type: website` project (`output_dir = _site`, `Q-21-3` before Plan 2)
/// all render with their image: typst fails on a missing image, so a PDF
/// proves it resolved.
#[test]
fn typst_request_directory_renders_images_in_every_project_shape() {
    require_typst!();
    for (name, yml, nested_yml) in [
        ("default", "project:\n  title: x\n", false),
        ("website", "project:\n  type: website\n", false),
        ("nested", "project:\n  title: x\n", true),
    ] {
        let proj = TempDir::new().unwrap();
        let root = proj.path().canonicalize().unwrap();
        write(&root.join("_quarto.yml"), yml);
        let doc_dir = if nested_yml {
            write(&root.join("inner/_quarto.yml"), "project:\n  title: y\n");
            root.join("inner/sub")
        } else {
            root.join("sub")
        };
        write(
            &doc_dir.join("a.qmd"),
            "---\ntitle: T\n---\n\n![](pic.png)\n",
        );
        std::fs::write(doc_dir.join("pic.png"), PNG).unwrap();
        let gone = in_request_dir(&doc_dir.join("a.qmd"), None, |r, dir| {
            r.as_ref()
                .unwrap_or_else(|e| panic!("{name}: render failed: {e}"));
            assert!(dir.join("a.pdf").is_file(), "{name}: no pdf");
            // The request directory is inside the discovered project.
            let discovered = if nested_yml {
                root.join("inner")
            } else {
                root.clone()
            };
            assert!(
                dir.starts_with(discovered.join(".quarto/preview-out")),
                "{name}: {dir:?}"
            );
        });
        assert!(!gone.exists());
    }
}
