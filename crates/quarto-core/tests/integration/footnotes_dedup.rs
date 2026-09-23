/*
 * tests/footnotes_dedup.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Regression test for the pandoc-hybrid P1 Task 5 fix round 1: two
 * references to the same named footnote must share one footnote entry
 * under the real HTML render pipeline.
 */

//! Splitting `FootnotesTransform` into its B1 (`footnotes`) and B2/B4
//! (`footnotes-resolve`) halves initially lost the pre-split transform's
//! `resolve_reference` dedup: two `[^a]` references to one `[^a]: text`
//! definition used to share a single footnote number/entry (one `<li>`, both
//! reference spans pointing at it); the split's first cut resolved each
//! reference independently, producing two duplicated entries instead.
//!
//! The fix carries the reference id across the B1/B2 cut *inside* the
//! `Inline::Note`'s own content (a `RawBlock` marker `FootnotesTransform`
//! prepends and `FootnotesResolveTransform` strips) rather than a
//! `RenderContext` side channel — see `mark_with_ref_id`/
//! `extract_and_strip_ref_id` in `footnotes.rs`/`footnotes_resolve.rs`.
//!
//! This test drives the real `render_to_file(..., "html", ...)` pipeline
//! (both halves, in their real registered order) — not the transforms
//! called directly — so it also exercises the actual HTML writer's
//! rendering of the deduped structure, not just the intermediate AST.
//!
//! A third case, `inline_note_and_numerically_named_footnote_do_not_collide`,
//! pins a related but distinct fix: the same marker-based dedup also
//! resolved a pre-existing id-space collision between inline `^[...]` notes
//! (pre-split, assigned the synthetic id `number.to_string()`) and named
//! references whose literal id is a decimal integer (e.g. `[^1]`). See that
//! test's doc comment for the mechanism.

use std::path::Path;
use std::sync::Arc;

use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

/// Render `contents` as a single-file HTML document, returning the HTML.
fn render_html(contents: &str) -> String {
    let temp = tempfile::TempDir::new().unwrap();
    let qmd_path = temp.path().join("doc.qmd");
    write_file(&qmd_path, contents);
    let options = RenderToFileOptions {
        quiet: true,
        ..Default::default()
    };
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result = render_to_file(&qmd_path, "html", &options, runtime).expect("html render failed");
    read(&result.output_path)
}

#[test]
fn two_references_to_same_footnote_id_share_one_entry() {
    let html = render_html(
        "---\ntitle: Test\n---\n\nFirst[^a] and second[^a].\n\n[^a]: Shared note text.\n",
    );

    // The shared definition content appears exactly once — not duplicated
    // into two separate footnote items.
    let occurrences = html.matches("Shared note text.").count();
    assert_eq!(
        occurrences, 1,
        "two references to the same footnote id must share ONE footnote entry, not \
         duplicate the content; got {occurrences} occurrence(s) in html:\n{html}"
    );

    // Both reference sites still get their own superscript link, and both
    // point at the SAME shared entry (byte-identical to the pre-split
    // transform's behavior of reusing one number/fnref id across repeat
    // references — the resulting duplicate `id="fnref1"` DOM attribute is a
    // known, pre-existing, deliberately-unfixed quirk, not something this
    // regression test's fix is responsible for changing).
    let fnref_count = html.matches("id=\"fnref1\"").count();
    assert_eq!(
        fnref_count, 2,
        "both reference occurrences must produce their own fnref span, sharing the same \
         number/id (matching pre-split behavior); got {fnref_count} in html:\n{html}"
    );

    // Exactly one footnote item exists in the trailing footnotes section.
    let fn_item_count = html.matches("id=\"fn1\"").count();
    assert_eq!(
        fn_item_count, 1,
        "exactly one footnote item (fn1) must exist in the footnotes section — a second, \
         duplicated entry would mean the dedup regressed; got {fn_item_count} in html:\n{html}"
    );

    // Only one footnote-back backlink — the shared entry has one, not two.
    let backlink_count = html.matches("footnote-back").count();
    assert_eq!(
        backlink_count, 1,
        "the shared footnote entry must have exactly one backlink; got {backlink_count} in \
         html:\n{html}"
    );
}

/// Distinct footnote ids must NOT be merged — the dedup keys on the
/// reference id, not on coincidentally-identical content or on being a
/// footnote at all. Guards against an overly broad dedup implementation
/// (e.g. content-equality-based) that would wrongly collapse two unrelated
/// footnotes that happen to say the same thing.
#[test]
fn distinct_footnote_ids_are_not_merged_even_with_identical_content() {
    let html = render_html(
        "---\ntitle: Test\n---\n\nOne[^a] and two[^b].\n\n[^a]: Same text.\n\n[^b]: Same text.\n",
    );

    let occurrences = html.matches("Same text.").count();
    assert_eq!(
        occurrences, 2,
        "two DIFFERENT footnote ids with coincidentally identical content must remain two \
         separate entries; got {occurrences} in html:\n{html}"
    );
    assert!(html.contains("id=\"fn1\""));
    assert!(html.contains("id=\"fn2\""));
}

/// Regression test for final-review Important #1: pins the fix for a
/// pre-existing id-space collision the B1/B2 split incidentally corrected.
///
/// Pre-split, an inline `^[...]` note was assigned the synthetic id
/// `number.to_string()`, and named-reference dedup scanned for
/// `footnote.id == ref_id` — so an inline note landing on number *N*
/// collided with a later reference whose literal id is `"N"`. For a
/// document with an inline note followed by `[^1]`, the collision made the
/// pre-split transform silently drop `[^1]`'s definition content entirely
/// (its content was already removed from the AST by
/// `collect_note_definitions`, and the dedup scan then found the id "1"
/// already claimed by the inline note and never consumed the definition).
///
/// Post-split, `FootnoteCollector::resolve_or_add` in
/// `footnotes_resolve.rs` keys dedup on the *named* reference id carried by
/// the `FOOTNOTE_REF_ID_MARKER_FORMAT` marker, which inline notes never
/// carry — so the two id spaces no longer collide, and both footnotes
/// render. This is a genuine (and correct) behavior change from the
/// pre-split transform, not a regression; see the "byte-identical except…"
/// note on Task 5 in
/// `claude-notes/plans/2026-09-18-pandoc-hybrid-P1-implementation.md`.
#[test]
fn inline_note_and_numerically_named_footnote_do_not_collide() {
    let html = render_html(
        "---\ntitle: Test\n---\n\nAlpha^[Inline note body.]\n\nBeta[^1]\n\n[^1]: Named note body.\n",
    );

    assert_eq!(
        html.matches("Inline note body.").count(),
        1,
        "the inline ^[...] note's content must appear exactly once; html:\n{html}"
    );
    assert_eq!(
        html.matches("Named note body.").count(),
        1,
        "the named [^1] footnote's content must NOT be silently dropped by an id-space \
         collision with the inline note; html:\n{html}"
    );

    // Two distinct footnote entries, not one.
    assert!(html.contains("id=\"fn1\""), "expected fn1; html:\n{html}");
    assert!(html.contains("id=\"fn2\""), "expected fn2; html:\n{html}");
    assert_eq!(
        html.matches("footnote-back").count(),
        2,
        "two separate footnote entries must each carry their own backlink; html:\n{html}"
    );
}
