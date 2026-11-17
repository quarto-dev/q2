/*
 * stage/stages/authored_metadata.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Record the document's authored, merged metadata at the end of the
 * ingest phase (bd-xzsnuoqq).
 */

//! The authored-metadata checkpoint.
//!
//! The head pipeline ingests a document (source conversion, parse,
//! metadata merge) and then enriches it: `LanguageResolveStage`,
//! `IncludeResolveStage` and `ListingItemInfoStage` write derived keys
//! (`quarto.language`, `rendered.includes`, `listing-item`) into
//! `ast.meta`, because templates, Lua filters and listing templates read
//! them from there. After that, `ast.meta` is no longer what the author
//! wrote.
//!
//! This stage sits on the boundary and copies `ast.meta` into
//! [`DocumentAst::authored_meta`]: project `_quarto.yml`, directory
//! `_metadata.yml` layers, front matter and `format.<id>.*` flattening,
//! and nothing derived. It is the value `q2 get-config` reports (which
//! runs the ingest phase alone, see
//! [`crate::get_config::merge_document_metadata`]) and the value the
//! language server answers `quarto/documentMetadata` with, taken from the
//! same Pass-1 run as the profile.
//!
//! See `crate::pipeline::build_head_stages` for the phase layout.
//!
//! [`DocumentAst::authored_meta`]: crate::stage::DocumentAst::authored_meta

use async_trait::async_trait;

use crate::stage::{PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext};

/// Copies the merged metadata into [`crate::stage::DocumentAst::authored_meta`].
/// Read-only with respect to the AST.
pub struct AuthoredMetadataStage;

impl AuthoredMetadataStage {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AuthoredMetadataStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl PipelineStage for AuthoredMetadataStage {
    fn name(&self) -> &str {
        "authored-metadata"
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
        _ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        let PipelineData::DocumentAst(mut doc) = input else {
            return Err(PipelineError::unexpected_input(
                self.name(),
                self.input_kind(),
                input.kind(),
            ));
        };
        doc.authored_meta = Some(doc.ast.meta.clone());
        Ok(PipelineData::DocumentAst(doc))
    }
}
