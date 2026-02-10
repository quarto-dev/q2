/*
 * inline_splice_safety_tests.rs
 *
 * Tests for Phase 5b/5c: the inline splice safety check and inline utilities.
 * Tests `is_inline_splice_safe()`, `inline_subtree_has_break()`, `inline_children()`,
 * `inline_source_span()`, and `write_inline_to_string()`.
 *
 * See: claude-notes/plans/2026-02-10-inline-splicing.md
 * Beads issue: bd-1hwd
 *
 * Copyright (c) 2026 Posit, PBC
 */

use hashlink::LinkedHashMap;
use pampa::pandoc::{Block, Inline};
use pampa::writers::incremental::{
    inline_children, inline_source_span, inline_subtree_has_break, is_inline_splice_safe,
    write_inline_to_string,
};
use quarto_ast_reconcile::types::{InlineAlignment, InlineReconciliationPlan};
use quarto_source_map::SourceInfo;

// =============================================================================
// Helpers: construct Inline nodes for testing
// =============================================================================

fn si() -> SourceInfo {
    SourceInfo::default()
}

fn make_str(text: &str) -> Inline {
    Inline::Str(quarto_pandoc_types::inline::Str {
        text: text.to_string(),
        source_info: si(),
    })
}

fn make_space() -> Inline {
    Inline::Space(quarto_pandoc_types::inline::Space { source_info: si() })
}

fn make_softbreak() -> Inline {
    Inline::SoftBreak(quarto_pandoc_types::inline::SoftBreak { source_info: si() })
}

fn make_linebreak() -> Inline {
    Inline::LineBreak(quarto_pandoc_types::inline::LineBreak { source_info: si() })
}

fn make_emph(content: Vec<Inline>) -> Inline {
    Inline::Emph(quarto_pandoc_types::inline::Emph {
        content,
        source_info: si(),
    })
}

fn make_strong(content: Vec<Inline>) -> Inline {
    Inline::Strong(quarto_pandoc_types::inline::Strong {
        content,
        source_info: si(),
    })
}

fn make_code(text: &str) -> Inline {
    Inline::Code(quarto_pandoc_types::inline::Code {
        text: text.to_string(),
        attr: quarto_pandoc_types::attr::Attr::default(),
        source_info: si(),
        attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
    })
}

fn make_plan(alignments: Vec<InlineAlignment>) -> InlineReconciliationPlan {
    InlineReconciliationPlan {
        inline_alignments: alignments,
        inline_container_plans: LinkedHashMap::new(),
        note_block_plans: LinkedHashMap::new(),
        custom_node_plans: LinkedHashMap::new(),
    }
}

fn make_plan_with_container(
    alignments: Vec<InlineAlignment>,
    container_plans: Vec<(usize, InlineReconciliationPlan)>,
) -> InlineReconciliationPlan {
    let mut plans = LinkedHashMap::new();
    for (idx, plan) in container_plans {
        plans.insert(idx, plan);
    }
    InlineReconciliationPlan {
        inline_alignments: alignments,
        inline_container_plans: plans,
        note_block_plans: LinkedHashMap::new(),
        custom_node_plans: LinkedHashMap::new(),
    }
}

// =============================================================================
// Tests for inline_children()
// =============================================================================

#[test]
fn inline_children_of_leaf_nodes() {
    // Leaf nodes should return empty slice
    assert!(inline_children(&make_str("hello")).is_empty());
    assert!(inline_children(&make_space()).is_empty());
    assert!(inline_children(&make_softbreak()).is_empty());
    assert!(inline_children(&make_linebreak()).is_empty());
    assert!(inline_children(&make_code("x")).is_empty());
}

#[test]
fn inline_children_of_emph() {
    let emph = make_emph(vec![make_str("hello"), make_space(), make_str("world")]);
    let children = inline_children(&emph);
    assert_eq!(children.len(), 3);
}

#[test]
fn inline_children_of_strong() {
    let strong = make_strong(vec![make_str("bold")]);
    let children = inline_children(&strong);
    assert_eq!(children.len(), 1);
}

#[test]
fn inline_children_of_nested_containers() {
    // Strong > Emph > Str
    let emph = make_emph(vec![make_str("deep")]);
    let strong = make_strong(vec![emph]);
    let children = inline_children(&strong);
    assert_eq!(children.len(), 1);
    // The child is Emph, which itself has children
    assert_eq!(inline_children(&children[0]).len(), 1);
}

// =============================================================================
// Tests for inline_subtree_has_break()
// =============================================================================

#[test]
fn subtree_has_break_leaf_str() {
    assert!(!inline_subtree_has_break(&make_str("hello")));
}

#[test]
fn subtree_has_break_leaf_space() {
    assert!(!inline_subtree_has_break(&make_space()));
}

#[test]
fn subtree_has_break_leaf_code() {
    assert!(!inline_subtree_has_break(&make_code("x")));
}

#[test]
fn subtree_has_break_softbreak() {
    assert!(inline_subtree_has_break(&make_softbreak()));
}

#[test]
fn subtree_has_break_linebreak() {
    assert!(inline_subtree_has_break(&make_linebreak()));
}

#[test]
fn subtree_has_break_emph_no_break() {
    let emph = make_emph(vec![make_str("hello"), make_space(), make_str("world")]);
    assert!(!inline_subtree_has_break(&emph));
}

#[test]
fn subtree_has_break_emph_with_softbreak() {
    let emph = make_emph(vec![make_str("hello"), make_softbreak(), make_str("world")]);
    assert!(inline_subtree_has_break(&emph));
}

#[test]
fn subtree_has_break_emph_with_linebreak() {
    let emph = make_emph(vec![make_str("hello"), make_linebreak(), make_str("world")]);
    assert!(inline_subtree_has_break(&emph));
}

#[test]
fn subtree_has_break_nested_deep() {
    // Strong > Emph > [Str, SoftBreak, Str]
    let emph = make_emph(vec![make_str("a"), make_softbreak(), make_str("b")]);
    let strong = make_strong(vec![emph]);
    assert!(inline_subtree_has_break(&strong));
}

#[test]
fn subtree_has_break_nested_no_break() {
    // Strong > Emph > [Str, Space, Str]
    let emph = make_emph(vec![make_str("a"), make_space(), make_str("b")]);
    let strong = make_strong(vec![emph]);
    assert!(!inline_subtree_has_break(&strong));
}

// =============================================================================
// Tests for is_inline_splice_safe()
// =============================================================================

// --- Scenario A: Zero breaks, simple cases ---

#[test]
fn safe_str_change_only() {
    // Original: [Str("Hello"), Space, Str("world")]
    // New:      [Str("Goodbye"), Space, Str("world")]
    // Plan: [UseAfter(0), KeepBefore(1), KeepBefore(2)]
    let new_inlines = vec![make_str("Goodbye"), make_space(), make_str("world")];
    let plan = make_plan(vec![
        InlineAlignment::UseAfter(0),
        InlineAlignment::KeepBefore(1),
        InlineAlignment::KeepBefore(2),
    ]);
    assert!(is_inline_splice_safe(&new_inlines, &plan));
}

#[test]
fn safe_all_keep_before() {
    // Nothing changed — all KeepBefore
    let new_inlines = vec![make_str("Hello"), make_space(), make_str("world")];
    let plan = make_plan(vec![
        InlineAlignment::KeepBefore(0),
        InlineAlignment::KeepBefore(1),
        InlineAlignment::KeepBefore(2),
    ]);
    assert!(is_inline_splice_safe(&new_inlines, &plan));
}

#[test]
fn safe_all_use_after_no_breaks() {
    // All inlines are replaced but none contain breaks
    let new_inlines = vec![make_str("A"), make_space(), make_str("B")];
    let plan = make_plan(vec![
        InlineAlignment::UseAfter(0),
        InlineAlignment::UseAfter(1),
        InlineAlignment::UseAfter(2),
    ]);
    assert!(is_inline_splice_safe(&new_inlines, &plan));
}

#[test]
fn safe_emph_text_changed() {
    // Emph containing only Str children, text changed
    // Original: [Emph([Str("Hello")])]
    // New:      [Emph([Str("World")])]
    // Plan: RecurseIntoContainer, inner: [UseAfter(0)]
    let new_inlines = vec![make_emph(vec![make_str("World")])];

    let inner_plan = make_plan(vec![InlineAlignment::UseAfter(0)]);
    let plan = make_plan_with_container(
        vec![InlineAlignment::RecurseIntoContainer {
            before_idx: 0,
            after_idx: 0,
        }],
        vec![(0, inner_plan)],
    );
    assert!(is_inline_splice_safe(&new_inlines, &plan));
}

#[test]
fn safe_code_change() {
    // Code inline replaced (leaf, no children)
    let new_inlines = vec![
        make_str("Use"),
        make_space(),
        make_code("new_fn"),
        make_str("."),
    ];
    let plan = make_plan(vec![
        InlineAlignment::KeepBefore(0),
        InlineAlignment::KeepBefore(1),
        InlineAlignment::UseAfter(2),
        InlineAlignment::KeepBefore(3),
    ]);
    assert!(is_inline_splice_safe(&new_inlines, &plan));
}

// --- Scenario B: Breaks exist, all KeepBefore ---

#[test]
fn safe_scenario_b_breaks_all_kept() {
    // Original: [Str("Hello"), SoftBreak, Str("world")]
    // New:      [Str("Goodbye"), SoftBreak, Str("world")]
    // Plan: [UseAfter(0), KeepBefore(1), KeepBefore(2)]
    // The SoftBreak is KeepBefore → preserved verbatim → safe
    let new_inlines = vec![make_str("Goodbye"), make_softbreak(), make_str("world")];
    let plan = make_plan(vec![
        InlineAlignment::UseAfter(0),
        InlineAlignment::KeepBefore(1),
        InlineAlignment::KeepBefore(2),
    ]);
    assert!(is_inline_splice_safe(&new_inlines, &plan));
}

#[test]
fn safe_scenario_b_linebreak_kept() {
    // Same as above but with LineBreak instead of SoftBreak
    let new_inlines = vec![make_str("Goodbye"), make_linebreak(), make_str("world")];
    let plan = make_plan(vec![
        InlineAlignment::UseAfter(0),
        InlineAlignment::KeepBefore(1),
        InlineAlignment::KeepBefore(2),
    ]);
    assert!(is_inline_splice_safe(&new_inlines, &plan));
}

#[test]
fn safe_scenario_b_multiple_breaks_all_kept() {
    // Three-line paragraph: [Str, SoftBreak, Str, SoftBreak, Str]
    // Only middle Str changes
    let new_inlines = vec![
        make_str("Hello"),
        make_softbreak(),
        make_str("beautiful"),
        make_softbreak(),
        make_str("world"),
    ];
    let plan = make_plan(vec![
        InlineAlignment::KeepBefore(0),
        InlineAlignment::KeepBefore(1),
        InlineAlignment::UseAfter(2),
        InlineAlignment::KeepBefore(3),
        InlineAlignment::KeepBefore(4),
    ]);
    assert!(is_inline_splice_safe(&new_inlines, &plan));
}

#[test]
fn safe_scenario_b_recurse_into_emph_with_kept_break() {
    // RecurseIntoContainer for Emph, inner Str changed but SoftBreak is KeepBefore
    // Original: [Emph([Str("Hello"), SoftBreak, Str("world")])]
    // New:      [Emph([Str("Goodbye"), SoftBreak, Str("world")])]
    // Plan: RecurseIntoContainer, inner: [UseAfter(0), KeepBefore(1), KeepBefore(2)]
    let new_inlines = vec![make_emph(vec![
        make_str("Goodbye"),
        make_softbreak(),
        make_str("world"),
    ])];

    let inner_plan = make_plan(vec![
        InlineAlignment::UseAfter(0),
        InlineAlignment::KeepBefore(1),
        InlineAlignment::KeepBefore(2),
    ]);
    let plan = make_plan_with_container(
        vec![InlineAlignment::RecurseIntoContainer {
            before_idx: 0,
            after_idx: 0,
        }],
        vec![(0, inner_plan)],
    );
    assert!(is_inline_splice_safe(&new_inlines, &plan));
}

// --- Unsafe cases ---

#[test]
fn unsafe_use_after_softbreak() {
    // Adding a SoftBreak via UseAfter
    // New: [Str("Hello"), SoftBreak, Str("world")]
    // Plan: [KeepBefore(0), UseAfter(1), UseAfter(2)]
    let new_inlines = vec![make_str("Hello"), make_softbreak(), make_str("world")];
    let plan = make_plan(vec![
        InlineAlignment::KeepBefore(0),
        InlineAlignment::UseAfter(1),
        InlineAlignment::UseAfter(2),
    ]);
    assert!(!is_inline_splice_safe(&new_inlines, &plan));
}

#[test]
fn unsafe_use_after_linebreak() {
    // Adding a LineBreak via UseAfter
    let new_inlines = vec![make_str("Hello"), make_linebreak(), make_str("world")];
    let plan = make_plan(vec![
        InlineAlignment::KeepBefore(0),
        InlineAlignment::UseAfter(1),
        InlineAlignment::UseAfter(2),
    ]);
    assert!(!is_inline_splice_safe(&new_inlines, &plan));
}

#[test]
fn unsafe_use_after_emph_with_softbreak() {
    // UseAfter for an Emph whose subtree contains a SoftBreak
    let emph_with_break = make_emph(vec![make_str("Hello"), make_softbreak(), make_str("world")]);
    let new_inlines = vec![emph_with_break];
    let plan = make_plan(vec![InlineAlignment::UseAfter(0)]);
    assert!(!is_inline_splice_safe(&new_inlines, &plan));
}

#[test]
fn unsafe_use_after_nested_break() {
    // UseAfter for Strong > Emph > [Str, SoftBreak, Str]
    // The break is deeply nested but still unsafe
    let emph = make_emph(vec![make_str("a"), make_softbreak(), make_str("b")]);
    let strong = make_strong(vec![emph]);
    let new_inlines = vec![strong];
    let plan = make_plan(vec![InlineAlignment::UseAfter(0)]);
    assert!(!is_inline_splice_safe(&new_inlines, &plan));
}

#[test]
fn unsafe_recurse_into_emph_with_written_break() {
    // RecurseIntoContainer for Emph, and the nested plan writes a SoftBreak
    // New: [Emph([Str("Hello"), SoftBreak, Str("world")])]
    // Inner plan: [UseAfter(0), UseAfter(1), UseAfter(2)]
    // The SoftBreak is UseAfter → we'd write it → unsafe
    let new_inlines = vec![make_emph(vec![
        make_str("Hello"),
        make_softbreak(),
        make_str("world"),
    ])];

    let inner_plan = make_plan(vec![
        InlineAlignment::UseAfter(0),
        InlineAlignment::UseAfter(1),
        InlineAlignment::UseAfter(2),
    ]);
    let plan = make_plan_with_container(
        vec![InlineAlignment::RecurseIntoContainer {
            before_idx: 0,
            after_idx: 0,
        }],
        vec![(0, inner_plan)],
    );
    assert!(!is_inline_splice_safe(&new_inlines, &plan));
}

// --- Mixed safe/unsafe: one unsafe makes the whole plan unsafe ---

#[test]
fn unsafe_one_bad_among_many_good() {
    // Multiple inlines, mostly safe, but one UseAfter contains a SoftBreak
    // [Str("A"), Space, Str("B"), SoftBreak, Str("C")]
    // Plan: [KeepBefore(0), KeepBefore(1), UseAfter(2), UseAfter(3), UseAfter(4)]
    let new_inlines = vec![
        make_str("A"),
        make_space(),
        make_str("B"),
        make_softbreak(),
        make_str("C"),
    ];
    let plan = make_plan(vec![
        InlineAlignment::KeepBefore(0),
        InlineAlignment::KeepBefore(1),
        InlineAlignment::UseAfter(2),
        InlineAlignment::UseAfter(3),
        InlineAlignment::UseAfter(4),
    ]);
    assert!(!is_inline_splice_safe(&new_inlines, &plan));
}

#[test]
fn safe_multiple_changes_all_safe() {
    // Multiple UseAfter inlines, all break-free
    let new_inlines = vec![
        make_str("New1"),
        make_space(),
        make_str("New2"),
        make_space(),
        make_str("New3"),
    ];
    let plan = make_plan(vec![
        InlineAlignment::UseAfter(0),
        InlineAlignment::UseAfter(1),
        InlineAlignment::UseAfter(2),
        InlineAlignment::UseAfter(3),
        InlineAlignment::UseAfter(4),
    ]);
    assert!(is_inline_splice_safe(&new_inlines, &plan));
}

// --- Edge cases ---

#[test]
fn safe_empty_plan() {
    // No inlines, no alignments
    let new_inlines: Vec<Inline> = vec![];
    let plan = make_plan(vec![]);
    assert!(is_inline_splice_safe(&new_inlines, &plan));
}

#[test]
fn safe_single_str_use_after() {
    // Simplest possible splice: single Str replaced
    let new_inlines = vec![make_str("Hello")];
    let plan = make_plan(vec![InlineAlignment::UseAfter(0)]);
    assert!(is_inline_splice_safe(&new_inlines, &plan));
}

#[test]
fn safe_recurse_no_nested_plan() {
    // RecurseIntoContainer but no nested plan → container content is identical
    let new_inlines = vec![make_emph(vec![make_str("same")])];
    let plan = make_plan(vec![InlineAlignment::RecurseIntoContainer {
        before_idx: 0,
        after_idx: 0,
    }]);
    // No nested plan in inline_container_plans → treated as safe (content identical)
    assert!(is_inline_splice_safe(&new_inlines, &plan));
}

#[test]
fn safe_deeply_nested_recurse_all_safe() {
    // Strong > Emph > [Str changed]
    // Outer: RecurseIntoContainer at 0 with nested plan
    // Inner (Strong children): RecurseIntoContainer at 0 with nested plan
    // Innermost (Emph children): [UseAfter(0)]
    let new_inlines = vec![make_strong(vec![make_emph(vec![make_str("deep")])])];

    let innermost_plan = make_plan(vec![InlineAlignment::UseAfter(0)]);
    let inner_plan = make_plan_with_container(
        vec![InlineAlignment::RecurseIntoContainer {
            before_idx: 0,
            after_idx: 0,
        }],
        vec![(0, innermost_plan)],
    );
    let plan = make_plan_with_container(
        vec![InlineAlignment::RecurseIntoContainer {
            before_idx: 0,
            after_idx: 0,
        }],
        vec![(0, inner_plan)],
    );
    assert!(is_inline_splice_safe(&new_inlines, &plan));
}

#[test]
fn unsafe_deeply_nested_recurse_break_at_bottom() {
    // Strong > Emph > [Str, SoftBreak (UseAfter), Str]
    // Break at the bottom level written via UseAfter → unsafe
    let new_inlines = vec![make_strong(vec![make_emph(vec![
        make_str("a"),
        make_softbreak(),
        make_str("b"),
    ])])];

    let innermost_plan = make_plan(vec![
        InlineAlignment::UseAfter(0),
        InlineAlignment::UseAfter(1), // SoftBreak written → unsafe
        InlineAlignment::UseAfter(2),
    ]);
    let inner_plan = make_plan_with_container(
        vec![InlineAlignment::RecurseIntoContainer {
            before_idx: 0,
            after_idx: 0,
        }],
        vec![(0, innermost_plan)],
    );
    let plan = make_plan_with_container(
        vec![InlineAlignment::RecurseIntoContainer {
            before_idx: 0,
            after_idx: 0,
        }],
        vec![(0, inner_plan)],
    );
    assert!(!is_inline_splice_safe(&new_inlines, &plan));
}

#[test]
fn safe_deeply_nested_recurse_break_kept() {
    // Strong > Emph > [Str (UseAfter), SoftBreak (KeepBefore), Str (KeepBefore)]
    // Break at the bottom level but KeepBefore → safe
    let new_inlines = vec![make_strong(vec![make_emph(vec![
        make_str("goodbye"),
        make_softbreak(),
        make_str("b"),
    ])])];

    let innermost_plan = make_plan(vec![
        InlineAlignment::UseAfter(0),
        InlineAlignment::KeepBefore(1), // SoftBreak kept → safe
        InlineAlignment::KeepBefore(2),
    ]);
    let inner_plan = make_plan_with_container(
        vec![InlineAlignment::RecurseIntoContainer {
            before_idx: 0,
            after_idx: 0,
        }],
        vec![(0, innermost_plan)],
    );
    let plan = make_plan_with_container(
        vec![InlineAlignment::RecurseIntoContainer {
            before_idx: 0,
            after_idx: 0,
        }],
        vec![(0, inner_plan)],
    );
    assert!(is_inline_splice_safe(&new_inlines, &plan));
}

// =============================================================================
// Tests for inline_source_span() (Phase 5c)
// =============================================================================

fn parse_qmd(input: &str) -> pampa::pandoc::Pandoc {
    let result = pampa::readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    );
    result.expect("Failed to parse QMD").0
}

fn first_block_inlines(doc: &pampa::pandoc::Pandoc) -> &[Inline] {
    match &doc.blocks[0] {
        Block::Paragraph(p) => &p.content,
        Block::Plain(p) => &p.content,
        Block::Header(h) => &h.content,
        other => panic!(
            "Expected Paragraph/Plain/Header, got {:?}",
            std::mem::discriminant(other)
        ),
    }
}

#[test]
fn source_span_str_in_paragraph() {
    let input = "Hello world\n";
    let doc = parse_qmd(input);
    let inlines = first_block_inlines(&doc);
    // First inline should be Str("Hello")
    let span = inline_source_span(&inlines[0]);
    assert_eq!(&input[span.clone()], "Hello");
    assert_eq!(span, 0..5);
}

#[test]
fn source_span_space() {
    let input = "Hello world\n";
    let doc = parse_qmd(input);
    let inlines = first_block_inlines(&doc);
    // Second inline should be Space
    let span = inline_source_span(&inlines[1]);
    assert_eq!(&input[span.clone()], " ");
    assert_eq!(span.end - span.start, 1);
}

#[test]
fn source_span_emph_includes_delimiters() {
    let input = "*Hello* world\n";
    let doc = parse_qmd(input);
    let inlines = first_block_inlines(&doc);
    let span = inline_source_span(&inlines[0]);
    assert_eq!(&input[span.clone()], "*Hello*");
}

#[test]
fn source_span_emph_child_excludes_delimiters() {
    let input = "*Hello* world\n";
    let doc = parse_qmd(input);
    let inlines = first_block_inlines(&doc);
    let children = inline_children(&inlines[0]);
    let child_span = inline_source_span(&children[0]);
    assert_eq!(&input[child_span.clone()], "Hello");
}

#[test]
fn source_span_softbreak_in_blockquote() {
    let input = "> Hello\n> world\n";
    let doc = parse_qmd(input);
    let bq = match &doc.blocks[0] {
        Block::BlockQuote(bq) => bq,
        other => panic!(
            "Expected BlockQuote, got {:?}",
            std::mem::discriminant(other)
        ),
    };
    let inner_inlines = match &bq.content[0] {
        Block::Paragraph(p) => &p.content,
        Block::Plain(p) => &p.content,
        other => panic!(
            "Expected Paragraph/Plain, got {:?}",
            std::mem::discriminant(other)
        ),
    };
    // SoftBreak should include the indentation prefix
    let sb_idx = inner_inlines
        .iter()
        .position(|i| matches!(i, Inline::SoftBreak(_)))
        .expect("Expected SoftBreak");
    let sb_span = inline_source_span(&inner_inlines[sb_idx]);
    // SoftBreak absorbs "\n> " (3 bytes)
    assert_eq!(sb_span.end - sb_span.start, 3);
    assert_eq!(&input[sb_span], "\n> ");
}

#[test]
fn source_spans_tile_perfectly() {
    // All inline spans should tile with zero gaps in a simple paragraph
    let input = "Hello world today\n";
    let doc = parse_qmd(input);
    let inlines = first_block_inlines(&doc);

    let mut prev_end = None;
    for inline in inlines {
        let span = inline_source_span(inline);
        if let Some(prev) = prev_end {
            assert_eq!(
                span.start, prev,
                "Gap detected between inlines at byte offset {}",
                prev
            );
        }
        prev_end = Some(span.end);
    }
}

// =============================================================================
// Tests for write_inline_to_string() (Phase 5c)
// =============================================================================

#[test]
fn write_inline_str() {
    let result = write_inline_to_string(&make_str("Hello")).unwrap();
    assert_eq!(result, "Hello");
}

#[test]
fn write_inline_space() {
    let result = write_inline_to_string(&make_space()).unwrap();
    assert_eq!(result, " ");
}

#[test]
fn write_inline_emph() {
    let emph = make_emph(vec![make_str("Hello")]);
    let result = write_inline_to_string(&emph).unwrap();
    assert_eq!(result, "*Hello*");
}

#[test]
fn write_inline_strong() {
    let strong = make_strong(vec![make_str("Bold")]);
    let result = write_inline_to_string(&strong).unwrap();
    assert_eq!(result, "**Bold**");
}

#[test]
fn write_inline_code() {
    let result = write_inline_to_string(&make_code("fn()")).unwrap();
    assert_eq!(result, "`fn()`");
}

#[test]
fn write_inline_emph_with_space() {
    let emph = make_emph(vec![make_str("Hello"), make_space(), make_str("world")]);
    let result = write_inline_to_string(&emph).unwrap();
    assert_eq!(result, "*Hello world*");
}

#[test]
fn write_inline_no_newlines_for_leaf_nodes() {
    // Verify that none of the common leaf inlines produce newlines
    for inline in [make_str("test"), make_space(), make_code("x")] {
        let result = write_inline_to_string(&inline).unwrap();
        assert!(
            !result.contains('\n'),
            "Leaf inline produced newline: {:?}",
            result
        );
    }
}

#[test]
fn write_inline_no_newlines_for_break_free_containers() {
    // Containers without breaks should produce no newlines
    let inlines = vec![
        make_emph(vec![make_str("em")]),
        make_strong(vec![make_str("strong")]),
        make_strong(vec![make_emph(vec![make_str("nested")])]),
    ];
    for inline in &inlines {
        let result = write_inline_to_string(inline).unwrap();
        assert!(
            !result.contains('\n'),
            "Break-free container produced newline: {:?}",
            result
        );
    }
}
