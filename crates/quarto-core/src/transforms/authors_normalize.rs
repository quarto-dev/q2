/*
 * authors_normalize.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Transform that normalizes author metadata and title-block labels.
 */

//! Author/label normalization transform.
//!
//! The Rust counterpart of Quarto 1's `authors.lua` metadata pass: it
//! derives, from the raw `author`/`authors` metadata,
//!
//! - **`by-author`** — the list shape the title-block templates
//!   iterate (`$for(by-author)$ … $it.name.literal$`). Phase 1 of the
//!   title-block parity epic (bd-gx9cic8z) populates only
//!   `name.literal`; Phase 2 (bd-ez0hiowa) adds the full normalized
//!   author schema (orcid, email, url, degrees, affiliations, …) plus
//!   `by-affiliation`.
//! - **`labels`** — the localizable title-block heading labels
//!   (`labels.authors`, `labels.published`, …), honoring the
//!   per-document `*-title` override options (`author-title`,
//!   `published-title`, `abstract-title`, `modified-title`,
//!   `doi-title`, `description-title`). Defaults are hardcoded
//!   English for now; a language-file system akin to Q1's
//!   `_language.yml` is future work tracked in the epic plan
//!   (decision Q3).
//! - **`rendered.has-title-block`** — a Q2-internal flag: true when
//!   any title-block content exists (title, subtitle, authors, date,
//!   or abstract). The built-in `title-block` template partial keys on
//!   it so documents with no title-block metadata emit no empty
//!   `<header>`.
//!
//! Like Q1's Lua pass, the derived keys are written into document
//! metadata (not a side channel) so downstream consumers — templates,
//! Lua filters, and the q2-preview React title block, which reads the
//! same metadata — all see one normalization.
//!
//! Plan: `claude-notes/plans/2026-07-15-html-title-block-parity.md`.

use quarto_pandoc_types::pandoc::Pandoc;
use quarto_pandoc_types::{ConfigMapEntry, ConfigValue};
use quarto_source_map::{By, SourceInfo};

use crate::Result;
use crate::metadata::authors::{Author, parse_authors};
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

/// Transform that writes `by-author`, `labels`, and
/// `rendered.has-title-block` into document metadata.
pub struct AuthorsNormalizeTransform;

impl AuthorsNormalizeTransform {
    /// Create a new authors normalization transform.
    pub fn new() -> Self {
        Self
    }
}

impl Default for AuthorsNormalizeTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for AuthorsNormalizeTransform {
    fn name(&self) -> &str {
        "authors-normalize"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Normalization
    }

    async fn transform(&self, ast: &mut Pandoc, _ctx: &mut RenderContext) -> Result<()> {
        normalize_authors_meta(&mut ast.meta);
        Ok(())
    }
}

fn gen_si() -> SourceInfo {
    SourceInfo::generated(By::programmatic_config())
}

/// Derive `by-author`, `labels`, and `rendered.has-title-block`.
///
/// Recomputes (overwrites) `by-author` and `labels` on every run, like
/// Q1's Lua pass — the raw `author` metadata is the source of truth.
pub fn normalize_authors_meta(meta: &mut ConfigValue) {
    if !meta.is_map() {
        return;
    }

    let authors = parse_authors(meta);

    if !authors.is_empty() {
        let by_author = ConfigValue::new_array(
            authors.iter().map(author_to_config_value).collect(),
            gen_si(),
        );
        meta.insert_path(&["by-author"], by_author);

        // Pandoc-convention `author-meta`: one plain-text name per
        // author, consumed by the template head's
        // `$for(author-meta)$<meta name="author" …>$endfor$`. Without
        // it, structured author maps would flatten into the meta tag
        // (the bd-8v34zny5 boolean-garbage shape).
        let author_meta = ConfigValue::new_array(
            authors
                .iter()
                .map(|a| ConfigValue::new_string(a.name.literal.clone(), gen_si()))
                .collect(),
            gen_si(),
        );
        meta.insert_path(&["author-meta"], author_meta);
    }

    let labels = compute_labels(meta, authors.len());
    meta.insert_path(&["labels"], labels);

    if has_title_block_content(meta, &authors) {
        meta.insert_path(
            &["rendered", "has-title-block"],
            ConfigValue::new_bool(true, gen_si()),
        );
    }
}

/// `{ name: { literal: "…" } }` — the Q1 `by-author` entry shape
/// (Phase-1 subset).
fn author_to_config_value(author: &Author) -> ConfigValue {
    let name = ConfigValue::new_map(
        vec![map_entry(
            "literal",
            ConfigValue::new_string(author.name.literal.clone(), gen_si()),
        )],
        gen_si(),
    );
    ConfigValue::new_map(vec![map_entry("name", name)], gen_si())
}

fn map_entry(key: &str, value: ConfigValue) -> ConfigMapEntry {
    ConfigMapEntry {
        key: key.to_string(),
        key_source: gen_si(),
        value,
    }
}

/// Compute the title-block heading labels.
///
/// Default English strings match Q1's `_language.yml` keys
/// (`title-block-author-single`, `title-block-published`, …); the
/// per-document `*-title` options override them.
fn compute_labels(meta: &ConfigValue, author_count: usize) -> ConfigValue {
    let override_of = |key: &str| meta.get(key).and_then(|v| v.as_plain_text());

    let authors_label = override_of("author-title").unwrap_or_else(|| {
        if author_count > 1 {
            "Authors".to_string()
        } else {
            "Author".to_string()
        }
    });
    let entries = vec![
        map_entry("authors", ConfigValue::new_string(authors_label, gen_si())),
        map_entry(
            "published",
            ConfigValue::new_string(
                override_of("published-title").unwrap_or_else(|| "Published".to_string()),
                gen_si(),
            ),
        ),
        map_entry(
            "modified",
            ConfigValue::new_string(
                override_of("modified-title").unwrap_or_else(|| "Modified".to_string()),
                gen_si(),
            ),
        ),
        map_entry(
            "doi",
            ConfigValue::new_string(
                override_of("doi-title").unwrap_or_else(|| "Doi".to_string()),
                gen_si(),
            ),
        ),
        map_entry(
            "abstract",
            ConfigValue::new_string(
                override_of("abstract-title").unwrap_or_else(|| "Abstract".to_string()),
                gen_si(),
            ),
        ),
        map_entry(
            "description",
            ConfigValue::new_string(
                override_of("description-title").unwrap_or_else(|| "Description".to_string()),
                gen_si(),
            ),
        ),
        map_entry(
            "keywords",
            ConfigValue::new_string("Keywords".to_string(), gen_si()),
        ),
    ];
    ConfigValue::new_map(entries, gen_si())
}

/// True when the document has any content the title block renders.
///
/// Phase 3 (bd-j6huijli) extends this list with `description`, `doi`,
/// `date-modified`, `keywords`, and `categories` when those fields
/// join the metadata grid.
fn has_title_block_content(meta: &ConfigValue, authors: &[Author]) -> bool {
    if !authors.is_empty() {
        return true;
    }
    ["title", "subtitle", "date", "abstract"]
        .iter()
        .any(|key| meta.get(key).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_source_map::{FileId, Location, Range};

    fn si() -> SourceInfo {
        SourceInfo::from_range(
            FileId(0),
            Range {
                start: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
            },
        )
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

    fn s(text: &str) -> ConfigValue {
        ConfigValue::new_string(text, si())
    }

    fn arr(items: Vec<ConfigValue>) -> ConfigValue {
        ConfigValue::new_array(items, si())
    }

    fn label(meta: &ConfigValue, key: &str) -> String {
        meta.get_path(&["labels", key])
            .and_then(|v| v.as_plain_text())
            .unwrap_or_default()
    }

    #[test]
    fn single_author_writes_by_author_and_singular_label() {
        let mut meta = map(vec![("title", s("T")), ("author", s("Norah Jones"))]);
        normalize_authors_meta(&mut meta);

        let by_author = meta.get("by-author").expect("by-author written");
        let entries = by_author.as_array().expect("array");
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0]
                .get_path(&["name", "literal"])
                .and_then(|v| v.as_plain_text()),
            Some("Norah Jones".to_string())
        );
        assert_eq!(label(&meta, "authors"), "Author");
    }

    #[test]
    fn two_authors_pluralize() {
        let mut meta = map(vec![(
            "author",
            arr(vec![s("Amelia Earhart"), s("Bill Malone")]),
        )]);
        normalize_authors_meta(&mut meta);
        assert_eq!(label(&meta, "authors"), "Authors");
        assert_eq!(
            meta.get("by-author")
                .and_then(|v| v.as_array())
                .map(|a| a.len()),
            Some(2)
        );
    }

    #[test]
    fn label_overrides_win() {
        let mut meta = map(vec![
            ("author", s("Norah Jones")),
            ("author-title", s("Written by")),
            ("published-title", s("Posted")),
            ("abstract-title", s("Summary")),
        ]);
        normalize_authors_meta(&mut meta);
        assert_eq!(label(&meta, "authors"), "Written by");
        assert_eq!(label(&meta, "published"), "Posted");
        assert_eq!(label(&meta, "abstract"), "Summary");
        // Untouched defaults remain.
        assert_eq!(label(&meta, "modified"), "Modified");
        assert_eq!(label(&meta, "doi"), "Doi");
    }

    #[test]
    fn no_authors_no_by_author_but_labels_written() {
        let mut meta = map(vec![("title", s("T")), ("date", s("2026-07-01"))]);
        normalize_authors_meta(&mut meta);
        assert!(meta.get("by-author").is_none());
        assert_eq!(label(&meta, "published"), "Published");
        assert_eq!(
            meta.get_path(&["rendered", "has-title-block"])
                .and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn empty_meta_sets_no_title_block_flag() {
        let mut meta = map(vec![("format", s("html"))]);
        normalize_authors_meta(&mut meta);
        assert!(meta.get_path(&["rendered", "has-title-block"]).is_none());
    }

    #[test]
    fn structured_authors_produce_names_not_booleans() {
        // bd-8v34zny5 regression shape.
        let mut meta = map(vec![(
            "author",
            arr(vec![
                map(vec![
                    ("name", s("Norah Jones")),
                    ("corresponding", ConfigValue::new_bool(true, si())),
                ]),
                map(vec![("name", s("Bill Malone"))]),
            ]),
        )]);
        normalize_authors_meta(&mut meta);
        let by_author = meta.get("by-author").and_then(|v| v.as_array()).unwrap();
        let names: Vec<String> = by_author
            .iter()
            .filter_map(|e| {
                e.get_path(&["name", "literal"])
                    .and_then(|v| v.as_plain_text())
            })
            .collect();
        assert_eq!(names, vec!["Norah Jones", "Bill Malone"]);
    }
}
