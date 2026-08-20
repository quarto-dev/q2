/*
 * transforms/code_block_render.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Code-block decoration *Render* transform.
//!
//! Format-specific half of the code-block decoration pipeline,
//! consuming the typed
//! [`CodeBlockDecoration`](super::code_block_generate::CodeBlockDecoration)
//! produced by
//! [`CodeBlockGenerateTransform`](super::code_block_generate::CodeBlockGenerateTransform).
//!
//! ## Wrapper stack (single-pass cumulative)
//!
//! `wrap_in_place` walks the decoration once and builds the wrapper
//! stack from innermost to outermost:
//!
//! 1. **Innermost: original CodeBlock.** Generate has already stamped
//!    `code-with-copy` onto its class list when copy is on; the HTML
//!    writer emits the class on the inner `<pre>` (or, for highlighted
//!    blocks, the outer `<div class="sourceCode …">` — either way the
//!    class is present in the rendered DOM).
//! 2. **Phase 1: filename wrapper.** When `decoration.filename` is set,
//!    wrap the inner block in a `<div class="code-with-filename">`
//!    containing the filename-header `RawBlock("html", …)` plus the
//!    inner block. The header markup matches Q1 byte-for-byte so the
//!    ported SCSS (`_quarto-rules-code-filename.scss`) keys off the
//!    exact same selectors.
//! 3. **Phase 2: copy scaffold (outer).** When `decoration.copy.is_on()`,
//!    wrap whatever the stack produced so far in a
//!    `<div class="code-copy-outer-scaffold">` whose children are
//!    `[inner, copy-button RawBlock]`. The button is a sibling of the
//!    inner wrapper / source-code div, matching Q1's TS post-DOM step
//!    (`format-html.ts:746-772`).
//! 4. **Phase 3 (future): `<details>` fold.** Will become the outermost
//!    layer; the single-pass shape extends naturally.
//!
//! Hover-vs-always visibility is controlled by the SCSS variable
//! `$code-copy-selector` ported in Commit 3, not by the markup — both
//! `CopyMode::Hover` and `CopyMode::Always` emit the same scaffold.
//!
//! Pipeline placement: **Finalization Phase**, alongside
//! [`CrossrefRenderTransform`](super::CrossrefRenderTransform) and
//! before [`AttributionRenderTransform`](super::AttributionRenderTransform).

use quarto_pandoc_types::attr::AttrSourceInfo;
use quarto_pandoc_types::block::{Block, Blocks, Div, RawBlock};
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};
use crate::transforms::code_block_generate::{
    CodeBlockDecoration, CodeBlockDecorationKey, decoration_has_any_field,
};

/// See module docs.
pub struct CodeBlockRenderTransform;

impl CodeBlockRenderTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CodeBlockRenderTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for CodeBlockRenderTransform {
    fn name(&self) -> &str {
        "code-block-render"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Finalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        // Short-circuit: nothing to do if Generate produced no
        // decorations. The HashMap lookup overhead would be harmless
        // but the AST walk isn't free.
        if ctx.code_block_decorations.is_empty() {
            return Ok(());
        }

        wrap_decorated_blocks(&mut ast.blocks, &ctx.code_block_decorations);
        Ok(())
    }
}

/// Walk `blocks`, replacing each decorated `CodeBlock` with the
/// appropriate wrapper structure. Descends into containers so
/// decorations attach to nested code blocks too.
///
/// Walks the same container variants as
/// [`super::code_block_generate::CodeBlockGenerateTransform`] — the
/// two must stay in sync.
fn wrap_decorated_blocks(
    blocks: &mut Blocks,
    decorations: &std::collections::HashMap<CodeBlockDecorationKey, CodeBlockDecoration>,
) {
    for block in blocks.iter_mut() {
        match block {
            Block::CodeBlock(cb) => {
                let Some(key) = CodeBlockDecorationKey::from_source_info(&cb.source_info) else {
                    continue;
                };
                let Some(decoration) = decorations.get(&key) else {
                    continue;
                };
                if !decoration_has_any_field(decoration) {
                    continue;
                }
                wrap_in_place(block, decoration);
            }
            Block::BlockQuote(bq) => wrap_decorated_blocks(&mut bq.content, decorations),
            Block::Div(div) => wrap_decorated_blocks(&mut div.content, decorations),
            Block::Figure(fig) => wrap_decorated_blocks(&mut fig.content, decorations),
            Block::OrderedList(list) => {
                for item in list.content.iter_mut() {
                    wrap_decorated_blocks(item, decorations);
                }
            }
            Block::BulletList(list) => {
                for item in list.content.iter_mut() {
                    wrap_decorated_blocks(item, decorations);
                }
            }
            Block::DefinitionList(dl) => {
                for (_term, defs) in dl.content.iter_mut() {
                    for def in defs.iter_mut() {
                        wrap_decorated_blocks(def, decorations);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Replace a `Block::CodeBlock` (already confirmed decorated) in place
/// with the single-pass cumulative wrapper stack described in the
/// module docs.
///
/// Move semantics: the original `CodeBlock` is moved into the stack,
/// so its content (including `data-hl-spans` annotations from
/// `CodeHighlightStage` and the `code-with-copy` class stamped by
/// Generate) is preserved verbatim.
fn wrap_in_place(block: &mut Block, decoration: &CodeBlockDecoration) {
    // `Block` has no `Default`, so we need a real placeholder to swap
    // out the original. A RawBlock("html", "") with the same source
    // info has zero rendered output and zero cost; it's the cheapest
    // sentinel we can construct.
    let source_info = block.source_info().clone();
    let placeholder = Block::RawBlock(RawBlock {
        format: "html".to_string(),
        text: String::new(),
        source_info: source_info.clone(),
    });
    let mut inner: Block = std::mem::replace(block, placeholder);

    // Layer 1 (Phase 1): filename header. When present, wrap `inner`
    // in `<div class="code-with-filename">` with [header, inner].
    if let Some(filename) = decoration.filename.as_ref() {
        inner = wrap_with_filename(inner, filename, source_info.clone());
    }

    // Layer 2 (Phase 2): copy scaffold. When copy is on, wrap whatever
    // we have so far in `<div class="code-copy-outer-scaffold">` whose
    // children are `[inner, button]`. The button is a sibling of the
    // inner wrapper (so it sits next to the sourceCode div), matching
    // Q1's TS post-DOM structure.
    if decoration.copy.is_on() {
        inner = wrap_with_copy_scaffold(inner, source_info.clone());
    }

    // Layer 3 (Phase 3): fold `<details>` — TODO.

    *block = inner;
}

/// Build a `<div class="code-with-filename">` wrapper around `inner`,
/// with the filename header as the first child.
fn wrap_with_filename(
    inner: Block,
    filename: &str,
    source_info: quarto_source_map::SourceInfo,
) -> Block {
    Block::Div(Div {
        attr: (
            String::new(),
            vec!["code-with-filename".to_string()],
            hashlink::LinkedHashMap::new(),
        ),
        content: vec![make_filename_header(filename, source_info.clone()), inner],
        source_info,
        attr_source: AttrSourceInfo::empty(),
    })
}

/// Build a `<div class="code-copy-outer-scaffold">` wrapper around
/// `inner`, appending the copy-button RawBlock as the second child.
/// Hover-vs-always visibility is controlled by the ported SCSS, not
/// by the markup.
fn wrap_with_copy_scaffold(inner: Block, source_info: quarto_source_map::SourceInfo) -> Block {
    Block::Div(Div {
        attr: (
            String::new(),
            vec!["code-copy-outer-scaffold".to_string()],
            hashlink::LinkedHashMap::new(),
        ),
        content: vec![inner, make_copy_button(source_info.clone())],
        source_info,
        attr_source: AttrSourceInfo::empty(),
    })
}

/// Build the copy-button RawBlock. The `title=` attribute drives the
/// Bootstrap-Tooltip popover; `aria-label="Copy code"` provides a
/// screen-reader name (a small a11y improvement over Q1's `title=`-only
/// markup). The `<i class="bi">` element is the bootstrap-icon slot —
/// the actual clipboard icon is painted by SCSS as a `background-image`
/// per `_quarto-rules-copy-code.scss`.
fn make_copy_button(source_info: quarto_source_map::SourceInfo) -> Block {
    // No user-controlled content lands in this string, so a literal
    // template is safe. The title text is the english default; future
    // work will read it from the language table (Q1 looks up
    // `kCopyButtonTooltip` → "Copy to Clipboard").
    let text = "<button title=\"Copy to Clipboard\" \
                class=\"code-copy-button\" \
                aria-label=\"Copy code\">\
                <i class=\"bi\"></i>\
                </button>"
        .to_string();
    Block::RawBlock(RawBlock {
        format: "html".to_string(),
        text,
        source_info,
    })
}

/// Build the filename-header sub-block. Emitted as a `RawBlock("html", …)`
/// so the HTML output matches Q1's
/// `<div class="code-with-filename-file"><pre><strong>filename</strong></pre></div>`
/// byte-for-byte — the ported SCSS keys off that exact structure.
fn make_filename_header(filename: &str, source_info: quarto_source_map::SourceInfo) -> Block {
    // Filename is user-controlled, so escape it for HTML.
    let escaped = html_escape(filename);
    let text = format!(
        "<div class=\"code-with-filename-file\"><pre><strong>{}</strong></pre></div>",
        escaped
    );
    Block::RawBlock(RawBlock {
        format: "html".to_string(),
        text,
        source_info,
    })
}

/// Minimal HTML escape — enough for an attribute / element text
/// value. We deliberately don't pull in a heavyweight HTML library
/// for a string that will never contain anything more exotic than a
/// filename.
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
    use quarto_pandoc_types::block::CodeBlock;
    use quarto_pandoc_types::{ConfigValue, attr::AttrSourceInfo};
    use quarto_source_map::SourceInfo;
    use quarto_source_map::types::FileId;

    fn source_info_at(file: usize, start: usize, end: usize) -> SourceInfo {
        SourceInfo::Original {
            file_id: FileId(file),
            start_offset: start,
            end_offset: end,
        }
    }

    fn make_codeblock(text: &str, kvs: Vec<(&str, &str)>) -> Block {
        let mut kv_map = hashlink::LinkedHashMap::new();
        for (k, v) in kvs {
            kv_map.insert(k.to_string(), v.to_string());
        }
        Block::CodeBlock(CodeBlock {
            attr: (String::new(), vec!["python".to_string()], kv_map),
            text: text.to_string(),
            source_info: source_info_at(0, 0, text.len()),
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

            ..Default::default()
        }
    }

    /// `meta` map carrying a single `code-copy` entry. Used by Phase 1
    /// tests to suppress the doc-default copy scaffold so they can
    /// assert filename behavior in isolation.
    fn meta_with_code_copy(value: quarto_pandoc_types::ConfigValueKind) -> ConfigValue {
        use quarto_pandoc_types::{ConfigMapEntry, ConfigValueKind, MergeOp};
        ConfigValue {
            value: ConfigValueKind::Map(vec![ConfigMapEntry {
                key: "code-copy".to_string(),
                key_source: SourceInfo::for_test(),
                value: ConfigValue {
                    value,
                    source_info: SourceInfo::for_test(),
                    merge_op: MergeOp::Concat,
                },
            }]),
            source_info: SourceInfo::for_test(),
            merge_op: MergeOp::Concat,
        }
    }

    fn meta_code_copy_false() -> ConfigValue {
        meta_with_code_copy(quarto_pandoc_types::ConfigValueKind::Scalar(
            yaml_rust2::Yaml::Boolean(false),
        ))
    }

    /// End-to-end shape test: run Generate then Render on a single
    /// code block with a filename and confirm the resulting AST is
    /// the wrapper Div containing a filename `RawBlock` followed by
    /// the original `CodeBlock`.
    ///
    /// Uses `code-copy: false` so the doc-default copy scaffold
    /// doesn't appear — this test isolates filename-only behavior.
    /// The filename + copy composition case has its own test below.
    #[tokio::test]
    async fn render_wraps_codeblock_with_filename_header() {
        use crate::transforms::CodeBlockGenerateTransform;

        let mut ast = Pandoc {
            meta: meta_code_copy_false(),
            blocks: vec![make_codeblock(
                "print('hi')",
                vec![("filename", "hello.py")],
            )],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        CodeBlockGenerateTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        CodeBlockRenderTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();

        // Top-level block is now the wrapper Div.
        assert_eq!(ast.blocks.len(), 1);
        let Block::Div(wrapper) = &ast.blocks[0] else {
            panic!("expected Block::Div wrapper; got {:?}", ast.blocks[0]);
        };
        assert!(
            wrapper.attr.1.contains(&"code-with-filename".to_string()),
            "wrapper must carry the code-with-filename class; got attrs {:?}",
            wrapper.attr
        );

        // Wrapper has two children: filename header + original code block.
        assert_eq!(wrapper.content.len(), 2);

        // First child is the filename header — a RawBlock with the
        // exact Q1 markup the ported SCSS expects.
        let Block::RawBlock(header) = &wrapper.content[0] else {
            panic!(
                "expected filename header as RawBlock; got {:?}",
                wrapper.content[0]
            );
        };
        assert_eq!(header.format, "html");
        assert_eq!(
            header.text,
            "<div class=\"code-with-filename-file\"><pre><strong>hello.py</strong></pre></div>"
        );

        // Second child is the original CodeBlock untouched (text and
        // attrs preserved).
        let Block::CodeBlock(cb) = &wrapper.content[1] else {
            panic!(
                "expected original CodeBlock as second child; got {:?}",
                wrapper.content[1]
            );
        };
        assert_eq!(cb.text, "print('hi')");
    }

    /// Code blocks without a filename decoration must NOT be wrapped
    /// in the filename Div. Uses `code-copy: false` so the
    /// doc-default copy scaffold doesn't introduce a wrapper either.
    #[tokio::test]
    async fn render_leaves_undecorated_codeblocks_alone() {
        use crate::transforms::CodeBlockGenerateTransform;

        let mut ast = Pandoc {
            meta: meta_code_copy_false(),
            blocks: vec![make_codeblock("print('hi')", vec![])],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        CodeBlockGenerateTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        CodeBlockRenderTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();

        // Still a bare CodeBlock at the top level.
        assert_eq!(ast.blocks.len(), 1);
        assert!(
            matches!(ast.blocks[0], Block::CodeBlock(_)),
            "undecorated code block must not be wrapped; got {:?}",
            ast.blocks[0],
        );
    }

    /// Filename text must be HTML-escaped so user-controlled values
    /// can't inject markup. Defense in depth — `filename` comes from
    /// the user via a kv attribute on the CodeBlock, and the produced
    /// RawBlock passes through to the writer verbatim. Uses
    /// `code-copy: false` to isolate filename behavior.
    #[tokio::test]
    async fn render_html_escapes_filename() {
        use crate::transforms::CodeBlockGenerateTransform;

        let mut ast = Pandoc {
            meta: meta_code_copy_false(),
            blocks: vec![make_codeblock(
                "x",
                vec![("filename", "<script>alert(1)</script>")],
            )],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        CodeBlockGenerateTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        CodeBlockRenderTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();

        let Block::Div(wrapper) = &ast.blocks[0] else {
            panic!("expected wrapper Div");
        };
        let Block::RawBlock(header) = &wrapper.content[0] else {
            panic!("expected RawBlock header");
        };
        assert!(
            !header.text.contains("<script>"),
            "raw <script> must be escaped; got:\n{}",
            header.text,
        );
        assert!(
            header.text.contains("&lt;script&gt;"),
            "expected escaped form; got:\n{}",
            header.text,
        );
    }

    // ── Phase 2: copy-button scaffold ─────────────────────────────────

    /// Under default Hover, a bare code block gets wrapped in the
    /// `code-copy-outer-scaffold` Div containing [original CodeBlock,
    /// copy-button RawBlock]. The inner CodeBlock carries the
    /// `code-with-copy` class added by Generate.
    #[tokio::test]
    async fn render_wraps_codeblock_with_copy_scaffold_under_default_hover() {
        use crate::transforms::CodeBlockGenerateTransform;

        let mut ast = Pandoc {
            meta: ConfigValue::default(), // unset → default Hover
            blocks: vec![make_codeblock("print('hi')", vec![])],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        CodeBlockGenerateTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        CodeBlockRenderTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();

        assert_eq!(ast.blocks.len(), 1);
        let Block::Div(scaffold) = &ast.blocks[0] else {
            panic!("expected outer copy-scaffold Div; got {:?}", ast.blocks[0]);
        };
        assert!(
            scaffold
                .attr
                .1
                .contains(&"code-copy-outer-scaffold".to_string()),
            "outer wrapper must carry code-copy-outer-scaffold; got attrs {:?}",
            scaffold.attr,
        );

        // Scaffold has two children: the original CodeBlock and the
        // copy-button RawBlock (in that order, so the button is a
        // sibling of the sourceCode div in the rendered HTML).
        assert_eq!(scaffold.content.len(), 2);

        let Block::CodeBlock(cb) = &scaffold.content[0] else {
            panic!(
                "first scaffold child must be the original CodeBlock; got {:?}",
                scaffold.content[0]
            );
        };
        assert_eq!(cb.text, "print('hi')");
        assert!(
            cb.attr.1.contains(&"code-with-copy".to_string()),
            "inner CodeBlock must carry the code-with-copy class added by Generate; \
             got classes {:?}",
            cb.attr.1,
        );

        let Block::RawBlock(button) = &scaffold.content[1] else {
            panic!(
                "second scaffold child must be the button RawBlock; got {:?}",
                scaffold.content[1]
            );
        };
        assert_eq!(button.format, "html");
        assert!(
            button.text.contains("code-copy-button"),
            "button RawBlock must contain class=\"code-copy-button\"; got {:?}",
            button.text,
        );
    }

    /// `code-copy: false` at doc level fully disables the copy
    /// scaffold. The block is left untouched.
    #[tokio::test]
    async fn render_omits_copy_scaffold_when_code_copy_false() {
        use crate::transforms::CodeBlockGenerateTransform;

        let mut ast = Pandoc {
            meta: meta_code_copy_false(),
            blocks: vec![make_codeblock("print('hi')", vec![])],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        CodeBlockGenerateTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        CodeBlockRenderTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();

        assert_eq!(ast.blocks.len(), 1);
        let Block::CodeBlock(cb) = &ast.blocks[0] else {
            panic!(
                "code-copy: false should leave the CodeBlock bare; got {:?}",
                ast.blocks[0]
            );
        };
        assert!(
            !cb.attr.1.contains(&"code-with-copy".to_string()),
            "code-with-copy class must NOT be added when copy is off; got {:?}",
            cb.attr.1,
        );
    }

    /// Composition: filename + default Hover → outermost wrapper is
    /// `code-copy-outer-scaffold`, then `code-with-filename`, then
    /// the [filename header, original CodeBlock]. The button is the
    /// second child of the scaffold, sibling of the filename wrapper.
    #[tokio::test]
    async fn render_composes_filename_inside_copy_scaffold() {
        use crate::transforms::CodeBlockGenerateTransform;

        let mut ast = Pandoc {
            meta: ConfigValue::default(), // default Hover
            blocks: vec![make_codeblock(
                "print('hi')",
                vec![("filename", "hello.py")],
            )],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        CodeBlockGenerateTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        CodeBlockRenderTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();

        // Outermost: code-copy-outer-scaffold.
        let Block::Div(scaffold) = &ast.blocks[0] else {
            panic!("expected outer scaffold Div; got {:?}", ast.blocks[0]);
        };
        assert!(
            scaffold
                .attr
                .1
                .contains(&"code-copy-outer-scaffold".to_string()),
            "outermost class must be code-copy-outer-scaffold; got {:?}",
            scaffold.attr.1,
        );
        assert_eq!(scaffold.content.len(), 2);

        // First child: the filename wrapper.
        let Block::Div(filename_wrapper) = &scaffold.content[0] else {
            panic!(
                "first scaffold child must be the filename wrapper Div; got {:?}",
                scaffold.content[0]
            );
        };
        assert!(
            filename_wrapper
                .attr
                .1
                .contains(&"code-with-filename".to_string()),
            "filename wrapper must carry code-with-filename; got {:?}",
            filename_wrapper.attr.1,
        );

        // Filename wrapper has [header RawBlock, original CodeBlock].
        assert_eq!(filename_wrapper.content.len(), 2);
        let Block::RawBlock(header) = &filename_wrapper.content[0] else {
            panic!(
                "first filename-wrapper child must be the header RawBlock; got {:?}",
                filename_wrapper.content[0]
            );
        };
        assert!(
            header.text.contains("hello.py"),
            "header must contain the filename text; got {:?}",
            header.text,
        );
        let Block::CodeBlock(cb) = &filename_wrapper.content[1] else {
            panic!(
                "second filename-wrapper child must be the original CodeBlock; got {:?}",
                filename_wrapper.content[1]
            );
        };
        assert!(
            cb.attr.1.contains(&"code-with-copy".to_string()),
            "innermost CodeBlock must still carry code-with-copy; got {:?}",
            cb.attr.1,
        );

        // Second child of scaffold: the button RawBlock.
        let Block::RawBlock(button) = &scaffold.content[1] else {
            panic!(
                "second scaffold child must be the button RawBlock; got {:?}",
                scaffold.content[1]
            );
        };
        assert!(button.text.contains("code-copy-button"));
    }

    /// The copy-button markup must include both Q1's `title=`
    /// attribute (driving the tooltip) and the `aria-label` added by
    /// Q2 for screen-reader users. The icon element is a Bootstrap
    /// `<i class="bi">` whose actual image is painted by SCSS.
    #[tokio::test]
    async fn render_copy_button_markup_carries_a11y_attrs() {
        use crate::transforms::CodeBlockGenerateTransform;

        let mut ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![make_codeblock("x", vec![])],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        CodeBlockGenerateTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        CodeBlockRenderTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();

        let Block::Div(scaffold) = &ast.blocks[0] else {
            panic!("expected scaffold Div");
        };
        let Block::RawBlock(button) = &scaffold.content[1] else {
            panic!("expected button RawBlock at index 1");
        };
        assert!(
            button.text.contains("title=\""),
            "button must carry a title= attribute; got {:?}",
            button.text,
        );
        assert!(
            button.text.contains("aria-label=\"Copy code\""),
            "button must carry aria-label=\"Copy code\"; got {:?}",
            button.text,
        );
        assert!(
            button.text.contains("<i class=\"bi\""),
            "button must contain the Bootstrap-icon span; got {:?}",
            button.text,
        );
    }
}
