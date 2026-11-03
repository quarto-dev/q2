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
//! `known_options`. [`present_knitr_cell`] reads them all and renders the cell, so the three
//! presenter paths (`present_document`, the splice stage, the replay engine) share one
//! implementation and cannot drift.
//!
//! Epic decision 11: the chunk\'s *current* header options win over the recorded ones, so an
//! edit to a presentation option shows without a re-run; the document scope is not layered
//! (decision 3), because it is part of the hash.

use quarto_pandoc_types::config_value::{ConfigValue, ConfigValueKind, InterpretationContext};
use serde_json::Value;

use crate::record::{EngineEntry, RecordCell};

use super::cell::{HoldMode, apply_holds, is_plot_output, render_cell_interleaved};
use super::figures::FigureSink;
use super::options::{CellOptions, OutputKind, config_to_json};
use super::output::{CellOutput, CellResult};
use super::visibility::knitr_visibility;

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

/// Present `cell` as a structured knitr cell, or `None` when it is not one (it has no
/// `quarto_resolved_options`), so the caller presents it as it always did.
///
/// `code` is the cell\'s partitioned code (the `#|` lines removed) and `header` its current
/// `#|` options, resolved without the document scope. `original` is the chunk as written:
/// the info string (`r c1`) and its leading `#|` lines.
pub fn present_knitr_cell(
    language: &str,
    code: &str,
    cell: &RecordCell,
    engine: Option<&EngineEntry>,
    header: Option<&ConfigValue>,
    original: (&str, &str),
    fig: &mut dyn FigureSink,
) -> Option<String> {
    let recorded = cell.metadata.extra.get(RESOLVED_OPTIONS_KEY)?;
    // The code knitr echoed, when it differs from the authored code; the anchors and
    // expression starts index into it.
    let echoed = echoed_code(cell);
    let code = echoed.as_deref().unwrap_or(code);
    let known = engine.and_then(|e| e.known_options.as_deref());
    let merged = layer_options(recorded, header, known);

    let mut result = CellResult::from_nbformat(&cell.outputs, cell.execution_count);
    let mut kinds = output_kinds(cell);
    let mut anchors = usize_array(cell.metadata.extra.get(OUTPUT_ANCHORS_KEY));
    let starts = usize_array(cell.metadata.extra.get(EXPRESSION_STARTS_KEY)).unwrap_or_default();

    // Holds run at present time over the capture-order anchors (epic decision 21).
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
                || (hold.text && matches!(o, CellOutput::Stream { name, .. } if name == "stdout"))
        };
        // Held outputs are never a message or a warning, so the kinds of the others stay in
        // order and each output moved to the end has none.
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

    let options = CellOptions::from_knitr_known(&merged, known)
        .with_output_kinds(kinds)
        .with_original_chunk(original.0, original.1);
    Some(render_cell_interleaved(
        language,
        code,
        &result,
        anchors.as_deref(),
        &starts,
        &options,
        fig,
        knitr_visibility(&merged),
    ))
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
    if let Some(entries) = header.and_then(ConfigValue::as_map_entries) {
        for entry in entries {
            // The capture evaluated an `!expr` in R and recorded the result; the expression
            // text must not replace it.
            if matches!(entry.value.value, ConfigValueKind::Expr(_)) {
                continue;
            }
            let dotted = entry.key.replace('-', ".");
            let key = if known.is_some_and(|k| k.contains(&dotted)) {
                dotted
            } else {
                entry.key.clone()
            };
            merged.insert(key, config_to_json(&entry.value));
        }
    }
    Value::Object(merged)
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
        assert_eq!(merged["fig.cap"], "evaluated", "{merged}");
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
}
