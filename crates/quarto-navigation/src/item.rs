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
use quarto_pandoc_types::config_value::ConfigValue;
use quarto_source_map::SourceInfo;

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
    /// root). `SourceInfo::default()` for programmatically-constructed
    /// items (tests, in-memory builders) — the path resolution helper
    /// short-circuits on that case and treats the href as already
    /// project-root-relative.
    pub href_source: SourceInfo,

    /// Display text. Preserved as `ConfigValue` so markdown inlines from
    /// document metadata survive without being flattened.
    pub text: Option<ConfigValue>,

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

    /// Whether this item represents the currently-rendered page.
    /// Set by the `mark_active` pass in Generate transforms; read by
    /// Render transforms when emitting the `active` class on the anchor.
    /// Does not roundtrip through YAML input (users don't author this),
    /// but does roundtrip through the generate→render handoff via
    /// `to_config_value` / `from_config_value`.
    pub active: bool,
}

impl NavigationItem {
    /// Parse a single item from a `ConfigValue` in one of the three accepted
    /// shapes. Returns `None` if the shape is unrecognisable (e.g. a number).
    pub fn from_config_value(cv: &ConfigValue) -> Option<Self> {
        // Bare path form: `- about.qmd`. The source_info travels with
        // the bare ConfigValue itself (no wrapping map node).
        if let Some(s) = cv.as_plain_text() {
            // Only treat as a path if it isn't a map or array. `as_plain_text`
            // already narrows to scalar-ish shapes, so this is safe.
            return Some(NavigationItem {
                href: Some(s),
                href_source: cv.source_info.clone(),
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
            .map(|(s, info)| (Some(s), info))
            .unwrap_or_else(|| (None, SourceInfo::default()));

        let text = cv.get("text").cloned();
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
            icon,
            aria_label,
            rel,
            target,
            menu,
            active,
        })
    }

    /// Serialise back to a `ConfigValue` map suitable for storing at
    /// `navigation.navbar` / `navigation.footer`. Empty fields are omitted.
    ///
    /// `href`'s `SourceInfo` is round-tripped from `self.href_source` so
    /// the Generate → Render handoff preserves the original YAML
    /// location. Other fields use `SourceInfo::default()` — they are
    /// either programmatic (`active`) or unused for diagnostic
    /// location today.
    pub fn to_config_value(&self) -> ConfigValue {
        let source_info = SourceInfo::default();
        let mut entries: Vec<ConfigMapEntry> = Vec::new();

        if let Some(ref href) = self.href {
            entries.push(ConfigMapEntry {
                key: "href".to_string(),
                key_source: source_info.clone(),
                value: ConfigValue::new_string(href, self.href_source.clone()),
            });
        }
        if let Some(ref text) = self.text {
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
}
