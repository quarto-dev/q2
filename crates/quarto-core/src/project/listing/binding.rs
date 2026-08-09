/*
 * project/listing/binding.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Build the per-listing [`TemplateValue::Map`] binding consumed by
//! the doctemplate built-in templates.
//!
//! See L2 §"Per-item template binding" for the canonical shape.
//! This module is the *load-bearing public contract* for L8 custom
//! templates: adding a key is non-breaking, but renaming or
//! removing a key is breaking and must be called out in commits +
//! the L11 close-out report.
//!
//! Note: this v1 binding intentionally does **not** include:
//! - `other-metadata-html` — defer to bd-0wyo.
//!
//! `category-html` was added by L5 (bd-5vsr); see
//! [`helpers::category_html`].

use std::collections::HashMap;

use quarto_doctemplate::{TemplateContext, TemplateValue};
use quarto_pandoc_types::ConfigValue;

use super::config::{GridItemAlign, ImageAlign, Listing, ListingCategoriesMode, ListingType};
use super::helpers;
use super::item::ListingItem;
use crate::dates::{DateStyle, format_date, parse_date};

/// Build the full [`TemplateContext`] for one [`Listing`] +
/// hydrated items. The returned context has a single entry,
/// keyed by `"listing"`, plus an `"items"` entry — matching the
/// L2 binding contract — plus a `"project"` map carrying any
/// project-wide values the templates read.
///
/// Arguments:
///
/// - `host_dir` is the host page's project-relative directory
///   (forward-slash, no trailing slash; empty string when the
///   host is at the project root). Used to compute
///   host-dir-relative `.qmd` link targets so
///   [`crate::transforms::LinkRewriteTransform`] can rewrite them
///   downstream.
/// - `project_meta` is the host page's merged metadata; we read
///   `website.site-url` and `website.title` from it for the
///   `project.*` binding.
pub fn build_listing_context(
    listing: &Listing,
    items: &[ListingItem],
    host_dir: &str,
    project_meta: &ConfigValue,
) -> TemplateContext {
    let mut ctx = TemplateContext::new();

    // Effective date style for date-typed item fields (bd-13f821l5):
    // listing-level `date-format` > document `date-format` > `medium`
    // — Q1's precedence in website-listing-template.ts, with items
    // pre-formatted before the (logic-less) template interpolates
    // them, exactly like Q1 pre-formats its EJS records.
    let date_style = listing
        .date_format
        .clone()
        .or_else(|| {
            project_meta
                .get("date-format")
                .and_then(|v| v.as_plain_text())
        })
        .map_or(DateStyle::Medium, |s| DateStyle::parse(&s));
    // Effective field set: author-explicit `fields:` verbatim;
    // defaulted sets presence-filtered against the items (Q1
    // parity, bd-listing-table-fields-peg1w3b3). Everything below
    // (the `listing.fields` binding, `show.*`, the table
    // header/rows) reads this list, not `listing.fields`.
    let fields = effective_fields(listing, items, &date_style);
    ctx.insert("listing", build_listing_map(listing, &fields));
    ctx.insert(
        "items",
        build_items_list(listing, items, host_dir, &date_style, &fields),
    );
    ctx.insert("project", build_project_map(project_meta));

    ctx
}

/// Q1 parity (`website-listing-read.ts` suggested-fields filter):
/// a *defaulted* field set keeps only fields at least one item
/// carries — `image` always survives (a listing-level
/// `image-placeholder` can fill it at render time). If the filter
/// empties the list, keep the unfiltered defaults (defensive;
/// `title` is non-optional on [`ListingItem`], so built-in default
/// sets can never fully empty). Author-explicit `fields:` is used
/// verbatim.
fn effective_fields(
    listing: &Listing,
    items: &[ListingItem],
    date_style: &DateStyle,
) -> Vec<String> {
    if listing.fields_explicit {
        return listing.fields.clone();
    }
    let filtered: Vec<String> = listing
        .fields
        .iter()
        .filter(|f| {
            f.as_str() == "image"
                || items
                    .iter()
                    .any(|it| item_field_display_value(it, f, date_style).is_some())
        })
        .cloned()
        .collect();
    if filtered.is_empty() {
        listing.fields.clone()
    } else {
        filtered
    }
}

fn build_listing_map(listing: &Listing, fields: &[String]) -> TemplateValue {
    let mut m = HashMap::new();
    m.insert("id".to_string(), TemplateValue::String(listing.id.clone()));
    m.insert(
        "type".to_string(),
        TemplateValue::String(listing_type_name(listing.kind).to_string()),
    );
    m.insert(
        "fields".to_string(),
        TemplateValue::List(
            fields
                .iter()
                .map(|s| TemplateValue::String(s.clone()))
                .collect(),
        ),
    );
    m.insert(
        "field-links".to_string(),
        TemplateValue::List(
            listing
                .field_links
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(|s| TemplateValue::String(s.clone()))
                .collect(),
        ),
    );
    // Pre-rendered markdown header for the table built-in: the
    // header row (display names) + the separator row
    // (bd-listing-table-fields-peg1w3b3).
    m.insert(
        "table-header".to_string(),
        TemplateValue::String(table_header(fields, &listing.field_display_names)),
    );
    m.insert(
        "page-size".to_string(),
        TemplateValue::String(listing.page_size.to_string()),
    );
    if let Some(max) = listing.max_items {
        m.insert(
            "max-items".to_string(),
            TemplateValue::String(max.to_string()),
        );
    }
    m.insert(
        "filter-ui".to_string(),
        TemplateValue::Bool(listing.filter_ui),
    );
    m.insert("sort-ui".to_string(), TemplateValue::Bool(listing.sort_ui));
    m.insert(
        "max-description-length".to_string(),
        TemplateValue::String(listing.max_description_length.to_string()),
    );
    m.insert(
        "categories".to_string(),
        TemplateValue::String(categories_mode_name(listing.categories).to_string()),
    );

    // Type-specific knobs.
    if let Some(align) = listing.image_align {
        m.insert(
            "image-align".to_string(),
            TemplateValue::String(image_align_name(align).to_string()),
        );
    }
    if let Some(h) = listing.image_height.as_deref() {
        m.insert(
            "image-height".to_string(),
            TemplateValue::String(h.to_string()),
        );
    }
    if let Some(lazy) = listing.image_lazy_loading {
        m.insert("image-lazy-loading".to_string(), TemplateValue::Bool(lazy));
    }
    if let Some(cols) = listing.grid_columns {
        m.insert(
            "grid-columns".to_string(),
            TemplateValue::String(cols.to_string()),
        );
    }
    if let Some(border) = listing.grid_item_border {
        m.insert("grid-item-border".to_string(), TemplateValue::Bool(border));
    }
    if let Some(align) = listing.grid_item_align {
        m.insert(
            "grid-item-align".to_string(),
            TemplateValue::String(grid_item_align_name(align).to_string()),
        );
    }
    if let Some(striped) = listing.table_striped {
        m.insert("table-striped".to_string(), TemplateValue::Bool(striped));
    }
    if let Some(hover) = listing.table_hover {
        m.insert("table-hover".to_string(), TemplateValue::Bool(hover));
    }

    // Author-supplied template-params get exposed verbatim under
    // `listing.template-params` for L8 custom templates. v1 is
    // permissive: any ConfigValue scalars become strings.
    if !listing.template_params.is_empty() {
        let mut tp = HashMap::new();
        for (k, v) in &listing.template_params {
            tp.insert(k.clone(), config_value_to_template_value(v));
        }
        m.insert("template-params".to_string(), TemplateValue::Map(tp));
    }

    TemplateValue::Map(m)
}

fn build_items_list(
    listing: &Listing,
    items: &[ListingItem],
    host_dir: &str,
    date_style: &DateStyle,
    fields: &[String],
) -> TemplateValue {
    TemplateValue::List(
        items
            .iter()
            .enumerate()
            .map(|(i, item)| build_item_map(listing, item, i, host_dir, date_style, fields))
            .collect(),
    )
}

/// Format a raw item date for display; unparseable values pass
/// through unchanged (the item metadata may carry arbitrary strings).
fn display_date(raw: &str, style: &DateStyle) -> String {
    match parse_date(raw) {
        Some(parsed) => format_date(&parsed, style).0,
        None => raw.to_string(),
    }
}

fn build_item_map(
    listing: &Listing,
    item: &ListingItem,
    index: usize,
    host_dir: &str,
    date_style: &DateStyle,
    fields: &[String],
) -> TemplateValue {
    let mut m = HashMap::new();

    // Curated fields. Optional fields are only inserted when
    // present so $if(<field>)$ correctly skips undefined values
    // (rather than seeing an empty-string truthy false).
    m.insert(
        "title".to_string(),
        TemplateValue::String(item.title.clone()),
    );
    if let Some(s) = item.subtitle.as_deref() {
        m.insert("subtitle".to_string(), TemplateValue::String(s.to_string()));
    }
    if let Some(s) = item.description.as_deref() {
        m.insert(
            "description".to_string(),
            TemplateValue::String(s.to_string()),
        );
    }
    if let Some(s) = item.author.as_deref() {
        m.insert("author".to_string(), TemplateValue::String(s.to_string()));
    }
    if !item.authors.is_empty() {
        m.insert(
            "authors".to_string(),
            TemplateValue::List(
                item.authors
                    .iter()
                    .map(|a| TemplateValue::String(a.clone()))
                    .collect(),
            ),
        );
    }
    if let Some(s) = item.date.as_deref() {
        m.insert(
            "date".to_string(),
            TemplateValue::String(display_date(s, date_style)),
        );
    }
    if let Some(s) = item.date_modified.as_deref() {
        m.insert(
            "date-modified".to_string(),
            TemplateValue::String(display_date(s, date_style)),
        );
    }
    if !item.categories.is_empty() {
        m.insert(
            "categories".to_string(),
            TemplateValue::List(
                item.categories
                    .iter()
                    .map(|c| TemplateValue::String(c.clone()))
                    .collect(),
            ),
        );
    }
    if let Some(s) = item.image.as_deref() {
        m.insert("image".to_string(), TemplateValue::String(s.to_string()));
    }
    if let Some(s) = item.image_alt.as_deref() {
        m.insert(
            "image-alt".to_string(),
            TemplateValue::String(s.to_string()),
        );
    }
    if let Some(n) = item.reading_time_minutes {
        m.insert(
            "reading-time".to_string(),
            TemplateValue::String(format!("{} min read", n)),
        );
        m.insert(
            "reading-time-minutes".to_string(),
            TemplateValue::String(n.to_string()),
        );
    }
    if let Some(n) = item.word_count {
        m.insert(
            "word-count".to_string(),
            TemplateValue::String(n.to_string()),
        );
    }

    // Path bookkeeping.
    //
    // `path` is the link target the templates use. We emit the
    // item's source-path-relative-to-the-host-page (e.g.
    // `a-second-listing.qmd` from `posts/index.qmd`) — the same
    // form a body link `[label](other.qmd)` would take.
    // `LinkRewriteTransform` (which runs after this transform in
    // the AstTransformsStage Finalization phase) then rewrites the
    // .qmd to the page-relative output URL via the active
    // resolver. This is what makes the listing's links navigate
    // correctly in:
    //   - native CLI: rewrites to e.g. `a-second-listing.html`
    //   - hub-client/WASM (vfs_root resolver): rewrites to a
    //     `/.quarto/project-artifacts/...` URL that
    //     iframePostProcessor reverse-maps to .qmd for in-app
    //     navigation (case 3 in iframePostProcessor.ts).
    //
    // `outputHref` retains the rendered .html path for templates
    // (e.g. L7's placeholder href, RSS feed item URLs) that need
    // the post-render output href specifically.
    let path = host_relative_qmd(&item.source_path, host_dir);
    m.insert("path".to_string(), TemplateValue::String(path.clone()));
    m.insert(
        "outputHref".to_string(),
        TemplateValue::String(item.output_href.clone()),
    );
    m.insert(
        "filename".to_string(),
        TemplateValue::String(
            item.source_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string(),
        ),
    );

    // Pre-rendered helper strings.
    let img_html = helpers::image_html(item, listing, host_dir);
    m.insert(
        "image-html".to_string(),
        TemplateValue::String(img_html.clone()),
    );
    m.insert(
        "metadata-attrs".to_string(),
        TemplateValue::String(helpers::metadata_attrs(item, index)),
    );
    // Description envelope (L7 plan §"How the begin / end markers
    // reach the templates"). Both keys are always populated;
    // `$description-placeholder-begin$` and `-end$` flank the L1
    // fallback `$description$` block in the templates so the L7
    // post-render step has a region to substitute.
    m.insert(
        "description-placeholder-begin".to_string(),
        TemplateValue::String(helpers::description_placeholder_begin(item, listing)),
    );
    m.insert(
        "description-placeholder-end".to_string(),
        TemplateValue::String(helpers::description_placeholder_end()),
    );
    // Image envelope. Always populated regardless of whether
    // `item.image` is set: the templates only reference these keys
    // inside the `$if(image-html)$ ... $else$ ... $endif$` block, so
    // markers for image-present items never reach the rendered
    // output. Keeping the binding unconditional keeps the template
    // logic simple — the decision lives in `$if(image-html)$`, not
    // in the binding.
    m.insert(
        "image-placeholder-begin".to_string(),
        TemplateValue::String(helpers::image_placeholder_begin(item, listing, index)),
    );
    m.insert(
        "image-placeholder-end".to_string(),
        TemplateValue::String(helpers::image_placeholder_end()),
    );
    m.insert(
        "category-html".to_string(),
        TemplateValue::String(helpers::category_html(item)),
    );

    // Free-form author fields.
    if !item.extra.is_empty() {
        let mut extra = HashMap::new();
        for (k, v) in &item.extra {
            extra.insert(k.clone(), config_value_to_template_value(v));
        }
        m.insert("extra".to_string(), TemplateValue::Map(extra));
    }

    // Per-item show flags computed from the effective field set
    // (author-explicit fields, or presence-filtered defaults).
    // Templates that want to know "did the listing config say to
    // show field X?" read `$item.show.<field>$` (Q1 semantics).
    let mut show = HashMap::new();
    for field in fields {
        show.insert(field.clone(), TemplateValue::Bool(true));
    }
    m.insert("show".to_string(), TemplateValue::Map(show));

    // Pre-rendered markdown table row for the table built-in
    // (bd-listing-table-fields-peg1w3b3). Uses the values already
    // computed above (path for linked cells, image-html for the
    // image column).
    m.insert(
        "table-row".to_string(),
        TemplateValue::String(table_row(
            item, listing, fields, date_style, &path, &img_html,
        )),
    );

    TemplateValue::Map(m)
}

// ─────────────────────────────────────────────────────────────────
// Table listing pre-rendered strings
// (bd-listing-table-fields-peg1w3b3)
// ─────────────────────────────────────────────────────────────────

/// Q1's built-in field display names (`_language.yml`
/// `listing-page-field-*`, English hardcoded until Q2 grows
/// language support). Unknown fields fall back to the raw field
/// name — Q1's `utilities.fieldName` behavior, deliberately not
/// title-cased.
fn default_display_name(field: &str) -> &str {
    match field {
        "image" => " ",
        "date" => "Date",
        "title" => "Title",
        "description" => "Description",
        "author" => "Author",
        "filename" => "File Name",
        "date-modified" | "file-modified" => "Modified",
        "subtitle" => "Subtitle",
        "reading-time" => "Reading Time",
        "word-count" => "Word Count",
        "categories" => "Categories",
        other => other,
    }
}

fn display_name(field: &str, overrides: &std::collections::BTreeMap<String, String>) -> String {
    overrides
        .get(field)
        .map_or_else(|| default_display_name(field), String::as_str)
        .to_string()
}

/// Make a string safe inside one markdown pipe-table cell:
/// newlines flatten to spaces (a cell is one line by definition)
/// and `|` is escaped so it can't terminate the cell.
fn escape_table_cell(s: &str) -> String {
    s.replace("\r\n", " ")
        .replace(['\n', '\r'], " ")
        .replace('|', "\\|")
}

/// The pre-rendered `listing.table-header` value: header row from
/// the effective fields (display-name overlay applied) plus the
/// pipe-table separator row.
fn table_header(
    fields: &[String],
    overrides: &std::collections::BTreeMap<String, String>,
) -> String {
    let names: Vec<String> = fields
        .iter()
        .map(|f| escape_table_cell(&display_name(f, overrides)))
        .collect();
    format!(
        "| {} |\n|{}|",
        names.join(" | "),
        vec!["---"; fields.len().max(1)].join("|")
    )
}

/// The pre-rendered per-item `table-row` value.
fn table_row(
    item: &ListingItem,
    listing: &Listing,
    fields: &[String],
    date_style: &DateStyle,
    path: &str,
    image_html: &str,
) -> String {
    let linked = listing.field_links.as_deref().unwrap_or_default();
    let cells: Vec<String> = fields
        .iter()
        .map(|field| {
            if field == "image" {
                return escape_table_cell(image_html);
            }
            let Some(value) = item_field_display_value(item, field, date_style) else {
                return String::new();
            };
            let value = escape_table_cell(&value);
            if !value.is_empty() && !path.is_empty() && linked.iter().any(|l| l == field) {
                // Same link shape the templates use for titles;
                // LinkRewriteTransform rewrites the `.qmd` target
                // downstream.
                format!("[{}]({}){{.no-external}}", value, path)
            } else {
                value
            }
        })
        .collect();
    format!("| {} |", cells.join(" | "))
}

/// Display value for one field of one item — the table-cell
/// equivalent of Q1's `readField` + item-record pre-formatting.
/// `None` means "the item doesn't carry this field" (renders as an
/// empty cell; also drives presence filtering of defaulted field
/// sets).
fn item_field_display_value(
    item: &ListingItem,
    field: &str,
    date_style: &DateStyle,
) -> Option<String> {
    match field {
        "title" => Some(item.title.clone()),
        "subtitle" => item.subtitle.clone(),
        "description" => item.description.clone(),
        "author" => item.author.clone(),
        "authors" => (!item.authors.is_empty()).then(|| item.authors.join(", ")),
        "date" => item.date.as_deref().map(|s| display_date(s, date_style)),
        "date-modified" | "file-modified" => item
            .date_modified
            .as_deref()
            .map(|s| display_date(s, date_style)),
        "categories" => (!item.categories.is_empty()).then(|| item.categories.join(", ")),
        "image-alt" => item.image_alt.clone(),
        "reading-time" => item.reading_time_minutes.map(|n| format!("{} min read", n)),
        "word-count" => item.word_count.map(|n| n.to_string()),
        "filename" => item
            .source_path
            .file_name()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        _ => extra_field_value(&item.extra, field),
    }
}

/// Free-form field lookup: literal key first, then Q1's
/// dotted-path deref (`a.b` walks nested maps).
fn extra_field_value(
    extra: &std::collections::BTreeMap<String, ConfigValue>,
    field: &str,
) -> Option<String> {
    if let Some(cv) = extra.get(field) {
        return config_value_cell_string(cv);
    }
    if !field.contains('.') {
        return None;
    }
    let mut parts = field.split('.');
    let mut cv = extra.get(parts.next()?)?;
    for part in parts {
        cv = cv.get(part)?;
    }
    config_value_cell_string(cv)
}

/// Scalar → text; arrays join with `", "` (Q1 `readField`); maps
/// have no cell representation.
fn config_value_cell_string(cv: &ConfigValue) -> Option<String> {
    if let Some(items) = cv.as_array() {
        let parts: Vec<String> = items.iter().filter_map(config_value_cell_string).collect();
        return Some(parts.join(", "));
    }
    cv.as_plain_text()
}

/// Compute a host-page-relative forward-slash path string for
/// the item's source file. Empty `host_dir` means the host is at
/// the project root; otherwise we strip the host-dir prefix when
/// the item is inside it. Items outside the host's directory
/// stay project-relative — the `LinkRewriteTransform` resolver
/// handles either form, but the host-relative form keeps the
/// emitted markdown closer to what an author would write by hand.
fn host_relative_qmd(source_path: &std::path::Path, host_dir: &str) -> String {
    let project_relative = source_path
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(os) => os.to_str().map(str::to_string),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/");
    if host_dir.is_empty() {
        return project_relative;
    }
    // Walk off the shared directory prefix, then climb out of the
    // remaining host segments with `..`. An item outside the host's
    // directory (legal since bd-v7ixzsp5 — e.g. a `../rootpost.qmd`
    // or `_quarto.yml`-declared glob) gets `../…` exactly as a
    // hand-written body link from the host would, so
    // `LinkRewriteTransform` resolves it to the right page-relative
    // output URL.
    let host_segments: Vec<&str> = host_dir.split('/').collect();
    let path_segments: Vec<&str> = project_relative.split('/').collect();
    let common = host_segments
        .iter()
        .zip(path_segments.iter())
        .take_while(|(h, p)| h == p)
        .count();
    let mut out: Vec<&str> = Vec::new();
    out.extend(std::iter::repeat_n("..", host_segments.len() - common));
    out.extend(&path_segments[common..]);
    out.join("/")
}

fn build_project_map(meta: &ConfigValue) -> TemplateValue {
    let mut m = HashMap::new();
    if let Some(url) = meta
        .get_path(&["website", "site-url"])
        .and_then(|v| v.as_plain_text())
    {
        m.insert("site-url".to_string(), TemplateValue::String(url));
    }
    if let Some(title) = meta
        .get_path(&["website", "title"])
        .and_then(|v| v.as_plain_text())
    {
        m.insert("title".to_string(), TemplateValue::String(title));
    }
    TemplateValue::Map(m)
}

/// Lightweight `ConfigValue → TemplateValue` for the binding's
/// `extra` and `template-params` slots. Forwards to pampa's
/// authoritative bridge so PandocInlines/Blocks render correctly
/// for HTML.
fn config_value_to_template_value(cv: &ConfigValue) -> TemplateValue {
    use pampa::template::config_merge::{ConfigConversionContext, config_to_template_value};
    use pampa::template::context::MetaWriter;
    let mut conv = ConfigConversionContext::new(MetaWriter::Html);
    config_to_template_value(cv, &mut conv)
}

fn listing_type_name(t: ListingType) -> &'static str {
    match t {
        ListingType::Default => "default",
        ListingType::Grid => "grid",
        ListingType::Table => "table",
        ListingType::Custom => "custom",
    }
}

fn image_align_name(a: ImageAlign) -> &'static str {
    match a {
        ImageAlign::Left => "left",
        ImageAlign::Right => "right",
    }
}

fn grid_item_align_name(a: GridItemAlign) -> &'static str {
    match a {
        GridItemAlign::Left => "left",
        GridItemAlign::Right => "right",
        GridItemAlign::Center => "center",
    }
}

fn categories_mode_name(m: ListingCategoriesMode) -> &'static str {
    match m {
        ListingCategoriesMode::Disabled => "",
        ListingCategoriesMode::Default => "default",
        ListingCategoriesMode::Unnumbered => "unnumbered",
        ListingCategoriesMode::Cloud => "cloud",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::listing::config::apply_type_defaults;
    use crate::project::listing::config::{Listing, ListingType};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn item(title: &str) -> ListingItem {
        ListingItem {
            title: title.to_string(),
            subtitle: None,
            description: Some("A descr.".to_string()),
            author: Some("Jane".to_string()),
            authors: vec!["Jane".to_string()],
            date: Some("2026-01-01".to_string()),
            date_modified: None,
            categories: vec![],
            image: None,
            image_alt: None,
            image_lazy_loading: None,
            reading_time_minutes: Some(5),
            word_count: None,
            source_path: PathBuf::from("posts/foo.qmd"),
            output_href: "posts/foo.html".to_string(),
            extra: BTreeMap::new(),
        }
    }

    fn listing() -> Listing {
        let mut l = Listing {
            id: "main".to_string(),
            kind: ListingType::Default,
            ..Listing::default()
        };
        apply_type_defaults(&mut l);
        l
    }

    #[test]
    fn binding_includes_listing_id_and_items_array() {
        let ctx = build_listing_context(
            &listing(),
            &[item("A"), item("B")],
            "posts",
            &ConfigValue::default(),
        );
        let listing_map = ctx.get("listing").unwrap();
        assert_eq!(
            listing_map.get_path(&["id"]),
            Some(&TemplateValue::String("main".to_string()))
        );
        let items = ctx.get("items").unwrap();
        if let TemplateValue::List(arr) = items {
            assert_eq!(arr.len(), 2);
        } else {
            panic!("expected items list, got {:?}", items);
        }
    }

    #[test]
    fn item_binding_has_curated_fields() {
        let ctx = build_listing_context(
            &listing(),
            &[item("Hello")],
            "posts",
            &ConfigValue::default(),
        );
        let items = ctx.get("items").unwrap();
        let TemplateValue::List(arr) = items else {
            panic!("items not a list");
        };
        let TemplateValue::Map(m) = &arr[0] else {
            panic!("item not a map");
        };
        assert_eq!(
            m.get("title"),
            Some(&TemplateValue::String("Hello".to_string()))
        );
        // Dates are pre-formatted at record-build (bd-13f821l5);
        // `medium` is the default style, Q1's listing default.
        assert_eq!(
            m.get("date"),
            Some(&TemplateValue::String("Jan 1, 2026".to_string()))
        );
        assert_eq!(
            m.get("description"),
            Some(&TemplateValue::String("A descr.".to_string()))
        );
        assert!(m.contains_key("path"));
        assert!(m.contains_key("outputHref"));
        // Description envelope keys (L7 plan Phase 2 #7).
        assert!(m.contains_key("description-placeholder-begin"));
        assert!(m.contains_key("description-placeholder-end"));
        // Image envelope keys (always populated; L7 plan Phase 2 #8/#9).
        assert!(m.contains_key("image-placeholder-begin"));
        assert!(m.contains_key("image-placeholder-end"));
        // image-html is empty when no image
        assert_eq!(
            m.get("image-html"),
            Some(&TemplateValue::String(String::new()))
        );
    }

    // L7 plan §"Tests" Phase 2 #7
    #[test]
    fn binding_emits_description_begin_end_pair() {
        let ctx = build_listing_context(
            &listing(),
            &[item("Hello")],
            "posts",
            &ConfigValue::default(),
        );
        let TemplateValue::List(arr) = ctx.get("items").unwrap() else {
            panic!()
        };
        let TemplateValue::Map(m) = &arr[0] else {
            panic!()
        };
        let TemplateValue::String(begin) = m
            .get("description-placeholder-begin")
            .expect("description-placeholder-begin present")
        else {
            panic!("description-placeholder-begin not a string");
        };
        let TemplateValue::String(end) = m
            .get("description-placeholder-end")
            .expect("description-placeholder-end present")
        else {
            panic!("description-placeholder-end not a string");
        };
        assert!(
            begin.starts_with("<!-- desc-begin(5A0113B34292)["),
            "begin marker shape: {begin}"
        );
        assert_eq!(end, "<!-- desc-end(5A0113B34292) -->");
    }

    // L7 plan §"Tests" Phase 2 #8
    #[test]
    fn binding_emits_image_placeholder_begin_end_when_no_image() {
        let mut i = item("X");
        i.image = None;
        let ctx = build_listing_context(&listing(), &[i], "posts", &ConfigValue::default());
        let TemplateValue::List(arr) = ctx.get("items").unwrap() else {
            panic!()
        };
        let TemplateValue::Map(m) = &arr[0] else {
            panic!()
        };
        let TemplateValue::String(begin) = m
            .get("image-placeholder-begin")
            .expect("image-placeholder-begin present")
        else {
            panic!("image-placeholder-begin not a string");
        };
        let TemplateValue::String(end) = m
            .get("image-placeholder-end")
            .expect("image-placeholder-end present")
        else {
            panic!("image-placeholder-end not a string");
        };
        assert!(
            begin.starts_with("<!-- img-begin(9CEB782EFEE6)["),
            "begin marker shape: {begin}"
        );
        assert!(
            !begin.is_empty(),
            "begin marker must be non-empty when no image"
        );
        assert_eq!(end, "<!-- img-end(9CEB782EFEE6) -->");
    }

    // L7 plan §"Tests" Phase 2 #9 — clarified per the implementation
    // notes: we always populate the envelope markers, regardless of
    // `item.image`. The template's `$if(image-html)$ ... $else$ ...`
    // controls visibility, not the binding.
    #[test]
    fn binding_image_placeholder_present_even_when_image_set() {
        let mut i = item("X");
        i.image = Some("static.png".to_string());
        let ctx = build_listing_context(&listing(), &[i], "posts", &ConfigValue::default());
        let TemplateValue::List(arr) = ctx.get("items").unwrap() else {
            panic!()
        };
        let TemplateValue::Map(m) = &arr[0] else {
            panic!()
        };
        // Both keys exist and are non-empty even when image is set —
        // they're inert because the template's $else$ branch never
        // fires for image-present items.
        assert!(m.contains_key("image-placeholder-begin"));
        assert!(m.contains_key("image-placeholder-end"));
    }

    // L7 plan §"Tests" Phase 2 #13
    #[test]
    fn binding_image_placeholder_default_url_b64_encoded_into_marker() {
        use base64::Engine;
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let mut l = listing();
        l.image_placeholder = Some("assets/default.png".to_string());
        let mut i = item("X");
        i.image = None;
        let ctx = build_listing_context(&l, &[i], "posts", &ConfigValue::default());
        let TemplateValue::List(arr) = ctx.get("items").unwrap() else {
            panic!()
        };
        let TemplateValue::Map(m) = &arr[0] else {
            panic!()
        };
        let TemplateValue::String(begin) = m.get("image-placeholder-begin").unwrap() else {
            panic!()
        };
        let expected_b64 = URL_SAFE_NO_PAD.encode("assets/default.png".as_bytes());
        assert!(
            begin.contains(&format!(":{} -->", expected_b64)),
            "marker should carry b64-encoded default URL `{}`; got: {}",
            expected_b64,
            begin
        );
    }

    #[test]
    fn item_binding_omits_unset_optionals() {
        let mut i = item("X");
        i.subtitle = None;
        i.image = None;
        let ctx = build_listing_context(&listing(), &[i], "posts", &ConfigValue::default());
        let TemplateValue::List(arr) = ctx.get("items").unwrap() else {
            panic!()
        };
        let TemplateValue::Map(m) = &arr[0] else {
            panic!()
        };
        assert!(!m.contains_key("subtitle"));
        // `image` field absent (no image), but `image-html` is
        // always present (as empty string) so $if($image-html)$
        // skips cleanly.
        assert!(!m.contains_key("image"));
        assert_eq!(
            m.get("image-html"),
            Some(&TemplateValue::String(String::new()))
        );
    }

    #[test]
    fn item_binding_extra_passes_through_via_pampa_bridge() {
        use quarto_pandoc_types::ConfigValue;
        use quarto_source_map::SourceInfo;

        let mut i = item("X");
        i.extra.insert(
            "status".to_string(),
            ConfigValue::new_string("draft", SourceInfo::for_test()),
        );
        let ctx = build_listing_context(&listing(), &[i], "posts", &ConfigValue::default());
        let TemplateValue::List(arr) = ctx.get("items").unwrap() else {
            panic!()
        };
        let TemplateValue::Map(m) = &arr[0] else {
            panic!()
        };
        let extra = m.get("extra").expect("extra present");
        let TemplateValue::Map(extra_map) = extra else {
            panic!("extra not a map: {:?}", extra)
        };
        assert_eq!(
            extra_map.get("status"),
            Some(&TemplateValue::String("draft".to_string()))
        );
    }

    // L5 phase 2: per-item binding picks up `category-html`.
    #[test]
    fn item_binding_category_html_present_with_categories() {
        let mut i = item("X");
        i.categories = vec!["rust".to_string(), "design".to_string()];
        let ctx = build_listing_context(&listing(), &[i], "posts", &ConfigValue::default());
        let TemplateValue::List(arr) = ctx.get("items").unwrap() else {
            panic!()
        };
        let TemplateValue::Map(m) = &arr[0] else {
            panic!()
        };
        let TemplateValue::String(html) = m
            .get("category-html")
            .expect("category-html present on per-item map")
        else {
            panic!("category-html not a string");
        };
        // Two chips, b64 onclick args, plain category text.
        assert_eq!(
            html.matches(r#"<div class="listing-category""#).count(),
            2,
            "expected two chips, got: {html}"
        );
        assert!(html.contains(">rust<"));
        assert!(html.contains(">design<"));
    }

    #[test]
    fn item_binding_category_html_empty_when_no_categories() {
        // Default `item()` fixture has empty categories.
        let ctx = build_listing_context(&listing(), &[item("X")], "posts", &ConfigValue::default());
        let TemplateValue::List(arr) = ctx.get("items").unwrap() else {
            panic!()
        };
        let TemplateValue::Map(m) = &arr[0] else {
            panic!()
        };
        // Always present (so $if(category-html)$ never sees "undefined"),
        // but empty so the template skips the surrounding markup.
        assert_eq!(
            m.get("category-html"),
            Some(&TemplateValue::String(String::new()))
        );
    }

    // ─────────────────────────────────────────────────────────────
    // bd-listing-table-fields-peg1w3b3: dynamic table columns.
    // `listing.table-header` + per-item `table-row` are pre-rendered
    // markdown strings computed from listing.fields /
    // field-display-names / field-links (Q1 parity).
    // ─────────────────────────────────────────────────────────────

    fn table_listing(fields: &[&str]) -> Listing {
        let mut l = Listing {
            id: "t".to_string(),
            kind: ListingType::Table,
            ..Listing::default()
        };
        apply_type_defaults(&mut l);
        if !fields.is_empty() {
            l.fields = fields.iter().map(|s| s.to_string()).collect();
            l.fields_explicit = true;
        }
        // Decouple these unit tests from apply_type_defaults'
        // field-links hydration; tests that want other linking set
        // the field explicitly.
        l.field_links
            .get_or_insert_with(|| vec!["title".to_string(), "filename".to_string()]);
        l
    }

    fn header_of(ctx: &TemplateContext) -> String {
        let TemplateValue::String(s) = ctx
            .get("listing")
            .unwrap()
            .get_path(&["table-header"])
            .expect("listing.table-header present")
            .clone()
        else {
            panic!("table-header not a string");
        };
        s
    }

    fn row_of(ctx: &TemplateContext) -> String {
        let TemplateValue::List(arr) = ctx.get("items").unwrap() else {
            panic!("items not a list");
        };
        let TemplateValue::Map(m) = &arr[0] else {
            panic!("item not a map");
        };
        let TemplateValue::String(s) = m.get("table-row").expect("item table-row present") else {
            panic!("table-row not a string");
        };
        s.clone()
    }

    fn ctx_for(l: &Listing, items: &[ListingItem]) -> TemplateContext {
        build_listing_context(l, items, "posts", &ConfigValue::default())
    }

    #[test]
    fn table_header_uses_display_name_overlay_and_defaults() {
        let mut l = table_listing(&["date", "title", "author"]);
        l.field_display_names
            .insert("title".to_string(), "How To".to_string());
        let ctx = ctx_for(&l, &[item("A")]);
        assert_eq!(header_of(&ctx), "| Date | How To | Author |\n|---|---|---|");
    }

    #[test]
    fn table_header_unknown_field_falls_back_to_raw_name() {
        let l = table_listing(&["title", "status"]);
        let ctx = ctx_for(&l, &[item("A")]);
        assert_eq!(header_of(&ctx), "| Title | status |\n|---|---|");
    }

    // Q1's default display name for `image` is a single space.
    #[test]
    fn table_header_image_field_header_is_blank() {
        let l = table_listing(&["image", "title"]);
        let ctx = ctx_for(&l, &[item("A")]);
        assert_eq!(header_of(&ctx), "|   | Title |\n|---|---|");
    }

    #[test]
    fn table_row_links_title_and_formats_date() {
        let l = table_listing(&["title", "date"]);
        let ctx = ctx_for(&l, &[item("Hello")]);
        assert_eq!(
            row_of(&ctx),
            "| [Hello](foo.qmd){.no-external} | Jan 1, 2026 |"
        );
    }

    #[test]
    fn table_row_missing_value_renders_empty_cell() {
        let l = table_listing(&["title", "date"]);
        let mut i = item("X");
        i.date = None;
        let ctx = ctx_for(&l, &[i]);
        assert_eq!(row_of(&ctx), "| [X](foo.qmd){.no-external} |  |");
    }

    #[test]
    fn table_row_escapes_pipes_and_flattens_newlines() {
        let l = table_listing(&["title"]);
        let mut i = item("A|B\nC");
        i.date = None;
        let ctx = ctx_for(&l, &[i]);
        assert_eq!(row_of(&ctx), "| [A\\|B C](foo.qmd){.no-external} |");
    }

    #[test]
    fn table_row_field_links_empty_unlinks_title() {
        let mut l = table_listing(&["title"]);
        l.field_links = Some(Vec::new());
        let ctx = ctx_for(&l, &[item("Hello")]);
        assert_eq!(row_of(&ctx), "| Hello |");
    }

    #[test]
    fn table_row_filename_linked_by_default() {
        let l = table_listing(&["filename"]);
        let ctx = ctx_for(&l, &[item("X")]);
        assert_eq!(row_of(&ctx), "| [foo.qmd](foo.qmd){.no-external} |");
    }

    #[test]
    fn table_row_dotted_path_reads_nested_extra() {
        use quarto_pandoc_types::ConfigMapEntry;
        use quarto_source_map::SourceInfo;
        let l = table_listing(&["title", "meta.status"]);
        let mut i = item("X");
        let inner = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "status".to_string(),
                key_source: SourceInfo::for_test(),
                value: ConfigValue::new_string("draft", SourceInfo::for_test()),
            }],
            SourceInfo::for_test(),
        );
        i.extra.insert("meta".to_string(), inner);
        let ctx = ctx_for(&l, &[i]);
        assert_eq!(row_of(&ctx), "| [X](foo.qmd){.no-external} | draft |");
    }

    #[test]
    fn table_row_array_extra_field_joins_with_comma() {
        use quarto_pandoc_types::ConfigValue;
        use quarto_source_map::SourceInfo;
        let l = table_listing(&["tags"]);
        let mut i = item("X");
        i.extra.insert(
            "tags".to_string(),
            ConfigValue::new_array(
                vec![
                    ConfigValue::new_string("x", SourceInfo::for_test()),
                    ConfigValue::new_string("y", SourceInfo::for_test()),
                ],
                SourceInfo::for_test(),
            ),
        );
        let ctx = ctx_for(&l, &[i]);
        assert_eq!(row_of(&ctx), "| x, y |");
    }

    #[test]
    fn table_row_authors_and_categories_join_with_comma() {
        let l = table_listing(&["authors", "categories"]);
        let mut i = item("X");
        i.authors = vec!["A".to_string(), "B".to_string()];
        i.categories = vec!["rust".to_string(), "design".to_string()];
        let ctx = ctx_for(&l, &[i]);
        assert_eq!(row_of(&ctx), "| A, B | rust, design |");
    }

    #[test]
    fn table_row_reading_time_uses_min_read_form() {
        let l = table_listing(&["reading-time"]);
        let ctx = ctx_for(&l, &[item("X")]);
        assert_eq!(row_of(&ctx), "| 5 min read |");
    }

    #[test]
    fn table_row_image_field_uses_image_html() {
        let l = table_listing(&["image"]);
        let mut i = item("X");
        i.image = Some("static.png".to_string());
        let ctx = ctx_for(&l, &[i]);
        let row = row_of(&ctx);
        assert!(row.contains("<img"), "expected inline img html: {row}");
        assert!(!row.contains('\n'), "cell must be single-line: {row}");
    }

    // ── Presence filtering of *defaulted* fields (Q1 parity) ──

    #[test]
    fn defaulted_table_fields_presence_filtered_when_no_author() {
        // Table default fields [date, title, author], NOT explicit.
        let mut l = table_listing(&[]);
        assert!(!l.fields_explicit);
        l.field_links = Some(Vec::new());
        let mut i = item("X");
        i.author = None;
        i.authors = Vec::new();
        let ctx = ctx_for(&l, &[i]);
        assert_eq!(header_of(&ctx), "| Date | Title |\n|---|---|");
        assert_eq!(row_of(&ctx), "| Jan 1, 2026 | X |");
        // The binding's fields list + show flags follow the filter.
        let listing_map = ctx.get("listing").unwrap();
        let TemplateValue::List(fields) = listing_map.get_path(&["fields"]).unwrap() else {
            panic!("fields not a list");
        };
        assert_eq!(
            fields,
            &vec![
                TemplateValue::String("date".to_string()),
                TemplateValue::String("title".to_string())
            ]
        );
        let TemplateValue::List(arr) = ctx.get("items").unwrap() else {
            panic!()
        };
        let TemplateValue::Map(m) = &arr[0] else {
            panic!()
        };
        let TemplateValue::Map(show) = m.get("show").unwrap() else {
            panic!("show not a map");
        };
        assert!(!show.contains_key("author"));
    }

    #[test]
    fn explicit_fields_never_presence_filtered() {
        let mut l = table_listing(&["title", "author"]);
        l.field_links = Some(Vec::new());
        let mut i = item("X");
        i.author = None;
        i.authors = Vec::new();
        let ctx = ctx_for(&l, &[i]);
        assert_eq!(header_of(&ctx), "| Title | Author |\n|---|---|");
        assert_eq!(row_of(&ctx), "| X |  |");
    }

    // `image` is exempt from presence filtering (Q1: placeholders
    // can fill it at render time even when no item declares one).
    #[test]
    fn presence_filter_keeps_image_field() {
        let mut l = table_listing(&[]);
        l.fields = vec!["title".to_string(), "image".to_string()];
        l.fields_explicit = false;
        l.field_links = Some(Vec::new());
        let mut i = item("X");
        i.image = None;
        let ctx = ctx_for(&l, &[i]);
        assert_eq!(header_of(&ctx), "| Title |   |\n|---|---|");
    }

    #[test]
    fn project_binding_pulls_website_keys_when_present() {
        use quarto_pandoc_types::ConfigMapEntry;
        use quarto_pandoc_types::ConfigValue;
        use quarto_source_map::SourceInfo;
        let website = ConfigValue::new_map(
            vec![
                ConfigMapEntry {
                    key: "site-url".to_string(),
                    key_source: SourceInfo::for_test(),
                    value: ConfigValue::new_string("https://example.com", SourceInfo::for_test()),
                },
                ConfigMapEntry {
                    key: "title".to_string(),
                    key_source: SourceInfo::for_test(),
                    value: ConfigValue::new_string("My Site", SourceInfo::for_test()),
                },
            ],
            SourceInfo::for_test(),
        );
        let meta = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "website".to_string(),
                key_source: SourceInfo::for_test(),
                value: website,
            }],
            SourceInfo::for_test(),
        );
        let ctx = build_listing_context(&listing(), &[item("A")], "posts", &meta);
        let TemplateValue::Map(p) = ctx.get("project").unwrap() else {
            panic!()
        };
        assert_eq!(
            p.get("site-url"),
            Some(&TemplateValue::String("https://example.com".to_string()))
        );
        assert_eq!(
            p.get("title"),
            Some(&TemplateValue::String("My Site".to_string()))
        );
    }
}
