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

use std::path::{Path, PathBuf};

use hashlink::LinkedHashMap;
use quarto_doctemplate::{PartialResolver, Template, TemplateContext, project_listing_resolver};
use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::attr::AttrSourceInfo;
use quarto_pandoc_types::block::{Block, Div};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::{By, SourceInfo};

use crate::Result;
use crate::project::listing::ResolvedListing;
use crate::project::listing::binding::build_listing_context;
use crate::project::listing::config::ListingType;
use crate::project::listing::templates::{builtins_resolver, top_level_template_source};
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};
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

    fn phase(&self) -> TransformPhase {
        TransformPhase::Navigation
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

        // The host page's project-relative directory feeds into the
        // binding's `path` field so listing item links emit as
        // host-dir-relative `.qmd` source paths — `LinkRewriteTransform`
        // (later in this stage) then rewrites them via the resolver.
        let host_path_str = crate::transforms::navigation_active::page_relative_source(ctx);
        let host_dir = std::path::Path::new(&host_path_str)
            .parent()
            .map(|p| {
                p.components()
                    .filter_map(|c| match c {
                        std::path::Component::Normal(os) => os.to_str().map(str::to_string),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("/")
            })
            .unwrap_or_default();

        // Absolute host-page path. Used by `load_custom_template`
        // to resolve `template:` paths relative to the host page's
        // directory (Q1-parity).
        let host_input: PathBuf = ctx.document.input.clone();

        for r in &resolved {
            render_one(ast, r, &host_dir, &host_input, &mut diags);
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

fn render_one(
    ast: &mut Pandoc,
    r: &ResolvedListing,
    host_dir: &str,
    host_input: &Path,
    diags: &mut Vec<DiagnosticMessage>,
) {
    // Build the binding. The host page's meta is used to extract
    // project.* values (site-url, title) for the templates;
    // `host_dir` feeds the per-item path computation.
    let template_ctx = build_listing_context(&r.listing, &r.items, host_dir, &ast.meta);

    // Compile + render the top-level template. Custom listings load
    // a user-supplied file via a chained (filesystem → built-ins)
    // resolver so they can include built-in partials and have their
    // own neighboring partial files. Built-in listings use the
    // embedded sources directly with the in-memory built-ins
    // resolver — no filesystem lookup is attempted, so partial-name
    // collisions with files in the user's CWD are impossible.
    let markdown = match r.listing.kind {
        ListingType::Custom => match load_custom_template(r, host_input, diags) {
            Some(custom) => compile_and_render(
                &r.listing.id,
                &custom.source,
                &custom.template_path,
                &custom.resolver,
                &template_ctx,
                diags,
            ),
            // No usable custom template — fall back to default. The
            // appropriate Q-12-* diagnostic was already emitted by
            // `load_custom_template`.
            None => render_builtin(&r.listing.id, ListingType::Default, &template_ctx, diags),
        },
        kind => render_builtin(&r.listing.id, kind, &template_ctx, diags),
    };
    let Some(markdown) = markdown else { return };

    // Q1 parity (bd-listing-ellipsis-no-matching-l963osy1): every
    // listing — built-in and custom alike — is followed by a hidden
    // "No matching items" placeholder, mirroring Q1's
    // `_pagination.ejs.md` partial (appended after the rendered
    // template in `website-listing-template.ts`). The vendored
    // quarto-listing.js reveals it when a filter/search matches
    // nothing (the List.js init wiring that drives that is
    // bd-nbv80e33). Emitted as a qmd div rather than raw HTML so
    // Lua filters and AST transforms can see it. The text comes
    // from the localized `listing-page-no-matches` term.
    let no_matching = crate::language::LanguageTerms::from_meta(&ast.meta)
        .and_then(|terms| {
            terms
                .get("listing-page-no-matches")
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| "No matching items".to_string());
    let markdown =
        format!("{markdown}\n\n::: {{.listing-no-matching .d-none}}\n{no_matching}\n:::\n");

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
                        .map_or("(no message)", |d| d.title.as_str())
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
            source_info: SourceInfo::generated(By::programmatic_config()),
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

/// A successfully-loaded custom listing template, ready to compile.
struct LoadedCustomTemplate {
    /// Template source text.
    source: String,
    /// Absolute on-disk path. Used as the `base_path` for partial
    /// resolution: a partial referenced as `my-helper` resolves to
    /// `<host-dir>/my-helper.template` via [`FileSystemResolver`],
    /// falling back to the in-memory built-ins.
    template_path: PathBuf,
    /// Filesystem-then-built-ins resolver chain. The filesystem
    /// layer is primary, so a custom template can shadow a built-in
    /// partial (e.g. `item-default`) by placing a same-named file
    /// next to the host page.
    resolver: quarto_doctemplate::ChainedResolver<
        quarto_doctemplate::FileSystemResolver,
        quarto_doctemplate::MemoryResolver,
    >,
}

/// Try to load a user-supplied listing template.
///
/// Returns `None` (and emits a `Q-12-*` diagnostic) when the template
/// is unavailable; the caller falls back to the `default` built-in.
/// Failure modes:
///
/// - **`Q-12-14`** — `type: custom` declared but no `template:` path.
/// - **`Q-12-8`** — `template:` was supplied but the file could not
///   be read (missing, permission denied, not UTF-8, …). All I/O
///   failure modes share one diagnostic; users see "missing or
///   unreadable" framing in the rendered output.
///
/// Path resolution (Q1-parity): `template:` is resolved relative to
/// the host page's directory. Absolute paths are accepted as-is.
/// Project-root and `_extensions/` lookups are not in v1; see the L8
/// sub-plan for the deferred-followup linkage.
fn load_custom_template(
    r: &ResolvedListing,
    host_input: &Path,
    diags: &mut Vec<DiagnosticMessage>,
) -> Option<LoadedCustomTemplate> {
    let Some(template_rel) = r.listing.template.as_deref() else {
        push_diag(
            diags,
            "Q-12-14",
            format!(
                "Listing `{}` declares `type: custom` but no `template:` path. \
                 Falling back to the `default` built-in.",
                r.listing.id
            ),
        );
        return None;
    };

    let template_abs = if template_rel.is_absolute() {
        template_rel.to_path_buf()
    } else {
        host_input
            .parent()
            .map_or_else(|| template_rel.to_path_buf(), |p| p.join(template_rel))
    };

    match std::fs::read_to_string(&template_abs) {
        Ok(source) => Some(LoadedCustomTemplate {
            source,
            template_path: template_abs,
            resolver: project_listing_resolver(builtins_resolver()),
        }),
        Err(_) => {
            push_diag(
                diags,
                "Q-12-8",
                format!(
                    "Listing `{}`: template file `{}` could not be read. \
                     Falling back to the `default` built-in.",
                    r.listing.id,
                    template_abs.display()
                ),
            );
            None
        }
    }
}

/// Compile the embedded source for a built-in listing type and
/// render it. Built-ins use the in-memory resolver only — no
/// filesystem lookup, so partial-name collisions with files in the
/// host's directory are impossible.
fn render_builtin(
    listing_id: &str,
    kind: ListingType,
    template_ctx: &TemplateContext,
    diags: &mut Vec<DiagnosticMessage>,
) -> Option<String> {
    let source = top_level_template_source(kind);
    // Synthetic name with `.template` extension so the
    // FileSystemResolver-style partial-name fallback (within
    // `Template::compile_with_resolver`) lines up; no filesystem
    // lookup happens here because the resolver is `MemoryResolver`.
    let template_path = Path::new("listing.template");
    let resolver = builtins_resolver();
    compile_and_render(
        listing_id,
        source,
        template_path,
        &resolver,
        template_ctx,
        diags,
    )
}

/// Compile a doctemplate source and render it against `template_ctx`.
/// Compile / render / diagnostic-channel errors all surface as
/// `Q-12-10` and return `None`; the caller skips the listing.
fn compile_and_render<R: PartialResolver>(
    listing_id: &str,
    source: &str,
    template_path: &Path,
    resolver: &R,
    template_ctx: &TemplateContext,
    diags: &mut Vec<DiagnosticMessage>,
) -> Option<String> {
    let template = match Template::compile_with_resolver(source, template_path, resolver, 0) {
        Ok(t) => t,
        Err(e) => {
            push_diag(
                diags,
                "Q-12-10",
                format!(
                    "Listing `{listing_id}` template failed to compile: {e:?}. \
                     Listing skipped."
                ),
            );
            return None;
        }
    };
    let (rendered, render_diags) = template.render_with_diagnostics(template_ctx);
    let markdown = match rendered {
        Ok(s) => s,
        Err(()) => {
            push_diag(
                diags,
                "Q-12-10",
                format!("Listing `{listing_id}` template rendering failed; listing skipped."),
            );
            return None;
        }
    };
    if !render_diags.is_empty() {
        push_diag(
            diags,
            "Q-12-10",
            format!(
                "Listing `{}` doctemplate produced {} diagnostic(s); first: {}",
                listing_id,
                render_diags.len(),
                render_diags[0].title
            ),
        );
    }
    Some(markdown)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_profile::DocumentProfile;
    use crate::format::Format;
    use crate::project::index::ProjectIndex;
    use crate::project::listing::config::apply_type_defaults;
    use crate::project::listing::config::{Listing, ListingType};
    use crate::project::listing::item::{ItemOrigin, ItemTarget, ListingItem};
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

            ..Default::default()
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
            order: None,
            target: ItemTarget::document(
                format!("posts/{}.qmd", title),
                format!("posts/{}.html", title),
            ),
            origin: ItemOrigin::Document,
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

    /// Run the transform with a host page rooted at a caller-supplied
    /// project directory. Used by custom-template tests that need to
    /// place a real `<host-dir>/<template>.template` file on disk.
    async fn run_transform_at(
        ast: Pandoc,
        resolved: Vec<ResolvedListing>,
        project_dir: &Path,
        host_input: &Path,
    ) -> (Pandoc, Vec<DiagnosticMessage>) {
        let mut ast = ast;
        let project = ProjectContext {
            dir: project_dir.to_path_buf(),
            config: ProjectConfig::default(),
            is_single_file: false,
            files: vec![DocumentInfo::from_path(host_input)],
            output_dir: project_dir.join("_site"),

            ..Default::default()
        };
        let doc = DocumentInfo::from_path(host_input);
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
                source_info: SourceInfo::for_test(),
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

    // bd-listing-table-fields-peg1w3b3: a table listing with an
    // author-explicit `fields:` subset must render only those
    // columns and must not emit per-item "Undefined variable"
    // doctemplate diagnostics (previously surfaced as Q-12-10) for
    // curated fields the items lack.
    #[tokio::test]
    async fn table_fields_subset_renders_single_column_without_diagnostics() {
        let mut listing = make_listing(ListingType::Table);
        listing.fields = vec!["title".to_string()];
        listing.fields_explicit = true;
        listing
            .field_display_names
            .insert("title".to_string(), "How To".to_string());
        // Neither item has a date; both have author "Jane" (which
        // must not leak into the single-column table).
        let items = vec![make_item("alpha", None), make_item("beta", None)];
        let resolved = vec![ResolvedListing { listing, items }];
        let (ast, diags) = run_transform(empty_pandoc(), resolved).await;
        assert!(
            diags.is_empty(),
            "fields subset must not produce Q-12-10 diagnostics; got: {:?}",
            diags
        );
        let rendered = format!("{:?}", ast);
        assert!(
            rendered.contains("How"),
            "expected `How To` header in rendered table; got: {rendered}"
        );
        assert!(
            !rendered.contains("Jane"),
            "author column leaked into a fields: [title] table; got: {rendered}"
        );
        assert!(
            !rendered.contains("Author"),
            "Author header leaked into a fields: [title] table; got: {rendered}"
        );
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
            titles.iter().any(|t| t.contains('a')),
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

    // L8 / bd-rqgx test #2:
    // type: custom without template: emits Q-12-14 and falls back to
    // the `default` built-in. Replaces the L8-deferral path that
    // emitted Q-12-1.
    #[tokio::test]
    async fn custom_listing_without_template_path_emits_q_12_14_and_falls_back() {
        let listing = make_listing(ListingType::Custom);
        // make_listing leaves template = None.
        let items = vec![make_item("a", Some("2026-01-01"))];
        let resolved = vec![ResolvedListing { listing, items }];
        let (ast, diags) = run_transform(empty_pandoc(), resolved).await;
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("Q-12-14")),
            "expected Q-12-14 when type:custom has no template path; got: {:?}",
            diags
        );
        // The default built-in wraps items in `::: {.list
        // .quarto-listing-default}`, which parses to a Div with that
        // class on its attr tuple. The wrapper is on a Div, not in
        // raw HTML, so we check the AST debug-serialization.
        let serialized = format!("{:?}", ast);
        assert!(
            serialized.contains("quarto-listing-default"),
            "expected fallback to render the default built-in (class \
             `quarto-listing-default`); AST: {serialized}"
        );
    }

    // L8 / bd-rqgx test #3:
    // type: custom with a `template:` pointing at a missing file
    // emits Q-12-8 and falls back to the default built-in.
    #[tokio::test]
    async fn custom_listing_with_missing_template_file_emits_q_12_8_and_falls_back() {
        let mut listing = make_listing(ListingType::Custom);
        listing.template = Some(PathBuf::from("nonexistent-listing-template.template"));
        let items = vec![make_item("a", Some("2026-01-01"))];
        let resolved = vec![ResolvedListing { listing, items }];
        let (ast, diags) = run_transform(empty_pandoc(), resolved).await;
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("Q-12-8")),
            "expected Q-12-8 when template file is missing; got: {:?}",
            diags
        );
        // Must NOT emit Q-12-14 — the user did supply a path, it just
        // doesn't exist.
        assert!(
            diags.iter().all(|d| d.code.as_deref() != Some("Q-12-14")),
            "expected Q-12-14 NOT emitted when path is supplied; got: {:?}",
            diags
        );
        let serialized = format!("{:?}", ast);
        assert!(
            serialized.contains("quarto-listing-default"),
            "expected fallback to render the default built-in; AST: {serialized}"
        );
    }

    /// Build a minimal tempdir-backed host project for custom-
    /// template tests. Writes the supplied template content into
    /// `<tmp>/posts/<template_name>` and returns the
    /// `(tmpdir-handle, project_root, host_input_abs)` triple. The
    /// caller drives `run_transform_at` with these values.
    fn custom_template_project(
        template_name: &str,
        template_body: &str,
    ) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let posts_dir = tmp.path().join("posts");
        std::fs::create_dir_all(&posts_dir).expect("create posts dir");
        std::fs::write(posts_dir.join(template_name), template_body).expect("write template");
        let host_input = posts_dir.join("index.qmd");
        // The host page itself doesn't need to exist on disk — the
        // transform only reads its directory to resolve template
        // paths. Touch the file so any future code that assumes
        // existence still works.
        std::fs::write(&host_input, "").expect("touch host");
        let root = tmp.path().to_path_buf();
        (tmp, root, host_input)
    }

    fn make_custom_listing(template_name: &str) -> Listing {
        let mut l = Listing {
            id: "main-listing".to_string(),
            kind: ListingType::Custom,
            template: Some(PathBuf::from(template_name)),
            ..Listing::default()
        };
        apply_type_defaults(&mut l);
        l
    }

    // L8 / bd-rqgx test #5:
    // A custom template renders against the same listing/items
    // bindings the built-ins use.
    #[tokio::test]
    async fn custom_template_renders_with_listing_and_items_bindings() {
        let (_tmp, root, host) = custom_template_project(
            "simple.template",
            // Distinctive sentinel string so we know the custom
            // template fired — `quarto-listing-default` would tell
            // us we hit the fallback instead.
            "::: {.list .my-custom-layout}\n\
             id=$listing.id$\n\n\
             $for(items)$- $it.title$\n$endfor$\n\
             :::\n",
        );
        let listing = make_custom_listing("simple.template");
        let items = vec![make_item("alpha", Some("2026-01-01"))];
        let resolved = vec![ResolvedListing { listing, items }];
        let (ast, diags) = run_transform_at(empty_pandoc(), resolved, &root, &host).await;
        assert!(diags.is_empty(), "expected no diagnostics; got: {diags:?}");
        let serialized = format!("{:?}", ast);
        assert!(
            serialized.contains("my-custom-layout"),
            "expected custom-template wrapper class in AST; got: {serialized}"
        );
        assert!(
            serialized.contains("id=main-listing"),
            "expected listing.id binding to render; got: {serialized}"
        );
        assert!(
            serialized.contains("alpha"),
            "expected item title `alpha` to render; got: {serialized}"
        );
    }

    // L8 / bd-rqgx test #6:
    // A custom template can call the built-in `item-default` partial
    // through the chained resolver.
    #[tokio::test]
    async fn custom_template_can_call_built_in_item_default_partial() {
        // `$items:item-default()$` is the same partial-include syntax
        // the built-in `listing-default` template uses; we expect to
        // see the same rendered markup (e.g. the `quarto-post` class
        // from item-default.template).
        let (_tmp, root, host) = custom_template_project(
            "wrap.template",
            "::: {.list .wrapper-marker}\n\n$items:item-default()$\n\n:::\n",
        );
        let listing = make_custom_listing("wrap.template");
        let items = vec![make_item("alpha", Some("2026-01-01"))];
        let resolved = vec![ResolvedListing { listing, items }];
        let (ast, diags) = run_transform_at(empty_pandoc(), resolved, &root, &host).await;
        assert!(diags.is_empty(), "expected no diagnostics; got: {diags:?}");
        let serialized = format!("{:?}", ast);
        assert!(
            serialized.contains("wrapper-marker"),
            "expected custom wrapper class; got: {serialized}"
        );
        assert!(
            serialized.contains("quarto-post"),
            "expected the built-in `item-default` partial markup \
             (class `quarto-post`) to be expanded inside the custom \
             wrapper; got: {serialized}"
        );
    }

    // L8 / bd-rqgx test #7:
    // A neighboring file with the same name as a built-in partial
    // shadows the built-in via the FileSystemResolver primary.
    #[tokio::test]
    async fn custom_template_can_shadow_a_built_in_partial_with_local_file() {
        let (tmp, root, host) = custom_template_project(
            "wrap.template",
            "::: {.list .wrapper-marker}\n\n$items:item-default()$\n\n:::\n",
        );
        // Drop a local `item-default.template` next to the host page;
        // FileSystemResolver is the primary, so it must win over the
        // embedded built-in.
        let posts_dir = tmp.path().join("posts");
        std::fs::write(
            posts_dir.join("item-default.template"),
            "[SHADOWED:$title$]{.shadowed-item}\n",
        )
        .expect("write shadow partial");
        let listing = make_custom_listing("wrap.template");
        let items = vec![make_item("alpha", Some("2026-01-01"))];
        let resolved = vec![ResolvedListing { listing, items }];
        let (ast, diags) = run_transform_at(empty_pandoc(), resolved, &root, &host).await;
        assert!(diags.is_empty(), "expected no diagnostics; got: {diags:?}");
        let serialized = format!("{:?}", ast);
        assert!(
            serialized.contains("shadowed-item"),
            "expected local file to shadow built-in partial; got: {serialized}"
        );
        assert!(
            serialized.contains("SHADOWED"),
            "expected literal text from local partial; got: {serialized}"
        );
        assert!(
            !serialized.contains("quarto-post"),
            "the built-in partial must NOT be expanded when shadowed; got: {serialized}"
        );
    }

    // L8 / bd-rqgx test #8:
    // Custom templates see `listing.template-params` keys verbatim.
    #[tokio::test]
    async fn custom_template_sees_listing_template_params() {
        let (_tmp, root, host) = custom_template_project(
            "params.template",
            "::: {.list .params-marker}\n\
             color=$listing.template-params.color$ count=$listing.template-params.count$\n\
             :::\n",
        );
        let mut listing = make_custom_listing("params.template");
        listing.template_params.insert(
            "color".to_string(),
            ConfigValue::new_string("red".to_string(), SourceInfo::for_test()),
        );
        listing.template_params.insert(
            "count".to_string(),
            ConfigValue::new_string("3".to_string(), SourceInfo::for_test()),
        );
        let items = vec![make_item("a", Some("2026-01-01"))];
        let resolved = vec![ResolvedListing { listing, items }];
        let (ast, diags) = run_transform_at(empty_pandoc(), resolved, &root, &host).await;
        assert!(diags.is_empty(), "expected no diagnostics; got: {diags:?}");
        let serialized = format!("{:?}", ast);
        assert!(
            serialized.contains("color=red"),
            "expected color=red from template-params; got: {serialized}"
        );
        assert!(
            serialized.contains("count=3"),
            "expected count=3 from template-params; got: {serialized}"
        );
    }

    // L8 / bd-rqgx test #9:
    // Custom templates see `item.extra.<key>` from each item's
    // free-form authoring map.
    #[tokio::test]
    async fn custom_template_sees_item_extra() {
        let (_tmp, root, host) = custom_template_project(
            "extra.template",
            "::: {.list .extra-marker}\n\
             $for(items)$- $it.title$ status=$it.extra.status$\n$endfor$\n\
             :::\n",
        );
        let listing = make_custom_listing("extra.template");
        let mut a = make_item("alpha", Some("2026-01-01"));
        a.extra.insert(
            "status".to_string(),
            ConfigValue::new_string("draft".to_string(), SourceInfo::for_test()),
        );
        let mut b = make_item("beta", Some("2026-01-02"));
        b.extra.insert(
            "status".to_string(),
            ConfigValue::new_string("published".to_string(), SourceInfo::for_test()),
        );
        let resolved = vec![ResolvedListing {
            listing,
            items: vec![a, b],
        }];
        let (ast, diags) = run_transform_at(empty_pandoc(), resolved, &root, &host).await;
        assert!(diags.is_empty(), "expected no diagnostics; got: {diags:?}");
        let serialized = format!("{:?}", ast);
        assert!(
            serialized.contains("status=draft"),
            "expected status=draft from item.extra; got: {serialized}"
        );
        assert!(
            serialized.contains("status=published"),
            "expected status=published from item.extra; got: {serialized}"
        );
    }

    // L8 / bd-rqgx test #10:
    // Custom templates see `listing.fields` and per-item
    // `show.<field>` flags. Q1 semantics (and the built-in
    // templates' usage) is `$if(it.show.<field>)$` — `show.<field>`
    // is only present in the binding when the field is in
    // `listing.fields`, and `$if$` treats missing as false. This
    // test exercises the same idiom an author would write.
    #[tokio::test]
    async fn custom_template_sees_listing_fields_and_per_item_show() {
        let (_tmp, root, host) = custom_template_project(
            "fields.template",
            "::: {.list .fields-marker}\n\
             $for(items)$$if(it.show.title)$[t-yes]$endif$\
             $if(it.show.date)$[d-yes]$endif$\
             $if(it.show.author)$[a-yes]$endif$\n$endfor$\n\
             :::\n",
        );
        let mut listing = make_custom_listing("fields.template");
        // apply_type_defaults for `Custom` leaves fields empty; set
        // explicitly so this test exercises the binding the way an
        // author would.
        listing.fields = vec!["title".to_string(), "date".to_string()];
        let items = vec![make_item("alpha", Some("2026-01-01"))];
        let resolved = vec![ResolvedListing { listing, items }];
        let (ast, diags) = run_transform_at(empty_pandoc(), resolved, &root, &host).await;
        assert!(diags.is_empty(), "expected no diagnostics; got: {diags:?}");
        let serialized = format!("{:?}", ast);
        assert!(
            serialized.contains("t-yes"),
            "expected `title` show flag truthy; got: {serialized}"
        );
        assert!(
            serialized.contains("d-yes"),
            "expected `date` show flag truthy; got: {serialized}"
        );
        assert!(
            !serialized.contains("a-yes"),
            "expected `author` show flag falsy (not in fields); got: {serialized}"
        );
    }

    // L8 / bd-rqgx test #11:
    // A custom template that fails to compile (e.g. unbalanced
    // `$if`) emits Q-12-10 and skips the listing.
    #[tokio::test]
    async fn custom_template_with_compile_error_emits_q_12_10_and_skips_listing() {
        let (_tmp, root, host) = custom_template_project(
            "broken.template",
            // Unbalanced `$if$` — no `$endif$`.
            "::: {.list .broken-marker}\n$if(listing.id)$oops$\n:::\n",
        );
        let listing = make_custom_listing("broken.template");
        let items = vec![make_item("a", Some("2026-01-01"))];
        let resolved = vec![ResolvedListing { listing, items }];
        let (ast, diags) = run_transform_at(empty_pandoc(), resolved, &root, &host).await;
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("Q-12-10")),
            "expected Q-12-10 for compile error; got: {diags:?}"
        );
        // The listing was skipped — the host AST should not contain
        // the marker class. (No fallback to default for compile
        // errors — that path returns None, which the transform
        // treats as "skip the listing".)
        let serialized = format!("{:?}", ast);
        assert!(
            !serialized.contains("broken-marker"),
            "expected the listing to be skipped on compile error; got: {serialized}"
        );
        assert!(
            !serialized.contains("quarto-listing-default"),
            "expected NO default fallback on compile error; got: {serialized}"
        );
    }

    // L8 / bd-rqgx test #12:
    // An absolute `template:` path is read from that absolute
    // location (not joined to host_dir).
    #[tokio::test]
    async fn custom_template_with_absolute_path_resolves_correctly() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let abs_template = tmp.path().join("absolute-layout.template");
        std::fs::write(
            &abs_template,
            "::: {.list .absolute-marker}\n$listing.id$\n:::\n",
        )
        .expect("write absolute template");
        // Host page lives in a separate tempdir to confirm the
        // template is NOT looked up host-relative.
        let host_tmp = tempfile::tempdir().expect("host tempdir");
        let posts_dir = host_tmp.path().join("posts");
        std::fs::create_dir_all(&posts_dir).expect("create posts dir");
        let host_input = posts_dir.join("index.qmd");
        std::fs::write(&host_input, "").expect("touch host");
        let mut listing = make_custom_listing("absolute-layout.template");
        listing.template = Some(abs_template.clone());
        let items = vec![make_item("a", Some("2026-01-01"))];
        let resolved = vec![ResolvedListing { listing, items }];
        let (ast, diags) =
            run_transform_at(empty_pandoc(), resolved, host_tmp.path(), &host_input).await;
        assert!(diags.is_empty(), "expected no diagnostics; got: {diags:?}");
        let serialized = format!("{:?}", ast);
        assert!(
            serialized.contains("absolute-marker"),
            "expected absolute-path template to render; got: {serialized}"
        );
    }

    // L8 / bd-rqgx test #13:
    // `template:` paths resolve relative to the host page's
    // directory, not the project root. A template file at the
    // project root with the same name as a missing one in the
    // host directory does NOT satisfy the lookup.
    #[tokio::test]
    async fn custom_template_path_uses_host_dir_not_project_root() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let posts_dir = tmp.path().join("posts");
        std::fs::create_dir_all(&posts_dir).expect("create posts dir");
        // Decoy at project root.
        std::fs::write(
            tmp.path().join("layout.template"),
            "::: {.list .root-decoy}\n$listing.id$\n:::\n",
        )
        .expect("write root decoy");
        // Host page in posts/ — no `layout.template` next to it.
        let host_input = posts_dir.join("index.qmd");
        std::fs::write(&host_input, "").expect("touch host");
        let listing = make_custom_listing("layout.template");
        let items = vec![make_item("a", Some("2026-01-01"))];
        let resolved = vec![ResolvedListing { listing, items }];
        let (ast, diags) =
            run_transform_at(empty_pandoc(), resolved, tmp.path(), &host_input).await;
        // Q-12-8 must fire — the file isn't in posts/ — and the
        // root-level decoy must NOT have rendered.
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("Q-12-8")),
            "expected Q-12-8 (host-dir lookup miss); got: {diags:?}"
        );
        let serialized = format!("{:?}", ast);
        assert!(
            !serialized.contains("root-decoy"),
            "project-root decoy must NOT have been picked up; got: {serialized}"
        );
        assert!(
            serialized.contains("quarto-listing-default"),
            "expected default fallback after Q-12-8; got: {serialized}"
        );
    }

    // L8 / bd-rqgx test #14:
    // A `.ejs.md` template (genuine EJS syntax) emits Q-12-9 at
    // parse-time (covered separately) and Q-12-10 at compile-time
    // here. The listing falls back via the compile-error path
    // (skip), not via the default-fallback path.
    //
    // (The parse-time Q-12-9 emitter lives in
    // `project/listing/config.rs` and is exercised by config tests;
    // L8 only verifies the runtime side: load + compile-fail.)
    #[tokio::test]
    async fn custom_template_with_ejs_md_extension_attempts_load_and_fails_compile() {
        let (_tmp, root, host) = custom_template_project(
            "legacy.ejs.md",
            // EJS-style markup — `<%= … %>` is not valid
            // doctemplate syntax. The doctemplate parser will reject
            // it with a `Q-10-*` error which the listing render path
            // surfaces as Q-12-10.
            "<ul>\n<% items.forEach(function(item) { %>\n  \
             <li><%= item.title %></li>\n<% }); %>\n</ul>\n",
        );
        let mut listing = make_custom_listing("legacy.ejs.md");
        listing.template = Some(PathBuf::from("legacy.ejs.md"));
        let items = vec![make_item("a", Some("2026-01-01"))];
        let resolved = vec![ResolvedListing { listing, items }];
        let (_ast, diags) = run_transform_at(empty_pandoc(), resolved, &root, &host).await;
        // We expect a compile error (Q-12-10). The .ejs.md text is
        // not valid doctemplate syntax; whether the parser emits
        // exactly Q-10-N here is a doctemplate concern. What L8
        // promises: a compile-or-render failure surfaces as Q-12-10
        // and the listing is skipped.
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("Q-12-10")),
            "expected Q-12-10 from EJS-syntax compile error; got: {diags:?}"
        );
    }

    // L8 / bd-rqgx test #15:
    // A custom template that wraps `$items:item-default()$` inherits
    // the L7 description envelope markers from the built-in partial.
    #[tokio::test]
    async fn custom_template_using_item_default_partial_emits_l7_envelopes() {
        let (_tmp, root, host) = custom_template_project(
            "wrap.template",
            "::: {.list .wrapper-marker}\n\n$items:item-default()$\n\n:::\n",
        );
        let listing = make_custom_listing("wrap.template");
        let items = vec![make_item("alpha", Some("2026-01-01"))];
        let resolved = vec![ResolvedListing { listing, items }];
        let (ast, diags) = run_transform_at(empty_pandoc(), resolved, &root, &host).await;
        assert!(diags.is_empty(), "expected no diagnostics; got: {diags:?}");
        let serialized = format!("{:?}", ast);
        assert!(
            serialized.contains("desc-begin(5A0113B34292)"),
            "expected description begin marker (inherited from \
             item-default partial); AST: {serialized}"
        );
        assert!(
            serialized.contains("desc-end(5A0113B34292)"),
            "expected description end marker; AST: {serialized}"
        );
    }

    // L8 / bd-rqgx test #16:
    // A custom template that inlines its own item markup (no use of
    // the built-in item partials, no reference to the placeholder
    // bindings) renders correctly using the static binding values.
    // L7's post-render step finds no envelopes to substitute; the
    // listing remains correct via the L1 fallback contract.
    #[tokio::test]
    async fn custom_template_inlining_own_item_markup_can_omit_l7_envelopes() {
        let (_tmp, root, host) = custom_template_project(
            "inline.template",
            "::: {.list .inline-marker}\n\
             $for(items)$- $it.title$ — $it.description$\n$endfor$\n\
             :::\n",
        );
        let listing = make_custom_listing("inline.template");
        let items = vec![make_item("alpha", Some("2026-01-01"))];
        let resolved = vec![ResolvedListing { listing, items }];
        let (ast, diags) = run_transform_at(empty_pandoc(), resolved, &root, &host).await;
        assert!(diags.is_empty(), "expected no diagnostics; got: {diags:?}");
        let serialized = format!("{:?}", ast);
        // The L7 envelope markers must NOT appear — the template
        // never referenced them.
        assert!(
            !serialized.contains("desc-begin"),
            "expected NO description envelope markers; AST: {serialized}"
        );
        assert!(
            !serialized.contains("img-begin"),
            "expected NO image envelope markers; AST: {serialized}"
        );
        // The static L1 fallback content (item title + description)
        // must still be rendered. The post-render markdown is
        // re-parsed, so the description text "alpha description"
        // becomes tokenized inlines (`Str("alpha"), Space,
        // Str("description")`). Assert on tokens present.
        let alpha_count = serialized.matches("text: \"alpha\"").count();
        assert!(
            alpha_count >= 2,
            "expected the literal `alpha` to appear at least twice (title + description); AST: {serialized}"
        );
        assert!(
            serialized.contains("text: \"description\""),
            "expected the description token; AST: {serialized}"
        );
    }

    // L8 / bd-rqgx test #4:
    // The L8-deferral diagnostic Q-12-1 is gone. Whether the user
    // provides a template or not, no Q-12-1 should appear.
    #[tokio::test]
    async fn custom_listing_q_12_1_no_longer_emitted() {
        // Case A: type:custom with no template.
        let listing = make_listing(ListingType::Custom);
        let items = vec![make_item("a", Some("2026-01-01"))];
        let resolved = vec![ResolvedListing { listing, items }];
        let (_ast, diags_a) = run_transform(empty_pandoc(), resolved).await;
        assert!(
            diags_a.iter().all(|d| d.code.as_deref() != Some("Q-12-1")),
            "Q-12-1 must no longer be emitted (no-template case); got: {:?}",
            diags_a
        );
        // Case B: type:custom with a (missing) template file.
        let mut listing = make_listing(ListingType::Custom);
        listing.template = Some(PathBuf::from("nonexistent.template"));
        let items = vec![make_item("a", Some("2026-01-01"))];
        let resolved = vec![ResolvedListing { listing, items }];
        let (_ast, diags_b) = run_transform(empty_pandoc(), resolved).await;
        assert!(
            diags_b.iter().all(|d| d.code.as_deref() != Some("Q-12-1")),
            "Q-12-1 must no longer be emitted (missing-template case); got: {:?}",
            diags_b
        );
    }

    // L7 plan §"Tests" Phase 2 #10. The description envelope's
    // begin AND end markers should land in the rendered AST, with
    // the L1 fallback `description` between them.
    #[tokio::test]
    async fn render_emits_description_begin_end_envelope_around_l1_fallback() {
        let listing = make_listing(ListingType::Default);
        let items = vec![make_item("foo", Some("2026-01-01"))];
        let resolved = vec![ResolvedListing { listing, items }];
        let (ast, _) = run_transform(empty_pandoc(), resolved).await;
        let serialized = format!("{:?}", ast);
        assert!(
            serialized.contains("desc-begin(5A0113B34292)"),
            "expected description begin marker in rendered AST"
        );
        assert!(
            serialized.contains("desc-end(5A0113B34292)"),
            "expected description end marker in rendered AST"
        );
    }

    // L7 plan §"Tests" Phase 2 #11. With no static image, the image
    // envelope must wrap the empty placeholder div.
    #[tokio::test]
    async fn render_emits_image_placeholder_begin_end_when_l1_image_unset() {
        let listing = make_listing(ListingType::Default);
        // make_item leaves image=None.
        let items = vec![make_item("foo", Some("2026-01-01"))];
        let resolved = vec![ResolvedListing { listing, items }];
        let (ast, _) = run_transform(empty_pandoc(), resolved).await;
        let serialized = format!("{:?}", ast);
        assert!(
            serialized.contains("img-begin(9CEB782EFEE6)"),
            "expected image begin marker when no static image; AST: {serialized}"
        );
        assert!(
            serialized.contains("img-end(9CEB782EFEE6)"),
            "expected image end marker when no static image"
        );
        assert!(
            serialized.contains("listing-item-img-placeholder"),
            "expected empty placeholder div between markers"
        );
    }

    // L7 plan §"Tests" Phase 2 #12. With a static image, the image
    // envelope must NOT appear in the rendered output (the template's
    // $if(image-html)$ branch fires, not $else$).
    #[tokio::test]
    async fn render_omits_image_placeholder_when_l1_image_set() {
        let listing = make_listing(ListingType::Default);
        let mut item = make_item("foo", Some("2026-01-01"));
        item.image = Some("/img/foo.png".to_string());
        let items = vec![item];
        let resolved = vec![ResolvedListing { listing, items }];
        let (ast, diags) = run_transform(empty_pandoc(), resolved).await;
        let serialized = format!("{:?}", ast);
        assert!(
            !serialized.contains("img-begin(9CEB782EFEE6)"),
            "image envelope must NOT appear when static image is set; AST: {serialized}"
        );
        assert!(
            serialized.contains("/img/foo.png"),
            "expected static image src in rendered AST"
        );
        // Regression: the static-image branch must not trigger
        // the Q-2-9 ("HTML element converted to raw HTML") /
        // Q-12-10 re-parse warning. The template wraps
        // `$image-html$` in explicit `` `…`{=html} `` syntax for
        // exactly this reason — a bare `<img>` inside link
        // brackets would auto-convert and warn.
        assert!(
            diags.iter().all(|d| d.code.as_deref() != Some("Q-12-10")),
            "expected no Q-12-10 from static-image branch; got: {diags:?}"
        );
    }

    // L7 plan §"Tests" Phase 2 #13. The image marker carries the
    // listing's image-placeholder URL (URL_SAFE_NO_PAD base64) for
    // L7 to consume at substitution time.
    #[tokio::test]
    async fn render_carries_image_placeholder_default_url_into_marker() {
        use base64::Engine;
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let mut listing = make_listing(ListingType::Default);
        listing.image_placeholder = Some("assets/default.png".to_string());
        let items = vec![make_item("foo", Some("2026-01-01"))];
        let resolved = vec![ResolvedListing { listing, items }];
        let (ast, _) = run_transform(empty_pandoc(), resolved).await;
        let serialized = format!("{:?}", ast);
        let expected = URL_SAFE_NO_PAD.encode("assets/default.png".as_bytes());
        assert!(
            serialized.contains(&format!(":{} -->", expected)),
            "expected b64 default URL `{}` in image begin marker; AST: {serialized}",
            expected
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
                key_source: SourceInfo::for_test(),
                value: ConfigValue::new_bool(false, SourceInfo::for_test()),
            }],
            SourceInfo::for_test(),
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

    /// Walk the AST and concatenate every `RawBlock` / `RawInline`
    /// HTML payload into a single string. Convenient for substring
    /// asserting "did the template emit the chip markup".
    fn collect_raw_html(ast: &Pandoc) -> String {
        use quarto_pandoc_types::block::Block as B;
        use quarto_pandoc_types::inline::Inline as I;
        fn walk_blocks(blocks: &[B], out: &mut String) {
            for b in blocks {
                match b {
                    B::RawBlock(rb) if rb.format.eq_ignore_ascii_case("html") => {
                        out.push_str(&rb.text);
                        out.push('\n');
                    }
                    B::Div(d) => walk_blocks(&d.content, out),
                    B::BlockQuote(bq) => walk_blocks(&bq.content, out),
                    B::OrderedList(ol) => {
                        for item in &ol.content {
                            walk_blocks(item, out);
                        }
                    }
                    B::BulletList(bl) => {
                        for item in &bl.content {
                            walk_blocks(item, out);
                        }
                    }
                    B::Paragraph(p) => walk_inlines(&p.content, out),
                    B::Plain(p) => walk_inlines(&p.content, out),
                    B::Header(h) => walk_inlines(&h.content, out),
                    _ => {}
                }
            }
        }
        fn walk_inlines(inlines: &[I], out: &mut String) {
            for i in inlines {
                if let I::RawInline(ri) = i
                    && ri.format.eq_ignore_ascii_case("html")
                {
                    out.push_str(&ri.text);
                    out.push('\n');
                }
            }
        }
        let mut out = String::new();
        walk_blocks(&ast.blocks, &mut out);
        out
    }

    fn item_with_categories(title: &str, cats: &[&str]) -> ListingItem {
        let mut i = make_item(title, Some("2026-01-01"));
        i.categories = cats.iter().map(|s| s.to_string()).collect();
        i
    }

    // L5 plan §"Tests" #26
    #[tokio::test]
    async fn item_default_renders_category_chips_when_categories_field_enabled() {
        let listing = make_listing(ListingType::Default);
        // `categories` is in the default `Default`-type fields list,
        // so `show.categories` is truthy by default.
        assert!(listing.fields.iter().any(|f| f == "categories"));
        let items = vec![item_with_categories("a", &["rust", "design"])];
        let resolved = vec![ResolvedListing { listing, items }];
        let (ast, diags) = run_transform(empty_pandoc(), resolved).await;
        assert!(diags.is_empty(), "diags: {:?}", diags);
        let html = collect_raw_html(&ast);
        assert_eq!(
            html.matches(r#"<div class="listing-category""#).count(),
            2,
            "expected two chips, got HTML: {html}"
        );
        assert!(html.contains(">rust<"));
        assert!(html.contains(">design<"));
    }

    // L5 plan §"Tests" #27
    #[tokio::test]
    async fn item_default_omits_category_chips_when_field_disabled() {
        let mut listing = make_listing(ListingType::Default);
        listing.fields.retain(|f| f != "categories");
        assert!(!listing.fields.iter().any(|f| f == "categories"));
        let items = vec![item_with_categories("a", &["rust", "design"])];
        let resolved = vec![ResolvedListing { listing, items }];
        let (ast, _) = run_transform(empty_pandoc(), resolved).await;
        let html = collect_raw_html(&ast);
        assert_eq!(
            html.matches(r#"<div class="listing-category""#).count(),
            0,
            "expected no chips when categories field disabled, got: {html}"
        );
    }

    // L5 plan §"Tests" #28
    #[tokio::test]
    async fn item_grid_renders_category_chips() {
        let listing = make_listing(ListingType::Grid);
        assert!(listing.fields.iter().any(|f| f == "categories"));
        let items = vec![item_with_categories("a", &["rust", "design"])];
        let resolved = vec![ResolvedListing { listing, items }];
        let (ast, _) = run_transform(empty_pandoc(), resolved).await;
        let html = collect_raw_html(&ast);
        assert_eq!(
            html.matches(r#"<div class="listing-category""#).count(),
            2,
            "expected two chips on grid template, got: {html}"
        );
    }

    // L5 plan §"Tests" #29
    #[tokio::test]
    async fn item_table_unchanged_no_chips() {
        // The table template is hardcoded title/date/author and v1
        // does not honor `listing.fields` for the table cells. L5
        // explicitly does not add per-item chips to the table.
        let listing = make_listing(ListingType::Table);
        let items = vec![item_with_categories("a", &["rust", "design"])];
        let resolved = vec![ResolvedListing { listing, items }];
        let (ast, _) = run_transform(empty_pandoc(), resolved).await;
        let html = collect_raw_html(&ast);
        assert_eq!(
            html.matches(r#"<div class="listing-category""#).count(),
            0,
            "table listing must not emit per-item chips"
        );
    }

    #[tokio::test]
    async fn unlinked_record_item_renders_title_without_anchor() {
        let mut item = make_item("Card", None);
        item.origin = ItemOrigin::Record;
        item.target = ItemTarget::None;
        let resolved = vec![ResolvedListing {
            listing: make_listing(ListingType::Default),
            items: vec![item],
        }];
        let (ast, diags) = run_transform(empty_pandoc(), resolved).await;
        assert!(diags.is_empty(), "{diags:?}");
        let rendered = format!("{:?}", ast);
        assert!(
            rendered.contains("listing-title"),
            "title heading present: {rendered}"
        );
        assert!(rendered.contains("Card"), "{rendered}");
        assert!(
            !rendered.contains("Link("),
            "no Link inline without a target: {rendered}"
        );
    }

    #[tokio::test]
    async fn document_item_still_renders_title_as_link() {
        let resolved = vec![ResolvedListing {
            listing: make_listing(ListingType::Default),
            items: vec![make_item("Doc", None)],
        }];
        let (ast, _) = run_transform(empty_pandoc(), resolved).await;
        let rendered = format!("{:?}", ast);
        assert!(
            rendered.contains("Link("),
            "document items keep their anchor: {rendered}"
        );
    }
}
