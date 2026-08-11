/*
 * transforms/mermaid.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Render ` ```mermaid ` fenced code blocks as browser-rendered
//! diagrams for HTML-family output (`format: html`, `format: revealjs`).
//!
//! A plain fenced block with the `mermaid` class — pampa parses
//! ` ```mermaid ` to `CodeBlock(("", ["mermaid"], []), source)` — is
//! replaced with `RawBlock("html", "<pre class=\"mermaid\">…</pre>")`
//! (source HTML-escaped). When at least one diagram was found, the
//! vendored mermaid runtime is registered as a project-shared
//! artifact and a once-per-document `<script src="…">` + init pair is
//! appended to the canonical `rendered.includes.after-body` list (the
//! [`WebsiteFaviconTransform`](super::WebsiteFaviconTransform) /
//! [`AttributionViewerTransform`](super::AttributionViewerTransform)
//! channel); both the Bootstrap HTML template (`$include-after$`) and
//! the reveal scaffold ([`render_revealjs_document`]) wire that slot
//! in before `</body>`.
//!
//! Design notes (bd-5m4ga0s1, plan
//! `claude-notes/plans/2026-07-20-mermaid-regular-rendering.md`):
//!
//! - **Not an engine.** This supersedes the unmerged
//!   `feature/mermaid-engine` approach (bd-je48v): no `engine:`
//!   declaration, no capture/replay involvement — the block is plain
//!   markdown that any surface can fall back to rendering as code.
//! - **Presentation transform**: [`TransformPhase::Finalization`],
//!   ordered before [`CodeBlockRenderTransform`](super::CodeBlockRenderTransform)
//!   so diagram blocks never grow copy-button/filename chrome, and
//!   (being inside `AstTransformsStage`) before `CodeHighlightStage`
//!   ever sees them.
//! - **Excluded from the q2-preview pipeline**
//!   (`Q2_PREVIEW_TRANSFORM_EXCLUDED`): in `q2 preview` / hub-client
//!   the raw `CodeBlock` must survive to the React layer, where the
//!   built-in mermaid component (ts-packages/preview-renderer) owns
//!   rendering for both `q2-preview` and `q2-slides`.
//! - **`{mermaid}` executable cells are deliberately NOT recognized**
//!   (first-cut decision, ratified 2026-07-20): knitr's
//!   `handledLanguages` already claims `mermaid`, so brace-form cells
//!   remain engine territory.
//! - Explicit `mermaid.run()` rather than `startOnLoad: true` so the
//!   diagrams render regardless of when the module executes relative
//!   to `DOMContentLoaded`.
//!
//! # Bundling (bd-mermaid-runtime-not-bundled-vxejw159)
//!
//! The runtime is **vendored and shipped with the rendered site**, not
//! fetched from a CDN at page load. This makes rendered output work in
//! air-gapped and closed-network settings, removes a third-party
//! runtime dependency (and reader-IP leak) from published
//! documentation, and stops a CDN outage from silently degrading every
//! diagram page.
//!
//! Mechanically this mirrors `resources/revealjs/` (see
//! [`register_reveal_assets`]): the bytes live in
//! `resources/mermaid/mermaid.min.js`, are embedded at compile time
//! via `include_str!`, and are registered as an
//! [`ArtifactScope::Project`] artifact at `mermaid/mermaid.min.js` —
//! which the resolver writes to `_site/site_libs/mermaid/` once per
//! site and references from each page with the correct number of
//! `../` hops.
//!
//! Two things about the vendored file are load-bearing and guarded by
//! tests (see `resources/mermaid/README.md` for the full rationale):
//!
//! - It must be `dist/mermaid.min.js`, which is **self-contained**.
//!   The ESM build (`dist/mermaid.esm.min.mjs`) is a ~26 KB stub over
//!   a 146-file chunk tree whose per-diagram-type chunks load lazily,
//!   so vendoring it would break offline rendering *per diagram type*.
//! - Its embedded version must match [`MERMAID_VERSION`].
//!
//! Because the bundle is a classic script that assigns
//! `globalThis.mermaid`, the emitted markup is a plain
//! `<script src>` followed by an inline init — not a module import.
//!
//! [`render_revealjs_document`]: crate::revealjs::render_revealjs_document
//! [`register_reveal_assets`]: crate::revealjs::register_reveal_assets

use std::path::Path;

use quarto_pandoc_types::block::{Block, Blocks, RawBlock};
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::artifact::{Artifact, ArtifactScope, ArtifactStore};
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};
use crate::transforms::append_with_sentinel;

/// Exact-pinned mermaid.js version (decision 1, plan §Resolved
/// decisions — matches the copy Quarto 1 bundles). The TS preview
/// component pins the same version in
/// `ts-packages/preview-renderer/src/q2-preview/blocks/MermaidCodeBlock.tsx`;
/// bump both together.
///
/// Must equal the version embedded in the vendored bundle — enforced
/// by `vendored_bundle_matches_pinned_version`.
pub const MERMAID_VERSION: &str = "11.12.0";

/// The vendored mermaid runtime, embedded at compile time.
///
/// See the module docs and `resources/mermaid/README.md`: this is the
/// **self-contained** `dist/mermaid.min.js`, deliberately not the
/// chunk-loading ESM build.
const MERMAID_JS: &str = include_str!("../../../../resources/mermaid/mermaid.min.js");

/// Destination path for the runtime, relative to the project lib dir —
/// so `_site/site_libs/mermaid/mermaid.min.js` for a website, and the
/// per-page resource dir for a single-doc render.
const MERMAID_ARTIFACT_PATH: &str = "mermaid/mermaid.min.js";

/// Artifact store key.
///
/// Deliberately **not** the `js:` prefix used by
/// [`crate::dependency::store_html_dependencies`] and the reveal
/// assets, because `js:` means something specific: `ApplyTemplateStage`
/// collects every `js:` artifact and emits a `<script src>` for it in
/// the Bootstrap template's `<head>` (`apply_template.rs:167`). Two
/// reasons that is the wrong channel here:
///
/// 1. **Double emission.** This transform emits its own `<script src>`
///    so it can put the runtime immediately next to the
///    `mermaid.initialize(...)` / `mermaid.run(...)` call that depends
///    on it. A `js:` key would additionally emit a head tag, loading
///    2.6 MiB twice per page.
/// 2. **revealjs would be missed.** The reveal scaffold collects only
///    `js:revealjs:*` (`apply_template.rs:305`) — a deck never wants
///    the Bootstrap asset set — so a `js:mermaid:` artifact would
///    produce no tag at all in a deck, silently breaking diagrams in
///    presentations.
///
/// Writing to disk does not depend on the prefix: `flush` is
/// path-driven (`artifact_flush.rs:109` skips only artifacts with no
/// path), so the runtime still lands under `site_libs/` exactly like
/// the `js:`-keyed assets do.
const MERMAID_ARTIFACT_KEY: &str = "mermaid:runtime";

/// HTML-comment sentinel embedded in the injected `<script>` block so
/// a transform re-run is idempotent (same contract as the
/// attribution-viewer includes).
const MERMAID_JS_SENTINEL: &str = "<!-- quarto-mermaid-js -->";

/// See module docs.
pub struct MermaidRenderTransform;

impl MermaidRenderTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MermaidRenderTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for MermaidRenderTransform {
    fn name(&self) -> &str {
        "mermaid-render"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Finalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        // Self-gate: mermaid.js is a browser runtime; only HTML-family
        // output (html, revealjs) can host it. A future PDF/docx story
        // needs pre-rendering (Q1 uses headless Chrome) — out of scope
        // for the first cut, see the plan's §Syntax decision.
        if !ctx.format.identifier.is_html_based() {
            return Ok(());
        }

        let mut found = false;
        convert_mermaid_blocks(&mut ast.blocks, &mut found);
        if found {
            // Register before computing the URL: the artifact carries
            // the destination path, the resolver turns it into a
            // page-relative href. Diagram-free documents reach neither
            // line, so they ship no runtime and no script.
            register_mermaid_assets(&mut ctx.artifacts);
            let runtime_url = mermaid_runtime_url(ctx);
            append_with_sentinel(
                &mut ast.meta,
                "after-body",
                MERMAID_JS_SENTINEL,
                mermaid_script_block(&runtime_url),
            );
        }
        Ok(())
    }
}

/// Store the vendored runtime as a project-shared artifact.
///
/// [`ArtifactScope::Project`] means a multi-page site writes one copy
/// under `site_libs/mermaid/` and every page links to it; single-doc
/// renders resolve Project scope to the per-page resource directory,
/// so the behavior degrades sensibly. Storing under a fixed key makes
/// repeat calls idempotent — re-registering overwrites with identical
/// bytes rather than accumulating copies.
///
/// Public for the same reason [`crate::revealjs::register_reveal_assets`]
/// is: the preview/hub-client path will need to serve the same bytes
/// (bd-1vwtdwtq).
pub fn register_mermaid_assets(artifacts: &mut ArtifactStore) {
    artifacts.store(
        MERMAID_ARTIFACT_KEY,
        Artifact::from_string(MERMAID_JS, "text/javascript")
            .with_path(MERMAID_ARTIFACT_PATH)
            .with_scope(ArtifactScope::Project),
    );
}

/// Page-relative URL for the runtime.
///
/// The resolver knows how deep the current page sits and emits the
/// right number of `../` hops (`site_libs/…` at the site root,
/// `../site_libs/…` one level down). When no resolver is attached —
/// some unit tests, and in-memory callers that never write files — we
/// fall back to the bare relative path, matching
/// `collect_artifact_urls` in the apply-template stage.
fn mermaid_runtime_url(ctx: &RenderContext) -> String {
    match &ctx.resource_resolver {
        Some(resolver) => {
            resolver.html_url_for(ArtifactScope::Project, Path::new(MERMAID_ARTIFACT_PATH))
        }
        None => MERMAID_ARTIFACT_PATH.to_string(),
    }
}

/// Walk `blocks`, replacing each `CodeBlock` carrying the `mermaid`
/// class with the `<pre class="mermaid">` RawBlock. Descends into the
/// same container variants as
/// [`CodeBlockRenderTransform`](super::CodeBlockRenderTransform)'s
/// walker so nested diagrams (columns, slide sections, list items)
/// convert too.
fn convert_mermaid_blocks(blocks: &mut Blocks, found: &mut bool) {
    for block in blocks.iter_mut() {
        match block {
            Block::CodeBlock(cb) if cb.attr.1.iter().any(|c| c == "mermaid") => {
                *found = true;
                let text = format!("<pre class=\"mermaid\">\n{}\n</pre>", html_escape(&cb.text));
                let source_info = cb.source_info.clone();
                *block = Block::RawBlock(RawBlock {
                    format: "html".to_string(),
                    text,
                    source_info,
                });
            }
            Block::BlockQuote(bq) => convert_mermaid_blocks(&mut bq.content, found),
            Block::Div(div) => convert_mermaid_blocks(&mut div.content, found),
            Block::Figure(fig) => convert_mermaid_blocks(&mut fig.content, found),
            Block::OrderedList(list) => {
                for item in list.content.iter_mut() {
                    convert_mermaid_blocks(item, found);
                }
            }
            Block::BulletList(list) => {
                for item in list.content.iter_mut() {
                    convert_mermaid_blocks(item, found);
                }
            }
            Block::DefinitionList(dl) => {
                for (_term, defs) in dl.content.iter_mut() {
                    for def in defs.iter_mut() {
                        convert_mermaid_blocks(def, found);
                    }
                }
            }
            _ => {}
        }
    }
}

/// The once-per-document runtime loader.
///
/// A classic `<script src>` rather than a module import: the vendored
/// bundle assigns `globalThis.mermaid`, and classic scripts in the
/// after-body slot execute in document order, so the inline init below
/// is guaranteed to see it.
///
/// `mermaid.run()` is called explicitly (not `startOnLoad: true`) so
/// diagrams render regardless of when the script executes relative to
/// `DOMContentLoaded` — in embedded contexts (iframes, late injection)
/// load-event timing is not guaranteed. Every `pre.mermaid` element
/// precedes the after-body slot, so they are all parsed by this point.
fn mermaid_script_block(runtime_url: &str) -> String {
    format!(
        "{MERMAID_JS_SENTINEL}\n\
         <script src=\"{}\"></script>\n\
         <script>\n\
         mermaid.initialize({{ startOnLoad: false }});\n\
         mermaid.run({{ querySelector: 'pre.mermaid' }});\n\
         </script>",
        html_escape(runtime_url)
    )
}

/// HTML-escape diagram source for embedding as `<pre>` text content.
/// Same five-character escape as
/// [`CodeBlockRenderTransform`](super::CodeBlockRenderTransform)'s
/// filename escaping: mermaid source is user-controlled and flows
/// into a RawBlock the writer emits verbatim.
fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use crate::render::BinaryDependencies;
    use quarto_pandoc_types::ConfigValue;
    use quarto_pandoc_types::attr::AttrSourceInfo;
    use quarto_pandoc_types::block::{Block, CodeBlock, Div};
    use quarto_source_map::SourceInfo;

    fn make_codeblock(classes: &[&str], text: &str) -> Block {
        Block::CodeBlock(CodeBlock {
            attr: (
                String::new(),
                classes.iter().map(|c| c.to_string()).collect(),
                hashlink::LinkedHashMap::new(),
            ),
            text: text.to_string(),
            source_info: SourceInfo::for_test(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    fn make_test_project() -> ProjectContext {
        ProjectContext {
            dir: std::path::PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/doc.qmd")],
            output_dir: std::path::PathBuf::from("/project"),
        }
    }

    async fn run_transform(ast: &mut Pandoc, format: Format) {
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        MermaidRenderTransform::new()
            .transform(ast, &mut ctx)
            .await
            .unwrap();
    }

    /// Read `rendered.includes.after-body` as plain strings.
    fn after_body_includes(meta: &ConfigValue) -> Vec<String> {
        meta.get_path(&["rendered", "includes", "after-body"])
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn default_meta() -> ConfigValue {
        // A Map, as real documents always have (front matter). The
        // includes helper no-ops on non-map meta.
        ConfigValue::new_map(vec![], SourceInfo::for_test())
    }

    /// A `CodeBlock` with class `mermaid` becomes a
    /// `RawBlock("html", <pre class="mermaid">…</pre>)`, and the
    /// runtime script lands in `rendered.includes.after-body`.
    #[tokio::test]
    async fn mermaid_codeblock_becomes_pre_rawblock() {
        let mut ast = Pandoc {
            meta: default_meta(),
            blocks: vec![make_codeblock(&["mermaid"], "flowchart LR\n  a --> b")],
        };
        run_transform(&mut ast, Format::html()).await;

        assert_eq!(ast.blocks.len(), 1);
        let Block::RawBlock(raw) = &ast.blocks[0] else {
            panic!("expected RawBlock; got {:?}", ast.blocks[0]);
        };
        assert_eq!(raw.format, "html");
        assert_eq!(
            raw.text,
            "<pre class=\"mermaid\">\nflowchart LR\n  a --&gt; b\n</pre>"
        );

        let includes = after_body_includes(&ast.meta);
        assert_eq!(includes.len(), 1);
        let script = &includes[0];
        assert!(
            script.contains("mermaid.initialize({ startOnLoad: false })"),
            "script must initialize with startOnLoad false; got:\n{script}"
        );
        assert!(
            script.contains("mermaid.run({ querySelector: 'pre.mermaid' })"),
            "script must run explicitly on pre.mermaid; got:\n{script}"
        );
    }

    /// bd-mermaid-runtime-not-bundled-vxejw159: the emitted script must
    /// reference the bundled runtime by relative URL, never a CDN. This
    /// is the regression this strand exists to prevent.
    #[tokio::test]
    async fn script_references_bundled_runtime_not_cdn() {
        let mut ast = Pandoc {
            meta: default_meta(),
            blocks: vec![make_codeblock(&["mermaid"], "flowchart LR\n  a --> b")],
        };
        run_transform(&mut ast, Format::html()).await;

        let script = &after_body_includes(&ast.meta)[0];
        assert!(
            !script.contains("cdn.jsdelivr.net"),
            "script must not reference a CDN; got:\n{script}"
        );
        assert!(
            !script.contains("http://") && !script.contains("https://"),
            "script must not reference any absolute URL; got:\n{script}"
        );
        assert!(
            script.contains("<script src=\"mermaid/mermaid.min.js\"></script>"),
            "script must load the bundled runtime relatively; got:\n{script}"
        );
        assert!(
            !script.contains("type=\"module\""),
            "the vendored bundle is a classic script, not a module; got:\n{script}"
        );
    }

    /// With a resolver attached, the URL gets the right number of
    /// `../` hops for the page's depth. Nested-page behavior is what
    /// broke sites in the CDN-free design, so it is pinned here as
    /// well as end-to-end in `mermaid_bundling_pipeline.rs`.
    #[tokio::test]
    async fn runtime_url_is_page_relative() {
        use crate::resource_resolver::ResourceResolverContext;

        async fn url_for_page(site_root: &str, page_output: &str) -> String {
            let resolver =
                ResourceResolverContext::website(site_root, page_output, "site_libs", "doc");
            let project = make_test_project();
            let doc = DocumentInfo::from_path("/project/doc.qmd");
            let format = Format::html();
            let binaries = BinaryDependencies::new();
            let mut ctx = RenderContext::new(&project, &doc, &format, &binaries)
                .with_resource_resolver(resolver);
            let mut ast = Pandoc {
                meta: default_meta(),
                blocks: vec![make_codeblock(&["mermaid"], "flowchart LR\n  a --> b")],
            };
            MermaidRenderTransform::new()
                .transform(&mut ast, &mut ctx)
                .await
                .unwrap();
            after_body_includes(&ast.meta)[0].clone()
        }

        let root = url_for_page("/site", "/site/index.html").await;
        assert!(
            root.contains("src=\"site_libs/mermaid/mermaid.min.js\""),
            "root page must link without `../`; got:\n{root}"
        );

        let nested = url_for_page("/site", "/site/docs/api.html").await;
        assert!(
            nested.contains("src=\"../site_libs/mermaid/mermaid.min.js\""),
            "nested page must link with one `../`; got:\n{nested}"
        );

        let deep = url_for_page("/site", "/site/a/b/c/page.html").await;
        assert!(
            deep.contains("src=\"../../../site_libs/mermaid/mermaid.min.js\""),
            "3-deep page must link with three `../`; got:\n{deep}"
        );
    }

    /// The runtime is stored as a project-shared artifact at the
    /// `site_libs`-relative path, with the vendored bytes.
    #[test]
    fn register_mermaid_assets_stores_project_scoped_artifact() {
        let mut store = ArtifactStore::new();
        register_mermaid_assets(&mut store);

        let artifact = store
            .get(MERMAID_ARTIFACT_KEY)
            .expect("runtime artifact must be stored under the js: key");
        assert_eq!(
            artifact.path.as_deref(),
            Some(Path::new(MERMAID_ARTIFACT_PATH))
        );
        assert_eq!(artifact.scope, ArtifactScope::Project);
        assert_eq!(artifact.content_type, "text/javascript");
        assert_eq!(
            artifact.content.len(),
            MERMAID_JS.len(),
            "artifact must carry the full vendored bundle"
        );
    }

    /// Registering twice is idempotent — one entry, not two.
    #[test]
    fn register_mermaid_assets_is_idempotent() {
        let mut store = ArtifactStore::new();
        register_mermaid_assets(&mut store);
        register_mermaid_assets(&mut store);
        assert_eq!(store.get_by_prefix("mermaid:").len(), 1);
    }

    /// The runtime must NOT be keyed under `js:`. `ApplyTemplateStage`
    /// turns every `js:` artifact into a `<script src>` in the
    /// Bootstrap template head, which would both double-load the 2.6
    /// MiB bundle and — because the reveal scaffold collects only
    /// `js:revealjs:*` — still leave presentations without a tag. See
    /// the `MERMAID_ARTIFACT_KEY` docs.
    #[test]
    fn runtime_is_not_keyed_as_a_template_script() {
        let mut store = ArtifactStore::new();
        register_mermaid_assets(&mut store);
        assert!(
            store.get_by_prefix("js:").is_empty(),
            "runtime must not use the `js:` prefix; ApplyTemplateStage would emit a \
             duplicate head <script> for it"
        );
    }

    /// A document with a diagram registers the runtime through the
    /// transform (i.e. the transform, not just the helper, wires it up).
    #[tokio::test]
    async fn transform_registers_runtime_when_diagram_present() {
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        let mut ast = Pandoc {
            meta: default_meta(),
            blocks: vec![make_codeblock(&["mermaid"], "flowchart LR\n  a --> b")],
        };
        MermaidRenderTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();

        assert!(
            ctx.artifacts.get(MERMAID_ARTIFACT_KEY).is_some(),
            "transform must register the runtime artifact"
        );
    }

    /// A diagram-free document registers nothing — no 2.6 MiB asset
    /// written for documents that never use it.
    #[tokio::test]
    async fn transform_registers_nothing_without_diagrams() {
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        let mut ast = Pandoc {
            meta: default_meta(),
            blocks: vec![make_codeblock(&["python"], "print('hi')")],
        };
        MermaidRenderTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();

        assert!(
            ctx.artifacts.get(MERMAID_ARTIFACT_KEY).is_none(),
            "diagram-free document must not register the runtime"
        );
    }

    /// Non-HTML formats register nothing either (the transform
    /// self-gates before doing any work).
    #[tokio::test]
    async fn transform_registers_nothing_for_non_html() {
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::pdf();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        let mut ast = Pandoc {
            meta: default_meta(),
            blocks: vec![make_codeblock(&["mermaid"], "flowchart LR\n  a --> b")],
        };
        MermaidRenderTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();

        assert!(ctx.artifacts.get(MERMAID_ARTIFACT_KEY).is_none());
    }

    // ── Guards on the vendored bundle ────────────────────────────────
    //
    // See `resources/mermaid/README.md`. These are cheap, always-on
    // invariants that catch the two ways re-vendoring goes wrong.

    /// The bundle's own embedded version must match `MERMAID_VERSION`.
    /// Catches a `MERMAID_VERSION` bump without a re-vendor (or the
    /// reverse). The `version:"…"` string occurs exactly once in the
    /// bundle, so this is an unambiguous anchor.
    #[test]
    fn vendored_bundle_matches_pinned_version() {
        let needle = format!("version:\"{MERMAID_VERSION}\"");
        assert!(
            MERMAID_JS.contains(&needle),
            "vendored resources/mermaid/mermaid.min.js does not embed {needle} — \
             MERMAID_VERSION and the vendored file disagree. Re-vendor the bundle \
             (see resources/mermaid/README.md § Updating)."
        );
    }

    /// The bundle must be self-contained: no dynamic `import(` and no
    /// reference to the `chunks/` tree.
    ///
    /// This is the trap that motivated the strand. `dist/mermaid.esm.min.mjs`
    /// looks like "the mermaid runtime" but is a ~26 KB stub that lazily
    /// imports a chunk **per diagram type** — vendoring it would leave
    /// offline rendering broken in a way a flowchart-only test would miss.
    #[test]
    fn vendored_bundle_is_self_contained() {
        assert!(
            !MERMAID_JS.contains("import("),
            "vendored bundle contains a dynamic import — it is not self-contained. \
             Vendor dist/mermaid.min.js, not the ESM build (see resources/mermaid/README.md)."
        );
        assert!(
            !MERMAID_JS.contains("chunks/"),
            "vendored bundle references the chunks/ tree — it is not self-contained. \
             Vendor dist/mermaid.min.js, not the ESM build (see resources/mermaid/README.md)."
        );
        assert!(
            MERMAID_JS.contains("globalThis[\"mermaid\"]"),
            "vendored bundle must assign globalThis.mermaid; the emitted \
             <script src> + inline init depends on that global."
        );
    }

    /// Reveal-style drift check against the npm package, for when
    /// mermaid is present in `node_modules`.
    ///
    /// mermaid is deliberately *not* a dependency of this repo (66 MB
    /// unpacked, and nothing here imports it — the preview component
    /// still uses a CDN, see bd-1vwtdwtq), so this normally skips. It
    /// starts working automatically if a future change adds mermaid to
    /// the npm graph, at which point vendored and npm copies must not
    /// drift. Mirrors `vendored_reveal_assets_match_npm_package`.
    #[test]
    fn vendored_bundle_matches_npm_package() {
        let npm_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../node_modules/mermaid/dist/mermaid.min.js"
        );
        if !Path::new(npm_path).exists() {
            eprintln!(
                "skipping mermaid vendoring check: {npm_path} absent \
                 (mermaid is not an npm dependency of this repo; see bd-1vwtdwtq)"
            );
            return;
        }
        // Content compare with line endings normalized, for the same
        // reason the reveal check does it: a Windows checkout may
        // rewrite the committed-LF vendored file to CRLF, and that is
        // irrelevant to every browser. We want content drift only.
        let norm = |s: &str| s.replace("\r\n", "\n");
        let npm =
            std::fs::read_to_string(npm_path).unwrap_or_else(|e| panic!("reading {npm_path}: {e}"));
        assert_eq!(
            norm(MERMAID_JS),
            norm(&npm),
            "vendored resources/mermaid/mermaid.min.js has drifted from \
             node_modules/mermaid/dist/mermaid.min.js — re-sync the vendored copy \
             (see resources/mermaid/README.md § Updating)"
        );
    }

    /// Two diagrams → two `<pre>` blocks, but the script is appended
    /// exactly once.
    #[tokio::test]
    async fn script_appended_once_for_multiple_diagrams() {
        let mut ast = Pandoc {
            meta: default_meta(),
            blocks: vec![
                make_codeblock(&["mermaid"], "flowchart LR\n  a --> b"),
                make_codeblock(&["mermaid"], "sequenceDiagram\n  A->>B: hi"),
            ],
        };
        run_transform(&mut ast, Format::html()).await;

        assert!(
            matches!(&ast.blocks[0], Block::RawBlock(r) if r.text.contains("pre class=\"mermaid\""))
        );
        assert!(
            matches!(&ast.blocks[1], Block::RawBlock(r) if r.text.contains("pre class=\"mermaid\""))
        );
        assert_eq!(after_body_includes(&ast.meta).len(), 1);
    }

    /// Re-running the transform on already-transformed output must not
    /// duplicate the script (sentinel dedup, mirroring the
    /// attribution-viewer contract).
    #[tokio::test]
    async fn rerun_does_not_duplicate_script() {
        let mut ast = Pandoc {
            meta: default_meta(),
            blocks: vec![make_codeblock(&["mermaid"], "flowchart LR\n  a --> b")],
        };
        run_transform(&mut ast, Format::html()).await;
        // Second run: the block is now a RawBlock (no CodeBlock to
        // match), but even a doc that still contains a mermaid block
        // must not gain a second script.
        ast.blocks
            .push(make_codeblock(&["mermaid"], "flowchart TD\n  x --> y"));
        run_transform(&mut ast, Format::html()).await;
        assert_eq!(after_body_includes(&ast.meta).len(), 1);
    }

    /// Documents without mermaid blocks: AST untouched, no script.
    #[tokio::test]
    async fn no_mermaid_no_script_no_change() {
        let mut ast = Pandoc {
            meta: default_meta(),
            blocks: vec![make_codeblock(&["python"], "print('hi')")],
        };
        run_transform(&mut ast, Format::html()).await;

        assert!(
            matches!(&ast.blocks[0], Block::CodeBlock(cb) if cb.text == "print('hi')"),
            "non-mermaid code block must pass through; got {:?}",
            ast.blocks[0]
        );
        assert!(after_body_includes(&ast.meta).is_empty());
    }

    /// The brace form `{mermaid}` is engine territory (knitr claims
    /// it) and must NOT be matched. pampa parses ` ```{mermaid} ` to a
    /// class list containing `{mermaid}`, not `mermaid`.
    #[tokio::test]
    async fn brace_form_mermaid_cell_untouched() {
        let mut ast = Pandoc {
            meta: default_meta(),
            blocks: vec![make_codeblock(&["{mermaid}"], "flowchart LR\n  a --> b")],
        };
        run_transform(&mut ast, Format::html()).await;

        assert!(
            matches!(&ast.blocks[0], Block::CodeBlock(_)),
            "brace-form cell must pass through; got {:?}",
            ast.blocks[0]
        );
        assert!(after_body_includes(&ast.meta).is_empty());
    }

    /// Diagram source is HTML-escaped: `&`, `<`, `>`.
    #[tokio::test]
    async fn diagram_source_is_html_escaped() {
        let mut ast = Pandoc {
            meta: default_meta(),
            blocks: vec![make_codeblock(
                &["mermaid"],
                "flowchart LR\n  a[\"<b> & </b>\"] --> b",
            )],
        };
        run_transform(&mut ast, Format::html()).await;

        let Block::RawBlock(raw) = &ast.blocks[0] else {
            panic!("expected RawBlock");
        };
        assert!(
            raw.text
                .contains("a[&quot;&lt;b&gt; &amp; &lt;/b&gt;&quot;] --&gt; b"),
            "source must be escaped; got:\n{}",
            raw.text
        );
        assert!(
            !raw.text.contains("<b>"),
            "raw <b> must not survive; got:\n{}",
            raw.text
        );
    }

    /// A mermaid block nested inside a Div (e.g. a `::: {.column}`
    /// container or a reveal slide section) is converted too.
    #[tokio::test]
    async fn nested_mermaid_block_is_converted() {
        let inner = make_codeblock(&["mermaid"], "flowchart LR\n  a --> b");
        let mut ast = Pandoc {
            meta: default_meta(),
            blocks: vec![Block::Div(Div {
                attr: (
                    String::new(),
                    vec!["section".to_string()],
                    hashlink::LinkedHashMap::new(),
                ),
                content: vec![inner],
                source_info: SourceInfo::for_test(),
                attr_source: AttrSourceInfo::empty(),
            })],
        };
        run_transform(&mut ast, Format::html()).await;

        let Block::Div(div) = &ast.blocks[0] else {
            panic!("expected Div wrapper to survive");
        };
        assert!(
            matches!(&div.content[0], Block::RawBlock(r) if r.text.contains("pre class=\"mermaid\"")),
            "nested mermaid block must be converted; got {:?}",
            div.content[0]
        );
        assert_eq!(after_body_includes(&ast.meta).len(), 1);
    }

    /// A code block whose class list includes `mermaid` among others
    /// (e.g. ` ```{.mermaid .extra} `) is still converted.
    #[tokio::test]
    async fn mermaid_with_extra_classes_is_converted() {
        let mut ast = Pandoc {
            meta: default_meta(),
            blocks: vec![make_codeblock(
                &["mermaid", "extra"],
                "flowchart LR\n  a --> b",
            )],
        };
        run_transform(&mut ast, Format::html()).await;
        assert!(matches!(&ast.blocks[0], Block::RawBlock(_)));
    }

    /// Non-HTML-family formats: transform is a no-op (self-gated).
    #[tokio::test]
    async fn non_html_format_is_untouched() {
        let mut ast = Pandoc {
            meta: default_meta(),
            blocks: vec![make_codeblock(&["mermaid"], "flowchart LR\n  a --> b")],
        };
        run_transform(&mut ast, Format::pdf()).await;

        assert!(
            matches!(&ast.blocks[0], Block::CodeBlock(_)),
            "non-html format must leave the block alone; got {:?}",
            ast.blocks[0]
        );
        assert!(after_body_includes(&ast.meta).is_empty());
    }
}
