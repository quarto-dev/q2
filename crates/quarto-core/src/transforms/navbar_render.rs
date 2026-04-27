/*
 * navbar_render.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! HTML rendering transform for the navbar.
//!
//! Reads the resolved structure from `navigation.navbar` (populated by
//! [`NavbarGenerateTransform`](super::NavbarGenerateTransform) or a user
//! override), produces an HTML string via
//! [`quarto_navigation::render_html::navbar_to_html`], and stores the result
//! at `rendered.navigation.navbar` for the template to inject.
//!
//! ## Skip conditions
//!
//! - `navbar: false` (affirmative disable).
//! - `rendered.navigation.navbar` already populated — user pre-rendered HTML.
//! - `navigation.navbar` absent — nothing to render.

use quarto_navigation::{Navbar, render_html::navbar_to_html};
use quarto_pandoc_types::config_value::ConfigValue;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceInfo;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::AstTransform;
use crate::transforms::is_feature_disabled;

pub struct NavbarRenderTransform;

impl NavbarRenderTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NavbarRenderTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for NavbarRenderTransform {
    fn name(&self) -> &str {
        "navbar-render"
    }

    async fn transform(&self, ast: &mut Pandoc, _ctx: &mut RenderContext) -> Result<()> {
        if is_feature_disabled(&ast.meta, "navbar") {
            return Ok(());
        }

        if ast
            .meta
            .contains_path(&["rendered", "navigation", "navbar"])
        {
            return Ok(());
        }

        let Some(navbar_cv) = ast.meta.get_path(&["navigation", "navbar"]) else {
            return Ok(());
        };

        let navbar = Navbar::from_config_value(navbar_cv);
        let fallback = ast.meta.get("title").and_then(|v| v.as_plain_text());
        let html = navbar_to_html(&navbar, fallback.as_deref());

        ast.meta.insert_path(
            &["rendered", "navigation", "navbar"],
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
    use quarto_navigation::{Navbar, NavbarTitle};
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
        NavbarRenderTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        ast.meta
    }

    #[tokio::test]
    async fn skips_when_navigation_navbar_missing() {
        let out = run(ConfigValue::default()).await;
        assert!(!out.contains_path(&["rendered", "navigation", "navbar"]));
    }

    #[tokio::test]
    async fn skips_when_navbar_false() {
        // Even if navigation.navbar was populated earlier, `navbar: false`
        // must suppress render.
        let navbar = Navbar {
            title: NavbarTitle::Text(s("Ignored")),
            ..Navbar::with_defaults()
        };
        let mut meta = config_map(vec![("navbar", b(false))]);
        meta.insert_path(&["navigation", "navbar"], navbar.to_config_value());
        let out = run(meta).await;
        assert!(!out.contains_path(&["rendered", "navigation", "navbar"]));
    }

    #[tokio::test]
    async fn skips_when_prerendered() {
        let mut meta = ConfigValue::default();
        meta.insert_path(
            &["navigation", "navbar"],
            Navbar::with_defaults().to_config_value(),
        );
        meta.insert_path(
            &["rendered", "navigation", "navbar"],
            s("<nav>User-provided</nav>"),
        );
        let out = run(meta).await;
        let rendered = out.get_path(&["rendered", "navigation", "navbar"]).unwrap();
        assert_eq!(
            rendered.as_plain_text().as_deref(),
            Some("<nav>User-provided</nav>")
        );
    }

    #[tokio::test]
    async fn renders_navbar_html() {
        let navbar = Navbar {
            title: NavbarTitle::Text(s("My Site")),
            background: Some("primary".to_string()),
            ..Navbar::with_defaults()
        };
        let mut meta = ConfigValue::default();
        meta.insert_path(&["navigation", "navbar"], navbar.to_config_value());
        let out = run(meta).await;
        let rendered = out.get_path(&["rendered", "navigation", "navbar"]).unwrap();
        let html = rendered.as_plain_text().unwrap();
        assert!(html.contains("<nav class=\"navbar navbar-expand-lg bg-primary\""));
        assert!(html.contains("My Site"));
    }

    #[tokio::test]
    async fn falls_back_to_document_title() {
        // With no explicit navbar title but a document-level `title:`, the
        // brand anchor uses the document title.
        let mut meta = config_map(vec![("title", s("Doc Title"))]);
        meta.insert_path(
            &["navigation", "navbar"],
            Navbar::with_defaults().to_config_value(),
        );
        let out = run(meta).await;
        let html = out
            .get_path(&["rendered", "navigation", "navbar"])
            .unwrap()
            .as_plain_text()
            .unwrap();
        assert!(html.contains("Doc Title"));
    }
}
