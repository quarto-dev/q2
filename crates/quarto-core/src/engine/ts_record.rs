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

use std::collections::{BTreeMap, HashMap};

use serde_json::{Value, json};

use crate::engine::context::{ExecuteResult, ExecutionContext};
use crate::engine::diagnostics::{EngineRecordProblem, engine_record_diagnostic};
use crate::engine::ts_presentation::presentation_for;
use crate::engine::ts_protocol::{TsAlignment, TsCoverage, TsRecordInfo, TsUnit, TsUnitKind};
use crate::engine::units::{Unit, line_at, locate_units, view_for_engine};
use crate::presenter::knitr_inputs::{EXPRESSION_STARTS_KEY, OUTPUT_ANCHORS_KEY, OUTPUT_KINDS_KEY};
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
    /// The raw 0-based position among **all** the notebook\'s cells (OPR-7f: the wrapper id).
    index: usize,
    /// For a code cell, its 1-based position among the notebook\'s code cells (OPR-7f: the
    /// figure file name); 0 for any other cell.
    code_ordinal: usize,
    unit_id: Option<String>,
    is_code: bool,
    result: UnitResult,
    /// The cell's source text (the lines joined).
    source: String,
    /// `metadata.quarto`, the unit annotation QuartoNotebookRunner adds under
    /// `emitUnits`. `None` for an engine that does not annotate.
    quarto: Option<QuartoAnnotation>,
}

/// QuartoNotebookRunner's `metadata.quarto` on one cell.
struct QuartoAnnotation {
    language: Option<String>,
    role: Option<String>,
    line: Option<u64>,
    /// `expansion`: this cell is the m-th (1-based) expansion of a cell that expanded.
    expansion: Option<u64>,
    /// `inline`: one entry per span evaluated in a markdown cell, in order.
    inline: Vec<InlineEntry>,
    /// The runner's capture facts, present only for a runner built with them (OPR-4b).
    /// Written onto the record cell under the knitr keys ([`facts_as_cell_metadata`], OPR-4d).
    facts: CaptureFacts,
}

/// The three optional keys QuartoNotebookRunner adds to `metadata.quarto` for the presenter
/// (OPR-4a spike, wire table): where each expression starts, where each output anchors, and what
/// kind each output is. All `None` when the runner does not write them.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct CaptureFacts {
    /// `expression_starts`: the 0-based line of the cell code at which each expression starts.
    expression_starts: Option<Vec<u64>>,
    /// `output_anchors`: the expression each output follows.
    output_anchors: Option<Vec<u64>>,
    /// `output_kinds`: per output, `message`, `warning`, or `null` for anything else.
    output_kinds: Option<Vec<Option<String>>>,
}

/// An optional array of non-negative integers, or an error naming the key.
fn read_u64_array(quarto: &Value, key: &str) -> Result<Option<Vec<u64>>, String> {
    match quarto.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(a)) => a
            .iter()
            .map(|v| v.as_u64())
            .collect::<Option<Vec<_>>>()
            .map(Some)
            .ok_or_else(|| format!("`metadata.quarto.{key}` holds a non-integer")),
        Some(_) => Err(format!("`metadata.quarto.{key}` is not an array")),
    }
}

fn read_capture_facts(quarto: &Value) -> Result<CaptureFacts, String> {
    let output_kinds = match quarto.get("output_kinds") {
        None | Some(Value::Null) => None,
        Some(Value::Array(a)) => Some(
            a.iter()
                .map(|v| match v {
                    Value::Null => Ok(None),
                    Value::String(s) => Ok(Some(s.clone())),
                    _ => Err("`metadata.quarto.output_kinds` holds a non-string".to_string()),
                })
                .collect::<Result<Vec<_>, String>>()?,
        ),
        Some(_) => return Err("`metadata.quarto.output_kinds` is not an array".to_string()),
    };
    Ok(CaptureFacts {
        expression_starts: read_u64_array(quarto, "expression_starts")?,
        output_anchors: read_u64_array(quarto, "output_anchors")?,
        output_kinds,
    })
}

/// The facts the runner supplied, as record cell metadata under the names knitr uses, so the one
/// per-cell step reads them with no engine test (OPR-4d, decision D4). Only the keys present
/// are written; a cell with none gets none, and its output is as before.
fn facts_as_cell_metadata(facts: &CaptureFacts) -> BTreeMap<String, Value> {
    let mut extra = BTreeMap::new();
    if let Some(starts) = &facts.expression_starts {
        extra.insert(EXPRESSION_STARTS_KEY.to_string(), json!(starts));
    }
    if let Some(anchors) = &facts.output_anchors {
        extra.insert(OUTPUT_ANCHORS_KEY.to_string(), json!(anchors));
    }
    if let Some(kinds) = &facts.output_kinds {
        extra.insert(OUTPUT_KINDS_KEY.to_string(), json!(kinds));
    }
    extra
}

struct InlineEntry {
    language: String,
    /// The span's expression with the marker removed, which pairs it with a scanner unit.
    code: String,
    result: UnitResult,
}

fn read_annotation(quarto: &Value, kernel: &Option<String>) -> Result<QuartoAnnotation, String> {
    let str_of = |key: &str| quarto.get(key).and_then(Value::as_str).map(str::to_string);
    let inline = match quarto.get("inline") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(entries)) => entries
            .iter()
            .map(|e| {
                let language = e
                    .get("language")
                    .and_then(Value::as_str)
                    .ok_or("an inline entry has no `language`")?
                    .to_string();
                let code = e
                    .get("code")
                    .and_then(Value::as_str)
                    .ok_or("an inline entry has no `code`")?
                    .to_string();
                let outputs = match e.get("outputs") {
                    None | Some(Value::Null) => Vec::new(),
                    Some(Value::Array(a)) => a.clone(),
                    Some(_) => return Err("an inline entry's `outputs` is not an array".into()),
                };
                Ok(InlineEntry {
                    language,
                    code,
                    result: UnitResult {
                        outputs,
                        execution_count: None,
                        kernel: kernel.clone(),
                    },
                })
            })
            .collect::<Result<Vec<_>, String>>()?,
        Some(_) => return Err("`metadata.quarto.inline` is not an array".to_string()),
    };
    Ok(QuartoAnnotation {
        language: str_of("language"),
        role: str_of("role"),
        line: quarto.get("line").and_then(Value::as_u64),
        expansion: quarto.get("expansion").and_then(Value::as_u64),
        inline,
        facts: read_capture_facts(quarto)?,
    })
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
    let mut code_cells = 0usize;
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
            let kernel = meta_str("quarto_kernel").or_else(|| notebook_kernel.map(str::to_string));
            let quarto = match metadata.and_then(|m| m.get("quarto")) {
                Some(q) if q.is_object() => {
                    Some(read_annotation(q, &kernel).map_err(|d| format!("cell {i}: {d}"))?)
                }
                _ => None,
            };
            let is_code = cell.get("cell_type").and_then(Value::as_str) == Some("code");
            if is_code {
                code_cells += 1;
            }
            Ok(NotebookCell {
                index: i,
                code_ordinal: if is_code { code_cells } else { 0 },
                quarto,
                unit_id: meta_str("quarto_cell_id"),
                is_code,
                source: match cell.get("source") {
                    Some(Value::String(s)) => s.clone(),
                    Some(Value::Array(lines)) => {
                        lines.iter().filter_map(Value::as_str).collect::<String>()
                    }
                    _ => String::new(),
                },
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

/// What pairs a scanner unit with a QNR result: its kind and its trimmed code (a cell's
/// text between the fences, `#|` lines included, which is what QNR reports as the cell's
/// source; a span's expression).
#[derive(PartialEq, Eq)]
struct UnitKey {
    is_cell: bool,
    code: String,
}

/// A scanner unit this engine owns, as the QNR-annotated alignment sees it.
struct Slot {
    /// The stage's index for the unit; `None` for one an earlier engine introduced.
    index: Option<usize>,
    key: UnitKey,
}

/// One QNR result for a unit it ran.
struct QnrResult {
    key: UnitKey,
    result: UnitResult,
    /// The cell's capture facts; `None` for a span and for an expanded cell, whose outputs are
    /// the expansions' and so do not line up with one cell's anchors.
    facts: Option<CaptureFacts>,
    /// The cell\'s `quarto_attrs` for the wrapper (OPR-7f); `None` for a span and an expanded cell.
    attrs: Option<Value>,
}

/// Attach the `m`-th expansion of a unit to its result, as extra outputs. A marker
/// output (an empty `display_data`, so an expansion with no output of its own is still
/// there) carries the expansion's `index` and its `source`, the cell text with its own
/// `#|` option lines; the expansion's outputs follow, each tagged with the same `index`.
/// The tag is a key on the output, which a record round-trips with the rest of it.
fn attach_expansion(unit: &mut UnitResult, m: u64, source: &str, outputs: &[Value]) {
    unit.outputs.push(serde_json::json!({
        "output_type": "display_data",
        "data": {},
        "metadata": {},
        "quarto_expansion": {"index": m, "source": source},
    }));
    for output in outputs {
        let mut output = output.clone();
        if let Some(obj) = output.as_object_mut() {
            obj.insert("quarto_expansion".into(), serde_json::json!({"index": m}));
        }
        unit.outputs.push(output);
    }
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
    // The capture facts of the cells that carried them, by stage unit index.
    let mut facts: BTreeMap<usize, CaptureFacts> = BTreeMap::new();
    // The wrapper `quarto_attrs` of the annotated cells, by stage unit index (OPR-7f).
    let mut attrs: BTreeMap<usize, Value> = BTreeMap::new();
    let stem = ctx
        .source_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("document")
        .to_string();
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
                if let Some(q) = &cell.quarto {
                    facts.insert(index, q.facts.clone());
                }
            }
        }
        TsAlignment::Positional if info.coverage == TsCoverage::Subset => {
            return Err(EngineRecordProblem::Malformed {
                detail: "`coverage: subset` needs `alignment: ids`".to_string(),
            });
        }
        TsAlignment::Positional if cells.iter().any(|c| c.quarto.is_some()) => {
            // QuartoNotebookRunner's annotated notebook (EXR-8 B5b). Only the units this
            // engine owns count, and spans in the front matter are never evaluated by QNR.
            // The scanner and QNR do not always agree on which units exist (a nested cell
            // or an indented span QNR does not run; a double-backtick span the scanner
            // never counted), so the two lists are paired on content, in document order,
            // not by count. A unit with no QNR result is not pushed, so the record has no
            // cell for it and the presenter leaves its source as written; a QNR result with
            // no unit is dropped.
            //
            // Not on `line`: measured on a real render, QNR's `line` for a code cell is the
            // line in the original source file (through the source ranges), while a span's
            // is the mapped start of its markdown chunk plus the newlines inside the chunk,
            // which the generated front matter throws off (141 where the file has 8 and the
            // engine's text 140). No shared coordinate system, so `line` is not used.
            let slots: Vec<Slot> = view_for_engine(&located, &|lang| owns(ctx, lang))
                .into_iter()
                .filter(|eu| eu.owned && !matches!(eu.unit, Unit::Inline(i) if i.in_front_matter))
                .map(|eu| Slot {
                    index: unit_id(eu.unit).and_then(|id| by_id.get(id).copied()),
                    key: match eu.unit {
                        Unit::Cell(c) => UnitKey {
                            is_cell: true,
                            code: c.code.trim().to_string(),
                        },
                        Unit::Inline(i) => UnitKey {
                            is_cell: false,
                            code: i.code.trim().to_string(),
                        },
                    },
                })
                .collect();
            let mut results: Vec<QnrResult> = Vec::new();
            // Whether the unit last pushed expanded, so `expansion` cells follow it.
            let mut expanding = false;
            for cell in &cells {
                let Some(q) = &cell.quarto else { continue };
                let owned_here = |lang: &str| owns(ctx, lang);
                if cell.is_code && q.language.as_deref().is_some_and(owned_here) {
                    match q.expansion {
                        // One of the expansions of the unit just seen.
                        Some(m) if expanding => {
                            let unit = results.last_mut().expect("`expanding` follows a push");
                            let shown = presentation_for(
                                cell.index,
                                cell.code_ordinal,
                                &cell.source,
                                cell.result.execution_count,
                                &cell.result.outputs,
                                &stem,
                            );
                            attach_expansion(&mut unit.result, m, &cell.source, &shown.outputs);
                        }
                        Some(m) => {
                            return Err(EngineRecordProblem::Malformed {
                                detail: format!(
                                    "expansion {m} (line {}) has no expansion source cell before it",
                                    q.line.map_or_else(|| "?".to_string(), |l| l.to_string())
                                ),
                            });
                        }
                        None => {
                            expanding = q.role.as_deref() == Some("expansion_source");
                            let shown = presentation_for(
                                cell.index,
                                cell.code_ordinal,
                                &cell.source,
                                cell.result.execution_count,
                                &cell.result.outputs,
                                &stem,
                            );
                            let mut result = cell.result.clone();
                            result.outputs = shown.outputs;
                            results.push(QnrResult {
                                key: UnitKey {
                                    is_cell: true,
                                    code: cell.source.trim().to_string(),
                                },
                                result,
                                facts: (!expanding).then(|| q.facts.clone()),
                                attrs: (!expanding).then_some(shown.cell_attrs),
                            });
                        }
                    }
                }
                results.extend(
                    q.inline
                        .iter()
                        .filter(|e| owned_here(&e.language))
                        .map(|e| QnrResult {
                            key: UnitKey {
                                is_cell: false,
                                code: e.code.trim().to_string(),
                            },
                            result: e.result.clone(),
                            facts: None,
                            attrs: None,
                        }),
                );
            }
            // Both lists are in document order, so each result looks for its unit from the
            // one after the last pair. Known limit: a unit QNR did not run that has the
            // same code as a later one takes that one's result, and the later one then
            // shows its source (identical units are told apart only by their order).
            let mut next = 0;
            for r in results {
                if let Some(offset) = slots[next..].iter().position(|s| s.key == r.key) {
                    if let Some(index) = slots[next + offset].index {
                        pairs.push((index, r.result));
                        if let Some(f) = r.facts {
                            facts.insert(index, f);
                        }
                        if let Some(a) = r.attrs {
                            attrs.insert(index, a);
                        }
                    }
                    next += offset + 1;
                }
            }
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
                    if let Some(q) = &cell.quarto {
                        facts.insert(*index, q.facts.clone());
                    }
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
    let mut record = builder
        .finish(EngineEntryInput {
            can_freeze,
            owned_languages: ctx.owned_languages.clone(),
            document_metadata: ctx.document_metadata_json(),
            ..Default::default()
        })
        .map_err(build)?;
    // The label is set only with the presentation acknowledgement (OPR-7i, decision 8): an
    // engine that presented itself stored its own `toMarkdown` includes in the entry, so "label
    // present" must mean "core presents, so core derives". Outside the hash definition.
    if acknowledged_presentation(Some(info)) {
        record.engine.dependency_data = info.dependency_data.clone();
    }
    // The `quarto_` keys are outside the unit hash, so this changes nothing the record hashes.
    for (index, f) in &facts {
        let Some(id) = unit_id(&ctx.units[*index]) else {
            continue;
        };
        if let Some(cell) = record
            .cells
            .iter_mut()
            .find(|c| c.metadata.quarto_cell_id == id)
        {
            cell.metadata.extra.extend(facts_as_cell_metadata(f));
        }
    }
    // Display data too: `quarto_attrs` is outside the hash, like the facts.
    for (index, a) in attrs {
        let Some(id) = unit_id(&ctx.units[index]) else {
            continue;
        };
        if let Some(cell) = record
            .cells
            .iter_mut()
            .find(|c| c.metadata.quarto_cell_id == id)
        {
            cell.metadata.extra.insert("quarto_attrs".into(), a);
        }
    }
    Ok(record)
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
    if !ctx.builds_record() || ctx.units.is_empty() {
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

/// Whether core offers to present this run's output (OPR-7a, decision D8): whenever it
/// builds a record and the document has units. No engine name is tested; an engine that
/// ignores the offer keeps presenting for itself.
pub(super) fn offers_presentation(ctx: &ExecutionContext) -> bool {
    ctx.builds_record() && !ctx.units.is_empty()
}

/// Whether the engine acknowledged the offer on its returned record info. Core presents
/// from the record exactly when this is true.
pub(super) fn acknowledged_presentation(info: Option<&TsRecordInfo>) -> bool {
    info.is_some_and(|i| i.presented_by == Some(crate::engine::ts_protocol::TsPresentedBy::Core))
}

/// After [`attach_record`] on a run core asked to present (`presentedBy: "core"`): replace
/// the result's markdown with the presenter's, made from the record over `input`.
///
/// An engine that did not honor the request (an older copy) still returned its own
/// markdown; with a record, core's replaces it, so the two cannot disagree. With no record
/// and no markdown there is nothing to show, so that is an error naming the engine's own
/// diagnostic: a document that silently lost every output would be worse.
pub(super) fn present_from_record(
    engine: &str,
    input: &str,
    ctx: &ExecutionContext,
    result: &mut ExecuteResult,
) -> Result<(), crate::engine::error::ExecutionError> {
    use crate::engine::error::ExecutionError;
    use crate::presenter::document::present_document;
    use crate::presenter::figures::FsFigureSink;
    use crate::presenter::inline::InlineTarget;

    let Some(record) = &result.record else {
        if result.markdown.is_empty() {
            let why = result
                .warnings
                .iter()
                .map(|w| {
                    w.to_text_with_options(
                        None,
                        &quarto_error_reporting::TextRenderOptions::plain(),
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            return Err(ExecutionError::execution_failed(
                engine,
                format!(
                    "core presents this engine's output, but no execution record could be \
                     built from the notebook it returned, so there is nothing to present. {why}"
                ),
            ));
        }
        return Ok(());
    };
    let units = locate_units(&ctx.units, input);
    let mut fig = FsFigureSink::new(&ctx.source_path);
    let presented = present_document(
        &record.cells,
        input,
        &units,
        ctx.execute_scope.as_ref(),
        InlineTarget::from_format(&ctx.format),
        &ctx.format,
        std::slice::from_ref(&record.engine),
        &mut fig,
    )
    .map_err(|e| {
        ExecutionError::execution_failed(engine, format!("presenting the record failed: {e}"))
    })?;
    result.markdown = presented.markdown;
    result.derived_header_includes = presented.header_includes;
    result.supporting_files.extend(presented.files_written);
    result.warnings.extend(
        presented
            .notes
            .iter()
            .map(|note| crate::engine::diagnostics::note_diagnostic(note, &units, ctx)),
    );
    Ok(())
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

    /// A record built from `info` over the document's owned cells.
    fn entry_label(info: TsRecordInfo) -> Option<String> {
        let ctx = ctx_for(DOC, &["python"], true);
        let cells: Vec<Value> = owned_python_ids(&ctx)
            .iter()
            .map(|id| cell(Some(id), "o\n", None))
            .collect();
        let result = run(&ctx, Some(notebook(cells)), Some(info));
        result.record.expect("a record").engine.dependency_data
    }

    fn labeled_info(
        presented_by: Option<crate::engine::ts_protocol::TsPresentedBy>,
    ) -> TsRecordInfo {
        let mut info = default_record_info();
        info.alignment = TsAlignment::Ids;
        info.presented_by = presented_by;
        info.dependency_data = Some("jupyter-outputs".into());
        info
    }

    #[test]
    fn the_label_reaches_the_entry_only_with_the_presentation_acknowledgement() {
        use crate::engine::ts_protocol::TsPresentedBy;
        // OPR-7i decision 8: an engine that presented itself stored its own includes in the
        // entry, so a label there would make core derive a second copy.
        assert_eq!(
            entry_label(labeled_info(Some(TsPresentedBy::Core))).as_deref(),
            Some("jupyter-outputs")
        );
        assert_eq!(entry_label(labeled_info(None)), None);
        let mut unlabeled = labeled_info(Some(TsPresentedBy::Core));
        unlabeled.dependency_data = None;
        assert_eq!(entry_label(unlabeled), None);
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
            presented_by: None,
            dependency_data: None,
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
            presented_by: None,
            dependency_data: None,
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
            presented_by: None,
            dependency_data: None,
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

    // ---- QNR-annotated notebooks (EXR-8 B2) ----
    //
    // QuartoNotebookRunner's notebook under `emitUnits`: `metadata.quarto` on
    // every code cell (`unit`, `language`, `line`) and `metadata.quarto.inline`
    // on a markdown cell (one entry per evaluated span). Alignment is positional.

    const JDOC: &str = "\
---
title: \"`{julia} 0`\"
---

Inline `{julia} 1 + 1` and `{python} 2` here.

```{julia}
x = 1
```

```{python}
y = 2
```

```{julia}
#| eval: false
z = 3
```

````md
`{julia} 9`

```{julia}
fenced = 1
```
````
";

    fn positional() -> TsRecordInfo {
        TsRecordInfo {
            version: 1,
            granularity: Granularity::Unit,
            output_kind: Default::default(),
            alignment: TsAlignment::Positional,
            coverage: Default::default(),
            hash_definition: None,
            presented_by: None,
            dependency_data: None,
        }
    }

    fn out(text: &str) -> Value {
        json!({"output_type": "stream", "name": "stdout", "text": [text]})
    }

    fn qnr_code(
        unit: u64,
        lang: &str,
        line: u64,
        code: &str,
        outputs: Vec<Value>,
        count: u64,
    ) -> Value {
        json!({
            "cell_type": "code",
            "id": unit.to_string(),
            "execution_count": count,
            "metadata": {"quarto": {"unit": unit, "language": lang, "file": "doc.qmd", "line": line}},
            "outputs": outputs,
            "source": [code],
        })
    }

    fn qnr_prefix(unit: u64) -> Value {
        json!({
            "cell_type": "markdown",
            "metadata": {"quarto": {"unit": unit, "role": "code_prefix"}},
            "source": ["```python\ny = 2\n```\n"],
        })
    }

    /// A markdown cell with one inline entry per `(language, line, code, output text)`.
    fn qnr_md(inline: Vec<(&str, u64, &str, &str)>) -> Value {
        let entries: Vec<Value> = inline
            .into_iter()
            .map(|(lang, line, code, text)| {
                json!({"language": lang, "code": code, "line": line, "outputs": [out(text)]})
            })
            .collect();
        let metadata = if entries.is_empty() {
            json!({})
        } else {
            json!({"quarto": {"inline": entries}})
        };
        json!({"cell_type": "markdown", "metadata": metadata, "source": ["prose"]})
    }

    fn run_julia(ctx: &ExecutionContext, cells: Vec<Value>) -> ExecuteResult {
        let mut result = ExecuteResult::default();
        attach_record(
            "julia",
            JDOC,
            ctx,
            false,
            Some(notebook(cells)),
            Some(positional()),
            &mut result,
        );
        result
    }

    /// Build the record for an arbitrary document from QNR-shaped cells.
    fn run_doc(doc: &str, cells: Vec<Value>) -> (ExecutionContext, ExecuteResult) {
        let ctx = ctx_for(doc, &["julia"], true);
        let mut result = ExecuteResult::default();
        attach_record(
            "julia",
            doc,
            &ctx,
            false,
            Some(notebook(cells)),
            Some(positional()),
            &mut result,
        );
        (ctx, result)
    }

    /// The notebook QNR returns for `JDOC` when `julia` is the only owned language
    /// and `python` is run by nobody (so QNR treats it as its own, unowned).
    fn julia_nb() -> Vec<Value> {
        vec![
            qnr_md(vec![
                ("julia", 5, "1 + 1", "inline\n"),
                ("python", 5, "2", "ignored\n"),
            ]),
            qnr_code(1, "julia", 7, "x = 1", vec![out("x\n")], 1),
            qnr_prefix(2),
            qnr_code(2, "python", 11, "y = 2", vec![out("y\n")], 1),
            qnr_code(3, "julia", 15, "#| eval: false\nz = 3", vec![], 0),
        ]
    }

    fn texts(record: &crate::record::EngineRecord) -> Vec<String> {
        record
            .cells
            .iter()
            .map(|c| {
                c.outputs
                    .first()
                    .and_then(|o| o["text"][0].as_str())
                    .unwrap_or("")
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn julia_record_unit_count_matches_scanner() {
        let ctx = ctx_for(JDOC, &["julia"], true);
        let owned_outside_front_matter = wire_units(JDOC, &ctx)
            .unwrap()
            .iter()
            .filter(|u| u.owned && !u.in_front_matter)
            .count();
        let result = run_julia(&ctx, julia_nb());
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        let record = result.record.expect("a record");
        assert_eq!(record.cells.len(), owned_outside_front_matter);
        assert_eq!(record.cells.len(), 3);
    }

    #[test]
    fn julia_record_aligns_by_unit_index() {
        let ctx = ctx_for(JDOC, &["julia"], true);
        let record = run_julia(&ctx, julia_nb()).record.expect("a record");
        // Document order over the owned units: the inline span, then the two cells.
        assert_eq!(texts(&record), vec!["inline\n", "x\n", ""]);
        let owned_ids: Vec<String> = wire_units(JDOC, &ctx)
            .unwrap()
            .into_iter()
            .filter(|u| u.owned && !u.in_front_matter)
            .map(|u| u.id.unwrap())
            .collect();
        let ids: Vec<_> = record
            .cells
            .iter()
            .map(|c| c.metadata.quarto_cell_id.clone())
            .collect();
        assert_eq!(ids, owned_ids);
        assert_eq!(record.cells[1].execution_count, Some(1));
        assert_eq!(
            record.cells[1].metadata.quarto_kernel.as_deref(),
            Some("nb-kernel")
        );
    }

    #[test]
    fn julia_record_ignores_unowned_span_qnr_evaluated() {
        // An older QNR still evaluated the `python` span; its entry is filtered out
        // by language before alignment, so there is no count mismatch.
        let ctx = ctx_for(JDOC, &["julia"], true);
        let result = run_julia(&ctx, julia_nb());
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        assert!(!texts(&result.record.unwrap()).contains(&"ignored\n".to_string()));
    }

    #[test]
    fn julia_python_cell_owned_by_nobody_yields_no_record_unit() {
        let ctx = ctx_for(JDOC, &["julia"], true);
        let record = run_julia(&ctx, julia_nb()).record.expect("a record");
        assert!(!texts(&record).contains(&"y\n".to_string()));
        // The units after the `python` cell still align.
        assert_eq!(record.cells.len(), 3);
        assert_eq!(texts(&record)[2], "");
    }

    #[test]
    fn front_matter_span_is_excluded_from_julia_alignment() {
        let ctx = ctx_for(JDOC, &["julia"], true);
        let wire = wire_units(JDOC, &ctx).unwrap();
        assert!(wire.iter().any(|u| u.owned && u.in_front_matter));
        // QNR (FrontMatterRule) did not evaluate the title span: no entry for it.
        let result = run_julia(&ctx, julia_nb());
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        assert_eq!(result.record.unwrap().cells.len(), 3);
    }

    /// OPR-7f: the mapping stamps what the plain route would (wrapper id from the raw notebook
    /// index, figure name from the code-cell ordinal, counts, the figure class), on the record
    /// cell and its outputs, with the unit hash untouched.
    #[test]
    fn julia_record_carries_the_plain_routes_ids_names_and_counts() {
        let doc = "Prose.\n\n```{julia}\nplot()\n```\n";
        let plot = json!({"output_type": "display_data",
            "data": {"image/png": "AAAA", "text/plain": "<Plot>"}, "metadata": {}});
        let cells = vec![
            qnr_md(vec![]),
            qnr_code(1, "julia", 3, "plot()", vec![out("hi\n"), plot], 4),
        ];
        let (_ctx, result) = run_doc(doc, cells);
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        let record = result.record.expect("a record");
        let cell = &record.cells[0];
        assert_eq!(cell.metadata.extra["quarto_attrs"]["id"], "cell-1");
        assert_eq!(
            cell.metadata.extra["quarto_attrs"]["attributes"]["execution_count"],
            "4"
        );
        // One markdown cell, then the code cell: raw index 1, but its figure is cell-2 and,
        // after the printed line, output 2.
        assert_eq!(
            cell.outputs[1]["metadata"]["quarto_file_path"],
            "doc_files/figure-html/cell-2-output-2.png"
        );
        assert_eq!(
            cell.outputs[1]["metadata"]["quarto_attrs"]["classes"],
            json!(["cell-output"])
        );
        // The stream output is untouched.
        assert_eq!(cell.outputs[0], out("hi\n"));
    }

    #[test]
    fn julia_eval_false_cell_is_a_unit_with_no_outputs() {
        let ctx = ctx_for(JDOC, &["julia"], true);
        let record = run_julia(&ctx, julia_nb()).record.expect("a record");
        assert!(record.cells[2].outputs.is_empty());
    }

    #[test]
    fn spans_inside_fences_are_not_units() {
        // The `{julia}` span and cell inside the four-backtick `md` fence: the
        // scanner never claims them, and QNR does not run them.
        let ctx = ctx_for(JDOC, &["julia"], true);
        let owned = wire_units(JDOC, &ctx)
            .unwrap()
            .iter()
            .filter(|u| u.owned && !u.in_front_matter)
            .count();
        assert_eq!(owned, 3);
        assert!(run_julia(&ctx, julia_nb()).record.is_some());
    }

    #[test]
    fn julia_record_ignores_prefix_cells() {
        let ctx = ctx_for(JDOC, &["julia"], true);
        let mut cells = julia_nb();
        cells.insert(1, qnr_prefix(1));
        cells.insert(2, qnr_prefix(1));
        let result = run_julia(&ctx, cells);
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        assert_eq!(result.record.unwrap().cells.len(), 3);
    }

    /// B5b: a unit QNR returned no result for is not executed: no record cell, no
    /// diagnostic. (Before B5b this was a count mismatch and no record.)
    #[test]
    fn a_unit_with_no_qnr_result_is_left_out_of_the_record() {
        let ctx = ctx_for(JDOC, &["julia"], true);
        let mut cells = julia_nb();
        cells.pop();
        let result = run_julia(&ctx, cells);
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        let record = result.record.expect("a record");
        assert_eq!(record.cells.len(), 2);
        assert_eq!(texts(&record), vec!["inline\n", "x\n"]);
    }

    /// B5b: a `{julia}` cell the scanner counts but QNR does not run (it sits in a
    /// blockquote; measured: QNR leaves it as text in a markdown cell, with no annotation).
    /// Before B5b this was a count mismatch and no record.
    #[test]
    fn a_nested_julia_cell_qnr_does_not_run_is_left_out_of_the_record() {
        let doc = "```{julia}\na = 1\n```\n\n> ```{julia}\n> b = 2\n> ```\n";
        let (ctx, result) = run_doc(
            doc,
            vec![
                qnr_code(1, "julia", 1, "a = 1", vec![out("a\n")], 1),
                json!({"cell_type": "markdown", "metadata": {}, "source": ["\n> ```{julia}\n> b = 2\n> ```\n"]}),
            ],
        );
        let owned = wire_units(doc, &ctx)
            .unwrap()
            .iter()
            .filter(|u| u.owned)
            .count();
        assert_eq!(owned, 2, "the scanner counts the nested cell");
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        let record = result.record.expect("a record");
        assert_eq!(texts(&record), vec!["a\n"]);
        assert_eq!(record.cells[0].source, vec!["a = 1".to_string()]);
    }

    /// The unmatched unit comes first, so a pairing by order would give the executed
    /// cell's output to the nested one.
    #[test]
    fn pairing_is_by_content_not_by_position() {
        let doc = "> ```{julia}\n> b = 2\n> ```\n\n```{julia}\na = 1\n```\n";
        let (_, result) = run_doc(
            doc,
            vec![qnr_code(1, "julia", 5, "a = 1", vec![out("a\n")], 1)],
        );
        let record = result.record.expect("a record");
        assert_eq!(record.cells.len(), 1);
        assert_eq!(record.cells[0].source, vec!["a = 1".to_string()]);
        assert_eq!(texts(&record), vec!["a\n"]);
    }

    /// Known limit: units with the same code are told apart only by their order.
    #[test]
    fn identical_spans_on_one_line_pair_in_order() {
        let doc = "A `{julia} 1` and `{julia} 1`.\n";
        let (_, result) = run_doc(
            doc,
            vec![qnr_md(vec![
                ("julia", 1, "1", "first\n"),
                ("julia", 1, "1", "second\n"),
            ])],
        );
        assert_eq!(
            texts(&result.record.expect("a record")),
            vec!["first\n", "second\n"]
        );
    }

    /// Known limit, pinned so a change is deliberate: a unit QNR did not run that has the
    /// same code as a later run one takes that one's result, and the later one keeps its
    /// source. QNR's `line` could break the tie but is not in a shared coordinate system.
    #[test]
    fn an_unrun_twin_before_a_run_unit_takes_its_result() {
        // The tab span is not run; the space span after it has the same code.
        let doc = "Tab `{julia}\t1 + 1`, then `{julia} 1 + 1`.\n";
        let (ctx, result) = run_doc(doc, vec![qnr_md(vec![("julia", 1, "1 + 1", "ran\n")])]);
        let record = result.record.expect("a record");
        let first_id = wire_units(doc, &ctx).unwrap()[0].id.clone().unwrap();
        assert_eq!(record.cells[0].metadata.quarto_cell_id, first_id);
    }

    /// A span QNR ran and the scanner did not count (a double-backtick span) has no unit,
    /// so its result is dropped and the neighbors still pair.
    #[test]
    fn a_qnr_result_with_no_scanner_unit_is_dropped() {
        let doc = "Double ``{julia} 1 + 1`` and `{julia} 3 + 3`.\n";
        let (_, result) = run_doc(
            doc,
            vec![qnr_md(vec![
                ("julia", 1, "1 + 1", "dropped\n"),
                ("julia", 1, "3 + 3", "kept\n"),
            ])],
        );
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        assert_eq!(texts(&result.record.expect("a record")), vec!["kept\n"]);
    }

    // The B2b corpus (`tests/fixtures/span-conformance/`): the four documents where the
    // scanner and QNR disagree, with QNR's real evaluated spans from `qnr-spans.json`.

    fn corpus_doc(name: &str) -> (String, Vec<(u64, String)>) {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/span-conformance");
        let doc = std::fs::read_to_string(dir.join(name)).unwrap();
        let spans: Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("qnr-spans.json")).unwrap())
                .unwrap();
        let spans = spans[name]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| {
                (
                    s["line"].as_u64().unwrap(),
                    s["code"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        (doc, spans)
    }

    /// The record for a corpus document: QNR's evaluated spans as one markdown cell, each
    /// span's output its own code.
    fn corpus_record(name: &str, extra: &str) -> (String, crate::record::EngineRecord) {
        let (mut doc, mut spans) = corpus_doc(name);
        if !extra.is_empty() {
            // A span after the disagreement, so the document has a unit QNR ran.
            let line = doc.lines().count() as u64 + 2;
            doc.push_str(&format!("\n{extra}\n"));
            let code = extra
                .split("`{julia} ")
                .nth(1)
                .unwrap()
                .trim_end_matches(['`', '.']);
            spans.push((line, code.to_string()));
        }
        let entries: Vec<(&str, u64, &str, String)> = spans
            .iter()
            .map(|(l, c)| ("julia", *l, c.as_str(), format!("{c}\n")))
            .collect();
        let entries: Vec<(&str, u64, &str, &str)> = entries
            .iter()
            .map(|(a, b, c, d)| (*a, *b, *c, d.as_str()))
            .collect();
        let (_, result) = run_doc(&doc, vec![qnr_md(entries)]);
        assert!(result.warnings.is_empty(), "{name}: {:?}", result.warnings);
        (
            doc,
            result.record.unwrap_or_else(|| panic!("{name}: no record")),
        )
    }

    #[test]
    fn corpus_tab_separator_records_the_span_qnr_ran() {
        let (_, record) = corpus_record("tab-separator.qmd", "");
        // `{julia}<tab>1 + 1` was not run; `2 + 2` was.
        assert_eq!(record.cells.len(), 1);
        assert_eq!(record.cells[0].source, vec!["2 + 2".to_string()]);
        assert_eq!(texts(&record), vec!["2 + 2\n"]);
    }

    #[test]
    fn corpus_escaped_backtick_records_the_span_qnr_ran() {
        let (_, record) = corpus_record("escaped-backtick.qmd", "");
        assert_eq!(record.cells.len(), 1);
        assert_eq!(record.cells[0].source, vec!["2 + 2".to_string()]);
    }

    #[test]
    fn corpus_indented_code_records_the_span_qnr_ran() {
        let (_, record) = corpus_record("indented-code.qmd", "");
        assert_eq!(record.cells.len(), 1);
        assert_eq!(record.cells[0].source, vec!["2 + 2".to_string()]);
    }

    #[test]
    fn corpus_double_backtick_drops_the_span_the_scanner_never_counted() {
        // On its own the document has no units, so there is no record at all; add one
        // ordinary span so core has something to present.
        let (_, record) = corpus_record("double-backtick.qmd", "Tail `{julia} 3 + 3`.");
        assert_eq!(record.cells.len(), 1);
        assert_eq!(record.cells[0].source, vec!["3 + 3".to_string()]);
        assert_eq!(texts(&record), vec!["3 + 3\n"]);
    }

    /// QNR's expansion shape for the unit at `line`: a mock `expansion_source` cell, then
    /// one cell per expansion with `expansion: m` and its own source (`#|` lines included).
    fn qnr_expansion(
        unit: u64,
        line: u64,
        code: &str,
        parts: Vec<(&str, Vec<Value>)>,
    ) -> Vec<Value> {
        let mut cells = vec![json!({
            "cell_type": "code",
            "id": unit.to_string(),
            "execution_count": 1,
            "metadata": {"quarto": {"unit": unit, "language": "julia", "line": line,
                                    "role": "expansion_source"}},
            "outputs": [],
            "source": [code],
        })];
        for (m, (source, outputs)) in parts.into_iter().enumerate() {
            let m = m as u64 + 1;
            cells.push(json!({
                "cell_type": "code",
                "id": format!("{unit}_{m}"),
                "execution_count": 1,
                "metadata": {"quarto": {"unit": unit, "language": "julia", "line": line,
                                        "expansion": m}},
                "outputs": outputs,
                "source": [source],
            }));
        }
        cells
    }

    #[test]
    fn expansion_outputs_attach_to_the_owning_unit_with_their_own_source() {
        let ctx = ctx_for(JDOC, &["julia"], true);
        let mut cells = julia_nb();
        let expansion = qnr_expansion(
            1,
            7,
            "x = 1",
            vec![
                ("#| echo: false\nfirst()", vec![out("a\n")]),
                ("second()", vec![out("b\n"), out("c\n")]),
            ],
        );
        cells.splice(1..2, expansion);
        let record = run_julia(&ctx, cells).record.expect("a record");
        // The unit keeps one record cell; the expansions are its extra outputs.
        assert_eq!(
            record.cells.len(),
            texts(&run_julia(&ctx, julia_nb()).record.unwrap()).len()
        );
        let outputs = &record.cells[1].outputs;
        let tags: Vec<_> = outputs
            .iter()
            .map(|o| o["quarto_expansion"]["index"].as_u64())
            .collect();
        // marker 1, its output, marker 2, its two outputs.
        assert_eq!(
            tags,
            vec![Some(1), Some(1), Some(2), Some(2), Some(2)],
            "{outputs:?}"
        );
        assert_eq!(outputs[0]["output_type"], "display_data");
        assert_eq!(
            outputs[0]["quarto_expansion"]["source"],
            "#| echo: false\nfirst()"
        );
        assert_eq!(outputs[1]["text"][0], "a\n");
        assert_eq!(outputs[2]["quarto_expansion"]["source"], "second()");
        // The units after the expansion still line up.
        assert_eq!(record.cells.len(), 3);
        assert_eq!(record.cells[2].outputs.len(), 0);
    }

    #[test]
    fn an_expansion_with_no_outputs_still_gets_its_marker() {
        let ctx = ctx_for(JDOC, &["julia"], true);
        let mut cells = julia_nb();
        cells.splice(
            1..2,
            qnr_expansion(1, 7, "x = 1", vec![("quiet()", vec![])]),
        );
        let record = run_julia(&ctx, cells).record.expect("a record");
        let outputs = &record.cells[1].outputs;
        assert_eq!(outputs.len(), 1, "{outputs:?}");
        assert_eq!(outputs[0]["quarto_expansion"]["source"], "quiet()");
    }

    #[test]
    fn a_unit_that_does_not_expand_has_untagged_outputs() {
        let ctx = ctx_for(JDOC, &["julia"], true);
        let record = run_julia(&ctx, julia_nb()).record.unwrap();
        assert!(record.cells[1].outputs[0].get("quarto_expansion").is_none());
    }

    #[test]
    fn an_expansion_cell_with_no_source_cell_before_it_yields_no_record() {
        let ctx = ctx_for(JDOC, &["julia"], true);
        let mut cells = julia_nb();
        let mut expansion = qnr_expansion(1, 7, "x = 1", vec![("orphan()", vec![out("a\n")])]);
        expansion.remove(0);
        cells.splice(1..2, expansion);
        let result = run_julia(&ctx, cells);
        assert!(result.record.is_none());
        assert_eq!(result.warnings.len(), 1);
    }

    // ---- core presentation (the flip) ----

    fn presented(ctx: &ExecutionContext, cells: Vec<Value>) -> Result<ExecuteResult, String> {
        let mut result = run_julia(ctx, cells);
        present_from_record("julia", JDOC, ctx, &mut result)
            .map(|()| result)
            .map_err(|e| e.to_string())
    }

    #[test]
    fn core_offers_whenever_it_builds_a_record_for_a_document_with_units() {
        assert!(offers_presentation(&ctx_for(JDOC, &["julia"], true)));
        assert!(!offers_presentation(&ctx_for(JDOC, &["julia"], false)));
        assert!(offers_presentation(
            &ctx_for(JDOC, &["julia"], false).with_present_from_record(true)
        ));
        let no_units = ExecutionContext::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/proj"),
            PathBuf::from("/proj/doc.qmd"),
            "html",
        )
        .with_want_record(true);
        assert!(!offers_presentation(&no_units));
    }

    #[test]
    fn only_an_acknowledgement_on_the_record_info_means_present() {
        let mut info = positional();
        assert!(!acknowledged_presentation(None));
        assert!(!acknowledged_presentation(Some(&info)));
        info.presented_by = Some(crate::engine::ts_protocol::TsPresentedBy::Core);
        assert!(acknowledged_presentation(Some(&info)));
    }

    #[test]
    fn core_presents_the_record_as_the_markdown() {
        let ctx = ctx_for(JDOC, &["julia"], true);
        let mut cells = julia_nb();
        // A span's result is a value (an `execute_result`), not a stream.
        let value = json!({"output_type": "execute_result", "execution_count": 1,
                           "data": {"text/plain": "VALUE"}, "metadata": {}});
        cells[0]["metadata"]["quarto"]["inline"][0]["outputs"] = json!([value]);
        let result = presented(&ctx, cells).unwrap();
        let md = &result.markdown;
        // The julia cell is a cell div with its output; the inline span is its value.
        // OPR-7f: with the plain route\'s wrapper id (raw notebook index 1) and count.
        assert!(
            md.contains("::: {#cell-1 .cell execution_count='1'}"),
            "{md}"
        );
        assert!(md.contains("cell-output-stdout"), "{md}");
        assert!(md.contains("Inline VALUE and"), "{md}");
        // The python cell and span belong to nobody here and stay as they were written.
        assert!(md.contains("```{python}"), "{md}");
        // Both julia cells were converted; the one left is inside the `md` fence.
        assert_eq!(md.matches("```{julia}").count(), 1, "{md}");
        assert_eq!(md.matches("{.julia .cell-code}").count(), 2, "{md}");
    }

    #[test]
    fn core_presents_an_expanded_unit_as_one_cell_per_expansion() {
        let ctx = ctx_for(JDOC, &["julia"], true);
        let mut cells = julia_nb();
        cells.splice(
            1..2,
            qnr_expansion(
                1,
                7,
                "x = 1",
                vec![("a()", vec![out("A\n")]), ("b()", vec![out("B\n")])],
            ),
        );
        let md = presented(&ctx, cells).unwrap().markdown;
        assert!(md.matches("::: {.cell}").count() >= 3, "{md}");
        assert!(md.contains("a()") && md.contains("b()"), "{md}");
    }

    /// A malformed notebook stays a failed render with the diagnostic in the message (no
    /// engine fallback, decided 2026-11-01): here an inline entry with no `code`.
    #[test]
    fn a_malformed_notebook_fails_the_render_naming_the_problem() {
        let ctx = ctx_for(JDOC, &["julia"], true);
        let mut cells = julia_nb();
        cells[0]["metadata"]["quarto"]["inline"][0]
            .as_object_mut()
            .unwrap()
            .remove("code");
        let err = presented(&ctx, cells).unwrap_err();
        assert!(err.contains("no execution record"), "{err}");
        assert!(err.contains("`code`"), "{err}");
    }

    /// Measured on a real render (B5b): core's presenter leaves a unit with no record
    /// cell exactly as written, and presents the unit beside it.
    #[test]
    fn core_leaves_an_unexecuted_unit_as_its_source() {
        let doc = "> ```{julia}\n> b = 2\n> ```\n\n```{julia}\na = 1\n```\n";
        let ctx = ctx_for(doc, &["julia"], true);
        let mut result = ExecuteResult::default();
        attach_record(
            "julia",
            doc,
            &ctx,
            false,
            Some(notebook(vec![qnr_code(
                1,
                "julia",
                5,
                "a = 1",
                vec![out("a\n")],
                1,
            )])),
            Some(positional()),
            &mut result,
        );
        present_from_record("julia", doc, &ctx, &mut result).unwrap();
        let md = &result.markdown;
        assert!(md.starts_with("> ```{julia}\n> b = 2\n> ```\n"), "{md}");
        assert!(md.contains("cell-output-stdout"), "{md}");
    }

    #[test]
    fn with_no_record_the_engines_own_markdown_is_kept() {
        let ctx = ctx_for(JDOC, &["julia"], true);
        let mut result = ExecuteResult::new("engine markdown");
        present_from_record("julia", JDOC, &ctx, &mut result).unwrap();
        assert_eq!(result.markdown, "engine markdown");
    }

    /// A notebook with one code cell whose `metadata.quarto` is `quarto`.
    fn notebook_with_annotation(quarto: serde_json::Value) -> Value {
        json!({
            "nbformat": 4,
            "nbformat_minor": 5,
            "metadata": {"kernelspec": {"name": "julia-1.12"}},
            "cells": [{
                "cell_type": "code",
                "execution_count": null,
                "metadata": {"quarto": quarto},
                "source": ["x = 1 + 1\n", "x\n", "y = x * 3"],
                "outputs": []
            }]
        })
    }

    /// The runner's capture facts (OPR-4a, 4b) reach q2 in `metadata.quarto`, in the shapes the
    /// runner wrote for a three-expression cell (a real run, 2026-11-09): the start line of each
    /// expression, the output anchors, and the kind of each output (`null` for a plain value).
    #[test]
    fn capture_facts_in_the_cell_annotation_are_read_without_loss() {
        let nb = notebook_with_annotation(json!({
            "unit": 2,
            "language": "julia",
            "line": 15,
            "expression_starts": [0, 1, 2],
            "output_expressions": [1, 2, 3],
            "output_anchors": [1, 2, 3],
            "output_kinds": ["message", null, null],
        }));
        let cells = read_engine_notebook(&nb).unwrap();
        let facts = &cells[0].quarto.as_ref().unwrap().facts;
        assert_eq!(facts.expression_starts, Some(vec![0, 1, 2]));
        assert_eq!(facts.output_anchors, Some(vec![1, 2, 3]));
        assert_eq!(
            facts.output_kinds,
            Some(vec![Some("message".to_string()), None, None])
        );
    }

    /// With none of the keys (an older runner, or units absent) a cell reads as before.
    #[test]
    fn an_annotation_without_capture_facts_reads_as_before() {
        let nb = notebook_with_annotation(json!({"unit": 1, "language": "julia", "line": 9}));
        let cells = read_engine_notebook(&nb).unwrap();
        let q = cells[0].quarto.as_ref().unwrap();
        assert_eq!(q.language.as_deref(), Some("julia"));
        assert_eq!(q.line, Some(9));
        assert_eq!(q.facts, CaptureFacts::default());
    }

    /// A key of the wrong shape is a runner bug, reported as the other malformed fields are.
    #[test]
    fn a_malformed_capture_fact_is_an_error() {
        for (key, value) in [
            ("expression_starts", json!("0,1")),
            ("output_anchors", json!([1, "two"])),
            ("output_kinds", json!([1])),
        ] {
            let nb = notebook_with_annotation(json!({"language": "julia", key: value}));
            let err = read_engine_notebook(&nb).err().expect(key);
            assert!(err.contains(key), "{err}");
        }
    }
    // ---- OPR-4d: the capture facts become cell metadata under the knitr names ----

    /// `qnr_code` with capture facts added to `metadata.quarto`.
    fn qnr_code_with_facts(
        unit: u64,
        line: u64,
        code: &str,
        outputs: Vec<Value>,
        facts: Value,
    ) -> Value {
        let mut cell = qnr_code(unit, "julia", line, code, outputs, 1);
        for (k, v) in facts.as_object().unwrap() {
            cell["metadata"]["quarto"][k] = v.clone();
        }
        cell
    }

    fn julia_nb_with(cell1: Value) -> Vec<Value> {
        let mut nb = julia_nb();
        nb[1] = cell1;
        nb
    }

    #[test]
    fn julia_capture_facts_become_cell_metadata_under_the_knitr_keys() {
        let ctx = ctx_for(JDOC, &["julia"], true);
        let facts = json!({
            "expression_starts": [0, 1],
            "output_anchors": [1, 2],
            "output_kinds": ["message", null],
        });
        let nb = julia_nb_with(qnr_code_with_facts(
            1,
            7,
            "x = 1",
            vec![out("a\n"), out("b\n")],
            facts,
        ));
        let result = run_julia(&ctx, nb);
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        let record = result.record.expect("a record");
        let extra = &record.cells[1].metadata.extra;
        assert_eq!(extra[EXPRESSION_STARTS_KEY], json!([0, 1]));
        assert_eq!(extra[OUTPUT_ANCHORS_KEY], json!([1, 2]));
        assert_eq!(extra[OUTPUT_KINDS_KEY], json!(["message", null]));
        // Only the cell that carried facts has the keys; the span and the `eval: false` cell do not.
        for i in [0, 2] {
            assert!(
                !record.cells[i]
                    .metadata
                    .extra
                    .contains_key(OUTPUT_ANCHORS_KEY)
            );
        }
    }

    #[test]
    fn julia_keys_present_are_mapped_and_absent_ones_are_left_out() {
        let ctx = ctx_for(JDOC, &["julia"], true);
        let nb = julia_nb_with(qnr_code_with_facts(
            1,
            7,
            "x = 1",
            vec![out("x\n")],
            json!({"expression_starts": [0]}),
        ));
        let record = run_julia(&ctx, nb).record.expect("a record");
        let extra = &record.cells[1].metadata.extra;
        assert_eq!(extra[EXPRESSION_STARTS_KEY], json!([0]));
        assert!(!extra.contains_key(OUTPUT_ANCHORS_KEY));
        assert!(!extra.contains_key(OUTPUT_KINDS_KEY));
    }

    /// Facts do not change what is hashed: the `quarto_` keys are outside the unit hash.
    #[test]
    fn julia_capture_facts_leave_the_exec_hash_alone() {
        let ctx = ctx_for(JDOC, &["julia"], true);
        let plain = run_julia(&ctx, julia_nb()).record.expect("a record");
        let nb = julia_nb_with(qnr_code_with_facts(
            1,
            7,
            "x = 1",
            vec![out("x\n")],
            json!({"expression_starts": [0], "output_anchors": [1], "output_kinds": [null]}),
        ));
        let with = run_julia(&ctx, nb).record.expect("a record");
        assert_eq!(
            plain.cells[1].metadata.quarto_exec_hash,
            with.cells[1].metadata.quarto_exec_hash
        );
    }
}
