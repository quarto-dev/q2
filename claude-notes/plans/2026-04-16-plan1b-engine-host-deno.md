# Plan 1b: @quarto/engine-host-deno (Deno harness)

**Grand plan:** [2026-04-16-ts-engine-extensions-subprocess.md](2026-04-16-ts-engine-extensions-subprocess.md)
**Depends on:** plan1a-protocol (Rust core: protocol types), **RTQ
(`plan1a-return-to-q1`)**, AND Plan 2A (`@quarto/api` foundation — the
`@quarto/api/config` subpath this plan imports). This plan needs the
metadata-partition key lists from Plan 2A and the **post-RTQ** protocol
schema.

> **⚠ RTQ is a hard predecessor for 1b's Rust-facing surface.** 1b's body is
> written against the *post-RTQ* wire, but as of this writing **RTQ has not
> executed any code** (every RTQ code box is `- [ ]`). The frozen Phase-1/1.5
> schema in `ts_protocol.rs` today is the *pre-RTQ* shape and is **missing
> everything 1b's Rust-facing tests assume**: the `Dependencies` verb,
> `engineDependencies` + `dependencies: bool` on the execute types, the
> `Init { global }` / `HostGlobalConfig` message, the `EngineProjectContext`
> launch shape (today it is the older `EngineHostContext`), the ENG-1
> discovery-tier statics on `LoadEngineResult` (`generates_figures` /
> `can_freeze` / `quarto_required`), and the FC-1 inert carrier fields on
> `TsExecuteResult`. The B3 stub relabel (`requiresLaunchContextError` →
> `notYetImplementedError("Plan 2")`) is also pending on RTQ Item A.
> **Concretely, RTQ Item A + ENG-1 + FC-1 + FC-2 (incl. the B3 code half) must
> land before 1b's ambient-API tests, execute step-6 field routing, the
> deferred-deps wire seam, and any Plan-1c E2E can go green.** RTQ ENG-2 is
> *not* a gate (its behavior already landed). What 1b *can* build pre-RTQ is the
> pure-TS layer — `framing.ts`, `mapped-source.ts` (T2), the dispatch loop +
> per-engine queue + cancel/poison/relaunch (T3/T5/T6/T7), T1 (uses
> `@quarto/api/config`, which exists), T4 — all in vitest against an in-memory
> duplex and a 1b-authored `src/types.ts` written to the post-RTQ shape.
>
> Status of the rest: plan1a-host (subprocess management) **has landed**
> (Part 1+2 — `ts_process.rs`: transport split, demux, teardown, bundle
> extraction); plan1a-engine (trait extensions, `TsEngine` struct) **has
> landed** and runs in parallel with 1b. The placeholder
> `dist/engine-host-deno.js` + its `include_str!` are in place.
**Blocks:** Plan 1c (extension integration + E2E echo test), Plan 2 Phase A
(the deferred launch-context bodies plug into 1b's QuartoAPI assembly), Plan 3
Phase 3E (wire jupyter into the harness), Plan 4 (Julia validation).
**Estimated sessions:** 2–3 (the original "1" predates the parallel-Pass-2 /
multiplexing rework, which expanded scope: 7 Phase-0 seams + ~12 contract tests,
multiplexed dispatch, cooperative cancel + poison/re-launch, `framing.ts`, the
`cargo xtask` bundle step, the staleness diagnostic, and the CI freshness check).

## Status: COMPLETE (2026-06-30)

All Work Items, Engine-API contract tests, and Success Criteria delivered and verified
(every checkbox reconciled to `[x]` against actual landed code). Implemented via
subagent-driven development over 11 sub-tasks on `feature/ts-engine-extensions`, commit
range `78be126c3..ed2b701ac` (the 14 feature commits + cleanup + this reconcile). Final
whole-branch review (opus): **READY TO MERGE: YES** — no Critical/Important; every Success
Criterion met with file:line evidence; the harness composes end-to-end (wire types consistent
across all handlers, execute pipeline composes, the four concurrency invariants interact
safely, build/CI/embed non-circular). Workspace green: `cargo nextest` 10451 passed/197
skipped; engine-host-deno vitest 105 passed; 3 deno-tests; bundle byte-stable (freshness gate
exit 0); `cargo build` re-embeds cleanly.

**Two scope clarifications (delivered intent, slightly different shape than the prose above):**
1. **esbuild entry is `src/main.ts`, not `src/host.ts`.** The Phase-2 entry-point split keeps
   `host.ts` (the testable `runHost` core) free of `Deno.*` so it runs under vitest; the thin
   Deno `main()` lives in `src/main.ts` (excluded from tsc, the esbuild entry point). The
   "bundle `src/host.ts`" wording predates the split.
2. **Staleness diagnostic = committed `dist/build-info.json` stamp (gitCommit/gitDirty/builtAt,
   gitignored) + the CI freshness gate**, not a runtime `--launcher-info` flag. The engine-host
   bundle is embedded in `quarto-core` via `include_str!` and has no standalone launcher binary
   (unlike `q2 mcp`), so the byte-stable bundle + the `git diff --exit-code` freshness check is
   the stale-bundle guard. `builtAt` is kept out of the bundled bytes so the diff is non-flaky.

**Deferred as designed (not 1b gaps):** Rust-side `SourceInfo::Concat` reconstruction of the
`markdownForFile` source-map (A′; the TS serializer is built, wire rides unconsumed in v1);
`path.runtime`/`resource`/`dataDir` + `system.pandoc` bodies (Plan 2 `notYetImplementedError`
stubs); the `jupyter` namespace (Plan 3 stub); the `LanguageClaim`-object interop/fallback
passthrough in `mapLanguageClaim` (reachable only via the object form; add with the first
q2-native engine that needs it); the q2 render-orchestrator that drives the `dependencies`
round-trip (Plan 1c); the Plan-1c E2E echo test. Carried Minor polish items recorded in the
session ledger.

## Overview

Build the Deno-side subprocess harness — the TypeScript package that
receives framed `Request` envelopes, dispatches each to a loaded
engine module via a **non-blocking read loop** (concurrent across engines,
serialized per engine instance), and writes `Response` envelopes back.
In the original v1 design this rode the child's **stdin**/**stdout**; Phase
1.6 has since landed and moved the channel to a private loopback-TCP
connection (see the blockquote below and
`claude-notes/plans/2026-07-08-plan1a6-off-stdout-loopback-tcp.md`). This is
the counterpart to plan1a-host's Rust-side subprocess manager and its demux.

> **Concurrency (Phase 1.5).** Pass-2 render is now parallel, so the shared
> subprocess is reached concurrently. The wire is **multiplexed** — every frame
> carries an `id` (plan1a-protocol Phase 1.5), and the harness keeps reading
> while prior requests are still running, so cross-engine requests interleave on
> the Deno event loop. **The channel stayed stdin/stdout in the original v1
> design** — multiplexing was all parallel Pass-2 needed, and the "stdout is
> protocol; `console.log` is forbidden" contract held at the time. Moving the
> protocol off stdout onto **loopback TCP** (to delete that footgun) was an
> orthogonal cleanup, **Phase 1.6**, which has since landed — that contract no
> longer applies. Canonical model:
> `claude-notes/designs/engine-host-concurrency.md`.

**Build model:** the harness is a **generated build artifact** — esbuild bundles
the TS sources into a single `dist/engine-host-deno.js`, which is committed and
embedded via `include_str!`. This is the **`q2 mcp` `dist-bundle/` / `q2-preview-spa`
`dist/` pattern** (commit the build *output*, embed it), **not** the `resources/…`
*source* pattern (reveal.js/clipboard/bootstrap embed hand-written source — a
different thing). See plan1a-host's "Bundle embedding" section for the full framing;
the short version:
1. Source lives in `ts-packages/quarto-engine-host-deno/src/`
2. **esbuild** bundles it into a single `dist/engine-host-deno.js` (the committed build output)
3. Rust embeds it via
   `include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ts-packages/quarto-engine-host-deno/dist/engine-host-deno.js"))`
   in `ts_process.rs` (behind `#[cfg(not(target_arch = "wasm32"))]` with the rest of the module).
   The path is anchored at `CARGO_MANIFEST_DIR` (the `quarto-core` crate root, an
   absolute path known to rustc) — two `..`s reach the repo root
   (`quarto-core/`→`crates/`→root). It is deliberately **not** a source-file-relative
   `"../../../../…"` from `ts_process.rs`: that form silently breaks if the file moves
   within the crate, the trap plan1a-host's "Bundle embedding" explicitly calls out.
   plan1a-host ships a placeholder file at this path so `include_str!` compiles cleanly on fresh clones; Plan 1b replaces the placeholder with the real esbuild output.
4. At runtime, writes the embedded JS to a temp file and runs `deno run --allow-all <tempfile>`
5. Only developers editing the TS harness need to rebuild (via the esbuild bundle step in Phase 4)

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

- [x] **T1 — metadata partition (`metadataAsFormat` port).** *Tier:* pure
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

- [x] **T2 — MappedString rehydration accuracy (`mapped-source.ts`).**
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

- [x] **T3 — per-message-type dispatch + `id` correlation (`host.ts` loop).**
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

- [x] **T4 — `markdownForFile` mapping serialization (TS side only in v1).**
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

- [x] **T5 — concurrent dispatch, per-engine serialization (`host.ts`).**
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

- [x] **T6 — cooperative cancel via `Cancel` (`host.ts`).** *Tier:* integration
  (vitest). *Real unit:* the loop's `Cancel { target }` handling. *Seam:* a test
  engine whose `execute` awaits an `AbortSignal`-aware deferred that rejects when
  aborted; send `execute` (id=N), then `Cancel { target: N }` before resolving;
  assert the engine's `AbortSignal` fired and the request N resolves as
  `Cancelled`, while a *concurrent* request on another engine is unaffected.
  *Mock boundary:* the engine's abortable deferred; the loop is real. *Named
  reverts:* ▸ drop the `case 'cancel'` arm → the abort-fired assertion RED. ▸
  abort *all* in-flight tasks instead of only `target` → the "sibling
  unaffected" assertion RED.

- [x] **T7 — concurrent same-instance poison → transparent re-launch (`host.ts`).**
  *Tier:* integration (vitest). *Real unit:* the dispatch path's
  reconstruct-on-missing-instance, **run inside the per-engine serialized continuation**.
  *Seam:* one engine (julia). Send `execute` (id=A), then `execute` (id=B) and
  `execute` (id=C) on julia — B and C queue behind A on the per-engine queue.
  `Cancel { target: A }` → A poisons (instance dropped). Assert: B **transparently
  re-launches** and **completes normally** (resolves with an `executeResult`, NOT an
  error); C then runs on the **already-reconstructed** instance; and across the B+C
  dequeue `engine.launch` is called **exactly once** for the reconstruct (the queue
  serializes them, so C sees the instance B rebuilt — no double-launch). *Mock
  boundary:* the engine's deferreds + a `launch` call-counter; the loop + queue are
  real. *Named reverts:* ▸ make dispatch *fail* a request whose instance is missing
  (instead of reconstructing) → B's "completes normally" assertion RED. ▸ move the
  check-and-reconstruct into the **synchronous dispatch prologue** (before chaining
  onto the per-engine queue) → B and C both observe the missing instance and both
  call `engine.launch` → the "exactly once" assertion RED (binds the Phase-2
  implementation guard).
  *Scope:* the test-double engine reconstructs cleanly, so "completes normally"
  holds here. With a *real* daemon left wedged by A's aborted work, B's re-run
  re-poisons on its **own** `window` (self-healing) — B is guaranteed only *not to
  fail for A's timeout*, not to succeed. That wedged-daemon path is covered by B's
  independent timeout, not by T7 (see Phase 2's poison bullet and
  `engine-host-concurrency.md` §3 ¶3).

**Missing-test pass (reasoned, per the skill).** Behavior deliberately left
unguarded *here*, with rationale:
- **Whole-subprocess SIGKILL** (crash / compromised channel / teardown) —
  accepted-untested in 1b. Rationale: the kill is issued and observed on the
  Rust side; the binding test is plan1a-host's "crash/malformed → subprocess
  gone". 1b's contract is the *cooperative* path (T6), not the SIGKILL one.
- **Stdout-violation detection** (`console.log` corrupted the v1 stdout protocol)
  — accepted-untested in 1b; owned and tested by plan1a-host (the Rust side parsed
  stdout). 1b's contract was only "the harness writes nothing but `Response`
  frames to stdout," which T3 indirectly exercised (every asserted line is valid
  JSON). (Phase 1.6 has since landed and retired this concern — stdout is no
  longer the protocol channel.)
- **`htmlDependency` relative-path normalization** and **`loadEngine`
  path-drift error** — already itemized as bound tests in the Engine-API
  contract block below; not re-listed.
- **`Init`-frame handling** (response-less first frame; builds the API before the
  first `loadEngine`; pre-`Init` message → error) — **now bound by T-A5** in the
  Engine-API contract block (added with the RTQ Item-A host-loop ownership); the
  entry-point split (`runHost(reader, writer, host)`) is exercised by every loop
  test (T3/T5/T6/T7/T-A5) driving the in-memory duplex, so it needs no separate row.
- **`drain-before-exit` on EOF/`shutdown`** (a still-in-flight `Execute`'s
  `Response` is flushed before `Deno.exit`, not truncated) — **accepted-untested in
  1b v1.** Rationale: in the normal teardown order q2 stops issuing requests before
  closing stdin, so the drain is the empty common case; the backstop is bounded by
  plan1a-host's `Drop` SIGKILL. Flagged here rather than silently omitted — a
  focused test (queue a slow `Execute`, then EOF; assert the response still
  arrives) is worth adding if parallel-Pass-2 teardown ever races in practice.

### Phase 1: Package setup + esbuild

- [x] Create `ts-packages/quarto-engine-host-deno/package.json`:
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
- [x] Create `esbuild.config.mjs` — bundle `src/host.ts` → `dist/engine-host-deno.js`.
    Use `platform: "neutral"` and `format: "esm"` (NOT `platform: "browser"` /
    `format: "iife"` — that shape targets an embedded QuickJS/Boa runtime, whereas
    engine-host-deno targets Deno, which runs ES modules and has its own globals
    like `Deno.stdout`, `Deno.Command`)
- [x] Add `@quarto/api` and `@quarto/types` as dependencies. The Plan 2A
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

- [x] **Add the `HtmlDependency` type to `@quarto/types`** (gap — it does
    **not** exist anywhere in `ts-packages/` today). Shape: `{ name: string;
    stylesheets?: string[]; scripts?: string[] }`, mirroring the Rust
    `TsHtmlDependency` (plan1a-protocol). Both the engine-author SDK and this
    plan's `src/types.ts` import it from `@quarto/types` — do **not** redefine it
    locally. This is a small addition to the (otherwise "done") Plan 2A
    `@quarto/types` package that 1b owns because 1b is its first consumer (the
    return-based `htmlDependencies` field in Phase 3).

### Phase 2: `host.ts` main loop

- [x] Create `src/host.ts` — **non-blocking, multiplexed dispatch over
  stdin/stdout** (v1).

  > **Entry-point split — testability constraint (binds T1–T7).** The dispatch
  > loop must be a **stream-injected core** that takes its reader, writer, and
  > `PlatformHost` as parameters — `runHost(reader, writer, host): Promise<void>`
  > — with a **thin Deno `main()`** that is the *only* place `Deno.stdin` /
  > `Deno.stdout` are touched (`runHost(Deno.stdin.readable, Deno.stdout, denoHost)`).
  > The contract tests (T3/T5/T6/T7) drive `runHost` over an **in-memory framed
  > duplex** under **vitest (Node)** — they never spawn Deno — so the core must
  > not reach for `Deno.*` at module scope or inside the loop. A module that
  > reads `Deno.stdout` at top level (as the sketch below does for brevity)
  > would fail to import under Node and make the tests unrunnable. Keep the
  > `Deno.*` surface confined to `main()`.

  ```typescript
  // Sketch (inside runHost(reader, writer, host) — NOT module top-level):
  // Original v1 design: protocol ran on the injected reader/writer
  // (Deno.stdin/stdout in main()). Capture the protocol-write target BEFORE
  // any engine code runs (engines must not write to it — see "Stdout/stderr
  // contract"). The harness does NOT override console.* — engine authors use
  // stderr (console.error/warn or quarto.console.*). Phase 1.6 has since
  // landed: it swaps stdin/stdout for a loopback-TCP conn and frees stdout
  // for diagnostics — see plan1a-host "Phase 1.6 — the protocol moved off
  // stdout (loopback TCP) — LANDED".
  const protocolOut = writer;                        // Deno.stdout in main() (v1); TCP conn since Phase 1.6
  const writeMutex = new AsyncMutex();               // serialize async frame writes
  const perEngineQueue = new Map<string, Promise<unknown>>();  // tail per engine
  const inflight = new Map<number, AbortController>();          // by request id

  let quartoAPI;                                     // built from the Init frame
  for await (const frame of readFrames(reader)) {    // <-- never awaits handler
    const { id, msg } = frame;
    if (msg.type === "init") {                       // RTQ Item A: first frame,
      quartoAPI = buildQuartoAPI(msg.global, host);  //   response-less — no reply
      continue;
    }
    if (!quartoAPI) { writeError(id, "message before Init"); continue; }
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
    - **Unbounded tail — accepted for v1.** The per-engine `perEngineQueue` entry
      is a promise chain with no depth cap, so a project that routes thousands of
      files through one engine builds a long tail (each link is a small closure;
      latency, not memory, is the practical limit, and it's bounded anyway by the
      daemon's serial throughput — concurrency past one same-engine `Execute` is
      physics, not a choice; see `engine-host-concurrency.md`). Each link is also
      pruned as it settles (overwrite the map entry when the tail resolves so a
      finished chain is GC'd, not retained for the subprocess's life). If a single
      engine's queue depth ever becomes a problem, bound it with backpressure
      (pause reading frames for that engine) — not built now.
  - **`id` correlation.** Each response is written as `{ id, msg: <FromEngine> }`
    echoing the request's `id`; the Rust demux routes by it. (T3 binds this.)
  - **`Cancel { target }`.** Each request runs under an `AbortController` stored
    in `inflight[id]`. **Register `inflight[id]` at dispatch time — before the
    per-engine queue admits the task** — so a request that times out while still
    *queued* behind a sibling is cancellable (the Rust `window` ticks during
    queue-wait; see `engine-host-concurrency.md` "Timeout includes same-engine
    queue wait"). `Cancel` aborts exactly that one (passing its `AbortSignal` into
    the engine's `execute`) and leaves siblings untouched. (T6 binds this.)
    **`Cancel` is fire-and-forget — no ack.** The harness writes a `Cancelled`
    response under that `id`, but a *timeout*-initiated `Cancel` has already
    resolved the Rust worker's slot locally (`recv_timeout` → `Timeout`), so the
    Rust demux drops that late `Cancelled` as an unknown id. The wire `Cancelled`
    exists for protocol completeness; q2 learns of cancellation from its own
    local resolution, not from a returned frame.
  - **Poison the instance when the cancelled request was an `Execute`** (the only
    daemon-engaging request): the harness **drops its `instance` entry** for that
    engine. Cancelling a non-`Execute` request engages no daemon and drops no
    instance. (Harness half of plan1a-host's poison policy.)
  - **A concurrent same-instance request transparently re-launches — it is not
    failed.** When `dispatch` dequeues a request (e.g. worker B's `Execute`) for
    an engine whose `instance` entry was dropped by a poison, it **re-runs
    `engine.launch(stashedProject)` to reconstruct the instance, then runs the
    request** — B never fails for A's timeout and never sees a half-torn-down
    instance. (Composes with idempotency: a *present* instance makes
    `LaunchEngine`/re-launch a no-op; a *missing* one triggers exactly one lazy
    reconstruct, whether from a queued request here or a fresh `LaunchEngine`
    from q2. The harness stashes the last `project` (`EngineProjectContext`) per
    engine for exactly this — so re-launch reconstructs with the same project the
    instance was originally launched from.)
    **Implementation guard — the reconstruct runs inside the per-engine serialized
    continuation, not the synchronous dispatch prologue.** The check-and-reconstruct
    ("instance missing? → `engine.launch()`") must execute in the dequeued task that
    the per-engine queue runs one-at-a-time, so two *adjacent* same-engine requests
    cannot both observe a missing instance and both call `engine.launch()`. The first
    dequeued request reconstructs; the next, serialized after it, finds the instance
    present (idempotent no-op). Doing the check in the prologue — before chaining onto
    the per-engine queue — would race the two into a double-launch.
    See the design note's poison §3. (T7 binds this.)
    **Re-launch reconstructs the JS instance, not the daemon — it does not
    *guarantee* B completes.** `engine.launch()` rebuilds the `ExecutionEngineInstance`
    object; the detached daemon (Julia/Jupyter) that A's aborted-but-still-running
    work may have left wedged is re-discovered lazily by B's `execute()`. If it is
    genuinely wedged, B's re-run hits *its own* `window` and re-poisons on its own
    merits — self-healing, but B is guaranteed only *not to fail for A's timeout*,
    not to succeed (`engine-host-concurrency.md` §3 ¶3). T7 exercises only the
    clean-reconstruct path (test-double engine); the wedged-daemon path is covered
    by B's independent timeout, not by T7.
  - **Frame writes are async and serialized under `writeMutex`.** `writeFrame`
    `await`s the write (`await out.write(bytes)`) rather than blocking the thread,
    so the read loop keeps draining stdin while a large response goes out — the
    **continuous-drain** property that prevents a large-payload pipe deadlock
    (`engine-host-concurrency.md`: continuous drain is "load-bearing on stdio, not
    optional"). Because an async write yields, two concurrent dispatch tasks could
    otherwise interleave their bytes, so `writeMutex` serializes them and each frame
    is written whole. (Single-threaded Deno makes each individual `write` atomic, but
    that is *not* sufficient — the yield between awaits is why the mutex is needed.
    Do **not** swap in a synchronous `writeSync`: a blocking write stalls the read
    loop and can deadlock both pipes under concurrent large frames.) See `framing.ts`.
  - Handle handler errors gracefully (catch → send `error` under the same `id`,
    never crash the loop).

- [x] **Must dispatch all message types** from the protocol (the landed `ToEngine` enum, **plus** the two RTQ Item-A additions — the `Init` message and the `Dependencies` verb (FC-2)):
    - `init` **(RTQ Item A — first frame, response-less)** → the loop's **very
      first** action: read one `Init { global: HostGlobalConfig }` frame, call
      `buildQuartoAPI(global, denoHost)` once, and stash the single shared
      `quartoAPI` reference for every later `loadEngine`'s `engine.init?.()`.
      **`Init` is fire-and-forget — the harness writes NO response** (it is sent
      like `Shutdown`, in a `Request` envelope with a throwaway `id` and **no
      pending slot** on the Rust side, so a reply would be dropped as an unknown
      id). **Ordering is guaranteed by the single-threaded stdio stream:** q2
      sends `Init` before the first `loadEngine`, and frames are processed in
      arrival order, so `quartoAPI` is always built before any engine's
      `init()` runs. Defensive guard: a `loadEngine` (or any other message)
      arriving while `quartoAPI` is still unset is a protocol violation — reply
      `error` ("engine message before Init"); do not silently build a partial
      API. `Init` carries **no** correlated `FromEngine` variant — `src/types.ts`
      adds `Init` to `ToEngine` only, with nothing on the response side.
    - `loadEngine` → `await import(toFileUrl(enginePath))`, validate
      exports, call `engine.init?.(quartoAPI)` (optional — engines that
      use `quarto.*` implement it to stash the reference, per Q1
      contract). The QuartoAPI handed to `init()` is built over the
      process-stable `Init { global }` config (delivered once at spawn),
      so **every namespace — including `path.runtime`/`resource`/`dataDir`
      and `system.pandoc` — resolves immediately; there is no gating**
      (RTQ Item A). See the "Engine API contract" section below. Store
      the `ExecutionEngineDiscovery` object keyed by engine name. Return
      `loaded` with `LoadEngineResult` (name, validExtensions,
      **generatesFigures, canFreeze, quartoRequired** — the static
      discovery-tier fields, read off the discovery object; RTQ ENG-1). If
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
    - `launchEngine` → carries the per-render **`project: EngineProjectContext`**
      (RTQ Item A / DQ-7) — **not** the old combined `EngineHostContext`. The
      four fields, mapping to Q1's `EngineProjectContext` (`project/types.ts`):
      `projectDir?` (the project root, `Option`), `isSingleFile` (bool),
      `config?` and `output_dir?`. **These last two overlap in name only — keep
      both:**
      - `config?` — the **raw declared settings** from `_quarto.yml`: the
        project `engines:` block **plus the declared `output-dir` config key**
        as written (may be relative, defaulted, or absent). This is Q1's
        `EngineProjectContext.config?` — config *data*, not a resolved path.
      - `output_dir?` — the **resolved** output directory (Q1's
        `getOutputDirectory()` return, turned from a callback into a plain value
        — DQ-5): a computed/absolute path the engine actually writes to. Wire/serde
        name `output_dir`, surfaced in TS as `outputDir?`.

      So `config` *carries* the declared `output-dir` setting while `output_dir`
      *is* the resolved directory — the engine reads whichever it needs. The
      harness reconstitutes the **full** Q1
      `EngineProjectContext` from these four (see below: the harness-local
      `fileInformationCache` Map and the push-model `resolveFullMarkdownForFile`
      are synthesized by the harness, not carried on the wire — DQ-1). If the
      engine is already
      launched (cached `instance` in the harness's per-engine record), return the
      cached `LaunchEngineResult` without re-running `engine.launch(project)`. In
      dev builds, assert the supplied `project` matches the cached one on a small
      **identity key** — `(projectDir, isSingleFile)` — **not** a full deep-equal
      (a per-render `temp_dir`/`outputDir` shim may legitimately differ run-to-run).
      In release, silently use the first `project`.
      Otherwise: call `engine.launch(project)`, store the resulting
      `ExecutionEngineInstance`, and return `launched` with `LaunchEngineResult`
      (**`canFreeze` only** — read off the `ExecutionEngineInstance`,
      `execute/types.ts:95`). **`generatesFigures`/`canFreeze`/`quartoRequired`
      are discovery-tier and were already returned on `loaded`** (RTQ ENG-1) — do
      **not** re-source `generatesFigures` here. There is **no `state.context` to
      set and nothing to unblock**: the whole API was available from the
      `Init { global }` config delivered at spawn (RTQ Item A — gating removed).
      `engine.launch(project)` only **constructs** the `ExecutionEngineInstance`
      object — it is cheap (~0), matching Quarto 1, where `launch()` is a
      synchronous object-literal construction that starts no daemon. **`launch()`
      takes Q1's `EngineProjectContext` (`execute/types.ts:86`)**, which the
      harness builds from `msg.project` — including a **harness-local
      `fileInformationCache` Map** and a `resolveFullMarkdownForFile` that returns
      the pushed resolved markdown (push model, DQ-1; no engine→host callback). It
      *separately* synthesizes the minimal `ProjectContext` shim for `execute()`
      (see the execute-dispatch flow) — keep the two shims distinct; do not feed
      one object to both. The expensive engine startup (Julia control server /
      Jupyter kernel: 5+ s) happens **lazily inside the engine's `execute()` on the
      first call** (see the `execute` handler below) and is amortized by the
      external daemon — never at `launchEngine`. The idempotency rule still matters
      — double-launching would build duplicate instance objects — and it mirrors
      the `loadEngine` one, which is what makes the Rust-side
      `OnceLock<LaunchEngineResult>` safe under concurrent racers (see
      plan1a-engine).
    - `claimsLanguage` / `claimsFile` → call discovery methods on the loaded
      engine. Engine must be loaded; not required to be launched.
      **`claimsLanguage` normalization:** the engine may return
      `boolean | number | LanguageClaim` (the kind-tagged object); the harness
      normalizes to the tagged wire result before replying — `false`/`null`/
      `undefined` → `None`, `true` → `{kind:"primary",priority:1}`, `number n`
      → `{kind:"primary",priority:n}` (**no sign games — a negative number is a
      low-priority primary, never interop**), and a `LanguageClaim` object
      passes through as its kind. `interop`/`fallback` are reachable only via
      the object. **The `LanguageClaim` object is a q2 extension, not Q1 parity:**
      Q1's `claimsLanguage` returns `boolean | number` only (`execute/types.ts:56`),
      so the Julia validation target uses just those; the kind-tagged object exists
      solely to let q2-native engines express `interop`/`fallback`. See
      `claude-notes/designs/engine-resolution.md` §3.2 and plan1a-protocol's
      `TsLanguageClaim` appendix.
    - `markdownForFile` → call `instance.markdownForFile(file)` (non-QMD files
      only); serialize the MappedString result with `source_map` for
      `markdownForFileResult`. Engine must be launched.
    - `execute` → see the execute-dispatch flow below. Engine must
      be launched. This is where the engine daemon comes up: the engine's
      `execute()` **starts the external daemon (Julia control server /
      Jupyter kernel: 5+ s) lazily on the first call, or reconnects to an
      already-running one** keyed by a transport file in the runtime
      directory. The daemon is amortized across renders and survives a
      Deno-subprocess respawn (reconnect, not relaunch) — it is never
      started at `launchEngine`.
    - `intermediateFiles` → call `instance.intermediateFiles(input)` if
      implemented; else return `undefined`. Engine must be launched.
    - `dependencies` **(new verb — RTQ FC-2; not in the v1-frozen `ToEngine` enum
      yet)** → call `instance.dependencies(options)` and reply with
      `dependenciesResult { includes }`. Engine must be launched. The harness is a
      **thin pass-through** — **q2's render orchestrator** drives this, not the
      harness: when an `execute` reply carries a non-empty `engineDependencies`
      (only under `dependencies: false`), q2 iterates that map by engine name and
      sends one `dependencies` message per key (mirroring Q1's `render.ts:90-109`,
      with `output` = the final/merged output). The returned
      `DependenciesResult.includes` (`inHeader`/`beforeBody`/`afterBody` file paths)
      lands in q2's `includes`/`format.pandoc` — **NOT** `htmlDependencies` (the two
      dep channels are disjoint; plan1a-protocol appendix "Two disjoint dep
      channels"). It is the symmetric sibling of `intermediateFiles`. Deferred
      feature (book/project rendering); inert for real engines until Plan 3E lands
      `quarto.jupyter.widgetDependencyIncludes`.
    - `shutdown` → clean up, exit. **AND: the read loop must also exit when
      stdin reaches EOF** — q2's graceful `TsEngineHost::shutdown()` sends the
      `Shutdown` frame and *then closes the child's stdin*, and the host then
      `join`s waiting for the child to exit (plan1a-host "Teardown & reaping").
      If the harness does not exit on stdin EOF, graceful teardown blocks until
      the host's `Drop` SIGKILL fires — defeating the clean-exit path. So:
      `for await (… of readFrames(Deno.stdin))` falling through (iterator done =
      stdin EOF) must break the loop and `Deno.exit(0)`, the same terminal as the
      `shutdown` message.
      **Drain before exiting.** Because dispatch is non-blocking, requests may
      still be in flight when EOF/`shutdown` is reached. Before `Deno.exit(0)`,
      `await` the `perEngineQueue` tails (and flush their `Response` frames) so a
      concurrently-running `Execute`'s response is written, not truncated. In the
      normal teardown order q2 stops issuing requests before it closes stdin, so
      the drain is usually empty — it is a correctness backstop under parallel
      Pass-2, not the common path. (A hung drain is still bounded by the host's
      `Drop` SIGKILL.)

  Discovery messages without a prior `loadEngine` for that engine, or
  instance messages without a prior `launchEngine`, return an `error` with
  a clear message. (Rust side guards against this via the `TsEngine` state
  machine, but the harness validates defensively.)

- [x] **Lifecycle methods deliberately NOT on the protocol.** Q1's
    `filterFormat`, `executeTargetSkipped`, `postprocess`, `canKeepSource`,
    `postRender`, and `partitionedMarkdown` are not protocol messages. The
    harness does NOT dispatch them as top-level messages. (`dependencies` **is**
    now a protocol message — the new `dependencies` verb (RTQ FC-2), **not** folded
    into `execute`; q2 orchestrates it. See the `dependencies` arm above.)
    `partitionedMarkdown` is subsumed by q2's `DocumentProfile`
    checkpoint plus filter-aware `markdown_for_file` (see
    `claude-notes/plans/2026-04-23-ipynb-filters-and-engine-partitioning.md`).
    The other Q1 lifecycle hooks have no q2 caller and are deferred —
    when q2 grows callers, they'll appear here as new message types.

- [x] **Execute dispatch flow:**
    1. Call `instance.target(file, quiet?, markdown?)` if implemented
       (harness-internal). **Arity is `(file, quiet?, markdown?)`, file-first**
       (`execute/engine.ts:370`) — the reconstructed `MappedString` is the third
       (`markdown`) argument, not the sole one. Use its result (including the opaque
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
       `TsExecuteOptions`, the per-render launch `project`
       (`EngineProjectContext`), and the process-stable `Init { global }`
       config:
       - `target` ← built per step 1 above.
       - `format` ← built per step 2 above.
       - `resourceDir` ← the `Init` `global.resourceDir`.
       - `tempDir` ← `TsExecuteOptions.temp_dir`.
       - `libDir` ← `TsExecuteOptions.lib_dir`.
       - `projectDir` ← the launch `project.projectDir` (passes
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
         on the Rust side.) **Harness contract:** 1b owns only *faithful
         forwarding* — when the engine throws, the harness emits a protocol
         `error` frame verbatim and never swallows it (the generic catch→`error`
         path, exercised by T3's negative path). The loud-failure *semantics*
         (an owner that can't execute its language) are bound on the Rust side
         (plan1a-engine's `NoHandlerForLanguage`), not by a 1b-specific test.
       - `dependencies` ← `TsExecuteOptions.dependencies` — a **real wire field**
         (`#[serde(default)]`, **default `true`**; added by RTQ FC-2), mirroring
         Q1's `resolveDependencies` (default true — `render-files.ts:146,224`).
         The **v1 path always sends `true`**: `execute()` resolves deps **inline**
         into `includes` (jupyter `resultIncludes`, `jupyter.ts:557`) and produces
         **no** `engineDependencies`. A future q2 **book/project renderer** sends
         `false` (many chapter `execute()`s merged into one output, deps resolved
         once at the final render): `execute()` then returns the deferred
         `engineDependencies` map, which the harness **forwards on the wire** for
         q2's orchestrator to resolve via the separate `dependencies` verb (step 4,
         and the `dependencies` arm above). The flag + verb + `engineDependencies`
         wire field are **built now as infrastructure** even though no v1/Julia
         caller sends `false` ("defer features, not infrastructure" — see RTQ FC-2).
       - `project: ProjectContext` — synthesize a minimal Q1
         `ProjectContext`-shaped record with **only the fields engines
         actually read** (`isSingleFile` from the launch
         `project.isSingleFile`; `temp` from a small
         shim that wraps `TsExecuteOptions.temp_dir` with a
         Q1-compatible `createFileFromString` helper). Other fields
         on Q1's `ProjectContext` (notebookContext, config, files) are
         not set — engines that read them will get `undefined`, but the
         engine-side audit confirmed no engine in Q1's tree reads
         them in `execute()`. (The `fileInformationCache` an engine reads
         is the harness-local Map built for `launch()`, not a wire field —
         push model, DQ-1.) If a future Q1 sync brings in such a reader,
         expand the launch `project` / `Init global` and the synthesizer
         together.
       - `previewServer: false` — no q2 use case; pass a safe default.
         (Do **not** add `output` here: `output` is not a member of
         `ExecuteOptions` (`execute/types.ts:149-163`); it belongs only to
         `DependenciesOptions`/`PostProcessOptions`.)
    3. Call `instance.execute(options)`, get back an `ExecuteResult` in
       Q1's shape.
    4. **Do NOT resolve dependencies in the harness — forward
       `engineDependencies` on the wire.** When `dependencies: false`, `execute()`
       returns a non-empty `engineDependencies` map (engine-name-keyed,
       `Record<string, Array>` — `execute/types.ts:174`). **Forward it verbatim** as
       `TsExecuteResult.engineDependencies` (a new wire field, RTQ FC-2); the harness
       does **not** call `dependencies()` here. q2's render orchestrator owns the
       round-trip: it iterates `engineDependencies` by engine name and sends a
       separate **`dependencies`** message per key (the `dependencies` arm above;
       mirroring Q1's `render.ts:90-109`, where `output = recipe.output`). Folding it
       into `execute` here would be both un-Q1 *and* useless for the deferred path's
       only purpose — resolving deps **once at a merged output** (book/project
       rendering) — which the harness cannot do because it sees one document at a
       time. When `dependencies: true` (the v1 path) there is no `engineDependencies`
       and this is a no-op. (The eventual `DependenciesResult.includes` from the
       `dependencies` round-trip lands in q2's `includes`, **not**
       `htmlDependencies` — disjoint channels.)
    5. **`htmlDependencies` is populated from the engine's return value**,
       not from any imperative registration. A q2-native engine emits structured
       deps as an optional `htmlDependencies?: HtmlDependency[]` field on the value
       it **returns** from `execute()` (the one q2 deviation from Q1's result
       shape — and a return-based one, matching Q1's "deps are return values"
       philosophy; see Phase 3). The harness reads `result.htmlDependencies` off
       the returned object and emits it on the wire. There is **no** harness-side
       accumulator and **no** `quarto.*` registration method (that would be mutable
       state on the shared QuartoAPI, which races under concurrent cross-engine
       `execute()` — see Phase 3). Each entry's `stylesheets` and `scripts` MUST be
       absolute paths to files already on disk; the harness normalizes any relative
       paths in the returned list against `TsExecuteOptions.lib_dir` before
       serializing. For Q1-shaped engines that return no such field, wire
       `htmlDependencies` is empty.
    6. Build `TsExecuteResult` from the Q1-shaped `ExecuteResult`, routing
       **every** field deliberately (Q1's `ExecuteResult`,
       `execute/types.ts:166-178`) — do not silently drop any:
       - `markdown` → `TsExecuteResult.markdown`.
       - **`supporting` → `TsExecuteResult.supporting` — forward it; it is
         load-bearing.** The wire field exists (`ts_protocol.rs`) and the Rust
         side maps it to `ExecuteResult.supporting_files`
         (`crates/quarto-core/src/engine/context.rs`), which the orchestrator
         drains into the resource report and copies into the output directory
         (bd-o8pr) — the **same path knitr's / Jupyter's `<doc>_files/` figure
         dirs take**. Dropping it would write engine-produced figures to disk but
         leave them untracked (orphaned, or not copied into the site). This is
         exactly what Q1 uses `supporting` for (render-side copy + cleanup).
       - **`filters` → `TsExecuteResult.filters` — forward it.** Maps to
         `ExecuteResult.filters` (`context.rs:220`), q2's per-document
         pandoc-filter list (e.g. the "quarto" filter). q2 carries/traces it but
         has **no filter-application stage acting on it yet** (only `trace.rs`),
         so it is effectively inert downstream for now — but forward it rather
         than drop, so engine-declared filters survive until q2 grows the
         consumer. (Not a deliberate drop — the field exists on both wire and
         core.)
       - `includes` → `TsExecuteResult.includes` — the engine's **inline** includes
         from `execute()` (jupyter `resultIncludes` under `dependencies: true`) plus
         any direct-includes it emitted. (Deferred-path deps are **not** here — they
         arrive later via the `dependencies` verb's `DependenciesResult.includes`,
         merged by q2.)
       - `htmlDependencies` → from the engine result's `htmlDependencies`
         **return field** (Phase 3).
       - **`engineDependencies` → `TsExecuteResult.engineDependencies`** (new wire
         field, RTQ FC-2) — forwarded verbatim when present (only under
         `dependencies: false`); q2 orchestrates the `dependencies` round-trip from
         it (step 4). Empty/absent on the v1 inline path.
       - **Carried on the wire but inert (RTQ FC-1 — `#[serde(default)]` carriers
         with no consumer yet; forwarded, not dropped):**
         - `metadata` — q2 has no post-execute metadata back-merge yet, but the
           field is carried so a future merge step is a body-fill, not a protocol
           change.
         - `pandoc` — engine format-contributions; carried as a **loose JSON map**
           (the SDK's `pandoc?` is `Record<string, unknown>`, **not** a typed
           `FormatPandoc`). Inert until a "format-contribution" merge point exists.
         - `resourceFiles` — extra resources discovered during execution (distinct
           from `supporting`, which q2 *does* drain); carried, awaiting a TS engine
           that needs full-site resource tracking.
         - `preserve` / `postProcess` — Q1's field is `postProcess?: boolean`
           (`execute/types.ts:176`, not `needsPostprocess`); carried so the
           `postprocess` recovery (an AST transform reading `preserve` — the
           No-DOM-postprocessor rule) is a later body-fill. `needs_postprocess` is
           **wire-fed from `postProcess`** on the Rust side, not hardcoded `false`.
       - **The only true drop:** `engine` — trivial; the harness/Rust side already
         knows the engine from message routing. (`engineDependencies` is also on
         the wire — forwarded, see above.)
    7. Send `executeResult` (now carrying `engineDependencies` when the engine
       produced a deferred map).

- [x] **Test F2b (deferred-deps wire seam) — `engineDependencies` forwarding +
    `dependencies` verb (RTQ FC-2).** (RTQ's "F2b" — owned and run here in 1b, not
    RTQ.) A **fake** engine (not real jupyter) that
    reads `options.dependencies` and, when `false`, returns a non-empty
    `engineDependencies` (one engine-name key); and that implements
    `dependencies(opts)` returning `{ includes }` derived from `opts.dependencies`.
    Two parts:
    - **Forwarding (execute):** send `execute` with `dependencies: false`; assert
      the response's `engineDependencies` carries the engine's map **verbatim** and
      the harness did **not** call `dependencies()` (pass-through, no fold). Send
      `execute` with `dependencies: true`; assert `engineDependencies` is
      empty/absent.
    - **`dependencies` verb:** send a `dependencies` message with
      `DependenciesOptions.dependencies = engineDependencies[<key>]` and a given
      `output`; assert the harness calls `instance.dependencies(opts)` with those
      fields and replies `dependenciesResult { includes }` carrying the engine's
      `includes`.
    *Named reverts:* ▸ make the `execute` arm call `dependencies()` itself (the old
    fold) → the "harness did not call `dependencies()`" assertion RED. ▸ drop
    `engineDependencies` from the forwarded `TsExecuteResult` → the forwarding
    assertion RED. ▸ remove the `dependencies` message arm → the verb-response
    assertion RED. (T1–T7 never feed a non-empty `engineDependencies`; fake engine
    because real `widgetDependencyIncludes` is Plan 3E.)

- [x] **`target()` is harness-internal**, not a protocol message. Before
    calling `execute()`, the harness checks if the engine implements
    `target()`. If so, it calls `target(file, quiet?, markdown?)` (file-first;
    the reconstructed MappedString is the `markdown` arg — `execute/engine.ts:370`),
    and uses the returned `ExecutionTarget` (including the opaque `data` cookie
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

- [x] **Cooperative, per-request cancellation (`Cancel { target }`).** Under
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

The harness exposes the QuartoAPI to engines, built once over the
`Init { global }` config and **not gated on launch** (RTQ Item A). The contract:

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

- **Ambient API surface (no gating).** The QuartoAPI handed to `init()` is
  built once at startup over the **`Init { global }`** config (resource/runtime/
  data dirs, pandoc, version, interactive/CI — delivered at spawn, before any
  `loadEngine`). Object identity is stable — engines stash the reference and it
  remains valid for their lifetime. **Every namespace resolves immediately**,
  including the path/system methods Q1 derives from process globals:

  | Available from `init()` onward (the whole surface) |
  |---|
  | `quarto.text.*`, `quarto.markdownRegex.*`, `quarto.crypto.*` (pure) |
  | `quarto.format.*` (pure — always takes an explicit `Format`) |
  | `quarto.console.*` (host-only — writes stderr via `host.log`) |
  | `quarto.mappedString.*` (host-only — uses `denoHost.fs`) |
  | `quarto.path.*` incl. `runtime`/`resource`/`dataDir` (closed over `global`) |
  | `quarto.system.*` incl. `pandoc` (closed over `global.pandocPath`) |
  | `quarto.jupyter.*` conversion logic (mostly pure; figure writes go through host) |

  `path.runtime`/`resource`/`dataDir` and `system.pandoc` are **ambient** — the
  `@quarto/api` factories close over the `Init { global }` config injected at
  harness assembly, exactly like Q1's `resourcePath()`/`quartoRuntimeDir()` read
  process globals (`core/api/path.ts`). They were **never** gated in Q1 and are
  not gated here: the earlier "gated until launchEngine" model — a shared mutable
  `HostState.context` set at first launch — is **removed by RTQ Item A**.
  `format.*` is likewise pure: it always takes an explicit `Format` and reads no
  context (there is no no-arg form — `julia-engine.ts:236`, `core/api/types.ts:132`).

  **§2aa→1b body-stub caveat (not gating).** At 1b's landing the *real bodies* of
  `path.runtime`/`resource`/`dataDir` and `system.pandoc` are still deferred to
  Plan 2 (§2aa ships them as `notYetImplementedError("Plan 2")` stubs — RTQ B3,
  relabeled from the now-false `requiresLaunchContextError`). So those four throw a
  **"not yet implemented"** stub error until Plan 2 lands their bodies — a
  missing-body, **not** a launch gate, and it does not depend on `launchEngine`.

- **No per-launch context state.** There is no shared `HostState.context`, no
  "first launch unblocks the API," and no per-engine context invariant to assert.
  The only per-render context is the launch `project` (`EngineProjectContext`),
  captured in each launched instance's closure (pure Q1) — not a slot the API reads.

- **Classification ownership.** The pure / host-only split is a *contract*
  recorded as jsdoc on the `QuartoAPI` type in `@quarto/types` (Plan 2A / refined
  in 2E), so the namespace implementations in `@quarto/api` agree by construction.
  When Plans 2/3 add a method, they classify it there.

#### Tests for the Engine API contract

*Tier (all): integration (vitest) over the real host loop + in-memory framed
duplex, unless a row says "pure". The unit under test (`host.ts` /
`quarto-api.ts`) is never mocked; only the engine module + duplex are test
doubles.*

- [x] **Test T-A5 — `Init` frame is consumed, response-less, and gates
    `loadEngine` (RTQ Item A; the host-loop behavior RTQ handed to 1b).** *Real
    unit:* `host.ts`'s first-frame handling. *Seam:* (a) send `Init { global }`
    (throwaway id), then `loadEngine` for an engine whose `init()` calls
    `quarto.text.lines("a\nb")` and stashes the result; assert the engine's
    `init()` saw a **built** API (the stashed value is `["a","b"]`, no throw) and
    that **no `Response` frame is emitted carrying the `Init` id** (capture every
    written frame; assert none has that id). (b) Send a `claimsLanguage` (or any
    discovery/instance message) **before** any `Init`; assert the harness replies
    `error` ("message before Init"). *Mock boundary:* the engine + duplex; loop +
    builder real. *Named reverts:* ▸ make the loop write a `Response` for the
    `Init` id → the "no frame with the Init id" assertion RED. ▸ skip
    `buildQuartoAPI` on `Init` (leave `quartoAPI` unset / pass `undefined` to
    `init`) → the "init saw a built API → `["a","b"]`" assertion RED. ▸ remove the
    `if (!quartoAPI) writeError` guard → the pre-`Init` message is dispatched
    instead of erroring → the "message before Init → error" assertion RED.
- [x] Test: `init()` is called once when the harness handles
    `loadEngine` for an engine that exports it. A test engine records
    each call; verify a single invocation per loadEngine message.
    *Named revert:* ▸ drop the `loadEngine` cache-hit short-circuit so a repeat
    re-runs `engine.init?.()` → the "single invocation" count assertion RED.
- [x] Test: engine that does not export `init` loads cleanly (no
    error), and any subsequent `claimsLanguage`/`claimsFile` calls
    succeed. *Named revert:* ▸ change `engine.init?.(quartoAPI)` to a
    non-optional `engine.init(quartoAPI)` (or assert-present) → loading an
    init-less engine throws → the "loads cleanly" assertion RED.
- [x] Test: pure namespaces are callable from inside `init()`. A test
    engine calls `quarto.text.lines("a\nb")` and `quarto.markdownRegex`
    methods inside `init()`; verify no throw **and** the returned value is
    correct (`text.lines("a\nb") === ["a","b"]`) — not merely "did not throw",
    so a stubbed-out namespace can't pass it. *Named revert:* ▸ build the API
    without wiring the real `@quarto/api` `text`/`markdownRegex` namespaces (hand
    `init` an object whose methods throw) → the value assertion RED.
- [x] **Test: `path`/`system` are ambient — available before any `launchEngine`
    (RTQ Item A T-A1).** Deliver `Init { global }`, send `loadEngine`, then —
    *without* any `launchEngine` — call `quarto.path.runtime()`,
    `quarto.path.resource(...)`, `quarto.path.dataDir()`, `quarto.system.pandoc(...)`.
    At 1b's landing their **bodies** are still §2aa stubs
    (`notYetImplementedError("Plan 2")`, RTQ B3), so assert each throws the **"not yet
    implemented"** stub error — **not** a launch-gating error, and the call is reachable
    pre-launch (there is no "before engine launch" gate). *Named revert:* re-introduce a
    `state.context`-unset gate on these → the pre-launch calls throw a *gating* error →
    assertion RED. (Once Plan 2 lands the real bodies, this test asserts real values
    pre-launch.)
- [x] Test: `quarto.format.*` is pure — `quarto.format.isHtmlCompatible(format)`
    returns a real value with no launch and no context (format reads no context; it
    arrives per-`Execute`). There is no no-arg form. (Real in §2aa, needs no launch.)
    *Named revert:* ▸ wrap `format.*` in a launch/context gate (throw unless a
    context slot is set) → the pre-launch `isHtmlCompatible(format)` call throws →
    the "returns a real value with no launch" assertion RED.
- [x] **Test: `LaunchEngine` carries the *project* context; `launch()` receives it
    (RTQ Item A T-A2).** A test engine records the `context` arg its `launch()` gets;
    send `Init { global }` + `loadEngine` + `launchEngine { engine, project: {projectDir,
    isSingleFile, config, output_dir} }` where **`config` and `output_dir` are given
    deliberately *different* values** (e.g. `config: { "output-dir": "_site" }` —
    relative, as declared — and `output_dir: "/abs/proj/_site"` — resolved). Assert
    `launch()` saw an `EngineProjectContext` whose four fields match the `project`
    sent on `launchEngine` (not derived from `Init`), and **specifically that
    `config["output-dir"]` is the declared `"_site"` while `getOutputDirectory()`/
    `output_dir` is the resolved `"/abs/proj/_site"`** — the two are not collapsed
    into one value. *Named reverts:* ▸ stop threading `LaunchEngine.project` into
    `launch()` (pass `undefined`) → the "matches project" assertion RED. ▸ source
    *both* the declared and resolved output dir from the **same** field (collapse
    `config`'s `output-dir` and `output_dir`) → the "declared `_site` vs resolved
    `/abs/proj/_site`" distinctness assertion RED.
- [x] **Test: no shared cross-engine context state (RTQ Item A T-A4).** Two engines A,
    B; deliver `Init { global }`; `loadEngine` both; `launchEngine` A only.
    **Discriminator (avoids the stub-collapse vacuity): compare B's *actual thrown
    error identity* before vs. after A's launch — not merely "both throw."** At 1b's
    landing `path.runtime` is a `notYetImplementedError("Plan 2")` stub, so assert B's
    `quarto.path.runtime()` throws the **same `notYetImplementedError` stub before A's
    launch as after** (string-equal the two errors) — it never flips from a gating
    error to a stub across A's launch, because nothing A does mutates B's world.
    *Named revert:* re-introduce a shared mutable context slot **set by A's
    `launch()`** plus a gate that consults it → B's *pre*-A-launch `path.runtime()`
    throws a *gating* error while its *post*-A-launch call throws the
    `notYetImplemented` stub (gate now satisfied) → before ≠ after → the "identical
    error across A's launch" assertion RED. (Comparing error identity — not "both
    threw" — is what keeps this row binding at 1b despite the stub bodies; once Plan 2
    lands real bodies it asserts identical real values.)
- [x] Test: `init()` throwing is a load failure. The harness sends
    an `error` response for the `loadEngine` message and does not
    register the discovery surface. *Named revert:* ▸ remove the try/catch around
    `engine.init?.()` that maps a throw to an `error` response (let it propagate /
    register anyway) → `loadEngine` returns `loaded` and the engine is registered →
    both the "error response" and "not registered" assertions RED.
- [x] Test: `async init()` is awaited defensively. A test engine
    declares `async init()` and resolves after a tick; verify that
    `loaded` is sent only after the resolution, and any error from
    the rejection is reported. *Named revert:* ▸ drop the defensive `await` on
    `init()`'s result → `loaded` is written before the async `init` resolves → the
    "loaded only after resolution" ordering assertion RED.
- [x] Test: module top-level code that accesses `quarto.*` fails (no
    global available). Verify the failure mode is a clean load error,
    not silent corruption. *Named revert:* ▸ re-introduce a global
    `getQuartoAPI()`/registry populated before `import()` → module-top-level
    `quarto.*` access succeeds instead of failing → the "clean load error"
    assertion RED (binds "No global `getQuartoAPI()` registry").

- [x] **Test: harness idempotency contract** (plan1a-engine relies on this
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

- [x] **Test: harness idempotency under *concurrent* repeat** (this is the
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

- [x] **Test: return-based `htmlDependencies` forwarding.** A test engine
    **returns** `{ …, htmlDependencies: [{ name, stylesheets, scripts }] }`
    from `execute()` (one entry with a relative `scripts`/`stylesheets` path);
    assert the entries appear on the response's `htmlDependencies` array, with
    the relative paths normalized to the **expected absolute path** under
    `lib_dir` (assert the exact resolved string, not just "is absolute"). (No
    `quarto.htmlDependency()` call — there is no such method; deps ride the return
    value.) *Named reverts:* ▸ drop the `result.htmlDependencies` read off the
    returned object → the response's `htmlDependencies` is empty → the "entries
    appear" assertion RED. ▸ drop the relative-path normalization against `lib_dir`
    → the path stays relative → the "resolved absolute path" assertion RED.

### Phase 3: Supporting modules

- [x] Create `src/deno-host.ts` — the `PlatformHost` implementation used by
    `@quarto/api` factory exports. **`PlatformHost` is a q2-original
    abstraction, not a Q1 port** — Q1 has no host-injection seam (it hardwires
    `Deno.*` inside each namespace's backing functions). The `./platform`
    subpath and the `PlatformHost` interface are defined by Plan 2A §2aa; this is
    the seam that later lets a
    `@quarto/engine-host-wasm` supply a VFS-backed host without touching
    `@quarto/api`. **`denoHost` must implement the full landed interface**
    (`ts-packages/quarto-api/src/platform/index.ts`), not just the subset sketched
    below. The sketch omits these required (non-optional) members:
    `cwd()`, `fs.{ensureDir, makeTempDir, makeTempFile, remove}`,
    `process.{exec (structured `ExecOptions`/`ExecResult`), onExit, exit}`,
    `env.get`, and `log.{info, warning, error}` (plus the optional
    `log.clearLine?`) — the temp-context and cleanup namespaces won't construct
    without them. (Verified against the landed
    `ts-packages/quarto-api/src/platform/index.ts`; `cwd()` is mandatory with no
    default, and `env.get`/`realPath` are present for the deferred
    `path.runtime`/`dataDir` bodies even though nothing calls them in v1.)
    > **Forward-compat: give `makeTempDir` an `opts` object now.** Plan 2 widens
    > `makeTempDir` with an optional `suffix` (its `TempContext` work), landing
    > *after* 1b. Because the param is *optional* this is not the hard ordering
    > problem `walk` was — but write `denoHost.makeTempDir(opts?: { … })` to take an
    > options object from the start (threading `dir`/`prefix` as the interface
    > already declares) so Plan 2's `suffix` is a **body-fill, not a signature
    > change**. Same shape-ahead courtesy for any other temp-context member the
    > §2aa interface already declares with room to grow.
  ```typescript
  import type { PlatformHost } from "@quarto/api/platform";
  import { walkSync } from "jsr:@std/fs";
  export const denoHost: PlatformHost = {
      fs: {
          readTextFileSync: Deno.readTextFileSync,
          writeFileSync: (p, c) => Deno.writeFileSync(p,
              typeof c === "string" ? new TextEncoder().encode(c) : c),
          exists: (p) => { try { Deno.statSync(p); return true; } catch { return false; } },
          // RTQ/Plan-3 consumer: jupyter assets() (see walk work item below)
          walk: (root, opts) =>
              [...walkSync(root, {
                  maxDepth: opts?.maxDepth,
                  includeDirs: opts?.includeDirs ?? false,
              })].map((e) => ({ path: e.path, isFile: e.isFile, isDirectory: e.isDirectory })),
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

- [x] **Add `PlatformHost.fs.walk` — interface member *and* `denoHost` impl, both
    owned by 1b.** Plan 3's `@quarto/api/jupyter` `assets()` ports Q1's
    `jupyterAssets` (`jupyter.ts:665-696`): `ensureDir(figures_dir)` + a directory
    walk to promote the supporting dir. The landed `PlatformHost.fs`
    (`ts-packages/quarto-api/src/platform/index.ts`) has no enumeration primitive,
    so Plan 3 can't implement `assets()` as a pure consumer without one. **1b owns
    the whole addition** — it is the first plan that needs `walk` to exist (to
    implement `denoHost.walk` and to typecheck `denoHost: PlatformHost`), so 1b adds
    the **one interface member** alongside its **Deno body** in lockstep. This keeps
    the seam self-contained and removes any cross-plan ordering question: the member
    exists the moment 1b lands, and Plan 3 consumes a method that is already there.
    - **Interface** — add to `PlatformHost.fs` in
      `ts-packages/quarto-api/src/platform/index.ts` (alongside `ensureDir`/`remove`/
      …; **synchronous, no `Sync` suffix** to match them):
      ```typescript
      // PlatformHost.fs
      walk(root: string, opts?: { maxDepth?: number; includeDirs?: boolean }):
        Array<{ path: string; isFile: boolean; isDirectory: boolean }>;
      ```
    - **`denoHost` impl** — back it with `walkSync` from `jsr:@std/fs`, mapping each
      `WalkEntry` to `{ path, isFile, isDirectory }` (sketch above). Default
      `includeDirs: false` (files-only) so the common "promote the figures" case
      needs no flag.
    - **Consumer** — Plan 3's `assets.ts` calls `quarto`'s host `fs.walk`; the
      WASM host (`@quarto/engine-host-wasm`, future) supplies its own VFS-backed
      `walk` against the same member. (Cross-ref to the Plan 3 reviewer: 1b now owns
      both halves; nothing for Plan 2 to add.)

- [x] **Test: `denoHost.fs.walk` enumerates a real tree.** *Tier:* **`deno test`**
    (real filesystem; `walkSync`/`Deno.*` are Deno-only — this is the one module that
    cannot run under the vitest/Node tier, so it lives in a small `deno test` suite
    that the provisioned Deno (Phase 4 CI item) runs). *Real unit:* `denoHost.fs.walk`.
    *Seam:* `makeTempDir`, write `a.txt`, `sub/b.txt`, and an empty `sub/deep/`
    dir; call `denoHost.fs.walk(root)` and `denoHost.fs.walk(root, { maxDepth: 1 })`
    and `{ includeDirs: true }`. Assert: default returns the two files (absolute
    `path`, `isFile: true`, `isDirectory: false`) and **not** the directories;
    `maxDepth: 1` omits `sub/b.txt`; `includeDirs: true` includes `sub`/`sub/deep`
    with `isDirectory: true`. *Mock boundary:* none — real temp dir. *Named reverts:*
    ▸ hard-code `isFile`/`isDirectory` (e.g. always `isFile: true`) in the
    `WalkEntry` mapping → the `isDirectory` assertion under `includeDirs: true` RED.
    ▸ drop the `maxDepth` pass-through to `walkSync` → `sub/b.txt` appears under
    `maxDepth: 1` → that assertion RED. ▸ drop the `includeDirs` default/flag → the
    "default excludes dirs" (or "includeDirs includes them") assertion RED.

- [x] Create `src/quarto-api.ts` — QuartoAPI builder over the `Init { global }` config:
  - Define `HostGlobalConfig` (resource/runtime/data dirs, pandoc, version,
    interactive/CI) — received once on the `Init` frame at spawn.
  - Export `buildQuartoAPI(global: HostGlobalConfig, host: PlatformHost): QuartoAPI`
    that returns a plain nested record (no registry pattern — Q1's
    `QuartoAPIRegistry`/`register.ts` is deliberately not ported).
  - The builder threads `global` and `host` through the `@quarto/api` factories.
    **Every namespace resolves immediately** — `path`/`system` close over `global`
    (so `path.runtime`/`resource`/`dataDir`/`system.pandoc` are ambient), pure
    namespaces need nothing. **No gating, no `state.context`** (RTQ Item A). The
    per-render `project` (`EngineProjectContext`) is NOT read by the API — it goes
    to `engine.launch()` and lives in the instance closure (pure Q1).
  - The harness builds one QuartoAPI at startup (right after `Init`) and hands it
    to each engine via `engine.init?.(quartoAPI)` on its `loadEngine`.
  - **What's real at 1b vs. deferred.** 1b's contract tests exercise concrete
    behavior, so the namespaces they touch must be *real* — Plan 2A §2aa delivers
    the pure namespaces `text`, `markdownRegex`, `format`, `crypto` and the
    host-only `console`, `path`, `system`, `mappedString` (IO through the injected
    `PlatformHost`). The `jupyter` namespace stays `notYetImplementedError("Plan 3")`
    until Plan 3, and the four deferred method *bodies*
    (`path.runtime`/`resource`/`dataDir`, `system.pandoc`) stay
    `notYetImplementedError("Plan 2")` until Plan 2 — a missing body, not a gate.
  - **`text` and `mappedString` are separate top-level namespaces** in
    Q1's `QuartoAPI` (`core/api/types.ts`: `mappedString: MappedStringNamespace`
    at :238, `text: TextNamespace` at :243) — `mappedString` is NOT a
    subset of `text`. Earlier drafts said both "pull from
    `@quarto/api/text`"; that's wrong. `text` has `lines`,
    `trimEmptyLines`, `lineColToIndex`, …; `mappedString` has
    `fromString`, `fromFile`, `splitLines`, `indexToLineCol`,
    `normalizeNewlines`. `mappedString.fromFile` is the one host-backed
    method (reads disk via the `PlatformHost`); the rest are pure.

- [x] **Q2-native structured deps are a return field, not a registration call.**
    There is **no `quarto.htmlDependency()` API method.** An imperative registration
    helper would have to stash registrations in mutable state on the single shared
    QuartoAPI, cleared before each `execute()` and drained after —
    which **races** under parallel Pass-2: two engines' `execute()`s interleave
    across `await`s and write the same slot, so deps get dropped or misattributed.
    Q1 has no such mechanism in its engine protocol — every engine output is a field
    on a freshly-returned result object, the `quarto` API is shared but never mutated
    to carry results (`core/api/types.ts:38-40`), and Q1's render loop is serial
    (`render-files.ts:316-345`). The plan had imported a mechanism Q1 doesn't have
    into a newly-concurrent context; the race was the port's own creation. So:

    A q2-native engine emits structured deps as an optional field on the value it
    **returns** from `execute()`:
    ```typescript
    // engine side — return-based, no shared state:
    return { ...q1ExecuteResult, htmlDependencies: [{ name, stylesheets, scripts }, …] };
    ```
    where `htmlDependencies?: HtmlDependency[]` (`{ name: string; stylesheets?:
    string[]; scripts?: string[] }`). The harness reads `result.htmlDependencies`
    off the returned object and forwards it to `TsExecuteResult.htmlDependencies`
    (Phase 2 step 6). This is the **single** q2 deviation from Q1's result shape, and
    it is return-based — matching Q1's "deps are return values" philosophy.
    - **`HtmlDependency` type ownership (gap — must be created).** This type does
      **not** currently exist anywhere in `ts-packages/` (it is absent from
      `@quarto/types`). 1b **adds it to `@quarto/types`** (shared engine-author SDK
      surface, mirroring the Rust `TsHtmlDependency` in plan1a-protocol), and both
      the engine SDK and `src/types.ts` import it from there — do **not** redefine
      it locally in the harness.
    - **Relative-path normalization stays.** The harness normalizes relative
      `stylesheets`/`scripts` in the returned list against `TsExecuteOptions.lib_dir`
      before emitting on the wire (plan1a-protocol's path contract: absolute, on-disk).
    - **No "called outside `execute()`" error** — there is no method to misuse. An
      engine that wants imperative ergonomics internally keeps its **own**
      module-level list and drains it into its own return value; that private state
      is safe because that engine's `execute()`s are serialized by its per-engine
      queue and is never shared across engines.
    - plan1a-engine's q2-side `store_html_dependencies`
      (`crates/quarto-core/src/dependency.rs`) dedupes by `name` (first-wins) with a
      **content-equality check** — identical re-registration is skipped silently; a
      name collision with *differing* content drops the later one and emits one
      `DiagnosticMessage::warning` (RTQ ENG-2). Framed as deduping the **returned**
      list (across the engines whose results land in one render), not cross-engine
      "registrations." **Status: landed** — the content-check + warning are
      implemented; ENG-2 only reconciles the doc-comments to match.
    - This return field is the only sanctioned route to populate
      `executeResult.htmlDependencies`. Q1's `engine.dependencies(...)` output goes
      to `executeResult.includes` (Phase 2's execute-dispatch flow); the harness
      MUST NOT auto-translate Q1 deps into `htmlDependencies`.

- [x] Create `src/mapped-source.ts` — MappedString rehydration from
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

- [x] **MappedString serialization for `markdownForFile` (Deno → Rust):**
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

- [x] Create `src/engine-loader.ts`:
  - Dynamically import the engine module: `await import(toFileUrl(path))`
  - Validate it has a default export with `name`, `claimsLanguage`, `launch`
  - Return the `ExecutionEngineDiscovery` object

- [x] Create `src/framing.ts` — originally the v1 channel plumbing `host.ts`
    imported (stdin/stdout), since replaced by the Phase 1.6 loopback-TCP
    plumbing:
  - `readFrames(stream): AsyncIterable<Request>` — newline-framed JSON decoder
    yielding parsed `Request` envelopes (`{ id, msg }`); originally from
    `Deno.stdin` (v1), now from the loopback-TCP connection (Phase 1.6).
  - `writeFrame(out, response)` performs an **async** write (`await out.write(line)`)
    serialized under an `AsyncMutex` (`writeMutex`). Async (not `writeSync`) so a
    large frame yields and the read loop keeps draining the channel — the
    continuous-drain property that prevents a large-payload deadlock. The mutex
    serializes concurrent dispatch tasks' writes so two frames never interleave
    across the `await`. `AsyncMutex` is a tiny hand-rolled promise-chain
    serializer (each acquirer chains onto a `tail` promise), **not** a
    dependency.
  - In the original v1 design, a stray engine `console.log` was a *separate*
    complete line on stdout, caught as malformed by the Rust demux — the v1
    stdout contract. Phase 1.6 has since landed and retired this: stdout is
    diagnostic-only, and `console.log` is harmless.
  - **(Phase 1.6 — landed)** a `connectControl(args)` that parses
    `--control <addr>` and reads the one-time token as the first line on
    stdin, dials `std::net`-bound **loopback TCP**
    (`Deno.connect({ hostname: "127.0.0.1", port })`), and presents the token
    as a pre-line on the socket — replacing stdin/stdout as the channel and
    freeing stdout for diagnostics. **Not UDS / named pipe** (see plan1a-host
    "Phase 1.6 — the protocol moved off stdout (loopback TCP) — LANDED").

- [x] Create `src/types.ts` — protocol message type definitions (must match
    the Rust enums **in their post-RTQ form** exactly), **including the Phase 1.5
    envelope** (`Request = { id: number, msg: ToEngine }`,
    `Response = { id: number, msg: FromEngine }`), the `Cancel { target }` /
    `Cancelled` control messages, and the **RTQ Item-A `Init { global:
    HostGlobalConfig }`** variant on `ToEngine` (response-less — there is **no**
    `FromEngine` counterpart) plus the `Dependencies` verb / `DependenciesResult`
    (FC-2).
    > **Build-order coordination.** `src/types.ts` must mirror the *post-RTQ*
    > `ts_protocol.rs` — the `Dependencies` verb, `engineDependencies` /
    > `dependencies`, the `Init { global }` / `EngineProjectContext` shapes, the
    > ENG-1 statics on `LoadEngineResult`, and the FC-1 carrier fields — none of
    > which exist in the pre-RTQ wire today. Author these against RTQ's specified
    > shape, and treat **RTQ's serde field/rename freeze as the gate** for landing
    > this file: if `types.ts` lands before RTQ finalizes the exact wire names, the
    > two will drift and the 1c E2E will fail on a serde mismatch. (Pure-TS modules
    > with no protocol-field surface — `framing.ts`, the loop mechanics — can land
    > earlier; this file specifically waits on RTQ's name freeze.)

### Phase 4: Bundle + CI

- [x] **Add `cargo xtask build-engine-host-bundle`** (esbuild) that regenerates
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
- [x] **Staleness diagnostic.** Mirror `q2 mcp --launcher-info`: surface the
    embedded bundle's **git commit, dirty flag, and build time** (e.g. baked in by
    the xtask) via a `--launcher-info`-style flag, so a `cargo build --bin q2`
    that re-embedded a stale on-disk bundle is detectable. (A plain `cargo build`
    re-embeds whatever bytes are on disk — same trap as the MCP/SPA bundles.)
- [x] Add a CI check that verifies the checked-in bundle is up to date with the
    sources. **Do not byte-compare a fresh build against the committed file** —
    esbuild output is not guaranteed byte-stable across versions/platforms/line
    endings, so naive byte-equality is flaky. Instead: (1) **pin the exact esbuild
    version** in `devDependencies` (no `^`/`~`), and (2) have CI run the xtask then
    `git diff --exit-code -- ts-packages/quarto-engine-host-deno/dist/`, failing if
    the rebuild changed the committed bytes. The pin makes the rebuild
    deterministic enough for the diff to be a true "you edited TS but forgot to
    rebuild" signal rather than cross-environment noise.

- [x] **Provision Deno in CI + `dev-setup` (1b owns this — it is the first plan
    whose deliverable needs a real Deno to be validated).** `ts_process.rs`
    locates Deno by running `deno --version` on `PATH` (`is_available()`,
    `:149`); when it is **absent the real-subprocess tests SKIP**
    (`is_available()` guards at `:485`, `:1978`). plan1a-host shipped with that
    skip plus `MockTransport` coverage, so its CI gap was benign. **It stops being
    benign at 1b/1c:** the real harness JS and the Plan-1c echo E2E have no mock
    substitute — on a Deno-less runner they skip and CI goes green **without ever
    running the harness** (the tests-pass-but-feature-broken trap CLAUDE.md warns
    about). Concretely:
    - **`test-suite.yml`** (runs `cargo nextest run --tests --cargo-profile ci`
      on `ubuntu-latest` + `macos-latest`): add a **Set up Deno** step before the
      "Test Rust code" step, mirroring the existing per-OS "Set up Pandoc /
      tree-sitter / minisign" steps. Use `denoland/setup-deno@v2` with a **pinned**
      `deno-version:` (no floating range) — one cross-platform step covers both
      OSes. (`hub-client-e2e.yml` needs it too **iff** the 1c browser E2E reaches a
      TS engine; add there only if so.) The vitest/Node tests (T1–T7, the
      Engine-API contract rows) run over the in-memory duplex and need **no** Deno
      (see the entry-point split in Phase 2). **But the `denoHost` `deno test` suite
      *does* need Deno** — `denoHost.fs.walk`/`exists`/etc. call `Deno.*` /
      `jsr:@std/fs` and cannot run under Node. So `ts-test-suite.yml` (or wherever
      the TS suites run) gains a `deno test` step for `src/deno-host*.test.ts`,
      gated on the same provisioned Deno; it is the one TS leg that requires it.
    - **Fail, don't skip, in CI.** Add a single nextest test (e.g.
      `ci_requires_deno_when_QUARTO_CI`) that, when `QUARTO_CI=1` is set in the
      workflow env, asserts `ts_process::is_available()` is `true` — so a
      regression in the setup step (or a runner without Deno) turns the silent
      skip into a **hard red**, while local dev with no Deno still just skips the
      spawn tests. (Local dev: Deno on `PATH` — e.g. `brew install deno` — is
      sufficient; no env var, no version floor, the harness shells
      `deno run --allow-all` with no extra config.)
    - **`cargo xtask dev-setup`**: add Deno to the dev-tools it provisions (it
      already installs cargo-nextest / wasm-bindgen-cli; bd-7giz tracks extending
      it). Document the `brew install deno` / `deno.land` fallback in the plan's
      "engine-author documentation" follow-up.
    - **Pin a minimum Deno version** somewhere checked (the setup-deno pin is the
      de-facto floor); `is_available()` only proves it runs, not that it is new
      enough for `Deno.Command` / `Deno.stdin.isTerminal()` / Phase-1.6
      `Deno.connect`.

## Dependency on RTQ

Per the RTQ reviewer's 2026-06-29 "Option A" split, **RTQ is now scoped to the
landed-1a Rust corrections only** (the protocol split + the Rust-side
consumption), and **Plan 1b is the sole owner of every harness (Deno/TS)
behavior** — RTQ points to 1b, not the reverse. 1b consumes RTQ Item A's protocol
split, so **1b is gated on RTQ landing first** (see the header callout).

- **Confirmed RTQ-owned (not a 1b deliverable):** the two new wire structs
  (`HostGlobalConfig` on `Init`; `EngineProjectContext` on `LaunchEngine`), the
  `Dependencies` verb + `engineDependencies`/`dependencies` fields, the ENG-1
  discovery statics, the FC-1 carriers, **and the Rust-side `ts_engine.rs`
  field-routing that consumes them** (extend `:243 build_execute_options` for
  `dependencies: bool`; read the FC-1 carriers + `engineDependencies` off
  `TsExecuteResult` at `:463`; switch `EngineHostContext` → `EngineProjectContext`
  + send `Init { global }`). 1b's harness forwards/consumes these on the TS side;
  the Rust counterpart is RTQ's.
- **Q-C (resolved 2026-06-29) — `HostGlobalConfig.pandocPath` source.** q2 does
  **not** bundle pandoc (or any other engine tool); it uses the **pandoc found in
  the environment** (PATH discovery), the **same model q2 already uses for pandoc
  and for deno**. So `Init.global.pandocPath` is the environment-discovered pandoc
  path (RTQ/q2 populates it when building `Init`; `resourceDir` is q2's own
  resource directory). Plan 2's `system.pandoc` body therefore resolves to that
  environment pandoc. **Long term**, `pampa.wasm` is intended to replace
  `system.pandoc` (no external pandoc) — but the easy thing first: shell the
  environment pandoc. No open questions remain for RTQ.

## Design Notes

### Diagnostic stream (stderr) — original v1 design; superseded by Phase 1.6

In the original v1 design the protocol ran on stdout, so **stderr was the
diagnostic stream** (forwarded to q2's logging). The harness prefixes its own
log lines with level markers so q2 can route them:
```
[INFO] Checking Julia installation...
[WARN] Julia server connection slow
[ERROR] Julia process crashed
```
Unprefixed stderr lines (from the engine or Deno) are logged at INFO. **Phase
1.6 has since landed**: the protocol now rides a loopback-TCP connection, and
both stdout and stderr are diagnostic-only (see
`claude-notes/plans/2026-07-08-plan1a6-off-stdout-loopback-tcp.md`).

### Stdout/stderr contract — original v1 design; retired by Phase 1.6

**In the original v1 design the protocol ran on stdout**, one `Response` per
line, so the following contract held: engines must **not** write to stdout.
The harness captured the real `Deno.stdout` for protocol writes at startup and
did **not** override `console.*` — protection was by contract, not mechanism.

- **Use `quarto.console.*`** (preferred — `[INFO]`/`[WARN]`/`[ERROR]` prefixes,
  written to stderr) or `console.error`/`console.warn` (stderr) for diagnostics.
- In v1: **do not use `console.log`/`console.info` or `Deno.stdout.writeSync`**
  — these wrote to stdout and corrupted the protocol. The Rust side detected a
  non-`Response` line on stdout as malformed and SIGKILLed (plan1a-host
  category 9), naming `console.log` as the likely cause.
- In v1: **do not read `Deno.stdin`** — it was the protocol *input* channel; an
  engine reading it (e.g. an interactive prompt) stole frames the harness's
  read loop needed. The symmetric footgun to the stdout one.

**This contract is gone.** Phase 1.6 has landed and moved the protocol to a
loopback-TCP connection, freeing both stdin and stdout for the engine —
`console.log` is now a harmless INFO-routed line, and the capture/contract
described above no longer applies. `quarto.console.*` remains the preferred
diagnostic API regardless.

### Where is engine-host-deno.js at runtime?

The engine-host-deno harness is bundled into a single `.js` file using **esbuild**.

**Build pipeline:**
1. `ts-packages/quarto-engine-host-deno/esbuild.config.mjs` bundles `src/host.ts` → `dist/engine-host-deno.js`
2. The bundle is checked into git as a **generated build artifact** (the `q2 mcp` `dist-bundle/` / `q2-preview-spa` `dist/` pattern — commit the build *output*, not hand-written source like `reveal.js`). plan1a-host ships the path with placeholder content so `include_str!` compiles cleanly; Plan 1b replaces the placeholder with the real esbuild output. From cargo's perspective the committed bundle is just another `include_str!` input.
3. `include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ts-packages/quarto-engine-host-deno/dist/engine-host-deno.js"))` embeds it in the q2 binary (anchored at the `quarto-core` crate root — two `..`s reach the repo root — **not** a source-file-relative `"../../../../…"` from `ts_process.rs`)
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
(Plan 2A §2aa), with these still stubbed: the `jupyter` namespace, pending
**Plan 3** (`notYetImplementedError("Plan 3")`), and the four deferred method
*bodies* `path.runtime`/`path.resource`/`path.dataDir` and `system.pandoc`,
pending **Plan 2** (`notYetImplementedError("Plan 2")`) — each a *missing body*,
**not** a launch gate (RTQ Item A removed gating; B3 relabels the four body
stubs, today still `requiresLaunchContextError`, as part of Item A's §2aa
edit). They are ambient — reachable before *and* after `launchEngine` — they
simply have no body yet. So the initial bundle is somewhat smaller than the
steady-state size below.

**Bundle size note (post-Plan-3):** The bundle may be large (200-500 KB
estimated, depending on `@quarto/api/jupyter` complexity). The
engine-host-deno bundle is gated behind
`#[cfg(not(target_arch = "wasm32"))]` so WASM builds don't carry it.
Flagged as a possible future concern — if the bundle grows problematically,
options include a cargo feature flag to gate the embed, or loading from a
known filesystem path instead of embedding. For now, embedding is the
simplest approach and matches q2's existing generated-bundle embeds
(`q2 mcp`'s `dist-bundle/`, the preview SPA's `dist/`).

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

- The QuartoAPI contract: which methods are pure, which are host-only, the
  ambient `path`/`system` methods (closed over the `Init { global }` config), and
  the `init(quartoAPI)` reference-stashing pattern. (Nothing is gated behind
  `launchEngine` — RTQ Item A.)
- Module top-level access prohibition (no `quarto.*` outside methods).
- Diagnostics: use `quarto.console.*` (level-routed) or `console.error`/`.warn`
  (stderr) as the preferred style. **In the original v1 design, `console.log`/
  `console.info` were forbidden** — they wrote to stdout, then the protocol
  channel, and corrupted it. Phase 1.6 has since landed, moving the protocol
  off stdout — `console.log` is now harmless (forwarded as a diagnostic), so
  this prohibition no longer applies.
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

- [x] `@quarto/engine-host-deno` package exists with package.json,
  esbuild.config.mjs, tsconfig
- [x] Harness dispatches every protocol message type from the `ToEngine`
  enum (`LoadEngine`, `LaunchEngine`, `Shutdown`, `ClaimsLanguage`,
  `ClaimsFile`, `MarkdownForFile`, `Execute`, `IntermediateFiles`, `Cancel`, and
  the new `Dependencies` verb added by RTQ FC-2)
- [x] Two-step lifecycle works: `LoadEngine` produces a discovery surface
  without launching; `LaunchEngine` produces an instance; messages requiring
  the wrong state return a clear error
- [x] `target()` handled as harness-internal (never reaches the protocol)
- [x] deferred-deps infra built (RTQ FC-2): `dependencies: bool` (default true) on
  `TsExecuteOptions`; under `false`, `execute()`'s `engineDependencies` is
  **forwarded** on `TsExecuteResult` (the harness does **not** fold); the harness
  handles the new `dependencies` verb as a thin pass-through to
  `engine.dependencies()`, replying `dependenciesResult { includes }`. q2's render
  orchestrator (not the harness) drives the round-trip per `render.ts:90-109`;
  `DependenciesResult.includes` → q2 `includes` (not `htmlDependencies`); inert for
  real engines until Plan 3E provides `widgetDependencyIncludes`
- [x] Every Q1 `ExecuteResult` field is routed deliberately (step 7): in
  particular **`supporting` is forwarded to `TsExecuteResult.supporting`** (→
  Rust `ExecuteResult.supporting_files`, copied to output by the orchestrator —
  engine figures must not be orphaned) and `filters` is forwarded; `metadata` /
  `pandoc` / `resourceFiles` / `preserve` / `postProcess` / `engineDependencies`
  are **carried on the wire as `#[serde(default)]` inert carriers** (RTQ FC-1/FC-2),
  not dropped; only `engine` is a true drop (known from message routing)
- [x] `partitionedMarkdown` is **not** dispatched by the harness — it is
  not a protocol message in q2 (`DocumentProfile` covers the Q1 use
  cases; see grand plan and ipynb-filters research plan)
- [x] MappedString rehydration from `source_map` works end-to-end — a
  `.map(index)` call returns `{ index, originalString }` pointing at the
  correct file and offset even through include boundaries; the
  base-per-file cache is per-rehydration-call (not cross-message)
- [x] MappedString serialization for `markdownForFile` responses is
  implemented on the TS side (one-entry-per-piece, no coalescing); the
  `source_map` rides the wire **unconsumed in v1** — Rust-side
  `SourceInfo::Concat` reconstruction is A′-deferred (plan1a-engine SEAM-3 / C′)
- [x] Multiplexed dispatch: non-blocking read loop over stdin/stdout (v1);
  responses echo the request `id`; cross-engine requests run concurrently while
  same-engine requests serialize on a per-engine queue (T3, T5)
- [x] Cooperative cancellation: `Cancel { target }` aborts exactly that request's
  `AbortController` (resolving it `Cancelled`) without affecting siblings; no
  SIGINT handler; whole-subprocess SIGKILL stays Rust-side and reserved for
  crash/compromised-channel/teardown (T6)
- [x] Concurrent same-instance poison → **transparent re-launch**: a queued
  same-engine `Execute` whose instance was poisoned reconstructs the instance
  (`engine.launch(stashedProject)`) and completes normally — it is not failed (T7)
- [x] `target()` is called fresh per `Execute` message; results are not
  memoized
- [x] Ambient QuartoAPI (RTQ Item A): `init?(quarto)` is called at `loadEngine`;
  **every** namespace resolves immediately — `path.runtime`/`resource`/`dataDir`
  and `system.pandoc` close over the `Init { global }` config (never gated); the
  four deferred bodies throw `notYetImplementedError("Plan 2")` until Plan 2 (a
  missing body, not a gate). No `state.context` and no per-launch unblocking; the
  per-render `project` rides `launchEngine` into the instance closure
- [x] Protocol ran on stdin/stdout (v1); harness captured `Deno.stdout` for
  protocol writes; stderr was the diagnostic stream; the stdout contract held
  at the time (`console.log` forbidden — corrupted the protocol). Phase 1.6
  has since landed, moving the protocol to loopback TCP and retiring that
  contract — `console.log` is now harmless.
- [x] Lifecycle methods that are NOT on the protocol (`filterFormat`,
  `executeTargetSkipped`, `postprocess`, `canKeepSource`, `postRender`)
  are simply not dispatched — the harness has no top-level handler for them
- [x] `denoHost: PlatformHost` in place (importing the q2-original
  `PlatformHost` from `@quarto/api/platform`, Plan 2A §2aa); `quarto-api.ts`
  builds the ambient QuartoAPI (over the `Init { global }` config, no gating —
  RTQ Item A) over the nine Q1 namespaces. The
  pure + host-only namespaces 1b's tests exercise (`text`,
  `markdownRegex`, `format`, `crypto`, `console`, `path`, `system`,
  `mappedString`) are real (from `@quarto/api`, Plan 2A §2aa); only `jupyter`
  and launch-context method bodies may throw "not yet implemented"
  pending Plans 2/3
- [x] **`PlatformHost.fs.walk` lands in 1b — both the interface member (added to
  `@quarto/api`'s `PlatformHost.fs`) and the `denoHost` impl (`walkSync`-backed),
  with the `deno test` covering it** (files-only default, `maxDepth`, `includeDirs`);
  Plan 3's jupyter `assets()` consumes it, nothing left for Plan 2 to add
- [x] Bundle builds cleanly with `npm run build`, produces
  `dist/engine-host-deno.js`, and the rebuilt bundle is committed to
  git (replacing plan1a-host's placeholder)
- [x] CI check verifies the checked-in bundle matches what
  `npm run build` produces — catches "edited TS but didn't rebuild"
  drift
- [x] **Deno is provisioned in CI** (`test-suite.yml` pinned `setup-deno`) and in
  `cargo xtask dev-setup`; a `QUARTO_CI=1`-gated test makes the real-subprocess
  path **fail (not skip)** when Deno is absent, so green CI actually exercises the
  harness (and the Plan-1c echo E2E)
- [x] **`HtmlDependency` added to `@quarto/types`** (the return-based deps field's
  type; absent from the tree today)
- [x] **`host.ts` is split into a stream-injected `runHost(reader, writer, host)`
  core + a thin Deno `main()`** — T1–T7 run under vitest/Node over an in-memory
  duplex with no `Deno.*` at module scope
- [x] Harness idempotency contract test passes (`loadEngine` /
  `launchEngine` repeats are no-ops; backs plan1a-engine's naive-OnceLock
  reasoning)
- [x] Concurrent idempotency holds: K (≥3) same-name `loadEngine` frames
  in flight at once run `import()`/`init()` exactly once with identical
  `LoadEngineResult`, and K same-engine `launchEngine` frames in flight run
  `engine.launch()` exactly once with identical `LaunchEngineResult` — this is
  the "engine.launch() invoked exactly once across the real harness" assertion
  plan1a-engine defers to Plan 1b for its parallel-Pass-2 `OnceLock` fan-out
- [x] q2-native structured deps are **return-based**: the engine returns an
  optional `htmlDependencies` field on its `execute()` result; the harness
  forwards it to `executeResult.htmlDependencies` (no `quarto.*` registration
  method, no accumulator); relative paths normalize against `lib_dir`
- [x] Phase 0 Test Seam Spec tests pass and bind: T1 (metadata
  partition, incl. the `pdf-standard` order discriminator and the
  nested-bin peel), T2 (MappedString rehydration offset + `source: None`
  tolerance + closest scan), T3 (per-message-type dispatch + `id`
  correlation + the state-guard negative path), T4 (`markdownForFile`
  no-coalescing TS serialize only — Rust `SourceInfo::Concat` reconstruct is
  A′-deferred per plan1a-engine SEAM-3),
  T5 (cross-engine concurrency + same-engine serialization), T6 (cooperative
  `Cancel` aborts only the target), and T7 (concurrent same-instance poison →
  transparent re-launch, `engine.launch()` exactly once).
- [x] Engine-API contract tests pass and bind, **each with a named revert** —
  including the RTQ Item-A rows: **T-A5** (`Init` consumed, response-less, gates
  `loadEngine`), **T-A1** (`path`/`system` ambient pre-launch, discriminated by
  *error type* — stub vs gating), **T-A2** (`launch()` receives the project, with
  `config` declared vs `output_dir` resolved kept distinct), **T-A4** (no shared
  cross-engine state, discriminated by *error identity across A's launch* — not the
  stub-collapse "both throw"), and **F2b** (deferred-deps forwarding / no harness
  fold). Each test asserts a *discriminating* surface (not "did not throw"), and
  each names the production hunk whose revert reddens it.
