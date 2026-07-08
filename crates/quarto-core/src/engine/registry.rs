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

    /// Engines that would need to load to answer a claim consultation and
    /// are not covered by a metadata claim table (Phase 4 — feeds the
    /// Phase-5 "why didn't this doc lift to Pass-1" warning).
    ///
    /// "Would need to load" means `try_claims_language` returns `None` — a
    /// per-engine property, uniform across all languages (see the trait
    /// docs), so probing with a single dummy language is well-defined. A
    /// name present in `tabled` is excluded regardless of its own answer:
    /// the metadata claim table covers it, so the engine itself is never
    /// consulted (`claim_for`/`claim_for_noload`'s table-first precedence).
    ///
    /// Returns `(name, Option<_extension.yml path>)`; the path comes from
    /// [`super::traits::ExecutionEngine::extension_yml_path`] (Phase 5) —
    /// `Some` for an External TS engine, `None` for a built-in.
    pub fn engines_needing_load(
        &self,
        tabled: &std::collections::HashSet<String>,
    ) -> Vec<(String, Option<std::path::PathBuf>)> {
        self.engines
            .values()
            .filter(|engine| !tabled.contains(engine.name()))
            .filter(|engine| engine.try_claims_language("__probe__", None).is_none())
            .map(|engine| (engine.name().to_string(), engine.extension_yml_path()))
            .collect()
    }

    /// `(engine name, _extension.yml absolute path)` pairs for every
    /// registered engine that carries extension provenance (External TS
    /// engines only — built-ins answer `None` from `extension_yml_path`
    /// and are excluded). Sorted by name for deterministic Pass-1 cache-key
    /// hashing (Plan 6 decision 9 — `project::cache_key::Pass1KeyInputs`).
    pub fn engine_extension_provenance(&self) -> Vec<(String, std::path::PathBuf)> {
        let mut out: Vec<(String, std::path::PathBuf)> = self
            .engines
            .values()
            .filter_map(|engine| {
                engine
                    .extension_yml_path()
                    .map(|path| (engine.name().to_string(), path))
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
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

    // ── Phase 4: engines_needing_load ────────────────────────────────────────

    /// A minimal engine that never overrides `try_claims_language` — the
    /// trait-default `None` (would-load) applies. Stands in for a claims-less
    /// dynamic engine (e.g. a legacy `TsEngine` with no `_extension.yml`
    /// `claims:`).
    #[derive(Debug)]
    struct DynamicTestEngine {
        name: &'static str,
    }

    impl ExecutionEngine for DynamicTestEngine {
        fn name(&self) -> &str {
            self.name
        }

        fn execute(
            &self,
            input: &str,
            _ctx: &crate::engine::context::ExecutionContext,
        ) -> Result<crate::engine::context::ExecuteResult, ExecutionError> {
            Ok(crate::engine::context::ExecuteResult::passthrough(input))
        }
    }

    /// After Phase 4's `try_claims_language` overrides, every built-in
    /// answers statically — with no claim table at all, nothing needs load.
    #[test]
    fn test_engines_needing_load_empty_for_builtins_with_no_table() {
        let registry = EngineRegistry::new();
        let tabled: std::collections::HashSet<String> = std::collections::HashSet::new();
        assert!(
            registry.engines_needing_load(&tabled).is_empty(),
            "all built-ins answer try_claims_language statically; none should need load"
        );
    }

    /// An untabled engine whose `try_claims_language` returns `None` (the
    /// trait default) must be reported.
    #[test]
    fn test_engines_needing_load_reports_untabled_dynamic_engine() {
        let mut registry = EngineRegistry::empty();
        registry.register(Arc::new(DynamicTestEngine { name: "dynamic" }));
        let tabled: std::collections::HashSet<String> = std::collections::HashSet::new();
        let needing = registry.engines_needing_load(&tabled);
        assert_eq!(
            needing.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
            vec!["dynamic"],
            "an untabled engine that would-load must be reported"
        );
    }

    /// A tabled engine is excluded regardless of its own `try_claims_language`
    /// answer — the table covers it, so it never needs load.
    #[test]
    fn test_engines_needing_load_excludes_tabled_dynamic_engine() {
        let mut registry = EngineRegistry::empty();
        registry.register(Arc::new(DynamicTestEngine { name: "dynamic" }));
        let mut tabled: std::collections::HashSet<String> = std::collections::HashSet::new();
        tabled.insert("dynamic".to_string());
        assert!(
            registry.engines_needing_load(&tabled).is_empty(),
            "a tabled engine must be excluded even though it would-load on its own"
        );
    }

    /// Build a non-spawned `TsEngine` for provenance tests. `TsEngineHost::new`
    /// does not spawn a subprocess (first round-trip does), so no `deno`
    /// gating is needed — these tests never call anything that loads/launches.
    fn make_ts_engine(name: &str) -> crate::engine::ts_engine::TsEngine {
        use crate::engine::ts_engine::TsEngine;
        use crate::engine::ts_process::TsEngineHost;
        use crate::engine::ts_protocol::HostGlobalConfig;

        let global = HostGlobalConfig {
            resource_dir: "/res".to_string(),
            runtime_dir: "/rt".to_string(),
            data_dir: "/data".to_string(),
            pandoc_path: None,
            is_interactive_session: false,
            running_in_ci: false,
            quarto_version: "0.1.0".to_string(),
        };
        let host = Arc::new(TsEngineHost::new(global));
        TsEngine::new(
            name,
            true,
            std::path::PathBuf::from("/ext/dist/engine.js"),
            host,
            None,
            None,
            None,
            ExtensionId::new(name),
            Arc::new(Mutex::new(std::collections::HashMap::new())),
            Arc::new(Mutex::new(Vec::new())),
        )
    }

    /// Plan 6 Phase 5: a `TsEngine` with no `extension_yml_path` set
    /// answers `None` from the trait method (matches every other engine's
    /// default).
    #[test]
    fn test_ts_engine_extension_yml_path_defaults_none() {
        let engine = make_ts_engine("legacy");
        assert_eq!(engine.extension_yml_path(), None);
    }

    /// After `set_extension_yml_path`, the trait method reflects it — and
    /// `engines_needing_load` surfaces the real path instead of a stub
    /// `None` (the pre-Phase-5 behavior).
    #[test]
    fn test_engines_needing_load_reports_ts_engine_extension_yml_path() {
        let engine = make_ts_engine("legacy");
        let yml_path = std::path::PathBuf::from("/ext/_extension.yml");
        engine.set_extension_yml_path(yml_path.clone());
        assert_eq!(engine.extension_yml_path(), Some(yml_path.clone()));

        let mut registry = EngineRegistry::empty();
        registry.register(Arc::new(engine));
        let tabled: std::collections::HashSet<String> = std::collections::HashSet::new();
        let needing = registry.engines_needing_load(&tabled);
        assert_eq!(
            needing,
            vec![("legacy".to_string(), Some(yml_path))],
            "engines_needing_load must report the real _extension.yml path, \
             not the pre-Phase-5 None stub"
        );
    }

    // ── Plan 6 Phase 5: engine_extension_provenance (cache-key input) ────────

    #[test]
    fn test_engine_extension_provenance_empty_for_builtins() {
        let registry = EngineRegistry::new();
        assert!(
            registry.engine_extension_provenance().is_empty(),
            "built-in engines carry no extension provenance"
        );
    }

    #[test]
    fn test_engine_extension_provenance_includes_ts_engines_sorted_by_name() {
        let b = make_ts_engine("b-engine");
        b.set_extension_yml_path(std::path::PathBuf::from("/ext/b/_extension.yml"));
        let a = make_ts_engine("a-engine");
        a.set_extension_yml_path(std::path::PathBuf::from("/ext/a/_extension.yml"));

        let mut registry = EngineRegistry::empty();
        registry.register(Arc::new(b));
        registry.register(Arc::new(a));

        assert_eq!(
            registry.engine_extension_provenance(),
            vec![
                (
                    "a-engine".to_string(),
                    std::path::PathBuf::from("/ext/a/_extension.yml")
                ),
                (
                    "b-engine".to_string(),
                    std::path::PathBuf::from("/ext/b/_extension.yml")
                ),
            ],
            "provenance pairs must be sorted by engine name regardless of \
             registration order"
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
