//! RIU-5: `export_for_positron` against real records (rendered once through `q2` with
//! `execute: {record: true}` and checked in under `tests/fixtures/records-export/`, so no kernel
//! runs here), the conformance mirror of Positron's loader, and the inverse check with the
//! import side.

use quarto_core::record::{
    ExportOptions, ExportedCache, Record, SkipReason, export_for_positron, import_positron_cache,
    read_record,
};
use serde_json::Value;

use super::positron_conformance::{LoaderVerdict, positron_loader};

fn fixture(name: &str) -> Record {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/records-export")
            .join(name),
    )
    .unwrap();
    read_record(&text).unwrap()
}

fn export(record: &Record) -> (Value, ExportedCache) {
    let cache = export_for_positron(
        record,
        &ExportOptions {
            source: "doc.qmd".into(),
            last_updated: 1,
        },
    )
    .unwrap();
    (serde_json::from_slice(&cache.bytes).unwrap(), cache)
}

/// Export, check the mirror accepts it, and import it back: every exported cell's outputs come
/// back, in kind and count, and streams and errors come back byte-for-byte.
fn round_trip(record: &Record, owner: &str) -> ExportedCache {
    let (file, cache) = export(record);
    assert_eq!(positron_loader(&file), LoaderVerdict::Keeps);
    let import = import_positron_cache(&cache.bytes).unwrap();
    let mut seen = std::collections::BTreeMap::<String, usize>::new();
    for cell in &record.cells {
        if cache
            .skipped
            .iter()
            .any(|s| s.id == cell.metadata.quarto_cell_id)
        {
            continue;
        }
        let hash = &cell.metadata.quarto_content_hash;
        let occurrence = seen.entry(hash.clone()).or_default();
        let got = import
            .outputs_for(hash, *occurrence, owner, &cell.metadata.quarto_lang, false)
            .unwrap_or_else(|| panic!("no outputs back for {}", cell.metadata.quarto_cell_id));
        *occurrence += 1;
        assert_eq!(
            got.outputs().len(),
            cell.outputs.len(),
            "{}",
            cell.metadata.quarto_cell_id
        );
        for (back, orig) in got.outputs().iter().zip(&cell.outputs) {
            if matches!(orig["output_type"].as_str(), Some("stream" | "error")) {
                assert_eq!(back, orig);
            }
        }
    }
    cache
}

#[test]
fn the_basic_python_record_exports_four_cells_and_skips_the_inline_span() {
    let record = fixture("basic.ipynb");
    let (file, cache) = export(&record);
    assert_eq!(cache.cells, 4);
    assert_eq!(cache.skipped.len(), 1);
    assert_eq!(cache.skipped[0].reason, SkipReason::InlineUnit);
    assert!(cache.dropped.is_empty());
    assert_eq!(positron_loader(&file), LoaderVerdict::Keeps);
    // The matplotlib PNG and the pandas table both survive.
    let all = file.to_string();
    assert!(all.contains("image/png") && all.contains("<table"));
    round_trip(&record, "jupyter");
}

#[test]
fn the_dup_record_exports_all_three_cells_in_order() {
    let record = fixture("dup.ipynb");
    let (file, cache) = export(&record);
    assert_eq!(cache.cells, 3);
    assert!(cache.skipped.is_empty());
    let hashes: Vec<&str> = file["cells"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["metadata"]["quarto_content_hash"].as_str().unwrap())
        .collect();
    assert_eq!(hashes[0], hashes[1]);
    assert_ne!(hashes[1], hashes[2]);
    assert_eq!(file["cells"][2]["metadata"]["quarto_label"], "lbl-one");
    round_trip(&record, "jupyter");
}

/// The mirror deletes a file whose code cell lacks `outputs`; the export never produces one.
/// The bad case is built by mutating the output, not the function.
#[test]
fn an_export_the_mirror_would_delete_cannot_be_produced() {
    let (mut file, _) = export(&fixture("basic.ipynb"));
    assert_eq!(positron_loader(&file), LoaderVerdict::Keeps);
    for cell in file["cells"].as_array_mut().unwrap() {
        assert!(
            cell["outputs"].is_array(),
            "every exported cell has outputs"
        );
    }
    file["cells"][0].as_object_mut().unwrap().remove("outputs");
    assert!(matches!(positron_loader(&file), LoaderVerdict::Deletes(_)));
}

/// Spike J: a Julia plot is one bundle of four representations, and Positron shows every item
/// of a bundle. The export sends one image: the PNG.
#[test]
fn the_julia_plot_record_exports_each_image_once() {
    let record = fixture("julia-plot.ipynb");
    let (file, cache) = export(&record);
    assert_eq!(cache.cells, 4);
    assert!(cache.skipped.is_empty());
    let plot = &file["cells"][2]["outputs"][0]["data"];
    let mut mimes: Vec<&str> = plot
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    mimes.sort_unstable();
    assert_eq!(mimes, ["image/png"]);
    let mut dropped: Vec<&str> = cache.dropped.iter().map(|d| d.mime.as_str()).collect();
    dropped.sort_unstable();
    assert_eq!(dropped, ["application/pdf", "image/svg+xml", "text/html"]);
    // The HTML cell is not an image: its HTML stays.
    assert!(
        file["cells"][3]["outputs"][0]["data"]
            .get("text/html")
            .is_some()
    );
    // The import gate for the `julia` owner is RIU-3's to widen; `jupyter` stands in here.
    round_trip(&record, "jupyter");
}

#[test]
fn a_julia_error_cell_exports_with_its_error_output() {
    let record = fixture("julia-error.ipynb");
    let (file, cache) = export(&record);
    assert_eq!(cache.cells, 2);
    assert_eq!(file["cells"][1]["outputs"][0]["output_type"], "error");
    round_trip(&record, "jupyter");
}
