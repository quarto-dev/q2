/*
 * tests/integration/include_heading_id_dedup.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Heading-id disambiguation across include boundaries
 * (bd-duplicate-heading-ids-mou5z7ux).
 */

//! A fragment carrying a heading, included more than once, must not
//! emit the same auto-generated id verbatim every time. The fix is a
//! **scoped uniqueIdent** pass at the tail of `IncludeExpansionStage`:
//! pandoc's probe algorithm (`base`, `base-1`, `base-2`, … against a
//! seen-set), where the *renameable* set is only the include-injected
//! auto headers but the *seen-set* is the whole document.
//!
//! Deliberate divergences from Quarto 1, pinned here so they stay
//! conscious decisions rather than drift (design round 2, plan
//! `claude-notes/plans/2026-08-18-duplicate-heading-ids-includes.md`):
//!
//! - When an included heading *precedes* an inline duplicate, the
//!   *included* one is renamed (Q1's whole-document counter would
//!   rename the later, inline one). Inline headers are never
//!   renameable.
//! - Author-written `{#id}` attributes are never renamed, even when
//!   they collide (a diagnostic for that case is bd-8wf5brc8).
//!
//! These tests drive the full HTML pipeline via `render_fixture` and
//! assert on the emitted section ids in document order.

use crate::include_expansion_diagnostics::render_fixture;

/// Collect `id="..."` attribute values from `html`, in document order,
/// keeping only ids equal to `base` or `base-<digits>`.
fn ids_with_base(html: &str, base: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(pos) = rest.find("id=\"") {
        let after = &rest[pos + 4..];
        let Some(end) = after.find('"') else { break };
        let id = &after[..end];
        let is_match = id == base
            || (id.len() > base.len() + 1
                && id.starts_with(base)
                && id.as_bytes()[base.len()] == b'-'
                && id[base.len() + 1..].bytes().all(|b| b.is_ascii_digit()));
        if is_match {
            out.push(id.to_string());
        }
        rest = &after[end..];
    }
    out
}

/// The filed repro: one heading in a shared fragment, included once per
/// provider section. Expected: `create-the-integration`, `-1`, `-2` —
/// matching Quarto 1 exactly (pure-include repetition).
#[tokio::test]
async fn repeated_include_disambiguates_heading_ids() {
    let output = render_fixture(
        &[
            (
                "index.qmd",
                "## Provider A\n\n{{< include _shared.qmd >}}\n\n\
                 ## Provider B\n\n{{< include _shared.qmd >}}\n\n\
                 ## Provider C\n\n{{< include _shared.qmd >}}\n",
            ),
            (
                "_shared.qmd",
                "#### Create the integration\n\nShared boilerplate.\n",
            ),
        ],
        "index.qmd",
    )
    .await;

    assert_eq!(
        ids_with_base(&output.html, "create-the-integration"),
        vec![
            "create-the-integration",
            "create-the-integration-1",
            "create-the-integration-2"
        ],
        "each include occurrence must get a distinct id; html was:\n{}",
        output.html
    );
}

/// Nested includes: the parent includes a file that itself includes the
/// heading-carrying fragment twice. Both occurrences arrive through the
/// recursive expansion and must be disambiguated.
#[tokio::test]
async fn nested_repeated_include_disambiguates() {
    let output = render_fixture(
        &[
            ("index.qmd", "{{< include _a.qmd >}}\n"),
            (
                "_a.qmd",
                "{{< include _b.qmd >}}\n\n{{< include _b.qmd >}}\n",
            ),
            ("_b.qmd", "## Fragment heading\n\nBody.\n"),
        ],
        "index.qmd",
    )
    .await;

    assert_eq!(
        ids_with_base(&output.html, "fragment-heading"),
        vec!["fragment-heading", "fragment-heading-1"],
        "nested include occurrences must be disambiguated; html was:\n{}",
        output.html
    );
}

/// An inline heading followed by an included duplicate: the included
/// one probes to `-1`. This matches Quarto 1 (document order).
#[tokio::test]
async fn inline_then_included_duplicate() {
    let output = render_fixture(
        &[
            (
                "index.qmd",
                "## Setup\n\nInline body.\n\n{{< include _child.qmd >}}\n",
            ),
            ("_child.qmd", "## Setup\n\nIncluded body.\n"),
        ],
        "index.qmd",
    )
    .await;

    assert_eq!(
        ids_with_base(&output.html, "setup"),
        vec!["setup", "setup-1"],
        "included duplicate must probe past the inline id; html was:\n{}",
        output.html
    );
}

/// An included heading followed by an inline duplicate: the *included*
/// one is renamed even though it appears first (inline headers are
/// never renameable). Deliberate divergence from Quarto 1, which would
/// emit `setup` (included) / `setup-1` (inline).
#[tokio::test]
async fn included_then_inline_duplicate_renames_the_included_one() {
    let output = render_fixture(
        &[
            (
                "index.qmd",
                "{{< include _child.qmd >}}\n\n## Setup\n\nInline body.\n",
            ),
            ("_child.qmd", "## Setup\n\nIncluded body.\n"),
        ],
        "index.qmd",
    )
    .await;

    assert_eq!(
        ids_with_base(&output.html, "setup"),
        vec!["setup-1", "setup"],
        "the included heading is renamed; the inline one keeps its id; html was:\n{}",
        output.html
    );
}

/// uniqueIdent probes a *set*, not a per-base counter: with `setup-1`
/// already taken by an explicit author id, the second included `Setup`
/// must skip to `setup-2`.
#[tokio::test]
async fn probe_skips_explicitly_taken_suffix() {
    let output = render_fixture(
        &[
            (
                "index.qmd",
                "## Other section {#setup-1}\n\nBody.\n\n\
                 {{< include _child.qmd >}}\n\n{{< include _child.qmd >}}\n",
            ),
            ("_child.qmd", "## Setup\n\nIncluded body.\n"),
        ],
        "index.qmd",
    )
    .await;

    assert_eq!(
        ids_with_base(&output.html, "setup"),
        vec!["setup-1", "setup", "setup-2"],
        "probe must skip the explicit `setup-1`; html was:\n{}",
        output.html
    );
}

/// Author-written `{#id}` attributes are never renamed, even when the
/// same fragment (and thus the same explicit id) is included twice.
/// The resulting duplicate is invalid HTML; warning about it is
/// bd-8wf5brc8, not this fix's job.
#[tokio::test]
async fn explicit_id_in_included_file_kept_verbatim() {
    let output = render_fixture(
        &[
            (
                "index.qmd",
                "{{< include _child.qmd >}}\n\n{{< include _child.qmd >}}\n",
            ),
            ("_child.qmd", "## Stable heading {#stable}\n\nBody.\n"),
        ],
        "index.qmd",
    )
    .await;

    assert_eq!(
        ids_with_base(&output.html, "stable"),
        vec!["stable", "stable"],
        "explicit ids must never be renamed; html was:\n{}",
        output.html
    );
}

/// Headers outside included content are never renamed: the reader's
/// per-parse dedup result for inline duplicates stands, and the
/// included duplicate probes past it.
#[tokio::test]
async fn inline_duplicates_keep_reader_ids_included_probes_past() {
    let output = render_fixture(
        &[
            (
                "index.qmd",
                "## Setup\n\nFirst.\n\n## Setup\n\nSecond.\n\n{{< include _child.qmd >}}\n",
            ),
            ("_child.qmd", "## Setup\n\nIncluded body.\n"),
        ],
        "index.qmd",
    )
    .await;

    assert_eq!(
        ids_with_base(&output.html, "setup"),
        vec!["setup", "setup-1", "setup-2"],
        "inline pair keeps reader-assigned ids; included probes to -2; html was:\n{}",
        output.html
    );
}

/// A document with no includes is untouched by the pass (the stage
/// gates on "at least one include expanded"): the reader's per-parse
/// dedup remains exactly what renders. Pins today's behavior.
#[tokio::test]
async fn no_include_document_keeps_reader_dedup() {
    let output = render_fixture(
        &[("index.qmd", "## Setup\n\nFirst.\n\n## Setup\n\nSecond.\n")],
        "index.qmd",
    )
    .await;

    assert_eq!(
        ids_with_base(&output.html, "setup"),
        vec!["setup", "setup-1"],
        "inline-only documents keep reader dedup; html was:\n{}",
        output.html
    );
}
