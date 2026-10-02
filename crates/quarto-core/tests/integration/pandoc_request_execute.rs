//! R1: the native executor runs a `PandocRequest` as data, and the pandoc
//! version gate still stops `PandocWriteStage` before anything runs. A fake
//! `pandoc` records what it was given, so none of this needs the real one.
//! (Replaces the source-grepping `test_version_gate_is_wired_into_run`.)

use std::path::{Path, PathBuf};
use std::sync::Arc;

use quarto_core::format::{Format, FormatIdentifier};
use quarto_core::pandoc_request::{PandocRequest, PrepareOptions};
use quarto_core::pipeline::{build_pandoc_pipeline_stages, run_pipeline};
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};
use quarto_core::stage::PandocWriteStage;

/// A fake pandoc. `--version` prints `pandoc $FAKE_VERSION`; any other call
/// writes its argv and the interesting env to `$FAKE_LOG`, and exits 3 with
/// `lua: boom` on stderr when `$FAKE_FAIL` is set.
#[cfg(unix)]
fn write_fake_pandoc(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("fake-pandoc");
    std::fs::write(
        &path,
        "#!/bin/sh\n\
         if [ \"$1\" = \"--version\" ]; then echo \"pandoc $FAKE_VERSION\"; exit 0; fi\n\
         { for a in \"$@\"; do echo \"ARG:$a\"; done\n\
           echo \"SHARE:$QUARTO_SHARE_PATH\"\n\
           echo \"SDE:${SOURCE_DATE_EPOCH-unset}\"; } > \"$FAKE_LOG\"\n\
         if [ -n \"$FAKE_FAIL\" ]; then echo \"lua: boom\" >&2; exit 3; fi\n\
         exit 0\n",
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[cfg(not(unix))]
fn write_fake_pandoc(dir: &Path) -> PathBuf {
    let path = dir.join("fake-pandoc.cmd");
    std::fs::write(
        &path,
        "@echo off\r\n\
         if \"%~1\"==\"--version\" (echo pandoc %FAKE_VERSION% & exit /b 0)\r\n\
         (for %%a in (%*) do echo ARG:%%a) > \"%FAKE_LOG%\"\r\n\
         echo SHARE:%QUARTO_SHARE_PATH%>> \"%FAKE_LOG%\"\r\n\
         if defined FAKE_FAIL (echo lua: boom 1>&2 & exit /b 3)\r\n\
         exit /b 0\r\n",
    )
    .unwrap();
    path
}

struct Fake {
    _dir: tempfile::TempDir,
    log: PathBuf,
}

/// Point `QUARTO_PANDOC` at a fresh fake. nextest runs each test in its own
/// process, so the process-global env is private to the test.
fn install_fake(version: &str, fail: bool) -> (Fake, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let fake = write_fake_pandoc(dir.path());
    let log = dir.path().join("pandoc.log");
    unsafe {
        std::env::set_var("QUARTO_PANDOC", &fake);
        std::env::set_var("FAKE_VERSION", version);
        std::env::set_var("FAKE_LOG", &log);
        if fail {
            std::env::set_var("FAKE_FAIL", "1");
        }
    }
    (Fake { _dir: dir, log }, fake)
}

struct Render {
    result: Result<(), String>,
    project_dir: PathBuf,
    temp_root: PathBuf,
    _scratch: tempfile::TempDir,
}

/// Render `# Hi` through the real Pandoc pipeline (docx) with
/// `PandocWriteStage` as its last stage.
fn render_docx() -> Render {
    let scratch = tempfile::tempdir().unwrap();
    let project_dir = scratch.path().join("proj");
    let temp_root = scratch.path().join("tmp");
    std::fs::create_dir_all(&project_dir).unwrap();
    let input = project_dir.join("doc.qmd");
    std::fs::write(&input, "# Hi\n").unwrap();
    let project = ProjectContext {
        dir: project_dir.clone(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path(&input)],
        output_dir: project_dir.clone(),
        ..Default::default()
    };
    let doc = DocumentInfo::from_path(&input).with_output(project_dir.join("doc.docx"));
    let format = Format::docx();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
    ctx.prepare_options = Some(PrepareOptions::native(temp_root.clone()));
    let stages = build_pandoc_pipeline_stages(FormatIdentifier::Docx);
    let runtime = Arc::new(quarto_system_runtime::NativeRuntime::new());
    let result = pollster::block_on(run_pipeline(
        b"# Hi\n", "doc.qmd", &mut ctx, runtime, stages,
    ))
    .map(|_| ())
    .map_err(|e| format!("{e:?}"));
    Render {
        result,
        project_dir,
        temp_root,
        _scratch: scratch,
    }
}

#[test]
fn version_gate_stops_the_stage_before_anything_runs() {
    let (fake, _) = install_fake("2.0", false);
    let r = render_docx();
    let err = r.result.expect_err("an old pandoc must fail the render");
    assert!(err.contains("Q-20-2"), "expected Q-20-2, got: {err}");
    assert!(
        !fake.log.exists(),
        "pandoc must not be run after a failed gate"
    );
    assert!(
        !r.temp_root.join("pandoc-input.json").exists()
            && !r.temp_root.join("pandoc-share").exists(),
        "no request effect may run after a failed gate"
    );
    assert!(!r.project_dir.join("doc.docx").exists());
}

#[test]
fn stage_runs_the_prepared_request_through_the_native_executor() {
    let (fake, _) = install_fake("3.11", false);
    let r = render_docx();
    r.result.expect("render with a fake pandoc succeeds");

    let log = std::fs::read_to_string(&fake.log).expect("fake pandoc was run");
    let args: Vec<&str> = log.lines().filter_map(|l| l.strip_prefix("ARG:")).collect();
    assert_eq!(&args[..4], ["-f", "json", "-t", "docx"]);
    let share = r.temp_root.join("pandoc-share");
    let norm = |p: &Path| quarto_core::pandoc_request::normalize_request_path(p);
    let datadir = args.iter().position(|a| *a == "--data-dir").unwrap();
    assert_eq!(args[datadir + 1], norm(&share.join("pandoc/datadir")));
    let out = args.iter().position(|a| *a == "-o").unwrap();
    assert_eq!(args[out + 1], norm(&r.project_dir.join("doc.docx")));
    assert!(log.contains(&format!("SHARE:{}", norm(&share))), "{log}");
    assert!(
        log.contains("SDE:unset"),
        "native requests carry no SOURCE_DATE_EPOCH"
    );

    // The effects ran: share tree extracted, successful run removed the JSON.
    assert!(share.join("filters/main.lua").is_file());
    assert!(r.temp_root.join("pandoc-filter-deps.txt").is_file());
    assert!(!r.temp_root.join("pandoc-input.json").exists());
}

/// `execute()` applies a request as data: dirs, files, share tree, argv, env.
#[test]
fn execute_applies_dirs_files_share_tree_argv_and_env() {
    let (fake, fake_bin) = install_fake("3.11", false);
    let scratch = tempfile::tempdir().unwrap();
    let mut request: PandocRequest =
        serde_json::from_str(include_str!("../../schemas/pandoc-request.golden.json")).unwrap();
    let norm = |p: &Path| quarto_core::pandoc_request::normalize_request_path(p);
    let root = scratch.path().join("root");
    // Re-root the golden under a scratch dir.
    let reroot = |s: &str| s.replace("/__q2_share__", &norm(&root));
    request.share_root = reroot(&request.share_root);
    request.share_tree_path = reroot(&request.share_tree_path);
    request.json_path = reroot(&request.json_path);
    request.argv = request.argv.iter().map(|a| reroot(a)).collect();
    for f in &mut request.files {
        f.path = reroot(&f.path);
    }
    request.dirs = vec![norm(&scratch.path().join("out/nested"))];
    request.env.insert("SOURCE_DATE_EPOCH".into(), "123".into());
    request
        .env
        .insert("QUARTO_SHARE_PATH".into(), request.share_tree_path.clone());

    let warnings = PandocWriteStage::new()
        .execute(&request, &fake_bin)
        .unwrap();
    assert!(warnings.is_empty());

    assert!(scratch.path().join("out/nested").is_dir());
    let input = root.join("pandoc-input.json");
    assert!(!input.exists(), "success removes the temp JSON");
    assert!(root.join("pandoc-filter-deps.txt").is_file());
    assert!(root.join("pandoc-share/filters/main.lua").is_file());
    let log = std::fs::read_to_string(&fake.log).unwrap();
    let args: Vec<&str> = log.lines().filter_map(|l| l.strip_prefix("ARG:")).collect();
    assert_eq!(
        args,
        request.argv[1..]
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
    );
    assert!(log.contains("SDE:123"), "request env is applied: {log}");
}

#[test]
fn execute_failure_is_q_20_3_and_keeps_the_temp_json() {
    let (_fake, fake_bin) = install_fake("3.11", true);
    let scratch = tempfile::tempdir().unwrap();
    let mut request: PandocRequest =
        serde_json::from_str(include_str!("../../schemas/pandoc-request.golden.json")).unwrap();
    let norm = |p: &Path| quarto_core::pandoc_request::normalize_request_path(p);
    let root = norm(&scratch.path().join("root"));
    request.share_tree_path = format!("{root}/pandoc-share");
    request.json_path = format!("{root}/pandoc-input.json");
    request.files[0].path = request.json_path.clone();
    request.files[1].path = format!("{root}/pandoc-filter-deps.txt");
    request.dirs.clear();

    let err = PandocWriteStage::new()
        .execute(&request, &fake_bin)
        .expect_err("a non-zero exit is an error");
    let text = format!("{err:?}");
    assert!(text.contains("boom"), "{text}");
    assert!(
        Path::new(&request.json_path).exists(),
        "the temp JSON is retained on failure"
    );
}
