/*
 * pandoc_filters/author_names.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Rewrites object-form `author` entries to plain names before the
 * Pandoc-hybrid leg hands its metadata to pandoc (GH #1316).
 */

//! Object-form `author` entries → plain names for pandoc's writers.
//!
//! Quarto lets an author be a map (`- name: Jane Doe` plus
//! `affiliation:`, `orcid:`, …). Pandoc's own writers (docx, pptx, epub,
//! …) only read `author` entries that are inlines or strings, so a map
//! entry is silently dropped: no author under the title, an empty
//! `dc:creator`.
//!
//! Quarto 1 resolves this in TypeScript before running pandoc
//! (`src/command/render/pandoc.ts`, "Resolve the author metadata into a
//! form that Pandoc will recognize"): `author` becomes one plain name per
//! entry and the raw entries move to `authors`, where the vendored
//! `authors.lua` filter reads them. [`author_names_for_pandoc`] does the
//! same, using [`parse_authors`] for the name, with Quarto 1's strictness:
//! if any entry is not recognized as an author, `author` is left alone.
//!
//! Plain-name entries are kept as they are (their inline formatting
//! included), so a document without object-form authors serializes
//! exactly as before.

use quarto_pandoc_types::{ConfigMapEntry, ConfigValue};

use crate::metadata::authors::parse_authors;

/// Rewrite object-form `author` entries in `meta` to plain-name strings,
/// and (when `authors` is absent) keep the raw entries in `authors`.
///
/// A no-op when `author` is absent, has no map entries, or has an entry
/// that is not recognized as an author. Idempotent: after one call
/// `author` has no map entries left.
pub fn author_names_for_pandoc(meta: &mut ConfigValue) {
    let Some(author) = meta.get("author") else {
        return;
    };
    let entries: Vec<ConfigValue> = match author.as_array() {
        Some(items) => items.to_vec(),
        None => vec![author.clone()],
    };
    if !entries.iter().any(ConfigValue::is_map) {
        return;
    }
    let source_info = author.source_info.clone();

    // Parse `author` on its own, so `parse_authors` cannot fall back to an
    // `authors` key when `author` yields nothing.
    let author_only = ConfigValue::new_map(
        vec![ConfigMapEntry {
            key: "author".to_string(),
            key_source: source_info.clone(),
            value: author.clone(),
        }],
        source_info.clone(),
    );
    let parsed = parse_authors(&author_only);
    if parsed.len() != entries.len() {
        return;
    }

    let names = entries
        .iter()
        .zip(&parsed)
        .map(|(entry, parsed)| {
            if entry.is_map() {
                ConfigValue::new_string(parsed.name.literal.clone(), entry.source_info.clone())
            } else {
                entry.clone()
            }
        })
        .collect();
    if !meta.contains_key("authors") {
        meta.insert_path(
            &["authors"],
            ConfigValue::new_array(entries, source_info.clone()),
        );
    }
    meta.insert_path(&["author"], ConfigValue::new_array(names, source_info));
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_source_map::SourceInfo;

    fn si() -> SourceInfo {
        SourceInfo::for_test()
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

    fn texts(value: &ConfigValue) -> Vec<String> {
        value
            .as_array()
            .expect("an array")
            .iter()
            .map(|v| v.as_plain_text().expect("plain text"))
            .collect()
    }

    fn jane() -> ConfigValue {
        map(vec![
            ("name", s("Jane Doe")),
            ("affiliation", s("Example Institute")),
        ])
    }

    #[test]
    fn object_author_becomes_name_and_moves_to_authors() {
        let mut meta = map(vec![("author", ConfigValue::new_array(vec![jane()], si()))]);
        author_names_for_pandoc(&mut meta);

        assert_eq!(texts(meta.get("author").unwrap()), vec!["Jane Doe"]);
        let authors = meta.get("authors").unwrap().as_array().unwrap();
        assert_eq!(authors.len(), 1);
        assert_eq!(
            authors[0].get("affiliation").unwrap().as_plain_text(),
            Some("Example Institute".to_string())
        );
    }

    #[test]
    fn single_object_author_is_listed() {
        let mut meta = map(vec![("author", jane())]);
        author_names_for_pandoc(&mut meta);

        assert_eq!(texts(meta.get("author").unwrap()), vec!["Jane Doe"]);
        assert_eq!(meta.get("authors").unwrap().as_array().unwrap().len(), 1);
    }

    #[test]
    fn structured_name_joins_its_parts() {
        let name = map(vec![
            ("given", s("Vincent")),
            ("non-dropping-particle", s("van")),
            ("family", s("Gogh")),
        ]);
        let mut meta = map(vec![(
            "author",
            ConfigValue::new_array(vec![map(vec![("name", name)]), s("Alice")], si()),
        )]);
        author_names_for_pandoc(&mut meta);

        assert_eq!(
            texts(meta.get("author").unwrap()),
            vec!["Vincent van Gogh", "Alice"]
        );
    }

    #[test]
    fn plain_names_are_untouched() {
        let mut meta = map(vec![(
            "author",
            ConfigValue::new_array(vec![s("Alice"), s("Bob")], si()),
        )]);
        let before = meta.clone();
        author_names_for_pandoc(&mut meta);
        assert_eq!(meta, before);
    }

    #[test]
    fn existing_authors_key_is_kept() {
        let existing = ConfigValue::new_array(vec![s("Someone Else")], si());
        let mut meta = map(vec![("author", jane()), ("authors", existing.clone())]);
        author_names_for_pandoc(&mut meta);

        assert_eq!(texts(meta.get("author").unwrap()), vec!["Jane Doe"]);
        assert_eq!(meta.get("authors"), Some(&existing));
    }

    #[test]
    fn unrecognized_entry_leaves_author_alone() {
        let mut meta = map(vec![(
            "author",
            ConfigValue::new_array(vec![jane(), map(vec![("email", s("x@y.z"))])], si()),
        )]);
        let before = meta.clone();
        author_names_for_pandoc(&mut meta);
        assert_eq!(meta, before);
    }

    #[test]
    fn is_idempotent() {
        let mut meta = map(vec![("author", jane())]);
        author_names_for_pandoc(&mut meta);
        let once = meta.clone();
        author_names_for_pandoc(&mut meta);
        assert_eq!(meta, once);
    }
}
