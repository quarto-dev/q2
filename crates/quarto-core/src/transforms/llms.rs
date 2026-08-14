/*
 * llms.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * llms markdown capture (bd-llms-txt-unimplemented-oih6z6j7).
 */

//! Per-page markdown capture for `website.llms-txt`.
//!
//! When a website project sets `llms-txt: true`, every non-draft,
//! non-404 HTML page gets a markdown companion (`<page>.md`) written
//! next to its HTML output, plus a site-level `llms.txt` index and
//! `llms-full.txt` concatenation. This module owns the *capture*
//! half: [`LlmsCaptureTransform`] runs at the end of the
//! Finalization phase — after `CrossrefRenderTransform`, so figure /
//! table numbers and `@ref` text are resolved — clones the page AST,
//! reduces the clone to a clean markdown view, serializes it with
//! pampa's qmd writer, and deposits the string as a **path-less,
//! Project-scoped artifact** under the `llms-md/` key prefix.
//!
//! Path-less artifacts are skipped by every flusher
//! (`artifact_flush.rs` contract), so nothing reaches disk here.
//! `WebsiteProjectType::post_render` picks the strings up from the
//! project artifact store, checks the output ledger for collisions
//! (Q-5-28), and performs all writes in one place — see
//! `project::website_post_render`. The WASM preview never runs the
//! native post-render hooks, so captures are inert there (same
//! policy as sitemap.xml).
//!
//! ## The llms view
//!
//! The clone-side cleanup ([`build_llms_view`]):
//!
//! - resolves the conditional-content markers
//!   ([`LLMS_KEEP_CLASS`] kept & unwrapped, [`LLMS_OMIT_CLASS`]
//!   dropped) that `ConditionalContentTransform` leaves behind when
//!   the llms view is active — see that module for the four-quadrant
//!   `when-format="llms"` semantics;
//! - unwraps `.content-visible` / `.content-hidden` wrappers that
//!   survive in both views (the HTML keeps the wrapper div; markdown
//!   doesn't need the noise);
//! - unwraps sectionize `section` divs, restoring the section id
//!   onto the heading so `#fragment` anchors keep a target;
//! - drops raw HTML blocks/inlines (they are presentation for the
//!   HTML view; markdown consumers get clean text);
//! - retargets same-site `.html` links to their `.md` siblings when
//!   the target page has a companion (non-draft, non-404 pages in
//!   the project index), keeping fragments. Links to drafts, the
//!   404 page, external URLs, and non-page resources are untouched.
//!
//! The main-AST cleanup ([`resolve_marker_classes`]) then removes
//! `.quarto-llms-keep` subtrees (they were retained solely for the
//! capture) and strips the `.quarto-llms-omit` marker class from
//! nodes that stay in the HTML. This cleanup runs even when the
//! page itself is skipped (draft / 404): the markers must never
//! reach the HTML writer.

use std::path::Path;

use quarto_pandoc_types::pandoc::Pandoc;
use quarto_pandoc_types::{Attr, AttrSourceInfo, Block, ConfigValue, Inline};

use crate::artifact::{Artifact, ArtifactScope};
use crate::project::ProjectKind;
use crate::project::index::ProjectIndex;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};
use crate::transforms::navigation_active::page_relative_source;

/// Marker class for content visible in the HTML view but hidden in
/// the llms view. Stays in the HTML (minus the marker); dropped from
/// the companion.
pub const LLMS_OMIT_CLASS: &str = "quarto-llms-omit";

/// Marker class for content hidden in the HTML view but visible in
/// the llms view. Dropped from the HTML; unwrapped into the
/// companion.
pub const LLMS_KEEP_CLASS: &str = "quarto-llms-keep";

/// The format token `when-format` / `unless-format` matches for the
/// llms view. Exact match only — `when-format="markdown"` does *not*
/// select the llms view (the view is "what an LLM should read", not
/// a markdown render target).
pub const LLMS_FORMAT: &str = "llms";

/// Artifact-store key prefix for captured companions. The full key
/// is `llms-md/<output_href with .html → .md>`.
pub const LLMS_ARTIFACT_PREFIX: &str = "llms-md/";

/// Output href of the conventional 404 page, excluded from the
/// companion set and the index.
pub const HREF_404: &str = "404.html";

/// Is the llms view active for this render?
///
/// True only when all of: the project is a website, the target is
/// the plain HTML format family member `html` (slides and other
/// derived formats don't get companions), and `website.llms-txt` is
/// `true` in the merged metadata.
///
/// **Contract:** `ConditionalContentTransform` and
/// [`LlmsCaptureTransform`] must agree on this predicate — the
/// first only plants marker classes when it is true, and the second
/// is the only thing that cleans them up.
pub fn llms_view_active(meta: &ConfigValue, ctx: &RenderContext) -> bool {
    ctx.project.config.project_kind == ProjectKind::Website
        && crate::format::lua_format_for(&ctx.format.target_format) == "html"
        && crate::project::website_config::website_llms_txt_enabled(meta)
}

/// Markdown-companion href for a page's output href: `about.html` →
/// `about.md`. `None` when the output is not an `.html` page.
pub fn companion_href(output_href: &str) -> Option<String> {
    output_href
        .strip_suffix(".html")
        .map(|stem| format!("{stem}.md"))
}

/// Does this profile get a companion? Non-draft, non-404, `.html`
/// output. The single answer shared by capture (link retargeting)
/// and post-render (index assembly) so they can't drift.
pub fn profile_has_companion(profile: &crate::document_profile::DocumentProfile) -> bool {
    !profile.draft && profile.output_href != HREF_404 && profile.output_href.ends_with(".html")
}

/// See the module docs. Registered at the tail of the Finalization
/// phase in `build_transform_pipeline`.
pub struct LlmsCaptureTransform;

impl LlmsCaptureTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LlmsCaptureTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for LlmsCaptureTransform {
    fn name(&self) -> &str {
        "llms-capture"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Finalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> crate::Result<()> {
        if !llms_view_active(&ast.meta, ctx) {
            // ConditionalContentTransform used the same predicate, so
            // no markers exist and there is nothing to capture.
            return Ok(());
        }

        let index = ctx.project_index.clone();
        let capture = capture_target(index.as_deref(), ctx);

        if let Some((md_href, cur_dir)) = capture {
            let mut view = build_llms_view(
                ast.blocks.clone(),
                &ViewContext {
                    index: index.as_deref(),
                    cur_dir,
                },
            );
            // The HTML `<h1>` title comes from the template, not the
            // AST (`TitleBlockTransform` skips full-template mode),
            // so the markdown view synthesizes its own.
            let has_h1 = view
                .iter()
                .any(|b| matches!(b, Block::Header(h) if h.level == 1));
            if !has_h1
                && let Some(title) =
                    crate::transforms::title_block::extract_title_inlines(&ast.meta)
            {
                view.insert(
                    0,
                    crate::transforms::title_block::create_title_header(title),
                );
            }
            // The qmd writer wants a map for the (empty) metadata.
            let doc = Pandoc {
                meta: ConfigValue::new_map(
                    vec![],
                    quarto_source_map::SourceInfo::generated(quarto_source_map::By::unknown()),
                ),
                blocks: view,
            };
            let mut buf: Vec<u8> = Vec::new();
            match pampa::writers::qmd::write(&doc, &mut buf) {
                Ok(()) => {
                    let markdown = String::from_utf8_lossy(&buf).into_owned();
                    ctx.artifacts.store(
                        format!("{LLMS_ARTIFACT_PREFIX}{md_href}"),
                        Artifact::from_string(markdown, "text/markdown")
                            .with_scope(ArtifactScope::Project),
                    );
                }
                Err(diags) => {
                    // A page whose view can't serialize loses its
                    // companion, not its render. Say so.
                    ctx.diagnostics.push(
                        quarto_error_reporting::DiagnosticMessageBuilder::warning(format!(
                            "Could not produce the markdown companion `{md_href}`"
                        ))
                        .problem(
                            "The page rendered normally, but its llms markdown \
                             serialization failed; the companion is skipped.",
                        )
                        .build(),
                    );
                    ctx.diagnostics.extend(diags);
                }
            }
        }

        // Always resolve the conditional-content markers on the main
        // AST — even for skipped (draft / 404) pages — so they never
        // reach the HTML writer.
        resolve_marker_classes(&mut ast.blocks);
        Ok(())
    }
}

/// Decide whether the current page gets a companion; returns its
/// companion href and the directory prefix of the page (for
/// resolving page-relative links), or `None` to skip capture.
fn capture_target(index: Option<&ProjectIndex>, ctx: &RenderContext) -> Option<(String, String)> {
    let index = index?;
    let source = page_relative_source(ctx);
    let profile = index.lookup_by_source(Path::new(&source))?;
    if !profile_has_companion(profile) {
        return None;
    }
    let md_href = companion_href(&profile.output_href)?;
    let cur_dir = match profile.output_href.rfind('/') {
        Some(idx) => profile.output_href[..idx].to_string(),
        None => String::new(),
    };
    Some((md_href, cur_dir))
}

// ═══════════════════════════════════════════════════════════════════
// The llms view (clone-side cleanup)
// ═══════════════════════════════════════════════════════════════════

struct ViewContext<'a> {
    index: Option<&'a ProjectIndex>,
    /// Directory prefix of the current page's output href
    /// (forward-slash, no trailing slash; `""` at the site root).
    cur_dir: String,
}

fn has_class(attr: &Attr, class: &str) -> bool {
    attr.1.iter().any(|c| c == class)
}

/// Classes whose wrapper is noise in the markdown view: the
/// conditional-content markers and survivors, sectionize's section
/// wrappers, and the HTML presentation scaffolding that late
/// transforms (crossref-render, code-block-render, footnotes,
/// appendix) build for the HTML writer.
fn is_unwrap_div(attr: &Attr) -> bool {
    // Anonymous wrappers (no classes) only ever carry presentation
    // attributes at this point in the pipeline (e.g. the
    // `aria-describedby` float inner wrapper) — the *user's*
    // attribute-less fenced divs were already plain and unwrapping
    // them loses nothing but the wrapper.
    attr.1.is_empty()
        || has_class(attr, LLMS_KEEP_CLASS)
        || has_class(attr, "content-visible")
        || has_class(attr, "content-hidden")
        || has_class(attr, "section")
        || has_class(attr, "code-copy-outer-scaffold")
        || has_class(attr, "quarto-figure")
        || attr.0 == "quarto-appendix"
}

/// The resolved-float wrapper (`crossref-render` output). Unwrapped
/// like chrome, but its id is the crossref anchor and must survive —
/// see `clean_block_into`.
fn is_float_div(attr: &Attr) -> bool {
    has_class(attr, "quarto-float")
}

/// Strip HTML-presentation attributes from a kept element:
/// quarto-internal and styling classes, `data-*` / `aria-*` /
/// `role` attributes. Ids and user classes survive.
fn sanitize_attr(attr: &mut Attr, attr_source: &mut AttrSourceInfo) {
    let class_noise = |c: &str| {
        c.starts_with("quarto-")
            || matches!(c, "anchored" | "code-with-copy" | "cell" | "callout-titled")
    };
    if attr.1.len() == attr_source.classes.len() {
        let keep: Vec<bool> = attr.1.iter().map(|c| !class_noise(c)).collect();
        let mut it = keep.iter();
        attr_source.classes.retain(|_| *it.next().unwrap());
    } else if attr.1.iter().any(|c| class_noise(c)) {
        attr_source.classes.clear();
    }
    attr.1.retain(|c| !class_noise(c));

    let kv_noise = |k: &str| {
        k.starts_with("data-") || k.starts_with("aria-") || matches!(k, "role" | "tabindex")
    };
    if attr.2.len() == attr_source.attributes.len() {
        let keep: Vec<bool> = attr.2.keys().map(|k| !kv_noise(k)).collect();
        let mut it = keep.iter();
        attr_source.attributes.retain(|_| *it.next().unwrap());
    } else if attr.2.keys().any(|k| kv_noise(k)) {
        attr_source.attributes.clear();
    }
    attr.2.retain(|k, _| !kv_noise(k));
}

fn build_llms_view(blocks: Vec<Block>, cx: &ViewContext) -> Vec<Block> {
    let mut out = Vec::with_capacity(blocks.len());
    for block in blocks {
        clean_block_into(block, cx, &mut out);
    }
    out
}

fn clean_block_into(mut block: Block, cx: &ViewContext, out: &mut Vec<Block>) {
    match block {
        Block::Div(div) => {
            if has_class(&div.attr, LLMS_OMIT_CLASS) {
                return;
            }
            if has_class(&div.attr, "callout") {
                out.extend(rebuild_callout(div, cx));
                return;
            }
            if is_float_div(&div.attr) {
                // Keep exactly one minimal `::: {#id}` wrapper per
                // resolved float — the id is the crossref anchor
                // (`[Table 1](#tbl-nums)` needs a target) — and
                // unwrap all nested float chrome.
                let id = div.attr.0.clone();
                let children = build_llms_view(div.content, cx);
                if id.is_empty() {
                    out.extend(children);
                } else {
                    out.push(Block::Div(quarto_pandoc_types::block::Div {
                        attr: (id, vec![], hashlink::LinkedHashMap::new()),
                        content: children,
                        source_info: div.source_info,
                        attr_source: AttrSourceInfo::empty(),
                    }));
                }
                return;
            }
            if is_unwrap_div(&div.attr) {
                let mut children = build_llms_view(div.content, cx);
                // Sectionize moved the heading's id onto the section
                // div; restore it so `#fragment` links keep a target.
                if !div.attr.0.is_empty()
                    && let Some(Block::Header(h)) = children.first_mut()
                    && h.attr.0.is_empty()
                {
                    h.attr.0 = div.attr.0.clone();
                }
                out.extend(children);
                return;
            }
            let mut div = div;
            sanitize_attr(&mut div.attr, &mut div.attr_source);
            div.content = build_llms_view(div.content, cx);
            out.push(Block::Div(div));
        }
        Block::RawBlock(raw) => {
            if raw_format_survives(&raw.format) {
                out.push(Block::RawBlock(raw));
            }
        }
        Block::CodeBlock(mut cb) => {
            sanitize_attr(&mut cb.attr, &mut cb.attr_source);
            out.push(Block::CodeBlock(cb));
        }
        Block::Header(mut h) => {
            sanitize_attr(&mut h.attr, &mut h.attr_source);
            clean_inlines(&mut h.content, cx);
            out.push(Block::Header(h));
        }
        Block::Figure(mut fig) => {
            if is_float_div(&fig.attr) {
                // crossref-render's float DOM uses a Figure as the
                // inner chrome layer (the outer `#id` div already
                // carries the anchor). Unwrap to content + caption.
                out.extend(build_llms_view(std::mem::take(&mut fig.content), cx));
                if let Some(long) = fig.caption.long.take() {
                    out.extend(build_llms_view(long, cx));
                }
                return;
            }
            sanitize_attr(&mut fig.attr, &mut fig.attr_source);
            fig.content = build_llms_view(std::mem::take(&mut fig.content), cx);
            if let Some(short) = &mut fig.caption.short {
                clean_inlines(short, cx);
            }
            if let Some(long) = &mut fig.caption.long {
                *long = build_llms_view(std::mem::take(long), cx);
            }
            out.push(Block::Figure(fig));
        }
        _ => {
            clean_block_children(&mut block, cx);
            out.push(block);
        }
    }
}

/// Rebuild a resolved callout (the HTML scaffold `callout-resolve`
/// produced) back into the author-side form the qmd writer can emit
/// legibly: `::: {.callout-note}` with an optional `## title` and
/// the body blocks.
fn rebuild_callout(div: quarto_pandoc_types::block::Div, cx: &ViewContext) -> Vec<Block> {
    const CALLOUT_TYPES: [&str; 5] = ["note", "tip", "warning", "caution", "important"];
    let callout_type = div
        .attr
        .1
        .iter()
        .find_map(|c| {
            c.strip_prefix("callout-")
                .filter(|t| CALLOUT_TYPES.contains(t))
        })
        .map(str::to_string);

    // Walk the scaffold for the title and body containers.
    fn find_container<'a>(
        blocks: &'a [Block],
        class: &str,
    ) -> Option<&'a quarto_pandoc_types::block::Div> {
        for block in blocks {
            if let Block::Div(d) = block {
                if has_class(&d.attr, class) {
                    return Some(d);
                }
                if let Some(found) = find_container(&d.content, class) {
                    return Some(found);
                }
            }
        }
        None
    }
    let title_text = find_container(&div.content, "callout-title-container")
        .map(|d| plain_text_of_blocks(&d.content));
    let body = find_container(&div.content, "callout-body-container")
        .map_or_else(|| div.content.clone(), |d| d.content.clone());

    let mut content: Vec<Block> = Vec::new();
    if let (Some(title), Some(ty)) = (title_text.as_deref(), callout_type.as_deref()) {
        // The default title is just the capitalized type name
        // ("Note"); only an author-supplied title is worth a heading.
        let is_default = title.trim().eq_ignore_ascii_case(ty);
        if !is_default && !title.trim().is_empty() {
            content.push(crate::transforms::title_block::create_title_header(vec![
                Inline::Str(quarto_pandoc_types::inline::Str {
                    text: title.trim().to_string(),
                    source_info: quarto_source_map::SourceInfo::generated(
                        quarto_source_map::By::unknown(),
                    ),
                }),
            ]));
            if let Some(Block::Header(h)) = content.last_mut() {
                h.level = 2;
            }
        }
    }
    content.extend(build_llms_view(body, cx));

    let class = callout_type.map_or_else(|| "callout".to_string(), |t| format!("callout-{t}"));
    vec![Block::Div(quarto_pandoc_types::block::Div {
        attr: (String::new(), vec![class], hashlink::LinkedHashMap::new()),
        content,
        source_info: div.source_info,
        attr_source: AttrSourceInfo::empty(),
    })]
}

/// Flatten the plain text of a block list (for callout titles).
fn plain_text_of_blocks(blocks: &[Block]) -> String {
    fn walk_inlines(inlines: &[Inline], out: &mut String) {
        for inline in inlines {
            match inline {
                Inline::Str(s) => out.push_str(&s.text),
                Inline::Space(_) => out.push(' '),
                Inline::Emph(i) => walk_inlines(&i.content, out),
                Inline::Strong(i) => walk_inlines(&i.content, out),
                Inline::Span(i) => walk_inlines(&i.content, out),
                Inline::Code(c) => out.push_str(&c.text),
                _ => {}
            }
        }
    }
    let mut out = String::new();
    for block in blocks {
        match block {
            Block::Plain(b) => walk_inlines(&b.content, &mut out),
            Block::Paragraph(b) => walk_inlines(&b.content, &mut out),
            Block::Header(h) => walk_inlines(&h.content, &mut out),
            Block::Div(d) => out.push_str(&plain_text_of_blocks(&d.content)),
            _ => {}
        }
    }
    out
}

/// Recurse into a non-Div block's child blocks/inlines.
fn clean_block_children(block: &mut Block, cx: &ViewContext) {
    match block {
        Block::Plain(b) => clean_inlines(&mut b.content, cx),
        Block::Paragraph(b) => clean_inlines(&mut b.content, cx),
        Block::Header(h) => clean_inlines(&mut h.content, cx),
        Block::LineBlock(lb) => {
            for line in &mut lb.content {
                clean_inlines(line, cx);
            }
        }
        Block::BlockQuote(bq) => bq.content = build_llms_view(std::mem::take(&mut bq.content), cx),
        Block::OrderedList(ol) => {
            for item in &mut ol.content {
                *item = build_llms_view(std::mem::take(item), cx);
            }
        }
        Block::BulletList(bl) => {
            for item in &mut bl.content {
                *item = build_llms_view(std::mem::take(item), cx);
            }
        }
        Block::DefinitionList(dl) => {
            for (term, defs) in &mut dl.content {
                clean_inlines(term, cx);
                for def in defs {
                    *def = build_llms_view(std::mem::take(def), cx);
                }
            }
        }
        Block::Figure(fig) => {
            fig.content = build_llms_view(std::mem::take(&mut fig.content), cx);
            if let Some(short) = &mut fig.caption.short {
                clean_inlines(short, cx);
            }
            if let Some(long) = &mut fig.caption.long {
                *long = build_llms_view(std::mem::take(long), cx);
            }
        }
        Block::Table(table) => {
            if let Some(short) = &mut table.caption.short {
                clean_inlines(short, cx);
            }
            if let Some(long) = &mut table.caption.long {
                *long = build_llms_view(std::mem::take(long), cx);
            }
            for row in &mut table.head.rows {
                for cell in &mut row.cells {
                    cell.content = build_llms_view(std::mem::take(&mut cell.content), cx);
                }
            }
            for body in &mut table.bodies {
                for row in &mut body.body {
                    for cell in &mut row.cells {
                        cell.content = build_llms_view(std::mem::take(&mut cell.content), cx);
                    }
                }
            }
            for row in &mut table.foot.rows {
                for cell in &mut row.cells {
                    cell.content = build_llms_view(std::mem::take(&mut cell.content), cx);
                }
            }
        }
        Block::Custom(custom) => {
            for (_name, slot) in &mut custom.slots {
                use quarto_pandoc_types::custom::Slot;
                match slot {
                    Slot::Block(b) => clean_block_children(b, cx),
                    Slot::Blocks(bs) => *bs = build_llms_view(std::mem::take(bs), cx),
                    Slot::Inline(_) => {}
                    Slot::Inlines(is) => clean_inlines(is, cx),
                }
            }
        }
        _ => {}
    }
}

fn clean_inlines(inlines: &mut Vec<Inline>, cx: &ViewContext) {
    let mut out = Vec::with_capacity(inlines.len());
    for inline in std::mem::take(inlines) {
        clean_inline_into(inline, cx, &mut out);
    }
    *inlines = out;
}

fn clean_inline_into(mut inline: Inline, cx: &ViewContext, out: &mut Vec<Inline>) {
    match inline {
        Inline::Span(span) => {
            if has_class(&span.attr, LLMS_OMIT_CLASS) {
                return;
            }
            // Anonymous spans (footnote-ref anchors and other
            // presentation wrappers) unwrap along with the marker /
            // conditional classes.
            if span.attr.1.is_empty() || is_unwrap_div(&span.attr) {
                let mut children = span.content;
                clean_inlines(&mut children, cx);
                out.extend(children);
                return;
            }
            let mut span = span;
            sanitize_attr(&mut span.attr, &mut span.attr_source);
            clean_inlines(&mut span.content, cx);
            out.push(Inline::Span(span));
        }
        Inline::RawInline(raw) => {
            if raw_format_survives(&raw.format) {
                out.push(Inline::RawInline(raw));
            }
        }
        Inline::Link(mut link) => {
            // Footnote plumbing (`FootnotesTransform` output): the
            // back-link is pure HTML navigation — drop it; the ref
            // link keeps just its visible number.
            if has_class(&link.attr, "footnote-back") {
                return;
            }
            if has_class(&link.attr, "footnote-ref") {
                let mut children = link.content;
                clean_inlines(&mut children, cx);
                out.extend(children);
                return;
            }
            sanitize_attr(&mut link.attr, &mut link.attr_source);
            clean_inlines(&mut link.content, cx);
            link.target.0 = retarget_href(&link.target.0, cx);
            out.push(Inline::Link(link));
        }
        Inline::Image(mut image) => {
            sanitize_attr(&mut image.attr, &mut image.attr_source);
            clean_inlines(&mut image.content, cx);
            out.push(Inline::Image(image));
        }
        _ => {
            clean_inline_children(&mut inline, cx);
            out.push(inline);
        }
    }
}

fn clean_inline_children(inline: &mut Inline, cx: &ViewContext) {
    match inline {
        Inline::Emph(i) => clean_inlines(&mut i.content, cx),
        Inline::Underline(i) => clean_inlines(&mut i.content, cx),
        Inline::Strong(i) => clean_inlines(&mut i.content, cx),
        Inline::Strikeout(i) => clean_inlines(&mut i.content, cx),
        Inline::Superscript(i) => clean_inlines(&mut i.content, cx),
        Inline::Subscript(i) => clean_inlines(&mut i.content, cx),
        Inline::SmallCaps(i) => clean_inlines(&mut i.content, cx),
        Inline::Quoted(i) => clean_inlines(&mut i.content, cx),
        Inline::Cite(i) => clean_inlines(&mut i.content, cx),
        Inline::Image(i) => clean_inlines(&mut i.content, cx),
        Inline::Note(note) => note.content = build_llms_view(std::mem::take(&mut note.content), cx),
        _ => {}
    }
}

/// Raw content that survives into the markdown view: markdown
/// itself. HTML (and every other format's) raw payload is
/// presentation for that format.
fn raw_format_survives(format: &str) -> bool {
    matches!(format, "markdown" | "md" | "qmd")
}

// ═══════════════════════════════════════════════════════════════════
// Link retargeting
// ═══════════════════════════════════════════════════════════════════

/// Rewrite a same-site `.html` link to its `.md` companion when the
/// target page has one. Everything else passes through verbatim.
fn retarget_href(href: &str, cx: &ViewContext) -> String {
    let Some(index) = cx.index else {
        return href.to_string();
    };
    // External / non-path targets pass through.
    if href.is_empty()
        || href.starts_with('#')
        || href.starts_with("//")
        || href.contains("://")
        || href.starts_with("mailto:")
        || href.starts_with("data:")
    {
        return href.to_string();
    }
    let (path, fragment) = match href.split_once('#') {
        Some((p, f)) => (p, Some(f)),
        None => (href, None),
    };
    if !path.ends_with(".html") {
        return href.to_string();
    }
    // Site-root-relative (`/a/b.html`) resolves from ""; page-relative
    // resolves from the current page's directory.
    let resolved = if let Some(rooted) = path.strip_prefix('/') {
        normalize_href(rooted)
    } else if cx.cur_dir.is_empty() {
        normalize_href(path)
    } else {
        normalize_href(&format!("{}/{}", cx.cur_dir, path))
    };
    let Some(resolved) = resolved else {
        return href.to_string();
    };
    let eligible = index
        .lookup_by_href(&resolved)
        .is_some_and(profile_has_companion);
    if !eligible {
        return href.to_string();
    }
    // Rewrite the extension in the *original* spelling so the link
    // stays relative exactly as written.
    let retargeted = format!("{}.md", &path[..path.len() - ".html".len()]);
    match fragment {
        Some(f) => format!("{retargeted}#{f}"),
        None => retargeted,
    }
}

/// Resolve `.` / `..` segments in a forward-slash href. `None` when
/// the path escapes the site root.
fn normalize_href(path: &str) -> Option<String> {
    let mut segments: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            s => segments.push(s),
        }
    }
    Some(segments.join("/"))
}

// ═══════════════════════════════════════════════════════════════════
// Main-AST marker resolution
// ═══════════════════════════════════════════════════════════════════

/// Remove `.quarto-llms-keep` subtrees and strip the
/// `.quarto-llms-omit` marker class, in the whole tree. The HTML
/// view must never see either marker.
pub fn resolve_marker_classes(blocks: &mut Vec<Block>) {
    blocks.retain_mut(keep_in_html);
}

fn keep_in_html(block: &mut Block) -> bool {
    match block {
        Block::Div(div) => {
            if has_class(&div.attr, LLMS_KEEP_CLASS) {
                return false;
            }
            strip_class(&mut div.attr, &mut div.attr_source, LLMS_OMIT_CLASS);
            resolve_marker_classes(&mut div.content);
        }
        Block::CodeBlock(cb) => {
            if has_class(&cb.attr, LLMS_KEEP_CLASS) {
                return false;
            }
            strip_class(&mut cb.attr, &mut cb.attr_source, LLMS_OMIT_CLASS);
        }
        Block::Plain(b) => resolve_marker_inlines(&mut b.content),
        Block::Paragraph(b) => resolve_marker_inlines(&mut b.content),
        Block::Header(h) => resolve_marker_inlines(&mut h.content),
        Block::LineBlock(lb) => {
            for line in &mut lb.content {
                resolve_marker_inlines(line);
            }
        }
        Block::BlockQuote(bq) => resolve_marker_classes(&mut bq.content),
        Block::OrderedList(ol) => {
            for item in &mut ol.content {
                resolve_marker_classes(item);
            }
        }
        Block::BulletList(bl) => {
            for item in &mut bl.content {
                resolve_marker_classes(item);
            }
        }
        Block::DefinitionList(dl) => {
            for (term, defs) in &mut dl.content {
                resolve_marker_inlines(term);
                for def in defs {
                    resolve_marker_classes(def);
                }
            }
        }
        Block::Figure(fig) => {
            resolve_marker_classes(&mut fig.content);
            if let Some(short) = &mut fig.caption.short {
                resolve_marker_inlines(short);
            }
            if let Some(long) = &mut fig.caption.long {
                resolve_marker_classes(long);
            }
        }
        Block::Table(table) => {
            if let Some(short) = &mut table.caption.short {
                resolve_marker_inlines(short);
            }
            if let Some(long) = &mut table.caption.long {
                resolve_marker_classes(long);
            }
            for row in &mut table.head.rows {
                for cell in &mut row.cells {
                    resolve_marker_classes(&mut cell.content);
                }
            }
            for body in &mut table.bodies {
                for row in &mut body.body {
                    for cell in &mut row.cells {
                        resolve_marker_classes(&mut cell.content);
                    }
                }
            }
            for row in &mut table.foot.rows {
                for cell in &mut row.cells {
                    resolve_marker_classes(&mut cell.content);
                }
            }
        }
        Block::Custom(custom) => {
            for (_name, slot) in &mut custom.slots {
                use quarto_pandoc_types::custom::Slot;
                match slot {
                    Slot::Block(b) => {
                        let _ = keep_in_html(b);
                    }
                    Slot::Blocks(bs) => resolve_marker_classes(bs),
                    Slot::Inline(i) => {
                        let _ = keep_inline_in_html(i);
                    }
                    Slot::Inlines(is) => resolve_marker_inlines(is),
                }
            }
        }
        _ => {}
    }
    true
}

fn resolve_marker_inlines(inlines: &mut Vec<Inline>) {
    inlines.retain_mut(keep_inline_in_html);
}

fn keep_inline_in_html(inline: &mut Inline) -> bool {
    match inline {
        Inline::Span(span) => {
            if has_class(&span.attr, LLMS_KEEP_CLASS) {
                return false;
            }
            strip_class(&mut span.attr, &mut span.attr_source, LLMS_OMIT_CLASS);
            resolve_marker_inlines(&mut span.content);
        }
        Inline::Emph(i) => resolve_marker_inlines(&mut i.content),
        Inline::Underline(i) => resolve_marker_inlines(&mut i.content),
        Inline::Strong(i) => resolve_marker_inlines(&mut i.content),
        Inline::Strikeout(i) => resolve_marker_inlines(&mut i.content),
        Inline::Superscript(i) => resolve_marker_inlines(&mut i.content),
        Inline::Subscript(i) => resolve_marker_inlines(&mut i.content),
        Inline::SmallCaps(i) => resolve_marker_inlines(&mut i.content),
        Inline::Quoted(i) => resolve_marker_inlines(&mut i.content),
        Inline::Cite(i) => resolve_marker_inlines(&mut i.content),
        Inline::Link(i) => resolve_marker_inlines(&mut i.content),
        Inline::Image(i) => resolve_marker_inlines(&mut i.content),
        Inline::Note(note) => resolve_marker_classes(&mut note.content),
        _ => {}
    }
    true
}

/// Remove one class, keeping the parallel `AttrSourceInfo.classes`
/// aligned (same discipline as
/// `conditional_content::strip_condition_attrs`).
fn strip_class(attr: &mut Attr, attr_source: &mut AttrSourceInfo, class: &str) {
    if attr.1.len() == attr_source.classes.len() {
        let keep: Vec<bool> = attr.1.iter().map(|c| c != class).collect();
        let mut it = keep.iter();
        attr_source.classes.retain(|_| *it.next().unwrap());
    } else if attr.1.iter().any(|c| c == class) {
        attr_source.classes.clear();
    }
    attr.1.retain(|c| c != class);
}

/// Add a marker class, keeping `AttrSourceInfo.classes` aligned.
/// Used by `ConditionalContentTransform` when planting the llms
/// markers.
pub(crate) fn add_marker_class(attr: &mut Attr, attr_source: &mut AttrSourceInfo, class: &str) {
    if attr.1.len() == attr_source.classes.len() {
        attr_source.classes.push(None);
    }
    attr.1.push(class.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_profile::DocumentProfile;
    use crate::project::index::ProjectIndex;

    #[test]
    fn companion_href_maps_html_pages_only() {
        assert_eq!(companion_href("about.html"), Some("about.md".to_string()));
        assert_eq!(
            companion_href("guide/intro.html"),
            Some("guide/intro.md".to_string())
        );
        assert_eq!(companion_href("feed.xml"), None);
    }

    #[test]
    fn normalize_href_resolves_dots() {
        assert_eq!(normalize_href("a/b.html"), Some("a/b.html".to_string()));
        assert_eq!(normalize_href("a/../b.html"), Some("b.html".to_string()));
        assert_eq!(normalize_href("./b.html"), Some("b.html".to_string()));
        assert_eq!(normalize_href("../escape.html"), None);
    }

    fn profile(href: &str, draft: bool) -> DocumentProfile {
        DocumentProfile {
            source_path: std::path::PathBuf::from(href.replace(".html", ".qmd")),
            output_href: href.to_string(),
            draft,
            ..Default::default()
        }
    }

    #[test]
    fn retarget_rewrites_eligible_links_only() {
        let index = ProjectIndex::new(vec![
            profile("about.html", false),
            profile("guide/intro.html", false),
            profile("secret.html", true),
            profile("404.html", false),
        ]);
        let cx = ViewContext {
            index: Some(&index),
            cur_dir: String::new(),
        };
        // Eligible same-site link, with and without fragment.
        assert_eq!(retarget_href("about.html", &cx), "about.md");
        assert_eq!(retarget_href("about.html#sec", &cx), "about.md#sec");
        // Root-relative spelling resolves but keeps its spelling.
        assert_eq!(retarget_href("/about.html", &cx), "/about.md");
        // Draft and 404 targets keep their .html.
        assert_eq!(retarget_href("secret.html", &cx), "secret.html");
        assert_eq!(retarget_href("404.html", &cx), "404.html");
        // External, anchor, and non-page targets untouched.
        assert_eq!(
            retarget_href("https://example.com/x.html", &cx),
            "https://example.com/x.html"
        );
        assert_eq!(retarget_href("#local", &cx), "#local");
        assert_eq!(retarget_href("data.csv", &cx), "data.csv");
        // Unknown page keeps .html.
        assert_eq!(retarget_href("missing.html", &cx), "missing.html");

        // Page-relative resolution from a subdirectory.
        let cx_sub = ViewContext {
            index: Some(&index),
            cur_dir: "guide".to_string(),
        };
        assert_eq!(retarget_href("../about.html", &cx_sub), "../about.md");
        assert_eq!(retarget_href("intro.html", &cx_sub), "intro.md");
    }

    #[test]
    fn profile_has_companion_excludes_drafts_and_404() {
        assert!(profile_has_companion(&profile("about.html", false)));
        assert!(!profile_has_companion(&profile("about.html", true)));
        assert!(!profile_has_companion(&profile("404.html", false)));
        assert!(!profile_has_companion(&profile("feed.xml", false)));
    }
}
