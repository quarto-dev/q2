/*
 * book_scope_cli_e2e.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Plan 3 of the `q2 preview` hub-parity epic: `q2 render --book-scope`.
 * Book scope is explicit: with no option a book renders whole to a format that
 * consolidates it (typst, epub); `chapter` renders the one named file alone;
 * a page outside the book is delivered standalone with a notice; html, docx
 * and pptx ignore the option with an info diagnostic.
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
fn no_scope_renders_the_whole_book_for_a_named_chapter() {
    if !pandoc_available() {
        return;
    }
    let (_t, dir) = book("");
    let out = q2(&dir, &["one.qmd", "--to", "epub"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(books(&dir), ["Scope-Book.epub"], "{}", text(&out));
    assert!(!text(&out).contains("Q-5-41"), "{}", text(&out));
}

#[test]
fn scope_book_renders_the_whole_book_for_a_named_chapter() {
    if !pandoc_available() {
        return;
    }
    let (_t, dir) = book("");
    let out = q2(&dir, &["one.qmd", "--to", "epub", "--book-scope", "book"]);
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
        &["one.qmd", "--to", "epub", "--book-scope", "chapter"],
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
    let out = q2(&dir, &["notes.qmd", "--to", "epub"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(books(&dir), ["notes.epub"], "{}", text(&out));
    assert!(text(&out).contains("Q-5-41"), "{}", text(&out));

    let out = q2(&dir, &["notes.qmd", "--to", "epub", "--book-scope", "book"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("Q-5-41"), "{}", text(&out));
}

#[test]
fn chapter_scope_on_a_page_outside_the_book_has_no_notice() {
    if !pandoc_available() {
        return;
    }
    let (_t, dir) = book("");
    let out = q2(
        &dir,
        &["notes.qmd", "--to", "epub", "--book-scope", "chapter"],
    );
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(books(&dir), ["notes.epub"], "{}", text(&out));
    assert!(!text(&out).contains("Q-5-41"), "{}", text(&out));
}

#[test]
fn typst_chapter_alone_writes_just_that_chapter() {
    // `output-ext: typ` keeps the typst source and skips the compile, so this
    // needs neither typst nor the network.
    let (_t, dir) = book("format:\n  typst:\n    output-ext: typ\n");
    let out = q2(
        &dir,
        &["one.qmd", "--to", "typst", "--book-scope", "chapter"],
    );
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(books(&dir), ["one.typ"], "{}", text(&out));
    let typ = std::fs::read_to_string(dir.join("_book/one.typ")).unwrap();
    assert!(typ.contains("First chapter"), "{typ}");
    assert!(!typ.contains("Second chapter"), "{typ}");
}

#[test]
fn scope_is_ignored_with_an_info_diagnostic_for_html_only_when_given() {
    let (_t, dir) = book("");
    let given = q2(
        &dir,
        &["one.qmd", "--to", "html", "--book-scope", "chapter"],
    );
    assert!(given.status.success(), "{}", text(&given));
    assert!(text(&given).contains("Q-5-42"), "{}", text(&given));
    assert!(dir.join("_book/one.html").exists());

    let absent = q2(&dir, &["one.qmd", "--to", "html"]);
    assert!(absent.status.success(), "{}", text(&absent));
    assert!(!text(&absent).contains("Q-5-42"), "{}", text(&absent));
}

#[test]
fn scope_chapter_needs_exactly_one_file() {
    let (_t, dir) = book("");
    for args in [
        vec!["--to", "epub", "--book-scope", "chapter"],
        vec![
            "one.qmd",
            "two.qmd",
            "--to",
            "epub",
            "--book-scope",
            "chapter",
        ],
        vec![".", "--to", "epub", "--book-scope", "chapter"],
    ] {
        let out = q2(&dir, &args);
        assert!(!out.status.success(), "{args:?}: {}", text(&out));
        assert!(
            text(&out).contains("`--book-scope chapter` needs exactly one file"),
            "{args:?}: {}",
            text(&out)
        );
    }
    assert!(books(&dir).is_empty());
}
