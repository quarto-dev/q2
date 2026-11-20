/*
 * about_page_diagnostics.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Warn that `about:` pages are not supported yet.
 */

//! About page diagnostics (bd-7hzkh4m6).
//!
//! Quarto 1 lays out a website or book page that sets `about:` with one of
//! its about templates (jolla, trestles, solana, marquee, broadside).
//! Quarto 2 does not implement them yet (bd-5xmy5lle), so such a page used
//! to render as a plain document with no diagnostic. This transform warns
//! (`Q-5-57`); it does not rewrite.
//!
//! Scope mirrors Quarto 1, which only builds about pages in the website
//! project type's HTML extras (books inherit them) and not for slides: a
//! standalone document or a non-HTML render ignores `about:` there, so it
//! does here too.

use quarto_error_reporting::DiagnosticMessageBuilder;
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::format::is_revealjs_target;
use crate::project::ProjectKind;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

/// An `about:` page, which q2 renders as a plain document.
const CODE_ABOUT_PAGE_UNSUPPORTED: &str = "Q-5-57";

/// Warns about `about:` front matter without modifying the AST.
pub struct AboutPageDiagnosticsTransform;

impl AboutPageDiagnosticsTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AboutPageDiagnosticsTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for AboutPageDiagnosticsTransform {
    fn name(&self) -> &str {
        "about-page-diagnostics"
    }

    fn phase(&self) -> TransformPhase {
        // Reads front matter, mutates nothing.
        TransformPhase::Normalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        let website = matches!(
            ctx.project.project_kind(),
            ProjectKind::Website | ProjectKind::Book
        );
        if !website || !ctx.format.is_html() || is_revealjs_target(&ctx.format.target_format) {
            return Ok(());
        }
        let Some(about) = ast.meta.get("about") else {
            return Ok(());
        };
        ctx.diagnostics.push(
            DiagnosticMessageBuilder::warning("`about:` pages are not supported yet")
                .with_code(CODE_ABOUT_PAGE_UNSUPPORTED)
                .with_location(about.source_info.clone())
                .problem(
                    "Quarto 2 does not implement about page templates (tracked as \
                     bd-5xmy5lle); this page renders as a plain document.",
                )
                .build(),
        );
        Ok(())
    }
}
