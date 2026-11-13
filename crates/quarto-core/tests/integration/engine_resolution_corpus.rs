//! Engine-resolution corpus (bd-yt2o2ovt design review, 2026-11-04).
//!
//! One test drives every case in `engine_resolution_corpus/cases.yaml`
//! through the pure resolver (`resolve_engines`: metadata + AST + registry,
//! no kernels, no I/O). The expected owners are the TARGET design (D2b, see
//! `claude-notes/plans/2026-11-04-engine-interface-review.md`). A case whose
//! `status` is `pending-phase4` is expected to differ from the target today:
//! the test pins the current behavior (`today`) and fails if the case starts
//! passing, so whoever lands Phase B3 flips the marker instead of leaving a
//! stale expected-fail.
//!
//! The human-readable table lives at
//! `claude-notes/research/2026-11-04-engine-resolution-corpus.md`.

use std::collections::BTreeMap;
use std::sync::Arc;

use quarto_config::MergedConfig;
use quarto_core::engine::{
    EngineRegistry, EngineResolution, ExecuteResult, ExecutionContext, ExecutionEngine,
    ExecutionError, JupyterEngine, KnitrEngine, LanguageClaim, MarkdownEngine, resolve_engines,
};
use quarto_pandoc_types::Pandoc;
use serde::Deserialize;

const CASES: &str = include_str!("engine_resolution_corpus/cases.yaml");

#[derive(Debug, Deserialize)]
struct Case {
    id: String,
    #[allow(dead_code)]
    title: String,
    #[serde(default)]
    project: Option<String>,
    front_matter: String,
    languages: Vec<String>,
    registry: String,
    expect: Outcome,
    status: Status,
    #[serde(default)]
    today: Option<Outcome>,
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone)]
struct Outcome {
    sequence: Vec<String>,
    ownership: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "kebab-case")]
enum Status {
    Pass,
    PendingPhase4,
}

/// A mock extension engine with a fixed claim function.
struct Ext {
    name: &'static str,
    claim: fn(&str, Option<&str>) -> LanguageClaim,
}

impl ExecutionEngine for Ext {
    fn name(&self) -> &str {
        self.name
    }
    fn claims_language(&self, language: &str, first_class: Option<&str>) -> LanguageClaim {
        (self.claim)(language, first_class)
    }
    fn try_claims_language(
        &self,
        language: &str,
        first_class: Option<&str>,
    ) -> Option<LanguageClaim> {
        Some((self.claim)(language, first_class))
    }
    fn execute(
        &self,
        _input: &str,
        _ctx: &ExecutionContext,
    ) -> Result<ExecuteResult, ExecutionError> {
        Ok(ExecuteResult::new(String::new()))
    }
}

fn julia_claim(language: &str, _fc: Option<&str>) -> LanguageClaim {
    if language == "julia" {
        LanguageClaim::Primary(1)
    } else {
        LanguageClaim::None
    }
}

fn marimo_claim_p1(language: &str, fc: Option<&str>) -> LanguageClaim {
    if language == "python" && fc == Some("marimo") {
        LanguageClaim::Primary(1)
    } else {
        LanguageClaim::None
    }
}

fn marimo_claim_p2(language: &str, fc: Option<&str>) -> LanguageClaim {
    if language == "python" && fc == Some("marimo") {
        LanguageClaim::Primary(2)
    } else {
        LanguageClaim::None
    }
}

/// Registries by corpus name. Extension engines are registered after the
/// built-ins; the registry's contribution order has no public setter, which is
/// why the corpus compares sequence order as a set for E2/E3/E3b.
fn registry(name: &str) -> EngineRegistry {
    let mut r = EngineRegistry::empty();
    r.register(Arc::new(KnitrEngine::new()));
    r.register(Arc::new(JupyterEngine::new()));
    r.register(Arc::new(MarkdownEngine::new()));
    match name {
        "E1" => {}
        "E2" => r.register(Arc::new(Ext {
            name: "julia",
            claim: julia_claim,
        })),
        "E3" => r.register(Arc::new(Ext {
            name: "marimo",
            claim: marimo_claim_p1,
        })),
        "E3b" => r.register(Arc::new(Ext {
            name: "marimo",
            claim: marimo_claim_p2,
        })),
        other => panic!("unknown corpus registry {other}"),
    }
    r
}

fn parse(src: &str) -> Pandoc {
    let mut sink = Vec::new();
    let (ast, _ctx, _diags) =
        pampa::readers::qmd::read(src.as_bytes(), false, "corpus.qmd", &mut sink, true, None)
            .expect("corpus case must parse");
    ast
}

fn front_matter(yaml: &str) -> String {
    if yaml.trim().is_empty() {
        String::new()
    } else {
        format!("---\n{yaml}\n---\n\n")
    }
}

fn run(case: &Case) -> (Outcome, EngineResolution) {
    let mut body = String::new();
    for lang in &case.languages {
        body.push_str(&format!("```{{{lang}}}\n1\n```\n\n"));
    }
    let mut ast = parse(&format!("{}{body}", front_matter(&case.front_matter)));
    if let Some(project) = case.project.as_deref() {
        let project_ast = parse(&front_matter(project));
        ast.meta = MergedConfig::new(vec![&project_ast.meta, &ast.meta])
            .materialize()
            .expect("layers must merge");
    }
    let resolution = resolve_engines(&ast.meta, &ast, &registry(&case.registry), None);
    let outcome = Outcome {
        sequence: resolution.sequence.iter().map(|e| e.name.clone()).collect(),
        ownership: resolution
            .ownership
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
    };
    (outcome, resolution)
}

/// Sequence order is exact for E1 and a set comparison for extension
/// registries (see the module docs).
fn same(case: &Case, a: &Outcome, b: &Outcome) -> bool {
    if a.ownership != b.ownership {
        return false;
    }
    if case.registry == "E1" {
        a.sequence == b.sequence
    } else {
        let mut x = a.sequence.clone();
        let mut y = b.sequence.clone();
        x.sort();
        y.sort();
        x == y
    }
}

#[test]
fn corpus_matches_target_or_pins_current_behavior() {
    let cases: Vec<Case> = serde_yaml::from_str(CASES).expect("cases.yaml must parse");

    let mut ids = std::collections::HashSet::new();
    for c in &cases {
        assert!(ids.insert(c.id.clone()), "duplicate corpus id {}", c.id);
    }

    let (mut pass, mut expected_fail) = (0usize, 0usize);
    let mut problems: Vec<String> = Vec::new();

    for case in &cases {
        let (actual, _res) = run(case);
        let meets_target = same(case, &actual, &case.expect);
        match case.status {
            Status::Pass => {
                if meets_target {
                    pass += 1;
                } else {
                    problems.push(format!(
                        "{}: expected {:?} but resolver gave {:?}",
                        case.id, case.expect, actual
                    ));
                }
            }
            Status::PendingPhase4 => {
                let Some(today) = case.today.as_ref() else {
                    problems.push(format!("{}: pending-phase4 needs a `today` block", case.id));
                    continue;
                };
                if meets_target {
                    problems.push(format!(
                        "{}: now meets the target; change status to `pass` and drop `today`",
                        case.id
                    ));
                } else if !same(case, &actual, today) {
                    problems.push(format!(
                        "{}: current behavior changed; pinned {:?} but resolver gave {:?}",
                        case.id, today, actual
                    ));
                } else {
                    expected_fail += 1;
                }
            }
        }
    }

    eprintln!(
        "engine resolution corpus: {} cases, {} pass, {} expected-fail (pending Phase B3)",
        cases.len(),
        pass,
        expected_fail
    );
    assert!(
        problems.is_empty(),
        "corpus problems:\n{}",
        problems.join("\n")
    );
}
