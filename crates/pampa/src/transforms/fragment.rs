/*
 * transforms/fragment.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Preparation of markdown parsed on its own, as a *fragment* of a larger
//! document: an lmd `{= … =}` fragment (`quarto_core::lmd`) or the string
//! `quarto.markdown(str)` reads at run time.
//!
//! The qmd reader leaves two things for the document pipeline to finish
//! that a fragment has to finish for itself, because the fragment's nodes
//! become `pandoc.*` constructor calls (or a `pandoc.Nodes` value) with no
//! place for them:
//!
//! - **Footnotes.** The reader emits a definition `[^id]: …` as a
//!   `NoteDefinitionPara` / `NoteDefinitionFencedBlock` block and lowers a
//!   reference `[^id]` to an empty `Span.quarto-note-reference` with a
//!   `reference-id` attribute (see `postprocess.rs`); quarto-core's
//!   `footnotes` transform pairs the two into an inline `Note` late in the
//!   pipeline. Neither block has a Lua constructor. [`resolve_local_notes`]
//!   does the pairing *within the fragment*, exactly as pandoc pairs them
//!   within one document: each reference whose definition is in the
//!   fragment becomes a `Note` holding the definition's content, and the
//!   definitions are removed. A reference with no local definition is left
//!   as the reference span, for the document's transform to resolve against
//!   the document's definitions (so a fragment may reference a note defined
//!   in the body). A definition with no local reference is dropped, as
//!   pandoc drops an unreferenced definition. Repeated references each get
//!   their own `Note`, as for `^[…]` inline notes.
//!
//! - **Heading ids.** The reader derives an id for every heading from its
//!   text, de-duplicated within what it parsed. A fragment's text may hold
//!   lmd placeholders (`### `{lmd} title`` gives `lmd-title`) and a fragment
//!   reused as a component would emit the same id every time.
//!   [`strip_derived_heading_ids`] clears the derived ids so the document's
//!   `heading-id` transform derives one from the final text, unique across
//!   the whole document; an author-written `{#id}`, which carries a source
//!   location, is kept.
//!
//! [`prepare_fragment_blocks`] does both.

use std::collections::HashMap;

use quarto_source_map::SourceInfo;

use crate::filter_context::FilterContext;
use crate::filters::{Filter, FilterReturn, topdown_traverse_blocks};
use crate::pandoc::block::Paragraph;
use crate::pandoc::inline::Note;
use crate::pandoc::{Block, Blocks, Inline, Inlines};

/// Class of the span the reader lowers a `[^id]` reference to. Keep in
/// sync with `postprocess.rs` (`with_note_reference`) and quarto-core's
/// `transforms/footnotes.rs`.
pub const NOTE_REFERENCE_CLASS: &str = "quarto-note-reference";

/// Attribute of that span holding the note id.
pub const NOTE_REFERENCE_ID_KEY: &str = "reference-id";

/// Everything a fragment parsed on its own needs before its nodes leave the
/// reader's world: local footnotes paired, derived heading ids dropped.
pub fn prepare_fragment_blocks(blocks: Blocks) -> Blocks {
    strip_derived_heading_ids(resolve_local_notes(blocks))
}

/// What a definition holds: a single paragraph's inlines (`[^id]: text`) or
/// blocks (the fenced `::: ^id` form).
enum NoteContent {
    Inlines(Inlines),
    Blocks(Blocks),
}

impl NoteContent {
    /// The `Note`'s content; a paragraph definition is wrapped in a
    /// `Paragraph` carrying the reference's span, as the document transform
    /// does.
    fn to_blocks(&self, source_info: &SourceInfo) -> Blocks {
        match self {
            NoteContent::Inlines(inlines) => vec![Block::Paragraph(Paragraph {
                content: inlines.clone(),
                source_info: source_info.clone(),
            })],
            NoteContent::Blocks(blocks) => blocks.clone(),
        }
    }
}

/// Pair the footnote definitions in `blocks` (at any depth) with the
/// references in `blocks`; see the module docs for the rule.
pub fn resolve_local_notes(blocks: Blocks) -> Blocks {
    let mut definitions: HashMap<String, NoteContent> = HashMap::new();
    let mut ctx = FilterContext::new();

    // Pass 1: collect and remove the definitions, wherever they sit.
    let blocks = topdown_traverse_blocks(
        blocks,
        &mut Filter::new().with_blocks(|blocks, _ctx| {
            let mut kept: Blocks = Vec::with_capacity(blocks.len());
            let mut removed = false;
            for block in blocks {
                match block {
                    Block::NoteDefinitionPara(def) => {
                        definitions.insert(def.id, NoteContent::Inlines(def.content));
                        removed = true;
                    }
                    Block::NoteDefinitionFencedBlock(def) => {
                        definitions.insert(def.id, NoteContent::Blocks(def.content));
                        removed = true;
                    }
                    other => kept.push(other),
                }
            }
            if removed {
                FilterReturn::FilterResult(kept, true)
            } else {
                FilterReturn::Unchanged(kept)
            }
        }),
        &mut ctx,
    );
    if definitions.is_empty() {
        return blocks;
    }

    // Pass 2: each reference with a local definition becomes a Note.
    let note_for = |id: &str, source_info: &SourceInfo| -> Option<Inline> {
        definitions.get(id).map(|content| {
            Inline::Note(Note {
                content: content.to_blocks(source_info),
                source_info: source_info.clone(),
            })
        })
    };
    topdown_traverse_blocks(
        blocks,
        &mut Filter::new()
            .with_span(|span, _ctx| {
                if let Some(id) = note_reference_id(&span)
                    && let Some(note) = note_for(&id, &span.source_info)
                {
                    return FilterReturn::FilterResult(vec![note], false);
                }
                FilterReturn::Unchanged(span)
            })
            .with_note_reference(|reference, _ctx| {
                // The typed form, in case a caller runs this before the
                // reader's postprocess lowered it.
                if let Some(note) = note_for(&reference.id, &reference.source_info) {
                    return FilterReturn::FilterResult(vec![note], false);
                }
                FilterReturn::Unchanged(reference)
            }),
        &mut ctx,
    )
}

/// The note id of a reader-lowered reference span, if `span` is one.
fn note_reference_id(span: &crate::pandoc::Span) -> Option<String> {
    if !span.content.is_empty() || !span.attr.1.iter().any(|c| c == NOTE_REFERENCE_CLASS) {
        return None;
    }
    span.attr.2.get(NOTE_REFERENCE_ID_KEY).cloned()
}

/// Clear the id of every heading whose id the reader derived (no source
/// location for it); keep author-written ids.
pub fn strip_derived_heading_ids(blocks: Blocks) -> Blocks {
    let mut ctx = FilterContext::new();
    topdown_traverse_blocks(
        blocks,
        &mut Filter::new().with_header(|mut header, _ctx| {
            if header.attr_source.id.is_none() && !header.attr.0.is_empty() {
                header.attr.0.clear();
                // A heading holds no headings; nothing below needs this pass.
                return FilterReturn::FilterResult(vec![Block::Header(Box::new(header))], false);
            }
            FilterReturn::Unchanged(header)
        }),
        &mut ctx,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(markdown: &str) -> Blocks {
        let mut sink: Vec<u8> = Vec::new();
        let (pandoc, _ctx, _warnings) = crate::readers::qmd::read(
            markdown.as_bytes(),
            false,
            "fragment.md",
            &mut sink,
            true,
            None,
        )
        .expect("fragment parses");
        pandoc.blocks
    }

    /// Every `Note` in the paragraph at `index`, flattened to the text of
    /// its content.
    fn notes_in_para(blocks: &[Block], index: usize) -> Vec<String> {
        let Block::Paragraph(p) = &blocks[index] else {
            panic!("block {index} is not a paragraph: {:?}", blocks[index]);
        };
        p.content
            .iter()
            .filter_map(|i| match i {
                Inline::Note(n) => Some(text_of_blocks(&n.content)),
                _ => None,
            })
            .collect()
    }

    fn text_of_blocks(blocks: &[Block]) -> String {
        let mut out = String::new();
        for block in blocks {
            match block {
                Block::Paragraph(p) => out.push_str(&text_of_inlines(&p.content)),
                Block::Plain(p) => out.push_str(&text_of_inlines(&p.content)),
                other => out.push_str(&format!("<{}>", block_name(other))),
            }
            out.push('|');
        }
        out
    }

    fn text_of_inlines(inlines: &[Inline]) -> String {
        inlines
            .iter()
            .map(|i| match i {
                Inline::Str(s) => s.text.to_string(),
                Inline::Space(_) | Inline::SoftBreak(_) => " ".to_string(),
                Inline::Emph(e) => format!("*{}*", text_of_inlines(&e.content)),
                Inline::Note(_) => "[note]".to_string(),
                Inline::Span(s) if note_reference_id(s).is_some() => {
                    format!("[^{}]", note_reference_id(s).unwrap())
                }
                other => format!("<{other:?}>"),
            })
            .collect()
    }

    fn block_name(block: &Block) -> &'static str {
        match block {
            Block::NoteDefinitionPara(_) => "NoteDefinitionPara",
            Block::NoteDefinitionFencedBlock(_) => "NoteDefinitionFencedBlock",
            Block::Div(_) => "Div",
            Block::BulletList(_) => "BulletList",
            Block::Header(_) => "Header",
            _ => "other",
        }
    }

    fn has_definitions(blocks: &[Block]) -> bool {
        blocks.iter().any(|b| {
            matches!(
                b,
                Block::NoteDefinitionPara(_) | Block::NoteDefinitionFencedBlock(_)
            ) || match b {
                Block::Div(d) => has_definitions(&d.content),
                Block::BulletList(l) => l.content.iter().any(|i| has_definitions(i)),
                _ => false,
            }
        })
    }

    #[test]
    fn a_paragraph_definition_pairs_with_its_reference() {
        let blocks = resolve_local_notes(parse("text[^1] more\n\n[^1]: the *note*\n"));
        assert_eq!(blocks.len(), 1, "{blocks:?}");
        assert_eq!(notes_in_para(&blocks, 0), vec!["the *note*|"]);
        assert!(!has_definitions(&blocks));
    }

    #[test]
    fn a_fenced_definition_keeps_its_blocks() {
        let blocks = resolve_local_notes(parse(
            "text[^a]\n\n::: ^a\nFirst para.\n\nSecond para.\n:::\n",
        ));
        assert_eq!(blocks.len(), 1, "{blocks:?}");
        assert_eq!(notes_in_para(&blocks, 0), vec!["First para.|Second para.|"]);
    }

    #[test]
    fn definitions_and_references_pair_across_nesting() {
        let blocks = resolve_local_notes(parse(
            "::: {.card}\n- item[^x]\n\n[^x]: nested note\n:::\n\nouter[^x]\n",
        ));
        assert!(!has_definitions(&blocks), "{blocks:?}");
        // The reference inside the list item resolved.
        let Block::Div(div) = &blocks[0] else {
            panic!("{blocks:?}")
        };
        let Block::BulletList(list) = &div.content[0] else {
            panic!("{:?}", div.content)
        };
        let Block::Plain(item) = &list.content[0][0] else {
            panic!("{:?}", list.content)
        };
        assert_eq!(text_of_inlines(&item.content), "item[note]");
        // So did the one outside the div: a definition is visible to the
        // whole fragment, as in a document.
        assert_eq!(notes_in_para(&blocks, 1), vec!["nested note|"]);
    }

    #[test]
    fn a_reference_without_a_local_definition_is_left_for_the_document() {
        let blocks = resolve_local_notes(parse("text[^doc]\n"));
        assert_eq!(blocks.len(), 1);
        let Block::Paragraph(p) = &blocks[0] else {
            panic!("{blocks:?}")
        };
        assert_eq!(text_of_inlines(&p.content), "text[^doc]");
    }

    #[test]
    fn a_mix_resolves_the_local_one_and_keeps_the_other() {
        let blocks = resolve_local_notes(parse("a[^here] b[^there]\n\n[^here]: local\n"));
        let Block::Paragraph(p) = &blocks[0] else {
            panic!("{blocks:?}")
        };
        assert_eq!(text_of_inlines(&p.content), "a[note] b[^there]");
    }

    #[test]
    fn an_unreferenced_definition_is_dropped() {
        let blocks = resolve_local_notes(parse("text\n\n[^orphan]: never used\n"));
        assert_eq!(blocks.len(), 1, "{blocks:?}");
        assert!(!has_definitions(&blocks));
    }

    #[test]
    fn repeated_references_each_get_a_note() {
        let blocks = resolve_local_notes(parse("a[^1] b[^1]\n\n[^1]: twice\n"));
        assert_eq!(notes_in_para(&blocks, 0), vec!["twice|", "twice|"]);
    }

    #[test]
    fn derived_heading_ids_are_dropped_and_explicit_ones_kept() {
        let blocks = strip_derived_heading_ids(parse(
            "### `{lmd} title`\n\n### Pinned {#pinned}\n\n::: x\n#### Inner\n:::\n",
        ));
        let ids: Vec<String> = blocks
            .iter()
            .flat_map(|b| match b {
                Block::Header(h) => vec![h.attr.0.clone()],
                Block::Div(d) => d
                    .content
                    .iter()
                    .filter_map(|b| match b {
                        Block::Header(h) => Some(h.attr.0.clone()),
                        _ => None,
                    })
                    .collect(),
                _ => vec![],
            })
            .collect();
        assert_eq!(ids, vec!["", "pinned", ""]);
    }

    #[test]
    fn prepare_does_both() {
        let blocks = prepare_fragment_blocks(parse("## Title\n\ntext[^1]\n\n[^1]: note\n"));
        assert_eq!(blocks.len(), 2, "{blocks:?}");
        let Block::Header(h) = &blocks[0] else {
            panic!("{blocks:?}")
        };
        assert_eq!(h.attr.0, "");
        assert_eq!(notes_in_para(&blocks, 1), vec!["note|"]);
    }
}
