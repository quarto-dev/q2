/*
 * stage/stages/language_resolve.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Resolve the document's localization term table (bd-llhlzd7p).
 */

//! Resolve the document's language term table and inject it into metadata.
//!
//! Runs immediately after [`MetadataMergeStage`](super::MetadataMergeStage),
//! so `lang` and `language:` are already merged across project, directory,
//! and document layers. The stage:
//!
//! 1. reads `lang` (BCP 47 tag, default `"en"`);
//! 2. stacks the term layers, lowest precedence first: the embedded
//!    `_language*.yml` catalog (subtag walk), the project-root
//!    `_language.yml` (+ `_language-<tag>.yml` siblings, projects only),
//!    and the user `language:` value (inline map, or a path to a YAML file
//!    resolved against the document directory, then the project root);
//! 3. injects the resolved table into `doc.ast.meta` at **`quarto.language`**
//!    as literal string scalars.
//!
//! `quarto.language` is the single transport for localized terms: templates
//! read `$quarto.language.<key>$` through the ordinary metadata→template
//! context walk, and AST transforms reconstruct the table with
//! [`LanguageTerms::from_meta`]. Only the `quarto.language` subtree is
//! reserved — the stage preserves any other user-set `quarto.*` keys.
//!
//! Design: `claude-notes/plans/2026-07-17-localization-i18n-design.md`.

use async_trait::async_trait;
use pampa::utils::diagnostic_collector::DiagnosticCollector;
use quarto_error_reporting::DiagnosticMessageBuilder;
use quarto_pandoc_types::{ConfigMapEntry, ConfigValue, ConfigValueKind};
use quarto_source_map::{By, SourceInfo};

use crate::language::{
    LanguageTerms, StructuredTermLayer, language_subtag_prefixes, parse_language_file,
    parse_term_file, resolve_language, structured_layer_from_config,
};
use crate::stage::{PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext};

/// Resolve localized terms into `quarto.language` metadata.
pub struct LanguageResolveStage;

impl LanguageResolveStage {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LanguageResolveStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl PipelineStage for LanguageResolveStage {
    fn name(&self) -> &str {
        "language-resolve"
    }

    fn input_kind(&self) -> PipelineDataKind {
        PipelineDataKind::DocumentAst
    }

    fn output_kind(&self) -> PipelineDataKind {
        PipelineDataKind::DocumentAst
    }

    async fn run(
        &self,
        input: PipelineData,
        ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        let PipelineData::DocumentAst(mut doc) = input else {
            return Err(PipelineError::unexpected_input(
                self.name(),
                self.input_kind(),
                input.kind(),
            ));
        };

        let lang = doc
            .ast
            .meta
            .get("lang")
            .and_then(|v| v.as_plain_text())
            .unwrap_or_else(|| "en".to_string());

        let mut collector = DiagnosticCollector::new();
        let mut layers: Vec<StructuredTermLayer> = Vec::new();

        // Project-root `_language.yml` auto-detection (projects only; Q1:
        // "alongside your _quarto.yml").
        if !ctx.project.is_single_file
            && let Some(layer) = project_root_layer(ctx, &lang, &mut collector)
        {
            layers.push(layer);
        }

        // User `language:` value: an inline map, or a path to a YAML file.
        if let Some(value) = doc.ast.meta.get("language") {
            match &value.value {
                ConfigValueKind::Map(_) => {
                    layers.push(structured_layer_from_config(value, &mut collector));
                }
                _ => {
                    if let Some(path_str) = value.as_plain_text()
                        && let Some(layer) = load_language_file(
                            ctx,
                            &doc.path,
                            &path_str,
                            &value.source_info,
                            &mut collector,
                        )
                    {
                        layers.push(layer);
                    }
                }
            }
        }

        let terms = resolve_language(&lang, &layers);
        inject_quarto_language(&mut doc.ast.meta, &terms);

        ctx.diagnostics.extend(collector.into_diagnostics());
        Ok(PipelineData::DocumentAst(doc))
    }
}

/// Loads the project-root `_language.yml` (plus `_language-<prefix>.yml`
/// siblings along the subtag walk of `lang`) as one structured layer.
fn project_root_layer(
    ctx: &StageContext,
    lang: &str,
    collector: &mut DiagnosticCollector,
) -> Option<StructuredTermLayer> {
    let base_path = ctx.project.dir.join("_language.yml");
    if !ctx.runtime.is_file(&base_path).unwrap_or(false) {
        return None;
    }
    let filename = base_path.to_string_lossy().to_string();
    let content = match ctx.runtime.file_read_string(&base_path) {
        Ok(content) => content,
        Err(e) => {
            collector.add(
                DiagnosticMessageBuilder::warning(format!("could not read {filename}: {e}"))
                    .build(),
            );
            return None;
        }
    };
    let mut layer = match parse_language_file(&content, &filename, collector) {
        Ok(layer) => layer,
        Err(e) => {
            collector.add(DiagnosticMessageBuilder::warning(e.to_string()).build());
            return None;
        }
    };
    // Sibling `_language-<prefix>.yml` files act as per-language sublayers.
    for prefix in language_subtag_prefixes(lang) {
        let sibling = ctx.project.dir.join(format!("_language-{prefix}.yml"));
        if !ctx.runtime.is_file(&sibling).unwrap_or(false) {
            continue;
        }
        let sibling_name = sibling.to_string_lossy().to_string();
        match ctx
            .runtime
            .file_read_string(&sibling)
            .map_err(|e| e.to_string())
            .and_then(|c| parse_term_file(&c, &sibling_name).map_err(|e| e.to_string()))
        {
            Ok(sublayer) => {
                layer.sublayers.insert(prefix, sublayer);
            }
            Err(e) => {
                collector.add(
                    DiagnosticMessageBuilder::warning(format!("could not use {sibling_name}: {e}"))
                        .build(),
                );
            }
        }
    }
    Some(layer)
}

/// Loads a user-specified `language: <file>.yml`, resolving the path against
/// the document directory first, then the project root. A missing or
/// malformed file is an **error** diagnostic (the config asked for a file
/// that cannot be honored); resolution continues with the shipped terms.
fn load_language_file(
    ctx: &StageContext,
    doc_path: &std::path::Path,
    path_str: &str,
    declared_at: &SourceInfo,
    collector: &mut DiagnosticCollector,
) -> Option<StructuredTermLayer> {
    let doc_dir = doc_path
        .parent()
        .map_or_else(|| ctx.project.dir.clone(), |p| p.to_path_buf());
    let candidates = [doc_dir.join(path_str), ctx.project.dir.join(path_str)];
    let Some(path) = candidates
        .iter()
        .find(|p| ctx.runtime.is_file(p).unwrap_or(false))
    else {
        collector.add(
            DiagnosticMessageBuilder::error(format!(
                "specified `language` file does not exist: {path_str}"
            ))
            .add_hint("Is the path relative to the document (or the project root)?")
            .with_location(declared_at.clone())
            .build(),
        );
        return None;
    };
    let filename = path.to_string_lossy().to_string();
    let content = match ctx.runtime.file_read_string(path) {
        Ok(content) => content,
        Err(e) => {
            collector.add(
                DiagnosticMessageBuilder::error(format!("could not read {filename}: {e}"))
                    .with_location(declared_at.clone())
                    .build(),
            );
            return None;
        }
    };
    match parse_language_file(&content, &filename, collector) {
        Ok(layer) => Some(layer),
        Err(e) => {
            collector.add(
                DiagnosticMessageBuilder::error(e.to_string())
                    .with_location(declared_at.clone())
                    .build(),
            );
            None
        }
    }
}

/// Injects the resolved table at `meta.quarto.language`, preserving any
/// other user-set `quarto.*` subkeys.
fn inject_quarto_language(meta: &mut ConfigValue, terms: &LanguageTerms) {
    let table = terms.to_config_value();
    let generated = || SourceInfo::generated(By::programmatic_config());

    let ConfigValueKind::Map(ref mut entries) = meta.value else {
        return;
    };
    let language_entry = |table: ConfigValue| ConfigMapEntry {
        key: "language".to_string(),
        key_source: generated(),
        value: table,
    };
    if let Some(quarto) = entries.iter_mut().find(|e| e.key == "quarto") {
        if let ConfigValueKind::Map(ref mut sub_entries) = quarto.value.value {
            sub_entries.retain(|e| e.key != "language");
            sub_entries.push(language_entry(table));
        } else {
            // A non-map `quarto:` value cannot host subkeys; replace it —
            // the subtree is reserved for Quarto-injected state.
            quarto.value = ConfigValue::new_map(vec![language_entry(table)], generated());
        }
    } else {
        entries.push(ConfigMapEntry {
            key: "quarto".to_string(),
            key_source: generated(),
            value: ConfigValue::new_map(vec![language_entry(table)], generated()),
        });
    }
}
