//! Reference types for CSL-JSON bibliographic data.
//!
//! This module defines types for parsing and representing bibliographic
//! references in CSL-JSON format.

use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;

/// Disambiguation state for a reference.
///
/// This is computed during citation processing to resolve ambiguous citations.
#[derive(Debug, Clone, Default)]
pub struct DisambiguationData {
    /// Assigned year suffix (1=a, 2=b, etc.). None means no suffix assigned.
    pub year_suffix: Option<i32>,
    /// Hints for expanding names (per-name map).
    pub name_hints: HashMap<String, NameHint>,
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
    #[serde(deserialize_with = "deserialize_string_or_int")]
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
    #[serde(skip_serializing_if = "Option::is_none")]
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
    pub other: HashMap<String, serde_json::Value>,

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

/// Deserialize a value that can be either a string or an integer into a String.
/// CSL-JSON allows reference IDs to be either strings or integers.
fn deserialize_string_or_int<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;
    let value: serde_json::Value = Deserialize::deserialize(deserializer)?;
    match value {
        serde_json::Value::String(s) => Ok(s),
        serde_json::Value::Number(n) => Ok(n.to_string()),
        _ => Err(Error::custom("expected string or number for id")),
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
    #[serde(rename = "non-dropping-particle", skip_serializing_if = "Option::is_none")]
    pub non_dropping_particle: Option<String>,

    /// Suffix (e.g., "Jr.", "III").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suffix: Option<String>,

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub season: Option<i32>,

    /// Circa flag (approximate date).
    #[serde(skip_serializing_if = "Option::is_none")]
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
                let inner: Vec<i32> = inner_value.into_iter().map(|v| v.0).collect();
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
struct DatePartValue(i32);

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
                Ok(DatePartValue(v as i32))
            }

            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(DatePartValue(v as i32))
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                v.parse::<i32>()
                    .map(DatePartValue)
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
                let month = month_from_parts.or_else(|| {
                    self.season.map(|s| 20 + s)
                });
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
        self.date_parts
            .as_ref()
            .map(|p| p.len() > 1)
            .unwrap_or(false)
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
                        if first.is_empty() {
                            None
                        } else {
                            Some(first)
                        }
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
            // Check other fields
            _ => self
                .other
                .get(name)
                .and_then(|v| v.as_str().map(|s| s.to_string())),
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
}
