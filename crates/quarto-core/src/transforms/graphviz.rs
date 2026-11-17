/*
 * transforms/graphviz.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Render ` ```dot ` fenced code blocks as Graphviz diagrams for
//! HTML-family output (`format: html`, `format: revealjs`, dashboards).
//!
//! A plain fenced block with the `dot` class — pampa parses ` ```dot `
//! to `CodeBlock(("", ["dot"], []), source)` — is replaced with
//! `RawBlock("html", "<pre class=\"graphviz\">…</pre>")` (source
//! HTML-escaped). When at least one diagram was found, the vendored
//! Graphviz runtime is registered as a project-shared artifact and a
//! once-per-document `<script src="…">` + init pair is appended to the
//! canonical `rendered.includes.after-body` list. The init lays out
//! every `pre.graphviz` in the reader's browser and swaps the source
//! text for the resulting inline `<svg>`.
//!
//! This follows [`MermaidRenderTransform`](super::MermaidRenderTransform)
//! piece by piece; read that module's docs for the reasoning shared by
//! both (not an engine, Finalization phase, the after-body channel, the
//! non-`js:` artifact key). Design notes specific to Graphviz
//! (bd-ijlapt70, plan `claude-notes/plans/2026-11-13-graphviz-support.md`):
//!
//! - **Client-side for the HTML family.** Graphviz does not need a DOM,
//!   so it could be laid out at render time, as Quarto 1 does. The first
//!   cut is client-side because the browser runtime is needed for
//!   `q2 preview` and QuartoHub in any case. A render-time SVG path for
//!   Typst and the other formats is tracked as bd-atz1c25y.
//! - **Excluded from the q2-preview pipeline**
//!   (`Q2_PREVIEW_TRANSFORM_EXCLUDED`): the raw `CodeBlock` must reach
//!   the React layer, where `GraphvizCodeBlock.tsx`
//!   (ts-packages/preview-renderer) owns rendering.
//! - **`{dot}` executable cells are deliberately NOT recognized**,
//!   following the mermaid precedent. `Q-2-58` tells the author to drop
//!   the braces.
//! - **`graph-layout`** selects the layout engine. It reaches this
//!   transform as a code-block attribute and is emitted as
//!   `data-graph-layout`, allow-listed against the engines the pinned
//!   build ships.
//! - **Errors show in the page.** A Graphviz syntax error cannot fail
//!   the render, because layout happens in the browser. The init leaves
//!   the source in place, marks the `<pre>` with `graphviz-error`, and
//!   puts the message above it.
//!
//! # Bundling
//!
//! The runtime is `resources/graphviz/viz-global.js` (Viz.js, a
//! WebAssembly build of Graphviz), embedded with `include_str!` and
//! registered as an [`ArtifactScope::Project`] artifact at
//! `graphviz/viz-global.js`. See `resources/graphviz/README.md` for why
//! that build and not the ES module, and for the two properties guarded
//! by tests here: the pinned versions, and self-containment.

use std::path::Path;

use quarto_pandoc_types::block::{Block, Blocks, RawBlock};
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::artifact::{Artifact, ArtifactScope, ArtifactStore};
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};
use crate::transforms::append_with_sentinel;

/// Exact-pinned Viz.js version. The TS preview component pins the same
/// version in
/// `ts-packages/preview-renderer/src/q2-preview/blocks/GraphvizCodeBlock.tsx`;
/// bump both together (see `resources/graphviz/README.md` § Updating).
///
/// Must equal the version named in the vendored bundle's header —
/// enforced by `vendored_bundle_matches_pinned_version`.
pub const VIZ_VERSION: &str = "3.31.0";

/// The Graphviz version that [`VIZ_VERSION`] builds. Recorded because
/// it, not the wrapper version, decides how a diagram is laid out.
pub const GRAPHVIZ_VERSION: &str = "16.1.0";

/// The vendored Graphviz runtime, embedded at compile time: the
/// self-contained classic-script build, `dist/viz-global.js`.
const GRAPHVIZ_JS: &str = include_str!("../../../../resources/graphviz/viz-global.js");

/// Destination path for the runtime, relative to the project lib dir.
const GRAPHVIZ_ARTIFACT_PATH: &str = "graphviz/viz-global.js";

/// Artifact store key. Deliberately not a `js:` key, for the reasons
/// given on `MERMAID_ARTIFACT_KEY`: a `js:` key would add a second
/// `<script>` in the Bootstrap `<head>` and would produce no tag at all
/// in a reveal deck.
const GRAPHVIZ_ARTIFACT_KEY: &str = "graphviz:runtime";

/// HTML-comment sentinel embedded in the injected `<script>` block so a
/// transform re-run is idempotent.
const GRAPHVIZ_JS_SENTINEL: &str = "<!-- quarto-graphviz-js -->";

/// Layout engines in the pinned build (`engines` in `@viz-js/viz`).
/// Kept in step with `ENGINES` in `GraphvizCodeBlock.tsx`.
pub(crate) const LAYOUT_ENGINES: &[&str] = &[
    "circo",
    "dot",
    "fdp",
    "neato",
    "nop",
    "nop1",
    "nop2",
    "osage",
    "patchwork",
    "sfdp",
    "twopi",
];

/// See module docs.
pub struct GraphvizRenderTransform;

impl GraphvizRenderTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GraphvizRenderTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for GraphvizRenderTransform {
    fn name(&self) -> &str {
        "graphviz-render"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Finalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        // Self-gate: the runtime is a browser script, so only
        // HTML-family output can host it. Other formats keep the code
        // block until the render-time SVG path exists (bd-atz1c25y).
        if !ctx.format.identifier.is_html_based() {
            return Ok(());
        }

        let mut found = false;
        convert_graphviz_blocks(&mut ast.blocks, &mut found);
        if found {
            // Register before computing the URL: the artifact carries
            // the destination path, the resolver turns it into a
            // page-relative href. Diagram-free documents reach neither
            // line, so they ship no runtime and no script.
            register_graphviz_assets(&mut ctx.artifacts);
            let runtime_url = graphviz_runtime_url(ctx);
            append_with_sentinel(
                &mut ast.meta,
                "after-body",
                GRAPHVIZ_JS_SENTINEL,
                graphviz_script_block(&runtime_url),
            );
        }
        Ok(())
    }
}

/// Store the vendored runtime as a project-shared artifact. A fixed key
/// makes repeat calls idempotent.
pub fn register_graphviz_assets(artifacts: &mut ArtifactStore) {
    artifacts.store(
        GRAPHVIZ_ARTIFACT_KEY,
        Artifact::from_string(GRAPHVIZ_JS, "text/javascript")
            .with_path(GRAPHVIZ_ARTIFACT_PATH)
            .with_scope(ArtifactScope::Project),
    );
}

/// Page-relative URL for the runtime (see `mermaid_runtime_url`).
fn graphviz_runtime_url(ctx: &RenderContext) -> String {
    match &ctx.resource_resolver {
        Some(resolver) => {
            resolver.html_url_for(ArtifactScope::Project, Path::new(GRAPHVIZ_ARTIFACT_PATH))
        }
        None => GRAPHVIZ_ARTIFACT_PATH.to_string(),
    }
}

/// The `<pre>` opening tag for one diagram.
///
/// Three code-block attributes shape it. Each is allow-listed or
/// escaped here, because the attribute form
/// (`{.dot graph-layout=...}`) reaches this transform without going
/// through any cell-option validation:
///
/// - `fig-align` still on the block means no figure wrapper harvested
///   it — a bare diagram. It becomes a `quarto-figure-<align>` class.
/// - `graph-layout` becomes `data-graph-layout`.
/// - `fig-alt` becomes `data-fig-alt`.
fn pre_open_tag(attrs: &hashlink::LinkedHashMap<String, String>) -> String {
    let mut tag = String::from("<pre class=\"graphviz");
    if let Some(align @ ("left" | "center" | "right")) = attrs.get("fig-align").map(String::as_str)
    {
        tag.push_str(" quarto-figure-");
        tag.push_str(align);
    }
    tag.push('"');
    if let Some(engine) = attrs.get("graph-layout")
        && LAYOUT_ENGINES.contains(&engine.as_str())
    {
        tag.push_str(" data-graph-layout=\"");
        tag.push_str(engine);
        tag.push('"');
    }
    if let Some(alt) = attrs.get("fig-alt") {
        tag.push_str(" data-fig-alt=\"");
        tag.push_str(&html_escape(alt));
        tag.push('"');
    }
    tag.push('>');
    tag
}

/// Walk `blocks`, replacing each `CodeBlock` carrying the `dot` class
/// with the `<pre class="graphviz">` RawBlock. Descends into the same
/// container variants as the mermaid walker, so nested diagrams
/// (columns, slide sections, list items, figure wrappers) convert too.
fn convert_graphviz_blocks(blocks: &mut Blocks, found: &mut bool) {
    for block in blocks.iter_mut() {
        match block {
            Block::CodeBlock(cb) if cb.attr.1.iter().any(|c| c == "dot") => {
                *found = true;
                let text = format!(
                    "{}\n{}\n</pre>",
                    pre_open_tag(&cb.attr.2),
                    html_escape(&cb.text)
                );
                let source_info = cb.source_info.clone();
                *block = Block::RawBlock(Box::new(RawBlock {
                    format: "html".to_string(),
                    text,
                    source_info,
                }));
            }
            Block::BlockQuote(bq) => convert_graphviz_blocks(&mut bq.content, found),
            Block::Div(div) => convert_graphviz_blocks(&mut div.content, found),
            Block::Figure(fig) => convert_graphviz_blocks(&mut fig.content, found),
            Block::OrderedList(list) => {
                for item in list.content.iter_mut() {
                    convert_graphviz_blocks(item, found);
                }
            }
            Block::BulletList(list) => {
                for item in list.content.iter_mut() {
                    convert_graphviz_blocks(item, found);
                }
            }
            Block::DefinitionList(dl) => {
                for (_term, defs) in dl.content.iter_mut() {
                    for def in defs.iter_mut() {
                        convert_graphviz_blocks(def, found);
                    }
                }
            }
            _ => {}
        }
    }
}

/// The inline init, run once the runtime has loaded.
///
/// For every `pre.graphviz` it lays the source out and swaps the text
/// for the inline `<svg>`. The SVG is parsed with `DOMParser` and
/// imported, not assigned to `innerHTML`. The `<pre>` stays as the
/// container (as mermaid.js keeps `pre.mermaid`), so the alignment
/// rules and any figure wrapper keep applying.
///
/// - `data-graph-layout` picks the engine; the default is `dot`.
/// - `data-fig-alt` becomes the SVG's accessible name. Without it the
///   SVG keeps the `<title>` Graphviz writes (the graph's name).
/// - A failed layout leaves the source visible, adds the
///   `graphviz-error` class, and puts Graphviz's message above the
///   source. Layout does not need the element to be visible, so
///   diagrams on hidden reveal slides are drawn too.
const GRAPHVIZ_INIT_JS: &str = r#"Viz.instance().then(function (viz) {
  document.querySelectorAll('pre.graphviz').forEach(function (pre) {
    var source = pre.textContent;
    var result = viz.render(source, { format: 'svg', engine: pre.dataset.graphLayout || 'dot' });
    var svg = null;
    if (result.status === 'success') {
      svg = new DOMParser().parseFromString(result.output, 'image/svg+xml').documentElement;
    }
    if (!svg || svg.localName !== 'svg') {
      var message = result.errors.map(function (e) { return e.message.trim(); }).join('; ');
      var note = document.createElement('strong');
      note.textContent = 'Graphviz error: ' + (message || 'could not lay out this diagram') + '\n';
      pre.classList.add('graphviz-error');
      pre.insertBefore(note, pre.firstChild);
      return;
    }
    svg.style.maxWidth = '100%';
    svg.style.height = 'auto';
    if (pre.dataset.figAlt) {
      svg.setAttribute('role', 'img');
      svg.setAttribute('aria-label', pre.dataset.figAlt);
    }
    pre.replaceChildren(document.importNode(svg, true));
  });
});"#;

/// The once-per-document runtime loader.
///
/// A classic `<script src>` rather than a module import: the vendored
/// bundle assigns `globalThis.Viz`, and classic scripts in the
/// after-body slot execute in document order, so the inline init is
/// guaranteed to see it, and the page works when opened from disk.
/// Every `pre.graphviz` precedes the after-body slot, so they are all
/// parsed by this point.
///
/// `charset="utf-8"` because the bundle stores its WebAssembly binary
/// as raw non-ASCII characters; decoded in any other encoding the
/// binary is corrupt (see `resources/graphviz/README.md`).
fn graphviz_script_block(runtime_url: &str) -> String {
    format!(
        "{GRAPHVIZ_JS_SENTINEL}\n\
         <script src=\"{}\" charset=\"utf-8\"></script>\n\
         <script>\n\
         {GRAPHVIZ_INIT_JS}\n\
         </script>",
        html_escape(runtime_url)
    )
}

/// HTML-escape diagram source (or an attribute value) for embedding in
/// a RawBlock the writer emits verbatim.
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

    const DIGRAPH: &str = "digraph G {\n  a -> b\n}";

    fn make_codeblock(classes: &[&str], text: &str) -> Block {
        make_codeblock_with_attrs(classes, text, &[])
    }

    fn make_codeblock_with_attrs(classes: &[&str], text: &str, attrs: &[(&str, &str)]) -> Block {
        let mut map = hashlink::LinkedHashMap::new();
        for (k, v) in attrs {
            map.insert(k.to_string(), v.to_string());
        }
        Block::CodeBlock(Box::new(CodeBlock {
            attr: (
                String::new(),
                classes.iter().map(|c| c.to_string()).collect(),
                map,
            )
                .into(),
            text: text.to_string(),
            source_info: SourceInfo::for_test(),
            attr_source: AttrSourceInfo::empty(),
        }))
    }

    fn make_test_project() -> ProjectContext {
        ProjectContext {
            dir: std::path::PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/doc.qmd")],
            output_dir: std::path::PathBuf::from("/project"),
            ..Default::default()
        }
    }

    async fn run_transform(ast: &mut Pandoc, format: Format) {
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        GraphvizRenderTransform::new()
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
        ConfigValue::new_map(vec![], SourceInfo::for_test())
    }

    fn doc(blocks: Vec<Block>) -> Pandoc {
        Pandoc {
            meta: default_meta(),
            blocks,
        }
    }

    /// The `<pre …>` opening tag the walker emits for one block.
    fn pre_tag(block: Block) -> String {
        let mut blocks = vec![block];
        let mut found = false;
        convert_graphviz_blocks(&mut blocks, &mut found);
        let Block::RawBlock(raw) = &blocks[0] else {
            panic!("expected RawBlock; got {:?}", blocks[0]);
        };
        raw.text.lines().next().unwrap().to_string()
    }

    /// A `CodeBlock` with class `dot` becomes a
    /// `RawBlock("html", <pre class="graphviz">…</pre>)`, and the
    /// runtime script lands in `rendered.includes.after-body`.
    #[tokio::test]
    async fn dot_codeblock_becomes_pre_rawblock() {
        let mut ast = doc(vec![make_codeblock(&["dot"], DIGRAPH)]);
        run_transform(&mut ast, Format::html()).await;

        assert_eq!(ast.blocks.len(), 1);
        let Block::RawBlock(raw) = &ast.blocks[0] else {
            panic!("expected RawBlock; got {:?}", ast.blocks[0]);
        };
        assert_eq!(raw.format, "html");
        assert_eq!(
            raw.text,
            "<pre class=\"graphviz\">\ndigraph G {\n  a -&gt; b\n}\n</pre>"
        );

        let includes = after_body_includes(&ast.meta);
        assert_eq!(includes.len(), 1);
        let script = &includes[0];
        assert!(
            script.contains("Viz.instance()"),
            "script must create the Viz instance; got:\n{script}"
        );
        assert!(
            script.contains("querySelectorAll('pre.graphviz')"),
            "script must lay out every pre.graphviz; got:\n{script}"
        );
    }

    /// The emitted script must reference the bundled runtime by
    /// relative URL, as a classic script, and never a CDN.
    #[tokio::test]
    async fn script_references_bundled_runtime_not_cdn() {
        let mut ast = doc(vec![make_codeblock(&["dot"], DIGRAPH)]);
        run_transform(&mut ast, Format::html()).await;

        let script = &after_body_includes(&ast.meta)[0];
        assert!(
            !script.contains("http://") && !script.contains("https://"),
            "script must not reference any absolute URL; got:\n{script}"
        );
        assert!(
            script.contains("<script src=\"graphviz/viz-global.js\" charset=\"utf-8\"></script>"),
            "script must load the bundled runtime relatively, decoded as UTF-8; got:\n{script}"
        );
        assert!(
            !script.contains("type=\"module\""),
            "the vendored bundle is a classic script, not a module; got:\n{script}"
        );
    }

    /// With a resolver attached, the URL gets the right number of
    /// `../` hops for the page's depth.
    #[tokio::test]
    async fn runtime_url_is_page_relative() {
        use crate::resource_resolver::ResourceResolverContext;

        async fn url_for_page(site_root: &str, page_output: &str) -> String {
            let resolver =
                ResourceResolverContext::website(site_root, page_output, "site_libs", "doc");
            let project = make_test_project();
            let doc_info = DocumentInfo::from_path("/project/doc.qmd");
            let format = Format::html();
            let binaries = BinaryDependencies::new();
            let mut ctx = RenderContext::new(&project, &doc_info, &format, &binaries)
                .with_resource_resolver(resolver);
            let mut ast = doc(vec![make_codeblock(&["dot"], DIGRAPH)]);
            GraphvizRenderTransform::new()
                .transform(&mut ast, &mut ctx)
                .await
                .unwrap();
            after_body_includes(&ast.meta)[0].clone()
        }

        let root = url_for_page("/site", "/site/index.html").await;
        assert!(
            root.contains("src=\"site_libs/graphviz/viz-global.js\""),
            "root page must link without `../`; got:\n{root}"
        );

        let nested = url_for_page("/site", "/site/docs/api.html").await;
        assert!(
            nested.contains("src=\"../site_libs/graphviz/viz-global.js\""),
            "nested page must link with one `../`; got:\n{nested}"
        );
    }

    /// The runtime is stored as a project-shared artifact at the
    /// `site_libs`-relative path, with the vendored bytes.
    #[test]
    fn register_graphviz_assets_stores_project_scoped_artifact() {
        let mut store = ArtifactStore::new();
        register_graphviz_assets(&mut store);

        let artifact = store
            .get(GRAPHVIZ_ARTIFACT_KEY)
            .expect("runtime artifact must be stored");
        assert_eq!(
            artifact.path.as_deref(),
            Some(Path::new(GRAPHVIZ_ARTIFACT_PATH))
        );
        assert_eq!(artifact.scope, ArtifactScope::Project);
        assert_eq!(artifact.content_type, "text/javascript");
        assert_eq!(
            artifact.content.len(),
            GRAPHVIZ_JS.len(),
            "artifact must carry the full vendored bundle"
        );
    }

    /// Registering twice is idempotent — one entry, not two.
    #[test]
    fn register_graphviz_assets_is_idempotent() {
        let mut store = ArtifactStore::new();
        register_graphviz_assets(&mut store);
        register_graphviz_assets(&mut store);
        assert_eq!(store.get_by_prefix("graphviz:").len(), 1);
    }

    /// The runtime must NOT be keyed under `js:` (see
    /// `GRAPHVIZ_ARTIFACT_KEY`).
    #[test]
    fn runtime_is_not_keyed_as_a_template_script() {
        let mut store = ArtifactStore::new();
        register_graphviz_assets(&mut store);
        assert_eq!(store.get_by_prefix("graphviz:").len(), 1);
        assert!(
            store.get_by_prefix("js:").is_empty(),
            "runtime must not use the `js:` prefix; ApplyTemplateStage would emit a \
             duplicate head <script> for it"
        );
    }

    async fn artifact_registered_for(format: Format, block: Block) -> bool {
        let project = make_test_project();
        let doc_info = DocumentInfo::from_path("/project/doc.qmd");
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc_info, &format, &binaries);
        let mut ast = doc(vec![block]);
        GraphvizRenderTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        ctx.artifacts.get(GRAPHVIZ_ARTIFACT_KEY).is_some()
    }

    /// The transform (not just the helper) registers the runtime when a
    /// diagram is present, and only then.
    #[tokio::test]
    async fn transform_registers_runtime_only_when_needed() {
        assert!(
            artifact_registered_for(Format::html(), make_codeblock(&["dot"], DIGRAPH)).await,
            "transform must register the runtime artifact"
        );
        assert!(
            !artifact_registered_for(Format::html(), make_codeblock(&["python"], "print('hi')"))
                .await,
            "diagram-free document must not register the runtime"
        );
        assert!(
            !artifact_registered_for(Format::pdf(), make_codeblock(&["dot"], DIGRAPH)).await,
            "non-HTML format must not register the runtime"
        );
    }

    // ── Guards on the vendored bundle ────────────────────────────────
    //
    // See `resources/graphviz/README.md`.

    /// The bundle's header names the Viz.js version, and it embeds the
    /// Graphviz version it builds. Both must match the constants.
    #[test]
    fn vendored_bundle_matches_pinned_version() {
        let viz = format!("Viz.js {VIZ_VERSION}\n");
        assert!(
            GRAPHVIZ_JS.contains(&viz),
            "vendored resources/graphviz/viz-global.js does not name `Viz.js {VIZ_VERSION}` \
             in its header — VIZ_VERSION and the vendored file disagree. Re-vendor the \
             bundle (see resources/graphviz/README.md § Updating)."
        );
        let graphviz = format!("graphvizVersion=\"{GRAPHVIZ_VERSION}\"");
        assert!(
            GRAPHVIZ_JS.contains(&graphviz),
            "vendored bundle does not embed {graphviz} — GRAPHVIZ_VERSION and the vendored \
             file disagree (see resources/graphviz/README.md § Updating)."
        );
    }

    /// The bundle must be self-contained and must assign the `Viz`
    /// global. `fetch(` is deliberately not forbidden: the bundle
    /// carries an Emscripten fallback that never runs (see the README).
    #[test]
    fn vendored_bundle_is_self_contained() {
        assert!(
            !GRAPHVIZ_JS.contains("import("),
            "vendored bundle contains a dynamic import — it is not self-contained. \
             Vendor dist/viz-global.js (see resources/graphviz/README.md)."
        );
        assert!(
            !GRAPHVIZ_JS.contains(".wasm"),
            "vendored bundle references a separate .wasm file — it is not self-contained. \
             Vendor dist/viz-global.js (see resources/graphviz/README.md)."
        );
        assert!(
            GRAPHVIZ_JS.contains(".Viz={}"),
            "vendored bundle must assign the `Viz` global; the emitted <script src> + \
             inline init depends on it. Vendor dist/viz-global.js, not the ES module."
        );
    }

    /// Drift check against the npm package. `@viz-js/viz` is an npm
    /// dependency of the workspace (the preview component imports it),
    /// so this runs whenever `npm install` has been run, and skips on a
    /// checkout without `node_modules`.
    ///
    /// Byte-for-byte, with no line-ending normalization: the bundle
    /// stores a WebAssembly binary as raw characters, and
    /// `.gitattributes` marks it `-text` so no checkout rewrites it.
    #[test]
    fn vendored_bundle_matches_npm_package() {
        let npm_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../node_modules/@viz-js/viz/dist/viz-global.js"
        );
        if !Path::new(npm_path).exists() {
            eprintln!("skipping Graphviz vendoring check: {npm_path} absent (run `npm install`)");
            return;
        }
        let npm =
            std::fs::read_to_string(npm_path).unwrap_or_else(|e| panic!("reading {npm_path}: {e}"));
        assert!(
            GRAPHVIZ_JS == npm,
            "vendored resources/graphviz/viz-global.js has drifted from \
             node_modules/@viz-js/viz/dist/viz-global.js — re-sync the vendored copy and the \
             npm pin together (see resources/graphviz/README.md § Updating)"
        );
    }

    /// Two diagrams → two `<pre>` blocks, but the script is appended
    /// exactly once.
    #[tokio::test]
    async fn script_appended_once_for_multiple_diagrams() {
        let mut ast = doc(vec![
            make_codeblock(&["dot"], DIGRAPH),
            make_codeblock(&["dot"], "graph H { x -- y }"),
        ]);
        run_transform(&mut ast, Format::html()).await;

        for block in &ast.blocks {
            assert!(
                matches!(block, Block::RawBlock(r) if r.text.contains("pre class=\"graphviz\""))
            );
        }
        assert_eq!(after_body_includes(&ast.meta).len(), 1);
    }

    /// Re-running the transform must not duplicate the script.
    #[tokio::test]
    async fn rerun_does_not_duplicate_script() {
        let mut ast = doc(vec![make_codeblock(&["dot"], DIGRAPH)]);
        run_transform(&mut ast, Format::html()).await;
        ast.blocks
            .push(make_codeblock(&["dot"], "graph H { x -- y }"));
        run_transform(&mut ast, Format::html()).await;
        assert_eq!(after_body_includes(&ast.meta).len(), 1);
    }

    /// Documents without dot blocks: AST untouched, no script.
    #[tokio::test]
    async fn no_dot_no_script_no_change() {
        let mut ast = doc(vec![make_codeblock(&["python"], "print('hi')")]);
        run_transform(&mut ast, Format::html()).await;

        assert!(
            matches!(&ast.blocks[0], Block::CodeBlock(cb) if cb.text == "print('hi')"),
            "non-dot code block must pass through; got {:?}",
            ast.blocks[0]
        );
        assert!(after_body_includes(&ast.meta).is_empty());
    }

    /// A mermaid block is not this transform's business.
    #[tokio::test]
    async fn mermaid_block_untouched() {
        let mut ast = doc(vec![make_codeblock(
            &["mermaid"],
            "flowchart LR\n  a --> b",
        )]);
        run_transform(&mut ast, Format::html()).await;
        assert!(matches!(&ast.blocks[0], Block::CodeBlock(_)));
        assert!(after_body_includes(&ast.meta).is_empty());
    }

    /// The brace form `{dot}` must NOT be matched. pampa parses
    /// ` ```{dot} ` to a class list containing `{dot}`, not `dot`.
    #[tokio::test]
    async fn brace_form_dot_cell_untouched() {
        let mut ast = doc(vec![make_codeblock(&["{dot}"], DIGRAPH)]);
        run_transform(&mut ast, Format::html()).await;

        assert!(
            matches!(&ast.blocks[0], Block::CodeBlock(_)),
            "brace-form cell must pass through; got {:?}",
            ast.blocks[0]
        );
        assert!(after_body_includes(&ast.meta).is_empty());
    }

    /// Diagram source is HTML-escaped. Graphviz HTML-like labels put
    /// real markup in the source, so this is the common case, not an
    /// edge case.
    #[tokio::test]
    async fn diagram_source_is_html_escaped() {
        let mut ast = doc(vec![make_codeblock(
            &["dot"],
            "digraph G { a [label=<<b>bold</b> & more>] }",
        )]);
        run_transform(&mut ast, Format::html()).await;

        let Block::RawBlock(raw) = &ast.blocks[0] else {
            panic!("expected RawBlock");
        };
        assert!(
            raw.text
                .contains("label=&lt;&lt;b&gt;bold&lt;/b&gt; &amp; more&gt;"),
            "source must be escaped; got:\n{}",
            raw.text
        );
        assert!(
            !raw.text.contains("<b>"),
            "raw <b> must not survive; got:\n{}",
            raw.text
        );
    }

    /// A dot block nested inside a Div (a column, a slide section) is
    /// converted too.
    #[tokio::test]
    async fn nested_dot_block_is_converted() {
        let mut ast = doc(vec![Block::Div(Box::new(Div {
            attr: (
                String::new(),
                vec!["section".to_string()],
                hashlink::LinkedHashMap::new(),
            )
                .into(),
            content: vec![make_codeblock(&["dot"], DIGRAPH)],
            source_info: SourceInfo::for_test(),
            attr_source: AttrSourceInfo::empty(),
        }))]);
        run_transform(&mut ast, Format::html()).await;

        let Block::Div(div) = &ast.blocks[0] else {
            panic!("expected Div wrapper to survive");
        };
        assert!(
            matches!(&div.content[0], Block::RawBlock(r) if r.text.contains("pre class=\"graphviz\"")),
            "nested dot block must be converted; got {:?}",
            div.content[0]
        );
        assert_eq!(after_body_includes(&ast.meta).len(), 1);
    }

    /// A code block whose class list includes `dot` among others is
    /// still converted.
    #[tokio::test]
    async fn dot_with_extra_classes_is_converted() {
        let mut ast = doc(vec![make_codeblock(&["dot", "extra"], DIGRAPH)]);
        run_transform(&mut ast, Format::html()).await;
        assert!(matches!(&ast.blocks[0], Block::RawBlock(_)));
    }

    /// `graph-layout` selects the layout engine through a data
    /// attribute the init script reads.
    #[test]
    fn graph_layout_becomes_data_attribute() {
        assert_eq!(
            pre_tag(make_codeblock_with_attrs(
                &["dot"],
                "graph G {}",
                &[("graph-layout", "neato")]
            )),
            "<pre class=\"graphviz\" data-graph-layout=\"neato\">"
        );
    }

    /// The attribute form is not validated anywhere else, so a value
    /// that is not a layout engine must not reach the markup.
    #[test]
    fn unknown_graph_layout_value_is_not_emitted() {
        assert_eq!(
            pre_tag(make_codeblock_with_attrs(
                &["dot"],
                "graph G {}",
                &[("graph-layout", "\"><script>")]
            )),
            "<pre class=\"graphviz\">"
        );
    }

    /// A bare diagram's `fig-align` becomes the alignment class on the
    /// `<pre>`, as it does for mermaid.
    #[test]
    fn bare_fig_align_becomes_pre_class() {
        assert_eq!(
            pre_tag(make_codeblock_with_attrs(
                &["dot"],
                "graph G {}",
                &[("fig-align", "right")]
            )),
            "<pre class=\"graphviz quarto-figure-right\">"
        );
        assert_eq!(
            pre_tag(make_codeblock_with_attrs(
                &["dot"],
                "graph G {}",
                &[("fig-align", "\"><script>")]
            )),
            "<pre class=\"graphviz\">"
        );
    }

    /// `fig-alt` reaches the page as an escaped data attribute; the
    /// init script turns it into the SVG's accessible name.
    #[test]
    fn fig_alt_becomes_escaped_data_attribute() {
        assert_eq!(
            pre_tag(make_codeblock_with_attrs(
                &["dot"],
                "graph G {}",
                &[("fig-alt", "A \"quoted\" <alt>")]
            )),
            "<pre class=\"graphviz\" data-fig-alt=\"A &quot;quoted&quot; &lt;alt&gt;\">"
        );
    }

    /// The init script must handle the three things the markup can ask
    /// of it: the layout engine, the alt text, and a failed layout.
    #[test]
    fn script_handles_layout_alt_text_and_errors() {
        let script = graphviz_script_block("graphviz/viz-global.js");
        assert!(script.starts_with(GRAPHVIZ_JS_SENTINEL));
        for needle in [
            "dataset.graphLayout",
            "dataset.figAlt",
            "aria-label",
            "graphviz-error",
            "result.errors",
        ] {
            assert!(
                script.contains(needle),
                "init script must mention `{needle}`; got:\n{script}"
            );
        }
    }

    /// The init is JavaScript inside a Rust raw string, and no Rust test
    /// can parse it. These are the cheap structural checks: a string
    /// literal broken across lines is a syntax error that stops every
    /// diagram on the page (it happened once, with `'\n'` written as a
    /// real newline), and unbalanced brackets are the other easy slip.
    #[test]
    fn init_script_is_structurally_sound() {
        for (number, line) in GRAPHVIZ_INIT_JS.lines().enumerate() {
            assert!(
                line.matches('\'').count() % 2 == 0,
                "line {} of the init script has an unterminated string literal:\n{line}",
                number + 1
            );
        }
        for (open, close) in [('(', ')'), ('{', '}'), ('[', ']')] {
            assert_eq!(
                GRAPHVIZ_INIT_JS.matches(open).count(),
                GRAPHVIZ_INIT_JS.matches(close).count(),
                "init script has unbalanced `{open}` `{close}`"
            );
        }
        assert!(GRAPHVIZ_INIT_JS.contains(r"+ '\n';"));
    }

    /// Non-HTML-family formats: transform is a no-op (self-gated).
    #[tokio::test]
    async fn non_html_format_is_untouched() {
        let mut ast = doc(vec![make_codeblock(&["dot"], DIGRAPH)]);
        run_transform(&mut ast, Format::pdf()).await;

        assert!(
            matches!(&ast.blocks[0], Block::CodeBlock(_)),
            "non-html format must leave the block alone; got {:?}",
            ast.blocks[0]
        );
        assert!(after_body_includes(&ast.meta).is_empty());
    }

    /// revealjs is HTML-family, so the transform runs there.
    #[tokio::test]
    async fn revealjs_format_is_converted() {
        let mut ast = doc(vec![make_codeblock(&["dot"], DIGRAPH)]);
        run_transform(&mut ast, Format::from_format_string("revealjs").unwrap()).await;
        assert!(matches!(&ast.blocks[0], Block::RawBlock(_)));
        assert_eq!(after_body_includes(&ast.meta).len(), 1);
    }
}
