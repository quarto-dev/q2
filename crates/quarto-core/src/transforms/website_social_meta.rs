/*
 * website_social_meta.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * AST transform: Open Graph and Twitter Card meta tags for website pages.
 */

//! Write `og:*` and `twitter:*` `<meta>` tags for website and book pages
//! that enable `open-graph` / `twitter-card` (bd-tyvt, GH #1259).
//!
//! Port of Quarto 1's `website-meta.ts` and `util/discover-meta.ts`. Quarto
//! 1 does this as an HTML post-processor over the rendered page; q2 does it
//! on the AST, before the title block and sections exist, and hands the tags
//! to the head through `rendered.includes.header` (the channel
//! `website-favicon` and `draft-alert` use). The behaviour, including the
//! deliberate divergences from Quarto 1, is recorded against Quarto 1's own
//! output in `claude-notes/plans/2026-11-20-social-meta-tags.md`.
//!
//! **Enabling.** A provider (`open-graph`, `twitter-card`) runs when the
//! page sets it to `true` or a map, or leaves it unset and `website.<key>` is
//! `true` or a map. A page `false` turns it off. The page map's options win
//! over the site map's; a page `true` keeps the site's options (Quarto 1
//! drops them, which the plan records as a divergence).
//!
//! **Values.**
//! - Title: the provider's `title`, else the page's `pagetitle` (which
//!   `website-title-prefix` has already composed as `Page – Site`).
//! - Description: the provider's `description`, else the page's
//!   `description` / `abstract` / `subtitle`, else `website.description`,
//!   else the first top-level paragraph, cut to 200 characters at sentence
//!   punctuation. (Quarto 1 means to use the paragraph but its markdown
//!   pipeline overwrites it with `""`; q2 emits the paragraph.)
//! - Image: the page map's `image`, else the page's `image:`, else the site
//!   map's `image`; failing those, unless the page sets `image: false`, an
//!   image discovered in the body, else `website.image`. Discovery follows
//!   Quarto 1's order: an image with class `preview-image`; an image in a
//!   `cell-output-display` inside a `.preview-image` cell; an image whose
//!   file name contains `preview`, `feature`, `cover` or `thumbnail`; the
//!   first local image. Data URIs are skipped, since crawlers fetch the
//!   image by URL.
//! - Image URLs are absolute, built on `website.site-url`. Without a
//!   `site-url` the image tags are left out and `Q-5-58` says why.
//! - `og:image:width` / `og:image:height` come from the image file when it
//!   can be read, else from the declared `image-width` / `image-height`.
//!   A size declared on the site describes the site's image, so an image
//!   the page supplies only takes the page's declared size. Twitter Cards
//!   have no size tags (`Q-5-61`).
//!
//! **Types.** The options this transform reads are type-checked (`Q-5-59`):
//! a value of the wrong type is ignored with a warning. Unknown keys are
//! ignored silently; the option maps are an open schema.
//!
//! **Scope.** Website and book projects, HTML-family formats (decks
//! included, as in Quarto 1's website format extras). Images are found in
//! paragraphs, figures, divs, lists, block quotes and custom nodes
//! (callouts, tabsets), not in tables or raw HTML.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::block::Block;
use quarto_pandoc_types::config_value::{ConfigValueKind, blocks_to_plain_text};
use quarto_pandoc_types::custom::Slot;
use quarto_pandoc_types::inline::{Inline, Inlines};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_pandoc_types::{ConfigMapEntry, ConfigValue};
use quarto_source_map::{By, SourceInfo};
use quarto_system_runtime::SystemRuntime;

use crate::Result;
use crate::config_paths::{
    PathCtx, ResolvedConfigPath, resolve_config_path, resolve_config_path_str,
};
use crate::project::ProjectKind;
use crate::project::website_config::{website_description, website_site_url, website_title};
use crate::render::{RenderContext, ResourceCopyIntent};
use crate::transform::{AstTransform, TransformPhase};
use crate::transforms::inlines_to_plain_text;

/// A social image was found, but `website.site-url` is not set.
const CODE_IMAGE_NEEDS_SITE_URL: &str = "Q-5-58";
/// An `open-graph` / `twitter-card` value or option has the wrong type.
const CODE_INVALID_OPTION: &str = "Q-5-59";
/// A social image is a data URI.
const CODE_DATA_URI_IMAGE: &str = "Q-5-60";
/// `twitter-card` sets `image-width` / `image-height`.
const CODE_TWITTER_IMAGE_SIZE: &str = "Q-5-61";

/// Length Quarto 1 cuts a discovered description to.
const DESCRIPTION_MAX_CHARS: usize = 200;

/// AST transform: append `og:*` / `twitter:*` meta tags to the page head.
pub struct WebsiteSocialMetaTransform {
    runtime: Arc<dyn SystemRuntime>,
}

impl WebsiteSocialMetaTransform {
    pub fn new(runtime: Arc<dyn SystemRuntime>) -> Self {
        Self { runtime }
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for WebsiteSocialMetaTransform {
    fn name(&self) -> &str {
        "website-social-meta"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Normalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        let website = matches!(
            ctx.project.project_kind(),
            ProjectKind::Website | ProjectKind::Book
        );
        if !website || !ctx.format.is_html() {
            return Ok(());
        }

        let mut diagnostics = Vec::new();
        let options = SocialOptions::read(&ast.meta, &mut diagnostics);
        if options.open_graph.is_none() && options.twitter.is_none() {
            ctx.diagnostics.extend(diagnostics);
            return Ok(());
        }

        let path_ctx =
            PathCtx::for_document(ctx.source_context, &ctx.project.dir, &ctx.document.input);
        let page = PageValues::read(&ast.meta);
        let body = BodyValues::discover(&ast.blocks);
        let site_url = website_site_url(&ast.meta);

        let mut env = ResolveEnv {
            runtime: self.runtime.as_ref(),
            path_ctx: &path_ctx,
            project_dir: &ctx.project.dir,
            site_url: site_url.as_deref(),
            diagnostics: &mut diagnostics,
            warned_site_url: false,
            warned_data_uri: false,
            copies: Vec::new(),
        };

        let mut tags = Vec::new();
        if let Some(og) = &options.open_graph {
            tags.extend(open_graph_tags(og, &page, &body, &mut env));
        }
        if let Some(tw) = &options.twitter {
            tags.extend(twitter_tags(tw, &page, &body, &mut env));
        }
        let copies = std::mem::take(&mut env.copies);

        if let Some(resolver) = ctx.resource_resolver.as_ref() {
            let doc_dir = path_ctx.document_dir;
            for src in copies {
                let Some(rel) = pathdiff::diff_paths(&src, doc_dir) else {
                    continue;
                };
                let dest = quarto_util::path::normalize_lexically(&resolver.page_dir().join(rel));
                if ctx.resource_copies.iter().any(|c| c.dest == dest) {
                    continue;
                }
                ctx.resource_copies.push(ResourceCopyIntent {
                    src,
                    dest,
                    origin: SourceInfo::generated(By::programmatic_config()),
                });
            }
        }

        ctx.diagnostics.extend(diagnostics);
        if !tags.is_empty() {
            let html = tags
                .iter()
                .map(MetaTag::to_html)
                .collect::<Vec<_>>()
                .join("\n");
            super::website_favicon::append_to_rendered_header(&mut ast.meta, html);
        }
        Ok(())
    }
}

// ─── Options ───────────────────────────────────────────────────────────

/// The enabled providers' options: the page map's entry for a key wins
/// over the site map's.
struct SocialOptions<'a> {
    open_graph: Option<ProviderOptions<'a>>,
    twitter: Option<ProviderOptions<'a>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Provider {
    OpenGraph,
    Twitter,
}

impl Provider {
    fn key(self) -> &'static str {
        match self {
            Provider::OpenGraph => "open-graph",
            Provider::Twitter => "twitter-card",
        }
    }

    /// The options this provider reads, with their expected type.
    fn known_options(self) -> &'static [(&'static str, OptionType)] {
        use OptionType::{Int, Text};
        match self {
            Provider::OpenGraph => &[
                ("title", Text),
                ("description", Text),
                ("image", Text),
                ("image-alt", Text),
                ("image-width", Int),
                ("image-height", Int),
                ("locale", Text),
                ("site-name", Text),
            ],
            Provider::Twitter => &[
                ("title", Text),
                ("description", Text),
                ("image", Text),
                ("image-alt", Text),
                ("creator", Text),
                ("site", Text),
                ("card-style", Text),
            ],
        }
    }
}

#[derive(Clone, Copy)]
enum OptionType {
    Text,
    Int,
}

/// One provider's options. Each entry has passed its type check.
#[derive(Default)]
struct ProviderOptions<'a> {
    /// Options from the page's own map.
    page: Vec<&'a ConfigMapEntry>,
    /// Options from `website.<key>`.
    site: Vec<&'a ConfigMapEntry>,
}

impl<'a> ProviderOptions<'a> {
    fn page_value(&self, key: &str) -> Option<&'a ConfigValue> {
        self.page.iter().find(|e| e.key == key).map(|e| &e.value)
    }

    fn site_value(&self, key: &str) -> Option<&'a ConfigValue> {
        self.site.iter().find(|e| e.key == key).map(|e| &e.value)
    }

    fn value(&self, key: &str) -> Option<&'a ConfigValue> {
        self.page_value(key).or_else(|| self.site_value(key))
    }

    fn text(&self, key: &str) -> Option<String> {
        self.value(key).and_then(text_of)
    }

    fn int(&self, key: &str) -> Option<i64> {
        self.value(key).and_then(ConfigValue::as_int_lenient)
    }
}

/// How a page or site sets a provider.
enum Setting<'a> {
    Absent,
    Off,
    On(Option<&'a [ConfigMapEntry]>),
}

impl<'a> SocialOptions<'a> {
    fn read(meta: &'a ConfigValue, diagnostics: &mut Vec<DiagnosticMessage>) -> Self {
        Self {
            open_graph: read_provider(meta, Provider::OpenGraph, diagnostics),
            twitter: read_provider(meta, Provider::Twitter, diagnostics),
        }
    }
}

fn read_provider<'a>(
    meta: &'a ConfigValue,
    provider: Provider,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Option<ProviderOptions<'a>> {
    let key = provider.key();
    let page = setting(meta.get(key), key, diagnostics);
    let site_label = format!("website.{key}");
    let site = setting(meta.get_path(&["website", key]), &site_label, diagnostics);

    let enabled = match (&page, &site) {
        (Setting::Off, _) => false,
        (Setting::On(_), _) => true,
        (Setting::Absent, Setting::On(_)) => true,
        (Setting::Absent, _) => false,
    };
    if !enabled {
        return None;
    }

    let mut options = ProviderOptions::default();
    if let Setting::On(Some(entries)) = page {
        options.page = checked_entries(entries, provider, key, diagnostics);
    }
    if let Setting::On(Some(entries)) = site {
        options.site = checked_entries(entries, provider, &site_label, diagnostics);
    }
    Some(options)
}

fn setting<'a>(
    value: Option<&'a ConfigValue>,
    label: &str,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Setting<'a> {
    let Some(value) = value else {
        return Setting::Absent;
    };
    if value.is_null() {
        return Setting::Absent;
    }
    if let Some(on) = value.as_bool() {
        return if on { Setting::On(None) } else { Setting::Off };
    }
    if let ConfigValueKind::Map(entries) = &value.value {
        return Setting::On(Some(entries));
    }
    diagnostics.push(invalid_option(
        value,
        format!("`{label}` must be `true`, `false`, or a map of options; it is ignored."),
    ));
    Setting::Absent
}

/// The known options in `entries` whose value has the expected type.
/// Unknown keys are dropped silently; ill-typed known ones with `Q-5-59`.
fn checked_entries<'a>(
    entries: &'a [ConfigMapEntry],
    provider: Provider,
    label: &str,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Vec<&'a ConfigMapEntry> {
    let mut out = Vec::new();
    for entry in entries {
        if provider == Provider::Twitter
            && (entry.key == "image-width" || entry.key == "image-height")
        {
            diagnostics.push(
                DiagnosticMessageBuilder::warning("`twitter-card` image size is ignored")
                    .with_code(CODE_TWITTER_IMAGE_SIZE)
                    .with_location(entry.key_source.clone())
                    .problem(format!(
                        "Twitter Cards have no image size tags, so `{label}.{}` has no \
                         effect. Set it under `open-graph` instead.",
                        entry.key
                    ))
                    .build(),
            );
            continue;
        }
        let Some((_, ty)) = provider
            .known_options()
            .iter()
            .find(|(name, _)| *name == entry.key)
        else {
            continue;
        };
        if entry.value.is_null() {
            continue;
        }
        let ok = match ty {
            OptionType::Text => text_of(&entry.value).is_some(),
            OptionType::Int => entry.value.as_int_lenient().is_some(),
        };
        if ok {
            out.push(entry);
        } else {
            let expected = match ty {
                OptionType::Text => "a string",
                OptionType::Int => "a whole number",
            };
            diagnostics.push(invalid_option(
                &entry.value,
                format!("`{label}.{}` must be {expected}; it is ignored.", entry.key),
            ));
        }
    }
    out
}

fn invalid_option(value: &ConfigValue, problem: String) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning("Invalid social metadata option")
        .with_code(CODE_INVALID_OPTION)
        .with_location(value.source_info.clone())
        .problem(problem)
        .build()
}

// ─── Page and body values ──────────────────────────────────────────────

/// What the page's metadata says, independent of the provider.
struct PageValues<'a> {
    title: Option<String>,
    description: Option<String>,
    site_description: Option<String>,
    site_name: Option<String>,
    /// The page's `image:` (a string; `false` is [`Self::image_disabled`]).
    image: Option<&'a ConfigValue>,
    image_disabled: bool,
    image_alt: Option<String>,
    website_image: Option<&'a ConfigValue>,
    website_image_alt: Option<String>,
}

impl<'a> PageValues<'a> {
    fn read(meta: &'a ConfigValue) -> Self {
        let image_value = meta.get("image");
        let image_disabled = image_value.and_then(ConfigValue::as_bool) == Some(false);
        let image = image_value.filter(|v| text_of(v).is_some());
        let title = meta
            .get("pagetitle")
            .and_then(text_of)
            .or_else(|| meta.get("title").and_then(text_of));
        let description = ["description", "abstract", "subtitle"]
            .iter()
            .find_map(|key| meta.get(key).and_then(text_of));
        Self {
            title,
            description,
            site_description: website_description(meta).and_then(non_empty),
            site_name: website_title(meta).and_then(non_empty),
            image,
            image_disabled,
            image_alt: meta.get("image-alt").and_then(text_of),
            website_image: meta
                .get_path(&["website", "image"])
                .filter(|v| text_of(v).is_some()),
            website_image_alt: meta.get_path(&["website", "image-alt"]).and_then(text_of),
        }
    }
}

/// What the body offers when the metadata is silent.
struct BodyValues {
    description: Option<String>,
    image: Option<BodyImage>,
}

#[derive(Debug, Clone, PartialEq)]
struct BodyImage {
    src: String,
    alt: Option<String>,
}

impl BodyValues {
    fn discover(blocks: &[Block]) -> Self {
        Self {
            description: first_paragraph_text(blocks)
                .map(|text| truncate_at_punctuation(&text, DESCRIPTION_MAX_CHARS)),
            image: find_preview_image(blocks),
        }
    }
}

// ─── Tags ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
struct MetaTag {
    name: String,
    content: String,
}

impl MetaTag {
    fn new(name: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            content: content.into(),
        }
    }

    /// Quarto 1's `writeMetaTag`: `property=` for Open Graph, `name=`
    /// otherwise.
    fn to_html(&self) -> String {
        let attr = if self.name.starts_with("og:") {
            "property"
        } else {
            "name"
        };
        format!(
            r#"<meta {attr}="{}" content="{}">"#,
            escape_attr(&self.name),
            escape_attr(&self.content)
        )
    }
}

/// Where the chosen image came from; decides which declared size applies.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ImageOrigin {
    /// The page: its option map, its `image:`, or its body.
    Page,
    /// The site: `website.<provider>.image` or `website.image`.
    Site,
}

/// An image picked for a provider, before URL resolution.
struct ChosenImage<'a> {
    source: ImageSource<'a>,
    alt: Option<String>,
    origin: ImageOrigin,
}

enum ImageSource<'a> {
    /// A metadata value, resolved through its provenance.
    Meta(&'a ConfigValue),
    /// A body image's `src`, relative to the document.
    Body(String),
}

/// An image ready to emit.
struct ResolvedImage {
    url: String,
    size: Option<(u32, u32)>,
}

fn open_graph_tags(
    options: &ProviderOptions<'_>,
    page: &PageValues<'_>,
    body: &BodyValues,
    env: &mut ResolveEnv<'_>,
) -> Vec<MetaTag> {
    let Some(title) = options.text("title").or_else(|| page.title.clone()) else {
        return Vec::new();
    };
    let mut tags = vec![MetaTag::new("og:title", title)];
    if let Some(description) = description(options, page, body) {
        tags.push(MetaTag::new("og:description", description));
    }
    let chosen = choose_image(options, page, body, env);
    let resolved = chosen.as_ref().and_then(|c| env.resolve(c));
    if let Some(image) = &resolved {
        tags.push(MetaTag::new("og:image", image.url.clone()));
    }
    if let Some(site_name) = options.text("site-name").or_else(|| page.site_name.clone()) {
        tags.push(MetaTag::new("og:site_name", site_name));
    }
    if let (Some(image), Some(chosen)) = (&resolved, &chosen) {
        let declared = |key: &str| match chosen.origin {
            ImageOrigin::Page => options
                .page_value(key)
                .and_then(ConfigValue::as_int_lenient),
            ImageOrigin::Site => options.int(key),
        };
        let (width, height) = match image.size {
            Some((w, h)) => (Some(i64::from(w)), Some(i64::from(h))),
            None => (declared("image-width"), declared("image-height")),
        };
        if let Some(h) = height {
            tags.push(MetaTag::new("og:image:height", h.to_string()));
        }
        if let Some(w) = width {
            tags.push(MetaTag::new("og:image:width", w.to_string()));
        }
    }
    if let Some(locale) = options.text("locale") {
        tags.push(MetaTag::new("og:locale", locale));
    }
    if let (Some(_), Some(alt)) = (&resolved, chosen.and_then(|c| c.alt)) {
        tags.push(MetaTag::new("og:image:alt", alt));
    }
    tags
}

fn twitter_tags(
    options: &ProviderOptions<'_>,
    page: &PageValues<'_>,
    body: &BodyValues,
    env: &mut ResolveEnv<'_>,
) -> Vec<MetaTag> {
    let Some(title) = options.text("title").or_else(|| page.title.clone()) else {
        return Vec::new();
    };
    let mut tags = vec![MetaTag::new("twitter:title", title)];
    if let Some(description) = description(options, page, body) {
        tags.push(MetaTag::new("twitter:description", description));
    }
    let chosen = choose_image(options, page, body, env);
    let resolved = chosen.as_ref().and_then(|c| env.resolve(c));
    if let Some(image) = &resolved {
        tags.push(MetaTag::new("twitter:image", image.url.clone()));
    }
    if let Some(creator) = options.text("creator") {
        tags.push(MetaTag::new("twitter:creator", creator));
    }
    if let Some(site) = options.text("site") {
        tags.push(MetaTag::new("twitter:site", site));
    }
    if let (Some(_), Some(alt)) = (&resolved, chosen.and_then(|c| c.alt)) {
        tags.push(MetaTag::new("twitter:image:alt", alt));
    }
    let card = options.text("card-style").unwrap_or_else(|| {
        if resolved.is_some() {
            "summary_large_image".to_string()
        } else {
            "summary".to_string()
        }
    });
    tags.push(MetaTag::new("twitter:card", card));
    tags
}

fn description(
    options: &ProviderOptions<'_>,
    page: &PageValues<'_>,
    body: &BodyValues,
) -> Option<String> {
    options
        .text("description")
        .or_else(|| page.description.clone())
        .or_else(|| page.site_description.clone())
        .or_else(|| body.description.clone())
}

/// Pick the provider's image (see the module docs for the order). A data
/// URI is reported and skipped, falling through to the next candidate.
fn choose_image<'a>(
    options: &ProviderOptions<'a>,
    page: &PageValues<'a>,
    body: &BodyValues,
    env: &mut ResolveEnv<'_>,
) -> Option<ChosenImage<'a>> {
    let explicit_alt = || options.text("image-alt").or_else(|| page.image_alt.clone());
    let explicit = [
        (options.page_value("image"), ImageOrigin::Page),
        (page.image, ImageOrigin::Page),
        (options.site_value("image"), ImageOrigin::Site),
    ];
    for (value, origin) in explicit {
        let Some(value) = value else { continue };
        if env.rejects_data_uri(value) {
            continue;
        }
        return Some(ChosenImage {
            source: ImageSource::Meta(value),
            alt: explicit_alt(),
            origin,
        });
    }
    if page.image_disabled {
        return None;
    }
    // A provider's `image-alt` describes the provider's own image, so a
    // discovered or site-wide image doesn't take it; as in Quarto 1, only
    // the page's `image-alt` backs up the image's own alt text.
    if let Some(found) = &body.image {
        return Some(ChosenImage {
            source: ImageSource::Body(found.src.clone()),
            alt: found.alt.clone().or_else(|| page.image_alt.clone()),
            origin: ImageOrigin::Page,
        });
    }
    let website_image = page.website_image?;
    if env.rejects_data_uri(website_image) {
        return None;
    }
    Some(ChosenImage {
        source: ImageSource::Meta(website_image),
        alt: page
            .website_image_alt
            .clone()
            .or_else(|| page.image_alt.clone()),
        origin: ImageOrigin::Site,
    })
}

// ─── Resolution ────────────────────────────────────────────────────────

/// Per-page state for turning chosen images into URLs and sizes.
struct ResolveEnv<'a> {
    runtime: &'a dyn SystemRuntime,
    path_ctx: &'a PathCtx<'a>,
    project_dir: &'a Path,
    site_url: Option<&'a str>,
    diagnostics: &'a mut Vec<DiagnosticMessage>,
    warned_site_url: bool,
    warned_data_uri: bool,
    /// Image files named in metadata, to copy into the output tree.
    copies: Vec<PathBuf>,
}

impl ResolveEnv<'_> {
    /// `true` (after warning once per page) when `value` is a data URI.
    fn rejects_data_uri(&mut self, value: &ConfigValue) -> bool {
        let Some(text) = text_of(value) else {
            return false;
        };
        if !is_data_uri(&text) {
            return false;
        }
        if !self.warned_data_uri {
            self.warned_data_uri = true;
            self.diagnostics.push(
                DiagnosticMessageBuilder::warning("Data URI social image ignored")
                    .with_code(CODE_DATA_URI_IMAGE)
                    .with_location(value.source_info.clone())
                    .problem(
                        "Social media sites fetch the preview image by URL, so a data URI \
                         can't be used. Use an image file instead.",
                    )
                    .build(),
            );
        }
        true
    }

    fn resolve(&mut self, chosen: &ChosenImage<'_>) -> Option<ResolvedImage> {
        let (target, location, from_meta) = match &chosen.source {
            ImageSource::Meta(value) => (
                resolve_config_path(value, self.path_ctx)?,
                Some(value.source_info.clone()),
                true,
            ),
            ImageSource::Body(src) => (
                // A body image is relative to the document; a generated
                // source info carries no provenance, so the resolver uses
                // the document directory (and the project root for `/`).
                resolve_config_path_str(
                    src,
                    &SourceInfo::generated(By::programmatic_config()),
                    self.path_ctx,
                ),
                None,
                false,
            ),
        };

        let Some(site_url) = self.site_url else {
            self.warn_needs_site_url(&chosen.source, location);
            return None;
        };

        match target {
            ResolvedConfigPath::External(url) => Some(ResolvedImage { url, size: None }),
            ResolvedConfigPath::File(path) => {
                let rel = path.strip_prefix(self.project_dir).ok()?;
                let url = join_url(site_url, &quarto_util::to_forward_slashes(rel));
                let size = self
                    .runtime
                    .file_read(&path)
                    .ok()
                    .and_then(|bytes| image_size(&bytes));
                if from_meta
                    && !self.copies.contains(&path)
                    && self.runtime.is_file(&path).unwrap_or(false)
                {
                    self.copies.push(path);
                }
                Some(ResolvedImage { url, size })
            }
        }
    }

    fn warn_needs_site_url(&mut self, source: &ImageSource<'_>, location: Option<SourceInfo>) {
        if self.warned_site_url {
            return;
        }
        self.warned_site_url = true;
        let src = match source {
            ImageSource::Meta(value) => text_of(value).unwrap_or_default(),
            ImageSource::Body(src) => src.clone(),
        };
        let mut builder = DiagnosticMessageBuilder::warning("Social image needs `site-url`")
            .with_code(CODE_IMAGE_NEEDS_SITE_URL)
            .problem(format!(
                "The page's social media preview image `{src}` is left out: \
                 `og:image` and `twitter:image` need an absolute URL, and \
                 `website.site-url` is not set."
            ));
        if let Some(location) = location {
            builder = builder.with_location(location);
        }
        self.diagnostics.push(builder.build());
    }
}

/// Pixel size from an image file's header. Native only: the WASM preview
/// doesn't carry the `imagesize` dependency, and the tags it writes into
/// the preview are never read by a crawler.
#[cfg(not(target_arch = "wasm32"))]
fn image_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let size = imagesize::blob_size(bytes).ok()?;
    Some((
        u32::try_from(size.width).ok()?,
        u32::try_from(size.height).ok()?,
    ))
}

#[cfg(target_arch = "wasm32")]
fn image_size(_bytes: &[u8]) -> Option<(u32, u32)> {
    None
}

/// Quarto 1's `joinUrl`: exactly one `/` between base and path.
fn join_url(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

// ─── Discovery ─────────────────────────────────────────────────────────

/// Plain text of the first top-level paragraph that has any.
fn first_paragraph_text(blocks: &[Block]) -> Option<String> {
    blocks.iter().find_map(|block| match block {
        Block::Paragraph(p) => non_empty(inlines_to_plain_text(&p.content)),
        _ => None,
    })
}

/// An image found in the body, with the context discovery ranks it by.
#[derive(Debug)]
struct Candidate {
    src: String,
    alt: Option<String>,
    has_preview_class: bool,
    in_preview_cell_output: bool,
}

/// Quarto 1's `findPreviewImgEl`, over the AST.
fn find_preview_image(blocks: &[Block]) -> Option<BodyImage> {
    let mut found = Vec::new();
    collect_blocks(blocks, Context::default(), &mut found);
    let usable: Vec<&Candidate> = found.iter().filter(|c| !is_data_uri(&c.src)).collect();
    let pick = usable
        .iter()
        .find(|c| c.has_preview_class)
        .or_else(|| usable.iter().find(|c| c.in_preview_cell_output))
        .or_else(|| usable.iter().find(|c| is_named_preview_file(&c.src)))
        .or_else(|| {
            usable
                .iter()
                .find(|c| !c.src.starts_with("http:") && !c.src.starts_with("https:"))
        })?;
    Some(BodyImage {
        src: pick.src.clone(),
        alt: pick.alt.clone(),
    })
}

/// Quarto 1's `kNamedFilePattern`: the file name mentions `preview`,
/// `feature`, `cover` or `thumbnail` and ends in an image extension.
fn is_named_preview_file(src: &str) -> bool {
    static PATTERN: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?:preview|feature|cover|thumbnail).*?\.(?:png|gif|jpg|jpeg|webp|svg)")
            .expect("named preview pattern compiles")
    });
    PATTERN.is_match(src)
}

#[derive(Clone, Copy, Default)]
struct Context {
    in_preview_div: bool,
    in_cell_output: bool,
    in_figure: bool,
}

fn has_class(classes: &[String], class: &str) -> bool {
    classes.iter().any(|c| c == class)
}

fn collect_blocks(blocks: &[Block], ctx: Context, out: &mut Vec<Candidate>) {
    for block in blocks {
        collect_block(block, ctx, out);
    }
}

fn collect_block(block: &Block, ctx: Context, out: &mut Vec<Candidate>) {
    match block {
        Block::Plain(p) => collect_inlines(&p.content, ctx, out),
        Block::Paragraph(p) => collect_inlines(&p.content, ctx, out),
        Block::Header(h) => collect_inlines(&h.content, ctx, out),
        Block::LineBlock(lb) => {
            for line in &lb.content {
                collect_inlines(line, ctx, out);
            }
        }
        Block::BlockQuote(q) => collect_blocks(&q.content, ctx, out),
        Block::OrderedList(l) => {
            for item in &l.content {
                collect_blocks(item, ctx, out);
            }
        }
        Block::BulletList(l) => {
            for item in &l.content {
                collect_blocks(item, ctx, out);
            }
        }
        Block::DefinitionList(dl) => {
            for (term, defs) in &dl.content {
                collect_inlines(term, ctx, out);
                for def in defs {
                    collect_blocks(def, ctx, out);
                }
            }
        }
        Block::Figure(f) => collect_blocks(
            &f.content,
            Context {
                in_figure: true,
                ..ctx
            },
            out,
        ),
        Block::Div(d) => {
            let classes = &d.attr.1;
            let inner = Context {
                in_preview_div: ctx.in_preview_div || has_class(classes, "preview-image"),
                in_cell_output: ctx.in_cell_output
                    || (ctx.in_preview_div && has_class(classes, "cell-output-display")),
                ..ctx
            };
            collect_blocks(&d.content, inner, out);
        }
        Block::Custom(node) => collect_slots(node.slots.values(), ctx, out),
        _ => {}
    }
}

fn collect_slots<'s>(
    slots: impl Iterator<Item = &'s Slot>,
    ctx: Context,
    out: &mut Vec<Candidate>,
) {
    for slot in slots {
        match slot {
            Slot::Block(b) => collect_block(b, ctx, out),
            Slot::Blocks(bs) => collect_blocks(bs, ctx, out),
            Slot::Inline(i) => collect_inline(i, ctx, out),
            Slot::Inlines(is) => collect_inlines(is, ctx, out),
        }
    }
}

fn collect_inlines(inlines: &Inlines, ctx: Context, out: &mut Vec<Candidate>) {
    for inline in inlines {
        collect_inline(inline, ctx, out);
    }
}

fn collect_inline(inline: &Inline, ctx: Context, out: &mut Vec<Candidate>) {
    match inline {
        Inline::Image(img) => {
            let src = img.target.0.clone();
            if src.is_empty() {
                return;
            }
            // A figure's caption is not alt text (Quarto 1 reads the
            // rendered `alt` attribute, which a figure image only gets
            // from `fig-alt`).
            let alt = if ctx.in_figure {
                img.attr.2.get("fig-alt").cloned().and_then(non_empty)
            } else {
                non_empty(inlines_to_plain_text(&img.content))
            };
            out.push(Candidate {
                src,
                alt,
                has_preview_class: has_class(&img.attr.1, "preview-image"),
                in_preview_cell_output: ctx.in_cell_output,
            });
        }
        Inline::Emph(e) => collect_inlines(&e.content, ctx, out),
        Inline::Underline(u) => collect_inlines(&u.content, ctx, out),
        Inline::Strong(s) => collect_inlines(&s.content, ctx, out),
        Inline::Strikeout(s) => collect_inlines(&s.content, ctx, out),
        Inline::Superscript(s) => collect_inlines(&s.content, ctx, out),
        Inline::Subscript(s) => collect_inlines(&s.content, ctx, out),
        Inline::SmallCaps(s) => collect_inlines(&s.content, ctx, out),
        Inline::Quoted(q) => collect_inlines(&q.content, ctx, out),
        Inline::Link(l) => collect_inlines(&l.content, ctx, out),
        Inline::Span(s) => collect_inlines(&s.content, ctx, out),
        Inline::Insert(i) => collect_inlines(&i.content, ctx, out),
        Inline::Highlight(h) => collect_inlines(&h.content, ctx, out),
        Inline::Custom(node) => collect_slots(node.slots.values(), ctx, out),
        _ => {}
    }
}

// ─── Text helpers ──────────────────────────────────────────────────────

/// Plain text of a metadata value that holds text: a string, markdown
/// inlines, or markdown blocks (a `description: |` block scalar).
fn text_of(value: &ConfigValue) -> Option<String> {
    let text = match &value.value {
        ConfigValueKind::PandocBlocks(blocks) => blocks_to_plain_text(blocks),
        _ => value.as_plain_text()?,
    };
    non_empty(text.trim().to_string())
}

fn non_empty(text: String) -> Option<String> {
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

fn is_data_uri(src: &str) -> bool {
    src.trim_start()
        .get(..5)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("data:"))
}

fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Quarto 1's `truncateText(text, max, "punctuation")`: text shorter than
/// `max` characters is kept; otherwise it is cut to `max` characters and
/// then back to its last `.`, `?` or `!`, or, with none, to its last space
/// with an ellipsis.
fn truncate_at_punctuation(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() < max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max).collect();
    if let Some(end) = cut.rfind(['.', '?', '!']) {
        return cut[..=end].to_string();
    }
    // `trimAtSpace`: drop the last character, then back up to a space.
    let mut shorter: String = cut.chars().take(max.saturating_sub(1)).collect();
    if let Some(space) = shorter.rfind(' ')
        && space > 0
    {
        shorter.truncate(space);
    }
    if shorter.ends_with([',', '/', ':']) {
        shorter.pop();
    }
    format!("{shorter}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::attr::Attr;
    use quarto_pandoc_types::block::{Div, Figure, Paragraph, Plain};
    use quarto_pandoc_types::caption::Caption;
    use quarto_pandoc_types::inline::{Image, Str};

    fn si() -> SourceInfo {
        SourceInfo::for_test()
    }

    fn s(text: &str) -> ConfigValue {
        ConfigValue::new_string(text.to_string(), si())
    }

    fn b(value: bool) -> ConfigValue {
        ConfigValue::new_bool(value, si())
    }

    fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        ConfigValue::new_map(
            entries
                .into_iter()
                .map(|(k, v)| ConfigMapEntry {
                    key: k.to_string(),
                    key_source: si(),
                    value: v,
                })
                .collect(),
            si(),
        )
    }

    fn attr(classes: &[&str], kvs: &[(&str, &str)]) -> Attr {
        let mut map = hashlink::LinkedHashMap::new();
        for (k, v) in kvs {
            map.insert(k.to_string(), v.to_string());
        }
        Attr::from((
            String::new(),
            classes.iter().map(|c| c.to_string()).collect(),
            map,
        ))
    }

    fn image(src: &str, alt: &str, classes: &[&str]) -> Inline {
        Inline::Image(Box::new(Image {
            attr: attr(classes, &[]),
            content: vec![Inline::Str(Str {
                text: alt.to_string(),
                source_info: si(),
            })],
            target: (src.to_string(), String::new()),
            source_info: si(),
            attr_source: Default::default(),
            target_source: quarto_pandoc_types::TargetSourceInfo::empty(),
        }))
    }

    fn para(inlines: Vec<Inline>) -> Block {
        Block::Paragraph(Paragraph {
            content: inlines,
            source_info: si(),
        })
    }

    fn text_para(text: &str) -> Block {
        para(vec![Inline::Str(Str {
            text: text.to_string(),
            source_info: si(),
        })])
    }

    fn div(classes: &[&str], content: Vec<Block>) -> Block {
        Block::Div(Box::new(Div {
            attr: attr(classes, &[]),
            content,
            source_info: si(),
            attr_source: Default::default(),
        }))
    }

    fn figure(img: Inline) -> Block {
        Block::Figure(Box::new(Figure {
            attr: attr(&[], &[]),
            caption: Caption {
                short: None,
                long: None,
                source_info: si(),
            },
            content: vec![Block::Plain(Plain {
                content: vec![img],
                source_info: si(),
            })],
            source_info: si(),
            attr_source: Default::default(),
        }))
    }

    fn found(blocks: Vec<Block>) -> Option<BodyImage> {
        find_preview_image(&blocks)
    }

    fn src_of(found: Option<BodyImage>) -> Option<String> {
        found.map(|f| f.src)
    }

    // ── Enabling and option types ──────────────────────────────────────

    fn enabled(meta: &ConfigValue) -> (bool, bool, usize) {
        let mut diags = Vec::new();
        let opts = SocialOptions::read(meta, &mut diags);
        (
            opts.open_graph.is_some(),
            opts.twitter.is_some(),
            diags.len(),
        )
    }

    #[test]
    fn providers_are_off_by_default() {
        assert_eq!(enabled(&map(vec![])), (false, false, 0));
    }

    #[test]
    fn site_setting_enables_and_page_false_disables() {
        let site = map(vec![(
            "website",
            map(vec![("open-graph", b(true)), ("twitter-card", map(vec![]))]),
        )]);
        assert_eq!(enabled(&site), (true, true, 0));

        let mut page_off = site.clone();
        page_off.insert_path(&["twitter-card"], b(false));
        assert_eq!(enabled(&page_off), (true, false, 0));
    }

    #[test]
    fn page_setting_enables_without_site() {
        let meta = map(vec![("open-graph", map(vec![("locale", s("en_US"))]))]);
        assert_eq!(enabled(&meta), (true, false, 0));
    }

    #[test]
    fn wrong_setting_type_warns_and_falls_back_to_the_site() {
        let meta = map(vec![
            ("open-graph", s("yes")),
            ("website", map(vec![("open-graph", b(true))])),
        ]);
        assert_eq!(enabled(&meta), (true, false, 1));
    }

    #[test]
    fn page_options_win_over_site_options() {
        let meta = map(vec![
            ("open-graph", map(vec![("locale", s("fr_FR"))])),
            (
                "website",
                map(vec![(
                    "open-graph",
                    map(vec![("locale", s("en_US")), ("site-name", s("S"))]),
                )]),
            ),
        ]);
        let mut diags = Vec::new();
        let og = read_provider(&meta, Provider::OpenGraph, &mut diags).unwrap();
        assert_eq!(og.text("locale").as_deref(), Some("fr_FR"));
        assert_eq!(og.text("site-name").as_deref(), Some("S"));
    }

    #[test]
    fn ill_typed_options_are_dropped_and_unknown_keys_ignored() {
        let meta = map(vec![(
            "open-graph",
            map(vec![
                ("title", ConfigValue::new_array(vec![s("a")], si())),
                ("image-width", s("wide")),
                ("image-height", s("40")),
                ("whatever", ConfigValue::new_array(vec![], si())),
            ]),
        )]);
        let mut diags = Vec::new();
        let og = read_provider(&meta, Provider::OpenGraph, &mut diags).unwrap();
        assert_eq!(og.text("title"), None);
        assert_eq!(og.int("image-width"), None);
        assert_eq!(og.int("image-height"), Some(40));
        let codes: Vec<_> = diags.iter().filter_map(|d| d.code.as_deref()).collect();
        assert_eq!(codes, vec![CODE_INVALID_OPTION, CODE_INVALID_OPTION]);
    }

    #[test]
    fn twitter_image_size_is_reported_and_ignored() {
        let meta = map(vec![("twitter-card", map(vec![("image-height", s("4"))]))]);
        let mut diags = Vec::new();
        let tw = read_provider(&meta, Provider::Twitter, &mut diags).unwrap();
        assert!(tw.value("image-height").is_none());
        assert_eq!(diags[0].code.as_deref(), Some(CODE_TWITTER_IMAGE_SIZE));
    }

    // ── Discovery ──────────────────────────────────────────────────────

    #[test]
    fn preview_class_wins() {
        let blocks = vec![
            para(vec![image("cover.png", "", &[])]),
            para(vec![image("marked.png", "", &["preview-image"])]),
        ];
        assert_eq!(src_of(found(blocks)).as_deref(), Some("marked.png"));
    }

    #[test]
    fn preview_cell_output_beats_named_files() {
        let blocks = vec![
            para(vec![image("thumbnail.png", "", &[])]),
            div(
                &["cell", "preview-image"],
                vec![div(
                    &["cell-output-display"],
                    vec![para(vec![image("plot.png", "", &[])])],
                )],
            ),
        ];
        assert_eq!(src_of(found(blocks)).as_deref(), Some("plot.png"));
    }

    #[test]
    fn image_in_preview_div_but_not_in_cell_output_is_not_marked() {
        let blocks = vec![
            div(
                &["preview-image"],
                vec![para(vec![image("a.png", "", &[])])],
            ),
            para(vec![image("b-cover.png", "", &[])]),
        ];
        assert_eq!(src_of(found(blocks)).as_deref(), Some("b-cover.png"));
    }

    #[test]
    fn named_file_beats_first_image_and_is_case_sensitive() {
        let blocks = vec![
            para(vec![image("first.png", "", &[])]),
            para(vec![image("COVER.png", "", &[])]),
            para(vec![image("img/feature-1.jpg", "", &[])]),
        ];
        assert_eq!(src_of(found(blocks)).as_deref(), Some("img/feature-1.jpg"));
    }

    #[test]
    fn first_local_image_skips_remote_and_data_uris() {
        let blocks = vec![
            para(vec![image("https://example.org/a.png", "", &[])]),
            para(vec![image(
                "data:image/png;base64,AA",
                "",
                &["preview-image"],
            )]),
            para(vec![image("local.png", "", &[])]),
        ];
        assert_eq!(src_of(found(blocks)).as_deref(), Some("local.png"));
    }

    #[test]
    fn inline_images_carry_alt_and_figures_only_fig_alt() {
        let inline = found(vec![para(vec![image("a.png", "inline alt", &[])])]).unwrap();
        assert_eq!(inline.alt.as_deref(), Some("inline alt"));

        let fig = found(vec![figure(image("a.png", "caption", &[]))]).unwrap();
        assert_eq!(fig.alt, None);

        let mut with_fig_alt = image("a.png", "caption", &[]);
        if let Inline::Image(img) = &mut with_fig_alt {
            img.attr = attr(&[], &[("fig-alt", "Described")]);
        }
        let fig = found(vec![figure(with_fig_alt)]).unwrap();
        assert_eq!(fig.alt.as_deref(), Some("Described"));
    }

    #[test]
    fn description_is_the_first_non_empty_top_level_paragraph() {
        let blocks = vec![
            div(&["callout"], vec![text_para("inside a div")]),
            text_para("   "),
            text_para("The one."),
            text_para("Not this."),
        ];
        assert_eq!(first_paragraph_text(&blocks).as_deref(), Some("The one."));
    }

    // ── Text and URLs ──────────────────────────────────────────────────

    #[test]
    fn truncation_matches_quarto_1() {
        assert_eq!(truncate_at_punctuation("Short.", 200), "Short.");
        let long = format!("First sentence. {}", "word ".repeat(60));
        assert_eq!(truncate_at_punctuation(&long, 200), "First sentence.");
        let no_stop = "word ".repeat(60);
        let cut = truncate_at_punctuation(&no_stop, 50);
        assert!(cut.ends_with("word…"), "{cut}");
        assert!(cut.chars().count() <= 50, "{cut}");
        assert_eq!(truncate_at_punctuation("a, b, c, d", 5), "a…");
    }

    #[test]
    fn truncation_counts_characters_not_bytes() {
        let text = "é".repeat(300);
        let cut = truncate_at_punctuation(&text, 200);
        assert_eq!(cut.chars().count(), 200);
        assert!(cut.ends_with('…'));
    }

    #[test]
    fn join_url_has_exactly_one_slash() {
        assert_eq!(join_url("https://e.com/", "/a.png"), "https://e.com/a.png");
        assert_eq!(join_url("https://e.com", "a.png"), "https://e.com/a.png");
        assert_eq!(
            join_url("https://e.com/base", "x/a.png"),
            "https://e.com/base/x/a.png"
        );
    }

    #[test]
    fn meta_tags_escape_and_pick_attribute_by_prefix() {
        assert_eq!(
            MetaTag::new("og:title", "A \"&\" <B>").to_html(),
            r#"<meta property="og:title" content="A &quot;&amp;&quot; &lt;B&gt;">"#
        );
        assert_eq!(
            MetaTag::new("twitter:card", "summary").to_html(),
            r#"<meta name="twitter:card" content="summary">"#
        );
    }

    #[test]
    fn data_uri_detection_ignores_case() {
        assert!(is_data_uri("DATA:image/png;base64,AA"));
        assert!(!is_data_uri("data.png"));
    }
}
