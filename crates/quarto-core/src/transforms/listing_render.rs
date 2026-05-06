/*
 * listing_render.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Pass-2 listing-render transform.
//!
//! Reads `RenderContext::resolved_listings` (populated by
//! [`super::listing_generate::ListingGenerateTransform`]), builds
//! the per-listing [`TemplateContext`] binding, applies the chosen
//! built-in doctemplate, re-parses the output via
//! [`pampa::readers::qmd::read`], and splices the resulting blocks
//! into the host page's AST.
//!
//! Slot rules (per L3 sub-plan §"Render transform" step 7):
//!
//! - **Explicit slot.** If the host AST already contains a
//!   top-level `Div` with `id == "<listing.id>"`, the transform
//!   replaces the Div's contents with the rendered blocks and
//!   marks the Div with the `data-listing-rendered="1"`
//!   attribute so re-runs are idempotent.
//! - **Implicit slot.** Otherwise the transform appends a fresh
//!   `Div` (with that id and class `quarto-listing`) to the end
//!   of `ast.blocks`.
//!
//! Re-parse diagnostics: the doctemplate output is parsed into
//! Pandoc blocks via `pampa::readers::qmd::read`; the fresh
//! `SourceContext` is discarded and any diagnostics from the
//! re-parse are collapsed into a single `Q-12-10` warning on the
//! host page. Full source-info threading is tracked by `bd-0jyl`.
//!
//! TODO(bd-0fd0): Today the resolved listings travel from generate
//! to render via the typed `RenderContext::resolved_listings`
//! field. When a Lua filter slot lands between generate and render,
//! we'll add a meta serialize/deserialize bridge at that boundary.

use std::path::Path;

use hashlink::LinkedHashMap;
use quarto_doctemplate::Template;
use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::attr::AttrSourceInfo;
use quarto_pandoc_types::block::{Block, Div};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceInfo;

use crate::Result;
use crate::project::listing::ResolvedListing;
use crate::project::listing::binding::build_listing_context;
use crate::project::listing::config::ListingType;
use crate::project::listing::templates::{builtins_resolver, top_level_template_source};
use crate::render::RenderContext;
use crate::transform::AstTransform;
use crate::transforms::is_feature_disabled;

pub struct ListingRenderTransform;

impl ListingRenderTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ListingRenderTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for ListingRenderTransform {
    fn name(&self) -> &str {
        "listing-render"
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        if is_feature_disabled(&ast.meta, "listing") {
            return Ok(());
        }

        if ctx.resolved_listings.is_empty() {
            return Ok(());
        }

        // Take ownership so the borrow checker lets us mutate
        // `ctx.diagnostics` while iterating over the resolved set.
        let resolved = std::mem::take(&mut ctx.resolved_listings);
        let mut diags = std::mem::take(&mut ctx.diagnostics);

        for r in &resolved {
            render_one(ast, r, &mut diags);
        }

        // Register the vendored client-side JS artifacts so the
        // sort/filter UI markup our templates emit is functional.
        // The `js:` key prefix is the convention `ApplyTemplateStage`
        // recognizes for auto-emitting `<script>` tags into the
        // rendered HTML; the resolver maps the relative path to
        // `_site/site_libs/listing/<file>.js` (or the WASM VFS
        // equivalent). `ArtifactStore::store` overwrites by key, so
        // re-running across files is fine.
        register_listing_js_artifacts(&mut ctx.artifacts);

        // Restore; downstream stages can still read resolved
        // listings if they want (e.g. L5 categories sidebar).
        ctx.resolved_listings = resolved;
        ctx.diagnostics = diags;
        Ok(())
    }
}

/// Bytes for the vendored `list.min.js` (third-party MIT) and
/// `quarto-listing.js` (Q1-owned glue) — copied locally per
/// CLAUDE.md §"External Sources Policy". The SCSS is *not* shipped
/// here; per L3 D5 the SCSS needs proper SassLayer integration with
/// the existing theme-CSS pipeline, which is filed as a follow-up
/// rather than wired in this commit.
const LIST_MIN_JS: &[u8] = include_bytes!("../../../../resources/listing/list.min.js");
const QUARTO_LISTING_JS: &[u8] = include_bytes!("../../../../resources/listing/quarto-listing.js");

const LIST_MIN_REL_PATH: &str = "listing/list.min.js";
const QUARTO_LISTING_REL_PATH: &str = "listing/quarto-listing.js";

fn register_listing_js_artifacts(artifacts: &mut crate::artifact::ArtifactStore) {
    use crate::artifact::{Artifact, ArtifactScope};
    artifacts.store(
        "js:listing:list.min.js",
        Artifact::from_bytes(LIST_MIN_JS.to_vec(), "application/javascript")
            .with_path(LIST_MIN_REL_PATH)
            .with_scope(ArtifactScope::Project),
    );
    artifacts.store(
        "js:listing:quarto-listing.js",
        Artifact::from_bytes(QUARTO_LISTING_JS.to_vec(), "application/javascript")
            .with_path(QUARTO_LISTING_REL_PATH)
            .with_scope(ArtifactScope::Project),
    );
}

fn render_one(ast: &mut Pandoc, r: &ResolvedListing, diags: &mut Vec<DiagnosticMessage>) {
    // L8 deferral: custom templates fall back to default with a
    // Q-12-1 diagnostic. The fallback was already done in the
    // generate transform's listing config (we receive a Default
    // listing here), but we keep the diagnostic emission close to
    // the user-visible site so it shows up reliably.
    let kind = if r.listing.kind == ListingType::Custom {
        push_diag(
            diags,
            "Q-12-1",
            "Custom listing templates land in a follow-up (bd-rqgx). \
             For now, this listing falls back to the `default` built-in. \
             Set `type: default | grid | table` to silence this diagnostic.",
        );
        ListingType::Default
    } else {
        r.listing.kind
    };

    // Build the binding. The host page's meta is used to extract
    // project.* values (site-url, title) for the templates.
    let template_ctx = build_listing_context(&r.listing, &r.items, &ast.meta);

    // Compile + render the top-level template.
    let template_source = top_level_template_source(kind);
    let resolver = builtins_resolver();
    // The synthetic template path needs an extension so the
    // FileSystemResolver-style partial-name fallback works
    // consistently; we use `.template` to mirror the embedded
    // file extension.
    let template_path = Path::new("listing.template");
    let template =
        match Template::compile_with_resolver(template_source, template_path, &resolver, 0) {
            Ok(t) => t,
            Err(e) => {
                push_diag(
                    diags,
                    "Q-12-10",
                    format!(
                        "Listing `{}` template failed to compile: {:?}. \
                     Listing skipped.",
                        r.listing.id, e
                    ),
                );
                return;
            }
        };
    let (rendered, render_diags) = template.render_with_diagnostics(&template_ctx);
    let markdown = match rendered {
        Ok(s) => s,
        Err(()) => {
            push_diag(
                diags,
                "Q-12-10",
                format!(
                    "Listing `{}` template rendering failed; listing skipped.",
                    r.listing.id
                ),
            );
            return;
        }
    };
    if !render_diags.is_empty() {
        push_diag(
            diags,
            "Q-12-10",
            format!(
                "Listing `{}` doctemplate produced {} diagnostic(s); first: {}",
                r.listing.id,
                render_diags.len(),
                render_diags[0].title
            ),
        );
    }

    // Re-parse the markdown. Discard the fresh SourceContext
    // (bd-0jyl tracks proper threading); collect any parse
    // diagnostics into a single host-page warning.
    let mut sink = std::io::sink();
    let parse_result = pampa::readers::qmd::read(
        markdown.as_bytes(),
        false,
        &format!("listing:{}", r.listing.id),
        &mut sink,
        true,
        None,
    );
    let parsed_blocks: Vec<Block> = match parse_result {
        Ok((parsed, _ctx, parse_diags)) => {
            if !parse_diags.is_empty() {
                push_diag(
                    diags,
                    "Q-12-10",
                    format!(
                        "Re-parsing rendered listing `{}` produced {} diagnostic(s); first: {}",
                        r.listing.id,
                        parse_diags.len(),
                        parse_diags[0].title
                    ),
                );
            }
            parsed.blocks
        }
        Err(parse_diags) => {
            push_diag(
                diags,
                "Q-12-10",
                format!(
                    "Re-parsing rendered listing `{}` failed with {} diagnostic(s); first: {}. \
                     Listing skipped.",
                    r.listing.id,
                    parse_diags.len(),
                    parse_diags
                        .first()
                        .map(|d| d.title.as_str())
                        .unwrap_or("(no message)")
                ),
            );
            return;
        }
    };

    // Splice into the AST.
    if !try_replace_explicit_slot(ast, &r.listing.id, &parsed_blocks) {
        // No explicit slot — append a fresh wrapper Div.
        let mut attrs = LinkedHashMap::new();
        attrs.insert("data-listing-rendered".to_string(), "1".to_string());
        ast.blocks.push(Block::Div(Div {
            attr: (
                r.listing.id.clone(),
                vec!["quarto-listing".to_string()],
                attrs,
            ),
            content: parsed_blocks,
            source_info: SourceInfo::default(),
            attr_source: AttrSourceInfo::empty(),
        }));
    }
}

/// Walk the host AST looking for a Div with the given id.
/// Replaces its content + marks it with `data-listing-rendered="1"`.
/// Returns `true` when a slot was found and updated.
///
/// Recursion is needed because the SectionizeTransform (which runs
/// in the Normalization phase, ahead of Navigation) wraps top-level
/// headings in `Div .section` containers, so a user's
/// `::: {#my-blog}` slot inside a section is no longer a top-level
/// block by the time the listing renders. Q1 recurses too. Already-
/// rendered slots short-circuit so the recursion is idempotent.
fn try_replace_explicit_slot(ast: &mut Pandoc, id: &str, blocks: &[Block]) -> bool {
    fill_in_blocks(&mut ast.blocks, id, blocks)
}

fn fill_in_blocks(blocks_in: &mut Vec<Block>, id: &str, payload: &[Block]) -> bool {
    for block in blocks_in.iter_mut() {
        if let Block::Div(div) = block {
            // `Attr` is the tuple `(id, classes, attributes)`.
            if div.attr.0 == id {
                // Idempotency: if we already populated this slot,
                // skip the second pass.
                let already_rendered =
                    div.attr.2.get("data-listing-rendered").map(String::as_str) == Some("1");
                if already_rendered {
                    return true;
                }
                div.content = payload.to_vec();
                div.attr
                    .2
                    .insert("data-listing-rendered".to_string(), "1".to_string());
                return true;
            }
            // Recurse into the Div's content. Handles nested
            // sections (from SectionizeTransform) as well as nested
            // user Divs.
            if fill_in_blocks(&mut div.content, id, payload) {
                return true;
            }
        }
    }
    false
}

fn push_diag(diags: &mut Vec<DiagnosticMessage>, code: &str, message: impl Into<String>) {
    diags.push(
        DiagnosticMessageBuilder::warning(message)
            .with_code(code)
            .build(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_profile::DocumentProfile;
    use crate::format::Format;
    use crate::project::index::ProjectIndex;
    use crate::project::listing::config::apply_type_defaults;
    use crate::project::listing::config::{Listing, ListingType};
    use crate::project::listing::item::ListingItem;
    use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use crate::render::BinaryDependencies;
    use quarto_pandoc_types::ConfigValue;
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn make_project() -> ProjectContext {
        ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: false,
            files: vec![DocumentInfo::from_path("/project/posts/index.qmd")],
            output_dir: PathBuf::from("/project/_site"),
        }
    }

    fn make_item(title: &str, date: Option<&str>) -> ListingItem {
        ListingItem {
            title: title.to_string(),
            subtitle: None,
            description: Some(format!("{} description", title)),
            author: Some("Jane".to_string()),
            authors: vec!["Jane".to_string()],
            date: date.map(String::from),
            date_modified: None,
            categories: vec![],
            image: None,
            image_alt: None,
            image_lazy_loading: None,
            reading_time_minutes: Some(5),
            word_count: None,
            source_path: PathBuf::from(format!("posts/{}.qmd", title)),
            output_href: format!("posts/{}.html", title),
            extra: BTreeMap::new(),
        }
    }

    fn make_listing(kind: ListingType) -> Listing {
        let mut l = Listing {
            id: "main-listing".to_string(),
            kind,
            ..Listing::default()
        };
        apply_type_defaults(&mut l);
        l
    }

    async fn run_transform(
        ast: Pandoc,
        resolved: Vec<ResolvedListing>,
    ) -> (Pandoc, Vec<DiagnosticMessage>) {
        let mut ast = ast;
        let project = make_project();
        let doc = DocumentInfo::from_path("/project/posts/index.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let index = Arc::new(ProjectIndex::new(Vec::<DocumentProfile>::new()));
        let mut ctx =
            RenderContext::new(&project, &doc, &format, &binaries).with_project_index(index);
        ctx.resolved_listings = resolved;
        ListingRenderTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        (ast, ctx.diagnostics)
    }

    fn empty_pandoc() -> Pandoc {
        Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![],
        }
    }

    fn pandoc_with_slot(id: &str) -> Pandoc {
        Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![Block::Div(Div {
                attr: (id.to_string(), vec![], LinkedHashMap::new()),
                content: vec![],
                source_info: SourceInfo::default(),
                attr_source: AttrSourceInfo::empty(),
            })],
        }
    }

    fn rendered_block_titles(div: &Div) -> Vec<String> {
        // Walk the Div content and extract any titles we can
        // identify (Header text). Used as a smoke check that the
        // listing markup got spliced in.
        let mut out = Vec::new();
        collect_header_text(&div.content, &mut out);
        out
    }

    fn collect_header_text(blocks: &[Block], out: &mut Vec<String>) {
        for b in blocks {
            match b {
                Block::Header(h) => {
                    let mut s = String::new();
                    inlines_to_text(&h.content, &mut s);
                    out.push(s);
                }
                Block::Div(d) => collect_header_text(&d.content, out),
                _ => {}
            }
        }
    }

    fn inlines_to_text(inlines: &[quarto_pandoc_types::inline::Inline], out: &mut String) {
        for i in inlines {
            match i {
                quarto_pandoc_types::inline::Inline::Str(s) => out.push_str(&s.text),
                quarto_pandoc_types::inline::Inline::Space(_) => out.push(' '),
                quarto_pandoc_types::inline::Inline::Link(l) => inlines_to_text(&l.content, out),
                _ => {}
            }
        }
    }

    // 33. render_emits_div_at_listing_id_slot
    #[tokio::test]
    async fn render_fills_explicit_slot() {
        let listing = make_listing(ListingType::Default);
        let items = vec![make_item("a", Some("2026-01-01"))];
        let resolved = vec![ResolvedListing { listing, items }];
        let (ast, diags) = run_transform(pandoc_with_slot("main-listing"), resolved).await;
        assert!(diags.is_empty(), "diags: {:?}", diags);
        // The original Div is still there at index 0; its
        // content should now contain rendered listing markup.
        let Block::Div(div) = &ast.blocks[0] else {
            panic!("expected Div at index 0")
        };
        assert_eq!(
            div.attr.2.get("data-listing-rendered").map(String::as_str),
            Some("1"),
            "expected data-listing-rendered attribute on slot, got: {:?}",
            div.attr.2
        );
        let titles = rendered_block_titles(div);
        assert!(
            titles.iter().any(|t| t.contains("a")),
            "expected item title `a` in rendered headers, got {:?}",
            titles
        );
    }

    // 34. render_appends_div_when_no_explicit_slot
    #[tokio::test]
    async fn render_appends_div_when_no_explicit_slot() {
        let listing = make_listing(ListingType::Default);
        let items = vec![make_item("a", Some("2026-01-01"))];
        let resolved = vec![ResolvedListing { listing, items }];
        let (ast, _) = run_transform(empty_pandoc(), resolved).await;
        // A new Div with id `main-listing` and class
        // `quarto-listing` was appended.
        assert_eq!(ast.blocks.len(), 1);
        let Block::Div(div) = &ast.blocks[0] else {
            panic!()
        };
        assert_eq!(div.attr.0, "main-listing");
        assert!(div.attr.1.iter().any(|c| c == "quarto-listing"));
    }

    // 35. render_idempotent_on_repeat
    #[tokio::test]
    async fn render_is_idempotent_on_repeat() {
        let listing = make_listing(ListingType::Default);
        let items = vec![make_item("a", Some("2026-01-01"))];
        let resolved = vec![ResolvedListing {
            listing: listing.clone(),
            items: items.clone(),
        }];
        let mut ast = pandoc_with_slot("main-listing");

        let project = make_project();
        let doc = DocumentInfo::from_path("/project/posts/index.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let index = Arc::new(ProjectIndex::new(Vec::<DocumentProfile>::new()));
        let mut ctx =
            RenderContext::new(&project, &doc, &format, &binaries).with_project_index(index);
        ctx.resolved_listings = resolved.clone();

        // First pass.
        ListingRenderTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        let first_pass_block_count = if let Block::Div(d) = &ast.blocks[0] {
            d.content.len()
        } else {
            0
        };
        assert!(first_pass_block_count > 0);

        // Second pass — listing data is still on ctx; the slot
        // is now marked rendered so the run is a no-op.
        ctx.resolved_listings = resolved;
        ListingRenderTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        let second_pass_block_count = if let Block::Div(d) = &ast.blocks[0] {
            d.content.len()
        } else {
            0
        };
        assert_eq!(
            first_pass_block_count, second_pass_block_count,
            "second pass should not re-emit content"
        );
    }

    // 36. render_falls_back_to_default_for_custom_type
    #[tokio::test]
    async fn render_emits_q_12_1_for_custom_type() {
        let listing = make_listing(ListingType::Custom);
        let items = vec![make_item("a", Some("2026-01-01"))];
        let resolved = vec![ResolvedListing { listing, items }];
        let (_ast, diags) = run_transform(empty_pandoc(), resolved).await;
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("Q-12-1")),
            "expected Q-12-1 diagnostic for custom type, got: {:?}",
            diags
        );
    }

    // 37. render_emits_description_placeholder
    #[tokio::test]
    async fn rendered_output_contains_description_placeholder() {
        // The description-placeholder string should land in the
        // rendered AST (as a literal `<!-- desc(...) -->` text
        // node inside the description Div).
        let listing = make_listing(ListingType::Default);
        let items = vec![make_item("foo", Some("2026-01-01"))];
        let resolved = vec![ResolvedListing { listing, items }];
        let (ast, _) = run_transform(empty_pandoc(), resolved).await;
        let serialized = format!("{:?}", ast);
        assert!(
            serialized.contains("desc(5A0113B34292)"),
            "expected description placeholder in rendered AST"
        );
    }

    #[tokio::test]
    async fn render_skips_when_listing_disabled() {
        // is_feature_disabled short-circuits even if resolved
        // listings are present (defensive).
        use quarto_pandoc_types::ConfigMapEntry;

        let listing = make_listing(ListingType::Default);
        let items = vec![make_item("a", Some("2026-01-01"))];
        let mut ast = empty_pandoc();
        ast.meta = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "listing".to_string(),
                key_source: SourceInfo::default(),
                value: ConfigValue::new_bool(false, SourceInfo::default()),
            }],
            SourceInfo::default(),
        );
        let resolved = vec![ResolvedListing { listing, items }];
        let (ast, _) = run_transform(ast, resolved).await;
        // No new blocks; nothing rendered.
        assert!(ast.blocks.is_empty());
    }

    #[tokio::test]
    async fn render_grid_emits_grid_class() {
        let listing = make_listing(ListingType::Grid);
        let items = vec![make_item("a", Some("2026-01-01"))];
        let resolved = vec![ResolvedListing { listing, items }];
        let (ast, _) = run_transform(empty_pandoc(), resolved).await;
        let serialized = format!("{:?}", ast);
        assert!(
            serialized.contains("quarto-listing-grid") || serialized.contains("quarto-grid-item"),
            "expected grid class in rendered AST"
        );
    }
}
