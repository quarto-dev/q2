//! End-to-end CLI tests for coalescing repeated per-page diagnostics
//! (bd-mg3ckvp7).
//!
//! When a single underlying problem lives in a file shared by every
//! page — the motivating case is a broken navbar `href:` in
//! `_quarto.yml` — each per-document pipeline re-diagnoses it and the
//! render summary used to print one identical warning per page (186
//! copies on the connect-docs testbed). These tests pin the coalesced
//! behavior: one emission per distinct source span, with an
//! `Affected files:` tail listing the pages that re-reported it.
//!
//! Contract under verification (per
//! `claude-notes/plans/2026-07-31-repeated-diagnostics-coalescing.md`):
//! - A config-anchored warning (same `_quarto.yml` span reported by N
//!   pages) prints exactly once, with all N pages in the tail.
//! - A warning reported by a single page prints without any
//!   `Affected files:` tail (legacy shape preserved).
//! - Coalescing is print-only: exit codes are unchanged (warnings
//!   still exit 0 without `--strict`).
//!
//! TDD note: written before the implementation; the first test must
//! fail (3 copies, no tail) before Phase 2/3 of the plan start.

use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// Strip ANSI escape sequences (CSI color codes and OSC-8 hyperlinks)
/// so assertions can match the plain text of ariadne-rendered
/// snippets, which interleave color codes inside highlighted spans.
fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.next() {
            // CSI: `ESC [ ... <alpha>`
            Some('[') => {
                for n in chars.by_ref() {
                    if n.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            // OSC: `ESC ] ... (BEL | ESC \)`
            Some(']') => {
                while let Some(n) = chars.next() {
                    if n == '\u{7}' {
                        break;
                    }
                    if n == '\u{1b}' {
                        chars.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Run `q2 render .` from `cwd` and return (exit-success, stderr with
/// ANSI escapes stripped).
fn run_q2_render(cwd: &Path) -> (bool, String) {
    let output = Command::new(Q2_BIN)
        .current_dir(cwd)
        .args(["render", "."])
        .output()
        .expect("spawn q2 binary");
    (
        output.status.success(),
        strip_ansi(&String::from_utf8_lossy(&output.stderr)),
    )
}

/// A website project whose navbar references a document that is not
/// in the render set. Every page's navbar-render transform reports
/// the miss at the same `_quarto.yml` span.
fn write_broken_navbar_project(dir: &Path, pages: &[&str]) {
    write_file(
        &dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\nwebsite:\n  title: Coalesce\n  navbar:\n    left:\n      - href: missing.qmd\n        text: Missing\n",
    );
    for page in pages {
        write_file(
            &dir.join(format!("{page}.qmd")),
            &format!("---\ntitle: {page}\n---\n\nBody of {page}.\n"),
        );
    }
}

/// Three pages re-reporting the same `_quarto.yml` navbar miss must
/// produce exactly one Q-13-2 emission, tailed by all three pages.
#[test]
fn config_anchored_warning_coalesces_across_pages() {
    let dir = TempDir::new().unwrap();
    write_broken_navbar_project(dir.path(), &["index", "a", "b"]);

    let (success, stderr) = run_q2_render(dir.path());
    assert!(
        success,
        "warnings alone must not fail the render:\n{stderr}"
    );

    let q13_count = stderr.matches("Q-13-2").count();
    assert_eq!(
        q13_count, 1,
        "expected exactly one coalesced Q-13-2 emission, got {q13_count}:\n{stderr}"
    );

    let tail = stderr
        .lines()
        .find(|l| l.starts_with("Affected files:"))
        .unwrap_or_else(|| panic!("expected an `Affected files:` tail:\n{stderr}"));
    for page in ["index.qmd", "a.qmd", "b.qmd"] {
        assert!(tail.contains(page), "tail should name {page}; got: {tail}");
    }
}

/// More affected pages than the display cap: the tail lists the cap's
/// worth of names and summarizes the rest as `(and N other…)`.
#[test]
fn affected_files_tail_caps_long_lists() {
    let dir = TempDir::new().unwrap();
    write_broken_navbar_project(dir.path(), &["index", "a", "b", "c", "d"]);

    let (success, stderr) = run_q2_render(dir.path());
    assert!(
        success,
        "warnings alone must not fail the render:\n{stderr}"
    );

    assert_eq!(
        stderr.matches("Q-13-2").count(),
        1,
        "expected exactly one coalesced Q-13-2 emission:\n{stderr}"
    );
    let tail = stderr
        .lines()
        .find(|l| l.starts_with("Affected files:"))
        .unwrap_or_else(|| panic!("expected an `Affected files:` tail:\n{stderr}"));
    assert!(
        tail.contains("(and 2 others)"),
        "5 pages with a 3-name cap should summarize 2 more; got: {tail}"
    );
}

/// The coalesced config-anchored warning renders the `_quarto.yml`
/// source snippet (bd-mg3ckvp7 Phase 4 / plan D5). The warning's
/// location FileId is quarto_yaml's hash of the config path, which no
/// per-document `SourceContext` registers — so before Phase 4 the
/// block printed with no file, line, or snippet at all, leaving the
/// user with no pointer to the offending line.
#[test]
fn config_anchored_warning_shows_config_snippet() {
    let dir = TempDir::new().unwrap();
    write_broken_navbar_project(dir.path(), &["index", "a", "b"]);

    let (success, stderr) = run_q2_render(dir.path());
    assert!(
        success,
        "warnings alone must not fail the render:\n{stderr}"
    );

    assert!(
        stderr.contains("_quarto.yml"),
        "coalesced Q-13-2 should name _quarto.yml as its source:\n{stderr}"
    );
    // The YAML source line itself, which only appears if the ariadne
    // snippet rendered (the problem text says 'missing.qmd' but never
    // `href: missing.qmd`).
    assert!(
        stderr.contains("href: missing.qmd"),
        "coalesced Q-13-2 should render the offending YAML line:\n{stderr}"
    );
}

/// A single-page project still emits the warning once, with no
/// `Affected files:` tail — the legacy single-page shape.
#[test]
fn singleton_warning_keeps_legacy_shape() {
    let dir = TempDir::new().unwrap();
    write_broken_navbar_project(dir.path(), &["index"]);

    let (success, stderr) = run_q2_render(dir.path());
    assert!(
        success,
        "warnings alone must not fail the render:\n{stderr}"
    );

    assert_eq!(
        stderr.matches("Q-13-2").count(),
        1,
        "single page emits the warning exactly once:\n{stderr}"
    );
    assert!(
        !stderr.contains("Affected files:"),
        "singleton group must not grow a tail:\n{stderr}"
    );
}
