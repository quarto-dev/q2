# Plan 2A: TS package foundations (@quarto/api skeleton + config, @quarto/types vendor)

**Grand plan:** [2026-04-16-ts-engine-extensions-subprocess.md](2026-04-16-ts-engine-extensions-subprocess.md)
**Depends on:** the npm workspace (no epic dependency — independent root, peer of plan1a-protocol)
**Blocks:** Plan 1b (imports `@quarto/api/config`; depends on `@quarto/types` to typecheck/bundle; its contract tests need the §2aa runtime surface below), Plan 2 (rest of `@quarto/api`; Plan 2E refines `@quarto/types`), Plan 3 (`@quarto/api/jupyter` needs the skeleton)
**Estimated sessions:** ~1 for the foundation (done) + ~1 for §2aa (the runtime surface, not yet built)
**Status:** the **foundation** (config + `@quarto/types` + package shell) is implemented on `feature/ts-engine-extensions`. The **§2aa** runtime surface below — the `platform` seam + pure/host-only namespaces — is **not yet built**.

## Overview

This plan creates the two TypeScript package **foundations** the rest of the
epic builds on:

1. The `@quarto/api` package shell at `ts-packages/quarto-api/` plus its
   `config` subpath holding Quarto 1's metadata-partition key lists.
2. The `@quarto/types` package at `ts-packages/quarto-types/`, **vendored** from
   Quarto 1, so Plan 1b can `import type { … } from "@quarto/types"` and
   typecheck/bundle against it. (Vendored in-repo; also published to
   `jsr:`/`npm` for external engine authors per the grand plan's
   "Distribution of the engine-author SDK".)

The **foundation** here is deliberately minimal: package scaffolding + the
five key-list constants + the vendored `@quarto/types`. This part is done.
The runtime `@quarto/api` surface that Plan 1b's contract tests exercise — the
`platform` seam (the q2-original `PlatformHost`) and the pure/host-only
namespaces (`text`, `markdownRegex`, `mappedString`, `format`, `path`,
`system`, `console`, `crypto`) — is **§2aa** (the "Section 2aa" below in this
same plan), and is not yet built. `jupyter` and launch-context method bodies
are Plan 3 / Plan 2; the q2-specific `@quarto/types` refinements are Plan 2E.
The foundation lands first because Plan 1b needs *both* `@quarto/api/config`
and `@quarto/types` present before it can typecheck.

Plan 2A is an **independent root** of the epic — it depends on nothing but the
npm workspace (already present) and is a peer of plan1a-protocol, with no edge
between them. Extracting it out of Plan 2 lets Plan 1b import the real
metadata-partition key lists from `@quarto/api/config` instead of inlining
constants, so 1b is unblocked without waiting for the rest of `@quarto/api`.

The metadata-partition key lists — `kExecuteDefaultsKeys`, `kRenderDefaultsKeys`,
`kPandocDefaultsKeys`, `kIdentifierDefaultsKeys`, and `kLanguageDefaultsKeys` —
live in **`@quarto/api/config`** (`ts-packages/quarto-api/src/config/`). They are
a **careful extraction** of the same five lists in Quarto 1's
`external-sources/quarto-cli/src/config/constants.ts` — same key *names*, same
grouping — and are re-synced whenever Q1 drifts. The extraction is not a literal
`cp`: Q1 defines each list as an array of *symbol references*
(`kExecuteDefaultsKeys = [kFigWidth, kFigHeight, …]`), so it resolves those
symbols to their string values. Q1's `constants.ts` is the **parity reference** —
read-only, never imported. `@quarto/api/config` is the **runtime home** the
engine-host harness imports to partition q2's single merged metadata map
(`doc.ast.meta` after `MetadataMergeStage`) into Q1's nested `Format` shape
(`format.execute` / `format.render` / `format.pandoc` / `format.identifier` /
`format.language`), with anything matching no list falling through to the
`format.metadata` catch-all. Keeping the lists on the side that speaks Q1's
vocabulary means a Q1 re-sync is a transcription of one file, with no Rust-side
translation table. **Resolved (match Q1):** Q1's `metadataAsFormat`
(`config/metadata.ts:200-210`) never consults `kLanguageDefaultsKeys` for
flat-key classification — it has no flat-key language branch, and
`format.language` is filled only by the nested `language:` peel. So the
partition is a **four-list** flat classification (identifier/render/execute/
pandoc, else `format.metadata`); see Plan 1b's "Resolved partition decisions".
`@quarto/api/config` still **transcribes all five** `kXxxDefaultsKeys` (so the
parity test against `constants.ts` stays whole and the list is available to a
future consumer), but `kLanguageDefaultsKeys` is **not consulted by the
partition** — annotate it `// not used by metadataAsFormat partition; present
for parity` in `src/config/`.

Note also that `kPandocDefaultsKeys` is **not purely symbol references** in Q1:
it mixes imported symbols (`kFilters`, …) with ~30 inline string literals
(`"defaults"`, `"metadata"`, `"file-scope"`, `"trace"`, …). The transcription
must capture the inline literals too — following only symbol imports would miss
them.

## Work Items

- [x] Create `ts-packages/quarto-api/package.json` (`name: @quarto/api`,
  `type: module`, a `version`). Follow the existing ts-packages shape exactly
  (`ts-packages/pandoc-types/`, `ts-packages/quarto-automerge-schema/`):
  `main: "dist/index.js"`, `types: "src/index.ts"`, `files: ["dist"]`, and
  `scripts` for `build` (`tsc`), `clean` (`rm -rf dist`), and `test`
  (`vitest run`). `devDependencies`: `typescript` + `vitest` (match the versions
  pinned by `quarto-automerge-schema`). Each `exports` entry uses the
  three-condition form the other ts-packages use —
  `{ "types": "./src/<sub>/index.ts", "source": "./src/<sub>/index.ts",
  "import": "./dist/<sub>/index.js" }`. The map starts with **only** `"."` (the
  aggregate) and `"./config"`. **§2aa (below) adds `./platform` and the
  pure/host-only namespace subpaths** (`./text`, `./markdownRegex`,
  `./mappedString`, `./format`, `./path`, `./system`, `./console`, `./crypto` —
  Q1's exact names; note it is `markdownRegex` not `markdown`, and `mappedString`
  is its own top-level namespace), and **Plan 3 adds `./jupyter`**, each as they
  create the module so the package builds clean before those land.
  `dependencies: { "yaml": "^2.0.0" }` (the
  package's single dep list; `yaml`'s first consumer is Plan 2's `markdown/`, but
  it is declared here so the dep list is set once). Run `npm install` from the
  repo root. `@quarto/api` is published to jsr/npm (see the grand plan's
  "Distribution of the engine-author SDK"); the registry identity is set here,
  the first publish happens once the surface is ready in Plan 2.
- [x] Create `ts-packages/quarto-api/tsconfig.json` matching repo ts-packages
  conventions (copy `ts-packages/pandoc-types/tsconfig.json`: `target ES2022`,
  `module node16`, `outDir ./dist`, `rootDir ./src`, declarations + maps on).
- [x] Create `src/index.ts` re-exporting `./config` for now (Plan 2 / Plan 3
  extend it as they add subpaths).
- [x] Create `src/config/` holding the five metadata-partition key lists
  (`kExecuteDefaultsKeys`, `kRenderDefaultsKeys`, `kPandocDefaultsKeys`,
  `kIdentifierDefaultsKeys`, `kLanguageDefaultsKeys`). Carefully transcribe them
  from `external-sources/quarto-cli/src/config/constants.ts` — same key names,
  same grouping, resolving Q1's symbol-reference arrays to their string values.
  Add a `// parity: keep in sync with
  external-sources/quarto-cli/src/config/constants.ts` header comment.
- [x] Parity test for the key lists: diff `@quarto/api/config` against
  `external-sources/quarto-cli/src/config/constants.ts` (the five
  `kXxxDefaultsKeys`) and fail on any difference, so a Q1 drift is caught at
  test time instead of silently misrouting a new config key into the wrong
  `Format` bucket. When `external-sources` isn't checked out (e.g. CI), fall
  back to asserting the lists are non-empty and contain known anchor keys.

### @quarto/types (vendored)

- [x] Create `ts-packages/quarto-types/` by **vendoring** Quarto 1's
  `external-sources/quarto-cli/packages/quarto-types/` — copy its type sources
  into `src/`. Type-only package: follow the `ts-packages/pandoc-types/` shape
  (`type: module`, `tsc` build, three-condition `exports`, `files: ["dist"]`),
  no runtime code. Add a `// parity: vendored from
  external-sources/quarto-cli/packages/quarto-types` header comment. This lives
  at the foundation so Plan 1b can typecheck and bundle against `@quarto/types`;
  Plan 2E refines the q2-specific surface (QuartoAPI signatures, `LanguageClaim`,
  `init` jsdoc, import map) on top of this baseline.
  - Distribution is **decided** (no longer open): `@quarto/types` and
    `@quarto/api` are published to `jsr:`/`npm` for external engine authors
    (grand plan, "Distribution of the engine-author SDK"). Within the q2 repo
    they remain a checked-in copy re-synced from Q1 the same way `config` is —
    the repo bundles from source while external authors consume from the
    registry; the two coexist.
- [x] Run `npm install` from the repo root so the new `@quarto/types` workspace
  package resolves.

---

## Section 2aa — `@quarto/api` runtime surface (platform + pure/host-only namespaces)

**Status: not yet built.** Everything above (the `@quarto/api` shell + `config`,
and the vendored `@quarto/types`) is the **complete foundation**. This section
is the runtime `@quarto/api` surface Plan 1b's contract tests require — carved
out as a distinct section so the foundation's checklist can be marked done
while this stays open work. It is **not** a separate plan; it is the remaining
scope of Plan 2A.

Why it's a 1b prerequisite: Plan 1b's contract tests call concrete namespace
methods (`quarto.text.lines`, `quarto.markdownRegex.*`, `quarto.console.error`,
and the gated `path.runtime`/`path.resource`/`system.pandoc`/
`format.isHtmlCompatible`) and need them to return real values (or throw the
*gating* error), not "not yet implemented". `jupyter` (kernel discovery, Python
subprocess, project context) and launch-context method *bodies* stay in
**Plan 3 / Plan 2**; the `@quarto/types` QuartoAPI/jsdoc refinements stay in
**Plan 2E**.

### `@quarto/api/platform` — the PlatformHost seam (q2-original)

- [x] Create `src/platform/` exporting the **`PlatformHost`** interface and
  re-export it from `@quarto/api/platform`. This is a **new q2 abstraction,
  not a Q1 port** — Q1 has no host-injection seam (it calls `Deno.*` directly
  inside each namespace's backing functions). **Authoritative shape: the landed
  `ts-packages/quarto-api/src/platform/index.ts`** — read it rather than this
  sketch. As built it is richer than the original sketch below: `fs` also has
  `makeTempFile`/`makeTempDir` (with an optional `dir`), `process.exec` uses
  structured `ExecOptions`/`ExecResult`, and there is an `env` accessor (reserved
  for the deferred `path.runtime`/`dataDir` bodies). Illustrative sketch:
  ```typescript
  export interface PlatformHost {
    fs: {
      readTextFileSync(path: string): string;
      writeFileSync(path: string, content: string | Uint8Array): void;
      exists(path: string): boolean;
      // + makeTempFile / makeTempDir (see landed source)
    };
    process: { exec(cmd: string, args: string[], opts?: ExecOptions): Promise<ExecResult> };
    realPath(path: string): string;
    env: { get(name: string): string | undefined };
    isInteractive: boolean;
    isCI: boolean;
  }
  ```
  `@quarto/api`'s host-only namespaces take a `PlatformHost` (constructor-
  injected or passed per call) rather than importing `Deno.*`, so the package
  stays platform-neutral and a `@quarto/engine-host-wasm` can supply a
  VFS-backed host later. No `Deno.*` / `node:*` in `@quarto/api` itself.

### `@quarto/api` pure + host-only namespaces (the 1b-prerequisite surface)

Port these from Q1 (`external-sources/quarto-cli/src/core/api/*` and the
backing `core/lib/*` modules — parity reference, never imported). Each gets a
subpath and a `// parity:` header. **Pure** namespaces are straight ports (no
IO); **host-only** namespaces take their IO through `PlatformHost`.

- [x] `src/text/` (**pure**) — `lines`, `trimEmptyLines`, `lineColToIndex`,
  `executeInlineCodeHandler`, `asYamlText`, `postProcessRestorePreservedHtml`
  (from `core/lib/text.ts`).
- [x] `src/markdownRegex/` (**pure**) — `extractYaml`, `partition`,
  `getLanguages`, `getLanguagesWithClasses`, `breakQuartoMd` (from
  `core/lib/break-quarto-md.ts` etc.).
- [x] `src/mappedString/` (**mostly pure**) — `fromString`,
  `normalizeNewlines`, `splitLines`, `indexToLineCol` (pure, from
  `core/lib/mapped-text.ts`); **`fromFile`** reads disk via
  `PlatformHost.fs.readTextFileSync` (the one host-only method). Note: this is
  the same `MappedString` type Plan 1b's `mapped-source.ts` rehydrates into —
  one type, not two.
- [x] `src/format/` (**pure**) — `isHtmlCompatible`, `isIpynbOutput`,
  `isLatexOutput`, `isMarkdownOutput`, `isPresentationOutput`,
  `isHtmlDashboardOutput`, `isServerShiny`, `isServerShinyPython` (predicates
  over a `Format`).
- [x] `src/crypto/` (**pure**) — `md5Hash` (from `core/hash.ts`).
- [x] `src/console/` (**host-only**) — `info`, `warning`, `error`,
  `withSpinner`, `completeMessage`; writes to stderr via the host's logger.
  (Plan 1b's idempotency test depends on `console.error` actually reaching
  stderr.)
- [x] `src/path/` (**host-only / env-dependent**) — `absolute`,
  `toForwardSlashes`, `dirAndStem`, `isQmdFile`, `inputFilesDir` (pure string
  ops) plus `runtime`, `resource`, `dataDir` (read env/filesystem via
  `PlatformHost`). The env/fs ones are the methods Plan 1b *gates* until
  `launchEngine`.
- [x] `src/system/` (**host-only + context-dependent**) — `execProcess`,
  `isInteractiveSession`, `runningInCI`, `tempContext`, `onCleanup` are real
  via `PlatformHost`. `pandoc`, `checkRender`, and `runExternalPreviewServer`
  ship as stubs (the landed impl throws `requiresLaunchContextError`/
  `notYetImplementedError`); their **real bodies are deferred to Plan 2**
  (Phase A). Note: this means **no §2aa gated method returns a real value at
  1b time** — `path.runtime`/`resource`/`dataDir` and `system.pandoc` are all
  Plan-2 bodies. The only gated method with a real body in §2aa is
  `format.isHtmlCompatible` (pure; the gate is only on the
  default-format-from-context path). See the §2aa→1b sequencing note in Plan 1b's
  gated-method tests.
- [x] Unit tests per namespace (pure ones: direct input/output; host-only
  ones: inject a fake `PlatformHost`). These back Plan 1b's contract tests —
  if a namespace 1b calls isn't real here, 1b's tests can't pass.

> `jupyter` (kernel discovery, Python subprocess, project context) and the
> launch-context-dependent method bodies are **not** in §2aa — Plan 3 / Plan 2.

## Success Criteria

### Foundation (done)

- [x] `@quarto/api` package exists with `package.json` (incl. `./config`
  export) and `tsconfig`.
- [x] `@quarto/api/config` exports the five key lists, transcribed from Q1
  (key strings resolved from Q1's symbol-reference arrays).
- [x] No `Deno.*` / `node:*` in `@quarto/api/config` (pure data).
- [x] `@quarto/types` package exists at `ts-packages/quarto-types/`, vendored
  from Q1, resolvable as a workspace dependency; a trivial
  `import type { … } from "@quarto/types"` typechecks.

### §2aa — runtime surface (not yet built)

- [x] No `Deno.*` / `node:*` anywhere in the `@quarto/api` namespaces
  (host-only namespaces take IO through `PlatformHost`, not direct `Deno.*`).
- [x] `@quarto/api/platform` exports the q2-original `PlatformHost` interface.
- [x] `@quarto/api` ships real `text`, `markdownRegex`, `mappedString`,
  `format`, `path`, `system`, `console`, `crypto` namespaces (Q1's names),
  each with unit tests. The pure + non-context host-only methods Plan 1b's
  contract tests call (`text.lines`, `markdownRegex.*`, `console.error`,
  `format.isHtmlCompatible`) return real values. The **context-dependent**
  gated methods (`path.runtime`/`resource`/`dataDir`, `system.pandoc`) ship as
  stubs here — real bodies are **Plan 2 (Phase A)**. `jupyter` is the only
  namespace fully absent (Plan 3).

## Note

What's still downstream of this plan: **`jupyter`** (kernel discovery, Python
subprocess, project context) is **Plan 3**; the QuartoAPI *aggregation*
(assembling the nine namespaces into one object) and the launch-context method
bodies are **Plan 2**; the q2-specific `@quarto/types` refinements (QuartoAPI
signatures carrying the pure/host-only/gated jsdoc classification,
`LanguageClaim`, `init` jsdoc, import map) are **Plan 2E**. The **foundation**
(shell + `config` + vendored `@quarto/types`) is done; **§2aa** adds the
`platform` seam + pure/host-only namespaces — enough that Plan 1b is fully
unblocked (real partition constants, a real `PlatformHost` to import, and real
namespace bodies for its contract tests, without inlining constants,
hand-stubbing types, or depending on throwing stubs).

**Cross-reference:** Plan 1b's "Resolved partition decisions" recorded two
Q1-parity calls (no flat-key `format.language` branch; move-not-duplicate).
Both are now settled to match Q1; `@quarto/api/config` transcribes all five
key lists but the partition consults only four (`kLanguageDefaultsKeys` is
present for parity, unused by the partition).

## §2aa resolved decisions (build session)

Scout port-map: `.superpowers/sdd/2aa-portmap.md`. Decisions taken before building:

1. **`PlatformHost` stays a generic seam** — `fs` (readTextFileSync, writeFileSync, exists,
   ensureDir, makeTempDir, makeTempFile), `process` (exec, onExit, exit), `env.get`, `log`
   (info/warning/error, optional clearLine), `cwd()`, `realPath()`, `isInteractive`, `isCI`.
   **No** quarto-specific `quartoSharePath`/`pandocBinaryPath` on it.
2. **Context-dependent methods are throwing stubs in §2aa** (real bodies → 1b / Plan 2):
   `path.resource`, `path.dataDir`, `path.runtime`, and `system.pandoc`. They throw a clear,
   specific "requires launch context" error (NOT a vague "not implemented") so 1b's gated-method
   test can rely on / replace it. This intentionally relaxes the §2aa success criterion that those
   exact methods "return real values" — user decision. `path` pure ops (toForwardSlashes, dirAndStem,
   isQmdFile, inputFilesDir) and `path.absolute` (via host.cwd) ARE real. `system` execProcess /
   isInteractiveSession / runningInCI / tempContext / onCleanup ARE real (via PlatformHost);
   checkRender / runExternalPreviewServer stay stubbed per the plan.
3. **`text.postProcessRestorePreservedHtml` is DEFERRED** — it does file IO (plan mis-labels it
   "pure") and no 1b contract test calls it. Port the other 5 text functions (genuinely pure) only.
4. **Injection pattern**: factory `make<Ns>(host)` with `Pick<PlatformHost, …>` scoping per namespace.
5. Deps: `crypto.md5Hash` via `blueimp-md5`; `console.withSpinner` uses a neutral (non-cliffy)
   implementation routing through `PlatformHost.log`.
