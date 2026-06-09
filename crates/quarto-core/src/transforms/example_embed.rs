/*
 * example_embed.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Transform that rewrites `.embed-example-iframe` placeholder Divs into
 * live <iframe> embeds of a project-relative static asset.
 */

//! Example-iframe embed transform.
//!
//! Documentation pages mark a live-example embed site with a generic
//! fenced div carrying the class `embed-example-iframe` and a `file=`
//! attribute naming a **project-relative static asset** (typically a
//! pre-rendered `…/slides.html`):
//!
//! ```markdown
//! ::: {.embed-example-iframe file="examples/presentations/03-fragments/slides.html"}
//! [View source](https://github.com/quarto-dev/q2/tree/main/examples/presentations/03-fragments)
//! :::
//! ```
//!
//! This transform replaces each such Div with a container holding:
//!
//! 1. a `RawBlock(html)` `<iframe class="embed-example-iframe" src="…">`
//!    pointing at the resolved `file=` value, and
//! 2. the original body (a human "View source" link) wrapped in a
//!    `.embed-example-source` Div so it reads as a caption under the frame.
//!
//! ## Static-asset-only contract
//!
//! `file=` MUST name a pre-existing static asset. Pointing it at a `.qmd`
//! (or other source document that would need dynamic rendering) is
//! **rejected** with a diagnostic, and the placeholder degrades to just
//! its fallback link. This is what lets the iframe behave like any other
//! project link — copied into `_site/` on render, served from the VFS on
//! preview — and it dodges infinite-recursion footguns (an iframe whose
//! target re-embeds the page that contains it).
//!
//! ## Why a built-in transform (not a shortcode / Lua filter)
//!
//! Running in the shared transform pipeline means render *and* preview,
//! HTML *and* revealjs all get the rewrite for free. The placeholders are
//! authored as Divs, so this is the natural surface. See
//! `claude-notes/plans/2026-06-09-website-example-iframe-embed.md`.

use hashlink::LinkedHashMap;
use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::attr::{Attr, AttrSourceInfo};
use quarto_pandoc_types::block::{Block, Div, RawBlock};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceInfo;

use crate::Result;
use crate::render::RenderContext;
use crate::resource_resolver::ResourceResolverContext;
use crate::transform::AstTransform;
use crate::transforms::navigation_active::page_relative_source;
use crate::transforms::navigation_href::resolve_static_resource_href;

/// Page context needed to relativize the iframe `src`: the page's
/// project-relative source path and the optional per-page resolver.
/// Copy so it threads cheaply through the recursive walk.
#[derive(Clone, Copy)]
struct Resolve<'a> {
    /// Project-relative source path of the page being rendered, e.g.
    /// `presentations/revealjs/index.qmd`. Anchors `..`/leading-`/`
    /// normalization of the `file=` target.
    source: &'a str,
    /// Per-page resolver. `None` for standalone renders / unit tests —
    /// the `file=` value is then emitted verbatim.
    resolver: Option<&'a ResourceResolverContext>,
}

/// Class a doc author writes to request an example embed. Deliberately
/// verbose so it is never typed by accident — a built-in filter silently
/// activates on it.
const MATCH_CLASS: &str = "embed-example-iframe";
/// Class on the container Div this transform produces (replaces
/// [`MATCH_CLASS`], so a second pass would not re-rewrite).
const CONTAINER_CLASS: &str = "embed-example";
/// Class wrapping the human fallback link beneath the frame.
const SOURCE_CLASS: &str = "embed-example-source";
/// Attribute naming the project-relative static asset to embed.
const FILE_ATTR: &str = "file";

/// File extensions whose targets would need dynamic rendering. Pointing
/// `file=` at one of these is rejected (static-asset-only contract).
const DYNAMIC_SOURCE_EXTS: &[&str] = &["qmd", "md", "ipynb", "rmd"];

/// Transform that rewrites `.embed-example-iframe` Divs into live
/// `<iframe>` embeds. See the module docs.
pub struct ExampleEmbedTransform;

impl ExampleEmbedTransform {
    /// Create a new example-embed transform.
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
        // Compute the page anchor + borrow the resolver before taking
        // diagnostics out, so we never hold a `&mut ctx` and a `&ctx`
        // at once (mirrors `LinkRewriteTransform`).
        let source = page_relative_source(ctx);
        let mut diagnostics = std::mem::take(&mut ctx.diagnostics);
        let resolve = Resolve {
            source: &source,
            resolver: ctx.resource_resolver.as_ref(),
        };
        transform_blocks(&mut ast.blocks, resolve, &mut diagnostics);
        ctx.diagnostics = diagnostics;
        Ok(())
    }
}

/// Walk a block vector, rewriting any matching Divs in place.
fn transform_blocks(blocks: &mut Vec<Block>, resolve: Resolve, diags: &mut Vec<DiagnosticMessage>) {
    for block in blocks.iter_mut() {
        transform_block(block, resolve, diags);
    }
}

/// Recurse into a block's children, then rewrite the block itself if it
/// is a matching embed placeholder.
fn transform_block(block: &mut Block, resolve: Resolve, diags: &mut Vec<DiagnosticMessage>) {
    // Recurse into nested blocks first (an embed can live inside a
    // section Div, blockquote, list item, etc.).
    match block {
        Block::BlockQuote(bq) => transform_blocks(&mut bq.content, resolve, diags),
        Block::Div(div) => transform_blocks(&mut div.content, resolve, diags),
        Block::Figure(fig) => transform_blocks(&mut fig.content, resolve, diags),
        Block::OrderedList(ol) => {
            for item in &mut ol.content {
                transform_blocks(item, resolve, diags);
            }
        }
        Block::BulletList(bl) => {
            for item in &mut bl.content {
                transform_blocks(item, resolve, diags);
            }
        }
        Block::DefinitionList(dl) => {
            for (_term, defs) in &mut dl.content {
                for def in defs {
                    transform_blocks(def, resolve, diags);
                }
            }
        }
        Block::Table(table) => {
            for body in &mut table.bodies {
                for row in &mut body.body {
                    for cell in &mut row.cells {
                        transform_blocks(&mut cell.content, resolve, diags);
                    }
                }
            }
            for row in &mut table.head.rows {
                for cell in &mut row.cells {
                    transform_blocks(&mut cell.content, resolve, diags);
                }
            }
            for row in &mut table.foot.rows {
                for cell in &mut row.cells {
                    transform_blocks(&mut cell.content, resolve, diags);
                }
            }
        }
        _ => {}
    }

    // Then rewrite this block if it is a matching `.embed-example-iframe` Div.
    if let Block::Div(div) = block
        && div.attr.1.iter().any(|c| c == MATCH_CLASS)
    {
        let rewritten = rewrite_embed(div, resolve, diags);
        *block = Block::Div(rewritten);
    }
}

/// Rewrite a matched placeholder Div into the container + iframe + source
/// structure. On a missing/invalid `file=`, emits a diagnostic and
/// degrades to just the fallback link (no iframe).
fn rewrite_embed(div: &Div, resolve: Resolve, diags: &mut Vec<DiagnosticMessage>) -> Div {
    let file = div
        .attr
        .2
        .get(FILE_ATTR)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let fallback = source_wrapper(div);

    let Some(file) = file else {
        diags.push(
            DiagnosticMessageBuilder::warning("Example Embed Missing `file=`")
                .with_code("Q-5-4")
                .with_location(div.source_info.clone())
                .problem(format!(
                    "An `.{MATCH_CLASS}` placeholder must carry a `{FILE_ATTR}=` attribute \
                     naming the asset to embed."
                ))
                .add_hint(
                    "Add `file=\"path/to/asset.html\"` pointing at a project-relative static asset?",
                )
                .build(),
        );
        return container(div, vec![fallback]);
    };

    if is_dynamic_source(&file) {
        diags.push(
            DiagnosticMessageBuilder::warning("Example Embed Target Is Not a Static Asset")
                .with_code("Q-5-5")
                .with_location(div.source_info.clone())
                .problem(format!(
                    "An `.{MATCH_CLASS}` `{FILE_ATTR}` can't point at a source document that \
                     would need rendering."
                ))
                .add_detail(format!("`{FILE_ATTR}=\"{file}\"` is a source document."))
                .add_hint("Point `file=` at the pre-rendered output instead (e.g. a `.html` file)?")
                .build(),
        );
        return container(div, vec![fallback]);
    }

    let iframe = iframe_block(&file, div, &div.source_info, resolve);
    container(div, vec![iframe, fallback])
}

/// Build the `<iframe>` RawBlock for a validated static `file` target.
///
/// The `src` is the `file=` value run through
/// [`resolve_static_resource_href`] so it is **page-relative** (e.g.
/// `../../examples/x/slides.html` from a depth-2 page), not a
/// host-absolute `/examples/...` that breaks under a deploy subpath. In
/// the hub-client preview the same call yields a VFS-root URL; in a
/// standalone render (no resolver) the value is emitted verbatim.
///
/// Sizing: an optional `height=` attribute on the placeholder overrides
/// the default; otherwise the frame fills its width at a 16:9 aspect
/// ratio (the common deck shape). Both are inline styles so the feature
/// is usable before any SCSS lands.
fn iframe_block(file: &str, div: &Div, source_info: &SourceInfo, resolve: Resolve) -> Block {
    let src = resolve_static_resource_href(file, resolve.source, resolve.resolver);
    let style = match div
        .attr
        .2
        .get("height")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        Some(height) => format!("width: 100%; height: {};", attr_escape(height)),
        None => "width: 100%; aspect-ratio: 16 / 9;".to_string(),
    };
    let title = div
        .attr
        .2
        .get("title")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|t| format!(" title=\"{}\"", attr_escape(t)))
        .unwrap_or_default();
    let html = format!(
        "<iframe class=\"{MATCH_CLASS}\" src=\"{src}\"{title} \
         style=\"{style}\" loading=\"lazy\" allowfullscreen></iframe>",
        src = attr_escape(&src),
        title = title,
        style = attr_escape_style(&style),
    );
    Block::RawBlock(RawBlock {
        format: "html".to_string(),
        text: html,
        source_info: source_info.clone(),
    })
}

/// Wrap the placeholder's original body (the human fallback link) in a
/// `.embed-example-source` Div so it can be styled as a caption.
fn source_wrapper(div: &Div) -> Block {
    Block::Div(Div {
        attr: (
            String::new(),
            vec![SOURCE_CLASS.to_string()],
            LinkedHashMap::new(),
        ),
        content: div.content.clone(),
        source_info: div.source_info.clone(),
        attr_source: AttrSourceInfo::empty(),
    })
}

/// Build the container Div, preserving the placeholder's id and any
/// non-`MATCH_CLASS` classes, and swapping in [`CONTAINER_CLASS`].
fn container(div: &Div, content: Vec<Block>) -> Div {
    let id = div.attr.0.clone();
    let mut classes = vec![CONTAINER_CLASS.to_string()];
    for c in &div.attr.1 {
        if c != MATCH_CLASS && c != CONTAINER_CLASS {
            classes.push(c.clone());
        }
    }
    let attr: Attr = (id, classes, LinkedHashMap::new());
    Div {
        attr,
        content,
        source_info: div.source_info.clone(),
        attr_source: AttrSourceInfo::empty(),
    }
}

/// True if `file` names a source document that would need dynamic
/// rendering (so it cannot be embedded as a static asset).
fn is_dynamic_source(file: &str) -> bool {
    // Strip any #fragment / ?query before looking at the extension.
    let path = file.split(['#', '?']).next().unwrap_or(file);
    match path.rsplit('.').next() {
        Some(ext) if !ext.is_empty() && ext.len() < path.len() => {
            let ext = ext.to_ascii_lowercase();
            DYNAMIC_SOURCE_EXTS.contains(&ext.as_str())
        }
        _ => false,
    }
}

/// Escape a string for use inside a double-quoted HTML attribute value.
fn attr_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Escape a value destined for a `style="…"` attribute. Same rules as a
/// normal attribute but kept distinct so the call sites read clearly.
fn attr_escape_style(s: &str) -> String {
    attr_escape(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::attr::TargetSourceInfo;
    use quarto_pandoc_types::block::Paragraph;
    use quarto_pandoc_types::inline::{Inline, Link, Str};
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

    /// Build the fallback "View source" link the docs author writes
    /// inside the placeholder body.
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

    /// A placeholder Div with the given key/value attributes and a
    /// single fallback link body.
    fn placeholder(kvs: &[(&str, &str)]) -> Block {
        let mut map = LinkedHashMap::new();
        for (k, v) in kvs {
            map.insert((*k).to_string(), (*v).to_string());
        }
        Block::Div(Div {
            attr: (String::new(), vec![MATCH_CLASS.to_string()], map),
            content: vec![source_link("View source", "https://github.com/q/x")],
            source_info: dummy_source_info(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    async fn run(blocks: Vec<Block>) -> (Pandoc, Vec<DiagnosticMessage>) {
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

    /// Run the transform with a website resolver pinned at a given page,
    /// so the iframe `src` is relativized against the page's depth.
    async fn run_at_page(blocks: Vec<Block>, doc_path: &str, output_href: &str) -> Pandoc {
        let project = ProjectContext {
            dir: std::path::PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: false,
            files: vec![DocumentInfo::from_path(doc_path)],
            output_dir: std::path::PathBuf::from("/project/_site"),
        };
        let doc = DocumentInfo::from_path(doc_path);
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        let page_output = format!("/project/_site/{}", output_href);
        let stem = std::path::Path::new(output_href)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("index");
        ctx.resource_resolver = Some(crate::resource_resolver::ResourceResolverContext::website(
            "/project/_site",
            page_output,
            "site_libs",
            stem,
        ));
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks,
        };
        ExampleEmbedTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        ast
    }

    /// Recursively collect all RawBlock html text under a block.
    fn collect_raw_html(block: &Block) -> String {
        let mut out = String::new();
        fn go(block: &Block, out: &mut String) {
            match block {
                Block::RawBlock(r) => out.push_str(&r.text),
                Block::Div(d) => {
                    for b in &d.content {
                        go(b, out);
                    }
                }
                _ => {}
            }
        }
        go(block, &mut out);
        out
    }

    /// Find the first Link href anywhere under a block.
    fn first_link_href(block: &Block) -> Option<String> {
        fn link_href(inline: &Inline) -> Option<String> {
            match inline {
                Inline::Link(l) => Some(l.target.0.clone()),
                _ => None,
            }
        }
        match block {
            Block::Div(d) => d.content.iter().find_map(first_link_href),
            Block::Paragraph(p) => p.content.iter().find_map(link_href),
            Block::Plain(p) => p.content.iter().find_map(link_href),
            _ => None,
        }
    }

    fn as_div(block: &Block) -> &Div {
        match block {
            Block::Div(d) => d,
            other => panic!("expected Div, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_transform_name() {
        assert_eq!(ExampleEmbedTransform::new().name(), "example-embed");
    }

    #[tokio::test]
    async fn test_rewrites_div_to_iframe_container() {
        let (ast, diags) = run(vec![placeholder(&[(
            "file",
            "examples/presentations/03-fragments/slides.html",
        )])])
        .await;
        assert!(diags.is_empty(), "valid embed should emit no diagnostics");
        assert_eq!(ast.blocks.len(), 1);
        let div = as_div(&ast.blocks[0]);
        // Container class swapped; the match class is gone so a second
        // pass would not re-rewrite.
        assert!(
            div.attr.1.iter().any(|c| c == CONTAINER_CLASS),
            "container should carry `{CONTAINER_CLASS}`; got {:?}",
            div.attr.1
        );
        assert!(
            !div.attr.1.iter().any(|c| c == MATCH_CLASS),
            "match class `{MATCH_CLASS}` should be consumed; got {:?}",
            div.attr.1
        );
        // An <iframe> with the file as src is present.
        let html = collect_raw_html(&ast.blocks[0]);
        assert!(
            html.contains("<iframe"),
            "expected an <iframe>; got: {html}"
        );
        assert!(
            html.contains("src=\"examples/presentations/03-fragments/slides.html\""),
            "iframe src should be the file= value; got: {html}"
        );
        assert!(
            html.contains(&format!("class=\"{MATCH_CLASS}\"")),
            "iframe should carry the `{MATCH_CLASS}` class; got: {html}"
        );
    }

    #[tokio::test]
    async fn test_src_relativized_with_resolver_at_nested_page() {
        let ast = run_at_page(
            vec![placeholder(&[(
                "file",
                "/examples/presentations/03-fragments/slides.html",
            )])],
            "/project/presentations/revealjs/index.qmd",
            "presentations/revealjs/index.html",
        )
        .await;
        let html = collect_raw_html(&ast.blocks[0]);
        assert!(
            html.contains("src=\"../../examples/presentations/03-fragments/slides.html\""),
            "iframe src must be page-relative (../../) for a depth-2 page, not host-absolute; \
             got: {html}"
        );
        assert!(
            !html.contains("src=\"/examples"),
            "must not emit a host-absolute /examples src; got: {html}"
        );
    }

    #[tokio::test]
    async fn test_source_link_retained() {
        let (ast, _) = run(vec![placeholder(&[("file", "examples/x/slides.html")])]).await;
        let div = as_div(&ast.blocks[0]);
        // The source link survives somewhere under the container.
        assert_eq!(
            first_link_href(&ast.blocks[0]).as_deref(),
            Some("https://github.com/q/x"),
            "the fallback source link must be retained"
        );
        // ... and is wrapped in a `.embed-example-source` Div.
        let has_source_wrapper = div.content.iter().any(|b| match b {
            Block::Div(d) => d.attr.1.iter().any(|c| c == SOURCE_CLASS),
            _ => false,
        });
        assert!(
            has_source_wrapper,
            "fallback link should be wrapped in `.{SOURCE_CLASS}`"
        );
    }

    #[tokio::test]
    async fn test_rejects_qmd_target() {
        let (ast, diags) = run(vec![placeholder(&[("file", "examples/x/slides.qmd")])]).await;
        let html = collect_raw_html(&ast.blocks[0]);
        assert!(
            !html.contains("<iframe"),
            "a .qmd target must NOT produce an iframe; got: {html}"
        );
        assert_eq!(
            diags.len(),
            1,
            "a .qmd target must emit exactly one diagnostic"
        );
        assert_eq!(
            diags[0].code.as_deref(),
            Some("Q-5-5"),
            "a non-static target must carry code Q-5-5"
        );
        // Fallback link still present so the page is useful.
        assert_eq!(
            first_link_href(&ast.blocks[0]).as_deref(),
            Some("https://github.com/q/x")
        );
    }

    #[tokio::test]
    async fn test_missing_file_attr_degrades() {
        let (ast, diags) = run(vec![placeholder(&[])]).await;
        let html = collect_raw_html(&ast.blocks[0]);
        assert!(
            !html.contains("<iframe"),
            "missing file= must not produce an iframe"
        );
        assert_eq!(
            diags.len(),
            1,
            "missing file= must emit exactly one diagnostic"
        );
        assert_eq!(
            diags[0].code.as_deref(),
            Some("Q-5-4"),
            "a missing file= must carry code Q-5-4"
        );
        assert_eq!(
            first_link_href(&ast.blocks[0]).as_deref(),
            Some("https://github.com/q/x")
        );
    }

    /// Belt-and-braces: the codes this transform emits must be
    /// registered in the shared error catalog under the `project`
    /// subsystem. Mirrors `resource_error_codes_are_registered_in_catalog`
    /// in `project_resources.rs`.
    #[test]
    fn diagnostic_codes_are_registered_in_catalog() {
        for code in ["Q-5-4", "Q-5-5"] {
            assert!(
                quarto_error_reporting::catalog::get_error_info(code).is_some(),
                "code {code} is not registered in error_catalog.json"
            );
            assert_eq!(
                quarto_error_reporting::catalog::get_subsystem(code),
                Some("project"),
                "code {code} should be under the 'project' subsystem"
            );
        }
    }

    #[tokio::test]
    async fn test_non_matching_div_untouched() {
        let plain = Block::Div(Div {
            attr: (
                String::new(),
                vec!["note".to_string()],
                LinkedHashMap::new(),
            ),
            content: vec![source_link("x", "y")],
            source_info: dummy_source_info(),
            attr_source: AttrSourceInfo::empty(),
        });
        let (ast, diags) = run(vec![plain]).await;
        assert!(diags.is_empty());
        let div = as_div(&ast.blocks[0]);
        assert!(
            div.attr.1.iter().any(|c| c == "note"),
            "a non-matching div must be left untouched"
        );
        assert!(!collect_raw_html(&ast.blocks[0]).contains("<iframe"));
    }

    #[tokio::test]
    async fn test_nested_embed_in_blockquote() {
        let bq = Block::BlockQuote(quarto_pandoc_types::block::BlockQuote {
            content: vec![placeholder(&[("file", "examples/x/slides.html")])],
            source_info: dummy_source_info(),
        });
        let (ast, _) = run(vec![bq]).await;
        let html = collect_raw_html_anywhere(&ast.blocks[0]);
        assert!(
            html.contains("<iframe"),
            "an embed nested in a blockquote must still be rewritten; got: {html}"
        );
    }

    /// Like `collect_raw_html` but also descends BlockQuote.
    fn collect_raw_html_anywhere(block: &Block) -> String {
        let mut out = String::new();
        fn go(block: &Block, out: &mut String) {
            match block {
                Block::RawBlock(r) => out.push_str(&r.text),
                Block::Div(d) => d.content.iter().for_each(|b| go(b, out)),
                Block::BlockQuote(bq) => bq.content.iter().for_each(|b| go(b, out)),
                _ => {}
            }
        }
        go(block, &mut out);
        out
    }

    #[tokio::test]
    async fn test_height_attr_sets_explicit_height() {
        let (ast, _) = run(vec![placeholder(&[
            ("file", "examples/x/slides.html"),
            ("height", "500px"),
        ])])
        .await;
        let html = collect_raw_html(&ast.blocks[0]);
        assert!(
            html.contains("height: 500px"),
            "explicit height= should appear in the iframe style; got: {html}"
        );
    }

    #[tokio::test]
    async fn test_is_dynamic_source() {
        assert!(is_dynamic_source("foo/bar.qmd"));
        assert!(is_dynamic_source("foo/bar.QMD"));
        assert!(is_dynamic_source("a.ipynb"));
        assert!(is_dynamic_source("notebook.md"));
        assert!(!is_dynamic_source("foo/slides.html"));
        assert!(!is_dynamic_source("a/b/index.htm"));
        assert!(!is_dynamic_source("noext"));
        assert!(!is_dynamic_source("slides.html#section"));
        assert!(is_dynamic_source("slides.qmd#section"));
    }
}
