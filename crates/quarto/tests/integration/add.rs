//! End-to-end CLI tests for `q2 add` (bd-rfz4e3bw).
//!
//! Each test spawns the real `q2` binary and asserts on the files it
//! writes (and the ones it must not), its exit code, and its output.
//!
//! Contract under verification (per
//! `claude-notes/plans/2026-10-22-q2-add-extension.md`):
//!
//! - **Sources.** A local folder (containing `_extensions/`, or being
//!   one), a local `.tar.gz` / `.zip`, and an archive URL all install.
//!   GitHub shorthand shares the URL path and is covered at the unit
//!   level with a scripted fetcher; no test talks to github.com.
//! - **Install root.** The project root when inside a project, cwd
//!   otherwise, `--dir` over both.
//! - **Q1's table.** Install / Update / Revert / No Change, with
//!   `from -> to`. No Change writes nothing.
//! - **The record.** Every install writes `.quarto-source.yml` with a
//!   sha256 per file; replacing an extension whose files changed since
//!   needs `--force`.
//! - **Trust.** A remote source needs `--trust` when the command cannot
//!   prompt, and is refused *before any request* — in a dry run too,
//!   since a dry run still downloads. `--trust` combines with
//!   `--dry-run`; `--force` does not.
//! - **It works.** An installed custom project type renders.
//!
//! The key gates were checked against deliberately broken code (no
//! edit check, no trust gate, merge instead of replace) and fail there.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use sha2::{Digest, Sha256};
use tempfile::TempDir;

use crate::archive_server::{TestServer, tar_gz};

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

/// Run `q2 add <args...>` from `cwd`, never interactively.
fn run_q2_add(cwd: &Path, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(Q2_BIN);
    cmd.current_dir(cwd);
    cmd.arg("add");
    cmd.args(args);
    // Keep the prompt gate closed regardless of how the test runner is
    // attached; these tests assert non-interactive behavior.
    cmd.env("CI", "1");
    cmd.stdin(Stdio::null());
    cmd.output().expect("spawn q2 binary")
}

fn stdout_str(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr_str(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn assert_succeeded(out: &std::process::Output) {
    assert!(
        out.status.success(),
        "expected success.\nstdout: {}\nstderr: {}",
        stdout_str(out),
        stderr_str(out)
    );
}

fn assert_failed(out: &std::process::Output, expect_in_stderr: &str) {
    assert!(
        !out.status.success(),
        "expected failure, got success.\nstdout: {}\nstderr: {}",
        stdout_str(out),
        stderr_str(out)
    );
    let stderr = stderr_str(out);
    assert!(
        stderr.contains(expect_in_stderr),
        "stderr should mention {expect_in_stderr:?}; got:\n{stderr}"
    );
}

fn write_tree(root: &Path, files: &[(&str, &str)]) {
    for (name, content) in files {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }
}

fn tree(files: &[(&str, &str)]) -> TempDir {
    let dir = TempDir::new().unwrap();
    write_tree(dir.path(), files);
    dir
}

fn manifest(version: &str) -> String {
    format!(
        "title: Demo Shortcode\nversion: {version}\ncontributes:\n  shortcodes:\n    - demo.lua\n"
    )
}

const DEMO_LUA: &str = "return {}\n";

/// A source folder with `_extensions/demo/` at `version`.
fn demo_source(version: &str) -> TempDir {
    tree(&[
        ("_extensions/demo/_extension.yml", &manifest(version)),
        ("_extensions/demo/demo.lua", DEMO_LUA),
    ])
}

fn project() -> TempDir {
    tree(&[
        ("_quarto.yml", "project:\n  type: website\n"),
        ("index.qmd", "# Hi\n"),
    ])
}

fn path_arg(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn record(dir: &Path) -> serde_yaml::Value {
    serde_yaml::from_str(&read(&dir.join(".quarto-source.yml"))).unwrap()
}

fn demo_files(version: &str) -> Vec<(&'static str, String)> {
    vec![
        ("_extensions/demo/_extension.yml", manifest(version)),
        ("_extensions/demo/demo.lua", DEMO_LUA.to_string()),
    ]
}

fn as_refs<'a>(files: &'a [(&'static str, String)]) -> Vec<(&'static str, &'a str)> {
    files.iter().map(|(n, c)| (*n, c.as_str())).collect()
}

// ====================================================================
// Sources and placement
// ====================================================================

#[test]
fn a_local_folder_installs_at_the_project_root_with_a_record() {
    let src = demo_source("1.0.0");
    let proj = project();
    std::fs::create_dir_all(proj.path().join("posts")).unwrap();

    // Run from a subfolder: the install still lands at the project root.
    let out = run_q2_add(&proj.path().join("posts"), &[&path_arg(src.path())]);
    assert_succeeded(&out);

    let installed = proj.path().join("_extensions").join("demo");
    assert_eq!(read(&installed.join("demo.lua")), DEMO_LUA);
    assert!(!proj.path().join("posts").join("_extensions").exists());

    let stdout = stdout_str(&out);
    assert!(stdout.contains("Demo Shortcode"), "{stdout}");
    assert!(stdout.contains("[Install]"), "{stdout}");
    assert!(stdout.contains("(shortcode)"), "{stdout}");

    let text = read(&installed.join(".quarto-source.yml"));
    assert!(
        text.starts_with("# Generated by `q2 add`. Do not edit."),
        "{text}"
    );
    let rec = record(&installed);
    assert_eq!(rec["kind"].as_str(), Some("local-dir"));
    let expected = hex(Sha256::digest(DEMO_LUA.as_bytes()).as_slice());
    assert_eq!(rec["files"]["demo.lua"].as_str(), Some(expected.as_str()));
    assert!(rec["files"].get(".quarto-source.yml").is_none());
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn a_path_to_the_extensions_folder_itself_is_accepted() {
    let src = demo_source("1.0.0");
    let proj = project();
    let out = run_q2_add(proj.path(), &[&path_arg(&src.path().join("_extensions"))]);
    assert_succeeded(&out);
    assert!(
        proj.path()
            .join("_extensions/demo/_extension.yml")
            .is_file()
    );
}

#[test]
fn local_archives_install_wrapped_or_not() {
    let files = demo_files("1.0.0");
    for (label, prefix) in [("wrapped", "repo-main/"), ("unwrapped", "")] {
        let archives = TempDir::new().unwrap();
        let tgz = archives.path().join("demo.tar.gz");
        std::fs::write(&tgz, tar_gz(prefix, &as_refs(&files))).unwrap();

        let proj = project();
        let out = run_q2_add(proj.path(), &[&path_arg(&tgz)]);
        assert_succeeded(&out);
        let installed = proj.path().join("_extensions/demo");
        assert_eq!(
            read(&installed.join("demo.lua")),
            DEMO_LUA,
            "{label} tar.gz"
        );
        assert_eq!(record(&installed)["kind"].as_str(), Some("local-archive"));
    }

    let archives = TempDir::new().unwrap();
    let zip_path = archives.path().join("demo.zip");
    {
        let file = std::fs::File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options: zip::write::SimpleFileOptions =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, content) in &files {
            zip.start_file(format!("demo-1.0.0/{name}"), options)
                .unwrap();
            zip.write_all(content.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }
    let proj = project();
    let out = run_q2_add(proj.path(), &[&path_arg(&zip_path)]);
    assert_succeeded(&out);
    assert_eq!(
        read(&proj.path().join("_extensions/demo/demo.lua")),
        DEMO_LUA
    );
}

#[test]
fn outside_a_project_it_installs_in_cwd_and_dir_overrides() {
    let src = demo_source("1.0.0");
    let cwd = TempDir::new().unwrap();
    let out = run_q2_add(cwd.path(), &[&path_arg(src.path())]);
    assert_succeeded(&out);
    assert!(cwd.path().join("_extensions/demo/_extension.yml").is_file());

    let proj = project();
    std::fs::create_dir_all(proj.path().join("elsewhere")).unwrap();
    let out = run_q2_add(proj.path(), &["--dir", "elsewhere", &path_arg(src.path())]);
    assert_succeeded(&out);
    assert!(
        proj.path()
            .join("elsewhere/_extensions/demo/_extension.yml")
            .is_file()
    );
    assert!(!proj.path().join("_extensions").exists());
}

// ====================================================================
// The action table, and what each action writes
// ====================================================================

#[test]
fn a_second_identical_add_reports_no_change_and_writes_nothing() {
    let src = demo_source("1.0.0");
    let proj = project();
    assert_succeeded(&run_q2_add(proj.path(), &[&path_arg(src.path())]));

    let installed = proj.path().join("_extensions/demo");
    let mtime = |p: &Path| std::fs::metadata(p).unwrap().modified().unwrap();
    let before = (
        mtime(&installed.join("demo.lua")),
        mtime(&installed.join(".quarto-source.yml")),
    );

    let out = run_q2_add(proj.path(), &[&path_arg(src.path())]);
    assert_succeeded(&out);
    let stdout = stdout_str(&out);
    assert!(stdout.contains("No changes required"), "{stdout}");
    let after = (
        mtime(&installed.join("demo.lua")),
        mtime(&installed.join(".quarto-source.yml")),
    );
    assert_eq!(
        before, after,
        "No Change must not touch the installed files"
    );
}

#[test]
fn a_newer_version_replaces_the_whole_directory() {
    let proj = project();
    let v1 = tree(&[
        ("_extensions/demo/_extension.yml", &manifest("1.0.0")),
        ("_extensions/demo/demo.lua", DEMO_LUA),
        ("_extensions/demo/stale.lua", "-- only in 1.0.0\n"),
    ]);
    assert_succeeded(&run_q2_add(proj.path(), &[&path_arg(v1.path())]));

    let out = run_q2_add(proj.path(), &[&path_arg(demo_source("1.1.0").path())]);
    assert_succeeded(&out);
    let stdout = stdout_str(&out);
    assert!(stdout.contains("[Update]"), "{stdout}");
    assert!(stdout.contains("1.0.0  ->  1.1.0"), "{stdout}");

    let installed = proj.path().join("_extensions/demo");
    assert!(
        !installed.join("stale.lua").exists(),
        "an update replaces, not merges"
    );
    assert!(read(&installed.join("_extension.yml")).contains("1.1.0"));
    assert!(record(&installed)["files"].get("stale.lua").is_none());
}

#[test]
fn an_older_version_is_a_revert() {
    let proj = project();
    assert_succeeded(&run_q2_add(
        proj.path(),
        &[&path_arg(demo_source("2.0.0").path())],
    ));
    let out = run_q2_add(proj.path(), &[&path_arg(demo_source("1.0.0").path())]);
    assert_succeeded(&out);
    let stdout = stdout_str(&out);
    assert!(
        stdout.contains("[Revert]") && stdout.contains("2.0.0  ->  1.0.0"),
        "{stdout}"
    );
}

// ====================================================================
// Local edits and --force
// ====================================================================

#[test]
fn local_edits_block_an_update_until_force() {
    let proj = project();
    assert_succeeded(&run_q2_add(
        proj.path(),
        &[&path_arg(demo_source("1.0.0").path())],
    ));
    let installed = proj.path().join("_extensions/demo");
    std::fs::write(installed.join("demo.lua"), "-- my local tweak\n").unwrap();
    std::fs::write(installed.join("mine.lua"), "-- added by hand\n").unwrap();

    let v2 = demo_source("1.1.0");
    let out = run_q2_add(proj.path(), &[&path_arg(v2.path())]);
    assert_failed(&out, "local changes");
    let stderr = stderr_str(&out);
    assert!(stderr.contains("demo.lua (modified)"), "{stderr}");
    assert!(stderr.contains("mine.lua (added)"), "{stderr}");
    assert!(stderr.contains("--force"), "{stderr}");
    assert_eq!(
        read(&installed.join("demo.lua")),
        "-- my local tweak\n",
        "nothing written"
    );

    let out = run_q2_add(proj.path(), &["--force", &path_arg(v2.path())]);
    assert_succeeded(&out);
    assert_eq!(read(&installed.join("demo.lua")), DEMO_LUA);
    assert!(!installed.join("mine.lua").exists());
}

#[test]
fn local_edits_survive_a_no_change_add_and_force_resets_them() {
    let proj = project();
    let src = demo_source("1.0.0");
    assert_succeeded(&run_q2_add(proj.path(), &[&path_arg(src.path())]));
    let lua = proj.path().join("_extensions/demo/demo.lua");
    std::fs::write(&lua, "-- my local tweak\n").unwrap();

    assert_succeeded(&run_q2_add(proj.path(), &[&path_arg(src.path())]));
    assert_eq!(
        read(&lua),
        "-- my local tweak\n",
        "No Change leaves edits alone"
    );

    assert_succeeded(&run_q2_add(
        proj.path(),
        &["--force", &path_arg(src.path())],
    ));
    assert_eq!(read(&lua), DEMO_LUA, "--force reinstalls");
}

#[test]
fn an_untracked_install_updates_with_a_note() {
    // As left by Quarto 1, or a hand copy: no `.quarto-source.yml`.
    let proj = project();
    write_tree(
        proj.path(),
        &[
            ("_extensions/demo/_extension.yml", &manifest("1.0.0")),
            ("_extensions/demo/demo.lua", "-- hand-copied\n"),
        ],
    );
    let out = run_q2_add(proj.path(), &[&path_arg(demo_source("1.1.0").path())]);
    assert_succeeded(&out);
    let stdout = stdout_str(&out);
    assert!(stdout.contains("no install record"), "{stdout}");
    assert!(
        proj.path()
            .join("_extensions/demo/.quarto-source.yml")
            .is_file()
    );
}

// ====================================================================
// The record as a shared format (bd-yjg91sls)
// ====================================================================

/// GH #1175: another tool's keys in the record survive `q2 add`
/// replacing the extension, both on an update and on a `--force`
/// reinstall. The keys q2 owns are rewritten.
#[test]
fn keys_q2_does_not_own_survive_a_rewrite_of_the_record() {
    let proj = project();
    assert_succeeded(&run_q2_add(
        proj.path(),
        &[&path_arg(demo_source("1.0.0").path())],
    ));
    let installed = proj.path().join("_extensions/demo");
    let record_path = installed.join(".quarto-source.yml");
    let mut text = read(&record_path);
    text.push_str(
        "quarto-wizard:\n  track: branch\n  commit: 3f2a9c1e5b7d4a60918c2d4e6f8a0b1c3d5e7f90\n",
    );
    std::fs::write(&record_path, text).unwrap();

    let v2 = demo_source("1.1.0");
    let out = run_q2_add(proj.path(), &[&path_arg(v2.path())]);
    assert_succeeded(&out);
    assert!(
        stdout_str(&out).contains("[Update]"),
        "{}",
        stdout_str(&out)
    );
    let rec = record(&installed);
    assert_eq!(
        rec["quarto-wizard"]["commit"].as_str(),
        Some("3f2a9c1e5b7d4a60918c2d4e6f8a0b1c3d5e7f90"),
        "{}",
        read(&record_path)
    );
    assert_eq!(rec["quarto-wizard"]["track"].as_str(), Some("branch"));
    assert_eq!(rec["version"].as_u64(), Some(1));
    assert!(rec["files"].get("demo.lua").is_some());

    assert_succeeded(&run_q2_add(proj.path(), &["--force", &path_arg(v2.path())]));
    assert_eq!(
        record(&installed)["quarto-wizard"]["track"].as_str(),
        Some("branch"),
        "a --force reinstall keeps them too"
    );
}

/// GH #1176: a record with a schema version above the one this q2
/// writes is not read with this version's rules. Replacing it needs
/// `--force`, which writes a clean current-version record.
#[test]
fn a_record_from_a_newer_q2_needs_force_to_replace() {
    let proj = project();
    assert_succeeded(&run_q2_add(
        proj.path(),
        &[&path_arg(demo_source("1.1.0").path())],
    ));
    let installed = proj.path().join("_extensions/demo");
    let record_path = installed.join(".quarto-source.yml");
    let text = read(&record_path).replace("\nversion: 1\n", "\nversion: 2\n") + "v2-only: x\n";
    assert!(text.contains("version: 2"), "{text}");
    std::fs::write(&record_path, &text).unwrap();

    let v3 = demo_source("1.2.0");
    let out = run_q2_add(proj.path(), &[&path_arg(v3.path())]);
    assert_failed(&out, "local changes");
    let stderr = stderr_str(&out);
    assert!(
        stderr.contains(".quarto-source.yml (written by a newer version of q2"),
        "{stderr}"
    );
    assert!(stderr.contains("Upgrade q2"), "{stderr}");
    assert_eq!(read(&record_path), text, "nothing written");

    assert_succeeded(&run_q2_add(proj.path(), &["--force", &path_arg(v3.path())]));
    let rec = record(&installed);
    assert_eq!(rec["version"].as_u64(), Some(1));
    assert!(
        rec.get("v2-only").is_none(),
        "a newer record's keys are not carried: q2 can't tell what they meant"
    );
}

/// The `.gitattributes` that stops git from converting line endings:
/// created by an install, beside the extensions, never overwritten,
/// and not created by a run that writes nothing.
#[test]
fn an_install_creates_extensions_gitattributes_only_when_missing() {
    let proj = project();
    let src = demo_source("1.0.0");
    let attrs = proj.path().join("_extensions/.gitattributes");

    let out = run_q2_add(proj.path(), &["--dry-run", &path_arg(src.path())]);
    assert_succeeded(&out);
    assert!(
        stdout_str(&out).contains("would also create"),
        "{}",
        stdout_str(&out)
    );
    assert!(!attrs.exists());

    let out = run_q2_add(proj.path(), &[&path_arg(src.path())]);
    assert_succeeded(&out);
    assert!(
        stdout_str(&out).contains(".gitattributes"),
        "{}",
        stdout_str(&out)
    );
    let content = read(&attrs);
    assert!(content.lines().any(|l| l == "* -text"), "{content}");
    assert!(
        record(&proj.path().join("_extensions/demo"))["files"]
            .get(".gitattributes")
            .is_none(),
        "it sits outside the extension, so it is never hashed"
    );

    // No Change writes nothing, even when the file is gone.
    std::fs::remove_file(&attrs).unwrap();
    assert_succeeded(&run_q2_add(proj.path(), &[&path_arg(src.path())]));
    assert!(!attrs.exists());

    // An existing file is the user's.
    std::fs::write(&attrs, "*.lua text eol=lf\n").unwrap();
    let out = run_q2_add(
        proj.path(),
        &["--json", &path_arg(demo_source("1.1.0").path())],
    );
    assert_succeeded(&out);
    assert_eq!(read(&attrs), "*.lua text eol=lf\n");
    let v: serde_json::Value = serde_json::from_str(&stdout_str(&out)).unwrap();
    assert_eq!(v["gitattributes_written"], false);
}

/// A checkout that converted LF to CRLF (git's `core.autocrlf`) is
/// still refused without `--force`, but the error names the cause and
/// the `.gitattributes` fix.
#[test]
fn line_ending_only_changes_are_named_with_the_gitattributes_fix() {
    let proj = project();
    assert_succeeded(&run_q2_add(
        proj.path(),
        &[&path_arg(demo_source("1.0.0").path())],
    ));
    let manifest_path = proj.path().join("_extensions/demo/_extension.yml");
    let crlf = read(&manifest_path).replace('\n', "\r\n");
    std::fs::write(&manifest_path, crlf).unwrap();

    let out = run_q2_add(proj.path(), &[&path_arg(demo_source("1.1.0").path())]);
    assert_failed(&out, "local changes");
    let stderr = stderr_str(&out);
    assert!(
        stderr.contains("_extension.yml (line endings changed)"),
        "{stderr}"
    );
    assert!(stderr.contains("core.autocrlf"), "{stderr}");
    assert!(stderr.contains("_extensions/.gitattributes"), "{stderr}");
}

// ====================================================================
// --dry-run and --json
// ====================================================================

#[test]
fn dry_run_shows_the_table_and_writes_nothing() {
    let proj = project();
    let out = run_q2_add(
        proj.path(),
        &["--dry-run", &path_arg(demo_source("1.0.0").path())],
    );
    assert_succeeded(&out);
    let stdout = stdout_str(&out);
    assert!(
        stdout.contains("[Install]") && stdout.contains("nothing was written"),
        "{stdout}"
    );
    assert!(!proj.path().join("_extensions").exists());

    let out = run_q2_add(proj.path(), &["--dry-run", "--force", "x/y"]);
    assert_failed(&out, "cannot be combined with --dry-run");
}

#[test]
fn json_emits_one_result_object() {
    let proj = project();
    let out = run_q2_add(
        proj.path(),
        &["--json", &path_arg(demo_source("1.0.0").path())],
    );
    assert_succeeded(&out);
    let stdout = stdout_str(&out);
    assert_eq!(
        stdout.lines().count(),
        1,
        "exactly one line on stdout: {stdout}"
    );
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(v["version"], 1);
    let ext = &v["extensions"][0];
    assert_eq!(ext["id"], "demo");
    assert_eq!(ext["path"], "_extensions/demo");
    assert_eq!(ext["action"], "install");
    assert_eq!(ext["to"], "1.0.0");
    assert_eq!(ext["written"], true);
    assert_eq!(v["gitattributes_written"], true);
}

// ====================================================================
// Invalid sources
// ====================================================================

#[test]
fn invalid_sources_are_refused_and_write_nothing() {
    let cases: Vec<(&str, TempDir, &str)> = vec![
        (
            "no extensions",
            tree(&[("README.md", "hi")]),
            "No extensions found",
        ),
        (
            "manifest without contributes",
            tree(&[("_extensions/bad/_extension.yml", "title: Bad\n")]),
            "does not load",
        ),
        (
            "hidden extension name",
            tree(&[("_extensions/.sneaky/_extension.yml", &manifest("1.0.0"))]),
            "Refusing to install",
        ),
    ];
    for (label, src, expected) in cases {
        let proj = project();
        let out = run_q2_add(proj.path(), &[&path_arg(src.path())]);
        assert_failed(&out, expected);
        assert!(
            !proj.path().join("_extensions").exists(),
            "{label}: nothing written"
        );
    }
}

/// `.tar.gz` bytes for `files` plus one symlink entry `link -> target`.
fn tar_gz_with_symlink(files: &[(&str, &str)], link: &str, target: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let encoder = flate2::write::GzEncoder::new(&mut buf, flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        for (name, content) in files {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Regular);
            header.set_mode(0o644);
            header.set_size(content.len() as u64);
            header.set_path(name).unwrap();
            header.set_cksum();
            builder.append(&header, content.as_bytes()).unwrap();
        }
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_mode(0o777);
        header.set_size(0);
        header.set_path(link).unwrap();
        header.set_link_name(target).unwrap();
        header.set_cksum();
        builder.append(&header, std::io::empty()).unwrap();
        builder.into_inner().unwrap().finish().unwrap();
    }
    buf
}

#[test]
fn a_symlink_outside_the_extension_does_not_block_the_install() {
    // The posit-dev/product-doc-theme shape: `example/_extensions` links
    // back to the real `_extensions/`.
    let files = demo_files("1.0.0");
    let archives = TempDir::new().unwrap();
    let tgz = archives.path().join("repo.tar.gz");
    std::fs::write(
        &tgz,
        tar_gz_with_symlink(&as_refs(&files), "example/_extensions", "../_extensions"),
    )
    .unwrap();

    let proj = project();
    let out = run_q2_add(proj.path(), &[&path_arg(&tgz)]);
    assert_succeeded(&out);
    assert_eq!(
        read(&proj.path().join("_extensions/demo/demo.lua")),
        DEMO_LUA
    );
}

#[test]
fn a_symlink_inside_the_extension_is_refused() {
    let files = demo_files("1.0.0");
    let archives = TempDir::new().unwrap();
    let tgz = archives.path().join("repo.tar.gz");
    std::fs::write(
        &tgz,
        tar_gz_with_symlink(
            &as_refs(&files),
            "_extensions/demo/shared.lua",
            "../../shared.lua",
        ),
    )
    .unwrap();

    let proj = project();
    let out = run_q2_add(proj.path(), &[&path_arg(&tgz)]);
    assert_failed(&out, "uses symbolic links");
    assert!(
        stderr_str(&out).contains("shared.lua"),
        "{}",
        stderr_str(&out)
    );
    assert!(!proj.path().join("_extensions").exists(), "nothing written");
}

// ====================================================================
// Remote sources, against a localhost server
// ====================================================================

#[test]
fn a_remote_source_without_trust_is_refused_before_any_request() {
    let server = TestServer::start(tar_gz("repo-main/", &as_refs(&demo_files("1.0.0"))));
    let proj = project();
    let out = run_q2_add(proj.path(), &["--no-prompt", &server.url()]);
    assert_failed(&out, "--trust");
    assert!(
        !stderr_str(&out).contains("--dry-run"),
        "the hint is for a real run, not a preview:\n{}",
        stderr_str(&out)
    );
    assert!(
        server.requests.lock().unwrap().is_empty(),
        "nothing may be fetched"
    );
    assert!(!proj.path().join("_extensions").exists());
}

#[test]
fn a_remote_dry_run_without_trust_is_refused_before_any_request() {
    // A dry run still downloads the archive to compute the table, so
    // the fetch needs consent in this mode too.
    let server = TestServer::start(tar_gz("repo-main/", &as_refs(&demo_files("1.0.0"))));
    let proj = project();
    let out = run_q2_add(proj.path(), &["--no-prompt", "--dry-run", &server.url()]);
    assert_failed(&out, "--dry-run --trust");
    assert!(
        server.requests.lock().unwrap().is_empty(),
        "nothing may be fetched"
    );
    assert!(!proj.path().join("_extensions").exists());
}

#[test]
fn a_trusted_remote_dry_run_shows_the_table_and_writes_nothing() {
    let server = TestServer::start(tar_gz("repo-main/", &as_refs(&demo_files("1.0.0"))));
    let proj = project();
    let out = run_q2_add(
        proj.path(),
        &["--no-prompt", "--dry-run", "--trust", &server.url()],
    );
    assert_succeeded(&out);
    let stdout = stdout_str(&out);
    assert!(
        stdout.contains("[Install]") && stdout.contains("nothing was written"),
        "{stdout}"
    );
    assert!(!proj.path().join("_extensions").exists());
    assert_eq!(
        server.requests.lock().unwrap().as_slice(),
        ["/archive.tar.gz"]
    );
}

#[test]
fn a_trusted_archive_url_installs() {
    let server = TestServer::start(tar_gz("repo-main/", &as_refs(&demo_files("1.0.0"))));
    let proj = project();
    let out = run_q2_add(proj.path(), &["--trust", &server.url()]);
    assert_succeeded(&out);
    let installed = proj.path().join("_extensions/demo");
    assert_eq!(read(&installed.join("demo.lua")), DEMO_LUA);
    let rec = record(&installed);
    assert_eq!(rec["kind"].as_str(), Some("url"));
    assert_eq!(rec["source"].as_str(), Some(server.url().as_str()));
    assert_eq!(
        server.requests.lock().unwrap().as_slice(),
        ["/archive.tar.gz"]
    );
}

// ====================================================================
// It works: an installed custom project type renders
// ====================================================================

#[test]
fn an_installed_custom_project_type_renders() {
    let src = tree(&[(
        "_extensions/fancysite/_extension.yml",
        "title: Fancy Site\ncontributes:\n  project:\n    project:\n      type: website\n    \
         website:\n      title: \"From The Extension\"\n",
    )]);
    let proj = tree(&[
        ("_quarto.yml", "project:\n  type: fancysite\n"),
        ("index.qmd", "---\ntitle: Home\n---\n\nHello.\n"),
    ]);
    assert_succeeded(&run_q2_add(proj.path(), &[&path_arg(src.path())]));

    let out = Command::new(Q2_BIN)
        .current_dir(proj.path())
        .arg("render")
        .env("CI", "1")
        .stdin(Stdio::null())
        .output()
        .expect("spawn q2 render");
    assert_succeeded(&out);
    let html: PathBuf = proj.path().join("_site").join("index.html");
    assert!(
        read(&html).contains("From The Extension"),
        "the extension's website title should reach the page"
    );
}
