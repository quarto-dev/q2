/*
 * tiling_phase3_tests.rs
 *
 * Plan 7g Phase 3 — TDD tests for handler-enforced tiling fixes.
 * Each test parses a known-violating document, runs audit_source_range_tiling,
 * and asserts that no SiblingOverlap (or ContainmentViolation) findings appear.
 *
 * Tests are RED before the handler fix and GREEN after.
 */

use pampa::writers::incremental::{TilingFindingKind, audit_source_range_tiling};

fn parse_qmd(src: &str) -> pampa::pandoc::Pandoc {
    pampa::readers::qmd::read(
        src.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    )
    .expect("parse failed")
    .0
}

fn audit(src: &str) -> Vec<pampa::writers::incremental::TilingFinding> {
    let ast = parse_qmd(src);
    audit_source_range_tiling(&ast, src)
}

fn overlap_messages(src: &str) -> Vec<String> {
    audit(src)
        .into_iter()
        .filter(|f| f.kind == TilingFindingKind::SiblingOverlap)
        .map(|f| f.message)
        .collect()
}

// =============================================================================
// code_span_helpers.rs — inline code span leading space
// =============================================================================

#[test]
fn code_span_leading_space_no_overlap() {
    // "a `code` b" — Space and Code both absorb [1..8) with current bug.
    // After fix: Space [1..2), Code [2..8).
    let overlaps = overlap_messages("a `code` b\n");
    assert!(
        overlaps.is_empty(),
        "Space∩Code overlap after code_span fix: {overlaps:#?}"
    );
}

#[test]
fn code_span_leading_space_abbreviation_cascade_no_overlap() {
    // "e.g. `code`" — coalesce_abbreviations inherits the wrong Space range
    // [4..11) and creates Str "e.g. " with Concat [0..4)+[4..11) = [0..11),
    // which then overlaps Code [4..11). Fixing code_span_helpers clears both.
    let overlaps = overlap_messages("e.g. `code`\n");
    assert!(
        overlaps.is_empty(),
        "Str∩Code cascade overlap after code_span fix: {overlaps:#?}"
    );
}

#[test]
fn raw_inline_leading_space_no_overlap() {
    // " `r code`{=r}" — raw inline variant also goes through code_span_helpers.
    let overlaps = overlap_messages("a `code`{=raw} b\n");
    assert!(
        overlaps.is_empty(),
        "Space∩RawInline overlap after code_span fix: {overlaps:#?}"
    );
}

// =============================================================================
// citation.rs — leading space on citation
// =============================================================================

#[test]
fn citation_leading_space_no_overlap() {
    // "Hi @cite" — Space and Cite both absorb [2..8) with current bug.
    // After fix: Space [2..3), Cite [3..8).
    let overlaps = overlap_messages("Hi @pandoc2024\n");
    assert!(
        overlaps.is_empty(),
        "Space∩Cite overlap after citation fix: {overlaps:#?}"
    );
}

// =============================================================================
// quote_helpers.rs — quoted span with leading space
// =============================================================================

#[test]
fn quoted_span_leading_space_no_overlap() {
    // "a \"hello\"" — Space has correct [1..2) but Quoted absorbs [1..8) (whole node).
    // After fix: Quoted [2..9) (excluding the leading space).
    let overlaps = overlap_messages("a \"hello\"\n");
    assert!(
        overlaps.is_empty(),
        "Space∩Quoted overlap after quote_helpers fix: {overlaps:#?}"
    );
}

// =============================================================================
// postprocess.rs — math-with-attr Span (ScatteredConcat)
// =============================================================================

#[test]
fn math_with_attr_span_no_scattered_concat() {
    // "$E = mc^2$ {#eq-einstein}" — the Span wrapping math + attr currently
    // stores Concat[math_body, attr_content] with ' {' gap (non-whitespace).
    // After fix: Span gets a single tight Original range.
    let findings = audit("$E = mc^2$ {#eq-einstein}\n");
    let scattered: Vec<_> = findings
        .iter()
        .filter(|f| f.kind == TilingFindingKind::ScatteredConcat)
        .collect();
    assert!(
        scattered.is_empty(),
        "ScatteredConcat after math-with-attr fix: {scattered:#?}"
    );
}

#[test]
fn display_math_with_attr_no_scattered_concat() {
    // "$$\np(x)\n$$ {#eq-p}" — display math variant.
    let src = "$$\np(x)\n$$ {#eq-p}\n";
    let findings = audit(src);
    let scattered: Vec<_> = findings
        .iter()
        .filter(|f| f.kind == TilingFindingKind::ScatteredConcat)
        .collect();
    assert!(
        scattered.is_empty(),
        "ScatteredConcat after display math fix: {scattered:#?}"
    );
}

// =============================================================================
// Phase 4b: coalesce_abbreviations — WhitespaceGapConcat
// =============================================================================

#[test]
fn abbreviation_coalesce_hull_no_whitespace_gap_concat() {
    // "(e.g. this fails)" — `coalesce_abbreviations` merges Str "e.g." + Space
    // + Str "this" into a Str with source_info = Concat[Original[1..5), Original[5..6)].
    // Non-contiguous Concat → WhitespaceGapConcat. After fix: contiguous Original hull.
    let findings = audit("(e.g. this fails)\n");
    let wgc: Vec<_> = findings
        .iter()
        .filter(|f| f.kind == TilingFindingKind::WhitespaceGapConcat)
        .collect();
    assert!(
        wgc.is_empty(),
        "WhitespaceGapConcat after abbreviation-coalesce fix: {wgc:#?}"
    );
}

#[test]
fn dr_smith_abbreviation_hull_correct() {
    // "Dr. Smith wrote" — the 2-token coalesce case from the plan.
    // merged Str should resolve to Some(hull) via preimage_in, not None.
    let src = "Dr. Smith wrote.\n";
    let ast = parse_qmd(src);
    let findings = audit_source_range_tiling(&ast, src);
    let wgc: Vec<_> = findings
        .iter()
        .filter(|f| f.kind == TilingFindingKind::WhitespaceGapConcat)
        .collect();
    assert!(
        wgc.is_empty(),
        "WhitespaceGapConcat in Dr. Smith case: {wgc:#?}"
    );
}

// =============================================================================
// Phase 4: postprocess.rs — Figure Plain∩Plain duplication
// =============================================================================

#[test]
fn figure_plain_plain_no_overlap() {
    // "![alt text](url)" — desugared into a Figure with two sibling Plain blocks.
    // Both currently get image.source_info (the full ![]() range) → SiblingOverlap.
    // After fix: caption Plain gets Generated source_info (no contiguous claim).
    let overlaps = overlap_messages("![alt text](url)\n");
    assert!(
        overlaps.is_empty(),
        "Plain∩Plain overlap after Figure fix: {overlaps:#?}"
    );
}

// =============================================================================
// list_table.rs — cell containment violation
// =============================================================================

#[test]
fn list_table_cell_containment_no_violation() {
    // Two-paragraph list-table cell from the census: cell range [30..47]
    // doesn't contain child Paragraph [47..63].
    // After fix: cell source_info covers all its content.
    let src = "::: {.list-table}\n- - foo\n  - Add values:\n\n    Then more text.\n:::\n";
    let findings = audit(src);
    let containment: Vec<_> = findings
        .iter()
        .filter(|f| f.kind == TilingFindingKind::ContainmentViolation)
        .collect();
    assert!(
        containment.is_empty(),
        "ContainmentViolation after list_table fix: {containment:#?}"
    );
}

// =============================================================================
// check_tightness — nodes whose own text retains the boundary whitespace
// =============================================================================

fn tightness_messages(src: &str) -> Vec<String> {
    audit(src)
        .into_iter()
        .filter(|f| f.kind == TilingFindingKind::TightnessViolation)
        .map(|f| f.message)
        .collect()
}

#[test]
fn abbreviation_nbsp_str_not_tightness_violation() {
    // `e.g. ` — the abbreviation handler substitutes a NON-BREAKING space for
    // the source space and keeps it in the Str's text ("e.g.\u{a0}"), so the
    // node's range legitimately ends on a source space byte: those 5 bytes are
    // exactly what produced the node. Same reasoning that already excludes
    // Space/SoftBreak/LineBreak — a node whose own content *is* that
    // whitespace is not claiming a neighbour's bytes.
    //
    // Found by the corpus audit (pandoc-match-corpus/markdown/048.qmd), which
    // was reporting this as a violation (bd-1d6io).
    let msgs = tightness_messages("e.g. `code`\n");
    assert!(
        msgs.is_empty(),
        "abbreviation NBSP Str should not be a TightnessViolation, got: {msgs:?}"
    );
}
