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
//! `patterns.qmd` mirrors the example projects under `examples/lmd/`
//! (the documents the lmd docs page embeds), with their front matters
//! merged, so CI pins what the page shows; keep the three in step
//! (bd-lmd-docs-41adv2nx).
//!
//! Update with `INSTA_UPDATE=always cargo nextest run -p quarto -E
//! 'test(lmd_snapshots)'` and review the `.snap` diff.

use std::path::Path;
use std::process::Command;

use super::coalesced_diagnostics::strip_ansi;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

const FIXTURES: &[&str] = &[
    "fragments",
    "cells",
    "nodes-and-ids",
    "errors",
    "patterns",
    "modules",
];

/// Fixtures rendered to HTML through the native pipeline (not the pandoc
/// leg) and snapshotted as the page's `<main>`. `--to markdown` runs
/// Quarto 1's Lua filters, whose own list-table filter would convert the
/// cell-built divs this fixture exists to observe.
const HTML_FIXTURES: &[&str] = &["desugar"];

fn fixtures_dir() -> std::path::PathBuf {
    Path::new(&*std::env::var("CARGO_MANIFEST_DIR").unwrap().leak())
        .join("tests/integration/fixtures/lmd")
}

/// Render one fixture to markdown in a scratch copy; return (markdown,
/// normalized stderr).
fn render(name: &str) -> (String, String) {
    render_to(name, "markdown", "md")
}

/// Render one fixture to HTML in a scratch copy; return (the `<main>`
/// element, normalized stderr).
fn render_html(name: &str) -> (String, String) {
    let (html, stderr) = render_to(name, "html", "html");
    (main_element(&html).to_string(), stderr)
}

/// Render one fixture with `--to <format>` in a scratch copy and read back
/// `<name>.<ext>`; return (output, normalized stderr).
///
/// The scratch copy holds the fixture's `.qmd`, every non-`.qmd` file in
/// the fixtures directory (shared resources such as images) and every
/// subdirectory (the `modules/` the modules fixture requires), so a fixture
/// that references one renders without a missing-resource warning.
fn render_to(name: &str, format: &str, ext: &str) -> (String, String) {
    let temp = tempfile::TempDir::new().unwrap();
    let dir = temp.path().to_path_buf();
    let qmd = dir.join(format!("{name}.qmd"));
    std::fs::copy(fixtures_dir().join(format!("{name}.qmd")), &qmd).unwrap();
    for entry in std::fs::read_dir(fixtures_dir()).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            // Module directories (`modules/*.lmd`) and other grouped resources.
            copy_dir(&path, &dir.join(entry.file_name()));
        } else if path.extension().is_some_and(|e| e != "qmd") {
            std::fs::copy(&path, dir.join(entry.file_name())).unwrap();
        }
    }

    let output = Command::new(Q2_BIN)
        .args(["render", &format!("{name}.qmd"), "--to", format])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .expect("q2 runs");
    let stderr = normalize(
        &strip_ansi(&String::from_utf8_lossy(&output.stderr)),
        &dir,
        name,
    );
    let out = std::fs::read_to_string(dir.join(format!("{name}.{ext}"))).unwrap_or_else(|e| {
        panic!(
            "no {format} output for {name}: {e}\nexit: {:?}\nstdout:\n{}\nstderr:\n{stderr}",
            output.status,
            String::from_utf8_lossy(&output.stdout)
        )
    });
    (out, stderr)
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
    // Lua's abbreviated chunk name, `...<tail of dir>/<file>:`, becomes
    // `<dir>/<file>:`. The scratch directory's own last component marks
    // where the tail ends (so a module under `modules/` keeps its relative
    // path); when the abbreviation cut into that component too, the
    // fixture's own file name is the fallback marker.
    let base = format!("{}/", dir.file_name().unwrap().to_string_lossy());
    let file = format!("{name}.qmd:");
    let unbroken = |s: &str| !s.contains(char::is_whitespace);
    let mut result = String::new();
    let mut rest = out.as_str();
    while let Some(i) = rest.find("...") {
        let after = &rest[i + 3..];
        let cut = after
            .find(&base)
            .map(|j| j + base.len())
            .filter(|&j| unbroken(&after[..j]))
            .or_else(|| after.find(&file).filter(|&j| unbroken(&after[..j])));
        match cut {
            Some(j) => {
                result.push_str(&rest[..i]);
                result.push_str("<dir>/");
                rest = &after[j..];
            }
            None => {
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

/// Render a project fixture under `fixtures/<fixture>` in a scratch copy
/// and return `<main>` of the output page at `_site/<page>` plus the
/// normalized stderr.
fn render_project(fixture: &str, page: &str) -> (String, String) {
    let temp = tempfile::TempDir::new().unwrap();
    let dir = temp.path().to_path_buf();
    copy_dir(
        &Path::new(&*std::env::var("CARGO_MANIFEST_DIR").unwrap().leak())
            .join(format!("tests/integration/fixtures/{fixture}")),
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
    let html = std::fs::read_to_string(dir.join("_site").join(page)).unwrap_or_else(|e| {
        panic!(
            "no output page for {fixture}: {e}\nexit: {:?}\nstdout:\n{}\nstderr:\n{stderr}",
            output.status,
            String::from_utf8_lossy(&output.stdout)
        )
    });
    (main_element(&html).to_string(), stderr)
}

/// Render the `lmd-blog` project fixture (a website whose index page
/// declares a `type: metadata` listing and builds the markup in lmd
/// cells; bd-2oqen84t) and return the host page's `<main>` plus the
/// normalized stderr.
fn render_blog() -> (String, String) {
    render_project("lmd-blog", "posts/index.html")
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

/// bd-76ogq4ew phase 3: modules across a project, end to end. A module at
/// the project root and an extension's module, both autoloaded by
/// `lmd: modules:` in `_quarto.yml`; a `/`-rooted `require` whose module
/// requires its neighbour; a parameterised `quarto.include` of an `.lmd`
/// partial and a `/`-rooted include of a markdown one.
#[test]
fn lmd_project_renders_as_snapshotted() {
    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_path("snapshots/lmd");
    settings.set_prepend_module_to_snapshot(false);
    settings.bind(|| {
        let (main, stderr) = render_project("lmd-project", "index.html");
        insta::assert_snapshot!("project.main.html", main);
        insta::assert_snapshot!("project.stderr", stderr);
    });
}

/// bd-7gqr148v (GH #846): the structural desugaring contract. A
/// list-table / definition-list div or a single-image paragraph must
/// become a `Table` / `DefinitionList` / `Figure` whether the reader
/// parsed it, an lmd cell built it from nodes, a fragment carried it, or
/// `pandoc.read` returned it.
#[test]
fn lmd_html_fixtures_render_as_snapshotted() {
    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_path("snapshots/lmd");
    settings.set_prepend_module_to_snapshot(false);
    settings.bind(|| {
        for name in HTML_FIXTURES {
            let (main, stderr) = render_html(name);
            insta::assert_snapshot!(format!("{name}.main.html"), main);
            insta::assert_snapshot!(format!("{name}.stderr"), stderr);
        }
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
