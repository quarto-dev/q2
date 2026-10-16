/*
 * output_dir_images_e2e.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * q2-preview hub-parity Plan 2: `q2 render --output-dir` keeps images
 * for an absolute and a relative directory (the relative one used to be
 * re-interpreted against the project dir), for docx and typst.
 */

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

/// 1x1 PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0xfc, 0xcf, 0xc0, 0x50,
    0x0f, 0x00, 0x04, 0x85, 0x01, 0x80, 0x84, 0xa9, 0x8c, 0x21, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

fn have(bin: &str) -> bool {
    Command::new(bin)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// `<work>/proj` with `sub/a.qmd` + `sub/pic.png`; returns `<work>`.
fn fixture() -> (TempDir, PathBuf) {
    let temp = TempDir::new().unwrap();
    let work = temp.path().canonicalize().unwrap();
    write(&work.join("proj/_quarto.yml"), b"project:\n  title: x\n");
    write(
        &work.join("proj/sub/a.qmd"),
        b"---\ntitle: T\n---\n\n![](pic.png)\n",
    );
    write(&work.join("proj/sub/pic.png"), PNG);
    (temp, work)
}

fn render(work: &Path, format: &str, output_dir: &str) -> String {
    let output = Command::new(Q2_BIN)
        .current_dir(work)
        .args([
            "render",
            "proj/sub/a.qmd",
            "--to",
            format,
            "--output-dir",
            output_dir,
        ])
        .output()
        .expect("spawn q2 binary");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "render failed:\n{text}");
    assert!(
        !text.contains("Q-5-6") && !text.contains("Q-21-3"),
        "unexpected resource diagnostic:\n{text}"
    );
    text
}

fn docx_has_image(path: &Path) -> bool {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).unwrap();
        if entry.name().unwrap().contains("media/") {
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf).unwrap();
            if buf == PNG {
                return true;
            }
        }
    }
    false
}

#[test]
fn docx_keeps_images_for_absolute_and_relative_output_dir() {
    if !have("pandoc") {
        eprintln!("SKIP: `pandoc` not found on PATH");
        return;
    }
    let (_temp, work) = fixture();
    let abs = work.join("out-abs");
    render(&work, "docx", abs.to_str().unwrap());
    assert!(docx_has_image(&abs.join("a.docx")), "absolute --output-dir");

    render(&work, "docx", "out-rel");
    assert!(
        docx_has_image(&work.join("out-rel/a.docx")),
        "relative --output-dir (resolved against the cwd)"
    );
}

#[test]
fn typst_builds_with_relative_output_dir_inside_the_project() {
    if !have("pandoc") || !have("typst") {
        eprintln!("SKIP: `pandoc` and `typst` are both required");
        return;
    }
    let (_temp, work) = fixture();
    render(&work, "typst", "proj/.quarto/preview-out/r");
    assert!(work.join("proj/.quarto/preview-out/r/a.pdf").is_file());
}
