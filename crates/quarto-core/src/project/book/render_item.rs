/*
 * render_item.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Book render items: the ordered chapter/part/appendix list a book
 * project renders, with numbering assigned. Port of Q1's
 * `book-types.ts` (`BookRenderItem`) and `book-config.ts`
 * (`bookRenderItems`), plus the `isNumberedChapter` gate from
 * `book-shared.ts` / `pandoc-partition.ts`.
 */

use std::path::{Path, PathBuf};

use quarto_error_reporting::DiagnosticMessageBuilder;
use quarto_pandoc_types::config_value::ConfigValue;
use quarto_system_runtime::SystemRuntime;

use crate::error::{ParseError, QuartoError, Result};

/// What a single entry in a book's render list is.
///
/// Q1 types this as `"index" | "chapter" | "appendix" | "part"`, where
/// appendix *chapters* are typed `"chapter"` and only the synthetic
/// "Appendices" divider is `"appendix"`. Q2 gives appendix chapters the
/// `Appendix` kind directly (the divider is the `Appendix` item with
/// `file: None`), and adds `References` for the designated references
/// page — an explicit marker Q1 recovers later by scanning for the
/// first rendered file with a `#refs` div (`book-bibliography.ts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BookRenderItemKind {
    /// The book's home page (`index.*`), moved to the front of the render list.
    Index,
    /// An ordinary chapter.
    Chapter,
    /// The "Appendices" divider (`file: None`) or an appendix chapter (`file: Some`).
    Appendix,
    /// A part divider (`file: None` unless the part names an `href:`).
    Part,
    /// The designated references page (`book.references`).
    References,
}

/// One entry in a book's render list, in book order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookRenderItem {
    pub kind: BookRenderItemKind,
    /// Nesting depth: 0 for top-level chapters, 1+ for chapters inside parts (or
    /// appendix chapters, which Q1 nests under the synthetic "Appendices"
    /// divider at depth 1.
    pub depth: u32,
    /// Part/appendix divider title; `None` for ordinary chapters.
    pub text: Option<String>,
    /// Chapter source, relative to the project directory.
    pub file: Option<PathBuf>,
    /// Chapter number (1-based); `None` for unnumbered chapters and
    /// non-file items. Appendices get their own fresh 1-based sequence (Q1 presents
    /// these as letters elsewhere).
    pub number: Option<u32>,
}

impl BookRenderItem {
    /// All files this item names, if any.
    pub fn files<'a>(&'a self) -> impl Iterator<Item = &'a PathBuf> + 'a {
        self.file.iter()
    }
}

/// Build the book's render list from its `book:` config map.
///
/// Port of Q1's `bookRenderItems` (`book-config.ts`): chapters, then the
/// designated references page, then the appendix divider + appendix
/// chapters — with the `index.*` home page moved to the front and
/// re-kinded [`BookRenderItemKind::Index`].
///
/// Numbering is assigned in book order: chapters number 1, 2, …; the
/// references page continues the chapter sequence; appendices restart
/// at 1 in their own sequence (presented as letters elsewhere). An
/// `.unnumbered` chapter (front-matter title or first-heading class,
/// per [`chapter_is_numbered`]) consumes no slot and gets
/// `number: None` — including an individually-unnumbered *appendix*
/// chapter, which Q1's mechanism genuinely supports (the numbering
/// gate applies identically to appendix items; see design doc §4's
/// "cross-artifact risk" note).
///
/// Errors:
/// - `Q-5-35` — a listed chapter file does not exist on disk.
/// - `Q-5-36` — no `index.*` home page in the list.
/// - `Q-5-34` — a `part:` entry nested inside another part.
pub fn book_render_items(
    project_dir: &Path,
    book: &ConfigValue,
    appendices_title: &str,
    runtime: &dyn SystemRuntime,
) -> Result<Vec<BookRenderItem>> {
    let mut items: Vec<BookRenderItem> = Vec::new();
    let mut next_number: u32 = 1;

    // Chapters, numbered 1.. in a fresh sequence.
    if let Some(chapters) = book.get("chapters").and_then(|c| c.as_array()) {
        find_inputs(
            project_dir,
            chapters,
            BookRenderItemKind::Chapter,
            0,
            &mut next_number,
            &mut items,
            runtime,
        )?;
    }

    // The designated references page: its own kind, but numbered in the
    // *same* sequence as the chapters (Q1 resets `nextNumber` only at
    // each `findChapters` call, and references are processed between the
    // two calls).
    if let Some(references) = book.get("references") {
        let entries = [references.clone()];
        find_inputs(
            project_dir,
            &entries,
            BookRenderItemKind::References,
            0,
            &mut next_number,
            &mut items,
            runtime,
        )?;
    }

    // Appendices: fresh 1-based sequence (presented as letters downstream),
    // nested under a synthetic "Appendices" divider.
    if let Some(appendices) = book.get("appendices").and_then(|a| a.as_array()) {
        items.push(BookRenderItem {
            kind: BookRenderItemKind::Appendix,
            depth: 0,
            text: Some(format!("{appendices_title} {{.unnumbered}}")),
            file: None,
            number: None,
        });
        next_number = 1;
        find_inputs(
            project_dir,
            appendices,
            BookRenderItemKind::Appendix,
            1,
            &mut next_number,
            &mut items,
            runtime,
        )?;
    }

    // Move the index page to the front, error if there is none.
    let index_pos = items.iter().position(|item| {
        item.file
            .as_ref()
            .is_some_and(|f| f.to_string_lossy().starts_with("index."))
    });
    let index_pos = match index_pos {
        Some(pos) => pos,
        None => {
            return Err(book_diagnostic(
                "Q-5-36",
                "book has no home page",
                "A book project's chapter list must include a home page: a file \
                 whose name starts with `index.` (e.g. `index.qmd`).",
                "Add an `index.qmd` (or similarly named `index.*` file) to `book.chapters`.",
            ));
        }
    };
    let mut index = items.remove(index_pos);
    index.kind = BookRenderItemKind::Index;
    items.insert(0, index);

    Ok(items)
}

/// Walk one chapter list, pushing render items. Port of Q1's
/// `findInputs` — parts recurse at depth+1, files get existence-checked
/// and numbered.
///
/// Divergence from Q1: Q1's `findInputs` silently skips a file with no
/// execution engine (`fileExecutionEngine` returning undefined). Q2
/// accepts every listed file that exists — the render itself reports an
/// unrenderable input, which is the friendlier failure.
fn find_inputs(
    project_dir: &Path,
    entries: &[ConfigValue],
    kind: BookRenderItemKind,
    depth: u32,
    next_number: &mut u32,
    items: &mut Vec<BookRenderItem>,
    runtime: &dyn SystemRuntime,
) -> Result<()> {
    for entry in entries {
        // A part entry: `part:` + `chapters:` (→ section + contents in
        // the sidebar translation).
        if let Some(part) = entry.get("part") {
            let part_title = part
                .as_plain_text()
                .unwrap_or_else(|| "Untitled part".to_string());
            let href = entry.get("href").and_then(|h| h.as_plain_text());
            items.push(BookRenderItem {
                kind: BookRenderItemKind::Part,
                depth,
                text: Some(part_title),
                file: href.map(PathBuf::from),
                number: None,
            });
            if let Some(chapters) = entry.get("chapters").and_then(|c| c.as_array()) {
                // Nested parts are unsupported (Q-5-34); detect before recursing.
                for chapter in chapters {
                    if chapter.get("part").is_some() {
                        return Err(book_diagnostic(
                            "Q-5-34",
                            "book parts cannot be nested",
                            "A `part:` entry appears inside another part's `chapters:` \
                             list. Book projects support two levels only: parts, and \
                             chapters within parts.",
                            "Move the nested part to the top level of `book.chapters`, \
                             or flatten the structure.",
                        ));
                    }
                }
                find_inputs(
                    project_dir,
                    chapters,
                    kind,
                    depth + 1,
                    next_number,
                    items,
                    runtime,
                )?;
            }
            continue;
        }

        // A file entry: bare string or `{href: ...}` map. A bare string
        // of dashes is a sidebar divider, not a file — Q1 skips those
        // silently.
        let href = match entry.get("href").and_then(|h| h.as_plain_text()) {
            Some(h) => Some(h),
            None => entry.as_plain_text(),
        };
        let Some(href) = href else {
            // A text-only item: Q1 skips dash dividers and errors on
            // anything else; treat a non-string, non-map entry the same.
            continue;
        };
        if !href.trim().is_empty() && href.trim().chars().all(|c| c == '-') {
            continue;
        }

        // Existence check (Q1's `throwInputNotFound`).
        let full_path = project_dir.join(&href);
        let exists = runtime.is_file(&full_path).unwrap_or(false);
        if !exists {
            return Err(book_diagnostic(
                "Q-5-35",
                format!("book chapter `{href}` not found"),
                format!(
                    "The file `{href}` listed in `book.chapters`, `book.appendices`, or \
                     `book.references` does not exist in the project directory.",
                ),
                "Create the file, fix the path in `_quarto.yml`, or remove the entry.",
            ));
        }

        // Numbering gate (Q1's `inputIsNumbered`): the chapter's own
        // front-matter title or first heading decides. A read failure
        // leaves the chapter unnumbered, matching Q1's
        // `partitionedMarkdownForInput` failure path.
        let number = match runtime.file_read_string(&full_path) {
            Ok(content) if chapter_is_numbered(&content) => {
                let n = *next_number;
                *next_number += 1;
                Some(n)
            }
            _ => None,
        };

        items.push(BookRenderItem {
            kind,
            depth,
            text: entry.get("text").and_then(|t| t.as_plain_text()),
            file: Some(PathBuf::from(href)),
            number,
        });
    }
    Ok(())
}

/// Is this chapter numbered? Port of Q1's `isNumberedChapter`
/// (`book-shared.ts`) over the partition of the chapter's own source:
/// a front-matter `title:` string wins over the first heading, and in
/// either case a `{.unnumbered}` attr on the parsed title means
/// unnumbered.
///
/// Q1 parses the front-matter title through `parsePandocTitle`, whose
/// attr regex requires a leading `#` — a YAML title essentially never
/// carries one, so **a string front-matter title means numbered, full
/// stop**. This port reproduces that literal behavior.
pub fn chapter_is_numbered(content: &str) -> bool {
    let (front_matter, body) = partition_front_matter(content);
    if let Some(fm) = front_matter
        && let Some(title) = front_matter_title(&fm)
    {
        return !pandoc_attr_classes(&title)
            .is_some_and(|classes| classes.iter().any(|c| c == "unnumbered"));
    }
    match first_atx_heading_attr_classes(body) {
        Some(classes) => !classes.iter().any(|c| c == "unnumbered"),
        None => true,
    }
}

/// Split leading YAML front matter from the body. Returns
/// `(Some(yaml), body)` when a front-matter block is present, otherwise
/// `(None, content)`.
fn partition_front_matter(content: &str) -> (Option<String>, &str) {
    let Some(yaml) = crate::format::extract_yaml_frontmatter(content) else {
        return (None, content);
    };
    // Advance past the opening `---` line, the yaml, and the closing
    // terminator line to find the body start.
    let s = content.strip_prefix('\u{feff}').unwrap_or(content);
    let after_open = s
        .strip_prefix("---\n")
        .or_else(|| s.strip_prefix("---\r\n"))
        .unwrap_or(s);
    // `extract_yaml_frontmatter` found a terminator; locate its line end.
    for terminator in ["\n---", "\n..."] {
        if let Some(idx) = after_open.find(terminator) {
            let rest = &after_open[idx + 1..];
            // The terminator line runs to the next newline (or EOF).
            let body_start = rest.find('\n').map_or(rest.len(), |i| i + 1);
            return (Some(yaml), &rest[body_start..]);
        }
    }
    (None, content)
}

/// The front matter's `title:` as a plain string, if present and scalar.
fn front_matter_title(yaml: &str) -> Option<String> {
    let value: serde_yaml::Value = serde_yaml::from_str(yaml).ok()?;
    value.get("title")?.as_str().map(|s| s.to_string())
}

/// Attr classes of the first ATX heading in `body`, fence-aware so a
/// `#`-looking line inside a code block is never mistaken for the
/// heading. Port of Q1's `markdownWithExtractedHeading` (ATX arm only —
/// setext headings carry no attr, so they always read as numbered
/// either way) plus `parsePandocTitle`.
fn first_atx_heading_attr_classes(body: &str) -> Option<Vec<String>> {
    let mut fence: Option<(char, usize)> = None;
    for line in body.lines() {
        // Fence marker: up to 3 leading spaces (CommonMark), then a run
        // of 3+ backticks/tildes, all the same char.
        let indent = line.len() - line.trim_start_matches(' ').len();
        let marker = if indent <= 3 { &line[indent..] } else { "" };
        let run = marker
            .chars()
            .next()
            .filter(|c| *c == '`' || *c == '~')
            .map(|c| (c, marker.chars().take_while(|ch| *ch == c).count()));
        match fence {
            Some((fchar, flen)) => {
                // Closing fence: same char, run >= opener, nothing but
                // whitespace after.
                if let Some((c, n)) = run
                    && c == fchar
                    && n >= flen
                    && marker[n..].trim().is_empty()
                {
                    fence = None;
                }
                continue;
            }
            None => {
                if let Some((c, n)) = run
                    && n >= 3
                {
                    fence = Some((c, n));
                    continue;
                }
            }
        }
        // ATX heading (Q1's regex allows no leading spaces).
        let hashes = line.chars().take_while(|c| *c == '#').count();
        if hashes >= 1 && line.chars().nth(hashes).is_some_and(char::is_whitespace) {
            return pandoc_attr_classes(line);
        }
    }
    None
}

/// Attr classes parsed from a `# ... {attrs}` title line, port of Q1's
/// `parsePandocTitle` (`^\#{1,}\s(?:(.*)\s)?\{(.*)\}$`) plus the class
/// half of `pandocAttrParseText`. Returns `None` when the title carries
/// no parseable attr block (numbered, per Q1's `!attr?...` fallthrough).
fn pandoc_attr_classes(title: &str) -> Option<Vec<String>> {
    let t = title.trim();
    // Prefix: 1+ '#' then a whitespace char.
    let hashes = t.chars().take_while(|c| *c == '#').count();
    if hashes == 0 {
        return None;
    }
    let after = &t[hashes..];
    if !after.chars().next().is_some_and(char::is_whitespace) {
        return None;
    }
    let rest = after.trim_start();
    // Attr block: `{...}` at end of line, the `{` either at the start
    // or preceded by whitespace (the greedy `(?:(.*)\s)?` in Q1's
    // regex picks the rightmost such `{`).
    if !rest.ends_with('}') {
        return None;
    }
    let mut attr_raw: Option<&str> = None;
    for (i, c) in rest.char_indices().rev() {
        if c != '{' {
            continue;
        }
        let preceded_ok = i == 0
            || rest[..i]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace);
        if preceded_ok {
            attr_raw = Some(&rest[i + 1..rest.len() - 1]);
            break;
        }
    }
    let attr_raw = attr_raw?;
    // Classes: whitespace-separated tokens starting with '.'.
    let classes: Vec<String> = attr_raw
        .split_whitespace()
        .filter_map(|tok| tok.strip_prefix('.').map(|s| s.to_string()))
        .collect();
    Some(classes)
}

/// Build a coded error for the book chapter-list walk, with an empty
/// `SourceContext` (the config value's own span is attached by the
/// caller's diagnostic rendering when available).
pub(crate) fn book_diagnostic(
    code: &str,
    title: impl Into<String>,
    problem: impl Into<String>,
    hint: &str,
) -> QuartoError {
    let diagnostic = DiagnosticMessageBuilder::error(title.into())
        .with_code(code)
        .problem(problem.into())
        .add_hint(hint)
        .build();
    QuartoError::Parse(ParseError::new(
        vec![diagnostic],
        quarto_source_map::SourceContext::new(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::config_value::ConfigMapEntry;
    use quarto_source_map::{By, SourceInfo};
    use quarto_system_runtime::NativeRuntime;
    use std::path::Path;
    use tempfile::TempDir;

    fn si() -> SourceInfo {
        SourceInfo::generated(By::programmatic_config())
    }
    fn s(v: &str) -> ConfigValue {
        ConfigValue::new_string(v, si())
    }
    fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        ConfigValue::new_map(
            entries
                .into_iter()
                .map(|(k, v)| ConfigMapEntry {
                    key: k.to_string(),
                    key_source: si(),
                    value: v,
                })
                .collect(),
            si(),
        )
    }
    fn arr(items: Vec<ConfigValue>) -> ConfigValue {
        ConfigValue::new_array(items, si())
    }

    fn write(path: &Path, contents: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    /// A minimal book project dir with the given chapter files (`name` → content).
    fn book_dir(chapters: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        for (name, content) in chapters {
            write(&dir.path().join(name), content);
        }
        dir
    }

    fn render_items(dir: &Path, book: &ConfigValue) -> Result<Vec<BookRenderItem>> {
        let runtime = NativeRuntime::new();
        book_render_items(dir, book, "Appendices", &runtime)
    }

    #[test]
    fn nested_parts_produce_depth_tagged_dividers() {
        let dir = book_dir(&[
            ("index.qmd", "# Preface {.unnumbered}\n"),
            ("intro.qmd", "# Introduction\n"),
            ("chap1.qmd", "# One\n"),
            ("chap2.qmd", "# Two\n"),
            ("conclusion.qmd", "# Conclusion\n"),
        ]);
        let book = map(vec![(
            "chapters",
            arr(vec![
                s("index.qmd"),
                s("intro.qmd"),
                map(vec![
                    ("part", s("Part I")),
                    ("chapters", arr(vec![s("chap1.qmd"), s("chap2.qmd")])),
                ]),
                s("conclusion.qmd"),
            ]),
        )]);
        let items = render_items(dir.path(), &book).unwrap();

        let kinds: Vec<_> = items.iter().map(|i| (i.kind, i.depth)).collect();
        assert_eq!(
            kinds,
            vec![
                (BookRenderItemKind::Index, 0),
                (BookRenderItemKind::Chapter, 0),
                (BookRenderItemKind::Part, 0),
                (BookRenderItemKind::Chapter, 1),
                (BookRenderItemKind::Chapter, 1),
                (BookRenderItemKind::Chapter, 0),
            ]
        );
        // Part divider carries its title; the index was moved to front.
        assert_eq!(items[2].text.as_deref(), Some("Part I"));
        assert_eq!(items[0].file, Some(PathBuf::from("index.qmd")));
    }

    #[test]
    fn appendices_number_as_fresh_sequence_after_chapters() {
        let dir = book_dir(&[
            ("index.qmd", "# Preface {.unnumbered}\n"),
            ("intro.qmd", "# Introduction\n"),
            ("app-a.qmd", "# First Appendix\n"),
            ("app-b.qmd", "# Second Appendix\n"),
        ]);
        let book = map(vec![
            ("chapters", arr(vec![s("index.qmd"), s("intro.qmd")])),
            ("appendices", arr(vec![s("app-a.qmd"), s("app-b.qmd")])),
        ]);
        let items = render_items(dir.path(), &book).unwrap();

        // Chapters: intro numbered 1 (index unnumbered).
        let intro = items
            .iter()
            .find(|i| i.file == Some(PathBuf::from("intro.qmd")))
            .unwrap();
        assert_eq!(intro.number, Some(1));

        // Appendix divider, then appendix chapters numbered 1, 2 (fresh).
        let divider_pos = items
            .iter()
            .position(|i| i.kind == BookRenderItemKind::Appendix && i.file.is_none())
            .unwrap();
        let app_a = items
            .iter()
            .position(|i| i.file == Some(PathBuf::from("app-a.qmd")))
            .unwrap();
        let app_b = items
            .iter()
            .position(|i| i.file == Some(PathBuf::from("app-b.qmd")))
            .unwrap();
        assert!(
            divider_pos
                > items
                    .iter()
                    .position(|i| i.file == Some(PathBuf::from("intro.qmd")))
                    .unwrap()
        );
        assert!(app_a > divider_pos && app_b > app_a);
        assert_eq!(items[app_a].kind, BookRenderItemKind::Appendix);
        assert_eq!(items[app_a].number, Some(1));
        assert_eq!(items[app_b].number, Some(2));
        assert_eq!(items[app_a].depth, 1);
    }

    #[test]
    fn references_is_its_own_kind_and_continues_chapter_numbering() {
        let dir = book_dir(&[
            ("index.qmd", "# Preface {.unnumbered}\n"),
            ("intro.qmd", "# Introduction\n"),
            ("references.qmd", "# References\n"),
        ]);
        let book = map(vec![
            ("chapters", arr(vec![s("index.qmd"), s("intro.qmd")])),
            ("references", s("references.qmd")),
        ]);
        let items = render_items(dir.path(), &book).unwrap();

        let refs = items
            .iter()
            .find(|i| i.kind == BookRenderItemKind::References)
            .unwrap();
        assert_eq!(refs.file, Some(PathBuf::from("references.qmd")));
        // Continues the chapter sequence: intro is 1, references is 2.
        assert_eq!(refs.number, Some(2));
    }

    #[test]
    fn unnumbered_appendix_chapter_consumes_no_slot() {
        let dir = book_dir(&[
            ("index.qmd", "# Preface {.unnumbered}\n"),
            ("intro.qmd", "# Introduction\n"),
            ("app-a.qmd", "# First Appendix\n"),
            ("app-b.qmd", "# Errata {.unnumbered}\n"),
            ("app-c.qmd", "# Third Appendix\n"),
        ]);
        let book = map(vec![
            ("chapters", arr(vec![s("index.qmd"), s("intro.qmd")])),
            (
                "appendices",
                arr(vec![s("app-a.qmd"), s("app-b.qmd"), s("app-c.qmd")]),
            ),
        ]);
        let items = render_items(dir.path(), &book).unwrap();

        let num_of = |name: &str| {
            items
                .iter()
                .find(|i| i.file == Some(PathBuf::from(name)))
                .unwrap()
                .number
        };
        assert_eq!(num_of("app-a.qmd"), Some(1));
        assert_eq!(num_of("app-b.qmd"), None);
        assert_eq!(num_of("app-c.qmd"), Some(2));
    }

    #[test]
    fn missing_chapter_file_is_q_5_35() {
        let dir = book_dir(&[("index.qmd", "# Preface {.unnumbered}\n")]);
        let book = map(vec![("chapters", arr(vec![s("index.qmd"), s("nope.qmd")]))]);
        let err = render_items(dir.path(), &book).unwrap_err();
        let text = format!("{err}");
        assert!(text.contains("Q-5-35"), "expected Q-5-35, got: {text}");
        assert!(text.contains("nope.qmd"), "expected filename, got: {text}");
    }

    #[test]
    fn no_index_page_is_q_5_36() {
        let dir = book_dir(&[("intro.qmd", "# Introduction\n")]);
        let book = map(vec![("chapters", arr(vec![s("intro.qmd")]))]);
        let err = render_items(dir.path(), &book).unwrap_err();
        let text = format!("{err}");
        assert!(text.contains("Q-5-36"), "expected Q-5-36, got: {text}");
    }

    #[test]
    fn nested_part_is_q_5_34() {
        let dir = book_dir(&[
            ("index.qmd", "# Preface {.unnumbered}\n"),
            ("chap1.qmd", "# One\n"),
            ("chap2.qmd", "# Two\n"),
        ]);
        let book = map(vec![(
            "chapters",
            arr(vec![
                s("index.qmd"),
                map(vec![
                    ("part", s("Outer")),
                    (
                        "chapters",
                        arr(vec![
                            s("chap1.qmd"),
                            map(vec![
                                ("part", s("Inner")),
                                ("chapters", arr(vec![s("chap2.qmd")])),
                            ]),
                        ]),
                    ),
                ]),
            ]),
        )]);
        let err = render_items(dir.path(), &book).unwrap_err();
        let text = format!("{err}");
        assert!(text.contains("Q-5-34"), "expected Q-5-34, got: {text}");
    }

    // === chapter_is_numbered (port of Q1 isNumberedChapter) ===

    #[test]
    fn unnumbered_heading_class_marks_chapter_unnumbered() {
        assert!(!chapter_is_numbered("# Preface {.unnumbered}\n\nBody.\n"));
    }

    #[test]
    fn plain_heading_is_numbered() {
        assert!(chapter_is_numbered("# Introduction\n\nBody.\n"));
    }

    #[test]
    fn front_matter_title_wins_and_is_numbered() {
        // Q1 literal behavior: a string front-matter title is parsed
        // with a `#`-requiring regex, so it never carries attrs →
        // numbered, even though the first heading is unnumbered.
        assert!(chapter_is_numbered(
            "---\ntitle: Preface\n---\n\n# Preface {.unnumbered}\n"
        ));
    }

    #[test]
    fn fenced_code_does_not_hide_the_real_heading() {
        assert!(!chapter_is_numbered(
            "```markdown\n# Fake {.unnumbered}\n```\n\n# Real {.unnumbered}\n"
        ));
        assert!(chapter_is_numbered(
            "```markdown\n# Fake {.unnumbered}\n```\n\n# Real\n"
        ));
    }

    #[test]
    fn heading_with_other_classes_is_numbered() {
        assert!(chapter_is_numbered("# Intro {#sec-intro .foo}\n"));
    }

    #[test]
    fn no_heading_at_all_is_numbered() {
        assert!(chapter_is_numbered("Just some text.\n"));
    }
}
