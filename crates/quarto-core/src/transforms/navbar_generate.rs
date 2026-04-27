/*
 * navbar_generate.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Navbar resolution transform.
//!
//! Reads the raw `navbar:` YAML from merged metadata, hands it to
//! [`quarto_navigation::resolve_navbar`], and stores the resolved structure at
//! `navigation.navbar`. Follows the same Generate-then-Render split as the
//! TOC transforms — see [`TocGenerateTransform`](super::TocGenerateTransform)
//! for the pattern.
//!
//! ## Skip conditions
//!
//! - `navbar: false` (affirmative disable) — handled by the shared
//!   `is_feature_disabled` guard.
//! - `navbar` absent or `navbar: true` — `resolve_navbar` returns `None`.
//! - `navigation.navbar` already populated — treat as user override and leave
//!   it alone.

use quarto_navigation::resolve_navbar;
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::AstTransform;
use crate::transforms::is_feature_disabled;

/// Transform that resolves the user's `navbar:` config and stores it at
/// `navigation.navbar`.
pub struct NavbarGenerateTransform;

impl NavbarGenerateTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NavbarGenerateTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for NavbarGenerateTransform {
    fn name(&self) -> &str {
        "navbar-generate"
    }

    async fn transform(&self, ast: &mut Pandoc, _ctx: &mut RenderContext) -> Result<()> {
        if is_feature_disabled(&ast.meta, "navbar") {
            return Ok(());
        }

        if ast.meta.contains_path(&["navigation", "navbar"]) {
            return Ok(());
        }

        let Some(navbar) = resolve_navbar(&ast.meta) else {
            return Ok(());
        };

        ast.meta
            .insert_path(&["navigation", "navbar"], navbar.to_config_value());

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

    fn array_value(items: Vec<ConfigValue>) -> ConfigValue {
        ConfigValue::new_array(items, SourceInfo::default())
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
        NavbarGenerateTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        ast.meta
    }

    #[tokio::test]
    async fn skips_when_absent() {
        let meta = run_transform(ConfigValue::default()).await;
        assert!(!meta.contains_path(&["navigation", "navbar"]));
    }

    #[tokio::test]
    async fn skips_when_false() {
        let meta = run_transform(config_map(vec![("navbar", bool_value(false))])).await;
        assert!(!meta.contains_path(&["navigation", "navbar"]));
    }

    #[tokio::test]
    async fn skips_when_bare_true() {
        // `navbar: true` alone carries no content; resolve_navbar returns None.
        let meta = run_transform(config_map(vec![("navbar", bool_value(true))])).await;
        assert!(!meta.contains_path(&["navigation", "navbar"]));
    }

    #[tokio::test]
    async fn skips_when_navigation_navbar_already_set() {
        // Pre-populate navigation.navbar to simulate a user or prior filter
        // providing an override. The transform must not overwrite it.
        let pre = config_map(vec![("title", str_value("Pre-existing"))]);
        let mut meta = config_map(vec![(
            "navbar",
            config_map(vec![("title", str_value("Fresh"))]),
        )]);
        meta.insert_path(&["navigation", "navbar"], pre.clone());
        let out = run_transform(meta).await;
        let stored = out.get_path(&["navigation", "navbar"]).unwrap();
        assert_eq!(
            stored
                .get("title")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("Pre-existing")
        );
    }

    #[tokio::test]
    async fn populates_navigation_navbar_from_full_config() {
        let navbar_cv = config_map(vec![
            ("title", str_value("My Site")),
            ("background", str_value("primary")),
            (
                "left",
                array_value(vec![str_value("index.qmd"), str_value("about.qmd")]),
            ),
        ]);
        let meta = run_transform(config_map(vec![("navbar", navbar_cv)])).await;
        assert!(meta.contains_path(&["navigation", "navbar"]));
        let stored = meta.get_path(&["navigation", "navbar"]).unwrap();
        assert_eq!(
            stored
                .get("background")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("primary")
        );
        let left = stored.get("left").and_then(|v| v.as_array()).unwrap();
        assert_eq!(left.len(), 2);
    }
}
