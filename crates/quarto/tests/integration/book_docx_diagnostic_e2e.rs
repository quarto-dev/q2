/*
 * book_docx_diagnostic_e2e.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * book-projects P3: `q2 render <book> --to docx` must fail with the
 * explicit Q-5-33 unsupported-format diagnostic — naming the unsupported
 * combination — not a silent per-chapter render, a panic, or a success
 * exit. Library-level equivalent: quarto-core's `book_project_type.rs::
 * unsupported_format_errors_q_5_33_and_still_shuts_down_once`; this drives
 * the real binary end-to-end.
 */

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn canonical(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

fn write_book_fixture(project_dir: &Path) {
    write_file(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: book\n\nbook:\n  title: \"Docx Probe\"\n  chapters:\n    - index.qmd\n    - ch1.qmd\n",
    );
    write_file(&project_dir.join("index.qmd"), "# Home\n\nWelcome.\n");
    write_file(&project_dir.join("ch1.qmd"), "# One\n\nChapter one.\n");
}

/// A book project targeting docx exits non-zero with the Q-5-33
/// diagnostic naming the unsupported combination — no `.docx` output is
/// produced and the failure is a diagnostic, not a panic.
#[test]
fn e2e_book_to_docx_fails_with_q_5_33_diagnostic() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write_book_fixture(&project_dir);

    let output = Command::new(Q2_BIN)
        .current_dir(&project_dir)
        .args(["render", ".", "--to", "docx"])
        .output()
        .expect("spawn q2 binary");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");

    assert!(
        !output.status.success(),
        "rendering a book to docx must fail, got success. stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        !combined.contains("panicked"),
        "the unsupported combination is a diagnostic, not a panic:\n{combined}"
    );
    assert!(
        combined.contains("Q-5-33"),
        "diagnostic must carry the Q-5-33 code:\n{combined}"
    );
    assert!(
        combined.contains("`docx`"),
        "diagnostic must name the unsupported target format:\n{combined}"
    );

    let docx_files: Vec<_> = std::fs::read_dir(&project_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".docx"))
        .collect();
    assert!(
        docx_files.is_empty(),
        "no docx output may be produced for an unsupported book target: {docx_files:?}"
    );
}
