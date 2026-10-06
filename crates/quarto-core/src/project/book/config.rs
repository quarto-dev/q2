/*
 * config.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Book config translation: `book.*` → `website.*`, the chapter list →
 * `website.sidebar.contents` (with baked chapter-number decoration),
 * download/sharing sidebar tools, book-wide format defaults, and
 * special-date resolution. Port of Q1's `book-config.ts`
 * (`bookProjectConfig`, `bookChaptersToSidebarItems`,
 * `chapterToSidebarItem`, `downloadTools`, `sharingTools`),
 * `book-chapters.ts` (`numberChapterHtmlNav`, `chapterInfoForInput`),
 * `book-shared.ts` (`bookOutputStem`), and `book.ts` (`bookPreRender`,
 * `formatExtras`' format-agnostic defaults).
 */

use std::path::{Path, PathBuf};

use hashlink::LinkedHashMap;
use quarto_pandoc_types::config_value::{ConfigMapEntry, ConfigValue};
use quarto_pandoc_types::inline::{Inline, Space, Span, Str};
use quarto_pandoc_types::{AttrSourceInfo, Inlines};
use quarto_source_map::{By, SourceInfo};
use quarto_system_runtime::SystemRuntime;

use crate::dates::ParsedDate;
use crate::project::book::render_item::{BookRenderItem, BookRenderItemKind};
use crate::project::index::ProjectIndex;

fn gen_si() -> SourceInfo {
    SourceInfo::generated(By::programmatic_config())
}

fn new_map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
    ConfigValue::new_map(
        entries
            .into_iter()
            .map(|(k, v)| ConfigMapEntry {
                key: k.to_string(),
                key_source: gen_si(),
                value: v,
            })
            .collect(),
        gen_si(),
    )
}

/// Keys Q1's `bookProjectConfig` copies from `book.*` into `website.*`
/// verbatim (book-config.ts). `page-navigation` is *not* in this list:
/// Q1 sets it unconditionally (`!== false`), handled separately.
const BOOK_TO_SITE_KEYS: &[&str] = &[
    "title",
    "favicon",
    "site-url",
    "site-path",
    "repo-url",
    "repo-link-target",
    "repo-link-rel",
    "repo-subdir",
    "repo-branch",
    "repo-actions",
    "issue-url",
    "navbar",
    "sidebar",
    "open-graph",
    "twitter-card",
    "image",
    "image-alt",
    "margin-header",
    "margin-footer",
    "body-header",
    "body-footer",
    "search",
    "reader-mode",
    "google-analytics",
    "plausible-analytics",
    "cookie-consent",
    "announcement",
    "back-to-top-navigation",
    "llms-txt",
    "comments",
    "bread-crumbs",
    "other-links",
    "code-links",
];

/// Copy the `book.*` keys a book shares with websites into the
/// `website` map (present keys only; book wins), plus Q1's unconditional
/// `page-navigation: true` default. Port of the first half of Q1's
/// `bookProjectConfig`.
///
/// `meta` is the full project metadata (`_quarto.yml` as a ConfigValue).
pub fn copy_book_keys_to_website(meta: &mut ConfigValue) {
    let Some(book) = meta.get("book").cloned() else {
        return;
    };
    // Ensure a `website` map exists.
    if meta.get("website").is_none() {
        meta.insert_path(&["website"], new_map(vec![]));
    }
    let mut site = meta.get("website").unwrap().clone();
    for key in BOOK_TO_SITE_KEYS {
        if let Some(value) = book.get(key) {
            site.insert_path(&[key], value.clone());
        }
    }
    if let Some(value) = book.get("page-footer") {
        site.insert_path(&["page-footer"], value.clone());
    }
    // Q1: `site[kSitePageNavigation] = book[kSitePageNavigation] !== false`
    // — set even when the book never mentions the key.
    let page_nav = book
        .get("page-navigation")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    site.insert_path(
        &["page-navigation"],
        ConfigValue::new_bool(page_nav, gen_si()),
    );
    meta.insert_path(&["website"], site);
}

/// The label prefix used in sidebar decoration for a render item:
/// the chapter number for chapters/references, the letter form
/// (`A`, `B`, …) for appendix chapters. `None` for unnumbered items.
/// Port of Q1's `chapterInfoForInput` label-prefix computation.
pub fn chapter_label_prefix(item: &BookRenderItem) -> Option<String> {
    let number = item.number?;
    match item.kind {
        // Q1: `String.fromCharCode(64 + number)` — 1 → 'A', 2 → 'B', …
        BookRenderItemKind::Appendix => {
            Some(char::from_u32(64 + number).map_or_else(|| number.to_string(), |c| c.to_string()))
        }
        _ => Some(number.to_string()),
    }
}

/// The decorated sidebar text for a numbered chapter: Q1's
/// `numberChapterHtmlNav` HTML
/// (`<span class='chapter-number'>N</span>&nbsp; <span class='chapter-title'>T</span>`)
/// expressed as Pandoc inlines, which Q2's sidebar renderer round-trips
/// verbatim (design doc §4).
pub fn decorated_chapter_text(prefix: &str, title: &str) -> ConfigValue {
    let span = |class: &str, text: &str| {
        Inline::Span(Span {
            attr: (String::new(), vec![class.to_string()], LinkedHashMap::new()),
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: gen_si(),
            })],
            source_info: gen_si(),
            attr_source: AttrSourceInfo::empty(),
        })
    };
    let inlines: Inlines = vec![
        span("chapter-number", prefix),
        Inline::Str(Str {
            text: "\u{00A0}".to_string(),
            source_info: gen_si(),
        }),
        Inline::Space(Space {
            source_info: gen_si(),
        }),
        span("chapter-title", title),
    ];
    ConfigValue::new_inlines(inlines, gen_si())
}

/// One chapter-list entry → one sidebar contents entry, with chapter
/// numbering baked into the `text`. Port of Q1's
/// `chapterToSidebarItem` (part → section, chapters → contents) plus
/// the decoration Q1 applies at nav-render time via
/// `ProjectType.navItemText` — Q2 bakes it here instead, since chapter
/// numbers are fully known at config-translation time (design doc §4).
///
/// A chapter whose title can't be determined here (no explicit `text:`
/// and no profile title) stays a bare-href entry: the sidebar's own
/// enrichment fills in the plain title at generate time — undecorated,
/// a documented fallback.
fn chapter_to_sidebar_item(
    entry: &ConfigValue,
    render_items: &[BookRenderItem],
    index: Option<&ProjectIndex>,
) -> ConfigValue {
    // Part → section, recursing into chapters.
    if let Some(part) = entry.get("part") {
        let title = part
            .as_plain_text()
            .unwrap_or_else(|| "Untitled part".to_string());
        let mut item = vec![("section", ConfigValue::new_string(title, gen_si()))];
        if let Some(href) = entry.get("href") {
            item.push(("href", href.clone()));
        }
        if let Some(chapters) = entry.get("chapters").and_then(|c| c.as_array()) {
            item.push((
                "contents",
                ConfigValue::new_array(
                    chapters
                        .iter()
                        .map(|c| chapter_to_sidebar_item(c, render_items, index))
                        .collect(),
                    gen_si(),
                ),
            ));
        }
        return new_map(item);
    }

    // Chapter file.
    let href = entry
        .get("href")
        .and_then(|h| h.as_plain_text())
        .or_else(|| entry.as_plain_text());
    let Some(href) = href else {
        return entry.clone();
    };
    // Dash dividers pass through untouched.
    if !href.trim().is_empty() && href.trim().chars().all(|c| c == '-') {
        return entry.clone();
    }

    let explicit_text = entry.get("text").and_then(|t| t.as_plain_text());
    let label_prefix = render_items
        .iter()
        .find(|i| {
            i.file
                .as_ref()
                .is_some_and(|f| f.as_path() == Path::new(&href))
        })
        .and_then(chapter_label_prefix);
    let title = explicit_text.clone().or_else(|| {
        index.and_then(|idx| {
            idx.lookup_by_source(Path::new(&href))
                .and_then(|p| p.title.clone())
        })
    });

    match (label_prefix, title) {
        (Some(prefix), Some(title)) => new_map(vec![
            ("href", ConfigValue::new_string(href, gen_si())),
            ("text", decorated_chapter_text(&prefix, &title)),
        ]),
        (None, Some(title)) if explicit_text.is_some() => new_map(vec![
            ("href", ConfigValue::new_string(href, gen_si())),
            ("text", ConfigValue::new_string(title, gen_si())),
        ]),
        _ => ConfigValue::new_string(href, gen_si()),
    }
}

/// Translate one `book.chapters`-shaped list into sidebar contents.
pub fn chapters_to_sidebar_contents(
    chapters: &[ConfigValue],
    render_items: &[BookRenderItem],
    index: Option<&ProjectIndex>,
) -> Vec<ConfigValue> {
    chapters
        .iter()
        .map(|c| chapter_to_sidebar_item(c, render_items, index))
        .collect()
}

/// The full `website.sidebar.contents` array for a book: chapters,
/// then the designated references page, then the appendix section.
/// Port of the sidebar-translation half of Q1's `bookProjectConfig`.
pub fn book_sidebar_contents(
    book: &ConfigValue,
    render_items: &[BookRenderItem],
    index: Option<&ProjectIndex>,
    appendices_title: &str,
) -> Vec<ConfigValue> {
    let mut contents = Vec::new();
    if let Some(chapters) = book.get("chapters").and_then(|c| c.as_array()) {
        contents.extend(chapters_to_sidebar_contents(chapters, render_items, index));
    }
    if let Some(references) = book.get("references") {
        contents.push(chapter_to_sidebar_item(references, render_items, index));
    }
    if let Some(appendices) = book.get("appendices").and_then(|a| a.as_array()) {
        contents.push(new_map(vec![
            (
                "section",
                ConfigValue::new_string(appendices_title, gen_si()),
            ),
            (
                "contents",
                ConfigValue::new_array(
                    chapters_to_sidebar_contents(appendices, render_items, index),
                    gen_si(),
                ),
            ),
        ]));
    }
    contents
}

/// Sidebar tool entries for `book.downloads` — one entry per
/// downloadable single-file format (`pdf`, `epub`, `docx`), a lone
/// entry flattening to a single download button. Port of Q1's
/// `downloadTools`.
///
/// **Q2 has no sidebar-tools renderer (bd-fod3)** — these are inert
/// config entries written so that the shape is right when the renderer
/// lands. Divergence from Q1: unknown download names are skipped; Q1's
/// fallback produces a literally broken `Download action}` text (a
/// missing `${}` in its template literal) that nothing could want.
pub fn download_tools(book: &ConfigValue, project_dir: &Path) -> Vec<ConfigValue> {
    // Q1's kDownloadableItems: action → (extension, display name, icon).
    const DOWNLOADABLE: &[(&str, &str, &str)] = &[
        ("epub", "ePub", "journal"),
        ("pdf", "PDF", "file-pdf"),
        ("docx", "Docx", "file-word"),
    ];
    let Some(downloads) = book.get("downloads").and_then(|d| d.as_array()) else {
        return Vec::new();
    };
    let stem = book_output_stem(project_dir, Some(book));
    let mut entries: Vec<ConfigValue> = Vec::new();
    for download in downloads {
        let Some(action) = download.as_plain_text() else {
            continue;
        };
        let Some((ext, name, icon)) = DOWNLOADABLE
            .iter()
            .find(|(ext, _, _)| *ext == action.as_str())
        else {
            continue;
        };
        entries.push(new_map(vec![
            ("icon", ConfigValue::new_string(*icon, gen_si())),
            (
                "text",
                ConfigValue::new_string(format!("Download {name}"), gen_si()),
            ),
            (
                "href",
                ConfigValue::new_string(format!("/{stem}.{ext}"), gen_si()),
            ),
        ]));
    }
    match entries.len() {
        0 => Vec::new(),
        1 => {
            // A lone download flattens to a single button with the
            // generic download icon.
            let mut entry = entries.pop().unwrap();
            entry.insert_path(&["icon"], ConfigValue::new_string("download", gen_si()));
            vec![entry]
        }
        _ => vec![new_map(vec![
            ("icon", ConfigValue::new_string("download", gen_si())),
            ("text", ConfigValue::new_string("Download", gen_si())),
            ("menu", ConfigValue::new_array(entries, gen_si())),
        ])],
    }
}

/// Sidebar tool entries for `book.sharing` — one per known network
/// (`linkedin`, `facebook`, `twitter`), multiple entries folding into a
/// "Share" menu. Port of Q1's `sharingTools`, including its `kSharingUrls`
/// quirk: LinkedIn's entry carries `href` while Facebook/Twitter carry
/// `url` — a real Q1 inconsistency, ported literally since these entries
/// are inert config in Q2 (no tools renderer, bd-fod3) and fidelity is
/// the goal.
pub fn sharing_tools(book: &ConfigValue) -> Vec<ConfigValue> {
    let Some(sharing) = book.get("sharing").and_then(|s| s.as_array()) else {
        return Vec::new();
    };
    let tool = |icon: &str, text: &str, link_key: &str, link: &str| {
        new_map(vec![
            ("icon", ConfigValue::new_string(icon, gen_si())),
            ("text", ConfigValue::new_string(text, gen_si())),
            (link_key, ConfigValue::new_string(link, gen_si())),
        ])
    };
    let mut entries: Vec<ConfigValue> = Vec::new();
    for action in sharing {
        let Some(action) = action.as_plain_text() else {
            continue;
        };
        let entry = match action.as_str() {
            "linkedin" => tool(
                "linkedin",
                "LinkedIn",
                "href",
                "https://www.linkedin.com/sharing/share-offsite/?url=|url|",
            ),
            "facebook" => tool(
                "facebook",
                "Facebook",
                "url",
                "https://www.facebook.com/sharer/sharer.php?u=|url|",
            ),
            "twitter" => tool(
                "twitter",
                "Twitter",
                "url",
                "https://twitter.com/intent/tweet?url=|url|",
            ),
            _ => continue,
        };
        entries.push(entry);
    }
    match entries.len() {
        0 => Vec::new(),
        1 => entries,
        _ => vec![new_map(vec![
            ("text", ConfigValue::new_string("Share", gen_si())),
            ("icon", ConfigValue::new_string("share", gen_si())),
            ("menu", ConfigValue::new_array(entries, gen_si())),
        ])],
    }
}

/// The book's output-file stem (`bookOutputStem` port): `book.output-file` || `book.title` ||
/// the project directory's basename, with Q1's `texSafeFilename`
/// applied (non-filename-safe characters replaced by `-`). Divergence:
/// `{{< var >}}` resolution against project `vars` is not ported —
/// P-later.
pub fn book_output_stem(project_dir: &Path, book: Option<&ConfigValue>) -> String {
    let raw = book
        .and_then(|b| b.get("output-file").and_then(|v| v.as_plain_text()))
        .or_else(|| book.and_then(|b| b.get("title").and_then(|v| v.as_plain_text())))
        .or_else(|| {
            project_dir
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "book".to_string());
    // Q1's texSafeFilename: /[ <>()|\:&;#?*'\\\/]/g → '-'.
    raw.chars()
        .map(|c| {
            if matches!(
                c,
                ' ' | '<'
                    | '>'
                    | '('
                    | ')'
                    | '|'
                    | '\\'
                    | ':'
                    | '&'
                    | ';'
                    | '#'
                    | '?'
                    | '*'
                    | '\''
                    | '/'
            ) {
                '-'
            } else {
                c
            }
        })
        .collect()
}

/// Book-wide format defaults (`book.ts` `formatExtras`' format-agnostic
/// pair): `number-sections: true` and `crossref.chapters: true`, set as
/// *defaults* in the project metadata — a value already present (from
/// `_quarto.yml` or a document's own front matter, which merges above
/// the project layer) always wins.
pub fn apply_format_defaults(meta: &mut ConfigValue) {
    if meta.get("number-sections").is_none() {
        meta.insert_path(&["number-sections"], ConfigValue::new_bool(true, gen_si()));
    }
    if meta.get_path(&["crossref", "chapters"]).is_none() {
        meta.insert_path(
            &["crossref", "chapters"],
            ConfigValue::new_bool(true, gen_si()),
        );
    }
}

/// Resolve a special `book.date` (`today` / `now` / `last-modified`)
/// once, project-wide, writing the concrete ISO timestamp back into the
/// book config. Port of Q1's `bookPreRender` (`isSpecialDate` /
/// `parseSpecialDate`). Returns `true` when a special date was resolved.
///
/// `inputs` are the project's input file paths, used for
/// `last-modified` (Q1 takes the max mtime across all project inputs).
/// Dates resolve against UTC, matching Q2's `date_normalize` transform
/// (a documented deviation from Q1's local time).
pub fn resolve_special_book_date(
    book: &mut ConfigValue,
    inputs: &[PathBuf],
    runtime: &dyn SystemRuntime,
) -> bool {
    let Some(date) = book.get("date").and_then(|d| d.as_plain_text()) else {
        return false;
    };
    let iso = |dt: time::OffsetDateTime, has_time: bool| {
        ParsedDate {
            datetime: time::PrimitiveDateTime::new(dt.date(), dt.time()),
            offset: Some(time::UtcOffset::UTC),
            has_time,
        }
        .iso_string()
    };
    let resolved = match date.as_str() {
        "today" => runtime
            .unix_timestamp()
            .ok()
            .and_then(|ts| time::OffsetDateTime::from_unix_timestamp(ts as i64).ok())
            .map(|dt| iso(dt.date().midnight().assume_utc(), true)),
        "now" => runtime
            .unix_timestamp()
            .ok()
            .and_then(|ts| time::OffsetDateTime::from_unix_timestamp(ts as i64).ok())
            .map(|dt| iso(dt, true)),
        "last-modified" => inputs
            .iter()
            .filter_map(|p| runtime.path_metadata(p).ok())
            .filter_map(|m| m.modified)
            .max()
            .map(time::OffsetDateTime::from)
            .map(|dt| iso(dt, true)),
        _ => None,
    };
    match resolved {
        Some(resolved) => {
            book.insert_path(&["date"], ConfigValue::new_string(resolved, gen_si()));
            true
        }
        None => false,
    }
}

/// Files a book must render even though `book.chapters` doesn't name
/// them (port of `bookProjectConfig`'s render-list additions): every
/// relative `href` in a `page-footer` region, plus a `404.qmd` /
/// `404.md` sitting in the project directory. Paths are project-relative.
pub fn additional_render_files(
    project_dir: &Path,
    meta: &ConfigValue,
    runtime: &dyn SystemRuntime,
) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = Vec::new();
    // page-footer regions (Q1's `resolvePageFooter` + `addFooterItems`).
    if let Some(footer) = meta.get_path(&["website", "page-footer"]) {
        for region in ["left", "right", "center"] {
            let Some(items) = footer.get(region).and_then(|r| r.as_array()) else {
                continue;
            };
            for item in items {
                let Some(href) = item.get("href").and_then(|h| h.as_plain_text()) else {
                    continue;
                };
                // Q1's isAbsoluteRef: ^https?:// only.
                if href.starts_with("http://") || href.starts_with("https://") {
                    continue;
                }
                files.push(PathBuf::from(href));
            }
        }
    }
    // A 404 page in the project directory (Q1 checks every engine
    // extension; Q2's always-renderable set is qmd/md).
    for ext in ["qmd", "md"] {
        let candidate = project_dir.join(format!("404.{ext}"));
        if runtime.is_file(&candidate).unwrap_or(false) {
            files.push(PathBuf::from(format!("404.{ext}")));
            break;
        }
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_profile::DocumentProfile;
    use quarto_pandoc_types::config_value::ConfigValueKind;
    use quarto_system_runtime::NativeRuntime;
    use std::path::Path;
    use tempfile::TempDir;

    fn si() -> SourceInfo {
        SourceInfo::generated(By::programmatic_config())
    }
    fn s(v: &str) -> ConfigValue {
        ConfigValue::new_string(v, si())
    }
    fn b(v: bool) -> ConfigValue {
        ConfigValue::new_bool(v, si())
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

    fn chapter(kind: BookRenderItemKind, file: &str, number: Option<u32>) -> BookRenderItem {
        BookRenderItem {
            kind,
            depth: 0,
            text: None,
            file: Some(PathBuf::from(file)),
            number,
        }
    }

    /// A project index holding profiles with the given (source, title) pairs.
    fn index_of(profiles: &[(&str, &str)]) -> ProjectIndex {
        ProjectIndex::new(
            profiles
                .iter()
                .map(|(source, title)| DocumentProfile {
                    source_path: PathBuf::from(source),
                    title: Some(title.to_string()),
                    ..DocumentProfile::default()
                })
                .collect(),
        )
    }

    /// Extract the decorated-text spans from a sidebar item's `text`.
    fn decorated_parts(text: &ConfigValue) -> (String, String) {
        let ConfigValueKind::PandocInlines(inlines) = &text.value else {
            panic!("expected PandocInlines, got {:?}", text.value);
        };
        let mut prefix = None;
        let mut title = None;
        for inline in inlines {
            if let Inline::Span(span) = inline {
                let class = span.attr.1.first().map(|s| s.as_str());
                let text = span
                    .content
                    .iter()
                    .find_map(|i| match i {
                        Inline::Str(s) => Some(s.text.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                match class {
                    Some("chapter-number") => prefix = Some(text),
                    Some("chapter-title") => title = Some(text),
                    _ => {}
                }
            }
        }
        (
            prefix.expect("chapter-number span"),
            title.expect("chapter-title span"),
        )
    }

    #[test]
    fn translation_produces_website_sidebar_shape() {
        let book = map(vec![
            (
                "chapters",
                arr(vec![
                    s("index.qmd"),
                    s("intro.qmd"),
                    map(vec![
                        ("part", s("Part I")),
                        ("chapters", arr(vec![s("chap1.qmd"), s("chap2.qmd")])),
                    ]),
                ]),
            ),
            ("references", s("references.qmd")),
            ("appendices", arr(vec![s("app-a.qmd")])),
        ]);
        let render_items = vec![
            chapter(BookRenderItemKind::Index, "index.qmd", None),
            chapter(BookRenderItemKind::Chapter, "intro.qmd", Some(1)),
            BookRenderItem {
                kind: BookRenderItemKind::Part,
                depth: 0,
                text: Some("Part I".to_string()),
                file: None,
                number: None,
            },
            chapter(BookRenderItemKind::Chapter, "chap1.qmd", Some(2)),
            chapter(BookRenderItemKind::Chapter, "chap2.qmd", Some(3)),
            chapter(BookRenderItemKind::References, "references.qmd", Some(4)),
            chapter(BookRenderItemKind::Appendix, "app-a.qmd", Some(1)),
        ];
        let index = index_of(&[
            ("index.qmd", "Preface"),
            ("intro.qmd", "Introduction"),
            ("chap1.qmd", "One"),
            ("chap2.qmd", "Two"),
            ("references.qmd", "References"),
            ("app-a.qmd", "First Appendix"),
        ]);
        let contents = book_sidebar_contents(&book, &render_items, Some(&index), "Appendices");
        assert_eq!(contents.len(), 5);

        // intro: decorated with chapter number 1 and profile title.
        let intro = &contents[1];
        assert_eq!(
            intro.get("href").and_then(|h| h.as_plain_text()).as_deref(),
            Some("intro.qmd")
        );
        let text = intro.get("text").unwrap();
        assert_eq!(
            decorated_parts(text),
            ("1".to_string(), "Introduction".to_string())
        );

        // Part → section with nested contents.
        let part = &contents[2];
        assert_eq!(
            part.get("section")
                .and_then(|t| t.as_plain_text())
                .as_deref(),
            Some("Part I")
        );
        let part_contents = part.get("contents").and_then(|c| c.as_array()).unwrap();
        assert_eq!(part_contents.len(), 2);
        assert_eq!(
            decorated_parts(part_contents[0].get("text").unwrap()),
            ("2".to_string(), "One".to_string())
        );

        // References: continues chapter numbering (4).
        let refs = &contents[3];
        assert_eq!(
            decorated_parts(refs.get("text").unwrap()),
            ("4".to_string(), "References".to_string())
        );

        // Appendices: section titled "Appendices", letter-decorated entry.
        let appendix = &contents[4];
        assert_eq!(
            appendix
                .get("section")
                .and_then(|t| t.as_plain_text())
                .as_deref(),
            Some("Appendices")
        );
        let app_contents = appendix.get("contents").and_then(|c| c.as_array()).unwrap();
        assert_eq!(
            decorated_parts(app_contents[0].get("text").unwrap()),
            ("A".to_string(), "First Appendix".to_string())
        );
    }

    #[test]
    fn unnumbered_chapter_sidebar_text_is_plain() {
        let book = map(vec![(
            "chapters",
            arr(vec![
                map(vec![("href", s("preface.qmd")), ("text", s("Preface"))]),
                s("intro.qmd"),
            ]),
        )]);
        let render_items = vec![
            chapter(BookRenderItemKind::Chapter, "preface.qmd", None),
            chapter(BookRenderItemKind::Chapter, "intro.qmd", Some(1)),
        ];
        let index = index_of(&[("intro.qmd", "Introduction")]);
        let contents = book_sidebar_contents(&book, &render_items, Some(&index), "Appendices");

        // Unnumbered chapter with explicit text: plain string, no decoration.
        let preface_text = contents[0].get("text").unwrap();
        assert_eq!(preface_text.as_plain_text().as_deref(), Some("Preface"));
        assert!(!matches!(
            preface_text.value,
            ConfigValueKind::PandocInlines(_)
        ));

        // Numbered chapter: decorated via profile title.
        assert_eq!(
            decorated_parts(contents[1].get("text").unwrap()),
            ("1".to_string(), "Introduction".to_string())
        );
    }

    #[test]
    fn unnumbered_appendix_chapter_sidebar_text_has_no_letter() {
        let book = map(vec![
            ("chapters", arr(vec![s("index.qmd")])),
            (
                "appendices",
                arr(vec![
                    s("app-a.qmd"),
                    map(vec![("href", s("errata.qmd")), ("text", s("Errata"))]),
                    s("app-b.qmd"),
                ]),
            ),
        ]);
        let render_items = vec![
            chapter(BookRenderItemKind::Index, "index.qmd", None),
            chapter(BookRenderItemKind::Appendix, "app-a.qmd", Some(1)),
            chapter(BookRenderItemKind::Appendix, "errata.qmd", None),
            chapter(BookRenderItemKind::Appendix, "app-b.qmd", Some(2)),
        ];
        let index = index_of(&[("app-a.qmd", "First"), ("app-b.qmd", "Second")]);
        let contents = book_sidebar_contents(&book, &render_items, Some(&index), "Appendices");
        let appendix = contents.last().unwrap();
        let app_contents = appendix.get("contents").and_then(|c| c.as_array()).unwrap();
        assert_eq!(app_contents.len(), 3);
        assert_eq!(
            decorated_parts(app_contents[0].get("text").unwrap()),
            ("A".to_string(), "First".to_string())
        );
        // The unnumbered appendix chapter: plain text, no letter.
        let errata_text = app_contents[1].get("text").unwrap();
        assert_eq!(errata_text.as_plain_text().as_deref(), Some("Errata"));
        assert!(!matches!(
            errata_text.value,
            ConfigValueKind::PandocInlines(_)
        ));
        // The next numbered appendix chapter gets B — the unnumbered one
        // consumed no slot.
        assert_eq!(
            decorated_parts(app_contents[2].get("text").unwrap()),
            ("B".to_string(), "Second".to_string())
        );
    }

    #[test]
    fn bare_href_without_profile_stays_bare() {
        // No index, no explicit text → bare string entry; the sidebar's
        // own enrichment fills the title (undecorated fallback).
        let book = map(vec![("chapters", arr(vec![s("mystery.qmd")]))]);
        let render_items = vec![chapter(BookRenderItemKind::Chapter, "mystery.qmd", Some(1))];
        let contents = book_sidebar_contents(&book, &render_items, None, "Appendices");
        assert_eq!(contents[0].as_plain_text().as_deref(), Some("mystery.qmd"));
    }

    #[test]
    fn copy_book_keys_into_website_book_wins() {
        let mut meta = map(vec![
            (
                "book",
                map(vec![
                    ("title", s("My Book")),
                    ("site-url", s("https://example.com/book")),
                    ("navbar", map(vec![("background", s("dark"))])),
                ]),
            ),
            ("website", map(vec![("title", s("Wrong Title"))])),
        ]);
        copy_book_keys_to_website(&mut meta);
        let site = meta.get("website").unwrap();
        assert_eq!(
            site.get("title").unwrap().as_plain_text().as_deref(),
            Some("My Book")
        );
        assert_eq!(
            site.get("site-url").unwrap().as_plain_text().as_deref(),
            Some("https://example.com/book")
        );
        assert!(site.get("navbar").is_some());
        // Q1's unconditional page-navigation default.
        assert_eq!(site.get("page-navigation").unwrap().as_bool(), Some(true));
        // Book keys themselves stay under `book:`.
        assert!(meta.get("book").is_some());
    }

    #[test]
    fn download_tools_entries_match_q1_shape() {
        let dir = Path::new("/proj/my-book");
        let book = map(vec![
            ("title", s("My Book")),
            ("downloads", arr(vec![s("pdf"), s("epub")])),
        ]);
        let tools = download_tools(&book, dir);
        assert_eq!(tools.len(), 1, "multiple downloads fold into a menu");
        let menu = tools[0].get("menu").and_then(|m| m.as_array()).unwrap();
        assert_eq!(menu.len(), 2);
        // Q1 order follows kDownloadableItems filtering over the user's
        // array order: entries appear in the user's order.
        let first = &menu[0];
        assert_eq!(
            first.get("icon").unwrap().as_plain_text().as_deref(),
            Some("file-pdf")
        );
        assert_eq!(
            first.get("text").unwrap().as_plain_text().as_deref(),
            Some("Download PDF")
        );
        assert_eq!(
            first.get("href").unwrap().as_plain_text().as_deref(),
            Some("/My-Book.pdf")
        );
        let second = &menu[1];
        assert_eq!(
            second.get("href").unwrap().as_plain_text().as_deref(),
            Some("/My-Book.epub")
        );
    }

    #[test]
    fn single_download_flattens_to_button() {
        let dir = Path::new("/proj/stemmed");
        let book = map(vec![
            ("output-file", s("the-book")),
            ("downloads", arr(vec![s("epub")])),
        ]);
        let tools = download_tools(&book, dir);
        assert_eq!(tools.len(), 1);
        assert_eq!(
            tools[0].get("icon").unwrap().as_plain_text().as_deref(),
            Some("download")
        );
        assert_eq!(
            tools[0].get("href").unwrap().as_plain_text().as_deref(),
            Some("/the-book.epub")
        );
    }

    #[test]
    fn sharing_tools_entries_match_q1_shape() {
        let book = map(vec![("sharing", arr(vec![s("twitter"), s("mastodon")]))]);
        let tools = sharing_tools(&book);
        // Unknown networks are filtered out; one remains → no menu.
        assert_eq!(tools.len(), 1);
        assert_eq!(
            tools[0].get("icon").unwrap().as_plain_text().as_deref(),
            Some("twitter")
        );
        assert_eq!(
            tools[0].get("url").unwrap().as_plain_text().as_deref(),
            Some("https://twitter.com/intent/tweet?url=|url|")
        );
    }

    #[test]
    fn format_defaults_apply_regardless_of_target() {
        // HTML target.
        let mut meta = map(vec![]);
        apply_format_defaults(&mut meta);
        assert_eq!(meta.get("number-sections").unwrap().as_bool(), Some(true));
        assert_eq!(
            meta.get_path(&["crossref", "chapters"]).unwrap().as_bool(),
            Some(true)
        );
        // Non-HTML target — same defaults (format-agnostic).
        let mut meta2 = map(vec![("format", map(vec![("typst", map(vec![]))]))]);
        apply_format_defaults(&mut meta2);
        assert_eq!(meta2.get("number-sections").unwrap().as_bool(), Some(true));
        assert_eq!(
            meta2.get_path(&["crossref", "chapters"]).unwrap().as_bool(),
            Some(true)
        );
        // An explicit user value wins (defaults never overwrite).
        let mut meta3 = map(vec![("number-sections", b(false))]);
        apply_format_defaults(&mut meta3);
        assert_eq!(meta3.get("number-sections").unwrap().as_bool(), Some(false));
    }

    #[test]
    fn book_date_today_resolves_once_project_wide() {
        let runtime = NativeRuntime::new();
        let mut book = map(vec![("date", s("today"))]);
        assert!(resolve_special_book_date(&mut book, &[], &runtime));
        let resolved = book.get("date").unwrap().as_plain_text().unwrap();
        // Resolved to a concrete ISO timestamp, not the keyword.
        assert_ne!(resolved, "today");
        assert!(
            resolved.starts_with("20"),
            "expected ISO date, got {resolved}"
        );
        // Resolving again is a no-op (already concrete).
        assert!(!resolve_special_book_date(&mut book, &[], &runtime));
        assert_eq!(book.get("date").unwrap().as_plain_text().unwrap(), resolved);
    }

    #[test]
    fn book_date_last_modified_uses_max_input_mtime() {
        let temp = TempDir::new().unwrap();
        let a = temp.path().join("a.qmd");
        let b = temp.path().join("b.qmd");
        std::fs::write(&a, "# a\n").unwrap();
        std::fs::write(&b, "# b\n").unwrap();
        let runtime = NativeRuntime::new();
        let mut book = map(vec![("date", s("last-modified"))]);
        assert!(resolve_special_book_date(&mut book, &[a, b], &runtime));
        let resolved = book.get("date").unwrap().as_plain_text().unwrap();
        assert!(
            resolved.starts_with("20"),
            "expected ISO date, got {resolved}"
        );
    }

    #[test]
    fn footer_and_404_files_are_render_list_additions() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join("404.qmd"), "# Not found\n").unwrap();
        let meta = map(vec![(
            "website",
            map(vec![(
                "page-footer",
                map(vec![(
                    "left",
                    arr(vec![
                        map(vec![("href", s("extra.qmd")), ("text", s("Extra"))]),
                        map(vec![("href", s("https://example.com")), ("text", s("Ext"))]),
                    ]),
                )]),
            )]),
        )]);
        let runtime = NativeRuntime::new();
        let files = additional_render_files(temp.path(), &meta, &runtime);
        assert_eq!(
            files,
            vec![PathBuf::from("extra.qmd"), PathBuf::from("404.qmd")]
        );
    }
}
