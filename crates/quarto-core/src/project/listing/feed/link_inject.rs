/*
 * project/listing/feed/link_inject.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Pass-2 [`ListingFeedLinkTransform`] — inject
//! `<link rel="alternate" type="application/rss+xml">` into the
//! host page's head metadata for each feed-configured listing.
//!
//! Unlike the rest of the `feed/` submodule, this file is **not**
//! native-only: it does no I/O, has no native-only dependencies,
//! and runs on both `build_html_pipeline_stages_with_apply_config`
//! and `build_wasm_html_pipeline`. The hub-client preview ends up
//! with a live `<link rel="alternate">` element pointing at a feed
//! file the WASM environment never produces; clicking it 404s.
//! That's acceptable v1 behavior — keeping the rendered HTML
//! byte-for-byte identical between the two render paths is more
//! valuable than hiding the link in preview, and the listings
//! reference docs (L11) carry a callout for users who notice.
//!
//! The link's `href` is the host-relative feed filename
//! (`<stem>.xml` for single-listing hosts; `<stem>-<listing-id>.xml`
//! when the host has multiple feed-configured listings — same
//! qualifier rule the stage transform uses; see plan D7).

use async_trait::async_trait;
use quarto_pandoc_types::config_value::{ConfigValue, ConfigValueKind};
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::project::listing::ResolvedListing;
use crate::project::website_config::{website_site_url, website_title};
use crate::render::RenderContext;
use crate::transform::AstTransform;
use crate::transforms::is_feature_disabled;

/// Pass-2 transform: append a `<link rel="alternate">` to
/// `rendered.includes.header` for every feed-configured listing on
/// the host page.
///
/// Multiple feed-configured listings → multiple link tags (one per
/// listing). RSS readers handle the resulting fan-out fine.
pub struct ListingFeedLinkTransform;

impl ListingFeedLinkTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ListingFeedLinkTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl AstTransform for ListingFeedLinkTransform {
    fn name(&self) -> &str {
        "listing-feed-link"
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        if is_feature_disabled(&ast.meta, "listing") {
            return Ok(());
        }
        if ctx.resolved_listings.is_empty() {
            return Ok(());
        }
        let feed_listings: Vec<&ResolvedListing> = ctx
            .resolved_listings
            .iter()
            .filter(|r| r.listing.feed.is_some())
            .collect();
        if feed_listings.is_empty() {
            return Ok(());
        }
        // No site-url → no feeds will be written by the stage
        // transform, so we skip the link tag too. The Q-12-15
        // diagnostic is emitted from the stage transform; no need
        // to duplicate here.
        if website_site_url(&ast.meta).is_none() {
            return Ok(());
        }

        let host_output_path = ctx.output_path();
        let host_stem = host_output_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("index")
            .to_string();
        let qualify = feed_listings.len() > 1;
        let website_title_default = website_title(&ast.meta).unwrap_or_default();

        let links: Vec<String> = feed_listings
            .iter()
            .map(|r| {
                let feed = r.listing.feed.as_ref().expect("filtered by has-feed above");
                let stem = if qualify {
                    format!("{}-{}", host_stem, r.listing.id)
                } else {
                    host_stem.clone()
                };
                let title = feed
                    .title
                    .clone()
                    .unwrap_or_else(|| website_title_default.clone());
                let title_attr = escape_html_attr(&title);
                let href_attr = escape_html_attr(&format!("{}.xml", stem));
                format!(
                    r#"<link rel="alternate" type="application/rss+xml" title="{}" href="{}">"#,
                    title_attr, href_attr
                )
            })
            .collect();

        for link in links {
            append_to_rendered_header(&mut ast.meta, link);
        }
        Ok(())
    }
}

/// Append an HTML literal to the canonical
/// `rendered.includes.header` list. Mirrors
/// [`crate::transforms::website_favicon`]'s
/// `append_to_rendered_header` (currently private to that module).
/// **Hoisting candidate** — bd to file at L9 close-out: a third
/// caller (sitemap link, OG tags, etc.) probably wants the same
/// helper.
fn append_to_rendered_header(meta: &mut ConfigValue, html: String) {
    if !matches!(&meta.value, ConfigValueKind::Map(_)) {
        return;
    }
    let source_info = meta.source_info.clone();

    if !meta.contains_path(&["rendered", "includes", "header"]) {
        meta.insert_path(
            &["rendered", "includes", "header"],
            ConfigValue::new_array(vec![], source_info.clone()),
        );
    }

    if let Some(slot) = meta.get_path_mut(&["rendered", "includes", "header"])
        && let ConfigValueKind::Array(items) = &mut slot.value
    {
        items.push(ConfigValue::new_string(html, source_info));
    }
}

/// Minimal HTML attribute escaper, mirroring the
/// `escape_html_attr` in `transforms::website_favicon`. Escapes
/// `&`, `<`, `>`, and `"`. Sufficient for double-quoted attribute
/// values.
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

// ─────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_profile::DocumentProfile;
    use crate::format::Format;
    use crate::project::index::ProjectIndex;
    use crate::project::listing::ResolvedListing;
    use crate::project::listing::config::{
        FeedType, Listing, ListingFeedOptions, ListingType, apply_type_defaults,
    };
    use crate::project::listing::item::ListingItem;
    use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use crate::render::BinaryDependencies;
    use quarto_pandoc_types::ConfigMapEntry;
    use quarto_source_map::SourceInfo;
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

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

    fn make_item(title: &str) -> ListingItem {
        ListingItem {
            title: title.to_string(),
            subtitle: None,
            description: None,
            author: None,
            authors: vec![],
            date: None,
            date_modified: None,
            categories: vec![],
            image: None,
            image_alt: None,
            image_lazy_loading: None,
            reading_time_minutes: None,
            word_count: None,
            source_path: PathBuf::from(format!("posts/{}.qmd", title)),
            output_href: format!("posts/{}.html", title),
            extra: BTreeMap::new(),
        }
    }

    fn make_listing(id: &str, feed: Option<ListingFeedOptions>) -> Listing {
        let mut l = Listing {
            id: id.to_string(),
            kind: ListingType::Default,
            feed,
            ..Listing::default()
        };
        apply_type_defaults(&mut l);
        l
    }

    fn default_feed_options() -> ListingFeedOptions {
        ListingFeedOptions {
            items: None,
            kind: FeedType::Full,
            title: None,
            description: None,
            categories: Vec::new(),
            image: None,
            language: None,
            xml_stylesheet: None,
        }
    }

    fn site_meta() -> ConfigValue {
        map(vec![(
            "website",
            map(vec![
                ("site-url", s("https://example.com")),
                ("title", s("Example Site")),
            ]),
        )])
    }

    fn no_url_meta() -> ConfigValue {
        map(vec![("website", map(vec![("title", s("Example"))]))])
    }

    fn make_project(project_dir: &Path) -> ProjectContext {
        ProjectContext {
            dir: project_dir.to_path_buf(),
            config: ProjectConfig::default(),
            is_single_file: false,
            files: vec![DocumentInfo::from_path(project_dir.join("posts.qmd"))],
            output_dir: project_dir.join("_site"),
        }
    }

    async fn run_transform(
        project_dir: &Path,
        meta: ConfigValue,
        resolved: Vec<ResolvedListing>,
    ) -> Pandoc {
        let project = make_project(project_dir);
        let mut doc = DocumentInfo::from_path(project_dir.join("posts.qmd"));
        doc.output = Some(project_dir.join("_site").join("posts.html"));
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let index = Arc::new(ProjectIndex::new(Vec::<DocumentProfile>::new()));
        let mut ctx =
            RenderContext::new(&project, &doc, &format, &binaries).with_project_index(index);
        ctx.resolved_listings = resolved;

        let mut ast = Pandoc {
            meta,
            blocks: vec![],
        };

        ListingFeedLinkTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .expect("transform should not error");
        ast
    }

    /// Extract every string in `ast.meta.rendered.includes.header`.
    fn header_includes(ast: &Pandoc) -> Vec<String> {
        let Some(slot) = ast.meta.get_path(&["rendered", "includes", "header"]) else {
            return Vec::new();
        };
        let ConfigValueKind::Array(items) = &slot.value else {
            return Vec::new();
        };
        items.iter().filter_map(|v| v.as_plain_text()).collect()
    }

    // ---- Plan test #25: alternate link for main feed -----

    #[tokio::test]
    async fn link_inject_adds_alternate_for_main_feed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let resolved = vec![ResolvedListing {
            listing: make_listing("listing-1", Some(default_feed_options())),
            items: vec![make_item("foo")],
        }];
        let ast = run_transform(dir.path(), site_meta(), resolved).await;
        let includes = header_includes(&ast);
        let link = includes
            .iter()
            .find(|s| {
                s.contains(r#"rel="alternate""#) && s.contains(r#"type="application/rss+xml""#)
            })
            .unwrap_or_else(|| panic!("expected alternate link; got: {:?}", includes));
        assert!(
            link.contains(r#"href="posts.xml""#),
            "expected href to point at posts.xml; got: {}",
            link
        );
        assert!(
            link.contains(r#"title="Example Site""#),
            "expected title to fall back to website.title; got: {}",
            link
        );
    }

    // ---- Plan test #26: skips when no feed -----

    #[tokio::test]
    async fn link_inject_skips_when_no_feed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let resolved = vec![ResolvedListing {
            listing: make_listing("no-feed", None),
            items: vec![make_item("foo")],
        }];
        let ast = run_transform(dir.path(), site_meta(), resolved).await;
        let includes = header_includes(&ast);
        assert!(
            includes
                .iter()
                .all(|s| !s.contains(r#"type="application/rss+xml""#)),
            "no alternate link expected; got: {:?}",
            includes
        );
    }

    // ---- Plan test #27: skips when no site-url -----

    #[tokio::test]
    async fn link_inject_skips_when_no_site_url() {
        let dir = tempfile::tempdir().expect("tempdir");
        let resolved = vec![ResolvedListing {
            listing: make_listing("listing-1", Some(default_feed_options())),
            items: vec![make_item("foo")],
        }];
        let ast = run_transform(dir.path(), no_url_meta(), resolved).await;
        let includes = header_includes(&ast);
        assert!(
            includes
                .iter()
                .all(|s| !s.contains(r#"type="application/rss+xml""#)),
            "no alternate link expected without site-url; got: {:?}",
            includes
        );
    }

    // ---- Plan test #29: feed.title overrides website.title -----

    #[tokio::test]
    async fn link_inject_uses_feed_title_when_set() {
        let dir = tempfile::tempdir().expect("tempdir");
        let opts = ListingFeedOptions {
            title: Some("My Feed".to_string()),
            ..default_feed_options()
        };
        let resolved = vec![ResolvedListing {
            listing: make_listing("listing-1", Some(opts)),
            items: vec![make_item("foo")],
        }];
        let ast = run_transform(dir.path(), site_meta(), resolved).await;
        let includes = header_includes(&ast);
        let link = includes
            .iter()
            .find(|s| s.contains(r#"type="application/rss+xml""#))
            .expect("alternate link should exist");
        assert!(
            link.contains(r#"title="My Feed""#),
            "expected feed.title to win over website.title; got: {}",
            link
        );
    }

    // ---- multi-listing qualifier emits multiple alternate links --

    #[tokio::test]
    async fn link_inject_multi_listing_emits_qualified_hrefs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let resolved = vec![
            ResolvedListing {
                listing: make_listing("listing-a", Some(default_feed_options())),
                items: vec![make_item("foo")],
            },
            ResolvedListing {
                listing: make_listing("listing-b", Some(default_feed_options())),
                items: vec![make_item("bar")],
            },
        ];
        let ast = run_transform(dir.path(), site_meta(), resolved).await;
        let includes = header_includes(&ast);
        let alt: Vec<&String> = includes
            .iter()
            .filter(|s| s.contains(r#"type="application/rss+xml""#))
            .collect();
        assert_eq!(alt.len(), 2, "expected two alternate links; got: {:?}", alt);
        assert!(
            alt.iter()
                .any(|s| s.contains(r#"href="posts-listing-a.xml""#)),
            "expected qualified href for listing-a; got: {:?}",
            alt
        );
        assert!(
            alt.iter()
                .any(|s| s.contains(r#"href="posts-listing-b.xml""#)),
            "expected qualified href for listing-b; got: {:?}",
            alt
        );
    }

    // ---- escape_html_attr unit ------

    #[test]
    fn escape_html_attr_handles_special_characters() {
        assert_eq!(
            escape_html_attr(r#"a "b" & <c>"#),
            r#"a &quot;b&quot; &amp; &lt;c&gt;"#
        );
    }
}
