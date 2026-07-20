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
//! (source HTML-escaped). When at least one diagram was found, a
//! once-per-document `<script type="module">` that loads mermaid.js
//! from the jsdelivr CDN and calls `mermaid.run()` is appended to the
//! canonical `rendered.includes.after-body` list (the
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
//! [`render_revealjs_document`]: crate::revealjs::render_revealjs_document

use quarto_pandoc_types::block::{Block, Blocks, RawBlock};
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};
use crate::transforms::append_with_sentinel;

/// Exact-pinned mermaid.js version (decision 1, plan §Resolved
/// decisions — matches the copy Quarto 1 bundles). The TS preview
/// component pins the same version in
/// `ts-packages/preview-renderer/src/q2-preview/blocks/MermaidCodeBlock.tsx`;
/// bump both together.
pub const MERMAID_VERSION: &str = "11.12.0";

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
            append_with_sentinel(
                &mut ast.meta,
                "after-body",
                MERMAID_JS_SENTINEL,
                mermaid_script_block(),
            );
        }
        Ok(())
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

/// The once-per-document CDN loader. `mermaid.run()` is called
/// explicitly (not `startOnLoad: true`) so diagrams render regardless
/// of when the module executes relative to `DOMContentLoaded` — the
/// script sits in the after-body slot, and in embedded contexts
/// (iframes, late injection) load-event timing is not guaranteed.
fn mermaid_script_block() -> String {
    format!(
        "{MERMAID_JS_SENTINEL}\n\
         <script type=\"module\">\n\
         import mermaid from 'https://cdn.jsdelivr.net/npm/mermaid@{MERMAID_VERSION}/dist/mermaid.esm.min.mjs';\n\
         mermaid.initialize({{ startOnLoad: false }});\n\
         mermaid.run({{ querySelector: 'pre.mermaid' }});\n\
         </script>"
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
    /// `RawBlock("html", <pre class="mermaid">…</pre>)`, and the CDN
    /// script lands in `rendered.includes.after-body`.
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
            script
                .contains("https://cdn.jsdelivr.net/npm/mermaid@11.12.0/dist/mermaid.esm.min.mjs"),
            "script must import the exact-pinned CDN build; got:\n{script}"
        );
        assert!(
            script.contains("mermaid.initialize({ startOnLoad: false })"),
            "script must initialize with startOnLoad false; got:\n{script}"
        );
        assert!(
            script.contains("mermaid.run({ querySelector: 'pre.mermaid' })"),
            "script must run explicitly on pre.mermaid; got:\n{script}"
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
