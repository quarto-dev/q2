/*
 * stage/stages/include_expansion.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Pipeline stage that expands `{{< include file.qmd >}}` shortcodes
 * in the AST before engine execution.
 */

//! Include shortcode expansion stage.
//!
//! Resolves block-level `{{< include file.qmd >}}` shortcodes by parsing
//! the included file and splicing its AST blocks into the main document.
//! Expansion applies at **every block-list position** — top level and
//! nested inside divs, blockquotes, list items, tables, figures, and
//! footnote definitions (bd-1fz3vh99); the included file is parsed
//! standalone and its blocks become children of the containing
//! construct. Runs before engine execution so that included code cells
//! are visible to the engine.
//!
//! Path resolution: relative paths anchor at the including file's
//! directory (at every nesting level); a leading `/` is
//! **project-root-relative** per the Quarto path convention
//! (bd-w9koo1i2, see [`resolve_include_target`]).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use async_trait::async_trait;

use quarto_pandoc_types::block::Blocks;
use quarto_pandoc_types::shortcode::ShortcodeArg;
use quarto_pandoc_types::{Block, Inline};
use quarto_source_map::SourceInfo;

use crate::document_profile::IncludeEntry;
use crate::stage::data::DocumentAst;
use crate::stage::{PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext};

pub struct IncludeExpansionStage;

impl IncludeExpansionStage {
    pub fn new() -> Self {
        Self
    }
}

impl Default for IncludeExpansionStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl PipelineStage for IncludeExpansionStage {
    fn name(&self) -> &str {
        "include-expansion"
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

        let mut include_stack = HashSet::new();
        let doc_path = doc.path.clone();
        include_stack.insert(doc_path.clone());

        expand_includes_in_blocks(&mut doc, ctx, &doc_path, &mut include_stack)?;

        Ok(PipelineData::DocumentAst(doc))
    }
}

/// Document-level entry point: expand include shortcodes at every
/// block-list position in the AST (bd-1fz3vh99), recursively.
fn expand_includes_in_blocks(
    doc: &mut DocumentAst,
    ctx: &mut StageContext,
    current_file: &Path,
    include_stack: &mut HashSet<PathBuf>,
) -> Result<(), PipelineError> {
    // Destructure so the expander can hold the document-level state
    // alongside a mutable borrow of the block tree.
    let DocumentAst {
        ast,
        ast_context,
        source_context,
        recorded_includes,
        ..
    } = doc;
    let mut expander = IncludeExpander {
        ctx,
        ast_context,
        source_context,
        recorded_includes,
        include_stack,
    };
    expander.expand_blocks(&mut ast.blocks, current_file)
}

/// Walks the block tree expanding include shortcodes, carrying the
/// document-level bookkeeping every splice needs: the two source
/// contexts (which must grow in lockstep — each included file is
/// registered in both under the same `FileId`), the recorded-includes
/// side-channel for cache invalidation, and the cycle-detection stack.
struct IncludeExpander<'a> {
    ctx: &'a mut StageContext,
    ast_context: &'a mut pampa::pandoc::ASTContext,
    source_context: &'a mut quarto_source_map::SourceContext,
    recorded_includes: &'a mut Vec<IncludeEntry>,
    include_stack: &'a mut HashSet<PathBuf>,
}

impl IncludeExpander<'_> {
    /// Expand includes in one block list. `current_file` is the file
    /// whose source produced these blocks — relative include paths
    /// resolve against its directory; leading-`/` paths resolve
    /// against the project root (see [`resolve_include_target`]).
    ///
    /// An included file's blocks are recursively expanded *before*
    /// being spliced into `blocks`, so every nesting level sees the
    /// same shape: resolve → parse → register → remap → recurse →
    /// splice. Blocks that are not includes have their child block
    /// lists (div/list/table/… — see [`child_block_lists_mut`])
    /// walked in place.
    fn expand_blocks(
        &mut self,
        blocks: &mut Vec<Block>,
        current_file: &Path,
    ) -> Result<(), PipelineError> {
        let mut i = 0;
        while i < blocks.len() {
            let Some(include_path) = extract_include_path(&blocks[i]) else {
                // A code fence is a leaf with no child block list, but
                // its *text* can carry includes (the Q1 listing idiom
                // — bd-include-in-code-block-f8mvtczn). Those splice
                // textually and in place; the block itself stays.
                if let Block::CodeBlock(code_block) = &mut blocks[i] {
                    self.expand_code_fence(code_block, current_file);
                }
                for list in child_block_lists_mut(&mut blocks[i]) {
                    self.expand_blocks(list, current_file)?;
                }
                i += 1;
                continue;
            };

            let base_dir = current_file.parent().unwrap_or(Path::new("."));
            let resolved = resolve_include_target(base_dir, &self.ctx.project.dir, &include_path);

            // Canonicalize for cycle detection
            let canonical = resolved.canonicalize().unwrap_or_else(|_| resolved.clone());

            // Check for circular includes
            if self.include_stack.contains(&canonical) {
                self.ctx.diagnostics.push(
                    quarto_error_reporting::DiagnosticMessageBuilder::warning("Circular include")
                        .with_code("Q-17-1")
                        .with_location(blocks[i].source_info().clone())
                        .problem(format!(
                            "Circular include detected: '{}' is already being included",
                            resolved.display()
                        ))
                        .add_hint("Check for files that include each other, directly or indirectly")
                        .build(),
                );
                // Remove the failed include's paragraph: the failure is
                // reported; leaving the shortcode in the AST would make
                // the shortcode-resolve transform misreport it as an
                // unknown shortcode (bd-qpvoamvu).
                blocks.remove(i);
                continue;
            }

            // Read the included file
            let content = match self.ctx.runtime.file_read(&resolved) {
                Ok(bytes) => bytes,
                Err(e) => {
                    self.ctx.diagnostics.push(
                        quarto_error_reporting::DiagnosticMessageBuilder::warning(
                            "Include file not found",
                        )
                        .with_code("Q-17-2")
                        .with_location(blocks[i].source_info().clone())
                        .problem(format!(
                            "Could not read included file '{}': {}",
                            resolved.display(),
                            e
                        ))
                        .build(),
                    );
                    // See the circular-include arm for why the block is
                    // removed rather than skipped.
                    blocks.remove(i);
                    continue;
                }
            };

            // Parse the included file
            let mut stderr_buf = Vec::new();
            let filename = resolved.to_string_lossy().to_string();
            let parse_result =
                pampa::readers::qmd::read(&content, false, &filename, &mut stderr_buf, true, None);

            let (included_pandoc, included_ast_context, included_warnings) = match parse_result {
                Ok(result) => result,
                Err(inner_diagnostics) => {
                    // Register the included file's content in both
                    // SourceContexts so the inner diagnostics can render
                    // ariadne snippets from it. Registering in *both* —
                    // even though only `self.source_context` is read here —
                    // keeps the two contexts growing in lockstep, which
                    // the success path's debug_assert relies on for any
                    // later include in the same document.
                    let content_str = String::from_utf8_lossy(&content).into_owned();
                    let new_file_id = self
                        .ast_context
                        .source_context
                        .add_file(filename.clone(), Some(content_str.clone()));
                    let snippet_file_id = self
                        .source_context
                        .add_file(filename.clone(), Some(content_str));
                    debug_assert_eq!(
                        new_file_id, snippet_file_id,
                        "FileId mismatch between ast_context.source_context and source_context"
                    );

                    self.ctx.diagnostics.push(
                        quarto_error_reporting::DiagnosticMessageBuilder::error(
                            "Include file parse error",
                        )
                        .with_code("Q-17-3")
                        .with_location(blocks[i].source_info().clone())
                        .problem(format!(
                            "Included file '{}' has {} parse error(s), reported below",
                            resolved.display(),
                            inner_diagnostics.len()
                        ))
                        .build(),
                    );

                    // Surface the included file's own diagnostics,
                    // remapped from the child parse's private FileId(0)
                    // into the parent document's SourceContext.
                    let remap = |id: quarto_source_map::FileId| {
                        if id == quarto_source_map::FileId(0) {
                            new_file_id
                        } else {
                            id
                        }
                    };
                    for mut diag in inner_diagnostics {
                        remap_diagnostic_locations(&mut diag, &remap);
                        self.ctx.diagnostics.push(diag);
                    }

                    // The output still depends on the broken file's
                    // bytes: fixing it must invalidate any cached
                    // render, so record it like a successful include.
                    record_include(self.recorded_includes, &canonical, &content);

                    // See the circular-include arm for why the block is
                    // removed rather than skipped.
                    blocks.remove(i);
                    continue;
                }
            };

            // Register included file in BOTH SourceContexts with the same FileId
            let content_str = String::from_utf8_lossy(&content).into_owned();

            // Register in ast_context.source_context (for map_offset resolution)
            let new_file_id = if let Some(file_info) = included_ast_context
                .source_context
                .get_file(quarto_source_map::FileId(0))
                .and_then(|f| f.file_info.clone())
            {
                self.ast_context
                    .source_context
                    .add_file_with_info(filename.clone(), file_info)
            } else {
                self.ast_context
                    .source_context
                    .add_file(filename.clone(), Some(content_str.clone()))
            };

            // Register in top-level source_context (for ariadne error snippets)
            // Use add_file which returns a new FileId, but we need the same one.
            // Since both contexts grow sequentially, they should stay in sync if
            // we register in the same order. However, to be safe we verify.
            let snippet_file_id = self
                .source_context
                .add_file(filename.clone(), Some(content_str));
            debug_assert_eq!(
                new_file_id, snippet_file_id,
                "FileId mismatch between ast_context.source_context and source_context"
            );

            // Merge filenames
            for name in &included_ast_context.filenames {
                if !self.ast_context.filenames.contains(name) {
                    self.ast_context.filenames.push(name.clone());
                }
            }

            // Remap FileIds in the parsed AST: FileId(0) → new_file_id
            let remap = |id: quarto_source_map::FileId| {
                if id == quarto_source_map::FileId(0) {
                    new_file_id
                } else {
                    id
                }
            };
            let mut temp_pandoc = quarto_pandoc_types::pandoc::Pandoc {
                meta: quarto_pandoc_types::config_value::ConfigValue::default(),
                blocks: included_pandoc.blocks,
            };
            quarto_ast_reconcile::remap_file_ids(&mut temp_pandoc, &remap);
            let mut children = temp_pandoc.blocks;

            // Surface the included file's parse *warnings* the same way
            // the error path surfaces its errors (previously they were
            // silently dropped — bd-1fz3vh99's folded-in gap).
            for mut diag in included_warnings {
                remap_diagnostic_locations(&mut diag, &remap);
                self.ctx.diagnostics.push(diag);
            }

            // Record this child in the `recorded_includes` side-channel
            // for `bd-r82e` / Phase-8 cache invalidation. The
            // `canonical` path is what cycle detection keys on; when
            // canonicalize fails, it falls back to the resolved path
            // (still a stable identifier for hashing). Dedupe so a
            // child included twice in different positions appears once.
            record_include(self.recorded_includes, &canonical, &content);

            // Recursively expand the included blocks BEFORE splicing:
            // nested includes resolve against the included file's own
            // directory, and the spliced result needs no re-walking.
            self.include_stack.insert(canonical.clone());
            self.expand_blocks(&mut children, &resolved)?;
            self.include_stack.remove(&canonical);

            // Replace the include block with the fully-expanded children.
            let num_inserted = children.len();
            blocks.splice(i..i + 1, children);
            i += num_inserted;
        }
        Ok(())
    }

    /// Splice code-fence includes into `code_block.text`
    /// (bd-include-in-code-block-f8mvtczn).
    ///
    /// This is the one include position where Q1's *textual* model is
    /// the right model rather than a legacy quirk: the destination is
    /// raw text, not a block list, so the fence-corruption hazards that
    /// motivated the AST approach elsewhere do not apply. The target's
    /// bytes go in verbatim — no parsing, no re-indentation (Q1 does
    /// neither), one trailing newline trimmed (see
    /// [`trim_one_trailing_newline`]).
    ///
    /// **Recursive**, matching Q1: spliced text is itself re-scanned for
    /// include lines, so a `.qmd` embedded as a listing shows the same
    /// content it would show as a page. Q1's `standaloneInclude`
    /// (`src/core/handlers/include-standalone.ts`) does the same, with
    /// the same cycle guard. Recursion is also what keeps a nested
    /// include from surviving to `ShortcodeResolveTransform`, which
    /// renders any unhandled include as the `?include` token this whole
    /// change exists to remove.
    ///
    /// Nested paths anchor at the *including file's* directory, matching
    /// how block-position includes nest (bd-1fz3vh99). Q1 anchors nested
    /// fence includes at the document instead; ours is the more
    /// consistent rule and the one q2 already documents.
    ///
    /// Runs before the engine, so an authored executable cell has its
    /// include spliced into the cell source *before* execution — again
    /// matching Q1's text-level model. The `.cell-code` half of the
    /// opt-out cannot fire here (the engine writes that class later),
    /// but `shortcodes="false"` is authored and does.
    ///
    /// Errors are non-fatal: a diagnostic is pushed and the offending
    /// line is dropped. Dropping rather than leaving it matters for the
    /// same `?include` reason.
    fn expand_code_fence(
        &mut self,
        code_block: &mut quarto_pandoc_types::block::CodeBlock,
        current_file: &Path,
    ) {
        let includes = code_fence_includes(code_block);
        if includes.is_empty() {
            return;
        }
        code_block.text = self.splice_fence_text(
            &code_block.text,
            includes,
            current_file,
            &code_block.source_info,
        );
    }

    /// Replace each include line in `text` with its target's content,
    /// recursively. `location` anchors any diagnostic at the originating
    /// fence — nested text has no source span of its own.
    fn splice_fence_text(
        &mut self,
        text: &str,
        includes: Vec<(usize, String)>,
        current_file: &Path,
        location: &SourceInfo,
    ) -> String {
        let base_dir = current_file.parent().unwrap_or(Path::new("."));
        // `(line index, replacement)` in ascending line order, mirroring
        // `code_fence_includes`. `None` means the line is dropped.
        let mut replacements: Vec<(usize, Option<String>)> = Vec::with_capacity(includes.len());

        for (line_idx, raw_path) in includes {
            let resolved = resolve_include_target(base_dir, &self.ctx.project.dir, &raw_path);
            let canonical = resolved.canonicalize().unwrap_or_else(|_| resolved.clone());

            // A file that embeds itself as a listing would recurse
            // forever: the spliced copy carries the same include line.
            if self.include_stack.contains(&canonical) {
                self.ctx.diagnostics.push(
                    quarto_error_reporting::DiagnosticMessageBuilder::warning("Circular include")
                        .with_code("Q-17-1")
                        .with_location(location.clone())
                        .problem(format!(
                            "Circular include detected: '{}' is already being included",
                            resolved.display()
                        ))
                        .add_hint("Check for files that include each other, directly or indirectly")
                        .build(),
                );
                replacements.push((line_idx, None));
                continue;
            }

            match self.ctx.runtime.file_read(&resolved) {
                Ok(bytes) => {
                    // Record before trimming: the render depends on the
                    // file's real bytes, so the cache-invalidation hash
                    // must cover them all (bd-r82e).
                    record_include(self.recorded_includes, &canonical, &bytes);
                    let content = String::from_utf8_lossy(trim_one_trailing_newline(&bytes));

                    let nested = code_fence_include_lines(&content, location);
                    let spliced = if nested.is_empty() {
                        content.into_owned()
                    } else {
                        self.include_stack.insert(canonical.clone());
                        let out = self.splice_fence_text(&content, nested, &resolved, location);
                        self.include_stack.remove(&canonical);
                        out
                    };
                    replacements.push((line_idx, Some(spliced)));
                }
                Err(e) => {
                    self.ctx.diagnostics.push(
                        quarto_error_reporting::DiagnosticMessageBuilder::warning(
                            "Include file not found",
                        )
                        .with_code("Q-17-2")
                        .with_location(location.clone())
                        .problem(format!(
                            "Could not read included file '{}': {}",
                            resolved.display(),
                            e
                        ))
                        .build(),
                    );
                    replacements.push((line_idx, None));
                }
            }
        }

        // Sequential merge — both sequences run in ascending line
        // order, so one pass with a cursor suffices.
        let mut pending = replacements.into_iter().peekable();
        let rebuilt: Vec<String> = text
            .split('\n')
            .enumerate()
            .filter_map(|(idx, line)| match pending.peek() {
                // `Some(text)` splices the file in; `None` drops a line
                // whose include could not be read.
                Some((at, _)) if *at == idx => pending.next().and_then(|(_, text)| text),
                _ => Some(line.to_string()),
            })
            .collect();
        rebuilt.join("\n")
    }
}

/// Remap the `FileId`s in a diagnostic's primary location and every
/// detail location — used to carry an included file's parse
/// diagnostics (errors and warnings alike) from the child parse's
/// private `FileId(0)` into the parent document's `SourceContext`.
fn remap_diagnostic_locations(
    diag: &mut quarto_error_reporting::DiagnosticMessage,
    remap: &impl Fn(quarto_source_map::FileId) -> quarto_source_map::FileId,
) {
    if let Some(loc) = diag.location.as_mut() {
        loc.remap_file_ids(remap);
    }
    for detail in diag.details.iter_mut() {
        if let Some(loc) = detail.location.as_mut() {
            loc.remap_file_ids(remap);
        }
    }
}

/// The child block lists of a block — every `Blocks` position include
/// expansion descends into. This is the **single source of truth** for
/// "where can an include appear": both the expander
/// ([`IncludeExpander::expand_blocks`]) and the path collector
/// ([`collect_include_paths`], used by the preview dep-graph) walk
/// through this accessor, so the two can never drift.
fn child_block_lists_mut(block: &mut Block) -> Vec<&mut Blocks> {
    match block {
        Block::Div(div) => vec![&mut div.content],
        Block::BlockQuote(quote) => vec![&mut quote.content],
        Block::BulletList(list) => list.content.iter_mut().collect(),
        Block::OrderedList(list) => list.content.iter_mut().collect(),
        Block::DefinitionList(dl) => dl
            .content
            .iter_mut()
            .flat_map(|(_term, definitions)| definitions.iter_mut())
            .collect(),
        Block::Figure(figure) => figure
            .caption
            .long
            .iter_mut()
            .chain(std::iter::once(&mut figure.content))
            .collect(),
        Block::NoteDefinitionFencedBlock(note) => vec![&mut note.content],
        Block::Table(table) => {
            let mut lists: Vec<&mut Blocks> = Vec::new();
            lists.extend(table.caption.long.iter_mut());
            let rows = table
                .head
                .rows
                .iter_mut()
                .chain(
                    table
                        .bodies
                        .iter_mut()
                        .flat_map(|body| body.head.iter_mut().chain(body.body.iter_mut())),
                )
                .chain(table.foot.rows.iter_mut());
            for row in rows {
                lists.extend(row.cells.iter_mut().map(|cell| &mut cell.content));
            }
            lists
        }
        // Custom nodes are created by AstTransformsStage, which runs
        // after include expansion — none exist at this stage. Every
        // other variant is a leaf: no nested block lists.
        _ => Vec::new(),
    }
}

/// Collect every include path recognized at any block-list position,
/// in document order (the exact set [`IncludeExpander`] would expand).
///
/// Takes `&mut` — not because it mutates, but so it can share
/// [`child_block_lists_mut`] with the expander; a parallel immutable
/// accessor would be a second copy of the container-position list that
/// could silently drift. Callers (the preview dep-graph scanner) own
/// their freshly-parsed `Pandoc`, so the mutable borrow costs nothing.
pub fn collect_include_paths(blocks: &mut Blocks) -> Vec<String> {
    fn walk(blocks: &mut Blocks, out: &mut Vec<String>) {
        for block in blocks.iter_mut() {
            if let Some(path) = extract_include_path(block) {
                out.push(path);
                continue;
            }
            // Code-fence includes are dependencies too: without them,
            // editing the embedded source file never rebuilds the page
            // that shows it (bd-include-in-code-block-f8mvtczn).
            if let Block::CodeBlock(code_block) = block {
                out.extend(code_fence_includes(code_block).into_iter().map(|(_, p)| p));
            }
            for list in child_block_lists_mut(block) {
                walk(list, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(blocks, &mut out);
    out
}

/// Append an [`IncludeEntry`] for a freshly-spliced child file,
/// deduplicating by resolved path (a child included twice in
/// different positions of the parent appears once in the recorded
/// set). The hash captures the bytes that were spliced.
fn record_include(out: &mut Vec<IncludeEntry>, resolved: &Path, bytes: &[u8]) {
    if out.iter().any(|e| e.path == resolved) {
        return;
    }
    out.push(IncludeEntry::new(resolved.to_path_buf(), bytes));
}

/// Resolve a raw include-shortcode path against its anchor.
///
/// A path beginning with `/` (or `\` — Windows-authored) is
/// **project-root-relative**, per the Quarto path convention (glob
/// decision D2; Q1's `resolvePath` in `core/handlers/base.ts`):
/// `/a/b.qmd` means `<project>/a/b.qmd`, never the filesystem root.
/// For single-file renders `project_dir` is the input file's own
/// directory ([`crate::project::ProjectContext::discover`]), which
/// matches Q1's `rootDir = sourceDir` fallback without a branch here.
///
/// Everything else resolves against `base_dir`, the including file's
/// directory — so nested relative includes anchor at each included
/// file (bd-1fz3vh99) while the leading-`/` anchor stays fixed at the
/// project root at every nesting level.
///
/// Windows drive-absolute paths (`C:\…`) are not part of the
/// convention (Q1 defines no semantics for them either); they fall
/// through the relative arm, where `Path::join` keeps them absolute.
fn resolve_include_target(base_dir: &Path, project_dir: &Path, raw: &str) -> PathBuf {
    if raw.starts_with('/') || raw.starts_with('\\') {
        // Normalize separators before anchoring so a Windows-authored
        // `\a\b.qmd` behaves identically everywhere.
        let normalized = raw.replace('\\', "/");
        project_dir.join(normalized.trim_start_matches('/'))
    } else {
        base_dir.join(raw)
    }
}

/// Recognize a `{{< include … >}}` occupying a whole line of code-fence
/// text, returning its raw path argument.
///
/// **Line-strict** (bd-include-in-code-block-f8mvtczn): the shortcode
/// must be the sole content of the line, modulo surrounding whitespace
/// — which is exactly Q1's rule (`isBlockShortcode` anchors
/// `/^\s*{{< … >}}\s*$/` in `src/core/lib/parse-shortcode.ts`). A
/// shortcode sharing its line with code is left alone, so the rule can
/// be widened later without invalidating documents; it could not be
/// narrowed again.
///
/// Parsing goes through [`parse_text_shortcodes`], the same text-level
/// parser `ShortcodeResolveTransform` uses, rather than a second
/// hand-rolled matcher. That is what makes the escaped form
/// (`{{{< include … >}}}`, which arrives as a literal segment) fall out
/// correctly instead of needing its own special case.
///
/// [`parse_text_shortcodes`]: crate::transforms::parse_text_shortcodes
fn code_fence_include_line(line: &str, source_info: &SourceInfo) -> Option<String> {
    let segments = crate::transforms::parse_text_shortcodes(line, source_info)?;

    // Exactly one shortcode, and every other segment blank.
    let mut shortcode = None;
    for segment in &segments {
        match segment {
            crate::transforms::TextSegment::Literal(text) => {
                if !text.trim().is_empty() {
                    return None;
                }
            }
            crate::transforms::TextSegment::Shortcode(s) => {
                if shortcode.is_some() {
                    return None;
                }
                shortcode = Some(s);
            }
        }
    }

    let shortcode = shortcode?;
    if shortcode.name != "include" {
        return None;
    }
    shortcode.positional_args.first().and_then(|arg| match arg {
        ShortcodeArg::String(s) => Some(s.clone()),
        _ => None,
    })
}

/// Every include a code fence's text contributes, as
/// `(line index, raw path)` in document order.
///
/// This is the **single source of truth** for "does this fence contain
/// an include", in the same way [`child_block_lists_mut`] is for
/// block-list positions: the expander
/// ([`IncludeExpander::expand_code_fence`]) and the path collector
/// ([`collect_include_paths`], which feeds the preview dep-graph) both
/// go through it, so neither can drift from the other. It also owns the
/// opt-out check, so "which fences are off-limits" is answered in one
/// place too.
fn code_fence_includes(code_block: &quarto_pandoc_types::block::CodeBlock) -> Vec<(usize, String)> {
    if crate::transforms::code_shortcode_opt_out(&code_block.attr) {
        return Vec::new();
    }
    code_fence_include_lines(&code_block.text, &code_block.source_info)
}

/// The include lines in a block of fence text, as `(line index, raw
/// path)`.
///
/// Split out from [`code_fence_includes`] because recursion re-scans
/// *spliced* text, which has no `CodeBlock` of its own — the opt-out is
/// a property of the originating fence and is checked once, there.
fn code_fence_include_lines(text: &str, source_info: &SourceInfo) -> Vec<(usize, String)> {
    text.split('\n')
        .enumerate()
        .filter_map(|(idx, line)| code_fence_include_line(line, source_info).map(|p| (idx, p)))
        .collect()
}

/// Drop exactly one trailing newline (and the `\r` of a `\r\n`).
///
/// The qmd parser's convention for a fence whose last line has content
/// is text *without* a trailing newline, and the HTML writer emits that
/// text verbatim — so splicing a POSIX source file's bytes as-is would
/// give every listing a blank final line. Q1's rendered output has
/// none: it appends newlines when splicing, but Pandoc's markdown
/// re-read absorbs them. q2 emits HTML straight from the AST with no
/// such re-read, so the normalization has to happen here.
///
/// Exactly one, not all: a file ending in two newlines is how an author
/// asks for a blank final line, the same affordance a hand-written
/// fence has.
fn trim_one_trailing_newline(bytes: &[u8]) -> &[u8] {
    let bytes = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    bytes.strip_suffix(b"\r").unwrap_or(bytes)
}

/// Check if a block is a paragraph (or plain block) containing only an
/// include shortcode. Returns the include path if so, None otherwise.
///
/// `Plain` is accepted alongside `Paragraph` because tight list items
/// and table cells wrap their content in `Plain` — a lone
/// `- {{< include … >}}` item parses as `Plain[Shortcode]`
/// (bd-1fz3vh99).
///
/// Public so other crates that need the same "what does this page
/// include?" answer (Phase D.6's `/api/preview/deps` endpoint) share
/// the *exact* recognition rules `IncludeExpansionStage` uses — no
/// drift between "what the renderer treats as an include" and "what
/// the preview dep filter considers a dependency."
pub fn extract_include_path(block: &Block) -> Option<String> {
    let inlines = match block {
        Block::Paragraph(para) => &para.content,
        Block::Plain(plain) => &plain.content,
        _ => return None,
    };

    // Must contain exactly one inline, and it must be a shortcode
    if inlines.len() != 1 {
        return None;
    }

    let Inline::Shortcode(shortcode) = &inlines[0] else {
        return None;
    };

    if shortcode.name != "include" {
        return None;
    }

    // Extract file path from first positional argument
    shortcode.positional_args.first().and_then(|arg| match arg {
        ShortcodeArg::String(s) => Some(s.clone()),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::block::Paragraph;
    use quarto_pandoc_types::inline::Str;
    use quarto_pandoc_types::shortcode::Shortcode;
    use quarto_source_map::{FileId, SourceInfo};
    use std::collections::HashMap;

    fn make_include_paragraph(path: &str) -> Block {
        Block::Paragraph(Paragraph {
            content: vec![Inline::Shortcode(Shortcode {
                is_escaped: false,
                name: "include".to_string(),
                positional_args: vec![ShortcodeArg::String(path.to_string())],
                keyword_args: hashlink::LinkedHashMap::new(),
                source_info: SourceInfo::original(FileId(0), 0, 30),
            })],
            source_info: SourceInfo::original(FileId(0), 0, 30),
        })
    }

    // === resolve_include_target (bd-w9koo1i2) ===

    #[test]
    fn resolve_relative_against_base_dir() {
        assert_eq!(
            resolve_include_target(Path::new("/proj/sub"), Path::new("/proj"), "x.qmd"),
            Path::new("/proj/sub/x.qmd")
        );
        assert_eq!(
            resolve_include_target(Path::new("/proj/sub"), Path::new("/proj"), "../up.qmd"),
            Path::new("/proj/sub/../up.qmd")
        );
    }

    #[test]
    fn resolve_leading_slash_against_project_dir() {
        assert_eq!(
            resolve_include_target(Path::new("/proj/sub"), Path::new("/proj"), "/a/b.qmd"),
            Path::new("/proj/a/b.qmd")
        );
        // Already at the root: same anchor.
        assert_eq!(
            resolve_include_target(Path::new("/proj"), Path::new("/proj"), "/a.qmd"),
            Path::new("/proj/a.qmd")
        );
    }

    #[test]
    fn resolve_leading_backslash_is_project_root_relative() {
        // Windows-authored separators normalize before anchoring.
        assert_eq!(
            resolve_include_target(Path::new("/proj/sub"), Path::new("/proj"), "\\a\\b.qmd"),
            Path::new("/proj/a/b.qmd")
        );
    }

    #[test]
    fn resolve_redundant_leading_slashes_collapse() {
        // Path comparison is component-wise, so the interior `//`
        // in the joined form is equivalent to `/`.
        assert_eq!(
            resolve_include_target(Path::new("/proj/sub"), Path::new("/proj"), "//a//b.qmd"),
            Path::new("/proj/a/b.qmd")
        );
    }

    #[test]
    fn extract_include_path_from_paragraph() {
        let block = make_include_paragraph("other.qmd");
        assert_eq!(extract_include_path(&block), Some("other.qmd".to_string()));
    }

    #[test]
    fn extract_include_path_non_include_shortcode() {
        let block = Block::Paragraph(Paragraph {
            content: vec![Inline::Shortcode(Shortcode {
                is_escaped: false,
                name: "meta".to_string(),
                positional_args: vec![ShortcodeArg::String("title".to_string())],
                keyword_args: hashlink::LinkedHashMap::new(),
                source_info: SourceInfo::for_test(),
            })],
            source_info: SourceInfo::for_test(),
        });
        assert_eq!(extract_include_path(&block), None);
    }

    #[test]
    fn extract_include_path_inline_include_not_detected() {
        // Paragraph with text + include shortcode → NOT an include
        let block = Block::Paragraph(Paragraph {
            content: vec![
                Inline::Str(Str {
                    text: "some text ".to_string(),
                    source_info: SourceInfo::for_test(),
                }),
                Inline::Shortcode(Shortcode {
                    is_escaped: false,
                    name: "include".to_string(),
                    positional_args: vec![ShortcodeArg::String("file.qmd".to_string())],
                    keyword_args: hashlink::LinkedHashMap::new(),
                    source_info: SourceInfo::for_test(),
                }),
            ],
            source_info: SourceInfo::for_test(),
        });
        assert_eq!(extract_include_path(&block), None);
    }

    #[test]
    fn extract_include_path_from_non_paragraph() {
        // `extract_include_path` recognizes *block-position* includes
        // only. A code fence is not one: its include lives in the
        // fence's text and is recognized by `code_fence_includes`
        // instead (bd-include-in-code-block-f8mvtczn). The two
        // recognizers are disjoint by design — this test pins both
        // halves so neither silently grows into the other's territory.
        let block = make_code_block("{{< include file.qmd >}}", &[], &[]);
        assert_eq!(extract_include_path(&block), None);

        let Block::CodeBlock(code_block) = &block else {
            unreachable!()
        };
        assert_eq!(
            code_fence_includes(code_block),
            vec![(0, "file.qmd".to_string())]
        );
    }

    // === Code-fence includes (bd-include-in-code-block-f8mvtczn) ===

    fn make_code_block(text: &str, classes: &[&str], kvs: &[(&str, &str)]) -> Block {
        let mut attrs = hashlink::LinkedHashMap::new();
        for (k, v) in kvs {
            attrs.insert(k.to_string(), v.to_string());
        }
        Block::CodeBlock(quarto_pandoc_types::block::CodeBlock {
            attr: (
                String::new(),
                classes.iter().map(|c| c.to_string()).collect(),
                attrs,
            ),
            text: text.to_string(),
            source_info: SourceInfo::for_test(),
            attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
        })
    }

    /// Recognize on one line, for the strictness tests.
    fn recognize(line: &str) -> Option<String> {
        code_fence_include_line(line, &SourceInfo::for_test())
    }

    #[test]
    fn code_fence_line_accepts_lone_include() {
        assert_eq!(recognize("{{< include app.py >}}"), Some("app.py".into()));
    }

    #[test]
    fn code_fence_line_accepts_surrounding_whitespace() {
        // Q1's `isBlockShortcode` anchors with `^\s*…\s*$`, so an
        // indented include line is still an include (and splices
        // without re-indentation).
        assert_eq!(
            recognize("    {{< include app.py >}}"),
            Some("app.py".into())
        );
        assert_eq!(
            recognize("\t{{< include app.py >}}  "),
            Some("app.py".into())
        );
        // A trailing `\r` from CRLF input counts as whitespace.
        assert_eq!(recognize("{{< include app.py >}}\r"), Some("app.py".into()));
    }

    #[test]
    fn code_fence_line_accepts_quoted_path() {
        assert_eq!(
            recognize(r#"{{< include "my file.py" >}}"#),
            Some("my file.py".into())
        );
    }

    #[test]
    fn code_fence_line_rejects_mid_line() {
        // D2: strict — the shortcode must be the sole content of its
        // line. Relaxing this later is possible; tightening is not.
        assert_eq!(recognize("x = 1  {{< include app.py >}}"), None);
        assert_eq!(recognize("{{< include app.py >}} # trailing"), None);
    }

    #[test]
    fn code_fence_line_rejects_two_per_line() {
        assert_eq!(recognize("{{< include a.py >}}{{< include b.py >}}"), None);
    }

    #[test]
    fn code_fence_line_rejects_other_shortcodes() {
        // `{{< meta … >}}` in a fence keeps its existing text-level
        // behavior via ShortcodeResolveTransform; include expansion
        // must not claim it.
        assert_eq!(recognize("{{< meta version >}}"), None);
        assert_eq!(recognize("{{< var key >}}"), None);
    }

    #[test]
    fn code_fence_line_rejects_escaped_include() {
        // `{{{< … >}}}` is the documented "render literally" form.
        assert_eq!(recognize("{{{< include app.py >}}}"), None);
    }

    #[test]
    fn code_fence_line_rejects_include_without_path() {
        assert_eq!(recognize("{{< include >}}"), None);
    }

    #[test]
    fn code_fence_line_rejects_plain_code() {
        assert_eq!(recognize("import os"), None);
        assert_eq!(recognize(""), None);
    }

    #[test]
    fn code_fence_includes_reports_every_line_in_order() {
        let block = make_code_block(
            "before\n{{< include a.py >}}\nmiddle\n{{< include b.py >}}",
            &["python"],
            &[],
        );
        let Block::CodeBlock(cb) = &block else {
            unreachable!()
        };
        assert_eq!(
            code_fence_includes(cb),
            vec![(1, "a.py".to_string()), (3, "b.py".to_string())]
        );
    }

    #[test]
    fn code_fence_includes_respects_shortcodes_false() {
        // D5: the authored opt-out must keep winning. This is how the
        // docs show include syntax without expanding it.
        let block = make_code_block(
            "{{< include app.py >}}",
            &["markdown"],
            &[("shortcodes", "false")],
        );
        let Block::CodeBlock(cb) = &block else {
            unreachable!()
        };
        assert_eq!(code_fence_includes(cb), vec![]);
    }

    #[test]
    fn code_fence_includes_respects_cell_code_class() {
        // `.cell-code` is engine-produced and cannot carry an authored
        // include at this stage, but the opt-out predicate is shared
        // with ShortcodeResolveTransform and must behave identically.
        let block = make_code_block("{{< include app.py >}}", &["python", "cell-code"], &[]);
        let Block::CodeBlock(cb) = &block else {
            unreachable!()
        };
        assert_eq!(code_fence_includes(cb), vec![]);
    }

    // === Trailing-newline normalization (D4) ===

    #[test]
    fn trim_one_trailing_newline_removes_exactly_one() {
        // q2's parser yields fence text WITHOUT a trailing newline for
        // a content-final line, and the HTML writer emits the text
        // verbatim — so splicing a POSIX file's bytes as-is would add a
        // blank final line to every listing. Q1's rendered output has
        // none (Pandoc's markdown re-read absorbs it); q2 has no such
        // re-read, so the trim happens here.
        assert_eq!(trim_one_trailing_newline(b"import os\n"), b"import os");
        assert_eq!(trim_one_trailing_newline(b"import os\r\n"), b"import os");
    }

    #[test]
    fn trim_one_trailing_newline_keeps_the_rest() {
        // Two trailing newlines is how an author asks for a blank final
        // line — the same affordance a hand-written fence has.
        assert_eq!(trim_one_trailing_newline(b"import os\n\n"), b"import os\n");
    }

    #[test]
    fn trim_one_trailing_newline_is_a_noop_without_one() {
        assert_eq!(trim_one_trailing_newline(b"import os"), b"import os");
        assert_eq!(trim_one_trailing_newline(b""), b"");
    }

    #[test]
    fn extract_include_path_empty_paragraph() {
        let block = Block::Paragraph(Paragraph {
            content: vec![],
            source_info: SourceInfo::for_test(),
        });
        assert_eq!(extract_include_path(&block), None);
    }

    // === Integration tests using expand_includes_in_blocks ===

    use std::path::PathBuf;
    use std::sync::Arc;

    /// Mock runtime that serves file content from an in-memory map.
    struct MockFileRuntime {
        files: HashMap<PathBuf, Vec<u8>>,
    }

    impl MockFileRuntime {
        fn new(files: Vec<(&str, &str)>) -> Self {
            Self {
                files: files
                    .into_iter()
                    .map(|(p, c)| (PathBuf::from(p), c.as_bytes().to_vec()))
                    .collect(),
            }
        }
    }

    // Delegate all SystemRuntime methods to defaults except file_read/path_exists/canonicalize
    macro_rules! mock_runtime_stubs {
        () => {
            fn file_write(
                &self,
                _path: &std::path::Path,
                _contents: &[u8],
            ) -> quarto_system_runtime::RuntimeResult<()> {
                Ok(())
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
            fn file_remove(
                &self,
                _path: &std::path::Path,
            ) -> quarto_system_runtime::RuntimeResult<()> {
                Ok(())
            }
            fn path_metadata(
                &self,
                _path: &std::path::Path,
            ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::PathMetadata> {
                unimplemented!()
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
            fn temp_dir(
                &self,
                _template: &str,
            ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::TempDir> {
                Ok(quarto_system_runtime::TempDir::new(PathBuf::from(
                    "/tmp/test",
                )))
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
            ) -> quarto_system_runtime::RuntimeResult<std::collections::HashMap<String, String>> {
                Ok(std::collections::HashMap::new())
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
        };
    }

    #[async_trait::async_trait]
    impl quarto_system_runtime::SystemRuntime for MockFileRuntime {
        fn file_read(
            &self,
            path: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<Vec<u8>> {
            self.files.get(path).cloned().ok_or_else(|| {
                quarto_system_runtime::RuntimeError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("mock: file not found: {}", path.display()),
                ))
            })
        }
        fn path_exists(
            &self,
            path: &std::path::Path,
            _kind: Option<quarto_system_runtime::PathKind>,
        ) -> quarto_system_runtime::RuntimeResult<bool> {
            Ok(self.files.contains_key(path))
        }
        fn canonicalize(
            &self,
            path: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            Ok(path.to_path_buf())
        }
        async fn fetch_url(
            &self,
            _url: &str,
        ) -> quarto_system_runtime::RuntimeResult<(Vec<u8>, String)> {
            Err(quarto_system_runtime::RuntimeError::NotSupported(
                "mock".to_string(),
            ))
        }
        mock_runtime_stubs!();
    }

    fn make_stage_context(runtime: Arc<dyn quarto_system_runtime::SystemRuntime>) -> StageContext {
        use crate::format::Format;
        use crate::project::{DocumentInfo, ProjectContext};

        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: crate::project::ProjectConfig::default(),
            is_single_file: true,
            files: vec![],
            output_dir: PathBuf::from("/project"),

            ..Default::default()
        };
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();

        StageContext::new(runtime, format, project, doc).unwrap()
    }

    fn parse_to_doc_ast(content: &str, path: &str) -> DocumentAst {
        let mut stderr = Vec::new();
        let (pandoc, ast_context, _warnings) =
            pampa::readers::qmd::read(content.as_bytes(), false, path, &mut stderr, true, None)
                .expect("parse failed");

        // Register the main file in source_context (mirrors ParseDocumentStage)
        let mut source_context = quarto_source_map::SourceContext::new();
        source_context.add_file(path.to_string(), Some(content.to_string()));

        DocumentAst {
            path: PathBuf::from(path),
            ast: pandoc,
            ast_context,
            source_context,
            warnings: vec![],
            recorded_includes: Vec::new(),
        }
    }

    #[test]
    fn simple_include_replaces_paragraph() {
        let runtime = Arc::new(MockFileRuntime::new(vec![(
            "/project/included.qmd",
            "Included content",
        )]));
        let mut ctx = make_stage_context(runtime);

        let mut doc = parse_to_doc_ast(
            "Before\n\n{{< include included.qmd >}}\n\nAfter",
            "/project/doc.qmd",
        );

        let mut include_stack = HashSet::new();
        include_stack.insert(PathBuf::from("/project/doc.qmd"));

        expand_includes_in_blocks(
            &mut doc,
            &mut ctx,
            &PathBuf::from("/project/doc.qmd"),
            &mut include_stack,
        )
        .unwrap();

        assert!(ctx.diagnostics.is_empty(), "No errors expected");

        // Should have 3 blocks: Before paragraph, Included content paragraph, After paragraph
        assert_eq!(
            doc.ast.blocks.len(),
            3,
            "Expected 3 blocks after include expansion, got {}",
            doc.ast.blocks.len()
        );
    }

    #[test]
    fn missing_file_produces_diagnostic() {
        let runtime = Arc::new(MockFileRuntime::new(vec![]));
        let mut ctx = make_stage_context(runtime);

        let mut doc = parse_to_doc_ast("{{< include nonexistent.qmd >}}", "/project/doc.qmd");

        let mut include_stack = HashSet::new();
        include_stack.insert(PathBuf::from("/project/doc.qmd"));

        expand_includes_in_blocks(
            &mut doc,
            &mut ctx,
            &PathBuf::from("/project/doc.qmd"),
            &mut include_stack,
        )
        .unwrap();

        assert_eq!(ctx.diagnostics.len(), 1);
        assert!(ctx.diagnostics[0].title.contains("not found"));
    }

    #[test]
    fn circular_include_produces_diagnostic() {
        // doc.qmd includes circular.qmd, which includes doc.qmd
        let runtime = Arc::new(MockFileRuntime::new(vec![(
            "/project/circular.qmd",
            "{{< include doc.qmd >}}",
        )]));
        let mut ctx = make_stage_context(runtime);

        let mut doc = parse_to_doc_ast("{{< include circular.qmd >}}", "/project/doc.qmd");

        let mut include_stack = HashSet::new();
        include_stack.insert(PathBuf::from("/project/doc.qmd"));

        expand_includes_in_blocks(
            &mut doc,
            &mut ctx,
            &PathBuf::from("/project/doc.qmd"),
            &mut include_stack,
        )
        .unwrap();

        // Should have a circular include diagnostic
        assert!(
            ctx.diagnostics.iter().any(|d| d.title.contains("Circular")),
            "Expected circular include diagnostic, got: {:?}",
            ctx.diagnostics
        );
    }

    #[test]
    fn recursive_include_works() {
        // doc includes a.qmd, a.qmd includes b.qmd
        let runtime = Arc::new(MockFileRuntime::new(vec![
            ("/project/a.qmd", "From A\n\n{{< include b.qmd >}}"),
            ("/project/b.qmd", "From B"),
        ]));
        let mut ctx = make_stage_context(runtime);

        let mut doc = parse_to_doc_ast(
            "Before\n\n{{< include a.qmd >}}\n\nAfter",
            "/project/doc.qmd",
        );

        let mut include_stack = HashSet::new();
        include_stack.insert(PathBuf::from("/project/doc.qmd"));

        expand_includes_in_blocks(
            &mut doc,
            &mut ctx,
            &PathBuf::from("/project/doc.qmd"),
            &mut include_stack,
        )
        .unwrap();

        assert!(
            ctx.diagnostics.is_empty(),
            "No errors expected: {:?}",
            ctx.diagnostics
        );

        // Should have 4 blocks: Before, From A, From B, After
        assert_eq!(
            doc.ast.blocks.len(),
            4,
            "Expected 4 blocks after recursive include, got {}",
            doc.ast.blocks.len()
        );
    }

    #[test]
    fn included_file_frontmatter_stripped() {
        let runtime = Arc::new(MockFileRuntime::new(vec![(
            "/project/with_yaml.qmd",
            "---\ntitle: Included\n---\n\nIncluded body",
        )]));
        let mut ctx = make_stage_context(runtime);

        let mut doc = parse_to_doc_ast("{{< include with_yaml.qmd >}}", "/project/doc.qmd");

        let mut include_stack = HashSet::new();
        include_stack.insert(PathBuf::from("/project/doc.qmd"));

        expand_includes_in_blocks(
            &mut doc,
            &mut ctx,
            &PathBuf::from("/project/doc.qmd"),
            &mut include_stack,
        )
        .unwrap();

        assert!(ctx.diagnostics.is_empty(), "No errors expected");

        // Should have just the body paragraph, not the YAML metadata
        assert_eq!(doc.ast.blocks.len(), 1);
        // Verify it's the body content, not metadata
        if let Block::Paragraph(p) = &doc.ast.blocks[0] {
            let text: String = p
                .content
                .iter()
                .filter_map(|i| {
                    if let Inline::Str(s) = i {
                        Some(s.text.as_str())
                    } else {
                        None
                    }
                })
                .collect();
            assert!(
                text.contains("Included"),
                "Expected 'Included' body text, got: {}",
                text
            );
        }
    }

    #[test]
    fn included_blocks_have_correct_file_id() {
        let runtime = Arc::new(MockFileRuntime::new(vec![(
            "/project/other.qmd",
            "Other content",
        )]));
        let mut ctx = make_stage_context(runtime);

        let mut doc = parse_to_doc_ast("Main\n\n{{< include other.qmd >}}", "/project/doc.qmd");

        let mut include_stack = HashSet::new();
        include_stack.insert(PathBuf::from("/project/doc.qmd"));

        expand_includes_in_blocks(
            &mut doc,
            &mut ctx,
            &PathBuf::from("/project/doc.qmd"),
            &mut include_stack,
        )
        .unwrap();

        assert!(ctx.diagnostics.is_empty());
        assert_eq!(doc.ast.blocks.len(), 2);

        // First block (Main) should have FileId(0)
        let main_si = doc.ast.blocks[0].source_info();
        if let SourceInfo::Original { file_id, .. } = main_si {
            assert_eq!(*file_id, FileId(0), "Main block should be FileId(0)");
        }

        // Second block (Other content) should have a different FileId (the included file)
        let included_si = doc.ast.blocks[1].source_info();
        if let SourceInfo::Original { file_id, .. } = included_si {
            assert_ne!(
                *file_id,
                FileId(0),
                "Included block should NOT be FileId(0)"
            );
        }
    }

    #[test]
    fn inline_include_not_expanded() {
        // Include shortcode among other inlines should NOT be expanded
        let runtime = Arc::new(MockFileRuntime::new(vec![(
            "/project/other.qmd",
            "Included",
        )]));
        let mut ctx = make_stage_context(runtime);

        let mut doc = parse_to_doc_ast("text {{< include other.qmd >}} more", "/project/doc.qmd");
        let original_block_count = doc.ast.blocks.len();

        let mut include_stack = HashSet::new();
        include_stack.insert(PathBuf::from("/project/doc.qmd"));

        expand_includes_in_blocks(
            &mut doc,
            &mut ctx,
            &PathBuf::from("/project/doc.qmd"),
            &mut include_stack,
        )
        .unwrap();

        // Block count should be unchanged — inline include not expanded
        assert_eq!(doc.ast.blocks.len(), original_block_count);
    }

    /// Trips the parser: bare apostrophe after a plural noun reads as
    /// an unmatched closing smart quote (Q-2-10).
    const UNPARSEABLE_QMD: &str =
        "This line mentions the groups' Unique IDs instead of their names.\n";

    fn has_include_shortcode_block(blocks: &[Block]) -> bool {
        blocks.iter().any(|b| extract_include_path(b).is_some())
    }

    #[test]
    fn parse_error_include_removes_block_and_surfaces_inner_diagnostics() {
        let runtime = Arc::new(MockFileRuntime::new(vec![(
            "/project/bad.qmd",
            UNPARSEABLE_QMD,
        )]));
        let mut ctx = make_stage_context(runtime);

        let mut doc = parse_to_doc_ast(
            "Before\n\n{{< include bad.qmd >}}\n\nAfter",
            "/project/doc.qmd",
        );

        let mut include_stack = HashSet::new();
        include_stack.insert(PathBuf::from("/project/doc.qmd"));

        expand_includes_in_blocks(
            &mut doc,
            &mut ctx,
            &PathBuf::from("/project/doc.qmd"),
            &mut include_stack,
        )
        .unwrap();

        // The failed include's paragraph is removed: only Before/After
        // remain, and no include shortcode survives to later transforms.
        assert_eq!(
            doc.ast.blocks.len(),
            2,
            "expected the failed include block to be removed, got {:?}",
            doc.ast.blocks
        );
        assert!(!has_include_shortcode_block(&doc.ast.blocks));

        // Wrapper first, then the included file's own diagnostics.
        assert!(
            ctx.diagnostics.len() >= 2,
            "expected wrapper + inner diagnostics, got: {:?}",
            ctx.diagnostics
        );
        assert_eq!(ctx.diagnostics[0].code.as_deref(), Some("Q-17-3"));

        // The inner diagnostic's location must resolve — through the
        // parent document's SourceContext — to the included file.
        let inner = &ctx.diagnostics[1];
        let loc = inner.location.as_ref().expect("inner has a location");
        let mapped = loc
            .map_offset(0, &doc.source_context)
            .expect("inner location resolves in the parent SourceContext");
        let file = doc
            .source_context
            .get_file(mapped.file_id)
            .expect("mapped file registered");
        assert!(
            file.path.ends_with("bad.qmd"),
            "inner diagnostic must point into the included file, got {}",
            file.path
        );

        // Both source contexts grew in lockstep (the success path
        // debug_asserts this; the error path must preserve it so a
        // later successful include doesn't desynchronize).
        let in_ast_ctx = doc
            .ast_context
            .source_context
            .get_file(mapped.file_id)
            .expect("included file also registered in ast_context");
        assert!(in_ast_ctx.path.ends_with("bad.qmd"));
    }

    #[test]
    fn missing_include_removes_block() {
        let runtime = Arc::new(MockFileRuntime::new(vec![]));
        let mut ctx = make_stage_context(runtime);

        let mut doc = parse_to_doc_ast(
            "Before\n\n{{< include nonexistent.qmd >}}\n\nAfter",
            "/project/doc.qmd",
        );

        let mut include_stack = HashSet::new();
        include_stack.insert(PathBuf::from("/project/doc.qmd"));

        expand_includes_in_blocks(
            &mut doc,
            &mut ctx,
            &PathBuf::from("/project/doc.qmd"),
            &mut include_stack,
        )
        .unwrap();

        assert_eq!(ctx.diagnostics.len(), 1);
        assert_eq!(ctx.diagnostics[0].code.as_deref(), Some("Q-17-2"));
        assert_eq!(
            doc.ast.blocks.len(),
            2,
            "expected the failed include block to be removed, got {:?}",
            doc.ast.blocks
        );
        assert!(!has_include_shortcode_block(&doc.ast.blocks));
    }

    #[test]
    fn circular_include_removes_block() {
        let runtime = Arc::new(MockFileRuntime::new(vec![(
            "/project/circular.qmd",
            "In the loop\n\n{{< include doc.qmd >}}",
        )]));
        let mut ctx = make_stage_context(runtime);

        let mut doc = parse_to_doc_ast("{{< include circular.qmd >}}", "/project/doc.qmd");

        let mut include_stack = HashSet::new();
        include_stack.insert(PathBuf::from("/project/doc.qmd"));

        expand_includes_in_blocks(
            &mut doc,
            &mut ctx,
            &PathBuf::from("/project/doc.qmd"),
            &mut include_stack,
        )
        .unwrap();

        assert_eq!(ctx.diagnostics.len(), 1);
        assert_eq!(ctx.diagnostics[0].code.as_deref(), Some("Q-17-1"));
        assert!(
            !has_include_shortcode_block(&doc.ast.blocks),
            "the cyclic include block must be removed, got {:?}",
            doc.ast.blocks
        );
    }

    // === Nested-container expansion (bd-1fz3vh99) ===
    //
    // One rule: an include block (Paragraph/Plain whose sole inline is
    // the include shortcode) expands at ANY block-list position. The
    // shared driver below runs the doc-level entry point and returns
    // (doc, ctx) for structural assertions.

    /// Parses OK but emits a Q-2-9 "HTML element converted to raw
    /// HTML" warning — used to pin warning surfacing (U10).
    const WARNING_QMD: &str = "<div>\n\nhello\n\n</div>\n";

    fn expand(main: &str, files: Vec<(&str, &str)>) -> (DocumentAst, StageContext) {
        let runtime = Arc::new(MockFileRuntime::new(files));
        let mut ctx = make_stage_context(runtime);
        let mut doc = parse_to_doc_ast(main, "/project/doc.qmd");

        let mut include_stack = HashSet::new();
        include_stack.insert(PathBuf::from("/project/doc.qmd"));

        expand_includes_in_blocks(
            &mut doc,
            &mut ctx,
            &PathBuf::from("/project/doc.qmd"),
            &mut include_stack,
        )
        .unwrap();
        (doc, ctx)
    }

    /// Collapse a block to its plain-text content (Str/Space only) for
    /// structural assertions.
    fn block_text(block: &Block) -> String {
        let inlines = match block {
            Block::Paragraph(p) => &p.content,
            Block::Plain(p) => &p.content,
            _ => return String::new(),
        };
        inlines
            .iter()
            .map(|i| match i {
                Inline::Str(s) => s.text.clone(),
                Inline::Space(_) => " ".to_string(),
                _ => String::new(),
            })
            .collect()
    }

    fn texts(blocks: &[Block]) -> Vec<String> {
        blocks.iter().map(block_text).collect()
    }

    const INC_TWO_PARAS: &str = "Included one\n\nIncluded two\n";

    #[test]
    fn include_inside_div_expands() {
        let (doc, ctx) = expand(
            "::: {.note}\n{{< include inc.qmd >}}\n:::",
            vec![("/project/inc.qmd", INC_TWO_PARAS)],
        );
        assert!(ctx.diagnostics.is_empty(), "{:?}", ctx.diagnostics);
        assert_eq!(doc.ast.blocks.len(), 1);
        let Block::Div(div) = &doc.ast.blocks[0] else {
            panic!("expected Div, got {:?}", doc.ast.blocks[0]);
        };
        assert_eq!(
            texts(&div.content),
            vec!["Included one", "Included two"],
            "included blocks must land inside the div"
        );
    }

    #[test]
    fn include_inside_blockquote_expands() {
        let (doc, ctx) = expand(
            "> {{< include inc.qmd >}}",
            vec![("/project/inc.qmd", INC_TWO_PARAS)],
        );
        assert!(ctx.diagnostics.is_empty(), "{:?}", ctx.diagnostics);
        assert_eq!(doc.ast.blocks.len(), 1);
        let Block::BlockQuote(bq) = &doc.ast.blocks[0] else {
            panic!("expected BlockQuote, got {:?}", doc.ast.blocks[0]);
        };
        assert_eq!(texts(&bq.content), vec!["Included one", "Included two"]);
    }

    #[test]
    fn include_as_tight_bullet_item_expands() {
        // Tight list items hold Plain, not Paragraph — the recognizer
        // must accept both shapes.
        let (doc, ctx) = expand(
            "- {{< include inc.qmd >}}\n- other item",
            vec![("/project/inc.qmd", INC_TWO_PARAS)],
        );
        assert!(ctx.diagnostics.is_empty(), "{:?}", ctx.diagnostics);
        let Block::BulletList(list) = &doc.ast.blocks[0] else {
            panic!("expected BulletList, got {:?}", doc.ast.blocks[0]);
        };
        assert_eq!(list.content.len(), 2, "sibling item must survive");
        assert_eq!(
            texts(&list.content[0]),
            vec!["Included one", "Included two"],
            "include expands inside its own item"
        );
        assert_eq!(texts(&list.content[1]), vec!["other item"]);
    }

    #[test]
    fn include_in_ordered_list_item_expands() {
        let (doc, ctx) = expand(
            "1. {{< include inc.qmd >}}",
            vec![("/project/inc.qmd", INC_TWO_PARAS)],
        );
        assert!(ctx.diagnostics.is_empty(), "{:?}", ctx.diagnostics);
        let Block::OrderedList(list) = &doc.ast.blocks[0] else {
            panic!("expected OrderedList, got {:?}", doc.ast.blocks[0]);
        };
        assert_eq!(
            texts(&list.content[0]),
            vec!["Included one", "Included two"]
        );
    }

    #[test]
    fn include_in_nested_divs_expands() {
        let (doc, ctx) = expand(
            "::: {.outer}\n\n::: {.inner}\n{{< include inc.qmd >}}\n:::\n\n:::",
            vec![("/project/inc.qmd", "Deep content\n")],
        );
        assert!(ctx.diagnostics.is_empty(), "{:?}", ctx.diagnostics);
        let Block::Div(outer) = &doc.ast.blocks[0] else {
            panic!("expected outer Div, got {:?}", doc.ast.blocks[0]);
        };
        let Some(Block::Div(inner)) = outer.content.iter().find(|b| matches!(b, Block::Div(_)))
        else {
            panic!("expected inner Div, got {:?}", outer.content);
        };
        assert_eq!(texts(&inner.content), vec!["Deep content"]);
    }

    #[test]
    fn nested_include_resolves_relative_to_declaring_file() {
        // doc.qmd (in /project) includes sub/x.qmd inside a div;
        // x.qmd includes y.qmd, which must resolve against /project/sub.
        let (doc, ctx) = expand(
            "::: {.d}\n{{< include sub/x.qmd >}}\n:::",
            vec![
                ("/project/sub/x.qmd", "From X\n\n{{< include y.qmd >}}"),
                ("/project/sub/y.qmd", "From Y\n"),
            ],
        );
        assert!(ctx.diagnostics.is_empty(), "{:?}", ctx.diagnostics);
        let Block::Div(div) = &doc.ast.blocks[0] else {
            panic!("expected Div, got {:?}", doc.ast.blocks[0]);
        };
        assert_eq!(texts(&div.content), vec!["From X", "From Y"]);
    }

    #[test]
    fn cycle_through_container_reports_and_removes() {
        // doc → (div) loop.qmd → doc: the cyclic include is removed
        // from loop.qmd's spliced blocks, inside the div.
        let (doc, ctx) = expand(
            "::: {.d}\n{{< include loop.qmd >}}\n:::",
            vec![("/project/loop.qmd", "{{< include doc.qmd >}}")],
        );
        assert_eq!(ctx.diagnostics.len(), 1, "{:?}", ctx.diagnostics);
        assert_eq!(ctx.diagnostics[0].code.as_deref(), Some("Q-17-1"));
        let Block::Div(div) = &doc.ast.blocks[0] else {
            panic!("expected Div, got {:?}", doc.ast.blocks[0]);
        };
        assert!(
            div.content.is_empty(),
            "cyclic include must be removed from the div: {:?}",
            div.content
        );
    }

    #[test]
    fn parse_error_inside_div_reports_and_removes() {
        let (doc, ctx) = expand(
            "::: {.d}\n{{< include bad.qmd >}}\n:::",
            vec![("/project/bad.qmd", UNPARSEABLE_QMD)],
        );
        assert!(
            ctx.diagnostics.len() >= 2,
            "expected wrapper + inner diagnostics, got {:?}",
            ctx.diagnostics
        );
        assert_eq!(ctx.diagnostics[0].code.as_deref(), Some("Q-17-3"));
        let Block::Div(div) = &doc.ast.blocks[0] else {
            panic!("expected Div, got {:?}", doc.ast.blocks[0]);
        };
        assert!(
            div.content.is_empty(),
            "failed include must be removed from the div's children: {:?}",
            div.content
        );
    }

    #[test]
    fn include_in_table_cell_expands() {
        let (doc, ctx) = expand(
            "| {{< include inc.qmd >}} |\n|---|\n| cell |",
            vec![("/project/inc.qmd", INC_TWO_PARAS)],
        );
        assert!(ctx.diagnostics.is_empty(), "{:?}", ctx.diagnostics);
        let Block::Table(table) = &doc.ast.blocks[0] else {
            panic!("expected Table, got {:?}", doc.ast.blocks[0]);
        };
        let head_cell = &table.head.rows[0].cells[0];
        assert_eq!(
            texts(&head_cell.content),
            vec!["Included one", "Included two"],
            "include expands to multi-block content inside the cell"
        );
    }

    #[test]
    fn included_file_parse_warnings_surface_remapped() {
        // WARNING_QMD parses successfully but emits Q-2-9. The warning
        // must reach ctx.diagnostics with a location that resolves —
        // through the parent's SourceContext — to the included file.
        let (doc, ctx) = expand(
            "{{< include warn.qmd >}}",
            vec![("/project/warn.qmd", WARNING_QMD)],
        );
        let warning = ctx
            .diagnostics
            .iter()
            .find(|d| d.code.as_deref() == Some("Q-2-9"))
            .unwrap_or_else(|| {
                panic!(
                    "expected included file's Q-2-9 warning to surface, got {:?}",
                    ctx.diagnostics
                )
            });
        let loc = warning.location.as_ref().expect("warning has a location");
        let mapped = loc
            .map_offset(0, &doc.source_context)
            .expect("warning location resolves in the parent SourceContext");
        let file = doc
            .source_context
            .get_file(mapped.file_id)
            .expect("mapped file registered");
        assert!(
            file.path.ends_with("warn.qmd"),
            "warning must point into the included file, got {}",
            file.path
        );
    }

    #[test]
    fn include_with_code_cell() {
        let runtime = Arc::new(MockFileRuntime::new(vec![(
            "/project/code.qmd",
            "```python\nprint('hello')\n```",
        )]));
        let mut ctx = make_stage_context(runtime);

        let mut doc = parse_to_doc_ast(
            "Before\n\n{{< include code.qmd >}}\n\nAfter",
            "/project/doc.qmd",
        );

        let mut include_stack = HashSet::new();
        include_stack.insert(PathBuf::from("/project/doc.qmd"));

        expand_includes_in_blocks(
            &mut doc,
            &mut ctx,
            &PathBuf::from("/project/doc.qmd"),
            &mut include_stack,
        )
        .unwrap();

        assert!(ctx.diagnostics.is_empty());

        // Should have a CodeBlock from the included file
        let has_code_block = doc
            .ast
            .blocks
            .iter()
            .any(|b| matches!(b, Block::CodeBlock(_)));
        assert!(
            has_code_block,
            "Expected a CodeBlock from included file in the AST"
        );
    }

    // === Includes inside a fenced code block ===
    // (bd-include-in-code-block-f8mvtczn; the Q1 listing idiom)

    /// The text of the first code block anywhere in `blocks`.
    fn first_code_text(blocks: &[Block]) -> String {
        fn walk(blocks: &[Block]) -> Option<String> {
            for block in blocks {
                if let Block::CodeBlock(cb) = block {
                    return Some(cb.text.clone());
                }
                if let Block::Div(div) = block
                    && let Some(found) = walk(&div.content)
                {
                    return Some(found);
                }
            }
            None
        }
        walk(blocks).expect("expected a code block")
    }

    const APP_PY: &str = "import os\n\nprint(\"hello\")\n";

    #[test]
    fn code_fence_include_splices_file_text() {
        let (doc, ctx) = expand(
            "```{.python filename=\"app.py\"}\n{{< include app.py >}}\n```",
            vec![("/project/app.py", APP_PY)],
        );
        assert!(ctx.diagnostics.is_empty(), "{:?}", ctx.diagnostics);
        // Exactly the file's content, with the one trailing newline
        // trimmed (D4) so the listing has no blank final line.
        assert_eq!(
            first_code_text(&doc.ast.blocks),
            "import os\n\nprint(\"hello\")"
        );
    }

    #[test]
    fn code_fence_include_keeps_surrounding_lines() {
        let (doc, ctx) = expand(
            "```{.python}\n# header\n{{< include app.py >}}\n# footer\n```",
            vec![("/project/app.py", "body\n")],
        );
        assert!(ctx.diagnostics.is_empty(), "{:?}", ctx.diagnostics);
        assert_eq!(first_code_text(&doc.ast.blocks), "# header\nbody\n# footer");
    }

    #[test]
    fn code_fence_include_does_not_reindent() {
        // Q1 splices at column 0 regardless of the include line's own
        // indentation; matching that keeps existing documents stable.
        let (doc, ctx) = expand(
            "```{.python}\n    {{< include app.py >}}\n```",
            vec![("/project/app.py", "a\n  b\n")],
        );
        assert!(ctx.diagnostics.is_empty(), "{:?}", ctx.diagnostics);
        assert_eq!(first_code_text(&doc.ast.blocks), "a\n  b");
    }

    #[test]
    fn code_fence_include_expands_multiple_targets() {
        let (doc, ctx) = expand(
            "```{.python}\n{{< include a.py >}}\n{{< include b.py >}}\n```",
            vec![("/project/a.py", "AAA\n"), ("/project/b.py", "BBB\n")],
        );
        assert!(ctx.diagnostics.is_empty(), "{:?}", ctx.diagnostics);
        assert_eq!(first_code_text(&doc.ast.blocks), "AAA\nBBB");
    }

    #[test]
    fn code_fence_include_records_dependency() {
        // Without this the preview never rebuilds the page when the
        // embedded source file changes.
        let (doc, _ctx) = expand(
            "```{.python}\n{{< include app.py >}}\n```",
            vec![("/project/app.py", APP_PY)],
        );
        let recorded: Vec<_> = doc
            .recorded_includes
            .iter()
            .map(|e| e.path.clone())
            .collect();
        assert_eq!(recorded, vec![PathBuf::from("/project/app.py")]);
    }

    #[test]
    fn code_fence_include_resolves_project_absolute_path() {
        let (doc, ctx) = expand(
            "```{.python}\n{{< include /shared/app.py >}}\n```",
            vec![("/project/shared/app.py", "shared\n")],
        );
        assert!(ctx.diagnostics.is_empty(), "{:?}", ctx.diagnostics);
        assert_eq!(first_code_text(&doc.ast.blocks), "shared");
    }

    #[test]
    fn code_fence_include_missing_file_reports_and_drops_line() {
        let (doc, ctx) = expand("```{.python}\nkept\n{{< include gone.py >}}\n```", vec![]);
        let codes: Vec<_> = ctx
            .diagnostics
            .iter()
            .filter_map(|d| d.code.clone())
            .collect();
        assert_eq!(codes, vec!["Q-17-2".to_string()]);
        // The unresolved shortcode must not survive into the fence:
        // ShortcodeResolveTransform would later render it as the
        // `?include` token this strand exists to remove.
        let text = first_code_text(&doc.ast.blocks);
        assert_eq!(text, "kept");
        assert!(!text.contains("include"), "got {text:?}");
    }

    #[test]
    fn code_fence_include_respects_shortcodes_false() {
        let (doc, ctx) = expand(
            "```{.markdown shortcodes=\"false\"}\n{{< include app.py >}}\n```",
            vec![("/project/app.py", APP_PY)],
        );
        assert!(ctx.diagnostics.is_empty(), "{:?}", ctx.diagnostics);
        assert_eq!(first_code_text(&doc.ast.blocks), "{{< include app.py >}}");
    }

    #[test]
    fn code_fence_include_inside_div_expands() {
        // Fences nest wherever block lists do; the walker must reach
        // them at every level, not just the top.
        let (doc, ctx) = expand(
            "::: {.panel}\n```{.python}\n{{< include app.py >}}\n```\n:::",
            vec![("/project/app.py", "nested\n")],
        );
        assert!(ctx.diagnostics.is_empty(), "{:?}", ctx.diagnostics);
        assert_eq!(first_code_text(&doc.ast.blocks), "nested");
    }

    #[test]
    fn code_fence_include_recurses() {
        // D3 (revised): spliced text is re-scanned, matching Q1's
        // `standaloneInclude`. Recursion is also what stops a nested
        // include from surviving to ShortcodeResolveTransform, which
        // would render it as `?include` — the very bug being fixed,
        // one level down.
        let (doc, ctx) = expand(
            "```{.markdown}\n{{< include outer.qmd >}}\n```",
            vec![
                (
                    "/project/outer.qmd",
                    "top\n{{< include inner.qmd >}}\nbot\n",
                ),
                ("/project/inner.qmd", "INNER\n"),
            ],
        );
        assert!(ctx.diagnostics.is_empty(), "{:?}", ctx.diagnostics);
        assert_eq!(first_code_text(&doc.ast.blocks), "top\nINNER\nbot");
    }

    #[test]
    fn code_fence_include_recursion_anchors_at_the_including_file() {
        // Nested paths resolve against the file that declared them, as
        // block-position includes do (bd-1fz3vh99) — not against the
        // document. (Q1 anchors at the document here; ours is the
        // consistent rule.)
        let (doc, ctx) = expand(
            "```{.markdown}\n{{< include sub/outer.qmd >}}\n```",
            vec![
                ("/project/sub/outer.qmd", "{{< include sibling.qmd >}}\n"),
                ("/project/sub/sibling.qmd", "FROM-SUBDIR\n"),
            ],
        );
        assert!(ctx.diagnostics.is_empty(), "{:?}", ctx.diagnostics);
        assert_eq!(first_code_text(&doc.ast.blocks), "FROM-SUBDIR");
    }

    #[test]
    fn code_fence_include_cycle_reports_and_drops_line() {
        // A file that embeds itself as a listing would recurse forever:
        // the spliced copy carries the same include line.
        let (doc, ctx) = expand(
            "```{.markdown}\n{{< include a.qmd >}}\n```",
            vec![
                ("/project/a.qmd", "A-TOP\n{{< include b.qmd >}}\n"),
                ("/project/b.qmd", "B-TOP\n{{< include a.qmd >}}\n"),
            ],
        );
        let codes: Vec<_> = ctx
            .diagnostics
            .iter()
            .filter_map(|d| d.code.clone())
            .collect();
        assert_eq!(codes, vec!["Q-17-1".to_string()]);
        // Expansion stops at the cycle; the offending line is dropped
        // rather than left to become `?include`.
        assert_eq!(first_code_text(&doc.ast.blocks), "A-TOP\nB-TOP");
    }

    #[test]
    fn code_fence_include_of_self_reports_cycle() {
        // The document itself is already on the include stack.
        let (doc, ctx) = expand(
            "```{.markdown}\n{{< include doc.qmd >}}\n```",
            vec![("/project/doc.qmd", "anything\n")],
        );
        let codes: Vec<_> = ctx
            .diagnostics
            .iter()
            .filter_map(|d| d.code.clone())
            .collect();
        assert_eq!(codes, vec!["Q-17-1".to_string()]);
        assert_eq!(first_code_text(&doc.ast.blocks), "");
    }

    #[test]
    fn code_fence_include_leaves_other_shortcodes_alone() {
        // `{{< meta … >}}` keeps its text-level expansion downstream;
        // include expansion must not consume or disturb it.
        let (doc, ctx) = expand(
            "```{.python}\n{{< meta version >}}\n{{< include app.py >}}\n```",
            vec![("/project/app.py", "spliced\n")],
        );
        assert!(ctx.diagnostics.is_empty(), "{:?}", ctx.diagnostics);
        assert_eq!(
            first_code_text(&doc.ast.blocks),
            "{{< meta version >}}\nspliced"
        );
    }

    #[test]
    fn collect_include_paths_finds_code_fence_targets() {
        // The preview dep-graph shares this walker with the expander;
        // if it misses fence includes, editing the embedded file never
        // rebuilds the page that shows it.
        let mut doc = parse_to_doc_ast(
            "{{< include block.qmd >}}\n\n```{.python}\n{{< include fence.py >}}\n```",
            "/project/doc.qmd",
        );
        assert_eq!(
            collect_include_paths(&mut doc.ast.blocks),
            vec!["block.qmd".to_string(), "fence.py".to_string()]
        );
    }
}
