/*
 * example_embed.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * `.embed-example-iframe` placeholders → cross-referenceable embedded
 * runnable-example blocks ("Demo N").
 */

//! Example-iframe embed: sugar + render.
//!
//! Documentation pages mark a live-example embed site with a fenced div
//! carrying the class `embed-example-iframe` and a `file=` attribute
//! naming a **project-relative static asset** (typically a pre-rendered
//! `…/slides.html`):
//!
//! ```markdown
//! ::: {.embed-example-iframe #demo-fragments file="/examples/presentations/03-fragments/slides.html"}
//! [View source](https://github.com/quarto-dev/q2/tree/main/examples/presentations/03-fragments)
//! :::
//! ```
//!
//! ## Cross-referenceable "Demo" blocks (bd-t3cert81)
//!
//! When the div carries a crossref id with the `demo-` prefix, the example
//! becomes a **numbered, cross-referenceable** block on the same footing as
//! figures and theorems: prose can write `@demo-fragments` to get a
//! "Demo 1" link, resolved through the shared crossref index / resolve /
//! render machinery. Without a `demo-` id it stays a plain (unnumbered)
//! embed. (`demo`/"Demo" is deliberately distinct from the theorem-like
//! `exm`/"Example" built-in — see the registry.)
//!
//! ## Two stages
//!
//! This module is a **sugar → render** pair, mirroring
//! callout / float crossref handling:
//!
//! 1. [`ExampleEmbedTransform`] (sugar, normalization phase, *before* the
//!    theorem / float sugar so a `#demo-…` div is never claimed as a
//!    generic float): rewrites `Div.embed-example-iframe` into a
//!    `CustomNode("ExampleEmbed")`. When the id is `demo-…` and `file=`
//!    validates, it populates the standard crossref triple
//!    `{ref_type, kind, identifier}` so `CrossrefIndexTransform` numbers
//!    it; otherwise the triple is omitted and the node is left unnumbered.
//! 2. [`ExampleEmbedRenderTransform`] (finalization, *after*
//!    `CrossrefRenderTransform` so the assigned `order` is available):
//!    turns the `CustomNode` into the final markup — the `<iframe>` (with
//!    a page-relative `src`), a numbered "Demo N: …" caption when numbered,
//!    and the human source link.
//!
//! ## Static-asset-only contract
//!
//! `file=` MUST name a pre-existing static asset. Pointing it at a `.qmd`
//! (or other source document that would need dynamic rendering) is rejected
//! with a diagnostic (Q-5-5; a missing `file=` is Q-5-4), and the
//! placeholder degrades to just its fallback link. This lets the iframe
//! behave like any other project link — copied into `_site/` on render,
//! served from the VFS on preview — and dodges infinite-recursion footguns.
//!
//! See `claude-notes/plans/2026-06-09-website-example-iframe-embed.md` and
//! `…/2026-06-09-crossreferenceable-examples.md`.

use hashlink::LinkedHashMap;
use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::attr::{Attr, AttrSourceInfo};
use quarto_pandoc_types::block::{Block, Div, Plain, RawBlock};
use quarto_pandoc_types::custom::{CustomNode, Slot};
use quarto_pandoc_types::inline::{Inline, Span, Str};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceInfo;
use serde_json::{Value, json};

use crate::Result;
use crate::render::RenderContext;
use crate::resource_resolver::ResourceResolverContext;
use crate::transform::AstTransform;
use crate::transforms::navigation_active::page_relative_source;
use crate::transforms::navigation_href::resolve_static_resource_href;

/// Class a doc author writes to request an example embed. Deliberately
/// verbose so it is never typed by accident — a built-in filter silently
/// activates on it.
const MATCH_CLASS: &str = "embed-example-iframe";
/// Class on the container Div the render step produces.
const CONTAINER_CLASS: &str = "embed-example";
/// Class wrapping the caption / source line beneath the frame.
const SOURCE_CLASS: &str = "embed-example-source";
/// Class on the "Demo N" label span of a numbered example.
const LABEL_CLASS: &str = "embed-example-label";
/// Attribute naming the project-relative static asset to embed.
const FILE_ATTR: &str = "file";
/// `CustomNode.type_name` carried between the sugar and render steps.
const NODE_TYPE: &str = "ExampleEmbed";
/// Crossref prefix + display kind for numbered examples. Distinct from the
/// theorem-like `exm`/"Example" built-in. Registered in
/// `crate::crossref::registry`.
const EXAMPLE_REF_TYPE: &str = "demo";
const EXAMPLE_KIND: &str = "Demo";

/// File extensions whose targets would need dynamic rendering. Pointing
/// `file=` at one of these is rejected (static-asset-only contract).
const DYNAMIC_SOURCE_EXTS: &[&str] = &["qmd", "md", "ipynb", "rmd"];

// ===========================================================================
// Sugar: Div.embed-example-iframe → CustomNode("ExampleEmbed")
// ===========================================================================

/// Sugar transform: `Div.embed-example-iframe` → `CustomNode("ExampleEmbed")`.
/// See the module docs.
pub struct ExampleEmbedTransform;

impl ExampleEmbedTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ExampleEmbedTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for ExampleEmbedTransform {
    fn name(&self) -> &str {
        "example-embed"
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        let mut diagnostics = Vec::new();
        sugar_blocks(&mut ast.blocks, &mut diagnostics);
        ctx.diagnostics.extend(diagnostics);
        Ok(())
    }
}

fn sugar_blocks(blocks: &mut Vec<Block>, diags: &mut Vec<DiagnosticMessage>) {
    for block in blocks.iter_mut() {
        sugar_block(block, diags);
    }
}

fn sugar_block(block: &mut Block, diags: &mut Vec<DiagnosticMessage>) {
    recurse_blocks(block, &mut |bs| sugar_blocks(bs, diags));

    if let Block::Div(div) = block
        && div.attr.1.iter().any(|c| c == MATCH_CLASS)
    {
        *block = Block::Custom(sugar_embed(div, diags));
    }
}

/// Convert a matched placeholder Div into an `ExampleEmbed` CustomNode.
///
/// The node always carries the embed payload in `plain_data` (`file`,
/// optional `height`/`title`) and the author's caption/source content in
/// the `body` slot. The crossref triple is populated **only** when `file=`
/// validates *and* the div has a `demo-` crossref id — that is what makes
/// `CrossrefIndexTransform` number it.
fn sugar_embed(div: &Div, diags: &mut Vec<DiagnosticMessage>) -> CustomNode {
    let id = div.attr.0.clone();
    let file = div
        .attr
        .2
        .get(FILE_ATTR)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    // Validate the static-asset contract. On any failure, `file` is left
    // unset so the render step degrades to just the fallback link.
    let valid_file = match &file {
        None => {
            diags.push(missing_file_diagnostic(&div.source_info));
            None
        }
        Some(f) if is_dynamic_source(f) => {
            diags.push(dynamic_target_diagnostic(f, &div.source_info));
            None
        }
        Some(f) => Some(f.clone()),
    };

    let mut plain_data = serde_json::Map::new();
    if let Some(f) = &valid_file {
        plain_data.insert("file".into(), json!(f));
    }
    if let Some(h) = div
        .attr
        .2
        .get("height")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        plain_data.insert("height".into(), json!(h));
    }
    if let Some(t) = div
        .attr
        .2
        .get("title")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        plain_data.insert("title".into(), json!(t));
    }

    // Numbered iff a valid file AND a `demo-` crossref id. We populate the
    // standard triple; `CrossrefIndexTransform` does the counting.
    if valid_file.is_some() && is_demo_id(&id) {
        plain_data.insert("ref_type".into(), json!(EXAMPLE_REF_TYPE));
        plain_data.insert("kind".into(), json!(EXAMPLE_KIND));
        plain_data.insert("identifier".into(), json!(id));
    }

    // Preserve the id (the crossref anchor + index key) and any non-marker
    // classes; `file`/`height`/`title` move into plain_data.
    let mut classes: Vec<String> = Vec::new();
    for c in &div.attr.1 {
        if c != MATCH_CLASS && c != CONTAINER_CLASS {
            classes.push(c.clone());
        }
    }
    let attr: Attr = (id, classes, LinkedHashMap::new());

    let mut node = CustomNode::new(NODE_TYPE, attr, div.source_info.clone());
    node.plain_data = Value::Object(plain_data);
    node.set_slot("body", Slot::Blocks(div.content.clone()));
    node
}

// ===========================================================================
// Render: CustomNode("ExampleEmbed") → container Div with iframe + caption
// ===========================================================================

/// Page context for relativizing the iframe `src`. Copy so it threads
/// cheaply through the recursive walk.
#[derive(Clone, Copy)]
struct Resolve<'a> {
    source: &'a str,
    resolver: Option<&'a ResourceResolverContext>,
}

/// Render transform: `CustomNode("ExampleEmbed")` → final markup.
pub struct ExampleEmbedRenderTransform;

impl ExampleEmbedRenderTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ExampleEmbedRenderTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for ExampleEmbedRenderTransform {
    fn name(&self) -> &str {
        "example-embed-render"
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        let source = page_relative_source(ctx);
        let resolve = Resolve {
            source: &source,
            resolver: ctx.resource_resolver.as_ref(),
        };
        render_blocks(&mut ast.blocks, resolve);
        Ok(())
    }
}

fn render_blocks(blocks: &mut Vec<Block>, resolve: Resolve) {
    for block in blocks.iter_mut() {
        render_block(block, resolve);
    }
}

fn render_block(block: &mut Block, resolve: Resolve) {
    recurse_blocks(block, &mut |bs| render_blocks(bs, resolve));

    if let Block::Custom(node) = block
        && node.type_name == NODE_TYPE
    {
        *block = Block::Div(render_embed(node, resolve));
    }
}

/// Build the final container Div from an `ExampleEmbed` node.
fn render_embed(node: &mut CustomNode, resolve: Resolve) -> Div {
    let source_info = node.source_info.clone();
    let id = node.attr.0.clone();
    let extra_classes = node.attr.1.clone();

    let file = node.plain_data.get("file").and_then(|v| v.as_str());
    let body = match node.slots.remove("body") {
        Some(Slot::Blocks(bs)) => bs,
        Some(Slot::Block(b)) => vec![*b],
        _ => Vec::new(),
    };

    let mut content: Vec<Block> = Vec::new();
    if let Some(file) = file {
        content.push(iframe_block(file, node, &source_info, resolve));
    }

    // Caption / source line. When the example is numbered (the indexer
    // wrote `plain_data.order`), prepend a "Demo N: " label.
    let order_num = node
        .plain_data
        .get("order")
        .and_then(|o| o.get("order"))
        .and_then(|n| n.as_u64());
    let kind = node
        .plain_data
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or(EXAMPLE_KIND);
    let caption_blocks = match order_num {
        Some(n) => with_number_label(body, kind, n, &source_info),
        None => body,
    };
    content.push(Block::Div(Div {
        attr: (
            String::new(),
            vec![SOURCE_CLASS.to_string()],
            LinkedHashMap::new(),
        ),
        content: caption_blocks,
        source_info: source_info.clone(),
        attr_source: AttrSourceInfo::empty(),
    }));

    // The container carries the id so `@demo-…` references jump to it.
    let mut classes = vec![CONTAINER_CLASS.to_string()];
    classes.extend(extra_classes);
    Div {
        attr: (id, classes, LinkedHashMap::new()),
        content,
        source_info,
        attr_source: AttrSourceInfo::empty(),
    }
}

/// Build the `<iframe>` RawBlock for a validated static `file` target.
///
/// The `src` is the `file=` value run through
/// [`resolve_static_resource_href`] so it is **page-relative** (e.g.
/// `../../examples/x/slides.html` from a depth-2 page), not a host-absolute
/// `/examples/...` that breaks under a deploy subpath.
///
/// Sizing: an optional `height` (from the placeholder's `height=`) overrides
/// the default; otherwise the frame fills its width at a 16:9 aspect ratio.
fn iframe_block(
    file: &str,
    node: &CustomNode,
    source_info: &SourceInfo,
    resolve: Resolve,
) -> Block {
    let src = resolve_static_resource_href(file, resolve.source, resolve.resolver);
    let style = match node.plain_data.get("height").and_then(|v| v.as_str()) {
        Some(height) => format!("width: 100%; height: {};", attr_escape(height)),
        None => "width: 100%; aspect-ratio: 16 / 9;".to_string(),
    };
    let title = node
        .plain_data
        .get("title")
        .and_then(|v| v.as_str())
        .map(|t| format!(" title=\"{}\"", attr_escape(t)))
        .unwrap_or_default();
    let html = format!(
        "<iframe class=\"{MATCH_CLASS}\" src=\"{src}\"{title} \
         style=\"{style}\" loading=\"lazy\" allowfullscreen></iframe>",
        src = attr_escape(&src),
        title = title,
        style = attr_escape(&style),
    );
    Block::RawBlock(RawBlock {
        format: "html".to_string(),
        text: html,
        source_info: source_info.clone(),
    })
}

/// Prepend a `Demo N: ` label (a styled span) to the caption body. If the
/// first block is a `Para`/`Plain`, the label is inlined into it; otherwise
/// a standalone label paragraph is inserted first.
fn with_number_label(
    mut body: Vec<Block>,
    kind: &str,
    order: u64,
    source_info: &SourceInfo,
) -> Vec<Block> {
    let label = Inline::Span(Span {
        attr: (
            String::new(),
            vec![LABEL_CLASS.to_string()],
            LinkedHashMap::new(),
        ),
        content: vec![Inline::Str(Str {
            // Non-breaking space between kind and number, matching crossref
            // reference text ("Figure\u{a0}1").
            text: format!("{kind}\u{a0}{order}"),
            source_info: source_info.clone(),
        })],
        source_info: source_info.clone(),
        attr_source: AttrSourceInfo::empty(),
    });
    let sep = Inline::Str(Str {
        text: ": ".to_string(),
        source_info: source_info.clone(),
    });

    match body.first_mut() {
        Some(Block::Paragraph(p)) => {
            p.content.splice(0..0, [label, sep]);
        }
        Some(Block::Plain(p)) => {
            p.content.splice(0..0, [label, sep]);
        }
        _ => {
            // No leading inline block to attach to — emit a bare label line.
            body.insert(
                0,
                Block::Plain(Plain {
                    content: vec![label],
                    source_info: source_info.clone(),
                }),
            );
        }
    }
    body
}

// ===========================================================================
// Shared helpers
// ===========================================================================

/// Apply `f` to the child-block vectors of `block` (the containers an embed
/// can be nested inside). Leaf blocks are untouched. Shared by both stages.
fn recurse_blocks(block: &mut Block, f: &mut impl FnMut(&mut Vec<Block>)) {
    match block {
        Block::BlockQuote(bq) => f(&mut bq.content),
        Block::Div(div) => f(&mut div.content),
        Block::Figure(fig) => f(&mut fig.content),
        Block::OrderedList(ol) => {
            for item in &mut ol.content {
                f(item);
            }
        }
        Block::BulletList(bl) => {
            for item in &mut bl.content {
                f(item);
            }
        }
        Block::DefinitionList(dl) => {
            for (_term, defs) in &mut dl.content {
                for def in defs {
                    f(def);
                }
            }
        }
        Block::Table(table) => {
            for body in &mut table.bodies {
                for row in &mut body.body {
                    for cell in &mut row.cells {
                        f(&mut cell.content);
                    }
                }
            }
            for row in &mut table.head.rows {
                for cell in &mut row.cells {
                    f(&mut cell.content);
                }
            }
            for row in &mut table.foot.rows {
                for cell in &mut row.cells {
                    f(&mut cell.content);
                }
            }
        }
        Block::Custom(node) => {
            for (_name, slot) in node.slots.iter_mut() {
                match slot {
                    Slot::Block(b) => recurse_blocks(b, f),
                    Slot::Blocks(bs) => f(bs),
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

/// True if `id` is a `demo-…` crossref id (the numbered-example prefix).
fn is_demo_id(id: &str) -> bool {
    id.split_once('-')
        .map(|(prefix, rest)| prefix == EXAMPLE_REF_TYPE && !rest.is_empty())
        .unwrap_or(false)
}

/// True if `file` names a source document that would need dynamic
/// rendering (so it cannot be embedded as a static asset).
fn is_dynamic_source(file: &str) -> bool {
    let path = file.split(['#', '?']).next().unwrap_or(file);
    match path.rsplit('.').next() {
        Some(ext) if !ext.is_empty() && ext.len() < path.len() => {
            DYNAMIC_SOURCE_EXTS.contains(&ext.to_ascii_lowercase().as_str())
        }
        _ => false,
    }
}

fn missing_file_diagnostic(source_info: &SourceInfo) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning("Example Embed Missing `file=`")
        .with_code("Q-5-4")
        .with_location(source_info.clone())
        .problem(format!(
            "An `.{MATCH_CLASS}` placeholder must carry a `{FILE_ATTR}=` attribute \
             naming the asset to embed."
        ))
        .add_hint("Add `file=\"path/to/asset.html\"` pointing at a project-relative static asset?")
        .build()
}

fn dynamic_target_diagnostic(file: &str, source_info: &SourceInfo) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning("Example Embed Target Is Not a Static Asset")
        .with_code("Q-5-5")
        .with_location(source_info.clone())
        .problem(format!(
            "An `.{MATCH_CLASS}` `{FILE_ATTR}` can't point at a source document that \
             would need rendering."
        ))
        .add_detail(format!("`{FILE_ATTR}=\"{file}\"` is a source document."))
        .add_hint("Point `file=` at the pre-rendered output instead (e.g. a `.html` file)?")
        .build()
}

/// Escape a string for use inside a double-quoted HTML attribute value.
fn attr_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::attr::TargetSourceInfo;
    use quarto_pandoc_types::block::Paragraph;
    use quarto_pandoc_types::inline::Link;
    use quarto_source_map::{FileId, Location, Range};

    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use crate::render::{BinaryDependencies, RenderContext};

    fn dummy_source_info() -> SourceInfo {
        SourceInfo::from_range(
            FileId(0),
            Range {
                start: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
            },
        )
    }

    fn make_test_project() -> ProjectContext {
        ProjectContext {
            dir: std::path::PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/doc.qmd")],
            output_dir: std::path::PathBuf::from("/project"),
        }
    }

    fn source_link(text: &str, href: &str) -> Block {
        Block::Paragraph(Paragraph {
            content: vec![Inline::Link(Link {
                attr: (String::new(), vec![], LinkedHashMap::new()),
                content: vec![Inline::Str(Str {
                    text: text.to_string(),
                    source_info: dummy_source_info(),
                })],
                target: (href.to_string(), String::new()),
                source_info: dummy_source_info(),
                attr_source: AttrSourceInfo::empty(),
                target_source: TargetSourceInfo::empty(),
            })],
            source_info: dummy_source_info(),
        })
    }

    /// A placeholder Div with the given id and key/value attributes plus a
    /// single fallback link body.
    fn placeholder(id: &str, kvs: &[(&str, &str)]) -> Block {
        let mut map = LinkedHashMap::new();
        for (k, v) in kvs {
            map.insert((*k).to_string(), (*v).to_string());
        }
        Block::Div(Div {
            attr: (id.to_string(), vec![MATCH_CLASS.to_string()], map),
            content: vec![source_link("View source", "https://github.com/q/x")],
            source_info: dummy_source_info(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    /// Run ONLY the sugar stage.
    async fn run_sugar(blocks: Vec<Block>) -> (Pandoc, Vec<DiagnosticMessage>) {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks,
        };
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        ExampleEmbedTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        (ast, ctx.diagnostics)
    }

    /// Run sugar, then (optionally) simulate the indexer assigning an order,
    /// then render — exercising the full local pipeline minus the real
    /// crossref index. `order` simulates `CrossrefIndexTransform`.
    async fn run_sugar_then_render(
        blocks: Vec<Block>,
        order: Option<u64>,
        doc_path: &str,
        output_href: Option<&str>,
    ) -> Pandoc {
        let (mut ast, _) = run_sugar(blocks).await;
        if let Some(n) = order {
            // Find the first ExampleEmbed node and stamp plain_data.order,
            // as CrossrefIndexTransform would.
            stamp_order(&mut ast.blocks, n);
        }
        let project = ProjectContext {
            dir: std::path::PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: output_href.is_none(),
            files: vec![DocumentInfo::from_path(doc_path)],
            output_dir: std::path::PathBuf::from("/project/_site"),
        };
        let doc = DocumentInfo::from_path(doc_path);
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        if let Some(href) = output_href {
            let page_output = format!("/project/_site/{}", href);
            let stem = std::path::Path::new(href)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("index");
            ctx.resource_resolver = Some(ResourceResolverContext::website(
                "/project/_site",
                page_output,
                "site_libs",
                stem,
            ));
        }
        ExampleEmbedRenderTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        ast
    }

    fn stamp_order(blocks: &mut [Block], n: u64) -> bool {
        for b in blocks.iter_mut() {
            if let Block::Custom(node) = b
                && node.type_name == NODE_TYPE
                && let Some(obj) = node.plain_data.as_object_mut()
            {
                obj.insert("order".into(), json!({"section": [], "order": n}));
                return true;
            }
        }
        false
    }

    fn first_custom(ast: &Pandoc) -> &CustomNode {
        match &ast.blocks[0] {
            Block::Custom(n) => n,
            other => panic!("expected CustomNode, got {:?}", other),
        }
    }

    fn collect_raw_html(block: &Block) -> String {
        let mut out = String::new();
        fn go(block: &Block, out: &mut String) {
            match block {
                Block::RawBlock(r) => out.push_str(&r.text),
                Block::Div(d) => d.content.iter().for_each(|b| go(b, out)),
                _ => {}
            }
        }
        go(block, &mut out);
        out
    }

    fn collect_text(block: &Block) -> String {
        let mut out = String::new();
        fn inl(i: &Inline, out: &mut String) {
            match i {
                Inline::Str(s) => out.push_str(&s.text),
                Inline::Span(s) => s.content.iter().for_each(|c| inl(c, out)),
                Inline::Link(l) => l.content.iter().for_each(|c| inl(c, out)),
                _ => {}
            }
        }
        fn go(block: &Block, out: &mut String) {
            match block {
                Block::Div(d) => d.content.iter().for_each(|b| go(b, out)),
                Block::Paragraph(p) => p.content.iter().for_each(|i| inl(i, out)),
                Block::Plain(p) => p.content.iter().for_each(|i| inl(i, out)),
                _ => {}
            }
        }
        go(block, &mut out);
        out
    }

    // ---- sugar stage ----

    #[tokio::test]
    async fn test_names() {
        assert_eq!(ExampleEmbedTransform::new().name(), "example-embed");
        assert_eq!(
            ExampleEmbedRenderTransform::new().name(),
            "example-embed-render"
        );
    }

    #[tokio::test]
    async fn sugar_produces_custom_node() {
        let (ast, diags) = run_sugar(vec![placeholder(
            "",
            &[("file", "/examples/x/slides.html")],
        )])
        .await;
        assert!(diags.is_empty());
        let node = first_custom(&ast);
        assert_eq!(node.type_name, NODE_TYPE);
        assert_eq!(
            node.plain_data.get("file").and_then(|v| v.as_str()),
            Some("/examples/x/slides.html")
        );
        // No demo id → no crossref triple → unnumbered.
        assert!(node.plain_data.get("ref_type").is_none());
    }

    #[tokio::test]
    async fn sugar_demo_id_populates_crossref_triple() {
        let (ast, _) = run_sugar(vec![placeholder(
            "demo-frag",
            &[("file", "/examples/x/slides.html")],
        )])
        .await;
        let node = first_custom(&ast);
        assert_eq!(
            node.plain_data.get("ref_type").and_then(|v| v.as_str()),
            Some("demo")
        );
        assert_eq!(
            node.plain_data.get("kind").and_then(|v| v.as_str()),
            Some("Demo")
        );
        assert_eq!(
            node.attr.0, "demo-frag",
            "id preserved on the node for indexing"
        );
    }

    #[tokio::test]
    async fn sugar_non_demo_id_is_not_numbered() {
        let (ast, _) = run_sugar(vec![placeholder(
            "myexample",
            &[("file", "/examples/x/slides.html")],
        )])
        .await;
        let node = first_custom(&ast);
        assert!(
            node.plain_data.get("ref_type").is_none(),
            "only `demo-` ids are numbered"
        );
        assert_eq!(node.attr.0, "myexample", "id still preserved");
    }

    #[tokio::test]
    async fn sugar_rejects_qmd_target() {
        let (ast, diags) = run_sugar(vec![placeholder(
            "demo-frag",
            &[("file", "/examples/x/slides.qmd")],
        )])
        .await;
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code.as_deref(), Some("Q-5-5"));
        let node = first_custom(&ast);
        // Bad file → no file payload, and NOT numbered (broken example).
        assert!(node.plain_data.get("file").is_none());
        assert!(node.plain_data.get("ref_type").is_none());
    }

    #[tokio::test]
    async fn sugar_missing_file_diagnoses() {
        let (ast, diags) = run_sugar(vec![placeholder("demo-frag", &[])]).await;
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code.as_deref(), Some("Q-5-4"));
        assert!(first_custom(&ast).plain_data.get("file").is_none());
    }

    // ---- render stage ----

    #[tokio::test]
    async fn render_unnumbered_emits_iframe_and_source() {
        let ast = run_sugar_then_render(
            vec![placeholder("", &[("file", "/examples/x/slides.html")])],
            None,
            "/project/doc.qmd",
            None,
        )
        .await;
        let html = collect_raw_html(&ast.blocks[0]);
        assert!(html.contains("<iframe"), "expected an iframe; got {html}");
        // No resolver → src verbatim.
        assert!(html.contains("src=\"/examples/x/slides.html\""));
        let text = collect_text(&ast.blocks[0]);
        assert!(text.contains("View source"), "source link retained");
        assert!(
            !text.contains("Demo"),
            "unnumbered example has no Demo label; got {text:?}"
        );
    }

    #[tokio::test]
    async fn render_numbered_prepends_demo_label_and_relativizes_src() {
        let ast = run_sugar_then_render(
            vec![placeholder(
                "demo-frag",
                &[("file", "/examples/presentations/03-fragments/slides.html")],
            )],
            Some(1),
            "/project/presentations/revealjs/index.qmd",
            Some("presentations/revealjs/index.html"),
        )
        .await;
        let html = collect_raw_html(&ast.blocks[0]);
        // Page-relative src for a depth-2 page.
        assert!(
            html.contains("src=\"../../examples/presentations/03-fragments/slides.html\""),
            "src must be page-relative; got {html}"
        );
        let text = collect_text(&ast.blocks[0]);
        assert!(
            text.contains("Demo\u{a0}1"),
            "numbered example shows `Demo 1`; got {text:?}"
        );
        assert!(
            text.contains("View source"),
            "source link retained alongside the label"
        );
        // The container carries the crossref anchor id.
        let Block::Div(container) = &ast.blocks[0] else {
            panic!()
        };
        assert_eq!(
            container.attr.0, "demo-frag",
            "container id is the crossref anchor"
        );
    }

    #[tokio::test]
    async fn render_bad_file_degrades_to_source_only() {
        let ast = run_sugar_then_render(
            vec![placeholder(
                "demo-frag",
                &[("file", "/examples/x/slides.qmd")],
            )],
            None,
            "/project/doc.qmd",
            None,
        )
        .await;
        let html = collect_raw_html(&ast.blocks[0]);
        assert!(
            !html.contains("<iframe"),
            "a rejected target must not emit an iframe"
        );
        assert!(collect_text(&ast.blocks[0]).contains("View source"));
    }

    #[tokio::test]
    async fn render_nested_embed_in_blockquote() {
        let bq = Block::BlockQuote(quarto_pandoc_types::block::BlockQuote {
            content: vec![placeholder("", &[("file", "/examples/x/slides.html")])],
            source_info: dummy_source_info(),
        });
        let ast = run_sugar_then_render(vec![bq], None, "/project/doc.qmd", None).await;
        // The embed nested in a blockquote is sugared + rendered.
        fn find_iframe(b: &Block) -> bool {
            match b {
                Block::RawBlock(r) => r.text.contains("<iframe"),
                Block::Div(d) => d.content.iter().any(find_iframe),
                Block::BlockQuote(bq) => bq.content.iter().any(find_iframe),
                _ => false,
            }
        }
        assert!(
            find_iframe(&ast.blocks[0]),
            "nested embed must still render an iframe"
        );
    }

    #[tokio::test]
    async fn test_is_dynamic_source() {
        assert!(is_dynamic_source("foo/bar.qmd"));
        assert!(is_dynamic_source("a.IPYNB"));
        assert!(!is_dynamic_source("foo/slides.html"));
        assert!(!is_dynamic_source("noext"));
        assert!(is_dynamic_source("slides.qmd#section"));
        assert!(!is_dynamic_source("slides.html#section"));
    }

    #[tokio::test]
    async fn test_is_demo_id() {
        assert!(is_demo_id("demo-frag"));
        assert!(!is_demo_id("demo")); // no suffix
        assert!(!is_demo_id("demonstration")); // no dash
        assert!(!is_demo_id("fig-x"));
        assert!(!is_demo_id(""));
    }
}
