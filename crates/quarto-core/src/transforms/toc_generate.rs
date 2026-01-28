/*
 * toc_generate.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Transform that generates TOC from document headings.
 */

//! TOC generation transform for Quarto documents.
//!
//! This transform generates a Table of Contents from document headings and
//! stores it in the document metadata at `navigation.toc`. The transform:
//!
//! - Checks if `toc: true` or `toc: auto` is set in format metadata
//! - Skips if `navigation.toc` already exists (user-provided or from earlier filter)
//! - Delegates to `pampa::toc::generate_toc` for the actual TOC extraction
//! - Stores the result in document metadata for later rendering
//!
//! ## Configuration
//!
//! - `toc`: `true` (boolean) or `auto` (string) to enable auto-generation
//! - `toc-depth`: Maximum heading depth to include (1-6, default: 3)
//! - `toc-title`: Title for the TOC (optional)
//!
//! ## Metadata Output
//!
//! The transform stores TOC data at `navigation.toc`:
//!
//! ```yaml
//! navigation:
//!   toc:
//!     title: "Contents"
//!     entries:
//!       - id: "introduction"
//!         title: "Introduction"
//!         level: 1
//!         children: [...]
//! ```

use pampa::toc::{TocConfig, generate_toc};
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::AstTransform;

/// Transform that generates TOC from document headings.
///
/// This transform is triggered when `toc: true` or `toc: auto` is set in
/// the format metadata. It generates a hierarchical TOC structure from
/// the document's headers and stores it in the metadata.
///
/// ## User Override Points
///
/// Users can bypass auto-generation by providing their own `navigation.toc`
/// in the document metadata. The transform detects this and skips generation.
pub struct TocGenerateTransform;

impl TocGenerateTransform {
    /// Create a new TOC generation transform.
    pub fn new() -> Self {
        Self
    }
}

impl Default for TocGenerateTransform {
    fn default() -> Self {
        Self::new()
    }
}

impl AstTransform for TocGenerateTransform {
    fn name(&self) -> &str {
        "toc-generate"
    }

    fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        // Check if TOC auto-generation is requested
        let should_generate = match ctx.format_metadata("toc") {
            Some(v) if v.as_bool() == Some(true) => true,
            Some(v) if v.as_str() == Some("auto") => true,
            _ => false,
        };

        if !should_generate {
            return Ok(());
        }

        // Check if navigation.toc already exists (user-provided or from earlier filter)
        if ast.meta.contains_path(&["navigation", "toc"]) {
            // TODO: emit warning via appropriate mechanism
            // "navigation.toc already exists in metadata, skipping auto-generation."
            return Ok(());
        }

        // Read configuration from format metadata
        let depth = ctx
            .format_metadata("toc-depth")
            .and_then(|v| v.as_i64())
            .unwrap_or(3) as i32;

        // Default title is "Table of Contents" if not specified
        let title = ctx
            .format_metadata("toc-title")
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| Some("Table of Contents".to_string()));

        let config = TocConfig { depth, title };

        // Generate TOC from document blocks
        let toc = generate_toc(&ast.blocks, &config);

        // Skip if no entries were generated
        if toc.entries.is_empty() {
            return Ok(());
        }

        // Store TOC data at navigation.toc
        ast.meta
            .insert_path(&["navigation", "toc"], toc.to_config_value());

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectContext};
    use crate::render::BinaryDependencies;
    use quarto_pandoc_types::block::{Block, Header, Paragraph};
    use quarto_pandoc_types::inline::{Inline, Str};
    use quarto_source_map::SourceInfo;
    use std::path::PathBuf;

    fn dummy_source_info() -> SourceInfo {
        SourceInfo::default()
    }

    fn make_test_project() -> ProjectContext {
        ProjectContext {
            dir: PathBuf::from("/project"),
            config: None,
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/doc.qmd")],
            output_dir: PathBuf::from("/project"),
        }
    }

    fn make_header(level: usize, id: &str, text: &str) -> Block {
        Block::Header(Header {
            level,
            attr: (id.to_string(), vec![], hashlink::LinkedHashMap::new()),
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
            attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
        })
    }

    fn make_para(text: &str) -> Block {
        Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })
    }

    #[test]
    fn test_transform_name() {
        let transform = TocGenerateTransform::new();
        assert_eq!(transform.name(), "toc-generate");
    }

    #[test]
    fn test_skips_when_toc_not_enabled() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![
                make_header(2, "intro", "Introduction"),
                make_para("Content."),
            ],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        // No toc setting in format metadata
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = TocGenerateTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        // Should not have navigation.toc
        assert!(!ast.meta.contains_path(&["navigation", "toc"]));
    }

    #[test]
    fn test_generates_toc_when_enabled() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![
                make_header(2, "intro", "Introduction"),
                make_para("Content."),
                make_header(2, "methods", "Methods"),
                make_para("More content."),
            ],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html().with_metadata(serde_json::json!({
            "toc": true
        }));
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = TocGenerateTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        // Should have navigation.toc
        assert!(ast.meta.contains_path(&["navigation", "toc"]));

        // Check entries exist
        let toc = ast.meta.get_path(&["navigation", "toc"]).unwrap();
        let entries = toc.get("entries").unwrap().as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].get("id").unwrap().as_str(), Some("intro"));
        assert_eq!(entries[1].get("id").unwrap().as_str(), Some("methods"));
    }

    #[test]
    fn test_generates_toc_with_string_auto() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![
                make_header(2, "intro", "Introduction"),
                make_para("Content."),
            ],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        // toc: "auto" (string)
        let format = Format::html().with_metadata(serde_json::json!({
            "toc": "auto"
        }));
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = TocGenerateTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        // Should have navigation.toc
        assert!(ast.meta.contains_path(&["navigation", "toc"]));
    }

    #[test]
    fn test_skips_when_navigation_toc_exists() {
        use quarto_pandoc_types::config_value::ConfigValue;

        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![
                make_header(2, "intro", "Introduction"),
                make_para("Content."),
            ],
        };

        // Pre-populate navigation.toc with user-provided data
        ast.meta.insert_path(
            &["navigation", "toc"],
            ConfigValue::new_string("user-provided", SourceInfo::default()),
        );

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html().with_metadata(serde_json::json!({
            "toc": true
        }));
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = TocGenerateTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        // Should keep user-provided toc
        let toc = ast.meta.get_path(&["navigation", "toc"]).unwrap();
        assert_eq!(toc.as_str(), Some("user-provided"));
    }

    #[test]
    fn test_respects_toc_depth() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![
                make_header(1, "h1", "Level 1"),
                make_header(2, "h2", "Level 2"),
                make_header(3, "h3", "Level 3"),
                make_header(4, "h4", "Level 4"),
            ],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        // toc-depth: 2 should only include h1 and h2
        let format = Format::html().with_metadata(serde_json::json!({
            "toc": true,
            "toc-depth": 2
        }));
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = TocGenerateTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        let toc = ast.meta.get_path(&["navigation", "toc"]).unwrap();
        let entries = toc.get("entries").unwrap().as_array().unwrap();

        // Only h1 at top level
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].get("id").unwrap().as_str(), Some("h1"));

        // h2 should be a child
        let children = entries[0].get("children").unwrap().as_array().unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].get("id").unwrap().as_str(), Some("h2"));

        // h2's children should be empty (h3/h4 excluded by depth limit)
        assert!(
            children[0].get("children").is_none() || {
                children[0]
                    .get("children")
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .is_empty()
            }
        );
    }

    #[test]
    fn test_respects_toc_title() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![
                make_header(2, "intro", "Introduction"),
                make_para("Content."),
            ],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html().with_metadata(serde_json::json!({
            "toc": true,
            "toc-title": "Contents"
        }));
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = TocGenerateTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        let toc = ast.meta.get_path(&["navigation", "toc"]).unwrap();
        assert_eq!(toc.get("title").unwrap().as_str(), Some("Contents"));
    }

    #[test]
    fn test_skips_when_no_headings() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![make_para("Just a paragraph."), make_para("Another one.")],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html().with_metadata(serde_json::json!({
            "toc": true
        }));
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = TocGenerateTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        // No TOC should be generated if there are no headings
        assert!(!ast.meta.contains_path(&["navigation", "toc"]));
    }

    #[test]
    fn test_default_trait() {
        let _transform: TocGenerateTransform = Default::default();
    }

    #[test]
    fn test_default_toc_title() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![
                make_header(2, "intro", "Introduction"),
                make_para("Content."),
            ],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        // No toc-title specified - should get default
        let format = Format::html().with_metadata(serde_json::json!({
            "toc": true
        }));
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = TocGenerateTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        let toc = ast.meta.get_path(&["navigation", "toc"]).unwrap();
        assert_eq!(
            toc.get("title").unwrap().as_str(),
            Some("Table of Contents")
        );
    }
}
