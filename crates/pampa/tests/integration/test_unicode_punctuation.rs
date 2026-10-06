/*
 * test_unicode_punctuation.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Tests that every non-ASCII punctuation (`\p{P}`) and symbol (`\p{S}`)
 * code point is prose content: the qmd reader folds it into the
 * surrounding `Str`, as Pandoc does, and the qmd writer re-emits it
 * verbatim. q2 reserves no non-ASCII punctuation as syntax.
 *
 * The grammar used to enumerate Po/Pc/Sm/Sk/Sc ranges by hand and missed
 * Ps/Pe entirely, so `⟨`, `⌈`, `「`, `（` were uncoded parse errors.
 *
 * Background and the full-sweep tool: see
 *   claude-notes/plans/2026-09-25-unicode-punctuation-coverage.md
 * Braid: bd-angle-bracket-u27e8-parse-error-r6l55zmh.
 */

use pampa::readers;
use pampa::writers;

/// A curated sample: a few code points from each family that used to fail,
/// plus representatives of families that already worked (regression).
const NON_ASCII_PUNCT_SYMBOLS: &[char] = &[
    // Ps/Pe: math brackets
    '⟨', '⟩', '⟦', '⟧', '⟪', '⟫', '⌈', '⌉', '⌊', '⌋', '〈', '〉', '⦃', '⦄',
    // Ps/Pe: superscript/subscript parentheses, ornaments
    '⁽', '⁾', '₍', '₎', '❨', '❩', '⁅', '⁆', // Ps/Pe: CJK brackets and full-width forms
    '「', '」', '『', '』', '【', '】', '〈', '〉', '《', '》', '〔', '〕', '（', '）', '［', '］',
    '｛', '｝', '｢', '｣', '〝', '〞', '〟', // Pi/Pf: editorial brackets
    '⸂', '⸃', '⸄', '⸅', '⸜', '⸝', '⸠', '⸡',
    // Po added in Unicode 14 (missed by the enumerated table)
    '⹓', '⹔', // Sc hole in the enumerated currency list
    '₰', // Families that already worked (regression)
    '§', '¶', '†', '•', '…', '—', '±', '→', '€', '©', '¨', '«', '»',
];

fn parse_to_native(input: &str) -> String {
    let mut output = Vec::new();
    let (doc, ctx, _warnings) =
        readers::qmd::read(input.as_bytes(), false, "test.qmd", &mut output, true, None)
            .unwrap_or_else(|diagnostics| {
                let mut source_context = quarto_source_map::SourceContext::new();
                source_context.add_file("test.qmd".to_string(), Some(input.to_string()));
                let rendered: Vec<String> = diagnostics
                    .iter()
                    .map(|d| d.to_text(Some(&source_context)))
                    .collect();
                panic!(
                    "qmd reader failed to parse {input:?}:\n{}",
                    rendered.join("\n")
                )
            });
    let mut buf = Vec::new();
    writers::native::write(&doc, &ctx, &mut buf).expect("native writer failed");
    String::from_utf8(buf).expect("native output is not valid UTF-8")
}

fn round_trip_to_qmd(input: &str) -> String {
    let mut output = Vec::new();
    let (doc, _ctx, _warnings) =
        readers::qmd::read(input.as_bytes(), false, "test.qmd", &mut output, true, None)
            .expect("qmd reader failed");
    let mut buf = Vec::new();
    writers::qmd::write(&doc, &mut buf).expect("qmd writer failed");
    String::from_utf8(buf).expect("qmd output is not valid UTF-8")
}

#[test]
fn strand_repros_parse_as_str() {
    // Pandoc 3.11: [Str "a", Space, Str "\10216b"]
    assert_eq!(
        parse_to_native("a ⟨b\n"),
        r#"[ Para [Str "a", Space, Str "⟨b"] ]"#
    );
    // Seen in claude-notes/plans/2026-08-20-provenance-2-consumers.md.
    assert_eq!(
        parse_to_native("Revert ⟨hunk⟩ then RED\n"),
        r#"[ Para [Str "Revert", Space, Str "⟨hunk⟩", Space, Str "then", Space, Str "RED"] ]"#
    );
}

#[test]
fn cjk_bracketed_text_is_one_str() {
    assert_eq!(
        parse_to_native("「引用」（注）【見出し】\n"),
        r#"[ Para [Str "「引用」（注）【見出し】"] ]"#
    );
}

#[test]
fn non_ascii_punct_and_symbols_fold_into_str() {
    for &c in NON_ASCII_PUNCT_SYMBOLS {
        // Token start after a space, mid-word, and at the end of a word.
        let input = format!("a {c}b a{c}b a{c} b\n");
        let expected = format!(
            r#"[ Para [Str "a", Space, Str "{c}b", Space, Str "a{c}b", Space, Str "a{c}", Space, Str "b"] ]"#
        );
        assert_eq!(
            parse_to_native(&input),
            expected,
            "U+{:04X} {c:?}",
            c as u32
        );
    }
}

#[test]
fn non_ascii_punct_and_symbols_round_trip() {
    // The invariant is AST stability, not byte identity: the qmd writer
    // spells smart typography (`…`, `—`, `’`) in its ASCII source form, as
    // Pandoc's markdown writer does, and the reader converts it back.
    for &c in NON_ASCII_PUNCT_SYMBOLS {
        let input = format!("a {c}b a{c}b a{c} b\n");
        let written = round_trip_to_qmd(&input);
        assert_eq!(
            parse_to_native(&written),
            parse_to_native(&input),
            "U+{:04X} {c:?}: wrote {written:?}",
            c as u32
        );
        // Everything except smart typography is emitted verbatim.
        if !matches!(c, '…' | '—') {
            assert_eq!(written, input, "U+{:04X} {c:?}", c as u32);
        }
    }
}

#[test]
fn ascii_markup_is_still_markup() {
    // The non-ASCII class must not leak ASCII delimiters into Str.
    let native = parse_to_native("*x* [y]{.z}\n");
    assert!(native.contains("Emph"), "{native}");
    assert!(native.contains("Span"), "{native}");
}
