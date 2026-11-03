//! KNR-3 slice 4: the corpus candidate built the way the product builds it. The real
//! `KnitrEngine` runs the fixture with a record wanted; the record is written, read back, and
//! presented with `present_document` (the product path, not the runner's direct one). The
//! engine's own markdown is the live oracle.

#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;

use quarto_core::engine::units::scan_units;
use quarto_core::engine::{ExecutionContext, KnitrEngine};
use quarto_core::presenter::document::present_document;
use quarto_core::presenter::figures::FsFigureSink;
use quarto_core::presenter::inline::InlineTarget;
use quarto_core::record::{AuthorTexts, coalesce, fill_ids, read_record, write_record};

use super::knitr_corpus_baseline::Baseline;
use super::knitr_corpus_run::materialize;
use super::knitr_corpus_support::Fixture;

/// A fixture run through the engine.
pub struct RecordRun {
    /// The live markdown, the oracle: the engine's output with no record wanted, so no unit id
    /// is injected into the chunks (with a record wanted the ids leak into `fenced.echo` and
    /// shift numeric `eval`; see the KNR-3 plan, slice 4 notes).
    pub markdown: String,
    /// The directory the live run ran in.
    pub live_dir: tempfile::TempDir,
    /// The record's presentation, or why there is none.
    pub candidate: Baseline,
    /// The directory the fixture ran in (figure paths are relative to it).
    pub dir: tempfile::TempDir,
}

/// The live run: the engine as a render without a record runs it.
fn live_markdown(f: &Fixture, format: &str) -> Result<(String, tempfile::TempDir), String> {
    let (dir, doc_path) = materialize(f);
    let units = scan_units(&f.text);
    let ctx = ExecutionContext::new(
        dir.path().to_path_buf(),
        PathBuf::from(dir.path()),
        doc_path,
        format,
    )
    .with_owned_languages(vec!["r".to_string(), "bash".to_string()])
    .with_units(units)
    .with_quiet(std::env::var_os("KNR_VERBOSE").is_none());
    let result = quarto_core::engine::ExecutionEngine::execute(&KnitrEngine::new(), &f.text, &ctx)
        .map_err(|e| format!("{e}"))?;
    Ok((result.markdown, dir))
}

/// `KNR_DUMP_RECORDS=<dir>` keeps every written record as `<dir>/<fixture>@<format>.ipynb`, for
/// `uv run scripts/exr-record/nbformat_facts.py <dir>/*.ipynb` (EXR-4's nbformat validator,
/// which exits non-zero on any error or warning).
fn dump_record(f: &Fixture, format: &str, text: &str) {
    let Some(dir) = std::env::var_os("KNR_DUMP_RECORDS") else {
        return;
    };
    let name = format!("{}@{format}.ipynb", f.name.replace('/', "__"));
    let path = PathBuf::from(dir).join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create KNR_DUMP_RECORDS dir");
    }
    std::fs::write(path, text).expect("write dumped record");
}

/// Run `f` under `format`. `Err` carries the engine's failure (an aborted render).
pub fn run_through_record(f: &Fixture, format: &str) -> Result<RecordRun, String> {
    let (live, live_dir) = live_markdown(f, format)?;
    let (dir, doc_path) = materialize(f);
    let mut units = scan_units(&f.text);
    let texts = AuthorTexts::from_scanned(&units);
    fill_ids(&mut units, &texts).map_err(|e| format!("{e}"))?;
    let ctx = ExecutionContext::new(
        dir.path().to_path_buf(),
        PathBuf::from(dir.path()),
        doc_path.clone(),
        format,
    )
    .with_owned_languages(vec!["r".to_string(), "bash".to_string()])
    .with_units(units.clone())
    .with_author_texts(texts)
    .with_want_record(true)
    .with_quiet(std::env::var_os("KNR_VERBOSE").is_none());
    let result = quarto_core::engine::ExecutionEngine::execute(&KnitrEngine::new(), &f.text, &ctx)
        .map_err(|e| format!("{e}"))?;
    let Some(record) = result.record else {
        let why = result
            .warnings
            .iter()
            .map(|w| w.to_text(None))
            .collect::<Vec<_>>()
            .join("; ");
        return Ok(RecordRun {
            markdown: live,
            live_dir,
            candidate: Baseline::Fallback(format!("no record: {why}")),
            dir,
        });
    };
    // Written and read back: what the presenter sees is what a real render leaves on disk.
    let file = coalesce(&[record], &units).map_err(|e| format!("coalesce: {e}"))?;
    let text = write_record(&file).map_err(|e| format!("write: {e}"))?;
    dump_record(f, format, &text);
    let record = read_record(&text).map_err(|e| format!("read back: {e}"))?;
    let engine = record
        .metadata
        .quarto_engines
        .iter()
        .find(|e| e.name == "knitr");
    let mut fig = FsFigureSink::new(&doc_path);
    let candidate = match present_document(
        &record.cells,
        &f.text,
        &units,
        None,
        InlineTarget::from_format(format),
        format,
        engine,
        &mut fig,
    ) {
        Ok(p) => Baseline::Candidate(p.markdown),
        Err(e) => Baseline::Fallback(format!("present: {e:?}")),
    };
    Ok(RecordRun {
        markdown: live,
        live_dir,
        candidate,
        dir,
    })
}
