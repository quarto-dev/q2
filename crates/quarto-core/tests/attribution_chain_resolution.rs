//! Phase 0 test #8 — `SourceInfo` chain resolution pin.
//!
//! A node whose `SourceInfo` is `Substring(parent=Original{0..20},
//! 5..10)` resolves to file 0, bytes 5..10 *in the original file*,
//! not 5..10 in the substring. This already works for `map_offset`
//! in `quarto-source-map/src/mapping.rs`; pinning it here guards the
//! contract attribution-lookup relies on.

use quarto_source_map::types::{Location, Range};
use quarto_source_map::{SourceContext, SourceInfo};

#[test]
fn map_offset_resolves_substring_chain_to_original_file_bytes() {
    let mut ctx = SourceContext::new();
    let file_id = ctx.add_file(
        "test.qmd".to_string(),
        Some("0123456789ABCDEFGHIJ".to_string()),
    );

    let original = SourceInfo::from_range(
        file_id,
        Range {
            start: Location {
                offset: 0,
                row: 0,
                column: 0,
            },
            end: Location {
                offset: 20,
                row: 0,
                column: 20,
            },
        },
    );

    // Substring extracting bytes 5..10 ("56789") of the original.
    let substring = SourceInfo::substring(original, 5, 10);

    // Map offset 0 in the substring → should land at byte 5 in the
    // original file (not byte 5 in the substring's local coordinates).
    let mapped = substring.map_offset(0, &ctx).expect("map offset");
    assert_eq!(
        mapped.file_id, file_id,
        "chain resolves back to the original file"
    );
    assert_eq!(
        mapped.location.offset, 5,
        "offset is in original-file coordinates, not substring-local"
    );

    // Map offset 4 in substring → byte 9 of original.
    let mapped = substring.map_offset(4, &ctx).expect("map offset");
    assert_eq!(mapped.location.offset, 9);
}

#[test]
fn map_range_pinned_for_attribution_lookup_path() {
    // Same fixture, but driving the range API: attribution-lookup
    // resolves a node's (start, end) range through the chain to get
    // back to original-file bytes.
    let mut ctx = SourceContext::new();
    let file_id = ctx.add_file(
        "test.qmd".to_string(),
        Some("0123456789ABCDEFGHIJ".to_string()),
    );

    let original = SourceInfo::from_range(
        file_id,
        Range {
            start: Location {
                offset: 0,
                row: 0,
                column: 0,
            },
            end: Location {
                offset: 20,
                row: 0,
                column: 20,
            },
        },
    );
    let substring = SourceInfo::substring(original, 5, 10);

    let (start, end) = substring.map_range(0, 5, &ctx).expect("map range");
    assert_eq!(start.file_id, file_id);
    assert_eq!(end.file_id, file_id);
    assert_eq!(start.location.offset, 5);
    assert_eq!(end.location.offset, 10);
}
