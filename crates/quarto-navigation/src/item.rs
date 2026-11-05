/*
 * item.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! The shared [`NavigationItem`] shape used by navbar left/right lists,
//! navbar dropdown menus, and page-footer regions.
//!
//! Parsing accepts three YAML shapes:
//!
//! 1. A bare scalar string, treated as a file path: `- about.qmd` →
//!    `NavigationItem { href: Some("about.qmd"), .. }`.
//! 2. A map with any of the supported keys (`href` / `file`, `text`, `icon`,
//!    `aria-label`, `rel`, `target`, `menu`).
//! 3. A map with only a `menu` (a dropdown with no direct href).
//!
//! Text fields are retained as `ConfigValue` so the caller can distinguish
//! markdown-parsed inlines (the default in document-metadata context) from
//! literal strings (the default in project-config context). Renderers use
//! `as_plain_text()` or walk the inlines themselves.

use quarto_pandoc_types::ConfigMapEntry;
use quarto_pandoc_types::config_value::{ConfigValue, ConfigValueKind};
use quarto_source_map::{By, SourceInfo};
use yaml_rust2::Yaml;

use crate::breakpoint::Breakpoint;

/// A single navigation item — a link, an icon button, or a submenu.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NavigationItem {
    /// Target URL or project-relative path (Quarto 1's `href` / `file` alias).
    /// Absent for pure submenus.
    pub href: Option<String>,

    /// `SourceInfo` of the YAML scalar that produced `href`.
    ///
    /// bd-qor9a — the navigation parsers retain the source location
    /// so Generate transforms can resolve href values relative to the
    /// file they were authored in (frontmatter sidebars resolve against
    /// the doc's directory; `_quarto.yml` sidebars against the project
    /// root). `SourceInfo::generated(By::programmatic_config())` for programmatically-constructed
    /// items (tests, in-memory builders) — the path resolution helper
    /// short-circuits on that case and treats the href as already
    /// project-root-relative.
    pub href_source: SourceInfo,

    /// Display text. Preserved as `ConfigValue` so markdown inlines from
    /// document metadata survive without being flattened.
    ///
    /// When the author wrote breakpoint-keyed text (`text: {lg: Docs,
    /// xl: Documentation}`), this holds the *canonical* text: the
    /// widest non-blank variant. Consumers that need a single label
    /// (enrichment, search breadcrumbs, sidebars, footers) read it and
    /// never see the map. Only the navbar renderer reads
    /// [`text_variants`](Self::text_variants).
    pub text: Option<ConfigValue>,

    /// Breakpoint-keyed text variants (bd-navbar-responsive-text-am98rw95),
    /// sorted ascending by breakpoint, at most one per breakpoint.
    /// Empty when `text:` was a scalar or absent. Each variant applies
    /// from its breakpoint up to the next one; the lowest also covers
    /// every narrower width. A blank variant (`""`) means "no text in
    /// this range" (the icon-only idiom).
    ///
    /// Round-trips through `to_config_value` as the authored map, and
    /// `text` is re-derived from it on every parse.
    pub text_variants: Vec<(Breakpoint, ConfigValue)>,

    /// Bootstrap icon name (e.g. `github`, `bluesky`).
    pub icon: Option<String>,

    /// Accessibility label, mirroring the HTML `aria-label` attribute.
    pub aria_label: Option<String>,

    /// HTML `rel` attribute.
    pub rel: Option<String>,

    /// HTML `target` attribute (e.g. `_blank`).
    pub target: Option<String>,

    /// Nested menu items. Only populated for dropdown entries; typically empty.
    pub menu: Vec<NavigationItem>,

    /// For a page-footer item authored as a *bare scalar*: the original
    /// `ConfigValue`, retained so the item can fall back to display text
    /// when its provisional `href` turns out not to name a project
    /// document (bd-page-footer-items-f4th80mj, defect 2 — see
    /// [`BareScalar::TextIfUnresolved`]).
    ///
    /// `None` for every other shape, including navbar/sidebar bare
    /// scalars, which are unconditionally hrefs. Boxed to keep
    /// `NavigationItem` small — this field is set on a minority of items.
    ///
    /// Consumed by the footer transform's demotion pass; it does not
    /// roundtrip through `to_config_value` because demotion happens
    /// before serialization.
    pub bare_text: Option<Box<ConfigValue>>,

    /// Whether this item represents the currently-rendered page.
    /// Set by the `mark_active` pass in Generate transforms; read by
    /// Render transforms when emitting the `active` class on the anchor.
    /// Does not roundtrip through YAML input (users don't author this),
    /// but does roundtrip through the generate→render handoff via
    /// `to_config_value` / `from_config_value`.
    pub active: bool,
}

/// How a *bare scalar* item (`- something`) should be read.
///
/// The two navigation regions disagree, and both readings are right for
/// their context (bd-page-footer-items-f4th80mj, defect 2):
///
/// - Navbars and sidebars: `- about.qmd` always means "link to this
///   page", so a bare scalar is an **href**.
/// - Page-footer regions: Q1 decides by *resolution*. A bare scalar that
///   names a project document becomes a link carrying that document's
///   title; anything else — `Copyright © 2026 …`, a bare external URL,
///   a path that matches no document — is display text. Measured
///   against Q1 (see the plan's "Q1's bare-scalar rule" section).
///
/// Because that decision needs the project index, the parser cannot make
/// it. [`BareScalar::TextIfUnresolved`] therefore records the scalar in
/// [`NavigationItem::bare_text`] *and* keeps it as a provisional href, so
/// enrichment can run normally; the footer transform then demotes any
/// item enrichment failed to resolve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BareScalar {
    /// A bare scalar is always a link target (navbar, sidebar).
    #[default]
    Href,
    /// A bare scalar is a link target if it resolves to a project
    /// document, and display text otherwise (page-footer regions).
    TextIfUnresolved,
}

impl NavigationItem {
    /// Parse a single item from a `ConfigValue` in one of the three accepted
    /// shapes. Returns `None` if the shape is unrecognisable (e.g. a number).
    ///
    /// Bare scalars are read as hrefs; see
    /// [`from_config_value_with`](Self::from_config_value_with) for the
    /// page-footer reading.
    pub fn from_config_value(cv: &ConfigValue) -> Option<Self> {
        Self::from_config_value_with(cv, BareScalar::Href)
    }

    /// Like [`from_config_value`](Self::from_config_value), but with an
    /// explicit policy for bare scalars.
    pub fn from_config_value_with(cv: &ConfigValue, bare: BareScalar) -> Option<Self> {
        // Bare form: `- about.qmd` (href) or `- Copyright © …` (text).
        // The source_info travels with the bare ConfigValue itself (no
        // wrapping map node).
        if let Some(s) = cv.as_plain_text() {
            // Only a scalar-ish shape reaches here — `as_plain_text`
            // already narrows out maps and arrays.
            return Some(NavigationItem {
                href: Some(s),
                href_source: cv.source_info.clone(),
                // Keep the whole `ConfigValue`, not the flattened string:
                // by this point `ConfigMarkdownTransform` may have turned
                // it into `PandocInlines`, and `render_text` needs those.
                bare_text: match bare {
                    BareScalar::Href => None,
                    BareScalar::TextIfUnresolved => Some(Box::new(cv.clone())),
                },
                ..NavigationItem::default()
            });
        }

        // Object form. Every field is optional. `href` is also keyed by
        // `file:` (Q1 alias); whichever wins, we capture *that* value's
        // source_info — the one the user wrote.
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

        let (text, text_variants) = match cv.get("text") {
            Some(t) if t.is_map() => {
                let variants = parse_text_variants(t);
                (canonical_text(&variants).cloned(), variants)
            }
            other => (other.cloned(), Vec::new()),
        };
        let icon = cv.get("icon").and_then(|v| v.as_plain_text());
        let aria_label = cv.get("aria-label").and_then(|v| v.as_plain_text());
        let rel = cv.get("rel").and_then(|v| v.as_plain_text());
        let target = cv.get("target").and_then(|v| v.as_plain_text());

        let menu = cv
            .get("menu")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(NavigationItem::from_config_value)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        // `active` roundtrips through the Generate→Render handoff
        // (a Generate transform writes `active: true` into the
        // navigation subtree; Render parses it back here). Absent by
        // default for user-authored YAML.
        let active = cv.get("active").and_then(|v| v.as_bool()).unwrap_or(false);

        // If we didn't recognise any fields, reject.
        if href.is_none()
            && text.is_none()
            && icon.is_none()
            && aria_label.is_none()
            && rel.is_none()
            && target.is_none()
            && menu.is_empty()
            && !active
        {
            return None;
        }

        Some(NavigationItem {
            href,
            href_source,
            text,
            text_variants,
            icon,
            aria_label,
            rel,
            target,
            menu,
            active,
            // Object form: the author wrote explicit keys, so there is no
            // bare scalar to fall back to.
            bare_text: None,
        })
    }

    /// Re-derive [`text`](Self::text) from
    /// [`text_variants`](Self::text_variants) after the variants were
    /// edited in place (e.g. link rewriting). No-op for scalar text.
    pub fn refresh_canonical_text(&mut self) {
        if !self.text_variants.is_empty() {
            self.text = canonical_text(&self.text_variants).cloned();
        }
    }

    /// Serialise back to a `ConfigValue` map suitable for storing at
    /// `navigation.navbar` / `navigation.footer`. Empty fields are omitted.
    ///
    /// `href`'s `SourceInfo` is round-tripped from `self.href_source` so
    /// the Generate → Render handoff preserves the original YAML
    /// location. Other fields use `SourceInfo::generated(By::programmatic_config())` — they are
    /// either programmatic (`active`) or unused for diagnostic
    /// location today.
    pub fn to_config_value(&self) -> ConfigValue {
        let source_info = SourceInfo::generated(By::programmatic_config());
        let mut entries: Vec<ConfigMapEntry> = Vec::new();

        if let Some(ref href) = self.href {
            entries.push(ConfigMapEntry {
                key: "href".to_string(),
                key_source: source_info.clone(),
                value: ConfigValue::new_string(href, self.href_source.clone()),
            });
        }
        if !self.text_variants.is_empty() {
            let variant_entries = self
                .text_variants
                .iter()
                .map(|(bp, value)| ConfigMapEntry {
                    key: bp.as_str().to_string(),
                    key_source: source_info.clone(),
                    value: value.clone(),
                })
                .collect();
            entries.push(ConfigMapEntry {
                key: "text".to_string(),
                key_source: source_info.clone(),
                value: ConfigValue::new_map(variant_entries, source_info.clone()),
            });
        } else if let Some(ref text) = self.text {
            entries.push(ConfigMapEntry {
                key: "text".to_string(),
                key_source: source_info.clone(),
                value: text.clone(),
            });
        }
        if let Some(ref icon) = self.icon {
            entries.push(ConfigMapEntry {
                key: "icon".to_string(),
                key_source: source_info.clone(),
                value: ConfigValue::new_string(icon, source_info.clone()),
            });
        }
        if let Some(ref aria) = self.aria_label {
            entries.push(ConfigMapEntry {
                key: "aria-label".to_string(),
                key_source: source_info.clone(),
                value: ConfigValue::new_string(aria, source_info.clone()),
            });
        }
        if let Some(ref rel) = self.rel {
            entries.push(ConfigMapEntry {
                key: "rel".to_string(),
                key_source: source_info.clone(),
                value: ConfigValue::new_string(rel, source_info.clone()),
            });
        }
        if let Some(ref target) = self.target {
            entries.push(ConfigMapEntry {
                key: "target".to_string(),
                key_source: source_info.clone(),
                value: ConfigValue::new_string(target, source_info.clone()),
            });
        }
        if !self.menu.is_empty() {
            let menu_values: Vec<ConfigValue> = self
                .menu
                .iter()
                .map(NavigationItem::to_config_value)
                .collect();
            entries.push(ConfigMapEntry {
                key: "menu".to_string(),
                key_source: source_info.clone(),
                value: ConfigValue::new_array(menu_values, source_info.clone()),
            });
        }
        // Only emit `active` when it's been set, to keep stored metadata
        // tidy and match the omit-default convention used elsewhere.
        if self.active {
            entries.push(ConfigMapEntry {
                key: "active".to_string(),
                key_source: source_info.clone(),
                value: ConfigValue::new_bool(true, source_info.clone()),
            });
        }

        ConfigValue::new_map(entries, source_info)
    }
}

/// Read a map-valued `text:` into breakpoint variants. Unknown keys are
/// skipped here (the generate transform warns about them via
/// [`responsive_text_warnings`]); a repeated key keeps its last value,
/// matching YAML-map override semantics.
fn parse_text_variants(map: &ConfigValue) -> Vec<(Breakpoint, ConfigValue)> {
    let mut variants: Vec<(Breakpoint, ConfigValue)> = Vec::new();
    for entry in map.as_map_entries().unwrap_or_default() {
        let Some(bp) = Breakpoint::from_str(&entry.key) else {
            continue;
        };
        variants.retain(|(existing, _)| *existing != bp);
        variants.push((bp, entry.value.clone()));
    }
    variants.sort_by_key(|(bp, _)| *bp);
    variants
}

/// The widest non-blank variant, or `None` when every variant is blank.
fn canonical_text(variants: &[(Breakpoint, ConfigValue)]) -> Option<&ConfigValue> {
    variants
        .iter()
        .rev()
        .map(|(_, v)| v)
        .find(|v| !is_blank_text(v))
}

/// Whether a text value renders as nothing: null, an empty/whitespace
/// string, or empty parsed markdown. Decided by kind rather than by
/// plain-text extraction, so an image-only label (plain text `""`) is
/// *not* blank.
pub fn is_blank_text(cv: &ConfigValue) -> bool {
    match &cv.value {
        ConfigValueKind::Scalar {
            yaml: Yaml::Null, ..
        } => true,
        ConfigValueKind::Scalar {
            yaml: Yaml::String(s),
            ..
        } => s.trim().is_empty(),
        ConfigValueKind::PandocInlines(inlines) => inlines.is_empty(),
        ConfigValueKind::PandocBlocks(blocks) => blocks.is_empty(),
        _ => false,
    }
}

/// A problem with an authored breakpoint-keyed `text:` map.
#[derive(Debug, Clone, PartialEq)]
pub enum ResponsiveTextWarning {
    /// A key that is not a breakpoint name. Its text is ignored.
    UnknownBreakpoint { key: String, location: SourceInfo },
    /// A `text:` map with no breakpoint keys at all, so the item has
    /// no authored text.
    NoBreakpoints { location: SourceInfo },
}

/// Check every map-valued `text:` in a list of *authored* navigation
/// items (and their nested `menu:` items). Run on the raw config, not
/// on parsed [`NavigationItem`]s, because parsing drops the unknown
/// keys and their source locations.
pub fn responsive_text_warnings(items: &ConfigValue) -> Vec<ResponsiveTextWarning> {
    let mut warnings = Vec::new();
    collect_text_warnings(items, &mut warnings);
    warnings
}

fn collect_text_warnings(items: &ConfigValue, warnings: &mut Vec<ResponsiveTextWarning>) {
    for item in items.as_array().unwrap_or_default() {
        if let Some(text) = item.get("text")
            && let Some(entries) = text.as_map_entries()
        {
            let mut any_known = false;
            for entry in entries {
                if Breakpoint::from_str(&entry.key).is_some() {
                    any_known = true;
                } else {
                    warnings.push(ResponsiveTextWarning::UnknownBreakpoint {
                        key: entry.key.clone(),
                        location: entry.key_source.clone(),
                    });
                }
            }
            if !any_known {
                warnings.push(ResponsiveTextWarning::NoBreakpoints {
                    location: text.source_info.clone(),
                });
            }
        }
        if let Some(menu) = item.get("menu") {
            collect_text_warnings(menu, warnings);
        }
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

    #[test]
    fn short_form_is_treated_as_href() {
        let item = NavigationItem::from_config_value(&s("about.qmd")).unwrap();
        assert_eq!(item.href.as_deref(), Some("about.qmd"));
        assert!(item.text.is_none());
        assert!(item.menu.is_empty());
    }

    #[test]
    fn object_form_parses_all_fields() {
        let cv = map(vec![
            ("href", s("about.qmd")),
            ("text", s("About")),
            ("icon", s("info")),
            ("aria-label", s("About page")),
            ("rel", s("me")),
            ("target", s("_blank")),
        ]);
        let item = NavigationItem::from_config_value(&cv).unwrap();
        assert_eq!(item.href.as_deref(), Some("about.qmd"));
        assert_eq!(
            item.text.as_ref().unwrap().as_plain_text().as_deref(),
            Some("About")
        );
        assert_eq!(item.icon.as_deref(), Some("info"));
        assert_eq!(item.aria_label.as_deref(), Some("About page"));
        assert_eq!(item.rel.as_deref(), Some("me"));
        assert_eq!(item.target.as_deref(), Some("_blank"));
    }

    #[test]
    fn file_is_alias_for_href() {
        let cv = map(vec![("file", s("talks.qmd"))]);
        let item = NavigationItem::from_config_value(&cv).unwrap();
        assert_eq!(item.href.as_deref(), Some("talks.qmd"));
    }

    #[test]
    fn menu_parses_nested_items() {
        let menu_arr = ConfigValue::new_array(
            vec![
                s("sub1.qmd"),
                map(vec![("text", s("Sub Two")), ("href", s("sub2.qmd"))]),
            ],
            SourceInfo::for_test(),
        );
        let cv = map(vec![("text", s("Parent")), ("menu", menu_arr)]);
        let item = NavigationItem::from_config_value(&cv).unwrap();
        assert_eq!(item.menu.len(), 2);
        assert_eq!(item.menu[0].href.as_deref(), Some("sub1.qmd"));
        assert_eq!(item.menu[1].href.as_deref(), Some("sub2.qmd"));
        assert_eq!(
            item.menu[1]
                .text
                .as_ref()
                .unwrap()
                .as_plain_text()
                .as_deref(),
            Some("Sub Two")
        );
    }

    #[test]
    fn empty_map_is_rejected() {
        let cv = map(vec![]);
        assert!(NavigationItem::from_config_value(&cv).is_none());
    }

    #[test]
    fn roundtrip_preserves_basic_fields() {
        let original = NavigationItem {
            href: Some("index.qmd".to_string()),
            text: Some(s("Home")),
            icon: Some("house".to_string()),
            aria_label: Some("Home".to_string()),
            ..NavigationItem::default()
        };
        let cv = original.to_config_value();
        let reparsed = NavigationItem::from_config_value(&cv).unwrap();
        assert_eq!(reparsed.href, original.href);
        assert_eq!(reparsed.icon, original.icon);
        assert_eq!(reparsed.aria_label, original.aria_label);
        assert_eq!(
            reparsed.text.as_ref().unwrap().as_plain_text(),
            original.text.as_ref().unwrap().as_plain_text()
        );
        assert!(!reparsed.active);
    }

    // Phase 3 test 1 — `active` defaults to false for every factory path.
    #[test]
    fn navigation_item_default_has_inactive() {
        assert!(!NavigationItem::default().active);
        // Bare-string form.
        let from_bare = NavigationItem::from_config_value(&s("about.qmd")).unwrap();
        assert!(!from_bare.active);
        // Object-form without active: key.
        let from_obj =
            NavigationItem::from_config_value(&map(vec![("href", s("about.qmd"))])).unwrap();
        assert!(!from_obj.active);
        // Menu item.
        let menu = NavigationItem::from_config_value(&map(vec![
            ("text", s("Dropdown")),
            (
                "menu",
                ConfigValue::new_array(vec![s("sub.qmd")], SourceInfo::for_test()),
            ),
        ]))
        .unwrap();
        assert!(!menu.active);
        assert!(!menu.menu[0].active);
    }

    // Phase 3 test 2 — `to_config_value` emits `active: true` when set,
    // and `from_config_value` reads it back. This is the
    // Generate→Render handoff path.
    #[test]
    fn navigation_item_roundtrip_preserves_active() {
        let active_item = NavigationItem {
            href: Some("about.qmd".to_string()),
            text: Some(s("About")),
            active: true,
            ..NavigationItem::default()
        };
        let cv = active_item.to_config_value();
        // The stored ConfigValue carries `active: true`.
        assert_eq!(cv.get("active").and_then(|v| v.as_bool()), Some(true));
        let reparsed = NavigationItem::from_config_value(&cv).unwrap();
        assert!(reparsed.active);
    }

    // Phase 3 test 2 (cont.) — inactive items don't emit the `active`
    // key at all (tidy metadata; omit-default convention).
    #[test]
    fn navigation_item_inactive_omits_active_key() {
        let item = NavigationItem {
            href: Some("about.qmd".to_string()),
            text: Some(s("About")),
            ..NavigationItem::default()
        };
        let cv = item.to_config_value();
        assert!(cv.get("active").is_none());
    }

    #[test]
    fn roundtrip_preserves_menu() {
        let original = NavigationItem {
            text: Some(s("Docs")),
            menu: vec![
                NavigationItem {
                    href: Some("start.qmd".to_string()),
                    text: Some(s("Start")),
                    ..NavigationItem::default()
                },
                NavigationItem {
                    href: Some("ref.qmd".to_string()),
                    text: Some(s("Reference")),
                    ..NavigationItem::default()
                },
            ],
            ..NavigationItem::default()
        };
        let cv = original.to_config_value();
        let reparsed = NavigationItem::from_config_value(&cv).unwrap();
        assert_eq!(reparsed.menu.len(), 2);
        assert_eq!(reparsed.menu[0].href, original.menu[0].href);
        assert_eq!(reparsed.menu[1].href, original.menu[1].href);
    }

    // bd-navbar-responsive-text-am98rw95 — breakpoint-keyed `text:`.

    fn text_map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        map(vec![("href", s("docs.qmd")), ("text", map(entries))])
    }

    fn variant_keys(item: &NavigationItem) -> Vec<&'static str> {
        item.text_variants
            .iter()
            .map(|(bp, _)| bp.as_str())
            .collect()
    }

    #[test]
    fn text_map_parses_sorted_variants_and_canonical_text() {
        let item = NavigationItem::from_config_value(&text_map(vec![
            ("xl", s("Documentation")),
            ("lg", s("Docs")),
        ]))
        .unwrap();
        assert_eq!(variant_keys(&item), vec!["lg", "xl"]);
        // Canonical text is the widest variant.
        assert_eq!(
            item.text.unwrap().as_plain_text().as_deref(),
            Some("Documentation")
        );
    }

    #[test]
    fn text_map_canonical_skips_blank_widest_variant() {
        let item =
            NavigationItem::from_config_value(&text_map(vec![("xs", s("GitHub")), ("lg", s(""))]))
                .unwrap();
        assert_eq!(
            item.text.unwrap().as_plain_text().as_deref(),
            Some("GitHub")
        );
    }

    #[test]
    fn text_map_all_blank_has_no_canonical_text() {
        let item =
            NavigationItem::from_config_value(&text_map(vec![("xs", s("")), ("lg", s("  "))]))
                .unwrap();
        assert!(item.text.is_none());
        assert_eq!(item.text_variants.len(), 2);
    }

    #[test]
    fn text_map_ignores_unknown_keys() {
        let item =
            NavigationItem::from_config_value(&text_map(vec![("md2", s("X")), ("md", s("Y"))]))
                .unwrap();
        assert_eq!(variant_keys(&item), vec!["md"]);
        let none = NavigationItem::from_config_value(&text_map(vec![("wide", s("X"))])).unwrap();
        assert!(none.text.is_none());
        assert!(none.text_variants.is_empty());
    }

    #[test]
    fn text_map_repeated_key_keeps_last_value() {
        let item =
            NavigationItem::from_config_value(&text_map(vec![("lg", s("A")), ("lg", s("B"))]))
                .unwrap();
        assert_eq!(item.text_variants.len(), 1);
        assert_eq!(
            item.text_variants[0].1.as_plain_text().as_deref(),
            Some("B")
        );
    }

    #[test]
    fn scalar_text_has_no_variants() {
        let item = NavigationItem::from_config_value(&map(vec![("text", s("About"))])).unwrap();
        assert!(item.text_variants.is_empty());
    }

    #[test]
    fn roundtrip_preserves_text_variants() {
        let original = NavigationItem::from_config_value(&text_map(vec![
            ("xs", s("Documentation")),
            ("lg", s("Docs")),
            ("xl", s("Documentation")),
        ]))
        .unwrap();
        let cv = original.to_config_value();
        assert!(cv.get("text").unwrap().is_map());
        let reparsed = NavigationItem::from_config_value(&cv).unwrap();
        assert_eq!(variant_keys(&reparsed), vec!["xs", "lg", "xl"]);
        assert_eq!(
            reparsed.text.unwrap().as_plain_text().as_deref(),
            Some("Documentation")
        );
    }

    #[test]
    fn blank_text_is_decided_by_kind() {
        assert!(is_blank_text(&s("")));
        assert!(is_blank_text(&s(" ")));
        assert!(is_blank_text(&ConfigValue::null(SourceInfo::for_test())));
        assert!(is_blank_text(&ConfigValue::new_inlines(
            vec![],
            SourceInfo::for_test()
        )));
        assert!(!is_blank_text(&s("Docs")));
    }

    #[test]
    fn warnings_flag_unknown_keys_and_keyless_maps_including_menus() {
        let nested = map(vec![
            ("text", s("Parent")),
            (
                "menu",
                ConfigValue::new_array(
                    vec![text_map(vec![("wide", s("X"))])],
                    SourceInfo::for_test(),
                ),
            ),
        ]);
        let items = ConfigValue::new_array(
            vec![
                text_map(vec![("md2", s("X")), ("lg", s("Y"))]),
                map(vec![("text", s("Plain"))]),
                nested,
            ],
            SourceInfo::for_test(),
        );
        let warnings = responsive_text_warnings(&items);
        assert_eq!(warnings.len(), 3, "{warnings:?}");
        assert!(matches!(
            &warnings[0],
            ResponsiveTextWarning::UnknownBreakpoint { key, .. } if key == "md2"
        ));
        assert!(matches!(
            &warnings[1],
            ResponsiveTextWarning::UnknownBreakpoint { key, .. } if key == "wide"
        ));
        assert!(matches!(
            &warnings[2],
            ResponsiveTextWarning::NoBreakpoints { .. }
        ));
    }
}
