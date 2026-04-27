/*
 * footer_generate.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Page-footer resolution transform.
//!
//! Reads the raw `page-footer:` YAML from merged metadata, hands it to
//! [`quarto_navigation::resolve_page_footer`], and stores the resolved
//! structure at `navigation.footer`. Mirrors
//! [`NavbarGenerateTransform`](super::NavbarGenerateTransform).
//!
//! ## Skip conditions
//!
//! - `page-footer: false` (affirmative disable).
//! - `page-footer` absent or `page-footer: true`.
//! - `navigation.footer` already populated (user override).

use quarto_navigation::resolve_page_footer;
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::AstTransform;
use crate::transforms::is_feature_disabled;

/// Transform that resolves the user's `page-footer:` config and stores it at
/// `navigation.footer`.
pub struct FooterGenerateTransform;

impl FooterGenerateTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for FooterGenerateTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for FooterGenerateTransform {
    fn name(&self) -> &str {
        "footer-generate"
    }

    async fn transform(&self, ast: &mut Pandoc, _ctx: &mut RenderContext) -> Result<()> {
        if is_feature_disabled(&ast.meta, "page-footer") {
            return Ok(());
        }

        if ast.meta.contains_path(&["navigation", "footer"]) {
            return Ok(());
        }

        let Some(footer) = resolve_page_footer(&ast.meta) else {
            return Ok(());
        };

        ast.meta
            .insert_path(&["navigation", "footer"], footer.to_config_value());

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use crate::render::BinaryDependencies;
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

    fn bool_value(b: bool) -> ConfigValue {
        ConfigValue::new_bool(b, SourceInfo::default())
    }

    fn str_value(s: &str) -> ConfigValue {
        ConfigValue::new_string(s, SourceInfo::default())
    }

    async fn run_transform(meta: ConfigValue) -> ConfigValue {
        let mut ast = Pandoc {
            meta,
            blocks: vec![],
        };
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        FooterGenerateTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        ast.meta
    }

    #[tokio::test]
    async fn skips_when_absent() {
        let meta = run_transform(ConfigValue::default()).await;
        assert!(!meta.contains_path(&["navigation", "footer"]));
    }

    #[tokio::test]
    async fn skips_when_false() {
        let meta = run_transform(config_map(vec![("page-footer", bool_value(false))])).await;
        assert!(!meta.contains_path(&["navigation", "footer"]));
    }

    #[tokio::test]
    async fn string_shortcut_populates_center() {
        let meta = run_transform(config_map(vec![(
            "page-footer",
            str_value("Copyright 2026"),
        )]))
        .await;
        assert!(meta.contains_path(&["navigation", "footer"]));
        let footer = meta.get_path(&["navigation", "footer"]).unwrap();
        assert_eq!(
            footer
                .get("center")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("Copyright 2026")
        );
    }

    #[tokio::test]
    async fn object_form_populates_regions() {
        let footer_cv = config_map(vec![
            ("left", str_value("© 2026")),
            ("background", str_value("light")),
        ]);
        let meta = run_transform(config_map(vec![("page-footer", footer_cv)])).await;
        let stored = meta.get_path(&["navigation", "footer"]).unwrap();
        assert_eq!(
            stored
                .get("left")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("© 2026")
        );
        assert_eq!(
            stored
                .get("background")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("light")
        );
    }

    #[tokio::test]
    async fn skips_when_navigation_footer_already_set() {
        let pre = config_map(vec![("left", str_value("Pre-existing"))]);
        let mut meta = config_map(vec![(
            "page-footer",
            config_map(vec![("left", str_value("Fresh"))]),
        )]);
        meta.insert_path(&["navigation", "footer"], pre);
        let out = run_transform(meta).await;
        let stored = out.get_path(&["navigation", "footer"]).unwrap();
        assert_eq!(
            stored
                .get("left")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("Pre-existing")
        );
    }
}
