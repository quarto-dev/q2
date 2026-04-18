# Plan 1b: Extension Integration & End-to-End

**Grand plan:** [2026-04-16-ts-engine-extensions-subprocess.md](2026-04-16-ts-engine-extensions-subprocess.md)
**Depends on:** Plan 1a (Protocol & Core Infrastructure)
**Blocks:** Plan 4 (Julia Validation)
**Estimated sessions:** 1-2

## Overview

Wire the TS engine infrastructure from Plan 1a into the extension system and
detection pipeline. Parse engine contributions from `_extension.yml`, build TS
extensions with `deno bundle`, migrate the `EngineRegistry` into `StageContext`,
rewrite engine detection with the 4-phase algorithm, and validate end-to-end with
an echo engine integration test.

## Phase order

Phase 1 → Phase 2 → Phase 3

## Work Items

### Phase 1: Extension discovery and build

Parse `_extension.yml` for engine contributions, build TS extensions into bundled JS, and register `TsEngine` instances.

Following Quarto 1's approach: engine extensions are **built** (bundled from TS to a single JS file) before execution. At runtime, q2 loads the bundled `.js` file — no import map or TS transpilation needed at execution time.

- [ ] Add `engines` field to the `Contributes` struct in `crates/quarto-core/src/extension/types.rs`:
  ```rust
  /// Engine contributions (paths to TS engine modules).
  pub engines: Vec<EngineContribution>,
  ```
  And define:
  ```rust
  /// An engine contributed by an extension.
  #[derive(Debug, Clone)]
  pub struct EngineContribution {
      /// Absolute path to the engine module (.ts source or .js bundle).
      pub path: PathBuf,
  }
  ```
  Note: Quarto 1's schema also allows bare strings (engine names for reordering),
  but those are ordering hints handled in `_quarto.yml` `engines:`, not contributions.
  Extension `contributes.engines` entries always have a `path`.

- [ ] Add `engines` parsing in `parse_contributes()` in `crates/quarto-core/src/extension/read.rs`:
  - Handle array of objects with `path` key (resolve to absolute paths relative to ext_dir)
  - Include `engines` in the "at least one sub-field" validation check
  - This supersedes Phase 8 of the extensions grand plan
    (`claude-notes/plans/2026-03-16-extensions-grand-plan.md`)

- [ ] Define extension YAML schema for engines, matching Quarto 1's schema:
  ```yaml
  contributes:
    engines:
      - path: julia-engine.js
  ```
  **Quarto 1 reference:** The extension schema (in `src/resources/schema/extension.yml`) defines engines as an array of either strings (engine names for reordering) or objects with a `path` property. The Julia engine's `_extension.yml` uses `- path: julia-engine.js` (pointing to the pre-built bundle). The engine's name comes from the module's `name` property at runtime, not from the YAML.

  In q2, the `path` can point to either the `.ts` source (build step produces `.js`) or a pre-built `.js` bundle. Discovery queries (claimsLanguage, claimsFile) are handled dynamically by the subprocess.
- [ ] Implement engine extension build step:
  - Provide an import map (`resources/extension-build/import-map.json`) that resolves:
    - `@quarto/types` → our type definitions (`.d.ts`, erased during bundling)
    - `"path"` → `jsr:@std/path`
    - `"fs/exists"` → `jsr:@std/fs/exists`
    - `"encoding/base64"` → `jsr:@std/encoding/base64`
  - Provide a `deno.json` config pointing to the import map
  - Use `deno bundle --config=<deno.json> <entry.ts>` to produce a single `.js` file
  - Output the bundle to `_extensions/{name}/{stem}.js`
  - This mirrors Quarto 1's `quarto call build-ts-extension` command
- [ ] Implement a `quarto build-ts-extension` subcommand (or integrate into existing build pipeline). CLI subcommands are defined using `clap` in `crates/quarto/src/main.rs` — add a new variant to the `Commands` enum, create a handler module in `crates/quarto/src/commands/`, and add the match arm in `main()`.
- [ ] Scan `_extensions/` for engine contributions during project initialization.
  **Quarto 1 reference:** `resolveEngineExtensions()` in `src/project/project-context.ts` discovers extensions with `contributes.engines`, merges them into `projectConfig.engines`. Then `resolveEngines()` in `src/execute/engine.ts` imports and registers them.
- [ ] For each discovered engine:
  1. Check if a bundled `.js` exists (built output)
  2. If not, check if the `.ts` source exists and auto-build it (or error with instructions)
  3. Create a `TsEngine` instance pointing to the bundled `.js`
  4. Register it in the `EngineRegistry`
- [ ] Support `_quarto.yml` `engines:` list for ordering. Following Quarto 1's model:
  1. Extension-contributed engines are appended to `projectConfig.engines`
  2. Engines listed explicitly in `_quarto.yml` `engines:` come first (higher priority on ties)
  3. Standard engines (knitr, jupyter, markdown) follow
  4. This ordering affects `claims_language` tie-breaking (first engine with highest score wins)
- [ ] Update engine detection to recognize extension-provided engine names
- [ ] Support `engine: julia` in document YAML triggering the extension engine
- [ ] Write test: fixture extension directory → build → engine registered and detectable
- [ ] Write test: `_quarto.yml` `engines:` list controls ordering

### Phase 2: Engine detection rewrite + registry migration

Rewrite engine detection to use the 4-phase algorithm, move the `EngineRegistry`
from `EngineExecutionStage` into `StageContext`, and restructure the pipeline
entry point so that `claimsFile` (Phase 1) runs before `ParseDocument`.

**Current state:** `detection.rs` only checks YAML metadata (explicit `engine:` key
and engine-name top-level keys). No language-based detection exists — the file has a
"Future Enhancements" comment. The `EngineRegistry` is created inside
`EngineExecutionStage::new()` with only built-in engines, and
`EngineExecutionStage.run()` takes `&self` so can't mutate the registry.

**Quarto 1 reference:** `fileExecutionEngine()` in `src/execute/engine.ts` (lines
302-351) and `markdownExecutionEngine()` (lines 146-211) implement the 4-phase algorithm.

**Modernization:** In Quarto 1, Jupyter explicitly claimed "julia" via `claimsLanguage`,
creating a priority conflict with the Julia engine extension (both returned `true` =
priority 1, winner depended on registration order). In q2, Jupyter does NOT claim
"julia" — that's the Julia extension's job. Everything else matches Quarto 1: Jupyter
claims no languages via Phase 3 (Python and other languages reach Jupyter via the
Phase 4 fallback, same as Quarto 1), and Phase 4 is preserved because Jupyter is a
universal kernel executor.

**Pre-parse engine detection:** Phase 1 (`claimsFile`) must run BEFORE
`ParseDocument`, because if an engine claims a non-QMD file (e.g., `.jl`
percent script), the engine must convert it to QMD before pampa can parse it.
The flow is:

```
Input file
  │
  ├─ claimsFile: engine claims it (non-QMD) ─→ markdownForFile ─→ QMD text
  │                                                                   │
  └─ claimsFile: no engine claims it ─────────────────────────────────┤
                                                                      ▼
                                                              ParseDocument
                                                                      │
                                                              (rest of pipeline)
```

For QMD files, no engine claims via `claimsFile` (`.qmd` is not a
percent/spin format), so parsing proceeds directly. Engine detection
continues later via Phases 2-4 (YAML, language scan, fallback) which
operate on the parsed AST.

For non-QMD files (`.jl`, `.py`, `.r`, `.ipynb`), an engine claims the
file, provides QMD text via `markdownForFile`, and that text enters the
pipeline. For TS engines, this requires the Deno subprocess to be running
— it's lazily spawned on first `claimsFile` query.

- [ ] **Move `EngineRegistry` from `EngineExecutionStage` into `StageContext`.**
  Currently `EngineExecutionStage` owns the registry (created with built-ins only in
  `new()`). Move it to `StageContext` where it's built during `StageContext::new()`:
  1. Start with `EngineRegistry::new()` (built-in engines)
  2. Scan `ctx.extensions` for engine contributions (`contributes.engines`)
  3. For each `EngineContribution`, create a `TsEngine` and `registry.register()` it
  4. Store as `ctx.registry: EngineRegistry`
  `EngineExecutionStage` becomes stateless — its `run()` reads `ctx.registry`.
  Remove the `registry` field from `EngineExecutionStage`; the `with_registry()`
  test constructor is replaced by tests that build a `StageContext` with a custom registry.

- [ ] **Remove the `KNOWN_ENGINES` constant** from `detection.rs`. Currently hardcoded
  as `["markdown", "knitr", "jupyter"]`. With extension engines, the set of known
  engines is dynamic — it's whatever's in the registry. Replace any usage of
  `KNOWN_ENGINES` (currently used in Phase 2 detection for top-level YAML key scanning)
  with a query against the registry's engine names: `registry.engine_names()`.

- [ ] Implement 4-phase detection (new function or refactor of `detect_engine()`).
  New signature: `detect_engine(metadata, registry, ast) → DetectedEngine`
  (takes the registry and parsed AST, not just metadata).
  1. **Phase 1 — File extension claims**: For each engine in registry order, call
     `claims_file(file, ext)`. First engine to claim wins. Used for `.ipynb` → jupyter,
     `.rmd` → knitr.
  2. **Phase 2 — YAML declaration**: Check explicit `engine:` key in frontmatter
     (existing logic). Also check engine-name top-level keys — scan
     `registry.engine_names()` instead of `KNOWN_ENGINES`. Skip phases 3-4.
  3. **Phase 3 — Language scanning**: Extract languages + first classes from code blocks
     using the **parsed AST** (not regex — q2 has pampa for this). For each language,
     call each engine's `claims_language(language, first_class)`. Highest `Option<u32>`
     score wins. Engine iteration order breaks ties (user-specified engines first).
  4. **Phase 4 — Fallback**: If unclaimed computational languages exist (not handler
     languages like `ojs`), default to Jupyter (it may have a kernel). If no
     computational code blocks at all, default to markdown engine.
- [ ] For language extraction from AST: use pampa's existing parsing to get code block
  languages and their classes, rather than regex. Quarto 1 uses
  `languagesWithClasses()` regex on raw markdown; we should use the parsed
  tree-sitter AST instead.
- [ ] `claimsFile` results are NOT cacheable — implementations may inspect file content
  (e.g., Julia engine reads the file to check for percent script `# %%` markers).
  Cache `claimsLanguage` results per engine per `(language, first_class)` pair.
- [ ] When a document has an explicit `engine: julia` in YAML, skip discovery entirely
  — just look up the engine by name in the registry. This is the common case and
  requires zero subprocess calls.
- [ ] Write test: engine claims "julia" language, document with `{julia}` blocks selects it
- [ ] Write test: explicit `engine: julia` in YAML skips discovery, resolves directly
- [ ] Write test: priority scoring — higher score wins over lower score
- [ ] Write test: unclaimed computational language → falls through to Jupyter (Phase 4 fallback)
- [ ] Write test: no code blocks → falls through to markdown engine
- [ ] Write test: extension engine registered in context, discoverable by name

### Phase 3: Echo engine integration test

End-to-end test with a minimal TypeScript engine.

**Dependency note:** The echo engine imports types from `@quarto/types`. If Plan 2
Phase 2D hasn't defined these yet, create a minimal type stub inline in the echo
engine file (just the interfaces it needs: `ExecutionEngineDiscovery`,
`ExecutionEngineInstance`, `QuartoAPI`). These can be replaced with proper imports later.

- [ ] Create test fixture `tests/fixtures/extensions/echo-engine/`:
  ```
  _extension.yml
  src/echo-engine.ts
  ```
- [ ] `echo-engine.ts` — claims "echo" language, returns input with markers:
  ```typescript
  const echoEngine: ExecutionEngineDiscovery = {
      name: "echo",
      claimsLanguage: (lang) => lang === "echo",
      launch: (ctx) => ({
          name: "echo",
          canFreeze: false,
          execute: async (opts) => ({
              engine: "echo",
              markdown: opts.target.markdown.value.replace(
                  /```\{echo\}[\s\S]*?```/g,
                  "**ECHO_EXECUTED**"
              ),
              supporting: [],
              filters: [],
          }),
          // ... minimal stubs for other methods
      }),
  };
  export default echoEngine;
  ```
- [ ] Write Rust integration test:
  1. Set up project with echo engine extension
  2. Render a .qmd with `{echo}` code blocks using `cargo run -- render <file>` (the `quarto` crate is the main CLI binary). Alternatively, write a Rust test that programmatically invokes the rendering pipeline — check existing tests in `crates/quarto/tests/` for patterns.
  3. Verify output contains "ECHO_EXECUTED"
- [ ] This test validates the full pipeline: discovery → subprocess spawn → protocol → execution → result

## Design Notes

### Extension build model

Following Quarto 1's two-step approach:
1. **Build time:** `deno bundle --config=<deno.json> <entry.ts>` bundles the TS engine extension into a single `.js` file. An import map resolves `@quarto/types` (erased as type-only), Deno std lib imports, etc. All dependencies are inlined. `deno bundle` is a stable Deno feature (reintroduced in Deno 2.4, permanently supported; uses esbuild under the hood).
2. **Runtime:** The Deno subprocess loads the bundled `.js` file via dynamic `import()`. No import map or TS transpilation needed — everything is already resolved and bundled.

Note: The **engine-host harness** is built with esbuild (matching existing q2 patterns), while **engine extensions** are built with `deno bundle` (matching Quarto 1's extension build model and handling Deno-specific imports like `jsr:` specifiers). These are different build steps for different artifacts.

This means the Deno subprocess invocation is simple:
```bash
deno run --allow-all <engine-host.js>
```

No `--import-map` flag needed at runtime.

## Future Work: Built-in engine percent/spin script support

The pre-parse `claimsFile` → `markdownForFile` flow (Phase 2 above) is
designed for TS engine extensions but also applies to built-in engines.
Currently, q2's built-in engines don't implement `claims_file` or
`markdown_for_file` — they only handle `.qmd` input.

Adding non-QMD file support to built-in engines requires implementing
the trait methods on each engine:

- **Jupyter**: `claims_file(".py") → true`, `claims_file(".jl") → true`,
  `markdown_for_file` with a Rust percent-script converter (port of
  Quarto 1's `markdownFromJupyterPercentScript`)
- **Knitr**: `claims_file(".r") → true` (for spin scripts),
  `markdown_for_file` invoking R's `knitr::spin()` via the R subprocess

No pipeline changes needed — the architecture from Phase 2 supports it.
This is out of scope for this plan (validation target is `.qmd` files)
but is a natural follow-on.

## Success Criteria

- [ ] Extension discovery finds engine extensions in `_extensions/`
- [ ] `_quarto.yml` `engines:` list controls engine ordering
- [ ] `EngineRegistry` lives in `StageContext`, populated with extension engines
- [ ] `KNOWN_ENGINES` constant removed; detection uses registry dynamically
- [ ] 4-phase engine detection works: file extension → YAML → language scan → Jupyter fallback (unclaimed langs) / markdown (no code)
- [ ] Echo engine integration test passes end-to-end
- [ ] Tests requiring Deno are skipped if Deno is absent (same pattern as pandoc)
- [ ] All existing tests pass (no regressions)
