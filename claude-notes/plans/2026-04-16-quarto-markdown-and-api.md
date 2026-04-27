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
- `denoHost: PlatformHost` and the state-machine `buildQuartoAPI(...)` assembly
  **with gating** live in **`@quarto/engine-host-deno`** (Plan 1b:
  `deno-host.ts`, `quarto-api.ts`). The old "Plan 2 wires the namespaces /
  replaces Plan 1b's stubs" model is gone: §2aa ships real namespace bodies and
  Plan 1b wires + gates them. Plan 2 does **not** own the assembly.

What **remains** in Plan 2 is two things, below.

> The detailed method-by-method specs that used to live here (extract-yaml,
> partition, breakQuartoMd, MappedString `.map()`, etc.) are realized in §2aa;
> they're recoverable from git history at the §2aa implementation commits if a
> requirement needs re-checking. The reusable rationale survives under "Design
> Notes" below.

## Phase A — deferred launch-context bodies (fill §2aa's stubs)

§2aa shipped these methods as `async` "not yet implemented" stubs (they reject
rather than throw synchronously — see the §2aa final-review fixes) because they
need render-service or environment/filesystem context. Give them real bodies.
They plug into the existing §2aa namespace modules and take their IO through the
injected `PlatformHost`. **These are exactly the methods Plan 1b gates until
`launchEngine`** — Phase A is the "returns a real value after launch" side of
Plan 1b's gated-method contract test.

- [ ] `system.pandoc` — locate and invoke the pandoc binary (Q1:
  `pandocBinaryPath` + `execProcess`), routed through `PlatformHost.process.exec`.
- [ ] `system.checkRender` — the `quarto check` render probe; needs a
  render-service/launch context, so it stays behind the gate.
- [ ] `system.runExternalPreviewServer` — spawn the external preview server via
  the host's `process.exec`. No q2 caller yet; keep for Q1 parity, gated.
- [ ] `path.runtime`, `path.resource`, `path.dataDir` — resolve the quarto
  runtime/resource/data directories via `PlatformHost.env`/`realPath` (the §2aa
  `platform/index.ts` comment reserves `env.get` + `realPath` for exactly these).
  Q1: `quartoRuntimeDir` / `resourcePath` / `quartoDataDir`.
- [ ] Unit tests for each, injecting a fake `PlatformHost` (mirrors §2aa's
  namespace tests).

> The pure/host-only methods these sit beside (`path.absolute`,
> `path.dirAndStem`, `system.execProcess`, `system.tempContext`, …) are already
> real in §2aa; only the context-dependent bodies above are deferred here.

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
- [ ] **Anchor the pure/host-only/gated classification as jsdoc on `QuartoAPI`.**
  Plan 1b's "Engine API contract" table (which methods are pure, host-only, or
  gated-until-`launchEngine`) is currently the sole source of truth. Record it
  as jsdoc on the `QuartoAPI` type here so the harness's gating (Plan 1b) and
  the namespace bodies (§2aa) agree against a written contract rather than by
  convention. Plan 1b's table remains canonical; this mirrors it.
- [ ] **Claim constructors live in `@quarto/api` (runtime), not `@quarto/types`
  (erased).** Add tiny helpers `primary(priority?)`, `interop(priority?)`,
  `fallback(priority?)` that return the corresponding `LanguageClaim` objects,
  so authors write `claimsLanguage: (lang) => lang === "julia" ? primary() :
  null` instead of hand-writing tags. Export from `@quarto/api` (a small
  `@quarto/api/claims` subpath or the package root); pure data, no host
  dependency.
- [ ] For compatibility with Quarto 1 extensions: our type names should match
  Quarto 1's.
- [ ] **Document the state-machine init() timing in the
  `ExecutionEngineDiscovery.init` jsdoc.** Plan 1b introduces a
  q2-specific lifecycle deviation from Q1: the QuartoAPI is built once
  at `loadEngine` over a shared `HostState`, but its
  context-dependent methods are gated until the first `launchEngine`.
  Update the `init?` jsdoc to spell out:
  - When `init()` runs (during `loadEngine` handling, after the
    module's exports are validated).
  - What's available immediately (pure namespaces — `text`,
    `markdownRegex`, `console`, `crypto` — and host-only namespaces —
    `mappedString`, most of `path`, most of `system`).
  - What's gated until `launchEngine` (`path.runtime`,
    `path.resource`, `system.pandoc`, and `format.*` when called
    without an explicit format argument).
  - The contract that engines may NOT access `quarto.*` at module
    top-level — only from inside methods.
  - That `init()` is sync per Q1's contract, but the harness `await`s
    its return defensively, so an `async init()` works correctly.
  - That throwing/rejecting from `init()` is a fatal load failure.
  Cross-reference Plan 1b's "Engine API contract" section as the
  canonical source for the gated/available method table.
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

- [ ] **Phase A:** `system.pandoc`, `system.checkRender`,
  `system.runExternalPreviewServer`, and `path.runtime`/`path.resource`/
  `path.dataDir` have real implementations (no longer "not yet implemented"
  stubs), each routed through `PlatformHost` and covered by a fake-host unit
  test. After these land, Plan 1b's gated-method contract test sees real values
  post-`launchEngine`.
- [ ] **Phase B:** `@quarto/types` carries the q2-refined signatures
  (`ExecutionEngineDiscovery`/`Instance`, `ExecuteOptions`/`Result`/`Target`,
  `QuartoAPI` with namespace signatures + the pure/host-only/gated jsdoc
  classification, `MappedString`, `EngineProjectContext`, `LanguageClaim`).
- [ ] `LanguageClaim` claim constructors (`primary`/`interop`/`fallback`)
  exported from `@quarto/api`.
- [ ] `ExecutionEngineDiscovery.init` jsdoc documents the q2 state-machine
  timing (loadEngine call site, available-immediately vs. gated-until-launch
  namespaces, module-top-level prohibition, sync/defensive-await behavior,
  load-failure-on-throw).
- [ ] A published-SDK `resources/extension-build/deno.json` template referencing
  `jsr:@quarto/api` / `jsr:@quarto/types`.
- [ ] All tests pass.
