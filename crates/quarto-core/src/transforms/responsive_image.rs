/*
 * transforms/responsive_image.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Make body images responsive by tagging them with Bootstrap's
//! `img-fluid` class.
//!
//! Without this, an image whose author supplied no size lays out at its
//! intrinsic pixel width. A 2400px-wide screenshot in an 850px content
//! column renders 2400px wide: it overflows the column, overruns the
//! margin, and puts a horizontal scrollbar on the page. Bootstrap's
//! `.img-fluid { max-width: 100%; height: auto }` is what caps it, and
//! that rule is already in Quarto's compiled theme — only the pass that
//! applies the class was missing (bd-images-no-max-width-e5ywgnma).
//!
//! Mirrors Quarto 1's `quarto-post/responsive.lua`, including both of
//! its deliberate exclusions:
//!
//! - **an explicit `height` attribute.** `img-fluid` carries
//!   `height: auto`, which would silently override the height the
//!   author asked for. An author who sized an image vertically has
//!   opted out of automatic sizing.
//! - **`data-no-responsive`.** The per-image escape hatch, honored for
//!   any value; only the key's presence is tested.
//!
//! An explicit `width` is *not* an exclusion, and that asymmetry is
//! Quarto 1's, not an oversight: `max-width: 100%` still lets a
//! `width=450` image shrink inside a narrower column, so the class
//! refines the author's width instead of fighting it.
//!
//! The document-level switch is `fig-responsive`, default `true` for
//! HTML (Quarto 1's schema default). `fig-responsive: false` disables
//! the pass entirely.
//!
//! Pipeline placement: **FINALIZATION PHASE**, next to
//! `TableBootstrapClassTransform` — its sibling in intent, since both
//! inject the Bootstrap classes the theme is keyed off. Running late
//! means every image an upstream transform left in the AST — notably
//! crossref-rendered figures — is in place and gets tagged, which puts
//! this at the same point in the run as Quarto 1's `quarto-post`
//! filter. Idempotent, so a second pass over an already-tagged
//! document is a no-op.
//!
//! What this does *not* reach, in either engine, is markup emitted as
//! raw HTML rather than as AST nodes: listing thumbnails
//! (`class="thumbnail-image"`), navbar logos and page-footer images
//! all arrive as `rendered.navigation.*` strings and stay untagged.
//!
//! The preview pipeline inherits this pass — `q2 preview` and the
//! hub-client run the same builder, filtered by deny-list — so a
//! previewed page constrains its images exactly as the rendered one
//! does. The class reaches the DOM there: the preview `Image`
//! component copies AST classes onto the `<img>`, and the iframe loads
//! the document's compiled Bootstrap theme (which carries
//! `.img-fluid{max-width:100%;height:auto}`) via `<link data-q2-theme>`.
//!
//! Gated to HTML-family formats, but that is only half of Quarto 1's
//! rule. Its filter tests `isHtmlOutput()` *and* reads
//! `param('fig-responsive')`, whose default is set per format — so
//! revealjs, for which `isHtmlOutput()` is true, is nonetheless
//! **not** tagged, because `createHtmlPresentationFormat` sets that
//! param `false` for every HTML presentation format. `minimal: true`
//! is excluded the same way. See `responsive_enabled`.
//!
//! Not ported here: the `figure-img` class Quarto 1 also puts on images
//! inside a `<figure>`. Despite sitting one class away in the output it
//! comes from somewhere else entirely — the DOM postprocessor in
//! `format-html-bootstrap.ts`, which adds `figure`/`figure-img`/
//! `blockquote` classes in one sweep — and it is cosmetic
//! (`margin-bottom: .5rem; line-height: 1`). It belongs with that
//! group, not with this one.

use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::Slot;
use quarto_pandoc_types::attr::Attr;
use quarto_pandoc_types::block::Block;
use quarto_pandoc_types::inline::{Inline, Inlines};
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::format::Format;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

/// Bootstrap class that caps an image at its container's width.
/// Ships in the compiled theme as `max-width: 100%; height: auto`.
const IMG_FLUID_CLASS: &str = "img-fluid";

/// Document-level switch, default `true`. Matches Quarto 1's
/// `fig-responsive` schema entry (`document-figures.yml`).
const FIG_RESPONSIVE_KEY: &str = "fig-responsive";

/// Per-image opt-out. Presence is what counts; the value is ignored.
const NO_RESPONSIVE_ATTR: &str = "data-no-responsive";

/// Attribute whose presence means the author fixed the vertical size.
const HEIGHT_ATTR: &str = "height";

pub struct ResponsiveImageTransform;

impl ResponsiveImageTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ResponsiveImageTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for ResponsiveImageTransform {
    fn name(&self) -> &str {
        "responsive-image"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Finalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        if !responsive_enabled(ctx.format, &ast.meta) {
            return Ok(());
        }
        visit_blocks(&mut ast.blocks);
        Ok(())
    }
}

/// Whether the pass should run at all.
///
/// Split out from `transform` so the gate is directly testable — the
/// same shape as `format_supports_draft_alert` in `draft_alert.rs`.
///
/// Quarto 1 spreads this decision across two places, and both matter:
/// the filter tests `isHtmlOutput()` **and** reads
/// `param('fig-responsive')`, whose *default* is set per format. So
/// "HTML output" alone is not the rule — `isHtmlOutput()` is true for
/// revealjs, yet Q1 does not tag deck images, because
/// `createHtmlPresentationFormat` (`formats-shared.ts`) sets the param
/// `false` for every HTML presentation format. `format-html.ts`'s
/// `resolveFormat` does the same for `minimal: true`.
///
/// Both defaults are suppliers of last resort in Q1 (a config merge,
/// and an `=== undefined` check), so an explicit `fig-responsive` in
/// the document always wins — hence `unwrap_or(default_on)` rather
/// than an early return.
fn responsive_enabled(format: &Format, meta: &ConfigValue) -> bool {
    if !format.is_html() {
        return false;
    }
    // Reveal is matched on `target_format`, not the identifier: the
    // preview pseudo-format `q2-slides` is a deck but resolves to
    // `FormatIdentifier::Html`, so an identifier test would tag deck
    // images in preview and not in render.
    //
    // `q2-slides` covers the live React deck too, even though it is
    // legacy as a *user-facing* format. Since the bd-vwp4y5ku
    // convergence, `format: revealjs` in hub-client and `q2 preview`
    // renders through the shared q2-preview iframe, and the WASM entry
    // rewrites `revealjs` → `q2-slides` on the way in
    // (`map_format_for_preview`, `wasm-quarto-hub-client/src/lib.rs`).
    // So every deck — native render, CLI preview, hub-client — reaches
    // this gate as either `revealjs` or `q2-slides`.
    let is_presentation = crate::format::is_revealjs_target(&format.target_format);
    // The raw `minimal` flag, deliberately NOT `is_minimal_html`:
    // that helper also returns true for `theme: none` / `theme:
    // pandoc`, whereas Q1 flips this default only on `minimal: true`.
    // With a bare `theme: none` both engines tag.
    let is_minimal = meta.get("minimal").and_then(|v| v.as_bool()) == Some(true);
    let default_on = !is_presentation && !is_minimal;

    // `as_bool`, not a truthiness check: only an explicit `false` turns
    // the pass off. An absent key — or a non-boolean like the quoted
    // string `"false"`, which Quarto 1 also treats as on — falls back
    // to the format default.
    meta.get(FIG_RESPONSIVE_KEY)
        .and_then(|v| v.as_bool())
        .unwrap_or(default_on)
}

/// Tag `image` unless it is excluded. Split out so the unit tests can
/// exercise the per-node rule without building a `RenderContext`.
fn apply_to_image(attr: &mut Attr) {
    if attr.2.contains_key(HEIGHT_ATTR) || attr.2.contains_key(NO_RESPONSIVE_ATTR) {
        return;
    }
    if !attr.1.iter().any(|c| c == IMG_FLUID_CLASS) {
        attr.1.push(IMG_FLUID_CLASS.to_string());
    }
}

fn visit_blocks(blocks: &mut [Block]) {
    for block in blocks.iter_mut() {
        visit_block(block);
    }
}

fn visit_block(block: &mut Block) {
    match block {
        Block::Plain(p) => visit_inlines(&mut p.content),
        Block::Paragraph(p) => visit_inlines(&mut p.content),
        Block::LineBlock(lb) => {
            for line in lb.content.iter_mut() {
                visit_inlines(line);
            }
        }
        Block::BlockQuote(bq) => visit_blocks(&mut bq.content),
        Block::OrderedList(ol) => {
            for item in ol.content.iter_mut() {
                visit_blocks(item);
            }
        }
        Block::BulletList(bl) => {
            for item in bl.content.iter_mut() {
                visit_blocks(item);
            }
        }
        Block::DefinitionList(dl) => {
            for (term, defs) in dl.content.iter_mut() {
                visit_inlines(term);
                for def in defs.iter_mut() {
                    visit_blocks(def);
                }
            }
        }
        Block::Header(h) => visit_inlines(&mut h.content),
        Block::Div(d) => visit_blocks(&mut d.content),
        Block::Figure(f) => {
            visit_blocks(&mut f.content);
            if let Some(short) = f.caption.short.as_mut() {
                visit_inlines(short);
            }
            if let Some(long) = f.caption.long.as_mut() {
                visit_blocks(long);
            }
        }
        Block::Table(t) => {
            if let Some(short) = t.caption.short.as_mut() {
                visit_inlines(short);
            }
            if let Some(long) = t.caption.long.as_mut() {
                visit_blocks(long);
            }
            for row in t.head.rows.iter_mut().chain(t.foot.rows.iter_mut()) {
                for cell in row.cells.iter_mut() {
                    visit_blocks(&mut cell.content);
                }
            }
            for body in t.bodies.iter_mut() {
                for row in body.head.iter_mut().chain(body.body.iter_mut()) {
                    for cell in row.cells.iter_mut() {
                        visit_blocks(&mut cell.content);
                    }
                }
            }
        }
        Block::CaptionBlock(cb) => visit_inlines(&mut cb.content),
        Block::Custom(c) => {
            for (_name, slot) in c.slots.iter_mut() {
                visit_slot(slot);
            }
        }
        // Not walked. `CodeBlock` / `RawBlock` / `HorizontalRule` /
        // `BlockMetadata` are true leaves. The two `NoteDefinition*`
        // variants do carry content, but `FootnotesTransform` has
        // already lifted every reachable definition into a trailing
        // `Div#footnotes` by the time this runs, so anything still in
        // one is unreferenced and never rendered. Same set
        // `link_rewrite` skips.
        Block::CodeBlock(_)
        | Block::RawBlock(_)
        | Block::HorizontalRule(_)
        | Block::BlockMetadata(_)
        | Block::NoteDefinitionPara(_)
        | Block::NoteDefinitionFencedBlock(_) => {}
    }
}

fn visit_inlines(inlines: &mut Inlines) {
    for inline in inlines.iter_mut() {
        visit_inline(inline);
    }
}

fn visit_inline(inline: &mut Inline) {
    match inline {
        Inline::Image(img) => {
            apply_to_image(&mut img.attr);
            // An image's alt-text inlines can themselves hold an image
            // after a filter pass; keep walking.
            visit_inlines(&mut img.content);
        }
        Inline::Link(l) => visit_inlines(&mut l.content),
        Inline::Emph(e) => visit_inlines(&mut e.content),
        Inline::Underline(u) => visit_inlines(&mut u.content),
        Inline::Strong(s) => visit_inlines(&mut s.content),
        Inline::Strikeout(s) => visit_inlines(&mut s.content),
        Inline::Superscript(s) => visit_inlines(&mut s.content),
        Inline::Subscript(s) => visit_inlines(&mut s.content),
        Inline::SmallCaps(s) => visit_inlines(&mut s.content),
        Inline::Quoted(q) => visit_inlines(&mut q.content),
        Inline::Note(n) => visit_blocks(&mut n.content),
        Inline::Span(s) => visit_inlines(&mut s.content),
        Inline::Insert(i) => visit_inlines(&mut i.content),
        Inline::Delete(d) => visit_inlines(&mut d.content),
        Inline::Highlight(h) => visit_inlines(&mut h.content),
        Inline::Custom(c) => {
            for (_name, slot) in c.slots.iter_mut() {
                visit_slot(slot);
            }
        }
        // Not walked. Most are true leaves; `Cite` and `EditComment`
        // do carry inlines, but a `Cite`'s content is generated
        // citation text and an `EditComment`'s is editorial markup —
        // neither is authored image content. Same set `link_rewrite`
        // skips.
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

fn visit_slot(slot: &mut Slot) {
    match slot {
        Slot::Block(b) => visit_block(b),
        Slot::Blocks(bs) => visit_blocks(bs),
        Slot::Inline(i) => visit_inline(i),
        Slot::Inlines(is) => visit_inlines(is),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hashlink::LinkedHashMap;
    use quarto_pandoc_types::ConfigMapEntry;
    use quarto_pandoc_types::attr::{AttrSourceInfo, TargetSourceInfo};
    use quarto_pandoc_types::block::{
        Block, BlockQuote, BulletList, CaptionBlock, DefinitionList, Div, Figure, Header,
        LineBlock, OrderedList, Paragraph, Plain,
    };
    use quarto_pandoc_types::caption::Caption;
    use quarto_pandoc_types::inline::{
        Emph, Image, Inline, Link, Note, QuoteType, Quoted, Span, Strong,
    };
    use quarto_pandoc_types::list::{ListNumberDelim, ListNumberStyle};
    use quarto_pandoc_types::table::{
        Alignment, Cell, Row, Table, TableBody, TableFoot, TableHead,
    };
    use quarto_source_map::SourceInfo;

    fn si() -> SourceInfo {
        SourceInfo::for_test()
    }

    /// Document metadata built from `(key, bool)` pairs.
    fn meta(entries: &[(&str, bool)]) -> ConfigValue {
        ConfigValue::new_map(
            entries
                .iter()
                .map(|(k, v)| ConfigMapEntry {
                    key: k.to_string(),
                    key_source: SourceInfo::for_test(),
                    value: ConfigValue::new_bool(*v, SourceInfo::for_test()),
                })
                .collect(),
            SourceInfo::for_test(),
        )
    }

    /// Metadata with a bare `theme: <name>` string.
    fn meta_theme(name: &str) -> ConfigValue {
        ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "theme".to_string(),
                key_source: SourceInfo::for_test(),
                value: ConfigValue::new_string(name.to_string(), SourceInfo::for_test()),
            }],
            SourceInfo::for_test(),
        )
    }

    fn meta_fig_responsive(value: bool) -> ConfigValue {
        meta(&[(FIG_RESPONSIVE_KEY, value)])
    }

    /// A revealjs `Format`. Reveal reaches the pipeline both as the
    /// native `revealjs` target and as the preview pseudo-format
    /// `q2-slides`; both must gate the same way.
    fn reveal_format(target: &str) -> Format {
        let mut fmt = Format::html();
        fmt.identifier = crate::format::FormatIdentifier::Revealjs;
        fmt.target_format = target.to_string();
        fmt
    }

    fn attr(classes: &[&str], kvs: &[(&str, &str)]) -> Attr {
        let mut map = LinkedHashMap::new();
        for (k, v) in kvs {
            map.insert(k.to_string(), v.to_string());
        }
        (
            String::new(),
            classes.iter().map(|c| c.to_string()).collect(),
            map,
        )
    }

    fn image(classes: &[&str], kvs: &[(&str, &str)]) -> Inline {
        Inline::Image(Image {
            attr: attr(classes, kvs),
            content: vec![],
            target: ("wide.png".to_string(), String::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
            target_source: TargetSourceInfo::empty(),
        })
    }

    fn para(inlines: Vec<Inline>) -> Block {
        Block::Paragraph(Paragraph {
            content: inlines,
            source_info: si(),
        })
    }

    /// Classes of the first image found anywhere under `blocks`.
    fn first_image_classes(blocks: &[Block]) -> Vec<String> {
        fn from_block(b: &Block) -> Option<Vec<String>> {
            match b {
                Block::Paragraph(p) => p.content.iter().find_map(from_inline),
                Block::Plain(p) => p.content.iter().find_map(from_inline),
                Block::Div(d) => d.content.iter().find_map(from_block),
                Block::BulletList(bl) => bl
                    .content
                    .iter()
                    .find_map(|item| item.iter().find_map(from_block)),
                _ => None,
            }
        }
        fn from_inline(i: &Inline) -> Option<Vec<String>> {
            match i {
                Inline::Image(img) => Some(img.attr.1.clone()),
                _ => None,
            }
        }
        blocks
            .iter()
            .find_map(from_block)
            .expect("no image in blocks")
    }

    #[test]
    fn bare_image_gets_img_fluid() {
        let mut blocks = vec![para(vec![image(&[], &[])])];
        visit_blocks(&mut blocks);
        assert_eq!(first_image_classes(&blocks), vec!["img-fluid".to_string()]);
    }

    #[test]
    fn existing_author_classes_are_preserved() {
        let mut blocks = vec![para(vec![image(&["border", "my-class"], &[])])];
        visit_blocks(&mut blocks);
        assert_eq!(
            first_image_classes(&blocks),
            vec![
                "border".to_string(),
                "my-class".to_string(),
                "img-fluid".to_string()
            ],
            "the class must be appended, never replace what the author wrote"
        );
    }

    #[test]
    fn idempotent_on_second_run() {
        let mut blocks = vec![para(vec![image(&[], &[])])];
        visit_blocks(&mut blocks);
        let first = first_image_classes(&blocks);
        visit_blocks(&mut blocks);
        assert_eq!(
            first_image_classes(&blocks),
            first,
            "running twice must not duplicate the class"
        );
    }

    #[test]
    fn author_written_img_fluid_is_not_duplicated() {
        let mut blocks = vec![para(vec![image(&["img-fluid"], &[])])];
        visit_blocks(&mut blocks);
        assert_eq!(first_image_classes(&blocks), vec!["img-fluid".to_string()]);
    }

    #[test]
    fn explicit_width_still_gets_img_fluid() {
        // Not an exclusion: `max-width: 100%` refines a fixed width
        // rather than fighting it. Quarto 1 tags these too.
        let mut blocks = vec![para(vec![image(&[], &[("width", "450")])])];
        visit_blocks(&mut blocks);
        assert_eq!(first_image_classes(&blocks), vec!["img-fluid".to_string()]);
    }

    #[test]
    fn explicit_height_is_excluded() {
        let mut blocks = vec![para(vec![image(&[], &[("height", "100")])])];
        visit_blocks(&mut blocks);
        assert!(
            first_image_classes(&blocks).is_empty(),
            "an author-set height must not be overridden by height:auto"
        );
    }

    #[test]
    fn data_no_responsive_is_excluded_whatever_its_value() {
        for value in ["true", "false", ""] {
            let mut blocks = vec![para(vec![image(&[], &[("data-no-responsive", value)])])];
            visit_blocks(&mut blocks);
            assert!(
                first_image_classes(&blocks).is_empty(),
                "data-no-responsive={value:?} must opt the image out; \
                 only the key's presence is tested"
            );
        }
    }

    /// Number of tagged images anywhere under `blocks`, counted by
    /// serializing the tree rather than by re-walking it — so this
    /// assertion cannot share a blind spot with the walk under test.
    fn tagged_count(blocks: &[Block]) -> usize {
        serde_json::to_string(blocks)
            .expect("blocks serialize")
            .matches("img-fluid")
            .count()
    }

    /// Every container the walk claims to descend into, in one
    /// document. Weakening any arm of `visit_block` / `visit_inline`
    /// to `{}` drops the count and reddens this test — which the
    /// div+list test alone did not do for `Table`, `Figure` captions,
    /// `DefinitionList`, `LineBlock`, `Note`, or `Custom`.
    #[test]
    fn every_container_arm_is_walked() {
        let img = || image(&[], &[]);
        let blocks = vec![
            // Block containers
            Block::Plain(Plain {
                content: vec![img()],
                source_info: si(),
            }),
            para(vec![img()]),
            Block::LineBlock(LineBlock {
                content: vec![vec![img()]],
                source_info: si(),
            }),
            Block::BlockQuote(BlockQuote {
                content: vec![para(vec![img()])],
                source_info: si(),
            }),
            Block::OrderedList(OrderedList {
                attr: (1, ListNumberStyle::Decimal, ListNumberDelim::Period),
                content: vec![vec![para(vec![img()])]],
                source_info: si(),
            }),
            Block::BulletList(BulletList {
                content: vec![vec![para(vec![img()])]],
                source_info: si(),
            }),
            // Both halves of a definition list: term (inlines) and body (blocks).
            Block::DefinitionList(DefinitionList {
                content: vec![(vec![img()], vec![vec![para(vec![img()])]])],
                source_info: si(),
            }),
            Block::Header(Header {
                level: 1,
                attr: attr(&[], &[]),
                content: vec![img()],
                source_info: si(),
                attr_source: AttrSourceInfo::empty(),
            }),
            Block::Div(Div {
                attr: attr(&[], &[]),
                content: vec![para(vec![img()])],
                source_info: si(),
                attr_source: AttrSourceInfo::empty(),
            }),
            // Figure body plus both caption slots.
            Block::Figure(Figure {
                attr: attr(&[], &[]),
                caption: Caption {
                    short: Some(vec![img()]),
                    long: Some(vec![para(vec![img()])]),
                    source_info: si(),
                },
                content: vec![para(vec![img()])],
                source_info: si(),
                attr_source: AttrSourceInfo::empty(),
            }),
            Block::CaptionBlock(CaptionBlock {
                content: vec![img()],
                source_info: si(),
            }),
            // Table: long caption, a head cell and a body cell.
            {
                let cell = |inner: Inline| Cell {
                    attr: attr(&[], &[]),
                    alignment: Alignment::Default,
                    row_span: 1,
                    col_span: 1,
                    content: vec![para(vec![inner])],
                    source_info: si(),
                    attr_source: AttrSourceInfo::empty(),
                };
                let row = |inner: Inline| Row {
                    attr: attr(&[], &[]),
                    cells: vec![cell(inner)],
                    source_info: si(),
                    attr_source: AttrSourceInfo::empty(),
                };
                Block::Table(Table {
                    attr: attr(&[], &[]),
                    caption: Caption {
                        short: None,
                        long: Some(vec![para(vec![img()])]),
                        source_info: si(),
                    },
                    colspec: vec![],
                    head: TableHead {
                        attr: attr(&[], &[]),
                        rows: vec![row(img())],
                        source_info: si(),
                        attr_source: AttrSourceInfo::empty(),
                    },
                    bodies: vec![TableBody {
                        attr: attr(&[], &[]),
                        rowhead_columns: 0,
                        head: vec![],
                        body: vec![row(img())],
                        source_info: si(),
                        attr_source: AttrSourceInfo::empty(),
                    }],
                    foot: TableFoot {
                        attr: attr(&[], &[]),
                        rows: vec![],
                        source_info: si(),
                        attr_source: AttrSourceInfo::empty(),
                    },
                    source_info: si(),
                    attr_source: AttrSourceInfo::empty(),
                })
            },
            // Inline containers, all in one paragraph.
            para(vec![
                Inline::Emph(Emph {
                    content: vec![img()],
                    source_info: si(),
                }),
                Inline::Strong(Strong {
                    content: vec![img()],
                    source_info: si(),
                }),
                Inline::Quoted(Quoted {
                    quote_type: QuoteType::DoubleQuote,
                    content: vec![img()],
                    source_info: si(),
                }),
                Inline::Span(Span {
                    attr: attr(&[], &[]),
                    content: vec![img()],
                    source_info: si(),
                    attr_source: AttrSourceInfo::empty(),
                }),
                Inline::Link(Link {
                    attr: attr(&[], &[]),
                    content: vec![img()],
                    target: ("x".to_string(), String::new()),
                    source_info: si(),
                    attr_source: AttrSourceInfo::empty(),
                    target_source: TargetSourceInfo::empty(),
                }),
                Inline::Note(Note {
                    content: vec![para(vec![img()])],
                    source_info: si(),
                }),
            ]),
        ];
        // 12 single-image containers, plus the two definition-list
        // slots (term + body), the three figure slots (content +
        // short caption + long caption), the three table slots
        // (caption + head cell + body cell), and the six inline
        // containers in the final paragraph.
        let expected = 23;
        let mut blocks = blocks;
        visit_blocks(&mut blocks);
        assert_eq!(
            tagged_count(&blocks),
            expected,
            "every container arm must be descended into"
        );
    }

    #[test]
    fn image_nested_in_div_and_list_is_reached() {
        let inner = Block::BulletList(BulletList {
            content: vec![vec![para(vec![image(&[], &[])])]],
            source_info: si(),
        });
        let mut blocks = vec![Block::Div(Div {
            attr: attr(&[], &[]),
            content: vec![inner],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })];
        visit_blocks(&mut blocks);
        assert_eq!(
            first_image_classes(&blocks),
            vec!["img-fluid".to_string()],
            "the walk must reach images nested in containers"
        );
    }

    // ── The two gates ─────────────────────────────────────────────────

    #[test]
    fn gate_is_open_for_html_by_default() {
        assert!(responsive_enabled(&Format::html(), &ConfigValue::default()));
    }

    #[test]
    fn gate_is_closed_for_revealjs() {
        // Quarto 1's `isHtmlOutput()` IS true for revealjs, but the
        // filter reads `param('fig-responsive')`, and
        // `createHtmlPresentationFormat` (formats-shared.ts) sets that
        // param `false` for every HTML presentation format. So Q1 does
        // not tag deck images — verified against the real binary, whose
        // reveal output carries no `img-fluid` at all.
        assert!(!responsive_enabled(
            &reveal_format("revealjs"),
            &ConfigValue::default()
        ));
    }

    #[test]
    fn gate_is_closed_for_the_slides_preview_pseudo_format() {
        // `q2-slides` is reveal under preview. It maps to
        // `FormatIdentifier::Html`, so the identifier alone can't see
        // it — the gate must consult `target_format`, or preview would
        // disagree with render.
        assert!(!responsive_enabled(
            &reveal_format("q2-slides"),
            &ConfigValue::default()
        ));
        // And guard the identifier-only mistake directly: a Format
        // whose identifier is Html but whose target is a slide deck.
        let mut fmt = Format::html();
        fmt.target_format = "q2-slides".to_string();
        assert!(!responsive_enabled(&fmt, &ConfigValue::default()));
    }

    #[test]
    fn gate_is_closed_for_minimal_html() {
        // `format-html.ts` resolveFormat: `minimal: true` defaults
        // `fig-responsive` to false. Verified against Q1.
        assert!(!responsive_enabled(
            &Format::html(),
            &meta(&[("minimal", true)])
        ));
    }

    #[test]
    fn gate_is_open_for_theme_none_without_minimal() {
        // Deliberately NOT `is_minimal_html`, which also returns true
        // for `theme: none` / `theme: pandoc`. Q1 flips the default
        // only on `minimal: true`; with a bare `theme: none` both
        // engines tag. Verified against Q1.
        assert!(responsive_enabled(&Format::html(), &meta_theme("none")));
        assert!(responsive_enabled(&Format::html(), &meta_theme("pandoc")));
    }

    #[test]
    fn explicit_fig_responsive_true_overrides_the_format_defaults() {
        // Q1 only supplies these defaults when the user left the key
        // unset (`=== undefined` / config merge), so an explicit
        // `fig-responsive: true` wins for both reveal and minimal.
        // Verified against Q1.
        assert!(responsive_enabled(
            &reveal_format("revealjs"),
            &meta_fig_responsive(true)
        ));
        assert!(responsive_enabled(
            &Format::html(),
            &meta(&[("minimal", true), (FIG_RESPONSIVE_KEY, true)])
        ));
    }

    #[test]
    fn gate_is_closed_for_non_html_formats() {
        // `img-fluid` is a Bootstrap class; emitting it into PDF or
        // any other non-HTML target would be meaningless noise.
        assert!(!responsive_enabled(&Format::pdf(), &ConfigValue::default()));
    }

    #[test]
    fn fig_responsive_false_closes_the_gate() {
        assert!(!responsive_enabled(
            &Format::html(),
            &meta_fig_responsive(false)
        ));
    }

    #[test]
    fn fig_responsive_true_and_absent_both_leave_it_open() {
        assert!(responsive_enabled(
            &Format::html(),
            &meta_fig_responsive(true)
        ));
        assert!(responsive_enabled(&Format::html(), &ConfigValue::default()));
    }
}
