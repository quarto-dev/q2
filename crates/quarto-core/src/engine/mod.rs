/*
 * engine/mod.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Execution engine infrastructure.
 */

//! Execution engine infrastructure for Quarto.
//!
//! This module provides the core abstractions for code execution in
//! Quarto documents. Execution engines transform QMD documents with
//! executable code cells into documents with execution outputs.
//!
//! # Architecture
//!
//! The engine system consists of:
//!
//! - [`ExecutionEngine`] trait - Interface for all execution engines
//! - [`EngineRegistry`] - Collection of available engines
//! - [`detect_engine`] - Detection of engine from document metadata
//! - Concrete engines:
//!   - [`MarkdownEngine`] - No-op engine (always available)
//!   - [`KnitrEngine`] - R code execution (native only)
//!   - [`JupyterEngine`] - Python/Julia execution (native only)
//!
//! # Platform Support
//!
//! | Engine | Native | WASM |
//! |--------|--------|------|
//! | markdown | ✓ | ✓ |
//! | knitr | ✓ | ✗ |
//! | jupyter | ✓ | ✗ |
//!
//! In WASM builds, requesting an unavailable engine will result in a
//! warning and fallback to the markdown engine.
//!
//! # Example
//!
//! ```ignore
//! use quarto_core::engine::{EngineRegistry, detect_engine};
//!
//! // Create registry with all available engines
//! let registry = EngineRegistry::new();
//!
//! // Detect engine from document metadata
//! let detected = detect_engine(&doc.ast.meta);
//!
//! // Get the engine (with fallback)
//! let mut warnings = Vec::new();
//! let engine = registry.get_or_default(&detected.name, &mut warnings);
//!
//! // Execute
//! let result = engine.execute(&qmd_content, &context)?;
//! ```

pub mod capture_files;
pub mod capture_splice;
mod context;
mod detection;
mod error;
mod markdown;
pub mod preview_record;
mod registry;
mod replay;
pub mod resolution;
mod traits;
pub mod ts_protocol;

// Native-only: subprocess management + demux for TS engine extensions.
#[cfg(not(target_arch = "wasm32"))]
pub mod ts_process;

// Native-only: TsEngine struct implementing ExecutionEngine via TsEngineHost.
#[cfg(not(target_arch = "wasm32"))]
mod ts_engine;

// File-backed test engine for exercising multi-engine sequencing
// (bd-5yff4). Native-only test utility; never registered in the default
// registry. See `fixture.rs` module docs.
#[cfg(not(target_arch = "wasm32"))]
mod fixture;

// Native-only modules
#[cfg(not(target_arch = "wasm32"))]
pub mod jupyter;
#[cfg(not(target_arch = "wasm32"))]
mod knitr;

use std::time::Duration;

// ── Shared, WASM-clean types consumed by traits, resolution, and TsEngine ──

/// Resolution tier for multi-engine language ownership.
///
/// See `claude-notes/designs/engine-resolution.md` §3.1 for the full contract.
///
/// **Semantics:**
/// - `kind` sets the resolution tier; `priority` orders *only within* a kind
///   (kind dominates priority — `Primary(-100)` beats `Fallback(100)`).
/// - `Interop` is presence-gated: fires only for an engine already in the
///   sequence via a positive claim ("extend if I'm already here," not "claim
///   anywhere").
/// - `Fallback` is the universal-kernel role; no longer hardcoded to jupyter —
///   any engine can declare it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LanguageClaim {
    /// I execute this language. (default priority 1)
    Primary(i32),
    /// Extend my ownership to this language iff I'm already present. (default priority 0)
    Interop(i32),
    /// Universal-kernel role: I will execute any language as a fallback. (default priority 0)
    Fallback(i32),
    /// I make no claim on this language.
    None,
}

/// Languages that Quarto handles downstream via cell handlers (ojs, mermaid, dot).
///
/// Engines must **leave these blocks unchanged** in their output — they are
/// pass-through cell handlers, not languages the engine executes. This is an
/// instruction to engines, not documentation: when q2 grows real cell-handler
/// support, this constant migrates to a registry. Single source of truth until
/// then.
pub const HANDLED_LANGUAGES: &[&str] = &["ojs", "mermaid", "dot"];

/// Default per-request timeout for engine execution (5 minutes).
///
/// Promoted from `engine/jupyter/execute.rs` so it can be shared by the
/// `ExecutionContext` builder and future TS-engine callers without a native-only
/// import path.
pub const DEFAULT_EXECUTE_TIMEOUT: Duration = Duration::from_secs(300);

// Re-export public types
pub use context::{ExecuteResult, ExecutionContext};
pub use detection::{
    DetectedEngine, EngineSequence, detect_engine, detect_engine_sequence, detect_engines,
};
pub use error::ExecutionError;
#[cfg(not(target_arch = "wasm32"))]
pub use fixture::FixtureEngine;
pub use markdown::MarkdownEngine;
pub use registry::EngineRegistry;
pub use replay::ReplayEngine;
pub use resolution::{EngineResolution, resolve_engines};
pub use traits::ExecutionEngine;

// Re-export native-only engines
#[cfg(not(target_arch = "wasm32"))]
pub use jupyter::JupyterEngine;
#[cfg(not(target_arch = "wasm32"))]
pub use knitr::KnitrEngine;
#[cfg(not(target_arch = "wasm32"))]
pub use ts_engine::TsEngine;

/// Print `perf.engine-discover jupyter=N rscript=N` to stderr when
/// `QUARTO_PERF_STATS=1`. Call once at the end of a top-level
/// command (e.g. `q2 render`) so the gauge survives the work it
/// measures. See `claude-notes/plans/2026-05-22-engine-discovery-cache.md`.
pub fn print_discovery_stats_if_enabled() {
    if !std::env::var_os("QUARTO_PERF_STATS").is_some_and(|v| v == "1") {
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let jupyter = jupyter::find_jupyter_call_count();
        let rscript = knitr::find_rscript_call_count();
        eprintln!("perf.engine-discover jupyter={jupyter} rscript={rscript}");
    }
    #[cfg(target_arch = "wasm32")]
    {
        eprintln!("perf.engine-discover jupyter=0 rscript=0");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::ConfigValue;
    use quarto_pandoc_types::config_value::ConfigMapEntry;
    use quarto_source_map::SourceInfo;
    use std::path::PathBuf;
    use std::sync::Arc;

    /// Helper to create a map ConfigValue
    fn map_config(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        let map_entries: Vec<ConfigMapEntry> = entries
            .into_iter()
            .map(|(key, value)| ConfigMapEntry {
                key: key.to_string(),
                key_source: SourceInfo::for_test(),
                value,
            })
            .collect();
        ConfigValue::new_map(map_entries, SourceInfo::for_test())
    }

    /// Helper to create a string ConfigValue
    fn string_config(s: &str) -> ConfigValue {
        ConfigValue::new_string(s, SourceInfo::for_test())
    }

    fn make_test_context() -> ExecutionContext {
        ExecutionContext::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/project"),
            PathBuf::from("/project/doc.qmd"),
            "html",
        )
    }

    // === Integration tests ===

    #[test]
    fn test_engine_detection_and_lookup() {
        // engine: markdown
        let meta = map_config(vec![("engine", string_config("markdown"))]);
        let detected = detect_engine(&meta);

        let registry = EngineRegistry::new();
        let engine = registry.get(&detected.name);

        assert!(engine.is_some());
        assert_eq!(engine.unwrap().name(), "markdown");
    }

    #[test]
    fn test_engine_execution_markdown() {
        let registry = EngineRegistry::new();
        let engine = registry.get("markdown").unwrap();
        let ctx = make_test_context();

        let input = "# Hello\n\nWorld";
        let result = engine.execute(input, &ctx).unwrap();

        assert_eq!(result.markdown, input);
    }

    #[test]
    fn test_engine_fallback_on_unknown() {
        let meta = map_config(vec![("engine", string_config("unknown-engine"))]);
        let detected = detect_engine(&meta);

        // Detection returns the unknown name as-is
        assert_eq!(detected.name, "unknown-engine");

        let registry = EngineRegistry::new();
        let mut warnings = Vec::new();
        let engine = registry.get_or_default(&detected.name, &mut warnings);

        // Registry falls back to markdown and adds warning
        assert_eq!(engine.name(), "markdown");
        assert!(!warnings.is_empty());
        assert!(warnings[0].contains("unknown-engine"));
    }

    #[test]
    fn test_engine_with_config() {
        // engine:
        //   jupyter:
        //     kernel: python3
        let jupyter_config = map_config(vec![("kernel", string_config("python3"))]);
        let engine_value = map_config(vec![("jupyter", jupyter_config)]);
        let meta = map_config(vec![("engine", engine_value)]);

        let detected = detect_engine(&meta);

        assert_eq!(detected.name, "jupyter");
        assert!(detected.config.is_some());

        let config = detected.config.unwrap();
        assert!(config.get("kernel").is_some());
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn test_native_engines_registered() {
        let registry = EngineRegistry::new();

        assert!(registry.has_engine("markdown"));
        assert!(registry.has_engine("knitr"));
        assert!(registry.has_engine("jupyter"));
    }

    #[test]
    fn test_engine_trait_object_safety() {
        // Verify ExecutionEngine can be used as a trait object
        let registry = EngineRegistry::new();
        let engine: Arc<dyn ExecutionEngine> = registry.default_engine();

        assert_eq!(engine.name(), "markdown");
        assert!(engine.is_available());
    }

    #[test]
    fn test_detected_engine_default() {
        let empty_meta = ConfigValue::new_map(vec![], SourceInfo::for_test());
        let detected = detect_engine(&empty_meta);

        assert_eq!(detected.name, "markdown");
        assert!(detected.is_markdown());
        assert!(!detected.requires_runtime());
    }

    /// Pin the contract: HANDLED_LANGUAGES must be exactly ["ojs", "mermaid", "dot"].
    /// All three knitr sites read from this constant; any accidental change here
    /// would silently alter the leave-alone set sent to the R subprocess.
    #[test]
    fn test_handled_languages_constant() {
        assert_eq!(HANDLED_LANGUAGES, &["ojs", "mermaid", "dot"]);
    }
}
