/*
 * crossref/section_number.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Section-number formatting ("1.2", "A.1") for `sec` crossref targets.
 */

//! Port of Q1's `sectionNumber` / `formatChapterIndex`
//! (`resources/pandoc-filters/filters/crossref/format.lua`).
//!
//! The semantics, pinned empirically against Q1 on 2026-09-23
//! (book-projects P0, Amendment A):
//!
//! - The top component is rendered only when `max_heading == 1` — i.e. the
//!   document's shallowest heading is H1, or `crossref.chapters` forced it.
//!   An H2-led document numbers relative to the shallowest level ("1",
//!   "1.1"), because the components above `max_heading` are always zero and
//!   drop out of the appended tail.
//! - The top component is a *letter* for an appendix chapter. Q1 derives
//!   the letter from `file.bookItemNumber`; with Q2's per-file
//!   appendix-local seeding, `section[0]` is exactly that number.
//!   (`chapters-alpha` is deliberately deferred — P0 scope.)
//! - Trailing zero components are trimmed; interior zeros are kept
//!   ("1.0.1" for a skipped level), matching the Lua loop.

/// Format a section path (`[1, 2]` → `"1.2"`) the way Q1's
/// `sectionNumber(section)` does.
///
/// `max_heading` is the document's `CrossrefIndex::max_heading`;
/// `is_appendix` is the per-entry (per-file) appendix flag.
pub fn format_section_number(section: &[u32], max_heading: u32, is_appendix: bool) -> String {
    let mut num = String::new();
    if max_heading == 1 && !section.is_empty() {
        num = format_chapter_index(section[0], is_appendix);
    }
    // Trailing-zero trim: the last index with a non-zero component, or 1
    // (Lua's 1-based `lastIndex`) when every deeper component is zero — in
    // which case the append loop below runs empty.
    let mut last = 1;
    for i in (2..=section.len()).rev() {
        if section[i - 1] > 0 {
            last = i;
            break;
        }
    }
    for i in 2..=last {
        if !num.is_empty() {
            num.push('.');
        }
        num.push_str(&section[i - 1].to_string());
    }
    num
}

/// Port of Q1's `formatChapterIndex`: appendix chapters format their top
/// component as a letter (`1 → "A"`). `chapters-alpha` (letter formatting
/// for *non*-appendix chapters) is deferred — out of P0 scope.
///
/// Beyond 26 appendix chapters Q1's `string.char(64 + n)` drifts out of the
/// alphabet ("[", "\", …); we mirror it literally rather than invent a
/// correction Q1 doesn't have.
fn format_chapter_index(index: u32, is_appendix: bool) -> String {
    if is_appendix {
        char::from_u32(64u32.saturating_add(index))
            .unwrap_or('?')
            .to_string()
    } else {
        index.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h1_led_includes_top_component() {
        assert_eq!(format_section_number(&[1], 1, false), "1");
        assert_eq!(format_section_number(&[1, 2], 1, false), "1.2");
        assert_eq!(format_section_number(&[2, 1, 3], 1, false), "2.1.3");
    }

    #[test]
    fn h2_led_drops_zero_top_component() {
        // max_heading == 2: the always-zero top component never renders.
        assert_eq!(format_section_number(&[0, 1], 2, false), "1");
        assert_eq!(format_section_number(&[0, 1, 1], 2, false), "1.1");
    }

    #[test]
    fn trailing_zeros_trimmed_interior_kept() {
        assert_eq!(format_section_number(&[1, 0], 1, false), "1");
        assert_eq!(format_section_number(&[1, 0, 0], 1, false), "1");
        assert_eq!(format_section_number(&[1, 0, 1], 1, false), "1.0.1");
    }

    #[test]
    fn appendix_formats_top_component_as_letter() {
        assert_eq!(format_section_number(&[1], 1, true), "A");
        assert_eq!(format_section_number(&[2, 1], 1, true), "B.1");
        // The appendix flag only touches the top component; deeper
        // components stay numeric.
        assert_eq!(format_section_number(&[1, 2], 1, true), "A.2");
        // And it is inert when the top component isn't rendered.
        assert_eq!(format_section_number(&[0, 1], 2, true), "1");
    }
}
