/*
 * engine/ts_engine.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * TsEngine — the Rust struct implementing `ExecutionEngine` by delegating to
 * a shared Deno subprocess via `TsEngineHost`.
 *
 * Gate: `#[cfg(not(target_arch = "wasm32"))]` — same as knitr/jupyter.
 * The host (`TsEngineHost`) and transport are already native-only; TsEngine
 * consumes them so it must carry the same gate.
 *
 * # Two-step lazy lifecycle
 *
 * `ensure_loaded` → `ensure_launched`. Discovery methods only need the
 * first step; instance methods (execute, intermediate_files,
 * markdown_for_file) need both.
 *
 * # Race-free init
 *
 * - `discovery` (`OnceLock`) — benign double-issue; Plan 1b's harness is
 *   idempotent for repeat `LoadEngine`.
 * - `instance` (`Mutex<Option<…>>`) — exclusive init under the lock;
 *   `LaunchEngine` issued exactly once.
 */

#![cfg(not(target_arch = "wasm32"))]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use pampa::lua::HtmlDependency;
use quarto_error_reporting::DiagnosticMessage;
use quarto_source_map::{By, SourceInfo};
use quarto_system_runtime::SystemRuntime;

use crate::engine::LanguageClaim;
use crate::engine::context::{ExecuteResult, ExecutionContext};
use crate::engine::error::ExecutionError;
use crate::engine::traits::ExecutionEngine;
use crate::engine::ts_process::{TsEngineHost, is_available as deno_is_available};
use crate::engine::ts_protocol::{
    EngineProjectContext, FromEngine, LaunchEngineResult, LoadEngineResult, ToEngine,
    TsExecuteOptions, TsExecuteResult, TsFormatIdentifier, TsFormatInfo, TsHtmlDependency,
    TsLanguageClaim,
};
use crate::extension::types::ExtensionId;
use crate::stage::PandocIncludes;
use crate::stage::cancellation::Cancellation;

// ============================================================================
// Wire → resolution conversion
// ============================================================================

/// Convert a protocol `Option<TsLanguageClaim>` to the resolution-layer
/// `LanguageClaim`.
///
/// `None` on the wire means "no claim" → `LanguageClaim::None`.
/// The three struct variants map directly to their enum counterparts.
/// This is the **only seam** where the near-identically-named protocol DTO
/// and the resolution enum meet; the protocol type stays confined to this file.
impl From<Option<TsLanguageClaim>> for LanguageClaim {
    fn from(wire: Option<TsLanguageClaim>) -> Self {
        match wire {
            None => LanguageClaim::None,
            Some(TsLanguageClaim::Primary { priority }) => LanguageClaim::Primary(priority),
            Some(TsLanguageClaim::Interop { priority }) => LanguageClaim::Interop(priority),
            Some(TsLanguageClaim::Fallback { priority }) => LanguageClaim::Fallback(priority),
        }
    }
}

// ============================================================================
// TsEngine struct
// ============================================================================

/// A Quarto execution engine backed by a Deno subprocess via `TsEngineHost`.
///
/// # Lifecycle
///
/// Two-step lazy init:
/// 1. `ensure_loaded` — loads the JS module (cheap; no daemon).
/// 2. `ensure_launched` — creates an engine instance (also cheap; the
///    actual Julia/Jupyter/Python kernel starts lazily on first `execute`).
///
/// # Thread Safety
///
/// `Send + Sync` is satisfied at the type level:
/// - `Arc<TsEngineHost>` — `Send + Sync`.
/// - `OnceLock<LoadEngineResult>` — `Send + Sync`.
/// - `Mutex<Option<LaunchEngineResult>>` — `Send + Sync`.
/// - `Mutex<HashMap<…>>` caches — `Send + Sync`.
/// Required by the `Arc<dyn ExecutionEngine>` registry contract.
pub struct TsEngine {
    /// Registry key / display name.
    name: String,

    /// Whether `name` was declared up-front in `_extension.yml`.
    ///
    /// When `true`, the first `LoadEngine` validates
    /// `LoadEngineResult.name == self.name` and errors on mismatch.
    /// When `false`, the first `LoadEngine` inserts the runtime name into the
    /// registry's alias map.
    name_declared: bool,

    /// Path to the engine's JS/TS entry point.
    engine_path: PathBuf,

    /// Shared subprocess — one `TsEngineHost` per registry instance.
    host: Arc<TsEngineHost>,

    // ── Two-step init state ──────────────────────────────────────────────────
    /// Discovery result (immutable once set — OnceLock).
    discovery: OnceLock<LoadEngineResult>,

    /// Instance result — `Mutex<Option>` to allow poisoning.
    instance: Mutex<Option<LaunchEngineResult>>,

    /// Per-render project context. Set by `set_project` before `ensure_launched`.
    /// Consumed by `ensure_launched` via `unwrap_or_default()`.
    /// TODO(plan1c): add production call site + render-boundary reset.
    project: Mutex<Option<EngineProjectContext>>,

    // ── Caches ───────────────────────────────────────────────────────────────
    claims_language_cache: Mutex<HashMap<(String, Option<String>), LanguageClaim>>,
    claims_file_cache: Mutex<HashMap<PathBuf, bool>>,

    // ── Static hints from `_extension.yml` ───────────────────────────────────
    /// Language hints (pre-filter for `claims_language`).
    language_hints: Option<Vec<String>>,

    /// File-extension hints (pre-filter for `claims_file` / `valid_extensions`).
    file_extension_hints: Option<Vec<String>>,

    // ── Registry sinks (leaf-Arc sharing; no cycle back to EngineRegistry) ───
    extension_id: ExtensionId,
    aliases: Arc<Mutex<HashMap<String, ExtensionId>>>,
    diagnostics: Arc<Mutex<Vec<DiagnosticMessage>>>,
}

impl TsEngine {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        name: impl Into<String>,
        name_declared: bool,
        engine_path: PathBuf,
        host: Arc<TsEngineHost>,
        language_hints: Option<Vec<String>>,
        file_extension_hints: Option<Vec<String>>,
        extension_id: ExtensionId,
        aliases: Arc<Mutex<HashMap<String, ExtensionId>>>,
        diagnostics: Arc<Mutex<Vec<DiagnosticMessage>>>,
    ) -> Self {
        Self {
            name: name.into(),
            name_declared,
            engine_path,
            host,
            discovery: OnceLock::new(),
            instance: Mutex::new(None),
            project: Mutex::new(None),
            claims_language_cache: Mutex::new(HashMap::new()),
            claims_file_cache: Mutex::new(HashMap::new()),
            language_hints,
            file_extension_hints,
            extension_id,
            aliases,
            diagnostics,
        }
    }

    // ========================================================================
    // Two-step lazy lifecycle helpers
    // ========================================================================

    fn ensure_loaded(&self, c: &Cancellation) -> Result<&LoadEngineResult, ExecutionError> {
        self.host.ensure_started()?;

        if self.discovery.get().is_none() {
            let result = self.host.load_engine(&self.engine_path, c)?;

            if self.name_declared {
                if result.name != self.name {
                    return Err(ExecutionError::other(format!(
                        "Engine extension declares 'name: {}' in _extension.yml but the loaded \
                         module reports 'name: {}'. Update _extension.yml or the engine module's \
                         name property.",
                        self.name, result.name
                    )));
                }
            } else {
                try_insert_alias(&self.aliases, &result.name, &self.extension_id)?;
            }

            // Hint validation: file_extension_hints must superset valid_extensions.
            if let Some(hints) = &self.file_extension_hints
                && !hints.is_empty()
            {
                let missing: Vec<&String> = result
                    .valid_extensions
                    .iter()
                    .filter(|ext| !hints.contains(ext))
                    .collect();
                if !missing.is_empty() {
                    let msg = format!(
                        "Engine '{}' declares file_extension_hints {:?} but LoadEngine \
                         reports additional extensions {:?}. The pre-filter will silently \
                         miss those extensions. Declare them in _extension.yml or remove \
                         file_extension_hints.",
                        self.name, hints, missing
                    );
                    let mut diag = self.diagnostics.lock().unwrap();
                    // Idempotent: only push once per engine.
                    let already_pushed = diag.iter().any(|d| format!("{d:?}").contains(&self.name));
                    if !already_pushed {
                        diag.push(DiagnosticMessage::warning(msg));
                    }
                }
            }

            // Best-effort set; second racer fails silently.
            let _ = self.discovery.set(result);
        }

        Ok(self.discovery.get().unwrap())
    }

    /// Set the per-render project context for the next `ensure_launched` call.
    /// Production call site + render-boundary reset owned by plan1c.
    pub fn set_project(&self, project: EngineProjectContext) {
        *self.project.lock().unwrap() = Some(project);
    }

    fn ensure_launched(&self, c: &Cancellation) -> Result<LaunchEngineResult, ExecutionError> {
        self.ensure_loaded(c)?;

        let mut guard = self.instance.lock().unwrap();
        if let Some(ref result) = *guard {
            return Ok(result.clone());
        }
        let project = self.project.lock().unwrap().clone().unwrap_or_default();
        let result = self.host.launch_engine(&self.name, project, c)?;
        *guard = Some(result.clone());
        Ok(result)
    }

    fn poison_instance(&self) {
        let mut guard = self.instance.lock().unwrap();
        guard.take();
    }

    // ========================================================================
    // Protocol → q2-native translation helpers
    // ========================================================================

    fn build_execute_options(input: &str, ctx: &ExecutionContext) -> TsExecuteOptions {
        use crate::engine::ts_protocol::TsSourceMapEntry;

        let identifier = TsFormatIdentifier {
            base_format: ctx.format.clone(),
            target_format: ctx.format.clone(),
            display_name: ctx.format.clone(),
            extension_name: None,
        };

        let format_info = TsFormatInfo {
            identifier,
            metadata: HashMap::new(),
        };

        let source_map: Vec<TsSourceMapEntry> = Vec::new();

        let lib_dir = ctx.source_path.file_stem().map_or_else(
            || "lib".to_string(),
            |s| {
                ctx.source_path
                    .parent()
                    .unwrap_or(Path::new("."))
                    .join(format!("{}_files", s.to_string_lossy()))
                    .to_string_lossy()
                    .into_owned()
            },
        );

        TsExecuteOptions {
            input: input.to_string(),
            source_path: ctx.source_path.to_string_lossy().into_owned(),
            format: format_info,
            temp_dir: ctx.temp_dir.to_string_lossy().into_owned(),
            cwd: ctx.cwd.to_string_lossy().into_owned(),
            project_dir: ctx
                .project_dir
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
            lib_dir,
            quiet: ctx.quiet,
            handled_languages: ctx.handled_languages.clone(),
            params: None,
            source_map,
            // v1/Julia always resolves dependencies inline; deferred consumer
            // (render orchestrator) is RTQ FC-2 and not yet wired.
            dependencies: true,
        }
    }

    fn translate_html_dep(dep: TsHtmlDependency) -> HtmlDependency {
        HtmlDependency {
            name: dep.name,
            stylesheets: dep.stylesheets.into_iter().map(PathBuf::from).collect(),
            scripts: dep.scripts.into_iter().map(PathBuf::from).collect(),
        }
    }

    fn translate_includes(
        ts: Option<crate::engine::ts_protocol::TsPandocIncludes>,
    ) -> PandocIncludes {
        match ts {
            None => PandocIncludes::default(),
            Some(inc) => PandocIncludes {
                header_includes: inc.in_header.unwrap_or_default(),
                include_before: inc.before_body.unwrap_or_default(),
                include_after: inc.after_body.unwrap_or_default(),
            },
        }
    }

    /// Map a wire `TsExecuteResult` to the internal `ExecuteResult`.
    ///
    /// Extracted for direct unit-testability (F1 test seam).
    fn map_execute_result(result: TsExecuteResult) -> ExecuteResult {
        let html_dependencies = result
            .html_dependencies
            .into_iter()
            .map(Self::translate_html_dep)
            .collect();
        let includes = Self::translate_includes(result.includes);
        let supporting_files = result.supporting.into_iter().map(PathBuf::from).collect();

        ExecuteResult {
            markdown: result.markdown,
            supporting_files,
            filters: result.filters,
            includes,
            needs_postprocess: result.post_process,
            html_dependencies,
            metadata: result.metadata,
            pandoc: result.pandoc,
            resource_files: result.resource_files,
            preserve: result.preserve,
        }
    }
}

// ============================================================================
// Identity-aware alias insertion
// ============================================================================

/// Try to insert `runtime_name → extension_id` into the alias map.
///
/// Identity-aware: a re-insert of the **same** `ExtensionId` is an idempotent
/// no-op. A collision between **different** `ExtensionId`s is a hard error.
pub(crate) fn try_insert_alias(
    aliases: &Arc<Mutex<HashMap<String, ExtensionId>>>,
    runtime_name: &str,
    extension_id: &ExtensionId,
) -> Result<(), ExecutionError> {
    let mut guard = aliases.lock().unwrap();
    match guard.get(runtime_name) {
        Some(existing) if existing == extension_id => Ok(()),
        Some(existing) => Err(ExecutionError::other(format!(
            "Engine name collision: runtime name '{runtime_name}' is claimed by both \
             extension '{existing}' and '{extension_id}'. \
             Each engine must have a unique name.",
        ))),
        None => {
            guard.insert(runtime_name.to_string(), extension_id.clone());
            Ok(())
        }
    }
}

// ============================================================================
// ExecutionEngine impl
// ============================================================================

impl ExecutionEngine for TsEngine {
    fn name(&self) -> &str {
        &self.name
    }

    fn is_available(&self) -> bool {
        deno_is_available()
    }

    fn can_freeze(&self) -> bool {
        let guard = self.instance.lock().unwrap();
        guard.as_ref().is_some_and(|i| i.can_freeze)
    }

    fn valid_extensions(&self) -> Vec<String> {
        if let Some(hints) = &self.file_extension_hints {
            return hints.clone();
        }
        let c = Cancellation::new();
        match self.ensure_loaded(&c) {
            Ok(discovery) => discovery.valid_extensions.clone(),
            Err(_) => Vec::new(),
        }
    }

    fn claims_language(&self, language: &str, first_class: Option<&str>) -> LanguageClaim {
        if let Some(hints) = &self.language_hints {
            if hints.is_empty() {
                return LanguageClaim::None;
            }
            if !hints.contains(&language.to_string()) {
                return LanguageClaim::None;
            }
        }

        let cache_key = (language.to_string(), first_class.map(str::to_string));
        {
            let guard = self.claims_language_cache.lock().unwrap();
            if let Some(&cached) = guard.get(&cache_key) {
                return cached;
            }
        }

        let c = Cancellation::new();
        let result = (|| -> Result<LanguageClaim, ExecutionError> {
            self.ensure_loaded(&c)?;
            let msg = ToEngine::ClaimsLanguage {
                engine: self.name.clone(),
                language: language.to_string(),
                first_class: first_class.map(str::to_string),
            };
            let response = self.host.request(msg, Some(Duration::from_secs(10)), &c)?;
            match response {
                FromEngine::ClaimsLanguageResult { result } => Ok(LanguageClaim::from(result)),
                other => Err(ExecutionError::other(format!(
                    "unexpected response to ClaimsLanguage: {other:?}"
                ))),
            }
        })();

        match result {
            Ok(claim) => {
                self.claims_language_cache
                    .lock()
                    .unwrap()
                    .insert(cache_key, claim);
                claim
            }
            Err(_) => LanguageClaim::None,
        }
    }

    fn claims_file(&self, file: &str, ext: &str) -> bool {
        if let Some(hints) = &self.file_extension_hints {
            if hints.is_empty() {
                return false;
            }
            if !hints.contains(&ext.to_string()) {
                return false;
            }
        }

        let path_key = PathBuf::from(file);
        {
            let guard = self.claims_file_cache.lock().unwrap();
            if let Some(&cached) = guard.get(&path_key) {
                return cached;
            }
        }

        let c = Cancellation::new();
        let result = (|| -> Result<bool, ExecutionError> {
            self.ensure_loaded(&c)?;
            let msg = ToEngine::ClaimsFile {
                engine: self.name.clone(),
                file: file.to_string(),
                ext: ext.to_string(),
            };
            let response = self.host.request(msg, Some(Duration::from_secs(10)), &c)?;
            match response {
                FromEngine::ClaimsFileResult { result } => Ok(result),
                other => Err(ExecutionError::other(format!(
                    "unexpected response to ClaimsFile: {other:?}"
                ))),
            }
        })();

        match result {
            Ok(claim) => {
                self.claims_file_cache
                    .lock()
                    .unwrap()
                    .insert(path_key, claim);
                claim
            }
            Err(_) => false,
        }
    }

    fn execute(
        &self,
        input: &str,
        ctx: &ExecutionContext,
    ) -> Result<ExecuteResult, ExecutionError> {
        self.ensure_launched(&ctx.cancellation)?;

        let options = Self::build_execute_options(input, ctx);
        let msg = ToEngine::Execute {
            engine: self.name.clone(),
            options,
        };

        let response = self
            .host
            .request(msg, ctx.execute_timeout, &ctx.cancellation)
            .inspect_err(|e| {
                if matches!(
                    e,
                    ExecutionError::Cancelled | ExecutionError::Timeout { .. }
                ) {
                    self.poison_instance();
                }
            })?;

        match response {
            FromEngine::ExecuteResult { result } => Ok(Self::map_execute_result(result)),
            other => Err(ExecutionError::other(format!(
                "unexpected response to Execute: {other:?}"
            ))),
        }
    }

    fn intermediate_files(&self, input_path: &Path) -> Vec<PathBuf> {
        let c = Cancellation::new();
        let result = (|| -> Result<Vec<PathBuf>, ExecutionError> {
            self.ensure_launched(&c)?;
            let msg = ToEngine::IntermediateFiles {
                engine: self.name.clone(),
                input: input_path.to_string_lossy().into_owned(),
            };
            let response = self.host.request(msg, Some(Duration::from_secs(10)), &c)?;
            match response {
                FromEngine::IntermediateFilesResult { result } => Ok(result
                    .unwrap_or_default()
                    .into_iter()
                    .map(PathBuf::from)
                    .collect()),
                other => Err(ExecutionError::other(format!(
                    "unexpected response to IntermediateFiles: {other:?}"
                ))),
            }
        })();

        match result {
            Ok(files) => files,
            Err(e) => {
                let mut diag = self.diagnostics.lock().unwrap();
                diag.push(DiagnosticMessage::warning(format!(
                    "Engine '{}' failed to enumerate intermediate files for '{}': {e}",
                    self.name,
                    input_path.display()
                )));
                Vec::new()
            }
        }
    }

    fn markdown_for_file(
        &self,
        file: &Path,
        _runtime: &Arc<dyn SystemRuntime>,
    ) -> Result<(String, SourceInfo), ExecutionError> {
        let c = Cancellation::new();
        self.ensure_launched(&c)?;
        let msg = ToEngine::MarkdownForFile {
            engine: self.name.clone(),
            file: file.to_string_lossy().into_owned(),
        };
        let response = self.host.request(msg, Some(Duration::from_secs(30)), &c)?;
        match response {
            FromEngine::MarkdownForFileResult { result } => {
                Ok((result.value, SourceInfo::generated(By::unknown())))
            }
            other => Err(ExecutionError::other(format!(
                "unexpected response to MarkdownForFile: {other:?}"
            ))),
        }
    }
}

// ============================================================================
// Debug impl
// ============================================================================

impl std::fmt::Debug for TsEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TsEngine")
            .field("name", &self.name)
            .field("name_declared", &self.name_declared)
            .field("engine_path", &self.engine_path)
            .field(
                "discovery",
                if self.discovery.get().is_some() {
                    &"loaded"
                } else {
                    &"pending"
                },
            )
            .field(
                "instance",
                if self.instance.lock().unwrap().is_some() {
                    &"launched"
                } else {
                    &"pending"
                },
            )
            .finish()
    }
}

// ============================================================================
// Tests (native-only, MockTransport-backed — no Deno required)
// ============================================================================

#[cfg(test)]
mod tests {
    use std::sync::Barrier;
    use std::time::Duration;

    use super::*;
    use crate::engine::ts_process::{MockTransport, MockWriteHalf, TsEngineHost};
    use crate::engine::ts_protocol::{
        FromEngine, HostGlobalConfig, LaunchEngineResult, LoadEngineResult, ToEngine,
        TsExecuteResult,
    };

    // ── Test helpers ─────────────────────────────────────────────────────────

    fn make_host_global_config() -> HostGlobalConfig {
        HostGlobalConfig {
            resource_dir: "/res".to_string(),
            runtime_dir: "/rt".to_string(),
            data_dir: "/data".to_string(),
            pandoc_path: None,
            is_interactive_session: false,
            running_in_ci: false,
            quarto_version: "0.1.0".to_string(),
        }
    }

    fn loaded_response(name: &str, exts: Vec<&str>) -> FromEngine {
        FromEngine::Loaded {
            discovery: LoadEngineResult {
                name: name.to_string(),
                valid_extensions: exts.into_iter().map(str::to_string).collect(),
                generates_figures: false,
                can_freeze: false,
                quarto_required: None,
            },
        }
    }

    fn launched_response() -> FromEngine {
        FromEngine::Launched {
            instance: LaunchEngineResult { can_freeze: false },
        }
    }

    fn claims_none_response() -> FromEngine {
        FromEngine::ClaimsLanguageResult { result: None }
    }

    fn claims_primary_response(priority: i32) -> FromEngine {
        FromEngine::ClaimsLanguageResult {
            result: Some(TsLanguageClaim::Primary { priority }),
        }
    }

    fn make_engine_with_mock(
        name: &str,
        language_hints: Option<Vec<String>>,
        file_extension_hints: Option<Vec<String>>,
    ) -> (TsEngine, Arc<MockWriteHalf>) {
        let (write, read, mock) = MockTransport::pair_with_handle();
        let ctx = make_host_global_config();
        let host = Arc::new(TsEngineHost::with_transport(write, read, ctx));
        let aliases = Arc::new(Mutex::new(HashMap::new()));
        let diag = Arc::new(Mutex::new(Vec::new()));
        let ext_id = ExtensionId::new(name);
        let engine = TsEngine::new(
            name,
            false,
            PathBuf::from(format!("/engines/{name}.ts")),
            host,
            language_hints,
            file_extension_hints,
            ext_id,
            aliases,
            diag,
        );
        (engine, mock)
    }

    fn watchdog<F, R>(timeout: Duration, f: F) -> R
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
        rx.recv_timeout(timeout)
            .expect("DEADLOCK DETECTED: test timed out")
    }

    // ── MockTransport round-trip smoke ────────────────────────────────────────

    #[test]
    fn test_mock_transport_round_trip_smoke() {
        watchdog(Duration::from_secs(10), || {
            let (write, read, mock) = MockTransport::pair_with_handle();
            let ctx = make_host_global_config();
            let host = Arc::new(TsEngineHost::with_transport(write, read, ctx));

            mock.script_response(0, loaded_response("julia", vec!["jl"]));
            mock.script_response(1, claims_primary_response(1));

            let c = Cancellation::new();
            let load_result = host.load_engine(Path::new("/engine.ts"), &c).unwrap();
            assert_eq!(load_result.name, "julia");

            let claims_response = host
                .request(
                    ToEngine::ClaimsLanguage {
                        engine: "julia".to_string(),
                        language: "julia".to_string(),
                        first_class: None,
                    },
                    Some(Duration::from_secs(5)),
                    &c,
                )
                .unwrap();

            assert!(
                matches!(claims_response, FromEngine::ClaimsLanguageResult { .. }),
                "expected ClaimsLanguageResult, got: {claims_response:?}"
            );

            let sent = mock.sent_messages();
            assert!(
                sent.iter()
                    .any(|m| matches!(m, ToEngine::LoadEngine { .. }))
            );
            assert!(
                sent.iter()
                    .any(|m| matches!(m, ToEngine::ClaimsLanguage { .. }))
            );

            mock.signal_eof();
        });
    }

    // ── Row 5: Two-step lifecycle ─────────────────────────────────────────────
    //
    // Named revert: if `claims_language` called `ensure_launched` instead of
    // `ensure_loaded`, after a discovery-only sequence `sent_messages()` would
    // contain ≥1 `LaunchEngine` → assertion RED.

    #[test]
    fn test_two_step_lifecycle_no_launch_on_discovery() {
        watchdog(Duration::from_secs(10), || {
            let (engine, mock) = make_engine_with_mock("julia", None, None);

            mock.script_response(0, loaded_response("julia", vec!["jl"]));
            mock.script_response(1, claims_none_response());

            let claim = engine.claims_language("python", None);
            assert_eq!(claim, LanguageClaim::None);

            let sent = mock.sent_messages();
            let launch_count = sent
                .iter()
                .filter(|m| matches!(m, ToEngine::LaunchEngine { .. }))
                .count();
            assert_eq!(
                launch_count, 0,
                "discovery-only path must not issue LaunchEngine; sent: {sent:?}"
            );

            let load_count = sent
                .iter()
                .filter(|m| matches!(m, ToEngine::LoadEngine { .. }))
                .count();
            assert!(
                load_count >= 1,
                "claims_language must have issued LoadEngine; sent: {sent:?}"
            );

            mock.signal_eof();
        });
    }

    // ── Row 6: Race-free instance (exclusive) ─────────────────────────────────
    //
    // Named revert: replace the `Mutex<Option>` init with naive get/launch/set
    // → `LaunchEngine` count > 1 → assertion RED.

    #[test]
    fn test_race_free_instance_exclusive() {
        watchdog(Duration::from_secs(15), || {
            let (write, read, mock) = MockTransport::pair_with_handle();
            let ctx = make_host_global_config();
            let host = Arc::new(TsEngineHost::with_transport(write, read, ctx));
            mock.enable_auto_echo();

            let aliases = Arc::new(Mutex::new(HashMap::new()));
            let diag = Arc::new(Mutex::new(Vec::new()));
            let ext_id = ExtensionId::new("julia");
            let engine = Arc::new(TsEngine::new(
                "julia",
                false,
                PathBuf::from("/engines/julia.ts"),
                Arc::clone(&host),
                None,
                None,
                ext_id,
                aliases,
                diag,
            ));

            let barrier = Arc::new(Barrier::new(2));
            let mock_clone = Arc::clone(&mock);

            // Watcher: delivers response when it sees a LaunchEngine message.
            let launch_watcher = std::thread::spawn(move || {
                let mut delivered_ids = std::collections::HashSet::new();
                for _ in 0..300 {
                    std::thread::sleep(Duration::from_millis(10));
                    let sent = mock_clone.sent_messages();
                    for (i, msg) in sent.iter().enumerate() {
                        if matches!(msg, ToEngine::LaunchEngine { .. })
                            && !delivered_ids.contains(&i)
                        {
                            delivered_ids.insert(i);
                            mock_clone.deliver_late(i as u64, launched_response());
                        }
                    }
                    if !delivered_ids.is_empty() {
                        break;
                    }
                }
                delivered_ids.len()
            });

            let engine1 = Arc::clone(&engine);
            let engine2 = Arc::clone(&engine);
            let barrier1 = Arc::clone(&barrier);
            let barrier2 = Arc::clone(&barrier);

            let h1 = std::thread::spawn(move || {
                barrier1.wait();
                let c = Cancellation::new();
                engine1.ensure_launched(&c).is_ok()
            });
            let h2 = std::thread::spawn(move || {
                barrier2.wait();
                let c = Cancellation::new();
                engine2.ensure_launched(&c).is_ok()
            });

            let r1 = h1.join().unwrap();
            let r2 = h2.join().unwrap();
            let _ = launch_watcher.join();

            assert!(r1 || r2, "at least one ensure_launched must succeed");

            let sent = mock.sent_messages();
            let launch_count = sent
                .iter()
                .filter(|m| matches!(m, ToEngine::LaunchEngine { .. }))
                .count();
            assert_eq!(
                launch_count, 1,
                "exactly 1 LaunchEngine expected; sent: {sent:?}"
            );

            mock.signal_eof();
        });
    }

    // ── Row 7: Race-free discovery (convergence) ──────────────────────────────
    //
    // Named revert: replace OnceLock with always-reloading per-call → threads
    // may observe DIFFERENT LoadEngineResults → convergence assertion RED.

    #[test]
    fn test_race_free_discovery_convergence() {
        watchdog(Duration::from_secs(15), || {
            let (write, read, mock) = MockTransport::pair_with_handle();
            let ctx = make_host_global_config();
            let host = Arc::new(TsEngineHost::with_transport(write, read, ctx));

            let aliases = Arc::new(Mutex::new(HashMap::new()));
            let diag = Arc::new(Mutex::new(Vec::new()));
            let ext_id = ExtensionId::new("julia");
            let engine = Arc::new(TsEngine::new(
                "julia",
                false,
                PathBuf::from("/engines/julia.ts"),
                Arc::clone(&host),
                None,
                None,
                ext_id,
                aliases,
                diag,
            ));

            let mock_clone = Arc::clone(&mock);
            let deliver_thread = std::thread::spawn(move || {
                let mut delivered = 0usize;
                let responses = [
                    loaded_response("julia-v1", vec!["jl"]),
                    loaded_response("julia-v2", vec!["jl", "julia"]),
                ];
                for _ in 0..300 {
                    std::thread::sleep(Duration::from_millis(5));
                    let sent = mock_clone.sent_messages();
                    let load_count = sent
                        .iter()
                        .filter(|m| matches!(m, ToEngine::LoadEngine { .. }))
                        .count();
                    while delivered < load_count && delivered < responses.len() {
                        mock_clone.deliver_late(delivered as u64, responses[delivered].clone());
                        delivered += 1;
                    }
                    if delivered >= 1 {
                        break;
                    }
                }
                delivered
            });

            let barrier = Arc::new(Barrier::new(2));
            let e1 = Arc::clone(&engine);
            let e2 = Arc::clone(&engine);
            let b1 = Arc::clone(&barrier);
            let b2 = Arc::clone(&barrier);

            let h1 = std::thread::spawn(move || {
                b1.wait();
                let c = Cancellation::new();
                e1.ensure_loaded(&c).ok().map(|r| r.name.clone())
            });
            let h2 = std::thread::spawn(move || {
                b2.wait();
                let c = Cancellation::new();
                e2.ensure_loaded(&c).ok().map(|r| r.name.clone())
            });

            let name1 = h1.join().unwrap();
            let name2 = h2.join().unwrap();
            let _ = deliver_thread.join();

            // BINDING: both threads observe the SAME cached value.
            assert_eq!(
                name1, name2,
                "OnceLock convergence failed: threads observed different discovery results"
            );
            assert!(name1.is_some(), "thread 1 failed to load");

            let sent = mock.sent_messages();
            let load_count = sent
                .iter()
                .filter(|m| matches!(m, ToEngine::LoadEngine { .. }))
                .count();
            assert!(
                (1..=2).contains(&load_count),
                "LoadEngine count must be 1 or 2; got {load_count}"
            );

            mock.signal_eof();
        });
    }

    // ── Row 8a: Poison policy — Timeout poisons ───────────────────────────────

    #[test]
    fn test_poison_policy_timeout_poisons() {
        watchdog(Duration::from_secs(15), || {
            let (write, read, mock) = MockTransport::pair_with_handle();
            let ctx = make_host_global_config();
            let host = Arc::new(TsEngineHost::with_transport(write, read, ctx));
            let aliases = Arc::new(Mutex::new(HashMap::new()));
            let diag = Arc::new(Mutex::new(Vec::new()));
            let ext_id = ExtensionId::new("julia");
            let engine = TsEngine::new(
                "julia",
                false,
                PathBuf::from("/engines/julia.ts"),
                Arc::clone(&host),
                None,
                None,
                ext_id,
                aliases,
                diag,
            );

            mock.enable_auto_echo();
            // id=1: LaunchEngine (id=0 is the auto-echo LoadEngine).
            mock.script_response(1, launched_response());
            // id=2: Execute is withheld → timeout fires.

            let exec_ctx = ExecutionContext::new(
                PathBuf::from("/tmp"),
                PathBuf::from("/proj"),
                PathBuf::from("/proj/doc.qmd"),
                "html",
            )
            .with_execute_timeout(Some(Duration::from_millis(150)));

            let result = engine.execute("# test", &exec_ctx);
            assert!(
                matches!(result, Err(ExecutionError::Timeout { .. })),
                "expected Timeout from execute, got: {result:?}"
            );

            // Instance must be None (poisoned).
            assert!(
                engine.instance.lock().unwrap().is_none(),
                "instance must be None after Timeout"
            );

            // A subsequent execute must issue a new LaunchEngine.
            let prev_launch_count = mock
                .sent_messages()
                .iter()
                .filter(|m| matches!(m, ToEngine::LaunchEngine { .. }))
                .count();

            let mock2 = Arc::clone(&mock);
            let deliver_relaunch = std::thread::spawn(move || {
                for _ in 0..200 {
                    std::thread::sleep(Duration::from_millis(10));
                    let sent = mock2.sent_messages();
                    let curr = sent
                        .iter()
                        .filter(|m| matches!(m, ToEngine::LaunchEngine { .. }))
                        .count();
                    if curr > prev_launch_count {
                        let idx = sent.len() - 1;
                        mock2.deliver_late(idx as u64, launched_response());
                        mock2.deliver_late(
                            idx as u64 + 1,
                            FromEngine::ExecuteResult {
                                result: crate::engine::ts_protocol::TsExecuteResult {
                                    markdown: "# done".to_string(),
                                    ..Default::default()
                                },
                            },
                        );
                        return curr;
                    }
                }
                prev_launch_count
            });

            let exec_ctx2 = ExecutionContext::new(
                PathBuf::from("/tmp"),
                PathBuf::from("/proj"),
                PathBuf::from("/proj/doc.qmd"),
                "html",
            )
            .with_execute_timeout(Some(Duration::from_secs(5)));
            let _result2 = engine.execute("# test2", &exec_ctx2);
            let _ = deliver_relaunch.join();

            let sent = mock.sent_messages();
            let total_launches = sent
                .iter()
                .filter(|m| matches!(m, ToEngine::LaunchEngine { .. }))
                .count();
            assert!(
                total_launches > prev_launch_count,
                "after Timeout poison, a new LaunchEngine must be issued; \
                 before={prev_launch_count}, after={total_launches}"
            );

            mock.signal_eof();
        });
    }

    // ── Row 8b: Poison policy — ExecutionFailed does NOT poison ───────────────

    #[test]
    fn test_poison_policy_execution_failed_does_not_poison() {
        watchdog(Duration::from_secs(10), || {
            let (write, read, mock) = MockTransport::pair_with_handle();
            let ctx = make_host_global_config();
            let host = Arc::new(TsEngineHost::with_transport(write, read, ctx));
            let aliases = Arc::new(Mutex::new(HashMap::new()));
            let diag = Arc::new(Mutex::new(Vec::new()));
            let ext_id = ExtensionId::new("julia2");
            let engine = TsEngine::new(
                "julia2",
                false,
                PathBuf::from("/engines/julia2.ts"),
                Arc::clone(&host),
                None,
                None,
                ext_id,
                aliases,
                diag,
            );

            mock.enable_auto_echo();
            mock.script_response(1, launched_response());
            mock.script_response(
                2,
                FromEngine::Error {
                    message: "kernel died".to_string(),
                    stack: None,
                },
            );

            let exec_ctx = ExecutionContext::new(
                PathBuf::from("/tmp"),
                PathBuf::from("/proj"),
                PathBuf::from("/proj/doc.qmd"),
                "html",
            )
            .with_execute_timeout(Some(Duration::from_secs(5)));

            let result = engine.execute("# test", &exec_ctx);
            assert!(
                matches!(result, Err(ExecutionError::ExecutionFailed { .. })),
                "expected ExecutionFailed, got: {result:?}"
            );

            // Instance must still be Some (not poisoned).
            assert!(
                engine.instance.lock().unwrap().is_some(),
                "instance must stay Some after ExecutionFailed"
            );

            mock.signal_eof();
        });
    }

    // ── Row 9: Registry name-collision (identity-aware) ───────────────────────

    #[test]
    fn test_alias_insert_identity_aware() {
        let aliases: Arc<Mutex<HashMap<String, ExtensionId>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let id_a = ExtensionId::new("ext-a");
        let id_b = ExtensionId::new("ext-b");

        assert!(
            try_insert_alias(&aliases, "foo", &id_a).is_ok(),
            "first insert must succeed"
        );
        assert!(
            try_insert_alias(&aliases, "foo", &id_a).is_ok(),
            "same-id re-insert must be Ok (idempotent)"
        );
        assert!(
            try_insert_alias(&aliases, "foo", &id_b).is_err(),
            "different-id collision must be Err"
        );

        let guard = aliases.lock().unwrap();
        assert_eq!(guard.get("foo"), Some(&id_a));
    }

    // ── Row 10: Hint-validation warning ──────────────────────────────────────

    #[test]
    fn test_hint_validation_warning_on_missing_ext() {
        watchdog(Duration::from_secs(10), || {
            let (write, read, mock) = MockTransport::pair_with_handle();
            let ctx = make_host_global_config();
            let host = Arc::new(TsEngineHost::with_transport(write, read, ctx));

            let aliases = Arc::new(Mutex::new(HashMap::new()));
            let diag: Arc<Mutex<Vec<DiagnosticMessage>>> = Arc::new(Mutex::new(Vec::new()));
            let ext_id = ExtensionId::new("julia");

            let engine = TsEngine::new(
                "julia",
                false,
                PathBuf::from("/engines/julia.ts"),
                Arc::clone(&host),
                None,
                Some(vec!["jl".to_string()]), // hints declare only "jl"
                ext_id,
                Arc::clone(&aliases),
                Arc::clone(&diag),
            );

            // LoadEngine reports both "jl" and "julia".
            mock.script_response(
                0,
                FromEngine::Loaded {
                    discovery: LoadEngineResult {
                        name: "julia".to_string(),
                        valid_extensions: vec!["jl".to_string(), "julia".to_string()],
                        generates_figures: false,
                        can_freeze: false,
                        quarto_required: None,
                    },
                },
            );

            let c = Cancellation::new();
            let _ = engine.ensure_loaded(&c);

            let diag_guard = diag.lock().unwrap();
            assert!(
                !diag_guard.is_empty(),
                "diagnostics must be non-empty after hint mismatch"
            );

            let warning_text = format!("{:?}", &*diag_guard);
            assert!(
                warning_text.contains("julia"),
                "warning must name the engine; got: {warning_text}"
            );

            mock.signal_eof();
        });
    }

    // ── Row 11: Hint pre-filter (no load) ────────────────────────────────────

    #[test]
    fn test_hint_prefilter_no_load() {
        watchdog(Duration::from_secs(10), || {
            let (engine, mock) =
                make_engine_with_mock("julia", Some(vec!["python".to_string()]), None);

            let claim = engine.claims_language("ruby", None);
            assert_eq!(
                claim,
                LanguageClaim::None,
                "pre-filter must return None for 'ruby'"
            );

            let sent = mock.sent_messages();
            let load_count = sent
                .iter()
                .filter(|m| matches!(m, ToEngine::LoadEngine { .. }))
                .count();
            assert_eq!(
                load_count, 0,
                "hint pre-filter must not issue LoadEngine; sent: {sent:?}"
            );

            mock.signal_eof();
        });
    }

    // ── Row 12: Claims cache (success cached) ─────────────────────────────────

    #[test]
    fn test_claims_language_cache_hit() {
        watchdog(Duration::from_secs(10), || {
            let (write, read, mock) = MockTransport::pair_with_handle();
            let ctx = make_host_global_config();
            let host = Arc::new(TsEngineHost::with_transport(write, read, ctx));

            let aliases = Arc::new(Mutex::new(HashMap::new()));
            let diag = Arc::new(Mutex::new(Vec::new()));
            let ext_id = ExtensionId::new("julia");
            let engine = TsEngine::new(
                "julia",
                false,
                PathBuf::from("/engines/julia.ts"),
                Arc::clone(&host),
                None,
                None,
                ext_id,
                aliases,
                diag,
            );

            mock.script_response(0, loaded_response("julia", vec!["jl"]));
            mock.script_response(1, claims_primary_response(1));

            let claim1 = engine.claims_language("julia", None);
            let claim2 = engine.claims_language("julia", None);

            assert_eq!(claim1, claim2);
            assert!(matches!(claim1, LanguageClaim::Primary(1)));

            let sent = mock.sent_messages();
            let claims_count = sent
                .iter()
                .filter(|m| matches!(m, ToEngine::ClaimsLanguage { .. }))
                .count();
            assert_eq!(
                claims_count, 1,
                "two calls with same key must produce exactly 1 wire query; sent: {sent:?}"
            );

            mock.signal_eof();
        });
    }

    // ── From<Option<TsLanguageClaim>> coverage ────────────────────────────────

    #[test]
    fn test_from_option_ts_language_claim_conversion() {
        assert_eq!(LanguageClaim::from(None), LanguageClaim::None);
        assert_eq!(
            LanguageClaim::from(Some(TsLanguageClaim::Primary { priority: 1 })),
            LanguageClaim::Primary(1)
        );
        assert_eq!(
            LanguageClaim::from(Some(TsLanguageClaim::Interop { priority: 0 })),
            LanguageClaim::Interop(0)
        );
        assert_eq!(
            LanguageClaim::from(Some(TsLanguageClaim::Fallback { priority: -1 })),
            LanguageClaim::Fallback(-1)
        );
    }

    // ── FC-1: map_execute_result wires post_process → needs_postprocess ───────
    //
    // Named revert (a): re-hardcode `needs_postprocess: false` in
    // `map_execute_result` → this assertion REDS (behavioral bind).

    #[test]
    fn test_map_execute_result_wires_post_process() {
        let ts_result = TsExecuteResult {
            post_process: true,
            ..Default::default()
        };
        let result = TsEngine::map_execute_result(ts_result);
        assert!(
            result.needs_postprocess,
            "post_process: true must wire-feed needs_postprocess: true via map_execute_result"
        );
    }
}
