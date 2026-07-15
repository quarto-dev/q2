/*
 * metadata/authors.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Typed author model parsed from document metadata.
 */

//! Typed author model.
//!
//! Parses the `author` / `authors` metadata keys into typed structs,
//! mirroring the normalization Quarto 1 performs in
//! `src/resources/filters/modules/authors.lua`.
//!
//! **Phase 1 scope (bd-tezzk9vp):** name extraction only — enough for
//! the title block to render one `<p>` per author and pluralize its
//! heading. Phase 2 (bd-ez0hiowa) grows this into the full Q1 author
//! schema (structured names with particles, degrees, orcid, email,
//! url, roles, attribute flags, affiliations with `ref:` resolution)
//! and becomes the source for `DocumentProfile`'s structured author
//! field. Plan: `claude-notes/plans/2026-07-15-html-title-block-parity.md`.

use quarto_pandoc_types::ConfigValue;

/// One document author.
///
/// Phase 1 carries only the resolved display name; the fields of Q1's
/// normalized author schema land in Phase 2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Author {
    /// The author's name.
    pub name: AuthorName,
}

/// An author's name.
///
/// Phase 1 resolves every input shape down to the `literal` display
/// form. Phase 2 adds the structured components (`given`, `family`,
/// particles) alongside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorName {
    /// The full display name ("Norah Jones").
    pub literal: String,
}

/// Parse the document's authors from metadata.
///
/// Accepts, per Quarto 1 semantics (both `author` and `authors` keys,
/// first non-empty wins):
///
/// - a single scalar string → one author
/// - an array of strings → one author each
/// - an array of maps with a `name` key → one author each
/// - a single map with a `name` key → one author
///
/// A `name` value may itself be a scalar or a map with
/// `literal` / `given` / `family` / `dropping-particle` /
/// `non-dropping-particle` components; components are joined in
/// display order when no `literal` is given.
///
/// Entries whose name cannot be resolved are dropped.
pub fn parse_authors(meta: &ConfigValue) -> Vec<Author> {
    for key in ["author", "authors"] {
        if let Some(value) = meta.get(key) {
            if let Some(arr) = value.as_array() {
                let authors: Vec<Author> = arr.iter().filter_map(parse_author_entry).collect();
                if !authors.is_empty() {
                    return authors;
                }
            } else if let Some(author) = parse_author_entry(value) {
                return vec![author];
            }
        }
    }
    Vec::new()
}

/// Parse one author entry (scalar name or map with a `name` key).
fn parse_author_entry(value: &ConfigValue) -> Option<Author> {
    let literal = if let Some(s) = value.as_plain_text() {
        s
    } else {
        let name = value.get("name")?;
        resolve_name_literal(name)?
    };
    let literal = literal.trim().to_string();
    if literal.is_empty() {
        return None;
    }
    Some(Author {
        name: AuthorName { literal },
    })
}

/// Resolve a `name` value (scalar, or component map) to a display
/// string.
///
/// Component join order follows CSL display order:
/// `given [dropping-particle] [non-dropping-particle] family`.
/// An explicit `literal` wins over components.
fn resolve_name_literal(name: &ConfigValue) -> Option<String> {
    if let Some(s) = name.as_plain_text() {
        return Some(s);
    }
    if let Some(lit) = name.get("literal").and_then(|v| v.as_plain_text()) {
        return Some(lit);
    }
    let mut parts: Vec<String> = Vec::new();
    for key in [
        "given",
        "dropping-particle",
        "non-dropping-particle",
        "family",
    ] {
        if let Some(part) = name.get(key).and_then(|v| v.as_plain_text()) {
            let part = part.trim().to_string();
            if !part.is_empty() {
                parts.push(part);
            }
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::ConfigMapEntry;
    use quarto_source_map::{FileId, Location, Range, SourceInfo};

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

    fn names(meta: &ConfigValue) -> Vec<String> {
        parse_authors(meta)
            .into_iter()
            .map(|a| a.name.literal)
            .collect()
    }

    #[test]
    fn scalar_author() {
        let meta = map(vec![("author", s("Norah Jones"))]);
        assert_eq!(names(&meta), vec!["Norah Jones"]);
    }

    #[test]
    fn string_list_authors() {
        let meta = map(vec![(
            "author",
            arr(vec![s("Amelia Earhart"), s("Bill Malone")]),
        )]);
        assert_eq!(names(&meta), vec!["Amelia Earhart", "Bill Malone"]);
    }

    #[test]
    fn authors_key_accepted() {
        let meta = map(vec![("authors", arr(vec![s("Norah Jones")]))]);
        assert_eq!(names(&meta), vec!["Norah Jones"]);
    }

    #[test]
    fn map_authors_with_scalar_name() {
        let meta = map(vec![(
            "author",
            arr(vec![
                map(vec![("name", s("Norah Jones")), ("orcid", s("0000"))]),
                map(vec![("name", s("Bill Malone"))]),
            ]),
        )]);
        assert_eq!(names(&meta), vec!["Norah Jones", "Bill Malone"]);
    }

    #[test]
    fn single_map_author() {
        let meta = map(vec![("author", map(vec![("name", s("Norah Jones"))]))]);
        assert_eq!(names(&meta), vec!["Norah Jones"]);
    }

    #[test]
    fn structured_name_components() {
        let meta = map(vec![(
            "author",
            arr(vec![map(vec![(
                "name",
                map(vec![
                    ("given", s("Vincent")),
                    ("family", s("Gogh")),
                    ("non-dropping-particle", s("van")),
                ]),
            )])]),
        )]);
        assert_eq!(names(&meta), vec!["Vincent van Gogh"]);
    }

    #[test]
    fn structured_name_literal_wins() {
        let meta = map(vec![(
            "author",
            arr(vec![map(vec![(
                "name",
                map(vec![("literal", s("N. Jones")), ("given", s("Norah"))]),
            )])]),
        )]);
        assert_eq!(names(&meta), vec!["N. Jones"]);
    }

    #[test]
    fn nameless_entry_dropped() {
        let meta = map(vec![(
            "author",
            arr(vec![map(vec![("orcid", s("0000"))]), s("Bill Malone")]),
        )]);
        assert_eq!(names(&meta), vec!["Bill Malone"]);
    }

    #[test]
    fn no_author_key() {
        let meta = map(vec![("title", s("Untitled"))]);
        assert!(names(&meta).is_empty());
    }

    #[test]
    fn corresponding_bool_does_not_leak() {
        // The bd-8v34zny5 shape: boolean attribute values must never
        // surface as author names.
        let meta = map(vec![(
            "author",
            arr(vec![map(vec![
                ("name", s("Norah Jones")),
                ("corresponding", ConfigValue::new_bool(true, si())),
            ])]),
        )]);
        assert_eq!(names(&meta), vec!["Norah Jones"]);
    }
}
