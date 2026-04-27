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
    /// Run `import(enginePath)` and construct the `ExecutionEngineDiscovery` object.
    #[serde(rename = "loadEngine")]
    LoadEngine { engine_path: String },

    /// Call `engine.launch(context)` and track the resulting instance.
    #[serde(rename = "launchEngine")]
    LaunchEngine {
        engine: String,
        context: EngineHostContext,
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
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LoadEngineResult {
    pub name: String,
    pub valid_extensions: Vec<String>,
}

/// Response to `LaunchEngine` — instance metadata available after `launch()`.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LaunchEngineResult {
    pub can_freeze: bool,
    pub generates_figures: bool,
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

// ==================== Engine host context ====================

/// Context sent with each `LaunchEngine`.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EngineHostContext {
    pub project_dir: Option<String>,
    pub is_single_file: bool,
    pub resource_dir: String,
    pub runtime_dir: String,
    pub pandoc_path: Option<String>,
    pub is_interactive_session: bool,
    pub running_in_ci: bool,
    pub quarto_version: String,
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
    pub handled_languages: Vec<String>,
    pub params: Option<HashMap<String, TsMetadataValue>>,
    pub source_map: Vec<TsSourceMapEntry>,
}

// ==================== Execute result ====================

/// Result returned by the engine after execution.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TsExecuteResult {
    pub markdown: String,
    pub supporting: Vec<String>,
    pub filters: Vec<String>,
    pub includes: Option<TsPandocIncludes>,
    pub html_dependencies: Vec<TsHtmlDependency>,
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
        }
    }

    fn make_host_context() -> EngineHostContext {
        EngineHostContext {
            project_dir: None,
            is_single_file: true,
            resource_dir: "/res".to_string(),
            runtime_dir: "/rt".to_string(),
            pandoc_path: None,
            is_interactive_session: false,
            running_in_ci: false,
            quarto_version: "0.1.0".to_string(),
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
            ToEngine::LoadEngine {
                engine_path: "/e.ts".to_string(),
            },
            ToEngine::LaunchEngine {
                engine: "julia".to_string(),
                context: make_host_context(),
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
            context: EngineHostContext {
                project_dir: None,
                is_single_file: true,
                resource_dir: "/res".to_string(),
                runtime_dir: "/rt".to_string(),
                pandoc_path: None,
                is_interactive_session: false,
                running_in_ci: false,
                quarto_version: "0.1.0".to_string(),
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
            },
        };
        let j = to_value(&v).unwrap();
        assert_eq!(j["type"], "loaded");
    }

    #[test]
    fn test_from_engine_launched_tag() {
        let v = FromEngine::Launched {
            instance: LaunchEngineResult {
                can_freeze: true,
                generates_figures: false,
            },
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
                supporting: vec![],
                filters: vec![],
                includes: None,
                html_dependencies: vec![],
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
        };
        let j = to_value(&v).unwrap();
        assert_eq!(j["validExtensions"], json!(["jl"]));
        assert!(j.get("valid_extensions").is_none());
    }

    #[test]
    fn test_launch_engine_result_camel_case_can_freeze_generates_figures() {
        let v = LaunchEngineResult {
            can_freeze: true,
            generates_figures: false,
        };
        let j = to_value(&v).unwrap();
        assert_eq!(j["canFreeze"], true);
        assert_eq!(j["generatesFigures"], false);
        assert!(j.get("can_freeze").is_none());
        assert!(j.get("generates_figures").is_none());
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
    // Row 6 — EngineHostContext camelCase; projectDir None = null (not absent)
    // ==========================================================

    #[test]
    fn test_engine_host_context_camel_case_keys() {
        let v = EngineHostContext {
            project_dir: None,
            is_single_file: true,
            resource_dir: "/res".to_string(),
            runtime_dir: "/rt".to_string(),
            pandoc_path: None,
            is_interactive_session: false,
            running_in_ci: false,
            quarto_version: "0.1.0".to_string(),
        };
        let j = to_value(&v).unwrap();
        assert_eq!(j["isSingleFile"], true);
        assert_eq!(j["resourceDir"], "/res");
        assert_eq!(j["runtimeDir"], "/rt");
        assert_eq!(j["isInteractiveSession"], false);
        assert_eq!(j["runningInCi"], false);
        assert_eq!(j["quartoVersion"], "0.1.0");
        // projectDir None → null (present, not absent)
        assert_eq!(j["projectDir"], json!(null));
        assert!(j.get("is_single_file").is_none());
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
            supporting: vec![],
            filters: vec![],
            includes: None,
            html_dependencies: vec![TsHtmlDependency {
                name: "mylib".to_string(),
                stylesheets: vec!["/path/style.css".to_string()],
                scripts: vec![],
            }],
        };
        let j = to_value(&v).unwrap();
        assert_eq!(j["htmlDependencies"][0]["name"], "mylib");
        assert!(j.get("html_dependencies").is_none());
        assert_eq!(j["includes"], json!(null));
    }

    #[test]
    fn test_ts_execute_result_includes_some_round_trip() {
        let v = TsExecuteResult {
            markdown: String::new(),
            supporting: vec![],
            filters: vec![],
            includes: Some(TsPandocIncludes {
                in_header: Some(vec!["<style>".to_string()]),
                before_body: None,
                after_body: None,
            }),
            html_dependencies: vec![],
        };
        let serialized = serde_json::to_string(&v).unwrap();
        let deserialized: TsExecuteResult = from_str(&serialized).unwrap();
        assert_eq!(v, deserialized);
        let j = to_value(&v).unwrap();
        assert_eq!(j["includes"]["inHeader"], json!(["<style>"]));
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
        let result =
            from_str::<LaunchEngineResult>(r#"{"canFreeze": "yes", "generatesFigures": false}"#);
        assert!(result.is_err());
    }
}
