/*
 * book_scope_cli_e2e.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Plan 3 of the `q2 preview` hub-parity epic, reworked by the format-resolution
 * epic (F3b): `book-scope` is ordinary metadata, `-M book-scope:...`. A named
 * file renders alone by default; no file renders the whole book to a format
 * that consolidates it (typst, epub); `book` asks for the whole book from a
 * named file; a page outside the book is delivered standalone with a notice
 * (an info, or a warning for an explicit `book`); an explicit `book` for html,
 * docx and pptx warns that it cannot be honoured.
 *
 * Whole-book and chapter checks use epub (pandoc only, no network). Typst
 * chapter-alone uses `output-ext: typ`, which skips the typst compile.
 */

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use quarto_system_runtime::{NativeRuntime, SystemRuntime};
use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

fn pandoc_available() -> bool {
    let found = NativeRuntime::new()
        .find_binary("pandoc", "QUARTO_PANDOC")
        .is_some();
    if !found {
        eprintln!("skipping: pandoc is not installed");
    }
    found
}

fn write_file(path: &Path, contents: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

/// A three-chapter book plus `notes.qmd`, which is in the project but not the
/// book. `extra_yaml` is appended to `_quarto.yml`.
fn book(extra_yaml: &str) -> (TempDir, PathBuf) {
    let temp = TempDir::new().unwrap();
    let dir = temp.path().canonicalize().unwrap();
    write_file(
        &dir.join("_quarto.yml"),
        &format!(
            "project:\n  type: book\nbook:\n  title: Scope Book\n  chapters:\n    - index.qmd\n    - one.qmd\n    - two.qmd\n{extra_yaml}"
        ),
    );
    write_file(&dir.join("index.qmd"), "# Preface\n\nHello.\n");
    write_file(&dir.join("one.qmd"), "# One\n\nFirst chapter.\n");
    write_file(&dir.join("two.qmd"), "# Two\n\nSecond chapter.\n");
    write_file(&dir.join("notes.qmd"), "# Notes\n\nOutside the book.\n");
    (temp, dir)
}

fn q2(dir: &Path, args: &[&str]) -> Output {
    Command::new(Q2_BIN)
        .current_dir(dir)
        .env("RUST_BACKTRACE", "0")
        .arg("render")
        .args(args)
        .output()
        .expect("spawn q2")
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn books(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir.join("_book"))
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.ends_with(".epub") || n.ends_with(".typ"))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn no_scope_renders_a_named_chapter_alone() {
    if !pandoc_available() {
        return;
    }
    let (_t, dir) = book("");
    let out = q2(&dir, &["one.qmd", "--to", "epub"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(books(&dir), ["one.epub"], "{}", text(&out));
    assert!(!text(&out).contains("Q-5-41"), "{}", text(&out));
}

#[test]
fn no_file_renders_the_whole_book() {
    if !pandoc_available() {
        return;
    }
    let (_t, dir) = book("");
    let out = q2(&dir, &["--to", "epub"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(books(&dir), ["Scope-Book.epub"], "{}", text(&out));
}

#[test]
fn scope_book_renders_the_whole_book_for_a_named_chapter() {
    if !pandoc_available() {
        return;
    }
    let (_t, dir) = book("");
    let out = q2(&dir, &["one.qmd", "--to", "epub", "-M", "book-scope:book"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(books(&dir), ["Scope-Book.epub"], "{}", text(&out));
}

#[test]
fn scope_chapter_renders_the_named_chapter_alone() {
    if !pandoc_available() {
        return;
    }
    let (_t, dir) = book("");
    let out = q2(
        &dir,
        &["one.qmd", "--to", "epub", "-M", "book-scope:chapter"],
    );
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(books(&dir), ["one.epub"], "{}", text(&out));
    assert!(!text(&out).contains("Q-5-41"), "{}", text(&out));
}

#[test]
fn a_page_outside_the_book_renders_standalone_with_a_notice() {
    if !pandoc_available() {
        return;
    }
    let (_t, dir) = book("");
    // The default: an info notice, once.
    let out = q2(&dir, &["notes.qmd", "--to", "epub"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(books(&dir), ["notes.epub"], "{}", text(&out));
    assert_eq!(text(&out).matches("Q-5-41").count(), 1, "{}", text(&out));
    assert!(!text(&out).contains("Q-5-48"), "{}", text(&out));

    // An explicit `book` cannot be honoured: the warning replaces the info.
    let out = q2(
        &dir,
        &["notes.qmd", "--to", "epub", "-M", "book-scope:book"],
    );
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(text(&out).matches("Q-5-48").count(), 1, "{}", text(&out));
    assert!(!text(&out).contains("Q-5-41"), "{}", text(&out));
}

#[test]
fn chapter_scope_on_a_page_outside_the_book_has_the_info_notice() {
    if !pandoc_available() {
        return;
    }
    let (_t, dir) = book("");
    let out = q2(
        &dir,
        &["notes.qmd", "--to", "epub", "-M", "book-scope:chapter"],
    );
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(books(&dir), ["notes.epub"], "{}", text(&out));
    assert_eq!(text(&out).matches("Q-5-41").count(), 1, "{}", text(&out));
}

#[test]
fn typst_chapter_alone_writes_just_that_chapter() {
    // `output-ext: typ` keeps the typst source and skips the compile, so this
    // needs neither typst nor the network.
    let (_t, dir) = book("format:\n  typst:\n    output-ext: typ\n");
    let out = q2(
        &dir,
        &["one.qmd", "--to", "typst", "-M", "book-scope:chapter"],
    );
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(books(&dir), ["one.typ"], "{}", text(&out));
    let typ = std::fs::read_to_string(dir.join("_book/one.typ")).unwrap();
    assert!(typ.contains("First chapter"), "{typ}");
    assert!(!typ.contains("Second chapter"), "{typ}");
}

#[test]
fn an_explicit_book_for_html_warns_once_and_nothing_else_does() {
    let (_t, dir) = book("");
    let asked = q2(&dir, &["one.qmd", "--to", "html", "-M", "book-scope:book"]);
    assert!(asked.status.success(), "{}", text(&asked));
    assert_eq!(
        text(&asked).matches("Q-5-42").count(),
        1,
        "{}",
        text(&asked)
    );
    assert!(dir.join("_book/one.html").exists());

    // `chapter` is honoured (a member of an html book is a chapter anyway).
    let chapter = q2(
        &dir,
        &["one.qmd", "--to", "html", "-M", "book-scope:chapter"],
    );
    assert!(chapter.status.success(), "{}", text(&chapter));
    assert!(!text(&chapter).contains("Q-5-42"), "{}", text(&chapter));

    let absent = q2(&dir, &["one.qmd", "--to", "html"]);
    assert!(absent.status.success(), "{}", text(&absent));
    assert!(!text(&absent).contains("Q-5-42"), "{}", text(&absent));
}

#[test]
fn a_book_scope_in_the_project_config_does_not_warn() {
    // Only a request made for this render earns Q-5-42.
    let (_t, dir) = book("book-scope: book\n");
    let out = q2(&dir, &["one.qmd", "--to", "html"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(!text(&out).contains("Q-5-42"), "{}", text(&out));
}

#[test]
fn a_non_member_in_an_html_book_gets_exactly_one_notice() {
    // The html dispatch never reaches `resolve_book_scope`: the orchestrator's
    // tail emits the notice, and nothing else does.
    let (_t, dir) = book("");
    let out = q2(&dir, &["notes.qmd", "--to", "html"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(text(&out).matches("Q-5-41").count(), 1, "{}", text(&out));
    let out = q2(
        &dir,
        &["notes.qmd", "--to", "html", "-M", "book-scope:book"],
    );
    assert_eq!(text(&out).matches("Q-5-48").count(), 1, "{}", text(&out));
    assert!(!text(&out).contains("Q-5-41"), "{}", text(&out));
}

#[test]
fn scope_chapter_with_nothing_to_render_is_the_resolvers_error() {
    let (_t, dir) = book("");
    // No file, and a directory: the resolver's own error.
    for args in [
        vec!["--to", "epub", "-M", "book-scope:chapter"],
        vec![".", "--to", "epub", "-M", "book-scope:chapter"],
    ] {
        let out = q2(&dir, &args);
        assert!(!out.status.success(), "{args:?}: {}", text(&out));
        assert!(
            text(&out).contains("`book-scope: chapter` needs a file to render"),
            "{args:?}: {}",
            text(&out)
        );
        assert!(text(&out).contains("Q-5-47"), "{args:?}: {}", text(&out));
    }
}

#[test]
fn scope_chapter_with_several_files_renders_each_on_its_own() {
    if !pandoc_available() {
        return;
    }
    let (_t, dir) = book("");
    let out = q2(
        &dir,
        &[
            "one.qmd",
            "two.qmd",
            "--to",
            "epub",
            "-M",
            "book-scope:chapter",
        ],
    );
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(books(&dir), ["one.epub", "two.epub"], "{}", text(&out));
}
