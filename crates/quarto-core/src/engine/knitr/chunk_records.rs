/*
 * engine/knitr/chunk_records.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! EXR-11 Phase 11C: the Rust half of knitr's chunk records.
//!
//! - [`insert_unit_ids`] puts a `quarto-unit-id` option line into every chunk knitr
//!   owns, so the chunk hook (`hooks.R`) can echo the id back.
//! - [`attach_records`] turns the chunks the hook returned into the engine's
//!   record, one rendered-markdown unit per chunk, or a warning and no record when the
//!   returned chunks do not line up with the units knitr owns.
//!
//! - EXR-12 adds the inline spans to the same record ([`Spans`], built in `span_records`).
//!
//! The record is a side product. The live path never reads it, and a mismatch drops the
//! record without failing the render (epic decision 19).

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::engine::context::{ExecuteResult, ExecutionContext};
use crate::engine::diagnostics::{EngineRecordProblem, engine_record_diagnostic};
use crate::engine::units::{Unit, locate_units, view_for_engine};
use crate::record::{
    BuildError, EngineEntryInput, HashDefinition, QUARTO_MARKDOWN_MIME, RecordBuilder, UnitResult,
};

use super::adapter::{Captured, build_structured};
use super::span_records::span_output;
use super::types::{KnitrChunkRecord, KnitrSpanRecord};

/// The chunk option the id travels in. Added to `other_opts` in `hooks.R`, so it is
/// never emitted as a div attribute.
pub(super) const UNIT_ID_OPTION: &str = "quarto-unit-id";

const KNITR: &str = "knitr";

fn unit_id(unit: &Unit) -> Option<&str> {
    match unit {
        Unit::Cell(c) => c.id.as_ref().map(|i| i.0.as_str()),
        Unit::Inline(i) => i.id.as_ref().map(|i| i.0.as_str()),
    }
}

fn owns(ctx: &ExecutionContext, lang: &str) -> bool {
    ctx.owned_languages
        .iter()
        .any(|o| o.eq_ignore_ascii_case(lang))
}

/// `text` with a `quarto-unit-id` option line after the opener of every cell knitr owns
/// and the stage numbered, and those ids in document order.
///
/// `text` is the text the stage scanned (before this engine's other edits), so the
/// shared units are relocated into it with `locate_units`; a cell an earlier engine's
/// output introduced has no id and is left alone. Returns `text` unchanged when no
/// record is wanted or the context carries no units, which keeps the live path
/// byte-identical.
pub(super) fn insert_unit_ids(text: &str, ctx: &ExecutionContext) -> (String, Vec<String>) {
    if !ctx.want_record || ctx.units.is_empty() {
        return (text.to_string(), Vec::new());
    }
    let located = locate_units(&ctx.units, text);
    let mut edits: Vec<(usize, String)> = Vec::new();
    let mut ids = Vec::new();
    for eu in view_for_engine(&located, &|lang| owns(ctx, lang)) {
        let (Unit::Cell(cell), true) = (eu.unit, eu.owned) else {
            continue;
        };
        let (Some(id), Some(opener_len)) = (unit_id(eu.unit), text[cell.start..].find('\n')) else {
            continue;
        };
        let syntax = crate::cell_options::comment_syntax_for(&cell.lang);
        // knitr reads option lines under one comment prefix and stops at the first line that uses
        // another. It accepts `#|` in any chunk, so an author's `#|` lines in a `{sql}` chunk
        // (whose scanner prefix is `--`) must not be split from the options by a `--|` line.
        let first_line = text[cell.start + opener_len + 1..]
            .lines()
            .next()
            .unwrap_or("")
            .strip_prefix(cell.container_prefix.as_str())
            .unwrap_or("")
            .trim_start_matches(' ');
        let (prefix, suffix) = if first_line.starts_with("#|") {
            ("#", None)
        } else {
            (syntax.prefix, syntax.suffix)
        };
        // A quoted scalar keeps an id that looks like a number or a keyword a string.
        let line = format!(
            "{}{}{}| {UNIT_ID_OPTION}: {}{}\n",
            cell.container_prefix,
            " ".repeat(cell.indent),
            prefix,
            Value::String(id.to_string()),
            suffix.unwrap_or(""),
        );
        edits.push((cell.start + opener_len + 1, line));
        ids.push(id.to_string());
    }
    let mut out = text.to_string();
    // Back to front, so earlier offsets stay valid.
    for (at, line) in edits.into_iter().rev() {
        out.insert_str(at, &line);
    }
    (out, ids)
}

/// The record's one output for a chunk: its finished markdown as `display_data`, tagged
/// with the output format it was rendered for. The options digest and the files the
/// chunk wrote travel in the output's metadata; neither is matched on.
pub(super) fn chunk_output(chunk: &KnitrChunkRecord, format: &str) -> Value {
    let mut metadata = json!({
        "quarto_format": format,
        "quarto_options_digest": chunk.options_digest,
    });
    if !chunk.files.is_empty() {
        metadata["quarto_files"] = json!(chunk.files);
    }
    json!({
        "output_type": "display_data",
        "data": { QUARTO_MARKDOWN_MIME: chunk.markdown },
        "metadata": metadata,
    })
}

/// Check that `found` is `expected`, id for id and in order, naming the first difference.
pub(super) fn align(expected: &[String], found: &[&str]) -> Result<(), EngineRecordProblem> {
    if expected.len() != found.len() {
        return Err(EngineRecordProblem::CountMismatch {
            expected: expected.len(),
            found: found.len(),
        });
    }
    for (i, id) in found.iter().enumerate() {
        if *id == expected[i] {
            continue;
        }
        return Err(if found[..i].contains(id) {
            EngineRecordProblem::DuplicateUnitId { id: id.to_string() }
        } else if !expected.iter().any(|e| e == id) {
            EngineRecordProblem::UnknownUnitId { id: id.to_string() }
        } else {
            EngineRecordProblem::Build {
                detail: format!("unit `{id}` came back where `{}` was expected", expected[i]),
            }
        });
    }
    Ok(())
}

/// What the engine sent for the inline spans and what it handed back.
pub(super) struct Spans<'a> {
    /// The ids the rewrite embedded (`SpanPlan::expected`), in document order.
    pub expected: &'a [String],
    pub found: &'a [KnitrSpanRecord],
    /// `file:line` of the span with a given id, for a diagnostic.
    pub location: &'a dyn Fn(&str) -> String,
}

impl Spans<'_> {
    fn align(&self) -> Result<(), EngineRecordProblem> {
        let found: Vec<&str> = self.found.iter().map(|s| s.id.as_str()).collect();
        if let Some(missing) = self
            .expected
            .iter()
            .find(|e| !found.contains(&e.as_str()))
            .filter(|_| found.len() <= self.expected.len())
        {
            return Err(EngineRecordProblem::MissingSpan {
                location: (self.location)(missing),
            });
        }
        align(self.expected, &found)
    }
}

fn build(
    ctx: &ExecutionContext,
    def: &HashDefinition,
    chunks: &[KnitrChunkRecord],
    spans: &[KnitrSpanRecord],
) -> Result<crate::record::EngineRecord, BuildError> {
    let by_id: BTreeMap<&str, usize> = ctx
        .units
        .iter()
        .enumerate()
        .filter_map(|(i, u)| unit_id(u).map(|id| (id, i)))
        .collect();
    let mut builder = RecordBuilder::new(KNITR, def, &ctx.units, &ctx.author_texts)?;
    for chunk in chunks {
        let index = by_id[chunk.id.as_str()];
        builder.push(
            index,
            UnitResult {
                outputs: vec![chunk_output(chunk, &ctx.format)],
                ..Default::default()
            },
        )?;
    }
    for span in spans {
        let index = by_id[span.id.as_str()];
        builder.push(
            index,
            UnitResult {
                outputs: vec![span_output(span, &ctx.format, &|_| None)],
                ..Default::default()
            },
        )?;
    }
    builder.finish(EngineEntryInput {
        can_freeze: true,
        owned_languages: ctx.owned_languages.clone(),
        document_metadata: ctx.document_metadata_json(),
        ..Default::default()
    })
}

/// Build `result.record` from the chunks and spans the hooks returned, or push a warning
/// and leave it `None` when they do not line up with what was sent: `expected` (the ids
/// [`insert_unit_ids`] sent) and `spans.expected`. Does nothing when no record is wanted or
/// no ids were sent.
pub(super) fn attach_records(
    ctx: &ExecutionContext,
    def: &HashDefinition,
    expected: &[String],
    chunks: &[KnitrChunkRecord],
    spans: &Spans<'_>,
    captured: Option<&Captured<'_>>,
    result: &mut ExecuteResult,
) {
    if !ctx.want_record || ctx.units.is_empty() {
        return;
    }
    let found: Vec<&str> = chunks.iter().map(|c| c.id.as_str()).collect();
    // A captured run (KNR-3) builds structured units from the pieces; otherwise every chunk is
    // EXR-11\'s one rendered-markdown unit.
    let outcome = align(expected, &found)
        .and_then(|()| spans.align())
        .and_then(|()| match captured {
            Some(captured) => build_structured(ctx, def, expected, captured, chunks, spans.found),
            None => build(ctx, def, chunks, spans.found).map_err(|e| EngineRecordProblem::Build {
                detail: e.to_string(),
            }),
        });
    match outcome {
        Ok(record) => result.record = Some(record),
        Err(problem) => result
            .warnings
            .push(engine_record_diagnostic(KNITR, &problem)),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::engine::units::scan_units;
    use crate::record::{AuthorTexts, fill_ids};

    const DOC: &str = "\
# T

```{r}
x <- 1
```

Prose with `{r} x`.

```{python}
y = 2
```

> ```{r}
> #| echo: false
> z
> ```

```{ojs}
1
```
";

    fn ctx(doc: &str, owned: &[&str], want_record: bool) -> ExecutionContext {
        let mut units = scan_units(doc);
        let texts = AuthorTexts::from_scanned(&units);
        fill_ids(&mut units, &texts).unwrap();
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

    /// `attach_records` for a document with no spans.
    fn attach_chunk_records(
        ctx: &ExecutionContext,
        def: &HashDefinition,
        expected: &[String],
        chunks: &[KnitrChunkRecord],
        result: &mut ExecuteResult,
    ) {
        let spans = Spans {
            expected: &[],
            found: &[],
            location: &|_| String::new(),
        };
        attach_records(ctx, def, expected, chunks, &spans, None, result);
    }

    fn chunk(id: &str, markdown: &str) -> KnitrChunkRecord {
        KnitrChunkRecord {
            id: id.to_string(),
            markdown: markdown.to_string(),
            options_digest: "d".to_string(),
            files: vec![],
        }
    }

    #[test]
    fn no_ids_are_sent_unless_a_record_is_wanted() {
        let c = ctx(DOC, &["r"], false);
        assert_eq!(insert_unit_ids(DOC, &c), (DOC.to_string(), vec![]));
        let bare = ExecutionContext::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/proj"),
            PathBuf::from("/proj/doc.qmd"),
            "html",
        )
        .with_want_record(true);
        assert_eq!(insert_unit_ids(DOC, &bare), (DOC.to_string(), vec![]));
    }

    /// knitr reads a chunk's option lines under one comment prefix and stops at the first line
    /// that uses another. A `{sql}` chunk whose author wrote `#|` lines (knitr accepts them, though
    /// Quarto's scanner reads `--|`) must therefore get its id line in the author's prefix, or the
    /// author's options fall out of the block and `connection` never reaches the engine.
    #[test]
    fn the_id_line_uses_the_prefix_of_the_authors_option_lines() {
        let doc = "```{sql}\n#| connection: con\nSELECT 1\n```\n\n\
                   ```{sql}\n--| connection: con\nSELECT 2\n```\n\n\
                   ```{sql}\nSELECT 3\n```\n";
        let c = ctx(doc, &["sql"], true);
        let (text, ids) = insert_unit_ids(doc, &c);
        assert_eq!(ids.len(), 3, "{text}");
        let line = |i: usize, prefix: &str| format!("{prefix}| quarto-unit-id: \"{}\"\n", ids[i]);
        assert!(
            text.contains(&format!("```{{sql}}\n{}#| connection: con\n", line(0, "#"))),
            "{text}"
        );
        assert!(
            text.contains(&format!(
                "```{{sql}}\n{}--| connection: con\n",
                line(1, "--")
            )),
            "{text}"
        );
        assert!(
            text.contains(&format!("```{{sql}}\n{}SELECT 3\n", line(2, "--"))),
            "{text}"
        );
    }

    #[test]
    fn an_id_line_follows_the_opener_of_each_owned_cell_only() {
        let c = ctx(DOC, &["r"], true);
        let (text, ids) = insert_unit_ids(DOC, &c);
        assert_eq!(
            ids.len(),
            2,
            "two r cells; python, ojs and the span are not owned"
        );
        for id in &ids {
            assert!(
                text.contains(&format!("#| quarto-unit-id: \"{id}\"\n")),
                "{text}"
            );
        }
        assert!(text.contains("```{r}\n#| quarto-unit-id: \""), "{text}");
        // Inside the blockquote the line keeps the prefix, and goes ahead of the author's options.
        assert!(
            text.contains(&format!(
                "> ```{{r}}\n> #| quarto-unit-id: \"{}\"\n> #| echo: false\n",
                ids[1]
            )),
            "{text}"
        );
        assert!(!text.contains("```{python}\n#| quarto-unit-id"), "{text}");
        assert!(!text.contains("```{ojs}\n#| quarto-unit-id"), "{text}");
        // Stripping the inserted lines gives the original back.
        let stripped: String = text
            .split_inclusive('\n')
            .filter(|l| !l.contains("quarto-unit-id"))
            .collect();
        assert_eq!(stripped, DOC);
    }

    #[test]
    fn ids_come_back_in_document_order_and_match_the_shared_list() {
        let c = ctx(DOC, &["r", "python"], true);
        let (_, ids) = insert_unit_ids(DOC, &c);
        let shared: Vec<String> = c
            .units
            .iter()
            .filter(|u| matches!(u, Unit::Cell(_)) && matches!(u.lang(), "r" | "python"))
            .map(|u| unit_id(u).unwrap().to_string())
            .collect();
        assert_eq!(ids, shared);
    }

    #[test]
    fn a_cell_an_earlier_engine_introduced_gets_no_id() {
        let c = ctx(DOC, &["r"], true);
        let text = format!("{DOC}\n```{{r}}\nnew\n```\n");
        let (out, ids) = insert_unit_ids(&text, &c);
        assert_eq!(ids.len(), 2);
        assert_eq!(out.matches("quarto-unit-id").count(), 2);
    }

    #[test]
    fn matching_chunks_build_a_record_of_rendered_markdown_units() {
        let c = ctx(DOC, &["r"], true);
        let (_, ids) = insert_unit_ids(DOC, &c);
        let mut with_files = chunk(&ids[1], "::: cell\nz\n:::\n");
        with_files.files = vec!["doc_files/figure-html/a-1.png".to_string()];
        let chunks = vec![chunk(&ids[0], "x\n"), with_files];
        let mut result = ExecuteResult::default();
        attach_chunk_records(
            &c,
            &HashDefinition::conservative("knitr"),
            &ids,
            &chunks,
            &mut result,
        );
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        let record = result.record.expect("a record");
        assert_eq!(record.engine.name, "knitr");
        assert_eq!(record.cells.len(), 2);
        let out = &record.cells[1].outputs[0];
        assert_eq!(out["output_type"], "display_data");
        assert_eq!(out["data"][QUARTO_MARKDOWN_MIME], "::: cell\nz\n:::\n");
        assert_eq!(out["metadata"]["quarto_format"], "html");
        assert_eq!(out["metadata"]["quarto_options_digest"], "d");
        assert_eq!(
            out["metadata"]["quarto_files"],
            json!(["doc_files/figure-html/a-1.png"])
        );
        assert!(
            record.cells[0].outputs[0]["metadata"]
                .get("quarto_files")
                .is_none()
        );
    }

    #[test]
    fn a_short_record_is_a_warning_naming_both_counts_and_no_record() {
        let c = ctx(DOC, &["r"], true);
        let (_, ids) = insert_unit_ids(DOC, &c);
        let mut result = ExecuteResult::default();
        attach_chunk_records(
            &c,
            &HashDefinition::conservative("knitr"),
            &ids,
            &[chunk(&ids[0], "x")],
            &mut result,
        );
        assert!(result.record.is_none());
        assert_eq!(result.warnings.len(), 1);
        let text = result.warnings[0].to_text(None);
        assert!(
            text.contains("knitr") && text.contains("1 annotated") && text.contains("2 units"),
            "{text}"
        );
    }

    #[test]
    fn unknown_duplicate_and_out_of_order_ids_drop_the_record() {
        let c = ctx(DOC, &["r"], true);
        let (_, ids) = insert_unit_ids(DOC, &c);
        let def = HashDefinition::conservative("knitr");
        for (chunks, needle) in [
            (vec![chunk(&ids[0], ""), chunk("nope", "")], "nope"),
            (
                vec![chunk(&ids[0], ""), chunk(&ids[0], "")],
                "more than once",
            ),
            (
                vec![chunk(&ids[1], ""), chunk(&ids[0], "")],
                "came back where",
            ),
        ] {
            let mut result = ExecuteResult::default();
            attach_chunk_records(&c, &def, &ids, &chunks, &mut result);
            assert!(result.record.is_none());
            assert!(
                result.warnings[0].to_text(None).contains(needle),
                "{needle}"
            );
        }
    }

    /// The unit exec hash of the single `r` chunk in `doc`, through the knitr declaration.
    fn exec_hash(doc: &str) -> String {
        let c = ctx(doc, &["r"], true);
        let (_, ids) = insert_unit_ids(doc, &c);
        let chunks: Vec<_> = ids.iter().map(|id| chunk(id, "")).collect();
        let mut result = ExecuteResult::default();
        let def = {
            let mut d = HashDefinition::conservative("knitr");
            d.document_keys = vec!["knitr".to_string()];
            d
        };
        attach_chunk_records(&c, &def, &ids, &chunks, &mut result);
        let warnings: Vec<String> = result.warnings.iter().map(|w| w.to_text(None)).collect();
        let record = result
            .record
            .unwrap_or_else(|| panic!("no record for {doc:?}: {warnings:?}"));
        record.cells[0].metadata.quarto_exec_hash.clone()
    }

    #[test]
    fn knitr_unit_hash_covers_all_options() {
        let base = exec_hash("```{r}\n#| echo: true\n1\n```\n");
        assert_ne!(
            base,
            exec_hash("```{r}\n#| echo: false\n1\n```\n"),
            "#| option"
        );
        // Options in the fence's brace tail count too: they are in no `#|` line.
        assert_ne!(
            exec_hash("```{r echo=TRUE}\n1\n```\n"),
            exec_hash("```{r echo=FALSE}\n1\n```\n"),
            "brace option"
        );
        assert_ne!(
            exec_hash("```{r}\n1\n```\n"),
            exec_hash("```{r echo=TRUE}\n1\n```\n")
        );
        // Presentation options count too: the markdown is rendered, so nothing is excluded.
        assert_ne!(
            exec_hash("```{r}\n#| fig-cap: a\n1\n```\n"),
            exec_hash("```{r}\n#| fig-cap: b\n1\n```\n")
        );
        // Prose around the chunk is not part of the unit.
        assert_eq!(
            base,
            exec_hash("Some prose.\n\n```{r}\n#| echo: true\n1\n```\n\nMore prose.\n")
        );
    }

    #[test]
    fn nothing_is_attached_when_no_record_is_wanted() {
        let c = ctx(DOC, &["r"], false);
        let mut result = ExecuteResult::default();
        attach_chunk_records(
            &c,
            &HashDefinition::conservative("knitr"),
            &[],
            &[],
            &mut result,
        );
        assert!(result.record.is_none() && result.warnings.is_empty());
    }

    // === EXR-12: spans join the record ===

    fn span(id: &str, markdown: &str) -> KnitrSpanRecord {
        KnitrSpanRecord {
            id: id.to_string(),
            markdown: markdown.to_string(),
            piece: None,
        }
    }

    /// Chunk ids, span ids and the chunk records for `DOC` (two `r` chunks, one `r` span).
    fn doc_ids(c: &ExecutionContext) -> (Vec<String>, Vec<String>, Vec<KnitrChunkRecord>) {
        let (text, ids) = insert_unit_ids(DOC, c);
        let span_ids = super::super::span_records::SpanPlan::new(&text, c).expected;
        let chunks = ids.iter().map(|id| chunk(id, "c\n")).collect();
        (ids, span_ids, chunks)
    }

    #[test]
    fn matching_spans_join_the_record_as_rendered_markdown_units() {
        let c = ctx(DOC, &["r"], true);
        let (ids, span_ids, chunks) = doc_ids(&c);
        assert_eq!(span_ids.len(), 1);
        let found = vec![span(&span_ids[0], "x\\*y")];
        let mut result = ExecuteResult::default();
        attach_records(
            &c,
            &HashDefinition::conservative("knitr"),
            &ids,
            &chunks,
            &Spans {
                expected: &span_ids,
                found: &found,
                location: &|_| String::new(),
            },
            None,
            &mut result,
        );
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        let record = result.record.expect("a record");
        assert_eq!(record.cells.len(), 3, "two chunks and a span");
        let cell = record
            .cells
            .iter()
            .find(|c| c.metadata.quarto_cell_id == span_ids[0])
            .expect("the span's cell");
        assert_eq!(cell.outputs[0]["data"][QUARTO_MARKDOWN_MIME], "x\\*y");
        assert_eq!(cell.outputs[0]["metadata"]["quarto_format"], "html");
    }

    #[test]
    fn inline_count_mismatch_skips_record_with_diagnostic() {
        let c = ctx(DOC, &["r"], true);
        let (ids, span_ids, chunks) = doc_ids(&c);
        let mut result = ExecuteResult::default();
        attach_records(
            &c,
            &HashDefinition::conservative("knitr"),
            &ids,
            &chunks,
            &Spans {
                expected: &span_ids,
                found: &[],
                location: &|id| {
                    assert_eq!(id, span_ids[0]);
                    "doc.qmd:7".to_string()
                },
            },
            None,
            &mut result,
        );
        assert!(result.record.is_none(), "no record for the document");
        assert_eq!(result.warnings.len(), 1);
        let text = result.warnings[0].to_text(None);
        assert!(
            text.contains("knitr") && text.contains("doc.qmd:7") && text.contains("inline"),
            "{text}"
        );
    }

    #[test]
    fn an_unknown_or_duplicate_span_id_drops_the_record() {
        let c = ctx(DOC, &["r"], true);
        let (ids, span_ids, chunks) = doc_ids(&c);
        for (found, needle) in [
            // the owned span never came back: reported at its location
            (vec![span("nope", "")], "inline expression at loc"),
            (
                vec![span(&span_ids[0], ""), span(&span_ids[0], "")],
                "2 annotated",
            ),
        ] {
            let mut result = ExecuteResult::default();
            attach_records(
                &c,
                &HashDefinition::conservative("knitr"),
                &ids,
                &chunks,
                &Spans {
                    expected: &span_ids,
                    found: &found,
                    location: &|_| "loc".to_string(),
                },
                None,
                &mut result,
            );
            assert!(result.record.is_none());
            let text = result.warnings[0].to_text(None);
            assert!(text.contains(needle), "{text}");
        }
    }
}
