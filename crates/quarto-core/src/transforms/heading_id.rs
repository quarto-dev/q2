/*
 * transforms/heading_id.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Re-derive heading ids that expansion made stale.
 */

//! `HeadingIdTransform`: give every heading an id that reflects its *final*
//! text.
//!
//! The qmd reader derives a heading's auto id in its postprocess pass, from
//! the text as written. Two things rewrite heading text after that:
//! shortcode expansion (`## before {{< meta foo >}} after`, whose reader id
//! `before-after` ignores the shortcode) and lmd cells (`` ## Hello, `{lmd}
//! name` ``, whose reader id `hello-lmd-name` is derived from the
//! placeholder). Quarto 1 derives ids after expansion, so its ids follow the
//! expanded text (bd-2wv8431v); and an lmd fragment reused as a component
//! would otherwise emit the same placeholder-derived id every time
//! (bd-lmd-exploration-jqv2nbl4, spike 5).
//!
//! This transform runs right after `shortcode-resolve` (lmd cells ran
//! earlier, in `lmd-execute`) and re-derives the id of every heading whose
//! id is **stale**: empty, or reader-derived but no longer what
//! `auto_generated_id` gives for the current content. Author-written ids
//! (`{#x}`, which carry a source location on `attr_source.id`) are never
//! touched, and neither are ids on headings a filter or cell built with
//! `pandoc.Header(…, pandoc.Attr("x"))`: those have no source location
//! either, so they are told apart by their *generated* `source_info`. A
//! heading with an empty id is filled whatever its origin, since an
//! unlinkable heading helps nobody.
//!
//! Re-derivation and de-duplication are pampa's `dedup_scoped_heading_ids`
//! (Pandoc's `uniqueIdent` probe, seeded with every id the pass must not
//! rename), so a reused fragment heading becomes `hello`, `hello-1`, … and
//! never collides with an explicit id elsewhere. The qmd writer's
//! round-trip rule (omit an id equal to `auto_generated_id(content)`)
//! keeps holding, since that is exactly what this pass writes.

use pampa::utils::autoid::{auto_generated_id, dedup_scoped_heading_ids};
use quarto_pandoc_types::block::Header;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceInfo;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

/// Whether `header` is in scope for re-derivation (see the module docs).
fn id_is_stale(header: &Header) -> bool {
    if header.attr.0.is_empty() {
        return true;
    }
    if header.attr_source.id.is_some() {
        return false; // author-written `{#x}`
    }
    if matches!(header.source_info, SourceInfo::Generated(_)) {
        return false; // built by a filter or cell; its id is deliberate
    }
    header.attr.0 != auto_generated_id(&header.content)
}

/// Re-derive every stale heading id in `doc` (see the module docs).
pub fn rederive_stale_heading_ids(doc: Pandoc) -> Pandoc {
    dedup_scoped_heading_ids(doc, id_is_stale)
}

#[derive(Default)]
pub struct HeadingIdTransform;

impl HeadingIdTransform {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for HeadingIdTransform {
    fn name(&self) -> &str {
        "heading-id"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Normalization
    }

    async fn transform(&self, ast: &mut Pandoc, _ctx: &mut RenderContext) -> Result<()> {
        let doc = std::mem::take(ast);
        *ast = rederive_stale_heading_ids(doc);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::attr::AttrSourceInfo;
    use quarto_pandoc_types::block::Block;
    use quarto_pandoc_types::inline::{Inline, Str};
    use quarto_source_map::By;

    fn parse(qmd: &str) -> Pandoc {
        let mut sink: Vec<u8> = Vec::new();
        let (ast, _ctx, _warnings) =
            pampa::readers::qmd::read(qmd.as_bytes(), false, "test.qmd", &mut sink, true, None)
                .expect("test document parses");
        ast
    }

    fn header_ids(doc: &Pandoc) -> Vec<String> {
        doc.blocks
            .iter()
            .filter_map(|b| match b {
                Block::Header(h) => Some(h.attr.0.clone()),
                _ => None,
            })
            .collect()
    }

    fn set_header_text(doc: &mut Pandoc, index: usize, text: &str) {
        let Block::Header(h) = &mut doc.blocks[index] else {
            panic!("block {index} is not a header");
        };
        h.content = vec![Inline::Str(Str {
            text: text.to_string(),
            source_info: SourceInfo::generated(By::unknown()),
        })];
    }

    #[test]
    fn ids_that_still_match_their_text_are_untouched() {
        let doc = parse("## Alpha\n\n## Alpha\n\n## Beta {#custom}\n");
        let before = header_ids(&doc);
        assert_eq!(before, vec!["alpha", "alpha-1", "custom"]);
        let after = rederive_stale_heading_ids(doc);
        assert_eq!(header_ids(&after), before);
    }

    #[test]
    fn a_reader_derived_id_follows_text_that_changed_after_parsing() {
        // What shortcode expansion and inline lmd cells do: the reader
        // derived the id from placeholder text, then the text changed.
        let mut doc = parse("## before after\n");
        assert_eq!(header_ids(&doc), vec!["before-after"]);
        set_header_text(&mut doc, 0, "before 1.2 after");
        let after = rederive_stale_heading_ids(doc);
        assert_eq!(header_ids(&after), vec!["before-1.2-after"]);
    }

    #[test]
    fn an_empty_id_is_filled_whatever_the_headings_origin() {
        let mut doc = parse("## Intro\n");
        doc.blocks.push(Block::Header(Header {
            level: 2,
            attr: (String::new(), vec![], Default::default()),
            content: vec![Inline::Str(Str {
                text: "Built".to_string(),
                source_info: SourceInfo::generated(By::unknown()),
            })],
            source_info: SourceInfo::generated(By::unknown()),
            attr_source: AttrSourceInfo::empty(),
        }));
        let after = rederive_stale_heading_ids(doc);
        assert_eq!(header_ids(&after), vec!["intro", "built"]);
    }

    #[test]
    fn author_written_ids_are_never_renamed_even_when_text_changed() {
        let mut doc = parse("## Hello {#keep-me}\n");
        set_header_text(&mut doc, 0, "Something else");
        let after = rederive_stale_heading_ids(doc);
        assert_eq!(header_ids(&after), vec!["keep-me"]);
    }

    #[test]
    fn a_filter_built_heading_keeps_the_id_it_was_given() {
        // pandoc.Header(2, "x", pandoc.Attr("intro")) from Lua: no source
        // location on the id, but a generated source_info.
        let mut doc = parse("## Alpha\n");
        doc.blocks.push(Block::Header(Header {
            level: 2,
            attr: ("intro".to_string(), vec![], Default::default()),
            content: vec![Inline::Str(Str {
                text: "x".to_string(),
                source_info: SourceInfo::generated(By::unknown()),
            })],
            source_info: SourceInfo::generated(By::unknown()),
            attr_source: AttrSourceInfo::empty(),
        }));
        let after = rederive_stale_heading_ids(doc);
        assert_eq!(header_ids(&after), vec!["alpha", "intro"]);
    }

    #[test]
    fn rederived_ids_are_unique_against_everything_else_in_the_document() {
        // Two stale headings whose new text collides with each other and
        // with an explicit id: hello is taken, so they get hello-1, hello-2.
        let mut doc = parse("## Placeholder one\n\n## Placeholder two\n\n## Third {#hello}\n");
        set_header_text(&mut doc, 0, "Hello");
        set_header_text(&mut doc, 1, "Hello");
        let after = rederive_stale_heading_ids(doc);
        assert_eq!(header_ids(&after), vec!["hello-1", "hello-2", "hello"]);
    }

    #[test]
    fn the_pass_is_idempotent() {
        let mut doc = parse("## a\n\n## b\n");
        set_header_text(&mut doc, 0, "changed");
        let once = rederive_stale_heading_ids(doc);
        let twice = rederive_stale_heading_ids(once.clone());
        assert_eq!(header_ids(&once), vec!["changed", "b"]);
        assert_eq!(header_ids(&twice), header_ids(&once));
    }
}
