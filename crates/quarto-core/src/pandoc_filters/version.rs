//! Pandoc version comparison, the availability/floor gate, and the
//! `(quarto-cli tag, pandoc version)` pin.
//!
//! This duplicates version-parsing logic that already exists twice in this
//! workspace (`crates/xtask/src/dev_setup.rs`'s `check_pandoc`, and pampa's
//! oracle-test floor at `crates/pampa/tests/integration/test.rs:117-118`,
//! reconciled via `crates/xtask/src/pandoc_check.rs`) — a third,
//! independent copy, because `quarto-core` cannot depend on `xtask` (a
//! dev-only binary crate) and pandoc-hybrid's floor (`3.11`) is a distinct,
//! stricter requirement from pampa's oracle-test window (`3.6`-`3.11`).
//! Reconciling all three into one shared crate is out of scope for this
//! task; see `claude-notes/plans/2026-09-18-pandoc-hybrid-P4-implementation.md`
//! Findings for Gordon, item 11(a).

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};

use super::PANDOC_PIN;

/// The minimum `(major, minor)` pandoc version the Pandoc-hybrid leg
/// requires. Parsed from [`super::PANDOC_PIN`] rather than duplicated as a
/// second literal, so the two can never drift independently.
pub fn pandoc_floor() -> (u32, u32) {
    let (major, minor, _) =
        parse_pandoc_version(PANDOC_PIN).expect("PANDOC_PIN must itself parse as a version");
    (major, minor)
}

/// A version string contained no parseable `major[.minor[.patch]]` number.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct VersionParseError;

/// Parses the first `major[.minor[.patch]]` numeric run found in `input` —
/// accepts either a bare version literal (`"3.10"`) or the full `pandoc
/// --version` stdout (`"pandoc 3.8.1\nFeatures: ..."`), since callers pass
/// both shapes (bare literals in tests and constants, real subprocess
/// output at the actual gate).
pub fn parse_pandoc_version(input: &str) -> Result<(u32, u32, u32), VersionParseError> {
    let start = input
        .find(|c: char| c.is_ascii_digit())
        .ok_or(VersionParseError)?;
    let rest = &input[start..];
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(rest.len());
    let version_str = &rest[..end];

    let mut parts = version_str.split('.');
    let major = parts
        .next()
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse().ok())
        .ok_or(VersionParseError)?;
    let minor = parts
        .next()
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let patch = parts
        .next()
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    Ok((major, minor, patch))
}

/// Numeric `(major, minor) >= floor` comparison — never lexicographic
/// (`"3.9" > "3.10"` as strings, but `3.9 < 3.10` numerically). Returns
/// `false` (not an error) when `input` doesn't parse, since callers treat
/// "couldn't determine the version" the same as "too old" at the gate.
pub fn at_least(input: &str, floor: (u32, u32)) -> bool {
    match parse_pandoc_version(input) {
        Ok((major, minor, _)) => (major, minor) >= floor,
        Err(VersionParseError) => false,
    }
}

/// The pandoc availability/floor gate: turns "absent or too old" into a
/// `Q-18-*` diagnostic. `found` mirrors
/// `BinaryDependencies::pandoc`/`SystemRuntime::find_binary`'s
/// `Option<PathBuf>` shape by taking the *version string* already read from
/// that binary (callers own the subprocess call — this function is pure).
pub fn gate(found: Option<&str>) -> Result<(), DiagnosticMessage> {
    match found {
        None => Err(DiagnosticMessageBuilder::error("Pandoc Not Found")
            .with_code("Q-20-1")
            .problem("The pandoc binary could not be found on PATH, and QUARTO_PANDOC is not set.")
            .build()),
        Some(version_str) => {
            let floor = pandoc_floor();
            if at_least(version_str, floor) {
                Ok(())
            } else {
                Err(DiagnosticMessageBuilder::error("Pandoc Version Too Old")
                    .with_code("Q-20-2")
                    .problem(format!(
                        "pandoc reported version {version_str:?}, which is older than the \
                         minimum required version {}.{}.",
                        floor.0, floor.1
                    ))
                    .build())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T7.1: numeric comparison, not lexicographic; parses both a bare
    /// version literal and full `pandoc --version` stdout.
    ///
    /// Revert hunk: reverting the `(major, minor) >= floor` tuple
    /// comparison to a lexicographic string comparison makes
    /// `"3.9.9" >= (3, 10)` incorrectly read `true` (`"3.9.9"` sorts after
    /// `"3.10"` as a string).
    #[test]
    fn test_version_compare_is_numeric() {
        assert!(at_least("3.10", (3, 10)));
        assert!(!at_least("3.9.9", (3, 10)));
        assert!(at_least("3.10.1", (3, 10)));
        assert_eq!(
            parse_pandoc_version("pandoc 3.8.1\nFeatures: +server +lua"),
            Ok((3, 8, 1))
        );
        assert_eq!(parse_pandoc_version(""), Err(VersionParseError));
    }

    /// T7.2: the floor check branch in `gate`.
    ///
    /// Revert hunk: removing the floor check (always returning `Ok`) makes
    /// this RED.
    #[test]
    fn test_gate_rejects_old() {
        let err = gate(Some("3.8.3")).expect_err("3.8.3 is below the 3.11 floor");
        assert_eq!(err.code.as_deref(), Some("Q-20-2"));

        let err = gate(Some("3.10")).expect_err("3.10 is below the 3.11 floor");
        assert_eq!(err.code.as_deref(), Some("Q-20-2"));

        assert!(gate(Some("3.11")).is_ok());
    }

    /// T7.3: the `None` arm in `gate`, carrying a code distinct from the
    /// too-old case.
    ///
    /// Revert hunk: removing the `None` arm (e.g. treating `None` as `Ok`)
    /// makes this RED.
    #[test]
    fn test_gate_rejects_absent() {
        let err = gate(None).expect_err("no pandoc found should be an error");
        assert_eq!(err.code.as_deref(), Some("Q-20-1"));
        assert_ne!(
            err.code,
            gate(Some("3.8.3")).unwrap_err().code,
            "not-found and too-old must carry distinct codes"
        );
    }
}
