//! Classify a `quarto-required` version range by the Quarto major
//! versions it admits (bd-gw8fmaz3).
//!
//! `quarto-required` is Quarto 1's key for "which Quarto versions may
//! render this": a string holding an npm-style semver range (`^2`,
//! `>= 1.4.0`, `1.*`). A project declares that it targets Quarto 2 by
//! writing a range that admits Quarto 2 and excludes Quarto 1, which
//! every Quarto 1 since 1.4 already refuses to render. See
//! `claude-notes/plans/2026-11-17-project-quarto-version-marker.md`.
//!
//! # What "admits 1.x / 2.x" means
//!
//! A range admits major `M` when some version with major `M` satisfies
//! it, prereleases included, with one deliberate exception: prereleases
//! of `M.0.0` itself (`2.0.0-alpha`) do not count as `M.x`. Under npm's
//! conventions `<2.0.0` is how an author spells "below 2", and such a
//! range technically admits `2.0.0-alpha`; counting that would classify
//! `>=1.7 <2.0.0` as spanning both majors, which is not what its author
//! meant (likewise `<1` and `1.0.0-alpha`). A Quarto 2 prerelease such as
//! `2.0.46-prerelease` is a 2.x version.
//!
//! The answer is computed with exact interval intersection
//! ([`nodejs_semver::Range::allows_any`]), not by probing sample
//! versions: a probe set misses exact pins (`2.3.4`) and narrow ranges.
//! (Node's own `semver.intersects` is not usable as a reference here: it
//! answers inconsistently at the 1/2 boundary depending on how the probe
//! range is spelled.)
//!
//! # Checking a range against q2's own version
//!
//! This module only classifies the range; it never compares it with the
//! running q2's version. If that check is added later, note that under
//! npm semantics a prerelease satisfies a range only when the range
//! names a prerelease of the same `major.minor.patch`:
//! `2.0.46-prerelease` does **not** satisfy `^2` unless the check is
//! made with `satisfies_with_prerelease(.., true)`.

use nodejs_semver::Range;

/// Which Quarto major versions a `quarto-required` range admits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MajorCoverage {
    /// Some 1.x version satisfies the range.
    pub quarto1: bool,
    /// Some 2.x version satisfies the range.
    pub quarto2: bool,
}

impl MajorCoverage {
    /// The range declares a Quarto 2 project: it admits 2.x and no 1.x.
    pub fn targets_quarto2_only(&self) -> bool {
        self.quarto2 && !self.quarto1
    }
}

/// A `quarto-required` value that is not a usable version range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidRange {
    /// Why the range was rejected, for the user-facing diagnostic.
    pub reason: String,
}

/// The 1.x versions, prereleases of `1.0.0` excluded (see the module
/// docs).
const QUARTO1: &str = ">=1.0.0 <2.0.0-0";
/// The 2.x versions, prereleases of `2.0.0` excluded.
const QUARTO2: &str = ">=2.0.0 <3.0.0-0";

/// Classify a `quarto-required` range by the majors it admits.
///
/// An empty (or all-whitespace) value is rejected rather than read as
/// npm's "any version": it is far more likely an unfinished edit than a
/// deliberate declaration, and Quarto 1 ignores it too.
pub fn classify_quarto_required(range: &str) -> Result<MajorCoverage, InvalidRange> {
    if range.trim().is_empty() {
        return Err(InvalidRange {
            reason: "the range is empty".to_string(),
        });
    }
    let parsed = Range::parse(range).map_err(|_| InvalidRange {
        reason: format!("`{range}` is not a valid version range"),
    })?;
    // The two constants are valid ranges; parsing them cannot fail.
    let quarto1 = Range::parse(QUARTO1).expect("QUARTO1 is a valid range");
    let quarto2 = Range::parse(QUARTO2).expect("QUARTO2 is a valid range");
    Ok(MajorCoverage {
        quarto1: parsed.allows_any(&quarto1),
        quarto2: parsed.allows_any(&quarto2),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coverage(range: &str) -> (bool, bool) {
        let c = classify_quarto_required(range)
            .unwrap_or_else(|e| panic!("`{range}` should parse: {e:?}"));
        (c.quarto1, c.quarto2)
    }

    #[test]
    fn quarto2_only_ranges() {
        for range in [
            "^2",
            ">=2",
            "2.3.4",
            ">=2.0.0 <3",
            "2.x",
            "~2.1",
            "2.0.46-prerelease",
            // Admits 3.x too; still no 1.x, so it targets Quarto 2.
            ">=2 <4",
        ] {
            assert_eq!(coverage(range), (false, true), "{range}");
            assert!(
                classify_quarto_required(range)
                    .unwrap()
                    .targets_quarto2_only()
            );
        }
    }

    #[test]
    fn ranges_spanning_both_majors() {
        for range in [">=1.4", "*", "x", ">=2.5 || 1.9", "1.x || >=2.5.0", "<3"] {
            assert_eq!(coverage(range), (true, true), "{range}");
            assert!(
                !classify_quarto_required(range)
                    .unwrap()
                    .targets_quarto2_only()
            );
        }
    }

    #[test]
    fn quarto1_only_ranges() {
        for range in [
            "~1.7",
            "^1.7",
            "1.*",
            ">=1.7 <2.0.0",
            "<2",
            "<=2.0.0-0",
            "1.7.31",
        ] {
            assert_eq!(coverage(range), (true, false), "{range}");
        }
    }

    #[test]
    fn ranges_admitting_neither_major() {
        for range in ["^3", ">=3", "0.46.0", "<1"] {
            assert_eq!(coverage(range), (false, false), "{range}");
        }
    }

    #[test]
    fn prereleases_of_2_0_0_are_not_2x() {
        // Only 2.0.0 prereleases: an author who writes this has not
        // declared a released Quarto 2 (see the module docs).
        assert_eq!(coverage(">=2.0.0-0 <2.0.0"), (false, false));
        // …but a range reaching 2.0.0 itself does admit 2.x.
        assert_eq!(coverage("2.0.0-0 - 2.0.0"), (false, true));
    }

    #[test]
    fn malformed_and_empty_ranges_are_rejected() {
        for range in ["foo", ">=", "2.0.0.0", "", "   "] {
            assert!(
                classify_quarto_required(range).is_err(),
                "`{range}` should be rejected"
            );
        }
        assert_eq!(
            classify_quarto_required("foo").unwrap_err().reason,
            "`foo` is not a valid version range"
        );
        assert_eq!(
            classify_quarto_required("").unwrap_err().reason,
            "the range is empty"
        );
    }
}
