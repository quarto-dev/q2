/*
 * pass1.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Pass 1 for one document: the public entry point.
//!
//! [`profile_document`] runs the head of the render pipeline
//! ([`build_head_stages`]) on caller-supplied bytes and stops at the
//! `DocumentProfile` checkpoint. The project orchestrator's Pass 1 calls it
//! for every document, and long-lived callers (the language server) call it
//! with the text of an unsaved editor buffer, so both compute exactly the
//! profile a full render would.
//!
//! It never reads or writes the profile cache; caching is the caller's
//! business (see `project::orchestrator::pass1_profile_with_cache`).
//!
//! Plan: `claude-notes/plans/2026-11-16-lsp-document-profile-requests.md`
//! (epic bd-3f6ijnw9, child C1).
//!
//! [`build_head_stages`]: crate::pipeline::build_head_stages

use std::sync::Arc;

use pampa::pandoc::ASTContext;
use quarto_error_reporting::DiagnosticMessage;
use quarto_pandoc_types::ConfigValue;
use quarto_source_map::SourceContext;
use quarto_system_runtime::SystemRuntime;

use crate::document_profile::DocumentProfile;
use crate::error::{QuartoError, Result};
use crate::format::Format;
use crate::pipeline::{build_head_stages, run_pipeline};
use crate::project::{DocumentInfo, ProjectContext};
use crate::render::{BinaryDependencies, RenderContext};

/// What Pass 1 knows about one document.
#[derive(Debug)]
pub struct Pass1Document {
    /// The static snapshot extracted at the profile checkpoint.
    pub profile: DocumentProfile,
    /// The document's authored, merged metadata — project `_quarto.yml`,
    /// directory `_metadata.yml` layers, front matter, `format.<id>.*`
    /// flattening — as recorded by `AuthoredMetadataStage` at the end of the
    /// ingest phase, before the enrich stages add derived keys. The same
    /// value `q2 get-config` reports. Deliberately not a `DocumentProfile`
    /// field: it is open-shaped, it repeats the project configuration in
    /// every document, and `ConfigValue`'s serde form is internal.
    pub authored_meta: ConfigValue,
    /// Needed to project prose values in `authored_meta`
    /// (`pampa::config_json::ProseMode::Pandoc`).
    pub ast_context: ASTContext,
    /// Maps the source spans in `profile` and `diagnostics` back to files.
    pub source_context: SourceContext,
    /// Non-fatal diagnostics raised by the head stages. A fatal problem is
    /// the `Err` of [`profile_document`] instead.
    pub diagnostics: Vec<DiagnosticMessage>,
}

/// Run Pass 1 on `source_bytes` as the contents of `doc_info.input`.
///
/// The bytes need not match the file on disk: other files (includes,
/// `_metadata.yml`, `_quarto.yml`) are read through `runtime`.
pub async fn profile_document(
    runtime: Arc<dyn SystemRuntime>,
    project: &ProjectContext,
    format: &Format,
    doc_info: &DocumentInfo,
    source_bytes: &[u8],
) -> Result<Pass1Document> {
    let source_name = doc_info.input.to_string_lossy().to_string();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(project, doc_info, format, &binaries);
    // Pass 1 runs before the real render, which writes the trace. Its own trace would
    // replace the recording a replay is reading (KNR-6).
    ctx.suppress_trace_output = true;

    // Exactly the full pipeline's head, so the Pass-1 profile equals
    // the one the full render computes (see `build_head_stages`).
    // Source conversion runs first: a non-QMD input without it would
    // yield a garbage DocumentProfile / ProjectIndex entry
    // (plan1c §1067-1086); include expansion precedes the checkpoint so
    // transitive `{{< include … >}}` content and body links are visible
    // (bd-xfwx, Phase 8.0d).
    let stages = build_head_stages();

    let (output, diagnostics) =
        run_pipeline(source_bytes, &source_name, &mut ctx, runtime, stages).await?;

    let at_profile = output.into_at_profile().ok_or_else(|| {
        QuartoError::other(
            "Pass 1 did not produce an AtProfile variant — pipeline shape unexpected",
        )
    })?;
    let ast = at_profile.ast;
    let authored_meta = ast.authored_meta.ok_or_else(|| {
        QuartoError::other(
            "Pass 1 did not record authored metadata — AuthoredMetadataStage missing from the head",
        )
    })?;
    Ok(Pass1Document {
        profile: at_profile.profile,
        authored_meta,
        ast_context: ast.ast_context,
        source_context: ast.source_context,
        diagnostics,
    })
}
