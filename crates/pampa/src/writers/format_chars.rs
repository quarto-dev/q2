/*
 * format_chars.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Unicode *format* characters (general category `Cf`) and the character
//! references the qmd writer spells them with.
//!
//! Format characters are invisible in source (zero-width space, soft hyphen,
//! bidi controls, the MathML invisible operators, tag characters, …). The
//! reader decodes `&ZeroWidthSpace;` / `&#x200B;` into the raw codepoint
//! inside a `Str`; if the writer emitted that codepoint back verbatim the
//! result would be unreadable in an editor and — before bd-wuiu1of7 — a hard
//! parse error, because the grammar's prose regexes rejected every `Cf`
//! character except ZWNJ/ZWJ (GH #672). So the writer re-encodes them.
//!
//! Entity names come from the WHATWG HTML named character references table
//! (<https://html.spec.whatwg.org/multipage/named-characters.html>, machine
//! readable at <https://html.spec.whatwg.org/entities.json>), which is the same
//! table the grammar's `entity_reference` regex and the reader's decoder are
//! generated from (`crates/tree-sitter-qmd/common/html_entities.json`). The
//! spec has no notion of a *preferred* name — several names decode to the same
//! codepoint (five for U+200B) — so the choice among aliases is ours: the
//! HTML 4 legacy name where one exists (`&shy;`, `&lrm;`, `&rlm;`), otherwise
//! the descriptive name rather than a spacing alias (`&ZeroWidthSpace;`, not
//! `&NegativeThinSpace;`; `&ApplyFunction;`, not `&af;`). Format characters
//! with no name get a hexadecimal numeric reference.
//!
//! The source spelling is not preserved: `&af;`, `&ApplyFunction;` and a raw
//! U+2061 all decode to the same `Str`, so the writer cannot tell them apart.
//! Pandoc's markdown writer is lossy the same way (it emits the raw codepoint
//! for all three, or with `--ascii` one arbitrary alias).

/// Every `Cf` codepoint as inclusive ranges, ascending and non-overlapping.
///
/// Generated from the Unicode Character Database; `tests::table_matches_regex_cf`
/// pins it against the `regex` crate's `\p{Cf}` so the two cannot drift apart
/// silently when either is updated.
const FORMAT_CHAR_RANGES: &[(char, char)] = &[
    ('\u{00AD}', '\u{00AD}'),   // SOFT HYPHEN
    ('\u{0600}', '\u{0605}'),   // ARABIC NUMBER SIGN .. ARABIC NUMBER MARK ABOVE
    ('\u{061C}', '\u{061C}'),   // ARABIC LETTER MARK
    ('\u{06DD}', '\u{06DD}'),   // ARABIC END OF AYAH
    ('\u{070F}', '\u{070F}'),   // SYRIAC ABBREVIATION MARK
    ('\u{0890}', '\u{0891}'),   // ARABIC POUND MARK ABOVE .. ARABIC PIASTRE MARK ABOVE
    ('\u{08E2}', '\u{08E2}'),   // ARABIC DISPUTED END OF AYAH
    ('\u{180E}', '\u{180E}'),   // MONGOLIAN VOWEL SEPARATOR
    ('\u{200B}', '\u{200F}'),   // ZERO WIDTH SPACE .. RIGHT-TO-LEFT MARK
    ('\u{202A}', '\u{202E}'),   // LEFT-TO-RIGHT EMBEDDING .. RIGHT-TO-LEFT OVERRIDE
    ('\u{2060}', '\u{2064}'),   // WORD JOINER .. INVISIBLE PLUS
    ('\u{2066}', '\u{206F}'),   // LEFT-TO-RIGHT ISOLATE .. NOMINAL DIGIT SHAPES
    ('\u{FEFF}', '\u{FEFF}'),   // ZERO WIDTH NO-BREAK SPACE
    ('\u{FFF9}', '\u{FFFB}'),   // INTERLINEAR ANNOTATION ANCHOR .. TERMINATOR
    ('\u{110BD}', '\u{110BD}'), // KAITHI NUMBER SIGN
    ('\u{110CD}', '\u{110CD}'), // KAITHI NUMBER SIGN ABOVE
    ('\u{13430}', '\u{1343F}'), // EGYPTIAN HIEROGLYPH VERTICAL JOINER .. END WALLED ENCLOSURE
    ('\u{1BCA0}', '\u{1BCA3}'), // SHORTHAND FORMAT LETTER OVERLAP .. UP STEP
    ('\u{1D173}', '\u{1D17A}'), // MUSICAL SYMBOL BEGIN BEAM .. END PHRASE
    ('\u{E0001}', '\u{E0001}'), // LANGUAGE TAG
    ('\u{E0020}', '\u{E007F}'), // TAG SPACE .. CANCEL TAG
];

/// Preferred WHATWG entity name for the format characters that have one.
/// See the module docs for the alias rule; `tests::preferred_names_decode_to_their_char`
/// pins every entry against the shared entity table.
const PREFERRED_ENTITY_NAMES: &[(char, &str)] = &[
    ('\u{00AD}', "&shy;"),
    ('\u{200B}', "&ZeroWidthSpace;"),
    ('\u{200C}', "&zwnj;"),
    ('\u{200D}', "&zwj;"),
    ('\u{200E}', "&lrm;"),
    ('\u{200F}', "&rlm;"),
    ('\u{2060}', "&NoBreak;"),
    ('\u{2061}', "&ApplyFunction;"),
    ('\u{2062}', "&InvisibleTimes;"),
    ('\u{2063}', "&InvisibleComma;"),
];

/// Is `c` a Unicode format character (general category `Cf`)?
pub(crate) fn is_format_char(c: char) -> bool {
    FORMAT_CHAR_RANGES
        .binary_search_by(|&(lo, hi)| {
            if c < lo {
                std::cmp::Ordering::Greater
            } else if c > hi {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// The character reference the qmd writer spells a format character with:
/// the preferred named entity when one exists, else `&#xXXXX;`.
///
/// Returns `None` for anything that is not a format character. Callers decide
/// which format characters to encode (the qmd writer leaves ZWNJ/ZWJ raw).
pub(crate) fn format_char_reference(c: char) -> Option<String> {
    if !is_format_char(c) {
        return None;
    }
    Some(
        match PREFERRED_ENTITY_NAMES.iter().find(|(fc, _)| *fc == c) {
            Some((_, name)) => (*name).to_string(),
            None => format!("&#x{:X};", c as u32),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_are_ascending_and_non_overlapping() {
        for w in FORMAT_CHAR_RANGES.windows(2) {
            let (_, prev_hi) = w[0];
            let (lo, hi) = w[1];
            assert!(lo <= hi, "range {lo:?}..={hi:?} is inverted");
            assert!(
                prev_hi < lo,
                "ranges {:?} and {:?} overlap or are out of order",
                w[0],
                w[1]
            );
        }
    }

    #[test]
    fn table_matches_regex_cf() {
        // Every scalar value, in one string, so a single regex pass finds the
        // whole category as the `regex` crate's Unicode tables define it.
        let all: String = (0..=0x0010_FFFF_u32).filter_map(char::from_u32).collect();
        let re = regex::Regex::new(r"\p{Cf}").unwrap();
        let from_regex: Vec<char> = re
            .find_iter(&all)
            .flat_map(|m| m.as_str().chars())
            .collect();
        let from_table: Vec<char> = all.chars().filter(|&c| is_format_char(c)).collect();
        assert_eq!(
            from_table, from_regex,
            "FORMAT_CHAR_RANGES drifted from the regex crate's \\p{{Cf}}"
        );
        assert_eq!(from_regex.len(), 170);
    }

    #[test]
    fn preferred_names_decode_to_their_char() {
        let table = crate::pandoc::treesitter_utils::entity_reference::entity_table();
        for &(c, name) in PREFERRED_ENTITY_NAMES {
            assert!(is_format_char(c), "{name} maps to {c:?}, which is not Cf");
            let decoded = table
                .get(name)
                .unwrap_or_else(|| panic!("{name} is not in html_entities.json"));
            assert_eq!(decoded, &c.to_string(), "{name} decodes to something else");
        }
    }

    #[test]
    fn reference_spelling() {
        assert_eq!(
            format_char_reference('\u{200B}').as_deref(),
            Some("&ZeroWidthSpace;")
        );
        assert_eq!(
            format_char_reference('\u{2064}').as_deref(),
            Some("&#x2064;")
        );
        assert_eq!(
            format_char_reference('\u{061C}').as_deref(),
            Some("&#x61C;")
        );
        assert_eq!(
            format_char_reference('\u{E0001}').as_deref(),
            Some("&#xE0001;")
        );
        assert_eq!(format_char_reference('a'), None);
        assert_eq!(format_char_reference('\u{00A0}'), None); // Zs, not Cf
        assert_eq!(format_char_reference('\u{0301}'), None); // Mn, not Cf
    }
}
