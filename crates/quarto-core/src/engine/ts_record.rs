/*
 * engine/ts_record.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * EXR-7: the unit list a TS engine receives and the record built from the
 * notebook it returns.
 */

//! The two halves of EXR-7's engine wire that need core's unit machinery.
//!
//! - [`wire_units`]: the stage's one shared unit list, relocated into the text
//!   this engine receives, as [`TsUnit`]s.
//! - [`attach_record`]: the engine's returned notebook, read leniently and
//!   rebuilt as an [`EngineRecord`] by [`RecordBuilder`].
//!
//! The notebook an engine returns is a plain nbformat-4 notebook, **not** one of
//! core's records: it has no `quarto_record_version`, no hashes and no
//! `quarto_` cell metadata beyond what it chooses to echo. So it is not read
//! with `record::read_record_value` (that validates core's own record format).
//! Core takes only outputs, execution count and kernel from it; the source,
//! ids, hashes and location of every cell come from the stage's unit, so code
//! an engine trimmed or rewrote cannot change matching.
//!
//! How a cell names its unit (`alignment: "ids"`): its `metadata.quarto_cell_id`
//! is the unit's wire `id`. The kernel is the cell's `metadata.quarto_kernel`,
//! else the notebook's `metadata.kernelspec.name`.

use std::collections::HashMap;

use serde_json::Value;

use crate::engine::context::{ExecuteResult, ExecutionContext};
use crate::engine::diagnostics::{EngineRecordProblem, engine_record_diagnostic};
use crate::engine::ts_protocol::{TsAlignment, TsCoverage, TsRecordInfo, TsUnit, TsUnitKind};
use crate::engine::units::{Unit, line_at, locate_units, view_for_engine};
use crate::record::{BuildError, EngineEntryInput, RecordBuilder, UnitResult};

/// Whether `owned_languages` contains `lang`, case-insensitively (the same rule
/// `view_for_engine` applies by lowercasing the token first).
fn owns(ctx: &ExecutionContext, lang: &str) -> bool {
    ctx.owned_languages
        .iter()
        .any(|o| o.eq_ignore_ascii_case(lang))
}

fn unit_id(unit: &Unit) -> Option<&str> {
    match unit {
        Unit::Cell(c) => c.id.as_ref().map(|i| i.0.as_str()),
        Unit::Inline(i) => i.id.as_ref().map(|i| i.0.as_str()),
    }
}

/// The wire form of the stage's unit list for the engine receiving `input`.
/// `None` when the context carries no units (an engine driven without the stage).
///
/// `id` and `index` keep the shared scan's values, `owned` is this engine's view,
/// and `line` is re-derived in `input` (units carry byte offsets only, so the line
/// is computed here, in the text the engine actually holds).
pub(super) fn wire_units(input: &str, ctx: &ExecutionContext) -> Option<Vec<TsUnit>> {
    if ctx.units.is_empty() {
        return None;
    }
    let located = locate_units(&ctx.units, input);
    let view = view_for_engine(&located, &|lang| owns(ctx, lang));
    Some(
        view.into_iter()
            .map(|eu| {
                let (kind, index, in_front_matter) = match eu.unit {
                    Unit::Cell(c) => (TsUnitKind::Cell, Some(c.index), false),
                    Unit::Inline(i) => (TsUnitKind::Inline, None, i.in_front_matter),
                };
                TsUnit {
                    id: unit_id(eu.unit).map(str::to_string),
                    index,
                    kind,
                    language: eu.unit.lang().to_string(),
                    line: line_at(input, eu.unit.start()),
                    owned: eu.owned,
                    in_front_matter,
                }
            })
            .collect(),
    )
}

/// One cell of the engine's notebook, reduced to what core takes from it.
struct NotebookCell {
    unit_id: Option<String>,
    is_code: bool,
    result: UnitResult,
}

/// Read an engine's notebook leniently: nbformat 4 with a `cells` array, and any
/// cell present must be well formed. Nothing else is required.
fn read_engine_notebook(nb: &Value) -> Result<Vec<NotebookCell>, String> {
    let obj = nb.as_object().ok_or("the notebook is not a JSON object")?;
    match obj.get("nbformat").and_then(Value::as_u64) {
        Some(4) => {}
        _ => return Err("`nbformat` is not 4".to_string()),
    }
    let cells = obj
        .get("cells")
        .and_then(Value::as_array)
        .ok_or("`cells` is missing or not an array")?;
    let notebook_kernel = obj
        .get("metadata")
        .and_then(|m| m.get("kernelspec"))
        .and_then(|k| k.get("name"))
        .and_then(Value::as_str);
    cells
        .iter()
        .enumerate()
        .map(|(i, cell)| {
            let cell = cell
                .as_object()
                .ok_or_else(|| format!("cell {i} is not a JSON object"))?;
            let outputs = match cell.get("outputs") {
                None | Some(Value::Null) => Vec::new(),
                Some(Value::Array(a)) => a.clone(),
                Some(_) => return Err(format!("cell {i}: `outputs` is not an array")),
            };
            let execution_count = match cell.get("execution_count") {
                None | Some(Value::Null) => None,
                Some(v) => Some(v.as_u64().ok_or_else(|| {
                    format!("cell {i}: `execution_count` is not a non-negative integer")
                })?),
            };
            let metadata = cell.get("metadata");
            let meta_str = |key: &str| {
                metadata
                    .and_then(|m| m.get(key))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            };
            Ok(NotebookCell {
                unit_id: meta_str("quarto_cell_id"),
                is_code: cell.get("cell_type").and_then(Value::as_str) == Some("code"),
                result: UnitResult {
                    outputs,
                    execution_count,
                    kernel: meta_str("quarto_kernel")
                        .or_else(|| notebook_kernel.map(str::to_string)),
                },
            })
        })
        .collect()
}

/// Build the engine's record from its notebook.
///
/// The engine's own units are the owned units in the text it received. Those the
/// stage numbered go into the record; one an earlier engine's output introduced
/// (`id: None`) cannot, since the builder takes the stage's list, so it is run
/// by the engine but never recorded. Under `ids` alignment such a unit has no
/// id to match, so the count is over the engine's numbered units; under
/// `positional` it still occupies its place in the order.
fn build_record(
    engine: &str,
    input: &str,
    ctx: &ExecutionContext,
    can_freeze: bool,
    notebook: &Value,
    info: &TsRecordInfo,
) -> Result<crate::record::EngineRecord, EngineRecordProblem> {
    let cells = read_engine_notebook(notebook)
        .map_err(|detail| EngineRecordProblem::Malformed { detail })?;

    // The stage's index for each unit id.
    let by_id: HashMap<&str, usize> = ctx
        .units
        .iter()
        .enumerate()
        .filter_map(|(i, u)| unit_id(u).map(|id| (id, i)))
        .collect();
    let located = locate_units(&ctx.units, input);
    let owned: Vec<Option<usize>> = view_for_engine(&located, &|lang| owns(ctx, lang))
        .into_iter()
        .filter(|eu| eu.owned)
        .map(|eu| unit_id(eu.unit).and_then(|id| by_id.get(id).copied()))
        .collect();

    // (stage unit index, engine result) for every cell that lines up with a unit.
    let mut pairs: Vec<(usize, UnitResult)> = Vec::new();
    match info.alignment {
        TsAlignment::Ids => {
            let expected = owned.iter().flatten().count();
            let annotated: Vec<&NotebookCell> =
                cells.iter().filter(|c| c.unit_id.is_some()).collect();
            // A `subset` engine applies its own ownership rule, so the count is its to
            // validate; each id is still checked below.
            if info.coverage == TsCoverage::Owned && annotated.len() != expected {
                return Err(EngineRecordProblem::CountMismatch {
                    expected,
                    found: annotated.len(),
                });
            }
            for cell in annotated {
                let id = cell.unit_id.as_deref().expect("filtered on Some");
                let index = by_id
                    .get(id)
                    .copied()
                    .filter(|i| owned.contains(&Some(*i)))
                    .ok_or_else(|| EngineRecordProblem::UnknownUnitId { id: id.to_string() })?;
                if pairs.iter().any(|(i, _)| *i == index) {
                    return Err(EngineRecordProblem::DuplicateUnitId { id: id.to_string() });
                }
                pairs.push((index, cell.result.clone()));
            }
        }
        TsAlignment::Positional if info.coverage == TsCoverage::Subset => {
            return Err(EngineRecordProblem::Malformed {
                detail: "`coverage: subset` needs `alignment: ids`".to_string(),
            });
        }
        TsAlignment::Positional => {
            let code: Vec<&NotebookCell> = cells.iter().filter(|c| c.is_code).collect();
            if code.len() != owned.len() {
                return Err(EngineRecordProblem::CountMismatch {
                    expected: owned.len(),
                    found: code.len(),
                });
            }
            for (slot, cell) in owned.iter().zip(code) {
                if let Some(index) = slot {
                    pairs.push((*index, cell.result.clone()));
                }
            }
        }
    }

    let build = |e: BuildError| EngineRecordProblem::Build {
        detail: e.to_string(),
    };
    let def = info.hash_definition_or_conservative(engine);
    let mut builder =
        RecordBuilder::new(engine, &def, &ctx.units, &ctx.author_texts).map_err(build)?;
    for (index, result) in pairs {
        builder.push(index, result).map_err(build)?;
    }
    builder
        .finish(EngineEntryInput {
            can_freeze,
            owned_languages: ctx.owned_languages.clone(),
            document_metadata: ctx.document_metadata_json(),
            ..Default::default()
        })
        .map_err(build)
}

/// Turn the engine's returned notebook into `result.record`, or push a warning
/// and leave the record `None`. Does nothing when no record is wanted, the
/// context has no units, or the engine returned no notebook.
pub(super) fn attach_record(
    engine: &str,
    input: &str,
    ctx: &ExecutionContext,
    can_freeze: bool,
    notebook: Option<Value>,
    info: Option<TsRecordInfo>,
    result: &mut ExecuteResult,
) {
    let Some(notebook) = notebook else { return };
    if !ctx.want_record || ctx.units.is_empty() {
        return;
    }
    let info = info.unwrap_or_else(default_record_info);
    match build_record(engine, input, ctx, can_freeze, &notebook, &info) {
        Ok(record) => result.record = Some(record),
        Err(problem) => result
            .warnings
            .push(engine_record_diagnostic(engine, &problem)),
    }
}

/// `recordInfo` omitted: every field's wire default.
fn default_record_info() -> TsRecordInfo {
    serde_json::from_str("{}").expect("every recordInfo field has a default")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::*;
    use crate::engine::units::scan_units;
    use crate::record::{AuthorTexts, Granularity, HashDefinition, fill_ids};

    const DOC: &str = "\
---
title: t
subtitle: \"`{python} 1 + 1`\"
---

Inline `{python} 2 + 2` and `{r} 3` here.

```{r}
y <- 2
```

```{python}
#| label: first
x = 1
```

```{python}
z = 3
```
";

    /// Units with ids filled and their author texts, as the stage builds them.
    fn staged(doc: &str) -> (Vec<Unit>, AuthorTexts) {
        let mut units = scan_units(doc);
        let texts = AuthorTexts::from_scanned(&units);
        fill_ids(&mut units, &texts).unwrap();
        (units, texts)
    }

    fn ctx_for(doc: &str, owned: &[&str], want_record: bool) -> ExecutionContext {
        let (units, texts) = staged(doc);
        ExecutionContext::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/proj"),
            PathBuf::from("/proj/doc.qmd"),
            "html",
        )
        .with_owned_languages(owned.iter().map(|s| s.to_string()).collect())
        .with_units(units)
        .with_author_texts(texts)
        .with_want_record(want_record)
    }

    fn cell_ids(ctx: &ExecutionContext, lang: &str) -> Vec<String> {
        ctx.units
            .iter()
            .filter(|u| matches!(u, Unit::Cell(_)) && u.lang() == lang)
            .map(|u| match u {
                Unit::Cell(c) => c.id.as_ref().unwrap().0.clone(),
                Unit::Inline(_) => unreachable!(),
            })
            .collect()
    }

    // ---- wire units ----

    #[test]
    fn no_units_on_the_context_means_no_units_on_the_wire() {
        let ctx = ExecutionContext::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/proj"),
            PathBuf::from("/proj/doc.qmd"),
            "html",
        );
        assert_eq!(wire_units(DOC, &ctx), None);
    }

    #[test]
    fn stage_units_reach_every_engine_with_filled_ids() {
        let ctx = ctx_for(DOC, &["python"], false);
        let wire = wire_units(DOC, &ctx).unwrap();
        assert_eq!(wire.len(), ctx.units.len());
        assert!(wire.iter().all(|u| u.id.is_some()), "{wire:?}");
        // Ceded units are listed with owned: false.
        assert!(wire.iter().any(|u| !u.owned && u.language == "r"));
        assert!(wire.iter().any(|u| u.owned && u.language == "python"));
    }

    #[test]
    fn wire_cell_units_carry_index_and_span_units_do_not() {
        let ctx = ctx_for(DOC, &["python", "r"], false);
        let wire = wire_units(DOC, &ctx).unwrap();
        let cells: Vec<_> = wire.iter().filter(|u| u.kind == TsUnitKind::Cell).collect();
        assert_eq!(
            cells.iter().map(|u| u.index).collect::<Vec<_>>(),
            vec![Some(0), Some(1), Some(2)]
        );
        for span in wire.iter().filter(|u| u.kind == TsUnitKind::Inline) {
            assert_eq!(span.index, None);
            let v = serde_json::to_value(span).unwrap();
            assert!(v.get("index").is_none(), "{v}");
        }
        // The line is the unit's own line in `input`.
        let first = cells[1];
        assert_eq!(DOC.lines().nth(first.line - 1), Some("```{python}"));
    }

    #[test]
    fn front_matter_span_unit_carries_in_front_matter_true() {
        let ctx = ctx_for(DOC, &["python"], false);
        let wire = wire_units(DOC, &ctx).unwrap();
        let spans: Vec<_> = wire
            .iter()
            .filter(|u| u.kind == TsUnitKind::Inline && u.language == "python")
            .collect();
        assert_eq!(spans.len(), 2);
        let fm = serde_json::to_value(spans[0]).unwrap();
        assert_eq!(fm["inFrontMatter"], true);
        let body = serde_json::to_value(spans[1]).unwrap();
        assert!(body.get("inFrontMatter").is_none(), "{body}");
    }

    #[test]
    fn second_engine_units_keep_original_ids_and_locate_lines_in_its_text() {
        let ctx = ctx_for(DOC, &["python"], false);
        // Engine 1 (r) ran: its fence is gone from the text engine 2 receives.
        let engine2_text = DOC.replace("```{r}\ny <- 2\n```\n\n", "[1] 2\n\n");
        assert_ne!(engine2_text, DOC);
        let wire = wire_units(&engine2_text, &ctx).unwrap();

        // The r cell is gone; the python cells keep their document-wide ids and indexes.
        assert!(
            !wire
                .iter()
                .any(|u| u.kind == TsUnitKind::Cell && u.language == "r")
        );
        let py: Vec<_> = wire.iter().filter(|u| u.kind == TsUnitKind::Cell).collect();
        assert_eq!(py.len(), 2);
        assert_eq!(
            py.iter().map(|u| u.id.clone().unwrap()).collect::<Vec<_>>(),
            cell_ids(&ctx, "python")
        );
        assert_eq!(
            py.iter().map(|u| u.index).collect::<Vec<_>>(),
            vec![Some(1), Some(2)]
        );
        assert!(py.iter().all(|u| u.owned));
        // Lines point into engine 2's text, not the original.
        for u in &py {
            assert_eq!(engine2_text.lines().nth(u.line - 1), Some("```{python}"));
        }
        let original_line = wire_units(DOC, &ctx)
            .unwrap()
            .into_iter()
            .find(|u| u.index == Some(1))
            .unwrap()
            .line;
        assert_ne!(py[0].line, original_line);
    }

    // ---- attach_record ----

    fn notebook(cells: Vec<Value>) -> Value {
        json!({"nbformat": 4, "nbformat_minor": 5, "metadata": {"kernelspec": {"name": "nb-kernel"}}, "cells": cells})
    }

    fn cell(id: Option<&str>, out: &str, count: Option<u64>) -> Value {
        let mut metadata = json!({});
        if let Some(id) = id {
            metadata["quarto_cell_id"] = json!(id);
        }
        json!({
            "cell_type": "code",
            "execution_count": count,
            "metadata": metadata,
            "outputs": [{"output_type": "stream", "name": "stdout", "text": [out]}],
            "source": ["trimmed by the engine"],
        })
    }

    fn owned_python_ids(ctx: &ExecutionContext) -> Vec<String> {
        wire_units(DOC, ctx)
            .unwrap()
            .into_iter()
            .filter(|u| u.owned)
            .map(|u| u.id.unwrap())
            .collect()
    }

    fn run(ctx: &ExecutionContext, nb: Option<Value>, info: Option<TsRecordInfo>) -> ExecuteResult {
        let mut result = ExecuteResult::default();
        attach_record("julia", DOC, ctx, false, nb, info, &mut result);
        result
    }

    #[test]
    fn ids_alignment_builds_a_record_from_outputs_only() {
        let ctx = ctx_for(DOC, &["python"], true);
        let ids = owned_python_ids(&ctx);
        let cells: Vec<Value> = ids
            .iter()
            .enumerate()
            .map(|(i, id)| cell(Some(id), &format!("out{i}\n"), Some(i as u64 + 1)))
            .collect();
        let result = run(&ctx, Some(notebook(cells)), None);
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        let record = result.record.expect("a record");
        assert_eq!(record.engine.name, "julia");
        assert_eq!(record.engine.owned_languages, vec!["python".to_string()]);
        assert_eq!(record.cells.len(), ids.len());
        for (rc, id) in record.cells.iter().zip(&ids) {
            assert_eq!(&rc.metadata.quarto_cell_id, id);
            assert_eq!(rc.metadata.quarto_engine.as_deref(), Some("julia"));
            assert_eq!(rc.metadata.quarto_kernel.as_deref(), Some("nb-kernel"));
            assert!(!rc.source.concat().contains("trimmed"), "{:?}", rc.source);
            assert_eq!(rc.outputs.len(), 1);
        }
        assert_eq!(record.cells[0].execution_count, Some(1));
        // No declaration: the conservative default named for the engine.
        assert_eq!(
            record.engine.hash_definition,
            HashDefinition::conservative("julia")
        );
    }

    #[test]
    fn declared_hash_definition_is_carried_into_the_entry() {
        let ctx = ctx_for(DOC, &["python"], true);
        let cells: Vec<Value> = owned_python_ids(&ctx)
            .iter()
            .map(|id| cell(Some(id), "o\n", None))
            .collect();
        let mut def = HashDefinition::conservative("julia");
        def.granularity = Granularity::Document;
        def.presentation_cell_options = vec!["echo".into()];
        let info = TsRecordInfo {
            version: 1,
            granularity: Granularity::Document,
            output_kind: Default::default(),
            alignment: TsAlignment::Ids,
            coverage: Default::default(),
            hash_definition: Some(def.clone()),
        };
        let record = run(&ctx, Some(notebook(cells)), Some(info)).record.unwrap();
        assert_eq!(record.engine.hash_definition, def);
    }

    #[test]
    fn positional_alignment_matches_owned_units_by_order() {
        let ctx = ctx_for(DOC, &["python"], true);
        let n = owned_python_ids(&ctx).len();
        let cells: Vec<Value> = (0..n)
            .map(|i| cell(None, &format!("p{i}\n"), None))
            .collect();
        let info = TsRecordInfo {
            version: 1,
            granularity: Granularity::Unit,
            output_kind: Default::default(),
            alignment: TsAlignment::Positional,
            coverage: Default::default(),
            hash_definition: None,
        };
        let record = run(&ctx, Some(notebook(cells)), Some(info)).record.unwrap();
        let ids: Vec<_> = record
            .cells
            .iter()
            .map(|c| c.metadata.quarto_cell_id.clone())
            .collect();
        assert_eq!(ids, owned_python_ids(&ctx));
        assert!(
            record.cells[0].outputs[0]["text"][0]
                .as_str()
                .unwrap()
                .starts_with("p0")
        );
    }

    #[test]
    fn unit_count_mismatch_is_an_engine_diagnostic() {
        let ctx = ctx_for(DOC, &["python"], true);
        let ids = owned_python_ids(&ctx);
        let expected = ids.len();
        // One annotated cell fewer than the engine owns.
        let cells = vec![cell(Some(&ids[0]), "o\n", None)];
        let result = run(&ctx, Some(notebook(cells)), None);
        assert!(result.record.is_none(), "the record is dropped");
        assert_eq!(result.warnings.len(), 1);
        let d = &result.warnings[0];
        assert_eq!(d.kind, quarto_error_reporting::DiagnosticKind::Warning);
        let text = d.to_text(None);
        assert!(text.contains("julia"), "{text}");
        assert!(text.contains(&format!("{expected} units")), "{text}");
        assert!(text.contains("returned 1 annotated"), "{text}");
        // Counted over the engine's own units: the ceded `r` units are not in `expected`.
        assert!(ctx.units.len() > expected);
    }

    /// What the marimo engine returns as `recordInfo` (EXR-9 Phase 9A): `ids` alignment, the
    /// default `owned` coverage, and the document-granularity declaration.
    fn marimo_info() -> TsRecordInfo {
        let mut def = HashDefinition::conservative("marimo");
        def.granularity = Granularity::Document;
        def.document_keys = vec!["eval".into(), "external-env".into(), "pyproject".into()];
        TsRecordInfo {
            version: 1,
            granularity: Granularity::Document,
            output_kind: Default::default(),
            alignment: TsAlignment::Ids,
            coverage: Default::default(),
            hash_definition: Some(def),
        }
    }

    fn run_marimo(
        ctx: &ExecutionContext,
        nb: Option<Value>,
        info: Option<TsRecordInfo>,
    ) -> ExecuteResult {
        let mut result = ExecuteResult::default();
        attach_record("marimo", DOC, ctx, false, nb, info, &mut result);
        result
    }

    #[test]
    fn marimo_declaration_is_stored_with_the_engine_entry() {
        let ctx = ctx_for(DOC, &["python"], true);
        let cells: Vec<Value> = owned_python_ids(&ctx)
            .iter()
            .map(|id| cell(Some(id), "o\n", None))
            .collect();
        let result = run_marimo(&ctx, Some(notebook(cells)), Some(marimo_info()));
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        let record = result.record.expect("a record");
        assert_eq!(record.engine.name, "marimo");
        assert_eq!(
            record.engine.hash_definition,
            marimo_info().hash_definition.unwrap()
        );
    }

    #[test]
    fn marimo_count_mismatch_skips_the_record_with_a_diagnostic() {
        // Default coverage (`owned`): under `subset` a dropped cell is invisible to core, so
        // this is the case the join can catch.
        let ctx = ctx_for(DOC, &["python"], true);
        let ids = owned_python_ids(&ctx);
        let expected = ids.len();
        assert!(expected > 1);
        let cells = vec![cell(Some(&ids[0]), "o\n", None)];
        let result = run_marimo(&ctx, Some(notebook(cells)), Some(marimo_info()));
        assert!(result.record.is_none(), "the record is dropped");
        // A warning, not an error: the render carries on with the engine's markdown.
        assert_eq!(result.warnings.len(), 1);
        let d = &result.warnings[0];
        assert_eq!(d.kind, quarto_error_reporting::DiagnosticKind::Warning);
        let text = d.to_text(None);
        assert!(text.contains("marimo"), "{text}");
        assert!(text.contains(&format!("{expected} units")), "{text}");
        assert!(text.contains("returned 1 annotated"), "{text}");
    }

    #[test]
    fn unknown_and_duplicate_ids_drop_the_record_with_a_diagnostic() {
        let ctx = ctx_for(DOC, &["python"], true);
        let ids = owned_python_ids(&ctx);
        let mut cells: Vec<Value> = ids.iter().map(|id| cell(Some(id), "o\n", None)).collect();
        cells[1] = cell(Some("99-deadbeef-unlabeled"), "o\n", None);
        let result = run(&ctx, Some(notebook(cells)), None);
        assert!(result.record.is_none());
        assert!(
            result.warnings[0]
                .to_text(None)
                .contains("99-deadbeef-unlabeled")
        );

        let mut cells: Vec<Value> = ids.iter().map(|id| cell(Some(id), "o\n", None)).collect();
        cells[1] = cell(Some(&ids[0]), "o\n", None);
        let result = run(&ctx, Some(notebook(cells)), None);
        assert!(result.record.is_none());
        assert!(result.warnings[0].to_text(None).contains("more than once"));
    }

    fn subset_info() -> TsRecordInfo {
        let mut info = default_record_info();
        info.coverage = TsCoverage::Subset;
        info
    }

    #[test]
    fn subset_coverage_accepts_fewer_cells_than_the_language_level_owned_count() {
        // The engine's own rule (marimo's class rule) runs only some of the units core's
        // language view marks owned; the cells it names are the ones it ran.
        let ctx = ctx_for(DOC, &["python"], true);
        let ids = owned_python_ids(&ctx);
        assert!(ids.len() > 1);
        let cells = vec![cell(Some(&ids[1]), "o\n", None)];
        let result = run(&ctx, Some(notebook(cells)), Some(subset_info()));
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        let record = result.record.expect("a record");
        assert_eq!(record.cells.len(), 1);
        assert_eq!(record.cells[0].metadata.quarto_cell_id, ids[1]);

        // The same notebook without the declaration keeps the strict count.
        let cells = vec![cell(Some(&ids[1]), "o\n", None)];
        let strict = run(&ctx, Some(notebook(cells)), None);
        assert!(strict.record.is_none());
    }

    #[test]
    fn subset_coverage_still_rejects_unknown_ceded_and_duplicate_ids() {
        let ctx = ctx_for(DOC, &["python"], true);
        let ids = owned_python_ids(&ctx);
        let ceded = ctx
            .units
            .iter()
            .find(|u| matches!(u, Unit::Cell(_)) && u.lang() == "r")
            .map(|u| unit_id(u).unwrap().to_string())
            .unwrap();

        for (bad, needle) in [
            ("99-deadbeef-unlabeled".to_string(), "99-deadbeef-unlabeled"),
            (ceded.clone(), ceded.as_str()),
        ] {
            let cells = vec![
                cell(Some(&ids[0]), "o\n", None),
                cell(Some(&bad), "o\n", None),
            ];
            let result = run(&ctx, Some(notebook(cells)), Some(subset_info()));
            assert!(result.record.is_none(), "{bad}");
            assert!(result.warnings[0].to_text(None).contains(needle), "{bad}");
        }

        let cells = vec![
            cell(Some(&ids[0]), "o\n", None),
            cell(Some(&ids[0]), "o\n", None),
        ];
        let result = run(&ctx, Some(notebook(cells)), Some(subset_info()));
        assert!(result.record.is_none());
        assert!(result.warnings[0].to_text(None).contains("more than once"));
    }

    #[test]
    fn subset_coverage_needs_ids_alignment() {
        let ctx = ctx_for(DOC, &["python"], true);
        let mut info = subset_info();
        info.alignment = TsAlignment::Positional;
        let result = run(
            &ctx,
            Some(notebook(vec![cell(None, "o\n", None)])),
            Some(info),
        );
        assert!(result.record.is_none());
        assert!(
            result.warnings[0].to_text(None).contains("ids"),
            "{:?}",
            result.warnings[0].to_text(None)
        );
    }

    #[test]
    fn malformed_notebook_is_a_loud_diagnostic_not_a_dropped_one() {
        let ctx = ctx_for(DOC, &["python"], true);
        for bad in [
            json!("not an object"),
            json!({"nbformat": 3, "cells": []}),
            json!({"nbformat": 4}),
            json!({"nbformat": 4, "cells": [{"cell_type": "code", "outputs": "x"}]}),
            json!({"nbformat": 4, "cells": [{"cell_type": "code", "execution_count": "one"}]}),
        ] {
            let result = run(&ctx, Some(bad.clone()), None);
            assert!(result.record.is_none(), "{bad}");
            assert_eq!(result.warnings.len(), 1, "{bad}");
            assert!(
                result.warnings[0].to_text(None).contains("not readable"),
                "{bad}"
            );
        }
    }

    #[test]
    fn no_record_wanted_or_no_notebook_is_silent() {
        let ctx = ctx_for(DOC, &["python"], false);
        let result = run(&ctx, Some(notebook(vec![])), None);
        assert!(result.record.is_none() && result.warnings.is_empty());

        let ctx = ctx_for(DOC, &["python"], true);
        let result = run(&ctx, None, None);
        assert!(result.record.is_none() && result.warnings.is_empty());
    }
}
