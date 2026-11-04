/*
 * project/search/config.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * `website.search` resolution (bd-eip0naf4).
 */

//! Resolve the `website.search` configuration into [`SearchOptions`].
//!
//! The surface is Quarto 1's (`website-search.ts::searchOptions`),
//! with the defaults the Quarto 1 *code* applies rather than the ones
//! its docs claim (`limit` 50, `collapse-after` 3):
//!
//! ```yaml
//! website:
//!   search:                 # true / absent → defaults; false → off
//!     location: navbar      # navbar | sidebar (default: navbar when
//!                           #   the site has one, else sidebar)
//!     type: overlay         # overlay | textbox (default by location)
//!     limit: 50             # max matched sections shown
//!     collapse-after: 3     # rows per document before "N more"; false
//!     copy-button: false
//!     keyboard-shortcut: ["f", "/", "s"]
//!     show-item-context: false   # tree | parent | root | bool
//!     merge-navbar-crumbs: true
//!     federate:             # q2 addition — other Pagefind-indexed sites
//!       - url: https://docs.example.org/child/
//!         title: Child Docs
//!         weight: 0.5
//!         bundle: pagefind/ # only for non-q2 sites
//!     algolia: {…}          # opt-in backend (Phase 4)
//!     search-json: true     # q2 addition — write Quarto 1's
//!                           #   `search.json` (default: only when
//!                           #   `algolia` is fully configured)
//! ```
//!
//! Unlike most website keys, the options are read from `website.search`
//! only. A top-level `search:` is a *document* key — Quarto 1's per-page
//! `search: false` opt-out, which removes the page from the index and
//! nothing else — and is answered by [`page_search_enabled`].
//!
//! Unknown keys and out-of-range values are reported as warnings
//! (Q-5-38 / Q-5-39) and the default is used; search configuration
//! never fails a render.

use quarto_config::resolve_website_value;
use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::{ConfigValue, ConfigValueKind};

/// Where the search box is placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchLocation {
    Navbar,
    Sidebar,
}

impl SearchLocation {
    pub fn as_str(self) -> &'static str {
        match self {
            SearchLocation::Navbar => "navbar",
            SearchLocation::Sidebar => "sidebar",
        }
    }
}

/// How the search box behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchType {
    /// An icon button that opens a modal ("detached") panel.
    Overlay,
    /// An inline input with a drop-down panel.
    Textbox,
}

impl SearchType {
    pub fn as_str(self) -> &'static str {
        match self {
            SearchType::Overlay => "overlay",
            SearchType::Textbox => "textbox",
        }
    }
}

/// `show-item-context`: which crumbs to show on a document card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemContext {
    None,
    /// The full trail joined with ` > ` (`true` means this).
    Tree,
    /// The crumb just above the page.
    Parent,
    /// The outermost crumb.
    Root,
}

/// One other site whose index the search box also queries
/// (`federate:`). See the plan's "Federated search across sites".
#[derive(Debug, Clone, PartialEq)]
pub struct FederatedSite {
    /// Site root URL, normally absolute (`https://docs.example.org/child/`).
    pub url: String,
    /// Shown as the result context; falls back to the child's manifest
    /// title at run time.
    pub title: Option<String>,
    /// Pagefind `indexWeight` for the child's results.
    pub weight: Option<f64>,
    /// Bundle path relative to `url` for a site that is not a q2
    /// website (q2 sites are found through the fixed
    /// `site_libs/quarto-search/` location).
    pub bundle: Option<String>,
}

/// `website.search.algolia`: the opt-in Algolia backend (Quarto 1's
/// `algoliaOptions`). Only complete configurations survive resolution:
/// a block missing any of the three required keys is reported (Q-5-40)
/// and dropped, so the page falls back to the built-in index.
#[derive(Debug, Clone, PartialEq)]
pub struct AlgoliaOptions {
    pub application_id: String,
    pub search_only_api_key: String,
    pub index_name: String,
    /// Extra Algolia query parameters, passed through verbatim.
    pub params: Option<ConfigValue>,
    /// Field-name remapping (`href`, `section`, `title`, `text`) for
    /// indexes whose records use other names.
    pub index_fields: Option<ConfigValue>,
    pub analytics_events: bool,
    pub show_logo: bool,
}

/// Resolved `website.search` options.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchOptions {
    pub location: SearchLocation,
    pub kind: SearchType,
    pub limit: u32,
    /// `None` for `collapse-after: false`.
    pub collapse_after: Option<u32>,
    pub copy_button: bool,
    pub keyboard_shortcut: Vec<String>,
    pub show_item_context: ItemContext,
    pub merge_navbar_crumbs: bool,
    pub federate: Vec<FederatedSite>,
    /// The Algolia backend, when fully configured.
    pub algolia: Option<AlgoliaOptions>,
    /// Write Quarto 1's record array to `<output>/search.json`. q2's
    /// own search box never reads it, so by default it is written only
    /// for Algolia (whose index it populates); an explicit
    /// `search-json: true | false` overrides that.
    pub search_json: bool,
}

pub const DEFAULT_LIMIT: u32 = 50;
pub const DEFAULT_COLLAPSE_AFTER: u32 = 3;
pub const DEFAULT_KEYBOARD_SHORTCUT: &[&str] = &["f", "/", "s"];

impl SearchOptions {
    /// Quarto 1's defaults for a site with (`has_navbar`) or without a
    /// navbar.
    pub fn defaults(has_navbar: bool) -> Self {
        let location = if has_navbar {
            SearchLocation::Navbar
        } else {
            SearchLocation::Sidebar
        };
        Self {
            location,
            kind: default_type_for(location),
            limit: DEFAULT_LIMIT,
            collapse_after: Some(DEFAULT_COLLAPSE_AFTER),
            copy_button: false,
            keyboard_shortcut: DEFAULT_KEYBOARD_SHORTCUT
                .iter()
                .map(|s| s.to_string())
                .collect(),
            show_item_context: ItemContext::None,
            merge_navbar_crumbs: true,
            federate: Vec::new(),
            algolia: None,
            search_json: false,
        }
    }

    /// Quarto 1's internal `panel-placement`: `end` for the navbar,
    /// `start` for the sidebar.
    pub fn panel_placement(&self) -> &'static str {
        match self.location {
            SearchLocation::Navbar => "end",
            SearchLocation::Sidebar => "start",
        }
    }
}

fn default_type_for(location: SearchLocation) -> SearchType {
    match location {
        SearchLocation::Navbar => SearchType::Overlay,
        SearchLocation::Sidebar => SearchType::Textbox,
    }
}

/// Does the merged metadata declare a navbar? Mirrors the presence
/// test Quarto 1 uses to pick the default search location.
pub fn has_navbar(meta: &ConfigValue) -> bool {
    match resolve_website_value(meta, "navbar") {
        None => false,
        Some(v) => v.as_bool() != Some(false),
    }
}

/// Quarto 1's per-page opt-out: a document whose frontmatter says
/// `search: false` is left out of the index (the search box on that
/// page is unaffected). Anything else, including absence, is `true`.
pub fn page_search_enabled(meta: &ConfigValue) -> bool {
    meta.get("search").and_then(|v| v.as_bool()) != Some(false)
}

/// Resolve `website.search` from the merged metadata. `None` means
/// search is turned off (`website.search: false`); absent or `true`
/// yields the defaults. Problems are appended to `diagnostics` as
/// warnings and never make this return `None`.
pub fn resolve_website_search(
    meta: &ConfigValue,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Option<SearchOptions> {
    let has_navbar = has_navbar(meta);
    let Some(value) = meta.get_path(&["website", "search"]) else {
        return Some(SearchOptions::defaults(has_navbar));
    };
    if let Some(b) = value.as_bool() {
        return b.then(|| SearchOptions::defaults(has_navbar));
    }
    let Some(entries) = value.as_map_entries() else {
        diagnostics.push(invalid_value(
            "search",
            "`true`, `false`, or a map of options",
            value,
        ));
        return Some(SearchOptions::defaults(has_navbar));
    };

    let mut opts = SearchOptions::defaults(has_navbar);
    // `type` defaults by location, so resolve `location` first and
    // only then apply an explicit `type`.
    let mut explicit_type = None;
    // `search-json` defaults on whether `algolia` resolved, so (like
    // `type`) apply an explicit value only after the loop.
    let mut explicit_search_json = None;
    for entry in entries {
        let key = entry.key.as_str();
        let v = &entry.value;
        match key {
            "location" => match string_of(v).as_deref() {
                Some("navbar") => opts.location = SearchLocation::Navbar,
                Some("sidebar") => opts.location = SearchLocation::Sidebar,
                _ => diagnostics.push(invalid_value(key, "`navbar` or `sidebar`", v)),
            },
            "type" => match string_of(v).as_deref() {
                Some("overlay") => explicit_type = Some(SearchType::Overlay),
                Some("textbox") => explicit_type = Some(SearchType::Textbox),
                _ => diagnostics.push(invalid_value(key, "`overlay` or `textbox`", v)),
            },
            "limit" => match v.as_int() {
                Some(n) if n > 0 => opts.limit = n as u32,
                _ => diagnostics.push(invalid_value(key, "a positive integer", v)),
            },
            "collapse-after" => match (v.as_bool(), v.as_int()) {
                (Some(false), _) => opts.collapse_after = None,
                (Some(true), _) => opts.collapse_after = Some(DEFAULT_COLLAPSE_AFTER),
                (_, Some(n)) if n > 0 => opts.collapse_after = Some(n as u32),
                _ => diagnostics.push(invalid_value(key, "a positive integer or `false`", v)),
            },
            "copy-button" => match v.as_bool() {
                Some(b) => opts.copy_button = b,
                None => diagnostics.push(invalid_value(key, "`true` or `false`", v)),
            },
            "keyboard-shortcut" => {
                if let Some(s) = string_of(v) {
                    opts.keyboard_shortcut = vec![s];
                } else if let Some(items) = v.as_array() {
                    let keys: Vec<String> = items.iter().filter_map(string_of).collect();
                    if keys.len() == items.len() {
                        opts.keyboard_shortcut = keys;
                    } else {
                        diagnostics.push(invalid_value(key, "a key or a list of keys", v));
                    }
                } else {
                    diagnostics.push(invalid_value(key, "a key or a list of keys", v));
                }
            }
            "show-item-context" => match (v.as_bool(), string_of(v).as_deref()) {
                (Some(false), _) => opts.show_item_context = ItemContext::None,
                (Some(true), _) | (_, Some("tree")) => opts.show_item_context = ItemContext::Tree,
                (_, Some("parent")) => opts.show_item_context = ItemContext::Parent,
                (_, Some("root")) => opts.show_item_context = ItemContext::Root,
                _ => diagnostics.push(invalid_value(
                    key,
                    "`tree`, `parent`, `root`, `true`, or `false`",
                    v,
                )),
            },
            "merge-navbar-crumbs" => match v.as_bool() {
                Some(b) => opts.merge_navbar_crumbs = b,
                None => diagnostics.push(invalid_value(key, "`true` or `false`", v)),
            },
            "federate" => match v.as_array() {
                Some(items) => {
                    opts.federate = items
                        .iter()
                        .filter_map(|item| resolve_federated_site(item, diagnostics))
                        .collect();
                }
                None => diagnostics.push(invalid_value(key, "a list of sites", v)),
            },
            "algolia" => {
                if v.as_map_entries().is_some() {
                    opts.algolia = resolve_algolia(v, diagnostics);
                } else {
                    diagnostics.push(invalid_value(key, "a map of Algolia settings", v));
                }
            }
            "search-json" => match v.as_bool() {
                Some(b) => explicit_search_json = Some(b),
                None => diagnostics.push(invalid_value(key, "`true` or `false`", v)),
            },
            // Quarto 1 interface fields nothing ever set; accept
            // silently rather than warn about documented-but-dead keys.
            "panel-placement" | "full-width" | "input-wrapper-width" => {}
            other => diagnostics.push(unknown_key(other, entry)),
        }
    }
    opts.kind = explicit_type.unwrap_or_else(|| default_type_for(opts.location));
    opts.search_json = explicit_search_json.unwrap_or(opts.algolia.is_some());
    Some(opts)
}

fn resolve_algolia(
    value: &ConfigValue,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Option<AlgoliaOptions> {
    let text = |key: &str| {
        value
            .get(key)
            .and_then(string_of)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let flag = |key: &str| value.get(key).and_then(|v| v.as_bool()).unwrap_or(false);
    for entry in value.as_map_entries().unwrap_or_default() {
        match entry.key.as_str() {
            "application-id"
            | "search-only-api-key"
            | "index-name"
            | "params"
            | "index-fields"
            | "analytics-events"
            | "show-logo" => {}
            other => diagnostics.push(unknown_key(&format!("algolia.{other}"), entry)),
        }
    }
    let (Some(application_id), Some(search_only_api_key), Some(index_name)) = (
        text("application-id"),
        text("search-only-api-key"),
        text("index-name"),
    ) else {
        diagnostics.push(
            DiagnosticMessageBuilder::warning("Algolia search is misconfigured")
                .with_code("Q-5-40")
                .problem(
                    "`website.search.algolia` needs `application-id`, `search-only-api-key`, \
                     and `index-name`; the Algolia settings are ignored and the built-in \
                     search index is used instead.",
                )
                .with_location(value.source_info.clone())
                .build(),
        );
        return None;
    };
    Some(AlgoliaOptions {
        application_id,
        search_only_api_key,
        index_name,
        params: value
            .get("params")
            .filter(|v| v.as_map_entries().is_some())
            .cloned(),
        index_fields: value
            .get("index-fields")
            .filter(|v| v.as_map_entries().is_some())
            .cloned(),
        analytics_events: flag("analytics-events"),
        show_logo: flag("show-logo"),
    })
}

fn resolve_federated_site(
    item: &ConfigValue,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Option<FederatedSite> {
    // A bare string is the URL.
    if let Some(url) = string_of(item) {
        return Some(FederatedSite {
            url,
            title: None,
            weight: None,
            bundle: None,
        });
    }
    let Some(entries) = item.as_map_entries() else {
        diagnostics.push(invalid_value(
            "federate",
            "a site URL or a map with `url`",
            item,
        ));
        return None;
    };
    let mut site = FederatedSite {
        url: String::new(),
        title: None,
        weight: None,
        bundle: None,
    };
    for entry in entries {
        let v = &entry.value;
        match entry.key.as_str() {
            "url" => match string_of(v) {
                Some(s) if !s.trim().is_empty() => site.url = s.trim().to_string(),
                _ => diagnostics.push(invalid_value("federate.url", "a site URL", v)),
            },
            "title" => match v.as_plain_text() {
                Some(s) => site.title = Some(s),
                None => diagnostics.push(invalid_value("federate.title", "a string", v)),
            },
            "weight" => match as_f64(v) {
                Some(w) if w > 0.0 => site.weight = Some(w),
                _ => diagnostics.push(invalid_value("federate.weight", "a positive number", v)),
            },
            "bundle" => match string_of(v) {
                Some(s) => site.bundle = Some(s),
                None => diagnostics.push(invalid_value("federate.bundle", "a relative path", v)),
            },
            other => diagnostics.push(unknown_key(&format!("federate.{other}"), entry)),
        }
    }
    if site.url.is_empty() {
        diagnostics.push(
            DiagnosticMessageBuilder::warning("`website.search.federate` entry has no `url`")
                .with_code("Q-5-39")
                .problem("Each federated site needs a `url`; this entry is ignored.")
                .with_location(item.source_info.clone())
                .build(),
        );
        return None;
    }
    Some(site)
}

/// String value of a scalar. Merged metadata carries strings either
/// as plain scalars or, after the markdown-interpretation pass, as
/// parsed inlines; both flatten to the same text.
fn string_of(v: &ConfigValue) -> Option<String> {
    if v.as_map_entries().is_some() || v.as_array().is_some() {
        return None;
    }
    v.as_str().map(str::to_owned).or_else(|| v.as_plain_text())
}

fn as_f64(v: &ConfigValue) -> Option<f64> {
    match &v.value {
        ConfigValueKind::Scalar { yaml, .. } => {
            yaml.as_f64().or_else(|| yaml.as_i64().map(|i| i as f64))
        }
        _ => None,
    }
}

/// Q-5-38: a key under `website.search` that Quarto does not know.
fn unknown_key(key: &str, entry: &quarto_pandoc_types::ConfigMapEntry) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning(format!("Unknown `website.search` option `{key}`"))
        .with_code("Q-5-38")
        .problem("The option is ignored.")
        .add_hint(
            "Known options: location, type, limit, collapse-after, copy-button, \
             keyboard-shortcut, show-item-context, merge-navbar-crumbs, federate, algolia, \
             search-json.",
        )
        .with_location(entry.key_source.clone())
        .build()
}

/// Q-5-39: a `website.search` option with a value of the wrong shape.
fn invalid_value(key: &str, expected: &str, value: &ConfigValue) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning(format!("Invalid value for `website.search.{key}`"))
        .with_code("Q-5-39")
        .problem(format!("Expected {expected}; the default is used instead."))
        .with_location(value.source_info.clone())
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(yaml: &str) -> ConfigValue {
        let parsed = quarto_yaml::parse(yaml).expect("fixture yaml parses");
        let (config, diags) = crate::cell_options::options_to_config(parsed);
        assert!(diags.is_empty(), "fixture diagnostics: {diags:?}");
        config
    }

    fn resolve(yaml: &str) -> (Option<SearchOptions>, Vec<DiagnosticMessage>) {
        let mut diags = Vec::new();
        let opts = resolve_website_search(&meta(yaml), &mut diags);
        (opts, diags)
    }

    #[test]
    fn absent_means_defaults_navbar_overlay_when_navbar_exists() {
        let (opts, diags) = resolve("website:\n  navbar:\n    left: [index.qmd]\n");
        let opts = opts.unwrap();
        assert!(diags.is_empty());
        assert_eq!(opts.location, SearchLocation::Navbar);
        assert_eq!(opts.kind, SearchType::Overlay);
        assert_eq!(opts.limit, 50);
        assert_eq!(opts.collapse_after, Some(3));
        assert!(!opts.copy_button);
        assert_eq!(opts.keyboard_shortcut, vec!["f", "/", "s"]);
        assert_eq!(opts.show_item_context, ItemContext::None);
        assert!(opts.merge_navbar_crumbs);
        assert!(opts.federate.is_empty());
        assert_eq!(opts.panel_placement(), "end");
    }

    #[test]
    fn absent_without_navbar_means_sidebar_textbox() {
        let (opts, _) = resolve("website:\n  title: x\n");
        let opts = opts.unwrap();
        assert_eq!(opts.location, SearchLocation::Sidebar);
        assert_eq!(opts.kind, SearchType::Textbox);
        assert_eq!(opts.panel_placement(), "start");
    }

    #[test]
    fn navbar_false_counts_as_no_navbar() {
        let (opts, _) = resolve("website:\n  navbar: false\n");
        assert_eq!(opts.unwrap().location, SearchLocation::Sidebar);
    }

    #[test]
    fn false_turns_search_off() {
        let (opts, diags) = resolve("website:\n  search: false\n");
        assert!(opts.is_none());
        assert!(diags.is_empty());
    }

    #[test]
    fn true_means_defaults() {
        let (opts, _) = resolve("website:\n  navbar:\n    left: [a.qmd]\n  search: true\n");
        assert_eq!(opts.unwrap().kind, SearchType::Overlay);
    }

    #[test]
    fn explicit_options_override_defaults() {
        let (opts, diags) = resolve(
            "website:\n  navbar:\n    left: [a.qmd]\n  search:\n    location: sidebar\n\
             \x20   limit: 20\n    collapse-after: false\n    copy-button: true\n\
             \x20   keyboard-shortcut: k\n    show-item-context: parent\n\
             \x20   merge-navbar-crumbs: false\n",
        );
        let opts = opts.unwrap();
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(opts.location, SearchLocation::Sidebar);
        assert_eq!(
            opts.kind,
            SearchType::Textbox,
            "type defaults by resolved location"
        );
        assert_eq!(opts.limit, 20);
        assert_eq!(opts.collapse_after, None);
        assert!(opts.copy_button);
        assert_eq!(opts.keyboard_shortcut, vec!["k"]);
        assert_eq!(opts.show_item_context, ItemContext::Parent);
        assert!(!opts.merge_navbar_crumbs);
    }

    #[test]
    fn explicit_type_wins_over_location_default() {
        let (opts, _) = resolve("website:\n  search:\n    location: sidebar\n    type: overlay\n");
        assert_eq!(opts.unwrap().kind, SearchType::Overlay);
    }

    #[test]
    fn show_item_context_true_is_tree() {
        let (opts, _) = resolve("website:\n  search:\n    show-item-context: true\n");
        assert_eq!(opts.unwrap().show_item_context, ItemContext::Tree);
    }

    #[test]
    fn keyboard_shortcut_list() {
        let (opts, _) = resolve("website:\n  search:\n    keyboard-shortcut: [\"/\", \"s\"]\n");
        assert_eq!(opts.unwrap().keyboard_shortcut, vec!["/", "s"]);
    }

    #[test]
    fn top_level_search_is_the_page_opt_out_not_an_option_source() {
        let m = meta("website:\n  search:\n    limit: 10\nsearch: false\n");
        let mut diags = Vec::new();
        let opts = resolve_website_search(&m, &mut diags).expect("site search stays on");
        assert_eq!(opts.limit, 10);
        assert!(!page_search_enabled(&m));
        assert!(page_search_enabled(&meta("website:\n  search: false\n")));
        assert!(page_search_enabled(&meta("title: x\n")));
    }

    #[test]
    fn federate_entries_parse_with_defaults() {
        let (opts, diags) = resolve(
            "website:\n  search:\n    federate:\n\
             \x20     - url: https://docs.example.org/child/\n        title: Child\n        weight: 0.5\n\
             \x20     - https://other.example.org/\n\
             \x20     - url: https://x.example.org/\n        bundle: pagefind/\n",
        );
        let opts = opts.unwrap();
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(opts.federate.len(), 3);
        assert_eq!(opts.federate[0].url, "https://docs.example.org/child/");
        assert_eq!(opts.federate[0].title.as_deref(), Some("Child"));
        assert_eq!(opts.federate[0].weight, Some(0.5));
        assert_eq!(opts.federate[1].url, "https://other.example.org/");
        assert_eq!(opts.federate[1].title, None);
        assert_eq!(opts.federate[2].bundle.as_deref(), Some("pagefind/"));
    }

    #[test]
    fn federate_entry_without_url_is_dropped_with_warning() {
        let (opts, diags) = resolve("website:\n  search:\n    federate:\n      - title: Nope\n");
        assert!(opts.unwrap().federate.is_empty());
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code.as_deref(), Some("Q-5-39"));
    }

    #[test]
    fn unknown_key_warns_and_is_ignored() {
        let (opts, diags) = resolve("website:\n  search:\n    colour: blue\n    limit: 5\n");
        assert_eq!(opts.unwrap().limit, 5);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code.as_deref(), Some("Q-5-38"));
    }

    #[test]
    fn invalid_values_warn_and_fall_back() {
        let (opts, diags) = resolve(
            "website:\n  navbar:\n    left: [a.qmd]\n  search:\n    location: footer\n\
             \x20   type: popup\n    limit: -3\n    collapse-after: maybe\n",
        );
        let opts = opts.unwrap();
        assert_eq!(opts.location, SearchLocation::Navbar);
        assert_eq!(opts.kind, SearchType::Overlay);
        assert_eq!(opts.limit, 50);
        assert_eq!(opts.collapse_after, Some(3));
        assert_eq!(diags.len(), 4);
        assert!(diags.iter().all(|d| d.code.as_deref() == Some("Q-5-39")));
    }

    #[test]
    fn non_bool_non_map_search_value_warns() {
        let (opts, diags) = resolve("website:\n  search: 42\n");
        assert!(opts.is_some());
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn algolia_complete_config_is_resolved() {
        let (opts, diags) = resolve(
            "website:\n  search:\n    algolia:\n      index-name: idx\n      application-id: app\n\
             \x20     search-only-api-key: key\n      analytics-events: true\n      show-logo: true\n\
             \x20     params:\n        hitsPerPage: 5\n      index-fields:\n        text: body\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
        let algolia = opts.unwrap().algolia.expect("algolia resolved");
        assert_eq!(algolia.index_name, "idx");
        assert_eq!(algolia.application_id, "app");
        assert_eq!(algolia.search_only_api_key, "key");
        assert!(algolia.analytics_events);
        assert!(algolia.show_logo);
        assert!(algolia.params.is_some());
        assert_eq!(
            algolia
                .index_fields
                .as_ref()
                .and_then(|f| f.get("text"))
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("body")
        );
    }

    const ALGOLIA: &str = "    algolia:\n      index-name: idx\n      application-id: app\n      search-only-api-key: key\n";

    #[test]
    fn search_json_defaults_off_without_algolia() {
        let (opts, _) = resolve("website:\n  title: x\n");
        assert!(!opts.unwrap().search_json);
        let (opts, _) = resolve("website:\n  search:\n    limit: 5\n");
        assert!(!opts.unwrap().search_json);
    }

    #[test]
    fn search_json_defaults_on_with_complete_algolia() {
        let (opts, diags) = resolve(&format!("website:\n  search:\n{ALGOLIA}"));
        assert!(diags.is_empty(), "{diags:?}");
        assert!(opts.unwrap().search_json);
    }

    #[test]
    fn search_json_stays_off_for_incomplete_algolia() {
        let (opts, diags) = resolve("website:\n  search:\n    algolia:\n      index-name: idx\n");
        assert!(!opts.unwrap().search_json);
        assert_eq!(diags[0].code.as_deref(), Some("Q-5-40"));
    }

    #[test]
    fn explicit_search_json_overrides_default_in_either_key_order() {
        for yaml in [
            format!("website:\n  search:\n    search-json: false\n{ALGOLIA}"),
            format!("website:\n  search:\n{ALGOLIA}    search-json: false\n"),
        ] {
            let (opts, diags) = resolve(&yaml);
            assert!(diags.is_empty(), "{diags:?}");
            let opts = opts.unwrap();
            assert!(opts.algolia.is_some());
            assert!(!opts.search_json, "{yaml}");
        }
        let (opts, _) = resolve("website:\n  search:\n    search-json: true\n");
        assert!(opts.unwrap().search_json);
    }

    #[test]
    fn non_bool_search_json_warns_and_uses_default() {
        let (opts, diags) = resolve(&format!(
            "website:\n  search:\n    search-json: sometimes\n{ALGOLIA}"
        ));
        assert!(opts.unwrap().search_json, "default with algolia");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code.as_deref(), Some("Q-5-39"));
    }

    #[test]
    fn algolia_missing_key_warns_and_is_dropped() {
        let (opts, diags) = resolve(
            "website:\n  search:\n    algolia:\n      index-name: idx\n      application-id: app\n",
        );
        assert!(opts.unwrap().algolia.is_none());
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code.as_deref(), Some("Q-5-40"));
    }
}
