# Plan 1b: @quarto/engine-host-deno (Deno harness)

**Grand plan:** [2026-04-16-ts-engine-extensions-subprocess.md](2026-04-16-ts-engine-extensions-subprocess.md)
**Depends on:** Plan 1a (Rust core: protocol types, `TsEngine`) — this
plan needs the frozen JSON protocol schema from Plan 1a Phase 1. Strictly
speaking, only the schema gates 1b; the rest of 1a (subprocess management,
trait extensions, `TsEngine` struct) runs in parallel with 1b if two
people are working.
**Blocks:** Plan 1c (extension integration + E2E echo test), Plan 2 Phase 2C
(wire QuartoAPI namespaces into the harness), Plan 3 Phase 3E (wire jupyter
into the harness), Plan 4 (Julia validation).
**Estimated sessions:** 1

## Overview

Build the Deno-side subprocess harness — the TypeScript package that
receives JSON protocol messages on stdin, dispatches to a loaded engine
module, and writes typed responses to stdout. This is the counterpart to
Plan 1a's Rust-side subprocess manager.

**Build model:** Following the existing `quarto-system-runtime` pattern (see `crates/quarto-system-runtime/js/`):
1. Source lives in `ts-packages/quarto-engine-host-deno/src/`
2. **esbuild** bundles it into a single `dist/engine-host-deno.js` (checked into git)
3. Rust embeds it via `include_str!("../../ts-packages/quarto-engine-host-deno/dist/engine-host-deno.js")`
   in `ts_process.rs` (behind `#[cfg(not(target_arch = "wasm32"))]` with the rest of the module)
4. At runtime, writes the embedded JS to a temp file and runs `deno run --allow-all <tempfile>`
5. Only developers editing the TS harness need to rebuild (via `npm run build` in the package)

## Phase order

Phase 1 → Phase 2 → Phase 3 → Phase 4

## Work Items

### Phase 1: Package setup + esbuild

- [ ] Create `ts-packages/quarto-engine-host-deno/package.json`:
  ```json
  {
    "name": "@quarto/engine-host-deno",
    "version": "0.1.0",
    "type": "module",
    "main": "src/host.ts",
    "scripts": {
      "build": "node esbuild.config.mjs"
    }
  }
  ```
- [ ] Create `esbuild.config.mjs` — bundle `src/host.ts` → `dist/engine-host-deno.js`.
    Use `platform: "neutral"` and `format: "esm"` (NOT the `platform: "browser"` /
    `format: "iife"` pattern from `quarto-system-runtime` — that targets QuickJS via
    Boa, while engine-host-deno targets Deno which runs ES modules and has its own globals
    like `Deno.stdout`, `Deno.Command`)
- [ ] Add `@quarto/api` and `@quarto/types` as dependencies. At this point
    in the sequence, `@quarto/api` may still be a skeleton (Plan 2A) with
    stubs for most namespaces — that's fine; see Phase 3.

### Phase 2: `host.ts` main loop

- [ ] Create `src/host.ts`:
  ```typescript
  // Redirect stdout so engine code can't accidentally corrupt the protocol
  const protocolOut = Deno.stdout;
  // Read JSON messages from stdin, dispatch, write responses to protocolOut
  ```
  - Read lines from stdin, parse as JSON, dispatch by `type` field
  - Write JSON response + newline to protocol stdout
  - Handle errors gracefully (catch, send error message, don't crash)

- [ ] **Must dispatch all message types** from the protocol (matching Plan 1a's
  `ToEngine` enum exactly):
    - `init` → load engine, call `engine.init(quartoAPI)`, call `engine.launch(context)`, return `ready`
    - `claimsLanguage` / `claimsFile` → call discovery methods on loaded engine
    - `markdownForFile` → call `instance.markdownForFile(file)` (non-QMD files only)
    - `partitionedMarkdown` → call `instance.partitionedMarkdown(file, format?)` if
      implemented; else fallback to `partition(markdownForFile(file).value)`
    - `execute` → call `instance.target()` if implemented (harness-internal),
      then construct `ExecutionTarget` from target result or `TsExecuteOptions` fields
      (source_path, input text wrapped as MappedString, pre-extracted metadata),
      construct `Format` from `TsFormatInfo`, call `instance.execute(options)`
    - `filterFormat` → call `instance.filterFormat(source, options, format)` if implemented
    - `executeTargetSkipped` → call `instance.executeTargetSkipped(target, format)` if implemented
    - `dependencies` → call `instance.dependencies(options)`
    - `postprocess` → call `instance.postprocess(options)`
    - `postRender` → call `instance.postRender(file)` if implemented
    - `canKeepSource` → call `instance.canKeepSource(target)` if implemented
    - `intermediateFiles` → call `instance.intermediateFiles(input)` if implemented
    - `shutdown` → clean up, exit

- [ ] **`target()` is harness-internal**, not a protocol message. Before
    calling `execute()`, the harness checks if the engine implements
    `target()`. If so, it calls it with the reconstructed MappedString, and
    uses the returned `ExecutionTarget` (including the opaque `data` cookie
    like Jupyter's kernelspec). If not, the harness constructs
    `ExecutionTarget` from `TsExecuteOptions` fields. Entirely Deno-side —
    q2 never sees target() results.

- [ ] **`partitionedMarkdown` dispatch** — dispatched when q2 sends the
    `PartitionedMarkdown` message. If the engine implements it, call it.
    If not, fall back to `partition(markdownForFile(file).value)` — calls
    `markdownForFile` first (handles percent/spin conversion), then partitions.

- [ ] For the `execute` dispatch, the harness constructs `Format` from
    `TsFormatInfo` and bridges q2's data to the shapes the engine expects.

- [ ] For optional methods (`filterFormat`, `executeTargetSkipped`, `canKeepSource`,
    `intermediateFiles`, `postRender`, `run`): if the engine doesn't implement them,
    return sensible defaults (pass-through format, true, empty list, void).

### Phase 3: Supporting modules

- [ ] Create `src/deno-host.ts` — the `PlatformHost` implementation used by
    `@quarto/api` factory exports:
  ```typescript
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
  ```

- [ ] Create `src/quarto-api.ts` — stub implementation:
  - Build a `QuartoAPI` object from `EngineHostContext` as a plain nested
    record (no registry pattern — Quarto 1's `QuartoAPIRegistry` and
    `register.ts` side-effect module are deliberately not ported).
  - For now, return stubs for every namespace that throw "not yet implemented".
    The real implementations live in `@quarto/api` and are wired in by
    Plans 2 and 3 (which replace these stubs with real factories threaded
    through `denoHost`).
  - Both `quarto.text` and `quarto.mappedString` namespaces exist on the
    surface (Q1 compat); their implementations will later both pull from
    `@quarto/api/text`.

- [ ] Create `src/mapped-source.ts` — MappedString rehydration from
    `TsSourceMapEntry[]`. This is the q2-specific piece (not in `@quarto/api`
    itself) because the `source_map` crosses the protocol boundary as data
    rather than in-memory references.

  **Rust side flattens before sending:**
    - `Original { file_id, start_offset }` → resolve FileId to path via SourceContext
    - `Substring` → walk parent chain to Original, compute absolute file offset
    - `FilterProvenance` → emit with empty `file` string (sentinel, unmappable)
    - Nested `Concat` → flatten recursively
    File IDs are resolved to path strings on the Rust side since the Deno
    process doesn't have SourceContext.

  **Deno-side reconstruction:**
    1. For each unique file in `source_map`, lazily read the file via
       `denoHost.fs.readTextFileSync` and create a base `MappedString` with
       `.fileName` set (cached per file for identity + single read).
    2. The main MappedString's `.map(index)` binary-searches the sorted
       entries to find which piece contains the index, computes the offset
       in the original file (`piece.fileOffset + (index - piece.start)`),
       and returns `{ index: offset, originalString: baseForFile }`.
    3. For `closest=true` on an unmappable range (empty file sentinel),
       scan to the nearest entry with a valid file mapping.
    4. `splitLines` and `indexToLineCol` are pure TS utilities that
       operate on this MappedString — no special protocol support needed.
    5. This gives character-level accuracy — engines like Julia that
       call `line.map(0, true)` in `buildSourceRanges()` get correct
       original file + position, even through include boundaries.

- [ ] **MappedString serialization for `markdownForFile` (Deno → Rust):**
    When an engine converts a non-QMD file to QMD via `markdownForFile`, the
    result is a `MappedString` with provenance back to the original file.
    The harness serializes this mapping by walking the output text, calling
    `.map()` to find contiguous ranges mapping to the same file with sequential
    offsets, and emitting `TsSourceMapEntry` values. The Rust side converts
    these entries to `SourceInfo::Concat` and attaches it to the AST parsed
    from the generated QMD, enabling error positions in the original `.jl`/`.py`
    file rather than in ephemeral generated text.

- [ ] Create `src/engine-loader.ts`:
  - Dynamically import the engine module: `await import(toFileUrl(path))`
  - Validate it has a default export with `name`, `claimsLanguage`, `launch`
  - Return the `ExecutionEngineDiscovery` object

- [ ] Create `src/types.ts` — protocol message type definitions (must match
    the Rust enums in Plan 1a exactly).

### Phase 4: Bundle + CI

- [ ] Build the bundle with `npm run build` and check `dist/engine-host-deno.js` into git.
- [ ] Add a CI check (or xtask lint) that verifies the checked-in bundle is up
    to date with the sources.

## Design Notes

### Stderr handling

The subprocess's stderr is forwarded to q2's logging. The engine-host-deno
harness prefixes log lines with level markers so q2 can parse them:
```
[INFO] Checking Julia installation...
[WARN] Julia server connection slow
[ERROR] Julia process crashed
```

Unprefixed stderr lines (from the engine itself or from Deno) are logged at INFO level.

### Stdout/stderr contract

**Stdout is exclusively for JSON protocol messages**, one per line. The
engine-host-deno harness writes responses there. If anything else writes to
stdout, the protocol is corrupted.

- The harness overrides `console.log`/`console.info`/`console.warn`/`console.error`
  to all write to **stderr** instead. This handles the common case of engines using
  `console.log` for debugging.
- Engines should use `quarto.console.*` (which writes to stderr with level prefixes)
  for diagnostics.
- We **cannot** prevent a determined engine from calling `Deno.stdout.writeSync()`
  directly — this is documented as a contract violation that will break the protocol.
- On the Rust side (Plan 1a), if a line from stdout fails to parse as JSON,
  report a clear error: "Engine wrote non-protocol output to stdout. Engine
  extensions must use stderr for diagnostics."

### Where is engine-host-deno.js at runtime?

The engine-host-deno harness is bundled into a single `.js` file using **esbuild**.

**Build pipeline:**
1. `ts-packages/quarto-engine-host-deno/esbuild.config.mjs` bundles `src/host.ts` → `dist/engine-host-deno.js`
2. The bundle is checked into git (like `quarto-system-runtime/js/dist/ejs-bundle.js`)
3. `include_str!("../../ts-packages/quarto-engine-host-deno/dist/engine-host-deno.js")` embeds it in the q2 binary
4. At runtime, write the embedded string to a temp file, run `deno run --allow-all <tempfile>`

The engine-host-deno bundle includes `@quarto/api` (all subpaths — text,
markdown, jupyter, format, path, system, console, crypto) and the harness
glue (host, deno-host, quarto-api, mapped-source, engine-loader) — a
single self-contained `.js` file. Only developers editing the TS harness
or `@quarto/api` code need to rebuild it.

**Bundle size note:** The bundle may be large (200-500 KB estimated, depending on
`@quarto/api/jupyter` complexity). Currently q2 only embeds ~50 KB of JS via
`include_str!`. The engine-host-deno bundle is gated behind
`#[cfg(not(target_arch = "wasm32"))]` so WASM builds don't carry it. Flagged
as a possible future concern — if the bundle grows problematically, options
include a cargo feature flag to gate the embed, or loading from a known
filesystem path instead of embedding. For now, embedding is the simplest
approach and matches the existing `quarto-system-runtime` pattern.

### Why a separate plan from 1a?

The Rust-side infrastructure (Plan 1a) and the Deno-side harness (this plan)
are independent once the protocol schema is frozen. Splitting them makes
Plan 1a focus on the Rust compile-time / trait / subprocess-management
concerns, while this plan focuses on a TypeScript package with its own build
pipeline, esbuild config, and test setup. They can be worked on in parallel
if two people are available, and the separation naturally reflects the
`@quarto/engine-host-deno` / `@quarto/engine-host-wasm` split that the
`PlatformHost` abstraction enables later.

## Success Criteria

- [ ] `@quarto/engine-host-deno` package exists with package.json,
  esbuild.config.mjs, tsconfig
- [ ] Harness dispatches every protocol message type from Plan 1a's `ToEngine`
  enum; optional engine methods fall back to sensible defaults when absent
- [ ] `target()` handled as harness-internal (never reaches the protocol)
- [ ] `partitionedMarkdown` falls back to `partition(markdownForFile(...))`
  when the engine doesn't override it
- [ ] MappedString rehydration from `source_map` works end-to-end — a
  `.map(index)` call returns `{ index, originalString }` pointing at the
  correct file and offset even through include boundaries
- [ ] MappedString serialization for `markdownForFile` responses is
  implemented and round-trips through the Rust side
- [ ] `denoHost: PlatformHost` in place; `quarto-api.ts` stub returns a
  QuartoAPI object where every namespace throws "not yet implemented"
  (replaced by Plans 2 and 3)
- [ ] Bundle builds cleanly with `npm run build`, produces
  `dist/engine-host-deno.js`, and the bundle is checked into git
- [ ] CI check verifies the checked-in bundle is up to date
