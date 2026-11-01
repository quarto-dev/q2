//! EXR-13b compatibility matrix for capture payloads across releases.
//!
//! Rows: legacy document through the new reader; record document through the new
//! reader; and a record document through a model of the *previous* release's reader
//! (release N-1, before `parse_capture_payload`), asserting the documented failure
//! mode so a later change cannot make it worse unnoticed. The previous reader gunzipped,
//! tried the `EngineCapture` array, then a single `EngineCapture`, and returned
//! `Err("JSON parse failed: ...")` when neither fit (the old `parse_capture_gz`).

use quarto_core::engine::capture_files::{gzip_captures, gzip_record};
use quarto_core::pandoc_request::captures::{CapturePayload, parse_capture_payload};
use quarto_core::record::read_record;
use quarto_trace::EngineCapture;
use std::io::Read as _;

type PreviousArrayShape = Vec<EngineCapture>;
type PreviousSingleShape = EngineCapture;

fn record_json() -> &'static str {
    include_str!("../fixtures/records/basic.json")
}

fn capture() -> EngineCapture {
    serde_json::from_value(serde_json::json!({
        "engine_name": "jupyter", "input_qmd": "x", "result": {}
    }))
    .unwrap()
}

#[test]
fn legacy_capture_doc_through_the_new_reader() {
    let gz = gzip_captures(&[capture()]).unwrap();
    match parse_capture_payload(Some(&gz), "doc.qmd").unwrap() {
        CapturePayload::Legacy(caps) => assert_eq!(caps.len(), 1),
        other => panic!("expected Legacy, got {other:?}"),
    }
}

#[test]
fn record_doc_through_the_new_reader() {
    let record = read_record(record_json()).unwrap();
    let gz = gzip_record(&record).unwrap();
    match parse_capture_payload(Some(&gz), "doc.qmd").unwrap() {
        CapturePayload::Record(r) => assert_eq!(*r, record),
        other => panic!("expected Record, got {other:?}"),
    }
}

/// The previous release's reader cannot parse a record: it fails on both shapes it
/// knew, so its caller sees `Err("JSON parse failed: ...")` and the preview falls back
/// to source. This is the one-way-door cost the release gating exists to avoid.
#[test]
fn record_doc_through_the_previous_release_reader_fails_both_shapes() {
    let gz = gzip_record(&read_record(record_json()).unwrap()).unwrap();
    let mut json = Vec::new();
    flate2::read::GzDecoder::new(gz.as_slice())
        .read_to_end(&mut json)
        .unwrap();

    let as_array = serde_json::from_slice::<PreviousArrayShape>(&json).unwrap_err();
    assert!(
        as_array.to_string().contains("expected a sequence"),
        "{as_array}"
    );
    let as_single = serde_json::from_slice::<PreviousSingleShape>(&json).unwrap_err();
    assert!(
        as_single.to_string().contains("missing field"),
        "{as_single}"
    );
}
