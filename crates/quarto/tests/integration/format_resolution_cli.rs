/*
 * format_resolution_cli.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * F5a of the format-resolution epic: `q2 render` chooses its format after it
 * knows the project (FR1-FR4, FR6, FR8, FR11, FR18, FR22). Whole-book checks
 * use epub (pandoc only, no network); typst chapter checks use
 * `output-ext: typ`, which skips the typst compile. Tests that need pandoc
 * skip (return early) when it is absent, so a skip is a silent pass: CI
 * installs it.
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

fn q2_in(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(Q2_BIN);
    cmd.current_dir(dir)
        .env("RUST_BACKTRACE", "0")
        .env_remove("QUARTO_PROFILE")
        .arg("render")
        .args(args);
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.output().expect("spawn q2")
}

fn q2(dir: &Path, args: &[&str]) -> Output {
    q2_in(dir, args, &[])
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// Every output file under `_book/` (or `_site/`), relative, sorted, with the
/// `site_libs` support directory and the search index (`search.json`, which
/// a book render writes whatever the format) left out.
fn outputs(dir: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in rd.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                if entry.file_name() != "site_libs"
                    && !entry.file_name().to_string_lossy().ends_with("_files")
                {
                    walk(root, &path, out);
                }
            } else {
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                if rel != "search.json" {
                    out.push(rel);
                }
            }
        }
    }
    let mut names = Vec::new();
    for out_dir in ["_book", "_site"] {
        let root = dir.join(out_dir);
        walk(&root, &root, &mut names);
    }
    names.sort();
    names
}

struct Fixture {
    _temp: TempDir,
    dir: PathBuf,
}

fn fixture(files: &[(&str, &str)]) -> Fixture {
    let temp = TempDir::new().unwrap();
    let dir = temp.path().canonicalize().unwrap();
    for (name, contents) in files {
        write_file(&dir.join(name), contents);
    }
    Fixture { _temp: temp, dir }
}

const PLAIN: &str = "# Plain\n\nBody.\n";
const SAYS_HTML: &str = "---\nformat: html\n---\n\n# Says html\n\nBody.\n";

/// A book whose project format is `format_yaml`, chapters `index`, `one`,
/// `two` (front matter `chapter_two`), and `notes.qmd` outside the book.
fn book(format_yaml: &str, chapter_two: &str) -> Fixture {
    fixture(&[
        (
            "_quarto.yml",
            &format!(
                "project:\n  type: book\n{format_yaml}book:\n  title: Plain Book\n  chapters:\n    - index.qmd\n    - one.qmd\n    - two.qmd\n"
            ),
        ),
        ("index.qmd", "# Preface\n\nHello.\n"),
        ("one.qmd", "# One\n\nFirst chapter.\n"),
        ("two.qmd", chapter_two),
        ("notes.qmd", "# Notes\n\nOutside the book.\n"),
    ])
}

fn epub_book() -> Fixture {
    book("format: epub\n", SAYS_HTML)
}

// ---------------------------------------------------------------------------
// The project's format reaches the CLI (the strand's symptom)
// ---------------------------------------------------------------------------

#[test]
fn a_project_format_epub_book_renders_as_a_book_with_no_to() {
    if !pandoc_available() {
        return;
    }
    let f = epub_book();
    let out = q2(&f.dir, &[]);
    assert!(out.status.success(), "{}", text(&out));
    // `two.qmd` says `format: html`, which a whole-book render ignores (FR4).
    assert_eq!(outputs(&f.dir), ["Plain-Book.epub"], "{}", text(&out));
    for code in ["Q-21-3", "Q-5-42", "Q-5-33"] {
        assert!(!text(&out).contains(code), "{code}: {}", text(&out));
    }
}

#[test]
fn a_named_chapter_renders_alone_in_its_own_format() {
    if !pandoc_available() {
        return;
    }
    let f = epub_book();
    let out = q2(&f.dir, &["one.qmd"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(outputs(&f.dir), ["one.epub"], "{}", text(&out));

    // `two.qmd` says html: a named chapter honors its own format (FR4).
    let out = q2(&f.dir, &["two.qmd"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(outputs(&f.dir), ["one.epub", "two.html"], "{}", text(&out));
}

#[test]
fn book_scope_book_renders_the_whole_book_for_named_chapters() {
    if !pandoc_available() {
        return;
    }
    let f = epub_book();
    for args in [
        vec!["one.qmd", "-M", "book-scope:book"],
        vec!["one.qmd", "two.qmd", "-M", "book-scope:book"],
    ] {
        let out = q2(&f.dir, &args);
        assert!(out.status.success(), "{args:?}: {}", text(&out));
        assert_eq!(
            outputs(&f.dir),
            ["Plain-Book.epub"],
            "{args:?}: {}",
            text(&out)
        );
        std::fs::remove_dir_all(f.dir.join("_book")).unwrap();
    }
}

// ---------------------------------------------------------------------------
// Several named files each render on their own (F5a-D3b)
// ---------------------------------------------------------------------------

#[test]
fn several_named_files_each_render_on_their_own_even_when_they_are_every_chapter() {
    if !pandoc_available() {
        return;
    }
    let f = epub_book();
    let out = q2(&f.dir, &["one.qmd", "two.qmd"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(outputs(&f.dir), ["one.epub", "two.html"], "{}", text(&out));
    std::fs::remove_dir_all(f.dir.join("_book")).unwrap();

    // Every chapter of a consolidating book is still one render per file.
    let out = q2(&f.dir, &["index.qmd", "one.qmd", "two.qmd"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(
        outputs(&f.dir),
        ["index.epub", "one.epub", "two.html"],
        "{}",
        text(&out)
    );
    // One manifest lists every output (the last writer does not win).
    let manifest = std::fs::read_to_string(f.dir.join(".quarto/render-manifest.json")).unwrap();
    for name in ["index.epub", "one.epub", "two.html"] {
        assert!(manifest.contains(name), "{name} missing from {manifest}");
    }
}

#[test]
fn a_directory_renders_each_file_under_it_on_its_own() {
    if !pandoc_available() {
        return;
    }
    let f = fixture(&[
        (
            "_quarto.yml",
            "project:\n  type: book\nformat: epub\nbook:\n  title: Dir Book\n  chapters:\n    - index.qmd\n    - sub/a.qmd\n    - sub/b.qmd\n",
        ),
        ("index.qmd", PLAIN),
        ("sub/a.qmd", PLAIN),
        ("sub/b.qmd", PLAIN),
    ]);
    let out = q2(&f.dir, &["sub/"]);
    assert!(out.status.success(), "{}", text(&out));
    let produced = outputs(&f.dir);
    assert_eq!(produced.len(), 2, "{produced:?}: {}", text(&out));
    assert!(
        produced
            .iter()
            .all(|p| p.ends_with("a.epub") || p.ends_with("b.epub"))
    );
}

#[test]
fn naming_every_page_of_a_website_still_renders_the_whole_project() {
    let f = fixture(&[
        (
            "_quarto.yml",
            "project:\n  type: website\nwebsite:\n  title: W\n",
        ),
        ("index.qmd", PLAIN),
        ("about.qmd", PLAIN),
    ]);
    let out = q2(&f.dir, &["index.qmd", "about.qmd"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("Rendered 2 of 2"), "{}", text(&out));
    assert!(f.dir.join("_site/sitemap.xml").exists() || f.dir.join("_site/index.html").exists());
}

// ---------------------------------------------------------------------------
// html books keep today's path (FR19, F5a-D3b step 2)
// ---------------------------------------------------------------------------

const HTML_ONE: &str = "# One\n\nSee @fig-two.\n";
const HTML_TWO: &str = "# Two\n\n```{.python}\n#| label: fig-two\n#| fig-cap: Two\nx = 1\n```\n";

fn html_book(chapter_two: &str) -> Fixture {
    fixture(&[
        (
            "_quarto.yml",
            "project:\n  type: book\nbook:\n  title: H Book\n  chapters:\n    - index.qmd\n    - one.qmd\n    - two.qmd\n",
        ),
        ("index.qmd", "# Preface {.unnumbered}\n\nHello.\n"),
        ("one.qmd", HTML_ONE),
        ("two.qmd", chapter_two),
        ("notes.qmd", "# Notes\n\nOutside the book.\n"),
    ])
}

fn read(dir: &Path, rel: &str) -> String {
    std::fs::read_to_string(dir.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

#[test]
fn naming_every_chapter_of_an_html_book_renders_the_whole_book() {
    let f = html_book(HTML_TWO);
    let out = q2(&f.dir, &["index.qmd", "one.qmd", "two.qmd"]);
    assert!(out.status.success(), "{}", text(&out));
    let one = read(&f.dir, "_book/one.html");
    assert!(
        !one.contains("quarto-unresolved-ref"),
        "the cross-chapter reference must resolve: {one}"
    );
    // The same render with the non-member also named renders it too.
    std::fs::remove_dir_all(f.dir.join("_book")).unwrap();
    let out = q2(&f.dir, &["index.qmd", "one.qmd", "two.qmd", "notes.qmd"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(f.dir.join("_book/notes.html").exists(), "{}", text(&out));
    assert!(!read(&f.dir, "_book/one.html").contains("quarto-unresolved-ref"));
    assert_eq!(text(&out).matches("Q-5-41").count(), 1, "{}", text(&out));
}

#[test]
fn naming_every_chapter_when_one_says_typst_renders_each_on_its_own() {
    let f = html_book(&format!(
        "---\nformat:\n  typst:\n    output-ext: typ\n---\n\n{HTML_TWO}"
    ));
    let out = q2(&f.dir, &["index.qmd", "one.qmd", "two.qmd"]);
    assert!(out.status.success(), "{}", text(&out));
    let produced = outputs(&f.dir);
    assert!(produced.contains(&"one.html".to_string()), "{produced:?}");
    assert!(produced.contains(&"two.typ".to_string()), "{produced:?}");
    // Not the whole-book pass: the html chapters keep the known limitation.
    assert!(read(&f.dir, "_book/one.html").contains("quarto-unresolved-ref"));
}

#[test]
fn a_member_chapter_of_an_html_book_renders_as_html_with_the_known_limitation() {
    // FR19: the html chapter path is unchanged (`bd-html-chapter-xref-szfxmvyg`).
    // `single_chapter_subset_render_keeps_one_file_render_set` pins only the
    // output count; this pins the unresolved marker and the diagnostic title,
    // through the CLI so a reroute inside `render_once` is caught. When that
    // strand is fixed this is the test to update.
    let f = html_book(HTML_TWO);
    let out = q2(&f.dir, &["one.qmd"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(outputs(&f.dir), ["one.html"], "{}", text(&out));
    let one = read(&f.dir, "_book/one.html");
    assert!(
        one.contains("class=\"quarto-xref quarto-unresolved-ref\"") && one.contains("?fig-two?"),
        "{one}"
    );
    assert!(text(&out).contains("unresolved crossref"), "{}", text(&out));

    // Whole-book: the same reference resolves.
    std::fs::remove_dir_all(f.dir.join("_book")).unwrap();
    let out = q2(&f.dir, &[]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(!read(&f.dir, "_book/one.html").contains("quarto-unresolved-ref"));
}

#[test]
fn a_named_page_outside_an_html_book_renders_on_its_own_with_one_notice() {
    let f = html_book(HTML_TWO);
    let out = q2(&f.dir, &["notes.qmd"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(outputs(&f.dir), ["notes.html"], "{}", text(&out));
    assert_eq!(text(&out).matches("Q-5-41").count(), 1, "{}", text(&out));
}

#[test]
fn an_html_book_with_a_typst_chapter_renders_html_for_the_book_and_typst_for_the_chapter() {
    let f = html_book(&format!(
        "---\nformat:\n  typst:\n    output-ext: typ\n---\n\n{HTML_TWO}"
    ));
    let out = q2(&f.dir, &[]);
    assert!(out.status.success(), "{}", text(&out));
    let produced = outputs(&f.dir);
    assert!(produced.contains(&"two.html".to_string()), "{produced:?}");
    assert!(
        !produced.iter().any(|p| p.ends_with(".typ")),
        "{produced:?}"
    );
    std::fs::remove_dir_all(f.dir.join("_book")).unwrap();

    let out = q2(&f.dir, &["two.qmd"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(outputs(&f.dir), ["two.typ"], "{}", text(&out));
}

// ---------------------------------------------------------------------------
// Explicit requests (FR6)
// ---------------------------------------------------------------------------

#[test]
fn chapter_scope_with_nothing_to_render_is_an_error_only_in_a_book() {
    let f = epub_book();
    let out = q2(&f.dir, &["-M", "book-scope:chapter"]);
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("Q-5-47"), "{}", text(&out));
    assert!(
        text(&out).contains("`book-scope: chapter` needs a file to render"),
        "{}",
        text(&out)
    );

    // A website: an info, never an error.
    let site = fixture(&[
        (
            "_quarto.yml",
            "project:\n  type: website\nwebsite:\n  title: W\n",
        ),
        ("index.qmd", PLAIN),
    ]);
    let out = q2(&site.dir, &["-M", "book-scope:chapter"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(!text(&out).contains("Q-5-47"), "{}", text(&out));

    // A standalone document too.
    let alone = fixture(&[("doc.qmd", PLAIN)]);
    let out = q2(&alone.dir, &["doc.qmd", "-M", "book-scope:chapter"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(!text(&out).contains("Q-5-47"), "{}", text(&out));
}

#[test]
fn to_html_overrides_the_project_format_without_changing_the_scope() {
    let f = epub_book();
    let out = q2(&f.dir, &["--to", "html"]);
    assert!(out.status.success(), "{}", text(&out));
    let produced = outputs(&f.dir);
    assert!(produced.contains(&"index.html".to_string()), "{produced:?}");
    assert!(
        !produced.iter().any(|p| p.ends_with(".epub")),
        "{produced:?}"
    );
    std::fs::remove_dir_all(f.dir.join("_book")).unwrap();

    let out = q2(&f.dir, &["one.qmd", "--to", "html"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(outputs(&f.dir), ["one.html"], "{}", text(&out));
}

#[test]
fn a_runtime_format_selects_a_declared_key_and_loses_to_to() {
    let f = book("format:\n  epub: default\n  html: default\n", PLAIN);
    // No request: the first key, with the not-yet-supported notice.
    if pandoc_available() {
        let out = q2(&f.dir, &[]);
        assert!(out.status.success(), "{}", text(&out));
        assert_eq!(outputs(&f.dir), ["Plain-Book.epub"], "{}", text(&out));
        assert!(text(&out).contains("Q-20-8"), "{}", text(&out));
        assert!(text(&out).contains("not yet supported"), "{}", text(&out));
        std::fs::remove_dir_all(f.dir.join("_book")).unwrap();
    }
    // `-M format:html` picks the second key, quietly.
    let out = q2(&f.dir, &["-M", "format:html"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        outputs(&f.dir).contains(&"index.html".to_string()),
        "{}",
        text(&out)
    );
    assert!(!text(&out).contains("Q-20-8"), "{}", text(&out));
    std::fs::remove_dir_all(f.dir.join("_book")).unwrap();

    // `--to` beats the runtime format.
    let out = q2(&f.dir, &["--to", "html", "-M", "format:epub"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        outputs(&f.dir).contains(&"index.html".to_string()),
        "{}",
        text(&out)
    );
    assert!(!text(&out).contains("Q-20-8"), "{}", text(&out));
}

#[test]
fn a_multi_key_project_warns_and_to_silences_it() {
    let site = fixture(&[
        (
            "_quarto.yml",
            "project:\n  type: website\nwebsite:\n  title: W\nformat:\n  html: default\n  revealjs: default\n",
        ),
        ("index.qmd", PLAIN),
    ]);
    let out = q2(&site.dir, &[]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(text(&out).matches("Q-20-8").count(), 1, "{}", text(&out));
    let out = q2(&site.dir, &["--to", "html"]);
    assert!(!text(&out).contains("Q-20-8"), "{}", text(&out));
}

// ---------------------------------------------------------------------------
// Where the format comes from
// ---------------------------------------------------------------------------

#[test]
fn the_project_format_can_come_from_a_profile() {
    if !pandoc_available() {
        return;
    }
    let f = book("", PLAIN);
    write_file(&f.dir.join("_quarto-epub.yml"), "format: epub\n");
    let out = q2(&f.dir, &["--profile", "epub"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(outputs(&f.dir), ["Plain-Book.epub"], "{}", text(&out));
    std::fs::remove_dir_all(f.dir.join("_book")).unwrap();

    // The environment variable reaches the same layer.
    let out = q2_in(&f.dir, &[], &[("QUARTO_PROFILE", "epub")]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(outputs(&f.dir), ["Plain-Book.epub"], "{}", text(&out));
}

#[test]
fn a_nested_metadata_file_format_applies_to_a_named_page_in_a_plain_project() {
    if !pandoc_available() {
        return;
    }
    let f = fixture(&[
        ("_quarto.yml", "project:\n  title: P\n"),
        ("sub/_metadata.yml", "format: epub\n"),
        ("sub/a.qmd", PLAIN),
    ]);
    let out = q2(&f.dir, &["sub/a.qmd"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(contains_file(&f.dir, "a.epub"), "{}", text(&out));
}

fn contains_file(dir: &Path, name: &str) -> bool {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return false;
    };
    rd.filter_map(|e| e.ok()).any(|e| {
        let p = e.path();
        if p.is_dir() {
            contains_file(&p, name)
        } else {
            e.file_name() == name
        }
    })
}

// ---------------------------------------------------------------------------
// Q-5-33: a book whose own format has no book support
// ---------------------------------------------------------------------------

#[test]
fn a_whole_book_render_in_an_unsupported_format_is_refused_and_a_chapter_is_not() {
    let f = book("format: docx\n", PLAIN);
    let out = q2(&f.dir, &[]);
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("Q-5-33"), "{}", text(&out));
    assert!(text(&out).contains("project's `format:`"), "{}", text(&out));
    assert!(outputs(&f.dir).is_empty());

    if pandoc_available() {
        let out = q2(&f.dir, &["one.qmd"]);
        assert!(out.status.success(), "{}", text(&out));
        assert_eq!(outputs(&f.dir), ["one.docx"], "{}", text(&out));
    }
}

#[test]
fn to_docx_on_a_whole_book_names_to_and_on_a_chapter_renders_the_chapter() {
    let f = book("format:\n  typst:\n    output-ext: typ\n", PLAIN);
    let out = q2(&f.dir, &["--to", "docx"]);
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("Q-5-33"), "{}", text(&out));
    assert!(text(&out).contains("--to"), "{}", text(&out));
    assert!(
        !text(&out).contains("This book project targets"),
        "{}",
        text(&out)
    );

    if pandoc_available() {
        let out = q2(&f.dir, &["one.qmd", "--to", "docx"]);
        assert!(out.status.success(), "{}", text(&out));
        assert_eq!(outputs(&f.dir), ["one.docx"], "{}", text(&out));
    }
}

// ---------------------------------------------------------------------------
// Chapter-alone keeps an extension format
// ---------------------------------------------------------------------------

#[test]
fn a_typst_book_chapter_with_output_ext_typ_renders_just_that_chapter_from_the_project_format() {
    // Project-level `format: typst` with no `--to`: the chapter renders alone
    // (no whole-book merge, so no typst binary and no network are needed).
    let f = book("format:\n  typst:\n    output-ext: typ\n", PLAIN);
    let out = q2(&f.dir, &["one.qmd"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(outputs(&f.dir), ["one.typ"], "{}", text(&out));
    let typ = read(&f.dir, "_book/one.typ");
    assert!(typ.contains("First chapter"), "{typ}");
}

#[test]
fn a_typst_book_with_a_chapter_saying_html_honors_it_only_in_chapter_scope() {
    let f = book("format:\n  typst:\n    output-ext: typ\n", SAYS_HTML);
    let out = q2(&f.dir, &["two.qmd"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(outputs(&f.dir), ["two.html"], "{}", text(&out));
}

#[test]
fn several_files_in_a_typst_book_each_render_alone_in_input_order() {
    let f = book("format:\n  typst:\n    output-ext: typ\n", SAYS_HTML);
    let out = q2(&f.dir, &["two.qmd", "one.qmd", "notes.qmd"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(
        outputs(&f.dir),
        ["notes.typ", "one.typ", "two.html"],
        "{}",
        text(&out)
    );
    assert_eq!(text(&out).matches("Q-5-41").count(), 1, "{}", text(&out));
    assert!(text(&out).contains("Rendered 3 of 4"), "{}", text(&out));
}

#[test]
fn fail_fast_stops_the_loop_after_the_first_failure() {
    let f = book("format:\n  typst:\n    output-ext: typ\n", PLAIN);
    // A chapter that cannot parse.
    write_file(
        &f.dir.join("one.qmd"),
        "---\ntitle: One\n---\n\nLorem ipsum _dolor\n",
    );
    let out = q2(&f.dir, &["one.qmd", "two.qmd", "--fail-fast"]);
    assert!(!out.status.success(), "{}", text(&out));
    assert!(
        !outputs(&f.dir).contains(&"two.typ".to_string()),
        "{}",
        text(&out)
    );
}
