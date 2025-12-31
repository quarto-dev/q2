//! Core types for citation processing.

use crate::Result;
use crate::locale::LocaleManager;
use crate::output::QuoteConfig;
use crate::reference::Reference;
use hashlink::LinkedHashMap;
use quarto_csl::Style;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

/// A computed sort key value with its sort direction.
#[derive(Debug, Clone)]
pub struct SortKeyValue {
    /// The computed string value for sorting.
    pub value: String,
    /// Whether this key sorts in descending order.
    pub descending: bool,
}

/// Compare two sets of sort keys.
/// Per CSL spec and Pandoc citeproc: empty/missing values sort AFTER non-empty values.
/// The descending flag only affects comparison of non-empty values, not empty-vs-non-empty.
pub fn compare_sort_keys(a: &[SortKeyValue], b: &[SortKeyValue]) -> Ordering {
    for (ka, kb) in a.iter().zip(b.iter()) {
        // Normalize for comparison: lowercase and strip non-alphabetic leading/trailing chars
        let va = normalize_for_sort(&ka.value);
        let vb = normalize_for_sort(&kb.value);

        // Empty values sort AFTER non-empty values (like Haskell's Nothing vs Just)
        // This is NOT affected by the descending flag - empty always sorts last.
        // See: sort_StatusFieldAscending.txt, sort_StatusFieldDescending.txt
        let cmp = match (va.is_empty(), vb.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater, // empty sorts after non-empty (always)
            (false, true) => Ordering::Less,    // non-empty sorts before empty (always)
            (false, false) => {
                // Only reverse for descending when BOTH values are non-empty
                let value_cmp = va.cmp(&vb);
                if ka.descending {
                    value_cmp.reverse()
                } else {
                    value_cmp
                }
            }
        };

        if cmp != Ordering::Equal {
            return cmp;
        }
    }
    // If all compared keys are equal, compare by length (more keys = greater)
    a.len().cmp(&b.len())
}

/// Strip HTML tags from a string.
fn strip_html_tags(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(c),
            _ => {}
        }
    }
    result
}

/// Check if a character is a word separator for sort key normalization.
/// Based on Haskell citeproc's normalizeSortKey: splits on spaces, quotes (various types),
/// commas, and Arabic transliteration marks.
/// Also includes brackets which the old implementation filtered out.
fn is_sort_word_separator(c: char) -> bool {
    c.is_whitespace()
        || c == '\'' // ASCII single quote
        || c == '\u{2019}' // RIGHT SINGLE QUOTATION MARK
        || c == '\u{2018}' // LEFT SINGLE QUOTATION MARK
        || c == '\u{201C}' // LEFT DOUBLE QUOTATION MARK
        || c == '\u{201D}' // RIGHT DOUBLE QUOTATION MARK
        || c == '"'  // ASCII double quote
        || c == ','
        || c == '[' // brackets (previously filtered out)
        || c == ']'
        || c == '\u{02BE}' // MODIFIER LETTER RIGHT HALF RING (ayn in transliterated Arabic)
        || c == '\u{02BF}' // MODIFIER LETTER LEFT HALF RING (hamza in transliterated Arabic)
}

/// Normalize a string for sort comparison.
/// Based on Haskell citeproc's normalizeSortKey: strips HTML tags, splits on word separators
/// (quotes, commas, spaces, etc.), case-folds, and joins with spaces.
fn normalize_for_sort(s: &str) -> String {
    // First strip HTML tags
    let s = strip_html_tags(s);
    // Split on word separators (like Haskell's T.split isWordSep), filter empty, join
    // This handles quotes, commas, spaces, and special marks
    s.split(is_sort_word_separator)
        .filter(|word| !word.is_empty())
        .map(|word| word.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Convert a date part (year, month, day) to a sortable string.
/// Following Pandoc citeproc's dateToText:
/// - Positive years: P{9-digit-year}{2-digit-month}{2-digit-day}
/// - Negative (BC) years: N{999999999+year}{2-digit-month}{2-digit-day}
/// This ensures negative years sort before positive, and within each category
/// dates sort chronologically.
fn date_part_to_sort_string(year: i32, month: i32, day: i32) -> String {
    let (prefix, sort_year) = if year < 0 {
        // Negative (BC) years: N prefix, offset to make them sort correctly
        // -100 → N999999899, -1 → N999999998, 0 → P000000000
        ('N', (999_999_999 + year) as u32)
    } else {
        // Positive (AD) years: P prefix
        ('P', year as u32)
    };
    format!(
        "{}{:09}{:02}{:02}",
        prefix,
        sort_year,
        month.max(0) as u32,
        day.max(0) as u32
    )
}

/// A citation request.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Citation {
    /// Optional citation ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Note number (for note-based styles).
    #[serde(rename = "noteNumber", skip_serializing_if = "Option::is_none")]
    pub note_number: Option<i32>,

    /// Citation items (references being cited).
    #[serde(rename = "citationItems")]
    pub items: Vec<CitationItem>,
}

/// A single item within a citation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CitationItem {
    /// Reference ID.
    pub id: String,

    /// Locator type (e.g., "page", "chapter", "section").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locator: Option<String>,

    /// Locator value (e.g., "42-45").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,

    /// Prefix text (e.g., "see").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,

    /// Suffix text (e.g., "for details").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suffix: Option<String>,

    /// Suppress author in output.
    #[serde(rename = "suppress-author", skip_serializing_if = "Option::is_none")]
    pub suppress_author: Option<bool>,

    /// Author only (no date/title).
    #[serde(rename = "author-only", skip_serializing_if = "Option::is_none")]
    pub author_only: Option<bool>,

    /// Citation position flags (for note-style citations).
    /// Bitmask: 1=First, 2=Subsequent, 4=Ibid, 8=IbidWithLocator, 16=NearNote
    /// Multiple positions can be true at once (e.g., Ibid + NearNote + Subsequent).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<i32>,
}

/// Position bitmask constants for serialization.
/// Internal representation uses Vec<Position>, but JSON uses i32 for compatibility.
pub mod position_bitmask {
    pub const FIRST: i32 = 0x100;
    pub const SUBSEQUENT: i32 = 0x200;
    pub const IBID: i32 = 0x400;
    pub const IBID_WITH_LOCATOR: i32 = 0x800;
    pub const NEAR_NOTE: i32 = 0x1000;
}

/// Convert a list of positions to a bitmask for serialization.
pub fn positions_to_bitmask(positions: &[quarto_csl::Position]) -> i32 {
    use quarto_csl::Position;
    let mut flags = 0;
    for pos in positions {
        flags |= match pos {
            Position::First => position_bitmask::FIRST,
            Position::Subsequent => position_bitmask::SUBSEQUENT,
            Position::Ibid => position_bitmask::IBID,
            Position::IbidWithLocator => position_bitmask::IBID_WITH_LOCATOR,
            Position::NearNote => position_bitmask::NEAR_NOTE,
        };
    }
    flags
}

/// Convert a bitmask (or legacy value) to a list of positions.
pub fn bitmask_to_positions(value: i32) -> Vec<quarto_csl::Position> {
    use quarto_csl::Position;

    // Handle legacy format (0-4) vs new bitmask format (>= 0x100)
    if value <= 4 {
        match value {
            0 => vec![Position::First],
            1 => vec![Position::Subsequent],
            2 => vec![Position::Ibid, Position::Subsequent],
            3 => vec![
                Position::IbidWithLocator,
                Position::Ibid,
                Position::Subsequent,
            ],
            4 => vec![Position::NearNote, Position::Subsequent],
            _ => vec![Position::First],
        }
    } else {
        // New bitmask format
        let mut positions = Vec::new();
        if (value & position_bitmask::FIRST) != 0 {
            positions.push(Position::First);
        }
        if (value & position_bitmask::SUBSEQUENT) != 0 {
            positions.push(Position::Subsequent);
        }
        if (value & position_bitmask::IBID) != 0 {
            positions.push(Position::Ibid);
        }
        if (value & position_bitmask::IBID_WITH_LOCATOR) != 0 {
            positions.push(Position::IbidWithLocator);
        }
        if (value & position_bitmask::NEAR_NOTE) != 0 {
            positions.push(Position::NearNote);
        }
        positions
    }
}

/// Citation processor that applies CSL styles to references.
pub struct Processor {
    /// The CSL style to use.
    pub style: Style,

    /// Locale manager for term lookup.
    pub locales: LocaleManager,

    /// References by ID (preserves insertion order).
    references: LinkedHashMap<String, Reference>,

    /// Initial citation numbers (assigned based on citation order, used for sorting).
    initial_citation_numbers: LinkedHashMap<String, i32>,

    /// Final citation numbers (reassigned after bibliography sorting, used for rendering).
    /// If None, falls back to initial_citation_numbers.
    final_citation_numbers: Option<LinkedHashMap<String, i32>>,

    /// Next citation number for initial assignment.
    next_citation_number: i32,

    /// Citation history tracking for position calculation.
    /// Maps (note_index, item_id) to citation info for ibid detection.
    citation_history: CitationHistory,
}

// Position is represented as Vec<quarto_csl::Position> internally,
// matching the citeproc reference implementation.

/// Tracks citation history for position calculation.
#[derive(Default)]
struct CitationHistory {
    /// Last item cited in each note (for ibid detection within same note).
    /// Maps note_index -> (item_id, locator, label)
    last_item_in_note: LinkedHashMap<i32, (String, Option<String>, Option<String>)>,
    /// The globally last item cited in the immediately previous citation.
    /// For ibid detection across notes. Only tracked for single-item citations.
    /// (item_id, locator, label, note_index)
    last_single_citation_item: Option<(String, Option<String>, Option<String>, i32)>,
    /// Last citation info for each item (for near-note and subsequent detection).
    /// Maps item_id -> (note_index, locator, label)
    last_cited: LinkedHashMap<String, (i32, Option<String>, Option<String>)>,
}

impl Processor {
    /// Create a new processor with a CSL style.
    pub fn new(style: Style) -> Self {
        let default_locale = style.default_locale.clone();
        Self {
            style,
            locales: LocaleManager::new(default_locale),
            references: LinkedHashMap::new(),
            initial_citation_numbers: LinkedHashMap::new(),
            final_citation_numbers: None,
            next_citation_number: 1,
            citation_history: CitationHistory::default(),
        }
    }

    /// Calculate positions for a citation item based on citation history.
    /// Returns a list of positions that are true for this citation,
    /// matching the citeproc reference implementation.
    ///
    /// Position hierarchy (per CSL spec):
    /// - ibid-with-locator implies ibid implies subsequent
    /// - near-note implies subsequent
    /// - Multiple positions can be true simultaneously (e.g., [NearNote, Ibid, Subsequent])
    fn calculate_position(
        &self,
        note_index: i32,
        item_id: &str,
        locator: Option<&str>,
        label: Option<&str>,
        is_single_item_citation: bool,
        near_note_distance: u32,
    ) -> Vec<quarto_csl::Position> {
        use quarto_csl::Position;

        // Check if item was ever cited before
        let prev_citation = self.citation_history.last_cited.get(item_id);
        if prev_citation.is_none() {
            return vec![Position::First];
        }

        // Start with Subsequent as the base (like citeproc reference impl)
        let mut positions = vec![Position::Subsequent];

        // Get previous citation info
        let (prev_note, prev_locator, prev_label) = prev_citation.unwrap();

        // Check for near-note: previous citation within near_note_distance
        // CSL spec: "does not precede by more than near_note_distance notes"
        // This means distance <= near_note_distance (e.g., distance=0 with near-note-distance=0 is valid)
        let distance = (note_index - prev_note).abs();
        if distance <= near_note_distance as i32 {
            positions.push(Position::NearNote);
        }

        // Check for ibid within the same note
        let is_ibid_same_note = if let Some((last_item_id, _, _)) =
            self.citation_history.last_item_in_note.get(&note_index)
        {
            last_item_id == item_id
        } else {
            false
        };

        // Check for ibid across notes (single-item citations only)
        let is_ibid_cross_note = if is_single_item_citation {
            if let Some((last_item_id, _, _, _)) = &self.citation_history.last_single_citation_item
            {
                last_item_id == item_id
            } else {
                false
            }
        } else {
            false
        };

        if is_ibid_same_note || is_ibid_cross_note {
            // Determine if ibid or ibid-with-locator
            let locator_changed =
                locator != prev_locator.as_deref() || label != prev_label.as_deref();
            if locator.is_some() && locator_changed {
                positions.push(Position::IbidWithLocator);
                positions.push(Position::Ibid);
            } else {
                positions.push(Position::Ibid);
            }
        }

        positions
    }

    /// Update citation history after processing a citation item.
    fn update_citation_history(
        &mut self,
        note_index: i32,
        item_id: &str,
        locator: Option<&str>,
        label: Option<&str>,
        is_single_item_citation: bool,
    ) {
        // Track last citation of each item (for subsequent and near-note detection)
        self.citation_history.last_cited.insert(
            item_id.to_string(),
            (
                note_index,
                locator.map(|s| s.to_string()),
                label.map(|s| s.to_string()),
            ),
        );
        // Track last item in each note (for same-note ibid detection)
        self.citation_history.last_item_in_note.insert(
            note_index,
            (
                item_id.to_string(),
                locator.map(|s| s.to_string()),
                label.map(|s| s.to_string()),
            ),
        );
        // Track globally last single-item citation for cross-note ibid
        if is_single_item_citation {
            self.citation_history.last_single_citation_item = Some((
                item_id.to_string(),
                locator.map(|s| s.to_string()),
                label.map(|s| s.to_string()),
                note_index,
            ));
        }
    }

    /// Reset citation history (useful for tests).
    pub fn reset_citation_history(&mut self) {
        self.citation_history = CitationHistory::default();
    }

    /// Add a reference to the processor.
    pub fn add_reference(&mut self, mut reference: Reference) {
        // Extract particles from name fields at parse time (matches Haskell citeproc behavior)
        // e.g., "Givenname al" -> given="Givenname", dropping_particle="al"
        // e.g., "van Gogh" -> non_dropping_particle="van", family="Gogh"
        reference.extract_all_particles();
        self.references.insert(reference.id.clone(), reference);
    }

    /// Copy initial citation numbers from another processor.
    /// Used when evaluating macros for sorting.
    pub fn copy_initial_citation_numbers(&mut self, other: &Processor) {
        self.initial_citation_numbers = other.initial_citation_numbers.clone();
        self.next_citation_number = other.next_citation_number;
    }

    /// Add multiple references.
    pub fn add_references(&mut self, references: impl IntoIterator<Item = Reference>) {
        for reference in references {
            self.add_reference(reference);
        }
    }

    /// Get a reference by ID.
    pub fn get_reference(&self, id: &str) -> Option<&Reference> {
        self.references.get(id)
    }

    /// Get or assign an initial citation number for a reference.
    /// This is used during citation processing and for sorting.
    pub fn get_initial_citation_number(&mut self, id: &str) -> i32 {
        if let Some(&num) = self.initial_citation_numbers.get(id) {
            num
        } else {
            let num = self.next_citation_number;
            self.next_citation_number += 1;
            self.initial_citation_numbers.insert(id.to_string(), num);
            num
        }
    }

    /// Get the final citation number for rendering.
    /// Uses reassigned numbers if available, otherwise falls back to initial numbers.
    pub fn get_citation_number(&self, id: &str) -> Option<i32> {
        if let Some(ref final_nums) = self.final_citation_numbers {
            final_nums.get(id).copied()
        } else {
            self.initial_citation_numbers.get(id).copied()
        }
    }

    /// Reassign citation numbers based on the final bibliography order.
    /// Called after bibliography sorting to update numbers for rendering.
    pub fn reassign_citation_numbers(&mut self, sorted_ids: &[String]) {
        let mut final_numbers = LinkedHashMap::new();
        for (index, id) in sorted_ids.iter().enumerate() {
            final_numbers.insert(id.clone(), (index + 1) as i32);
        }
        self.final_citation_numbers = Some(final_numbers);
    }

    /// Process a citation and return formatted output.
    ///
    /// Returns a string representation of the formatted citation.
    pub fn process_citation(&mut self, citation: &Citation) -> Result<String> {
        let output = self.process_citation_to_output(citation)?;
        Ok(output.render())
    }

    /// Process a citation and return the Output AST.
    ///
    /// This is the lower-level API that returns the intermediate representation.
    /// Use `to_inlines()` on the result to convert to Pandoc Inlines.
    pub fn process_citation_to_output(
        &mut self,
        citation: &Citation,
    ) -> Result<crate::output::Output> {
        crate::eval::evaluate_citation_to_output(self, citation)
    }

    /// Process a citation WITHOUT applying collapse logic.
    ///
    /// This is used for disambiguation detection, which needs to see each item's
    /// full rendered form before any name suppression from collapsing.
    fn process_citation_to_output_no_collapse(
        &mut self,
        citation: &Citation,
    ) -> Result<crate::output::Output> {
        crate::eval::evaluate_citation_to_output_no_collapse(self, citation)
    }

    /// Generate a bibliography entry for a reference.
    pub fn format_bibliography_entry(&mut self, id: &str) -> Result<Option<String>> {
        if self.style.bibliography.is_none() {
            return Ok(None);
        }

        let reference = self
            .get_reference(id)
            .ok_or_else(|| crate::Error::ReferenceNotFound {
                id: id.to_string(),
                location: None,
            })?
            .clone();

        crate::eval::evaluate_bibliography_entry(self, &reference).map(Some)
    }

    /// Generate a bibliography entry for a reference, returning the Output AST.
    pub fn format_bibliography_entry_to_output(
        &mut self,
        id: &str,
    ) -> Result<Option<crate::output::Output>> {
        if self.style.bibliography.is_none() {
            return Ok(None);
        }

        let reference = self
            .get_reference(id)
            .ok_or_else(|| crate::Error::ReferenceNotFound {
                id: id.to_string(),
                location: None,
            })?
            .clone();

        crate::eval::evaluate_bibliography_entry_to_output(self, &reference).map(Some)
    }

    /// Generate the full bibliography.
    pub fn generate_bibliography(&mut self) -> Result<Vec<(String, String)>> {
        let bib = match &self.style.bibliography {
            Some(b) => b,
            None => return Ok(Vec::new()),
        };

        // LinkedHashMap preserves insertion order
        let ids: Vec<String> = self.references.keys().cloned().collect();

        // Get sort keys from bibliography
        let sort_keys = bib.sort.as_ref().map(|s| &s.keys[..]).unwrap_or(&[]);

        // Check if any sort key uses citation-number
        let uses_citation_number = sort_keys.iter().any(
            |k| matches!(&k.key, quarto_csl::SortKeyType::Variable(v) if v == "citation-number"),
        );

        // If there are sort keys, sort the IDs; otherwise preserve insertion order
        let final_ids = if sort_keys.is_empty() {
            ids
        } else {
            // Compute sort key values for each reference
            let mut sorted_ids: Vec<(String, Vec<SortKeyValue>)> = ids
                .into_iter()
                .map(|id| {
                    let keys = self.compute_sort_keys(&id, sort_keys);
                    (id, keys)
                })
                .collect();

            // Sort by the computed keys
            sorted_ids.sort_by(|a, b| compare_sort_keys(&a.1, &b.1));

            sorted_ids.into_iter().map(|(id, _)| id).collect()
        };

        // Determine whether to reassign citation numbers:
        // - If citation-number is the ONLY sort key, don't reassign (keep original numbers)
        // - If there are multiple sort keys or citation-number is secondary, reassign
        let is_citation_number_only = sort_keys.len() == 1
            && matches!(&sort_keys[0].key, quarto_csl::SortKeyType::Variable(v) if v == "citation-number");

        if uses_citation_number && !sort_keys.is_empty() && !is_citation_number_only {
            self.reassign_citation_numbers(&final_ids);
        }

        // Format entries in order
        let mut entries = Vec::new();
        for id in &final_ids {
            if let Some(formatted) = self.format_bibliography_entry(id)? {
                entries.push((id.clone(), formatted));
            }
        }

        Ok(entries)
    }

    /// Generate the full bibliography, returning Output AST for each entry.
    ///
    /// This is the lower-level API that returns the intermediate representation.
    /// Use `to_inlines()` on each Output to convert to Pandoc Inlines.
    pub fn generate_bibliography_to_outputs(
        &mut self,
    ) -> Result<Vec<(String, crate::output::Output)>> {
        // Extract values from bibliography before mutable operations
        let (sort_keys_opt, subsequent_author_substitute, subsequent_author_substitute_rule) =
            match &self.style.bibliography {
                Some(b) => (
                    b.sort.clone(),
                    b.subsequent_author_substitute.clone(),
                    b.subsequent_author_substitute_rule,
                ),
                None => return Ok(Vec::new()),
            };

        // LinkedHashMap preserves insertion order
        let ids: Vec<String> = self.references.keys().cloned().collect();

        // Get sort keys from bibliography
        let sort_keys = sort_keys_opt.as_ref().map(|s| &s.keys[..]).unwrap_or(&[]);

        // Check if any sort key uses citation-number
        let uses_citation_number = sort_keys.iter().any(
            |k| matches!(&k.key, quarto_csl::SortKeyType::Variable(v) if v == "citation-number"),
        );

        // If there are sort keys, sort the IDs; otherwise preserve insertion order
        let final_ids = if sort_keys.is_empty() {
            ids
        } else {
            // Compute sort key values for each reference
            let mut sorted_ids: Vec<(String, Vec<SortKeyValue>)> = ids
                .into_iter()
                .map(|id| {
                    let keys = self.compute_sort_keys(&id, sort_keys);
                    (id, keys)
                })
                .collect();

            // Sort by the computed keys
            sorted_ids.sort_by(|a, b| compare_sort_keys(&a.1, &b.1));

            sorted_ids.into_iter().map(|(id, _)| id).collect()
        };

        // Determine whether to reassign citation numbers:
        // - If citation-number is the ONLY sort key, don't reassign (keep original numbers)
        // - If there are multiple sort keys or citation-number is secondary, reassign
        let is_citation_number_only = sort_keys.len() == 1
            && matches!(&sort_keys[0].key, quarto_csl::SortKeyType::Variable(v) if v == "citation-number");

        if uses_citation_number && !sort_keys.is_empty() && !is_citation_number_only {
            self.reassign_citation_numbers(&final_ids);
        }

        // Format entries in order
        let mut entries = Vec::new();
        for id in &final_ids {
            if let Some(output) = self.format_bibliography_entry_to_output(id)? {
                entries.push((id.clone(), output));
            }
        }

        // Apply subsequent-author-substitute if configured
        let entries = if let Some(ref substitute) = subsequent_author_substitute {
            crate::output::apply_subsequent_author_substitute(
                entries,
                substitute,
                subsequent_author_substitute_rule,
            )
        } else {
            entries
        };

        Ok(entries)
    }

    /// Compute sort key values for a reference.
    pub fn compute_sort_keys(
        &self,
        id: &str,
        sort_keys: &[quarto_csl::SortKey],
    ) -> Vec<SortKeyValue> {
        let reference = match self.get_reference(id) {
            Some(r) => r,
            None => return vec![],
        };

        sort_keys
            .iter()
            .map(|key| {
                let value = match &key.key {
                    quarto_csl::SortKeyType::Variable(var) => {
                        self.get_sort_value_for_variable(reference, var)
                    }
                    quarto_csl::SortKeyType::Macro(name) => {
                        self.get_sort_value_for_macro(reference, name, key)
                    }
                };
                SortKeyValue {
                    value,
                    descending: key.sort_order == quarto_csl::SortOrder::Descending,
                }
            })
            .collect()
    }

    /// Get the sort value for a variable.
    fn get_sort_value_for_variable(&self, reference: &Reference, var: &str) -> String {
        match var {
            // Name variables - extract family names for sorting
            "author" | "editor" | "translator" | "director" | "interviewer" | "illustrator"
            | "composer" | "collection-editor" | "container-author" => {
                if let Some(names) = reference.get_names(var) {
                    names
                        .iter()
                        .filter_map(|n| n.literal.as_ref().or(n.family.as_ref()).cloned())
                        .collect::<Vec<_>>()
                        .join(" ")
                } else {
                    String::new()
                }
            }
            // Date variables - format as sortable string
            // Following Pandoc citeproc: concatenate ALL date parts (for date ranges)
            // Format: P{9-digit-year}{2-digit-month}{2-digit-day} for positive years
            //         N{999999999+year}{2-digit-month}{2-digit-day} for negative (BC) years
            // This ensures: negative years sort before positive, and within each category
            // dates sort chronologically.
            "issued" | "accessed" | "event-date" | "original-date" | "submitted" => {
                if let Some(date) = reference.get_date(var) {
                    if let Some(all_parts) = date.date_parts.as_ref() {
                        // Concatenate sortable strings for ALL date parts (handles ranges)
                        all_parts
                            .iter()
                            .map(|parts| {
                                let year = parts.first().copied().unwrap_or(0);
                                let month = parts.get(1).copied().unwrap_or(0);
                                let day = parts.get(2).copied().unwrap_or(0);
                                date_part_to_sort_string(year, month, day)
                            })
                            .collect::<String>()
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                }
            }
            // Citation number - use initial assignment for sorting
            "citation-number" => {
                if let Some(&num) = self.initial_citation_numbers.get(&reference.id) {
                    // Zero-pad for proper string sorting (up to 10 digits)
                    format!("{:010}", num)
                } else {
                    // If not yet assigned, use a high value to sort last
                    format!("{:010}", i32::MAX)
                }
            }
            // String variables
            _ => reference.get_variable(var).unwrap_or_default(),
        }
    }

    /// Get the sort value by evaluating a macro.
    fn get_sort_value_for_macro(
        &self,
        reference: &Reference,
        macro_name: &str,
        sort_key: &quarto_csl::SortKey,
    ) -> String {
        // Evaluate the macro and return plain text (stripping formatting)
        // For now, just try to evaluate it using the bibliography context
        if let Some(macro_def) = self.style.macros.get(macro_name) {
            // Create a minimal context and evaluate
            // The sort_key carries name formatting overrides (names-min, etc.)
            crate::eval::evaluate_macro_for_sort(self, reference, &macro_def.elements, sort_key)
                .unwrap_or_default()
        } else {
            String::new()
        }
    }

    /// Get a term from the locale.
    pub fn get_term(&self, name: &str, form: quarto_csl::TermForm, plural: bool) -> Option<String> {
        // First check style-level locale overrides
        for locale in &self.style.locales {
            for term in &locale.terms {
                if term.name == name && term.form == form {
                    if plural {
                        if let Some(ref m) = term.multiple {
                            return Some(m.clone());
                        }
                    } else {
                        if let Some(ref s) = term.single {
                            return Some(s.clone());
                        }
                    }
                    if let Some(ref v) = term.value {
                        return Some(v.clone());
                    }
                }
            }
        }

        // Fall back to locale manager
        self.locales.get_term(name, form, plural)
    }

    /// Get a date format from the locale.
    ///
    /// This follows the CSL spec locale priority order:
    /// 1. Style locale with exact language match (e.g., xml:lang="en-US" for en-US)
    /// 2. Style locale with base language match (e.g., xml:lang="en" for en-US)
    /// 3. Style locale with no language (fallback locale)
    /// 4. External locale files from the locale manager
    pub fn get_date_format(&self, form: quarto_csl::DateForm) -> Option<&quarto_csl::DateFormat> {
        // Get the effective language for locale lookup
        let effective_lang = self.style.default_locale.as_deref().unwrap_or("en-US");

        // Extract base language (e.g., "en" from "en-US")
        let base_lang = effective_lang.split('-').next().unwrap_or(effective_lang);

        // Priority 1: Exact language match (e.g., xml:lang="en-US" for en-US)
        for locale in &self.style.locales {
            if let Some(ref lang) = locale.lang {
                if lang == effective_lang {
                    for df in &locale.date_formats {
                        if df.form == form {
                            return Some(df);
                        }
                    }
                }
            }
        }

        // Priority 2: Base language match (e.g., xml:lang="en" for en-US)
        // Only if it differs from the exact match
        if base_lang != effective_lang {
            for locale in &self.style.locales {
                if let Some(ref lang) = locale.lang {
                    if lang == base_lang {
                        for df in &locale.date_formats {
                            if df.form == form {
                                return Some(df);
                            }
                        }
                    }
                }
            }
        }

        // Priority 3: Style locale with no xml:lang (fallback locale)
        for locale in &self.style.locales {
            if locale.lang.is_none() {
                for df in &locale.date_formats {
                    if df.form == form {
                        return Some(df);
                    }
                }
            }
        }

        // Fall back to locale manager (external locale files)
        self.locales.get_date_format(form)
    }

    /// Check if day ordinals should be limited to day 1 only.
    /// This checks locale overrides first, then falls back to style options.
    pub fn limit_day_ordinals_to_day_1(&self) -> bool {
        // First check style-level locale overrides
        for locale in &self.style.locales {
            if let Some(ref opts) = locale.options {
                return opts.limit_day_ordinals_to_day_1;
            }
        }

        // Fall back to style options
        self.style.options.limit_day_ordinals_to_day_1
    }

    /// Check if punctuation should be moved inside quotes.
    /// This checks style-level locale overrides first, then external locale files,
    /// then falls back to style options.
    /// When true, periods and commas following a closing quote are moved inside.
    pub fn punctuation_in_quote(&self) -> bool {
        // First check style-level locale overrides
        for locale in &self.style.locales {
            if let Some(ref opts) = locale.options {
                return opts.punctuation_in_quote;
            }
        }

        // Check external locale files
        if let Some(punct_in_quote) = self.locales.get_punctuation_in_quote() {
            return punct_in_quote;
        }

        // Fall back to style options
        self.style.options.punctuation_in_quote
    }

    /// Get locale-specific quote configuration.
    ///
    /// This returns the appropriate quote characters for the current locale,
    /// allowing proper rendering of quotation marks (e.g., English "..." vs French « ... »).
    pub fn get_quote_config(&self) -> QuoteConfig {
        self.locales.get_quote_config()
    }

    /// Get the bibliography sort key for a reference.
    ///
    /// This is used for year suffix assignment, which needs to follow
    /// bibliography order.
    pub fn get_bib_sort_key(&self, id: &str) -> Vec<SortKeyValue> {
        let bib = match &self.style.bibliography {
            Some(b) => b,
            None => return vec![],
        };

        let sort_keys = bib.sort.as_ref().map(|s| &s.keys[..]).unwrap_or(&[]);
        self.compute_sort_keys(id, sort_keys)
    }

    /// Set the year suffix for a reference.
    ///
    /// This updates the reference's disambiguation data with the assigned suffix.
    pub fn set_year_suffix(&mut self, id: &str, suffix: i32) {
        if let Some(reference) = self.references.get_mut(id) {
            let disamb = reference
                .disambiguation
                .get_or_insert_with(crate::reference::DisambiguationData::default);
            disamb.year_suffix = Some(suffix);
        }
    }

    /// Set the et-al names override for a reference.
    ///
    /// This sets the number of names to show instead of using et-al truncation.
    pub fn set_et_al_names(&mut self, id: &str, count: u32) {
        if let Some(reference) = self.references.get_mut(id) {
            let disamb = reference
                .disambiguation
                .get_or_insert_with(crate::reference::DisambiguationData::default);
            disamb.et_al_names = Some(count);
        }
    }

    /// Set a name hint for disambiguation.
    ///
    /// This stores a hint for how to render a specific name for a reference.
    pub fn set_name_hint(
        &mut self,
        id: &str,
        name: &crate::reference::Name,
        hint: crate::reference::NameHint,
    ) {
        if let Some(reference) = self.references.get_mut(id) {
            let disamb = reference
                .disambiguation
                .get_or_insert_with(crate::reference::DisambiguationData::default);
            // Use a key based on family name (or literal) to identify the name
            let key = name
                .family
                .clone()
                .or_else(|| name.literal.clone())
                .unwrap_or_default();
            disamb.name_hints.insert(key, hint);
        }
    }

    /// Set the disambiguate condition for a reference.
    ///
    /// This marks whether `disambiguate="true"` conditions should match.
    pub fn set_disamb_condition(&mut self, id: &str, value: bool) {
        if let Some(reference) = self.references.get_mut(id) {
            let disamb = reference
                .disambiguation
                .get_or_insert_with(crate::reference::DisambiguationData::default);
            disamb.disamb_condition = value;
        }
    }

    /// Get a mutable reference to a reference by ID.
    pub fn get_reference_mut(&mut self, id: &str) -> Option<&mut Reference> {
        self.references.get_mut(id)
    }

    /// Get all reference IDs.
    pub fn reference_ids(&self) -> impl Iterator<Item = &str> {
        self.references.keys().map(|s| s.as_str())
    }

    /// Process multiple citations with disambiguation enabled.
    ///
    /// This is the main API for processing citations when disambiguation is needed.
    /// It performs a two-pass rendering:
    /// 1. First pass: render all citations to detect ambiguities
    /// 2. Apply disambiguation methods (year suffixes, name expansion, etc.)
    /// 3. Second pass: re-render with disambiguation applied
    ///
    /// # Returns
    /// A vector of formatted citation strings, one per input citation.
    pub fn process_citations_with_disambiguation(
        &mut self,
        citations: &[Citation],
    ) -> Result<Vec<String>> {
        // Use the Output-based version for proper name extraction
        let outputs = self.process_citations_with_disambiguation_to_outputs(citations)?;
        Ok(outputs.iter().map(|o| o.render()).collect())
    }

    /// Process multiple citations with disambiguation, returning Output AST.
    ///
    /// This is the lower-level version that returns the intermediate representation.
    /// It uses the Output AST for proper name extraction during disambiguation.
    ///
    /// Note: This function ALWAYS performs disambiguation detection, even if no
    /// explicit methods (year-suffix, add-names, add-givenname) are enabled.
    /// This is required for the `<if disambiguate="true">` condition to work.
    ///
    /// The disambiguation algorithm follows CSL spec order:
    /// 1. Apply givenname disambiguation (global or by-cite)
    /// 2. Re-render and refresh ambiguities
    /// 3. If still ambiguous, add names (expand et-al)
    /// 4. Re-render and refresh ambiguities
    /// 5. If still ambiguous, add year suffixes
    /// 6. Set disambiguate condition for any remaining ambiguities
    pub fn process_citations_with_disambiguation_to_outputs(
        &mut self,
        citations: &[Citation],
    ) -> Result<Vec<crate::output::Output>> {
        use crate::disambiguation::{
            apply_global_name_disambiguation, assign_year_suffixes, extract_disamb_data,
            extract_disamb_data_with_processor, find_ambiguities,
            find_year_suffix_with_full_author_match, merge_ambiguity_groups,
            set_disambiguate_condition, try_add_given_names_with_rule, try_add_names,
        };
        use quarto_csl::GivenNameDisambiguationRule;

        let strategy = &self.style.citation.disambiguation;
        let add_names = strategy.add_names;
        let add_givenname = strategy.add_givenname;
        let add_year_suffix = strategy.add_year_suffix;
        let near_note_distance = self.style.citation.near_note_distance;

        // Reset citation history for fresh position calculation
        self.reset_citation_history();

        // Calculate positions for items that don't have them explicitly set
        let citations_with_positions: Vec<Citation> = citations
            .iter()
            .map(|citation| {
                let note_index = citation.note_number.unwrap_or(0);
                let is_single_item_citation = citation.items.len() == 1;
                let items_with_positions: Vec<CitationItem> = citation
                    .items
                    .iter()
                    .map(|item| {
                        let mut item = item.clone();
                        // Only calculate position if not explicitly set
                        if item.position.is_none() {
                            let positions = self.calculate_position(
                                note_index,
                                &item.id,
                                item.locator.as_deref(),
                                item.label.as_deref(),
                                is_single_item_citation,
                                near_note_distance,
                            );
                            item.position = Some(positions_to_bitmask(&positions));
                        }
                        // Update history after calculating position for this item
                        self.update_citation_history(
                            note_index,
                            &item.id,
                            item.locator.as_deref(),
                            item.label.as_deref(),
                            is_single_item_citation,
                        );
                        item
                    })
                    .collect();
                Citation {
                    id: citation.id.clone(),
                    note_number: citation.note_number,
                    items: items_with_positions,
                }
            })
            .collect();

        // First pass: render all citations WITHOUT collapsing for disambiguation detection
        // Pandoc's citeproc runs disambiguation before collapsing, so we need to see
        // each item's full rendered form (without name suppression from collapsing)
        // to correctly detect ambiguities.
        let mut outputs: Vec<crate::output::Output> = citations_with_positions
            .iter()
            .map(|c| self.process_citation_to_output_no_collapse(c))
            .collect::<Result<Vec<_>>>()?;

        // Extract DisambData and find initial ambiguities
        let mut disamb_data = extract_disamb_data_with_processor(&outputs, self);
        let initial_ambiguities = find_ambiguities(disamb_data.clone());
        let mut ambiguities = initial_ambiguities.clone();

        // 1. Apply global name disambiguation for non-ByCite rules
        // This runs even without ambiguities (global name disambiguation)
        if let Some(rule) = add_givenname {
            if rule != GivenNameDisambiguationRule::ByCite {
                apply_global_name_disambiguation(self, &disamb_data, rule);
                // Re-render (without collapse) and refresh ambiguities
                outputs = citations_with_positions
                    .iter()
                    .map(|c| self.process_citation_to_output_no_collapse(c))
                    .collect::<Result<Vec<_>>>()?;
                disamb_data = extract_disamb_data(&outputs);
                ambiguities = find_ambiguities(disamb_data.clone());
            }
        }

        // 2. Add names (expand et-al) if still ambiguous
        if add_names && !ambiguities.is_empty() {
            try_add_names(self, &ambiguities, add_givenname);
            // Re-render (without collapse) and refresh ambiguities
            outputs = citations_with_positions
                .iter()
                .map(|c| self.process_citation_to_output_no_collapse(c))
                .collect::<Result<Vec<_>>>()?;
            disamb_data = extract_disamb_data(&outputs);
            ambiguities = find_ambiguities(disamb_data.clone());
        }

        // 3. Add given names (ByCite rule) - uses INITIAL ambiguities
        // The ByCite rule needs to operate on the original ambiguity groups,
        // not the refreshed ones after add_names. This is because ByCite
        // incrementally expands givennames within the original groups.
        if let Some(GivenNameDisambiguationRule::ByCite) = add_givenname {
            if !initial_ambiguities.is_empty() {
                try_add_given_names_with_rule(
                    self,
                    &initial_ambiguities,
                    GivenNameDisambiguationRule::ByCite,
                );
                // Re-render (without collapse) and refresh ambiguities
                outputs = citations_with_positions
                    .iter()
                    .map(|c| self.process_citation_to_output_no_collapse(c))
                    .collect::<Result<Vec<_>>>()?;
                disamb_data = extract_disamb_data(&outputs);
                ambiguities = find_ambiguities(disamb_data.clone());
            }
        }

        // 4. Add year suffixes if enabled
        // We need to handle two cases:
        // a) Items that render identically (same author-rendered + same year)
        //    These are detected by rendered-text ambiguities (now working correctly
        //    since disambiguation runs before collapsing)
        // b) Items with same author + same year but different non-disambiguating parts
        //    (e.g., different accessed dates) - these need author+year grouping
        //
        // We merge both approaches to cover all cases.
        if add_year_suffix {
            // Get rendered-text ambiguities (handles case a)
            let rendered_ambiguities = &ambiguities;

            // Get author+year ambiguities (handles case b)
            let author_year_ambiguities =
                find_year_suffix_with_full_author_match(disamb_data.clone(), self);

            // Merge both sets of ambiguity groups
            let year_suffix_groups =
                if !rendered_ambiguities.is_empty() && !author_year_ambiguities.is_empty() {
                    merge_ambiguity_groups(rendered_ambiguities, &author_year_ambiguities)
                } else if !rendered_ambiguities.is_empty() {
                    rendered_ambiguities.clone()
                } else {
                    author_year_ambiguities
                };

            if !year_suffix_groups.is_empty() {
                let suffixes = assign_year_suffixes(self, &year_suffix_groups);
                for (item_id, suffix) in suffixes {
                    self.set_year_suffix(&item_id, suffix);
                }
                // Re-render (without collapse) and refresh ambiguities
                outputs = citations_with_positions
                    .iter()
                    .map(|c| self.process_citation_to_output_no_collapse(c))
                    .collect::<Result<Vec<_>>>()?;
                disamb_data = extract_disamb_data(&outputs);
                ambiguities = find_ambiguities(disamb_data);
            }
        }

        // 5. Set disambiguate condition for any remaining ambiguities
        set_disambiguate_condition(self, &ambiguities);

        // Final pass: re-render WITH collapsing applied (and all disambiguation)
        citations_with_positions
            .iter()
            .map(|c| self.process_citation_to_output(c))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_citation() {
        let json = r#"{
            "citationItems": [
                {"id": "smith2020", "locator": "42", "label": "page"}
            ]
        }"#;

        let citation: Citation = serde_json::from_str(json).unwrap();
        assert_eq!(citation.items.len(), 1);
        assert_eq!(citation.items[0].id, "smith2020");
        assert_eq!(citation.items[0].locator, Some("42".to_string()));
    }

    #[test]
    fn test_parse_citation_with_multiple_items() {
        let json = r#"{
            "citationItems": [
                {"id": "smith2020"},
                {"id": "jones2021", "prefix": "see also"}
            ],
            "noteNumber": 1
        }"#;

        let citation: Citation = serde_json::from_str(json).unwrap();
        assert_eq!(citation.items.len(), 2);
        assert_eq!(citation.note_number, Some(1));
        assert_eq!(citation.items[1].prefix, Some("see also".to_string()));
    }
}
