/*
 * website_favicon.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * AST transform: append a <link rel="icon"> to header-includes when
 * `website.favicon` is set.
 */

//! Emit a `<link rel="icon">` tag into every page's `<head>` for
//! website projects with a configured favicon.
//!
//! This transform reads `website.favicon` from the merged metadata,
//! resolves a page-relative href via the per-page
//! [`ResourceResolverContext`], and appends a `<link>` element to
//! the document's `header-includes` list. The full HTML template
//! (`crates/quarto-core/src/template.rs`) loops over
//! `header-includes` inside the `<head>`, so the link reaches the
//! output without further wiring.
//!
//! See `claude-notes/plans/2026-04-27-websites-phase-7.md` Decision 5.
//!
//! [`ResourceResolverContext`]: crate::resource_resolver::ResourceResolverContext

use quarto_pandoc_types::ConfigMapEntry;
use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::config_value::ConfigValueKind;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceInfo;

use crate::Result;
use crate::project::website_config::{normalize_favicon_path, website_favicon};
use crate::render::RenderContext;
use crate::resource_resolver::ResourceResolverContext;
use crate::transform::AstTransform;

/// AST transform: append a `<link rel="icon">` to `header-includes`
/// for website projects with a configured favicon.
pub struct WebsiteFaviconTransform;

impl WebsiteFaviconTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WebsiteFaviconTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for WebsiteFaviconTransform {
    fn name(&self) -> &str {
        "website-favicon"
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        apply_favicon(&mut ast.meta, ctx.resource_resolver.as_ref());
        Ok(())
    }
}

/// Append a `<link rel="icon">` tag for `website.favicon` to the
/// `header-includes` list. No-op if the key is absent or `meta` is
/// not a map.
fn apply_favicon(meta: &mut ConfigValue, resolver: Option<&ResourceResolverContext>) {
    let Some(raw_favicon) = website_favicon(meta) else {
        return;
    };
    let normalized = normalize_favicon_path(&raw_favicon);
    if normalized.is_empty() {
        return;
    }

    // Compute the page-relative href via the resolver (Phase 5).
    // No-resolver fallback: the path verbatim.
    let href = match resolver {
        Some(r) => r.page_url_for(&normalized),
        None => normalized.clone(),
    };

    let link = build_favicon_link(&href, &normalized);

    let source_info = meta.source_info.clone();
    let ConfigValueKind::Map(entries) = &mut meta.value else {
        return;
    };
    append_to_header_includes(entries, link, source_info);
}

/// Build a `<link rel="icon" ...>` element for the given href, with
/// a `type` attribute when the file extension maps to a known MIME
/// type.
fn build_favicon_link(href: &str, normalized_path: &str) -> String {
    let escaped_href = escape_html_attr(href);
    match favicon_mime_type(normalized_path) {
        Some(mime) => format!(
            r#"<link rel="icon" href="{}" type="{}">"#,
            escaped_href, mime
        ),
        None => format!(r#"<link rel="icon" href="{}">"#, escaped_href),
    }
}

/// Map a favicon file extension to its IANA MIME type.
///
/// Returns `None` for unknown extensions; callers omit the
/// `type="..."` attribute in that case.
fn favicon_mime_type(path: &str) -> Option<&'static str> {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".ico") {
        Some("image/x-icon")
    } else if lower.ends_with(".png") {
        Some("image/png")
    } else if lower.ends_with(".svg") {
        Some("image/svg+xml")
    } else if lower.ends_with(".gif") {
        Some("image/gif")
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        Some("image/jpeg")
    } else {
        None
    }
}

/// Minimal HTML attribute escaper. The favicon href and computed
/// MIME type are author-controlled, but we still escape `&`, `<`,
/// `>`, and `"` to avoid producing malformed HTML when the user
/// writes `website.favicon: "a&b.ico"`.
fn escape_html_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Append a `<link>` (or any HTML string) to the `header-includes`
/// metadata list.
///
/// `header-includes` may be absent, a single string, an array of
/// strings, or a PandocInlines value. We normalize to an `Array` of
/// strings, preserving any existing entries.
fn append_to_header_includes(
    entries: &mut Vec<ConfigMapEntry>,
    link_html: String,
    source_info: SourceInfo,
) {
    let new_item = ConfigValue::new_string(link_html, source_info.clone());

    if let Some(entry) = entries.iter_mut().find(|e| e.key == "header-includes") {
        match &mut entry.value.value {
            ConfigValueKind::Array(items) => {
                items.push(new_item);
            }
            _ => {
                // Promote scalar / inlines / etc. to an array of two
                // items: the existing value followed by the new link.
                let existing = std::mem::replace(
                    &mut entry.value,
                    ConfigValue::new_array(vec![], source_info.clone()),
                );
                let mut items = Vec::with_capacity(2);
                items.push(existing);
                items.push(new_item);
                entry.value = ConfigValue::new_array(items, source_info);
            }
        }
        return;
    }

    entries.push(ConfigMapEntry {
        key: "header-includes".to_string(),
        key_source: source_info.clone(),
        value: ConfigValue::new_array(vec![new_item], source_info),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource_resolver::ResourceResolverContext;
    use std::path::PathBuf;

    fn s(value: &str) -> ConfigValue {
        ConfigValue::new_string(value.to_string(), SourceInfo::default())
    }

    fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        let entries = entries
            .into_iter()
            .map(|(k, v)| ConfigMapEntry {
                key: k.to_string(),
                key_source: SourceInfo::default(),
                value: v,
            })
            .collect();
        ConfigValue::new_map(entries, SourceInfo::default())
    }

    /// Read `header-includes` as a list of plain strings.
    fn header_includes_strings(meta: &ConfigValue) -> Vec<String> {
        match meta.get("header-includes").map(|v| &v.value) {
            Some(ConfigValueKind::Array(items)) => items
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect(),
            Some(ConfigValueKind::Scalar(_)) => meta
                .get("header-includes")
                .and_then(|v| v.as_str())
                .map(|s| vec![s.to_string()])
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// Site-mode resolver where `_site/index.html` is the current
    /// page. `page_url_for("favicon.ico")` returns `"favicon.ico"`.
    fn root_page_resolver() -> ResourceResolverContext {
        ResourceResolverContext::website(
            PathBuf::from("/proj/_site"),
            PathBuf::from("/proj/_site/index.html"),
            "site_libs".to_string(),
            "index".to_string(),
        )
    }

    /// Site-mode resolver where `_site/docs/api.html` is the
    /// current page. `page_url_for("favicon.ico")` returns
    /// `"../favicon.ico"`.
    fn nested_page_resolver() -> ResourceResolverContext {
        ResourceResolverContext::website(
            PathBuf::from("/proj/_site"),
            PathBuf::from("/proj/_site/docs/api.html"),
            "site_libs".to_string(),
            "api".to_string(),
        )
    }

    /// Plan test 13: no `website.favicon` → `header-includes`
    /// untouched.
    #[test]
    fn favicon_no_op_without_website_favicon() {
        let mut meta = map(vec![("title", s("Doc"))]);
        apply_favicon(&mut meta, Some(&root_page_resolver()));
        assert!(header_includes_strings(&meta).is_empty());
    }

    /// Plan test 14: from a nested page, the favicon `<link>` href
    /// should be page-relative (`"../favicon.ico"`) and carry the
    /// correct MIME type.
    #[test]
    fn favicon_appends_link_with_resolved_href() {
        let mut meta = map(vec![("website", map(vec![("favicon", s("favicon.ico"))]))]);
        apply_favicon(&mut meta, Some(&nested_page_resolver()));
        let includes = header_includes_strings(&meta);
        assert_eq!(includes.len(), 1);
        assert_eq!(
            includes[0],
            r#"<link rel="icon" href="../favicon.ico" type="image/x-icon">"#
        );
    }

    /// Plan test 15: an unknown extension yields a `<link>` without
    /// the `type` attribute.
    #[test]
    fn favicon_appends_without_type_for_unknown_extension() {
        let mut meta = map(vec![("website", map(vec![("favicon", s("favicon.foo"))]))]);
        apply_favicon(&mut meta, Some(&root_page_resolver()));
        let includes = header_includes_strings(&meta);
        assert_eq!(includes.len(), 1);
        assert_eq!(includes[0], r#"<link rel="icon" href="favicon.foo">"#);
    }

    /// Plan test 16: with no resolver, the favicon path is used
    /// verbatim. Defensive — tests / future callers that don't wire
    /// a resolver still produce a valid `<link>`.
    #[test]
    fn favicon_falls_back_to_path_verbatim_without_resolver() {
        let mut meta = map(vec![("website", map(vec![("favicon", s("favicon.ico"))]))]);
        apply_favicon(&mut meta, None);
        let includes = header_includes_strings(&meta);
        assert_eq!(includes.len(), 1);
        assert_eq!(
            includes[0],
            r#"<link rel="icon" href="favicon.ico" type="image/x-icon">"#
        );
    }

    /// Plan test 17: a sub-directory favicon path resolves via the
    /// resolver, with the right MIME type for `.svg`.
    #[test]
    fn favicon_handles_subdirectory_path() {
        let mut meta = map(vec![(
            "website",
            map(vec![("favicon", s("assets/favicon.svg"))]),
        )]);
        apply_favicon(&mut meta, Some(&nested_page_resolver()));
        let includes = header_includes_strings(&meta);
        assert_eq!(includes.len(), 1);
        assert_eq!(
            includes[0],
            r#"<link rel="icon" href="../assets/favicon.svg" type="image/svg+xml">"#
        );
    }

    /// Plan test 18: an existing `header-includes` is preserved; the
    /// new `<link>` is appended.
    #[test]
    fn favicon_appends_to_existing_header_includes() {
        let mut meta = map(vec![
            (
                "header-includes",
                ConfigValue::new_array(
                    vec![s("<meta name=\"foo\" content=\"bar\">")],
                    SourceInfo::default(),
                ),
            ),
            ("website", map(vec![("favicon", s("favicon.ico"))])),
        ]);
        apply_favicon(&mut meta, Some(&root_page_resolver()));
        let includes = header_includes_strings(&meta);
        assert_eq!(includes.len(), 2);
        assert_eq!(includes[0], r#"<meta name="foo" content="bar">"#);
        assert_eq!(
            includes[1],
            r#"<link rel="icon" href="favicon.ico" type="image/x-icon">"#
        );
    }

    /// Open-question 4 (resolved): a leading-slash favicon path is
    /// normalized to project-relative before resolving.
    #[test]
    fn favicon_strips_leading_slash_in_path() {
        let mut meta = map(vec![("website", map(vec![("favicon", s("/favicon.ico"))]))]);
        apply_favicon(&mut meta, Some(&nested_page_resolver()));
        let includes = header_includes_strings(&meta);
        assert_eq!(includes.len(), 1);
        // Without normalization, page_url_for would have received
        // a leading-slash path and returned an unrooted absolute
        // result. With normalization it produces page-relative.
        assert_eq!(
            includes[0],
            r#"<link rel="icon" href="../favicon.ico" type="image/x-icon">"#
        );
    }

    /// HTML-escape: a path with `&` doesn't produce broken HTML.
    #[test]
    fn favicon_escapes_ampersand_in_path() {
        let mut meta = map(vec![("website", map(vec![("favicon", s("a&b.ico"))]))]);
        apply_favicon(&mut meta, Some(&root_page_resolver()));
        let includes = header_includes_strings(&meta);
        assert_eq!(includes.len(), 1);
        assert!(
            includes[0].contains("href=\"a&amp;b.ico\""),
            "unexpected: {}",
            includes[0]
        );
    }

    /// MIME type detection covers all five known extensions.
    #[test]
    fn favicon_mime_type_table() {
        assert_eq!(favicon_mime_type("favicon.ico"), Some("image/x-icon"));
        assert_eq!(favicon_mime_type("favicon.png"), Some("image/png"));
        assert_eq!(favicon_mime_type("favicon.svg"), Some("image/svg+xml"));
        assert_eq!(favicon_mime_type("favicon.gif"), Some("image/gif"));
        assert_eq!(favicon_mime_type("favicon.jpg"), Some("image/jpeg"));
        assert_eq!(favicon_mime_type("favicon.jpeg"), Some("image/jpeg"));
        assert_eq!(favicon_mime_type("FAVICON.ICO"), Some("image/x-icon"));
        assert_eq!(favicon_mime_type("favicon.foo"), None);
        assert_eq!(favicon_mime_type("favicon"), None);
    }
}
