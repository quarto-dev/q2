//! Core types for citation processing.

use crate::locale::LocaleManager;
use crate::reference::Reference;
use crate::Result;
use quarto_csl::Style;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

    /// References by ID.
    references: HashMap<String, Reference>,

    /// Citation number assignments (for numeric styles).
    citation_numbers: HashMap<String, i32>,

    /// Next citation number.
    next_citation_number: i32,
}

impl Processor {
    /// Create a new processor with a CSL style.
    pub fn new(style: Style) -> Self {
        let default_locale = style.default_locale.clone();
        Self {
            style,
            locales: LocaleManager::new(default_locale),
            references: HashMap::new(),
            citation_numbers: HashMap::new(),
            next_citation_number: 1,
        }
    }

    /// Add a reference to the processor.
    pub fn add_reference(&mut self, reference: Reference) {
        self.references.insert(reference.id.clone(), reference);
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

    /// Get or assign a citation number for a reference.
    pub fn get_citation_number(&mut self, id: &str) -> i32 {
        if let Some(&num) = self.citation_numbers.get(id) {
            num
        } else {
            let num = self.next_citation_number;
            self.next_citation_number += 1;
            self.citation_numbers.insert(id.to_string(), num);
            num
        }
    }

    /// Process a citation and return formatted output.
    ///
    /// Returns a string representation of the formatted citation.
    /// In the future, this will return Pandoc Inlines.
    pub fn process_citation(&mut self, citation: &Citation) -> Result<String> {
        crate::eval::evaluate_citation(self, citation)
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

    /// Generate the full bibliography.
    pub fn generate_bibliography(&mut self) -> Result<Vec<(String, String)>> {
        if self.style.bibliography.is_none() {
            return Ok(Vec::new());
        }

        let mut entries = Vec::new();
        let ids: Vec<String> = self.references.keys().cloned().collect();

        for id in ids {
            if let Some(formatted) = self.format_bibliography_entry(&id)? {
                entries.push((id, formatted));
            }
        }

        // TODO: Sort according to bibliography sort keys
        Ok(entries)
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
