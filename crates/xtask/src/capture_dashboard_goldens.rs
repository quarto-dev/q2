//! `cargo xtask capture-dashboard-goldens` — dev-only capture of Quarto 1
//! dashboard skeletons for the dashboard compatibility corpus.
//!
//! Renders every fixture in the corpus manifest that has `q1_golden = true`
//! with a real Quarto 1 `quarto` (`quarto render <file> --to dashboard`),
//! extracts the dashboard skeleton with
//! `quarto_output_extract::dashboard::dashboard_skeleton`, and writes it to
//! `goldens/<name>.q1.skeleton`. The assertion half
//! (`crates/quarto-core/tests/integration/dashboard_goldens.rs`) extracts Q2's
//! output with the same function and compares.
//!
//! Unlike `capture-pandoc-goldens`, this does not insist on a pinned release:
//! dashboard behavior is stable across recent Q1 versions, and the goldens'
//! provenance (version string, binary path, and `git describe` when the
//! binary lives in a quarto-cli checkout) is written to
//! `goldens/q1-provenance.txt` on every capture. Many fixtures run R or Python;
//! a fixture that fails to render (missing package, Q1 bug) is reported and
//! skipped, never fatal, so one missing package does not block the rest.
//!
//! Never invoked from `cargo xtask verify` or CI (see `mod tests`).
//!
//! Plan: `claude-notes/plans/2026-11-03-dashboard-format-port.md`, Phase 0.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::switch_task::current_worktree_root;
use quarto_output_extract::dashboard::dashboard_skeleton;
use quarto_output_extract::dashboard_corpus::{CORPUS_DIR_RELATIVE, Manifest, q1_golden_path};

/// Corpus subdirectories staged wholesale into the capture work dir, so each
/// fixture sees its sibling resources (images, CSVs) at the same relative paths.
const STAGED_DIRS: &[&str] = &["acceptance", "contract", "q1", "q1-atx", "regressions"];

pub struct Options {
    /// The Quarto 1 binary (default: `quarto` on `PATH`).
    pub quarto: String,
    /// Only capture fixtures whose path contains this substring.
    pub only: Option<String>,
}

fn quarto_version(quarto: &str) -> Result<String> {
    let output = Command::new(quarto)
        .arg("--version")
        .output()
        .with_context(|| {
            format!(
                "could not run `{quarto} --version` \u{2014} capture-dashboard-goldens needs a \
             Quarto 1 `quarto` binary (pass --quarto <path> or put it on PATH)"
            )
        })?;
    if !output.status.success() {
        bail!("`{quarto} --version` exited non-zero");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Version string, resolved binary path, and `git describe` + commit when the
/// binary lives inside a quarto-cli checkout (a dev build reports `99.9.9`).
fn provenance(quarto: &str) -> Result<String> {
    let version = quarto_version(quarto)?;
    let resolved = which(quarto)
        .and_then(|p| std::fs::canonicalize(&p).ok())
        .map_or_else(|| quarto.to_string(), |p| p.display().to_string());
    let mut text = format!("quarto --version: {version}\nbinary: {resolved}\n");
    if let Some(dir) = Path::new(&resolved).parent() {
        let describe = Command::new("git")
            .args(["describe", "--tags", "--always", "--dirty"])
            .current_dir(dir)
            .output();
        let commit = Command::new("git")
            .args(["log", "-1", "--format=%H %cI"])
            .current_dir(dir)
            .output();
        if let (Ok(d), Ok(c)) = (describe, commit)
            && d.status.success()
            && c.status.success()
        {
            text.push_str(&format!(
                "git describe: {}\ncommit: {}\n",
                String::from_utf8_lossy(&d.stdout).trim(),
                String::from_utf8_lossy(&c.stdout).trim()
            ));
        }
    }
    Ok(text)
}

fn which(bin: &str) -> Option<PathBuf> {
    if bin.contains(std::path::MAIN_SEPARATOR) {
        return Some(PathBuf::from(bin));
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(bin))
            .find(|p| p.is_file())
    })
}

fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry?;
        let rel = entry.path().strip_prefix(src)?;
        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target)?;
        } else {
            std::fs::copy(entry.path(), &target)
                .with_context(|| format!("copying {}", entry.path().display()))?;
        }
    }
    Ok(())
}

/// Remove ANSI SGR color sequences (`ESC [ ... m`) from Q1's stderr.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            for d in chars.by_ref() {
                if d.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Render one staged fixture with Q1 and return the output HTML.
fn render(quarto: &str, staged: &Path) -> Result<String> {
    let dir = staged.parent().context("fixture has a parent dir")?;
    let file = staged.file_name().context("fixture has a file name")?;
    let output = Command::new(quarto)
        .current_dir(dir)
        .arg("render")
        .arg(file)
        .arg("--to")
        .arg("dashboard")
        .output()
        .with_context(|| format!("spawning `{quarto} render`"))?;
    if !output.status.success() {
        let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));
        let first = stderr
            .lines()
            .find(|l| l.contains("ERROR") || l.contains("FATAL") || l.contains("Error"))
            .unwrap_or("(no error line)");
        bail!("exit {:?}: {}", output.status.code(), first.trim());
    }
    let html = staged.with_extension("html");
    std::fs::read_to_string(&html).with_context(|| format!("reading {}", html.display()))
}

pub fn run(options: &Options) -> Result<()> {
    let root = current_worktree_root()?;
    let corpus = root.join(CORPUS_DIR_RELATIVE);
    let manifest = Manifest::load(&corpus).map_err(anyhow::Error::msg)?;
    let prov = provenance(&options.quarto)?;
    println!("Capturing with:\n{prov}");

    let work = tempfile::tempdir().context("creating capture work dir")?;
    for dir in STAGED_DIRS {
        copy_dir(&corpus.join(dir), &work.path().join(dir))?;
    }

    let mut written = 0usize;
    let mut failures = Vec::new();
    for fixture in manifest.fixtures.iter().filter(|f| f.q1_golden) {
        if let Some(only) = &options.only
            && !fixture.path.contains(only.as_str())
        {
            continue;
        }
        print!("{} ... ", fixture.path);
        match render(&options.quarto, &work.path().join(&fixture.path)) {
            Ok(html) => {
                let golden = corpus.join(q1_golden_path(&fixture.path));
                std::fs::create_dir_all(golden.parent().expect("golden has a parent"))?;
                std::fs::write(&golden, dashboard_skeleton(&html))?;
                written += 1;
                println!("ok");
            }
            Err(e) => {
                println!("FAILED");
                failures.push(format!("{}: {e:#}", fixture.path));
            }
        }
    }
    std::fs::write(corpus.join("goldens").join("q1-provenance.txt"), &prov)?;

    for fixture in manifest.fixtures.iter().filter(|f| !f.q1_golden) {
        let stale = corpus.join(q1_golden_path(&fixture.path));
        if stale.exists() {
            println!(
                "warning: {} has q1_golden = false but {} exists",
                fixture.path,
                stale.display()
            );
        }
    }

    println!("Wrote {written} goldens.");
    if !failures.is_empty() {
        eprintln!(
            "{} fixture(s) failed to render with Q1. Install what they need, or set \
             `q1_golden = false` with a note in manifest.toml:",
            failures.len()
        );
        for f in &failures {
            eprintln!("  {f}");
        }
        bail!("{} capture failure(s)", failures.len());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_ansi_removes_color_codes() {
        assert_eq!(strip_ansi("\u{1b}[31mError\u{1b}[39m here"), "Error here");
        assert_eq!(strip_ansi("plain"), "plain");
    }

    #[test]
    fn missing_quarto_fails_naming_the_binary() {
        let err = quarto_version("/nonexistent/quarto-binary").unwrap_err();
        assert!(
            err.to_string().contains("/nonexistent/quarto-binary"),
            "{err}"
        );
    }

    #[test]
    fn corpus_manifest_loads_and_every_q1_golden_fixture_is_in_a_staged_dir() {
        let root = current_worktree_root().unwrap();
        let manifest = Manifest::load(&root.join(CORPUS_DIR_RELATIVE)).unwrap();
        for f in &manifest.fixtures {
            assert!(
                STAGED_DIRS
                    .iter()
                    .any(|d| f.path.starts_with(&format!("{d}/"))),
                "{} is outside the staged dirs {STAGED_DIRS:?}",
                f.path
            );
        }
    }

    #[test]
    fn capture_not_wired_into_verify_or_build_all() {
        let root = current_worktree_root().unwrap();
        for rel in [
            "crates/xtask/src/verify.rs",
            "crates/xtask/src/build_all.rs",
        ] {
            let text = std::fs::read_to_string(root.join(rel)).unwrap();
            assert!(
                !text.contains("capture_dashboard_goldens")
                    && !text.contains("capture-dashboard-goldens"),
                "{rel} must not invoke capture-dashboard-goldens \u{2014} it is dev-only"
            );
        }
    }
}
