/*
 * incremental_writer_tests.rs
 *
 * Tests for the incremental QMD writer.
 * See: claude-notes/plans/2026-02-07-incremental-writer.md
 *
 * Copyright (c) 2026 Posit, PBC
 */

use pampa::pandoc::Pandoc;
use pampa::writers;
use proptest::prelude::*;
use quarto_ast_reconcile::compute_reconciliation;
use std::io::Cursor;

// =============================================================================
// Helpers
// =============================================================================

/// Parse a QMD string into a Pandoc AST with accurate source spans.
fn parse_qmd(input: &str) -> Pandoc {
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

/// Write a Pandoc AST to a QMD string using the standard writer.
fn write_qmd(ast: &Pandoc) -> String {
    let mut buf = Vec::new();
    writers::qmd::write(ast, &mut buf).expect("Failed to write QMD");
    String::from_utf8(buf).expect("Writer produced invalid UTF-8")
}

/// Write a Pandoc AST to JSON.
fn write_json(ast: &Pandoc) -> String {
    let mut buf = Vec::new();
    let context = pampa::pandoc::ASTContext::default();
    writers::json::write(ast, &context, &mut buf).expect("Failed to write JSON");
    String::from_utf8(buf).expect("Writer produced invalid UTF-8")
}

/// Read a Pandoc AST from JSON.
fn read_json(json: &str) -> Pandoc {
    let mut cursor = Cursor::new(json.as_bytes());
    pampa::readers::json::read(&mut cursor)
        .expect("Failed to read JSON")
        .0
}

/// Simulate the WASM incremental_write_qmd path (Plan 7 contract):
/// 1. Parse original_qmd to get the baseline AST with accurate source spans
///    (in the real bridge the caller supplies this; here we synthesize it
///    from the qmd to keep the helper self-contained)
/// 2. JSON round-trip the new_ast (simulates client serialization/deserialization)
/// 3. Compute reconciliation plan and run incremental_write
fn incremental_write_via_json_roundtrip(original_qmd: &str, new_ast: &Pandoc) -> String {
    let original_ast = parse_qmd(original_qmd);
    let json = write_json(new_ast);
    let new_ast_from_json = read_json(&json);
    let plan = compute_reconciliation(&original_ast, &new_ast_from_json);
    writers::incremental::incremental_write(original_qmd, &original_ast, &new_ast_from_json, &plan)
        .expect("incremental_write failed")
        .0
}

// =============================================================================
// Property 2: Idempotence — incremental_write(qmd, ast, ast, identity_plan) == qmd
// =============================================================================

/// Test idempotence for a given QMD input.
/// The incremental writer with no changes should produce byte-for-byte identical output.
fn assert_idempotent(input: &str) {
    let ast = parse_qmd(input);
    let plan = compute_reconciliation(&ast, &ast);

    // All blocks should be KeepBefore (hash-matched to themselves)
    for alignment in &plan.block_alignments {
        assert!(
            matches!(
                alignment,
                quarto_ast_reconcile::types::BlockAlignment::KeepBefore(_)
            ),
            "Expected all KeepBefore for identity reconciliation, got {:?}",
            alignment
        );
    }

    let result = writers::incremental::incremental_write(input, &ast, &ast, &plan)
        .expect("incremental_write failed")
        .0;

    assert_eq!(
        result, input,
        "Idempotence violated:\n--- expected ---\n{:?}\n--- got ---\n{:?}",
        input, result
    );
}

// --- Simple documents ---

#[test]
fn idempotent_single_paragraph() {
    assert_idempotent("Hello world.\n");
}

#[test]
fn idempotent_two_paragraphs() {
    assert_idempotent("First paragraph.\n\nSecond paragraph.\n");
}

#[test]
fn idempotent_three_paragraphs() {
    assert_idempotent("First.\n\nSecond.\n\nThird.\n");
}

// --- Headers ---

#[test]
fn idempotent_header_and_paragraph() {
    assert_idempotent("## Title\n\nA paragraph.\n");
}

#[test]
fn idempotent_multiple_headers() {
    assert_idempotent("# Title\n\n## Subtitle\n\nContent.\n\n### Sub-subtitle\n\nMore content.\n");
}

// --- Code blocks ---

#[test]
fn idempotent_code_block() {
    assert_idempotent("Before.\n\n```python\nprint('hello')\n```\n\nAfter.\n");
}

// --- Horizontal rule ---

#[test]
fn idempotent_horizontal_rule() {
    assert_idempotent("Before.\n\n***\n\nAfter.\n");
}

// --- Block quotes ---

#[test]
fn idempotent_block_quote() {
    assert_idempotent("Before.\n\n> Quoted text.\n\nAfter.\n");
}

#[test]
fn idempotent_block_quote_multiline() {
    assert_idempotent("Before.\n\n> First line.\n> Second line.\n\nAfter.\n");
}

// --- Bullet lists ---

#[test]
fn idempotent_bullet_list() {
    assert_idempotent("Before.\n\n* First item\n* Second item\n* Third item\n\nAfter.\n");
}

#[test]
fn idempotent_bullet_list_at_end() {
    // BulletList trailing blank line quirk — the list span absorbs \n\n
    assert_idempotent("Before.\n\n* Item one\n* Item two\n");
}

// --- Ordered lists ---

#[test]
fn idempotent_ordered_list() {
    assert_idempotent("Before.\n\n1. First\n2. Second\n3. Third\n\nAfter.\n");
}

// --- Fenced divs ---

#[test]
fn idempotent_fenced_div() {
    assert_idempotent("Before.\n\n::: {.note}\n\nInner content.\n\n:::\n\nAfter.\n");
}

// --- YAML front matter ---

#[test]
fn idempotent_with_front_matter() {
    assert_idempotent("---\ntitle: Hello\n---\n\nA paragraph.\n");
}

#[test]
fn idempotent_with_front_matter_multiple_keys() {
    assert_idempotent("---\ntitle: Hello\nauthor: World\n---\n\nA paragraph.\n");
}

// =============================================================================
// Metadata gap preservation — bd-1kvf
// =============================================================================
//
// When the incremental writer rewrites a block (e.g., a Div with a toggled
// checkbox), the blank line between the YAML front matter and the first block
// must be preserved. This tests the WASM path where the new AST comes from
// JSON round-trip (which loses source_info accuracy in metadata).

#[test]
fn metadata_gap_preserved_when_block_rewritten_via_json() {
    // Document with front matter + blank line + div containing a checkbox list
    let original_qmd = "\
---
title: Hello
---

::: {#todo}

* [x] First item
* [x] Second item

:::
";

    // Parse to get the AST, then modify it (toggle first checkbox)
    let mut new_ast = parse_qmd(original_qmd);

    // Navigate: blocks[0] = Div -> content[0] = BulletList -> content[0][0] = Plain -> content[0] = Span
    // Toggle the first checkbox from [x] to [ ] by clearing the span content
    if let pampa::pandoc::Block::Div(ref mut div) = new_ast.blocks[0] {
        if let pampa::pandoc::Block::BulletList(ref mut bl) = div.content[0] {
            if let pampa::pandoc::Block::Plain(ref mut plain) = bl.content[0][0] {
                if let pampa::pandoc::Inline::Span(ref mut span) = plain.content[0] {
                    span.content.clear(); // Toggle [x] -> [ ]
                }
            }
        }
    }

    // Run through JSON round-trip path (simulates WASM/client)
    let result = incremental_write_via_json_roundtrip(original_qmd, &new_ast);

    // The blank line between --- and ::: {#todo} must be preserved
    assert!(
        result.contains("---\n\n::: {#todo}") || result.contains("---\n\n:::"),
        "Blank line between front matter and first block was lost!\nResult:\n{}",
        result
    );
}

#[test]
fn metadata_gap_preserved_when_paragraph_rewritten_via_json() {
    // Simpler case: front matter + blank line + paragraph
    let original_qmd = "\
---
title: Hello
---

A paragraph.
";

    let mut new_ast = parse_qmd(original_qmd);
    // Change the paragraph text
    if let pampa::pandoc::Block::Paragraph(ref mut p) = new_ast.blocks[0] {
        p.content = vec![pampa::pandoc::Inline::Str(pampa::pandoc::Str {
            text: "Modified paragraph.".to_string(),
            source_info: quarto_source_map::SourceInfo::for_test(),
        })];
    }

    let result = incremental_write_via_json_roundtrip(original_qmd, &new_ast);

    assert!(
        result.contains("---\n\n"),
        "Blank line between front matter and first block was lost!\nResult:\n{}",
        result
    );
}

#[test]
fn metadata_gap_preserved_identical_ast_via_json() {
    // Even with NO changes, JSON round-trip should preserve the gap
    let original_qmd = "\
---
title: Hello
---

A paragraph.
";

    let ast = parse_qmd(original_qmd);
    let result = incremental_write_via_json_roundtrip(original_qmd, &ast);

    assert_eq!(
        result, original_qmd,
        "Idempotence violated when AST goes through JSON round-trip!\nExpected:\n{:?}\nGot:\n{:?}",
        original_qmd, result
    );
}

// =============================================================================
// Sectionize wrapper soft-drop (incremental.rs RecurseIntoContainer regression)
// =============================================================================
//
// The post-q2-preview-pipeline AST wraps all user content in a single
// top-level `Block::Div` with `SourceInfo::Generated { by: sectionize }`
// (no Invocation anchor). When the React side mutates a child Para and
// posts the new AST, the reconciler aligns "1 Div : 1 Div" as a
// `RecurseIntoContainer`. The Plan 7 soft-drop guard in coarsen
// (`incremental.rs:342`) trips because `is_editable_inside_block` on a
// no-preimage Generated wrapper returns false — and since the *whole*
// document is the wrapper, the resulting `CoarsenedEntry::Omit`
// produces an empty document.
//
// The correct behavior: recurse Transparent into the wrapper's
// source-bearing children using `block_container_plans[result_idx]`,
// the same way `coarsen_keep_before_block` handles unchanged
// non-atomic Generated wrappers (`incremental.rs:459-479`).

/// Construct a `Pandoc` whose first (and only) top-level block is a
/// `Generated { by: sectionize }` Div wrapping the parsed AST of the
/// supplied qmd. The inner blocks retain their original Source positions.
fn wrap_in_sectionize_div(parsed: pampa::pandoc::Pandoc) -> pampa::pandoc::Pandoc {
    use pampa::pandoc::Block;
    let wrapper_si = quarto_source_map::SourceInfo::generated(quarto_source_map::By::sectionize());
    let wrapper = Block::Div(pampa::pandoc::Div {
        attr: (
            String::new(),
            vec!["section".to_string()],
            hashlink::LinkedHashMap::new(),
        ),
        content: parsed.blocks,
        source_info: wrapper_si,
        attr_source: pampa::pandoc::attr::AttrSourceInfo::empty(),
    });
    pampa::pandoc::Pandoc {
        blocks: vec![wrapper],
        ..parsed
    }
}

#[test]
fn sectionize_wrapper_with_inner_para_edit_produces_nonempty_output() {
    // Original qmd: a header followed by a paragraph.
    let original_qmd = "# Heading\n\nA paragraph that the user will edit.\n";

    // Baseline AST mirrors the post-pipeline shape: the whole document
    // wrapped in a sectionize Div.
    let baseline_ast = wrap_in_sectionize_div(parse_qmd(original_qmd));

    // New AST: copy baseline, dive into the Div's content, append a
    // reaction Span to the inner Paragraph (mirrors comment.tsx's
    // addReaction path).
    let mut new_ast = baseline_ast.clone();
    {
        let pampa::pandoc::Block::Div(ref mut div) = new_ast.blocks[0] else {
            panic!("expected wrapper Div at blocks[0]");
        };
        let last_idx = div
            .content
            .iter()
            .rposition(|b| matches!(b, pampa::pandoc::Block::Paragraph(_)))
            .expect("paragraph inside wrapper");
        if let pampa::pandoc::Block::Paragraph(ref mut p) = div.content[last_idx] {
            let attr = (
                String::new(),
                vec!["quarto-edit-comment".to_string()],
                hashlink::LinkedHashMap::new(),
            );
            p.content
                .push(pampa::pandoc::Inline::Span(pampa::pandoc::Span {
                    attr,
                    content: vec![pampa::pandoc::Inline::Str(pampa::pandoc::Str {
                        text: "🎉".to_string(),
                        source_info: quarto_source_map::SourceInfo::for_test(),
                    })],
                    source_info: quarto_source_map::SourceInfo::for_test(),
                    attr_source: pampa::pandoc::attr::AttrSourceInfo::empty(),
                }));
        }
    }

    let plan = compute_reconciliation(&baseline_ast, &new_ast);
    let (result_qmd, warnings) =
        writers::incremental::incremental_write(original_qmd, &baseline_ast, &new_ast, &plan)
            .expect("incremental_write Ok arm");

    assert!(
        !result_qmd.is_empty(),
        "sectionize-wrapper with inner Para edit yielded empty qmd \
         (warnings: {})",
        warnings.len()
    );

    // The user's appended reaction should land in the inner Para; the
    // wrapper itself should not re-emit any synthetic bytes.
    assert!(
        result_qmd.contains("[>> 🎉]"),
        "expected reaction span [>> 🎉] in result; got:\n{}",
        result_qmd
    );
    // Unchanged Header (the orig blocks[0] inside the wrapper) should
    // also be preserved.
    assert!(
        result_qmd.contains("# Heading"),
        "expected '# Heading' (unchanged sibling inside wrapper) in result; got:\n{:?}",
        result_qmd
    );
}

#[test]
fn sectionize_wrapper_preserves_frontmatter_after_inner_edit() {
    // Reproduce the second-order bug: when the post-pipeline AST wraps
    // the user content in a top-level sectionize Div, the writer's
    // `emit_metadata_prefix` reads `blocks[0].start_offset()` to decide
    // where the metadata region ends. The wrapper's start_offset is 0
    // (Generated, no preimage), so the function concludes "no metadata"
    // and deletes the YAML frontmatter from the output.
    let original_qmd = "\
---
format: q2-preview
render-components:
  - comment.tsx
---

# Heading

A paragraph that the user will edit.
";

    let baseline_ast = wrap_in_sectionize_div(parse_qmd(original_qmd));

    let mut new_ast = baseline_ast.clone();
    {
        let pampa::pandoc::Block::Div(ref mut div) = new_ast.blocks[0] else {
            panic!("expected wrapper Div at blocks[0]");
        };
        let para_idx = div
            .content
            .iter()
            .rposition(|b| matches!(b, pampa::pandoc::Block::Paragraph(_)))
            .expect("paragraph inside wrapper");
        if let pampa::pandoc::Block::Paragraph(ref mut p) = div.content[para_idx] {
            let attr = (
                String::new(),
                vec!["quarto-edit-comment".to_string()],
                hashlink::LinkedHashMap::new(),
            );
            p.content
                .push(pampa::pandoc::Inline::Span(pampa::pandoc::Span {
                    attr,
                    content: vec![pampa::pandoc::Inline::Str(pampa::pandoc::Str {
                        text: "🎉".to_string(),
                        source_info: quarto_source_map::SourceInfo::for_test(),
                    })],
                    source_info: quarto_source_map::SourceInfo::for_test(),
                    attr_source: pampa::pandoc::attr::AttrSourceInfo::empty(),
                }));
        }
    }

    let plan = compute_reconciliation(&baseline_ast, &new_ast);
    let (result_qmd, _warnings) =
        writers::incremental::incremental_write(original_qmd, &baseline_ast, &new_ast, &plan)
            .expect("incremental_write Ok arm");

    assert!(
        result_qmd
            .starts_with("---\nformat: q2-preview\nrender-components:\n  - comment.tsx\n---\n"),
        "frontmatter deleted from output. result:\n{}",
        result_qmd,
    );
    // And the edit still lands inside the wrapper's child.
    assert!(
        result_qmd.contains("[>> 🎉]"),
        "expected reaction span in result; got:\n{}",
        result_qmd
    );
}

#[test]
fn sectionize_wrapper_with_shortcode_child_edit_does_not_panic() {
    // Discovered 2026-05-25 during the TS-gate-bypass UX experiment.
    // When the framework's atomic-aware NOOP gate is disabled,
    // edits to shortcode-resolved content (e.g. inside
    // `{{< lipsum 3 >}}`) reach the writer. The writer's
    // RecurseIntoContainer arm for the top-level sectionize wrapper
    // descends via the Transparent recursion (commit bdcfdc53),
    // which calls coarsen_blocks on the wrapper's children with a
    // CHILD-RELATIVE plan. Inside that recursion, the existing
    // `coarsen_keep_before_block` catch-all (~line 484) emits
    // `Rewrite { new_idx: result_idx }` — but result_idx is the
    // child-relative index, not the top-level index. `emit_entries`
    // later does `new_ast.blocks[*new_idx]` (top-level) and panics
    // with "index out of bounds".
    //
    // The doc-comment on coarsen_keep_before_block explicitly notes
    // this is "not exercised by today's synthesizers" — true before
    // the Transparent recursion was added, no longer true now.
    //
    // This test pins the panic so the architectural fix (carry the
    // text on the Rewrite entry instead of an index, mirroring
    // InlineSplice's pattern) has a regression target.
    use pampa::pandoc::{Block, Header, Inline, Pandoc, Paragraph, Span, Str};
    use quarto_pandoc_types::{AttrSourceInfo, ConfigValue};
    use quarto_source_map::{AnchorRole, By, FileId, SourceInfo};
    use std::sync::Arc;

    const TARGET: FileId = FileId(0);
    // Original qmd byte ranges are illustrative; the source text is
    // long enough to contain all the byte ranges referenced below.
    let original_qmd = "# Heading\n\n{{< lipsum 3 >}}\n\nMore text.\n";

    // Build the lipsum shortcode token's anchor (Original in target).
    let token_si = SourceInfo::original(TARGET, 11, 27); // "{{< lipsum 3 >}}"

    // Construct a Generated{shortcode} Para representing one of
    // lipsum's resolved paragraphs.
    let mut lipsum_si = SourceInfo::generated(By::shortcode("lipsum"));
    lipsum_si.append_anchor(AnchorRole::Invocation, Arc::new(token_si.clone()));

    // Also construct a child Para that has NEITHER preimage in
    // target NOR a recognized Generated kind: an Original Para from
    // a DIFFERENT file. This is the cross-file-Original case that
    // coarsen_keep_before_block's catch-all falls through to.
    // (Pre-Plan-8 the AST didn't carry these; the panic the user
    // observed must hit a different shape — but the structural
    // failure is the same: a Rewrite emitted inside a Transparent
    // wrapper.)
    let other_file_para_si = SourceInfo::original(FileId(1), 0, 10);

    fn make_header(level: usize, text: &str, si: SourceInfo) -> Block {
        Block::Header(Header {
            level,
            attr: (String::new(), Vec::new(), hashlink::LinkedHashMap::new()),
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: SourceInfo::for_test(),
            })],
            source_info: si,
            attr_source: AttrSourceInfo::empty(),
        })
    }
    fn make_para(text: &str, si: SourceInfo) -> Block {
        Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: SourceInfo::for_test(),
            })],
            source_info: si,
        })
    }

    // Wrapper children: Header + cross-file Para + lipsum Para.
    let header = make_header(1, "Heading", SourceInfo::original(TARGET, 0, 9));
    let other_file_para = make_para("Cross", other_file_para_si);
    let lipsum_para = make_para("Lorem ipsum…", lipsum_si.clone());
    let original = wrap_in_sectionize_div(Pandoc {
        blocks: vec![header.clone(), other_file_para.clone(), lipsum_para],
        meta: ConfigValue::default(),
    });

    // User clicks +react on the lipsum Para — append a Span to its
    // inlines. The cross-file Para and Header are unchanged.
    let mut lipsum_para_new = make_para("Lorem ipsum…", lipsum_si);
    if let Block::Paragraph(ref mut p) = lipsum_para_new {
        p.content.push(Inline::Span(Span {
            attr: (
                String::new(),
                vec!["quarto-edit-comment".to_string()],
                hashlink::LinkedHashMap::new(),
            ),
            content: vec![Inline::Str(Str {
                text: "🎉".to_string(),
                source_info: SourceInfo::for_test(),
            })],
            source_info: SourceInfo::for_test(),
            attr_source: AttrSourceInfo::empty(),
        }));
    }
    let new = wrap_in_sectionize_div(Pandoc {
        blocks: vec![header, other_file_para, lipsum_para_new],
        meta: ConfigValue::default(),
    });

    let plan = compute_reconciliation(&original, &new);

    // Before the architectural fix: panics with
    // "index out of bounds: the len is 1 but the index is N".
    // After the fix: returns Ok. (This test does NOT assert on
    // output bytes — see `sectionize_wrapper_shortcode_child_edit_soft_drops`
    // for the byte-level expectation.)
    let result = writers::incremental::incremental_write(original_qmd, &original, &new, &plan);
    assert!(
        result.is_ok(),
        "incremental_write should not panic on a sectionize wrapper containing \
         a cross-file child + a shortcode child + an inline edit; got {:?}",
        result.err()
    );
}

#[test]
fn sectionize_wrapper_shortcode_child_edit_soft_drops() {
    // The user clicks +react on a paragraph inside `{{< lipsum 3 >}}`
    // with the framework's atomic-aware NOOP gate bypassed. The
    // shortcode resolution is atomic-kind Generated; the inline edit
    // has no source-side knob (the user's source is the token, not
    // the resolved bytes). The writer must:
    //
    //   (a) preserve the `{{< lipsum 3 >}}` token bytes in the qmd
    //   (b) NOT emit the resolved bytes / the reactji
    //   (c) surface a Q-3-42 or Q-3-43 warning so the UI can show
    //       a Monaco squiggle on the token line
    //
    // Two alignment shapes can reach the lipsum Para at child level
    // of a Transparent (sectionize) recursion:
    //
    //   1. `RecurseIntoContainer { lipsum_idx, lipsum_idx }` —
    //      reconciler matches the original and the new structurally.
    //      Hits the existing soft-drop cascade priority 1
    //      (preimage_in → Verbatim of token bytes). Works today.
    //
    //   2. `UseAfter(lipsum_idx)` (paired with a KeepBefore on the
    //      previous original) — reconciler can't pair the original
    //      and the new and treats it as a wholesale replacement.
    //      Falls through to let-user-win Rewrite (the writer emits
    //      the new block's resolved bytes verbatim). That's wrong
    //      for atomic-Generated with preimage.
    //
    // This test exercises shape #2 by giving the new Para a
    // SourceInfo::for_test() (simulating a React-side wholesale
    // replacement that loses provenance), then asserts the soft-drop
    // outcome. Pre-fix: the resolved bytes leak into the qmd. Post-
    // fix: the token is preserved + Q-3-42/43 fires.
    use pampa::pandoc::{Block, Header, Inline, Pandoc, Paragraph, Span, Str};
    use quarto_pandoc_types::{AttrSourceInfo, ConfigValue};
    use quarto_source_map::{AnchorRole, By, FileId, SourceInfo};
    use std::sync::Arc;

    const TARGET: FileId = FileId(0);
    let original_qmd = "# Heading\n\n{{< lipsum 3 >}}\n";

    let token_si = SourceInfo::original(TARGET, 11, 27);
    let mut lipsum_si = SourceInfo::generated(By::shortcode("lipsum"));
    lipsum_si.append_anchor(AnchorRole::Invocation, Arc::new(token_si));

    fn make_header(level: usize, text: &str, si: SourceInfo) -> Block {
        Block::Header(Header {
            level,
            attr: (String::new(), Vec::new(), hashlink::LinkedHashMap::new()),
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: SourceInfo::for_test(),
            })],
            source_info: si,
            attr_source: AttrSourceInfo::empty(),
        })
    }
    fn make_para_with_text(text: &str, si: SourceInfo) -> Block {
        Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: SourceInfo::for_test(),
            })],
            source_info: si,
        })
    }

    let header = make_header(1, "Heading", SourceInfo::original(TARGET, 0, 9));

    // Original lipsum paragraph carries the shortcode anchor.
    let lipsum_orig = make_para_with_text(
        "Lorem ipsum dolor sit amet, consectetur adipiscing elit.",
        lipsum_si.clone(),
    );
    let original = wrap_in_sectionize_div(Pandoc {
        blocks: vec![header.clone(), lipsum_orig],
        meta: ConfigValue::default(),
    });

    // New lipsum paragraph: different inline content + reactji Span,
    // but source_info IS preserved (matches what the React framework
    // does when constructing the post-edit AST — block source_info
    // is inherited from the original).
    let mut lipsum_new = make_para_with_text("Etiam maximus accumsan gravida.", lipsum_si.clone());
    if let Block::Paragraph(ref mut p) = lipsum_new {
        p.content.push(Inline::Span(Span {
            attr: (
                String::new(),
                vec!["quarto-edit-comment".to_string()],
                hashlink::LinkedHashMap::new(),
            ),
            content: vec![Inline::Str(Str {
                text: "🎉".to_string(),
                source_info: SourceInfo::for_test(),
            })],
            source_info: SourceInfo::for_test(),
            attr_source: AttrSourceInfo::empty(),
        }));
    }
    let new = wrap_in_sectionize_div(Pandoc {
        blocks: vec![header, lipsum_new],
        meta: ConfigValue::default(),
    });

    let plan = compute_reconciliation(&original, &new);
    eprintln!("plan = {:#?}", plan);

    let (qmd, warnings) =
        writers::incremental::incremental_write(original_qmd, &original, &new, &plan)
            .expect("write should succeed");
    eprintln!("--- qmd ---\n{}\n--- end ---", qmd);
    eprintln!("--- warnings ({}) ---", warnings.len());
    for w in &warnings {
        eprintln!("  code={:?} title={:?}", w.code, w.title);
    }

    // (a) token bytes preserved.
    assert!(
        qmd.contains("{{< lipsum 3 >}}"),
        "qmd should preserve the lipsum token bytes; got: {:?}",
        qmd
    );
    // (b) reactji NOT emitted.
    assert!(
        !qmd.contains("🎉"),
        "qmd should NOT contain the user's reactji; got: {:?}",
        qmd
    );
    // (b cont.) resolved bytes (the new Para's text) NOT emitted.
    assert!(
        !qmd.contains("Etiam maximus accumsan"),
        "qmd should NOT contain the new Para's resolved-shortcode bytes; \
         got: {:?}",
        qmd
    );
    // (c) Q-3-42 or Q-3-43 warning fired.
    let saw_soft_drop = warnings
        .iter()
        .any(|w| matches!(w.code.as_deref(), Some("Q-3-42") | Some("Q-3-43")));
    assert!(
        saw_soft_drop,
        "expected a Q-3-42 or Q-3-43 soft-drop warning; got: {:?}",
        warnings.iter().map(|w| &w.code).collect::<Vec<_>>()
    );
}

// --- target_file_id derivation skips no-root_file_id first blocks ---
//
// Plan 7c Phase 8 — `coarsen`'s `target_file_id` is derived from the
// first block whose `root_file_id()` resolves to `Some`. A synthesized
// title-block (or sectionize wrapper) at `blocks[0]` with no
// `Invocation` anchor returns `None`, so the writer needs to skip past
// it and look at later blocks. Pre-fix, the fallback to `FileId(0)`
// would make `preimage_in(target)` return `None` for every real block
// at `FileId(N != 0)` — i.e. all editability checks fail and edits
// silently soft-drop.

#[test]
fn target_file_id_skips_synthesized_first_block() {
    use pampa::pandoc::{Block, Header, Pandoc, Paragraph, Str};
    use quarto_pandoc_types::{AttrSourceInfo, ConfigValue};
    use quarto_source_map::{By, FileId, SourceInfo};

    // blocks[0] = synthesized title-block Header (Generated, no
    // Invocation). blocks[1] = real Paragraph at FileId(7).
    const REAL_FILE: FileId = FileId(7);
    let title_block = Block::Header(Header {
        level: 1,
        attr: (String::new(), Vec::new(), hashlink::LinkedHashMap::new()),
        content: vec![pampa::pandoc::Inline::Str(Str {
            text: "Synthesized title".to_string(),
            source_info: SourceInfo::for_test(),
        })],
        source_info: SourceInfo::generated(By::title_block()),
        attr_source: AttrSourceInfo::empty(),
    });
    // Real Para holds two Strs, both at FileId(7). The user edit
    // mutates the second Str so the reconciler emits a
    // RecurseIntoContainer with an inline plan. That path checks
    // `is_editable_inside_block` on the orig Para, which in turn
    // calls `preimage_in(target_file_id)` — and that's where a wrong
    // `target_file_id` (FileId(0) fallback) makes the editability
    // check return false and the writer soft-drops with Q-3-43.
    let original_qmd = "Real text";
    let real_para_orig = Block::Paragraph(Paragraph {
        content: vec![
            pampa::pandoc::Inline::Str(Str {
                text: "Real".to_string(),
                source_info: SourceInfo::original(REAL_FILE, 0, 4),
            }),
            pampa::pandoc::Inline::Space(pampa::pandoc::Space {
                source_info: SourceInfo::original(REAL_FILE, 4, 5),
            }),
            pampa::pandoc::Inline::Str(Str {
                text: "text".to_string(),
                source_info: SourceInfo::original(REAL_FILE, 5, 9),
            }),
        ],
        source_info: SourceInfo::original(REAL_FILE, 0, 9),
    });
    // Mutated Para: replace the second Str with a new (no-source) Str.
    let real_para_mut = Block::Paragraph(Paragraph {
        content: vec![
            pampa::pandoc::Inline::Str(Str {
                text: "Real".to_string(),
                source_info: SourceInfo::original(REAL_FILE, 0, 4),
            }),
            pampa::pandoc::Inline::Space(pampa::pandoc::Space {
                source_info: SourceInfo::original(REAL_FILE, 4, 5),
            }),
            pampa::pandoc::Inline::Str(Str {
                text: "edited".to_string(),
                source_info: SourceInfo::for_test(),
            }),
        ],
        source_info: SourceInfo::original(REAL_FILE, 0, 9),
    });
    let orig = Pandoc {
        blocks: vec![title_block.clone(), real_para_orig],
        meta: ConfigValue::default(),
    };
    let new = Pandoc {
        blocks: vec![title_block, real_para_mut],
        meta: ConfigValue::default(),
    };

    let plan = compute_reconciliation(&orig, &new);
    let (_qmd, warnings) =
        writers::incremental::incremental_write(original_qmd, &orig, &new, &plan)
            .expect("incremental_write Ok arm");

    // Pre-fix target_file_id falls back to FileId(0); preimage_in(0)
    // on REAL_FILE-Original Para returns None; coarsen's
    // RecurseIntoContainer arm soft-drops with Q-3-43 ("Generated
    // content edit dropped"). Post-fix target_file_id resolves to
    // REAL_FILE and the inline edit proceeds without a warning.
    assert!(
        warnings.is_empty(),
        "expected no soft-drop warnings; got: {:?}",
        warnings.iter().map(|w| &w.title).collect::<Vec<_>>()
    );
}

#[test]
fn target_file_id_defaults_to_zero_for_empty_document() {
    // Empty `blocks` — the fallback to `FileId(0)` should fire.
    // Driving an identity reconcile on an empty AST should produce a
    // no-op write without warnings or panics.
    use pampa::pandoc::Pandoc;
    use quarto_pandoc_types::ConfigValue;
    let ast = Pandoc {
        blocks: vec![],
        meta: ConfigValue::default(),
    };
    let plan = compute_reconciliation(&ast, &ast);
    let (result, warnings) = writers::incremental::incremental_write("", &ast, &ast, &plan)
        .expect("incremental_write Ok arm on empty document");
    assert_eq!(result, "");
    assert!(warnings.is_empty());
}

// --- Mixed documents ---

#[test]
fn idempotent_mixed_document() {
    assert_idempotent(
        "## Title\n\nFirst paragraph.\n\n```python\ncode()\n```\n\nSecond paragraph.\n",
    );
}

#[test]
fn idempotent_complex_document() {
    assert_idempotent(
        "\
## Introduction

This is the first paragraph.

> A block quote with
> multiple lines.

* Item one
* Item two
* Item three

### Code Example

```python
print('hello')
```

Final paragraph.
",
    );
}

// =============================================================================
// Property 2 edge cases
// =============================================================================

#[test]
fn idempotent_empty_document() {
    // Empty document — no blocks
    assert_idempotent("");
}

#[test]
fn idempotent_single_header_no_trailing_newline() {
    // Some documents might not have trailing newlines after the last block
    // but the parser requires them, so test with trailing newline
    assert_idempotent("# Title\n");
}

// =============================================================================
// Property 1: Round-trip correctness — basic tests with hand-crafted mutations
// =============================================================================

/// Test that changing a block produces a result that round-trips correctly.
/// read(incremental_write(qmd, orig, new, plan)) ≡ new  (structural equality)
fn assert_roundtrip(original_qmd: &str, new_qmd: &str) {
    let original_ast = parse_qmd(original_qmd);
    let new_ast = parse_qmd(new_qmd);

    let plan = compute_reconciliation(&original_ast, &new_ast);
    let result =
        writers::incremental::incremental_write(original_qmd, &original_ast, &new_ast, &plan)
            .expect("incremental_write failed")
            .0;

    // Verify the result round-trips: read(result) should match new_ast structurally
    let result_ast = parse_qmd(&result);

    // Compare block count
    assert_eq!(
        result_ast.blocks.len(),
        new_ast.blocks.len(),
        "Block count mismatch:\n  result has {} blocks\n  expected {} blocks\n  result text: {:?}",
        result_ast.blocks.len(),
        new_ast.blocks.len(),
        result
    );

    // Compare blocks using structural equality (ignoring source info)
    for (i, (result_block, new_block)) in result_ast
        .blocks
        .iter()
        .zip(new_ast.blocks.iter())
        .enumerate()
    {
        assert!(
            quarto_ast_reconcile::structural_eq_block(result_block, new_block),
            "Block {} structurally different:\n  result: {:?}\n  expected: {:?}",
            i,
            result_block,
            new_block
        );
    }
}

// --- Change text in a paragraph ---

#[test]
fn roundtrip_change_paragraph_text() {
    assert_roundtrip(
        "First paragraph.\n\nSecond paragraph.\n\nThird paragraph.\n",
        "First paragraph.\n\nModified second.\n\nThird paragraph.\n",
    );
}

#[test]
fn roundtrip_change_first_paragraph() {
    assert_roundtrip(
        "First paragraph.\n\nSecond paragraph.\n",
        "Changed first.\n\nSecond paragraph.\n",
    );
}

#[test]
fn roundtrip_change_last_paragraph() {
    assert_roundtrip(
        "First paragraph.\n\nSecond paragraph.\n",
        "First paragraph.\n\nChanged second.\n",
    );
}

// --- Add a block ---

#[test]
fn roundtrip_add_paragraph_at_end() {
    assert_roundtrip(
        "First paragraph.\n\nSecond paragraph.\n",
        "First paragraph.\n\nSecond paragraph.\n\nThird paragraph.\n",
    );
}

#[test]
fn roundtrip_add_paragraph_at_start() {
    assert_roundtrip(
        "First paragraph.\n\nSecond paragraph.\n",
        "New first.\n\nFirst paragraph.\n\nSecond paragraph.\n",
    );
}

// --- Remove a block ---

#[test]
fn roundtrip_remove_middle_paragraph() {
    assert_roundtrip("First.\n\nSecond.\n\nThird.\n", "First.\n\nThird.\n");
}

#[test]
fn roundtrip_remove_first_paragraph() {
    assert_roundtrip("First.\n\nSecond.\n\nThird.\n", "Second.\n\nThird.\n");
}

// --- Change header ---

#[test]
fn roundtrip_change_header_text() {
    assert_roundtrip("## Title\n\nParagraph.\n", "## New Title\n\nParagraph.\n");
}

#[test]
fn roundtrip_add_header_attribute() {
    assert_roundtrip(
        "## Title {.feature created=\"2026-02-10\"}\n\nParagraph.\n",
        "## Title {.feature created=\"2026-02-10\" status=\"todo\"}\n\nParagraph.\n",
    );
}

#[test]
fn roundtrip_change_header_attribute() {
    assert_roundtrip(
        "## Title {.feature status=\"todo\"}\n\nParagraph.\n",
        "## Title {.feature status=\"done\"}\n\nParagraph.\n",
    );
}

#[test]
fn roundtrip_add_header_attribute_with_frontmatter() {
    assert_roundtrip(
        "---\ntitle: my board\n---\n\n## Title {.feature created=\"2026-02-10\"}\n\nParagraph.\n",
        "---\ntitle: my board\n---\n\n## Title {.feature created=\"2026-02-10\" status=\"todo\"}\n\nParagraph.\n",
    );
}

#[test]
fn roundtrip_add_header_attribute_via_json_roundtrip() {
    // Simulates the WASM path: parse original, parse new, JSON-serialize new AST,
    // JSON-deserialize, then incremental write
    let original_qmd = "## Title {.feature created=\"2026-02-10\"}\n\nParagraph.\n";
    let new_qmd = "## Title {.feature created=\"2026-02-10\" status=\"todo\"}\n\nParagraph.\n";
    let new_ast = parse_qmd(new_qmd);
    let result = incremental_write_via_json_roundtrip(original_qmd, &new_ast);
    let result_ast = parse_qmd(&result);
    assert_eq!(result_ast.blocks.len(), new_ast.blocks.len());
    for (result_block, new_block) in result_ast.blocks.iter().zip(new_ast.blocks.iter()) {
        assert!(
            quarto_ast_reconcile::structural_eq_block(result_block, new_block),
            "Block structural mismatch after JSON roundtrip"
        );
    }
}

#[test]
fn roundtrip_kanban_status_change() {
    // Reproduces the kanban demo scenario: multiple cards, change status on one
    let original_qmd = concat!(
        "---\ntitle: test kanban\n---\n\n",
        "# Cards\n\n",
        "## Work Week {.milestone deadline=\"2026-03-25\" created=\"2026-02-10\"}\n\n",
        "Items:\n\n",
        "- [ ] [Project Export](#project-export)\n\n",
        "## Project Export {.feature created=\"2026-02-10\"}\n\n",
        "## ACLs {.feature created=\"2026-02-10\"}\n\n",
        "Some body text.\n",
    );
    let new_qmd = concat!(
        "---\ntitle: test kanban\n---\n\n",
        "# Cards\n\n",
        "## Work Week {.milestone deadline=\"2026-03-25\" created=\"2026-02-10\"}\n\n",
        "Items:\n\n",
        "- [ ] [Project Export](#project-export)\n\n",
        "## Project Export {.feature created=\"2026-02-10\" status=\"todo\"}\n\n",
        "## ACLs {.feature created=\"2026-02-10\"}\n\n",
        "Some body text.\n",
    );
    assert_roundtrip(original_qmd, new_qmd);
}

#[test]
fn roundtrip_kanban_status_change_via_json() {
    // Same as above but going through JSON roundtrip path (like WASM)
    let original_qmd = concat!(
        "---\ntitle: test kanban\n---\n\n",
        "# Cards\n\n",
        "## Work Week {.milestone deadline=\"2026-03-25\" created=\"2026-02-10\"}\n\n",
        "Items:\n\n",
        "- [ ] [Project Export](#project-export)\n\n",
        "## Project Export {.feature created=\"2026-02-10\"}\n\n",
        "## ACLs {.feature created=\"2026-02-10\"}\n\n",
        "Some body text.\n",
    );
    let new_qmd = concat!(
        "---\ntitle: test kanban\n---\n\n",
        "# Cards\n\n",
        "## Work Week {.milestone deadline=\"2026-03-25\" created=\"2026-02-10\"}\n\n",
        "Items:\n\n",
        "- [ ] [Project Export](#project-export)\n\n",
        "## Project Export {.feature created=\"2026-02-10\" status=\"todo\"}\n\n",
        "## ACLs {.feature created=\"2026-02-10\"}\n\n",
        "Some body text.\n",
    );
    let new_ast = parse_qmd(new_qmd);
    let result = incremental_write_via_json_roundtrip(original_qmd, &new_ast);
    let result_ast = parse_qmd(&result);
    assert_eq!(result_ast.blocks.len(), new_ast.blocks.len());
}

/// Changing an explicit ID (`{#custom-id}` → `{#new-id}`) should produce correct output.
/// The explicit ID is in the source suffix, so InlineSplice would preserve the old ID.
/// The fix should detect that the explicit ID changed and fall back to Rewrite.
#[test]
fn roundtrip_change_explicit_header_id() {
    assert_roundtrip(
        "## Title {#custom-id .feature}\n\nParagraph.\n",
        "## Title {#new-id .feature}\n\nParagraph.\n",
    );
}

/// When only the auto-generated ID changes (because header text changed),
/// InlineSplice should still be used — no explicit `{#id}` should appear in output.
/// This is a regression guard: the ID comparison must NOT trigger Rewrite for auto-generated IDs.
#[test]
fn roundtrip_auto_id_change_no_explicit_id_in_output() {
    let original_qmd = "## Hello World\n\nParagraph.\n";
    let new_qmd = "## Goodbye World\n\nParagraph.\n";

    let original_ast = parse_qmd(original_qmd);
    let new_ast = parse_qmd(new_qmd);
    let plan = compute_reconciliation(&original_ast, &new_ast);
    let result =
        writers::incremental::incremental_write(original_qmd, &original_ast, &new_ast, &plan)
            .expect("incremental_write failed")
            .0;

    // Should NOT contain an explicit ID attribute — auto-generated IDs stay implicit
    assert!(
        !result.contains("{#"),
        "Auto-generated ID should not appear as explicit attribute in output.\nGot: {:?}",
        result
    );
    assert_eq!(result, "## Goodbye World\n\nParagraph.\n");
}

// --- Verbatim preservation tests ---

/// Verify that unchanged blocks preserve their EXACT text (byte-for-byte).
#[test]
fn verbatim_preservation_unchanged_blocks() {
    let original_qmd = "First paragraph.\n\nSecond paragraph.\n\nThird paragraph.\n";
    let new_qmd = "First paragraph.\n\nModified second.\n\nThird paragraph.\n";

    let original_ast = parse_qmd(original_qmd);
    let new_ast = parse_qmd(new_qmd);

    let plan = compute_reconciliation(&original_ast, &new_ast);
    let result =
        writers::incremental::incremental_write(original_qmd, &original_ast, &new_ast, &plan)
            .expect("incremental_write failed")
            .0;

    // The first and third paragraphs should be byte-for-byte identical
    assert!(
        result.starts_with("First paragraph.\n"),
        "First paragraph should be preserved verbatim. Result starts with: {:?}",
        &result[..result.len().min(30)]
    );
    assert!(
        result.ends_with("Third paragraph.\n"),
        "Third paragraph should be preserved verbatim. Result ends with: {:?}",
        &result[result.len().saturating_sub(30)..]
    );
}

// =============================================================================
// QMD String Generators (for property-based tests)
// =============================================================================
//
// These generators produce QMD strings at increasing levels of complexity.
// Each generated string is a valid QMD document that can be parsed by the reader.
//
// The approach is to generate QMD text directly rather than ASTs, because:
// 1. The reader gives us accurate source spans (needed by the incremental writer)
// 2. We don't need the test-only generators from quarto-ast-reconcile
// 3. QMD strings are the natural input domain for the incremental writer
//
// Text uses only lowercase letters and spaces to avoid accidentally producing
// markdown syntax (e.g., `*` for lists, `#` for headers, `>` for block quotes).

/// Generate a single word: 2-8 lowercase letters.
fn gen_word() -> BoxedStrategy<String> {
    "[a-z]{2,8}".boxed()
}

/// Generate paragraph text: 2-6 words separated by spaces.
fn gen_paragraph_text() -> BoxedStrategy<String> {
    prop::collection::vec(gen_word(), 2..7)
        .prop_map(|words| words.join(" "))
        .boxed()
}

/// Generate a paragraph block (text ending with `\n`).
fn gen_paragraph_block() -> BoxedStrategy<String> {
    gen_paragraph_text()
        .prop_map(|text| format!("{}\n", text))
        .boxed()
}

/// Generate a header block (`## Title\n`).
fn gen_header_block() -> BoxedStrategy<String> {
    (1..4usize, gen_paragraph_text())
        .prop_map(|(level, text)| format!("{} {}\n", "#".repeat(level), text))
        .boxed()
}

/// Generate a fenced code block.
fn gen_code_block() -> BoxedStrategy<String> {
    ("[a-z]{3,6}", "[a-z0-9 ]{3,20}")
        .prop_map(|(lang, code)| format!("```{}\n{}\n```\n", lang, code))
        .boxed()
}

/// Generate a horizontal rule.
fn gen_hr_block() -> BoxedStrategy<String> {
    Just("***\n".to_string()).boxed()
}

/// Generate a single-line block quote.
fn gen_blockquote_block() -> BoxedStrategy<String> {
    gen_paragraph_text()
        .prop_map(|text| format!("> {}\n", text))
        .boxed()
}

/// Generate a bullet list with 2-4 items.
fn gen_bullet_list_block() -> BoxedStrategy<String> {
    prop::collection::vec(gen_paragraph_text(), 2..5)
        .prop_map(|items| {
            items
                .iter()
                .map(|item| format!("* {}\n", item))
                .collect::<String>()
        })
        .boxed()
}

/// Generate an ordered list with 2-4 items.
fn gen_ordered_list_block() -> BoxedStrategy<String> {
    prop::collection::vec(gen_paragraph_text(), 2..5)
        .prop_map(|items| {
            items
                .iter()
                .enumerate()
                .map(|(i, item)| format!("{}. {}\n", i + 1, item))
                .collect::<String>()
        })
        .boxed()
}

/// Level 0: Paragraphs only. The simplest documents.
fn gen_qmd_level0() -> BoxedStrategy<String> {
    prop::collection::vec(gen_paragraph_block(), 1..6)
        .prop_map(|blocks| blocks.join("\n"))
        .boxed()
}

/// Level 1: Leaf blocks — paragraphs, headers, code blocks, horizontal rules.
fn gen_qmd_level1() -> BoxedStrategy<String> {
    prop::collection::vec(
        prop_oneof![
            4 => gen_paragraph_block(),
            2 => gen_header_block(),
            2 => gen_code_block(),
            1 => gen_hr_block(),
        ],
        1..6,
    )
    .prop_map(|blocks| blocks.join("\n"))
    .boxed()
}

/// Level 2: Container blocks — adds block quotes, bullet lists, ordered lists.
fn gen_qmd_level2() -> BoxedStrategy<String> {
    prop::collection::vec(
        prop_oneof![
            4 => gen_paragraph_block(),
            2 => gen_header_block(),
            2 => gen_code_block(),
            1 => gen_hr_block(),
            2 => gen_blockquote_block(),
            2 => gen_bullet_list_block(),
            1 => gen_ordered_list_block(),
        ],
        1..6,
    )
    .prop_map(|blocks| blocks.join("\n"))
    .boxed()
}

/// Level 3: Adds YAML front matter.
fn gen_qmd_level3() -> BoxedStrategy<String> {
    (
        prop::bool::ANY,
        prop::collection::vec(
            prop_oneof![
                4 => gen_paragraph_block(),
                2 => gen_header_block(),
                2 => gen_code_block(),
                1 => gen_hr_block(),
                2 => gen_blockquote_block(),
                2 => gen_bullet_list_block(),
                1 => gen_ordered_list_block(),
            ],
            1..6,
        ),
    )
        .prop_map(|(has_front_matter, blocks)| {
            let body = blocks.join("\n");
            if has_front_matter {
                format!("---\ntitle: Test\n---\n\n{}", body)
            } else {
                body
            }
        })
        .boxed()
}

// =============================================================================
// QMD Mutation Generators (for Property 1 round-trip tests)
// =============================================================================
//
// These generate (original_qmd, new_qmd) pairs where the documents share some
// blocks but differ in others, testing the incremental writer's ability to
// preserve unchanged blocks while correctly rewriting changed ones.

/// Generate a (original, new) pair by mutating a single block in a document.
///
/// Strategy: generate a document of 2-5 blocks, pick one block to replace with
/// a freshly generated block. The other blocks remain identical.
fn gen_qmd_pair_single_mutation() -> BoxedStrategy<(String, String)> {
    // Generate the block list, then pick an index to mutate
    prop::collection::vec(gen_paragraph_block(), 2..6)
        .prop_flat_map(|blocks| {
            let n = blocks.len();
            (Just(blocks), 0..n, gen_paragraph_block())
        })
        .prop_map(|(blocks, idx, new_block)| {
            let original = blocks.join("\n");
            let mut new_blocks = blocks;
            new_blocks[idx] = new_block;
            let new = new_blocks.join("\n");
            (original, new)
        })
        .boxed()
}

/// Generate a (original, new) pair by adding a block.
fn gen_qmd_pair_add_block() -> BoxedStrategy<(String, String)> {
    prop::collection::vec(gen_paragraph_block(), 2..5)
        .prop_flat_map(|blocks| {
            let n = blocks.len();
            (Just(blocks), 0..=n, gen_paragraph_block())
        })
        .prop_map(|(blocks, insert_pos, new_block)| {
            let original = blocks.join("\n");
            let mut new_blocks = blocks;
            new_blocks.insert(insert_pos, new_block);
            let new = new_blocks.join("\n");
            (original, new)
        })
        .boxed()
}

/// Generate a (original, new) pair by removing a block.
fn gen_qmd_pair_remove_block() -> BoxedStrategy<(String, String)> {
    prop::collection::vec(gen_paragraph_block(), 3..6)
        .prop_flat_map(|blocks| {
            let n = blocks.len();
            (Just(blocks), 0..n)
        })
        .prop_map(|(blocks, remove_idx)| {
            let original = blocks.join("\n");
            let mut new_blocks = blocks;
            new_blocks.remove(remove_idx);
            let new = new_blocks.join("\n");
            (original, new)
        })
        .boxed()
}

/// Generate a (original, new) pair using mixed block types and any mutation.
fn gen_qmd_pair_mixed() -> BoxedStrategy<(String, String)> {
    prop_oneof![
        // Mutate a single block in a level 1 document
        prop::collection::vec(
            prop_oneof![
                4 => gen_paragraph_block(),
                2 => gen_header_block(),
                2 => gen_code_block(),
                1 => gen_hr_block(),
            ],
            2..6,
        )
        .prop_flat_map(|blocks| {
            let n = blocks.len();
            (Just(blocks), 0..n, gen_paragraph_block())
        })
        .prop_map(|(blocks, idx, new_block)| {
            let original = blocks.join("\n");
            let mut new_blocks = blocks;
            new_blocks[idx] = new_block;
            let new_doc = new_blocks.join("\n");
            (original, new_doc)
        }),
        // Add a block to a level 2 document
        prop::collection::vec(
            prop_oneof![
                3 => gen_paragraph_block(),
                1 => gen_header_block(),
                1 => gen_blockquote_block(),
                1 => gen_bullet_list_block(),
            ],
            2..5,
        )
        .prop_flat_map(|blocks| {
            let n = blocks.len();
            (Just(blocks), 0..=n, gen_paragraph_block())
        })
        .prop_map(|(blocks, insert_pos, new_block)| {
            let original = blocks.join("\n");
            let mut new_blocks = blocks;
            new_blocks.insert(insert_pos, new_block);
            let new_doc = new_blocks.join("\n");
            (original, new_doc)
        }),
        // Remove a block from a level 2 document
        prop::collection::vec(
            prop_oneof![
                3 => gen_paragraph_block(),
                1 => gen_header_block(),
                1 => gen_blockquote_block(),
                1 => gen_bullet_list_block(),
            ],
            3..6,
        )
        .prop_flat_map(|blocks| {
            let n = blocks.len();
            (Just(blocks), 0..n)
        })
        .prop_map(|(blocks, remove_idx)| {
            let original = blocks.join("\n");
            let mut new_blocks = blocks;
            new_blocks.remove(remove_idx);
            let new_doc = new_blocks.join("\n");
            (original, new_doc)
        }),
    ]
    .boxed()
}

// =============================================================================
// Property 2: Idempotence — proptest
// =============================================================================

proptest! {
    #[test]
    fn proptest_idempotent_level0(qmd in gen_qmd_level0()) {
        assert_idempotent(&qmd);
    }

    #[test]
    fn proptest_idempotent_level1(qmd in gen_qmd_level1()) {
        assert_idempotent(&qmd);
    }

    #[test]
    fn proptest_idempotent_level2(qmd in gen_qmd_level2()) {
        assert_idempotent(&qmd);
    }

    #[test]
    fn proptest_idempotent_level3(qmd in gen_qmd_level3()) {
        assert_idempotent(&qmd);
    }
}

// =============================================================================
// Property 1: Round-trip correctness — proptest
// =============================================================================

proptest! {
    #[test]
    fn proptest_roundtrip_single_mutation(
        (original, new) in gen_qmd_pair_single_mutation()
    ) {
        assert_roundtrip(&original, &new);
    }

    #[test]
    fn proptest_roundtrip_add_block(
        (original, new) in gen_qmd_pair_add_block()
    ) {
        assert_roundtrip(&original, &new);
    }

    #[test]
    fn proptest_roundtrip_remove_block(
        (original, new) in gen_qmd_pair_remove_block()
    ) {
        assert_roundtrip(&original, &new);
    }

    #[test]
    fn proptest_roundtrip_mixed(
        (original, new) in gen_qmd_pair_mixed()
    ) {
        assert_roundtrip(&original, &new);
    }
}

// =============================================================================
// Property 3: Equivalence with full writer — proptest
// =============================================================================
//
// read(incremental_write(qmd, orig, new, plan)) ≡ read(write(new_ast))
//
// The incremental writer and full writer should produce semantically equivalent
// documents, even though the byte-level representations may differ.

/// Assert Property 3: incremental result is semantically equivalent to full writer result.
fn assert_equivalent_to_full_writer(original_qmd: &str, new_qmd: &str) {
    let original_ast = parse_qmd(original_qmd);
    let new_ast = parse_qmd(new_qmd);

    let plan = compute_reconciliation(&original_ast, &new_ast);
    let incremental_result =
        writers::incremental::incremental_write(original_qmd, &original_ast, &new_ast, &plan)
            .expect("incremental_write failed")
            .0;

    let full_result = write_qmd(&new_ast);

    // Parse both results and compare structurally
    let incremental_ast = parse_qmd(&incremental_result);
    let full_ast = parse_qmd(&full_result);

    assert_eq!(
        incremental_ast.blocks.len(),
        full_ast.blocks.len(),
        "Block count mismatch between incremental and full writer:\n  incremental: {} blocks\n  full: {} blocks\n  incremental text: {:?}\n  full text: {:?}",
        incremental_ast.blocks.len(),
        full_ast.blocks.len(),
        incremental_result,
        full_result
    );

    for (i, (inc_block, full_block)) in incremental_ast
        .blocks
        .iter()
        .zip(full_ast.blocks.iter())
        .enumerate()
    {
        assert!(
            quarto_ast_reconcile::structural_eq_block(inc_block, full_block),
            "Block {} structurally different between incremental and full writer:\n  incremental: {:?}\n  full: {:?}",
            i,
            inc_block,
            full_block
        );
    }
}

proptest! {
    #[test]
    fn proptest_equivalent_to_full_writer(
        (original, new) in gen_qmd_pair_mixed()
    ) {
        assert_equivalent_to_full_writer(&original, &new);
    }
}

// =============================================================================
// Property 4: Verbatim preservation of unchanged blocks — proptest
// =============================================================================
//
// When a single block changes, all other blocks should be preserved
// byte-for-byte in the incremental writer's output.
//
// Note: The stronger form of Property 4 (locality of edit ranges from
// compute_incremental_edits) requires fine-grained edit computation,
// which is a future optimization. This tests the weaker but important
// invariant that unchanged blocks are verbatim-preserved.

/// Assert that unchanged blocks in the result are byte-for-byte identical
/// to their original text.
fn assert_verbatim_preservation(blocks: &[String], mutate_idx: usize, new_block: &str) {
    let original = blocks.join("\n");
    let mut new_blocks: Vec<String> = blocks.to_vec();
    new_blocks[mutate_idx] = new_block.to_string();
    let new = new_blocks.join("\n");

    let original_ast = parse_qmd(&original);
    let new_ast = parse_qmd(&new);

    let plan = compute_reconciliation(&original_ast, &new_ast);
    let result = writers::incremental::incremental_write(&original, &original_ast, &new_ast, &plan)
        .expect("incremental_write failed")
        .0;

    // For each unchanged block, verify its text appears verbatim in the result.
    // We check by finding the original block text in the result string.
    for (i, block_text) in blocks.iter().enumerate() {
        if i == mutate_idx {
            continue; // This block was mutated — skip
        }
        // The block text (without trailing separator) should appear in the result
        let block_content = block_text.trim_end_matches('\n');
        assert!(
            result.contains(block_content),
            "Block {} should be preserved verbatim.\n  Expected to find: {:?}\n  In result: {:?}",
            i,
            block_content,
            result
        );
    }
}

proptest! {
    #[test]
    fn proptest_verbatim_preservation(
        blocks in prop::collection::vec(gen_paragraph_block(), 3..6),
        new_block in gen_paragraph_block(),
    ) {
        let n = blocks.len();
        // Pick a random block to mutate (use first byte of new_block as seed)
        let mutate_idx = new_block.as_bytes()[0] as usize % n;
        assert_verbatim_preservation(&blocks, mutate_idx, &new_block);
    }
}

// =============================================================================
// Property 5: Monotonicity of edit spans — proptest
// =============================================================================
//
// compute_incremental_edits produces edits that are:
// - Sorted by range.start
// - Non-overlapping (each edit's range.end <= next edit's range.start)

/// Assert Property 5: edits are sorted and non-overlapping.
fn assert_edits_monotonic(original_qmd: &str, new_qmd: &str) {
    let original_ast = parse_qmd(original_qmd);
    let new_ast = parse_qmd(new_qmd);

    let plan = compute_reconciliation(&original_ast, &new_ast);
    let (edits, _warnings) = writers::incremental::compute_incremental_edits(
        original_qmd,
        &original_ast,
        &new_ast,
        &plan,
    )
    .expect("compute_incremental_edits failed");

    // Verify sorted by range.start
    for window in edits.windows(2) {
        assert!(
            window[0].range.start <= window[1].range.start,
            "Edits not sorted by start: {:?} before {:?}",
            window[0],
            window[1]
        );
    }

    // Verify non-overlapping
    for window in edits.windows(2) {
        assert!(
            window[0].range.end <= window[1].range.start,
            "Edits overlap: {:?} and {:?}",
            window[0],
            window[1]
        );
    }

    // Verify all edit ranges are within bounds
    for edit in &edits {
        assert!(
            edit.range.end <= original_qmd.len(),
            "Edit range {:?} exceeds document length {}",
            edit.range,
            original_qmd.len()
        );
    }
}

proptest! {
    #[test]
    fn proptest_edits_monotonic(
        (original, new) in gen_qmd_pair_mixed()
    ) {
        assert_edits_monotonic(&original, &new);
    }

    #[test]
    fn proptest_edits_monotonic_identity(qmd in gen_qmd_level2()) {
        // Identity case: should produce zero edits
        let ast = parse_qmd(&qmd);
        let plan = compute_reconciliation(&ast, &ast);
        let (edits, _warnings) =
            writers::incremental::compute_incremental_edits(&qmd, &ast, &ast, &plan)
                .expect("compute_incremental_edits failed");
        prop_assert!(
            edits.is_empty(),
            "Identity reconciliation should produce zero edits, got {} edits",
            edits.len()
        );
    }
}

// =============================================================================
// Sugar/Desugar Roundtrip Tests
// =============================================================================
//
// These verify that desugar(sugar(node)) ≡ node for Table and DefinitionList.
// The incremental writer relies on this property: when a Table or DefinitionList
// block is marked as `KeepBefore`, its verbatim source text must parse back to
// an equivalent AST node. When it's marked as `Rewrite`, the writer's sugaring
// must produce output that deserializes identically.

/// Assert that a QMD document round-trips through write→parse with structural equality.
fn assert_sugar_roundtrip(input: &str) {
    let ast1 = parse_qmd(input);
    let written = write_qmd(&ast1);
    let ast2 = parse_qmd(&written);

    assert_eq!(
        ast1.blocks.len(),
        ast2.blocks.len(),
        "Block count changed after write→parse roundtrip:\n  original: {} blocks\n  after roundtrip: {} blocks\n  written text: {:?}",
        ast1.blocks.len(),
        ast2.blocks.len(),
        written
    );

    for (i, (b1, b2)) in ast1.blocks.iter().zip(ast2.blocks.iter()).enumerate() {
        assert!(
            quarto_ast_reconcile::structural_eq_block(b1, b2),
            "Block {} structurally different after write→parse roundtrip:\n  before: {:?}\n  after: {:?}\n  written text: {:?}",
            i,
            b1,
            b2,
            written
        );
    }
}

// --- List-table sugar/desugar roundtrips ---

#[test]
fn sugar_roundtrip_list_table_basic() {
    assert_sugar_roundtrip(
        "::: {.list-table}\n\n* - Cell 1,1\n  - Cell 1,2\n* - Cell 2,1\n  - Cell 2,2\n\n:::\n",
    );
}

#[test]
fn sugar_roundtrip_list_table_with_header() {
    assert_sugar_roundtrip(
        "::: {.list-table header-rows=\"1\"}\n\n* - Header 1\n  - Header 2\n* - Cell 1,1\n  - Cell 1,2\n\n:::\n",
    );
}

// --- Definition-list sugar/desugar roundtrips ---

// NOTE: Definition-list sugar/desugar roundtrip is LOSSY. The writer produces
// Pandoc-native definition list syntax ("term\n:   definition\n") but the reader
// only recognizes the `::: {.definition-list}` div syntax. This means:
//   - KeepBefore (verbatim copy): works correctly (preserves original div syntax)
//   - Rewrite: broken — writer output doesn't parse back to DefinitionList
// This is a pre-existing writer bug, not specific to the incremental writer.
// These tests are ignored until the writer is fixed to produce div syntax for
// definition lists (or the reader is extended to parse Pandoc-native syntax).

#[test]
#[ignore = "definition-list sugar roundtrip is lossy: writer produces Pandoc-native syntax, reader expects div syntax"]
fn sugar_roundtrip_definition_list_basic() {
    assert_sugar_roundtrip(
        "::: {.definition-list}\n* term one\n  - definition one\n* term two\n  - definition two\n\n:::\n",
    );
}

#[test]
#[ignore = "definition-list sugar roundtrip is lossy: writer produces Pandoc-native syntax, reader expects div syntax"]
fn sugar_roundtrip_definition_list_multiple_defs() {
    assert_sugar_roundtrip(
        "::: {.definition-list}\n* term\n  - definition a\n  - definition b\n\n:::\n",
    );
}

// --- Idempotence of incremental writer with sugared constructs ---

#[test]
fn idempotent_list_table() {
    assert_idempotent(
        "::: {.list-table}\n\n* - Cell 1,1\n  - Cell 1,2\n* - Cell 2,1\n  - Cell 2,2\n\n:::\n",
    );
}

#[test]
fn idempotent_definition_list() {
    assert_idempotent(
        "::: {.definition-list}\n* term one\n  - definition one\n* term two\n  - definition two\n\n:::\n",
    );
}

#[test]
fn idempotent_mixed_with_table() {
    assert_idempotent(
        "## Title\n\nBefore the table.\n\n::: {.list-table}\n\n* - A\n  - B\n* - C\n  - D\n\n:::\n\nAfter the table.\n",
    );
}

// --- Roundtrip of incremental writer with sugared constructs ---

#[test]
fn roundtrip_change_paragraph_near_table() {
    assert_roundtrip(
        "Before.\n\n::: {.list-table}\n\n* - A\n  - B\n* - C\n  - D\n\n:::\n\nAfter.\n",
        "Changed before.\n\n::: {.list-table}\n\n* - A\n  - B\n* - C\n  - D\n\n:::\n\nAfter.\n",
    );
}

#[test]
fn roundtrip_change_paragraph_near_deflist() {
    assert_roundtrip(
        "Before.\n\n::: {.definition-list}\n* term\n  - def\n\n:::\n\nAfter.\n",
        "Before.\n\n::: {.definition-list}\n* term\n  - def\n\n:::\n\nChanged after.\n",
    );
}

// =============================================================================
// HTML comment preservation — bd-1066
// =============================================================================
//
// HTML comments (<!-- ... -->) are now preserved in the Pandoc AST as
// RawInline(html, "<!-- ... -->"). The QMD writer emits them in native
// syntax. This ensures comments survive both standard writes and
// incremental writes.

/// Idempotence with standalone block-level comment.
#[test]
fn idempotent_with_standalone_comment() {
    assert_idempotent("Before.\n\n<!-- a comment -->\n\nAfter.\n");
}

/// Idempotence with inline comment inside a paragraph.
#[test]
fn idempotent_with_inline_comment() {
    assert_idempotent("Hello <!-- comment --> world.\n");
}

/// Idempotence with multiple inline comments.
#[test]
fn idempotent_with_multiple_inline_comments() {
    assert_idempotent("Text <!-- one --> and <!-- two --> done.\n");
}

/// Idempotence with comment inside a blockquote.
#[test]
fn idempotent_with_comment_in_blockquote() {
    assert_idempotent("> <!-- comment -->\n> Some text.\n");
}

/// Idempotence with comments at start and end of document.
#[test]
fn idempotent_with_edge_comments() {
    assert_idempotent("<!-- first -->\n\nMiddle.\n\n<!-- last -->\n");
}

/// Standalone comment preserved when adjacent block changes.
#[test]
fn comment_preserved_when_adjacent_block_changes() {
    let original_qmd = "Before.\n\n<!-- a comment -->\n\nAfter.\n";
    let new_qmd = "Changed.\n\n<!-- a comment -->\n\nAfter.\n";

    let original_ast = parse_qmd(original_qmd);
    let new_ast = parse_qmd(new_qmd);
    let plan = compute_reconciliation(&original_ast, &new_ast);

    let result =
        writers::incremental::incremental_write(original_qmd, &original_ast, &new_ast, &plan)
            .expect("incremental_write failed")
            .0;

    assert!(
        result.contains("<!-- a comment -->"),
        "Standalone comment lost in incremental write output:\n{:?}",
        result
    );
}

/// Inline comment preserved when containing paragraph is rewritten.
/// The comment is now in the AST as RawInline(html), so it survives rewrite.
#[test]
fn comment_preserved_when_containing_paragraph_rewritten() {
    let original_qmd = "Hello <!-- comment --> world.\n";
    let new_qmd = "Hi <!-- comment --> world.\n";

    let original_ast = parse_qmd(original_qmd);
    let new_ast = parse_qmd(new_qmd);

    // Verify the comment IS in the AST as RawInline
    if let pampa::pandoc::Block::Paragraph(para) = &new_ast.blocks[0] {
        let has_raw_inline = para.content.iter().any(|inline| {
            matches!(inline, pampa::pandoc::Inline::RawInline(ri) if ri.text.contains("<!-- comment -->"))
        });
        assert!(
            has_raw_inline,
            "Expected RawInline with comment in paragraph. Got: {:?}",
            para.content
        );
    } else {
        panic!("Expected Paragraph block");
    }

    let plan = compute_reconciliation(&original_ast, &new_ast);
    let result =
        writers::incremental::incremental_write(original_qmd, &original_ast, &new_ast, &plan)
            .expect("incremental_write failed")
            .0;

    assert!(
        result.contains("<!-- comment -->"),
        "Inline comment lost in incremental write output:\n{:?}",
        result
    );
}

/// Comment inside blockquote preserved on rewrite.
#[test]
fn comment_inside_blockquote_preserved_on_rewrite() {
    let original_qmd = "> <!-- comment -->\n> Some text.\n";
    let new_qmd = "> <!-- comment -->\n> Changed text.\n";

    let original_ast = parse_qmd(original_qmd);
    let new_ast = parse_qmd(new_qmd);
    let plan = compute_reconciliation(&original_ast, &new_ast);

    let result =
        writers::incremental::incremental_write(original_qmd, &original_ast, &new_ast, &plan)
            .expect("incremental_write failed")
            .0;

    assert!(
        result.contains("<!-- comment -->"),
        "Comment inside blockquote lost on rewrite:\n{:?}",
        result
    );
}

/// Standalone comment block survives when blocks are added around it.
#[test]
fn comment_block_preserved_when_blocks_added() {
    let original_qmd = "Before.\n\n<!-- a comment -->\n\nAfter.\n";
    let new_qmd = "Before.\n\nNew paragraph.\n\n<!-- a comment -->\n\nAfter.\n";

    let original_ast = parse_qmd(original_qmd);
    let new_ast = parse_qmd(new_qmd);
    let plan = compute_reconciliation(&original_ast, &new_ast);

    let result =
        writers::incremental::incremental_write(original_qmd, &original_ast, &new_ast, &plan)
            .expect("incremental_write failed")
            .0;

    assert!(
        result.contains("<!-- a comment -->"),
        "Standalone comment lost when blocks added:\n{:?}",
        result
    );
}

/// Standard writer round-trip: standalone comment.
#[test]
fn roundtrip_standalone_comment() {
    assert_roundtrip(
        "Before.\n\n<!-- a comment -->\n\nAfter.\n",
        "Changed.\n\n<!-- a comment -->\n\nAfter.\n",
    );
}

/// Standard writer round-trip: inline comment.
#[test]
fn roundtrip_inline_comment() {
    assert_roundtrip(
        "Hello <!-- comment --> world.\n",
        "Hi <!-- comment --> world.\n",
    );
}

/// Standard writer round-trip: comment in blockquote.
#[test]
fn roundtrip_comment_in_blockquote() {
    assert_roundtrip(
        "> <!-- comment -->\n> Some text.\n",
        "> <!-- comment -->\n> Changed text.\n",
    );
}

// --- Phase 4: Edge case tests ---

/// Multi-line inline comment idempotence.
#[test]
fn idempotent_with_multiline_inline_comment() {
    assert_idempotent("Text <!-- multi\nline\ncomment --> done.\n");
}

/// Multi-line standalone block comment idempotence.
#[test]
fn idempotent_with_multiline_block_comment() {
    assert_idempotent("Before.\n\n<!--\nmulti\nline\nblock\n-->\n\nAfter.\n");
}

/// Empty comment idempotence.
#[test]
fn idempotent_with_empty_comment() {
    assert_idempotent("Before <!-- --> after.\n");
}

/// Comment with double dashes inside idempotence.
#[test]
fn idempotent_with_dashes_in_comment() {
    assert_idempotent("Has <!-- double -- dashes --> inside.\n");
}

/// Multi-line inline comment preserved on rewrite.
#[test]
fn multiline_comment_preserved_on_rewrite() {
    let original = "Text <!-- multi\nline\ncomment --> done.\n";
    let new = "Changed <!-- multi\nline\ncomment --> done.\n";

    let original_ast = parse_qmd(original);
    let new_ast = parse_qmd(new);
    let plan = compute_reconciliation(&original_ast, &new_ast);

    let result = writers::incremental::incremental_write(original, &original_ast, &new_ast, &plan)
        .expect("incremental_write failed")
        .0;

    assert!(
        result.contains("<!-- multi\nline\ncomment -->"),
        "Multi-line comment lost in incremental write:\n{:?}",
        result
    );
}

/// Multi-line block comment preserved when adjacent block changes.
#[test]
fn multiline_block_comment_preserved_on_adjacent_change() {
    let original = "Before.\n\n<!--\nmulti\nline\n-->\n\nAfter.\n";
    let new = "Changed.\n\n<!--\nmulti\nline\n-->\n\nAfter.\n";

    let original_ast = parse_qmd(original);
    let new_ast = parse_qmd(new);
    let plan = compute_reconciliation(&original_ast, &new_ast);

    let result = writers::incremental::incremental_write(original, &original_ast, &new_ast, &plan)
        .expect("incremental_write failed")
        .0;

    assert!(
        result.contains("<!--\nmulti\nline\n-->"),
        "Multi-line block comment lost:\n{:?}",
        result
    );
}

/// Empty comment round-trips through standard writer.
#[test]
fn roundtrip_empty_comment() {
    assert_roundtrip("Before <!-- --> after.\n", "Changed <!-- --> after.\n");
}

/// Comment with special characters round-trips.
#[test]
fn roundtrip_comment_with_dashes() {
    assert_roundtrip(
        "Has <!-- double -- dashes --> inside.\n",
        "Changed <!-- double -- dashes --> inside.\n",
    );
}

// =============================================================================
// Missing trailing newline — bd-1c6x
// =============================================================================
//
// The QMD reader internally pads input with `\n` if it doesn't end with one,
// producing source spans relative to the padded (longer) input. When the
// incremental writer receives the *original* (unpadded) string, it panics
// trying to slice at a byte index that is out of bounds.

/// Idempotence when document has no trailing newline.
#[test]
fn idempotent_no_trailing_newline() {
    assert_idempotent("Hello world.");
}

/// Idempotence: multiple blocks, no trailing newline.
#[test]
fn idempotent_two_paragraphs_no_trailing_newline() {
    assert_idempotent("First paragraph.\n\nSecond paragraph.");
}

/// Roundtrip: modify a paragraph in a document with no trailing newline.
#[test]
fn roundtrip_no_trailing_newline() {
    assert_roundtrip(
        "First paragraph.\n\nSecond paragraph.",
        "First paragraph.\n\nModified second.",
    );
}

/// Roundtrip via JSON: no trailing newline (mimics WASM path).
#[test]
fn roundtrip_no_trailing_newline_via_json() {
    let original_qmd = "## Title {.feature created=\"2026-02-10\"}\n\nParagraph.";
    let new_qmd = "## Title {.feature created=\"2026-02-10\" status=\"todo\"}\n\nParagraph.";
    let new_ast = parse_qmd(new_qmd);
    let result = incremental_write_via_json_roundtrip(original_qmd, &new_ast);
    let result_ast = parse_qmd(&result);
    assert_eq!(result_ast.blocks.len(), new_ast.blocks.len());
    for (result_block, new_block) in result_ast.blocks.iter().zip(new_ast.blocks.iter()) {
        assert!(
            quarto_ast_reconcile::structural_eq_block(result_block, new_block),
            "Block structural mismatch after JSON roundtrip (no trailing newline)"
        );
    }
}

/// Kanban-like document with no trailing newline — the original crash scenario.
#[test]
fn roundtrip_kanban_no_trailing_newline() {
    let original_qmd = concat!(
        "---\ntitle: test kanban\n---\n\n",
        "# Cards\n\n",
        "## Work Week {.milestone deadline=\"2026-03-25\" created=\"2026-02-10\"}\n\n",
        "Items:\n\n",
        "- [ ] [Project Export](#project-export)\n\n",
        "## Project Export {.feature created=\"2026-02-10\"}\n\n",
        "## ACLs {.feature created=\"2026-02-10\"}\n\n",
        "Some body text.", // <-- no trailing \n
    );
    let new_qmd = concat!(
        "---\ntitle: test kanban\n---\n\n",
        "# Cards\n\n",
        "## Work Week {.milestone deadline=\"2026-03-25\" created=\"2026-02-10\"}\n\n",
        "Items:\n\n",
        "- [ ] [Project Export](#project-export)\n\n",
        "## Project Export {.feature created=\"2026-02-10\" status=\"done\"}\n\n",
        "## ACLs {.feature created=\"2026-02-10\"}\n\n",
        "Some body text.", // <-- no trailing \n
    );
    assert_roundtrip(original_qmd, new_qmd);
}

/// Kanban-like document with no trailing newline, via JSON roundtrip (WASM path).
#[test]
fn roundtrip_kanban_no_trailing_newline_via_json() {
    let original_qmd = concat!(
        "---\ntitle: test kanban\n---\n\n",
        "# Cards\n\n",
        "## Work Week {.milestone deadline=\"2026-03-25\" created=\"2026-02-10\"}\n\n",
        "Items:\n\n",
        "- [ ] [Project Export](#project-export)\n\n",
        "## Project Export {.feature created=\"2026-02-10\"}\n\n",
        "## ACLs {.feature created=\"2026-02-10\"}\n\n",
        "Some body text.", // <-- no trailing \n
    );
    let new_qmd = concat!(
        "---\ntitle: test kanban\n---\n\n",
        "# Cards\n\n",
        "## Work Week {.milestone deadline=\"2026-03-25\" created=\"2026-02-10\"}\n\n",
        "Items:\n\n",
        "- [ ] [Project Export](#project-export)\n\n",
        "## Project Export {.feature created=\"2026-02-10\" status=\"done\"}\n\n",
        "## ACLs {.feature created=\"2026-02-10\"}\n\n",
        "Some body text.", // <-- no trailing \n
    );
    let new_ast = parse_qmd(new_qmd);
    let result = incremental_write_via_json_roundtrip(original_qmd, &new_ast);
    let result_ast = parse_qmd(&result);
    assert_eq!(result_ast.blocks.len(), new_ast.blocks.len());
    for (result_block, new_block) in result_ast.blocks.iter().zip(new_ast.blocks.iter()) {
        assert!(
            quarto_ast_reconcile::structural_eq_block(result_block, new_block),
            "Block structural mismatch in kanban no-trailing-newline JSON roundtrip"
        );
    }
}

// =============================================================================
// Plan 7g Phase 8 — writer crash on Concat/Generated-led inline boundary
//
// A paragraph whose first inline carries a (contiguous, well-formed) `Concat`
// source_info — e.g. `Str "Table:"` = Concat[Original "Table" ++ Original ":"] —
// has `start_offset() == 0` (the Concat sentinel). `assemble_inline_splice`
// computed prefix/suffix via `start_offset()`/`end_offset()`, so editing inside
// such a block produced `original_qmd[block.start .. 0]` — a reversed slice that
// panics. Fix: derive boundaries via `preimage_in`, fall back when None.
// =============================================================================

/// Edit a Str inside a Concat-led paragraph and confirm the incremental writer
/// does not panic on the reversed prefix slice.
#[test]
fn inline_splice_concat_led_paragraph_does_not_panic() {
    use pampa::pandoc::{Block, Inline};
    use quarto_source_map::SourceInfo;

    // Real parse: `Table:` becomes a contiguous Concat-led Str, and the
    // paragraph is block[1] (block.start = 35 > 0).
    let original_qmd =
        std::fs::read_to_string("tests/smoke/table.qmd").expect("table.qmd fixture present");
    let orig = parse_qmd(&original_qmd);

    // Sanity: block[1] is the Concat-led paragraph.
    assert!(matches!(orig.blocks.get(1), Some(Block::Paragraph(_))));

    // Mutate the last Str in block[1] ("work." -> "works.") so the reconciler
    // pairs the block as RecurseIntoContainer -> InlineSplice. The prefix slice
    // uses orig_inlines[0] ("Table:"), whose start_offset() is the Concat
    // sentinel 0 -> reversed slice on the current code.
    let mut new = orig.clone();
    if let Some(Block::Paragraph(p)) = new.blocks.get_mut(1) {
        if let Some(Inline::Str(s)) = p.content.last_mut() {
            s.text = "works.".to_string();
            s.source_info = SourceInfo::for_test();
        }
    }

    let plan = compute_reconciliation(&orig, &new);
    // Currently panics ("byte range starts at 35 but ends at 0"). Post-fix: Ok.
    let (out, _warnings) =
        writers::incremental::incremental_write(&original_qmd, &orig, &new, &plan)
            .expect("incremental_write should not error");
    assert!(
        out.contains("Table:"),
        "output should preserve the paragraph text"
    );
}

/// Property: `incremental_write` must never panic on any parseable qmd in the
/// pampa corpus, under identity reconciliation OR a single-Str edit (which
/// forces the InlineSplice path). Covers every confirmed Plan 7g Phase 8
/// trigger — table captions, links/images, anchor shorthands, math-with-attr,
/// smart-punctuation — by scanning the real fixtures rather than hand-built ASTs.
#[test]
fn incremental_write_never_panics_on_pampa_corpus() {
    use pampa::pandoc::{Block, Inline, Pandoc};
    use quarto_source_map::SourceInfo;

    // Clone `ast`, mutate the first `Str` in the first inline-content block, so
    // the reconciler pairs that block as RecurseIntoContainer -> InlineSplice.
    fn mutate_first_str(ast: &Pandoc) -> Option<Pandoc> {
        let mut new = ast.clone();
        for b in new.blocks.iter_mut() {
            let content = match b {
                Block::Paragraph(p) => &mut p.content,
                Block::Plain(p) => &mut p.content,
                Block::Header(h) => &mut h.content,
                _ => continue,
            };
            for inl in content.iter_mut() {
                if let Inline::Str(s) = inl {
                    s.text = format!("{}X", s.text);
                    s.source_info = SourceInfo::for_test();
                    return Some(new);
                }
            }
        }
        None
    }

    let listed = std::process::Command::new("git")
        .args(["ls-files", "*.qmd"])
        .output()
        .expect("git ls-files");
    let files = String::from_utf8(listed.stdout).unwrap();

    // Suppress panic backtraces during the sweep; collect offenders instead.
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let mut failures: Vec<String> = Vec::new();
    let mut scanned = 0usize;
    for f in files.lines() {
        let Ok(src) = std::fs::read_to_string(f) else {
            continue;
        };
        let Ok((orig, _, _)) =
            pampa::readers::qmd::read(src.as_bytes(), false, f, &mut std::io::sink(), true, None)
        else {
            continue;
        };
        scanned += 1;
        let new = mutate_first_str(&orig).unwrap_or_else(|| orig.clone());
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let plan = compute_reconciliation(&orig, &new);
            let _ = writers::incremental::incremental_write(&src, &orig, &new, &plan);
        }));
        if res.is_err() {
            failures.push(f.to_string());
        }
    }
    std::panic::set_hook(prev);

    assert!(
        failures.is_empty(),
        "incremental_write panicked on {} of {} parseable qmd files: {:?}",
        failures.len(),
        scanned,
        failures,
    );
    assert!(
        scanned > 0,
        "scanned no qmd files — corpus enumeration broken"
    );
}
