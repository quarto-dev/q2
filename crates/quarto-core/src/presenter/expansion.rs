/*
 * presenter/expansion.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Presenting a cell that an engine expanded into several.
 */

//! A cell the engine expanded (QuartoNotebookRunner's `expand` cells) is one unit in the
//! record, with each expansion attached as extra outputs on that unit's cell.
//!
//! An expansion is a *marker output* (an empty `display_data` whose `quarto_expansion` key
//! holds the expansion's `index` and its `source`, the text with its own `#|` option
//! lines) followed by its outputs, each carrying `quarto_expansion: {index}`. The tag is a
//! key on the output, so a record keeps it with the rest of the output when it is written
//! and read back. A cell with no tagged output is not expanded and presents as one cell.

use quarto_pandoc_types::ConfigValue;
use quarto_source_map::{By, SourceInfo};
use serde_json::Value;

use super::cell::render_cell_with;
use super::figures::FigureSink;
use super::options::CellOptions;
use super::output::CellResult;
use super::visibility::{CellVisibility, resolve_cell_options};
use crate::cell_options::partition_cell_options;

/// The key an expansion's marker and outputs carry.
pub const EXPANSION_KEY: &str = "quarto_expansion";

/// One expansion: its own source text and the outputs it produced.
#[derive(Debug, Clone, PartialEq)]
pub struct Expansion {
    pub source: String,
    pub outputs: Vec<Value>,
}

/// The expansions in a unit's outputs, in order, or `None` when none is tagged.
pub fn split_expansions(outputs: &[Value]) -> Option<Vec<Expansion>> {
    let mut found: Vec<Expansion> = Vec::new();
    for output in outputs {
        let Some(tag) = output.get(EXPANSION_KEY) else {
            continue;
        };
        if let Some(source) = tag.get("source").and_then(Value::as_str) {
            found.push(Expansion {
                source: source.to_string(),
                outputs: Vec::new(),
            });
        } else if let Some(current) = found.last_mut() {
            current.outputs.push(output.clone());
        }
    }
    (!found.is_empty()).then_some(found)
}

/// Present each expansion as its own cell, joined by a newline. The caller has already
/// begun the first cell for figure numbering; this begins one more per further expansion.
///
/// `Err` names an expansion whose `#|` lines are not valid YAML.
pub fn render_expansions(
    lang: &str,
    expansions: &[Expansion],
    scope: Option<&ConfigValue>,
    fig: &mut dyn FigureSink,
) -> Result<String, String> {
    render_each(lang, expansions, scope, fig, false)
}

/// Present an expanded unit as the engine's own markdown does: the author's cell first,
/// with its source shown or hidden by its own options over the document scope and no
/// output of its own, then each expansion as its own cell. The caller has already begun
/// the first cell, which the author's cell takes; each expansion begins its own.
pub fn render_expanded_unit(
    lang: &str,
    own_code: &str,
    own_options: &CellOptions,
    own_visibility: CellVisibility,
    expansions: &[Expansion],
    scope: Option<&ConfigValue>,
    fig: &mut dyn FigureSink,
) -> Result<String, String> {
    let own = render_cell_with(
        lang,
        own_code,
        &CellResult::from_nbformat(&[], None),
        own_options,
        fig,
        own_visibility,
    );
    let rest = render_each(lang, expansions, scope, fig, true)?;
    Ok(if own.is_empty() {
        rest
    } else if rest.is_empty() {
        own
    } else {
        format!("{own}\n{rest}")
    })
}

fn render_each(
    lang: &str,
    expansions: &[Expansion],
    scope: Option<&ConfigValue>,
    fig: &mut dyn FigureSink,
    begin_first: bool,
) -> Result<String, String> {
    let mut parts: Vec<String> = Vec::new();
    for (i, expansion) in expansions.iter().enumerate() {
        let parsed = partition_cell_options(
            lang,
            &expansion.source,
            SourceInfo::generated(By::unknown()),
        )
        .map_err(|e| e.to_string())?;
        let resolved = resolve_cell_options(scope, parsed.options);
        let visibility = CellVisibility::resolve(resolved.as_ref());
        if i > 0 || begin_first {
            fig.begin_cell();
        }
        let result = CellResult::from_nbformat(&expansion.outputs, None);
        let options = CellOptions::from_config(resolved.as_ref());
        let text = render_cell_with(lang, &parsed.code, &result, &options, fig, visibility);
        if !text.is_empty() {
            parts.push(text);
        }
    }
    Ok(parts.join("\n"))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::json;

    use super::*;
    use crate::presenter::figures::FsFigureSink;

    fn marker(index: u64, source: &str) -> Value {
        json!({"output_type": "display_data", "data": {}, "metadata": {},
               "quarto_expansion": {"index": index, "source": source}})
    }

    fn stdout(index: u64, text: &str) -> Value {
        json!({"output_type": "stream", "name": "stdout", "text": [text],
               "quarto_expansion": {"index": index}})
    }

    fn render(lang: &str, expansions: &[Expansion], scope: Option<&ConfigValue>) -> String {
        let mut fig = FsFigureSink::new(Path::new("/nonexistent/doc.qmd"));
        render_expansions(lang, expansions, scope, &mut fig).unwrap()
    }

    #[test]
    fn untagged_outputs_are_not_an_expansion() {
        let plain = json!({"output_type": "stream", "name": "stdout", "text": ["x"]});
        assert_eq!(split_expansions(&[plain]), None);
        assert_eq!(split_expansions(&[]), None);
    }

    #[test]
    fn outputs_group_under_the_marker_before_them() {
        let outputs = vec![
            marker(1, "a()"),
            stdout(1, "A"),
            marker(2, "b()"),
            stdout(2, "B1"),
            stdout(2, "B2"),
        ];
        let split = split_expansions(&outputs).unwrap();
        assert_eq!(split.len(), 2);
        assert_eq!(split[0].source, "a()");
        assert_eq!(split[0].outputs.len(), 1);
        assert_eq!(split[1].source, "b()");
        assert_eq!(split[1].outputs.len(), 2);
    }

    #[test]
    fn each_expansion_is_its_own_cell_with_its_own_echo() {
        let split = split_expansions(&[
            marker(1, "#| echo: false\na()"),
            stdout(1, "A"),
            marker(2, "b()"),
            stdout(2, "B"),
        ])
        .unwrap();
        let md = render("julia", &split, None);
        assert_eq!(md.matches("::: {.cell}").count(), 2, "{md}");
        assert!(
            !md.contains("a()"),
            "first expansion hides its source: {md}"
        );
        assert!(md.contains("b()"), "second expansion echoes: {md}");
        assert!(md.contains("\nA\n") && md.contains("\nB\n"), "{md}");
        // Option lines are consumed, not echoed.
        assert!(!md.contains("#|"), "{md}");
    }

    #[test]
    fn an_expansion_with_no_output_still_echoes_its_source() {
        let split = split_expansions(&[marker(1, "quiet()")]).unwrap();
        let md = render("julia", &split, None);
        assert!(md.contains("quiet()") && md.contains(".cell-code"), "{md}");
    }

    #[test]
    fn a_fully_hidden_expansion_leaves_nothing_between_the_others() {
        let split = split_expansions(&[
            marker(1, "#| include: false\na()"),
            stdout(1, "A"),
            marker(2, "b()"),
            stdout(2, "B"),
        ])
        .unwrap();
        let md = render("julia", &split, None);
        assert_eq!(md.matches("::: {.cell}").count(), 1, "{md}");
    }

    #[test]
    fn an_expansions_option_wins_over_the_document_scope() {
        use quarto_pandoc_types::ConfigMapEntry;
        let scope = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "echo".to_string(),
                key_source: SourceInfo::for_test(),
                value: ConfigValue::new_bool(false, SourceInfo::for_test()),
            }],
            SourceInfo::for_test(),
        );
        let split = split_expansions(&[
            marker(1, "a()"),
            stdout(1, "A"),
            marker(2, "#| echo: true\nb()"),
            stdout(2, "B"),
        ])
        .unwrap();
        let md = render("julia", &split, Some(&scope));
        assert!(!md.contains("a()"), "scope echo: false applies: {md}");
        assert!(
            md.contains("b()"),
            "the expansion's own echo: true wins: {md}"
        );
    }

    #[test]
    fn invalid_option_yaml_in_an_expansion_is_an_error() {
        let split = split_expansions(&[marker(1, "#| echo: [\na()")]).unwrap();
        let mut fig = FsFigureSink::new(Path::new("/nonexistent/doc.qmd"));
        assert!(render_expansions("julia", &split, None, &mut fig).is_err());
    }
}
