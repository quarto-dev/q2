//! Shared lexical patterns for finding executable cells and inline spans in
//! qmd text (EXR-2 decision 5).
//!
//! The executor's fence scanner (`jupyter/text_execute.rs`), the nested-cell
//! mask (`nested_cell_mask.rs`) and, from Phase 2, the unit scanner import
//! these constants instead of each spelling their own language class, so the
//! scanners cannot drift apart.
//!
//! **Pampa is the authority.** It accepts a language token that starts with a
//! letter and continues with letters, digits, `_`, `.` or `-` (verified
//! 2026-10-29; `{python.marimo}` and `{py-thon}` are single tokens, `{1abc}` and
//! `{_x}` are Q-2-36 errors). [`LANG_TOKEN`] is exactly that class, so a
//! scanner built on it accepts at least every token pampa accepts.

/// The language token inside braces: `[A-Za-z][A-Za-z0-9_.-]*`.
pub(crate) const LANG_TOKEN: &str = r"[A-Za-z][A-Za-z0-9_.-]*";

/// The mask's token class: [`LANG_TOKEN`] widened on the first character to
/// also admit a digit or underscore. The mask must be a superset of every
/// scanner, and pampa's rejection of `{1abc}` is not a reason to leave a
/// hole in the mask.
pub(crate) const MASK_LANG_TOKEN: &str = r"[A-Za-z0-9_][A-Za-z0-9_.-]*";

/// Everything between a language token and the closing brace of a fence
/// opener, up to a line end: the attribute tail (`{python .marimo}`) and any
/// legacy option text. Used by the mask, which preserves it verbatim.
pub(crate) const ATTR_TAIL: &str = r"[^}\r\n]*";

/// The executor's fence regex source: a column-0 three-backtick opener with
/// a bare `{lang}` (no attribute tail), lazily up to a bare closing fence.
/// Capture 1 is the language, capture 2 the code. Quirks are pinned by the
/// `exr2_p0_*` tests in `text_execute.rs`.
#[cfg(test)]
pub(crate) fn executor_fence_pattern() -> String {
    format!(r"(?m)^```\s*\{{({LANG_TOKEN})\}}\s*\n([\s\S]*?)^```\s*$")
}

/// The mask's inline-span regex source. Group 1 is the guard character,
/// group 2 the marker plus expression. The brace spelling accepts
/// [`MASK_LANG_TOKEN`]; the classic `r` spelling is always included.
pub(crate) fn mask_inline_pattern() -> String {
    format!(r"(^|[^`])`((?:\{{{MASK_LANG_TOKEN}\}}|r)[ \t#][^`]+)`")
}

/// The unit scanner's span regex source. Group 1 is the guard (empty at the
/// start of the scanned text, else one non-backtick character), group 2 the
/// language token, group 3 the expression. Brace spelling only: the classic
/// `` `r expr` `` spelling is knitr's private business (EXR-2 decision 2).
pub(crate) fn unit_span_pattern() -> String {
    format!(r"(^|[^`])`\{{({LANG_TOKEN})\}}[ \t]([^`]+)`")
}

/// One line that opens a backtick fence: optional blockquote prefix, then
/// indentation, three or more backticks, then an info string with no
/// backtick. Groups: `prefix`, `indent`, `ticks`, `info` (trimmed).
pub(crate) const FENCE_OPENER_LINE: &str =
    r"^(?P<prefix>(?: {0,3}> ?)*)(?P<indent> *)(?P<ticks>`{3,})[ \t]*(?P<info>[^`]*?)[ \t]*$";

/// One line that can close a backtick fence: the same shape with no info.
pub(crate) const FENCE_CLOSER_LINE: &str = r"^(?:(?: {0,3}> ?)*)(?: *)(?P<ticks>`{3,})[ \t]*$";

/// A braced cell info string: `{lang}` or `{lang tail}`. Groups: `lang`,
/// `tail` (the attribute tail, untrimmed, optional).
pub(crate) fn braced_info_pattern() -> String {
    format!(r"^\{{(?P<lang>{LANG_TOKEN})(?:[ \t]+(?P<tail>[^}}]*))?\}}$")
}

#[cfg(test)]
mod tests {
    use super::*;
    use regex::Regex;

    fn token_re() -> Regex {
        Regex::new(&format!("^{LANG_TOKEN}$")).unwrap()
    }

    #[test]
    fn lang_token_matches_what_pampa_accepts() {
        let re = token_re();
        for ok in [
            "python",
            "Python",
            "py-thon",
            "py.thon",
            "python.marimo",
            "r",
        ] {
            assert!(re.is_match(ok), "{ok}");
        }
        for bad in ["1abc", "_x", "é", "", "py thon", "=html"] {
            assert!(!re.is_match(bad), "{bad}");
        }
    }

    #[test]
    fn mask_token_is_a_superset_on_the_first_character() {
        let t = Regex::new(&format!("^{LANG_TOKEN}$")).unwrap();
        let m = Regex::new(&format!("^{MASK_LANG_TOKEN}$")).unwrap();
        for s in ["python", "py-thon", "python.marimo"] {
            assert!(t.is_match(s) && m.is_match(s));
        }
        for s in ["1abc", "_x"] {
            assert!(!t.is_match(s) && m.is_match(s));
        }
    }
}
