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

mod context;
mod detection;
mod error;
mod markdown;
pub mod reconcile;
mod registry;
mod traits;

// Native-only modules
#[cfg(not(target_arch = "wasm32"))]
pub mod jupyter;
#[cfg(not(target_arch = "wasm32"))]
mod knitr;

// Re-export public types
pub use context::{ExecuteResult, ExecutionContext};
pub use detection::{DetectedEngine, KNOWN_ENGINES, detect_engine, is_known_engine};
pub use error::ExecutionError;
pub use markdown::MarkdownEngine;
pub use registry::EngineRegistry;
pub use traits::ExecutionEngine;

// Re-export native-only engines
#[cfg(not(target_arch = "wasm32"))]
pub use jupyter::JupyterEngine;
#[cfg(not(target_arch = "wasm32"))]
pub use knitr::KnitrEngine;

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
                key_source: SourceInfo::default(),
                value,
            })
            .collect();
        ConfigValue::new_map(map_entries, SourceInfo::default())
    }

    /// Helper to create a string ConfigValue
    fn string_config(s: &str) -> ConfigValue {
        ConfigValue::new_string(s, SourceInfo::default())
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
        let empty_meta = ConfigValue::new_map(vec![], SourceInfo::default());
        let detected = detect_engine(&empty_meta);

        assert_eq!(detected.name, "markdown");
        assert!(detected.is_markdown());
        assert!(!detected.requires_runtime());
    }
}
