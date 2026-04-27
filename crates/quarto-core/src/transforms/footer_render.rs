/*
 * footer_render.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! HTML rendering transform for the page footer.
//!
//! Reads the resolved structure from `navigation.footer` (populated by
//! [`FooterGenerateTransform`](super::FooterGenerateTransform) or a user
//! override), produces an HTML string via
//! [`quarto_navigation::render_html::page_footer_to_html`], and stores the
//! result at `rendered.navigation.footer`.
//!
//! Mirrors [`NavbarRenderTransform`](super::NavbarRenderTransform).

use quarto_navigation::{PageFooter, render_html::page_footer_to_html};
use quarto_pandoc_types::config_value::ConfigValue;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceInfo;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::AstTransform;
use crate::transforms::is_feature_disabled;

pub struct FooterRenderTransform;

impl FooterRenderTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for FooterRenderTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for FooterRenderTransform {
    fn name(&self) -> &str {
        "footer-render"
    }

    async fn transform(&self, ast: &mut Pandoc, _ctx: &mut RenderContext) -> Result<()> {
        if is_feature_disabled(&ast.meta, "page-footer") {
            return Ok(());
        }

        if ast
            .meta
            .contains_path(&["rendered", "navigation", "footer"])
        {
            return Ok(());
        }

        let Some(footer_cv) = ast.meta.get_path(&["navigation", "footer"]) else {
            return Ok(());
        };

        let footer = PageFooter::from_config_value(footer_cv);
        let html = page_footer_to_html(&footer);

        ast.meta.insert_path(
            &["rendered", "navigation", "footer"],
            ConfigValue::new_string(&html, SourceInfo::default()),
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use crate::render::BinaryDependencies;
    use quarto_navigation::{FooterRegion, PageFooter};
    use quarto_pandoc_types::ConfigMapEntry;
    use quarto_pandoc_types::config_value::ConfigValue;
    use quarto_source_map::SourceInfo;
    use std::path::PathBuf;

    fn make_test_project() -> ProjectContext {
        ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/doc.qmd")],
            output_dir: PathBuf::from("/project"),
        }
    }

    fn config_map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        let map_entries: Vec<ConfigMapEntry> = entries
            .into_iter()
            .map(|(k, v)| ConfigMapEntry {
                key: k.to_string(),
                key_source: SourceInfo::default(),
                value: v,
            })
            .collect();
        ConfigValue::new_map(map_entries, SourceInfo::default())
    }

    fn s(x: &str) -> ConfigValue {
        ConfigValue::new_string(x, SourceInfo::default())
    }

    fn b(x: bool) -> ConfigValue {
        ConfigValue::new_bool(x, SourceInfo::default())
    }

    async fn run(meta: ConfigValue) -> ConfigValue {
        let mut ast = Pandoc {
            meta,
            blocks: vec![],
        };
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        FooterRenderTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        ast.meta
    }

    #[tokio::test]
    async fn skips_when_navigation_footer_missing() {
        let out = run(ConfigValue::default()).await;
        assert!(!out.contains_path(&["rendered", "navigation", "footer"]));
    }

    #[tokio::test]
    async fn skips_when_page_footer_false() {
        let footer = PageFooter {
            center: FooterRegion::Text(s("Ignored")),
            ..PageFooter::default()
        };
        let mut meta = config_map(vec![("page-footer", b(false))]);
        meta.insert_path(&["navigation", "footer"], footer.to_config_value());
        let out = run(meta).await;
        assert!(!out.contains_path(&["rendered", "navigation", "footer"]));
    }

    #[tokio::test]
    async fn skips_when_prerendered() {
        let mut meta = ConfigValue::default();
        meta.insert_path(
            &["navigation", "footer"],
            PageFooter::default().to_config_value(),
        );
        meta.insert_path(
            &["rendered", "navigation", "footer"],
            s("<footer>User</footer>"),
        );
        let out = run(meta).await;
        assert_eq!(
            out.get_path(&["rendered", "navigation", "footer"])
                .unwrap()
                .as_plain_text()
                .as_deref(),
            Some("<footer>User</footer>")
        );
    }

    #[tokio::test]
    async fn renders_footer_html() {
        let footer = PageFooter {
            center: FooterRegion::Text(s("Copyright 2026")),
            ..PageFooter::default()
        };
        let mut meta = ConfigValue::default();
        meta.insert_path(&["navigation", "footer"], footer.to_config_value());
        let out = run(meta).await;
        let html = out
            .get_path(&["rendered", "navigation", "footer"])
            .unwrap()
            .as_plain_text()
            .unwrap();
        assert!(html.contains("<footer class=\"footer\">"));
        assert!(html.contains("nav-footer-center"));
        assert!(html.contains("Copyright 2026"));
    }
}
