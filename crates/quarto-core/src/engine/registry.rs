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
use std::sync::{Arc, Mutex};

use quarto_error_reporting::DiagnosticMessage;

use crate::extension::types::ExtensionId;

use super::ExecutionError;
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
/// The `aliases` and `diagnostics` fields are `Arc<Mutex<…>>` so that
/// `TsEngine` instances can hold clones of the `Arc` (leaf-Arc sharing)
/// without creating a cycle back to `EngineRegistry`. The registry and
/// each engine share the same underlying `Mutex` data.
#[derive(Debug)]
pub struct EngineRegistry {
    engines: HashMap<String, Arc<dyn ExecutionEngine>>,
    /// Runtime-alias map: TS engine runtime name → resolved extension id.
    /// Populated lazily by `TsEngine` on first `LoadEngine` response.
    /// `Arc<Mutex<…>>` so `TsEngine` can hold a clone without a cycle.
    pub aliases: Arc<Mutex<HashMap<String, ExtensionId>>>,
    /// Diagnostics accumulated during registry lifetime (e.g. hint-validation
    /// warnings from `TsEngine` lazy init). Drained by the stage at
    /// end-of-render and forwarded to the pipeline's diagnostic sink.
    pub diagnostics: Arc<Mutex<Vec<DiagnosticMessage>>>,
    /// User-/extension-specified engine ordering: External engine names (registration order)
    /// followed by Reorder hints, in declared order. Consumed by resolution's auto-promotion
    /// (candidate_engines). Empty for a built-ins-only registry.
    pub(crate) contribution_order: Vec<String>,
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
            aliases: Arc::new(Mutex::new(HashMap::new())),
            diagnostics: Arc::new(Mutex::new(Vec::new())),
            contribution_order: Vec::new(),
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
            aliases: Arc::new(Mutex::new(HashMap::new())),
            diagnostics: Arc::new(Mutex::new(Vec::new())),
            contribution_order: Vec::new(),
        }
    }

    /// Register an engine.
    ///
    /// If an engine with the same name already exists, it is replaced.
    pub fn register(&mut self, engine: Arc<dyn ExecutionEngine>) {
        self.engines.insert(engine.name().to_string(), engine);
    }

    /// Read-only view of the engine contribution order (registration/priority
    /// order). Write access stays in-crate (direct field); a public write API is
    /// deferred to Plan 4b-C.
    pub fn contribution_order(&self) -> &[String] {
        &self.contribution_order
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

    /// Iterate engines in a deterministic order suitable for `claims_file` queries.
    ///
    /// Order: `contribution_order` (TS/extension engines, registration order) →
    /// built-in names (`knitr`, `jupyter`, `markdown`) → remaining engines
    /// alphabetically. Engines absent from the registry are skipped.
    ///
    /// This mirrors the candidate-engine ordering used by `resolve_engines`
    /// (the shared `resolution::BUILTIN_ORDER`) so `EngineClaimsFileStage`
    /// picks the same first-claimer that language resolution would.
    pub fn engines_in_order(&self) -> Vec<Arc<dyn ExecutionEngine>> {
        use super::resolution::BUILTIN_ORDER;

        let mut order: Vec<Arc<dyn ExecutionEngine>> = Vec::new();
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();

        // Extension engines in registration order.
        for name in &self.contribution_order {
            if !seen.contains(name.as_str())
                && let Some(engine) = self.engines.get(name.as_str())
            {
                seen.insert(name.as_str());
                order.push(Arc::clone(engine));
            }
        }

        // Built-in engines in declared order.
        for builtin in BUILTIN_ORDER {
            if !seen.contains(*builtin)
                && let Some(engine) = self.engines.get(*builtin)
            {
                seen.insert(builtin);
                order.push(Arc::clone(engine));
            }
        }

        // Remaining engines alphabetically.
        let mut extra: Vec<(&str, &Arc<dyn ExecutionEngine>)> = self
            .engines
            .iter()
            .filter(|(name, _)| !seen.contains(name.as_str()))
            .map(|(name, engine)| (name.as_str(), engine))
            .collect();
        extra.sort_unstable_by_key(|(name, _)| *name);
        for (_, engine) in extra {
            order.push(Arc::clone(engine));
        }

        order
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

    /// Build a registry that substitutes a [`super::ReplayEngine`] for
    /// the engine of the same name (bd-45yw).
    ///
    /// Starts from a default registry, then registers a replay engine
    /// constructed from `capture`. Because [`Self::register`] is
    /// last-write-wins, the replay engine replaces the real engine
    /// with the matching `name()`. Engines whose names don't match the
    /// recorded engine are left untouched, so a single replay run can
    /// still mix replayed and real engines if a future use case wants
    /// it.
    ///
    /// Activation lives at the orchestrator/CLI layer (`q2 render
    /// --replay <trace>`); this constructor is the seam.
    pub fn with_replay(capture: quarto_trace::EngineCapture) -> Self {
        Self::with_replay_many(vec![capture])
    }

    /// Build a registry that substitutes a [`super::ReplayEngine`] for
    /// each recorded engine in a sequence (bd-5yff4).
    ///
    /// Starts from a default registry, then registers one replay engine
    /// per capture, each keyed by its recorded `engine_name`. Because the
    /// engines in a sequence are distinct (the trace records one capture
    /// per engine, in order), the name-keyed registry holds them all
    /// without collision, and `EngineExecutionStage` drives them in the
    /// order the document's `engine:` sequence declares — which must match
    /// the recording. Each replay engine validates its own `input_qmd`.
    ///
    /// Captures whose `engine_name` is not also in the document's engine
    /// sequence simply never run; extra real engines (those without a
    /// matching capture) keep their default implementations.
    pub fn with_replay_many(captures: Vec<quarto_trace::EngineCapture>) -> Self {
        let mut registry = Self {
            engines: HashMap::new(),
            aliases: Arc::new(Mutex::new(HashMap::new())),
            diagnostics: Arc::new(Mutex::new(Vec::new())),
            contribution_order: Vec::new(),
        };
        // Start from the default engine set, then overlay replay engines.
        registry.register(Arc::new(MarkdownEngine::new()));
        #[cfg(not(target_arch = "wasm32"))]
        {
            registry.register(Arc::new(KnitrEngine::new()));
            registry.register(Arc::new(JupyterEngine::new()));
        }
        for capture in captures {
            registry.register(Arc::new(super::ReplayEngine::new(capture)));
        }
        registry
    }

    /// Accessor for the alias map (for `TsEngine` lazy init and Plan 1c
    /// alias-collision logic). Returns a cloned `Arc` so `TsEngine` can
    /// hold it without a cycle back to `EngineRegistry`.
    pub fn aliases(&self) -> Arc<Mutex<HashMap<String, ExtensionId>>> {
        Arc::clone(&self.aliases)
    }

    /// Accessor for the diagnostics vec (for `TsEngine` hint-validation
    /// warnings and stage drain). Returns a cloned `Arc` so `TsEngine` can
    /// hold it without a cycle back to `EngineRegistry`.
    pub fn diagnostics(&self) -> Arc<Mutex<Vec<DiagnosticMessage>>> {
        Arc::clone(&self.diagnostics)
    }

    /// Shut down every registered engine's backing subprocess. Best-effort: attempts ALL
    /// engines even if one errors, returning the first error encountered (the caller logs and
    /// continues — the host's Drop backstop reaps anything left). Idempotent.
    pub fn shutdown_all(&self) -> Result<(), ExecutionError> {
        let mut first_err = None;
        for engine in self.engines.values() {
            if let Err(e) = engine.shutdown()
                && first_err.is_none()
            {
                first_err = Some(e);
            }
        }
        match first_err {
            Some(e) => Err(e),
            None => Ok(()),
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
        assert!(!registry.is_empty());
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

    // ── Task 6 tests ──────────────────────────────────────────────────────────

    /// `shutdown_all` on a built-ins-only registry is a no-op `Ok`.
    /// Markdown, knitr, and jupyter all use the default no-op shutdown, so
    /// calling shutdown_all on a fresh registry must succeed without error.
    #[test]
    fn test_shutdown_all_noop_on_builtins() {
        let registry = EngineRegistry::new();
        assert!(
            registry.shutdown_all().is_ok(),
            "shutdown_all on built-ins-only registry must return Ok"
        );
    }

    /// `contribution_order` field stores names in declaration order.
    #[test]
    fn test_contribution_order_roundtrip() {
        let mut registry = EngineRegistry::new();
        registry.contribution_order.push("julia".to_string());
        registry.contribution_order.push("r-custom".to_string());
        assert_eq!(
            registry.contribution_order,
            vec!["julia", "r-custom"],
            "contribution_order must preserve insertion order"
        );
    }

    /// Real TsEngine shutdown via `shutdown_all` (Deno-gated).
    ///
    /// Registers a TsEngine whose host is backed by a real subprocess,
    /// asserts the host is alive before calling shutdown_all, then asserts
    /// it is dead after.  If `shutdown_all` does not call `engine.shutdown()`
    /// (or `TsEngine::shutdown` is the default no-op), the host stays alive
    /// and the second assertion fails.
    ///
    /// Exercised-guard: `is_alive()==true` before shutdown prevents a vacuously
    /// passing test where the process never started.
    #[test]
    // Spawns `sh` (Unix-only); gate on unix so a future Windows runner with Deno doesn't panic.
    #[cfg(all(not(target_arch = "wasm32"), unix))]
    fn test_shutdown_all_kills_ts_engine() {
        use crate::engine::ts_engine::TsEngine;
        use crate::engine::ts_process::{TsEngineHost, is_available as deno_is_available};
        use crate::engine::ts_protocol::HostGlobalConfig;
        use crate::extension::types::ExtensionId;
        use std::path::PathBuf;
        use std::process::Command;

        if !deno_is_available() {
            return;
        }

        let global = HostGlobalConfig {
            resource_dir: "/res".to_string(),
            runtime_dir: "/rt".to_string(),
            data_dir: "/data".to_string(),
            pandoc_path: None,
            is_interactive_session: false,
            running_in_ci: false,
            quarto_version: "0.1.0".to_string(),
        };

        // Spawn a simple subprocess that reads stdin indefinitely (exits on EOF,
        // which shutdown() triggers by closing stdin).
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("cat >/dev/null");

        let host = Arc::new(
            TsEngineHost::start_with_command(cmd, global).expect("start_with_command failed"),
        );

        // Exercised-guard: host must be alive before we call shutdown.
        assert!(
            host.is_alive(),
            "host must be alive immediately after start_with_command"
        );

        let aliases = Arc::new(Mutex::new(std::collections::HashMap::new()));
        let diag = Arc::new(Mutex::new(Vec::new()));
        let ext_id = ExtensionId::new("test-ts-engine");
        let engine = Arc::new(TsEngine::new(
            "test-ts-engine",
            false,
            PathBuf::from("/test/engine.ts"),
            Arc::clone(&host),
            None,
            None,
            None,
            ext_id,
            aliases,
            diag,
        ));

        let mut registry = EngineRegistry::empty();
        registry.register(engine);

        registry
            .shutdown_all()
            .expect("shutdown_all must return Ok");

        assert!(
            !host.is_alive(),
            "host must be dead after shutdown_all delegated to TsEngine::shutdown"
        );
    }

    #[test]
    fn test_engines_in_order_builtins_match_resolver_order() {
        // engines_in_order documents that it mirrors resolve_engines'
        // candidate ordering; pin the built-in segment to the resolver's
        // constant so the two orderings cannot diverge.
        let registry = EngineRegistry::new();
        let ordered: Vec<String> = registry
            .engines_in_order()
            .iter()
            .map(|e| e.name().to_string())
            .collect();
        let builtins: Vec<&str> = ordered
            .iter()
            .map(|s| s.as_str())
            .filter(|n| super::super::resolution::BUILTIN_ORDER.contains(n))
            .collect();
        assert_eq!(builtins, super::super::resolution::BUILTIN_ORDER);
    }
}
