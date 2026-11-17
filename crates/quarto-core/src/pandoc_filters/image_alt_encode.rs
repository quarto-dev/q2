//! Respell a standalone image's alt text the way Quarto 1's Lua filters
//! read it (bd-ykjwiqcc, GH #877).
//!
//! q2's canonical AST keeps an image's alt text in its alt inlines
//! (`Image.content`) — Pandoc 3's model, produced by
//! `transforms::image_alt_normalize`. Pandoc's writers and q2's own read it
//! there. The vendored Q1 filters that run on the Pandoc-hybrid leg do not
//! always: `quarto-pre/parsefiguredivs.lua`'s `Para` handler, via
//! `discoverFigure` / `discoverLinkedFigure`, still applies the Pandoc 2
//! rule that a paragraph holding nothing but an image (or a link around
//! one) *with alt text* is an implicit figure, whose caption is that alt
//! text. Handed `Para [Image "Alt A"]`, typst and docx would render a
//! numbered "Figure 1: Alt A" where the author wrote a plain image.
//!
//! q2 never hands those filters an implicit figure in that shape: its
//! reader turns every captioned single-image paragraph into a `Figure`.
//! So a *standalone* image with alt inlines at the cut is always a plain
//! image, and Quarto 1 spells that as an empty-alt image carrying
//! `fig-alt` — exactly what its own reader produces for
//! `![](x.png){fig-alt="…"}`, and what its late consumers read
//! (`quarto-post/typst.lua`, `quarto-post/docx.lua`'s `render_docx_fixups`,
//! `quarto-post/html.lua` for epub). This pass rewrites that one shape
//! into that spelling, immediately before serialization. Every other image
//! keeps its alt inlines: Pandoc's writers read them, and the Lua filters
//! don't read them as captions.

use quarto_pandoc_types::block::Block;
use quarto_pandoc_types::config_value::inlines_to_plain_text;

use crate::transforms::image_alt_normalize::FIG_ALT_ATTR;
use crate::transforms::image_walk::for_each_image_with_context_mut;

/// Move each standalone image's alt inlines into `fig-alt` (unless it
/// already has one, which Quarto 1 prefers anyway) and empty them.
pub fn encode_standalone_image_alt(blocks: &mut [Block]) {
    for_each_image_with_context_mut(
        blocks,
        &mut |image, context| {
            if !context.standalone || image.content.is_empty() {
                return;
            }
            if !image.attr.2.contains_key(FIG_ALT_ATTR) {
                let tracked = image.attr_source.attributes.len() == image.attr.2.len();
                image.attr.2.insert(
                    FIG_ALT_ATTR.to_string(),
                    inlines_to_plain_text(&image.content),
                );
                if tracked {
                    image.attr_source.attributes.push((None, None));
                }
            }
            image.content.clear();
        },
        &mut |_float| {},
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use hashlink::LinkedHashMap;
    use quarto_pandoc_types::attr::{AttrSourceInfo, TargetSourceInfo};
    use quarto_pandoc_types::block::{Figure, Paragraph, Plain};
    use quarto_pandoc_types::caption::Caption;
    use quarto_pandoc_types::inline::{Image, Inline, Link, Str, split_string_to_inlines};
    use quarto_source_map::SourceInfo;

    fn si() -> SourceInfo {
        SourceInfo::for_test()
    }

    fn image(alt: &str, kvs: &[(&str, &str)]) -> Inline {
        Inline::Image(Box::new(Image {
            attr: (
                String::new(),
                vec![],
                kvs.iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect::<LinkedHashMap<_, _>>(),
            )
                .into(),
            content: split_string_to_inlines(alt),
            target: ("x.png".to_string(), String::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
            target_source: TargetSourceInfo::empty(),
        }))
    }

    fn para(content: Vec<Inline>) -> Block {
        Block::Paragraph(Paragraph {
            content,
            source_info: si(),
        })
    }

    fn word(s: &str) -> Inline {
        Inline::Str(Str {
            text: s.to_string(),
            source_info: si(),
        })
    }

    /// (alt plain text, fig-alt) of every image, in document order.
    fn summary(mut blocks: Vec<Block>) -> Vec<(String, Option<String>)> {
        encode_standalone_image_alt(&mut blocks);
        let mut out = Vec::new();
        crate::transforms::image_walk::for_each_image_mut(&mut blocks, &mut |img| {
            out.push((
                inlines_to_plain_text(&img.content),
                img.attr.2.get(FIG_ALT_ATTR).cloned(),
            ));
        });
        out
    }

    #[test]
    fn standalone_image_alt_moves_to_fig_alt() {
        assert_eq!(
            summary(vec![para(vec![image("Alt A", &[])])]),
            vec![(String::new(), Some("Alt A".into()))]
        );
    }

    #[test]
    fn standalone_linked_image_alt_moves_to_fig_alt() {
        let link = Inline::Link(Box::new(Link {
            attr: (String::new(), vec![], LinkedHashMap::new()).into(),
            content: vec![image("Alt L", &[])],
            target: ("https://example.com".into(), String::new()),
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
            target_source: TargetSourceInfo::empty(),
        }));
        assert_eq!(
            summary(vec![para(vec![link])]),
            vec![(String::new(), Some("Alt L".into()))]
        );
    }

    #[test]
    fn an_existing_fig_alt_wins_and_the_inlines_still_empty() {
        assert_eq!(
            summary(vec![para(vec![image("inlines", &[("fig-alt", "attr")])])]),
            vec![(String::new(), Some("attr".into()))]
        );
    }

    #[test]
    fn inline_and_figure_images_keep_their_alt_inlines() {
        let figure = Block::Figure(Box::new(Figure {
            attr: (String::new(), vec![], LinkedHashMap::new()).into(),
            caption: Caption {
                short: None,
                long: Some(vec![Block::Plain(Plain {
                    content: split_string_to_inlines("Caption B"),
                    source_info: si(),
                })]),
                source_info: si(),
            },
            content: vec![Block::Plain(Plain {
                content: vec![image("Alt B", &[])],
                source_info: si(),
            })],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        }));
        assert_eq!(
            summary(vec![
                para(vec![word("Text"), image("Alt D", &[])]),
                figure,
                para(vec![image("", &[])]),
            ]),
            vec![
                ("Alt D".into(), None),
                ("Alt B".into(), None),
                (String::new(), None),
            ]
        );
    }
}
