/*
 * transforms/image_alt_normalize.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Fold `fig-alt` (and a bare `alt` key-value) into an image's alt text.
//!
//! The canonical home of an image's alt text is its alt inlines
//! (`Image.content`). That is Pandoc's model — its reader folds
//! `{alt="…"}` on an implicit figure into them, and a `Figure`'s caption
//! is a separate field — and every writer q2 reaches reads it there: the
//! HTML writer's `alt=`, the q2-preview `Image.tsx`, Pandoc's docx
//! `descr`, and the vendored `typst.lua`'s caption-as-alt fallback.
//! `fig-alt` is Quarto's author-facing override for it; left as an
//! attribute, the HTML writer data-prefixes it into `data-fig-alt` and the
//! alt text stays empty or a copy of the caption (GH #877, bd-ykjwiqcc).
//!
//! Per image, in priority order:
//!
//! 1. the image's own `fig-alt`;
//! 2. the image's own `alt` key-value — Pandoc's form (a known HTML
//!    attribute, so left in place it would make the writer emit `alt`
//!    twice);
//! 3. the `fig-alt` of the `FloatRefTarget` whose `content` slot holds
//!    the image (`::: {#fig-x fig-alt="…"}`), for `fig` floats only, as in
//!    Q1's `quarto-pre/figures.lua`. The nearest float wins: a subfigure's
//!    image takes its own float's `fig-alt`, never its parent's.
//!
//! The winning text replaces the alt inlines (an empty value means a
//! decorative image: `alt=""`), and both keys are removed from the image
//! and `fig-alt` from the float. Images without either key are left
//! alone — an implicit figure keeps its caption as alt text; whether to
//! *present* that duplicate is the writer's call.
//!
//! **Placement.** End of the Normalization Phase, right after
//! `float-ref-target-sugar`: every image source has run by then (engine
//! output, includes, shortcodes, user pre-filters, the book cover), every
//! float is a `FloatRefTarget` with its attributes still in hand, and
//! every consumer — crossref index/render, llms capture, the preview, the
//! Pandoc cut — sees one shape. User *post*-filters therefore see the alt
//! inlines (`image.caption` in Lua), not `fig-alt`. See
//! `claude-notes/plans/2026-10-21-fig-alt-to-alt.md` § "Placement".
//!
//! Not done in the reader: the qmd writer's implicit-figure round-trip
//! depends on the reader's caption == alt shape.

use quarto_pandoc_types::attr::{Attr, AttrSourceInfo};
use quarto_pandoc_types::block::Block;
use quarto_pandoc_types::inline::{Image, Inline, Inlines, split_string_to_inlines};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceInfo;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};
use crate::transforms::image_walk::{ImageContext, for_each_image_with_context_mut};

/// Quarto's alt-text override attribute.
pub(crate) const FIG_ALT_ATTR: &str = "fig-alt";
/// Pandoc's alt-text attribute.
const ALT_ATTR: &str = "alt";

pub struct ImageAltNormalizeTransform;

impl ImageAltNormalizeTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ImageAltNormalizeTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for ImageAltNormalizeTransform {
    fn name(&self) -> &str {
        "image-alt-normalize"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Normalization
    }

    async fn transform(&self, ast: &mut Pandoc, _ctx: &mut RenderContext) -> Result<()> {
        normalize_blocks(&mut ast.blocks);
        Ok(())
    }
}

fn normalize_blocks(blocks: &mut [Block]) {
    for_each_image_with_context_mut(blocks, &mut normalize_image, &mut |float| {
        if is_figure_float(&float.plain_data) {
            take_attr(&mut float.attr, None, FIG_ALT_ATTR);
        }
    });
}

fn normalize_image(image: &mut Image, context: ImageContext<'_>) {
    let fig_alt = take_attr(&mut image.attr, Some(&mut image.attr_source), FIG_ALT_ATTR);
    let alt = take_attr(&mut image.attr, Some(&mut image.attr_source), ALT_ATTR);
    let chosen = fig_alt.or(alt).or_else(|| {
        context
            .float
            .filter(|s| is_figure_float(s.plain_data))
            .and_then(|s| s.attr.2.get(FIG_ALT_ATTR))
            .map(|text| (text.clone(), None))
    });
    if let Some((text, source)) = chosen {
        image.content = alt_inlines(&text, source);
    }
}

/// `FloatRefTarget.plain_data.ref_type == "fig"`.
fn is_figure_float(plain_data: &serde_json::Value) -> bool {
    plain_data.get("ref_type").and_then(|v| v.as_str()) == Some("fig")
}

/// Remove `key` from `attr`'s key-values, returning its value and, when
/// `attr_source` is tracked in step with them, the value's source. The
/// parallel `attr_source.attributes` entry is removed too, so the two stay
/// aligned by index.
fn take_attr(
    attr: &mut Attr,
    attr_source: Option<&mut AttrSourceInfo>,
    key: &str,
) -> Option<(String, Option<SourceInfo>)> {
    let index = attr.2.keys().position(|k| k == key)?;
    let tracked = attr_source.filter(|s| s.attributes.len() == attr.2.len());
    let value = attr.2.remove(key)?;
    let source = tracked.and_then(|s| s.attributes.remove(index).1);
    Some((value, source))
}

/// The alt text as Pandoc would read it (`B.text`: `Str` runs separated
/// by `Space`/`SoftBreak`), attributed to the attribute value it came from.
fn alt_inlines(text: &str, source: Option<SourceInfo>) -> Inlines {
    let mut inlines = split_string_to_inlines(text);
    if let Some(source) = source {
        for inline in inlines.iter_mut() {
            match inline {
                Inline::Str(s) => s.source_info = source.clone(),
                Inline::Space(s) => s.source_info = source.clone(),
                Inline::SoftBreak(s) => s.source_info = source.clone(),
                _ => {}
            }
        }
    }
    inlines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crossref::FLOAT_REF_TARGET;
    use hashlink::LinkedHashMap;
    use quarto_pandoc_types::attr::TargetSourceInfo;
    use quarto_pandoc_types::block::{Figure, Paragraph, Plain};
    use quarto_pandoc_types::caption::Caption;
    use quarto_pandoc_types::config_value::inlines_to_plain_text;
    use quarto_pandoc_types::custom::{CustomNode, Slot};
    use quarto_pandoc_types::inline::Note;
    use quarto_pandoc_types::table::{
        Alignment, Cell, ColWidth, Row, Table, TableBody, TableFoot, TableHead,
    };
    use serde_json::json;

    fn si() -> SourceInfo {
        SourceInfo::for_test()
    }

    fn text(s: &str) -> Inlines {
        split_string_to_inlines(s)
    }

    fn image(alt: &str, kvs: &[(&str, &str)]) -> Image {
        let mut map = LinkedHashMap::new();
        for (k, v) in kvs {
            map.insert(k.to_string(), v.to_string());
        }
        Image {
            attr: (String::new(), vec![], map),
            content: text(alt),
            target: ("x.png".to_string(), String::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
            target_source: TargetSourceInfo::empty(),
        }
    }

    fn para(inlines: Inlines) -> Block {
        Block::Paragraph(Paragraph {
            content: inlines,
            source_info: si(),
        })
    }

    fn plain(inlines: Inlines) -> Block {
        Block::Plain(Plain {
            content: inlines,
            source_info: si(),
        })
    }

    /// The implicit figure the reader builds for `![caption](x.png){…}`.
    fn implicit_figure(caption: &str, kvs: &[(&str, &str)]) -> Block {
        Block::Figure(Figure {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            caption: Caption {
                short: None,
                long: Some(vec![plain(text(caption))]),
                source_info: si(),
            },
            content: vec![plain(vec![Inline::Image(Box::new(image(caption, kvs)))])],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    fn float(ref_type: &str, kvs: &[(&str, &str)], content: Vec<Block>, caption: &str) -> Block {
        let mut map = LinkedHashMap::new();
        for (k, v) in kvs {
            map.insert(k.to_string(), v.to_string());
        }
        let node = CustomNode::new(
            FLOAT_REF_TARGET,
            (format!("{ref_type}-x"), vec![], map),
            si(),
        )
        .with_data(json!({ "ref_type": ref_type, "kind": "Figure", "identifier": format!("{ref_type}-x") }))
        .with_slot("content", Slot::Blocks(content))
        .with_slot("caption_long", Slot::Blocks(vec![plain(text(caption))]));
        Block::Custom(node)
    }

    /// Every image in document order: (alt text, remaining key-values).
    fn images(blocks: &mut [Block]) -> Vec<(String, Vec<(String, String)>)> {
        let mut out = Vec::new();
        crate::transforms::image_walk::for_each_image_mut(blocks, &mut |img| {
            out.push((
                inlines_to_plain_text(&img.content),
                img.attr
                    .2
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            ))
        });
        out
    }

    fn run(mut blocks: Vec<Block>) -> Vec<Block> {
        normalize_blocks(&mut blocks);
        blocks
    }

    #[test]
    fn fig_alt_on_a_bare_image_becomes_its_alt_text() {
        let mut out = run(vec![para(vec![Inline::Image(Box::new(image(
            "",
            &[("fig-alt", "Alt A"), ("width", "50%")],
        )))])]);
        assert_eq!(
            images(&mut out),
            vec![("Alt A".into(), vec![("width".into(), "50%".into())])]
        );
    }

    #[test]
    fn alt_text_is_tokenized_like_pandoc() {
        let mut out = run(vec![para(vec![Inline::Image(Box::new(image(
            "",
            &[("fig-alt", "Two words")],
        )))])]);
        let Block::Paragraph(p) = &mut out[0] else {
            panic!()
        };
        let Inline::Image(img) = &p.content[0] else {
            panic!()
        };
        assert!(matches!(
            img.content.as_slice(),
            [Inline::Str(a), Inline::Space(_), Inline::Str(b)] if a.text == "Two" && b.text == "words"
        ));
    }

    #[test]
    fn fig_alt_on_an_implicit_figure_replaces_alt_but_not_caption() {
        let mut out = run(vec![implicit_figure("Caption B", &[("fig-alt", "Alt B")])]);
        let Block::Figure(fig) = &out[0] else {
            panic!()
        };
        let caption = fig.caption.long.as_ref().unwrap();
        let Block::Plain(cap) = &caption[0] else {
            panic!()
        };
        assert_eq!(inlines_to_plain_text(&cap.content), "Caption B");
        assert_eq!(images(&mut out), vec![("Alt B".into(), vec![])]);
    }

    #[test]
    fn alt_key_value_is_folded_like_pandoc() {
        let mut out = run(vec![para(vec![Inline::Image(Box::new(image(
            "",
            &[("alt", "KV A")],
        )))])]);
        assert_eq!(images(&mut out), vec![("KV A".into(), vec![])]);
    }

    #[test]
    fn fig_alt_beats_alt_and_both_are_removed() {
        let mut out = run(vec![para(vec![Inline::Image(Box::new(image(
            "",
            &[("alt", "loses"), ("fig-alt", "wins")],
        )))])]);
        assert_eq!(images(&mut out), vec![("wins".into(), vec![])]);
    }

    #[test]
    fn empty_fig_alt_makes_the_image_decorative() {
        let mut out = run(vec![implicit_figure("Cap I", &[("fig-alt", "")])]);
        assert_eq!(images(&mut out), vec![(String::new(), vec![])]);
    }

    #[test]
    fn image_without_either_key_is_untouched() {
        let mut out = run(vec![implicit_figure("Alt C", &[("width", "3in")])]);
        assert_eq!(
            images(&mut out),
            vec![("Alt C".into(), vec![("width".into(), "3in".into())])]
        );
    }

    #[test]
    fn attr_source_stays_aligned_when_a_key_is_removed() {
        let mut img = image("", &[("width", "1"), ("fig-alt", "A"), ("height", "2")]);
        let w = SourceInfo::original(quarto_source_map::FileId(0), 1, 2);
        let fa = SourceInfo::original(quarto_source_map::FileId(0), 3, 4);
        let h = SourceInfo::original(quarto_source_map::FileId(0), 5, 6);
        img.attr_source.attributes = vec![
            (None, Some(w.clone())),
            (None, Some(fa.clone())),
            (None, Some(h.clone())),
        ];
        normalize_image(
            &mut img,
            ImageContext {
                float: None,
                standalone: false,
            },
        );
        assert_eq!(
            img.attr_source.attributes,
            vec![(None, Some(w)), (None, Some(h))]
        );
        let Inline::Str(s) = &img.content[0] else {
            panic!()
        };
        assert_eq!(s.source_info, fa, "alt text points at the authored value");
    }

    #[test]
    fn images_in_tables_and_footnotes_are_reached() {
        let cell = Cell {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            alignment: Alignment::Default,
            row_span: 1,
            col_span: 1,
            content: vec![plain(vec![Inline::Image(Box::new(image(
                "",
                &[("fig-alt", "In cell")],
            )))])],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        };
        let table = Block::Table(Table {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            caption: Caption {
                short: None,
                long: None,
                source_info: si(),
            },
            colspec: vec![(Alignment::Default, ColWidth::Default)],
            head: TableHead {
                attr: (String::new(), vec![], LinkedHashMap::new()),
                rows: vec![],
                source_info: si(),
                attr_source: AttrSourceInfo::empty(),
            },
            bodies: vec![TableBody {
                attr: (String::new(), vec![], LinkedHashMap::new()),
                rowhead_columns: 0,
                head: vec![],
                body: vec![Row {
                    attr: (String::new(), vec![], LinkedHashMap::new()),
                    cells: vec![cell],
                    source_info: si(),
                    attr_source: AttrSourceInfo::empty(),
                }],
                source_info: si(),
                attr_source: AttrSourceInfo::empty(),
            }],
            foot: TableFoot {
                attr: (String::new(), vec![], LinkedHashMap::new()),
                rows: vec![],
                source_info: si(),
                attr_source: AttrSourceInfo::empty(),
            },
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        });
        let note = para(vec![Inline::Note(Note {
            content: vec![para(vec![Inline::Image(Box::new(image(
                "",
                &[("fig-alt", "In note")],
            )))])],
            source_info: si(),
        })]);
        let mut out = run(vec![table, note]);
        assert_eq!(
            images(&mut out),
            vec![("In cell".into(), vec![]), ("In note".into(), vec![])]
        );
    }

    #[test]
    fn float_fig_alt_reaches_its_image_and_leaves_the_float() {
        let mut out = run(vec![float(
            "fig",
            &[("fig-alt", "Div alt")],
            vec![plain(vec![Inline::Image(Box::new(image("", &[])))])],
            "Div caption",
        )]);
        let Block::Custom(node) = &out[0] else {
            panic!()
        };
        assert!(!node.attr.2.contains_key("fig-alt"));
        assert_eq!(images(&mut out), vec![("Div alt".into(), vec![])]);
    }

    #[test]
    fn image_fig_alt_beats_float_fig_alt() {
        let mut out = run(vec![float(
            "fig",
            &[("fig-alt", "Float")],
            vec![plain(vec![
                Inline::Image(Box::new(image("", &[("fig-alt", "Own")]))),
                Inline::Image(Box::new(image("", &[]))),
            ])],
            "cap",
        )]);
        assert_eq!(
            images(&mut out),
            vec![("Own".into(), vec![]), ("Float".into(), vec![])]
        );
    }

    #[test]
    fn nested_float_does_not_inherit_its_parents_fig_alt() {
        let sub = float(
            "fig",
            &[],
            vec![plain(vec![Inline::Image(Box::new(image("", &[])))])],
            "(a)",
        );
        let mut out = run(vec![float(
            "fig",
            &[("fig-alt", "Parent")],
            vec![sub],
            "Both",
        )]);
        assert_eq!(images(&mut out), vec![(String::new(), vec![])]);
    }

    #[test]
    fn float_fig_alt_does_not_reach_images_in_its_caption() {
        let mut node = float("fig", &[("fig-alt", "Float")], vec![], "");
        let Block::Custom(n) = &mut node else {
            panic!()
        };
        n.set_slot(
            "caption_long",
            Slot::Blocks(vec![plain(vec![Inline::Image(Box::new(image(
                "icon",
                &[],
            )))])]),
        );
        let mut out = run(vec![node]);
        assert_eq!(images(&mut out), vec![("icon".into(), vec![])]);
    }

    #[test]
    fn non_figure_float_keeps_its_fig_alt_and_lends_it_to_no_one() {
        let mut out = run(vec![float(
            "tbl",
            &[("fig-alt", "Not mine")],
            vec![plain(vec![Inline::Image(Box::new(image("", &[])))])],
            "cap",
        )]);
        let Block::Custom(node) = &out[0] else {
            panic!()
        };
        assert_eq!(
            node.attr.2.get("fig-alt").map(String::as_str),
            Some("Not mine")
        );
        assert_eq!(images(&mut out), vec![(String::new(), vec![])]);
    }

    /// Plan test 11 (D6): what the qmd writer makes of the canonical shape
    /// reads back, through this transform, into the same AST — the llms
    /// mirror and any other post-transform qmd stay faithful. Oracle: the
    /// read → normalize → write cycle is a fixed point after one turn.
    #[test]
    fn qmd_written_from_the_canonical_shape_is_a_fixed_point() {
        fn cycle(qmd: &str) -> String {
            let (mut doc, _ctx, _) = pampa::readers::qmd::read(
                qmd.as_bytes(),
                false,
                "test.qmd",
                &mut std::io::sink(),
                true,
                None,
            )
            .expect("qmd parses");
            normalize_blocks(&mut doc.blocks);
            let mut out = Vec::new();
            pampa::writers::qmd::write(&doc, &mut out).expect("qmd writes");
            String::from_utf8(out).unwrap()
        }
        let source = "![](a.png){fig-alt=\"Alt A\"}\n\n\
                      ![Caption B](b.png){fig-alt=\"Alt B\"}\n\n\
                      ![Alt C](c.png)\n\n\
                      Text ![](d.png){fig-alt=\"Alt D\"} text.\n\n\
                      ![](h.png){alt=\"KV H\"}\n\n\
                      ![Cap I](i.png){fig-alt=\"\"}\n";
        let once = cycle(source);
        assert_eq!(
            once,
            "![](a.png){fig-alt=\"Alt A\"}\n\n\
             ![Caption B](b.png){fig-alt=\"Alt B\"}\n\n\
             ![Alt C](c.png)\n\n\
             Text ![Alt D](d.png) text.\n\n\
             ![](h.png){fig-alt=\"KV H\"}\n\n\
             ![Cap I](i.png){fig-alt=\"\"}\n"
        );
        assert_eq!(cycle(&once), once);
    }
}
