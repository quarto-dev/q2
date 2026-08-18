# Plan 2: @quarto/api deferred launch-context bodies + @quarto/types refinements

**Grand plan:** [2026-04-16-ts-engine-extensions-subprocess.md](2026-04-16-ts-engine-extensions-subprocess.md)
**Depends on:** Plan 2A — both the **foundation** (`@quarto/api` shell + `./config` + vendored `@quarto/types`) and **§2aa** (the runtime surface: the `text`/`markdownRegex`/`mappedString`/`format`/`path`/`system`/`console`/`crypto` namespaces + `@quarto/api/platform`), both implemented. **Phase A also depends on Plan 1b** (#8) — its `buildQuartoAPI(global, host)` assembly (`@quarto/engine-host-deno/src/quarto-api.ts`, landed) is the integration point that threads `Init { global }` into the factories Phase A gives bodies; Phase A lands the `global`-param seam Plan 1b already stubbed (the `_global` it accepts but ignores). Phase B (types) is otherwise independent. Phase A fills stubs that §2aa shipped, so it follows §2aa.
**Blocks:** Plan 4 (Julia Validation) needs all of Plan 2.
**Estimated sessions:** 2-3 (revised up from ~1 — Phase A's `global`+`fs` seam + ensureDir port + Model-1 comment cleanup, B1's full `core/process.ts` port + two-tier tests, B2's derive-all + onCleanup reconciliation + negative type test, and Phase B as the coherence gate are materially more than the original "fill the stubs" estimate).

## Scope (reconciled after §2aa landed)

This plan originally owned "create the `@quarto/api` namespaces + assemble the
QuartoAPI." Since then, **Plan 2A §2aa** (the runtime-surface section of Plan 2A)
implemented the namespaces and the `platform` seam, and **Plan 1b** owns the
QuartoAPI assembly. So most of the original Plan 2 is **already delivered
elsewhere** and is no longer in scope here:

- The eight pure/host-only namespaces — `text`, `markdownRegex`, `mappedString`,
  `format`, `path`, `system`, `console`, `crypto` — live under
  `ts-packages/quarto-api/src/` (§2aa). (Q1's names: it is `markdownRegex`, not
  `markdown`, and `mappedString` is its own top-level namespace, not part of
  `text`.) The former **Phase 2B** (`markdown/`) and **Phase 2C** (`text/` +
  MappedString) are done here.
- `@quarto/api/platform` defines the q2-original `PlatformHost`. The
  **authoritative** interface is the landed
  `ts-packages/quarto-api/src/platform/index.ts` (`fs` with
  `readTextFileSync`/`writeFileSync`/`exists`/`makeTempFile`/`makeTempDir`,
  `process.exec` via `ExecOptions`/`ExecResult`, `realPath`, `env`,
  `isInteractive`, `isCI`). Do **not** re-spec it; read the source.
- `denoHost: PlatformHost` and the `buildQuartoAPI(global, host)` assembly live
  in **`@quarto/engine-host-deno`** (Plan 1b: `deno-host.ts`, `quarto-api.ts`).
  The old "Plan 2 wires the namespaces / replaces Plan 1b's stubs" model is gone:
  §2aa ships real namespace bodies and Plan 1b assembles them over the
  `Init { global }` config (ambient — **no launch-gating**, RTQ Item A). Plan 2
  does **not** own the assembly.

What **remains** in Plan 2 is two things, below.

> The detailed method-by-method specs that used to live here (extract-yaml,
> partition, breakQuartoMd, MappedString `.map()`, etc.) are realized in §2aa;
> they're recoverable from git history at the §2aa implementation commits if a
> requirement needs re-checking. The reusable rationale survives under "Design
> Notes" below.

## Execution order (one sequence, four workstreams)

The plan has four workstreams — **Phase A** (deferred bodies), **B1** (`execProcess`
return-to-Q1), **B2** (QuartoAPI conformance / cast removal), **Phase B** (the
`@quarto/types` coherence gate). They are **not** independent; the effective order is:

1. **Phase A + B1 first.** Both edit `@quarto/api` (`system/index.ts`,
   `platform/index.ts`, `path/index.ts`) with real bodies/signatures and touch adjacent
   code (`makeSystem`, `ExecOptions`), so do them back-to-back rather than on parallel
   branches. B1 settles the six-positional `execProcess` **signature** that B2 derives from.
2. **B2 next.** It derives `SystemNamespace = QuartoAPI["system"]` (and the `Pick` subsets),
   which **requires B1's signature already landed** (the derived type carries all six
   params) and Phase A's method bodies typed. B2 also retypes the staying stubs and deletes
   the harness `as unknown as QuartoAPI` cast.
3. **Phase B last.** It is the **`tsc` green gate** (per-package `tsc --noEmit`, not a `tsc -b`
   project-references graph — none exists over ts-packages) and **subsumes B2's conformance
   check** (one `tsc`, not two). It cannot go green until Phase A + B1 + B2 have landed,
   because the gate compiles the derived namespaces (B2) carrying the Q1-shaped
   `execProcess` (B1) against the refined `@quarto/types`.

So: **A + B1 → B2 → Phase B gate**, under the epic-level **1b → 2 → 3** (1b's
`HtmlDependency` before Phase B; Phase B's frozen contract before Plan 3's runtime). The
four "Success Criteria" bullets are deliverables, not a running order.

## Phase A — deferred launch-context bodies (fill §2aa's stubs)

§2aa shipped these methods as "not yet implemented" stubs because their *bodies*
need the process-stable config. Give them real bodies. **Two stub shapes — do not
conflate them:**

- `path.runtime`/`resource`/`dataDir` are **synchronous** `: string` stubs that
  throw synchronously (`path/index.ts:107/113/119` decls, impls `:167-177`). They
  must **stay synchronous** — Q1's `quartoDir` uses `ensureDirSync`, and Julia calls
  `quarto.path.runtime("julia")` synchronously and catches a sync throw
  (`julia-engine.ts:946`). Do **not** async-ify them.
- `system.pandoc`/`checkRender`/`runExternalPreviewServer` are **async** stubs that
  reject rather than throw synchronously (`system/index.ts:296-313` — the `async`
  wrapper returns a rejected promise so `.catch()`-style callers work; see the §2aa
  final-review fixes). **Caveat — `runExternalPreviewServer` is only *transiently* async:**
  Q1 returns `PreviewServer` **synchronously**, so **B2 re-types it as a synchronous
  throwing stub** (see B2). Phase A leaves it in its current async §2aa shape; do not treat
  that shape as final for this one method.

**These are not gated** (RTQ Item A removed the launch-gating): `path.runtime`/
`resource`/`dataDir` and `system.pandoc` resolve from the `Init { global }` config,
so they are available **pre-launch**. The config is `HostGlobalConfig`
(`ts_protocol.rs:229`; the `Init` variant is at `:38`), whose **7** camelCase fields
are `resourceDir`, `runtimeDir`, `dataDir`, `pandocPath` (`Option<String>`),
`isInteractiveSession`, `runningInCi`, `quartoVersion` — Phase A reads the first four.

**The seam (#3 — was underspecified).** The config is **not** IO on `PlatformHost`;
it reaches the bodies as a **`global` parameter on the factories.** §2aa's factories
take only host subsets — `makePathHost(host: Pick<…,"cwd">)`, `makeSystem(host: Pick<…>)`
— so Phase A widens exactly those two: `makePathHost(host: Pick<…,"cwd"|"fs">, global)`
and `makeSystem(host, global)` (pure factories unchanged). `buildQuartoAPI` (Plan 1b)
threads the `_global` it already accepts but ignores (`quarto-api.ts:116-119`, with the
marked injection sites at `:175-176,189-190`).

**Delete the stale Model-1 comments (#1 — the source currently contradicts this plan).**
The landed §2aa source still describes the *old* env-based, launch-gated model:
`path/index.ts:107/113/119` say the stubs "throw until the engine host is initialized at
launchEngine," and `platform/index.ts:93-102,116-125` reserve `env.get`/`realPath` as
"the interface point for the real bodies of `path.runtime`/`dataDir`." Under the
config-injected model those reservations are **vestigial** — rewrite those comments to
the config model; do **not** implement env-based resolution. (q2's Rust side already does
the XDG/Library/AppData resolution and ships the resolved dirs on `Init`; re-deriving in
the spawned Deno subprocess can't reproduce `resource`/`pandoc` anyway — it isn't the q2
binary — so config injection is the faithful port, not a shortcut.)

- [x] `system.pandoc` — invoke the pandoc binary at `global.pandocPath`, routed
  through `PlatformHost.process.exec`. **`pandocPath` is `Option<String>`
  (`ts_protocol.rs:234`) — when absent, throw a distinct "pandoc unavailable" error
  (NOT `notYetImplementedError`)** (#5): q2 requires ambient pandoc, resolved in Rust;
  if Rust couldn't find it, TS won't do better (decision 2026-06-30). (Q1's
  `pandocBinaryPath` always resolves a bundled fallback; under the config model that
  resolution is Rust-side, so a `None` is a genuine environment failure, not an
  unfinished feature.) **Note — no in-scope caller yet:** knitr/jupyter are native
  Rust and julia/marimo bypass the seam, so nothing invokes `system.pandoc` today.
  Only the errors-on-`None` path is exercised by a test; the happy path (pandoc
  actually invoked) ships real but unexercised. It still gets a real body — unlike
  `checkRender`/`runExternalPreviewServer`, which stay stubs — because it is cheaply
  config-derivable from `global.pandocPath` and a future TS engine will need it.
- [x] `system.checkRender` — the `quarto check` render probe; no q2 caller yet,
  so it stays a `notYetImplementedError` stub.
- [x] `system.runExternalPreviewServer` — spawn the external preview server via
  the host's `process.exec`. No q2 caller yet; stays a `notYetImplementedError`
  stub (kept for Q1 parity).
- [x] `path.runtime`, `path.resource`, `path.dataDir` — resolve from `global`
  (`runtimeDir`/`resourceDir`/`dataDir`), pre-launch, ambient. Port Q1's
  `appdirs.ts::quartoDir`:
  - **`runtime(subdir?)` / `dataDir(subdir?, roaming?)` CREATE the dir** (#4) —
    `host.fs.ensureDir(join(global.<base>Dir, subdir))`, recursive, **errors
    propagate.** Load-bearing: Julia's `juliaRuntimeDir()` (`julia-engine.ts:944`)
    calls `path.runtime("julia")` and treats a throw as "could not create… permission
    issue"; a pure string read would silently skip creation and break Julia downstream.
    (This is why `makePathHost` needs `fs` — see the seam above. Use a small internal
    `join`; no `node:path`/`@std/path` — portability constraint #2.)
  - **`resource(...parts)` does NOT create** — `join(global.resourceDir, ...parts)`,
    no `ensureDir` (Q1 parity: resources are read-only; creating one would mask a
    missing-resource / packaging bug).
  - **`roaming` is a no-op** (#6) — Q1's Windows roaming/local split needs two bases;
    `global` ships one resolved `dataDir`. Keep the param in the signature (Q1-source
    compat) but ignore it; documented divergence. If a real roaming consumer appears,
    ship a second base on `HostGlobalConfig` then.
  - Otherwise match `quartoDir` exactly — plain join + `ensureDir`, no extra
    normalization (Julia does its own `toForwardSlashes`).
- [x] **Bind the deferred Test Seam Spec rows** (Phase 0) against the now-real
  signatures: the ensureDir test (assert `fs.ensureDir` invoked for `runtime`/`data`,
  **not** `resource`; a throwing `ensureDir` propagates — revert the `ensureDir` call →
  RED), the `system.pandoc` errors-on-None test (revert the None-guard → no "pandoc
  unavailable" rejection → RED), and the `makePathHost(host, global)`/`makeSystem(host,
  global)` seam tests (revert the `global` threading → body reads `undefined` dir → RED).
  Fake `PlatformHost` + fake `global`; mock only the genuine env dep.

> The pure/host-only methods these sit beside (`path.absolute`,
> `path.dirAndStem`, `system.execProcess`, `system.tempContext`, …) are already
> real in §2aa; only the context-dependent bodies above are deferred here.
> (Per **B1** below, `system.execProcess` carries Q1's `mergeOutput`/
> `stderrFilter` — knitr uses both, `rmd.ts:440` — by bringing the runtime up to
> Q1's six-positional signature (not flattening); the knobs thread through
> `PlatformHost.ExecOptions` and `mergeOutput`/`stderrFilter` get real
> `denoHost` bodies. B1 was relocated here from RTQ by the Option A split.)

## Phase B — @quarto/types and import map (was Phase 2E)

Following Quarto 1's model, engine extensions import types via
`import type { ... } from "@quarto/types"`. These are erased during the
build step (bundling), so no runtime code is needed — just a `.d.ts` file
referenced by the import map.

- [x] Refine the q2-specific type surface in `@quarto/types` — the package is
  **vendored from Q1 by Plan 2A** (`ts-packages/quarto-types/`); this phase
  adjusts and extends that baseline to match q2's signatures:
  - `ExecutionEngineDiscovery`, `ExecutionEngineInstance`
  - `ExecuteOptions`, `ExecuteResult`, `ExecutionTarget`
  - `QuartoAPI` (with our namespace signatures)
  - `MappedString`, `PartitionedMarkdown`, `Metadata`
  - `EngineProjectContext`
  - **`LanguageClaim`** — the kind-tagged claim returned by `claimsLanguage`:
    `{ kind: "primary" | "interop" | "fallback"; priority?: number }`.
    `ExecutionEngineDiscovery.claimsLanguage`'s return type widens to
    `boolean | number | LanguageClaim | null` — the `boolean`/`number` forms
    stay Q1-compatible (the harness normalizes them; a bare `number` is always
    a `primary`, never interop), and `interop`/`fallback` are reachable only via
    the object. This is the one deliberate Q1-API extension in the epic; see
    plan1b's normalization and `claude-notes/designs/engine-resolution.md` §3.2.
- [x] **Anchor the pure/host-only/ambient classification as jsdoc on `QuartoAPI`.**
  Plan 1b's "Engine API contract" table (which methods are pure, host-only, or
  ambient — resolving from the `Init { global }` config) is the source of truth.
  Record it as jsdoc on the `QuartoAPI` type here so the harness assembly (Plan 1b)
  and the namespace bodies (§2aa) agree against a written contract rather than by
  convention. Plan 1b's table remains canonical; this mirrors it. (There is **no
  gated class** — RTQ Item A.)
- [x] **Claim constructors live in `@quarto/api` (runtime), not `@quarto/types`
  (erased).** Add tiny helpers `primary(priority?)`, `interop(priority?)`,
  `fallback(priority?)` that return the corresponding `LanguageClaim` objects,
  so authors write `claimsLanguage: (lang) => lang === "julia" ? primary() :
  null` instead of hand-writing tags. **Omitted priority → bare `{kind}`, not a
  baked-in default** (#7): `primary()` ⇒ `{kind:"primary"}`, `interop(3)` ⇒
  `{kind:"interop",priority:3}`. The harness normalization owns the defaults
  (`?? 1` for primary, `?? 0` for interop/fallback — `engine-resolution.md` §3.2),
  so baking them into the constructor too would duplicate the default and invite
  drift. Bound by **T-B2-claims**. Export from `@quarto/api` (a small
  `@quarto/api/claims` subpath or the package root); pure data, no host
  dependency.
- [x] For compatibility with Quarto 1 extensions: our type names should match
  Quarto 1's.
- [x] **Document the init() timing in the `ExecutionEngineDiscovery.init`
  jsdoc.** Plan 1b builds the QuartoAPI over the `Init { global }` config at
  harness assembly (RTQ Item A — no shared mutable `HostState`, no launch-gating).
  Update the `init?` jsdoc to spell out:
  - When `init()` runs (during `loadEngine` handling, after the
    module's exports are validated).
  - What's available immediately — **everything the engine needs pre-launch**:
    the pure namespaces (`text`, `markdownRegex`, `console`, `crypto`), the
    host-only namespaces (`mappedString`, `path`, `system`), and the ambient
    `path.runtime`/`resource`/`dataDir` + `system.pandoc` (they resolve from the
    injected `Init { global }`). `format.*` is always available too — every
    predicate takes a `Format` arg, so it is never gated.
  - The contract that engines may NOT access `quarto.*` at module
    top-level — only from inside methods.
  - That `init()` is sync per Q1's contract, but the harness `await`s
    its return defensively, so an `async init()` works correctly.
  - That throwing/rejecting from `init()` is a fatal load failure.
  - That the per-render **project** context arrives separately on each
    `launchEngine` (captured in the instance closure), not via `init()`.
  Cross-reference Plan 1b's "Engine API contract" section as the
  canonical method table.
- [x] Create a template `resources/extension-build/deno.json` that engine
  authors copy/extend. **Already landed by plan1c Task 12** (`resources/extension-build/deno.json`
  + `resources/extension-build/deno.workspace.json`): the shipped `deno.json` already maps
  `@quarto/api`/`@quarto/types` to `jsr:` and the std lib to `jsr:/@std/`, and `deno.workspace.json`
  provides the in-repo dev mapping to `ts-packages/…`. **This item is now reconcile-and-verify, not
  create** — confirm the landed files match the intent below. (The example below is illustrative and
  slightly stale: the landed `deno.json` uses a single `@std/` prefix map rather than per-module
  `@std/path`/`@std/fs`/`@std/encoding` entries.) Its imports reference the **published** SDK and
  std lib (no q2-local import map for the SDK):
  ```json
  {
    "compilerOptions": { "strict": true, "lib": ["deno.ns", "DOM", "ES2021"] },
    "imports": {
      "@quarto/api": "jsr:@quarto/api",
      "@quarto/types": "jsr:@quarto/types",
      "@std/path": "jsr:@std/path",
      "@std/fs": "jsr:@std/fs",
      "@std/encoding": "jsr:@std/encoding"
    }
  }
  ```
  Within the q2 repo, a workspace mapping resolves `@quarto/api` /
  `@quarto/types` to `ts-packages/…` for dev builds against unpublished
  changes.

### Phase B is the `@quarto/types` coherence gate (sequencing: 1b → 2 → 3)

`@quarto/types` has three writers and, until now, no named owner — which is how it
silently drifts. **Phase B is the explicit consolidation/coherence gate**, not one
editor among three:

- **1b** added `HtmlDependency` (`pandoc.ts:28`, mirrors Rust `TsHtmlDependency`) — a
  landed gap-fill. **Reconcile *around* it; do not redefine, move, or drop it.**
- **Phase B** (this section) refines the bulk: `ExecutionEngineDiscovery`/`Instance`,
  `ExecuteOptions`/`Result`/`Target`, the `QuartoAPI` namespace signatures, `MappedString`,
  `EngineProjectContext`, `LanguageClaim`, plus the B1 `execProcess` alignment.
- **Plan 3** consumes the vendored jupyter *data* types and (RTQ B5) relies on Phase B
  having reconciled the jupyter *namespace signatures* its runtime typechecks against.

**Sequencing assumption — 1b → 2 → 3.** This only works clean in that order: 1b's
`HtmlDependency` lands before Phase B; Phase B's reconciled contract lands before Plan 3's
runtime. Do **not** run Phase B before 1b.

**Scope this gate must own (beyond the bulk list above):**

1. **Jupyter namespace *signatures* (not just data types).** The jupyter data types
   (`JupyterNotebook`, `JupyterWidgetDependencies`, …) are already vendored by 2A; Phase B
   must additionally reconcile the **method signatures** Plan 3 implements — specifically
   `widgetDependencyIncludes` (now an in-scope producer for RTQ FC-2). Its return is
   `PandocIncludes` (`quarto-api.ts:295-298`), which **must line up with**
   `DependenciesResult.includes` and the Rust wire type `TsPandocIncludes`
   (`ts_protocol.rs:144,267`). Putting the signature in Phase B is what lets Plan 3's runtime
   typecheck against a frozen contract.
2. **The TS side of the dual-defined wire types — mind the *three* distinct types.**
   - **Wire types** (Rust ↔ the hand-written TS mirror
     `ts-packages/quarto-engine-host-deno/src/types.ts`): `HostGlobalConfig`,
     `TsPandocIncludes`, `EngineProjectContext` (author-facing `project-context.ts:51`;
     wire mirror in `types.ts`), and the wire claim enum **`TsLanguageClaim`** — these
     **already exist on both sides**. The Rust wire enum is `TsLanguageClaim`
     (`ts_protocol.rs:216-222`, `#[serde(tag="kind", rename_all="lowercase")]`); the TS
     mirror is `TsLanguageClaim` (`types.ts:87`). Phase B's job here is to keep the TS
     mirror's *shapes* matching the Rust structs (enforced by the parity check below),
     **not** to create these.
   - **The native Rust `LanguageClaim`** (`engine/mod.rs:105`) is a resolution enum with
     **no serde** — it is **not** a wire dual. Its only seam with the wire type is
     `From<Option<TsLanguageClaim>> for LanguageClaim` (`ts_engine.rs:65`). Do not confuse
     it with the wire enum.
   - **The author-SDK `LanguageClaim`** (`{kind: "primary"|"interop"|"fallback";
     priority?: number}`) is the genuinely **net-new** type — **Phase B creates it in
     `@quarto/types`** (absent from `quarto-types/src/` today). This is the ergonomic
     author-facing shape the `primary`/`interop`/`fallback` constructors produce.
   - **Normalization already lives in TS**, not Rust: `mapLanguageClaim`
     (`host.ts:150-168`) maps Q1's `boolean|number` → `TsLanguageClaim | null`
     (`false/null/undefined→null`, `true→{primary,1}`, `n→{primary,n}`, negative =
     low-priority primary, **never** interop — matching `engine-resolution.md` §3.2). Phase
     B does **not** re-implement this; it only anchors the type shapes. This is also the TS
     half of the Phase-A seam (#3).

**Success criterion — the gate, tiered (consumers land at different times):**

- [x] **At Phase B:** the engine TS packages' **per-package `tsc --noEmit`** are green with every
   *present* consumer compiling against the one `@quarto/types` — 1b's `buildQuartoAPI` assembly
   (typed `: QuartoAPI`, B2's conforming version; already a compiled unit in
   `quarto-engine-host-deno`'s `tsc`) **and** an author-style fixture. **Mechanism — there is *no*
   `tsc -b` project-references graph over ts-packages** (each package runs plain `tsc`; cross-package
   types resolve via node16 `exports`): add the fixture as an **`include`d source file** in an engine
   TS package (e.g. `ts-packages/quarto-engine-host-deno/src/__fixtures__/engine-fixture.ts`, or
   `quarto-api`), importing `ExecutionEngineDiscovery` from `@quarto/types` and `primary` from
   `@quarto/api` via their `exports` — so it is a genuine compiled unit, not a dangling file. **The
   gate subsumes B2's conformance check — one mechanism, not two:** B2 makes the `@quarto/api`
   *runtime* conform (producer side); the fixture + `buildQuartoAPI` typecheck makes all *consumers*
   conform; the same `tsc` pins the contract from both sides. The jupyter *signatures* are present and
   wire-aligned even though Plan 3's *runtime* isn't built yet.
- [ ] **At Plan 3:** the jupyter runtime compiles against the **frozen** contract with **zero**
   `@quarto/types` edits. If it needs an edit, that is a **logged Phase B miss**, not a silent
   Plan 3 patch. *(Left unchecked deliberately — this is a forward criterion verified when Plan 3
   builds the jupyter runtime; it is not Plan 2 work. The contract it must hold against is now frozen.)*
- [x] **TS↔Rust parity (tsc's blind spot) ‡.** A green `tsc` proves TS-internal coherence only; it
   cannot see a Rust rename of a dual-defined wire field, and **TS interface keys are erased at
   runtime**, so "assert every key is a field of the TS interface" is not directly reflectable.
   **No cross-language parity mechanism exists in the repo today** — the TS mirror `types.ts` is kept
   in sync by its "source of truth: `ts_protocol.rs`" header comment alone (manual discipline), and
   the existing serde tests (`ts_protocol.rs:985,1147`) are **Rust-only round-trips that emit no
   artifact**.

   **‡ Recommended mechanism** (mechanizable in both directions):
   1. A Rust `#[test]` serializes one canonical instance of each **wire-dual** type
      (`HostGlobalConfig` — all 7 fields, `TsPandocIncludes`, `EngineProjectContext`,
      **`TsLanguageClaim`**) and writes it to a committed fixture, regen-gated the way
      `crates/quarto-error-reporting/tests/schema_drift.rs` gates its schemas
      (`QUARTO_REGEN_SCHEMAS=1` to rewrite; CI fails on drift).
   2. A deno-tier test in `quarto-engine-host-deno` loads each fixture and asserts
      `Object.keys(sample)` **set-equals** a runtime key list pinned to the TS type:
      ```ts
      const HOST_GLOBAL_CONFIG_KEYS = [
        "resourceDir","runtimeDir","dataDir","pandocPath",
        "isInteractiveSession","runningInCi","quartoVersion",
      ] as const satisfies readonly (keyof HostGlobalConfig)[];
      type _Exhaustive =
        Exclude<keyof HostGlobalConfig, (typeof HOST_GLOBAL_CONFIG_KEYS)[number]> extends never
          ? true : never;   // compile error if KEYS omits a field
      ```
      A Rust-only key (fixture has it, `KEYS` lacks it) fails the set-equality at runtime;
      a TS-only key (`KEYS`/type has it, fixture lacks it) also fails set-equality, and a
      `KEYS` that drifts from the type fails to compile. This binds all three: Rust shape,
      TS shape, and the runtime list.

   **Accepted fallback** if the above is too heavy for this plan's scope: a *documented sync
   checkpoint* in the `types.ts` header enumerating the wire-dual types and their Rust source
   structs. "Single `tsc` green" is necessary but **not** sufficient for the wire-dual types.

## Phase 0 — Test Seam Spec (frozen)

Per **prevalidating-test-seams** + **fail-on-revert**: each row names the real unit mounted, the mock
boundary, and the **one production hunk whose revert turns the named assertion RED**. Once a test goes
green its assertions + harness are **frozen** — never edited to go green. This table is the binding
contract; the prose sketches in B1/B2 are context, not the spec.

| # | Tier | Real unit | Seam (mount + events + assertion) | Mock boundary | Named revert → RED |
|---|------|-----------|-----------------------------------|---------------|--------------------|
| T-B1a | vitest (marshalling) | real `makeSystem` | call `execProcess({cmd}, stdin, "stdout>stderr", f)` **positionally**; assert recorded `ExecOptions.mergeOutput === "stdout>stderr"` **and** `ExecOptions.stderrFilter === f` (ref) | fake `PlatformHost` (records `ExecOptions`) | drop the positional-param→`ExecOptions` threading → fields absent → RED |
| T-B1b-merge | **deno** (real subprocess) | real `denoHost.process.exec` | child writes `OUT` to stdout **and** `ERR` to stderr; `execProcess(_, _, "stdout>stderr")`; assert merged sink contains **`OUT`** | none (real `Deno.Command`) | revert `mergeOutput` routing → `OUT` not in merged sink → RED |
| T-B1b-filter | deno | real `denoHost.process.exec` | child writes `ERR`; `stderrFilter = s => "F:"+s`; assert captured stderr starts with **`F:`** | none | revert `stderrFilter` application → stderr unprefixed → RED |
| T-B1b-timeout | deno | real `denoHost.process.exec` | child sleeps **longer** than `timeout`; assert the call rejects/killed (did **not** complete) | none | revert `Promise.race`/`kill` → sleeper completes → "killed" assertion RED |
| T-B1c-gate | vitest (marshalling) | real `makeSystem` | `execProcess({cmd, stdout:"piped"}, _, "stdout>stderr")`; assert `ProcessResult.stdout` reflects the merge (Q1 `process.ts:138-142`) | fake host | revert the merge-vs-`stdout`-gating handling → stdout populated as if unmerged → RED |
| T-B2-conform | tsc (`--noEmit`) | real `buildQuartoAPI` + derived namespace interfaces | assign a deliberately mis-typed namespace into the assembly; `tsd` `expectError` on **that line** | none (type-level) | re-add `as unknown as QuartoAPI` cast (or revert `SystemNamespace = QuartoAPI["system"]` derive) → mis-typed compiles → `expectError` unsatisfied → RED |
| T-B2-onCleanup | tsc | real `QuartoAPI["system"].onCleanup` **param type** | `tsd` `expectType<() => void>` on `Parameters<QuartoAPI["system"]["onCleanup"]>[0]` — assert the handler param is **exactly** `() => void`. **Do NOT** `expectError` on an async handler: void-return assignability lets `() => Promise<void>` satisfy `() => void`, so it compiles either way and `expectError` would be unmountable (RED in both states). | none (type-level) | revert the prune (re-widen to `() => void \| Promise<void>`) → param type widens → `expectType<() => void>` mismatch → RED |
| T-B2-claims | vitest (pure) | real `primary`/`interop`/`fallback` | `expect(primary()).toEqual({kind:"primary"})`; `expect(interop(3)).toEqual({kind:"interop",priority:3})` | none | make `primary()` bake `priority:1` → `toEqual({kind:"primary"})` RED (extra key) |
| T-Gate-tsc | per-package `tsc --noEmit` | real `@quarto/types` + all *present* consumers as compiled units | `buildQuartoAPI` (`: QuartoAPI`, no cast) **and** an author fixture (`engine-fixture.ts` importing `ExecutionEngineDiscovery` from `@quarto/types`, `primary` from `@quarto/api`) are **`include`d source files** in an engine package's `tsconfig` (deps resolved via node16 `exports` — there is no `tsc -b` graph); assert the package `tsc --noEmit` exits 0 | none | drop a `QuartoAPI.system` method from a namespace impl → a consumer fails to compile → non-zero exit |
| T-Gate-parity | cross-lang | real Rust serde output + real TS mirror `types.ts` | a Rust test emits one canonical serde-JSON fixture per wire-dual type (`HostGlobalConfig` — **all 7 fields**, `TsPandocIncludes`, `EngineProjectContext`, **`TsLanguageClaim`** — **not** the native `LanguageClaim`); a deno-tier test set-equals each fixture's key-set against a `satisfies readonly (keyof T)[]` runtime `KEYS` list (mechanism ‡ above). Symmetric: a Rust-only key **and** a TS-only key both fail | none (real serde + real TS type) | rename a Rust serde key (e.g. `resourceDir`→`resource_dir`) → key absent from fixture key-set → set-inequality → RED |

**Vacuity guards (these go green *without* the feature unless held):**
- **T-B1b-merge** — the child MUST write to **both** streams with distinguishable content, and the
  assertion targets the **stdout** content appearing in the merged sink. Writing to stdout only passes
  with or without merge.
- **T-B1b-timeout** — the sleeper MUST **outlast** the timeout and the assertion is "killed / did not
  complete." A sleep shorter than the timeout never exercises the kill path.
- **T-Gate-tsc** — the fixture + `buildQuartoAPI` MUST be **compiled units** actually reached by a
  package's `tsc` (`include`d source files, deps resolvable via node16 `exports`), or "tsc green"
  says nothing about them.
- **T-Gate-parity** — derive at least the Rust side from a real serde **artifact**; two hand-maintained
  field lists survive their own revert (the `Dv`/`Dv` collapse trap). **The canonical Rust instance
  MUST populate every optional field** (`EngineProjectContext.projectDir?/config?/outputDir?`,
  `pandocPath?`): serde omits `None`s from the JSON, so an unpopulated optional drops its key from
  `Object.keys(fixture)` and false-fails set-equality against the full `KEYS` list.

**Missing-test pass — accepted-untested (logged, not silently omitted):**
- **`respectStreams` body** — implemented by the wholesale `core/process.ts` port but **not separately
  asserted.** Rationale: no in-scope engine sets it (knitr/jupyter are native Rust; julia/marimo bypass
  the seam); carried for Q1 signature parity. If a future engine sets it, add a deno-tier
  stream-separation assertion then.
- **Whole-subprocess SIGKILL / channel teardown** — owned and tested on the Rust side (plan1a-host),
  not here.

**Deferred until the Phase A batch is written (bind against real signatures, not speculation):** the
`path.runtime`/`dataDir` ensureDir test (revert the `ensureDir` call → "invoked for runtime/data, not
`resource`" RED), the `system.pandoc` errors-on-None test (revert the None-guard → no "pandoc
unavailable" rejection → RED), and the `makePathHost(host, global)` / `makeSystem(host, global)` seam
tests. These have named reverts in discussion but are not frozen here because their signatures land
with Phase A.

## B1 — restore `system.execProcess` `mergeOutput`/`stderrFilter` (return-to-Q1; relocated from RTQ)

**Relocated from `2026-06-25-plan1a-return-to-q1.md` by the Option A split (2026-06-29).** B1 is a
standalone return-to-Q1 correction of **landed §2aa code** — it touches no 1a file, only
`@quarto/api` + the vendored types — so it belongs with Plan 2's `@quarto/api` work, not with RTQ's
landed-1a corrections. It is testable **now** against a fake `PlatformHost` (no Deno harness
needed).

**Severity:** Low–Moderate · **Necessary?:** unforced reduction (return-to-Q1) · **Touches:**
`@quarto/api` (`system/index.ts`, `platform/index.ts`), vendored `@quarto/types`
(`quarto-api.ts`).

**Verified (2026-06-26):** Q1 `core/api/types.ts:165-172` declares `execProcess(options, stdin?,
mergeOutput?: "stderr>stdout"|"stdout>stderr", stderrFilter?, respectStreams?, timeout?)` (6-param).
knitr `rmd.ts:440-458` calls it with `"stdout>stderr"` **and** a `stderrFilter` closure — real
engine-author use. q2 runtime `system/index.ts:97-100` is 2-param `(options, stdin?)`;
`ExecProcessOptions` (`:43-58`) and `platform/index.ts:25-32`'s `ExecOptions = {cwd?, env?, stdin?}`
carry **neither** knob — they have no home below the seam. The vendored
`@quarto/types/quarto-api.ts:616-623` keeps the full 6-param signature → **runtime and vendored
signatures disagree**, so the `QuartoAPI` aggregation can't typecheck until reconciled. Engine
survey: only knitr (`rmd.ts:440`, the 2 knobs) and `jupyter-kernel.ts:181` (2-arg, unaffected) call
`execProcess`; julia/marimo use raw `Deno.Command`. **`respectStreams`/`timeout` are used by no
engine, so their *bodies* may be deferred — but the signature stays Q1-shaped (see Decision);**
`mergeOutput`/`stderrFilter` are the real gap and get real bodies now.

**Decision (revised 2026-06-30): restore-now, Q1-shape — in scope.** No *in-scope* TS engine uses
these today (knitr/jupyter are native Rust in q2; julia/marimo bypass the seam), but the framework
must carry them: future TS engines and the grand plan's Q1-adoption path (Q1's `QuartoAPIRegistry`
delegating to `@quarto/api` providers). **Keep Q1's signature — do NOT flatten.** The vendored
`@quarto/types` (`quarto-api.ts:616-623`) already carries Q1's six-positional
`execProcess(options, stdin?, mergeOutput?, stderrFilter?, respectStreams?, timeout?)`; that type is
already correct, and it is the *runtime* that is under-built (2-param). Reconcile **upward** — bring
the runtime to the six-positional shape (no change to the vendored type) and thread the knobs through
the seam internally. There is **no technical reason to flatten**: `execProcess` is an **in-process**
call (the `stderrFilter` *closure* proves it never crosses the wire), so there is no serde/wire
pressure, and `host.process.exec`'s `ExecOptions` carries the knobs below the seam regardless of how
the public parameters are spelled. Flattening would only buy named-options ergonomics while
*regressing* the Q1 parity this section ("return-to-Q1") exists to preserve — and under B2#3 (derive
`SystemNamespace = QuartoAPI["system"]`) it would lock that regression into the single source of
truth.

- [x] **Runtime (`@quarto/api`).** Bring `SystemNamespace.execProcess` (`system/index.ts:97-100`) to
  Q1's six-positional signature `(options, stdin?, mergeOutput?, stderrFilter?, respectStreams?,
  timeout?)`. Add `mergeOutput`/`stderrFilter`/`respectStreams`/`timeout` to `PlatformHost.ExecOptions`
  (`platform/index.ts:25-32`) and **implement all four in `denoHost.process.exec`** — a wholesale port
  of Q1 `core/process.ts:46-190` (route stdout↔stderr per `mergeOutput`; map each stderr chunk through
  `stderrFilter`; `respectStreams` is the two stream-passthrough ternaries at `:168,183`; `timeout` is
  `Promise.race` + `process.kill()` at `:32,55-59`). **Implement all four, not half:** under B2#3 the
  derived signature carries all six params regardless, so a "present but ignored" knob is a silent
  no-op trap — and they all live in the *same* ~70-line function, so half-porting saves almost
  nothing. Do **not** flatten the knobs into `ExecProcessOptions`.
- [x] **Types (Phase B / B2#3).** No change to the vendored `@quarto/types` `execProcess` — it is
  already Q1-shaped. Under B2#3 the runtime `SystemNamespace` *derives* from `QuartoAPI["system"]`, so
  "runtime == vendored" holds **by construction**; the only work is making the runtime *impl* conform
  (the bullet above). (This replaces the former "align the vendored type to the flattened runtime"
  step, obsolete under the keep-Q1-shape decision.)
- [x] **Test seams T-B1 — two tiers.** *(a) Wiring (TS/vitest, frozen):* mount `makeSystem(fakeHost)`
  with a fake `PlatformHost` that records the `ExecOptions` it receives; call
  `execProcess({cmd}, stdin, "stdout>stderr", f)` **positionally** (Q1's call shape, as knitr writes
  it — `rmd.ts:440`); assert the recorded `ExecOptions` carries `mergeOutput` **and** the
  `stderrFilter` ref. *Named revert:* drop the two fields from the `execProcess → ExecOptions`
  threading → the fake never sees them → RED. *(b) Behavior (deno-tier, `deno-host.deno-test.ts`):*
  the fake-host test proves *wiring*, not that the stream port *works* — spawn a real subprocess that
  writes to both stdout and stderr, call `execProcess` with `mergeOutput` + a `stderrFilter` (and a
  `timeout` on a sleeper), and assert the captured output is actually merged/filtered and the sleeper
  is actually killed. *(Mount the real `makeSystem`/`denoHost`; mock only the genuine environment dep.)*
- [x] **Re-green note (mostly additive).** The landed `system.test.ts:84-160` execProcess tests use
  the 2-arg call form; the new positional params are optional, so those tests stay valid unchanged.
  The one invariant to recheck: the `stdout`-gating test (`:151-159`, "maps stdout only when piped")
  must still hold once `mergeOutput: "stdout>stderr"` routes stdout into stderr (Q1 handles this at
  `process.ts:138-142`).

## B2 — QuartoAPI conformance: retire the harness `as unknown as QuartoAPI` cast (found in Plan 1b)

Plan 1b's `buildQuartoAPI` assembles the §2aa namespaces and asserts the result with a
broad **`as unknown as QuartoAPI`** cast (`quarto-engine-host-deno/src/quarto-api.ts:243`)
because `@quarto/api`'s per-namespace interfaces (`SystemNamespace`, …) and the loosely-typed
stubs **do not structurally conform** to the vendored `QuartoAPI` (`@quarto/types/src/quarto-api.ts`).
The cast is correct today (the final 1b review verified it hides no mis-wiring) but it **suppresses
compile-time checking on the whole assembly** — a future mis-wired namespace would not be caught.
This is **maintainability, not a runtime bug** (no functional consumer), which is why it rides here
in Plan 2 — the plan already in these files for the bodies (Phase A) + the `@quarto/types`
reconciliation (Phase B) — and **not** in 1c. The `execProcess` 6-vs-2-param divergence is **handled
by corrected B1** (keep Q1's six-positional signature; the runtime conforms — see B1); under B2's
derivation that decision becomes the single source of truth, so **B1 must land with B2.** B2 closes
the *remaining* divergences and deletes the cast:

- [x] **Type the staying-stub methods to their real Q1 returns + `throw` (not `...args: unknown[]`).**
  In `@quarto/api/system/index.ts`, `checkRender` (`:130`) and `runExternalPreviewServer` (`:136`)
  are typed `(...args: unknown[]): Promise<unknown>` — the loose shape that **forces** the cast
  (`Promise<unknown>` ⊄ `Promise<CheckRenderResult>`; ⊄ `PreviewServer`). Re-type them to the real
  `QuartoAPI.system` signatures (`checkRender(o: CheckRenderOptions): Promise<CheckRenderResult>`;
  `runExternalPreviewServer(o): PreviewServer`) and keep the `notYetImplementedError` throw — a
  throwing body satisfies any declared return, so they conform *and* stay stubs. (Phase A keeps their
  bodies stubbed; B2 only fixes their *types*.) **Async/sync split — important:** `checkRender`
  returns `Promise<CheckRenderResult>` → keep it `async` (it throws as a *rejected promise* — the
  §2aa stub contract, `system/index.ts:298-300`, which protects `.catch()`-style callers).
  `runExternalPreviewServer` returns `PreviewServer` **synchronously** in Q1, so it becomes a
  **synchronous throwing stub** — do **NOT** keep it `async`. The "stubs are async" rule applies only
  to genuinely `Promise`-returning methods; async-izing this one was itself a Q1 divergence B2 is
  removing, and re-adding `async` would re-break conformance under B2#3.
- [x] **Export `text.postProcessRestorePreservedHtml` from `@quarto/api/text`** (it is currently
  **not exported** — `text/index.ts:9` "DEFERRED, does file IO"; the harness fills the gap with a
  local throwing stub at `quarto-api.ts:135-140`). Export it typed to the real
  `(o: PostProcessOptions): void` signature, throwing `notYetImplementedError` until its body lands
  (it does file IO via `PlatformHost` — a natural Phase-A-style body when a consumer appears). This
  lets the harness drop its local stub.
- [x] **Make conformance compiler-enforced (the durable fix — Fix B), split by namespace category.**
  Have `@quarto/api`'s interfaces *derive from* the SDK contract instead of redefining it — but
  respect the two factory categories `index.ts` defines (a naive "derive everything from
  `QuartoAPI[ns]`" does **not** compile for the mostly-pure namespaces):
  - **Fully-host namespaces** (`console`, `system`) — the factory returns the *whole* namespace, so
    derive directly:
    ```ts
    // @quarto/api/system/index.ts
    import type { QuartoAPI } from "@quarto/types";
    export type SystemNamespace = QuartoAPI["system"];
    ```
  - **Mostly-pure namespaces** (`path`, `mappedString`) — the `make<Ns>Host` factory returns only the
    *host subset*; the full namespace is assembled by mixing pure exports + that subset
    (`quarto-engine-host-deno/src/quarto-api.ts:176-182, 197-204`). So `PathHostNamespace =
    QuartoAPI["path"]` is a **category error** (it forces `makePathHost` to also return the pure
    functions). Instead derive the *subset* — `export type PathHostNamespace = Pick<QuartoAPI["path"],
    "absolute" | "runtime" | "resource" | "dataDir">` — and enforce the full shape where it is actually
    assembled: annotate the mixed object `… satisfies QuartoAPI["path"]` in `buildQuartoAPI` (likewise
    `mappedString`). Bullet 4's `: QuartoAPI` on `buildQuartoAPI` is the end-to-end backstop and catches
    the full assembly regardless.

  **Deriving is not mechanical — it surfaces real shape reconciliations.** Expect (and resolve, with
  intent) at least:
  - **`system.onCleanup` → prune to sync (`() => void`).** **The prune is safe for existing callers
    and does *not* reject async handlers:** by TS's void-return assignability a `() => Promise<void>`
    handler still satisfies `() => void` (`const f: () => void = async () => {}` compiles), so no call
    site breaks — the prune only makes the *declared* type match Q1's actual `VoidFunction` registry.
    Do **not** claim the narrowing "catches" async handlers (it cannot), and do **not** bind a test to
    an `expectError` on an async handler — it would be unmountable (see T-B2-onCleanup). Vendored is
    `(handler: () => void | Promise<void>) => void` (`quarto-api.ts:643`), but Q1's *actual* cleanup
    registry `core/cleanup.ts:11` types the handler `VoidFunction` and calls it without `await`
    (`:20`) — and denoHost's `onExit` (`deno-host.ts:140`) registers an `unload` listener Deno does
    not await (pending promises drop on `Deno.exit`). So the `Promise<void>` is an **aspirational type
    Q1 itself never honors.** Prune the vendored type to `(handler: () => void) => void` — it conforms
    at all three levels (type, runtime, denoHost) and is **more** Q1-faithful, not a divergence. Do
    **not** widen `onExit`; that would build an await path Q1 lacks.
  - **`execProcess`** — covered by corrected B1 (keep Q1's six-positional; runtime conforms).
  - Audit `TempContext` and `ProcessResult`/`ExecResult` for the same subset/shape drift while wiring
    the derive.

  Where q2 *deliberately* won't honor a Q1 signature, prune the vendored `QuartoAPI` to match — **with
  intent** — rather than carry a parity lie (a Phase-B `@quarto/types` edit). Then the compiler forces
  every impl to match the engine-author contract **at the source**, so drift cannot silently reappear.
- [x] **Delete the harness cast + local stub (the payoff).** With the above in place,
  `buildQuartoAPI` (`quarto-engine-host-deno/src/quarto-api.ts`) assembles genuinely-conforming
  namespaces: remove `as unknown as QuartoAPI` (type it `: QuartoAPI`), and remove the local
  `postProcessRestorePreservedHtml` stub. The `jupyter` `Proxy` cast may remain until Plan 3 ships
  the real `jupyter` namespace (then it too is typed). After this, a real mis-wiring of `buildQuartoAPI`
  is a **compile error** again — the safety net 1b had to switch off is back on.
- [x] **Test seam T-B2 (TS/tsc, frozen — a *negative* type test).** Add a type-level fixture where a
  deliberately mis-typed namespace is assigned into the `buildQuartoAPI` assembly and **must** fail to
  compile. Prefer a `tsd`-style `expectError`/type-assertion that pins the *specific* mismatch over a
  bare `// @ts-expect-error` (which passes on *any* error on the line, including an unrelated one), and
  run it under the engine package's `tsc --noEmit` (the same typecheck the Phase-B gate uses — there is
  no `tsc -b` project-references graph over ts-packages; CLAUDE.md's stricter-`tsc -b` note is about
  hub-client, not these packages). *Named revert:* re-introduce the
  `as unknown as QuartoAPI` cast → the mis-typed assignment compiles → the negative assertion → RED.
  This binds "the cast is gone and conformance is enforced," not merely "it compiles today."

## Portability constraints

The goal is that `@quarto/api` can later move to its own repo and/or be
consumed by Quarto 1. To keep that option cheap, the plan commits to:

1. **Self-contained package.** Own `package.json`, own `tsconfig.json`, own
   tests. No `../../some-q2-thing` imports.
2. **No Deno globals inside `@quarto/api`.** All platform I/O goes through
   the `PlatformHost` interface. `@quarto/api` itself never references
   `Deno.*`, `globalThis.Deno`, `node:*`, or platform-specific modules.
   This is the invariant that lets the same package run under
   `@quarto/engine-host-deno` today and `@quarto/engine-host-wasm` later.
3. **Bootstrap mechanism NOT ported.** We port implementations only, not
   Q1's `QuartoAPIRegistry` / `register.ts` / `getQuartoAPI()` singleton.
   Engine-host builds the QuartoAPI via direct construction. Q1, if it
   adopts `@quarto/api` later, keeps its own registry and just replaces the
   provider bodies with calls into our submodules.
4. **ESM + package.exports map.** Committed from day one so bundlers and
   Q1's future import paths don't have to be renegotiated.
5. **No cross-package coupling to engine-host.** `@quarto/api` never imports
   from `@quarto/engine-host-deno` — the dependency runs only one direction.
   q2-specific glue (protocol types, source-map rehydration) lives in
   `@quarto/engine-host-deno`, not here.
6. **Published to a registry.** `@quarto/api` (and `@quarto/types`) are
   published to jsr.io or npmjs.com as appropriate — this is how engine authors
   get the SDK (see the grand plan's "Distribution of the engine-author SDK").
   The package carries a `version`, plus a `publishConfig` for npm.
7. **Scope naming.** `@quarto/api` is intended to coexist with Q1's existing
   `@quarto/types`. If Q1's package layout changes, we coordinate naming.

## Design Notes

These rationales are now **realized in §2aa** (the namespaces were rewritten,
not extracted, in a single `@quarto/api` package); they are kept as the
durable justification for those choices.

### Why rewrite instead of extract?

Quarto 1's markdown utilities are tangled with the YAML schema/validation
system (~30+ files), tree-sitter, mapped-text infrastructure, and lodash.
Clean rewrites of the actual logic are ~50-300 lines per function, vs.
extracting would require bringing 30+ files and stubbing their dependencies.
The logic itself is straightforward — it's the plumbing that's tangled.

### Why a single `@quarto/api` package?

We use a single `@quarto/api` package with subpath exports rather than separate
`@quarto/markdown` / `@quarto/jupyter` sibling packages because:

- One `package.json`, one version, one dep list (`yaml` lives once).
- Q1 adopts once (`import { ... } from "@quarto/api/markdownRegex"`), not three times.
- MappedString has a natural home (`@quarto/api/mappedString`) without debate over which
  sibling owns it.
- Cross-submodule deps (if any) don't require version coordination.
- Tree-shaking via subpath exports gives the same bundle cost as separate packages.
- `git subtree split` can later extract a subdirectory if one piece outgrows the rest.

`@quarto/engine-host-deno` stays separate because it's q2-specific (stdio protocol,
source-map rehydration).

### YAML cell options: simplified approach

Quarto 1's `partitionCellOptions` uses the full YAML schema system to
validate cell options. The §2aa `markdownRegex`/`breakQuartoMd` skips validation
and just parses YAML with `js-yaml`. This means:
- Cell options with typos won't be caught at parse time
- That's fine — validation happens elsewhere in q2's pipeline
- The engine extension just needs the parsed options as a plain object

### Future: Quarto 1 adoption

`@quarto/api` is designed so that Q1 could import it in place of its own
tangled implementations (`external-sources/quarto-cli/src/core/lib/mapped-text.ts`,
`external-sources/quarto-cli/src/core/pandoc/pandoc-partition.ts`, etc.). The API signatures match Q1's
existing interfaces. If/when Q1 adopts it, Q1's `QuartoAPIRegistry` keeps
its existing shape but providers delegate to `@quarto/api` submodules.

## Success Criteria

Delivered elsewhere (cross-reference, not this plan): the `@quarto/api`
namespaces, `@quarto/api/platform`/`PlatformHost`, the parity tests, and the
`No Deno.*/node:*` invariant are **§2aa**; `denoHost`, the `buildQuartoAPI`
assembly with gating, and `fromSourceMap` source-map rehydration are **Plan 1b**.

This plan:

- [x] **Phase A:** `system.pandoc` and `path.runtime`/`path.resource`/
  `path.dataDir` have real bodies (no longer "not yet implemented" stubs), reading
  `global` via the widened `makePathHost(host, global)`/`makeSystem(host, global)`
  seam (#3); `runtime`/`dataDir` create the dir via `host.fs.ensureDir` (#4, Julia
  depends on it), `resource` does not, `roaming` is a documented no-op (#6),
  `pandoc` throws "pandoc unavailable" on a `None` path (#5); the stale Model-1
  env/launch-gated comments are deleted (#1). Covered by the bound Test Seam Spec
  rows (ensureDir / pandoc-None / `global`-seam) and resolves **pre-launch**
  (ambient, RTQ Item A). (`checkRender`/`runExternalPreviewServer` stay
  `notYetImplementedError` until a caller exists.)
- [x] **Phase B (the `@quarto/types` coherence gate):** the package carries the q2-refined signatures
  (`ExecutionEngineDiscovery`/`Instance`, `ExecuteOptions`/`Result`/`Target`,
  `QuartoAPI` with namespace signatures + the pure/host-only/ambient jsdoc
  classification, `MappedString`, `EngineProjectContext`, `LanguageClaim`, and the jupyter namespace
  signatures incl. `widgetDependencyIncludes`→`PandocIncludes`). The engine packages' per-package
  `tsc --noEmit` are green with all *present* consumers (1b's `buildQuartoAPI`, an author-fixture
  source unit) — no `tsc -b` graph exists over ts-packages; the wire-dual types have a TS↔Rust
  parity check; sequencing is 1b → 2 → 3. (See "coherence gate" above.)
- [x] **B1 (return-to-Q1, relocated from RTQ):** `system.execProcess` matches Q1's six-positional
  signature `(options, stdin?, mergeOutput?, stderrFilter?, respectStreams?, timeout?)` — runtime
  brought up to the already-correct vendored type (not flattened); **all four** knobs threaded through
  `PlatformHost.ExecOptions` with real `denoHost` bodies (wholesale port of `core/process.ts`); T-B1
  two-tier seams green (fake-host wiring + deno-tier behavior).
- [x] **B2 (QuartoAPI conformance — retires Plan 1b's cast):** `checkRender`/
  `runExternalPreviewServer` typed to their real returns + throw (no `...args: unknown[]`);
  `text.postProcessRestorePreservedHtml` exported (real-typed stub); `@quarto/api`'s namespace
  interfaces derive from `QuartoAPI['<ns>']` (conformance compiler-enforced); the harness's
  `as unknown as QuartoAPI` cast + local `postProcessRestorePreservedHtml` stub **deleted**
  (`buildQuartoAPI` typed `: QuartoAPI` with no cast, except the `jupyter` Proxy pending Plan 3);
  T-B2 negative type-test green (a mis-typed namespace fails to compile).
- [x] `LanguageClaim` claim constructors (`primary`/`interop`/`fallback`)
  exported from `@quarto/api`.
- [x] `ExecutionEngineDiscovery.init` jsdoc documents the q2 init timing
  (loadEngine call site, everything-available-pre-launch from the `Init { global }`
  config — no gating, RTQ Item A — module-top-level prohibition,
  sync/defensive-await behavior, load-failure-on-throw).
- [x] A published-SDK `resources/extension-build/deno.json` template referencing
  `jsr:@quarto/api` / `jsr:@quarto/types` — **landed by plan1c Task 12** (+ `deno.workspace.json`
  for the in-repo dev mapping); reconcile/verify only.
- [x] All tests pass.

## Status (Plan 2 — 2026-07-01)

**Plan 2 complete and review-clean** (subagent-driven-development; sonnet implementers/reviewers,
opus for the B2 derive, the T-Gate-parity gate, and the final whole-branch review). All Phase A,
B1, B2, and Phase-B items landed; each task passed an individual spec+quality review, and the
final whole-branch review returned **READY TO MERGE = YES** with no Critical/Important findings.

Commit range: `08cb9f4d3..HEAD` on `feature/ts-engine-extensions` (integration line; **not** merged
to main, **not** pushed). Key commits: Phase A `f40f197dc`(+`58980b25d`), B1 `c54424a75`,
B2 `ef272cd41`, LanguageClaim `69556fdc3`, stdin return-to-Q1 `8e830d05f`, Phase-B jsdoc
`114cbc2a7`(+`6a7def0cf`), T-Gate-tsc `8f061f374`, T-Gate-parity `ba300bda1`, finish fixups `f4904e588`.

Notable resolutions / deviations:
- **execProcess stdin** was corrected to Q1's stream-**MODE** semantics (content via the positional
  param), which also let B2's `SystemNamespace` derive `= QuartoAPI["system"]` **carve-out-free**
  (stronger conformance). The one frozen §2aa test changed here was **user-ratified** (2026-07-01).
- **interop/fallback** claims are wired end-to-end: `mapLanguageClaim` now normalizes the author
  `LanguageClaim` object (defaults primary→1, interop/fallback→0 per `engine-resolution.md` §3.2).
- **The harness `as unknown as QuartoAPI` cast is gone** — `buildQuartoAPI` is typed `: QuartoAPI`
  (only the jupyter `Proxy` cast remains, authorized until Plan 3).
- `console` is documented as **host-only** (`makeConsole(host)`), a corrected classification vs the
  older subprocess-plan table; the canonical classification source is plan1b's "Engine API contract".

Out-of-Plan-2 follow-ups discovered (candidates for braid strands — NOT Plan 2 defects):
- **Wire optionality asymmetry:** TS marks several wire fields optional while the Rust `Option<T>`
  structs lack `#[serde(default)]`; serde tolerates missing on deserialize (→ `None`) and emits
  `null` (not omit) on serialize, so key-*sets* agree — the T-Gate-parity gate is key-set parity and
  does not police optionality/default handling. A dedicated check would be a separate concern.
- **denoHost large-stdin deadlock (pre-existing, not Plan 2):** `process.exec` writes+closes stdin
  before starting the stdout/stderr readers and outside the `timeout` race, so a child that fills its
  stdout pipe before draining a large stdin could block un-killed. No in-scope caller today.
