/*
 * get_config.rs
 * Copyright (c) 2025 Posit, PBC
 */

//! Support for the `q2 get-config` CLI command (bd-xoaic, GH #256).
//!
//! Runs the parse + metadata-merge prefix of the render pipeline for a single
//! document and returns the fully-merged metadata. This reuses the exact
//! [`MetadataMergeStage`](crate::stage::MetadataMergeStage) a real render runs
//! (decision D3 in the plan): there is **one** merge code path, shared between
//! rendering and `get-config`. No engines, filters, or rendering execute, so the
//! query is cheap — it is the same work Pass-1 already performs per document.
//!
//! The JSON projection of the returned [`ConfigValue`] lives in
//! [`pampa::config_json`].

use std::sync::Arc;

use pampa::pandoc::ASTContext;
use quarto_pandoc_types::ConfigValue;
use quarto_system_runtime::SystemRuntime;

use crate::error::{QuartoError, Result};
use crate::format::Format;
use crate::pipeline::run_pipeline;
use crate::project::{DocumentInfo, ProjectContext};
use crate::render::{BinaryDependencies, RenderContext};
use crate::stage::{MetadataMergeStage, ParseDocumentStage, PipelineStage};

/// JSON-projection helpers for the merged [`ConfigValue`], re-exported so CLI
/// callers can stay within the `quarto-core` API surface rather than depending
/// on `pampa` directly. The projection itself lives in [`pampa::config_json`].
pub use pampa::config_json::{ProseMode, config_value_to_json, navigate};

/// Merge a single document's metadata exactly as a render would, returning the
/// fully-merged `ast.meta` plus the document's [`ASTContext`] (needed to
/// serialize prose values as Pandoc AST in `--output pandoc`).
///
/// The merge applies, in render precedence: project `_quarto.yml`, directory
/// `_metadata.yml` layers, document frontmatter, and `format.<id>.*` flattening
/// for `format`. The pipeline is `[ParseDocumentStage, MetadataMergeStage]`;
/// include expansion, engines, filters, and rendering do not run (includes do
/// not affect `meta`).
pub async fn merge_document_metadata(
    runtime: Arc<dyn SystemRuntime>,
    project: &ProjectContext,
    format: &Format,
    doc_info: &DocumentInfo,
    source_bytes: &[u8],
) -> Result<(ConfigValue, ASTContext)> {
    let source_name = doc_info.input.to_string_lossy().to_string();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(project, doc_info, format, &binaries);

    let stages: Vec<Box<dyn PipelineStage>> = vec![
        Box::new(ParseDocumentStage::new()),
        Box::new(MetadataMergeStage::new()),
    ];

    let (output, _diagnostics) =
        run_pipeline(source_bytes, &source_name, &mut ctx, runtime, stages).await?;

    let doc = output.into_document_ast().ok_or_else(|| {
        QuartoError::other(
            "get-config: pipeline did not produce a DocumentAst after metadata merge",
        )
    })?;

    Ok((doc.ast.meta, doc.ast_context))
}
