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
use crate::extension::types::{
    ExtensionId, StaticLanguageClaim, lookup_static_claim, static_claim_to_language_claim,
};
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

    // ── Static claims from `_extension.yml` ──────────────────────────────────
    /// Authoritative static language claims (from `_extension.yml`). `Some` → answer
    /// claims_language from this map WITHOUT loading; `None` → legacy dynamic load.
    claims: Option<HashMap<String, StaticLanguageClaim>>,
    /// Complete handled-extension set; the claims_file pre-filter. `Some` → authoritative
    /// for valid_extensions; pre-filters claims_file (ext ∉ set ⇒ false, no load).
    file_extensions: Option<Vec<String>>,
    /// Unconditional static claims_file. `Some` → answer ext∈claims_files without loading;
    /// `None` → content-inspecting engine, dynamic claims_file load.
    claims_files: Option<Vec<String>>,
    /// Records each statically-answered claims_language key for execute-time validation.
    static_answers: Mutex<Vec<(String, Option<String>)>>,
    /// Records each statically-answered claims_file extension for execute-time validation.
    static_file_answers: Mutex<Vec<String>>,
    /// Per-render markdown_for_file conversion cache (canonical path → converted QMD).
    /// Field added here; CONSUMED by Task 10's EngineClaimsFileStage — unused for now.
    #[allow(dead_code)] // Task 10 consumes this
    conversion_cache: Mutex<HashMap<PathBuf, String>>,

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
        claims: Option<HashMap<String, StaticLanguageClaim>>,
        file_extensions: Option<Vec<String>>,
        claims_files: Option<Vec<String>>,
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
            claims,
            file_extensions,
            claims_files,
            static_answers: Mutex::new(Vec::new()),
            static_file_answers: Mutex::new(Vec::new()),
            conversion_cache: Mutex::new(HashMap::new()),
            extension_id,
            aliases,
            diagnostics,
        }
    }

    // ========================================================================
    // Two-step lazy lifecycle helpers
    // ========================================================================

    /// The name the *harness* addresses this engine by on the wire.
    ///
    /// The Deno harness keys every loaded engine by its runtime `discovery.name`
    /// (returned in `LoadEngineResult`). For a statically-declared engine that
    /// equals `self.name` (validated in [`Self::ensure_loaded`]); but for an
    /// **unnamed** engine `self.name` is the extension-id registry key
    /// (e.g. `echo-legacy`) while the harness keyed the module under its runtime
    /// name (e.g. `echolegacy`). So every wire frame sent *after* LoadEngine —
    /// `ClaimsLanguage`, `ClaimsFile`, `LaunchEngine`, `Execute`, … — must address
    /// the engine by this runtime name, not the registry key, or the harness
    /// rejects it with `engine not loaded: <ext-id>`.
    ///
    /// Falls back to `self.name` before the first load completes (no frame that
    /// depends on the runtime name is sent in that window).
    fn wire_name(&self) -> String {
        self.discovery
            .get()
            .map_or_else(|| self.name.clone(), |d| d.name.clone())
    }

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

            // Static-vs-dynamic validation: compare each statically-answered claim to
            // the dynamic result from the now-loaded module. A mismatch is a hard error
            // because the extension author declared something the module contradicts.
            // One-directional: catches over-claiming (declared but wrong); under-declaration
            // is never caught (the engine never loads those paths).
            let static_language_answers: Vec<(String, Option<String>)> =
                self.static_answers.lock().unwrap().clone();

            if !static_language_answers.is_empty()
                && let Some(claims_map) = &self.claims
            {
                for (language, first_class) in &static_language_answers {
                    // Recompute what the static map says (same logic as claims_language static branch).
                    let mut static_claim =
                        lookup_static_claim(claims_map, language, first_class.as_deref());
                    if static_claim == LanguageClaim::None
                        && let Some(fb) = claims_map.get("fallback")
                    {
                        static_claim = static_claim_to_language_claim(fb, first_class.as_deref());
                    }

                    // Send the dynamic wire call. Address the harness by the
                    // runtime name it keyed the just-loaded module under.
                    let msg = ToEngine::ClaimsLanguage {
                        engine: result.name.clone(),
                        language: language.clone(),
                        first_class: first_class.clone(),
                    };
                    let dynamic_claim: LanguageClaim = match self.host.request(
                        msg,
                        Some(Duration::from_secs(10)),
                        c,
                    ) {
                        Ok(FromEngine::ClaimsLanguageResult { result }) => {
                            LanguageClaim::from(result)
                        }
                        Ok(other) => {
                            return Err(ExecutionError::other(format!(
                                "unexpected response to ClaimsLanguage during validation: {other:?}"
                            )));
                        }
                        Err(e) => return Err(e),
                    };

                    if static_claim != dynamic_claim {
                        return Err(ExecutionError::other(format!(
                            "Engine '{}' statically declares claim {:?} for language '{}' but \
                             the loaded module's claimsLanguage reports {:?}. Update \
                             _extension.yml or the engine module.",
                            self.name, static_claim, language, dynamic_claim
                        )));
                    }
                }
            }

            // Validate static claims_file answers similarly.
            let static_file_answer_list: Vec<String> =
                self.static_file_answers.lock().unwrap().clone();

            if !static_file_answer_list.is_empty() {
                for ext in &static_file_answer_list {
                    let static_claimed = self
                        .claims_files
                        .as_ref()
                        .is_some_and(|cf| cf.iter().any(|e| e == ext));

                    // Synthetic filename "x<ext>" — declaring claims_files is an assertion
                    // that the engine claims every file of that extension unconditionally,
                    // regardless of content. The filename itself doesn't matter; the ext does.
                    let msg = ToEngine::ClaimsFile {
                        engine: result.name.clone(),
                        file: format!("x{ext}"),
                        ext: ext.clone(),
                    };
                    let dynamic_claimed: bool =
                        match self.host.request(msg, Some(Duration::from_secs(10)), c) {
                            Ok(FromEngine::ClaimsFileResult { result }) => result,
                            Ok(other) => {
                                return Err(ExecutionError::other(format!(
                                    "unexpected response to ClaimsFile during validation: {other:?}"
                                )));
                            }
                            Err(e) => return Err(e),
                        };

                    if static_claimed != dynamic_claimed {
                        return Err(ExecutionError::other(format!(
                            "Engine '{}' statically declares claims_file {:?} for extension \
                             '{}' but the loaded module's claimsFile reports {:?}. Update \
                             _extension.yml or the engine module.",
                            self.name, static_claimed, ext, dynamic_claimed
                        )));
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
        let result = self.host.launch_engine(&self.wire_name(), project, c)?;
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
        // Authoritative static answer — no load.
        if let Some(exts) = &self.file_extensions {
            return exts.clone();
        }
        // Legacy: load and discover.
        let c = Cancellation::new();
        match self.ensure_loaded(&c) {
            Ok(discovery) => discovery.valid_extensions.clone(),
            Err(_) => Vec::new(),
        }
    }

    fn claims_language(&self, language: &str, first_class: Option<&str>) -> LanguageClaim {
        let cache_key = (language.to_string(), first_class.map(str::to_string));
        {
            let guard = self.claims_language_cache.lock().unwrap();
            if let Some(&cached) = guard.get(&cache_key) {
                return cached;
            }
        }

        if let Some(map) = &self.claims {
            // AUTHORITATIVE static answer — no load.
            let mut claim = lookup_static_claim(map, language, first_class);
            if claim == LanguageClaim::None {
                // Universal fallback: a `fallback` map key claims any otherwise-unclaimed language.
                if let Some(fb) = map.get("fallback") {
                    claim = static_claim_to_language_claim(fb, first_class);
                }
            }
            // Record for execute-time validation — positive answers only.
            // Validation is one-directional (catches over-claiming); recording None answers
            // would turn tolerated under-declaration into a spurious hard render error.
            if claim != LanguageClaim::None {
                self.static_answers
                    .lock()
                    .unwrap()
                    .push((language.to_string(), first_class.map(str::to_string)));
            }
            // Cache and return.
            self.claims_language_cache
                .lock()
                .unwrap()
                .insert(cache_key, claim);
            claim
        } else {
            // Legacy dynamic path — ensure_loaded + ClaimsLanguage wire call + cache.
            let c = Cancellation::new();
            let result = (|| -> Result<LanguageClaim, ExecutionError> {
                self.ensure_loaded(&c)?;
                let msg = ToEngine::ClaimsLanguage {
                    engine: self.wire_name(),
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
    }

    fn claims_file(&self, file: &str, ext: &str) -> bool {
        // 1. Pre-filter: if file_extensions is Some and ext ∉ it ⇒ false, no load.
        if let Some(exts) = &self.file_extensions
            && !exts.iter().any(|e| e == ext)
        {
            return false;
        }

        let path_key = PathBuf::from(file);
        {
            let guard = self.claims_file_cache.lock().unwrap();
            if let Some(&cached) = guard.get(&path_key) {
                return cached;
            }
        }

        // 2. Authoritative static claims_file: if claims_files is Some ⇒ answer ext ∈ claims_files, no load.
        if let Some(cf) = &self.claims_files {
            let claimed = cf.iter().any(|e| e == ext);
            // Record positive answers for execute-time validation, de-duped by ext.
            // Only true answers are recorded: false means unclaimed — no validation needed.
            // De-dup because a render probes many files of the same ext; each ext at most once.
            if claimed {
                let mut file_answers = self.static_file_answers.lock().unwrap();
                if !file_answers.contains(&ext.to_string()) {
                    file_answers.push(ext.to_string());
                }
            }
            self.claims_file_cache
                .lock()
                .unwrap()
                .insert(path_key, claimed);
            return claimed;
        }

        // 3. Dynamic path — cache per canonical path + ClaimsFile wire call.
        let c = Cancellation::new();
        let result = (|| -> Result<bool, ExecutionError> {
            self.ensure_loaded(&c)?;
            let msg = ToEngine::ClaimsFile {
                engine: self.wire_name(),
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
            engine: self.wire_name(),
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
                engine: self.wire_name(),
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
        // P2-17: cache the converted QMD per canonical path so both passes of a
        // two-pass (website) render share one conversion, not two subprocess
        // round-trips.  The key is the file path as supplied by the caller (already
        // normalized to absolute + lexically clean by `EngineClaimsFileStage`).
        {
            let guard = self.conversion_cache.lock().unwrap();
            if let Some(cached) = guard.get(file) {
                return Ok((cached.clone(), SourceInfo::generated(By::unknown())));
            }
        }

        let c = Cancellation::new();
        self.ensure_launched(&c)?;
        let msg = ToEngine::MarkdownForFile {
            engine: self.wire_name(),
            file: file.to_string_lossy().into_owned(),
        };
        let response = self.host.request(msg, Some(Duration::from_secs(30)), &c)?;
        match response {
            FromEngine::MarkdownForFileResult { result } => {
                let qmd = result.value;
                self.conversion_cache
                    .lock()
                    .unwrap()
                    .insert(file.to_path_buf(), qmd.clone());
                Ok((qmd, SourceInfo::generated(By::unknown())))
            }
            other => Err(ExecutionError::other(format!(
                "unexpected response to MarkdownForFile: {other:?}"
            ))),
        }
    }

    fn quarto_required(&self) -> Option<&str> {
        // Read the OnceLock only — do NOT trigger a load.
        self.discovery
            .get()
            .and_then(|d| d.quarto_required.as_deref())
    }

    fn shutdown(&self) -> Result<(), ExecutionError> {
        self.host.shutdown()
    }

    fn is_alive(&self) -> bool {
        self.host.is_alive()
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
    use crate::extension::types::{ClaimKind, StaticLanguageClaim};

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
        claims: Option<HashMap<String, StaticLanguageClaim>>,
        file_extensions: Option<Vec<String>>,
        claims_files: Option<Vec<String>>,
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
            claims,
            file_extensions,
            claims_files,
            ext_id,
            aliases,
            diag,
        );
        (engine, mock)
    }

    /// Build a simple single-language static claims map for tests.
    fn single_claim(
        language: &str,
        kind: ClaimKind,
        priority: Option<i32>,
    ) -> HashMap<String, StaticLanguageClaim> {
        let mut m = HashMap::new();
        m.insert(
            language.to_string(),
            StaticLanguageClaim {
                kind,
                priority,
                when_class: None,
            },
        );
        m
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
            let (engine, mock) = make_engine_with_mock("julia", None, None, None);

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

    // Row 10 previously tested a file_extension_hints superset-check warning that was
    // removed in Task 4. The new authoritative `file_extensions` field has no superset
    // validation — it IS the answer. This test verifies that `valid_extensions()` returns
    // the static list without triggering a load.
    #[test]
    fn test_file_extensions_static_no_load() {
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
                Some(vec!["jl".to_string()]), // file_extensions declared statically
                None,
                ext_id,
                Arc::clone(&aliases),
                Arc::clone(&diag),
            );

            // valid_extensions returns the static list without any load.
            let exts = engine.valid_extensions();
            assert_eq!(
                exts,
                vec!["jl"],
                "static file_extensions must be returned directly"
            );
            assert_eq!(
                host.load_engine_count(),
                0,
                "static valid_extensions must not issue LoadEngine"
            );

            mock.signal_eof();
        });
    }

    // ── Row 11: Static claims — unknown language returns None without load ───────
    // Named revert: replacing the static claims lookup with an unconditional load
    // would make load_count ≥ 1 → assertion RED.

    #[test]
    fn test_hint_prefilter_no_load() {
        watchdog(Duration::from_secs(10), || {
            // Engine claims only "python"; "ruby" is absent → None, no load.
            let (engine, mock) = make_engine_with_mock(
                "julia",
                Some(single_claim("python", ClaimKind::Primary, None)),
                None,
                None,
            );

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

    // ── P1-12: static zero-load vs legacy loaded ──────────────────────────────
    //
    // Named revert: removing the `if let Some(map) = &self.claims` branch and
    // always taking the dynamic path would make the static engine call ensure_loaded,
    // bumping load_engine_count to ≥ 1 → assertion RED.

    #[test]
    fn test_p1_12_static_zero_load() {
        watchdog(Duration::from_secs(10), || {
            let (write, read, mock) = MockTransport::pair_with_handle();
            let ctx = make_host_global_config();
            let host = Arc::new(TsEngineHost::with_transport(write, read, ctx));
            let aliases = Arc::new(Mutex::new(HashMap::new()));
            let diag = Arc::new(Mutex::new(Vec::new()));
            let ext_id = ExtensionId::new("echo");
            let engine = TsEngine::new(
                "echo",
                false,
                PathBuf::from("/engines/echo.ts"),
                Arc::clone(&host),
                Some(single_claim("echo", ClaimKind::Primary, Some(1))),
                None,
                None,
                ext_id,
                aliases,
                diag,
            );

            let claim = engine.claims_language("echo", None);
            assert_eq!(
                claim,
                LanguageClaim::Primary(1),
                "static engine must return Primary(1)"
            );
            assert!(!host.is_alive(), "static answer must not spawn subprocess");
            assert_eq!(
                host.load_engine_count(),
                0,
                "static answer must not issue LoadEngine"
            );

            mock.signal_eof();
        });
    }

    #[test]
    fn test_p1_12_legacy_does_load() {
        // Named revert (exercised guard): if the legacy branch were removed, load_engine_count
        // would stay 0 → the "DID load" assertion RED.
        watchdog(Duration::from_secs(10), || {
            let (engine, mock) = make_engine_with_mock("echo", None, None, None);

            mock.script_response(0, loaded_response("echo", vec![]));
            mock.script_response(1, claims_primary_response(1));

            let claim = engine.claims_language("echo", None);
            assert_eq!(
                claim,
                LanguageClaim::Primary(1),
                "legacy dynamic engine must return Primary(1)"
            );
            // EXERCISED GUARD: the legacy arm DID load (contrast with static arm above).
            assert_eq!(
                engine.host.load_engine_count(),
                1,
                "legacy engine must issue exactly 1 LoadEngine"
            );

            mock.signal_eof();
        });
    }

    // ── P1-14: whenClass at TsEngine level (both directions, NO load) ─────────
    //
    // Named revert: removing the when_class guard in static_claim_to_language_claim
    // would make the bare/other cases wrongly return Primary(1) → assertion RED.

    #[test]
    fn test_p1_14_when_class_at_engine_level() {
        watchdog(Duration::from_secs(10), || {
            let mut claims_map = HashMap::new();
            claims_map.insert(
                "python".to_string(),
                StaticLanguageClaim {
                    kind: ClaimKind::Primary,
                    priority: None,
                    when_class: Some("marimo".to_string()),
                },
            );

            let (write, read, mock) = MockTransport::pair_with_handle();
            let ctx = make_host_global_config();
            let host = Arc::new(TsEngineHost::with_transport(write, read, ctx));
            let aliases = Arc::new(Mutex::new(HashMap::new()));
            let diag = Arc::new(Mutex::new(Vec::new()));
            let ext_id = ExtensionId::new("marimo-engine");
            let engine = TsEngine::new(
                "marimo-engine",
                false,
                PathBuf::from("/engines/marimo.ts"),
                Arc::clone(&host),
                Some(claims_map),
                None,
                None,
                ext_id,
                aliases,
                diag,
            );

            // Matching whenClass → Primary(1), zero-load.
            let claim = engine.claims_language("python", Some("marimo"));
            assert_eq!(
                claim,
                LanguageClaim::Primary(1),
                "matching whenClass must give Primary(1)"
            );
            assert!(!host.is_alive(), "whenClass match must be zero-load");
            assert_eq!(
                host.load_engine_count(),
                0,
                "whenClass match must not issue LoadEngine"
            );

            // No whenClass → None (mismatch).
            let claim2 = engine.claims_language("python", None);
            assert_eq!(claim2, LanguageClaim::None, "no whenClass must give None");

            // Other whenClass → None (mismatch).
            let claim3 = engine.claims_language("python", Some("other"));
            assert_eq!(
                claim3,
                LanguageClaim::None,
                "other whenClass must give None"
            );

            mock.signal_eof();
        });
    }

    // ── claims_file: pre-filter and static (no load) ──────────────────────────

    #[test]
    fn test_claims_file_pre_filter_and_static() {
        watchdog(Duration::from_secs(10), || {
            let (write, read, mock) = MockTransport::pair_with_handle();
            let ctx = make_host_global_config();
            let host = Arc::new(TsEngineHost::with_transport(write, read, ctx));
            let aliases = Arc::new(Mutex::new(HashMap::new()));
            let diag = Arc::new(Mutex::new(Vec::new()));
            let ext_id = ExtensionId::new("echo");
            let engine = TsEngine::new(
                "echo",
                false,
                PathBuf::from("/engines/echo.ts"),
                Arc::clone(&host),
                None,
                Some(vec![".echo".to_string()]),
                Some(vec![".echo".to_string()]),
                ext_id,
                aliases,
                diag,
            );

            // Claimed extension → true, no load.
            assert!(
                engine.claims_file("x.echo", ".echo"),
                "declared ext must return true"
            );
            assert_eq!(
                host.load_engine_count(),
                0,
                "static claims_file must not issue LoadEngine"
            );

            // Undeclared extension → false (pre-filtered), no load.
            assert!(
                !engine.claims_file("x.py", ".py"),
                "undeclared ext must return false (pre-filtered)"
            );
            assert_eq!(
                host.load_engine_count(),
                0,
                "pre-filter must not issue LoadEngine"
            );

            mock.signal_eof();
        });
    }

    // ── valid_extensions: static (no load) ────────────────────────────────────

    #[test]
    fn test_valid_extensions_static_no_load() {
        watchdog(Duration::from_secs(10), || {
            let (write, read, mock) = MockTransport::pair_with_handle();
            let ctx = make_host_global_config();
            let host = Arc::new(TsEngineHost::with_transport(write, read, ctx));
            let aliases = Arc::new(Mutex::new(HashMap::new()));
            let diag = Arc::new(Mutex::new(Vec::new()));
            let ext_id = ExtensionId::new("echo");
            let engine = TsEngine::new(
                "echo",
                false,
                PathBuf::from("/engines/echo.ts"),
                Arc::clone(&host),
                None,
                Some(vec![".echo".to_string()]),
                None,
                ext_id,
                aliases,
                diag,
            );

            let exts = engine.valid_extensions();
            assert_eq!(
                exts,
                vec![".echo"],
                "static file_extensions must be returned directly"
            );
            assert_eq!(
                host.load_engine_count(),
                0,
                "static valid_extensions must not issue LoadEngine"
            );

            mock.signal_eof();
        });
    }

    // ── Universal fallback key lookup (no load) ───────────────────────────────

    #[test]
    fn test_universal_fallback_lookup() {
        watchdog(Duration::from_secs(10), || {
            let mut claims_map = HashMap::new();
            claims_map.insert(
                "fallback".to_string(),
                StaticLanguageClaim {
                    kind: ClaimKind::Fallback,
                    priority: Some(0),
                    when_class: None,
                },
            );

            let (write, read, mock) = MockTransport::pair_with_handle();
            let ctx = make_host_global_config();
            let host = Arc::new(TsEngineHost::with_transport(write, read, ctx));
            let aliases = Arc::new(Mutex::new(HashMap::new()));
            let diag = Arc::new(Mutex::new(Vec::new()));
            let ext_id = ExtensionId::new("fallback-engine");
            let engine = TsEngine::new(
                "fallback-engine",
                false,
                PathBuf::from("/engines/fallback.ts"),
                Arc::clone(&host),
                Some(claims_map),
                None,
                None,
                ext_id,
                aliases,
                diag,
            );

            // Any language not in the map falls through to the "fallback" key.
            let claim = engine.claims_language("anylang", None);
            assert_eq!(
                claim,
                LanguageClaim::Fallback(0),
                "universal fallback key must apply for unrecognised language"
            );
            assert_eq!(
                host.load_engine_count(),
                0,
                "universal fallback must not issue LoadEngine"
            );

            mock.signal_eof();
        });
    }

    // ── P1-13: static-vs-dynamic validation (MockTransport) ──────────────────
    //
    // Named revert: removing the static-vs-dynamic validation loop from
    // `ensure_loaded` ⇒ no mismatch error → `result.is_err()` assertion RED.

    #[test]
    fn test_p1_13_static_vs_dynamic_validation_mismatch() {
        watchdog(Duration::from_secs(10), || {
            let (write, read, mock) = MockTransport::pair_with_handle();
            let ctx = make_host_global_config();
            let host = Arc::new(TsEngineHost::with_transport(write, read, ctx));
            let aliases = Arc::new(Mutex::new(HashMap::new()));
            let diag = Arc::new(Mutex::new(Vec::new()));
            let ext_id = ExtensionId::new("echo");
            let engine = TsEngine::new(
                "echo",
                false,
                PathBuf::from("/engines/echo.ts"),
                Arc::clone(&host),
                Some(single_claim("echo", ClaimKind::Primary, Some(1))),
                None,
                None,
                ext_id,
                aliases,
                diag,
            );

            // Script: LoadEngine succeeds, but ClaimsLanguage("echo") answers None (mismatch).
            mock.script_response(0, loaded_response("echo", vec![]));
            mock.script_response(1, claims_none_response()); // contradicts static Primary(1)

            // Step 1: static claims_language records ("echo", None) in static_answers.
            let static_claim = engine.claims_language("echo", None);
            assert_eq!(
                static_claim,
                LanguageClaim::Primary(1),
                "static claim must be Primary(1) before loading"
            );

            // Step 2: ensure_loaded sends LoadEngine (id=0) then validates static claims
            // by sending ClaimsLanguage (id=1) → gets None → mismatch → hard error.
            let c = Cancellation::new();
            let result = engine.ensure_loaded(&c);

            assert!(
                result.is_err(),
                "static-vs-dynamic mismatch must produce an error"
            );
            let err_msg = format!("{:?}", result.unwrap_err());
            assert!(
                err_msg.contains("echo"),
                "error message must name the engine; got: {err_msg}"
            );

            mock.signal_eof();
        });
    }

    // ── Important #1: Under-declaration is tolerated (one-directional validation) ──
    //
    // Validation must catch OVER-claiming only. A language that the module claims
    // dynamically but that the YAML never declared (under-declaration) must NOT
    // cause a hard render error — `ensure_loaded` must simply never validate it.
    //
    // Named revert: recording None answers in `static_answers` (the bug) would
    // cause "ruby" to be validated, producing a None-vs-Primary(1) mismatch →
    // hard Err. With the fix, "ruby" is never recorded → never validated → Ok.

    #[test]
    fn test_under_declaration_is_tolerated() {
        watchdog(Duration::from_secs(10), || {
            let (write, read, mock) = MockTransport::pair_with_handle();
            let ctx = make_host_global_config();
            let host = Arc::new(TsEngineHost::with_transport(write, read, ctx));
            let aliases = Arc::new(Mutex::new(HashMap::new()));
            let diag = Arc::new(Mutex::new(Vec::new()));
            let ext_id = ExtensionId::new("echo");
            let engine = TsEngine::new(
                "echo",
                false,
                PathBuf::from("/engines/echo.ts"),
                Arc::clone(&host),
                Some(single_claim("echo", ClaimKind::Primary, Some(1))),
                None,
                None,
                ext_id,
                aliases,
                diag,
            );

            // Probe "echo" → positive claim, must be recorded in static_answers.
            let claim_echo = engine.claims_language("echo", None);
            assert_eq!(
                claim_echo,
                LanguageClaim::Primary(1),
                "static 'echo' must be Primary(1)"
            );

            // Probe "ruby" → None (not in claims map). Must NOT be recorded because
            // recording None answers is what turns under-declaration into a hard error.
            let claim_ruby = engine.claims_language("ruby", None);
            assert_eq!(
                claim_ruby,
                LanguageClaim::None,
                "absent 'ruby' must return None"
            );

            // Script responses:
            //   id=0 LoadEngine → success
            //   id=1 ClaimsLanguage("echo") → Primary(1) (matches static → ok)
            //   id=2 ClaimsLanguage("ruby") → Primary(1) (under-declared; only reached if bug is present)
            // With the fix, id=2 is never sent. With the bug, id=2 is sent, static says None
            // but dynamic says Primary(1) → mismatch → Err.
            mock.script_response(0, loaded_response("echo", vec![]));
            mock.script_response(1, claims_primary_response(1)); // "echo" validation
            mock.script_response(2, claims_primary_response(1)); // "ruby" — only buggy code reaches this

            let c = Cancellation::new();
            let result = engine.ensure_loaded(&c);
            assert!(
                result.is_ok(),
                "under-declaration of 'ruby' must NOT produce an error; got: {:?}",
                result.err()
            );

            // Confirm only "echo" was validated: exactly 1 ClaimsLanguage wire call.
            let sent = mock.sent_messages();
            let claims_wire_count = sent
                .iter()
                .filter(|m| matches!(m, ToEngine::ClaimsLanguage { .. }))
                .count();
            assert_eq!(
                claims_wire_count, 1,
                "exactly 1 ClaimsLanguage wire call expected (echo only, not ruby); sent: {sent:?}"
            );

            mock.signal_eof();
        });
    }

    // ── Minor: claims_file pre-filter binding (claims_files: None, file_extensions only) ──
    //
    // The existing test has both file_extensions and claims_files set to [".echo"], so
    // deleting the pre-filter doesn't turn the .py→false assertion RED (the static
    // claims_files branch returns false too). This test uses claims_files: None — the
    // engine is content-inspecting (dynamic) for .echo files. With ONLY file_extensions
    // set, .py must be rejected purely by the pre-filter.
    //
    // Named revert: removing the pre-filter would fall through to the dynamic path
    // (ensure_loaded → ClaimsFile wire call), making load_engine_count ≥ 1 → RED.

    #[test]
    fn test_claims_file_pre_filter_binding() {
        watchdog(Duration::from_secs(10), || {
            let (write, read, mock) = MockTransport::pair_with_handle();
            let ctx = make_host_global_config();
            let host = Arc::new(TsEngineHost::with_transport(write, read, ctx));
            let aliases = Arc::new(Mutex::new(HashMap::new()));
            let diag = Arc::new(Mutex::new(Vec::new()));
            let ext_id = ExtensionId::new("echo-prefilter");
            let engine = TsEngine::new(
                "echo-prefilter",
                false,
                PathBuf::from("/engines/echo-prefilter.ts"),
                Arc::clone(&host),
                None,
                Some(vec![".echo".to_string()]), // file_extensions only — pre-filter guard
                None, // claims_files: None → content-inspecting (dynamic)
                ext_id,
                aliases,
                diag,
            );

            // .py is not in file_extensions → pre-filter returns false immediately, no load.
            assert!(
                !engine.claims_file("x.py", ".py"),
                ".py must be rejected by pre-filter (false)"
            );
            assert_eq!(
                host.load_engine_count(),
                0,
                "pre-filter must not trigger a load; if this fails, the pre-filter is missing"
            );

            mock.signal_eof();
        });
    }

    // ── quarto_required: carrier (inert in 1c) ────────────────────────────────

    #[test]
    fn test_quarto_required_carrier() {
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
                None,
                ext_id,
                aliases,
                diag,
            );

            // Unloaded → None.
            assert_eq!(
                engine.quarto_required(),
                None,
                "unloaded engine must return None for quarto_required"
            );

            // Script a LoadEngine response that includes quarto_required.
            mock.script_response(
                0,
                FromEngine::Loaded {
                    discovery: LoadEngineResult {
                        name: "julia".to_string(),
                        valid_extensions: vec![],
                        generates_figures: false,
                        can_freeze: false,
                        quarto_required: Some(">=1.9".to_string()),
                    },
                },
            );

            // Trigger load.
            let c = Cancellation::new();
            engine
                .ensure_loaded(&c)
                .expect("ensure_loaded must succeed");

            // After load → quarto_required returns the loaded value.
            assert_eq!(
                engine.quarto_required(),
                Some(">=1.9"),
                "loaded engine must report quarto_required from LoadEngineResult"
            );

            mock.signal_eof();
        });
    }
}
