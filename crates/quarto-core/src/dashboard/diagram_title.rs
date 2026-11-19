/*
 * dashboard/diagram_title.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! A diagram's `title` option as its dashboard card title.
//!
//! Quarto 1 reads `title` on a `{dot}` / `{mermaid}` cell as the title of
//! the cell's card. In q2 the option reader leaves it as a `title`
//! attribute on the diagram code block (`crossref::codeblock_shorthand`),
//! but `mermaid-render` and `graphviz-render` replace that block with a
//! raw `<pre>` that carries no attributes, long before
//! [`DashboardTransform`](super::DashboardTransform) builds cards. So this
//! transform runs just before them and turns a titled diagram into the
//! `.card` div an author would write by hand:
//! `::: {.card title="..."}` around the diagram. The card builder then
//! titles it like any other card.

use quarto_pandoc_types::attr::AttrSourceInfo;
use quarto_pandoc_types::block::{Block, Blocks, Div};
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

/// Wraps each titled diagram block in a titled `.card` div (see the
/// module docs). Dashboard-only; other formats are left alone.
pub struct DashboardDiagramTitleTransform;

impl DashboardDiagramTitleTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DashboardDiagramTitleTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for DashboardDiagramTitleTransform {
    fn name(&self) -> &str {
        "dashboard-diagram-title"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Finalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        if ctx.format.base_format() == "dashboard" {
            wrap_titled_diagrams(&mut ast.blocks);
        }
        Ok(())
    }
}

/// The classes the diagram transforms convert (`mermaid-render`,
/// `graphviz-render`).
fn is_diagram_block(classes: &[String]) -> bool {
    classes.iter().any(|c| c == "mermaid" || c == "dot")
}

/// Walk `blocks` and the divs inside them, except `.card` divs: a diagram
/// already in a card keeps that card's title, and its own is dropped.
fn wrap_titled_diagrams(blocks: &mut Blocks) {
    for block in blocks.iter_mut() {
        match block {
            Block::CodeBlock(cb) if is_diagram_block(&cb.attr.1) => {
                let Some(title) = cb.attr.2.remove("title") else {
                    continue;
                };
                let source_info = cb.source_info.clone();
                let diagram = std::mem::replace(
                    block,
                    Block::Div(Box::new(Div {
                        attr: (
                            String::new(),
                            vec!["card".to_string()],
                            [("title".to_string(), title)].into_iter().collect(),
                        )
                            .into(),
                        content: Vec::new(),
                        source_info,
                        attr_source: AttrSourceInfo::empty(),
                    })),
                );
                if let Block::Div(card) = block {
                    card.content.push(diagram);
                }
            }
            Block::Div(d) if !d.attr.1.iter().any(|c| c == "card") => {
                wrap_titled_diagrams(&mut d.content)
            }
            Block::Div(d) => drop_titles(&mut d.content),
            _ => {}
        }
    }
}

/// Inside a `.card` div, a diagram's own `title` has no card to name.
fn drop_titles(blocks: &mut Blocks) {
    for block in blocks.iter_mut() {
        match block {
            Block::CodeBlock(cb) if is_diagram_block(&cb.attr.1) => {
                cb.attr.2.remove("title");
            }
            Block::Div(d) => drop_titles(&mut d.content),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::block::CodeBlock;
    use quarto_source_map::SourceInfo;

    fn diagram(class: &str, title: Option<&str>) -> Block {
        Block::CodeBlock(Box::new(CodeBlock {
            attr: (
                String::new(),
                vec![class.to_string()],
                title
                    .map(|t| ("title".to_string(), t.to_string()))
                    .into_iter()
                    .collect(),
            )
                .into(),
            text: "a".to_string(),
            source_info: SourceInfo::for_test(),
            attr_source: AttrSourceInfo::empty(),
        }))
    }

    fn div(classes: &[&str], content: Blocks) -> Block {
        Block::Div(Box::new(Div {
            attr: (
                String::new(),
                classes.iter().map(|c| c.to_string()).collect(),
                hashlink::LinkedHashMap::new(),
            )
                .into(),
            content,
            source_info: SourceInfo::for_test(),
            attr_source: AttrSourceInfo::empty(),
        }))
    }

    /// The `.card` div a titled diagram becomes, and the diagram in it.
    fn card_of(block: &Block) -> (&Div, &CodeBlock) {
        let Block::Div(card) = block else {
            panic!("expected a .card div; got {block:?}");
        };
        let [Block::CodeBlock(cb)] = card.content.as_slice() else {
            panic!("expected the diagram alone; got {:?}", card.content);
        };
        (card, cb)
    }

    /// A titled diagram, top level or in a plain div, becomes a titled
    /// `.card` div holding the diagram, which loses its own `title`.
    #[test]
    fn titled_diagrams_become_titled_cards() {
        let mut blocks = vec![
            diagram("dot", Some("Flow")),
            div(&["columns"], vec![diagram("mermaid", Some("Chart"))]),
        ];
        wrap_titled_diagrams(&mut blocks);
        let (card, cb) = card_of(&blocks[0]);
        assert_eq!(card.attr.1, ["card"]);
        assert_eq!(card.attr.2.get("title").map(String::as_str), Some("Flow"));
        assert_eq!(cb.attr.1, ["dot"]);
        assert!(cb.attr.2.get("title").is_none());
        let Block::Div(columns) = &blocks[1] else {
            panic!("{:?}", blocks[1]);
        };
        let (card, _) = card_of(&columns.content[0]);
        assert_eq!(card.attr.2.get("title").map(String::as_str), Some("Chart"));
    }

    /// Untitled diagrams and other code blocks are left alone.
    #[test]
    fn untitled_diagrams_and_other_code_are_untouched() {
        let mut blocks = vec![diagram("dot", None), diagram("python", Some("Not a card"))];
        let before = blocks.clone();
        wrap_titled_diagrams(&mut blocks);
        assert_eq!(blocks, before);
    }

    /// A diagram already in a `.card` div is not wrapped again: the
    /// card's own title stands, and the diagram's is dropped.
    #[test]
    fn diagram_in_a_card_keeps_the_card() {
        let mut blocks = vec![div(&["card"], vec![diagram("dot", Some("Inner"))])];
        wrap_titled_diagrams(&mut blocks);
        let Block::Div(card) = &blocks[0] else {
            panic!("{:?}", blocks[0]);
        };
        let [Block::CodeBlock(cb)] = card.content.as_slice() else {
            panic!("{:?}", card.content);
        };
        assert!(cb.attr.2.get("title").is_none());
    }
}
