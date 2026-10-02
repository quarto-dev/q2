//! R1: `PandocWriteStage::prepare()` builds a `PandocRequest` that is stable,
//! sensitive, filesystem-free, path-clean, rule-abiding and equal to what a
//! real native run did (R0's recordings).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine as _;
use quarto_core::format::{Format, FormatIdentifier};
use quarto_core::pandoc_request::{
    PandocRequest, PrepareOptions, RequestFile, constants, validate_mounts,
};
use quarto_core::pipeline::{
    build_pandoc_pipeline_stages, build_pandoc_request_stages, run_pipeline,
};
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};
use quarto_core::stage::stages::PandocPrepareStage;
use serde_json::Value;

const SDE: i64 = 1_700_000_000;

fn wasm_opts() -> PrepareOptions {
    PrepareOptions {
        temp_root: PathBuf::from(&constants().share_root),
        source_date_epoch: Some(SDE),
        collect_resources: true,
        typst_available_fonts: None,
        post: quarto_core::pandoc_request::RequestPost::None,
    }
}

/// Run the real pre-write pipeline over `qmd` with `PandocPrepareStage` in
/// place of `PandocWriteStage`. `project_dir` need not exist.
fn prepare_at(project_dir: &Path, qmd: &[u8], opts: Option<PrepareOptions>) -> PandocRequest {
    let input = project_dir.join("doc.qmd");
    let project = ProjectContext {
        dir: project_dir.to_path_buf(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path(&input)],
        output_dir: project_dir.to_path_buf(),
        ..Default::default()
    };
    let doc = DocumentInfo::from_path(&input).with_output(project_dir.join("doc.docx"));
    let format = Format::docx();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
    ctx.prepare_options = opts;
    let mut stages = build_pandoc_pipeline_stages(FormatIdentifier::Docx);
    stages.pop();
    stages.push(Box::new(PandocPrepareStage::new()));
    let runtime = Arc::new(quarto_system_runtime::NativeRuntime::new());
    // The full path, as the hub passes it: the input JSON's source table names it.
    let source_name = input.to_string_lossy();
    pollster::block_on(run_pipeline(qmd, &source_name, &mut ctx, runtime, stages))
        .expect("pipeline runs");
    ctx.pandoc_request
        .take()
        .expect("PandocPrepareStage left a request")
}

const DOC: &[u8] = b"---\ntitle: T\n---\n\n# Hi\n\nSome *text*.\n";

#[test]
fn job_id_is_stable_across_temp_roots_and_epochs_and_sensitive_to_inputs() {
    let dir = Path::new("/nonexistent/proj");
    let opts = |root: &str, sde| PrepareOptions {
        temp_root: PathBuf::from(root),
        source_date_epoch: sde,
        collect_resources: false,
        typst_available_fonts: None,
        post: quarto_core::pandoc_request::RequestPost::None,
    };
    let a = prepare_at(dir, DOC, Some(opts("/nonexistent/tmp-aaa", Some(1))));
    let b = prepare_at(dir, DOC, Some(opts("/nonexistent/tmp-bbb", Some(2))));
    let c = prepare_at(dir, DOC, Some(opts("/nonexistent/tmp-ccc", None)));
    assert_ne!(a.share_root, b.share_root);
    assert_eq!(
        a.job_id, b.job_id,
        "temp root and SOURCE_DATE_EPOCH must not matter"
    );
    assert_eq!(a.job_id, c.job_id);

    let mut m = a.clone();
    m.env.insert("EXTRA".into(), "1".into());
    assert_ne!(m.compute_job_id(), a.job_id, "one env var");
    let mut m = a.clone();
    m.argv.push("--toc".into());
    assert_ne!(m.compute_job_id(), a.job_id, "one argv element");
    let mut m = a.clone();
    m.files[0].bytes.push(b' ');
    assert_ne!(m.compute_job_id(), a.job_id, "one file's bytes");

    let other = prepare_at(
        dir,
        b"# Different\n",
        Some(opts("/nonexistent/tmp-aaa", None)),
    );
    assert_ne!(other.job_id, a.job_id, "a different document");
}

#[test]
fn prepare_writes_nothing_and_needs_nothing_on_disk() {
    let root = std::env::temp_dir().join("q2-r1-prepare-nonexistent");
    let _ = std::fs::remove_dir_all(&root);
    let share = root.join("share");
    let proj = root.join("proj");
    let request = prepare_at(
        &proj,
        DOC,
        Some(PrepareOptions {
            temp_root: share.clone(),
            source_date_epoch: None,
            collect_resources: false,
            typst_available_fonts: None,
            post: quarto_core::pandoc_request::RequestPost::None,
        }),
    );
    assert!(!root.exists(), "prepare() created {}", root.display());
    assert!(
        request
            .files
            .iter()
            .any(|f| f.path.ends_with("/pandoc-input.json"))
    );
}

fn is_abs(s: &str) -> bool {
    s.starts_with('/') || (s.len() >= 3 && s.as_bytes()[1] == b':' && s.as_bytes()[2] == b'/')
}

#[test]
fn argv_paths_are_absolute_slash_normalized_and_env_is_an_allowlist() {
    let request = prepare_at(Path::new("/nonexistent/proj"), DOC, Some(wasm_opts()));
    assert!(
        request.argv.iter().all(|a| !a.contains('\\')),
        "{:?}",
        request.argv
    );
    for flag in [
        "--data-dir",
        "-L",
        "--resource-path",
        "-o",
        "--reference-doc",
        "--template",
    ] {
        if let Some(i) = request.argv.iter().position(|a| a == flag) {
            assert!(
                is_abs(&request.argv[i + 1]),
                "{flag} {}",
                request.argv[i + 1]
            );
        }
    }
    assert!(is_abs(request.argv.last().unwrap()));
    let keys: Vec<&str> = request.env.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        [
            "QUARTO_FILTER_DEPENDENCY_FILE",
            "QUARTO_FILTER_PARAMS",
            "QUARTO_SHARE_PATH",
            "SOURCE_DATE_EPOCH"
        ]
    );
    for p in request.files.iter().map(|f| &f.path).chain(&request.dirs) {
        assert!(is_abs(p) && !p.contains('\\'), "{p}");
    }
}

#[test]
fn wasm_and_native_style_options_differ_as_specified() {
    let wasm = prepare_at(Path::new("/nonexistent/proj"), DOC, Some(wasm_opts()));
    assert!(wasm.dirs.iter().any(|d| d == "/tmp"));
    assert_eq!(wasm.share_root, constants().share_root);
    assert_eq!(
        wasm.share_tree_path,
        format!("{}/pandoc-share", wasm.share_root)
    );
    assert_eq!(wasm.env["SOURCE_DATE_EPOCH"], SDE.to_string());
    assert_eq!(wasm.expected_pandoc_wasm_sha256, constants().wasm_sha256);

    let native = prepare_at(
        Path::new("/nonexistent/proj"),
        DOC,
        Some(PrepareOptions::native(PathBuf::from(
            "/nonexistent/native-tmp",
        ))),
    );
    assert!(!native.dirs.iter().any(|d| d == "/tmp"));
    assert!(!native.env.contains_key("SOURCE_DATE_EPOCH"));
    assert!(native.resource_refs.is_empty());
}

#[test]
fn relative_highlight_theme_resolves_against_the_document_directory() {
    let qmd = b"---\nformat:\n  docx:\n    highlight-style: my.theme\n---\n\n# Hi\n";
    let request = prepare_at(Path::new("/nonexistent/proj"), qmd, Some(wasm_opts()));
    let i = request
        .argv
        .iter()
        .position(|a| a == "--highlight-style")
        .expect("--highlight-style forwarded");
    assert_eq!(request.argv[i + 1], "/nonexistent/proj/my.theme");

    // A built-in style name stays verbatim.
    let qmd = b"---\nformat:\n  docx:\n    highlight-style: tango\n---\n\n# Hi\n";
    let request = prepare_at(Path::new("/nonexistent/proj"), qmd, Some(wasm_opts()));
    let i = request
        .argv
        .iter()
        .position(|a| a == "--highlight-style")
        .unwrap();
    assert_eq!(request.argv[i + 1], "tango");
}

fn file(path: &str, bytes: &[u8]) -> RequestFile {
    RequestFile {
        path: path.into(),
        bytes: bytes.to_vec(),
    }
}

fn base_request() -> PandocRequest {
    let mut r = prepare_at(Path::new("/nonexistent/proj"), DOC, Some(wasm_opts()));
    r.resource_refs.clear();
    r
}

#[test]
fn mount_rules() {
    // Identical entries are deduped.
    let mut r = base_request();
    r.resource_refs = vec![file("/proj/a.png", b"x"), file("/proj/a.png", b"x")];
    r.dirs.push("/proj/empty".into());
    r.dirs.push("/proj/empty".into());
    validate_mounts(&mut r, true).unwrap();
    assert_eq!(r.resource_refs.len(), 1);
    assert_eq!(r.dirs.iter().filter(|d| *d == "/proj/empty").count(), 1);

    let fails = |f: &dyn Fn(&mut PandocRequest), wasm: bool| {
        let mut r = base_request();
        f(&mut r);
        validate_mounts(&mut r, wasm).is_err()
    };
    // A path in two of files/resource_refs/dirs.
    assert!(fails(
        &|r| r.resource_refs.push(file(&r.files[0].path.clone(), b"")),
        true
    ));
    assert!(fails(&|r| r.dirs.push(r.files[0].path.clone()), true));
    assert!(fails(
        &|r| {
            r.resource_refs.push(file("/proj/x", b""));
            r.dirs.push("/proj/x".into());
        },
        true
    ));
    // Both a file and a directory (a file under another file's path).
    assert!(fails(
        &|r| {
            r.resource_refs.push(file("/proj/a", b""));
            r.resource_refs.push(file("/proj/a/b", b""));
        },
        true
    ));
    // A `files` path equal to a share-tree entry.
    assert!(fails(
        &|r| {
            let p = format!("{}/filters/main.lua", r.share_tree_path);
            r.files.push(file(&p, b"x"));
        },
        false
    ));
    // Wasm mode only: reserved prefixes.
    let under_share =
        |r: &mut PandocRequest| r.resource_refs.push(file("/__q2_share__/img.png", b""));
    assert!(fails(&under_share, true));
    assert!(!fails(&under_share, false));
    let under_tmp = |r: &mut PandocRequest| r.resource_refs.push(file("/tmp/img.png", b""));
    assert!(fails(&under_tmp, true));
    assert!(!fails(&under_tmp, false));
    let file_tmp = |r: &mut PandocRequest| r.files.push(file("/tmp/f", b""));
    assert!(fails(&file_tmp, true));
    assert!(!fails(&file_tmp, false));
}

// --- the request against a recorded real native run (R0) ---------------

const DOCX_FIXTURES: &[(&str, &str, &str, &[&str])] = &[
    ("callouts", "pandoc-goldens", "callouts.qmd", &[]),
    (
        "crossrefs",
        "pandoc-goldens",
        "crossrefs/all-docx.qmd",
        &["crossrefs/img/thinker.jpg"],
    ),
    (
        "citations",
        "pandoc-recordings/sources/citations",
        "citations.qmd",
        &["refs.bib"],
    ),
    (
        "tables",
        "pandoc-recordings/sources/tables",
        "tables.qmd",
        &[],
    ),
    (
        "shortcodes",
        "pandoc-recordings/sources/shortcodes",
        "shortcodes.qmd",
        &[],
    ),
    (
        "images",
        "pandoc-recordings/sources/images",
        "images.qmd",
        &["img/square.png"],
    ),
];

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Rewrite the request's real roots to R0's placeholders. The recordings put
/// the share tree at `/__q2_share__` itself, the request puts it at
/// `<temp_root>/pandoc-share`.
fn to_recording_roots(s: &str, req: &PandocRequest) -> String {
    s.replace(&req.share_tree_path, "/__q2_share__")
        .replace(&req.share_root, "/__q2_tmp__")
        .replace(&req.doc_dir, "/__q2_doc__")
}

/// Machine-specific values inside the params blob: the CLI path and the
/// Typst path are process state, not document state, and the recorded run's
/// font list came from `typst fonts`, which the browser request does not run
/// (the host supplies it).
fn scrub_params(v: &mut Value) {
    if let Some(obj) = v.as_object_mut() {
        for key in ["quarto-cli-path", "typst-path", "typst-available-fonts"] {
            obj.remove(key);
        }
    }
}

fn decode_params(blob: &str) -> Value {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(blob)
        .unwrap();
    let mut v: Value = serde_json::from_slice(&bytes).unwrap();
    scrub_params(&mut v);
    v
}

/// Prepare each fixture as `format` and compare with what native pandoc
/// did (R0's recordings): argv, the path/share env and the filter params.
/// Returns the request and the recording directory per fixture, for the
/// caller's format-specific checks.
fn check_against_recordings(
    format: &Format,
    suffix: &str,
    stages: impl Fn() -> Vec<Box<dyn quarto_core::stage::PipelineStage>>,
    mut extra: impl FnMut(&str, &PandocRequest, &Path),
) {
    let rec_root = fixtures_dir().join("pandoc-recordings/recordings");
    for (name, root, qmd, resources) in DOCX_FIXTURES {
        let scratch = tempfile::tempdir().unwrap();
        let work = scratch.path().join("docs");
        for rel in std::iter::once(qmd).chain(resources.iter()) {
            let dst = work.join(rel);
            std::fs::create_dir_all(dst.parent().unwrap()).unwrap();
            std::fs::copy(fixtures_dir().join(root).join(rel), &dst).unwrap();
        }
        let input = work.join(qmd);
        let doc_dir = input.parent().unwrap().to_path_buf();
        let temp_root = scratch.path().join("tmp");

        let project = ProjectContext {
            dir: doc_dir.clone(),
            is_single_file: true,
            files: vec![DocumentInfo::from_path(&input)],
            output_dir: doc_dir.clone(),
            ..Default::default()
        };
        let output = input.with_extension(&format.output_extension);
        let doc = DocumentInfo::from_path(&input).with_output(&output);
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, format, &binaries);
        ctx.prepare_options = Some(PrepareOptions {
            temp_root,
            source_date_epoch: Some(SDE),
            collect_resources: false,
            typst_available_fonts: None,
            post: quarto_core::pandoc_request::RequestPost::None,
        });
        let runtime = Arc::new(quarto_system_runtime::NativeRuntime::new());
        let content = std::fs::read(&input).unwrap();
        let file_name = input.file_name().unwrap().to_str().unwrap();
        pollster::block_on(run_pipeline(
            &content,
            file_name,
            &mut ctx,
            runtime,
            stages(),
        ))
        .unwrap_or_else(|e| panic!("{name}: {e}"));
        let req = ctx.pandoc_request.take().unwrap();

        let rec = rec_root.join(format!("{name}-{suffix}"));
        let argv: Vec<String> =
            serde_json::from_slice(&std::fs::read(rec.join("argv.json")).unwrap()).unwrap();
        let got_argv: Vec<String> = req
            .argv
            .iter()
            .map(|a| to_recording_roots(a, &req))
            .collect();
        assert_eq!(got_argv, argv, "{name}: argv");

        let env: std::collections::BTreeMap<String, String> =
            serde_json::from_slice(&std::fs::read(rec.join("env.json")).unwrap()).unwrap();
        for key in ["QUARTO_SHARE_PATH", "QUARTO_FILTER_DEPENDENCY_FILE"] {
            assert_eq!(
                to_recording_roots(&req.env[key], &req),
                env[key],
                "{name}: {key}"
            );
        }
        let want = decode_params(&env["QUARTO_FILTER_PARAMS"]);
        let got = {
            let text = String::from_utf8(
                base64::engine::general_purpose::STANDARD
                    .decode(&req.env["QUARTO_FILTER_PARAMS"])
                    .unwrap(),
            )
            .unwrap();
            let mut v: Value = serde_json::from_str(&to_recording_roots(&text, &req)).unwrap();
            scrub_params(&mut v);
            v
        };
        assert_eq!(got, want, "{name}: QUARTO_FILTER_PARAMS");
        assert!(!req.env.contains_key("LANG"));
        extra(name, &req, &rec);
    }
}

#[test]
fn docx_request_matches_the_recorded_native_run() {
    check_against_recordings(
        &Format::docx(),
        "docx",
        || {
            let mut stages = build_pandoc_pipeline_stages(FormatIdentifier::Docx);
            stages.pop();
            stages.push(Box::new(PandocPrepareStage::new()));
            stages
        },
        |_, _, _| {},
    );
}

/// The typst request is the recorded native run structurally: argv, env,
/// params and every staged template file (R4). The `.typ` limitations the
/// recording does not have (design D8.6): no `typst-available-fonts` (the
/// host's list or none; scrubbed above), and no `typst-packages/` or
/// fonts, which only the PDF compile reads.
#[test]
fn typst_request_matches_the_recorded_native_run() {
    check_against_recordings(
        &Format::from_format_string("typst").unwrap(),
        "typst",
        || build_pandoc_request_stages(Vec::new()),
        |name, req, rec| {
            let tmp = rec.join("fs/__q2_tmp__");
            let mut checked = 0;
            for entry in walk(&tmp.join("pandoc-typst-template")) {
                let rel = entry
                    .strip_prefix(&tmp)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                let want_path = format!("{}/{rel}", req.share_root);
                let file = req
                    .files
                    .iter()
                    .find(|f| f.path == want_path)
                    .unwrap_or_else(|| panic!("{name}: no request file {want_path}"));
                assert_eq!(file.bytes, std::fs::read(&entry).unwrap(), "{name}: {rel}");
                checked += 1;
            }
            assert!(checked >= 8, "{name}: template files checked: {checked}");
            // Nothing else of the template tree sneaks in.
            let staged = req
                .files
                .iter()
                .filter(|f| f.path.contains("/pandoc-typst-template/"))
                .count();
            assert_eq!(staged, checked, "{name}: template files in the request");
        },
    );
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(walk(&path));
        } else {
            out.push(path);
        }
    }
    out
}

// --- the published golden is a real request ----------------------------

const GOLDEN_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/schemas/pandoc-request.golden.json"
);
const GOLDEN_DOC: &[u8] =
    b"---\ntitle: Golden\n---\n\n# Hello\n\nA *docx* paragraph.\n\n![A figure](figure.png)\n";

/// What `schemas/pandoc-request.golden.json` is made of: a real wasm-style
/// `prepare()` of a small document at `/project/doc.qmd` that references
/// `figure.png`, plus that file's `resource_refs` entry. The entry is added
/// by hand because a native test cannot serve `/project` from a VFS
/// (`WasmRuntime` is wasm32-only); it is exactly what the collector emits
/// there, and `hub-client/src/services/pandocRequest.wasm.test.ts` proves it
/// by rendering this document in the built wasm and deep-equaling the golden.
fn golden_request() -> PandocRequest {
    let mut request = prepare_at(Path::new("/project"), GOLDEN_DOC, Some(wasm_opts()));
    request
        .resource_refs
        .push(file("/project/figure.png", b"\x89PNG\r\n\x1a\n"));
    request.job_id = request.compute_job_id();
    request
}

/// Regenerate with `Q2_REGENERATE_GOLDEN=1 cargo nextest run -p quarto-core
/// golden_file`. Without it the golden must agree with a fresh `prepare()` on
/// everything except the AST-derived bytes (an unrelated pampa JSON change
/// must not break this test).
#[test]
fn golden_file_is_structurally_current() {
    let fresh = golden_request();
    if std::env::var_os("Q2_REGENERATE_GOLDEN").is_some() {
        let mut text = serde_json::to_string_pretty(&fresh).unwrap();
        text.push('\n');
        std::fs::write(GOLDEN_PATH, text).unwrap();
    }
    let golden: PandocRequest =
        serde_json::from_str(&std::fs::read_to_string(GOLDEN_PATH).unwrap()).unwrap();
    assert_eq!(golden.argv, fresh.argv);
    assert_eq!(
        golden.env.keys().collect::<Vec<_>>(),
        fresh.env.keys().collect::<Vec<_>>()
    );
    for key in [
        "QUARTO_SHARE_PATH",
        "QUARTO_FILTER_DEPENDENCY_FILE",
        "SOURCE_DATE_EPOCH",
    ] {
        assert_eq!(golden.env[key], fresh.env[key], "{key}");
    }
    let paths = |r: &PandocRequest| -> Vec<String> {
        r.files
            .iter()
            .chain(&r.resource_refs)
            .map(|f| f.path.clone())
            .collect()
    };
    assert_eq!(paths(&golden), paths(&fresh));
    assert_eq!(golden.dirs, fresh.dirs);
    assert_eq!(golden.share_tree_version, fresh.share_tree_version);
    assert_eq!(
        golden.expected_pandoc_wasm_sha256,
        fresh.expected_pandoc_wasm_sha256
    );
    assert_eq!(golden.job_id, golden.compute_job_id());
}
