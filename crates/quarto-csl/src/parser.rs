//! CSL parser that converts XmlWithSourceInfo to semantic CSL types.

use crate::error::{Error, Result};
use crate::types::*;
use quarto_source_map::SourceInfo;
use quarto_xml::{XmlAttribute, XmlElement, XmlWithSourceInfo};
use std::collections::{HashMap, HashSet};

/// Check if the style explicitly uses the year-suffix or citation-label variable.
/// When true, year suffix should not be added implicitly to dates.
///
/// This returns true if the style uses either:
/// - `year-suffix` variable: explicitly renders year suffix
/// - `citation-label` variable: implicitly includes year suffix in the label
fn check_uses_year_suffix(
    citation: &Layout,
    bibliography: Option<&Layout>,
    macros: &HashMap<String, Macro>,
) -> bool {
    fn check_element(
        el: &Element,
        macros: &HashMap<String, Macro>,
        visited: &mut HashSet<String>,
    ) -> bool {
        match &el.element_type {
            ElementType::Text(text) => {
                if let TextSource::Variable { name, .. } = &text.source {
                    // Both year-suffix and citation-label handle year suffixes
                    // When either is used, we shouldn't add implicit year suffixes to dates
                    if name == "year-suffix" || name == "citation-label" {
                        return true;
                    }
                }
                if let TextSource::Macro { name, .. } = &text.source {
                    // Skip if already visited (prevents infinite recursion with circular macros)
                    if visited.contains(name) {
                        return false;
                    }
                    if let Some(m) = macros.get(name) {
                        visited.insert(name.clone());
                        let result = m.elements.iter().any(|e| check_element(e, macros, visited));
                        visited.remove(name);
                        return result;
                    }
                }
                false
            }
            ElementType::Group(group) => group
                .elements
                .iter()
                .any(|e| check_element(e, macros, visited)),
            ElementType::Choose(choose) => choose.branches.iter().any(|branch| {
                branch
                    .elements
                    .iter()
                    .any(|e| check_element(e, macros, visited))
            }),
            _ => false,
        }
    }

    let mut visited = HashSet::new();

    // Check citation layout
    if citation
        .elements
        .iter()
        .any(|e| check_element(e, macros, &mut visited))
    {
        return true;
    }

    // Check bibliography layout if present
    if let Some(bib) = bibliography {
        visited.clear();
        if bib
            .elements
            .iter()
            .any(|e| check_element(e, macros, &mut visited))
        {
            return true;
        }
    }

    false
}

/// Parse a CSL style from a string.
///
/// This is the main entry point for parsing CSL files. It parses the XML
/// and converts it to semantic CSL types while preserving source locations.
///
/// # Example
///
/// ```rust
/// use quarto_csl::parse_csl;
///
/// let csl = r#"<?xml version="1.0" encoding="utf-8"?>
/// <style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0">
///   <info><title>Test</title></info>
///   <citation><layout><text variable="title"/></layout></citation>
/// </style>"#;
///
/// let style = parse_csl(csl).unwrap();
/// assert_eq!(style.class, quarto_csl::StyleClass::InText);
/// ```
pub fn parse_csl(content: &str) -> Result<Style> {
    let xml = quarto_xml::parse(content)?;
    parse_style(&xml)
}

/// Parse a Style from pre-parsed XML.
pub fn parse_style(xml: &XmlWithSourceInfo) -> Result<Style> {
    let parser = CslParser::new();
    let style = parser.parse_style_element(&xml.root)?;

    // Validate macro references and check for cycles
    validate_macros(&style)?;

    Ok(style)
}

/// Validate all macro references in a style.
///
/// This checks that:
/// 1. All macro references point to defined macros
/// 2. There are no circular macro dependencies
fn validate_macros(style: &Style) -> Result<()> {
    let macro_names: HashSet<&str> = style.macros.keys().map(|s| s.as_str()).collect();

    // Check macro definitions for undefined references
    for (name, macro_def) in &style.macros {
        check_elements_for_undefined_macros(&macro_def.elements, &macro_names)?;

        // Check for circular dependencies starting from this macro
        let mut visited = HashSet::new();
        let mut chain = vec![name.clone()];
        check_macro_cycle(name, &style.macros, &mut visited, &mut chain)?;
    }

    // Check citation layout for undefined macro references
    check_elements_for_undefined_macros(&style.citation.elements, &macro_names)?;

    // Check bibliography layout if present
    if let Some(ref bib) = style.bibliography {
        check_elements_for_undefined_macros(&bib.elements, &macro_names)?;
    }

    Ok(())
}

/// Check elements for undefined macro references.
fn check_elements_for_undefined_macros(
    elements: &[Element],
    defined: &HashSet<&str>,
) -> Result<()> {
    for element in elements {
        check_element_for_undefined_macros(element, defined)?;
    }
    Ok(())
}

/// Check a single element for undefined macro references.
fn check_element_for_undefined_macros(element: &Element, defined: &HashSet<&str>) -> Result<()> {
    match &element.element_type {
        ElementType::Text(text) => {
            if let TextSource::Macro { name, name_source } = &text.source
                && !defined.contains(name.as_str()) {
                    let suggestion = find_similar_macro(name, defined);
                    return Err(Error::UndefinedMacro {
                        name: name.clone(),
                        reference_location: name_source.clone(),
                        suggestion,
                    });
                }
        }
        ElementType::Group(group) => {
            check_elements_for_undefined_macros(&group.elements, defined)?;
        }
        ElementType::Choose(choose) => {
            for branch in &choose.branches {
                check_elements_for_undefined_macros(&branch.elements, defined)?;
            }
        }
        ElementType::Names(names) => {
            if let Some(ref substitute) = names.substitute {
                check_elements_for_undefined_macros(substitute, defined)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Find a similar macro name for suggestions.
fn find_similar_macro(name: &str, defined: &HashSet<&str>) -> Option<String> {
    // Simple Levenshtein-like matching: find macro with smallest edit distance
    let name_lower = name.to_lowercase();
    let mut best: Option<(&str, usize)> = None;

    for &defined_name in defined {
        let dist = levenshtein_distance(&name_lower, &defined_name.to_lowercase());
        if dist <= 3 {
            // Only suggest if reasonably close
            if best.is_none() || dist < best.unwrap().1 {
                best = Some((defined_name, dist));
            }
        }
    }

    best.map(|(s, _)| s.to_string())
}

/// Simple Levenshtein distance calculation.
fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let m = a_chars.len();
    let n = b_chars.len();

    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }

    let mut prev = (0..=n).collect::<Vec<_>>();
    let mut curr = vec![0; n + 1];

    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };
            curr[j] = (prev[j] + 1).min((curr[j - 1] + 1).min(prev[j - 1] + cost));
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[n]
}

/// Check for circular macro dependencies using DFS.
fn check_macro_cycle(
    start: &str,
    macros: &HashMap<String, Macro>,
    visited: &mut HashSet<String>,
    chain: &mut Vec<String>,
) -> Result<()> {
    if let Some(macro_def) = macros.get(start) {
        let refs = collect_macro_refs(&macro_def.elements);

        for (ref_name, ref_location) in refs {
            if chain.contains(&ref_name) {
                // Found a cycle
                chain.push(ref_name);
                return Err(Error::CircularMacro {
                    chain: chain.clone(),
                    location: ref_location,
                });
            }

            if !visited.contains(&ref_name) {
                visited.insert(ref_name.clone());
                chain.push(ref_name.clone());
                check_macro_cycle(&ref_name, macros, visited, chain)?;
                chain.pop();
            }
        }
    }

    Ok(())
}

/// Collect all macro references from elements.
fn collect_macro_refs(elements: &[Element]) -> Vec<(String, SourceInfo)> {
    let mut refs = Vec::new();
    for element in elements {
        collect_macro_refs_from_element(element, &mut refs);
    }
    refs
}

/// Collect macro references from a single element.
fn collect_macro_refs_from_element(element: &Element, refs: &mut Vec<(String, SourceInfo)>) {
    match &element.element_type {
        ElementType::Text(text) => {
            if let TextSource::Macro { name, name_source } = &text.source {
                refs.push((name.clone(), name_source.clone()));
            }
        }
        ElementType::Group(group) => {
            for el in &group.elements {
                collect_macro_refs_from_element(el, refs);
            }
        }
        ElementType::Choose(choose) => {
            for branch in &choose.branches {
                for el in &branch.elements {
                    collect_macro_refs_from_element(el, refs);
                }
            }
        }
        ElementType::Names(names) => {
            if let Some(ref substitute) = names.substitute {
                for el in substitute {
                    collect_macro_refs_from_element(el, refs);
                }
            }
        }
        _ => {}
    }
}

/// Internal CSL parser.
struct CslParser;

impl CslParser {
    fn new() -> Self {
        Self
    }

    fn parse_style_element(&self, element: &XmlElement) -> Result<Style> {
        // Verify root element is <style>
        if element.name != "style" {
            return Err(Error::InvalidRootElement {
                found: element.name.clone(),
                location: element.source_info.clone(),
            });
        }

        // Parse version (required)
        let version_attr = self.require_attr(element, "version")?;
        let version = version_attr.value.clone();
        let version_source = version_attr.value_source.clone();

        // Parse class (required)
        let class_attr = self.require_attr(element, "class")?;
        let class = match class_attr.value.as_str() {
            "in-text" => StyleClass::InText,
            "note" => StyleClass::Note,
            other => {
                return Err(Error::InvalidAttributeValue {
                    element: "style".to_string(),
                    attribute: "class".to_string(),
                    value: other.to_string(),
                    expected: "\"in-text\" or \"note\"".to_string(),
                    location: class_attr.value_source.clone(),
                });
            }
        };

        // Parse optional default-locale
        let default_locale = self
            .get_attr(element, "default-locale")
            .map(|a| a.value.clone());

        // Parse style options from attributes
        let options = self.parse_style_options(element);

        // Parse style-level name options from attributes
        let name_options = self.parse_inheritable_name_options(element);

        // Parse style-level names-delimiter
        let names_delimiter = self
            .get_attr(element, "names-delimiter")
            .map(|a| a.value.clone());

        // Parse child elements
        let mut info = None;
        let mut locales = Vec::new();
        let mut macros: HashMap<String, Macro> = HashMap::new();
        let mut citation = None;
        let mut bibliography = None;

        for child in element.all_children() {
            match child.name.as_str() {
                "info" => {
                    info = Some(self.parse_info(child)?);
                }
                "locale" => {
                    locales.push(self.parse_locale(child)?);
                }
                "macro" => {
                    let macro_def = self.parse_macro(child)?;
                    let name = macro_def.name.clone();
                    if let Some(existing) = macros.get(&name) {
                        return Err(Error::DuplicateMacro {
                            name,
                            first_location: existing.source_info.clone(),
                            second_location: macro_def.source_info.clone(),
                        });
                    }
                    macros.insert(name, macro_def);
                }
                "citation" => {
                    citation = Some(self.parse_layout(child, "citation")?);
                }
                "bibliography" => {
                    bibliography = Some(self.parse_layout(child, "bibliography")?);
                }
                _ => {
                    // Ignore unknown elements (info, etc.)
                }
            }
        }

        // Citation is required
        let citation = citation.ok_or_else(|| Error::MissingElement {
            parent: "style".to_string(),
            element: "citation".to_string(),
            location: element.source_info.clone(),
        })?;

        // Check if the style explicitly uses year-suffix variable
        let uses_year_suffix = check_uses_year_suffix(&citation, bibliography.as_ref(), &macros);

        let mut options = options;
        options.uses_year_suffix_variable = uses_year_suffix;

        Ok(Style {
            version,
            version_source,
            class,
            default_locale,
            options,
            info,
            locales,
            macros,
            citation,
            bibliography,
            name_options,
            names_delimiter,
            source_info: element.source_info.clone(),
        })
    }

    fn parse_style_options(&self, element: &XmlElement) -> StyleOptions {
        let demote = self
            .get_attr(element, "demote-non-dropping-particle")
            .map(|a| match a.value.as_str() {
                "never" => DemoteNonDroppingParticle::Never,
                "sort-only" => DemoteNonDroppingParticle::SortOnly,
                "display-and-sort" => DemoteNonDroppingParticle::DisplayAndSort,
                _ => DemoteNonDroppingParticle::default(),
            })
            .unwrap_or_default();

        let init_hyphen = self
            .get_attr(element, "initialize-with-hyphen")
            .map(|a| a.value == "true")
            .unwrap_or(true);

        let page_range =
            self.get_attr(element, "page-range-format")
                .and_then(|a| match a.value.as_str() {
                    "chicago" | "chicago-15" => Some(PageRangeFormat::Chicago15),
                    "chicago-16" => Some(PageRangeFormat::Chicago16),
                    "expanded" => Some(PageRangeFormat::Expanded),
                    "minimal" => Some(PageRangeFormat::Minimal),
                    "minimal-two" => Some(PageRangeFormat::MinimalTwo),
                    _ => None,
                });

        let limit_day_ordinals = self
            .get_attr(element, "limit-day-ordinals-to-day-1")
            .map(|a| a.value == "true")
            .unwrap_or(false);

        let punctuation_in_quote = self
            .get_attr(element, "punctuation-in-quote")
            .map(|a| a.value == "true")
            .unwrap_or(false);

        StyleOptions {
            demote_non_dropping_particle: demote,
            initialize_with_hyphen: init_hyphen,
            page_range_format: page_range,
            limit_day_ordinals_to_day_1: limit_day_ordinals,
            punctuation_in_quote,
            uses_year_suffix_variable: false, // Will be set during full style parse
            source_info: Some(element.source_info.clone()),
        }
    }

    fn parse_info(&self, element: &XmlElement) -> Result<StyleInfo> {
        let mut title = None;
        let mut title_short = None;
        let mut id = None;
        let mut authors = Vec::new();
        let mut contributors = Vec::new();
        let mut categories = Vec::new();
        let mut updated = None;

        for child in element.all_children() {
            match child.name.as_str() {
                "title" => title = child.text().map(|s| s.to_string()),
                "title-short" => title_short = child.text().map(|s| s.to_string()),
                "id" => id = child.text().map(|s| s.to_string()),
                "author" => authors.push(self.parse_contributor(child)),
                "contributor" => contributors.push(self.parse_contributor(child)),
                "category" => categories.push(self.parse_category(child)),
                "updated" => updated = child.text().map(|s| s.to_string()),
                _ => {}
            }
        }

        Ok(StyleInfo {
            title,
            title_short,
            id,
            authors,
            contributors,
            categories,
            updated,
            source_info: element.source_info.clone(),
        })
    }

    fn parse_contributor(&self, element: &XmlElement) -> Contributor {
        let mut name = None;
        let mut email = None;
        let mut uri = None;

        for child in element.all_children() {
            match child.name.as_str() {
                "name" => name = child.text().map(|s| s.to_string()),
                "email" => email = child.text().map(|s| s.to_string()),
                "uri" => uri = child.text().map(|s| s.to_string()),
                _ => {}
            }
        }

        Contributor { name, email, uri }
    }

    fn parse_category(&self, element: &XmlElement) -> Category {
        Category {
            citation_format: self
                .get_attr(element, "citation-format")
                .map(|a| a.value.clone()),
            field: self.get_attr(element, "field").map(|a| a.value.clone()),
        }
    }

    fn parse_locale(&self, element: &XmlElement) -> Result<Locale> {
        // Get lang from xml:lang attribute
        let lang = element
            .attributes
            .iter()
            .find(|a| a.name == "lang" && a.prefix.as_deref() == Some("xml"))
            .map(|a| a.value.clone());

        let mut terms = Vec::new();
        let mut date_formats = Vec::new();
        let mut options = None;

        for child in element.all_children() {
            match child.name.as_str() {
                "terms" => {
                    for term_el in child.all_children() {
                        if term_el.name == "term" {
                            terms.push(self.parse_term(term_el)?);
                        }
                    }
                }
                "date" => {
                    date_formats.push(self.parse_date_format(child)?);
                }
                "style-options" => {
                    options = Some(self.parse_style_options(child));
                }
                _ => {}
            }
        }

        Ok(Locale {
            lang,
            terms,
            date_formats,
            options,
            source_info: element.source_info.clone(),
        })
    }

    fn parse_term(&self, element: &XmlElement) -> Result<Term> {
        let name = self.require_attr(element, "name")?.value.clone();
        let form = self.parse_term_form(element);

        let mut single = None;
        let mut multiple = None;
        let mut value = None;

        // Check for nested single/multiple elements
        let children = element.all_children();
        if children.is_empty() {
            // Simple term with text content
            // If the element has no text, treat it as an explicitly empty term
            // (e.g., <term name="page"></term> means "page" term is empty)
            value = Some(element.text().unwrap_or("").to_string());
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

    fn parse_term_form(&self, element: &XmlElement) -> TermForm {
        self.get_attr(element, "form")
            .map(|a| match a.value.as_str() {
                "short" => TermForm::Short,
                "verb" => TermForm::Verb,
                "verb-short" => TermForm::VerbShort,
                "symbol" => TermForm::Symbol,
                _ => TermForm::Long,
            })
            .unwrap_or(TermForm::Long)
    }

    fn parse_date_format(&self, element: &XmlElement) -> Result<DateFormat> {
        let form = self
            .get_attr(element, "form")
            .map(|a| match a.value.as_str() {
                "numeric" => DateForm::Numeric,
                _ => DateForm::Text,
            })
            .unwrap_or(DateForm::Text);

        let delimiter = self.get_attr(element, "delimiter").map(|a| a.value.clone());

        let mut parts = Vec::new();
        for child in element.all_children() {
            if child.name == "date-part" {
                parts.push(self.parse_date_part(child)?);
            }
        }

        Ok(DateFormat {
            form,
            parts,
            delimiter,
            source_info: element.source_info.clone(),
        })
    }

    fn parse_macro(&self, element: &XmlElement) -> Result<Macro> {
        let name_attr = self.require_attr(element, "name")?;
        let name = name_attr.value.clone();
        let name_source = name_attr.value_source.clone();

        let elements = self.parse_elements(element)?;

        Ok(Macro {
            name,
            name_source,
            elements,
            source_info: element.source_info.clone(),
        })
    }

    fn parse_layout(&self, element: &XmlElement, _context: &str) -> Result<Layout> {
        let formatting = self.parse_formatting(element);
        let delimiter = self.get_attr(element, "delimiter").map(|a| a.value.clone());

        // Parse inheritable name options from citation/bibliography element
        let name_options = self.parse_inheritable_name_options(element);

        // Find layout child element
        let layout_el = element.get_children("layout");
        let layout_element = if layout_el.is_empty() {
            element
        } else {
            layout_el[0]
        };

        let layout_formatting = {
            let mut fmt = if layout_el.is_empty() {
                formatting.clone()
            } else {
                self.parse_formatting(layout_element)
            };
            // Layout elements should have affixes inside formatting
            // (per CSL spec and Pandoc citeproc reference)
            fmt.affixes_inside = true;
            fmt
        };

        let layout_delimiter = if layout_el.is_empty() {
            delimiter.clone()
        } else {
            self.get_attr(layout_element, "delimiter")
                .map(|a| a.value.clone())
        };

        // Parse sort if present
        let sort = element
            .get_children("sort")
            .first()
            .map(|s| self.parse_sort(s))
            .transpose()?;

        // Parse collapse attributes (only meaningful for citation, but parse anyway)
        let collapse = self
            .get_attr(element, "collapse")
            .map(|a| match a.value.as_str() {
                "citation-number" => Collapse::CitationNumber,
                "year" => Collapse::Year,
                "year-suffix" => Collapse::YearSuffix,
                "year-suffix-ranged" => Collapse::YearSuffixRanged,
                _ => Collapse::None,
            })
            .unwrap_or(Collapse::None);

        let cite_group_delimiter = self
            .get_attr(element, "cite-group-delimiter")
            .map(|a| a.value.clone());

        let after_collapse_delimiter = self
            .get_attr(element, "after-collapse-delimiter")
            .map(|a| a.value.clone());

        let year_suffix_delimiter = self
            .get_attr(element, "year-suffix-delimiter")
            .map(|a| a.value.clone());

        // Parse near-note-distance (defaults to 5 per CSL spec)
        let near_note_distance = self
            .get_attr(element, "near-note-distance")
            .and_then(|a| a.value.parse::<u32>().ok())
            .unwrap_or(5);

        // Parse disambiguation strategy
        let disambiguation = self.parse_disambiguation_strategy(element);

        // Parse second-field-align (bibliography only)
        let second_field_align = self
            .get_attr(element, "second-field-align")
            .and_then(|a| match a.value.as_str() {
                "flush" => Some(SecondFieldAlign::Flush),
                "margin" => Some(SecondFieldAlign::Margin),
                _ => None,
            });

        // Parse subsequent-author-substitute (bibliography only)
        let subsequent_author_substitute = self
            .get_attr(element, "subsequent-author-substitute")
            .map(|a| a.value.clone());

        // Parse subsequent-author-substitute-rule (bibliography only, defaults to CompleteAll)
        let subsequent_author_substitute_rule = self
            .get_attr(element, "subsequent-author-substitute-rule")
            .map(|a| match a.value.as_str() {
                "complete-all" => SubsequentAuthorSubstituteRule::CompleteAll,
                "complete-each" => SubsequentAuthorSubstituteRule::CompleteEach,
                "partial-each" => SubsequentAuthorSubstituteRule::PartialEach,
                "partial-first" => SubsequentAuthorSubstituteRule::PartialFirst,
                _ => SubsequentAuthorSubstituteRule::CompleteAll,
            })
            .unwrap_or_default();

        // Parse names-delimiter (delimiter between name variable groups in <names>)
        let names_delimiter = self
            .get_attr(element, "names-delimiter")
            .map(|a| a.value.clone());

        let elements = self.parse_elements(layout_element)?;

        Ok(Layout {
            formatting: layout_formatting,
            delimiter: layout_delimiter,
            sort,
            name_options,
            names_delimiter,
            elements,
            collapse,
            cite_group_delimiter,
            after_collapse_delimiter,
            year_suffix_delimiter,
            disambiguation,
            near_note_distance,
            second_field_align,
            subsequent_author_substitute,
            subsequent_author_substitute_rule,
            source_info: element.source_info.clone(),
        })
    }

    /// Parse disambiguation strategy from a citation element.
    fn parse_disambiguation_strategy(&self, element: &XmlElement) -> DisambiguationStrategy {
        let add_names = self
            .get_attr(element, "disambiguate-add-names")
            .map(|a| a.value == "true")
            .unwrap_or(false);

        let add_year_suffix = self
            .get_attr(element, "disambiguate-add-year-suffix")
            .map(|a| a.value == "true")
            .unwrap_or(false);

        // disambiguate-add-givenname enables given name disambiguation
        // givenname-disambiguation-rule specifies the rule (defaults to by-cite)
        let add_givenname = self
            .get_attr(element, "disambiguate-add-givenname")
            .and_then(|a| {
                if a.value == "true" {
                    let rule = self
                        .get_attr(element, "givenname-disambiguation-rule")
                        .map(|r| match r.value.as_str() {
                            "all-names" => GivenNameDisambiguationRule::AllNames,
                            "all-names-with-initials" => {
                                GivenNameDisambiguationRule::AllNamesWithInitials
                            }
                            "primary-name" => GivenNameDisambiguationRule::PrimaryName,
                            "primary-name-with-initials" => {
                                GivenNameDisambiguationRule::PrimaryNameWithInitials
                            }
                            _ => GivenNameDisambiguationRule::ByCite, // default
                        })
                        .unwrap_or(GivenNameDisambiguationRule::ByCite);
                    Some(rule)
                } else {
                    None
                }
            });

        DisambiguationStrategy {
            add_names,
            add_givenname,
            add_year_suffix,
        }
    }

    /// Parse inheritable name options from an element (style, citation, bibliography).
    fn parse_inheritable_name_options(&self, element: &XmlElement) -> InheritableNameOptions {
        let and = self
            .get_attr(element, "and")
            .map(|a| match a.value.as_str() {
                "symbol" => NameAnd::Symbol,
                _ => NameAnd::Text,
            });

        let delimiter = self
            .get_attr(element, "name-delimiter")
            .map(|a| a.value.clone());

        let delimiter_precedes_last =
            self.get_attr(element, "delimiter-precedes-last")
                .map(|a| match a.value.as_str() {
                    "always" => DelimiterPrecedesLast::Always,
                    "never" => DelimiterPrecedesLast::Never,
                    "after-inverted-name" => DelimiterPrecedesLast::AfterInvertedName,
                    _ => DelimiterPrecedesLast::Contextual,
                });

        let delimiter_precedes_et_al =
            self.get_attr(element, "delimiter-precedes-et-al")
                .map(|a| match a.value.as_str() {
                    "always" => DelimiterPrecedesLast::Always,
                    "never" => DelimiterPrecedesLast::Never,
                    "after-inverted-name" => DelimiterPrecedesLast::AfterInvertedName,
                    _ => DelimiterPrecedesLast::Contextual,
                });

        let et_al_min = self
            .get_attr(element, "et-al-min")
            .and_then(|a| a.value.parse().ok());
        let et_al_use_first = self
            .get_attr(element, "et-al-use-first")
            .and_then(|a| a.value.parse().ok());
        let et_al_use_last = self
            .get_attr(element, "et-al-use-last")
            .map(|a| a.value == "true");

        let initialize = self
            .get_attr(element, "initialize")
            .map(|a| a.value != "false");

        let initialize_with = self
            .get_attr(element, "initialize-with")
            .map(|a| a.value.clone());

        let form = self
            .get_attr(element, "name-form")
            .map(|a| match a.value.as_str() {
                "short" => NameForm::Short,
                "count" => NameForm::Count,
                _ => NameForm::Long,
            });

        let name_as_sort_order =
            self.get_attr(element, "name-as-sort-order")
                .map(|a| match a.value.as_str() {
                    "all" => NameAsSortOrder::All,
                    _ => NameAsSortOrder::First,
                });

        let sort_separator = self
            .get_attr(element, "sort-separator")
            .map(|a| a.value.clone());

        InheritableNameOptions {
            and,
            delimiter,
            delimiter_precedes_last,
            delimiter_precedes_et_al,
            et_al_min,
            et_al_use_first,
            et_al_use_last,
            initialize,
            initialize_with,
            form,
            name_as_sort_order,
            sort_separator,
        }
    }

    fn parse_sort(&self, element: &XmlElement) -> Result<Sort> {
        let mut keys = Vec::new();
        for child in element.all_children() {
            if child.name == "key" {
                keys.push(self.parse_sort_key(child)?);
            }
        }

        Ok(Sort {
            keys,
            source_info: element.source_info.clone(),
        })
    }

    fn parse_sort_key(&self, element: &XmlElement) -> Result<SortKey> {
        let key = if let Some(var) = self.get_attr(element, "variable") {
            SortKeyType::Variable(var.value.clone())
        } else if let Some(mac) = self.get_attr(element, "macro") {
            SortKeyType::Macro(mac.value.clone())
        } else {
            return Err(Error::MissingAttribute {
                element: "key".to_string(),
                attribute: "variable or macro".to_string(),
                location: element.source_info.clone(),
            });
        };

        let sort_order = self
            .get_attr(element, "sort")
            .map(|a| match a.value.as_str() {
                "descending" => SortOrder::Descending,
                _ => SortOrder::Ascending,
            })
            .unwrap_or_default();

        // Parse name override attributes for sort keys.
        // These map to et-al-min/et-al-use-first/et-al-use-last internally and affect
        // all names generated via macros called by this key.
        let names_min = self
            .get_attr(element, "names-min")
            .and_then(|a| a.value.parse().ok());
        let names_use_first = self
            .get_attr(element, "names-use-first")
            .and_then(|a| a.value.parse().ok());
        let names_use_last = self
            .get_attr(element, "names-use-last")
            .map(|a| a.value == "true");

        Ok(SortKey {
            key,
            sort_order,
            names_min,
            names_use_first,
            names_use_last,
            source_info: element.source_info.clone(),
        })
    }

    fn parse_elements(&self, parent: &XmlElement) -> Result<Vec<Element>> {
        let mut elements = Vec::new();
        for child in parent.all_children() {
            if let Some(el) = self.parse_element(child)? {
                elements.push(el);
            }
        }
        Ok(elements)
    }

    fn parse_element(&self, element: &XmlElement) -> Result<Option<Element>> {
        let element_type = match element.name.as_str() {
            "text" => Some(ElementType::Text(self.parse_text_element(element)?)),
            "number" => Some(ElementType::Number(self.parse_number_element(element)?)),
            "label" => Some(ElementType::Label(self.parse_label_element(element)?)),
            "names" => Some(ElementType::Names(self.parse_names_element(element)?)),
            "date" => Some(ElementType::Date(self.parse_date_element(element)?)),
            "group" => Some(ElementType::Group(self.parse_group_element(element)?)),
            "choose" => Some(ElementType::Choose(self.parse_choose_element(element)?)),
            _ => None, // Ignore unknown elements
        };

        Ok(element_type.map(|et| Element {
            element_type: et,
            formatting: self.parse_formatting(element),
            source_info: element.source_info.clone(),
        }))
    }

    fn parse_text_element(&self, element: &XmlElement) -> Result<TextElement> {
        let source = if let Some(attr) = self.get_attr(element, "variable") {
            let form = self
                .get_attr(element, "form")
                .map(|a| match a.value.as_str() {
                    "short" => VariableForm::Short,
                    _ => VariableForm::Long,
                })
                .unwrap_or(VariableForm::Long);
            TextSource::Variable {
                name: attr.value.clone(),
                name_source: attr.value_source.clone(),
                form,
            }
        } else if let Some(attr) = self.get_attr(element, "macro") {
            TextSource::Macro {
                name: attr.value.clone(),
                name_source: attr.value_source.clone(),
            }
        } else if let Some(attr) = self.get_attr(element, "term") {
            TextSource::Term {
                name: attr.value.clone(),
                form: self.parse_term_form(element),
                plural: self
                    .get_attr(element, "plural")
                    .map(|a| a.value == "true")
                    .unwrap_or(false),
            }
        } else if let Some(attr) = self.get_attr(element, "value") {
            TextSource::Value {
                value: attr.value.clone(),
            }
        } else {
            return Err(Error::MissingTextSource {
                location: element.source_info.clone(),
            });
        };

        Ok(TextElement { source })
    }

    fn parse_number_element(&self, element: &XmlElement) -> Result<NumberElement> {
        let variable = self.require_attr(element, "variable")?.value.clone();
        let form = self
            .get_attr(element, "form")
            .map(|a| match a.value.as_str() {
                "ordinal" => NumberForm::Ordinal,
                "long-ordinal" => NumberForm::LongOrdinal,
                "roman" => NumberForm::Roman,
                _ => NumberForm::Numeric,
            })
            .unwrap_or_default();

        Ok(NumberElement { variable, form })
    }

    fn parse_label_element(&self, element: &XmlElement) -> Result<LabelElement> {
        let variable = self.require_attr(element, "variable")?.value.clone();
        let form = self.parse_term_form(element);
        let plural = self
            .get_attr(element, "plural")
            .map(|a| match a.value.as_str() {
                "always" => LabelPlural::Always,
                "never" => LabelPlural::Never,
                _ => LabelPlural::Contextual,
            })
            .unwrap_or_default();

        Ok(LabelElement {
            variable,
            form,
            plural,
        })
    }

    fn parse_names_element(&self, element: &XmlElement) -> Result<NamesElement> {
        let variables: Vec<String> = self
            .require_attr(element, "variable")?
            .value
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();

        // Parse delimiter between name variable groups (e.g., between author and editor)
        let delimiter = self.get_attr(element, "delimiter").map(|a| a.value.clone());

        let mut name = None;
        let mut et_al = None;
        let mut label = None;
        let mut substitute = None;
        // Track whether label appears before name in the CSL source.
        // If we see <label> before <name>, the label should be rendered first.
        let mut label_before_name = false;

        for child in element.all_children() {
            match child.name.as_str() {
                "name" => name = Some(self.parse_name(child)?),
                "et-al" => et_al = Some(self.parse_et_al(child)),
                "label" => {
                    label = Some(self.parse_names_label(child)?);
                    // If we haven't seen <name> yet, label comes before name
                    label_before_name = name.is_none();
                }
                "substitute" => substitute = Some(self.parse_elements(child)?),
                _ => {}
            }
        }

        Ok(NamesElement {
            variables,
            delimiter,
            name,
            et_al,
            label,
            label_before_name,
            substitute,
        })
    }

    fn parse_name(&self, element: &XmlElement) -> Result<Name> {
        // Parse formatting attributes (prefix, suffix, etc.) from the <name> element
        let formatting = self.parse_formatting(element);
        let formatting = if formatting.has_any_formatting() {
            Some(formatting)
        } else {
            None
        };

        let and = self
            .get_attr(element, "and")
            .map(|a| match a.value.as_str() {
                "symbol" => NameAnd::Symbol,
                _ => NameAnd::Text,
            });

        let delimiter = self.get_attr(element, "delimiter").map(|a| a.value.clone());

        let delimiter_precedes_last =
            self.get_attr(element, "delimiter-precedes-last")
                .map(|a| match a.value.as_str() {
                    "always" => DelimiterPrecedesLast::Always,
                    "never" => DelimiterPrecedesLast::Never,
                    "after-inverted-name" => DelimiterPrecedesLast::AfterInvertedName,
                    _ => DelimiterPrecedesLast::Contextual,
                });

        let delimiter_precedes_et_al =
            self.get_attr(element, "delimiter-precedes-et-al")
                .map(|a| match a.value.as_str() {
                    "always" => DelimiterPrecedesLast::Always,
                    "never" => DelimiterPrecedesLast::Never,
                    "after-inverted-name" => DelimiterPrecedesLast::AfterInvertedName,
                    _ => DelimiterPrecedesLast::Contextual,
                });

        let et_al_min = self
            .get_attr(element, "et-al-min")
            .and_then(|a| a.value.parse().ok());
        let et_al_use_first = self
            .get_attr(element, "et-al-use-first")
            .and_then(|a| a.value.parse().ok());
        let et_al_use_last = self
            .get_attr(element, "et-al-use-last")
            .map(|a| a.value == "true");

        let initialize = self
            .get_attr(element, "initialize")
            .map(|a| a.value != "false");

        let initialize_with = self
            .get_attr(element, "initialize-with")
            .map(|a| a.value.clone());

        let form = self
            .get_attr(element, "form")
            .map(|a| match a.value.as_str() {
                "short" => NameForm::Short,
                "count" => NameForm::Count,
                _ => NameForm::Long,
            });

        let name_as_sort_order =
            self.get_attr(element, "name-as-sort-order")
                .map(|a| match a.value.as_str() {
                    "all" => NameAsSortOrder::All,
                    _ => NameAsSortOrder::First,
                });

        let sort_separator = self
            .get_attr(element, "sort-separator")
            .map(|a| a.value.clone());

        // Parse <name-part> child elements for per-part formatting
        let mut family_formatting = None;
        let mut given_formatting = None;
        for child in element.all_children() {
            if child.name == "name-part"
                && let Some(name_attr) = self.get_attr(child, "name") {
                    let formatting = self.parse_formatting(child);
                    // Only store if there's actual formatting
                    let has_formatting = formatting.font_style.is_some()
                        || formatting.font_weight.is_some()
                        || formatting.font_variant.is_some()
                        || formatting.text_decoration.is_some()
                        || formatting.vertical_align.is_some()
                        || formatting.text_case.is_some()
                        || formatting.prefix.is_some()
                        || formatting.suffix.is_some();
                    if has_formatting {
                        match name_attr.value.as_str() {
                            "family" => family_formatting = Some(formatting),
                            "given" => given_formatting = Some(formatting),
                            _ => {}
                        }
                    }
                }
        }

        Ok(Name {
            and,
            delimiter,
            delimiter_precedes_last,
            delimiter_precedes_et_al,
            et_al_min,
            et_al_use_first,
            et_al_use_last,
            initialize,
            initialize_with,
            form,
            name_as_sort_order,
            sort_separator,
            family_formatting,
            given_formatting,
            formatting,
            source_info: Some(element.source_info.clone()),
        })
    }

    fn parse_et_al(&self, element: &XmlElement) -> EtAl {
        EtAl {
            term: self.get_attr(element, "term").map(|a| a.value.clone()),
            formatting: Some(self.parse_formatting(element)),
        }
    }

    fn parse_names_label(&self, element: &XmlElement) -> Result<NamesLabel> {
        let form = self.parse_term_form(element);
        let plural = self
            .get_attr(element, "plural")
            .map(|a| match a.value.as_str() {
                "always" => LabelPlural::Always,
                "never" => LabelPlural::Never,
                _ => LabelPlural::Contextual,
            })
            .unwrap_or_default();

        Ok(NamesLabel {
            form,
            plural,
            formatting: self.parse_formatting(element),
            source_info: element.source_info.clone(),
        })
    }

    fn parse_date_element(&self, element: &XmlElement) -> Result<DateElement> {
        use crate::DatePartsFilter;

        let variable = self.require_attr(element, "variable")?.value.clone();

        let form = self
            .get_attr(element, "form")
            .map(|a| match a.value.as_str() {
                "numeric" => DateForm::Numeric,
                _ => DateForm::Text,
            });

        let date_parts = self
            .get_attr(element, "date-parts")
            .map(|a| match a.value.as_str() {
                "year" => DatePartsFilter::Year,
                "year-month" => DatePartsFilter::YearMonth,
                _ => DatePartsFilter::YearMonthDay,
            })
            .unwrap_or_default();

        let delimiter = self.get_attr(element, "delimiter").map(|a| a.value.clone());

        let range_delimiter = self
            .get_attr(element, "range-delimiter")
            .map(|a| a.value.clone());

        let mut parts = Vec::new();
        for child in element.all_children() {
            if child.name == "date-part" {
                parts.push(self.parse_date_part(child)?);
            }
        }

        Ok(DateElement {
            variable,
            form,
            date_parts,
            parts,
            delimiter,
            range_delimiter,
        })
    }

    fn parse_date_part(&self, element: &XmlElement) -> Result<DatePart> {
        let name_str = self.require_attr(element, "name")?.value.clone();
        let name = match name_str.as_str() {
            "year" => DatePartName::Year,
            "month" => DatePartName::Month,
            "day" => DatePartName::Day,
            _ => {
                return Err(Error::InvalidAttributeValue {
                    element: "date-part".to_string(),
                    attribute: "name".to_string(),
                    value: name_str,
                    expected: "\"year\", \"month\", or \"day\"".to_string(),
                    location: element.source_info.clone(),
                });
            }
        };

        let form = self
            .get_attr(element, "form")
            .map(|a| match a.value.as_str() {
                "short" => DatePartForm::Short,
                "numeric" => DatePartForm::Numeric,
                "numeric-leading-zeros" => DatePartForm::NumericLeadingZeros,
                "ordinal" => DatePartForm::Ordinal,
                _ => DatePartForm::Long,
            });

        let range_delimiter = self
            .get_attr(element, "range-delimiter")
            .map(|a| a.value.clone());

        let strip_periods = self
            .get_attr(element, "strip-periods")
            .map(|a| a.value == "true")
            .unwrap_or(false);

        Ok(DatePart {
            name,
            form,
            formatting: self.parse_formatting(element),
            range_delimiter,
            strip_periods,
            source_info: element.source_info.clone(),
        })
    }

    fn parse_group_element(&self, element: &XmlElement) -> Result<GroupElement> {
        let delimiter = self.get_attr(element, "delimiter").map(|a| a.value.clone());
        let elements = self.parse_elements(element)?;

        Ok(GroupElement {
            elements,
            delimiter,
        })
    }

    fn parse_choose_element(&self, element: &XmlElement) -> Result<ChooseElement> {
        let mut branches = Vec::new();

        for child in element.all_children() {
            match child.name.as_str() {
                "if" | "else-if" => {
                    branches.push(self.parse_choose_branch(child, false)?);
                }
                "else" => {
                    branches.push(self.parse_choose_branch(child, true)?);
                }
                _ => {}
            }
        }

        Ok(ChooseElement { branches })
    }

    fn parse_choose_branch(&self, element: &XmlElement, is_else: bool) -> Result<ChooseBranch> {
        let conditions = if is_else {
            Vec::new()
        } else {
            self.parse_conditions(element)?
        };

        let match_type = self
            .get_attr(element, "match")
            .map(|a| match a.value.as_str() {
                "any" => MatchType::Any,
                "none" => MatchType::None,
                _ => MatchType::All,
            })
            .unwrap_or_default();

        let elements = self.parse_elements(element)?;

        Ok(ChooseBranch {
            conditions,
            match_type,
            elements,
            source_info: element.source_info.clone(),
        })
    }

    fn parse_conditions(&self, element: &XmlElement) -> Result<Vec<Condition>> {
        let mut conditions = Vec::new();

        if let Some(attr) = self.get_attr(element, "type") {
            let types: Vec<String> = attr
                .value
                .split_whitespace()
                .map(|s| s.to_string())
                .collect();
            conditions.push(Condition {
                condition_type: ConditionType::Type(types),
                source_info: attr.value_source.clone(),
            });
        }

        if let Some(attr) = self.get_attr(element, "variable") {
            let vars: Vec<String> = attr
                .value
                .split_whitespace()
                .map(|s| s.to_string())
                .collect();
            conditions.push(Condition {
                condition_type: ConditionType::Variable(vars),
                source_info: attr.value_source.clone(),
            });
        }

        if let Some(attr) = self.get_attr(element, "is-numeric") {
            let vars: Vec<String> = attr
                .value
                .split_whitespace()
                .map(|s| s.to_string())
                .collect();
            conditions.push(Condition {
                condition_type: ConditionType::IsNumeric(vars),
                source_info: attr.value_source.clone(),
            });
        }

        if let Some(attr) = self.get_attr(element, "is-uncertain-date") {
            let vars: Vec<String> = attr
                .value
                .split_whitespace()
                .map(|s| s.to_string())
                .collect();
            conditions.push(Condition {
                condition_type: ConditionType::IsUncertainDate(vars),
                source_info: attr.value_source.clone(),
            });
        }

        if let Some(attr) = self.get_attr(element, "locator") {
            let locs: Vec<String> = attr
                .value
                .split_whitespace()
                .map(|s| s.to_string())
                .collect();
            conditions.push(Condition {
                condition_type: ConditionType::Locator(locs),
                source_info: attr.value_source.clone(),
            });
        }

        if let Some(attr) = self.get_attr(element, "position") {
            let positions: Vec<Position> = attr
                .value
                .split_whitespace()
                .filter_map(|s| match s {
                    "first" => Some(Position::First),
                    "subsequent" => Some(Position::Subsequent),
                    "ibid-with-locator" => Some(Position::IbidWithLocator),
                    "ibid" => Some(Position::Ibid),
                    "near-note" => Some(Position::NearNote),
                    _ => None,
                })
                .collect();
            conditions.push(Condition {
                condition_type: ConditionType::Position(positions),
                source_info: attr.value_source.clone(),
            });
        }

        if let Some(attr) = self.get_attr(element, "disambiguate") {
            conditions.push(Condition {
                condition_type: ConditionType::Disambiguate(attr.value == "true"),
                source_info: attr.value_source.clone(),
            });
        }

        Ok(conditions)
    }

    fn parse_formatting(&self, element: &XmlElement) -> Formatting {
        Formatting {
            font_style: self
                .get_attr(element, "font-style")
                .and_then(|a| match a.value.as_str() {
                    "italic" => Some(FontStyle::Italic),
                    "oblique" => Some(FontStyle::Oblique),
                    "normal" => Some(FontStyle::Normal),
                    _ => None,
                }),
            font_variant: self.get_attr(element, "font-variant").and_then(|a| {
                match a.value.as_str() {
                    "small-caps" => Some(FontVariant::SmallCaps),
                    "normal" => Some(FontVariant::Normal),
                    _ => None,
                }
            }),
            font_weight: self.get_attr(element, "font-weight").and_then(|a| {
                match a.value.as_str() {
                    "bold" => Some(FontWeight::Bold),
                    "light" => Some(FontWeight::Light),
                    "normal" => Some(FontWeight::Normal),
                    _ => None,
                }
            }),
            text_decoration: self.get_attr(element, "text-decoration").and_then(|a| {
                match a.value.as_str() {
                    "underline" => Some(TextDecoration::Underline),
                    "none" => Some(TextDecoration::None),
                    _ => None,
                }
            }),
            vertical_align: self.get_attr(element, "vertical-align").and_then(|a| {
                match a.value.as_str() {
                    "sup" => Some(VerticalAlign::Sup),
                    "sub" => Some(VerticalAlign::Sub),
                    "baseline" => Some(VerticalAlign::Baseline),
                    _ => None,
                }
            }),
            text_case: self
                .get_attr(element, "text-case")
                .and_then(|a| match a.value.as_str() {
                    "lowercase" => Some(TextCase::Lowercase),
                    "uppercase" => Some(TextCase::Uppercase),
                    "capitalize-first" => Some(TextCase::CapitalizeFirst),
                    "capitalize-all" => Some(TextCase::CapitalizeAll),
                    "sentence" => Some(TextCase::Sentence),
                    "title" => Some(TextCase::Title),
                    _ => None,
                }),
            prefix: self.get_attr(element, "prefix").map(|a| a.value.clone()),
            suffix: self.get_attr(element, "suffix").map(|a| a.value.clone()),
            display: self
                .get_attr(element, "display")
                .and_then(|a| match a.value.as_str() {
                    "block" => Some(Display::Block),
                    "left-margin" => Some(Display::LeftMargin),
                    "right-inline" => Some(Display::RightInline),
                    "indent" => Some(Display::Indent),
                    _ => None,
                }),
            quotes: self
                .get_attr(element, "quotes")
                .map(|a| a.value == "true")
                .unwrap_or(false),
            strip_periods: self
                .get_attr(element, "strip-periods")
                .map(|a| a.value == "true")
                .unwrap_or(false),
            // Delimiter between children (e.g., between multiple name variables)
            delimiter: self.get_attr(element, "delimiter").map(|a| a.value.clone()),
            // Default to false; set to true for layout elements in parse_layout
            affixes_inside: false,
        }
    }

    // Helper methods

    fn require_attr<'a>(&self, element: &'a XmlElement, name: &str) -> Result<&'a XmlAttribute> {
        element
            .attributes
            .iter()
            .find(|a| a.name == name)
            .ok_or_else(|| Error::MissingAttribute {
                element: element.name.clone(),
                attribute: name.to_string(),
                location: element.source_info.clone(),
            })
    }

    fn get_attr<'a>(&self, element: &'a XmlElement, name: &str) -> Option<&'a XmlAttribute> {
        element.attributes.iter().find(|a| a.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_style() {
        let csl = r#"<?xml version="1.0" encoding="utf-8"?>
<style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0">
  <citation><layout><text variable="title"/></layout></citation>
</style>"#;

        let style = parse_csl(csl).unwrap();
        assert_eq!(style.version, "1.0");
        assert_eq!(style.class, StyleClass::InText);
        assert!(!style.citation.elements.is_empty());
    }

    #[test]
    fn test_parse_style_with_macros() {
        let csl = r#"<?xml version="1.0" encoding="utf-8"?>
<style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0">
  <macro name="author">
    <names variable="author"/>
  </macro>
  <citation>
    <layout>
      <text macro="author"/>
    </layout>
  </citation>
</style>"#;

        let style = parse_csl(csl).unwrap();
        assert!(style.macros.contains_key("author"));
    }

    #[test]
    fn test_parse_style_with_choose() {
        let csl = r#"<?xml version="1.0" encoding="utf-8"?>
<style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0">
  <citation>
    <layout>
      <choose>
        <if type="book">
          <text variable="title" font-style="italic"/>
        </if>
        <else>
          <text variable="title"/>
        </else>
      </choose>
    </layout>
  </citation>
</style>"#;

        let style = parse_csl(csl).unwrap();
        let layout = &style.citation;
        assert_eq!(layout.elements.len(), 1);

        if let ElementType::Choose(choose) = &layout.elements[0].element_type {
            assert_eq!(choose.branches.len(), 2);
        } else {
            panic!("Expected Choose element");
        }
    }

    #[test]
    fn test_missing_version_error() {
        let csl = r#"<style xmlns="http://purl.org/net/xbiblio/csl" class="in-text">
  <citation><layout><text variable="title"/></layout></citation>
</style>"#;

        let result = parse_csl(csl);
        assert!(result.is_err());
        if let Err(Error::MissingAttribute { attribute, .. }) = result {
            assert_eq!(attribute, "version");
        } else {
            panic!("Expected MissingAttribute error");
        }
    }

    #[test]
    fn test_missing_citation_error() {
        let csl = r#"<style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0">
</style>"#;

        let result = parse_csl(csl);
        assert!(result.is_err());
        if let Err(Error::MissingElement { element, .. }) = result {
            assert_eq!(element, "citation");
        } else {
            panic!("Expected MissingElement error");
        }
    }

    #[test]
    fn test_parse_formatting_attributes() {
        let csl = r#"<?xml version="1.0" encoding="utf-8"?>
<style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0">
  <citation>
    <layout>
      <text variable="title" font-style="italic" text-case="uppercase" quotes="true"/>
    </layout>
  </citation>
</style>"#;

        let style = parse_csl(csl).unwrap();
        let text_el = &style.citation.elements[0];
        assert_eq!(text_el.formatting.font_style, Some(FontStyle::Italic));
        assert_eq!(text_el.formatting.text_case, Some(TextCase::Uppercase));
        assert!(text_el.formatting.quotes);
    }

    #[test]
    fn test_undefined_macro_error() {
        let csl = r#"<?xml version="1.0" encoding="utf-8"?>
<style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0">
  <citation>
    <layout>
      <text macro="undefined-macro"/>
    </layout>
  </citation>
</style>"#;

        let result = parse_csl(csl);
        assert!(result.is_err());
        if let Err(Error::UndefinedMacro { name, .. }) = result {
            assert_eq!(name, "undefined-macro");
        } else {
            panic!("Expected UndefinedMacro error, got {:?}", result);
        }
    }

    #[test]
    fn test_undefined_macro_with_suggestion() {
        let csl = r#"<?xml version="1.0" encoding="utf-8"?>
<style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0">
  <macro name="author">
    <names variable="author"/>
  </macro>
  <citation>
    <layout>
      <text macro="autor"/>
    </layout>
  </citation>
</style>"#;

        let result = parse_csl(csl);
        assert!(result.is_err());
        if let Err(Error::UndefinedMacro {
            name, suggestion, ..
        }) = result
        {
            assert_eq!(name, "autor");
            assert_eq!(suggestion, Some("author".to_string()));
        } else {
            panic!("Expected UndefinedMacro error, got {:?}", result);
        }
    }

    #[test]
    fn test_circular_macro_error() {
        let csl = r#"<?xml version="1.0" encoding="utf-8"?>
<style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0">
  <macro name="a">
    <text macro="b"/>
  </macro>
  <macro name="b">
    <text macro="a"/>
  </macro>
  <citation>
    <layout>
      <text macro="a"/>
    </layout>
  </citation>
</style>"#;

        let result = parse_csl(csl);
        assert!(result.is_err());
        if let Err(Error::CircularMacro { chain, .. }) = result {
            // Chain should contain a -> b -> a or b -> a -> b
            assert!(chain.len() >= 3);
            // First and last should be the same (cycle detected)
            assert_eq!(chain.first(), chain.last());
        } else {
            panic!("Expected CircularMacro error, got {:?}", result);
        }
    }

    #[test]
    fn test_self_referencing_macro_error() {
        let csl = r#"<?xml version="1.0" encoding="utf-8"?>
<style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0">
  <macro name="self">
    <text macro="self"/>
  </macro>
  <citation>
    <layout>
      <text variable="title"/>
    </layout>
  </citation>
</style>"#;

        let result = parse_csl(csl);
        assert!(result.is_err());
        if let Err(Error::CircularMacro { chain, .. }) = result {
            assert!(chain.contains(&"self".to_string()));
        } else {
            panic!("Expected CircularMacro error, got {:?}", result);
        }
    }

    #[test]
    fn test_valid_macro_chain() {
        let csl = r#"<?xml version="1.0" encoding="utf-8"?>
<style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0">
  <macro name="author">
    <names variable="author"/>
  </macro>
  <macro name="author-short">
    <text macro="author"/>
  </macro>
  <citation>
    <layout>
      <text macro="author-short"/>
    </layout>
  </citation>
</style>"#;

        // Should succeed - no cycle, all macros defined
        let style = parse_csl(csl).unwrap();
        assert!(style.macros.contains_key("author"));
        assert!(style.macros.contains_key("author-short"));
    }

    #[test]
    fn test_undefined_macro_in_macro_definition() {
        let csl = r#"<?xml version="1.0" encoding="utf-8"?>
<style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0">
  <macro name="citation">
    <text macro="nonexistent"/>
  </macro>
  <citation>
    <layout>
      <text variable="title"/>
    </layout>
  </citation>
</style>"#;

        let result = parse_csl(csl);
        assert!(result.is_err());
        if let Err(Error::UndefinedMacro { name, .. }) = result {
            assert_eq!(name, "nonexistent");
        } else {
            panic!("Expected UndefinedMacro error, got {:?}", result);
        }
    }
}
