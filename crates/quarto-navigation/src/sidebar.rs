/*
 * sidebar.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Sidebar data model and YAML resolution.
//!
//! A sidebar is a list of entries shown in a project's left column
//! (or right; template placement is the caller's concern). Entries can
//! be leaf links, nested collapsible sections, separators, or
//! `auto:` directives that expand against a
//! [`ProjectIndex`](https://docs.rs/quarto-core) at Generate time.
//!
//! ## Staged resolution
//!
//! Parsing (this module) is **format-agnostic**. Links carry the
//! author's project-relative source path (e.g. `about.qmd`); the
//! format-specific `.qmd → .html` rewrite happens later, in the
//! HTML Render transform. See
//! `claude-notes/plans/2026-04-24-websites-phase-2.md`
//! §"Decision 7/8".
//!
//! ## Config shapes accepted
//!
//! The top-level `website.sidebar:` may be either a single sidebar
//! object or an array of sidebars. Use [`Sidebar::parse_list_from_config`]
//! to handle both uniformly.
//!
//! Within a sidebar, each item in `contents:` may be:
//!
//! - A bare string path (`- about.qmd`) — a leaf [`SidebarEntry::Link`].
//! - A separator (`- "---"` or three-or-more dashes) —
//!   [`SidebarEntry::Separator`].
//! - An object with `section:` + `contents:` keys —
//!   [`SidebarEntry::Section`].
//! - An object with `auto:` — [`SidebarEntry::Auto`].
//! - An object with `text:` but no `href`/`contents` — plain
//!   [`SidebarEntry::Heading`].
//! - An object with any of `href`, `text`, `icon`, … — leaf
//!   [`SidebarEntry::Link`].

use quarto_pandoc_types::ConfigMapEntry;
use quarto_pandoc_types::config_value::ConfigValue;
use quarto_source_map::{By, SourceInfo};
use yaml_rust2::Yaml;

use crate::item::NavigationItem;

/// Display style for a sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SidebarStyle {
    /// Always-visible column; participates in the page grid.
    Docked,
    /// Floats beside the content on wide viewports; overlay on narrow.
    #[default]
    Floating,
}

impl SidebarStyle {
    pub fn as_str(&self) -> &'static str {
        match self {
            SidebarStyle::Docked => "docked",
            SidebarStyle::Floating => "floating",
        }
    }

    // Deliberate Option-returning parser; `FromStr` would force a `Result`/`Err`
    // type this lightweight enum doesn't need.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "docked" => Some(Self::Docked),
            "floating" => Some(Self::Floating),
            _ => None,
        }
    }
}

/// Title treatment for a sidebar.
///
/// Mirrors [`crate::navbar::NavbarTitle`]. The transform layer
/// (`SidebarGenerateTransform`) is responsible for resolving
/// [`SidebarTitle::Default`] into a concrete title using
/// `website.title`; if no website title is available the variant
/// stays `Default` and the renderer emits no header.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum SidebarTitle {
    /// No explicit title field; the resolver may substitute the
    /// website title before render. If still `Default` at render
    /// time, no header is emitted.
    #[default]
    Default,
    /// Explicitly suppressed via `title: false`. Never substituted.
    Hidden,
    /// Title text. Preserved as a `ConfigValue` so document-context
    /// markdown survives.
    Text(ConfigValue),
}

/// An `auto:` directive expands into concrete sidebar entries at
/// Generate time, by consulting the project's set of discovered
/// documents.
///
/// Paths are **globs**, matched with q2's shared glob semantics
/// (`claude-notes/designs/glob-semantics.md`) against project-relative
/// document paths: `*` covers one directory level, `**` crosses
/// levels, a bare directory name matches everything beneath it, and a
/// leading `!` excludes. Before bd-mt7a6uc4 these were not globs at
/// all — trailing wildcards were stripped and what remained was
/// prefix-matched — so `docs/*` used to mean what `docs` means here.
#[derive(Debug, Clone, PartialEq)]
pub enum AutoSpec {
    /// `auto: true` — every non-draft, non-index document in the
    /// project.
    All,
    /// A single glob: `auto: docs` (everything beneath `docs/`),
    /// `auto: "docs/*.qmd"` (documents directly in `docs/`), or
    /// `auto: "docs/**/*.qmd"` (documents anywhere beneath it).
    Path(String),
    /// `auto: ["docs", "!docs/internal"]` — the union of several
    /// globs, minus any the `!` entries exclude.
    Paths(Vec<String>),
}

impl AutoSpec {
    /// Parse the value of an `auto:` key. Returns `None` if the shape
    /// is unrecognised (e.g. `auto: false` or `auto: 42`).
    pub fn from_config_value(cv: &ConfigValue) -> Option<Self> {
        if cv.as_bool() == Some(true) {
            return Some(AutoSpec::All);
        }
        if cv.as_bool() == Some(false) {
            return None;
        }
        if let Some(arr) = cv.as_array() {
            let paths: Vec<String> = arr.iter().filter_map(|v| v.as_plain_text()).collect();
            if paths.is_empty() {
                return None;
            }
            return Some(AutoSpec::Paths(paths));
        }
        if let Some(s) = cv.as_plain_text() {
            return Some(AutoSpec::Path(s));
        }
        None
    }

    pub fn to_config_value(&self) -> ConfigValue {
        let info = SourceInfo::generated(By::programmatic_config());
        match self {
            AutoSpec::All => ConfigValue::new_bool(true, info),
            AutoSpec::Path(p) => ConfigValue::new_string(p, info),
            AutoSpec::Paths(ps) => {
                let values: Vec<ConfigValue> = ps
                    .iter()
                    .map(|p| {
                        ConfigValue::new_string(p, SourceInfo::generated(By::programmatic_config()))
                    })
                    .collect();
                ConfigValue::new_array(values, info)
            }
        }
    }
}

/// A single entry in a sidebar's `contents:`.
#[derive(Debug, Clone, PartialEq)]
pub enum SidebarEntry {
    /// A leaf link. The current-page highlight lives on
    /// [`NavigationItem::active`], set by the Generate step when the
    /// entry's `href` matches the current page's source path.
    Link { item: NavigationItem },

    /// A collapsible section. `expanded: true` is the rendered state
    /// (including both the YAML `expanded:` override and the
    /// active-state expansion path).
    Section {
        /// Display text from `section:` or `text:`. `None` when the
        /// section is keyed on an href alone.
        text: Option<ConfigValue>,
        /// Optional link for the section header row.
        href: Option<String>,
        /// `SourceInfo` of the YAML scalar that produced `href`.
        /// bd-qor9a — paired with `href` so the resolver knows which
        /// YAML file the path was authored in. `SourceInfo::generated(By::programmatic_config())`
        /// for programmatically-constructed sections.
        href_source: SourceInfo,
        /// Stable anchor id for the collapsible group. Auto-generated
        /// from the text or href when absent.
        id: Option<String>,
        /// Nested entries.
        contents: Vec<SidebarEntry>,
        /// Whether the section is expanded in the rendered output.
        expanded: bool,
    },

    /// A visual divider rendered as `<hr>`.
    Separator,

    /// Plain label (no link, no children, no icon).
    Heading(ConfigValue),

    /// An `auto:` directive that hasn't been expanded yet. Replaced
    /// by concrete entries during `SidebarGenerateTransform`.
    Auto(AutoSpec),
}

impl SidebarEntry {
    /// Parse a single entry from a `ConfigValue`. Returns `None` when
    /// the shape is unrecognisable.
    pub fn from_config_value(cv: &ConfigValue) -> Option<Self> {
        // Bare string: either a separator or a path. `cv.source_info`
        // identifies the YAML scalar so a downstream resolver can
        // resolve the path against the source file's directory
        // (bd-qor9a).
        if let Some(s) = cv.as_plain_text() {
            return Some(Self::from_plain_string(&s, cv.source_info.clone()));
        }

        // Object form.
        // `auto:` takes precedence over anything else — Q1's sidebar
        // items short-circuit on auto.
        if let Some(auto) = cv.get("auto") {
            return AutoSpec::from_config_value(auto).map(SidebarEntry::Auto);
        }

        // `section:` creates a section. Its value is the section text.
        // Contents may be an array, the string "auto", or absent.
        let section_text = cv.get("section").cloned();
        let has_contents = cv.get("contents").is_some();

        // A section is a Section entry when it either has a `section:`
        // key, or has `contents:`. (Q1 lets sections go text-less when
        // they only have `href:` + `contents:`.)
        if section_text.is_some() || has_contents {
            let text = section_text.filter(|v| v.as_plain_text().is_some());
            let (href, href_source) = cv
                .get("href")
                .and_then(|v| v.as_plain_text().map(|s| (s, v.source_info.clone())))
                .or_else(|| {
                    cv.get("file")
                        .and_then(|v| v.as_plain_text().map(|s| (s, v.source_info.clone())))
                })
                .map_or_else(
                    || (None, SourceInfo::generated(By::programmatic_config())),
                    |(s, info)| (Some(s), info),
                );
            let id = cv.get("id").and_then(|v| v.as_plain_text());
            let contents = parse_contents(cv.get("contents"));
            let expanded = cv
                .get("expanded")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            return Some(SidebarEntry::Section {
                text,
                href,
                href_source,
                id,
                contents,
                expanded,
            });
        }

        // `text:` without `href:`/`icon:`/`menu:` — plain heading.
        if let Some(text_cv) = cv.get("text") {
            let has_link_bits = cv.get("href").is_some()
                || cv.get("file").is_some()
                || cv.get("icon").is_some()
                || cv.get("menu").is_some();
            if !has_link_bits {
                return Some(SidebarEntry::Heading(text_cv.clone()));
            }
        }

        // Otherwise, try to parse as a leaf link. `NavigationItem` now
        // roundtrips its own `active` field, so there's nothing extra
        // to splice in here.
        NavigationItem::from_config_value(cv).map(|item| SidebarEntry::Link { item })
    }

    /// Classify a bare string: a run of 3+ dashes is a separator; any
    /// other text is taken as an href.
    ///
    /// `source` is the `SourceInfo` of the bare YAML scalar that
    /// produced `s`. It travels onto `NavigationItem.href_source` so
    /// bd-qor9a's path resolver knows which file the href was
    /// authored in.
    fn from_plain_string(s: &str, source: SourceInfo) -> Self {
        if is_separator_string(s) {
            return SidebarEntry::Separator;
        }
        SidebarEntry::Link {
            item: NavigationItem {
                href: Some(s.to_string()),
                href_source: source,
                ..NavigationItem::default()
            },
        }
    }

    /// Serialize back to a `ConfigValue` — round-trips the parse.
    pub fn to_config_value(&self) -> ConfigValue {
        let info = SourceInfo::generated(By::programmatic_config());
        match self {
            SidebarEntry::Link { item } => item.to_config_value(),
            SidebarEntry::Section {
                text,
                href,
                href_source,
                id,
                contents,
                expanded,
            } => {
                let mut entries: Vec<ConfigMapEntry> = Vec::new();
                if let Some(text) = text {
                    entries.push(ConfigMapEntry {
                        key: "section".to_string(),
                        key_source: info.clone(),
                        value: text.clone(),
                    });
                }
                if let Some(href) = href {
                    // Round-trip the href's SourceInfo so the
                    // Generate → Render handoff preserves the original
                    // YAML location (bd-qor9a).
                    entries.push(ConfigMapEntry {
                        key: "href".to_string(),
                        key_source: info.clone(),
                        value: ConfigValue::new_string(href, href_source.clone()),
                    });
                }
                if let Some(id) = id {
                    entries.push(ConfigMapEntry {
                        key: "id".to_string(),
                        key_source: info.clone(),
                        value: ConfigValue::new_string(id, info.clone()),
                    });
                }
                if !contents.is_empty() {
                    let values: Vec<ConfigValue> =
                        contents.iter().map(SidebarEntry::to_config_value).collect();
                    entries.push(ConfigMapEntry {
                        key: "contents".to_string(),
                        key_source: info.clone(),
                        value: ConfigValue::new_array(values, info.clone()),
                    });
                }
                if *expanded {
                    entries.push(ConfigMapEntry {
                        key: "expanded".to_string(),
                        key_source: info.clone(),
                        value: ConfigValue::new_bool(true, info.clone()),
                    });
                }
                ConfigValue::new_map(entries, info)
            }
            SidebarEntry::Separator => ConfigValue::new_string("---", info),
            SidebarEntry::Heading(text) => {
                let entries = vec![ConfigMapEntry {
                    key: "text".to_string(),
                    key_source: info.clone(),
                    value: text.clone(),
                }];
                ConfigValue::new_map(entries, info)
            }
            SidebarEntry::Auto(spec) => {
                let entries = vec![ConfigMapEntry {
                    key: "auto".to_string(),
                    key_source: info.clone(),
                    value: spec.to_config_value(),
                }];
                ConfigValue::new_map(entries, info)
            }
        }
    }
}

/// Fully resolved sidebar configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct Sidebar {
    pub id: Option<String>,
    pub title: SidebarTitle,
    pub subtitle: Option<ConfigValue>,
    pub style: SidebarStyle,
    /// Collapse depth. Defaults to `2` (Q1 convention).
    pub collapse_level: u32,
    pub background: Option<String>,
    /// Override for the vertical separator between the sidebar and main
    /// content. `Some(true)` forces it on; `Some(false)` forces it off;
    /// `None` lets the implicit default apply (`style == Docked`). Q1
    /// parity for the `sidebar.border:` YAML knob (see
    /// `quarto-cli/.../format-html-scss.ts:631-642`); the value is
    /// consumed by `quarto-core::derive_doc_scss_layer` to emit
    /// `$sidebar-border` per-document.
    pub border: Option<bool>,
    pub contents: Vec<SidebarEntry>,
    pub pinned: bool,
}

impl Default for Sidebar {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl Sidebar {
    pub fn with_defaults() -> Self {
        Self {
            id: None,
            title: SidebarTitle::Default,
            subtitle: None,
            style: SidebarStyle::default(),
            collapse_level: 2,
            background: None,
            border: None,
            contents: Vec::new(),
            pinned: false,
        }
    }

    /// Parse a single sidebar object.
    pub fn from_config_value(cv: &ConfigValue) -> Self {
        let mut sb = Self::with_defaults();
        sb.id = cv.get("id").and_then(|v| v.as_plain_text());
        if let Some(title_cv) = cv.get("title") {
            sb.title = if title_cv.as_bool() == Some(false) {
                SidebarTitle::Hidden
            } else if title_cv.as_bool() == Some(true) {
                // `title: true` keeps the default behavior (fall back to
                // website title at resolve time). Mirrors NavbarTitle.
                SidebarTitle::Default
            } else {
                SidebarTitle::Text(title_cv.clone())
            };
        }
        sb.subtitle = cv.get("subtitle").cloned();
        sb.background = cv.get("background").and_then(|v| v.as_plain_text());
        sb.border = cv.get("border").and_then(|v| v.as_bool());
        sb.pinned = cv.get("pinned").and_then(|v| v.as_bool()).unwrap_or(false);
        if let Some(style) = cv
            .get("style")
            .and_then(|v| v.as_plain_text())
            .and_then(|s| SidebarStyle::from_str(&s))
        {
            sb.style = style;
        }
        if let Some(level) = cv.get("collapse-level").and_then(|v| v.as_int())
            && let Ok(u) = u32::try_from(level.max(0))
        {
            sb.collapse_level = u;
        }
        sb.contents = parse_contents(cv.get("contents"));
        sb
    }

    /// Parse the top-level `website.sidebar:` value, which may be a
    /// single object or an array of objects. Returns an empty `Vec`
    /// when the input is neither.
    pub fn parse_list_from_config(cv: &ConfigValue) -> Vec<Sidebar> {
        if let Some(arr) = cv.as_array() {
            return arr
                .iter()
                .filter(|v| v.as_map_entries().is_some())
                .map(Self::from_config_value)
                .collect();
        }
        if cv.as_map_entries().is_some() {
            return vec![Self::from_config_value(cv)];
        }
        Vec::new()
    }

    pub fn to_config_value(&self) -> ConfigValue {
        let info = SourceInfo::generated(By::programmatic_config());
        let mut entries: Vec<ConfigMapEntry> = Vec::new();

        if let Some(ref id) = self.id {
            entries.push(string_entry("id", id, &info));
        }
        match &self.title {
            SidebarTitle::Default => {}
            SidebarTitle::Hidden => entries.push(ConfigMapEntry {
                key: "title".to_string(),
                key_source: info.clone(),
                value: ConfigValue::new_bool(false, info.clone()),
            }),
            SidebarTitle::Text(cv) => entries.push(cv_entry("title", cv.clone(), &info)),
        }
        if let Some(ref sub) = self.subtitle {
            entries.push(cv_entry("subtitle", sub.clone(), &info));
        }
        if self.style != SidebarStyle::default() {
            entries.push(string_entry("style", self.style.as_str(), &info));
        }
        if self.collapse_level != 2 {
            entries.push(int_entry(
                "collapse-level",
                self.collapse_level as i64,
                &info,
            ));
        }
        if let Some(ref bg) = self.background {
            entries.push(string_entry("background", bg, &info));
        }
        if let Some(border) = self.border {
            entries.push(bool_entry("border", border, &info));
        }
        if self.pinned {
            entries.push(bool_entry("pinned", true, &info));
        }
        if !self.contents.is_empty() {
            let values: Vec<ConfigValue> = self
                .contents
                .iter()
                .map(SidebarEntry::to_config_value)
                .collect();
            entries.push(cv_entry(
                "contents",
                ConfigValue::new_array(values, info.clone()),
                &info,
            ));
        }

        ConfigValue::new_map(entries, info)
    }
}

fn parse_contents(cv: Option<&ConfigValue>) -> Vec<SidebarEntry> {
    let Some(cv) = cv else {
        return Vec::new();
    };
    // A **scalar** `contents:` is an `auto:` spec. Q1's
    // `normalizeSidebarItems` (website-sidebar-auto.ts) rewrites
    // `contents: <s>` to `[{auto: true}]` when `<s>` is "auto" and to
    // `[{auto: <s>}]` for every other string — so `contents: guides`
    // auto-generates the directory's entries, and `contents: intro.qmd`
    // expands to that one document. There is no file-vs-directory
    // branch here on purpose; expansion resolves it (D1/D3 of
    // bd-sidebar-contents-dir-shorthand-z7arvhx8).
    //
    // This is deliberately **scalar-only**. A bare string inside a
    // `contents:` *array* is not an auto spec — Q1 sends those through
    // `normalizeSidebarItem` (project-config.ts), which yields a link
    // or a plain label. Those still take the `from_plain_string` route
    // in `SidebarEntry::from_config_value`.
    if let Some(s) = cv.as_plain_text() {
        if s == "auto" {
            return vec![SidebarEntry::Auto(AutoSpec::All)];
        }
        // A lone separator is not a path; keep it a separator rather
        // than an auto spec that could only ever match nothing.
        if is_separator_string(&s) {
            return vec![SidebarEntry::Separator];
        }
        return vec![SidebarEntry::Auto(AutoSpec::Path(s))];
    }
    // Normal case: array of entries.
    if let Some(arr) = cv.as_array() {
        return arr
            .iter()
            .filter_map(SidebarEntry::from_config_value)
            .collect();
    }
    Vec::new()
}

fn is_separator_string(s: &str) -> bool {
    let trimmed = s.trim();
    !trimmed.is_empty() && trimmed.len() >= 3 && trimmed.chars().all(|c| c == '-')
}

fn string_entry(key: &str, value: &str, info: &SourceInfo) -> ConfigMapEntry {
    ConfigMapEntry {
        key: key.to_string(),
        key_source: info.clone(),
        value: ConfigValue::new_string(value, info.clone()),
    }
}

fn bool_entry(key: &str, value: bool, info: &SourceInfo) -> ConfigMapEntry {
    ConfigMapEntry {
        key: key.to_string(),
        key_source: info.clone(),
        value: ConfigValue::new_bool(value, info.clone()),
    }
}

fn int_entry(key: &str, value: i64, info: &SourceInfo) -> ConfigMapEntry {
    ConfigMapEntry {
        key: key.to_string(),
        key_source: info.clone(),
        value: ConfigValue::new_scalar(Yaml::Integer(value), info.clone()),
    }
}

fn cv_entry(key: &str, value: ConfigValue, info: &SourceInfo) -> ConfigMapEntry {
    ConfigMapEntry {
        key: key.to_string(),
        key_source: info.clone(),
        value,
    }
}

// ----------------------------------------------------------------------------
// Sidebar-for-page selection and active-state resolution.
//
// Both functions are **format-agnostic**. They compare against project
// source paths (forward-slash, project-relative strings), never against
// format-specific output hrefs. See
// `claude-notes/plans/2026-04-24-websites-phase-2.md` §Decision 7/8.
// ----------------------------------------------------------------------------

/// Pick the sidebar that applies to the current page.
///
/// Rules, in order (matches Q1 `sidebarForHref` with source-path
/// comparisons instead of output-href comparisons):
///
/// 1. If `meta` sets `site-sidebar: <id>` (Q1-compat, canonical) or
///    `website.sidebar-id: <id>` (namespaced alternative), prefer the
///    sidebar with that `id`.
/// 2. Otherwise, if exactly one sidebar is configured *and* it has
///    no `id`, that sidebar applies regardless of containment (Q1
///    wildcard).
/// 3. Otherwise, find the first sidebar whose contents (recursively)
///    reference `page_source`.
/// 4. Otherwise, no sidebar for this page.
pub fn sidebar_for_page<'a>(
    sidebars: &'a [Sidebar],
    page_source: &str,
    meta: &ConfigValue,
) -> Option<&'a Sidebar> {
    if sidebars.is_empty() {
        return None;
    }

    // Rule 1 — explicit override.
    let explicit_id = meta
        .get("site-sidebar")
        .and_then(|v| v.as_plain_text())
        .or_else(|| {
            meta.get_path(&["website", "sidebar-id"])
                .and_then(|v| v.as_plain_text())
        });
    if let Some(ref id) = explicit_id
        && let Some(found) = sidebars.iter().find(|sb| sb.id.as_deref() == Some(id))
    {
        return Some(found);
    }
    // The user asked for an id that doesn't exist. Fall through
    // rather than silently applying the wrong sidebar.

    // Rule 2 — wildcard single sidebar.
    if sidebars.len() == 1 && sidebars[0].id.is_none() {
        return Some(&sidebars[0]);
    }

    // Rule 3 — containment.
    sidebars
        .iter()
        .find(|sb| contains_source_path(&sb.contents, page_source))
}

fn contains_source_path(entries: &[SidebarEntry], page_source: &str) -> bool {
    for entry in entries {
        match entry {
            SidebarEntry::Link { item } => {
                if item.href.as_deref() == Some(page_source) {
                    return true;
                }
            }
            SidebarEntry::Section { href, contents, .. } => {
                if href.as_deref() == Some(page_source) {
                    return true;
                }
                if contains_source_path(contents, page_source) {
                    return true;
                }
            }
            SidebarEntry::Separator | SidebarEntry::Heading(_) | SidebarEntry::Auto(_) => {}
        }
    }
    false
}

/// Walk the sidebar tree, marking the entry matching `self_source`
/// active and all its ancestor sections `expanded: true`.
///
/// `self_source` is the current page's project-relative source path
/// in forward-slash form — compared directly against sidebar entry
/// `href`s.
///
/// Returns `true` when any entry matched. Callers can inspect this
/// to decide whether to emit a "page not in sidebar" diagnostic.
pub fn resolve_active_state(sidebar: &mut Sidebar, self_source: &str) -> bool {
    // `contents` is borrowed mutably only here; the helper encodes
    // the post-order bubble-up that marks ancestors expanded.
    mark_active_in(&mut sidebar.contents, self_source)
}

/// Returns `true` if any descendant (or direct child) of `entries`
/// matched and was marked active. Callers are responsible for then
/// setting their own `Section::expanded = true` if they wrap a match.
fn mark_active_in(entries: &mut [SidebarEntry], self_source: &str) -> bool {
    let mut any = false;
    for entry in entries.iter_mut() {
        match entry {
            SidebarEntry::Link { item } => {
                if item.href.as_deref() == Some(self_source) {
                    item.active = true;
                    any = true;
                }
            }
            SidebarEntry::Section {
                href,
                contents,
                expanded,
                ..
            } => {
                // Check header href first.
                let header_matches = href.as_deref() == Some(self_source);
                let child_matched = mark_active_in(contents, self_source);
                if header_matches || child_matched {
                    *expanded = true;
                    any = true;
                }
            }
            SidebarEntry::Separator | SidebarEntry::Heading(_) | SidebarEntry::Auto(_) => {}
        }
    }
    any
}

// ----------------------------------------------------------------------------
// Page-navigation flatten (Phase 4).
//
// Walks the sidebar tree depth-first to produce the linear sequence of
// "navigable positions" that prev/next neighbors are picked from. The
// rules — included shapes, dedupe-by-href, separators-as-boundary —
// match Q1's `flattenItems` + `nextAndPrevious`. See
// `claude-notes/plans/2026-04-24-websites-phase-4.md` §Decision 4.
// ----------------------------------------------------------------------------

/// One position in the page-nav flat list. Items are positions you can
/// navigate to; separators are hard boundaries that interrupt prev/next
/// adjacency without occupying a navigable position themselves.
#[derive(Debug, Clone, PartialEq)]
pub enum FlatEntry {
    Item(NavigationItem),
    Separator,
}

impl FlatEntry {
    /// True when this entry is an `Item` whose `href` equals
    /// `page_source` (project-relative source path, forward-slash
    /// form). Separators and href-less items never match.
    pub fn is_link_with_href(&self, page_source: &str) -> bool {
        match self {
            FlatEntry::Item(item) => item.href.as_deref() == Some(page_source),
            FlatEntry::Separator => false,
        }
    }
}

/// Depth-first flatten of `entries` for page-nav prev/next computation.
///
/// Inclusion rules:
/// - `Link { item }` with an internal `href` → `Item(item.clone())`.
/// - `Section { href: Some(_), contents, .. }` with an internal href:
///   the header is emitted as `Item(...)` *and* contents are recursed
///   into.
/// - `Section { href: None | external, contents, .. }`: header skipped,
///   contents still recursed.
/// - `Separator` → `Separator`.
/// - `Heading(_)` and `Auto(_)` are skipped (Auto defensively — should
///   have been expanded earlier).
///
/// External hrefs (`http://`, `https://`, `mailto:`, etc. — see
/// [`is_external_href`]) never produce navigable items.
///
/// After collection, items are de-duplicated by `href`, keeping the
/// first occurrence. Separators are never deduped.
pub fn flatten_for_page_nav(entries: &[SidebarEntry]) -> Vec<FlatEntry> {
    let mut out = Vec::new();
    walk_for_page_nav(entries, &mut out);
    dedupe_items_by_href(&mut out);
    out
}

fn walk_for_page_nav(entries: &[SidebarEntry], out: &mut Vec<FlatEntry>) {
    for entry in entries {
        match entry {
            SidebarEntry::Link { item } => {
                if let Some(href) = item.href.as_deref()
                    && !is_external_href(href)
                {
                    out.push(FlatEntry::Item(item.clone()));
                }
            }
            SidebarEntry::Section {
                href,
                contents,
                text,
                ..
            } => {
                if let Some(h) = href.as_deref()
                    && !is_external_href(h)
                {
                    // The section header is a navigable position
                    // — synthesize a NavigationItem from the
                    // section's href + text.
                    let item = NavigationItem {
                        href: Some(h.to_string()),
                        text: text.clone(),
                        ..NavigationItem::default()
                    };
                    out.push(FlatEntry::Item(item));
                }
                walk_for_page_nav(contents, out);
            }
            SidebarEntry::Separator => {
                out.push(FlatEntry::Separator);
            }
            SidebarEntry::Heading(_) | SidebarEntry::Auto(_) => {
                // Heading: pure label, no navigable position.
                // Auto: defensively skip (should have been expanded).
            }
        }
    }
}

fn dedupe_items_by_href(list: &mut Vec<FlatEntry>) {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    list.retain(|e| match e {
        FlatEntry::Item(item) => match item.href.as_deref() {
            Some(h) => seen.insert(h.to_string()),
            None => true, // hrefless items (shouldn't happen post-walk) survive
        },
        FlatEntry::Separator => true,
    });
}

/// External-URL classifier. Local copy here so `quarto-navigation`
/// stays free of a `quarto-core` dep; semantics match
/// `quarto-core::transforms::navigation_href::is_external`.
fn is_external_href(href: &str) -> bool {
    href.starts_with("http://")
        || href.starts_with("https://")
        || href.starts_with("mailto:")
        || href.starts_with("tel:")
        || href.starts_with("ftp://")
        || href.starts_with("//")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        let info = SourceInfo::for_test();
        let map_entries: Vec<ConfigMapEntry> = entries
            .into_iter()
            .map(|(k, v)| ConfigMapEntry {
                key: k.to_string(),
                key_source: info.clone(),
                value: v,
            })
            .collect();
        ConfigValue::new_map(map_entries, info)
    }

    fn s(x: &str) -> ConfigValue {
        ConfigValue::new_string(x, SourceInfo::for_test())
    }

    fn b(x: bool) -> ConfigValue {
        ConfigValue::new_bool(x, SourceInfo::for_test())
    }

    fn i(x: i64) -> ConfigValue {
        ConfigValue::new_scalar(Yaml::Integer(x), SourceInfo::for_test())
    }

    fn arr(items: Vec<ConfigValue>) -> ConfigValue {
        ConfigValue::new_array(items, SourceInfo::for_test())
    }

    /// Test 1 — single object form with leaf links.
    #[test]
    fn parse_sidebar_single_object() {
        let cv = map(vec![("contents", arr(vec![s("a.qmd"), s("b.qmd")]))]);
        let list = Sidebar::parse_list_from_config(&cv);
        assert_eq!(list.len(), 1);
        let sb = &list[0];
        assert_eq!(sb.contents.len(), 2);
        match &sb.contents[0] {
            SidebarEntry::Link { item } => {
                assert_eq!(item.href.as_deref(), Some("a.qmd"));
                assert!(!item.active);
            }
            other => panic!("expected Link, got {:?}", other),
        }
        match &sb.contents[1] {
            SidebarEntry::Link { item } => {
                assert_eq!(item.href.as_deref(), Some("b.qmd"));
            }
            other => panic!("expected Link, got {:?}", other),
        }
    }

    /// Test 2 — array form yields multiple sidebars.
    #[test]
    fn parse_sidebar_array_form() {
        let cv = arr(vec![
            map(vec![("id", s("main")), ("contents", arr(vec![s("a.qmd")]))]),
            map(vec![
                ("id", s("other")),
                ("contents", arr(vec![s("b.qmd")])),
            ]),
        ]);
        let list = Sidebar::parse_list_from_config(&cv);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id.as_deref(), Some("main"));
        assert_eq!(list[1].id.as_deref(), Some("other"));
    }

    /// Test 3 — `section:` with `contents:` parses as Section.
    #[test]
    fn parse_sidebar_nested_section() {
        let section = map(vec![
            ("section", s("Docs")),
            ("contents", arr(vec![s("x.qmd")])),
        ]);
        let cv = map(vec![("contents", arr(vec![section]))]);
        let list = Sidebar::parse_list_from_config(&cv);
        let sb = &list[0];
        assert_eq!(sb.contents.len(), 1);
        match &sb.contents[0] {
            SidebarEntry::Section {
                text,
                contents,
                expanded,
                ..
            } => {
                assert_eq!(
                    text.as_ref().unwrap().as_plain_text().as_deref(),
                    Some("Docs")
                );
                assert_eq!(contents.len(), 1);
                assert!(!expanded); // default false
                match &contents[0] {
                    SidebarEntry::Link { item } => {
                        assert_eq!(item.href.as_deref(), Some("x.qmd"));
                    }
                    other => panic!("expected Link inside section, got {:?}", other),
                }
            }
            other => panic!("expected Section, got {:?}", other),
        }
    }

    /// Test 4 — `auto: true`, `auto: "docs"`, `auto: ["a", "b"]` all parse.
    #[test]
    fn parse_sidebar_auto_variants() {
        let item_true = map(vec![("auto", b(true))]);
        let item_path = map(vec![("auto", s("docs"))]);
        let item_paths = map(vec![("auto", arr(vec![s("a"), s("b/*")]))]);
        let cv = map(vec![(
            "contents",
            arr(vec![item_true, item_path, item_paths]),
        )]);
        let list = Sidebar::parse_list_from_config(&cv);
        let sb = &list[0];
        assert_eq!(sb.contents.len(), 3);
        match &sb.contents[0] {
            SidebarEntry::Auto(AutoSpec::All) => {}
            other => panic!("expected Auto(All), got {:?}", other),
        }
        match &sb.contents[1] {
            SidebarEntry::Auto(AutoSpec::Path(p)) => assert_eq!(p, "docs"),
            other => panic!("expected Auto(Path), got {:?}", other),
        }
        match &sb.contents[2] {
            SidebarEntry::Auto(AutoSpec::Paths(ps)) => {
                assert_eq!(ps, &vec!["a".to_string(), "b/*".to_string()]);
            }
            other => panic!("expected Auto(Paths), got {:?}", other),
        }
    }

    /// Test 5 — bare `"---"` string is a separator.
    #[test]
    fn parse_sidebar_separator() {
        let cv = map(vec![(
            "contents",
            arr(vec![s("a.qmd"), s("---"), s("-----"), s("b.qmd")]),
        )]);
        let list = Sidebar::parse_list_from_config(&cv);
        let sb = &list[0];
        assert_eq!(sb.contents.len(), 4);
        assert!(matches!(sb.contents[1], SidebarEntry::Separator));
        assert!(matches!(sb.contents[2], SidebarEntry::Separator));
    }

    /// Test 6 — defaults: style=Floating, collapse_level=2.
    #[test]
    fn parse_sidebar_defaults() {
        let cv = map(vec![]);
        let sb = Sidebar::from_config_value(&cv);
        assert_eq!(sb.style, SidebarStyle::Floating);
        assert_eq!(sb.collapse_level, 2);
        assert!(sb.contents.is_empty());
    }

    /// Test 7 — round-trip a full sidebar through `to_config_value` /
    /// `from_config_value`.
    #[test]
    fn roundtrip_sidebar_to_config_value() {
        let original = Sidebar {
            id: Some("main".to_string()),
            title: SidebarTitle::Text(s("Docs")),
            style: SidebarStyle::Docked,
            collapse_level: 3,
            background: Some("light".to_string()),
            border: Some(true),
            pinned: true,
            contents: vec![
                SidebarEntry::Link {
                    item: NavigationItem {
                        href: Some("intro.qmd".to_string()),
                        text: Some(s("Intro")),
                        active: true,
                        ..NavigationItem::default()
                    },
                },
                SidebarEntry::Separator,
                SidebarEntry::Section {
                    text: Some(s("Advanced")),
                    href: None,
                    href_source: SourceInfo::for_test(),
                    id: Some("adv".to_string()),
                    expanded: true,
                    contents: vec![SidebarEntry::Link {
                        item: NavigationItem {
                            href: Some("deep.qmd".to_string()),
                            text: Some(s("Deep")),
                            ..NavigationItem::default()
                        },
                    }],
                },
                SidebarEntry::Heading(s("Resources")),
                SidebarEntry::Auto(AutoSpec::Path("appendix".to_string())),
            ],
            subtitle: None,
        };
        let cv = original.to_config_value();
        let reparsed = Sidebar::from_config_value(&cv);

        // Scalar fields.
        assert_eq!(reparsed.id, original.id);
        assert_eq!(reparsed.style, original.style);
        assert_eq!(reparsed.collapse_level, original.collapse_level);
        assert_eq!(reparsed.background, original.background);
        assert_eq!(reparsed.pinned, original.pinned);

        // Contents — check shape preservation rather than full PartialEq
        // because inline markup may serialize differently.
        assert_eq!(reparsed.contents.len(), original.contents.len());
        match (&reparsed.contents[0], &original.contents[0]) {
            (SidebarEntry::Link { item: a }, SidebarEntry::Link { item: b }) => {
                assert_eq!(a.href, b.href);
                assert_eq!(a.active, b.active);
            }
            _ => panic!("link shape lost in roundtrip"),
        }
        assert!(matches!(reparsed.contents[1], SidebarEntry::Separator));
        match (&reparsed.contents[2], &original.contents[2]) {
            (
                SidebarEntry::Section {
                    contents: a,
                    expanded: a_exp,
                    ..
                },
                SidebarEntry::Section {
                    contents: b,
                    expanded: b_exp,
                    ..
                },
            ) => {
                assert_eq!(a.len(), b.len());
                assert_eq!(a_exp, b_exp);
            }
            _ => panic!("section shape lost in roundtrip"),
        }
        assert!(matches!(reparsed.contents[3], SidebarEntry::Heading(_)));
        match &reparsed.contents[4] {
            SidebarEntry::Auto(AutoSpec::Path(p)) => assert_eq!(p, "appendix"),
            other => panic!("auto shape lost: {:?}", other),
        }
    }

    /// A scalar `contents:` is an `auto:` spec — Q1's
    /// `normalizeSidebarItems` turns `contents: <s>` into
    /// `[{auto: <s>}]` for every string but `auto` itself.
    ///
    /// bd-sidebar-contents-dir-shorthand-z7arvhx8: this previously
    /// asserted a `Link`, which is what made `contents: guides` render
    /// a dead link. A file-shaped scalar goes down the same path — Q1
    /// makes no file-vs-directory distinction here, because expansion
    /// resolves it: a spec naming one document expands to one link.
    #[test]
    fn parse_sidebar_bare_string_contents_is_auto_spec() {
        let cv = map(vec![("contents", s("hello.qmd"))]);
        let list = Sidebar::parse_list_from_config(&cv);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].contents.len(), 1);
        match &list[0].contents[0] {
            SidebarEntry::Auto(AutoSpec::Path(p)) => assert_eq!(p, "hello.qmd"),
            other => panic!("expected Auto(Path), got {:?}", other),
        }
    }

    /// The reported bug: `contents: guides` must become an auto spec
    /// so expansion can generate the directory's entries.
    #[test]
    fn parse_sidebar_directory_shorthand_is_auto_spec() {
        let cv = map(vec![("contents", s("guides"))]);
        let list = Sidebar::parse_list_from_config(&cv);
        match &list[0].contents[0] {
            SidebarEntry::Auto(AutoSpec::Path(p)) => assert_eq!(p, "guides"),
            other => panic!("expected Auto(Path), got {:?}", other),
        }
    }

    /// The shorthand reaches nested section `contents:` too — Q1's
    /// `expandAutoSidebarItems` recurses into `item.contents`.
    #[test]
    fn parse_sidebar_directory_shorthand_applies_to_nested_section() {
        let cv = map(vec![(
            "contents",
            arr(vec![map(vec![
                ("section", s("Guides")),
                ("contents", s("guides")),
            ])]),
        )]);
        let list = Sidebar::parse_list_from_config(&cv);
        match &list[0].contents[0] {
            SidebarEntry::Section { contents, .. } => match &contents[0] {
                SidebarEntry::Auto(AutoSpec::Path(p)) => assert_eq!(p, "guides"),
                other => panic!("expected nested Auto(Path), got {:?}", other),
            },
            other => panic!("expected Section, got {:?}", other),
        }
    }

    /// Fence: the shorthand is **scalar-only**. A bare string that is
    /// an *array element* stays a `Link`, matching Q1, where array
    /// items go through `normalizeSidebarItem` (project-config.ts) and
    /// never become an `auto`.
    #[test]
    fn parse_sidebar_bare_string_in_array_stays_a_link() {
        let cv = map(vec![("contents", arr(vec![s("guides"), s("about.qmd")]))]);
        let list = Sidebar::parse_list_from_config(&cv);
        assert_eq!(list[0].contents.len(), 2);
        for entry in &list[0].contents {
            assert!(
                matches!(entry, SidebarEntry::Link { .. }),
                "array elements must stay Links, got {:?}",
                entry
            );
        }
    }

    /// Fence: a scalar separator is still a separator, not an auto
    /// spec that would match nothing and warn.
    #[test]
    fn parse_sidebar_scalar_separator_is_not_auto() {
        let cv = map(vec![("contents", s("---"))]);
        let list = Sidebar::parse_list_from_config(&cv);
        assert!(matches!(list[0].contents[0], SidebarEntry::Separator));
    }

    /// `contents: auto` shorthand expands to a single `Auto(All)` entry.
    #[test]
    fn parse_sidebar_contents_auto_shorthand() {
        let cv = map(vec![("contents", s("auto"))]);
        let list = Sidebar::parse_list_from_config(&cv);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].contents.len(), 1);
        assert!(matches!(
            list[0].contents[0],
            SidebarEntry::Auto(AutoSpec::All)
        ));
    }

    /// `text:` without link bits becomes a `Heading`.
    #[test]
    fn parse_sidebar_text_only_is_heading() {
        let cv = map(vec![(
            "contents",
            arr(vec![map(vec![("text", s("Section label"))])]),
        )]);
        let list = Sidebar::parse_list_from_config(&cv);
        match &list[0].contents[0] {
            SidebarEntry::Heading(cv) => {
                assert_eq!(cv.as_plain_text().as_deref(), Some("Section label"));
            }
            other => panic!("expected Heading, got {:?}", other),
        }
    }

    /// `collapse-level: 4` overrides the default.
    #[test]
    fn parse_sidebar_collapse_level_override() {
        let cv = map(vec![("collapse-level", i(4))]);
        let sb = Sidebar::from_config_value(&cv);
        assert_eq!(sb.collapse_level, 4);
    }

    /// `style: docked` parses.
    #[test]
    fn parse_sidebar_style_docked() {
        let cv = map(vec![("style", s("docked"))]);
        let sb = Sidebar::from_config_value(&cv);
        assert_eq!(sb.style, SidebarStyle::Docked);
    }

    // --- `border:` (Phase 3 of bd-k8y0) ---
    //
    // Q1 parity: `sidebar.border: true|false` lets users force the
    // vertical border on/off independently of `style`. Absent ⇒ `None`,
    // and `derive_doc_scss_layer` falls back to `(style == "docked")`.

    #[test]
    fn parse_sidebar_border_absent_is_none() {
        let cv = map(vec![("style", s("docked"))]);
        let sb = Sidebar::from_config_value(&cv);
        assert_eq!(
            sb.border, None,
            "absent `border:` should remain None so the implicit default applies"
        );
    }

    #[test]
    fn parse_sidebar_border_true() {
        let cv = map(vec![("border", b(true))]);
        let sb = Sidebar::from_config_value(&cv);
        assert_eq!(sb.border, Some(true));
    }

    #[test]
    fn parse_sidebar_border_false() {
        let cv = map(vec![("border", b(false))]);
        let sb = Sidebar::from_config_value(&cv);
        assert_eq!(sb.border, Some(false));
    }

    #[test]
    fn sidebar_border_round_trip() {
        let cv_in = map(vec![("style", s("floating")), ("border", b(true))]);
        let sb = Sidebar::from_config_value(&cv_in);
        assert_eq!(sb.border, Some(true));
        let cv_out = sb.to_config_value();
        // `border:` should re-emit when explicitly set
        let entries = cv_out.as_map_entries().expect("map");
        let border_entry = entries
            .iter()
            .find(|e| e.key == "border")
            .expect("explicit border must round-trip through to_config_value");
        assert_eq!(border_entry.value.as_bool(), Some(true));
    }

    #[test]
    fn sidebar_border_round_trip_omitted_when_none() {
        let cv_in = map(vec![("style", s("docked"))]);
        let sb = Sidebar::from_config_value(&cv_in);
        assert_eq!(sb.border, None);
        let cv_out = sb.to_config_value();
        let entries = cv_out.as_map_entries().expect("map");
        assert!(
            entries.iter().all(|e| e.key != "border"),
            "absent border must not appear in to_config_value output \
             (otherwise we'd round-trip None → false)"
        );
    }

    /// A separator that's shorter than three dashes is treated as a
    /// literal href instead (so the rule isn't over-eager).
    #[test]
    fn short_dashes_are_not_separators() {
        let cv = map(vec![("contents", arr(vec![s("--")]))]);
        let list = Sidebar::parse_list_from_config(&cv);
        match &list[0].contents[0] {
            SidebarEntry::Link { item } => {
                assert_eq!(item.href.as_deref(), Some("--"));
            }
            other => panic!("expected Link for two-dash string, got {:?}", other),
        }
    }

    // --- SidebarTitle tri-state parsing (sidebar-default-title) ------------
    //
    // Mirrors NavbarTitle::{Default, Hidden, Text}: the per-sidebar `title:`
    // field can be absent (→ Default, the renderer/transform falls back to
    // website.title), `false` (→ Hidden, no header), `true` (→ Default,
    // matches Navbar semantics), or a value (→ Text(value)).

    #[test]
    fn parse_sidebar_title_default_when_absent() {
        let cv = map(vec![]);
        let sb = Sidebar::from_config_value(&cv);
        assert!(
            matches!(sb.title, SidebarTitle::Default),
            "absent title should parse as Default; got {:?}",
            sb.title
        );
    }

    #[test]
    fn parse_sidebar_title_false_is_hidden() {
        let cv = map(vec![("title", b(false))]);
        let sb = Sidebar::from_config_value(&cv);
        assert!(
            matches!(sb.title, SidebarTitle::Hidden),
            "title: false should parse as Hidden; got {:?}",
            sb.title
        );
    }

    #[test]
    fn parse_sidebar_title_true_is_default() {
        let cv = map(vec![("title", b(true))]);
        let sb = Sidebar::from_config_value(&cv);
        assert!(
            matches!(sb.title, SidebarTitle::Default),
            "title: true should parse as Default (Navbar parity); got {:?}",
            sb.title
        );
    }

    #[test]
    fn parse_sidebar_title_string_is_text() {
        let cv = map(vec![("title", s("Hello"))]);
        let sb = Sidebar::from_config_value(&cv);
        match &sb.title {
            SidebarTitle::Text(cv) => {
                assert_eq!(cv.as_plain_text().as_deref(), Some("Hello"));
            }
            other => panic!("title: \"Hello\" should parse as Text; got {:?}", other),
        }
    }

    #[test]
    fn roundtrip_sidebar_title_default() {
        // Default round-trips as "no title key emitted" → re-parse stays
        // Default. We construct a Sidebar with Default and verify both the
        // emitted ConfigValue lacks a `title:` key and the re-parsed value
        // is Default.
        let sb = Sidebar {
            title: SidebarTitle::Default,
            ..Sidebar::with_defaults()
        };
        let cv = sb.to_config_value();
        assert!(
            cv.get("title").is_none(),
            "Default should emit no `title:` key; got cv: {:?}",
            cv
        );
        let reparsed = Sidebar::from_config_value(&cv);
        assert!(matches!(reparsed.title, SidebarTitle::Default));
    }

    #[test]
    fn roundtrip_sidebar_title_hidden() {
        let sb = Sidebar {
            title: SidebarTitle::Hidden,
            ..Sidebar::with_defaults()
        };
        let cv = sb.to_config_value();
        assert_eq!(
            cv.get("title").and_then(|v| v.as_bool()),
            Some(false),
            "Hidden should emit `title: false`; got cv: {:?}",
            cv
        );
        let reparsed = Sidebar::from_config_value(&cv);
        assert!(matches!(reparsed.title, SidebarTitle::Hidden));
    }

    #[test]
    fn roundtrip_sidebar_title_text() {
        let sb = Sidebar {
            title: SidebarTitle::Text(s("My Site")),
            ..Sidebar::with_defaults()
        };
        let cv = sb.to_config_value();
        assert_eq!(
            cv.get("title").and_then(|v| v.as_plain_text()).as_deref(),
            Some("My Site"),
            "Text should emit `title: <value>`; got cv: {:?}",
            cv
        );
        let reparsed = Sidebar::from_config_value(&cv);
        match &reparsed.title {
            SidebarTitle::Text(cv) => {
                assert_eq!(cv.as_plain_text().as_deref(), Some("My Site"));
            }
            other => panic!("expected Text after roundtrip; got {:?}", other),
        }
    }

    // --- Sidebar-for-page + active-state tests (Phase 2) -------------------

    fn sidebar_from_yaml(cv: ConfigValue) -> Sidebar {
        Sidebar::from_config_value(&cv)
    }

    /// Test 14 — a single sidebar without `id` applies to every page.
    #[test]
    fn resolve_single_sidebar_without_id_matches_every_page() {
        let sb = sidebar_from_yaml(map(vec![("contents", arr(vec![s("other.qmd")]))]));
        let sidebars = vec![sb];
        // The current page isn't even referenced, yet the wildcard
        // single-sidebar rule still applies.
        let picked = sidebar_for_page(&sidebars, "random.qmd", &map(vec![]));
        assert!(picked.is_some());
    }

    /// Test 15 — explicit `site-sidebar: <id>` wins.
    #[test]
    fn resolve_explicit_id_override_wins() {
        let main = sidebar_from_yaml(map(vec![
            ("id", s("main")),
            ("contents", arr(vec![s("a.qmd")])),
        ]));
        let reference = sidebar_from_yaml(map(vec![
            ("id", s("reference")),
            ("contents", arr(vec![s("r.qmd")])),
        ]));
        let sidebars = vec![main, reference];
        let meta = map(vec![("site-sidebar", s("reference"))]);
        let picked = sidebar_for_page(&sidebars, "some-other.qmd", &meta).unwrap();
        assert_eq!(picked.id.as_deref(), Some("reference"));
    }

    /// Test 15 variant — `website.sidebar-id: <id>` is also accepted.
    #[test]
    fn resolve_explicit_website_sidebar_id_also_accepted() {
        let main = sidebar_from_yaml(map(vec![
            ("id", s("main")),
            ("contents", arr(vec![s("a.qmd")])),
        ]));
        let reference = sidebar_from_yaml(map(vec![
            ("id", s("reference")),
            ("contents", arr(vec![s("r.qmd")])),
        ]));
        let sidebars = vec![main, reference];
        let meta = map(vec![("website", map(vec![("sidebar-id", s("reference"))]))]);
        let picked = sidebar_for_page(&sidebars, "some-other.qmd", &meta).unwrap();
        assert_eq!(picked.id.as_deref(), Some("reference"));
    }

    /// Test 15 edge case — an unknown explicit id falls through to
    /// containment rather than silently matching the wrong sidebar.
    #[test]
    fn resolve_explicit_unknown_id_falls_through() {
        let main = sidebar_from_yaml(map(vec![
            ("id", s("main")),
            ("contents", arr(vec![s("a.qmd")])),
        ]));
        let sidebars = vec![main];
        let meta = map(vec![("site-sidebar", s("does-not-exist"))]);
        // "a.qmd" IS in the only sidebar, so containment picks it even
        // though the explicit id failed.
        let picked = sidebar_for_page(&sidebars, "a.qmd", &meta);
        assert!(picked.is_some());
    }

    /// Test 16 — containment fallback matches by source path.
    #[test]
    fn resolve_containment_fallback() {
        let first = sidebar_from_yaml(map(vec![
            ("id", s("first")),
            ("contents", arr(vec![s("intro.qmd"), s("about.qmd")])),
        ]));
        let second = sidebar_from_yaml(map(vec![
            ("id", s("second")),
            ("contents", arr(vec![s("docs/api.qmd")])),
        ]));
        let sidebars = vec![first, second];
        let picked = sidebar_for_page(&sidebars, "docs/api.qmd", &map(vec![])).unwrap();
        assert_eq!(picked.id.as_deref(), Some("second"));
    }

    /// Test 17 — no match in any sidebar returns `None`.
    #[test]
    fn resolve_no_match_returns_none() {
        let first = sidebar_from_yaml(map(vec![
            ("id", s("first")),
            ("contents", arr(vec![s("a.qmd")])),
        ]));
        let second = sidebar_from_yaml(map(vec![
            ("id", s("second")),
            ("contents", arr(vec![s("b.qmd")])),
        ]));
        let sidebars = vec![first, second];
        let picked = sidebar_for_page(&sidebars, "unrelated.qmd", &map(vec![]));
        assert!(picked.is_none());
    }

    /// Test 18 — containment recurses into sections.
    #[test]
    fn resolve_containment_checks_nested_sections() {
        let section = map(vec![
            ("section", s("Advanced")),
            ("contents", arr(vec![s("advanced/deep.qmd")])),
        ]);
        let first = sidebar_from_yaml(map(vec![
            ("id", s("main")),
            ("contents", arr(vec![s("top.qmd"), section])),
        ]));
        let second = sidebar_from_yaml(map(vec![
            ("id", s("other")),
            ("contents", arr(vec![s("ignored.qmd")])),
        ]));
        let sidebars = vec![first, second];
        let picked = sidebar_for_page(&sidebars, "advanced/deep.qmd", &map(vec![])).unwrap();
        assert_eq!(picked.id.as_deref(), Some("main"));
    }

    /// Test 26 — active state marks a leaf and expands every ancestor.
    #[test]
    fn active_state_marks_leaf_and_expands_ancestors() {
        let inner_section = map(vec![
            ("section", s("Inner")),
            ("contents", arr(vec![s("deep.qmd")])),
        ]);
        let outer_section = map(vec![
            ("section", s("Outer")),
            ("contents", arr(vec![inner_section])),
        ]);
        let mut sb = sidebar_from_yaml(map(vec![(
            "contents",
            arr(vec![s("other.qmd"), outer_section]),
        )]));
        let matched = resolve_active_state(&mut sb, "deep.qmd");
        assert!(matched);

        // Outer section expanded?
        match &sb.contents[1] {
            SidebarEntry::Section {
                expanded, contents, ..
            } => {
                assert!(*expanded, "outer section should be expanded");
                match &contents[0] {
                    SidebarEntry::Section {
                        expanded, contents, ..
                    } => {
                        assert!(*expanded, "inner section should be expanded");
                        match &contents[0] {
                            SidebarEntry::Link { item } => {
                                assert!(item.active, "matching leaf should be active");
                            }
                            other => panic!("expected Link, got {:?}", other),
                        }
                    }
                    other => panic!("expected inner Section, got {:?}", other),
                }
            }
            other => panic!("expected outer Section, got {:?}", other),
        }

        // Unrelated leaf stays inactive.
        match &sb.contents[0] {
            SidebarEntry::Link { item } => assert!(!item.active),
            _ => unreachable!(),
        }
    }

    /// Test 27 — when the current page isn't referenced, nothing changes.
    #[test]
    fn active_state_no_self_source_no_changes() {
        let mut sb = sidebar_from_yaml(map(vec![("contents", arr(vec![s("a.qmd"), s("b.qmd")]))]));
        let matched = resolve_active_state(&mut sb, "elsewhere.qmd");
        assert!(!matched);
        for entry in &sb.contents {
            match entry {
                SidebarEntry::Link { item } => assert!(!item.active),
                _ => unreachable!(),
            }
        }
    }

    /// Test 27a — active-state resolution is source-path-keyed: it
    /// matches on the sidebar entry's href being the source path, not
    /// on any derived output href. Proves Generate is format-agnostic.
    #[test]
    fn active_state_is_source_path_keyed() {
        // Entry href is the literal source path; active-state matches
        // when the current page's source path equals it. There's no
        // output-href lookup happening.
        let mut sb = sidebar_from_yaml(map(vec![("contents", arr(vec![s("about.qmd")]))]));
        let matched = resolve_active_state(&mut sb, "about.qmd");
        assert!(matched);
        match &sb.contents[0] {
            SidebarEntry::Link { item } => {
                assert!(item.active);
                // Href unchanged — Generate does not rewrite.
                assert_eq!(item.href.as_deref(), Some("about.qmd"));
            }
            _ => unreachable!(),
        }
    }

    /// Active-state also matches a Section's header-href, not just leaves.
    #[test]
    fn active_state_matches_section_header_href() {
        let section = map(vec![
            ("section", s("Docs")),
            ("href", s("docs/index.qmd")),
            ("contents", arr(vec![s("docs/a.qmd")])),
        ]);
        let mut sb = sidebar_from_yaml(map(vec![("contents", arr(vec![section]))]));
        let matched = resolve_active_state(&mut sb, "docs/index.qmd");
        assert!(matched);
        match &sb.contents[0] {
            SidebarEntry::Section { expanded, .. } => assert!(*expanded),
            _ => unreachable!(),
        }
    }

    // --- Phase 4: page-nav flatten ----------------------------------------

    fn flatten_yaml(yaml: ConfigValue) -> Vec<FlatEntry> {
        let sb = sidebar_from_yaml(yaml);
        flatten_for_page_nav(&sb.contents)
    }

    fn item_href(e: &FlatEntry) -> Option<&str> {
        match e {
            FlatEntry::Item(item) => item.href.as_deref(),
            FlatEntry::Separator => None,
        }
    }

    /// Test 4 — internal links flow through, external links don't.
    #[test]
    fn flatten_includes_internal_links_only() {
        let cv = map(vec![(
            "contents",
            arr(vec![s("about.qmd"), s("https://example.com/")]),
        )]);
        let flat = flatten_yaml(cv);
        assert_eq!(
            flat.len(),
            1,
            "external link should be excluded; got {:?}",
            flat
        );
        assert_eq!(item_href(&flat[0]), Some("about.qmd"));
    }

    /// Test 5 — section header with an internal href emits as an Item,
    /// and the section's children are recursed into.
    #[test]
    fn flatten_includes_section_header_with_href() {
        let section = map(vec![
            ("section", s("Docs")),
            ("href", s("docs/index.qmd")),
            ("contents", arr(vec![s("docs/a.qmd"), s("docs/b.qmd")])),
        ]);
        let cv = map(vec![("contents", arr(vec![section]))]);
        let flat = flatten_yaml(cv);
        let hrefs: Vec<&str> = flat.iter().filter_map(item_href).collect();
        assert_eq!(
            hrefs,
            vec!["docs/index.qmd", "docs/a.qmd", "docs/b.qmd"],
            "header then children, depth-first"
        );
    }

    /// Test 6 — section without an href has its header skipped, but
    /// children are still walked.
    #[test]
    fn flatten_skips_section_header_without_href() {
        let section = map(vec![
            ("section", s("Group")),
            ("contents", arr(vec![s("a.qmd"), s("b.qmd")])),
        ]);
        let cv = map(vec![("contents", arr(vec![section]))]);
        let flat = flatten_yaml(cv);
        let hrefs: Vec<&str> = flat.iter().filter_map(item_href).collect();
        assert_eq!(hrefs, vec!["a.qmd", "b.qmd"]);
    }

    /// Test 7 — separators appear in the flat list as boundary markers.
    #[test]
    fn flatten_includes_separators_as_markers() {
        let cv = map(vec![(
            "contents",
            arr(vec![s("a.qmd"), s("---"), s("b.qmd")]),
        )]);
        let flat = flatten_yaml(cv);
        assert_eq!(flat.len(), 3);
        assert!(matches!(flat[0], FlatEntry::Item(_)));
        assert!(matches!(flat[1], FlatEntry::Separator));
        assert!(matches!(flat[2], FlatEntry::Item(_)));
    }

    /// Test 8 — pure heading labels are skipped.
    #[test]
    fn flatten_skips_headings() {
        let heading = map(vec![("text", s("Group label"))]);
        let cv = map(vec![("contents", arr(vec![heading, s("a.qmd")]))]);
        let flat = flatten_yaml(cv);
        let hrefs: Vec<&str> = flat.iter().filter_map(item_href).collect();
        assert_eq!(hrefs, vec!["a.qmd"]);
    }

    /// Test 9 — defensive: stray Auto entries (should have been
    /// expanded earlier) are skipped, not panicked-on.
    #[test]
    fn flatten_skips_stray_auto() {
        // Construct a Sidebar manually to plant an Auto that would
        // otherwise have been expanded.
        let mut sb = Sidebar::with_defaults();
        sb.contents = vec![
            SidebarEntry::Auto(AutoSpec::All),
            SidebarEntry::Link {
                item: NavigationItem {
                    href: Some("a.qmd".to_string()),
                    ..NavigationItem::default()
                },
            },
        ];
        let flat = flatten_for_page_nav(&sb.contents);
        let hrefs: Vec<&str> = flat.iter().filter_map(item_href).collect();
        assert_eq!(hrefs, vec!["a.qmd"]);
    }

    /// Test 10 — dedupe-by-href keeps the first occurrence (section
    /// header that shares an href with one of its children).
    #[test]
    fn flatten_dedupes_by_href_keeping_first() {
        let section = map(vec![
            ("section", s("Docs")),
            ("href", s("docs.qmd")),
            ("contents", arr(vec![s("docs.qmd"), s("other.qmd")])),
        ]);
        let cv = map(vec![("contents", arr(vec![section]))]);
        let flat = flatten_yaml(cv);
        let hrefs: Vec<&str> = flat.iter().filter_map(item_href).collect();
        assert_eq!(
            hrefs,
            vec!["docs.qmd", "other.qmd"],
            "section header occurs first; later child duplicate dropped"
        );
        // Header text comes from the section, not the child link.
        match &flat[0] {
            FlatEntry::Item(item) => assert_eq!(
                item.text
                    .as_ref()
                    .and_then(|t| t.as_plain_text())
                    .as_deref(),
                Some("Docs"),
                "first-occurrence wins"
            ),
            _ => unreachable!(),
        }
    }

    /// Test 11 — separators are never deduped; two separators on
    /// either side of a link survive as three flat entries.
    #[test]
    fn flatten_dedupe_does_not_collapse_separators() {
        let cv = map(vec![(
            "contents",
            arr(vec![s("---"), s("a.qmd"), s("---")]),
        )]);
        let flat = flatten_yaml(cv);
        assert_eq!(flat.len(), 3);
        assert!(matches!(flat[0], FlatEntry::Separator));
        assert!(matches!(flat[1], FlatEntry::Item(_)));
        assert!(matches!(flat[2], FlatEntry::Separator));
    }

    /// Test 12 — depth-first traversal pins exact Q1-style ordering on
    /// a two-level fixture. Reviewer can eyeball the expected sequence.
    #[test]
    fn flatten_depth_first_order_matches_q1() {
        // sidebar:
        //   - index.qmd
        //   - section: Setup
        //     href: setup/index.qmd
        //     contents:
        //       - setup/install.qmd
        //       - section: Advanced
        //         contents:
        //           - setup/advanced/tuning.qmd
        //           - setup/advanced/profile.qmd
        //   - about.qmd
        let advanced = map(vec![
            ("section", s("Advanced")),
            (
                "contents",
                arr(vec![
                    s("setup/advanced/tuning.qmd"),
                    s("setup/advanced/profile.qmd"),
                ]),
            ),
        ]);
        let setup = map(vec![
            ("section", s("Setup")),
            ("href", s("setup/index.qmd")),
            ("contents", arr(vec![s("setup/install.qmd"), advanced])),
        ]);
        let cv = map(vec![(
            "contents",
            arr(vec![s("index.qmd"), setup, s("about.qmd")]),
        )]);
        let flat = flatten_yaml(cv);
        let hrefs: Vec<&str> = flat.iter().filter_map(item_href).collect();
        assert_eq!(
            hrefs,
            vec![
                "index.qmd",
                "setup/index.qmd",
                "setup/install.qmd",
                "setup/advanced/tuning.qmd",
                "setup/advanced/profile.qmd",
                "about.qmd",
            ]
        );
    }

    /// Sanity: `is_link_with_href` matches Item href and rejects
    /// separators / non-matching items.
    #[test]
    fn flat_entry_is_link_with_href() {
        let item = FlatEntry::Item(NavigationItem {
            href: Some("about.qmd".to_string()),
            ..NavigationItem::default()
        });
        assert!(item.is_link_with_href("about.qmd"));
        assert!(!item.is_link_with_href("other.qmd"));
        assert!(!FlatEntry::Separator.is_link_with_href("about.qmd"));
    }
}
