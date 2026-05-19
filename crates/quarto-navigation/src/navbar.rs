/*
 * navbar.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Navbar data model and YAML resolution.
//!
//! [`resolve_navbar`] accepts the merged `ast.meta` and returns a [`Navbar`]
//! populated with defaults, or `None` if the user has suppressed the navbar
//! (`navbar: false`) or not configured it at all.
//!
//! The YAML surface deliberately mirrors Quarto 1 so migration is familiar:
//! `title`, `logo`, `logo-alt`, `logo-href`, `background`, `foreground`,
//! `search`, `pinned`, `collapse`, `collapse-below`, `toggle-position`,
//! `tools-collapse`, `left`, `right`.

use quarto_config::resolve_website_value;
use quarto_pandoc_types::ConfigMapEntry;
use quarto_pandoc_types::config_value::ConfigValue;
use quarto_source_map::SourceInfo;

use crate::item::NavigationItem;

/// Title treatment for the navbar.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum NavbarTitle {
    /// No explicit title; renderer may fall back to the document title.
    #[default]
    Default,
    /// Title text. Preserved as a `ConfigValue` so document-context markdown
    /// survives.
    Text(ConfigValue),
    /// Explicitly suppressed via `title: false`.
    Hidden,
}

/// Responsive breakpoint at which the navbar collapses into a hamburger menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CollapseBelow {
    Sm,
    Md,
    #[default]
    Lg,
    Xl,
    Xxl,
}

impl CollapseBelow {
    pub fn as_str(&self) -> &'static str {
        match self {
            CollapseBelow::Sm => "sm",
            CollapseBelow::Md => "md",
            CollapseBelow::Lg => "lg",
            CollapseBelow::Xl => "xl",
            CollapseBelow::Xxl => "xxl",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "sm" => Some(Self::Sm),
            "md" => Some(Self::Md),
            "lg" => Some(Self::Lg),
            "xl" => Some(Self::Xl),
            "xxl" => Some(Self::Xxl),
            _ => None,
        }
    }
}

/// Position of the collapsed-navbar toggle in responsive mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TogglePosition {
    #[default]
    Left,
    Right,
}

impl TogglePosition {
    pub fn as_str(&self) -> &'static str {
        match self {
            TogglePosition::Left => "left",
            TogglePosition::Right => "right",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            _ => None,
        }
    }
}

/// Fully resolved navbar configuration.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Navbar {
    pub title: NavbarTitle,
    pub logo: Option<String>,
    pub logo_alt: Option<String>,
    pub logo_href: Option<String>,
    pub background: Option<String>,
    pub foreground: Option<String>,
    pub search: bool,
    pub pinned: bool,
    /// Defaults to `true` per Quarto 1 semantics.
    pub collapse: bool,
    pub collapse_below: CollapseBelow,
    pub toggle_position: TogglePosition,
    pub tools_collapse: bool,
    pub left: Vec<NavigationItem>,
    pub right: Vec<NavigationItem>,
}

impl Navbar {
    /// Build a `Navbar` with Quarto 1-matched defaults.
    pub fn with_defaults() -> Self {
        Self {
            title: NavbarTitle::Default,
            logo: None,
            logo_alt: None,
            logo_href: None,
            background: None,
            foreground: None,
            search: false,
            pinned: false,
            collapse: true,
            collapse_below: CollapseBelow::Lg,
            toggle_position: TogglePosition::Left,
            tools_collapse: false,
            left: Vec::new(),
            right: Vec::new(),
        }
    }

    /// Parse a navbar from its YAML object form.
    ///
    /// This expects the value at the `navbar:` key (already unwrapped —
    /// [`resolve_navbar`] handles the boolean form). Returns a navbar even if
    /// every field is missing; callers decide whether an empty navbar is
    /// meaningful.
    pub fn from_config_value(cv: &ConfigValue) -> Self {
        let mut nav = Self::with_defaults();

        if let Some(title_cv) = cv.get("title") {
            nav.title = if title_cv.as_bool() == Some(false) {
                NavbarTitle::Hidden
            } else if title_cv.as_bool() == Some(true) {
                // `title: true` keeps the default behavior (fall back).
                NavbarTitle::Default
            } else {
                NavbarTitle::Text(title_cv.clone())
            };
        }

        nav.logo = cv.get("logo").and_then(|v| v.as_plain_text());
        nav.logo_alt = cv.get("logo-alt").and_then(|v| v.as_plain_text());
        nav.logo_href = cv.get("logo-href").and_then(|v| v.as_plain_text());
        nav.background = cv.get("background").and_then(|v| v.as_plain_text());
        nav.foreground = cv.get("foreground").and_then(|v| v.as_plain_text());

        if let Some(v) = cv.get("search").and_then(|v| v.as_bool()) {
            nav.search = v;
        }
        if let Some(v) = cv.get("pinned").and_then(|v| v.as_bool()) {
            nav.pinned = v;
        }
        if let Some(v) = cv.get("collapse").and_then(|v| v.as_bool()) {
            nav.collapse = v;
        }
        if let Some(v) = cv.get("tools-collapse").and_then(|v| v.as_bool()) {
            nav.tools_collapse = v;
        }

        if let Some(v) = cv
            .get("collapse-below")
            .and_then(|v| v.as_plain_text())
            .and_then(|s| CollapseBelow::from_str(&s))
        {
            nav.collapse_below = v;
        }
        if let Some(v) = cv
            .get("toggle-position")
            .and_then(|v| v.as_plain_text())
            .and_then(|s| TogglePosition::from_str(&s))
        {
            nav.toggle_position = v;
        }

        nav.left = parse_item_list(cv.get("left"));
        nav.right = parse_item_list(cv.get("right"));

        nav
    }

    /// Serialise back to a map suitable for storage at `navigation.navbar`.
    pub fn to_config_value(&self) -> ConfigValue {
        let info = SourceInfo::default();
        let mut entries: Vec<ConfigMapEntry> = Vec::new();

        match &self.title {
            NavbarTitle::Default => {}
            NavbarTitle::Hidden => entries.push(ConfigMapEntry {
                key: "title".to_string(),
                key_source: info.clone(),
                value: ConfigValue::new_bool(false, info.clone()),
            }),
            NavbarTitle::Text(cv) => entries.push(ConfigMapEntry {
                key: "title".to_string(),
                key_source: info.clone(),
                value: cv.clone(),
            }),
        }

        push_optional_string(&mut entries, "logo", &self.logo, &info);
        push_optional_string(&mut entries, "logo-alt", &self.logo_alt, &info);
        push_optional_string(&mut entries, "logo-href", &self.logo_href, &info);
        push_optional_string(&mut entries, "background", &self.background, &info);
        push_optional_string(&mut entries, "foreground", &self.foreground, &info);

        // Booleans: emit only when non-default, to keep stored metadata tidy.
        if self.search {
            entries.push(bool_entry("search", true, &info));
        }
        if self.pinned {
            entries.push(bool_entry("pinned", true, &info));
        }
        if !self.collapse {
            entries.push(bool_entry("collapse", false, &info));
        }
        if self.tools_collapse {
            entries.push(bool_entry("tools-collapse", true, &info));
        }

        if self.collapse_below != CollapseBelow::default() {
            entries.push(string_entry(
                "collapse-below",
                self.collapse_below.as_str(),
                &info,
            ));
        }
        if self.toggle_position != TogglePosition::default() {
            entries.push(string_entry(
                "toggle-position",
                self.toggle_position.as_str(),
                &info,
            ));
        }

        if !self.left.is_empty() {
            entries.push(item_list_entry("left", &self.left, &info));
        }
        if !self.right.is_empty() {
            entries.push(item_list_entry("right", &self.right, &info));
        }

        ConfigValue::new_map(entries, info)
    }
}

/// Resolve the user's `navbar:` input from `ast.meta`.
///
/// Accepts both authoring locations and merges them:
///
/// - **Top-level** `navbar:` (feature-scoped — works for single-doc
///   renders, and absorbs document-frontmatter contributions).
/// - **Nested** `website.navbar:` (Quarto 1 compatible).
///
/// When both are present the top-level form wins on overlapping
/// fields, matching the precedence baked into
/// `quarto_core::transforms::config::resolve_website_bool` for
/// boolean website flags. `!prefer` on either layer escapes the
/// default field-wise merge.
///
/// Returns `None` when the merged value is absent (no `navbar` key in
/// either location) or explicitly disabled (`navbar: false`). Returns
/// `Some(Navbar)` for the object form.
pub fn resolve_navbar(meta: &ConfigValue) -> Option<Navbar> {
    let cv = resolve_website_value(meta, "navbar")?;
    if cv.as_bool() == Some(false) {
        return None;
    }
    if cv.as_bool() == Some(true) {
        // `navbar: true` with no content is not meaningful — the plan drops
        // this shorthand. Treat as absent.
        return None;
    }
    Some(Navbar::from_config_value(&cv))
}

fn parse_item_list(cv: Option<&ConfigValue>) -> Vec<NavigationItem> {
    let Some(cv) = cv else {
        return Vec::new();
    };
    let Some(arr) = cv.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(NavigationItem::from_config_value)
        .collect()
}

fn push_optional_string(
    entries: &mut Vec<ConfigMapEntry>,
    key: &str,
    value: &Option<String>,
    info: &SourceInfo,
) {
    if let Some(v) = value {
        entries.push(string_entry(key, v, info));
    }
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

fn item_list_entry(key: &str, items: &[NavigationItem], info: &SourceInfo) -> ConfigMapEntry {
    let values: Vec<ConfigValue> = items.iter().map(NavigationItem::to_config_value).collect();
    ConfigMapEntry {
        key: key.to_string(),
        key_source: info.clone(),
        value: ConfigValue::new_array(values, info.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        let info = SourceInfo::default();
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
        ConfigValue::new_string(x, SourceInfo::default())
    }

    fn b(x: bool) -> ConfigValue {
        ConfigValue::new_bool(x, SourceInfo::default())
    }

    fn arr(items: Vec<ConfigValue>) -> ConfigValue {
        ConfigValue::new_array(items, SourceInfo::default())
    }

    #[test]
    fn resolve_returns_none_for_absent() {
        let meta = map(vec![]);
        assert!(resolve_navbar(&meta).is_none());
    }

    #[test]
    fn resolve_returns_none_for_false() {
        let meta = map(vec![("navbar", b(false))]);
        assert!(resolve_navbar(&meta).is_none());
    }

    #[test]
    fn resolve_returns_none_for_bare_true() {
        let meta = map(vec![("navbar", b(true))]);
        // `navbar: true` with no config is not meaningful; treat as absent.
        assert!(resolve_navbar(&meta).is_none());
    }

    #[test]
    fn resolve_parses_full_object() {
        let navbar_cv = map(vec![
            ("title", s("My Site")),
            ("background", s("primary")),
            ("search", b(true)),
            ("collapse-below", s("xl")),
            ("toggle-position", s("right")),
            ("pinned", b(true)),
            (
                "left",
                arr(vec![
                    s("index.qmd"),
                    map(vec![("text", s("About")), ("href", s("about.qmd"))]),
                ]),
            ),
            (
                "right",
                arr(vec![map(vec![
                    ("icon", s("github")),
                    ("href", s("https://github.com/")),
                ])]),
            ),
        ]);
        let meta = map(vec![("navbar", navbar_cv)]);
        let nav = resolve_navbar(&meta).unwrap();

        match &nav.title {
            NavbarTitle::Text(cv) => {
                assert_eq!(cv.as_plain_text().as_deref(), Some("My Site"));
            }
            other => panic!("expected Text title, got {:?}", other),
        }
        assert_eq!(nav.background.as_deref(), Some("primary"));
        assert!(nav.search);
        assert!(nav.pinned);
        assert_eq!(nav.collapse_below, CollapseBelow::Xl);
        assert_eq!(nav.toggle_position, TogglePosition::Right);
        assert_eq!(nav.left.len(), 2);
        assert_eq!(nav.left[0].href.as_deref(), Some("index.qmd"));
        assert_eq!(nav.left[1].href.as_deref(), Some("about.qmd"));
        assert_eq!(nav.right.len(), 1);
        assert_eq!(nav.right[0].icon.as_deref(), Some("github"));
    }

    #[test]
    fn title_false_is_hidden() {
        let meta = map(vec![("navbar", map(vec![("title", b(false))]))]);
        let nav = resolve_navbar(&meta).unwrap();
        assert!(matches!(nav.title, NavbarTitle::Hidden));
    }

    #[test]
    fn title_true_is_default() {
        let meta = map(vec![("navbar", map(vec![("title", b(true))]))]);
        let nav = resolve_navbar(&meta).unwrap();
        assert!(matches!(nav.title, NavbarTitle::Default));
    }

    #[test]
    fn defaults_applied() {
        let meta = map(vec![("navbar", map(vec![]))]);
        let nav = resolve_navbar(&meta).unwrap();
        assert!(matches!(nav.title, NavbarTitle::Default));
        assert_eq!(nav.collapse, true);
        assert_eq!(nav.collapse_below, CollapseBelow::Lg);
        assert_eq!(nav.toggle_position, TogglePosition::Left);
        assert!(!nav.search);
        assert!(!nav.pinned);
    }

    // ---- bd-jjep / bd-telo: accept `website.navbar` form as well ----

    #[test]
    fn resolve_picks_up_nested_website_navbar() {
        // Quarto 1 compatible form: navbar under `website:`.
        // Before the fix this returned None; the navbar silently never
        // rendered when authors used this (more common) layout.
        let navbar_cv = map(vec![
            ("logo", s("quarto.png")),
            (
                "left",
                arr(vec![map(vec![
                    ("text", s("Overview")),
                    ("href", s("index.qmd")),
                ])]),
            ),
        ]);
        let meta = map(vec![("website", map(vec![("navbar", navbar_cv)]))]);
        let nav = resolve_navbar(&meta).expect("website.navbar must resolve");
        assert_eq!(nav.logo.as_deref(), Some("quarto.png"));
        assert_eq!(nav.left.len(), 1);
        assert_eq!(nav.left[0].href.as_deref(), Some("index.qmd"));
    }

    #[test]
    fn resolve_merges_top_level_over_website_navbar() {
        // website.navbar.logo = nested.png, navbar.logo = top.png
        // top-level wins on overlap; non-overlapping fields from
        // website.navbar are preserved.
        let nested = map(vec![
            ("logo", s("nested.png")),
            ("background", s("primary")),
        ]);
        let top = map(vec![("logo", s("top.png"))]);
        let meta = map(vec![
            ("website", map(vec![("navbar", nested)])),
            ("navbar", top),
        ]);
        let nav = resolve_navbar(&meta).unwrap();
        assert_eq!(
            nav.logo.as_deref(),
            Some("top.png"),
            "top-level logo must win"
        );
        assert_eq!(
            nav.background.as_deref(),
            Some("primary"),
            "non-overlapping nested field must survive"
        );
    }

    #[test]
    fn resolve_returns_none_for_nested_false() {
        // `website.navbar: false` should also disable, matching the
        // top-level affirmative-disable semantics.
        let meta = map(vec![("website", map(vec![("navbar", b(false))]))]);
        assert!(resolve_navbar(&meta).is_none());
    }

    #[test]
    fn resolve_top_level_false_overrides_nested_navbar() {
        // Top-level wins, so `navbar: false` disables even when
        // `website.navbar: { ... }` is configured.
        let nested = map(vec![("logo", s("nested.png"))]);
        let meta = map(vec![
            ("website", map(vec![("navbar", nested)])),
            ("navbar", b(false)),
        ]);
        assert!(resolve_navbar(&meta).is_none());
    }

    #[test]
    fn roundtrip_preserves_fields() {
        let original = Navbar {
            title: NavbarTitle::Text(s("Home")),
            background: Some("primary".to_string()),
            search: true,
            collapse_below: CollapseBelow::Xl,
            left: vec![NavigationItem {
                href: Some("index.qmd".to_string()),
                text: Some(s("Home")),
                ..NavigationItem::default()
            }],
            ..Navbar::with_defaults()
        };
        let cv = original.to_config_value();
        let reparsed = Navbar::from_config_value(&cv);
        assert_eq!(reparsed.background, original.background);
        assert_eq!(reparsed.search, original.search);
        assert_eq!(reparsed.collapse_below, original.collapse_below);
        assert_eq!(reparsed.left.len(), 1);
        assert_eq!(reparsed.left[0].href, original.left[0].href);
        match (&reparsed.title, &original.title) {
            (NavbarTitle::Text(a), NavbarTitle::Text(b)) => {
                assert_eq!(a.as_plain_text(), b.as_plain_text());
            }
            _ => panic!("title did not round-trip"),
        }
    }

    #[test]
    fn roundtrip_preserves_hidden_title() {
        let original = Navbar {
            title: NavbarTitle::Hidden,
            ..Navbar::with_defaults()
        };
        let cv = original.to_config_value();
        let reparsed = Navbar::from_config_value(&cv);
        assert!(matches!(reparsed.title, NavbarTitle::Hidden));
    }

    #[test]
    fn roundtrip_default_navbar_stays_default() {
        let original = Navbar::with_defaults();
        let cv = original.to_config_value();
        let reparsed = Navbar::from_config_value(&cv);
        assert_eq!(reparsed, original);
    }
}
