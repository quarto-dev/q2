//! KNR-3 slice 8, end to end through the real `KnitrEngine`: a record written from a corpus
//! fixture carries each structured cell's header keys, and the same document with its `#|` lines
//! deleted reads as stale per unit, while the unchanged document stays fresh. Needs R with knitr; a missing R is a skip.

#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;

use quarto_core::engine::units::{Unit, scan_units};
use quarto_core::engine::{ExecutionContext, KnitrEngine};
use quarto_core::record::{
    AuthorTexts, HEADER_KEYS_KEY, Record, UnitKey, UnitKind, coalesce, fill_ids, lost_header_keys,
    match_units, record_is_stale,
};

use super::knitr_corpus_run::{materialize, r_available};
use super::knitr_corpus_runner::corpus_jobs;
use super::knitr_corpus_support::Fixture;

/// The record the engine writes for `f` under html.
fn record_of(f: &Fixture) -> Record {
    let (dir, doc_path) = materialize(f);
    let mut units = scan_units(&f.text);
    let texts = AuthorTexts::from_scanned(&units);
    fill_ids(&mut units, &texts).unwrap();
    let ctx = ExecutionContext::new(
        dir.path().to_path_buf(),
        PathBuf::from(dir.path()),
        doc_path,
        "html",
    )
    .with_owned_languages(vec!["r".to_string(), "bash".to_string()])
    .with_units(units.clone())
    .with_author_texts(texts)
    .with_want_record(true)
    .with_quiet(true);
    let result = quarto_core::engine::ExecutionEngine::execute(&KnitrEngine::new(), &f.text, &ctx)
        .expect("the engine runs");
    let engine = result.record.expect("a record was wanted");
    coalesce(&[engine], &units).unwrap()
}

/// The first YAML-form fixture of `option`.
fn fixture_for(option: &str) -> Fixture {
    let prefix = format!("options/{option}/yaml");
    corpus_jobs()
        .into_iter()
        .map(|(f, _)| f)
        .find(|f| f.name.starts_with(&prefix))
        .unwrap_or_else(|| panic!("no fixture under {prefix}"))
}

fn without_header(text: &str) -> String {
    let kept: Vec<&str> = text.lines().filter(|l| !l.starts_with("#|")).collect();
    format!("{}\n", kept.join("\n"))
}

/// The unit keys the splice builds for `text` under the record\'s knitr declaration.
fn keys_for(record: &Record, text: &str) -> Vec<UnitKey> {
    let entry = &record.metadata.quarto_engines[0];
    let mut units = scan_units(text);
    let texts = AuthorTexts::from_scanned(&units);
    fill_ids(&mut units, &texts).unwrap();
    units
        .iter()
        .zip(&texts.0)
        .map(|(u, t)| {
            let (kind, tail) = match u {
                Unit::Cell(c) => (UnitKind::Cell, c.attr_tail.as_str()),
                Unit::Inline(_) => (UnitKind::Inline, ""),
            };
            UnitKey::from_author_text_with_profiles(
                &entry.hash_definition,
                kind,
                u.lang(),
                &t.text,
                tail,
                "html",
            )
            .unwrap()
        })
        .collect()
}

/// The per-unit judgment the splice makes (`match_units`, then `lost_header_keys`): the
/// unchanged document keeps every cell matched; the header-deleted one matches the same cells by
/// hash (the option is not in the structured hash) and then demotes them.
///
/// The whole-document judgment is `a_real_knitr_record_is_fresh_unchanged_and_stale_without_its_header`.
#[test]
fn a_real_knitr_record_demotes_a_unit_whose_header_key_was_deleted() {
    if !r_available() {
        eprintln!("SKIP: R with knitr not available");
        return;
    }
    for option in ["echo", "fig.cap"] {
        let f = fixture_for(option);
        let record = record_of(&f);
        let recorded: Vec<_> = record
            .cells
            .iter()
            .filter_map(|c| c.metadata.extra.get(HEADER_KEYS_KEY))
            .collect();
        eprintln!("{}: recorded header keys {recorded:?}", f.name);
        assert!(
            recorded.iter().any(|k| !k.as_array().unwrap().is_empty()),
            "{}: a cell recorded the header it was written with",
            f.name
        );

        let keys = keys_for(&record, &f.text);
        let m = match_units(&keys, &record.cells);
        assert!(m.assignment.iter().any(Option::is_some), "{}", f.name);
        assert!(lost_header_keys(&m, &keys, &record.cells).is_empty());

        let deleted = without_header(&f.text);
        assert_ne!(deleted, f.text, "{}: the fixture has a header", f.name);
        let keys = keys_for(&record, &deleted);
        let m = match_units(&keys, &record.cells);
        let lost = lost_header_keys(&m, &keys, &record.cells);
        eprintln!(
            "{}: after deleting the header, matched {:?}, demoted units {lost:?}",
            f.name, m.assignment
        );
        // A presentation key is outside the structured hash, so the unit still matches
        // and the predicate is what demotes it; a hashed key already fails the match.
        let demoted_or_unmatched = m
            .assignment
            .iter()
            .enumerate()
            .filter(|(_, a)| a.is_none())
            .count()
            + lost.len();
        assert!(demoted_or_unmatched > 0, "{}: nothing went stale", f.name);
    }
}

/// The whole-document `record_is_stale` on a real knitr record (KNR-3 slice 9): the unchanged
/// document is fresh (it was stale before the check hashed profiled cells under their profile),
/// and the same document without its `#|` header is stale, through the slice 8 predicate for a
/// presentation key and through the hash for an execution key.
#[test]
fn a_real_knitr_record_is_fresh_unchanged_and_stale_without_its_header() {
    if !r_available() {
        eprintln!("SKIP: R with knitr not available");
        return;
    }
    let metadata = std::collections::BTreeMap::new();
    for option in ["echo", "fig.cap"] {
        let f = fixture_for(option);
        let record = record_of(&f);
        let stale = |text: &str| record_is_stale(&record, text, &[], &metadata).unwrap();
        assert!(!stale(&f.text), "{}: unchanged reads fresh", f.name);
        assert!(
            stale(&without_header(&f.text)),
            "{}: header deleted reads stale",
            f.name
        );
    }
}
