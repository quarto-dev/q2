/*
 * website_canonical_url.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * AST transform: emit `<link rel="canonical">` for website pages.
 */

//! Populate the document's `canonical-url` metadata key from
//! `website.site-url + output_href` so the full HTML template emits
//! a `<link rel="canonical">` element.
//!
//! The full template (`crates/quarto-core/src/template.rs:146-148`)
//! already has a `$canonical-url$` slot that is rendered when the
//! key is present. This transform fills it for website projects with
//! a configured `site-url`.
//!
//! See `claude-notes/plans/2026-04-27-websites-phase-7.md` Decision 6.

use std::path::Path;

use quarto_pandoc_types::ConfigMapEntry;
use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::config_value::ConfigValueKind;
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::project::website_config::website_site_url;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};
use crate::transforms::navigation_active::page_relative_source;

/// AST transform: set `canonical-url` from
/// `website.site-url + output_href`.
pub struct WebsiteCanonicalUrlTransform;

impl WebsiteCanonicalUrlTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WebsiteCanonicalUrlTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for WebsiteCanonicalUrlTransform {
    fn name(&self) -> &str {
        "website-canonical-url"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Normalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        let output_href = ctx.project_index.as_deref().and_then(|index| {
            let page_source = page_relative_source(ctx);
            index
                .lookup_by_source(Path::new(&page_source))
                .map(|p| p.output_href.clone())
        });
        apply_canonical_url(&mut ast.meta, output_href.as_deref());
        Ok(())
    }
}

/// Pure-metadata helper that powers the transform.
///
/// - **No `website.site-url`** → no-op.
/// - **No `output_href` available** → no-op (single-doc renders or
///   when the current page is missing from the index — defensive).
/// - **Both present** → set `canonical-url = compose_canonical(...)`.
fn apply_canonical_url(meta: &mut ConfigValue, output_href: Option<&str>) {
    let Some(site_url) = website_site_url(meta) else {
        return;
    };
    let Some(href) = output_href else {
        return;
    };
    let canonical = compose_canonical(&site_url, href);
    set_canonical_url(meta, canonical);
}

/// Compose the canonical URL from a site-url and a project-relative
/// output href.
///
/// Strips a trailing `/` from `site_url` (Decision 7) and a leading
/// `/` from `output_href` (defensive — the profile contract is
/// no-leading-slash, but normalize anyway) before joining with a
/// single `/`.
fn compose_canonical(site_url: &str, output_href: &str) -> String {
    let base = site_url.trim_end_matches('/');
    let tail = output_href.trim_start_matches('/');
    format!("{base}/{tail}")
}

/// Set or replace the `canonical-url` entry on a metadata map.
fn set_canonical_url(meta: &mut ConfigValue, canonical: String) {
    let source_info = meta.source_info.clone();
    let ConfigValueKind::Map(entries) = &mut meta.value else {
        return;
    };
    let new_value = ConfigValue::new_string(canonical, source_info.clone());
    if let Some(entry) = entries.iter_mut().find(|e| e.key == "canonical-url") {
        entry.value = new_value;
    } else {
        entries.push(ConfigMapEntry {
            key: "canonical-url".to_string(),
            key_source: source_info,
            value: new_value,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_source_map::SourceInfo;

    fn map_with(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
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

    fn s(value: &str) -> ConfigValue {
        ConfigValue::new_string(value.to_string(), SourceInfo::for_test())
    }

    /// Plan test 19: no `website.site-url` → `canonical-url`
    /// unchanged.
    #[test]
    fn canonical_url_no_op_without_site_url() {
        let mut meta = map_with(vec![("title", s("Doc"))]);
        apply_canonical_url(&mut meta, Some("doc.html"));
        assert!(meta.get("canonical-url").is_none());
    }

    /// Plan test 20: site-url + output_href compose to a full URL.
    #[test]
    fn canonical_url_composes_site_url_and_output_href() {
        let mut meta = map_with(vec![(
            "website",
            map_with(vec![("site-url", s("https://example.com/"))]),
        )]);
        apply_canonical_url(&mut meta, Some("docs/api.html"));
        assert_eq!(
            meta.get("canonical-url").and_then(|v| v.as_str()),
            Some("https://example.com/docs/api.html")
        );
    }

    /// Plan test 21: site-url without trailing slash still
    /// composes correctly (verbatim helper).
    #[test]
    fn canonical_url_handles_trailing_slash_on_site_url() {
        assert_eq!(
            compose_canonical("https://example.com", "docs/api.html"),
            "https://example.com/docs/api.html"
        );
    }

    /// Plan test 22: with no `output_href` (single-doc render or
    /// missing-from-index page), the transform leaves the metadata
    /// alone — even when `site-url` is set.
    #[test]
    fn canonical_url_no_op_without_output_href() {
        let mut meta = map_with(vec![(
            "website",
            map_with(vec![("site-url", s("https://example.com/"))]),
        )]);
        apply_canonical_url(&mut meta, None);
        assert!(meta.get("canonical-url").is_none());
    }

    /// Defensive: leading-slash in output_href is normalized away.
    #[test]
    fn canonical_url_strips_leading_slash_in_output_href() {
        assert_eq!(
            compose_canonical("https://example.com", "/docs/api.html"),
            "https://example.com/docs/api.html"
        );
    }

    /// Sub-path site URLs (e.g. `https://example.com/site`) work.
    #[test]
    fn canonical_url_handles_sub_path_site_url() {
        assert_eq!(
            compose_canonical("https://example.com/site/", "docs/api.html"),
            "https://example.com/site/docs/api.html"
        );
    }

    /// Set or replace mutates metadata correctly.
    #[test]
    fn set_canonical_url_inserts_when_absent() {
        let mut meta = ConfigValue::new_map(Vec::new(), SourceInfo::for_test());
        set_canonical_url(&mut meta, "https://example.com/x.html".to_string());
        assert_eq!(
            meta.get("canonical-url").and_then(|v| v.as_str()),
            Some("https://example.com/x.html")
        );
    }

    /// Set or replace overwrites an existing entry rather than
    /// stacking duplicates.
    #[test]
    fn set_canonical_url_replaces_existing_entry() {
        let mut meta = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "canonical-url".to_string(),
                key_source: SourceInfo::for_test(),
                value: ConfigValue::new_string("old", SourceInfo::for_test()),
            }],
            SourceInfo::for_test(),
        );
        set_canonical_url(&mut meta, "new".to_string());
        if let ConfigValueKind::Map(entries) = &meta.value {
            let count = entries.iter().filter(|e| e.key == "canonical-url").count();
            assert_eq!(count, 1);
            assert_eq!(
                entries
                    .iter()
                    .find(|e| e.key == "canonical-url")
                    .and_then(|e| e.value.as_str()),
                Some("new")
            );
        } else {
            panic!("meta is not a map");
        }
    }

    /// Defensive: a non-map metadata is not mutated.
    #[test]
    fn set_canonical_url_no_op_on_non_map_meta() {
        let mut meta = ConfigValue::null(SourceInfo::for_test());
        set_canonical_url(&mut meta, "https://example.com/x.html".to_string());
        // Just shouldn't panic; the value remains null.
        assert!(matches!(
            meta.value,
            ConfigValueKind::Scalar {
                yaml: yaml_rust2::Yaml::Null,
                ..
            }
        ));
    }

    // The `lookup_by_source` / `project_index` branch of the
    // transform is exercised end-to-end by the integration test
    // `pipeline_canonical_url_per_page` in
    // `crates/quarto-core/tests/website_post_render.rs`.
}
