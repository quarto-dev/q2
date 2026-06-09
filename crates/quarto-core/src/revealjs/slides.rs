/*
 * revealjs/slides.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Reveal.js slide construction (bd-30mk70ld, Phase 1.3).
 */

//! Build reveal.js's exactly-two-level slide tree from a flat block list.
//!
//! reveal.js supports two nesting levels only: `.slides > section`
//! (horizontal slides) and `section > section` (vertical slides within a
//! stack). This module ports Pandoc's `--slide-level` algorithm — which is
//! what Quarto 1 relies on (its generic section-div machinery does *not*
//! build reveal slides; Pandoc's reveal writer does). See
//! `claude-notes/plans/2026-06-08-revealjs-presentations.md` decision on the
//! slide-split design.
//!
//! With slide level `N` (default 2):
//!
//! - header level `< N` → a **section divider**: opens a horizontal *stack*
//!   `<section>` whose first vertical child is the divider slide, and whose
//!   subsequent `== N` slides become vertical children.
//! - header level `== N` → a **slide**: a vertical child of the current stack
//!   if one is open, otherwise a top-level horizontal slide.
//! - header level `> N` → an ordinary heading rendered *within* the current
//!   slide (not a new slide).
//! - `HorizontalRule` → a slide break at the current nesting.
//!
//! Each slide is emitted as a `Div` carrying the `section` class so the HTML
//! writer serializes it as `<section>` (`pampa/src/writers/html.rs`). The
//! heading's `id`, classes, and key-value attributes are hoisted onto the
//! enclosing section (matching Pandoc, which moves header attributes onto the
//! slide `<section>`); the heading keeps its classes/attrs but loses its id.

use hashlink::LinkedHashMap;
use quarto_pandoc_types::attr::AttrSourceInfo;
use quarto_pandoc_types::block::{Block, Div, Header};
use quarto_source_map::{By, SourceInfo};

type Attr = (String, Vec<String>, LinkedHashMap<String, String>);

/// The default slide level, matching Quarto 1 (`format-reveal.ts` sets
/// `slide-level: 2`).
pub const DEFAULT_SLIDE_LEVEL: usize = 2;

/// Attr for an untitled slide (preamble, HR-split, or section divider): no id,
/// but the `section` class so the HTML writer emits `<section>`.
fn slide_attr() -> Attr {
    (
        String::new(),
        vec!["section".to_string()],
        LinkedHashMap::new(),
    )
}

fn make_section(attr: Attr, content: Vec<Block>) -> Block {
    Block::Div(Div {
        attr,
        content,
        source_info: SourceInfo::generated(By::revealjs()),
        attr_source: AttrSourceInfo::empty(),
    })
}

/// Split a heading into (section attr, id-stripped header). The section gets
/// the `section` class plus the heading's user classes; the heading keeps its
/// classes/attrs but loses its id (which moves to the section).
fn section_attr_and_header(h: &Header) -> (Attr, Header) {
    let (id, classes, attrs) = &h.attr;
    let mut section_classes = Vec::with_capacity(classes.len() + 1);
    section_classes.push("section".to_string());
    section_classes.extend(classes.clone());
    let section_attr = (id.clone(), section_classes, attrs.clone());

    let header = Header {
        level: h.level,
        attr: (String::new(), classes.clone(), attrs.clone()),
        content: h.content.clone(),
        source_info: h.source_info.clone(),
        attr_source: h.attr_source.clone(),
    };
    (section_attr, header)
}

/// Accumulators for the slide-building state machine.
struct Builder {
    result: Vec<Block>,
    /// An open horizontal stack: `(attr, vertical child sections)`. Present
    /// only while inside a `< slide_level` section divider.
    stack: Option<(Attr, Vec<Block>)>,
    /// The open leaf slide accumulating content: `(attr, content blocks)`.
    slide: Option<(Attr, Vec<Block>)>,
}

impl Builder {
    fn new() -> Self {
        Self {
            result: Vec::new(),
            stack: None,
            slide: None,
        }
    }

    /// Close the open leaf slide into the current stack (if any) or the
    /// top-level result.
    fn flush_slide(&mut self) {
        if let Some((attr, content)) = self.slide.take() {
            let sec = make_section(attr, content);
            match self.stack.as_mut() {
                Some((_, children)) => children.push(sec),
                None => self.result.push(sec),
            }
        }
    }

    /// Close the open leaf slide and then the open stack into the result.
    fn flush_stack(&mut self) {
        self.flush_slide();
        if let Some((attr, children)) = self.stack.take() {
            self.result.push(make_section(attr, children));
        }
    }

    /// Append a block to the open slide, opening an anonymous slide if none.
    fn push_content(&mut self, block: Block) {
        if self.slide.is_none() {
            self.slide = Some((slide_attr(), Vec::new()));
        }
        self.slide.as_mut().unwrap().1.push(block);
    }
}

/// Build the reveal two-level slide sections from a flat block list.
pub fn build_reveal_slides(blocks: Vec<Block>, slide_level: usize) -> Vec<Block> {
    let slide_level = slide_level.max(1);
    let mut b = Builder::new();

    for block in blocks {
        match &block {
            Block::Header(h) if h.level < slide_level => {
                // Section divider → new horizontal stack. The stack carries the
                // heading's id/classes/attrs; the divider slide is its first
                // vertical child.
                b.flush_stack();
                let (stack_attr, header) = section_attr_and_header(h);
                b.stack = Some((stack_attr, Vec::new()));
                b.slide = Some((slide_attr(), vec![Block::Header(header)]));
            }
            Block::Header(h) if h.level == slide_level => {
                // A slide: vertical child of the open stack, else top-level.
                b.flush_slide();
                let (slide_attr, header) = section_attr_and_header(h);
                b.slide = Some((slide_attr, vec![Block::Header(header)]));
            }
            Block::Header(_) => {
                // Level > slide_level → ordinary heading within the slide.
                b.push_content(block);
            }
            Block::HorizontalRule(_) => {
                // Slide break at the current nesting.
                b.flush_slide();
            }
            _ => {
                b.push_content(block);
            }
        }
    }

    b.flush_stack();
    b.result
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::block::{HorizontalRule, Paragraph};
    use quarto_pandoc_types::inline::{Inline, Str};

    fn header(level: usize, id: &str, text: &str) -> Block {
        Block::Header(Header {
            level,
            attr: (id.to_string(), vec![], LinkedHashMap::new()),
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: SourceInfo::generated(By::revealjs()),
            })],
            source_info: SourceInfo::generated(By::revealjs()),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    fn header_with(level: usize, id: &str, classes: &[&str], text: &str) -> Block {
        Block::Header(Header {
            level,
            attr: (
                id.to_string(),
                classes.iter().map(|s| s.to_string()).collect(),
                LinkedHashMap::new(),
            ),
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: SourceInfo::generated(By::revealjs()),
            })],
            source_info: SourceInfo::generated(By::revealjs()),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    fn para(text: &str) -> Block {
        Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: SourceInfo::generated(By::revealjs()),
            })],
            source_info: SourceInfo::generated(By::revealjs()),
        })
    }

    fn hr() -> Block {
        Block::HorizontalRule(HorizontalRule {
            source_info: SourceInfo::generated(By::revealjs()),
        })
    }

    fn as_section(block: &Block) -> &Div {
        let Block::Div(div) = block else {
            panic!("expected section Div, got {block:?}");
        };
        assert!(
            div.attr.1.contains(&"section".to_string()),
            "section Div must carry the `section` class, got {:?}",
            div.attr.1
        );
        div
    }

    /// Count nested section Divs (depth-first), for asserting total `<section>`.
    fn count_sections(blocks: &[Block]) -> usize {
        blocks
            .iter()
            .map(|b| match b {
                Block::Div(d) if d.attr.1.contains(&"section".to_string()) => {
                    1 + count_sections(&d.content)
                }
                _ => 0,
            })
            .sum()
    }

    #[test]
    fn flat_h2_deck_yields_sibling_horizontal_slides() {
        let blocks = vec![
            header(2, "a", "A"),
            para("a body"),
            header(2, "b", "B"),
            para("b body"),
            header(2, "c", "C"),
        ];
        let out = build_reveal_slides(blocks, 2);
        assert_eq!(out.len(), 3, "three top-level horizontal slides");
        assert_eq!(count_sections(&out), 3, "no vertical nesting");
        assert_eq!(as_section(&out[0]).attr.0, "a", "id hoisted to section");
    }

    #[test]
    fn section_divider_creates_vertical_stack() {
        // # Sec / ## A / ## B  →  one stack with [divider, A, B] vertical kids.
        let blocks = vec![
            header(1, "sec", "Sec"),
            para("intro"),
            header(2, "a", "A"),
            header(2, "b", "B"),
        ];
        let out = build_reveal_slides(blocks, 2);
        assert_eq!(out.len(), 1, "one top-level stack section");
        let stack = as_section(&out[0]);
        assert_eq!(stack.attr.0, "sec", "stack carries the divider id");
        // children: divider slide + A + B
        let kids: Vec<&Block> = stack
            .content
            .iter()
            .filter(|b| matches!(b, Block::Div(d) if d.attr.1.contains(&"section".to_string())))
            .collect();
        assert_eq!(kids.len(), 3, "divider + 2 vertical slides");
        assert_eq!(as_section(kids[1]).attr.0, "a");
        assert_eq!(as_section(kids[2]).attr.0, "b");
    }

    #[test]
    fn deep_heading_stays_within_slide() {
        // ## Slide / ### Sub  → ### is content inside the H2 slide, not a slide.
        let blocks = vec![header(2, "s", "Slide"), header(3, "sub", "Sub"), para("x")];
        let out = build_reveal_slides(blocks, 2);
        assert_eq!(out.len(), 1);
        assert_eq!(count_sections(&out), 1, "the H3 is not its own section");
        let slide = as_section(&out[0]);
        // header(H2, id-stripped), header(H3, kept as content), para
        assert!(
            slide
                .content
                .iter()
                .any(|b| matches!(b, Block::Header(h) if h.level == 3)),
            "H3 retained as an in-slide heading"
        );
    }

    #[test]
    fn horizontal_rule_breaks_slides() {
        let blocks = vec![para("one"), hr(), para("two")];
        let out = build_reveal_slides(blocks, 2);
        assert_eq!(out.len(), 2, "HR splits content into two slides");
    }

    #[test]
    fn content_before_first_header_opens_a_slide() {
        let blocks = vec![para("preamble"), header(2, "a", "A")];
        let out = build_reveal_slides(blocks, 2);
        assert_eq!(out.len(), 2, "anonymous preamble slide + the H2 slide");
        assert_eq!(as_section(&out[0]).attr.0, "", "preamble slide has no id");
    }

    #[test]
    fn no_headers_yields_single_slide() {
        let out = build_reveal_slides(vec![para("a"), para("b")], 2);
        assert_eq!(out.len(), 1);
        assert_eq!(as_section(&out[0]).content.len(), 2);
    }

    #[test]
    fn empty_input_yields_no_slides() {
        assert!(build_reveal_slides(vec![], 2).is_empty());
    }

    #[test]
    fn user_classes_hoist_to_section_header_keeps_them() {
        let blocks = vec![header_with(2, "s", &["center", "smaller"], "S")];
        let out = build_reveal_slides(blocks, 2);
        let sec = as_section(&out[0]);
        assert!(sec.attr.1.contains(&"center".to_string()));
        assert!(sec.attr.1.contains(&"smaller".to_string()));
        // header keeps classes but loses id
        let Block::Header(h) = &sec.content[0] else {
            panic!("first child is the heading");
        };
        assert_eq!(h.attr.0, "", "header id moved to section");
        assert!(h.attr.1.contains(&"center".to_string()));
    }

    #[test]
    fn slide_level_three_treats_h2_as_divider() {
        // slide-level 3: H1 and H2 are dividers, H3 is the slide.
        let blocks = vec![
            header(1, "p1", "Part 1"),
            header(2, "c1", "Chapter 1"),
            header(3, "s1", "Slide 1"),
        ];
        let out = build_reveal_slides(blocks, 3);
        // H1 opens a stack; H2 (<3) opens a *new* stack (flushing the first);
        // H3 (==3) is a slide inside the second stack.
        assert_eq!(
            out.len(),
            2,
            "two stacks: Part 1 (just divider) and Chapter 1"
        );
    }
}
