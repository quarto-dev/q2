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
use quarto_pandoc_types::custom::Slot;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceInfo;
use quarto_source_map::types::FileId;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

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
/// `AstTransformsStage` so they share the same `RenderContext` within
/// one stage-list call; across a book chapter's pause/resume split
/// (book-projects P5) the map survives on the `StageContext` bridge
/// between the two calls.
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
    /// Phase 3: fold mode for an executed cell's source block. Resolved
    /// from the block's `code-fold` attribute, falling back to the
    /// document-level `code-fold` key. Always [`FoldMode::None`] for
    /// blocks without the `cell-code` class and for formats that are
    /// not HTML-based, matching Q1's `foldcode.lua`.
    pub fold: FoldMode,
    /// Phase 3: text of the `<summary>` element. `Some` exactly when
    /// `fold` is not [`FoldMode::None`]. Resolved from the block's
    /// `code-summary` attribute, then the document-level
    /// `code-summary`, then Q1's default `"Code"`.
    pub summary: Option<String>,
    /// Phase 3: the block carried the `hidden` class, which Q1 copies
    /// onto the `<details>` element so a hidden cell's fold is hidden
    /// with it.
    pub fold_hidden: bool,
}

/// Resolution of the `code-fold` option for one code block.
///
/// Q1's mapping (`foldAttribute` in `quarto-post/foldcode.lua`):
///
/// - `true`, `"true"`, `"1"` → folded and closed.
/// - `false`, `"false"`, `"0"`, unset → not folded.
/// - `"show"` → folded and open.
/// - any other string → folded and closed (Q1 only tests for `"show"`
///   when it writes the `open` attribute).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FoldMode {
    /// No `<details>` wrapper.
    #[default]
    None,
    /// `<details class="code-fold">`, closed on load.
    Hide,
    /// `<details open class="code-fold">`, open on load.
    Show,
}

impl FoldMode {
    /// `true` when a `<details>` wrapper should be emitted.
    pub fn is_on(self) -> bool {
        !matches!(self, FoldMode::None)
    }

    /// Parse the string form of a `code-fold` value (a block attribute
    /// or a document-level string).
    pub fn from_attr(value: &str) -> Self {
        match value.trim() {
            "" | "false" | "0" | "none" => FoldMode::None,
            "show" => FoldMode::Show,
            _ => FoldMode::Hide,
        }
    }
}

/// Q1's default `<summary>` text (`summaryAttribute` in `foldcode.lua`).
pub const DEFAULT_CODE_SUMMARY: &str = "Code";

/// Resolve the document-level `code-fold` default. Unset is
/// [`FoldMode::None`].
pub fn resolve_default_fold_mode(meta: &quarto_pandoc_types::ConfigValue) -> FoldMode {
    let Some(value) = meta.get("code-fold") else {
        return FoldMode::None;
    };
    if let Some(b) = value.as_bool() {
        return if b { FoldMode::Hide } else { FoldMode::None };
    }
    // `as_plain_text`, not `as_str`: a bare `code-fold: show` is stored
    // as `PandocInlines` (see `resolve_default_copy_mode`).
    match value.as_plain_text() {
        Some(s) => FoldMode::from_attr(&s),
        None => FoldMode::None,
    }
}

/// Resolve the document-level `code-summary` default, or
/// [`DEFAULT_CODE_SUMMARY`] when the key is unset or empty.
pub fn resolve_default_code_summary(meta: &quarto_pandoc_types::ConfigValue) -> String {
    meta.get("code-summary")
        .and_then(|v| v.as_plain_text())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_CODE_SUMMARY.to_string())
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

/// Resolution of `code-line-numbers` for one block
/// (`lineNumbersAttribute` in Q1's `quarto-pre/line-numbers.lua`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineNumbers {
    /// No numbering.
    Off,
    /// Number every line.
    On,
    /// A string other than a boolean: a line spec such as `"2,4"` or
    /// `"|1|2"`. Numbers the block; Reveal also highlights from it.
    Spec(String),
}

impl LineNumbers {
    pub fn from_bool(b: bool) -> Self {
        if b { LineNumbers::On } else { LineNumbers::Off }
    }

    /// Parse the string form. `"1"` means `true` except in formats that
    /// accept line specs (`accept_specs`, Reveal), where it is line 1.
    pub fn from_attr(value: &str, accept_specs: bool) -> Self {
        match value.trim() {
            "true" => LineNumbers::On,
            "1" if !accept_specs => LineNumbers::On,
            "false" | "0" | "" => LineNumbers::Off,
            other => LineNumbers::Spec(other.to_string()),
        }
    }
}

/// `true` for the two spellings of Pandoc's line-numbering class.
fn is_number_lines_class(class: &str) -> bool {
    class == "number-lines" || class == "numberLines"
}

/// Map the document-level `code-overflow` key to the class Q1 writes
/// for the per-block form. `None` when the key is unset or not one of
/// `wrap` / `scroll`.
pub fn resolve_default_overflow_class(
    meta: &quarto_pandoc_types::ConfigValue,
) -> Option<&'static str> {
    match meta.get("code-overflow")?.as_plain_text()?.trim() {
        "wrap" => Some("code-overflow-wrap"),
        "scroll" => Some("code-overflow-scroll"),
        _ => None,
    }
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

    fn phase(&self) -> TransformPhase {
        TransformPhase::Normalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        // Resolve document-level defaults once. Phase 2 only needs
        // `code-copy`; future phases will join more keys here (e.g.
        // `code-fold`, `code-line-numbers`).
        //
        // revealjs now honors `code-copy:` exactly like HTML (bd-lg6t6qfy):
        // the `.code-copy-button` styling ships via the shared `copy-code.scss`
        // SCSS layer (`load_copy_code_layer`, bundled by `assemble_reveal_scss`)
        // and the clipboard JS ships as `js:revealjs:*` assets in native
        // render. This lifts the bd-fu1a5g6l suppression that forced
        // `CopyMode::Off` for revealjs while that support was missing.
        // (Preview / hub-client buttons are styled + hover-hidden but inert,
        // matching plain-HTML preview; an iframe-safe init is a follow-up.)
        let doc_default_copy = resolve_default_copy_mode(&ast.meta);
        // Phase 3: fold defaults. Q1 folds only in HTML-based formats
        // (`_quarto.format.isHtmlOutput()`), so other formats keep the
        // attributes on the block untouched.
        // Document-level `code-overflow: wrap | scroll`. Q1 feeds this to
        // a Sass variable (`$code-white-space`); q2 stamps the per-block
        // class instead, which the ported `.code-overflow-*` rules style.
        let doc_overflow_class = resolve_default_overflow_class(&ast.meta);
        let fold_enabled = ctx.format.identifier.is_html_based();
        // Phase 6: code annotations. Runs first so an annotated block has
        // its `annotated-cell-N` id before line numbering looks for one.
        // Not in the q2-preview React renderer; see `code_annotations`.
        if fold_enabled && ctx.pipeline_profile != crate::format::PipelineProfile::HtmlPreview {
            let mode = super::code_annotations::AnnotationMode::from_meta(&ast.meta);
            super::code_annotations::apply_code_annotations(&mut ast.blocks, mode);
        }

        // Phase 4: line numbers. HTML-based formats only; elsewhere the
        // attribute stays on the block for Pandoc's own filters.
        let is_revealjs = ctx.format.identifier == crate::format::FormatIdentifier::Revealjs;
        let doc_default_line_numbers =
            ast.meta
                .get("code-line-numbers")
                .map_or(LineNumbers::Off, |v| match v.as_bool() {
                    Some(b) => LineNumbers::from_bool(b),
                    None => v.as_plain_text().map_or(LineNumbers::Off, |s| {
                        LineNumbers::from_attr(&s, is_revealjs)
                    }),
                });
        // Every source block counts, numbered or not, so a block's id
        // does not change when its neighbours gain line numbers.
        let mut source_block_counter = 0usize;

        let doc_default_fold = resolve_default_fold_mode(&ast.meta);
        let doc_default_summary = resolve_default_code_summary(&ast.meta);

        walk_blocks_mut(&mut ast.blocks, &mut |block| {
            let Block::CodeBlock(cb) = block else {
                return;
            };
            // Build a decoration from the per-block attributes and the
            // resolved document-level defaults. Phase 1 reads only
            // `filename` per-block; Phase 2 takes copy from the
            // document default (Q1 supports no per-block override).
            // Future phases will extend this match.
            let filename = cb.attr.2.get("filename").cloned();

            // Phase 3: only an executed cell's source block folds
            // (`.cell-code`); a plain fenced block never does, even
            // with a `code-fold` attribute. The attributes are consumed
            // here so they do not leak into the page as `data-code-fold`.
            let (fold, summary, fold_hidden) =
                if fold_enabled && cb.attr.1.iter().any(|c| c == "cell-code") {
                    let fold_attr = cb.attr.2.remove("code-fold");
                    let summary_attr = cb.attr.2.remove("code-summary");
                    let fold = match fold_attr.as_deref() {
                        Some(v) => FoldMode::from_attr(v),
                        None => doc_default_fold,
                    };
                    if fold.is_on() {
                        let summary = summary_attr
                            .filter(|s| !s.trim().is_empty())
                            .unwrap_or_else(|| doc_default_summary.clone());
                        let hidden = cb.attr.1.iter().any(|c| c == "hidden");
                        (fold, Some(summary), hidden)
                    } else {
                        (FoldMode::None, None, false)
                    }
                } else {
                    (FoldMode::None, None, false)
                };

            // Q1 adds the copy button to `pre.sourceCode` only, that is,
            // to blocks with a language. A block with no class at all
            // (a cell's printed output, a bare fence) gets none.
            // (bd-xaaucvfz)
            let copy = if cb.attr.1.is_empty() {
                CopyMode::Off
            } else {
                doc_default_copy
            };

            // Phase 4: `code-line-numbers` (block attribute, then the
            // document default) resolves to Pandoc's `.number-lines`
            // class, which the HTML writer turns into per-line markup.
            // Q1: `quarto-pre/line-numbers.lua`, source blocks only.
            if fold_enabled && !cb.attr.1.is_empty() {
                source_block_counter += 1;
                let line_numbers = match cb.attr.2.remove("code-line-numbers") {
                    Some(v) => LineNumbers::from_attr(&v, is_revealjs),
                    None => doc_default_line_numbers.clone(),
                };
                if line_numbers != LineNumbers::Off
                    && !cb.attr.1.iter().any(|c| is_number_lines_class(c))
                {
                    cb.attr.1.push("number-lines".to_string());
                }
                // Reveal keeps a line spec ("1,3", "|1|2") for its
                // highlight step; other formats only number.
                if is_revealjs && let LineNumbers::Spec(spec) = &line_numbers {
                    cb.attr
                        .2
                        .insert("code-line-numbers".to_string(), spec.clone());
                }
                // Numbered lines need a block id for their anchors
                // (`cb3-10`). Pandoc names every code block this way.
                if cb.attr.0.is_empty() && cb.attr.1.iter().any(|c| is_number_lines_class(c)) {
                    cb.attr.0 = format!("cb{source_block_counter}");
                }
            }

            // A block's own `.code-overflow-*` class (or the cell option
            // the engine turned into one) wins over the document default.
            if let Some(class) = doc_overflow_class
                && !cb.attr.1.is_empty()
                && !cb.attr.1.iter().any(|c| c.starts_with("code-overflow-"))
            {
                cb.attr.1.push(class.to_string());
            }

            let decoration = CodeBlockDecoration {
                filename,
                copy,
                fold,
                summary,
                fold_hidden,
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

        // `code-tools`: the title-block button. It offers Show/Hide All
        // Code only when some cell folds, which is known here.
        let has_folded_cell = ctx.code_block_decorations.values().any(|d| d.fold.is_on());
        super::code_tools::apply_code_tools(ast, ctx, has_folded_cell);
        Ok(())
    }
}

/// Returns `true` when at least one decoration-triggering field is
/// populated. Render uses this both to short-circuit the AST walk
/// when nothing is decorated and to skip individual blocks that
/// landed in the sideband but turned out to have no actionable
/// content (defensive — Generate already filters with the same
/// predicate).
pub(crate) fn decoration_has_any_field(d: &CodeBlockDecoration) -> bool {
    d.filename.is_some() || d.copy.is_on() || d.fold.is_on()
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
        walk_block_mut(block, f);
    }
}

/// Single-block counterpart of [`walk_blocks_mut`].
fn walk_block_mut(block: &mut Block, f: &mut impl FnMut(&mut Block)) {
    match block {
        Block::CodeBlock(_) => f(block),
        Block::BlockQuote(bq) => walk_blocks_mut(&mut bq.content, f),
        Block::Div(div) => {
            hoist_div_filename(div);
            walk_blocks_mut(&mut div.content, f)
        }
        // Custom-node block slots: since #1050 a callout is still a
        // `Callout` custom node when this transform runs (bd-akkproqy).
        Block::Custom(node) => {
            for (_name, slot) in node.slots.iter_mut() {
                match slot {
                    Slot::Blocks(blocks) => walk_blocks_mut(blocks, f),
                    Slot::Block(b) => walk_block_mut(b, f),
                    Slot::Inline(_) | Slot::Inlines(_) => {}
                }
            }
        }
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
        // variants / CaptionBlock are all leaf-ish from the perspective
        // of block-level code-block discovery.
        _ => {}
    }
}

/// Move a `filename` attribute from a `Div` onto the `CodeBlock` that
/// is its first child, so the per-block filename path decorates it.
///
/// This is how a `filename` cell option arrives: the engine writes it
/// on the `div.cell`, not on the source fence. Q1 has the same rule in
/// `quarto-pre/code-filename.lua` ("Div with `filename` whose first
/// block is a CodeBlock"). A filename already on the code block wins.
fn hoist_div_filename(div: &mut quarto_pandoc_types::block::Div) {
    if !div.attr.2.contains_key("filename") {
        return;
    }
    let Some(Block::CodeBlock(cb)) = div.content.first_mut() else {
        return;
    };
    if let Some(filename) = div.attr.2.remove("filename")
        && !cb.attr.2.contains_key("filename")
    {
        cb.attr.2.insert("filename".to_string(), filename);
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
        Block::CodeBlock(Box::new(CodeBlock {
            attr: (
                String::new(),
                classes.into_iter().map(String::from).collect(),
                kv_map,
            )
                .into(),
            text: text.to_string(),
            source_info: source_info_at(0, 0, text.len()),
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
        let deco = &ctx.code_block_decorations[&key];
        assert_eq!(deco.filename.as_deref(), Some("hello.py"));
    }

    /// Builds a `meta` map carrying a single `code-copy` entry. Helper
    /// for the Phase 2 doc-default resolution tests below.
    fn meta_with_code_copy(value: quarto_pandoc_types::ConfigValueKind) -> ConfigValue {
        use quarto_pandoc_types::{ConfigMapEntry, ConfigValue, ConfigValueKind, MergeOp};
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

    fn yaml_bool(b: bool) -> quarto_pandoc_types::ConfigValueKind {
        quarto_pandoc_types::ConfigValueKind::scalar(yaml_rust2::Yaml::Boolean(b))
    }

    fn yaml_str(s: &str) -> quarto_pandoc_types::ConfigValueKind {
        quarto_pandoc_types::ConfigValueKind::scalar(yaml_rust2::Yaml::String(s.to_string()))
    }

    /// A `code-copy` value as it is actually stored after a bare front-matter
    /// string is parsed as markdown: `PandocInlines`, not `Scalar(String)`.
    fn yaml_inlines(s: &str) -> quarto_pandoc_types::ConfigValueKind {
        use quarto_pandoc_types::inline::{Inline, Str};
        quarto_pandoc_types::ConfigValueKind::PandocInlines(vec![Inline::Str(Str {
            text: s.to_string(),
            source_info: quarto_source_map::SourceInfo::generated(quarto_source_map::By::revealjs()),
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
        let deco = &ctx.code_block_decorations[&key];
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
        let deco = &ctx.code_block_decorations[&key];
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
        let deco = &ctx.code_block_decorations[&key];
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
        let deco = &ctx.code_block_decorations[&key];
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
        let deco = &ctx.code_block_decorations[&key];
        assert_eq!(deco.filename.as_deref(), Some("hello.py"));
        assert_eq!(deco.copy, CopyMode::Hover);
    }

    // ── Phase 3: fold ─────────────────────────────────────────────────

    /// `meta` map with arbitrary entries, for the fold tests.
    fn meta_with(entries: Vec<(&str, quarto_pandoc_types::ConfigValueKind)>) -> ConfigValue {
        use quarto_pandoc_types::{ConfigMapEntry, ConfigValueKind, MergeOp};
        ConfigValue {
            value: ConfigValueKind::Map(
                entries
                    .into_iter()
                    .map(|(key, value)| ConfigMapEntry {
                        key: key.to_string(),
                        key_source: SourceInfo::for_test(),
                        value: ConfigValue {
                            value,
                            source_info: SourceInfo::for_test(),
                            merge_op: MergeOp::Concat,
                        },
                    })
                    .collect(),
            ),
            source_info: SourceInfo::for_test(),
            merge_op: MergeOp::Concat,
        }
    }

    /// Run Generate over one block and return its decoration (if any)
    /// plus the block as Generate left it.
    async fn generate_one(
        meta: ConfigValue,
        block: Block,
        format: Format,
    ) -> (Option<CodeBlockDecoration>, Block) {
        let mut ast = Pandoc {
            meta,
            blocks: vec![block],
        };
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        CodeBlockGenerateTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        let key = CodeBlockDecorationKey::from_source_info(ast.blocks[0].source_info()).unwrap();
        let deco = ctx.code_block_decorations.get(&key).cloned();
        (deco, ast.blocks.remove(0))
    }

    fn kv<'a>(block: &'a Block, key: &str) -> Option<&'a String> {
        let Block::CodeBlock(cb) = block else {
            panic!("expected CodeBlock");
        };
        cb.attr.2.get(key)
    }

    #[test]
    fn fold_mode_from_attr_follows_q1() {
        assert_eq!(FoldMode::from_attr("true"), FoldMode::Hide);
        assert_eq!(FoldMode::from_attr("1"), FoldMode::Hide);
        assert_eq!(FoldMode::from_attr("show"), FoldMode::Show);
        assert_eq!(FoldMode::from_attr("false"), FoldMode::None);
        assert_eq!(FoldMode::from_attr("0"), FoldMode::None);
        assert_eq!(FoldMode::from_attr("none"), FoldMode::None);
        // Q1 writes `open` only for "show"; any other string folds closed.
        assert_eq!(FoldMode::from_attr("hide"), FoldMode::Hide);
    }

    #[test]
    fn resolve_default_fold_mode_reads_bool_and_string() {
        assert_eq!(
            resolve_default_fold_mode(&ConfigValue::default()),
            FoldMode::None
        );
        assert_eq!(
            resolve_default_fold_mode(&meta_with(vec![("code-fold", yaml_bool(true))])),
            FoldMode::Hide
        );
        assert_eq!(
            resolve_default_fold_mode(&meta_with(vec![("code-fold", yaml_bool(false))])),
            FoldMode::None
        );
        assert_eq!(
            resolve_default_fold_mode(&meta_with(vec![("code-fold", yaml_str("show"))])),
            FoldMode::Show
        );
        // Bare front-matter strings arrive as PandocInlines.
        assert_eq!(
            resolve_default_fold_mode(&meta_with(vec![("code-fold", yaml_inlines("show"))])),
            FoldMode::Show
        );
    }

    #[test]
    fn resolve_default_code_summary_falls_back_to_code() {
        assert_eq!(
            resolve_default_code_summary(&ConfigValue::default()),
            "Code"
        );
        assert_eq!(
            resolve_default_code_summary(&meta_with(vec![(
                "code-summary",
                yaml_inlines("Reveal")
            )])),
            "Reveal"
        );
    }

    #[tokio::test]
    async fn generate_folds_cell_code_from_block_attribute() {
        let block = make_codeblock(
            "x",
            vec!["r", "cell-code"],
            vec![("code-fold", "true"), ("code-summary", "Show the sum")],
        );
        let (deco, block) = generate_one(ConfigValue::default(), block, Format::html()).await;
        let deco = deco.expect("decoration");
        assert_eq!(deco.fold, FoldMode::Hide);
        assert_eq!(deco.summary.as_deref(), Some("Show the sum"));
        assert!(!deco.fold_hidden);
        // Consumed, so the writer does not emit `data-code-fold`.
        assert!(kv(&block, "code-fold").is_none());
        assert!(kv(&block, "code-summary").is_none());
    }

    #[tokio::test]
    async fn generate_folds_cell_code_from_document_default() {
        let meta = meta_with(vec![
            ("code-fold", yaml_inlines("show")),
            ("code-summary", yaml_inlines("Reveal code")),
        ]);
        let block = make_codeblock("x", vec!["r", "cell-code"], vec![]);
        let (deco, _) = generate_one(meta, block, Format::html()).await;
        let deco = deco.expect("decoration");
        assert_eq!(deco.fold, FoldMode::Show);
        assert_eq!(deco.summary.as_deref(), Some("Reveal code"));
    }

    #[tokio::test]
    async fn generate_block_fold_false_overrides_document_default() {
        let meta = meta_with(vec![("code-fold", yaml_bool(true))]);
        let block = make_codeblock("x", vec!["r", "cell-code"], vec![("code-fold", "false")]);
        let (deco, block) = generate_one(meta, block, Format::html()).await;
        let deco = deco.expect("copy decoration still present");
        assert_eq!(deco.fold, FoldMode::None);
        assert!(deco.summary.is_none());
        assert!(kv(&block, "code-fold").is_none());
    }

    #[tokio::test]
    async fn generate_summary_defaults_to_code() {
        let block = make_codeblock("x", vec!["r", "cell-code"], vec![("code-fold", "true")]);
        let (deco, _) = generate_one(ConfigValue::default(), block, Format::html()).await;
        assert_eq!(deco.unwrap().summary.as_deref(), Some("Code"));
    }

    #[tokio::test]
    async fn generate_never_folds_a_plain_fenced_block() {
        // Q1 folds only `.cell-code`. A plain block keeps its attribute,
        // also as in Q1, even under a document default.
        let meta = meta_with(vec![("code-fold", yaml_bool(true))]);
        let block = make_codeblock("x", vec!["python"], vec![("code-fold", "true")]);
        let (deco, block) = generate_one(meta, block, Format::html()).await;
        assert_eq!(deco.unwrap().fold, FoldMode::None);
        assert_eq!(kv(&block, "code-fold").map(String::as_str), Some("true"));
    }

    #[tokio::test]
    async fn generate_carries_hidden_class_to_fold() {
        let block = make_codeblock(
            "x",
            vec!["r", "cell-code", "hidden"],
            vec![("code-fold", "true")],
        );
        let (deco, _) = generate_one(ConfigValue::default(), block, Format::html()).await;
        assert!(deco.unwrap().fold_hidden);
    }

    #[tokio::test]
    async fn generate_folds_in_revealjs_and_dashboard() {
        for name in ["revealjs", "dashboard"] {
            let format = Format::from_format_string(name).unwrap();
            let block = make_codeblock("x", vec!["r", "cell-code"], vec![("code-fold", "true")]);
            let (deco, _) = generate_one(ConfigValue::default(), block, format).await;
            assert_eq!(deco.unwrap().fold, FoldMode::Hide);
        }
    }

    #[tokio::test]
    async fn generate_does_not_fold_outside_html_formats() {
        let meta = meta_with(vec![("code-copy", yaml_bool(false))]);
        let block = make_codeblock("x", vec!["r", "cell-code"], vec![("code-fold", "true")]);
        let (deco, block) = generate_one(meta, block, Format::docx()).await;
        assert!(deco.is_none());
        assert_eq!(kv(&block, "code-fold").map(String::as_str), Some("true"));
    }

    #[tokio::test]
    async fn generate_hoists_cell_filename_onto_its_code_block() {
        // The engine writes a `filename` cell option on `div.cell`.
        let mut kvs = hashlink::LinkedHashMap::new();
        kvs.insert("filename".to_string(), "calc.R".to_string());
        let cell = Block::Div(Box::new(quarto_pandoc_types::block::Div {
            attr: (String::new(), vec!["cell".to_string()], kvs).into(),
            content: vec![make_codeblock("f <- 6", vec!["r", "cell-code"], vec![])],
            source_info: SourceInfo::for_test(),
            attr_source: AttrSourceInfo::empty(),
        }));
        let mut ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![cell],
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

        let Block::Div(cell) = &ast.blocks[0] else {
            panic!("expected cell Div");
        };
        assert!(
            cell.attr.2.get("filename").is_none(),
            "the Div must not keep the attribute (it would render as data-filename)"
        );
        let key = CodeBlockDecorationKey::from_source_info(cell.content[0].source_info()).unwrap();
        assert_eq!(
            ctx.code_block_decorations[&key].filename.as_deref(),
            Some("calc.R")
        );
    }

    #[tokio::test]
    async fn generate_leaves_div_filename_when_first_child_is_not_code() {
        let mut kvs = hashlink::LinkedHashMap::new();
        kvs.insert("filename".to_string(), "calc.R".to_string());
        let div = Block::Div(Box::new(quarto_pandoc_types::block::Div {
            attr: (String::new(), vec![], kvs).into(),
            content: vec![],
            source_info: SourceInfo::for_test(),
            attr_source: AttrSourceInfo::empty(),
        }));
        let mut ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![div],
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
        let Block::Div(div) = &ast.blocks[0] else {
            panic!("expected Div");
        };
        assert_eq!(
            div.attr.2.get("filename").map(String::as_str),
            Some("calc.R")
        );
    }

    #[tokio::test]
    async fn generate_gives_no_copy_button_to_a_block_without_a_language() {
        // bd-xaaucvfz: a cell's printed output is a class-less CodeBlock.
        let block = make_codeblock("[1] 0.6", vec![], vec![]);
        let (deco, block) = generate_one(ConfigValue::default(), block, Format::html()).await;
        assert!(deco.is_none(), "got {:?}", deco);
        let Block::CodeBlock(cb) = &block else {
            panic!("expected CodeBlock");
        };
        assert!(cb.attr.1.is_empty(), "no code-with-copy class expected");
    }

    fn classes(block: &Block) -> &Vec<String> {
        let Block::CodeBlock(cb) = block else {
            panic!("expected CodeBlock");
        };
        &cb.attr.1
    }

    #[tokio::test]
    async fn generate_stamps_document_code_overflow_on_source_blocks() {
        let meta = meta_with(vec![("code-overflow", yaml_inlines("wrap"))]);
        let block = make_codeblock("x", vec!["python"], vec![]);
        let (_, block) = generate_one(meta, block, Format::html()).await;
        assert!(classes(&block).contains(&"code-overflow-wrap".to_string()));
    }

    #[tokio::test]
    async fn generate_keeps_a_blocks_own_code_overflow_class() {
        let meta = meta_with(vec![("code-overflow", yaml_inlines("wrap"))]);
        let block = make_codeblock("x", vec!["python", "code-overflow-scroll"], vec![]);
        let (_, block) = generate_one(meta, block, Format::html()).await;
        assert!(!classes(&block).contains(&"code-overflow-wrap".to_string()));
    }

    #[tokio::test]
    async fn generate_does_not_stamp_code_overflow_on_output_blocks() {
        let meta = meta_with(vec![("code-overflow", yaml_inlines("wrap"))]);
        let block = make_codeblock("[1] 1", vec![], vec![]);
        let (_, block) = generate_one(meta, block, Format::html()).await;
        assert!(classes(&block).is_empty());
    }

    // ── Phase 4: line numbers ─────────────────────────────────────────

    fn block_id(block: &Block) -> &str {
        let Block::CodeBlock(cb) = block else {
            panic!("expected CodeBlock");
        };
        &cb.attr.0
    }

    #[test]
    fn line_numbers_from_attr_follows_q1() {
        assert_eq!(LineNumbers::from_attr("true", false), LineNumbers::On);
        assert_eq!(LineNumbers::from_attr("1", false), LineNumbers::On);
        assert_eq!(LineNumbers::from_attr("false", false), LineNumbers::Off);
        assert_eq!(LineNumbers::from_attr("0", true), LineNumbers::Off);
        assert_eq!(
            LineNumbers::from_attr("2,4", false),
            LineNumbers::Spec("2,4".to_string())
        );
        // Reveal reads "1" as line 1.
        assert_eq!(
            LineNumbers::from_attr("1", true),
            LineNumbers::Spec("1".to_string())
        );
    }

    #[tokio::test]
    async fn generate_turns_code_line_numbers_attr_into_number_lines_class() {
        let block = make_codeblock("x", vec!["python"], vec![("code-line-numbers", "true")]);
        let (_, block) = generate_one(ConfigValue::default(), block, Format::html()).await;
        assert!(classes(&block).contains(&"number-lines".to_string()));
        assert!(kv(&block, "code-line-numbers").is_none());
        assert_eq!(block_id(&block), "cb1");
    }

    #[tokio::test]
    async fn generate_applies_document_code_line_numbers_and_block_false_wins() {
        let meta = meta_with(vec![("code-line-numbers", yaml_bool(true))]);
        let block = make_codeblock("x", vec!["python"], vec![]);
        let (_, block) = generate_one(meta.clone(), block, Format::html()).await;
        assert!(classes(&block).contains(&"number-lines".to_string()));

        let block = make_codeblock("x", vec!["python"], vec![("code-line-numbers", "false")]);
        let (_, block) = generate_one(meta.clone(), block, Format::html()).await;
        assert!(!classes(&block).contains(&"number-lines".to_string()));
        assert!(block_id(&block).is_empty());

        // An output block (no class) is never numbered.
        let block = make_codeblock("[1] 1", vec![], vec![]);
        let (_, block) = generate_one(meta, block, Format::html()).await;
        assert!(classes(&block).is_empty());
    }

    #[tokio::test]
    async fn generate_line_spec_numbers_in_html_and_is_kept_for_revealjs() {
        let block = make_codeblock("x", vec!["python"], vec![("code-line-numbers", "2,4")]);
        let (_, block) = generate_one(ConfigValue::default(), block, Format::html()).await;
        assert!(classes(&block).contains(&"number-lines".to_string()));
        assert!(kv(&block, "code-line-numbers").is_none());

        let block = make_codeblock("x", vec!["python"], vec![("code-line-numbers", "2,4")]);
        let format = Format::from_format_string("revealjs").unwrap();
        let (_, block) = generate_one(ConfigValue::default(), block, format).await;
        assert!(classes(&block).contains(&"number-lines".to_string()));
        assert_eq!(
            kv(&block, "code-line-numbers").map(String::as_str),
            Some("2,4")
        );
    }

    #[tokio::test]
    async fn generate_gives_an_id_to_a_number_lines_block_and_keeps_an_existing_one() {
        let block = make_codeblock("x", vec!["python", "numberLines"], vec![]);
        let (_, block) = generate_one(ConfigValue::default(), block, Format::html()).await;
        assert_eq!(block_id(&block), "cb1");

        let mut block = make_codeblock("x", vec!["python", "number-lines"], vec![]);
        if let Block::CodeBlock(cb) = &mut block {
            cb.attr.0 = "mine".to_string();
        }
        let (_, block) = generate_one(ConfigValue::default(), block, Format::html()).await;
        assert_eq!(block_id(&block), "mine");
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
    async fn generate_emits_code_with_copy_class_for_revealjs() {
        // bd-lg6t6qfy: revealjs now ships the copy-button support — the
        // `.code-copy-button` CSS (shared `copy-code.scss` layer) and the
        // clipboard JS (`js:revealjs:*` assets). So revealjs honors `code-copy:`
        // exactly like HTML, lifting the bd-fu1a5g6l suppression: the copy
        // scaffold IS emitted under the default (Hover) and explicit
        // `code-copy: true`, and is suppressed only by explicit `code-copy: false`.
        for meta in [ConfigValue::default(), meta_with_code_copy(yaml_bool(true))] {
            let mut ast = Pandoc {
                meta,
                blocks: vec![make_codeblock("print('hi')", vec!["python"], vec![])],
            };

            let project = make_test_project();
            let doc = DocumentInfo::from_path("/project/doc.qmd");
            let format = Format::from_format_string("revealjs").unwrap();
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
                "revealjs must get code-with-copy when copy is on; got {:?}",
                cb.attr.1,
            );
        }
    }

    #[tokio::test]
    async fn generate_honors_code_copy_false_for_revealjs() {
        // The flip side of the lift: an explicit `code-copy: false` still
        // suppresses the scaffold for revealjs, matching HTML.
        let mut ast = Pandoc {
            meta: meta_with_code_copy(yaml_bool(false)),
            blocks: vec![make_codeblock("print('hi')", vec!["python"], vec![])],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::from_format_string("revealjs").unwrap();
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
            "revealjs must honor code-copy: false; got {:?}",
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
                    Block::Div(Box::new(Div {
                        attr: (String::new(), vec![], hashlink::LinkedHashMap::new()).into(),
                        content: vec![cb_inside_div],
                        source_info: source_info_at(0, 100, 200),
                        attr_source: AttrSourceInfo::empty(),
                    })),
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

        let filter = SourceInfo::generated(quarto_source_map::By::filter("fixture.lua", 1));
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

    /// Code blocks inside a custom node's block slots are decorated too
    /// (bd-akkproqy): since #1050 a callout is still a `Callout` custom
    /// node when this transform runs.
    #[tokio::test]
    async fn generate_visits_codeblocks_in_custom_node_slots() {
        use quarto_pandoc_types::custom::{CustomNode, Slot};
        let mut callout = CustomNode::new(
            "Callout",
            (String::new(), vec![], hashlink::LinkedHashMap::new()).into(),
            source_info_at(0, 0, 100),
        );
        callout.set_slot(
            "content",
            Slot::Blocks(vec![make_codeblock(
                "x = 1",
                vec!["python"],
                vec![("filename", "in-callout.py")],
            )]),
        );
        let mut ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![Block::Custom(Box::new(callout))],
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

        let filenames: Vec<_> = ctx
            .code_block_decorations
            .values()
            .filter_map(|d| d.filename.clone())
            .collect();
        assert_eq!(filenames, vec!["in-callout.py".to_string()]);
    }
}
