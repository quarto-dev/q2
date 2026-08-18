/*
 * engine/ts_protocol.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * JSON wire protocol types for q2 ↔ Deno engine-host communication.
 *
 * Pure serde data — no behavior, no conversion logic, no subprocess code.
 * Conversions to/from q2-native types live in a later plan's `ts_engine.rs`.
 */

//! JSON wire protocol types for q2 ↔ Deno engine-host communication.
//!
//! All types carry `#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]`
//! so callers can construct, assert, and compare protocol values in tests.
//!
//! # Wire-shape conventions
//!
//! - Struct field names: **camelCase** on the wire (`#[serde(rename_all = "camelCase")]`).
//! - Message enum tags: internal `type` field with explicit per-variant renames.
//! - `TsLanguageClaim`: internal `kind` tag, lowercase variant names.
//! - `TsFormatIdentifier`: explicit kebab-case per-field renames; `extension-name` omitted when `None`.
//! - `TsMetadataValue`: untagged (each variant serializes bare).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ==================== Message enums ====================

/// Messages from Rust (q2) → Deno engine host.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(tag = "type", rename_all_fields = "camelCase")]
pub enum ToEngine {
    // === Lifecycle (two-step init) ===
    /// Fire-and-forget global config frame, sent once at subprocess spawn.
    /// Not addressed to any engine; carries process-stable ambient config.
    #[serde(rename = "init")]
    Init { global: HostGlobalConfig },

    /// Run `import(enginePath)` and construct the `ExecutionEngineDiscovery` object.
    #[serde(rename = "loadEngine")]
    LoadEngine { engine_path: String },

    /// Call `engine.launch(project)` and track the resulting instance.
    #[serde(rename = "launchEngine")]
    LaunchEngine {
        engine: String,
        project: EngineProjectContext,
    },

    /// Shut down the entire subprocess (all engines).
    #[serde(rename = "shutdown")]
    Shutdown,

    // === Discovery (ExecutionEngineDiscovery) — needs LoadEngine only ===
    #[serde(rename = "claimsLanguage")]
    ClaimsLanguage {
        engine: String,
        language: String,
        first_class: Option<String>,
    },

    #[serde(rename = "claimsFile")]
    ClaimsFile {
        engine: String,
        file: String,
        ext: String,
    },

    // === Instance methods — need LaunchEngine ===
    #[serde(rename = "markdownForFile")]
    MarkdownForFile { engine: String, file: String },

    #[serde(rename = "execute")]
    Execute {
        engine: String,
        options: TsExecuteOptions,
    },

    /// Pure prediction of intermediate file paths alongside the primary output.
    #[serde(rename = "intermediateFiles")]
    IntermediateFiles { engine: String, input: String },

    /// Deferred-dependency resolution: resolve the `engineDependencies` slice
    /// that was returned by a prior `Execute` with `dependencies: false`.
    /// Symmetric sibling of `IntermediateFiles` / `IntermediateFilesResult`.
    /// (RTQ FC-2 infrastructure; the caller — the render orchestrator — is
    /// a deferred consumer and is NOT yet wired in RTQ v1.)
    #[serde(rename = "dependencies")]
    Dependencies {
        engine: String,
        options: TsDependenciesOptions,
    },

    /// Cooperative cancel of an in-flight request (fire-and-forget). `target` is
    /// the `id` of the request to abort. `Cancel` rides its own `Request`
    /// envelope, but the host does NOT register a pending slot for it — the
    /// *target* request resolves with `Cancelled` (or its natural result if it
    /// finished first). See plan1a-protocol Phase 1.5 / engine-host-concurrency.md.
    #[serde(rename = "cancel")]
    Cancel { target: u64 },
}

/// Messages from Deno engine host → Rust (q2).
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(tag = "type", rename_all_fields = "camelCase")]
pub enum FromEngine {
    // === Lifecycle ===
    #[serde(rename = "loaded")]
    Loaded { discovery: LoadEngineResult },

    #[serde(rename = "launched")]
    Launched { instance: LaunchEngineResult },

    #[serde(rename = "error")]
    Error {
        message: String,
        stack: Option<String>,
    },

    // === Discovery responses ===
    #[serde(rename = "claimsLanguageResult")]
    ClaimsLanguageResult { result: Option<TsLanguageClaim> },

    #[serde(rename = "claimsFileResult")]
    ClaimsFileResult { result: bool },

    // === Instance method responses ===
    #[serde(rename = "markdownForFileResult")]
    MarkdownForFileResult { result: TsMappedStringWithMap },

    #[serde(rename = "executeResult")]
    ExecuteResult { result: TsExecuteResult },

    #[serde(rename = "intermediateFilesResult")]
    IntermediateFilesResult { result: Option<Vec<String>> },

    /// Response to `Dependencies`: the resolved Pandoc includes for this engine's
    /// deferred deps.
    /// (RTQ FC-2 infrastructure; no RTQ v1 caller — the `FromEngine` response
    /// match in `ts_engine.rs` treats this as an unexpected variant via its
    /// catch-all `other =>` arm.)
    #[serde(rename = "dependenciesResult")]
    DependenciesResult { includes: TsPandocIncludes },

    /// Acknowledges a cooperative `Cancel`, delivered under the cancelled
    /// request's `id`. Unit variant: under internal tagging it serializes to
    /// exactly `{"type":"cancelled"}` (the `type` tag is the only wire field),
    /// matching the harness's payload-free `cancelled` message.
    #[serde(rename = "cancelled")]
    Cancelled,
}

// ==================== Correlation envelope (Phase 1.5) ====================

/// A `ToEngine` message wrapped with a correlation `id` allocated by the Rust
/// host; the response echoes it back. Wire shape: `{ "id": N, "msg": { "type":
/// …, … } }`.
///
/// The `msg` is **nested**, deliberately NOT `#[serde(flatten)]` —
/// flatten round-trips poorly with internally-tagged enums (`#[serde(tag =
/// "type")]`). The thin envelope is all that is added; the typed payload stays
/// `serde_json::Value`-free.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Request {
    pub id: u64,
    pub msg: ToEngine,
}

/// A `FromEngine` message wrapped with the `id` of the request it answers. The
/// demux on the Rust side routes responses by this `id`; a response whose `id`
/// is no longer pending (a late reply after a cancel) is dropped.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Response {
    pub id: u64,
    pub msg: FromEngine,
}

// ==================== Lifecycle response payloads ====================

/// Response to `LoadEngine` — discovery surface (cheap to obtain).
///
/// Mirrors Q1's `ExecutionEngineDiscovery` (`execute/types.ts`).  Fields are
/// static properties knowable before the engine is launched.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LoadEngineResult {
    pub name: String,
    pub valid_extensions: Vec<String>,
    /// Whether the engine can produce figures (Q1: `generatesFigures`).
    /// Defaults to `false` when absent from the wire (harness emission is plan1b).
    #[serde(default)]
    pub generates_figures: bool,
    /// Whether the engine supports `freeze` (Q1: `canFreeze`).
    /// Defaults to `false` when absent from the wire (harness emission is plan1b).
    #[serde(default)]
    pub can_freeze: bool,
    /// Minimum Quarto version required by this engine (Q1: `quartoRequired?`).
    /// Inert wire field — no gate/enforcement logic at this tier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quarto_required: Option<String>,
}

/// Response to `LaunchEngine` — instance metadata available after `launch()`.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LaunchEngineResult {
    pub can_freeze: bool,
}

// ==================== Language claim ====================

/// Kind-tagged language claim returned by `claimsLanguage`.
///
/// `kind` sets the resolution tier; `priority` orders within a kind.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum TsLanguageClaim {
    Primary { priority: i32 },
    Interop { priority: i32 },
    Fallback { priority: i32 },
}

// ==================== Engine host context (split into two frames) ====================

/// Process-stable config, delivered once on `Init` at spawn (ambient — never gated).
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HostGlobalConfig {
    pub resource_dir: String,
    pub runtime_dir: String,
    /// Q1-compatible data dir, new in RTQ.
    pub data_dir: String,
    pub pandoc_path: Option<String>,
    pub is_interactive_session: bool,
    pub running_in_ci: bool,
    pub quarto_version: String,
}

/// Per-render project context, carried on each `LaunchEngine`, captured in the instance.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct EngineProjectContext {
    pub project_dir: Option<String>,
    pub is_single_file: bool,
    /// DQ-5: project `engines` settings + `output-dir` key, as values.
    pub config: Option<HashMap<String, TsMetadataValue>>,
    /// DQ-5: Q1 getOutputDirectory() return, as a value.
    pub output_dir: Option<String>,
}

// ==================== Mapped string with source map ====================

/// Used in `MarkdownForFileResult` (non-QMD file conversion).
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TsMappedStringWithMap {
    pub value: String,
    pub file_name: Option<String>,
    pub source_map: Vec<TsSourceMapEntry>,
}

// ==================== Pandoc types ====================

/// Wire values are file PATHS, not content — this mirrors Q1's
/// `--include-in-header`/`--include-before-body`/`--include-after-body`
/// contract, which marimo, jupyter, and knitr's TS engines all code against
/// (marimo-engine.ts writes a temp file and sends its path "like Jupyter
/// does"). `TsEngine::translate_includes` (`ts_engine.rs`) reads each path
/// into content at the wire boundary before it reaches q2's internal
/// `PandocIncludes` (content) contract; a value that isn't a readable file
/// is treated as an engine protocol violation, not silently passed through.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TsPandocIncludes {
    pub in_header: Option<Vec<String>>,
    pub before_body: Option<Vec<String>>,
    pub after_body: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TsPandocAttr {
    pub id: String,
    pub classes: Vec<String>,
    /// `Vec` preserves duplicate keys over the wire.
    pub keyvalue: Vec<(String, String)>,
}

// ==================== Format info ====================

/// Format identifier + merged document metadata sent with `Execute`.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TsFormatInfo {
    pub identifier: TsFormatIdentifier,
    /// Merged document metadata, JSON-shaped.
    pub metadata: HashMap<String, TsMetadataValue>,
}

/// Q1-compatible `FormatIdentifier` with kebab-case wire keys.
///
/// `extension-name` is omitted from the wire when `None`.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct TsFormatIdentifier {
    #[serde(rename = "base-format")]
    pub base_format: String,
    #[serde(rename = "target-format")]
    pub target_format: String,
    #[serde(rename = "display-name")]
    pub display_name: String,
    #[serde(rename = "extension-name", skip_serializing_if = "Option::is_none")]
    pub extension_name: Option<String>,
}

// ==================== Metadata value ====================

/// JSON-shaped metadata value. Serializes as a bare JSON value (untagged).
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(untagged)]
pub enum TsMetadataValue {
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<TsMetadataValue>),
    Map(HashMap<String, TsMetadataValue>),
    Null,
}

// ==================== Source map ====================

/// Byte-range source-map entry.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TsSourceMapEntry {
    pub start: usize,
    pub length: usize,
    pub source: Option<TsSourcePosition>,
}

/// Source-file position for a mappable range.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TsSourcePosition {
    pub file: String,
    pub file_offset: usize,
}

// ==================== Execute options ====================

/// Returns `true`; used as a `serde(default = …)` fn for
/// `TsExecuteOptions::dependencies` so that an absent key deserializes as `true`
/// (Q1's default) rather than `false` (Rust's `bool` zero-value).
fn default_true() -> bool {
    true
}

/// Options sent to the engine with each `Execute` message.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TsExecuteOptions {
    pub input: String,
    pub source_path: String,
    pub format: TsFormatInfo,
    pub temp_dir: String,
    pub cwd: String,
    pub project_dir: Option<String>,
    pub lib_dir: String,
    pub quiet: bool,
    /// The **leave-alone set** for this engine, not a positive "languages
    /// assigned to me" set. Mirrors
    /// `EngineResolution::handled_languages_for` (`resolution.rs:292`):
    /// q2's built-in `HANDLED_LANGUAGES` ∪ every language this render
    /// assigned to a DIFFERENT engine. A TS engine that wants to know
    /// "did q2 assign ME ownership of language L for this render" must
    /// infer it as the *complement* — L present in the document AND L
    /// absent from `handled_languages` — not by looking for its own name
    /// inside the array (it never appears there). This is sound because
    /// q2's resolver assigns every language present in the document an
    /// owner, or hard-fails, before `execute()` ever runs, so "L absent
    /// from this set" and "L owned by me" coincide for any L the engine
    /// actually has to make a decision about.
    ///
    /// Got this backwards once already: the marimo TS engine's bare-`sql`
    /// Interop gate (`bareSqlOwned`) read `handled_languages.includes("sql")`
    /// as a positive-ownership check, so it evaluated `true` exactly when
    /// q2 had left `sql` alone for some OTHER engine — precisely when
    /// marimo does NOT own it — and the bare-sql interop feature never
    /// fired (q2 plan4c FINDING #4, fixed 2026-07-02 upstream in
    /// `quarto-marimo`).
    pub handled_languages: Vec<String>,
    pub params: Option<HashMap<String, TsMetadataValue>>,
    pub source_map: Vec<TsSourceMapEntry>,
    /// Whether to resolve dependencies inline (`true`, Q1 default) or defer them
    /// to an explicit `dependencies(…)` call (`false`).  Custom serde default so
    /// an absent wire key deserializes as `true` (Q1-compatible behaviour).
    #[serde(default = "default_true")]
    pub dependencies: bool,
}

// ==================== Execute result ====================

/// Result returned by the engine after execution.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct TsExecuteResult {
    pub markdown: String,
    pub supporting: Vec<String>,
    pub filters: Vec<String>,
    pub includes: Option<TsPandocIncludes>,
    pub html_dependencies: Vec<TsHtmlDependency>,
    /// Engine-produced document metadata (carried-and-ignored until a consumer lands).
    #[serde(default)]
    pub metadata: Option<HashMap<String, TsMetadataValue>>,
    /// Engine-produced pandoc options (carried-and-ignored until a consumer lands).
    ///
    /// Note: SDK's `pandoc?` is `Record<string, unknown>` — loosely typed; do
    /// NOT over-type as a structured `FormatPandoc`.
    #[serde(default)]
    pub pandoc: Option<HashMap<String, TsMetadataValue>>,
    /// Resource files produced by the engine (carried-and-ignored until a consumer lands).
    #[serde(default)]
    pub resource_files: Vec<String>,
    /// Key→value preserve map from the engine (carried-and-ignored until a consumer lands).
    #[serde(default)]
    pub preserve: HashMap<String, String>,
    /// Whether the output requires post-processing.
    ///
    /// Wire-fed directly into `ExecuteResult::needs_postprocess`.
    #[serde(default)]
    pub post_process: bool,
    /// Engine-name-keyed deferred dependency map, populated when
    /// `TsExecuteOptions::dependencies` is `false`.
    /// Forwarded to q2; NOT resolved by the harness.
    /// The render orchestrator later sends a `Dependencies` message per key.
    /// (Deferred consumer — NOT yet wired in RTQ; will land with the book feature.)
    #[serde(default)]
    pub engine_dependencies: Option<HashMap<String, Vec<TsMetadataValue>>>,
}

// ==================== Dependencies options ====================

/// Options sent to the engine with each `Dependencies` message.
///
/// Symmetric sibling of `TsExecuteOptions` for the deferred-dependency path.
///
/// Q2-shape reconciliations vs Q1's `DependenciesOptions`:
///   - **(DQ-3)** Flattened target fields (`input`, `source_path`, …) instead of
///     an `ExecutionTarget` cookie.
///   - **(Item A)** `resource_dir` is OMITTED — ambient via `Init.global` /
///     `path.resource`, exactly as `TsExecuteOptions` omits it.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TsDependenciesOptions {
    pub input: String,
    pub source_path: String,
    pub source_map: Vec<TsSourceMapEntry>,
    pub format: TsFormatInfo,
    /// Final/merged output path — supplied at the call site by the orchestrator.
    pub output: String,
    /// Per-render scratch directory (NOT `resource_dir` — that is ambient via `Init.global`).
    pub temp_dir: String,
    pub lib_dir: Option<String>,
    pub project_dir: Option<String>,
    /// The deferred deps for this engine (= `engineDependencies[<engine>]` from the
    /// prior `Execute` result).
    pub dependencies: Vec<TsMetadataValue>,
    pub quiet: bool,
}

// ==================== HTML dependency ====================

/// Structured HTML dependency manifest.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TsHtmlDependency {
    pub name: String,
    pub stylesheets: Vec<String>,
    pub scripts: Vec<String>,
}

// ==================== Tests ====================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{from_str, json, to_value};

    // ---------- helpers ----------

    fn make_format_identifier(ext: Option<&str>) -> TsFormatIdentifier {
        TsFormatIdentifier {
            base_format: "html".to_string(),
            target_format: "html".to_string(),
            display_name: "HTML".to_string(),
            extension_name: ext.map(String::from),
        }
    }

    fn make_format_info() -> TsFormatInfo {
        TsFormatInfo {
            identifier: make_format_identifier(None),
            metadata: HashMap::new(),
        }
    }

    fn make_execute_options() -> TsExecuteOptions {
        TsExecuteOptions {
            input: "# Hello".to_string(),
            source_path: "/project/doc.qmd".to_string(),
            format: make_format_info(),
            temp_dir: "/tmp".to_string(),
            cwd: "/project".to_string(),
            project_dir: None,
            lib_dir: "/project/doc_files".to_string(),
            quiet: false,
            handled_languages: vec![],
            params: None,
            source_map: vec![],
            dependencies: true,
        }
    }

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

    fn make_engine_project_context() -> EngineProjectContext {
        EngineProjectContext {
            project_dir: None,
            is_single_file: true,
            config: None,
            output_dir: None,
        }
    }

    // ==========================================================
    // Phase 1.5 — Correlation envelope + cooperative cancel
    // (host plan Test Seam Spec rows 1–2 + coverage smoke)
    // ==========================================================

    // --- Seam row 1: Request/Response envelope wire-shape ---
    //
    // VACUITY GUARD: assert the WIRE SHAPE (id present + tag NESTED under `msg`),
    // NOT round-trip equality — both a nested envelope and a `serde(flatten)`'d
    // one round-trip, so only the shape discriminates. Reverting `msg` to
    // `#[serde(flatten)]` hoists `type` to the top level and reddens this.

    #[test]
    fn test_request_envelope_wire_shape() {
        let r = Request {
            id: 7,
            msg: ToEngine::LoadEngine {
                engine_path: "/path/to/engine.ts".to_string(),
            },
        };
        let j = to_value(&r).unwrap();
        assert_eq!(j["id"], 7);
        assert_eq!(j["msg"]["type"], "loadEngine");
        assert_eq!(j["msg"]["enginePath"], "/path/to/engine.ts");
        // The tag MUST be nested under `msg`, never hoisted to the top level.
        assert!(
            j.get("type").is_none(),
            "msg must be nested, not flattened to the top level"
        );
    }

    #[test]
    fn test_response_envelope_wire_shape() {
        let r = Response {
            id: 42,
            msg: FromEngine::ClaimsFileResult { result: true },
        };
        let j = to_value(&r).unwrap();
        assert_eq!(j["id"], 42);
        assert_eq!(j["msg"]["type"], "claimsFileResult");
        assert!(
            j.get("type").is_none(),
            "msg must be nested, not flattened to the top level"
        );
    }

    #[test]
    fn test_request_response_envelope_round_trip() {
        let req = Request {
            id: 3,
            msg: ToEngine::Shutdown,
        };
        assert_eq!(
            req,
            from_str(&serde_json::to_string(&req).unwrap()).unwrap()
        );

        let resp = Response {
            id: 3,
            msg: FromEngine::Cancelled,
        };
        assert_eq!(
            resp,
            from_str(&serde_json::to_string(&resp).unwrap()).unwrap()
        );
    }

    // --- Seam row 2: Cancel / Cancelled tags ---

    #[test]
    fn test_to_engine_cancel_tag() {
        let v = ToEngine::Cancel { target: 9 };
        let j = to_value(&v).unwrap();
        assert_eq!(j["type"], "cancel");
        assert_eq!(j["target"], 9);
    }

    #[test]
    fn test_from_engine_cancelled_tag() {
        let v = FromEngine::Cancelled;
        let j = to_value(&v).unwrap();
        assert_eq!(j["type"], "cancelled");
        // Empty struct: the tag is the ONLY field on the wire.
        assert_eq!(j, json!({ "type": "cancelled" }));
    }

    // --- Coverage smoke (logged as coverage, NOT a discriminating row):
    // every ToEngine variant is envelope-serializable and round-trips. ---

    #[test]
    fn test_request_wraps_each_to_engine_variant() {
        let variants = vec![
            ToEngine::Init {
                global: make_host_global_config(),
            },
            ToEngine::LoadEngine {
                engine_path: "/e.ts".to_string(),
            },
            ToEngine::LaunchEngine {
                engine: "julia".to_string(),
                project: make_engine_project_context(),
            },
            ToEngine::Shutdown,
            ToEngine::ClaimsLanguage {
                engine: "julia".to_string(),
                language: "julia".to_string(),
                first_class: None,
            },
            ToEngine::ClaimsFile {
                engine: "julia".to_string(),
                file: "doc.jl".to_string(),
                ext: "jl".to_string(),
            },
            ToEngine::MarkdownForFile {
                engine: "julia".to_string(),
                file: "doc.jl".to_string(),
            },
            ToEngine::Execute {
                engine: "julia".to_string(),
                options: make_execute_options(),
            },
            ToEngine::IntermediateFiles {
                engine: "julia".to_string(),
                input: "doc.qmd".to_string(),
            },
            ToEngine::Dependencies {
                engine: "julia".to_string(),
                options: make_dependencies_options(),
            },
            ToEngine::Cancel { target: 1 },
        ];
        for (i, msg) in variants.into_iter().enumerate() {
            let r = Request {
                id: i as u64,
                msg: msg.clone(),
            };
            let back: Request = from_str(&serde_json::to_string(&r).unwrap()).unwrap();
            assert_eq!(r, back);
            let j = to_value(&r).unwrap();
            assert!(j["msg"]["type"].is_string(), "each variant carries a tag");
        }
    }

    // ==========================================================
    // Row 1 — ToEngine variant tags
    // ==========================================================

    #[test]
    fn test_to_engine_load_engine_tag() {
        let v = ToEngine::LoadEngine {
            engine_path: "/path/to/engine.ts".to_string(),
        };
        let j = to_value(&v).unwrap();
        assert_eq!(j["type"], "loadEngine");
        // secondary: round-trip
        assert_eq!(v, from_str(&j.to_string()).unwrap());
    }

    #[test]
    fn test_to_engine_launch_engine_tag() {
        let v = ToEngine::LaunchEngine {
            engine: "julia".to_string(),
            project: EngineProjectContext {
                project_dir: None,
                is_single_file: true,
                config: None,
                output_dir: None,
            },
        };
        let j = to_value(&v).unwrap();
        assert_eq!(j["type"], "launchEngine");
    }

    #[test]
    fn test_to_engine_shutdown_tag() {
        let v = ToEngine::Shutdown;
        let j = to_value(&v).unwrap();
        assert_eq!(j["type"], "shutdown");
    }

    #[test]
    fn test_to_engine_claims_language_tag() {
        let v = ToEngine::ClaimsLanguage {
            engine: "julia".to_string(),
            language: "julia".to_string(),
            first_class: None,
        };
        let j = to_value(&v).unwrap();
        assert_eq!(j["type"], "claimsLanguage");
    }

    #[test]
    fn test_to_engine_claims_file_tag() {
        let v = ToEngine::ClaimsFile {
            engine: "julia".to_string(),
            file: "doc.jl".to_string(),
            ext: "jl".to_string(),
        };
        let j = to_value(&v).unwrap();
        assert_eq!(j["type"], "claimsFile");
    }

    #[test]
    fn test_to_engine_markdown_for_file_tag() {
        let v = ToEngine::MarkdownForFile {
            engine: "julia".to_string(),
            file: "doc.jl".to_string(),
        };
        let j = to_value(&v).unwrap();
        assert_eq!(j["type"], "markdownForFile");
    }

    #[test]
    fn test_to_engine_execute_tag() {
        let v = ToEngine::Execute {
            engine: "julia".to_string(),
            options: make_execute_options(),
        };
        let j = to_value(&v).unwrap();
        assert_eq!(j["type"], "execute");
    }

    #[test]
    fn test_to_engine_intermediate_files_tag() {
        let v = ToEngine::IntermediateFiles {
            engine: "julia".to_string(),
            input: "doc.qmd".to_string(),
        };
        let j = to_value(&v).unwrap();
        assert_eq!(j["type"], "intermediateFiles");
    }

    // ==========================================================
    // Row 2 — camelCase fields: enginePath, firstClass
    // ==========================================================

    #[test]
    fn test_to_engine_load_engine_camel_case_engine_path() {
        let v = ToEngine::LoadEngine {
            engine_path: "/path/to/engine.ts".to_string(),
        };
        let j = to_value(&v).unwrap();
        // "enginePath" must be present (snake_case "engine_path" would be rejected)
        assert_eq!(j["enginePath"], "/path/to/engine.ts");
        assert!(
            j.get("engine_path").is_none(),
            "snake_case key must not appear"
        );
    }

    #[test]
    fn test_to_engine_claims_language_camel_case_first_class() {
        let v = ToEngine::ClaimsLanguage {
            engine: "julia".to_string(),
            language: "julia".to_string(),
            first_class: Some("primary".to_string()),
        };
        let j = to_value(&v).unwrap();
        assert_eq!(j["firstClass"], "primary");
        assert!(
            j.get("first_class").is_none(),
            "snake_case key must not appear"
        );
    }

    // ==========================================================
    // Row 3 — FromEngine variant tags
    // ==========================================================

    #[test]
    fn test_from_engine_loaded_tag() {
        let v = FromEngine::Loaded {
            discovery: LoadEngineResult {
                name: "julia".to_string(),
                valid_extensions: vec!["jl".to_string()],
                generates_figures: false,
                can_freeze: false,
                quarto_required: None,
            },
        };
        let j = to_value(&v).unwrap();
        assert_eq!(j["type"], "loaded");
    }

    #[test]
    fn test_from_engine_launched_tag() {
        let v = FromEngine::Launched {
            instance: LaunchEngineResult { can_freeze: true },
        };
        let j = to_value(&v).unwrap();
        assert_eq!(j["type"], "launched");
    }

    #[test]
    fn test_from_engine_error_tag() {
        let v = FromEngine::Error {
            message: "oops".to_string(),
            stack: None,
        };
        let j = to_value(&v).unwrap();
        assert_eq!(j["type"], "error");
    }

    #[test]
    fn test_from_engine_claims_language_result_tag() {
        let v = FromEngine::ClaimsLanguageResult { result: None };
        let j = to_value(&v).unwrap();
        assert_eq!(j["type"], "claimsLanguageResult");
    }

    #[test]
    fn test_from_engine_claims_file_result_tag() {
        let v = FromEngine::ClaimsFileResult { result: true };
        let j = to_value(&v).unwrap();
        assert_eq!(j["type"], "claimsFileResult");
    }

    #[test]
    fn test_from_engine_markdown_for_file_result_tag() {
        let v = FromEngine::MarkdownForFileResult {
            result: TsMappedStringWithMap {
                value: "# Hi".to_string(),
                file_name: None,
                source_map: vec![],
            },
        };
        let j = to_value(&v).unwrap();
        assert_eq!(j["type"], "markdownForFileResult");
    }

    #[test]
    fn test_from_engine_execute_result_tag() {
        let v = FromEngine::ExecuteResult {
            result: TsExecuteResult {
                markdown: String::new(),
                ..Default::default()
            },
        };
        let j = to_value(&v).unwrap();
        assert_eq!(j["type"], "executeResult");
    }

    #[test]
    fn test_from_engine_intermediate_files_result_tag() {
        let v = FromEngine::IntermediateFilesResult { result: None };
        let j = to_value(&v).unwrap();
        assert_eq!(j["type"], "intermediateFilesResult");
    }

    // ==========================================================
    // Row 4 — camelCase: validExtensions, canFreeze, generatesFigures
    // ==========================================================

    #[test]
    fn test_load_engine_result_camel_case_valid_extensions() {
        let v = LoadEngineResult {
            name: "julia".to_string(),
            valid_extensions: vec!["jl".to_string()],
            generates_figures: false,
            can_freeze: false,
            quarto_required: None,
        };
        let j = to_value(&v).unwrap();
        assert_eq!(j["validExtensions"], json!(["jl"]));
        assert!(j.get("valid_extensions").is_none());
    }

    /// `LaunchEngineResult` keeps `canFreeze` but `generatesFigures` moved to
    /// `LoadEngineResult` (the discovery tier). This test guards both invariants.
    #[test]
    fn test_launch_engine_result_camel_case_can_freeze() {
        let v = LaunchEngineResult { can_freeze: true };
        let j = to_value(&v).unwrap();
        assert_eq!(j["canFreeze"], true);
        assert!(
            j.get("can_freeze").is_none(),
            "snake_case key must not appear"
        );
        assert!(
            j.get("generatesFigures").is_none(),
            "generatesFigures must be ABSENT from LaunchEngineResult (moved to discovery tier)"
        );
    }

    // ==========================================================
    // E1 — discovery tier completeness: generates_figures + can_freeze +
    //       quarto_required on LoadEngineResult; generatesFigures ABSENT on
    //       LaunchEngineResult.
    // ==========================================================

    #[test]
    fn test_discovery_tier_fields_present_and_launch_tier_generates_figures_absent() {
        // (a) LoadEngineResult carries generates_figures, can_freeze, and quarto_required.
        let loaded = FromEngine::Loaded {
            discovery: LoadEngineResult {
                name: "julia".to_string(),
                valid_extensions: vec!["jl".to_string()],
                generates_figures: true,
                can_freeze: true,
                quarto_required: Some("1.4.0".to_string()),
            },
        };
        let jl = to_value(&loaded).unwrap();
        assert_eq!(
            jl["discovery"]["generatesFigures"], true,
            "generatesFigures must be present on LoadEngineResult"
        );
        assert_eq!(
            jl["discovery"]["canFreeze"], true,
            "canFreeze must be present on LoadEngineResult"
        );
        assert_eq!(
            jl["discovery"]["quartoRequired"], "1.4.0",
            "quartoRequired must be present on LoadEngineResult"
        );

        // (b) LaunchEngineResult keeps canFreeze but MUST NOT carry generatesFigures.
        let launched = FromEngine::Launched {
            instance: LaunchEngineResult { can_freeze: true },
        };
        let jn = to_value(&launched).unwrap();
        assert_eq!(
            jn["instance"]["canFreeze"], true,
            "canFreeze must still be present on LaunchEngineResult"
        );
        assert!(
            jn["instance"].get("generatesFigures").is_none(),
            "generatesFigures must be ABSENT on LaunchEngineResult (it moved to discovery)"
        );
    }

    // ==========================================================
    // Row 5 — TsLanguageClaim tagged shape + Option None = null
    // ==========================================================

    #[test]
    fn test_ts_language_claim_primary_shape() {
        let v = TsLanguageClaim::Primary { priority: 5 };
        assert_eq!(
            to_value(&v).unwrap(),
            json!({"kind": "primary", "priority": 5})
        );
    }

    #[test]
    fn test_ts_language_claim_interop_shape() {
        let v = TsLanguageClaim::Interop { priority: 0 };
        assert_eq!(
            to_value(&v).unwrap(),
            json!({"kind": "interop", "priority": 0})
        );
    }

    #[test]
    fn test_ts_language_claim_fallback_shape() {
        let v = TsLanguageClaim::Fallback { priority: -1 };
        assert_eq!(
            to_value(&v).unwrap(),
            json!({"kind": "fallback", "priority": -1})
        );
    }

    #[test]
    fn test_ts_language_claim_option_none_is_json_null() {
        let v: Option<TsLanguageClaim> = None;
        assert_eq!(to_value(&v).unwrap(), json!(null));
    }

    // ==========================================================
    // Row 6 — HostGlobalConfig / EngineProjectContext camelCase keys
    // ==========================================================

    #[test]
    fn test_host_global_config_camel_case_keys() {
        let v = make_host_global_config();
        let j = to_value(&v).unwrap();
        assert_eq!(j["resourceDir"], "/res");
        assert_eq!(j["runtimeDir"], "/rt");
        assert_eq!(j["dataDir"], "/data");
        assert_eq!(j["isInteractiveSession"], false);
        assert_eq!(j["runningInCi"], false);
        assert_eq!(j["quartoVersion"], "0.1.0");
        assert!(j.get("resource_dir").is_none());
        assert!(j.get("runtime_dir").is_none());
        assert!(j.get("data_dir").is_none());
        // round-trip
        let back: HostGlobalConfig = from_str(&serde_json::to_string(&v).unwrap()).unwrap();
        assert_eq!(v, back);
    }

    #[test]
    fn test_engine_project_context_camel_case_keys() {
        let v = EngineProjectContext {
            project_dir: None,
            is_single_file: true,
            config: None,
            output_dir: None,
        };
        let j = to_value(&v).unwrap();
        assert_eq!(j["isSingleFile"], true);
        // projectDir None → null (present, not absent)
        assert_eq!(j["projectDir"], json!(null));
        assert!(j.get("is_single_file").is_none());
        assert!(j.get("project_dir").is_none());
        // round-trip
        let back: EngineProjectContext = from_str(&serde_json::to_string(&v).unwrap()).unwrap();
        assert_eq!(v, back);
    }

    // ==========================================================
    // Row 7 — TsFormatIdentifier kebab keys; extension-name absent when None
    // ==========================================================

    #[test]
    fn test_ts_format_identifier_kebab_keys_no_extension() {
        let v = make_format_identifier(None);
        let j = to_value(&v).unwrap();
        assert_eq!(j["base-format"], "html");
        assert_eq!(j["target-format"], "html");
        assert_eq!(j["display-name"], "HTML");
        // extension-name ABSENT when None
        assert!(
            j.get("extension-name").is_none(),
            "extension-name must be absent when None"
        );
        assert!(j.get("extension_name").is_none());
    }

    #[test]
    fn test_ts_format_identifier_extension_name_present_when_some() {
        let v = make_format_identifier(Some("acm"));
        let j = to_value(&v).unwrap();
        assert_eq!(j["extension-name"], "acm");
    }

    // ==========================================================
    // Row 8 — TsMetadataValue untagged (bare values)
    // ==========================================================

    #[test]
    fn test_ts_metadata_value_string_bare() {
        let v = TsMetadataValue::String("x".to_string());
        assert_eq!(to_value(&v).unwrap(), json!("x"));
    }

    #[test]
    fn test_ts_metadata_value_bool_bare() {
        assert_eq!(to_value(TsMetadataValue::Bool(true)).unwrap(), json!(true));
    }

    #[test]
    fn test_ts_metadata_value_number_bare() {
        // Use 2.5 — not an approximate of any well-known constant (avoids clippy::approx_constant)
        assert_eq!(to_value(TsMetadataValue::Number(2.5)).unwrap(), json!(2.5));
    }

    #[test]
    fn test_ts_metadata_value_array_bare() {
        let v = TsMetadataValue::Array(vec![
            TsMetadataValue::Number(1.0),
            TsMetadataValue::String("a".to_string()),
        ]);
        assert_eq!(to_value(&v).unwrap(), json!([1.0, "a"]));
    }

    #[test]
    fn test_ts_metadata_value_map_bare() {
        let mut map = HashMap::new();
        map.insert("k".to_string(), TsMetadataValue::Bool(false));
        let v = TsMetadataValue::Map(map);
        assert_eq!(to_value(&v).unwrap(), json!({"k": false}));
    }

    #[test]
    fn test_ts_metadata_value_null_bare() {
        assert_eq!(to_value(TsMetadataValue::Null).unwrap(), json!(null));
    }

    // ==========================================================
    // Row 9 — TsSourceMapEntry / TsSourcePosition camelCase
    // ==========================================================

    #[test]
    fn test_ts_source_map_entry_camel_case() {
        let v = TsSourceMapEntry {
            start: 10,
            length: 5,
            source: Some(TsSourcePosition {
                file: "/project/doc.qmd".to_string(),
                file_offset: 42,
            }),
        };
        let j = to_value(&v).unwrap();
        assert_eq!(j["start"], 10);
        assert_eq!(j["length"], 5);
        assert_eq!(j["source"]["file"], "/project/doc.qmd");
        assert_eq!(j["source"]["fileOffset"], 42);
        assert!(j["source"].get("file_offset").is_none());
    }

    #[test]
    fn test_ts_source_map_entry_source_none_is_null() {
        let v = TsSourceMapEntry {
            start: 0,
            length: 3,
            source: None,
        };
        let j = to_value(&v).unwrap();
        assert_eq!(j["source"], json!(null));
    }

    // ==========================================================
    // Row 10 — TsMappedStringWithMap camelCase
    // ==========================================================

    #[test]
    fn test_ts_mapped_string_with_map_camel_case() {
        let v = TsMappedStringWithMap {
            value: "# Hello".to_string(),
            file_name: None,
            source_map: vec![],
        };
        let j = to_value(&v).unwrap();
        assert_eq!(j["value"], "# Hello");
        assert_eq!(j["fileName"], json!(null));
        assert_eq!(j["sourceMap"], json!([]));
        assert!(j.get("file_name").is_none());
        assert!(j.get("source_map").is_none());
    }

    // ==========================================================
    // Row 11 — TsPandocIncludes camelCase
    // ==========================================================

    #[test]
    fn test_ts_pandoc_includes_camel_case() {
        let v = TsPandocIncludes {
            in_header: Some(vec!["<style>".to_string()]),
            before_body: None,
            after_body: None,
        };
        let j = to_value(&v).unwrap();
        assert_eq!(j["inHeader"], json!(["<style>"]));
        assert_eq!(j["beforeBody"], json!(null));
        assert_eq!(j["afterBody"], json!(null));
        assert!(j.get("in_header").is_none());
    }

    // ==========================================================
    // Row 12 — TsPandocAttr: duplicate keyvalue pairs preserved
    // ==========================================================

    #[test]
    fn test_ts_pandoc_attr_duplicate_keyvalue_preserved() {
        let v = TsPandocAttr {
            id: "myid".to_string(),
            classes: vec![],
            keyvalue: vec![
                ("a".to_string(), "1".to_string()),
                ("a".to_string(), "2".to_string()),
            ],
        };
        // Serialize → deserialize → both pairs survive in order
        let serialized = serde_json::to_string(&v).unwrap();
        let deserialized: TsPandocAttr = from_str(&serialized).unwrap();
        assert_eq!(deserialized.keyvalue.len(), 2);
        assert_eq!(deserialized.keyvalue[0], ("a".to_string(), "1".to_string()));
        assert_eq!(deserialized.keyvalue[1], ("a".to_string(), "2".to_string()));
        // Shape: keyvalue is an array of 2-element arrays on the wire
        let j = to_value(&v).unwrap();
        assert_eq!(j["keyvalue"], json!([["a", "1"], ["a", "2"]]));
    }

    // ==========================================================
    // Row 13 — TsExecuteOptions camelCase keys
    // ==========================================================

    #[test]
    fn test_ts_execute_options_camel_case() {
        let v = make_execute_options();
        let j = to_value(&v).unwrap();
        assert_eq!(j["sourcePath"], "/project/doc.qmd");
        assert_eq!(j["tempDir"], "/tmp");
        assert_eq!(j["libDir"], "/project/doc_files");
        assert_eq!(j["handledLanguages"], json!([]));
        assert_eq!(j["sourceMap"], json!([]));
        // format is a nested object
        assert!(j["format"].is_object());
        assert!(j.get("source_path").is_none());
        assert!(j.get("temp_dir").is_none());
    }

    // ==========================================================
    // Row 14 — TsExecuteResult: htmlDependencies; includes None/Some
    // ==========================================================

    #[test]
    fn test_ts_execute_result_html_dependencies_key() {
        let v = TsExecuteResult {
            markdown: "# Done".to_string(),
            html_dependencies: vec![TsHtmlDependency {
                name: "mylib".to_string(),
                stylesheets: vec!["/path/style.css".to_string()],
                scripts: vec![],
            }],
            ..Default::default()
        };
        let j = to_value(&v).unwrap();
        assert_eq!(j["htmlDependencies"][0]["name"], "mylib");
        assert!(j.get("html_dependencies").is_none());
        assert_eq!(j["includes"], json!(null));
    }

    #[test]
    fn test_ts_execute_result_includes_some_round_trip() {
        let v = TsExecuteResult {
            includes: Some(TsPandocIncludes {
                in_header: Some(vec!["<style>".to_string()]),
                before_body: None,
                after_body: None,
            }),
            ..Default::default()
        };
        let serialized = serde_json::to_string(&v).unwrap();
        let deserialized: TsExecuteResult = from_str(&serialized).unwrap();
        assert_eq!(v, deserialized);
        let j = to_value(&v).unwrap();
        assert_eq!(j["includes"]["inHeader"], json!(["<style>"]));
    }

    // ==========================================================
    // FC-1: TsExecuteResult new carrier fields wire-shape
    //
    // VACUITY GUARDS:
    //   (a) re-hardcode `needs_postprocess: false` in `map_execute_result` → the
    //       mapping test's `needs_postprocess == true` assertion REDS.
    //   (b) add `#[serde(skip)]` to `pandoc` in TsExecuteResult →
    //       the `j["pandoc"].is_some()` assertion REDS.
    // ==========================================================

    #[test]
    fn test_ts_execute_result_new_carrier_fields_wire_shape() {
        let mut metadata = HashMap::new();
        metadata.insert(
            "author".to_string(),
            TsMetadataValue::String("Alice".to_string()),
        );
        let mut pandoc_map = HashMap::new();
        pandoc_map.insert("fig-width".to_string(), TsMetadataValue::Number(6.0));
        let mut preserve = HashMap::new();
        preserve.insert("key1".to_string(), "val1".to_string());

        let v = TsExecuteResult {
            metadata: Some(metadata),
            pandoc: Some(pandoc_map),
            resource_files: vec!["fig.png".to_string()],
            preserve,
            post_process: true,
            ..Default::default()
        };

        let j = to_value(&v).unwrap();
        assert!(
            j.get("metadata").is_some(),
            "metadata must be present on the wire"
        );
        assert!(
            j.get("pandoc").is_some(),
            "pandoc must be present on the wire"
        );
        assert!(
            j.get("resourceFiles").is_some(),
            "resourceFiles must be present on the wire (camelCase)"
        );
        assert!(
            j.get("preserve").is_some(),
            "preserve must be present on the wire"
        );
        assert_eq!(
            j["postProcess"], true,
            "postProcess must be wire-fed from post_process"
        );
        // Confirm camelCase rename is applied (no snake_case leakage).
        assert!(
            j.get("resource_files").is_none(),
            "resource_files (snake_case) must NOT appear on the wire"
        );
        assert!(
            j.get("post_process").is_none(),
            "post_process (snake_case) must NOT appear on the wire"
        );
    }

    // ==========================================================
    // Row 15 — TsHtmlDependency shape (single-word keys)
    // ==========================================================

    #[test]
    fn test_ts_html_dependency_shape() {
        let v = TsHtmlDependency {
            name: "mathjax".to_string(),
            stylesheets: vec!["/abs/style.css".to_string()],
            scripts: vec!["/abs/main.js".to_string()],
        };
        let j = to_value(&v).unwrap();
        assert_eq!(j["name"], "mathjax");
        assert_eq!(j["stylesheets"], json!(["/abs/style.css"]));
        assert_eq!(j["scripts"], json!(["/abs/main.js"]));
    }

    // ==========================================================
    // T-A3 — Init/LaunchEngine field-placement round-trip
    // (RTQ Item A protocol split: HostGlobalConfig on Init,
    //  EngineProjectContext on LaunchEngine)
    //
    // Named revert (must produce RED): move `project` onto `Init`
    // (or `global` onto `LaunchEngine`) → field-placement assertions fail.
    // ==========================================================

    #[test]
    fn test_init_and_launch_engine_protocol_split() {
        // --- Init: carries global config, not project ---
        let global = HostGlobalConfig {
            resource_dir: "/res".to_string(),
            runtime_dir: "/rt".to_string(),
            data_dir: "/data".to_string(),
            pandoc_path: None,
            is_interactive_session: false,
            running_in_ci: false,
            quarto_version: "0.7.0".to_string(),
        };
        let init = ToEngine::Init {
            global: global.clone(),
        };
        let j_init = to_value(&init).unwrap();
        // type tag
        assert_eq!(j_init["type"], "init");
        // camelCase keys present on global object
        assert_eq!(j_init["global"]["resourceDir"], "/res");
        assert_eq!(j_init["global"]["runtimeDir"], "/rt");
        assert_eq!(j_init["global"]["dataDir"], "/data");
        assert_eq!(j_init["global"]["quartoVersion"], "0.7.0");
        // global must be a nested object
        assert!(
            j_init["global"].is_object(),
            "`global` must be a nested object on Init; got: {j_init}"
        );
        // round-trip
        let back: ToEngine = from_str(&serde_json::to_string(&init).unwrap()).unwrap();
        assert_eq!(init, back);

        // --- LaunchEngine: carries project context, not global config ---
        let project = EngineProjectContext {
            project_dir: Some("/proj".to_string()),
            is_single_file: true,
            config: None,
            output_dir: None,
        };
        let launch = ToEngine::LaunchEngine {
            engine: "julia".to_string(),
            project: project.clone(),
        };
        let j_launch = to_value(&launch).unwrap();
        // type tag
        assert_eq!(j_launch["type"], "launchEngine");
        // camelCase keys present on project object
        assert_eq!(j_launch["project"]["projectDir"], "/proj");
        assert_eq!(j_launch["project"]["isSingleFile"], true);
        // project must be a nested object
        assert!(
            j_launch["project"].is_object(),
            "`project` must be a nested object on LaunchEngine; got: {j_launch}"
        );
        // round-trip
        let back2: ToEngine = from_str(&serde_json::to_string(&launch).unwrap()).unwrap();
        assert_eq!(launch, back2);

        // --- Field-placement bind (named revert target) ---
        // `project` must NOT appear on Init
        assert!(
            j_init.get("project").is_none(),
            "`project` must NOT ride the Init frame; got: {j_init}"
        );
        // `global` must NOT appear on LaunchEngine
        assert!(
            j_launch.get("global").is_none(),
            "`global` must NOT ride the LaunchEngine frame; got: {j_launch}"
        );
        // `context` (old field name) must NOT appear on LaunchEngine
        assert!(
            j_launch.get("context").is_none(),
            "`context` must NOT appear on LaunchEngine frame; got: {j_launch}"
        );
    }

    // ==========================================================
    // F2a — FC-2: deferred-dependencies wire surface
    //
    // Three assertions bound by one named revert:
    //   flip `default_true` to return `false`
    //   → sub-test (1) "absent key → true" flips RED.
    // ==========================================================

    fn make_dependencies_options() -> TsDependenciesOptions {
        TsDependenciesOptions {
            input: "# Hello".to_string(),
            source_path: "/project/doc.qmd".to_string(),
            source_map: vec![],
            format: make_format_info(),
            output: "/project/doc.html".to_string(),
            temp_dir: "/tmp".to_string(),
            lib_dir: None,
            project_dir: None,
            dependencies: vec![],
            quiet: false,
        }
    }

    /// (1) Absent `dependencies` key in JSON → deserialized value must be `true`
    ///     (binds the `default_true` custom serde default).
    ///
    /// Named revert: change `default_true` to return `false` → this assertion REDS.
    #[test]
    fn test_fc2_execute_options_dependencies_default_true() {
        // Build a minimal TsExecuteOptions JSON with NO `dependencies` key.
        let j = json!({
            "input": "# Hi",
            "sourcePath": "/project/doc.qmd",
            "format": {
                "identifier": {
                    "base-format": "html",
                    "target-format": "html",
                    "display-name": "HTML"
                },
                "metadata": {}
            },
            "tempDir": "/tmp",
            "cwd": "/project",
            "projectDir": null,
            "libDir": "/project/doc_files",
            "quiet": false,
            "handledLanguages": [],
            "params": null,
            "sourceMap": []
        });
        let opts: TsExecuteOptions =
            serde_json::from_value(j).expect("TsExecuteOptions must deserialize");
        assert!(
            opts.dependencies,
            "absent `dependencies` key must default to true (named revert: change \
             default_true to return false → this assertion flips RED)"
        );
    }

    /// (2) Round-trip `ToEngine::Dependencies` and `FromEngine::DependenciesResult`.
    #[test]
    fn test_fc2_dependencies_verb_round_trip() {
        let to = ToEngine::Dependencies {
            engine: "julia".to_string(),
            options: make_dependencies_options(),
        };
        let j = to_value(&to).unwrap();
        assert_eq!(
            j["type"], "dependencies",
            "Dependencies variant must have tag 'dependencies'"
        );
        assert_eq!(j["engine"], "julia");
        let back: ToEngine = serde_json::from_value(j).expect("Dependencies must round-trip");
        assert_eq!(to, back);

        let from = FromEngine::DependenciesResult {
            includes: TsPandocIncludes {
                in_header: Some(vec!["<style>".to_string()]),
                before_body: None,
                after_body: None,
            },
        };
        let jf = to_value(&from).unwrap();
        assert_eq!(
            jf["type"], "dependenciesResult",
            "DependenciesResult variant must have tag 'dependenciesResult'"
        );
        let back_from: FromEngine =
            serde_json::from_value(jf).expect("DependenciesResult must round-trip");
        assert_eq!(from, back_from);
    }

    /// (3) `TsExecuteResult` with `engine_dependencies: Some(…)` → wire key
    ///     `engineDependencies` must be present.
    #[test]
    fn test_fc2_execute_result_engine_dependencies_present() {
        let mut deps = HashMap::new();
        deps.insert(
            "julia".to_string(),
            vec![TsMetadataValue::String("dep-a".to_string())],
        );
        let v = TsExecuteResult {
            engine_dependencies: Some(deps),
            ..Default::default()
        };
        let j = to_value(&v).unwrap();
        assert!(
            j.get("engineDependencies").is_some(),
            "engineDependencies must appear on the wire when Some"
        );
        assert_eq!(
            j["engineDependencies"]["julia"],
            json!(["dep-a"]),
            "engineDependencies value must round-trip"
        );
    }

    // ==========================================================
    // Row 16 — Error tests
    // ==========================================================

    #[test]
    fn test_error_malformed_json() {
        // 16a — malformed JSON → Err
        let result = from_str::<ToEngine>("{ not json");
        assert!(result.is_err());
    }

    #[test]
    fn test_error_unknown_type_tag() {
        // 16b — unknown type tag → Err
        let result = from_str::<ToEngine>(r#"{"type": "bogus", "engine": "julia"}"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_error_missing_required_field() {
        // 16c — missing required field enginePath → Err
        let result = from_str::<ToEngine>(r#"{"type": "loadEngine"}"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_error_wrong_field_type() {
        // 16d — wrong type (string where bool expected for canFreeze) → Err
        let result = from_str::<LaunchEngineResult>(r#"{"canFreeze": "yes"}"#);
        assert!(result.is_err());
    }

    // ==========================================================
    // T-Gate-parity — TS↔Rust wire-dual parity (Plan 2 Phase B gate)
    //
    // tsc proves TS-internal coherence only; it cannot see a Rust rename of a
    // dual-defined wire field, and TS interface keys are erased at runtime. So
    // we mechanize a TWO-SIDED parity check for the four wire-dual types:
    //
    //   Rust side (here): serialize one canonical instance of each wire-dual
    //     type to JSON via serde and compare to a committed fixture. The fixture
    //     is a real serde ARTIFACT — NOT a hand-maintained key list — so a Rust
    //     wire-key rename actually changes the fixture's key-set (defeating the
    //     "two hand lists survive their own revert" Dv/Dv collapse trap).
    //
    //   Deno side (src/wire-parity.deno-test.ts): reads the same fixture and
    //     set-equates each instance's `Object.keys(...)` against a `KEYS` list
    //     pinned to the TS wire type via `satisfies readonly (keyof T)[]` + an
    //     `_Exhaustive` compile guard. Symmetric set-equality binds Rust shape
    //     (fixture), TS shape (type), and the runtime KEYS list together.
    //
    // The fixture lives at `tests/fixtures/ts_wire_parity.json` (relative to
    // this crate). This test WRITES it (under regen); the deno test READS it.
    //
    // Idiomatic invocation (schema_drift.rs-style):
    //
    //   # Verify (default — what CI does):
    //   cargo nextest run -p quarto-core -E \
    //     'test(ts_protocol::tests::test_ts_wire_parity_fixture)'
    //
    //   # Regenerate after a wire-shape change to any of the four types:
    //   QUARTO_REGEN_WIRE_FIXTURES=1 cargo nextest run -p quarto-core -E \
    //     'test(ts_protocol::tests::test_ts_wire_parity_fixture)'
    //
    // VACUITY GUARD: each canonical instance populates EVERY optional field.
    // Note: the four wire-dual structs here carry bare `Option<T>` fields with
    // NO `#[serde(skip_serializing_if)]`, so serde serializes `None` as
    // `"key": null` — the key IS retained. Populating all optionals with
    // `Some(...)` is therefore belt-and-suspenders (the key-set gate would not
    // drop a `None` key anyway), but it keeps the fixture representative and
    // makes the intent of each field visible at a glance.
    // ==========================================================

    /// Recursively sort the keys of every JSON object so the committed fixture
    /// is stable regardless of the workspace-wide `serde_json/preserve_order`
    /// feature (see schema_drift.rs for the same rationale). Key ORDER does not
    /// matter to the deno set-equality, but a stable file avoids spurious diffs.
    fn canonicalize_keys(value: serde_json::Value) -> serde_json::Value {
        use serde_json::Value;
        match value {
            Value::Object(map) => {
                let mut entries: Vec<(String, Value)> = map.into_iter().collect();
                entries.sort_by(|a, b| a.0.cmp(&b.0));
                let mut out = serde_json::Map::new();
                for (k, v) in entries {
                    out.insert(k, canonicalize_keys(v));
                }
                Value::Object(out)
            }
            Value::Array(arr) => Value::Array(arr.into_iter().map(canonicalize_keys).collect()),
            other => other,
        }
    }

    /// Build the canonical wire-dual instances, one per type, ALL optionals
    /// populated. Returns the pretty-printed JSON object (type name → instance)
    /// with a trailing newline, keys canonically sorted.
    fn render_wire_parity_fixture() -> String {
        let host_global = HostGlobalConfig {
            resource_dir: "/res".to_string(),
            runtime_dir: "/rt".to_string(),
            data_dir: "/data".to_string(),
            pandoc_path: Some("/usr/bin/pandoc".to_string()), // optional → Some
            is_interactive_session: false,
            running_in_ci: true,
            quarto_version: "0.7.0".to_string(),
        };

        let pandoc_includes = TsPandocIncludes {
            in_header: Some(vec!["<style></style>".to_string()]), // optional → Some
            before_body: Some(vec!["<header/>".to_string()]),     // optional → Some
            after_body: Some(vec!["<footer/>".to_string()]),      // optional → Some
        };

        let mut config = HashMap::new();
        config.insert(
            "output-dir".to_string(),
            TsMetadataValue::String("_out".to_string()),
        );
        let project_ctx = EngineProjectContext {
            project_dir: Some("/proj".to_string()), // optional → Some
            is_single_file: false,
            config: Some(config),                       // optional → Some
            output_dir: Some("/proj/_out".to_string()), // optional → Some
        };

        // Wire claim: required `priority`, no optionals. One variant suffices —
        // all three variants share the same key-set {kind, priority}.
        let language_claim = TsLanguageClaim::Primary { priority: 5 };

        let obj = serde_json::json!({
            "HostGlobalConfig": to_value(&host_global).unwrap(),
            "TsPandocIncludes": to_value(&pandoc_includes).unwrap(),
            "EngineProjectContext": to_value(&project_ctx).unwrap(),
            "TsLanguageClaim": to_value(&language_claim).unwrap(),
        });

        let canonical = canonicalize_keys(obj);
        let mut s = serde_json::to_string_pretty(&canonical).expect("fixture must serialize");
        s.push('\n');
        s
    }

    #[test]
    fn test_ts_wire_parity_fixture() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("ts_wire_parity.json");
        let generated = render_wire_parity_fixture();
        let regen = std::env::var("QUARTO_REGEN_WIRE_FIXTURES").is_ok_and(|v| v == "1");

        let existing = std::fs::read_to_string(&path).ok();
        if existing.as_deref() == Some(&generated) {
            return;
        }

        if regen {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("create tests/fixtures/ dir");
            }
            std::fs::write(&path, &generated)
                .unwrap_or_else(|e| panic!("failed to write fixture to {}: {}", path.display(), e));
            eprintln!("regenerated {}", path.display());
            return;
        }

        let existing_summary = match &existing {
            None => "(no file on disk)".to_string(),
            Some(s) => format!("({} bytes)", s.len()),
        };
        panic!(
            "TS↔Rust wire-parity fixture drift.\n\
             The checked-in fixture {} {} does not match the JSON serialized from\n\
             the current wire-dual types in this file. This fixture is the Rust\n\
             half of the T-Gate-parity check (the deno half is\n\
             src/wire-parity.deno-test.ts). To regenerate, run:\n\
             \n\
             \tQUARTO_REGEN_WIRE_FIXTURES=1 cargo nextest run -p quarto-core -E \\\n\
             \t  'test(ts_protocol::tests::test_ts_wire_parity_fixture)'\n\
             \n\
             Then review the diff AND update the matching TS type in\n\
             ts-packages/quarto-engine-host-deno/src/types.ts before committing.",
            path.display(),
            existing_summary,
        );
    }
}
