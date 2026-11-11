/*
 * engine/context.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Execution context and result types for engines.
 */

//! Execution context and result types for engines.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use pampa::lua::HtmlDependency;
use quarto_pandoc_types::ConfigValue;
use quarto_source_map::{By, SourceContext, SourceInfo};

use crate::engine::ts_protocol::TsMetadataValue;
use crate::engine::{DEFAULT_EXECUTE_TIMEOUT, HANDLED_LANGUAGES};
use crate::stage::PandocIncludes;
use crate::stage::cancellation::Cancellation;

/// One crossref-consumed cell option, keyed by the cell's wrapper Div id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellOption {
    /// The `id` of the wrapper Div the crossref pass produced for this cell.
    pub div_id: String,
    /// The `label:` value the pass consumed from the cell's option lines.
    pub label: String,
}

/// The per-cell options the crossref pass consumed, in document order.
///
/// The execution stage records the canonical serialized document, in which
/// these options have been lifted onto wrapper Divs. Engines that need them
/// back in the cell text (knitr's label-based output naming) re-add them from
/// this map at the engine boundary; engines that do not need them ignore it.
/// An empty map means nothing was consumed (or the pass never ran), and
/// re-insertion is then a no-op.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CellOptionsMap {
    entries: Vec<CellOption>,
}

impl CellOptionsMap {
    /// Build a map from entries already in document order.
    pub fn new(entries: Vec<CellOption>) -> Self {
        Self { entries }
    }

    /// The entries, in document order.
    pub fn entries(&self) -> &[CellOption] {
        &self.entries
    }

    /// True when no option was consumed.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Context provided to execution engines.
///
/// This contains all the information an engine needs to execute code cells
/// in a document, including paths, configuration, and engine-specific options.
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    /// Temporary directory for engine use.
    ///
    /// Engines can create intermediate files here. The directory is
    /// cleaned up after rendering completes.
    pub temp_dir: PathBuf,

    /// Working directory for execution.
    ///
    /// This is typically the directory containing the source document,
    /// so relative paths in code cells resolve correctly.
    pub cwd: PathBuf,

    /// Project directory, if rendering within a Quarto project.
    ///
    /// `None` for single-file renders (no `_quarto.yml`).
    pub project_dir: Option<PathBuf>,

    /// Path to the source document being rendered.
    pub source_path: PathBuf,

    /// Target output format identifier (e.g., "html", "pdf").
    pub format: String,

    /// Whether to run quietly (suppress engine output).
    pub quiet: bool,

    /// Engine-specific configuration from document metadata.
    ///
    /// This is a clone of the ConfigValue found under the engine key.
    /// For example, for `engine: { jupyter: { kernel: python3 } }`,
    /// this would contain the `{ kernel: python3 }` map.
    pub engine_config: Option<ConfigValue>,

    /// The document's merged `execute:` scope, if it has one.
    ///
    /// This is the document-level default every cell option resolves
    /// against: cell `#|` options merge *over* this map, cell wins
    /// (Q1's `shouldInclude` in `src/core/jupyter/tags.ts`). It comes
    /// from the fully merged metadata — `MetadataMergeStage` runs
    /// before engine execution — so project `_quarto.yml` defaults and
    /// profile overlays are already folded in.
    ///
    /// Engines that need it: jupyter resolves `echo`/`output`/
    /// `warning`/`include`/`error` against it in Rust; knitr forwards
    /// it to R as `format$execute`, where `execute.R` builds
    /// `opts_chunk` from it (bd-nn2fou8h).
    pub execute_scope: Option<ConfigValue>,

    /// Source provenance for the input text.
    ///
    /// Maps byte offsets in the engine's input `&str` back to original source
    /// files (possibly through include expansion boundaries).
    ///
    /// Use `source_info.map_offset(byte_offset, &source_context)` to resolve
    /// a position in the engine's input text to the original file, line, and
    /// column.
    pub source_info: SourceInfo,

    /// Source context for resolving `FileId`s in `source_info`.
    ///
    /// Contains file paths and content needed by `map_offset()` to convert
    /// byte offsets to file/line/column locations. Shared via `Arc` because
    /// the context is finalized after include expansion and doesn't change
    /// during engine execution.
    pub source_context: Arc<SourceContext>,

    /// Languages that this engine must leave unchanged in its output.
    ///
    /// The set is `HANDLED_LANGUAGES ∪ { lang : ownership[lang] != this_engine }`:
    /// cell-handler languages (ojs/mermaid/dot) plus any language owned by a
    /// different engine in the sequence. Populated by `EngineExecutionStage`
    /// via `EngineResolution::handled_languages_for`; defaults to `HANDLED_LANGUAGES`
    /// so any code that builds an `ExecutionContext` directly (tests, knitr) gets the
    /// same leave-alone set as before this field existed.
    pub handled_languages: Vec<String>,

    /// Languages present in this render that q2 assigned to **this** engine
    /// (the positive complement of `handled_languages`). Populated by
    /// `EngineExecutionStage` via `EngineResolution::owned_languages_for`.
    /// Defaults to empty (direct-construction tests do not set it). Carried to TS
    /// engines on the wire (Plan 4d) and read by the Jupyter engine (JOL-2b).
    pub owned_languages: Vec<String>,

    /// Cell options the crossref pass consumed, keyed by wrapper Div id.
    ///
    /// Defaults to empty. Only knitr consumes it today (EXR-10).
    pub cell_options: CellOptionsMap,

    /// Cancellation token for graceful shutdown.
    ///
    /// Engines that support cancellation (TS engines) poll `cancellation.is_cancelled()`
    /// and abort early when set. Built-in engines (knitr/jupyter) ignore it for now.
    /// Defaults to a fresh, non-cancelled token.
    pub cancellation: Cancellation,

    /// Per-request execution timeout.
    ///
    /// `Some(d)` — abort if the engine doesn't respond within `d`.
    /// `None`    — no timeout (equivalent to `execute: {timeout: false}`).
    ///
    /// Resolved from `execute.timeout` in document metadata (tri-state):
    /// - integer `N` → `Some(Duration::from_secs(N))`
    /// - boolean `false` → `None`
    /// - absent / boolean `true` → `Some(DEFAULT_EXECUTE_TIMEOUT)` (300 s)
    ///
    /// Defaults to `Some(DEFAULT_EXECUTE_TIMEOUT)` so existing call sites
    /// (tests, knitr, jupyter) are unaffected.
    pub execute_timeout: Option<Duration>,

    /// Whether this engine is running in a multi-engine sequence
    /// (`|resolution.sequence| > 1`).
    ///
    /// Used by jupyter's `plan_document` to gate the "owned-but-unrunnable →
    /// loud error" branch (P2-13): in a multi-engine sequence, a cell jupyter
    /// owns but cannot execute (e.g. `{sql}`) raises `NoHandlerForLanguage`;
    /// in a single-engine sequence the cell is passed through unexecuted
    /// (display-only, Q1's `quartoMdToJupyter` behaviour for non-kernel cells).
    ///
    /// Defaults to `false`; set by `EngineExecutionStage` from
    /// `resolution.sequence.len() > 1`.
    pub multi_engine: bool,

    /// Merged document metadata, lowered to the wire's flat
    /// `Record<string, TsMetadataValue>` shape (P1.1b).
    ///
    /// Populated by `EngineExecutionStage` from `doc_ast.ast.meta` via
    /// [`crate::project::document_metadata_to_ts_map`] (the same recursive
    /// `ConfigValue → TsMetadataValue` lowering P1.1 built for the `engines`
    /// subtree, called here over the full merged map). `TsEngine::execute`
    /// carries this into `TsExecuteOptions.format.metadata`, which the Node
    /// host's `metadataAsFormat` partitions into Q1's six-bin `Format`
    /// (`execute`/`render`/`pandoc`/`identifier`/`language`/`metadata`).
    /// knitr reads only its `knitr` key, forwarded to R as
    /// `format$metadata$knitr` (GH #1009); jupyter ignores this field.
    ///
    /// Defaults to empty so existing call sites (tests, knitr, jupyter) are
    /// unaffected.
    pub metadata: HashMap<String, TsMetadataValue>,

    /// Environment pairs from the project's `_environment` files to
    /// pass to engine subprocesses, **already filtered** to keys the
    /// real process environment does not define (see
    /// [`crate::project::environment::env_for_subprocess`]). q2 never
    /// mutates its own environment; executed code sees these values
    /// only because spawn sites apply them with `Command::env`.
    pub project_env: Vec<(String, String)>,

    /// The per-document execute environment Quarto 1 gives code cells
    /// (`QUARTO_PROJECT_ROOT`, `QUARTO_DOCUMENT_PATH`,
    /// `QUARTO_DOCUMENT_FILE`, `QUARTO_EXECUTE_INFO`, `QUARTO_FIG_*`; see
    /// `engine::execute_info`, GH #1131). Unlike [`Self::project_env`]
    /// these values change from one document to the next, so an engine
    /// whose process outlives a document (Jupyter's shared kernels) must
    /// set them per document, not at spawn. Empty when no stage ran.
    pub execute_env: Vec<(String, String)>,

    /// The document's execution units from the one shared scan
    /// (`engine::units::scan_units`), run by `EngineExecutionStage` before any
    /// engine. The list is shared: every engine receives the same one and
    /// relocates it into the text it is given (`units::locate_units`) and
    /// applies its own ownership rule (`units::view_for_engine`). Empty when
    /// no stage ran (an engine driven directly), in which case the engine
    /// scans its own text (`units::units_for_engine`).
    pub units: Vec<crate::engine::units::Unit>,

    /// The author text and its content hash for every unit in `units`,
    /// index-for-index (EXR-4's `AuthorTexts`). The stage builds it from the
    /// pre-sugaring snapshot when a record is wanted; the record builder
    /// hashes these, never the engine-visible text, because crossref sugaring
    /// strips label and caption lines from what the engine sees (epic
    /// decision 36). Empty when no record is wanted or no stage ran.
    pub author_texts: crate::record::AuthorTexts,

    /// Whether the engine should build an execution record as a side product
    /// of this run (EXR-5). Set by `EngineExecutionStage` from
    /// `PipelineObserver::wants_execution_record` or from
    /// `execute: {record: true}` in the merged metadata; a plain render leaves
    /// it `false` and the engine allocates nothing for the record.
    pub want_record: bool,
}

impl ExecutionContext {
    /// Create a new execution context with required fields.
    pub fn new(
        temp_dir: PathBuf,
        cwd: PathBuf,
        source_path: PathBuf,
        format: impl Into<String>,
    ) -> Self {
        Self {
            temp_dir,
            cwd,
            project_dir: None,
            source_path,
            format: format.into(),
            quiet: false,
            engine_config: None,
            execute_scope: None,
            // "no source location known yet" sentinel; `with_source_info`
            // overwrites this with the real qmd serialization range before
            // any consumer reads it.
            source_info: SourceInfo::generated(By::unknown()),
            source_context: Arc::new(SourceContext::new()),
            // Default: leave ojs/mermaid/dot alone. EngineExecutionStage
            // overrides this via `with_handled_languages` using the resolver.
            handled_languages: HANDLED_LANGUAGES.iter().map(|s| s.to_string()).collect(),
            owned_languages: Vec::new(),
            cell_options: CellOptionsMap::default(),
            cancellation: Cancellation::new(),
            execute_timeout: Some(DEFAULT_EXECUTE_TIMEOUT),
            // Default false; EngineExecutionStage sets true when |sequence| > 1.
            multi_engine: false,
            // Default empty; EngineExecutionStage sets this from the merged
            // document metadata (P1.1b).
            metadata: HashMap::new(),
            project_env: Vec::new(),
            execute_env: Vec::new(),
            units: Vec::new(),
            author_texts: crate::record::AuthorTexts::default(),
            want_record: false,
        }
    }

    /// Set the author-text mapping (see [`ExecutionContext::author_texts`]).
    pub fn with_author_texts(mut self, author_texts: crate::record::AuthorTexts) -> Self {
        self.author_texts = author_texts;
        self
    }

    /// Ask the engine for an execution record (see [`ExecutionContext::want_record`]).
    pub fn with_want_record(mut self, want: bool) -> Self {
        self.want_record = want;
        self
    }

    /// The merged document metadata as JSON, the shape the record's document hash
    /// reads (EXR-5; `TsMetadataValue` is untagged JSON, so the lowering is exact).
    /// An engine passes it as `EngineEntryInput::document_metadata`.
    pub fn document_metadata_json(&self) -> std::collections::BTreeMap<String, serde_json::Value> {
        match serde_json::to_value(&self.metadata) {
            Ok(serde_json::Value::Object(map)) => map.into_iter().collect(),
            _ => std::collections::BTreeMap::new(),
        }
    }

    /// Set the shared execution-unit list (see [`ExecutionContext::units`]).
    pub fn with_units(mut self, units: Vec<crate::engine::units::Unit>) -> Self {
        self.units = units;
        self
    }

    /// Set the project environment pairs passed to engine
    /// subprocesses (pre-filtered by
    /// [`crate::project::environment::env_for_subprocess`]).
    pub fn with_project_env(mut self, project_env: Vec<(String, String)>) -> Self {
        self.project_env = project_env;
        self
    }

    /// Set the per-document execute environment (see
    /// [`ExecutionContext::execute_env`]).
    pub fn with_execute_env(mut self, execute_env: Vec<(String, String)>) -> Self {
        self.execute_env = execute_env;
        self
    }

    /// Set the project directory.
    pub fn with_project_dir(mut self, dir: Option<PathBuf>) -> Self {
        self.project_dir = dir;
        self
    }

    /// Set quiet mode.
    pub fn with_quiet(mut self, quiet: bool) -> Self {
        self.quiet = quiet;
        self
    }

    /// Set engine configuration.
    pub fn with_engine_config(mut self, config: Option<ConfigValue>) -> Self {
        self.engine_config = config;
        self
    }

    /// Set source provenance and context for the engine's input text.
    pub fn with_source_info(
        mut self,
        source_info: SourceInfo,
        source_context: Arc<SourceContext>,
    ) -> Self {
        self.source_info = source_info;
        self.source_context = source_context;
        self
    }

    /// Set the leave-alone language set for this engine.
    ///
    /// Computed by `EngineResolution::handled_languages_for(engine_name)`:
    /// `HANDLED_LANGUAGES ∪ { lang : ownership[lang] != this_engine }`.
    pub fn with_handled_languages(mut self, languages: Vec<String>) -> Self {
        self.handled_languages = languages;
        self
    }

    /// Set the positive owned-language set for this engine.
    ///
    /// Computed by `EngineResolution::owned_languages_for(engine_name)`.
    pub fn with_owned_languages(mut self, languages: Vec<String>) -> Self {
        self.owned_languages = languages;
        self
    }

    /// Set the cell options the crossref pass consumed.
    pub fn with_cell_options(mut self, cell_options: CellOptionsMap) -> Self {
        self.cell_options = cell_options;
        self
    }

    /// Set the cancellation token.
    pub fn with_cancellation(mut self, cancellation: Cancellation) -> Self {
        self.cancellation = cancellation;
        self
    }

    /// Set the per-request execution timeout.
    ///
    /// `Some(d)` — abort after `d`; `None` — no timeout.
    pub fn with_execute_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.execute_timeout = timeout;
        self
    }

    /// Set whether this engine runs in a multi-engine sequence.
    ///
    /// `true` iff `|resolution.sequence| > 1`. Used by jupyter's
    /// `plan_document` to gate the "owned-but-unrunnable → loud error"
    /// branch (P2-13).
    pub fn with_multi_engine(mut self, multi: bool) -> Self {
        self.multi_engine = multi;
        self
    }

    /// Set the lowered merged document metadata (P1.1b).
    ///
    /// See the `metadata` field doc for the wire path this feeds.
    pub fn with_metadata(mut self, metadata: HashMap<String, TsMetadataValue>) -> Self {
        self.metadata = metadata;
        self
    }

    /// Set the document's merged `execute:` scope (bd-nn2fou8h).
    pub fn with_execute_scope(mut self, execute_scope: Option<ConfigValue>) -> Self {
        self.execute_scope = execute_scope;
        self
    }
}

/// Result of engine execution.
///
/// Contains the transformed markdown with execution outputs,
/// along with any supporting files and metadata produced.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ExecuteResult {
    /// The transformed markdown content with execution outputs.
    ///
    /// Code cells have been replaced with their outputs (text, images, etc.).
    pub markdown: String,

    /// Supporting files produced during execution.
    ///
    /// These are typically figure images, data files, or other resources
    /// that need to be included in the final output. Each path is treated
    /// as a published resource by the orchestrator (`bd-o8pr`): drained
    /// from `StageContext.resource_report` after engine execution,
    /// resolved against the project root, and copied into the project's
    /// output directory alongside YAML- and Lua-filter-declared
    /// resources. Paths can be absolute or anchored at the document's
    /// parent directory.
    pub supporting_files: Vec<PathBuf>,

    /// Pandoc filters to apply to the document.
    ///
    /// Some engines require specific filters to process their output
    /// (e.g., the "quarto" filter for processing Quarto extensions).
    pub filters: Vec<String>,

    /// Content to inject at specific locations in the document.
    ///
    /// Engines can add CSS, JavaScript, or other content to the
    /// document header, before the body, or after the body.
    pub includes: PandocIncludes,

    /// Whether the output requires post-processing.
    ///
    /// If true, additional processing steps may be needed after
    /// the main rendering is complete.
    pub needs_postprocess: bool,

    /// Structured HTML dependencies registered by the engine.
    ///
    /// Each entry is a `{ name, stylesheets, scripts }` manifest that
    /// `EngineExecutionStage` passes to `store_html_dependencies` after
    /// each engine runs.  This is a **disjoint** channel from
    /// `includes`: `includes` carries pre-rendered HTML/text fragments
    /// from Q1-shaped engines (the harness routes `engine.dependencies()`
    /// results there), while `html_dependencies` carries structured
    /// manifests from engines that use the Q2-native
    /// `quarto.htmlDependency()` API (Plan 1b).
    ///
    /// `#[serde(default)]` allows stored engine-capture fixtures that
    /// pre-date this field to deserialize successfully (missing field
    /// becomes an empty `Vec`).
    #[serde(default)]
    pub html_dependencies: Vec<HtmlDependency>,

    /// Engine-produced document metadata (carried-and-ignored until a consumer lands).
    ///
    /// `#[serde(default)]` allows stored engine-capture fixtures that
    /// pre-date this field to deserialize successfully.
    #[serde(default)]
    pub metadata: Option<HashMap<String, TsMetadataValue>>,

    /// Engine-produced pandoc options (carried-and-ignored until a consumer lands).
    ///
    /// `#[serde(default)]` allows stored engine-capture fixtures that
    /// pre-date this field to deserialize successfully.
    #[serde(default)]
    pub pandoc: Option<HashMap<String, TsMetadataValue>>,

    /// Resource files produced by the engine (carried-and-ignored until a consumer lands).
    ///
    /// `#[serde(default)]` allows stored engine-capture fixtures that
    /// pre-date this field to deserialize successfully.
    #[serde(default)]
    pub resource_files: Vec<String>,

    /// Key→value preserve map from the engine (carried-and-ignored until a consumer lands).
    ///
    /// `#[serde(default)]` allows stored engine-capture fixtures that
    /// pre-date this field to deserialize successfully.
    #[serde(default)]
    pub preserve: HashMap<String, String>,

    /// Warnings the engine raised without failing — e.g. knitr's `Q-18-2`
    /// for an include file it named but Quarto could not read.
    /// `EngineExecutionStage` drains these into the stage's diagnostics so
    /// they are counted, printed, and promotable by `--strict`
    /// (bd-gy2ozix3).
    ///
    /// `#[serde(default)]` keeps stored engine-capture fixtures that
    /// pre-date this field deserializable.
    #[serde(default)]
    pub warnings: Vec<quarto_error_reporting::DiagnosticMessage>,

    /// The engine's execution record for this run (EXR-5), built only when
    /// `ExecutionContext::want_record` is set; `None` otherwise. Skipped by
    /// serde on purpose: this struct is serialized whole into the legacy
    /// `EngineCapture` payload, which must not grow (the trace carries the
    /// record through its own `engine_capture.record` field instead).
    #[serde(skip)]
    pub record: Option<crate::record::EngineRecord>,

    /// KNR-6: the skeleton a whole-document engine built the document around, with the nonce
    /// its markers carry. `markdown` holds the final presented text; replay presents the
    /// record over `skeleton.text` again. `None` for an engine with no skeleton (Jupyter) and
    /// for a trace that predates the field, which replays its markdown verbatim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skeleton: Option<Skeleton>,
}

/// A skeleton document: text whose `<!-- q2:<nonce>:<unit-id> -->` markers stand where the
/// units belong (see `engine::knitr::skeleton`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Skeleton {
    pub text: String,
    pub nonce: String,
    /// How many units rendered from shipped markdown instead of the presenter, by reason; the
    /// trace carries it as KNR-8's evidence. Never shown to users.
    #[serde(default)]
    pub fallbacks: super::skeleton::FallbackCounts,
}

impl ExecuteResult {
    /// Create a new result with just markdown content.
    pub fn new(markdown: impl Into<String>) -> Self {
        Self {
            markdown: markdown.into(),
            ..Default::default()
        }
    }

    /// Create a passthrough result (input unchanged).
    pub fn passthrough(input: &str) -> Self {
        Self::new(input)
    }

    /// Add supporting files.
    pub fn with_supporting_files(mut self, files: Vec<PathBuf>) -> Self {
        self.supporting_files = files;
        self
    }

    /// Add filters.
    pub fn with_filters(mut self, filters: Vec<String>) -> Self {
        self.filters = filters;
        self
    }

    /// Add includes.
    pub fn with_includes(mut self, includes: PandocIncludes) -> Self {
        self.includes = includes;
        self
    }

    /// Set post-processing flag.
    pub fn with_postprocess(mut self, needs: bool) -> Self {
        self.needs_postprocess = needs;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // === ExecutionContext tests ===

    #[test]
    fn test_execution_context_new() {
        let ctx = ExecutionContext::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/project"),
            PathBuf::from("/project/doc.qmd"),
            "html",
        );

        assert_eq!(ctx.temp_dir, PathBuf::from("/tmp"));
        assert_eq!(ctx.cwd, PathBuf::from("/project"));
        assert_eq!(ctx.source_path, PathBuf::from("/project/doc.qmd"));
        assert_eq!(ctx.format, "html");
        assert!(!ctx.quiet);
        assert!(ctx.project_dir.is_none());
        assert!(ctx.engine_config.is_none());
    }

    #[test]
    fn test_execution_context_with_project_dir() {
        let ctx = ExecutionContext::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/project"),
            PathBuf::from("/project/doc.qmd"),
            "html",
        )
        .with_project_dir(Some(PathBuf::from("/project")));

        assert_eq!(ctx.project_dir, Some(PathBuf::from("/project")));
    }

    #[test]
    fn test_execution_context_with_quiet() {
        let ctx = ExecutionContext::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/project"),
            PathBuf::from("/project/doc.qmd"),
            "html",
        )
        .with_quiet(true);

        assert!(ctx.quiet);
    }

    #[test]
    fn test_execution_context_clone() {
        let ctx = ExecutionContext::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/project"),
            PathBuf::from("/project/doc.qmd"),
            "html",
        );
        let cloned = ctx.clone();

        assert_eq!(ctx.temp_dir, cloned.temp_dir);
        assert_eq!(ctx.format, cloned.format);
    }

    // === ExecuteResult tests ===

    #[test]
    fn test_execute_result_new() {
        let result = ExecuteResult::new("# Hello\n\nWorld");

        assert_eq!(result.markdown, "# Hello\n\nWorld");
        assert!(result.supporting_files.is_empty());
        assert!(result.filters.is_empty());
        assert!(!result.needs_postprocess);
    }

    #[test]
    fn test_execute_result_passthrough() {
        let input = "Some markdown content";
        let result = ExecuteResult::passthrough(input);

        assert_eq!(result.markdown, input);
    }

    #[test]
    fn test_execute_result_with_supporting_files() {
        let result = ExecuteResult::new("content").with_supporting_files(vec![
            PathBuf::from("figure1.png"),
            PathBuf::from("figure2.png"),
        ]);

        assert_eq!(result.supporting_files.len(), 2);
    }

    #[test]
    fn test_execute_result_with_filters() {
        let result = ExecuteResult::new("content").with_filters(vec!["quarto".to_string()]);

        assert_eq!(result.filters, vec!["quarto"]);
    }

    #[test]
    fn test_execute_result_with_postprocess() {
        let result = ExecuteResult::new("content").with_postprocess(true);

        assert!(result.needs_postprocess);
    }

    #[test]
    fn test_execute_result_default() {
        let result = ExecuteResult::default();

        assert!(result.markdown.is_empty());
        assert!(result.supporting_files.is_empty());
        assert!(result.filters.is_empty());
        assert!(!result.needs_postprocess);
        assert!(result.html_dependencies.is_empty());
    }

    // -----------------------------------------------------------------------
    // Row 14 — HtmlDependency serde round-trip through ExecuteResult.
    //
    // An ExecuteResult carrying a non-empty html_dependencies must survive
    // serde_json serialization and deserialization with its deps intact.
    // Vacuity: the input is non-empty and asserted equal post-round-trip
    // (requires PartialEq on HtmlDependency added in pampa::lua::quarto_doc).
    // -----------------------------------------------------------------------
    #[test]
    fn test_execute_result_html_dependencies_serde_round_trip() {
        let dep = HtmlDependency {
            name: "jquery".to_string(),
            version: None,
            stylesheets: vec![PathBuf::from("libs/jquery/jquery.css")],
            scripts: vec![PathBuf::from("libs/jquery/jquery.js")],
        };

        let original = ExecuteResult {
            markdown: "# Hello".to_string(),
            html_dependencies: vec![dep],
            ..Default::default()
        };

        // The input must be non-empty (vacuity guard).
        assert!(
            !original.html_dependencies.is_empty(),
            "vacuity: html_dependencies must be non-empty for this test to be meaningful"
        );

        // Round-trip through serde_json (the EngineCapture path).
        let json = serde_json::to_string(&original).expect("serialize ExecuteResult");
        let restored: ExecuteResult =
            serde_json::from_str(&json).expect("deserialize ExecuteResult");

        // The deps must survive intact.
        assert_eq!(
            restored.html_dependencies, original.html_dependencies,
            "html_dependencies must round-trip through serde_json unchanged"
        );
    }

    #[test]
    fn test_execute_result_builder_chain() {
        let result = ExecuteResult::new("# Title")
            .with_supporting_files(vec![PathBuf::from("img.png")])
            .with_filters(vec!["filter1".to_string()])
            .with_postprocess(true);

        assert_eq!(result.markdown, "# Title");
        assert_eq!(result.supporting_files.len(), 1);
        assert_eq!(result.filters.len(), 1);
        assert!(result.needs_postprocess);
    }
}
