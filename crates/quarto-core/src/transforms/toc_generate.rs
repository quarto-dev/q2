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

use pampa::toc::{TocConfig, config_value_to_inlines, generate_toc, plain_inlines};
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::format::FormatIdentifier;
use crate::project::ProjectKind;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};
use crate::transforms::is_feature_disabled;

/// The language-catalog term holding the TOC title for this render.
///
/// Quarto 1 keys the title off render context
/// (`src/command/render/pandoc.ts:493`):
///
/// ```text
/// projectIsWebsite(project) && !projectIsBook(project)
///     && isHtmlOutput(format.pandoc, /* strict */ true)
///     ? "toc-title-website"      // "On this page"
///     : "toc-title-document"     // "Table of contents"
/// ```
///
/// Two of Q1's three conditions carry over; the third is free:
///
/// - **Website.** Read from [`ProjectKind`], not from a `website:` key in
///   merged metadata. A website project need not define `website:` at
///   all (`project: {type: website}` alone is valid and Q1 still calls it
///   a website), and a stray `website:` key in a standalone document is
///   not a claim about project type. `project_kind` also already holds
///   the *base* kind for custom project types (`resolve_project_type`
///   rewrites them), so extension-defined website types work unchanged.
/// - **Not a book.** Free here: Q1's `projectIsWebsite` is true for books
///   (book extends website), which is why it needs `!projectIsBook`.
///   [`ProjectKind::Book`] is a distinct variant, so `== Website`
///   excludes books already.
/// - **Strict HTML.** `isHtmlOutput(…, strict = true)` matches only
///   `html`/`html4`/`html5` — it **excludes revealjs and epub**. So this
///   compares [`FormatIdentifier::Html`] directly and deliberately does
///   *not* use [`Format::is_html`], which delegates to `is_html_based()`
///   and would also match [`FormatIdentifier::Revealjs`]. The format
///   check is load-bearing rather than incidental: `TocGenerateTransform`
///   is pushed into the pipeline for *every* format (see the
///   Navigation-phase comment in `pipeline.rs`), so without it a PDF
///   render of a website project would say "On this page".
fn toc_title_term(ctx: &RenderContext) -> &'static str {
    let is_website = ctx.project.project_kind() == ProjectKind::Website;
    let is_strict_html = ctx.format.identifier == FormatIdentifier::Html;

    if is_website && is_strict_html {
        "toc-title-website"
    } else {
        "toc-title-document"
    }
}

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

#[async_trait::async_trait(?Send)]
impl AstTransform for TocGenerateTransform {
    fn name(&self) -> &str {
        "toc-generate"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Navigation
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        // Affirmative disable: `toc: false` in merged metadata suppresses
        // generation. Handled explicitly so the intent is symmetric with the
        // render transform (which also short-circuits on `toc: false`) and
        // with the navbar/footer transforms that share the convention.
        if is_feature_disabled(&ast.meta, "toc") {
            return Ok(());
        }

        // Check if TOC auto-generation is requested.
        // Read from ast.meta which contains merged project + document metadata.
        // `as_plain_text` (not `as_str`): a bare `toc: auto` front-matter string
        // is stored as `ConfigValueKind::PandocInlines`, for which `as_str`
        // returns `None`, so the `== "auto"` comparison silently failed.
        // (bd-y89ihf0i)
        let should_generate = match ast.meta.get("toc") {
            Some(v) if v.as_bool() == Some(true) => true,
            Some(v) if v.as_plain_text().as_deref() == Some("auto") => true,
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

        // Read configuration from ast.meta (merged project + document metadata)
        let depth = ast
            .meta
            .get("toc-depth")
            .and_then(|v| v.as_int())
            .unwrap_or(3) as i32;

        // Title precedence (decided 2026-07-17, bd-llhlzd7p): user
        // `toc-title` metadata > localized `toc-title-document` term >
        // English literal (stage-less unit-test fallback).
        //
        // The user-supplied value is read as **inlines**, not text
        // (bd-toc-smart-quotes-6nro57ed). A front-matter `toc-title` is
        // already `ConfigValueKind::PandocInlines`, so `as_plain_text()`
        // — the previous read, itself a fix for the `as_str()` trap in
        // bd-y89ihf0i — was discarding markup the metadata layer had
        // just parsed. A `_quarto.yml` `toc-title` arrives as
        // `Scalar(String)` and gets markdown semantics from
        // `MARKDOWN_CONFIG_PATHS` upstream, so by here both sources
        // agree.
        //
        // The two fallbacks are genuinely plain text — a localized term
        // from the language catalog, and an English literal — so they
        // are wrapped in a single `Str`.
        //
        // Which localized term depends on context
        // (bd-website-toc-title-wn80ymab): website pages get
        // `toc-title-website` ("On this page"), everything else gets
        // `toc-title-document` ("Table of contents"). See
        // `toc_title_term` for the predicate and its Q1 provenance. The
        // *English literal* stays context-free by decision — it only
        // fires when no catalog is loaded at all, which is a stage-less
        // unit-test path, and giving it two spellings would just muddy
        // which string is canonical.
        let title = ast
            .meta
            .get("toc-title")
            .and_then(config_value_to_inlines)
            .or_else(|| {
                let term = toc_title_term(ctx);
                crate::language::LanguageTerms::from_meta(&ast.meta)
                    .and_then(|t| t.get(term).map(plain_inlines))
            })
            .or_else(|| Some(plain_inlines("Table of Contents")));

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
    use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use crate::render::BinaryDependencies;
    use quarto_pandoc_types::ConfigMapEntry;
    use quarto_pandoc_types::block::{Block, Header, Paragraph};
    use quarto_pandoc_types::config_value::ConfigValue;
    use quarto_pandoc_types::inline::{Inline, Str};
    use quarto_source_map::SourceInfo;
    use std::path::PathBuf;

    fn dummy_source_info() -> SourceInfo {
        SourceInfo::for_test()
    }

    /// Helper to create a ConfigValue map from key-value pairs
    fn config_map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        let map_entries: Vec<ConfigMapEntry> = entries
            .into_iter()
            .map(|(k, v)| ConfigMapEntry {
                key: k.to_string(),
                key_source: SourceInfo::for_test(),
                value: v,
            })
            .collect();
        ConfigValue::new_map(map_entries, SourceInfo::for_test())
    }

    /// Helper to create a scalar bool ConfigValue
    fn config_bool(b: bool) -> ConfigValue {
        ConfigValue::new_bool(b, SourceInfo::for_test())
    }

    /// Helper to create a scalar string ConfigValue
    fn config_str(s: &str) -> ConfigValue {
        ConfigValue::new_string(s, SourceInfo::for_test())
    }

    /// Helper to create a scalar i64 ConfigValue
    fn config_int(i: i64) -> ConfigValue {
        use yaml_rust2::Yaml;
        ConfigValue::new_scalar(Yaml::Integer(i), SourceInfo::for_test())
    }

    fn make_test_project() -> ProjectContext {
        ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/doc.qmd")],
            output_dir: PathBuf::from("/project"),

            ..Default::default()
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

    #[tokio::test]
    async fn test_transform_name() {
        let transform = TocGenerateTransform::new();
        assert_eq!(transform.name(), "toc-generate");
    }

    #[tokio::test]
    async fn test_skips_when_toc_not_enabled() {
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
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        // Should not have navigation.toc
        assert!(!ast.meta.contains_path(&["navigation", "toc"]));
    }

    #[tokio::test]
    async fn test_generates_toc_when_enabled() {
        let mut ast = Pandoc {
            meta: config_map(vec![("toc", config_bool(true))]),
            blocks: vec![
                make_header(2, "intro", "Introduction"),
                make_para("Content."),
                make_header(2, "methods", "Methods"),
                make_para("More content."),
            ],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = TocGenerateTransform::new();
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        // Should have navigation.toc
        assert!(ast.meta.contains_path(&["navigation", "toc"]));

        // Check entries exist
        let toc = ast.meta.get_path(&["navigation", "toc"]).unwrap();
        let entries = toc.get("entries").unwrap().as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].get("id").unwrap().as_str(), Some("intro"));
        assert_eq!(entries[1].get("id").unwrap().as_str(), Some("methods"));
    }

    #[tokio::test]
    async fn test_toc_false_skips_generation() {
        // `toc: false` (the post-merge winner when a document overrides a
        // project-level `toc: true`) must not generate a TOC.
        let mut ast = Pandoc {
            meta: config_map(vec![("toc", config_bool(false))]),
            blocks: vec![
                make_header(2, "intro", "Introduction"),
                make_para("Content."),
            ],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = TocGenerateTransform::new();
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        assert!(!ast.meta.contains_path(&["navigation", "toc"]));
    }

    #[tokio::test]
    async fn test_generates_toc_with_string_auto() {
        // toc: "auto" (string)
        let mut ast = Pandoc {
            meta: config_map(vec![("toc", config_str("auto"))]),
            blocks: vec![
                make_header(2, "intro", "Introduction"),
                make_para("Content."),
            ],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = TocGenerateTransform::new();
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        // Should have navigation.toc
        assert!(ast.meta.contains_path(&["navigation", "toc"]));
    }

    #[tokio::test]
    async fn test_skips_when_navigation_toc_exists() {
        let mut ast = Pandoc {
            meta: config_map(vec![("toc", config_bool(true))]),
            blocks: vec![
                make_header(2, "intro", "Introduction"),
                make_para("Content."),
            ],
        };

        // Pre-populate navigation.toc with user-provided data
        ast.meta
            .insert_path(&["navigation", "toc"], config_str("user-provided"));

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = TocGenerateTransform::new();
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        // Should keep user-provided toc
        let toc = ast.meta.get_path(&["navigation", "toc"]).unwrap();
        assert_eq!(toc.as_str(), Some("user-provided"));
    }

    #[tokio::test]
    async fn test_respects_toc_depth() {
        // toc-depth: 2 should only include h1 and h2
        let mut ast = Pandoc {
            meta: config_map(vec![
                ("toc", config_bool(true)),
                ("toc-depth", config_int(2)),
            ]),
            blocks: vec![
                make_header(1, "h1", "Level 1"),
                make_header(2, "h2", "Level 2"),
                make_header(3, "h3", "Level 3"),
                make_header(4, "h4", "Level 4"),
            ],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = TocGenerateTransform::new();
        transform.transform(&mut ast, &mut ctx).await.unwrap();

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

    #[tokio::test]
    async fn test_respects_toc_title() {
        let mut ast = Pandoc {
            meta: config_map(vec![
                ("toc", config_bool(true)),
                ("toc-title", config_str("Contents")),
            ]),
            blocks: vec![
                make_header(2, "intro", "Introduction"),
                make_para("Content."),
            ],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = TocGenerateTransform::new();
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        let toc = ast.meta.get_path(&["navigation", "toc"]).unwrap();
        // The title is `PandocInlines` now, so `as_str()` returns
        // `None` — project it instead (bd-toc-smart-quotes-6nro57ed).
        assert_eq!(
            toc.get("title").unwrap().as_plain_text().as_deref(),
            Some("Contents")
        );
    }

    #[tokio::test]
    async fn test_skips_when_no_headings() {
        let mut ast = Pandoc {
            meta: config_map(vec![("toc", config_bool(true))]),
            blocks: vec![make_para("Just a paragraph."), make_para("Another one.")],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = TocGenerateTransform::new();
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        // No TOC should be generated if there are no headings
        assert!(!ast.meta.contains_path(&["navigation", "toc"]));
    }

    #[tokio::test]
    async fn test_default_trait() {
        let _transform: TocGenerateTransform = Default::default();
    }

    #[tokio::test]
    async fn test_default_toc_title() {
        // No toc-title specified - should get default
        let mut ast = Pandoc {
            meta: config_map(vec![("toc", config_bool(true))]),
            blocks: vec![
                make_header(2, "intro", "Introduction"),
                make_para("Content."),
            ],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = TocGenerateTransform::new();
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        let toc = ast.meta.get_path(&["navigation", "toc"]).unwrap();
        assert_eq!(
            toc.get("title").unwrap().as_plain_text().as_deref(),
            Some("Table of Contents")
        );
    }

    /// A `toc-title` carrying markup keeps it: the front-matter value is
    /// already `PandocInlines`, and the transform must not flatten it
    /// (bd-toc-smart-quotes-6nro57ed).
    #[tokio::test]
    async fn test_toc_title_keeps_inline_markup() {
        use quarto_pandoc_types::inline::Strong;

        let styled = ConfigValue::new_inlines(
            vec![
                Inline::Str(Str {
                    text: "On ".to_string(),
                    source_info: dummy_source_info(),
                }),
                Inline::Strong(Strong {
                    content: vec![Inline::Str(Str {
                        text: "this".to_string(),
                        source_info: dummy_source_info(),
                    })],
                    source_info: dummy_source_info(),
                }),
            ],
            dummy_source_info(),
        );

        let mut ast = Pandoc {
            meta: config_map(vec![("toc", config_bool(true)), ("toc-title", styled)]),
            blocks: vec![
                make_header(2, "intro", "Introduction"),
                make_para("Content."),
            ],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        TocGenerateTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();

        let title = ast
            .meta
            .get_path(&["navigation", "toc", "title"])
            .expect("toc title");
        let inlines = pampa::toc::config_value_to_inlines(title).expect("inlines");
        assert!(
            inlines.iter().any(|i| matches!(i, Inline::Strong(_))),
            "the Strong node must survive; got {inlines:?}"
        );
    }

    // -----------------------------------------------------------------
    // bd-website-toc-title-wn80ymab: the TOC title term is context-keyed.
    //
    // Quarto 1's language catalog carries two keys and picks between them
    // (`src/command/render/pandoc.ts:493`):
    //
    //     projectIsWebsite(project) && !projectIsBook(project)
    //         && isHtmlOutput(format.pandoc, /* strict */ true)
    //         ? "toc-title-website"      // "On this page"
    //         : "toc-title-document"     // "Table of contents"
    //
    // q2 needs no `!projectIsBook` equivalent: Q1's `projectIsWebsite` is
    // true for books (book extends website), whereas `ProjectKind::Book`
    // is a distinct variant here, so `== Website` excludes it already.
    //
    // `strict` matters: it restricts the website title to `html`/`html4`/
    // `html5` and **excludes revealjs**. `Format::is_html()` delegates to
    // `is_html_based()`, which *includes* `Revealjs` — so it is the wrong
    // predicate. These tests pin the identifier check instead.
    // -----------------------------------------------------------------

    /// The two catalog terms, as `LanguageResolveStage` injects them at
    /// `quarto.language` (see `LanguageTerms::from_meta`).
    fn config_language_terms(document: &str, website: &str) -> ConfigValue {
        config_map(vec![(
            "language",
            config_map(vec![
                ("toc-title-document", config_str(document)),
                ("toc-title-website", config_str(website)),
            ]),
        )])
    }

    /// `toc: true` plus the English catalog terms — the shape a real
    /// render reaches this transform with.
    fn meta_with_terms() -> ConfigValue {
        config_map(vec![
            ("toc", config_bool(true)),
            (
                "quarto",
                config_language_terms("Table of contents", "On this page"),
            ),
        ])
    }

    fn make_project_of_kind(kind: crate::project::ProjectKind) -> ProjectContext {
        let mut project = make_test_project();
        project.config.project_kind = kind;
        project.is_single_file = false;
        project
    }

    fn two_section_blocks() -> Vec<Block> {
        vec![
            make_header(2, "intro", "Introduction"),
            make_para("Content."),
            make_header(2, "methods", "Methods"),
            make_para("More content."),
        ]
    }

    /// Flatten the generated `navigation.toc.title` to plain text.
    ///
    /// Deliberately local rather than reaching for a shared helper: the
    /// title is stored as inlines (bd-toc-smart-quotes-6nro57ed), and
    /// these tests only ever assert on plain-text terms.
    fn toc_title_text(ast: &Pandoc) -> String {
        let title = ast
            .meta
            .get_path(&["navigation", "toc", "title"])
            .expect("navigation.toc.title");
        let inlines = pampa::toc::config_value_to_inlines(title).expect("title inlines");
        inlines
            .iter()
            .map(|i| match i {
                Inline::Str(s) => s.text.clone(),
                Inline::Space(_) => " ".to_string(),
                other => panic!("unexpected inline in a plain-text title: {other:?}"),
            })
            .collect()
    }

    /// Run the transform over `blocks` with the given project + format.
    async fn title_for(meta: ConfigValue, project: &ProjectContext, format: &Format) -> String {
        let mut ast = Pandoc {
            meta,
            blocks: two_section_blocks(),
        };
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(project, &doc, format, &binaries);

        TocGenerateTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();

        toc_title_text(&ast)
    }

    /// The headline case: a website page renders "On this page".
    #[tokio::test]
    async fn website_html_uses_toc_title_website() {
        let project = make_project_of_kind(crate::project::ProjectKind::Website);
        let title = title_for(meta_with_terms(), &project, &Format::html()).await;
        assert_eq!(
            title, "On this page",
            "a website page must use the `toc-title-website` term"
        );
    }

    /// Regression guard for the standalone-document case, which is what
    /// the transform did unconditionally before this change.
    #[tokio::test]
    async fn default_project_uses_toc_title_document() {
        let project = make_project_of_kind(crate::project::ProjectKind::Default);
        let title = title_for(meta_with_terms(), &project, &Format::html()).await;
        assert_eq!(
            title, "Table of contents",
            "a standalone document must keep the `toc-title-document` term"
        );
    }

    /// A user-supplied `toc-title` keeps top precedence on a website —
    /// the context split must not disturb the chain decided in
    /// bd-llhlzd7p.
    #[tokio::test]
    async fn user_toc_title_still_wins_on_a_website() {
        let project = make_project_of_kind(crate::project::ProjectKind::Website);
        let meta = config_map(vec![
            ("toc", config_bool(true)),
            ("toc-title", config_str("My Contents")),
            (
                "quarto",
                config_language_terms("Table of contents", "On this page"),
            ),
        ]);
        let title = title_for(meta, &project, &Format::html()).await;
        assert_eq!(
            title, "My Contents",
            "an explicit `toc-title` must outrank the localized website term"
        );
    }

    /// Localization still flows through the website branch: the term is
    /// read from the catalog, not hardcoded per-context.
    #[tokio::test]
    async fn website_uses_the_localized_website_term() {
        let project = make_project_of_kind(crate::project::ProjectKind::Website);
        let meta = config_map(vec![
            ("toc", config_bool(true)),
            ("lang", config_str("pt")),
            ("quarto", config_language_terms("Índice", "Nesta página")),
        ]);
        let title = title_for(meta, &project, &Format::html()).await;
        assert_eq!(
            title, "Nesta página",
            "the website term must come from the language catalog"
        );
    }

    /// Q1 gates on `isHtmlOutput(…, strict = true)`, which excludes
    /// revealjs. Pins decision 2 against a later drift to
    /// `Format::is_html()` (which would match `Revealjs` and break this).
    #[tokio::test]
    async fn website_revealjs_uses_toc_title_document() {
        let project = make_project_of_kind(crate::project::ProjectKind::Website);
        let revealjs = Format::from_format_string("revealjs").expect("revealjs format");
        let title = title_for(meta_with_terms(), &project, &revealjs).await;
        assert_eq!(
            title, "Table of contents",
            "revealjs is excluded by Q1's strict isHtmlOutput, so it keeps the document term"
        );
    }

    /// A non-HTML render of a website project keeps the document term —
    /// the transform runs for every format (see the Navigation-phase
    /// comment in `pipeline.rs`), so the gate has to be explicit.
    #[tokio::test]
    async fn website_pdf_uses_toc_title_document() {
        let project = make_project_of_kind(crate::project::ProjectKind::Website);
        let title = title_for(meta_with_terms(), &project, &Format::pdf()).await;
        assert_eq!(
            title, "Table of contents",
            "a PDF render of a website project must not say \"On this page\""
        );
    }

    /// Q1 excludes books explicitly (`!projectIsBook`); q2 gets it free
    /// from `ProjectKind::Book` being a distinct variant. Pinned so a
    /// future `is_website_like()` helper cannot silently absorb books.
    #[tokio::test]
    async fn book_project_uses_toc_title_document() {
        let project = make_project_of_kind(crate::project::ProjectKind::Book);
        let title = title_for(meta_with_terms(), &project, &Format::html()).await;
        assert_eq!(
            title, "Table of contents",
            "a book must keep the document term, matching Q1's !projectIsBook guard"
        );
    }

    /// The English literal fallback stays context-free (decision 3): it
    /// only fires when no catalog is loaded, which is a stage-less
    /// unit-test path.
    #[tokio::test]
    async fn website_without_a_catalog_falls_back_to_the_english_literal() {
        let project = make_project_of_kind(crate::project::ProjectKind::Website);
        let meta = config_map(vec![("toc", config_bool(true))]);
        let title = title_for(meta, &project, &Format::html()).await;
        assert_eq!(
            title, "Table of Contents",
            "with no catalog the transform keeps its English literal, uncontextualized"
        );
    }
}
