/*
 * typst_citations.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Citations that Typst's native bibliography cannot resolve
//! (bd-yjf99rzx).
//!
//! Typst keeps Quarto 1's `citeproc: false` default: a citation reaches
//! Typst as `@key`, and Typst looks the key up in the files its
//! `#bibliography(...)` call names, which the template builds from
//! `bibliography:`. Front-matter `references:` never reach Typst. So a
//! citation whose key is defined only in `references:` fails `typst
//! compile` with `label <key> does not exist in the document` (Quarto 1
//! fails the same way). This module finds that case before Typst runs:
//!
//! - `Q-21-5` (error): a cited key is defined in `references:` and in no
//!   `bibliography:` file.
//! - `Q-21-6` (warning): a cited key is defined in `references:` and in a
//!   `bibliography:` file. Typst uses the file's entry; the inline one is
//!   silently ignored, in Quarto 1 as well.
//!
//! Only keys defined in `references:` are checked. A cited key defined
//! nowhere is bd-74zz2qko. When a `bibliography:` file cannot be read or
//! parsed, a key missing from the other files may still be in it, so no
//! `Q-21-5` is reported (Typst then reports the file itself).

use std::collections::{HashMap, HashSet};
use std::path::Path;

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::{Block, ConfigValue, Inline};
use quarto_source_map::SourceInfo;

/// One `bibliography:` file as Typst will read it.
#[derive(Debug, Clone)]
pub struct BibliographyFile {
    /// How the diagnostics name the file.
    pub display: String,
    /// The entry keys it defines, or `None` when the file could not be
    /// read or parsed.
    pub keys: Option<HashSet<String>>,
}

/// The diagnostics for one document.
#[derive(Debug, Default)]
pub struct CitationCheck {
    /// `Q-21-5` errors: Typst would fail on these citations.
    pub errors: Vec<DiagnosticMessage>,
    /// `Q-21-6` warnings.
    pub warnings: Vec<DiagnosticMessage>,
}

/// The entry keys of a bibliography file Typst can read: BibLaTeX `.bib`
/// or Hayagriva `.yml`/`.yaml` (the only formats Typst's
/// `#bibliography` accepts). `None` when the format is another one or the
/// content does not parse.
pub fn bibliography_keys(path: &Path, content: &str) -> Option<HashSet<String>> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "bib" => {
            let bibliography = biblatex::Bibliography::parse(content).ok()?;
            Some(bibliography.iter().map(|e| e.key.clone()).collect())
        }
        "yml" | "yaml" => {
            let mapping: serde_yaml::Mapping = serde_yaml::from_str(content).ok()?;
            mapping
                .keys()
                .map(|k| match k {
                    serde_yaml::Value::String(s) => Some(s.clone()),
                    serde_yaml::Value::Number(n) => Some(n.to_string()),
                    _ => None,
                })
                .collect()
        }
        _ => None,
    }
}

/// Check the citations that will reach Typst as `@key`.
///
/// `blocks` is the final document AST: by now citeproc has replaced every
/// `Cite` it resolves, so a remaining `Cite` reaches Typst. `files` are
/// the document's `bibliography:` files. Nothing is reported when
/// `citeproc: true` is set (in `citation-location: margin` mode the
/// citations are then formatted inside Pandoc, after this check).
///
/// `typst_compiles` is false when the render stops at the Typst source
/// (`output-ext` other than `pdf`). Typst does not run then, so an
/// inline-only key is a `Q-21-5` warning rather than an error: the source
/// is still written, and compiling it later will fail.
pub fn check(
    blocks: &mut [Block],
    meta: &ConfigValue,
    files: &[BibliographyFile],
    typst_compiles: bool,
) -> CitationCheck {
    let mut result = CitationCheck::default();
    if meta.get("citeproc").and_then(|v| v.as_bool()) == Some(true) {
        return result;
    }
    let inline_ids = inline_reference_ids(meta);
    if inline_ids.is_empty() {
        return result;
    }

    for (key, location) in cited_keys(blocks) {
        if !inline_ids.contains(&key) {
            continue;
        }
        let holder = files
            .iter()
            .find(|f| f.keys.as_ref().is_some_and(|keys| keys.contains(&key)));
        match holder {
            Some(file) => result
                .warnings
                .push(shadowed(&key, &file.display, location)),
            None if files.iter().all(|f| f.keys.is_some()) => {
                let diagnostic = inline_only(&key, files, location, typst_compiles);
                if typst_compiles {
                    result.errors.push(diagnostic);
                } else {
                    result.warnings.push(diagnostic);
                }
            }
            None => {}
        }
    }
    result
}

/// The `id`s of the front-matter `references:` entries.
fn inline_reference_ids(meta: &ConfigValue) -> HashSet<String> {
    meta.get("references")
        .and_then(|v| v.as_array())
        .unwrap_or_default()
        .iter()
        .filter_map(|entry| entry.get("id").and_then(|id| id.as_plain_text()))
        .collect()
}

/// Each cited key once, in document order, with the source of its first
/// citation.
fn cited_keys(blocks: &mut [Block]) -> Vec<(String, Option<SourceInfo>)> {
    let mut keys: Vec<(String, Option<SourceInfo>)> = Vec::new();
    let mut seen: HashMap<String, ()> = HashMap::new();
    crate::ast_walk::for_each_inline_mut(blocks, &mut |inline| {
        if let Inline::Cite(cite) = inline {
            for citation in &cite.citations {
                if seen.insert(citation.id.clone(), ()).is_none() {
                    let location = citation
                        .id_source
                        .clone()
                        .unwrap_or_else(|| cite.source_info.clone());
                    keys.push((citation.id.clone(), Some(location)));
                }
            }
        }
    });
    keys
}

fn inline_only(
    key: &str,
    files: &[BibliographyFile],
    location: Option<SourceInfo>,
    typst_compiles: bool,
) -> DiagnosticMessage {
    const TITLE: &str = "Inline Reference Not Available To Typst";
    let builder = if typst_compiles {
        DiagnosticMessageBuilder::error(TITLE)
    } else {
        DiagnosticMessageBuilder::warning(TITLE)
    };
    let mut builder = builder.with_code("Q-21-5").problem(format!(
        "The citation `@{key}` refers to an entry in the front-matter `references:` \
             list, which Typst's native bibliography does not read."
    ));
    if !files.is_empty() {
        let names: Vec<String> = files.iter().map(|f| format!("`{}`", f.display)).collect();
        builder = builder.add_detail(format!(
            "`{key}` is not defined in the `bibliography:` files: {}",
            names.join(", ")
        ));
    }
    if !typst_compiles {
        builder = builder.add_detail(format!(
            "The Typst source is still written, but compiling it will fail with \
             `label <{key}> does not exist in the document`."
        ));
    }
    builder = builder.add_hint(
        "Set `citeproc: true` to format citations with citeproc, or move the entry into a \
         `bibliography:` file (BibLaTeX `.bib` or Hayagriva `.yml`)",
    );
    if let Some(location) = location {
        builder = builder.with_location(location);
    }
    builder.build()
}

fn shadowed(key: &str, file: &str, location: Option<SourceInfo>) -> DiagnosticMessage {
    let mut builder = DiagnosticMessageBuilder::warning("Inline Reference Ignored By Typst")
        .with_code("Q-21-6")
        .problem(format!(
            "The citation `@{key}` has entries in both the front-matter `references:` list \
             and `{file}`. Typst uses the entry from `{file}` and ignores the inline one."
        ))
        .add_hint(
            "Remove one of the two entries, or set `citeproc: true` to format citations \
             with citeproc",
        );
    if let Some(location) = location {
        builder = builder.with_location(location);
    }
    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::{Citation, CitationMode, Cite, ConfigMapEntry, Paragraph, Str};

    fn si() -> SourceInfo {
        SourceInfo::for_test()
    }

    fn cite(keys: &[&str]) -> Inline {
        Inline::Cite(Box::new(Cite {
            citations: keys
                .iter()
                .map(|k| Citation {
                    id: k.to_string(),
                    prefix: vec![],
                    suffix: vec![],
                    mode: CitationMode::NormalCitation,
                    note_num: 1,
                    hash: 0,
                    id_source: Some(si()),
                })
                .collect(),
            content: vec![],
            source_info: si(),
        }))
    }

    fn doc(inlines: Vec<Inline>) -> Vec<Block> {
        vec![Block::Paragraph(Paragraph {
            content: inlines,
            source_info: si(),
        })]
    }

    fn text(s: &str) -> Inline {
        Inline::Str(Str {
            text: s.to_string(),
            source_info: si(),
        })
    }

    fn entry(key: &str, value: ConfigValue) -> ConfigMapEntry {
        ConfigMapEntry {
            key: key.to_string(),
            key_source: si(),
            value,
        }
    }

    /// Metadata with `references:` entries for `ids`, plus `extra` keys.
    fn meta(ids: &[&str], extra: Vec<ConfigMapEntry>) -> ConfigValue {
        let references = ids
            .iter()
            .map(|id| {
                ConfigValue::new_map(vec![entry("id", ConfigValue::new_string(*id, si()))], si())
            })
            .collect();
        let mut entries = vec![entry(
            "references",
            ConfigValue::new_array(references, si()),
        )];
        entries.extend(extra);
        ConfigValue::new_map(entries, si())
    }

    fn file(display: &str, keys: Option<&[&str]>) -> BibliographyFile {
        BibliographyFile {
            display: display.to_string(),
            keys: keys.map(|ks| ks.iter().map(|k| k.to_string()).collect()),
        }
    }

    fn codes(messages: &[DiagnosticMessage]) -> Vec<String> {
        messages
            .iter()
            .map(|m| m.code.clone().unwrap_or_default())
            .collect()
    }

    #[test]
    fn inline_only_key_is_an_error() {
        let mut blocks = doc(vec![text("See "), cite(&["item"])]);
        let result = check(&mut blocks, &meta(&["item"], vec![]), &[], true);
        assert_eq!(codes(&result.errors), ["Q-21-5"]);
        assert!(result.warnings.is_empty());
        let text = result.errors[0].to_text(None);
        assert!(
            text.contains("@item") && text.contains("citeproc: true"),
            "{text}"
        );
    }

    #[test]
    fn inline_only_key_with_other_bibliography_files_is_an_error() {
        let mut blocks = doc(vec![cite(&["item"])]);
        let files = [file("refs.bib", Some(&["knuth"]))];
        let result = check(&mut blocks, &meta(&["item"], vec![]), &files, true);
        assert_eq!(codes(&result.errors), ["Q-21-5"]);
        assert!(result.errors[0].to_text(None).contains("refs.bib"));
    }

    #[test]
    fn key_also_in_a_bibliography_file_is_a_warning() {
        let mut blocks = doc(vec![cite(&["both"])]);
        let files = [file("refs.bib", Some(&["both"]))];
        let result = check(&mut blocks, &meta(&["both"], vec![]), &files, true);
        assert!(result.errors.is_empty());
        assert_eq!(codes(&result.warnings), ["Q-21-6"]);
        assert!(result.warnings[0].to_text(None).contains("refs.bib"));
    }

    #[test]
    fn uncited_inline_entries_and_bibliography_keys_are_fine() {
        let mut blocks = doc(vec![cite(&["knuth"])]);
        let files = [file("refs.bib", Some(&["knuth"]))];
        let result = check(&mut blocks, &meta(&["item"], vec![]), &files, true);
        assert!(result.errors.is_empty() && result.warnings.is_empty());
    }

    #[test]
    fn citeproc_true_turns_the_check_off() {
        let mut blocks = doc(vec![cite(&["item"])]);
        let extra = vec![entry("citeproc", ConfigValue::new_bool(true, si()))];
        let result = check(&mut blocks, &meta(&["item"], extra), &[], true);
        assert!(result.errors.is_empty() && result.warnings.is_empty());
    }

    #[test]
    fn an_unreadable_bibliography_file_suppresses_the_error() {
        let mut blocks = doc(vec![cite(&["item"])]);
        let files = [file("refs.bib", Some(&["knuth"])), file("broken.bib", None)];
        let result = check(&mut blocks, &meta(&["item"], vec![]), &files, true);
        assert!(result.errors.is_empty() && result.warnings.is_empty());
    }

    #[test]
    fn each_key_is_reported_once() {
        let mut blocks = doc(vec![
            cite(&["item"]),
            text(" and "),
            cite(&["item", "other"]),
        ]);
        let result = check(&mut blocks, &meta(&["item", "other"], vec![]), &[], true);
        assert_eq!(codes(&result.errors), ["Q-21-5", "Q-21-5"]);
    }

    #[test]
    fn typst_source_only_output_makes_the_inline_only_key_a_warning() {
        let mut blocks = doc(vec![cite(&["item"])]);
        let result = check(&mut blocks, &meta(&["item"], vec![]), &[], false);
        assert!(result.errors.is_empty());
        assert_eq!(codes(&result.warnings), ["Q-21-5"]);
        let text = result.warnings[0].to_text(None);
        assert!(text.contains("still written"), "{text}");
    }

    #[test]
    fn no_references_means_nothing_to_check() {
        let mut blocks = doc(vec![cite(&["item"])]);
        let result = check(&mut blocks, &ConfigValue::new_map(vec![], si()), &[], true);
        assert!(result.errors.is_empty() && result.warnings.is_empty());
    }

    #[test]
    fn bib_keys_are_read() {
        let keys = bibliography_keys(
            Path::new("refs.bib"),
            "@book{knuth, title={The TeXbook}, year={1984}}\n@online{web, title={W}}\n",
        )
        .unwrap();
        assert_eq!(
            keys,
            HashSet::from(["knuth".to_string(), "web".to_string()])
        );
    }

    #[test]
    fn hayagriva_keys_are_read() {
        let keys = bibliography_keys(
            Path::new("refs.yml"),
            "item:\n  type: web\n  title: A page\nother:\n  type: book\n  title: B\n",
        )
        .unwrap();
        assert_eq!(
            keys,
            HashSet::from(["item".to_string(), "other".to_string()])
        );
    }

    #[test]
    fn unparseable_or_unsupported_files_have_no_keys() {
        assert!(bibliography_keys(Path::new("refs.bib"), "@book{oops, title={").is_none());
        assert!(bibliography_keys(Path::new("refs.yml"), "- a list\n- not a map\n").is_none());
        assert!(bibliography_keys(Path::new("refs.json"), "[]").is_none());
    }
}
