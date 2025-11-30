//! Locale management for CSL term lookup.
//!
//! This module handles loading and querying locale data for
//! language-specific terms (like "and", "et al.") and date formats.

use crate::locale_parser::parse_locale_xml;
use quarto_csl::{Locale, Term, TermForm};
use rust_embed::Embed;
use std::collections::HashMap;

/// Embedded locale files from the locales/ directory.
#[derive(Embed)]
#[folder = "locales/"]
#[include = "*.xml"]
struct LocaleFiles;

/// Manages locale data for term and date format lookup.
pub struct LocaleManager {
    /// Default locale code (e.g., "en-US").
    default_locale: String,

    /// Loaded locales by language code.
    locales: HashMap<String, Locale>,
}

impl LocaleManager {
    /// Create a new locale manager with a default locale.
    pub fn new(default_locale: Option<String>) -> Self {
        let default = default_locale.unwrap_or_else(|| "en-US".to_string());
        let mut manager = Self {
            default_locale: default.clone(),
            locales: HashMap::new(),
        };
        // Eagerly load the default locale
        manager.load_locale(&default);
        manager
    }

    /// Load a locale from embedded data.
    pub fn load_locale(&mut self, lang: &str) -> Option<&Locale> {
        if self.locales.contains_key(lang) {
            return self.locales.get(lang);
        }

        // Try to load from embedded locale files
        if let Some(locale) = load_embedded_locale(lang) {
            self.locales.insert(lang.to_string(), locale);
            return self.locales.get(lang);
        }

        // Try base language (e.g., "en" for "en-US")
        if let Some(base) = lang.split('-').next() {
            if base != lang {
                if let Some(locale) = load_embedded_locale(base) {
                    self.locales.insert(lang.to_string(), locale);
                    return self.locales.get(lang);
                }
            }
        }

        None
    }

    /// Get a term from the locale.
    pub fn get_term(&self, name: &str, form: TermForm, plural: bool) -> Option<String> {
        // Try the default locale
        if let Some(term) = self.get_term_from_locale(&self.default_locale, name, form, plural) {
            return Some(term);
        }

        // Try base language
        if let Some(base) = self.default_locale.split('-').next() {
            if base != self.default_locale {
                if let Some(term) = self.get_term_from_locale(base, name, form, plural) {
                    return Some(term);
                }
            }
        }

        // Fall back to en-US
        if self.default_locale != "en-US" {
            if let Some(term) = self.get_term_from_locale("en-US", name, form, plural) {
                return Some(term);
            }
        }

        None
    }

    /// Get a term from a specific locale.
    fn get_term_from_locale(
        &self,
        lang: &str,
        name: &str,
        form: TermForm,
        plural: bool,
    ) -> Option<String> {
        let locale = self.locales.get(lang)?;

        // First try exact form match
        for term in &locale.terms {
            if term.name == name && term.form == form {
                return get_term_value(term, plural);
            }
        }

        // Fall back to long form if specific form not found
        if form != TermForm::Long {
            for term in &locale.terms {
                if term.name == name && term.form == TermForm::Long {
                    return get_term_value(term, plural);
                }
            }
        }

        None
    }

    /// Get the default locale code.
    pub fn default_locale(&self) -> &str {
        &self.default_locale
    }

    /// Set a locale directly (for testing or style-embedded locales).
    pub fn set_locale(&mut self, lang: String, locale: Locale) {
        self.locales.insert(lang, locale);
    }

    /// Get the punctuation-in-quote option from the locale.
    /// Returns None if no locale has this option set.
    pub fn get_punctuation_in_quote(&self) -> Option<bool> {
        // Try the default locale
        if let Some(locale) = self.locales.get(&self.default_locale) {
            if let Some(ref opts) = locale.options {
                return Some(opts.punctuation_in_quote);
            }
        }

        // Try base language
        if let Some(base) = self.default_locale.split('-').next() {
            if base != self.default_locale {
                if let Some(locale) = self.locales.get(base) {
                    if let Some(ref opts) = locale.options {
                        return Some(opts.punctuation_in_quote);
                    }
                }
            }
        }

        // Fall back to en-US
        if self.default_locale != "en-US" {
            if let Some(locale) = self.locales.get("en-US") {
                if let Some(ref opts) = locale.options {
                    return Some(opts.punctuation_in_quote);
                }
            }
        }

        None
    }

    /// Get a date format from the locale.
    pub fn get_date_format(&self, form: quarto_csl::DateForm) -> Option<&quarto_csl::DateFormat> {
        // Try the default locale
        if let Some(locale) = self.locales.get(&self.default_locale) {
            for df in &locale.date_formats {
                if df.form == form {
                    return Some(df);
                }
            }
        }

        // Try base language
        if let Some(base) = self.default_locale.split('-').next() {
            if base != self.default_locale {
                if let Some(locale) = self.locales.get(base) {
                    for df in &locale.date_formats {
                        if df.form == form {
                            return Some(df);
                        }
                    }
                }
            }
        }

        // Fall back to en-US
        if self.default_locale != "en-US" {
            if let Some(locale) = self.locales.get("en-US") {
                for df in &locale.date_formats {
                    if df.form == form {
                        return Some(df);
                    }
                }
            }
        }

        None
    }
}

/// Get the appropriate value from a term.
fn get_term_value(term: &Term, plural: bool) -> Option<String> {
    if plural {
        term.multiple
            .clone()
            .or_else(|| term.value.clone())
            .or_else(|| term.single.clone())
    } else {
        term.single
            .clone()
            .or_else(|| term.value.clone())
            .or_else(|| term.multiple.clone())
    }
}

/// Load a locale from embedded data.
fn load_embedded_locale(lang: &str) -> Option<Locale> {
    // Try exact match first (e.g., "en-US.xml")
    let filename = format!("{}.xml", lang);
    if let Some(file) = LocaleFiles::get(&filename) {
        let xml = std::str::from_utf8(file.data.as_ref()).ok()?;
        return parse_locale_xml(xml).ok();
    }

    // Try with country code variations for base language
    // e.g., for "en", try "en-US.xml"
    if !lang.contains('-') {
        // Look for any file starting with the base language
        for entry in LocaleFiles::iter() {
            let entry_str = entry.as_ref();
            if entry_str.starts_with(lang) && entry_str.ends_with(".xml") {
                if let Some(file) = LocaleFiles::get(entry_str) {
                    let xml = std::str::from_utf8(file.data.as_ref()).ok()?;
                    return parse_locale_xml(xml).ok();
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_term() {
        let mut manager = LocaleManager::new(Some("en-US".to_string()));
        manager.load_locale("en-US");

        assert_eq!(
            manager.get_term("and", TermForm::Long, false),
            Some("and".to_string())
        );
        assert_eq!(
            manager.get_term("and", TermForm::Symbol, false),
            Some("&".to_string())
        );
        assert_eq!(
            manager.get_term("et-al", TermForm::Long, false),
            Some("et al.".to_string())
        );
    }

    #[test]
    fn test_get_term_plural() {
        let mut manager = LocaleManager::new(Some("en-US".to_string()));
        manager.load_locale("en-US");

        assert_eq!(
            manager.get_term("editor", TermForm::Long, false),
            Some("editor".to_string())
        );
        assert_eq!(
            manager.get_term("editor", TermForm::Long, true),
            Some("editors".to_string())
        );
        assert_eq!(
            manager.get_term("editor", TermForm::Short, false),
            Some("ed.".to_string())
        );
        assert_eq!(
            manager.get_term("editor", TermForm::Short, true),
            Some("eds.".to_string())
        );
    }

    #[test]
    fn test_fallback_to_long_form() {
        let mut manager = LocaleManager::new(Some("en-US".to_string()));
        manager.load_locale("en-US");

        // "and" only has Long and Symbol forms, so VerbShort should fall back to Long
        assert_eq!(
            manager.get_term("and", TermForm::VerbShort, false),
            Some("and".to_string())
        );
    }

    #[test]
    fn test_month_names() {
        let mut manager = LocaleManager::new(Some("en-US".to_string()));
        manager.load_locale("en-US");

        // Check long month names
        assert_eq!(
            manager.get_term("month-01", TermForm::Long, false),
            Some("January".to_string())
        );
        assert_eq!(
            manager.get_term("month-12", TermForm::Long, false),
            Some("December".to_string())
        );

        // Check short month names
        assert_eq!(
            manager.get_term("month-01", TermForm::Short, false),
            Some("Jan.".to_string())
        );
        assert_eq!(
            manager.get_term("month-06", TermForm::Short, false),
            Some("June".to_string()) // June has no abbreviation
        );
    }

    #[test]
    fn test_load_other_locales() {
        let mut manager = LocaleManager::new(Some("de-DE".to_string()));
        manager.load_locale("de-DE");

        // Check German terms
        assert_eq!(
            manager.get_term("and", TermForm::Long, false),
            Some("und".to_string())
        );
        assert_eq!(
            manager.get_term("month-01", TermForm::Long, false),
            Some("Januar".to_string())
        );
    }

    #[test]
    fn test_all_embedded_locales_parse() {
        // Verify that all embedded locale XML files can be parsed successfully
        let locale_files: Vec<_> = LocaleFiles::iter().collect();
        assert!(!locale_files.is_empty(), "No locale files found!");

        let mut count = 0;
        for filename in locale_files {
            let filename_str = filename.as_ref();
            if !filename_str.ends_with(".xml") {
                continue;
            }

            let file = LocaleFiles::get(filename_str)
                .unwrap_or_else(|| panic!("Failed to get file: {}", filename_str));
            let xml = std::str::from_utf8(file.data.as_ref())
                .unwrap_or_else(|e| panic!("Invalid UTF-8 in {}: {}", filename_str, e));
            let locale = parse_locale_xml(xml)
                .unwrap_or_else(|e| panic!("Failed to parse {}: {}", filename_str, e));

            // Verify basic structure
            assert!(
                locale.lang.is_some(),
                "Locale {} has no language",
                filename_str
            );
            assert!(
                !locale.terms.is_empty(),
                "Locale {} has no terms",
                filename_str
            );

            count += 1;
        }

        // We should have at least 50 locale files
        assert!(
            count >= 50,
            "Expected at least 50 locale files, got {}",
            count
        );
    }

    #[test]
    fn test_punctuation_in_quote_en_us() {
        let manager = LocaleManager::new(Some("en-US".to_string()));
        let result = manager.get_punctuation_in_quote();
        println!("en-US punctuation_in_quote: {:?}", result);
        assert_eq!(result, Some(true));
    }

    #[test]
    fn test_punctuation_in_quote_en_gb() {
        let manager = LocaleManager::new(Some("en-GB".to_string()));
        let result = manager.get_punctuation_in_quote();
        println!("en-GB punctuation_in_quote: {:?}", result);
        assert_eq!(result, Some(false));
    }
}
