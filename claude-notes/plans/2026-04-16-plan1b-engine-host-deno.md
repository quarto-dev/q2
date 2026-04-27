# Plan 1b: @quarto/engine-host-deno (Deno harness)

**Grand plan:** [2026-04-16-ts-engine-extensions-subprocess.md](2026-04-16-ts-engine-extensions-subprocess.md)
**Depends on:** plan1a-protocol (Rust core: protocol types) AND Plan 2A
(`@quarto/api` foundation — the `@quarto/api/config` subpath this plan
imports). This plan needs the frozen JSON protocol schema from
plan1a-protocol and the metadata-partition key lists from Plan 2A.
Strictly speaking, only the schema gates the Rust-facing work; plan1a-host
(subprocess management) and plan1a-engine (trait extensions, `TsEngine`
struct) run in parallel with 1b if two people are working.
**Blocks:** Plan 1c (extension integration + E2E echo test), Plan 2 Phase 2D
(wire QuartoAPI namespaces into the harness), Plan 3 Phase 3E (wire jupyter
into the harness), Plan 4 (Julia validation).
**Estimated sessions:** 1

## Overview

Build the Deno-side subprocess harness — the TypeScript package that
receives framed `Request` envelopes on **stdin**, dispatches each to a loaded
engine module via a **non-blocking read loop** (concurrent across engines,
serialized per engine instance), and writes `Response` envelopes to **stdout**.
This is the counterpart to plan1a-host's Rust-side subprocess manager and its
demux.

> **Concurrency (Phase 1.5).** Pass-2 render is now parallel, so the shared
> subprocess is reached concurrently. The wire is **multiplexed** — every frame
> carries an `id` (plan1a-protocol Phase 1.5), and the harness keeps reading
> while prior requests are still running, so cross-engine requests interleave on
> the Deno event loop. **The channel stays stdin/stdout in v1** — multiplexing
> is all parallel Pass-2 needs, and the existing "stdout is protocol;
> `console.log` is forbidden" contract is retained. Moving the protocol off
> stdout onto **loopback TCP** (to delete that footgun) is an orthogonal cleanup
> deferred to **Phase 1.6**. Canonical model:
> `claude-notes/designs/engine-host-concurrency.md`.

**Build model:** commit a pre-built JS artifact and embed it via
`include_str!`. q2 already does exactly this for browser-side JS assets —
`crates/quarto-core` `include_str!`s the committed
`resources/revealjs/reveal.js` and `resources/attribution/viewer.js`.
(Note: the older `crates/quarto-system-runtime/js/dist/ejs-bundle.js`
precedent some earlier drafts cited was **removed** with deno_core/rusty_v8
in bd-3e3sam51 — do not look for it.) The model stands on its own merits and
does not depend on a surviving sibling; the reveal.js embed is cited only as
evidence the pattern is already in the tree:
1. Source lives in `ts-packages/quarto-engine-host-deno/src/`
2. **esbuild** bundles it into a single `dist/engine-host-deno.js` (checked into git)
3. Rust embeds it via `include_str!("../../../../ts-packages/quarto-engine-host-deno/dist/engine-host-deno.js")`
   in `ts_process.rs` (behind `#[cfg(not(target_arch = "wasm32"))]` with the rest of the module).
   Path is relative to `crates/quarto-core/src/engine/ts_process.rs` — four `..`s
   reach the repo root (`engine/`→`src/`→`quarto-core/`→`crates/`→root), and
   `ts-packages/` sits at the root, so the path resolves.
   plan1a-host ships a placeholder file at this path so `include_str!` compiles cleanly on fresh clones; Plan 1b replaces the placeholder with the real esbuild output.
4. At runtime, writes the embedded JS to a temp file and runs `deno run --allow-all <tempfile>`
5. Only developers editing the TS harness need to rebuild (via `npm run build` in the package)

## Phase order

Phase 1 → Phase 2 → Phase 3 → Phase 4

**Compilation/build order note.** The phases below are *conceptual layers*,
not a strict file-creation order. `host.ts` (Phase 2) is the spine and is
described first for narrative clarity, but it **imports** the Phase 3
supporting modules (`deno-host.ts`, `quarto-api.ts`, `mapped-source.ts`,
`engine-loader.ts`, `types.ts`). Create those modules first (even as typed
skeletons) so `host.ts` typechecks; flesh out `host.ts`'s dispatch body
against them. Treat Phase 0 (the Test Seam Spec, below) as the true first
step per this repo's tests-first workflow.

## Work Items

### Phase 0: Test Seam Spec (write these tests first)

This repo is tests-first. The existing "Tests for the Engine API contract"
block (below, under the dispatch section) covers `init`/gating/idempotency/
`htmlDependency`. This phase adds the **missing** seams the review surfaced —
the most error-prone logic in the harness — each bound to a named revert hunk
(the one production change whose removal turns the named assertion RED). Once a
test goes green its assertions and harness are frozen; never edit a test to go
green. All five run in the **node/deno (vitest) tier** — there is no layout/
geometry/scroll behavior here, so jsdom-vs-browser does not arise. The unit
under test is never mocked; mock only the genuine boundaries noted.

- [ ] **T1 — metadata partition (`metadataAsFormat` port).** *Tier:* pure
  logic (vitest, no IO, no mock). *Real unit:* the partition function that
  turns the merged metadata map + the five `@quarto/api/config` lists into
  Q1's nested `Format`. *Seam:* call it with a fixture map containing (a) a
  nested `execute: { echo: false }` bin, (b) one flat key from each of
  `kIdentifier/kRender/kExecute/kPandoc`, (c) the **`pdf-standard`** key
  (the one cross-list overlap — in both render and pandoc), (d) tail-norm
  inputs: a string `server`, a singular `ipynb-filter`, a `gfm+...` variant;
  assert the resulting bin for each. *Mock boundary:* none. *Named reverts:*
  ▸ revert the Stage-1 nested-bin peel → assertion "`format.execute.echo ===
  false`" RED (without the peel it misfiles into `format.metadata`).
  ▸ revert the render-before-pandoc ordering → assertion "`pdf-standard ∈
  format.render`" RED (it would land in pandoc). This assertion is the
  discriminator: `pdf-standard` is in *both* lists, so it distinguishes
  "render checked first" from "pandoc checked first" — a non-overlapping key
  could not.
  ▸ revert the `ipynb-filter`→`ipynb-filters` coalesce → that assertion RED.
  Per the resolved partition decisions (match Q1, see "Resolved partition
  decisions"), add two more bound assertions:
  ▸ a *flat* `kLanguageDefaultsKeys`-style key (e.g. a `toc-title-document` at
  top level — a real member of the list; there is no bare `toc-title` key)
  lands in `format.metadata`, **not** `format.language` (revert: add a
  flat-key `→ format.language` branch → RED); and a *nested* `language: {…}`
  block does land in `format.language` (revert: drop `language` from the
  Stage-1 peel set → RED).
  ▸ a key classified into a bin (e.g. `echo` → `format.execute`) is **absent**
  from `format.metadata` (revert: re-add the mirror-into-metadata copy → the
  "absent from metadata" assertion RED).

- [ ] **T2 — MappedString rehydration accuracy (`mapped-source.ts`).**
  *Tier:* pure logic + a real temp file (vitest). *Real unit:*
  `mapped-source.ts`'s `MappedString.map(index, closest)`. *Seam:* build a
  `TsSourceMapEntry[]` with two mappable pieces pointing into a written temp
  file (choose entries where `file_offset ≠ start` so a no-op `.map()` that
  returned `index` unchanged would fail) plus one `source: None` piece; call
  `.map(idx)` on a mappable index and `.map(idx, true)` on the unmappable
  range. *Mock boundary:* the filesystem only — either a real temp file or a
  fake `PlatformHost.fs`; never mock the rehydration itself. *Named reverts:*
  ▸ revert the offset computation `piece.fileOffset + (index - piece.start)`
  (e.g. to return `index`) → the mapped-offset assertion RED. The `file_offset
  ≠ start` choice is what makes this assertion actually exercise the
  computation rather than pass vacuously.
  ▸ revert the ENOENT/`source: None` tolerance (the try/catch) → the
  missing-file case flips from "returns synthetic position" to throwing.
  ▸ revert the `closest=true` nearest-entry scan → the unmappable-with-closest
  assertion RED.

- [ ] **T3 — per-message-type dispatch + `id` correlation (`host.ts` loop).**
  *Tier:* integration (vitest) against the real dispatch loop over an in-memory
  framed duplex (a paired reader/writer standing in for stdin/stdout).
  *Real unit:* `host.ts`. *Seam:* feed each `Request` frame
  (`{ id, msg: { type: 'loadEngine' | 'launchEngine' | 'claimsLanguage' |
  'claimsFile' | 'markdownForFile' | 'execute' | 'intermediateFiles' |
  'shutdown' | 'cancel' } }`) and assert the `Response` frame carries the
  matching response `type` **and the same `id`**. *Mock boundary:* a minimal
  test engine module + the in-memory duplex; the loop is real. *Named reverts:*
  ▸ remove the `case 'execute'` arm (parametrize one revert per arm) → that
  message's response assertion RED. ▸ remove the
  "discovery-message-before-loadEngine → error" guard → the negative-path
  assertion (error response) RED. ▸ hard-code the response `id` to `0` instead
  of echoing the request's → the id-correlation assertion RED.

- [ ] **T4 — `markdownForFile` mapping serialization (TS side only in v1).**
  *Tier:* TS unit (vitest). (a) serialize a known multi-piece `MappedString` →
  assert **one entry per piece, no coalescing** (construct two
  adjacent-and-contiguous pieces; assert the result still has two entries, not
  one). *Named revert:* ▸ add an adjacent-merge (coalescing) step on the TS side
  → the "two entries" assertion RED.
  **v1 scope note (plan1a-engine SEAM-3 / C′):** parts (b) the *Rust-side
  `SourceInfo::Concat` reconstruction* and (c) the *end-to-end faithful-position*
  pass are **deferred to A′** — v1 carries `source_map` on the wire **unconsumed**,
  so there is no Rust reconstruction to assert against yet. Keep only the TS
  serialization unit test (a); it pins the future-A′ input shape without testing
  code v1 doesn't build.

- [ ] **T5 — concurrent dispatch, per-engine serialization (`host.ts`).**
  *Tier:* integration (vitest). *Real unit:* the loop's non-blocking dispatch +
  the per-engine-instance serialization queue. *Seam:* register two test engines
  (A, B), each with an `execute` that blocks on a controllable deferred and logs
  ordered side-effects. Two assertions:
  - **Cross-engine concurrency:** send `execute`→A then `execute`→B *without*
    resolving A's deferred; assert **B's handler starts** (the read loop did not
    block on A). Resolve both; both responses come back with their own `id`.
  - **Same-engine serialization:** send `execute`→A then a second `execute`→A
    before resolving the first; assert the second does **not** start until the
    first resolves (the per-engine queue serializes them).

  *Mock boundary:* the engines' deferreds; the loop + queue are real. *Named
  reverts:* ▸ make the read loop `await` each handler before reading the next
  frame → the cross-engine-concurrency assertion RED. ▸ remove the per-engine
  queue (dispatch same-engine requests concurrently) → the same-engine-ordering
  assertion RED.

- [ ] **T6 — cooperative cancel via `Cancel` (`host.ts`).** *Tier:* integration
  (vitest). *Real unit:* the loop's `Cancel { target }` handling. *Seam:* a test
  engine whose `execute` awaits an `AbortSignal`-aware deferred that rejects when
  aborted; send `execute` (id=N), then `Cancel { target: N }` before resolving;
  assert the engine's `AbortSignal` fired and the request N resolves as
  `Cancelled`, while a *concurrent* request on another engine is unaffected.
  *Mock boundary:* the engine's abortable deferred; the loop is real. *Named
  reverts:* ▸ drop the `case 'cancel'` arm → the abort-fired assertion RED. ▸
  abort *all* in-flight tasks instead of only `target` → the "sibling
  unaffected" assertion RED.

- [ ] **T7 — concurrent same-instance poison → transparent re-launch (`host.ts`).**
  *Tier:* integration (vitest). *Real unit:* the dispatch path's
  reconstruct-on-missing-instance. *Seam:* one engine (julia). Send `execute`
  (id=A) and a second `execute` (id=B) on julia; B queues behind A on the
  per-engine queue. `Cancel { target: A }` → A poisons (instance dropped). Assert
  B then **transparently re-launches** (a second `engine.launch` is observed) and
  **completes normally** (B resolves with an `executeResult`, NOT an error).
  *Mock boundary:* the engine's deferreds + a `launch` call-counter; the loop +
  queue are real. *Named revert:* ▸ make dispatch *fail* a request whose instance
  is missing (instead of reconstructing) → B's "completes normally" assertion RED.

**Missing-test pass (reasoned, per the skill).** Behavior deliberately left
unguarded *here*, with rationale:
- **Whole-subprocess SIGKILL** (crash / compromised channel / teardown) —
  accepted-untested in 1b. Rationale: the kill is issued and observed on the
  Rust side; the binding test is plan1a-host's "crash/malformed → subprocess
  gone". 1b's contract is the *cooperative* path (T6), not the SIGKILL one.
- **Stdout-violation detection** (`console.log` corrupts the v1 stdout protocol)
  — accepted-untested in 1b; owned and tested by plan1a-host (the Rust side parses
  stdout). 1b's contract is only "the harness writes nothing but `Response`
  frames to stdout," which T3 indirectly exercises (every asserted line is valid
  JSON). (Phase 1.6 retires this concern.)
- **`htmlDependency` relative-path normalization** and **`loadEngine`
  path-drift error** — already itemized as bound tests in the Engine-API
  contract block below; not re-listed.

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
    Use `platform: "neutral"` and `format: "esm"` (NOT `platform: "browser"` /
    `format: "iife"` — that shape targets an embedded QuickJS/Boa runtime, whereas
    engine-host-deno targets Deno, which runs ES modules and has its own globals
    like `Deno.stdout`, `Deno.Command`)
- [ ] Add `@quarto/api` and `@quarto/types` as dependencies. The Plan 2A
    **foundation** (done) provides the `@quarto/api/config` subpath and the
    `@quarto/types` package (vendored in-repo, published to `jsr:`/`npm` for
    external engine authors per the grand plan's "Distribution of the
    engine-author SDK"). The rest of what this plan's tests exercise comes
    from **Plan 2A §2aa** (the runtime-surface section of the same plan): the
    `@quarto/api/platform` subpath (the q2-original `PlatformHost` type — see
    Phase 3) and **real** implementations of the pure + host-only namespaces
    1b's contract tests call (`text`, `markdownRegex`, `format`, `crypto`,
    `console`, `path`, `system`, `mappedString`). Only the heavy/launch-context
    namespace `jupyter` and the launch-context-dependent method *bodies* remain
    deferred to Plans 2/3 — see Phase 3 for what 1b wires vs. what it can
    leave throwing "not yet implemented".

    **Format-key partition lists.** The metadata-partition key lists —
    `kExecuteDefaultsKeys`, `kRenderDefaultsKeys`, `kPandocDefaultsKeys`,
    `kIdentifierDefaultsKeys`, `kLanguageDefaultsKeys` — live in
    **`@quarto/api/config`** (`ts-packages/quarto-api/src/config/`). They are a
    careful extraction of the same five lists in Quarto 1's
    `external-sources/quarto-cli/src/config/constants.ts` (same key names, same
    grouping; Q1's symbol-reference arrays resolved to string values) and are
    re-synced whenever Q1 drifts. Q1's `constants.ts` is the **parity
    reference** — read-only, never imported. `@quarto/api/config` is the
    **runtime home** the engine-host harness imports to partition q2's single
    merged metadata map (`doc.ast.meta` after `MetadataMergeStage`) into Q1's
    nested `Format` shape (`format.execute` / `format.render` / `format.pandoc`
    / `format.identifier` / `format.language`). Keeping the lists on the side
    that speaks Q1's vocabulary means a Q1 re-sync is a transcription of one
    file with no Rust-side translation table. Plan 2A creates this subpath; see
    the partition rule in Phase 2.

### Phase 2: `host.ts` main loop

- [ ] Create `src/host.ts` — **non-blocking, multiplexed dispatch over
  stdin/stdout** (v1):
  ```typescript
  // v1: protocol runs on stdin/stdout. Capture the real stdout reference for
  // protocol writes BEFORE any engine code runs (engines must not write to
  // stdout — see "Stdout/stderr contract"). The harness does NOT override
  // console.* — engine authors use stderr (console.error/warn or
  // quarto.console.*). (Phase 1.6 swaps stdin/stdout for a loopback-TCP conn
  // and frees stdout for diagnostics — see plan1a-host "Deferred: Phase 1.6".)
  const protocolOut = Deno.stdout;
  const writeMutex = new AsyncMutex();               // serialize frame writes
  const perEngineQueue = new Map<string, Promise<unknown>>();  // tail per engine
  const inflight = new Map<number, AbortController>();          // by request id

  for await (const frame of readFrames(Deno.stdin)) {  // <-- never awaits handler
    const { id, msg } = frame;
    if (msg.type === "cancel") { inflight.get(msg.target)?.abort(); continue; }
    dispatch(id, msg);                                 // fire-and-forget task
  }
  ```
  - **Non-blocking read loop.** The loop reads the next frame *without* awaiting
    the previous handler — that is what makes cross-engine requests run
    concurrently on the Deno event loop. (T5 binds this.)
  - **Per-engine-instance serialization.** `dispatch` chains the request on
    `perEngineQueue.get(msg.engine)` (a promise tail), so two requests to the
    *same* engine run one-after-another (a kernel is not re-entrant), while
    different engines proceed in parallel. (T5 binds this.)
  - **`id` correlation.** Each response is written as `{ id, msg: <FromEngine> }`
    echoing the request's `id`; the Rust demux routes by it. (T3 binds this.)
  - **`Cancel { target }`.** Each in-flight request runs under an
    `AbortController` stored in `inflight[id]`; `Cancel` aborts exactly that
    one (passing its `AbortSignal` into the engine's `execute`), resolves the
    request as `Cancelled`, and leaves siblings untouched. (T6 binds this.)
  - **Poison the instance when the cancelled request was an `Execute`** (the only
    daemon-engaging request): the harness **drops its `instance` entry** for that
    engine. Cancelling a non-`Execute` request engages no daemon and drops no
    instance. (Harness half of plan1a-host's poison policy.)
  - **A concurrent same-instance request transparently re-launches — it is not
    failed.** When `dispatch` dequeues a request (e.g. worker B's `Execute`) for
    an engine whose `instance` entry was dropped by a poison, it **re-runs
    `engine.launch(stashedContext)` to reconstruct the instance, then runs the
    request** — B never fails for A's timeout and never sees a half-torn-down
    instance. (Composes with idempotency: a *present* instance makes
    `LaunchEngine`/re-launch a no-op; a *missing* one triggers exactly one lazy
    reconstruct, whether from a queued request here or a fresh `LaunchEngine`
    from q2. The harness stashes the last `context` per engine for exactly this.)
    See the design note's poison §3. (T7 binds this.)
  - **Frame writes are mutexed** (`writeMutex`) so concurrent tasks never
    interleave bytes of two frames on stdout.
  - Handle handler errors gracefully (catch → send `error` under the same `id`,
    never crash the loop).

- [ ] **Must dispatch all message types** from the protocol (matching plan1a-protocol's `ToEngine` enum exactly):
    - `loadEngine` → `await import(toFileUrl(enginePath))`, validate
      exports, call `engine.init?.(quartoAPI)` (optional — engines that
      use `quarto.*` implement it to stash the reference, per Q1
      contract). The QuartoAPI handed to `init()` is bound to a shared
      `HostState` whose `context` is unset until the first
      `launchEngine`; pure and host-only methods work immediately, gated
      methods throw a clear "not available before engine launch" error
      until then. See the "Engine API contract" section below. Store
      the `ExecutionEngineDiscovery` object keyed by engine name. Return
      `loaded` with `LoadEngineResult` (name, validExtensions). If
      `init()` throws, treat as a load failure and return `error`.

      **Idempotent on repeat.** A `loadEngine` for an engine name that
      is already present in the harness's `Map<engineName, ...>` MUST
      NOT re-run `import()` or re-call `engine.init?.(quartoAPI)`. The
      harness returns the cached `LoadEngineResult` directly. If the
      message's `enginePath` differs from the cached entry's path,
      respond with `error` ("engine name reused with different path:
      ${cachedPath} vs ${msg.enginePath}") — config drift is a bug, not
      a silent overwrite. This idempotency is what lets plan1a-engine use
      naive `OnceLock<...>` for the Rust-side init state without
      double-checked locking; see plan1a-engine "Race-free init via
      harness idempotency."
    - `launchEngine` → if the engine is already launched (cached
      `instance` in the harness's per-engine record), return the cached
      `LaunchEngineResult` without re-running `engine.launch(context)`.
      In dev builds, assert the supplied `context` matches the cached one
      on a small **identity key** — `(project_dir, resource_dir,
      is_single_file)` — **not** a full deep-equal (the context carries
      absolute paths and a per-render `temp_dir` shim that legitimately
      differ run-to-run, so deep-equal would false-positive). In release,
      silently use the first context.
      Otherwise: set `state.context = msg.context` (the same shared
      `HostState` the QuartoAPI closes over; this unblocks the gated
      methods for *all* loaded engines), then call
      `engine.launch(context)`, store the resulting
      `ExecutionEngineInstance`, and return `launched` with
      `LaunchEngineResult` (canFreeze, generatesFigures). **Source these from
      two different objects:** `canFreeze` is on the
      `ExecutionEngineInstance` (`execute/types.ts:95`); `generatesFigures` is
      **not** on the instance — it lives on the `ExecutionEngineDiscovery`
      (`execute/types.ts:58`), so read it from the stored discovery object
      captured at `loadEngine`, not from `engine.launch()`'s return.
      `engine.launch(context)` only **constructs** the
      `ExecutionEngineInstance` object — it is cheap (~0), matching
      Quarto 1, where `launch()` is a synchronous object-literal
      construction that starts no daemon. The expensive engine startup
      (Julia control server / Jupyter kernel: 5+ s) happens **lazily
      inside the engine's `execute()` on the first call** (see the
      `execute` handler below) and is amortized by the external daemon —
      never at `launchEngine`. The idempotency rule still matters —
      double-launching would build duplicate instance objects and confuse
      the shared-context invariant — and it mirrors the `loadEngine` one,
      which is what makes the Rust-side `OnceLock<LaunchEngineResult>`
      safe under concurrent racers (see plan1a-engine). In dev builds,
      also assert that `state.context` (if previously set by another
      engine's launch) hasn't changed on the same identity key —
      protocol allows per-engine context but in practice all engines
      share one.
    - `claimsLanguage` / `claimsFile` → call discovery methods on the loaded
      engine. Engine must be loaded; not required to be launched.
      **`claimsLanguage` normalization:** the engine may return
      `boolean | number | LanguageClaim` (the kind-tagged object); the harness
      normalizes to the tagged wire result before replying — `false`/`null`/
      `undefined` → `None`, `true` → `{kind:"primary",priority:1}`, `number n`
      → `{kind:"primary",priority:n}` (**no sign games — a negative number is a
      low-priority primary, never interop**), and a `LanguageClaim` object
      passes through as its kind. `interop`/`fallback` are reachable only via
      the object. See `claude-notes/designs/engine-resolution.md` §3.2 and
      plan1a-protocol's `TsLanguageClaim` appendix.
    - `markdownForFile` → call `instance.markdownForFile(file)` (non-QMD files
      only); serialize the MappedString result with `source_map` for
      `markdownForFileResult`. Engine must be launched.
    - `execute` → see below for the dependencies-folding flow. Engine must
      be launched. This is where the engine daemon comes up: the engine's
      `execute()` **starts the external daemon (Julia control server /
      Jupyter kernel: 5+ s) lazily on the first call, or reconnects to an
      already-running one** keyed by a transport file in the runtime
      directory. The daemon is amortized across renders and survives a
      Deno-subprocess respawn (reconnect, not relaunch) — it is never
      started at `launchEngine`.
    - `intermediateFiles` → call `instance.intermediateFiles(input)` if
      implemented; else return `undefined`. Engine must be launched.
    - `shutdown` → clean up, exit. **AND: the read loop must also exit when
      stdin reaches EOF** — q2's graceful `TsEngineHost::shutdown()` sends the
      `Shutdown` frame and *then closes the child's stdin*, and the host then
      `join`s waiting for the child to exit (plan1a-host "Teardown & reaping").
      If the harness does not exit on stdin EOF, graceful teardown blocks until
      the host's `Drop` SIGKILL fires — defeating the clean-exit path. So:
      `for await (… of readFrames(Deno.stdin))` falling through (iterator done =
      stdin EOF) must break the loop and `Deno.exit(0)`, the same terminal as the
      `shutdown` message.

  Discovery messages without a prior `loadEngine` for that engine, or
  instance messages without a prior `launchEngine`, return an `error` with
  a clear message. (Rust side guards against this via the `TsEngine` state
  machine, but the harness validates defensively.)

- [ ] **Lifecycle methods deliberately NOT on the protocol.** Q1's
    `filterFormat`, `executeTargetSkipped`, `postprocess`, `canKeepSource`,
    `postRender`, `dependencies`, and `partitionedMarkdown` are not
    protocol messages. The harness does NOT dispatch them as top-level
    messages. `dependencies` is folded into `execute` (see the next
    item). `partitionedMarkdown` is subsumed by q2's `DocumentProfile`
    checkpoint plus filter-aware `markdown_for_file` (see
    `claude-notes/plans/2026-04-23-ipynb-filters-and-engine-partitioning.md`).
    The other Q1 lifecycle hooks have no q2 caller and are deferred —
    when q2 grows callers, they'll appear here as new message types.

- [ ] **Execute dispatch with dependencies folding:**
    1. Call `instance.target()` if implemented (harness-internal), passing
       a reconstructed `MappedString`. Use its result (including the opaque
       `data` cookie like Jupyter's kernelspec) to build the
       `ExecutionTarget` for `execute()`. If not implemented, construct
       `ExecutionTarget` from `TsExecuteOptions` fields (source_path, input
       wrapped as MappedString, pre-extracted metadata).
    2. Construct Q1's nested `Format` object from `TsFormatInfo`, mirroring
       Q1's `metadataAsFormat()`
       (`external-sources/quarto-cli/src/config/metadata.ts:165`). The harness
       imports Q1's canonical key-classification lists — `kExecuteDefaultsKeys`,
       `kRenderDefaultsKeys`, `kPandocDefaultsKeys`, `kIdentifierDefaultsKeys`,
       `kLanguageDefaultsKeys` — from **`@quarto/api/config`**, a careful
       extraction of `external-sources/quarto-cli/src/config/constants.ts`
       (Plan 2A provides this subpath).

       **The metadata map may be nested, not flat.** q2 preserves an
       explicitly-written bin — `execute:\n  echo: false` — as a *nested map*
       under `meta["execute"]`; it does **not** hoist `echo` to the top level
       (verified against `crates/quarto-config/src/format.rs`'s
       `test_resolve_format_nested_objects`). So the partition has two stages,
       matching `metadataAsFormat`:

       - **Stage 1 — peel explicitly-nested bins.** If a top-level key is itself
         a bin name (`execute`, `render`, `pandoc`, `metadata`, `language`, or an
         identifier field) and its value is a map, merge that map's entries
         directly into the corresponding `format.*` bin. A flat-only membership
         test would misfile a nested `execute: {…}` block into the catch-all.
         **This is the only path that fills `format.language`** — Q1 populates
         `format.language` solely from a nested `language:` block (matching
         `metadata.ts:178`, whose bin-name set includes `kLanguageDefaults`),
         never from flat keys (see Stage 2).
       - **Stage 2 — classify the remaining flat keys.** For each remaining
         `(key, value)`, in Q1's list order:
         - If `key ∈ kIdentifierDefaultsKeys` → `format.identifier` (merged with
           the explicitly-shipped identifier fields from `TsFormatInfo.identifier`
           — the explicit values win on overlap; in dev builds, assert no overlap).
           Note `TsFormatIdentifier` is a **quad**, not a triple: `base-format`,
           `target-format`, `display-name`, and an optional `extension-name`
           (skip-if-none, no current engine consumer). Merge all four; don't
           assume three.
         - Else if `key ∈ kRenderDefaultsKeys` → `format.render`.
         - Else if `key ∈ kExecuteDefaultsKeys` → `format.execute`.
         - Else if `key ∈ kPandocDefaultsKeys` → `format.pandoc`.
         - Otherwise → `format.metadata` (the Q1 catch-all).
       - **No flat-key `language` branch** (decided: match Q1). Q1's
         `metadataAsFormat` classifies flat keys against only these **four**
         arrays — `kIdentifier/Render/Execute/PandocDefaultsKeys` — and never
         consults `kLanguageDefaultsKeys`; flat language-ish keys fall to the
         `format.metadata` catch-all. `format.language` comes only from the
         Stage-1 nested `language:` peel.
       - The classification is **order-sensitive** (the if/else-if order above is
         Q1's), so it is deterministic regardless of whether the lists are
         disjoint. They are *not* fully disjoint: `pdf-standard` is in both
         `kRenderDefaultsKeys` and `kPandocDefaultsKeys`, and render-before-pandoc
         ordering sends it to `format.render` — do not reorder without checking
         overlaps against the current `constants.ts` (T1 guards this).
       - **Tail normalization** (Q1's `metadataAsFormat` after the loop): normalize
         `server`, coalesce `ipynb-filter`/`ipynb-filters`, and expand the `gfm`
         pandoc variant. Port these for parity rather than dropping them.
       - **Move, don't duplicate** (decided: match Q1). Each flat key lands in
         **exactly one** bin — the `if/else-if/else` chain partitions keys out
         of `metadata`; it does NOT leave a copy in `format.metadata`. (If a
         specific engine ever genuinely needs a key visible in both
         `format.execute` and `format.metadata`, add it as a named, documented
         q2 exception with the engine identified — not as a blanket mirror.)
       - **Why the harness, not Rust:** Q1's `constants.ts` is the parity
         reference (read-only, never imported); `@quarto/api/config`
         (`ts-packages/quarto-api/src/config/`, provided by Plan 2A) is the
         runtime home the harness imports. Keeping the lists on the side
         that already speaks Q1's vocabulary means a Q1 re-sync is a
         transcription of one file with no Rust-side translation table to
         maintain. See plan1a-protocol's appendix discussion of `TsFormatInfo`
         for the full rationale.
       Construct the remaining Q1 `ExecuteOptions` fields from
       `TsExecuteOptions` and the stashed `EngineHostContext`:
       - `target` ← built per step 1 above.
       - `format` ← built per step 2 above.
       - `resourceDir` ← `EngineHostContext.resource_dir`.
       - `tempDir` ← `TsExecuteOptions.temp_dir`.
       - `libDir` ← `TsExecuteOptions.lib_dir`.
       - `projectDir` ← `EngineHostContext.project_dir` (passes
         through `Option`).
       - `cwd` ← `TsExecuteOptions.cwd`.
       - `params` ← `TsExecuteOptions.params` (`Option<Map>` →
         JS object or undefined). **`params` is its own
         `TsExecuteOptions` field** — q2's runtime `-P`/`--execute-param`
         and `--execute-params <file>` channel — and is **NOT** part of
         the partitioned `format.metadata` map (step 2). It is never
         sourced from `format.metadata["params"]`; a frontmatter `params:`
         key or a `-M params=…` metadata entry lives in `format.metadata`
         and reaches the engine through that separate channel.
       - `quiet` ← `TsExecuteOptions.quiet`.
       - `handledLanguages` ← `TsExecuteOptions.handled_languages` (the
         leave-alone set — languages the engine must re-emit unexecuted). **Its
         inverse is a loud-failure obligation (design doc §10 case 4,
         plan1a-engine):** if the engine is handed a cell in a language it
         *owns* (i.e. NOT in `handledLanguages`) but cannot execute, it must
         return a protocol `error` — which the harness surfaces loudly — and
         MUST NOT silently skip the cell or emit it unexecuted. A silent no-op
         here is exactly the "cell quietly didn't run" failure the ownership
         model exists to prevent. (Maps to plan1a-engine's `NoHandlerForLanguage`
         on the Rust side.)
       - `dependencies: true` — set unconditionally. q2 always wants
         dependencies materialized inline: this forces Jupyter's
         `executeResultIncludes()` path (immediate widget-deps
         materialization) instead of the deferred `engineDependencies`
         map, which the harness then composes with Q1's `dependencies()`
         resolution per step 4. (In Q1 this is a `resolveDependencies`
         option that *defaults* to `true`: `render-files.ts:224` passes
         the flag — overridable, defaulting true at `:146` — and
         `jupyter-embed.ts:602` passes a literal `true`; the `false`
         branch is a Q1-internal optimization. q2 does not expose the
         flag.) **Limitation:** an engine that depends on the
         deferred-deps (`false`) behavior cannot be driven that way from
         q2; if one ever appears, add the flag to `TsExecuteOptions`.
       - `project: ProjectContext` — synthesize a minimal Q1
         `ProjectContext`-shaped record with **only the fields engines
         actually read** (`isSingleFile` from
         `EngineHostContext.is_single_file`; `temp` from a small
         shim that wraps `TsExecuteOptions.temp_dir` with a
         Q1-compatible `createFileFromString` helper). Other fields
         on Q1's `ProjectContext` (notebookContext,
         fileInformationCache, config, files) are not set —
         engines that read them will get `undefined`, but the
         engine-side audit confirmed no engine in Q1's tree reads
         them in `execute()`. If a future Q1 sync brings in such a
         reader, expand `EngineHostContext` and the synthesizer
         together.
       - `previewServer: false` — no q2 use case; pass a safe default.
         (Do **not** add `output` here: `output` is not a member of
         `ExecuteOptions` (`execute/types.ts:149-163`); it belongs only to
         `DependenciesOptions`/`PostProcessOptions`.)
    3. Call `instance.execute(options)`, get back an `ExecuteResult` in
       Q1's shape.
    4. **If the result has `engineDependencies` and the engine implements
       `dependencies()`, call it now**, on the TS side, before responding.
       Q1's `dependencies()` is **not nullary** — it takes a
       `DependenciesOptions` argument (`execute/types.ts:123`). Construct
       that object with all of Q1's **required** fields
       (`execute/types.ts:201-211`): `target` (the same `ExecutionTarget`
       built for `execute()` in step 1), `format` (built per step 2),
       `output`, `resourceDir` ← `EngineHostContext.resource_dir`, and
       `tempDir` ← `TsExecuteOptions.temp_dir`; plus the optional `libDir` ←
       `lib_dir` (a `String` on the protocol — q2 always provides one, see
       plan1a-protocol's `TsExecuteOptions`) and the minimal `projectDir`
       shim. Note `target` and `resourceDir` are mandatory and were easy to
       miss — omitting them is a type error at the call. The engine
       writes any required widget files to `lib_dir` and returns
       `DependenciesResult { includes: PandocIncludes }` — Q1's
       canonical shape (file paths to HTML wrapper files containing
       `<script>` tags, inline registrations, etc.).
    5. **Route Q1's `dependencies()` output to `TsExecuteResult.includes`,
       NOT `htmlDependencies`.** The `DependenciesResult.includes`
       (`inHeader` / `beforeBody` / `afterBody` file paths) merges into
       the `executeResult.includes` field on the wire. Q1's deps shape
       is HTML wrapper files containing inline scripts and CDN URLs;
       converting that to Q2's `{ name, stylesheets, scripts }` is lossy
       (the inline registrations would be dropped). The two dep
       channels are disjoint — see plan1a-protocol's appendix "Two disjoint
       dep channels" note. For Q1-shaped engines, `htmlDependencies` on
       the wire is empty.
    6. **`htmlDependencies` is populated separately**, only by engines
       that opt into the Q2-native structured-deps registration API
       (`quarto.htmlDependency({ name, stylesheets, scripts })`). The
       harness accumulates registrations made during `instance.execute()`
       (see the helper specification in Phase 3) and emits them on the
       wire as `htmlDependencies`. Each entry's `stylesheets` and
       `scripts` MUST be absolute paths to files already on disk; the
       harness normalizes any relative paths against `TsExecuteOptions.lib_dir`
       before serializing.
    7. Build `TsExecuteResult` with `includes` (from Q1's `dependencies()`
       resolution and any direct-includes the engine emitted) and
       `htmlDependencies` (from `quarto.htmlDependency()` registrations).
       Drop `engineDependencies`, `preserve`, `pandoc`, and
       `postProcess` (Q1's field is `postProcess?: boolean`,
       `execute/types.ts:176`, not `needsPostprocess`; deferred — q2 has
       no postprocess stage).
    8. Send `executeResult`.

    Engines that don't implement `dependencies()` but return
    `engineDependencies` get a diagnostic on stderr; the data is
    silently dropped.

- [ ] **`target()` is harness-internal**, not a protocol message. Before
    calling `execute()`, the harness checks if the engine implements
    `target()`. If so, it calls it with the reconstructed MappedString, and
    uses the returned `ExecutionTarget` (including the opaque `data` cookie
    like Jupyter's kernelspec). If not, the harness constructs
    `ExecutionTarget` from `TsExecuteOptions` fields. Entirely Deno-side —
    q2 never sees target() results.

    **No caching.** The harness calls `target()` fresh per `Execute`
    message when the engine implements it. Results are not memoized
    across messages — `target()` is cheap relative to `execute()`,
    Q1 doesn't cache it across renders either, and caching introduces
    invalidation correctness obligations the protocol cannot enforce
    on opaque engine state (`data` cookies, transient notebooks). If a
    future engine genuinely needs target() memoization, it can implement
    the cache itself in module scope where it has full visibility into
    what the cookie depends on.

- [ ] **Cooperative, per-request cancellation (`Cancel { target }`).** Under
    parallel Pass-2 the subprocess is shared, so a per-request timeout/cancel
    **cannot** SIGKILL it (that would kill sibling documents mid-`Execute`).
    Instead, q2 sends `Cancel { target: id }`; the harness aborts *that request's*
    `AbortController` and passes the `AbortSignal` into the engine's `execute`
    (engines that honor it can short-circuit; engines that don't simply ignore
    it). The request resolves as `Cancelled`. Other in-flight requests — on the
    same or other engines — are untouched.
    - **Daemon ambiguity is q2's problem, not the harness's.** If aborting leaves
      an engine's daemon mid-computation, plan1a-host **poisons that engine
      instance and lazily relaunches it** on next use (the daemon is detached and
      outlives the harness via transport files). The blast radius is one engine
      instance, never the subprocess.
    - **SIGKILL is reserved** for genuine everyone-affecting failures — subprocess
      crash, a compromised/unparseable control channel, and teardown — and is
      issued from the Rust side (plan1a-host). The harness still installs **no
      SIGINT handler**; cancellation arrives as a protocol `Cancel`, not a signal.
    - This **reverses** an earlier "no cooperative cancel; SIGKILL is the honest
      path" decision, which held only while SIGKILL coincided with "q2 is exiting
      anyway" — broken by parallel Pass-2. Full rationale:
      `claude-notes/designs/engine-host-concurrency.md`.

### Engine API contract

The harness exposes the QuartoAPI to engines via a state-machine pattern,
not a Q1-style global. The contract:

- **Access channel: `init(quartoAPI)` is the only way an engine receives
  the API.** Engines that use `quarto.*` implement the optional
  `init?: (quarto: QuartoAPI) => void` on their discovery surface
  (matching Q1's `ExecutionEngineDiscovery.init`) and stash the
  reference for use in their own methods. Engines that don't need the
  API (echo engines, trivial markdown-only engines) skip `init()`.

- **No global `getQuartoAPI()` registry.** Q1's
  `QuartoAPIRegistry`/`register.ts` is deliberately not ported. The
  reference passed to `init()` is the engine's only handle.

- **Module top-level access is forbidden.** Top-level engine module
  code runs during `import()`, before `init()` is called. Engines
  must not access `quarto.*` at module scope — only from inside
  methods (`init`, `claimsLanguage`, `claimsFile`, `launch`,
  `execute`, etc.).

- **`init()` is sync.** Returns `void` per Q1's contract. Async setup
  belongs in `launch()`. The harness nevertheless `await`s the result
  defensively — engines that mistakenly write `async init()` get
  correct-enough behavior rather than a silent unawaited Promise.
  Synchronous or asynchronous, throwing/rejecting is a fatal load
  failure: the harness sends `error` for the `loadEngine` message and
  the engine never enters the registry.

- **State-machine API surface.** The QuartoAPI handed to `init()` is
  built once at `loadEngine` time, parameterized over a shared mutable
  `HostState` whose `context` field is unset until the first
  `launchEngine`. Object identity is stable — engines stash the
  reference and it remains valid for their lifetime; what changes is
  that gated methods unblock when context arrives.

  | Available immediately after `init()` | Gated until `launchEngine` |
  |---|---|
  | `quarto.text.*` (pure) | `quarto.path.runtime` |
  | `quarto.markdownRegex.*` (pure) | `quarto.path.resource` |
  | `quarto.console.*` (pure) | `quarto.system.pandoc` |
  | `quarto.crypto.*` (pure) | `quarto.format.*` *when called without an explicit format argument* |
  | `quarto.mappedString.*` (host-only — uses `denoHost.fs`) | |
  | `quarto.path.{dirAndStem,isQmdFile,toForwardSlashes,absolute,inputFilesDir}` (pure or host-only) | |
  | `quarto.system.{execProcess,tempContext,onCleanup,isInteractiveSession,runningInCI}` (host-only) | |
  | `quarto.jupyter.*` conversion logic (mostly pure; figure writes go through host) | |

- **Single shared state across all engines.** All engines loaded into
  the subprocess share one `HostState` object. The first
  `launchEngine` sets `state.context`; subsequent launches assert it
  hasn't changed (in dev) and otherwise leave it. This encodes the
  practical invariant that all engines in a project render share the
  same context. Per-engine state is an open extension point if a
  future engine genuinely needs different context.

- **Classification ownership.** The pure / host-only / gated split in
  the table above is a *contract*, not a 1b-local convention. It is
  recorded as jsdoc on the `QuartoAPI` type in `@quarto/types` (Plan 2A
  / refined in 2E), so the harness's gating and the namespace
  implementations in `@quarto/api` agree by construction. When Plans
  2/3 add a method, they classify it there; 1b's gating reads that
  classification rather than re-deciding it.

- **Error message contract.** Gated methods throw a clear,
  method-named error before launch. No prescriptive remedies in the
  message — name the method, state the constraint, let the stack
  trace and the engine author do the rest. The exception is
  `quarto.format.*` where the API itself offers an alternative (pass
  an explicit format argument), so its error message names that
  remedy:
  ```
  Error: quarto.path.runtime is unavailable before engine launch.
  Error: quarto.format.isHtmlCompatible: no format argument provided
         and no default is available before engine launch. Pass an
         explicit format argument.
  ```

#### Tests for the Engine API contract

- [ ] Test: `init()` is called once when the harness handles
    `loadEngine` for an engine that exports it. A test engine records
    each call; verify a single invocation per loadEngine message.
- [ ] Test: engine that does not export `init` loads cleanly (no
    error), and any subsequent `claimsLanguage`/`claimsFile` calls
    succeed.
- [ ] Test: pure namespaces are callable from inside `init()`. A test
    engine calls `quarto.text.lines("a\nb")` and `quarto.markdownRegex`
    methods inside `init()`; verify no throw.
- [ ] Test: gated method called before `launchEngine` throws a
    method-named error. For each of `quarto.path.runtime`,
    `quarto.path.resource`, `quarto.system.pandoc`, and
    `quarto.format.isHtmlCompatible()` (no args), verify the thrown
    `Error.message` names that method and mentions "before engine
    launch".
- [ ] Test: `quarto.format.isHtmlCompatible(format)` with an explicit
    format works before launch (the gating only applies to the
    default-from-context path).
- [ ] Test: gated methods unblock after `launchEngine`. After sending
    `launchEngine` for any engine, the gate is lifted.
    **§2aa→1b sequencing caveat:** at 1b's landing, the *real bodies* of
    `path.runtime`/`resource`/`dataDir` and `system.pandoc` are still
    deferred to Plan 2 (§2aa ships them as stubs), so post-launch they no
    longer throw the *gating* error but may still throw their stub error
    until Plan 2 lands. Therefore assert the **returns-a-real-value**
    behavior with `quarto.format.isHtmlCompatible()` (real in §2aa), and for
    the deferred-body methods assert only that the *gating* error is gone
    (the stub error, if any, is distinct and acceptable). Once Plan 2
    Phase A lands the real bodies, extend this test to assert real values
    for them too.
- [ ] Test: state is shared — when engine A is launched first, engine
    B's stashed `quartoAPI` reference (handed to its `init()` before
    A's launch) also sees the gate lifted (asserted via
    `format.isHtmlCompatible()`, per the caveat above).
- [ ] Test: `init()` throwing is a load failure. The harness sends
    an `error` response for the `loadEngine` message and does not
    register the discovery surface.
- [ ] Test: `async init()` is awaited defensively. A test engine
    declares `async init()` and resolves after a tick; verify that
    `loaded` is sent only after the resolution, and any error from
    the rejection is reported.
- [ ] Test: module top-level code that accesses `quarto.*` fails (no
    global available). Verify the failure mode is a clean load error,
    not silent corruption.

- [ ] **Test: harness idempotency contract** (plan1a-engine relies on this
    so its Rust-side init can use naive `OnceLock`). Two `loadEngine`
    messages for the same engine name with the same `enginePath`:
    `import()` runs once, `engine.init?.()` is called once, both
    responses carry identical `LoadEngineResult`. Two `launchEngine`
    messages for the same already-loaded engine: `engine.launch()` is
    invoked exactly once, both responses carry identical
    `LaunchEngineResult`. Use a test engine that increments per-call
    counters and exposes them via `quarto.console.error` for stderr
    capture. This is **the** contract test that backs plan1a-engine's
    "Race-free init via harness idempotency" reasoning; if it
    regresses, plan1a-engine's `OnceLock`-based approach becomes unsound.

- [ ] **Test: harness idempotency under *concurrent* repeat** (this is the
    race plan1a-engine's benign-`OnceLock` argument actually rests on — up to
    N rayon workers fire the same `loadEngine`/`launchEngine` *concurrently*,
    not sequentially). *Tier:* integration (vitest) against the real dispatch
    loop over the in-memory framed duplex (same harness as T3/T5). *Seam:*
    enqueue K (≥3) `loadEngine` frames for the same engine name + same
    `enginePath` *before* the first resolves (a test engine whose `import()`
    side and `init()` block on a controllable deferred and increment per-call
    counters); then, after `launchEngine`, enqueue K `launchEngine` frames for
    that engine the same way. Assert: `import()` runs **exactly once**,
    `engine.init?.()` is called **exactly once**, `engine.launch()` is invoked
    **exactly once**, all K `loaded` responses carry an identical
    `LoadEngineResult`, and all K `launched` responses carry an identical
    `LaunchEngineResult`. *Mock boundary:* the engine's deferreds + per-call
    counters; the loop + per-engine record are real. *Named reverts:* ▸ make
    the `loadEngine` arm re-run `import()` whenever called (drop the
    cache-hit short-circuit on the `Map<engineName, …>` entry) → the
    "`import()` exactly once" assertion RED. ▸ make the `launchEngine` arm
    re-run `engine.launch(context)` whenever called (drop the cached-`instance`
    short-circuit) → the "`engine.launch()` exactly once" assertion RED. This
    is the assertion plan1a-engine names as **"engine.launch() invoked exactly
    once across the real harness" — Plan 1b's contract** (plan1a-engine
    "Race-free init"); the sequential idempotency test above does not exercise
    the concurrent in-flight window, which is the one the N-worker fan-out hits.

- [ ] **Test: harness `htmlDependency()` accumulator.** A test engine
    calls `quarto.htmlDependency({ name, stylesheets, scripts })`
    inside `execute()`; verify the registration appears in the response's
    `htmlDependencies` array, with relative paths resolved against
    `lib_dir`. Calling outside `execute()` (e.g., inside `init()` or
    `claimsLanguage`) throws a method-named error.

### Phase 3: Supporting modules

- [ ] Create `src/deno-host.ts` — the `PlatformHost` implementation used by
    `@quarto/api` factory exports. **`PlatformHost` is a q2-original
    abstraction, not a Q1 port** — Q1 has no host-injection seam (it hardwires
    `Deno.*` inside each namespace's backing functions). The `./platform`
    subpath and the `PlatformHost` interface are defined by Plan 2A §2aa (see
    #2 in the review); this is the seam that later lets a
    `@quarto/engine-host-wasm` supply a VFS-backed host without touching
    `@quarto/api`. **`denoHost` must implement the full landed interface**
    (`ts-packages/quarto-api/src/platform/index.ts`) — including
    `fs.makeTempFile`/`makeTempDir`, structured `process.exec`
    (`ExecOptions`/`ExecResult`), and `env.get` — not just the subset sketched
    below.
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

- [ ] Create `src/quarto-api.ts` — state-machine QuartoAPI builder:
  - Define `HostState = { context?: EngineHostContext }`.
  - Export `buildQuartoAPI(state: HostState, host: PlatformHost): QuartoAPI`
    that returns a plain nested record (no registry pattern — Q1's
    `QuartoAPIRegistry`/`register.ts` is deliberately not ported).
  - The builder constructs every namespace closure to *read* `state.context`
    at call time. Pure and host-only namespaces work whenever called.
    Gated methods (see "Engine API contract" above) check `state.context`
    and throw a clear method-named error if unset.
  - The harness creates one `HostState` and one QuartoAPI at startup,
    hands the QuartoAPI to each engine via `engine.init?.(quartoAPI)`
    on its `loadEngine`, and sets `state.context` on the first
    `launchEngine`.
  - **What's real at 1b vs. deferred.** 1b's contract tests exercise
    concrete behavior, so the namespaces they touch must be *real* (not
    throwing stubs) — Plan 2A §2aa delivers them (see #6 in the review):
    the pure namespaces `text`, `markdownRegex`, `format`, `crypto`, and
    the host-only namespaces `console`, `path`, `system`, `mappedString`
    (the host-only ones get their IO through the injected `PlatformHost`,
    not direct `Deno.*`). Only `jupyter` and the launch-context-dependent
    method *bodies* may remain `throw "not yet implemented"` until Plans
    2/3. The state-machine *wiring* (gating on `state.context`) is what
    this plan delivers; the pure/host-only namespace bodies come from
    `@quarto/api` (Plan 2A §2aa).
  - **`text` and `mappedString` are separate top-level namespaces** in
    Q1's `QuartoAPI` (`core/api/types.ts:236`: `text: TextNamespace`,
    `mappedString: MappedStringNamespace`) — `mappedString` is NOT a
    subset of `text`. Earlier drafts said both "pull from
    `@quarto/api/text`"; that's wrong. `text` has `lines`,
    `trimEmptyLines`, `lineColToIndex`, …; `mappedString` has
    `fromString`, `fromFile`, `splitLines`, `indexToLineCol`,
    `normalizeNewlines`. `mappedString.fromFile` is the one host-backed
    method (reads disk via the `PlatformHost`); the rest are pure.

- [ ] **Add `quarto.htmlDependency()` engine-author helper.** Q2-native
    structured-deps registration API, the engine-side counterpart to
    `TsExecuteResult.htmlDependencies`. Signature:
    ```typescript
    quarto.htmlDependency({
      name: string,
      stylesheets?: string[],   // absolute or relative paths to CSS files
      scripts?: string[],       // absolute or relative paths to JS files
    }): void;
    ```
    Behavior:
    - Implementation captures the registration into the per-`Execute`
      accumulator on `HostState` (cleared at the start of each `Execute`
      message dispatch, drained when assembling the response).
    - Relative paths in `stylesheets` / `scripts` are normalized against
      `TsExecuteOptions.lib_dir` before the harness emits them on the
      wire (plan1a-protocol's path contract: absolute, on-disk).
    - Calling outside an `execute()` context (e.g., during `init()` or
      `claimsLanguage`) is a programming error; throw with a clear
      method-named message. This is consistent with the gated-namespace
      pattern.
    - plan1a-engine's q2-side `store_html_dependencies` dedupes by `name`
      (first-wins) with a `DiagnosticMessage::warning` on duplicate, so
      engine authors can rely on idempotent registration without tracking
      what's already been added by other engines.
    - This helper is the only sanctioned route to populate
      `executeResult.htmlDependencies`. Q1's `engine.dependencies(...)`
      output goes to `executeResult.includes` (see Phase 2's
      execute-dispatch flow); the harness MUST NOT auto-translate Q1
      deps into `htmlDependencies`.

- [ ] Create `src/mapped-source.ts` — MappedString rehydration from
    `TsSourceMapEntry[]`. This is the q2-specific piece (not in `@quarto/api`
    itself) because the `source_map` crosses the protocol boundary as data
    rather than in-memory references.

  **Wire shape.** Each `TsSourceMapEntry` is
  `{ start, length, source: Option<TsSourcePosition> }`, where
  `TsSourcePosition = { file, file_offset }` (plan1a-protocol). The
  `source: Option<…>` indirection is load-bearing: `source: None` is how an
  *unmappable* piece is encoded on the wire. (Earlier drafts here described a
  flat `(start, length, file, file_offset)` tuple — that is wrong and can't
  represent the unmappable sentinel; align to the nested `Option` shape.)

  **Rust side flattens before sending:**
    - `Original { file_id, start_offset, end_offset }` → resolve FileId to path via
      SourceContext; emit `source: Some({ file, file_offset })`.
    - `Substring` → walk parent chain to Original, compute absolute file offset.
    - `Generated { by, from }` → if an anchor with role `AnchorRole::Invocation`
      is present (an `Anchor { role: AnchorRole::Invocation, source_info }`, not a
      standalone `Invocation` type), walk its `source_info` to the underlying
      `Original`/`Substring` source and emit a mappable entry; pure-synthesis
      `Generated` (empty `from`) emits `source: None` (unmappable). (The May 2026
      source-provenance overhaul replaced the old `FilterProvenance` case with the
      richer `Generated` variant — see plan1a-protocol's flattening appendix and
      the `SourceInfo` enum in `crates/quarto-source-map/src/source_info.rs`.)
    - Nested `Concat` → flatten recursively
    File IDs are resolved to path strings on the Rust side since the Deno
    process doesn't have SourceContext.

  **Deno-side reconstruction:**
    1. For each unique file in `source_map`, lazily read the file via
       `denoHost.fs.readTextFileSync` and create a base `MappedString` with
       `.fileName` set. **Tolerate `ENOENT` (and other I/O errors)
       gracefully** — files may not exist on disk (in-memory-only
       sources registered via `SourceContext::add_file(path, Some(content))`,
       deleted-since-execution files). On read failure, treat the
       affected piece as unmappable (same behavior as the empty-file
       sentinel below): `.map(index)` returns the synthetic position,
       no exception thrown. Log the read failure once per file at
       `[INFO]` via stderr; engines should not see surprises from the
       protocol layer. The base-per-file cache is **scoped to a single
       rehydration call** — built when rehydration starts, used to share
       `MappedString` identity among pieces of that one source map, then
       discarded. No cross-message caching: files may have changed between
       Execute messages and a longer-lived cache would mask that.
    2. The main MappedString's `.map(index)` binary-searches the sorted
       entries to find which piece contains the index, computes the offset
       in the original file (`piece.fileOffset + (index - piece.start)`),
       and returns `{ index: offset, originalString: baseForFile }`.
    3. For `closest=true` on an unmappable range (`source: None`
       OR file that failed to read), scan to the nearest entry with a
       valid file mapping.
    4. `splitLines` and `indexToLineCol` are pure TS utilities that
       operate on this MappedString — no special protocol support needed.
    5. This gives character-level accuracy — engines like Julia that
       call `line.map(0, true)` in `buildSourceRanges()`
       (`julia-engine.ts:644`) get correct original file + position, even
       through include boundaries.

    **One MappedString type, not two.** The object `mapped-source.ts`
    produces MUST be the same `MappedString` type from `@quarto/types`
    that `quarto.mappedString.*` produces and that engines call `.map()`
    on — not a parallel look-alike. `mapped-source.ts` is only a
    *constructor* (it builds a `MappedString` from `TsSourceMapEntry[]`
    instead of from a string/file); the resulting object satisfies the
    same interface, so an engine that receives a rehydrated MappedString
    (e.g. from `markdownForFile` or the `execute` input) cannot tell it
    apart from one built by `quarto.mappedString.fromString`.

  **Path conventions on the wire** (plan1a-protocol appendix, "Path conventions on
  the wire"). All `file` strings in `TsSourceMapEntry` are absolute and
  lexically normalized (no symlink resolution). The harness can rely on
  string equality between two paths referring to the same file; no
  re-canonicalization needed on receipt.

- [ ] **MappedString serialization for `markdownForFile` (Deno → Rust):**
    When an engine converts a non-QMD file to QMD via `markdownForFile`, the
    result is a `MappedString` with provenance back to the original file.
    The harness serializes this mapping by walking the `MappedString`'s
    underlying piece structure and emitting **one `TsSourceMapEntry` per
    piece** — same shape and same rule as the Rust side's flattening of
    `SourceInfo::Concat::pieces` (plan1a-protocol appendix, "Source-map
    flattening"). Each piece becomes one entry
    `{ start, length, source: Some({ file, file_offset }) }` derived from the
    piece's offset in the output and its mapped position in the source; a
    piece with no source preimage serializes as `source: None`. **No
    coalescing** (contiguous entries are not merged — the boundaries are the
    engine's meaningful provenance structure).

    **v1: this serialization is forward-wiring; the Rust side does NOT consume
    it (plan1a-engine SEAM-3, "C′").** plan1a-engine scopes `markdown_for_file`
    to C′: the converted text gets provenance into an ephemeral synthetic
    buffer, and the **`source_map` rides the wire UNCONSUMED** — there is **no
    Rust-side `SourceInfo::Concat` reconstruction in v1**. Keep this TS-side
    serializer (it's cheap and is exactly the input the future "A′" remap
    needs), but the engine is **not required to produce a *faithful* map in
    v1** — it serializes whatever its `MappedString` carries (the echo engine
    returns `sourceMap: []`). The Rust-side reconstruction that turns these
    entries into `SourceInfo::Concat` on the AST is **A′ / deferred**, not a
    Plan 1b v1 deliverable.

- [ ] Create `src/engine-loader.ts`:
  - Dynamically import the engine module: `await import(toFileUrl(path))`
  - Validate it has a default export with `name`, `claimsLanguage`, `launch`
  - Return the `ExecutionEngineDiscovery` object

- [ ] Create `src/framing.ts` — the v1 channel plumbing `host.ts` imports
    (stdin/stdout):
  - `readFrames(stream): AsyncIterable<Request>` — newline-framed JSON decoder
    yielding parsed `Request` envelopes (`{ id, msg }`) from `Deno.stdin`.
  - `writeFrame(out, response)` + an `AsyncMutex` so concurrent dispatch tasks
    never interleave bytes of two frames on `Deno.stdout`.
  - **Single-threaded-atomic writes:** each frame is written with one
    `writeSync` of a complete line; Deno's single thread can't interleave two
    `writeSync`s, so frames never corrupt each other (a stray engine
    `console.log` is a *separate* complete line, caught as malformed — the v1
    stdout contract).
  - **(Phase 1.6, deferred)** a `connectControl(args)` that parses
    `--control <addr> --token <nonce>`, dials `std::net`-bound **loopback TCP**
    (`Deno.connect({ hostname: "127.0.0.1", port })`), and presents the token in
    frame 1 — replacing stdin/stdout as the channel and freeing stdout for
    diagnostics. **Not UDS / named pipe** (see plan1a-host "Deferred: Phase 1.6").

- [ ] Create `src/types.ts` — protocol message type definitions (must match
    the Rust enums in plan1a-protocol exactly), **including the Phase 1.5
    envelope** (`Request = { id: number, msg: ToEngine }`,
    `Response = { id: number, msg: FromEngine }`) and the `Cancel { target }` /
    `Cancelled` control messages.

### Phase 4: Bundle + CI

- [ ] **Add `cargo xtask build-engine-host-bundle`** (esbuild) that regenerates
    `dist/engine-host-deno.js` from the TS sources, and **wire it into
    `cargo xtask build-all`** (ordered before the Rust build, like the MCP/SPA
    bundle steps). Build the bundle and check the result into git, replacing the
    placeholder plan1a-host committed at the same path. **Framing (per plan1a-host's
    reworked "Bundle embedding"):** the harness is a **generated build artifact**
    embedded via `include_str!`, treated **exactly like `q2 mcp`'s `dist-bundle/`
    and the preview SPA's `dist/`** — it is **NOT** the `resources/…`-*source*
    pattern (reveal.js/clipboard/bootstrap). (The earlier "bundle is source like
    `reveal.js`" framing was wrong; the cited `quarto-system-runtime`/`ejs-bundle.js`
    precedent does not exist.) The committed bundle (+ Plan 1a's placeholder) is
    what keeps plain `cargo build` working on a fresh checkout with no npm/esbuild
    having run; `@quarto/engine-host-deno` is **not published** to a registry —
    it ships only embedded in the binary.
- [ ] **Staleness diagnostic.** Mirror `q2 mcp --launcher-info`: surface the
    embedded bundle's **git commit, dirty flag, and build time** (e.g. baked in by
    the xtask) via a `--launcher-info`-style flag, so a `cargo build --bin q2`
    that re-embedded a stale on-disk bundle is detectable. (A plain `cargo build`
    re-embeds whatever bytes are on disk — same trap as the MCP/SPA bundles.)
- [ ] Add a CI check that verifies the checked-in bundle is up to date with the
    sources. **Do not byte-compare a fresh build against the committed file** —
    esbuild output is not guaranteed byte-stable across versions/platforms/line
    endings, so naive byte-equality is flaky. Instead: (1) **pin the exact esbuild
    version** in `devDependencies` (no `^`/`~`), and (2) have CI run the xtask then
    `git diff --exit-code -- ts-packages/quarto-engine-host-deno/dist/`, failing if
    the rebuild changed the committed bytes. The pin makes the rebuild
    deterministic enough for the diff to be a true "you edited TS but forgot to
    rebuild" signal rather than cross-environment noise.

## Design Notes

### Diagnostic stream (stderr) — v1

In v1 the protocol runs on stdout, so **stderr is the diagnostic stream**
(forwarded to q2's logging). The harness prefixes its own log lines with level
markers so q2 can route them:
```
[INFO] Checking Julia installation...
[WARN] Julia server connection slow
[ERROR] Julia process crashed
```
Unprefixed stderr lines (from the engine or Deno) are logged at INFO. (Phase 1.6
moves the protocol to loopback TCP and frees stdout for diagnostics too.)

### Stdout/stderr contract (v1)

**In v1 the protocol runs on stdout**, one `Response` per line, so the existing
contract holds: engines must **not** write to stdout. The harness captures the
real `Deno.stdout` for protocol writes at startup and does **not** override
`console.*` — protection is by contract.

- **Use `quarto.console.*`** (preferred — `[INFO]`/`[WARN]`/`[ERROR]` prefixes,
  written to stderr) or `console.error`/`console.warn` (stderr) for diagnostics.
- **Do not use `console.log`/`console.info` or `Deno.stdout.writeSync`** — these
  write to stdout and corrupt the protocol. The Rust side detects a non-`Response`
  line on stdout as malformed and SIGKILLs (plan1a-host category 9), naming
  `console.log` as the likely cause.
- **Do not read `Deno.stdin`** — it is the protocol *input* channel; an engine
  reading it (e.g. an interactive prompt) steals frames the harness's read loop
  needs. The symmetric footgun to the stdout one.

**Phase 1.6 retires this contract**: moving the protocol to loopback TCP frees
both stdin and stdout for the engine, after which `console.log` is a harmless
INFO line and the capture/contract disappear.

### Where is engine-host-deno.js at runtime?

The engine-host-deno harness is bundled into a single `.js` file using **esbuild**.

**Build pipeline:**
1. `ts-packages/quarto-engine-host-deno/esbuild.config.mjs` bundles `src/host.ts` → `dist/engine-host-deno.js`
2. The bundle is checked into git (the same commit-and-embed pattern as `crates/quarto-core`'s `resources/revealjs/reveal.js`). plan1a-host ships the path with placeholder content so `include_str!` compiles cleanly; Plan 1b replaces the placeholder with the real esbuild output. The bundle is treated as a versioned source artifact — q2's Rust crate `include_str!`-embeds it, so from cargo's perspective it's just another input file like any `.rs` source.
3. `include_str!("../../../../ts-packages/quarto-engine-host-deno/dist/engine-host-deno.js")` embeds it in the q2 binary (path is relative to `crates/quarto-core/src/engine/ts_process.rs` — four `..`s reach the repo root)
4. At runtime, write the embedded string to a temp file, run `deno run --allow-all <tempfile>`

**Subprocess lifetime.** The bundle is loaded once per `TsEngineHost`
instance, and that host is project-scoped — one Deno subprocess covers
both Pass 1 and Pass 2 of a project render and every per-file
`StageContext`. Spawn is lazy (deferred until the first protocol
round-trip). See Plan 1c "Move `EngineRegistry` and `Arc<TsEngineHost>`
to `ProjectContext`" for ownership details and the grand plan's
"Project-render integration" for the lifecycle picture.

**Forward-looking bundle composition.** Once Plan 3 lands `jupyter`, the
engine-host-deno bundle includes `@quarto/api` (all subpaths — `text`,
`markdownRegex`, `mappedString`, `jupyter`, `format`, `path`, `system`,
`console`, `crypto` — Q1's nine namespaces) and the harness glue (host,
deno-host, quarto-api, mapped-source, engine-loader) — a single
self-contained `.js` file. Only developers editing the TS harness or
`@quarto/api` code need to rebuild it.

At Plan 1b's landing the pure + host-only namespaces are already real
(Plan 2A §2aa); only `jupyter` is still a stub, so the initial bundle is
somewhat smaller than the steady-state size below.

**Bundle size note (post-Plan-3):** The bundle may be large (200-500 KB
estimated, depending on `@quarto/api/jupyter` complexity). The
engine-host-deno bundle is gated behind
`#[cfg(not(target_arch = "wasm32"))]` so WASM builds don't carry it.
Flagged as a possible future concern — if the bundle grows problematically,
options include a cargo feature flag to gate the embed, or loading from a
known filesystem path instead of embedding. For now, embedding is the
simplest approach and matches q2's existing commit-and-`include_str!`
practice for browser-side JS (`reveal.js`).

### Why a separate plan from 1a?

The Rust-side infrastructure (plan1a-protocol/host/engine) and the Deno-side harness (this plan)
are independent once the protocol schema is frozen. Splitting them makes
The plan1a sub-plans focus on the Rust compile-time / trait / subprocess-management
concerns, while this plan focuses on a TypeScript package with its own build
pipeline, esbuild config, and test setup. They can be worked on in parallel
if two people are available, and the separation naturally reflects the
`@quarto/engine-host-deno` / `@quarto/engine-host-wasm` split that the
`PlatformHost` abstraction enables later.

### Out of scope: engine-author documentation

User-facing documentation for engine extension authors is **required
follow-up but not part of these plans**. Specifically, a future
documentation effort (target: `docs/engine-extensions.md` or equivalent)
needs to cover:

- The state-machine QuartoAPI contract: which methods are pure, which
  are host-only, which are gated behind `launchEngine`, and the
  `init(quartoAPI)` reference-stashing pattern.
- Module top-level access prohibition (no `quarto.*` outside methods).
- Diagnostics: use `quarto.console.*` (level-routed) or `console.error`/`.warn`
  (stderr). **In v1 do not use `console.log`/`console.info`** — they write to
  stdout, the protocol channel, and corrupt it. (Phase 1.6 moves the protocol
  off stdout and makes `console.log` harmless.)
- Cooperative cancellation: a per-request `Cancel` aborts an `AbortSignal` the
  engine's `execute` may honor; daemon-backed engines should still be
  daemon-detached-by-default (the Q1 transport-file pattern) so a poisoned
  instance's daemon survives and is re-discovered on relaunch.
- Engines that spawn long-lived subprocesses should follow the
  daemon-detached pattern rather than expecting the harness lifecycle
  to manage them.
- Build model: extensions ship a pre-built `.js` bundle produced by
  `q2 build-ts-extension`; q2 never auto-builds during render
  (Plan 1c).

These plans (1a/1b/1c) deliver the implementation; the user-facing
documentation is intentionally deferred.

## Resolved partition decisions (both: match Q1)

Two Stage-2 rules once claimed Q1 parity but diverged from Q1's actual
`metadataAsFormat` (`config/metadata.ts:200-210`). Both originated as a
deliberate "be faithful to all of Q1" edit (git `24b8c1ab4`) built on an
imprecise model of Q1. Both are now **resolved to match Q1's real behavior**;
this record exists so the divergence is not silently re-introduced by a future
"be faithful to all five lists" pass.

1. **No flat-key `language` branch.** Q1's `metadataAsFormat` classifies flat
   keys against only four arrays (identifier → render → execute → pandoc →
   else `metadata`) and never consults `kLanguageDefaultsKeys`; `format.language`
   is filled solely by the Stage-1 nested `language:` peel. q2's own Rust
   `format.rs` has no `language` bin and no engine reads `format.language`.
   **Decision:** Stage 2 is a four-list classification; flat language-ish keys
   fall to `format.metadata`; `format.language` comes only from the nested
   peel. (`@quarto/api/config` still *transcribes* `kLanguageDefaultsKeys` for
   parity-completeness against `constants.ts`, but the partition does not
   consult it — annotated as such in Plan 2A.)

2. **Move, don't duplicate.** Q1's chain is `if/else-if/else`, so each flat key
   lands in **exactly one** bin — partitioned keys are *moved*, not copied into
   `format.metadata`. **Decision:** move, not duplicate. A key needed in two
   bins is an explicit, engine-named q2 exception if it ever arises — not a
   blanket mirror.

## Success Criteria

- [ ] `@quarto/engine-host-deno` package exists with package.json,
  esbuild.config.mjs, tsconfig
- [ ] Harness dispatches every protocol message type from plan1a-protocol's `ToEngine`
  enum (`LoadEngine`, `LaunchEngine`, `Shutdown`, `ClaimsLanguage`,
  `ClaimsFile`, `MarkdownForFile`, `Execute`, `IntermediateFiles`)
- [ ] Two-step lifecycle works: `LoadEngine` produces a discovery surface
  without launching; `LaunchEngine` produces an instance; messages requiring
  the wrong state return a clear error
- [ ] `target()` handled as harness-internal (never reaches the protocol)
- [ ] `dependencies()` folded into `execute`: harness calls it when
  `engineDependencies` is present, materializes files, returns q2-shaped
  `htmlDependencies` on `executeResult`
- [ ] `partitionedMarkdown` is **not** dispatched by the harness — it is
  not a protocol message in q2 (`DocumentProfile` covers the Q1 use
  cases; see grand plan and ipynb-filters research plan)
- [ ] MappedString rehydration from `source_map` works end-to-end — a
  `.map(index)` call returns `{ index, originalString }` pointing at the
  correct file and offset even through include boundaries; the
  base-per-file cache is per-rehydration-call (not cross-message)
- [ ] MappedString serialization for `markdownForFile` responses is
  implemented on the TS side (one-entry-per-piece, no coalescing); the
  `source_map` rides the wire **unconsumed in v1** — Rust-side
  `SourceInfo::Concat` reconstruction is A′-deferred (plan1a-engine SEAM-3 / C′)
- [ ] Multiplexed dispatch: non-blocking read loop over stdin/stdout (v1);
  responses echo the request `id`; cross-engine requests run concurrently while
  same-engine requests serialize on a per-engine queue (T3, T5)
- [ ] Cooperative cancellation: `Cancel { target }` aborts exactly that request's
  `AbortController` (resolving it `Cancelled`) without affecting siblings; no
  SIGINT handler; whole-subprocess SIGKILL stays Rust-side and reserved for
  crash/compromised-channel/teardown (T6)
- [ ] Concurrent same-instance poison → **transparent re-launch**: a queued
  same-engine `Execute` whose instance was poisoned reconstructs the instance
  (`engine.launch(stashedContext)`) and completes normally — it is not failed (T7)
- [ ] `target()` is called fresh per `Execute` message; results are not
  memoized
- [ ] State-machine QuartoAPI: `init?(quarto)` is called at `loadEngine`;
  pure and host-only methods work immediately; gated methods
  (`path.runtime`, `path.resource`, `system.pandoc`, `format.*` without
  explicit format) throw a clear method-named error before
  `launchEngine`; `state.context` set on first `launchEngine` unblocks
  the gated set for all loaded engines
- [ ] Protocol runs on stdin/stdout (v1); harness captures `Deno.stdout` for
  protocol writes; stderr is the diagnostic stream; the stdout contract holds
  (`console.log` forbidden — corrupts the protocol). (Phase 1.6 moves to loopback
  TCP and retires the contract.)
- [ ] Lifecycle methods that are NOT on the protocol (`filterFormat`,
  `executeTargetSkipped`, `postprocess`, `canKeepSource`, `postRender`)
  are simply not dispatched — the harness has no top-level handler for them
- [ ] `denoHost: PlatformHost` in place (importing the q2-original
  `PlatformHost` from `@quarto/api/platform`, Plan 2A §2aa); `quarto-api.ts`
  builds the state-machine QuartoAPI over the nine Q1 namespaces. The
  pure + host-only namespaces 1b's tests exercise (`text`,
  `markdownRegex`, `format`, `crypto`, `console`, `path`, `system`,
  `mappedString`) are real (from `@quarto/api`, Plan 2A §2aa); only `jupyter`
  and launch-context method bodies may throw "not yet implemented"
  pending Plans 2/3
- [ ] Bundle builds cleanly with `npm run build`, produces
  `dist/engine-host-deno.js`, and the rebuilt bundle is committed to
  git (replacing plan1a-host's placeholder)
- [ ] CI check verifies the checked-in bundle matches what
  `npm run build` produces — catches "edited TS but didn't rebuild"
  drift
- [ ] Harness idempotency contract test passes (`loadEngine` /
  `launchEngine` repeats are no-ops; backs plan1a-engine's naive-OnceLock
  reasoning)
- [ ] Concurrent idempotency holds: K (≥3) same-name `loadEngine` frames
  in flight at once run `import()`/`init()` exactly once with identical
  `LoadEngineResult`, and K same-engine `launchEngine` frames in flight run
  `engine.launch()` exactly once with identical `LaunchEngineResult` — this is
  the "engine.launch() invoked exactly once across the real harness" assertion
  plan1a-engine defers to Plan 1b for its parallel-Pass-2 `OnceLock` fan-out
- [ ] `quarto.htmlDependency()` engine-author helper accumulates
  registrations during `execute()` and emits them as
  `executeResult.htmlDependencies`; relative paths normalize against
  `lib_dir`
- [ ] Phase 0 Test Seam Spec tests pass and bind: T1 (metadata
  partition, incl. the `pdf-standard` order discriminator and the
  nested-bin peel), T2 (MappedString rehydration offset + `source: None`
  tolerance + closest scan), T3 (per-message-type dispatch + `id`
  correlation + the state-guard negative path), T4 (`markdownForFile`
  no-coalescing TS serialize only — Rust `SourceInfo::Concat` reconstruct is
  A′-deferred per plan1a-engine SEAM-3),
  T5 (cross-engine concurrency + same-engine serialization), T6 (cooperative
  `Cancel` aborts only the target). Each has a named revert that reddens its
  asserted surface.
