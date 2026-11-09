/*
 * presenter/cell_step.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * OPR-1a: the one per-cell presenter step.
 */

//! The one per-cell step both presenter loops call (`present_document` for render, the splice
//! stage for preview, the hub and a supplied record).
//!
//! The step reads what the cell supplies ([`CellInputs`]), never which engine made it. It owns
//! the parts of presenting one fenced cell that the two loops used to do twice: partitioning
//! the `#|` options, layering the document scope (D3), expanded cells, forwarding unknown
//! options as cell attributes (D9) and the render itself. The loops keep stored markdown,
//! matching, error policy, figure numbering and stamping.

use quarto_source_map::{By, SourceInfo};

use super::expansion::{render_expanded_unit, split_expansions};
use super::figures::FigureSink;
use super::knitr_inputs::{CellInputs, knitr_header_options, present_with_inputs};
use super::options::CellOptions;
use super::visibility::{CellVisibility, cell_config, resolve_cell_options};
use crate::cell_options::partition_cell_options;
use crate::record::{EngineEntry, Record, RecordCell};
use quarto_pandoc_types::ConfigValue;

/// The entry that ran `cell`: the one its `quarto_engine` names, else the only one.
pub fn engine_entry_for<'a>(
    engines: &'a [EngineEntry],
    cell: &RecordCell,
) -> Option<&'a EngineEntry> {
    match cell.metadata.quarto_engine.as_deref() {
        Some(name) => engines.iter().find(|e| e.name == name),
        None => (engines.len() == 1).then(|| &engines[0]),
    }
}

/// [`engine_entry_for`] over a record\'s entries.
pub fn record_engine_entry<'a>(record: &'a Record, cell: &RecordCell) -> Option<&'a EngineEntry> {
    engine_entry_for(&record.metadata.quarto_engines, cell)
}

/// Present one fenced cell. `author_text` is the unit\'s author text, `attr_tail` the rest of
/// its info string, `scope` the document\'s merged `execute:` scope. The caller has already
/// handled stored markdown and called `fig.begin_cell()`. An `Err` is the message for invalid
/// cell options: render fails with it, the splice keeps the source.
pub fn present_cell_step(
    language: &str,
    attr_tail: &str,
    author_text: &str,
    cell: &RecordCell,
    engine: Option<&EngineEntry>,
    scope: Option<&ConfigValue>,
    fig: &mut dyn FigureSink,
) -> Result<String, String> {
    let parsed =
        partition_cell_options(language, author_text, SourceInfo::generated(By::unknown()))
            .map_err(|e| e.to_string())?;
    let cell_only = cell_config(parsed.options.clone());
    let resolved = resolve_cell_options(scope, parsed.options.clone());
    let visibility = CellVisibility::resolve(resolved.as_ref());
    let options = CellOptions::from_config(resolved.as_ref()).forwarding(cell_only.as_ref());
    if let Some(expansions) = split_expansions(&cell.outputs) {
        // An expanded cell is one unit presented as one cell per expansion, each with its own
        // options.
        return render_expanded_unit(
            language,
            &parsed.code,
            &options,
            visibility,
            &expansions,
            scope,
            fig,
        );
    }
    let (info, lines) = super::knitr_inputs::original_chunk(language, attr_tail, author_text);
    let inputs = CellInputs::from_cell(cell, engine, (&info, &lines));
    let header = inputs
        .resolved_options
        .is_some()
        .then(|| knitr_header_options(parsed.options))
        .flatten();
    Ok(present_with_inputs(
        language,
        &parsed.code,
        cell,
        &inputs,
        header.as_ref(),
        (options, visibility),
        fig,
    ))
}
