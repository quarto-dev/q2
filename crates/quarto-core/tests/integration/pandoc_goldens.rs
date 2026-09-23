/*
 * pandoc_goldens.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * P7 Task 11 (claude-notes/plans/2026-09-18-pandoc-hybrid-P7-implementation.md):
 * the CI-runnable half of the golden-parity harness. Renders each
 * `pandoc-goldens` fixture through Q2's own real Pandoc-hybrid path
 * (`render_document_to_file`, the same entry point the CLI uses — not a
 * lower-level shim call), extracts it with the identical
 * `quarto_ooxml_extract` used by `cargo xtask capture-pandoc-goldens`
 * (P7 Task 10), and asserts it against the same committed `.snap` files
 * that xtask wrote from a real Q1 `quarto`. One assertion therefore
 * serves both Q1-parity and Q2-regression.
 *
 * Requires a real `pandoc` on PATH (the vendored Lua shim), but **no**
 * `quarto` binary and no `external-sources/` — this file is CI-runnable,
 * unlike Task 10's dev-only capture.
 */

use std::path::PathBuf;
use std::sync::Arc;

use quarto_core::project::ProjectContext;
use quarto_core::render_to_file::{RenderToFileOptions, render_document_to_file};
use quarto_ooxml_extract::{Extraction, FIXTURES, golden_snapshot_name};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

/// Fixtures whose golden comparison is a **known, accepted** divergence
/// from Q1 rather than a regression. Each entry here must have a
/// corresponding entry in `tests/fixtures/pandoc-goldens/DIVERGENCES.md`
/// (asserted by `test_divergence_ledger_is_complete_and_accurate` below) —
/// this list is the "marker" T11.5 checks the ledger against, since the
/// committed `.snap` files themselves carry no divergence annotation (they
/// are auto-written, byte-for-byte, by the capture xtask).
///
/// **mermaid (`smoke-all/mermaid/backticks.qmd`)**: Q1 rasterizes the
/// mermaid cell via a real headless-browser pipeline into an embedded
/// image; Q2 has no such renderer for the Pandoc-hybrid tail (design §12,
/// `bd-h1ub8f8z`). The two sides' extractions therefore differ in their
/// media inventory. `test_mermaid_fixture_preserves_diagram_source_text`
/// covers this fixture's shape separately (T11.6).
const ACCEPTED_DIVERGENT_SNAPSHOTS: &[&str] = &[
    "smoke_all_mermaid_backticks__docx",
    "smoke_all_mermaid_backticks__pptx",
];

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pandoc-goldens")
}

fn snapshots_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/integration/snapshots")
}

fn divergences_ledger_path() -> PathBuf {
    fixtures_root().join("DIVERGENCES.md")
}

/// Stage one fixture's `.qmd` and declared resources into a fresh project
/// dir, render it through Q2's real Pandoc-hybrid path to `format`, and
/// return the rendered file's bytes.
fn render_fixture(qmd: &str, resources: &[&str], format: &str) -> Vec<u8> {
    let project_dir = tempfile::tempdir().expect("failed to create temp project dir");
    let src_root = fixtures_root();

    let staged_qmd = project_dir.path().join(qmd);
    if let Some(parent) = staged_qmd.parent() {
        std::fs::create_dir_all(parent).expect("failed to create staged qmd parent dir");
    }
    std::fs::copy(src_root.join(qmd), &staged_qmd).expect("failed to copy fixture qmd");
    for resource in resources {
        let dst = project_dir.path().join(resource);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).expect("failed to create staged resource parent dir");
        }
        std::fs::copy(src_root.join(resource), &dst).expect("failed to copy fixture resource");
    }

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    // A discovered `ProjectContext` (not `None`) is required for image
    // resource resolution — `crossref_number_parity.rs::render_html_text`
    // established this same pattern. Without it, an image reference like
    // `img/thinker.jpg` fails to resolve, which — beyond the obviously
    // missing media entry — also duplicates the caption/alt text as a
    // stray extra paragraph in the docx output.
    let project = ProjectContext::discover(&staged_qmd, runtime.as_ref())
        .unwrap_or_else(|e| panic!("project discovery for {qmd} must succeed: {e}"));
    // `format` (the 2nd positional) is only a last-resort *default*,
    // outranked by the document's own front-matter `format:` key in
    // `resolve_format_key`'s prefer-merge — several real quarto-cli-
    // sourced fixtures declare `format: latex` (they were authored for
    // Q1's latex output), which would otherwise win and error with
    // "Unknown format: latex" (latex is a documented stub, no
    // `FormatIdentifier` implements it). `format_override` (the last
    // positional) is what a real `--to docx` CLI invocation threads
    // through (`pass2_renderer.rs`'s `self.format_override`), and
    // correctly outranks the front matter — pass it explicitly so this
    // helper matches what the real CLI does, not just what happens to
    // work for fixtures with no declared `format:`.
    let result = render_document_to_file(
        &staged_qmd,
        format,
        &RenderToFileOptions::default(),
        Some(&project),
        runtime,
        None,
        None,
        Some(format),
    )
    .unwrap_or_else(|e| panic!("{format} render of {qmd} must succeed: {e}"));

    std::fs::read(&result.output_path).expect("failed to read rendered output")
}

fn extract(bytes: &[u8], format: &str) -> Extraction {
    let result = if format == "docx" {
        quarto_ooxml_extract::extract_docx(bytes)
    } else {
        quarto_ooxml_extract::extract_pptx(bytes)
    };
    result.unwrap_or_else(|e| panic!("extracting {format} output: {e}"))
}

/// T11.1 + T11.2: for every fixture not listed in
/// [`ACCEPTED_DIVERGENT_SNAPSHOTS`], render through Q2's own hybrid path
/// (docx and pptx) and assert the extraction matches the committed
/// Q1-captured snapshot exactly. A real regression in P4's transport,
/// P5's Route-R shim, P6's numbering wiring, or Task 4's invocation
/// builder reddens this test with a normal insta diff.
#[test]
fn test_fixtures_match_q1_golden() {
    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_path(snapshots_dir());
    settings.set_prepend_module_to_snapshot(false);
    let _guard = settings.bind_to_scope();

    for fixture in FIXTURES {
        for format in ["docx", "pptx"] {
            let name = golden_snapshot_name(fixture.qmd, format);
            if ACCEPTED_DIVERGENT_SNAPSHOTS.contains(&name.as_str()) {
                continue;
            }
            let bytes = render_fixture(fixture.qmd, fixture.resources, format);
            let extraction = extract(&bytes, format);
            insta::assert_snapshot!(name, extraction.to_string());
        }
    }
}

/// T11.4: the shared naming function's lookup name equals the capture
/// xtask's write name for all 20 pairs, because both sides call the exact
/// same `golden_snapshot_name` — never a second, independently-derived
/// name (T10.3's whole point, restated on the read side).
#[test]
fn test_lookup_name_matches_capture_write_name_for_all_pairs() {
    assert_eq!(
        FIXTURES.len(),
        10,
        "fixture manifest must have exactly 10 entries"
    );
    for fixture in FIXTURES {
        for format in ["docx", "pptx"] {
            let read_name = golden_snapshot_name(fixture.qmd, format);
            let write_name = golden_snapshot_name(fixture.qmd, format);
            assert_eq!(
                read_name, write_name,
                "the read-side and write-side name must be produced by literally the same call"
            );
            let snap_path = snapshots_dir().join(format!("{read_name}.snap"));
            assert!(
                snap_path.exists(),
                "expected a committed snapshot at {} for fixture {} ({format}) — \
                 run `cargo xtask capture-pandoc-goldens` if it is genuinely missing",
                snap_path.display(),
                fixture.qmd
            );
        }
    }
}

/// T11.6: the mermaid fixture specifically. Its golden comparison is a
/// known accepted divergence (see [`ACCEPTED_DIVERGENT_SNAPSHOTS`]), so
/// this test binds a narrower shape instead: the mermaid cell's diagram
/// source text ("This would have been a problem.", per the fixture's own
/// `%%| echo: true`) must still survive as body content in Q2's docx
/// output, even though Q2 does not rasterize the diagram itself.
///
/// This assertion's expected value *is* the divergence, so it cannot
/// discriminate a change in mermaid handling that still leaves the source
/// text present (see the plan's vacuity check for this row) — it exists
/// only to guard that the divergence stays labeled and reviewable rather
/// than silently drifting into something else entirely (e.g. the cell
/// vanishing).
#[test]
fn test_mermaid_fixture_preserves_diagram_source_text() {
    let bytes = render_fixture("smoke-all/mermaid/backticks.qmd", &[], "docx");
    let extraction = extract(&bytes, "docx");
    let text = extraction.to_string();
    assert!(
        text.contains("This would have been a problem."),
        "expected the mermaid cell's echoed source text to survive as body content, got:\n{text}"
    );
}

/// T11.5: every snapshot named in [`ACCEPTED_DIVERGENT_SNAPSHOTS`] has a
/// matching entry in `DIVERGENCES.md`, and every `DIVERGENCES.md` entry
/// names a snapshot that actually exists — so `cargo insta review` cannot
/// quietly convert a "Q1-parity" snapshot into an unlabeled "Q2 baseline"
/// (deleting the ledger entry while leaving the snapshot un-asserted would
/// redden this test, not silently pass).
#[test]
fn test_divergence_ledger_is_complete_and_accurate() {
    let ledger = std::fs::read_to_string(divergences_ledger_path())
        .expect("failed to read tests/fixtures/pandoc-goldens/DIVERGENCES.md");

    for snapshot_name in ACCEPTED_DIVERGENT_SNAPSHOTS {
        assert!(
            ledger.contains(snapshot_name),
            "DIVERGENCES.md must name every accepted-divergent snapshot; missing {snapshot_name}"
        );
        let snap_path = snapshots_dir().join(format!("{snapshot_name}.snap"));
        assert!(
            snap_path.exists(),
            "DIVERGENCES.md-adjacent list names {snapshot_name}, but no such snapshot exists at {}",
            snap_path.display()
        );
    }

    // Every fenced snapshot-name-looking token the ledger names on a
    // "Snapshot(s):" line must itself be one of the accepted-divergent
    // names above — catches a stale ledger entry surviving after its
    // snapshot was removed from the accepted list. Tolerates the
    // markdown list/bold decoration around the label
    // (`- **Snapshot(s):**`).
    for line in ledger.lines() {
        let Some((_, rest)) = line.split_once("Snapshot(s):") else {
            continue;
        };
        let rest = rest.trim_start().trim_start_matches("**");
        for token in rest.split(',') {
            let name = token.trim().trim_matches('`');
            if name.is_empty() {
                continue;
            }
            assert!(
                ACCEPTED_DIVERGENT_SNAPSHOTS.contains(&name),
                "DIVERGENCES.md names snapshot `{name}` but it is not in \
                 ACCEPTED_DIVERGENT_SNAPSHOTS — either the ledger is stale or the \
                 code-side list needs updating"
            );
        }
    }
}

/// T11.3: number-identity, independently of the post-filter-AST check
/// P6 companion Task 5 already does (`crossref_number_parity.rs`) — this
/// row checks the number survives all the way to the **rendered OOXML**,
/// where a number can be correct in the AST and still be lost by the
/// writer (the `<m:oMath>` case is exactly this shape).
///
/// Adapted from the plan's illustrative `(Figure 1, Note 1, Theorem 1)`
/// example to the numbers these three real quarto-cli-sourced fixtures
/// actually carry: fixture 1 (`crossrefs/all-docx.qmd`) has one Figure;
/// fixture 3 (`crossrefs/callouts.qmd`) has two Figures (one nested
/// inside a callout, the case this fixture exists to exercise — verified
/// empirically, this fixture's real content has no numbered callout, so
/// there is no "Note" number to check); fixture 4
/// (`crossrefs/theorems.qmd`) has one Theorem.
///
/// **Figure gets a digit-only comparison, same as Theorem.** The
/// original intent here was a byte-wise, separator-included check (Q1's
/// docx Lua and Q2's own crossref *link* text both join "Figure" with
/// U+00A0) to catch an extractor that over-normalizes NBSP to a plain
/// space. That assumption doesn't hold for the caption text specifically:
/// `crossref_render::prefix_caption` (mirrored byte-for-byte in the
/// preview renderer's `FloatRefTarget.tsx`, with its own explicit
/// "ASCII space, NOT NBSP" test) has always used a plain space between
/// kind and number for Figure/Table captions, independently of the NBSP
/// used everywhere else for the same kind+number pairing. bd-n3sark9b
/// (2026-09-17) fixed a real bug where that caption prefix was silently
/// dropped for `Plain`-typed captions (native `![cap](img){#fig-x}`
/// figures); once fixed, the caption's plain-space text — not the link's
/// NBSP'd text — becomes the first "Figure" occurrence in the HTML output,
/// which is what forced this comparison to widen. Changing
/// `prefix_caption`'s separator to NBSP would fix the asymmetry, but it's
/// a cross-language change (Rust + the preview renderer, each with their
/// own tests) outside this branch's scope; see bd-n3sark9b's own history
/// for why the two paths still disagree. Theorem's separator legitimately
/// differs by design too (docx: plain space via `theorem.lua:282-283`'s
/// `pandoc.Space()`; html: NBSP) — comparing full bytes there would
/// falsely redden on presentation alone, so Theorem gets the digit-only
/// comparison, matching `crossref_number_parity.rs`'s own established
/// convention. Figure now follows the same pattern for the same reason:
/// a presentational separator difference, not a numbering regression.
#[test]
fn test_docx_and_html_legs_agree_on_numbers() {
    let byte_wise_cases: &[(&str, &[&str], &[u32])] = &[
        (
            // Two occurrences: the figure's own caption ("Figure 1:
            // Elephant") and the body's "See Figure 1 for an
            // illustration" cross-reference.
            "crossrefs/all-docx.qmd",
            &["crossrefs/img/thinker.jpg"],
            &[1, 1],
        ),
        (
            "crossrefs/callouts.qmd",
            &["crossrefs/img/painter.jpg", "crossrefs/img/abbas.jpg"],
            &[1, 2],
        ),
    ];
    for (qmd, resources, expected_numbers) in byte_wise_cases {
        let docx_bytes = render_fixture(qmd, resources, "docx");
        let docx_text = extract(&docx_bytes, "docx").to_string();

        let docx_fields = separator_and_digits_after(&docx_text, "Figure");
        assert_eq!(
            docx_fields.iter().map(|f| f.number).collect::<Vec<_>>(),
            *expected_numbers,
            "{qmd}: expected docx Figure numbers {expected_numbers:?}, got \
             {docx_fields:?} from:\n{docx_text}"
        );

        // `crossrefs/callouts.qmd` has no `@fig-...` cross-reference in
        // its body and no literal "Figure N" text in its HTML
        // `<figcaption>` (the caption number is not rendered as literal
        // text on the HTML leg at all — see the comment below) — so
        // there is *no* html-side textual occurrence of "Figure" to
        // compare against for this fixture. Its docx-side digit check
        // above is the only assertion this fixture can carry; skip the
        // html byte-wise comparison rather than fabricate a comparison
        // point that doesn't exist on both sides.
        if *qmd == "crossrefs/callouts.qmd" {
            continue;
        }

        let html_bytes = render_fixture(qmd, resources, "html");
        let html_text = String::from_utf8(html_bytes).expect("html output must be UTF-8");
        let html_fields = separator_and_digits_after(&html_text, "Figure");

        // Compare only the first occurrence's *digit*, not the full byte
        // sequence (see the module doc: the caption's separator and the
        // link's separator are independently, presentationally styled —
        // digit agreement is what actually matters here). The first
        // occurrence on each side is still the same referent (figure
        // `#fig-elephant`'s own number).
        let docx_first = docx_fields
            .first()
            .unwrap_or_else(|| panic!("{qmd}: expected at least one docx Figure occurrence"))
            .number;
        let html_first = html_fields
            .first()
            .unwrap_or_else(|| panic!("{qmd}: expected at least one html Figure occurrence"))
            .number;
        assert_eq!(
            docx_first, html_first,
            "{qmd}: docx's first Figure number {docx_first} must equal html's {html_first} \
             (a mismatch here means a real numbering regression)\ndocx text:\n{docx_text}\n\
             html text:\n{html_text}"
        );
    }

    let docx_bytes = render_fixture("crossrefs/theorems.qmd", &[], "docx");
    let docx_text = extract(&docx_bytes, "docx").to_string();
    let html_bytes = render_fixture("crossrefs/theorems.qmd", &[], "html");
    let html_text = String::from_utf8(html_bytes).expect("html output must be UTF-8");
    let docx_numbers: Vec<u32> = separator_and_digits_after(&docx_text, "Theorem")
        .iter()
        .map(|f| f.number)
        .collect();
    let html_numbers: Vec<u32> = separator_and_digits_after(&html_text, "Theorem")
        .iter()
        .map(|f| f.number)
        .collect();
    assert_eq!(
        docx_numbers,
        vec![1, 1],
        "expected docx Theorem number 1 at both its definition and its \
         \"See Theorem 1.\" cross-reference, got from:\n{docx_text}"
    );
    assert_eq!(
        docx_numbers, html_numbers,
        "Theorem digit must agree between docx ({docx_text:?}) and html ({html_text:?}) \
         (digit only — the separator legitimately differs by design)"
    );
}

/// One `kind<separator><digits>` occurrence: the parsed number. Every
/// comparison in this file is digit-only — the separator between `kind`
/// and the digits legitimately differs by design across contexts (see
/// `test_docx_and_html_legs_agree_on_numbers`'s module doc).
#[derive(Debug)]
struct NumberField {
    number: u32,
}

fn separator_and_digits_after(text: &str, kind: &str) -> Vec<NumberField> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(idx) = rest.find(kind) {
        let after = &rest[idx + kind.len()..];
        let sep_len = after
            .chars()
            .next()
            .filter(|c| *c == '\u{a0}' || *c == ' ')
            .map_or(0, |c| c.len_utf8());
        let digits_start = &after[sep_len..];
        let digits: String = digits_start
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        rest = &after[sep_len + digits.len()..];
        if digits.is_empty() {
            continue;
        }
        let Ok(number) = digits.parse::<u32>() else {
            continue;
        };
        out.push(NumberField { number });
    }
    out
}
