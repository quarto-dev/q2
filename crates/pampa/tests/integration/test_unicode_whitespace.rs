/*
 * test_unicode_whitespace.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Tests that the qmd parser accepts non-ASCII bytes (Unicode whitespace and
 * other non-ASCII content) inside inline text runs, matching Pandoc's
 * behavior of folding such bytes into the surrounding `Str` node rather
 * than treating them as whitespace or as parse errors.
 *
 * Background and policy: see
 *   claude-notes/plans/2026-04-30-unicode-whitespace-handling.md
 * Braid: bd-rmx3 (bug), bd-8oe4 (workspace-wide audit).
 */

use pampa::readers;
use pampa::writers;

/// Run the qmd reader on `input_bytes` and return the native-AST string,
/// or panic with a useful message including any diagnostics.
fn parse_to_native(input_bytes: &[u8]) -> String {
    let mut output = Vec::new();
    let (doc, ctx, _warnings) =
        match readers::qmd::read(input_bytes, false, "test.qmd", &mut output, true, None) {
            Ok(t) => t,
            Err(diagnostics) => {
                let mut source_context = quarto_source_map::SourceContext::new();
                source_context.add_file(
                    "test.qmd".to_string(),
                    Some(String::from_utf8_lossy(input_bytes).into_owned()),
                );
                let rendered: Vec<String> = diagnostics
                    .iter()
                    .map(|d| d.to_text(Some(&source_context)))
                    .collect();
                panic!(
                    "qmd reader failed to parse input:\n{}\n--- diagnostics ---\n{}",
                    String::from_utf8_lossy(input_bytes),
                    rendered.join("\n")
                );
            }
        };

    let mut buf = Vec::new();
    writers::native::write(&doc, &ctx, &mut buf).expect("native writer failed");
    String::from_utf8(buf).expect("native output is not valid UTF-8")
}

#[test]
fn u202f_in_claude_timestamp_paste_parses_as_str_content() {
    // Repro from a Claude.ai web UI conversation transcript pasted into a
    // qmd document. The space between "10:18" and "AM" is U+202F NARROW
    // NO-BREAK SPACE (bytes e2 80 af), not U+0020.
    //
    // Pandoc 3.9.0.2 (markdown reader) produces:
    //   [ Para
    //       [ Str "You"
    //       , Space
    //       , Str "(Apr"
    //       , Space
    //       , Str "16,"
    //       , Space
    //       , Str "2026,"
    //       , Space
    //       , Str "10:18\8239AM)"
    //       ]
    //   ]
    // i.e. the U+202F is folded into the trailing Str — it is *not*
    // tokenized as whitespace and does *not* produce a Space node.
    let input: &[u8] = b"You (Apr 16, 2026, 10:18\xe2\x80\xafAM)\n";

    let native = parse_to_native(input);

    // The fingerprint of correct behavior is that U+202F survives inside a
    // Str token, adjacent to "AM)". Pampa's native writer emits the
    // codepoint literally rather than as a `\8239` escape, so we look for
    // the literal UTF-8 sequence inside a Str.
    let expected_substring = "Str \"10:18\u{202f}AM)\"";
    assert!(
        native.contains(expected_substring),
        "expected native AST to contain {:?}, got:\n{}",
        expected_substring,
        native
    );

    // And no spurious Space node should have been emitted in place of the
    // U+202F: the AST must not contain "Str \"10:18\"" (which would
    // indicate the U+202F was treated as a whitespace separator).
    assert!(
        !native.contains("Str \"10:18\""),
        "U+202F was incorrectly tokenized as whitespace; got:\n{}",
        native
    );
}

/// The Unicode `White_Space=Yes` codepoints that are *not* ASCII
/// whitespace. These are the ones whose handling has been classified
/// per Pandoc 3.9.0.2 behavior — see plan doc, "Pandoc 3.9.0.2
/// experiment" section. ASCII whitespace (U+0009, U+000A, U+000B,
/// U+000C, U+000D, U+0020) is excluded because it already has
/// established meaning in the qmd grammar.
///
/// U+0085 (NEXT LINE) is intentionally excluded from this list: it
/// is a C1 control character, and we have not characterised its
/// behavior in Pandoc as part of the experiment table. If we
/// subsequently want to support it, add it here and add a Pandoc
/// experiment row to the plan.
const NON_ASCII_WHITESPACE: &[char] = &[
    '\u{00A0}', // NO-BREAK SPACE
    '\u{1680}', // OGHAM SPACE MARK
    '\u{2000}', // EN QUAD
    '\u{2001}', // EM QUAD
    '\u{2002}', // EN SPACE
    '\u{2003}', // EM SPACE
    '\u{2004}', // THREE-PER-EM SPACE
    '\u{2005}', // FOUR-PER-EM SPACE
    '\u{2006}', // SIX-PER-EM SPACE
    '\u{2007}', // FIGURE SPACE
    '\u{2008}', // PUNCTUATION SPACE
    '\u{2009}', // THIN SPACE
    '\u{200A}', // HAIR SPACE
    '\u{2028}', // LINE SEPARATOR
    '\u{2029}', // PARAGRAPH SEPARATOR
    '\u{202F}', // NARROW NO-BREAK SPACE
    '\u{205F}', // MEDIUM MATHEMATICAL SPACE
    '\u{3000}', // IDEOGRAPHIC SPACE
];

/// For a single codepoint and a single test position, build the input
/// bytes and assert pampa's native AST matches the per-position
/// expectation. The expectation strings are *substring* checks against
/// pampa's native writer output.
fn assert_native_contains(label: &str, input: String, expected_substring: &str) {
    let native = parse_to_native(input.as_bytes());
    assert!(
        native.contains(expected_substring),
        "[{label}] expected native AST to contain {expected:?}\ninput bytes: {input_bytes:x?}\nfull native output:\n{native}",
        label = label,
        expected = expected_substring,
        input_bytes = input.as_bytes(),
        native = native,
    );
}

#[test]
fn non_ascii_whitespace_inside_word_stays_in_str() {
    // Per Pandoc: `aXb` with X = any non-ASCII whitespace codepoint
    // tokenizes as a single `Str "aXb"`. Verify for every codepoint in
    // the experiment table.
    for &cp in NON_ASCII_WHITESPACE {
        let input = format!("a{cp}b\n");
        let expected = format!("Str \"a{cp}b\"");
        assert_native_contains(&format!("U+{:04X} mid-word", cp as u32), input, &expected);
    }
}

#[test]
fn non_ascii_whitespace_in_ascii_spaced_context_is_standalone_str() {
    // Per Pandoc: `a X b` (with ASCII spaces around X) tokenizes as
    // `Str "a", Space, Str "X", Space, Str "b"` — the codepoint
    // becomes its own Str rather than being absorbed into the Spaces.
    for &cp in NON_ASCII_WHITESPACE {
        let input = format!("a {cp} b\n");
        let expected = format!("Str \"{cp}\"");
        assert_native_contains(
            &format!("U+{:04X} ASCII-spaced", cp as u32),
            input,
            &expected,
        );
    }
}

#[test]
fn non_ascii_whitespace_alone_on_line_is_str_in_para() {
    // Per Pandoc: a line containing only the codepoint produces
    // `Para [Str "X"]` — the line is *not* a blank line.
    for &cp in NON_ASCII_WHITESPACE {
        let input = format!("{cp}\n");
        let expected = format!("Str \"{cp}\"");
        assert_native_contains(
            &format!("U+{:04X} alone on line", cp as u32),
            input,
            &expected,
        );
    }
}

#[test]
fn non_ascii_whitespace_with_crlf_line_endings() {
    // Cross-platform: `aXb\r\n` should parse the same as `aXb\n`.
    // bd-ylig (CRLF pipe-table fix) is a recent reminder that
    // CRLF + scanner-edge-cases need explicit coverage.
    for &cp in NON_ASCII_WHITESPACE {
        let input = format!("a{cp}b\r\n");
        let expected = format!("Str \"a{cp}b\"");
        assert_native_contains(&format!("U+{:04X} CRLF", cp as u32), input, &expected);
    }
}

#[test]
fn line_of_only_nbsp_does_not_separate_paragraphs() {
    // Negative test for (b) classification of blank-line detection in
    // scanner.c: a line of just U+00A0 between two ASCII paragraphs
    // must NOT act as a paragraph separator. Per Pandoc, the three
    // lines collapse into a single Para joined by SoftBreaks.
    let input = "a\n\u{00A0}\nb\n";
    let native = parse_to_native(input.as_bytes());

    // We should see a single Para, with the U+00A0 as Str content and
    // SoftBreaks (not paragraph breaks) joining the lines.
    let para_count = native.matches("Para").count();
    assert_eq!(
        para_count, 1,
        "expected exactly one Para; got {} in:\n{}",
        para_count, native
    );
    assert!(
        native.contains("Str \"\u{00A0}\""),
        "U+00A0 should appear as standalone Str content, got:\n{}",
        native
    );
}

/// Parse `input_bytes` through pampa and re-emit qmd, returning the
/// emitted bytes. Panics on any error.
fn round_trip_to_qmd(input_bytes: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();
    let (doc, _ctx, _warnings) =
        readers::qmd::read(input_bytes, false, "test.qmd", &mut output, true, None)
            .expect("qmd reader failed");
    let mut buf = Vec::new();
    writers::qmd::write(&doc, &mut buf).expect("qmd writer failed");
    buf
}

#[test]
fn non_ascii_whitespace_round_trips_byte_for_byte() {
    // The Pandoc-compat policy says non-ASCII whitespace is content,
    // not whitespace. The corollary is that the qmd writer must emit
    // those bytes verbatim — no escaping, no normalization.
    //
    // The U+202F Claude-timestamp repro is the headline case: the
    // input was a real-world paste where the user expects the file
    // they save to be the file they typed.
    let input: &[u8] = b"You (Apr 16, 2026, 10:18\xe2\x80\xafAM)\n";
    let output = round_trip_to_qmd(input);
    assert_eq!(
        output, input,
        "expected byte-identical round-trip\ninput:  {:x?}\noutput: {:x?}",
        input, output
    );

    // And every codepoint from the experiment table mid-word.
    for &cp in NON_ASCII_WHITESPACE {
        let input = format!("a{cp}b\n").into_bytes();
        let output = round_trip_to_qmd(&input);
        assert_eq!(
            output, input,
            "U+{:04X}: expected byte-identical round-trip\ninput:  {:x?}\noutput: {:x?}",
            cp as u32, input, output
        );
    }
}

#[test]
fn multiple_non_ascii_whitespace_in_one_word_all_stay_in_str() {
    // From the Pandoc experiment: `a<U+00A0>b<U+202F>c<U+3000>d`
    // tokenizes as `Str "a\160b\8239c\12288d"` — all three codepoints
    // are content, no Spaces are produced.
    let input = "a\u{00A0}b\u{202F}c\u{3000}d\n";
    let expected = "Str \"a\u{00A0}b\u{202F}c\u{3000}d\"";
    assert_native_contains("multi-codepoint word", input.to_string(), expected);
}
