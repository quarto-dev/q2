# Plan 2: @quarto/api (text, markdown, utilities) + QuartoAPI assembly

**Grand plan:** [2026-04-16-ts-engine-extensions-subprocess.md](2026-04-16-ts-engine-extensions-subprocess.md)
**Depends on:** Phases 2A, 2B, 2D are independent. Phase 2C (wiring into engine-host) requires Plan 1b to have created the `@quarto/engine-host-deno` package.
**Blocks:** Plan 3 (@quarto/api/jupyter) is gated on Phase 2A (package skeleton). Plan 4 (Julia Validation) needs all of Plan 2.
**Estimated sessions:** 1-2

## Overview

Create the `@quarto/api` TypeScript package and populate the `text/`,
`markdown/`, `format/`, `path/`, `system/`, `console/`, and `crypto/`
subpaths. Flesh out the QuartoAPI assembly in `@quarto/engine-host-deno` that
Plan 1a stubbed.

This plan covers everything in the QuartoAPI surface except `quarto.jupyter`
(which lives under `@quarto/api/jupyter` and is the subject of Plan 3).

## Package layout

A single `@quarto/api` package with subpath exports. Everything lives under
`ts-packages/quarto-api/src/`:

```
text/                ← MappedString + text utilities
markdown/            ← extractYaml, partition, getLanguages, breakQuartoMd
format/              ← isHtmlCompatible, isLatexOutput, …
path/                ← dirAndStem, isQmdFile, toForwardSlashes, …
system/              ← execProcess, pandoc, tempContext, …
console/             ← info, warning, error, withSpinner
crypto/              ← md5Hash
jupyter/             ← Plan 3

platform.ts          ← PlatformHost interface (see below)
```

One `package.json`, one version, one dep list, `exports` map for targeted imports.

## Cross-environment portability: `PlatformHost`

`@quarto/api` must run in two environments without modification:

1. **Deno subprocess** (`@quarto/engine-host-deno`) — the harness built in
   Plan 1a. Has full Deno APIs.
2. **q2's WASM runtime in hub-client** (future, not in this plan) — file I/O
   goes through q2's VFS (`vfsReadFile`, `vfsAddFile`, …), no subprocesses,
   no `Deno` global.

To keep both targets viable, every I/O-touching submodule takes a
`PlatformHost` parameter instead of calling `Deno.*` directly. Pure submodules
(`markdown/`, most of `text/`, `format/`, `console/`, `crypto/`, most of
`jupyter/`) have no host dependency and work in any JS environment.

### `src/platform.ts`

```typescript
export interface PlatformHost {
    fs: {
        readTextFileSync(path: string): string;
        writeFileSync(path: string, content: string | Uint8Array): void;
        exists(path: string): boolean;
    };
    process?: {            // undefined → execProcess throws "not supported"
        exec(cmd: string, args: string[], opts?: ExecOptions): Promise<ExecResult>;
    };
    realPath?(path: string): string;   // undefined → absolute() returns path as-is
    isInteractive: boolean;
    isCI: boolean;
}
```

### Submodules that take a host (factories)

| Submodule | Export shape |
|---|---|
| `text/mapped-from-file.ts` | `createMappedStringFromFile(host)` returning `(path) => MappedString` |
| `path/index.ts` | `createPath(host)` for `absolute()`; pure path-string helpers remain direct exports |
| `system/index.ts` | `createSystem(host)` returning the full system namespace |

### Submodules with no host (pure exports)

`markdown/`, `text/mapped.ts` + `text/text.ts` + `text/ranged.ts` + `text/binary-search.ts`,
`format/`, `console/`, `crypto/`, `jupyter/` (the conversion logic; figure
writes from `jupyter/assets.ts` go through the host).

### What this plan implements

- [ ] `src/platform.ts` — the `PlatformHost` interface, no implementations.
- [ ] All factory exports listed above.
- [ ] A `denoHost: PlatformHost` in `@quarto/engine-host-deno`, wiring
  `Deno.readTextFileSync`, `Deno.Command`, etc.

### What this plan does NOT implement

- A WASM-side host. That's a future piece of work and needs its own package
  (working name: `@quarto/engine-host-wasm`). Design invariant for this plan:
  nothing in `@quarto/api` prevents that from being written later.
- A decision about *how* TS engine extensions run in the browser (Web
  Worker? Sandbox? Different mechanism entirely?). See
  `crates/quarto-system-runtime` and the deepwiki analysis
  (2026-04-22 discussion) for context. The `PlatformHost` abstraction is
  necessary but not sufficient for browser hosting.

## Work Items

### Phase 2A: Package skeleton + @quarto/api/markdown

- [ ] Create `ts-packages/quarto-api/package.json`:
  ```json
  {
    "name": "@quarto/api",
    "version": "0.1.0",
    "type": "module",
    "exports": {
      ".": "./src/index.ts",
      "./text":     "./src/text/index.ts",
      "./markdown": "./src/markdown/index.ts",
      "./jupyter":  "./src/jupyter/index.ts",
      "./format":   "./src/format/index.ts",
      "./path":     "./src/path/index.ts",
      "./system":   "./src/system/index.ts",
      "./console":  "./src/console/index.ts",
      "./crypto":   "./src/crypto/index.ts"
    },
    "dependencies": { "yaml": "^2.0.0" }
  }
  ```
  Run `npm install` from the repo root after creating the package.

- [ ] Create `ts-packages/quarto-api/tsconfig.json` matching the repo's existing
  ts-packages conventions.

- [ ] Create `src/index.ts` (optional aggregate re-export — convenience for
  callers who want everything under one import).

- [ ] Create `src/markdown/` — clean reimplementations of the markdown
  utilities. These power `quarto.markdownRegex.*` on the API surface.

  - [ ] `src/markdown/extract-yaml.ts` — `extractYaml(markdown: string) → Metadata`:
    - Find YAML front matter between `---` delimiters
    - Parse with `yaml` package
    - Support the `!expr` YAML tag (Quarto convention: treat as raw string)
    - Handle edge cases: no front matter, empty front matter, malformed YAML
    - Reference: Quarto 1's `readYamlFromMarkdown` in `src/core/yaml.ts`
    - ~50 lines + tests

  - [ ] `src/markdown/pandoc-attr.ts` — `pandocAttrParseText(text: string) → PandocAttr | null`:
    - Parse Pandoc-style attributes on code blocks (`{#id .class key=value}`)
    - Used by `partition.ts`
    - Reference: Quarto 1's `pandocAttrParseText` in `src/core/pandoc/pandoc-partition.ts`

  - [ ] `src/markdown/partition.ts` — `partition(markdown: string) → PartitionedMarkdown`:
    - Split markdown into: yaml front matter, heading (first heading if present), body
    - Uses `extractYaml` for the YAML part
    - Reference: Quarto 1's `partitionMarkdown` in `src/core/pandoc/pandoc-partition.ts`
    - ~200 lines + tests

  - [ ] `src/markdown/languages.ts` — `getLanguages(markdown: string) → Set<string>`:
    - Extract language specifiers from fenced code blocks via regex
    - Match `` ```{language} `` patterns
    - Pure regex, zero dependencies
    - Reference: Quarto 1's `languagesInMarkdown` — literally a copy, it's self-contained
    - ~30 lines + tests

  - [ ] `src/markdown/break-quarto-md.ts` — `breakQuartoMd(markdown: string) → QuartoMdCell[]`:
    - Split markdown into alternating code cells and markdown cells
    - Parse cell options from YAML comments within code blocks
    - **Simplified from Quarto 1**: use `yaml` package directly for cell
      options, no schema validation, no tree-sitter
    - Handle: fenced code blocks, shortcodes, raw blocks
    - Reference: Quarto 1's `breakQuartoMd` in `src/core/lib/break-quarto-md.ts`
    - Note: the Julia engine does NOT use this method, but the engine
      template does and other engines will
    - ~300 lines + tests

  - [ ] `src/markdown/index.ts` — barrel re-export.

- [ ] Write unit tests for each function. Check existing ts-packages for the
  test runner convention (likely Vitest, since that's what the Rust monorepo's
  other ts-packages use). If nothing is set up yet, use Vitest and add a
  `test` script to `package.json`.

### Phase 2B: @quarto/api/text (including MappedString)

In q2's design, `quarto.text` and `quarto.mappedString` are **two separate
QuartoAPI namespaces** (Q1 compat) powered by a **single underlying module**,
`@quarto/api/text`. Layout mirrors Q1's groupings in `@quarto/types/src/text.ts`.

- [ ] `src/text/types.ts` — types only (matches Q1's
  `@quarto/types/src/text.ts`):
  ```typescript
  export interface Range { start: number; end: number; }
  export interface MappedString {
      readonly value: string;
      readonly fileName?: string;
      readonly map: (index: number, closest?: boolean) => StringMapResult;
  }
  export type StringMapResult = {
      index: number;
      originalString: MappedString;
  } | undefined;
  export type EitherString = string | MappedString;
  export interface RangedSubstring { substring: string; range: Range; }
  export type StringChunk = string | Range | MappedString;
  ```

- [ ] `src/text/binary-search.ts` — `glb(arr, value)` helper (copy from Q1's
  `src/core/lib/binary-search.ts`, trivially small).

- [ ] `src/text/ranged.ts` — `RangedSubstring`, `rangedLines` (internal,
  used by `mapped.ts`). Copy from Q1's `src/core/lib/ranged-text.ts`.

- [ ] `src/text/text.ts` — plain-string utilities that power `quarto.text.*`:
  - `lines(text)` → `text.split("\n")`
  - `trimEmptyLines(lines, trim)` → filter empty lines from start/end
  - `lineBreakPositions(text)`, `indexToLineCol(text)`, `matchAll` (internal
    helpers used by `mapped.ts`, also exposed)

- [ ] `src/text/yaml-text.ts` — `asYamlText(metadata)` → `yaml.dump(metadata)`.

- [ ] `src/text/html-preserve.ts` — `postProcessRestorePreservedHtml(options)`
  — replace preservation markers with original HTML.

- [ ] `src/text/mapped.ts` — core `MappedString` implementation. Direct
  port of Q1's `src/core/lib/mapped-text.ts` (~200 lines):
  - `asMappedString(str, fileName?)` — base with identity `.map()`
  - `mappedSubstring(source, start, end)` — shifted view that delegates to source
  - `mappedConcat(strings)` — concatenation with binary-search `.map()`
  - `mappedString(source, pieces, fileName?)` — sugar over the above
  - `mappedLines(ms, keepNewLines?)`, `mappedNormalizeNewlines(ms)`,
    `mappedIndexToLineCol(ms)`

- [ ] `src/text/mapped-from-file.ts` — factory for FS-backed MappedString:
  ```typescript
  import type { PlatformHost } from "../platform.ts";
  export function createMappedStringFromFile(host: PlatformHost) {
      return (path: string): MappedString =>
          asMappedString(host.fs.readTextFileSync(path), path);
  }
  ```
  The only FS-touching function in `text/` — isolating it behind the host
  factory keeps the rest of `text/` (MappedString algebra, text utilities)
  pure and portable.

- [ ] `src/text/index.ts` — barrel re-export.

- [ ] Write tests for the text utilities and the MappedString algebra
  (including `.map()` composition through multiple `mappedConcat`/
  `mappedSubstring` layers).

**Note on source-map rehydration:** The `fromSourceMap` function that
constructs a MappedString from `TsSourceMapEntry[]` byte ranges does
**not** live in `@quarto/api/text`. It is q2-specific (needed because
`source_map` crossed the protocol boundary) and lives in
`@quarto/engine-host-deno/src/mapped-source.ts`. It is built on top of the
primitives from `@quarto/api/text` (`asMappedString`, `mappedConcat`) and
maintains a base-per-file cache so all pieces sharing a source file share
one base `MappedString` object. See Plan 1b for the algorithm.

### Phase 2C: Remaining @quarto/api submodules + engine-host wiring

Populate the remaining `@quarto/api` submodules, then flesh out the stub
`quarto-api.ts` in `@quarto/engine-host-deno` that Plan 1a created.

**Construction model** — no registry pattern, and I/O runs through a
`PlatformHost` plugged in by the consumer:

```typescript
// engine-host-deno/src/deno-host.ts
import type { PlatformHost } from "@quarto/api/platform";
export const denoHost: PlatformHost = {
    fs: {
        readTextFileSync: Deno.readTextFileSync,
        writeFileSync: (p, c) => Deno.writeFileSync(p,
            typeof c === "string" ? new TextEncoder().encode(c) : c),
        exists: (p) => { try { Deno.statSync(p); return true; } catch { return false; } },
    },
    process: {
        exec: async (cmd, args, opts) =>
            await new Deno.Command(cmd, { args, ...opts }).output(),
    },
    realPath: Deno.realPathSync,
    isInteractive: Deno.stdin.isTerminal(),
    isCI: !!Deno.env.get("CI"),
};

// engine-host-deno/src/quarto-api.ts
import { denoHost } from "./deno-host.ts";
import * as text from "@quarto/api/text";
import { createMappedStringFromFile } from "@quarto/api/text/mapped-from-file";
import * as markdown from "@quarto/api/markdown";
import * as format from "@quarto/api/format";
import * as pathMod from "@quarto/api/path";
import { createPath } from "@quarto/api/path";
import { createSystem } from "@quarto/api/system";
import * as quartoConsole from "@quarto/api/console";
import * as crypto from "@quarto/api/crypto";
// jupyter wired in by Plan 3

export function buildQuartoAPI(context: EngineHostContext): QuartoAPI {
    const mappedStringFromFile = createMappedStringFromFile(denoHost);
    const pathNamespace = { ...pathMod, ...createPath(denoHost) };
    const systemNamespace = createSystem(denoHost);
    return {
        text:          buildTextNamespace(text),
        mappedString:  buildMappedStringNamespace(text, mappedStringFromFile),
        markdownRegex: buildMarkdownRegexNamespace(markdown),
        format:        buildFormatNamespace(format, context),
        path:          buildPathNamespace(pathNamespace, context),
        system:        buildSystemNamespace(systemNamespace, context),
        console:       buildConsoleNamespace(quartoConsole),
        crypto:        buildCryptoNamespace(crypto),
        jupyter:       buildJupyterNamespace(...),  // Plan 3
    };
}
```

Direct construction, plain nested object. Quarto 1's
`QuartoAPIRegistry`/`register.ts` infrastructure is **not** being ported.
The same pattern in a future `@quarto/engine-host-wasm` replaces `denoHost`
with a VFS-backed host and leaves the rest of the assembly identical.

#### src/format/

- [ ] `src/format/index.ts` — pure computation from `format.pandoc.to` string.
  Each method accepts an optional `format` parameter (Q1 compat) and falls back
  to a context-provided default:
  ```typescript
  export function isHtmlCompatible(format, defaultTo: string): boolean {
      const to = format?.pandoc?.to ?? defaultTo;
      return ["html", "html4", "html5", "revealjs", "s5", "slideous", "slidy",
              "epub", "epub2", "epub3"].some(f => to.startsWith(f));
  }
  export function isLatexOutput(format, defaultTo: string): boolean { … }
  // etc.
  ```
  Engine-host's `buildFormatNamespace` closes over `context.format.pandocTo`
  to provide the default.

#### src/path/

Pure path-string helpers are direct exports (no host dependency):
- [ ] `toForwardSlashes(path)` → `path.replace(/\\/g, "/")`
- [ ] `dirAndStem(file)` → `[dirname(file), basename(file, extname(file))]`
- [ ] `inputFilesDir(input)` → `join(dirname(input), basename(input, ext) + "_files")`
- [ ] `isQmdFile(file)` → check extension

Host-dependent piece is a factory:
- [ ] `createPath(host)` → returns an `absolute(path)` that uses
  `host.realPath` when available, otherwise returns `path` unchanged. The
  WASM-host implementation of `absolute` will typically be identity (VFS
  paths are already canonical); the Deno host delegates to `Deno.realPathSync`.

Engine-host-deno's `buildPathNamespace` composes these with the
`runtime(subdir)` and `resource(...parts)` closures that use
`context.runtimeDir` / `context.resourceDir`.

#### src/system/

All of `system/` is host-dependent — expose as a factory:

- [ ] `createSystem(host: PlatformHost)` returning:
  - `execProcess(options)` → uses `host.process.exec()` if available, else throws
    `"execProcess is not available in this environment"`
  - `tempContext()` — creates temp dir via `host.fs` (Deno: `Deno.makeTempDirSync`;
    browser: a VFS-scoped directory). Returns a cleanup helper.
  - `onCleanup(handler)` — pure JS; registers in a module-level list processed
    on exit / dispose.
  - `isInteractiveSession()` → `host.isInteractive`
  - `runningInCI()` → `host.isCI`

Engine-host-deno's `buildSystemNamespace` wraps the factory output with a
`pandoc(args, stdin?)` convenience that uses `context.pandocPath`. (In the
future WASM host, `pandoc` can't be spawned — either route through a WASM
build of pandoc or throw unsupported.)

#### src/console/

- [ ] `src/console/index.ts`:
  - `info(message, options?)` → `console.error("[INFO]", message)` (goes to stderr)
  - `warning(message, options?)` → `console.error("[WARN]", message)`
  - `error(message, options?)` → `console.error("[ERROR]", message)`
  - `withSpinner(options, fn)` → log start/end, call fn (no actual spinner in subprocess)

  The `options` parameter (`{ bold, newline, indent, ... }`) is accepted but
  formatting hints are best-effort in a subprocess context (no terminal control).

#### src/crypto/

- [ ] `src/crypto/index.ts` — `md5Hash(content)`. Note: Web Crypto doesn't
  natively support MD5. Options: `npm:md5`, `node:crypto` (available in
  Deno), or a small pure-JS MD5.

#### Wire-up in engine-host

- [ ] Update `@quarto/engine-host-deno/src/quarto-api.ts` to import from
  `@quarto/api/*` and assemble the QuartoAPI object as shown above.
- [ ] Add `@quarto/api` as a dependency of `@quarto/engine-host-deno`.
- [ ] Write a smoke test that invokes each namespace method with trivial
  inputs to verify the wiring.

### Phase 2D: @quarto/types and import map

Following Quarto 1's model, engine extensions import types via
`import type { ... } from "@quarto/types"`. These are erased during the
build step (bundling), so no runtime code is needed — just a `.d.ts` file
referenced by the import map.

- [ ] Define our type definitions in `ts-packages/quarto-types/` (or
  `resources/extension-build/quarto-types.d.ts`):
  - `ExecutionEngineDiscovery`, `ExecutionEngineInstance`
  - `ExecuteOptions`, `ExecuteResult`, `ExecutionTarget`
  - `QuartoAPI` (with our namespace signatures)
  - `MappedString`, `PartitionedMarkdown`, `Metadata`
  - `EngineProjectContext`
- [ ] For compatibility with Quarto 1 extensions: our type names should match
  Quarto 1's.
- [ ] Create `resources/extension-build/import-map.json`:
  ```json
  {
    "imports": {
      "@quarto/types": "./quarto-types.d.ts",
      "path": "jsr:@std/path",
      "fs/exists": "jsr:@std/fs/exists",
      "encoding/base64": "jsr:@std/encoding/base64"
    }
  }
  ```
- [ ] Create `resources/extension-build/deno.json`:
  ```json
  {
    "compilerOptions": { "strict": true, "lib": ["deno.ns", "DOM", "ES2021"] },
    "importMap": "./import-map.json"
  }
  ```
- [ ] Copy `quarto-types.d.ts` into `resources/extension-build/` during the
  build process.

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
6. **Publish target deferred, but shape committed.** We don't publish to npm
   or jsr yet. When we do, no structural changes should be needed — just add
   a `publishConfig` and a version.
7. **Scope naming.** `@quarto/api` is intended to coexist with Q1's existing
   `@quarto/types`. If Q1's package layout changes, we coordinate naming.

## Design Notes

### Why rewrite instead of extract?

Quarto 1's markdown utilities are tangled with the YAML schema/validation
system (~30+ files), tree-sitter, mapped-text infrastructure, and lodash.
Clean rewrites of the actual logic are ~50-300 lines per function, vs.
extracting would require bringing 30+ files and stubbing their dependencies.
The logic itself is straightforward — it's the plumbing that's tangled.

### Why a single `@quarto/api` package?

Earlier drafts of this plan proposed `@quarto/markdown`, `@quarto/jupyter`,
and `@quarto/engine-host-deno` as sibling packages. We consolidated to a single
`@quarto/api` package with subpath exports because:

- One `package.json`, one version, one dep list (`yaml` lives once).
- Q1 adopts once (`import { ... } from "@quarto/api/markdown"`), not three times.
- MappedString has a natural home (`@quarto/api/text`) without debate over which
  sibling owns it.
- Cross-submodule deps (if any) don't require version coordination.
- Tree-shaking via subpath exports gives the same bundle cost as separate packages.
- `git subtree split` can later extract a subdirectory if one piece outgrows the rest.

`@quarto/engine-host-deno` stays separate because it's q2-specific (stdio protocol,
source-map rehydration).

### YAML cell options: simplified approach

Quarto 1's `partitionCellOptions` uses the full YAML schema system to
validate cell options. Our `breakQuartoMd` skips validation and just parses
YAML with `js-yaml`. This means:
- Cell options with typos won't be caught at parse time
- That's fine — validation happens elsewhere in q2's pipeline
- The engine extension just needs the parsed options as a plain object

### Future: Quarto 1 adoption

`@quarto/api` is designed so that Q1 could import it in place of its own
tangled implementations (`src/core/lib/mapped-text.ts`,
`src/core/pandoc/pandoc-partition.ts`, etc.). The API signatures match Q1's
existing interfaces. If/when Q1 adopts it, Q1's `QuartoAPIRegistry` keeps
its existing shape but providers delegate to `@quarto/api` submodules.

## Success Criteria

- [ ] `@quarto/api` package exists with package.json, tsconfig, exports map
- [ ] `@quarto/api/platform` defines the `PlatformHost` interface
- [ ] No `Deno.*` or `node:*` references anywhere inside `@quarto/api`
  (verified by a simple grep check in CI or xtask lint)
- [ ] `@quarto/api/markdown` with extractYaml, partition, getLanguages, breakQuartoMd
- [ ] `@quarto/api/text` with MappedString + helpers (full `.map()` provenance),
  `createMappedStringFromFile(host)` factory for FS-backed construction
- [ ] `@quarto/api/format`, `/path`, `/system`, `/console`, `/crypto` all implemented;
  `path` and `system` expose host-factory constructors (`createPath`, `createSystem`)
- [ ] `@quarto/engine-host-deno` provides a `denoHost: PlatformHost` and uses it
  to build every namespace
- [ ] All QuartoAPI namespaces except jupyter wired into engine-host-deno's
  `quarto-api.ts` via direct construction (no registry)
- [ ] `fromSourceMap` in engine-host-deno reconstructs provenance from
  byte-range entries
- [ ] `@quarto/types` definitions in place for engine extension imports
- [ ] All tests pass
