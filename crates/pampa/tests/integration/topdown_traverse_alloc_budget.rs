//! Allocation budget for `pampa::filters::topdown_traverse`.
//!
//! A traversal pass whose filter changes nothing must not allocate per
//! visited node. Before bd-w0x91nmh every visited inline was wrapped in
//! a `vec![inline]` and the result vector grew from zero capacity, so a
//! no-op pass over N inlines cost > N allocations (~268k for a 200k-inline
//! document, 29 ms per pass). The budget below is one allocation per
//! container vector plus slack; the old code exceeds it by two orders of
//! magnitude.
//!
//! Uses the counting `#[global_allocator]` in `main.rs`.

use pampa::filter_context::FilterContext;
use pampa::filters::{Filter, FilterReturn, topdown_traverse};
use pampa::pandoc::{Block, Inline, Pandoc, Paragraph, Space, Str};
use quarto_pandoc_types::{ConfigValue, ConfigValueKind};
use quarto_source_map::SourceInfo;

const PARAGRAPHS: usize = 100;
const WORDS_PER_PARAGRAPH: usize = 50;

fn doc() -> Pandoc {
    let blocks = (0..PARAGRAPHS)
        .map(|_| {
            let mut content = Vec::new();
            for w in 0..WORDS_PER_PARAGRAPH {
                content.push(Inline::Str(Str {
                    text: format!("w{w}"),
                    source_info: SourceInfo::for_test(),
                }));
                content.push(Inline::Space(Space {
                    source_info: SourceInfo::for_test(),
                }));
            }
            Block::Paragraph(Paragraph {
                content,
                source_info: SourceInfo::for_test(),
            })
        })
        .collect();
    Pandoc {
        meta: ConfigValue {
            value: ConfigValueKind::Map(vec![]),
            source_info: SourceInfo::for_test(),
            merge_op: Default::default(),
        },
        blocks,
    }
}

#[test]
fn noop_header_filter_does_not_allocate_per_node() {
    let doc = doc();
    let inlines = PARAGRAPHS * WORDS_PER_PARAGRAPH * 2;
    let mut filter = Filter::new().with_header(|h, _ctx| FilterReturn::Unchanged(h));
    let mut ctx = FilterContext::new();

    let before = crate::alloc_counter::allocs();
    let out = topdown_traverse(doc, &mut filter, &mut ctx);
    let allocs = crate::alloc_counter::allocs() - before;

    assert_eq!(out.blocks.len(), PARAGRAPHS);
    // One rebuilt Vec per paragraph, one for the block list, plus slack.
    let budget = (PARAGRAPHS as u64) * 2 + 64;
    assert!(
        allocs <= budget,
        "no-op traversal over {inlines} inlines made {allocs} allocations (budget {budget})"
    );
}

/// Size ceilings for the AST node enums. Every by-value move in every
/// walker copies `size_of::<Inline>()` / `size_of::<Block>()` bytes, so a
/// fat new field on any variant is a cross-cutting performance regression.
/// History: `Inline` was 776 and `Block` 1552 until `SourceInfo` shrank from
/// 136 to 32 bytes (bd-1c085k3a). Raise a ceiling only deliberately.
#[test]
fn ast_node_size_ceilings() {
    use std::mem::size_of;
    let (si, inline, block) = (
        size_of::<quarto_source_map::SourceInfo>(),
        size_of::<Inline>(),
        size_of::<Block>(),
    );
    eprintln!("size_of SourceInfo={si} Inline={inline} Block={block}");
    assert!(si <= 32, "SourceInfo is {si} bytes");
    assert!(inline <= 400, "Inline is {inline} bytes");
    assert!(block <= 900, "Block is {block} bytes");
}
