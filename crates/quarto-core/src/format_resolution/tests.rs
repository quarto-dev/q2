use super::*;
use crate::metadata_layers::test_runtime::MetaRuntime;
use serde_json::json;
use std::collections::HashSet;
use std::path::PathBuf;
use tempfile::TempDir;

fn s(keys: &[&str]) -> Vec<String> {
    keys.iter().map(|k| k.to_string()).collect()
}

// ── select_formats ───────────────────────────────────────────────────────

#[test]
fn nothing_declared_falls_back_to_html() {
    let sel = select_formats(FormatInputs::default());
    assert_eq!(sel.selected, "html");
    assert!(sel.keys.is_empty());
    assert_eq!(sel.source, None);
    assert!(!sel.requested);
}

#[test]
fn the_first_key_is_selected_and_every_key_returned() {
    let project = s(&["typst", "html"]);
    let sel = select_formats(FormatInputs {
        project_keys: &project,
        ..Default::default()
    });
    assert_eq!(sel.selected, "typst");
    assert_eq!(sel.keys, project);
    assert_eq!(sel.source, Some(LayerKind::Project));
}

#[test]
fn document_keys_replace_the_projects_list() {
    let project = s(&["typst"]);
    let doc = s(&["revealjs"]);
    let sel = select_formats(FormatInputs {
        project_keys: &project,
        document_keys: &doc,
        ..Default::default()
    });
    assert_eq!(sel.selected, "revealjs");
    assert_eq!(sel.keys, doc);
    assert_eq!(sel.source, Some(LayerKind::Document));
}

#[test]
fn document_keys_come_before_directory_keys() {
    let dir = s(&["pdf", "html"]);
    let doc = s(&["docx"]);
    let sel = select_formats(FormatInputs {
        directory_keys: &dir,
        document_keys: &doc,
        ..Default::default()
    });
    assert_eq!(sel.keys, s(&["docx", "pdf", "html"]));
    assert_eq!(sel.selected, "docx");
}

#[test]
fn directory_keys_alone_replace_the_projects_list() {
    let project = s(&["html"]);
    let dir = s(&["typst"]);
    let sel = select_formats(FormatInputs {
        project_keys: &project,
        directory_keys: &dir,
        ..Default::default()
    });
    assert_eq!(sel.selected, "typst");
    assert_eq!(sel.source, Some(LayerKind::Directory));
}

#[test]
fn the_projects_list_applies_when_neither_declares_one() {
    let project = s(&["epub"]);
    let sel = select_formats(FormatInputs {
        project_keys: &project,
        ..Default::default()
    });
    assert_eq!(sel.selected, "epub");
}

#[test]
fn project_formats_only_ignores_document_and_directory_keys() {
    let project = s(&["html"]);
    let dir = s(&["pdf"]);
    let doc = s(&["typst"]);
    let sel = select_formats(FormatInputs {
        project_keys: &project,
        directory_keys: &dir,
        document_keys: &doc,
        project_formats_only: true,
        ..Default::default()
    });
    assert_eq!(sel.selected, "html");
    assert_eq!(sel.keys, project);
}

#[test]
fn a_requested_format_wins_and_may_be_undeclared() {
    let doc = s(&["html", "typst"]);
    let sel = select_formats(FormatInputs {
        document_keys: &doc,
        cli_to: Some("docx"),
        ..Default::default()
    });
    assert_eq!(sel.selected, "docx");
    assert_eq!(sel.keys, doc, "the declared list stays");
    assert!(sel.requested);
}

#[test]
fn a_runtime_scalar_selects_a_non_first_key() {
    let doc = s(&["html", "typst"]);
    let sel = select_formats(FormatInputs {
        document_keys: &doc,
        runtime_format: Some("typst"),
        ..Default::default()
    });
    assert_eq!(sel.selected, "typst");
}

#[test]
fn to_beats_a_runtime_scalar() {
    let sel = select_formats(FormatInputs {
        cli_to: Some("docx"),
        runtime_format: Some("typst"),
        ..Default::default()
    });
    assert_eq!(sel.selected, "docx");
}

#[test]
fn the_requested_format_applies_even_to_project_formats_only() {
    let project = s(&["html"]);
    let sel = select_formats(FormatInputs {
        project_keys: &project,
        project_formats_only: true,
        cli_to: Some("typst"),
        ..Default::default()
    });
    assert_eq!(sel.selected, "typst");
}

// ── format_keys_of (ConfigValue) ─────────────────────────────────────────

fn keys_of_yaml(front: &str) -> Vec<String> {
    let layer =
        crate::metadata_layers::document_layer_from_content(&format!("---\n{front}\n---\n"));
    layer.get("format").map(format_keys_of).unwrap_or_default()
}

#[test]
fn scalar_and_map_keys_are_read_in_order() {
    assert_eq!(keys_of_yaml("format: typst"), s(&["typst"]));
    assert_eq!(
        keys_of_yaml("format:\n  docx: default\n  html:\n    toc: true"),
        s(&["docx", "html"])
    );
}

#[test]
fn null_and_false_entries_are_skipped() {
    assert_eq!(
        keys_of_yaml("format:\n  html: null\n  typst: default\n  pdf: false"),
        s(&["typst"])
    );
    assert!(keys_of_yaml("format: false").is_empty());
    assert!(keys_of_yaml("format: null").is_empty());
}

// ── requested_book_scope and its adapters ────────────────────────────────

#[test]
fn the_string_function_accepts_book_and_chapter_only() {
    assert_eq!(requested_book_scope(None), Ok(None));
    assert_eq!(
        requested_book_scope(Some("book")),
        Ok(Some(BookScope::Book))
    );
    assert_eq!(
        requested_book_scope(Some("chapter")),
        Ok(Some(BookScope::Chapter))
    );
    assert!(requested_book_scope(Some("Book")).is_err());
    let none = requested_book_scope(Some("none")).unwrap_err();
    assert!(none.message().contains("output"), "{}", none.message());
}

#[test]
fn the_json_adapter_reads_the_top_level_key_only() {
    assert_eq!(
        book_scope_in_json(&json!({"book-scope": "book"})),
        Ok(Some(BookScope::Book))
    );
    assert_eq!(book_scope_in_json(&json!({})), Ok(None));
    assert_eq!(
        book_scope_in_json(&json!({"format": {"typst": {"book-scope": "book"}}})),
        Ok(None)
    );
    assert!(book_scope_in_json(&json!({"book-scope": true})).is_err());
    assert!(book_scope_in_json(&json!({"book-scope": "none"})).is_err());
}

#[test]
fn the_config_adapter_reads_the_top_level_key_only() {
    let doc = |front: &str| crate::metadata_layers::document_layer_from_content(front);
    assert_eq!(
        book_scope_in_config(&doc("---\nbook-scope: chapter\n---\n")),
        Ok(Some(BookScope::Chapter))
    );
    assert_eq!(
        book_scope_in_config(&doc("---\nformat:\n  typst:\n    book-scope: book\n---\n")),
        Ok(None)
    );
    assert!(book_scope_in_config(&doc("---\nbook-scope: [a]\n---\n")).is_err());
    assert!(book_scope_in_config(&doc("---\nbook-scope: none\n---\n")).is_err());
}

// ── fixtures ─────────────────────────────────────────────────────────────

fn write(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

struct Fixture {
    _dir: TempDir,
    root: PathBuf,
}

impl Fixture {
    fn new(quarto_yml: &str, files: &[(&str, &str)]) -> Self {
        let dir = TempDir::new().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        write(&root.join("_quarto.yml"), quarto_yml);
        for (name, content) in files {
            write(&root.join(name), content);
        }
        Fixture { _dir: dir, root }
    }

    fn project(&self, runtime: &dyn SystemRuntime) -> ProjectContext {
        ProjectContext::discover(self.root.join("index.qmd"), runtime).unwrap()
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
}

const BOOK: &str =
    "project:\n  type: book\nbook:\n  title: T\n  chapters:\n    - index.qmd\n    - ch1.qmd\n";
const WEBSITE: &str = "project:\n  type: website\n";

fn ch(front: &str) -> String {
    format!("---\n{front}\n---\n\n# H\n")
}

fn active(f: &Fixture, name: &str) -> RenderMode {
    RenderMode::ActivePage(f.path(name))
}

fn subset(f: &Fixture, names: &[&str]) -> RenderMode {
    RenderMode::Subset(names.iter().map(|n| f.path(n)).collect::<HashSet<_>>())
}

fn codes(r: &ScopeRequest) -> Vec<String> {
    r.diagnostics
        .iter()
        .filter_map(|d| d.code.clone())
        .collect()
}

// ── resolve_render_scope ─────────────────────────────────────────────────

#[test]
fn the_target_sets_the_default_scope() {
    let f = Fixture::new(
        BOOK,
        &[
            ("index.qmd", "# I\n"),
            ("ch1.qmd", "# C\n"),
            ("ch2.qmd", "# D\n"),
        ],
    );
    let rt = MetaRuntime::new(None);
    let p = f.project(&rt);
    let full = resolve_render_scope(&p, &RenderMode::Full, &rt);
    assert_eq!((full.value, full.explicit), (BookScope::Book, None));
    let one = resolve_render_scope(&p, &active(&f, "ch1.qmd"), &rt);
    assert_eq!((one.value, one.explicit), (BookScope::Chapter, None));
    let two = resolve_render_scope(&p, &subset(&f, &["ch1.qmd", "ch2.qmd"]), &rt);
    assert_eq!((two.value, two.explicit), (BookScope::Chapter, None));
    assert!(
        full.diagnostics.is_empty() && one.diagnostics.is_empty() && two.diagnostics.is_empty()
    );
}

#[test]
fn precedence_is_runtime_over_document_over_directory_over_project() {
    let quarto = format!("{BOOK}book-scope: chapter\n");
    let f = Fixture::new(
        &quarto,
        &[
            ("index.qmd", "# I\n"),
            ("sub/_metadata.yml", "book-scope: book\n"),
            ("sub/ch1.qmd", "# C\n"),
            ("doc.qmd", &ch("book-scope: book")),
        ],
    );
    let rt = MetaRuntime::new(None);
    let p = f.project(&rt);
    // project only
    let r = resolve_render_scope(&p, &active(&f, "index.qmd"), &rt);
    assert_eq!(
        (r.value, r.explicit),
        (BookScope::Chapter, Some(LayerKind::Project))
    );
    // directory over project
    let r = resolve_render_scope(&p, &active(&f, "sub/ch1.qmd"), &rt);
    assert_eq!(
        (r.value, r.explicit),
        (BookScope::Book, Some(LayerKind::Directory))
    );
    // document over project
    let r = resolve_render_scope(&p, &active(&f, "doc.qmd"), &rt);
    assert_eq!(
        (r.value, r.explicit),
        (BookScope::Book, Some(LayerKind::Document))
    );
    // runtime over document
    let rt = MetaRuntime::new(Some(json!({"book-scope": "chapter"})));
    let r = resolve_render_scope(&p, &active(&f, "doc.qmd"), &rt);
    assert_eq!(
        (r.value, r.explicit),
        (BookScope::Chapter, Some(LayerKind::Runtime))
    );
}

#[test]
fn document_and_directory_layers_count_only_for_one_active_document() {
    let f = Fixture::new(
        BOOK,
        &[
            ("index.qmd", &ch("book-scope: chapter")),
            ("ch1.qmd", &ch("book-scope: chapter")),
        ],
    );
    let rt = MetaRuntime::new(None);
    let p = f.project(&rt);
    let full = resolve_render_scope(&p, &RenderMode::Full, &rt);
    assert_eq!((full.value, full.explicit), (BookScope::Book, None));
    assert!(full.diagnostics.is_empty(), "{:?}", codes(&full));
    let two = resolve_render_scope(&p, &subset(&f, &["index.qmd", "ch1.qmd"]), &rt);
    assert_eq!((two.value, two.explicit), (BookScope::Chapter, None));
}

#[test]
fn a_book_scope_under_format_is_not_read_and_warns() {
    let f = Fixture::new(
        BOOK,
        &[(
            "index.qmd",
            &ch("format:\n  typst:\n    book-scope: chapter"),
        )],
    );
    let rt = MetaRuntime::new(None);
    let p = f.project(&rt);
    let r = resolve_render_scope(&p, &active(&f, "index.qmd"), &rt);
    assert_eq!((r.value, r.explicit), (BookScope::Chapter, None));
    assert_eq!(codes(&r), ["Q-5-44"]);
    // And under `Full`, where the document layer is not read, the value is
    // still not seen (a project-level one is).
    let quarto = format!("{BOOK}format:\n  typst:\n    book-scope: chapter\n");
    let f = Fixture::new(&quarto, &[("index.qmd", "# I\n")]);
    let p = f.project(&rt);
    let r = resolve_render_scope(&p, &RenderMode::Full, &rt);
    assert_eq!((r.value, r.explicit), (BookScope::Book, None));
    assert_eq!(codes(&r), ["Q-5-44"]);
}

#[test]
fn an_invalid_value_warns_and_the_next_layer_applies() {
    let quarto = format!("{BOOK}book-scope: chapter\n");
    let f = Fixture::new(&quarto, &[("index.qmd", &ch("book-scope: none"))]);
    let rt = MetaRuntime::new(None);
    let p = f.project(&rt);
    let r = resolve_render_scope(&p, &active(&f, "index.qmd"), &rt);
    assert_eq!(
        (r.value, r.explicit),
        (BookScope::Chapter, Some(LayerKind::Project))
    );
    assert_eq!(codes(&r), ["Q-5-43"]);
    assert!(
        r.diagnostics[0]
            .problem
            .as_ref()
            .is_some_and(|p| p.as_str().contains("output")),
        "the message says `none` is output only"
    );
    // A non-string runtime value is invalid too.
    let rt = MetaRuntime::new(Some(json!({"book-scope": true})));
    let r = resolve_render_scope(&p, &active(&f, "index.qmd"), &rt);
    assert!(codes(&r).contains(&"Q-5-43".to_string()));
}

#[test]
fn a_non_runtime_chapter_is_ignored_in_a_full_render() {
    let quarto = format!("{BOOK}book-scope: chapter\n");
    let f = Fixture::new(&quarto, &[("index.qmd", "# I\n")]);
    let rt = MetaRuntime::new(None);
    let p = f.project(&rt);
    let r = resolve_render_scope(&p, &RenderMode::Full, &rt);
    assert_eq!((r.value, r.explicit), (BookScope::Book, None));
    assert_eq!(codes(&r), ["Q-5-45"]);
    assert!(r.error().is_none());
}

#[test]
fn book_scope_in_a_website_is_an_info() {
    let f = Fixture::new(
        &format!("{WEBSITE}book-scope: book\n"),
        &[("index.qmd", "# I\n")],
    );
    let rt = MetaRuntime::new(None);
    let p = f.project(&rt);
    let r = resolve_render_scope(&p, &active(&f, "index.qmd"), &rt);
    assert_eq!(codes(&r), ["Q-5-46"]);
    assert!(r.error().is_none());
}

#[test]
fn a_runtime_chapter_with_full_is_an_error_only_for_a_book() {
    let rt = MetaRuntime::new(Some(json!({"book-scope": "chapter"})));
    let f = Fixture::new(BOOK, &[("index.qmd", "# I\n")]);
    let p = f.project(&rt);
    let r = resolve_render_scope(&p, &RenderMode::Full, &rt);
    let err = r.error().expect("the FR6 error");
    assert_eq!(err.code.as_deref(), Some("Q-5-47"));
    assert!(err.problem.as_ref().is_some_and(|p| {
        p.as_str()
            .contains("needs a file to render: name the chapter(s)")
    }));

    let f = Fixture::new(WEBSITE, &[("index.qmd", "# I\n")]);
    let p = f.project(&rt);
    let r = resolve_render_scope(&p, &RenderMode::Full, &rt);
    assert!(r.error().is_none());
    assert_eq!(r.value, BookScope::Chapter);
    assert_eq!(codes(&r), ["Q-5-46"]);
    // A standalone document asked as `ActivePage`: info, chapter.
    let r = resolve_render_scope(&p, &active(&f, "index.qmd"), &rt);
    assert!(r.error().is_none());
    assert_eq!(r.value, BookScope::Chapter);
}

#[test]
fn a_runtime_book_with_one_file_requests_the_book() {
    let rt = MetaRuntime::new(Some(json!({"book-scope": "book"})));
    let f = Fixture::new(BOOK, &[("index.qmd", "# I\n"), ("ch1.qmd", "# C\n")]);
    let p = f.project(&rt);
    let r = resolve_render_scope(&p, &active(&f, "ch1.qmd"), &rt);
    assert_eq!(
        (r.value, r.explicit),
        (BookScope::Book, Some(LayerKind::Runtime))
    );
    assert!(r.diagnostics.is_empty());
}

// ── resolve_document_format and resolve_project_format ───────────────────

fn chapter_format(f: &Fixture, rt: &dyn SystemRuntime, name: &str, to: Option<&str>) -> String {
    let p = f.project(rt);
    resolve_document_format(&p, &f.path(name), None, &ScopeRequest::chapter(), to, rt).key
}

#[test]
fn a_document_wins_in_chapter_scope_and_the_project_in_book_scope() {
    let quarto = format!("{BOOK}format: html\n");
    let f = Fixture::new(
        &quarto,
        &[("index.qmd", "# I\n"), ("ch1.qmd", &ch("format: typst"))],
    );
    let rt = MetaRuntime::new(None);
    let p = f.project(&rt);
    let chapter = ScopeRequest::chapter();
    let book = ScopeRequest {
        value: BookScope::Book,
        explicit: None,
        diagnostics: Vec::new(),
    };
    let doc = f.path("ch1.qmd");
    assert_eq!(
        resolve_document_format(&p, &doc, None, &chapter, None, &rt).key,
        "typst"
    );
    assert_eq!(
        resolve_document_format(&p, &doc, None, &book, None, &rt).key,
        "html"
    );
    assert_eq!(
        resolve_document_format(&p, &doc, None, &book, Some("epub"), &rt).key,
        "epub",
        "the requested format applies to every member"
    );
    // A website is never project-formats-only.
    let f = Fixture::new(
        &format!("{WEBSITE}format: html\n"),
        &[
            ("index.qmd", "# I\n"),
            ("deck.qmd", &ch("format: revealjs")),
        ],
    );
    let p = f.project(&rt);
    let r = resolve_document_format(&p, &f.path("deck.qmd"), None, &book, None, &rt);
    assert_eq!(r.key, "revealjs");
}

#[test]
fn a_project_only_format_applies_to_a_document_with_none() {
    let f = Fixture::new(
        &format!("{WEBSITE}format: typst\n"),
        &[("index.qmd", "# I\n")],
    );
    let rt = MetaRuntime::new(None);
    assert_eq!(chapter_format(&f, &rt, "index.qmd", None), "typst");
}

#[test]
fn a_content_override_replaces_the_disk_copy() {
    let f = Fixture::new(
        &format!("{WEBSITE}format: html\n"),
        &[("index.qmd", &ch("format: typst"))],
    );
    let rt = MetaRuntime::new(None);
    let p = f.project(&rt);
    let r = resolve_document_format(
        &p,
        &f.path("index.qmd"),
        Some(&ch("format: revealjs")),
        &ScopeRequest::chapter(),
        None,
        &rt,
    );
    assert_eq!(r.key, "revealjs");
}

#[test]
fn nested_metadata_files_merge_before_their_keys_are_taken() {
    let f = Fixture::new(
        &format!("{WEBSITE}format: html\n"),
        &[
            ("index.qmd", "# I\n"),
            (
                "p/_metadata.yml",
                "format:\n  html: default\n  pdf: default\n",
            ),
            ("p/a/_metadata.yml", "format: typst\n"),
            ("p/a/doc.qmd", "# D\n"),
            ("p/b/doc.qmd", "# D\n"),
        ],
    );
    let rt = MetaRuntime::new(None);
    let p = f.project(&rt);
    let chapter = ScopeRequest::chapter();
    let r = resolve_document_format(&p, &f.path("p/a/doc.qmd"), None, &chapter, None, &rt);
    assert_eq!(r.key, "typst");
    assert_eq!(r.all_keys, s(&["typst"]), "the child's key, not a union");
    let r = resolve_document_format(&p, &f.path("p/b/doc.qmd"), None, &chapter, None, &rt);
    assert_eq!(r.all_keys, s(&["html", "pdf"]));
    assert_eq!(r.source, Some(LayerKind::Directory));
}

#[test]
fn a_requested_typst_pdf_is_the_typst_format() {
    let f = Fixture::new(WEBSITE, &[("index.qmd", "# I\n")]);
    let rt = MetaRuntime::new(None);
    assert_eq!(
        chapter_format(&f, &rt, "index.qmd", Some("typst-pdf")),
        "typst"
    );
}

#[test]
fn a_runtime_scalar_selects_a_non_first_key_and_loses_to_to() {
    let f = Fixture::new(
        WEBSITE,
        &[(
            "index.qmd",
            &ch("format:\n  html: default\n  typst: default"),
        )],
    );
    let rt = MetaRuntime::new(Some(json!({"format": "typst"})));
    assert_eq!(chapter_format(&f, &rt, "index.qmd", None), "typst");
    assert_eq!(chapter_format(&f, &rt, "index.qmd", Some("docx")), "docx");
}

#[test]
fn a_runtime_map_never_selects() {
    let f = Fixture::new(
        &format!("{WEBSITE}format: typst\n"),
        &[("index.qmd", "# I\n")],
    );
    let rt = MetaRuntime::new(Some(json!({"format": {"html": {"toc": true}}})));
    assert_eq!(chapter_format(&f, &rt, "index.qmd", None), "typst");
}

#[test]
fn the_project_format_is_the_projects_keys_plus_the_requested_format() {
    let f = Fixture::new(
        &format!("{BOOK}format: typst\n"),
        &[("index.qmd", "# I\n"), ("ch1.qmd", &ch("format: html"))],
    );
    let rt = MetaRuntime::new(None);
    let p = f.project(&rt);
    let req = ScopeRequest::chapter();
    assert_eq!(resolve_project_format(&p, &req, None, &rt).key, "typst");
    assert_eq!(
        resolve_project_format(&p, &req, Some("epub"), &rt).key,
        "epub"
    );
    let rt = MetaRuntime::new(Some(json!({"format": "docx"})));
    assert_eq!(resolve_project_format(&p, &req, None, &rt).key, "docx");
    let r = resolve_project_format(
        &f.project(&MetaRuntime::new(None)),
        &req,
        None,
        &MetaRuntime::new(None),
    );
    assert_eq!(r.all_keys, s(&["typst"]));
}

#[test]
fn key_or_uses_the_callers_default_only_when_nothing_decided() {
    let f = Fixture::new(WEBSITE, &[("index.qmd", "# I\n")]);
    let rt = MetaRuntime::new(None);
    let p = f.project(&rt);
    let r = resolve_document_format(
        &p,
        &f.path("index.qmd"),
        None,
        &ScopeRequest::chapter(),
        None,
        &rt,
    );
    assert_eq!(r.key_or("q2-preview"), "q2-preview");
    let r = resolve_document_format(
        &p,
        &f.path("index.qmd"),
        None,
        &ScopeRequest::chapter(),
        Some("typst"),
        &rt,
    );
    assert_eq!(r.key_or("q2-preview"), "typst");
}

// ── per-keystroke cost ───────────────────────────────────────────────────

/// The resolver is called per keystroke with the editor buffer, so it must
/// read the front matter only. By construction: the module reads documents
/// through `document_layer_from_content` and never names a document parser.
#[test]
fn the_resolver_never_runs_the_document_parser() {
    let source = include_str!("../format_resolution.rs");
    let production = source.split("#[cfg(test)]").next().unwrap();
    for parser in ["read_qmd", "parse_qmd", "pampa::readers", "readers::qmd"] {
        assert!(
            !production.contains(parser),
            "format_resolution.rs must not call the document parser ({parser})"
        );
    }
    assert!(production.contains("DocumentLayer::Content"));
}

/// A body the markdown parser would spend its time on (or reject) does not
/// change the answer or its cost: only the front matter is read.
#[test]
fn the_body_of_the_buffer_is_never_read() {
    let f = Fixture::new(WEBSITE, &[("index.qmd", "# I\n")]);
    let rt = MetaRuntime::new(None);
    let p = f.project(&rt);
    let body = "```{python\n:::{{{ [unclosed `\n".repeat(20_000);
    let content = format!("---\nformat: typst\n---\n\n{body}");
    let start = std::time::Instant::now();
    let r = resolve_document_format(
        &p,
        &f.path("index.qmd"),
        Some(&content),
        &ScopeRequest::chapter(),
        None,
        &rt,
    );
    let elapsed = start.elapsed();
    assert_eq!(r.key, "typst");
    eprintln!(
        "resolve_document_format on a {} byte buffer: {elapsed:?}",
        content.len()
    );
}
