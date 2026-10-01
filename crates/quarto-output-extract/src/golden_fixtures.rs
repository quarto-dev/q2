//! The pandoc-goldens fixture manifest and snapshot-naming function,
//! shared by the `G`-tier capture xtask
//! (`crates/xtask/src/capture_pandoc_goldens.rs`, P7 Task 10) and the
//! `I`-tier assertion test
//! (`crates/quarto-core/tests/integration/pandoc_goldens.rs`, P7 Task 11).
//!
//! This lives here — not in either consumer — for the same reason the
//! extraction function does: `xtask` must not gain a `quarto-core`
//! dependency, so the piece both sides call has to be the leaf they
//! already share. Both sides **must** call [`golden_snapshot_name`]
//! directly; re-deriving the name independently on either side is
//! exactly the silent-"created new" failure mode T10.3/T11.4 guard
//! against.

/// One fixture: its `.qmd` path (relative to
/// `crates/quarto-core/tests/fixtures/pandoc-goldens/`) and any resource
/// files it references (also relative to that same root), which must be
/// copied alongside it for images to resolve.
pub struct FixtureEntry {
    pub qmd: &'static str,
    pub resources: &'static [&'static str],
}

/// The frozen fixture manifest — 10 entries, per the plan's fixture table.
/// The count is asserted as a literal (T10.4), not derived, so an
/// accidental deletion is caught rather than silently shrinking the set.
pub const FIXTURES: &[FixtureEntry] = &[
    FixtureEntry {
        qmd: "crossrefs/all-docx.qmd",
        resources: &["crossrefs/img/thinker.jpg"],
    },
    FixtureEntry {
        qmd: "callouts.qmd",
        resources: &[],
    },
    FixtureEntry {
        qmd: "crossrefs/callouts.qmd",
        resources: &["crossrefs/img/painter.jpg", "crossrefs/img/abbas.jpg"],
    },
    FixtureEntry {
        qmd: "crossrefs/theorems.qmd",
        resources: &[],
    },
    FixtureEntry {
        qmd: "crossrefs/theorem-types.qmd",
        resources: &[],
    },
    FixtureEntry {
        qmd: "crossrefs/equations.qmd",
        resources: &[],
    },
    FixtureEntry {
        qmd: "smoke-all/crossrefs/theorem/proof-rendering.qmd",
        resources: &[],
    },
    FixtureEntry {
        qmd: "smoke-all/2025/01/08/7260.qmd",
        resources: &[],
    },
    FixtureEntry {
        qmd: "smoke-all/mermaid/backticks.qmd",
        resources: &[],
    },
    FixtureEntry {
        qmd: "tabset-subfloat.qmd",
        resources: &[],
    },
];

/// The shared snapshot-naming function. **Both** the `G`-tier capture
/// xtask (writing) and `pandoc_goldens.rs`'s assertion test (reading,
/// P7 Task 11) must call this same function — never re-derive it
/// independently on either side (T10.3/T11.4's whole point: a naming
/// mismatch is a silent "created new" pass, not a build error).
pub fn golden_snapshot_name(qmd_path: &str, format: &str) -> String {
    let stem = qmd_path.trim_end_matches(".qmd").replace(['/', '-'], "_");
    format!("{stem}__{format}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_snapshot_name_matches_hardcoded_literals() {
        assert_eq!(
            golden_snapshot_name("crossrefs/all-docx.qmd", "docx"),
            "crossrefs_all_docx__docx"
        );
        assert_eq!(
            golden_snapshot_name("callouts.qmd", "pptx"),
            "callouts__pptx"
        );
        assert_eq!(
            golden_snapshot_name("tabset-subfloat.qmd", "docx"),
            "tabset_subfloat__docx"
        );
    }

    #[test]
    fn test_fixture_manifest_has_exactly_ten_entries() {
        assert_eq!(FIXTURES.len(), 10);
    }
}
