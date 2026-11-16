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

use quarto_error_reporting::DiagnosticMessage;
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
    /// Maps the source spans in `profile` and `diagnostics` back to files.
    pub source_context: SourceContext,
    /// Non-fatal diagnostics raised by the head stages. A fatal problem is
    /// the `Err` of [`profile_document`] instead.
    pub diagnostics: Vec<DiagnosticMessage>,
}

// No merged metadata here, deliberately. By the checkpoint, stages after
// `MetadataMergeStage` have written their own keys into `ast.meta`
// (`quarto.language`, `rendered.includes`, `listing-item`), so it is not the
// authored merge that `q2 get-config` reports. Callers that want that value
// call `crate::get_config::merge_document_metadata` on the same bytes; it
// stops right after the merge. (Found in bd-n0uth2zt.)

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
    Ok(Pass1Document {
        profile: at_profile.profile,
        source_context: at_profile.ast.source_context,
        diagnostics,
    })
}
