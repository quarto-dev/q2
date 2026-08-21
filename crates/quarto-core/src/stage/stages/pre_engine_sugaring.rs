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

use async_trait::async_trait;
use quarto_error_reporting::DiagnosticMessage;
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

        // Desugar code-block crossref shorthand into the canonical Div
        // scaffold so engine execution sees plain code blocks. The walk
        // uses the finalized registry so user-declared categories work.
        codeblock_shorthand::desugar_blocks(
            &mut doc.ast.blocks,
            &registry,
            &doc.ast_context.source_context,
            &mut ctx.diagnostics,
        );

        ctx.ref_type_registry = Some(registry);
        // Only seed the index if no prior stage has set one. Idempotent so
        // re-running the stage in tests is harmless.
        if ctx.crossref_index.is_none() {
            ctx.crossref_index = Some(index);
        }

        Ok(PipelineData::DocumentAst(doc))
    }
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
                "/tmp/test",
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
        }
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
