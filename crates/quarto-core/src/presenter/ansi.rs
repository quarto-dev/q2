//! ANSI escapes in output text (OPR-7g).
//!
//! The record keeps raw output bytes; stripping is a presentation step. Every piece of
//! printed text the presenter writes (streams, `text/plain`, errors) goes through
//! [`output_text`], so a later colorizing presenter (`bd-18410csp`) has one place to
//! land its HTML branch.

use std::borrow::Cow;
use std::sync::LazyLock;

use regex::Regex;

/// What counts as an escape. The same pattern, and the same test table
/// (`tests/fixtures/ansi/strip-cases.json`), as `stripAnsiCode` in
/// `ts-packages/quarto-api/src/jupyter/to-markdown.ts`: CSI (parameter, intermediate and one
/// final byte), OSC up to BEL or `ESC \` (so an OSC 8 hyperlink goes whole) and the two-byte
/// charset selectors. A lone ESC, or an escape cut by a line break, is not an escape.
static ANSI: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?:[\x1b\u{9b}]\[[0-?]*[ -/]*[@-~])|(?:\x1b\][^\x07\x1b]*(?:\x07|\x1b\\))|(?:\x1b[()][0-9A-Za-z])",
    )
    .expect("valid regex")
});

/// Remove every ANSI escape from `s`.
pub fn strip_ansi(s: &str) -> Cow<'_, str> {
    ANSI.replace_all(s, "")
}

/// The one place printed output text (streams, `text/plain`, error text) is prepared.
///
/// The record keeps raw bytes; this is where presentation decides. `keeps_engine_text` is
/// true for a cell that carries resolved knitr options (R wrote the text; knitr leaves its
/// escapes as they are, and the corpus pins that). Every other cell is stripped. This is an
/// origin test, not a format policy: a colorizing HTML branch (`bd-18410csp`) lands here, and
/// the other formats keep stripping. An R cell imported from an Ark record carries no
/// resolved options, so it is stripped.
pub fn output_text(text: &str, keeps_engine_text: bool) -> Cow<'_, str> {
    if keeps_engine_text {
        Cow::Borrowed(text)
    } else {
        strip_ansi(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The table shared with `ts-packages/quarto-api` (`stripAnsiCode`).
    #[test]
    fn ansi_strip_matches_the_shared_table() {
        let table: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/ansi/strip-cases.json"))
                .unwrap();
        let mut bad = Vec::new();
        for case in table.as_array().unwrap() {
            // lint:allow(metadata-as-str) — serde_json `as_str`
            let (name, input, expected) = (
                case["name"].as_str().unwrap(),
                case["input"].as_str().unwrap(),
                case["expected"].as_str().unwrap(),
            );
            let got = strip_ansi(input).into_owned();
            if got != expected {
                bad.push(format!("{name}: got {got:?}, want {expected:?}"));
            }
        }
        assert!(bad.is_empty(), "{}", bad.join("\n"));
    }
}
