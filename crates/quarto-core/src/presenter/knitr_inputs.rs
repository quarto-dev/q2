/*
 * presenter/knitr_inputs.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * KNR-3 Phase 3B: the one reader from a knitr record cell to the presenter's inputs.
 */

//! The shared record-to-presenter reader for knitr cells (KNR-3 re-scope item 3).
//!
//! A structured knitr cell carries, besides its nbformat outputs, the inputs the presenter
//! needs and cannot recover from them: `quarto_output_anchors`, `quarto_output_kinds`,
//! `quarto_expression_starts` and `quarto_resolved_options`, plus the engine entry\'s
//! `known_options`. [`CellInputs::from_cell`] reads them all and [`present_with_inputs`] renders the cell, so the
//! presenter paths (`present_document`, the splice stage, the replay engine) share one
//! implementation and cannot drift.
//!
//! Epic decision 11: the chunk\'s *current* header options win over the recorded ones, so an
//! edit to a presentation option shows without a re-run; the document scope is not layered
//! (decision 3), because it is part of the hash.

use quarto_pandoc_types::config_value::{ConfigValue, ConfigValueKind, InterpretationContext};
use serde_json::Value;

use crate::record::{EngineEntry, RecordCell};

use super::cell::{
    HoldMode, apply_holds, before_code_anchors, is_included_graphic, is_plot_output,
    render_cell_interleaved,
};
use super::figures::FigureSink;
use super::options::{CellOptions, OutputKind, config_to_json};
use super::output::{CellOutput, CellResult};
use super::visibility::CellVisibility;

/// The cell-metadata keys this reader consumes.
pub const ECHOED_CODE_KEY: &str = "quarto_echoed_code";
pub const OUTPUT_ANCHORS_KEY: &str = "quarto_output_anchors";
pub const OUTPUT_KINDS_KEY: &str = "quarto_output_kinds";
pub const EXPRESSION_STARTS_KEY: &str = "quarto_expression_starts";
pub const RESOLVED_OPTIONS_KEY: &str = "quarto_resolved_options";

/// The chunk as the author wrote it, for `fenced.echo`: the info string (`r c1`) and the
/// leading `#|` lines of `author_code`.
pub fn original_chunk(lang: &str, attr_tail: &str, author_code: &str) -> (String, String) {
    let lines = author_code
        .lines()
        .take_while(|l| l.starts_with("#|"))
        .collect::<Vec<_>>()
        .join("\n");
    (format!("{lang} {attr_tail}").trim().to_string(), lines)
}

/// Everything a record cell can supply beyond its outputs, each input independently optional
/// (OPR-1a, decision D1). A missing input degrades one feature and no other: without anchors
/// the cell renders whole; without expression starts `prompt` treats the code as one
/// expression; without kinds numeric `warning` and `message` are ignored; without a figure
/// count figures get no id or caption wrapper.
///
/// `resolved_options` is one field like the rest. Its presence also decides the scope rule
/// (D3): R already baked the document `execute:` scope into resolved options, so a cell that
/// carries them is not layered with the scope; a cell that carries none is.
#[derive(Debug, Clone, Default)]
pub struct CellInputs {
    /// The lines the engine echoed, when they differ from the authored code; the anchors and
    /// expression starts index into them.
    pub echoed_code: Option<String>,
    /// Per output, the number of code lines that precede it.
    pub anchors: Option<Vec<usize>>,
    /// The line index where each expression of the code starts.
    pub expression_starts: Option<Vec<usize>>,
    /// Per output, whether it is a message or a warning; empty when unknown.
    pub output_kinds: Vec<Option<OutputKind>>,
    /// The number of plots the engine\'s plot hook wrote.
    pub figure_count: Option<usize>,
    /// The engine\'s resolved chunk options, under the engine\'s own names.
    pub resolved_options: Option<Value>,
    /// The option names the engine knows; any other becomes a cell attribute.
    pub known_options: Option<Vec<String>>,
    /// The chunk as written (info string, `#|` lines), for `fenced.echo`.
    pub original_chunk: Option<(String, String)>,
    /// The engine\'s declaration puts `warning` in the hash, so capture enforced it and the
    /// presenter leaves stderr alone (D2). `false`, the default, is presenter enforcement.
    pub capture_enforced_warning: bool,
}

impl CellInputs {
    /// Read the inputs from `cell`\'s metadata and its engine entry. `original` is the chunk as
    /// written; it is kept only with resolved options, the one case that read it before.
    /// This is the only place `quarto_resolved_options` is read.
    pub fn from_cell(
        cell: &RecordCell,
        engine: Option<&EngineEntry>,
        original: (&str, &str),
    ) -> Self {
        let extra = &cell.metadata.extra;
        let resolved_options = extra.get(RESOLVED_OPTIONS_KEY).cloned();
        let figure_count = resolved_options.as_ref().map(|_| {
            CellResult::from_nbformat(&cell.outputs, cell.execution_count)
                .outputs
                .iter()
                .filter(|o| is_plot_output(o) && !is_included_graphic(o))
                .count()
        });
        let original_chunk = resolved_options
            .as_ref()
            .map(|_| (original.0.to_string(), original.1.to_string()));
        CellInputs {
            echoed_code: echoed_code(cell),
            anchors: usize_array(extra.get(OUTPUT_ANCHORS_KEY)),
            expression_starts: usize_array(extra.get(EXPRESSION_STARTS_KEY)),
            output_kinds: output_kinds(cell),
            figure_count,
            resolved_options,
            known_options: engine.and_then(|e| e.known_options.clone()),
            original_chunk,
            capture_enforced_warning: !presenter_enforces_warning(cell, engine),
        }
    }
}

/// Decision D2: whether the presenter enforces `warning` for `cell`. Exactly when the cell\'s
/// engine declares it display-only; an engine with no usable declaration, and a record with no
/// entry for the cell\'s engine (an imported one), get presenter enforcement.
pub fn presenter_enforces_warning(cell: &RecordCell, engine: Option<&EngineEntry>) -> bool {
    engine.is_none_or(|e| {
        e.hash_definition
            .presenter_enforces(cell.metadata.quarto_hash_rule.as_deref(), "warning")
    })
}

/// Present one cell from its outputs and `inputs`.
///
/// `code` is the cell\'s partitioned code (the `#|` lines removed) and `header` its current
/// `#|` options (read as knitr reads them), used only with resolved options. `fallback` is the
/// options and visibility a cell with no resolved options gets (the document scope layered
/// under its own options).
pub fn present_with_inputs(
    language: &str,
    code: &str,
    cell: &RecordCell,
    inputs: &CellInputs,
    header: Option<&ConfigValue>,
    fallback: (CellOptions, CellVisibility),
    fig: &mut dyn FigureSink,
) -> String {
    let code = inputs.echoed_code.as_deref().unwrap_or(code);
    let known = inputs.known_options.as_deref();
    let mut result = CellResult::from_nbformat(&cell.outputs, cell.execution_count);
    let mut merged: Option<Value> = None;
    let (options, visibility) = match &inputs.resolved_options {
        Some(recorded) => {
            let mut m = layer_options(recorded, header, known);
            synthesize_layout(
                &mut m,
                result.outputs.iter().filter(|o| is_plot_output(o)).count(),
            );
            let pair = (
                CellOptions::from_knitr_known(&m, known),
                CellVisibility::from_flags(
                    |k| m.get(k).and_then(Value::as_bool),
                    !inputs.capture_enforced_warning,
                ),
            );
            merged = Some(m);
            pair
        }
        None => fallback,
    };
    let mut kinds = inputs.output_kinds.clone();
    let mut anchors = inputs.anchors.clone();
    let starts = inputs.expression_starts.clone().unwrap_or_default();

    if let Some(merged) = &merged {
        // `fig.beforecode` then the holds, as knitr orders them; both run at present time over
        // the capture-order anchors (epic decision 21).
        if merged.get("fig.beforecode").and_then(Value::as_bool) == Some(true)
            && let Some(a) = anchors
                .as_deref()
                .filter(|a| a.len() == result.outputs.len())
        {
            anchors = Some(before_code_anchors(&result.outputs, a));
        }
        let is = |key: &str, want: &str| merged.get(key).and_then(Value::as_str) == Some(want);
        let hold = HoldMode {
            plots: is("fig.show", "hold"),
            text: is("results", "hold"),
        };
        if let Some(a) = anchors
            .as_deref()
            .filter(|a| a.len() == result.outputs.len())
            && (hold.plots || hold.text)
        {
            let held = |o: &CellOutput| {
                (hold.plots && is_plot_output(o))
                    || (hold.text
                        && matches!(o, CellOutput::Stream { name, .. } if name == "stdout"))
            };
            // Held outputs are never a message or a warning, so the kinds of the others stay
            // in order and each output moved to the end has none.
            if kinds.len() == result.outputs.len() {
                let kept: Vec<_> = kinds
                    .iter()
                    .zip(&result.outputs)
                    .filter(|(_, o)| !held(o))
                    .map(|(k, _)| *k)
                    .collect();
                kinds = kept;
            }
            let (outputs, new_anchors) = apply_holds(
                std::mem::take(&mut result.outputs),
                a,
                code.lines().count(),
                hold,
            );
            if !kinds.is_empty() || outputs.is_empty() {
                kinds.resize(outputs.len(), None);
            }
            result.outputs = outputs;
            anchors = Some(new_anchors);
        }
    }

    let mut options = options.with_output_kinds(kinds);
    if let Some(n) = inputs.figure_count {
        options = options.with_figure_count(n);
    }
    if let Some((info, lines)) = &inputs.original_chunk {
        options = options.with_original_chunk(info, lines);
    }
    render_cell_interleaved(
        language,
        code,
        &result,
        anchors.as_deref(),
        &starts,
        &options,
        fig,
        visibility,
    )
}

/// The knitr `#|` header read as literal YAML scalars (R\'s `yaml.load` and Quarto 1 keep
/// them): `InterpretationContext::ProjectConfig` does not parse a string as markdown, so
/// `comment: "##"` is not an empty heading and `*marked up*` keeps its asterisks. The Jupyter
/// path keeps `DocumentMetadata`.
pub fn knitr_header_options(
    options: Option<quarto_yaml::YamlWithSourceInfo>,
) -> Option<ConfigValue> {
    let o = options?;
    let mut collector = pampa::utils::diagnostic_collector::DiagnosticCollector::new();
    let config = pampa::pandoc::meta::yaml_to_config_value(
        o,
        InterpretationContext::ProjectConfig,
        &mut collector,
    );
    for d in collector.into_diagnostics() {
        tracing::warn!("cell option conversion diagnostic: {d:?}");
    }
    Some(config)
}

/// `quarto_echoed_code`: the lines knitr echoed, joined; `None` when absent or malformed.
fn echoed_code(cell: &RecordCell) -> Option<String> {
    cell.metadata
        .extra
        .get(ECHOED_CODE_KEY)?
        .as_array()?
        .iter()
        .map(|l| l.as_str())
        .collect::<Option<String>>()
}

/// The recorded options with the current header options layered over them (epic decision 11).
/// A header key written with a dash is read under its dotted name when `known` lists that
/// name: q2\'s R layer accepts either spelling for a knitr option (`normalize_options`), and
/// the record keeps knitr\'s.
fn layer_options(
    recorded: &Value,
    header: Option<&ConfigValue>,
    known: Option<&[String]>,
) -> Value {
    let mut merged = recorded.as_object().cloned().unwrap_or_default();
    // The names R could not serialize are bookkeeping, not an option.
    merged.remove("unserializable");
    // Step 1 (D1): the recorded keys under their canonical names.
    super::options::canonicalize_options(&mut merged);
    // Step 2: overlay the header.
    if let Some(entries) = header.and_then(ConfigValue::as_map_entries) {
        for entry in entries {
            // The capture evaluated an `!expr` in R and recorded the result; the expression
            // text must not replace it.
            if matches!(entry.value.value, ConfigValueKind::Expr(_)) {
                continue;
            }
            // An aliased option is read under its canonical name, whichever spelling the
            // header used; any other dashed key keeps the dotted rewrite when `known` lists it.
            let canonical = super::options::canonical_option_name(&entry.key);
            let key = if canonical != entry.key || super::options::is_canonical_alias(canonical) {
                canonical.to_string()
            } else {
                let dotted = entry.key.replace('-', ".");
                if known.is_some_and(|k| k.contains(&dotted)) {
                    dotted
                } else {
                    entry.key.clone()
                }
            };
            merged.insert(key, config_to_json(&entry.value));
        }
    }
    // Step 3 (D2): knitr\'s sentinels, after the overlay.
    super::options::drop_knitr_sentinels(&mut merged);
    Value::Object(merged)
}

/// hooks.R's layout synthesis (lines 485-531): `fig.sep` builds `layout` from the figure count
/// and `out.width` (a non-empty separator ends a row), else `fig.ncol` sets `layout-ncol`, and
/// `fig.valign` sets `layout-valign` unless it is `"default"`. `fig.sep` and `fig.ncol` are
/// consumed (knitr options, never forwarded); `fig.valign` stays, see below. `fig_num` is the number of plots the
/// cell shows; with none, there is nothing to lay out.
fn synthesize_layout(merged: &mut Value, fig_num: usize) {
    let Some(map) = merged.as_object_mut() else {
        return;
    };
    let mut take = |dotted: &str, dashed: &str| {
        let a = map.remove(dotted);
        let b = map.remove(dashed);
        b.or(a).filter(|v| !v.is_null())
    };
    let sep = take("fig.sep", "fig-sep");
    let ncol = take("fig.ncol", "fig-ncol");
    // Not consumed: knitr does not know `fig.valign`, so live knitr also forwards it as an
    // unknown attribute beside `layout-valign` (the corpus oracle shows both).
    let valign = ["fig-valign", "fig.valign"]
        .into_iter()
        .find_map(|k| map.get(k).filter(|v| !v.is_null()).cloned());
    if let Some(sep) = sep {
        if fig_num > 0 {
            let recycle = |v: &Value| -> Vec<Value> {
                match v {
                    Value::Array(a) if !a.is_empty() => a.clone(),
                    Value::Array(_) => vec![],
                    other => vec![other.clone()],
                }
            };
            let seps = recycle(&sep);
            let widths = map
                .get("out-width")
                .or_else(|| map.get("out.width"))
                .map(recycle)
                .filter(|w| !w.is_empty())
                .unwrap_or_else(|| vec![Value::from(1)]);
            let mut layout: Vec<Value> = Vec::new();
            let mut row: Vec<Value> = Vec::new();
            for i in 0..fig_num {
                row.push(widths[i % widths.len()].clone());
                let ends_row = match seps.get(i % seps.len().max(1)) {
                    Some(Value::String(s)) => !s.is_empty(),
                    Some(_) => true,
                    None => false,
                };
                if ends_row {
                    layout.push(Value::Array(std::mem::take(&mut row)));
                }
            }
            if !row.is_empty() {
                layout.push(Value::Array(row));
            }
            map.insert("layout".to_string(), Value::Array(layout));
        }
    } else if let Some(ncol) = ncol {
        map.insert("layout-ncol".to_string(), ncol);
    }
    if let Some(v) = valign.filter(|v| v.as_str() != Some("default")) {
        map.insert("layout-valign".to_string(), v);
    }
}

/// `quarto_output_kinds`: `"message"`, `"warning"` or null per output; empty when absent.
fn output_kinds(cell: &RecordCell) -> Vec<Option<OutputKind>> {
    cell.metadata
        .extra
        .get(OUTPUT_KINDS_KEY)
        .and_then(Value::as_array)
        .map(|kinds| {
            kinds
                .iter()
                .map(|k| match k.as_str() {
                    Some("message") => Some(OutputKind::Message),
                    Some("warning") => Some(OutputKind::Warning),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// An array of non-negative integers, or `None` when absent or malformed.
fn usize_array(value: Option<&Value>) -> Option<Vec<usize>> {
    value?
        .as_array()?
        .iter()
        .map(|v| v.as_u64().map(|n| n as usize))
        .collect()
}

/// A rich hand-built knitr cell shared by the three presenter paths\' tests: every input the
/// reader consumes is set, so each path\'s test proves it reaches the reader (and that the three
/// cannot drift).
#[cfg(test)]
pub(crate) mod fixture {
    use super::*;
    use serde_json::json;

    /// The chunk as written: a dashed header option and an unknown one.
    pub const SOURCE: &str = "```{r}\n#| class-source: mine\n#| bar.unknown: 2\na\nb\n```\n";
    pub const KNOWN: &[&str] = &[
        "echo",
        "class.source",
        "class.message",
        "class.warning",
        "results",
        "prompt",
    ];

    /// stdout at line 1, a message at line 1, stdout at line 2.
    pub fn outputs() -> Vec<Value> {
        vec![
            json!({"output_type": "stream", "name": "stdout", "text": "1"}),
            json!({"output_type": "stream", "name": "stderr", "text": "a message"}),
            json!({"output_type": "stream", "name": "stdout", "text": "2"}),
        ]
    }

    /// Add the knitr keys to a cell built from [`SOURCE`] and its `outputs()`.
    pub fn apply(cell: &mut RecordCell) {
        let extra = [
            (
                RESOLVED_OPTIONS_KEY,
                json!({"results": "hold", "prompt": true, "class.message": "m",
                       "foo.unknown": "1", "unserializable": ["fig.process"]}),
            ),
            (OUTPUT_KINDS_KEY, json!([null, "message", null])),
            (OUTPUT_ANCHORS_KEY, json!([1, 1, 2])),
            (EXPRESSION_STARTS_KEY, json!([0, 1])),
        ];
        for (k, v) in extra {
            cell.metadata.extra.insert(k.to_string(), v);
        }
    }

    /// One assertion per input.
    pub fn assert_rich(md: &str) {
        // The splice stage writes the AST back as qmd, which spells an attribute with double
        // quotes and a lone class as an info word; the reader writes the other forms.
        let md = &md.replace('"', "'").replace("```m\n", "```{.m}\n");
        // known_options: only the names knitr does not know become attributes, from the record
        // (`foo.unknown`) and from the current header (`bar.unknown`).
        assert!(md.contains("foo.unknown='1'"), "{md}");
        assert!(md.contains("bar.unknown='2'"), "{md}");
        assert!(!md.contains("unserializable"), "{md}");
        // quarto_output_kinds
        assert!(md.contains("```{.m}\na message"), "{md}");
        // the header spelling `class-source`, read under its known dotted name
        assert!(md.contains(".mine"), "{md}");
        // quarto_expression_starts (prompt)
        assert!(md.contains("\n> a\n") && md.contains("\n> b\n"), "{md}");
        // quarto_output_anchors with `results = hold`: the message stays after line 1 and
        // the held text is one stream after the source
        assert!(
            md.find("a message").unwrap() < md.find("\n12\n").unwrap(),
            "{md}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell_options::partition_cell_options;
    use crate::presenter::figures::FsFigureSink;
    use crate::presenter::visibility::resolve_cell_options;
    use crate::record::{CellMetadata, CellType, HashDefinition, UnitKind};
    use quarto_source_map::SourceInfo;
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::path::Path;

    /// The pre-OPR-1a entry point, kept for these tests: `None` for a cell with no resolved options.
    fn present_knitr_cell(
        language: &str,
        code: &str,
        cell: &RecordCell,
        engine: Option<&EngineEntry>,
        header: Option<&ConfigValue>,
        original: (&str, &str),
        fig: &mut dyn FigureSink,
    ) -> Option<String> {
        let inputs = CellInputs::from_cell(cell, engine, original);
        inputs.resolved_options.as_ref()?;
        let fallback = (CellOptions::default(), CellVisibility::resolve(None, true));
        Some(present_with_inputs(
            language, code, cell, &inputs, header, fallback, fig,
        ))
    }

    fn stream(name: &str, text: &str) -> Value {
        json!({"output_type": "stream", "name": name, "text": text})
    }

    /// A hand-built structured knitr cell: `extra` are its `quarto_*` keys.
    fn cell(source: &str, outputs: Vec<Value>, extra: Vec<(&str, Value)>) -> RecordCell {
        let mut keys: BTreeMap<String, Value> = BTreeMap::new();
        keys.insert(RESOLVED_OPTIONS_KEY.into(), json!({}));
        for (k, v) in extra {
            keys.insert(k.into(), v);
        }
        RecordCell {
            cell_type: CellType::Code,
            execution_count: None,
            id: "id".into(),
            metadata: CellMetadata {
                quarto_cell_id: "0-aaaaaaaa-unlabeled".into(),
                quarto_content_hash: "aaaaaaaa".into(),
                quarto_label: None,
                quarto_kind: UnitKind::Cell,
                quarto_lang: "r".into(),
                quarto_engine: Some("knitr".into()),
                quarto_kernel: None,
                quarto_location: None,
                quarto_exec_hash: "h".into(),
                quarto_hash_rule: Some("structured".into()),
                extra: keys,
            },
            outputs,
            source: vec![source.to_string()],
        }
    }

    fn engine(known: Option<&[&str]>) -> EngineEntry {
        let e = crate::record::EngineEntryInput {
            known_options: known.map(|k| k.iter().map(|s| s.to_string()).collect()),
            ..Default::default()
        };
        let def = HashDefinition::conservative("knitr");
        let units = [];
        let texts = crate::record::AuthorTexts::default();
        crate::record::RecordBuilder::new("knitr", &def, &units, &texts)
            .unwrap()
            .finish(e)
            .unwrap()
            .engine
    }

    /// `#|` lines as the resolved header options (no document scope).
    fn header(lines: &str) -> Option<ConfigValue> {
        let parsed = partition_cell_options(
            "r",
            lines,
            SourceInfo::generated(quarto_source_map::By::unknown()),
        )
        .unwrap();
        resolve_cell_options(None, parsed.options)
    }

    fn present(
        code: &str,
        c: &RecordCell,
        engine: Option<&EngineEntry>,
        header: Option<&ConfigValue>,
    ) -> Option<String> {
        let mut fig = FsFigureSink::new(Path::new("/nonexistent/doc.qmd"));
        present_knitr_cell("r", code, c, engine, header, ("r", ""), &mut fig)
    }

    #[test]
    fn a_cell_with_none_of_the_knitr_keys_is_not_a_knitr_cell() {
        let mut c = cell("x", vec![], vec![]);
        c.metadata.extra.clear();
        assert_eq!(present("x", &c, None, None), None);
    }

    #[test]
    fn quarto_output_kinds_separate_messages_from_warnings() {
        let c = cell(
            "a\nb\n",
            vec![
                stream("stderr", "a message"),
                stream("stderr", "Warning: w"),
            ],
            vec![
                (
                    RESOLVED_OPTIONS_KEY,
                    json!({"class.message": "m", "class.warning": "w"}),
                ),
                (OUTPUT_KINDS_KEY, json!(["message", "warning"])),
            ],
        );
        let md = present("a\nb\n", &c, None, None).unwrap();
        assert!(md.contains("```{.m}\na message"), "{md}");
        assert!(md.contains("```{.w}\nWarning: w"), "{md}");
    }

    #[test]
    fn quarto_output_anchors_interleave_source_and_output() {
        let c = cell(
            "a\nb\n",
            vec![stream("stdout", "1"), stream("stdout", "2")],
            vec![(OUTPUT_ANCHORS_KEY, json!([1, 2]))],
        );
        let md = present("a\nb\n", &c, None, None).unwrap();
        let at = |s: &str| md.find(s).unwrap_or_else(|| panic!("{s:?} in {md}"));
        assert!(
            at("\na\n") < at("\n1\n") && at("\n1\n") < at("\nb\n") && at("\nb\n") < at("\n2\n"),
            "{md}"
        );
    }

    #[test]
    fn without_anchors_the_source_comes_first() {
        let c = cell("a\nb\n", vec![stream("stdout", "1")], vec![]);
        let md = present("a\nb\n", &c, None, None).unwrap();
        assert!(
            md.find("\nb\n").unwrap() < md.find("\n1\n").unwrap(),
            "{md}"
        );
    }

    #[test]
    fn recorded_options_are_read_from_quarto_resolved_options() {
        let c = cell(
            "a\n",
            vec![stream("stdout", "1")],
            vec![(RESOLVED_OPTIONS_KEY, json!({"echo": false}))],
        );
        let md = present("a\n", &c, None, None).unwrap();
        assert!(!md.contains("cell-code"), "{md}");
        assert!(md.contains("\n1\n"), "{md}");
    }

    #[test]
    fn echoed_code_replaces_the_authored_code_and_the_anchors_index_into_it() {
        // A numeric `eval` masked the first line; the output follows the second.
        let c = cell(
            "print(1)\nprint(2)\n",
            vec![stream("stdout", "[1] 2\n")],
            vec![
                (RESOLVED_OPTIONS_KEY, json!({})),
                (ECHOED_CODE_KEY, json!(["## print(1)\n", "print(2)"])),
                (OUTPUT_ANCHORS_KEY, json!([2])),
            ],
        );
        let md = present("print(1)\nprint(2)\n", &c, None, None).unwrap();
        assert!(md.contains("## print(1)\nprint(2)\n```"), "{md}");
        assert!(!md.contains("\nprint(1)"), "{md}");
    }

    #[test]
    fn the_current_header_options_win_over_the_recorded_ones() {
        let c = cell(
            "a\n",
            vec![stream("stdout", "1")],
            vec![(RESOLVED_OPTIONS_KEY, json!({"echo": true}))],
        );
        let h = header("#| echo: false");
        let md = present("a\n", &c, None, h.as_ref()).unwrap();
        assert!(!md.contains("cell-code"), "{md}");
    }

    /// `#|` lines as knitr header options (the literal reader).
    fn knitr_header(lines: &str) -> Option<ConfigValue> {
        let parsed = partition_cell_options(
            "r",
            lines,
            SourceInfo::generated(quarto_source_map::By::unknown()),
        )
        .unwrap();
        knitr_header_options(parsed.options)
    }

    #[test]
    fn header_strings_stay_literal_scalars() {
        // `"##"` is an empty heading and `*x*` loses its markup when read as markdown.
        let h = knitr_header("#| comment: \"##\"\n#| fig-cap: \"*marked up*\"").unwrap();
        assert_eq!(h.get("comment").unwrap().as_str(), Some("##"));
        assert_eq!(h.get("fig-cap").unwrap().as_str(), Some("*marked up*"));
    }

    #[test]
    fn a_literal_comment_reaches_the_presented_cell() {
        let c = cell(
            "a\n",
            vec![stream("stdout", "1")],
            vec![(RESOLVED_OPTIONS_KEY, json!({"comment": "#>"}))],
        );
        let h = knitr_header("#| comment: \"##\"");
        let md = present("a\n", &c, None, h.as_ref()).unwrap();
        assert!(md.contains("\n## 1\n"), "{md}");
    }

    #[test]
    fn an_expr_header_option_does_not_override_the_recorded_value() {
        // The capture evaluated `!expr` in R and recorded the result; the expression text
        // must not replace it.
        let c = cell(
            "a\n",
            vec![],
            vec![(RESOLVED_OPTIONS_KEY, json!({"fig.cap": "evaluated"}))],
        );
        let e = engine(Some(&["fig.cap"]));
        let h = knitr_header("#| fig-cap: !expr paste0('a', 'b')");
        let merged = layer_options(
            &c.metadata.extra[RESOLVED_OPTIONS_KEY],
            h.as_ref(),
            e.known_options.as_deref(),
        );
        assert_eq!(merged["fig-cap"], "evaluated", "{merged}");
    }

    /// The names `capture.R` ships as `known_options`: knitr\'s dotted spellings only.
    const SHIPPED_KNOWN: &[&str] = &[
        "echo",
        "fig.align",
        "fig.env",
        "fig.pos",
        "fig.scap",
        "fig.link",
        "fig.alt",
        "fig.cap",
        "fig.column",
        "out.width",
        "out.height",
        "out.extra",
        "class.source",
        "classes",
        "cap-location",
        "tbl-column",
        "tbl-cap-location",
        "code-overflow",
        "fig-column",
        "fig-cap-location",
        "layout-align",
    ];

    fn layered(recorded: Value, header_yaml: &str, known: &[&str]) -> Value {
        let h = header(header_yaml);
        let known: Vec<String> = known.iter().map(|s| s.to_string()).collect();
        layer_options(&recorded, h.as_ref(), Some(&known))
    }

    /// KNR-3 slice 7, D1: one canonical (Quarto) spelling; a recorded knitr spelling and the
    /// same option written in the header meet under it.
    #[test]
    fn a_recorded_dotted_name_and_a_canonical_one_give_the_same_merged_options() {
        let dotted = layered(
            json!({"fig.align": "center", "out.width": "50%"}),
            "",
            SHIPPED_KNOWN,
        );
        let canonical = layered(
            json!({"fig-align": "center", "out-width": "50%"}),
            "",
            SHIPPED_KNOWN,
        );
        assert_eq!(dotted, canonical);
        assert_eq!(dotted["fig-align"], "center");
        assert_eq!(dotted["out-width"], "50%");
        assert!(dotted.get("fig.align").is_none() && dotted.get("out.width").is_none());
    }

    /// D2: knitr\'s sentinels are dropped after the header overlay, never before, so a header
    /// `default` clears a recorded value.
    #[test]
    fn knitr_sentinels_are_dropped_after_the_overlay() {
        let m = layered(
            json!({"fig.align": "default", "fig.env": "figure", "fig.pos": "", "echo": true}),
            "",
            SHIPPED_KNOWN,
        );
        for gone in [
            "fig-align",
            "fig-env",
            "fig-pos",
            "fig.align",
            "fig.env",
            "fig.pos",
        ] {
            assert!(m.get(gone).is_none(), "{gone} survived: {m}");
        }
        assert_eq!(m["echo"], true);
        // A header `default` over a recorded `right` clears it (overlay first, then drop).
        let m = layered(
            json!({"fig.align": "right"}),
            "#| fig-align: default",
            SHIPPED_KNOWN,
        );
        assert!(
            m.get("fig-align").is_none() && m.get("fig.align").is_none(),
            "{m}"
        );
        // A header value over a recorded sentinel shows.
        let m = layered(
            json!({"fig.env": "figure"}),
            "#| fig-env: figure*",
            SHIPPED_KNOWN,
        );
        assert_eq!(m["fig-env"], "figure*");
    }

    #[test]
    fn a_header_edit_replaces_the_recorded_value_with_no_stale_twin() {
        let m = layered(
            json!({"fig-column": "page", "fig.column": "screen", "fig.align": "left"}),
            "#| fig-column: margin\n#| fig-align: right",
            SHIPPED_KNOWN,
        );
        assert_eq!(m["fig-column"], "margin");
        assert_eq!(m["fig-align"], "right");
        assert!(
            m.get("fig.column").is_none() && m.get("fig.align").is_none(),
            "{m}"
        );
        // A layer holding both spellings keeps the dashed one (hooks.R reads it first).
        let m = layered(
            json!({"fig-column": "page", "fig.column": "screen"}),
            "",
            SHIPPED_KNOWN,
        );
        assert_eq!(m["fig-column"], "page");
        // A non-aliased dashed header key still gets the existing dotted rewrite.
        let m = layered(json!({}), "#| class-source: mine", SHIPPED_KNOWN);
        assert_eq!(m["class.source"], "mine");
    }

    /// With the shipped `known_options` (dotted names only), a canonical dashed key is not an
    /// unknown option: it must not become a `key='value'` on the cell div.
    #[test]
    fn a_canonical_alias_never_leaks_as_a_cell_attribute() {
        let c = cell("a\n", vec![], vec![]);
        let e = engine(Some(SHIPPED_KNOWN));
        for line in [
            "#| fig-align: center",
            "#| fig-scap: short",
            "#| fig-link: http://x",
            "#| fig-env: figure*",
            "#| fig-pos: h",
            "#| out-width: 50%",
            "#| out-height: 50%",
            "#| out-extra: x",
        ] {
            let h = header(line);
            let md = present("a\n", &c, Some(&e), h.as_ref()).unwrap();
            let first = md.lines().next().unwrap();
            assert!(!first.contains("='"), "{line} leaked onto the div: {first}");
        }
    }

    #[test]
    fn a_float_option_survives_the_layering() {
        let m = layered(
            json!({"out-width": "50%"}),
            "#| out-width: 0.5",
            SHIPPED_KNOWN,
        );
        assert_eq!(m["out-width"], 0.5, "{m}");
    }

    #[test]
    fn a_dashed_header_option_is_read_under_its_known_dotted_name() {
        let c = cell("a\n", vec![], vec![]);
        let e = engine(Some(&["class.source", "echo"]));
        let h = header("#| class-source: mine");
        let md = present("a\n", &c, Some(&e), h.as_ref()).unwrap();
        assert!(
            md.contains(".cell-code .mine") || md.contains(".mine"),
            "{md}"
        );
    }

    #[test]
    fn known_options_decide_which_options_become_cell_attributes() {
        let c = cell(
            "a\n",
            vec![],
            vec![(
                RESOLVED_OPTIONS_KEY,
                json!({"echo": true, "foo.unknown": "1"}),
            )],
        );
        let e = engine(Some(&["echo"]));
        let md = present("a\n", &c, Some(&e), None).unwrap();
        assert!(md.starts_with("::: {.cell foo.unknown='1'}"), "{md}");
        // A new unknown option written in the header shows without a re-run.
        let h = header("#| bar.unknown: 2");
        let md = present("a\n", &c, Some(&e), h.as_ref()).unwrap();
        assert!(md.contains("bar.unknown='2'"), "{md}");
        // No engine entry, or no list on it, filters nothing.
        let md = present("a\n", &c, None, None).unwrap();
        assert!(md.starts_with("::: {.cell}"), "{md}");
        let md = present("a\n", &c, Some(&engine(None)), None).unwrap();
        assert!(md.starts_with("::: {.cell}"), "{md}");
    }

    #[test]
    fn the_original_chunk_feeds_fenced_echo() {
        let c = cell(
            "x\n",
            vec![],
            vec![(RESOLVED_OPTIONS_KEY, json!({"fenced.echo": true}))],
        );
        let mut fig = FsFigureSink::new(Path::new("/nonexistent/doc.qmd"));
        let md = present_knitr_cell(
            "r",
            "x\n",
            &c,
            None,
            None,
            ("r c1", "#| fenced.echo: true"),
            &mut fig,
        )
        .unwrap();
        assert!(
            md.contains("````{.cell-code}\n```{{r c1}}\n#| fenced.echo: true\nx\n```\n````"),
            "{md}"
        );
    }

    #[test]
    fn quarto_expression_starts_feed_prompt() {
        let c = cell(
            "a\nb\n",
            vec![],
            vec![
                (RESOLVED_OPTIONS_KEY, json!({"prompt": true})),
                (EXPRESSION_STARTS_KEY, json!([0, 1])),
            ],
        );
        let md = present("a\nb\n", &c, None, None).unwrap();
        assert!(md.contains("> a\n> b"), "{md}");
    }

    fn plot() -> Value {
        json!({"output_type": "display_data",
               "data": {"image/png": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg=="},
               "metadata": {}})
    }

    /// A cell of `n` plots under `options`, presented to markdown.
    fn present_plots(n: usize, options: Value) -> String {
        let c = cell(
            "plot(1)\n",
            (0..n).map(|_| plot()).collect(),
            vec![
                (RESOLVED_OPTIONS_KEY, options),
                (OUTPUT_ANCHORS_KEY, json!(vec![1; n])),
            ],
        );
        let dir = tempfile::tempdir().unwrap();
        let mut fig = FsFigureSink::new(&dir.path().join("doc.qmd"));
        present_knitr_cell("r", "plot(1)\n", &c, None, None, ("r", ""), &mut fig).unwrap()
    }

    #[test]
    fn a_figure_label_is_the_id_of_a_single_plots_image() {
        // hooks.R `knitr_plot_hook`: the label goes in the image's attributes.
        let md = present_plots(1, json!({"label": "fig-one", "fig.cap": "One"}));
        assert!(md.contains("![One](") && md.contains("){#fig-one}"), "{md}");
    }

    #[test]
    fn several_plots_number_the_figure_label() {
        // Cell hook: with more than one figure the placeholder becomes `label-<i>`.
        let md = present_plots(2, json!({"label": "fig-two", "fig.cap": "Two"}));
        assert!(md.contains("){#fig-two-1}"), "{md}");
        assert!(md.contains("){#fig-two-2}"), "{md}");
        assert!(!md.contains("{#fig-two}"), "{md}");
        assert!(!md.contains("#fig-two .cell"), "{md}");
    }

    #[test]
    fn the_label_moves_to_the_cell_div_with_both_fig_cap_and_fig_subcap() {
        let md = present_plots(
            2,
            json!({"label": "fig-pair", "fig.cap": "Both", "fig.subcap": ["First", "Second"]}),
        );
        assert!(md.contains("::: {#fig-pair .cell}"), "{md}");
        assert!(
            md.contains("![First](") && md.contains("){#fig-pair-1}"),
            "{md}"
        );
        assert!(
            md.contains("![Second](") && md.contains("){#fig-pair-2}"),
            "{md}"
        );
        // The cell caption closes the cell div, after the outputs.
        assert!(md.trim_end().ends_with("\nBoth\n:::"), "{md}");
    }

    #[test]
    fn a_figure_label_adds_the_id_before_the_other_image_attributes() {
        let md = present_plots(
            1,
            json!({"label": "fig-one", "fig.cap": "One", "fig.align": "center"}),
        );
        assert!(md.contains("){#fig-one fig-align='center'}"), "{md}");
    }

    #[test]
    fn fig_sep_synthesizes_a_layout_from_the_figure_count() {
        // hooks.R: fig.sep is recycled over the figures; a non-empty separator ends a row.
        let md = present_plots(3, json!({"fig.sep": ["", "\n"]}));
        assert!(md.contains("layout=\"[[1,1],[1]]\""), "{md}");
        assert!(!md.contains("fig.sep") && !md.contains("fig-sep"), "{md}");
    }

    #[test]
    fn fig_sep_uses_out_width_per_figure() {
        let md = present_plots(2, json!({"fig.sep": ["", "\n"], "out.width": [0.4, 0.6]}));
        assert!(md.contains("layout=\"[[0.4,0.6]]\""), "{md}");
    }

    #[test]
    fn fig_ncol_becomes_layout_ncol() {
        let md = present_plots(2, json!({"fig.ncol": 2}));
        assert!(md.contains("layout-ncol=\"2\""), "{md}");
        assert!(!md.contains("fig.ncol") && !md.contains("fig-ncol"), "{md}");
    }

    #[test]
    fn fig_sep_wins_over_fig_ncol() {
        let md = present_plots(2, json!({"fig.sep": ["", "\n"], "fig.ncol": 2}));
        assert!(md.contains("layout=\"[[1,1]]\""), "{md}");
        assert!(!md.contains("layout-ncol"), "{md}");
    }

    #[test]
    fn fig_valign_becomes_layout_valign_unless_default() {
        let md = present_plots(1, json!({"fig.valign": "top"}));
        assert!(md.contains("layout-valign=\"top\""), "{md}");
        // The unknown-attribute forward beside it is the corpus's (`options/fig.valign/*`):
        // it needs the record's `known_options`, which this hand-built cell has none of.
        let md = present_plots(1, json!({"fig.valign": "default"}));
        assert!(!md.contains("layout-valign"), "{md}");
    }

    #[test]
    fn dashed_spellings_of_the_layout_options_work_too() {
        let md = present_plots(2, json!({"fig-ncol": 2, "fig-valign": "bottom"}));
        assert!(md.contains("layout-ncol=\"2\""), "{md}");
        assert!(md.contains("layout-valign=\"bottom\""), "{md}");
    }

    #[test]
    fn results_hold_moves_printed_text_to_the_end() {
        let c = cell(
            "a\nb\n",
            vec![stream("stdout", "1"), stream("stdout", "2")],
            vec![
                (RESOLVED_OPTIONS_KEY, json!({"results": "hold"})),
                (OUTPUT_ANCHORS_KEY, json!([1, 2])),
            ],
        );
        let md = present("a\nb\n", &c, None, None).unwrap();
        assert!(md.contains("\n12\n"), "{md}");
        assert!(
            md.find("\nb\n").unwrap() < md.find("\n12\n").unwrap(),
            "{md}"
        );
    }

    #[test]
    fn include_false_hides_the_cell() {
        let c = cell(
            "a\n",
            vec![stream("stdout", "1")],
            vec![(RESOLVED_OPTIONS_KEY, json!({"include": false}))],
        );
        assert_eq!(present("a\n", &c, None, None).as_deref(), Some(""));
    }

    // ---- OPR-1a: each input is independently optional ------------------------------------

    /// Present `c` with exactly `inputs` and a plain (Jupyter-style) fallback carrying `fallback`.
    fn present_inputs(c: &RecordCell, code: &str, inputs: &CellInputs, fallback: Value) -> String {
        let dir = tempfile::tempdir().unwrap();
        let mut fig = FsFigureSink::new(&dir.path().join("doc.qmd"));
        present_with_inputs(
            "r",
            code,
            c,
            inputs,
            None,
            (
                CellOptions::from_knitr(&fallback),
                CellVisibility::resolve(None, true),
            ),
            &mut fig,
        )
    }

    #[test]
    fn with_every_input_absent_the_cell_renders_whole() {
        let c = cell("a\nb\n", vec![stream("stdout", "1")], vec![]);
        let md = present_inputs(&c, "a\nb\n", &CellInputs::default(), json!({}));
        assert!(
            md.find("a\nb").unwrap() < md.find("cell-output").unwrap(),
            "{md}"
        );
        assert_eq!(md.matches("cell-code").count(), 1, "{md}");
    }

    #[test]
    fn anchors_alone_interleave_source_and_output() {
        let c = cell("a\nb\n", vec![stream("stdout", "1")], vec![]);
        let inputs = CellInputs {
            anchors: Some(vec![1]),
            ..Default::default()
        };
        let md = present_inputs(&c, "a\nb\n", &inputs, json!({}));
        assert_eq!(md.matches("cell-code").count(), 2, "{md}");
        assert!(
            md.find("cell-output").unwrap() < md.rfind("cell-code").unwrap(),
            "{md}"
        );
    }

    #[test]
    fn expression_starts_alone_make_prompt_exact() {
        let c = cell("a\nb\n", vec![], vec![]);
        let without = present_inputs(
            &c,
            "a\nb\n",
            &CellInputs::default(),
            json!({"prompt": true}),
        );
        let inputs = CellInputs {
            expression_starts: Some(vec![0, 1]),
            ..Default::default()
        };
        let with = present_inputs(&c, "a\nb\n", &inputs, json!({"prompt": true}));
        assert!(with.contains("> a\n> b"), "{with}");
        assert_ne!(with, without);
    }

    #[test]
    fn a_figure_count_alone_adds_the_figure_id() {
        let c = cell("plot(1)\n", vec![plot()], vec![]);
        let opts = json!({"label": "fig-one", "fig.cap": "One"});
        let without = present_inputs(&c, "plot(1)\n", &CellInputs::default(), opts.clone());
        let inputs = CellInputs {
            figure_count: Some(1),
            ..Default::default()
        };
        let with = present_inputs(&c, "plot(1)\n", &inputs, opts);
        assert!(with.contains("){#fig-one}"), "{with}");
        assert!(!without.contains("{#fig-one}"), "{without}");
    }

    // ---- OPR-1a: the scope rule (D3), through the one step -------------------------------

    fn step_with_scope(extra: Vec<(&str, Value)>) -> String {
        use crate::presenter::cell_step::present_cell_step;
        let scope = ConfigValue::new_map(
            vec![quarto_pandoc_types::ConfigMapEntry {
                key: "echo".into(),
                key_source: SourceInfo::for_test(),
                value: ConfigValue::new_bool(false, SourceInfo::for_test()),
            }],
            SourceInfo::for_test(),
        );
        let mut c = cell("a\n", vec![stream("stdout", "OUT")], extra);
        if c.metadata.extra.get(RESOLVED_OPTIONS_KEY) == Some(&json!(null)) {
            c.metadata.extra.remove(RESOLVED_OPTIONS_KEY);
        }
        let dir = tempfile::tempdir().unwrap();
        let mut fig = FsFigureSink::new(&dir.path().join("doc.qmd"));
        present_cell_step("r", "", "a\n", &c, None, Some(&scope), &mut fig).unwrap()
    }

    #[test]
    fn a_cell_with_resolved_options_is_not_layered_with_the_document_scope() {
        // R baked the scope into the resolved options; an empty set means "echo as written".
        let md = step_with_scope(vec![]);
        assert!(md.contains("cell-code") && md.contains("OUT"), "{md}");
    }

    #[test]
    fn a_cell_without_resolved_options_is_layered_with_the_document_scope() {
        let md = step_with_scope(vec![(RESOLVED_OPTIONS_KEY, json!(null))]);
        assert!(!md.contains("cell-code") && md.contains("OUT"), "{md}");
    }
    // ---- OPR-4d: a Julia-shaped cell (the three keys, no resolved options) -------------------

    /// What `engine/ts_record.rs` writes for a Julia cell: the facts and nothing else. The
    /// step must interleave on the anchors with no engine test and no `quarto_resolved_options`.
    #[test]
    fn a_julia_shaped_cell_with_only_the_facts_interleaves_per_expression() {
        let c = cell(
            "a\nb\n",
            vec![stream("stdout", "1"), stream("stdout", "2")],
            vec![
                (EXPRESSION_STARTS_KEY, json!([0, 1])),
                (OUTPUT_ANCHORS_KEY, json!([1, 2])),
                (OUTPUT_KINDS_KEY, json!([null, null])),
            ],
        );
        let md = present("a\nb\n", &c, None, None).unwrap();
        assert_eq!(md.matches("cell-code").count(), 2, "{md}");
        assert!(
            md.find("cell-output").unwrap() < md.rfind("cell-code").unwrap(),
            "{md}"
        );
    }
}
