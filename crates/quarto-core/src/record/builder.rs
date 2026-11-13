//! `RecordBuilder`: one engine's cells and entry (EXR-4 Phase 3).
//!
//! The builder takes the stage's unit list and its [`AuthorTexts`] and, for each unit an engine
//! ran, only the outputs, execution count and kernel (a [`UnitResult`]). Everything that matters
//! for matching comes from the stage's unit and author text, never from what the engine ran, so
//! code an engine trimmed or rewrote cannot change a cell's source, id or hashes. The builder
//! does no scan and names no engine.
//!
//! The nbformat cell `id` is set to the plain content hash here; making it unique across the
//! whole record (`-2`, `-3` for later duplicates) is [`coalesce`](super::coalesce)'s job, since
//! duplicates can span engines.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::engine::units::Unit;

use super::exec_hash::{
    HashError, UnitHashInput, UnitHashRef, execution_options, hash_document, hash_unit,
    partition_author_text,
};
use super::hash_definition::HashDefinition;
use super::ids::author_label;
use super::types::{
    AuthorTexts, CellMetadata, CellType, EngineEntry, EngineRecord, RecordCell, UnitKind,
    UnitLocation, UnitResult,
};

/// Why a record could not be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BuildError {
    /// The author-text mapping is not total over the unit list.
    #[error("cannot build a record: {units} units but {author_texts} author texts")]
    LengthMismatch { units: usize, author_texts: usize },
    /// `push` named a unit the stage's list does not have.
    #[error("unit index {index} is out of range for {units} units")]
    UnitOutOfRange { index: usize, units: usize },
    /// A unit was pushed twice.
    #[error("unit {index} was pushed more than once")]
    DuplicatePush { index: usize },
    /// The unit's id slot is empty: the stage did not run `fill_ids`.
    #[error("unit {index} has no id; run fill_ids before building a record")]
    MissingId { index: usize },
    #[error(transparent)]
    Hash(#[from] HashError),
}

/// What an engine contributes to its [`EngineEntry`] besides cells and hashes.
#[derive(Debug, Clone, Default)]
pub struct EngineEntryInput {
    /// Engine version, kernelspec, executor version and platform. Informational only.
    pub provenance: BTreeMap<String, Value>,
    pub can_freeze: bool,
    /// The languages the engine owned for this document.
    pub owned_languages: Vec<String>,
    /// The merged top-level document metadata (project and document merged, overlay-free) that
    /// the document hash reads. It is a hash input only and is not stored; the stored
    /// `metadata` below is the engine's folded result.
    pub document_metadata: BTreeMap<String, Value>,

    // The folded `ExecuteResult` (`engine/context.rs`).
    pub includes: Option<Value>,
    pub filters: Vec<String>,
    pub preserve: BTreeMap<String, String>,
    pub needs_postprocess: bool,
    pub html_dependencies: Vec<Value>,
    pub pandoc: Option<BTreeMap<String, Value>>,
    pub metadata: Option<BTreeMap<String, Value>>,
    pub resource_files: Vec<String>,
    pub warnings: Vec<Value>,
    /// The option names the engine knows (see `EngineEntry::known_options`).
    pub known_options: Option<Vec<String>>,
    /// Whether the engine interleaves by default (see `EngineEntry::interleaves_output`).
    pub interleaves_output: bool,
}

/// Builds one engine's [`EngineRecord`] from the stage's units.
pub struct RecordBuilder<'a> {
    engine_name: String,
    def: &'a HashDefinition,
    units: &'a [Unit],
    author_texts: &'a AuthorTexts,
    /// Cells by unit index, so they come out in the stage's unit order whatever the push order.
    cells: BTreeMap<usize, RecordCell>,
}

impl<'a> RecordBuilder<'a> {
    /// Start a builder. `units` is the stage's single unit list with its id slots filled and
    /// `author_texts` is aligned with it index-for-index.
    pub fn new(
        engine_name: impl Into<String>,
        def: &'a HashDefinition,
        units: &'a [Unit],
        author_texts: &'a AuthorTexts,
    ) -> Result<Self, BuildError> {
        if units.len() != author_texts.len() {
            return Err(BuildError::LengthMismatch {
                units: units.len(),
                author_texts: author_texts.len(),
            });
        }
        Ok(Self {
            engine_name: engine_name.into(),
            def,
            units,
            author_texts,
            cells: BTreeMap::new(),
        })
    }

    /// Record the result of one unit the engine ran. A unit never pushed gets no cell.
    pub fn push(&mut self, unit_index: usize, result: UnitResult) -> Result<(), BuildError> {
        let unit = self
            .units
            .get(unit_index)
            .ok_or(BuildError::UnitOutOfRange {
                index: unit_index,
                units: self.units.len(),
            })?;
        if self.cells.contains_key(&unit_index) {
            return Err(BuildError::DuplicatePush { index: unit_index });
        }
        let author = &self.author_texts.0[unit_index];

        let (id, kind, location) = match unit {
            Unit::Cell(c) => (c.id.as_ref(), UnitKind::Cell, None),
            Unit::Inline(i) => (
                i.id.as_ref(),
                UnitKind::Inline,
                i.in_front_matter.then_some(UnitLocation::Metadata),
            ),
        };
        let id = id
            .ok_or(BuildError::MissingId { index: unit_index })?
            .0
            .clone();
        let lang = unit.lang();

        let attr_tail = match unit {
            Unit::Cell(c) => c.attr_tail.as_str(),
            Unit::Inline(_) => "",
        };

        // A span's text is its expression and has no option lines.
        let (options, code, label) = match kind {
            UnitKind::Inline => (BTreeMap::new(), author.text.clone(), None),
            UnitKind::Cell => {
                let (options, code) = partition_author_text(lang, &author.text)?;
                (options, code, author_label(&author.text, lang))
            }
        };
        let exec_hash = hash_unit(
            self.def,
            &UnitHashInput {
                kind,
                lang,
                code: &code,
                options: &options,
                attr_tail,
                profile: None,
            },
        )?;
        let flat_options = execution_options(self.def, None, &options)?;

        self.cells.insert(
            unit_index,
            RecordCell {
                cell_type: CellType::Code,
                execution_count: result.execution_count,
                id: author.content_hash.clone(),
                metadata: CellMetadata {
                    quarto_cell_id: id,
                    quarto_content_hash: author.content_hash.clone(),
                    quarto_label: label,
                    quarto_kind: kind,
                    quarto_lang: lang.to_string(),
                    quarto_engine: Some(self.engine_name.clone()),
                    quarto_kernel: result.kernel,
                    quarto_location: location,
                    quarto_exec_hash: exec_hash,
                    quarto_hash_rule: None,
                    extra: flat_options.into_iter().collect(),
                },
                outputs: result.outputs,
                source: source_lines(&author.text),
            },
        );
        Ok(())
    }

    /// Finish: compute the engine's document hash over its own cells' unit hashes (in unit
    /// order) and assemble the entry.
    pub fn finish(self, input: EngineEntryInput) -> Result<EngineRecord, BuildError> {
        let cells: Vec<RecordCell> = self.cells.into_values().collect();
        let refs: Vec<UnitHashRef> = cells
            .iter()
            .map(|c| UnitHashRef {
                hash: &c.metadata.quarto_exec_hash,
                profile: c.metadata.quarto_hash_rule.as_deref(),
            })
            .collect();
        let document_hash = hash_document(self.def, &input.document_metadata, &refs)?;
        Ok(EngineRecord {
            engine: EngineEntry {
                name: self.engine_name,
                provenance: input.provenance,
                can_freeze: input.can_freeze,
                hash_definition: self.def.clone(),
                document_hash,
                owned_languages: input.owned_languages,
                includes: input.includes,
                filters: input.filters,
                preserve: input.preserve,
                needs_postprocess: input.needs_postprocess,
                html_dependencies: input.html_dependencies,
                pandoc: input.pandoc,
                metadata: input.metadata,
                resource_files: input.resource_files,
                warnings: input.warnings,
                known_options: input.known_options,
                interleaves_output: input.interleaves_output,
                dependency_data: None,
                extra: BTreeMap::new(),
            },
            cells,
        })
    }
}

/// nbformat `source` as a line array: every line but the last keeps its newline.
fn source_lines(text: &str) -> Vec<String> {
    text.split_inclusive('\n').map(str::to_string).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::units::scan_units;
    use crate::record::{fill_ids, recompute_cell_hash};
    use serde_json::json;

    const DOC: &str = "\
---
title: t
---

Inline `{python} 1 + 1` here.

```{python}
#| label: first
#| fig-width: 7
#| echo: false
x = 1
```

```{r}
y <- 2
```

```{python}
z = 3
```
";

    /// Units with ids filled, plus their author texts.
    fn staged(doc: &str) -> (Vec<Unit>, AuthorTexts) {
        let mut units = scan_units(doc);
        let texts = AuthorTexts::from_scanned(&units);
        fill_ids(&mut units, &texts).unwrap();
        (units, texts)
    }

    fn ran(kernel: &str, n: u64) -> UnitResult {
        UnitResult {
            outputs: vec![json!({"output_type": "stream", "name": "stdout", "text": ["hi\n"]})],
            execution_count: Some(n),
            kernel: Some(kernel.into()),
        }
    }

    fn def() -> HashDefinition {
        let mut d = HashDefinition::conservative("jupyter");
        d.presentation_cell_options = vec!["echo".into()];
        d
    }

    fn index_of(units: &[Unit], lang: &str, nth: usize) -> usize {
        units
            .iter()
            .enumerate()
            .filter(|(_, u)| u.lang() == lang)
            .nth(nth)
            .unwrap()
            .0
    }

    #[test]
    fn cells_come_out_in_unit_order_with_the_stage_ids_whatever_the_push_order() {
        let (units, texts) = staged(DOC);
        let d = def();
        let py0 = index_of(&units, "python", 0);
        let py1 = index_of(&units, "python", 1);
        let py2 = index_of(&units, "python", 2);
        let mut b = RecordBuilder::new("jupyter", &d, &units, &texts).unwrap();
        // Pushed out of order.
        b.push(py2, ran("python3", 3)).unwrap();
        b.push(py0, ran("python3", 1)).unwrap();
        b.push(py1, ran("python3", 2)).unwrap();
        let rec = b.finish(EngineEntryInput::default()).unwrap();

        let ids: Vec<&str> = rec
            .cells
            .iter()
            .map(|c| c.metadata.quarto_cell_id.as_str())
            .collect();
        let expected: Vec<String> = [py0, py1, py2]
            .iter()
            .map(|i| match &units[*i] {
                Unit::Cell(c) => c.id.clone().unwrap().0,
                Unit::Inline(s) => s.id.clone().unwrap().0,
            })
            .collect();
        assert_eq!(ids, expected.iter().map(String::as_str).collect::<Vec<_>>());
        assert!(ids[0].ends_with("-inline"), "the span comes first: {ids:?}");
        assert!(ids[1].ends_with("-first"), "{ids:?}");
        assert_eq!(rec.cells[1].execution_count, Some(2));
        assert_eq!(rec.engine.name, "jupyter");
    }

    #[test]
    fn source_and_hashes_come_from_the_unit_not_from_the_engine() {
        let (units, texts) = staged(DOC);
        let d = def();
        let i = index_of(&units, "python", 1);
        let mut b = RecordBuilder::new("jupyter", &d, &units, &texts).unwrap();
        // An engine-side rewrite can only change outputs; there is no code in `UnitResult`.
        b.push(i, ran("python3", 9)).unwrap();
        let rec = b.finish(EngineEntryInput::default()).unwrap();
        let cell = &rec.cells[0];

        assert_eq!(cell.source.concat(), texts.0[i].text);
        assert!(cell.source[0].starts_with("#| label: first"));
        assert_eq!(cell.metadata.quarto_content_hash, texts.0[i].content_hash);
        assert_eq!(cell.id, texts.0[i].content_hash);
        assert_eq!(cell.metadata.quarto_label.as_deref(), Some("first"));
        assert_eq!(cell.metadata.quarto_lang, "python");
        assert_eq!(cell.metadata.quarto_engine.as_deref(), Some("jupyter"));
        assert_eq!(cell.metadata.quarto_kernel.as_deref(), Some("python3"));
        assert_eq!(cell.metadata.quarto_kind, UnitKind::Cell);
        assert_eq!(cell.metadata.quarto_location, None);
        // The stored hash recomputes from the cell and the stored declaration alone.
        assert_eq!(
            recompute_cell_hash(&rec.engine.hash_definition, cell).unwrap(),
            cell.metadata.quarto_exec_hash
        );
        // `fig-width` is a flat key; the presentation option `echo` is not recorded.
        assert_eq!(cell.metadata.extra.get("fig-width"), Some(&json!(7)));
        assert!(!cell.metadata.extra.contains_key("echo"));
        // `label` counts under this declaration, so it is recorded too (recorded keys are
        // exactly the options that move the hash).
        assert_eq!(cell.metadata.extra.get("label"), Some(&json!("first")));
    }

    fn one_cell(doc: &str, def: &HashDefinition) -> RecordCell {
        let (units, texts) = staged(doc);
        let mut b = RecordBuilder::new("jupyter", def, &units, &texts).unwrap();
        b.push(0, ran("python3", 1)).unwrap();
        b.finish(EngineEntryInput::default())
            .unwrap()
            .cells
            .remove(0)
    }

    #[test]
    fn execution_relevant_options_are_flat_keys_and_presentation_ones_are_not() {
        let cell = one_cell(
            "```{python}\n#| fig-width: 7\n#| echo: false\nx = 1\n```\n",
            &def(),
        );
        let keys: Vec<&str> = cell.metadata.options().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["fig-width"]);
        let json = serde_json::to_value(&cell).unwrap();
        assert_eq!(json["metadata"]["fig-width"], 7);
        assert!(json["metadata"].get("echo").is_none());
        assert!(json["metadata"].get("quarto_options").is_none());
    }

    #[test]
    fn recorded_keys_equal_the_options_that_move_the_exec_hash() {
        let d = def();
        let doc = |opts: &str| format!("```{{python}}\n{opts}x = 1\n```\n");
        let base = one_cell(
            &doc("#| fig-width: 7\n#| fig-height: 5\n#| echo: false\n"),
            &d,
        );
        let recorded: Vec<&str> = base.metadata.options().map(|(k, _)| k.as_str()).collect();
        assert_eq!(recorded, ["fig-height", "fig-width"]);
        for (changed, moves) in [
            ("#| fig-width: 8\n#| fig-height: 5\n#| echo: false\n", true),
            ("#| fig-width: 7\n#| fig-height: 6\n#| echo: false\n", true),
            ("#| fig-width: 7\n#| fig-height: 5\n#| echo: true\n", false),
        ] {
            let cell = one_cell(&doc(changed), &d);
            assert_eq!(
                cell.metadata.quarto_exec_hash != base.metadata.quarto_exec_hash,
                moves,
                "{changed}"
            );
        }
    }

    #[test]
    fn option_order_in_the_source_changes_neither_keys_nor_exec_hash_and_bytes_are_stable() {
        let d = def();
        let a = one_cell(
            "```{python}\n#| fig-width: 7\n#| fig-height: 5\nx = 1\n```\n",
            &d,
        );
        let b = one_cell(
            "```{python}\n#| fig-height: 5\n#| fig-width: 7\nx = 1\n```\n",
            &d,
        );
        // The author text differs (so the Positron content hash does), the execution input
        // does not.
        assert_ne!(
            a.metadata.quarto_content_hash,
            b.metadata.quarto_content_hash
        );
        assert_eq!(a.metadata.quarto_exec_hash, b.metadata.quarto_exec_hash);
        assert_eq!(a.metadata.extra, b.metadata.extra);
        // Keys are emitted sorted by name, identically on every run.
        let emitted = |c: &RecordCell| serde_json::to_string(&c.metadata).unwrap();
        let (ea, eb) = (emitted(&a), emitted(&b));
        assert!(ea.find("\"fig-height\"").unwrap() < ea.find("\"fig-width\"").unwrap());
        assert_eq!(
            ea.replace(&a.metadata.quarto_content_hash, "H")
                .replace(&a.metadata.quarto_cell_id, "I"),
            eb.replace(&b.metadata.quarto_content_hash, "H")
                .replace(&b.metadata.quarto_cell_id, "I")
        );
        assert_eq!(emitted(&a), emitted(&a));
    }

    #[test]
    fn a_unit_the_engine_did_not_run_gets_no_cell() {
        let (units, texts) = staged(DOC);
        let d = def();
        let mut b = RecordBuilder::new("jupyter", &d, &units, &texts).unwrap();
        b.push(index_of(&units, "python", 1), ran("python3", 1))
            .unwrap();
        let rec = b.finish(EngineEntryInput::default()).unwrap();
        assert_eq!(rec.cells.len(), 1);
    }

    #[test]
    fn the_document_hash_covers_this_engines_unit_hashes_only() {
        let (units, texts) = staged(DOC);
        let d = def();
        let hash_with = |py_indices: &[usize], extra: Option<usize>| {
            let mut b = RecordBuilder::new("jupyter", &d, &units, &texts).unwrap();
            for i in py_indices {
                b.push(*i, ran("python3", 1)).unwrap();
            }
            if let Some(i) = extra {
                b.push(i, ran("python3", 1)).unwrap();
            }
            b.finish(EngineEntryInput::default())
                .unwrap()
                .engine
                .document_hash
        };
        let py: Vec<usize> = (0..3).map(|n| index_of(&units, "python", n)).collect();
        let r = index_of(&units, "r", 0);
        let base = hash_with(&py, None);
        assert_eq!(base, hash_with(&py, None), "deterministic");
        // Dropping one of this engine's units moves it; the r unit, never pushed here, is
        // not an input.
        assert_ne!(base, hash_with(&py[..2], None));
        // The same engine pushing the r unit as well does change it (it is then its unit).
        assert_ne!(base, hash_with(&py, Some(r)));
    }

    #[test]
    fn the_document_hash_reads_the_declared_metadata() {
        let (units, texts) = staged(DOC);
        let mut d = def();
        d.document_keys = vec!["jupyter".into()];
        let hash_with = |md: Value| {
            let mut b = RecordBuilder::new("jupyter", &d, &units, &texts).unwrap();
            b.push(0, ran("python3", 1)).unwrap();
            b.finish(EngineEntryInput {
                document_metadata: md.as_object().unwrap().clone().into_iter().collect(),
                ..Default::default()
            })
            .unwrap()
            .engine
            .document_hash
        };
        assert_ne!(
            hash_with(json!({"jupyter": "python3"})),
            hash_with(json!({"jupyter": "julia-1.10"}))
        );
        assert_eq!(
            hash_with(json!({"title": "a"})),
            hash_with(json!({"title": "b"}))
        );
    }

    #[test]
    fn a_front_matter_span_records_its_location() {
        let doc = "---\ntitle: \"`{python} 1`\"\n---\n\n`{python} 2`\n";
        let (units, texts) = staged(doc);
        let d = def();
        let mut b = RecordBuilder::new("jupyter", &d, &units, &texts).unwrap();
        for i in 0..units.len() {
            b.push(i, ran("python3", 1)).unwrap();
        }
        let rec = b.finish(EngineEntryInput::default()).unwrap();
        assert_eq!(rec.cells.len(), 2);
        assert_eq!(
            rec.cells[0].metadata.quarto_location,
            Some(UnitLocation::Metadata)
        );
        assert_eq!(rec.cells[1].metadata.quarto_location, None);
        for c in &rec.cells {
            assert_eq!(c.metadata.quarto_kind, UnitKind::Inline);
            assert!(c.metadata.quarto_label.is_none());
        }
    }

    #[test]
    fn the_entry_carries_the_declaration_and_the_folded_result() {
        let (units, texts) = staged(DOC);
        let d = def();
        let b = RecordBuilder::new("jupyter", &d, &units, &texts).unwrap();
        let rec = b
            .finish(EngineEntryInput {
                can_freeze: true,
                owned_languages: vec!["python".into()],
                filters: vec!["f.lua".into()],
                needs_postprocess: true,
                provenance: BTreeMap::from([("kernelspec".into(), json!("python3"))]),
                ..Default::default()
            })
            .unwrap();
        assert!(rec.cells.is_empty());
        assert_eq!(rec.engine.hash_definition, d);
        assert!(rec.engine.can_freeze && rec.engine.needs_postprocess);
        assert_eq!(rec.engine.owned_languages, ["python"]);
        assert_eq!(rec.engine.filters, ["f.lua"]);
        assert_eq!(rec.engine.document_hash.len(), 64);
    }

    #[test]
    fn misuse_is_an_error_not_a_guess() {
        let (units, texts) = staged(DOC);
        let d = def();
        let short = AuthorTexts(texts.0[1..].to_vec());
        assert_eq!(
            RecordBuilder::new("jupyter", &d, &units, &short).err(),
            Some(BuildError::LengthMismatch {
                units: units.len(),
                author_texts: units.len() - 1
            })
        );

        let mut b = RecordBuilder::new("jupyter", &d, &units, &texts).unwrap();
        assert_eq!(
            b.push(99, UnitResult::default()),
            Err(BuildError::UnitOutOfRange {
                index: 99,
                units: units.len()
            })
        );
        b.push(0, UnitResult::default()).unwrap();
        assert_eq!(
            b.push(0, UnitResult::default()),
            Err(BuildError::DuplicatePush { index: 0 })
        );

        let unfilled = scan_units(DOC);
        let unfilled_texts = AuthorTexts::from_scanned(&unfilled);
        let mut b = RecordBuilder::new("jupyter", &d, &unfilled, &unfilled_texts).unwrap();
        assert_eq!(
            b.push(0, UnitResult::default()),
            Err(BuildError::MissingId { index: 0 })
        );
    }

    #[test]
    fn invalid_option_lines_surface_as_a_hash_error() {
        let doc = "```{python}\n#| echo: [unclosed\nx = 1\n```\n";
        let (units, texts) = staged(doc);
        let d = def();
        let mut b = RecordBuilder::new("jupyter", &d, &units, &texts).unwrap();
        assert!(matches!(
            b.push(0, UnitResult::default()),
            Err(BuildError::Hash(HashError::InvalidOptions(_)))
        ));
    }

    #[test]
    fn source_is_a_line_array_that_concatenates_back() {
        assert_eq!(source_lines(""), Vec::<String>::new());
        assert_eq!(source_lines("a"), ["a"]);
        assert_eq!(source_lines("a\nb"), ["a\n", "b"]);
        assert_eq!(source_lines("a\n\nb").concat(), "a\n\nb");
    }
}
