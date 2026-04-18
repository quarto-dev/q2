# Grand Plan: TypeScript Engine Extensions for q2 (v2 — Subprocess)

## Overview

Implement TypeScript engine extensions in q2 using a **Deno subprocess** architecture. Engine extensions are TypeScript modules (following Quarto 1's API) that q2 discovers, loads via a long-lived Deno subprocess, and communicates with via a JSON message protocol over stdin/stdout.

**Goal:** A user places a TypeScript engine extension in `_extensions/my-engine/` with an `_extension.yml`, and q2 discovers it, spawns a Deno engine-host process, queries the engine for file/language claims, and delegates code cell execution to it.

**Validation target:** The Julia engine extension from Quarto 1 (`julia-engine.ts`).

**Key design choice:** Subprocess over embedded Deno. This eliminates the need to add deno ext crates (deno_fs, deno_process, deno_net, deno_crypto, etc.) to q2's binary. The engine extension runs in a full Deno environment — all standard APIs available, all Deno standard library modules importable, TypeScript transpilation handled by Deno natively. The QuartoAPI is implemented primarily in TypeScript (using Deno's own APIs), with q2-specific context passed as a JSON blob at initialization.

## Architecture

### Subprocess lifecycle (one per project render)

```
q2 (Rust)                              engine-host (Deno subprocess)
─────────                              ─────────────────────────────
spawn deno engine-host.ts ──────────→  start, read init message
                                       
send: { type: "init",        ──────→  load engine module
        enginePath: "...",             call engine.init(quartoAPI)
        context: { paths, format, ... } }
recv: { type: "ready",       ←──────  engine loaded, ready for queries
        engineMeta: { name, canFreeze, ... } }

send: { type: "claimsLanguage", ───→  call engine.claimsLanguage("julia")
        language: "julia" }
recv: { type: "claimsResult", ←─────  return result
        result: true }

send: { type: "execute",     ──────→  call engine.launch(ctx).execute(opts)
        options: { ... } }
                                       (during execution, logs go to stderr)
recv: { type: "executeResult", ←────  return ExecuteResult
        result: { markdown, supporting, ... } }

send: { type: "shutdown" }   ──────→  clean up, exit
```

### Where things live

```
q2 repo
├── crates/
│   └── quarto-core/src/engine/
│       ├── ts_engine.rs          ← TsEngine struct (implements ExecutionEngine)
│       ├── ts_protocol.rs        ← JSON message types + serialization
│       └── ts_process.rs         ← Deno process management
│
├── ts-packages/                  ← npm workspace (already exists)
│   ├── quarto-engine-host/       ← NEW: Deno-side harness
│   │   ├── src/
│   │   │   ├── host.ts           ← stdin/stdout protocol handler
│   │   │   ├── quarto-api.ts     ← QuartoAPI construction from context
│   │   │   └── engine-loader.ts  ← dynamic import + validation
│   │   └── package.json
│   │
│   ├── quarto-markdown/          ← NEW: clean markdown utilities
│   │   ├── src/
│   │   │   ├── extract-yaml.ts
│   │   │   ├── partition.ts
│   │   │   ├── languages.ts
│   │   │   └── break-quarto-md.ts
│   │   └── package.json
│   │
│   └── quarto-jupyter/           ← NEW: clean notebook processing
│       ├── src/
│       │   ├── to-markdown.ts
│       │   ├── types.ts
│       │   ├── display-data.ts
│       │   ├── tags.ts
│       │   ├── labels.ts
│       │   ├── preserve.ts
│       │   └── ...
│       └── package.json
│
└── quarto-cli (reference only, at ~/src/quarto-cli)
    └── packages/quarto-types/    ← existing, pure .d.ts, use as-is
```

## Protocol Design

### Message format

All messages are JSON objects, one per line on stdin/stdout. Each has a `type` field.

**Rust → Deno (stdin):**
```typescript
// Initialize the engine host with context and engine module path
{ type: "init", enginePath: string, context: EngineHostContext }

// Discovery queries
{ type: "claimsLanguage", language: string, firstClass?: string }
{ type: "claimsFile", file: string, ext: string }

// File conversion (non-QMD files only — engine reads its native format)
{ type: "markdownForFile", file: string }

// Execution (q2 provides pre-extracted metadata and source_map)
{ type: "execute", options: TsExecuteOptions }

// Post-execute
{ type: "dependencies", options: TsDependenciesOptions }
{ type: "postprocess", options: TsPostProcessOptions }
{ type: "postRender", file: TsRenderResultFile }

// Queries
{ type: "filterFormat", source: string, options: TsRenderOptions, format: TsFormatInfo }
{ type: "canKeepSource", target: TsExecutionTarget }
{ type: "intermediateFiles", input: string }
{ type: "executeTargetSkipped", target: TsExecutionTarget, format: TsFormatInfo }

// Lifecycle
{ type: "shutdown" }
```

**Deno → Rust (stdout):**
```typescript
// Initialization response
{ type: "ready", engineMeta: { name, canFreeze, generatesFigures, validExtensions } }

// Discovery responses — returns false (no claim), true (priority 1), or number (custom priority)
{ type: "claimsResult", result: false | true | number }

// File conversion response
{ type: "markdownForFileResult", result: { value: string, fileName?: string } }

// Execution response
{ type: "executeResult", result: TsExecuteResult }

// Post-execute responses
{ type: "dependenciesResult", result: TsDependenciesResult }
{ type: "postprocessResult" }
{ type: "postRenderResult" }

// Query responses
{ type: "filterFormatResult", result: TsFormatInfo }
{ type: "canKeepSourceResult", result: boolean }
{ type: "intermediateFilesResult", result: string[] | undefined }
{ type: "executeTargetSkippedResult" }

// Errors
{ type: "error", message: string, stack?: string }
```

**Not in protocol** (q2 handles natively):
- `target()` — harness constructs `ExecutionTarget` from `TsExecuteOptions`
- `partitionedMarkdown()` — q2 has the full AST
- `run()` — interactive mode, deferred to future plan

See Plan 1a for the full protocol type definitions and rationale.

### TsExecuteResult

The execution response maps to q2's `ExecuteResult` plus additional fields from Quarto 1's `ExecuteResult`:

```typescript
interface TsExecuteResult {
  markdown: string;
  supporting: string[];
  filters: string[];
  includes?: {
    inHeader: string[];
    beforeBody: string[];
    afterBody: string[];
  };
  postProcess?: boolean;
  preserve?: Record<string, string>;      // HTML chunks to protect from Pandoc
  engineDependencies?: Record<string, Array<unknown>>; // widget deps for later resolution
  pandoc?: Record<string, unknown>;       // pandoc options to merge
}
```

**Field rationale:** The Julia engine (our validation target) populates `preserve`, `engineDependencies`, `pandoc`, and `includes` via `quarto.jupyter.toMarkdown()`. These fields are essential for interactive outputs:
- `preserve` + `postProcess`: raw HTML (widgets, DataFrames) replaced with placeholders before Pandoc, restored after
- `engineDependencies`: Jupyter widget CSS/JS deps, resolved later into `PandocIncludes`
- `pandoc`: format-specific pandoc options from `toMarkdown()`
- `includes`: immediate CSS/JS includes (alternative to deferred `engineDependencies`)

q2's `ExecuteResult` struct currently has `includes` and `needs_postprocess` but not `preserve`, `engineDependencies`, or `pandoc`. These will be added to the Rust struct as part of Plan 1a. `metadata` (from Quarto 1's `ExecuteResult`) is NOT included — the Julia engine doesn't populate it.

**Note on TsExecuteOptions:** q2 provides pre-extracted `metadata` (from the
AST) and a `source_map` (byte-range entries from Plan 0's SourceInfo) in the
execute options. The engine-host harness uses these to construct the
`ExecutionTarget` and `MappedString` the engine expects — the engine never
calls `target()` or `partitionedMarkdown()`. See Plan 1a for details.

**Logs (stderr):** Engine extensions write logs to stderr. The `quarto.console.*` methods write to stderr with level prefixes so q2 can parse and display them. Engine's own `Deno.stdout.writeSync` calls are redirected to stderr (the harness reassigns stdout).

### EngineHostContext (sent once at init)

```typescript
interface EngineHostContext {
  // Paths that the QuartoAPI needs
  projectDir?: string;
  tempDir: string;
  sourceFile: string;
  resourceDir: string;    // q2's bundled resources
  runtimeDir: string;     // q2's runtime directory

  // Format info
  format: {
    pandocTo: string;       // e.g., "html", "pdf", "revealjs"
  };

  // System info
  isInteractiveSession: boolean;
  runningInCI: boolean;
  quartoVersion: string;
  pandocPath: string;         // absolute path to pandoc binary
}
```

Most QuartoAPI methods are implemented in TypeScript using this context + Deno's own APIs. No callbacks to Rust needed.

## QuartoAPI Implementation Strategy

### Implemented in TypeScript (no Rust callbacks)

| Namespace | How |
|-----------|-----|
| `quarto.path` | Use `context.resourceDir`, `context.runtimeDir` + Deno's `Deno.realPathSync()` etc. |
| `quarto.format` | Compute from `context.format.pandocTo` string in TS. Methods accept optional `Format` parameter for API compat. |
| `quarto.system` | `isInteractiveSession`/`runningInCI` from context. `execProcess` via `Deno.Command`. `pandoc` via `Deno.Command` with pandoc binary. |
| `quarto.console` | Write to stderr with level prefixes |
| `quarto.crypto` | `crypto.subtle.digest("MD5", ...)` or a small npm dep |
| `quarto.mappedString` | Harness reconstructs `MappedString` with `.map()` provenance from `source_map` byte-range entries in `TsExecuteOptions` (serialized `SourceInfo`). `fromFile`/`fromString` also available for engine's own use. |
| `quarto.markdownRegex` | From `@quarto/markdown` package (new clean implementations) |
| `quarto.jupyter` | From `@quarto/jupyter` package (new clean implementations) |
| `quarto.text` | Small utility functions, straightforward TS |

### What's NOT needed from Rust

None of the QuartoAPI methods call back to Rust. All context flows one way (Rust → Deno at init time). This keeps the protocol simple and unidirectional during execution.

## Quarto 1 API Compatibility

We are NOT targeting 100% Quarto 1 API compatibility. The `@quarto/types` package provides the interface definitions. Our implementations may differ from Quarto 1's in:
- Simplified type signatures (flattened options objects)
- Missing methods that no current engine uses (stubbed with helpful errors)
- Different behavior in edge cases (especially around YAML validation)

Engine extensions may need minor adaptation to work with q2. The Julia engine is our compatibility benchmark.

## New TypeScript Packages

### @quarto/markdown

Clean reimplementations of markdown utilities. No dependency on Quarto 1 internals.

- `extractYaml(markdown)` — YAML front matter extraction using `yaml` package
- `partition(markdown)` — split into yaml / heading / body
- `getLanguages(markdown)` — extract language specifiers from code blocks (pure regex)
- `breakQuartoMd(markdown)` — split into code/markdown cells (simplified, no YAML schema validation)

Dependencies: `yaml` (js-yaml). No tree-sitter, no schema validation.

### @quarto/jupyter

Clean reimplementation of Jupyter notebook → markdown conversion. Inspired by Quarto 1's `core/jupyter/` but written as standalone modules without the tangled dependencies.

Core function: `jupyterToMarkdown(notebook, options)` — walks notebook cells, formats outputs as markdown, handles figures, HTML preservation, widget dependencies.

Supporting modules: types, display-data (MIME dispatch), tags (cell visibility), labels (captions), preserve (HTML protection), output formatting.

Dependencies: `yaml`. No deno-dom, no tree-sitter, no Quarto 1 internal imports.

### @quarto/engine-host

The Deno-side subprocess harness. Reads JSON messages from stdin, dispatches to the loaded engine module, writes responses to stdout.

- `host.ts` — main loop: read messages, dispatch, write responses
- `quarto-api.ts` — constructs the `QuartoAPI` object from `EngineHostContext`
- `engine-loader.ts` — dynamically imports the engine TS module, validates it exports `ExecutionEngineDiscovery`

Dependencies: `@quarto/markdown`, `@quarto/jupyter`, `@quarto/types`.

## Sub-Plans

| Plan | Sessions | Dependencies | Can start |
|------|----------|-------------|-----------|
| [Plan 0: Include Expansion & SourceInfo](2026-04-18-plan0-include-expansion-and-source-info.md) | 2-3 | Nothing | Now |
| [Plan 1a: Protocol & Core](2026-04-16-plan1a-protocol-and-core.md) | 2-3 | Plan 0 | After Plan 0 |
| [Plan 1b: Extension Integration](2026-04-16-plan1b-extension-integration.md) | 1-2 | Plan 1a | After Plan 1a |
| [Plan 2: @quarto/markdown + QuartoAPI](2026-04-16-quarto-markdown-and-api.md) | 1-2 | Nothing | Now (parallel with Plan 0) |
| [Plan 3: @quarto/jupyter](2026-04-16-quarto-jupyter.md) | 2-3 | Nothing | Now (parallel with Plan 0) |
| [Plan 4: Julia Validation](2026-04-16-julia-validation.md) | 1-2 | Plans 1a, 1b, 2, 3 | After all others |
| **Total** | **9-15** | | |

### Dependency graph

```
Plan 0 (Include Expansion & SourceInfo) ─┐
    │                                     │
Plan 1a (Protocol & Core) ──────────────┐│
    │                                    ││
Plan 1b (Extension Integration) ────────┤│
                                         ││
Plan 2 (@quarto/markdown + QuartoAPI) ──┼┼──→ Plan 4 (Julia Validation)
                                         ││
Plan 3 (@quarto/jupyter) ───────────────┘│
                                          │
(Plan 0 blocks Plan 1a; Plans 2 & 3 are independent of Plan 0)
```

**Plan 0** is a prerequisite for the TS engine protocol design. It delivers
pre-engine include shortcode expansion (correctness fix for all engines) and
SourceInfo on the engine interface (parity with Quarto 1's MappedString).
Plans 2 and 3 (TypeScript packages) can proceed in parallel since they don't
touch the Rust engine interface. Plan 1a depends on Plan 0 because the
protocol types must account for source mapping decisions made in Plan 0.

**Plan 1a** delivers the protocol types, subprocess management, `ExecutionEngine`
trait extensions, `TsEngine` struct, and the Deno engine-host harness.

**Plan 1b** wires this into the extension system: `_extension.yml` engine parsing,
`deno bundle` build step, registry migration to `StageContext`, 4-phase detection
rewrite, and the echo engine end-to-end test.

Plans 2 and 3 depend on Plan 1a (they need the engine-host harness from Phase 5),
NOT on Plan 1b. Plan 4 depends on everything including 1b.

Plans 1a, 2, and 3 each have a **standalone core** that is independent:
- Plan 1a: Rust subprocess infra + engine-host harness (Phases 1-5)
- Plan 2: `@quarto/markdown` package + types (Phases 2A, 2B, 2D)
- Plan 3: `@quarto/jupyter` package (Phases 3A-3D, 3F)

However, each plan has **integration phases** that depend on Plan 1a's engine-host:
- Plan 1b Phase 3 (echo engine test) needs minimal types from Plan 2D
- Plan 2C (wire QuartoAPI namespaces into engine-host) needs Plan 1a Phase 5
- Plan 3E (wire jupyter into engine-host) needs Plan 1a Phase 5

Plan 4 integrates everything and depends on all plans being complete.

### Critical path

With parallel execution: Plan 0 (2-3 sessions) → Plan 1a (2-3 sessions, parallel with 2/3) → Plan 1b (1-2 sessions) → Plan 4 (1-2 sessions) = **6-10 sessions elapsed**.

## Key File Paths (q2)

| Component | Path |
|-----------|------|
| Engine trait | `crates/quarto-core/src/engine/traits.rs` |
| Engine registry | `crates/quarto-core/src/engine/registry.rs` |
| Engine detection | `crates/quarto-core/src/engine/detection.rs` |
| Engine execution stage | `crates/quarto-core/src/stage/stages/engine_execution.rs` |
| Existing TS packages | `ts-packages/` |
| npm workspace config | `package.json` (workspaces: `ts-packages/*`) |

## Key File Paths (quarto-cli, for reference)

**Note:** quarto-cli is a separate repository at `~/src/quarto-cli`. It is the TypeScript/Deno implementation of Quarto 1. We reference it for API definitions and implementation patterns but do not import from it.

| Component | Path |
|-----------|------|
| Julia engine | `src/resources/extension-subtrees/julia-engine/src/julia-engine.ts` |
| Julia _extension.yml | `src/resources/extension-subtrees/julia-engine/_extensions/julia-engine/_extension.yml` |
| @quarto/types | `packages/quarto-types/` |
| QuartoAPI types | `packages/quarto-types/src/quarto-api.ts` |
| ExecutionEngineDiscovery | `src/execute/types.ts` |
| fileExecutionEngine | `src/execute/engine.ts` (4-phase algorithm) |
| markdownExecutionEngine | `src/execute/engine.ts` (language scanning + claiming) |
| resolveEngines | `src/execute/engine.ts` (engine registration + ordering) |
| resolveEngineExtensions | `src/project/project-context.ts` (extension → project config) |
| languagesWithClasses | `src/core/pandoc/pandoc-partition.ts` (code block language extraction) |
| Extension schema | `src/resources/schema/extension.yml` (contributes.engines schema) |
| jupyterToMarkdown | `src/core/jupyter/jupyter.ts` |
| JupyterToMarkdownResult | `src/core/jupyter/types.ts` (includes htmlPreserve, dependencies, pandoc) |
| Markdown regex | `src/core/api/markdown-regex.ts` → `src/core/pandoc/pandoc-partition.ts`, `src/core/lib/break-quarto-md.ts` |
| Engine template | `src/resources/create/extensions/engine/src/qstart-filesafename-qend.ejs.ts` |

## Extension Build Model

Following Quarto 1's approach, engine extensions go through a **build step** before execution:

1. **Build time:** Engine extension TS source is bundled into a single `.js` file using `deno bundle` with an import map. The import map resolves:
   - `@quarto/types` → type definitions (erased during bundling, type-only imports)
   - `"path"` → `jsr:@std/path`
   - `"fs/exists"` → `jsr:@std/fs/exists`
   - `"encoding/base64"` → `jsr:@std/encoding/base64`
   All dependencies are inlined into the bundle.

2. **Runtime:** The Deno subprocess loads the bundled `.js` file via dynamic `import()`. No import map or TS transpilation needed at execution time.

The `@quarto/engine-host` harness is also bundled into a single `.js` file that includes `@quarto/markdown`, `@quarto/jupyter`, and all QuartoAPI implementations. This bundle is built using **esbuild** (matching the existing `quarto-system-runtime` pattern), checked into git at `ts-packages/quarto-engine-host/dist/engine-host.js`, and embedded in the q2 binary via `include_str!()`. At runtime, the embedded JS is written to a temp file and executed with `deno run --allow-all`.

q2 provides:
- `resources/extension-build/import-map.json` — import map for building extensions
- `resources/extension-build/deno.json` — Deno config pointing to the import map
- A `quarto build-ts-extension` command (or auto-build during render)

## Runtime Dependency

**Deno must be on PATH** to use TS engine extensions (same model as pandoc — assumed present, not bundled). q2 should:
- Check for `deno` in PATH when a TS engine is needed
- Provide a clear error message if Deno is not found
- Document the Deno requirement for engine extension users
- The core q2 binary (markdown, knitr, jupyter engines) does NOT require Deno

**Future:** Deno may be bundled with q2 when a distribution/installer pipeline is built. For now, q2 has no installer infrastructure (pandoc is also assumed on PATH). Tests that require Deno should be skipped if it's absent, following the same pattern as pandoc-dependent tests.

## Engine Discovery and Language Claiming

q2 follows Quarto 1's 4-phase engine detection algorithm, with one modernization.

### Quarto 1's algorithm (reference)

In Quarto 1, `fileExecutionEngine()` uses this 4-phase algorithm:
1. **Extension claims (`claimsFile`)**: Each engine's `claimsFile(file, ext)` is checked — `.ipynb` → jupyter, `.rmd` → knitr
2. **YAML declaration**: Check for explicit `engine:` key or engine-name top-level key in frontmatter
3. **Language scanning (`claimsLanguage`)**: Extract languages from code blocks via `languagesWithClasses()`, call each engine's `claimsLanguage(language, firstClass?)`. Returns `false` (no claim), `true` (priority 1), or a number (custom priority). Highest score wins.
4. **Fallback**: If no engine claims any language but there are non-handler languages (not `ojs`, etc.), default to Jupyter. Otherwise, use markdown engine.

`claimsLanguage` receives both the language identifier and an optional `firstClass` extracted from code block attributes (e.g., `{python .marimo}` → language="python", firstClass="marimo"). This allows engines to make class-specific claims.

Engine ordering affects ties: `_quarto.yml` `engines:` list controls priority order. User-specified engines come first; standard engines follow. First engine to achieve the highest score wins.

### q2's algorithm (modernized)

q2 implements the same 4-phase algorithm with one change:

**Modernization — Jupyter no longer claims "julia" explicitly.** In Quarto 1, Jupyter's `claimsLanguage` returned `true` for "julia", creating a conflict with the Julia engine extension (both claimed priority 1, winner depended on registration order). In q2:

- The built-in Jupyter engine does NOT explicitly claim any language via `claims_language()` (matching Quarto 1's behavior for Python, which also relied on the Phase 4 fallback). Critically, it no longer claims `"julia"` either.
- The Julia engine extension claims `"julia"`. When installed, it wins cleanly with no ordering tricks needed.
- **Phase 4 is preserved:** If no engine explicitly claims a language but there ARE code blocks with unrecognized computational languages, Jupyter is the fallback — because Jupyter can handle any language it has a kernel for. This is correct behavior, not a hack: Jupyter is a universal kernel executor. If no Julia extension is installed and a doc has `{julia}` blocks, Jupyter handles it via its Julia kernel (same as Quarto 1's end result, but without the conflicting explicit claim).
- If there are no computational code blocks at all, the document falls through to the markdown engine.

### Implementation on the `ExecutionEngine` trait

Discovery methods are added to the `ExecutionEngine` trait with defaults (Option A from design discussion):

```rust
pub trait ExecutionEngine: Send + Sync {
    // ... existing methods ...

    /// File extensions this engine can handle (e.g., [".ipynb", ".py"]).
    /// Used as a pre-filter before any claiming logic.
    fn valid_extensions(&self) -> Vec<String> { Vec::new() }

    /// Whether this engine claims a language.
    /// Returns None (no claim), or Some(priority) where higher wins.
    /// `first_class` is the first CSS class from code block attributes
    /// (e.g., "marimo" from `{python .marimo}`).
    fn claims_language(&self, _language: &str, _first_class: Option<&str>) -> Option<u32> { None }

    /// Whether this engine claims a file by extension.
    fn claims_file(&self, _file: &str, _ext: &str) -> bool { false }
}
```

Built-in engines implement these directly:
- **Jupyter**: `claims_file(".ipynb") → true`. No `claims_language` overrides — Jupyter does not explicitly claim any language via Phase 3. Instead, it acts as the Phase 4 fallback for all unclaimed computational languages (matching Quarto 1, where Python also falls through to the Phase 4 Jupyter fallback). The only Quarto 1 change: Jupyter no longer claims "julia" explicitly, removing the priority conflict with the Julia extension.
- **Knitr**: `claims_language("r") → Some(1)`, `claims_file(".rmd") → true`
- **Markdown**: returns defaults (claims nothing)
- **TsEngine**: forwards queries to the Deno subprocess

Language + class information is extracted from the **parsed AST** (not regex), since q2 already has pampa for parsing.

### Engine ordering and registration

Following Quarto 1's `resolveEngineExtensions()` + `resolveEngines()` pipeline:

1. Extension discovery scans `_extensions/` for `contributes: engines:` entries
2. Extension engines are merged into `projectConfig.engines`
3. `_quarto.yml` `engines:` list controls ordering (user-specified engines first)
4. Standard engines (knitr, jupyter, markdown) are appended after user-specified engines
5. When two engines have the same priority score for a language, iteration order wins
