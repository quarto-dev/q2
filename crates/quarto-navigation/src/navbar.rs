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
use quarto_source_map::{By, SourceInfo};

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

    // Deliberate Option-returning parser; `FromStr` would force a `Result`/`Err`
    // type this lightweight enum doesn't need.
    #[allow(clippy::should_implement_trait)]
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

    // Deliberate Option-returning parser; `FromStr` would force a `Result`/`Err`
    // type this lightweight enum doesn't need.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            _ => None,
        }
    }
}

/// One resolved navbar-logo variant: a path, its optional alt text,
/// and the `SourceInfo` of the YAML scalar that authored the path
/// (bd-root-relative-paths-design-fc5pvkcv — mirroring
/// `logo_href_source`, bd-qor9a) so the resolver knows which YAML
/// file the path was authored in.
#[derive(Debug, Clone, PartialEq)]
pub struct LogoVariant {
    pub path: String,
    pub alt: Option<String>,
    pub source: SourceInfo,
}

/// Normalized navbar logo: always a light/dark pair, mirroring Q1's
/// `resolveLogo` normalization of the `logo-light-dark-specifier`
/// YAML shapes (bd-navbar-logo-unstyled-gbzd8vcu). A single-logo
/// spec (string or `{path, alt}`) fills both halves identically; a
/// `{light, dark}` spec with one half missing falls back to the
/// other. Brand.yml logo-name indirection is out of scope
/// (bd-v5z8w).
#[derive(Debug, Clone, PartialEq)]
pub struct NavbarLogo {
    pub light: LogoVariant,
    pub dark: LogoVariant,
}

impl NavbarLogo {
    /// Both halves render as one image: same path, same alt. (The
    /// renderer then emits a single unclassed `<img>` instead of a
    /// `light-content`/`dark-content` pair.)
    pub fn is_single(&self) -> bool {
        self.light.path == self.dark.path && self.light.alt == self.dark.alt
    }

    /// The shared path of a single-image logo, `None` when the
    /// variants differ.
    pub fn single_path(&self) -> Option<&str> {
        if self.is_single() {
            Some(&self.light.path)
        } else {
            None
        }
    }
}

/// Fully resolved navbar configuration.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Navbar {
    pub title: NavbarTitle,
    /// Normalized logo pair; `None` when unset or `logo: false`.
    /// Alt text lives per-variant (a sibling `logo-alt:` key fills
    /// variants that lack their own `alt`).
    pub logo: Option<NavbarLogo>,
    pub logo_href: Option<String>,
    /// `SourceInfo` of the YAML scalar that produced `logo_href`.
    /// bd-qor9a — paired with `logo_href` so the resolver knows which
    /// YAML file the brand link was authored in.
    pub logo_href_source: SourceInfo,
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
    /// Whether to render the dark-mode toggle in the navbar's tools
    /// slot. Not parsed from `navbar:` YAML — set by
    /// `NavbarGenerateTransform` when the format has a dark theme
    /// variant (bd-0pic6 A4; folds into general `tools:` support when
    /// bd-fod3 lands — bd-ld-toggle-into-tools-hpae7m9r).
    pub dark_mode_toggle: bool,
}

impl Navbar {
    /// Build a `Navbar` with Quarto 1-matched defaults.
    pub fn with_defaults() -> Self {
        Self {
            title: NavbarTitle::Default,
            logo: None,
            logo_href: None,
            logo_href_source: SourceInfo::generated(By::programmatic_config()),
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
            dark_mode_toggle: false,
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

        let logo_alt = cv.get("logo-alt").and_then(|v| v.as_plain_text());
        if let Some(logo_cv) = cv.get("logo") {
            nav.logo = parse_logo(logo_cv, logo_alt.as_deref());
        }
        if let Some(logo_href_cv) = cv.get("logo-href") {
            nav.logo_href = logo_href_cv.as_plain_text();
            if nav.logo_href.is_some() {
                nav.logo_href_source = logo_href_cv.source_info.clone();
            }
        }
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

        // Internal round-trip field (not authored YAML): written by
        // `to_config_value` so the flag survives the
        // generate-transform → metadata → render-transform trip.
        if let Some(v) = cv.get("dark-mode-toggle").and_then(|v| v.as_bool()) {
            nav.dark_mode_toggle = v;
        }

        nav
    }

    /// Serialise back to a map suitable for storage at `navigation.navbar`.
    pub fn to_config_value(&self) -> ConfigValue {
        let info = SourceInfo::generated(By::programmatic_config());
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

        // logo round-trips each variant path's source_info
        // (bd-root-relative-paths-design-fc5pvkcv) so the generate-time
        // path resolver can locate the authoring YAML file. A single
        // logo re-emits the historical `logo: <path>` + `logo-alt:`
        // wire shape; distinct variants emit a `{light, dark}` map.
        if let Some(logo) = &self.logo {
            if logo.is_single() {
                entries.push(ConfigMapEntry {
                    key: "logo".to_string(),
                    key_source: info.clone(),
                    value: ConfigValue::new_string(&logo.light.path, logo.light.source.clone()),
                });
                push_optional_string(&mut entries, "logo-alt", &logo.light.alt, &info);
            } else {
                let variant_entry = |key: &str, v: &LogoVariant| ConfigMapEntry {
                    key: key.to_string(),
                    key_source: info.clone(),
                    value: logo_variant_to_config_value(v, &info),
                };
                entries.push(ConfigMapEntry {
                    key: "logo".to_string(),
                    key_source: info.clone(),
                    value: ConfigValue::new_map(
                        vec![
                            variant_entry("light", &logo.light),
                            variant_entry("dark", &logo.dark),
                        ],
                        info.clone(),
                    ),
                });
            }
        }
        // logo-href round-trips its source_info (bd-qor9a) so the
        // diagnostic surface can locate it back in the YAML.
        push_optional_string(
            &mut entries,
            "logo-href",
            &self.logo_href,
            &self.logo_href_source,
        );
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
        if self.dark_mode_toggle {
            entries.push(bool_entry("dark-mode-toggle", true, &info));
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

/// Parse the `logo:` value into a normalized light/dark pair.
///
/// Accepted shapes (Q1's `logo-light-dark-specifier`, minus brand.yml
/// logo-name indirection — bd-v5z8w): `false` (returns `None`), a
/// plain string, a `{path, alt}` map, or a `{light, dark}` map whose
/// halves are each a string or `{path, alt}`. A missing half falls
/// back to the other; `logo_alt` (the sibling `logo-alt:` key) fills
/// any variant that lacks its own `alt`.
fn parse_logo(cv: &ConfigValue, logo_alt: Option<&str>) -> Option<NavbarLogo> {
    if cv.as_bool().is_some() {
        // `logo: false` disables; `logo: true` is meaningless — no path.
        return None;
    }

    let fill_alt = |mut v: LogoVariant| {
        if v.alt.is_none() {
            v.alt = logo_alt.map(str::to_string);
        }
        v
    };

    // `{light, dark}` shape — take it whenever either key is present.
    let light = cv.get("light").and_then(parse_logo_variant);
    let dark = cv.get("dark").and_then(parse_logo_variant);
    if light.is_some() || dark.is_some() {
        let light = light.map(&fill_alt);
        let dark = dark.map(&fill_alt);
        // Cross-fallback: a missing half mirrors the other (Q1
        // resolveLogo semantics, unconditional in Q2 — we have no
        // brand to gate the dark half on).
        let (light, dark) = match (light, dark) {
            (Some(l), Some(d)) => (l, d),
            (Some(l), None) => (l.clone(), l),
            (None, Some(d)) => (d.clone(), d),
            (None, None) => unreachable!("guarded by is_some above"),
        };
        return Some(NavbarLogo { light, dark });
    }

    // String or `{path, alt}` shape: one image for both modes.
    let single = fill_alt(parse_logo_variant(cv)?);
    Some(NavbarLogo {
        light: single.clone(),
        dark: single,
    })
}

/// Parse one variant: a plain string or a `{path, alt}` map. The
/// variant's `source` is the path scalar's `SourceInfo`.
fn parse_logo_variant(cv: &ConfigValue) -> Option<LogoVariant> {
    if let Some(path_cv) = cv.get("path") {
        let path = path_cv.as_plain_text()?;
        return Some(LogoVariant {
            path,
            alt: cv.get("alt").and_then(|v| v.as_plain_text()),
            source: path_cv.source_info.clone(),
        });
    }
    let path = cv.as_plain_text()?;
    Some(LogoVariant {
        path,
        alt: None,
        source: cv.source_info.clone(),
    })
}

/// Serialize one variant for the `{light, dark}` wire shape: a plain
/// string (carrying the path's `SourceInfo`) when there is no alt, a
/// `{path, alt}` map otherwise.
fn logo_variant_to_config_value(v: &LogoVariant, info: &SourceInfo) -> ConfigValue {
    let path_value = ConfigValue::new_string(&v.path, v.source.clone());
    match &v.alt {
        None => path_value,
        Some(alt) => ConfigValue::new_map(
            vec![
                ConfigMapEntry {
                    key: "path".to_string(),
                    key_source: info.clone(),
                    value: path_value,
                },
                ConfigMapEntry {
                    key: "alt".to_string(),
                    key_source: info.clone(),
                    value: ConfigValue::new_string(alt, info.clone()),
                },
            ],
            info.clone(),
        ),
    }
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

    fn arr(items: Vec<ConfigValue>) -> ConfigValue {
        ConfigValue::new_array(items, SourceInfo::for_test())
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
        assert!(nav.collapse);
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
        assert_eq!(
            nav.logo.as_ref().and_then(|l| l.single_path()),
            Some("quarto.png")
        );
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
            nav.logo.as_ref().and_then(|l| l.single_path()),
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

    /// Case A (bd-root-relative-paths-design-fc5pvkcv): a
    /// string-form `logo` is paired with the YAML scalar's
    /// `SourceInfo` — like `logo-href` (bd-qor9a) — so the
    /// generate-transform can resolve a frontmatter-authored logo
    /// path against the authoring file. Capture and round-trip both
    /// directions. (The distinct-variant case is covered by
    /// `logo_variant_sources_captured_and_round_tripped`.)
    #[test]
    fn logo_source_captured_and_round_tripped() {
        use quarto_source_map::FileId;
        let logo_loc = SourceInfo::original(FileId(7), 4, 19);
        let navbar_cv = map(vec![(
            "logo",
            ConfigValue::new_string("images/logo.svg", logo_loc.clone()),
        )]);
        let meta = map(vec![("navbar", navbar_cv)]);
        let nav = resolve_navbar(&meta).expect("navbar must resolve");
        let logo = nav.logo.as_ref().unwrap();
        assert_eq!(logo.single_path(), Some("images/logo.svg"));
        assert_eq!(
            logo.light.source, logo_loc,
            "the variant source must capture the YAML scalar's SourceInfo"
        );

        let reparsed = Navbar::from_config_value(&nav.to_config_value());
        assert_eq!(
            reparsed.logo.as_ref().unwrap().light.source,
            logo_loc,
            "the variant source must survive the to/from round-trip"
        );
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

    // === Logo light/dark variants (bd-navbar-logo-unstyled-gbzd8vcu) ====
    //
    // Q1's `logo-light-dark-specifier` accepts `false` | string |
    // `{path, alt}` | `{light, dark}` (each variant string or
    // `{path, alt}`), and `resolveLogo` normalizes every shape to a
    // light/dark pair with cross-fallback. These tests pin the same
    // normalization for Q2 (minus brand.yml indirection — bd-v5z8w).

    #[test]
    fn logo_string_parses_as_single_pair() {
        let navbar_cv = map(vec![("logo", s("quarto.png")), ("logo-alt", s("Q logo"))]);
        let meta = map(vec![("navbar", navbar_cv)]);
        let nav = resolve_navbar(&meta).unwrap();
        let logo = nav.logo.as_ref().expect("logo must parse");
        assert_eq!(logo.light.path, "quarto.png");
        assert_eq!(logo.dark.path, "quarto.png");
        assert_eq!(logo.light.alt.as_deref(), Some("Q logo"));
        assert_eq!(logo.dark.alt.as_deref(), Some("Q logo"));
        assert!(logo.is_single(), "identical halves must read as single");
        assert_eq!(logo.single_path(), Some("quarto.png"));
    }

    #[test]
    fn logo_path_alt_object_parses_as_single_pair() {
        let navbar_cv = map(vec![(
            "logo",
            map(vec![("path", s("img/p.svg")), ("alt", s("Alt text"))]),
        )]);
        let meta = map(vec![("navbar", navbar_cv)]);
        let nav = resolve_navbar(&meta).unwrap();
        let logo = nav.logo.as_ref().expect("logo must parse");
        assert!(logo.is_single());
        assert_eq!(logo.light.path, "img/p.svg");
        assert_eq!(logo.light.alt.as_deref(), Some("Alt text"));
        assert_eq!(logo.dark.alt.as_deref(), Some("Alt text"));
    }

    #[test]
    fn logo_object_alt_wins_over_logo_alt_key() {
        // The object's own `alt` is more specific than the sibling
        // `logo-alt` key.
        let navbar_cv = map(vec![
            ("logo", map(vec![("path", s("p.svg")), ("alt", s("inner"))])),
            ("logo-alt", s("outer")),
        ]);
        let meta = map(vec![("navbar", navbar_cv)]);
        let nav = resolve_navbar(&meta).unwrap();
        let logo = nav.logo.as_ref().unwrap();
        assert_eq!(logo.light.alt.as_deref(), Some("inner"));
    }

    #[test]
    fn logo_light_dark_distinct_variants() {
        let navbar_cv = map(vec![
            (
                "logo",
                map(vec![
                    ("light", s("l.svg")),
                    ("dark", map(vec![("path", s("d.svg")), ("alt", s("Dark"))])),
                ]),
            ),
            ("logo-alt", s("Generic")),
        ]);
        let meta = map(vec![("navbar", navbar_cv)]);
        let nav = resolve_navbar(&meta).unwrap();
        let logo = nav.logo.as_ref().expect("logo must parse");
        assert!(!logo.is_single());
        assert_eq!(logo.single_path(), None);
        assert_eq!(logo.light.path, "l.svg");
        assert_eq!(logo.dark.path, "d.svg");
        // Per-variant alt wins; `logo-alt` fills variants that lack one.
        assert_eq!(logo.dark.alt.as_deref(), Some("Dark"));
        assert_eq!(logo.light.alt.as_deref(), Some("Generic"));
    }

    #[test]
    fn logo_light_only_falls_back_to_dark() {
        let navbar_cv = map(vec![("logo", map(vec![("light", s("l.svg"))]))]);
        let meta = map(vec![("navbar", navbar_cv)]);
        let nav = resolve_navbar(&meta).unwrap();
        let logo = nav.logo.as_ref().expect("logo must parse");
        assert!(logo.is_single(), "missing dark must fall back to light");
        assert_eq!(logo.dark.path, "l.svg");
    }

    #[test]
    fn logo_dark_only_falls_back_to_light() {
        let navbar_cv = map(vec![("logo", map(vec![("dark", s("d.svg"))]))]);
        let meta = map(vec![("navbar", navbar_cv)]);
        let nav = resolve_navbar(&meta).unwrap();
        let logo = nav.logo.as_ref().expect("logo must parse");
        assert!(logo.is_single(), "missing light must fall back to dark");
        assert_eq!(logo.light.path, "d.svg");
    }

    #[test]
    fn logo_false_is_none() {
        let navbar_cv = map(vec![("logo", b(false)), ("title", s("Site"))]);
        let meta = map(vec![("navbar", navbar_cv)]);
        let nav = resolve_navbar(&meta).unwrap();
        assert!(nav.logo.is_none(), "logo: false must disable the logo");
    }

    #[test]
    fn logo_empty_map_is_none() {
        let navbar_cv = map(vec![("logo", map(vec![])), ("title", s("Site"))]);
        let meta = map(vec![("navbar", navbar_cv)]);
        let nav = resolve_navbar(&meta).unwrap();
        assert!(nav.logo.is_none(), "an empty logo map carries no path");
    }

    #[test]
    fn logo_single_roundtrips_as_string_wire_shape() {
        // A single logo re-emits the historical wire shape
        // (`logo: <path>` + `logo-alt:`), so stored metadata stays
        // stable for single-logo sites.
        let navbar_cv = map(vec![("logo", s("quarto.png")), ("logo-alt", s("Q"))]);
        let meta = map(vec![("navbar", navbar_cv)]);
        let nav = resolve_navbar(&meta).unwrap();

        let cv = nav.to_config_value();
        assert_eq!(
            cv.get("logo").and_then(|v| v.as_plain_text()).as_deref(),
            Some("quarto.png"),
            "single logo must serialize as a plain string"
        );
        assert_eq!(
            cv.get("logo-alt")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("Q")
        );

        let reparsed = Navbar::from_config_value(&cv);
        assert_eq!(reparsed.logo, nav.logo);
    }

    #[test]
    fn logo_variants_roundtrip_as_light_dark_map() {
        let navbar_cv = map(vec![(
            "logo",
            map(vec![
                ("light", map(vec![("path", s("l.svg")), ("alt", s("L"))])),
                ("dark", s("d.svg")),
            ]),
        )]);
        let meta = map(vec![("navbar", navbar_cv)]);
        let nav = resolve_navbar(&meta).unwrap();
        let cv = nav.to_config_value();

        let logo_cv = cv.get("logo").expect("logo entry");
        assert_eq!(
            logo_cv
                .get("light")
                .and_then(|l| l.get("path"))
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("l.svg"),
            "distinct variants must serialize as a light/dark map"
        );

        let reparsed = Navbar::from_config_value(&cv);
        assert_eq!(reparsed.logo, nav.logo);
    }

    /// Case A (bd-root-relative-paths-design-fc5pvkcv), extended to
    /// variants: each variant's `path` keeps the `SourceInfo` of the
    /// YAML scalar that authored it, and both survive the round-trip,
    /// so the generate-transform can resolve each against its own
    /// authoring file.
    #[test]
    fn logo_variant_sources_captured_and_round_tripped() {
        use quarto_source_map::FileId;
        let light_loc = SourceInfo::original(FileId(7), 4, 19);
        let dark_loc = SourceInfo::original(FileId(9), 6, 21);
        let navbar_cv = map(vec![(
            "logo",
            map(vec![
                (
                    "light",
                    ConfigValue::new_string("images/l.svg", light_loc.clone()),
                ),
                (
                    "dark",
                    ConfigValue::new_string("images/d.svg", dark_loc.clone()),
                ),
            ]),
        )]);
        let meta = map(vec![("navbar", navbar_cv)]);
        let nav = resolve_navbar(&meta).expect("navbar must resolve");
        let logo = nav.logo.as_ref().unwrap();
        assert_eq!(
            logo.light.source, light_loc,
            "light path must capture its YAML scalar's SourceInfo"
        );
        assert_eq!(
            logo.dark.source, dark_loc,
            "dark path must capture its YAML scalar's SourceInfo"
        );

        let reparsed = Navbar::from_config_value(&nav.to_config_value());
        let relogo = reparsed.logo.as_ref().unwrap();
        assert_eq!(
            relogo.light.source, light_loc,
            "light source must survive the to/from round-trip"
        );
        assert_eq!(
            relogo.dark.source, dark_loc,
            "dark source must survive the to/from round-trip"
        );
    }
}
