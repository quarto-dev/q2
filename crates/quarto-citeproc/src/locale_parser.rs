//! Parser for CSL locale XML files.
//!
//! Locale files are standalone XML files that define language-specific
//! terms, date formats, and style options.

use quarto_csl::{DateFormat, DateForm, DatePart, DatePartForm, DatePartName, Formatting, Locale, Term, TermForm};
use quarto_xml::{parse, XmlElement};

/// Parse a locale XML file.
pub fn parse_locale_xml(xml: &str) -> Result<Locale, String> {
    let doc = parse(xml).map_err(|e| format!("XML parse error: {:?}", e))?;

    let root = &doc.root;

    if root.name != "locale" {
        return Err(format!("Expected <locale> root element, found <{}>", root.name));
    }

    // Get lang from xml:lang attribute
    let lang = root
        .attributes
        .iter()
        .find(|a| a.name == "lang" && a.prefix.as_deref() == Some("xml"))
        .map(|a| a.value.clone());

    let mut terms = Vec::new();
    let mut date_formats = Vec::new();

    for child in root.all_children() {
        match child.name.as_str() {
            "terms" => {
                for term_el in child.all_children() {
                    if term_el.name == "term" {
                        if let Ok(term) = parse_term(term_el) {
                            terms.push(term);
                        }
                    }
                }
            }
            "date" => {
                if let Ok(date_fmt) = parse_date_format(child) {
                    date_formats.push(date_fmt);
                }
            }
            _ => {}
        }
    }

    Ok(Locale {
        lang,
        terms,
        date_formats,
        source_info: root.source_info.clone(),
    })
}

fn parse_term(element: &XmlElement) -> Result<Term, String> {
    let name = element
        .attributes
        .iter()
        .find(|a| a.name == "name")
        .map(|a| a.value.clone())
        .ok_or("Term missing 'name' attribute")?;

    let form = parse_term_form(element);

    let mut single = None;
    let mut multiple = None;
    let mut value = None;

    // Check for nested single/multiple elements
    let children = element.all_children();
    if children.is_empty() {
        // Simple term with text content
        value = element.text().map(|s| s.to_string());
    } else {
        for child in children {
            match child.name.as_str() {
                "single" => single = child.text().map(|s| s.to_string()),
                "multiple" => multiple = child.text().map(|s| s.to_string()),
                _ => {}
            }
        }
    }

    Ok(Term {
        name,
        form,
        single,
        multiple,
        value,
        source_info: element.source_info.clone(),
    })
}

fn parse_term_form(element: &XmlElement) -> TermForm {
    element
        .attributes
        .iter()
        .find(|a| a.name == "form")
        .map(|a| match a.value.as_str() {
            "short" => TermForm::Short,
            "verb" => TermForm::Verb,
            "verb-short" => TermForm::VerbShort,
            "symbol" => TermForm::Symbol,
            _ => TermForm::Long,
        })
        .unwrap_or(TermForm::Long)
}

fn parse_date_format(element: &XmlElement) -> Result<DateFormat, String> {
    let form = element
        .attributes
        .iter()
        .find(|a| a.name == "form")
        .map(|a| match a.value.as_str() {
            "numeric" => DateForm::Numeric,
            _ => DateForm::Text,
        })
        .unwrap_or(DateForm::Text);

    let mut parts = Vec::new();
    for child in element.all_children() {
        if child.name == "date-part" {
            if let Ok(part) = parse_date_part(child) {
                parts.push(part);
            }
        }
    }

    Ok(DateFormat {
        form,
        parts,
        source_info: element.source_info.clone(),
    })
}

fn parse_date_part(element: &XmlElement) -> Result<DatePart, String> {
    let name_str = element
        .attributes
        .iter()
        .find(|a| a.name == "name")
        .map(|a| a.value.clone())
        .ok_or("date-part missing 'name' attribute")?;

    let name = match name_str.as_str() {
        "year" => DatePartName::Year,
        "month" => DatePartName::Month,
        "day" => DatePartName::Day,
        _ => return Err(format!("Unknown date-part name: {}", name_str)),
    };

    let form = element
        .attributes
        .iter()
        .find(|a| a.name == "form")
        .map(|a| match a.value.as_str() {
            "numeric" => DatePartForm::Numeric,
            "numeric-leading-zeros" => DatePartForm::NumericLeadingZeros,
            "ordinal" => DatePartForm::Ordinal,
            "long" => DatePartForm::Long,
            "short" => DatePartForm::Short,
            _ => default_date_part_form(name),
        });

    let prefix = element
        .attributes
        .iter()
        .find(|a| a.name == "prefix")
        .map(|a| a.value.clone());

    let suffix = element
        .attributes
        .iter()
        .find(|a| a.name == "suffix")
        .map(|a| a.value.clone());

    let range_delimiter = element
        .attributes
        .iter()
        .find(|a| a.name == "range-delimiter")
        .map(|a| a.value.clone());

    Ok(DatePart {
        name,
        form,
        formatting: Formatting {
            prefix,
            suffix,
            ..Default::default()
        },
        range_delimiter,
        strip_periods: false,
        source_info: element.source_info.clone(),
    })
}

/// Get the default form for a date part.
fn default_date_part_form(name: DatePartName) -> DatePartForm {
    match name {
        DatePartName::Year => DatePartForm::Long,
        DatePartName::Month => DatePartForm::Long,
        DatePartName::Day => DatePartForm::Numeric,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_locale() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<locale xmlns="http://purl.org/net/xbiblio/csl" version="1.0" xml:lang="en-US">
  <terms>
    <term name="and">and</term>
    <term name="et-al">et al.</term>
    <term name="editor">
      <single>editor</single>
      <multiple>editors</multiple>
    </term>
    <term name="and" form="symbol">&amp;</term>
  </terms>
</locale>"#;

        let locale = parse_locale_xml(xml).unwrap();
        assert_eq!(locale.lang, Some("en-US".to_string()));
        assert_eq!(locale.terms.len(), 4);

        // Check simple term
        let and_term = locale.terms.iter().find(|t| t.name == "and" && t.form == TermForm::Long).unwrap();
        assert_eq!(and_term.value, Some("and".to_string()));

        // Check plural term
        let editor_term = locale.terms.iter().find(|t| t.name == "editor").unwrap();
        assert_eq!(editor_term.single, Some("editor".to_string()));
        assert_eq!(editor_term.multiple, Some("editors".to_string()));

        // Check symbol form
        let and_symbol = locale.terms.iter().find(|t| t.name == "and" && t.form == TermForm::Symbol).unwrap();
        assert_eq!(and_symbol.value, Some("&".to_string()));
    }

    #[test]
    fn test_parse_date_format() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<locale xmlns="http://purl.org/net/xbiblio/csl" version="1.0" xml:lang="en-US">
  <date form="text">
    <date-part name="month" suffix=" "/>
    <date-part name="day" suffix=", "/>
    <date-part name="year"/>
  </date>
</locale>"#;

        let locale = parse_locale_xml(xml).unwrap();
        assert_eq!(locale.date_formats.len(), 1);

        let date_fmt = &locale.date_formats[0];
        assert_eq!(date_fmt.form, DateForm::Text);
        assert_eq!(date_fmt.parts.len(), 3);

        assert_eq!(date_fmt.parts[0].name, DatePartName::Month);
        assert_eq!(date_fmt.parts[0].formatting.suffix, Some(" ".to_string()));
    }
}
