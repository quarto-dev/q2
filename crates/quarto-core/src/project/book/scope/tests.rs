use super::*;
use quarto_error_reporting::DiagnosticKind;
use quarto_system_runtime::NativeRuntime;
use std::path::PathBuf;

fn format(key: &str) -> Format {
    Format::from_format_string(key).unwrap()
}

fn request(value: BookScope, explicit: Option<LayerKind>) -> ScopeRequest {
    ScopeRequest {
        value,
        explicit,
        diagnostics: Vec::new(),
    }
}

fn codes(notices: &[DiagnosticMessage]) -> Vec<&str> {
    notices.iter().filter_map(|d| d.code.as_deref()).collect()
}

const MEMBER: BookMembership = BookMembership {
    chapters: 3,
    active_is_chapter: true,
};
const NON_MEMBER: BookMembership = BookMembership {
    chapters: 3,
    active_is_chapter: false,
};

// ── scope_notices ────────────────────────────────────────────────────────

/// Every (membership) x (request) x (format) row. Columns: member on html,
/// member on typst, non-member on html, non-member on typst.
#[test]
fn scope_notices_table() {
    use BookScope::{Book, Chapter};
    use LayerKind::{Directory, Document, Project, Runtime};
    type Row = (
        &'static str,
        BookScope,
        Option<LayerKind>,
        [&'static [&'static str]; 4],
    );
    let none: &[&str] = &[];
    let info: &[&str] = &["Q-5-41"];
    let warn: &[&str] = &["Q-5-48"];
    let ignored: &[&str] = &["Q-5-42"];
    let rows: [Row; 9] = [
        (
            "target default chapter",
            Chapter,
            None,
            [none, none, info, info],
        ),
        ("target default book", Book, None, [none, none, info, info]),
        (
            "runtime chapter",
            Chapter,
            Some(Runtime),
            [none, none, info, info],
        ),
        (
            "runtime book",
            Book,
            Some(Runtime),
            [ignored, none, warn, warn],
        ),
        (
            "project chapter",
            Chapter,
            Some(Project),
            [none, none, info, info],
        ),
        (
            "directory chapter",
            Chapter,
            Some(Directory),
            [none, none, info, info],
        ),
        (
            "document chapter",
            Chapter,
            Some(Document),
            [none, none, info, info],
        ),
        (
            "project book",
            Book,
            Some(Project),
            [none, none, info, info],
        ),
        (
            "document book",
            Book,
            Some(Document),
            [none, none, info, info],
        ),
    ];
    let dir = Path::new("/p");
    let file = Path::new("/p/notes.qmd");
    for (label, value, explicit, expected) in rows {
        let req = request(value, explicit);
        let cells = [
            (&MEMBER, "html", expected[0]),
            (&MEMBER, "typst", expected[1]),
            (&NON_MEMBER, "html", expected[2]),
            (&NON_MEMBER, "typst", expected[3]),
        ];
        for (membership, fmt, want) in cells {
            let got = scope_notices(&req, Some(membership), &format(fmt), Some(file), dir);
            assert_eq!(
                codes(&got),
                want,
                "{label}: member={} {fmt}",
                membership.active_is_chapter
            );
        }
    }
}

#[test]
fn a_render_naming_no_file_can_still_warn_about_an_unhonourable_book() {
    let dir = Path::new("/p");
    let runtime_book = request(BookScope::Book, Some(LayerKind::Runtime));
    for (fmt, want) in [
        ("html", vec!["Q-5-42"]),
        ("docx", vec!["Q-5-42"]),
        ("pptx", vec!["Q-5-42"]),
        ("typst", vec![]),
        ("epub", vec![]),
    ] {
        let got = scope_notices(&runtime_book, Some(&MEMBER), &format(fmt), None, dir);
        assert_eq!(codes(&got), want, "{fmt}");
    }
    let default = request(BookScope::Book, None);
    assert!(scope_notices(&default, Some(&MEMBER), &format("html"), None, dir).is_empty());
}

#[test]
fn a_project_that_is_not_a_book_gets_no_notice() {
    let req = request(BookScope::Book, Some(LayerKind::Runtime));
    let got = scope_notices(
        &req,
        None,
        &format("html"),
        Some(Path::new("/p/a.qmd")),
        Path::new("/p"),
    );
    assert!(got.is_empty());
}

#[test]
fn the_non_member_notices_name_the_file_relative_to_the_project() {
    let dir = Path::new("/p");
    let file = Path::new("/p/sub/notes.qmd");
    let info = scope_notices(
        &request(BookScope::Chapter, None),
        Some(&NON_MEMBER),
        &format("typst"),
        Some(file),
        dir,
    );
    let problem = info[0].problem.as_ref().unwrap().as_str().to_string();
    assert!(problem.contains("`sub/notes.qmd` is not a chapter of this book"));
    assert!(problem.contains("cannot link to the book's chapters"));
    assert_eq!(info[0].kind, DiagnosticKind::Info);
    let warn = scope_notices(
        &request(BookScope::Book, Some(LayerKind::Runtime)),
        Some(&NON_MEMBER),
        &format("typst"),
        Some(file),
        dir,
    );
    assert_eq!(warn[0].kind, DiagnosticKind::Warning);
    assert!(
        warn[0]
            .problem
            .as_ref()
            .unwrap()
            .as_str()
            .contains("`sub/notes.qmd`")
    );
}

#[test]
fn q_5_39_is_a_warning_worded_for_the_flag() {
    let got = scope_notices(
        &request(BookScope::Book, Some(LayerKind::Runtime)),
        Some(&MEMBER),
        &format("docx"),
        Some(Path::new("/p/a.qmd")),
        Path::new("/p"),
    );
    assert_eq!(got[0].kind, DiagnosticKind::Warning);
    let problem = got[0].problem.as_ref().unwrap().as_str();
    assert!(problem.contains("`book-scope: book` was requested"));
    assert!(!problem.contains("--book-scope"));
}

// ── resolve_book_scope ───────────────────────────────────────────────────

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// A three-chapter book plus `notes.qmd`, which is in the project but not a
/// chapter.
fn book() -> (tempfile::TempDir, ProjectContext, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    write(
        &root,
        "_quarto.yml",
        "project:\n  type: book\nbook:\n  title: B\n  chapters:\n    - index.qmd\n    - one.qmd\n    - two.qmd\n",
    );
    for f in ["index", "one", "two", "notes"] {
        write(&root, &format!("{f}.qmd"), &format!("# {f}\n"));
    }
    let project = ProjectContext::discover(root.join("one.qmd"), &NativeRuntime::new()).unwrap();
    (dir, project, root)
}

fn resolve(
    project: &ProjectContext,
    file: &Path,
    req: &ScopeRequest,
    fmt: &str,
) -> BookScopeResolution {
    resolve_book_scope(project, file, req, &format(fmt), &NativeRuntime::new()).unwrap()
}

#[test]
fn a_member_follows_the_request_on_a_consolidating_format() {
    let (_g, project, root) = book();
    let one = root.join("one.qmd");
    for fmt in ["typst", "epub"] {
        let book = resolve(&project, &one, &request(BookScope::Book, None), fmt);
        assert_eq!(book.scope, ResolvedBookScope::Book, "{fmt}");
        assert!(book.diagnostics.is_empty(), "{fmt}");
        assert_eq!(book.membership.map(|m| m.chapters), Some(3));
        let chapter = resolve(&project, &one, &request(BookScope::Chapter, None), fmt);
        assert_eq!(chapter.scope, ResolvedBookScope::Chapter, "{fmt}");
        assert!(chapter.diagnostics.is_empty(), "{fmt}");
    }
}

#[test]
fn a_member_is_a_chapter_on_a_format_with_no_whole_book_output() {
    let (_g, project, root) = book();
    let one = root.join("one.qmd");
    for fmt in ["html", "docx", "pptx"] {
        for value in [BookScope::Book, BookScope::Chapter] {
            let got = resolve(&project, &one, &request(value, None), fmt);
            assert_eq!(got.scope, ResolvedBookScope::Chapter, "{fmt} {value:?}");
            assert!(got.diagnostics.is_empty(), "{fmt} {value:?}");
        }
    }
    // Only an explicit runtime `book` earns the notice, once.
    let asked = request(BookScope::Book, Some(LayerKind::Runtime));
    let got = resolve(&project, &one, &asked, "html");
    assert_eq!(got.scope, ResolvedBookScope::Chapter);
    assert_eq!(codes(&got.diagnostics), ["Q-5-42"]);
    // `chapter` is honoured and never warns.
    let chapter = request(BookScope::Chapter, Some(LayerKind::Runtime));
    assert!(
        resolve(&project, &one, &chapter, "html")
            .diagnostics
            .is_empty()
    );
}

#[test]
fn a_non_member_renders_alone_for_every_request_and_format() {
    let (_g, project, root) = book();
    let notes = root.join("notes.qmd");
    let requests = [
        (request(BookScope::Chapter, None), "Q-5-41"),
        (request(BookScope::Book, None), "Q-5-41"),
        (
            request(BookScope::Chapter, Some(LayerKind::Runtime)),
            "Q-5-41",
        ),
        (
            request(BookScope::Chapter, Some(LayerKind::Project)),
            "Q-5-41",
        ),
        (request(BookScope::Book, Some(LayerKind::Project)), "Q-5-41"),
        (request(BookScope::Book, Some(LayerKind::Runtime)), "Q-5-48"),
    ];
    for fmt in ["html", "typst", "epub", "docx"] {
        for (req, code) in &requests {
            let got = resolve(&project, &notes, req, fmt);
            assert_eq!(got.scope, ResolvedBookScope::NoBook, "{fmt} {req:?}");
            assert_eq!(codes(&got.diagnostics), [*code], "{fmt} {req:?}");
            assert_eq!(got.membership.map(|m| m.chapters), Some(3));
        }
    }
}

#[test]
fn a_project_that_is_not_a_book_is_no_book_and_silent() {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    write(&root, "_quarto.yml", "project:\n  type: default\n");
    write(&root, "a.qmd", "# A\n");
    let project = ProjectContext::discover(root.join("a.qmd"), &NativeRuntime::new()).unwrap();
    let asked = request(BookScope::Book, Some(LayerKind::Runtime));
    for fmt in ["html", "typst"] {
        let got = resolve(&project, &root.join("a.qmd"), &asked, fmt);
        assert_eq!(got.scope, ResolvedBookScope::NoBook, "{fmt}");
        assert!(got.diagnostics.is_empty(), "{fmt}");
        assert!(got.membership.is_none(), "{fmt}");
    }
}
