# Plan 4: Julia Engine Validation

**Grand plan:** [2026-04-16-ts-engine-extensions-subprocess.md](2026-04-16-ts-engine-extensions-subprocess.md)
**Depends on:** Plans 1, 2, and 3 (all must be substantially complete)
**Blocks:** Nothing (this is the final validation plan)
**Estimated sessions:** 1-2

## Overview

End-to-end validation of the TypeScript engine extension system using the Julia engine from Quarto 1. Take the real `julia-engine.ts`, set it up as a q2 extension, and render documents with Julia code cells.

This plan is primarily integration debugging. If Plans 1a, 1b, 2, and 3 are solid and the echo engine test from Plan 1b Phase 3 passes, most of the infrastructure works. This plan surfaces the gaps specific to a real-world engine extension.

## Prerequisites

- [ ] Plans 1a and 1b complete: subprocess infrastructure works, echo engine passes
- [ ] Plan 2 complete: `@quarto/markdown` and QuartoAPI namespaces implemented
- [ ] Plan 3 complete: `@quarto/jupyter` with `toMarkdown` working
- [ ] Julia installed on the test machine (`julia` in PATH)

## Work Items

### Phase 4A: Set up Julia engine extension

- [ ] Copy Julia engine from Quarto 1's **source/development version** (NOT the pkg-working version):
  ```
  ~/src/quarto-cli/src/resources/extension-subtrees/julia-engine/
  ```
  Use this version because it resolves resource files via `import.meta.url` (relative to the JS file), rather than the distributed version which uses `quarto.path.resource()` pointing to Quarto's global `share/` directory.

- [ ] Create test fixture with engine source AND its resource files:
  ```
  tests/fixtures/extensions/julia-engine/
    _extension.yml
    src/
      julia-engine.ts
      constants.ts
    Project.toml                        ← Julia environment definition
    ensure_environment.jl               ← Julia setup script
    quartonotebookrunner.jl             ← Julia execution entry point
    start_quartonotebookrunner_detached.jl  ← Daemon launcher
  ```
  The .jl files and Project.toml live alongside the extension (same directory or parent) so that `dirname(import.meta.url)` resolves them. This matches Quarto 1's development-mode layout where the extension is self-contained.
- [ ] Write `_extension.yml`:
  ```yaml
  title: Julia Engine
  author: Quarto
  version: 1.0.0
  contributes:
    engines:
      - name: julia
        path: src/julia-engine.ts
  ```
- [ ] Identify needed modifications to `julia-engine.ts`:
  - Import paths: change `@quarto/types` imports if our type names differ
  - API calls: verify all `quarto.*` calls match our implementation signatures
  - Resource resolution: verify `import.meta.url`-based paths work after bundling (the bundled .js file's URL determines the base directory — resource files must be relative to where the bundle is loaded from)
  - Deno APIs: verify `Deno.Command`, `Deno.connect`, `crypto.subtle`, file I/O all work (they should — it's running in real Deno)
  - Standard library imports: `"path"`, `"fs/exists"`, `"encoding/base64"` — resolved at build time via the import map
- [ ] Document every modification in a compatibility log

### Phase 4B: Minimal Julia render

The simplest possible Julia document.

- [ ] Create test document:
  ```markdown
  ---
  engine: julia
  ---

  ```{julia}
  1 + 1
  ```
  ```
- [ ] Run through q2's render pipeline. Use `cargo run -- render <file.qmd>` (the `quarto` crate at `crates/quarto/` is the main CLI binary). Check existing smoke tests in `crates/quarto/tests/` for how integration tests invoke rendering programmatically.
- [ ] Debug the first failure. Common failure checklist:
  - [ ] Extension not discovered → `_extension.yml` parsing issue
  - [ ] Deno subprocess won't start → Deno not in PATH, or engine-host.ts issue
  - [ ] Engine module fails to load → import resolution, transpilation issue
  - [ ] `engine.init()` fails → QuartoAPI construction issue
  - [ ] `engine.launch()` fails → EngineProjectContext mismatch
  - [ ] Julia process won't start → `Deno.Command` issue, Julia not in PATH
  - [ ] Julia server connection fails → TCP connect issue, HMAC auth issue
  - [ ] Execution succeeds but output is wrong → `toMarkdown()` issue
  - [ ] Result deserialization fails → protocol/type mismatch
- [ ] Iterate until the simple document renders successfully
- [ ] Verify output HTML contains the result `2`

### Phase 4C: Julia with figures

- [ ] Create test document with a plot:
  ```markdown
  ---
  engine: julia
  ---

  ```{julia}
  using Plots
  plot(1:10, rand(10))
  ```
  ```
- [ ] Verify:
  - [ ] Figure file generated in `_files/` directory
  - [ ] Figure referenced correctly in output markdown
  - [ ] `supporting` files tracked in `ExecuteResult`
  - [ ] HTML output renders with the figure

### Phase 4D: Multiple cells and error handling

- [ ] Test multiple code cells:
  ```markdown
  ---
  engine: julia
  ---

  ```{julia}
  x = 42
  ```

  ```{julia}
  println("x is $x")
  ```
  ```
- [ ] Verify state persists between cells (x defined in first, used in second)

- [ ] Test error handling:
  ```markdown
  ---
  engine: julia
  ---

  ```{julia}
  error("this should fail gracefully")
  ```
  ```
- [ ] Verify error produces a useful message, not a crash

### Phase 4E: Julia-specific features

- [ ] Test daemon mode (`execute.daemon: true`) — Julia server stays alive
- [ ] Test `exeflags` option — arguments passed to Julia
- [ ] Test `env` option — environment variables set for Julia
- [ ] Test cell options: `echo: false`, `output: false`, `warning: false`

### Phase 4F: Regression audit

- [ ] Run same test documents through Quarto 1 for comparison
- [ ] Document output differences
- [ ] Verify all existing q2 tests pass (`cargo nextest run --workspace`)
- [ ] Run `cargo xtask verify` for full validation
- [ ] File issues (via `br create`) for any gaps discovered

### Phase 4G: Adaptation documentation

- [ ] Write a summary of all changes needed to `julia-engine.ts`
- [ ] Categorize changes:
  - Import path adjustments
  - API signature differences
  - Missing QuartoAPI methods (if any were stubbed)
  - Behavioral differences
- [ ] This becomes the basis for documentation for extension authors migrating from Quarto 1

## Design Notes

### Debugging approach

The subprocess architecture helps debugging — you can run the Deno engine-host independently:

```bash
# Run engine-host manually for debugging
echo '{"type":"init","enginePath":"./julia-engine.ts","context":{...}}' | \
  deno run --allow-all ts-packages/quarto-engine-host/src/host.ts
```

You can also add `console.error()` statements in the engine or harness and see them on stderr.

### Standard library imports

The Julia engine imports `"path"`, `"fs/exists"`, `"encoding/base64"` from Deno's standard library. Following Quarto 1's approach, these are resolved at **build time** via the import map (`"path"` → `jsr:@std/path`, etc.) and inlined into the bundled `.js` file. At runtime, no import resolution is needed.

The build step for the Julia engine fixture:
```bash
deno bundle --config=resources/extension-build/deno.json julia-engine.ts > julia-engine.js
```

### CI gating

Julia engine tests should be:
- Gated behind a feature flag or test tag (Julia may not be installed in CI)
- Run manually during development
- Optionally run in CI if Julia is available

## Success Criteria

- [ ] Julia engine extension discovered and loaded by q2
- [ ] Simple Julia code cell executes and produces correct output
- [ ] Figure generation works
- [ ] Multiple cells with shared state work
- [ ] Error handling produces useful messages
- [ ] All modifications to julia-engine.ts documented
- [ ] No regressions in existing tests
- [ ] `cargo xtask verify` passes
