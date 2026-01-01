//! Reference types for CSL-JSON bibliographic data.
//!
//! This module defines types for parsing and representing bibliographic
//! references in CSL-JSON format.

use hashlink::LinkedHashMap;
use serde::{Deserialize, Deserializer, Serialize};

/// Disambiguation state for a reference.
///
/// This is computed during citation processing to resolve ambiguous citations.
#[derive(Debug, Clone, Default)]
pub struct DisambiguationData {
    /// Assigned year suffix (1=a, 2=b, etc.). None means no suffix assigned.
    pub year_suffix: Option<i32>,
    /// Hints for expanding names (per-name map).
    pub name_hints: LinkedHashMap<String, NameHint>,
    /// Override for et-al-use-first (show more names for disambiguation).
    pub et_al_names: Option<u32>,
    /// Whether the disambiguate="true" condition should match.
    pub disamb_condition: bool,
}

/// Hint for how to expand a name for disambiguation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameHint {
    /// Add initials to this name.
    AddInitials,
    /// Add full given name to this name.
    AddGivenName,
    /// Add initials only if this is the primary (first) name.
    AddInitialsIfPrimary,
    /// Add full given name only if this is the primary (first) name.
    AddGivenNameIfPrimary,
}

/// A bibliographic reference in CSL-JSON format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reference {
    /// Unique identifier for this reference.
    /// CSL-JSON allows both string and integer IDs, so we accept both.
    /// If not provided, defaults to an empty string (some CSL tests omit the id).
    #[serde(deserialize_with = "deserialize_optional_string_or_int", default)]
    pub id: String,

    /// Reference type (e.g., "book", "article-journal", "chapter").
    /// Defaults to empty string if not provided (matching Pandoc citeproc behavior).
    #[serde(rename = "type", default)]
    pub ref_type: String,

    // Standard CSL variables - text
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(rename = "title-short", skip_serializing_if = "Option::is_none")]
    pub title_short: Option<String>,
    #[serde(rename = "container-title", skip_serializing_if = "Option::is_none")]
    pub container_title: Option<String>,
    /// Short form of container title.
    ///
    /// Legacy citeproc-js used "journalAbbreviation" for this field, so we accept
    /// that as an alias for backwards compatibility.
    /// See: https://github.com/jgm/citeproc/blob/master/src/Citeproc/Types.hs
    #[serde(
        rename = "container-title-short",
        alias = "journalAbbreviation",
        skip_serializing_if = "Option::is_none"
    )]
    pub container_title_short: Option<String>,
    #[serde(rename = "collection-title", skip_serializing_if = "Option::is_none")]
    pub collection_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    #[serde(rename = "publisher-place", skip_serializing_if = "Option::is_none")]
    pub publisher_place: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edition: Option<StringOrNumber>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<StringOrNumber>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issue: Option<StringOrNumber>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,
    #[serde(rename = "page-first", skip_serializing_if = "Option::is_none")]
    pub page_first: Option<String>,
    #[serde(rename = "number-of-pages", skip_serializing_if = "Option::is_none")]
    pub number_of_pages: Option<StringOrNumber>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chapter: Option<StringOrNumber>,
    #[serde(rename = "abstract", skip_serializing_if = "Option::is_none")]
    pub abstract_: Option<String>,
    #[serde(rename = "DOI", skip_serializing_if = "Option::is_none")]
    pub doi: Option<String>,
    #[serde(rename = "ISBN", skip_serializing_if = "Option::is_none")]
    pub isbn: Option<String>,
    #[serde(rename = "ISSN", skip_serializing_if = "Option::is_none")]
    pub issn: Option<String>,
    #[serde(rename = "URL", skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,

    // Name variables
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<Vec<Name>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub editor: Option<Vec<Name>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub translator: Option<Vec<Name>>,
    #[serde(rename = "container-author", skip_serializing_if = "Option::is_none")]
    pub container_author: Option<Vec<Name>>,
    #[serde(rename = "collection-editor", skip_serializing_if = "Option::is_none")]
    pub collection_editor: Option<Vec<Name>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub director: Option<Vec<Name>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interviewer: Option<Vec<Name>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipient: Option<Vec<Name>>,
    #[serde(rename = "reviewed-author", skip_serializing_if = "Option::is_none")]
    pub reviewed_author: Option<Vec<Name>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub composer: Option<Vec<Name>>,

    // Date variables
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issued: Option<DateVariable>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accessed: Option<DateVariable>,
    #[serde(rename = "event-date", skip_serializing_if = "Option::is_none")]
    pub event_date: Option<DateVariable>,
    #[serde(rename = "original-date", skip_serializing_if = "Option::is_none")]
    pub original_date: Option<DateVariable>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub submitted: Option<DateVariable>,

    // Other fields captured in a map for extensibility
    #[serde(flatten)]
    pub other: LinkedHashMap<String, serde_json::Value>,

    /// Disambiguation state (computed at runtime, not serialized).
    #[serde(skip)]
    pub disambiguation: Option<DisambiguationData>,
}

/// A string or number value (CSL allows both for some fields).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StringOrNumber {
    String(String),
    Number(i64),
}

impl StringOrNumber {
    /// Get the value as a string.
    pub fn as_str(&self) -> String {
        match self {
            StringOrNumber::String(s) => s.clone(),
            StringOrNumber::Number(n) => n.to_string(),
        }
    }

    /// Get the value as a number if possible.
    pub fn as_number(&self) -> Option<i64> {
        match self {
            StringOrNumber::String(s) => s.parse().ok(),
            StringOrNumber::Number(n) => Some(*n),
        }
    }
}

/// Deserialize an optional value that can be a string, integer, or missing.
/// Returns empty string if missing or null.
fn deserialize_optional_string_or_int<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value: serde_json::Value = Deserialize::deserialize(deserializer)?;
    match value {
        serde_json::Value::String(s) => Ok(s),
        serde_json::Value::Number(n) => Ok(n.to_string()),
        serde_json::Value::Null => Ok(String::new()),
        _ => Ok(String::new()),
    }
}

/// Deserialize a boolean that can be either true/false or 1/0.
/// CSL-JSON allows `"circa": 1` as equivalent to `"circa": true`.
fn deserialize_bool_or_int<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: Deserializer<'de>,
{
    let value: serde_json::Value = Deserialize::deserialize(deserializer)?;
    match value {
        serde_json::Value::Bool(b) => Ok(Some(b)),
        serde_json::Value::Number(n) => {
            // 0 = false, any other number = true
            Ok(Some(n.as_i64().is_some_and(|i| i != 0)))
        }
        serde_json::Value::Null => Ok(None),
        _ => Ok(None),
    }
}

/// Deserialize a season value that can be an integer or (erroneously) a string.
/// Some CSL test data has strings like "22:56:08" in the season field (for time).
/// We ignore non-integer values rather than failing.
fn deserialize_season<'de, D>(deserializer: D) -> Result<Option<i32>, D::Error>
where
    D: Deserializer<'de>,
{
    let value: serde_json::Value = Deserialize::deserialize(deserializer)?;
    match value {
        serde_json::Value::Number(n) => Ok(n.as_i64().map(|i| i as i32)),
        serde_json::Value::Null => Ok(None),
        // Strings that look like times (e.g., "22:56:08") - just ignore
        serde_json::Value::String(s) => {
            // Try to parse as integer first
            if let Ok(i) = s.parse::<i32>() {
                Ok(Some(i))
            } else {
                // Non-numeric string (like time) - ignore it
                Ok(None)
            }
        }
        _ => Ok(None),
    }
}

/// A name in CSL-JSON format.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq, Hash)]
pub struct Name {
    /// Family name (surname).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,

    /// Given name (first name).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub given: Option<String>,

    /// Dropping particle (e.g., "de" in "Ludwig de Beethoven").
    #[serde(rename = "dropping-particle", skip_serializing_if = "Option::is_none")]
    pub dropping_particle: Option<String>,

    /// Non-dropping particle (e.g., "van" in "Vincent van Gogh").
    #[serde(
        rename = "non-dropping-particle",
        skip_serializing_if = "Option::is_none"
    )]
    pub non_dropping_particle: Option<String>,

    /// Suffix (e.g., "Jr.", "III").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suffix: Option<String>,

    /// Whether suffix requires a comma before it.
    /// If true: "Smith, Jr." - If false or None: "Smith Jr."
    /// This is typically set when parsing names from BibTeX or other formats.
    #[serde(rename = "comma-suffix", skip_serializing_if = "Option::is_none")]
    pub comma_suffix: Option<bool>,

    /// Whether this name has static ordering (doesn't follow normal given/family rules).
    /// Used for CJK names and other non-Western name ordering conventions.
    #[serde(rename = "static-ordering", skip_serializing_if = "Option::is_none")]
    pub static_ordering: Option<bool>,

    /// Literal name (for institutional names or when family/given doesn't apply).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub literal: Option<String>,

    /// Parse status (for names parsed from a string).
    #[serde(rename = "parse-names", skip_serializing_if = "Option::is_none")]
    pub parse_names: Option<bool>,
}

impl Name {
    /// Check if this is a literal (institutional) name.
    pub fn is_literal(&self) -> bool {
        self.literal.is_some()
    }

    /// Check if this is a "Byzantine" (Western/Romanesque) name.
    ///
    /// Byzantine names use Western conventions (comma in sort order, etc.).
    /// Non-Byzantine names (like CJK names) use different formatting rules.
    ///
    /// A name is Byzantine if its family name contains any Romanesque character,
    /// which includes Latin, Greek, Cyrillic, Hebrew, Arabic, Thai, and related scripts.
    pub fn is_byzantine(&self) -> bool {
        self.family
            .as_ref()
            .is_some_and(|family| family.chars().any(is_byzantine_char))
    }

    /// Extract dropping and non-dropping particles from given/family names.
    ///
    /// Following CSL-JSON spec: lowercase elements at the end of the given name
    /// are treated as "dropping" particles, and lowercase elements at the start
    /// of the family name are treated as "non-dropping" particles.
    ///
    /// This mutates the Name in place, populating the particle fields.
    pub fn extract_particles(&mut self) {
        // Extract dropping particle from end of given name
        // e.g., "Givenname al" -> given="Givenname", dropping_particle="al"
        if self.dropping_particle.is_none()
            && let Some(given) = self.given.clone()
        {
            // Don't process quoted names (CSL-JSON convention for literal names)
            if given.starts_with('"') && given.ends_with('"') {
                self.given = Some(given[1..given.len() - 1].to_string());
            } else {
                let words: Vec<&str> = given.split_whitespace().collect();
                if words.len() > 1 {
                    // Find where particle words begin (all lowercase or particle punctuation)
                    let break_point = words
                        .iter()
                        .position(|w| w.chars().all(|c| c.is_lowercase() || is_particle_punct(c)));

                    if let Some(idx) = break_point {
                        // Check if ALL remaining words are particle-like
                        let all_particles = words[idx..]
                            .iter()
                            .all(|w| w.chars().all(|c| c.is_lowercase() || is_particle_punct(c)));

                        if all_particles && idx > 0 {
                            self.given = Some(words[..idx].join(" "));
                            self.dropping_particle = Some(words[idx..].join(" "));
                        }
                    }
                }
            }
        }

        // Extract non-dropping particle from start of family name
        // e.g., "van Gogh" -> non_dropping_particle="van", family="Gogh"
        if self.non_dropping_particle.is_none()
            && let Some(family) = self.family.clone()
        {
            // Don't process quoted names
            if family.starts_with('"') && family.ends_with('"') {
                self.family = Some(family[1..family.len() - 1].to_string());
            } else {
                let words: Vec<&str> = family.split_whitespace().collect();
                if words.len() > 1 {
                    // Find how many leading words are particle-like
                    let particle_count = words
                        .iter()
                        .take_while(|w| w.chars().all(|c| c.is_lowercase() || is_particle_punct(c)))
                        .count();

                    if particle_count > 0 && particle_count < words.len() {
                        self.non_dropping_particle = Some(words[..particle_count].join(" "));
                        self.family = Some(words[particle_count..].join(" "));
                    }
                }
            }
        }

        // If no space-separated non-dropping particle found, try punctuation-connected extraction
        // e.g., "d'Aubignac" -> non_dropping_particle="d'", family="Aubignac"
        // e.g., "al-One" -> non_dropping_particle="al-", family="One"
        // Reference: Haskell citeproc Types.hs:1253-1258
        if self.non_dropping_particle.is_none()
            && let Some(family) = self.family.clone()
        {
            // Find first particle punctuation character
            if let Some(punct_idx) = family.find(is_particle_punct) {
                let before = &family[..punct_idx];
                let punct_char = family[punct_idx..].chars().next().unwrap();
                let after = &family[punct_idx + punct_char.len_utf8()..];

                // Only extract if:
                // 1. "before" is not empty (must have particle text)
                // 2. "after" is not empty (must have actual family name)
                // 3. "before" is all particle characters (lowercase + punct)
                if !before.is_empty()
                    && !after.is_empty()
                    && before
                        .chars()
                        .all(|c| c.is_lowercase() || is_particle_punct(c))
                {
                    self.non_dropping_particle = Some(format!("{}{}", before, punct_char));
                    self.family = Some(after.to_string());
                }
            }
        }
    }

    /// Get the display name in "family, given" format.
    pub fn display_name(&self) -> String {
        if let Some(ref lit) = self.literal {
            return lit.clone();
        }

        let mut parts = Vec::new();

        if let Some(ref ndp) = self.non_dropping_particle {
            parts.push(ndp.clone());
        }

        if let Some(ref family) = self.family {
            parts.push(family.clone());
        }

        if let Some(ref suffix) = self.suffix {
            parts.push(suffix.clone());
        }

        let family_part = parts.join(" ");

        if let Some(ref given) = self.given {
            if family_part.is_empty() {
                given.clone()
            } else {
                format!("{}, {}", family_part, given)
            }
        } else {
            family_part
        }
    }
}

/// A date variable in CSL-JSON format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DateVariable {
    /// Date parts: [[year, month, day], [end_year, end_month, end_day]] for ranges.
    /// Values can be integers or strings (CSL-JSON allows both).
    #[serde(
        rename = "date-parts",
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_date_parts",
        default
    )]
    pub date_parts: Option<Vec<Vec<i32>>>,

    /// Literal date string (when structured date is not available).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub literal: Option<String>,

    /// Raw date string (unparsed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,

    /// Season (1=spring, 2=summer, 3=fall, 4=winter).
    /// Some CSL test data misuses this field for time strings; we accept those gracefully.
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_season",
        default
    )]
    pub season: Option<i32>,

    /// Circa flag (approximate date).
    /// CSL-JSON allows both boolean (true/false) and integer (1/0) values.
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_bool_or_int",
        default
    )]
    pub circa: Option<bool>,
}

/// Custom deserializer for date-parts that accepts both strings and integers.
fn deserialize_date_parts<'de, D>(deserializer: D) -> Result<Option<Vec<Vec<i32>>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, SeqAccess, Visitor};

    struct DatePartsArrayVisitor;

    impl<'de> Visitor<'de> for DatePartsArrayVisitor {
        type Value = Vec<Vec<i32>>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("date-parts array")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let mut outer = Vec::new();

            while let Some(inner_value) = seq.next_element::<Vec<DatePartValue>>()? {
                // Filter out None values (from empty strings) like Pandoc's removeEmptyStrings
                let inner: Vec<i32> = inner_value.into_iter().filter_map(|v| v.0).collect();
                outer.push(inner);
            }

            Ok(outer)
        }
    }

    struct OptionalDatePartsVisitor;

    impl<'de> Visitor<'de> for OptionalDatePartsVisitor {
        type Value = Option<Vec<Vec<i32>>>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("date-parts array or null")
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_some<D2>(self, deserializer: D2) -> Result<Self::Value, D2::Error>
        where
            D2: serde::Deserializer<'de>,
        {
            let value = deserializer.deserialize_seq(DatePartsArrayVisitor)?;
            Ok(Some(value))
        }
    }

    deserializer.deserialize_option(OptionalDatePartsVisitor)
}

/// A date part value that can be either a string or integer.
/// Empty strings are represented as None and filtered out during deserialization.
struct DatePartValue(Option<i32>);

impl<'de> Deserialize<'de> for DatePartValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, Visitor};

        struct DatePartValueVisitor;

        impl<'de> Visitor<'de> for DatePartValueVisitor {
            type Value = DatePartValue;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("an integer or string representing a date part")
            }

            fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(DatePartValue(Some(v as i32)))
            }

            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(DatePartValue(Some(v as i32)))
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                // Empty strings are filtered out (like Pandoc's removeEmptyStrings)
                if v.is_empty() {
                    return Ok(DatePartValue(None));
                }
                v.parse::<i32>()
                    .map(|i| DatePartValue(Some(i)))
                    .map_err(|_| de::Error::custom(format!("invalid date part: {}", v)))
            }
        }

        deserializer.deserialize_any(DatePartValueVisitor)
    }
}

impl DateVariable {
    /// Get the date parts if available.
    /// If a season is specified but no month is present in date-parts,
    /// the season is converted to a pseudo-month (season + 20) per CSL spec.
    pub fn parts(&self) -> Option<DateParts> {
        self.date_parts.as_ref().and_then(|parts| {
            parts.first().map(|p| {
                let year = p.first().copied();
                let month_from_parts = p.get(1).copied();
                // If there's no month but there's a season, use season as pseudo-month
                // Seasons are 1=spring, 2=summer, 3=fall/autumn, 4=winter
                // Pseudo-months are 21=spring, 22=summer, 23=fall, 24=winter
                let month = month_from_parts.or_else(|| self.season.map(|s| 20 + s));
                DateParts {
                    year,
                    month,
                    day: p.get(2).copied(),
                }
            })
        })
    }

    /// Get the end date parts for a date range.
    pub fn end_parts(&self) -> Option<DateParts> {
        self.date_parts.as_ref().and_then(|parts| {
            parts.get(1).map(|p| DateParts {
                year: p.first().copied(),
                month: p.get(1).copied(),
                day: p.get(2).copied(),
            })
        })
    }

    /// Check if this is a date range.
    pub fn is_range(&self) -> bool {
        self.date_parts.as_ref().is_some_and(|p| p.len() > 1)
    }
}

/// Parsed date parts.
#[derive(Debug, Clone, Copy)]
pub struct DateParts {
    pub year: Option<i32>,
    pub month: Option<i32>,
    pub day: Option<i32>,
}

impl Reference {
    /// Get a text variable by name.
    pub fn get_variable(&self, name: &str) -> Option<String> {
        match name {
            "title" => self.title.clone(),
            "title-short" => self.title_short.clone(),
            "container-title" => self.container_title.clone(),
            "container-title-short" => self.container_title_short.clone(),
            "collection-title" => self.collection_title.clone(),
            "publisher" => self.publisher.clone(),
            "publisher-place" => self.publisher_place.clone(),
            "edition" => self.edition.as_ref().map(|v| v.as_str()),
            "volume" => self.volume.as_ref().map(|v| v.as_str()),
            "issue" => self.issue.as_ref().map(|v| v.as_str()),
            "page" => self.page.clone(),
            "page-first" => {
                // If page-first is explicitly set, use it
                // Otherwise compute from page by extracting the first number
                self.page_first.clone().or_else(|| {
                    self.page.as_ref().and_then(|p| {
                        // Extract first number from page range (e.g., "22-45" -> "22")
                        let first: String = p.chars().take_while(|c| c.is_ascii_digit()).collect();
                        if first.is_empty() { None } else { Some(first) }
                    })
                })
            }
            "number-of-pages" => self.number_of_pages.as_ref().map(|v| v.as_str()),
            "chapter-number" => self.chapter.as_ref().map(|v| v.as_str()),
            "abstract" => self.abstract_.clone(),
            "DOI" => self.doi.clone(),
            "ISBN" => self.isbn.clone(),
            "ISSN" => self.issn.clone(),
            "URL" => self.url.clone(),
            "note" => self.note.clone(),
            "language" => self.language.clone(),
            "source" => self.source.clone(),
            // Citation label: check if explicitly provided in data, otherwise generate
            "citation-label" => {
                // First check if citation-label is explicitly provided in the data
                if let Some(label) = self.other.get("citation-label").and_then(|v| v.as_str()) {
                    Some(label.to_string())
                } else {
                    // Generate from author names + year
                    Some(self.generate_citation_label())
                }
            }
            // Check other fields (handle both strings and numbers)
            _ => self.other.get(name).and_then(|v| {
                if let Some(s) = v.as_str() {
                    Some(s.to_string())
                } else if let Some(n) = v.as_i64() {
                    Some(n.to_string())
                } else if let Some(n) = v.as_u64() {
                    Some(n.to_string())
                } else {
                    v.as_f64().map(|n| format!("{}", n))
                }
            }),
        }
    }

    /// Get a name variable by name.
    pub fn get_names(&self, name: &str) -> Option<&Vec<Name>> {
        match name {
            "author" => self.author.as_ref(),
            "editor" => self.editor.as_ref(),
            "translator" => self.translator.as_ref(),
            "container-author" => self.container_author.as_ref(),
            "collection-editor" => self.collection_editor.as_ref(),
            "director" => self.director.as_ref(),
            "interviewer" => self.interviewer.as_ref(),
            "recipient" => self.recipient.as_ref(),
            "reviewed-author" => self.reviewed_author.as_ref(),
            "composer" => self.composer.as_ref(),
            _ => None,
        }
    }

    /// Extract particles from all name fields.
    ///
    /// This should be called after parsing CSL-JSON to split embedded particles
    /// from given/family names into their proper fields.
    pub fn extract_all_particles(&mut self) {
        // Helper to extract particles from a Vec<Name>
        fn extract_from_names(names: &mut Option<Vec<Name>>) {
            if let Some(names) = names {
                for name in names.iter_mut() {
                    name.extract_particles();
                }
            }
        }

        extract_from_names(&mut self.author);
        extract_from_names(&mut self.editor);
        extract_from_names(&mut self.translator);
        extract_from_names(&mut self.container_author);
        extract_from_names(&mut self.collection_editor);
        extract_from_names(&mut self.director);
        extract_from_names(&mut self.interviewer);
        extract_from_names(&mut self.recipient);
        extract_from_names(&mut self.reviewed_author);
        extract_from_names(&mut self.composer);
    }

    /// Get a date variable by name.
    pub fn get_date(&self, name: &str) -> Option<&DateVariable> {
        match name {
            "issued" => self.issued.as_ref(),
            "accessed" => self.accessed.as_ref(),
            "event-date" => self.event_date.as_ref(),
            "original-date" => self.original_date.as_ref(),
            "submitted" => self.submitted.as_ref(),
            _ => None,
        }
    }

    /// Generate a citation label (trigraph) for this reference.
    ///
    /// The citation label is constructed from author names + year:
    /// - 1 author: first 4 chars of family name
    /// - 2-3 authors: first 2 chars of each family name
    /// - 4+ authors: first 1 char of first 4 family names
    /// - Plus: last 2 digits of year
    ///
    /// Examples:
    /// - "Asthma, Albert (1900)" → "Asth00"
    /// - "Roe + Noakes (1978)" → "RoNo78"
    /// - "von Dipheria + Eczema + Flatulence + Goiter (1926)" → "DEFG26"
    ///
    /// Note: This does NOT include the year suffix (a, b, c...). That is added
    /// during rendering when the citation-label variable is accessed.
    pub fn generate_citation_label(&self) -> String {
        let namepart = self.generate_citation_label_namepart();
        let yearpart = self.generate_citation_label_yearpart();
        format!("{}{}", namepart, yearpart)
    }

    /// Generate the name part of a citation label.
    fn generate_citation_label_namepart(&self) -> String {
        // Get author names, falling back to editor, translator, etc.
        // This matches Pandoc citeproc's behavior which prefers author,
        // then falls back to the first available name variable.
        let names = self
            .author
            .as_ref()
            .or(self.editor.as_ref())
            .or(self.translator.as_ref())
            .or(self.collection_editor.as_ref())
            .or(self.container_author.as_ref())
            .or(self.director.as_ref())
            .or(self.interviewer.as_ref())
            .or(self.recipient.as_ref())
            .or(self.reviewed_author.as_ref())
            .or(self.composer.as_ref());

        let Some(names) = names else {
            return "Xyz".to_string();
        };

        if names.is_empty() {
            return "Xyz".to_string();
        }

        // Determine how many characters to take from each name
        let chars_per_name = match names.len() {
            1 => 4,
            2 | 3 => 2,
            _ => 1, // 4 or more authors
        };

        names
            .iter()
            .take(4) // At most 4 names contribute
            .filter_map(|name| {
                // Get family name, stripping any embedded particle
                // e.g., "von Dipheria" -> "Dipheria" -> "D"
                name.family.as_ref().map(|f| strip_particle(f))
            })
            .map(|family| {
                // Take first N chars, handling Unicode properly
                family.chars().take(chars_per_name).collect::<String>()
            })
            .collect()
    }

    /// Generate the year part of a citation label.
    fn generate_citation_label_yearpart(&self) -> String {
        self.issued
            .as_ref()
            .and_then(|d| d.parts())
            .and_then(|p| p.year)
            .map(|y| format!("{:02}", y.abs() % 100))
            .unwrap_or_default()
    }
}

/// Check if a character is "Byzantine" (Romanesque/Western).
///
/// Based on citeproc-js's ROMANESQUE_REGEX. A name containing any Byzantine
/// character is considered Western and uses Western formatting conventions.
///
/// Byzantine characters include:
/// - ASCII letters and digits (a-z, A-Z, 0-9)
/// - Hyphen
/// - Latin Extended (U+00C0-U+017F)
/// - Greek (U+0370-U+03FF, U+1F00-U+1FFF)
/// - Cyrillic (U+0400-U+052F)
/// - Hebrew (U+0590-U+05D4, U+05D6-U+05FF)
/// - Arabic (U+0600-U+06FF)
/// - Thai (U+0E01-U+0E5B)
/// - Special characters and directional marks
/// Check if a character is particle punctuation.
/// Used for detecting particles in names (e.g., apostrophe in "d'Artagnan").
fn is_particle_punct(c: char) -> bool {
    c == '\'' || c == '\u{2019}' || c == '-' || c == '\u{2013}' || c == '.'
}

fn is_byzantine_char(c: char) -> bool {
    c == '-'
        || c.is_ascii_alphanumeric()
        || ('\u{00c0}'..='\u{017f}').contains(&c) // Latin Extended
        || ('\u{0370}'..='\u{03ff}').contains(&c) // Greek
        || ('\u{0400}'..='\u{052f}').contains(&c) // Cyrillic
        || ('\u{0590}'..='\u{05d4}').contains(&c) // Hebrew (part 1)
        || ('\u{05d6}'..='\u{05ff}').contains(&c) // Hebrew (part 2)
        || ('\u{0600}'..='\u{06ff}').contains(&c) // Arabic
        || ('\u{0e01}'..='\u{0e5b}').contains(&c) // Thai
        || ('\u{1f00}'..='\u{1fff}').contains(&c) // Greek Extended
        || ('\u{200c}'..='\u{200e}').contains(&c) // Zero-width characters
        || ('\u{2018}'..='\u{2019}').contains(&c) // Curly quotes
        || ('\u{021a}'..='\u{021b}').contains(&c) // Romanian letters
        || ('\u{202a}'..='\u{202e}').contains(&c) // Directional formatting
}

/// Strip common particles from the beginning of a family name.
///
/// This handles cases where particles are embedded in the family name
/// (e.g., "von Dipheria") rather than being in the separate
/// `non_dropping_particle` or `dropping_particle` fields.
///
/// Based on the particle patterns from CSL spec and citeproc implementations.
fn strip_particle(family: &str) -> &str {
    // Common particles (lowercase). Order matters for multi-word particles.
    const PARTICLES: &[&str] = &[
        // Multi-word particles first (longer matches before shorter)
        "van de ", "van der ", "van den ", "van het ", "von der ", "von dem ", "von zu ",
        "auf den ", "in de ", "in 't ", "in het ", "uit de ", "uit den ", "op de ",
        // Single-word particles
        "von ", "van ", "de ", "di ", "da ", "del ", "dela ", "della ", "dello ", "den ", "der ",
        "des ", "du ", "la ", "le ", "lo ", "les ", "ten ", "ter ", "te ", "auf ", "zum ", "zur ",
        "vom ", "am ", "el ", "al ", "il ", "dos ", "das ",
        // With apostrophe (these end with the particle including apostrophe)
        "l'", "d'", "'t ",
    ];

    let lower = family.to_lowercase();
    for particle in PARTICLES {
        if lower.starts_with(particle) {
            return &family[particle.len()..];
        }
    }
    family
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_reference() {
        let json = r#"{
            "id": "smith2020",
            "type": "book",
            "title": "A Great Book",
            "author": [{"family": "Smith", "given": "John"}],
            "issued": {"date-parts": [[2020]]}
        }"#;

        let reference: Reference = serde_json::from_str(json).unwrap();
        assert_eq!(reference.id, "smith2020");
        assert_eq!(reference.ref_type, "book");
        assert_eq!(reference.title, Some("A Great Book".to_string()));

        let author = &reference.author.as_ref().unwrap()[0];
        assert_eq!(author.family, Some("Smith".to_string()));
        assert_eq!(author.given, Some("John".to_string()));

        let date = reference.issued.as_ref().unwrap();
        let parts = date.parts().unwrap();
        assert_eq!(parts.year, Some(2020));
    }

    #[test]
    fn test_parse_reference_with_multiple_authors() {
        let json = r#"{
            "id": "team2021",
            "type": "article-journal",
            "title": "Collaborative Work",
            "author": [
                {"family": "Smith", "given": "Alice"},
                {"family": "Jones", "given": "Bob"},
                {"literal": "Research Team"}
            ]
        }"#;

        let reference: Reference = serde_json::from_str(json).unwrap();
        let authors = reference.author.as_ref().unwrap();
        assert_eq!(authors.len(), 3);
        assert_eq!(authors[0].display_name(), "Smith, Alice");
        assert_eq!(authors[1].display_name(), "Jones, Bob");
        assert_eq!(authors[2].display_name(), "Research Team");
    }

    #[test]
    fn test_parse_date_range() {
        let json = r#"{
            "id": "conf2020",
            "type": "paper-conference",
            "event-date": {"date-parts": [[2020, 6, 15], [2020, 6, 17]]}
        }"#;

        let reference: Reference = serde_json::from_str(json).unwrap();
        let date = reference.event_date.as_ref().unwrap();
        assert!(date.is_range());

        let start = date.parts().unwrap();
        assert_eq!(start.year, Some(2020));
        assert_eq!(start.month, Some(6));
        assert_eq!(start.day, Some(15));

        let end = date.end_parts().unwrap();
        assert_eq!(end.day, Some(17));
    }

    #[test]
    fn test_name_with_particles() {
        let name = Name {
            family: Some("Beethoven".to_string()),
            given: Some("Ludwig".to_string()),
            non_dropping_particle: Some("van".to_string()),
            ..Default::default()
        };
        assert_eq!(name.display_name(), "van Beethoven, Ludwig");
    }

    #[test]
    fn test_byzantine_name_detection() {
        // Western name (Byzantine)
        let western = Name {
            family: Some("Smith".to_string()),
            given: Some("John".to_string()),
            ..Default::default()
        };
        assert!(western.is_byzantine());

        // Chinese name (non-Byzantine)
        let chinese = Name {
            family: Some("毛".to_string()),
            given: Some("泽东".to_string()),
            ..Default::default()
        };
        assert!(!chinese.is_byzantine());

        // Japanese name (non-Byzantine)
        let japanese = Name {
            family: Some("山田".to_string()),
            given: Some("太郎".to_string()),
            ..Default::default()
        };
        assert!(!japanese.is_byzantine());

        // Greek name (Byzantine - Greek is included)
        let greek = Name {
            family: Some("Αλέξανδρος".to_string()),
            ..Default::default()
        };
        assert!(greek.is_byzantine());

        // Cyrillic name (Byzantine)
        let russian = Name {
            family: Some("Достоевский".to_string()),
            given: Some("Фёдор".to_string()),
            ..Default::default()
        };
        assert!(russian.is_byzantine());

        // No family name
        let no_family = Name {
            given: Some("Madonna".to_string()),
            ..Default::default()
        };
        assert!(!no_family.is_byzantine());
    }

    #[test]
    fn test_citation_label_one_author() {
        let json = r#"{
            "id": "item1",
            "type": "book",
            "author": [{"family": "Asthma", "given": "Albert"}],
            "issued": {"date-parts": [[1900]]}
        }"#;
        let reference: Reference = serde_json::from_str(json).unwrap();
        assert_eq!(reference.generate_citation_label(), "Asth00");
    }

    #[test]
    fn test_citation_label_two_authors() {
        let json = r#"{
            "id": "item1",
            "type": "book",
            "author": [
                {"family": "Roe", "given": "Jane"},
                {"family": "Noakes", "given": "Richard"}
            ],
            "issued": {"date-parts": [[1978]]}
        }"#;
        let reference: Reference = serde_json::from_str(json).unwrap();
        assert_eq!(reference.generate_citation_label(), "RoNo78");
    }

    #[test]
    fn test_citation_label_three_authors() {
        let json = r#"{
            "id": "item1",
            "type": "book",
            "author": [
                {"family": "Bronchitis", "given": "Buffy"},
                {"family": "Cholera", "given": "Cleopatra"},
                {"family": "Dengue", "given": "Diana"}
            ],
            "issued": {"date-parts": [[1998]]}
        }"#;
        let reference: Reference = serde_json::from_str(json).unwrap();
        assert_eq!(reference.generate_citation_label(), "BrChDe98");
    }

    #[test]
    fn test_citation_label_four_plus_authors() {
        let json = r#"{
            "id": "item1",
            "type": "book",
            "author": [
                {"family": "von Dipheria", "given": "Doug"},
                {"family": "Eczema", "given": "Elihugh"},
                {"family": "Flatulence", "given": "Frankie"},
                {"family": "Goiter", "given": "Gus"},
                {"family": "Hiccups", "given": "Harvey"}
            ],
            "issued": {"date-parts": [[1926]]}
        }"#;
        let reference: Reference = serde_json::from_str(json).unwrap();
        // "von Dipheria" should be stripped to "Dipheria" -> "D"
        assert_eq!(reference.generate_citation_label(), "DEFG26");
    }

    #[test]
    fn test_citation_label_data_override() {
        // When citation-label is provided in data, use it
        let json = r#"{
            "id": "item1",
            "type": "book",
            "citation-label": "CustomLabel",
            "author": [{"family": "Ignored", "given": "Author"}],
            "issued": {"date-parts": [[2000]]}
        }"#;
        let reference: Reference = serde_json::from_str(json).unwrap();
        // Should use the data-provided label, not generate one
        assert_eq!(
            reference.get_variable("citation-label"),
            Some("CustomLabel".to_string())
        );
    }

    #[test]
    fn test_citation_label_no_authors() {
        let json = r#"{
            "id": "item1",
            "type": "book",
            "title": "Anonymous Work",
            "issued": {"date-parts": [[2020]]}
        }"#;
        let reference: Reference = serde_json::from_str(json).unwrap();
        // Falls back to "Xyz" when no authors
        assert_eq!(reference.generate_citation_label(), "Xyz20");
    }

    #[test]
    fn test_citation_label_no_year() {
        let json = r#"{
            "id": "item1",
            "type": "book",
            "author": [{"family": "Smith", "given": "John"}]
        }"#;
        let reference: Reference = serde_json::from_str(json).unwrap();
        // No year means empty year part
        assert_eq!(reference.generate_citation_label(), "Smit");
    }

    #[test]
    fn test_citation_label_short_name() {
        let json = r#"{
            "id": "item1",
            "type": "book",
            "author": [{"family": "Li", "given": "Wei"}],
            "issued": {"date-parts": [[2005]]}
        }"#;
        let reference: Reference = serde_json::from_str(json).unwrap();
        // Short name takes as many chars as available
        assert_eq!(reference.generate_citation_label(), "Li05");
    }

    #[test]
    fn test_strip_particle() {
        assert_eq!(strip_particle("von Beethoven"), "Beethoven");
        assert_eq!(strip_particle("van Gogh"), "Gogh");
        assert_eq!(strip_particle("de la Cruz"), "la Cruz"); // Only strips first particle
        assert_eq!(strip_particle("van der Berg"), "Berg");
        assert_eq!(strip_particle("l'Amour"), "Amour");
        assert_eq!(strip_particle("d'Artagnan"), "Artagnan");
        assert_eq!(strip_particle("Smith"), "Smith"); // No particle
        assert_eq!(strip_particle("Von Trapp"), "Trapp"); // Case-insensitive
    }
}
