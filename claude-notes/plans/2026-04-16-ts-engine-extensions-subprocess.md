# Grand Plan: TypeScript Engine Extensions for q2 (v2 — Subprocess)

## Overview

Implement TypeScript engine extensions in q2 using a **Deno subprocess** architecture. Engine extensions are TypeScript modules (following Quarto 1's API) that q2 discovers, loads via a long-lived Deno subprocess, and communicates with via a JSON message protocol over stdin/stdout.

**Goal:** A user places a TypeScript engine extension in `_extensions/my-engine/` with an `_extension.yml`, and q2 discovers it, spawns a Deno engine-host process, queries the engine for file/language claims, and delegates code cell execution to it.

**Validation target:** The Julia engine extension from Quarto 1 (`julia-engine.ts`).

**Key design choice:** Subprocess over embedded Deno. This eliminates the need to add deno ext crates (deno_fs, deno_process, deno_net, deno_crypto, etc.) to q2's binary. The engine extension runs in a full Deno environment — all standard APIs available, all Deno standard library modules importable, TypeScript transpilation handled by Deno natively. The QuartoAPI is implemented in TypeScript in a platform-agnostic `@quarto/api` package; platform I/O goes through a `PlatformHost` interface, with the Deno-specific host (`@quarto/engine-host-deno`) providing the `denoHost` that calls `Deno.readTextFileSync`, `Deno.Command`, etc. A future `@quarto/engine-host-wasm` can provide a VFS-backed host for in-browser hosting without changes to `@quarto/api`.

## Architecture

### Shared subprocess lifecycle (one Deno process per project render)

All TS engine extensions share one Deno subprocess. Each engine is loaded
via a separate `Init` message. All other messages carry `engine: "<name>"`
for routing.

```
q2 (Rust)                              engine-host (shared Deno subprocess)
─────────                              ────────────────────────────────────
spawn deno engine-host-deno.js ─────→  start, wait for init messages
                                       
send: { type: "init",        ──────→  load julia-engine.js
        enginePath: "julia..." }       call engine.init(quartoAPI), engine.launch(ctx)
recv: { type: "ready",       ←──────  engine "julia" loaded
        engineMeta: { name: "julia", ... } }

send: { type: "init",        ──────→  load marimo-engine.js
        enginePath: "marimo..." }      call engine.init(quartoAPI), engine.launch(ctx)
recv: { type: "ready",       ←──────  engine "marimo" loaded
        engineMeta: { name: "marimo", ... } }

send: { type: "claimsLanguage", ───→  route to julia instance
        engine: "julia",               call julia.claimsLanguage("julia")
        language: "julia" }
recv: { type: "claimsLanguageResult", ←── return result
        result: 1 }

send: { type: "execute",     ──────→  route to julia instance
        engine: "julia",               call julia.execute(opts)
        options: { ... } }
recv: { type: "executeResult", ←────  return ExecuteResult
        result: { markdown, supporting, ... } }

send: { type: "shutdown" }   ──────→  clean up all engines, exit
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
│   ├── quarto-engine-host-deno/  ← NEW: Deno subprocess harness (q2 native binary)
│   │   ├── src/
│   │   │   ├── host.ts           ← stdin/stdout protocol handler
│   │   │   ├── deno-host.ts      ← PlatformHost impl (Deno.* APIs)
│   │   │   ├── quarto-api.ts     ← QuartoAPI assembly from @quarto/api + denoHost
│   │   │   ├── mapped-source.ts  ← MappedString rehydration from source_map
│   │   │   └── engine-loader.ts  ← dynamic import + validation
│   │   └── package.json
│   │
│   ├── (quarto-engine-host-wasm/ ← FUTURE, out of scope: browser harness for hub-client)
│   │
│   └── quarto-api/               ← NEW: shared QuartoAPI implementations
│       ├── package.json          ← single package, subpath exports
│       └── src/
│           ├── platform.ts       ← PlatformHost interface (no impls)
│           ├── text/             ← MappedString + text utilities
│           ├── markdown/         ← extractYaml, partition, getLanguages, breakQuartoMd
│           ├── jupyter/          ← notebook → markdown + helpers (Plan 3)
│           ├── format/           ← isHtmlCompatible, isLatexOutput, …
│           ├── path/             ← dirAndStem, isQmdFile, createPath(host)
│           ├── system/           ← createSystem(host): execProcess, tempContext, …
│           ├── console/          ← info, warning, error, withSpinner
│           └── crypto/           ← md5Hash
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

// Discovery responses (separate types for language vs file claims)
{ type: "claimsLanguageResult", result: number | null }  // null=no claim, 1=default, negative=low priority. Harness converts: false→null, true→1, number→Math.trunc() to i32
{ type: "claimsFileResult", result: boolean }

// File conversion response (includes source_map for provenance back to original file)
{ type: "markdownForFileResult", result: { value: string, fileName?: string, sourceMap: TsSourceMapEntry[] } }

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

**Optional protocol message:**
- `partitionedMarkdown()` — **also on Rust `ExecutionEngine` trait** (Jupyter
  needs it for ipynb-filters). Default impl: `partition(markdownForFile(file).value)`.
  See [ipynb-filters research plan](2026-04-23-ipynb-filters-and-engine-partitioning.md).

**Harness-internal** (not protocol messages):
- `target()` — the harness checks if the TS engine implements it, calls it
  if so, uses the result (including opaque `data` cookie like kernelspec) to
  build `ExecutionTarget` for `execute()`. All Deno-side. Falls back to
  constructing from `TsExecuteOptions` fields.

**Not in protocol:**
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

This is a q2 invention — Quarto 1 engines run in-process and don't need serialized
context. `EngineHostContext` carries only static/global and project-level info.
Per-document and per-format info arrives in per-call messages (`TsExecuteOptions`, etc.).

In Quarto 1, `QuartoAPI` is a global singleton with mostly stateless utility
functions (format helpers take `Format` as parameter, not global state). The
`EngineProjectContext` passed to `launch()` carries project dir and config.
`EngineHostContext` combines the bootstrap info for both.

```typescript
interface EngineHostContext {
  // Project info (→ EngineProjectContext for launch())
  projectDir?: string;
  isSingleFile: boolean;

  // Paths for QuartoAPI construction (q2-specific, can't be derived by Deno)
  resourceDir: string;    // q2's bundled resources
  runtimeDir: string;     // q2's runtime directory
  pandocPath: string;     // absolute path to pandoc binary

  // System info for QuartoAPI (q2 is source of truth)
  isInteractiveSession: boolean;
  runningInCI: boolean;
  quartoVersion: string;
}
```

Most QuartoAPI methods are implemented in TypeScript using `context` + the platform host. No callbacks to Rust needed.

## QuartoAPI Implementation Strategy

### Implemented in TypeScript (no Rust callbacks)

Implementations live in `@quarto/api` subpaths. Platform I/O is factored
through `PlatformHost` (see Plan 2) so the same package works under
`@quarto/engine-host-deno` today and `@quarto/engine-host-wasm` later.

| Namespace | Source | Host use |
|-----------|--------|----------|
| `quarto.path` | `@quarto/api/path` — pure string helpers + `createPath(host)` | `host.realPath` for `absolute()`; otherwise none. Engine-host-deno layer adds `runtime(subdir)` / `resource(...parts)` closures over `context.runtimeDir` / `context.resourceDir`. |
| `quarto.format` | `@quarto/api/format` — pure computation from `format.pandoc.to` | None. Format info arrives per-call in `TsExecuteOptions.format`, not at init time. Matches Quarto 1 (stateless). |
| `quarto.system` | `@quarto/api/system` — `createSystem(host)` | `host.process.exec` for `execProcess`; `host.fs` for `tempContext`. Throws "not available" in environments where `host.process` is undefined. Engine-host-deno wraps `execProcess` with a `pandoc(args, stdin?)` convenience that uses `context.pandocPath`. |
| `quarto.console` | `@quarto/api/console` — pure, writes to stderr with level prefixes | None. |
| `quarto.crypto` | `@quarto/api/crypto` — `md5Hash` via Web Crypto (`crypto.subtle.digest`) or small pure-JS dep | None. Works in Deno, browser, Node. |
| `quarto.mappedString` | `@quarto/api/text` (same module as `quarto.text`). `fromFile` routed through `createMappedStringFromFile(host)`. | `host.fs.readTextFileSync` for `fromFile`. For `options.target.markdown`, engine-host-deno's `mapped-source.ts` rehydrates a `MappedString` with `.map()` provenance from the `source_map` byte-range entries in `TsExecuteOptions`. |
| `quarto.markdownRegex` | `@quarto/api/markdown` — clean reimplementations | None. Pure parsing. |
| `quarto.jupyter` | `@quarto/api/jupyter` — `createJupyter(host)` (Plan 3) | `host.fs.writeFileSync` for figure image writes; `host.fs.readTextFileSync` for `isPercentScript` / `percentScriptToMarkdown`. The rest of the jupyter conversion logic is pure. |
| `quarto.text` | `@quarto/api/text` — pure string utilities | None. |

### What's NOT needed from Rust

None of the QuartoAPI methods call back to Rust. All context flows one way
(Rust → Deno at init time, per-call options on each execute). This keeps
the protocol simple and unidirectional during execution.

### What's NOT in `@quarto/api` itself

No references to `Deno.*` or `node:*`. Those live in the platform-specific
host implementations (`@quarto/engine-host-deno/src/deno-host.ts` today,
`@quarto/engine-host-wasm/src/wasm-host.ts` in the future).

## Quarto 1 API Compatibility

We are NOT targeting 100% Quarto 1 API compatibility. The `@quarto/types` package provides the interface definitions. Our implementations may differ from Quarto 1's in:
- Simplified type signatures (flattened options objects)
- Missing methods that no current engine uses (stubbed with helpful errors)
- Different behavior in edge cases (especially around YAML validation)

Engine extensions may need minor adaptation to work with q2. The Julia engine is our compatibility benchmark.

## New TypeScript Packages

### @quarto/api

A single shared package holding clean reimplementations of every QuartoAPI
namespace's underlying logic. Organized as subpaths rather than sibling
packages — one `package.json`, one version, one dep list, `exports` map for
targeted imports. Designed to be portable: consumable today by
`@quarto/engine-host-deno`, and in the future by Quarto 1 itself.

Subpaths:

- `@quarto/api/text` — `MappedString` (type + impl), `asMappedString`,
  `mappedSubstring`, `mappedConcat`, `mappedLines`, `mappedNormalizeNewlines`,
  `mappedIndexToLineCol`, `mappedStringFromFile`, plus plain-text utilities
  (`lines`, `trimEmptyLines`, `asYamlText`, `postProcessRestorePreservedHtml`).
  Powers both `quarto.text` and `quarto.mappedString` on the runtime API surface.
- `@quarto/api/markdown` — `extractYaml`, `partition`, `pandocAttrParseText`,
  `getLanguages`, `breakQuartoMd`. Powers `quarto.markdownRegex`.
- `@quarto/api/jupyter` — `jupyterToMarkdown`, `isPercentScript`,
  `percentScriptToMarkdown`, `assets`, `resultIncludes`,
  `resultEngineDependencies`, plus supporting modules (display-data, tags,
  labels, preserve, widgets, pandoc-id, cell-options). Powers `quarto.jupyter`.
  Subject of Plan 3.
- `@quarto/api/format` — `isHtmlCompatible`, `isLatexOutput`, `isMarkdownOutput`,
  `isIpynbOutput`, etc. Pure computation from `pandoc.to` strings.
- `@quarto/api/path` — `dirAndStem`, `isQmdFile`, `toForwardSlashes`,
  `inputFilesDir` as pure exports; `createPath(host)` for host-dependent
  `absolute()`.
- `@quarto/api/system` — `createSystem(host)` returning `execProcess`,
  `tempContext`, `onCleanup`, `isInteractiveSession`, `runningInCI`. All
  I/O goes through the platform host — in Deno, `host.process.exec` wraps
  `Deno.Command`; in a future WASM host, `execProcess` throws "not available".
- `@quarto/api/console` — `info`, `warning`, `error`, `withSpinner` (stderr writers).
- `@quarto/api/crypto` — `md5Hash`.

**Runtime-platform expectations:** `@quarto/api` itself contains **no**
references to `Deno.*`, `node:*`, or other platform-specific APIs. All I/O
(file read/write, subprocess execution, path canonicalization) goes through
a small `PlatformHost` interface that the consumer plugs in. This lets the
same `@quarto/api` package serve two environments:

- `@quarto/engine-host-deno` — the Deno subprocess harness delivered by
  Plan 1a. Provides a `denoHost` that calls `Deno.readTextFileSync`,
  `Deno.Command`, etc.
- `@quarto/engine-host-wasm` — **future work**, not part of these plans —
  the in-browser harness for hub-client. Would provide a `wasmHost` backed
  by q2's VFS (`vfsReadFile`, `vfsAddFile`, …). Subprocess-dependent
  QuartoAPI methods (`quarto.system.execProcess`, `quarto.system.pandoc`)
  would throw "not available in this environment".

See Plan 2 for the `PlatformHost` interface definition and which submodules
are pure vs. host-dependent.

**Bootstrap:** No registry pattern. `@quarto/engine-host-deno` imports the
submodules directly and builds the `QuartoAPI` object as plain nested
record — the QuartoAPI registry/singleton infrastructure from Quarto 1
(`src/core/api/registry.ts`, `register.ts`) is **not** being ported.
Implementations only.

Dependencies: `yaml` (used by `markdown` and `jupyter` for YAML parsing).

### @quarto/engine-host-deno

The Deno-side subprocess harness — q2-specific glue for the native binary,
never shared with Q1. Reads JSON messages from stdin, dispatches to the
loaded engine module, writes responses to stdout. Named `-deno` explicitly
to make room for a future `@quarto/engine-host-wasm` sibling.

- `host.ts` — main loop: read messages, dispatch, write responses
- `deno-host.ts` — the `PlatformHost` implementation backed by Deno APIs
  (`Deno.readTextFileSync`, `Deno.Command`, `Deno.realPathSync`, etc.)
- `quarto-api.ts` — imports `@quarto/api/*` submodules, threads `denoHost`
  through the host-taking factories, assembles the `QuartoAPI` object from
  `EngineHostContext`. Wires both `quarto.text` and `quarto.mappedString`
  from the same `@quarto/api/text` module.
- `mapped-source.ts` — rehydrates a `MappedString` from the `source_map`
  byte-range entries in `TsExecuteOptions`. Uses a base-per-file cache so
  multiple pieces sharing a source file share one base `MappedString`
  object. q2-specific (needed because `source_map` crossed the protocol
  boundary as data, not in-memory references).
- `engine-loader.ts` — dynamically imports the engine TS module, validates
  it exports `ExecutionEngineDiscovery`.

Dependencies: `@quarto/api`, `@quarto/types`.

### @quarto/engine-host-wasm (future, out of scope)

The in-browser equivalent for hub-client. Would provide a `wasmHost`
backed by q2's VFS JS bindings and run inside a Web Worker (or equivalent
sandbox — the mechanism is an open design question). Not part of Plans 1-4.
Called out here to fix the naming and to document that the `PlatformHost`
abstraction in Plan 2 is what enables it without rework to `@quarto/api`.

## Sub-Plans

| Plan | Sessions | Dependencies | Can start |
|------|----------|-------------|-----------|
| [Plan 0: Include Expansion & SourceInfo](2026-04-18-plan0-include-expansion-and-source-info.md) | 2-3 | Nothing | Now |
| [Plan 1a: Protocol & Rust Core](2026-04-16-plan1a-protocol-and-core.md) | 1-2 | Plan 0 | After Plan 0 |
| [Plan 1b: @quarto/engine-host-deno (Deno harness)](2026-04-16-plan1b-engine-host-deno.md) | 1 | Plan 1a Phase 1 (protocol schema) | After Plan 1a Phase 1 |
| [Plan 1c: Extension Integration & E2E](2026-04-16-plan1c-extension-integration.md) | 1-2 | Plans 1a, 1b | After Plans 1a + 1b |
| [Plan 2: @quarto/api (text, markdown, utilities) + QuartoAPI assembly](2026-04-16-quarto-markdown-and-api.md) | 1-2 | Nothing | Now (parallel with Plan 0) |
| [Plan 3: @quarto/api/jupyter](2026-04-16-quarto-jupyter.md) | 2-3 | Plan 2A (package skeleton) | After Plan 2A |
| [Plan 4: Julia Validation](2026-04-16-julia-validation.md) | 1-2 | Plans 1a, 1b, 1c, 2, 3 | After all others |
| **Total** | **9-15** | | |

### Dependency graph

```
Plan 0 (Include Expansion & SourceInfo)
    │
    ▼
Plan 1a (Protocol & Rust Core)
    │      │
    │      └─(Phase 1: protocol schema frozen)─→ Plan 1b (@quarto/engine-host-deno)
    │                                                │
    └────────────────────┬───────────────────────────┘
                         ▼
                   Plan 1c (Extension Integration & E2E)
                         │
                         │
Plan 2 (@quarto/api: text, markdown, utilities) ─┐
                         │                        ├──→ Plan 4 (Julia Validation)
Plan 3 (@quarto/api/jupyter) ────────────────────┘
```

**Plan 0** is a prerequisite for the TS engine protocol design. It delivers
pre-engine include shortcode expansion (correctness fix for all engines) and
SourceInfo on the engine interface (parity with Quarto 1's MappedString).
Plans 2 and 3 (TypeScript packages) can proceed in parallel since they don't
touch the Rust engine interface. Plan 1a depends on Plan 0 because the
protocol types must account for source mapping decisions made in Plan 0.

**Plan 1a** delivers the Rust-side infrastructure: protocol types,
subprocess management, `ExecutionEngine` trait extensions, `TsEngine` struct.

**Plan 1b** is the Deno-side harness (`@quarto/engine-host-deno` package):
esbuild bundle, `host.ts` main loop, `deno-host.ts` PlatformHost impl,
`mapped-source.ts` MappedString rehydration, `quarto-api.ts` stub,
`engine-loader.ts`. Gated only on Plan 1a Phase 1 (the frozen JSON schema);
otherwise runs in parallel with 1a Phases 2-4.

**Plan 1c** wires 1a + 1b into the extension system: `_extension.yml`
engine parsing, `deno bundle` build step, registry migration to
`StageContext`, 4-phase detection rewrite, and the echo engine end-to-end
test (which exercises the full stack).

Plans 1a, 1b, 2, and 3 each have a **standalone core** that is independent:
- Plan 1a: Rust infrastructure (Phases 1-4)
- Plan 1b: Deno harness package (Phases 1-4 of 1b, its own numbering)
- Plan 2: `@quarto/api` package skeleton + `text/` and `markdown/` subpaths + types (Phases 2A, 2B, 2D)
- Plan 3: `@quarto/api/jupyter` subpath (Phases 3A-3D, 3F)

Plan 3 depends on Plan 2 creating the `@quarto/api` package (package.json,
exports map, tsconfig); after that, `jupyter/` is just another subdirectory.

**Integration phases** that depend on Plan 1b's engine-host package:
- Plan 1c Phase 3 (echo engine E2E test) needs Plan 1b fully working plus
  minimal types from Plan 2D
- Plan 2C (wire QuartoAPI namespaces into engine-host) replaces Plan 1b's stubs
- Plan 3E (wire jupyter into engine-host) replaces Plan 1b's jupyter stub

Plan 4 integrates everything and depends on all plans being complete.

### Critical path

With parallel execution: Plan 0 (2-3 sessions) → Plan 1a Phase 1 (schema
freeze) → Plans 1a Phases 2-4, 1b, 2, and 3 in parallel → Plan 1c (1-2
sessions) → Plan 4 (1-2 sessions) = **6-10 sessions elapsed**.

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

The `@quarto/engine-host-deno` harness is also bundled into a single `.js` file that includes `@quarto/api` (all subpaths) and the harness glue. This bundle is built using **esbuild** (matching the existing `quarto-system-runtime` pattern), checked into git at `ts-packages/quarto-engine-host-deno/dist/engine-host-deno.js`, and embedded in the q2 binary via `include_str!()`. At runtime, the embedded JS is written to a temp file and executed with `deno run --allow-all`.

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
    /// Negative values mean "I'll take this if no one else will."
    /// `first_class` is the first CSS class from code block attributes
    /// (e.g., "marimo" from `{python .marimo}`).
    fn claims_language(&self, _language: &str, _first_class: Option<&str>) -> Option<i32> { None }

    /// Whether this engine claims a file by extension.
    fn claims_file(&self, _file: &str, _ext: &str) -> bool { false }
}
```

Built-in engines implement these directly:
- **Jupyter**: `claims_file(".ipynb") → true`. No `claims_language` overrides — Jupyter does not explicitly claim any language via Phase 3. Instead, it acts as the Phase 4 fallback for all unclaimed computational languages (matching Quarto 1, where Python also falls through to the Phase 4 Jupyter fallback). **Deliberate q2 interface change:** Jupyter no longer claims "julia" explicitly (Quarto 1 did this as a backward-compatibility hack), removing the priority conflict with the Julia extension.
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
