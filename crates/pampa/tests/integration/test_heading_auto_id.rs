//! Tests for auto-generated heading ids (`pampa::utils::autoid`).
//!
//! When a heading carries no explicit `{#id}`, the reader's postprocess
//! pass derives one from the heading text. Every expected value below was
//! measured against **pandoc 3.9.0.2** (`pandoc -f markdown+smart -t html
//! --section-divs`), which is the same algorithm Quarto 1 uses. The
//! measurement tables live in
//! `claude-notes/plans/heading-id-drops-inline-content-investigation/observed-2026-08-13.md`.
//!
//! Regression coverage for bd-heading-id-drops-inline-content-fl84n3ql:
//! `collect_text` used to handle only `Str`, `Space`, `Emph`, `Strong` and
//! `Code`, discarding every other inline kind *without recursing into it*,
//! so whole words vanished from the anchor id.

use pampa::pandoc::Block;
use pampa::readers;

fn heading_ids(input: &str) -> Vec<String> {
    let (pandoc, _, _) = readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    )
    .expect("Failed to parse QMD");

    pandoc
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Header(h) => Some(h.attr.0.clone()),
            _ => None,
        })
        .collect()
}

fn heading_id(input: &str) -> String {
    let ids = heading_ids(input);
    assert_eq!(ids.len(), 1, "expected exactly one heading in {input:?}");
    ids.into_iter().next().unwrap()
}

// ============================================================================
// Content that must survive into the id
//
// Each fixture is `## <label> <construct> end`, so a correct id keeps both
// sentinels. Before the fix, every one of these dropped the middle entirely.
// ============================================================================

#[test]
fn test_auto_id_keeps_quoted_span_content() {
    // The Connect-docs heading that surfaced this bug.
    assert_eq!(
        heading_id(r#"## Using a "raw" volume"#),
        "using-a-raw-volume"
    );
}

#[test]
fn test_auto_id_keeps_single_quoted_span_content() {
    assert_eq!(heading_id("## quoteds 'single' end"), "quoteds-single-end");
}

#[test]
fn test_auto_id_keeps_link_text() {
    assert_eq!(
        heading_id("## link [the docs](https://example.com) end"),
        "link-the-docs-end"
    );
}

#[test]
fn test_auto_id_keeps_strikeout_content() {
    assert_eq!(heading_id("## strike ~~out~~ end"), "strike-out-end");
}

#[test]
fn test_auto_id_keeps_math_text() {
    assert_eq!(heading_id("## math $x+y$ end"), "math-xy-end");
}

#[test]
fn test_auto_id_keeps_smallcaps_content() {
    assert_eq!(
        heading_id("## smallcaps [caps]{.smallcaps} end"),
        "smallcaps-caps-end"
    );
}

#[test]
fn test_auto_id_keeps_span_content() {
    assert_eq!(
        heading_id("## span [inner]{.myclass} end"),
        "span-inner-end"
    );
}

#[test]
fn test_auto_id_keeps_underline_content() {
    assert_eq!(
        heading_id("## underline [under]{.underline} end"),
        "underline-under-end"
    );
}

#[test]
fn test_auto_id_keeps_superscript_content() {
    assert_eq!(heading_id("## super a^sup^ end"), "super-asup-end");
}

#[test]
fn test_auto_id_keeps_subscript_content() {
    assert_eq!(heading_id("## sub a~sub~ end"), "sub-asub-end");
}

#[test]
fn test_auto_id_keeps_image_alt_text() {
    // Not in the strand's table; Pandoc's `stringify` walks into image alt.
    assert_eq!(
        heading_id("## image ![alt text](img.png) end"),
        "image-alt-text-end"
    );
}

#[test]
fn test_auto_id_keeps_cite_content() {
    // Not in the strand's table; Pandoc keeps the citation as written.
    assert_eq!(heading_id("## cite [@somekey] end"), "cite-somekey-end");
}

#[test]
fn test_auto_id_keeps_cite_prefix_and_suffix() {
    assert_eq!(
        heading_id("## cite [see @somekey, p. 33] end"),
        "cite-see-somekey-p.-33-end"
    );
}

#[test]
fn test_auto_id_keeps_multiple_citations() {
    assert_eq!(heading_id("## cite [@a; @b] end"), "cite-a-b-end");
}

#[test]
fn test_auto_id_keeps_author_in_text_citation() {
    // The one citation form where pampa populates `Cite.content`.
    assert_eq!(
        heading_id("## intext @somekey says end"),
        "intext-somekey-says-end"
    );
}

// ============================================================================
// Controls: kinds that were already correct and must stay that way
// ============================================================================

#[test]
fn test_auto_id_keeps_emphasis_strong_and_code() {
    // The strand's control row: the three container/leaf kinds the old
    // collector handled.
    assert_eq!(
        heading_id("## Use *emphasis* and **strong** and `code` here"),
        "use-emphasis-and-strong-and-code-here"
    );
}

#[test]
fn test_auto_id_excludes_footnote_body() {
    // Pandoc's `deNote` strips the note before stringifying.
    assert_eq!(
        heading_id("## note here^[a footnote body] end"),
        "note-here-end"
    );
}

#[test]
fn test_auto_id_excludes_raw_inline() {
    // `rawhtml` survives as a plain Str; the `<span>` tags are RawInline and
    // contribute nothing. Pandoc agrees.
    assert_eq!(
        heading_id("## raw <span>rawhtml</span> end"),
        "raw-rawhtml-end"
    );
}

#[test]
fn test_auto_id_excludes_shortcode() {
    // q2-only kind with no Pandoc analogue. q2 derives the id in the
    // reader's postprocess pass, before shortcodes are expanded, so an
    // unexpanded shortcode contributes nothing. Quarto 1 expands shortcodes
    // in a pre-Pandoc text pass and therefore includes the expansion --
    // a deliberate divergence, tracked separately.
    assert_eq!(
        heading_id("## before {{< meta foo >}} after"),
        "before-after"
    );
}

#[test]
fn test_auto_id_respects_explicit_id() {
    assert_eq!(
        heading_id(r#"## Using a "raw" volume {#explicit}"#),
        "explicit"
    );
}

// ============================================================================
// Slug filter: leading non-letters are dropped (Pandoc's `dropNonLetter`)
// ============================================================================

#[test]
fn test_auto_id_drops_leading_digit() {
    assert_eq!(heading_id("## 1 leading digit"), "leading-digit");
}

#[test]
fn test_auto_id_drops_leading_dot() {
    assert_eq!(heading_id("## .leading dot"), "leading-dot");
}

#[test]
fn test_auto_id_drops_leading_number_prefix() {
    assert_eq!(heading_id("## 2026 roadmap"), "roadmap");
}

#[test]
fn test_auto_id_keeps_leading_non_ascii_letter() {
    // `dropNonLetter` drops non-*letters*, and U+00DC is a letter.
    assert_eq!(heading_id("## Ünicode leading"), "ünicode-leading");
}

#[test]
fn test_auto_id_keeps_interior_punctuation() {
    assert_eq!(heading_id("## punct a.b_c-d end"), "punct-a.b_c-d-end");
}

// ============================================================================
// Empty-id fallback (Pandoc's `section` / `section-1` / ...)
// ============================================================================

#[test]
fn test_auto_id_empty_falls_back_to_section() {
    // An image with empty alt collects to the empty string. Before the fix
    // this emitted a heading with no id at all -- an unlinkable section.
    assert_eq!(heading_id("## ![](img.png)"), "section");
}

#[test]
fn test_auto_id_all_digits_falls_back_to_section() {
    // `dropNonLetter` empties the ident, and the fallback then applies.
    assert_eq!(heading_id("## 123"), "section");
}

#[test]
fn test_auto_id_repeated_empty_headings_are_deduplicated() {
    let ids = heading_ids("## ![](a.png)\n\n## ![](b.png)\n\n## ![](c.png)\n");
    assert_eq!(ids, vec!["section", "section-1", "section-2"]);
}

// ============================================================================
// The dedup interaction the strand predicted
// ============================================================================

#[test]
fn test_auto_id_headings_differing_only_inside_dropped_content_no_longer_collide() {
    // Before the fix all three collected to "", so the dedup counter handed
    // out "", "-1", "-2": the first heading was unlinkable and the other two
    // got ids unrelated to their text.
    let ids = heading_ids("## $x$\n\n## ~~gone~~\n\n## [also gone](https://example.com)\n");
    assert_eq!(ids, vec!["x", "gone", "also-gone"]);
}

#[test]
fn test_auto_id_distinct_quoted_headings_do_not_collide() {
    let ids = heading_ids("## Using a \"raw\" volume\n\n## Using a \"cooked\" volume\n");
    assert_eq!(ids, vec!["using-a-raw-volume", "using-a-cooked-volume"]);
}

// ============================================================================
// qmd writer round-trip of deduplicated ids
// (bd-duplicate-heading-ids-mou5z7ux Phase 0 pin)
// ============================================================================

#[test]
fn test_deduped_id_roundtrips_as_explicit_attr() {
    // A deduplicated auto id (`setup-1`) no longer equals what
    // `auto_generated_id` would recompute from the heading content
    // (`setup`), so the qmd writer must emit it as an explicit `{#setup-1}`
    // — otherwise a re-parse would reassign ids and anchors would drift.
    // This applies identically to ids assigned by the document-level
    // scoped-dedup pass, which also leaves `attr_source.id` as `None`.
    let (ast, _, _) = pampa::readers::qmd::read(
        b"## Setup\n\nFirst.\n\n## Setup\n\nSecond.\n",
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    )
    .expect("Failed to parse QMD");

    let mut buf = Vec::new();
    pampa::writers::qmd::write(&ast, &mut buf).expect("Failed to write QMD");
    let written = String::from_utf8(buf).expect("utf8");

    assert!(
        !written.contains("{#setup}"),
        "the first heading's id matches its recomputed base and must be \
         suppressed; got:\n{written}"
    );
    assert!(
        written.contains("## Setup {#setup-1}"),
        "the deduped heading must round-trip with an explicit id; got:\n{written}"
    );
}

// ============================================================================
// dedup_scoped_heading_ids (bd-duplicate-heading-ids-mou5z7ux)
//
// Unit tests for the scoped uniqueIdent routine. Include-splicing is
// simulated by parsing fragments separately (so the reader assigns each a
// fresh per-parse id, exactly like IncludeExpansionStage's standalone child
// parses) and concatenating their blocks; the scope predicate then selects
// the "injected" headers by level, standing in for the stage's file-id
// provenance test.
// ============================================================================

fn parse_blocks(input: &str) -> Vec<Block> {
    let (pandoc, _, _) = pampa::readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    )
    .expect("Failed to parse QMD");
    pandoc.blocks
}

fn doc_from_blocks(blocks: Vec<Block>) -> pampa::pandoc::Pandoc {
    pampa::pandoc::Pandoc {
        meta: Default::default(),
        blocks,
    }
}

fn header_ids_in(doc: &pampa::pandoc::Pandoc) -> Vec<String> {
    doc.blocks
        .iter()
        .filter_map(|b| match b {
            Block::Header(h) => Some(h.attr.0.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn test_dedup_scoped_renames_only_in_scope() {
    // "## H" (main) and "### H" (fragment) parsed separately both get `h`.
    let mut blocks = parse_blocks("## H\n");
    blocks.extend(parse_blocks("### H\n"));
    let doc =
        pampa::utils::autoid::dedup_scoped_heading_ids(doc_from_blocks(blocks), |h| h.level == 3);
    assert_eq!(header_ids_in(&doc), vec!["h", "h-1"]);
}

#[test]
fn test_dedup_scoped_probe_is_set_membership_not_counter() {
    // An explicit `{#h-1}` outside the scope forces the probe to skip it.
    let mut blocks = parse_blocks("## Other {#h-1}\n");
    blocks.extend(parse_blocks("### H\n"));
    blocks.extend(parse_blocks("### H\n"));
    let doc =
        pampa::utils::autoid::dedup_scoped_heading_ids(doc_from_blocks(blocks), |h| h.level == 3);
    assert_eq!(header_ids_in(&doc), vec!["h-1", "h", "h-2"]);
}

#[test]
fn test_dedup_scoped_explicit_ids_in_scope_untouched() {
    // Explicit ids are never renamed even when in scope and colliding.
    let mut blocks = parse_blocks("### Stable {#stable}\n");
    blocks.extend(parse_blocks("### Stable {#stable}\n"));
    let doc =
        pampa::utils::autoid::dedup_scoped_heading_ids(doc_from_blocks(blocks), |h| h.level == 3);
    assert_eq!(header_ids_in(&doc), vec!["stable", "stable"]);
}

#[test]
fn test_dedup_scoped_empty_scope_is_identity() {
    let mut blocks = parse_blocks("## H\n");
    blocks.extend(parse_blocks("## H\n"));
    let doc = pampa::utils::autoid::dedup_scoped_heading_ids(doc_from_blocks(blocks), |_| false);
    // Both keep the colliding reader-assigned id: nothing is in scope.
    assert_eq!(header_ids_in(&doc), vec!["h", "h"]);
}

#[test]
fn test_dedup_scoped_recompute_ignores_fragment_internal_numbering() {
    // A fragment with two identical headings parses to `h`, `h-1`
    // internally. The scoped pass recomputes from content, so after an
    // out-of-scope `h` is seeded, the fragment's pair probes to `h-1`,
    // `h-2` — not `h-1`, `h-1-1`.
    let mut blocks = parse_blocks("## H\n");
    blocks.extend(parse_blocks("### H\n\n### H\n"));
    let doc =
        pampa::utils::autoid::dedup_scoped_heading_ids(doc_from_blocks(blocks), |h| h.level == 3);
    assert_eq!(header_ids_in(&doc), vec!["h", "h-1", "h-2"]);
}
