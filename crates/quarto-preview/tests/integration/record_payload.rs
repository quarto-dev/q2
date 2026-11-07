//! EXR-13: `read_capture_from_doc` is a thin fetch of the doc's `content` plus
//! one call to `parse_capture_payload`, so it returns exactly what the parser
//! returns for the same bytes, whatever the payload kind.

use std::io::Write;
use std::sync::Arc;

use quarto_core::engine::units::scan_units;
use quarto_core::pandoc_request::captures::{CapturePayload, parse_capture_payload};
use quarto_core::record::{
    AuthorTexts, EngineEntryInput, HashDefinition, RecordBuilder, UnitResult, coalesce, fill_ids,
    write_record,
};
use quarto_hub::HubContext;
use quarto_hub::context::HubConfig;
use quarto_hub::resource::{
    CAPTURE_MIME_TYPE, CaptureDocMeta, RECORD_MIME_TYPE, create_binary_document,
    create_record_document,
};
use quarto_hub::storage::StorageManager;
use quarto_preview::capture_driver::read_capture_from_doc;

fn gz(text: &str) -> Vec<u8> {
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(text.as_bytes()).unwrap();
    enc.finish().unwrap()
}

fn record_json() -> String {
    let mut units = scan_units("# T\n\n```{python}\nprint(1)\n```\n");
    let texts = AuthorTexts::from_scanned(&units);
    fill_ids(&mut units, &texts).unwrap();
    let def = HashDefinition::conservative("jupyter");
    let mut b = RecordBuilder::new("jupyter", &def, &units, &texts).unwrap();
    b.push(
        0,
        UnitResult {
            outputs: vec![serde_json::json!({
                "output_type": "stream", "name": "stdout", "text": ["1\n"]
            })],
            execution_count: Some(1),
            kernel: Some("python3".into()),
        },
    )
    .unwrap();
    let entry = b
        .finish(EngineEntryInput {
            owned_languages: vec!["python".into()],
            ..Default::default()
        })
        .unwrap();
    write_record(&coalesce(&[entry], &units).unwrap()).unwrap()
}

fn describe(r: &Result<CapturePayload, String>) -> String {
    match r {
        Ok(CapturePayload::Empty) => "empty".into(),
        Ok(CapturePayload::Legacy(v)) => format!("legacy:{}", v.len()),
        Ok(CapturePayload::Record(r)) => format!("record:{}", r.cells.len()),
        Err(e) => format!("err:{e}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn read_capture_from_doc_returns_what_the_parser_returns() {
    let temp = tempfile::TempDir::new().unwrap();
    let storage = StorageManager::new(temp.path()).unwrap();
    let ctx = Arc::new(
        HubContext::new(storage, HubConfig::default())
            .await
            .unwrap(),
    );
    let one = r#"{"engine_name":"r","input_qmd":"","result":null,"files":[]}"#;
    let meta = CaptureDocMeta {
        source_path: "a.qmd".into(),
        engines: vec!["jupyter".into()],
    };

    let cases: Vec<(&str, automerge::Automerge)> = vec![
        (
            "legacy array",
            create_binary_document(&gz(&format!("[{one}]")), CAPTURE_MIME_TYPE).unwrap(),
        ),
        (
            "legacy single object",
            create_binary_document(&gz(one), CAPTURE_MIME_TYPE).unwrap(),
        ),
        (
            "record",
            create_record_document(&gz(&record_json()), &meta).unwrap(),
        ),
        (
            "record without meta",
            create_binary_document(&gz(&record_json()), RECORD_MIME_TYPE).unwrap(),
        ),
        (
            "corrupt gzip",
            create_binary_document(b"not gzip", RECORD_MIME_TYPE).unwrap(),
        ),
        (
            "neither shape",
            create_binary_document(&gz(r#"{"hello":1}"#), RECORD_MIME_TYPE).unwrap(),
        ),
        (
            "empty content",
            create_binary_document(&[], RECORD_MIME_TYPE).unwrap(),
        ),
    ];

    for (name, doc) in cases {
        let content = {
            use automerge::{ROOT, ReadDoc};
            doc.get(ROOT, "content")
                .unwrap()
                .unwrap()
                .0
                .into_bytes()
                .unwrap()
        };
        let handle = ctx.repo().create(doc).await.unwrap();
        let id = handle.document_id().to_string();
        let via_doc = read_capture_from_doc(&ctx, &id).await;
        let direct = parse_capture_payload(Some(&content), &id);
        assert_eq!(describe(&via_doc), describe(&direct), "case {name}");
    }

    // The kinds really are the kinds the table above claims.
    let rec = parse_capture_payload(Some(&gz(&record_json())), "a.qmd");
    assert_eq!(describe(&rec), "record:1");
}
