//! OPR-0b: the gated runner for the parity harness (`opr_parity_harness`).
//!
//! `opr_parity_report` does nothing unless `OPR_PARITY_REPORT` is set: a path to write the
//! markdown report to, or `1` to print it to stderr. A full run renders every document twice and
//! takes many minutes, and R and Julia are absent from CI, so it is not part of any default run.
//!
//! ```text
//! OPR_PARITY_REPORT=/path/report.md cargo nextest run -p quarto-core \
//!   -E 'binary(integration) & test(opr_parity_runner::opr_parity_report)' --no-capture
//! ```
//!
//! Narrowing: `OPR_PARITY_ONLY=<substring>` keeps documents whose name contains it;
//! `OPR_PARITY_KNITR=0` leaves out the existing knitr corpus (the plan's own corpus still runs).
//!
//! With `OPR_PARITY_REPORT` set the run also ASSERTS (OPR-9d): it fails when any case is an
//! unallowlisted difference (or the routes disagree on success), and when an allowlist entry that
//! covers compared cases is needed by none of them (a stale entry). Without the variable the test
//! does nothing.

#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use super::julia_engine_e2e::julia_available;
use super::knitr_corpus_run::{materialize, parallel, r_available};
use super::knitr_corpus_support::{CorpusManifest, corpus_dir};
use super::opr_parity_harness::{
    Allowlist, Applied, Side, Verdict, compare, normalize_text, prepare_dir, render_prepared,
};

fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("opr-parity")
}

#[derive(Debug, Deserialize)]
struct Manifest {
    #[serde(default)]
    document: Vec<ManifestDoc>,
}

#[derive(Debug, Clone, Deserialize)]
struct ManifestDoc {
    name: String,
    dir: String,
    file: String,
    #[serde(default)]
    requires: Vec<String>,
    #[serde(default)]
    formats: Vec<String>,
    #[serde(default)]
    pending: String,
    renders: Option<usize>,
    #[serde(default)]
    expect_failure: bool,
    #[serde(default)]
    note: String,
}

/// One document under one format, ready to render.
struct Job {
    name: String,
    src: PathBuf,
    file: String,
    format: String,
    requires: Vec<String>,
    pending: String,
    renders: usize,
    expect_failure: bool,
    /// Keeps a materialized fixture's directory alive.
    _holder: Option<tempfile::TempDir>,
}

/// The formats whose output is text the harness can compare. `docx` and `epub` are zip files;
/// `typst` is compared through the `.typ` the harness asks `keep-typ` for.
const COMPARABLE: &[&str] = &["html", "gfm", "revealjs", "typst"];

/// The formats the plan's own manifest may list: the text formats above plus the zip formats the
/// harness opens (`opr_parity_zip`). A format outside this list is reported as not compared, never
/// rendered and then misreported as "both routes failed".
const EXTRACTABLE: &[&str] = &["html", "gfm", "revealjs", "typst", "docx", "pptx", "epub"];

fn plan_jobs() -> (Vec<Job>, Vec<String>) {
    let only = std::env::var("OPR_PARITY_ONLY").ok();
    let wanted = |name: &str| only.as_ref().is_none_or(|o| name.contains(o.as_str()));
    let mut jobs = Vec::new();
    let mut not_compared = Vec::new();

    let text = std::fs::read_to_string(corpus_root().join("manifest.toml")).unwrap();
    let manifest: Manifest = toml::from_str(&text).unwrap();
    for d in manifest.document {
        if !wanted(&d.name) {
            continue;
        }
        let _ = &d.note;
        for format in &d.formats {
            if !EXTRACTABLE.contains(&format.as_str()) {
                not_compared.push(format!("{} [{format}] (no extractor)", d.name));
                continue;
            }
            jobs.push(Job {
                name: d.name.clone(),
                src: corpus_root().join(&d.dir),
                file: d.file.clone(),
                format: format.clone(),
                requires: d.requires.clone(),
                pending: d.pending.clone(),
                renders: d.renders.unwrap_or(1),
                expect_failure: d.expect_failure,
                _holder: None,
            });
        }
    }

    if std::env::var("OPR_PARITY_KNITR").is_ok_and(|v| v == "0") {
        return (jobs, not_compared);
    }
    let knitr = CorpusManifest::load(&corpus_dir()).unwrap();
    for f in knitr.generate(1) {
        if !wanted(&f.name) {
            continue;
        }
        for format in &f.formats {
            if !COMPARABLE.contains(&format.as_str()) {
                not_compared.push(format!("{} [{format}]", f.name));
                continue;
            }
            let (dir, doc) = materialize(&f);
            // `q2 render` takes no `.Rmd` input; the same text goes in as `doc.qmd`.
            let mut file = doc.file_name().unwrap().to_string_lossy().into_owned();
            if file.ends_with(".Rmd") {
                std::fs::rename(&doc, dir.path().join("doc.qmd")).unwrap();
                file = "doc.qmd".into();
            }
            jobs.push(Job {
                name: format!("knitr-corpus/{}", f.name),
                src: dir.path().to_path_buf(),
                file,
                format: format.clone(),
                requires: vec!["r".into()],
                pending: String::new(),
                renders: 1,
                expect_failure: f.expect_abort,
                _holder: Some(dir),
            });
        }
    }
    (jobs, not_compared)
}

/// `Ok` when the tool is usable, else why not.
fn have(requirement: &str) -> Result<(), String> {
    let ok = match requirement {
        "r" => r_available(),
        "julia" => julia_available() && quarto_core::engine::ts_process::node_available_for_tests(),
        "uv" => Command::new("uv")
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success()),
        // Checked per document, where the environment files are copied.
        "plots" => true,
        other => return Err(format!("unknown requirement `{other}`")),
    };
    if ok {
        Ok(())
    } else {
        Err(format!("`{requirement}` is not available on this machine"))
    }
}

/// The default Julia environment's `Project.toml` and `Manifest.toml` when it has `Plots`.
fn plots_environment() -> Option<(PathBuf, PathBuf)> {
    let depot = std::env::var_os("JULIA_DEPOT_PATH")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".julia")))?;
    let mut envs: Vec<PathBuf> = std::fs::read_dir(depot.join("environments"))
        .ok()?
        .flatten()
        .map(|e| e.path())
        .collect();
    envs.sort();
    envs.into_iter().rev().find_map(|dir| {
        let project = dir.join("Project.toml");
        let manifest = dir.join("Manifest.toml");
        let has_plots = std::fs::read_to_string(&project)
            .is_ok_and(|t| t.lines().any(|l| l.trim_start().starts_with("Plots")));
        (has_plots && manifest.exists()).then_some((project, manifest))
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    Compared {
        verdict: Verdict,
        /// The first run differed from the confirming run (transient noise); what it showed.
        unstable: Option<String>,
    },
    /// Both routes failed; whether with the same (normalized) message.
    BothFailed {
        same: bool,
        message: String,
    },
    /// One route failed and the other did not.
    OneFailed {
        plain: String,
        recorded: String,
    },
    Skipped(String),
}

#[derive(Debug, Clone)]
struct Case {
    name: String,
    format: String,
    pending: String,
    expect_failure: bool,
    outcome: Outcome,
}

fn first_lines(message: &str, dirs: &[&Path]) -> String {
    normalize_text(message, dirs)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(3)
        .collect::<Vec<_>>()
        .join(" | ")
}

/// Render `renders` times per route in one scratch directory each (so a second render sees the
/// first one's caches) and compare render by render.
fn measure(job: &Job, src: &Path, allowlist: &Allowlist) -> Vec<(usize, Outcome)> {
    let root = tempfile::tempdir().expect("scratch");
    let prepared = |recorded| prepare_dir(src, &job.file, &job.format, recorded, root.path());
    let (plain_dir, rec_dir) = match (prepared(false), prepared(true)) {
        (Ok(p), Ok(r)) => (p, r),
        (Err(e), _) | (_, Err(e)) => return vec![(0, Outcome::Skipped(format!("setup: {e}")))],
    };
    let mut out = Vec::new();
    for i in 0..job.renders {
        let plain = render_prepared(&plain_dir, &job.file, &job.format);
        let recorded = render_prepared(&rec_dir, &job.file, &job.format);
        let outcome = match (plain, recorded) {
            (Ok(p), Ok(r)) => Outcome::Compared {
                verdict: compare(&job.name, &p, &r, allowlist),
                unstable: None,
            },
            (Err(p), Err(r)) => {
                let (pm, rm) = (
                    first_lines(&p, &[&plain_dir, &rec_dir]),
                    first_lines(&r, &[&plain_dir, &rec_dir]),
                );
                Outcome::BothFailed {
                    same: pm == rm,
                    message: pm,
                }
            }
            (p, r) => Outcome::OneFailed {
                plain: p
                    .err()
                    .map(|e| first_lines(&e, &[&plain_dir]))
                    .unwrap_or_default(),
                recorded: r
                    .err()
                    .map(|e| first_lines(&e, &[&rec_dir]))
                    .unwrap_or_default(),
            },
        };
        out.push((i, outcome));
    }
    out
}

fn skip_reason(outcomes: &[(usize, Outcome)]) -> Option<String> {
    // R reports a missing package in its failure message; that is an environment gap, not a
    // difference between routes.
    for (_, o) in outcomes {
        let message = match o {
            Outcome::BothFailed { message, .. } => message,
            Outcome::OneFailed { plain, recorded } => {
                if plain.contains("no package called") {
                    plain
                } else {
                    recorded
                }
            }
            _ => continue,
        };
        if let Some(rest) = message.split("no package called").nth(1) {
            return Some(format!(
                "R package not installed:{}",
                rest.split('|').next().unwrap_or("")
            ));
        }
    }
    None
}

fn run_job(job: &Job, allowlist: &Allowlist) -> Vec<Case> {
    let case = |label: &str, outcome| Case {
        name: format!("{}{label}", job.name),
        format: job.format.clone(),
        pending: job.pending.clone(),
        expect_failure: job.expect_failure,
        outcome,
    };
    for requirement in &job.requires {
        if let Err(why) = have(requirement) {
            return vec![case("", Outcome::Skipped(why))];
        }
    }
    // A document that needs Plots gets the default environment's files beside it.
    let mut src = job.src.clone();
    let mut _plots_copy = None;
    if job.requires.iter().any(|r| r == "plots") {
        let Some((project, manifest)) = plots_environment() else {
            return vec![case(
                "",
                Outcome::Skipped("Julia `Plots` is not installed in a default environment".into()),
            )];
        };
        let dir = tempfile::tempdir().expect("scratch");
        super::opr_parity_harness::copy_tree(&job.src, dir.path()).unwrap();
        std::fs::copy(project, dir.path().join("Project.toml")).unwrap();
        std::fs::copy(manifest, dir.path().join("Manifest.toml")).unwrap();
        src = dir.path().to_path_buf();
        _plots_copy = Some(dir);
    }

    let mut first = measure(job, &src, allowlist);
    if let Some(reason) = skip_reason(&first) {
        return vec![case("", Outcome::Skipped(reason))];
    }
    // A difference is rendered again from scratch: a transient (the Julia server's first run can
    // print a stray stderr line) does not repeat, a route difference does.
    let needs_confirm = first.iter().any(|(_, o)| {
        matches!(
            o,
            Outcome::Compared {
                verdict: Verdict::Blocker { .. },
                ..
            } | Outcome::OneFailed { .. }
        )
    });
    if needs_confirm {
        let second = measure(job, &src, allowlist);
        for ((_, a), (_, b)) in first.iter_mut().zip(second) {
            if *a != b {
                let shown = match &*a {
                    Outcome::Compared {
                        verdict: Verdict::Blocker { diff },
                        ..
                    } => diff.clone(),
                    other => format!("{other:?}"),
                };
                *a = match b {
                    Outcome::Compared { verdict, .. } => Outcome::Compared {
                        verdict,
                        unstable: Some(shown),
                    },
                    other => other,
                };
            }
        }
    }
    let multi = first.len() > 1;
    first
        .into_iter()
        .map(|(i, o)| {
            let label = if multi {
                if i == 0 { " (miss)" } else { " (hit)" }
            } else {
                ""
            };
            case(label, o)
        })
        .collect()
}

/// A failure before any engine runs: the document does not parse, or names no engine. An
/// execution failure is also wrapped in a `ParseError` by the render path, so exclude it.
fn is_rejection(message: &str) -> bool {
    !message.contains("Execution failed in")
        && !message.contains("is registered but its runtime is not available")
        && (message.contains("ParseError") || message.contains("Can't determine execution engine"))
}

fn render_report(cases: &[Case], not_compared: &[String], allowlist: &Allowlist) -> String {
    let mut identical = Vec::new();
    let mut allowlisted: Vec<(&Case, &Vec<Applied>)> = Vec::new();
    let mut blockers: Vec<(&Case, &String, &Option<String>)> = Vec::new();
    let mut failed_same = Vec::new();
    let mut one_failed = Vec::new();
    let mut skipped = Vec::new();
    for c in cases {
        match &c.outcome {
            Outcome::Compared {
                verdict: Verdict::Identical,
                ..
            } => identical.push(c),
            Outcome::Compared {
                verdict: Verdict::Allowlisted { applied: rules },
                ..
            } => allowlisted.push((c, rules)),
            Outcome::Compared {
                verdict: Verdict::Blocker { diff },
                unstable,
            } => blockers.push((c, diff, unstable)),
            Outcome::BothFailed { same: true, .. } => failed_same.push(c),
            Outcome::BothFailed { same: false, .. } | Outcome::OneFailed { .. } => {
                one_failed.push(c)
            }
            Outcome::Skipped(_) => skipped.push(c),
        }
    }
    let tag = |c: &Case| {
        let mut t = format!("{} [{}]", c.name, c.format);
        if !c.pending.is_empty() {
            t.push_str(&format!(" (pending {})", c.pending));
        }
        t
    };
    let mut out = String::new();
    out.push_str("## Counts\n\n| Class | Cases |\n|---|---:|\n");
    out.push_str(&format!("| identical | {} |\n", identical.len()));
    out.push_str(&format!(
        "| allowlisted (proposed or signed entries) | {} |\n",
        allowlisted.len()
    ));
    out.push_str(&format!(
        "| unallowlisted difference (blocker) | {} |\n",
        blockers.len()
    ));
    let is_rejected = |c: &&Case| {
        matches!(&c.outcome, Outcome::BothFailed { message, .. }
            if is_rejection(message))
    };
    let (rejected, failed_same): (Vec<&Case>, Vec<&Case>) =
        failed_same.into_iter().partition(is_rejected);
    out.push_str(&format!(
        "| document rejected by q2 before any engine runs (same on both routes) | {} |\n",
        rejected.len()
    ));
    out.push_str(&format!(
        "| both routes failed at execution, same message | {} |\n",
        failed_same.len()
    ));
    out.push_str(&format!(
        "| routes disagree on success, or failed differently | {} |\n",
        one_failed.len()
    ));
    out.push_str(&format!("| skipped | {} |\n", skipped.len()));
    out.push_str(&format!(
        "| formats not compared (zip output) | {} |\n\n",
        not_compared.len()
    ));

    out.push_str("## Unallowlisted differences\n\n");
    if blockers.is_empty() && one_failed.is_empty() {
        out.push_str("None.\n\n");
    }
    for (c, diff, unstable) in &blockers {
        out.push_str(&format!("### {}\n\n```text\n{}```\n\n", tag(c), diff));
        if let Some(first) = unstable {
            out.push_str(&format!(
                "The first run differed from this confirming run:\n\n```text\n{first}```\n\n"
            ));
        }
    }
    for c in &one_failed {
        out.push_str(&format!(
            "### {}\n\n```text\n{:?}\n```\n\n",
            tag(c),
            c.outcome
        ));
    }

    out.push_str("## Allowlisted (proposed or signed entries)\n\n");
    for entry in &allowlist.entries {
        let touched: Vec<(&Case, &Applied)> = allowlisted
            .iter()
            .flat_map(|(c, applied)| {
                applied
                    .iter()
                    .filter(|a| a.entry == entry.name)
                    .map(move |a| (*c, a))
            })
            .collect();
        let (pf, rf) = touched.iter().fold((0, 0), |(p, r), (_, a)| {
            (p + a.plain_fires, r + a.recorded_fires)
        });
        out.push_str(&format!(
            "- `{}` ({}, {}, {}): explains {} cases (fires: plain {pf}, recorded {rf})\n",
            entry.rule,
            entry.name,
            entry.coverage(),
            entry.status,
            touched.len()
        ));
        for (c, a) in &touched {
            out.push_str(&format!(
                "  - {}: plain {}, recorded {}{}\n",
                tag(c),
                a.plain_fires,
                a.recorded_fires,
                if a.needed { "" } else { " (NOT needed)" }
            ));
        }
    }
    out.push('\n');
    for (c, applied) in &allowlisted {
        let names: Vec<&str> = applied.iter().map(|a| a.rule.as_str()).collect();
        out.push_str(&format!("- {}: {}\n", tag(c), names.join(", ")));
    }
    out.push_str("\n## Identical\n\n");
    for c in &identical {
        out.push_str(&format!("- {}\n", tag(c)));
    }
    out.push_str("\n## Documents q2 rejects before any engine runs\n\n");
    out.push_str(
        "These never reach a route, so they say nothing about parity. Grouped by message:\n\n",
    );
    let mut groups: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for c in &rejected {
        let Outcome::BothFailed { message, .. } = &c.outcome else {
            continue;
        };
        let key: String = message.chars().take(160).collect();
        groups.entry(key).or_default().push(tag(c));
    }
    for (message, names) in &groups {
        out.push_str(&format!(
            "- {} documents: `{}`\n",
            names.len(),
            message.replace('`', "'")
        ));
    }
    out.push_str("\n## Both routes failed at execution\n\n");
    for c in &failed_same {
        let Outcome::BothFailed { message, .. } = &c.outcome else {
            continue;
        };
        let expected = if c.expect_failure {
            "expected"
        } else {
            "NOT expected"
        };
        out.push_str(&format!("- {} ({expected}): {message}\n", tag(c)));
    }
    out.push_str("\n## Skipped\n\n");
    for c in &skipped {
        let Outcome::Skipped(why) = &c.outcome else {
            continue;
        };
        out.push_str(&format!("- {}: {why}\n", tag(c)));
    }
    out.push_str("\n## Formats not compared\n\n");
    out.push_str(&not_compared.join("\n"));
    out.push('\n');
    out
}

/// What makes the gate fail: an unallowlisted difference, routes that disagree on success, and a
/// stale entry (one that covers at least one compared case and is needed by none). An entry with
/// no compared case in scope (skipped tools, a narrowed run) proves nothing, so it is not stale.
/// `pending` cases are not exempt: the tag only labels them in the report.
fn gate_failures(cases: &[Case], allowlist: &Allowlist) -> Vec<String> {
    let mut failures = Vec::new();
    for c in cases {
        let label = format!("{} [{}]", c.name, c.format);
        match &c.outcome {
            Outcome::Compared {
                verdict: Verdict::Blocker { diff },
                ..
            } => {
                let first = diff.lines().next().unwrap_or("");
                failures.push(format!("unallowlisted difference: {label}: {first}"));
            }
            Outcome::OneFailed { .. } | Outcome::BothFailed { same: false, .. } => {
                failures.push(format!("routes disagree on success: {label}"));
            }
            _ => {}
        }
    }
    for entry in &allowlist.entries {
        let mut compared_in_scope = 0;
        let mut needed = false;
        for c in cases.iter().filter(|c| entry.covers(&c.name)) {
            let Outcome::Compared { verdict, .. } = &c.outcome else {
                continue;
            };
            compared_in_scope += 1;
            if let Verdict::Allowlisted { applied } = verdict {
                needed |= applied.iter().any(|a| a.entry == entry.name && a.needed);
            }
        }
        if compared_in_scope > 0 && !needed {
            failures.push(format!(
                "stale entry `{}` ({}): none of {compared_in_scope} compared cases needs it",
                entry.name,
                entry.coverage()
            ));
        }
    }
    failures
}

#[test]
fn opr_parity_report() {
    let Ok(target) = std::env::var("OPR_PARITY_REPORT") else {
        return;
    };
    let allowlist = Allowlist::load(&corpus_root().join("opr-parity-allowlist.toml")).unwrap();
    let (jobs, not_compared) = plan_jobs();
    // Julia and marimo share one server per machine: one at a time. The rest run in parallel.
    let (serial, concurrent): (Vec<usize>, Vec<usize>) =
        (0..jobs.len()).partition(|&i| jobs[i].requires.iter().any(|r| r == "julia" || r == "uv"));
    let mut cases: Vec<Case> = std::thread::scope(|s| {
        let serial_run = s.spawn(|| {
            serial
                .iter()
                .flat_map(|&i| run_job(&jobs[i], &allowlist))
                .collect::<Vec<_>>()
        });
        let parallel_run = parallel(concurrent.len(), |k| {
            run_job(&jobs[concurrent[k]], &allowlist)
        });
        let mut all: Vec<Case> = parallel_run.into_iter().flatten().collect();
        all.extend(serial_run.join().unwrap());
        all
    });
    cases.sort_by(|a, b| (&a.name, &a.format).cmp(&(&b.name, &b.format)));
    let report = render_report(&cases, &not_compared, &allowlist);
    if target == "1" {
        eprintln!("{report}");
    } else {
        std::fs::write(&target, &report).unwrap();
        eprintln!("wrote {target}");
    }
    let failures = gate_failures(&cases, &allowlist);
    assert!(
        failures.is_empty(),
        "the parity gate failed:\n{}",
        failures.join("\n")
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plans_own_manifest_parses_and_names_real_documents() {
        let text = std::fs::read_to_string(corpus_root().join("manifest.toml")).unwrap();
        let manifest: Manifest = toml::from_str(&text).unwrap();
        assert!(manifest.document.len() >= 30, "{}", manifest.document.len());
        for d in &manifest.document {
            assert!(
                corpus_root().join(&d.dir).join(&d.file).is_file(),
                "{} names a missing file {}/{}",
                d.name,
                d.dir,
                d.file
            );
            assert!(!d.formats.is_empty(), "{} has no formats", d.name);
            assert!(!d.note.is_empty(), "{} has no note", d.name);
        }
        let mut names: Vec<_> = manifest.document.iter().map(|d| d.name.clone()).collect();
        names.sort();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len(), "duplicate document names");
    }

    #[test]
    fn the_seeded_allowlist_parses_and_each_entry_is_proposed_or_signed() {
        let list = Allowlist::load(&corpus_root().join("opr-parity-allowlist.toml")).unwrap();
        // OPR-7f removed the five proposed Julia entries (wrapper id, data-quarto, output
        // execution count, figure output class, figure file names): the engine route and the
        // presenter now agree on them, so only the three signed entries were left. OPR-9e adds one
        // proposed entry: the pinned epub interleaving. OPR-9f adds five more: the interleaving
        // normalizer for presentation-attrs, records-plot, fig-subcap and capture-facts, and
        // capture-facts' pinned difference. Gordon signed all six on 2026-11-11.
        assert_eq!(list.entries.len(), 9);
        assert!(
            list.entries
                .iter()
                .all(|e| e.status == "proposed" || e.status == "signed")
        );
        // A signed entry says who signed it and when, in its reason: the first three on 2026-11-09
        // (OPR-7d start condition), the six interleaving entries on 2026-11-11 (OPR-9f).
        let mut signed: Vec<&str> = list
            .entries
            .iter()
            .filter(|e| e.status == "signed")
            .map(|e| e.rule.as_str())
            .collect();
        signed.sort_unstable();
        assert_eq!(
            signed,
            [
                "julia-code-leading-newline",
                "julia-interleaved-cell",
                "julia-interleaved-cell",
                "julia-interleaved-cell",
                "julia-interleaved-cell",
                "knitr-adjacent-blockquotes",
                "knitr-empty-cell-wrapper",
                "pinned-difference",
                "pinned-difference"
            ]
        );
        assert!(
            list.entries
                .iter()
                .filter(|e| e.status == "signed")
                .all(|e| {
                    e.reason.contains("Signed: Gordon, 2026-11-09")
                        || e.reason.contains("Signed: Gordon, 2026-11-11")
                })
        );
        // The subfigure-group references are a blocker, not an allowlist candidate.
        assert!(
            list.entries
                .iter()
                .all(|e| !e.reason.contains("fig-subcap") || e.reason.contains("not"))
        );
    }

    #[test]
    fn a_failed_execution_is_not_a_rejection() {
        assert!(is_rejection(
            "Parse(ParseError { diagnostics: [ ... Q-2-36 ..."
        ));
        assert!(is_rejection(
            "Other(\"Can't determine execution engine for x\")"
        ));
        assert!(!is_rejection(
            "Parse(ParseError { title: \"Execution failed in julia: ... intentional failure"
        ));
        assert!(!is_rejection(
            "Parse(ParseError { title: \"Engine 'jupyter' is registered but its runtime is not available."
        ));
    }

    #[test]
    fn the_report_names_every_class() {
        let list = Allowlist::default();
        let case = |name: &str, outcome| Case {
            name: name.into(),
            format: "html".into(),
            pending: String::new(),
            expect_failure: false,
            outcome,
        };
        let cases = vec![
            case(
                "a",
                Outcome::Compared {
                    verdict: Verdict::Identical,
                    unstable: None,
                },
            ),
            case(
                "b",
                Outcome::Compared {
                    verdict: Verdict::Blocker {
                        diff: "- x\n".into(),
                    },
                    unstable: None,
                },
            ),
            case("c", Outcome::Skipped("no R".into())),
        ];
        let report = render_report(&cases, &[], &list);
        assert!(report.contains("| identical | 1 |"), "{report}");
        assert!(
            report.contains("| unallowlisted difference (blocker) | 1 |"),
            "{report}"
        );
        assert!(report.contains("c [html]: no R"), "{report}");
        let _: Option<&Side> = None;
    }

    fn compared(name: &str, verdict: Verdict) -> Case {
        Case {
            name: name.into(),
            format: "html".into(),
            pending: String::new(),
            expect_failure: false,
            outcome: Outcome::Compared {
                verdict,
                unstable: None,
            },
        }
    }

    fn applied(entry: &str, needed: bool) -> Applied {
        Applied {
            entry: entry.into(),
            rule: "julia-code-leading-newline".into(),
            plain_fires: 1,
            recorded_fires: 1,
            needed,
        }
    }

    fn entry_list() -> Allowlist {
        Allowlist::parse(
            "[[entry]]\nname='nl'\nrule='julia-code-leading-newline'\nscope='julia/'\nstatus='proposed'\nreason='r'\n",
        )
        .unwrap()
    }

    #[test]
    fn the_gate_passes_when_every_case_is_identical_or_allowlisted_and_needed() {
        let cases = vec![
            compared("md/a", Verdict::Identical),
            compared(
                "julia/b",
                Verdict::Allowlisted {
                    applied: vec![applied("nl", true)],
                },
            ),
        ];
        assert_eq!(gate_failures(&cases, &entry_list()), Vec::<String>::new());
    }

    #[test]
    fn the_gate_fails_on_an_unallowlisted_blocker_and_names_it() {
        let cases = vec![compared(
            "julia/b",
            Verdict::Blocker {
                diff: "- x\n".into(),
            },
        )];
        let failures = gate_failures(&cases, &Allowlist::default());
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert!(failures[0].contains("julia/b"), "{failures:?}");
    }

    #[test]
    fn the_gate_fails_when_the_routes_disagree_on_success() {
        let mut case = compared("julia/c", Verdict::Identical);
        case.outcome = Outcome::OneFailed {
            plain: String::new(),
            recorded: "boom".into(),
        };
        assert_eq!(gate_failures(&[case], &Allowlist::default()).len(), 1);
    }

    #[test]
    fn the_gate_fails_on_a_stale_entry_and_not_on_a_needed_one() {
        let ids = vec![
            compared("julia/a", Verdict::Identical),
            compared(
                "julia/b",
                Verdict::Allowlisted {
                    applied: vec![applied("nl", false)],
                },
            ),
        ];
        let failures = gate_failures(&ids, &entry_list());
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert!(
            failures[0].contains("stale") && failures[0].contains("nl"),
            "{failures:?}"
        );
        // One document that needs it is enough.
        let mut cases = ids;
        cases.push(compared(
            "julia/c",
            Verdict::Allowlisted {
                applied: vec![applied("nl", true)],
            },
        ));
        assert!(gate_failures(&cases, &entry_list()).is_empty());
    }

    #[test]
    fn an_entry_with_no_compared_case_in_scope_is_not_called_stale() {
        // Skipped (no Julia) or out-of-scope cases prove nothing about the entry.
        let mut skipped = compared("julia/a", Verdict::Identical);
        skipped.outcome = Outcome::Skipped("no julia".into());
        let other = compared("md/x", Verdict::Identical);
        assert!(gate_failures(&[skipped, other], &entry_list()).is_empty());
    }

    #[test]
    fn the_plans_manifest_lists_only_formats_the_harness_can_read() {
        let text = std::fs::read_to_string(corpus_root().join("manifest.toml")).unwrap();
        let manifest: Manifest = toml::from_str(&text).unwrap();
        for d in &manifest.document {
            for f in &d.formats {
                assert!(EXTRACTABLE.contains(&f.as_str()), "{} lists `{f}`", d.name);
            }
        }
        // And each extractable format maps to an output extension the harness reads.
        use crate::opr_parity_harness::has_extractor;
        for ext in ["html", "md", "typ", "docx", "pptx", "epub"] {
            assert!(has_extractor(ext), "{ext}");
        }
        assert!(!has_extractor("pdf") && !has_extractor("odt"));
    }
}
