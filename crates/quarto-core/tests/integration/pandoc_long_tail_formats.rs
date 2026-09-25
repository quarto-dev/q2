/*
 * tests/integration/pandoc_long_tail_formats.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Long-tail Phase 2 — Tier A bulk tail: per-variant render smoke through
 * `render_document_to_file` (the plan-mandated entry point — the same
 * branch the real render path uses), plus the Textile fresh-baseline
 * snapshot (Q1's `texttile` typo means there is no Q1 behavior to mirror,
 * so the snapshot pins today's output as the baseline, not a parity
 * claim).
 */

use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::render_to_file::{RenderToFileOptions, render_document_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

/// (format name, expected output extension) for all 25 Tier A variants.
const TIER_A: &[(&str, &str)] = &[
    ("odt", "odt"),
    ("opendocument", "xml"),
    ("rtf", "rtf"),
    ("fb2", "fb2"),
    ("plain", "txt"),
    ("rst", "rst"),
    ("org", "org"),
    ("muse", "muse"),
    ("ms", "ms"),
    ("man", "man"),
    ("texinfo", "texinfo"),
    ("tei", "tei"),
    ("zimwiki", "zim"),
    ("dokuwiki", "dokuwiki"),
    ("haddock", "haddock"),
    ("json", "json"),
    ("native", "native"),
    ("icml", "icml"),
    ("jira", "jira"),
    ("mediawiki", "mediawiki"),
    ("xwiki", "xwiki"),
    ("textile", "textile"),
    ("docbook", "xml"),
    ("docbook4", "xml"),
    ("docbook5", "xml"),
];

const FIXTURE_QMD: &str =
    "---\ntitle: Tier A Fixture\n---\n\n# Head\n\nHelloTailBody with *emphasis*.\n";

/// Parametrized smoke: every Tier A variant renders through
/// `render_document_to_file` to a non-empty file with the right
/// extension. Format-specific shape checks ride along in the same loop:
/// odt must be a real zip (`PK\x03\x04` — pandoc's odt writer is a zip
/// writer, so plain-text "success" would still be wrong), and fb2 must
/// be FictionBook XML.
#[test]
fn tier_a_smoke_all_variants() {
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let options = RenderToFileOptions::default();

    for (name, ext) in TIER_A {
        let temp = TempDir::new().unwrap();
        let project_dir = temp.path().canonicalize().unwrap();
        let input_path = project_dir.join("f.qmd");
        std::fs::write(&input_path, FIXTURE_QMD).unwrap();

        let result = render_document_to_file(
            &input_path,
            name,
            &options,
            None,
            runtime.clone(),
            None,
            None,
            None,
        )
        .unwrap_or_else(|e| panic!("render --to {name} failed: {e}"));

        assert_eq!(
            result.output_path.extension().and_then(|e| e.to_str()),
            Some(*ext),
            "output extension for {name}"
        );
        let bytes = std::fs::read(&result.output_path)
            .unwrap_or_else(|e| panic!("output for {name} should exist: {e}"));
        assert!(!bytes.is_empty(), "output for {name} must be non-empty");

        if *name == "odt" {
            assert_eq!(&bytes[..4], b"PK\x03\x04", "odt output must be a zip");
        }
        if *name == "fb2" {
            let text = String::from_utf8_lossy(&bytes);
            assert!(
                text.starts_with("<?xml"),
                "fb2 output must start with an XML declaration: {:.80}",
                text
            );
            assert!(
                text.contains("<FictionBook"),
                "fb2 output must contain the FictionBook root element: {:.80}",
                text
            );
        }
    }
}

/// Q1 has no textile baseline to mirror (`formats.ts` spells it
/// `texttile`, so Q1 falls through to `unknownFormat("txt")`). This
/// snapshot pins **today's** q2 output as the regression baseline — a
/// fresh baseline, not a parity claim. The fixture is rich enough
/// (heading, emphasis, list, code block) that a writer regression can't
/// hide in body-only prose.
#[test]
fn textile_fresh_baseline_snapshot() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().canonicalize().unwrap();
    let input_path = project_dir.join("textile.qmd");
    std::fs::write(
        &input_path,
        "---\ntitle: Textile Fixture\n---\n\n# Head\n\nHello *emphasis* and **strong**.\n\n- first\n- second\n\n```python\nx = 1 + 1\n```\n",
    )
    .unwrap();

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result = render_document_to_file(
        &input_path,
        "textile",
        &RenderToFileOptions::default(),
        None,
        runtime,
        None,
        None,
        None,
    )
    .expect("textile render should succeed");

    let output = std::fs::read_to_string(&result.output_path).expect("textile output readable");
    insta::assert_snapshot!(output);
}

/// Q1 renders an escaped shortcode (`{{{< meta title >}}}`) to
/// markdown-family output as literal `{{< meta title >}}` text: q2's parser
/// produces an escaped `Shortcode`, `shortcode_resolve` preserves it as a
/// literal Str, and then pandoc's markdown-family **writers re-escape the
/// braces** (measured, pandoc 3.11 `-t gfm` on a Str containing
/// `{{< meta title >}}` writes `{{\< meta title \>}}`). Q1 undoes that with
/// the shortcode-unescape postprocessor (`format-markdown.ts:21`, applied for
/// every `isMarkdownOutput` flavor in `pandoc.ts:968-973`); without it the
/// output double-escapes and no longer shows the literal shortcode the
/// author asked for.
#[test]
fn gfm_shortcode_round_trip() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().canonicalize().unwrap();
    let input_path = project_dir.join("esc.qmd");
    std::fs::write(
        &input_path,
        "---\ntitle: RoundTrip Title\n---\n\n# Head\n\nEscaped: {{{< meta title >}}} stays literal.\n",
    )
    .unwrap();

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result = render_document_to_file(
        &input_path,
        "gfm",
        &RenderToFileOptions::default(),
        None,
        runtime,
        None,
        None,
        None,
    )
    .expect("gfm render should succeed");

    let md = std::fs::read_to_string(&result.output_path).expect("gfm output readable");
    assert!(
        md.contains("{{< meta title >}}"),
        "escaped shortcode must come out as literal {{< … >}} text: {md}"
    );
    assert!(
        !md.contains("{{\\<"),
        "writer-escaped open delimiter must be unescaped: {md}"
    );
    assert!(
        !md.contains("\\>}}"),
        "writer-escaped close delimiter must be unescaped: {md}"
    );
}
