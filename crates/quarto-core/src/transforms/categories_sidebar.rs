/*
 * categories_sidebar.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Categories sidebar transform (L5 — `bd-5vsr`).
//!
//! Aggregates categories across all resolved listings on the host
//! page that have `categories != Disabled`, renders the Q1-shape
//! right-margin sidebar HTML, and writes the result to
//! `meta.rendered.navigation.margin_categories`. The
//! [`crate::template::FULL_HTML_TEMPLATE`] consumes that key.
//!
//! ## Pipeline position
//!
//! Runs after [`super::ListingRenderTransform`] (so the resolved
//! item set is final) and before [`super::TocRenderTransform`] (so
//! both `rendered.navigation.*` keys land before
//! `ApplyTemplateStage` reads them).
//!
//! ## Encoding
//!
//! Pill `data-category` attributes carry Q1's `b64EncodeUnicode`
//! encoding (`btoa(encodeURIComponent(s))`) so the vendored
//! `quarto-listing.js` decoder (`decodeURIComponent(atob(...))`)
//! round-trips correctly for non-ASCII categories. Encoding is
//! reused from [`crate::project::listing::helpers`] to keep the
//! per-item chip and the sidebar pill in lockstep.
//!
//! TODO(bd-754f): revisit the encoding scheme — see the follow-up
//! bd filed at L5 hand-off.
//!
//! TODO(bd-0fd0): when a Lua filter slot lands between generate
//! and render transforms, this aggregation will read the resolved
//! listings via the same boundary the listing-render transform
//! uses today.

use std::collections::BTreeMap;

use base64::Engine;
use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceInfo;

use crate::Result;
use crate::project::listing::ResolvedListing;
use crate::project::listing::config::ListingCategoriesMode;
use crate::render::RenderContext;
use crate::transform::AstTransform;
use crate::transforms::is_feature_disabled;

pub struct CategoriesSidebarTransform;

impl CategoriesSidebarTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CategoriesSidebarTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for CategoriesSidebarTransform {
    fn name(&self) -> &str {
        "categories-sidebar"
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        if is_feature_disabled(&ast.meta, "listing") {
            return Ok(());
        }

        // Honor a pre-set rendered.navigation.margin_categories (Lua
        // filter or earlier stage).
        if ast
            .meta
            .contains_path(&["rendered", "navigation", "margin_categories"])
        {
            return Ok(());
        }

        let outcome = aggregate_categories(&ctx.resolved_listings);
        match outcome {
            AggregateOutcome::SilentSkip => {}
            AggregateOutcome::EnabledButEmpty {
                first_listing_id,
                source_info,
            } => {
                ctx.diagnostics.push(make_diag(
                    "Q-12-12",
                    &format!(
                        "Listing `{}` configures `categories:` but no resolved item has any categories defined; the sidebar is suppressed. Add `categories:` to the listing's content posts, or set `categories: false` to silence this warning.",
                        first_listing_id
                    ),
                    source_info,
                ));
            }
            AggregateOutcome::Rendered(agg) => {
                if agg.mixed_modes {
                    ctx.diagnostics.push(make_diag(
                        "Q-12-11",
                        "Two or more listings on this page declare different `categories:` modes; the first non-disabled mode in declaration order wins for the rendered sidebar.",
                        // The plan recommends pointing at a specific
                        // listing's span here, but with multiple
                        // listings disagreeing the page-level span is
                        // already informative. Refining this is part
                        // of phase 8's diagnostic-source-info polish.
                        SourceInfo::default(),
                    ));
                }
                let html = render_sidebar_html(&agg);
                ast.meta.insert_path(
                    &["rendered", "navigation", "margin_categories"],
                    ConfigValue::new_string(&html, SourceInfo::default()),
                );
            }
        }

        Ok(())
    }
}

fn make_diag(code: &str, message: &str, location: SourceInfo) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning(message)
        .with_code(code)
        .with_location(location)
        .build()
}

// ─────────────────────────────────────────────────────────────────
// Aggregation
// ─────────────────────────────────────────────────────────────────

/// Three-way outcome of aggregating categories across the resolved
/// listings on a host page. See L5 sub-plan §"What
/// `CategoriesSidebarTransform` does".
#[derive(Debug, Clone)]
pub enum AggregateOutcome {
    /// No listing on the page enables categories. Silent skip — no
    /// sidebar markup, no diagnostic.
    SilentSkip,
    /// At least one listing enables categories but no resolved
    /// item carries any. The transform writes no markup and emits
    /// `Q-12-12` pointing at the first such listing's
    /// `categories:` source span.
    EnabledButEmpty {
        first_listing_id: String,
        source_info: SourceInfo,
    },
    /// Aggregate is non-empty; render the sidebar.
    Rendered(AggregatedCategories),
}

/// Per-page aggregate of categories used to render the sidebar.
#[derive(Debug, Clone)]
pub struct AggregatedCategories {
    pub mode: ListingCategoriesMode,
    /// Counts keyed by category name. `BTreeMap` keeps a stable
    /// (case-sensitive) order; the case-insensitive sort happens
    /// at HTML-emit time per Q1's `localeCompare`.
    pub counts: BTreeMap<String, u32>,
    /// Total item count across non-disabled listings — i.e. Q1's
    /// `itemCount` value used as the denominator for cloud-mode
    /// font sizing.
    pub total_items: u32,
    /// Whether at least two non-disabled listings on the page
    /// declared *different* category modes. The transform fires
    /// `Q-12-11` when this is true (page-level "first declaration
    /// wins" rule per the L5 sub-plan).
    pub mixed_modes: bool,
}

/// Aggregate categories across the resolved-listings set of one
/// host page.
///
/// Rules (Q1-parity, L5 sub-plan §"What `CategoriesSidebarTransform`
/// does"):
/// 1. Listings with `categories == Disabled` are skipped.
/// 2. Page-level `mode` is the *first* non-`Disabled` mode in
///    declaration order (`mixed_modes = true` if a later listing
///    declares a different non-`Disabled` mode).
/// 3. Counts accumulate across all non-disabled listings.
/// 4. `total_items` is the sum of `items.len()` over the
///    non-disabled listings (Q1's misnamed `totalCategories`).
pub fn aggregate_categories(resolved: &[ResolvedListing]) -> AggregateOutcome {
    let mut counts: BTreeMap<String, u32> = BTreeMap::new();
    let mut total_items: u32 = 0;
    let mut chosen_mode: Option<ListingCategoriesMode> = None;
    let mut mixed_modes = false;
    let mut any_enabled = false;
    let mut first_enabled_id: Option<String> = None;
    let mut first_enabled_source: SourceInfo = SourceInfo::default();

    for r in resolved {
        if r.listing.categories == ListingCategoriesMode::Disabled {
            continue;
        }
        any_enabled = true;
        if first_enabled_id.is_none() {
            first_enabled_id = Some(r.listing.id.clone());
            first_enabled_source = r.listing.categories_source.clone();
        }
        match chosen_mode {
            None => chosen_mode = Some(r.listing.categories),
            Some(prev) if prev != r.listing.categories => mixed_modes = true,
            _ => {}
        }
        total_items = total_items.saturating_add(r.items.len() as u32);
        for item in &r.items {
            for cat in &item.categories {
                *counts.entry(cat.clone()).or_insert(0) += 1;
            }
        }
    }

    if !any_enabled {
        return AggregateOutcome::SilentSkip;
    }
    if counts.is_empty() {
        return AggregateOutcome::EnabledButEmpty {
            first_listing_id: first_enabled_id.unwrap_or_default(),
            source_info: first_enabled_source,
        };
    }
    AggregateOutcome::Rendered(AggregatedCategories {
        mode: chosen_mode.unwrap_or(ListingCategoriesMode::Default),
        counts,
        total_items,
        mixed_modes,
    })
}

// ─────────────────────────────────────────────────────────────────
// HTML emission
// ─────────────────────────────────────────────────────────────────

const HEADING_TEXT: &str = "Categories";
const ALL_PILL_TEXT: &str = "All";

/// Render the sidebar inner HTML (heading + container). The outer
/// `<div id="quarto-margin-sidebar">` wrapper is the template's job;
/// this function returns just what goes inside.
pub fn render_sidebar_html(agg: &AggregatedCategories) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        r#"<h5 class="quarto-listing-category-title">{}</h5>"#,
        html_escape_text(HEADING_TEXT),
    ));
    s.push('\n');
    s.push_str(&format!(
        r#"<div class="quarto-listing-category {}">"#,
        category_style_class(agg.mode),
    ));
    s.push('\n');

    // "All" pill is only emitted in default mode (matches Q1).
    if agg.mode == ListingCategoriesMode::Default {
        s.push_str(&render_pill_default(ALL_PILL_TEXT, "", agg.total_items));
        s.push('\n');
    }

    // Sort case-insensitively (Q1 uses `localeCompare` on lowercase).
    let mut sorted: Vec<(&String, &u32)> = agg.counts.iter().collect();
    sorted.sort_by(|(a, _), (b, _)| a.to_lowercase().cmp(&b.to_lowercase()));

    for (name, count) in &sorted {
        let count = **count;
        let pill = match agg.mode {
            ListingCategoriesMode::Default => render_pill_default(name, name, count),
            ListingCategoriesMode::Unnumbered => render_pill_unnumbered(name, name),
            ListingCategoriesMode::Cloud => render_pill_cloud(name, name, count, agg.total_items),
            // SAFETY: aggregate_categories never returns Rendered
            // with `Disabled`; emit a default pill if it ever does
            // rather than panic.
            ListingCategoriesMode::Disabled => render_pill_default(name, name, count),
        };
        s.push_str(&pill);
        s.push('\n');
    }

    s.push_str("</div>");
    s
}

fn category_style_class(mode: ListingCategoriesMode) -> &'static str {
    match mode {
        ListingCategoriesMode::Default => "category-default",
        ListingCategoriesMode::Unnumbered => "category-unnumbered",
        ListingCategoriesMode::Cloud => "category-cloud",
        ListingCategoriesMode::Disabled => "category-default",
    }
}

fn render_pill_default(display: &str, value: &str, count: u32) -> String {
    format!(
        r#"<div class="category" data-category="{b64}">{display} <span class="quarto-category-count">({count})</span></div>"#,
        b64 = escape_attr(&b64_encode_unicode(value)),
        display = html_escape_text(display),
        count = count,
    )
}

fn render_pill_unnumbered(display: &str, value: &str) -> String {
    format!(
        r#"<div class="category" data-category="{b64}">{display}</div>"#,
        b64 = escape_attr(&b64_encode_unicode(value)),
        display = html_escape_text(display),
    )
}

fn render_pill_cloud(display: &str, value: &str, count: u32, total: u32) -> String {
    let size = cloud_size(count, total);
    format!(
        r#"<div class="category" data-category="{b64}"><span class="quarto-category-count category-cloud-{size}">{display}</span></div>"#,
        b64 = escape_attr(&b64_encode_unicode(value)),
        display = html_escape_text(display),
        size = size,
    )
}

/// Q1's cloud sizing formula: `Math.ceil((count / total) * 10)`,
/// clamped to `[1, 10]`. Defensive against a zero `total` (which
/// shouldn't happen but produces NaN in JS); we map to size 1.
fn cloud_size(count: u32, total: u32) -> u32 {
    if total == 0 || count == 0 {
        return 1;
    }
    let raw = (count as f64) / (total as f64) * 10.0;
    let ceil = raw.ceil() as i64;
    ceil.clamp(1, 10) as u32
}

// ─────────────────────────────────────────────────────────────────
// Encoding helpers (mirror `helpers::b64_encode_unicode`)
// ─────────────────────────────────────────────────────────────────

/// Mirror Q1's `b64EncodeUnicode` from `core/base64.ts`:
/// `btoa(encodeURIComponent(s))`. Same implementation as
/// [`crate::project::listing::helpers::b64_encode_unicode`]; kept
/// duplicated rather than `pub`-exposed because the encoding
/// review (`bd-754f`) is expected to consolidate or replace this.
fn b64_encode_unicode(s: &str) -> String {
    let percent = encode_uri_component(s);
    base64::engine::general_purpose::STANDARD.encode(percent.as_bytes())
}

fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.as_bytes() {
        let b = *byte;
        let unreserved = b.is_ascii_alphanumeric()
            || matches!(
                b,
                b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')'
            );
        if unreserved {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn html_escape_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::listing::config::{Listing, ListingCategoriesMode, ListingType};
    use crate::project::listing::item::ListingItem;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn item(title: &str, categories: &[&str]) -> ListingItem {
        ListingItem {
            title: title.to_string(),
            subtitle: None,
            description: None,
            author: None,
            authors: vec![],
            date: None,
            date_modified: None,
            categories: categories.iter().map(|s| s.to_string()).collect(),
            image: None,
            image_alt: None,
            image_lazy_loading: None,
            reading_time_minutes: None,
            word_count: None,
            source_path: PathBuf::from(format!("posts/{}.qmd", title)),
            output_href: format!("posts/{}.html", title),
            extra: BTreeMap::new(),
        }
    }

    fn listing_with_mode(id: &str, mode: ListingCategoriesMode) -> Listing {
        Listing {
            id: id.to_string(),
            kind: ListingType::Default,
            categories: mode,
            ..Listing::default()
        }
    }

    fn rl(id: &str, mode: ListingCategoriesMode, items: Vec<ListingItem>) -> ResolvedListing {
        ResolvedListing {
            listing: listing_with_mode(id, mode),
            items,
        }
    }

    // L5 plan §"Tests" #5
    #[test]
    fn aggregate_returns_silent_skip_when_no_listings() {
        match aggregate_categories(&[]) {
            AggregateOutcome::SilentSkip => {}
            other => panic!("expected SilentSkip, got {other:?}"),
        }
    }

    // L5 plan §"Tests" #6
    #[test]
    fn aggregate_skips_disabled_listings() {
        let resolved = vec![rl(
            "x",
            ListingCategoriesMode::Disabled,
            vec![item("a", &["rust"])],
        )];
        match aggregate_categories(&resolved) {
            AggregateOutcome::SilentSkip => {}
            other => panic!("expected SilentSkip, got {other:?}"),
        }
    }

    // L5 plan §"Tests" #7
    #[test]
    fn aggregate_counts_categories() {
        let resolved = vec![rl(
            "x",
            ListingCategoriesMode::Default,
            vec![item("p1", &["a"]), item("p2", &["a"]), item("p3", &["b"])],
        )];
        match aggregate_categories(&resolved) {
            AggregateOutcome::Rendered(agg) => {
                assert_eq!(agg.mode, ListingCategoriesMode::Default);
                assert_eq!(agg.counts.get("a").copied(), Some(2));
                assert_eq!(agg.counts.get("b").copied(), Some(1));
                assert_eq!(agg.total_items, 3);
                assert!(!agg.mixed_modes);
            }
            other => panic!("expected Rendered, got {other:?}"),
        }
    }

    // L5 plan §"Tests" #8
    #[test]
    fn aggregate_unions_across_listings() {
        let resolved = vec![
            rl(
                "A",
                ListingCategoriesMode::Default,
                vec![item("p1", &["a"]), item("p2", &["b"])],
            ),
            rl(
                "B",
                ListingCategoriesMode::Default,
                vec![item("p3", &["b"]), item("p4", &["c"])],
            ),
        ];
        match aggregate_categories(&resolved) {
            AggregateOutcome::Rendered(agg) => {
                assert_eq!(agg.counts.get("a").copied(), Some(1));
                assert_eq!(agg.counts.get("b").copied(), Some(2));
                assert_eq!(agg.counts.get("c").copied(), Some(1));
                assert_eq!(agg.total_items, 4);
            }
            other => panic!("expected Rendered, got {other:?}"),
        }
    }

    // L5 plan §"Tests" #9
    #[test]
    fn aggregate_inherits_first_non_disabled_mode_and_flags_mix() {
        let resolved = vec![
            rl("A", ListingCategoriesMode::Cloud, vec![item("p1", &["a"])]),
            rl(
                "B",
                ListingCategoriesMode::Default,
                vec![item("p2", &["b"])],
            ),
        ];
        match aggregate_categories(&resolved) {
            AggregateOutcome::Rendered(agg) => {
                assert_eq!(agg.mode, ListingCategoriesMode::Cloud);
                assert!(agg.mixed_modes);
            }
            other => panic!("expected Rendered, got {other:?}"),
        }
    }

    // L5 plan §"Tests" #10
    #[test]
    fn aggregate_drops_items_with_no_categories_but_keeps_total_item_count() {
        let resolved = vec![rl(
            "x",
            ListingCategoriesMode::Default,
            vec![
                item("p1", &["a"]),
                item("p2", &[]),
                item("p3", &[]),
                item("p4", &["b"]),
                item("p5", &[]),
            ],
        )];
        match aggregate_categories(&resolved) {
            AggregateOutcome::Rendered(agg) => {
                // Only 2 items contributed counts.
                assert_eq!(agg.counts.values().sum::<u32>(), 2);
                // total_items is the listing's full item count.
                assert_eq!(agg.total_items, 5);
            }
            other => panic!("expected Rendered, got {other:?}"),
        }
    }

    // L5 plan §"Tests" #11 (case-insensitive sort happens at emit
    // time; here we just lock the BTreeMap-stable ordering.)
    #[test]
    fn aggregate_btree_map_preserves_case_sensitive_keys() {
        let resolved = vec![rl(
            "x",
            ListingCategoriesMode::Default,
            vec![item("p1", &["B"]), item("p2", &["a"])],
        )];
        match aggregate_categories(&resolved) {
            AggregateOutcome::Rendered(agg) => {
                let keys: Vec<&str> = agg.counts.keys().map(String::as_str).collect();
                // BTreeMap order is case-sensitive: 'B' < 'a' in ASCII.
                assert_eq!(keys, vec!["B", "a"]);
            }
            other => panic!("expected Rendered, got {other:?}"),
        }
    }

    #[test]
    fn aggregate_returns_enabled_but_empty_when_items_have_no_categories() {
        let resolved = vec![rl(
            "x",
            ListingCategoriesMode::Default,
            vec![item("p1", &[]), item("p2", &[])],
        )];
        match aggregate_categories(&resolved) {
            AggregateOutcome::EnabledButEmpty {
                first_listing_id, ..
            } => {
                assert_eq!(first_listing_id, "x");
            }
            other => panic!("expected EnabledButEmpty, got {other:?}"),
        }
    }

    // ─────────── Sidebar HTML emission tests ───────────

    fn agg_with(
        mode: ListingCategoriesMode,
        counts: &[(&str, u32)],
        total: u32,
    ) -> AggregatedCategories {
        let mut m = BTreeMap::new();
        for (k, v) in counts {
            m.insert((*k).to_string(), *v);
        }
        AggregatedCategories {
            mode,
            counts: m,
            total_items: total,
            mixed_modes: false,
        }
    }

    // L5 plan §"Tests" #12
    #[test]
    fn emits_default_mode_with_all_pill() {
        let agg = agg_with(
            ListingCategoriesMode::Default,
            &[("rust", 2), ("design", 1)],
            3,
        );
        let html = render_sidebar_html(&agg);
        assert!(html.contains(r#"<h5 class="quarto-listing-category-title">Categories</h5>"#));
        assert!(html.contains(r#"class="quarto-listing-category category-default""#));
        // "All" pill with total count
        assert!(html.contains(">All "));
        assert!(html.contains(r#"<span class="quarto-category-count">(3)</span>"#));
        // Per-category pills with their counts
        assert!(html.contains(r#"<span class="quarto-category-count">(2)</span>"#));
        assert!(html.contains(r#"<span class="quarto-category-count">(1)</span>"#));
        assert!(html.contains(">rust "));
        assert!(html.contains(">design "));
    }

    // L5 plan §"Tests" #13
    #[test]
    fn emits_unnumbered_mode_no_counts_no_all() {
        let agg = agg_with(
            ListingCategoriesMode::Unnumbered,
            &[("rust", 2), ("design", 1)],
            3,
        );
        let html = render_sidebar_html(&agg);
        assert!(html.contains(r#"class="quarto-listing-category category-unnumbered""#));
        // No count spans
        assert!(!html.contains(r#"<span class="quarto-category-count""#));
        // No "All" pill
        assert!(!html.contains(">All<"));
        assert!(!html.contains(">All "));
    }

    // L5 plan §"Tests" #14
    #[test]
    fn emits_cloud_mode_with_size_classes() {
        let agg = agg_with(ListingCategoriesMode::Cloud, &[("a", 5), ("b", 1)], 6);
        let html = render_sidebar_html(&agg);
        assert!(html.contains(r#"class="quarto-listing-category category-cloud""#));
        // ceil(5/6 * 10) = 9
        assert!(
            html.contains(r#"class="quarto-category-count category-cloud-9">a</span>"#),
            "missing category-cloud-9 for a; html: {html}"
        );
        // ceil(1/6 * 10) = 2
        assert!(
            html.contains(r#"class="quarto-category-count category-cloud-2">b</span>"#),
            "missing category-cloud-2 for b; html: {html}"
        );
        // No "All" pill in cloud mode
        assert!(!html.contains(">All "));
    }

    // L5 plan §"Tests" #15
    #[test]
    fn cloud_mode_clamps_to_one_minimum() {
        // count=0 (defensive); total=10 → 0/10 = 0 → clamp to 1.
        assert_eq!(cloud_size(0, 10), 1);
        // tiny ratio: 1/100 * 10 = 0.1 → ceil = 1
        assert_eq!(cloud_size(1, 100), 1);
        // ratio > 1: cloud_size(20, 10) → ceil(20.0) = 20 → clamp to 10.
        assert_eq!(cloud_size(20, 10), 10);
    }

    // L5 plan §"Tests" #16
    #[test]
    fn heading_is_categories() {
        let agg = agg_with(ListingCategoriesMode::Default, &[("a", 1)], 1);
        let html = render_sidebar_html(&agg);
        assert!(html.contains(r#"<h5 class="quarto-listing-category-title">Categories</h5>"#));
    }

    // L5 plan §"Tests" #17
    #[test]
    fn pills_b64_encode_data_category() {
        let agg = agg_with(ListingCategoriesMode::Default, &[("café", 1)], 1);
        let html = render_sidebar_html(&agg);
        // "café" -> encodeURIComponent -> "caf%C3%A9" -> btoa -> Y2FmJUMzJUE5
        assert!(
            html.contains(r#"data-category="Y2FmJUMzJUE5""#),
            "expected b64 of percent-encoded UTF-8; got: {html}"
        );
        // The "All" pill carries data-category="" (b64 of empty == empty).
        assert!(
            html.contains(r#"data-category="">All "#),
            "expected empty data-category on All pill; got: {html}"
        );
    }

    // L5 plan §"Tests" #18
    #[test]
    fn pills_html_escape_display_text() {
        let agg = agg_with(ListingCategoriesMode::Default, &[("<bold>", 1)], 1);
        let html = render_sidebar_html(&agg);
        assert!(html.contains("&lt;bold&gt;"));
        // No raw `<bold>` substring anywhere in the chip text.
        // (The container tags `<div class=...>` use `<` of course; the
        // invariant is that the user category text is escaped.)
        assert!(
            !html.contains(">bold<"),
            "raw category text leaked unescaped; html: {html}"
        );
    }

    // L5 plan §"Tests" #19
    #[test]
    fn pills_sorted_case_insensitive() {
        let agg = agg_with(
            ListingCategoriesMode::Default,
            &[("Zebra", 1), ("apple", 1)],
            2,
        );
        let html = render_sidebar_html(&agg);
        let zebra = html.find(">Zebra ").expect("Zebra not found");
        let apple = html.find(">apple ").expect("apple not found");
        assert!(
            apple < zebra,
            "expected case-insensitive order (apple before Zebra); html: {html}"
        );
    }

    // ─────────── Transform-level tests (#20-25) ───────────

    use crate::format::Format;
    use crate::project::index::ProjectIndex;
    use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use crate::render::BinaryDependencies;
    use std::sync::Arc;

    fn make_project() -> ProjectContext {
        ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: false,
            files: vec![DocumentInfo::from_path("/project/posts/index.qmd")],
            output_dir: PathBuf::from("/project/_site"),
        }
    }

    /// Run the transform once and return (mutated AST, diagnostics).
    /// The synthetic project / format are minimal — the transform
    /// only reads `ctx.resolved_listings` + `ast.meta`.
    async fn run_transform(
        ast: Pandoc,
        resolved: Vec<ResolvedListing>,
    ) -> (Pandoc, Vec<quarto_error_reporting::DiagnosticMessage>) {
        let mut ast = ast;
        let project = make_project();
        let doc = DocumentInfo::from_path("/project/posts/index.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let index = Arc::new(ProjectIndex::new(Vec::<
            crate::document_profile::DocumentProfile,
        >::new()));
        let mut ctx =
            RenderContext::new(&project, &doc, &format, &binaries).with_project_index(index);
        ctx.resolved_listings = resolved;
        CategoriesSidebarTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        (ast, ctx.diagnostics)
    }

    fn empty_pandoc() -> Pandoc {
        Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![],
        }
    }

    fn read_margin_categories(ast: &Pandoc) -> Option<String> {
        ast.meta
            .get_path(&["rendered", "navigation", "margin_categories"])
            .and_then(|v| v.as_plain_text())
    }

    // L5 plan §"Tests" #20
    #[tokio::test]
    async fn transform_no_op_when_listing_disabled_in_meta() {
        use quarto_pandoc_types::ConfigMapEntry;
        let mut ast = empty_pandoc();
        ast.meta = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "listing".to_string(),
                key_source: SourceInfo::default(),
                value: ConfigValue::new_bool(false, SourceInfo::default()),
            }],
            SourceInfo::default(),
        );
        let resolved = vec![rl(
            "x",
            ListingCategoriesMode::Default,
            vec![item("a", &["rust"])],
        )];
        let (ast, _) = run_transform(ast, resolved).await;
        assert!(read_margin_categories(&ast).is_none());
    }

    // L5 plan §"Tests" #21
    #[tokio::test]
    async fn transform_no_op_when_already_set_in_meta() {
        let mut ast = empty_pandoc();
        ast.meta.insert_path(
            &["rendered", "navigation", "margin_categories"],
            ConfigValue::new_string("preset html", SourceInfo::default()),
        );
        let resolved = vec![rl(
            "x",
            ListingCategoriesMode::Default,
            vec![item("a", &["rust"])],
        )];
        let (ast, _) = run_transform(ast, resolved).await;
        assert_eq!(
            read_margin_categories(&ast).as_deref(),
            Some("preset html"),
            "transform must not overwrite a pre-set value"
        );
    }

    // L5 plan §"Tests" #22
    #[tokio::test]
    async fn transform_no_op_with_empty_resolved_listings() {
        let (ast, _) = run_transform(empty_pandoc(), vec![]).await;
        assert!(read_margin_categories(&ast).is_none());
    }

    // L5 plan §"Tests" #23
    #[tokio::test]
    async fn transform_no_op_when_all_listings_have_categories_disabled() {
        let resolved = vec![rl(
            "x",
            ListingCategoriesMode::Disabled,
            vec![item("a", &["rust"])],
        )];
        let (ast, diags) = run_transform(empty_pandoc(), resolved).await;
        assert!(read_margin_categories(&ast).is_none());
        // No diagnostic when categories are explicitly disabled.
        assert!(diags.is_empty(), "diags: {:?}", diags);
    }

    // L5 plan §"Tests" #24
    #[tokio::test]
    async fn transform_writes_html_to_meta_path() {
        let resolved = vec![rl(
            "x",
            ListingCategoriesMode::Default,
            vec![item("a", &["rust", "design"]), item("b", &["rust"])],
        )];
        let (ast, _) = run_transform(empty_pandoc(), resolved).await;
        let html = read_margin_categories(&ast).expect("margin_categories present");
        assert!(html.contains(r#"<h5 class="quarto-listing-category-title">Categories</h5>"#));
        assert!(html.contains(r#"class="quarto-listing-category category-default""#));
        // Pills for both categories.
        assert!(html.contains(">rust "));
        assert!(html.contains(">design "));
    }

    // L5 plan §"Tests" #25
    #[tokio::test]
    async fn transform_does_not_consume_resolved_listings() {
        let project = make_project();
        let doc = DocumentInfo::from_path("/project/posts/index.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let index = Arc::new(ProjectIndex::new(Vec::<
            crate::document_profile::DocumentProfile,
        >::new()));
        let mut ctx =
            RenderContext::new(&project, &doc, &format, &binaries).with_project_index(index);
        ctx.resolved_listings = vec![rl(
            "x",
            ListingCategoriesMode::Default,
            vec![item("a", &["rust"])],
        )];

        let mut ast = empty_pandoc();
        CategoriesSidebarTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        // Downstream transforms still need to be able to read it.
        assert_eq!(ctx.resolved_listings.len(), 1);
        assert_eq!(ctx.resolved_listings[0].listing.id, "x");
    }

    // L5 plan §"Tests" #23b — Q-12-12 fires when a listing has
    // `categories: <mode>` set but no item carries any.
    #[tokio::test]
    async fn transform_emits_q_12_12_when_enabled_but_no_item_has_categories() {
        let resolved = vec![rl(
            "x",
            ListingCategoriesMode::Default,
            vec![item("a", &[]), item("b", &[])],
        )];
        let (ast, diags) = run_transform(empty_pandoc(), resolved).await;
        assert!(
            read_margin_categories(&ast).is_none(),
            "no sidebar when no items carry categories"
        );
        let q1212 = diags.iter().find(|d| d.code.as_deref() == Some("Q-12-12"));
        assert!(
            q1212.is_some(),
            "expected Q-12-12 diagnostic; got: {:?}",
            diags
        );
    }

    // L5 plan §"Tests" #23c — explicit `categories: false` (Disabled)
    // produces no sidebar AND no diagnostic.
    #[tokio::test]
    async fn transform_no_q_12_12_when_categories_explicitly_false() {
        // Even if items happen to carry categories, an explicit
        // Disabled mode means we emit no sidebar and no diagnostic.
        let resolved = vec![rl(
            "x",
            ListingCategoriesMode::Disabled,
            vec![item("a", &["rust"])],
        )];
        let (ast, diags) = run_transform(empty_pandoc(), resolved).await;
        assert!(read_margin_categories(&ast).is_none());
        assert!(
            !diags.iter().any(|d| d.code.as_deref() == Some("Q-12-12")),
            "no Q-12-12 when categories is explicitly false"
        );
    }

    // L5 plan §"Tests" #11 — Q-12-11 fires on mixed modes.
    #[tokio::test]
    async fn transform_emits_q_12_11_when_mixed_modes() {
        let resolved = vec![
            rl("A", ListingCategoriesMode::Cloud, vec![item("p1", &["a"])]),
            rl(
                "B",
                ListingCategoriesMode::Default,
                vec![item("p2", &["b"])],
            ),
        ];
        let (ast, diags) = run_transform(empty_pandoc(), resolved).await;
        // Sidebar is still written (first-mode-wins).
        assert!(read_margin_categories(&ast).is_some());
        let q1211 = diags.iter().find(|d| d.code.as_deref() == Some("Q-12-11"));
        assert!(
            q1211.is_some(),
            "expected Q-12-11 diagnostic; got: {:?}",
            diags
        );
    }

    // Single-listing happy path emits no diagnostic.
    #[tokio::test]
    async fn transform_no_diagnostic_on_single_listing_happy_path() {
        let resolved = vec![rl(
            "x",
            ListingCategoriesMode::Default,
            vec![item("a", &["rust"])],
        )];
        let (_ast, diags) = run_transform(empty_pandoc(), resolved).await;
        assert!(
            !diags.iter().any(|d| d
                .code
                .as_deref()
                .map(|c| c.starts_with("Q-12"))
                .unwrap_or(false)),
            "no Q-12 diagnostics on happy path; got: {:?}",
            diags
        );
    }
}
