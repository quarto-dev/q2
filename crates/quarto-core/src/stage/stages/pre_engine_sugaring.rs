/*
 * stage/stages/pre_engine_sugaring.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Pre-engine AST sugaring for crossrefs.
 */

//! Pre-engine crossref sugaring.
//!
//! This stage runs after metadata merge and before engine execution. Its
//! responsibilities, per design plan D2 + D7:
//!
//! 1. Build the [`RefTypeRegistry`] from built-ins plus any
//!    `crossref.custom` entries in merged document metadata, plus any
//!    prefixes implied by the `crossref.ids` manifest.
//! 2. Convert the code-block crossref shorthand into the canonical
//!    `Div(#<ref>-..) > CodeBlock` scaffold, so that engines see only plain
//!    code blocks.
//! 3. Strip consumed cell options from the code block body.
//! 4. Validate that declared `crossref.ids` entries use registered ref-type
//!    prefixes.
//!
//! Phases (1) through (4) are staged:
//!
//! - **Phase 0.1 scaffold:** seed the registry with built-ins and pass the
//!   AST through untouched.
//! - **Phase 1.a (current):** read `crossref.custom` and `crossref.ids`
//!   from merged document metadata, extend the registry, and seed a
//!   [`CrossrefIndex`] carrying the promised ids.
//! - **Phase 1.1:** add code-block shorthand detection/rewriting and
//!   option stripping.
//!
//! Reconciliation across the synthetic Div the future shorthand rewrite
//! introduces is handled by `EngineExecutionStage`, which serializes the
//! whole AST and reconciles the post-engine parse against it — see plan D2.

use crate::engine::cell_lang::engine_cell_lang;
use crate::record::SugaredBlock;
use async_trait::async_trait;
use quarto_error_reporting::DiagnosticMessage;
use quarto_pandoc_types::block::Block;
use quarto_source_map::FileId;

use crate::crossref::{
    CrossrefIndex, MetadataError, RefTypeRegistry, codeblock_shorthand, metadata,
};
use crate::stage::{
    EventLevel, PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext,
};
use crate::trace_event;

/// Pipeline stage that performs pre-engine crossref sugaring.
///
/// Inserted between `MetadataMergeStage` and `EngineExecutionStage`.
///
/// The stage is intentionally lightweight today — it seeds the
/// [`RefTypeRegistry`] and leaves the AST untouched. Shorthand rewriting and
/// manifest handling land in Phase 1 of the crossref plan.
pub struct PreEngineSugaringStage;

impl PreEngineSugaringStage {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PreEngineSugaringStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl PipelineStage for PreEngineSugaringStage {
    fn name(&self) -> &str {
        "pre-engine-sugaring"
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

        // Seed the ref-type registry if a caller hasn't pre-populated it
        // (tests sometimes do).
        let mut registry = ctx
            .ref_type_registry
            .take()
            .unwrap_or_else(RefTypeRegistry::builtin);

        // Localize built-in display names from the resolved term table
        // (bd-llhlzd7p) before user config extends the registry, so
        // `crossref.custom` names win over locale defaults.
        if let Some(terms) = crate::language::LanguageTerms::from_meta(&doc.ast.meta) {
            registry.localize_builtin_display_names(&terms);
        }

        // Extend the registry from `crossref.custom` and lift `crossref.ids`
        // into promised-id entries. Errors are non-fatal and become
        // diagnostics on the stage context.
        let extracted = metadata::read(&doc.ast.meta, &mut registry);
        for err in &extracted.errors {
            ctx.diagnostics.push(metadata_error_to_diagnostic(err));
        }

        // Register prefixes from promised ids that aren't otherwise known,
        // so the resolver can still classify them. Indexer diagnostics will
        // still flag realized ids whose prefix lacks a proper declaration.
        registry.extend_from_promised(&extracted.promised_ids);

        trace_event!(
            ctx,
            EventLevel::Debug,
            "ref-type registry finalized ({} entries), {} promised ids",
            registry.len(),
            extracted.promised_ids.len()
        );

        // Seed a CrossrefIndex so downstream transforms have one handle to
        // thread. The indexer (Phase 1.3) will fill in entries / sections.
        // Use FileId(0) — the render pipeline currently renders one document
        // per context; multi-file namespacing is a Phase 4 concern.
        let mut index = CrossrefIndex::new(FileId(0));
        index.promised_ids = extracted.promised_ids;

        // EXR-5 (epic decision 36): before the pass rewrites anything, record the
        // author's text of every braced executable block, in document order, the
        // untouched ones included. The engine stage pairs the list with its unit
        // scan to hash what the author wrote, since the desugaring below strips
        // the label and caption lines from what the engine sees.
        let author_texts = executable_block_texts(&doc.ast.blocks);

        // Desugar code-block crossref shorthand into the canonical Div
        // scaffold so engine execution sees plain code blocks. The walk
        // uses the finalized registry so user-declared categories work.
        codeblock_shorthand::desugar_blocks(
            &mut doc.ast.blocks,
            &registry,
            &doc.ast_context.source_context,
            &mut ctx.diagnostics,
        );

        let sugared_texts = executable_block_texts(&doc.ast.blocks);
        if author_texts.len() == sugared_texts.len() {
            ctx.sugared_blocks = author_texts
                .into_iter()
                .zip(sugared_texts)
                .map(|(author_text, sugared_text)| SugaredBlock {
                    author_text,
                    sugared_text,
                })
                .collect();
        } else {
            // Cannot happen: the desugaring replaces a block in place. Keep the
            // list empty rather than misalign it; the engine stage then hashes
            // the scanned text.
            trace_event!(
                ctx,
                EventLevel::Warn,
                "pre-engine sugaring: {} executable blocks before desugaring, {} after; \
                 no author-text snapshot",
                author_texts.len(),
                sugared_texts.len()
            );
            ctx.sugared_blocks = Vec::new();
        }

        ctx.ref_type_registry = Some(registry);
        // Only seed the index if no prior stage has set one. Idempotent so
        // re-running the stage in tests is harmless.
        if ctx.crossref_index.is_none() {
            ctx.crossref_index = Some(index);
        }

        Ok(PipelineData::DocumentAst(doc))
    }
}

/// The text of every braced executable code block under `blocks`, in document
/// order, descending into the same containers `desugar_blocks` descends into (so
/// the list before and the list after the pass align block for block).
fn executable_block_texts(blocks: &[Block]) -> Vec<String> {
    fn walk(blocks: &[Block], out: &mut Vec<String>) {
        for block in blocks {
            match block {
                Block::BlockQuote(bq) => walk(&bq.content, out),
                Block::Div(div) => walk(&div.content, out),
                Block::OrderedList(ol) => ol.content.iter().for_each(|item| walk(item, out)),
                Block::BulletList(bl) => bl.content.iter().for_each(|item| walk(item, out)),
                Block::DefinitionList(dl) => {
                    for (_term, defs) in &dl.content {
                        defs.iter().for_each(|def| walk(def, out));
                    }
                }
                Block::CodeBlock(cb) if engine_cell_lang(block).is_some() => {
                    out.push(cb.text.clone());
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(blocks, &mut out);
    out
}

/// Convert a metadata extraction error into a diagnostic message.
///
/// Extracted into a free function so it can be unit-tested separately from
/// the stage and so the stage's `run` remains readable.
fn metadata_error_to_diagnostic(err: &MetadataError) -> DiagnosticMessage {
    // The source info would ideally be passed to the diagnostic for
    // ariadne-style rendering. That plumbing lives on `DiagnosticMessage`
    // already but isn't convenient from this stage without a SourceContext
    // handle — leave it as a plain warning for now and upgrade once the
    // context bridging pattern is settled.
    DiagnosticMessage::warning(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use crate::stage::DocumentAst;
    use pampa::pandoc::ASTContext;
    use quarto_pandoc_types::pandoc::Pandoc;
    use quarto_source_map::SourceContext;
    use std::path::PathBuf;
    use std::sync::Arc;

    struct MockRuntime;

    #[async_trait::async_trait]
    impl quarto_system_runtime::SystemRuntime for MockRuntime {
        fn file_read(&self, _p: &std::path::Path) -> quarto_system_runtime::RuntimeResult<Vec<u8>> {
            Ok(vec![])
        }
        fn file_write(
            &self,
            _p: &std::path::Path,
            _c: &[u8],
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn path_exists(
            &self,
            _p: &std::path::Path,
            _k: Option<quarto_system_runtime::PathKind>,
        ) -> quarto_system_runtime::RuntimeResult<bool> {
            Ok(true)
        }
        fn canonicalize(
            &self,
            p: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            Ok(p.to_path_buf())
        }
        fn path_metadata(
            &self,
            _p: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::PathMetadata> {
            unimplemented!()
        }
        fn file_copy(
            &self,
            _s: &std::path::Path,
            _d: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn path_rename(
            &self,
            _o: &std::path::Path,
            _n: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn file_remove(&self, _p: &std::path::Path) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn dir_create(
            &self,
            _p: &std::path::Path,
            _r: bool,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn dir_remove(
            &self,
            _p: &std::path::Path,
            _r: bool,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn dir_list(
            &self,
            _p: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<Vec<PathBuf>> {
            Ok(vec![])
        }
        fn cwd(&self) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            Ok(PathBuf::from("/"))
        }
        fn temp_dir(
            &self,
            _t: &str,
        ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::TempDir> {
            Ok(quarto_system_runtime::TempDir::new(PathBuf::from(
                "/nonexistent/q2-mock-temp-dir",
            )))
        }
        fn exec_pipe(
            &self,
            _c: &str,
            _a: &[&str],
            _s: &[u8],
        ) -> quarto_system_runtime::RuntimeResult<Vec<u8>> {
            Ok(vec![])
        }
        fn exec_command(
            &self,
            _c: &str,
            _a: &[&str],
            _s: Option<&[u8]>,
        ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::CommandOutput> {
            Ok(quarto_system_runtime::CommandOutput {
                code: 0,
                stdout: vec![],
                stderr: vec![],
            })
        }
        fn env_get(&self, _n: &str) -> quarto_system_runtime::RuntimeResult<Option<String>> {
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
            _u: &str,
        ) -> quarto_system_runtime::RuntimeResult<(Vec<u8>, String)> {
            Err(quarto_system_runtime::RuntimeError::NotSupported(
                "mock".into(),
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
            _k: quarto_system_runtime::XdgDirKind,
            _s: Option<&std::path::Path>,
        ) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            Ok(PathBuf::from("/xdg"))
        }
        fn stdout_write(&self, _d: &[u8]) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn stderr_write(&self, _d: &[u8]) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
    }

    fn make_ctx() -> StageContext {
        let runtime = Arc::new(MockRuntime);
        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![],
            output_dir: PathBuf::from("/project"),

            ..Default::default()
        };
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let format = Format::html();
        StageContext::new(runtime, format, project, doc).unwrap()
    }

    fn make_doc_ast() -> DocumentAst {
        DocumentAst {
            path: PathBuf::from("/project/test.qmd"),
            ast: Pandoc::default(),
            ast_context: ASTContext::default(),
            source_context: SourceContext::new(),
            warnings: vec![],
            recorded_includes: Vec::new(),
            derived_listing: Default::default(),
        }
    }

    /// Parse `qmd` the way the pipeline does, so the snapshot sees real blocks.
    fn parse_doc(qmd: &str) -> DocumentAst {
        let (ast, ast_context, _warnings) = pampa::readers::qmd::read(
            qmd.as_bytes(),
            false,
            "test.qmd",
            &mut std::io::sink(),
            true,
            None,
        )
        .expect("qmd parses");
        DocumentAst {
            path: PathBuf::from("/project/test.qmd"),
            ast,
            source_context: ast_context.source_context.clone(),
            ast_context,
            warnings: vec![],
            recorded_includes: Vec::new(),
            derived_listing: Default::default(),
        }
    }

    async fn snapshot_of(qmd: &str) -> Vec<SugaredBlock> {
        let mut ctx = make_ctx();
        PreEngineSugaringStage::new()
            .run(PipelineData::DocumentAst(parse_doc(qmd)), &mut ctx)
            .await
            .expect("stage runs");
        ctx.sugared_blocks
    }

    /// EXR-5: the snapshot lists every braced executable block, the ones the
    /// pass leaves alone included, so an untouched cell whose text equals a
    /// rewritten cell\'s stripped text cannot take that cell\'s entry.
    #[tokio::test]
    async fn snapshot_lists_untouched_blocks_too() {
        let qmd = "```{python}\nx = 1\n```\n\n```{python}\n#| label: fig-a\n#| fig-cap: A\nx = 1\n```\n\n```{python}\nx = 1\n```\n";
        let snapshot = snapshot_of(qmd).await;
        assert_eq!(snapshot.len(), 3, "{snapshot:?}");
        assert_eq!(snapshot[0].author_text, "x = 1");
        assert_eq!(snapshot[0].sugared_text, "x = 1");
        assert_eq!(
            snapshot[1].author_text,
            "#| label: fig-a\n#| fig-cap: A\nx = 1"
        );
        assert_eq!(snapshot[1].sugared_text, "x = 1");
        assert_eq!(snapshot[2].author_text, "x = 1");
        assert_eq!(snapshot[2].sugared_text, "x = 1");
    }

    /// A labeled cell with a caption line between two kept options keeps the
    /// author\'s raw lines, in order, while the engine text drops the two.
    #[tokio::test]
    async fn snapshot_has_the_authors_raw_lines_for_a_sugared_cell() {
        let qmd = "```{python}\n#| echo: false\n#| fig-cap: \"A cap\"\n#| label: fig-x\n#| fig-width: 7\nplot()\n```\n";
        let snapshot = snapshot_of(qmd).await;
        assert_eq!(snapshot.len(), 1);
        assert_eq!(
            snapshot[0].author_text,
            "#| echo: false\n#| fig-cap: \"A cap\"\n#| label: fig-x\n#| fig-width: 7\nplot()"
        );
        assert!(
            !snapshot[0].sugared_text.contains("fig-cap"),
            "{:?}",
            snapshot[0]
        );
        assert!(
            !snapshot[0].sugared_text.contains("label"),
            "{:?}",
            snapshot[0]
        );
        assert!(
            snapshot[0].sugared_text.contains("#| echo: false"),
            "{:?}",
            snapshot[0]
        );
        assert!(
            snapshot[0].sugared_text.contains("#| fig-width: 7"),
            "{:?}",
            snapshot[0]
        );
    }

    /// A sugared cell inside a container is in the list, in document order.
    #[tokio::test]
    async fn snapshot_includes_cells_inside_containers() {
        let qmd = "```{python}\na\n```\n\n::: {.panel}\n```{python}\n#| label: fig-in\n#| fig-cap: In\nb\n```\n:::\n\n```{python}\nc\n```\n";
        let snapshot = snapshot_of(qmd).await;
        let authors: Vec<&str> = snapshot.iter().map(|b| b.author_text.as_str()).collect();
        assert_eq!(authors, ["a", "#| label: fig-in\n#| fig-cap: In\nb", "c"]);
        assert_eq!(snapshot[1].sugared_text, "b");
    }

    #[tokio::test]
    async fn scaffold_seeds_builtin_registry() {
        let mut ctx = make_ctx();
        assert!(ctx.ref_type_registry.is_none());

        let stage = PreEngineSugaringStage::new();
        let out = stage
            .run(PipelineData::DocumentAst(make_doc_ast()), &mut ctx)
            .await
            .expect("stage runs");
        assert!(matches!(out, PipelineData::DocumentAst(_)));

        let reg = ctx.ref_type_registry.as_ref().expect("registry seeded");
        assert!(reg.contains("fig"));
        assert!(reg.contains("tbl"));
    }

    #[tokio::test]
    async fn scaffold_preserves_existing_registry() {
        let mut ctx = make_ctx();
        let mut pre = RefTypeRegistry::builtin();
        pre.register_custom("dia", "Diagram", None).unwrap();
        ctx.ref_type_registry = Some(pre);

        let stage = PreEngineSugaringStage::new();
        stage
            .run(PipelineData::DocumentAst(make_doc_ast()), &mut ctx)
            .await
            .expect("stage runs");

        let reg = ctx.ref_type_registry.as_ref().unwrap();
        // Custom entry still there, not overwritten by a fresh builtin-only registry.
        assert!(reg.contains("dia"));
    }

    #[tokio::test]
    async fn scaffold_is_ast_passthrough() {
        let mut ctx = make_ctx();
        let stage = PreEngineSugaringStage::new();
        let doc_in = make_doc_ast();
        let ast_before = doc_in.ast.clone();

        let out = stage
            .run(PipelineData::DocumentAst(doc_in), &mut ctx)
            .await
            .unwrap();
        let doc_out = out.into_document_ast().unwrap();
        assert_eq!(doc_out.ast, ast_before);
    }

    #[tokio::test]
    async fn seeds_empty_crossref_index() {
        let mut ctx = make_ctx();
        let stage = PreEngineSugaringStage::new();
        stage
            .run(PipelineData::DocumentAst(make_doc_ast()), &mut ctx)
            .await
            .unwrap();
        let idx = ctx.crossref_index.as_ref().expect("index seeded");
        assert!(idx.entries.is_empty());
        assert!(idx.promised_ids.is_empty());
    }

    fn doc_with_meta(meta: quarto_pandoc_types::ConfigValue) -> DocumentAst {
        let ast = Pandoc {
            meta,
            blocks: vec![],
        };
        DocumentAst {
            path: PathBuf::from("/project/test.qmd"),
            ast,
            ast_context: ASTContext::default(),
            source_context: SourceContext::new(),
            warnings: vec![],
            recorded_includes: Vec::new(),
            derived_listing: Default::default(),
        }
    }

    // Builders mirroring the ones in crossref::metadata::tests. Kept local
    // to the stage test so this test doesn't reach across module
    // boundaries.
    fn scalar(s: &str) -> quarto_pandoc_types::ConfigValue {
        use quarto_pandoc_types::{ConfigValueKind, MergeOp};
        use yaml_rust2::Yaml;
        quarto_pandoc_types::ConfigValue {
            value: ConfigValueKind::scalar(Yaml::String(s.into())),
            source_info: quarto_source_map::SourceInfo::original(FileId(0), 0, 0),
            merge_op: MergeOp::default(),
        }
    }

    fn map(
        entries: Vec<(&str, quarto_pandoc_types::ConfigValue)>,
    ) -> quarto_pandoc_types::ConfigValue {
        use quarto_pandoc_types::{ConfigMapEntry, ConfigValueKind, MergeOp};
        quarto_pandoc_types::ConfigValue {
            value: ConfigValueKind::Map(
                entries
                    .into_iter()
                    .map(|(k, v)| ConfigMapEntry {
                        key: k.into(),
                        key_source: quarto_source_map::SourceInfo::original(FileId(0), 0, 0),
                        value: v,
                    })
                    .collect(),
            ),
            source_info: quarto_source_map::SourceInfo::original(FileId(0), 0, 0),
            merge_op: MergeOp::default(),
        }
    }

    fn array(items: Vec<quarto_pandoc_types::ConfigValue>) -> quarto_pandoc_types::ConfigValue {
        use quarto_pandoc_types::{ConfigValueKind, MergeOp};
        quarto_pandoc_types::ConfigValue {
            value: ConfigValueKind::Array(items),
            source_info: quarto_source_map::SourceInfo::original(FileId(0), 0, 0),
            merge_op: MergeOp::default(),
        }
    }

    #[tokio::test]
    async fn registers_crossref_custom_from_metadata() {
        let meta = map(vec![(
            "crossref",
            map(vec![(
                "custom",
                array(vec![map(vec![
                    ("key", scalar("dia")),
                    ("reference-prefix", scalar("Diagram")),
                ])]),
            )]),
        )]);
        let mut ctx = make_ctx();
        let stage = PreEngineSugaringStage::new();
        stage
            .run(PipelineData::DocumentAst(doc_with_meta(meta)), &mut ctx)
            .await
            .unwrap();

        let reg = ctx.ref_type_registry.as_ref().unwrap();
        let def = reg.classify_cite_id("dia-1").expect("dia registered");
        assert_eq!(def.kind, "Diagram");
        assert!(ctx.diagnostics.is_empty(), "no diagnostics expected");
    }

    #[tokio::test]
    async fn lifts_crossref_ids_into_index() {
        let meta = map(vec![(
            "crossref",
            map(vec![(
                "ids",
                array(vec![scalar("tbl-dynamic"), scalar("fig-generated")]),
            )]),
        )]);
        let mut ctx = make_ctx();
        let stage = PreEngineSugaringStage::new();
        stage
            .run(PipelineData::DocumentAst(doc_with_meta(meta)), &mut ctx)
            .await
            .unwrap();
        let idx = ctx.crossref_index.as_ref().unwrap();
        assert_eq!(idx.promised_ids.len(), 2);
        assert_eq!(idx.promised_ids[0].identifier, "tbl-dynamic");
    }

    #[tokio::test]
    async fn malformed_crossref_custom_produces_diagnostic() {
        let meta = map(vec![(
            "crossref",
            map(vec![(
                "custom",
                array(vec![map(vec![("key", scalar("dia"))])]), // missing reference-prefix
            )]),
        )]);
        let mut ctx = make_ctx();
        let stage = PreEngineSugaringStage::new();
        stage
            .run(PipelineData::DocumentAst(doc_with_meta(meta)), &mut ctx)
            .await
            .unwrap();
        assert_eq!(ctx.diagnostics.len(), 1);
        // And the registry stayed clean.
        let reg = ctx.ref_type_registry.as_ref().unwrap();
        assert!(!reg.contains("dia"));
    }

    #[tokio::test]
    async fn unknown_promised_prefix_registered_as_promised() {
        let meta = map(vec![(
            "crossref",
            map(vec![("ids", array(vec![scalar("mycustom-value")]))]),
        )]);
        let mut ctx = make_ctx();
        let stage = PreEngineSugaringStage::new();
        stage
            .run(PipelineData::DocumentAst(doc_with_meta(meta)), &mut ctx)
            .await
            .unwrap();
        let reg = ctx.ref_type_registry.as_ref().unwrap();
        let def = reg
            .classify_cite_id("mycustom-value")
            .expect("promised prefix lookup");
        assert_eq!(def.source, crate::crossref::RefTypeSource::Promised);
    }
}
