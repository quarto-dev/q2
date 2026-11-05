//! KNR-6 Phase 6A: a knitr record replayed through `ReplayEngine` presents what the live
//! engine produced, under epic decision 26 (Identical and SyntaxLevel pass). The corpus gate
//! proves this for `present_document`; this is the same claim through the replay path
//! (`EngineCapture` in, `ReplayEngine::execute` out), which is what `--replay` runs.

#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;

use quarto_core::engine::units::scan_units;
use quarto_core::engine::{ExecutionContext, ExecutionEngine, KnitrEngine, ReplayEngine};
use quarto_core::record::{AuthorTexts, coalesce, fill_ids};

use super::knitr_corpus_classify::{Class, classify};
use super::knitr_corpus_normalizer::normalize;
use super::knitr_corpus_run::r_available;

const CHUNKS: &str = "Before.\n\n```{r a}\nx <- 1:3\nx\n```\n\nMiddle.\n\n\
```{r b}\n#| echo: false\n1 + 1\n```\n\nAfter.\n";

/// The same, with a classic `r` span, which R substitutes and which is not a unit.
const WITH_SPAN: &str = "Before.\n\n```{r a}\nx <- 1:3\nx\n```\n\nAfter `r 1 + 1` inline.\n";

fn context(doc: &str, dir: &std::path::Path, want_record: bool) -> ExecutionContext {
    let mut units = scan_units(doc);
    let texts = AuthorTexts::from_scanned(&units);
    let mut ctx = ExecutionContext::new(
        dir.to_path_buf(),
        PathBuf::from(dir),
        dir.join("doc.qmd"),
        "html",
    )
    .with_owned_languages(vec!["r".to_string(), "bash".to_string()])
    .with_quiet(true);
    if want_record {
        fill_ids(&mut units, &texts).unwrap();
        ctx = ctx
            .with_units(units)
            .with_author_texts(texts)
            .with_want_record(true);
    } else {
        ctx = ctx.with_units(units);
    }
    ctx
}

/// Run `doc` with a record wanted and wrap the run as the trace writer wraps it.
fn record_capture(
    doc: &str,
) -> (
    quarto_trace::EngineCapture,
    tempfile::TempDir,
    quarto_core::engine::ExecuteResult,
) {
    let rec_dir = tempfile::tempdir().unwrap();
    std::fs::write(rec_dir.path().join("doc.qmd"), doc).unwrap();
    let ctx = context(doc, rec_dir.path(), true);
    let recorded = KnitrEngine::new().execute(doc, &ctx).expect("recorded run");
    let engine_record = recorded.record.clone().expect("a record");
    let record = coalesce(std::slice::from_ref(&engine_record), &ctx.units).unwrap();
    let capture = quarto_trace::EngineCapture {
        engine_name: "knitr".into(),
        input_qmd: doc.into(),
        result: serde_json::to_value(&recorded).unwrap(),
        files: Vec::new(),
        record: Some(serde_json::to_value(&record).unwrap()),
    };
    (capture, rec_dir, recorded)
}

/// Run `doc` live, record it, replay the record, and compare under decision 26.
fn assert_replay_equals_live(doc: &str) {
    if !r_available() {
        return;
    }
    // The oracle: a live run that wants no record (ids would leak into the chunks).
    let live_dir = tempfile::tempdir().unwrap();
    std::fs::write(live_dir.path().join("doc.qmd"), doc).unwrap();
    let live = KnitrEngine::new()
        .execute(doc, &context(doc, live_dir.path(), false))
        .expect("live run");

    let (capture, rec_dir, recorded) = record_capture(doc);
    let replayed = ReplayEngine::new(capture)
        .execute(doc, &context(doc, rec_dir.path(), false))
        .expect("replay");

    assert_ne!(
        replayed.markdown, recorded.markdown,
        "replay must present the record, not return the stored markdown"
    );
    let class = classify(
        &normalize(&live.markdown, live_dir.path()),
        &normalize(&replayed.markdown, rec_dir.path()),
    );
    assert!(
        class <= Class::SyntaxLevel,
        "replay differs from live beyond syntax: {class:?}\nlive:\n{}\nreplayed:\n{}",
        live.markdown,
        replayed.markdown
    );
}

/// A structured chunk next to the two kinds of chunk that stay on the fallback table: one
/// with `child=` and one whose output a user hook rewrites.
const MIXED: &str = "Before.\n\n```{r setup}\n\
knitr::knit_hooks$set(output = function(x, options) paste0('<<', x, '>>\\n'))\n\
```\n\n\
```{r a}\nx <- 1:3\nx\n```\n\n\
```{r kid, child='kid.Rmd'}\n```\n\nAfter.\n";

/// The recorded run's markdown equals the unrecorded live run's, under decision 26. Before the
/// flip both are R's markdown; after it the recorded run is presented, and a mixed document
/// (fallback chunks among structured ones) must come out exactly as before.
fn assert_recorded_run_equals_live(doc: &str, extra_files: &[(&str, &str)], must_contain: &[&str]) {
    if !r_available() {
        return;
    }
    let live_dir = tempfile::tempdir().unwrap();
    let rec_dir = tempfile::tempdir().unwrap();
    for d in [live_dir.path(), rec_dir.path()] {
        std::fs::write(d.join("doc.qmd"), doc).unwrap();
        for (name, body) in extra_files {
            std::fs::write(d.join(name), body).unwrap();
        }
    }
    let live = KnitrEngine::new()
        .execute(doc, &context(doc, live_dir.path(), false))
        .expect("live run");
    let recorded = KnitrEngine::new()
        .execute(doc, &context(doc, rec_dir.path(), true))
        .expect("recorded run");
    for needle in must_contain {
        assert!(
            live.markdown.contains(needle),
            "the live run lacks {needle:?}, so the fixture does not exercise it:\n{}",
            live.markdown
        );
    }
    let class = classify(
        &normalize(&live.markdown, live_dir.path()),
        &normalize(&recorded.markdown, rec_dir.path()),
    );
    assert!(
        class <= Class::SyntaxLevel,
        "recorded run differs from live beyond syntax: {class:?}\nlive:\n{}\nrecorded:\n{}",
        live.markdown,
        recorded.markdown
    );
}

#[test]
fn a_mixed_document_with_fallback_chunks_presents_as_before() {
    assert_recorded_run_equals_live(
        MIXED,
        &[("kid.Rmd", "Child text: `r 2 + 2`.\n")],
        &["<<", "Child text: 4."],
    );
}

#[test]
fn a_replayed_knitr_record_presents_what_the_live_engine_produced() {
    assert_replay_equals_live(CHUNKS);
}

/// Red until the flip: the presenter splices over the input text, where a classic span is
/// still `` `r 1 + 1` ``. After the flip R returns the skeleton (spans substituted, chunks as
/// markers) and `capture.result.markdown` holds it, and replay presents over that. The flip
/// commit removes this `ignore`.
#[test]
#[ignore = "KNR-6 flip: replay needs the skeleton for classic spans"]
fn a_replayed_knitr_record_keeps_classic_span_values() {
    assert_replay_equals_live(WITH_SPAN);
}

const SETUP_ECHO_FALSE: &str = "```{r setup}\nknitr::opts_chunk$set(echo = FALSE)\n```\n\n\
```{r a}\nx <- 1:3\nx\n```\n";

/// `opts_chunk$set` in the setup chunk is R-side state the document does not show; the
/// record carries the resolved options, so the presenter hides the source as live does.
#[test]
fn a_setup_chunk_opts_chunk_presents_the_same_as_live() {
    if !r_available() {
        return;
    }
    assert_replay_equals_live(SETUP_ECHO_FALSE);
    let (capture, dir, _) = record_capture(SETUP_ECHO_FALSE);
    let shown = ReplayEngine::new(capture)
        .execute(
            SETUP_ECHO_FALSE,
            &context(SETUP_ECHO_FALSE, dir.path(), false),
        )
        .expect("replay")
        .markdown;
    assert!(
        !shown.contains("x <- 1:3"),
        "echo = FALSE must hide the source:\n{shown}"
    );
    assert!(
        shown.contains("[1] 1 2 3"),
        "the output must show:\n{shown}"
    );
}

/// A 1x1 transparent PNG; knitr reads the header for the figure's DPI.
const PNG_1X1: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x64, 0x60, 0xf8, 0x5f,
    0x0f, 0x00, 0x02, 0x87, 0x01, 0x80, 0xeb, 0x47, 0xba, 0x92, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

/// Fallback renders (KNR-6, settled design point 2): a chunk whose output Rust cannot present
/// still renders, as the markdown R shipped for it, and the render succeeds. Before the flip the
/// recorded run returns R's markdown outright, so these hold trivially; after it they pin that
/// the shipped markdown is what stands in for an unpresentable chunk.
#[test]
fn a_gif_renders_through_the_shipped_markdown() {
    assert_recorded_run_equals_live(
        "Before.\n\n```{r}\nknitr::include_graphics('anim.gif')\n```\n\nAfter.\n",
        &[("anim.gif", "GIF89a")],
        &["anim.gif"],
    );
}

#[test]
fn an_include_graphics_outside_the_document_directory_renders() {
    if !r_available() {
        return;
    }
    let outside = tempfile::tempdir().unwrap();
    let image = outside.path().join("elsewhere.png");
    std::fs::write(&image, PNG_1X1).unwrap();
    let doc = format!(
        "Before.\n\n```{{r}}\nknitr::include_graphics('{}')\n```\n\nAfter.\n",
        image.display().to_string().replace('\\', "/")
    );
    assert_recorded_run_equals_live(&doc, &[], &["elsewhere.png"]);
}

// The unreadable-image case is Rust-side only (R has read the file by then), so it is pinned at
// the pieces level in Phase 6B, not here.
