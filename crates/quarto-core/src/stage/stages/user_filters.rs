/*
 * stage/stages/user_filters.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Apply user-specified filters (Lua, JSON, citeproc) to the document.
 */

use async_trait::async_trait;

use crate::filter_resolve::resolve_filters;
use crate::stage::{
    EventLevel, PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext,
};
use crate::trace_event;

/// Pipeline position for user filters.
#[derive(Debug, Clone, Copy)]
enum FilterPosition {
    /// Runs before `AstTransformsStage`
    Pre,
    /// Runs after `AstTransformsStage`
    Post,
}

/// Apply user-specified filters from the `filters` metadata key.
///
/// This stage reads the `filters` key from merged document metadata,
/// resolves filter paths, and applies them via pampa's filter engine.
///
/// Two instances are used in the pipeline:
/// - `UserFiltersStage::pre()` — runs before `AstTransformsStage`
/// - `UserFiltersStage::post()` — runs after `AstTransformsStage`
///
/// The `quarto` sentinel in the filters list controls which filters
/// run at each position. Filters can also use the `at` field to
/// specify an explicit entry point.
///
/// This stage is a no-op when no filters are configured for its position.
pub struct UserFiltersStage {
    position: FilterPosition,
}

impl UserFiltersStage {
    /// Create a stage that runs user filters before AST transforms.
    pub fn pre() -> Self {
        Self {
            position: FilterPosition::Pre,
        }
    }

    /// Create a stage that runs user filters after AST transforms.
    pub fn post() -> Self {
        Self {
            position: FilterPosition::Post,
        }
    }
}

/// Whether `format` renders through a real `pandoc` subprocess with Q1's
/// `main.lua` filter chain (docx, pptx, typst, …), as opposed to Q2's
/// native HTML writer. book-projects P2b: only Pandoc-hybrid targets have
/// a `main.lua` entry-point mechanism for `UserFiltersStage::post()` to
/// redirect `Position::Post` filters into — HTML has no `main.lua` leg at
/// all, so it must keep running them via pampa exactly as before.
fn is_pandoc_hybrid(format: &crate::format::Format) -> bool {
    matches!(
        crate::format::PipelineProfile::from_format(&format.target_format),
        crate::format::PipelineProfile::Pandoc(_)
    )
}

#[async_trait(?Send)]
impl PipelineStage for UserFiltersStage {
    fn name(&self) -> &str {
        match self.position {
            FilterPosition::Pre => "user-filters-pre",
            FilterPosition::Post => "user-filters-post",
        }
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

        // book-projects P2 citeproc deferral: a render that defers
        // citeproc (the book single-file merge runs it once on the
        // merged document instead of per chapter) strips `"citeproc"`
        // from the metadata *before* filter resolution reads it — the
        // stage that owns resolution owns the deferral point.
        if matches!(self.position, FilterPosition::Pre) && ctx.defer_citeproc {
            crate::project::book::strip_citeproc_from_filters(&mut doc.ast.meta);
        }

        // book-projects P6: a non-references book chapter gets
        // `suppress-bibliography: true` written into its metadata before
        // filter resolution reads it, so its own per-chapter citeproc pass
        // renders in-text citations normally but appends no local
        // bibliography div — the project-wide merge owns the one true
        // bibliography, installed later into the references chapter only.
        if matches!(self.position, FilterPosition::Pre) && ctx.suppress_book_bibliography {
            crate::project::book::set_suppress_bibliography(&mut doc.ast.meta);
        }

        // Resolve filters from merged metadata
        let document_dir = ctx
            .document
            .input
            .parent()
            .unwrap_or(std::path::Path::new("."));

        let resolved = resolve_filters(
            &doc.ast.meta,
            document_dir,
            &ctx.extensions,
            ctx.runtime.as_ref(),
        );

        // book-projects P6: record whether filter resolution placed
        // `"citeproc"` into `.post` — computed once, from the `Pre` pass
        // only (both passes resolve independently from the same
        // metadata, so `Pre` seeing it first is enough; `Post`'s own
        // resolution below would just repeat the same answer).
        if matches!(self.position, FilterPosition::Pre) {
            ctx.citeproc_filter_in_post = resolved
                .post
                .contains(&pampa::unified_filter::FilterSpec::Citeproc);
        }

        // book-projects P2b: on a Pandoc-hybrid target, `Position::Post`
        // filters are forwarded into `main.lua`'s own entry-point
        // mechanism by `PandocWriteStage` (see
        // `QuartoFilterEntryPointsContributor`) instead of running here
        // via pampa — pampa's Lua engine never implemented the Q1-ported
        // pure-Lua helpers (`quarto.utils.file_metadata_filter` etc.)
        // those filters may rely on. Running them here too would
        // double-execute them. HTML targets have no `main.lua` leg at
        // all, so they keep running `Position::Post` filters here
        // exactly as before.
        if matches!(self.position, FilterPosition::Post) && is_pandoc_hybrid(&ctx.format) {
            return Ok(PipelineData::DocumentAst(doc));
        }

        let filters: Vec<pampa::unified_filter::FilterSpec> = match self.position {
            FilterPosition::Pre => resolved.pre.clone(),
            FilterPosition::Post => resolved.post.clone(),
        };
        if filters.is_empty() {
            return Ok(PipelineData::DocumentAst(doc));
        }
        let filters = filters.as_slice();

        trace_event!(
            ctx,
            EventLevel::Debug,
            "applying {} user {} filter(s): {}",
            filters.len(),
            match self.position {
                FilterPosition::Pre => "pre",
                FilterPosition::Post => "post",
            },
            filters
                .iter()
                .map(|f| f.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );

        // The canonical Pandoc format Lua filters see as FORMAT. Using
        // `Format::lua_format()` (not `identifier.as_str()`) makes the reveal
        // *preview* pseudo-format `q2-slides` resolve to `revealjs` rather than
        // its HTML output-writer base — so a user filter's `is_format("revealjs")`
        // fires in preview exactly as in native reveal render (bd-5b21rbaq).
        let target_format = ctx.format.lua_format();

        // Build the attribution lookup handle when a sidecar is
        // present. `AttributionGenerateStage` runs before this stage
        // on both pre and post sub-passes, so the sidecar (when
        // attribution is on) is already populated. The handle is
        // cheap — wraps an `Arc<AttributionData>`.
        let attribution: Option<std::sync::Arc<dyn pampa::attribution::AttributionLookup>> =
            ctx.attribution_data.as_ref().map(|data| {
                std::sync::Arc::new(crate::attribution::AttributionLookupHandle::new(
                    data.clone(),
                )) as std::sync::Arc<dyn pampa::attribution::AttributionLookup>
            });

        // bd-oqoozmtr: citeproc's relative `bibliography`/`csl` resolve
        // against the document's own directory — the declaration site for
        // front-matter values. Captured before `doc.ast` is moved below.
        let doc_dir = doc
            .path
            .parent()
            .map_or_else(|| std::path::PathBuf::from("."), |p| p.to_path_buf());

        // The Lua filter future is !Send (mlua::Lua is !Send), but this
        // pipeline stage runs under #[async_trait] which requires Send on native.
        // On WASM (single-threaded, ?Send), we can .await directly.
        // On native, we bridge with block_in_place + a local tokio runtime.
        #[cfg(not(target_arch = "wasm32"))]
        let filter_result = {
            let ast = doc.ast;
            let ast_context = doc.ast_context;
            let runtime = ctx.runtime.clone();
            let attribution = attribution.clone();
            tokio::task::block_in_place(|| {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| PipelineError::stage_error(self.name(), e.to_string()))?;
                rt.block_on(pampa::unified_filter::apply_filters(
                    ast,
                    ast_context,
                    filters,
                    target_format,
                    runtime,
                    attribution,
                    &doc_dir,
                ))
                .map_err(|e| PipelineError::stage_error(self.name(), e.to_string()))
            })
        };
        #[cfg(target_arch = "wasm32")]
        let filter_result = pampa::unified_filter::apply_filters(
            doc.ast,
            doc.ast_context,
            filters,
            target_format,
            ctx.runtime.clone(),
            attribution,
            &doc_dir,
        )
        .await
        .map_err(|e| PipelineError::stage_error(self.name(), e.to_string()));

        let output = filter_result?;

        doc.ast = output.pandoc;
        doc.ast_context = output.context;
        ctx.diagnostics.extend(output.diagnostics);
        // book-projects P6: harvest this chapter's citation manifest
        // alongside the citeproc filter's own unchanged pass. `None` when
        // no filter in this position's list was `Citeproc`, or `Citeproc`
        // ran but resolved no citations.
        if output.citation_manifest.is_some() {
            ctx.citation_manifest = output.citation_manifest;
        }

        // Store HTML dependencies as artifacts and push text includes
        let mut dep_diagnostics = Vec::new();
        crate::dependency::store_html_dependencies(
            output.html_dependencies,
            &mut ctx.artifacts,
            ctx.runtime.as_ref(),
            &mut dep_diagnostics,
        );
        crate::dependency::push_text_includes(output.text_includes, &mut ctx.includes);
        ctx.diagnostics.extend(dep_diagnostics);

        // bd-o8pr Phase 3: route Lua-filter `quarto.doc.add_resource`
        // entries into the per-doc resource report. Tagged with the
        // document source so the orchestrator can resolve relative
        // paths against the doc's parent dir.
        if !output.resources.is_empty() {
            ctx.resource_report
                .add_lua_filter_files(&doc.path, output.resources);
        }

        trace_event!(ctx, EventLevel::Debug, "user filters complete");

        Ok(PipelineData::DocumentAst(doc))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectContext};
    use crate::stage::DocumentAst;
    use quarto_pandoc_types::ConfigValue;
    use quarto_pandoc_types::config_value::ConfigMapEntry;
    use quarto_pandoc_types::pandoc::Pandoc;
    use quarto_source_map::{SourceContext, SourceInfo};
    use quarto_system_runtime::TempDir;
    use std::path::PathBuf;
    use std::sync::Arc;

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

    /// Like [`MockRuntime`], but records every `file_read` path — book-
    /// projects P2b's negative controls need to distinguish "pampa never
    /// touched this filter" from "pampa touched it and happened to
    /// succeed" (a `MockRuntime` returns `Ok(vec![])` for *any* path, so
    /// even a "nonexistent" filter loads as an empty, successful no-op
    /// Lua script — `Result::is_err()` can't tell the two cases apart).
    struct SpyRuntime {
        inner: MockRuntime,
        file_reads: std::sync::Arc<std::sync::Mutex<Vec<PathBuf>>>,
    }

    impl SpyRuntime {
        fn new() -> (Arc<Self>, std::sync::Arc<std::sync::Mutex<Vec<PathBuf>>>) {
            let file_reads = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            (
                Arc::new(Self {
                    inner: MockRuntime,
                    file_reads: file_reads.clone(),
                }),
                file_reads,
            )
        }
    }

    #[async_trait::async_trait]
    impl quarto_system_runtime::SystemRuntime for SpyRuntime {
        fn file_read(
            &self,
            path: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<Vec<u8>> {
            self.file_reads.lock().unwrap().push(path.to_path_buf());
            self.inner.file_read(path)
        }
        fn file_write(
            &self,
            path: &std::path::Path,
            contents: &[u8],
        ) -> quarto_system_runtime::RuntimeResult<()> {
            self.inner.file_write(path, contents)
        }
        fn path_exists(
            &self,
            path: &std::path::Path,
            kind: Option<quarto_system_runtime::PathKind>,
        ) -> quarto_system_runtime::RuntimeResult<bool> {
            self.inner.path_exists(path, kind)
        }
        fn canonicalize(
            &self,
            path: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            self.inner.canonicalize(path)
        }
        fn path_metadata(
            &self,
            path: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::PathMetadata> {
            self.inner.path_metadata(path)
        }
        fn file_copy(
            &self,
            src: &std::path::Path,
            dst: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            self.inner.file_copy(src, dst)
        }
        fn path_rename(
            &self,
            old: &std::path::Path,
            new: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            self.inner.path_rename(old, new)
        }
        fn file_remove(&self, path: &std::path::Path) -> quarto_system_runtime::RuntimeResult<()> {
            self.inner.file_remove(path)
        }
        fn dir_create(
            &self,
            path: &std::path::Path,
            recursive: bool,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            self.inner.dir_create(path, recursive)
        }
        fn dir_remove(
            &self,
            path: &std::path::Path,
            recursive: bool,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            self.inner.dir_remove(path, recursive)
        }
        fn dir_list(
            &self,
            path: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<Vec<PathBuf>> {
            self.inner.dir_list(path)
        }
        fn cwd(&self) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            self.inner.cwd()
        }
        fn temp_dir(&self, template: &str) -> quarto_system_runtime::RuntimeResult<TempDir> {
            self.inner.temp_dir(template)
        }
        fn exec_pipe(
            &self,
            command: &str,
            args: &[&str],
            stdin: &[u8],
        ) -> quarto_system_runtime::RuntimeResult<Vec<u8>> {
            self.inner.exec_pipe(command, args, stdin)
        }
        fn exec_command(
            &self,
            command: &str,
            args: &[&str],
            stdin: Option<&[u8]>,
        ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::CommandOutput> {
            self.inner.exec_command(command, args, stdin)
        }
        fn env_get(&self, name: &str) -> quarto_system_runtime::RuntimeResult<Option<String>> {
            self.inner.env_get(name)
        }
        fn env_all(
            &self,
        ) -> quarto_system_runtime::RuntimeResult<std::collections::HashMap<String, String>>
        {
            self.inner.env_all()
        }
        async fn fetch_url(
            &self,
            url: &str,
        ) -> quarto_system_runtime::RuntimeResult<(Vec<u8>, String)> {
            self.inner.fetch_url(url).await
        }
        fn os_name(&self) -> &'static str {
            self.inner.os_name()
        }
        fn arch(&self) -> &'static str {
            self.inner.arch()
        }
        fn cpu_time(&self) -> quarto_system_runtime::RuntimeResult<u64> {
            self.inner.cpu_time()
        }
        fn xdg_dir(
            &self,
            kind: quarto_system_runtime::XdgDirKind,
            subpath: Option<&std::path::Path>,
        ) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            self.inner.xdg_dir(kind, subpath)
        }
        fn stdout_write(&self, data: &[u8]) -> quarto_system_runtime::RuntimeResult<()> {
            self.inner.stdout_write(data)
        }
        fn stderr_write(&self, data: &[u8]) -> quarto_system_runtime::RuntimeResult<()> {
            self.inner.stderr_write(data)
        }
    }

    fn make_ctx() -> StageContext {
        let runtime = Arc::new(MockRuntime);
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

    fn make_doc_ast(meta: ConfigValue) -> DocumentAst {
        DocumentAst {
            path: PathBuf::from("/project/test.qmd"),
            ast: Pandoc {
                meta,
                blocks: vec![],
            },
            ast_context: pampa::pandoc::ASTContext::default(),
            source_context: SourceContext::new(),
            warnings: vec![],
            recorded_includes: Vec::new(),
        }
    }

    fn cv_str(s: &str) -> ConfigValue {
        ConfigValue::new_string(s, SourceInfo::for_test())
    }

    fn cv_array(items: Vec<ConfigValue>) -> ConfigValue {
        ConfigValue::new_array(items, SourceInfo::for_test())
    }

    fn cv_map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        ConfigValue::new_map(
            entries
                .into_iter()
                .map(|(k, v)| ConfigMapEntry {
                    key: k.to_string(),
                    key_source: SourceInfo::for_test(),
                    value: v,
                })
                .collect(),
            SourceInfo::for_test(),
        )
    }

    #[tokio::test]
    async fn pre_stage_no_filters_is_passthrough() {
        let mut ctx = make_ctx();
        let stage = UserFiltersStage::pre();
        let doc = make_doc_ast(cv_map(vec![]));
        let input = PipelineData::DocumentAst(doc);
        let output = stage.run(input, &mut ctx).await.unwrap();
        assert!(output.into_document_ast().is_some());
    }

    #[tokio::test]
    async fn post_stage_no_filters_is_passthrough() {
        let mut ctx = make_ctx();
        let stage = UserFiltersStage::post();
        let doc = make_doc_ast(cv_map(vec![]));
        let input = PipelineData::DocumentAst(doc);
        let output = stage.run(input, &mut ctx).await.unwrap();
        assert!(output.into_document_ast().is_some());
    }

    #[tokio::test]
    async fn pre_stage_with_filters_key_but_empty_is_passthrough() {
        let mut ctx = make_ctx();
        let stage = UserFiltersStage::pre();
        let meta = cv_map(vec![("filters", cv_array(vec![]))]);
        let doc = make_doc_ast(meta);
        let input = PipelineData::DocumentAst(doc);
        let output = stage.run(input, &mut ctx).await.unwrap();
        assert!(output.into_document_ast().is_some());
    }

    #[tokio::test]
    async fn post_stage_ignores_pre_only_filters() {
        // Filters without sentinel all go to Pre, so Post stage should be a no-op
        let mut ctx = make_ctx();
        let stage = UserFiltersStage::post();
        let meta = cv_map(vec![("filters", cv_array(vec![cv_str("test.lua")]))]);
        let doc = make_doc_ast(meta);
        let input = PipelineData::DocumentAst(doc);
        let output = stage.run(input, &mut ctx).await.unwrap();
        assert!(output.into_document_ast().is_some());
    }

    /// book-projects P2b, negative control: an HTML target has no
    /// `main.lua` leg, so `Position::Post` filters must keep running via
    /// pampa exactly as before the redirect — proven by a `SpyRuntime`
    /// that records `file_read` calls: pampa must actually attempt to
    /// load the filter. (`Result::is_err()` can't distinguish this from
    /// "skipped" because `MockRuntime`/`SpyRuntime` return `Ok(vec![])`
    /// for any path, which pampa's Lua engine happily parses as an empty,
    /// successful no-op filter.)
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn post_stage_html_target_still_runs_filters_via_pampa() {
        let mut ctx = make_ctx();
        assert!(
            !is_pandoc_hybrid(&ctx.format),
            "make_ctx()'s default format must be HTML for this to be a negative control"
        );
        let (spy, file_reads) = SpyRuntime::new();
        ctx.runtime = spy;
        let stage = UserFiltersStage::post();
        // A bare (no-sentinel) filter list defaults to Pre, so put the
        // "quarto" sentinel first — everything after it lands in Post.
        let meta = cv_map(vec![(
            "filters",
            cv_array(vec![cv_str("quarto"), cv_str("/some/dir/post.lua")]),
        )]);
        let doc = make_doc_ast(meta);
        let input = PipelineData::DocumentAst(doc);
        stage
            .run(input, &mut ctx)
            .await
            .expect("SpyRuntime's file_read always succeeds, so the stage itself must not error");
        assert!(
            file_reads
                .lock()
                .unwrap()
                .iter()
                .any(|p| p.ends_with("post.lua")),
            "HTML target must still dispatch Post filters to pampa, which reads the filter file: {:?}",
            file_reads.lock().unwrap()
        );
    }

    /// book-projects P2b: on a Pandoc-hybrid target (docx here), a
    /// `Position::Post` filter must be skipped entirely by
    /// `UserFiltersStage::post()` — pampa must never even read the filter
    /// file, because `PandocWriteStage` forwards it into `main.lua`'s own
    /// entry-point mechanism instead.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn post_stage_pandoc_hybrid_target_skips_pampa_dispatch() {
        let mut ctx = make_ctx();
        ctx.format = crate::format::Format::docx();
        assert!(is_pandoc_hybrid(&ctx.format));
        let (spy, file_reads) = SpyRuntime::new();
        ctx.runtime = spy;
        let stage = UserFiltersStage::post();
        // "quarto" sentinel first so the filter path lands in Post, not
        // the no-sentinel-means-Pre default.
        let meta = cv_map(vec![(
            "filters",
            cv_array(vec![cv_str("quarto"), cv_str("/some/dir/post.lua")]),
        )]);
        let doc = make_doc_ast(meta);
        let input = PipelineData::DocumentAst(doc);
        let output = stage
            .run(input, &mut ctx)
            .await
            .expect("Pandoc-hybrid target must skip pampa dispatch for Post filters entirely");
        assert!(output.into_document_ast().is_some());
        assert!(
            file_reads.lock().unwrap().is_empty(),
            "pampa must never read a Post filter's file for a Pandoc-hybrid target: {:?}",
            file_reads.lock().unwrap()
        );
    }

    /// book-projects P2b, negative control: `Position::Pre` filters are
    /// untouched by the redirect for *any* target, Pandoc-hybrid included
    /// — they keep running via pampa because `AstTransformsStage` (and
    /// thus any pandoc handoff) hasn't happened yet.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pre_stage_runs_via_pampa_regardless_of_pandoc_hybrid_target() {
        let mut ctx = make_ctx();
        ctx.format = crate::format::Format::docx();
        assert!(is_pandoc_hybrid(&ctx.format));
        let (spy, file_reads) = SpyRuntime::new();
        ctx.runtime = spy;
        let stage = UserFiltersStage::pre();
        let meta = cv_map(vec![(
            "filters",
            cv_array(vec![cv_str("/some/dir/pre.lua")]),
        )]);
        let doc = make_doc_ast(meta);
        let input = PipelineData::DocumentAst(doc);
        stage
            .run(input, &mut ctx)
            .await
            .expect("SpyRuntime's file_read always succeeds, so the stage itself must not error");
        assert!(
            file_reads
                .lock()
                .unwrap()
                .iter()
                .any(|p| p.ends_with("pre.lua")),
            "Pre-position filters must still run via pampa even for a \
             Pandoc-hybrid target: {:?}",
            file_reads.lock().unwrap()
        );
    }

    #[test]
    fn stage_names_are_distinct() {
        let pre = UserFiltersStage::pre();
        let post = UserFiltersStage::post();
        assert_eq!(pre.name(), "user-filters-pre");
        assert_eq!(post.name(), "user-filters-post");
        assert_ne!(pre.name(), post.name());
    }

    #[test]
    fn stage_kinds_are_document_ast() {
        let stage = UserFiltersStage::pre();
        assert_eq!(stage.input_kind(), PipelineDataKind::DocumentAst);
        assert_eq!(stage.output_kind(), PipelineDataKind::DocumentAst);
    }

    /// book-projects P2 citeproc deferral: when the caller sets
    /// `ctx.defer_citeproc` (a book single-file merge that runs citeproc
    /// once on the merged document instead), `UserFiltersStage::pre()`
    /// must strip `"citeproc"` from `meta["filters"]` *before* filter
    /// resolution reads it — so the chapter's declared (here deliberately
    /// missing) bibliography is never touched.
    ///
    /// `multi_thread` because the pre-deferral code path under test
    /// reaches `tokio::task::block_in_place`, which panics on a
    /// current-thread runtime.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pre_stage_strips_citeproc_when_deferral_flag_set() {
        let mut ctx = make_ctx();
        ctx.defer_citeproc = true;
        let stage = UserFiltersStage::pre();
        let meta = cv_map(vec![
            ("bibliography", cv_str("/nonexistent/path/refs.json")),
            ("filters", cv_array(vec![cv_str("citeproc")])),
        ]);
        let doc = make_doc_ast(meta);
        let input = PipelineData::DocumentAst(doc);
        let output = stage
            .run(input, &mut ctx)
            .await
            .expect("deferred citeproc must not touch the missing bibliography");
        let out_doc = output.into_document_ast().unwrap();
        let filters = out_doc
            .ast
            .meta
            .get("filters")
            .expect("filters key remains after stripping");
        let quarto_pandoc_types::config_value::ConfigValueKind::Array(items) = &filters.value
        else {
            panic!("filters must stay an array after stripping: {filters:?}")
        };
        assert!(
            items
                .iter()
                .all(|i| i.as_plain_text().is_none_or(|s| s != "citeproc")),
            "citeproc must be stripped from meta filters, got: {filters:?}"
        );
    }

    /// Negative control for the deferral test above: with the flag at
    /// its default (`false`), the same input DOES resolve and run
    /// citeproc — which fails on the deliberately missing bibliography.
    /// The flag, not some incidental pass-through, is what carries the
    /// deferral.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pre_stage_runs_citeproc_when_deferral_flag_unset() {
        let mut ctx = make_ctx();
        let stage = UserFiltersStage::pre();
        let meta = cv_map(vec![
            ("bibliography", cv_str("/nonexistent/path/refs.json")),
            ("filters", cv_array(vec![cv_str("citeproc")])),
        ]);
        let doc = make_doc_ast(meta);
        let input = PipelineData::DocumentAst(doc);
        let result = stage.run(input, &mut ctx).await;
        assert!(
            result.is_err(),
            "without defer_citeproc, citeproc must run and fail on the missing bibliography"
        );
    }

    /// book-projects P6 regression: a non-book (or ordinary) render leaves
    /// `ctx.suppress_book_bibliography` at its default (`false`), and
    /// `UserFiltersStage::pre()` must not inject `suppress-bibliography`
    /// into the document's metadata in that case — only the book
    /// orchestrator setting the flag should trigger the injection from
    /// `crate::project::book::set_suppress_bibliography`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pre_stage_does_not_inject_suppress_bibliography_when_flag_unset() {
        let mut ctx = make_ctx();
        assert!(!ctx.suppress_book_bibliography);
        let stage = UserFiltersStage::pre();
        let meta = cv_map(vec![]);
        let doc = make_doc_ast(meta);
        let input = PipelineData::DocumentAst(doc);
        let output = stage
            .run(input, &mut ctx)
            .await
            .expect("no filters configured, so the stage must not error");
        let out_doc = output.into_document_ast().unwrap();
        assert!(
            out_doc.ast.meta.get("suppress-bibliography").is_none(),
            "suppress-bibliography must not be injected when \
             ctx.suppress_book_bibliography is false"
        );
    }
}
