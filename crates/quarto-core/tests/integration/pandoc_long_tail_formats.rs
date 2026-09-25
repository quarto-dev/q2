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

// === long-tail Phase 4: Tier C stretch (12 variants) ===

/// (format name, expected output extension) for all 12 Tier C variants.
const TIER_C: &[(&str, &str)] = &[
    ("djot", "dj"),
    ("t2t", "t2t"),
    ("xml", "xml"),
    ("ansi", "txt"),
    ("vimdoc", "txt"),
    ("bbcode", "txt"),
    ("bbcode_steam", "txt"),
    ("bbcode_phpbb", "txt"),
    ("bbcode_fluxbb", "txt"),
    ("bbcode_hubzilla", "txt"),
    ("bbcode_xenforo", "txt"),
    ("chunkedhtml", "zip"),
];

/// The title marker used to pin **bare invocation** at the output level.
/// Q1 gives the Tier C formats no format object (`unknownFormat`), so the
/// invocation must carry no `--standalone` — and that is *not* a no-op:
/// pandoc 3.11 ships default templates for ansi/djot/t2t/bbcode/vimdoc,
/// and with `--standalone` the djot template (measured) prepends the
/// title as `# TierCMarkerTitle`. Bare output carries the body only, so
/// the marker must be absent — a red flag the moment anyone adds
/// standalone chrome to this family.
const TIER_C_TITLE_MARKER: &str = "TierCMarkerTitle";

const TIER_C_FIXTURE_QMD: &str =
    "---\ntitle: TierCMarkerTitle\n---\n\n# Head\n\nHelloTierCBody with *emphasis*.\n";

/// Parametrized smoke: every Tier C variant renders through
/// `render_document_to_file` to a non-empty file with the right
/// extension, **bare invocation** (no template chrome — see
/// TIER_C_TITLE_MARKER; the AST-dump `xml` writer legitimately echoes
/// metadata, so it's exempt from the title check), plus format-specific
/// shape checks: ansi output must carry terminal escape bytes, and
/// chunkedhtml must be a valid zip archive whose `index.html` shell is
/// backed by a chapter entry carrying the body text (the chunked writer
/// splits content into `1-<slug>.html` files).
#[test]
fn tier_c_smoke_all_variants() {
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let options = RenderToFileOptions::default();

    for (name, ext) in TIER_C {
        let temp = TempDir::new().unwrap();
        let project_dir = temp.path().canonicalize().unwrap();
        let input_path = project_dir.join("f.qmd");
        std::fs::write(&input_path, TIER_C_FIXTURE_QMD).unwrap();

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

        let text = String::from_utf8_lossy(&bytes);
        if *name == "chunkedhtml" {
            assert_eq!(
                &bytes[..4],
                b"PK\x03\x04",
                "chunkedhtml output must be a zip"
            );
            let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes[..]))
                .unwrap_or_else(|e| panic!("chunkedhtml zip must parse: {e}"));
            archive
                .by_name("index.html")
                .unwrap_or_else(|e| panic!("zip must contain index.html: {e}"));
            // The chunked writer splits content into numbered chapter
            // files (`1-head.html`) — `index.html` is only the shell —
            // so the body text must appear in *some* entry of the zip.
            let mut found_body = false;
            for i in 0..archive.len() {
                let mut entry = archive.by_index(i).unwrap();
                let mut entry_text = String::new();
                if entry.is_dir()
                    || std::io::Read::read_to_string(&mut entry, &mut entry_text).is_err()
                {
                    continue;
                }
                if entry_text.contains("HelloTierCBody") {
                    found_body = true;
                    break;
                }
            }
            assert!(
                found_body,
                "chunkedhtml zip must carry the body text in some entry"
            );
        } else if *name == "ansi" {
            assert!(
                bytes.windows(2).any(|w| w == b"\x1b["),
                "ansi output must contain terminal escape sequences: {:.80}",
                text
            );
        } else if *name != "xml" {
            // xml is pandoc's native-AST dump: its <Metadata> echoes the
            // title by design. Every real *text* writer emits body only
            // when not standalone (measured across all ten), so a title
            // here means template chrome — the standalone regression.
            assert!(
                !text.contains(TIER_C_TITLE_MARKER),
                "{name} output must not contain the title (bare invocation, no --standalone template): {text:.120}"
            );
            assert!(
                text.contains("HelloTierCBody"),
                "{name} output must contain the body text: {text:.120}"
            );
        }
    }
}

// === Phase 5: Tier D — JS slide formats (s5/dzslides/slidy/slideous) ===
//
// Q1's `createHtmlPresentationFormat` family: standalone HTML decks. All
// four get `--standalone --wrap none`, so (unlike Tier C) the title page
// legitimately appears. Per-format markers measured against real pandoc
// 3.11 standalone output (deck fixture with `# One`/`# Two`):
//   s5       → bundled assets under `s5/default/`
//   dzslides → self-contained inline shim; the literal `dzslides` string
//              appears in the inlined template (no external JS asset to
//              reference)
//   slidy    → W3C CDN `slidy.js`
//   slideous → bundled `slideous/slideous.js`
const TIER_D: &[(&str, &str)] = &[
    ("s5", "html"),
    ("dzslides", "html"),
    ("slidy", "html"),
    ("slideous", "html"),
];

/// The distinguishing per-format marker each deck must reference (see
/// the TIER_D block comment for why dzslides' marker is a literal in the
/// inline shim rather than an asset path).
const TIER_D_JS_MARKER: &[(&str, &str)] = &[
    ("s5", "s5/default/"),
    ("dzslides", "dzslides"),
    ("slidy", "slidy.js"),
    ("slideous", "slideous.js"),
];

const TIER_D_FIXTURE_QMD: &str =
    "---\ntitle: TierDDeckTitle\n---\n\n# One\n\nTierDBodyOne.\n\n# Two\n\nTierDBodyTwo.\n";

/// Parametrized smoke: every Tier D variant renders through
/// `render_document_to_file` to a non-empty `.html` file that is a
/// **deck** — `class="slide…"` structure and the format's own asset/
/// shim marker — carrying the body text. A plain `-t html` document
/// would fail the slide-structure check, pinning the writer mapping.
#[test]
fn tier_d_smoke_all_variants() {
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let options = RenderToFileOptions::default();

    for (name, ext) in TIER_D {
        let temp = TempDir::new().unwrap();
        let project_dir = temp.path().canonicalize().unwrap();
        let input_path = project_dir.join("f.qmd");
        std::fs::write(&input_path, TIER_D_FIXTURE_QMD).unwrap();

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
        let text = String::from_utf8(
            std::fs::read(&result.output_path)
                .unwrap_or_else(|e| panic!("output for {name} should exist: {e}")),
        )
        .unwrap();
        assert!(!text.is_empty(), "output for {name} must be non-empty");

        assert!(
            text.contains("class=\"slide"),
            "{name} output must be a slide deck (class=\"slide\" structure): {:.200}",
            text
        );
        assert!(
            text.contains("TierDBodyOne"),
            "{name} output must contain the body text: {:.200}",
            text
        );
        let marker = TIER_D_JS_MARKER
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, m)| *m)
            .unwrap();
        assert!(
            text.contains(marker),
            "{name} deck must reference its own assets/marker {marker:?}: {:.200}",
            text
        );
    }
}
