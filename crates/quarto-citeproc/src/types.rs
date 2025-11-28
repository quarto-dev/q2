//! Core types for citation processing.

use crate::locale::LocaleManager;
use crate::reference::Reference;
use crate::Result;
use hashlink::LinkedHashMap;
use quarto_csl::Style;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;

/// A computed sort key value with its sort direction.
#[derive(Debug, Clone)]
pub struct SortKeyValue {
    /// The computed string value for sorting.
    pub value: String,
    /// Whether this key sorts in descending order.
    pub descending: bool,
}

/// Compare two sets of sort keys.
pub fn compare_sort_keys(a: &[SortKeyValue], b: &[SortKeyValue]) -> Ordering {
    for (ka, kb) in a.iter().zip(b.iter()) {
        // Normalize for comparison: lowercase and strip non-alphabetic leading/trailing chars
        let va = normalize_for_sort(&ka.value);
        let vb = normalize_for_sort(&kb.value);
        let cmp = va.cmp(&vb);
        if cmp != Ordering::Equal {
            return if ka.descending { cmp.reverse() } else { cmp };
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

/// Normalize a string for sort comparison.
/// Strips HTML tags, leading/trailing punctuation (but not alphanumerics), and lowercases.
fn normalize_for_sort(s: &str) -> String {
    // First strip HTML tags
    let s = strip_html_tags(s);
    // Strip leading punctuation (like brackets, quotes) but keep letters AND digits
    let s = s.trim_start_matches(|c: char| !c.is_alphanumeric());
    // Strip trailing punctuation (but keep letters and digits)
    let s = s.trim_end_matches(|c: char| !c.is_alphanumeric());
    // Lowercase and remove internal brackets for comparison
    s.to_lowercase()
        .chars()
        .filter(|c| *c != '[' && *c != ']')
        .collect()
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
    initial_citation_numbers: HashMap<String, i32>,

    /// Final citation numbers (reassigned after bibliography sorting, used for rendering).
    /// If None, falls back to initial_citation_numbers.
    final_citation_numbers: Option<HashMap<String, i32>>,

    /// Next citation number for initial assignment.
    next_citation_number: i32,
}

impl Processor {
    /// Create a new processor with a CSL style.
    pub fn new(style: Style) -> Self {
        let default_locale = style.default_locale.clone();
        Self {
            style,
            locales: LocaleManager::new(default_locale),
            references: LinkedHashMap::new(),
            initial_citation_numbers: HashMap::new(),
            final_citation_numbers: None,
            next_citation_number: 1,
        }
    }

    /// Add a reference to the processor.
    pub fn add_reference(&mut self, reference: Reference) {
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
        let mut final_numbers = HashMap::new();
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
        let uses_citation_number = sort_keys.iter().any(|k| {
            matches!(&k.key, quarto_csl::SortKeyType::Variable(v) if v == "citation-number")
        });

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
        let bib = match &self.style.bibliography {
            Some(b) => b,
            None => return Ok(Vec::new()),
        };

        // LinkedHashMap preserves insertion order
        let ids: Vec<String> = self.references.keys().cloned().collect();

        // Get sort keys from bibliography
        let sort_keys = bib.sort.as_ref().map(|s| &s.keys[..]).unwrap_or(&[]);

        // Check if any sort key uses citation-number
        let uses_citation_number = sort_keys.iter().any(|k| {
            matches!(&k.key, quarto_csl::SortKeyType::Variable(v) if v == "citation-number")
        });

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
                        self.get_sort_value_for_macro(reference, name)
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
            "author" | "editor" | "translator" | "director" | "interviewer"
            | "illustrator" | "composer" | "collection-editor" | "container-author" => {
                if let Some(names) = reference.get_names(var) {
                    names
                        .iter()
                        .filter_map(|n| {
                            n.literal.as_ref().or(n.family.as_ref()).cloned()
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                } else {
                    String::new()
                }
            }
            // Date variables - format as sortable string
            "issued" | "accessed" | "event-date" | "original-date" | "submitted" => {
                if let Some(date) = reference.get_date(var) {
                    // Format as YYYY-MM-DD for sorting
                    if let Some(parts) = date.date_parts.as_ref().and_then(|p| p.first()) {
                        let year = parts.first().copied().unwrap_or(0);
                        let month = parts.get(1).copied().unwrap_or(0);
                        let day = parts.get(2).copied().unwrap_or(0);
                        format!("{:04}-{:02}-{:02}", year, month, day)
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
    fn get_sort_value_for_macro(&self, reference: &Reference, macro_name: &str) -> String {
        // Evaluate the macro and return plain text (stripping formatting)
        // For now, just try to evaluate it using the bibliography context
        if let Some(macro_def) = self.style.macros.get(macro_name) {
            // Create a minimal context and evaluate
            // This is a simplified version - full implementation would need proper context
            crate::eval::evaluate_macro_for_sort(self, reference, &macro_def.elements)
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
    pub fn get_date_format(&self, form: quarto_csl::DateForm) -> Option<&quarto_csl::DateFormat> {
        // First check style-level locale overrides
        for locale in &self.style.locales {
            for df in &locale.date_formats {
                if df.form == form {
                    return Some(df);
                }
            }
        }

        // Fall back to locale manager
        self.locales.get_date_format(form)
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
