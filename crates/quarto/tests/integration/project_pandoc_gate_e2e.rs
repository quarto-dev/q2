/*
 * project_pandoc_gate_e2e.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * P7-foundation Task 2 — end-to-end CLI tests for the project-mode
 * containment gate (design doc §13, bd-bgeet2mw).
 */

//! `T2.4`/`T2.5` from the P7-foundation implementation companion.
//! These exercise the gate through the real `q2` binary rather than
//! in-process, so they only pass once P7-foundation Task 3 relaxes
//! `render.rs`'s format gate to admit `docx` — before that, `--to
//! docx` fails at the CLI refusal instead of reaching the gate.

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

fn run_q2(cwd: &Path, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(Q2_BIN);
    cmd.current_dir(cwd);
    cmd.arg("render");
    for a in args {
        cmd.arg(a);
    }
    cmd.output().expect("spawn q2 binary")
}

/// A minimal website project with `site-url` set (so `sitemap.xml`
/// would fire if the gate did not stop it for a Pandoc target).
fn write_minimal_website(project_dir: &Path) {
    write_file(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n\
         website:\n  site-url: \"https://example.com\"\n",
    );
    write_file(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\naliases:\n  - /old-name.html\n---\n\nHome body.\n",
    );
}

/// T2.4: `q2 render . --to docx` on a website project exits 0, writes
/// a non-empty `_site/index.docx`, and writes no `_site/sitemap.xml`.
#[test]
fn e2e_docx_project_no_sitemap() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write_minimal_website(&project_dir);

    let output = run_q2(&project_dir, &[".", "--to", "docx"]);
    assert!(
        output.status.success(),
        "q2 render --to docx should succeed; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let docx = project_dir.join("_site/index.docx");
    let len = std::fs::metadata(&docx)
        .unwrap_or_else(|e| panic!("expected {}: {}", docx.display(), e))
        .len();
    assert!(len > 0, "docx output must be non-empty");

    assert!(
        !project_dir.join("_site/sitemap.xml").exists(),
        "docx render must not write sitemap.xml"
    );
}

/// T2.5 ("the path was actually exercised" row, extended to
/// `revealjs` per the revert-hunk note — the `html` sub-case is
/// already covered at the `I`-tier by T2.2): every native, HTML-based
/// format still writes `sitemap.xml`. A gate that widened to also
/// exclude `revealjs` (e.g. `is_native() && identifier == Html`)
/// would pass T2.4 but redden here.
#[test]
fn e2e_html_project_sitemap_present() {
    for to in ["html", "revealjs"] {
        let temp = TempDir::new().unwrap();
        let project_dir = canonical(temp.path());
        write_minimal_website(&project_dir);

        let output = run_q2(&project_dir, &[".", "--to", to]);
        assert!(
            output.status.success(),
            "q2 render --to {to} should succeed; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        assert!(
            project_dir.join("_site/sitemap.xml").exists(),
            "sitemap.xml missing for --to {to}"
        );
    }
}
