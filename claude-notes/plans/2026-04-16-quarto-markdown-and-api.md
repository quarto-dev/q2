# Plan 2: @quarto/api deferred launch-context bodies + @quarto/types refinements

**Grand plan:** [2026-04-16-ts-engine-extensions-subprocess.md](2026-04-16-ts-engine-extensions-subprocess.md)
**Depends on:** Plan 2A — both the **foundation** (`@quarto/api` shell + `./config` + vendored `@quarto/types`) and **§2aa** (the runtime surface: the `text`/`markdownRegex`/`mappedString`/`format`/`path`/`system`/`console`/`crypto` namespaces + `@quarto/api/platform`), both implemented. Phase B (types) is otherwise independent. Phase A fills stubs that §2aa shipped, so it follows §2aa.
**Blocks:** Plan 4 (Julia Validation) needs all of Plan 2.
**Estimated sessions:** ~1 (down from 1-2 — the namespace creation moved to §2aa and is done).

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

## Phase A — deferred launch-context bodies (fill §2aa's stubs)

§2aa shipped these methods as `async` "not yet implemented" stubs (they reject
rather than throw synchronously — see the §2aa final-review fixes) because their
*bodies* need environment/filesystem (or, for the preview probes, render-service)
context. Give them real bodies. They plug into the existing §2aa namespace
modules and take their IO through the injected `PlatformHost`. **These are not
gated** (RTQ Item A removed the launch-gating): `path.runtime`/`resource`/
`dataDir` and `system.pandoc` resolve from the `Init { global }` config the
harness injects at assembly, so they are available **pre-launch** — Phase A just
fills the stub bodies that read that config.

- [ ] `system.pandoc` — locate and invoke the pandoc binary (Q1:
  `pandocBinaryPath` + `execProcess`), routed through `PlatformHost.process.exec`
  (the pandoc path comes from the `Init { global }`).
- [ ] `system.checkRender` — the `quarto check` render probe; no q2 caller yet,
  so it stays a `notYetImplementedError` stub.
- [ ] `system.runExternalPreviewServer` — spawn the external preview server via
  the host's `process.exec`. No q2 caller yet; stays a `notYetImplementedError`
  stub (kept for Q1 parity).
- [ ] `path.runtime`, `path.resource`, `path.dataDir` — resolve from the
  `Init { global }` config (`runtimeDir`/`resourceDir`/`dataDir`) the harness
  injects at assembly, so they resolve **immediately**, pre-launch (ambient, like
  Q1's `quartoRuntimeDir`/`resourcePath`/`quartoDataDir`). Not gated.
- [ ] Unit tests for each, injecting a fake `PlatformHost` / `Init` config
  (mirrors §2aa's namespace tests).

> The pure/host-only methods these sit beside (`path.absolute`,
> `path.dirAndStem`, `system.execProcess`, `system.tempContext`, …) are already
> real in §2aa; only the context-dependent bodies above are deferred here.
> (Per **B1** below, `system.execProcess` carries Q1's `mergeOutput`/
> `stderrFilter` — knitr uses both, `rmd.ts:440` — via the flatten-into-
> `ExecProcessOptions` fix; the runtime and vendored `@quarto/types` signatures
> are reconciled. B1 was relocated here from RTQ by the Option A split.)

## Phase B — @quarto/types and import map (was Phase 2E)

Following Quarto 1's model, engine extensions import types via
`import type { ... } from "@quarto/types"`. These are erased during the
build step (bundling), so no runtime code is needed — just a `.d.ts` file
referenced by the import map.

- [ ] Refine the q2-specific type surface in `@quarto/types` — the package is
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
- [ ] **Anchor the pure/host-only/ambient classification as jsdoc on `QuartoAPI`.**
  Plan 1b's "Engine API contract" table (which methods are pure, host-only, or
  ambient — resolving from the `Init { global }` config) is the source of truth.
  Record it as jsdoc on the `QuartoAPI` type here so the harness assembly (Plan 1b)
  and the namespace bodies (§2aa) agree against a written contract rather than by
  convention. Plan 1b's table remains canonical; this mirrors it. (There is **no
  gated class** — RTQ Item A.)
- [ ] **Claim constructors live in `@quarto/api` (runtime), not `@quarto/types`
  (erased).** Add tiny helpers `primary(priority?)`, `interop(priority?)`,
  `fallback(priority?)` that return the corresponding `LanguageClaim` objects,
  so authors write `claimsLanguage: (lang) => lang === "julia" ? primary() :
  null` instead of hand-writing tags. Export from `@quarto/api` (a small
  `@quarto/api/claims` subpath or the package root); pure data, no host
  dependency.
- [ ] For compatibility with Quarto 1 extensions: our type names should match
  Quarto 1's.
- [ ] **Document the init() timing in the `ExecutionEngineDiscovery.init`
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
- [ ] Create a template `resources/extension-build/deno.json` that engine
  authors copy/extend. Its imports reference the **published** SDK and std lib
  (no q2-local import map for the SDK):
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
`@quarto/types/quarto-api.ts:606-613` keeps the full 6-param signature → **runtime and vendored
signatures disagree**, so the `QuartoAPI` aggregation can't typecheck until reconciled. Engine
survey: only knitr (`rmd.ts:440`, the 2 knobs) and `jupyter-kernel.ts:181` (2-arg, unaffected) call
`execProcess`; julia/marimo use raw `Deno.Command`. **`respectStreams`/`timeout` are used by no
engine → safe to leave dropped;** `mergeOutput`/`stderrFilter` are the real gap.

**Decision (2026-06-26): restore-now — in scope.** No *in-scope* TS engine uses these today
(knitr/jupyter are native Rust in q2; julia/marimo bypass the seam), but the framework must carry
them: future TS engines + the grand plan's "consumable by Q1 itself" portability (Q1's own knitr
breaks on the reduced signature), and it unblocks the `QuartoAPI` aggregation typecheck. Fix is the
plan-compliant *flatten into the options object* (not new positional params):

- [ ] **Runtime (`@quarto/api`).** Add `mergeOutput?: "stderr>stdout"|"stdout>stderr"` and
  `stderrFilter?: (output: string) => string` to `ExecProcessOptions` (`system/index.ts:43-58`);
  thread them through `PlatformHost.ExecOptions` (`platform/index.ts:25-32`) → `host.process.exec`
  (their home below the seam, where they're applied). `respectStreams`/`timeout`: leave dropped (no
  engine uses them) or add for completeness.
- [ ] **Types (Phase B reconciliation).** Align the vendored `@quarto/types`
  `QuartoAPI.system.execProcess` (+ `ProcessResult`/`ExecProcessOptions`) so **runtime == vendored**.
  (Folds into Phase B's `@quarto/types` reconciliation — this is one of the signatures it syncs.)
- [ ] **Test seam T-B1 (TS/vitest, frozen).** Mount `makeSystem(fakeHost)` with a fake
  `PlatformHost` that records the `ExecOptions` it receives; call `execProcess({cmd, mergeOutput:
  "stdout>stderr", stderrFilter: f}, stdin)`; assert the recorded `ExecOptions` carries `mergeOutput`
  **and** the `stderrFilter` ref. *Named revert:* drop the two fields from the `ExecProcessOptions →
  ExecOptions` threading → the fake never sees them → RED. *(Mount the real `makeSystem` unit; mock
  only the genuine environment dep — the host.)*

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

- [ ] **Phase A:** `system.pandoc` and `path.runtime`/`path.resource`/
  `path.dataDir` have real bodies (no longer "not yet implemented" stubs),
  reading the injected `Init { global }` config / routed through `PlatformHost`
  and covered by fake-host unit tests — and resolve **pre-launch** (ambient,
  RTQ Item A). (`checkRender`/`runExternalPreviewServer` stay
  `notYetImplementedError` until a caller exists.)
- [ ] **Phase B:** `@quarto/types` carries the q2-refined signatures
  (`ExecutionEngineDiscovery`/`Instance`, `ExecuteOptions`/`Result`/`Target`,
  `QuartoAPI` with namespace signatures + the pure/host-only/ambient jsdoc
  classification, `MappedString`, `EngineProjectContext`, `LanguageClaim`).
- [ ] **B1 (return-to-Q1, relocated from RTQ):** `system.execProcess` carries
  `mergeOutput`/`stderrFilter` (flattened into `ExecProcessOptions`, threaded through
  `PlatformHost.ExecOptions`); runtime and vendored `@quarto/types` signatures reconciled; T-B1
  fake-host vitest seam green.
- [ ] `LanguageClaim` claim constructors (`primary`/`interop`/`fallback`)
  exported from `@quarto/api`.
- [ ] `ExecutionEngineDiscovery.init` jsdoc documents the q2 init timing
  (loadEngine call site, everything-available-pre-launch from the `Init { global }`
  config — no gating, RTQ Item A — module-top-level prohibition,
  sync/defensive-await behavior, load-failure-on-throw).
- [ ] A published-SDK `resources/extension-build/deno.json` template referencing
  `jsr:@quarto/api` / `jsr:@quarto/types`.
- [ ] All tests pass.
