//! KNR-1 Phase 1D control run and the two real-R gates. Real R and knitr; skips without them.

#![cfg(not(target_arch = "wasm32"))]

use super::knitr_corpus_normalizer::normalize;
use super::knitr_corpus_run::{Outcome, parallel, r_available, run_fixture};
use super::knitr_corpus_support::{CorpusManifest, corpus_dir};

/// Every (fixture, format) pair of the corpus.
fn jobs() -> Vec<(super::knitr_corpus_support::Fixture, String)> {
    let m = CorpusManifest::load(&corpus_dir()).unwrap();
    m.generate(1)
        .into_iter()
        .flat_map(|f| {
            let formats = f.formats.clone();
            formats.into_iter().map(move |fmt| (f.clone(), fmt))
        })
        .collect()
}

#[test]
fn control_run_is_identical_after_normalizer() {
    if !r_available() {
        eprintln!("SKIP: R with knitr not available");
        return;
    }
    let jobs = jobs();
    let results = parallel(jobs.len(), |i| {
        let (f, fmt) = &jobs[i];
        let a = Outcome::from_run(run_fixture(f, fmt, false));
        let b = Outcome::from_run(run_fixture(f, fmt, false));
        (a, b)
    });
    let mut differences = Vec::new();
    let (mut compared, mut aborted, mut unavailable) = (0, 0, 0);
    for ((f, fmt), (a, b)) in jobs.iter().zip(results) {
        match (a, b) {
            (Outcome::Ran(a), Outcome::Ran(b)) => {
                compared += 1;
                let (na, nb) = (
                    normalize(&a.markdown, a.doc_dir()),
                    normalize(&b.markdown, b.doc_dir()),
                );
                if na != nb {
                    differences.push(format!(
                        "== {} [{fmt}]\n--- run A\n{na}\n--- run B\n{nb}\n",
                        f.name
                    ));
                }
            }
            // Both runs abort the same way: a consistent outcome (S1 07, 08 and fixtures
            // such as `eval=1` whose code needs an earlier expression).
            (Outcome::Aborted(a), Outcome::Aborted(b)) if a == b => aborted += 1,
            (Outcome::Unavailable(_), Outcome::Unavailable(_)) => unavailable += 1,
            _ => differences.push(format!(
                "== {} [{fmt}] the two runs ended differently\n",
                f.name
            )),
        }
    }
    eprintln!(
        "control run: {compared} compared, {aborted} aborted identically, {unavailable} skipped (missing package)"
    );
    if let Ok(path) = std::env::var("KNR_CONTROL_REPORT") {
        std::fs::write(path, differences.join("\n")).unwrap();
    }
    assert!(
        differences.is_empty(),
        "{} of {} runs differ after the normalizer (set KNR_CONTROL_REPORT=<file> for details)",
        differences.len(),
        jobs.len()
    );
}

#[test]
fn capture_does_not_change_live_output() {
    if !r_available() {
        eprintln!("SKIP: R with knitr not available");
        return;
    }
    let jobs = jobs();
    let results = parallel(jobs.len(), |i| {
        let (f, fmt) = &jobs[i];
        let off = Outcome::from_run(run_fixture(f, fmt, false));
        let on = Outcome::from_run(run_fixture(f, fmt, true));
        (off, on)
    });
    let mut differences = Vec::new();
    for ((f, fmt), (off, on)) in jobs.iter().zip(results) {
        match (off, on) {
            (Outcome::Ran(off), Outcome::Ran(on)) => {
                let (n_off, n_on) = (
                    normalize(&off.markdown, off.doc_dir()),
                    normalize(&on.markdown, on.doc_dir()),
                );
                if n_off != n_on {
                    differences.push(format!(
                        "== {} [{fmt}]\n--- capture off\n{n_off}\n--- capture on\n{n_on}\n",
                        f.name
                    ));
                }
            }
            (Outcome::Aborted(a), Outcome::Aborted(b)) if a == b => {}
            (Outcome::Unavailable(_), Outcome::Unavailable(_)) => {}
            _ => differences.push(format!(
                "== {} [{fmt}] capture on and off ended differently\n",
                f.name
            )),
        }
    }
    if let Ok(path) = std::env::var("KNR_CAPTURE_REPORT") {
        std::fs::write(path, differences.join("\n")).unwrap();
    }
    assert!(
        differences.is_empty(),
        "{} of {} runs differ with capture on (set KNR_CAPTURE_REPORT=<file> for details)",
        differences.len(),
        jobs.len()
    );
}
