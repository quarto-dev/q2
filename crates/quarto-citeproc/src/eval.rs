//! Citation evaluation algorithm.
//!
//! This module implements the CSL evaluation algorithm that processes
//! citations and bibliography entries according to a CSL style.
//!
//! The evaluation produces an intermediate `Output` AST that preserves
//! semantic information for post-processing (disambiguation, hyperlinking, etc.),
//! then renders to the final string format.

use crate::output::{join_outputs, Output, Tag};
use crate::reference::Reference;
use crate::types::{Citation, Processor};
use crate::Result;
use quarto_csl::{
    Element, ElementType, Formatting, GroupElement, Layout, NamesElement, TextElement, TextSource,
};

/// Evaluation context for processing a single reference.
struct EvalContext<'a> {
    /// The processor (provides style, locales, references).
    processor: &'a mut Processor,
    /// The reference being processed.
    reference: &'a Reference,
}

impl<'a> EvalContext<'a> {
    fn new(processor: &'a mut Processor, reference: &'a Reference) -> Self {
        Self {
            processor,
            reference,
        }
    }

    /// Get the effective formatting (merged from stack).
    fn effective_formatting(&self, element_formatting: &Formatting) -> Formatting {
        // For now, just use the element's formatting
        // TODO: Properly merge formatting from parent elements
        element_formatting.clone()
    }

    /// Get a term from the locale.
    fn get_term(
        &self,
        name: &str,
        form: quarto_csl::TermForm,
        plural: bool,
    ) -> Option<String> {
        self.processor.get_term(name, form, plural)
    }
}

/// Evaluate a citation and return formatted output.
pub fn evaluate_citation(processor: &mut Processor, citation: &Citation) -> Result<String> {
    // Clone layout to avoid borrow conflicts
    let layout = processor.style.citation.clone();
    let delimiter = layout.delimiter.clone().unwrap_or_else(|| "; ".to_string());

    let mut item_outputs = Vec::new();

    for item in &citation.items {
        let reference = processor
            .get_reference(&item.id)
            .ok_or_else(|| crate::Error::ReferenceNotFound {
                id: item.id.clone(),
                location: None,
            })?
            .clone();

        let mut ctx = EvalContext::new(processor, &reference);
        let output = evaluate_layout(&mut ctx, &layout)?;

        // Apply prefix/suffix from citation item
        let mut parts = Vec::new();
        if let Some(ref prefix) = item.prefix {
            parts.push(Output::tagged(
                Tag::Prefix,
                Output::sequence(vec![Output::literal(prefix), Output::literal(" ")]),
            ));
        }
        parts.push(output);
        if let Some(ref suffix) = item.suffix {
            parts.push(Output::tagged(
                Tag::Suffix,
                Output::sequence(vec![Output::literal(" "), Output::literal(suffix)]),
            ));
        }

        item_outputs.push(Output::sequence(parts));
    }

    let combined = join_outputs(item_outputs, &delimiter);

    // Apply layout-level formatting
    let final_output = Output::formatted(layout.formatting.clone(), vec![combined]);

    Ok(final_output.render())
}

/// Evaluate a bibliography entry.
pub fn evaluate_bibliography_entry(
    processor: &mut Processor,
    reference: &Reference,
) -> Result<String> {
    // Clone layout to avoid borrow conflicts
    let layout = processor
        .style
        .bibliography
        .clone()
        .expect("bibliography layout required");

    let mut ctx = EvalContext::new(processor, reference);
    let output = evaluate_layout(&mut ctx, &layout)?;

    // Apply layout-level formatting
    let final_output = Output::formatted(layout.formatting.clone(), vec![output]);

    Ok(final_output.render())
}

/// Evaluate a layout (citation or bibliography).
fn evaluate_layout(ctx: &mut EvalContext, layout: &Layout) -> Result<Output> {
    let delimiter = layout.delimiter.clone().unwrap_or_default();
    evaluate_elements(ctx, &layout.elements, &delimiter)
}

/// Evaluate a sequence of elements.
fn evaluate_elements(
    ctx: &mut EvalContext,
    elements: &[Element],
    delimiter: &str,
) -> Result<Output> {
    let mut outputs = Vec::new();

    for element in elements {
        let output = evaluate_element(ctx, element)?;
        if !output.is_null() {
            outputs.push(output);
        }
    }

    Ok(join_outputs(outputs, delimiter))
}

/// Evaluate a single element.
fn evaluate_element(ctx: &mut EvalContext, element: &Element) -> Result<Output> {
    let formatting = ctx.effective_formatting(&element.formatting);

    let output = match &element.element_type {
        ElementType::Text(text_el) => evaluate_text(ctx, text_el, &formatting)?,
        ElementType::Names(names_el) => evaluate_names(ctx, names_el, &formatting)?,
        ElementType::Group(group_el) => evaluate_group(ctx, group_el, &formatting)?,
        ElementType::Choose(choose_el) => evaluate_choose(ctx, choose_el)?,
        ElementType::Number(num_el) => evaluate_number(ctx, num_el, &formatting)?,
        ElementType::Label(label_el) => evaluate_label(ctx, label_el, &formatting)?,
        ElementType::Date(date_el) => evaluate_date(ctx, date_el, &formatting)?,
    };

    // Apply formatting (prefix/suffix are part of formatting)
    if output.is_null() {
        Ok(Output::Null)
    } else {
        Ok(Output::formatted(formatting, vec![output]))
    }
}

/// Evaluate a text element.
fn evaluate_text(
    ctx: &mut EvalContext,
    text_el: &TextElement,
    _formatting: &Formatting,
) -> Result<Output> {
    let output = match &text_el.source {
        TextSource::Variable { name, .. } => {
            if let Some(value) = ctx.reference.get_variable(name) {
                // Tag title for potential hyperlinking
                if name == "title" {
                    Output::tagged(Tag::Title, Output::literal(value))
                } else {
                    Output::literal(value)
                }
            } else {
                Output::Null
            }
        }
        TextSource::Macro { name, .. } => {
            // Look up and evaluate the macro
            if let Some(macro_def) = ctx.processor.style.macros.get(name).cloned() {
                let delimiter = "".to_string();
                evaluate_elements(ctx, &macro_def.elements, &delimiter)?
            } else {
                Output::Null
            }
        }
        TextSource::Term { name, form, plural } => {
            if let Some(term) = ctx.get_term(name, *form, *plural) {
                Output::tagged(Tag::Term(name.clone()), Output::literal(term))
            } else {
                Output::Null
            }
        }
        TextSource::Value { value } => Output::literal(value),
    };

    Ok(output)
}

/// Evaluate a names element.
fn evaluate_names(
    ctx: &mut EvalContext,
    names_el: &NamesElement,
    _formatting: &Formatting,
) -> Result<Output> {
    // Try each variable in order until we find one with names
    for var in &names_el.variables {
        if let Some(names) = ctx.reference.get_names(var) {
            if !names.is_empty() {
                // Format the names
                let formatted = format_names(ctx, names, names_el);
                // Tag with names for disambiguation
                return Ok(Output::tagged(
                    Tag::Names {
                        variable: var.clone(),
                        names: names.to_vec(),
                    },
                    Output::literal(formatted),
                ));
            }
        }
    }

    // No names found - try substitute if present
    if let Some(ref substitute) = names_el.substitute {
        for element in substitute {
            let sub_output = evaluate_element(ctx, element)?;
            if !sub_output.is_null() {
                return Ok(sub_output);
            }
        }
    }

    Ok(Output::Null)
}

/// Format a list of names according to CSL rules.
fn format_names(
    ctx: &EvalContext,
    names: &[crate::reference::Name],
    names_el: &NamesElement,
) -> String {
    let name_format = names_el.name.as_ref();

    // Get formatting options
    let delimiter = name_format
        .and_then(|n| n.delimiter.clone())
        .unwrap_or_else(|| ", ".to_string());

    let and_word = name_format.and_then(|n| n.and.as_ref()).map(|a| {
        match a {
            quarto_csl::NameAnd::Text => ctx.get_term("and", quarto_csl::TermForm::Long, false)
                .unwrap_or_else(|| "and".to_string()),
            quarto_csl::NameAnd::Symbol => "&".to_string(),
        }
    });

    // et-al handling
    let et_al_min = name_format.and_then(|n| n.et_al_min).unwrap_or(u32::MAX);
    let et_al_use_first = name_format.and_then(|n| n.et_al_use_first).unwrap_or(1);

    let use_et_al = names.len() as u32 >= et_al_min;
    let names_to_show = if use_et_al {
        et_al_use_first as usize
    } else {
        names.len()
    };

    // Format individual names
    let formatted_names: Vec<String> = names
        .iter()
        .take(names_to_show)
        .map(|n| format_single_name(n, name_format))
        .collect();

    // Join names
    let mut result = if formatted_names.len() == 1 {
        formatted_names[0].clone()
    } else if formatted_names.len() == 2 {
        if let Some(ref and) = and_word {
            format!("{} {} {}", formatted_names[0], and, formatted_names[1])
        } else {
            formatted_names.join(&delimiter)
        }
    } else {
        let last_idx = formatted_names.len() - 1;
        let init = formatted_names[..last_idx].join(&delimiter);
        if let Some(ref and) = and_word {
            format!("{}{} {} {}", init, delimiter, and, formatted_names[last_idx])
        } else {
            format!("{}{}{}", init, delimiter, formatted_names[last_idx])
        }
    };

    // Add et al. if needed
    if use_et_al {
        let et_al = ctx
            .get_term("et-al", quarto_csl::TermForm::Long, false)
            .unwrap_or_else(|| "et al.".to_string());
        result = format!("{} {}", result, et_al);
    }

    result
}

/// Format a single name.
fn format_single_name(
    name: &crate::reference::Name,
    name_format: Option<&quarto_csl::Name>,
) -> String {
    // Handle literal names
    if let Some(ref lit) = name.literal {
        return lit.clone();
    }

    let form = name_format.map(|n| n.form).unwrap_or_default();
    let initialize_with = name_format.and_then(|n| n.initialize_with.clone());
    let sort_separator = name_format
        .and_then(|n| n.sort_separator.clone())
        .unwrap_or_else(|| ", ".to_string());

    match form {
        quarto_csl::NameForm::Short => {
            // Short form: family name only
            let mut parts = Vec::new();
            if let Some(ref ndp) = name.non_dropping_particle {
                parts.push(ndp.clone());
            }
            if let Some(ref family) = name.family {
                parts.push(family.clone());
            }
            parts.join(" ")
        }
        quarto_csl::NameForm::Long | quarto_csl::NameForm::Count => {
            // Long form: family, given
            let mut parts = Vec::new();

            // Non-dropping particle + family
            let family_part = {
                let mut fp = Vec::new();
                if let Some(ref ndp) = name.non_dropping_particle {
                    fp.push(ndp.clone());
                }
                if let Some(ref family) = name.family {
                    fp.push(family.clone());
                }
                fp.join(" ")
            };

            if !family_part.is_empty() {
                parts.push(family_part);
            }

            // Suffix
            if let Some(ref suffix) = name.suffix {
                if !parts.is_empty() {
                    parts.push(suffix.clone());
                }
            }

            // Given name (possibly initialized)
            if let Some(ref given) = name.given {
                let given_formatted = if let Some(ref init) = initialize_with {
                    initialize_name(given, init)
                } else {
                    given.clone()
                };
                parts.push(given_formatted);
            }

            if parts.len() <= 1 {
                parts.join("")
            } else {
                // family, given format
                let family_suffix = parts[..parts.len() - 1].join(", ");
                format!("{}{}{}", family_suffix, sort_separator, parts.last().unwrap())
            }
        }
    }
}

/// Initialize a given name (e.g., "John William" -> "J. W.").
fn initialize_name(given: &str, initialize_with: &str) -> String {
    given
        .split_whitespace()
        .filter_map(|part| part.chars().next())
        .map(|c| format!("{}{}", c.to_uppercase(), initialize_with))
        .collect::<Vec<_>>()
        .join("")
        .trim()
        .to_string()
}

/// Evaluate a group element.
fn evaluate_group(
    ctx: &mut EvalContext,
    group_el: &GroupElement,
    _formatting: &Formatting,
) -> Result<Output> {
    let delimiter = group_el.delimiter.clone().unwrap_or_default();
    let output = evaluate_elements(ctx, &group_el.elements, &delimiter)?;

    // Groups are suppressed if all child elements are empty
    // This is a key CSL behavior
    Ok(output)
}

/// Evaluate a choose element (conditionals).
fn evaluate_choose(
    ctx: &mut EvalContext,
    choose_el: &quarto_csl::ChooseElement,
) -> Result<Output> {
    for branch in &choose_el.branches {
        // Else branch has no conditions
        if branch.conditions.is_empty() {
            return evaluate_elements(ctx, &branch.elements, "");
        }

        // Evaluate conditions based on match type
        let matches = match branch.match_type {
            quarto_csl::MatchType::All => branch
                .conditions
                .iter()
                .all(|c| evaluate_condition(ctx, c)),
            quarto_csl::MatchType::Any => branch
                .conditions
                .iter()
                .any(|c| evaluate_condition(ctx, c)),
            quarto_csl::MatchType::None => branch
                .conditions
                .iter()
                .all(|c| !evaluate_condition(ctx, c)),
        };

        if matches {
            return evaluate_elements(ctx, &branch.elements, "");
        }
    }

    Ok(Output::Null)
}

/// Evaluate a condition.
fn evaluate_condition(ctx: &EvalContext, condition: &quarto_csl::Condition) -> bool {
    use quarto_csl::ConditionType;

    match &condition.condition_type {
        ConditionType::Type(types) => types.iter().any(|t| t == &ctx.reference.ref_type),
        ConditionType::Variable(vars) => vars.iter().any(|v| {
            ctx.reference.get_variable(v).is_some()
                || ctx.reference.get_names(v).is_some()
                || ctx.reference.get_date(v).is_some()
        }),
        ConditionType::IsNumeric(vars) => vars.iter().any(|v| {
            ctx.reference
                .get_variable(v)
                .map(|s| s.chars().all(|c| c.is_ascii_digit() || c == '-'))
                .unwrap_or(false)
        }),
        ConditionType::IsUncertainDate(vars) => vars.iter().any(|v| {
            ctx.reference
                .get_date(v)
                .map(|d| d.circa.unwrap_or(false))
                .unwrap_or(false)
        }),
        ConditionType::Locator(_) => false, // TODO: Implement locator checking
        ConditionType::Position(_) => false, // TODO: Implement position checking
        ConditionType::Disambiguate(_) => false, // TODO: Implement disambiguate checking
    }
}

/// Evaluate a number element.
fn evaluate_number(
    ctx: &mut EvalContext,
    num_el: &quarto_csl::NumberElement,
    _formatting: &Formatting,
) -> Result<Output> {
    if let Some(value) = ctx.reference.get_variable(&num_el.variable) {
        // TODO: Apply number form (ordinal, roman, etc.)
        Ok(Output::literal(value))
    } else {
        Ok(Output::Null)
    }
}

/// Evaluate a label element.
fn evaluate_label(
    ctx: &mut EvalContext,
    label_el: &quarto_csl::LabelElement,
    _formatting: &Formatting,
) -> Result<Output> {
    // Determine if plural
    let is_plural = match label_el.plural {
        quarto_csl::LabelPlural::Always => true,
        quarto_csl::LabelPlural::Never => false,
        quarto_csl::LabelPlural::Contextual => {
            // Check if the variable has multiple values
            // For now, assume singular
            false
        }
    };

    if let Some(term) = ctx.get_term(&label_el.variable, label_el.form, is_plural) {
        Ok(Output::tagged(Tag::Term(label_el.variable.clone()), Output::literal(term)))
    } else {
        Ok(Output::Null)
    }
}

/// Evaluate a date element.
fn evaluate_date(
    ctx: &mut EvalContext,
    date_el: &quarto_csl::DateElement,
    _formatting: &Formatting,
) -> Result<Output> {
    use crate::reference::DateParts;
    use quarto_csl::{DatePartName, DatePartsFilter};

    let Some(date_var) = ctx.reference.get_date(&date_el.variable) else {
        return Ok(Output::Null);
    };

    // Handle literal dates
    if let Some(ref literal) = date_var.literal {
        let output = Output::literal(literal);
        return Ok(Output::tagged(
            Tag::Date(date_el.variable.clone()),
            output,
        ));
    }

    let Some(start_parts) = date_var.parts() else {
        return Ok(Output::Null);
    };

    // Determine which date parts to render based on date_parts filter
    let include_year = true; // Always include year
    let include_month = matches!(
        date_el.date_parts,
        DatePartsFilter::YearMonth | DatePartsFilter::YearMonthDay
    );
    let include_day = matches!(date_el.date_parts, DatePartsFilter::YearMonthDay);

    // Get the date format from the locale if form is specified
    let locale_format = date_el.form.and_then(|form| ctx.processor.get_date_format(form));

    // Build format parts list
    let format_parts: Vec<_> = if let Some(locale_fmt) = locale_format {
        locale_fmt.parts.iter().collect()
    } else if !date_el.parts.is_empty() {
        date_el.parts.iter().collect()
    } else {
        Vec::new()
    };

    // Helper to check if a part should be included
    let should_include_part = |name: DatePartName, parts: &DateParts| -> bool {
        match name {
            DatePartName::Year => include_year && parts.year.is_some(),
            DatePartName::Month => include_month && parts.month.is_some(),
            DatePartName::Day => include_day && parts.day.is_some(),
        }
    };

    // Render the start date
    let start_output = render_date_parts(ctx, &start_parts, &format_parts, &should_include_part);

    // Build the final date output
    let date_output = if let Some(end_parts) = date_var.end_parts() {
        // Get range delimiter (default "–" en-dash)
        let range_delimiter = date_el.range_delimiter.as_deref().unwrap_or("–");

        // Render the end date
        let end_output = render_date_parts(ctx, &end_parts, &format_parts, &should_include_part);

        Output::sequence(vec![
            start_output,
            Output::literal(range_delimiter),
            end_output,
        ])
    } else {
        // Single date
        if format_parts.is_empty() {
            // Just render the year if no format parts
            if let Some(year) = start_parts.year {
                Output::literal(year.to_string())
            } else {
                Output::Null
            }
        } else {
            start_output
        }
    };

    if date_output.is_null() {
        Ok(Output::Null)
    } else {
        // Tag the date output for disambiguation
        Ok(Output::tagged(
            Tag::Date(date_el.variable.clone()),
            date_output,
        ))
    }
}

/// Render date parts according to the format specification.
fn render_date_parts<F>(
    ctx: &EvalContext,
    parts: &crate::reference::DateParts,
    format_parts: &[&quarto_csl::DatePart],
    should_include: &F,
) -> Output
where
    F: Fn(quarto_csl::DatePartName, &crate::reference::DateParts) -> bool,
{
    use quarto_csl::{DatePartForm, DatePartName};

    let mut outputs = Vec::new();

    for part in format_parts {
        if !should_include(part.name, parts) {
            continue;
        }

        let value = match part.name {
            DatePartName::Year => parts.year.map(|y| y.to_string()),
            DatePartName::Month => {
                parts.month.and_then(|m| {
                    let form = part.form.unwrap_or(DatePartForm::Long);
                    format_month_or_season(ctx, m, form)
                })
            }
            DatePartName::Day => {
                parts.day.map(|d| {
                    let form = part.form.unwrap_or(DatePartForm::Numeric);
                    format_day(d, form)
                })
            }
        };

        if let Some(v) = value {
            // Apply prefix
            if let Some(ref prefix) = part.formatting.prefix {
                outputs.push(Output::literal(prefix));
            }

            // Apply the value (with any strip-periods handling)
            let final_value = if part.strip_periods {
                v.replace('.', "")
            } else {
                v
            };
            outputs.push(Output::literal(final_value));

            // Apply suffix
            if let Some(ref suffix) = part.formatting.suffix {
                outputs.push(Output::literal(suffix));
            }
        }
    }

    Output::sequence(outputs)
}

/// Format a month or season value according to the specified form.
/// Months 1-12 are regular months, 21-24 are seasons (spring, summer, fall, winter).
fn format_month_or_season(
    ctx: &EvalContext,
    month: i32,
    form: quarto_csl::DatePartForm,
) -> Option<String> {
    use quarto_csl::{DatePartForm, TermForm};

    // Handle seasons (months 21-24 in CSL)
    if (21..=24).contains(&month) {
        let season_num = month - 20; // 21->1, 22->2, 23->3, 24->4
        let term_name = format!("season-{:02}", season_num);
        return ctx.get_term(&term_name, TermForm::Long, false);
    }

    match form {
        DatePartForm::Long => {
            // Get month name from locale (month-01, month-02, etc.)
            let term_name = format!("month-{:02}", month);
            ctx.get_term(&term_name, TermForm::Long, false)
        }
        DatePartForm::Short => {
            // Get abbreviated month name
            let term_name = format!("month-{:02}", month);
            ctx.get_term(&term_name, TermForm::Short, false)
                .or_else(|| ctx.get_term(&term_name, TermForm::Long, false))
        }
        DatePartForm::Numeric => Some(month.to_string()),
        DatePartForm::NumericLeadingZeros => Some(format!("{:02}", month)),
        DatePartForm::Ordinal => {
            // For now, just return numeric
            // TODO: Implement ordinal formatting with locale suffix
            Some(month.to_string())
        }
    }
}

/// Format a day value according to the specified form.
fn format_day(day: i32, form: quarto_csl::DatePartForm) -> String {
    use quarto_csl::DatePartForm;

    match form {
        DatePartForm::Numeric | DatePartForm::Long | DatePartForm::Short => day.to_string(),
        DatePartForm::NumericLeadingZeros => format!("{:02}", day),
        DatePartForm::Ordinal => {
            // Simple English ordinal suffixes for now
            // TODO: Use locale for ordinal suffixes
            let suffix = match day % 10 {
                1 if day != 11 => "st",
                2 if day != 12 => "nd",
                3 if day != 13 => "rd",
                _ => "th",
            };
            format!("{}{}", day, suffix)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reference::Reference;
    use quarto_csl::parse_csl;

    fn create_test_processor() -> Processor {
        let csl = r#"<?xml version="1.0" encoding="utf-8"?>
<style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0">
  <citation>
    <layout>
      <group delimiter=", ">
        <names variable="author">
          <name form="short"/>
        </names>
        <date variable="issued">
          <date-part name="year"/>
        </date>
      </group>
    </layout>
  </citation>
</style>"#;

        let style = parse_csl(csl).unwrap();
        Processor::new(style)
    }

    #[test]
    fn test_basic_citation() {
        let mut processor = create_test_processor();

        let reference: Reference = serde_json::from_str(
            r#"{
            "id": "smith2020",
            "type": "book",
            "author": [{"family": "Smith", "given": "John"}],
            "issued": {"date-parts": [[2020]]}
        }"#,
        )
        .unwrap();

        processor.add_reference(reference);

        let citation = Citation {
            items: vec![crate::types::CitationItem {
                id: "smith2020".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };

        let result = processor.process_citation(&citation).unwrap();
        assert_eq!(result, "Smith, 2020");
    }

    #[test]
    fn test_initialize_name() {
        // Note: trailing space is trimmed
        assert_eq!(initialize_name("John", ". "), "J.");
        assert_eq!(initialize_name("John William", ". "), "J. W.");
        assert_eq!(initialize_name("J.", ". "), "J.");
    }

    #[test]
    fn test_format_single_name_short() {
        let name = crate::reference::Name {
            family: Some("Smith".to_string()),
            given: Some("John".to_string()),
            ..Default::default()
        };

        let mut format = quarto_csl::Name::default();
        format.form = quarto_csl::NameForm::Short;

        assert_eq!(format_single_name(&name, Some(&format)), "Smith");
    }
}
