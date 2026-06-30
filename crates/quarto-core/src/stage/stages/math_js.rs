/*
 * stage/stages/math_js.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Inject a math-rendering JS engine (MathJax / KaTeX) when the
 * document contains Math elements.
 */

//! Inject a math-rendering JS engine when the document contains math.
//!
//! ## Pattern this stage demonstrates
//!
//! Unlike [`super::BootstrapJsStage`], whose trigger is a metadata
//! predicate (`is the theme Bootstrap-backed?`), this stage's trigger
//! depends on **document content** — "does the AST contain at least one
//! `Math` element?". The walker descends through blocks, inlines, and
//! `CustomNode` slots so that labelled equations (which are wrapped in
//! `CustomNode("Equation")` by [`crate::transforms::EquationLabelTransform`])
//! are not missed.
//!
//! Unlike Bootstrap (where we ship vendored bytes via a `js:bootstrap`
//! Project-scoped artifact), math defaults to **CDN-hosted** loaders —
//! the same behavior Pandoc and Quarto 1 produce. There are no bytes to
//! ship; the stage's only output is a string written to `meta.math`,
//! consumed by the new `$math$` template slot.
//!
//! ## Public AST contract (filter extensibility)
//!
//! This stage manipulates only the publicly-described AST:
//!
//! - It reads `Inline::Math(MathType, String)` and the
//!   `CustomNode("Equation")` wrapper that
//!   [`crate::transforms::EquationLabelTransform`] produces from
//!   `$$…$$ {#eq-…}` syntax.
//! - The HTML writer emits `<span class="math inline">\(…\)</span>` /
//!   `<span class="math display">\[…\]</span>` markup which the JS
//!   engine then typesets in the browser.
//!
//! A user-supplied Lua filter could in principle take ownership of math
//! handling by replacing `Inline::Math` nodes with custom markup — the
//! same property as our navbar rendering.
//!
//! ## WASM
//!
//! Math display is safe under hub-client's iframe-per-render preview
//! model: each iframe load gets a fresh DOM and the engine typesets
//! once on `DOMContentLoaded`. There's no JS-object state held across
//! reinits like Bootstrap had, so this stage IS included in
//! `build_wasm_html_pipeline()`.

use async_trait::async_trait;

use quarto_pandoc_types::block::Block;
use quarto_pandoc_types::config_value::ConfigValue;
use quarto_pandoc_types::custom::Slot;
use quarto_pandoc_types::inline::Inline;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::{By, SourceInfo};

use crate::stage::{
    EventLevel, PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext,
};
use crate::trace_event;

/// Default jsDelivr URL for the MathJax 3 `tex-chtml-full` loader.
/// Matches Pandoc's [`defaultMathJaxURL`] and Quarto 1's default.
///
/// [`defaultMathJaxURL`]: https://github.com/jgm/pandoc/blob/main/src/Text/Pandoc/Options.hs
pub const DEFAULT_MATHJAX_URL: &str =
    "https://cdn.jsdelivr.net/npm/mathjax@3/es5/tex-chtml-full.js";

/// Default jsDelivr base URL for KaTeX (same CDN as Pandoc's
/// `defaultKaTeXURL`). Loader is `${base}katex.min.js`, stylesheet is
/// `${base}katex.min.css`, auto-render is `${base}contrib/auto-render.min.js`.
///
/// Pinned to the exact version of the npm `katex` dependency the
/// preview surfaces bundle, so render (CDN) and preview (bundled)
/// render math identically and render output doesn't change under
/// users when the CDN's `latest` advances (bd-4b7f1hr7). The
/// `katex_cdn_version_matches_npm_pin` test enforces the pairing —
/// bump this together with the `katex` pins in the root and
/// `hub-client/quarto-hub-sandboxed-preview` package.json.
pub const DEFAULT_KATEX_URL_BASE: &str = "https://cdn.jsdelivr.net/npm/katex@0.17.0/dist/";

/// Math-rendering engine selected by `html-math-method:`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MathEngine {
    /// MathJax 3 (default). User-overridable URL points at the loader.
    Mathjax { url: String },
    /// KaTeX. User-overridable URL points at the dist base directory.
    Katex { url_base: String },
}

impl MathEngine {
    /// Default engine when `html-math-method` is absent (matches Q1).
    pub fn default_engine() -> Self {
        Self::Mathjax {
            url: DEFAULT_MATHJAX_URL.to_string(),
        }
    }

    /// Parse the `html-math-method` value out of the document metadata.
    ///
    /// Accepts both forms supported by Quarto 1 / Pandoc:
    /// - **String form** — `html-math-method: mathjax | katex`. Engine
    ///   is selected; URL falls back to the engine default.
    /// - **Object form** — `html-math-method: { method: ..., url: ... }`.
    ///   Both fields honored; `url` overrides the default. The `method`
    ///   key is required in this shape; if missing, we fall back to the
    ///   default engine.
    ///
    /// Unknown method strings (e.g. `webtex`, `gladtex`) are *not*
    /// supported in v1 and produce `None`. The caller should treat
    /// `None` as "math rendering is not q2's responsibility for this
    /// document" and skip injection. (Today this only applies if the
    /// user explicitly opts out; `None` is never returned for absent /
    /// `mathjax` / `katex`.)
    pub fn from_meta(meta: &ConfigValue) -> Option<Self> {
        let Some(value) = meta.get("html-math-method") else {
            return Some(Self::default_engine());
        };

        // Object form first: { method: ..., url?: ... }.
        if value.is_map() {
            let method = value.get("method").and_then(|v| v.as_plain_text());
            let url = value.get("url").and_then(|v| v.as_plain_text());
            return match method.as_deref() {
                Some("mathjax") | None => Some(Self::Mathjax {
                    url: url.unwrap_or_else(|| DEFAULT_MATHJAX_URL.to_string()),
                }),
                Some("katex") => Some(Self::Katex {
                    url_base: url.unwrap_or_else(|| DEFAULT_KATEX_URL_BASE.to_string()),
                }),
                Some(_other) => None,
            };
        }

        // String form.
        match value.as_plain_text().as_deref() {
            Some("mathjax") => Some(Self::Mathjax {
                url: DEFAULT_MATHJAX_URL.to_string(),
            }),
            Some("katex") => Some(Self::Katex {
                url_base: DEFAULT_KATEX_URL_BASE.to_string(),
            }),
            // Other strings (webtex, gladtex, mathml, plain) — defer.
            Some(_) => None,
            None => Some(Self::default_engine()),
        }
    }

    /// Render the engine's `$math$`-slot contents: an inline config /
    /// init block followed by the loader `<script>` tag(s). This whole
    /// string is what gets inserted into the document `<head>` via the
    /// `$math$` template slot.
    fn render_math_slot(&self) -> String {
        match self {
            Self::Mathjax { url } => mathjax_slot_html(url),
            Self::Katex { url_base } => katex_slot_html(url_base),
        }
    }
}

/// Render the MathJax `$math$`-slot contents. The order matters:
/// `window.MathJax = { ... }` MUST appear BEFORE the loader
/// `<script src=...>` so the loader picks up the configuration.
///
/// Defaults chosen to match Pandoc / Quarto 1 user expectations:
///
/// - `tex.inlineMath: [['\\(', '\\)']]` and `tex.displayMath: [['\\[', '\\]']]`
///   match q2's HTML writer, which emits `<span class="math inline">\(...\)</span>`
///   and `<span class="math display">\[...\]</span>` (`crates/pampa/src/writers/html.rs:754-767`).
/// - `tex.tags: 'ams'` enables `\tag{N}` and `\eqref{...}` for labelled
///   equations — `CrossrefRenderTransform` injects `\tag{N}` so this
///   flag is required for equation numbering to render.
/// - `options.skipHtmlTags` excludes elements MathJax must not typeset
///   (code blocks, scripts, pre, etc.) — Pandoc's defaults.
fn mathjax_slot_html(url: &str) -> String {
    format!(
        "<script>\n\
         window.MathJax = {{\n\
         \x20\x20tex: {{\n\
         \x20\x20\x20\x20inlineMath: [['\\\\(', '\\\\)']],\n\
         \x20\x20\x20\x20displayMath: [['\\\\[', '\\\\]']],\n\
         \x20\x20\x20\x20tags: 'ams',\n\
         \x20\x20}},\n\
         \x20\x20options: {{\n\
         \x20\x20\x20\x20skipHtmlTags: ['script', 'noscript', 'style', 'textarea', 'pre'],\n\
         \x20\x20}},\n\
         }};\n\
         </script>\n\
         <script defer src=\"{url}\" type=\"text/javascript\"></script>",
        url = html_escape_attr(url)
    )
}

/// Render the KaTeX `$math$`-slot contents. KaTeX is more involved
/// than MathJax because it has three pieces:
///
/// 1. A stylesheet `<link>` (KaTeX needs CSS to render correctly).
/// 2. The KaTeX `<script>` (defines `katex` global).
/// 3. The auto-render extension `<script>` (provides `renderMathInElement`).
/// 4. An inline `<script>` calling `renderMathInElement(document.body)`
///    once the page is ready, with delimiters matching q2's HTML writer
///    (`\(…\)` for inline, `\[…\]` for display) and `throwOnError: false`
///    so a single bad expression doesn't break the whole page.
///
/// All three external assets share a common base URL (`url_base`).
fn katex_slot_html(url_base: &str) -> String {
    let base = url_base.trim_end_matches('/');
    format!(
        "<link rel=\"stylesheet\" href=\"{base}/katex.min.css\">\n\
         <script defer src=\"{base}/katex.min.js\"></script>\n\
         <script defer src=\"{base}/contrib/auto-render.min.js\" \
         onload=\"renderMathInElement(document.body, {{\
         delimiters: [\
         {{left: '\\\\(', right: '\\\\)', display: false}}, \
         {{left: '\\\\[', right: '\\\\]', display: true}}\
         ], throwOnError: false}});\"></script>",
        base = html_escape_attr(base)
    )
}

/// Minimal escape for an attribute value used inside double quotes.
/// The URLs we insert come either from a const or from the user's
/// YAML, so they're already URL-shaped — but we still escape `"` and
/// `&` defensively to keep the emitted HTML well-formed.
fn html_escape_attr(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;")
}

/// Walk the document AST and return `true` as soon as an `Inline::Math`
/// node is encountered. Descends through every block container and
/// `CustomNode` slot — labelled equations live inside a
/// `CustomNode("Equation")` after `EquationLabelTransform` runs, and
/// math can also appear nested inside callouts / list items / table
/// cells / footnotes / etc.
///
/// Short-circuits on the first hit: O(1) for math-bearing docs once
/// the first node is seen, O(N) for math-free docs.
fn doc_has_math(ast: &Pandoc) -> bool {
    blocks_have_math(&ast.blocks)
}

fn blocks_have_math(blocks: &[Block]) -> bool {
    blocks.iter().any(block_has_math)
}

fn block_has_math(block: &Block) -> bool {
    match block {
        Block::Plain(p) => inlines_have_math(&p.content),
        Block::Paragraph(p) => inlines_have_math(&p.content),
        Block::LineBlock(lb) => lb.content.iter().any(|line| inlines_have_math(line)),
        Block::BlockQuote(bq) => blocks_have_math(&bq.content),
        Block::OrderedList(ol) => ol.content.iter().any(|item| blocks_have_math(item)),
        Block::BulletList(bl) => bl.content.iter().any(|item| blocks_have_math(item)),
        Block::DefinitionList(dl) => dl.content.iter().any(|(term, defs)| {
            inlines_have_math(term) || defs.iter().any(|def| blocks_have_math(def))
        }),
        Block::Header(h) => inlines_have_math(&h.content),
        Block::Div(d) => blocks_have_math(&d.content),
        Block::Figure(f) => {
            // Figure caption is Caption { short: Option<Inlines>, long: Option<Blocks> }.
            let cap_has_math = f
                .caption
                .short
                .as_ref()
                .is_some_and(|i| inlines_have_math(i))
                || f.caption
                    .long
                    .as_ref()
                    .is_some_and(|bs| blocks_have_math(bs));
            cap_has_math || blocks_have_math(&f.content)
        }
        Block::Table(t) => {
            let cap_has_math = t
                .caption
                .short
                .as_ref()
                .is_some_and(|i| inlines_have_math(i))
                || t.caption
                    .long
                    .as_ref()
                    .is_some_and(|bs| blocks_have_math(bs));
            if cap_has_math {
                return true;
            }
            for row in t.head.rows.iter().chain(t.foot.rows.iter()) {
                for cell in row.cells.iter() {
                    if blocks_have_math(&cell.content) {
                        return true;
                    }
                }
            }
            for body in t.bodies.iter() {
                for row in body.body.iter() {
                    for cell in row.cells.iter() {
                        if blocks_have_math(&cell.content) {
                            return true;
                        }
                    }
                }
            }
            false
        }
        Block::CaptionBlock(cb) => inlines_have_math(&cb.content),
        Block::Custom(c) => c.slots.values().any(slot_has_math),
        // Leaves that cannot contain math:
        // - CodeBlock / RawBlock are opaque text; even text that
        //   *looks* like math is rendered verbatim, not typeset.
        // - HorizontalRule has no content.
        // - BlockMetadata holds metadata, not body content.
        // - NoteDefinitionPara / NoteDefinitionFencedBlock hold
        //   footnote bodies; a `Note` inline points at them and is
        //   walked separately.
        Block::CodeBlock(_)
        | Block::RawBlock(_)
        | Block::HorizontalRule(_)
        | Block::BlockMetadata(_)
        | Block::NoteDefinitionPara(_)
        | Block::NoteDefinitionFencedBlock(_) => false,
    }
}

fn inlines_have_math(inlines: &[Inline]) -> bool {
    inlines.iter().any(inline_has_math)
}

fn inline_has_math(inline: &Inline) -> bool {
    match inline {
        // The reason we exist.
        Inline::Math(_) => true,
        // Containers — recurse.
        Inline::Emph(e) => inlines_have_math(&e.content),
        Inline::Underline(u) => inlines_have_math(&u.content),
        Inline::Strong(s) => inlines_have_math(&s.content),
        Inline::Strikeout(s) => inlines_have_math(&s.content),
        Inline::Superscript(s) => inlines_have_math(&s.content),
        Inline::Subscript(s) => inlines_have_math(&s.content),
        Inline::SmallCaps(s) => inlines_have_math(&s.content),
        Inline::Quoted(q) => inlines_have_math(&q.content),
        Inline::Cite(c) => inlines_have_math(&c.content),
        Inline::Link(l) => inlines_have_math(&l.content),
        Inline::Image(i) => inlines_have_math(&i.content),
        Inline::Span(s) => inlines_have_math(&s.content),
        Inline::Insert(i) => inlines_have_math(&i.content),
        Inline::Delete(d) => inlines_have_math(&d.content),
        Inline::Highlight(h) => inlines_have_math(&h.content),
        Inline::Note(n) => blocks_have_math(&n.content),
        Inline::Custom(c) => c.slots.values().any(slot_has_math),
        // Leaves: no math possible.
        Inline::Str(_)
        | Inline::Code(_)
        | Inline::Space(_)
        | Inline::SoftBreak(_)
        | Inline::LineBreak(_)
        | Inline::RawInline(_)
        | Inline::Shortcode(_)
        | Inline::NoteReference(_)
        | Inline::Attr(_)
        | Inline::EditComment(_) => false,
    }
}

fn slot_has_math(slot: &Slot) -> bool {
    match slot {
        Slot::Block(b) => block_has_math(b),
        Slot::Blocks(bs) => blocks_have_math(bs),
        Slot::Inline(i) => inline_has_math(i),
        Slot::Inlines(is) => inlines_have_math(is),
    }
}

/// Inject a math-rendering JS engine when the document contains math.
///
/// Walks the AST after user filters run. If at least one `Inline::Math`
/// node is found (including inside `CustomNode("Equation")` slots),
/// populates `meta.math` with the engine-specific config + loader
/// `<script>` blocks. The new `$math$` template slot in the built-in
/// HTML templates renders this string verbatim into `<head>` before
/// the `$for(scripts)$` loop, so the inline config block lands before
/// the loader (what MathJax expects).
pub struct MathJsStage;

impl MathJsStage {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MathJsStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl PipelineStage for MathJsStage {
    fn name(&self) -> &str {
        "math-js"
    }

    fn input_kind(&self) -> PipelineDataKind {
        PipelineDataKind::DocumentAst
    }

    fn output_kind(&self) -> PipelineDataKind {
        PipelineDataKind::DocumentAst
    }

    async fn run(
        &self,
        input: PipelineData,
        ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        let PipelineData::DocumentAst(mut doc) = input else {
            return Err(PipelineError::unexpected_input(
                self.name(),
                self.input_kind(),
                input.kind(),
            ));
        };

        // Bail early if no math anywhere — keeps math-free renders byte-
        // identical to before this stage existed (no `meta.math` set).
        if !doc_has_math(&doc.ast) {
            trace_event!(ctx, EventLevel::Debug, "no math in doc; skipping math-js");
            return Ok(PipelineData::DocumentAst(doc));
        }

        // Read the user-selected engine (or fall back to default).
        // `from_meta` returns `None` for explicitly-unsupported methods
        // (`webtex` / `gladtex` / `mathml` / `plain` — deferred). In
        // that case we leave `meta.math` unset so the document author
        // can supply their own approach via includes / custom template.
        let Some(engine) = MathEngine::from_meta(&doc.ast.meta) else {
            trace_event!(
                ctx,
                EventLevel::Debug,
                "html-math-method is set to an unsupported value; skipping math-js"
            );
            return Ok(PipelineData::DocumentAst(doc));
        };

        let math_html = engine.render_math_slot();

        // Stash the rendered string under `meta.math`. The `$math$`
        // slot in the built-in HTML templates renders this verbatim
        // into `<head>` before `$for(scripts)$`, so the inline config
        // block lands ahead of the loader (matters for MathJax).
        doc.ast.meta.insert_path(
            &["math"],
            ConfigValue::new_string(math_html, SourceInfo::generated(By::programmatic_config())),
        );

        trace_event!(
            ctx,
            EventLevel::Debug,
            "math-js: populated meta.math (engine={})",
            match &engine {
                MathEngine::Mathjax { .. } => "mathjax",
                MathEngine::Katex { .. } => "katex",
            }
        );

        Ok(PipelineData::DocumentAst(doc))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectContext};
    use crate::stage::DocumentAst;
    use hashlink::LinkedHashMap;
    use quarto_pandoc_types::block::{Block, Paragraph};
    use quarto_pandoc_types::custom::{CustomNode, Slot};
    use quarto_pandoc_types::inline::{Inline, Math, MathType, Str};
    use quarto_pandoc_types::pandoc::Pandoc;
    use quarto_pandoc_types::{Attr, ConfigMapEntry, ConfigValue, ConfigValueKind, MergeOp};
    use quarto_source_map::{SourceContext, SourceInfo};
    use quarto_system_runtime::TempDir;
    use std::path::PathBuf;
    use std::sync::Arc;
    use yaml_rust2::Yaml;

    // ── Test helpers ─────────────────────────────────────────────

    fn make_stage_context(runtime: Arc<dyn quarto_system_runtime::SystemRuntime>) -> StageContext {
        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: crate::project::ProjectConfig::default(),
            is_single_file: true,
            files: vec![],
            output_dir: PathBuf::from("/project"),

            ..Default::default()
        };
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();
        StageContext::new(runtime, format, project, doc).unwrap()
    }

    fn empty_meta() -> ConfigValue {
        ConfigValue {
            value: ConfigValueKind::Map(vec![]),
            source_info: SourceInfo::for_test(),
            merge_op: MergeOp::Concat,
        }
    }

    fn meta_with_string(key: &str, value: &str) -> ConfigValue {
        let v = ConfigValue {
            value: ConfigValueKind::Scalar(Yaml::String(value.to_string())),
            source_info: SourceInfo::for_test(),
            merge_op: MergeOp::Concat,
        };
        ConfigValue {
            value: ConfigValueKind::Map(vec![ConfigMapEntry {
                key: key.to_string(),
                key_source: SourceInfo::for_test(),
                value: v,
            }]),
            source_info: SourceInfo::for_test(),
            merge_op: MergeOp::Concat,
        }
    }

    /// Build a meta with `html-math-method: { method: <m>, url: <u> }`.
    fn meta_with_method_url(method: &str, url: &str) -> ConfigValue {
        let inner = ConfigValue {
            value: ConfigValueKind::Map(vec![
                ConfigMapEntry {
                    key: "method".to_string(),
                    key_source: SourceInfo::for_test(),
                    value: ConfigValue {
                        value: ConfigValueKind::Scalar(Yaml::String(method.to_string())),
                        source_info: SourceInfo::for_test(),
                        merge_op: MergeOp::Concat,
                    },
                },
                ConfigMapEntry {
                    key: "url".to_string(),
                    key_source: SourceInfo::for_test(),
                    value: ConfigValue {
                        value: ConfigValueKind::Scalar(Yaml::String(url.to_string())),
                        source_info: SourceInfo::for_test(),
                        merge_op: MergeOp::Concat,
                    },
                },
            ]),
            source_info: SourceInfo::for_test(),
            merge_op: MergeOp::Concat,
        };
        ConfigValue {
            value: ConfigValueKind::Map(vec![ConfigMapEntry {
                key: "html-math-method".to_string(),
                key_source: SourceInfo::for_test(),
                value: inner,
            }]),
            source_info: SourceInfo::for_test(),
            merge_op: MergeOp::Concat,
        }
    }

    fn inline_math(text: &str) -> Inline {
        Inline::Math(Math {
            math_type: MathType::InlineMath,
            text: text.to_string(),
            source_info: SourceInfo::for_test(),
        })
    }

    fn display_math(text: &str) -> Inline {
        Inline::Math(Math {
            math_type: MathType::DisplayMath,
            text: text.to_string(),
            source_info: SourceInfo::for_test(),
        })
    }

    fn str_inline(s: &str) -> Inline {
        Inline::Str(Str {
            text: s.to_string(),
            source_info: SourceInfo::for_test(),
        })
    }

    fn paragraph(content: Vec<Inline>) -> Block {
        Block::Paragraph(Paragraph {
            content,
            source_info: SourceInfo::for_test(),
        })
    }

    fn empty_attr() -> Attr {
        quarto_pandoc_types::attr::empty_attr()
    }

    fn doc_with_blocks(blocks: Vec<Block>, meta: ConfigValue) -> PipelineData {
        PipelineData::DocumentAst(DocumentAst {
            path: PathBuf::from("/project/test.qmd"),
            ast: Pandoc { meta, blocks },
            ast_context: pampa::pandoc::ASTContext::default(),
            source_context: SourceContext::new(),
            warnings: vec![],
            recorded_includes: Vec::new(),
        })
    }

    // ── Mock runtime ─────────────────────────────────────────────
    struct MockRuntime;

    #[async_trait::async_trait]
    impl quarto_system_runtime::SystemRuntime for MockRuntime {
        fn file_read(
            &self,
            _path: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<Vec<u8>> {
            Ok(vec![])
        }
        fn file_write(
            &self,
            _path: &std::path::Path,
            _contents: &[u8],
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn path_exists(
            &self,
            _path: &std::path::Path,
            _kind: Option<quarto_system_runtime::PathKind>,
        ) -> quarto_system_runtime::RuntimeResult<bool> {
            Ok(true)
        }
        fn canonicalize(
            &self,
            path: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            Ok(path.to_path_buf())
        }
        fn path_metadata(
            &self,
            _path: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::PathMetadata> {
            unimplemented!()
        }
        fn file_copy(
            &self,
            _src: &std::path::Path,
            _dst: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn path_rename(
            &self,
            _old: &std::path::Path,
            _new: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn file_remove(&self, _path: &std::path::Path) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn dir_create(
            &self,
            _path: &std::path::Path,
            _recursive: bool,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn dir_remove(
            &self,
            _path: &std::path::Path,
            _recursive: bool,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn dir_list(
            &self,
            _path: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<Vec<PathBuf>> {
            Ok(vec![])
        }
        fn cwd(&self) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            Ok(PathBuf::from("/"))
        }
        fn temp_dir(&self, _template: &str) -> quarto_system_runtime::RuntimeResult<TempDir> {
            Ok(TempDir::new(PathBuf::from("/tmp/test")))
        }
        fn exec_pipe(
            &self,
            _command: &str,
            _args: &[&str],
            _stdin: &[u8],
        ) -> quarto_system_runtime::RuntimeResult<Vec<u8>> {
            Ok(vec![])
        }
        fn exec_command(
            &self,
            _command: &str,
            _args: &[&str],
            _stdin: Option<&[u8]>,
        ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::CommandOutput> {
            Ok(quarto_system_runtime::CommandOutput {
                code: 0,
                stdout: vec![],
                stderr: vec![],
            })
        }
        fn env_get(&self, _name: &str) -> quarto_system_runtime::RuntimeResult<Option<String>> {
            Ok(None)
        }
        fn env_all(
            &self,
        ) -> quarto_system_runtime::RuntimeResult<std::collections::HashMap<String, String>>
        {
            Ok(std::collections::HashMap::new())
        }
        async fn fetch_url(
            &self,
            _url: &str,
        ) -> quarto_system_runtime::RuntimeResult<(Vec<u8>, String)> {
            Err(quarto_system_runtime::RuntimeError::NotSupported(
                "mock".to_string(),
            ))
        }
        fn os_name(&self) -> &'static str {
            "mock"
        }
        fn arch(&self) -> &'static str {
            "mock"
        }
        fn cpu_time(&self) -> quarto_system_runtime::RuntimeResult<u64> {
            Ok(0)
        }
        fn xdg_dir(
            &self,
            _kind: quarto_system_runtime::XdgDirKind,
            _subpath: Option<&std::path::Path>,
        ) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            Ok(PathBuf::from("/xdg"))
        }
        fn stdout_write(&self, _data: &[u8]) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn stderr_write(&self, _data: &[u8]) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
    }

    /// After running the stage, return the rendered `meta.math` value
    /// (a string) if it was set, else `None`.
    async fn run_and_get_math(blocks: Vec<Block>, meta: ConfigValue) -> Option<String> {
        let runtime = Arc::new(MockRuntime);
        let mut ctx = make_stage_context(runtime);
        let stage = MathJsStage::new();

        let output = stage
            .run(doc_with_blocks(blocks, meta), &mut ctx)
            .await
            .expect("stage.run failed");

        let PipelineData::DocumentAst(out_doc) = output else {
            panic!("stage returned unexpected pipeline data kind");
        };

        out_doc.ast.meta.get("math").and_then(|v| v.as_plain_text())
    }

    // ── Tests for the trigger walker (Phase 1.1) ────────────────

    /// Doc with a single inline math element + default config.
    /// `meta.math` must contain MathJax inline config + the default
    /// CDN loader URL.
    #[tokio::test]
    async fn inline_math_default_emits_mathjax_config_and_loader() {
        let math = run_and_get_math(
            vec![paragraph(vec![str_inline("see "), inline_math("x^2")])],
            empty_meta(),
        )
        .await;

        let math = math.expect("meta.math should be set when math is present");
        assert!(
            math.contains("window.MathJax"),
            "math output should contain MathJax inline config; got:\n{}",
            math
        );
        assert!(
            math.contains(DEFAULT_MATHJAX_URL),
            "math output should contain default MathJax CDN URL; got:\n{}",
            math
        );
    }

    /// Math-free doc → meta.math is not set.
    #[tokio::test]
    async fn no_math_in_doc_skips_meta_math() {
        let math = run_and_get_math(
            vec![paragraph(vec![str_inline("plain text only")])],
            empty_meta(),
        )
        .await;
        assert!(
            math.is_none(),
            "math-free doc must not set meta.math; got: {:?}",
            math
        );
    }

    /// Display math (`$$y$$`) triggers the same way as inline math.
    #[tokio::test]
    async fn display_math_triggers_injection() {
        let math = run_and_get_math(
            vec![paragraph(vec![display_math("y = mx + b")])],
            empty_meta(),
        )
        .await;
        assert!(
            math.is_some(),
            "display math must trigger meta.math population"
        );
    }

    /// Labelled equation: `EquationLabelTransform` has rewritten the
    /// `$$…$$ {#eq-foo}` AST into `CustomNode("Equation")` containing
    /// `Inline::Math` in the `content` slot. The walker must descend
    /// into the slot.
    #[tokio::test]
    async fn labelled_equation_via_customnode_triggers_injection() {
        let mut node = CustomNode::new("Equation", empty_attr(), SourceInfo::for_test());
        node.slots.insert(
            "content".to_string(),
            Slot::Inlines(vec![display_math("x = y \\tag{1}")]),
        );

        let math = run_and_get_math(
            vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Custom(node)],
                source_info: SourceInfo::for_test(),
            })],
            empty_meta(),
        )
        .await;

        assert!(
            math.is_some(),
            "labelled equation inside CustomNode must trigger meta.math"
        );
    }

    /// `html-math-method: katex` selects KaTeX. The output must
    /// reference the KaTeX CDN base, not MathJax.
    #[tokio::test]
    async fn katex_method_uses_katex_cdn() {
        let math = run_and_get_math(
            vec![paragraph(vec![inline_math("x")])],
            meta_with_string("html-math-method", "katex"),
        )
        .await
        .expect("meta.math should be set");

        assert!(
            math.contains(DEFAULT_KATEX_URL_BASE) || math.contains("katex.min.js"),
            "katex output should reference the default KaTeX CDN; got:\n{}",
            math
        );
        assert!(
            !math.contains("mathjax"),
            "katex output should NOT contain mathjax loader URL; got:\n{}",
            math
        );
    }

    /// `html-math-method: { method: mathjax, url: "https://custom..." }`
    /// honors the user's URL.
    #[tokio::test]
    async fn custom_url_is_honored() {
        let custom = "https://example.com/my/mathjax.js";
        let math = run_and_get_math(
            vec![paragraph(vec![inline_math("x")])],
            meta_with_method_url("mathjax", custom),
        )
        .await
        .expect("meta.math should be set");

        assert!(
            math.contains(custom),
            "custom URL should appear in meta.math; got:\n{}",
            math
        );
        assert!(
            !math.contains(DEFAULT_MATHJAX_URL),
            "custom URL should replace the default; got:\n{}",
            math
        );
    }

    /// Re-running the stage twice on the same input is idempotent.
    #[tokio::test]
    async fn rerun_is_idempotent() {
        let runtime = Arc::new(MockRuntime);
        let mut ctx = make_stage_context(runtime);
        let stage = MathJsStage::new();

        let blocks = || vec![paragraph(vec![inline_math("x")])];

        let out1 = stage
            .run(doc_with_blocks(blocks(), empty_meta()), &mut ctx)
            .await
            .unwrap();
        let PipelineData::DocumentAst(doc1) = out1 else {
            panic!()
        };
        let math1 = doc1.ast.meta.get("math").and_then(|v| v.as_plain_text());

        // Feed doc1's meta back in (so we exercise the case where
        // meta.math is already set from a prior pass).
        let out2 = stage
            .run(doc_with_blocks(blocks(), doc1.ast.meta), &mut ctx)
            .await
            .unwrap();
        let PipelineData::DocumentAst(doc2) = out2 else {
            panic!()
        };
        let math2 = doc2.ast.meta.get("math").and_then(|v| v.as_plain_text());

        assert!(math1.is_some());
        assert_eq!(math1, math2, "rerun must be byte-identical");
    }

    /// Math nested inside a BlockQuote (and other block containers)
    /// must be discovered by the walker. We test a BlockQuote containing
    /// a paragraph with inline math.
    #[tokio::test]
    async fn math_in_blockquote_triggers_injection() {
        let para = paragraph(vec![str_inline("see "), inline_math("E = mc^2")]);
        let bq = Block::BlockQuote(quarto_pandoc_types::block::BlockQuote {
            content: vec![para],
            source_info: SourceInfo::for_test(),
        });

        let math = run_and_get_math(vec![bq], empty_meta()).await;
        assert!(
            math.is_some(),
            "math nested in blockquote must trigger meta.math"
        );
    }

    /// Math nested in a Custom block's slot (e.g., a callout) must be
    /// found. We construct a `CustomNode("Callout")` with a Blocks slot
    /// containing a paragraph with math.
    #[tokio::test]
    async fn math_in_callout_slot_triggers_injection() {
        let inner_para = paragraph(vec![inline_math("a + b")]);
        let mut callout = CustomNode::new("Callout", empty_attr(), SourceInfo::for_test());
        callout
            .slots
            .insert("content".to_string(), Slot::Blocks(vec![inner_para]));

        let math = run_and_get_math(vec![Block::Custom(callout)], empty_meta()).await;
        assert!(
            math.is_some(),
            "math inside CustomNode block-slot must trigger meta.math"
        );
    }

    /// CodeBlock contents are NOT math even if they look like math.
    /// Walker must not be tricked by code-block text.
    #[tokio::test]
    async fn math_inside_codeblock_is_not_math() {
        // Code block whose text happens to contain dollar-signs; this
        // is not an Inline::Math node, just a CodeBlock with a string.
        let cb = Block::CodeBlock(quarto_pandoc_types::block::CodeBlock {
            attr: empty_attr(),
            text: "$x = y$".to_string(),
            source_info: SourceInfo::for_test(),
            attr_source: quarto_pandoc_types::AttrSourceInfo::empty(),
        });

        let math = run_and_get_math(vec![cb], empty_meta()).await;
        assert!(
            math.is_none(),
            "code-block text mentioning $x$ must NOT trigger meta.math"
        );
    }

    // ── Tests for the engine config parser (independent of stage) ──

    #[test]
    fn engine_default_is_mathjax_with_default_cdn() {
        let e = MathEngine::default_engine();
        match e {
            MathEngine::Mathjax { url } => assert_eq!(url, DEFAULT_MATHJAX_URL),
            other => panic!("expected default Mathjax engine, got {:?}", other),
        }
    }

    /// bd-4b7f1hr7: every KaTeX copy in the tree must reference the
    /// same, exact version — otherwise `q2 render` (CDN link) and the
    /// preview surfaces (bundled npm copies) can render math
    /// differently. Same drift class the reveal convergence fixed
    /// (bd-ibqkf9ry; cf. `vendored_reveal_assets_match_npm_package` in
    /// `revealjs/assemble.rs`). The npm pins must be exact (no
    /// `^`/`~`) for "CDN matches the npm pin" to be well-defined.
    ///
    /// Covered copies:
    /// - `DEFAULT_KATEX_URL_BASE` (this file) — the CDN URL render
    ///   emits for `html-math-method: katex`.
    /// - root `package.json` — hoisted dependency the q2-preview
    ///   iframe bundles (`q2-preview/entry.tsx` imports `katex` +
    ///   `katex/dist/katex.min.css`).
    /// - `hub-client/quarto-hub-sandboxed-preview/package.json` — a
    ///   standalone (non-workspace) sub-project with its own lockfile
    ///   and its own bundled KaTeX.
    #[test]
    fn katex_cdn_version_matches_npm_pin() {
        fn katex_pin(pkg_path: &str) -> String {
            let pkg_text = std::fs::read_to_string(pkg_path)
                .unwrap_or_else(|e| panic!("reading {pkg_path}: {e}"));
            let pkg: serde_json::Value = serde_json::from_str(&pkg_text)
                .unwrap_or_else(|e| panic!("parsing {pkg_path}: {e}"));
            let version = ["dependencies", "devDependencies"]
                .iter()
                .find_map(|section| pkg.get(section)?.get("katex")?.as_str())
                .unwrap_or_else(|| panic!("{pkg_path} must declare a `katex` dependency"));
            assert!(
                version.chars().next().is_some_and(|c| c.is_ascii_digit()),
                "{pkg_path}: `katex` must be pinned to an exact version (got \
                 `{version}`) — a range pin lets that copy drift from render's CDN URL"
            );
            version.to_string()
        }

        let root_version = katex_pin(concat!(env!("CARGO_MANIFEST_DIR"), "/../../package.json"));
        let sandboxed_version = katex_pin(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../hub-client/quarto-hub-sandboxed-preview/package.json"
        ));
        assert_eq!(
            root_version, sandboxed_version,
            "root package.json and quarto-hub-sandboxed-preview/package.json \
             must pin the same KaTeX version — bump them together"
        );

        let expected_base = format!("https://cdn.jsdelivr.net/npm/katex@{root_version}/dist/");
        assert_eq!(
            DEFAULT_KATEX_URL_BASE, expected_base,
            "DEFAULT_KATEX_URL_BASE must pin the same exact KaTeX version as the \
             npm `katex` dependency in the root package.json — bump the two together"
        );
    }

    /// Sanity: silence the unused-import warnings if the test scope
    /// drifts (CI is `-D warnings`).
    #[allow(dead_code)]
    fn _unused(_m: LinkedHashMap<String, Slot>) {}
}
