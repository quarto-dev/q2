/*
 * engine/knitr/adapter.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! KNR-3 Phase 3B: the adapter from a knitr run's captured chunks to record units.
//!
//! Each chunk the capture saw is joined to its unit by the `quarto-unit-id` option it carried.
//! A chunk KNR-1's mapper can present becomes a **structured** cell: nbformat outputs from the
//! pieces, plus the keys the shared presenter reader (`presenter::knitr_inputs`) consumes, and
//! the `structured` hash profile. A chunk it cannot map becomes a **fallback** cell holding the
//! chunk hook's finished markdown (`text/x-quarto-markdown`) under the default profile, as in
//! EXR-11. The record is a side product: any alignment problem drops it with a diagnostic and
//! the render goes on (epic decision 19).

use std::collections::BTreeMap;

use serde_json::{Value, json};

use quarto_source_map::{By, SourceInfo};

use crate::cell_options::partition_cell_options;
use crate::engine::context::ExecutionContext;
use crate::engine::diagnostics::EngineRecordProblem;
use crate::engine::units::Unit;
use crate::presenter::knitr_inputs::{
    ECHOED_CODE_KEY, EXPRESSION_STARTS_KEY, OUTPUT_ANCHORS_KEY, OUTPUT_KINDS_KEY,
    RESOLVED_OPTIONS_KEY,
};
use crate::presenter::options::OutputKind;
use crate::presenter::output::CellOutput;
#[cfg(test)]
use crate::record::QUARTO_MARKDOWN_MIME;
use crate::record::{
    EngineEntryInput, EngineRecord, HEADER_KEYS_KEY, HashDefinition, RecordBuilder, UnitResult,
    partition_author_text,
};

use super::chunk_records::{UNIT_ID_OPTION, align, chunk_output};

const KNITR: &str = "knitr";
use super::pieces_map::{expression_starts, output_kinds, pieces_to_cell_anchored};
use super::span_records::span_output;
use super::structured::{mark_structured, rehash_document};
use super::types::{KnitrCapturedChunk, KnitrChunkRecord, KnitrSpanRecord};

/// What a captured run hands the adapter.
pub(super) struct Captured<'a> {
    pub pieces: &'a [KnitrCapturedChunk],
    /// The option names knitr knows, from the run (`KnitrExecuteResult::known_options`).
    pub known_options: Option<&'a [String]>,
    /// The bytes of a figure path as knitr addresses it.
    pub read_file: &'a dyn Fn(&str) -> Option<Vec<u8>>,
}

/// Build the engine's record from a captured run: a structured cell per chunk the mapper can
/// present, a fallback cell (`fallback`, the chunk hook's finished markdown) per chunk it cannot,
/// and a span cell per inline span. Fails when the chunks do not line up with the units knitr
/// owns (`expected`, the ids sent): the caller then drops the record with a diagnostic.
pub(super) fn build_structured(
    ctx: &ExecutionContext,
    def: &HashDefinition,
    expected: &[String],
    captured: &Captured<'_>,
    fallback: &[KnitrChunkRecord],
    spans: &[KnitrSpanRecord],
) -> Result<EngineRecord, EngineRecordProblem> {
    let build = |e: &dyn std::fmt::Display| EngineRecordProblem::Build {
        detail: e.to_string(),
    };
    // Join by the id the chunk carried; a chunk with none is not one of the units.
    let chunks: Vec<(&str, &KnitrCapturedChunk)> = captured
        .pieces
        .iter()
        .filter_map(|c| Some((c.options.get(UNIT_ID_OPTION)?.as_str()?, c)))
        .collect();
    align(
        expected,
        &chunks.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
    )?;

    let by_id: BTreeMap<&str, usize> = ctx
        .units
        .iter()
        .enumerate()
        .filter_map(|(i, u)| unit_id(u).map(|id| (id, i)))
        .collect();
    let mut builder =
        RecordBuilder::new(KNITR, def, &ctx.units, &ctx.author_texts).map_err(|e| build(&e))?;
    // The structured cells, by unit index, with the keys the reader consumes.
    let mut structured: BTreeMap<usize, BTreeMap<String, Value>> = BTreeMap::new();
    for (id, chunk) in chunks {
        let index = by_id[id];
        // A chunk at or after the one that replaced a knit hook presents from the hook's
        // finished markdown: the presenter cannot reproduce what the override did.
        let mapped = if chunk.hook_override.is_some() {
            None
        } else {
            pieces_to_cell_anchored(chunk, captured.read_file)
        };
        let Some((echoed, result, anchors)) = mapped else {
            let Some(markdown) = fallback.iter().find(|f| f.id == id) else {
                return Err(build(&format!(
                    "no finished markdown for the fallback chunk `{id}`"
                )));
            };
            builder
                .push(
                    index,
                    UnitResult {
                        outputs: vec![chunk_output(markdown, &ctx.format)],
                        ..Default::default()
                    },
                )
                .map_err(|e| build(&e))?;
            continue;
        };
        let outputs: Vec<Value> = result.outputs.iter().map(CellOutput::to_nbformat).collect();
        let mut extra = BTreeMap::new();
        if let Some(keys) = header_keys(&ctx.units[index], &ctx.author_texts.0[index].text) {
            extra.insert(HEADER_KEYS_KEY.to_string(), json!(keys));
        }
        extra.insert(RESOLVED_OPTIONS_KEY.to_string(), resolved_options(chunk));
        if let Some(anchors) = anchors {
            extra.insert(OUTPUT_ANCHORS_KEY.to_string(), json!(anchors));
        }
        if let Some(lines) =
            echoed_code(&ctx.author_texts.0[index].text, &echoed, &ctx.units[index])
        {
            extra.insert(ECHOED_CODE_KEY.to_string(), json!(lines));
        }
        let kinds = output_kinds(chunk);
        if kinds.len() == outputs.len() {
            let kinds: Vec<Value> = kinds
                .iter()
                .map(|k| match k {
                    Some(OutputKind::Message) => json!("message"),
                    Some(OutputKind::Warning) => json!("warning"),
                    None => Value::Null,
                })
                .collect();
            extra.insert(OUTPUT_KINDS_KEY.to_string(), Value::Array(kinds));
        }
        if let Some(starts) = expression_starts(chunk) {
            extra.insert(EXPRESSION_STARTS_KEY.to_string(), json!(starts));
        }
        structured.insert(index, extra);
        builder
            .push(
                index,
                UnitResult {
                    outputs,
                    ..Default::default()
                },
            )
            .map_err(|e| build(&e))?;
    }
    for span in spans {
        builder
            .push(
                by_id[span.id.as_str()],
                UnitResult {
                    outputs: vec![span_output(span, &ctx.format, captured.read_file)],
                    ..Default::default()
                },
            )
            .map_err(|e| build(&e))?;
    }
    let mut record = builder
        .finish(EngineEntryInput {
            can_freeze: true,
            owned_languages: ctx.owned_languages.clone(),
            document_metadata: ctx.document_metadata_json(),
            known_options: captured.known_options.map(<[String]>::to_vec),
            ..Default::default()
        })
        .map_err(|e| build(&e))?;
    // `push` hashed every cell under the default profile; a structured cell takes its own
    // (KNR-3 decision 2), and the document hash then follows the cells.
    for (index, extra) in structured {
        let cell_id = unit_id(&ctx.units[index]).unwrap_or_default();
        let Some(cell) = record
            .cells
            .iter_mut()
            .find(|c| c.metadata.quarto_cell_id == cell_id)
        else {
            continue;
        };
        let attr_tail = match &ctx.units[index] {
            Unit::Cell(c) => c.attr_tail.as_str(),
            Unit::Inline(_) => "",
        };
        mark_structured(def, cell, attr_tail, &ctx.format).map_err(|e| build(&e))?;
        cell.metadata.extra.extend(extra);
    }
    rehash_document(&mut record, &ctx.document_metadata_json()).map_err(|e| build(&e))?;
    Ok(record)
}

/// The code knitr echoed, as nbformat `source` lines, when it differs from the authored code
/// (the author text without its `#|` lines): a numeric `eval` masks lines as `## x`,
/// `ref.label` supplies another chunk\'s code, `tidy` rewrites it. `None` when they agree, so an
/// ordinary chunk carries no key. The cell source stays the authored text: the unit hash is
/// recomputed from it.
fn echoed_code(author_text: &str, echoed: &str, unit: &Unit) -> Option<Vec<String>> {
    let Unit::Cell(cell) = unit else {
        return None;
    };
    let authored = partition_cell_options(
        &cell.lang,
        author_text,
        SourceInfo::generated(By::unknown()),
    )
    .ok()?
    .code;
    let trim = |s: &str| s.trim_end_matches('\n').to_string();
    if trim(&authored) == trim(echoed) {
        return None;
    }
    let echoed = trim(echoed);
    let mut lines: Vec<String> = echoed.split('\n').map(|l| format!("{l}\n")).collect();
    if let Some(last) = lines.last_mut() {
        last.pop();
    }
    Some(lines)
}

/// The keys of the unit\'s `#|` header as the author wrote them, sorted, from the same partition
/// the record builder hashes (KNR-3 slice 8, D1). The join id is not a header key. `None` for a
/// span, or when the options do not partition. Nothing is read from R, so the list is the same
/// for a blank chunk, `eval: false` and every knitr version.
fn header_keys(unit: &Unit, author_text: &str) -> Option<Vec<String>> {
    let Unit::Cell(cell) = unit else {
        return None;
    };
    let (options, _) = partition_author_text(&cell.lang, author_text).ok()?;
    Some(
        options
            .into_keys()
            .filter(|k| k != UNIT_ID_OPTION)
            .collect(),
    )
}

/// The chunk's resolved options as the record stores them: the options object as the capture
/// serialized it, less the unit id (the join key, not an option).
fn resolved_options(chunk: &KnitrCapturedChunk) -> Value {
    let mut options = chunk.options.as_object().cloned().unwrap_or_default();
    options.remove(UNIT_ID_OPTION);
    Value::Object(options)
}

fn unit_id(unit: &Unit) -> Option<&str> {
    match unit {
        Unit::Cell(c) => c.id.as_ref().map(|i| i.0.as_str()),
        Unit::Inline(i) => i.id.as_ref().map(|i| i.0.as_str()),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::engine::knitr::chunk_records::insert_unit_ids;
    use crate::engine::knitr::hash_rule::knitr_hash_definition;
    use crate::engine::units::scan_units;
    use crate::presenter::document::present_document;
    use crate::presenter::figures::FsFigureSink;
    use crate::presenter::inline::InlineTarget;
    use crate::record::{AuthorTexts, fill_ids};

    const DOC: &str = "\
```{r}
#| echo: true
x <- 1
x
message('hi')
```

Prose.

```{r}
#| class-source: mine
y
```
";

    fn ctx() -> ExecutionContext {
        let mut units = scan_units(DOC);
        let texts = AuthorTexts::from_scanned(&units);
        fill_ids(&mut units, &texts).unwrap();
        ExecutionContext::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/proj"),
            PathBuf::from("/proj/doc.qmd"),
            "html",
        )
        .with_owned_languages(vec!["r".into()])
        .with_units(units)
        .with_author_texts(texts)
        .with_want_record(true)
    }

    fn chunk(id: &str, code: &str, options: Value, pieces: Value) -> KnitrCapturedChunk {
        let mut options = options;
        options["quarto-unit-id"] = json!(id);
        serde_json::from_value(json!({
            "ordinal": 1, "label": "t", "code": code, "options": options, "pieces": pieces
        }))
        .unwrap()
    }

    fn no_files(_: &str) -> Option<Vec<u8>> {
        None
    }

    fn known() -> Vec<String> {
        ["echo", "class.source", "class.message"]
            .map(String::from)
            .to_vec()
    }

    /// Captured chunks for DOC's two cells.
    fn pieces(ids: &[String]) -> Vec<KnitrCapturedChunk> {
        vec![
            chunk(
                &ids[0],
                "x <- 1\nx\nmessage('hi')",
                json!({"echo": true, "label": "unnamed-chunk-1"}),
                json!([
                    {"class": "source", "text": "x <- 1\n"},
                    {"class": "source", "text": "x\n"},
                    {"class": "output", "text": "[1] 1\n"},
                    {"class": "source", "text": "message('hi')\n"},
                    {"class": "message", "text": "hi\n"},
                ]),
            ),
            chunk(
                &ids[1],
                "y",
                json!({"class.source": "mine", "foo.unknown": "1"}),
                json!([{"class": "source", "text": "y\n"}]),
            ),
        ]
    }

    fn build(
        c: &ExecutionContext,
        found: &[KnitrCapturedChunk],
        fallback: &[KnitrChunkRecord],
    ) -> Result<EngineRecord, EngineRecordProblem> {
        let (_, ids) = insert_unit_ids(DOC, c);
        let known = known();
        build_structured(
            c,
            &knitr_hash_definition(),
            &ids,
            &Captured {
                pieces: found,
                known_options: Some(&known),
                read_file: &no_files,
            },
            fallback,
            &[],
        )
    }

    fn ids(c: &ExecutionContext) -> Vec<String> {
        insert_unit_ids(DOC, c).1
    }

    #[test]
    fn a_mappable_chunk_becomes_a_structured_cell_with_the_reader_keys() {
        let c = ctx();
        let record = build(&c, &pieces(&ids(&c)), &[]).unwrap();
        assert_eq!(record.engine.name, "knitr");
        assert_eq!(record.engine.known_options, Some(known()));
        let cell = &record.cells[0];
        assert_eq!(
            cell.metadata.quarto_hash_rule.as_deref(),
            Some("structured")
        );
        let extra = &cell.metadata.extra;
        assert_eq!(extra[OUTPUT_KINDS_KEY], json!([null, "message"]));
        // stdout after the first two source lines, the message after the third
        assert_eq!(extra[OUTPUT_ANCHORS_KEY], json!([2, 3]));
        assert_eq!(extra[EXPRESSION_STARTS_KEY], json!([0, 1, 2]));
        assert_eq!(extra[RESOLVED_OPTIONS_KEY]["echo"], json!(true));
        // The id is the join key, not an option.
        assert!(extra[RESOLVED_OPTIONS_KEY].get("quarto-unit-id").is_none());
        assert_eq!(cell.outputs[0]["output_type"], "stream");
        assert_eq!(cell.outputs[1]["name"], "stderr");
    }

    /// KNR-3 slice 8 (D1): every `#|` key of the author text, presentation ones included, and
    /// never the join id.
    #[test]
    fn a_structured_cell_records_every_header_key_from_the_author_text() {
        let c = ctx();
        let record = build(&c, &pieces(&ids(&c)), &[]).unwrap();
        assert_eq!(
            record.cells[0].metadata.extra[HEADER_KEYS_KEY],
            json!(["echo"])
        );
        assert_eq!(
            record.cells[1].metadata.extra[HEADER_KEYS_KEY],
            json!(["class-source"])
        );
        for cell in &record.cells {
            let keys = cell.metadata.extra[HEADER_KEYS_KEY].to_string();
            assert!(!keys.contains("quarto-unit-id"), "{keys}");
        }
    }

    #[test]
    fn a_chunk_without_a_header_records_an_empty_key_list() {
        let doc = "```{r}
x
```
";
        let mut units = scan_units(doc);
        let texts = AuthorTexts::from_scanned(&units);
        fill_ids(&mut units, &texts).unwrap();
        let c = ExecutionContext::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/proj"),
            PathBuf::from("/proj/doc.qmd"),
            "html",
        )
        .with_owned_languages(vec!["r".into()])
        .with_units(units)
        .with_author_texts(texts)
        .with_want_record(true);
        let (_, ids) = insert_unit_ids(doc, &c);
        let found = vec![chunk(
            &ids[0],
            "x",
            json!({}),
            json!([{"class": "source", "text": "x\n"}]),
        )];
        let record = build_structured(
            &c,
            &knitr_hash_definition(),
            &ids,
            &Captured {
                pieces: &found,
                known_options: None,
                read_file: &no_files,
            },
            &[],
            &[],
        )
        .unwrap();
        assert_eq!(record.cells[0].metadata.extra[HEADER_KEYS_KEY], json!([]));
    }

    #[test]
    fn a_fallback_cell_has_no_header_keys() {
        let c = ctx();
        let found = ids(&c);
        let mut chunks = pieces(&found);
        chunks[1].hook_override = Some("output".into());
        let fallback = [KnitrChunkRecord {
            id: found[1].clone(),
            markdown: "OVERRIDDEN\n".into(),
            options_digest: "d".into(),
            files: vec![],
        }];
        let record = build(&c, &chunks, &fallback).unwrap();
        assert!(!record.cells[1].metadata.extra.contains_key(HEADER_KEYS_KEY));
        assert!(record.cells[0].metadata.extra.contains_key(HEADER_KEYS_KEY));
    }

    #[test]
    fn the_header_keys_are_in_no_hash_and_not_an_option() {
        let c = ctx();
        let record = build(&c, &pieces(&ids(&c)), &[]).unwrap();
        let def = knitr_hash_definition();
        for cell in &record.cells {
            let mut without = cell.clone();
            without.metadata.extra.remove(HEADER_KEYS_KEY);
            assert_eq!(
                crate::record::recompute_cell_hash(&def, cell).unwrap(),
                crate::record::recompute_cell_hash(&def, &without).unwrap()
            );
            assert!(
                cell.metadata.options().all(|(k, _)| k != HEADER_KEYS_KEY),
                "options() must not list it"
            );
        }
    }

    #[test]
    fn echoed_code_is_written_only_when_it_differs_from_the_authored_code() {
        let c = ctx();
        let found = ids(&c);
        let mut chunks = pieces(&found);
        // knitr echoed a masked line (numeric `eval`); the first chunk echoes what was written.
        chunks[1] = chunk(
            &found[1],
            "## y",
            json!({"class.source": "mine"}),
            json!([{"class": "source", "text": "## y\n"}]),
        );
        let record = build(&c, &chunks, &[]).unwrap();
        assert!(
            !record.cells[0].metadata.extra.contains_key(ECHOED_CODE_KEY),
            "an ordinary chunk carries no key"
        );
        assert_eq!(
            record.cells[1].metadata.extra[ECHOED_CODE_KEY],
            json!(["## y"])
        );
    }

    #[test]
    fn echoed_code_does_not_change_the_source_or_the_hashes() {
        let c = ctx();
        let found = ids(&c);
        let plain = build(&c, &pieces(&found), &[]).unwrap();
        let mut chunks = pieces(&found);
        chunks[1] = chunk(
            &found[1],
            "## y",
            json!({"class.source": "mine", "foo.unknown": "1"}),
            json!([{"class": "source", "text": "## y\n"}]),
        );
        let masked = build(&c, &chunks, &[]).unwrap();
        let (a, b) = (&plain.cells[1], &masked.cells[1]);
        assert_eq!(a.source, b.source);
        assert_eq!(a.metadata.quarto_exec_hash, b.metadata.quarto_exec_hash);
        assert_eq!(
            a.metadata.quarto_content_hash,
            b.metadata.quarto_content_hash
        );
        assert!(
            !b.source.concat().contains("## y"),
            "the cell source stays the authored text"
        );
        let def = knitr_hash_definition();
        assert_eq!(
            crate::record::recompute_cell_hash(&def, b).unwrap(),
            crate::record::recompute_cell_hash(&def, a).unwrap()
        );
    }

    #[test]
    fn the_flat_keys_are_the_structured_profiles_not_every_option() {
        let c = ctx();
        let record = build(&c, &pieces(&ids(&c)), &[]).unwrap();
        // `class.source` is presentation-only under the structured profile, so it is not a flat
        // key; the output format is.
        let opts: Vec<&str> = record.cells[1]
            .metadata
            .options()
            .map(|(k, _)| k.as_str())
            .collect();
        assert!(!opts.contains(&"class.source"), "{opts:?}");
        assert!(opts.contains(&"quarto-output-format"), "{opts:?}");
    }

    #[test]
    fn an_unmappable_chunk_becomes_a_fallback_cell_with_the_hooks_markdown() {
        let c = ctx();
        let found = ids(&c);
        let mut chunks = pieces(&found);
        chunks[1] = chunk(
            &found[1],
            "y",
            json!({}),
            json!([{"class": "other", "classes": ["recordedplot"]}]),
        );
        let fallback = [KnitrChunkRecord {
            id: found[1].clone(),
            markdown: "::: {.cell}\nFINISHED\n:::\n".into(),
            options_digest: "d".into(),
            files: vec![],
        }];
        let record = build(&c, &chunks, &fallback).unwrap();
        let cell = &record.cells[1];
        assert_eq!(cell.metadata.quarto_hash_rule, None);
        assert_eq!(
            cell.outputs[0]["data"][QUARTO_MARKDOWN_MIME],
            "::: {.cell}\nFINISHED\n:::\n"
        );
        assert!(!cell.metadata.extra.contains_key(RESOLVED_OPTIONS_KEY));
        // No unit has both structured outputs and stored markdown.
        assert_eq!(
            record.cells[0]
                .outputs
                .iter()
                .filter(|o| { o["data"].get(QUARTO_MARKDOWN_MIME).is_some() })
                .count(),
            0
        );
    }

    #[test]
    fn a_chunk_flagged_with_an_overridden_hook_is_a_fallback_cell_even_if_mappable() {
        let c = ctx();
        let found = ids(&c);
        let mut chunks = pieces(&found);
        // The same pieces that map to a structured cell, but the document replaced a hook.
        chunks[1].hook_override = Some("output".into());
        let fallback = [KnitrChunkRecord {
            id: found[1].clone(),
            markdown: "OVERRIDDEN\n".into(),
            options_digest: "d".into(),
            files: vec![],
        }];
        let record = build(&c, &chunks, &fallback).unwrap();
        let cell = &record.cells[1];
        assert_eq!(cell.metadata.quarto_hash_rule, None);
        assert_eq!(
            cell.outputs[0]["data"][QUARTO_MARKDOWN_MIME],
            "OVERRIDDEN\n"
        );
        assert!(!cell.metadata.extra.contains_key(RESOLVED_OPTIONS_KEY));
        // The chunk before the override stays structured.
        assert!(
            record.cells[0]
                .metadata
                .extra
                .contains_key(RESOLVED_OPTIONS_KEY)
        );
    }

    #[test]
    fn a_chunk_without_an_id_is_ignored() {
        let c = ctx();
        let mut chunks = pieces(&ids(&c));
        chunks.push(
            serde_json::from_value(json!({
                "ordinal": 3, "label": "extra", "code": "z", "options": {}, "pieces": []
            }))
            .unwrap(),
        );
        assert!(build(&c, &chunks, &[]).is_ok());
    }

    #[test]
    fn a_unit_with_no_captured_chunk_is_a_count_mismatch() {
        let c = ctx();
        let mut chunks = pieces(&ids(&c));
        chunks.pop();
        assert_eq!(
            build(&c, &chunks, &[]).unwrap_err(),
            EngineRecordProblem::CountMismatch {
                expected: 2,
                found: 1
            }
        );
    }

    #[test]
    fn the_record_presents_through_the_product_path() {
        let c = ctx();
        let record = build(&c, &pieces(&ids(&c)), &[]).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut fig = FsFigureSink::new(&dir.path().join("doc.qmd"));
        let out = present_document(
            &record.cells,
            DOC,
            &c.units,
            None,
            InlineTarget::from_format("html"),
            "html",
            std::slice::from_ref(&record.engine),
            &mut fig,
        )
        .unwrap();
        let md = &out.markdown;
        // source and output interleaved from the anchors, the message from the kinds
        assert!(
            md.find("[1] 1").unwrap() < md.find("message('hi')").unwrap(),
            "{md}"
        );
        assert!(md.contains("hi"), "{md}");
        // the unknown option, from known_options, and the header option the class names
        assert!(md.contains("foo.unknown='1'"), "{md}");
        assert!(md.contains(".mine"), "{md}");
    }

    #[test]
    fn notebook_level_results_stay_out_of_the_knitr_record() {
        // Includes, preserve, post-processing and dependencies belong to the `ExecuteResult`
        // (EXR-11 left them there, and the captured path keeps it): the entry carries none.
        let c = ctx();
        let record = build(&c, &pieces(&ids(&c)), &[]).unwrap();
        let entry = &record.engine;
        assert!(entry.includes.is_none());
        assert!(entry.filters.is_empty());
        assert!(entry.preserve.is_empty());
        assert!(!entry.needs_postprocess);
        assert!(entry.html_dependencies.is_empty());
        assert!(entry.resource_files.is_empty());
    }

    #[test]
    fn the_document_hash_follows_the_structured_cells() {
        let c = ctx();
        let record = build(&c, &pieces(&ids(&c)), &[]).unwrap();
        let mut again = record.clone();
        again.cells[0].metadata.quarto_exec_hash = "changed".into();
        super::rehash_document(&mut again, &c.document_metadata_json()).unwrap();
        assert_ne!(again.engine.document_hash, record.engine.document_hash);
        // and the stored hash is the one over the cells as they are
        let mut same = record.clone();
        super::rehash_document(&mut same, &c.document_metadata_json()).unwrap();
        assert_eq!(same.engine.document_hash, record.engine.document_hash);
    }
}
