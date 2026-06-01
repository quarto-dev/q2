/*
 * page_nav.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! The [`PageNavigation`] type — the prev/next strip rendered at the
//! bottom of a website page.
//!
//! Built by `PageNavGenerateTransform` from the already-resolved
//! sidebar for the current page (depth-first flatten + dedupe-by-href +
//! separator-as-boundary; see
//! `claude-notes/plans/2026-04-24-websites-phase-4.md` §Decision 4 for
//! the algorithm). Rendered to HTML by
//! `PageNavRenderTransform`. Hrefs remain in source-path space until
//! the Render step rewrites them to format-specific output hrefs.
//!
//! The shape is two `Option<NavigationItem>` slots — reusing the
//! existing nav item lets the Phase 3 href-rewrite helper drop in
//! without a wrapper. Item fields like `icon`, `menu`, `active`, and
//! `target` are not meaningful for page-nav but roundtrip harmlessly.

use quarto_pandoc_types::ConfigMapEntry;
use quarto_pandoc_types::config_value::ConfigValue;
use quarto_source_map::SourceInfo;

use crate::item::NavigationItem;

/// The prev/next strip for a single page.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PageNavigation {
    pub prev: Option<NavigationItem>,
    pub next: Option<NavigationItem>,
}

impl PageNavigation {
    pub fn is_empty(&self) -> bool {
        self.prev.is_none() && self.next.is_none()
    }

    /// Parse from a `ConfigValue` map. Missing `prev` / `next` keys
    /// (or unparseable items) yield `None` on that side.
    pub fn from_config_value(cv: &ConfigValue) -> Self {
        let prev = cv.get("prev").and_then(NavigationItem::from_config_value);
        let next = cv.get("next").and_then(NavigationItem::from_config_value);
        PageNavigation { prev, next }
    }

    /// Serialize to a `ConfigValue` map. Empty sides are omitted from
    /// the emitted map (omit-default convention; matches
    /// `NavigationItem::to_config_value`).
    pub fn to_config_value(&self) -> ConfigValue {
        let info = SourceInfo::default();
        let mut entries: Vec<ConfigMapEntry> = Vec::new();
        if let Some(ref prev) = self.prev {
            entries.push(ConfigMapEntry {
                key: "prev".to_string(),
                key_source: info.clone(),
                value: prev.to_config_value(),
            });
        }
        if let Some(ref next) = self.next {
            entries.push(ConfigMapEntry {
                key: "next".to_string(),
                key_source: info.clone(),
                value: next.to_config_value(),
            });
        }
        ConfigValue::new_map(entries, info)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(x: &str) -> ConfigValue {
        ConfigValue::new_string(x, SourceInfo::for_test())
    }

    /// Test 1 — default is empty.
    #[test]
    fn page_navigation_default_is_empty() {
        let pn = PageNavigation::default();
        assert!(pn.prev.is_none());
        assert!(pn.next.is_none());
        assert!(pn.is_empty());
    }

    /// Test 2 — populate with items bearing href + text; roundtrip
    /// preserves both sides.
    #[test]
    fn page_navigation_roundtrip_preserves_prev_next() {
        let original = PageNavigation {
            prev: Some(NavigationItem {
                href: Some("a.qmd".to_string()),
                text: Some(s("A")),
                ..NavigationItem::default()
            }),
            next: Some(NavigationItem {
                href: Some("c.qmd".to_string()),
                text: Some(s("C")),
                ..NavigationItem::default()
            }),
        };
        let cv = original.to_config_value();
        let reparsed = PageNavigation::from_config_value(&cv);
        assert_eq!(
            reparsed.prev.as_ref().and_then(|i| i.href.clone()),
            Some("a.qmd".to_string())
        );
        assert_eq!(
            reparsed
                .prev
                .as_ref()
                .and_then(|i| i.text.as_ref().and_then(|t| t.as_plain_text())),
            Some("A".to_string())
        );
        assert_eq!(
            reparsed.next.as_ref().and_then(|i| i.href.clone()),
            Some("c.qmd".to_string())
        );
        assert_eq!(
            reparsed
                .next
                .as_ref()
                .and_then(|i| i.text.as_ref().and_then(|t| t.as_plain_text())),
            Some("C".to_string())
        );
    }

    /// Test 3 — only `next` set: emitted map has no `prev` key, and
    /// roundtrip yields `prev: None`.
    #[test]
    fn page_navigation_roundtrip_empty_side_omits_key() {
        let original = PageNavigation {
            prev: None,
            next: Some(NavigationItem {
                href: Some("c.qmd".to_string()),
                ..NavigationItem::default()
            }),
        };
        let cv = original.to_config_value();
        assert!(
            cv.get("prev").is_none(),
            "prev: None should be omitted from the emitted map; got {:?}",
            cv
        );
        let reparsed = PageNavigation::from_config_value(&cv);
        assert!(reparsed.prev.is_none());
        assert!(reparsed.next.is_some());
    }
}
