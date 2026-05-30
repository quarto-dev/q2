/*
 * test_emphasis_opening_mark.rs
 *
 * Regression test for bd-1lpkx: the Q-2-12 "This is the opening '*' mark."
 * diagnostic detail must point at the '*' delimiter, not at the word that
 * precedes it.
 *
 * Pre-fix, the Q-2-12.json "simple" case captured the preceding text run
 * (content "a *", capture at column 0 = the "a") instead of the
 * emphasis_delimiter token, so find_matching_token resolved the
 * "emphasis-start" note to the most recent word before the error. This
 * test drives the real reader path (the same one the binary uses) and
 * asserts the opening-mark detail lands exactly on the '*'.
 */

use pampa::readers;

/// Parse `input`, expect a Q-2-12 diagnostic, and return the byte
/// start-offset of its "opening '*' mark." detail.
fn opening_star_offset(input: &str) -> usize {
    let result = readers::qmd::read(
        input.as_bytes(),
        false, // not loose
        "t.qmd",
        &mut std::io::sink(),
        true, // prune errors
        None,
    );

    let diagnostics = result.expect_err("expected an unclosed-emphasis error");
    let q2_12 = diagnostics
        .iter()
        .find(|d| d.code.as_deref() == Some("Q-2-12"))
        .unwrap_or_else(|| panic!("expected a Q-2-12 diagnostic, got: {diagnostics:#?}"));

    let opening = q2_12
        .details
        .iter()
        .find(|d| d.content.as_str().contains("opening"))
        .expect("expected an 'opening' detail on the Q-2-12 diagnostic");

    opening
        .location
        .as_ref()
        .expect("opening-mark detail should have a source location")
        .start_offset()
}

#[test]
fn star_opening_mark_points_at_delimiter_not_preceding_word() {
    // "hello world *baz\n": the '*' is at byte 12. Pre-fix the detail
    // landed inside "world" (~byte 6-7).
    assert_eq!(
        opening_star_offset("hello world *baz\n"),
        12,
        "opening '*' mark should point at the '*' (byte 12), not the preceding word"
    );

    // "foo *bar\n": the '*' is at byte 4. Pre-fix the detail landed at
    // byte 0 (the start of "foo").
    assert_eq!(
        opening_star_offset("foo *bar\n"),
        4,
        "opening '*' mark should point at the '*' (byte 4), not the preceding word"
    );
}
