//! Resolution helpers for website-scoped configuration keys.
//!
//! Quarto allows certain configuration keys (e.g. `navbar`, `page-footer`,
//! `sidebar`) to appear at two locations in the merged metadata:
//!
//! - **Top level**: `<key>: ...` — the "feature-scoped" form. Document
//!   frontmatter contributions land here naturally, and single-doc (no
//!   `website:` block) renders can configure these features without
//!   namespacing under `website:`.
//! - **Nested**: `website.<key>: ...` — the Quarto 1 compatible form.
//!
//! [`resolve_website_value`] returns a merged view that accepts either
//! location, with the top-level form winning on conflicts. This matches
//! the precedence baked into `resolve_website_bool` over in
//! `quarto-core` for boolean feature flags: document frontmatter
//! contributions land at the top level and naturally override project
//! chrome supplied via `website:`. Either layer can use `!prefer` to
//! take full precedence for its subtree.
//!
//! See `claude-notes/plans/2025-12-07-config-merging-design.md` for the
//! underlying tag-based merge semantics.

use crate::merged::MergedConfig;
use crate::types::ConfigValue;

/// Resolve a website-style configuration value that may live at either
/// `meta.<key>` (top-level) or `meta.website.<key>` (nested).
///
/// If both exist, the result is the materialization of a two-layer
/// merge with the nested form as the *outer* (lower-priority) layer
/// and the top-level form as the *inner* (higher-priority) layer. By
/// default, maps merge field-wise (top-level wins on overlapping
/// keys); arrays concatenate. Either layer can use `!prefer` to take
/// full precedence for its subtree.
///
/// Returns `None` if neither location is present. Returns
/// `Some(materialized)` otherwise — callers are responsible for any
/// `as_bool() == Some(false)` "affirmative disable" handling.
pub fn resolve_website_value(meta: &ConfigValue, key: &str) -> Option<ConfigValue> {
    let top = meta.get(key);
    let nested = meta.get_path(&["website", key]);
    match (top, nested) {
        (None, None) => None,
        (None, Some(w)) => Some(w.clone()),
        (Some(t), None) => Some(t.clone()),
        (Some(t), Some(w)) => MergedConfig::new(vec![w, t]).materialize().ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ConfigMapEntry, ConfigValueKind, MergeOp};
    use quarto_source_map::SourceInfo;
    use yaml_rust2::Yaml;

    fn s(x: &str) -> ConfigValue {
        ConfigValue::new_string(x, SourceInfo::default())
    }

    fn b(x: bool) -> ConfigValue {
        ConfigValue::new_bool(x, SourceInfo::default())
    }

    fn arr(items: Vec<ConfigValue>) -> ConfigValue {
        ConfigValue::new_array(items, SourceInfo::default())
    }

    fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        let map_entries: Vec<ConfigMapEntry> = entries
            .into_iter()
            .map(|(k, v)| ConfigMapEntry {
                key: k.to_string(),
                key_source: SourceInfo::default(),
                value: v,
            })
            .collect();
        ConfigValue::new_map(map_entries, SourceInfo::default())
    }

    #[test]
    fn returns_none_when_neither_present() {
        let meta = map(vec![]);
        assert!(resolve_website_value(&meta, "navbar").is_none());
    }

    #[test]
    fn returns_top_level_when_only_top_level_present() {
        let navbar = map(vec![("logo", s("a.png"))]);
        let meta = map(vec![("navbar", navbar)]);
        let resolved = resolve_website_value(&meta, "navbar").unwrap();
        assert_eq!(
            resolved
                .get("logo")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("a.png")
        );
    }

    #[test]
    fn returns_nested_when_only_nested_present() {
        let navbar = map(vec![("logo", s("b.png"))]);
        let meta = map(vec![("website", map(vec![("navbar", navbar)]))]);
        let resolved = resolve_website_value(&meta, "navbar").unwrap();
        assert_eq!(
            resolved
                .get("logo")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("b.png")
        );
    }

    #[test]
    fn merges_disjoint_fields_when_both_present() {
        // website.navbar = { logo: nested.png }
        // navbar         = { background: primary }
        // expect both fields after merge
        let meta = map(vec![
            (
                "website",
                map(vec![("navbar", map(vec![("logo", s("nested.png"))]))]),
            ),
            ("navbar", map(vec![("background", s("primary"))])),
        ]);
        let resolved = resolve_website_value(&meta, "navbar").unwrap();
        assert_eq!(
            resolved
                .get("logo")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("nested.png"),
            "nested logo should be preserved"
        );
        assert_eq!(
            resolved
                .get("background")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("primary"),
            "top-level background should be preserved"
        );
    }

    #[test]
    fn top_level_wins_on_overlapping_field() {
        // website.navbar = { logo: nested.png }
        // navbar         = { logo: top.png }
        // top-level wins
        let meta = map(vec![
            (
                "website",
                map(vec![("navbar", map(vec![("logo", s("nested.png"))]))]),
            ),
            ("navbar", map(vec![("logo", s("top.png"))])),
        ]);
        let resolved = resolve_website_value(&meta, "navbar").unwrap();
        assert_eq!(
            resolved
                .get("logo")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("top.png"),
            "top-level logo must override nested"
        );
    }

    #[test]
    fn arrays_concatenate_with_nested_first() {
        // website.navbar.left = [a, b]
        // navbar.left         = [c, d]
        // expect [a, b, c, d] (nested = lower priority, comes first)
        let meta = map(vec![
            (
                "website",
                map(vec![(
                    "navbar",
                    map(vec![("left", arr(vec![s("a.qmd"), s("b.qmd")]))]),
                )]),
            ),
            (
                "navbar",
                map(vec![("left", arr(vec![s("c.qmd"), s("d.qmd")]))]),
            ),
        ]);
        let resolved = resolve_website_value(&meta, "navbar").unwrap();
        let left = resolved.get("left").and_then(|v| v.as_array()).unwrap();
        let hrefs: Vec<String> = left.iter().filter_map(|cv| cv.as_plain_text()).collect();
        assert_eq!(hrefs, vec!["a.qmd", "b.qmd", "c.qmd", "d.qmd"]);
    }

    #[test]
    fn top_level_false_passes_through() {
        // Affirmative disable at top level still wins.
        let meta = map(vec![
            (
                "website",
                map(vec![("navbar", map(vec![("logo", s("nested.png"))]))]),
            ),
            ("navbar", b(false)),
        ]);
        let resolved = resolve_website_value(&meta, "navbar").unwrap();
        assert_eq!(
            resolved.as_bool(),
            Some(false),
            "top-level navbar: false must win, disabling the navbar"
        );
    }

    #[test]
    fn nested_false_passes_through_when_no_top_level() {
        let meta = map(vec![("website", map(vec![("navbar", b(false))]))]);
        let resolved = resolve_website_value(&meta, "navbar").unwrap();
        assert_eq!(resolved.as_bool(), Some(false));
    }

    #[test]
    fn prefer_on_top_level_resets_nested() {
        // website.navbar = { logo: nested.png, background: primary }
        // navbar = !prefer { logo: top.png }
        // expect { logo: top.png } only — background gone
        let mut top = map(vec![("logo", s("top.png"))]);
        top.merge_op = MergeOp::Prefer;
        let meta = map(vec![
            (
                "website",
                map(vec![(
                    "navbar",
                    map(vec![
                        ("logo", s("nested.png")),
                        ("background", s("primary")),
                    ]),
                )]),
            ),
            ("navbar", top),
        ]);
        let resolved = resolve_website_value(&meta, "navbar").unwrap();
        assert_eq!(
            resolved
                .get("logo")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("top.png")
        );
        assert!(
            resolved.get("background").is_none(),
            "!prefer on top-level should drop nested fields"
        );
    }

    #[test]
    fn nested_only_value_clones_cleanly() {
        // Make sure the (None, Some) branch returns a usable independent value.
        let navbar = map(vec![
            ("logo", s("only.png")),
            (
                "left",
                arr(vec![map(vec![
                    ("text", s("Home")),
                    ("href", s("index.qmd")),
                ])]),
            ),
        ]);
        let meta = map(vec![("website", map(vec![("navbar", navbar)]))]);
        let resolved = resolve_website_value(&meta, "navbar").unwrap();
        assert_eq!(
            resolved
                .get("logo")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("only.png")
        );
        let left = resolved.get("left").and_then(|v| v.as_array()).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(
            left[0]
                .get("href")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("index.qmd")
        );
    }

    #[test]
    fn scalar_at_meta_key_not_wrapped_in_map_still_returned() {
        // Smoke test: a scalar top-level value (e.g. `navbar: false` alone)
        // still resolves through the helper.
        let meta = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "navbar".to_string(),
                key_source: SourceInfo::default(),
                value: ConfigValue::new_scalar(Yaml::Boolean(false), SourceInfo::default()),
            }],
            SourceInfo::default(),
        );
        let resolved = resolve_website_value(&meta, "navbar").unwrap();
        assert_eq!(resolved.as_bool(), Some(false));
        // Sanity: ConfigValueKind preserved as scalar through (Some, None) branch.
        assert!(matches!(resolved.value, ConfigValueKind::Scalar(_)));
    }
}
