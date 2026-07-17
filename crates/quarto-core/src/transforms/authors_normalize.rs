/*
 * authors_normalize.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Transform that normalizes author metadata and title-block labels.
 */

//! Author/label normalization transform.
//!
//! The Rust counterpart of Quarto 1's `authors.lua` metadata pass: it
//! derives, from the raw `author`/`authors`/`affiliations`/`funding`
//! metadata,
//!
//! - **`authors`** — the normalized author list (affiliations in
//!   `{ref: id}` form), the Q1 output shape.
//! - **`affiliations`** — the normalized affiliation list with stable
//!   ids (`aff-N` when unspecified) and `number`/`letter` counters.
//! - **`by-author`** — authors with affiliations expanded inline; the
//!   shape the title-block templates iterate
//!   (`$for(by-author)$ … $it.name.literal$`, `$it.orcid$`, …).
//! - **`by-affiliation`** — affiliations with their authors expanded
//!   inline.
//! - **`funding`** — normalized funding groups (schema only; nothing
//!   in HTML consumes it, per design decision Q7).
//! - **`labels`** — the localizable title-block heading labels
//!   (`labels.authors`, `labels.affiliations`, `labels.published`,
//!   …), honoring the per-document `*-title` override options
//!   (`author-title`, `affiliation-title`, `published-title`,
//!   `abstract-title`, `modified-title`, `doi-title`,
//!   `description-title`). Defaults are hardcoded English for now; a
//!   language-file system akin to Q1's `_language.yml` is future work
//!   tracked in the epic plan (decision Q3).
//! - **`author-meta`** — one plain-text name per author, consumed by
//!   the template head's `<meta name="author" …>` loop.
//! - **`rendered.has-title-block`** — a Q2-internal flag: true when
//!   any title-block content exists (title, subtitle, authors, date,
//!   abstract, or — since P3, bd-j6huijli — date-modified, doi,
//!   keywords, description, categories). The built-in `title-block`
//!   template partial keys on it so documents with no title-block
//!   metadata emit no empty `<header>`.
//! - **`quarto-template-params.title-block-categories`** — Q1's
//!   template-param contract for the category chips: written (true)
//!   unless the document sets `title-block-categories: false`.
//!
//! Like Q1's Lua pass, the derived keys are written into document
//! metadata (not a side channel) so downstream consumers — templates,
//! Lua filters, and the q2-preview React title block, which reads the
//! same metadata — all see one normalization.
//!
//! Plan: `claude-notes/plans/2026-07-15-html-title-block-parity.md`
//! (Phase 2, bd-ez0hiowa).

use quarto_error_reporting::DiagnosticMessage;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_pandoc_types::{ConfigMapEntry, ConfigValue};
use quarto_source_map::{By, SourceInfo};
use yaml_rust2::Yaml;

use crate::Result;
use crate::metadata::authors::{
    Affiliation, Author, AuthorsModel, Award, FundingGroup, FundingParty, FundingSource,
    parse_authors_model,
};
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

/// Transform that writes the normalized author/affiliation model,
/// `labels`, and `rendered.has-title-block` into document metadata.
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

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        let issues = normalize_authors_meta(&mut ast.meta);
        for issue in issues {
            ctx.diagnostics.push(DiagnosticMessage::warning(issue));
        }
        Ok(())
    }
}

fn gen_si() -> SourceInfo {
    SourceInfo::generated(By::programmatic_config())
}

/// Derive the normalized author metadata (see the module docs for the
/// full key list). Returns human-readable normalization problems
/// (e.g. an undefined affiliation `ref:`) for the caller to surface.
///
/// Recomputes (overwrites) the derived keys on every run, like Q1's
/// Lua pass — the raw `author` metadata is the source of truth.
pub fn normalize_authors_meta(meta: &mut ConfigValue) -> Vec<String> {
    if !meta.is_map() {
        return Vec::new();
    }

    let model = parse_authors_model(meta);

    if !model.authors.is_empty() {
        let authors = ConfigValue::new_array(
            model
                .authors
                .iter()
                .map(|a| author_to_config_value(a, &model, false))
                .collect(),
            gen_si(),
        );
        meta.insert_path(&["authors"], authors);

        let by_author = ConfigValue::new_array(
            model
                .authors
                .iter()
                .map(|a| author_to_config_value(a, &model, true))
                .collect(),
            gen_si(),
        );
        meta.insert_path(&["by-author"], by_author);

        // Pandoc-convention `author-meta`: one plain-text name per
        // author, consumed by the template head's
        // `$for(author-meta)$<meta name="author" …>$endfor$`. Without
        // it, structured author maps would flatten into the meta tag
        // (the bd-8v34zny5 boolean-garbage shape).
        let author_meta = ConfigValue::new_array(
            model
                .authors
                .iter()
                .map(|a| ConfigValue::new_string(a.name.literal.clone(), gen_si()))
                .collect(),
            gen_si(),
        );
        meta.insert_path(&["author-meta"], author_meta);
    }

    if !model.affiliations.is_empty() {
        let affiliations = ConfigValue::new_array(
            model
                .affiliations
                .iter()
                .map(affiliation_to_config_value)
                .collect(),
            gen_si(),
        );
        meta.insert_path(&["affiliations"], affiliations);

        let by_affiliation = ConfigValue::new_array(
            model
                .affiliations
                .iter()
                .map(|aff| {
                    let mut entries = affiliation_entries(aff);
                    let authors: Vec<ConfigValue> = model
                        .authors
                        .iter()
                        .filter(|a| a.affiliations.iter().any(|r| r == &aff.id))
                        .map(|a| author_to_config_value(a, &model, false))
                        .collect();
                    if !authors.is_empty() {
                        entries.push(map_entry(
                            "authors",
                            ConfigValue::new_array(authors, gen_si()),
                        ));
                    }
                    ConfigValue::new_map(entries, gen_si())
                })
                .collect(),
            gen_si(),
        );
        meta.insert_path(&["by-affiliation"], by_affiliation);
    }

    if !model.funding.is_empty() {
        let funding = ConfigValue::new_array(
            model
                .funding
                .iter()
                .map(funding_group_to_config_value)
                .collect(),
            gen_si(),
        );
        meta.insert_path(&["funding"], funding);
    }

    let labels = compute_labels(meta, model.authors.len(), model.affiliations.len());
    meta.insert_path(&["labels"], labels);

    if has_title_block_content(meta, &model.authors) {
        meta.insert_path(
            &["rendered", "has-title-block"],
            ConfigValue::new_bool(true, gen_si()),
        );
    }

    // Q1's template-param contract: the title-block partial gates the
    // category chips on `quarto-template-params.title-block-categories`
    // (not on the raw option, whose *absence* must mean "show"). Using
    // Q1's exact key keeps Q1-ported custom `template-partials`
    // working. Written only when enabled — an absent variable is false
    // in `$if(…)$`.
    if meta.get("title-block-categories").and_then(|v| v.as_bool()) != Some(false) {
        meta.insert_path(
            &["quarto-template-params", "title-block-categories"],
            ConfigValue::new_bool(true, gen_si()),
        );
    }

    // `title-block-style: none` (P6, bd-vkiwhcny): the template's
    // Pandoc-fallback branch and the preview's none-branch key on this
    // flag. `plain` changes no markup (it only drops the SCSS layer,
    // handled by `ThemeConfig`), so only `none` gets a derived key.
    if crate::transforms::TitleBlockStyle::from_meta(meta)
        == crate::transforms::TitleBlockStyle::None
    {
        meta.insert_path(
            &["rendered", "title-block-none"],
            ConfigValue::new_bool(true, gen_si()),
        );
    }

    model.issues
}

fn map_entry(key: &str, value: ConfigValue) -> ConfigMapEntry {
    ConfigMapEntry {
        key: key.to_string(),
        key_source: gen_si(),
        value,
    }
}

fn string_entry(key: &str, value: &str) -> ConfigMapEntry {
    map_entry(key, ConfigValue::new_string(value, gen_si()))
}

fn int_entry(key: &str, value: usize) -> ConfigMapEntry {
    map_entry(
        key,
        ConfigValue::new_scalar(Yaml::Integer(value as i64), gen_si()),
    )
}

fn push_opt(entries: &mut Vec<ConfigMapEntry>, key: &str, value: &Option<String>) {
    if let Some(v) = value {
        entries.push(string_entry(key, v));
    }
}

/// Serialize an author into the Q1 `by-author`/`authors` entry shape.
/// `denormalize` expands affiliation refs into full objects
/// (`by-author`); otherwise they stay `{ref: id}` (`authors`).
fn author_to_config_value(author: &Author, model: &AuthorsModel, denormalize: bool) -> ConfigValue {
    let mut entries: Vec<ConfigMapEntry> = Vec::new();

    // Q1 uses the author number as the id when none is specified.
    match &author.id {
        Some(id) => entries.push(string_entry("id", id)),
        None => entries.push(int_entry("id", author.number)),
    }
    entries.push(int_entry("number", author.number));
    entries.push(string_entry("letter", &author.letter));

    let mut name_entries = vec![string_entry("literal", &author.name.literal)];
    push_opt(&mut name_entries, "given", &author.name.given);
    push_opt(&mut name_entries, "family", &author.name.family);
    push_opt(
        &mut name_entries,
        "dropping-particle",
        &author.name.dropping_particle,
    );
    push_opt(
        &mut name_entries,
        "non-dropping-particle",
        &author.name.non_dropping_particle,
    );
    entries.push(map_entry(
        "name",
        ConfigValue::new_map(name_entries, gen_si()),
    ));

    push_opt(&mut entries, "url", &author.url);
    push_opt(&mut entries, "email", &author.email);
    push_opt(&mut entries, "phone", &author.phone);
    push_opt(&mut entries, "fax", &author.fax);
    push_opt(&mut entries, "orcid", &author.orcid);
    push_opt(&mut entries, "acknowledgements", &author.acknowledgements);

    if !author.degrees.is_empty() {
        entries.push(map_entry(
            "degrees",
            ConfigValue::new_array(
                author
                    .degrees
                    .iter()
                    .map(|d| ConfigValue::new_string(d.clone(), gen_si()))
                    .collect(),
                gen_si(),
            ),
        ));
    }

    if let Some(note) = &author.note {
        entries.push(map_entry(
            "note",
            ConfigValue::new_map(
                vec![
                    int_entry("number", note.number),
                    string_entry("text", &note.text),
                ],
                gen_si(),
            ),
        ));
    }

    if !author.attributes.is_empty() {
        entries.push(map_entry(
            "attributes",
            ConfigValue::new_map(
                author
                    .attributes
                    .iter()
                    .map(|flag| map_entry(flag, ConfigValue::new_bool(true, gen_si())))
                    .collect(),
                gen_si(),
            ),
        ));
    }

    if !author.roles.is_empty() {
        entries.push(map_entry(
            "roles",
            ConfigValue::new_array(
                author
                    .roles
                    .iter()
                    .map(|role| {
                        let mut role_entries = vec![string_entry("role", &role.role)];
                        push_opt(
                            &mut role_entries,
                            "degree-of-contribution",
                            &role.degree_of_contribution,
                        );
                        push_opt(
                            &mut role_entries,
                            "vocab-identifier",
                            &role.vocab_identifier,
                        );
                        push_opt(&mut role_entries, "vocab-term", &role.vocab_term);
                        push_opt(
                            &mut role_entries,
                            "vocab-term-identifier",
                            &role.vocab_term_identifier,
                        );
                        ConfigValue::new_map(role_entries, gen_si())
                    })
                    .collect(),
                gen_si(),
            ),
        ));
    }

    if !author.metadata.is_empty() {
        entries.push(map_entry(
            "metadata",
            ConfigValue::new_map(author.metadata.clone(), gen_si()),
        ));
    }

    if !author.affiliations.is_empty() {
        let affs: Vec<ConfigValue> = if denormalize {
            author
                .affiliations
                .iter()
                .filter_map(|r| model.affiliations.iter().find(|a| &a.id == r))
                .map(affiliation_to_config_value)
                .collect()
        } else {
            author
                .affiliations
                .iter()
                .map(|r| ConfigValue::new_map(vec![string_entry("ref", r)], gen_si()))
                .collect()
        };
        entries.push(map_entry(
            "affiliations",
            ConfigValue::new_array(affs, gen_si()),
        ));
    }

    ConfigValue::new_map(entries, gen_si())
}

fn affiliation_entries(aff: &Affiliation) -> Vec<ConfigMapEntry> {
    let mut entries: Vec<ConfigMapEntry> = vec![
        string_entry("id", &aff.id),
        int_entry("number", aff.number),
        string_entry("letter", &aff.letter),
    ];
    push_opt(&mut entries, "name", &aff.name);
    push_opt(&mut entries, "department", &aff.department);
    push_opt(&mut entries, "group", &aff.group);
    push_opt(&mut entries, "address", &aff.address);
    push_opt(&mut entries, "city", &aff.city);
    push_opt(&mut entries, "region", &aff.region);
    push_opt(&mut entries, "country", &aff.country);
    push_opt(&mut entries, "postal-code", &aff.postal_code);
    push_opt(&mut entries, "url", &aff.url);
    push_opt(&mut entries, "isni", &aff.isni);
    push_opt(&mut entries, "ringgold", &aff.ringgold);
    push_opt(&mut entries, "ror", &aff.ror);
    if !aff.metadata.is_empty() {
        entries.push(map_entry(
            "metadata",
            ConfigValue::new_map(aff.metadata.clone(), gen_si()),
        ));
    }
    entries
}

fn affiliation_to_config_value(aff: &Affiliation) -> ConfigValue {
    ConfigValue::new_map(affiliation_entries(aff), gen_si())
}

fn funding_group_to_config_value(group: &FundingGroup) -> ConfigValue {
    let mut entries: Vec<ConfigMapEntry> = Vec::new();
    push_opt(&mut entries, "statement", &group.statement);
    push_opt(&mut entries, "open-access", &group.open_access);
    if !group.awards.is_empty() {
        entries.push(map_entry(
            "awards",
            ConfigValue::new_array(
                group.awards.iter().map(award_to_config_value).collect(),
                gen_si(),
            ),
        ));
    }
    ConfigValue::new_map(entries, gen_si())
}

fn award_to_config_value(award: &Award) -> ConfigValue {
    let mut entries: Vec<ConfigMapEntry> = Vec::new();
    push_opt(&mut entries, "id", &award.id);
    push_opt(&mut entries, "name", &award.name);
    push_opt(&mut entries, "description", &award.description);
    if !award.source.is_empty() {
        entries.push(map_entry(
            "source",
            ConfigValue::new_array(
                award
                    .source
                    .iter()
                    .map(funding_source_to_config_value)
                    .collect(),
                gen_si(),
            ),
        ));
    }
    for (key, parties) in [
        ("recipient", &award.recipient),
        ("investigator", &award.investigator),
    ] {
        if !parties.is_empty() {
            entries.push(map_entry(
                key,
                ConfigValue::new_array(
                    parties.iter().map(funding_party_to_config_value).collect(),
                    gen_si(),
                ),
            ));
        }
    }
    ConfigValue::new_map(entries, gen_si())
}

fn funding_party_entries(party: &FundingParty) -> Vec<ConfigMapEntry> {
    match party {
        FundingParty::Text(text) => vec![string_entry("text", text)],
        FundingParty::Name(name) => {
            let mut name_entries = vec![string_entry("literal", &name.literal)];
            push_opt(&mut name_entries, "given", &name.given);
            push_opt(&mut name_entries, "family", &name.family);
            vec![map_entry(
                "name",
                ConfigValue::new_map(name_entries, gen_si()),
            )]
        }
        FundingParty::Institution(aff) => {
            vec![map_entry("institution", affiliation_to_config_value(aff))]
        }
    }
}

fn funding_party_to_config_value(party: &FundingParty) -> ConfigValue {
    ConfigValue::new_map(funding_party_entries(party), gen_si())
}

fn funding_source_to_config_value(source: &FundingSource) -> ConfigValue {
    let mut entries = funding_party_entries(&source.party);
    push_opt(&mut entries, "country", &source.country);
    if let Some(t) = &source.source_type {
        entries.push(string_entry("type", t));
    }
    ConfigValue::new_map(entries, gen_si())
}

/// Compute the title-block heading labels.
///
/// Default English strings match Q1's `_language.yml` keys
/// (`title-block-author-single`, `title-block-affiliation-plural`,
/// `title-block-published`, …); the per-document `*-title` options
/// override them.
fn compute_labels(
    meta: &ConfigValue,
    author_count: usize,
    affiliation_count: usize,
) -> ConfigValue {
    let override_of = |key: &str| meta.get(key).and_then(|v| v.as_plain_text());

    let authors_label = override_of("author-title").unwrap_or_else(|| {
        if author_count > 1 {
            "Authors".to_string()
        } else {
            "Author".to_string()
        }
    });
    let affiliations_label = override_of("affiliation-title").unwrap_or_else(|| {
        if affiliation_count > 1 {
            "Affiliations".to_string()
        } else {
            "Affiliation".to_string()
        }
    });
    let entries = vec![
        map_entry("authors", ConfigValue::new_string(authors_label, gen_si())),
        map_entry(
            "affiliations",
            ConfigValue::new_string(affiliations_label, gen_si()),
        ),
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
/// The metadata-grid fields (`date-modified`, `doi`, `keywords`,
/// `description`, `categories`) count as content since P3
/// (bd-j6huijli). `categories` counts even when
/// `title-block-categories: false` — like Q1, the header (with its
/// always-present `quarto-title-meta` grid) still renders; only the
/// chips are suppressed.
fn has_title_block_content(meta: &ConfigValue, authors: &[Author]) -> bool {
    if !authors.is_empty() {
        return true;
    }
    [
        "title",
        "subtitle",
        "date",
        "abstract",
        "date-modified",
        "doi",
        "keywords",
        "description",
        "categories",
    ]
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
        // No categories → the template param is still written (the
        // chips gate is independent of whether categories exist).
        assert_eq!(
            meta.get_path(&["quarto-template-params", "title-block-categories"])
                .and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn metadata_grid_fields_set_title_block_flag() {
        // P3 (bd-j6huijli): each grid field alone is title-block content.
        for key in [
            "date-modified",
            "doi",
            "keywords",
            "description",
            "categories",
        ] {
            let mut meta = map(vec![(key, s("x"))]);
            normalize_authors_meta(&mut meta);
            assert_eq!(
                meta.get_path(&["rendered", "has-title-block"])
                    .and_then(|v| v.as_bool()),
                Some(true),
                "{key} should count as title-block content"
            );
        }
    }

    #[test]
    fn style_none_writes_title_block_none_flag() {
        // P6 (bd-vkiwhcny): the template's Pandoc-fallback branch keys
        // on `rendered.title-block-none`; plain/default write nothing.
        let mut meta = map(vec![("title", s("T")), ("title-block-style", s("none"))]);
        normalize_authors_meta(&mut meta);
        assert_eq!(
            meta.get_path(&["rendered", "title-block-none"])
                .and_then(|v| v.as_bool()),
            Some(true)
        );

        for style in ["plain", "default"] {
            let mut meta = map(vec![("title", s("T")), ("title-block-style", s(style))]);
            normalize_authors_meta(&mut meta);
            assert!(
                meta.get_path(&["rendered", "title-block-none"]).is_none(),
                "style {style} must not set the none flag"
            );
        }
    }

    #[test]
    fn title_block_categories_false_clears_template_param() {
        let mut meta = map(vec![
            ("title", s("T")),
            ("categories", arr(vec![s("analysis")])),
            ("title-block-categories", ConfigValue::new_bool(false, si())),
        ]);
        normalize_authors_meta(&mut meta);
        assert!(
            meta.get_path(&["quarto-template-params", "title-block-categories"])
                .is_none()
        );
        // The header itself still renders (chips-only suppression).
        assert_eq!(
            meta.get_path(&["rendered", "has-title-block"])
                .and_then(|v| v.as_bool()),
            Some(true)
        );
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

    // ── P2: full-model emission ──────────────────────────────────────

    fn rich_meta() -> ConfigValue {
        map(vec![(
            "author",
            arr(vec![
                map(vec![
                    ("name", s("Norah Jones")),
                    ("orcid", s("0000-0002-1825-0097")),
                    ("email", s("norah@example.com")),
                    ("url", s("https://example.com/norah")),
                    ("corresponding", ConfigValue::new_bool(true, si())),
                    ("degrees", arr(vec![s("PhD")])),
                    (
                        "affiliations",
                        arr(vec![map(vec![
                            ("name", s("Carnegie Mellon University")),
                            ("department", s("School of Music")),
                        ])]),
                    ),
                ]),
                map(vec![
                    ("name", s("Bill Malone")),
                    (
                        "affiliations",
                        arr(vec![map(vec![("name", s("University of Texas"))])]),
                    ),
                ]),
            ]),
        )])
    }

    #[test]
    fn by_author_carries_decorations_and_inline_affiliations() {
        let mut meta = rich_meta();
        normalize_authors_meta(&mut meta);

        let by_author = meta.get("by-author").and_then(|v| v.as_array()).unwrap();
        let norah = &by_author[0];
        assert_eq!(
            norah.get("orcid").and_then(|v| v.as_plain_text()),
            Some("0000-0002-1825-0097".to_string())
        );
        assert_eq!(
            norah.get("url").and_then(|v| v.as_plain_text()),
            Some("https://example.com/norah".to_string())
        );
        assert_eq!(
            norah
                .get_path(&["attributes", "corresponding"])
                .and_then(|v| v.as_bool()),
            Some(true)
        );
        let degrees = norah.get("degrees").and_then(|v| v.as_array()).unwrap();
        assert_eq!(degrees[0].as_plain_text(), Some("PhD".to_string()));
        // by-author affiliations are denormalized (full objects).
        let affs = norah
            .get("affiliations")
            .and_then(|v| v.as_array())
            .unwrap();
        assert_eq!(
            affs[0].get("name").and_then(|v| v.as_plain_text()),
            Some("Carnegie Mellon University".to_string())
        );
        assert_eq!(
            affs[0].get("department").and_then(|v| v.as_plain_text()),
            Some("School of Music".to_string())
        );
    }

    #[test]
    fn authors_key_keeps_refs() {
        let mut meta = rich_meta();
        normalize_authors_meta(&mut meta);

        let authors = meta.get("authors").and_then(|v| v.as_array()).unwrap();
        let affs = authors[0]
            .get("affiliations")
            .and_then(|v| v.as_array())
            .unwrap();
        assert_eq!(
            affs[0].get("ref").and_then(|v| v.as_plain_text()),
            Some("aff-1".to_string())
        );
        assert!(affs[0].get("name").is_none());
    }

    #[test]
    fn affiliations_and_by_affiliation_written() {
        let mut meta = rich_meta();
        normalize_authors_meta(&mut meta);

        let affiliations = meta.get("affiliations").and_then(|v| v.as_array()).unwrap();
        assert_eq!(affiliations.len(), 2);
        assert_eq!(
            affiliations[0].get("id").and_then(|v| v.as_plain_text()),
            Some("aff-1".to_string())
        );

        let by_affiliation = meta
            .get("by-affiliation")
            .and_then(|v| v.as_array())
            .unwrap();
        let cmu_authors = by_affiliation[0]
            .get("authors")
            .and_then(|v| v.as_array())
            .unwrap();
        assert_eq!(
            cmu_authors[0]
                .get_path(&["name", "literal"])
                .and_then(|v| v.as_plain_text()),
            Some("Norah Jones".to_string())
        );
    }

    #[test]
    fn affiliation_labels_pluralize_and_override() {
        let mut meta = rich_meta();
        normalize_authors_meta(&mut meta);
        assert_eq!(label(&meta, "affiliations"), "Affiliations");

        let mut single = map(vec![(
            "author",
            arr(vec![map(vec![
                ("name", s("A B")),
                ("affiliation", s("One University")),
            ])]),
        )]);
        normalize_authors_meta(&mut single);
        assert_eq!(label(&single, "affiliations"), "Affiliation");

        let mut overridden = map(vec![
            ("author", s("A B")),
            ("affiliation-title", s("Institutions")),
        ]);
        normalize_authors_meta(&mut overridden);
        assert_eq!(label(&overridden, "affiliations"), "Institutions");
    }

    #[test]
    fn funding_normalized_into_meta() {
        let mut meta = map(vec![
            ("author", s("Norah Jones")),
            ("funding", s("Funded by the Example Foundation.")),
        ]);
        normalize_authors_meta(&mut meta);
        let funding = meta.get("funding").and_then(|v| v.as_array()).unwrap();
        assert_eq!(
            funding[0].get("statement").and_then(|v| v.as_plain_text()),
            Some("Funded by the Example Foundation.".to_string())
        );
    }

    #[test]
    fn undefined_ref_reported_as_issue() {
        let mut meta = map(vec![(
            "author",
            arr(vec![map(vec![
                ("name", s("A B")),
                ("affiliations", arr(vec![map(vec![("ref", s("nowhere"))])])),
            ])]),
        )]);
        let issues = normalize_authors_meta(&mut meta);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("nowhere"));
    }

    #[test]
    fn ids_and_numbers_emitted() {
        let mut meta = rich_meta();
        normalize_authors_meta(&mut meta);
        let by_author = meta.get("by-author").and_then(|v| v.as_array()).unwrap();
        assert_eq!(by_author[0].get("id").and_then(|v| v.as_int()), Some(1));
        assert_eq!(by_author[0].get("number").and_then(|v| v.as_int()), Some(1));
        assert_eq!(
            by_author[1].get("letter").and_then(|v| v.as_plain_text()),
            Some("b".to_string())
        );
    }
}
