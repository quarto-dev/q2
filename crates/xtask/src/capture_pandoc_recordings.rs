//! `cargo xtask capture-pandoc-recordings` — dev-only capture of the exact
//! inputs of real native pandoc runs (pandoc.wasm epic, phase R0).
//!
//! Builds `q2`, renders every fixture in [`FIXTURES`] to every format in
//! [`FORMATS`] with `QUARTO_PANDOC` pointed at `scripts/pandoc-capture.sh`
//! (which logs argv, env, the input JSON, the share tree and every file the
//! argv names, then execs the real pandoc), and normalizes each raw capture
//! into a committed recording with `quarto-pandoc-recording`. Unix only (the
//! wrapper is a POSIX shell script). Never invoked from `cargo xtask verify`
//! or CI.
//!
//! Recordings land in `crates/quarto-core/tests/fixtures/pandoc-recordings/recordings/`;
//! consumers (R1, R4, R5, H0, H1, H3) all read that directory.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::switch_task::current_worktree_root;

/// Fixed so captures are reproducible; the production host pins it at click time.
pub const SOURCE_DATE_EPOCH: &str = "1700000000";

const FIXTURES_ROOT: &str = "crates/quarto-core/tests/fixtures";
const RECORDINGS_DIR: &str = "crates/quarto-core/tests/fixtures/pandoc-recordings/recordings";
const CONSTANTS: &str = "resources/pandoc-wasm.json";

pub const FORMATS: &[&str] = &["docx", "pptx", "epub", "typst"];

/// A fixture: a recording name, the directory (relative to
/// `crates/quarto-core/tests/fixtures`) its files live under, its `.qmd`, and
/// the resources it references. Layout under that directory is preserved.
pub struct Fixture {
    pub name: &'static str,
    pub root: &'static str,
    pub qmd: &'static str,
    pub resources: &'static [&'static str],
}

pub const FIXTURES: &[Fixture] = &[
    Fixture {
        name: "callouts",
        root: "pandoc-goldens",
        qmd: "callouts.qmd",
        resources: &[],
    },
    Fixture {
        name: "crossrefs",
        root: "pandoc-goldens",
        qmd: "crossrefs/all-docx.qmd",
        resources: &["crossrefs/img/thinker.jpg"],
    },
    Fixture {
        name: "citations",
        root: "pandoc-recordings/sources/citations",
        qmd: "citations.qmd",
        resources: &["refs.bib"],
    },
    Fixture {
        name: "tables",
        root: "pandoc-recordings/sources/tables",
        qmd: "tables.qmd",
        resources: &[],
    },
    Fixture {
        name: "shortcodes",
        root: "pandoc-recordings/sources/shortcodes",
        qmd: "shortcodes.qmd",
        resources: &[],
    },
    Fixture {
        name: "images",
        root: "pandoc-recordings/sources/images",
        qmd: "images.qmd",
        resources: &["img/square.png"],
    },
];

pub fn recording_name(fixture: &Fixture, format: &str) -> String {
    format!("{}-{format}", fixture.name)
}

/// The pandoc version pinned by `resources/pandoc-wasm.json`'s asset name
/// (`pandoc-<version>.wasm.zip`).
fn pinned_version(worktree_root: &Path) -> Result<String> {
    let text = std::fs::read_to_string(worktree_root.join(CONSTANTS))?;
    let json: serde_json::Value = serde_json::from_str(&text)?;
    let asset = json["asset_name"].as_str().context("asset_name")?;
    asset
        .strip_prefix("pandoc-")
        .and_then(|s| s.strip_suffix(".wasm.zip"))
        .map(str::to_string)
        .with_context(|| format!("unexpected asset_name {asset}"))
}

fn check_pandoc(pinned: &str) -> Result<PathBuf> {
    let out = Command::new("pandoc").arg("--version").output().context(
        "could not run `pandoc --version`; capture needs the pinned native pandoc on PATH",
    )?;
    let first = String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .to_string();
    if first.trim() != format!("pandoc {pinned}") {
        bail!("found `{first}`, but recordings are captured with exactly pandoc {pinned}");
    }
    let path = std::env::var_os("PATH")
        .and_then(|p| {
            std::env::split_paths(&p)
                .map(|d| d.join("pandoc"))
                .find(|c| c.is_file())
        })
        .context("pandoc not found on PATH")?;
    Ok(path)
}

fn stage(fixture: &Fixture, work: &Path) -> Result<PathBuf> {
    let src_root = Path::new(FIXTURES_ROOT).join(fixture.root);
    for rel in std::iter::once(&fixture.qmd).chain(fixture.resources.iter()) {
        let dst = work.join(rel);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(src_root.join(rel), &dst)
            .with_context(|| format!("staging {rel} for {}", fixture.name))?;
    }
    Ok(work.join(fixture.qmd))
}

pub fn run() -> Result<()> {
    #[cfg(not(unix))]
    {
        bail!(
            "capture-pandoc-recordings is Unix-only (the capture wrapper is a POSIX shell script)"
        );
    }
    #[cfg(unix)]
    {
        let root = current_worktree_root()?;
        std::env::set_current_dir(&root)?;
        let pandoc = check_pandoc(&pinned_version(&root)?)?;

        let status = Command::new("cargo")
            .args(["build", "--bin", "q2"])
            .status()?;
        if !status.success() {
            bail!("cargo build --bin q2 failed");
        }
        let q2 = root.join("target/debug/q2");
        let wrapper = root.join("scripts/pandoc-capture.sh");
        let recordings = root.join(RECORDINGS_DIR);
        let rewrite_pandoc = pandoc.clone();

        let scratch = tempfile::tempdir()?;
        for fixture in FIXTURES {
            for format in FORMATS {
                let name = recording_name(fixture, format);
                println!("Capturing {name}");
                let work = scratch.path().join("docs").join(&name);
                let qmd = stage(fixture, &work)?;
                let cap = scratch.path().join("cap").join(&name);
                let status = Command::new(&q2)
                    .current_dir(qmd.parent().expect("staged qmd has a parent"))
                    .arg("render")
                    .arg(qmd.file_name().expect("file name"))
                    .args(["--to", format])
                    .env("QUARTO_PANDOC", &wrapper)
                    .env("PANDOC_CAPTURE_DIR", &cap)
                    .env("PANDOC_CAPTURE_REAL", &pandoc)
                    .env("SOURCE_DATE_EPOCH", SOURCE_DATE_EPOCH)
                    .status()
                    .with_context(|| format!("spawning q2 render for {name}"))?;
                if !status.success() {
                    bail!("`q2 render {} --to {format}` failed", fixture.qmd);
                }
                quarto_pandoc_recording::rewrite::rewrite_run(
                    &cap.join("run-1"),
                    &recordings,
                    &name,
                    Some(&rewrite_pandoc),
                )
                .with_context(|| format!("rewriting {name}"))?;
            }
        }
        println!("Wrote recordings to {}", recordings.display());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_fixture_file_exists() {
        let root = current_worktree_root().expect("worktree root");
        for fixture in FIXTURES {
            let base = root.join(FIXTURES_ROOT).join(fixture.root);
            for rel in std::iter::once(&fixture.qmd).chain(fixture.resources.iter()) {
                assert!(base.join(rel).exists(), "{} missing {rel}", fixture.name);
            }
        }
    }

    #[test]
    fn recording_names_are_unique() {
        let mut names: Vec<_> = FIXTURES
            .iter()
            .flat_map(|f| FORMATS.iter().map(move |fmt| recording_name(f, fmt)))
            .collect();
        let n = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), n);
    }

    /// Like the goldens capture, this is a dev command: nothing automated runs it.
    #[test]
    fn verify_does_not_invoke_the_capture() {
        let verify = include_str!("verify.rs");
        assert!(!verify.contains("capture_pandoc_recordings"));
    }
}
