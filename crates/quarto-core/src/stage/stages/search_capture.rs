/*
 * stage/stages/search_capture.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Capture each rendered page's body for the website search index
 * (bd-eip0naf4).
 */

//! Store a [`CaptureRecord`] for the page being rendered.
//!
//! Sits between `RenderHtmlBodyStage` and `ApplyTemplateStage`, where
//! the rendered body exists but the template has not yet wrapped it in
//! navigation chrome — exactly the content a reader searches for, and
//! nothing else. The record goes into the artifact store as a
//! path-less, Project-scoped artifact (see `project::search::capture`
//! for the record and the key); `write_search_index` consumes it in
//! `WebsiteProjectType::post_resources`.
//!
//! Native-only: the index is a whole-site post-render product, and the
//! q2-preview pipeline (the WASM hub-client) excludes the surrounding
//! render-html-body / apply-template stages anyway.
//!
//! Every HTML page of a project with search enabled gets a record —
//! [`CaptureRecord::Excluded`] when the page opts out (`search: false`,
//! draft, 404) — so the writer can tell "rendered and excluded" from
//! "not rendered this run" (see the capture module docs).

use std::path::Path;

use async_trait::async_trait;
use quarto_error_reporting::DiagnosticMessageBuilder;

use crate::artifact::{Artifact, ArtifactScope};
use crate::project::ProjectKind;
use crate::project::search::capture::{
    CaptureRecord, PageCapture, artifact_key, crumbs_for_page, is_deck_format, is_indexable_format,
    page_title, prepare_deck_body, profile_is_indexable, restore_heading_ids,
};
use crate::project::search::{page_search_enabled, resolve_website_search};
use crate::stage::{
    EventLevel, PipelineData, PipelineDataKind, PipelineError, PipelineStage, RenderedOutput,
    StageContext,
};
use crate::trace_event;
use crate::transforms::navigation_active::page_relative_source_of;

pub struct SearchCaptureStage;

impl SearchCaptureStage {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SearchCaptureStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl PipelineStage for SearchCaptureStage {
    fn name(&self) -> &str {
        "search-capture"
    }

    fn input_kind(&self) -> PipelineDataKind {
        PipelineDataKind::RenderedOutput
    }

    fn output_kind(&self) -> PipelineDataKind {
        PipelineDataKind::RenderedOutput
    }

    async fn run(
        &self,
        input: PipelineData,
        ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        let PipelineData::RenderedOutput(rendered) = input else {
            return Err(PipelineError::unexpected_input(
                self.name(),
                self.input_kind(),
                input.kind(),
            ));
        };
        if let Some((href, record)) = capture_record(&rendered, ctx) {
            trace_event!(
                ctx,
                EventLevel::Debug,
                "search capture: {} ({})",
                href,
                match &record {
                    CaptureRecord::Page(p) => format!("{} bytes", p.body_html.len()),
                    CaptureRecord::Excluded => "excluded".to_string(),
                }
            );
            ctx.artifacts.store(
                artifact_key(&href),
                Artifact::from_string(record.to_json(), "application/json")
                    .with_scope(ArtifactScope::Project),
            );
        }
        Ok(PipelineData::RenderedOutput(rendered))
    }
}

/// Decide what to record for this page: `None` when the project has
/// no search at all (not a website/book, `website.search: false`, or
/// no project index), otherwise the page's href and its record.
fn capture_record(
    rendered: &RenderedOutput,
    ctx: &mut StageContext,
) -> Option<(String, CaptureRecord)> {
    if !matches!(
        ctx.project.config.project_kind,
        ProjectKind::Website | ProjectKind::Book
    ) {
        return None;
    }
    let meta = &rendered.metadata;
    // Configuration problems are reported once, by the writer; the
    // per-page resolution only needs the answer.
    let mut scratch = Vec::new();
    let opts = resolve_website_search(meta, &mut scratch)?;
    let index = ctx.project_index.clone()?;
    let source = page_relative_source_of(&ctx.document.input, &ctx.project.dir);
    let profile = index.lookup_by_source(Path::new(&source))?;
    let href = profile.output_href.clone();

    let target = &ctx.format.target_format;
    if !is_indexable_format(target)
        || !profile_is_indexable(&index, profile)
        || !page_search_enabled(meta)
    {
        return Some((href, CaptureRecord::Excluded));
    }

    // A deck is indexed per slide, as Quarto 1 does (see
    // `prepare_deck_body`).
    let body = if is_deck_format(target) {
        prepare_deck_body(&rendered.content).and_then(|b| restore_heading_ids(&b))
    } else {
        restore_heading_ids(&rendered.content)
    };
    let body_html = match body {
        Ok(body) => body,
        Err(err) => {
            ctx.diagnostics.push(
                DiagnosticMessageBuilder::warning(format!(
                    "Could not capture `{href}` for the search index"
                ))
                .problem(format!(
                    "{err} The page rendered normally but is left out of search."
                ))
                .build(),
            );
            return Some((href, CaptureRecord::Excluded));
        }
    };
    let capture = PageCapture {
        href: href.clone(),
        title: page_title(profile, meta),
        // Pagefind picks its stemmer from `<html lang>`; without one the
        // page is indexed as `unknown` (no stemming). Quarto's HTML
        // output defaults to English when `lang` is unset, so the index
        // does too.
        lang: Some(
            meta.get("lang")
                .and_then(|v| v.as_plain_text())
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .unwrap_or_else(|| "en".to_string()),
        ),
        crumbs: crumbs_for_page(meta, &source, Some(&index), opts.merge_navbar_crumbs),
        body_html,
    };
    Some((href, CaptureRecord::Page(capture)))
}
