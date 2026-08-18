# Quarto 1 Execution-Engine Lifecycle & Data-Flow Model

**Status:** reference / ground-truth research artifact
**Reconstructed:** 2026-06-25, entirely from the Quarto 1 (TypeScript) source at
`~/src/quarto-cli` (no other source consulted).
**Scope:** the execution-engine protocol — discovery, claiming, launch, execution,
dependency flow, post-processing, and serve — together with the engine-facing
`quarto` API object. Every claim below carries a `file:line` citation against the
Q1 source. Signatures are quoted verbatim from the source rather than paraphrased.

> This document describes Quarto 1 *on its own terms*. It is intended as a fixed
> reference against which other designs can later be diffed; it makes no claim
> about, and contains no comparison to, any reimplementation.

---

## 0. The two engine interfaces

Q1 splits an "execution engine" into two interfaces with **different lifetimes and
different owners**:

- **`ExecutionEngineDiscovery`** — the *static* object. One per engine, registered
  once in a process-global `Map`. Owns discovery/claiming and the `launch` factory.
  `src/execute/types.ts:35-87`.
- **`ExecutionEngineInstance`** — the *dynamic* object, produced by
  `discovery.launch(context)`. Owns per-file execution work (`target`, `execute`,
  `dependencies`, `postprocess`, `run`, `postRender`).
  `src/execute/types.ts:93-136`.

The exact discovery interface (verbatim, abridged to the protocol-relevant members):

```ts
export interface ExecutionEngineDiscovery {
  init?: (quarto: QuartoAPI) => void;                       // types.ts:41
  name: string;                                             // types.ts:43
  defaultExt: string;                                       // types.ts:44
  validExtensions: () => string[];                          // types.ts:47
  claimsFile: (file: string, ext: string) => boolean;      // types.ts:48
  claimsLanguage: (language: string, firstClass?: string)  // types.ts:56
    => boolean | number;
  canFreeze: boolean;                                       // types.ts:57
  generatesFigures: boolean;                                // types.ts:58
  quartoRequired?: string;                                  // types.ts:65
  launch: (context: EngineProjectContext)                  // types.ts:86
    => ExecutionEngineInstance;
}
```

The exact instance interface (verbatim, `src/execute/types.ts:93-136`):

```ts
export interface ExecutionEngineInstance {
  name: string;
  canFreeze: boolean;
  markdownForFile(file: string): Promise<MappedString>;
  target: (file, quiet?, markdown?) => Promise<ExecutionTarget | undefined>;
  partitionedMarkdown: (file, format?) => Promise<PartitionedMarkdown>;
  filterFormat?: (source, options, format) => Format;
  execute: (options: ExecuteOptions) => Promise<ExecuteResult>;
  executeTargetSkipped?: (target, format) => void;
  dependencies: (options: DependenciesOptions) => Promise<DependenciesResult>;
  postprocess: (options: PostProcessOptions) => Promise<void>;
  canKeepSource?: (target) => boolean;
  intermediateFiles?: (input) => string[] | undefined;
  run?: (options: RunOptions) => Promise<void>;
  postRender?: (file: RenderResultFile) => Promise<void>;
}
```

Note that **every method that produces engine output is `async` and returns a typed
value** (`Promise<ExecuteResult>`, `Promise<DependenciesResult>`). The only `void`
returns are `executeTargetSkipped` (a cleanup hook) and `postprocess`/`run`/
`postRender` (side-effecting hooks that operate on already-written output files,
not on dependency state).

---

## 1. The full ordered lifecycle

The pipeline runs **per file**, serially (see §5). For one file:

| # | Step | Owner | Sync/async | When | Returns |
|---|------|-------|-----------|------|---------|
| 1 | Register standard engines | module `engine.ts` | sync, once | first `resolveEngines` | — |
| 2 | `resolveEngines(project)` | `engine.ts` | async | per render | `Map<name, Discovery>` (reordered) |
| 3 | extension claim: `claimsFile(file, ext)` | **Discovery** | sync | `fileExecutionEngine` | `boolean` |
| 4 | markdown claim: `claimsLanguage(lang, firstClass?)` | **Discovery** | sync | `markdownExecutionEngine` | `boolean \| number` (score) |
| 5 | `launch(context)` | **Discovery** | sync | once a claim is made | `ExecutionEngineInstance` |
| 6 | `markdownForFile(file)` | **Instance** | async | `resolveFullMarkdownForFile` | `Promise<MappedString>` |
| 7 | `target(file, quiet?, markdown?)` | **Instance** | async | `fileExecutionEngineAndTarget` | `Promise<ExecutionTarget>` |
| 8 | `execute(options)` | **Instance** | async | `renderFileInternal` | `Promise<ExecuteResult>` |
| 9 | `dependencies(options)` | **Instance** | async | `render.ts` (only if `engineDependencies` present) | `Promise<DependenciesResult>` |
| 10 | `postprocess(options)` | **Instance** | async | after pandoc writes output | `Promise<void>` |
| 11 | `postRender(file)` | **Instance** | async (optional) | after render result produced | `Promise<void>` |
| 12 | `run(options)` | **Instance** | async (optional) | `quarto serve` / preview | `Promise<void>` |

### 1.1 Discovery & registration

`kStandardEngines` is the fixed registration list: knitr, jupyter, markdown
(`src/execute/engine.ts:49-53`). Engines live in a process-global
`const kEngines: Map<string, ExecutionEngineDiscovery> = new Map();`
(`engine.ts:46`). Registration is once-guarded by `enginesRegistered`
(`engine.ts:55`, `213-220`).

`registerExecutionEngine` rejects duplicates, checks the engine's `quartoRequired`
semver, inserts into `kEngines`, and — critically — calls `engine.init` with the
**global** API object:

```ts
export function registerExecutionEngine(engine: ExecutionEngineDiscovery) {
  if (kEngines.has(engine.name)) { throw ... }      // engine.ts:92-94
  checkEngineVersionRequirement(engine);            // engine.ts:97
  kEngines.set(engine.name, engine);                // engine.ts:99
  if (engine.init) { engine.init(getQuartoAPI()); } // engine.ts:100-102
}
```

Extension engines (from `project.config.engines` entries that are objects with a
`path`) are dynamically `import()`ed, validated for `name`/`launch`/`claimsLanguage`,
version-checked, inserted into the same `kEngines` map, and `init`ed with the same
`getQuartoAPI()` (`engine.ts:227-273`). They are placed *first* in the resolution
order via `userSpecifiedOrder` (`engine.ts:285-297`).

### 1.2 Claiming — `claimsFile` then `claimsLanguage` (with scoring)

`fileExecutionEngine` resolves in two tiers:

1. **Extension claim.** For each engine in resolution order, the first whose
   `claimsFile(file, ext)` returns `true` wins, and is launched immediately
   (`engine.ts:320-325`):
   ```ts
   for (const [_, engine] of engines) {
     if (engine.claimsFile(file, ext)) {
       return engine.launch(engineProjectContext(project));
     }
   }
   ```
   `claimsFile` is a pure predicate. Examples: knitr claims `.rmd`/`.rmarkdown` or a
   spin-script (`rmd.ts:72-75`); jupyter claims notebook extensions or percent
   scripts (`jupyter.ts:109-112`); julia (extension engine) claims `.jl` percent
   scripts (`julia-engine.ts:94-96`).

2. **Markdown claim** (for `.md`/`.qmd`). `markdownExecutionEngine` (`engine.ts:146-211`):
   - **YAML engine declaration first.** If the front matter names an engine
     (`yaml[engine.name]`) or sets `execute.engine`, that engine launches
     (`engine.ts:161-169`).
   - **Language scoring otherwise.** For each language found in the markdown, every
     engine's `claimsLanguage(language, firstClass)` is evaluated and the
     **highest score wins** (`engine.ts:177-198`):
     ```ts
     const claim = engine.claimsLanguage(language, firstClass);
     if (claim === false) { continue; }          // engine.ts:184-186
     const score = claim === true ? 1 : claim;   // engine.ts:188
     if (score > bestScore) { bestScore = score; bestEngine = engine; }
     ```
     **The scoring rule:** `false` → don't claim; `true` → score `1`; a number → that
     number is the score (higher wins). This is the mechanism by which julia
     (`claimsLanguage` returns `true` for `"julia"`, `julia-engine.ts:98-100`) and
     jupyter (also returns `true` for `"julia"`, `jupyter.ts:113-117`, to preserve
     default precedence) can both claim a language and ordering breaks the tie via
     `>` (strictly-greater), favoring the engine encountered first.
   - **Fallbacks:** a non-OJS, non-handler language forces jupyter
     (`engine.ts:200-206`); otherwise the markdown engine
     (`engine.ts:210`).

### 1.3 Launch → instance

`launch(context)` is **synchronous** and is the *only* place an
`ExecutionEngineInstance` is created. In every engine it is a closure that captures
the `EngineProjectContext` and the module-level `quarto` reference, then returns a
plain object literal of the instance methods (knitr `rmd.ts:225-404`; jupyter
`jupyter.ts:249`+; julia `julia-engine.ts:132-315`; markdown `markdown.ts:52-108`).
`launch` is called fresh at every claim site and again inside `target`
(`rmd.ts:245-249`) and inside the dependency round-trip (`render.ts:94`) — i.e. **a
new instance is cheap and is created multiple times for the same file**; the engine
does not assume a singleton instance.

### 1.4 target → execute

`fileExecutionEngineAndTarget` launches the engine, resolves full markdown, and
calls `engine.target(...)`, caching both on the project's file-information cache
(`engine.ts:353-380`). `target` produces an `ExecutionTarget`
(`{source, input, markdown, metadata, data?}`, `types.ts:139-146`) — a read-only
"cookie" handed to subsequent calls. The engine-specific kernel choice rides in
`data` (jupyter stores `{transient, kernelspec}` there, `jupyter.ts:64-67`,
`jupyter.ts:450`).

`execute(options)` is called once per file in `renderFileInternal`:
```ts
const executeResult = await context.engine.execute(executeOptions); // render-files.ts:237
```
and the result is returned up the stack (`render-files.ts:285`) — a **return
value**, captured into a local.

---

## 2. Ownership & lifetime of the `quarto` API object

**There is exactly one `quarto` API object per process, and it is immutable after
construction.**

- **Construction.** `getQuartoAPI()` lazily builds and caches a single instance:
  ```ts
  let _quartoAPI: QuartoAPI | null = null;          // src/core/api/index.ts:34
  export function getQuartoAPI(): QuartoAPI {
    if (_quartoAPI === null) {
      _quartoAPI = globalRegistry.createAPI();       // index.ts:46-48
    }
    return _quartoAPI;
  }
  ```
- **The registry builds it once and freezes registration.** `createAPI()` validates
  that all nine namespaces are registered, eagerly calls each provider exactly once,
  sets `this.finalized = true`, caches `this.apiInstance`, and returns it
  (`src/core/api/registry.ts:72-118`). After finalize, any further `register` throws
  `RegistryFinalizedError` (`registry.ts:53-55`).
- **Namespaces are registered by side-effect import**, before any engine runs:
  `src/quarto.ts:56` imports `./core/api/register.ts`, which imports the nine
  namespace modules (`src/core/api/register.ts:7-15`), each of which calls
  `globalRegistry.register(<ns>, provider)` at module-evaluation time (e.g.
  `format.ts:18`).
- **The API surface is nine namespaces** — `markdownRegex, mappedString, jupyter,
  format, path, system, text, console, crypto` (`src/core/api/types.ts:236-246`,
  enforced by `requiredNamespaces` in `registry.ts:79-89`).
- **One shared instance across all engines.** `registerExecutionEngine` and the
  extension-engine path both pass `getQuartoAPI()` — the same cached object — into
  every engine's `init` (`engine.ts:101`, `engine.ts:260`). The discovery interface
  documents this explicitly: *"May be called multiple times but always with the same
  QuartoAPI object."* (`types.ts:38-40`). Each engine stashes it in a module-level
  `let quarto: QuartoAPI` (knitr `rmd.ts:16,53-55`; jupyter `jupyter.ts:72,78-80`;
  julia `julia-engine.ts:49,76-78`).
- **Mutability of its methods.** The namespace methods are stateless functions
  (format predicates, path helpers, md5, mapped-string operations, process spawns).
  They do not write back into the API object or into any engine-visible accumulator;
  e.g. `quarto.jupyter.resultIncludes` / `resultEngineDependencies`
  (`types.ts:102-108`) *return* values that the engine then puts in its own
  `ExecuteResult` (jupyter `jupyter.ts:557-569`; julia `julia-engine.ts:255-269`).
  `quarto.system.onCleanup` (`types.ts:179`) registers a process-cleanup handler —
  that is process-lifecycle state, not engine-result state.

**Summary:** the `quarto` object is process-global, built once, validated, finalized,
shared by reference to every engine, and never mutated during a render in a way that
carries engine output.

---

## 3. The data-flow model (the load-bearing section)

**Every kind of engine output travels back to the host as a RETURN VALUE. There is
no mutable host-side accumulator anywhere in Q1's engine→host dependency/result
flow.**

### 3.1 The output shape (`ExecuteResult`, `types.ts:166-178`)

```ts
export interface ExecuteResult {
  markdown: string;
  supporting: string[];
  filters: string[];
  metadata?: Metadata;
  pandoc?: FormatPandoc;
  includes?: PandocIncludes;
  engine?: string;
  engineDependencies?: Record<string, Array<unknown>>;
  preserve?: Record<string, string>;
  postProcess?: boolean;
  resourceFiles?: string[];
}
```

`DependenciesResult` is just `{ includes: PandocIncludes }` (`types.ts:214-216`).

Per output kind:

| Output | Carrier | Mechanism | Proof |
|--------|---------|-----------|-------|
| Execution result (markdown) | `ExecuteResult.markdown` | return value | jupyter `jupyter.ts:589-591`; julia `julia-engine.ts:284-286`; markdown `markdown.ts:92-97` |
| Engine dependencies | `ExecuteResult.engineDependencies` | return value (a `Record<engine, Array>`) | jupyter `jupyter.ts:555-569,596`; julia `julia-engine.ts:254-269,291` |
| Includes (header/before/after body) | `ExecuteResult.includes` *or* `DependenciesResult.includes` | return value (`PandocIncludes`) | jupyter `jupyter.ts:557-560,595` and `dependencies` `jupyter.ts:607-624`; julia `julia-engine.ts:256-259` |
| Supporting files | `ExecuteResult.supporting` | return value (`string[]`) | jupyter `jupyter.ts:592`; julia `julia-engine.ts:287` |
| Preserved regions | `ExecuteResult.preserve` | return value (`Record<string,string>`) | jupyter `jupyter.ts:597`; julia `julia-engine.ts:292` |
| Post-process flag | `ExecuteResult.postProcess` | return value (`boolean`) | jupyter `jupyter.ts:598-599`; julia `julia-engine.ts:293-294` |

Note the engine *constructs and returns* an object literal — e.g. jupyter's
`return { engine, markdown, supporting, filters, pandoc, includes,
engineDependencies, preserve, postProcess };` (`jupyter.ts:589-600`). Nothing is
pushed into a caller-provided sink.

### 3.2 How the host consumes each return value

All consumption is by reading fields off the returned `executeResult` local in
`src/command/render/render.ts`:

- **`includes`** are merged into `format.pandoc` (`render.ts:80-81` via
  `mergePandocIncludes`), and later into pandoc defaults — they have already been
  folded into `format.pandoc[include-*]` before `runPandoc` (`render.ts:204`). The
  `render-files.ts` postprocess path likewise *merges* handler results into
  `executeResult.includes` by reassignment, not by a shared accumulator
  (`render-files.ts:492-516`).
- **`pandoc`** is merged into `format.pandoc` (`render.ts:83-88`).
- **`engineDependencies` round-trip — the key proof.** Only if the engine deferred
  dependency resolution (returned `engineDependencies` instead of `includes`) does
  the host invoke `dependencies(...)`. It iterates the returned record, **launches a
  fresh instance**, calls `dependencies` passing the returned array as
  `DependenciesOptions.dependencies`, and merges the returned `DependenciesResult.includes`:
  ```ts
  if (executeResult.engineDependencies) {                          // render.ts:91
    for (const engineName of Object.keys(executeResult.engineDependencies)) {
      const engine = executionEngine(engineName)!;                 // render.ts:93
      const engineInstance = engine.launch(                        // render.ts:94
        engineProjectContext(context.project));
      const dependenciesResult = await engineInstance.dependencies({// render.ts:95
        target: context.target, format, output: recipe.output, ...,
        dependencies: executeResult.engineDependencies[engineName],// render.ts:103
        quiet: ...,
      });
      format.pandoc = mergePandocIncludes(                         // render.ts:106-109
        format.pandoc, dependenciesResult.includes);
    }
  }
  ```
  This is a closed return→arg→return loop: `ExecuteResult.engineDependencies` (return)
  → `DependenciesOptions.dependencies` (arg) → `DependenciesResult.includes` (return)
  → merged into the local `format.pandoc`. **No shared accumulator is touched at any
  hop.** The `DependenciesOptions.dependencies?: Array<unknown>` field
  (`types.ts:209`) is precisely the inbound conduit for the data the engine returned.
- **`postProcess`/`preserve`** drive whether `postprocess(...)` runs later. The host
  calls `engine.postprocess(...)` at `render.ts:215`, gated on
  `executeResult.postProcess` (`render.ts:213`) and passing
  `preserve: executeResult.preserve` (`render.ts:222`). The hook returns
  `Promise<void>` (`types.ts:125`) and operates on the already-written output file
  (e.g. jupyter restores preserved HTML via
  `quarto.text.postProcessRestorePreservedHtml`, `jupyter.ts:626-629`) — it does not
  register dependencies.

After all merges, `format.pandoc` is fed through `generateDefaults` into `allDefaults`
in `pandoc.ts:471`, the include path-lists are written to a defaults YAML file, and
pandoc is invoked with `--defaults <file>` (`pandoc.ts:1140` →
`cmd.push("--defaults", defaultsFile)`). Thus the engine's returned include *file
paths* reach pandoc as defaults entries — still pure data, never a callback.
`pandoc-dependencies-html.ts:142-193` (`resolveDependencies`) deep-clones its input
(`safeCloneDeep`, line 150), renders `extras.html[kDependencies]` to temp HTML, and
*prepends* them onto the include lists (`pandoc-dependencies-html.ts:175-188`),
**returning a new `extras`** — no shared accumulator.

### 3.3 The explicit answer to "is there any accumulator?"

**No.** Searching the engine→host path: `execute` returns `ExecuteResult` (a fresh
literal); the host captures it into a local (`render-files.ts:237`); fields are read
and *merged into the per-render `format.pandoc` local* (`render.ts:80-110`);
`dependencies` is a pure return-in/return-out call. The engine is never handed a
mutable sink, callback registrar, or shared array to push into. The data model is
**entirely return-based**.

(The one place the host *re-shapes* a return value in place is `render-files.ts:492-516`,
where it reassigns `executeResult.includes = mergeConfigs(...)` to fold in cell-handler
results — but that mutates the *returned result object the host already owns*, not a
cross-engine accumulator, and it happens host-side after the engine returned.)

---

## 4. Statefulness

### 4.1 Per-instance state — minimal

An `ExecutionEngineInstance` is a closure literal. The only state it captures is:
- the `EngineProjectContext` (`launch`'s parameter) — read-mostly project info:
  `dir`, `isSingleFile`, `config`, `getOutputDirectory`, `resolveFullMarkdownForFile`,
  and a `fileInformationCache` (`src/project/types.ts:164-199`). The cache is the only
  mutable member, used by jupyter for transient-notebook tracking
  (`project/types.ts:188-192`).
- the module-level `quarto` reference (shared, immutable; §2).

No engine instance stores a kernel handle, socket, or accumulated dependency list on
itself. Instances are created freely and repeatedly (§1.3).

### 4.2 Process/daemon state — out-of-process, file-keyed

Long-lived execution state lives in **external daemon processes addressed by transport
files on disk**, never on the instance object:

- **Jupyter keepalive kernel.** `executeKernelKeepalive` connects to a kernel via a
  transport file whose path is a hash of the *input file*
  (`jupyter-kernel.ts:78,139,305-324`), so keepalive kernels are never reused across
  inputs sharing a hash (`jupyter.ts:462-464`). Oneshot mode aborts any existing
  keepalive first (`jupyter-kernel.ts:34-57`). The kernel process outlives a single
  `execute` call but is reached purely through the on-disk transport file, not a
  field on the instance.
- **Julia control server.** A single detached control-server process is started or
  reused based on `juliaTransportFile()` =
  `<runtime>/julia_transport.txt` (`julia-engine.ts:330-448,962-964`). Each `execute`
  opens a fresh TCP connection (`getJuliaServerConnection`, `julia-engine.ts:550-602`),
  optionally closing the per-notebook worker before/after in `oneShot` mode
  (`julia-engine.ts:703-749`). Again: the durable state is the OS process + transport
  file; the instance holds nothing.
- **knitr.** Stateless per call — each `execute`/`dependencies`/`run` spawns `Rscript`
  with a temp JSON results file and reads the JSON back (`rmd.ts:407-489`). No daemon.
- **markdown.** Fully pure — `execute` just returns the markdown (`markdown.ts:78-98`).

So: the **engine instance is effectively stateless** with respect to results; any
persistence is *process/daemon state external to the instance, keyed by file path on
disk*.

---

## 5. Concurrency

**Within one render invocation, files are executed strictly serially.** The driving
loop in `renderFiles` is a plain indexed `for` with an `await` per file:

```ts
for (let i = 0; i < files.length; i++) {           // render-files.ts:316
  const file = files[i];
  ...
  await renderFileInternal(fileLifetime, file, ...);// render-files.ts:330
  ...
}
```
(`src/command/render/render-files.ts:316-345`). There is no `Promise.all`, no
worker pool, no interleaving — each file's `execute` (and its dependency round-trip)
fully completes before the next file begins. `renderFile` (single-file path) likewise
awaits one `renderFileInternal` (`render-files.ts:373-399`).

Consequences:
- Different files' / different engines' `execute()` calls **do not overlap**.
- The `quarto` API object's state is never *written* (it is immutable; §2), so the
  question of concurrent writers is moot — but even the serial loop never mutates it.
- Daemon concurrency (a keepalive kernel staying warm across files) is an
  out-of-process optimization reached through transport files; it does not make the
  host-side `execute` calls concurrent.

---

## 6. Imperative-registration surfaces that DO exist in Q1 — and why they are not the engine protocol

Q1 *does* have imperative "register a dependency" APIs, but they live in a
**single-document, single-threaded filtering/handler context**, categorically
different from the engine→host result flow:

1. **Lua filter API `quarto.doc.addHtmlDependency`.** Defined in the Pandoc datadir
   init script as an alias of `quarto.doc.add_html_dependency`
   (`src/resources/pandoc/datadir/init.lua:1050`). This runs *inside Pandoc's Lua
   filter pass*, mutating the in-flight document's dependency set. Pandoc processes
   one document at a time, single-threaded, so an imperative "add this dependency now"
   call has a well-defined single target.

2. **Cell-handler `addHtmlDependency` (and siblings).** The `HandlerContext`
   interface exposes `addHtmlDependency`, `addSupporting`, `addResource`, `addInclude`
   (`src/core/handlers/types.ts:124`+), implemented in `src/core/handlers/base.ts:270-294`
   as pushes into a per-invocation `results` accumulator:
   ```ts
   addHtmlDependency(dep) {
     if (results.extras.html === undefined) {
       results.extras.html = { [kDependencies]: [dep] };
     } else {
       results.extras.html[kDependencies]!.push(dep);     // base.ts:276
     }
   },
   ```
   Consumers: e.g. `src/core/handlers/mermaid.ts:143,160,189`. This *is* a mutable
   accumulator — but its scope is **one cell-language handler pass over one document's
   markdown**, owned by a single `HandlerContextResults` object created for that one
   document. The accumulated `results` then flow back **as a return value**
   (`ExecuteResult.includes`/`supporting` are reconciled host-side at
   `render-files.ts:492-516`).

**Why this does not generalize to the engine protocol.** Both surfaces above operate
on a *single document being filtered/handled in a single synchronous pass*, where
"the current document" is unambiguous. The engine protocol, by contrast, spans
multiple files and (potentially) multiple engines across a render; Q1 keeps that flow
**return-based** precisely so there is no shared, ambiguously-owned sink. A reader
should not mistake the Lua/handler imperative dependency APIs for the way engines
report dependencies — engines report by returning `ExecuteResult.engineDependencies`
and `DependenciesResult.includes` (§3), never by calling an "add dependency" method
on the host.

---

## 7. Invariants for anyone porting this protocol

Stated as load-bearing properties of Quarto 1:

1. **Engine output is return data, not imperatively registered.** Every result —
   markdown, includes, engine dependencies, supporting files, preserved regions,
   post-process flag — leaves the engine as a field of a freshly-constructed
   `ExecuteResult` / `DependenciesResult` (`types.ts:166-216`; engine returns at
   `jupyter.ts:589-600`, `julia-engine.ts:284-295`, `markdown.ts:92-97`).

2. **There is no host-side accumulator in the engine path.** The host reads returned
   fields into per-render locals and merges them into the local `format.pandoc`
   (`render.ts:80-110`). The dependency phase is a closed return→arg→return loop
   (`render.ts:91-110`), never a push into a shared sink.

3. **The engine instance is the unit of *interface* state, but holds almost none.**
   Instances are cheap closures over a read-mostly `EngineProjectContext` and a shared
   `quarto` reference; they are created repeatedly even for the same file
   (`engine.ts:323`, `rmd.ts:247`, `render.ts:94`). Durable execution state (kernels,
   servers) lives in **external daemon processes keyed by on-disk transport files**
   (`jupyter-kernel.ts:305-324`; `julia-engine.ts:962-964`), not on the instance.

4. **The `quarto` API object is one shared, immutable, process-global instance.**
   Built once and finalized (`registry.ts:72-118`), validated for all nine namespaces,
   handed by reference to every engine's `init` (`engine.ts:101,260`;
   `types.ts:38-40`), and never mutated to carry engine results.

5. **Execution is strictly serial per render.** A single indexed `for … await` loop
   drives files one at a time (`render-files.ts:316-345`); no two `execute` calls
   overlap, so no shared mutable state is exposed to concurrent writers.

6. **Discovery and execution are split objects with split lifetimes.** `claimsFile`
   /`claimsLanguage` (sync predicates with the higher-score-wins rule,
   `engine.ts:177-198`) belong to the per-engine, process-global
   `ExecutionEngineDiscovery`; `execute`/`dependencies`/`postprocess` belong to the
   per-launch `ExecutionEngineInstance` (`types.ts:35-136`).

7. **The pre-existing imperative dependency APIs are document-filtering surfaces, not
   the engine protocol.** `quarto.doc.addHtmlDependency` (Lua,
   `init.lua:1050`) and the cell-handler `addHtmlDependency`
   (`handlers/base.ts:270`) accumulate within a single synchronous single-document
   pass and then surface their results *as return values*; they are not how engines
   communicate dependencies to the host.
