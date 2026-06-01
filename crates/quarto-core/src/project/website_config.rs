/*
 * project/website_config.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Site-level config readers for `website.*` keys.
 */

//! Centralized readers for `website.*` keys in a merged metadata
//! [`ConfigValue`].
//!
//! Phase 7 has six call sites that read `website.title`,
//! `website.site-url`, or `website.favicon`:
//!
//! 1. `WebsiteTitlePrefixTransform` (per-page Pass-2 transform).
//! 2. `WebsiteFaviconTransform` (per-page Pass-2 transform).
//! 3. `WebsiteCanonicalUrlTransform` (per-page Pass-2 transform).
//! 4. `copy_favicon` (post-render — favicon file copy).
//! 5. `write_sitemap` (post-render — sitemap.xml emission, gates on
//!    site-url).
//! 6. `write_robots_txt` (post-render — robots.txt emission, gates on
//!    site-url).
//!
//! Centralizing the reads keeps the keys behind named functions so
//! the eventual nav-config-placement migration (`bd-n9dr`) is a
//! single-file edit, and so per-page transforms and post-render code
//! cannot drift on key names.
//!
//! All three readers accept a `&ConfigValue`. Per-page transforms
//! pass `&ast.meta` (post-`MetadataMergeStage`, contains the merged
//! project + document metadata); post-render code passes
//! `project.config.metadata.as_ref()?` (raw project YAML). Either
//! source has the same `website.<key>` shape.
//!
//! See `claude-notes/plans/2026-04-27-websites-phase-7.md` Decision 7.

use quarto_pandoc_types::ConfigValue;

/// Read `website.title` from a merged metadata value.
///
/// Returns the plain-text form (Pandoc-inline titles are flattened
/// to their text content), or `None` if the key is absent or the
/// metadata is not a map.
pub fn website_title(meta: &ConfigValue) -> Option<String> {
    meta.get_path(&["website", "title"])
        .and_then(|v| v.as_plain_text())
}

/// Read `website.site-url` from a merged metadata value.
///
/// Trailing slashes are **not** stripped — callers strip when they
/// need to compose absolute URLs (e.g. sitemap and canonical-url).
/// This avoids surprising callers that want the verbatim user value.
pub fn website_site_url(meta: &ConfigValue) -> Option<String> {
    meta.get_path(&["website", "site-url"])
        .and_then(|v| v.as_plain_text())
}

/// Read `website.favicon` from a merged metadata value.
///
/// Returns the favicon path as written by the user. **Does not
/// normalize** a leading `/` — callers should call
/// [`normalize_favicon_path`] when they need the project-relative
/// form.
pub fn website_favicon(meta: &ConfigValue) -> Option<String> {
    meta.get_path(&["website", "favicon"])
        .and_then(|v| v.as_plain_text())
}

/// Normalize a user-written favicon path to project-relative form.
///
/// Strips a leading `/` if present (Q1 takes the path verbatim into
/// `offset + "/" + favicon`; we treat a leading `/` as
/// "site-rooted" — equivalent to "project-root-relative" since the
/// site root *is* the project's output root).
///
/// Forward-slash form is preserved; this is a path *expression*,
/// not a filesystem path.
pub fn normalize_favicon_path(raw: &str) -> String {
    raw.strip_prefix('/').unwrap_or(raw).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::ConfigMapEntry;
    use quarto_pandoc_types::inline::Inline;
    use quarto_source_map::SourceInfo;

    fn s(value: &str) -> ConfigValue {
        ConfigValue::new_string(value.to_string(), SourceInfo::for_test())
    }

    fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        let entries = entries
            .into_iter()
            .map(|(k, v)| ConfigMapEntry {
                key: k.to_string(),
                key_source: SourceInfo::for_test(),
                value: v,
            })
            .collect();
        ConfigValue::new_map(entries, SourceInfo::for_test())
    }

    fn null() -> ConfigValue {
        ConfigValue::null(SourceInfo::for_test())
    }

    fn pandoc_inlines(text: &str) -> ConfigValue {
        let inlines = vec![Inline::Str(quarto_pandoc_types::inline::Str {
            text: text.to_string(),
            source_info: SourceInfo::for_test(),
        })];
        ConfigValue::new_inlines(inlines, SourceInfo::for_test())
    }

    /// Test 1 (plan §Tests / Unit tests — `website_config` helpers):
    /// `website.title` as a string scalar is returned verbatim.
    #[test]
    fn website_title_reads_string() {
        let meta = map(vec![("website", map(vec![("title", s("Site"))]))]);
        assert_eq!(website_title(&meta), Some("Site".to_string()));
    }

    /// Test 2: `website.title` as Pandoc inlines is returned as
    /// flattened plain text.
    #[test]
    fn website_title_reads_inlines_as_plain_text() {
        let meta = map(vec![(
            "website",
            map(vec![("title", pandoc_inlines("Site Title"))]),
        )]);
        assert_eq!(website_title(&meta), Some("Site Title".to_string()));
    }

    /// Test 3: missing `website.title` returns `None`.
    #[test]
    fn website_title_missing_returns_none() {
        let meta = map(vec![("title", s("Doc Title"))]);
        assert_eq!(website_title(&meta), None);
    }

    /// Test 4: `website.site-url` as a string is returned verbatim
    /// (trailing slashes preserved).
    #[test]
    fn website_site_url_reads_string() {
        let meta = map(vec![(
            "website",
            map(vec![("site-url", s("https://example.com/"))]),
        )]);
        assert_eq!(
            website_site_url(&meta),
            Some("https://example.com/".to_string())
        );
    }

    /// Test 5: `website.favicon` as a string is returned verbatim
    /// (no normalization at the helper level).
    #[test]
    fn website_favicon_reads_string() {
        let meta = map(vec![("website", map(vec![("favicon", s("favicon.ico"))]))]);
        assert_eq!(website_favicon(&meta), Some("favicon.ico".to_string()));
    }

    /// Test 6: a non-map `meta` (e.g. null, scalar) returns `None`
    /// from all three helpers without panicking.
    #[test]
    fn website_helpers_handle_non_map_meta() {
        let meta = null();
        assert_eq!(website_title(&meta), None);
        assert_eq!(website_site_url(&meta), None);
        assert_eq!(website_favicon(&meta), None);

        let scalar = s("just a string");
        assert_eq!(website_title(&scalar), None);
        assert_eq!(website_site_url(&scalar), None);
        assert_eq!(website_favicon(&scalar), None);
    }

    /// Open-question 4 (resolved): leading-slash favicon paths
    /// normalize to project-relative form.
    #[test]
    fn normalize_favicon_strips_leading_slash() {
        assert_eq!(normalize_favicon_path("/favicon.ico"), "favicon.ico");
        assert_eq!(
            normalize_favicon_path("/assets/favicon.png"),
            "assets/favicon.png"
        );
    }

    /// Normalization is a no-op when no leading slash present.
    #[test]
    fn normalize_favicon_no_op_for_relative_path() {
        assert_eq!(normalize_favicon_path("favicon.ico"), "favicon.ico");
        assert_eq!(
            normalize_favicon_path("assets/favicon.png"),
            "assets/favicon.png"
        );
    }
}
