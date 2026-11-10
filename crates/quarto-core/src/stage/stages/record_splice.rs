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

use crate::engine::units::Unit;
use crate::presenter::document::{Edit, PresentInput, UnitOutcome, apply_edits, present_units};
use crate::presenter::figures::{FigureSink, RuntimeFigureSink};
use crate::presenter::inline::InlineTarget;
use crate::record::{
    EngineEntry, Record, UnitKey, UnitKind, UnitLocation, UnitStaleness, lost_header_keys,
    match_units,
};
use crate::stage::data::DocumentAst;
use crate::stage::stages::engine_execution::{
    merge_execute_scope, merge_markdown_into_ast_with_slot, scan_and_fill_ids, serialize_ast_to_qmd,
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
/// The spliced output is stamped (see [`stamp_spliced_output`]).
pub(crate) fn splice_record(
    doc: DocumentAst,
    record: &Record,
    ctx: &mut StageContext,
    doc_dir: &Path,
) -> (DocumentAst, SpliceReport) {
    splice_record_with(doc, record, ctx, doc_dir, stamp_by_default())
}

/// [`splice_record`] with an explicit switch for the stamping post-walk. A test seam: no flag,
/// setting or environment variable reaches it, so the stamp is always on in a real render.
pub(crate) fn splice_record_with(
    doc: DocumentAst,
    record: &Record,
    ctx: &mut StageContext,
    doc_dir: &Path,
    stamp: bool,
) -> (DocumentAst, SpliceReport) {
    // Derived at present time from every cell of the record, as render does, and never stored.
    let page_deps = crate::presenter::page_dependencies::page_dependencies(
        &record.cells,
        &record.metadata.quarto_engines,
        &ctx.format.identifier.to_string(),
    );
    let (mut doc, report) = splice_units(doc, record, ctx, doc_dir, stamp, &page_deps);
    // Includes must reach the pane even when no unit splices, so they are folded here rather
    // than by the merge. (Before bd-8svy3p89's metadata reconcile the merge replaced the meta
    // with the reparsed text's, which also dropped anything folded earlier.)
    fold_includes(record, &mut doc);
    // After the stored entries, which are never deduplicated; the derived head text is deduplicated
    // within `page_dependencies`.
    super::include_resolve::append_pandoc_includes(
        &mut doc.ast.meta,
        &crate::stage::data::PandocIncludes {
            header_includes: page_deps.header_includes.clone(),
            ..Default::default()
        },
    );
    fold_warnings(record, ctx);
    (doc, report)
}

thread_local! {
    /// Set only by [`with_stamp_switch`], which exists under test.
    static STAMP_OFF: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The stamp is always on outside tests. Under test, [`with_stamp_switch`] can turn it off
/// for the calling thread, so tests that render the same document with the stamp on and off
/// cannot disturb each other.
fn stamp_by_default() -> bool {
    !(cfg!(test) && STAMP_OFF.with(std::cell::Cell::get))
}

/// Run `f` with the default stamp switch set to `stamp`, on this thread only. The splice runs
/// on the calling thread in a current-thread runtime, which is how the render-invariance test
/// reaches it through the whole pipeline.
#[cfg(test)]
fn with_stamp_switch<T>(stamp: bool, f: impl FnOnce() -> T) -> T {
    let previous = STAMP_OFF.with(|c| c.replace(!stamp));
    let out = f();
    STAMP_OFF.with(|c| c.set(previous));
    out
}

fn splice_units(
    mut doc: DocumentAst,
    record: &Record,
    ctx: &mut StageContext,
    doc_dir: &Path,
    stamp: bool,
    page_deps: &crate::presenter::page_dependencies::PageDependencies,
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
                    UnitKey::from_author_text_with_profiles(
                        &e.hash_definition,
                        kind,
                        unit.lang(),
                        &text.text,
                        tail,
                        &format,
                    )
                    .ok()
                })
                // No owning engine, or options that do not parse: the unit can match by
                // content hash only.
                .unwrap_or_else(|| UnitKey::content_only(kind, &text.content_hash))
        })
        .collect();
    let mut matching = match_units(&keys, &record.cells);
    // A unit that lost a header key its cell recorded is stale (KNR-3 slice 8).
    for unit_index in lost_header_keys(&matching, &keys, &record.cells) {
        matching.demote(unit_index);
    }
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
    // The pairs the splice presents: a matched unit, except a cell recorded for the metadata.
    let matched: Vec<(usize, &crate::record::RecordCell)> = matching
        .assignment
        .iter()
        .enumerate()
        .filter_map(|(i, a)| a.map(|ci| (i, &record.cells[ci])))
        .filter(|(_, cell)| cell.metadata.quarto_location != Some(UnitLocation::Metadata))
        .collect();
    let input = PresentInput {
        text: &qmd,
        units: &units,
        author_texts: &texts,
        scope: doc_scope.as_ref(),
        target: InlineTarget::from_format(&format),
        format: &format,
        engines: &record.metadata.quarto_engines,
        page_deps,
    };
    // The splice keeps the source on every skip and every error, and ignores a front-matter span.
    let mut edits: Vec<Edit> = Vec::new();
    let mut edit_units: Vec<usize> = Vec::new();
    for report in present_units(&input, &matched, fig.as_mut()) {
        if let UnitOutcome::Replace(edit) = report.outcome
            && !edit.front_matter
        {
            edit_units.push(report.unit);
            edits.push(edit);
        }
    }
    if edits.is_empty() {
        return (doc, report);
    }

    // The text before the edits is the baseline for the metadata
    // reconcile: splice never edits front matter, so every value should
    // come back as it was (bd-8svy3p89).
    let baseline = crate::engine::nested_cell_mask::unmask(&qmd);
    // Where each unit's presented markdown lands in the final text, before the unmask.
    let unit_ids_by_start: std::collections::HashMap<usize, String> = edits
        .iter()
        .zip(&edit_units)
        .map(|(edit, &u)| (edit.start, unit_id(&units[u])))
        .collect();
    let (text, sorted, ranges) = apply_edits(&qmd, edits);
    let mut spans: Vec<StampSpan> = sorted
        .iter()
        .zip(ranges)
        .map(|(edit, range)| StampSpan {
            start: range.start,
            end: range.end,
            unit: unit_ids_by_start[&edit.start].clone(),
        })
        .collect();
    // Unmasking changes lengths outside the units too; carry each span through it.
    for span in &mut spans {
        span.start = crate::engine::nested_cell_mask::unmask(&text[..span.start]).len();
        span.end = crate::engine::nested_cell_mask::unmask(&text[..span.end]).len();
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
    match merge_markdown_into_ast_with_slot(ast, &mut ast_context, &baseline, &text, intermediate) {
        Ok((mut merged, parse_warnings, _plan, slot)) => {
            if stamp {
                let source = record
                    .metadata
                    .extra
                    .get("quarto_provenance")
                    .and_then(Value::as_str)
                    .unwrap_or("engine");
                stamp_spliced_output(&mut merged, slot, &spans, source);
            }
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

/// A unit's presented markdown, as a byte range of the reparsed text.
#[derive(Debug, Clone)]
struct StampSpan {
    start: usize,
    end: usize,
    unit: String,
}

/// The kind of every stamped node: `quarto-source-map`'s `jupyter-output`, which is atomic
/// (output is regenerated or imported, never edited in place; provenance contract section 7).
/// The name says "jupyter" for a Julia, knitr or Positron record too; `data.source` carries the
/// real origin.
fn record_output_by(source: &str, unit: &str) -> quarto_source_map::By {
    let mut by = quarto_source_map::By::jupyter_output();
    by.data = serde_json::json!({ "source": source, "unit": unit });
    by
}

/// Mark every node the reparse produced inside a unit's span as
/// `SourceInfo::Generated { from: [], by: jupyter-output }` with `data = {source, unit}`.
///
/// A node is spliced output when its source info resolves into `slot` (the file the reparse
/// registered; reconcile keeps the document's own nodes under the document's file id, so prose,
/// stale cells and unmatched units are never touched) and its range lies inside one span. A
/// `Str` is stamped when it merely overlaps a span, because the reparse merges a value with the
/// adjacent prose characters (`v1` and `.` become `Str("v1.")`).
///
/// Never stamped: a `CodeBlock` (the copy-button decoration is keyed on its `Original` range,
/// `CodeBlockDecorationKey::from_source_info`) and a `Header` (the heading-id step treats a
/// `Generated` header as having a deliberate id; epic decision 9). Attribute, target and
/// metadata source info are not nodes and are left alone.
///
/// An interim measure: when engines return provenance for the nodes they produce, this
/// post-walk goes away.
fn stamp_spliced_output(
    ast: &mut quarto_pandoc_types::Pandoc,
    slot: FileId,
    spans: &[StampSpan],
    source: &str,
) {
    use quarto_ast_reconcile::SourceRole;
    quarto_ast_reconcile::walk_source_infos(ast, &mut |si: &mut SourceInfo, role| {
        if matches!(
            role,
            SourceRole::CodeBlock | SourceRole::Header | SourceRole::Ancillary | SourceRole::Meta
        ) {
            return;
        }
        if si.root_file_id() != Some(slot) {
            return;
        }
        let Some(range) = si.preimage_in(slot) else {
            return;
        };
        let is_str = role == SourceRole::Str;
        let hit = spans.iter().find(|sp| {
            if is_str {
                range.start < sp.end && range.end > sp.start
            } else {
                range.start >= sp.start && range.end <= sp.end
            }
        });
        if let Some(span) = hit {
            *si = SourceInfo::generated(record_output_by(source, &span.unit));
        }
    });
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

    /// bd-8svy3p89: splicing re-reads the document through the same
    /// metadata round-trip as a live engine run, and never edits front
    /// matter, so the metadata comes back exactly as it was. A marked path
    /// with two underscores used to come back as emphasis (GH #1152).
    #[test]
    fn splice_keeps_the_metadata_as_it_was() {
        let mut doc = parse(THREE);
        let theme = quarto_pandoc_types::ConfigValue::new_path(
            "_extensions/a/_b.scss".into(),
            quarto_source_map::SourceInfo::original(quarto_source_map::FileId(77), 3, 9),
        );
        doc.ast.meta.insert_path(&["theme"], theme.clone());
        let before = doc.ast.meta.clone();
        let (doc, _) = splice_record(doc, &three_cells(), &mut ctx(), Path::new("/project"));
        assert!(text_of(&doc).contains("out0"), "the splice happened");
        assert_eq!(doc.ast.meta.get("theme"), Some(&theme));
        assert_eq!(doc.ast.meta, before);
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

    /// KNR-3 slice 8: a unit that lost a header key the record's cell carried is stale, stays
    /// source, and does not disturb its fresh neighbors.
    #[test]
    fn a_header_deleted_unit_is_stale_and_its_neighbors_still_splice() {
        let with = "```{python}\na = 1\n```\n\n```{python}\n#| echo: true\nb = 2\n```\n\n```{python}\nc = 3\n```\n";
        let hash_def = {
            let mut d = HashDefinition::conservative("jupyter");
            d.presentation_cell_options = vec!["echo".into()];
            d
        };
        let doc = parse(with);
        let (qmd, _) = serialize_ast_to_qmd(&doc.ast).unwrap();
        let (units, texts) = scan_and_fill_ids(&qmd, &[]);
        let mut b = RecordBuilder::new("jupyter", &hash_def, &units, &texts).unwrap();
        for i in 0..3 {
            b.push(i, stdout(&format!("out{i}\n"))).unwrap();
        }
        let mut entry = b
            .finish(EngineEntryInput {
                owned_languages: vec!["python".into()],
                ..Default::default()
            })
            .unwrap();
        entry.cells[1]
            .metadata
            .extra
            .insert("quarto_header_keys".into(), json!(["echo"]));
        let record = coalesce(&[entry], &units).unwrap();

        // Unchanged: all three splice.
        let (text, report) = splice(with, &record);
        assert!(text.contains("out1"), "{text}");
        assert!(report.units.iter().all(|(_, s)| *s == UnitStaleness::Fresh));

        // Header deleted: the unit stays source and reports Stale; the others splice.
        let (text, report) = splice(&with.replace("#| echo: true\n", ""), &record);
        assert!(text.contains("out0") && text.contains("out2"), "{text}");
        assert!(!text.contains("out1"), "{text}");
        assert!(text.contains("b = 2"), "{text}");
        assert_eq!(report.units[1].1, UnitStaleness::Stale);
        assert_eq!(report.units[0].1, UnitStaleness::Fresh);
        assert_eq!(report.orphaned.len(), 1);
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

    // ---- KNR-3 decision 8 (as corrected by interlude I3): structured knitr records through the
    // splice. A structured cell names the `structured` hash profile and carries the output format
    // as a flat option, so a format change or an execution-relevant edit goes stale and a
    // presentation-only edit stays fresh.

    use crate::engine::knitr::hash_rule::knitr_hash_definition;
    use crate::engine::knitr::structured::{mark_structured, rehash_document};
    use crate::record::QUARTO_MARKDOWN_MIME;

    enum Built {
        Structured(UnitResult),
        Fallback(String),
    }

    fn r_stdout(text: &str) -> Built {
        Built::Structured(stdout(text))
    }

    /// A knitr record the way the KNR-3 adapter writes it: `push`, then the structured profile,
    /// hash, flat keys and the format option on structured cells, and the document hash.
    fn knitr_record(
        source: &str,
        format: &str,
        built_for: impl Fn(usize) -> Option<Built>,
    ) -> Record {
        let doc = parse(source);
        let (qmd, _) = serialize_ast_to_qmd(&doc.ast).unwrap();
        let (units, texts): (_, AuthorTexts) = scan_and_fill_ids(&qmd, &[]);
        let def = knitr_hash_definition();
        let mut builder = RecordBuilder::new("knitr", &def, &units, &texts).unwrap();
        let mut structured_ids = std::collections::BTreeSet::new();
        for i in 0..units.len() {
            match built_for(i) {
                Some(Built::Structured(result)) => {
                    builder.push(i, result).unwrap();
                    structured_ids.insert(unit_id(&units[i]));
                }
                Some(Built::Fallback(markdown)) => builder
                    .push(
                        i,
                        UnitResult {
                            outputs: vec![json!({
                                "output_type": "display_data",
                                "data": {QUARTO_MARKDOWN_MIME: markdown},
                                "metadata": {"quarto_format": format}
                            })],
                            execution_count: None,
                            kernel: None,
                        },
                    )
                    .unwrap(),
                None => {}
            }
        }
        let mut entry = builder
            .finish(EngineEntryInput {
                owned_languages: vec!["r".into()],
                ..Default::default()
            })
            .unwrap();
        for cell in &mut entry.cells {
            if structured_ids.contains(&cell.metadata.quarto_cell_id) {
                mark_structured(&def, cell, "", format).unwrap();
            }
        }
        rehash_document(&mut entry, &Default::default()).unwrap();
        coalesce(&[entry], &units).unwrap()
    }

    fn splice_in(format: Format, source: &str, record: &Record) -> (String, SpliceReport) {
        let mut c = ctx();
        c.format = format;
        let (doc, report) = splice_record(parse(source), record, &mut c, Path::new("/project"));
        (text_of(&doc), report)
    }

    const R_ECHO: &str = "```{r}\n#| echo: true\nx <- 1\nx\n```\n";

    #[test]
    fn a_structured_record_for_another_format_is_stale() {
        let record = knitr_record(R_ECHO, "docx", |_| Some(r_stdout("[1] 1\n")));
        let (text, report) = splice_in(Format::html(), R_ECHO, &record);
        assert_eq!(report.units[0].1, UnitStaleness::Stale, "{text}");
        assert!(!text.contains("[1] 1"), "{text}");
    }

    #[test]
    fn a_structured_record_for_this_format_is_fresh() {
        let record = knitr_record(R_ECHO, "html", |_| Some(r_stdout("[1] 1\n")));
        let (text, report) = splice_in(Format::html(), R_ECHO, &record);
        assert_eq!(report.units[0].1, UnitStaleness::Fresh, "{text}");
        assert!(text.contains("[1] 1"), "{text}");
    }

    #[test]
    fn a_presentation_edit_is_fresh_and_uses_the_current_echo() {
        let record = knitr_record(R_ECHO, "html", |_| Some(r_stdout("[1] 1\n")));
        let edited = R_ECHO.replace("echo: true", "echo: false");
        let (text, report) = splice_in(Format::html(), &edited, &record);
        assert_eq!(report.units[0].1, UnitStaleness::Fresh, "{text}");
        assert!(
            text.contains("[1] 1") && !text.contains("cell-code"),
            "{text}"
        );
    }

    #[test]
    fn an_execution_relevant_edit_is_stale() {
        let record = knitr_record(R_ECHO, "html", |_| Some(r_stdout("[1] 1\n")));
        let edited = R_ECHO.replace("echo: true", "echo: true\n#| warning: false");
        let (_, report) = splice_in(Format::html(), &edited, &record);
        assert_eq!(report.units[0].1, UnitStaleness::Stale);
    }

    /// What EXR-11 wrote: the all-options declaration at version 1, no `structured` profile, no
    /// `quarto_hash_rule` or format option on the cell, and the output as pass-through markdown.
    fn exr11_record(source: &str, markdown: &str) -> Record {
        let doc = parse(source);
        let (qmd, _) = serialize_ast_to_qmd(&doc.ast).unwrap();
        let (units, texts): (_, AuthorTexts) = scan_and_fill_ids(&qmd, &[]);
        let mut def = crate::record::HashDefinition::conservative("knitr");
        def.version = 1;
        def.document_keys = vec!["knitr".to_string()];
        let mut builder = RecordBuilder::new("knitr", &def, &units, &texts).unwrap();
        for i in 0..units.len() {
            builder
                .push(
                    i,
                    UnitResult {
                        outputs: vec![json!({
                            "output_type": "display_data",
                            "data": {QUARTO_MARKDOWN_MIME: markdown},
                            "metadata": {"quarto_format": "html"}
                        })],
                        execution_count: None,
                        kernel: None,
                    },
                )
                .unwrap();
        }
        let mut entry = builder
            .finish(EngineEntryInput {
                owned_languages: vec!["r".into()],
                ..Default::default()
            })
            .unwrap();
        rehash_document(&mut entry, &Default::default()).unwrap();
        coalesce(&[entry], &units).unwrap()
    }

    #[test]
    fn an_exr11_era_record_still_presents_through_the_pass_through_presenter() {
        let record = exr11_record(R_ECHO, "```r\nx <- 1\nx\n```\n\n```\n[1] 1\n```\n");
        assert_eq!(record.metadata.quarto_engines[0].hash_definition.version, 1);
        let (text, report) = splice_in(Format::html(), R_ECHO, &record);
        // The record is judged by the declaration it stores (version 1), so it is fresh and its
        // pass-through markdown is placed.
        assert_eq!(report.units[0].1, UnitStaleness::Fresh, "{text}");
        assert!(text.contains("[1] 1"), "{text}");
        // The version bump restarts every hash: the current declaration never reproduces an
        // EXR-11 hash, so a new run's record shares no unit hash with the old one.
        let (units, texts): (_, AuthorTexts) = scan_and_fill_ids(R_ECHO, &[]);
        let Unit::Cell(cell) = &units[0] else {
            panic!("expected a cell")
        };
        let current = UnitKey::from_author_text(
            &knitr_hash_definition(),
            UnitKind::Cell,
            "r",
            &texts.0[0].text,
            &cell.attr_tail,
        )
        .unwrap();
        assert_ne!(
            current.exec_hash, record.cells[0].metadata.quarto_exec_hash,
            "version 2 must not reproduce a version 1 hash"
        );
    }

    #[test]
    fn a_cell_naming_no_profile_matches_as_before() {
        // A Jupyter-shaped record (no profile, no format option) is not affected.
        let record = record_of(THREE, |i| Some(stdout(&format!("out{i}\n"))));
        let (text, report) = splice_in(Format::html(), THREE, &record);
        assert!(report.units.iter().all(|(_, s)| *s == UnitStaleness::Fresh));
        assert!(text.contains("out1"), "{text}");
    }

    #[test]
    fn a_structured_knitr_cell_reaches_the_shared_reader() {
        use crate::presenter::knitr_inputs::fixture;
        let mut record = knitr_record(fixture::SOURCE, "html", |_| {
            Some(Built::Structured(UnitResult {
                outputs: fixture::outputs(),
                execution_count: None,
                kernel: None,
            }))
        });
        fixture::apply(&mut record.cells[0]);
        record.metadata.quarto_engines[0].known_options =
            Some(fixture::KNOWN.iter().map(|s| s.to_string()).collect());
        let (text, report) = splice_in(Format::html(), fixture::SOURCE, &record);
        assert_eq!(report.units[0].1, UnitStaleness::Fresh, "{text}");
        fixture::assert_rich(&text);
    }

    mod layering;
    mod opr_pinning;
    mod option_sweep;
    mod page_dependencies;
    mod stamp;

    const MIXED: &str =
        "```{r}\n#| echo: true\nx <- 1\nx\n```\n\n```{r}\n#| echo: true\ny <- 2\ny\n```\n";

    #[test]
    fn a_mixed_record_splices_both_and_a_fallback_presentation_edit_is_stale() {
        let fallback = "::: {.cell}\n\nFALLBACK-MARKDOWN\n\n:::\n";
        let record = knitr_record(MIXED, "html", |i| {
            Some(if i == 0 {
                r_stdout("[1] 1\n")
            } else {
                Built::Fallback(fallback.to_string())
            })
        });
        let (text, report) = splice_in(Format::html(), MIXED, &record);
        assert!(
            report.units.iter().all(|(_, s)| *s == UnitStaleness::Fresh),
            "{text}"
        );
        assert!(
            text.contains("[1] 1") && text.contains("FALLBACK-MARKDOWN"),
            "{text}"
        );
        // The structured unit ignores a presentation edit; the fallback unit counts every option.
        let edited = MIXED.replacen("echo: true", "echo: false", 2);
        let edited = edited.replacen("echo: false", "echo: true", 1);
        let (text, report) = splice_in(Format::html(), &edited, &record);
        assert_eq!(report.units[0].1, UnitStaleness::Fresh, "{text}");
        assert_eq!(report.units[1].1, UnitStaleness::Stale, "{text}");
    }

    #[test]
    fn a_presentation_edit_survives_a_cell_inserted_above() {
        // The id carries the document-wide index, which shifts when a cell is added, so the
        // match cannot go through the id.
        let record = knitr_record(R_ECHO, "html", |_| Some(r_stdout("[1] 1\n")));
        let edited = format!(
            "```{{r}}\nz <- 0\n```\n\n{}",
            R_ECHO.replace("echo: true", "echo: false")
        );
        let (text, report) = splice_in(Format::html(), &edited, &record);
        assert_eq!(report.units[1].1, UnitStaleness::Fresh, "{text}");
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
