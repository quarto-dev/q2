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
//! `crates/quarto-error-reporting/error_catalog.json`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::config_value::ConfigValueKind;
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
    pub field_display_names: BTreeMap<String, String>,
    pub field_types: BTreeMap<String, ColumnType>,
    pub field_links: Vec<String>,
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
            field_display_names: BTreeMap::new(),
            field_types: BTreeMap::new(),
            field_links: Vec::new(),
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
    /// Glob pattern resolved against the project file set; see
    /// L3 D10 (host-relative globs filtered through ProjectIndex).
    Glob(String),
    /// Inline metadata record. Schema accepts; L3 emits `Q-12-2`
    /// and skips the entry until a follow-up bd issue lands.
    Inline(BTreeMap<String, ConfigValue>),
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
    match &value.value {
        ConfigValueKind::Scalar(Yaml::Boolean(true)) => {
            // `listing: true` shorthand
            vec![default_listing_with_id("listing-1")]
        }
        ConfigValueKind::Scalar(Yaml::Boolean(false)) => {
            push_diag(
                diagnostics,
                "Q-12-6",
                "`listing: false` is not allowed; remove the key entirely instead.",
                value,
            );
            Vec::new()
        }
        ConfigValueKind::Scalar(Yaml::String(name)) => {
            // `listing: default | grid | table | custom`
            let kind = match parse_type_name(name) {
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
            vec![l]
        }
        ConfigValueKind::Map(_) => {
            let l = parse_one_listing(value, "listing-1", diagnostics);
            vec![l]
        }
        ConfigValueKind::Array(items) => {
            let mut explicit_ids: Vec<String> = Vec::new();
            let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

            // Pre-collect explicit ids so synthesized ids skip them.
            for item in items {
                if let ConfigValueKind::Map(_) = &item.value {
                    if let Some(id_val) = item.get("id") {
                        if let Some(id) = id_val.as_plain_text() {
                            explicit_ids.push(id);
                        }
                    }
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
                    let candidate = loop {
                        let c = format!("listing-{}", next_synth);
                        next_synth += 1;
                        if !explicit_ids.iter().any(|e| e == &c) && !seen.contains(&c) {
                            break c;
                        }
                    };
                    candidate
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
                        item,
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

    let mut l = Listing::default();
    l.id = fallback_id.to_string();

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
            }
            "field-display-names" => {
                l.field_display_names = parse_string_string_map(&entry.value, diagnostics);
            }
            "field-types" => {
                l.field_types = parse_field_types(&entry.value, diagnostics);
            }
            "field-links" => l.field_links = parse_string_list(&entry.value),
            "field-sort" => l.field_sort = parse_string_list(&entry.value),
            "field-filter" => l.field_filter = parse_string_list(&entry.value),
            "field-required" => l.field_required = parse_string_list(&entry.value),
            "page-size" => {
                if let Some(n) = entry.value.as_plain_text().and_then(|s| s.parse().ok()) {
                    l.page_size = n;
                }
            }
            "max-items" => {
                l.max_items = entry.value.as_plain_text().and_then(|s| s.parse().ok());
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
                l.sort = Some(parse_sort(&entry.value, diagnostics));
            }
            "template" => {
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
                l.grid_columns = entry.value.as_plain_text().and_then(|s| s.parse().ok());
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
                if let Some(n) = entry.value.as_plain_text().and_then(|s| s.parse().ok()) {
                    l.max_description_length = n;
                }
            }
            "include" => l.include = parse_filter_list(&entry.value),
            "exclude" => l.exclude = parse_filter_list(&entry.value),
            "categories" => {
                l.categories = parse_categories_mode(&entry.value);
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
    if l.template.is_some() && l.kind != ListingType::Custom {
        push_diag(
            diagnostics,
            "Q-12-7",
            "`template:` was set but `type:` is not `custom`; falling back to the built-in template for the declared type.",
            value,
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
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Vec<ListingContents> {
    match &value.value {
        ConfigValueKind::Scalar(Yaml::String(s)) => {
            vec![ListingContents::Glob(s.clone())]
        }
        ConfigValueKind::Glob(pattern) => {
            vec![ListingContents::Glob(pattern.clone())]
        }
        ConfigValueKind::Array(items) => items
            .iter()
            .filter_map(|item| match &item.value {
                ConfigValueKind::Scalar(Yaml::String(s)) => Some(ListingContents::Glob(s.clone())),
                ConfigValueKind::Glob(pattern) => Some(ListingContents::Glob(pattern.clone())),
                ConfigValueKind::Map(entries) => {
                    push_diag(
                        diagnostics,
                        "Q-12-2",
                        "Inline `contents:` records are not yet supported; entry skipped.",
                        item,
                    );
                    let map = entries
                        .iter()
                        .map(|e| (e.key.clone(), e.value.clone()))
                        .collect::<BTreeMap<_, _>>();
                    Some(ListingContents::Inline(map))
                }
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
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

fn parse_sort(value: &ConfigValue, diagnostics: &mut Vec<DiagnosticMessage>) -> Vec<ListingSort> {
    match &value.value {
        ConfigValueKind::Scalar(Yaml::Boolean(false)) => Vec::new(),
        ConfigValueKind::Scalar(Yaml::String(s)) => vec![parse_one_sort_key(s)],
        ConfigValueKind::Array(items) => items
            .iter()
            .filter_map(|v| v.as_plain_text())
            .map(|s| parse_one_sort_key(&s))
            .collect(),
        _ => {
            push_diag(
                diagnostics,
                "Q-12-3",
                "`sort:` must be a string, array of strings, or `false`.",
                value,
            );
            Vec::new()
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
        Some("true") | Some("default") => ListingCategoriesMode::Default,
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
                    feed.items = e.value.as_plain_text().and_then(|s| s.parse().ok());
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
        l.contents.push(ListingContents::Glob("*.qmd".to_string()));
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
        ConfigValue::new_string(value, SourceInfo::default())
    }

    fn b(value: bool) -> ConfigValue {
        ConfigValue::new_bool(value, SourceInfo::default())
    }

    fn arr(items: Vec<ConfigValue>) -> ConfigValue {
        ConfigValue::new_array(items, SourceInfo::default())
    }

    fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        let map_entries: Vec<ConfigMapEntry> = entries
            .into_iter()
            .map(|(k, v)| ConfigMapEntry {
                key: k.to_string(),
                key_source: SourceInfo::default(),
                value: v,
            })
            .collect();
        ConfigValue::new_map(map_entries, SourceInfo::default())
    }

    fn parse(value: ConfigValue) -> (Vec<Listing>, Vec<DiagnosticMessage>) {
        let mut diags = Vec::new();
        let listings = parse_listings(&value, &mut diags);
        (listings, diags)
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
        assert_eq!(
            listings[0].contents,
            vec![ListingContents::Glob("*.qmd".to_string())]
        );
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
        assert_eq!(
            listings[0].contents,
            vec![ListingContents::Glob("posts/**/*.qmd".to_string())]
        );
    }

    // 7. inline-record contents emits Q-12-2 and is captured
    #[test]
    fn contents_inline_record_emits_diagnostic() {
        let (listings, diags) = parse(map(vec![(
            "contents",
            arr(vec![map(vec![
                ("title", s("foo")),
                ("path", s("bar.html")),
            ])]),
        )]));
        assert_eq!(listings.len(), 1);
        // The inline entry is captured as ListingContents::Inline.
        assert_eq!(listings[0].contents.len(), 1);
        assert!(matches!(
            listings[0].contents[0],
            ListingContents::Inline(_)
        ));
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code.as_deref(), Some("Q-12-2"));
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

    // 9. sort: ["date desc"] → Desc
    #[test]
    fn sort_parses_field_with_direction() {
        let (listings, _) = parse(map(vec![("sort", arr(vec![s("date desc")]))]));
        let sort = listings[0].sort.as_ref().unwrap();
        assert_eq!(sort[0].field, "date");
        assert_eq!(sort[0].direction, SortDirection::Desc);
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
}
