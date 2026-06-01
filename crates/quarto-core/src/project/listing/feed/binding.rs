/*
 * project/listing/feed/binding.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Typed feed-channel + per-item bindings consumed by the L9 feed
//! templates.
//!
//! See `claude-notes/plans/2026-05-08-listings-L9-rss-feeds.md`
//! §"Architecture" → "Templates" for the template surface this
//! module produces. The templates emit `$channel.title$` /
//! `$item.link$` / etc. **verbatim** — server-side XML escaping
//! happens here, in the binding, before the values reach the
//! template evaluator. Mirrors the precedent set by L3's
//! `crates/quarto-core/src/project/listing/binding.rs`, where
//! pre-rendered helper strings (`image_html`, `metadata_attrs`,
//! `category_html`) flow through the binding rather than being
//! computed in the template.
//!
//! The two payload shapes:
//!
//! - [`FeedChannel`] — channel-level metadata for the preamble
//!   template (title, link, description, image, generator,
//!   lastBuildDate, optional language, optional xml-stylesheet).
//! - [`FeedItem`] — per-item metadata for the item template
//!   (title, link, guid, description-element, authors,
//!   categories, pubDate, image). The
//!   [`FeedItem::description_element`] field is either a
//!   `<description><![CDATA[…]]></description>` block (for
//!   `metadata` feeds, where the description is taken verbatim
//!   from the post's frontmatter) or a placeholder envelope
//!   (`<description>{B4F502887207:posts/foo.html}</description>`)
//!   that the L9 post-render step substitutes against the
//!   sibling's rendered HTML.
//!
//! Native-only via the parent module's cfg gate: this file uses
//! the `imagesize` crate, which is target-gated to
//! `cfg(not(target_arch = "wasm32"))` in `quarto-core`'s
//! `Cargo.toml`. WASM-side feed work is limited to the
//! link-inject transform, which doesn't consult these structs.

use std::path::Path;

use quarto_pandoc_types::ConfigValue;
use time::OffsetDateTime;
use time::format_description::well_known::{Rfc2822, Rfc3339};
use time::macros::format_description;

use crate::project::listing::config::{FeedType, ListingFeedOptions};
use crate::project::listing::item::ListingItem;
use crate::project::website_config::{website_site_url, website_title};

/// Q1-verbatim placeholder token. The post-render substitute step
/// matches `<description>\{B4F502887207:([^}]+)\}</description>` and
/// replaces the body with engine-rendered preview content from the
/// sibling output. Mirrors Q1's
/// `external-sources/quarto-cli/src/project/types/website/listing/website-listing-feed.ts`
/// `placeholder()` exactly.
pub const FEED_PLACEHOLDER_TOKEN: &str = "B4F502887207";

/// Fixed generator string for v1. Q1 emits
/// `quarto-${quartoConfig.version()}`; Q2's version story is in flux
/// (see plan D12), so v1 ships a stable `quarto-2`. A follow-up bd
/// at L9 close-out swaps in the real version.
pub const FEED_GENERATOR: &str = "quarto-2";

/// Channel-level metadata for the preamble template.
///
/// All string fields are XML-escaped at construction time; the
/// preamble template emits them verbatim inside `<title>` /
/// `<description>` / `<atom:link href="..."/>` etc. with no
/// further escaping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedChannel {
    /// XML-escaped channel title. Cascade:
    /// `feed.title` → `website.title` → empty string.
    pub title: String,
    /// Absolute URL of the host page, e.g.
    /// `https://example.com/posts.html`.
    pub link: String,
    /// Absolute URL of the feed file itself, e.g.
    /// `https://example.com/posts.xml`. Used in the
    /// `<atom:link rel="self">` element.
    pub feed_link: String,
    /// XML-escaped channel description. Cascade:
    /// `feed.description` → `website.description` → empty string.
    pub description: String,
    /// XML-escaped optional language code (e.g. `"en-US"`). When
    /// `None`, the preamble template's `$if(channel.language)$`
    /// branch is taken and the `<language>` element is omitted.
    pub language: Option<String>,
    /// Generator string (e.g. `"quarto-2"`). Always present.
    pub generator: String,
    /// RFC 2822 last-build date. Set to the most-recent
    /// item.date when available, falling back to "now" at build
    /// time.
    pub last_build_date: String,
    /// Optional channel image block.
    pub image: Option<FeedChannelImage>,
    /// Optional XML stylesheet href, emitted as
    /// `<?xml-stylesheet type="text/xsl" media="screen" href="..."?>`
    /// in the preamble. The template emits the value verbatim;
    /// L9 does not copy the stylesheet file to the output dir or
    /// validate the path. Q1 does the same.
    pub xml_stylesheet: Option<String>,
}

/// Channel-image block. All fields XML-escaped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedChannelImage {
    /// Absolute URL of the image, ready for `<url>` text content.
    pub url: String,
    /// Channel title repeated (RSS 2.0 convention).
    pub title: String,
    /// Channel link repeated.
    pub link: String,
    /// Optional `<height>` (image-header parsed dimensions, scaled
    /// to feed limits).
    pub height: Option<u32>,
    /// Optional `<width>`.
    pub width: Option<u32>,
}

/// Per-item metadata for the item template.
///
/// Constructed once per resolved item; the stage transform
/// iterates and emits one `<item>` per [`FeedItem`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedItem {
    /// XML-escaped item title.
    pub title: String,
    /// Absolute URL of the linked-to HTML output.
    pub link: String,
    /// `<guid>` value. v1 uses `link` verbatim (with
    /// `isPermaLink="true"` semantics, the default).
    pub guid: String,
    /// Either a `<description><![CDATA[…]]></description>` block
    /// (metadata feeds) or a placeholder envelope (partial /
    /// full feeds). Emitted by the item template as a single
    /// `$item.description-element$` reference; the template never
    /// has to switch on feed type.
    pub description_element: String,
    /// XML-escaped author display strings, one per `<dc:creator>`.
    /// Empty vec → no creator elements.
    pub authors: Vec<String>,
    /// XML-escaped categories, one per `<category>` element.
    pub categories: Vec<String>,
    /// RFC 2822-formatted `<pubDate>`. `None` → no `<pubDate>`
    /// element emitted (matches Q1).
    pub pub_date_rfc822: Option<String>,
    /// Optional `<media:content>` image block.
    pub image: Option<FeedItemImage>,
}

/// Per-item image block.
///
/// `attrs` is a pre-built attribute fragment with a leading space,
/// e.g. ` width="400" height="300" type="image/png"`. The item
/// template emits `<media:content url="$item.image.url$" medium="image"$item.image.attrs$/>`
/// — concatenating the attrs into the same element opener.
///
/// `attrs` is empty for absolute URLs, data URIs, and unreadable
/// local files: in those cases the binding emits a bare
/// `<media:content url="..." medium="image"/>`, matching Q1's
/// graceful degradation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedItemImage {
    pub url: String,
    pub attrs: String,
}

// ─────────────────────────────────────────────────────────────────
// Channel builder
// ─────────────────────────────────────────────────────────────────

/// Build the channel binding for a feed-configured listing.
///
/// Arguments:
/// - `feed_options`: parsed `feed:` config from the listing.
/// - `project_meta`: merged metadata for fallback to `website.*`
///   keys.
/// - `host_output_href`: project-relative output path of the host
///   page, e.g. `"posts.html"` (forward slashes; no leading
///   slash). Used to compute the channel `<link>`.
/// - `feed_output_href`: project-relative path of the feed file
///   relative to the output dir, e.g. `"posts.xml"`. Used to
///   compute the `<atom:link rel="self">` href.
/// - `last_build_date_iso`: ISO/RFC date string of the most recent
///   item, when one is available. `None` → use the current time.
/// - `project_dir`: filesystem path to the project root, for
///   image-dimension lookup via the `imagesize` crate.
pub fn build_feed_channel(
    feed_options: &ListingFeedOptions,
    project_meta: &ConfigValue,
    host_output_href: &str,
    feed_output_href: &str,
    last_build_date_iso: Option<&str>,
    project_dir: &Path,
) -> FeedChannel {
    let site_url = website_site_url(project_meta).unwrap_or_default();

    let title_raw = feed_options
        .title
        .clone()
        .or_else(|| website_title(project_meta))
        .unwrap_or_default();

    let description_raw = feed_options
        .description
        .clone()
        .or_else(|| website_description(project_meta))
        .unwrap_or_default();

    let language_raw = feed_options.language.clone();

    let link = absolute_url(&site_url, host_output_href);
    let feed_link = absolute_url(&site_url, feed_output_href);

    let last_build_date = last_build_date_iso
        .and_then(format_pub_date_rfc822)
        .unwrap_or_else(now_rfc2822);

    let image = build_channel_image(
        feed_options,
        project_meta,
        &site_url,
        &title_raw,
        &link,
        project_dir,
    );

    let xml_stylesheet = feed_options
        .xml_stylesheet
        .as_ref()
        .and_then(|p| p.to_str())
        .map(xml_escape_attr);

    FeedChannel {
        title: xml_escape_text(&title_raw),
        link,
        feed_link,
        description: xml_escape_text(&description_raw),
        language: language_raw.map(|s| xml_escape_text(&s)),
        generator: FEED_GENERATOR.to_string(),
        last_build_date,
        image,
        xml_stylesheet,
    }
}

fn build_channel_image(
    feed_options: &ListingFeedOptions,
    project_meta: &ConfigValue,
    site_url: &str,
    channel_title_raw: &str,
    channel_link: &str,
    project_dir: &Path,
) -> Option<FeedChannelImage> {
    let src_raw = feed_options
        .image
        .clone()
        .or_else(|| website_image(project_meta))?;
    if src_raw.is_empty() {
        return None;
    }
    let url = absolute_url(site_url, &src_raw);

    let (width, height) = if is_external_or_data_uri(&src_raw) {
        (None, None)
    } else {
        match imagesize::size(project_dir.join(&src_raw)) {
            Ok(sz) => {
                let (h, w) = scale_to_feed_dimensions(sz.height as u32, sz.width as u32);
                (Some(w), Some(h))
            }
            Err(_) => (None, None),
        }
    };

    Some(FeedChannelImage {
        url: xml_escape_text(&url),
        title: xml_escape_text(channel_title_raw),
        link: channel_link.to_string(),
        height,
        width,
    })
}

// ─────────────────────────────────────────────────────────────────
// Item builder
// ─────────────────────────────────────────────────────────────────

/// Build the per-item binding for a single listing item.
///
/// The `description_element` slot is determined by `feed_options.kind`:
/// - `Metadata`: `<description><![CDATA[<post description>]]></description>`,
///   inlined directly from `item.description`. No post-render
///   substitution needed.
/// - `Partial` / `Full`: `<description>{B4F502887207:<href>}</description>`,
///   a placeholder the post-render step rewrites by reading the
///   sibling's rendered HTML.
pub fn build_feed_item(
    item: &ListingItem,
    feed_options: &ListingFeedOptions,
    site_url: &str,
    project_dir: &Path,
) -> FeedItem {
    let link = absolute_url(site_url, &item.output_href);
    let description_element = match feed_options.kind {
        FeedType::Metadata => {
            let desc = item.description.as_deref().unwrap_or("");
            format!("<description><![CDATA[{}]]></description>", desc)
        }
        FeedType::Partial | FeedType::Full => {
            format!(
                "<description>{{{}:{}}}</description>",
                FEED_PLACEHOLDER_TOKEN, item.output_href
            )
        }
    };

    let pub_date_rfc822 = item.date.as_deref().and_then(format_pub_date_rfc822);

    let image = item
        .image
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|src| build_item_image(src, site_url, project_dir));

    FeedItem {
        title: xml_escape_text(&item.title),
        link: link.clone(),
        guid: link,
        description_element,
        authors: item.authors.iter().map(|a| xml_escape_text(a)).collect(),
        categories: item.categories.iter().map(|c| xml_escape_text(c)).collect(),
        pub_date_rfc822,
        image,
    }
}

/// Resolve an item's `image:` field into a [`FeedItemImage`].
///
/// - Absolute URLs (`http(s)://...`) and data URIs (`data:...`) are
///   passed through verbatim with empty `attrs` — `<media:content>`
///   gets `url="..." medium="image"` only. Q1 does the same: it
///   only looks up dimensions for project-local paths.
/// - Local paths are resolved as `project_dir.join(src)` and
///   parsed by `imagesize`. On success, dimensions are scaled per
///   [`scale_to_feed_dimensions`] and a `type="<mime>"` attribute
///   is added when the extension maps to a known MIME type.
/// - Unreadable / malformed images degrade to empty `attrs` (same
///   as the absolute-URL branch).
fn build_item_image(src: &str, site_url: &str, project_dir: &Path) -> FeedItemImage {
    let url = absolute_url(site_url, src);
    let url_escaped = xml_escape_attr(&url);

    if is_external_or_data_uri(src) {
        return FeedItemImage {
            url: url_escaped,
            attrs: String::new(),
        };
    }

    let abs_path = project_dir.join(src);
    let attrs = match imagesize::size(&abs_path) {
        Ok(sz) => {
            let (h, w) = scale_to_feed_dimensions(sz.height as u32, sz.width as u32);
            let mime = mime_for_path(&abs_path);
            build_image_attrs(h, w, mime)
        }
        Err(_) => String::new(),
    };

    FeedItemImage {
        url: url_escaped,
        attrs,
    }
}

/// Pure helper: build the `<media:content>` attribute fragment for
/// a successfully sized local image. The fragment has a leading
/// space so it can be concatenated directly after `medium="image"`
/// in the item template.
fn build_image_attrs(height: u32, width: u32, mime: Option<&str>) -> String {
    let mime_part = mime
        .map(|m| format!(r#" type="{}""#, m))
        .unwrap_or_default();
    format!(r#"{} width="{}" height="{}""#, mime_part, width, height)
}

// ─────────────────────────────────────────────────────────────────
// Helpers: URL, escaping, dates, image dimensions
// ─────────────────────────────────────────────────────────────────

/// True if `src` is already a fully-qualified URL or a data URI.
/// Mirrors Q1's `absoluteUrl` short-circuit.
fn is_external_or_data_uri(src: &str) -> bool {
    src.starts_with("http://")
        || src.starts_with("https://")
        || src.starts_with("data:")
        || src.starts_with("//")
}

/// Build an absolute URL by joining `site_url` and a project-
/// relative path. Mirrors `website_post_render::write_sitemap`'s
/// pattern: trim trailing `/` from the base, trim leading `/`
/// from the relative, single-`/` join. Leaves already-absolute
/// inputs alone.
fn absolute_url(site_url: &str, rel: &str) -> String {
    if is_external_or_data_uri(rel) {
        return rel.to_string();
    }
    let base = site_url.trim_end_matches('/');
    let path = rel.trim_start_matches('/');
    if base.is_empty() {
        return path.to_string();
    }
    format!("{}/{}", base, path)
}

/// XML-escape character data: `&`, `<`, `>`. Sufficient for all
/// element-text positions in RSS 2.0.
fn xml_escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
    out
}

/// XML-escape an attribute value (double-quoted). Adds `"` to the
/// `xml_escape_text` set. Apostrophes pass through (we use double
/// quotes throughout).
fn xml_escape_attr(s: &str) -> String {
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

/// Read `website.description` from merged metadata. Local helper
/// (no equivalent in `website_config.rs` because Phase-7 didn't
/// need it; consider hoisting if a third caller appears).
fn website_description(meta: &ConfigValue) -> Option<String> {
    meta.get_path(&["website", "description"])
        .and_then(|v| v.as_plain_text())
}

/// Read `website.image` from merged metadata. Same comment as
/// [`website_description`]: hoist to `website_config` once a third
/// caller exists.
fn website_image(meta: &ConfigValue) -> Option<String> {
    // Q1's `websiteImage` returns `{ src, ... }`; v1 reads the
    // bare `website.image` path. If/when L11 close-out finds an
    // author using the structured form, we can broaden.
    meta.get_path(&["website", "image"])
        .and_then(|v| v.as_plain_text())
}

/// Format an arbitrary date string as RFC 2822 (the format RSS
/// `<pubDate>` expects). Accepts (in order):
/// - RFC 3339 / ISO-8601 with time-zone (e.g.
///   `"2026-05-08T10:30:00Z"`),
/// - RFC 2822 (e.g. `"Thu, 08 May 2026 10:30:00 +0000"`),
/// - Date-only ISO-8601 (`"2026-05-08"`), interpreted as
///   midnight UTC.
///
/// Returns `None` for unparseable inputs (the caller omits
/// `<pubDate>` rather than emitting garbage).
pub fn format_pub_date_rfc822(date: &str) -> Option<String> {
    let dt = OffsetDateTime::parse(date, &Rfc3339)
        .or_else(|_| OffsetDateTime::parse(date, &Rfc2822))
        .or_else(|_| {
            let date_only = format_description!("[year]-[month]-[day]");
            time::Date::parse(date, &date_only).map(|d| {
                d.with_hms(0, 0, 0)
                    .expect("0:0:0 is always valid")
                    .assume_utc()
            })
        })
        .ok()?;
    dt.format(&Rfc2822).ok()
}

/// Current-time RFC 2822 string for `lastBuildDate` when no item
/// date is available. UTC, locale-independent.
fn now_rfc2822() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc2822)
        .unwrap_or_else(|_| String::from("Thu, 01 Jan 1970 00:00:00 +0000"))
}

/// Mirror of Q1's `feedImageSize`: scale (height, width) into the
/// 400x144 feed-image envelope, preserving aspect ratio. Returns
/// `(height, width)` (Q1 order).
fn scale_to_feed_dimensions(height: u32, width: u32) -> (u32, u32) {
    const MAX_HEIGHT: u32 = 400;
    const MAX_WIDTH: u32 = 144;
    if height <= MAX_HEIGHT && width <= MAX_WIDTH {
        return (height, width);
    }
    if height == 0 || width == 0 {
        return (height, width);
    }
    let h_scale = MAX_HEIGHT as f64 / height as f64;
    let w_scale = MAX_WIDTH as f64 / width as f64;
    let scale = h_scale.min(w_scale);
    (
        ((height as f64) * scale).round() as u32,
        ((width as f64) * scale).round() as u32,
    )
}

/// Map a path's extension to an image MIME type. Returns `None`
/// for unknown extensions (caller omits the `type` attribute).
fn mime_for_path(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" | "jfif" | "pjpeg" | "pjp" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "svg" => Some("image/svg+xml"),
        "apng" => Some("image/apng"),
        "avif" => Some("image/avif"),
        _ => None,
    }
}

// ─────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::ConfigMapEntry;
    use quarto_source_map::SourceInfo;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    // ---- Fixture helpers --------------------------------------------

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

    fn empty_listing_item() -> ListingItem {
        ListingItem {
            title: String::new(),
            subtitle: None,
            description: None,
            author: None,
            authors: Vec::new(),
            date: None,
            date_modified: None,
            categories: Vec::new(),
            image: None,
            image_alt: None,
            image_lazy_loading: None,
            reading_time_minutes: None,
            word_count: None,
            source_path: PathBuf::new(),
            output_href: String::new(),
            extra: BTreeMap::new(),
        }
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

    /// 100x100 PNG header (sufficient for `imagesize` to parse
    /// dimensions from). Bytes correspond to:
    ///   89 50 4E 47 0D 0A 1A 0A   (PNG signature, 8 bytes)
    ///   00 00 00 0D                (IHDR chunk length = 13)
    ///   49 48 44 52                ("IHDR")
    ///   00 00 00 64                (width = 100, BE)
    ///   00 00 00 64                (height = 100, BE)
    ///   08 02 00 00 00             (bit depth=8, color type=2 (RGB),
    ///                               compression=0, filter=0, interlace=0)
    /// `imagesize` reads dimensions from the IHDR header alone; the
    /// CRC and IDAT chunks are not required.
    fn png_100x100() -> Vec<u8> {
        vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // signature
            0x00, 0x00, 0x00, 0x0D, // IHDR length
            0x49, 0x48, 0x44, 0x52, // "IHDR"
            0x00, 0x00, 0x00, 0x64, // width = 100
            0x00, 0x00, 0x00, 0x64, // height = 100
            0x08, 0x02, 0x00, 0x00, 0x00, // depth, color, compression, filter, interlace
            // CRC (placeholder; imagesize doesn't verify it)
            0x00, 0x00, 0x00, 0x00,
        ]
    }

    /// 4000x3000 PNG header (used to exercise the scaling branch
    /// in `scale_to_feed_dimensions`). Width=4000=0x0FA0, height=3000=0x0BB8.
    fn png_4000x3000() -> Vec<u8> {
        vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, //
            0x00, 0x00, 0x00, 0x0D, //
            0x49, 0x48, 0x44, 0x52, //
            0x00, 0x00, 0x0F, 0xA0, // width = 4000
            0x00, 0x00, 0x0B, 0xB8, // height = 3000
            0x08, 0x02, 0x00, 0x00, 0x00, //
            0x00, 0x00, 0x00, 0x00, //
        ]
    }

    // ---- xml_escape_text / _attr -------------------------------------

    #[test]
    fn xml_escape_text_handles_amp_lt_gt() {
        assert_eq!(xml_escape_text("a & b < c > d"), "a &amp; b &lt; c &gt; d");
    }

    #[test]
    fn xml_escape_text_passes_quotes_through() {
        // text content doesn't need to escape quotes.
        assert_eq!(xml_escape_text(r#"He said "hi"."#), r#"He said "hi"."#);
    }

    #[test]
    fn xml_escape_attr_also_escapes_quotes() {
        assert_eq!(
            xml_escape_attr(r#"foo "bar" & <baz>"#),
            r#"foo &quot;bar&quot; &amp; &lt;baz&gt;"#
        );
    }

    // ---- absolute_url -----------------------------------------------

    #[test]
    fn absolute_url_joins_base_and_path() {
        assert_eq!(
            absolute_url("https://example.com", "posts/foo.html"),
            "https://example.com/posts/foo.html"
        );
    }

    #[test]
    fn absolute_url_strips_trailing_slash_on_base() {
        assert_eq!(
            absolute_url("https://example.com/", "posts/foo.html"),
            "https://example.com/posts/foo.html"
        );
    }

    #[test]
    fn absolute_url_strips_leading_slash_on_path() {
        assert_eq!(
            absolute_url("https://example.com", "/posts/foo.html"),
            "https://example.com/posts/foo.html"
        );
    }

    #[test]
    fn absolute_url_passes_external_through() {
        assert_eq!(
            absolute_url("https://example.com", "https://other.example/img.png"),
            "https://other.example/img.png"
        );
    }

    #[test]
    fn absolute_url_passes_data_uri_through() {
        let data = "data:image/png;base64,iVBORw0KG";
        assert_eq!(absolute_url("https://example.com", data), data);
    }

    // ---- scale_to_feed_dimensions / mime_for_path ----

    #[test]
    fn scale_under_limits_returns_input() {
        assert_eq!(scale_to_feed_dimensions(100, 100), (100, 100));
    }

    #[test]
    fn scale_height_bottlenecked() {
        // 4000h × 3000w should bottleneck on max-height (400):
        // h_scale = 400/4000 = 0.1; w_scale = 144/3000 ≈ 0.048
        // → use w_scale (0.048): h = 192, w = 144.
        let (h, w) = scale_to_feed_dimensions(4000, 3000);
        assert_eq!((h, w), (192, 144));
    }

    #[test]
    fn scale_width_bottlenecked() {
        // 200h × 800w: h_scale = 2, w_scale = 0.18 → 36, 144.
        let (h, w) = scale_to_feed_dimensions(200, 800);
        assert_eq!((h, w), (36, 144));
    }

    #[test]
    fn mime_for_path_known_extensions() {
        assert_eq!(mime_for_path(Path::new("foo.png")), Some("image/png"));
        assert_eq!(mime_for_path(Path::new("foo.JPG")), Some("image/jpeg"));
        assert_eq!(mime_for_path(Path::new("foo.svg")), Some("image/svg+xml"));
        assert_eq!(mime_for_path(Path::new("foo.unknown")), None);
        assert_eq!(mime_for_path(Path::new("noext")), None);
    }

    // ---- format_pub_date_rfc822 -------------------------------------

    #[test]
    fn format_pub_date_iso8601_z() {
        let out = format_pub_date_rfc822("2026-05-08T10:30:00Z");
        assert_eq!(out.as_deref(), Some("Fri, 08 May 2026 10:30:00 +0000"));
    }

    #[test]
    fn format_pub_date_date_only_midnight_utc() {
        // Test #9: item with date "2026-05-08" → midnight UTC.
        let out = format_pub_date_rfc822("2026-05-08");
        assert_eq!(out.as_deref(), Some("Fri, 08 May 2026 00:00:00 +0000"));
    }

    #[test]
    fn format_pub_date_rfc2822_round_trip() {
        let out = format_pub_date_rfc822("Fri, 08 May 2026 10:30:00 +0000");
        assert_eq!(out.as_deref(), Some("Fri, 08 May 2026 10:30:00 +0000"));
    }

    #[test]
    fn format_pub_date_unparseable_returns_none() {
        assert_eq!(format_pub_date_rfc822("not a date"), None);
        assert_eq!(format_pub_date_rfc822(""), None);
    }

    // ---- Plan test #7: build_channel_context_full_metadata ---------

    #[test]
    fn build_channel_full_metadata() {
        let project_meta = map(vec![(
            "website",
            map(vec![
                ("site-url", s("https://example.com/")),
                ("title", s("Example <Site>")),
                ("description", s("My & site")),
            ]),
        )]);
        let feed_options = ListingFeedOptions {
            language: Some("en-US".to_string()),
            ..default_feed_options()
        };
        let project_dir = std::env::temp_dir();
        let channel = build_feed_channel(
            &feed_options,
            &project_meta,
            "posts.html",
            "posts.xml",
            None,
            &project_dir,
        );
        // Title falls through to website.title and is XML-escaped.
        assert_eq!(channel.title, "Example &lt;Site&gt;");
        // Description falls through to website.description.
        assert_eq!(channel.description, "My &amp; site");
        // URLs are built absolutely.
        assert_eq!(channel.link, "https://example.com/posts.html");
        assert_eq!(channel.feed_link, "https://example.com/posts.xml");
        // Generator is "quarto-2".
        assert_eq!(channel.generator, FEED_GENERATOR);
        // Language is escaped (none of these chars need escaping;
        // the value flows through verbatim).
        assert_eq!(channel.language.as_deref(), Some("en-US"));
        // No xml-stylesheet, no image.
        assert!(channel.xml_stylesheet.is_none());
        assert!(channel.image.is_none());
        // last_build_date defaults to "now" when no most-recent
        // item date is supplied; we only assert the format shape.
        assert!(
            channel.last_build_date.contains(',') && channel.last_build_date.contains(':'),
            "last_build_date should be RFC 2822-ish; got {}",
            channel.last_build_date
        );
    }

    // ---- Plan test #8: build_channel_falls_back_to_website_keys ---

    #[test]
    fn build_channel_uses_feed_title_when_set() {
        let project_meta = map(vec![(
            "website",
            map(vec![
                ("site-url", s("https://example.com")),
                ("title", s("Site Title")),
            ]),
        )]);
        let feed_options = ListingFeedOptions {
            title: Some("Feed Title".to_string()),
            description: Some("Feed Desc".to_string()),
            ..default_feed_options()
        };
        let channel = build_feed_channel(
            &feed_options,
            &project_meta,
            "posts.html",
            "posts.xml",
            None,
            &std::env::temp_dir(),
        );
        assert_eq!(channel.title, "Feed Title");
        assert_eq!(channel.description, "Feed Desc");
    }

    #[test]
    fn build_channel_falls_back_to_website_when_feed_unset() {
        let project_meta = map(vec![(
            "website",
            map(vec![
                ("site-url", s("https://example.com")),
                ("title", s("Site Title")),
                ("description", s("Site Desc")),
            ]),
        )]);
        let channel = build_feed_channel(
            &default_feed_options(),
            &project_meta,
            "posts.html",
            "posts.xml",
            None,
            &std::env::temp_dir(),
        );
        assert_eq!(channel.title, "Site Title");
        assert_eq!(channel.description, "Site Desc");
    }

    // ---- Plan test #7 cont'd: last_build_date from item date ----

    #[test]
    fn build_channel_last_build_date_uses_supplied_iso() {
        let project_meta = map(vec![("website", map(vec![("site-url", s("x"))]))]);
        let channel = build_feed_channel(
            &default_feed_options(),
            &project_meta,
            "h.html",
            "h.xml",
            Some("2026-05-08T10:30:00Z"),
            &std::env::temp_dir(),
        );
        assert_eq!(channel.last_build_date, "Fri, 08 May 2026 10:30:00 +0000");
    }

    // ---- Plan test #9: build_item_context_pubdate_rfc822_format ---

    #[test]
    fn build_item_pub_date_rfc822_from_date_only() {
        let mut item = empty_listing_item();
        item.title = "Foo".to_string();
        item.date = Some("2026-05-08".to_string());
        item.output_href = "posts/foo.html".to_string();
        let feed_options = default_feed_options();
        let fi = build_feed_item(&item, &feed_options, "https://example.com", Path::new("/p"));
        assert_eq!(
            fi.pub_date_rfc822.as_deref(),
            Some("Fri, 08 May 2026 00:00:00 +0000")
        );
    }

    // ---- Plan test #10: xml escapes title/description/categories --

    #[test]
    fn build_item_xml_escapes_title_and_categories() {
        let mut item = empty_listing_item();
        item.title = "<script>alert(1)</script>".to_string();
        item.description = Some("a < b & c".to_string());
        item.categories = vec!["A & B".to_string(), "<C>".to_string()];
        item.output_href = "p.html".to_string();
        let feed_options = ListingFeedOptions {
            kind: FeedType::Metadata,
            ..default_feed_options()
        };
        let fi = build_feed_item(&item, &feed_options, "https://example.com", Path::new("/p"));
        assert_eq!(fi.title, "&lt;script&gt;alert(1)&lt;/script&gt;");
        assert_eq!(fi.categories, vec!["A &amp; B", "&lt;C&gt;"]);
        // Description is wrapped in CDATA verbatim — CDATA disables
        // XML escaping, so the raw `a < b & c` shows up unchanged.
        // Note: a real description containing `]]>` would need to
        // be split; v1 doesn't handle that pathology.
        assert!(
            fi.description_element
                .contains("<description><![CDATA[a < b & c]]></description>"),
            "expected CDATA-wrapped description; got: {}",
            fi.description_element
        );
    }

    // ---- Plan test #11: metadata feed inlines description -----

    #[test]
    fn build_item_metadata_inlines_description_as_cdata() {
        let mut item = empty_listing_item();
        item.title = "T".to_string();
        item.description = Some("Hello world".to_string());
        item.output_href = "posts/foo.html".to_string();
        let feed_options = ListingFeedOptions {
            kind: FeedType::Metadata,
            ..default_feed_options()
        };
        let fi = build_feed_item(&item, &feed_options, "https://example.com", Path::new("/p"));
        assert_eq!(
            fi.description_element,
            "<description><![CDATA[Hello world]]></description>"
        );
    }

    #[test]
    fn build_item_metadata_with_no_description_produces_empty_cdata() {
        let mut item = empty_listing_item();
        item.title = "T".to_string();
        item.output_href = "p.html".to_string();
        let feed_options = ListingFeedOptions {
            kind: FeedType::Metadata,
            ..default_feed_options()
        };
        let fi = build_feed_item(&item, &feed_options, "https://example.com", Path::new("/p"));
        assert_eq!(
            fi.description_element,
            "<description><![CDATA[]]></description>"
        );
    }

    // ---- Plan test #12: partial / full feeds emit placeholder -----

    #[test]
    fn build_item_partial_emits_placeholder_envelope() {
        let mut item = empty_listing_item();
        item.title = "T".to_string();
        item.output_href = "posts/foo.html".to_string();
        let feed_options = ListingFeedOptions {
            kind: FeedType::Partial,
            ..default_feed_options()
        };
        let fi = build_feed_item(&item, &feed_options, "https://example.com", Path::new("/p"));
        assert_eq!(
            fi.description_element,
            "<description>{B4F502887207:posts/foo.html}</description>"
        );
    }

    #[test]
    fn build_item_full_emits_placeholder_envelope() {
        let mut item = empty_listing_item();
        item.title = "T".to_string();
        item.output_href = "posts/bar.html".to_string();
        let feed_options = ListingFeedOptions {
            kind: FeedType::Full,
            ..default_feed_options()
        };
        let fi = build_feed_item(&item, &feed_options, "https://example.com", Path::new("/p"));
        assert_eq!(
            fi.description_element,
            "<description>{B4F502887207:posts/bar.html}</description>"
        );
    }

    // ---- Plan test #13: build_item_image_local_with_imagesize -----

    #[test]
    fn build_item_image_local_png_yields_dimensions_and_mime() {
        let dir = tempfile::tempdir().expect("tempdir");
        let img_dir = dir.path().join("posts");
        std::fs::create_dir_all(&img_dir).unwrap();
        let img_path = img_dir.join("cover.png");
        std::fs::write(&img_path, png_100x100()).unwrap();

        let img = build_item_image("posts/cover.png", "https://example.com", dir.path());
        assert_eq!(img.url, "https://example.com/posts/cover.png");
        // 100x100 ≤ both limits, so no scaling.
        assert_eq!(img.attrs, r#" type="image/png" width="100" height="100""#);
    }

    #[test]
    fn build_item_image_local_png_scales_oversize_dimensions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let img_path = dir.path().join("big.png");
        std::fs::write(&img_path, png_4000x3000()).unwrap();

        let img = build_item_image("big.png", "https://example.com", dir.path());
        // PNG fixture is 4000w × 3000h. With limits (max_h=400, max_w=144):
        //   h_scale = 400/3000 ≈ 0.133
        //   w_scale = 144/4000 = 0.036  ← smaller, wins
        //   height_out = round(3000 × 0.036) = 108
        //   width_out  = round(4000 × 0.036) = 144
        assert_eq!(img.attrs, r#" type="image/png" width="144" height="108""#);
    }

    // ---- Plan test #14: absolute URL → no attrs ------------

    #[test]
    fn build_item_image_absolute_url_emits_empty_attrs() {
        let img = build_item_image(
            "https://example.com/already-abs.png",
            "https://example.com",
            Path::new("/no/such/dir"),
        );
        assert_eq!(img.url, "https://example.com/already-abs.png");
        assert_eq!(img.attrs, "");
    }

    #[test]
    fn build_item_image_data_uri_emits_empty_attrs() {
        let data = "data:image/png;base64,iVBORw0KG";
        let img = build_item_image(data, "https://example.com", Path::new("/no/such/dir"));
        assert_eq!(img.url, data);
        assert_eq!(img.attrs, "");
    }

    // ---- Plan test #15: unreadable file → empty attrs -----

    #[test]
    fn build_item_image_unreadable_file_emits_empty_attrs() {
        let img = build_item_image(
            "missing.png",
            "https://example.com",
            Path::new("/no/such/dir"),
        );
        assert_eq!(img.url, "https://example.com/missing.png");
        // imagesize::size returned Err → empty attrs.
        assert_eq!(img.attrs, "");
    }

    // ---- xml-stylesheet plumbing (architecture sketch) -------

    #[test]
    fn build_channel_emits_xml_stylesheet_when_set() {
        let project_meta = map(vec![("website", map(vec![("site-url", s("x"))]))]);
        let feed_options = ListingFeedOptions {
            xml_stylesheet: Some(PathBuf::from("feed.xsl")),
            ..default_feed_options()
        };
        let channel = build_feed_channel(
            &feed_options,
            &project_meta,
            "h.html",
            "h.xml",
            None,
            &std::env::temp_dir(),
        );
        assert_eq!(channel.xml_stylesheet.as_deref(), Some("feed.xsl"));
    }
}
