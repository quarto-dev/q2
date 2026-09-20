//! `cargo xtask capture-pandoc-goldens` — dev-only `G`-tier capture of
//! docx/pptx golden fixtures from a real, pinned-release `quarto`.
//!
//! Locates a real `quarto` at exactly [`QUARTO_PINNED_VERSION`], renders
//! every fixture in [`FIXTURES`] to `--to docx` and `--to pptx`, extracts
//! each output with `quarto_ooxml_extract`, and writes the result as a
//! committed insta snapshot under [`snapshot_dir`]. `FIXTURES` and
//! [`golden_snapshot_name`] are re-exported from
//! `quarto_ooxml_extract::golden_fixtures` — the leaf crate this xtask and
//! `quarto-core` already both depend on — rather than defined here, so the
//! CI-runnable assertion half (Q2's own hybrid output against these same
//! snapshots, `crates/quarto-core/tests/integration/pandoc_goldens.rs`, P7
//! Task 11) calls the **same** function and iterates the **same** manifest
//! instead of an independently-derived copy of either.
//!
//! Never invoked from `cargo xtask verify` or CI (see `mod tests`' guard
//! against that).
//!
//! See `claude-notes/plans/2026-09-18-pandoc-hybrid-P7-implementation.md`,
//! Task 10, for the full spec this module implements.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::switch_task::current_worktree_root;
pub use quarto_ooxml_extract::{FIXTURES, FixtureEntry, golden_snapshot_name};

/// The pinned Q1 release tag goldens are captured against. Bump only on a
/// deliberate re-vendor (see the plan's "re-capture policy").
pub const QUARTO_PINNED_VERSION: &str = "1.11.3";

const FIXTURES_DIR_RELATIVE: &str = "crates/quarto-core/tests/fixtures/pandoc-goldens";
const SNAPSHOT_DIR_RELATIVE: &str = "crates/quarto-core/tests/integration/snapshots";

/// The absolute directory `.snap` files are written to / read from.
pub fn snapshot_dir(worktree_root: &Path) -> PathBuf {
    worktree_root.join(SNAPSHOT_DIR_RELATIVE)
}

fn fixtures_root(worktree_root: &Path) -> PathBuf {
    worktree_root.join(FIXTURES_DIR_RELATIVE)
}

/// Run `quarto --version` (optionally under an overridden `PATH`, for
/// tests) and return its trimmed stdout. Spawn failure (binary absent)
/// surfaces as `Err` naming the binary and the expected pinned tag —
/// **never** a skip.
fn quarto_version(path_env: Option<&str>) -> Result<String> {
    let mut cmd = Command::new("quarto");
    cmd.arg("--version");
    if let Some(path) = path_env {
        cmd.env("PATH", path);
    }
    let output = cmd.output().with_context(|| {
        format!(
            "could not run `quarto --version` \u{2014} \
             cargo xtask capture-pandoc-goldens needs a real, pinned-release `quarto` \
             (v{QUARTO_PINNED_VERSION}) on PATH. Download it from \
             https://github.com/quarto-dev/quarto-cli/releases/tag/v{QUARTO_PINNED_VERSION}"
        )
    })?;
    if !output.status.success() {
        bail!(
            "`quarto --version` exited non-zero \u{2014} \
             cargo xtask capture-pandoc-goldens needs a real, pinned-release `quarto` \
             (v{QUARTO_PINNED_VERSION}) on PATH."
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// T10.1 + T10.2: the binary-location and version preconditions, as one
/// function so both are gated the same way. Returns the detected version
/// string on success.
fn check_quarto_precondition(path_env: Option<&str>) -> Result<String> {
    let detected = quarto_version(path_env)?;
    if detected != QUARTO_PINNED_VERSION {
        bail!(
            "found quarto version `{detected}`, but cargo xtask capture-pandoc-goldens needs \
             exactly the pinned release `{QUARTO_PINNED_VERSION}`. Download the matching \
             release from \
             https://github.com/quarto-dev/quarto-cli/releases/tag/v{QUARTO_PINNED_VERSION}"
        );
    }
    Ok(detected)
}

/// T10.4: every manifest entry's `.qmd` and declared resources exist under
/// the fixtures root, and the manifest has exactly 10 entries.
fn check_fixture_manifest_complete(worktree_root: &Path) -> Result<()> {
    if FIXTURES.len() != 10 {
        bail!(
            "fixture manifest has {} entries, expected exactly 10",
            FIXTURES.len()
        );
    }
    let root = fixtures_root(worktree_root);
    for fixture in FIXTURES {
        let qmd_path = root.join(fixture.qmd);
        if !qmd_path.exists() {
            bail!(
                "fixture manifest names {} but it does not exist at {}. Copy it in per the External Sources Policy (see the fixtures README).",
                fixture.qmd,
                qmd_path.display()
            );
        }
        for resource in fixture.resources {
            let resource_path = root.join(resource);
            if !resource_path.exists() {
                bail!(
                    "fixture {} declares resource {resource} but it does not exist at {}. \
                     Copying the .qmd alone silently turns a figure into an unresolved image.",
                    fixture.qmd,
                    resource_path.display()
                );
            }
        }
    }
    Ok(())
}

/// Render one fixture to one format with the real `quarto`, returning the
/// rendered file's bytes.
fn render_fixture(
    quarto_bin: &str,
    fixtures_root: &Path,
    fixture: &FixtureEntry,
    format: &str,
    work_dir: &Path,
) -> Result<Vec<u8>> {
    let src_qmd = fixtures_root.join(fixture.qmd);
    let staged_qmd = work_dir.join(fixture.qmd);
    if let Some(parent) = staged_qmd.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(&src_qmd, &staged_qmd)
        .with_context(|| format!("copying {} into capture work dir", src_qmd.display()))?;
    for resource in fixture.resources {
        let src = fixtures_root.join(resource);
        let dst = work_dir.join(resource);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&src, &dst)
            .with_context(|| format!("copying resource {} into capture work dir", src.display()))?;
    }

    let ext = format;
    let qmd_file_name = staged_qmd
        .file_name()
        .expect("staged qmd path has a file name");
    let out_name = format!(
        "{}.{ext}",
        Path::new(fixture.qmd)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("output")
    );
    let staged_dir = staged_qmd
        .parent()
        .expect("staged qmd path has a parent")
        .to_path_buf();
    // `quarto render`'s `--output` resolves relative to the invoking
    // process's cwd, not the input file's directory (measured against a
    // real v1.11.3 build) -- an absolute path is explicitly rejected
    // ("--output option cannot specify a relative or absolute path"). So
    // both the input (a bare file name) and `--output` must be relative to
    // the same `current_dir`.
    let status = Command::new(quarto_bin)
        .current_dir(&staged_dir)
        .arg("render")
        .arg(qmd_file_name)
        .arg("--to")
        .arg(format)
        .arg("--output")
        .arg(&out_name)
        .status()
        .with_context(|| format!("spawning `quarto render` for {}", fixture.qmd))?;
    if !status.success() {
        bail!(
            "`quarto render {} --to {format}` failed (exit {:?})",
            fixture.qmd,
            status.code()
        );
    }

    let out_path = staged_dir.join(&out_name);
    std::fs::read(&out_path)
        .with_context(|| format!("reading rendered output at {}", out_path.display()))
}

/// Run the full capture: precondition checks, fixture manifest check,
/// render + extract + snapshot for every fixture/format pair.
pub fn run() -> Result<()> {
    let worktree_root = current_worktree_root()?;
    let detected = check_quarto_precondition(None)?;
    println!("Using quarto {detected} at the pinned release.");

    check_fixture_manifest_complete(&worktree_root)?;

    let fixtures_root_path = fixtures_root(&worktree_root);
    let snap_dir = snapshot_dir(&worktree_root);
    std::fs::create_dir_all(&snap_dir)
        .with_context(|| format!("creating snapshot dir {}", snap_dir.display()))?;

    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_path(&snap_dir);
    settings.set_prepend_module_to_snapshot(false);
    let _guard = settings.bind_to_scope();

    let work_dir = tempfile::tempdir().context("creating capture work dir")?;

    let mut count = 0usize;
    for fixture in FIXTURES {
        for format in ["docx", "pptx"] {
            println!("Capturing {} \u{2192} {format}", fixture.qmd);
            let bytes = render_fixture(
                "quarto",
                &fixtures_root_path,
                fixture,
                format,
                work_dir.path(),
            )?;
            let extraction = if format == "docx" {
                quarto_ooxml_extract::extract_docx(&bytes)
            } else {
                quarto_ooxml_extract::extract_pptx(&bytes)
            }
            .map_err(|e| anyhow::anyhow!("extracting {} ({format}): {e}", fixture.qmd))?;

            let name = golden_snapshot_name(fixture.qmd, format);
            insta::assert_snapshot!(name, extraction.to_string());
            count += 1;
        }
    }

    println!("Wrote {count} golden snapshots to {}", snap_dir.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- T10.1: missing binary fails loudly, never skips -----------------

    #[test]
    fn test_capture_fails_without_quarto() {
        // An empty PATH means `Command::new("quarto")` cannot find the
        // binary at all (spawn failure), which is the "no quarto on PATH"
        // case this row guards.
        let result = check_quarto_precondition(Some(""));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("quarto"),
            "error should name the binary: {msg}"
        );
        assert!(
            msg.contains(QUARTO_PINNED_VERSION),
            "error should name the expected tag: {msg}"
        );
    }

    // --- T10.2: wrong version fails loudly, naming both versions ---------

    #[cfg(unix)]
    fn write_stub_quarto(dir: &Path, reported_version: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("quarto");
        std::fs::write(
            &path,
            format!("#!/bin/sh\necho '{reported_version}'\nexit 0\n"),
        )
        .unwrap();
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();
        path
    }

    #[cfg(windows)]
    fn write_stub_quarto(dir: &Path, reported_version: &str) -> PathBuf {
        let path = dir.join("quarto.bat");
        std::fs::write(&path, format!("@echo {reported_version}\r\n")).unwrap();
        path
    }

    #[test]
    fn test_capture_fails_on_wrong_quarto_version() {
        let dir = tempfile::tempdir().unwrap();
        write_stub_quarto(dir.path(), "99.9.9");
        let path_env = dir.path().to_string_lossy().into_owned();

        let result = check_quarto_precondition(Some(&path_env));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("99.9.9"),
            "error should name the detected version: {msg}"
        );
        assert!(
            msg.contains(QUARTO_PINNED_VERSION),
            "error should name the expected version: {msg}"
        );
    }

    #[test]
    fn test_capture_accepts_pinned_quarto_version() {
        let dir = tempfile::tempdir().unwrap();
        write_stub_quarto(dir.path(), QUARTO_PINNED_VERSION);
        let path_env = dir.path().to_string_lossy().into_owned();

        let detected = check_quarto_precondition(Some(&path_env)).unwrap();
        assert_eq!(detected, QUARTO_PINNED_VERSION);
    }

    // --- T10.3: the shared naming function, hardcoded literals -----------
    // Covered by `quarto_ooxml_extract::golden_fixtures`'s own
    // `test_golden_snapshot_name_matches_hardcoded_literals` — this xtask
    // re-exports rather than re-derives, so the literal test lives with the
    // one definition.

    // --- T10.4: fixture manifest is complete ------------------------------

    #[test]
    fn test_fixture_manifest_complete() {
        assert_eq!(FIXTURES.len(), 10);
        let worktree_root = current_worktree_root().unwrap();
        check_fixture_manifest_complete(&worktree_root).unwrap();
    }

    // --- T10.7: capture is absent from verify/build-all -------------------

    #[test]
    fn test_capture_not_wired_into_verify_or_build_all() {
        let worktree_root = current_worktree_root().unwrap();
        for rel in [
            "crates/xtask/src/verify.rs",
            "crates/xtask/src/build_all.rs",
        ] {
            let path = worktree_root.join(rel);
            let source = std::fs::read_to_string(&path).unwrap();
            assert!(
                !source.contains("capture-pandoc-goldens")
                    && !source.contains("capture_pandoc_goldens"),
                "{rel} must not invoke capture-pandoc-goldens \u{2014} it is dev-only, never CI/verify"
            );
        }
    }
}
