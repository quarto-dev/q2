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

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use async_trait::async_trait;

use quarto_pandoc_types::block::Blocks;
use quarto_pandoc_types::shortcode::ShortcodeArg;
use quarto_pandoc_types::{Block, Inline};

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
    /// whose source produced these blocks — include paths resolve
    /// against its directory.
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
                for list in child_block_lists_mut(&mut blocks[i]) {
                    self.expand_blocks(list, current_file)?;
                }
                i += 1;
                continue;
            };

            // Resolve relative to the including file's directory
            let base_dir = current_file.parent().unwrap_or(Path::new("."));
            let resolved = base_dir.join(&include_path);

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
        // Code blocks are never includes
        let block = Block::CodeBlock(quarto_pandoc_types::block::CodeBlock {
            attr: quarto_pandoc_types::attr::empty_attr(),
            text: "{{< include file.qmd >}}".to_string(),
            source_info: SourceInfo::for_test(),
            attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
        });
        assert_eq!(extract_include_path(&block), None);
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
}
