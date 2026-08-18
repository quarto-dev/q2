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
    ExtensionId, FileClaim, StaticLanguageClaim, combine_claims, lookup_static_claim,
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
    /// Discovery result (immutable once set — OnceLock). Static discovery
    /// metadata (name / validExtensions / claims) never changes for a fixed
    /// `engine_path`, so this stays cached forever — including across a
    /// crash-triggered subprocess respawn. What CAN go stale across a
    /// respawn is the live subprocess's OWN registration of the module
    /// (`loadedByPath`/`engineByName` in host.ts, wiped when the process
    /// dies) — see `loaded_generation` below, which tracks THAT.
    discovery: OnceLock<LoadEngineResult>,

    /// Host generation (`TsEngineHost::spawn_count`) as of the last
    /// successful `LoadEngine` wire call. `ensure_loaded` compares this
    /// against the host's CURRENT generation to detect a crash-triggered
    /// respawn: a fresh subprocess has an empty `loadedByPath`/`engineByName`
    /// (host.ts), so `LaunchEngine` would fail with "engine not loaded"
    /// unless `LoadEngine` is resent first — even though `discovery` above
    /// (the cached RESULT) is still perfectly valid and is NOT re-validated,
    /// only re-sent. Timeout/Cancel do not bump the host generation (the
    /// process stays alive), so this stays a no-op for that existing path.
    loaded_generation: Mutex<Option<u64>>,

    /// Instance result — `Mutex<Option>` to allow poisoning.
    instance: Mutex<Option<LaunchEngineResult>>,

    /// Per-render project context. Set by `set_project` before `ensure_launched`
    /// (production call site: `build_engine_registry` at TsEngine construction),
    /// consumed by `ensure_launched` via `unwrap_or_default()`.
    ///
    /// First-write-wins per engine/host lifetime — launch caches make later
    /// writes inert by design. Every render builds a fresh registry+host, so the
    /// single write at registry build is complete. If engines ever outlive a
    /// ProjectContext (warm-host pooling), staleness is handled by invalidating
    /// the launched instance, not by resetting this field — see Plan 5
    /// §Invalidation.
    project: Mutex<Option<EngineProjectContext>>,

    /// Absolute path to the contributing extension's `_extension.yml`
    /// (Plan 6 Phase 5 provenance). Set post-construction via
    /// [`Self::set_extension_yml_path`], mirroring [`Self::set_project`] —
    /// the production call site (`build_engine_registry`) knows the path
    /// at `EngineContribution::External` destructuring time, one step
    /// after `TsEngine::new`.
    extension_yml_path: Mutex<Option<PathBuf>>,

    // ── Caches ───────────────────────────────────────────────────────────────
    claims_language_cache: Mutex<HashMap<(String, Option<String>), LanguageClaim>>,
    claims_file_cache: Mutex<HashMap<PathBuf, bool>>,

    // ── Static claims from `_extension.yml` ──────────────────────────────────
    /// Authoritative static language claims (from `_extension.yml`). `Some` → answer
    /// claims_language from this map WITHOUT loading; `None` → legacy dynamic load.
    claims: Option<HashMap<String, Vec<StaticLanguageClaim>>>,
    /// Complete handled-extension set; the claims_file pre-filter. `Some` → authoritative
    /// for valid_extensions; pre-filters claims_file (ext ∉ set ⇒ false, no load).
    file_extensions: Option<Vec<String>>,
    /// Unconditional static claims_file. `Some` → answer ext∈claims_files without loading;
    /// `None` → content-inspecting engine, dynamic claims_file load. Entries are
    /// undotted, lowercase (parse-time canonical form; see `extension::read::normalize_ext`).
    claims_files: Option<Vec<FileClaim>>,
    /// Records each statically-answered claims_language key for execute-time validation.
    static_answers: Mutex<Vec<(String, Option<String>)>>,
    /// Records each statically-answered claims_file extension for execute-time validation.
    static_file_answers: Mutex<Vec<String>>,
    /// Per-render markdown_for_file conversion cache (canonical path → converted QMD).
    /// Field added here; CONSUMED by Task 10's SourceConversionStage — unused for now.
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
        claims: Option<HashMap<String, Vec<StaticLanguageClaim>>>,
        file_extensions: Option<Vec<String>>,
        claims_files: Option<Vec<FileClaim>>,
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
            loaded_generation: Mutex::new(None),
            instance: Mutex::new(None),
            project: Mutex::new(None),
            extension_yml_path: Mutex::new(None),
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

        // `current_generation` reflects any respawn `ensure_started()` just
        // performed above (a crash-triggered reset clears the host's
        // transport, so the NEXT `ensure_started()` call — this one — does a
        // real respawn and bumps the generation before we read it here).
        //
        // Under a shared host with parallel renders, two engines can observe
        // the same fresh generation and both resend `LoadEngine` before either
        // records it below — a benign double-send: the TS harness's `loadEngine`
        // handler is idempotent per path (`loadedByPath`, host.ts), so the
        // second load returns the cached discovery without re-importing. Same
        // accepted double-send race as `discovery.set()` below.
        let current_generation = self.host.spawn_count();
        let stale_for_this_process =
            *self.loaded_generation.lock().unwrap() != Some(current_generation);

        if self.discovery.get().is_none() || stale_for_this_process {
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
                        static_claim = combine_claims(fb, first_class.as_deref());
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
                        .is_some_and(|cf| cf.iter().any(|c| c.extension == *ext));

                    // Synthetic filename "x<ext>" — declaring claims_files is an assertion
                    // that the engine claims every file of that extension unconditionally,
                    // regardless of content. The filename itself doesn't matter; the ext does.
                    // `ext` is the undotted canonical form; both wire fields go through
                    // `to_wire_ext` — the engine-side `claimsFile(file, ext)` JS contract
                    // compares `ext === ".echo"` (dotted), same as the dynamic path below.
                    let msg = ToEngine::ClaimsFile {
                        engine: result.name.clone(),
                        file: format!("x{}", to_wire_ext(ext)),
                        ext: to_wire_ext(ext),
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

            // Best-effort set; second racer fails silently (`discovery` is
            // immutable-once-set — a reload after a crash respawn recomputes
            // the SAME static result and this `set` is a harmless no-op).
            let _ = self.discovery.set(result);
            // Record the generation THIS successful LoadEngine round trip
            // was sent under, so a later respawn is detected again.
            *self.loaded_generation.lock().unwrap() = Some(current_generation);
        }

        Ok(self.discovery.get().unwrap())
    }

    /// Set the per-render project context for the next `ensure_launched` call.
    ///
    /// First-write-wins per engine/host lifetime — launch caches make later
    /// writes inert by design. Every render builds a fresh registry+host, so the
    /// single write at registry build (in `build_engine_registry`) is complete.
    /// If engines ever outlive a ProjectContext (warm-host pooling), staleness is
    /// handled by invalidating the launched instance, not by resetting this
    /// field — see Plan 5 §Invalidation.
    pub fn set_project(&self, project: EngineProjectContext) {
        *self.project.lock().unwrap() = Some(project);
    }

    /// Record the contributing extension's `_extension.yml` path
    /// (Plan 6 Phase 5 provenance). Mirrors [`Self::set_project`]:
    /// first-write-wins is fine because the production call site writes
    /// exactly once, right after construction.
    pub fn set_extension_yml_path(&self, path: PathBuf) {
        *self.extension_yml_path.lock().unwrap() = Some(path);
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
        let identifier = TsFormatIdentifier {
            base_format: ctx.format.clone(),
            target_format: ctx.format.clone(),
            display_name: ctx.format.clone(),
            extension_name: None,
        };

        let format_info = TsFormatInfo {
            identifier,
            // P1.1b: merged document metadata, lowered by
            // `EngineExecutionStage` via `document_metadata_to_ts_map` and
            // threaded through `ExecutionContext.metadata`. The Deno host's
            // `metadataAsFormat` partitions this flat map into the six-bin
            // `Format` the engine's `execute(opts)` receives.
            metadata: ctx.metadata.clone(),
        };

        let source_map = build_source_map(input, ctx);

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
            // The `TsHtmlDependency` wire type carries no version field, so
            // a TS engine cannot declare one today. `None` keeps the flat
            // `libs/{name}/` layout, which is exactly the pre-versioning
            // behavior (bd-add-html-dependency-version-5tnub5ds). Adding a
            // wire field would be a protocol change, out of scope here.
            version: None,
            stylesheets: dep.stylesheets.into_iter().map(PathBuf::from).collect(),
            scripts: dep.scripts.into_iter().map(PathBuf::from).collect(),
        }
    }

    /// Read each wire include value as a file path and return its content.
    ///
    /// The TS-engine wire contract puts temp-file PATHS in `includes`
    /// (mirroring Q1's `--include-in-header` contract that marimo/jupyter's
    /// TS engines all code against — see `TsPandocIncludes`'s doc comment).
    /// q2's internal `PandocIncludes` contract is CONTENT: `include_resolve.rs`
    /// folds these values verbatim, and the native knitr engine
    /// (`engine/knitr/mod.rs::convert_includes`) already reads its include
    /// files before populating the struct. No content-vs-path sniffing: a
    /// wire value that isn't a readable file is an engine protocol violation
    /// and fails loudly, naming the engine, the include key, and the
    /// offending value.
    fn read_include_contents(
        engine: &str,
        key: &str,
        paths: Option<Vec<String>>,
    ) -> Result<Vec<String>, ExecutionError> {
        paths
            .unwrap_or_default()
            .into_iter()
            .map(|path| {
                std::fs::read_to_string(&path).map_err(|e| {
                    ExecutionError::other(format!(
                        "engine '{engine}': include '{key}' value '{path}' is not a readable file: {e}"
                    ))
                })
            })
            .collect()
    }

    fn translate_includes(
        engine: &str,
        ts: Option<crate::engine::ts_protocol::TsPandocIncludes>,
    ) -> Result<PandocIncludes, ExecutionError> {
        match ts {
            None => Ok(PandocIncludes::default()),
            Some(inc) => Ok(PandocIncludes {
                header_includes: Self::read_include_contents(engine, "in_header", inc.in_header)?,
                include_before: Self::read_include_contents(
                    engine,
                    "before_body",
                    inc.before_body,
                )?,
                include_after: Self::read_include_contents(engine, "after_body", inc.after_body)?,
            }),
        }
    }

    /// Map a wire `TsExecuteResult` to the internal `ExecuteResult`.
    ///
    /// Extracted for direct unit-testability (F1 test seam). Fallible because
    /// `translate_includes` reads engine-reported include paths from disk —
    /// see its doc comment.
    fn map_execute_result(
        engine: &str,
        result: TsExecuteResult,
    ) -> Result<ExecuteResult, ExecutionError> {
        let html_dependencies = result
            .html_dependencies
            .into_iter()
            .map(Self::translate_html_dep)
            .collect();
        let includes = Self::translate_includes(engine, result.includes)?;
        let supporting_files = result.supporting.into_iter().map(PathBuf::from).collect();

        Ok(ExecuteResult {
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
        })
    }
}

// ============================================================================
// Execute source-map construction
// ============================================================================

/// Build the wire source map for the execute `input`, one entry per line, from
/// the per-render source provenance in `ctx.source_info` / `ctx.source_context`.
///
/// The engine-host rehydrates these entries into the `MappedString` the engine
/// receives (`rehydrateMappedString`). Engines that consume provenance require a
/// non-empty, correctly-mapped source map: the Julia engine's `buildSourceRanges`
/// maps every input line back to its origin, and an all-unmappable input would
/// make it send an *empty* `sourceRanges` array to QuartoNotebookRunner, which
/// crashes QNR's `compute_line_file_lookup` (`maximum` over an empty collection).
/// Previously this was stubbed to `Vec::new()`, which only worked for engines
/// (like echo) that never inspect the markdown's provenance.
///
/// Each line becomes one entry `{ start, length, source }`, where `source` maps
/// the line's start offset in `input` to its original file + byte offset via the
/// existing `SourceInfo::map_offset`. Lines with no recoverable origin (e.g.
/// `Generated`) get `source: None`, which the host treats as a synthetic segment.
/// The per-line entries tile `input` contiguously, so the host's greatest-lower-
/// bound `.map` lookup resolves any offset within a line.
fn build_source_map(
    input: &str,
    ctx: &ExecutionContext,
) -> Vec<crate::engine::ts_protocol::TsSourceMapEntry> {
    use crate::engine::ts_protocol::{TsSourceMapEntry, TsSourcePosition};

    let mut entries = Vec::new();
    let mut offset = 0usize;
    for line in input.split_inclusive('\n') {
        let length = line.len();
        let source = ctx
            .source_info
            .map_offset(offset, &ctx.source_context)
            .and_then(|mapped| {
                ctx.source_context
                    .get_file(mapped.file_id)
                    .map(|file| TsSourcePosition {
                        file: file.path.clone(),
                        file_offset: mapped.location.offset,
                    })
            });
        entries.push(TsSourceMapEntry {
            start: offset,
            length,
            source,
        });
        offset += length;
    }
    entries
}

// ============================================================================
// Wire-only re-dot adapter (change C, Plan 1c.2 Task 1)
// ============================================================================

/// The single Rust -> TS wire adapter for file extensions.
///
/// The canonical Rust-side form is **undotted** everywhere (parse-time
/// normalization in `extension::read::normalize_ext`; matching in
/// `claims_file` / `SourceConversionStage` compares undotted candidate
/// against undotted stored extensions). The wire contract stays **dotted**
/// (Q1 `extname()` parity; the engine-side JS contract compares
/// `ext === ".echo"`, e.g. `echo-engine.ts`'s `claimsFile`). This is the
/// *only* place that adds the dot back — call it at, and only at, a
/// Rust -> TS `ClaimsFile` wire seam. Empty stays empty (a file with no
/// extension was never dotted).
fn to_wire_ext(ext: &str) -> String {
    if ext.is_empty() {
        String::new()
    } else {
        format!(".{ext}")
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

/// Pure claim computation from a static `claims:` map (the two-step idiom:
/// the specific language key first, then the universal `fallback:` key when
/// the language itself is unclaimed). Shared by the cached, side-effecting
/// `claims_language` (which additionally records positive answers for
/// execute-time validation) and the side-effect-free `try_claims_language`
/// probe — neither loads anything; this function never touches `self`.
fn static_claim_from_map(
    map: &HashMap<String, Vec<StaticLanguageClaim>>,
    language: &str,
    first_class: Option<&str>,
) -> LanguageClaim {
    let mut claim = lookup_static_claim(map, language, first_class);
    if claim == LanguageClaim::None
        && let Some(fb) = map.get("fallback")
    {
        claim = combine_claims(fb, first_class);
    }
    claim
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
            let claim = static_claim_from_map(map, language, first_class);
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

    /// Answer from the static `claims:` map when present (no load); `None`
    /// (would-load) for a claims-less/legacy-dynamic engine. Deliberately
    /// does **not** call `ensure_loaded` and does **not** touch the cache or
    /// `static_answers` — this is a side-effect-free probe (Phase 4); any
    /// such recording belongs to the loading `claims_language` path so a
    /// Pass-1 probe never mutates execute-time validation state. See
    /// `ts_engine_static_iff_claims`.
    fn try_claims_language(
        &self,
        language: &str,
        first_class: Option<&str>,
    ) -> Option<LanguageClaim> {
        self.claims
            .as_ref()
            .map(|map| static_claim_from_map(map, language, first_class))
    }

    fn extension_yml_path(&self) -> Option<PathBuf> {
        self.extension_yml_path.lock().unwrap().clone()
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
            let claimed = cf.iter().any(|c| c.extension == ext);
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
                ext: to_wire_ext(ext),
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

        // Capture the transport generation the request is about to be sent on
        // (see the ProcessCrashed arm below). Read AFTER `ensure_launched`, so
        // it reflects the transport `request()` will actually use.
        let generation_at_send = self.host.spawn_count();

        // SCOPE NOTE: this poison guard covers crashes/timeouts/cancels
        // observed during the EXECUTE verb only. `ensure_launched()` above
        // (→ `ensure_loaded` → `host.load_engine` / `host.launch_engine`) uses
        // a bare `?` and is NOT wrapped by this guard, so a crash observed
        // during LoadEngine/LaunchEngine is not auto-recovered here. This
        // deliberately mirrors the PRE-EXISTING Cancel/Timeout guard scope
        // (also execute()-only) — expanding recovery to the load/launch verbs
        // is out of scope for the approved crash-relaunch fix.
        let response = self
            .host
            .request(msg, ctx.execute_timeout, &ctx.cancellation)
            .inspect_err(|e| {
                match e {
                    ExecutionError::Cancelled | ExecutionError::Timeout { .. } => {
                        // Process is still ALIVE — reuse the transport,
                        // only reset the logical launched instance (existing
                        // behavior, unchanged).
                        self.poison_instance();
                    }
                    ExecutionError::ProcessCrashed { .. } => {
                        // Process is DEAD — the transport (stdin pipe) is
                        // broken, not just the logical instance. Reset BOTH:
                        // the logical instance (so the next execute resends
                        // LaunchEngine) and the transport (so the next
                        // `ensure_started()` spawns a genuinely fresh
                        // subprocess instead of writing to a dead pipe).
                        //
                        // `generation_at_send` scopes the transport reset to
                        // the generation that actually crashed: under a shared
                        // host with parallel renders, a sibling may already
                        // have respawned by the time this stale observer runs,
                        // and `reset_after_crash` must NOT tear down that newer
                        // healthy transport (see its doc comment).
                        self.poison_instance();
                        self.host.reset_after_crash(generation_at_send);
                    }
                    _ => {}
                }
            })?;

        match response {
            FromEngine::ExecuteResult { result } => {
                Self::map_execute_result(&self.wire_name(), result)
            }
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
        // normalized to absolute + lexically clean by `SourceConversionStage`).
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
        claims: Option<HashMap<String, Vec<StaticLanguageClaim>>>,
        file_extensions: Option<Vec<String>>,
        claims_files: Option<Vec<FileClaim>>,
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

    /// Build a simple single-language static claims map for tests (1-element
    /// Vec per language — the pre-4c0 shape, still the common case).
    fn single_claim(
        language: &str,
        kind: ClaimKind,
        priority: Option<i32>,
    ) -> HashMap<String, Vec<StaticLanguageClaim>> {
        let mut m = HashMap::new();
        m.insert(
            language.to_string(),
            vec![StaticLanguageClaim {
                kind,
                priority,
                when_class: None,
            }],
        );
        m
    }

    /// Build a static claims map with an explicit multi-claim Vec per
    /// language (4c0 combine-rule test cases — e.g. a language key carrying
    /// both a `whenClass`-conditioned primary claim and an unconditional
    /// interop claim).
    fn multi_claim(
        entries: Vec<(&str, Vec<StaticLanguageClaim>)>,
    ) -> HashMap<String, Vec<StaticLanguageClaim>> {
        entries
            .into_iter()
            .map(|(lang, claims)| (lang.to_string(), claims))
            .collect()
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

    // ── Plan 4b F2: Poison policy — cooperative Cancel poisons ─────────────────
    //
    // Extends `test_cancel_distinguishable_and_prompt`
    // (crates/quarto-core/src/engine/ts_process.rs:~1723), which proves
    // Err(Cancelled) + promptness + `Cancel{target}` sent at the
    // `ts_process::request` layer. That test cannot observe `poison_instance()`
    // firing — poisoning happens one layer up, in `TsEngine::execute()`'s
    // `.inspect_err` on `Cancelled | Timeout` (~:805–815). This test drives the
    // SAME token-flip technique through `TsEngine::execute()` instead of
    // `TsEngineHost::request()` directly, so all four F2 assertions are bound
    // in one test: (1) `Err(Cancelled)`, (2) prompt return, (3) `Cancel{target}`
    // sent, (4) `poison_instance()` fired (instance is `None`, and a subsequent
    // `execute()` issues a fresh `LaunchEngine` — mirrors the sibling
    // `test_poison_policy_timeout_poisons` above for the Timeout branch of the
    // same `Cancelled | Timeout` poison guard).
    //
    // Named revert hunk: the cancel-flip poll in `ts_process.rs::request`
    // (~:693–701) — the same seam `test_cancel_distinguishable_and_prompt`
    // targets. Reverting it removes the `Cancelled` outcome, so `execute()`
    // never sees `Err(Cancelled)` and never poisons; this test (and others
    // sharing the seam, e.g. `test_cancel_distinguishable_and_prompt` and
    // `test_none_window_still_cancellable`) go RED on a full-crate run.
    #[test]
    fn test_poison_policy_cancel_poisons() {
        watchdog(Duration::from_secs(15), || {
            let (write, read, mock) = MockTransport::pair_with_handle();
            let ctx = make_host_global_config();
            let host = Arc::new(TsEngineHost::with_transport(write, read, ctx));
            let aliases = Arc::new(Mutex::new(HashMap::new()));
            let diag = Arc::new(Mutex::new(Vec::new()));
            let ext_id = ExtensionId::new("julia-cancel");
            let engine = TsEngine::new(
                "julia-cancel",
                false,
                PathBuf::from("/engines/julia-cancel.ts"),
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
            // id=2: Execute is withheld — cancelled via token flip, not timeout.

            let cancel = Cancellation::new();
            let cancel_clone = cancel.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(150));
                cancel_clone.cancel();
            });

            let exec_ctx = ExecutionContext::new(
                PathBuf::from("/tmp"),
                PathBuf::from("/proj"),
                PathBuf::from("/proj/doc.qmd"),
                "html",
            )
            // Long window: timeout can't masquerade as cancel.
            .with_execute_timeout(Some(Duration::from_secs(30)))
            .with_cancellation(cancel);

            let start = std::time::Instant::now();
            let result = engine.execute("# test", &exec_ctx);
            let elapsed = start.elapsed();

            // Assertion 1: distinguishable Cancelled error (not Timeout, not Ok).
            assert!(
                matches!(result, Err(ExecutionError::Cancelled)),
                "expected Cancelled from execute, got: {result:?}"
            );

            // Assertion 2: returns promptly — well under the 30s timeout window.
            assert!(
                elapsed < Duration::from_secs(5),
                "cancel took too long ({elapsed:?}); should be \u{226a} 30s"
            );

            // Assertion 3: Cancel{target} was sent on the wire.
            let sent = mock.sent_messages();
            assert!(
                sent.iter().any(|m| matches!(m, ToEngine::Cancel { .. })),
                "Cancel not in sent messages: {sent:?}"
            );

            // Assertion 4: poison_instance() fired — instance must be None.
            assert!(
                engine.instance.lock().unwrap().is_none(),
                "instance must be None (poisoned) after Cancelled"
            );

            // Corroborate poison behaviorally too: a subsequent execute must
            // issue a fresh LaunchEngine (mirrors test_poison_policy_timeout_poisons).
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
                                result: TsExecuteResult {
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
                "after Cancel poison, a new LaunchEngine must be issued; \
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

    // ── Phase 4: no-load claim probe (`try_claims_language`) ──────────────────
    //
    // Named revert: if `try_claims_language` fell back to `ensure_loaded`
    // for either branch, `sent_messages()` would be non-empty (the mock
    // transport only ever sees a message when the host is actually asked
    // to do something) → assertion RED. This is the strongest available
    // mechanical proof of "not loaded" without a real subprocess: the
    // engine here has no bundle/host behind the mock transport at all, so
    // any attempt to load would have to go through `sent_messages()`.

    #[test]
    fn ts_engine_static_iff_claims() {
        watchdog(Duration::from_secs(10), || {
            // `claims: Some` → answers directly from the static map, no wire
            // traffic at all.
            let (engine, mock) = make_engine_with_mock(
                "julia",
                Some(single_claim("r", ClaimKind::Primary, Some(1))),
                None,
                None,
            );
            assert_eq!(
                engine.try_claims_language("r", None),
                Some(LanguageClaim::Primary(1)),
                "static claims map ⇒ Some(...) without loading"
            );
            assert_eq!(
                engine.try_claims_language("python", None),
                Some(LanguageClaim::None),
                "static claims map ⇒ still a static Some(None) for an unclaimed language"
            );
            assert_eq!(
                mock.sent_messages().len(),
                0,
                "try_claims_language must be a side-effect-free probe: NO wire \
                 messages at all — proves ensure_loaded was never called"
            );
            mock.signal_eof();

            // `claims: None` (dynamic/legacy engine) → would-load ⇒ `None`,
            // and the probe itself must not trigger a load merely to answer.
            let (dyn_engine, mock2) = make_engine_with_mock("dynamic", None, None, None);
            assert_eq!(
                dyn_engine.try_claims_language("r", None),
                None,
                "no static claims map ⇒ would-load ⇒ None"
            );
            assert_eq!(
                mock2.sent_messages().len(),
                0,
                "try_claims_language must never call ensure_loaded, even to \
                 answer 'would-load'"
            );
            mock2.signal_eof();
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
        let result = TsEngine::map_execute_result("julia", ts_result)
            .expect("no includes present, so mapping must succeed");
        assert!(
            result.needs_postprocess,
            "post_process: true must wire-feed needs_postprocess: true via map_execute_result"
        );
    }

    // ── Fix #2: TS-engine wire includes are file paths — read into content ────
    //
    // Root cause: `translate_includes` mapped `TsPandocIncludes` string values
    // (temp-file PATHS, per the Q1 engine contract marimo/jupyter/knitr all
    // code against) verbatim into `PandocIncludes` (q2's internal CONTENT
    // contract). Named revert: reverting `read_include_contents` to pass the
    // path string straight through (instead of `std::fs::read_to_string`-ing
    // it) → the content assertions below RED because they'd observe the raw
    // path instead of the file's content.

    #[test]
    fn test_translate_includes_reads_in_header_file_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("header.html");
        std::fs::write(&path, "<marimo-code data-test>").unwrap();

        let ts_result = TsExecuteResult {
            includes: Some(crate::engine::ts_protocol::TsPandocIncludes {
                in_header: Some(vec![path.to_string_lossy().into_owned()]),
                before_body: None,
                after_body: None,
            }),
            ..Default::default()
        };

        let result = TsEngine::map_execute_result("marimo", ts_result)
            .expect("a readable include path must map successfully");

        assert_eq!(
            result.includes.header_includes,
            vec!["<marimo-code data-test>".to_string()],
            "header_includes must contain the file's CONTENT, not its path"
        );
    }

    #[test]
    fn test_translate_includes_reads_before_and_after_body_file_content() {
        let dir = tempfile::tempdir().unwrap();
        let before_path = dir.path().join("before.html");
        let after_path = dir.path().join("after.html");
        std::fs::write(&before_path, "<div>before</div>").unwrap();
        std::fs::write(&after_path, "<div>after</div>").unwrap();

        let ts_result = TsExecuteResult {
            includes: Some(crate::engine::ts_protocol::TsPandocIncludes {
                in_header: None,
                before_body: Some(vec![before_path.to_string_lossy().into_owned()]),
                after_body: Some(vec![after_path.to_string_lossy().into_owned()]),
            }),
            ..Default::default()
        };

        let result = TsEngine::map_execute_result("marimo", ts_result)
            .expect("readable include paths must map successfully");

        assert_eq!(
            result.includes.include_before,
            vec!["<div>before</div>".to_string()],
            "include_before must contain the file's CONTENT, not its path"
        );
        assert_eq!(
            result.includes.include_after,
            vec!["<div>after</div>".to_string()],
            "include_after must contain the file's CONTENT, not its path"
        );
    }

    #[test]
    fn test_translate_includes_nonexistent_path_errs_loudly() {
        let ts_result = TsExecuteResult {
            includes: Some(crate::engine::ts_protocol::TsPandocIncludes {
                in_header: Some(vec!["/nonexistent/does-not-exist.html".to_string()]),
                before_body: None,
                after_body: None,
            }),
            ..Default::default()
        };

        let err = TsEngine::map_execute_result("marimo", ts_result)
            .expect_err("a nonexistent include path must fail loudly, not pass through silently");
        let message = err.to_string();
        assert!(
            message.contains("in_header"),
            "error must name the offending include key; got: {message}"
        );
        assert!(
            message.contains("/nonexistent/does-not-exist.html"),
            "error must name the offending value; got: {message}"
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
                vec![StaticLanguageClaim {
                    kind: ClaimKind::Primary,
                    priority: None,
                    when_class: Some("marimo".to_string()),
                }],
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
                Some(vec![FileClaim {
                    extension: ".echo".to_string(),
                }]),
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

    // ── T11 (1c.2 Task 1, change C): wire carries dotted lowercase ────────────
    //
    // The stage (and `claims_file`'s callers generally) now pass extensions
    // UNDOTTED. `to_wire_ext` is the single adapter that re-dots at the two
    // Rust -> TS `ClaimsFile` seams. Named revert: replace both `to_wire_ext(...)`
    // call sites with the bare undotted value (`ext.to_string()`) → the
    // captured wire message's `ext`/`file` fields go undotted → assertions
    // below fail → RED.

    #[test]
    fn t11_wire_claims_file_messages_carry_dotted_lowercase_ext() {
        // ── Part A: dynamic path — engine has NO static claims_files, so
        // `claims_file` falls through to a real `ToEngine::ClaimsFile` wire
        // round-trip. Assert the wire `ext` field is dotted.
        watchdog(Duration::from_secs(10), || {
            let (write, read, mock) = MockTransport::pair_with_handle();
            let ctx = make_host_global_config();
            let host = Arc::new(TsEngineHost::with_transport(write, read, ctx));
            let aliases = Arc::new(Mutex::new(HashMap::new()));
            let diag = Arc::new(Mutex::new(Vec::new()));
            let ext_id = ExtensionId::new("echo-dynamic");
            let engine = TsEngine::new(
                "echo-dynamic",
                false,
                PathBuf::from("/engines/echo-dynamic.ts"),
                Arc::clone(&host),
                None,
                None,
                None, // claims_files: None → content-inspecting, dynamic wire call
                ext_id,
                aliases,
                diag,
            );

            mock.script_response(0, loaded_response("echo-dynamic", vec![]));
            mock.script_response(1, FromEngine::ClaimsFileResult { result: true });

            // Caller passes the candidate ext UNDOTTED (mirrors the stage post-change C).
            let claimed = engine.claims_file("x.echo", "echo");
            assert!(claimed, "engine must claim via the dynamic wire round-trip");

            let sent = mock.sent_messages();
            let claims_file_msg = sent
                .iter()
                .find(|m| matches!(m, ToEngine::ClaimsFile { .. }))
                .expect("a ToEngine::ClaimsFile message must have been sent");
            match claims_file_msg {
                ToEngine::ClaimsFile { ext, .. } => {
                    assert_eq!(
                        ext, ".echo",
                        "wire ext must be re-dotted by to_wire_ext even though the \
                         candidate passed to claims_file was undotted"
                    );
                }
                other => panic!("expected ToEngine::ClaimsFile, got {other:?}"),
            }

            mock.signal_eof();
        });

        // ── Part B: synthetic-file load-validation message — engine WITH a
        // static claims_files declaration. Assert the wire `file` field is
        // the dotted synthetic name ("x.echo", not "xecho").
        watchdog(Duration::from_secs(10), || {
            let (write, read, mock) = MockTransport::pair_with_handle();
            let ctx = make_host_global_config();
            let host = Arc::new(TsEngineHost::with_transport(write, read, ctx));
            let aliases = Arc::new(Mutex::new(HashMap::new()));
            let diag = Arc::new(Mutex::new(Vec::new()));
            let ext_id = ExtensionId::new("echo-static");
            let engine = TsEngine::new(
                "echo-static",
                false,
                PathBuf::from("/engines/echo-static.ts"),
                Arc::clone(&host),
                None,
                None,
                Some(vec![FileClaim {
                    extension: "echo".to_string(),
                }]),
                ext_id,
                aliases,
                diag,
            );

            // Static claim, no load: records undotted "echo" in static_file_answers.
            assert!(engine.claims_file("x.echo", "echo"));
            assert_eq!(host.load_engine_count(), 0);

            // ensure_loaded validates the recorded static answer against a
            // (mocked) LoadEngine + ClaimsFile round-trip.
            mock.script_response(0, loaded_response("echo-static", vec![]));
            mock.script_response(1, FromEngine::ClaimsFileResult { result: true });

            let c = Cancellation::new();
            let result = engine.ensure_loaded(&c);
            assert!(
                result.is_ok(),
                "validation must succeed when dynamic matches static; got {:?}",
                result.err()
            );

            let sent = mock.sent_messages();
            let claims_file_msg = sent
                .iter()
                .find(|m| matches!(m, ToEngine::ClaimsFile { .. }))
                .expect("a ToEngine::ClaimsFile validation message must have been sent");
            match claims_file_msg {
                ToEngine::ClaimsFile { file, ext, .. } => {
                    assert_eq!(
                        file, "x.echo",
                        "synthetic validation filename must be dotted (x.echo), not xecho"
                    );
                    assert_eq!(
                        ext, ".echo",
                        "validation wire ext must also be dotted (matches the engine-side \
                         `ext === \".echo\"` JS contract, e.g. echo-engine.ts's claimsFile)"
                    );
                }
                other => panic!("expected ToEngine::ClaimsFile, got {other:?}"),
            }

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
                vec![StaticLanguageClaim {
                    kind: ClaimKind::Fallback,
                    priority: Some(0),
                    when_class: None,
                }],
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

    // ── SC3: fallback-key Vec combine (claims_language, ts_engine.rs:604-607) ──
    //
    // Named revert: reverting the `map.get("fallback")` site back to the
    // pre-Vec single-claim `static_claim_to_language_claim(fb, …)` call is a
    // compile-error against the Vec-typed `claims` map — a compile failure
    // counts as RED per the plan's SC3 spec (the type change forces the
    // production site to route through the Vec combiner).

    #[test]
    fn sc3_fallback_key_vec_combine_zero_load() {
        watchdog(Duration::from_secs(10), || {
            // A registry with a real parsed-shape claims map: a normal
            // language claim ("python") alongside a "fallback" key, both
            // built from real `StaticLanguageClaim` Vecs (no closures/mocks
            // in the claim data itself — only the transport is mocked).
            let claims_map = multi_claim(vec![
                (
                    "python",
                    vec![StaticLanguageClaim {
                        kind: ClaimKind::Primary,
                        priority: Some(1),
                        when_class: None,
                    }],
                ),
                (
                    "fallback",
                    vec![StaticLanguageClaim {
                        kind: ClaimKind::Fallback,
                        priority: Some(-5),
                        when_class: None,
                    }],
                ),
            ]);

            let (engine, mock) =
                make_engine_with_mock("jupyter-like", Some(claims_map), None, None);

            // "ruby" has no direct entry — must fall through to the
            // "fallback" key's Vec, combined via the same reducer as any
            // other language key, entirely without loading.
            let claim = engine.claims_language("ruby", None);
            assert_eq!(
                claim,
                LanguageClaim::Fallback(-5),
                "unclaimed language must resolve via the fallback-key Vec combine"
            );
            assert!(
                !engine.host.is_alive(),
                "fallback Vec combine must not spawn a subprocess"
            );
            assert_eq!(
                engine.host.load_engine_count(),
                0,
                "fallback Vec combine must not issue LoadEngine"
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

    // ── build_source_map: per-line provenance mapping ──────────────────────

    /// `build_source_map` must resolve each line's *file* offset (not just
    /// its local offset in `input`) through the real `SourceInfo::map_offset`
    /// chain — this is the seam the Plan 4B task review flagged as
    /// native-untested: only the julia+deno-gated e2e exercised it before
    /// this test.
    ///
    /// Fixture: a file with a leading "before\n" line, so the engine's
    /// `input` (which covers only "line1\nline2\n", starting at file byte
    /// offset 7) has line-start file offsets that are *not* equal to their
    /// local offsets in `input`. An identity/stub mapping (e.g. the
    /// pre-6a5f80fc4 `Vec::new()`) cannot produce these values by accident.
    #[test]
    fn test_build_source_map_maps_lines_to_file_provenance() {
        use quarto_source_map::{Location, Range, SourceContext, SourceInfo};

        let mut source_context = SourceContext::new();
        let file_content = "before\nline1\nline2\n";
        let file_id =
            source_context.add_file("test.qmd".to_string(), Some(file_content.to_string()));

        // The engine's `input` is the substring "line1\nline2\n", which
        // starts at file byte offset 7 ("before\n".len()).
        let input = "line1\nline2\n";
        assert_eq!(&file_content[7..], input);

        let source_info = SourceInfo::from_range(
            file_id,
            Range {
                start: Location {
                    offset: 7,
                    row: 1,
                    column: 0,
                },
                end: Location {
                    offset: 7 + input.len(),
                    row: 3,
                    column: 0,
                },
            },
        );

        let ctx = ExecutionContext::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/tmp"),
            PathBuf::from("test.qmd"),
            "html",
        )
        .with_source_info(source_info, Arc::new(source_context));

        let entries = build_source_map(input, &ctx);

        assert_eq!(entries.len(), 2, "expected one source-map entry per line");

        // Line 1: "line1\n" — local [0, 6), file offset 7 (start of "line1").
        assert_eq!(entries[0].start, 0);
        assert_eq!(entries[0].length, 6);
        let source0 = entries[0]
            .source
            .as_ref()
            .expect("line 1 must resolve to real provenance, not None");
        assert_eq!(source0.file, "test.qmd");
        assert_eq!(
            source0.file_offset, 7,
            "line 1's file offset must be its real position in the file, not its local offset (0)"
        );

        // Line 2: "line2\n" — local [6, 12), file offset 13 (start of "line2").
        assert_eq!(entries[1].start, 6);
        assert_eq!(entries[1].length, 6);
        let source1 = entries[1]
            .source
            .as_ref()
            .expect("line 2 must resolve to real provenance, not None");
        assert_eq!(source1.file, "test.qmd");
        assert_eq!(
            source1.file_offset, 13,
            "line 2's file offset must be its real position in the file, not its local offset (6)"
        );
    }
}
