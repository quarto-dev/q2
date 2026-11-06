//! The deterministic record writer (EXR-4 Phase 4).
//!
//! The nbformat writer convention: one-space indent, every object's keys sorted, LF endings,
//! UTF-8 left unescaped, one trailing newline. There is no `last_updated` and nothing volatile,
//! so the same record always gives the same bytes and unchanged output means no file change.
//!
//! Keys are sorted explicitly. The workspace builds `serde_json` with `preserve_order`, so a
//! `Value` object keeps its insertion order and would otherwise leak the order an engine built
//! it in (outputs, `html_dependencies`, `pandoc`) into the bytes.

use serde::Serialize;
use serde_json::Value;

use super::types::Record;

/// Serialize `record` to its canonical bytes (always valid UTF-8, so a `String`).
///
/// Two things are normalized on the way out, so the file validates whatever an engine handed
/// over: every `execute_result` output carries `execution_count` (`null` when absent; the
/// nbformat validator rejects one without it), and the reserved `quarto_files_ref` sidecar
/// field is never emitted.
pub fn write_record(record: &Record) -> Result<String, serde_json::Error> {
    let mut record = record.clone();
    record.metadata.quarto_files_ref = None;
    let mut value = serde_json::to_value(&record)?;
    if let Some(cells) = value.get_mut("cells").and_then(Value::as_array_mut) {
        for cell in cells {
            if let Some(outputs) = cell.get_mut("outputs").and_then(Value::as_array_mut) {
                outputs.iter_mut().for_each(ensure_execution_count);
            }
        }
    }
    let mut out = to_canonical_string(&sort_keys(value))?;
    out.push('\n');
    Ok(out)
}

fn ensure_execution_count(output: &mut Value) {
    if let Value::Object(map) = output
        && map.get("output_type").and_then(Value::as_str) == Some("execute_result")
        && !map.contains_key("execution_count")
    {
        map.insert("execution_count".into(), Value::Null);
    }
}

/// Rebuild every object with its keys in sorted order.
fn sort_keys(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> = map.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(k, v)| (k, sort_keys(v)))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sort_keys).collect()),
        scalar => scalar,
    }
}

fn to_canonical_string(value: &Value) -> Result<String, serde_json::Error> {
    let mut buf = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b" ");
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
    value.serialize(&mut ser)?;
    Ok(String::from_utf8(buf).expect("serde_json writes UTF-8"))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;
    use crate::engine::units::scan_units;
    use crate::record::{
        AuthorTexts, EngineEntryInput, HashDefinition, RecordBuilder, RecordFile, UnitResult,
        coalesce, fill_ids,
    };

    const GOLDEN: &str = include_str!("../../tests/fixtures/records/golden.ipynb");
    const GOLDEN_PATH: &str = "tests/fixtures/records/golden.ipynb";

    const DOC: &str = "\
---
title: \"`{python} 1 + 1`\"
---

Prose with `{python} 2 + 2` inline.

```{python}
#| label: fig-plot
#| fig-width: 7
#| echo: false
import matplotlib
```

```{r}
y <- c(1, 2)
```
";

    /// A two-engine record that exercises every output kind and every record-level field.
    fn sample_record(shuffled: bool) -> Record {
        let mut units = scan_units(DOC);
        let texts = AuthorTexts::from_scanned(&units);
        fill_ids(&mut units, &texts).unwrap();

        // The same output object, built with its keys in either order.
        let display = if shuffled {
            json!({"metadata": {"b": 2, "a": 1}, "data": {"text/plain": "<Figure>", "image/png": "iVBORw0KGgo="}, "output_type": "display_data"})
        } else {
            json!({"output_type": "display_data", "data": {"image/png": "iVBORw0KGgo=", "text/plain": "<Figure>"}, "metadata": {"a": 1, "b": 2}})
        };
        let python = {
            let def = HashDefinition::conservative("jupyter");
            let mut b = RecordBuilder::new("jupyter", &def, &units, &texts).unwrap();
            for (i, u) in units.iter().enumerate() {
                if u.lang() != "python" {
                    continue;
                }
                let outputs = match i {
                    // The span.
                    0 | 1 => vec![json!({
                        "output_type": "execute_result",
                        "data": {"text/plain": "4"},
                        "execution_count": 1,
                        "metadata": {}
                    })],
                    _ => vec![
                        json!({"output_type": "stream", "name": "stdout", "text": ["hi\n"]}),
                        // An engine that forgot `execution_count`: the writer supplies null.
                        json!({"output_type": "execute_result", "data": {"text/plain": "1"}, "metadata": {}}),
                        display.clone(),
                    ],
                };
                b.push(
                    i,
                    UnitResult {
                        outputs,
                        execution_count: Some(i as u64 + 1),
                        kernel: Some("python3".into()),
                    },
                )
                .unwrap();
            }
            b.finish(EngineEntryInput {
                can_freeze: true,
                owned_languages: vec!["python".into()],
                provenance: BTreeMap::from([("kernelspec".into(), json!("python3"))]),
                includes: Some(if shuffled {
                    json!({"in_header": ["<style/>"], "before_body": ["<p/>"]})
                } else {
                    json!({"before_body": ["<p/>"], "in_header": ["<style/>"]})
                }),
                ..Default::default()
            })
            .unwrap()
        };
        let knitr = {
            let def = HashDefinition::conservative("knitr");
            let mut b = RecordBuilder::new("knitr", &def, &units, &texts).unwrap();
            for (i, u) in units.iter().enumerate() {
                if u.lang() == "r" {
                    b.push(
                        i,
                        UnitResult {
                            outputs: vec![json!({
                                "output_type": "display_data",
                                "data": {"text/x-quarto-markdown": "```\n[1] 1 2\n```\n"},
                                "metadata": {"quarto_format": "html"}
                            })],
                            execution_count: None,
                            kernel: None,
                        },
                    )
                    .unwrap();
                }
            }
            b.finish(EngineEntryInput {
                owned_languages: vec!["r".into()],
                ..Default::default()
            })
            .unwrap()
        };
        let mut record = coalesce(&[python, knitr], &units).unwrap();
        record.metadata.quarto_source = Some("docs/a.qmd".into());
        record.metadata.quarto_files.insert(
            "a_files/figure-html/plot.png".into(),
            RecordFile {
                data: "iVBORw0KGgo=".into(),
                size: 8,
            },
        );
        record.metadata.quarto_overlay = Some(BTreeMap::from([("params".into(), json!({"n": 1}))]));
        record
    }

    #[test]
    fn the_record_serializes_to_the_checked_in_golden_bytes() {
        let bytes = write_record(&sample_record(false)).unwrap();
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(GOLDEN_PATH);
            std::fs::write(path, &bytes).unwrap();
        }
        assert_eq!(
            bytes, GOLDEN,
            "the golden changed; rerun with UPDATE_GOLDEN=1 and review the diff"
        );
    }

    #[test]
    fn serializing_twice_gives_equal_bytes() {
        let record = sample_record(false);
        assert_eq!(
            write_record(&record).unwrap(),
            write_record(&record).unwrap()
        );
    }

    #[test]
    fn objects_built_in_a_different_key_order_give_identical_bytes() {
        assert_eq!(
            write_record(&sample_record(false)).unwrap(),
            write_record(&sample_record(true)).unwrap()
        );
    }

    #[test]
    fn the_bytes_follow_the_nbformat_convention() {
        let bytes = write_record(&sample_record(false)).unwrap();
        assert!(bytes.ends_with("}\n") && !bytes.ends_with("\n\n"));
        assert!(!bytes.contains('\r'), "LF endings only");
        assert!(
            bytes.starts_with("{\n \"cells\": ["),
            "one-space indent, sorted keys"
        );
        // Sorted at every level: the top-level keys come out cells, metadata, nbformat, ...
        let top: Vec<usize> = [
            "\"cells\"",
            "\"metadata\"",
            "\"nbformat\"",
            "\"nbformat_minor\"",
        ]
        .iter()
        .map(|k| bytes.find(&format!("\n {k}:")).unwrap())
        .collect();
        assert!(top.windows(2).all(|w| w[0] < w[1]), "{top:?}");
    }

    #[test]
    fn no_volatile_or_machine_specific_text_reaches_the_bytes() {
        let ts = regex::Regex::new(r"\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}").unwrap();
        for bytes in [
            GOLDEN.to_string(),
            write_record(&sample_record(false)).unwrap(),
        ] {
            assert!(!bytes.contains("last_updated"));
            assert!(!ts.is_match(&bytes), "a timestamp");
            for abs in [
                "/Users/",
                "/home/",
                "/tmp/",
                "/var/folders",
                "C:\\\\",
                "file://",
            ] {
                assert!(!bytes.contains(abs), "absolute path marker {abs:?}");
            }
        }
    }

    #[test]
    fn every_execute_result_carries_execution_count() {
        let value: Value =
            serde_json::from_str(&write_record(&sample_record(false)).unwrap()).unwrap();
        let mut seen = 0;
        for cell in value["cells"].as_array().unwrap() {
            for out in cell["outputs"].as_array().unwrap() {
                if out["output_type"] == "execute_result" {
                    seen += 1;
                    assert!(out.get("execution_count").is_some(), "{out}");
                }
            }
        }
        assert!(seen >= 2, "the sample has execute_results to check");
        // The engine's count survives; a missing one becomes null.
        let outs = &value["cells"][1]["outputs"];
        assert!(outs[1]["execution_count"].is_null(), "{outs}");
        assert_eq!(value["cells"][0]["outputs"][0]["execution_count"], 1);
    }

    #[test]
    fn the_reserved_sidecar_field_is_never_emitted() {
        let mut record = sample_record(false);
        record.metadata.quarto_files_ref = Some(json!("sidecar.bin"));
        assert!(!write_record(&record).unwrap().contains("quarto_files_ref"));
    }

    #[test]
    fn the_written_bytes_read_back_to_the_same_record() {
        let record = sample_record(false);
        let back: Record = serde_json::from_str(&write_record(&record).unwrap()).unwrap();
        // Only the writer's one normalization differs: the missing `execution_count`.
        let mut expected = record;
        for cell in &mut expected.cells {
            cell.outputs.iter_mut().for_each(ensure_execution_count);
        }
        assert_eq!(back, expected);
    }

    #[test]
    fn non_ascii_text_is_written_unescaped() {
        let mut record = sample_record(false);
        record.cells[1].outputs =
            vec![json!({"output_type": "stream", "name": "stdout", "text": ["café 😀\n"]})];
        assert!(write_record(&record).unwrap().contains("café 😀"));
    }
}
