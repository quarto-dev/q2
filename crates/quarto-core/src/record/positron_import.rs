//! Display-only import of Positron's output cache (EXR-4 Phase 5; EXR-16 specifies the rules).
//!
//! Positron's cache is never a source of truth for q2: imported outputs carry a
//! [`Provenance::Positron`] marker, live in their own [`DisplayOnlyOutputs`] type that
//! converts to nothing a reuse or freeze decision takes, and are normalized at read so the
//! rest of q2 only ever sees standard nbformat. A file this reader cannot trust (a wrong
//! `quarto_cache_version`, an invalid structure) is rejected, never deleted, unlike Positron's
//! own loader.
//!
//! Positron's cells carry no language, so the language gate runs on the q2 side, when a unit
//! asks for outputs ([`PositronImport::outputs_for`]).

use std::collections::BTreeMap;

use serde_json::Value;

/// The only Positron cache version this importer understands.
pub const POSITRON_CACHE_VERSION: u64 = 1;

/// The engine that owns plain kernel-language units, the only ones Positron outputs may enter.
pub const JUPYTER_ENGINE: &str = "jupyter";

/// Where imported outputs came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provenance {
    Positron,
}

/// Outputs imported from Positron: for display only. There is deliberately no conversion to
/// a `UnitResult` or `RecordCell`, so they cannot feed reuse or freeze (no provenance, possibly
/// out of order, stale kernel state).
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayOnlyOutputs {
    provenance: Provenance,
    outputs: Vec<Value>,
}

impl DisplayOnlyOutputs {
    pub fn provenance(&self) -> Provenance {
        self.provenance
    }

    /// Normalized, spec-conformant nbformat outputs.
    pub fn outputs(&self) -> &[Value] {
        &self.outputs
    }
}

/// Why a Positron cache file was rejected (the caller leaves the file alone).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ImportError {
    #[error("Positron cache is not valid JSON: {0}")]
    Json(String),
    #[error("Positron cache has an invalid structure: {0}")]
    InvalidStructure(&'static str),
    #[error("Positron cache version {found} is not supported (expected {POSITRON_CACHE_VERSION})")]
    VersionMismatch { found: String },
}

/// A parsed Positron cache: outputs keyed by content hash, in file order per hash.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PositronImport {
    cells: BTreeMap<String, Vec<DisplayOnlyOutputs>>,
    /// Non-fatal normalization notices (for example a JSON mime that was not valid JSON).
    pub notices: Vec<String>,
}

/// Read a Positron cache file.
pub fn import_positron_cache(bytes: &[u8]) -> Result<PositronImport, ImportError> {
    let root: Value =
        serde_json::from_slice(bytes).map_err(|e| ImportError::Json(e.to_string()))?;
    // The checks Positron's own loader makes, except that a failure rejects here.
    let obj = root
        .as_object()
        .ok_or(ImportError::InvalidStructure("not an object"))?;
    if !obj.get("nbformat").is_some_and(Value::is_number) {
        return Err(ImportError::InvalidStructure("`nbformat` is not a number"));
    }
    let cells = obj
        .get("cells")
        .and_then(Value::as_array)
        .ok_or(ImportError::InvalidStructure("`cells` is not an array"))?;
    let metadata = obj
        .get("metadata")
        .and_then(Value::as_object)
        .ok_or(ImportError::InvalidStructure("`metadata` is not an object"))?;
    let version = metadata
        .get("quarto_cache_version")
        .filter(|v| v.is_number())
        .ok_or(ImportError::InvalidStructure(
            "`metadata.quarto_cache_version` is not a number",
        ))?;
    if version.as_u64() != Some(POSITRON_CACHE_VERSION) {
        return Err(ImportError::VersionMismatch {
            found: version.to_string(),
        });
    }

    let mut import = PositronImport::default();
    for cell in cells {
        if cell.get("cell_type").and_then(Value::as_str) != Some("code") {
            continue;
        }
        let Some(hash) = cell
            .get("metadata")
            .and_then(|m| m.get("quarto_content_hash"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        let Some(raw) = cell.get("outputs").and_then(Value::as_array) else {
            continue;
        };
        let outputs = raw
            .iter()
            .filter_map(|o| normalize_output(o, &mut import.notices))
            .collect();
        import
            .cells
            .entry(hash.to_string())
            .or_default()
            .push(DisplayOnlyOutputs {
                provenance: Provenance::Positron,
                outputs,
            });
    }
    Ok(import)
}

impl PositronImport {
    /// The imported outputs for the `occurrence`-th unit (0-based, in document order) whose
    /// content hash is `content_hash`, or `None` (a stale or unmatched unit is skipped
    /// silently). Pairing the nth unit with the nth cached cell of the same hash is the
    /// occurrence rule for duplicates.
    ///
    /// The language gate: outputs go only to a unit the Jupyter engine owns whose language
    /// token is a plain kernel language, never to `python.marimo` (Positron reads it as
    /// `python`) or to a unit another engine owns. It cannot catch a cell of a different
    /// language with identical text, since the hash has no language.
    pub fn outputs_for(
        &self,
        content_hash: &str,
        occurrence: usize,
        owner: &str,
        lang: &str,
    ) -> Option<&DisplayOnlyOutputs> {
        if owner != JUPYTER_ENGINE || !is_plain_language(lang) {
            return None;
        }
        self.cells.get(content_hash)?.get(occurrence)
    }

    /// How many distinct content hashes the file holds.
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
}

/// Positron's fence language is `\w+`; q2's gate accepts exactly that.
fn is_plain_language(lang: &str) -> bool {
    !lang.is_empty() && lang.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// Normalize one Positron output to standard nbformat. Positron collapses every rich output
/// to `execute_result`; the standard form for a display-only import is `display_data` (no
/// execution count to invent). `stream` and `error` pass through.
fn normalize_output(output: &Value, notices: &mut Vec<String>) -> Option<Value> {
    let obj = output.as_object()?;
    match obj.get("output_type").and_then(Value::as_str)? {
        "stream" | "error" => Some(output.clone()),
        "execute_result" | "display_data" => {
            let mut data = obj.get("data").and_then(Value::as_object)?.clone();
            for (mime, value) in &mut data {
                if is_json_mime(mime)
                    && let Value::String(s) = value
                {
                    match serde_json::from_str::<Value>(s) {
                        Ok(parsed) => *value = parsed,
                        Err(e) => {
                            notices.push(format!("kept `{mime}` as a string: not valid JSON ({e})"))
                        }
                    }
                }
            }
            let mut metadata = obj
                .get("metadata")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            metadata.remove("quarto_webview_metadata");
            Some(serde_json::json!({
                "output_type": "display_data",
                "data": data,
                "metadata": metadata,
            }))
        }
        _ => None,
    }
}

fn is_json_mime(mime: &str) -> bool {
    mime == "application/json" || mime.ends_with("+json")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A Positron-written cache in its exact shape: every rich output is an `execute_result`,
    /// every payload a string, with `quarto_webview_metadata`.
    fn positron_file() -> Vec<u8> {
        serde_json::to_vec(&json!({
            "nbformat": 4,
            "nbformat_minor": 5,
            "metadata": {
                "quarto_source": "file:///proj/a.qmd",
                "quarto_cache_version": 1,
                "last_updated": 1_761_900_000_000_u64
            },
            "cells": [
                {
                    "cell_type": "code",
                    "execution_count": null,
                    "metadata": {
                        "quarto_cell_id": "0-aaaaaaaa-unlabeled",
                        "quarto_content_hash": "aaaaaaaaaaaaaaaa"
                    },
                    "outputs": [
                        {"output_type": "stream", "name": "stdout", "text": ["hi\n"]},
                        {
                            "output_type": "execute_result",
                            "execution_count": null,
                            "data": {
                                "text/plain": "<Figure>",
                                "application/json": "{\"a\": 1}",
                                "application/vnd.x+json": "{\"b\": [1, 2]}"
                            },
                            "metadata": {"quarto_webview_metadata": {"w": 1}, "k": "v"}
                        }
                    ],
                    "source": ["1 + 1"]
                },
                {
                    "cell_type": "code",
                    "execution_count": null,
                    "metadata": {
                        "quarto_cell_id": "1-bbbbbbbb-unlabeled",
                        "quarto_content_hash": "bbbbbbbbbbbbbbbb"
                    },
                    "outputs": [{
                        "output_type": "execute_result",
                        "execution_count": null,
                        "data": {"application/json": "not json {"},
                        "metadata": {}
                    }],
                    "source": ["x"]
                },
                {"cell_type": "markdown", "metadata": {}, "source": ["# not a cell"]}
            ]
        }))
        .unwrap()
    }

    fn with_version(v: Value) -> Vec<u8> {
        let mut file: Value = serde_json::from_slice(&positron_file()).unwrap();
        file["metadata"]["quarto_cache_version"] = v;
        serde_json::to_vec(&file).unwrap()
    }

    #[test]
    fn outputs_are_matched_by_content_hash_and_marked_display_only() {
        let imp = import_positron_cache(&positron_file()).unwrap();
        assert_eq!(imp.len(), 2);
        let got = imp
            .outputs_for("aaaaaaaaaaaaaaaa", 0, "jupyter", "python")
            .unwrap();
        assert_eq!(got.provenance(), Provenance::Positron);
        assert_eq!(got.outputs().len(), 2);
        // Unmatched or stale: skipped silently.
        assert!(
            imp.outputs_for("cccccccccccccccc", 0, "jupyter", "python")
                .is_none()
        );
        // A second unit with the same hash has no second cached cell.
        assert!(
            imp.outputs_for("aaaaaaaaaaaaaaaa", 1, "jupyter", "python")
                .is_none()
        );
    }

    /// EXR-16 S7(a): the importer drops only `quarto_webview_metadata`; any other per-output
    /// metadata key, including a nested one a self-contained widget might carry, survives.
    #[test]
    fn an_extra_per_output_metadata_key_survives_the_import() {
        let mut file: Value = serde_json::from_slice(&positron_file()).unwrap();
        file["cells"][0]["outputs"][1]["metadata"] = json!({
            "quarto_webview_metadata": {"w": 1},
            "quarto_widget": {"self_contained": true, "deps": ["a.js"]},
            "k": "v"
        });
        let imp = import_positron_cache(&serde_json::to_vec(&file).unwrap()).unwrap();
        let out = &imp
            .outputs_for("aaaaaaaaaaaaaaaa", 0, "jupyter", "python")
            .unwrap()
            .outputs()[1];
        assert_eq!(
            out["metadata"],
            json!({"quarto_widget": {"self_contained": true, "deps": ["a.js"]}, "k": "v"})
        );
    }

    #[test]
    fn a_rich_execute_result_imports_as_standard_display_data() {
        let imp = import_positron_cache(&positron_file()).unwrap();
        let out = &imp
            .outputs_for("aaaaaaaaaaaaaaaa", 0, "jupyter", "python")
            .unwrap()
            .outputs()[1];
        assert_eq!(out["output_type"], "display_data");
        assert!(out.get("execution_count").is_none());
        assert_eq!(out["data"]["text/plain"], "<Figure>");
        // The webview metadata is dropped; other metadata survives.
        assert_eq!(out["metadata"], json!({"k": "v"}));
        // `stream` passes through.
        let stream = &imp
            .outputs_for("aaaaaaaaaaaaaaaa", 0, "jupyter", "python")
            .unwrap()
            .outputs()[0];
        assert_eq!(stream["output_type"], "stream");
    }

    #[test]
    fn string_valued_json_mimes_become_objects() {
        let imp = import_positron_cache(&positron_file()).unwrap();
        let out = &imp
            .outputs_for("aaaaaaaaaaaaaaaa", 0, "jupyter", "python")
            .unwrap()
            .outputs()[1];
        assert_eq!(out["data"]["application/json"], json!({"a": 1}));
        assert_eq!(out["data"]["application/vnd.x+json"], json!({"b": [1, 2]}));
        // Only the deliberately invalid string in the other cell produces a notice.
        assert_eq!(imp.notices.len(), 1);
    }

    #[test]
    fn an_invalid_json_string_is_kept_with_a_notice() {
        let imp = import_positron_cache(&positron_file()).unwrap();
        let out = &imp
            .outputs_for("bbbbbbbbbbbbbbbb", 0, "jupyter", "python")
            .unwrap()
            .outputs()[0];
        assert_eq!(out["data"]["application/json"], "not json {");
        assert_eq!(imp.notices.len(), 1);
        assert!(imp.notices[0].contains("application/json"));
    }

    #[test]
    fn the_language_gate_runs_on_the_q2_side() {
        let imp = import_positron_cache(&positron_file()).unwrap();
        let h = "aaaaaaaaaaaaaaaa";
        assert!(imp.outputs_for(h, 0, "jupyter", "python").is_some());
        assert!(imp.outputs_for(h, 0, "jupyter", "Python").is_some());
        // Positron reads `{python.marimo}` as python; q2 owns it with marimo.
        assert!(imp.outputs_for(h, 0, "jupyter", "python.marimo").is_none());
        assert!(imp.outputs_for(h, 0, "marimo", "python.marimo").is_none());
        // Another engine's unit never receives Positron outputs.
        assert!(imp.outputs_for(h, 0, "knitr", "r").is_none());
        // Documented gap: the hash has no language, so a `{julia}` unit with the same text as
        // a Positron `{python}` cell still matches. Only the owner check and the occurrence
        // rule guard it.
        assert!(imp.outputs_for(h, 0, "jupyter", "julia").is_some());
    }

    #[test]
    fn a_version_mismatch_or_invalid_structure_is_rejected() {
        assert!(matches!(
            import_positron_cache(&with_version(json!(2))),
            Err(ImportError::VersionMismatch { .. })
        ));
        assert!(matches!(
            import_positron_cache(&with_version(json!("1"))),
            Err(ImportError::InvalidStructure(_))
        ));
        assert!(matches!(
            import_positron_cache(b"{"),
            Err(ImportError::Json(_))
        ));
        for bad in [
            json!([]),
            json!({"nbformat": "4", "cells": [], "metadata": {"quarto_cache_version": 1}}),
            json!({"nbformat": 4, "cells": {}, "metadata": {"quarto_cache_version": 1}}),
            json!({"nbformat": 4, "cells": [], "metadata": 3}),
        ] {
            assert!(matches!(
                import_positron_cache(&serde_json::to_vec(&bad).unwrap()),
                Err(ImportError::InvalidStructure(_))
            ));
        }
    }

    #[test]
    fn a_cell_without_an_outputs_array_is_skipped_not_fatal() {
        let file = json!({
            "nbformat": 4, "nbformat_minor": 5,
            "metadata": {"quarto_cache_version": 1},
            "cells": [{"cell_type": "code", "metadata": {"quarto_content_hash": "h"}, "source": []}]
        });
        let imp = import_positron_cache(&serde_json::to_vec(&file).unwrap()).unwrap();
        assert!(imp.is_empty());
    }

    #[test]
    fn a_q2_record_is_accepted_as_a_cache_file_while_the_key_is_one() {
        // The defensive `quarto_cache_version: 1` makes a q2 record structurally acceptable.
        let golden = include_str!("../../tests/fixtures/records/golden.ipynb");
        assert!(import_positron_cache(golden.as_bytes()).is_ok());
    }
}
