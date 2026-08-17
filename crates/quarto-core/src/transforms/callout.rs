/*
 * callout.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Transform that converts callout Divs to CustomNodes.
 */

//! Callout conversion transform.
//!
//! This transform finds Div blocks with `.callout-*` classes and converts
//! them to CustomNode blocks with type "Callout". This enables the HTML
//! writer to render them with proper callout styling.
//!
//! ## Input Structure
//!
//! A callout in the source document looks like:
//!
//! ```markdown
//! ::: {.callout-warning}
//! ## Optional Title
//!
//! Body content here.
//! :::
//! ```
//!
//! This is parsed as a Div with class "callout-warning" containing a Header
//! and Paragraph blocks.
//!
//! ## Output Structure
//!
//! The transform converts this to a CustomNode with:
//! - `type_name`: "Callout"
//! - `slots`:
//!   - "title": Inlines from the first Header (if present)
//!   - "content": Blocks (remaining blocks after title extraction)
//! - `plain_data`: `{"type": "warning", "appearance": "default", ...}`
//! - `attr`: Original Div attributes

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::attr::{Attr, AttrSourceInfo};
use quarto_pandoc_types::block::{Block, Div};
use quarto_pandoc_types::config_value::ConfigValueKind;
use quarto_pandoc_types::custom::{CustomNode, Slot};
use quarto_pandoc_types::inline::Inline;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::{By, SourceInfo};
use serde_json::json;

use crate::Result;
use crate::crossref::RefTypeRegistry;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

/// Known callout types in Quarto.
const CALLOUT_TYPES: &[&str] = &["note", "warning", "tip", "caution", "important"];

/// Transform that converts callout Divs to CustomNodes.
///
/// This allows the HTML writer to render callouts with proper structure
/// (header, icon, title, body) rather than as plain divs.
pub struct CalloutTransform;

impl CalloutTransform {
    /// Create a new callout transform.
    pub fn new() -> Self {
        Self
    }
}

impl Default for CalloutTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for CalloutTransform {
    fn name(&self) -> &str {
        "callout"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Normalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        let registry = ctx.ref_type_registry.as_ref();
        let mut diagnostics: Vec<DiagnosticMessage> = Vec::new();
        transform_blocks(&mut ast.blocks, registry, &mut diagnostics);
        ctx.diagnostics.extend(diagnostics);
        Ok(())
    }
}

/// Transform a vector of blocks, converting callout Divs to CustomNodes.
fn transform_blocks(
    blocks: &mut Vec<Block>,
    registry: Option<&RefTypeRegistry>,
    diagnostics: &mut Vec<DiagnosticMessage>,
) {
    for block in blocks.iter_mut() {
        transform_block(block, registry, diagnostics);
    }
}

/// Transform a single block, potentially converting it to a CustomNode.
fn transform_block(
    block: &mut Block,
    registry: Option<&RefTypeRegistry>,
    diagnostics: &mut Vec<DiagnosticMessage>,
) {
    // First, recursively transform any nested blocks
    match block {
        Block::BlockQuote(bq) => {
            transform_blocks(&mut bq.content, registry, diagnostics);
        }
        Block::OrderedList(ol) => {
            for item in &mut ol.content {
                transform_blocks(item, registry, diagnostics);
            }
        }
        Block::BulletList(bl) => {
            for item in &mut bl.content {
                transform_blocks(item, registry, diagnostics);
            }
        }
        Block::DefinitionList(dl) => {
            for (_term, defs) in &mut dl.content {
                for def in defs {
                    transform_blocks(def, registry, diagnostics);
                }
            }
        }
        Block::Figure(fig) => {
            transform_blocks(&mut fig.content, registry, diagnostics);
        }
        Block::Div(div) => {
            // First transform nested content
            transform_blocks(&mut div.content, registry, diagnostics);

            // Then check if this div is a callout and convert it
            if let Some(callout_type) = extract_callout_type(&div.attr) {
                let custom = convert_div_to_callout(div, &callout_type, registry, diagnostics);
                *block = Block::Custom(custom);
            }
        }
        Block::Table(table) => {
            // Transform table bodies
            for body in &mut table.bodies {
                for row in &mut body.body {
                    for cell in &mut row.cells {
                        transform_blocks(&mut cell.content, registry, diagnostics);
                    }
                }
            }
            // Transform table head
            for row in &mut table.head.rows {
                for cell in &mut row.cells {
                    transform_blocks(&mut cell.content, registry, diagnostics);
                }
            }
            // Transform table foot
            for row in &mut table.foot.rows {
                for cell in &mut row.cells {
                    transform_blocks(&mut cell.content, registry, diagnostics);
                }
            }
        }
        Block::Custom(custom) => {
            // Transform blocks inside custom node slots
            for (_name, slot) in &mut custom.slots {
                match slot {
                    Slot::Block(b) => transform_block(b, registry, diagnostics),
                    Slot::Blocks(bs) => transform_blocks(bs, registry, diagnostics),
                    _ => {}
                }
            }
        }
        // Other block types don't contain nested blocks
        _ => {}
    }
}

/// Extract the callout type from a Div's attributes.
///
/// Returns Some("warning") for a div with class "callout-warning", etc.
/// Returns None if this is not a callout div.
fn extract_callout_type(attr: &Attr) -> Option<String> {
    let (_id, classes, _attrs) = attr;

    for class in classes {
        // Check for "callout-TYPE" pattern
        if let Some(suffix) = class.strip_prefix("callout-") {
            // Verify it's a known callout type
            if CALLOUT_TYPES.contains(&suffix) {
                return Some(suffix.to_string());
            }
        }
    }

    None
}

/// Convert a Div to a CustomNode with type "Callout".
fn convert_div_to_callout(
    div: &mut Div,
    callout_type: &str,
    registry: Option<&RefTypeRegistry>,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> CustomNode {
    let mut content_blocks = std::mem::take(&mut div.content);

    // Title precedence mirrors Q1's `callout.lua:48-50`: the `title=`
    // attribute wins over a leading heading. Q1 accepts *any* header
    // level (`resolveHeadingCaption`), so there is no level test here.
    let attr_title = title_from_attribute(&div.attr, &div.attr_source, diagnostics);
    let has_heading = matches!(content_blocks.first(), Some(Block::Header(_)));

    // Where Q1 silently leaves the heading in the body when `title=` also
    // supplies a title, we warn and consume it: carrying both is almost
    // always an authoring mistake, and rendering the heading underneath a
    // different title reads as a duplicate.
    // (bd-callout-custom-title-dropped-9qi1p7iw, decision 3.)
    if attr_title.is_some() && has_heading {
        diagnostics.push(
            DiagnosticMessageBuilder::warning("Callout title given twice")
                .with_code("Q-2-43")
                .problem(
                    "This callout carries both a `title=` attribute and a leading heading. \
                     The `title=` attribute is used and the heading is dropped.",
                )
                .with_location(div.source_info.clone())
                .build(),
        );
    }

    let title_inlines = match attr_title {
        Some(inlines) => {
            if has_heading {
                content_blocks.remove(0);
            }
            inlines
        }
        // Fall back to the leading heading. The removal lives *inside*
        // this branch on purpose — Q1 only strips the header within
        // `resolveHeadingCaption`, which it never reaches when `title=`
        // is present.
        None => match content_blocks.first() {
            Some(Block::Header(header)) => {
                let inlines = header.content.clone();
                content_blocks.remove(0);
                inlines
            }
            _ => Vec::new(),
        },
    };

    // Extract additional attributes from the div
    let appearance_raw =
        extract_attr_value(&div.attr, "appearance").unwrap_or("default".to_string());
    let collapse_attr = extract_attr_value(&div.attr, "collapse");
    let collapse = collapse_attr.is_some();
    let collapse_starts_collapsed = collapse_attr.as_deref() == Some("true");
    let icon_raw = extract_attr_value(&div.attr, "icon").is_none_or(|v| v != "false");

    // Normalize `appearance="minimal"` → `appearance="simple"` AND `icon=false`,
    // matching TS Quarto's `nameForCalloutStyle` (src/resources/filters/customnodes/callout.lua).
    // Doing it here means the resolver only ever sees the canonical
    // `default` or `simple` appearance string.
    let (appearance, icon) = if appearance_raw == "minimal" {
        ("simple".to_string(), false)
    } else {
        (appearance_raw, icon_raw)
    };

    // Build the plain_data JSON
    let mut plain_data = json!({
        "type": callout_type,
        "appearance": appearance,
        "collapse": collapse,
        "collapse_starts_collapsed": collapse_starts_collapsed,
        "icon": icon
    });

    // If the callout has a crossref-eligible id, inject the standard
    // crossref triple so the indexer and resolver pick it up (plan 2.2).
    let identifier = &div.attr.0;
    if !identifier.is_empty()
        && let Some(reg) = registry
        && let Some(def) = reg.classify_cite_id(identifier)
        && let Some(obj) = plain_data.as_object_mut()
    {
        obj.insert("ref_type".into(), json!(def.ref_type));
        obj.insert("kind".into(), json!(def.kind));
        obj.insert("identifier".into(), json!(identifier));
    }

    // Create the CustomNode
    let mut custom = CustomNode::new("Callout", div.attr.clone(), div.source_info.clone());
    custom.plain_data = plain_data;

    // Add title slot (may be empty if no header)
    custom.set_slot("title", Slot::Inlines(title_inlines));

    // Add content slot
    custom.set_slot("content", Slot::Blocks(content_blocks));

    custom
}

/// Extract a key-value attribute from the Div's attr.
fn extract_attr_value(attr: &Attr, key: &str) -> Option<String> {
    let (_id, _classes, attrs) = attr;
    attrs.get(key).cloned()
}

/// Parse the `title=` attribute as markdown inlines, per Q1's
/// `string_to_quarto_ast_inlines(div.attr.attributes["title"])`.
///
/// Returns `None` when the attribute is absent, empty, or does not reduce
/// to inline content — in each case the caller falls back to a leading
/// heading. The `title=` key is deliberately left on `div.attr`; Q1 keeps
/// it on the rendered div, and so do we.
fn title_from_attribute(
    attr: &Attr,
    attr_source: &AttrSourceInfo,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Option<Vec<Inline>> {
    let value = attr.2.get("title")?;
    if value.is_empty() {
        return None;
    }

    let parent = attribute_value_source(attr, attr_source, "title", value.len());

    match pampa::pandoc::meta::parse_config_string_as_markdown(value, &parent, diagnostics) {
        ConfigValueKind::PandocInlines(inlines) => Some(inlines),
        ConfigValueKind::PandocBlocks(blocks) => match single_paragraph_inlines(blocks) {
            Some(inlines) => Some(inlines),
            None => {
                diagnostics.push(
                    DiagnosticMessageBuilder::warning(
                        "Callout `title=` is not usable as inline content",
                    )
                    .with_code("Q-2-44")
                    .problem(
                        "A callout title must be a single line of inline markdown. \
                         This value parses to block content, so it is ignored.",
                    )
                    .with_location(parent.clone())
                    .build(),
                );
                None
            }
        },
        _ => None,
    }
}

/// Unwrap a single-paragraph (or single-`Plain`) block list to its inlines.
fn single_paragraph_inlines(blocks: Vec<Block>) -> Option<Vec<Inline>> {
    let mut iter = blocks.into_iter();
    let first = iter.next()?;
    if iter.next().is_some() {
        return None;
    }
    match first {
        Block::Paragraph(p) => Some(p.content),
        Block::Plain(p) => Some(p.content),
        _ => None,
    }
}

/// The `SourceInfo` to use as the parent of a re-parsed attribute value.
///
/// This is subtler than it looks. `Attr.2` stores the value **unescaped
/// and unquoted** (`extract_quoted_text` → `unescape_punctuation` in
/// `pampa::pandoc::treesitter_utils::text_helpers`), while the recorded
/// span covers the **raw, quote-inclusive** source, because the grammar
/// token includes its delimiters. `SourceInfo::substring` composes only
/// *affine* maps, so handing it the raw span shifts every parsed node by
/// one (the opening quote) plus one more byte per collapsed escape — the
/// same latent drift the YAML path has.
///
/// We do not need the source text to detect this: the span's length *is*
/// the raw length, and the value's length is known.
///
/// | `span_len` vs `n` | meaning                    | result             |
/// |-------------------|----------------------------|--------------------|
/// | `== n`            | bare, unquoted value       | exact (as-is)      |
/// | `== n + 2`        | quoted, no escapes         | exact (skip quote) |
/// | otherwise         | escapes were collapsed     | bounded fallback   |
///
/// The fallback is safe rather than merely tolerable: unescaping only ever
/// shrinks the string, so every mapped offset stays inside the attribute's
/// own raw extent. The error is at most `1 + escapes` bytes and can never
/// point at a neighbouring attribute.
///
/// Exact non-affine mapping is tracked by bd-mxa44voa, which is shared
/// with the YAML and ipynb paths.
fn attribute_value_source(
    attr: &Attr,
    attr_source: &AttrSourceInfo,
    key: &str,
    value_len: usize,
) -> SourceInfo {
    let generated = || SourceInfo::generated(By::callout());

    let Some(index) = attr.2.keys().position(|k| k == key) else {
        return generated();
    };

    // `AttrSourceInfo.attributes[i]` is positionally aligned with `Attr.2`
    // in insertion order, but that invariant is broken on duplicate keys
    // (bd-3aolj / bd-1e6a5). Guard rather than index blind — the pattern
    // is borrowed from `theorem.rs`.
    debug_assert!(
        attr_source.attributes.is_empty() || attr.2.len() == attr_source.attributes.len(),
        "AttrSourceInfo.attributes is out of sync with Attr.2 (bd-3aolj / bd-1e6a5): kvs={}, attr_source={}",
        attr.2.len(),
        attr_source.attributes.len(),
    );
    if attr.2.len() != attr_source.attributes.len() {
        return generated();
    }

    let Some(value_source) = attr_source.attributes[index].1.clone() else {
        return generated();
    };

    match value_source.resolve_byte_range() {
        Some((_file_id, start, end)) if end >= start => {
            let span_len = end - start;
            if span_len == value_len {
                // Bare value: the span already is the value's text.
                value_source
            } else if span_len == value_len + 2 {
                // Quoted with nothing collapsed: step past the opening
                // quote and the mapping is exact.
                SourceInfo::substring(value_source, 1, 1 + value_len)
            } else {
                // Escapes were collapsed; no affine map exists.
                value_source
            }
        }
        _ => value_source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::attr::{AttrSourceInfo, empty_attr};
    use quarto_pandoc_types::block::{Header, Paragraph};
    use quarto_pandoc_types::inline::Str;
    use quarto_source_map::{FileId, Location, Range, SourceInfo};

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

            ..Default::default()
        }
    }

    fn callout_attr(callout_type: &str) -> Attr {
        (
            String::new(),
            vec![format!("callout-{}", callout_type)],
            hashlink::LinkedHashMap::new(),
        )
    }

    // ---------------------------------------------------------------
    // `title=` attribute support (bd-callout-custom-title-dropped-9qi1p7iw)
    //
    // These tests parse REAL qmd text rather than hand-building a Div.
    // That is load-bearing for two reasons:
    //
    //   1. The attribute value reaches `Attr.2` already *unescaped and
    //      unquoted*, while its `AttrSourceInfo` span covers the raw,
    //      quote-inclusive source. A hand-built fixture cannot express
    //      that mismatch, so a wrong span would be invisible.
    //   2. `SourceInfo::for_test()`-style spans are synthetic; per
    //      `quarto_config::span_assert`'s module docs, span assertions
    //      are only meaningful against text that was actually parsed.
    // ---------------------------------------------------------------

    /// Parse `text` as qmd and run `CalloutTransform` over it.
    ///
    /// Returns the transformed AST, the diagnostics the transform
    /// emitted, and the `SourceContext` the parse produced (so spans can
    /// be resolved back to the very bytes this text supplied).
    async fn parse_and_transform(
        text: &str,
    ) -> (
        Pandoc,
        Vec<quarto_error_reporting::DiagnosticMessage>,
        quarto_source_map::SourceContext,
    ) {
        let (mut ast, ast_context, _parse_diags) = pampa::readers::qmd::read(
            text.as_bytes(),
            false,
            "test.qmd",
            &mut std::io::sink(),
            false,
            None,
        )
        .expect("fixture should parse");

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        CalloutTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();

        (ast, ctx.diagnostics, ast_context.source_context)
    }

    /// The title slot's inlines for the first callout in `ast`.
    fn title_inlines(ast: &Pandoc) -> &[quarto_pandoc_types::inline::Inline] {
        let Some(Block::Custom(custom)) = ast.blocks.iter().find(|b| matches!(b, Block::Custom(_)))
        else {
            panic!("expected a Custom callout block; got {:?}", ast.blocks);
        };
        match custom.get_slot("title") {
            Some(Slot::Inlines(inlines)) => inlines,
            other => panic!("expected an Inlines title slot; got {other:?}"),
        }
    }

    /// The content slot's blocks for the first callout in `ast`.
    fn content_blocks(ast: &Pandoc) -> &[Block] {
        let Some(Block::Custom(custom)) = ast.blocks.iter().find(|b| matches!(b, Block::Custom(_)))
        else {
            panic!("expected a Custom callout block");
        };
        match custom.get_slot("content") {
            Some(Slot::Blocks(blocks)) => blocks,
            other => panic!("expected a Blocks content slot; got {other:?}"),
        }
    }

    /// Flatten inlines to their visible text, so assertions can name the
    /// title a reader would see.
    fn inlines_text(inlines: &[quarto_pandoc_types::inline::Inline]) -> String {
        use quarto_pandoc_types::inline::Inline;
        let mut out = String::new();
        for inline in inlines {
            match inline {
                Inline::Str(s) => out.push_str(&s.text),
                Inline::Space(_) => out.push(' '),
                Inline::Code(c) => out.push_str(&c.text),
                Inline::Emph(e) => out.push_str(&inlines_text(&e.content)),
                Inline::Strong(s) => out.push_str(&inlines_text(&s.content)),
                Inline::Span(s) => out.push_str(&inlines_text(&s.content)),
                // Straight quotes in the source become `Quoted` inlines
                // (Pandoc's smart-quote handling); render the delimiters
                // back so assertions can name what a reader sees.
                Inline::Quoted(q) => {
                    let delim = match q.quote_type {
                        quarto_pandoc_types::inline::QuoteType::DoubleQuote => '"',
                        quarto_pandoc_types::inline::QuoteType::SingleQuote => '\'',
                    };
                    out.push(delim);
                    out.push_str(&inlines_text(&q.content));
                    out.push(delim);
                }
                _ => {}
            }
        }
        out
    }

    fn diag_with_code<'a>(
        diags: &'a [quarto_error_reporting::DiagnosticMessage],
        code: &str,
    ) -> &'a quarto_error_reporting::DiagnosticMessage {
        diags
            .iter()
            .find(|d| d.code.as_deref() == Some(code))
            .unwrap_or_else(|| panic!("expected a {code} diagnostic; got: {diags:?}"))
    }

    #[tokio::test]
    async fn attribute_title_populates_the_title_slot() {
        let (ast, diags, _ctx) =
            parse_and_transform("::: {.callout-note title=\"Off-Host Execution\"}\nBody.\n:::\n")
                .await;

        assert_eq!(inlines_text(title_inlines(&ast)), "Off-Host Execution");
        assert!(
            diags.is_empty(),
            "a plain attribute title should warn about nothing; got {diags:?}"
        );
    }

    #[tokio::test]
    async fn heading_title_still_works() {
        let (ast, _diags, _ctx) =
            parse_and_transform("::: {.callout-note}\n## Heading Title\n\nBody.\n:::\n").await;

        assert_eq!(inlines_text(title_inlines(&ast)), "Heading Title");
        // The heading is consumed, leaving only the body paragraph.
        assert_eq!(content_blocks(&ast).len(), 1);
    }

    #[tokio::test]
    async fn attribute_title_wins_over_heading_and_warns() {
        let (ast, diags, _ctx) = parse_and_transform(
            "::: {.callout-note title=\"Attribute wins\"}\n## Heading loses\n\nBody.\n:::\n",
        )
        .await;

        // Q1's precedence: the attribute supplies the title.
        assert_eq!(inlines_text(title_inlines(&ast)), "Attribute wins");
        // Deliberate divergence from Q1: we consume the heading too,
        // rather than leaving a duplicate-looking title in the body.
        assert_eq!(
            content_blocks(&ast).len(),
            1,
            "the heading should be consumed, leaving only the body"
        );
        diag_with_code(&diags, "Q-2-43");
    }

    #[tokio::test]
    async fn markdown_in_attribute_title_is_parsed() {
        let (ast, _diags, _ctx) =
            parse_and_transform("::: {.callout-tip title=\"Use `renv` today\"}\nBody.\n:::\n")
                .await;

        use quarto_pandoc_types::inline::Inline;
        let inlines = title_inlines(&ast);
        assert!(
            inlines
                .iter()
                .any(|i| matches!(i, Inline::Code(c) if c.text == "renv")),
            "expected a Code inline for `renv`; got {inlines:?}"
        );
        assert_eq!(inlines_text(inlines), "Use renv today");
    }

    #[tokio::test]
    async fn empty_attribute_title_falls_back_to_heading() {
        let (ast, _diags, _ctx) =
            parse_and_transform("::: {.callout-note title=\"\"}\n## Fallback\n\nBody.\n:::\n")
                .await;

        assert_eq!(inlines_text(title_inlines(&ast)), "Fallback");
    }

    #[tokio::test]
    async fn level_one_heading_supplies_the_title() {
        // Q1's `resolveHeadingCaption` accepts any Header level; q2 used
        // to require level >= 2, leaving an H1 sitting in the body.
        let (ast, _diags, _ctx) =
            parse_and_transform("::: {.callout-note}\n# Big Title\n\nBody.\n:::\n").await;

        assert_eq!(inlines_text(title_inlines(&ast)), "Big Title");
        assert_eq!(content_blocks(&ast).len(), 1);
    }

    // --- Source mapping -------------------------------------------------
    //
    // The value handed to the nested parse is NOT the text its SourceInfo
    // describes (unescaped + unquoted vs. raw + quoted), and
    // `SourceInfo::substring` composes only affine maps. These pin the
    // cases where we can still be exact, and bound the one where we
    // cannot. See claude-notes/plans/callout-title-attribute-investigation/
    // escape-drift-probe.md.

    #[tokio::test]
    async fn quoted_title_spans_map_exactly() {
        let text = "::: {.callout-tip title=\"Use `renv` today\"}\nBody.\n:::\n";
        let (ast, _diags, ctx) = parse_and_transform(text).await;

        use quarto_pandoc_types::inline::Inline;
        let code = title_inlines(&ast)
            .iter()
            .find_map(|i| match i {
                Inline::Code(c) if c.text == "renv" => Some(c),
                _ => None,
            })
            .expect("expected a Code inline");

        let resolved = quarto_config::span_assert::resolve_span(&code.source_info, &ctx)
            .expect("code span should resolve");
        assert_eq!(
            resolved.text, "`renv`",
            "the code span must underline the backticked source, not a shifted window"
        );
    }

    #[tokio::test]
    async fn bare_unquoted_title_spans_map_exactly() {
        let text = "::: {.callout-note title=Solo}\nBody.\n:::\n";
        let (ast, _diags, ctx) = parse_and_transform(text).await;

        let inlines = title_inlines(&ast);
        assert_eq!(inlines_text(inlines), "Solo");

        let resolved = quarto_config::span_assert::resolve_span(inlines[0].source_info(), &ctx)
            .expect("title span should resolve");
        assert_eq!(resolved.text, "Solo");
    }

    #[tokio::test]
    async fn escaped_title_span_stays_inside_the_attribute() {
        // `\"` collapses to `"`, so the value is shorter than its span and
        // the mapping cannot be affine. We accept a bounded error, but it
        // must never point outside the attribute's own raw extent.
        let text = "::: {.callout-note title=\"Say \\\"hi\\\" now\"}\nBody.\n:::\n";
        let (ast, _diags, ctx) = parse_and_transform(text).await;

        let inlines = title_inlines(&ast);
        assert_eq!(inlines_text(inlines), "Say \"hi\" now");

        let resolved = quarto_config::span_assert::resolve_span(inlines[0].source_info(), &ctx)
            .expect("title span should resolve");
        let attribute_extent = "\"Say \\\"hi\\\" now\"";
        assert!(
            attribute_extent.contains(resolved.text.trim()),
            "span text {:?} escaped the attribute extent {:?}",
            resolved.text,
            attribute_extent
        );
    }

    #[tokio::test]
    async fn double_backslash_escapes_a_leading_hash() {
        // Two escaping layers run in sequence: the attribute layer
        // unescapes first (`\\` → `\`), then the markdown parser reads
        // `\#` as a literal `#`. A single backslash is consumed by the
        // first layer and leaves a real heading behind — see
        // `single_backslash_hash_is_still_a_heading`. Documented in
        // docs/errors/markdown/Q-2-44.qmd.
        let (ast, diags, _ctx) =
            parse_and_transform("::: {.callout-note title=\"\\\\# Overview\"}\nBody.\n:::\n").await;

        assert_eq!(inlines_text(title_inlines(&ast)), "# Overview");
        assert!(
            diags.is_empty(),
            "an escaped hash is valid inline content; got {diags:?}"
        );
    }

    #[tokio::test]
    async fn single_backslash_hash_is_still_a_heading() {
        // The counterpart to the test above: `\#` survives the attribute
        // layer as a bare `#`, so the value parses as a heading.
        let (ast, diags, _ctx) =
            parse_and_transform("::: {.callout-note title=\"\\# Overview\"}\nBody.\n:::\n").await;

        diag_with_code(&diags, "Q-2-44");
        assert!(
            title_inlines(&ast).is_empty(),
            "the block-valued title should be ignored, leaving the slot empty \
             for the resolver's default-title injection"
        );
    }

    #[tokio::test]
    async fn block_valued_title_warns_and_is_ignored() {
        // A value that parses to more than a single paragraph cannot fill
        // an inline title slot.
        let text = "::: {.callout-note title=\"# Heading\"}\n## Fallback\n\nBody.\n:::\n";
        let (ast, diags, _ctx) = parse_and_transform(text).await;

        diag_with_code(&diags, "Q-2-44");
        // Contents ignored → the heading fallback supplies the title.
        assert_eq!(inlines_text(title_inlines(&ast)), "Fallback");
    }

    #[tokio::test]
    async fn test_extract_callout_type_warning() {
        let attr = callout_attr("warning");
        assert_eq!(extract_callout_type(&attr), Some("warning".to_string()));
    }

    #[tokio::test]
    async fn test_extract_callout_type_note() {
        let attr = callout_attr("note");
        assert_eq!(extract_callout_type(&attr), Some("note".to_string()));
    }

    #[tokio::test]
    async fn test_extract_callout_type_unknown() {
        let attr = (
            String::new(),
            vec!["callout-unknown".to_string()],
            hashlink::LinkedHashMap::new(),
        );
        // Unknown callout types are not converted
        assert_eq!(extract_callout_type(&attr), None);
    }

    #[tokio::test]
    async fn test_extract_callout_type_not_callout() {
        let attr = (
            String::new(),
            vec!["panel-tabset".to_string()],
            hashlink::LinkedHashMap::new(),
        );
        assert_eq!(extract_callout_type(&attr), None);
    }

    #[tokio::test]
    async fn test_convert_simple_callout() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![Block::Div(Div {
                attr: callout_attr("warning"),
                content: vec![Block::Paragraph(Paragraph {
                    content: vec![quarto_pandoc_types::inline::Inline::Str(Str {
                        text: "Warning content".to_string(),
                        source_info: dummy_source_info(),
                    })],
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
                attr_source: AttrSourceInfo::empty(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = CalloutTransform::new();
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        // Verify the Div was converted to a Custom node
        assert_eq!(ast.blocks.len(), 1);
        match &ast.blocks[0] {
            Block::Custom(custom) => {
                assert_eq!(custom.type_name, "Callout");
                assert_eq!(custom.plain_data["type"], "warning");
                assert!(custom.get_slot("content").is_some());
            }
            _ => panic!("Expected Custom block"),
        }
    }

    #[tokio::test]
    async fn test_convert_callout_with_title() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![Block::Div(Div {
                attr: callout_attr("tip"),
                content: vec![
                    Block::Header(Header {
                        level: 2,
                        attr: empty_attr(),
                        content: vec![quarto_pandoc_types::inline::Inline::Str(Str {
                            text: "Pro Tip".to_string(),
                            source_info: dummy_source_info(),
                        })],
                        source_info: dummy_source_info(),
                        attr_source: AttrSourceInfo::empty(),
                    }),
                    Block::Paragraph(Paragraph {
                        content: vec![quarto_pandoc_types::inline::Inline::Str(Str {
                            text: "Tip content".to_string(),
                            source_info: dummy_source_info(),
                        })],
                        source_info: dummy_source_info(),
                    }),
                ],
                source_info: dummy_source_info(),
                attr_source: AttrSourceInfo::empty(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = CalloutTransform::new();
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        // Verify the Custom node has title and content
        match &ast.blocks[0] {
            Block::Custom(custom) => {
                assert_eq!(custom.type_name, "Callout");
                assert_eq!(custom.plain_data["type"], "tip");

                // Check title slot
                match custom.get_slot("title") {
                    Some(Slot::Inlines(inlines)) => {
                        assert_eq!(inlines.len(), 1);
                    }
                    _ => panic!("Expected title slot with Inlines"),
                }

                // Check content slot
                match custom.get_slot("content") {
                    Some(Slot::Blocks(blocks)) => {
                        assert_eq!(blocks.len(), 1); // Just the paragraph, header removed
                    }
                    _ => panic!("Expected content slot with Blocks"),
                }
            }
            _ => panic!("Expected Custom block"),
        }
    }

    #[tokio::test]
    async fn test_nested_callout_in_blockquote() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![Block::BlockQuote(quarto_pandoc_types::block::BlockQuote {
                content: vec![Block::Div(Div {
                    attr: callout_attr("note"),
                    content: vec![Block::Paragraph(Paragraph {
                        content: vec![quarto_pandoc_types::inline::Inline::Str(Str {
                            text: "Nested note".to_string(),
                            source_info: dummy_source_info(),
                        })],
                        source_info: dummy_source_info(),
                    })],
                    source_info: dummy_source_info(),
                    attr_source: AttrSourceInfo::empty(),
                })],
                source_info: dummy_source_info(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = CalloutTransform::new();
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        // Verify the nested Div was converted
        match &ast.blocks[0] {
            Block::BlockQuote(bq) => match &bq.content[0] {
                Block::Custom(custom) => {
                    assert_eq!(custom.type_name, "Callout");
                    assert_eq!(custom.plain_data["type"], "note");
                }
                _ => panic!("Expected Custom block inside BlockQuote"),
            },
            _ => panic!("Expected BlockQuote"),
        }
    }

    #[tokio::test]
    async fn test_transform_name() {
        let transform = CalloutTransform::new();
        assert_eq!(transform.name(), "callout");
    }

    #[tokio::test]
    async fn test_callout_with_crossref_id_gets_plain_data_triple() {
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        ctx.ref_type_registry = Some(RefTypeRegistry::builtin());

        let mut ast = quarto_pandoc_types::pandoc::Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![Block::Div(Div {
                attr: (
                    "nte-important".into(),
                    vec!["callout-note".into()],
                    hashlink::LinkedHashMap::new(),
                ),
                content: vec![Block::Paragraph(Paragraph {
                    content: vec![quarto_pandoc_types::inline::Inline::Str(Str {
                        text: "A note.".into(),
                        source_info: dummy_source_info(),
                    })],
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
                attr_source: AttrSourceInfo::empty(),
            })],
        };

        CalloutTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();

        let Block::Custom(node) = &ast.blocks[0] else {
            panic!("expected Custom");
        };
        assert_eq!(node.type_name, "Callout");
        // The crossref triple should be populated.
        assert_eq!(node.plain_data["ref_type"], "nte");
        assert_eq!(node.plain_data["kind"], "Note");
        assert_eq!(node.plain_data["identifier"], "nte-important");
        // And the normal callout fields are still present.
        assert_eq!(node.plain_data["type"], "note");
    }

    #[tokio::test]
    async fn test_callout_without_crossref_id_has_no_triple() {
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        ctx.ref_type_registry = Some(RefTypeRegistry::builtin());

        let mut ast = quarto_pandoc_types::pandoc::Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![Block::Div(Div {
                attr: callout_attr("note"),
                content: vec![Block::Paragraph(Paragraph {
                    content: vec![quarto_pandoc_types::inline::Inline::Str(Str {
                        text: "No id.".into(),
                        source_info: dummy_source_info(),
                    })],
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
                attr_source: AttrSourceInfo::empty(),
            })],
        };

        CalloutTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();

        let Block::Custom(node) = &ast.blocks[0] else {
            panic!("expected Custom");
        };
        // No crossref fields — empty id doesn't classify.
        assert!(node.plain_data.get("ref_type").is_none());
        assert!(node.plain_data.get("identifier").is_none());
    }

    #[tokio::test]
    async fn test_callout_with_non_crossref_id_has_no_triple() {
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        ctx.ref_type_registry = Some(RefTypeRegistry::builtin());

        let mut ast = quarto_pandoc_types::pandoc::Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![Block::Div(Div {
                attr: (
                    "my-callout".into(),
                    vec!["callout-warning".into()],
                    hashlink::LinkedHashMap::new(),
                ),
                content: vec![Block::Paragraph(Paragraph {
                    content: vec![quarto_pandoc_types::inline::Inline::Str(Str {
                        text: "body".into(),
                        source_info: dummy_source_info(),
                    })],
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
                attr_source: AttrSourceInfo::empty(),
            })],
        };

        CalloutTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();

        let Block::Custom(node) = &ast.blocks[0] else {
            panic!("expected Custom");
        };
        // "my" is not a registered ref-type prefix, so no triple.
        assert!(node.plain_data.get("ref_type").is_none());
    }
}
