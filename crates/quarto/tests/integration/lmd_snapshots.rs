/*
 * tests/integration/lmd_snapshots.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Snapshot suite for the lmd exploration (bd-lmd-exploration-jqv2nbl4):
 * each fixture under `fixtures/lmd/` is rendered with the real binary to
 * markdown, and both the output and the diagnostics are pinned.
 */

//! Drives `q2 render <fixture> --to markdown` and snapshots two things per
//! fixture: the `.md` the qmd writer produced, and the diagnostics on
//! stderr with ANSI escapes stripped and the temp directory normalized.
//!
//! The diagnostics snapshot is the one that guards source mapping: Lua
//! errors name the document line (`fixtures/lmd/errors.qmd:9:`), markdown
//! errors inside `{=: … :=}` fragments are underlined at their byte, and
//! every failing cell is left displayed. Lua abbreviates chunk names over
//! 60 bytes to `...` plus a tail, so the tail is normalized too.
//!
//! Update with `INSTA_UPDATE=always cargo nextest run -p quarto -E
//! 'test(lmd_snapshots)'` and review the `.snap` diff.

use std::path::Path;
use std::process::Command;

use super::coalesced_diagnostics::strip_ansi;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

const FIXTURES: &[&str] = &["fragments", "cells", "nodes-and-ids", "errors"];

fn fixtures_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/integration/fixtures/lmd")
}

/// Render one fixture in a scratch copy; return (markdown, normalized stderr).
fn render(name: &str) -> (String, String) {
    let temp = tempfile::TempDir::new().unwrap();
    let dir = temp.path().to_path_buf();
    let qmd = dir.join(format!("{name}.qmd"));
    std::fs::copy(fixtures_dir().join(format!("{name}.qmd")), &qmd).unwrap();

    let output = Command::new(Q2_BIN)
        .args(["render", &format!("{name}.qmd"), "--to", "markdown"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .expect("q2 runs");
    let stderr = normalize(
        &strip_ansi(&String::from_utf8_lossy(&output.stderr)),
        &dir,
        name,
    );
    let md = std::fs::read_to_string(dir.join(format!("{name}.md"))).unwrap_or_else(|e| {
        panic!(
            "no markdown output for {name}: {e}\nexit: {:?}\nstdout:\n{}\nstderr:\n{stderr}",
            output.status,
            String::from_utf8_lossy(&output.stdout)
        )
    });
    (md, stderr)
}

/// Replace the scratch directory (in both its raw and canonical spellings,
/// and as a `file://` URL) with `<dir>`, and Lua's `...`-abbreviated chunk
/// tails with the same, so the snapshot is machine-independent.
fn normalize(stderr: &str, dir: &Path, name: &str) -> String {
    let mut out = stderr.to_string();
    let raw = dir.to_string_lossy().to_string();
    let canonical = dir
        .canonicalize()
        .map_or_else(|_| raw.clone(), |p| p.to_string_lossy().to_string());
    for d in [&canonical, &raw] {
        out = out.replace(d, "<dir>");
    }
    // Lua's abbreviated chunk name: `...<tail>/<name>.qmd:` -> `<dir>/<name>.qmd:`.
    let file = format!("{name}.qmd:");
    let mut result = String::new();
    let mut rest = out.as_str();
    while let Some(i) = rest.find("...") {
        let after = &rest[i + 3..];
        match after.find(&file) {
            Some(j) if !after[..j].contains(char::is_whitespace) => {
                result.push_str(&rest[..i]);
                result.push_str("<dir>/");
                rest = &after[j..];
            }
            _ => {
                result.push_str(&rest[..i + 3]);
                rest = after;
            }
        }
    }
    result.push_str(rest);
    // The "Rendering single file:" line carries the path too.
    result
        .lines()
        .filter(|l| !l.starts_with("Rendering "))
        .map(|l| l.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Copy a fixture directory into `dst`, recursively.
fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let target = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// The `<main>` element of a rendered HTML page: the body the document
/// produced, without the head, navigation chrome and footer that every
/// website page shares.
fn main_element(html: &str) -> &str {
    let start = html.find("<main").expect("<main");
    let end = html.rfind("</main>").expect("</main>") + "</main>".len();
    &html[start..end]
}

/// Render the `lmd-blog` project fixture (a website whose index page
/// declares a `type: metadata` listing and builds the markup in lmd
/// cells; bd-2oqen84t) and return the host page's `<main>` plus the
/// normalized stderr.
fn render_blog() -> (String, String) {
    let temp = tempfile::TempDir::new().unwrap();
    let dir = temp.path().to_path_buf();
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/integration/fixtures/lmd-blog"),
        &dir,
    );
    let output = Command::new(Q2_BIN)
        .args(["render"])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .expect("q2 runs");
    let stderr = normalize(
        &strip_ansi(&String::from_utf8_lossy(&output.stderr)),
        &dir,
        "index",
    );
    let html = std::fs::read_to_string(dir.join("_site/posts/index.html")).unwrap_or_else(|e| {
        panic!(
            "no host page for lmd-blog: {e}\nexit: {:?}\nstdout:\n{}\nstderr:\n{stderr}",
            output.status,
            String::from_utf8_lossy(&output.stdout)
        )
    });
    (main_element(&html).to_string(), stderr)
}

/// bd-2oqen84t: the motivating example end to end. A `type: metadata`
/// listing publishes its items at `meta.listings.posts`; one cell
/// builds cards (title inlines in a link whose target the rewriter
/// turned into the output URL, display date, author, category count,
/// description and its source), another groups by year over
/// `date-iso`, and an inline cell counts the items. The listing itself
/// renders no markup: no `quarto-listing` container, no List.js.
#[test]
fn lmd_blog_project_renders_as_snapshotted() {
    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_path("snapshots/lmd");
    settings.set_prepend_module_to_snapshot(false);
    settings.bind(|| {
        let (main, stderr) = render_blog();
        assert!(
            !main.contains("quarto-listing") && !main.contains("list.min.js"),
            "a metadata listing renders nothing of its own:\n{main}"
        );
        insta::assert_snapshot!("blog.main.html", main);
        insta::assert_snapshot!("blog.stderr", stderr);
    });
}

#[test]
fn lmd_fixtures_render_as_snapshotted() {
    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_path("snapshots/lmd");
    settings.set_prepend_module_to_snapshot(false);
    settings.bind(|| {
        for name in FIXTURES {
            let (md, stderr) = render(name);
            insta::assert_snapshot!(format!("{name}.md"), md);
            insta::assert_snapshot!(format!("{name}.stderr"), stderr);
        }
    });
}
