//! Export a record as a Positron output-cache notebook (RIU-5; the reverse of
//! [`super::positron_import`]).
//!
//! Positron deletes a cache file its loader rejects, and rewrites a cache file keeping only
//! `quarto_cell_id`, `quarto_content_hash` and `quarto_label` per cell. It ignores
//! `quarto_source` on load. So the export is a filter plus a few output transforms; see
//! `claude-notes/plans/2026-11-02-riu-05-export-for-positron.md`.
//!
//! The function returns bytes. It writes no file and reads no clock or environment: the caller
//! supplies `quarto_source` (not interpreted) and `last_updated`.

use std::collections::BTreeSet;

use serde_json::{Map, Value, json};

use super::place::QUARTO_MARKDOWN_MIME;
use super::types::{Record, RecordCell, UnitKind};

/// Why a unit was left out of the export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    InlineUnit,
    EngineNotExportable,
    ExtensionLanguage,
    RenderedMarkdownOutput,
    DuplicateGroupWithSkippedMember,
}

impl SkipReason {
    pub fn as_str(self) -> &'static str {
        match self {
            SkipReason::InlineUnit => "inline unit",
            SkipReason::EngineNotExportable => "engine is not jupyter or julia",
            SkipReason::ExtensionLanguage => "extension language",
            SkipReason::RenderedMarkdownOutput => "rendered quarto-markdown output",
            SkipReason::DuplicateGroupWithSkippedMember => {
                "duplicate-hash group with a skipped member"
            }
        }
    }
}

/// Why one representation of an output was dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropReason {
    PdfHasNoRenderer,
    SvgWhenPngPresent,
    HtmlRepeatsImage,
}

impl DropReason {
    pub fn as_str(self) -> &'static str {
        match self {
            DropReason::PdfHasNoRenderer => "application/pdf has no renderer in Positron",
            DropReason::SvgWhenPngPresent => "image/svg+xml dropped: image/png is present",
            DropReason::HtmlRepeatsImage => "text/html is only a data-URI image that repeats",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedUnit {
    pub id: String,
    pub reason: SkipReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DroppedRepresentation {
    pub cell_id: String,
    pub mime: String,
    pub reason: DropReason,
}

#[derive(Debug, Clone, Default)]
pub struct ExportOptions {
    /// Written to `quarto_source` as given.
    pub source: String,
    /// Milliseconds since the epoch, supplied by the caller.
    pub last_updated: u64,
}

#[derive(Debug, Clone, Default)]
pub struct ExportedCache {
    pub bytes: Vec<u8>,
    /// How many cells were exported.
    pub cells: usize,
    pub skipped: Vec<SkippedUnit>,
    pub dropped: Vec<DroppedRepresentation>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExportError {
    #[error("cell `{cell_id}` has an empty `quarto_content_hash`, so Positron could not match it")]
    MissingContentHash { cell_id: String },
    #[error("cell `{cell_id}` has an output that is not an object")]
    MalformedOutput { cell_id: String },
}

/// The engines whose cells Positron's own kernel could have produced: the same two owners the
/// import gate admits for plain kernel languages (RIU-3), so the two directions agree.
pub const EXPORT_OWNERS: [&str; 2] = ["jupyter", "julia"];

const PDF: &str = "application/pdf";
const PNG: &str = "image/png";
const SVG: &str = "image/svg+xml";
const HTML: &str = "text/html";

fn skip_reason(record: &Record, cell: &RecordCell) -> Option<SkipReason> {
    let meta = &cell.metadata;
    if meta.quarto_kind == UnitKind::Inline {
        Some(SkipReason::InlineUnit)
    } else if !record
        .engine_of(cell)
        .is_some_and(|e| EXPORT_OWNERS.contains(&e))
    {
        Some(SkipReason::EngineNotExportable)
    } else if meta.quarto_lang.contains('.') {
        Some(SkipReason::ExtensionLanguage)
    } else if cell.outputs.iter().any(|o| {
        o.get("data")
            .is_some_and(|d| d.get(QUARTO_MARKDOWN_MIME).is_some())
    }) {
        Some(SkipReason::RenderedMarkdownOutput)
    } else {
        None
    }
}

/// Positron reads every data value as a string (`JSON.stringify` on anything else would quote
/// an HTML line array), so: a string stays, an array of strings is joined with no separator,
/// and any other value becomes compact JSON.
fn data_string(v: &Value) -> Value {
    match v {
        Value::String(_) => v.clone(),
        Value::Array(items) if items.iter().all(Value::is_string) => {
            Value::String(items.iter().filter_map(Value::as_str).collect())
        }
        other => Value::String(other.to_string()),
    }
}

/// True for a `text/html` whose whole body is one `<img>` with a `data:image/` source.
/// Base64 and percent-encoded data URIs hold no `<` or `>`, so a single pair of them means
/// there is no other markup.
fn is_data_uri_image_only(html: &str) -> bool {
    let h = html.trim();
    h.starts_with("<img")
        && h.ends_with('>')
        && h.matches('<').count() == 1
        && h.matches('>').count() == 1
        && (h.contains("src=\"data:image/") || h.contains("src='data:image/"))
}

fn export_output(
    output: &Value,
    cell_id: &str,
    dropped: &mut Vec<DroppedRepresentation>,
) -> Result<Value, ExportError> {
    let Some(obj) = output.as_object() else {
        return Err(ExportError::MalformedOutput {
            cell_id: cell_id.to_string(),
        });
    };
    let mut out = obj.clone();
    if let Some(data) = out.get_mut("data").and_then(Value::as_object_mut) {
        for v in data.values_mut() {
            *v = data_string(v);
        }
        let has_png = data.contains_key(PNG);
        let mut drop = |data: &mut Map<String, Value>, mime: &str, reason: DropReason| {
            data.remove(mime);
            dropped.push(DroppedRepresentation {
                cell_id: cell_id.to_string(),
                mime: mime.to_string(),
                reason,
            });
        };
        if data.contains_key(PDF) {
            drop(data, PDF, DropReason::PdfHasNoRenderer);
        }
        if has_png && data.contains_key(SVG) {
            drop(data, SVG, DropReason::SvgWhenPngPresent);
        }
        if data
            .get(HTML)
            .and_then(Value::as_str)
            .is_some_and(is_data_uri_image_only)
        {
            drop(data, HTML, DropReason::HtmlRepeatsImage);
        }
    }
    if let Some(meta) = out.get_mut("metadata").and_then(Value::as_object_mut) {
        meta.remove("quarto_format");
    }
    Ok(Value::Object(out))
}

/// Export `record` as a Positron cache file.
///
/// Keeps a unit only if it is a code cell (not an inline unit) owned by [`EXPORT_OWNERS`], in a
/// plain language, with no rendered-markdown output. A group of cells sharing a content hash is
/// kept whole or skipped whole, because Positron pairs by occurrence.
pub fn export_for_positron(
    record: &Record,
    options: &ExportOptions,
) -> Result<ExportedCache, ExportError> {
    let reasons: Vec<Option<SkipReason>> = record
        .cells
        .iter()
        .map(|c| skip_reason(record, c))
        .collect();
    let lost: BTreeSet<&str> = record
        .cells
        .iter()
        .zip(&reasons)
        .filter(|(_, r)| r.is_some())
        .map(|(c, _)| c.metadata.quarto_content_hash.as_str())
        .collect();

    let mut cache = ExportedCache::default();
    let mut cells = Vec::new();
    for (c, reason) in record.cells.iter().zip(&reasons) {
        let hash = c.metadata.quarto_content_hash.as_str();
        let id = c.metadata.quarto_cell_id.as_str();
        if reason.is_none() && hash.is_empty() {
            return Err(ExportError::MissingContentHash {
                cell_id: id.to_string(),
            });
        }
        let reason = reason.or_else(|| {
            lost.contains(hash)
                .then_some(SkipReason::DuplicateGroupWithSkippedMember)
        });
        if let Some(reason) = reason {
            cache.skipped.push(SkippedUnit {
                id: id.to_string(),
                reason,
            });
            continue;
        }
        let mut meta = Map::new();
        meta.insert("quarto_cell_id".into(), json!(id));
        meta.insert("quarto_content_hash".into(), json!(hash));
        if let Some(l) = &c.metadata.quarto_label {
            meta.insert("quarto_label".into(), json!(l));
        }
        let outputs = c
            .outputs
            .iter()
            .map(|o| export_output(o, id, &mut cache.dropped))
            .collect::<Result<Vec<_>, _>>()?;
        cells.push(json!({
            "cell_type": "code",
            "execution_count": null,
            "metadata": Value::Object(meta),
            "source": c.source,
            "outputs": outputs,
        }));
    }
    cache.cells = cells.len();
    let nb = json!({
        "nbformat": 4,
        "nbformat_minor": 5,
        "metadata": {
            "quarto_cache_version": 1,
            "quarto_source": options.source,
            "last_updated": options.last_updated,
        },
        "cells": cells,
    });
    cache.bytes = serde_json::to_vec_pretty(&nb).expect("a JSON value serializes");
    Ok(cache)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::record::read_record_value;

    fn cell(id: &str, hash: &str, lang: &str, engine: &str, kind: &str, outputs: Value) -> Value {
        json!({
            "cell_type": "code",
            "execution_count": 3,
            "id": id,
            "metadata": {
                "quarto_cell_id": id,
                "quarto_content_hash": hash,
                "quarto_engine": engine,
                "quarto_exec_hash": "e",
                "quarto_kind": kind,
                "quarto_lang": lang,
                "echo": false,
            },
            "outputs": outputs,
            "source": ["x"],
        })
    }

    fn py(id: &str, hash: &str, outputs: Value) -> Value {
        cell(id, hash, "python", "jupyter", "cell", outputs)
    }

    fn record(cells: Vec<Value>) -> Record {
        read_record_value(json!({
            "nbformat": 4,
            "nbformat_minor": 5,
            "metadata": {"quarto_record_version": 1},
            "cells": cells,
        }))
        .unwrap()
    }

    fn opts() -> ExportOptions {
        ExportOptions {
            source: "doc.qmd".into(),
            last_updated: 1234,
        }
    }

    fn export(cells: Vec<Value>) -> (Value, ExportedCache) {
        let cache = export_for_positron(&record(cells), &opts()).unwrap();
        let v: Value = serde_json::from_slice(&cache.bytes).expect("export is JSON");
        (v, cache)
    }

    fn result(data: Value) -> Value {
        json!([{"output_type": "execute_result", "execution_count": 1, "metadata": {}, "data": data}])
    }

    // ---- transform steps ----

    #[test]
    fn notebook_metadata_carries_version_source_and_the_supplied_timestamp() {
        let (v, _) = export(vec![py("a", "h1", json!([]))]);
        assert_eq!(v["nbformat"], 4);
        assert_eq!(v["metadata"]["quarto_cache_version"], 1);
        assert_eq!(v["metadata"]["quarto_source"], "doc.qmd");
        assert_eq!(v["metadata"]["last_updated"], 1234);
    }

    #[test]
    fn cell_metadata_is_reduced_to_positrons_three_keys() {
        let mut c = py("a", "h1", json!([]));
        c["metadata"]["quarto_label"] = json!("fig-x");
        let (v, _) = export(vec![c]);
        let meta = v["cells"][0]["metadata"].as_object().unwrap();
        let mut keys: Vec<_> = meta.keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            ["quarto_cell_id", "quarto_content_hash", "quarto_label"]
        );
        assert_eq!(v["cells"][0]["execution_count"], Value::Null);
        assert_eq!(v["cells"][0]["cell_type"], "code");
    }

    #[test]
    fn an_unlabeled_cell_has_no_label_key_and_outputs_is_always_an_array() {
        let (v, _) = export(vec![py("a", "h1", json!([]))]);
        assert!(v["cells"][0]["metadata"].get("quarto_label").is_none());
        assert_eq!(v["cells"][0]["outputs"], json!([]));
    }

    #[test]
    fn a_string_stays_an_array_of_lines_is_joined_and_other_values_become_compact_json() {
        let outputs = result(json!({
            "text/plain": "plain",
            "text/html": ["<b>", "x</b>"],
            "application/json": {"a": [1, 2]},
        }));
        let (v, _) = export(vec![py("a", "h1", outputs)]);
        let data = &v["cells"][0]["outputs"][0]["data"];
        assert_eq!(data["text/plain"], "plain");
        assert_eq!(data["text/html"], "<b>x</b>");
        assert_eq!(data["application/json"], "{\"a\":[1,2]}");
    }

    #[test]
    fn quarto_format_output_metadata_is_dropped_and_the_rest_kept() {
        let outputs = json!([{
            "output_type": "display_data",
            "metadata": {"quarto_format": "x", "image/png": {"width": 3}},
            "data": {"image/png": "AAAA"},
        }]);
        let (v, _) = export(vec![py("a", "h1", outputs)]);
        let meta = &v["cells"][0]["outputs"][0]["metadata"];
        assert!(meta.get("quarto_format").is_none());
        assert_eq!(meta["image/png"]["width"], 3);
    }

    #[test]
    fn streams_and_errors_pass_through_unchanged() {
        let outputs = json!([
            {"output_type": "stream", "name": "stdout", "text": ["hi\n"]},
            {"output_type": "error", "ename": "E", "evalue": "v", "traceback": ["tb"]},
        ]);
        let (v, _) = export(vec![py("a", "h1", outputs.clone())]);
        assert_eq!(v["cells"][0]["outputs"], outputs);
    }

    // ---- representation rules ----

    const DATA_IMG: &str = "<img src=\"data:image/png;base64,AAAA\" />";

    fn mimes(v: &Value) -> Vec<String> {
        let mut m: Vec<String> = v["cells"][0]["outputs"][0]["data"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        m.sort();
        m
    }

    #[test]
    fn the_pdf_is_dropped_and_the_drop_is_reported() {
        let (v, cache) = export(vec![py(
            "a",
            "h1",
            result(json!({"application/pdf": "JVBERg==", "image/png": "AAAA"})),
        )]);
        assert_eq!(mimes(&v), ["image/png"]);
        assert_eq!(
            cache.dropped,
            [DroppedRepresentation {
                cell_id: "a".into(),
                mime: "application/pdf".into(),
                reason: DropReason::PdfHasNoRenderer
            }]
        );
    }

    #[test]
    fn png_wins_over_svg() {
        let (v, cache) = export(vec![py(
            "a",
            "h1",
            result(json!({"image/png": "AAAA", "image/svg+xml": "<svg/>"})),
        )]);
        assert_eq!(mimes(&v), ["image/png"]);
        assert_eq!(cache.dropped[0].reason, DropReason::SvgWhenPngPresent);
    }

    #[test]
    fn svg_alone_stays() {
        let (v, cache) = export(vec![py(
            "a",
            "h1",
            result(json!({"image/svg+xml": "<svg/>"})),
        )]);
        assert_eq!(mimes(&v), ["image/svg+xml"]);
        assert!(cache.dropped.is_empty());
    }

    #[test]
    fn an_img_only_data_uri_html_is_dropped() {
        let (v, cache) = export(vec![py(
            "a",
            "h1",
            result(json!({"image/png": "AAAA", "text/html": DATA_IMG})),
        )]);
        assert_eq!(mimes(&v), ["image/png"]);
        assert_eq!(cache.dropped[0].reason, DropReason::HtmlRepeatsImage);
        assert_eq!(cache.dropped[0].mime, "text/html");
    }

    #[test]
    fn an_html_table_stays() {
        let (v, cache) = export(vec![py(
            "a",
            "h1",
            result(json!({"text/html": "<table><tr><td>1</td></tr></table>", "text/plain": "1"})),
        )]);
        assert_eq!(mimes(&v), ["text/html", "text/plain"]);
        assert!(cache.dropped.is_empty());
    }

    #[test]
    fn a_plotly_shaped_bundle_keeps_its_script_html_and_its_png() {
        let html =
            "<div><script>Plotly.newPlot()</script><img src=\"data:image/png;base64,AAAA\"></div>";
        let (v, cache) = export(vec![py(
            "a",
            "h1",
            result(json!({"text/html": html, "image/png": "AAAA"})),
        )]);
        assert_eq!(mimes(&v), ["image/png", "text/html"]);
        assert!(cache.dropped.is_empty());
    }

    #[test]
    fn a_remote_img_html_stays() {
        let (v, _) = export(vec![py(
            "a",
            "h1",
            result(json!({"text/html": "<img src=\"https://x/y.png\">"})),
        )]);
        assert_eq!(mimes(&v), ["text/html"]);
    }

    #[test]
    fn the_julia_plot_bundle_exports_one_image() {
        let (v, cache) = export(vec![cell(
            "a",
            "h1",
            "julia",
            "julia",
            "cell",
            result(json!({
                "application/pdf": "JVBERg==",
                "image/png": "AAAA",
                "image/svg+xml": "<svg/>",
                "text/html": DATA_IMG,
            })),
        )]);
        assert_eq!(mimes(&v), ["image/png"]);
        assert_eq!(cache.dropped.len(), 3);
    }

    // ---- skip classes ----

    fn skipped(cache: &ExportedCache) -> Vec<(&str, SkipReason)> {
        cache
            .skipped
            .iter()
            .map(|s| (s.id.as_str(), s.reason))
            .collect()
    }

    #[test]
    fn an_inline_unit_is_skipped_and_the_others_export() {
        let (v, cache) = export(vec![
            cell("i", "h0", "python", "jupyter", "inline", json!([])),
            py("a", "h1", json!([])),
        ]);
        assert_eq!(cache.cells, 1);
        assert_eq!(v["cells"].as_array().unwrap().len(), 1);
        assert_eq!(skipped(&cache), [("i", SkipReason::InlineUnit)]);
    }

    #[test]
    fn another_engines_cell_is_skipped() {
        let (_, cache) = export(vec![
            cell("k", "h0", "r", "knitr", "cell", json!([])),
            py("a", "h1", json!([])),
        ]);
        assert_eq!(cache.cells, 1);
        assert_eq!(skipped(&cache), [("k", SkipReason::EngineNotExportable)]);
    }

    #[test]
    fn an_extension_language_is_skipped() {
        let (_, cache) = export(vec![
            cell("m", "h0", "python.marimo", "jupyter", "cell", json!([])),
            py("a", "h1", json!([])),
        ]);
        assert_eq!(cache.cells, 1);
        assert_eq!(skipped(&cache), [("m", SkipReason::ExtensionLanguage)]);
    }

    #[test]
    fn a_cell_with_rendered_markdown_output_is_skipped() {
        let md = result(json!({"text/x-quarto-markdown": "# hi"}));
        let (_, cache) = export(vec![py("f", "h0", md), py("a", "h1", json!([]))]);
        assert_eq!(cache.cells, 1);
        assert_eq!(skipped(&cache), [("f", SkipReason::RenderedMarkdownOutput)]);
    }

    #[test]
    fn a_partial_duplicate_group_is_skipped_whole() {
        let md = result(json!({"text/x-quarto-markdown": "# hi"}));
        let (_, cache) = export(vec![
            py("d", "same", json!([])),
            py("d-2", "same", md),
            py("a", "h1", json!([])),
        ]);
        assert_eq!(cache.cells, 1);
        assert_eq!(
            skipped(&cache),
            [
                ("d", SkipReason::DuplicateGroupWithSkippedMember),
                ("d-2", SkipReason::RenderedMarkdownOutput),
            ]
        );
    }

    #[test]
    fn a_complete_duplicate_group_exports_in_order() {
        let (v, cache) = export(vec![
            py("d", "same", result(json!({"text/plain": "1"}))),
            py("d-2", "same", result(json!({"text/plain": "2"}))),
        ]);
        assert_eq!(cache.cells, 2);
        assert_eq!(v["cells"][0]["outputs"][0]["data"]["text/plain"], "1");
        assert_eq!(v["cells"][1]["outputs"][0]["data"]["text/plain"], "2");
    }

    #[test]
    fn julia_cells_are_owned_by_julia_and_export() {
        let (_, cache) = export(vec![
            cell("j", "h0", "julia", "julia", "cell", json!([])),
            cell("ji", "h1", "julia", "julia", "inline", json!([])),
        ]);
        assert_eq!(cache.cells, 1);
        assert_eq!(skipped(&cache), [("ji", SkipReason::InlineUnit)]);
    }

    #[test]
    fn an_untagged_cell_takes_its_engine_from_ownership() {
        let mut c = py("a", "h1", json!([]));
        c["metadata"]
            .as_object_mut()
            .unwrap()
            .remove("quarto_engine");
        let rec = read_record_value(json!({
            "nbformat": 4, "nbformat_minor": 5,
            "metadata": {"quarto_record_version": 1, "quarto_engines": [{
                "name": "jupyter", "can_freeze": true,
                "hash_definition": {"documentKeys": [], "granularity": "unit", "id": "jupyter",
                    "presentationCellOptions": [], "presentationExecuteKeys": [], "version": 1},
                "document_hash": "d", "owned_languages": ["python"]}]},
            "cells": [c],
        }))
        .unwrap();
        assert_eq!(rec.cells[0].metadata.quarto_engine, None);
        assert_eq!(export_for_positron(&rec, &opts()).unwrap().cells, 1);
    }

    // ---- errors ----

    #[test]
    fn an_empty_content_hash_is_an_error_naming_the_cell() {
        let err = export_for_positron(&record(vec![py("a", "", json!([]))]), &opts()).unwrap_err();
        assert_eq!(
            err,
            ExportError::MissingContentHash {
                cell_id: "a".into()
            }
        );
    }

    #[test]
    fn a_non_object_output_is_an_error_naming_the_cell() {
        let err =
            export_for_positron(&record(vec![py("a", "h", json!(["oops"]))]), &opts()).unwrap_err();
        assert_eq!(
            err,
            ExportError::MalformedOutput {
                cell_id: "a".into()
            }
        );
    }

    #[test]
    fn no_exportable_cells_is_an_empty_cache_not_an_error() {
        let (v, cache) = export(vec![cell(
            "i",
            "h",
            "python",
            "jupyter",
            "inline",
            json!([]),
        )]);
        assert_eq!(cache.cells, 0);
        assert_eq!(v["cells"], json!([]));
        assert_eq!(v["metadata"]["quarto_cache_version"], 1);
    }
}
