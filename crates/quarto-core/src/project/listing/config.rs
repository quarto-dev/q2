/*
 * project/listing/config.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Listing configuration types and parser.
//!
//! Authors put one or more listings under the top-level `listing:`
//! key on a host page's frontmatter. This module defines the typed
//! [`Listing`] struct + supporting enums, plus the
//! [`parse_listings`] entry point that converts a `ConfigValue`
//! into a `Vec<Listing>`.
//!
//! The shape is the L2 reference document
//! (`claude-notes/plans/2026-05-06-listings-L2-data-model.md`).
//! Diagnostic codes for invalid shapes are `Q-12-N`, registered in
//! `crates/quarto-error-catalog/error_catalog.json`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::config_value::ConfigValueKind;
use quarto_source_map::{By, SourceInfo};
use yaml_rust2::Yaml;

// ─────────────────────────────────────────────────────────────────
// Types
// ─────────────────────────────────────────────────────────────────

/// One listing declared on a host page. Authors put one or more of
/// these under the top-level `listing:` frontmatter key.
#[derive(Debug, Clone, PartialEq)]
pub struct Listing {
    pub id: String,
    pub kind: ListingType,
    pub contents: Vec<ListingContents>,
    pub fields: Vec<String>,
    /// `true` when the author supplied a non-empty `fields:` list.
    /// Author-explicit fields are used verbatim; defaulted fields are
    /// presence-filtered against the hydrated items at render time
    /// (Q1 parity, bd-listing-table-fields-peg1w3b3).
    pub fields_explicit: bool,
    pub field_display_names: BTreeMap<String, String>,
    pub field_types: BTreeMap<String, ColumnType>,
    /// Fields whose cell/entry value links to the item. `None` means
    /// the author didn't specify; [`apply_type_defaults`] then fills
    /// the Q1 default (`[title, filename]` for table listings, empty
    /// otherwise). An author-explicit `field-links: []` stays `Some`
    /// and disables linking entirely.
    pub field_links: Option<Vec<String>>,
    pub field_sort: Vec<String>,
    pub field_filter: Vec<String>,
    pub field_required: Vec<String>,
    pub page_size: u32,
    pub max_items: Option<u32>,
    pub filter_ui: bool,
    pub sort_ui: bool,
    pub image_placeholder: Option<String>,
    pub sort: Option<Vec<ListingSort>>,
    pub template: Option<PathBuf>,
    pub template_params: BTreeMap<String, ConfigValue>,
    pub grid_columns: Option<u32>,
    pub grid_item_border: Option<bool>,
    pub grid_item_align: Option<GridItemAlign>,
    pub table_striped: Option<bool>,
    pub table_hover: Option<bool>,
    pub image_align: Option<ImageAlign>,
    pub image_height: Option<String>,
    pub image_lazy_loading: Option<bool>,
    pub date_format: Option<String>,
    pub max_description_length: u32,
    pub include: Vec<ListingFilter>,
    pub exclude: Vec<ListingFilter>,
    pub categories: ListingCategoriesMode,
    /// Span on the `categories:` YAML key, captured by the parser
    /// for L5's `Q-12-12` "categories enabled but no item has any"
    /// diagnostic. `Generated{by: programmatic_config}` (a
    /// no-real-source sentinel — recognised by
    /// [`By::is_programmatic_sentinel`]) when the listing was
    /// constructed without parsing — e.g. by the `Default` impl
    /// or in tests.
    pub categories_source: SourceInfo,
    pub feed: Option<ListingFeedOptions>,
}

/// Type-specific defaults are applied during hydration; this
/// constructor returns a "neutral" Listing that the
/// [`hydrate_type_defaults`] pass adjusts based on `kind`.
impl Default for Listing {
    fn default() -> Self {
        Self {
            id: String::new(),
            kind: ListingType::Default,
            contents: Vec::new(),
            fields: Vec::new(),
            fields_explicit: false,
            field_display_names: BTreeMap::new(),
            field_types: BTreeMap::new(),
            field_links: None,
            field_sort: Vec::new(),
            field_filter: Vec::new(),
            field_required: Vec::new(),
            page_size: 25,
            max_items: None,
            filter_ui: false,
            sort_ui: false,
            image_placeholder: None,
            sort: None,
            template: None,
            template_params: BTreeMap::new(),
            grid_columns: None,
            grid_item_border: None,
            grid_item_align: None,
            table_striped: None,
            table_hover: None,
            image_align: None,
            image_height: None,
            image_lazy_loading: None,
            date_format: None,
            max_description_length: 175,
            include: Vec::new(),
            exclude: Vec::new(),
            categories: ListingCategoriesMode::Disabled,
            categories_source: SourceInfo::generated(By::programmatic_config()),
            feed: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListingType {
    #[default]
    Default,
    Grid,
    Table,
    Custom,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ListingContents {
    /// Glob pattern, kept as the user wrote it (a leading `!` marks
    /// a negation). `source` is the provenance of the YAML scalar —
    /// [`glob_resolve::resolve_content_globs`](super::glob_resolve::resolve_content_globs)
    /// uses it to recover the declaring file's directory, which is
    /// the base the pattern resolves against (GH #456,
    /// bd-v7ixzsp5).
    Glob { pattern: String, source: SourceInfo },
    /// Inline metadata record — the whole map, so the record's own
    /// span and each key's `key_source` survive to the generate
    /// transform (`record::parse_record`). The record *is* the item
    /// (plan §D2); no glob resolution is involved.
    Inline(ConfigValue),
}

impl ListingContents {
    /// Test/construction convenience: a glob entry with programmatic
    /// (no-file) provenance, which resolves against the host
    /// directory.
    pub fn glob_no_source(pattern: impl Into<String>) -> Self {
        ListingContents::Glob {
            pattern: pattern.into(),
            source: SourceInfo::generated(By::programmatic_config()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ListingSort {
    pub field: String,
    pub direction: SortDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortDirection {
    #[default]
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnType {
    Date,
    String,
    Number,
    Minutes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListingCategoriesMode {
    #[default]
    Disabled,
    Default,
    Unnumbered,
    Cloud,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ListingFilter {
    /// Multiple keys inside one record = AND match.
    /// Multiple [`ListingFilter`] records inside `include`/`exclude`
    /// = OR.
    pub fields: BTreeMap<String, ConfigValue>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ListingFeedOptions {
    pub items: Option<u32>,
    pub kind: FeedType,
    pub title: Option<String>,
    pub description: Option<String>,
    pub categories: Vec<String>,
    pub image: Option<String>,
    pub language: Option<String>,
    pub xml_stylesheet: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FeedType {
    #[default]
    Full,
    Partial,
    Metadata,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageAlign {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridItemAlign {
    Left,
    Right,
    Center,
}

// ─────────────────────────────────────────────────────────────────
// Parser entry point
// ─────────────────────────────────────────────────────────────────

/// Parse the value of a host page's `listing:` frontmatter key into
/// one or more [`Listing`] records. Returns an empty vec when the
/// key is absent (caller should check before calling); collects
/// any shape diagnostics into `diagnostics`.
///
/// The accepted shapes are documented in the L2 plan
/// (`claude-notes/plans/2026-05-06-listings-L2-data-model.md`):
///
/// - `listing: true` — synthesizes a default listing matching all
///   sibling `.qmd` files.
/// - `listing: default | grid | table | custom` — a string
///   shorthand for "one listing of the named type, defaults
///   otherwise".
/// - `listing: { ... }` — a single listing config map.
/// - `listing: [ {...}, ... ]` — multiple listings.
///
/// `listing: false` is rejected with `Q-12-6` (page-local; no
/// disable-via-parent semantics make sense). The caller should
/// short-circuit on absence rather than rely on `Vec::is_empty`.
pub fn parse_listings(
    value: &ConfigValue,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Vec<Listing> {
    // Boolean shorthand has to be checked before falling through to
    // `as_plain_text`, which would render `true`/`false` as the
    // strings "true"/"false" and confuse the type-name lookup.
    if let Some(b) = value.as_bool() {
        if b {
            return vec![default_listing_with_id("listing-1")];
        } else {
            push_diag(
                diagnostics,
                "Q-12-6",
                "`listing: false` is not allowed; remove the key entirely instead.",
                value,
            );
            return Vec::new();
        }
    }

    // Scalar string OR PandocInlines OR Path/Glob/Expr — Quarto
    // YAML often parses bare frontmatter strings as PandocInlines,
    // so we route through `as_plain_text` to handle every variant.
    if let Some(name) = value.as_plain_text() {
        let kind = match parse_type_name(&name) {
            Some(k) => k,
            None => {
                push_diag(
                    diagnostics,
                    "Q-12-1",
                    format!(
                        "Unknown listing type `{}`; expected one of: default, grid, table, custom.",
                        name
                    ),
                    value,
                );
                ListingType::Default
            }
        };
        // Build the bare struct with the chosen kind first, then
        // apply type defaults — `default_listing_with_id` would
        // bake in the *Default* type's defaults, which we then
        // can't undo.
        let mut l = Listing {
            id: "listing-1".to_string(),
            kind,
            ..Listing::default()
        };
        apply_type_defaults(&mut l);
        return vec![l];
    }

    match &value.value {
        ConfigValueKind::Map(_) => {
            let l = parse_one_listing(value, "listing-1", diagnostics);
            vec![l]
        }
        ConfigValueKind::Array(items) => {
            let mut explicit_ids: Vec<String> = Vec::new();
            let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

            // Pre-collect explicit ids so synthesized ids skip them.
            for item in items {
                if let ConfigValueKind::Map(_) = &item.value
                    && let Some(id_val) = item.get("id")
                    && let Some(id) = id_val.as_plain_text()
                {
                    explicit_ids.push(id);
                }
            }

            // Lazy synth-id generator: only advances the counter
            // when actually consumed, so an entry that ends up using
            // its explicit `id:` doesn't burn a synth slot. Skips
            // explicit ids and any already-emitted synth values.
            let mut next_synth = 1u32;
            let mut listings: Vec<Listing> = Vec::new();
            for item in items {
                // Skip-check: never propose a candidate that's
                // already explicit OR already used by an earlier
                // synth assignment.
                let needs_synth = match &item.value {
                    ConfigValueKind::Map(_) => item.get("id").is_none(),
                    _ => true,
                };
                let synth_id = if needs_synth {
                    loop {
                        let c = format!("listing-{}", next_synth);
                        next_synth += 1;
                        if !explicit_ids.iter().any(|e| e == &c) && !seen.contains(&c) {
                            break c;
                        }
                    }
                } else {
                    // The fallback id won't be used (parse_one_listing
                    // will read the explicit `id:`), but we still
                    // need to pass *something*. Use a placeholder
                    // that won't collide.
                    String::new()
                };
                let l = parse_one_listing(item, &synth_id, diagnostics);
                if seen.contains(&l.id) {
                    push_diag(
                        diagnostics,
                        "Q-12-4",
                        format!(
                            "Duplicate listing id `{}`; later occurrences are dropped.",
                            l.id
                        ),
                        // Blame the offending `id:` value, not the whole
                        // listing map — see `template_source` in
                        // `parse_one_listing` for why a map's span is not
                        // its own. A synthesized id has no `id:` entry to
                        // point at, so fall back to the entry itself.
                        map_entry_value(item, "id").unwrap_or(item),
                    );
                    continue;
                }
                seen.insert(l.id.clone());
                listings.push(l);
            }
            listings
        }
        // Other ConfigValueKind variants (PandocInlines/Blocks, etc.)
        // never appear here — `listing:` is parsed as a YAML config
        // value, not document body content. Treat as schema error.
        _ => {
            push_diag(
                diagnostics,
                "Q-12-1",
                "`listing:` value must be a boolean, type name, object, or array of objects.",
                value,
            );
            Vec::new()
        }
    }
}

fn default_listing_with_id(id: &str) -> Listing {
    let mut l = Listing {
        id: id.to_string(),
        ..Listing::default()
    };
    apply_type_defaults(&mut l);
    l
}

/// Parse one `Listing` from a map [`ConfigValue`]. The non-map
/// shorthand cases (boolean, string) are handled in
/// [`parse_listings`] before reaching here.
fn parse_one_listing(
    value: &ConfigValue,
    fallback_id: &str,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Listing {
    let map = match &value.value {
        ConfigValueKind::Map(entries) => entries,
        _ => {
            push_diag(
                diagnostics,
                "Q-12-1",
                "Listing entry must be an object.",
                value,
            );
            return default_listing_with_id(fallback_id);
        }
    };

    let mut l = Listing {
        id: fallback_id.to_string(),
        ..Default::default()
    };

    // Span to blame for the cross-field `template:`/`type:` check below.
    // Captured here rather than reusing the enclosing map's `source_info`,
    // which is not the map's span at all: `materialize_cursor` synthesizes
    // it from the map's *first entry's value*
    // (`quarto-config/src/materialize.rs:142-158`), so blaming `value`
    // underlines whichever key happens to come first — `sort: false` in
    // the report that opened bd-9yh3pzfu.
    //
    // Even once materialization preserves real container spans, blaming
    // the map would still be wrong here: quarto-yaml spans a mapping from
    // its first key to `MappingEnd`, so the caret would cover the whole
    // `listing:` block. A diagnostic about `template:` points at
    // `template:`.
    let mut template_source: Option<&ConfigValue> = None;

    for entry in map {
        match entry.key.as_str() {
            "id" => {
                if let Some(s) = entry.value.as_plain_text() {
                    l.id = s;
                }
            }
            "type" => {
                if let Some(name) = entry.value.as_plain_text() {
                    if let Some(k) = parse_type_name(&name) {
                        l.kind = k;
                    } else {
                        push_diag(
                            diagnostics,
                            "Q-12-1",
                            format!(
                                "Unknown listing type `{}`; expected one of: default, grid, table, custom.",
                                name
                            ),
                            &entry.value,
                        );
                    }
                }
            }
            "contents" => {
                l.contents = parse_contents(&entry.value, diagnostics);
            }
            "fields" => {
                l.fields = parse_string_list(&entry.value);
                // Explicit-but-empty `fields: []` falls through to
                // the type defaults, same as omitting the key.
                l.fields_explicit = !l.fields.is_empty();
            }
            "field-display-names" => {
                l.field_display_names = parse_string_string_map(&entry.value, diagnostics);
            }
            "field-types" => {
                l.field_types = parse_field_types(&entry.value, diagnostics);
            }
            "field-links" => l.field_links = Some(parse_string_list(&entry.value)),
            "field-sort" => l.field_sort = parse_string_list(&entry.value),
            "field-filter" => l.field_filter = parse_string_list(&entry.value),
            "field-required" => l.field_required = parse_string_list(&entry.value),
            "page-size" => {
                if let Some(n) = parse_u32_scalar(&entry.value) {
                    l.page_size = n;
                }
            }
            "max-items" => {
                l.max_items = parse_u32_scalar(&entry.value);
            }
            "filter-ui" => {
                l.filter_ui = entry.value.as_bool().unwrap_or(l.filter_ui);
            }
            "sort-ui" => {
                l.sort_ui = entry.value.as_bool().unwrap_or(l.sort_ui);
            }
            "image-placeholder" => {
                l.image_placeholder = entry.value.as_plain_text();
            }
            "sort" => {
                l.sort = parse_sort(&entry.value, diagnostics);
            }
            "template" => {
                template_source = Some(&entry.value);
                if let Some(path) = entry.value.as_plain_text() {
                    if path.ends_with(".ejs.md") {
                        push_diag(
                            diagnostics,
                            "Q-12-9",
                            format!(
                                "`{}` uses the deprecated `.ejs.md` extension; see the Q1 → Q2 listing template migration guide.",
                                path
                            ),
                            &entry.value,
                        );
                    }
                    l.template = Some(PathBuf::from(path));
                }
            }
            "template-params" => {
                if let ConfigValueKind::Map(entries) = &entry.value.value {
                    for e in entries {
                        l.template_params.insert(e.key.clone(), e.value.clone());
                    }
                }
            }
            "grid-columns" => {
                l.grid_columns = parse_u32_scalar(&entry.value);
            }
            "grid-item-border" => {
                l.grid_item_border = entry.value.as_bool();
            }
            "grid-item-align" => {
                l.grid_item_align = entry
                    .value
                    .as_plain_text()
                    .and_then(|s| parse_grid_item_align(&s));
            }
            "table-striped" => l.table_striped = entry.value.as_bool(),
            "table-hover" => l.table_hover = entry.value.as_bool(),
            "image-align" => {
                l.image_align = entry
                    .value
                    .as_plain_text()
                    .and_then(|s| parse_image_align(&s));
            }
            "image-height" => l.image_height = entry.value.as_plain_text(),
            "image-lazy-loading" => l.image_lazy_loading = entry.value.as_bool(),
            "date-format" => l.date_format = entry.value.as_plain_text(),
            "max-description-length" => {
                if let Some(n) = parse_u32_scalar(&entry.value) {
                    l.max_description_length = n;
                }
            }
            "include" => l.include = parse_filter_list(&entry.value),
            "exclude" => l.exclude = parse_filter_list(&entry.value),
            "categories" => {
                l.categories = parse_categories_mode(&entry.value);
                // Capture the YAML span on the `categories:` key for L5's
                // Q-12-12 diagnostic.
                l.categories_source = entry.key_source.clone();
            }
            "feed" => {
                l.feed = parse_feed(&entry.value);
            }
            _ => {
                // Unknown keys are tolerated for forward-compat; a
                // future strict-validation pass can flag them.
            }
        }
    }

    // Cross-field validation: `template:` set without `type: custom`
    // should warn (per Q-12-7) and leave the type alone.
    //
    // Blame the `template:` value, not the enclosing map — see the
    // `template_source` declaration above. `template_source` is always
    // `Some` here, since `l.template` is only set from that same entry;
    // the fallback keeps the diagnostic alive rather than silently
    // dropping it if that ever stops holding.
    if l.template.is_some() && l.kind != ListingType::Custom {
        push_diag(
            diagnostics,
            "Q-12-7",
            "`template:` was set but `type:` is not `custom`; falling back to the built-in template for the declared type.",
            template_source.unwrap_or(value),
        );
    }

    apply_type_defaults(&mut l);
    l
}

fn parse_type_name(name: &str) -> Option<ListingType> {
    match name {
        "default" => Some(ListingType::Default),
        "grid" => Some(ListingType::Grid),
        "table" => Some(ListingType::Table),
        "custom" => Some(ListingType::Custom),
        _ => None,
    }
}

fn parse_image_align(name: &str) -> Option<ImageAlign> {
    match name {
        "left" => Some(ImageAlign::Left),
        "right" => Some(ImageAlign::Right),
        _ => None,
    }
}

fn parse_grid_item_align(name: &str) -> Option<GridItemAlign> {
    match name {
        "left" => Some(GridItemAlign::Left),
        "right" => Some(GridItemAlign::Right),
        "center" => Some(GridItemAlign::Center),
        _ => None,
    }
}

fn parse_contents(
    value: &ConfigValue,
    _diagnostics: &mut Vec<DiagnosticMessage>,
) -> Vec<ListingContents> {
    // Since bd-v7ixzsp5, front-matter `contents:` strings arrive as
    // `ConfigValueKind::Glob` — the key-path annotation table in
    // pampa (`meta_annotations.rs`) types them at parse time, so
    // they never take the markdown-parsing path (which used to warn
    // Q-1-20 and could silently corrupt patterns whose asterisks
    // parsed as emphasis). The `as_plain_text` route below is kept
    // as a defensive fallback for string-shaped values from other
    // sources (programmatic construction, runtime metadata, legacy
    // `PandocInlines` values — the original bd-nwyp shape).
    if let Some(s) = value.as_plain_text() {
        return vec![ListingContents::Glob {
            pattern: s,
            source: value.source_info.clone(),
        }];
    }
    match &value.value {
        ConfigValueKind::Scalar {
            yaml: Yaml::String(s),
            ..
        } => {
            vec![ListingContents::Glob {
                pattern: s.clone(),
                source: value.source_info.clone(),
            }]
        }
        ConfigValueKind::Glob(pattern) => {
            vec![ListingContents::Glob {
                pattern: pattern.clone(),
                source: value.source_info.clone(),
            }]
        }
        ConfigValueKind::Array(items) => items
            .iter()
            .filter_map(|item| {
                if let Some(s) = item.as_plain_text() {
                    return Some(ListingContents::Glob {
                        pattern: s,
                        source: item.source_info.clone(),
                    });
                }
                match &item.value {
                    ConfigValueKind::Scalar {
                        yaml: Yaml::String(s),
                        ..
                    } => Some(ListingContents::Glob {
                        pattern: s.clone(),
                        source: item.source_info.clone(),
                    }),
                    ConfigValueKind::Glob(pattern) => Some(ListingContents::Glob {
                        pattern: pattern.clone(),
                        source: item.source_info.clone(),
                    }),
                    ConfigValueKind::Map(_) => Some(ListingContents::Inline(item.clone())),
                    _ => None,
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Numeric config scalar as `u32`: any scalar form a user can write
/// (unquoted integer, quoted string, bare front-matter scalar) via
/// [`ConfigValue::as_int_lenient`], clamped to `u32`. Out-of-range and
/// non-numeric values yield `None` (caller keeps its default).
/// History: bd-listing-ellipsis-no-matching-l963osy1 / bd-yjsz6hdu.
fn parse_u32_scalar(value: &ConfigValue) -> Option<u32> {
    value.as_int_lenient().and_then(|i| u32::try_from(i).ok())
}

fn parse_string_list(value: &ConfigValue) -> Vec<String> {
    match &value.value {
        ConfigValueKind::Array(items) => items.iter().filter_map(|v| v.as_plain_text()).collect(),
        _ => value.as_plain_text().map(|s| vec![s]).unwrap_or_default(),
    }
}

fn parse_string_string_map(
    value: &ConfigValue,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if let ConfigValueKind::Map(entries) = &value.value {
        for entry in entries {
            if let Some(s) = entry.value.as_plain_text() {
                out.insert(entry.key.clone(), s);
            } else {
                push_diag(
                    diagnostics,
                    "Q-12-5",
                    format!(
                        "`field-display-names.{}` must be a string; entry dropped.",
                        entry.key
                    ),
                    &entry.value,
                );
            }
        }
    }
    out
}

fn parse_field_types(
    value: &ConfigValue,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> BTreeMap<String, ColumnType> {
    let mut out = BTreeMap::new();
    if let ConfigValueKind::Map(entries) = &value.value {
        for entry in entries {
            let name = entry.value.as_plain_text();
            let ct = match name.as_deref() {
                Some("date") => Some(ColumnType::Date),
                Some("string") => Some(ColumnType::String),
                Some("number") => Some(ColumnType::Number),
                Some("minutes") => Some(ColumnType::Minutes),
                _ => None,
            };
            if let Some(ct) = ct {
                out.insert(entry.key.clone(), ct);
            } else {
                push_diag(
                    diagnostics,
                    "Q-12-1",
                    format!(
                        "`field-types.{}` must be one of: date, string, number, minutes.",
                        entry.key
                    ),
                    &entry.value,
                );
            }
        }
    }
    out
}

/// Parse the `sort:` value. `None` means "apply the default sort" —
/// `sort: true` is Q1's explicit spelling of the default, so it
/// parses the same as an absent key. `Some(vec![])` means sorting is
/// explicitly disabled (`sort: false`); `Some(keys)` is an author
/// sort spec.
fn parse_sort(
    value: &ConfigValue,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Option<Vec<ListingSort>> {
    if let ConfigValueKind::Scalar {
        yaml: Yaml::Boolean(b),
        ..
    } = &value.value
    {
        return if *b { None } else { Some(Vec::new()) };
    }
    // String-shaped values (including the routine PandocInlines
    // wrapping of front-matter strings) flatten via `as_plain_text`,
    // mirroring `parse_contents` — see bd-2qjnd / bd-nwyp.
    if let Some(s) = value.as_plain_text() {
        return Some(vec![parse_one_sort_key(&s)]);
    }
    match &value.value {
        ConfigValueKind::Array(items) => Some(
            items
                .iter()
                .filter_map(|v| v.as_plain_text())
                .map(|s| parse_one_sort_key(&s))
                .collect(),
        ),
        _ => {
            push_diag(
                diagnostics,
                "Q-12-3",
                "`sort:` must be a string, array of strings, or a boolean.",
                value,
            );
            Some(Vec::new())
        }
    }
}

fn parse_one_sort_key(s: &str) -> ListingSort {
    let trimmed = s.trim();
    let mut parts = trimmed.split_whitespace();
    let field = parts.next().unwrap_or("").to_string();
    let direction = match parts.next() {
        Some("desc") => SortDirection::Desc,
        Some("asc") | None => SortDirection::Asc,
        _ => SortDirection::Asc,
    };
    ListingSort { field, direction }
}

fn parse_filter_list(value: &ConfigValue) -> Vec<ListingFilter> {
    let mut out = Vec::new();
    let entries: Vec<&ConfigValue> = match &value.value {
        ConfigValueKind::Array(items) => items.iter().collect(),
        ConfigValueKind::Map(_) => vec![value],
        _ => return Vec::new(),
    };
    for entry in entries {
        if let ConfigValueKind::Map(map_entries) = &entry.value {
            let mut fields = BTreeMap::new();
            for me in map_entries {
                fields.insert(me.key.clone(), me.value.clone());
            }
            out.push(ListingFilter { fields });
        }
    }
    out
}

fn parse_categories_mode(value: &ConfigValue) -> ListingCategoriesMode {
    if let Some(b) = value.as_bool() {
        return if b {
            ListingCategoriesMode::Default
        } else {
            ListingCategoriesMode::Disabled
        };
    }
    match value.as_plain_text().as_deref() {
        Some("true" | "default") => ListingCategoriesMode::Default,
        Some("unnumbered") => ListingCategoriesMode::Unnumbered,
        Some("cloud") => ListingCategoriesMode::Cloud,
        _ => ListingCategoriesMode::Disabled,
    }
}

fn parse_feed(value: &ConfigValue) -> Option<ListingFeedOptions> {
    if let Some(true) = value.as_bool() {
        return Some(ListingFeedOptions {
            items: None,
            kind: FeedType::Full,
            title: None,
            description: None,
            categories: Vec::new(),
            image: None,
            language: None,
            xml_stylesheet: None,
        });
    }
    if let ConfigValueKind::Map(entries) = &value.value {
        let mut feed = ListingFeedOptions {
            items: None,
            kind: FeedType::Full,
            title: None,
            description: None,
            categories: Vec::new(),
            image: None,
            language: None,
            xml_stylesheet: None,
        };
        for e in entries {
            match e.key.as_str() {
                "items" => {
                    feed.items = parse_u32_scalar(&e.value);
                }
                "type" => {
                    feed.kind = match e.value.as_plain_text().as_deref() {
                        Some("partial") => FeedType::Partial,
                        Some("metadata") => FeedType::Metadata,
                        _ => FeedType::Full,
                    };
                }
                "title" => feed.title = e.value.as_plain_text(),
                "description" => feed.description = e.value.as_plain_text(),
                "categories" => feed.categories = parse_string_list(&e.value),
                "image" => feed.image = e.value.as_plain_text(),
                "language" => feed.language = e.value.as_plain_text(),
                "xml-stylesheet" => {
                    feed.xml_stylesheet = e.value.as_plain_text().map(PathBuf::from);
                }
                _ => {}
            }
        }
        return Some(feed);
    }
    None
}

/// Apply type-specific default values per L2 §"Type-specific
/// defaults". Author-supplied values are preserved (we only fill
/// `None` / falsy slots that the parser left at the [`Default`]
/// neutral baseline).
pub fn apply_type_defaults(l: &mut Listing) {
    // Default `fields` set if author didn't override.
    if l.fields.is_empty() {
        l.fields = match l.kind {
            ListingType::Default => vec![
                "date",
                "title",
                "author",
                "subtitle",
                "description",
                "image",
                "image-alt",
                "categories",
                "filename",
                "file-modified",
                "reading-time",
            ],
            ListingType::Grid => vec![
                "title",
                "subtitle",
                "author",
                "date",
                "image",
                "image-alt",
                "description",
                "categories",
                "filename",
                "file-modified",
                "reading-time",
            ],
            ListingType::Table => vec!["date", "title", "author"],
            ListingType::Custom => vec![],
        }
        .into_iter()
        .map(String::from)
        .collect();
    }
    // Q1's `kDefaultFieldLinks`: table listings link title +
    // filename cells; other types link nothing by default. An
    // author-explicit `field-links:` (even `[]`) is already `Some`
    // and wins.
    if l.field_links.is_none() {
        l.field_links = Some(match l.kind {
            ListingType::Table => vec!["title".to_string(), "filename".to_string()],
            _ => Vec::new(),
        });
    }
    // Type-specific knobs (only fill None).
    match l.kind {
        ListingType::Default => {
            l.image_align.get_or_insert(ImageAlign::Right);
            l.image_height.get_or_insert("120px".to_string());
            l.image_lazy_loading.get_or_insert(true);
        }
        ListingType::Grid => {
            l.grid_columns.get_or_insert(3);
            l.grid_item_border.get_or_insert(true);
            l.grid_item_align.get_or_insert(GridItemAlign::Left);
            l.image_lazy_loading.get_or_insert(true);
        }
        ListingType::Table => {
            l.sort_ui = true;
            l.filter_ui = true;
            l.page_size = 30;
            l.image_lazy_loading.get_or_insert(true);
        }
        ListingType::Custom => {}
    }
    // Default contents glob if absent: every `*.qmd` next to the
    // host page (Q1 default; the host file itself gets excluded
    // during item discovery, not here).
    if l.contents.is_empty() {
        l.contents.push(ListingContents::glob_no_source("*.qmd"));
    }
}

/// Flatten every `contents:` glob entry across all listings declared
/// on a host page's `meta.listing:` value, ignoring all other listing
/// config. Inline-record entries with document paths become dependency
/// edges (plan §D4). See [`record_path_as_glob`].
///
/// Consumers resolve the returned entries with
/// [`super::glob_resolve::resolve_content_globs`] — see that module
/// for the base-directory semantics. Routes through
/// [`parse_listings`] (diagnostics discarded) so shape-handling
/// stays in lockstep with the L3 generate transform (`bd-bqf2`).
pub fn flatten_content_globs(meta: &ConfigValue) -> Vec<ListingContents> {
    let Some(listing_value) = meta.get("listing") else {
        return Vec::new();
    };
    let mut throwaway_diagnostics: Vec<DiagnosticMessage> = Vec::new();
    let listings = parse_listings(listing_value, &mut throwaway_diagnostics);
    listings
        .into_iter()
        .flat_map(|l| l.contents)
        .filter_map(|c| match c {
            glob @ ListingContents::Glob { .. } => Some(glob),
            ListingContents::Inline(value) => record_path_as_glob(&value),
        })
        .collect()
}

/// A record's `path:` to a project document is a dependency edge
/// (plan §D4). Emitted as a literal pattern with the value's own
/// provenance so the base-directory rule is the generate
/// transform's. Glob-shaped paths are skipped rather than compiled.
fn record_path_as_glob(value: &ConfigValue) -> Option<ListingContents> {
    let path_value = value.get("path")?;
    let raw = path_value.as_plain_text()?;
    // `is_remote_src`, not `is_external_src`: a leading `/` is the
    // project root and *is* a dependency (plan §D4). The resolver
    // re-anchors it, exactly as it does for a `/`-anchored glob.
    if super::helpers::is_remote_src(&raw)
        || crate::glob::has_metacharacters(&raw)
        || !is_markdown_document_path(&raw)
    {
        return None;
    }
    Some(ListingContents::Glob {
        pattern: raw,
        source: path_value.source_info.clone(),
    })
}

/// Q1's `markdownExtensions` for record `path:` values, plus
/// notebooks (a q2 project input).
pub(crate) fn is_markdown_document_path(p: &str) -> bool {
    let ext = std::path::Path::new(p.split(['?', '#']).next().unwrap_or(""))
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    matches!(ext.as_deref(), Some("qmd" | "md" | "rmd" | "ipynb"))
}

/// The value of `key` in `value`, if `value` is a map containing it.
///
/// Used to blame a specific entry rather than its enclosing map. A map's
/// `source_info` is not the map's span — see `template_source` in
/// [`parse_one_listing`].
fn map_entry_value<'a>(value: &'a ConfigValue, key: &str) -> Option<&'a ConfigValue> {
    match &value.value {
        ConfigValueKind::Map(entries) => entries.iter().find(|e| e.key == key).map(|e| &e.value),
        _ => None,
    }
}

fn push_diag(
    diagnostics: &mut Vec<DiagnosticMessage>,
    code: &str,
    message: impl Into<String>,
    value: &ConfigValue,
) {
    diagnostics.push(
        DiagnosticMessageBuilder::warning(message)
            .with_code(code)
            .with_location(value.source_info.clone())
            .build(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::ConfigMapEntry;
    use quarto_source_map::SourceInfo;

    fn s(value: &str) -> ConfigValue {
        ConfigValue::new_string(value, SourceInfo::for_test())
    }

    fn b(value: bool) -> ConfigValue {
        ConfigValue::new_bool(value, SourceInfo::for_test())
    }

    fn arr(items: Vec<ConfigValue>) -> ConfigValue {
        ConfigValue::new_array(items, SourceInfo::for_test())
    }

    fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        let map_entries: Vec<ConfigMapEntry> = entries
            .into_iter()
            .map(|(k, v)| ConfigMapEntry {
                key: k.to_string(),
                key_source: SourceInfo::for_test(),
                value: v,
            })
            .collect();
        ConfigValue::new_map(map_entries, SourceInfo::for_test())
    }

    fn parse(value: ConfigValue) -> (Vec<Listing>, Vec<DiagnosticMessage>) {
        let mut diags = Vec::new();
        let listings = parse_listings(&value, &mut diags);
        (listings, diags)
    }

    /// The glob pattern strings of a contents list, ignoring the
    /// per-entry provenance (which tests can't reproduce exactly).
    fn glob_patterns(contents: &[ListingContents]) -> Vec<String> {
        contents
            .iter()
            .filter_map(|c| match c {
                ListingContents::Glob { pattern, .. } => Some(pattern.clone()),
                ListingContents::Inline(_) => None,
            })
            .collect()
    }

    // ── real-source fixtures (bd-9yh3pzfu) ──────────────────────────
    //
    // The helpers above stamp every value with `SourceInfo::for_test()`,
    // so a diagnostic pointing at the wrong key is indistinguishable
    // from one pointing at the right key — the span bug is invisible by
    // construction. Tests that assert on *where* a diagnostic points
    // must parse real text and travel the real path.
    //
    // That path matters: production reads `ast.meta.get("listing")`
    // (see `transforms/listing_generate.rs:72`), and `ast.meta` is the
    // *materialized* merge output — `MetadataMergeStage` replaces it
    // wholesale at `stage/stages/metadata_merge.rs:266-288`. Skipping
    // materialization would skip the defect.

    const FIXTURE_FILE: &str = "index.qmd";

    /// Parse YAML front-matter text the way the render pipeline does:
    /// YAML → `ConfigValue` → merge → **materialize**, then hand back
    /// the `listing:` value plus a `SourceContext` that can resolve the
    /// spans it carries.
    fn parse_from_yaml(
        yaml: &str,
    ) -> (
        Vec<Listing>,
        Vec<DiagnosticMessage>,
        quarto_source_map::SourceContext,
    ) {
        use pampa::pandoc::yaml_to_config_value;
        use pampa::utils::diagnostic_collector::DiagnosticCollector;
        use quarto_config::{InterpretationContext, MergedConfig};

        let parsed = quarto_yaml::parse_file(yaml, FIXTURE_FILE).expect("valid yaml");
        let mut collector = DiagnosticCollector::new();
        let doc_config = yaml_to_config_value(
            parsed,
            InterpretationContext::DocumentMetadata,
            &mut collector,
        );

        let merged = MergedConfig::new(vec![&doc_config])
            .materialize()
            .expect("materialize");
        let listing_value = merged.get("listing").expect("`listing:` key present");

        let mut diags = Vec::new();
        let listings = parse_listings(listing_value, &mut diags);
        (
            listings,
            diags,
            quarto_config::span_assert::context_for(FIXTURE_FILE, yaml),
        )
    }

    fn diag_with_code<'a>(diags: &'a [DiagnosticMessage], code: &str) -> &'a DiagnosticMessage {
        diags
            .iter()
            .find(|d| d.code.as_deref() == Some(code))
            .unwrap_or_else(|| panic!("expected a {code} diagnostic; got: {diags:?}"))
    }

    // bd-9yh3pzfu: Q-12-7 talks about `template:` but blamed whichever
    // key happened to come first in the map — here `sort:`. The key
    // order below is load-bearing: putting `template:` first would mask
    // the bug.
    #[test]
    fn q_12_7_underlines_the_template_key_not_a_sibling() {
        let yaml = "\
title: Vanity URLs
listing:
    sort: false
    template: ../template.ejs
    contents:
    - ./a.qmd
";
        let (_listings, diags, ctx) = parse_from_yaml(yaml);
        let q127 = diag_with_code(&diags, "Q-12-7");

        quarto_config::span_assert::assert_diagnostic_underlines(q127, &ctx, "../template.ejs");
    }

    // bd-9yh3pzfu: Q-12-4 named the duplicate id in its message but
    // blamed the whole listing map, so the caret landed on whichever
    // key came first. `contents:` is first here to keep that visible.
    #[test]
    fn q_12_4_underlines_the_duplicate_id_not_a_sibling() {
        let yaml = "\
listing:
    - contents: ./a.qmd
      id: dupe
    - contents: ./b.qmd
      id: dupe
";
        let (_listings, diags, ctx) = parse_from_yaml(yaml);
        let q124 = diag_with_code(&diags, "Q-12-4");

        quarto_config::span_assert::assert_diagnostic_underlines(q124, &ctx, "dupe");
    }

    // Discovered during bd-listing-ellipsis-no-matching-l963osy1: an
    // unquoted YAML integer reaches `parse_listings` as
    // `Yaml::Integer`, for which `as_plain_text()` returns `None` —
    // so `max-description-length: 40` silently kept the 175 default.
    // Must travel the real YAML path: the `s()` builder constructs
    // string scalars and can't reproduce the integer shape.
    #[test]
    fn max_description_length_accepts_unquoted_integer() {
        let yaml = "\
listing:
    contents: ./a.qmd
    max-description-length: 40
";
        let (listings, _diags, _ctx) = parse_from_yaml(yaml);
        assert_eq!(
            listings
                .first()
                .expect("one listing")
                .max_description_length,
            40
        );
    }

    // bd-yjsz6hdu: the sibling numeric keys share the trap fixed for
    // max-description-length above — unquoted YAML integers arrive as
    // `Yaml::Integer`, which `as_plain_text()` doesn't cover.

    #[test]
    fn page_size_accepts_unquoted_integer() {
        let yaml = "\
listing:
    contents: ./a.qmd
    page-size: 10
";
        let (listings, _diags, _ctx) = parse_from_yaml(yaml);
        assert_eq!(listings.first().expect("one listing").page_size, 10);
    }

    // Guard: the quoted-string form worked before bd-yjsz6hdu and must
    // keep working after the accessor migration.
    #[test]
    fn page_size_accepts_quoted_integer() {
        let yaml = "\
listing:
    contents: ./a.qmd
    page-size: \"10\"
";
        let (listings, _diags, _ctx) = parse_from_yaml(yaml);
        assert_eq!(listings.first().expect("one listing").page_size, 10);
    }

    #[test]
    fn max_items_accepts_unquoted_integer() {
        let yaml = "\
listing:
    contents: ./a.qmd
    max-items: 5
";
        let (listings, _diags, _ctx) = parse_from_yaml(yaml);
        assert_eq!(listings.first().expect("one listing").max_items, Some(5));
    }

    #[test]
    fn grid_columns_accepts_unquoted_integer() {
        let yaml = "\
listing:
    contents: ./a.qmd
    type: grid
    grid-columns: 4
";
        let (listings, _diags, _ctx) = parse_from_yaml(yaml);
        assert_eq!(listings.first().expect("one listing").grid_columns, Some(4));
    }

    #[test]
    fn feed_items_accepts_unquoted_integer() {
        let yaml = "\
listing:
    contents: ./a.qmd
    feed:
        items: 5
";
        let (listings, _diags, _ctx) = parse_from_yaml(yaml);
        let feed = listings
            .first()
            .expect("one listing")
            .feed
            .as_ref()
            .expect("feed configured");
        assert_eq!(feed.items, Some(5));
    }

    // bd-2mxo: L5 captures the `categories:` *key* span here purely to
    // anchor Q-12-12 ("categories enabled but no item has any"; see
    // `transforms/categories_sidebar.rs:213`). Materialization used to
    // replace every `key_source` with a programmatic-config sentinel,
    // which left that anchor inert — the feature existed but could
    // never point anywhere. This pins it to the real key.
    #[test]
    fn categories_source_anchors_the_real_categories_key() {
        let yaml = "\
listing:
    contents: ./a.qmd
    categories: true
";
        let (listings, _diags, ctx) = parse_from_yaml(yaml);
        let listing = listings.first().expect("one listing");

        let span = quarto_config::span_assert::resolve_span(&listing.categories_source, &ctx)
            .expect("categories_source should be a real key span, not a sentinel");
        assert_eq!(span.text, "categories");
    }

    // The two sites the Phase A audit deliberately left alone: each
    // already blamed the semantically correct value, and read wrong only
    // because that value was a container carrying a synthesized span.
    // Fixing materialization (bd-2mxo) is what makes them right, so
    // these assert the fix reaches beyond the call sites Phase A
    // touched.

    // 7. inline-record contents are captured whole, with no diagnostic
    #[test]
    fn contents_inline_record_is_captured_without_diagnostic() {
        let (listings, diags) = parse(map(vec![(
            "contents",
            arr(vec![map(vec![
                ("title", s("foo")),
                ("path", s("bar.html")),
            ])]),
        )]));
        assert_eq!(listings.len(), 1);
        assert_eq!(listings[0].contents.len(), 1);
        let ListingContents::Inline(record) = &listings[0].contents[0] else {
            panic!("expected Inline, got {:?}", listings[0].contents[0]);
        };
        assert_eq!(
            record
                .get("title")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("foo")
        );
        assert_eq!(
            record
                .get("path")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("bar.html")
        );
        assert!(diags.is_empty(), "Q-12-2 is retired; got {:?}", diags);
    }

    #[test]
    fn q_12_3_underlines_the_whole_sort_value() {
        let yaml = "\
listing:
    contents: ./a.qmd
    sort:
        field: title
";
        let (_listings, diags, ctx) = parse_from_yaml(yaml);
        let q123 = diag_with_code(&diags, "Q-12-3");

        let span = quarto_config::span_assert::resolve_diagnostic_span(q123, &ctx)
            .expect("Q-12-3 should resolve to a real span");
        assert!(
            span.text.contains("field: title"),
            "expected the offending `sort:` value, got {:?}",
            span.text
        );
    }

    // 1. config_parses_minimal — `listing: default` parses to a
    //    Listing with id synthesized, type Default, defaults applied.
    #[test]
    fn config_parses_minimal_default_string() {
        let (listings, diags) = parse(s("default"));
        assert_eq!(listings.len(), 1);
        assert_eq!(listings[0].id, "listing-1");
        assert_eq!(listings[0].kind, ListingType::Default);
        // Default contents glob filled in.
        assert_eq!(glob_patterns(&listings[0].contents), vec!["*.qmd"]);
        // Default fields set.
        assert!(listings[0].fields.contains(&"title".to_string()));
        assert!(listings[0].fields.contains(&"date".to_string()));
        assert!(diags.is_empty());
    }

    // 2. config_parses_explicit_id_and_type
    #[test]
    fn config_parses_explicit_id_and_type() {
        let (listings, diags) = parse(map(vec![
            ("id", s("foo")),
            ("type", s("grid")),
            ("contents", arr(vec![s("*.qmd")])),
        ]));
        assert_eq!(listings.len(), 1);
        assert_eq!(listings[0].id, "foo");
        assert_eq!(listings[0].kind, ListingType::Grid);
        assert_eq!(listings[0].grid_columns, Some(3));
        assert!(diags.is_empty());
    }

    // 3. config_parses_multi_listing
    #[test]
    fn config_parses_multi_listing() {
        let (listings, diags) = parse(arr(vec![
            map(vec![("type", s("default"))]),
            map(vec![("type", s("grid"))]),
        ]));
        assert_eq!(listings.len(), 2);
        assert_eq!(listings[0].id, "listing-1");
        assert_eq!(listings[1].id, "listing-2");
        assert_eq!(listings[0].kind, ListingType::Default);
        assert_eq!(listings[1].kind, ListingType::Grid);
        assert!(diags.is_empty());
    }

    // 3b. multi-listing synthesis skips explicit ids
    #[test]
    fn config_synth_ids_skip_explicit() {
        let (listings, diags) = parse(arr(vec![
            map(vec![("type", s("default"))]),
            map(vec![("id", s("listing-2")), ("type", s("grid"))]),
            map(vec![("type", s("table"))]),
        ]));
        assert_eq!(listings.len(), 3);
        assert_eq!(listings[0].id, "listing-1");
        assert_eq!(listings[1].id, "listing-2");
        // Synthesizer sees `listing-2` as taken, so the next synth
        // value is `listing-3`.
        assert_eq!(listings[2].id, "listing-3");
        assert!(diags.is_empty());
    }

    // 4. listing: true shorthand → default listing
    #[test]
    fn config_parses_listing_true_shorthand() {
        let (listings, diags) = parse(b(true));
        assert_eq!(listings.len(), 1);
        assert_eq!(listings[0].id, "listing-1");
        assert_eq!(listings[0].kind, ListingType::Default);
        assert!(diags.is_empty());
    }

    // 5. listing: false → diagnostic + empty
    #[test]
    fn config_rejects_listing_false() {
        let (listings, diags) = parse(b(false));
        assert!(listings.is_empty());
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code.as_deref(), Some("Q-12-6"));
    }

    // 6. contents glob string
    #[test]
    fn contents_glob_string_parses() {
        let (listings, _) = parse(map(vec![
            ("type", s("default")),
            ("contents", arr(vec![s("posts/**/*.qmd")])),
        ]));
        assert_eq!(glob_patterns(&listings[0].contents), vec!["posts/**/*.qmd"]);
    }

    // 6b. Quarto YAML often parses globs like `posts/*.qmd` as
    // `PandocInlines` (a Span with class `yaml-markdown-syntax-error`
    // because `*` triggers the markdown sublexer). `parse_contents`
    // must route through `as_plain_text` so the glob string is still
    // captured. Discovered when L5's snapshot tests #33 surfaced
    // empty listings — see bd-nwyp.
    #[test]
    fn contents_pandoc_inlines_string_parses_as_glob() {
        use quarto_pandoc_types::inline::{Inline, Str};
        // Build a PandocInlines value carrying the literal string
        // `posts/*.qmd`. `as_plain_text` flattens this back to the
        // raw string regardless of any wrapping spans/classes.
        let inlines: quarto_pandoc_types::inline::Inlines = vec![Inline::Str(Str {
            text: "posts/*.qmd".to_string(),
            source_info: SourceInfo::for_test(),
        })];
        let contents_val = ConfigValue::new_inlines(inlines, SourceInfo::for_test());
        let (listings, diags) = parse(map(vec![
            ("type", s("default")),
            ("contents", contents_val),
        ]));
        assert_eq!(
            glob_patterns(&listings[0].contents),
            vec!["posts/*.qmd"],
            "expected `posts/*.qmd` glob; diags: {:?}",
            diags
        );
    }

    // 6c. Same fix in the array-of-strings shape: each item may
    // also arrive as `PandocInlines`. Locks behavior for
    // `contents: [posts/*.qmd, notes/*.qmd]`.
    #[test]
    fn contents_array_with_pandoc_inlines_items_parses() {
        use quarto_pandoc_types::inline::{Inline, Str};
        let make_inlines = |t: &str| -> ConfigValue {
            let inlines: quarto_pandoc_types::inline::Inlines = vec![Inline::Str(Str {
                text: t.to_string(),
                source_info: SourceInfo::for_test(),
            })];
            ConfigValue::new_inlines(inlines, SourceInfo::for_test())
        };
        let arr_val = ConfigValue::new_array(
            vec![make_inlines("posts/*.qmd"), make_inlines("notes/*.qmd")],
            SourceInfo::for_test(),
        );
        let (listings, _) = parse(map(vec![("type", s("default")), ("contents", arr_val)]));
        assert_eq!(
            glob_patterns(&listings[0].contents),
            vec!["posts/*.qmd", "notes/*.qmd"]
        );
    }

    // 8. sort: ["date"] → default Asc
    #[test]
    fn sort_parses_field_only() {
        let (listings, _) = parse(map(vec![("sort", arr(vec![s("date")]))]));
        let sort = listings[0].sort.as_ref().unwrap();
        assert_eq!(sort.len(), 1);
        assert_eq!(sort[0].field, "date");
        assert_eq!(sort[0].direction, SortDirection::Asc);
    }

    // 8b. sort: false → Some([]) — sorting explicitly disabled,
    // declared contents order preserved downstream.
    #[test]
    fn sort_false_parses_to_empty_spec() {
        let (listings, diags) = parse(map(vec![("sort", b(false))]));
        assert_eq!(listings[0].sort.as_deref(), Some(&[][..]));
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }

    // 8c. sort: true → None — Q1's explicit spelling of "apply the
    // default sort"; same as an absent key, and NOT a field named
    // "true".
    #[test]
    fn sort_true_parses_like_absent() {
        let (listings, diags) = parse(map(vec![("sort", b(true))]));
        assert_eq!(listings[0].sort, None);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }

    // 9. sort: ["date desc"] → Desc
    #[test]
    fn sort_parses_field_with_direction() {
        let (listings, _) = parse(map(vec![("sort", arr(vec![s("date desc")]))]));
        let sort = listings[0].sort.as_ref().unwrap();
        assert_eq!(sort[0].field, "date");
        assert_eq!(sort[0].direction, SortDirection::Desc);
    }

    // 9b. A scalar `sort: "date desc"` routinely arrives as
    // `PandocInlines` (front-matter strings hit the markdown
    // sublexer). The scalar arm must route through `as_plain_text`
    // like the array arm does, instead of falling through to the
    // Q-12-3 diagnostic with an empty sort. See bd-2qjnd.
    #[test]
    fn sort_pandoc_inlines_scalar_parses() {
        use quarto_pandoc_types::inline::{Inline, Str};
        let inlines: quarto_pandoc_types::inline::Inlines = vec![Inline::Str(Str {
            text: "date desc".to_string(),
            source_info: SourceInfo::for_test(),
        })];
        let sort_val = ConfigValue::new_inlines(inlines, SourceInfo::for_test());
        let (listings, diags) = parse(map(vec![("sort", sort_val)]));
        let sort = listings[0]
            .sort
            .as_ref()
            .unwrap_or_else(|| panic!("sort dropped; diags: {diags:?}"));
        assert_eq!(sort[0].field, "date");
        assert_eq!(sort[0].direction, SortDirection::Desc);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }

    // 10. multi-key sort preserves order
    #[test]
    fn sort_parses_multi_key() {
        let (listings, _) = parse(map(vec![("sort", arr(vec![s("date desc"), s("title")]))]));
        let sort = listings[0].sort.as_ref().unwrap();
        assert_eq!(sort.len(), 2);
        assert_eq!(sort[0].field, "date");
        assert_eq!(sort[0].direction, SortDirection::Desc);
        assert_eq!(sort[1].field, "title");
        assert_eq!(sort[1].direction, SortDirection::Asc);
    }

    // 13. max-items
    #[test]
    fn max_items_parses() {
        let (listings, _) = parse(map(vec![("max-items", s("5"))]));
        assert_eq!(listings[0].max_items, Some(5));
    }

    // 14. type-specific default fields applied
    #[test]
    fn type_default_fields_table_has_three() {
        let (listings, _) = parse(s("table"));
        assert_eq!(listings[0].fields, vec!["date", "title", "author"]);
        // Table also flips sort_ui / filter_ui to true.
        assert!(listings[0].sort_ui);
        assert!(listings[0].filter_ui);
        assert_eq!(listings[0].page_size, 30);
    }

    // ─────────────────────────────────────────────────────────────
    // bd-listing-table-fields-peg1w3b3: field-links defaults +
    // explicit-fields tracking (Q1 parity for table listings).
    // ─────────────────────────────────────────────────────────────

    // Q1 `kDefaultFieldLinks` applies to table listings only.
    #[test]
    fn field_links_defaults_to_title_filename_for_table() {
        let (listings, _) = parse(s("table"));
        assert_eq!(
            listings[0].field_links,
            Some(vec!["title".to_string(), "filename".to_string()])
        );
    }

    #[test]
    fn field_links_defaults_to_empty_for_non_table_types() {
        let (listings, _) = parse(s("default"));
        assert_eq!(listings[0].field_links, Some(Vec::new()));
        let (listings, _) = parse(s("grid"));
        assert_eq!(listings[0].field_links, Some(Vec::new()));
    }

    // Author-explicit `field-links: []` disables linking; the table
    // default must not overwrite it.
    #[test]
    fn field_links_explicit_empty_survives_table_defaults() {
        let (listings, _) = parse(map(vec![
            ("type", s("table")),
            ("field-links", arr(vec![])),
        ]));
        assert_eq!(listings[0].field_links, Some(Vec::new()));
    }

    #[test]
    fn field_links_explicit_list_parses() {
        let (listings, _) = parse(map(vec![
            ("type", s("table")),
            ("field-links", arr(vec![s("author")])),
        ]));
        assert_eq!(listings[0].field_links, Some(vec!["author".to_string()]));
    }

    // `fields_explicit` gates render-time presence filtering: only
    // *defaulted* field sets are filtered against the items.
    #[test]
    fn fields_explicit_true_when_author_supplies_fields() {
        let (listings, _) = parse(map(vec![
            ("type", s("table")),
            ("fields", arr(vec![s("title")])),
        ]));
        assert!(listings[0].fields_explicit);
        assert_eq!(listings[0].fields, vec!["title"]);
    }

    #[test]
    fn fields_explicit_false_when_fields_defaulted() {
        let (listings, _) = parse(s("table"));
        assert!(!listings[0].fields_explicit);
    }

    // Explicit-but-empty `fields: []` falls back to the type default
    // set and is treated as non-explicit (same as today).
    #[test]
    fn fields_empty_list_treated_as_defaulted() {
        let (listings, _) = parse(map(vec![("type", s("table")), ("fields", arr(vec![]))]));
        assert!(!listings[0].fields_explicit);
        assert_eq!(listings[0].fields, vec!["date", "title", "author"]);
    }

    // template + non-custom type → Q-12-7
    #[test]
    fn template_with_non_custom_type_emits_q_12_7() {
        let (_listings, diags) = parse(map(vec![
            ("type", s("default")),
            ("template", s("custom.template")),
        ]));
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code.as_deref(), Some("Q-12-7"));
    }

    // .ejs.md template extension → Q-12-9 deprecation warning
    #[test]
    fn template_ejs_md_extension_emits_q_12_9() {
        let (_listings, diags) = parse(map(vec![
            ("type", s("custom")),
            ("template", s("legacy.ejs.md")),
        ]));
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("Q-12-9")),
            "expected Q-12-9, got: {:?}",
            diags
        );
    }

    // field-display-names with non-string value → Q-12-5
    #[test]
    fn field_display_names_non_string_emits_q_12_5() {
        let (listings, diags) = parse(map(vec![(
            "field-display-names",
            map(vec![("title", b(true))]),
        )]));
        assert!(listings[0].field_display_names.is_empty());
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code.as_deref(), Some("Q-12-5"));
    }

    // categories: true → Default mode
    #[test]
    fn categories_true_parses_to_default_mode() {
        let (listings, _) = parse(map(vec![("categories", b(true))]));
        assert_eq!(listings[0].categories, ListingCategoriesMode::Default);
    }

    // categories: cloud
    #[test]
    fn categories_cloud_parses() {
        let (listings, _) = parse(map(vec![("categories", s("cloud"))]));
        assert_eq!(listings[0].categories, ListingCategoriesMode::Cloud);
    }

    // collision: explicit duplicate id → Q-12-4 + first wins
    #[test]
    fn duplicate_id_emits_q_12_4_and_first_wins() {
        let (listings, diags) = parse(arr(vec![
            map(vec![("id", s("dupe")), ("type", s("default"))]),
            map(vec![("id", s("dupe")), ("type", s("grid"))]),
        ]));
        assert_eq!(listings.len(), 1);
        assert_eq!(listings[0].kind, ListingType::Default);
        assert!(diags.iter().any(|d| d.code.as_deref() == Some("Q-12-4")));
    }

    // ─────────────────────────────────────────────────────────────
    // L6 (bd-xbnf): flatten_content_globs
    //
    // The profile stage calls this to read just the glob entries
    // out of `meta.listing` (before resolving them via
    // `glob_resolve`), ignoring all the other listing config. It
    // must agree with `parse_listings` on every accepted shape so
    // graph edges line up with what the L3 generate transform
    // resolves at render time.
    // ─────────────────────────────────────────────────────────────

    fn meta_with_listing(value: ConfigValue) -> ConfigValue {
        map(vec![("listing", value)])
    }

    /// `listing: true` shorthand → default contents `*.qmd`.
    #[test]
    fn extract_globs_from_single_listing_default_shorthand() {
        let meta = meta_with_listing(b(true));
        assert_eq!(glob_patterns(&flatten_content_globs(&meta)), vec!["*.qmd"]);
    }

    /// Explicit `contents:` glob list.
    #[test]
    fn extract_globs_from_single_listing_with_explicit_contents() {
        let meta = meta_with_listing(map(vec![("contents", arr(vec![s("posts/*.qmd")]))]));
        assert_eq!(
            glob_patterns(&flatten_content_globs(&meta)),
            vec!["posts/*.qmd"]
        );
    }

    /// Map listing without `contents:` (e.g. `{ type: grid }`) →
    /// default contents `*.qmd` (matches `apply_type_defaults`).
    #[test]
    fn extract_globs_from_single_listing_no_contents_shorthand() {
        let meta = meta_with_listing(map(vec![("type", s("grid"))]));
        assert_eq!(glob_patterns(&flatten_content_globs(&meta)), vec!["*.qmd"]);
    }

    /// Array of listings; globs flatten across listings.
    #[test]
    fn extract_globs_from_array_of_listings() {
        let meta = meta_with_listing(arr(vec![
            map(vec![("contents", arr(vec![s("a/*.qmd")]))]),
            map(vec![("contents", arr(vec![s("b/*.qmd"), s("c/*.qmd")]))]),
        ]));
        assert_eq!(
            glob_patterns(&flatten_content_globs(&meta)),
            vec!["a/*.qmd", "b/*.qmd", "c/*.qmd"]
        );
    }

    /// A record's document `path:` is a dependency edge (plan §D4):
    /// it is emitted as a literal pattern carrying the value's own
    /// provenance, leading `/` included. Pathless, remote,
    /// non-document and glob-shaped paths contribute nothing.
    #[test]
    fn extract_globs_keeps_record_document_paths_only() {
        let meta = meta_with_listing(map(vec![(
            "contents",
            arr(vec![
                map(vec![("title", s("pathless"))]),
                map(vec![("path", s("download.qmd"))]),
                map(vec![("path", s("/guide/install.qmd"))]),
                map(vec![("path", s("https://example.com/x.qmd"))]),
                map(vec![("path", s("report.pdf"))]),
                map(vec![("path", s("posts/*.qmd"))]),
                s("*.qmd"),
            ]),
        )]));
        assert_eq!(
            glob_patterns(&flatten_content_globs(&meta)),
            vec!["download.qmd", "/guide/install.qmd", "*.qmd"],
            "a leading `/` is a project-root dependency, not a remote URL"
        );
    }

    /// `listing: false` → no globs (parse_listings emits Q-12-6
    /// here; we discard that diagnostic at extract time).
    #[test]
    fn extract_globs_listing_false_is_empty() {
        let meta = meta_with_listing(b(false));
        assert!(flatten_content_globs(&meta).is_empty());
    }

    /// Meta with no `listing:` key → empty globs.
    #[test]
    fn extract_globs_no_listing_key_is_empty() {
        let meta = map(vec![("title", s("Hello"))]);
        assert!(flatten_content_globs(&meta).is_empty());
    }

    /// `contents:` as a single string (not an array) — `parse_contents`
    /// accepts this shape; `flatten_content_globs` must agree.
    #[test]
    fn extract_globs_handles_string_shorthand_contents() {
        let meta = meta_with_listing(map(vec![("contents", s("*.qmd"))]));
        assert_eq!(glob_patterns(&flatten_content_globs(&meta)), vec!["*.qmd"]);
    }
}
