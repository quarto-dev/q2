/*
 * link_rewrite.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Body-content link and image-URL rewriting transform.
//!
//! Walks the AST body and rewrites two kinds of `target.0` URLs to
//! page-relative form: [`Inline::Link`]s that point at another
//! project document, and [`Inline::Image`]s, whose targets are
//! static resources. Phase 6 of the website-projects epic; images
//! added by bd-root-relative-paths-design-fc5pvkcv (Case B).
//!
//! See `claude-notes/plans/2026-04-24-websites-phase-6.md` for the
//! original design (especially Decisions 1, 2, 6, 7) and
//! `claude-notes/plans/2026-08-13-site-root-relative-paths.md` for
//! the image extension.
//!
//! ## What it rewrites
//!
//! - Internal `.qmd` references resolved through
//!   [`ProjectIndex`](crate::project::index::ProjectIndex), with
//!   `..` / `.` / leading `/` normalization handled by
//!   [`resolve_doc_relative_href`](super::navigation_href::resolve_doc_relative_href).
//! - `Image::target.0` via
//!   [`resolve_static_resource_href`](super::navigation_href::resolve_static_resource_href)
//!   — no index lookup, no `.qmd` diagnostic. This is what keeps a
//!   site-root-relative `![](/images/x.svg)` working under a deploy
//!   subpath: a page two levels deep emits `../../images/x.svg`,
//!   matching Q1. Relative targets round-trip (modulo `..`/`.`
//!   normalization, a deliberate side effect).
//! - Query strings and fragment anchors are preserved across both
//!   rewrites (`other.qmd?x=1#sec` → `other.html?x=1#sec`).
//!
//! ## What it leaves alone
//!
//! - External URLs (`http:`, `https:`, `mailto:`, `tel:`, `ftp:`,
//!   `//host/...`) and `data:` URIs.
//! - Fragment-only anchors (`#section`).
//! - Body *links* in a document rendered without a `ProjectIndex`
//!   (standalone single-doc render) — doc-link resolution needs the
//!   index (Decision 7). Images still rewrite whenever a
//!   [`ResourceResolverContext`] is attached; with neither index nor
//!   resolver the walk still runs but rewrites nothing — it only
//!   consumes `link-format` attributes (see [`LINK_FORMAT_ATTR`]),
//!   which must never reach a writer.
//!
//! ## Pipeline placement
//!
//! Runs at the start of the Finalization Phase, after all
//! Navigation Phase Render transforms but before
//! `AppendixStructureTransform`. By this point most semantic
//! transforms (callouts, crossrefs, shortcodes) have already
//! resolved their custom nodes, so the AST is mostly plain Pandoc
//! nodes. The walk recurses through `Inline::Custom` slots
//! defensively, in case any custom nodes remain.

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::Slot;
use quarto_pandoc_types::block::Block;
use quarto_pandoc_types::inline::{Inline, Inlines, Link};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceInfo;

use crate::Result;
use crate::project::index::ProjectIndex;
use crate::render::RenderContext;
use crate::resource_resolver::ResourceResolverContext;
use crate::transform::{AstTransform, TransformPhase};
use crate::transforms::llms::{companion_href, profile_has_companion, strip_kv};
use crate::transforms::navigation_active::page_relative_source;
use crate::transforms::navigation_href::{
    resolve_doc_relative_href, resolve_doc_relative_target, resolve_static_resource_href,
};

/// Attribute selecting which *output* of the target page a body link
/// points at (bd-llms-link-target-annotation-0zo2ppgx). Recognized
/// values today: `"html"` (keep the link on the HTML page, even
/// inside llms markdown companions) and `"llms"` (point the link at
/// the target page's markdown companion). The target must be written
/// as a project *source* path (`guide/index.qmd`-style); the
/// attribute alone selects the output. Consumed by this transform
/// and, for the surviving `html` pin, by `LlmsCaptureTransform` — it
/// must never reach a writer.
pub const LINK_FORMAT_ATTR: &str = "link-format";

/// Body-content link rewriter (Phase 6).
pub struct LinkRewriteTransform;

impl LinkRewriteTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LinkRewriteTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for LinkRewriteTransform {
    fn name(&self) -> &str {
        "link-rewrite"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Finalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        let index = ctx.project_index.as_deref();
        let resolver = ctx.resource_resolver.as_ref();
        let llms_enabled = crate::transforms::llms::llms_companions_enabled(&ast.meta, ctx);
        let llms_active = crate::transforms::llms::llms_view_active(&ast.meta, ctx);

        // With no index (Decision 7) and no resolver, links and
        // images pass through — but the walk still runs: it owns
        // consuming `link-format` attributes, which must not reach
        // the writer even in a bare standalone render.
        let source = page_relative_source(ctx);

        // Move diagnostics into a local buffer so the helper can
        // push without a borrow cycle on `ctx`.
        let mut local_diags = std::mem::take(&mut ctx.diagnostics);
        let mut rewriter = LinkRewriter {
            source: &source,
            index,
            resolver,
            llms_enabled,
            llms_active,
            diagnostics: &mut local_diags,
        };
        for block in &mut ast.blocks {
            rewriter.visit_block(block);
        }
        ctx.diagnostics = local_diags;

        Ok(())
    }
}

struct LinkRewriter<'a> {
    source: &'a str,
    /// `None` in standalone renders — link targets then pass through
    /// (Decision 7) while image targets still rewrite.
    index: Option<&'a ProjectIndex>,
    resolver: Option<&'a ResourceResolverContext>,
    /// `llms_companions_enabled`: the project generates markdown
    /// companions (website + `llms-txt: true`). Gates `llms`-pin
    /// satisfiability — any page of the site, whatever its own
    /// format, may target another page's companion.
    llms_enabled: bool,
    /// `llms_view_active`: this render's own llms view (companions
    /// enabled *and* plain html format). Gates whether an `html` pin
    /// survives to its consumer (`LlmsCaptureTransform` runs only
    /// then).
    llms_active: bool,
    diagnostics: &'a mut Vec<quarto_error_reporting::DiagnosticMessage>,
}

impl<'a> LinkRewriter<'a> {
    fn visit_block(&mut self, block: &mut Block) {
        match block {
            Block::Plain(p) => self.visit_inlines(&mut p.content),
            Block::Paragraph(p) => self.visit_inlines(&mut p.content),
            Block::LineBlock(lb) => {
                for line in lb.content.iter_mut() {
                    self.visit_inlines(line);
                }
            }
            Block::BlockQuote(bq) => {
                for b in bq.content.iter_mut() {
                    self.visit_block(b);
                }
            }
            Block::OrderedList(ol) => {
                for item in ol.content.iter_mut() {
                    for b in item.iter_mut() {
                        self.visit_block(b);
                    }
                }
            }
            Block::BulletList(bl) => {
                for item in bl.content.iter_mut() {
                    for b in item.iter_mut() {
                        self.visit_block(b);
                    }
                }
            }
            Block::DefinitionList(dl) => {
                for (term, defs) in dl.content.iter_mut() {
                    self.visit_inlines(term);
                    for def in defs.iter_mut() {
                        for b in def.iter_mut() {
                            self.visit_block(b);
                        }
                    }
                }
            }
            Block::Header(h) => self.visit_inlines(&mut h.content),
            Block::Div(d) => {
                for b in d.content.iter_mut() {
                    self.visit_block(b);
                }
            }
            Block::Figure(f) => {
                for b in f.content.iter_mut() {
                    self.visit_block(b);
                }
            }
            Block::Table(t) => {
                if let Some(short) = t.caption.short.as_mut() {
                    self.visit_inlines(short);
                }
                if let Some(long) = t.caption.long.as_mut() {
                    for b in long.iter_mut() {
                        self.visit_block(b);
                    }
                }
                for row in t.head.rows.iter_mut().chain(t.foot.rows.iter_mut()) {
                    for cell in row.cells.iter_mut() {
                        for b in cell.content.iter_mut() {
                            self.visit_block(b);
                        }
                    }
                }
                for body in t.bodies.iter_mut() {
                    for row in body.body.iter_mut() {
                        for cell in row.cells.iter_mut() {
                            for b in cell.content.iter_mut() {
                                self.visit_block(b);
                            }
                        }
                    }
                }
            }
            Block::CaptionBlock(cb) => self.visit_inlines(&mut cb.content),
            Block::Custom(c) => {
                for (_name, slot) in c.slots.iter_mut() {
                    self.visit_slot(slot);
                }
            }
            // Variants without nested rewritable content.
            Block::CodeBlock(_)
            | Block::RawBlock(_)
            | Block::HorizontalRule(_)
            | Block::BlockMetadata(_)
            | Block::NoteDefinitionPara(_)
            | Block::NoteDefinitionFencedBlock(_) => {}
        }
    }

    fn visit_inlines(&mut self, inlines: &mut Inlines) {
        for inline in inlines.iter_mut() {
            self.visit_inline(inline);
        }
    }

    fn visit_inline(&mut self, inline: &mut Inline) {
        match inline {
            Inline::Link(link) => {
                // Recurse into link content first — a link's inner
                // text could itself contain rewritable inlines
                // (uncommon, but possible after filter passes).
                self.visit_inlines(&mut link.content);
                if link.attr.2.contains_key(LINK_FORMAT_ATTR) {
                    self.apply_link_format(link);
                } else {
                    self.resolve_undecorated(link);
                }
            }
            Inline::Image(img) => {
                // Walk image content (alt-text inlines), then rebase
                // the image URL itself. Images point at static
                // resources, so this goes through the static helper:
                // no index lookup, no `.qmd` diagnostic — just
                // normalize (leading `/` = site-root, Decision 4 of
                // bd-root-relative-paths-design-fc5pvkcv) and
                // relativize to the page.
                //
                // EXCEPT in VFS-root mode (hub-client q2-preview):
                // preview images are not fetched by URL — the
                // parent-side asset walker reads the VFS and mints
                // blob URLs keyed by the *user-written* path (the
                // contract pinned by
                // `hub-client/src/services/assetManifestProject.wasm.test.ts`),
                // so a rewrite here would orphan every preview image.
                // Same mode-gate as `ResourceCollectorTransform`.
                self.visit_inlines(&mut img.content);
                if !self.resolver.is_some_and(|r| r.is_vfs_root_mode()) {
                    img.target.0 =
                        resolve_static_resource_href(&img.target.0, self.source, self.resolver);
                }
            }
            Inline::Emph(e) => self.visit_inlines(&mut e.content),
            Inline::Underline(u) => self.visit_inlines(&mut u.content),
            Inline::Strong(s) => self.visit_inlines(&mut s.content),
            Inline::Strikeout(s) => self.visit_inlines(&mut s.content),
            Inline::Superscript(s) => self.visit_inlines(&mut s.content),
            Inline::Subscript(s) => self.visit_inlines(&mut s.content),
            Inline::SmallCaps(s) => self.visit_inlines(&mut s.content),
            Inline::Quoted(q) => self.visit_inlines(&mut q.content),
            Inline::Note(n) => {
                for b in n.content.iter_mut() {
                    self.visit_block(b);
                }
            }
            Inline::Span(s) => self.visit_inlines(&mut s.content),
            Inline::Insert(i) => self.visit_inlines(&mut i.content),
            Inline::Delete(d) => self.visit_inlines(&mut d.content),
            Inline::Highlight(h) => self.visit_inlines(&mut h.content),
            Inline::Custom(c) => {
                for (_name, slot) in c.slots.iter_mut() {
                    self.visit_slot(slot);
                }
            }
            // No-op variants: leaves with no rewritable nested content.
            Inline::Str(_)
            | Inline::Cite(_)
            | Inline::Code(_)
            | Inline::Space(_)
            | Inline::SoftBreak(_)
            | Inline::LineBreak(_)
            | Inline::Math(_)
            | Inline::RawInline(_)
            | Inline::Shortcode(_)
            | Inline::NoteReference(_)
            | Inline::Attr(_)
            | Inline::EditComment(_) => {}
        }
    }

    /// The undecorated body-link resolution: source path → output
    /// href through the index (Decision 7: pass-through without one).
    fn resolve_undecorated(&mut self, link: &mut Link) {
        if let Some(index) = self.index {
            let new_url = resolve_doc_relative_href(
                &link.target.0,
                self.source,
                self.resolver,
                Some(index),
                link.target_source.url.clone(),
                self.diagnostics,
            );
            link.target.0 = new_url;
        }
    }

    /// Handle a `link-format`-decorated link.
    ///
    /// The one case that leaves the attribute in place is a
    /// satisfiable `html` pin: `LlmsCaptureTransform` (tail of
    /// Finalization) is its consumer — it skips the companion
    /// retarget and scrubs the attribute from both views. Every
    /// other path consumes the attribute here; unsatisfiable
    /// requests warn with Q-13-9 and fall back to the undecorated
    /// resolution.
    fn apply_link_format(&mut self, link: &mut Link) {
        let value = link
            .attr
            .2
            .get(LINK_FORMAT_ATTR)
            .cloned()
            .unwrap_or_default();
        let raw = link.target.0.clone();
        let location = link.target_source.url.clone();

        // The target page, under the source-paths-only rule: the
        // href must name a project source (`.qmd` / `.md`) that is
        // in the index.
        let target_profile = resolve_doc_relative_target(&raw, self.source)
            .and_then(|p| self.index.and_then(|idx| idx.lookup_by_source(&p)));

        // Satisfiable `html` pin: resolve normally, keep the attr.
        if value == "html" && self.llms_active && target_profile.is_some() {
            self.resolve_undecorated(link);
            return;
        }

        // `None` = consume silently; `Some` = warn with this problem.
        let failure: Option<String> = match value.as_str() {
            // Inert pin: with the llms view off nothing would ever
            // retarget the link, so the request is trivially
            // honored.
            "html" if !self.llms_active => None,
            "html" => Some(format!(
                "`{raw}` is not a project source path in the render set, \
                 so there is no page output to pin."
            )),
            "llms" if !self.llms_enabled => Some(
                "Markdown companions only exist on a website with \
                 `llms-txt: true` set under `website:`."
                    .to_string(),
            ),
            "llms" => match target_profile {
                None => Some(format!(
                    "`{raw}` is not a project source path in the render set, \
                     so it has no markdown companion to target."
                )),
                Some(profile) if !profile_has_companion(profile) => Some(format!(
                    "`{raw}` resolves to a page with no markdown companion \
                     (drafts and the 404 page are excluded)."
                )),
                Some(profile) => {
                    // Satisfied: point the link at the companion,
                    // page-relative, keeping any ?query/#fragment
                    // tail as written.
                    let companion = companion_href(&profile.output_href)
                        .expect("companion-eligible pages render to .html");
                    let tail = raw.find(['#', '?']).map_or("", |i| &raw[i..]);
                    let url = match self.resolver {
                        Some(r) => r.page_url_for(&companion),
                        None => companion,
                    };
                    link.target.0 = format!("{url}{tail}");
                    strip_kv(&mut link.attr, &mut link.attr_source, LINK_FORMAT_ATTR);
                    return;
                }
            },
            other => Some(format!(
                "`{other}` is not a recognized link-format value \
                 (expected \"html\" or \"llms\")."
            )),
        };

        strip_kv(&mut link.attr, &mut link.attr_source, LINK_FORMAT_ATTR);
        if let Some(problem) = failure {
            self.diagnostics
                .push(link_format_warning(problem, location));
        }
        self.resolve_undecorated(link);
    }

    fn visit_slot(&mut self, slot: &mut Slot) {
        match slot {
            Slot::Block(b) => self.visit_block(b),
            Slot::Blocks(bs) => {
                for b in bs.iter_mut() {
                    self.visit_block(b);
                }
            }
            Slot::Inline(i) => self.visit_inline(i),
            Slot::Inlines(is) => self.visit_inlines(is),
        }
    }
}

/// Q-13-9: a `link-format` request that cannot be honored. The
/// render proceeds with the undecorated resolution.
fn link_format_warning(problem: String, location: Option<SourceInfo>) -> DiagnosticMessage {
    let mut builder = DiagnosticMessageBuilder::warning("link-format request cannot be honored")
        .with_code("Q-13-9")
        .problem(problem)
        .add_hint(
            "Write the target as the page's source path (e.g. `guide/index.qmd`) \
             and use `link-format=\"html\"` or `link-format=\"llms\"`. The \
             attribute was ignored.",
        );
    if let Some(loc) = location {
        builder = builder.with_location(loc);
    }
    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_profile::DocumentProfile;
    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use crate::render::BinaryDependencies;
    use quarto_pandoc_types::attr::{Attr, AttrSourceInfo, TargetSourceInfo};
    use quarto_pandoc_types::block::{Block, BulletList, Div, Paragraph};
    use quarto_pandoc_types::config_value::ConfigValue;
    use quarto_pandoc_types::custom::CustomNode;
    use quarto_pandoc_types::inline::{Emph, Image, Inline, Link, Str};
    use quarto_pandoc_types::pandoc::Pandoc;
    use quarto_pandoc_types::{ConfigMapEntry, Slot};
    use quarto_source_map::{FileId, SourceInfo};
    use std::path::PathBuf;
    use std::sync::Arc;

    fn make_profile(source: &str, output_href: &str) -> DocumentProfile {
        DocumentProfile {
            source_path: PathBuf::from(source),
            output_href: output_href.to_string(),
            format_id: "html".to_string(),
            title: Some("T".to_string()),
            ..DocumentProfile::default()
        }
    }

    fn make_project() -> ProjectContext {
        ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: false,
            files: vec![DocumentInfo::from_path("/project/index.qmd")],
            output_dir: PathBuf::from("/project/_site"),

            ..Default::default()
        }
    }

    fn empty_meta() -> ConfigValue {
        ConfigValue::new_map(Vec::<ConfigMapEntry>::new(), SourceInfo::for_test())
    }

    fn link_inline(url: &str, text: &str) -> Inline {
        Inline::Link(Link {
            attr: Attr::default(),
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: SourceInfo::for_test(),
            })],
            target: (url.to_string(), String::new()),
            source_info: SourceInfo::for_test(),
            attr_source: AttrSourceInfo::empty(),
            target_source: TargetSourceInfo::empty(),
        })
    }

    /// Like `link_inline`, but stamps `target_source.url` with a real
    /// `SourceInfo` so tests can assert that diagnostics receive the
    /// link URL's source location.
    fn link_inline_with_url_source(url: &str, text: &str, url_source: SourceInfo) -> Inline {
        Inline::Link(Link {
            attr: Attr::default(),
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: SourceInfo::for_test(),
            })],
            target: (url.to_string(), String::new()),
            source_info: SourceInfo::for_test(),
            attr_source: AttrSourceInfo::empty(),
            target_source: TargetSourceInfo {
                url: Some(url_source),
                title: None,
            },
        })
    }

    fn image_inline(url: &str, alt: &str) -> Inline {
        Inline::Image(Image {
            attr: Attr::default(),
            content: vec![Inline::Str(Str {
                text: alt.to_string(),
                source_info: SourceInfo::for_test(),
            })],
            target: (url.to_string(), String::new()),
            source_info: SourceInfo::for_test(),
            attr_source: AttrSourceInfo::empty(),
            target_source: TargetSourceInfo::empty(),
        })
    }

    fn para(content: Vec<Inline>) -> Block {
        Block::Paragraph(Paragraph {
            content,
            source_info: SourceInfo::for_test(),
        })
    }

    /// Run the Link rewrite transform on a synthetic AST.
    ///
    /// `source_doc` is the project-relative path of the doc being
    /// rendered (e.g. `"docs/api.qmd"`).
    /// `output_href` is the target output path used by the resolver
    /// (e.g. `"docs/api.html"`).
    /// `with_index` decides whether to attach a `ProjectIndex` —
    /// `false` exercises the standalone-render no-op branch.
    async fn run(
        blocks: Vec<Block>,
        source_doc: &str,
        output_href: &str,
        index_profiles: Vec<DocumentProfile>,
        with_index: bool,
    ) -> (Vec<Block>, Vec<quarto_error_reporting::DiagnosticMessage>) {
        let project = make_project();
        let input_path = format!("/project/{}", source_doc);
        let doc = DocumentInfo::from_path(&input_path);
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        if with_index {
            ctx = ctx.with_project_index(Arc::new(ProjectIndex::new(index_profiles)));
        }
        // Wire a website-flavored resolver pinned at the given page.
        let page_output = format!("/project/_site/{}", output_href);
        let stem = std::path::Path::new(output_href)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("index");
        ctx.resource_resolver = Some(ResourceResolverContext::website(
            "/project/_site",
            page_output,
            "site_libs",
            stem,
        ));
        let mut ast = Pandoc {
            meta: empty_meta(),
            blocks,
        };
        LinkRewriteTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        (ast.blocks, ctx.diagnostics)
    }

    fn first_link_url(blocks: &[Block]) -> &str {
        for b in blocks {
            if let Block::Paragraph(p) = b {
                for i in &p.content {
                    if let Inline::Link(l) = i {
                        return &l.target.0;
                    }
                }
            }
        }
        panic!("no link found");
    }

    /// Plan test 29: standalone render (no `project_index`) is a
    /// no-op; every body link survives unchanged.
    #[tokio::test]
    async fn link_rewrite_skips_when_no_index() {
        let blocks = vec![para(vec![link_inline("about.qmd", "About")])];
        let (out, diags) = run(blocks, "index.qmd", "index.html", vec![], false).await;
        assert_eq!(first_link_url(&out), "about.qmd");
        assert!(diags.is_empty());
    }

    /// Plan test 30: walking through a Paragraph, multiple Inline
    /// links rewrite.
    #[tokio::test]
    async fn link_rewrite_walks_paragraph_inlines() {
        let blocks = vec![para(vec![
            link_inline("about.qmd", "About"),
            Inline::Str(Str {
                text: " or ".into(),
                source_info: SourceInfo::for_test(),
            }),
            link_inline("docs/api.qmd", "API"),
        ])];
        let (out, _) = run(
            blocks,
            "index.qmd",
            "index.html",
            vec![
                make_profile("about.qmd", "about.html"),
                make_profile("docs/api.qmd", "docs/api.html"),
            ],
            true,
        )
        .await;
        let p = match &out[0] {
            Block::Paragraph(p) => p,
            _ => unreachable!(),
        };
        let urls: Vec<&str> = p
            .content
            .iter()
            .filter_map(|i| match i {
                Inline::Link(l) => Some(l.target.0.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(urls, vec!["about.html", "docs/api.html"]);
    }

    /// Plan test 31: link nested inside `Emph`.
    #[tokio::test]
    async fn link_rewrite_walks_nested_emph_link() {
        let emph = Inline::Emph(Emph {
            content: vec![link_inline("about.qmd", "About")],
            source_info: SourceInfo::for_test(),
        });
        let blocks = vec![para(vec![emph])];
        let (out, _) = run(
            blocks,
            "index.qmd",
            "index.html",
            vec![make_profile("about.qmd", "about.html")],
            true,
        )
        .await;
        let p = match &out[0] {
            Block::Paragraph(p) => p,
            _ => unreachable!(),
        };
        let inner = match &p.content[0] {
            Inline::Emph(e) => &e.content,
            _ => unreachable!(),
        };
        match &inner[0] {
            Inline::Link(l) => assert_eq!(l.target.0, "about.html"),
            _ => panic!("expected Link"),
        }
    }

    /// Plan test 32: link inside a `Div`.
    #[tokio::test]
    async fn link_rewrite_walks_div_blocks() {
        let div = Block::Div(Div {
            attr: Attr::default(),
            content: vec![para(vec![link_inline("about.qmd", "About")])],
            source_info: SourceInfo::for_test(),
            attr_source: AttrSourceInfo::empty(),
        });
        let (out, _) = run(
            vec![div],
            "index.qmd",
            "index.html",
            vec![make_profile("about.qmd", "about.html")],
            true,
        )
        .await;
        let inner_blocks = match &out[0] {
            Block::Div(d) => &d.content,
            _ => unreachable!(),
        };
        assert_eq!(first_link_url(inner_blocks), "about.html");
    }

    /// Plan test 33: link inside a bullet list item.
    #[tokio::test]
    async fn link_rewrite_walks_lists() {
        let bl = Block::BulletList(BulletList {
            content: vec![vec![para(vec![link_inline("about.qmd", "About")])]],
            source_info: SourceInfo::for_test(),
        });
        let (out, _) = run(
            vec![bl],
            "index.qmd",
            "index.html",
            vec![make_profile("about.qmd", "about.html")],
            true,
        )
        .await;
        let item_blocks = match &out[0] {
            Block::BulletList(b) => &b.content[0],
            _ => unreachable!(),
        };
        assert_eq!(first_link_url(item_blocks), "about.html");
    }

    /// Plan test 34: link inside a `Custom` node's `Inlines` slot.
    #[tokio::test]
    async fn link_rewrite_walks_custom_node_slots() {
        // Build a minimal CustomNode with one Inlines slot
        // containing the link. We don't need a registered custom-
        // node type for the walker — it iterates `slots` generically.
        let mut custom = CustomNode::new("test:wrapper", Attr::default(), SourceInfo::for_test());
        let link = link_inline("about.qmd", "About");
        custom
            .slots
            .insert("content".to_string(), Slot::Inlines(vec![link]));
        let blocks = vec![Block::Custom(custom)];
        let (out, _) = run(
            blocks,
            "index.qmd",
            "index.html",
            vec![make_profile("about.qmd", "about.html")],
            true,
        )
        .await;
        let custom_out = match &out[0] {
            Block::Custom(c) => c,
            _ => unreachable!(),
        };
        let slot = custom_out.slots.get("content").unwrap();
        let inlines = match slot {
            Slot::Inlines(is) => is,
            _ => unreachable!(),
        };
        match &inlines[0] {
            Inline::Link(l) => assert_eq!(l.target.0, "about.html"),
            _ => panic!("expected Link"),
        }
    }

    /// Plan test 35: external URL passes through.
    #[tokio::test]
    async fn link_rewrite_external_pass_through() {
        let blocks = vec![para(vec![link_inline("https://example.com", "X")])];
        let (out, diags) = run(
            blocks,
            "index.qmd",
            "index.html",
            vec![make_profile("about.qmd", "about.html")],
            true,
        )
        .await;
        assert_eq!(first_link_url(&out), "https://example.com");
        assert!(diags.is_empty());
    }

    /// Plan test 36: fragment-only anchor passes through.
    #[tokio::test]
    async fn link_rewrite_fragment_pass_through() {
        let blocks = vec![para(vec![link_inline("#section", "X")])];
        let (out, diags) = run(blocks, "index.qmd", "index.html", vec![], true).await;
        assert_eq!(first_link_url(&out), "#section");
        assert!(diags.is_empty());
    }

    /// Plan test 37: image URLs are not rewritten, but a Link
    /// inside the image's alt-content is.
    #[tokio::test]
    async fn link_rewrite_image_url_unchanged() {
        // Image whose target.0 looks like a project doc — must NOT
        // be rewritten. We don't try the alt-content link here
        // (uncommon shape), just the image URL preservation contract.
        let blocks = vec![para(vec![image_inline("about.qmd", "alt text")])];
        let (out, _) = run(
            blocks,
            "index.qmd",
            "index.html",
            vec![make_profile("about.qmd", "about.html")],
            true,
        )
        .await;
        let p = match &out[0] {
            Block::Paragraph(p) => p,
            _ => unreachable!(),
        };
        match &p.content[0] {
            Inline::Image(i) => assert_eq!(i.target.0, "about.qmd"),
            _ => panic!("expected Image"),
        }
    }

    /// Plan test 38: body-link miss emits the structured Q-13-4
    /// diagnostic (bd-8d6rk migration).
    #[tokio::test]
    async fn link_rewrite_diagnostic_uses_body_link_label() {
        let blocks = vec![para(vec![link_inline("missing.qmd", "X")])];
        let (out, diags) = run(blocks, "index.qmd", "index.html", vec![], true).await;
        assert_eq!(first_link_url(&out), "missing.qmd");
        assert_eq!(diags.len(), 1);
        let d = &diags[0];
        assert_eq!(d.code.as_deref(), Some("Q-13-4"));
        assert!(d.title.starts_with("Body link"), "got title: {:?}", d.title);
        assert!(
            d.problem
                .as_ref()
                .is_some_and(|p| p.as_str().contains("missing.qmd")),
            "Q-13-4 problem must mention missing.qmd; got {:?}",
            d.problem
        );
    }

    /// bd-c05x6: body-link Q-13-4 diagnostic carries the URL's
    /// `SourceInfo` when the link node has `target_source.url` set
    /// (i.e. when the link was produced by the qmd parser, not
    /// programmatically constructed by a filter).
    #[tokio::test]
    async fn link_rewrite_diagnostic_carries_source_location() {
        // Place the URL at byte range [4, 15) of `FileId(0)` — the
        // exact range doesn't matter, only that it survives.
        let url_loc = SourceInfo::original(FileId(0), 4, 15);
        let blocks = vec![para(vec![link_inline_with_url_source(
            "missing.qmd",
            "X",
            url_loc.clone(),
        )])];
        let (_out, diags) = run(blocks, "index.qmd", "index.html", vec![], true).await;
        assert_eq!(diags.len(), 1, "expected exactly one diagnostic");
        let d = &diags[0];
        assert_eq!(d.code.as_deref(), Some("Q-13-4"));
        assert_eq!(
            d.location.as_ref(),
            Some(&url_loc),
            "Q-13-4 must carry the link URL's SourceInfo; got {:?}",
            d.location
        );
    }

    // ---- Case B (bd-root-relative-paths-design-fc5pvkcv): image targets ----
    //
    // Image targets are static resources; they rebase through
    // `resolve_static_resource_href` so a root-absolute (site-root)
    // path lands page-relative, matching Q1. The link row of the
    // strand's repro is the control: both are ordinary AST nodes in
    // the same paragraph, and both must resolve.

    fn image_urls(blocks: &[Block]) -> Vec<String> {
        let mut urls = Vec::new();
        for b in blocks {
            if let Block::Paragraph(p) = b {
                for i in &p.content {
                    if let Inline::Image(img) = i {
                        urls.push(img.target.0.clone());
                    }
                }
            }
        }
        urls
    }

    /// A root-absolute image target rebases to a page-relative URL on
    /// a depth-2 page — the exact repro row that q2 got wrong while
    /// getting the sibling link right.
    #[tokio::test]
    async fn image_rewrite_root_absolute_rebases_at_depth() {
        let blocks = vec![para(vec![image_inline("/images/x.svg", "x")])];
        let (out, diags) = run(
            blocks,
            "deep/deeper/index.qmd",
            "deep/deeper/index.html",
            vec![],
            true,
        )
        .await;
        assert_eq!(image_urls(&out), vec!["../../images/x.svg"]);
        assert!(diags.is_empty(), "static rebasing must not diagnose");
    }

    /// Relative image targets stay correct: `..`-laden paths
    /// normalize, plain relative paths round-trip unchanged
    /// (decision 2: all targets route through the resolver, and
    /// normalization is a desired side effect).
    #[tokio::test]
    async fn image_rewrite_normalizes_relative_targets() {
        let blocks = vec![para(vec![
            image_inline("a/../b.png", "b"),
            image_inline("../x.png", "x"),
            image_inline("figs/d.png", "d"),
        ])];
        let (out, diags) = run(
            blocks,
            "deep/deeper/index.qmd",
            "deep/deeper/index.html",
            vec![],
            true,
        )
        .await;
        assert_eq!(image_urls(&out), vec!["b.png", "../x.png", "figs/d.png"]);
        assert!(diags.is_empty());
    }

    /// External URLs, data: URIs, and fragment-only image targets pass
    /// through untouched.
    #[tokio::test]
    async fn image_rewrite_leaves_external_untouched() {
        let urls_in = [
            "https://example.com/remote.png",
            "data:image/png;base64,AAAA",
            "//cdn.example.com/x.png",
            "#gradient-stop",
        ];
        let blocks = vec![para(
            urls_in.iter().map(|u| image_inline(u, "alt")).collect(),
        )];
        let (out, diags) = run(
            blocks,
            "deep/deeper/index.qmd",
            "deep/deeper/index.html",
            vec![],
            true,
        )
        .await;
        assert_eq!(image_urls(&out), urls_in.to_vec());
        assert!(diags.is_empty());
    }

    /// Query / fragment tails survive the image rewrite.
    #[tokio::test]
    async fn image_rewrite_preserves_tail() {
        let blocks = vec![para(vec![
            image_inline("/images/x.svg#frag", "x"),
            image_inline("/images/x.svg?v=2", "x"),
        ])];
        let (out, _) = run(
            blocks,
            "deep/deeper/index.qmd",
            "deep/deeper/index.html",
            vec![],
            true,
        )
        .await;
        assert_eq!(
            image_urls(&out),
            vec!["../../images/x.svg#frag", "../../images/x.svg?v=2"]
        );
    }

    /// VFS-root mode (hub-client q2-preview): image targets pass
    /// through **untouched**. Preview images are not fetched by URL —
    /// the parent-side asset walker reads the VFS and mints blob URLs
    /// keyed by the *user-written* path
    /// (`hub-client/src/services/assetManifestProject.wasm.test.ts`
    /// pins that contract), so a rewrite here would orphan every
    /// preview image. Links are unaffected (they already rewrite in
    /// VFS mode — bd-kw93.14).
    #[tokio::test]
    async fn image_rewrite_skipped_in_vfs_root_mode() {
        let blocks = vec![para(vec![image_inline("../hero.png", "h")])];
        let project = make_project();
        let doc = DocumentInfo::from_path("/project/sub/page.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        ctx.resource_resolver = Some(ResourceResolverContext::vfs_root("/project"));
        let mut ast = Pandoc {
            meta: empty_meta(),
            blocks,
        };
        LinkRewriteTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        assert_eq!(
            image_urls(&ast.blocks),
            vec!["../hero.png"],
            "VFS-root mode must preserve the user-written image path"
        );
    }

    // ---- link-format attribute (bd-llms-link-target-annotation-0zo2ppgx) ----
    //
    // `link-format="llms"` retargets a source-path link at the target
    // page's markdown companion; `link-format="html"` survives this
    // transform (LlmsCaptureTransform consumes it) when the llms view
    // is active and is silently stripped when it is not. Unsatisfiable
    // pins warn with Q-13-9 and fall back to the normal resolution.

    use crate::project::ProjectKind;

    fn llms_meta() -> ConfigValue {
        let entry = |k: &str, v: ConfigValue| ConfigMapEntry {
            key: k.to_string(),
            key_source: SourceInfo::for_test(),
            value: v,
        };
        let website = ConfigValue::new_map(
            vec![entry(
                "llms-txt",
                ConfigValue::new_bool(true, SourceInfo::for_test()),
            )],
            SourceInfo::for_test(),
        );
        ConfigValue::new_map(vec![entry("website", website)], SourceInfo::for_test())
    }

    fn link_with_kv(url: &str, text: &str, kv: &[(&str, &str)]) -> Inline {
        let mut map = hashlink::LinkedHashMap::new();
        for (k, v) in kv {
            map.insert(k.to_string(), v.to_string());
        }
        Inline::Link(Link {
            attr: (String::new(), vec![], map),
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: SourceInfo::for_test(),
            })],
            target: (url.to_string(), String::new()),
            source_info: SourceInfo::for_test(),
            attr_source: AttrSourceInfo::empty(),
            target_source: TargetSourceInfo::empty(),
        })
    }

    /// Like [`run`], but with control over the llms view: when
    /// `llms_enabled` the project is a website with
    /// `website.llms-txt: true` in the metadata (the
    /// `llms_view_active` predicate holds).
    async fn run_cfg(
        blocks: Vec<Block>,
        source_doc: &str,
        output_href: &str,
        index_profiles: Vec<DocumentProfile>,
        with_index: bool,
        llms_enabled: bool,
    ) -> (Vec<Block>, Vec<quarto_error_reporting::DiagnosticMessage>) {
        let mut project = make_project();
        if llms_enabled {
            project.config.project_kind = ProjectKind::Website;
        }
        let input_path = format!("/project/{}", source_doc);
        let doc = DocumentInfo::from_path(&input_path);
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        if with_index {
            ctx = ctx.with_project_index(Arc::new(ProjectIndex::new(index_profiles)));
        }
        let page_output = format!("/project/_site/{}", output_href);
        let stem = std::path::Path::new(output_href)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("index");
        ctx.resource_resolver = Some(ResourceResolverContext::website(
            "/project/_site",
            page_output,
            "site_libs",
            stem,
        ));
        let mut ast = Pandoc {
            meta: if llms_enabled {
                llms_meta()
            } else {
                empty_meta()
            },
            blocks,
        };
        LinkRewriteTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        (ast.blocks, ctx.diagnostics)
    }

    fn first_link(blocks: &[Block]) -> &Link {
        for b in blocks {
            if let Block::Paragraph(p) = b {
                for i in &p.content {
                    if let Inline::Link(l) = i {
                        return l;
                    }
                }
            }
        }
        panic!("no link found");
    }

    fn assert_q13_9(diags: &[quarto_error_reporting::DiagnosticMessage], context: &str) {
        assert_eq!(
            diags.len(),
            1,
            "{context}: expected exactly one diagnostic; got {:?}",
            diags.iter().map(|d| d.title.clone()).collect::<Vec<_>>()
        );
        assert_eq!(
            diags[0].code.as_deref(),
            Some("Q-13-9"),
            "{context}: expected Q-13-9; got {:?}",
            diags[0].code
        );
    }

    /// `link-format="llms"` on a source-path link resolves to the
    /// target page's markdown companion, consuming the attribute.
    #[tokio::test]
    async fn link_format_llms_rewrites_to_companion() {
        let blocks = vec![para(vec![link_with_kv(
            "about.qmd",
            "About",
            &[("link-format", "llms")],
        )])];
        let (out, diags) = run_cfg(
            blocks,
            "index.qmd",
            "index.html",
            vec![make_profile("about.qmd", "about.html")],
            true,
            true,
        )
        .await;
        let link = first_link(&out);
        assert_eq!(link.target.0, "about.md");
        assert!(
            !link.attr.2.contains_key("link-format"),
            "attribute must be consumed"
        );
        assert!(diags.is_empty(), "satisfied pin must not diagnose");
    }

    /// The companion rewrite is page-relative and keeps the fragment:
    /// a depth-1 page linking `../about.qmd#sec` gets
    /// `../about.md#sec`.
    #[tokio::test]
    async fn link_format_llms_relativizes_and_keeps_fragment() {
        let blocks = vec![para(vec![link_with_kv(
            "../about.qmd#sec",
            "About",
            &[("link-format", "llms")],
        )])];
        let (out, diags) = run_cfg(
            blocks,
            "guide/intro.qmd",
            "guide/intro.html",
            vec![
                make_profile("about.qmd", "about.html"),
                make_profile("guide/intro.qmd", "guide/intro.html"),
            ],
            true,
            true,
        )
        .await;
        assert_eq!(first_link(&out).target.0, "../about.md#sec");
        assert!(diags.is_empty());
    }

    /// `link-format="llms"` when llms-txt is not enabled: Q-13-9
    /// warning, attribute stripped, normal `.html` resolution.
    #[tokio::test]
    async fn link_format_llms_warns_when_llms_disabled() {
        let blocks = vec![para(vec![link_with_kv(
            "about.qmd",
            "About",
            &[("link-format", "llms")],
        )])];
        let (out, diags) = run_cfg(
            blocks,
            "index.qmd",
            "index.html",
            vec![make_profile("about.qmd", "about.html")],
            true,
            false,
        )
        .await;
        let link = first_link(&out);
        assert_eq!(link.target.0, "about.html", "falls back to html output");
        assert!(!link.attr.2.contains_key("link-format"));
        assert_q13_9(&diags, "llms pin with llms-txt disabled");
    }

    /// `link-format="llms"` targeting a draft (no companion): Q-13-9
    /// warning, fallback to the `.html` output.
    #[tokio::test]
    async fn link_format_llms_draft_target_warns() {
        let draft = DocumentProfile {
            draft: true,
            ..make_profile("about.qmd", "about.html")
        };
        let blocks = vec![para(vec![link_with_kv(
            "about.qmd",
            "About",
            &[("link-format", "llms")],
        )])];
        let (out, diags) =
            run_cfg(blocks, "index.qmd", "index.html", vec![draft], true, true).await;
        let link = first_link(&out);
        assert_eq!(link.target.0, "about.html", "falls back to html output");
        assert!(!link.attr.2.contains_key("link-format"));
        assert_q13_9(&diags, "llms pin on a draft target");
    }

    /// An unknown `link-format` value warns and is stripped; the link
    /// resolves exactly as if undecorated.
    #[tokio::test]
    async fn link_format_unknown_value_warns() {
        let blocks = vec![para(vec![link_with_kv(
            "about.qmd",
            "About",
            &[("link-format", "pdf")],
        )])];
        let (out, diags) = run_cfg(
            blocks,
            "index.qmd",
            "index.html",
            vec![make_profile("about.qmd", "about.html")],
            true,
            true,
        )
        .await;
        let link = first_link(&out);
        assert_eq!(link.target.0, "about.html");
        assert!(!link.attr.2.contains_key("link-format"));
        assert_q13_9(&diags, "unknown link-format value");
    }

    /// With the llms view active, `link-format="html"` survives this
    /// transform — `LlmsCaptureTransform` (which runs later) is its
    /// consumer. The target resolves normally.
    #[tokio::test]
    async fn link_format_html_survives_for_capture_when_llms_active() {
        let blocks = vec![para(vec![link_with_kv(
            "about.qmd",
            "About",
            &[("link-format", "html")],
        )])];
        let (out, diags) = run_cfg(
            blocks,
            "index.qmd",
            "index.html",
            vec![make_profile("about.qmd", "about.html")],
            true,
            true,
        )
        .await;
        let link = first_link(&out);
        assert_eq!(link.target.0, "about.html");
        assert_eq!(
            link.attr.2.get("link-format").map(String::as_str),
            Some("html"),
            "html pin must ride through to LlmsCaptureTransform"
        );
        assert!(diags.is_empty());
    }

    /// With the llms view inactive, `link-format="html"` is stripped
    /// silently (nothing downstream consumes it) and behavior is
    /// exactly the undecorated behavior.
    #[tokio::test]
    async fn link_format_html_stripped_silently_when_llms_inactive() {
        let blocks = vec![para(vec![link_with_kv(
            "about.qmd",
            "About",
            &[("link-format", "html")],
        )])];
        let (out, diags) = run_cfg(
            blocks,
            "index.qmd",
            "index.html",
            vec![make_profile("about.qmd", "about.html")],
            true,
            false,
        )
        .await;
        let link = first_link(&out);
        assert_eq!(link.target.0, "about.html");
        assert!(
            !link.attr.2.contains_key("link-format"),
            "inactive-view html pin must be scrubbed before the writer"
        );
        assert!(diags.is_empty(), "inactive-view html pin is silent");
    }

    /// `link-format` on a target that is not a resolvable project
    /// source path (an external URL) is inconsistent per the design's
    /// source-paths-only rule: Q-13-9, attribute stripped, target
    /// untouched.
    #[tokio::test]
    async fn link_format_html_external_target_warns() {
        let blocks = vec![para(vec![link_with_kv(
            "https://example.com/x",
            "X",
            &[("link-format", "html")],
        )])];
        let (out, diags) = run_cfg(
            blocks,
            "index.qmd",
            "index.html",
            vec![make_profile("about.qmd", "about.html")],
            true,
            true,
        )
        .await;
        let link = first_link(&out);
        assert_eq!(link.target.0, "https://example.com/x");
        assert!(!link.attr.2.contains_key("link-format"));
        assert_q13_9(&diags, "link-format on an external target");
    }

    /// An `llms` pin targeting a revealjs page warns: slide decks
    /// render to `.html` but never get a markdown companion, so the
    /// pin cannot be honored.
    #[tokio::test]
    async fn link_format_llms_to_revealjs_target_warns() {
        let slides = DocumentProfile {
            format_id: "revealjs".to_string(),
            ..make_profile("slides.qmd", "slides.html")
        };
        let blocks = vec![para(vec![link_with_kv(
            "slides.qmd",
            "Slides",
            &[("link-format", "llms")],
        )])];
        let (out, diags) =
            run_cfg(blocks, "index.qmd", "index.html", vec![slides], true, true).await;
        let link = first_link(&out);
        assert_eq!(link.target.0, "slides.html", "falls back to html output");
        assert!(!link.attr.2.contains_key("link-format"));
        assert_q13_9(&diags, "llms pin on a revealjs target");
    }

    /// An `llms` pin is satisfiable *from* any page of the site — a
    /// revealjs deck can link another page's markdown companion. The
    /// gate is config-level (`llms-txt: true` on a website), not the
    /// linking page's own format.
    #[tokio::test]
    async fn link_format_llms_from_revealjs_page_succeeds() {
        let mut project = make_project();
        project.config.project_kind = ProjectKind::Website;
        let doc = DocumentInfo::from_path("/project/slides.qmd");
        let mut format = Format::html();
        format.target_format = "revealjs".to_string();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        ctx = ctx.with_project_index(Arc::new(ProjectIndex::new(vec![make_profile(
            "about.qmd",
            "about.html",
        )])));
        ctx.resource_resolver = Some(ResourceResolverContext::website(
            "/project/_site",
            "/project/_site/slides.html",
            "site_libs",
            "slides",
        ));
        let mut ast = Pandoc {
            meta: llms_meta(),
            blocks: vec![para(vec![link_with_kv(
                "about.qmd",
                "About notes",
                &[("link-format", "llms")],
            )])],
        };
        LinkRewriteTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        let link = first_link(&ast.blocks);
        assert_eq!(
            link.target.0, "about.md",
            "the deck links the html page's companion"
        );
        assert!(!link.attr.2.contains_key("link-format"));
        assert!(
            ctx.diagnostics.is_empty(),
            "satisfiable cross-format pin must not diagnose; got {:?}",
            ctx.diagnostics
                .iter()
                .map(|d| d.title.clone())
                .collect::<Vec<_>>()
        );
    }

    /// Even with neither index nor resolver (bare standalone render),
    /// the transform must still walk the AST to consume `link-format`
    /// attributes — an unsatisfiable `llms` pin warns, and nothing
    /// leaks into the writer.
    #[tokio::test]
    async fn link_format_consumed_without_index_and_resolver() {
        let blocks = vec![para(vec![link_with_kv(
            "about.qmd",
            "About",
            &[("link-format", "llms")],
        )])];
        let project = make_project();
        let doc = DocumentInfo::from_path("/project/index.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        let mut ast = Pandoc {
            meta: empty_meta(),
            blocks,
        };
        LinkRewriteTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        let link = first_link(&ast.blocks);
        assert_eq!(link.target.0, "about.qmd", "no resolution without index");
        assert!(
            !link.attr.2.contains_key("link-format"),
            "attribute must be consumed even on the no-index path"
        );
        assert_q13_9(&ctx.diagnostics, "llms pin in a standalone render");
    }

    /// Images rewrite even without a `ProjectIndex` — static-resource
    /// resolution needs only the resolver. Links still require the
    /// index and pass through (phase-6 Decision 7 unchanged).
    #[tokio::test]
    async fn image_rewrite_runs_without_index_links_untouched() {
        let blocks = vec![
            para(vec![image_inline("/images/x.svg", "x")]),
            para(vec![link_inline("about.qmd", "About")]),
        ];
        let (out, diags) = run(
            blocks,
            "deep/deeper/index.qmd",
            "deep/deeper/index.html",
            vec![],
            false,
        )
        .await;
        assert_eq!(image_urls(&out), vec!["../../images/x.svg"]);
        assert_eq!(first_link_url(&out), "about.qmd");
        assert!(diags.is_empty());
    }
}
