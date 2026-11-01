//! KNR-2 Phase 2G: the corpus gate and its allowlist of signed-off divergences.
//!
//! Epic decision 26: the gate accepts `identical` and `syntax-level` only. A `semantic` or
//! `blocker` difference needs an entry in `tests/fixtures/knitr-corpus/allowlist.toml` (epic
//! decision 23), which pins the case's name, optionally its formats, the class it is allowed
//! to have, and a one-line reason. Three things are failures: a difference with no entry, an
//! entry whose case is now `identical` or `syntax-level` (stale: delete it), and an entry
//! whose case moved to a different class than the one signed off (re-review it).
//!
//! Cases that were never compared (`fallback`, `aborted`, `unavailable`) are counted, not
//! judged: the report already lists them, and their reasons are by design or environmental.

#![cfg(not(target_arch = "wasm32"))]

use std::path::Path;

use serde::Deserialize;

use super::knitr_corpus_runner::{CaseRecord, CaseResult};
use super::knitr_corpus_support::corpus_dir;

/// One signed-off divergence.
#[derive(Debug, Clone, Deserialize)]
pub struct Entry {
    pub name: String,
    /// Formats the entry covers; absent means every format the case ran under.
    #[serde(default)]
    pub formats: Option<Vec<String>>,
    /// The class the divergence was signed off at: `semantic` or `blocker`.
    pub class: String,
    pub reason: String,
}

impl Entry {
    fn covers(&self, name: &str, format: &str) -> bool {
        self.name == name
            && self
                .formats
                .as_ref()
                .is_none_or(|fs| fs.iter().any(|f| f == format))
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Allowlist {
    #[serde(default, rename = "entry")]
    pub entries: Vec<Entry>,
}

impl Allowlist {
    pub fn parse(text: &str) -> Result<Self, String> {
        let list: Allowlist = toml::from_str(text).map_err(|e| e.to_string())?;
        for e in &list.entries {
            if !matches!(e.class.as_str(), "semantic" | "blocker") {
                return Err(format!(
                    "entry `{}`: class `{}` is not allowlistable (only `semantic` or `blocker`)",
                    e.name, e.class
                ));
            }
            if e.reason.trim().is_empty() {
                return Err(format!("entry `{}` has no reason", e.name));
            }
        }
        Ok(list)
    }

    pub fn load(dir: &Path) -> Result<Self, String> {
        let path = dir.join("allowlist.toml");
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// What the gate made of a set of case records.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Gate {
    /// `identical` or `syntax-level`.
    pub accepted: usize,
    /// Different, with a matching entry: `name [format]`.
    pub allowlisted: Vec<String>,
    /// Different, with no matching entry: `name [format] class`.
    pub unexpected: Vec<String>,
    /// An entry whose case is now accepted, or sits at another class: a description.
    pub stale: Vec<String>,
    /// `fallback`, `aborted` or `unavailable`.
    pub not_compared: usize,
}

impl Gate {
    pub fn passes(&self) -> bool {
        self.unexpected.is_empty() && self.stale.is_empty()
    }

    pub fn render(&self) -> String {
        let mut out = format!(
            "gate: {}\n  accepted {}, allowlisted {}, not compared {}\n",
            if self.passes() { "PASS" } else { "FAIL" },
            self.accepted,
            self.allowlisted.len(),
            self.not_compared
        );
        for (title, list) in [
            ("unexpected (no allowlist entry)", &self.unexpected),
            ("stale or reclassified allowlist entries", &self.stale),
            ("allowlisted", &self.allowlisted),
        ] {
            if !list.is_empty() {
                out.push_str(&format!("\n{title} ({}):\n", list.len()));
                for l in list {
                    out.push_str(&format!("  {l}\n"));
                }
            }
        }
        out
    }
}

/// Judge `records` against `allowlist`. An entry whose case is absent from `records` is
/// ignored, so a partial run (`KNR_ONLY`, `KNR_SAMPLE`) does not flag it; that an entry names
/// a real fixture is a separate test.
pub fn evaluate(records: &[CaseRecord], allowlist: &Allowlist) -> Gate {
    let mut gate = Gate::default();
    for r in records {
        let CaseResult::Compared { class, .. } = &r.result else {
            gate.not_compared += 1;
            continue;
        };
        let case = format!("{} [{}]", r.name, r.format);
        let entry = allowlist
            .entries
            .iter()
            .find(|e| e.covers(&r.name, &r.format));
        let accepted = matches!(class.as_str(), "identical" | "syntax-level");
        match (accepted, entry) {
            (true, None) => gate.accepted += 1,
            (true, Some(_)) => {
                gate.accepted += 1;
                gate.stale
                    .push(format!("{case} is now {class}: delete its entry"));
            }
            (false, None) => gate.unexpected.push(format!("{case} {class}")),
            (false, Some(e)) if e.class == *class => gate.allowlisted.push(case),
            (false, Some(e)) => gate.stale.push(format!(
                "{case} is now {class}, signed off as {}: re-review its entry",
                e.class
            )),
        }
    }
    gate
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compared(name: &str, format: &str, class: &str) -> CaseRecord {
        CaseRecord {
            name: name.into(),
            group: "g".into(),
            format: format.into(),
            result: CaseResult::Compared {
                class: class.into(),
                multi_group: false,
                diff: None,
                q1: None,
            },
        }
    }

    fn list(text: &str) -> Allowlist {
        Allowlist::parse(text).unwrap()
    }

    const ONE: &str = r#"
[[entry]]
name = "a/one"
class = "blocker"
reason = "by design"
"#;

    #[test]
    fn identical_and_syntax_level_are_accepted_without_an_entry() {
        let g = evaluate(
            &[
                compared("x", "html", "identical"),
                compared("y", "html", "syntax-level"),
            ],
            &Allowlist::default(),
        );
        assert_eq!((g.accepted, g.passes()), (2, true), "{}", g.render());
    }

    #[test]
    fn a_semantic_or_blocker_difference_needs_an_entry() {
        let g = evaluate(
            &[
                compared("x", "html", "semantic"),
                compared("y", "typst", "blocker"),
            ],
            &Allowlist::default(),
        );
        assert_eq!(g.unexpected, ["x [html] semantic", "y [typst] blocker"]);
        assert!(!g.passes());
    }

    #[test]
    fn a_matching_entry_allowlists_the_difference() {
        let g = evaluate(&[compared("a/one", "html", "blocker")], &list(ONE));
        assert_eq!(g.allowlisted, ["a/one [html]"]);
        assert!(g.passes(), "{}", g.render());
    }

    #[test]
    fn an_entry_with_formats_covers_only_those_formats() {
        let l = list(
            r#"
[[entry]]
name = "a/one"
formats = ["html"]
class = "blocker"
reason = "html only"
"#,
        );
        let g = evaluate(
            &[
                compared("a/one", "html", "blocker"),
                compared("a/one", "docx", "blocker"),
            ],
            &l,
        );
        assert_eq!(g.allowlisted, ["a/one [html]"]);
        assert_eq!(g.unexpected, ["a/one [docx] blocker"]);
    }

    #[test]
    fn an_entry_whose_case_is_now_accepted_is_stale() {
        let g = evaluate(&[compared("a/one", "html", "syntax-level")], &list(ONE));
        assert_eq!(g.stale.len(), 1, "{}", g.render());
        assert!(g.stale[0].contains("a/one"), "{:?}", g.stale);
        assert!(!g.passes());
    }

    #[test]
    fn an_entry_whose_case_changed_class_is_flagged() {
        let g = evaluate(&[compared("a/one", "html", "semantic")], &list(ONE));
        assert_eq!(g.stale.len(), 1, "{}", g.render());
        assert!(g.stale[0].contains("semantic"), "{:?}", g.stale);
        assert!(g.allowlisted.is_empty());
    }

    #[test]
    fn an_entry_for_a_case_outside_the_run_is_ignored() {
        let g = evaluate(&[compared("other", "html", "identical")], &list(ONE));
        assert!(g.passes(), "{}", g.render());
    }

    #[test]
    fn every_entry_names_a_corpus_fixture_and_a_format_it_runs_under() {
        use super::super::knitr_corpus_support::CorpusManifest;
        let dir = corpus_dir();
        let fixtures = CorpusManifest::load(&dir).unwrap().generate(1);
        for e in &Allowlist::load(&dir).unwrap().entries {
            let f = fixtures
                .iter()
                .find(|f| f.name == e.name)
                .unwrap_or_else(|| panic!("allowlist entry `{}` names no corpus fixture", e.name));
            for fmt in e.formats.iter().flatten() {
                assert!(
                    f.formats.contains(fmt),
                    "`{}` does not run under {fmt}",
                    e.name
                );
            }
        }
    }

    #[test]
    fn cases_that_were_not_compared_are_counted_not_judged() {
        let mut r = compared("x", "html", "identical");
        r.result = CaseResult::Fallback { reason: "r".into() };
        let g = evaluate(&[r], &Allowlist::default());
        assert_eq!((g.not_compared, g.accepted, g.passes()), (1, 0, true));
    }

    #[test]
    fn parse_rejects_an_unallowlistable_class_and_a_missing_reason() {
        let bad_class = "[[entry]]\nname = \"a\"\nclass = \"syntax-level\"\nreason = \"r\"\n";
        assert!(
            Allowlist::parse(bad_class)
                .unwrap_err()
                .contains("not allowlistable")
        );
        let no_reason = "[[entry]]\nname = \"a\"\nclass = \"blocker\"\nreason = \" \"\n";
        assert!(
            Allowlist::parse(no_reason)
                .unwrap_err()
                .contains("no reason")
        );
    }
}
