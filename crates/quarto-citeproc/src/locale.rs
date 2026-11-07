//! Locale management for CSL term lookup.
//!
//! This module handles loading and querying locale data for
//! language-specific terms (like "and", "et al.") and date formats.

use crate::locale_parser::parse_locale_xml;
use crate::output::QuoteConfig;
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
    ///
    /// If no bundled locale matches the requested one (e.g. `lang: xx`),
    /// the manager uses en-US instead, as Pandoc's citeproc does, so terms
    /// and date formats never come back empty.
    pub fn new(default_locale: Option<String>) -> Self {
        let default = default_locale.unwrap_or_else(|| "en-US".to_string());
        let mut manager = Self {
            default_locale: default.clone(),
            locales: HashMap::new(),
        };
        // Eagerly load the default locale
        if manager.load_locale(&default).is_none() && default != "en-US" {
            manager.default_locale = "en-US".to_string();
            manager.load_locale("en-US");
        }
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
        if let Some(base) = lang.split('-').next()
            && base != lang
            && let Some(locale) = load_embedded_locale(base)
        {
            self.locales.insert(lang.to_string(), locale);
            return self.locales.get(lang);
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
        if let Some(base) = self.default_locale.split('-').next()
            && base != self.default_locale
            && let Some(term) = self.get_term_from_locale(base, name, form, plural)
        {
            return Some(term);
        }

        // Fall back to en-US
        if self.default_locale != "en-US"
            && let Some(term) = self.get_term_from_locale("en-US", name, form, plural)
        {
            return Some(term);
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
        if let Some(locale) = self.locales.get(&self.default_locale)
            && let Some(ref opts) = locale.options
        {
            return Some(opts.punctuation_in_quote);
        }

        // Try base language
        if let Some(base) = self.default_locale.split('-').next()
            && base != self.default_locale
            && let Some(locale) = self.locales.get(base)
            && let Some(ref opts) = locale.options
        {
            return Some(opts.punctuation_in_quote);
        }

        // Fall back to en-US
        if self.default_locale != "en-US"
            && let Some(locale) = self.locales.get("en-US")
            && let Some(ref opts) = locale.options
        {
            return Some(opts.punctuation_in_quote);
        }

        None
    }

    /// Get locale-specific quote configuration.
    ///
    /// Looks up the quote terms from the locale:
    /// - `open-quote` / `close-quote` for outer (primary) quotes
    /// - `open-inner-quote` / `close-inner-quote` for inner (secondary) quotes
    ///
    /// Falls back to English curly quotes if terms are not found.
    pub fn get_quote_config(&self) -> QuoteConfig {
        let outer_open = self
            .get_term("open-quote", TermForm::Long, false)
            .unwrap_or_else(|| "\u{201C}".to_string()); // "
        let outer_close = self
            .get_term("close-quote", TermForm::Long, false)
            .unwrap_or_else(|| "\u{201D}".to_string()); // "
        let inner_open = self
            .get_term("open-inner-quote", TermForm::Long, false)
            .unwrap_or_else(|| "\u{2018}".to_string()); // '
        let inner_close = self
            .get_term("close-inner-quote", TermForm::Long, false)
            .unwrap_or_else(|| "\u{2019}".to_string()); // '

        QuoteConfig {
            outer_open,
            outer_close,
            inner_open,
            inner_close,
        }
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
        if let Some(base) = self.default_locale.split('-').next()
            && base != self.default_locale
            && let Some(locale) = self.locales.get(base)
        {
            for df in &locale.date_formats {
                if df.form == form {
                    return Some(df);
                }
            }
        }

        // Fall back to en-US
        if self.default_locale != "en-US"
            && let Some(locale) = self.locales.get("en-US")
        {
            for df in &locale.date_formats {
                if df.form == form {
                    return Some(df);
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

/// Primary dialect for each bare language code, from the CSL locales
/// repository's `locales.json` (`primary-dialects`), restricted to the
/// locales bundled in `locales/`. citeproc (and so Pandoc and Quarto 1)
/// resolves `lang: pt` to `pt-PT` and `lang: en` to `en-US` through this
/// table, not through whichever regional file sorts first.
const PRIMARY_DIALECTS: &[(&str, &str)] = &[
    ("af", "af-ZA"),
    ("ar", "ar"),
    ("bal", "bal-PK"),
    ("bg", "bg-BG"),
    ("brh", "brh-PK"),
    ("ca", "ca-AD"),
    ("cs", "cs-CZ"),
    ("cy", "cy-GB"),
    ("da", "da-DK"),
    ("de", "de-DE"),
    ("el", "el-GR"),
    ("en", "en-US"),
    ("es", "es-ES"),
    ("et", "et-EE"),
    ("eu", "eu"),
    ("fa", "fa-IR"),
    ("fi", "fi-FI"),
    ("fr", "fr-FR"),
    ("gl", "gl-ES"),
    ("he", "he-IL"),
    ("hi", "hi-IN"),
    ("hr", "hr-HR"),
    ("hu", "hu-HU"),
    ("id", "id-ID"),
    ("is", "is-IS"),
    ("it", "it-IT"),
    ("ja", "ja-JP"),
    ("km", "km-KH"),
    ("ko", "ko-KR"),
    ("la", "la"),
    ("lij", "lij-IT"),
    ("lt", "lt-LT"),
    ("lv", "lv-LV"),
    ("mn", "mn-MN"),
    ("ms", "ms-MY"),
    ("nb", "nb-NO"),
    ("nl", "nl-NL"),
    ("nn", "nn-NO"),
    ("pa", "pa-PK"),
    ("pl", "pl-PL"),
    ("pt", "pt-PT"),
    ("ro", "ro-RO"),
    ("ru", "ru-RU"),
    ("sk", "sk-SK"),
    ("sl", "sl-SI"),
    ("sr", "sr-RS"),
    ("sv", "sv-SE"),
    ("th", "th-TH"),
    ("tr", "tr-TR"),
    ("uk", "uk-UA"),
    ("vi", "vi-VN"),
    ("zh", "zh-CN"),
];

/// The primary dialect of a bare language code (`nl` → `nl-NL`), if known.
pub fn primary_dialect(lang: &str) -> Option<&'static str> {
    PRIMARY_DIALECTS
        .iter()
        .find(|(base, _)| base.eq_ignore_ascii_case(lang))
        .map(|(_, dialect)| *dialect)
}

/// Normalize a document language tag for locale lookup.
///
/// Accepts the spellings people write in `lang:` (`nl`, `NL`, `nl-nl`,
/// `nl_NL`) and returns the CSL form: lowercase language, uppercase
/// two-letter region (`nl-NL`). A bare language becomes its primary
/// dialect when one is known. Returns `None` for an empty tag.
pub fn normalize_lang(lang: &str) -> Option<String> {
    let lang = lang.trim();
    if lang.is_empty() {
        return None;
    }
    let mut parts = lang.split(['-', '_']);
    let language = parts.next().unwrap_or_default().to_ascii_lowercase();
    let rest: Vec<String> = parts
        .map(|part| {
            if part.len() == 2 {
                part.to_ascii_uppercase()
            } else {
                part.to_string()
            }
        })
        .collect();
    if rest.is_empty() {
        return Some(
            primary_dialect(&language)
                .map(str::to_string)
                .unwrap_or(language),
        );
    }
    Some(format!("{}-{}", language, rest.join("-")))
}

/// Load a locale from embedded data.
///
/// An exact file match wins (`nl-NL` → `nl-NL.xml`). A bare language
/// (`nl`) resolves through the primary-dialect table, then to the
/// alphabetically first `<lang>-*.xml` file for a language the table does
/// not list.
fn load_embedded_locale(lang: &str) -> Option<Locale> {
    // Try exact match first (e.g., "en-US.xml")
    if let Some(locale) = parse_embedded_file(&format!("{}.xml", lang)) {
        return Some(locale);
    }

    if lang.contains('-') {
        return None;
    }

    if let Some(dialect) = primary_dialect(lang) {
        return parse_embedded_file(&format!("{}.xml", dialect));
    }

    let prefix = format!("{}-", lang);
    let mut candidates: Vec<String> = LocaleFiles::iter()
        .filter(|entry| entry.starts_with(&prefix) && entry.ends_with(".xml"))
        .map(|entry| entry.into_owned())
        .collect();
    candidates.sort();
    candidates
        .first()
        .and_then(|filename| parse_embedded_file(filename))
}

fn parse_embedded_file(filename: &str) -> Option<Locale> {
    let file = LocaleFiles::get(filename)?;
    let xml = std::str::from_utf8(file.data.as_ref()).ok()?;
    parse_locale_xml(xml).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_lang_spellings() {
        assert_eq!(normalize_lang("nl").as_deref(), Some("nl-NL"));
        assert_eq!(normalize_lang("NL").as_deref(), Some("nl-NL"));
        assert_eq!(normalize_lang("nl_nl").as_deref(), Some("nl-NL"));
        assert_eq!(normalize_lang(" de-at ").as_deref(), Some("de-AT"));
        assert_eq!(normalize_lang("pt").as_deref(), Some("pt-PT"));
        assert_eq!(normalize_lang("zh-Hant-TW").as_deref(), Some("zh-Hant-TW"));
        assert_eq!(normalize_lang("xx").as_deref(), Some("xx"));
        assert_eq!(normalize_lang("  "), None);
    }

    #[test]
    fn every_primary_dialect_is_bundled() {
        for (base, dialect) in PRIMARY_DIALECTS {
            assert!(
                LocaleFiles::get(&format!("{dialect}.xml")).is_some(),
                "{base} -> {dialect}.xml is not in locales/"
            );
        }
    }

    #[test]
    fn bare_language_loads_primary_dialect_not_first_file() {
        // pt-BR.xml sorts before pt-PT.xml; the primary dialect wins.
        let pt = load_embedded_locale("pt").unwrap();
        assert_eq!(pt.lang.as_deref(), Some("pt-PT"));
        let en = load_embedded_locale("en").unwrap();
        assert_eq!(en.lang.as_deref(), Some("en-US"));
    }

    #[test]
    fn unknown_locale_falls_back_to_en_us() {
        let manager = LocaleManager::new(Some("xx".to_string()));
        assert_eq!(manager.default_locale(), "en-US");
        assert_eq!(
            manager.get_term("and", TermForm::Long, false).as_deref(),
            Some("and")
        );
    }

    #[test]
    fn region_without_bundled_file_uses_base_language() {
        // There is no nl-BE.xml; Dutch terms still apply.
        let manager = LocaleManager::new(Some("nl-BE".to_string()));
        assert_eq!(
            manager.get_term("and", TermForm::Long, false).as_deref(),
            Some("en")
        );
    }

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

    #[test]
    fn test_get_quote_config_english() {
        let manager = LocaleManager::new(Some("en-US".to_string()));
        let quotes = manager.get_quote_config();
        // English uses curly double quotes for outer, curly single for inner
        assert_eq!(quotes.outer_open, "\u{201C}"); // "
        assert_eq!(quotes.outer_close, "\u{201D}"); // "
        assert_eq!(quotes.inner_open, "\u{2018}"); // '
        assert_eq!(quotes.inner_close, "\u{2019}"); // '
    }

    #[test]
    fn test_get_quote_config_french() {
        let manager = LocaleManager::new(Some("fr-FR".to_string()));
        let quotes = manager.get_quote_config();
        println!(
            "French quotes: outer_open={:?}, outer_close={:?}",
            quotes.outer_open, quotes.outer_close
        );
        // French uses guillemets with non-breaking space
        // The locale file has: <term name="open-quote">«&#160;</term>
        assert!(
            quotes.outer_open.contains('«'),
            "Expected French open-quote to contain «, got {:?}",
            quotes.outer_open
        );
        assert!(
            quotes.outer_close.contains('»'),
            "Expected French close-quote to contain », got {:?}",
            quotes.outer_close
        );
    }
}
