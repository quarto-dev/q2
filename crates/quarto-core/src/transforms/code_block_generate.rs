/*
 * transforms/code_block_generate.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Code-block decoration *Generate* transform (Phase 0 skeleton).
//!
//! Format-agnostic half of the code-block decoration pipeline. Walks
//! every [`CodeBlock`](quarto_pandoc_types::block::CodeBlock) in the
//! document, parses its attributes plus the relevant document-level
//! defaults from `ast.meta`, and produces a typed
//! [`CodeBlockDecoration`] payload that [`CodeBlockRenderTransform`]
//! (see [`super::code_block_render`]) consumes.
//!
//! Today the payload is empty — this transform exists to lock in the
//! pipeline placement and the Generate/Render shape before Phases 1–3
//! (filename / copy / fold) add real fields. The walk runs in
//! `O(blocks)` and produces no AST mutation; for Phase 0 it is a
//! deliberate no-op end-to-end.
//!
//! Pipeline placement: **Normalization Phase**, after
//! [`MetadataNormalizeTransform`](super::MetadataNormalizeTransform)
//! so document-level defaults (e.g. `code-copy: true`) are visible
//! when computing per-block decorations.
//!
//! See `claude-notes/plans/2026-05-19-code-block-features.md` for the
//! full epic plan and the Phase 0 acceptance criteria.

use quarto_pandoc_types::block::{Block, Blocks};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceInfo;
use quarto_source_map::types::FileId;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::AstTransform;

/// Typed payload carrying everything format renderers need to wrap a
/// `CodeBlock` in the right outer structure (filename header, copy
/// button, fold details, etc.).
///
/// Storage shape: a sideband
/// [`HashMap<CodeBlockDecorationKey, CodeBlockDecoration>`](CodeBlockDecorationKey)
/// on [`RenderContext::code_block_decorations`](crate::render::RenderContext)
/// (decision pinned in
/// `claude-notes/plans/2026-05-19-code-block-features.md`). Generate
/// populates the map; Render reads it. Both transforms run inside
/// `AstTransformsStage` so they share the same `RenderContext` —
/// no `StageContext` bridge needed.
///
/// Per-feature fields land in Phases 1 – 3:
/// - Phase 1 (filename): see `filename` below.
/// - Phase 2 (copy): see `copy` below.
/// - Phase 3 (fold): `pub fold: FoldMode`, `pub summary: Option<String>`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodeBlockDecoration {
    /// Phase 1: the value of the `filename` attribute (or the
    /// `#| filename:` chunk option). `None` when no filename was
    /// declared; in that case Render emits no filename header.
    pub filename: Option<String>,
    /// Phase 2: copy-button mode. Resolved from document-level
    /// `code-copy` metadata (Q1 supports doc-level only, no per-block
    /// override). [`CopyMode::Off`] suppresses the copy scaffold;
    /// [`CopyMode::Hover`] and [`CopyMode::Always`] both emit the
    /// scaffold and only differ in the ported SCSS selector
    /// (`$code-copy-selector`) that controls button visibility.
    pub copy: CopyMode,
    // Phase 3: pub fold: FoldMode, pub summary: Option<String>
}

/// Resolution of the document-level `code-copy` metadata key.
///
/// Q1's logic (see `format-html.ts:710-712` and the
/// `$code-copy-selector` defaults block in `format-html-shared.ts:317-322`):
///
/// - explicit `code-copy: false` → off entirely.
/// - explicit `code-copy: true` or `code-copy: "always"` → always-visible.
/// - explicit `code-copy: "hover"` or unset → hover-only.
///
/// Q2 mirrors that mapping. The only difference between `Hover` and
/// `Always` at AST-rewrite time is which CSS rule paints the button
/// visible; the wrapper markup is identical.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CopyMode {
    /// No copy scaffold or button is emitted for this block.
    Off,
    /// Wrapper + button are emitted; SCSS shows the button only on
    /// hover of the outer scaffold. Q1's default and Q2's.
    #[default]
    Hover,
    /// Wrapper + button are emitted; SCSS shows the button at all
    /// times. Selected by `code-copy: true` or `code-copy: "always"`.
    Always,
}

impl CopyMode {
    /// `true` when the copy scaffold (outer Div + button RawBlock)
    /// should be emitted around the code block. Hover and Always
    /// both require the scaffold; only the SCSS selector differs.
    pub fn is_on(self) -> bool {
        matches!(self, CopyMode::Hover | CopyMode::Always)
    }
}

/// Resolve a [`CopyMode`] from a document's `meta`, applying Q1's
/// truthy/string rules for the `code-copy` key.
///
/// Returns [`CopyMode::Hover`] when the key is absent — Q1's default.
/// Returns [`CopyMode::Off`] only for an explicit `false`. Any
/// truthy or non-`"hover"` string value resolves to [`CopyMode::Always`].
pub fn resolve_default_copy_mode(meta: &quarto_pandoc_types::ConfigValue) -> CopyMode {
    let Some(value) = meta.get("code-copy") else {
        return CopyMode::Hover;
    };
    if let Some(b) = value.as_bool() {
        return if b { CopyMode::Always } else { CopyMode::Off };
    }
    // `as_plain_text` (not `as_str`): a bare `code-copy: always` (or `hover`)
    // front-matter string is stored as `ConfigValueKind::PandocInlines`, for
    // which `as_str` returns `None`, silently downgrading `always` to the
    // `hover` fallback. (bd-y89ihf0i)
    if let Some(s) = value.as_plain_text() {
        return match s.as_str() {
            "hover" => CopyMode::Hover,
            "false" => CopyMode::Off,
            "true" | "always" => CopyMode::Always,
            // Unknown string — fall back to hover. Q1 doesn't validate
            // this either; the SCSS selector branch treats anything
            // other than "hover" / undefined as always-visible, but
            // we prefer the safer default. Worth a diagnostic in a
            // future polish pass.
            _ => CopyMode::Hover,
        };
    }
    CopyMode::Hover
}

/// Stable identity used to key
/// [`RenderContext::code_block_decorations`](crate::render::RenderContext)
/// across the Generate → Render boundary.
///
/// Derived from a `CodeBlock`'s [`SourceInfo`], using the underlying
/// `Original` variant's `(file_id, start_offset, end_offset)` triple.
/// The triple is stable through every subsequent transform that
/// leaves the source-tracking intact, which is every transform in
/// the current pipeline — none of them rewrite a code block's
/// `source_info`.
///
/// **Non-`Original` variants are skipped at decoration time.**
/// `Substring` and `Concat` are inline-text artefacts that effectively
/// never occur on `Block::CodeBlock`. `FilterProvenance` applies to
/// elements created from inside a Lua filter; the user-filter slot
/// brackets the entire `AstTransformsStage`, so any filter-created
/// `CodeBlock` enters the pipeline either before Generate or after
/// Render — never between them, where the key would matter.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct CodeBlockDecorationKey {
    pub file_id: FileId,
    pub start_offset: usize,
    pub end_offset: usize,
}

impl CodeBlockDecorationKey {
    /// Build a key from a `SourceInfo`. Returns `None` when the
    /// source info isn't an `Original` (see struct docs).
    pub fn from_source_info(si: &SourceInfo) -> Option<Self> {
        match si {
            SourceInfo::Original {
                file_id,
                start_offset,
                end_offset,
            } => Some(Self {
                file_id: *file_id,
                start_offset: *start_offset,
                end_offset: *end_offset,
            }),
            _ => None,
        }
    }
}

/// See module docs.
pub struct CodeBlockGenerateTransform;

impl CodeBlockGenerateTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CodeBlockGenerateTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for CodeBlockGenerateTransform {
    fn name(&self) -> &str {
        "code-block-generate"
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        // Resolve document-level defaults once. Phase 2 only needs
        // `code-copy`; future phases will join more keys here (e.g.
        // `code-fold`, `code-line-numbers`).
        let doc_default_copy = resolve_default_copy_mode(&ast.meta);

        walk_blocks_mut(&mut ast.blocks, &mut |block| {
            let Block::CodeBlock(cb) = block else {
                return;
            };
            // Build a decoration from the per-block attributes and the
            // resolved document-level defaults. Phase 1 reads only
            // `filename` per-block; Phase 2 takes copy from the
            // document default (Q1 supports no per-block override).
            // Future phases will extend this match.
            let filename = cb.attr.2.get("filename").map(|s| s.clone());

            let decoration = CodeBlockDecoration {
                filename,
                copy: doc_default_copy,
            };

            // Skip blocks that produced no actual decoration — keeps
            // the sideband map sparse so Render can short-circuit on
            // empty.
            if !decoration_has_any_field(&decoration) {
                return;
            }

            // Phase 2: stamp the `code-with-copy` class onto the block's
            // class list when copy is on. Quarto 1's TS post-DOM step
            // adds this on the `<pre>` element directly. In Q2's
            // Pandoc-style writer, non-language classes land on the
            // outer `<div class="sourceCode …">` for highlighted blocks
            // and on the `<pre>` for unhighlighted blocks — either way
            // the class is present in the rendered DOM, which is what
            // any downstream consumer would query for. (Nothing in
            // Q2's pipeline keys off the class today; Q1 only references
            // it from a Lua filter we don't ship.)
            if decoration.copy.is_on() && !cb.attr.1.iter().any(|c| c == "code-with-copy") {
                cb.attr.1.push("code-with-copy".to_string());
            }

            // Source-info → key. Non-`Original` variants (Substring,
            // Concat, FilterProvenance) currently can't happen on
            // block-level `CodeBlock`; if they ever do, we skip
            // decoration for those blocks rather than panicking.
            let Some(key) = CodeBlockDecorationKey::from_source_info(&cb.source_info) else {
                return;
            };

            ctx.code_block_decorations.insert(key, decoration);
        });
        Ok(())
    }
}

/// Returns `true` when at least one decoration-triggering field is
/// populated. Render uses this both to short-circuit the AST walk
/// when nothing is decorated and to skip individual blocks that
/// landed in the sideband but turned out to have no actionable
/// content (defensive — Generate already filters with the same
/// predicate). Phase 3 will extend the disjunction with `fold`.
pub(crate) fn decoration_has_any_field(d: &CodeBlockDecoration) -> bool {
    d.filename.is_some() || d.copy.is_on()
}

/// Walk every `CodeBlock` in the document, descending into containers
/// (BlockQuote, Div, list items, table cells, figure body, …) so
/// decorations attach to nested code blocks too.
///
/// Visits every `Block::CodeBlock` reachable from `blocks` exactly
/// once. Containers we walk into are kept in sync with the structural
/// shape of `Block` — additions to that enum that can contain code
/// blocks must extend the match arms here.
fn walk_blocks_mut(blocks: &mut Blocks, f: &mut impl FnMut(&mut Block)) {
    for block in blocks.iter_mut() {
        match block {
            Block::CodeBlock(_) => f(block),
            Block::BlockQuote(bq) => walk_blocks_mut(&mut bq.content, f),
            Block::Div(div) => walk_blocks_mut(&mut div.content, f),
            Block::Figure(fig) => walk_blocks_mut(&mut fig.content, f),
            Block::OrderedList(list) => {
                for item in list.content.iter_mut() {
                    walk_blocks_mut(item, f);
                }
            }
            Block::BulletList(list) => {
                for item in list.content.iter_mut() {
                    walk_blocks_mut(item, f);
                }
            }
            Block::DefinitionList(dl) => {
                for (_term, defs) in dl.content.iter_mut() {
                    for def in defs.iter_mut() {
                        walk_blocks_mut(def, f);
                    }
                }
            }
            // Other block variants cannot contain a CodeBlock as a
            // direct child. Header / Paragraph / Plain / LineBlock /
            // RawBlock / HorizontalRule / Table / MetaBlock / Note
            // variants / CaptionBlock / Custom are all leaf-ish from
            // the perspective of block-level code-block discovery.
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use crate::render::BinaryDependencies;
    use quarto_pandoc_types::block::CodeBlock;
    use quarto_pandoc_types::{ConfigValue, attr::AttrSourceInfo};
    use std::sync::Arc;

    fn source_info_at(file: usize, start: usize, end: usize) -> SourceInfo {
        SourceInfo::Original {
            file_id: FileId(file),
            start_offset: start,
            end_offset: end,
        }
    }

    fn make_codeblock(text: &str, classes: Vec<&str>, kvs: Vec<(&str, &str)>) -> Block {
        let mut kv_map = hashlink::LinkedHashMap::new();
        for (k, v) in kvs {
            kv_map.insert(k.to_string(), v.to_string());
        }
        Block::CodeBlock(CodeBlock {
            attr: (
                String::new(),
                classes.into_iter().map(String::from).collect(),
                kv_map,
            ),
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
        }
    }

    #[tokio::test]
    async fn generate_populates_filename_decoration() {
        let mut ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![make_codeblock(
                "print('hi')",
                vec!["python"],
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

        assert_eq!(ctx.code_block_decorations.len(), 1);
        let key = CodeBlockDecorationKey::from_source_info(ast.blocks[0].source_info()).unwrap();
        let deco = ctx.code_block_decorations.get(&key).unwrap();
        assert_eq!(deco.filename.as_deref(), Some("hello.py"));
    }

    /// Builds a `meta` map carrying a single `code-copy` entry. Helper
    /// for the Phase 2 doc-default resolution tests below.
    fn meta_with_code_copy(value: quarto_pandoc_types::ConfigValueKind) -> ConfigValue {
        use quarto_pandoc_types::{ConfigMapEntry, ConfigValue, ConfigValueKind, MergeOp};
        ConfigValue {
            value: ConfigValueKind::Map(vec![ConfigMapEntry {
                key: "code-copy".to_string(),
                key_source: SourceInfo::default(),
                value: ConfigValue {
                    value,
                    source_info: SourceInfo::default(),
                    merge_op: MergeOp::Concat,
                },
            }]),
            source_info: SourceInfo::default(),
            merge_op: MergeOp::Concat,
        }
    }

    fn yaml_bool(b: bool) -> quarto_pandoc_types::ConfigValueKind {
        quarto_pandoc_types::ConfigValueKind::Scalar(yaml_rust2::Yaml::Boolean(b))
    }

    fn yaml_str(s: &str) -> quarto_pandoc_types::ConfigValueKind {
        quarto_pandoc_types::ConfigValueKind::Scalar(yaml_rust2::Yaml::String(s.to_string()))
    }

    /// A `code-copy` value as it is actually stored after a bare front-matter
    /// string is parsed as markdown: `PandocInlines`, not `Scalar(String)`.
    fn yaml_inlines(s: &str) -> quarto_pandoc_types::ConfigValueKind {
        use quarto_pandoc_types::inline::{Inline, Str};
        quarto_pandoc_types::ConfigValueKind::PandocInlines(vec![Inline::Str(Str {
            text: s.to_string(),
            source_info: quarto_source_map::SourceInfo::default(),
        })])
    }

    #[tokio::test]
    async fn generate_decorates_bare_codeblock_under_default_copy_hover() {
        // Phase 2: with `code-copy` unset, the doc default is Hover.
        // Every code block gets a decoration with `copy: Hover`, even
        // without per-block attributes — Q1's behavior.
        let mut ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![make_codeblock("print('hi')", vec!["python"], vec![])],
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

        assert_eq!(ctx.code_block_decorations.len(), 1);
        let key = CodeBlockDecorationKey::from_source_info(ast.blocks[0].source_info()).unwrap();
        let deco = ctx.code_block_decorations.get(&key).unwrap();
        assert_eq!(deco.copy, CopyMode::Hover);
        assert!(deco.filename.is_none());
    }

    #[tokio::test]
    async fn generate_skips_bare_codeblock_when_code_copy_false() {
        // Phase 2: explicit `code-copy: false` at doc level turns off
        // the copy scaffold. With no other decoration triggers, the
        // sideband stays empty.
        let mut ast = Pandoc {
            meta: meta_with_code_copy(yaml_bool(false)),
            blocks: vec![make_codeblock("print('hi')", vec!["python"], vec![])],
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

        assert!(
            ctx.code_block_decorations.is_empty(),
            "code-copy: false + no other decorations should leave sideband empty; got {:?}",
            ctx.code_block_decorations,
        );
    }

    #[tokio::test]
    async fn generate_resolves_code_copy_true_to_always() {
        let mut ast = Pandoc {
            meta: meta_with_code_copy(yaml_bool(true)),
            blocks: vec![make_codeblock("print('hi')", vec!["python"], vec![])],
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

        let key = CodeBlockDecorationKey::from_source_info(ast.blocks[0].source_info()).unwrap();
        let deco = ctx.code_block_decorations.get(&key).unwrap();
        assert_eq!(deco.copy, CopyMode::Always);
    }

    #[tokio::test]
    async fn generate_resolves_code_copy_hover_string_to_hover() {
        let mut ast = Pandoc {
            meta: meta_with_code_copy(yaml_str("hover")),
            blocks: vec![make_codeblock("print('hi')", vec!["python"], vec![])],
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

        let key = CodeBlockDecorationKey::from_source_info(ast.blocks[0].source_info()).unwrap();
        let deco = ctx.code_block_decorations.get(&key).unwrap();
        assert_eq!(deco.copy, CopyMode::Hover);
    }

    #[tokio::test]
    async fn generate_resolves_code_copy_always_string_to_always() {
        let mut ast = Pandoc {
            meta: meta_with_code_copy(yaml_str("always")),
            blocks: vec![make_codeblock("print('hi')", vec!["python"], vec![])],
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

        let key = CodeBlockDecorationKey::from_source_info(ast.blocks[0].source_info()).unwrap();
        let deco = ctx.code_block_decorations.get(&key).unwrap();
        assert_eq!(deco.copy, CopyMode::Always);
    }

    #[tokio::test]
    async fn generate_populates_filename_and_copy_together() {
        // When both a per-block filename and the default copy mode are
        // active, the decoration carries both fields so Render can
        // emit the cumulative wrapper stack in a single pass.
        let mut ast = Pandoc {
            meta: ConfigValue::default(), // unset → default Hover
            blocks: vec![make_codeblock(
                "print('hi')",
                vec!["python"],
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

        let key = CodeBlockDecorationKey::from_source_info(ast.blocks[0].source_info()).unwrap();
        let deco = ctx.code_block_decorations.get(&key).unwrap();
        assert_eq!(deco.filename.as_deref(), Some("hello.py"));
        assert_eq!(deco.copy, CopyMode::Hover);
    }

    // ── resolve_default_copy_mode (unit) ──────────────────────────────

    #[test]
    fn resolve_default_copy_unset_is_hover() {
        let meta = ConfigValue::default();
        assert_eq!(resolve_default_copy_mode(&meta), CopyMode::Hover);
    }

    #[test]
    fn resolve_default_copy_false_is_off() {
        let meta = meta_with_code_copy(yaml_bool(false));
        assert_eq!(resolve_default_copy_mode(&meta), CopyMode::Off);
    }

    #[test]
    fn resolve_default_copy_true_is_always() {
        let meta = meta_with_code_copy(yaml_bool(true));
        assert_eq!(resolve_default_copy_mode(&meta), CopyMode::Always);
    }

    #[test]
    fn resolve_default_copy_hover_string_is_hover() {
        let meta = meta_with_code_copy(yaml_str("hover"));
        assert_eq!(resolve_default_copy_mode(&meta), CopyMode::Hover);
    }

    #[test]
    fn resolve_default_copy_always_string_is_always() {
        let meta = meta_with_code_copy(yaml_str("always"));
        assert_eq!(resolve_default_copy_mode(&meta), CopyMode::Always);
    }

    /// bd-y89ihf0i: in real document-metadata context a bare
    /// `code-copy: always` is parsed as markdown and stored as
    /// `PandocInlines`, NOT `Scalar(String)`. With `as_str()` this resolved to
    /// `None` and silently fell through to the `hover` default; `as_plain_text`
    /// resolves it to `always`.
    ///
    /// NOTE: the Hover→Always distinction has no end-to-end HTML observable
    /// today — both emit identical markup and Q2 hardcodes the hover
    /// `$code-copy-selector` in SCSS (see `resources/scss/bootstrap/
    /// _bootstrap-rules.scss`). This unit test is therefore the mechanical
    /// regression for the accessor fix; an e2e test cannot distinguish the
    /// modes until the selector is wired from the document option.
    #[test]
    fn resolve_default_copy_always_inlines_is_always() {
        let meta = meta_with_code_copy(yaml_inlines("always"));
        assert_eq!(resolve_default_copy_mode(&meta), CopyMode::Always);
    }

    #[test]
    fn copy_mode_is_on() {
        assert!(!CopyMode::Off.is_on());
        assert!(CopyMode::Hover.is_on());
        assert!(CopyMode::Always.is_on());
    }

    // ── Phase 2 Commit 2: code-with-copy class on the CodeBlock ──────

    #[tokio::test]
    async fn generate_adds_code_with_copy_class_when_copy_on() {
        // Under default Hover, Generate adds "code-with-copy" to the
        // CodeBlock's class list. Quarto 1's TS post-DOM step does
        // this on the `<pre>` element; Q2 does it on the AST so the
        // class flows through the HTML writer.
        let mut ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![make_codeblock("print('hi')", vec!["python"], vec![])],
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

        let Block::CodeBlock(cb) = &ast.blocks[0] else {
            panic!("expected CodeBlock");
        };
        assert!(
            cb.attr.1.contains(&"code-with-copy".to_string()),
            "code-with-copy must be added to attr.1 when copy is on; got {:?}",
            cb.attr.1,
        );
    }

    #[tokio::test]
    async fn generate_omits_code_with_copy_class_when_copy_off() {
        let mut ast = Pandoc {
            meta: meta_with_code_copy(yaml_bool(false)),
            blocks: vec![make_codeblock("print('hi')", vec!["python"], vec![])],
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

        let Block::CodeBlock(cb) = &ast.blocks[0] else {
            panic!("expected CodeBlock");
        };
        assert!(
            !cb.attr.1.contains(&"code-with-copy".to_string()),
            "code-with-copy must NOT be added when copy is off; got {:?}",
            cb.attr.1,
        );
    }

    #[tokio::test]
    async fn generate_does_not_duplicate_code_with_copy_class() {
        // If a user has somehow pre-marked a block with code-with-copy
        // (filters, manual attr), Generate must not duplicate it.
        let mut ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![make_codeblock(
                "print('hi')",
                vec!["python", "code-with-copy"],
                vec![],
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

        let Block::CodeBlock(cb) = &ast.blocks[0] else {
            panic!("expected CodeBlock");
        };
        let count = cb.attr.1.iter().filter(|c| *c == "code-with-copy").count();
        assert_eq!(
            count, 1,
            "code-with-copy must appear exactly once after Generate; got {:?}",
            cb.attr.1,
        );
    }

    #[tokio::test]
    async fn generate_visits_nested_codeblocks() {
        use quarto_pandoc_types::block::{BlockQuote, Div};

        // Give the two inner code blocks distinct source ranges so
        // their decoration keys don't collide.
        let mut cb_inside_blockquote =
            make_codeblock("x = 1", vec!["python"], vec![("filename", "inner.py")]);
        if let Block::CodeBlock(cb) = &mut cb_inside_blockquote {
            cb.source_info = source_info_at(0, 10, 50);
        }
        let mut cb_inside_div =
            make_codeblock("y = 2", vec!["python"], vec![("filename", "deeper.py")]);
        if let Block::CodeBlock(cb) = &mut cb_inside_div {
            cb.source_info = source_info_at(0, 150, 180);
        }

        let mut ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![Block::BlockQuote(BlockQuote {
                content: vec![
                    cb_inside_blockquote,
                    Block::Div(Div {
                        attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
                        content: vec![cb_inside_div],
                        source_info: source_info_at(0, 100, 200),
                        attr_source: AttrSourceInfo::empty(),
                    }),
                ],
                source_info: source_info_at(0, 0, 300),
            })],
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

        let filenames: std::collections::HashSet<_> = ctx
            .code_block_decorations
            .values()
            .filter_map(|d| d.filename.clone())
            .collect();
        assert!(filenames.contains("inner.py"), "got {filenames:?}");
        assert!(filenames.contains("deeper.py"), "got {filenames:?}");
    }

    #[test]
    fn decoration_key_from_original_source_info() {
        let si = SourceInfo::Original {
            file_id: FileId(7),
            start_offset: 100,
            end_offset: 200,
        };
        let key = CodeBlockDecorationKey::from_source_info(&si).unwrap();
        assert_eq!(key.file_id, FileId(7));
        assert_eq!(key.start_offset, 100);
        assert_eq!(key.end_offset, 200);
    }

    #[test]
    fn decoration_key_skips_non_original_variants() {
        // Substring / Concat / FilterProvenance return None — see the
        // docs on `CodeBlockDecorationKey` for the timing argument
        // that makes this safe in practice.
        let original = Arc::new(SourceInfo::Original {
            file_id: FileId(1),
            start_offset: 0,
            end_offset: 10,
        });
        let substring = SourceInfo::Substring {
            parent: original.clone(),
            start_offset: 2,
            end_offset: 5,
        };
        assert!(CodeBlockDecorationKey::from_source_info(&substring).is_none());

        let concat = SourceInfo::Concat { pieces: vec![] };
        assert!(CodeBlockDecorationKey::from_source_info(&concat).is_none());

        let filter = SourceInfo::FilterProvenance {
            filter_path: "fixture.lua".into(),
            line: 1,
        };
        assert!(CodeBlockDecorationKey::from_source_info(&filter).is_none());
    }

    #[test]
    fn decoration_key_is_hash_eq_for_same_triple() {
        // The key is the load-bearing identity across the
        // Generate → Render boundary; two source infos with the same
        // `(file_id, start, end)` triple must hash and compare equal.
        let a = CodeBlockDecorationKey::from_source_info(&SourceInfo::Original {
            file_id: FileId(3),
            start_offset: 42,
            end_offset: 99,
        })
        .unwrap();
        let b = CodeBlockDecorationKey::from_source_info(&SourceInfo::Original {
            file_id: FileId(3),
            start_offset: 42,
            end_offset: 99,
        })
        .unwrap();
        assert_eq!(a, b);

        let mut map = std::collections::HashMap::new();
        map.insert(a, CodeBlockDecoration::default());
        assert!(map.contains_key(&b));
    }
}
