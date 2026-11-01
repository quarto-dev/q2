//! KNR-1 Phase 1D runner: for each corpus fixture, one capture-on R run gives knitr's markdown
//! (the oracle) and the chunks' pieces together; the baseline presents the pieces; both are
//! normalized and compared at three levels (`knitr_corpus_classify`).
//!
//! `corpus_report` is the entry point for the report: it does nothing unless
//! `KNR_REPORT=<file.json>` is set (a full run takes minutes), and then writes one JSON record
//! per (fixture, format).

#![cfg(not(target_arch = "wasm32"))]

use quarto_core::engine::{KnitrCapturedChunk, KnitrPiece};
use serde::Serialize;

use super::knitr_corpus_baseline::{Baseline, baseline};
use super::knitr_corpus_classify::{Class, classify};
use super::knitr_corpus_normalizer::{normalize, normalize_figure_paths};
use super::knitr_corpus_q1::{q1_enabled, q1_markdown};
use super::knitr_corpus_run::{Outcome, parallel, r_available, run_fixture_with_env};
use super::knitr_corpus_support::{CorpusManifest, Fixture, corpus_dir};

/// What one (fixture, format) case came to.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum CaseResult {
    /// Compared: its class and, unless identical, a short diff of the first difference.
    Compared {
        class: String,
        /// Some chunk has two or more source groups (a source run after an output): the S2
        /// divergence (one `render_cell` call per chunk puts all source first).
        multi_group: bool,
        diff: Option<String>,
        /// Quarto 1 against q2's current knitr output (`KNR_Q1=1`, html only): its class and
        /// first difference, or why Q1 could not render.
        #[serde(skip_serializing_if = "Option::is_none")]
        q1: Option<Q1Column>,
    },
    /// The baseline could not present the document chunk by chunk.
    Fallback { reason: String },
    /// knitr itself aborted (consistently with capture off, see the gates).
    Aborted { message: String },
    /// A package the fixture needs is not installed.
    Unavailable { package: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct Q1Column {
    pub class: Option<String>,
    pub diff: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseRecord {
    pub name: String,
    pub group: String,
    pub format: String,
    #[serde(flatten)]
    pub result: CaseResult,
}

/// Run one case. `env` pins knitr (see `run_fixture_with_env`).
pub fn run_case(f: &Fixture, format: &str, env: &[(String, String)]) -> CaseRecord {
    let result = match Outcome::from_run(run_fixture_with_env(f, format, true, env)) {
        Outcome::Aborted(message) => CaseResult::Aborted { message },
        Outcome::Unavailable(package) => CaseResult::Unavailable { package },
        Outcome::Ran(run) => {
            let doc_path = run.doc_dir().join(format!("doc.{}", f.ext));
            match baseline(&f.text, &doc_path, &run.chunks) {
                Baseline::Fallback(reason) => CaseResult::Fallback { reason },
                Baseline::Candidate(candidate) => {
                    let oracle = normalize_figure_paths(&normalize(&run.markdown, run.doc_dir()));
                    let candidate = normalize_figure_paths(&normalize(&candidate, run.doc_dir()));
                    let class = classify(&oracle, &candidate);
                    let q1 = (format == "html" && q1_enabled()).then(|| match q1_markdown(f) {
                        Err(error) => Q1Column {
                            class: None,
                            diff: None,
                            error: Some(error),
                        },
                        Ok(q1_md) => {
                            let q1_md = normalize_figure_paths(&q1_md);
                            let c = classify(&q1_md, &oracle);
                            Q1Column {
                                class: Some(c.label().to_string()),
                                diff: (c != Class::Identical)
                                    .then(|| first_difference(&q1_md, &oracle)),
                                error: None,
                            }
                        }
                    });
                    CaseResult::Compared {
                        q1,
                        class: class.label().to_string(),
                        multi_group: run.chunks.iter().any(|c| source_groups(c) > 1),
                        diff: (class != Class::Identical)
                            .then(|| first_difference(&oracle, &candidate)),
                    }
                }
            }
        }
    };
    CaseRecord {
        name: f.name.clone(),
        group: f.group.clone(),
        format: format.to_string(),
        result,
    }
}

/// The report's counts: per class and group, the fallback and abort lists, and the Quarto 1
/// column. Plain text, stable order, so two runs diff.
pub fn summarize(records: &[CaseRecord]) -> String {
    use std::collections::BTreeMap;
    let mut by_group: BTreeMap<&str, BTreeMap<String, usize>> = BTreeMap::new();
    let mut q1: BTreeMap<String, usize> = BTreeMap::new();
    let (mut fallbacks, mut aborts, mut missing) = (Vec::new(), Vec::new(), Vec::new());
    for r in records {
        let label = match &r.result {
            CaseResult::Compared {
                class,
                multi_group,
                q1: q1c,
                ..
            } => {
                if let Some(c) = q1c {
                    *q1.entry(c.class.clone().unwrap_or_else(|| "q1 failed".into()))
                        .or_default() += 1;
                }
                format!(
                    "{class}{}",
                    if *multi_group { " (multi-group)" } else { "" }
                )
            }
            CaseResult::Fallback { reason } => {
                fallbacks.push(format!("{} [{}]: {reason}", r.name, r.format));
                "fallback".to_string()
            }
            CaseResult::Aborted { .. } => {
                aborts.push(format!("{} [{}]", r.name, r.format));
                "aborted".to_string()
            }
            CaseResult::Unavailable { package } => {
                missing.push(format!("{} [{}]: {package}", r.name, r.format));
                "unavailable".to_string()
            }
        };
        *by_group
            .entry(r.group.as_str())
            .or_default()
            .entry(label)
            .or_default() += 1;
    }
    let mut out = format!("{} cases\n\nby group:\n", records.len());
    for (group, counts) in &by_group {
        let line: Vec<String> = counts.iter().map(|(k, v)| format!("{k}: {v}")).collect();
        out.push_str(&format!("  {group}: {}\n", line.join(", ")));
    }
    if !q1.is_empty() {
        let line: Vec<String> = q1.iter().map(|(k, v)| format!("{k}: {v}")).collect();
        out.push_str(&format!("\nquarto 1 vs q2 live: {}\n", line.join(", ")));
    }
    for (title, list) in [
        ("fallback", fallbacks),
        ("aborted", aborts),
        ("unavailable", missing),
    ] {
        out.push_str(&format!("\n{title} ({}):\n", list.len()));
        for l in list {
            out.push_str(&format!("  {l}\n"));
        }
    }
    out
}

/// How many runs of consecutive `source` pieces a chunk has.
pub fn source_groups(chunk: &KnitrCapturedChunk) -> usize {
    let mut groups = 0;
    let mut prev_source = false;
    for p in &chunk.pieces {
        let is_source = matches!(p, KnitrPiece::Source { .. });
        if is_source && !prev_source {
            groups += 1;
        }
        prev_source = is_source;
    }
    groups
}

/// The lines between the common prefix and the common suffix, oracle then candidate, each
/// capped so a report stays readable.
pub fn first_difference(oracle: &str, candidate: &str) -> String {
    let (a, b): (Vec<&str>, Vec<&str>) = (oracle.lines().collect(), candidate.lines().collect());
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let show = |lines: &[&str], sign: char| -> String {
        let mid = &lines[prefix..lines.len() - suffix];
        let mut s: String = mid
            .iter()
            .take(12)
            .map(|l| format!("{sign}{}\n", l.chars().take(160).collect::<String>()))
            .collect();
        if mid.len() > 12 {
            s.push_str(&format!("{sign}... {} more lines\n", mid.len() - 12));
        }
        s
    };
    format!(
        "@@ line {} @@\n{}{}",
        prefix + 1,
        show(&a, '-'),
        show(&b, '+')
    )
}

/// Every (fixture, format) pair, or the seeded subset when `KNR_SAMPLE=<n>` is set.
pub fn corpus_jobs() -> Vec<(Fixture, String)> {
    let m = CorpusManifest::load(&corpus_dir()).unwrap();
    let seed = 1;
    let fixtures = match std::env::var("KNR_SAMPLE")
        .ok()
        .and_then(|n| n.parse().ok())
    {
        Some(n) => m.sample(seed, n),
        None => m.generate(seed),
    };
    // `KNR_ONLY=<substring>` narrows the run to fixtures whose name contains it.
    let only = std::env::var("KNR_ONLY").ok();
    fixtures
        .into_iter()
        .filter(|f| only.as_ref().is_none_or(|o| f.name.contains(o.as_str())))
        .flat_map(|f| {
            let formats = f.formats.clone();
            formats.into_iter().map(move |fmt| (f.clone(), fmt))
        })
        .collect()
}

/// `R_LIBS` for a pinned knitr (`KNR_R_LIBS=<scratch lib dir>`, built by `scratch-libs.R`).
pub fn env_from_process() -> Vec<(String, String)> {
    std::env::var("KNR_R_LIBS")
        .map(|v| vec![("R_LIBS".to_string(), v)])
        .unwrap_or_default()
}

#[test]
fn corpus_report() {
    let Ok(path) = std::env::var("KNR_REPORT") else {
        return;
    };
    if !r_available() {
        eprintln!("SKIP: R with knitr not available");
        return;
    }
    let jobs = corpus_jobs();
    let env = env_from_process();
    let records = parallel(jobs.len(), |i| run_case(&jobs[i].0, &jobs[i].1, &env));
    std::fs::write(&path, serde_json::to_string_pretty(&records).unwrap()).unwrap();
    std::fs::write(format!("{path}.summary.txt"), summarize(&records)).unwrap();
}

#[test]
fn summary_counts_by_group_and_lists_the_unusual_cases() {
    let rec = |name: &str, group: &str, result: CaseResult| CaseRecord {
        name: name.into(),
        group: group.into(),
        format: "html".into(),
        result,
    };
    let compared = |class: &str, multi: bool| CaseResult::Compared {
        class: class.into(),
        multi_group: multi,
        diff: None,
        q1: None,
    };
    let text = summarize(&[
        rec("a/1", "a", compared("blocker", true)),
        rec("a/2", "a", compared("syntax-level", false)),
        rec("a/3", "a", compared("blocker", true)),
        rec(
            "b/1",
            "b",
            CaseResult::Fallback {
                reason: "child".into(),
            },
        ),
        rec(
            "b/2",
            "b",
            CaseResult::Aborted {
                message: "x".into(),
            },
        ),
        rec(
            "b/3",
            "b",
            CaseResult::Unavailable {
                package: "gifski".into(),
            },
        ),
    ]);
    assert!(text.starts_with("6 cases"), "{text}");
    assert!(
        text.contains("a: blocker (multi-group): 2, syntax-level: 1"),
        "{text}"
    );
    assert!(text.contains("b/1 [html]: child"), "{text}");
    assert!(text.contains("b/3 [html]: gifski"), "{text}");
}

#[test]
fn run_case_classifies_a_plain_chunk() {
    if !r_available() {
        eprintln!("SKIP: R with knitr not available");
        return;
    }
    let m = CorpusManifest::load(&corpus_dir()).unwrap();
    let f = m
        .generate(1)
        .into_iter()
        .find(|f| f.name == "s1/01_plain")
        .unwrap();
    let rec = run_case(&f, "html", &[]);
    assert!(
        matches!(rec.result, CaseResult::Compared { .. }),
        "{:?}",
        rec.result
    );
}

fn fixture_named(name: &str) -> Fixture {
    CorpusManifest::load(&corpus_dir())
        .unwrap()
        .generate(1)
        .into_iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("no fixture {name}"))
}

/// The chunk label as the header spells it: the first token of the text after the language.
fn scanner_label(attr_tail: &str) -> String {
    attr_tail
        .split([',', ' '])
        .next()
        .unwrap_or_default()
        .to_string()
}

#[test]
fn capture_one_unit_per_chunk_in_order() {
    if !r_available() {
        eprintln!("SKIP: R with knitr not available");
        return;
    }
    // nested (blockquote), list-nested and include=FALSE chunks, adjacent and first/last
    for name in ["s7/01_structure", "s1/02b_include_false_in_list"] {
        let f = fixture_named(name);
        let run = run_fixture_with_env(&f, "html", true, &[]).expect("run");
        let cells: Vec<String> = quarto_core::engine::units::scan_units(&f.text)
            .into_iter()
            .filter_map(|u| match u {
                quarto_core::engine::units::Unit::Cell(c) => Some(scanner_label(&c.attr_tail)),
                quarto_core::engine::units::Unit::Inline(_) => None,
            })
            .collect();
        let ordinals: Vec<u32> = run.chunks.iter().map(|c| c.ordinal).collect();
        let labels: Vec<&str> = run.chunks.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(
            ordinals,
            (1..=cells.len() as u32).collect::<Vec<_>>(),
            "{name}: ordinals are 1..n"
        );
        assert_eq!(
            labels, cells,
            "{name}: knitr\'s chunk order is the scanner\'s"
        );
    }
}

#[test]
fn capture_include_false_and_cache_hit_shapes() {
    if !r_available() {
        eprintln!("SKIP: R with knitr not available");
        return;
    }
    // S1: include=FALSE still yields the chunk\'s pieces (the presenter applies the switch).
    let run = run_fixture_with_env(&fixture_named("s1/02_include_false"), "html", true, &[])
        .expect("run");
    let hidden = run
        .chunks
        .iter()
        .find(|c| c.options.get("include").and_then(|v| v.as_bool()) == Some(false))
        .expect("an include=FALSE chunk is recorded");
    assert!(!hidden.pieces.is_empty());
    // S4: with capture on, cache is off, so a level-3 chunk reaches the hooks and yields
    // pieces, and the recorded option is 0 (knitr normalizes FALSE to 0).
    let run =
        run_fixture_with_env(&fixture_named("s1/14_cache_level3"), "html", true, &[]).expect("run");
    let cached = run
        .chunks
        .iter()
        .find(|c| c.options.get("cache").is_some())
        .expect("a chunk records its cache option");
    assert_eq!(cached.options["cache"], serde_json::json!(0));
    assert!(!cached.pieces.is_empty());
}
