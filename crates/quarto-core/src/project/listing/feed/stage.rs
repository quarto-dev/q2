/*
 * project/listing/feed/stage.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Pass-2 [`ListingFeedStageTransform`] — emits one staged feed
//! file per feed-configured listing during the host page's render.
//!
//! "Staged" means the file carries a sentinel extension (e.g.
//! `.feed-full-staged`) and may contain placeholder envelopes that
//! the L9 post-render step substitutes against sibling rendered
//! HTML. See `claude-notes/plans/2026-05-08-listings-L9-rss-feeds.md`
//! §"Architecture" → "The staged-file pattern".
//!
//! Native-only — feeds are written by `quarto render`'s post-render
//! step; the WASM hub-client preview doesn't generate feeds. The
//! feature gates apply at the `feed/` module level (see
//! `feed/mod.rs`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use quarto_doctemplate::{MemoryResolver, Template, TemplateContext, TemplateValue};
use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceInfo;

use crate::Result;
use crate::project::listing::ResolvedListing;
use crate::project::listing::config::{FeedType, ListingFeedOptions};
use crate::project::listing::item::ListingItem;
use crate::project::website_config::website_site_url;
use crate::render::RenderContext;
use crate::transform::AstTransform;
use crate::transforms::is_feature_disabled;

use super::binding::{
    FeedChannel, FeedItem, build_feed_channel, build_feed_item, format_pub_date_rfc822,
};

// Embedded template sources — the three feed templates ship as
// `include_str!`-baked strings so authors can read them as the
// canonical reference (the files at `templates/*.template` are
// tracked in git).
const FEED_PREAMBLE_SRC: &str = include_str!("templates/preamble.template");
const FEED_ITEM_SRC: &str = include_str!("templates/item.template");
const FEED_POSTAMBLE_SRC: &str = include_str!("templates/postamble.template");

/// Default item count when `feed.items:` is unset or zero. Q1
/// uses the same default in
/// `external-sources/quarto-cli/src/project/types/website/listing/website-listing-feed.ts`.
const DEFAULT_FEED_ITEMS: usize = 20;

/// Pass-2 transform: stage one RSS feed per feed-configured listing
/// on the host page.
///
/// Reads `RenderContext::resolved_listings` (populated by
/// [`crate::transforms::listing_generate::ListingGenerateTransform`])
/// and writes `<output_dir>/<dir>/<stem>.feed-{full|partial|metadata}-staged`
/// for each listing whose `feed:` is set. Per-category sub-feeds
/// produce additional staged files named
/// `<stem>-<lowercased-category>.feed-{type}-staged`.
///
/// When a host page has multiple listings with `feed:` configured,
/// each listing's filename is qualified with the listing id
/// (`<stem>-<listing-id>.feed-{type}-staged`) to avoid collisions.
/// When only one listing has `feed:`, the bare `<stem>.feed-...`
/// form is used (matches Q1's single-feed-per-page behavior; see
/// plan D7).
///
/// **Site-url gating:** without `website.site-url`, feeds are
/// skipped and a single `Q-12-15` diagnostic is emitted (once per
/// transform invocation, regardless of how many listings declared
/// feeds).
pub struct ListingFeedStageTransform;

impl ListingFeedStageTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ListingFeedStageTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl AstTransform for ListingFeedStageTransform {
    fn name(&self) -> &str {
        "listing-feed-stage"
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        if is_feature_disabled(&ast.meta, "listing") {
            return Ok(());
        }
        if ctx.resolved_listings.is_empty() {
            return Ok(());
        }
        let any_feed = ctx
            .resolved_listings
            .iter()
            .any(|r| r.listing.feed.is_some());
        if !any_feed {
            return Ok(());
        }

        let Some(site_url) = website_site_url(&ast.meta) else {
            ctx.diagnostics.push(make_q_12_15());
            return Ok(());
        };

        let host_output_path = ctx.output_path();
        let Some(host_output_dir) = host_output_path.parent().map(Path::to_path_buf) else {
            return Ok(());
        };
        let host_stem = host_output_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("index")
            .to_string();

        // Project-relative URL of the host page (forward-slashed,
        // no leading slash).
        let host_output_href =
            href_relative_to_output_dir(&host_output_path, &ctx.project.output_dir);
        // Project-relative URL of the host page's directory.
        let host_dir_href = parent_href(&host_output_href);

        let project_dir = ctx.project.dir.clone();
        let project_meta = ast.meta.clone();

        let feed_listings: Vec<&ResolvedListing> = ctx
            .resolved_listings
            .iter()
            .filter(|r| r.listing.feed.is_some())
            .collect();
        let qualify = feed_listings.len() > 1;

        // Compile templates once per call. Compilation failure
        // should be impossible at runtime (templates are embedded
        // and exercised in unit tests), so we panic on error rather
        // than emitting a soft diagnostic — a malformed embedded
        // template is a programming bug.
        let templates = compile_templates();

        let mut diagnostics = std::mem::take(&mut ctx.diagnostics);

        for r in feed_listings {
            let feed_options = r.listing.feed.as_ref().expect("filtered by has-feed above");
            let stem = if qualify {
                format!("{}-{}", host_stem, r.listing.id)
            } else {
                host_stem.clone()
            };

            // Main feed for this listing.
            stage_one_feed(
                r,
                feed_options,
                &templates,
                &stem,
                &host_output_dir,
                &host_dir_href,
                &project_dir,
                &project_meta,
                &site_url,
                &host_output_href,
                &mut diagnostics,
            )?;

            // Per-category sub-feeds.
            for category in &feed_options.categories {
                let cat_stem = format!("{}-{}", stem, category.to_lowercase());
                let filtered_items: Vec<ListingItem> = r
                    .items
                    .iter()
                    .filter(|it| it.categories.iter().any(|c| c == category))
                    .cloned()
                    .collect();
                stage_one_subfeed(
                    feed_options,
                    &filtered_items,
                    &templates,
                    &cat_stem,
                    &host_output_dir,
                    &host_dir_href,
                    &project_dir,
                    &project_meta,
                    &site_url,
                    &host_output_href,
                    &mut diagnostics,
                )?;
            }
        }

        ctx.diagnostics = diagnostics;
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────
// Single-feed staging
// ─────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn stage_one_feed(
    r: &ResolvedListing,
    feed_options: &ListingFeedOptions,
    templates: &CompiledTemplates,
    stem: &str,
    host_output_dir: &Path,
    host_dir_href: &str,
    project_dir: &Path,
    project_meta: &ConfigValue,
    site_url: &str,
    host_output_href: &str,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Result<()> {
    let truncated = truncate_items(&r.items, feed_options);
    stage_feed_inner(
        feed_options,
        &truncated,
        templates,
        stem,
        host_output_dir,
        host_dir_href,
        project_dir,
        project_meta,
        site_url,
        host_output_href,
        diagnostics,
    )
}

#[allow(clippy::too_many_arguments)]
fn stage_one_subfeed(
    feed_options: &ListingFeedOptions,
    filtered_items: &[ListingItem],
    templates: &CompiledTemplates,
    cat_stem: &str,
    host_output_dir: &Path,
    host_dir_href: &str,
    project_dir: &Path,
    project_meta: &ConfigValue,
    site_url: &str,
    host_output_href: &str,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Result<()> {
    let truncated = truncate_items(filtered_items, feed_options);
    stage_feed_inner(
        feed_options,
        &truncated,
        templates,
        cat_stem,
        host_output_dir,
        host_dir_href,
        project_dir,
        project_meta,
        site_url,
        host_output_href,
        diagnostics,
    )
}

#[allow(clippy::too_many_arguments)]
fn stage_feed_inner(
    feed_options: &ListingFeedOptions,
    items: &[ListingItem],
    templates: &CompiledTemplates,
    stem: &str,
    host_output_dir: &Path,
    host_dir_href: &str,
    project_dir: &Path,
    project_meta: &ConfigValue,
    site_url: &str,
    host_output_href: &str,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Result<()> {
    let staged_filename = format!(
        "{}.feed-{}-staged",
        stem,
        feed_type_extension(feed_options.kind)
    );
    let staged_path = host_output_dir.join(&staged_filename);
    let final_xml_relpath = if host_dir_href.is_empty() {
        format!("{}.xml", stem)
    } else {
        format!("{}/{}.xml", host_dir_href, stem)
    };

    let last_build_date_iso = most_recent_item_date(items);

    let channel = build_feed_channel(
        feed_options,
        project_meta,
        host_output_href,
        &final_xml_relpath,
        last_build_date_iso.as_deref(),
        project_dir,
    );
    let preamble = render_template(&templates.preamble, &channel_template_context(&channel));

    // Author-controlled item rendering. `prepareItems` in Q1
    // skips items missing both `title` and `path`; v1 does the
    // analogous filter (title empty OR output_href empty are
    // skipped).
    let mut body = String::new();
    for it in items {
        if it.title.trim().is_empty() || it.output_href.is_empty() {
            continue;
        }
        let fi = build_feed_item(it, feed_options, site_url, project_dir);
        let rendered = render_template(&templates.item, &item_template_context(&fi));
        body.push_str(&rendered);
    }

    let postamble = render_template(&templates.postamble, &TemplateContext::new());

    let mut full = String::new();
    full.push_str(&preamble);
    full.push_str(&body);
    full.push_str(&postamble);

    if let Some(parent) = staged_path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        diagnostics.push(diagnostic_warning(format!(
            "Could not create feed output directory {}: {}",
            parent.display(),
            e
        )));
        return Ok(());
    }
    if let Err(e) = std::fs::write(&staged_path, full.as_bytes()) {
        diagnostics.push(diagnostic_warning(format!(
            "Could not write staged feed {}: {}",
            staged_path.display(),
            e
        )));
    }
    Ok(())
}

/// Apply the `feed.items:` truncation. Q1 treats `0` and missing
/// equivalently (both → default 20).
fn truncate_items(items: &[ListingItem], feed_options: &ListingFeedOptions) -> Vec<ListingItem> {
    let limit = match feed_options.items {
        Some(0) | None => DEFAULT_FEED_ITEMS,
        Some(n) => n as usize,
    };
    items.iter().take(limit).cloned().collect()
}

/// Pick the most-recent date among the items (for `lastBuildDate`).
/// Q1 sorts by date descending and takes the first; we do the
/// equivalent without re-sorting (callers may have applied a
/// different listing-level sort, which we should not override).
fn most_recent_item_date(items: &[ListingItem]) -> Option<String> {
    // Convert each item's date string to RFC 2822 (parsing) and
    // compare on the ISO-equivalent representation. Because the
    // RFC 2822 string is canonical UTC-zoned, lexical max happens
    // to match chronological max only when the year is fixed —
    // safer to compare via parsed OffsetDateTime.
    let mut best: Option<(time::OffsetDateTime, String)> = None;
    for it in items {
        let Some(date) = it.date.as_deref() else {
            continue;
        };
        // Reuse the binding's parser by going through its public
        // helper; we discard the formatted string and re-parse
        // RFC 2822 to get a comparable OffsetDateTime. (Could be
        // optimized; v1 prioritizes simplicity.)
        let Some(rfc) = format_pub_date_rfc822(date) else {
            continue;
        };
        let Ok(dt) =
            time::OffsetDateTime::parse(&rfc, &time::format_description::well_known::Rfc2822)
        else {
            continue;
        };
        match &best {
            Some((prev_dt, _)) if *prev_dt >= dt => {}
            _ => best = Some((dt, date.to_string())),
        }
    }
    best.map(|(_, s)| s)
}

// ─────────────────────────────────────────────────────────────────
// Template plumbing
// ─────────────────────────────────────────────────────────────────

struct CompiledTemplates {
    preamble: Template,
    item: Template,
    postamble: Template,
}

fn compile_templates() -> CompiledTemplates {
    let resolver = MemoryResolver::new();
    let path = Path::new("feed.template");
    let preamble = Template::compile_with_resolver(FEED_PREAMBLE_SRC, path, &resolver, 0)
        .expect("L9 preamble.template must compile (programming bug if it doesn't)");
    let item = Template::compile_with_resolver(FEED_ITEM_SRC, path, &resolver, 0)
        .expect("L9 item.template must compile (programming bug if it doesn't)");
    let postamble = Template::compile_with_resolver(FEED_POSTAMBLE_SRC, path, &resolver, 0)
        .expect("L9 postamble.template must compile (programming bug if it doesn't)");
    CompiledTemplates {
        preamble,
        item,
        postamble,
    }
}

fn render_template(template: &Template, ctx: &TemplateContext) -> String {
    let (rendered, _diags) = template.render_with_diagnostics(ctx);
    rendered.unwrap_or_default()
}

/// Lift a [`FeedChannel`] into a flat template context with a
/// single top-level key `channel` mapping to a record of channel
/// fields.
fn channel_template_context(channel: &FeedChannel) -> TemplateContext {
    let mut t = TemplateContext::new();
    t.insert("channel", channel_value(channel));
    t
}

fn channel_value(channel: &FeedChannel) -> TemplateValue {
    let mut m: HashMap<String, TemplateValue> = HashMap::new();
    m.insert(
        "title".to_string(),
        TemplateValue::String(channel.title.clone()),
    );
    m.insert(
        "link".to_string(),
        TemplateValue::String(channel.link.clone()),
    );
    m.insert(
        "feed-link".to_string(),
        TemplateValue::String(channel.feed_link.clone()),
    );
    m.insert(
        "description".to_string(),
        TemplateValue::String(channel.description.clone()),
    );
    if let Some(lang) = &channel.language {
        m.insert("language".to_string(), TemplateValue::String(lang.clone()));
    }
    m.insert(
        "generator".to_string(),
        TemplateValue::String(channel.generator.clone()),
    );
    m.insert(
        "last-build-date".to_string(),
        TemplateValue::String(channel.last_build_date.clone()),
    );
    if let Some(img) = &channel.image {
        let mut img_m: HashMap<String, TemplateValue> = HashMap::new();
        img_m.insert("url".to_string(), TemplateValue::String(img.url.clone()));
        img_m.insert(
            "title".to_string(),
            TemplateValue::String(img.title.clone()),
        );
        img_m.insert("link".to_string(), TemplateValue::String(img.link.clone()));
        if let Some(h) = img.height {
            img_m.insert("height".to_string(), TemplateValue::String(h.to_string()));
        }
        if let Some(w) = img.width {
            img_m.insert("width".to_string(), TemplateValue::String(w.to_string()));
        }
        m.insert("image".to_string(), TemplateValue::Map(img_m));
    }
    if let Some(stylesheet) = &channel.xml_stylesheet {
        m.insert(
            "xml-stylesheet".to_string(),
            TemplateValue::String(stylesheet.clone()),
        );
    }
    TemplateValue::Map(m)
}

fn item_template_context(item: &FeedItem) -> TemplateContext {
    let mut t = TemplateContext::new();
    t.insert("item", item_value(item));
    t
}

fn item_value(item: &FeedItem) -> TemplateValue {
    let mut m: HashMap<String, TemplateValue> = HashMap::new();
    m.insert(
        "title".to_string(),
        TemplateValue::String(item.title.clone()),
    );
    m.insert("link".to_string(), TemplateValue::String(item.link.clone()));
    m.insert("guid".to_string(), TemplateValue::String(item.guid.clone()));
    m.insert(
        "description-element".to_string(),
        TemplateValue::String(item.description_element.clone()),
    );
    if !item.authors.is_empty() {
        m.insert(
            "authors".to_string(),
            TemplateValue::List(
                item.authors
                    .iter()
                    .map(|a| TemplateValue::String(a.clone()))
                    .collect(),
            ),
        );
    }
    if !item.categories.is_empty() {
        m.insert(
            "categories".to_string(),
            TemplateValue::List(
                item.categories
                    .iter()
                    .map(|c| TemplateValue::String(c.clone()))
                    .collect(),
            ),
        );
    }
    if let Some(pd) = &item.pub_date_rfc822 {
        m.insert("pub-date".to_string(), TemplateValue::String(pd.clone()));
    }
    if let Some(img) = &item.image {
        let mut img_m: HashMap<String, TemplateValue> = HashMap::new();
        img_m.insert("url".to_string(), TemplateValue::String(img.url.clone()));
        img_m.insert(
            "attrs".to_string(),
            TemplateValue::String(img.attrs.clone()),
        );
        m.insert("image".to_string(), TemplateValue::Map(img_m));
    }
    TemplateValue::Map(m)
}

// ─────────────────────────────────────────────────────────────────
// Path / URL helpers
// ─────────────────────────────────────────────────────────────────

fn feed_type_extension(kind: FeedType) -> &'static str {
    match kind {
        FeedType::Full => "full",
        FeedType::Partial => "partial",
        FeedType::Metadata => "metadata",
    }
}

/// Compute a forward-slash project-relative URL for a host's
/// output path under the project's output_dir. Falls back to the
/// host filename when stripping fails (e.g. a synthetic test path
/// outside the output_dir).
fn href_relative_to_output_dir(host_output_path: &Path, output_dir: &Path) -> String {
    let stripped = host_output_path
        .strip_prefix(output_dir)
        .unwrap_or(host_output_path);
    stripped
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(os) => os.to_str().map(str::to_string),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Given `"posts/index.html"`, return `"posts"`. Returns empty
/// string when there is no parent directory.
fn parent_href(href: &str) -> String {
    let path = PathBuf::from(href);
    path.parent()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default()
}

// ─────────────────────────────────────────────────────────────────
// Diagnostics
// ─────────────────────────────────────────────────────────────────

/// Q-12-15: feed configured but `website.site-url` missing.
fn make_q_12_15() -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning(
        "A listing has `feed:` configured, but the project's `website.site-url` is missing. \
         Feeds require an absolute base URL to construct item links. Set `website.site-url` \
         in `_quarto.yml` to enable feed generation. The listing host page renders correctly \
         otherwise."
            .to_string(),
    )
    .with_code("Q-12-15")
    .with_location(SourceInfo::default())
    .build()
}

fn diagnostic_warning(msg: String) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning(msg)
        .with_location(SourceInfo::default())
        .build()
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
    use crate::project::listing::config::{Listing, apply_type_defaults};
    use crate::project::listing::config::{ListingFeedOptions, ListingType};
    use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use crate::render::BinaryDependencies;
    use quarto_pandoc_types::ConfigMapEntry;
    use quarto_source_map::SourceInfo;
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::sync::Arc;

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

    fn make_item(title: &str, date: Option<&str>) -> ListingItem {
        ListingItem {
            title: title.to_string(),
            subtitle: None,
            description: Some(format!("Description of {}.", title)),
            author: Some("Jane".to_string()),
            authors: vec!["Jane".to_string()],
            date: date.map(String::from),
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

    fn make_item_with_categories(title: &str, categories: Vec<&str>) -> ListingItem {
        let mut it = make_item(title, Some("2026-05-08"));
        it.categories = categories.into_iter().map(String::from).collect();
        it
    }

    fn make_listing_with_feed(id: &str, opts: ListingFeedOptions) -> Listing {
        let mut l = Listing {
            id: id.to_string(),
            kind: ListingType::Default,
            feed: Some(opts),
            ..Listing::default()
        };
        apply_type_defaults(&mut l);
        l
    }

    fn site_meta() -> ConfigValue {
        map(vec![(
            "website",
            map(vec![
                ("site-url", s("https://example.com")),
                ("title", s("Example Site")),
                ("description", s("A site of examples.")),
            ]),
        )])
    }

    fn no_url_meta() -> ConfigValue {
        map(vec![("website", map(vec![("title", s("Example Site"))]))])
    }

    /// Build a `ProjectContext` rooted at `project_dir` with
    /// `_site` as the output dir. Uses an in-tempdir layout so
    /// the staged-file write actually hits disk.
    fn make_project(project_dir: &Path) -> ProjectContext {
        ProjectContext {
            dir: project_dir.to_path_buf(),
            config: ProjectConfig::default(),
            is_single_file: false,
            files: vec![DocumentInfo::from_path(project_dir.join("posts.qmd"))],
            output_dir: project_dir.join("_site"),
        }
    }

    /// Run the transform with a host page at `<project_dir>/posts.qmd`,
    /// output dir at `<project_dir>/_site/`. Sets
    /// `ctx.document.output` so `ctx.output_path()` is deterministic.
    async fn run_transform(
        project_dir: &Path,
        meta: ConfigValue,
        resolved: Vec<ResolvedListing>,
    ) -> Vec<DiagnosticMessage> {
        let project = make_project(project_dir);
        let mut doc = DocumentInfo::from_path(project_dir.join("posts.qmd"));
        // Force the output to a known path so output_path() is
        // stable regardless of format defaults.
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

        ListingFeedStageTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .expect("transform should not error");
        ctx.diagnostics
    }

    fn read_staged(project_dir: &Path, name: &str) -> Option<String> {
        let path = project_dir.join("_site").join(name);
        std::fs::read_to_string(&path).ok()
    }

    // ---- Plan test #16: metadata feed inlines descriptions -----

    #[tokio::test]
    async fn stage_writes_metadata_feed_with_inline_descriptions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let opts = ListingFeedOptions {
            kind: FeedType::Metadata,
            ..ListingFeedOptions::default_feed_options()
        };
        let listing = make_listing_with_feed("listing-1", opts);
        let items = vec![
            make_item("foo", Some("2026-05-01")),
            make_item("bar", Some("2026-05-02")),
        ];
        let resolved = vec![ResolvedListing { listing, items }];
        let _ = run_transform(dir.path(), site_meta(), resolved).await;

        let staged = read_staged(dir.path(), "posts.feed-metadata-staged")
            .expect("metadata-staged file should exist");
        assert!(
            staged.contains("<title>Example Site</title>"),
            "channel title missing; got:\n{}",
            staged
        );
        assert!(
            staged.contains("<description><![CDATA[Description of foo.]]></description>"),
            "foo description CDATA missing; got:\n{}",
            staged
        );
        assert!(
            staged.contains("<description><![CDATA[Description of bar.]]></description>"),
            "bar description CDATA missing; got:\n{}",
            staged
        );
        // No placeholder tokens for metadata feeds.
        assert!(
            !staged.contains("B4F502887207"),
            "metadata feed should not contain placeholder tokens"
        );
    }

    // ---- Plan test #17: partial feed emits placeholders ------

    #[tokio::test]
    async fn stage_writes_partial_feed_with_placeholders() {
        let dir = tempfile::tempdir().expect("tempdir");
        let opts = ListingFeedOptions {
            kind: FeedType::Partial,
            ..ListingFeedOptions::default_feed_options()
        };
        let listing = make_listing_with_feed("listing-1", opts);
        let items = vec![make_item("foo", Some("2026-05-01"))];
        let resolved = vec![ResolvedListing { listing, items }];
        let _ = run_transform(dir.path(), site_meta(), resolved).await;

        let staged = read_staged(dir.path(), "posts.feed-partial-staged")
            .expect("partial-staged file should exist");
        assert!(
            staged.contains("<description>{B4F502887207:posts/foo.html}</description>"),
            "expected placeholder envelope; got:\n{}",
            staged
        );
    }

    // ---- Plan test #18: full feed emits placeholders --------

    #[tokio::test]
    async fn stage_writes_full_feed_with_placeholders() {
        let dir = tempfile::tempdir().expect("tempdir");
        let opts = ListingFeedOptions {
            kind: FeedType::Full,
            ..ListingFeedOptions::default_feed_options()
        };
        let listing = make_listing_with_feed("listing-1", opts);
        let items = vec![make_item("foo", Some("2026-05-01"))];
        let resolved = vec![ResolvedListing { listing, items }];
        let _ = run_transform(dir.path(), site_meta(), resolved).await;

        let staged = read_staged(dir.path(), "posts.feed-full-staged")
            .expect("full-staged file should exist");
        assert!(
            staged.contains("<description>{B4F502887207:posts/foo.html}</description>"),
            "expected placeholder envelope; got:\n{}",
            staged
        );
    }

    // ---- Plan test #19: Q-12-15 when no site-url ------------

    #[tokio::test]
    async fn stage_emits_q_12_15_when_no_site_url() {
        let dir = tempfile::tempdir().expect("tempdir");
        let opts = ListingFeedOptions::default_feed_options();
        let listing = make_listing_with_feed("listing-1", opts);
        let items = vec![make_item("foo", Some("2026-05-01"))];
        let resolved = vec![ResolvedListing { listing, items }];
        let diags = run_transform(dir.path(), no_url_meta(), resolved).await;

        // Exactly one Q-12-15 emitted.
        let q15: Vec<_> = diags
            .iter()
            .filter(|d| d.code.as_deref() == Some("Q-12-15"))
            .collect();
        assert_eq!(
            q15.len(),
            1,
            "expected exactly one Q-12-15; got {}",
            q15.len()
        );

        // No staged file written.
        let path = dir.path().join("_site").join("posts.feed-full-staged");
        assert!(
            !path.exists(),
            "staged file should NOT exist when site-url is missing"
        );
    }

    // ---- Plan test #20: per-category sub-feeds --------------

    #[tokio::test]
    async fn stage_writes_per_category_subfeeds() {
        let dir = tempfile::tempdir().expect("tempdir");
        let opts = ListingFeedOptions {
            kind: FeedType::Full,
            categories: vec!["Software".to_string(), "Reproducibility".to_string()],
            ..ListingFeedOptions::default_feed_options()
        };
        let listing = make_listing_with_feed("listing-1", opts);
        let items = vec![
            make_item_with_categories("alpha", vec!["Software"]),
            make_item_with_categories("beta", vec!["Reproducibility"]),
            make_item_with_categories("gamma", vec!["Software", "Reproducibility"]),
        ];
        let resolved = vec![ResolvedListing { listing, items }];
        let _ = run_transform(dir.path(), site_meta(), resolved).await;

        let main = read_staged(dir.path(), "posts.feed-full-staged")
            .expect("main staged file should exist");
        assert!(main.contains("alpha"));
        assert!(main.contains("beta"));
        assert!(main.contains("gamma"));

        let software = read_staged(dir.path(), "posts-software.feed-full-staged")
            .expect("software sub-feed should exist");
        assert!(software.contains("alpha"));
        assert!(software.contains("gamma"));
        assert!(
            !software.contains(">beta</title>"),
            "software sub-feed should not contain `beta` item; got:\n{}",
            software
        );

        let repro = read_staged(dir.path(), "posts-reproducibility.feed-full-staged")
            .expect("reproducibility sub-feed should exist");
        assert!(repro.contains("beta"));
        assert!(repro.contains("gamma"));
        assert!(
            !repro.contains(">alpha</title>"),
            "reproducibility sub-feed should not contain `alpha` item; got:\n{}",
            repro
        );
    }

    // ---- Plan test #21: feed.items truncates --------

    #[tokio::test]
    async fn stage_truncates_to_feed_items_count() {
        let dir = tempfile::tempdir().expect("tempdir");
        let opts = ListingFeedOptions {
            kind: FeedType::Full,
            items: Some(3),
            ..ListingFeedOptions::default_feed_options()
        };
        let listing = make_listing_with_feed("listing-1", opts);
        let items: Vec<_> = (0..10)
            .map(|i| make_item(&format!("post{i}"), Some("2026-05-08")))
            .collect();
        let resolved = vec![ResolvedListing { listing, items }];
        let _ = run_transform(dir.path(), site_meta(), resolved).await;

        let staged = read_staged(dir.path(), "posts.feed-full-staged").expect("staged file");
        let item_count = staged.matches("<item>").count();
        assert_eq!(item_count, 3, "expected 3 items; got {item_count}");
    }

    // ---- Plan test #22: default 20 items ----------

    #[tokio::test]
    async fn stage_uses_default_20_items() {
        let dir = tempfile::tempdir().expect("tempdir");
        let opts = ListingFeedOptions::default_feed_options();
        let listing = make_listing_with_feed("listing-1", opts);
        let items: Vec<_> = (0..30)
            .map(|i| make_item(&format!("post{i}"), Some("2026-05-08")))
            .collect();
        let resolved = vec![ResolvedListing { listing, items }];
        let _ = run_transform(dir.path(), site_meta(), resolved).await;

        let staged = read_staged(dir.path(), "posts.feed-full-staged").expect("staged file");
        let item_count = staged.matches("<item>").count();
        assert_eq!(
            item_count, 20,
            "expected 20 items by default; got {item_count}"
        );
    }

    // ---- Plan test #23: skip when no feed ----------

    #[tokio::test]
    async fn stage_skips_when_no_listing_feed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut listing = Listing {
            id: "no-feed".to_string(),
            kind: ListingType::Default,
            feed: None,
            ..Listing::default()
        };
        apply_type_defaults(&mut listing);
        let items = vec![make_item("foo", Some("2026-05-01"))];
        let resolved = vec![ResolvedListing { listing, items }];
        let _ = run_transform(dir.path(), site_meta(), resolved).await;

        // No staged file should appear in the output dir.
        let entries = std::fs::read_dir(dir.path().join("_site")).map(|d| {
            d.filter_map(|r| r.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        });
        match entries {
            Ok(names) => {
                assert!(
                    !names.iter().any(|n| n.contains(".feed-")),
                    "no staged feeds expected; got entries: {:?}",
                    names
                );
            }
            // OK if the output dir doesn't even exist yet (no-op transform).
            Err(_) => {}
        }
    }

    // ---- Plan test #24: xml-stylesheet PI ----------

    #[tokio::test]
    async fn stage_xml_stylesheet_pi_emitted_when_set() {
        let dir = tempfile::tempdir().expect("tempdir");
        let opts = ListingFeedOptions {
            kind: FeedType::Full,
            xml_stylesheet: Some(PathBuf::from("feed.xsl")),
            ..ListingFeedOptions::default_feed_options()
        };
        let listing = make_listing_with_feed("listing-1", opts);
        let items = vec![make_item("foo", Some("2026-05-01"))];
        let resolved = vec![ResolvedListing { listing, items }];
        let _ = run_transform(dir.path(), site_meta(), resolved).await;

        let staged = read_staged(dir.path(), "posts.feed-full-staged").expect("staged file");
        assert!(
            staged.contains(r#"<?xml-stylesheet type="text/xsl" media="screen" href="feed.xsl"?>"#),
            "expected xml-stylesheet PI; got:\n{}",
            staged
        );
    }

    // ---- multi-listing qualifier ----------

    #[tokio::test]
    async fn stage_qualifies_filename_for_multi_feed_hosts() {
        let dir = tempfile::tempdir().expect("tempdir");
        let opts1 = ListingFeedOptions::default_feed_options();
        let opts2 = ListingFeedOptions {
            kind: FeedType::Metadata,
            ..ListingFeedOptions::default_feed_options()
        };
        let l1 = make_listing_with_feed("listing-a", opts1);
        let l2 = make_listing_with_feed("listing-b", opts2);
        let items = vec![make_item("foo", Some("2026-05-01"))];
        let resolved = vec![
            ResolvedListing {
                listing: l1,
                items: items.clone(),
            },
            ResolvedListing { listing: l2, items },
        ];
        let _ = run_transform(dir.path(), site_meta(), resolved).await;

        // Two distinct staged files, qualified by listing id.
        assert!(
            read_staged(dir.path(), "posts-listing-a.feed-full-staged").is_some(),
            "listing-a's staged file should exist"
        );
        assert!(
            read_staged(dir.path(), "posts-listing-b.feed-metadata-staged").is_some(),
            "listing-b's staged file should exist"
        );
        // Neither bare-stem file should exist.
        assert!(!dir.path().join("_site/posts.feed-full-staged").exists());
    }

    // ---- Helper for default-construction in tests ----

    impl ListingFeedOptions {
        fn default_feed_options() -> Self {
            Self {
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
    }
}
