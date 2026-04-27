/*
 * footer.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Page-footer data model and YAML resolution.
//!
//! [`resolve_page_footer`] accepts the merged `ast.meta` and returns a
//! [`PageFooter`] populated with defaults, or `None` when the footer is
//! absent (no `page-footer` key) or disabled (`page-footer: false`).
//!
//! A footer region is either free-form markdown text (stored as
//! [`ConfigValue`] to preserve `PandocInlines` from document context) or a
//! list of navigation items. Mixing the two within a single region is not
//! supported; a string value wins if both shapes are present at parse time.

use quarto_pandoc_types::ConfigMapEntry;
use quarto_pandoc_types::config_value::ConfigValue;
use quarto_source_map::SourceInfo;

use crate::item::NavigationItem;

/// Content of a single footer region (`left`, `center`, or `right`).
#[derive(Debug, Clone, PartialEq, Default)]
pub enum FooterRegion {
    /// No content in this region.
    #[default]
    Empty,
    /// Free-form text. Preserved as `ConfigValue` so markdown inlines from
    /// document metadata survive without being flattened.
    Text(ConfigValue),
    /// Navigation items (icons, links, dropdowns).
    Items(Vec<NavigationItem>),
}

impl FooterRegion {
    /// Parse a region from its `ConfigValue`. A missing key yields
    /// `FooterRegion::Empty`; a string yields `Text`; an array yields
    /// `Items`; any other shape yields `Empty`.
    pub fn from_config_value(cv: Option<&ConfigValue>) -> Self {
        let Some(cv) = cv else {
            return FooterRegion::Empty;
        };

        // Arrays are items.
        if let Some(arr) = cv.as_array() {
            let items: Vec<NavigationItem> = arr
                .iter()
                .filter_map(NavigationItem::from_config_value)
                .collect();
            if items.is_empty() {
                return FooterRegion::Empty;
            }
            return FooterRegion::Items(items);
        }

        // A plain-textable scalar (including PandocInlines) is text.
        if cv.as_plain_text().is_some() {
            return FooterRegion::Text(cv.clone());
        }

        FooterRegion::Empty
    }

    /// Serialise back to a `ConfigValue`. `Empty` returns `None` so callers
    /// can omit the key entirely.
    pub fn to_config_value(&self) -> Option<ConfigValue> {
        match self {
            FooterRegion::Empty => None,
            FooterRegion::Text(cv) => Some(cv.clone()),
            FooterRegion::Items(items) => {
                let values: Vec<ConfigValue> =
                    items.iter().map(NavigationItem::to_config_value).collect();
                Some(ConfigValue::new_array(values, SourceInfo::default()))
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        matches!(self, FooterRegion::Empty)
    }
}

/// Footer-border treatment.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum FooterBorder {
    /// Use the theme default (top border in Quarto 1's case).
    #[default]
    Default,
    Enabled,
    Disabled,
    /// Specific color (named or hex).
    Color(String),
}

impl FooterBorder {
    pub fn from_config_value(cv: &ConfigValue) -> Self {
        if let Some(b) = cv.as_bool() {
            return if b {
                FooterBorder::Enabled
            } else {
                FooterBorder::Disabled
            };
        }
        if let Some(s) = cv.as_plain_text() {
            return FooterBorder::Color(s);
        }
        FooterBorder::Default
    }

    pub fn to_config_value(&self) -> Option<ConfigValue> {
        let info = SourceInfo::default();
        match self {
            FooterBorder::Default => None,
            FooterBorder::Enabled => Some(ConfigValue::new_bool(true, info)),
            FooterBorder::Disabled => Some(ConfigValue::new_bool(false, info)),
            FooterBorder::Color(s) => Some(ConfigValue::new_string(s, info)),
        }
    }
}

/// Fully resolved page-footer configuration.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PageFooter {
    pub left: FooterRegion,
    pub center: FooterRegion,
    pub right: FooterRegion,
    pub border: FooterBorder,
    pub background: Option<String>,
    pub foreground: Option<String>,
}

impl PageFooter {
    /// Parse a page footer from its YAML object form. The boolean form is
    /// stripped in [`resolve_page_footer`] before this is called.
    pub fn from_config_value(cv: &ConfigValue) -> Self {
        // String shortcut: whole value is centered text.
        if cv.as_array().is_none()
            && cv.get("left").is_none()
            && cv.get("center").is_none()
            && cv.get("right").is_none()
            && cv.get("border").is_none()
            && cv.get("background").is_none()
            && cv.get("foreground").is_none()
            && cv.as_plain_text().is_some()
        {
            return PageFooter {
                center: FooterRegion::Text(cv.clone()),
                ..PageFooter::default()
            };
        }

        let mut footer = PageFooter::default();
        footer.left = FooterRegion::from_config_value(cv.get("left"));
        footer.center = FooterRegion::from_config_value(cv.get("center"));
        footer.right = FooterRegion::from_config_value(cv.get("right"));
        if let Some(border_cv) = cv.get("border") {
            footer.border = FooterBorder::from_config_value(border_cv);
        }
        footer.background = cv.get("background").and_then(|v| v.as_plain_text());
        footer.foreground = cv.get("foreground").and_then(|v| v.as_plain_text());
        footer
    }

    /// Serialise back to a map suitable for storage at `navigation.footer`.
    pub fn to_config_value(&self) -> ConfigValue {
        let info = SourceInfo::default();
        let mut entries: Vec<ConfigMapEntry> = Vec::new();

        if let Some(v) = self.left.to_config_value() {
            entries.push(ConfigMapEntry {
                key: "left".to_string(),
                key_source: info.clone(),
                value: v,
            });
        }
        if let Some(v) = self.center.to_config_value() {
            entries.push(ConfigMapEntry {
                key: "center".to_string(),
                key_source: info.clone(),
                value: v,
            });
        }
        if let Some(v) = self.right.to_config_value() {
            entries.push(ConfigMapEntry {
                key: "right".to_string(),
                key_source: info.clone(),
                value: v,
            });
        }
        if let Some(v) = self.border.to_config_value() {
            entries.push(ConfigMapEntry {
                key: "border".to_string(),
                key_source: info.clone(),
                value: v,
            });
        }
        if let Some(ref s) = self.background {
            entries.push(ConfigMapEntry {
                key: "background".to_string(),
                key_source: info.clone(),
                value: ConfigValue::new_string(s, info.clone()),
            });
        }
        if let Some(ref s) = self.foreground {
            entries.push(ConfigMapEntry {
                key: "foreground".to_string(),
                key_source: info.clone(),
                value: ConfigValue::new_string(s, info.clone()),
            });
        }

        ConfigValue::new_map(entries, info)
    }
}

/// Resolve the user's `page-footer:` input from `ast.meta`.
pub fn resolve_page_footer(meta: &ConfigValue) -> Option<PageFooter> {
    let cv = meta.get("page-footer")?;
    if cv.as_bool() == Some(false) {
        return None;
    }
    if cv.as_bool() == Some(true) {
        // `page-footer: true` alone is not meaningful (nothing to show);
        // treat as absent. Users supply content or set to `false`.
        return None;
    }
    Some(PageFooter::from_config_value(cv))
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
    fn resolve_absent() {
        let meta = map(vec![]);
        assert!(resolve_page_footer(&meta).is_none());
    }

    #[test]
    fn resolve_false_disables() {
        let meta = map(vec![("page-footer", b(false))]);
        assert!(resolve_page_footer(&meta).is_none());
    }

    #[test]
    fn string_shorthand_is_centered_text() {
        let meta = map(vec![("page-footer", s("Copyright 2026"))]);
        let footer = resolve_page_footer(&meta).unwrap();
        assert!(matches!(footer.left, FooterRegion::Empty));
        assert!(matches!(footer.right, FooterRegion::Empty));
        match &footer.center {
            FooterRegion::Text(cv) => {
                assert_eq!(cv.as_plain_text().as_deref(), Some("Copyright 2026"));
            }
            other => panic!("expected Text center, got {:?}", other),
        }
    }

    #[test]
    fn full_object_form() {
        let footer_cv = map(vec![
            ("left", s("Copyright 2026")),
            (
                "right",
                arr(vec![map(vec![
                    ("icon", s("github")),
                    ("href", s("https://github.com/")),
                ])]),
            ),
            ("border", b(true)),
            ("background", s("light")),
        ]);
        let meta = map(vec![("page-footer", footer_cv)]);
        let footer = resolve_page_footer(&meta).unwrap();

        match &footer.left {
            FooterRegion::Text(cv) => {
                assert_eq!(cv.as_plain_text().as_deref(), Some("Copyright 2026"));
            }
            other => panic!("expected Text left, got {:?}", other),
        }
        assert!(matches!(footer.center, FooterRegion::Empty));
        match &footer.right {
            FooterRegion::Items(items) => {
                assert_eq!(items.len(), 1);
                assert_eq!(items[0].icon.as_deref(), Some("github"));
            }
            other => panic!("expected Items right, got {:?}", other),
        }
        assert_eq!(footer.border, FooterBorder::Enabled);
        assert_eq!(footer.background.as_deref(), Some("light"));
    }

    #[test]
    fn border_false_disables_border() {
        let meta = map(vec![("page-footer", map(vec![("border", b(false))]))]);
        let footer = resolve_page_footer(&meta).unwrap();
        assert_eq!(footer.border, FooterBorder::Disabled);
    }

    #[test]
    fn border_color_string() {
        let meta = map(vec![("page-footer", map(vec![("border", s("#888"))]))]);
        let footer = resolve_page_footer(&meta).unwrap();
        assert_eq!(footer.border, FooterBorder::Color("#888".to_string()));
    }

    #[test]
    fn array_region_with_nav_items() {
        let region_arr = arr(vec![
            map(vec![("text", s("Privacy")), ("href", s("/privacy"))]),
            map(vec![("text", s("Terms")), ("href", s("/terms"))]),
        ]);
        let meta = map(vec![("page-footer", map(vec![("center", region_arr)]))]);
        let footer = resolve_page_footer(&meta).unwrap();
        match &footer.center {
            FooterRegion::Items(items) => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0].href.as_deref(), Some("/privacy"));
                assert_eq!(items[1].href.as_deref(), Some("/terms"));
            }
            other => panic!("expected Items, got {:?}", other),
        }
    }

    #[test]
    fn roundtrip_object_footer() {
        let original = PageFooter {
            left: FooterRegion::Text(s("© 2026")),
            right: FooterRegion::Items(vec![NavigationItem {
                icon: Some("github".to_string()),
                href: Some("https://github.com/".to_string()),
                ..NavigationItem::default()
            }]),
            border: FooterBorder::Enabled,
            background: Some("light".to_string()),
            ..PageFooter::default()
        };
        let cv = original.to_config_value();
        let reparsed = PageFooter::from_config_value(&cv);
        assert_eq!(reparsed.border, original.border);
        assert_eq!(reparsed.background, original.background);
        match (&reparsed.left, &original.left) {
            (FooterRegion::Text(a), FooterRegion::Text(b)) => {
                assert_eq!(a.as_plain_text(), b.as_plain_text());
            }
            _ => panic!("left did not round-trip"),
        }
        match (&reparsed.right, &original.right) {
            (FooterRegion::Items(a), FooterRegion::Items(b)) => {
                assert_eq!(a.len(), b.len());
                assert_eq!(a[0].icon, b[0].icon);
            }
            _ => panic!("right did not round-trip"),
        }
    }

    #[test]
    fn roundtrip_string_shorthand_becomes_center() {
        // The string shorthand inflates to a center-only footer.
        let original = PageFooter::from_config_value(&s("Copyright 2026"));
        let cv = original.to_config_value();
        let reparsed = PageFooter::from_config_value(&cv);
        assert_eq!(reparsed, original);
    }

    #[test]
    fn roundtrip_default_footer() {
        let original = PageFooter::default();
        let cv = original.to_config_value();
        let reparsed = PageFooter::from_config_value(&cv);
        assert_eq!(reparsed, original);
    }
}
