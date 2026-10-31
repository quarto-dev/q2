//! The freeze-readiness checklist (EXR-4 Phase 6, epic decision 1), one test per item.
//!
//! Freeze is a later epic; these tests pin the properties it will rely on, so the record
//! format cannot drift into something that cannot be frozen.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::engine::units::{Unit, scan_units};
use crate::record::{
    AuthorTexts, EngineEntryInput, HashDefinition, Record, RecordBuilder, UnitResult, coalesce,
    fill_ids, hash_record, read_record, write_record,
};

const DOC: &str = "\
---
title: Report
---

Some prose here.

```{python}
#| label: fig-a
#| fig-width: 7
x = 1
```

More prose with `{python} 2 + 2` inline.
";

/// What a build varies without it being an execution input.
#[derive(Clone)]
struct Variant {
    provenance: BTreeMap<String, Value>,
    kernel: Option<String>,
    overlay: Option<BTreeMap<String, Value>>,
}

impl Default for Variant {
    fn default() -> Self {
        Self {
            provenance: BTreeMap::from([("platform".into(), json!("linux"))]),
            kernel: Some("python3".into()),
            overlay: None,
        }
    }
}

fn build(doc: &str, variant: &Variant) -> (Record, Vec<Unit>) {
    let mut units = scan_units(doc);
    let texts = AuthorTexts::from_scanned(&units);
    fill_ids(&mut units, &texts).unwrap();
    let def = HashDefinition::conservative("jupyter");
    let mut b = RecordBuilder::new("jupyter", &def, &units, &texts).unwrap();
    for i in 0..units.len() {
        b.push(
            i,
            UnitResult {
                outputs: vec![json!({
                    "output_type": "stream", "name": "stdout", "text": ["out\n"]
                })],
                execution_count: Some(1),
                kernel: variant.kernel.clone(),
            },
        )
        .unwrap();
    }
    let entry = b
        .finish(EngineEntryInput {
            can_freeze: true,
            owned_languages: vec!["python".into()],
            provenance: variant.provenance.clone(),
            ..Default::default()
        })
        .unwrap();
    let mut record = coalesce(&[entry], &units).unwrap();
    record.metadata.quarto_overlay = variant.overlay.clone();
    (record, units)
}

/// Every key matching, staleness or reuse reads.
fn keys(
    record: &Record,
) -> (
    Option<String>,
    Vec<String>,
    Vec<(String, String, String, String)>,
) {
    (
        record.metadata.quarto_execution_hash.clone(),
        record
            .metadata
            .quarto_engines
            .iter()
            .map(|e| e.document_hash.clone())
            .collect(),
        record
            .cells
            .iter()
            .map(|c| {
                (
                    c.id.clone(),
                    c.metadata.quarto_cell_id.clone(),
                    c.metadata.quarto_content_hash.clone(),
                    c.metadata.quarto_exec_hash.clone(),
                )
            })
            .collect(),
    )
}

#[test]
fn a_prose_only_edit_keeps_the_execution_hash_and_a_code_edit_changes_it() {
    let base = build(DOC, &Variant::default()).0;
    let prose = DOC.replace("Some prose here.", "Entirely different prose, much longer.");
    assert_ne!(prose, DOC);
    let edited_prose = build(&prose, &Variant::default()).0;
    assert_eq!(
        keys(&base),
        keys(&edited_prose),
        "prose is in no execution key"
    );

    let code = DOC.replace("x = 1", "x = 2");
    let edited_code = build(&code, &Variant::default()).0;
    assert_ne!(
        base.metadata.quarto_execution_hash,
        edited_code.metadata.quarto_execution_hash
    );

    // An inline expression is code too.
    let inline = DOC.replace("2 + 2", "2 + 3");
    assert_ne!(
        base.metadata.quarto_execution_hash,
        build(&inline, &Variant::default())
            .0
            .metadata
            .quarto_execution_hash
    );
}

#[test]
fn mutating_provenance_changes_no_matching_staleness_or_reuse_key() {
    let base = build(DOC, &Variant::default()).0;
    let mutated = build(
        DOC,
        &Variant {
            provenance: BTreeMap::from([
                ("platform".into(), json!("windows")),
                ("engine_version".into(), json!("9.9")),
                (
                    "kernelspec".into(),
                    json!({"name": "other", "language": "python"}),
                ),
            ]),
            kernel: Some("some-other-kernel".into()),
            overlay: Some(BTreeMap::from([("params".into(), json!({"n": 5}))])),
        },
    )
    .0;
    // The provenance really did change what is stored...
    assert_ne!(
        base.metadata.quarto_engines[0].provenance,
        mutated.metadata.quarto_engines[0].provenance
    );
    assert_ne!(
        base.cells[0].metadata.quarto_kernel,
        mutated.cells[0].metadata.quarto_kernel
    );
    assert!(mutated.metadata.quarto_overlay.is_some());
    // ...and no key moved.
    assert_eq!(keys(&base), keys(&mutated));
    // The record-level key is exactly the hash of the stored document hashes.
    let docs: Vec<&str> = mutated
        .metadata
        .quarto_engines
        .iter()
        .map(|e| e.document_hash.as_str())
        .collect();
    assert_eq!(
        mutated.metadata.quarto_execution_hash.as_deref(),
        Some(hash_record(&docs).as_str())
    );
}

#[test]
fn every_execute_result_fold_field_round_trips() {
    let (rec, units) = build(DOC, &Variant::default());
    let mut entry = rec.metadata.quarto_engines[0].clone();
    entry.includes = Some(json!({"in_header": ["<style/>"], "after_body": ["<p/>"]}));
    entry.filters = vec!["a.lua".into(), "b.lua".into()];
    entry.preserve = BTreeMap::from([("ph1".into(), "<b>x</b>".into())]);
    entry.needs_postprocess = true;
    entry.html_dependencies = vec![json!({"name": "d", "version": "1.2", "scripts": ["d.js"]})];
    entry.pandoc = Some(BTreeMap::from([("to".into(), json!("html"))]));
    entry.metadata = Some(BTreeMap::from([("title".into(), json!("Folded"))]));
    entry.resource_files = vec!["data.csv".into()];
    entry.warnings = vec![json!({"title": "careful"})];
    let mut with_fold = rec.clone();
    with_fold.metadata.quarto_engines = vec![entry.clone()];

    let bytes = write_record(&with_fold).unwrap();
    let back = read_record(&bytes).unwrap();
    assert_eq!(back.metadata.quarto_engines, vec![entry]);
    assert_eq!(back, with_fold);
    assert_eq!(
        write_record(&back).unwrap(),
        bytes,
        "and the bytes are stable"
    );
    let _ = units;
}

#[test]
fn rendered_units_carry_quarto_format_and_raw_units_carry_none() {
    use crate::record::QUARTO_MARKDOWN_MIME;

    // The golden has a raw (Jupyter) engine and a rendered (knitr) one.
    let golden = read_record(include_str!("../../tests/fixtures/records/golden.ipynb")).unwrap();
    let (mut rendered, mut raw) = (0, 0);
    for cell in &golden.cells {
        for output in &cell.outputs {
            let is_rendered = output["data"].get(QUARTO_MARKDOWN_MIME).is_some();
            let has_format = output["metadata"].get("quarto_format").is_some();
            assert_eq!(
                is_rendered, has_format,
                "`quarto_format` goes with rendered markdown and only with it: {output}"
            );
            if is_rendered {
                rendered += 1;
                assert_eq!(cell.outputs.len(), 1, "a rendered unit holds one output");
                assert_eq!(output["output_type"], "display_data");
            } else {
                raw += 1;
            }
        }
    }
    assert!(rendered > 0 && raw > 0, "the golden must exercise both");
}

#[test]
fn bytes_are_deterministic() {
    // The Phase 4 golden (`writer::tests::the_record_serializes_to_the_checked_in_golden_bytes`)
    // pins the exact bytes; here, two independent builds of the same inputs agree.
    let a = write_record(&build(DOC, &Variant::default()).0).unwrap();
    let b = write_record(&build(DOC, &Variant::default()).0).unwrap();
    assert_eq!(a, b);
    assert!(a.ends_with("}\n") && !a.contains('\r') && !a.contains("last_updated"));
}

#[test]
fn figures_are_inline_and_the_sidecar_field_is_reserved() {
    let (mut rec, _) = build(DOC, &Variant::default());
    rec.cells[0].outputs = vec![json!({
        "output_type": "display_data",
        "data": {"image/png": "iVBORw0KGgo=", "text/plain": "<Figure>"},
        "metadata": {}
    })];
    let bytes = write_record(&rec).unwrap();
    // Image bytes ride inside the record, as base64 strings.
    assert!(bytes.contains("\"image/png\": \"iVBORw0KGgo=\""));
    assert!(!bytes.contains("quarto_files_ref"));
    // A record from a future sidecar-writing q2 still reads; the writer drops the field.
    let mut v: Value = serde_json::from_str(&bytes).unwrap();
    v["metadata"]["quarto_files_ref"] = json!({"path": "sidecar"});
    let read = crate::record::read_record_value(v).unwrap();
    assert!(read.metadata.quarto_files_ref.is_some());
    assert!(!write_record(&read).unwrap().contains("quarto_files_ref"));
}
