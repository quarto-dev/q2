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
//! Runs before engine execution so that included code cells are visible
//! to the engine.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use async_trait::async_trait;

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

/// Expand include shortcodes in a block list, recursively.
fn expand_includes_in_blocks(
    doc: &mut DocumentAst,
    ctx: &mut StageContext,
    current_file: &Path,
    include_stack: &mut HashSet<PathBuf>,
) -> Result<(), PipelineError> {
    let mut i = 0;
    while i < doc.ast.blocks.len() {
        if let Some(include_path) = extract_include_path(&doc.ast.blocks[i]) {
            // Resolve relative to the including file's directory
            let base_dir = current_file.parent().unwrap_or(Path::new("."));
            let resolved = base_dir.join(&include_path);

            // Canonicalize for cycle detection
            let canonical = resolved.canonicalize().unwrap_or_else(|_| resolved.clone());

            // Check for circular includes
            if include_stack.contains(&canonical) {
                ctx.diagnostics.push(
                    quarto_error_reporting::DiagnosticMessageBuilder::warning("Circular include")
                        .with_code("Q-17-1")
                        .with_location(doc.ast.blocks[i].source_info().clone())
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
                doc.ast.blocks.remove(i);
                continue;
            }

            // Read the included file
            let content = match ctx.runtime.file_read(&resolved) {
                Ok(bytes) => bytes,
                Err(e) => {
                    ctx.diagnostics.push(
                        quarto_error_reporting::DiagnosticMessageBuilder::warning(
                            "Include file not found",
                        )
                        .with_code("Q-17-2")
                        .with_location(doc.ast.blocks[i].source_info().clone())
                        .problem(format!(
                            "Could not read included file '{}': {}",
                            resolved.display(),
                            e
                        ))
                        .build(),
                    );
                    // See the circular-include arm for why the block is
                    // removed rather than skipped.
                    doc.ast.blocks.remove(i);
                    continue;
                }
            };

            // Parse the included file
            let mut stderr_buf = Vec::new();
            let filename = resolved.to_string_lossy().to_string();
            let parse_result =
                pampa::readers::qmd::read(&content, false, &filename, &mut stderr_buf, true, None);

            let (included_pandoc, included_ast_context, _warnings) = match parse_result {
                Ok(result) => result,
                Err(inner_diagnostics) => {
                    // Register the included file's content in both
                    // SourceContexts so the inner diagnostics can render
                    // ariadne snippets from it. Registering in *both* —
                    // even though only `doc.source_context` is read here —
                    // keeps the two contexts growing in lockstep, which
                    // the success path's debug_assert relies on for any
                    // later include in the same document.
                    let content_str = String::from_utf8_lossy(&content).into_owned();
                    let new_file_id = doc
                        .ast_context
                        .source_context
                        .add_file(filename.clone(), Some(content_str.clone()));
                    let snippet_file_id = doc
                        .source_context
                        .add_file(filename.clone(), Some(content_str));
                    debug_assert_eq!(
                        new_file_id, snippet_file_id,
                        "FileId mismatch between ast_context.source_context and source_context"
                    );

                    ctx.diagnostics.push(
                        quarto_error_reporting::DiagnosticMessageBuilder::error(
                            "Include file parse error",
                        )
                        .with_code("Q-17-3")
                        .with_location(doc.ast.blocks[i].source_info().clone())
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
                        if let Some(loc) = diag.location.as_mut() {
                            loc.remap_file_ids(&remap);
                        }
                        for detail in diag.details.iter_mut() {
                            if let Some(loc) = detail.location.as_mut() {
                                loc.remap_file_ids(&remap);
                            }
                        }
                        ctx.diagnostics.push(diag);
                    }

                    // The output still depends on the broken file's
                    // bytes: fixing it must invalidate any cached
                    // render, so record it like a successful include.
                    record_include(&mut doc.recorded_includes, &canonical, &content);

                    // See the circular-include arm for why the block is
                    // removed rather than skipped.
                    doc.ast.blocks.remove(i);
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
                doc.ast_context
                    .source_context
                    .add_file_with_info(filename.clone(), file_info)
            } else {
                doc.ast_context
                    .source_context
                    .add_file(filename.clone(), Some(content_str.clone()))
            };

            // Register in top-level source_context (for ariadne error snippets)
            // Use add_file which returns a new FileId, but we need the same one.
            // Since both contexts grow sequentially, they should stay in sync if
            // we register in the same order. However, to be safe we verify.
            let snippet_file_id = doc
                .source_context
                .add_file(filename.clone(), Some(content_str));
            debug_assert_eq!(
                new_file_id, snippet_file_id,
                "FileId mismatch between ast_context.source_context and source_context"
            );

            // Merge filenames
            for name in &included_ast_context.filenames {
                if !doc.ast_context.filenames.contains(name) {
                    doc.ast_context.filenames.push(name.clone());
                }
            }

            // Remap FileIds in the parsed AST: FileId(0) → new_file_id
            let mut included_blocks = included_pandoc.blocks;
            // Remap each block's source info and all nested source info
            let mut temp_pandoc = quarto_pandoc_types::pandoc::Pandoc {
                meta: quarto_pandoc_types::config_value::ConfigValue::default(),
                blocks: included_blocks,
            };
            quarto_ast_reconcile::remap_file_ids(&mut temp_pandoc, &|id| {
                if id == quarto_source_map::FileId(0) {
                    new_file_id
                } else {
                    id
                }
            });
            included_blocks = temp_pandoc.blocks;

            // Replace the paragraph containing the shortcode with included blocks
            doc.ast.blocks.remove(i);
            let num_inserted = included_blocks.len();
            for (j, block) in included_blocks.into_iter().enumerate() {
                doc.ast.blocks.insert(i + j, block);
            }

            // Record this child in the parent's `recorded_includes`
            // side-channel for `bd-r82e` / Phase-8 cache invalidation.
            // The `canonical` path is what cycle detection keys on;
            // when canonicalize fails, it falls back to the resolved
            // path (still a stable identifier for hashing). Dedupe so
            // a child included twice in different positions appears
            // once.
            record_include(&mut doc.recorded_includes, &canonical, &content);

            // Recursively expand includes in the newly inserted blocks
            include_stack.insert(canonical.clone());

            // Process the inserted blocks for nested includes
            let mut sub_doc = DocumentAst {
                path: resolved.clone(),
                ast: quarto_pandoc_types::pandoc::Pandoc {
                    meta: quarto_pandoc_types::config_value::ConfigValue::default(),
                    blocks: doc.ast.blocks.split_off(i),
                },
                ast_context: doc.ast_context.clone(),
                source_context: doc.source_context.clone(),
                warnings: vec![],
                recorded_includes: Vec::new(),
            };
            // Only process the newly inserted blocks
            let remaining = sub_doc.ast.blocks.split_off(num_inserted);
            expand_includes_in_blocks(&mut sub_doc, ctx, &resolved, include_stack)?;

            // Merge back: expanded blocks + remaining
            let mut all_blocks = doc.ast.blocks.clone(); // blocks before i
            all_blocks.extend(sub_doc.ast.blocks);
            all_blocks.extend(remaining);
            doc.ast.blocks = all_blocks;

            // Merge back any context changes from recursion
            doc.ast_context = sub_doc.ast_context;
            doc.source_context = sub_doc.source_context;

            // Merge back transitively-recorded includes, dedup-by-path.
            for entry in sub_doc.recorded_includes {
                if !doc.recorded_includes.iter().any(|e| e.path == entry.path) {
                    doc.recorded_includes.push(entry);
                }
            }

            include_stack.remove(&canonical);

            // Don't increment i — the new blocks at position i may themselves
            // have already been expanded, but blocks after the inserted range
            // still need processing. Advance past the inserted blocks.
            i += num_inserted;
        } else {
            i += 1;
        }
    }
    Ok(())
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

/// Check if a block is a paragraph containing only an include shortcode.
/// Returns the include path if so, None otherwise.
///
/// Public so other crates that need the same "what does this page
/// include?" answer (Phase D.6's `/api/preview/deps` endpoint) share
/// the *exact* recognition rules `IncludeExpansionStage` uses — no
/// drift between "what the renderer treats as an include" and "what
/// the preview dep filter considers a dependency."
pub fn extract_include_path(block: &Block) -> Option<String> {
    let Block::Paragraph(para) = block else {
        return None;
    };

    // Must contain exactly one inline, and it must be a shortcode
    if para.content.len() != 1 {
        return None;
    }

    let Inline::Shortcode(shortcode) = &para.content[0] else {
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
