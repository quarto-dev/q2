# Grand Plan: TypeScript Engine Extensions for q2 (v2 — Subprocess)

## Overview

Implement TypeScript engine extensions in q2 using a **Deno subprocess** architecture. Engine extensions are TypeScript modules (following Quarto 1's API) that q2 discovers, loads via a long-lived Deno subprocess, and communicates with via a **multiplexed JSON message protocol over the subprocess's stdin/stdout** (each frame carries an `id`; cross-engine requests run concurrently on the Deno event loop, serialized per engine instance). The shared subprocess is reached concurrently because **Pass-2 render is parallel** — see `claude-notes/designs/engine-host-concurrency.md`. (Moving the protocol off stdout onto loopback TCP, to delete the `console.log` footgun, is the deferred **Phase 1.6** cleanup.)

**Goal:** A user places a TypeScript engine extension in `_extensions/my-engine/` with an `_extension.yml`, and q2 discovers it, spawns a Deno engine-host process, queries the engine for file/language claims, and delegates code cell execution to it.

**Validation target:** The Julia engine extension from Quarto 1 (`julia-engine.ts`).

**Key design choice:** Subprocess over embedded Deno. This eliminates the need to add deno ext crates (deno_fs, deno_process, deno_net, deno_crypto, etc.) to q2's binary. The engine extension runs in a full Deno environment — all standard APIs available, all Deno standard library modules importable, TypeScript transpilation handled by Deno natively. The QuartoAPI is implemented in TypeScript in a platform-agnostic `@quarto/api` package; platform I/O goes through a `PlatformHost` interface, with the Deno-specific host (`@quarto/engine-host-deno`) providing the `denoHost` that calls `Deno.readTextFileSync`, `Deno.Command`, etc. A future `@quarto/engine-host-wasm` can provide a VFS-backed host for in-browser hosting without changes to `@quarto/api`.

## Architecture

### Project-render integration (post-merge with `main`)

Since these plans were drafted, the **website-project epic** has landed on
`main`. q2 now drives multi-document projects through a two-pass
orchestrator (`crates/quarto-core/src/project/orchestrator.rs`):

- **Pass 1** advances every file to a fixed `DocumentProfile` checkpoint
  (post-`MetadataMergeStage`, post-`IncludeExpansionStage`,
  pre-mutation). Pass 1 collects `Vec<DocumentProfile>` into a
  `ProjectIndex` shared across the rest of the render.
- **Pass 2** runs the full per-file render with `ProjectIndex` available
  via `StageContext.project_index: Option<Arc<ProjectIndex>>`.

Single-document renders go through the same orchestrator with
`DefaultProjectType` — there is one code path.

The TS-engine extension lifecycle integrates as follows:

- **`Arc<EngineRegistry>`** is built once at `ProjectContext`
  construction time (the natural pairing for "extensions discovery →
  registry"), threaded into `RenderContext` and onto each per-file
  `StageContext`. `EngineExecutionStage` becomes stateless and reads
  `ctx.registry`. Plan 1c specifies the migration; placement is
  `ProjectContext`, not per-file `StageContext::new()`.
- **`Arc<TsEngineHost>`** is similarly project-scoped: one host spans
  Pass 1 + Pass 2 + every per-file render. Spawned lazily on first
  need. `EngineClaimsFileStage` (Plan 1c) may trigger spawn during
  Pass 1 if a TS engine claims a non-QMD file via `claims_file`. For
  the common case where Pass 1 only reads QMD frontmatter, the
  subprocess does not start until Pass 2's first execute.
- **`DocumentProfile` subsumes Q1's `partitionedMarkdown` use cases.**
  Project indexing, listings, sidebar generation, link rewriting,
  format-resolution YAML harvest — all read `DocumentProfile`, not
  engine-side partition output. This is why `partitionedMarkdown` is
  not in the protocol.
- **HTML dependencies emitted by engines flow through `store_html_dependencies`**,
  which on `main` already tags engine deps with `ArtifactScope::Project`.
  Website renders dedupe to `_site/site_libs/...` automatically; single-doc
  renders treat the scope as a per-page directory with byte-identical
  pre-Phase-5 behavior.

References: `crates/quarto-core/src/document_profile.rs`,
`crates/quarto-core/src/project/index.rs`,
`crates/quarto-core/src/project/orchestrator.rs`,
`crates/quarto-core/src/dependency.rs`,
`claude-notes/designs/document-profile-contract.md`,
`claude-notes/plans/2026-04-23-website-project-epic.md`.

### Multi-engine resolution (post-merge with `main`)

Since these plans were drafted, **sequential multi-engine execution**
(bd-5yff4) and **engine capture/replay** (bd-45yw, bd-5qnj) have also landed
on `main`. A document may declare an ordered list of engines
(`engine: [knitr, jupyter]`); `EngineExecutionStage` threads the AST through
them in turn (each engine consumes the previous engine's output), and every
engine invocation is recorded as an `EngineCapture` (replayed by playing the
recorded captures back in order, **not** by re-resolving).

This epic supplies the missing *coordination* layer on top of that mechanism:
how q2 decides which engine(s) run and which engine runs which cell. The full
model — the `LanguageClaim` kinds (`Primary`/`Interop`/`Fallback`), the
resolution tiers, per-language ownership, `handled_languages` enforcement, and
the failure model — is the design contract
[`claude-notes/designs/engine-resolution.md`](../designs/engine-resolution.md);
the "Engine Discovery and Language Claiming" section below summarizes the parts
the protocol and trait surfaces depend on. Engine **resolution runs in Pass 2**
(engine `claims_*` must not load expensive TS engines merely to index a doc in
Pass 1); only the pre-parse **file-claim** half runs in Pass 1. A future
**per-doc lift** of resolution into Pass 1 (for docs whose engines all resolve
load-free) is researched in
[Plan 6](2026-06-29-plan6-pass1-engine-resolution.md) — additive, post-1c.

### Shared subprocess lifecycle (one Deno process per project render)

All TS engine extensions share one Deno subprocess. Engine lifecycle in the
subprocess is **two-step**: `LoadEngine` runs the engine module's `import()`
and exposes the discovery surface (~10–50 ms); `LaunchEngine` calls
`engine.launch(project)`, which **constructs the `ExecutionEngineInstance`
object — cheap (~0)**, matching Quarto 1, where `launch()` is a synchronous
object-literal construction that starts no daemon. The expensive engine startup
(Julia control server / Jupyter kernel: 5+ s) happens **lazily inside the
engine's `execute()` on the first call**, and is amortized across renders by the
external daemon, which is keyed by a transport file in the runtime directory and
survives even a Deno-subprocess respawn (reconnect, not relaunch). The two-step
split exists so project scans that only run discovery never pay launch or daemon
cost — **not** because launch itself is expensive. Discovery messages need only
`LoadEngine`; instance methods need `LaunchEngine`. All non-lifecycle messages
carry `engine: "<name>"` for routing.

```
q2 (Rust)                              engine-host (shared Deno subprocess)
─────────                              ────────────────────────────────────
spawn deno engine-host-deno.js ─────→  start, wait for messages
send: { type: "init",        ──────→  store process-stable global config
        global: { ... } }              (resource/runtime/data dirs, pandoc, …)

send: { type: "loadEngine",  ──────→  await import("julia-engine.js")
        enginePath: "julia..." }       call engine.init(quartoAPI) if present
recv: { type: "loaded",      ←──────  return discovery surface (static)
        discovery: { name: "julia", validExtensions: [...],
                     generatesFigures, canFreeze, quartoRequired } }

send: { type: "claimsLanguage", ───→  route to julia discovery (no launch)
        engine: "julia",               call julia.claimsLanguage("julia")
        language: "julia" }
recv: { type: "claimsLanguageResult", ←── return result
        result: 1 }

send: { type: "launchEngine", ─────→  call julia.launch(project)
        engine: "julia",               (cheap object construction, ~0;
        project: { ... } }             no daemon starts here)
recv: { type: "launched",    ←──────  return instance metadata
        instance: { canFreeze: true } }

send: { type: "execute",       ────→  route to julia instance — Julia daemon
        engine: "julia",               starts lazily on first execute (~5s),
        options: { ..., dependencies } } reused after
recv: { type: "executeResult", ←────  htmlDependencies resolved inline when
        result: { markdown, ...,        dependencies:true; else a deferred
                  htmlDependencies,      engineDependencies map the orchestrator
                  engineDependencies } }  resolves via a "dependencies" message

(deferred-deps path, book/project only — orchestrator-driven:)
send: { type: "dependencies", ─────→  call julia.dependencies(opts) at the
        engine: "julia",               merged output
        options: { ... } }
recv: { type: "dependenciesResult", ←─ return { includes }
        result: { includes } }

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
│   │   │   ├── host.ts           ← control-socket protocol handler (non-blocking, multiplexed)
│   │   │   ├── framing.ts        ← readFrames / writeFrame over stdin/stdout (Phase 1.5; +connectControl/TCP in 1.6)
│   │   │   ├── deno-host.ts      ← PlatformHost impl (Deno.* APIs)
│   │   │   ├── quarto-api.ts     ← QuartoAPI assembly from @quarto/api + denoHost
│   │   │   ├── mapped-source.ts  ← MappedString rehydration from source_map
│   │   │   └── engine-loader.ts  ← dynamic import + validation
│   │   └── package.json
│   │
│   ├── (quarto-engine-host-wasm/ ← FUTURE, out of scope: browser harness for hub-client)
│   │
│   ├── quarto-api/               ← NEW: shared QuartoAPI implementations
│   │   ├── package.json          ← single package, subpath exports
│   │   └── src/
│   │       ├── config/           ← metadata-partition key lists (Plan 2A foundation)
│   │       ├── platform/         ← PlatformHost interface (Plan 2A §2aa)
│   │       ├── text/             ← text utilities (Plan 2A §2aa)
│   │       ├── markdownRegex/    ← extractYaml, partition, getLanguages, breakQuartoMd (Plan 2A §2aa)
│   │       ├── mappedString/     ← MappedString + .map() provenance (Plan 2A §2aa)
│   │       ├── jupyter/          ← notebook → markdown + helpers (Plan 3)
│   │       ├── format/           ← isHtmlCompatible, isLatexOutput, … (Plan 2A §2aa)
│   │       ├── path/             ← dirAndStem, isQmdFile, … (Plan 2A §2aa; runtime/resource/dataDir bodies = Plan 2)
│   │       ├── system/           ← execProcess, tempContext, … (Plan 2A §2aa; pandoc/checkRender bodies = Plan 2)
│   │       ├── console/          ← info, warning, error, withSpinner (Plan 2A §2aa)
│   │       └── crypto/           ← md5Hash (Plan 2A §2aa)
│   │
│   └── quarto-types/             ← NEW: engine-author types, vendored from Q1 (Plan 2A; refined in 2E)
│
└── quarto-cli (reference only, at external-sources/quarto-cli)
    └── packages/quarto-types/    ← Q1 source we vendor from (read-only reference)
```

## Protocol Design

The full per-surface map (every Q1 engine field/method and its q2 disposition) is
`claude-notes/designs/engine-api-surface.md`; the actionable protocol items live in RTQ
(`2026-06-25-plan1a-return-to-q1.md`: Item A, ENG-1, FC-1, FC-2).

### Message format

All messages are JSON objects exchanged as **newline-framed `Request`/`Response`
envelopes over the subprocess's stdin/stdout** (Phase 1.5; Phase 1.6 moves them
to loopback TCP). Each frame is `{ id: number, msg: {...} }` where `msg` has a
`type` field; the `id` correlates a response to its request so many requests can
be in flight at once. The shapes below show the inner `msg`.

**Rust → Deno (`Request.msg`):**
```typescript
// One-time process init (sent once at spawn, before any loadEngine)
{ type: "init", global: HostGlobalConfig }   // process-stable: resource/runtime/data dirs, pandoc, version, interactive/CI

// Two-step lifecycle
{ type: "loadEngine", enginePath: string }                                // cheap: import()
{ type: "launchEngine", engine: string, project: EngineProjectContext }   // cheap: launch() builds the instance (~0); daemon starts later, in execute()

// Discovery (only requires loadEngine)
{ type: "claimsLanguage", engine: string, language: string, firstClass?: string }
{ type: "claimsFile", engine: string, file: string, ext: string }

// Instance methods (require launchEngine)
{ type: "markdownForFile", engine: string, file: string }
{ type: "execute", engine: string, options: TsExecuteOptions }            // options.dependencies selects inline vs deferred
{ type: "dependencies", engine: string, options: TsDependenciesOptions }  // deferred-deps round-trip, orchestrator-driven at the merged output
{ type: "intermediateFiles", engine: string, input: string }

// Lifecycle + cancellation
{ type: "shutdown" }
{ type: "cancel", target: number }   // Phase 1.5: cooperatively abort the in-flight request whose id == target
```

**Deno → Rust (`Response.msg`):**
```typescript
// Lifecycle responses
{ type: "loaded",   discovery: { name, validExtensions, generatesFigures, canFreeze, quartoRequired } }  // static, discovery-tier (ENG-1/DQ-4)
{ type: "launched", instance:  { canFreeze } }                                                           // canFreeze also rides the instance (Q1 has both tiers)

// Discovery responses
{ type: "claimsLanguageResult", result: { kind: "primary" | "interop" | "fallback", priority?: number } | null }  // null=no claim. Harness normalizes the boolean|number shorthand: false→null, true→Primary(1), number n→Primary(n) (negative = low-priority primary, NEVER interop). Interop/Fallback only via the object. See engine-resolution.md §3.
{ type: "claimsFileResult", result: boolean }

// Instance method responses
{ type: "markdownForFileResult", result: { value: string, fileName?: string, sourceMap: TsSourceMapEntry[] } }
{ type: "executeResult", result: TsExecuteResult }       // htmlDependencies inline, or a deferred engineDependencies map
{ type: "dependenciesResult", result: { includes: TsPandocIncludes } }
{ type: "intermediateFilesResult", result: string[] | undefined }

// Cancellation + errors
{ type: "cancelled" }                                     // Phase 1.5: delivered under the cancelled request's id
{ type: "error", message: string, stack?: string }
```

**Harness-internal** (not protocol messages, never reach the Rust side):
- `target()` — the harness checks if the TS engine implements it, calls it
  if so, uses the result (including opaque `data` cookie like kernelspec) to
  build `ExecutionTarget` for `execute()`. All Deno-side. Falls back to
  constructing from `TsExecuteOptions` fields.

`dependencies()` is **not** harness-internal: it is a first-class wire verb
(`dependencies` above), driven by q2's render orchestrator at the merged output —
mirroring Q1 `render.ts:90-109`. The harness is a thin pass-through to
`instance.dependencies(opts)`; it does **not** fold the call into `execute`
(that earlier design was un-Q1 and could not resolve at a merged output — see
RTQ FC-2).

**Deferred until q2 grows callers** (no q2 caller exists yet, so neither a
trait method nor a protocol message is added now):
- `filterFormat`, `executeTargetSkipped`, `postprocess`, `canKeepSource`,
  `postRender`. When added, they'll appear at both layers (trait method +
  protocol message) using q2-native types.
- `partitionedMarkdown`. Q1 used this to expose filter-aware notebook
  metadata to project-indexing and format-resolution callers. q2 has no
  caller for it: `DocumentProfile` (post-merge, pre-mutation pipeline
  checkpoint) carries the title / heading / draft data Q1 read via
  `partitionedMarkdown`, and Q1's pre-execute filter-YAML harvest is
  collapsed into the natural `MetadataMergeStage` cascade once filters
  run inside `markdown_for_file`. See
  `claude-notes/plans/2026-04-23-ipynb-filters-and-engine-partitioning.md`
  for the q2 ipynb-filter design.

**Not in protocol:**
- `run()` — interactive mode, deferred to future plan

See plan1a-protocol for the full protocol type definitions and rationale.

### TsExecuteResult

The execution response forwards Q1's full `ExecuteResult` shape. q2 honors the
fields it has a consumer for; the rest ride as `#[serde(default)]` **carriers**
(FC-1/FC-2), inert until a feature reads them — "defer the feature, not the
infrastructure."

```typescript
interface TsExecuteResult {
  markdown: string;
  supporting: string[];
  filters: string[];
  includes?: { inHeader: string[]; beforeBody: string[]; afterBody: string[] };
  htmlDependencies: Array<{ name: string; stylesheets: string[]; scripts: string[] }>;  // q2-native structured deps
  // Carried per FC-1/FC-2, inert until a consumer reads them:
  engineDependencies?: Record<string, unknown[]>;  // deferred-deps map (FC-2)
  metadata?: Record<string, unknown>;
  pandoc?: Record<string, unknown>;                // loose map — NOT a typed FormatPandoc
  resourceFiles?: string[];
  preserve?: Record<string, string>;
  postProcess?: boolean;
}
```

**Deferred-deps are orchestrator-driven, not folded.** With `dependencies:true`
(the v1 default) the engine resolves deps inline into `includes` and
`engineDependencies` stays empty. With `dependencies:false` the engine **returns**
the deferred `engineDependencies` map, and q2's render orchestrator later sends a
`dependencies` message per engine **at the merged output** and merges each
`DependenciesResult.includes` — mirroring Q1 `render.ts:90-109`. The harness does
not call `dependencies()` itself (RTQ FC-2). No v1/Julia caller sends `false`; the
deferred path is present-but-unexercised until book/project rendering and Plan 3E's
`quarto.jupyter.widgetDependencyIncludes` land.

**`html_dependencies` on q2's `ExecuteResult` struct** is the q2-native
landing site (plan1a-engine Phase 3). It reuses the existing
`quarto-core::dependency::HtmlDependency` type — same producer-side shape
that Lua filters already emit, same `store_html_dependencies` consumer path
in `EngineExecutionStage`. `metadata`/`pandoc`/`resourceFiles`/`preserve`/
`postProcess` are **carried but inert** (FC-1) — Julia populates none of them;
their consumers (e.g. a `preserve`-reading AST transform for `postprocess`
recovery) land with the features that need them. See
`claude-notes/plans/2026-04-18-html-js-deps-design.md` for the broader JS-deps
story.

**Note on TsExecuteOptions:** q2 provides pre-extracted `metadata` (from the
AST) and a `source_map` (byte-range entries from Plan 0's SourceInfo) in the
execute options. The engine-host harness uses these to construct the
`ExecutionTarget` and `MappedString` the engine expects. The engine never
calls `target()` (harness-internal). `partitionedMarkdown()` is not invoked
either — q2's `DocumentProfile` checkpoint covers Q1's project-indexing
and format-resolution use cases for that method. The harness also receives
`handled_languages` — the per-engine *leave-alone* set the resolver derives
from ownership (`HANDLED_LANGUAGES ∪ the languages the other engines in this
sequence own`), not a static constant; the engine executes only the cells it
owns and passes the rest through unchanged (engine-resolution.md §5). See
plan1a-protocol for details.

**Logs (v1: stderr is diagnostic; stdout is the protocol):** in v1 the protocol
runs on stdout, so **stderr** is the diagnostic stream, forwarded to q2's
logging. `quarto.console.*` emits level prefixes (`[INFO]`/`[WARN]`/`[ERROR]`)
to stderr that q2 routes accordingly. Engines must **not** write to stdout —
`console.log`/`console.info` corrupt the protocol and the Rust side SIGKILLs on
a non-`Response` line (naming `console.log` as the likely cause) — and must
**not** read stdin (the protocol *input* channel; reading it steals frames). The
harness captures `Deno.stdout` for protocol writes and does not override
`console.*`; protection is by contract. **Phase 1.6** moves the protocol to loopback TCP,
after which stdout is diagnostic-only and `console.log` is harmless. See Plans
1a and 1b for the full contract.

### Init and LaunchEngine payloads (Item A)

Serialized context is a q2 invention — Quarto 1 engines run in-process. q2 splits
the bootstrap info Q1 keeps as process globals + project context into **two frames,
each on the lifetime it matches** (RTQ Item A / DQ-7):

- **`Init { global: HostGlobalConfig }`** — sent **once at spawn**, before any
  `loadEngine`. Process-stable config q2 owns and Deno can't derive. The
  `@quarto/api` `path`/`system` factories close over it, so
  `path.runtime`/`resource`/`dataDir`/`system.pandoc` resolve **immediately**
  (ambient, like Q1's `resourcePath()`/`quartoRuntimeDir()` — never gated).

  ```typescript
  interface HostGlobalConfig {
    resourceDir: string;          // q2's bundled resources
    runtimeDir: string;           // q2's runtime directory
    dataDir: string;
    pandocPath: string;           // absolute path to pandoc binary
    isInteractiveSession: boolean;
    runningInCI: boolean;
    quartoVersion: string;
  }
  ```

- **`LaunchEngine { engine, project: EngineProjectContext }`** — sent **per render**,
  captured in the launched instance's closure (pure Q1 — `engine.launch(EngineProjectContext)`).

  ```typescript
  interface EngineProjectContext {
    projectDir?: string;
    isSingleFile: boolean;
    config?: { engines?: string[]; project?: { "output-dir"?: string } };  // values, not callbacks (DQ-5)
    outputDir?: string;
  }
  ```

`Init` = process-stable global config; `LaunchEngine` = render-scoped project context.
The engine-facing API stays exactly Q1; only the wire is split. This is the enabler for
reusing one subprocess across renders (`Init` once, project per `launchEngine`). The
earlier single combined `EngineHostContext` — one shared mutable `HostState.context` slot
gated until first launch — is removed: it conflated process-global with project-scoped state
and had no Q1 basis (RTQ Item A).

Most QuartoAPI methods are implemented in TypeScript using the `Init` global + the launch
`project` + the platform host. No callbacks to Rust needed.

## QuartoAPI Implementation Strategy

### Implemented in TypeScript (no Rust callbacks)

Implementations live in `@quarto/api` subpaths. Platform I/O is factored
through `PlatformHost` (see Plan 2) so the same package works under
`@quarto/engine-host-deno` today and `@quarto/engine-host-wasm` later.

| Namespace | Source | Host use |
|-----------|--------|----------|
| `quarto.path` | `@quarto/api/path` — pure string helpers + `createPath(host)` | `host.realPath` for `absolute()`; otherwise none. The `runtime(subdir)` / `resource(...parts)` closures close over the `Init` global config's `runtimeDir` / `resourceDir`. |
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
  Plan 1b. Provides a `denoHost` that calls `Deno.readTextFileSync`,
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
  through the host-taking factories, assembles the `QuartoAPI` object from the
  `Init` global config (and the launch `project`). Wires both `quarto.text` and `quarto.mappedString`
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
| [Plan 0: Include Expansion & SourceInfo](2026-04-18-plan0-include-expansion-and-source-info.md) | 2-3 | Nothing | ✓ **Complete** |
| [Plan 2A: TS package foundations (@quarto/api skeleton+config, @quarto/types vendor)](2026-04-16-plan2a-quarto-api-foundation.md) | ~1 + §2aa | Nothing (npm workspace only) — **independent root, peer of plan1a-protocol; blocks Plan 1b, 1b.1, 2, 3** | ✓ **Complete** (foundation + §2aa runtime surface landed long ago) |
| [Plan 1a-protocol: JSON message types](2026-04-16-plan1a-protocol.md) | 1 | Plan 0 | ✓ **Complete** (lone open box is a cross-ref note) |
| [Plan 1a-host: Subprocess + transport](2026-04-16-plan1a-host.md) | 1 | plan1a-protocol | ✓ **Complete** (LANDED host-side 2026-06-24; 46/46) |
| [Plan 1a-engine: TsEngine + trait extensions](2026-04-16-plan1a-engine.md) | 1 | plan1a-protocol, plan1a-host | ✓ **Complete** (open boxes are a 1c-exercised E2E gate + a cross-ref note) |
| [**RTQ: plan1a Return-to-Q1 (course correction over the 1a series)**](2026-06-25-plan1a-return-to-q1.md) | done | amends plan1a-protocol/host/engine | ✓ **Complete** (all 6 items landed + reviewed READY TO MERGE, 2026-06-29; lone open box = deferred book-feature consumer) |
| [**Plan 1a-host bugs (q2-introduced defects, HOST-1/2)**](2026-06-26-plan1a-host-bugs.md) | <1 | **none** (independent) | ✓ **Complete** (2026-06-30; 5/5) |
| [Plan 1b: @quarto/engine-host-deno (Deno harness)](2026-04-16-plan1b-engine-host-deno.md) | 2-3 | plan1a-protocol, **RTQ (Item A + ENG-1 + FC-1 + FC-2, incl. B3 code half)**, Plan 2A | ✓ **Complete** (2026-06-30; 74/74, whole-branch review READY TO MERGE) |
| [**Plan 1b.1: MappedString `segments()` accessor (foundation fix)**](2026-06-30-mapped-string-segments.md) | <1 | Plan 2A §2aa (mappedString) + Plan 1b | ✓ **Complete** (landed 2026-06-30; independent of Plan 1c) |
| [Plan 1c: Extension Integration & E2E](2026-04-16-plan1c-extension-integration.md) | 1-2 | plan1a-engine, plan1a-host, Plan 1b | ✓ **Complete** (T1–T15 + downstream fix, review-clean READY TO MERGE; 4 disclosed deferrals tracked in Plan 1c.2) |
| [**Plan 1c.2: TS engine-extensions loose ends (deferred follow-ups from 1c)**](2026-07-01-plan1c2-engine-extensions-loose-ends.md) | TBD | Plan 1c (parent) | **post-1c**; not started (disclosed 1c deferrals — **not** 1c merge-blockers) |
| [Plan 2: @quarto/api deferred launch-context bodies + @quarto/types refinements](2026-04-16-quarto-markdown-and-api.md) | 2-3 | Plan 2A §2aa + Plan 1b | ✓ **Complete** (impl + `cargo xtask verify` green; the one open box is a deliberate forward gate verified at Plan 3, not Plan 2 work) |
| [Plan 3: @quarto/api/jupyter](2026-04-16-quarto-jupyter.md) | 2-3 | Plan 2A | After Plan 2A |
| [Plan 4: Julia Validation](2026-04-16-julia-validation.md) | 1-2 | Plans 1a, 1b, 1c, 2, 3 | After all others |
| [**Plan 4b: Shadow-engine feature validation**](2026-07-01-plan4b-shadow-engine-features.md) | 2-3 | Plan 4 | **after Plan 4**; validates tier model + inert surfaces a single-Primary Julia can't reach; implements the `_quarto.yml engines:` splice (Task 9). Excludes Plan 5/6/7/Phase 12/1.6 work. |
| [**Plan 4c: Marimo engine validation**](2026-07-02-plan4c-marimo-validation.md) | 1-2 | Plans 1a–c, 1b, 2A, 2 (**not** Plan 3); reuses Plan 4's build scaffolding | parallel to 4b; adds `first_class` + shared-language (`{python .marimo}` vs `{python}`) coverage Julia can't reach; canonical non-fully-static engine (Plan 6) |
| [**Plan 5: engine-host pooling (preview re-compute warmth)**](2026-06-26-plan5-engine-host-pooling.md) | research stub | full stack (1a–c, 2, 4) + preview-wiring (plan1c R5) + DQ-7 | **last** (post-4; orthogonal to Plan 3) |
| [**Plan 6: Pass-1 engine resolution (per-doc lift)**](2026-06-29-plan6-pass1-engine-resolution.md) | research stub | plan1a-engine + plan1c (resolution machinery) | **additive**, post-1c; orthogonal to Plans 3/4/5 |
| [**Plan 7: native percent/spin conversion + precise SourceInfo**](2026-06-27-plan7-native-percent-spin-sourceinfo.md) | 2-3 | plan1c (`claims_file`/`markdown_for_file`) + Plan 0 (SourceInfo) + Plan 3 (jupyter percent helpers); **not** Plan 5/6 | post-1c (default after 4); pullable earlier; orthogonal to Plans 5/6 |
| [**Plan 8: HANDLED_LANGUAGES → claiming engines (absorb #241 mermaid + graphviz TS extension)**](2026-07-02-plan8-mermaid-absorption-graphviz-ts-extension.md) | 2-3 | Part A: #241 + plan1a-engine; Part B: full TS-engine stack (1a–c, 1b, 2A) + `build-ts-extension` | Part A independent/now; Part B post-1c; enables Plan 6 Q4 |
| **Total** | **10-16** — **everything through Plan 2 is ✓ complete** (Plan 0, 2A, 1a-protocol/host/engine, RTQ, 1a-host-bugs, 1b, 1b.1, 1c, 2). **Remaining:** Plan 3, 4, 4b, 5, 6, 7, 8, and 1c.2. | | |

> **Course correction — RTQ (`plan1a-return-to-q1`).** A **now-complete** (all 6 code items
> landed + reviewed READY TO MERGE, 2026-06-29), originally plan-only correction layer
> over the whole 1a series, added after the first 1a implementations landed. Its thesis: *the q2 wire
> is the framework between q2 and **all** engine features, not the Julia render subset the original 1a
> scoped to — defer features, never the infrastructure they need.* It carries Q1-divergence
> corrections (Item A: ambient `Init` global vs per-launch gating; ENG-1: discovery-tier statics),
> the framework Surface-coverage audit (provided **and** consumed halves), and the
> `#[serde(default)]` carrier seams (FC-1/FC-2). It **amends** the three 1a sub-plans; an
> implementing agent applies it on topic branches off this integration line. (The q2-introduced host
> bugs HOST-1/2 were carved out to `plan1a-host-bugs.md` — a separate, independent plan with its own
> row above.)

### Dependency graph

There are **two independent roots**: `plan1a-protocol` (its sole dependency, Plan 0, is complete — so it is unblocked)
and **Plan 2A** (`@quarto/api` foundation — gated on nothing in the epic but the
npm workspace). There is **no edge between them**. Plan 1b is the first node that
needs both — it depends on the frozen protocol schema *and* on Plan 2A's
`@quarto/api/config` constants. Plan 2A is a shared foundation that Plan 1b,
Plan 2, and Plan 3 all build on, so **the epic does not linearize cleanly past
the roots**.

```
Plan 0 ✓ complete (Include Expansion & SourceInfo)
  │
  ├─ plan1a-protocol ─┬─ plan1a-host ─ plan1a-engine ─┐  (Rust side, parallel with 1b)
  │                   │                               │
  │                   └─ RTQ (Item A/ENG-1/FC-1/FC-2) ┤  (post-RTQ wire — GATES 1b's
  │                                               │   │   Rust-facing surface + E2E)
  │                                               │   │
Plan 2A ──┬─ §2aa (namespaces) ─┬─ Plan 1b ───────────┴───┴─ Plan 1c ─┐
(@quarto/api                    │                                     │
 foundation)                    ├─ Plan 2 (deferred bodies + types) ──── ┤
                                │                                     ├─→ Plan 4
                                └─ Plan 3 (@quarto/api/jupyter) ───────── ┘
```

- `plan1a-host` and `plan1a-engine` run **in parallel with Plan 1b** (Rust side).
- **RTQ gates Plan 1b.** 1b's pure-TS layer (`framing.ts`, `mapped-source.ts`,
  the dispatch loop + T1/T3/T4/T5/T6/T7) can start against a 1b-authored
  `src/types.ts`, but the Rust-facing surface (ambient-API tests, execute
  step-6 field routing, the deferred-deps wire seam) and any Plan-1c E2E need
  RTQ's protocol edits (`Dependencies` verb, `engineDependencies`,
  `Init`/`EngineProjectContext`, ENG-1 statics, FC-1 carriers, B3 stub relabel)
  landed first. RTQ ENG-2 is *not* a gate (already landed).
- `Plan 1c` depends on `plan1a-host`, `plan1a-engine`, and Plan 1b.
- **Plan 1b.1** (`mapped-string-segments`, ✓ complete) hangs off the
  `Plan 2A §2aa (mappedString) → Plan 1b` edge — a foundation fix to the
  `MappedString` surface, landed **independently of Plan 1c** (not on the path
  to Plan 3/4; it also corrects Plan 1b's `mapped-source.ts` serializer).
- **Plan 1c.2** (`loose ends`, not started) hangs off **Plan 1c** as a post-1c
  continuation collecting 1c's disclosed deferrals; it does **not** gate Plan 4.
- Plan 2A blocks Plan 1b (imports `@quarto/api/config` **and** typechecks
  against the vendored `@quarto/types`, both provided at the foundation; its
  contract tests also need §2aa's namespaces + `platform`), and Plan 3
  (`@quarto/api/jupyter` needs the skeleton). Plan 2 (the deferred
  launch-context bodies + `@quarto/types` refinements) depends on §2aa; it does
  **not** depend on Plan 1b (the bodies plug into §2aa's namespaces, and the
  QuartoAPI assembly/gating lives in Plan 1b, not Plan 2).
- `Plan 4` depends on Plan 1c, Plan 2, and Plan 3.

**Plan 0** was the prerequisite for the TS engine protocol design and is now **complete**. It delivered
pre-engine include shortcode expansion (correctness fix for all engines) and
SourceInfo on the engine interface (parity with Quarto 1's MappedString).
Plans 2 and 3 (TypeScript packages) can proceed in parallel since they don't
touch the Rust engine interface. plan1a-protocol depends on Plan 0 because
the protocol types must account for source mapping decisions made in Plan 0.

**Plan 1a** delivers the Rust-side infrastructure, split into three sub-plans:
- **plan1a-protocol** — JSON message types, the `Init`/`LaunchEngine` payloads
  (`HostGlobalConfig` + `EngineProjectContext`),
  `TsExecuteOptions`, format mapping, source-map flattening, path
  conventions, `ConfigValue → TsMetadataValue` rules. Pure data, no
  behavior. Foundation for everything else.
- **plan1a-host** — `EngineTransport` trait, `StdioTransport`, the
  `TsEngineHost` demux + lifecycle, error categories, per-request timeouts,
  cooperative cancellation (`Cancel`/poison), diagnostic-stream forwarding,
  bundle embedding. The q2-side subprocess plumbing (multiplexed control
  socket — Phase 1.5).
- **plan1a-engine** — `ExecutionEngine` trait extensions (`claims_*`,
  `markdown_for_file`, `NotSupported`), `HtmlDependency` relocation,
  `HANDLED_LANGUAGES`, dedup; `TsEngine` struct, two-step lazy
  lifecycle, hint pre-filter, alias map, race-free init,
  `MockTransport` tests.

**Plan 1b** is the Deno-side harness (`@quarto/engine-host-deno` package):
esbuild bundle, `host.ts` main loop, `deno-host.ts` PlatformHost impl,
`mapped-source.ts` MappedString rehydration, `quarto-api.ts` stub,
`engine-loader.ts`. Gated on plan1a-protocol (the frozen JSON schema)
**and Plan 2A** (it imports the metadata-partition key lists from
`@quarto/api/config` and typechecks against the vendored `@quarto/types`);
otherwise runs in parallel with plan1a-host and plan1a-engine.
**plan1a-host ships a placeholder `dist/engine-host-deno.js` so its
`include_str!` compiles independently of Plan 1b — see plan1a-host's
"Bundle embedding" section.** plan1a-engine's tests use a `MockTransport`
that bypasses the subprocess, not the embedded placeholder. Plan 1b
replaces the placeholder file's contents with the real esbuild output at
the end of its work.

**Plan 1c** wires 1a + 1b into the extension system: `_extension.yml`
engine parsing, `deno bundle` build step, registry migration to
`StageContext`, 4-phase detection rewrite, and the echo engine end-to-end
test (which exercises the full stack).

The plans of the engine-extension stack each have a **standalone core**:
- plan1a-protocol: data types
- plan1a-host: subprocess plumbing
- plan1a-engine: trait extensions + TsEngine
- Plan 1b: Deno harness package
- Plan 2A: TS package foundations — `@quarto/api` skeleton (package.json,
  exports map, tsconfig) + `config/` subpath (metadata-partition key lists),
  plus the vendored `@quarto/types` package
- Plan 2: rest of `@quarto/api` — `text/` and `markdown/` subpaths + types
  (Phases 2B, 2C, 2D)
- Plan 3: `@quarto/api/jupyter` subpath (Phases 3A-3D, 3F)

Plan 2A's **foundation** creates the `@quarto/api` package (package.json,
exports map, tsconfig) plus the `config/` constants, and vendors the
`@quarto/types` package from Q1; Plan 2A **§2aa** then implements the runtime
namespaces (`text/`, `markdownRegex/`, `mappedString/`, `format/`, `path/`,
`system/`, `console/`, `crypto/`) + the `platform/` seam. Plan 3 adds
`jupyter/`. Plan 2 is reduced to the launch-context method bodies §2aa left
stubbed plus the `@quarto/types` refinements (formerly Phase 2E).

**Integration phases** that depend on Plan 1b's engine-host package:
- Plan 1c Phase 3 (echo engine E2E test) needs Plan 1b fully working plus
  minimal types from Plan 2 Phase B
- Plan 1b assembles the QuartoAPI from §2aa's real namespaces and gates the
  context-dependent methods (no "Plan 2 replaces 1b's stubs" — §2aa ships real
  bodies; Plan 2 only fills the deferred launch-context bodies)
- Plan 3E (wire jupyter into engine-host) replaces Plan 1b's jupyter stub

Plan 4 integrates everything and depends on all plans being complete.

**Plans 5, 6, and 7 are post-Plan-4 additive layers** — none is on the critical
path; all are explored after the core stack is validated (5 and 6 are research
stubs; 7 has a finalized design backing):
- **Plan 5** (engine-host pooling) keeps the Deno host **warm across preview
  re-computes** so a TS-engine re-execute doesn't respawn the subprocess +
  re-`import()` the module. Single-project, session-scoped behind
  `EngineTransport`; gated by a **measure-first** check (the kernel already
  survives a respawn via its transport file, so pooling only saves the
  Deno-spawn + import, ~hundreds of ms). Depends on the full stack plus the
  preview↔TS-engine wiring (plan1c R5 in RTQ) and DQ-7.
- **Plan 6** (Pass-1 engine resolution) lifts `resolve_engines` from Pass 2 into
  Pass 1 **per-doc**, for docs whose engines all resolve load-free (static
  `_extension.yml` claims, hint exclusion, or tier dominance), with clean
  fall-through to Pass 2 otherwise — preserving back-compat for legacy Q1
  engines that need a load. Relaxes the design's all-or-nothing "every engine
  static" gate (engine-resolution.md §7/§3.3/§12) to a per-doc partial lift;
  stamps `EngineResolution` on `DocumentProfile` (version bump) for engine-aware
  indexing, kernel pooling, and freeze. Additive on top of plan1a-engine +
  plan1c; orthogonal to Plans 3/4/5.
- **Plan 7** (native percent/spin + precise SourceInfo) implements the one thing
  the epic has only **deferred in pieces**: **percent scripts** (`.py`/`.jl`/`.r`
  `# %%`) and **R spin** (`.R`) converted to qmd **natively in Rust**, wired into
  built-in jupyter/knitr via `claims_file`/`markdown_for_file`, with **column-precise
  `SourceInfo`** for every supported language (per the finalized
  `2025-12-15-source-info-for-structured-formats.md`: per-line `Original` `Concat`
  for plain-text scripts; first-class `NotebookCell` for ipynb; sidecar storage). It
  **supersedes Plan 0's "percent conversion loses provenance" framing**, consolidates
  the scattered deferrals (1c Future Work, the knitr-plan spin deferral, Plan 0), and
  delivers the TS-engine **A′** faithful remap so Julia/marimo percent errors point at
  the original file with columns — adding the `.jl` percent-script case Plan 4
  currently excludes. The validation target (`.qmd`) masks this whole surface, so it
  is the clearest instance of the "carry the whole engine surface, not the validation
  target" mandate (`designs/engine-api-surface.md` § Governing principle). Depends on
  plan1c + Plan 0 + Plan 3; **independent of Plans 5/6** (pullable earlier). The
  *intra-line* column problem (re-serialization through the qmd writer) is the
  complementary, deferred concern of `2026-06-18-qmd-per-line-provenance.md` — Plan 7
  owns the input/converter half, that plan the output/writer half.

### Critical path

With parallel execution: Plan 0 is **complete** (so plan1a-protocol is unblocked) and
Plan 2A (~1 session) is the remaining independent root that can run from the start;
once plan1a-protocol freezes the
schema, plan1a-host, plan1a-engine, Plan 1b, Plan 2, and Plan 3 run in parallel
(Plan 1b also needs Plan 2A, which is finished long before) → Plan 1c (1-2
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

**Note:** quarto-cli is a separate repository, referenced via the `external-sources/quarto-cli` symlink. It is the TypeScript/Deno implementation of Quarto 1. We reference it for API definitions and implementation patterns but do not import from it.

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

1. **Build time:** Engine extension TS source is bundled into a single `.js` file using `deno bundle`. The author's `deno.json` resolves imports from the registry:
   - `@quarto/api` → `jsr:@quarto/api` (real code, inlined into the bundle)
   - `@quarto/types` → `jsr:@quarto/types` (type-only, erased during bundling)
   - `@std/*` (path, fs, encoding, …) → `jsr:@std/*`
   All dependencies are inlined into the bundle.

2. **Runtime:** The Deno subprocess loads the bundled `.js` file via dynamic `import()`. No import map or TS transpilation needed at execution time.

The `@quarto/engine-host-deno` harness is also bundled into a single `.js` file that includes `@quarto/api` (all subpaths) and the harness glue. This bundle is built using **esbuild** (matching the existing `quarto-system-runtime` pattern), checked into git at `ts-packages/quarto-engine-host-deno/dist/engine-host-deno.js`, and embedded in the q2 binary via `include_str!()`. At runtime, the embedded JS is written to a temp file and executed with `deno run --allow-all`.

**Distribution of the engine-author SDK.** `@quarto/api` and `@quarto/types`
are **published to a registry** — jsr.io or npmjs.com, as appropriate per
package. Engine authors depend on them from the registry (their extension's
`deno.json` references e.g. `jsr:@quarto/api` / `jsr:@quarto/types`), and
`q2 build-ts-extension` bundles against the published packages, so building an
extension needs **no q2 source clone and no build assets embedded in the q2
binary**. `@quarto/types` is type-only (erased during bundling); `@quarto/api`
is real code, inlined into the author's `.js` bundle by `deno bundle` (each
bundle freezes the `@quarto/api` version it built against — an API-stability
surface managed by semver on the published package). Within the q2 repo the
workspace package satisfies the same specifier for dev builds. This is a
deliberate departure from Quarto 1 (which ships these inside the quarto-cli
tree), chosen so the author SDK distributes independently of the q2 binary.

q2 provides a `q2 build-ts-extension` command and a template `deno.json` that
references the published `@quarto/api` / `@quarto/types`; it does not embed or
extract the SDK sources.

## Runtime Dependency

**Deno must be on PATH** to use TS engine extensions (same model as pandoc — assumed present, not bundled). q2 should:
- Check for `deno` in PATH when a TS engine is needed
- Provide a clear error message if Deno is not found
- Document the Deno requirement for engine extension users
- The core q2 binary (markdown, knitr, jupyter engines) does NOT require Deno

**Distribution (mechanism now exists).** A signed installer
(`install.sh`/`install.ps1`) + GitHub-releases pipeline landed 2026-06-12, so
"q2 has no installer" is no longer true. The stance for Deno:
- **Assume-on-PATH (v1)** — discover via `find_binary("deno", "QUARTO_DENO")`
  and fail loud if missing/too-old, exactly the pandoc and `q2 mcp`-Node model.
- **Fetch-on-first-use (upgrade path)** — download a pinned Deno into
  `~/.cache/quarto-deno/` on the first TS-engine use, reusing the hub-MCP
  bundle's existing cache/lock/GC machinery. No binary bloat; Deno is pinned in
  code, never embedded.
- **Embedding Deno in the binary is explicitly off the table** — bd-3e3sam51
  deliberately removed `deno_core`/`rusty_v8` (it blocked musl static builds
  and added ~100 MB of v8), and the release archive is binary-only. Do not
  reintroduce it.

Tests that require Deno are skipped if it's absent, following the
pandoc-dependent-test pattern.

## Engine Discovery and Language Claiming

q2 generalizes Quarto 1's single-engine selection into **per-language
ownership** across a possibly-multi-engine sequence (`engine: [a, b]`). The
authoritative model — claim kinds, the resolution tiers, presence-gating,
fallback, ownership enforcement, and the failure model — is the design
contract [`claude-notes/designs/engine-resolution.md`](../designs/engine-resolution.md).
This section records Quarto 1's algorithm for reference and the trait +
registration surface the rest of the epic builds on.

### Quarto 1's algorithm (reference)

In Quarto 1, `fileExecutionEngine()` uses this 4-phase algorithm:
1. **Extension claims (`claimsFile`)**: Each engine's `claimsFile(file, ext)` is checked — `.ipynb` → jupyter, `.rmd` → knitr
2. **YAML declaration**: Check for explicit `engine:` key or engine-name top-level key in frontmatter
3. **Language scanning (`claimsLanguage`)**: Extract languages from code blocks via `languagesWithClasses()`, call each engine's `claimsLanguage(language, firstClass?)`. Returns `false` (no claim), `true` (priority 1), or a number (custom priority). Highest score wins.
4. **Fallback**: If no engine claims any language but there are non-handler languages (not `ojs`, etc.), default to Jupyter. Otherwise, use markdown engine.

`claimsLanguage` receives both the language identifier and an optional `firstClass` extracted from code block attributes (e.g., `{python .marimo}` → language="python", firstClass="marimo"). This allows engines to make class-specific claims.

Engine ordering affects ties: `_quarto.yml` `engines:` list controls priority order. User-specified engines come first; standard engines follow. First engine to achieve the highest score wins.

### q2's algorithm (per-language ownership)

q2 separates **selection** (which engines run) from **division** (which engine
runs which cell) — the two questions Quarto 1 fused because it had one engine.
`claims_language` answers both: a tiered resolver
(`Primary → explicit-Fallback → Interop → implicit-Fallback`) assigns each
computational language to exactly one owner, and the distinct owners, in
registry/`engines:` order, form the execution sequence. See the design doc for
the tiers, presence-gating, and worked cases.

**Jupyter is the universal fallback, expressed as `Fallback(0)`.** It positively
claims nothing, so it never out-ranks a dedicated engine: a Julia extension's
`Primary(1)` for `julia` beats jupyter's `Fallback(0)`, so the extension wins
cleanly. This preserves the original modernization intent — jupyter must not
conflict with the Julia extension — but without the registration-order tie
Quarto 1 had (where both returned priority 1). When no engine claims a
computational language, jupyter is the implicit fallback; a document with no
executable cells falls through to the markdown engine.

**`first_class` drives selection, not per-cell routing.** It sharpens a claim
(a marimo engine returns `Primary` for `{python .marimo}`, `None` for plain
`{python}`), so it influences *which* engine wins a language — but ownership is
per-language, and a language has exactly one owner (enforcement is per-language;
see engine-resolution.md §4.2).

### Implementation on the `ExecutionEngine` trait

Discovery methods are added to the `ExecutionEngine` trait with defaults (Option A from design discussion):

```rust
pub trait ExecutionEngine: Send + Sync {
    // ... existing methods ...

    /// File extensions this engine can handle (e.g., [".ipynb", ".py"]).
    /// Used as a pre-filter before any claiming logic.
    fn valid_extensions(&self) -> Vec<String> { Vec::new() }

    /// This engine's claim on a language: `Primary` (I execute it),
    /// `Interop` (extend my ownership to it iff I'm already present),
    /// `Fallback` (universal kernel), or `None`. `first_class` is the first
    /// CSS class from code-block attributes (e.g. "marimo" from
    /// `{python .marimo}`). See engine-resolution.md §3-4 for how the kinds
    /// and priorities resolve.
    fn claims_language(&self, _language: &str, _first_class: Option<&str>) -> LanguageClaim {
        LanguageClaim::None
    }

    /// Whether this engine claims a file by extension.
    fn claims_file(&self, _file: &str, _ext: &str) -> bool { false }
}
```

Built-in engines implement these directly:
- **Jupyter**: `claims_file(".ipynb") → true`. `claims_language(...) → Fallback(0)` — the universal kernel executor, claiming every computational language it is asked about at the fallback floor. It makes no `Primary`/`Interop` claim, so a dedicated engine (e.g. the Julia extension's `Primary(1)` for `julia`) always wins; jupyter handles a language only when nothing else claims it (engine-resolution.md §4.3). This is the q2 form of "Jupyter no longer claims julia explicitly" — the conflict Quarto 1 had is gone because `Fallback(0) < Primary(1)`.
- **Knitr**: `claims_language("r") → Primary(1)`, `claims_language("python"/"sql"/…) → Interop` (reticulate — taken only when knitr is already running for `r`), `claims_file(".rmd") → true`
- **Markdown**: returns defaults (`None`)
- **TsEngine**: forwards queries to the Deno subprocess, or answers from the static claims declared in `_extension.yml` without loading (engine-resolution.md §3.3)

Language + class information is extracted from the **parsed AST** (not regex), since q2 already has pampa for parsing.

### Engine ordering and registration

Following Quarto 1's `resolveEngineExtensions()` + `resolveEngines()` pipeline:

1. Extension discovery scans `_extensions/` for `contributes: engines:` entries
2. Extension engines are merged into `projectConfig.engines`
3. `_quarto.yml` `engines:` list controls ordering (user-specified engines first)
4. Standard engines (knitr, jupyter, markdown) are appended after user-specified engines
5. When two engines have the same priority score for a language, iteration order wins
