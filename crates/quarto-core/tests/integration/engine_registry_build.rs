/*
 * tests/integration/engine_registry_build.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Task 7b registry-build seam tests (P1-1..P1-6, warning, bundle-missing).
 * All tests use real temp dir fixtures with `_extensions/<name>/_extension.yml`
 * plus a present (empty) `.js` file for tests that expect success.
 * Registration is zero-spawn — no Deno subprocess is started during construction.
 */

// Native-only: TsEngine / TsEngineHost are behind cfg(not(target_arch = "wasm32")).
#[cfg(not(target_arch = "wasm32"))]
mod registry_build_tests {
    use std::fs;

    use tempfile::TempDir;

    use quarto_core::project::ProjectContext;
    use quarto_system_runtime::NativeRuntime;

    fn runtime() -> NativeRuntime {
        NativeRuntime::new()
    }

    fn write_file(path: &std::path::Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    /// Set up a minimal project dir with one extension that has a present `.js` file.
    /// Returns the `TempDir` (keep alive for the test's duration).
    fn setup_project_with_engine_ext(ext_name: &str, extension_yml: &str) -> TempDir {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();

        // `_quarto.yml` marks this as a named project (not single-file).
        write_file(&dir.join("_quarto.yml"), "project:\n  type: default\n");

        let ext_dir = dir.join(format!("_extensions/{}", ext_name));

        // A present-but-empty `.js` file satisfies the bundle-exists check.
        write_file(&ext_dir.join("engine.js"), "// stub bundle");

        write_file(&ext_dir.join("_extension.yml"), extension_yml);

        tmp
    }

    // ── P1-1: extension engine registered in registry ──────────────────────────
    // RED: drop `registry.register(engine)` in step 4d → `engine_names()` lacks it.

    #[test]
    fn p1_1_extension_engine_appears_in_engine_names() {
        let tmp = setup_project_with_engine_ext(
            "my-engine",
            r#"
title: My Engine
author: Test
contributes:
  engines:
    - path: engine.js
      name: my-engine
      claims: {}
      file-extensions: []
      claims-files: []
"#,
        );
        let rt = runtime();
        let project = ProjectContext::discover(tmp.path(), &rt).unwrap();
        assert!(
            project.registry.engine_names().contains(&"my-engine"),
            "engine 'my-engine' should appear in engine_names(); got: {:?}",
            project.registry.engine_names()
        );
    }

    // ── P0: no extensions → built-ins only, no IO for host ──────────────────────
    // Before the lazy-host fix, build_engine_registry ALWAYS called
    // quarto_runtime_dir() / quarto_data_dir() to construct HostGlobalConfig,
    // even when no extension contributed an External engine. That widened the IO
    // failure surface (read-only HOME broke every render).
    //
    // This test binds the observable half: a project with no _extensions/ still
    // produces a working registry with the three built-ins and an empty
    // contribution_order. The structural guarantee (no IO in that branch) is bound
    // by the any_external_engine unit tests in project/mod.rs.

    #[test]
    fn p0_no_extension_project_builds_builtins_only() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        write_file(&dir.join("_quarto.yml"), "project:\n  type: default\n");
        // Deliberately no _extensions/ directory.

        let rt = runtime();
        let project = ProjectContext::discover(dir, &rt).unwrap();

        // All three built-ins present.
        let names = project.registry.engine_names();
        assert!(
            names.contains(&"markdown"),
            "built-in 'markdown' missing: {:?}",
            names
        );
        assert!(
            names.contains(&"knitr"),
            "built-in 'knitr' missing: {:?}",
            names
        );
        assert!(
            names.contains(&"jupyter"),
            "built-in 'jupyter' missing: {:?}",
            names
        );

        // No extension-contributed engines, so contribution_order is empty.
        assert!(
            project.registry.contribution_order().is_empty(),
            "contribution_order should be empty with no engine extensions; got: {:?}",
            project.registry.contribution_order()
        );
    }

    // ── P1-5 (registration half): declared-name engine registered; zero-spawn ──
    // RED: drop `registry.register(engine)` in step 4d → `has_engine("echo")` false.
    //
    // Note on what this test binds: the integration binary cannot access
    // TsEngineHost internals (is_alive() / spawn-count are private + #[cfg(test)]),
    // so this test only binds REGISTRATION (`has_engine`). The no-spawn property
    // is structurally guaranteed: build_engine_registry constructs TsEngineHost via
    // TsEngineHost::new(global) and never calls ensure_started(); the zero-load
    // mechanism is separately bound by the TsEngine unit tests (T4 P1-12).
    //
    // Zero-spawn guard: if any spawn had been attempted it would error (Deno not
    // available in typical CI).

    #[test]
    fn p1_5_named_engine_registered_without_spawn() {
        let tmp = setup_project_with_engine_ext(
            "echo-ext",
            r#"
title: Echo Engine
author: Test
contributes:
  engines:
    - path: engine.js
      name: echo
      claims: {}
      file-extensions: []
      claims-files: []
"#,
        );
        let rt = runtime();
        let project = ProjectContext::discover(tmp.path(), &rt).unwrap();
        assert!(
            project.registry.has_engine("echo"),
            "engine 'echo' (declared via `name:`) should be registered; got: {:?}",
            project.registry.engine_names()
        );
        // Zero-spawn: if the registry build had spawned Deno it would have been
        // observable (error or slow); the test must pass without Deno present.
    }

    // ── P1-6 (alias seam, registration half): unnamed engine → ext-id key ──────
    // RED: drop `registry.register(engine)` → `has_engine("julia-ext")` false.

    #[test]
    fn p1_6_unnamed_engine_registered_under_ext_id() {
        let tmp = setup_project_with_engine_ext(
            "julia-ext",
            r#"
title: Julia Engine
author: Test
contributes:
  engines:
    - path: engine.js
"#,
        );
        let rt = runtime();
        let project = ProjectContext::discover(tmp.path(), &rt).unwrap();
        assert!(
            project.registry.has_engine("julia-ext"),
            "unnamed engine should be registered under its extension-id 'julia-ext'; got: {:?}",
            project.registry.engine_names()
        );
    }

    // ── P1-4 (collision): two exts both declaring `name: julia` → Err naming both ─
    // RED: replace collision check with silent register → no Err returned.

    #[test]
    fn p1_4_name_collision_errors_and_names_both_contributors() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        write_file(&dir.join("_quarto.yml"), "project:\n  type: default\n");

        // First extension declares name: julia
        let ext_a_dir = dir.join("_extensions/ext-a");
        write_file(&ext_a_dir.join("engine.js"), "// stub");
        write_file(
            &ext_a_dir.join("_extension.yml"),
            r#"
title: Ext A
author: Test
contributes:
  engines:
    - path: engine.js
      name: julia
      claims: {}
      file-extensions: []
      claims-files: []
"#,
        );

        // Second extension also declares name: julia → collision
        let ext_b_dir = dir.join("_extensions/ext-b");
        write_file(&ext_b_dir.join("engine.js"), "// stub");
        write_file(
            &ext_b_dir.join("_extension.yml"),
            r#"
title: Ext B
author: Test
contributes:
  engines:
    - path: engine.js
      name: julia
      claims: {}
      file-extensions: []
      claims-files: []
"#,
        );

        let rt = runtime();
        let result = ProjectContext::discover(dir, &rt);
        assert!(result.is_err(), "engine name collision should return Err");
        let err = result.unwrap_err().to_string();

        // Error must name the colliding engine
        assert!(
            err.contains("julia"),
            "collision error should name the colliding engine 'julia': {}",
            err
        );
        // Error must name BOTH contributors
        assert!(
            err.contains("ext-a") && err.contains("ext-b"),
            "collision error should name both contributors 'ext-a' and 'ext-b': {}",
            err
        );
    }

    // ── P1-3 (unknown reorder): Reorder hint for unregistered engine → Err ──────
    // RED: drop the step-6 validation → no Err returned.

    #[test]
    fn p1_3_unknown_reorder_hint_errors_listing_available() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        write_file(&dir.join("_quarto.yml"), "project:\n  type: default\n");

        // Extension with a Reorder hint for a non-existent engine
        let ext_dir = dir.join("_extensions/reorder-ext");
        write_file(
            &ext_dir.join("_extension.yml"),
            r#"
title: Reorder Ext
author: Test
contributes:
  engines:
    - nonexistent-engine
"#,
        );

        let rt = runtime();
        let result = ProjectContext::discover(dir, &rt);
        assert!(
            result.is_err(),
            "Reorder hint naming an unregistered engine should return Err"
        );
        let err = result.unwrap_err().to_string();

        // Must name the bad engine
        assert!(
            err.contains("nonexistent-engine"),
            "error should name the unregistered engine 'nonexistent-engine': {}",
            err
        );
        // Must list available engines (markdown is always present)
        assert!(
            err.contains("markdown"),
            "error should list available engines (at least 'markdown'): {}",
            err
        );
    }

    // ── P1-2 (ordering): contribution_order populated from declarations ─────────
    // Verifies that External engine names appear in contribution_order after discover().

    #[test]
    fn p1_2_contribution_order_contains_declared_engines() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        write_file(&dir.join("_quarto.yml"), "project:\n  type: default\n");

        // One extension declaring name: alpha
        let ext_dir = dir.join("_extensions/alpha-ext");
        write_file(&ext_dir.join("engine.js"), "// stub");
        write_file(
            &ext_dir.join("_extension.yml"),
            r#"
title: Alpha Engine
author: Test
contributes:
  engines:
    - path: engine.js
      name: alpha
      claims: {}
      file-extensions: []
      claims-files: []
"#,
        );

        let rt = runtime();
        let project = ProjectContext::discover(dir, &rt).unwrap();
        assert!(
            project
                .registry
                .contribution_order()
                .contains(&"alpha".to_string()),
            "contribution_order should contain 'alpha'; got: {:?}",
            project.registry.contribution_order()
        );
    }

    // ── Warning surfaced: missing static fields → registry.diagnostics ───────────
    // RED: drop the step-4e push → diagnostics is empty.

    #[test]
    fn warning_missing_static_fields_appears_in_diagnostics() {
        // Extension declares only `path` — all four static fields (name, claims,
        // file-extensions, claims-files) are absent → one warning expected.
        let tmp = setup_project_with_engine_ext(
            "bare-engine",
            r#"
title: Bare Engine
author: Test
contributes:
  engines:
    - path: engine.js
"#,
        );
        let rt = runtime();
        let project = ProjectContext::discover(tmp.path(), &rt).unwrap();
        let diagnostics = project.registry.diagnostics.lock().unwrap();
        assert!(
            !diagnostics.is_empty(),
            "missing static fields should produce at least one diagnostic warning"
        );
        let diag_titles: Vec<&str> = diagnostics.iter().map(|d| d.title.as_str()).collect();
        let combined = diag_titles.join("\n");
        assert!(
            combined.contains("bare-engine"),
            "diagnostic should name the extension 'bare-engine': {}",
            combined
        );
    }

    // ── Bundle-missing: .js path absent → Err mentioning build-ts-extension ──────
    // RED: drop the step-4a exists check → no Err returned.

    #[test]
    fn bundle_missing_errors_with_build_ts_extension_hint() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        write_file(&dir.join("_quarto.yml"), "project:\n  type: default\n");

        let ext_dir = dir.join("_extensions/missing-bundle");
        // _extension.yml present but engine.js is deliberately absent
        write_file(
            &ext_dir.join("_extension.yml"),
            r#"
title: Missing Bundle Engine
author: Test
contributes:
  engines:
    - path: engine.js
      name: missing-engine
      claims: {}
      file-extensions: []
      claims-files: []
"#,
        );
        // engine.js intentionally NOT created

        let rt = runtime();
        let result = ProjectContext::discover(dir, &rt);
        assert!(
            result.is_err(),
            "missing .js bundle should return Err; got Ok"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("build-ts-extension"),
            "error should suggest 'q2 build-ts-extension': {}",
            err
        );
    }
}
