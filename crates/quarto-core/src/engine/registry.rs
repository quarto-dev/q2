/*
 * engine/registry.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Registry of available execution engines.
 */

//! Registry of available execution engines.
//!
//! The registry manages the collection of available engines and provides
//! lookup by name. It handles the difference between native and WASM builds,
//! registering only the engines available in each environment.

use std::collections::HashMap;
use std::sync::Arc;

use super::markdown::MarkdownEngine;
use super::traits::ExecutionEngine;

#[cfg(not(target_arch = "wasm32"))]
use super::jupyter::JupyterEngine;
#[cfg(not(target_arch = "wasm32"))]
use super::knitr::KnitrEngine;

/// Registry of available execution engines.
///
/// The registry holds references to engine implementations and provides
/// lookup by name. It is designed to be created once and shared across
/// the application.
///
/// # Platform Support
///
/// - **Native builds**: All engines (markdown, knitr, jupyter)
/// - **WASM builds**: Only markdown engine
///
/// # Thread Safety
///
/// The registry uses `Arc<dyn ExecutionEngine>` for thread-safe sharing.
#[derive(Debug)]
pub struct EngineRegistry {
    engines: HashMap<String, Arc<dyn ExecutionEngine>>,
}

impl EngineRegistry {
    /// Create a new registry with default engines.
    ///
    /// Registers all engines available for the current platform:
    /// - markdown: Always available
    /// - knitr: Native builds only
    /// - jupyter: Native builds only
    pub fn new() -> Self {
        let mut registry = Self {
            engines: HashMap::new(),
        };

        // Always register markdown engine
        registry.register(Arc::new(MarkdownEngine::new()));

        // Register native-only engines
        #[cfg(not(target_arch = "wasm32"))]
        {
            registry.register(Arc::new(KnitrEngine::new()));
            registry.register(Arc::new(JupyterEngine::new()));
        }

        registry
    }

    /// Create an empty registry (for testing).
    pub fn empty() -> Self {
        Self {
            engines: HashMap::new(),
        }
    }

    /// Register an engine.
    ///
    /// If an engine with the same name already exists, it is replaced.
    pub fn register(&mut self, engine: Arc<dyn ExecutionEngine>) {
        self.engines.insert(engine.name().to_string(), engine);
    }

    /// Get an engine by name.
    ///
    /// Returns `None` if no engine with the given name is registered.
    pub fn get(&self, name: &str) -> Option<Arc<dyn ExecutionEngine>> {
        self.engines.get(name).cloned()
    }

    /// Get the default engine (markdown).
    ///
    /// This always succeeds as the markdown engine is always registered.
    ///
    /// # Panics
    ///
    /// Panics if the markdown engine is not registered (should never happen
    /// with a properly constructed registry).
    pub fn default_engine(&self) -> Arc<dyn ExecutionEngine> {
        self.get("markdown")
            .expect("markdown engine should always be registered")
    }

    /// List all registered engine names.
    pub fn engine_names(&self) -> Vec<&str> {
        self.engines.keys().map(|s| s.as_str()).collect()
    }

    /// Check if an engine is registered.
    pub fn has_engine(&self, name: &str) -> bool {
        self.engines.contains_key(name)
    }

    /// Get the number of registered engines.
    pub fn len(&self) -> usize {
        self.engines.len()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.engines.is_empty()
    }

    /// Get an engine by name, falling back to default with a warning.
    ///
    /// If the requested engine is not found, returns the markdown engine
    /// and appends a warning message to the provided vector.
    pub fn get_or_default(
        &self,
        name: &str,
        warnings: &mut Vec<String>,
    ) -> Arc<dyn ExecutionEngine> {
        if let Some(engine) = self.get(name) {
            engine
        } else {
            warnings.push(format!(
                "Engine '{}' not available, falling back to markdown (no execution)",
                name
            ));
            self.default_engine()
        }
    }
}

impl Default for EngineRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// Implement Debug for Arc<dyn ExecutionEngine>
impl std::fmt::Debug for dyn ExecutionEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExecutionEngine")
            .field("name", &self.name())
            .field("available", &self.is_available())
            .field("can_freeze", &self.can_freeze())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_new_has_markdown() {
        let registry = EngineRegistry::new();
        assert!(registry.has_engine("markdown"));
    }

    #[test]
    fn test_registry_get_markdown() {
        let registry = EngineRegistry::new();
        let engine = registry.get("markdown");
        assert!(engine.is_some());
        assert_eq!(engine.unwrap().name(), "markdown");
    }

    #[test]
    fn test_registry_default_engine() {
        let registry = EngineRegistry::new();
        let engine = registry.default_engine();
        assert_eq!(engine.name(), "markdown");
    }

    #[test]
    fn test_registry_get_nonexistent() {
        let registry = EngineRegistry::new();
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_registry_empty() {
        let registry = EngineRegistry::empty();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn test_registry_register_custom() {
        let mut registry = EngineRegistry::empty();

        registry.register(Arc::new(MarkdownEngine::new()));

        assert!(registry.has_engine("markdown"));
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn test_registry_len() {
        let registry = EngineRegistry::new();
        // At minimum, markdown is registered
        assert!(registry.len() >= 1);
    }

    #[test]
    fn test_registry_engine_names() {
        let registry = EngineRegistry::new();
        let names = registry.engine_names();
        assert!(names.contains(&"markdown"));
    }

    #[test]
    fn test_registry_get_or_default_found() {
        let registry = EngineRegistry::new();
        let mut warnings = Vec::new();

        let engine = registry.get_or_default("markdown", &mut warnings);

        assert_eq!(engine.name(), "markdown");
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_registry_get_or_default_not_found() {
        let registry = EngineRegistry::new();
        let mut warnings = Vec::new();

        let engine = registry.get_or_default("unknown-engine", &mut warnings);

        assert_eq!(engine.name(), "markdown");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("unknown-engine"));
        assert!(warnings[0].contains("not available"));
    }

    #[test]
    fn test_registry_default_impl() {
        let registry = EngineRegistry::default();
        assert!(registry.has_engine("markdown"));
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn test_registry_native_has_knitr_and_jupyter() {
        let registry = EngineRegistry::new();
        assert!(registry.has_engine("knitr"));
        assert!(registry.has_engine("jupyter"));
    }

    #[test]
    fn test_registry_register_replaces() {
        let mut registry = EngineRegistry::empty();

        // Register markdown
        registry.register(Arc::new(MarkdownEngine::new()));
        assert_eq!(registry.len(), 1);

        // Register again (should replace, not add)
        registry.register(Arc::new(MarkdownEngine::new()));
        assert_eq!(registry.len(), 1);
    }
}
