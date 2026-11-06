//! Matching document units to record cells by hash (EXR-14, design decisions 1, 2 and 4).
//!
//! Pure: no AST, no pipeline, no I/O. The splice stage computes one [`UnitKey`] per scanned
//! document unit (in document order, front-matter spans included) and hands them to
//! [`match_units`] with the record's cells. Matching never reads the index inside
//! `quarto_cell_id`, because that index shifts when a cell is added or removed.
//!
//! Order of keys: the execution hash first (it leaves out presentation options such as
//! `echo`), then the Positron-compatible content hash for what is left. A record cell with no
//! execution hash (an imported Positron record) is reachable only by content hash. Duplicates
//! match by occurrence: the k-th document unit with hash H takes the k-th record cell with
//! hash H, counting in document order and record order respectively.

use std::collections::{BTreeMap, HashMap, VecDeque};

use super::exec_hash::{HashError, UnitHashInput, hash_unit, partition_author_text};
use super::hash_definition::HashDefinition;
use super::ids::{content_hash, span_content_hash};
use super::types::{RecordCell, UnitKind};

/// The two hashes of one document unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitKey {
    pub kind: UnitKind,
    /// `quarto_exec_hash` under the unit's own engine's `HashDefinition`.
    pub exec_hash: String,
    /// `quarto_content_hash` over the author text, `#|` lines included.
    pub content_hash: String,
}

impl UnitKey {
    /// Hash a unit's author text the way the record writer does (`RecordBuilder::push`):
    /// a cell is partitioned into options and code, a span is its expression with no options.
    pub fn from_author_text(
        def: &HashDefinition,
        kind: UnitKind,
        lang: &str,
        author_text: &str,
        attr_tail: &str,
    ) -> Result<Self, HashError> {
        let (options, code, content) = match kind {
            UnitKind::Inline => (
                BTreeMap::new(),
                author_text.to_string(),
                span_content_hash(lang, author_text),
            ),
            UnitKind::Cell => {
                let (options, code) = partition_author_text(lang, author_text)?;
                (options, code, content_hash(author_text))
            }
        };
        let exec_hash = hash_unit(
            def,
            &UnitHashInput {
                kind,
                lang,
                code: &code,
                options: &options,
                profile: None,
                attr_tail,
            },
        )?;
        Ok(Self {
            kind,
            exec_hash,
            content_hash: content,
        })
    }
}

/// Per-unit staleness for a document unit (design decision 4). A record cell nothing matched
/// is reported separately, in [`Matching::orphaned`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitStaleness {
    /// Matched by the execution hash or the content hash.
    Fresh,
    /// No record cell matches by either hash.
    Stale,
}

/// The result of [`match_units`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matching {
    /// For each document unit (same order as the input), the index of its record cell.
    pub assignment: Vec<Option<usize>>,
    /// Record cells no document unit matched, in record order.
    pub orphaned: Vec<usize>,
}

impl Matching {
    pub fn staleness(&self, unit_index: usize) -> UnitStaleness {
        match self.assignment.get(unit_index) {
            Some(Some(_)) => UnitStaleness::Fresh,
            _ => UnitStaleness::Stale,
        }
    }
}

/// Match `units` (document order) to `cells` (record order).
pub fn match_units(units: &[UnitKey], cells: &[RecordCell]) -> Matching {
    let mut assignment: Vec<Option<usize>> = vec![None; units.len()];
    let mut claimed = vec![false; cells.len()];

    // Pass 1 keys on the execution hash, pass 2 on the content hash among the cells pass 1
    // left. A cell with no execution hash (empty) is never in pass 1's queues.
    for pass in [Pass::Exec, Pass::Content] {
        let mut queues: HashMap<(UnitKind, &str), VecDeque<usize>> = HashMap::new();
        for (i, cell) in cells.iter().enumerate() {
            if claimed[i] {
                continue;
            }
            let hash = pass.cell_hash(cell);
            if hash.is_empty() {
                continue;
            }
            queues
                .entry((cell.metadata.quarto_kind, hash))
                .or_default()
                .push_back(i);
        }
        for (u, unit) in units.iter().enumerate() {
            if assignment[u].is_some() {
                continue;
            }
            let Some(queue) = queues.get_mut(&(unit.kind, pass.unit_hash(unit))) else {
                continue;
            };
            if let Some(i) = queue.pop_front() {
                claimed[i] = true;
                assignment[u] = Some(i);
            }
        }
    }

    let orphaned = (0..cells.len()).filter(|i| !claimed[*i]).collect();
    Matching {
        assignment,
        orphaned,
    }
}

#[derive(Clone, Copy)]
enum Pass {
    Exec,
    Content,
}

impl Pass {
    fn cell_hash(self, cell: &RecordCell) -> &str {
        match self {
            Pass::Exec => &cell.metadata.quarto_exec_hash,
            Pass::Content => &cell.metadata.quarto_content_hash,
        }
    }

    fn unit_hash(self, unit: &UnitKey) -> &str {
        match self {
            Pass::Exec => &unit.exec_hash,
            Pass::Content => &unit.content_hash,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::types::CellMetadata;
    use crate::record::{CellType, Granularity};

    fn def() -> HashDefinition {
        let mut def = HashDefinition::conservative("jupyter");
        def.granularity = Granularity::Unit;
        def.presentation_cell_options = vec!["echo".into()];
        def
    }

    fn key(text: &str) -> UnitKey {
        UnitKey::from_author_text(&def(), UnitKind::Cell, "python", text, "").unwrap()
    }

    /// The brace tail is hashed on this side as on the writer's (EXR-11): editing it makes
    /// the exec hash differ, and an empty tail leaves the hash as it was.
    #[test]
    fn brace_tail_changes_the_exec_hash() {
        let hash = |tail: &str| {
            UnitKey::from_author_text(&def(), UnitKind::Cell, "python", "x", tail)
                .unwrap()
                .exec_hash
        };
        assert_ne!(hash(""), hash("echo=FALSE"));
        assert_eq!(hash(""), key("x").exec_hash);
    }

    fn cell_from(key: &UnitKey) -> RecordCell {
        RecordCell {
            cell_type: CellType::Code,
            execution_count: None,
            id: key.content_hash.clone(),
            metadata: CellMetadata {
                quarto_cell_id: "0-ignored-unlabeled".into(),
                quarto_content_hash: key.content_hash.clone(),
                quarto_label: None,
                quarto_kind: key.kind,
                quarto_lang: "python".into(),
                quarto_engine: None,
                quarto_kernel: None,
                quarto_location: None,
                quarto_exec_hash: key.exec_hash.clone(),
                quarto_hash_rule: None,
                extra: BTreeMap::new(),
            },
            outputs: vec![],
            source: vec![],
        }
    }

    fn record_of(texts: &[&str]) -> Vec<RecordCell> {
        texts.iter().map(|t| cell_from(&key(t))).collect()
    }

    fn keys_of(texts: &[&str]) -> Vec<UnitKey> {
        texts.iter().map(|t| key(t)).collect()
    }

    #[test]
    fn every_unit_matches_its_cell() {
        let cells = record_of(&["a = 1", "b = 2", "c = 3"]);
        let m = match_units(&keys_of(&["a = 1", "b = 2", "c = 3"]), &cells);
        assert_eq!(m.assignment, vec![Some(0), Some(1), Some(2)]);
        assert!(m.orphaned.is_empty());
        assert_eq!(m.staleness(1), UnitStaleness::Fresh);
    }

    #[test]
    fn an_edited_cell_is_stale_and_its_old_record_cell_is_orphaned() {
        let cells = record_of(&["a = 1", "b = 2", "c = 3"]);
        let m = match_units(&keys_of(&["a = 1", "b = 22", "c = 3"]), &cells);
        assert_eq!(m.assignment, vec![Some(0), None, Some(2)]);
        assert_eq!(m.orphaned, vec![1]);
        assert_eq!(m.staleness(1), UnitStaleness::Stale);
        assert_eq!(m.staleness(0), UnitStaleness::Fresh);
    }

    #[test]
    fn reordering_matches_every_cell() {
        let cells = record_of(&["a = 1", "b = 2", "c = 3"]);
        let m = match_units(&keys_of(&["c = 3", "a = 1", "b = 2"]), &cells);
        assert_eq!(m.assignment, vec![Some(2), Some(0), Some(1)]);
        assert!(m.orphaned.is_empty());
    }

    #[test]
    fn duplicates_match_in_occurrence_order() {
        let cells = record_of(&["x = 1", "x = 1"]);
        let m = match_units(&keys_of(&["x = 1", "x = 1"]), &cells);
        assert_eq!(m.assignment, vec![Some(0), Some(1)]);
    }

    #[test]
    fn deleting_the_first_duplicate_takes_the_first_record_cell_and_orphans_the_second() {
        // The ids in `quarto_cell_id` carry indexes that no longer line up; matching must
        // not look at them (every fixture cell has the same placeholder id).
        let cells = record_of(&["x = 1", "x = 1"]);
        let m = match_units(&keys_of(&["x = 1"]), &cells);
        assert_eq!(m.assignment, vec![Some(0)]);
        assert_eq!(m.orphaned, vec![1]);
    }

    #[test]
    fn a_presentation_option_edit_keeps_the_cell_matched() {
        let cells = record_of(&["#| echo: true\nx = 1"]);
        let edited = keys_of(&["#| echo: false\nx = 1"]);
        assert_ne!(
            edited[0].content_hash,
            cells[0].metadata.quarto_content_hash
        );
        let m = match_units(&edited, &cells);
        assert_eq!(m.assignment, vec![Some(0)]);
        assert_eq!(m.staleness(0), UnitStaleness::Fresh);
    }

    #[test]
    fn an_execution_relevant_option_edit_makes_the_cell_stale() {
        let cells = record_of(&["#| fig-width: 5\nx = 1"]);
        let m = match_units(&keys_of(&["#| fig-width: 7\nx = 1"]), &cells);
        assert_eq!(m.assignment, vec![None]);
        assert_eq!(m.orphaned, vec![0]);
        assert_eq!(m.staleness(0), UnitStaleness::Stale);
    }

    /// An imported Positron record has no execution hash, so only the content hash keys it,
    /// and a `#|` edit of any kind (the content hash includes those lines) strands the cell.
    #[test]
    fn a_cell_with_only_a_content_hash_matches_by_content_hash() {
        let mut cells = record_of(&["#| echo: true\nx = 1"]);
        cells[0].metadata.quarto_exec_hash = String::new();

        let same = match_units(&keys_of(&["#| echo: true\nx = 1"]), &cells);
        assert_eq!(same.assignment, vec![Some(0)]);

        let echo_edit = match_units(&keys_of(&["#| echo: false\nx = 1"]), &cells);
        assert_eq!(echo_edit.assignment, vec![None]);
        assert_eq!(echo_edit.staleness(0), UnitStaleness::Stale);
    }

    #[test]
    fn the_execution_hash_wins_when_the_two_hashes_disagree() {
        // Record cell 0: the unit's content hash but another unit's exec hash.
        // Record cell 1: the unit's exec hash but another content hash.
        let unit = key("x = 1");
        let other = key("y = 2");
        let mut by_content = cell_from(&unit);
        by_content.metadata.quarto_exec_hash = other.exec_hash.clone();
        let mut by_exec = cell_from(&other);
        by_exec.metadata.quarto_exec_hash = unit.exec_hash.clone();
        let m = match_units(&[unit], &[by_content, by_exec]);
        assert_eq!(m.assignment, vec![Some(1)]);
        assert_eq!(m.orphaned, vec![0]);
    }

    #[test]
    fn spans_and_cells_never_match_each_other() {
        let span = UnitKey::from_author_text(&def(), UnitKind::Inline, "python", "x", "").unwrap();
        let cells = record_of(&["x"]);
        let m = match_units(&[span], &cells);
        assert_eq!(m.assignment, vec![None]);
    }

    #[test]
    fn span_keys_ignore_language_case() {
        let a = UnitKey::from_author_text(&def(), UnitKind::Inline, "Python", "a", "").unwrap();
        let b = UnitKey::from_author_text(&def(), UnitKind::Inline, "python", "a", "").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn an_edited_second_span_leaves_the_first_matched() {
        let span = |code: &str| {
            UnitKey::from_author_text(&def(), UnitKind::Inline, "python", code, "").unwrap()
        };
        let cells: Vec<RecordCell> = [span("a"), span("a"), span("b")]
            .iter()
            .map(cell_from)
            .collect();
        let m = match_units(&[span("a"), span("a2"), span("b")], &cells);
        assert_eq!(m.assignment, vec![Some(0), None, Some(2)]);
        assert_eq!(m.orphaned, vec![1]);
    }

    /// The splice must hash a unit exactly as the writer recorded it: the builder's cell
    /// and the matcher's key agree.
    #[test]
    fn keys_equal_what_the_record_builder_writes() {
        use crate::engine::units::scan_units;
        use crate::record::{AuthorTexts, RecordBuilder, UnitResult, fill_ids};
        let doc = "```{python}\n#| fig-width: 7\nx = 1\n```\n\nSee `{python} x`.\n";
        let mut units = scan_units(doc);
        let texts = AuthorTexts::from_scanned(&units);
        fill_ids(&mut units, &texts).unwrap();
        let d = def();
        let mut builder = RecordBuilder::new("jupyter", &d, &units, &texts).unwrap();
        builder.push(0, UnitResult::default()).unwrap();
        builder.push(1, UnitResult::default()).unwrap();
        let cells = builder
            .finish(crate::record::EngineEntryInput::default())
            .unwrap()
            .cells;
        let cell_key = key("#| fig-width: 7\nx = 1");
        let span_key = UnitKey::from_author_text(&d, UnitKind::Inline, "python", "x", "").unwrap();
        let m = match_units(&[cell_key, span_key], &cells);
        assert_eq!(m.assignment, vec![Some(0), Some(1)]);
    }
}
