/*
 * stage/stages/record_splice.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * The record arm of the capture splice (EXR-14).
 */

//! Splice a stored execution record into the live pre-engine AST, matching units by hash.
//!
//! The legacy structural splice parses a recorded input and result and walks
//! them in parallel. This arm does not: it works the way the engine stage does, so the two
//! agree on units, ids and hashes by construction.
//!
//! 1. Mask a clone of the AST and serialize it (the text the writer scanned).
//! 2. Scan once and fill every unit's id from the author text snapshotted before crossref
//!    sugaring (`StageContext::sugared_blocks`).
//! 3. Key each unit with the writer's hashes, under the `HashDefinition` of the record engine
//!    that owns its language, and match it to a record cell ([`match_units`]).
//! 4. Present each matched cell with the **current** cell's options (a stored markdown cell
//!    is placed as it is), substitute it into the text, unmask the text, and reparse and
//!    reconcile it into the AST ([`merge_markdown_into_ast`]).
//!
//! Fail-soft throughout: a unit nothing matches, or that cannot be presented, stays as source.
//! This module never imports the legacy splice module, so that module can be deleted without
//! touching it.

use std::path::Path;

use quarto_source_map::{FileId, SourceInfo};
use serde_json::Value;

use crate::cell_options::partition_cell_options;
use crate::engine::units::Unit;
use crate::presenter::cell::render_cell_with;
use crate::presenter::expansion::{render_expanded_unit, split_expansions};
use crate::presenter::figures::{FigureSink, RuntimeFigureSink};
use crate::presenter::inline::{InlineTarget, render_inline};
use crate::presenter::options::CellOptions;
use crate::presenter::output::{CellOutput, CellResult};
use crate::presenter::visibility::{CellVisibility, resolve_cell_options};
use crate::record::{
    EngineEntry, Record, UnitKey, UnitKind, UnitLocation, UnitStaleness, match_units, replacement,
    stored_markdown,
};
use crate::stage::data::DocumentAst;
use crate::stage::stages::engine_execution::{
    merge_execute_scope, merge_markdown_into_ast, scan_and_fill_ids, serialize_ast_to_qmd,
};
use crate::stage::{EventLevel, StageContext};
use crate::trace_event;

/// What the splice decided for every unit and record cell, for later UI and for EXR-15.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SpliceReport {
    /// One entry per document unit, in document order: its id and its staleness.
    pub units: Vec<(String, UnitStaleness)>,
    /// Record cells no document unit matched (their `quarto_cell_id`s), in record order.
    pub orphaned: Vec<String>,
}

impl SpliceReport {
    /// The payload of the [`RECORD_STALENESS_KIND`](super::RECORD_STALENESS_KIND) aux event.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "units": self.units.iter().map(|(id, s)| serde_json::json!({
                "id": id,
                "staleness": match s {
                    UnitStaleness::Fresh => "fresh",
                    UnitStaleness::Stale => "stale",
                },
            })).collect::<Vec<_>>(),
            "orphaned": self.orphaned,
        })
    }
}

/// The record engine that owns `lang`, by its recorded owned-language set.
fn engine_for<'a>(record: &'a Record, lang: &str) -> Option<&'a EngineEntry> {
    record.metadata.quarto_engines.iter().find(|e| {
        e.owned_languages
            .iter()
            .any(|l| l.eq_ignore_ascii_case(lang))
    })
}

fn unit_id(unit: &Unit) -> String {
    match unit {
        Unit::Cell(c) => c.id.as_ref(),
        Unit::Inline(i) => i.id.as_ref(),
    }
    .map(|id| id.0.clone())
    .unwrap_or_default()
}

/// Splice `record` into `doc`. `doc_dir` is where figure and supporting files are written.
pub(crate) fn splice_record(
    doc: DocumentAst,
    record: &Record,
    ctx: &mut StageContext,
    doc_dir: &Path,
) -> (DocumentAst, SpliceReport) {
    let (mut doc, report) = splice_units(doc, record, ctx, doc_dir);
    // After the merge: reconciling against the reparsed text takes its meta, which would drop
    // anything folded earlier. Includes must reach the pane even when no unit splices.
    fold_includes(record, &mut doc);
    fold_warnings(record, ctx);
    (doc, report)
}

fn splice_units(
    mut doc: DocumentAst,
    record: &Record,
    ctx: &mut StageContext,
    doc_dir: &Path,
) -> (DocumentAst, SpliceReport) {
    let format = ctx.format.identifier.to_string();

    // 1. The text the writer scanned: the masked, serialized AST.
    let mut masked = doc.ast.clone();
    crate::engine::nested_cell_mask::mask_document(&mut masked);
    let Ok((qmd, _)) = serialize_ast_to_qmd(&masked) else {
        trace_event!(
            ctx,
            EventLevel::Warn,
            "record-splice: cannot serialize the document; leaving it as source"
        );
        return (doc, SpliceReport::default());
    };

    // 2. The one scan, with ids.
    let (units, texts) = scan_and_fill_ids(&qmd, &ctx.sugared_blocks);

    // 3. Match.
    let keys: Vec<UnitKey> = units
        .iter()
        .zip(&texts.0)
        .map(|(unit, text)| {
            let kind = match unit {
                Unit::Cell(_) => UnitKind::Cell,
                Unit::Inline(_) => UnitKind::Inline,
            };
            engine_for(record, unit.lang())
                .and_then(|e| {
                    let tail = match unit {
                        Unit::Cell(c) => c.attr_tail.as_str(),
                        Unit::Inline(_) => "",
                    };
                    UnitKey::from_author_text(
                        &e.hash_definition,
                        kind,
                        unit.lang(),
                        &text.text,
                        tail,
                    )
                    .ok()
                })
                // No owning engine, or options that do not parse: the unit can match by
                // content hash only.
                .unwrap_or_else(|| UnitKey {
                    kind,
                    exec_hash: String::new(),
                    content_hash: text.content_hash.clone(),
                })
        })
        .collect();
    let matching = match_units(&keys, &record.cells);
    let report = SpliceReport {
        units: units
            .iter()
            .enumerate()
            .map(|(i, u)| (unit_id(u), matching.staleness(i)))
            .collect(),
        orphaned: matching
            .orphaned
            .iter()
            .map(|i| record.cells[*i].metadata.quarto_cell_id.clone())
            .collect(),
    };

    // Supporting files first: they must reach the pane even if no unit splices.
    materialize_files(record, ctx, doc_dir);

    // 4. Present and substitute.
    let doc_scope =
        merge_execute_scope(ctx.format.identifier, doc.ast.meta.get("execute").cloned());
    let mut fig: Box<dyn FigureSink> = Box::new(RuntimeFigureSink::new(
        ctx.runtime.clone(),
        &doc_dir.join(doc.path.file_name().unwrap_or_default()),
    ));
    let target = InlineTarget::from_format(&format);
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for (i, unit) in units.iter().enumerate() {
        let Some(cell_index) = matching.assignment[i] else {
            continue;
        };
        let cell = &record.cells[cell_index];
        if matches!(unit, Unit::Inline(u) if u.in_front_matter)
            || cell.metadata.quarto_location == Some(UnitLocation::Metadata)
        {
            continue;
        }
        let markdown = match unit {
            Unit::Cell(_) => {
                fig.begin_cell();
                present_cell(
                    unit,
                    &texts.0[i].text,
                    cell,
                    doc_scope.as_ref(),
                    fig.as_mut(),
                    &format,
                )
            }
            Unit::Inline(_) => present_span(cell, target),
        };
        if let Some(markdown) = markdown {
            edits.push(replacement(unit, &qmd, &markdown));
        }
    }
    if edits.is_empty() {
        return (doc, report);
    }

    let mut text = qmd;
    edits.sort_by_key(|e| e.0);
    for (start, end, replaced) in edits.into_iter().rev() {
        text.replace_range(start..end, &replaced);
    }
    let text = crate::engine::nested_cell_mask::unmask(&text);

    let intermediate = format!(
        "{}.record.rmarkdown",
        doc.path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    );
    let DocumentAst {
        path,
        ast,
        mut ast_context,
        source_context,
        mut warnings,
        recorded_includes,
        derived_listing,
    } = doc;
    let fallback = ast.clone();
    match merge_markdown_into_ast(ast, &mut ast_context, &text, intermediate) {
        Ok((merged, parse_warnings, _plan)) => {
            warnings.extend(parse_warnings);
            doc = DocumentAst {
                path,
                ast: merged,
                ast_context,
                source_context,
                warnings,
                recorded_includes,
                derived_listing,
            };
        }
        Err(_) => {
            trace_event!(
                ctx,
                EventLevel::Warn,
                "record-splice: the presented record did not parse; leaving the document as source"
            );
            doc = DocumentAst {
                path,
                ast: fallback,
                ast_context,
                source_context,
                warnings,
                recorded_includes,
                derived_listing,
            };
        }
    }
    (doc, report)
}

/// A record cell's stored nbformat outputs as the kernel result shape [`render_cell_with`] consumes.
fn stored_result_for_cell(cell: &Value) -> CellResult {
    let outputs = cell
        .get("outputs")
        .and_then(|v| v.as_array())
        .map_or(&[][..], Vec::as_slice);
    CellResult::from_nbformat(
        outputs,
        cell.get("execution_count").and_then(|v| v.as_u64()),
    )
}

/// A fenced cell: stored markdown is placed as it is; anything else is presented with the
/// current cell's options layered over the document scope.
fn present_cell(
    unit: &Unit,
    author_text: &str,
    cell: &crate::record::RecordCell,
    doc_scope: Option<&quarto_pandoc_types::ConfigValue>,
    fig: &mut dyn FigureSink,
    format: &str,
) -> Option<String> {
    if let Some((markdown, stored_format)) = stored_markdown(cell) {
        return (stored_format.as_deref() == Some(format)).then_some(markdown);
    }
    let lang = unit.lang();
    let body_source = SourceInfo::original(FileId(0), 0, author_text.len());
    let partitioned = partition_cell_options(lang, author_text, body_source).ok()?;
    let resolved = resolve_cell_options(doc_scope, partitioned.options);
    let visibility = CellVisibility::resolve(resolved.as_ref());
    let options = CellOptions::from_config(resolved.as_ref());
    if let Some(expansions) = split_expansions(&cell.outputs) {
        // An expanded cell: the author's cell, then one cell per expansion, each with its
        // own options.
        return render_expanded_unit(
            lang,
            &partitioned.code,
            &options,
            visibility,
            &expansions,
            doc_scope,
            fig,
        )
        .ok();
    }
    let result = stored_result_for_cell(&serde_json::to_value(cell).ok()?);
    Some(render_cell_with(
        lang,
        &partitioned.code,
        &result,
        &options,
        fig,
        visibility,
    ))
}

/// A span: its first output that carries data, as an inline value. `None` keeps the source.
fn present_span(cell: &crate::record::RecordCell, target: InlineTarget) -> Option<String> {
    let result = stored_result_for_cell(&serde_json::to_value(cell).ok()?);
    let bundle = result.outputs.iter().find_map(|o| match o {
        CellOutput::ExecuteResult { data, .. } | CellOutput::DisplayData { data, .. } => Some(data),
        _ => None,
    })?;
    render_inline(bundle, target).filter(|s| !s.is_empty())
}

/// Write the record's embedded files beside the document.
fn materialize_files(record: &Record, ctx: &StageContext, doc_dir: &Path) {
    let files: Vec<quarto_trace::CaptureFile> = record
        .metadata
        .quarto_files
        .iter()
        .map(|(path, file)| quarto_trace::CaptureFile {
            path: path.clone(),
            contents_base64: file.data.clone(),
        })
        .collect();
    if !files.is_empty() {
        crate::engine::capture_files::materialize_capture_files(
            ctx.runtime.as_ref(),
            doc_dir,
            &files,
        );
    }
}

/// Fold each engine entry's includes into `meta.rendered.includes.*`, the preview channel.
fn fold_includes(record: &Record, doc: &mut DocumentAst) {
    for entry in &record.metadata.quarto_engines {
        if let Some(includes) = &entry.includes {
            super::capture_splice::fold_includes_value_into_meta(&mut doc.ast.meta, includes);
        }
    }
}

/// Raise each engine entry\'s recorded warnings into the stage diagnostics, as the engine
/// stage does for a live run (replay parity, bd-cp3em). A stored warning that does not parse
/// as a diagnostic is skipped.
fn fold_warnings(record: &Record, ctx: &mut StageContext) {
    for entry in &record.metadata.quarto_engines {
        for stored in &entry.warnings {
            match serde_json::from_value::<quarto_error_reporting::DiagnosticMessage>(
                stored.clone(),
            ) {
                Ok(diagnostic) => ctx.add_diagnostic(diagnostic),
                Err(_) => trace_event!(
                    ctx,
                    EventLevel::Warn,
                    "record-splice: skipping a recorded warning that is not a diagnostic"
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use serde_json::{Value, json};

    use super::*;
    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectContext};
    use crate::record::{
        AuthorTexts, EngineEntryInput, HashDefinition, RecordBuilder, UnitResult, coalesce,
    };

    const PATH: &str = "/project/test.qmd";

    fn parse(qmd: &str) -> DocumentAst {
        let mut source_context = quarto_source_map::SourceContext::new();
        source_context.add_file(PATH.to_string(), Some(qmd.to_string()));
        let (ast, ast_context, warnings) = pampa::readers::qmd::read(
            qmd.as_bytes(),
            false,
            PATH,
            &mut std::io::sink(),
            true,
            None,
        )
        .expect("test qmd parses");
        DocumentAst {
            path: PathBuf::from(PATH),
            ast,
            ast_context,
            source_context,
            warnings,
            recorded_includes: Vec::new(),
            derived_listing: Default::default(),
        }
    }

    fn ctx() -> StageContext {
        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            is_single_file: true,
            output_dir: PathBuf::from("/project"),
            ..Default::default()
        };
        StageContext::new(
            Arc::new(quarto_system_runtime::NativeRuntime::new()),
            Format::html(),
            project,
            DocumentInfo::from_path(PATH),
        )
        .unwrap()
    }

    fn text_of(doc: &DocumentAst) -> String {
        let (bytes, _) = pampa::writers::qmd::write_with_source_info(&doc.ast).unwrap();
        String::from_utf8(bytes).unwrap()
    }

    fn stdout(text: &str) -> UnitResult {
        UnitResult {
            outputs: vec![json!({"output_type": "stream", "name": "stdout", "text": [text]})],
            execution_count: Some(1),
            kernel: None,
        }
    }

    fn value(text: &str) -> UnitResult {
        UnitResult {
            outputs: vec![json!({
                "output_type": "execute_result",
                "execution_count": 1,
                "data": {"text/plain": text},
                "metadata": {}
            })],
            execution_count: Some(1),
            kernel: None,
        }
    }

    /// Record `source` the way the writer does: scan the serialized AST, fill ids, and give
    /// unit `i` the result `result_for(i)`.
    fn record_of(source: &str, result_for: impl Fn(usize) -> Option<UnitResult>) -> Record {
        let doc = parse(source);
        let (qmd, _) = serialize_ast_to_qmd(&doc.ast).unwrap();
        let (units, texts): (_, AuthorTexts) = scan_and_fill_ids(&qmd, &[]);
        let def = HashDefinition::conservative("jupyter");
        let mut builder = RecordBuilder::new("jupyter", &def, &units, &texts).unwrap();
        for i in 0..units.len() {
            if let Some(result) = result_for(i) {
                builder.push(i, result).unwrap();
            }
        }
        let entry = builder
            .finish(EngineEntryInput {
                owned_languages: vec!["python".into()],
                ..Default::default()
            })
            .unwrap();
        coalesce(&[entry], &units).unwrap()
    }

    fn splice(source: &str, record: &Record) -> (String, SpliceReport) {
        let (doc, report) = splice_record(parse(source), record, &mut ctx(), Path::new("/project"));
        (text_of(&doc), report)
    }

    /// Replay parity (bd-cp3em): the live engine stage re-raises an engine\'s warnings into the
    /// stage diagnostics; a record carries them, so the splice does too.
    #[test]
    fn recorded_engine_warnings_are_raised_as_diagnostics() {
        use quarto_error_reporting::DiagnosticMessageBuilder;
        let mut record = three_cells();
        let warning = DiagnosticMessageBuilder::warning("engine warned")
            .with_code("Q-18-2")
            .build();
        record.metadata.quarto_engines[0]
            .warnings
            .push(serde_json::to_value(&warning).unwrap());
        // A stored warning that is not a diagnostic is skipped, never fatal.
        record.metadata.quarto_engines[0]
            .warnings
            .push(json!("not a diagnostic"));
        let mut ctx = ctx();
        let _ = splice_record(parse(THREE), &record, &mut ctx, Path::new("/project"));
        assert_eq!(ctx.diagnostics, vec![warning]);
    }

    #[test]
    fn a_record_without_warnings_raises_none() {
        let mut ctx = ctx();
        let _ = splice_record(
            parse(THREE),
            &three_cells(),
            &mut ctx,
            Path::new("/project"),
        );
        assert!(ctx.diagnostics.is_empty());
    }

    const THREE: &str =
        "```{python}\na = 1\n```\n\n```{python}\nb = 2\n```\n\n```{python}\nc = 3\n```\n";

    fn three_cells() -> Record {
        record_of(THREE, |i| Some(stdout(&format!("out{i}\n"))))
    }

    #[test]
    fn matched_cells_show_their_recorded_output() {
        let (text, report) = splice(THREE, &three_cells());
        for n in 0..3 {
            assert!(text.contains(&format!("out{n}")), "{text}");
        }
        assert!(!text.contains("{python}"), "{text}");
        assert!(report.units.iter().all(|(_, s)| *s == UnitStaleness::Fresh));
    }

    #[test]
    fn an_edited_cell_stays_as_source_and_the_rest_splice() {
        let edited = THREE.replace("b = 2", "b = 22");
        let (text, report) = splice(&edited, &three_cells());
        assert!(text.contains("out0") && text.contains("out2"), "{text}");
        assert!(!text.contains("out1"), "{text}");
        assert!(text.contains("b = 22"), "{text}");
        assert_eq!(report.units[1].1, UnitStaleness::Stale);
        assert_eq!(report.orphaned.len(), 1);
    }

    #[test]
    fn reordered_cells_all_splice() {
        let reordered =
            "```{python}\nc = 3\n```\n\n```{python}\na = 1\n```\n\n```{python}\nb = 2\n```\n";
        let (text, _) = splice(reordered, &three_cells());
        let (c, a, b) = (
            text.find("out2").unwrap(),
            text.find("out0").unwrap(),
            text.find("out1").unwrap(),
        );
        assert!(c < a && a < b, "{text}");
    }

    #[test]
    fn a_presentation_option_edit_splices_with_the_current_echo() {
        let hash_def = {
            let mut d = HashDefinition::conservative("jupyter");
            d.presentation_cell_options = vec!["echo".into()];
            d
        };
        // Record with a definition that excludes `echo` from the execution hash.
        let source = "```{python}\n#| echo: true\nx = 1\n```\n";
        let doc = parse(source);
        let (qmd, _) = serialize_ast_to_qmd(&doc.ast).unwrap();
        let (units, texts) = scan_and_fill_ids(&qmd, &[]);
        let mut b = RecordBuilder::new("jupyter", &hash_def, &units, &texts).unwrap();
        b.push(0, stdout("shown\n")).unwrap();
        let entry = b
            .finish(EngineEntryInput {
                owned_languages: vec!["python".into()],
                ..Default::default()
            })
            .unwrap();
        let record = coalesce(&[entry], &units).unwrap();

        let (with_echo, _) = splice(source, &record);
        assert!(with_echo.contains("cell-code"), "{with_echo}");

        let (without, report) = splice(&source.replace("echo: true", "echo: false"), &record);
        assert!(without.contains("shown"), "{without}");
        assert!(!without.contains("cell-code"), "{without}");
        assert_eq!(report.units[0].1, UnitStaleness::Fresh);
    }

    #[test]
    fn spans_splice_inline_values_and_edited_spans_stay_source() {
        let source = "A `{python} a` and `{python} a` and `{python} b`.\n";
        let record = record_of(source, |i| Some(value(&format!("v{i}"))));
        let (text, report) = splice(source, &record);
        assert!(
            text.contains("v0") && text.contains("v1") && text.contains("v2"),
            "{text}"
        );
        assert!(!text.contains("{python}"), "{text}");
        assert_eq!(report.units.len(), 3);

        let (text, report) = splice(
            &source.replace("and `{python} a` and", "and `{python} a2` and"),
            &record,
        );
        assert!(
            text.contains("v0") && text.contains("v2") && !text.contains("v1"),
            "{text}"
        );
        assert!(text.contains("{python} a2"), "{text}");
        assert_eq!(report.units[1].1, UnitStaleness::Stale);
    }

    #[test]
    fn a_nested_fence_in_a_display_block_is_not_a_unit() {
        let source = "````markdown\n```{python}\nx = 1\n```\n````\n\n```{python}\ny = 2\n```\n";
        let record = record_of(source, |_| Some(stdout("only\n")));
        assert_eq!(record.cells.len(), 1, "the writer saw one unit");
        let (text, report) = splice(source, &record);
        assert_eq!(report.units.len(), 1);
        assert!(text.contains("only"), "{text}");
        assert!(
            text.contains("```{python}\nx = 1"),
            "the display block is untouched: {text}"
        );
    }

    #[test]
    fn a_record_with_no_matching_unit_leaves_the_document_unchanged() {
        let record = record_of("```{python}\nzzz = 0\n```\n", |_| Some(stdout("nope\n")));
        let (text, report) = splice(THREE, &record);
        assert_eq!(text, text_of(&parse(THREE)));
        assert!(report.units.iter().all(|(_, s)| *s == UnitStaleness::Stale));
        assert_eq!(report.orphaned.len(), 1);
    }

    #[test]
    fn a_unit_of_a_language_the_record_does_not_own_stays_source() {
        let source = "```{python}\na = 1\n```\n\n```{r}\nb <- 2\n```\n";
        let record = record_of(source, |i| (i == 0).then(|| stdout("py\n")));
        let (text, _) = splice(source, &record);
        assert!(text.contains("```{r}"), "{text}");
    }

    /// `record_of` with a real sugaring snapshot, for labeled and captioned cells.
    fn record_with_snapshot(
        source_doc: &DocumentAst,
        snapshot: &[crate::record::SugaredBlock],
        result_for: impl Fn(usize) -> Option<UnitResult>,
    ) -> Record {
        let (qmd, _) = serialize_ast_to_qmd(&source_doc.ast).unwrap();
        let (units, texts): (_, AuthorTexts) = scan_and_fill_ids(&qmd, snapshot);
        let def = HashDefinition::conservative("jupyter");
        let mut builder = RecordBuilder::new("jupyter", &def, &units, &texts).unwrap();
        for i in 0..units.len() {
            if let Some(result) = result_for(i) {
                builder.push(i, result).unwrap();
            }
        }
        let entry = builder
            .finish(EngineEntryInput {
                owned_languages: vec!["python".into()],
                ..Default::default()
            })
            .unwrap();
        coalesce(&[entry], &units).unwrap()
    }

    /// Decision 36: a labeled, captioned Jupyter cell hashes the writer's range, not the
    /// sugared `CodeBlock.text`, so it still matches at splice time.
    #[tokio::test]
    async fn a_labeled_cell_hashes_the_writers_range() {
        use crate::stage::{PipelineData, PipelineStage, PreEngineSugaringStage};
        let source = "```{python}\n#| label: fig-x\n#| fig-cap: A figure\nx = 1\n```\n";
        let mut ctx = ctx();
        let PipelineData::DocumentAst(sugared) = PreEngineSugaringStage::new()
            .run(PipelineData::DocumentAst(parse(source)), &mut ctx)
            .await
            .expect("sugaring runs")
        else {
            panic!("sugaring returns a document");
        };
        assert_eq!(ctx.sugared_blocks.len(), 1, "{:?}", ctx.sugared_blocks);
        // The writer ran at exactly this point: sugared AST plus the snapshot.
        let record = record_with_snapshot(&sugared, &ctx.sugared_blocks, |_| Some(stdout("lab\n")));
        assert_eq!(record.cells.len(), 1);
        let (doc, report) = splice_record(sugared, &record, &mut ctx, Path::new("/project"));
        assert_eq!(report.units[0].1, UnitStaleness::Fresh, "{report:?}");
        assert!(text_of(&doc).contains("lab"), "{}", text_of(&doc));
    }

    #[test]
    fn cells_in_a_blockquote_and_a_list_item_match_by_hash() {
        let source = "> ```{python}\n> a = 1\n> ```\n\n- item\n\n  ```{python}\n  b = 2\n  ```\n\n```{python}\nc = 3\n```\n";
        let record = record_of(source, |i| Some(stdout(&format!("nest{i}\n"))));
        assert_eq!(record.cells.len(), 3);
        let (text, report) = splice(source, &record);
        for n in 0..3 {
            assert!(text.contains(&format!("nest{n}")), "{text}");
        }
        assert!(report.units.iter().all(|(_, s)| *s == UnitStaleness::Fresh));
        assert!(report.orphaned.is_empty());
    }

    #[test]
    fn a_front_matter_span_takes_an_occurrence_but_is_never_spliced() {
        let source = "---\ntitle: \"T `{python} a`\"\n---\n\nBody `{python} a`.\n";
        let record = record_of(source, |i| Some(value(&format!("v{i}"))));
        assert_eq!(
            record.cells.len(),
            2,
            "front matter and body are both units"
        );
        let (text, report) = splice(source, &record);
        assert_eq!(report.units.len(), 2);
        // Body is occurrence 1 and takes the second record value; the title keeps its source.
        assert!(text.contains("v1"), "{text}");
        assert!(!text.contains("v0"), "{text}");
        assert!(text.contains("T `{python} a`"), "{text}");
    }

    #[test]
    fn a_span_added_before_a_cell_leaves_the_cell_id_unchanged() {
        let cell = "```{python}\nx = 1\n```\n";
        let id_of = |source: &str| {
            let doc = parse(source);
            let (qmd, _) = serialize_ast_to_qmd(&doc.ast).unwrap();
            let (units, _) = scan_and_fill_ids(&qmd, &[]);
            units
                .iter()
                .find_map(|u| match u {
                    Unit::Cell(c) => c.id.as_ref().map(|i| i.0.clone()),
                    _ => None,
                })
                .unwrap()
        };
        let plain = id_of(cell);
        assert_eq!(plain, id_of(&format!("Span `{{python}} a`.\n\n{cell}")));
    }

    #[test]
    fn ceded_braced_cells_count_in_the_cell_index_and_raw_blocks_do_not() {
        let id_of_last = |source: &str| {
            let doc = parse(source);
            let (qmd, _) = serialize_ast_to_qmd(&doc.ast).unwrap();
            let (units, _) = scan_and_fill_ids(&qmd, &[]);
            units
                .iter()
                .rev()
                .find_map(|u| match u {
                    Unit::Cell(c) => c.id.as_ref().map(|i| i.0.clone()),
                    _ => None,
                })
                .unwrap()
        };
        let cell = "```{python}\nx = 1\n```\n";
        let base = id_of_last(cell);
        let after_ceded = id_of_last(&format!("```{{r}}\ny <- 1\n```\n\n{cell}"));
        assert_ne!(base, after_ceded, "a ceded braced cell takes an index");
        let after_marimo = id_of_last(&format!("```{{python.marimo}}\nz = 1\n```\n\n{cell}"));
        assert_ne!(base, after_marimo, "a marimo cell takes an index");
        let after_raw = id_of_last(&format!("```{{=html}}\n<hr/>\n```\n\n{cell}"));
        assert_eq!(base, after_raw, "a raw block takes none");
    }

    #[test]
    fn span_values_follow_the_value_kinds() {
        let source = "A `{python} a` end.\n";
        let splice_data = |data: Value| {
            let record = record_of(source, |_| {
                Some(UnitResult {
                    outputs: vec![json!({
                        "output_type": "execute_result",
                        "execution_count": 1,
                        "data": data.clone(),
                        "metadata": {}
                    })],
                    execution_count: Some(1),
                    kernel: None,
                })
            });
            splice(source, &record).0
        };
        // Q1 `asEscapedMarkdown`: unquoted plain text is markdown; a quoted value is escaped.
        let plain = splice_data(json!({"text/plain": "**plain**"}));
        assert!(
            plain.contains("**plain**"),
            "unquoted text is markdown: {plain}"
        );
        let quoted = splice_data(json!({"text/plain": "'a*b'"}));
        assert!(
            quoted.contains("a\\*b") && !quoted.contains('\''),
            "quoted text is escaped: {quoted}"
        );
        let broken = splice_data(json!({"text/plain": "x*y"}));
        assert_eq!(
            broken,
            text_of(&parse(source)),
            "unparseable markdown leaves the source"
        );
        let md = splice_data(json!({"text/markdown": "**bold**"}));
        assert!(md.contains("**bold**"), "markdown is parsed inline: {md}");
        let html = splice_data(json!({"text/html": "<b>h</b>"}));
        assert!(
            html.contains("`<b>h</b>`{=html}"),
            "HTML is a raw inline: {html}"
        );
    }

    #[test]
    fn embedded_files_materialize_and_includes_fold_into_meta() {
        let dir = tempfile::tempdir().unwrap();
        let source = "```{python}\nx = 1\n```\n";
        let mut record = record_of(source, |_| Some(stdout("f\n")));
        record.metadata.quarto_files.insert(
            "t_files/figure-html/p.png".into(),
            crate::record::RecordFile {
                data: "iVBORw0KGgo=".into(),
                size: 8,
            },
        );
        record.metadata.quarto_engines[0].includes =
            Some(json!({"header_includes": ["<h/>"], "include_before": [], "include_after": []}));
        let mut ctx = ctx();
        let (doc, _) = splice_record(parse(source), &record, &mut ctx, dir.path());
        assert!(dir.path().join("t_files/figure-html/p.png").is_file());
        let header: Vec<String> = doc
            .ast
            .meta
            .get_path(&["rendered", "includes", "header"])
            .and_then(|v| v.as_array())
            .unwrap_or(&[])
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        assert_eq!(header, vec!["<h/>"]);
    }

    #[test]
    fn the_stage_holds_a_record_or_captures_never_both() {
        use crate::stage::CaptureSpliceStage;
        let record = Arc::new(three_cells());
        let stage = CaptureSpliceStage::new().with_record(record.clone());
        assert!(stage.has_record());
        let stage = stage.with_captures(vec![]);
        assert!(!stage.has_record());
        let _: Value = json!(null);
    }

    #[tokio::test]
    async fn the_stage_publishes_staleness_as_aux_data_and_on_the_context() {
        use crate::stage::observer::PipelineObserver;
        use crate::stage::{CaptureSpliceStage, PipelineData, PipelineStage};

        #[derive(Default)]
        struct AuxLog(std::sync::Mutex<Vec<(String, Value)>>);
        impl PipelineObserver for AuxLog {
            fn on_auxiliary_data(&self, _s: &str, _i: usize, kind: &str, data: &Value) {
                self.0
                    .lock()
                    .unwrap()
                    .push((kind.to_string(), data.clone()));
            }
        }

        let log = Arc::new(AuxLog::default());
        let mut ctx = ctx().with_observer(log.clone());
        let edited = THREE.replace("b = 2", "b = 22");
        let stage = CaptureSpliceStage::new().with_record(Arc::new(three_cells()));
        stage
            .run(PipelineData::DocumentAst(parse(&edited)), &mut ctx)
            .await
            .unwrap();

        let events = log.0.lock().unwrap();
        let (kind, payload) = events
            .iter()
            .find(|(k, _)| k == crate::stage::stages::RECORD_STALENESS_KIND)
            .expect("a staleness event");
        assert_eq!(kind, "RecordStaleness");
        let states: Vec<&str> = payload["units"]
            .as_array()
            .unwrap()
            .iter()
            .map(|u| u["staleness"].as_str().unwrap())
            .collect();
        assert_eq!(states, ["fresh", "stale", "fresh"]);
        assert_eq!(payload["orphaned"].as_array().unwrap().len(), 1);
        let report = ctx.record_splice_report.as_ref().expect("report on ctx");
        assert_eq!(report.units[1].1, UnitStaleness::Stale);
    }

    /// Characterization (EXR-14 Phase B): the scan is textual, so a span is a unit, and is
    /// spliced, wherever it sits in the serialized document, including inside an attribute
    /// value. Pinned so a change to the scan or the substitution shows up here.
    #[test]
    fn spans_in_a_heading_a_table_cell_and_an_attribute_value_splice() {
        let cases = [
            ("# H `{python} a`\n\nbody\n", "# H VAL"),
            ("| A | B |\n|---|---|\n| `{python} a` | x |\n", "| VAL |"),
            ("[text]{title=\"`{python} a`\"}\n", "title=\"VAL\""),
        ];
        for (source, expected) in cases {
            let record = record_of(source, |_| Some(value("VAL")));
            assert_eq!(record.cells.len(), 1, "{source}");
            let (text, report) = splice(source, &record);
            assert_eq!(report.units.len(), 1, "{source}");
            assert!(text.contains(expected), "{source} => {text}");
        }
    }

    /// Design decision 7: the record path never depends on the legacy splice, so EXR-17 can
    /// delete `engine/capture_splice.rs` without touching it.
    #[test]
    fn the_record_arm_does_not_import_the_legacy_splice() {
        for (name, source) in [
            ("record_splice.rs", include_str!("record_splice.rs")),
            ("matching.rs", include_str!("../../record/matching.rs")),
        ] {
            let code = source.split("#[cfg(test)]").next().unwrap();
            // Needles are assembled so this file does not match the seam scanner itself.
            let module = ["engine", "capture_splice"].join("::");
            let entry = ["apply", "capture", "splice"].join("_");
            assert!(
                !code.contains(&module) && !code.contains(&entry),
                "{name} reaches into the legacy splice"
            );
        }
    }
}
