/*
 * tests/integration/julia_targets_cli.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * OPR-9e: a Julia document rendered to each graduated non-HTML target through the real binary.
 */

//! LOCAL EVIDENCE ONLY: CI has no Julia, so each test skips there (and wherever Julia, its
//! environment or Node is missing). Each graduated class (gfm, typst, docx, pptx, epub) is
//! rendered twice, once plain and once with `execute: {record: true}` (the route the presenter
//! owns), from the parity corpus document `julia/targets`. Both must succeed and write the figure
//! into `doc_files/figure-<format>`, and the two routes must agree on the figure files.

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

const DOC: &str =
    include_str!("../../../quarto-core/tests/fixtures/opr-parity/julia/targets/doc.qmd");

/// Julia on `PATH` and an environment with a `Manifest.toml` (`QUARTO_JULIA_PROJECT`, else the
/// engine's runtime directory), and `node` for the engine host. A copy of `julia_available` in
/// `quarto-core`'s tests, which this crate cannot reach.
fn julia_available() -> bool {
    let ok = |program: &str| {
        Command::new(program)
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    };
    if !ok("julia") || !ok("node") {
        return false;
    }
    std::env::var_os("QUARTO_JULIA_PROJECT")
        .map(PathBuf::from)
        .or_else(|| {
            quarto_util::quarto_runtime_dir()
                .ok()
                .map(|d| d.join("julia"))
        })
        .is_some_and(|dir| dir.join("Manifest.toml").exists())
}

/// Render `DOC` to `format`; returns the directory (kept alive by the guard).
fn render(format: &str, record: bool) -> TempDir {
    let dir = TempDir::new().unwrap();
    let doc = if record {
        DOC.replacen(
            "engine: julia\n",
            "engine: julia\nexecute:\n  record: true\n",
            1,
        )
    } else {
        DOC.to_string()
    };
    std::fs::write(dir.path().join("doc.qmd"), doc).unwrap();
    let out = Command::new(Q2_BIN)
        .current_dir(dir.path())
        .args(["render", "doc.qmd", "--to", format])
        .env_remove("QUARTO_PROFILE")
        .output()
        .expect("q2 runs");
    assert!(
        out.status.success(),
        "q2 render --to {format} (record: {record}) failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    dir
}

fn figure_files(dir: &Path, format: &str) -> Vec<String> {
    let figures = dir.join("doc_files").join(format!("figure-{format}"));
    let mut names: Vec<String> = std::fs::read_dir(&figures)
        .unwrap_or_else(|e| panic!("no {}: {e}", figures.display()))
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Media entry names of a zip output (docx, pptx, epub).
fn media_entries(file: &Path) -> Vec<String> {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(file).unwrap()).unwrap();
    (0..archive.len())
        .map(|i| String::from_utf8_lossy(archive.by_index(i).unwrap().name_raw()).into_owned())
        .filter(|n| n.contains("media/"))
        .collect()
}

fn check(format: &str, output: &str) {
    if !julia_available() {
        eprintln!("skipping {format}: Julia (with an environment) or Node is not available");
        return;
    }
    let plain = render(format, false);
    let recorded = render(format, true);
    for dir in [&plain, &recorded] {
        assert!(dir.path().join(output).exists(), "no {output} for {format}");
    }
    let figures = figure_files(plain.path(), format);
    assert!(!figures.is_empty(), "{format}: no figure written");
    assert_eq!(
        figures,
        figure_files(recorded.path(), format),
        "{format}: the two routes write different figure files"
    );
    if matches!(format, "docx" | "pptx" | "epub") {
        let media = media_entries(&plain.path().join(output));
        assert!(!media.is_empty(), "{format}: no media in the archive");
        assert_eq!(
            media.len(),
            media_entries(&recorded.path().join(output)).len(),
            "{format}: the routes embed a different number of media files"
        );
    }
}

#[test]
fn gfm_plain_and_recorded_agree() {
    check("gfm", "doc.md");
    if julia_available() {
        // The echo-false document has no interleaved cell, so the two pages are the same text.
        let plain = std::fs::read_to_string(render("gfm", false).path().join("doc.md")).unwrap();
        let recorded = std::fs::read_to_string(render("gfm", true).path().join("doc.md")).unwrap();
        assert!(plain.contains("hello"), "{plain}");
        assert_eq!(plain, recorded);
    }
}

#[test]
fn typst_plain_and_recorded_agree() {
    check("typst", "doc.pdf");
}

#[test]
fn docx_plain_and_recorded_agree() {
    check("docx", "doc.docx");
}

#[test]
fn pptx_plain_and_recorded_agree() {
    check("pptx", "doc.pptx");
}

#[test]
fn epub_plain_and_recorded_agree() {
    check("epub", "doc.epub");
}
