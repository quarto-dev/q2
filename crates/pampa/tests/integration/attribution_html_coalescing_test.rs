//! Phase 4b — HTML writer prose coalescing.
//!
//! Pins the contract from
//! `claude-notes/plans/2026-05-06-attribution-pipeline.md` § Phase 4b:
//! contiguous prose inlines (`Str` / `Space` / `SoftBreak` / `LineBreak`)
//! whose attribution lookup hits the same `(actor, time)` coalesce
//! into one outer `<span data-attr-*>` wrapper. Structured inlines
//! (`Code`, `Emph`, `Strong`, `Link`, `Span`, `Math`, …) break the
//! prose run — they carry their own attribution attrs directly on
//! their element tag and reset coalescing.
//!
//! When `include_source_locations` is also on, the per-inline
//! `<span data-sid=…>` wrappers nest **inside** the outer attribution
//! wrapper (the outer wrapper carries no `data-sid` because it spans
//! multiple inlines).
//!
//! Off-path (no `attribution_by_node`) the writer takes its existing
//! byte-identical code path; that invariant has its own dedicated
//! test in `attribution_baseline_snapshot.rs` at the orchestrated
//! `render_qmd_to_html` level.

use std::collections::HashMap;
use std::sync::Arc;

use pampa::pandoc::ast_context::ASTContext;
use pampa::pandoc::{Block, Code, Inline, Pandoc, Paragraph, Space, Str};
use pampa::writers::html::{HtmlAttributionRecord, HtmlConfig, write_with_config};
use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::attr::{Attr, AttrSourceInfo};
use quarto_source_map::SourceInfo;

fn si(start: usize, end: usize) -> SourceInfo {
    SourceInfo::Original {
        file_id: quarto_source_map::FileId(0),
        start_offset: start,
        end_offset: end,
    }
}

fn empty_meta() -> ConfigValue {
    ConfigValue::new_map(Vec::new(), SourceInfo::for_test())
}

fn make_str(text: &str, start: usize, end: usize) -> Inline {
    Inline::Str(Str {
        text: text.to_string(),
        source_info: si(start, end),
    })
}

fn make_space(start: usize) -> Inline {
    Inline::Space(Space {
        source_info: si(start, start + 1),
    })
}

fn make_code(text: &str, start: usize, end: usize) -> Inline {
    Inline::Code(Code {
        attr: Attr::default(),
        text: text.to_string(),
        source_info: si(start, end),
        attr_source: AttrSourceInfo::empty(),
    })
}

fn ptr_of(inline: &Inline) -> usize {
    inline.source_info() as *const SourceInfo as usize
}

fn block_ptr(block: &Block) -> usize {
    block.source_info() as *const SourceInfo as usize
}

fn render_body(pandoc: &Pandoc, config: HtmlConfig) -> String {
    let ctx = ASTContext::anonymous();
    let mut buf = Vec::new();
    write_with_config(pandoc, &mut buf, config).expect("html write");
    // suppress unused-var warning on ctx for now: write_with_config doesn't take it
    let _ = ctx;
    String::from_utf8(buf).expect("utf-8 body")
}

fn alice() -> Arc<str> {
    Arc::from("alice")
}

fn bob() -> Arc<str> {
    Arc::from("bob")
}

// ===========================================================================
// Test #7b — three contiguous Str inlines with same (actor, time)
//             coalesce into one outer span wrapper.
// ===========================================================================
#[test]
fn contiguous_same_attribution_prose_coalesces_into_one_outer_wrapper() {
    // [Str("hello"), Space, Str("there"), Space, Str("alice")]
    let inlines = vec![
        make_str("hello", 0, 5),
        make_space(5),
        make_str("there", 6, 11),
        make_space(11),
        make_str("alice", 12, 17),
    ];
    let inline_ptrs: Vec<usize> = inlines.iter().map(ptr_of).collect();
    let para = Block::Paragraph(Paragraph {
        content: inlines,
        source_info: si(0, 17),
    });
    let pandoc = Pandoc {
        blocks: vec![para],
        meta: empty_meta(),
    };
    let block_ptr_val = block_ptr(&pandoc.blocks[0]);

    let mut by_node: HashMap<usize, HtmlAttributionRecord> = HashMap::new();
    let a = alice();
    // Block-level lookup so the <p> tag carries the attribution attrs.
    by_node.insert(
        block_ptr_val,
        HtmlAttributionRecord {
            actor: Arc::clone(&a),
            time: 1,
        },
    );
    // Each Str (not Space) gets the same (alice, 1) lookup.
    for ptr in [inline_ptrs[0], inline_ptrs[2], inline_ptrs[4]] {
        by_node.insert(
            ptr,
            HtmlAttributionRecord {
                actor: Arc::clone(&a),
                time: 1,
            },
        );
    }

    let config = HtmlConfig {
        include_source_locations: false,
        attribution_by_node: Some(Arc::new(by_node)),
    };

    let body = render_body(&pandoc, config);

    // Three contiguous Str inlines should coalesce to ONE outer span.
    let outer_span_count = body.matches("<span data-attr-actor=\"alice\"").count();
    assert_eq!(
        outer_span_count, 1,
        "expected exactly one outer prose wrapper; got {}\nbody:\n{}",
        outer_span_count, body
    );

    // Block <p> carries the attribution attrs too.
    assert!(
        body.contains("<p data-attr-actor=\"alice\""),
        "block <p> should carry data-attr-actor; body:\n{}",
        body
    );

    // All three words appear in order inside the wrapper.
    assert!(body.contains("hello"));
    assert!(body.contains("there"));
    assert!(body.contains("alice"));
}

// ===========================================================================
// Test — coalescing breaks when (actor, time) changes mid-run.
// ===========================================================================
#[test]
fn coalescing_breaks_on_actor_change() {
    // [Str("hello", alice), Space, Str("world", bob)] → two wrappers.
    let inlines = vec![
        make_str("hello", 0, 5),
        make_space(5),
        make_str("world", 6, 11),
    ];
    let inline_ptrs: Vec<usize> = inlines.iter().map(ptr_of).collect();
    let para = Block::Paragraph(Paragraph {
        content: inlines,
        source_info: si(0, 11),
    });
    let pandoc = Pandoc {
        blocks: vec![para],
        meta: empty_meta(),
    };
    let block_ptr_val = block_ptr(&pandoc.blocks[0]);

    let mut by_node: HashMap<usize, HtmlAttributionRecord> = HashMap::new();
    let a = alice();
    let b = bob();
    // Block-level attribution: alice (arbitrary — first author).
    by_node.insert(
        block_ptr_val,
        HtmlAttributionRecord {
            actor: Arc::clone(&a),
            time: 1,
        },
    );
    by_node.insert(
        inline_ptrs[0],
        HtmlAttributionRecord {
            actor: Arc::clone(&a),
            time: 1,
        },
    );
    by_node.insert(
        inline_ptrs[2],
        HtmlAttributionRecord {
            actor: Arc::clone(&b),
            time: 2,
        },
    );

    let config = HtmlConfig {
        include_source_locations: false,
        attribution_by_node: Some(Arc::new(by_node)),
    };

    let body = render_body(&pandoc, config);

    let alice_wrappers = body.matches("<span data-attr-actor=\"alice\"").count();
    let bob_wrappers = body.matches("<span data-attr-actor=\"bob\"").count();
    // <p> tag itself carries one data-attr-actor="alice" so total alice = 2
    // (block + one prose wrapper). bob_wrappers = 1.
    assert_eq!(
        alice_wrappers, 1,
        "expected 1 alice prose wrapper; got {}\nbody:\n{}",
        alice_wrappers, body
    );
    assert_eq!(
        bob_wrappers, 1,
        "expected 1 bob prose wrapper; got {}\nbody:\n{}",
        bob_wrappers, body
    );
}

// ===========================================================================
// Test #7d — structured inlines break prose coalescing.
// ===========================================================================
#[test]
fn structured_inline_breaks_prose_coalescing() {
    // [Str("hello", alice), Code("world", alice), Str("foo", alice)]
    // All map to (alice, 1). Expected: THREE attribution wrappers —
    // one outer span for Str("hello"), one own wrapper on <code> for
    // Code("world"), one outer span for Str("foo").
    let inlines = vec![
        make_str("hello", 0, 5),
        make_code("world", 5, 10),
        make_str("foo", 10, 13),
    ];
    let inline_ptrs: Vec<usize> = inlines.iter().map(ptr_of).collect();
    let para = Block::Paragraph(Paragraph {
        content: inlines,
        source_info: si(0, 13),
    });
    let pandoc = Pandoc {
        blocks: vec![para],
        meta: empty_meta(),
    };
    let block_ptr_val = block_ptr(&pandoc.blocks[0]);

    let mut by_node: HashMap<usize, HtmlAttributionRecord> = HashMap::new();
    let a = alice();
    for ptr in [
        block_ptr_val,
        inline_ptrs[0],
        inline_ptrs[1],
        inline_ptrs[2],
    ] {
        by_node.insert(
            ptr,
            HtmlAttributionRecord {
                actor: Arc::clone(&a),
                time: 1,
            },
        );
    }

    let config = HtmlConfig {
        include_source_locations: false,
        attribution_by_node: Some(Arc::new(by_node)),
    };

    let body = render_body(&pandoc, config);

    // Three attribution-bearing elements: <p>, two <span>, one <code>.
    // The total count of `data-attr-actor="alice"` substrings is 4
    // (block + 3 inline-level carriers).
    let actor_attrs = body.matches("data-attr-actor=\"alice\"").count();
    assert_eq!(
        actor_attrs, 4,
        "expected 4 data-attr-actor occurrences (<p> + Str wrapper + <code> + Str wrapper); got {}\nbody:\n{}",
        actor_attrs, body
    );

    // Specifically: <code data-attr-actor="alice" ...> appears once,
    // and the two Str inlines do not get merged with it.
    assert!(
        body.contains("<code") && body.contains("data-attr-actor=\"alice\""),
        "code element carries its own attribution attrs; body:\n{}",
        body
    );

    // Two distinct outer <span data-attr-actor="alice"> wrappers
    // (one before, one after the <code>).
    let outer_spans = body.matches("<span data-attr-actor=\"alice\"").count();
    assert_eq!(
        outer_spans, 2,
        "expected 2 outer prose spans (one each side of <code>); got {}\nbody:\n{}",
        outer_spans, body
    );
}

// ===========================================================================
// Test #7c — attribution on, source-locations off compose orthogonally.
// ===========================================================================
#[test]
fn attribution_on_source_locations_off_produces_outer_wrapper_no_inner_span() {
    // Same three-Str fixture as test #7b, source-locations off.
    let inlines = vec![
        make_str("hello", 0, 5),
        make_space(5),
        make_str("there", 6, 11),
    ];
    let inline_ptrs: Vec<usize> = inlines.iter().map(ptr_of).collect();
    let para = Block::Paragraph(Paragraph {
        content: inlines,
        source_info: si(0, 11),
    });
    let pandoc = Pandoc {
        blocks: vec![para],
        meta: empty_meta(),
    };
    let block_ptr_val = block_ptr(&pandoc.blocks[0]);

    let mut by_node: HashMap<usize, HtmlAttributionRecord> = HashMap::new();
    let a = alice();
    by_node.insert(
        block_ptr_val,
        HtmlAttributionRecord {
            actor: Arc::clone(&a),
            time: 1,
        },
    );
    by_node.insert(
        inline_ptrs[0],
        HtmlAttributionRecord {
            actor: Arc::clone(&a),
            time: 1,
        },
    );
    by_node.insert(
        inline_ptrs[2],
        HtmlAttributionRecord {
            actor: Arc::clone(&a),
            time: 1,
        },
    );

    let config = HtmlConfig {
        include_source_locations: false,
        attribution_by_node: Some(Arc::new(by_node)),
    };

    let body = render_body(&pandoc, config);

    // No data-sid / data-loc anywhere.
    assert!(
        !body.contains("data-sid"),
        "no data-sid expected when source-locations off; body:\n{}",
        body
    );
    assert!(
        !body.contains("data-loc"),
        "no data-loc expected when source-locations off; body:\n{}",
        body
    );

    // One outer prose wrapper carries the per-node attribution attrs.
    let outer_open_count = body.matches("<span data-attr-actor=\"alice\"").count();
    assert_eq!(
        outer_open_count, 1,
        "exactly one outer prose wrapper; got {}\nbody:\n{}",
        outer_open_count, body
    );

    // No inner <span> around Str text inside the outer wrapper.
    // Pattern: the outer wrapper opens with `<span data-attr-actor=...>`
    // and its first inner content should be the raw escaped text
    // "hello", not another `<span`.
    let outer_open = body
        .find("<span data-attr-actor=\"alice\"")
        .expect("outer span present");
    let outer_open_close = body[outer_open..]
        .find('>')
        .expect("outer span open tag closes")
        + outer_open
        + 1;
    let after_open = &body[outer_open_close..];
    assert!(
        after_open.starts_with("hello"),
        "outer wrapper first child must be raw text (no inner span); got: {}",
        &after_open[..after_open.len().min(60)]
    );

    // Per-node attribution carries only the keys writers need to
    // identify the run; identity (name/color) is resolved by CSS rules
    // emitted once per actor by `AttributionViewerTransform`.
    let head = &body[outer_open..outer_open_close];
    for attr in ["data-attr-actor", "data-attr-time"] {
        assert!(
            head.contains(attr),
            "outer wrapper missing {} attr; tag: {}",
            attr,
            head
        );
    }
    for attr in ["data-attr-name", "data-attr-color"] {
        assert!(
            !head.contains(attr),
            "per-node wrapper must NOT carry {} (identity is render-time CSS); tag: {}",
            attr,
            head
        );
    }
}
