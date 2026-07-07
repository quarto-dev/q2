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
    use std::path::{Path, PathBuf};

    use tempfile::TempDir;

    use quarto_core::engine::resolve_engines;
    use quarto_core::project::ProjectContext;
    use quarto_pandoc_types::Pandoc;
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

    // ── Plan 4b Phase C: project `_quarto.yml` `engines:` ordering splice ──────
    //
    // C1-C4 exercise `build_engine_registry`'s Task-9 splice through the REAL
    // production path (`ProjectContext::discover`), using the committed
    // `alpha`/`beta` synthetic-engine fixtures (Task A1) which both declare a
    // static `Primary(1)` claim on the `synth` language — an unbreakable tie
    // absent an explicit ordering source. `resolve_engines` is a pure
    // function (no engine load / subprocess / Deno — `TsEngine::claims_language`
    // short-circuits on the fixtures' static `claims` map), so this stays RI
    // (Rust integration), matching p1_2/p1_3's harness.

    /// Absolute path to a committed fixture extension directory
    /// (`crates/quarto-core/tests/fixtures/extensions/<name>`). Mirrors
    /// `synth_engines_e2e.rs::fixture_ext_dir`.
    fn fixture_ext_dir(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/extensions")
            .join(name)
    }

    /// Recursively copy `src` into `dst` (dst is created). Mirrors
    /// `synth_engines_e2e.rs::copy_dir` / `marimo_resolution.rs::copy_dir`.
    fn copy_dir(src: &Path, dst: &Path) {
        fs::create_dir_all(dst).unwrap();
        for entry in fs::read_dir(src).unwrap() {
            let entry = entry.unwrap();
            let from = entry.path();
            let to = dst.join(entry.file_name());
            if from.is_dir() {
                copy_dir(&from, &to);
            } else {
                fs::copy(&from, &to).unwrap();
            }
        }
    }

    /// Build a temp project dir with a caller-supplied `_quarto.yml` body and
    /// the named committed fixture extensions installed under `_extensions/`.
    fn setup_project_with_quarto_yml(quarto_yml: &str, ext_names: &[&str]) -> TempDir {
        let tmp = TempDir::new().unwrap();
        write_file(&tmp.path().join("_quarto.yml"), quarto_yml);
        for name in ext_names {
            copy_dir(
                &fixture_ext_dir(name),
                &tmp.path().join("_extensions").join(name),
            );
        }
        tmp
    }

    /// Install a SINGLE "combo" extension that declares both `alpha` and
    /// `beta` as External engines in ONE `_extension.yml`'s
    /// `contributes.engines` array — deliberately NOT two sibling
    /// `_extensions/<name>/` directories.
    ///
    /// Why: `discover_extensions` scans `_extensions/` via `fs::read_dir`,
    /// whose entry order is filesystem-dependent (not guaranteed
    /// alphabetical or creation-order) — two sibling extension dirs give a
    /// contribution_order that is nondeterministic across platforms/runs.
    /// A single extension's YAML array order IS deterministic (parsed list
    /// order), so installing alpha+beta as two `contributes.engines` entries
    /// of the SAME extension makes the PRE-splice contribution_order
    /// reliably `["alpha", "beta"]` everywhere — required for C1's RED to be
    /// genuine (bound to the Task-9 splice) rather than incidental (bound to
    /// directory-scan luck). Confirmed empirically: the naive two-directory
    /// setup made C1 pass BEFORE any implementation existed, on this
    /// filesystem, because `fs::read_dir` happened to return beta before
    /// alpha.
    ///
    /// Reuses the REAL committed Task-A1 `dist/<name>.js` bundles and their
    /// identical static Primary(1)-on-`synth` claim shape (verbatim from
    /// each fixture's own `_extension.yml`), just declared from one
    /// consolidated YAML document instead of two.
    fn install_combo_alpha_beta_extension(project_dir: &Path) {
        let combo_dist = project_dir.join("_extensions/combo/dist");
        fs::create_dir_all(&combo_dist).unwrap();
        fs::copy(
            fixture_ext_dir("alpha").join("dist/alpha.js"),
            combo_dist.join("alpha.js"),
        )
        .unwrap();
        fs::copy(
            fixture_ext_dir("beta").join("dist/beta.js"),
            combo_dist.join("beta.js"),
        )
        .unwrap();
        write_file(
            &project_dir.join("_extensions/combo/_extension.yml"),
            r#"
title: Combo Alpha/Beta Engine
author: Test
version: 0.1.0
contributes:
  engines:
    - path: dist/alpha.js
      name: alpha
      claims:
        synth:
          kind: primary
          priority: 1
      file-extensions: []
      claims-files: []
    - path: dist/beta.js
      name: beta
      claims:
        synth:
          kind: primary
          priority: 1
      file-extensions: []
      claims-files: []
"#,
        );
    }

    /// Parse `qmd` through the REAL qmd reader — same parse path
    /// `resolve_doc` uses in `marimo_resolution.rs`.
    fn parse_qmd(qmd: &str) -> Pandoc {
        let (ast, _ctx, _warnings) = pampa::readers::qmd::read(
            qmd.as_bytes(),
            false,
            "test.qmd",
            &mut std::io::sink(),
            true,
            None,
        )
        .expect("qmd parse must succeed for a well-formed fixture doc");
        ast
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

    // ── C1 (Plan 4b Phase C): project `engines:` ordering RED→GREEN ────────────
    // `_quarto.yml` `engines: [beta, alpha]`, both alpha/beta declare static
    // Primary(1) on `synth` (an unbreakable tie without an ordering source).
    // Before the Task-9 splice, `engines:` (plural, project-level) is read by
    // NEITHER `resolve_engines` (reads the singular document `engine:` key)
    // NOR `contribution_order` (extension-registration order only, which the
    // combo extension's YAML array fixes deterministically as
    // `["alpha", "beta"]`) — so beta does NOT win pre-implementation; alpha
    // (declared first in the combo extension's array) does.
    //
    // RED: Task-9 splice absent (project/mod.rs:~749) → beta not first in
    // contribution_order → resolve_engines' equal-priority tiebreak (registry.rs
    // `contribution_order`) falls through to declaration order (alpha).
    #[test]
    fn c1_project_engines_key_orders_beta_before_alpha() {
        let tmp = setup_project_with_quarto_yml(
            "project:\n  type: default\nengines:\n  - beta\n  - alpha\n",
            &[],
        );
        install_combo_alpha_beta_extension(tmp.path());

        let rt = runtime();
        let project = ProjectContext::discover(tmp.path(), &rt).unwrap();

        let ast = parse_qmd("```{synth}\n1 + 1\n```\n");
        let resolution = resolve_engines(&ast.meta, &ast, &project.registry, None);

        assert_eq!(
            resolution.ownership.get("synth").map(String::as_str),
            Some("beta"),
            "project `engines: [beta, alpha]` should make beta win the Primary(1) tie \
             on synth; contribution_order: {:?}",
            project.registry.contribution_order()
        );
    }

    // ── C2 (Plan 4b Phase C): unknown-name validation ───────────────────────────
    // `engines: [nonexistent-synth]` with no engine of that name registered
    // anywhere (no extensions installed — built-ins only) → construction
    // returns Err "not a valid engine … Available engines are: …". DISTINCT
    // from p1_3 (which drives the SAME message via an extension's Reorder
    // hint, not the project `engines:` key) — this test binds the NEW
    // engines:-key entry-name validation path specifically.
    //
    // RED: the new engines:-key entry-name validation absent → project names
    // never enter `contribution_order`, so step 6 never sees
    // 'nonexistent-synth' → construction returns Ok instead of Err.
    #[test]
    fn c2_unknown_project_engine_name_errors_listing_available() {
        let tmp = setup_project_with_quarto_yml(
            "project:\n  type: default\nengines:\n  - nonexistent-synth\n",
            &[],
        );
        let rt = runtime();
        let result = ProjectContext::discover(tmp.path(), &rt);
        assert!(
            result.is_err(),
            "unknown name in project `engines:` should return Err; got Ok"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("nonexistent-synth"),
            "error should name the unregistered engine 'nonexistent-synth': {}",
            err
        );
        assert!(
            err.contains("not a valid engine"),
            "error should use the shared 'not a valid engine' message: {}",
            err
        );
        assert!(
            err.contains("Available engines are:"),
            "error should list available engines: {}",
            err
        );
    }

    // ── C3 (Plan 4b Phase C): {path:} reserved-skip ─────────────────────────────
    // `engines: [{path: ./x.js}]` — Q1's external-engine loader shape,
    // RESERVED/SKIPPED by q2 (engines arrive via `_extensions/` discovery
    // instead). Must contribute NO ordering entry and must NOT error, even
    // though `./x.js` never exists and is not a registered engine name.
    //
    // RED: the entry→Option<name> helper's `{path:}⇒None` arm absent (i.e. a
    // naive implementation that always treats a map's first key as a name) →
    // "path" (or the literal path string) leaks in as a name → either
    // contribution_order changes or step 6 errors on the bogus "path" name.
    #[test]
    fn c3_path_map_entry_reserved_skip_no_error_no_order_change() {
        let tmp = setup_project_with_quarto_yml(
            "project:\n  type: default\nengines:\n  - path: ./x.js\n",
            &[],
        );
        let rt = runtime();
        let project = ProjectContext::discover(tmp.path(), &rt)
            .expect("a {path:} entry must not error — it is reserved/skipped, not validated");
        assert!(
            project.registry.contribution_order().is_empty(),
            "a {{path:}} entry contributes no ordering name; contribution_order should stay \
             empty (no extensions installed): got {:?}",
            project.registry.contribution_order()
        );
    }

    // ── C4 (Plan 4b Phase C): {name:{claims}} ordering ──────────────────────────
    // `engines: [{beta: {claims: [...]}}]` — Plan 6's claim-table entry shape;
    // the payload (`claims: [...]`) is Plan 6's and is IGNORED in 4b. Only the
    // KEY ("beta") is extracted and used for ordering — beta must win the
    // Primary(1) tie on `synth` exactly as bare `- beta` (C1) would.
    //
    // RED: the single-key-map name-extraction arm absent (entry→Option<name>
    // returns None for a non-`{path:}` map, or extracts the wrong thing) →
    // beta's name never reaches contribution_order → alpha (declared first in
    // the combo extension) wins instead.
    #[test]
    fn c4_single_key_map_entry_orders_beta_first_payload_ignored() {
        let tmp = setup_project_with_quarto_yml(
            "project:\n  type: default\nengines:\n  - beta:\n      claims:\n        - unused: true\n",
            &[],
        );
        install_combo_alpha_beta_extension(tmp.path());

        let rt = runtime();
        let project = ProjectContext::discover(tmp.path(), &rt).unwrap();

        let ast = parse_qmd("```{synth}\n1 + 1\n```\n");
        let resolution = resolve_engines(&ast.meta, &ast, &project.registry, None);

        assert_eq!(
            resolution.ownership.get("synth").map(String::as_str),
            Some("beta"),
            "`engines: [{{beta: {{claims: […]}}}}]` should make beta win the Primary(1) tie \
             on synth exactly as bare `- beta` would (payload ignored); contribution_order: {:?}",
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
