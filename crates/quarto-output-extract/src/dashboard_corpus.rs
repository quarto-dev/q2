//! The dashboard compatibility corpus: its manifest format and golden paths.
//!
//! Shared by the dev-only capture (`cargo xtask capture-dashboard-goldens`) and
//! the assertion test (`crates/quarto-core/tests/integration/dashboard_goldens.rs`)
//! for the same reason as [`crate::golden_fixtures`]: both sides must name and
//! read goldens through one function, or one side silently drifts.
//!
//! The corpus lives at [`CORPUS_DIR_RELATIVE`]. `manifest.toml` lists every
//! fixture; `goldens/` holds one skeleton per fixture captured from Quarto 1
//! ([`q1_golden_path`]) and, for accepted divergences, the expected Q2 skeleton
//! ([`q2_golden_path`]). Plan:
//! `claude-notes/plans/2026-11-03-dashboard-format-port.md`, Phase 0.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The corpus root, relative to the worktree root.
pub const CORPUS_DIR_RELATIVE: &str = "crates/quarto-core/tests/fixtures/dashboard-goldens";

/// What the assertion test does with a fixture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    /// Not yet implemented in Q2; the test skips it. `note` names the phase.
    Pending,
    /// Q2's skeleton must equal the Q1 golden.
    Match,
    /// Q2 deliberately differs; its skeleton must equal the `.q2.skeleton`
    /// golden, and `note` must say why (a D5 fix or another plan decision).
    Divergence,
    /// Out of scope for the epic (a follow-up strand); `note` names it.
    Deferred,
}

/// One fixture in `manifest.toml`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fixture {
    /// Path relative to the corpus root (`contract/c01-rows-default.qmd`).
    pub path: String,
    /// Engines Q2 needs to render it (`knitr`, `jupyter`); empty when
    /// engine-free.
    #[serde(default)]
    pub requires: Vec<String>,
    pub status: Status,
    /// Whether a Q1 golden exists. False when Q1 cannot render the fixture
    /// (a Q1 bug the port fixes, or a package missing at capture time);
    /// `note` must say which.
    #[serde(default = "default_true")]
    pub q1_golden: bool,
    /// Why the fixture has its status (phase, strand, D5 item, capture gap).
    pub note: String,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    #[serde(rename = "fixture")]
    pub fixtures: Vec<Fixture>,
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|e| e.to_string())
    }

    pub fn load(corpus_root: &Path) -> Result<Self, String> {
        let path = corpus_root.join("manifest.toml");
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// `goldens/<path with '/' replaced by "__">.q1.skeleton`, relative to the
/// corpus root.
pub fn q1_golden_path(fixture_path: &str) -> PathBuf {
    golden(fixture_path, "q1")
}

/// The expected Q2 skeleton for a `divergence` fixture.
pub fn q2_golden_path(fixture_path: &str) -> PathBuf {
    golden(fixture_path, "q2")
}

fn golden(fixture_path: &str, side: &str) -> PathBuf {
    let stem = fixture_path.replace('/', "__");
    let stem = stem
        .strip_suffix(".qmd")
        .or_else(|| stem.strip_suffix(".ipynb"))
        .unwrap_or(&stem);
    Path::new("goldens").join(format!("{stem}.{side}.skeleton"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_paths_flatten_directories() {
        assert_eq!(
            q1_golden_path("contract/c01-rows-default.qmd"),
            Path::new("goldens").join("contract__c01-rows-default.q1.skeleton")
        );
        assert_eq!(
            q2_golden_path("regressions/issue-9056.ipynb"),
            Path::new("goldens").join("regressions__issue-9056.q2.skeleton")
        );
    }

    #[test]
    fn manifest_parses_with_defaults() {
        let m = Manifest::parse(
            r#"
[[fixture]]
path = "contract/c01.qmd"
status = "pending"
note = "Phase 3"

[[fixture]]
path = "q1/x.qmd"
requires = ["knitr"]
status = "divergence"
q1_golden = false
note = "D5"
"#,
        )
        .unwrap();
        assert_eq!(m.fixtures.len(), 2);
        assert!(m.fixtures[0].q1_golden);
        assert!(m.fixtures[0].requires.is_empty());
        assert_eq!(m.fixtures[1].status, Status::Divergence);
        assert!(!m.fixtures[1].q1_golden);
    }

    #[test]
    fn manifest_rejects_unknown_status_and_fields() {
        assert!(Manifest::parse("[[fixture]]\npath='a'\nstatus='maybe'\nnote='n'\n").is_err());
        assert!(
            Manifest::parse("[[fixture]]\npath='a'\nstatus='pending'\nnote='n'\nextra=1\n")
                .is_err()
        );
    }
}
