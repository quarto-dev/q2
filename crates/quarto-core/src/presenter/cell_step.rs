/*
 * presenter/cell_step.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * OPR-1a: the one per-cell presenter step.
 */

//! The one per-cell step both presenter loops call (`present_document` for render, the splice
//! stage for preview, the hub and a supplied record).
//!
//! The step reads what the cell supplies ([`CellInputs`]), never which engine made it. It owns
//! the parts of presenting one fenced cell that the two loops used to do twice: partitioning
//! the `#|` options, layering the document scope (D3), expanded cells, forwarding unknown
//! options as cell attributes (D9) and the render itself. The loops keep stored markdown,
//! matching, error policy, figure numbering and stamping.

use quarto_source_map::{By, SourceInfo};

use super::engine_attrs::{cell_engine_attrs, rejection_reasons};
use super::expansion::{render_expanded_unit, split_expansions};
use super::figures::FigureSink;
use super::knitr_inputs::{
    CellInputs, knitr_header_options, present_with_inputs, presenter_enforces_warning,
};
use super::options::CellOptions;
use super::visibility::{CellVisibility, cell_config, resolve_cell_options};
use crate::cell_options::partition_cell_options;
use crate::record::{EngineEntry, Record, RecordCell};
use quarto_pandoc_types::ConfigValue;

/// The entry that ran `cell`: the one its `quarto_engine` names, else the only one.
pub fn engine_entry_for<'a>(
    engines: &'a [EngineEntry],
    cell: &RecordCell,
) -> Option<&'a EngineEntry> {
    match cell.metadata.quarto_engine.as_deref() {
        Some(name) => engines.iter().find(|e| e.name == name),
        None => (engines.len() == 1).then(|| &engines[0]),
    }
}

/// [`engine_entry_for`] over a record\'s entries.
pub fn record_engine_entry<'a>(record: &'a Record, cell: &RecordCell) -> Option<&'a EngineEntry> {
    engine_entry_for(&record.metadata.quarto_engines, cell)
}

/// Present one fenced cell. `author_text` is the unit\'s author text, `attr_tail` the rest of
/// its info string, `scope` the document\'s merged `execute:` scope. The caller has already
/// handled stored markdown and called `fig.begin_cell()`. An `Err` is the message for invalid
/// cell options: render fails with it, the splice keeps the source. The `Ok` side carries the
/// markdown and one reason per engine `quarto_attrs` entry the grammar refused (OPR-7f).
pub fn present_cell_step(
    language: &str,
    attr_tail: &str,
    author_text: &str,
    cell: &RecordCell,
    engine: Option<&EngineEntry>,
    scope: Option<&ConfigValue>,
    fig: &mut dyn FigureSink,
    target: super::inline::InlineTarget,
) -> Result<(String, Vec<String>), String> {
    let parsed =
        partition_cell_options(language, author_text, SourceInfo::generated(By::unknown()))
            .map_err(|e| e.to_string())?;
    let cell_only = cell_config(parsed.options.clone());
    let resolved = resolve_cell_options(scope, parsed.options.clone());
    let visibility =
        CellVisibility::resolve(resolved.as_ref(), presenter_enforces_warning(cell, engine));
    let mut options = CellOptions::from_config(resolved.as_ref())
        .forwarding(cell_only.as_ref())
        .with_target(target);
    if let Some(attrs) = cell_engine_attrs(cell) {
        // Q1 `jupyter.ts:1366-1376`: the auto cell id goes on the wrapper only for ipynb, JATS and
        // html-family output. `InlineTarget.to_html` is the same identifier list as `isHtmlOutput`.
        let attrs = if target.to_html {
            attrs
        } else {
            attrs.without_id()
        };
        options = options.with_engine_attrs(attrs);
    }
    let rejected = rejection_reasons(cell);
    if let Some(expansions) = split_expansions(&cell.outputs) {
        // An expanded cell is one unit presented as one cell per expansion, each with its own
        // options.
        return render_expanded_unit(
            language,
            &parsed.code,
            &options,
            visibility,
            &expansions,
            scope,
            presenter_enforces_warning(cell, engine),
            fig,
        )
        .map(|md| (md, rejected));
    }
    let (info, lines) = super::knitr_inputs::original_chunk(language, attr_tail, author_text);
    let inputs = CellInputs::from_cell(cell, engine, (&info, &lines));
    let header = inputs
        .resolved_options
        .is_some()
        .then(|| knitr_header_options(parsed.options))
        .flatten();
    // `output-interleave` from the Quarto options (document scope under the cell\'s `#|` map),
    // never from a knitr record\'s resolved options: R does not record the scope (D3).
    let interleave = options
        .output_interleave()
        .unwrap_or_else(|| engine.is_some_and(|e| e.interleaves_output));
    let markdown = present_with_inputs(
        language,
        &parsed.code,
        cell,
        &inputs,
        header.as_ref(),
        (options, visibility),
        interleave,
        fig,
    );
    Ok((markdown, rejected))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presenter::figures::FsFigureSink;
    use crate::record::{
        AuthorTexts, CellMetadata, CellType, EngineEntryInput, HashDefinition, HashProfile,
        RecordBuilder, UnitKind,
    };
    use serde_json::{Value, json};
    use std::collections::BTreeMap;

    // === OPR-2 (D2): who enforces `warning` and `message` ===

    /// Jupyter\'s declaration: `warning` is display-only, so the presenter enforces it.
    fn jupyter_like() -> HashDefinition {
        let mut def = HashDefinition::conservative("jupyter");
        def.presentation_cell_options = vec!["echo".into(), "output".into(), "warning".into()];
        def
    }

    /// knitr\'s: the structured profile lists presentation options, never `warning`.
    fn knitr_like() -> HashDefinition {
        let mut def = HashDefinition::conservative("knitr");
        def.profiles.insert(
            "structured".into(),
            HashProfile {
                presentation_cell_options: vec!["echo".into(), "results".into()],
                presentation_execute_keys: vec![],
            },
        );
        def
    }

    fn entry(def: &HashDefinition) -> EngineEntry {
        RecordBuilder::new(&def.id, def, &[], &AuthorTexts::default())
            .unwrap()
            .finish(EngineEntryInput::default())
            .unwrap()
            .engine
    }

    /// A cell with a stdout line and a stderr line; `resolved` is the engine\'s recorded options.
    fn cell(def: &HashDefinition, resolved: Option<Value>) -> RecordCell {
        let mut extra: BTreeMap<String, Value> = BTreeMap::new();
        if let Some(r) = resolved {
            extra.insert("quarto_resolved_options".into(), r);
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
                quarto_lang: "python".into(),
                quarto_engine: Some(def.id.clone()),
                quarto_kernel: None,
                quarto_location: None,
                quarto_exec_hash: "h".into(),
                quarto_hash_rule: def.profiles.keys().next().cloned(),
                extra,
            },
            outputs: vec![
                json!({"output_type": "stream", "name": "stdout", "text": "fine\n"}),
                json!({"output_type": "stream", "name": "stderr", "text": "careful\n"}),
            ],
            source: vec!["x".into()],
        }
    }

    fn present(
        options: &str,
        cell: &RecordCell,
        engine: Option<&EngineEntry>,
        scope: Option<&ConfigValue>,
    ) -> String {
        present_with_reasons(options, cell, engine, scope).0
    }

    fn present_with_reasons(
        options: &str,
        cell: &RecordCell,
        engine: Option<&EngineEntry>,
        scope: Option<&ConfigValue>,
    ) -> (String, Vec<String>) {
        present_for("html", options, cell, engine, scope)
    }

    fn present_for(
        format: &str,
        options: &str,
        cell: &RecordCell,
        engine: Option<&EngineEntry>,
        scope: Option<&ConfigValue>,
    ) -> (String, Vec<String>) {
        let dir = tempfile::tempdir().unwrap();
        let mut fig = FsFigureSink::new(&dir.path().join("doc.qmd"));
        let author = format!("{options}\nx\n");
        let target = super::super::inline::InlineTarget::from_format(format);
        present_cell_step("python", "", &author, cell, engine, scope, &mut fig, target).unwrap()
    }

    // === OPR-7j: a group on the fallback path ===

    #[test]
    fn a_chunk_without_resolved_options_renders_the_group_on_the_generic_path() {
        // No `quarto_resolved_options`, so no knitr `figure_count`: the cell takes the generic
        // path (the same one Julia and Jupyter take) and the plot count makes it a group.
        const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
        let def = knitr_like();
        let mut c = cell(&def, None);
        c.outputs = (0..2)
            .map(|_| json!({"output_type": "display_data", "data": {"image/png": PNG}, "metadata": {}}))
            .collect();
        let md = present(
            "#| label: fig-g\n#| fig-cap: A group\n#| fig-subcap:\n#|   - First\n#|   - Second",
            &c,
            Some(&entry(&def)),
            None,
        );
        assert!(md.contains("#fig-g .cell"), "{md}");
        assert!(
            md.contains("{#fig-g-1}") && md.contains("{#fig-g-2}"),
            "{md}"
        );
        assert!(md.trim_end().ends_with("\nA group\n:::"), "{md}");
    }

    // === OPR-7f: the cell\'s `quarto_attrs` reach the wrapper; refusals come back ===

    #[test]
    fn a_cell_level_quarto_attrs_reaches_the_wrapper() {
        let def = jupyter_like();
        let (e, mut c) = (entry(&def), cell(&def, None));
        c.metadata.extra.insert(
            "quarto_attrs".into(),
            json!({"id": "cell-0", "attributes": {"execution_count": "1"}}),
        );
        let (md, reasons) = present_with_reasons("", &c, Some(&e), None);
        assert!(
            md.contains("::: {#cell-0 .cell execution_count='1'}"),
            "{md}"
        );
        assert!(reasons.is_empty(), "{reasons:?}");
    }

    // === OPR-9b: the engine's auto cell id goes on the wrapper only for html-family targets ===

    fn cell_with_auto_id(def: &HashDefinition) -> RecordCell {
        let mut c = cell(def, None);
        c.metadata.extra.insert(
            "quarto_attrs".into(),
            json!({"id": "cell-1", "attributes": {"execution_count": "1"}}),
        );
        c
    }

    #[test]
    fn the_auto_cell_id_stays_on_the_wrapper_for_html_family_targets() {
        let def = jupyter_like();
        let (e, c) = (entry(&def), cell_with_auto_id(&def));
        for format in ["html", "revealjs", "dashboard", "epub"] {
            let (md, _) = present_for(format, "", &c, Some(&e), None);
            assert!(
                md.contains("::: {#cell-1 .cell execution_count='1'}"),
                "{format}: {md}"
            );
        }
    }

    #[test]
    fn the_auto_cell_id_is_dropped_on_other_targets_and_the_rest_of_the_attrs_stay() {
        let def = jupyter_like();
        let (e, c) = (entry(&def), cell_with_auto_id(&def));
        for format in ["gfm", "markdown", "docx", "pptx", "typst"] {
            let (md, _) = present_for(format, "", &c, Some(&e), None);
            assert!(
                md.contains("::: {.cell execution_count='1'}"),
                "{format}: {md}"
            );
            assert!(!md.contains("#cell-1"), "{format}: {md}");
        }
    }

    #[test]
    fn a_labeled_container_keeps_its_label_id_on_every_target() {
        // A figure group: the label labels the `.cell` container (OPR-7j), engine id or not.
        const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
        let def = knitr_like();
        let e = entry(&def);
        let mut c = cell_with_auto_id(&def);
        c.outputs = (0..2)
            .map(|_| json!({"output_type": "display_data", "data": {"image/png": PNG}, "metadata": {}}))
            .collect();
        for format in ["html", "gfm", "docx", "typst"] {
            let (md, _) = present_for(
                format,
                "#| label: fig-g\n#| fig-cap: A group\n#| fig-subcap:\n#|   - First\n#|   - Second",
                &c,
                Some(&e),
                None,
            );
            assert!(md.contains("#fig-g .cell"), "{format}: {md}");
        }
    }

    #[test]
    fn refused_cell_and_output_entries_are_returned_and_the_rest_renders() {
        let def = jupyter_like();
        let (e, mut c) = (entry(&def), cell(&def, None));
        c.metadata.extra.insert(
            "quarto_attrs".into(),
            json!({"id": "cell-0", "classes": ["9bad"]}),
        );
        c.outputs.push(json!({
            "output_type": "execute_result",
            "execution_count": 1,
            "data": {"text/plain": "1"},
            "metadata": {"quarto_attrs": {"attributes": {"bad key": "v"}}}
        }));
        let (md, reasons) = present_with_reasons("", &c, Some(&e), None);
        assert!(md.contains("#cell-0"), "{md}");
        assert_eq!(reasons.len(), 2, "{reasons:?}");
        assert!(reasons[0].contains("9bad"), "{reasons:?}");
        assert!(reasons[1].contains("bad key"), "{reasons:?}");
    }

    #[test]
    fn no_quarto_attrs_means_no_reasons() {
        let def = jupyter_like();
        let (e, c) = (entry(&def), cell(&def, None));
        assert!(present_with_reasons("", &c, Some(&e), None).1.is_empty());
    }

    #[test]
    fn warning_false_is_enforced_once_when_the_engine_lists_it_display_only() {
        let def = jupyter_like();
        let (e, c) = (entry(&def), cell(&def, None));
        let md = present("#| warning: false", &c, Some(&e), None);
        assert!(md.contains("fine") && !md.contains("careful"), "{md}");
    }

    #[test]
    fn warning_false_is_not_filtered_again_when_capture_enforced_it() {
        // knitr-style: warning is in the hash, R recorded no warning, and the stderr that is
        // left is a message the author kept.
        let def = knitr_like();
        let (e, c) = (entry(&def), cell(&def, None));
        let md = present("#| warning: false", &c, Some(&e), None);
        assert!(md.contains("careful"), "{md}");
    }

    #[test]
    fn a_cell_with_resolved_options_follows_the_same_rule() {
        let jup = jupyter_like();
        let (e, c) = (entry(&jup), cell(&jup, Some(json!({"warning": false}))));
        let md = present("", &c, Some(&e), None);
        assert!(
            !md.contains("careful"),
            "display-only: presenter enforces: {md}"
        );
        let knit = knitr_like();
        let (e, c) = (entry(&knit), cell(&knit, Some(json!({"warning": false}))));
        let md = present("", &c, Some(&e), None);
        assert!(
            md.contains("careful"),
            "in the hash: capture enforced: {md}"
        );
    }

    #[test]
    fn an_undeclared_engine_gets_presenter_enforcement() {
        let def = HashDefinition::conservative("some-ts-engine");
        let e = entry(&def);
        for resolved in [None, Some(json!({"warning": false}))] {
            let c = cell(&def, resolved.clone());
            let md = present("#| warning: false", &c, Some(&e), None);
            assert!(!md.contains("careful"), "{resolved:?}: {md}");
        }
    }

    #[test]
    fn an_imported_record_with_no_engine_entry_gets_presenter_enforcement() {
        let def = knitr_like();
        for resolved in [None, Some(json!({"warning": false}))] {
            let c = cell(&def, resolved.clone());
            let md = present("#| warning: false", &c, None, None);
            assert!(!md.contains("careful"), "{resolved:?}: {md}");
        }
    }

    #[test]
    fn the_document_scope_warning_follows_the_rule_too() {
        let scope =
            crate::cell_options::options_to_config(quarto_yaml::parse("warning: false").unwrap()).0;
        let jup = jupyter_like();
        let (e, c) = (entry(&jup), cell(&jup, None));
        assert!(!present("", &c, Some(&e), Some(&scope)).contains("careful"));
        let knit = knitr_like();
        let (e, c) = (entry(&knit), cell(&knit, None));
        assert!(present("", &c, Some(&e), Some(&scope)).contains("careful"));
    }

    #[test]
    fn message_false_changes_nothing_for_any_engine() {
        for def in [
            jupyter_like(),
            knitr_like(),
            HashDefinition::conservative("x"),
        ] {
            let (e, c) = (entry(&def), cell(&def, None));
            let md = present("#| message: false", &c, Some(&e), None);
            assert!(
                md.contains("fine") && md.contains("careful"),
                "{}: {md}",
                def.id
            );
        }
    }

    // === output-interleave (2026-11-13-output-interleave.md) ===

    const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

    /// An entry whose engine declares (or not) that it interleaves by default.
    fn entry_with(def: &HashDefinition, interleaves: bool, known: Option<&[&str]>) -> EngineEntry {
        RecordBuilder::new(&def.id, def, &[], &AuthorTexts::default())
            .unwrap()
            .finish(EngineEntryInput {
                interleaves_output: interleaves,
                known_options: known.map(|k| k.iter().map(|s| s.to_string()).collect()),
                ..Default::default()
            })
            .unwrap()
            .engine
    }

    /// A two-line cell (`a`, `b`) with capture facts: stdout `one` and a message after line 1,
    /// stdout `two` after line 2. `resolved` makes it a knitr record.
    fn anchored(def: &HashDefinition, resolved: Option<Value>) -> RecordCell {
        let mut c = cell(def, resolved);
        c.outputs = vec![
            json!({"output_type": "stream", "name": "stdout", "text": "one\n"}),
            json!({"output_type": "stream", "name": "stderr", "text": "careful\n"}),
            json!({"output_type": "stream", "name": "stdout", "text": "two\n"}),
        ];
        let extra = &mut c.metadata.extra;
        extra.insert("quarto_output_anchors".into(), json!([1, 1, 2]));
        extra.insert("quarto_output_kinds".into(), json!([null, "message", null]));
        extra.insert("quarto_expression_starts".into(), json!([0, 1]));
        c
    }

    /// Present `cell` with `options` (the `#|` lines) over the code `a`, `b`.
    fn present_ab(
        options: &str,
        cell: &RecordCell,
        engine: Option<&EngineEntry>,
        scope: Option<&ConfigValue>,
    ) -> String {
        let dir = tempfile::tempdir().unwrap();
        let mut fig = FsFigureSink::new(&dir.path().join("doc.qmd"));
        let author = if options.is_empty() {
            "a\nb\n".to_string()
        } else {
            format!("{options}\na\nb\n")
        };
        let target = super::super::inline::InlineTarget::from_format("html");
        present_cell_step("python", "", &author, cell, engine, scope, &mut fig, target)
            .unwrap()
            .0
    }

    /// Whole: one source fence holding both lines, every output after it.
    fn is_held(md: &str) -> bool {
        md.contains("```{.python .cell-code}\na\nb\n```")
            && md.find("a\nb").unwrap() < md.find("one").unwrap()
    }

    /// Interleaved: `a` in its own fence, its outputs, then `b` in its own fence.
    fn is_interleaved(md: &str) -> bool {
        md.contains("```{.python .cell-code}\na\n```")
            && md.contains("```{.python .cell-code}\nb\n```")
            && md.find("one").unwrap() < md.find("\nb\n").unwrap()
    }

    fn scope(yaml: &str) -> ConfigValue {
        crate::cell_options::options_to_config(quarto_yaml::parse(yaml).unwrap()).0
    }

    #[test]
    fn a_cell_with_anchors_renders_held_when_its_engine_does_not_interleave() {
        // Julia-shaped: capture facts, no resolved options, an engine that declares nothing.
        let def = jupyter_like();
        let md = present_ab(
            "",
            &anchored(&def, None),
            Some(&entry_with(&def, false, None)),
            None,
        );
        assert!(is_held(&md), "{md}");
    }

    #[test]
    fn a_cell_with_anchors_and_no_engine_entry_renders_held() {
        let def = knitr_like();
        let md = present_ab("", &anchored(&def, Some(json!({}))), None, None);
        assert!(is_held(&md), "{md}");
    }

    #[test]
    fn output_interleave_true_on_the_cell_interleaves() {
        let def = jupyter_like();
        let e = entry_with(&def, false, None);
        let md = present_ab(
            "#| output-interleave: true",
            &anchored(&def, None),
            Some(&e),
            None,
        );
        assert!(is_interleaved(&md), "{md}");
    }

    #[test]
    fn output_interleave_in_the_scope_interleaves_and_the_cell_beats_it() {
        let def = jupyter_like();
        let (e, c) = (entry_with(&def, false, None), anchored(&def, None));
        let on = scope("output-interleave: true");
        assert!(is_interleaved(&present_ab("", &c, Some(&e), Some(&on))));
        let md = present_ab("#| output-interleave: false", &c, Some(&e), Some(&on));
        assert!(is_held(&md), "{md}");
    }

    #[test]
    fn results_hold_holds_printed_text_on_any_interleaved_cell() {
        // Not a knitr record: the hold used to apply only with resolved options.
        let def = jupyter_like();
        let e = entry_with(&def, false, None);
        let md = present_ab(
            "#| output-interleave: true\n#| results: hold",
            &anchored(&def, None),
            Some(&e),
            None,
        );
        // The message stays after `a`; the printed text is one stream after `b`.
        assert!(md.contains("```{.python .cell-code}\na\n```"), "{md}");
        assert!(
            md.find("careful").unwrap() < md.find("\nb\n").unwrap(),
            "{md}"
        );
        assert!(md.contains("one\ntwo"), "{md}");
        assert!(md.find("\nb\n").unwrap() < md.find("one").unwrap(), "{md}");
    }

    #[test]
    fn fig_show_hold_holds_plots_on_any_interleaved_cell() {
        let def = jupyter_like();
        let e = entry_with(&def, false, None);
        let mut c = anchored(&def, None);
        c.outputs[0] =
            json!({"output_type": "display_data", "data": {"image/png": PNG}, "metadata": {}});
        let md = present_ab(
            "#| output-interleave: true\n#| fig.show: hold",
            &c,
            Some(&e),
            None,
        );
        // The plot moves after `two`, the last output.
        assert!(md.find("two").unwrap() < md.find(".png").unwrap(), "{md}");
    }

    #[test]
    fn a_knitr_record_interleaves_by_default_and_output_interleave_false_holds_it() {
        let def = knitr_like();
        let e = entry_with(&def, true, None);
        let c = anchored(&def, Some(json!({})));
        assert!(is_interleaved(&present_ab("", &c, Some(&e), None)));
        let md = present_ab("#| output-interleave: false", &c, Some(&e), None);
        assert!(is_held(&md), "cell: {md}");
        // The document scope reaches a knitr cell too, although R does not record it.
        let off = scope("output-interleave: false");
        let md = present_ab("", &c, Some(&e), Some(&off));
        assert!(is_held(&md), "scope: {md}");
    }

    #[test]
    fn output_interleave_is_never_a_cell_attribute() {
        // Jupyter-shaped: forwarding of unhandled cell options.
        let def = jupyter_like();
        let e = entry_with(&def, false, None);
        let md = present_ab(
            "#| output-interleave: true",
            &anchored(&def, None),
            Some(&e),
            None,
        );
        assert!(!md.contains("output-interleave"), "{md}");
        // knitr: an option outside `known_options` is forwarded unless the reader handles it.
        let def = knitr_like();
        let e = entry_with(&def, true, Some(&["results"]));
        let md = present_ab(
            "#| output-interleave: false",
            &anchored(&def, Some(json!({}))),
            Some(&e),
            None,
        );
        assert!(!md.contains("output-interleave"), "{md}");
        assert!(!md.contains("output.interleave"), "{md}");
    }

    #[test]
    fn the_engine_declaration_round_trips_and_defaults_to_holding() {
        let def = knitr_like();
        let json = serde_json::to_value(entry_with(&def, true, None)).unwrap();
        assert_eq!(json["interleaves_output"], json!(true));
        let held = serde_json::to_value(entry_with(&def, false, None)).unwrap();
        assert!(held.get("interleaves_output").is_none(), "{held}");
        let back: EngineEntry = serde_json::from_value(held).unwrap();
        assert!(!back.interleaves_output);
    }

    #[test]
    fn echo_output_and_include_stay_presenter_enforced_for_every_declaration() {
        for def in [
            jupyter_like(),
            knitr_like(),
            HashDefinition::conservative("x"),
        ] {
            let e = entry(&def);
            let c = cell(&def, None);
            let md = present("#| output: false", &c, Some(&e), None);
            assert!(
                !md.contains("fine") && !md.contains("careful"),
                "{}: {md}",
                def.id
            );
            let c = cell(&def, Some(json!({"include": false})));
            assert_eq!(present("", &c, Some(&e), None).trim(), "", "{}", def.id);
        }
    }
}
