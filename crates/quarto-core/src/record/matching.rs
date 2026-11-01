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
    /// The exec hash under each named profile of the engine\'s declaration, with the current
    /// output format added under [`OUTPUT_FORMAT_OPTION`]. A record cell that names a profile
    /// is matched by one of these (KNR-3 decision 8). Empty for a unit built by
    /// [`UnitKey::from_author_text`].
    pub alt_exec_hashes: Vec<String>,
}

/// The reserved flat cell option that carries the output format on a cell that names a hash
/// profile (KNR-3 decision 8). It must not start with `quarto_`: `CellMetadata::options()` drops
/// every such key, and this one has to reach the hash. A profile that does not exclude it makes a
/// format change move the unit\'s hash.
pub const OUTPUT_FORMAT_OPTION: &str = "quarto-output-format";

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
            alt_exec_hashes: Vec::new(),
        })
    }

    /// [`Self::from_author_text`] plus one alternative exec hash per profile in `def.profiles`
    /// (cells only), each with the current `format` under [`OUTPUT_FORMAT_OPTION`]. The profile
    /// of the record cell this unit should match is not known here: the unit\'s id embeds its
    /// content hash, so it changes on exactly the edits that need the profile, and a lookup by
    /// id would miss. So every declared profile is tried, and the cell\'s own hash decides.
    pub fn from_author_text_with_profiles(
        def: &HashDefinition,
        kind: UnitKind,
        lang: &str,
        author_text: &str,
        attr_tail: &str,
        format: &str,
    ) -> Result<Self, HashError> {
        let mut key = Self::from_author_text(def, kind, lang, author_text, attr_tail)?;
        if kind == UnitKind::Cell {
            let (mut options, code) = partition_author_text(lang, author_text)?;
            options.insert(OUTPUT_FORMAT_OPTION.to_string(), format.into());
            for profile in def.profiles.keys() {
                key.alt_exec_hashes.push(hash_unit(
                    def,
                    &UnitHashInput {
                        kind,
                        lang,
                        code: &code,
                        options: &options,
                        profile: Some(profile),
                        attr_tail,
                    },
                )?);
            }
        }
        Ok(key)
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
    // left. A cell with no execution hash (empty) is never in pass 1's queues. A cell that
    // names a hash profile is never in pass 2's: its exec hash is the whole freshness rule (the
    // output format is in it, and the text hash alone would call a format change fresh).
    for pass in [Pass::Exec, Pass::Content] {
        let mut queues: HashMap<(UnitKind, &str), VecDeque<usize>> = HashMap::new();
        for (i, cell) in cells.iter().enumerate() {
            if claimed[i] {
                continue;
            }
            if matches!(pass, Pass::Content) && cell.metadata.quarto_hash_rule.is_some() {
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
            // Pass 1 also tries the profile hashes, after the default one.
            let alts: &[String] = match pass {
                Pass::Exec => &unit.alt_exec_hashes,
                Pass::Content => &[],
            };
            let hashes =
                std::iter::once(pass.unit_hash(unit)).chain(alts.iter().map(String::as_str));
            for hash in hashes {
                let Some(queue) = queues.get_mut(&(unit.kind, hash)) else {
                    continue;
                };
                if let Some(i) = queue.pop_front() {
                    claimed[i] = true;
                    assignment[u] = Some(i);
                    break;
                }
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

    // ---- KNR-3 decision 8: cells that name a hash profile.

    fn profiled_def() -> HashDefinition {
        let mut def = HashDefinition::conservative("knitr");
        def.profiles.insert(
            "structured".into(),
            crate::record::HashProfile {
                presentation_cell_options: vec!["echo".into()],
                presentation_execute_keys: vec![],
            },
        );
        def
    }

    fn profiled_key(text: &str, format: &str) -> UnitKey {
        UnitKey::from_author_text_with_profiles(
            &profiled_def(),
            UnitKind::Cell,
            "r",
            text,
            "",
            format,
        )
        .unwrap()
    }

    /// A record cell for `text` as the knitr adapter writes it: it names the profile and its
    /// hash counts the format.
    fn structured_cell(text: &str, format: &str) -> RecordCell {
        let key = profiled_key(text, format);
        let mut cell = cell_from(&key);
        cell.metadata.quarto_exec_hash = key.alt_exec_hashes[0].clone();
        cell.metadata.quarto_hash_rule = Some("structured".into());
        cell
    }

    #[test]
    fn a_unit_carries_one_alternative_hash_per_declared_profile() {
        let unit = profiled_key("x", "html");
        assert_eq!(unit.alt_exec_hashes.len(), 1);
        assert_ne!(
            unit.alt_exec_hashes[0], unit.exec_hash,
            "the format counts under the profile"
        );
        assert_ne!(
            unit.alt_exec_hashes,
            profiled_key("x", "typst").alt_exec_hashes
        );
        assert!(
            key("x").alt_exec_hashes.is_empty(),
            "no profiles declared, none carried"
        );
    }

    #[test]
    fn a_cell_naming_a_profile_matches_by_the_alternative_hash() {
        let cells = [structured_cell("#| echo: true\nx", "html")];
        let edited = profiled_key("#| echo: false\nx", "html");
        assert_eq!(match_units(&[edited], &cells).assignment, [Some(0)]);
    }

    #[test]
    fn a_cell_naming_a_profile_is_stale_for_another_format() {
        let cells = [structured_cell("x", "docx")];
        let m = match_units(&[profiled_key("x", "html")], &cells);
        assert_eq!(m.assignment, [None]);
        assert_eq!(m.orphaned, [0]);
    }

    /// The content-hash pass ignores a cell that names a profile: its text is unchanged here,
    /// but the format differs, and the text hash alone would call that fresh.
    #[test]
    fn pass_two_skips_a_cell_naming_a_profile() {
        let key = profiled_key("x", "html");
        let mut cell = structured_cell("x", "docx");
        assert_eq!(cell.metadata.quarto_content_hash, key.content_hash);
        assert_eq!(
            match_units(&[key.clone()], &[cell.clone()]).assignment,
            [None]
        );
        cell.metadata.quarto_hash_rule = None;
        assert_eq!(
            match_units(&[key], &[cell]).assignment,
            [Some(0)],
            "without the rule, pass 2 matches"
        );
    }
}
