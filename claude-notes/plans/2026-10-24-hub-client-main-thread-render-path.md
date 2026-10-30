---
title: 'hub-client main-thread render path: audit fixes'
date: 2026-10-24
description: 'Phased fixes from the 2026-10-24 main-thread render-path audit: render coalescing, parse-check-only classification, single-parse AST transport, binary asset transfer, then measurement-gated worker work.'
status: draft
---

## Overview

An audit on 2026-10-24 examined what heavy, general-purpose work runs on
the browser main thread on the hub-client render path, and what can be
removed, coalesced, or moved off it. Context: the entire Rust WASM render
pipeline (`wasm-quarto-hub-client`: VFS, qmd parse, transform pipeline, AST
production, theme CSS compile, LSP intelligence) runs on the main thread;
the only workers are pandoc.wasm, typst, and Monaco language workers
(architecture map: `claude-notes/designs/pandoc-wasm-architecture.md`,
which documents main-thread placement as an accepted decision). Every bit
of general work sharing that thread shows up as typing latency and preview
lag.

Eight findings, ranked by expected impact, are below. This plan sequences
the fixes cheapest-first, with measurement gating the larger ones per
`claude-notes/instructions/performance-profiling.md`.

Caveat: findings are from static analysis plus code-path reading. The cost
claims are hypotheses until Phase 0 instruments them. File:line references
were as of the audit; re-locate before editing.

Revised 2026-10-27: file:line references re-verified against main; updated
for bd-73lilznv (2026-10-26: the sandboxed preview's default origin moved
to `quarto-dev.github.io/q2-sandbox`, so the preview iframe is now
genuinely cross-origin — this affects findings 3 and 6 and Phases 3-4) and
for the TS-side perf instrumentation that has landed since the audit
(`hub-client/src/utils/perfCounters.ts`). Phase 3's per-consumer scoping is
now decided (manifest moves iframe-side; the other consumers stay, with
reasons recorded inline).

## Findings

| # | Finding | Primary site | Cost class |
|---|---------|--------------|------------|
| 1 | Per-keystroke full parse whose AST is discarded (format classification) | `hub-client/src/components/render/PreviewRouter.tsx:134` | wasted work per keystroke |
| 2 | Renders are debounce-free; stale queued renders still execute | `hub-client/src/components/render/ReactPreview.tsx:768`, `ts-packages/preview-runtime/src/wasmRenderer.ts:494` | N full pipelines per N-keystroke burst |
| 3 | AST serialized twice, parsed twice per render; untransformed AST always shipped | `ts-packages/preview-runtime/src/wasmRenderer.ts:611`, `hub-client/quarto-hub-sandboxed-preview/src/entry.tsx:330` | multi-MB JSON parse on main thread, discarded |
| 4 | Systemic JSON-string envelopes on every WASM call; binaries are base64-in-JSON | `ts-packages/preview-runtime/src/wasmRenderer.ts:331` and ~30 sites | boundary tax on every call |
| 5 | Whole-document semantic tokenization synchronous on main thread | `hub-client/src/services/intelligenceService.ts:263` | editor-blocking tree-sitter per token request |
| 6 | Asset proxy: base64 + JSON + string postMessage per asset | `hub-client/src/components/render/q2-sandboxed-preview/Q2SandboxedPreviewIframe.tsx:183` | ~33% bloat + copies per asset |
| 7 | Attribution: full Automerge history replay + char-to-byte map on main thread | `hub-client/src/hooks/useAttribution.ts`, `hub-client/src/services/attribution-runs.ts` | O(history) behind a toggle |
| 8 | dart-sass compiles on main thread via sync VFS bridge | `ts-packages/preview-runtime/src/wasmRenderer.ts:264` | ~500 ms per theme change, async so yielding |

Amplifier noted during the audit: per keystroke the full document text
crosses the JS-to-WASM boundary at least three times (sync callback
`ts-packages/preview-runtime/src/automergeSync.ts:108`, semantic-tokens
write `intelligenceService.ts:275`,
`resolvePandocFormats` contentOverride `PreviewRouter.tsx:147`) before the
render itself reads it. Folded into Phases 2 and 5 rather than tracked
separately.

Existing related strands: the closed bd-k9tfhsst covered SW asset-proxy
namespace plumbing, not the base64 wire format (finding 6 is new work).
Rust-side render CPU has its own perf strands (AST memmove, artifact
re-flush, template recompiles) — orthogonal to this plan.

## Conventions

- TDD per repo policy: behavior-preserving perf fixes rely on
  characterization tests written **before** the change; anything
  user-visible gets a failing regression test first.
- Measurement per `claude-notes/instructions/performance-profiling.md`:
  Phase 0 builds the harness; no phase past 1 starts without baseline
  numbers for its target. TS-side profiling uses the Node
  `--cpu-prof` driver workflow (never vitest), analysis via
  `hub-client/scripts/perf/analyze-cpuprofile.mjs`.
- After each phase: `npm run test:ci` and `npm run build:all` from
  `hub-client/`. Phases touching `crates/wasm-quarto-hub-client` (2, 3, 4)
  also need the WASM rebuild chain (`npm run build:wasm`,
  `cargo xtask build-q2-preview-spa` if the SPA is affected) and
  `cargo xtask verify`.
- The TS-side perf infrastructure Phase 0 calls for already exists:
  `hub-client/src/utils/perfCounters.ts` provides `QUARTO_PERF_STATS`-gated
  `perfMark`/`perfSegment`/`perfNow` (free when off), and
  `useAutomergeSync.ts` is already instrumented with it as a worked
  example. Extend it with the gauges below rather than building a parallel
  mechanism.
- End-to-end verification per AGENTS.md: each phase gets a manual smoke
  in the dev server (or states explicitly that browser verification was
  not done). Record the invocation and observed output.
- File a braid strand per phase when work begins; reference this plan.
  Snapshot-file changes (Phase 3 likely) get the AGENTS.md snapshot
  treatment: counts, summary, surprises called out.

## Phase 0 — Measurement harness and baselines

Build the TS-side profiling driver and record the baseline the later
phases are judged against. No production code changes in this phase
beyond instrumentation.

**Status: done 2026-10-29 (bd-k8ptcrd9).** Baselines below; later phases
record their before/after deltas against the "baseline" column.

- [x] Representative fixture: a mid-size qmd project document (order of
  100 KB source, several images, theme) plus a geometric scaling series
  (1x/2x/4x/8x body duplication) under a throwaway dir, per the
  profiling playbook. → generated fixture, ~99 KB at 1x / ~800 KB at 8x.
- [x] Standalone driver (`/tmp/...`, not committed): overrides
  `requestIdleCallback`, imports the render-path modules via tsx,
  simulates a keystroke burst (N edits, each triggering VFS write,
  format check, render enqueue, token request) against the real WASM.
  Run with `node --cpu-prof --cpu-prof-interval=200 --import tsx/esm`.
  Driver notes: a `file:` fetch shim lets the production `initWasm()`
  run unmodified; an ESM resolve hook maps `wasm-quarto-hub-client`,
  `/src/wasm-js-bridge/*` and `@quarto/*` workspace src; `?url` asset
  imports get a string shim; **`fake-indexeddb/auto` is required** —
  without it the WASM SASS cache bridge has no IndexedDB and every
  render recompiles the theme, overstating per-keystroke cost ~2x.
- [x] Gauges on the TS side via the existing
  `hub-client/src/utils/perfCounters.ts` (already `QUARTO_PERF_STATS`-gated
  and free when off), following the `perf.<name>` prefix convention:
  renders started, renders completed, renders discarded as stale, envelope
  `JSON.parse` bytes, VFS write count and bytes per keystroke, token
  requests. Implementation note: the module now exists as TWO copies
  (`hub-client/src/utils/perfCounters.ts` and
  `ts-packages/preview-runtime/src/perfCounters.ts`, drift-guarded by
  `perfCounters.sync.test.ts`) sharing one `globalThis` gauge sink —
  one table per JS realm, immune to the repo's partial
  `vi.mock('@quarto/preview-runtime')` mocks. New gauges:
  `render.started` / `render.completed` / `render.wasm-call` (the five
  leaf render entries), `render.discarded-stale` (ReactPreview
  lastContentRef guard), `wasm-envelope.parse` (segment + `bytes` extra,
  all wasmRenderer envelopes + the semantic-tokens envelope),
  `vfs.write` (segment + `bytes`), `intelligence.semantic-tokens` +
  `.tokenize`, `parse.qmd-to-ast`, `router.format-check` +
  `.parse` + `.classify`.
- [x] Baseline table (below).
- [x] Characterization tests pinning current behavior for the paths
  Phases 1-3 will touch:
  `ts-packages/preview-runtime/src/renderQueue.test.ts` (serial order,
  no-coalescing pin that Phase 1 will flip, rejection→caller +
  no-jam); `previewModeClassification.wasm.test.ts` gained three
  editor-edge pins with DISCOVERED outcomes (an unknown scalar format
  key is class `neither` — not `dom`; an unterminated front-matter
  block resolves *successfully* as the default `html`; a broken body
  still classifies from the front matter); `Q2SandboxedPreviewIframe.test.tsx`
  pins the exact 13-key UPDATE_AST payload set.

### Phase 0 baseline (2026-10-29, Node driver, release WASM, warm SASS cache)

Per-keystroke main-thread cost by component (phase A: 10 sequential
simulated keystrokes; numbers are gauge avg_ms):

| Component (gauge) | 1x (~99 KB) | 8x (~800 KB) | scale |
|---|---|---|---|
| render pipeline, total (`render.wasm-call`) | 302.1 ms | 2735.0 ms | 9.1x |
| format-check parse, AST discarded (`parse.qmd-to-ast`) | 79.4 ms | 707.8 ms | 8.9x |
| semantic tokens, synchronous (`intelligence.semantic-tokens.tokenize`) | 44.1 ms | 385.9 ms | 8.8x |
| envelope `JSON.parse`, total (`wasm-envelope.parse`) | 14.4 ms / 6 parses / 16.3 MB | 132.3 ms / 6 parses / 134.5 MB | 9.2x / 8.3x bytes |
| VFS writes (`vfs.write`) | 0.19 ms ×2 writes / 198 KB | 1.18 ms ×2 writes / 1.6 MB | 8.1x bytes |
| format resolve (`driver.resolve-formats`) | 0.48 ms | 1.5 ms | — |
| **per-keystroke wall** | **431 ms** | **3410 ms** | 7.9x |

Keystroke-burst render counts (phase B, fire-and-forget, finding 2):

| Scale | Burst | renders started | renders completed | wall |
|---|---|---|---|---|
| 1x | 10 | 10 | 10 | 3852 ms |
| 8x | 5 | 5 | 5 | 17 544 ms |

Every queued render executes — no coalescing. A 10-keystroke burst at
1x costs ~3.9 s of main-thread work, of which ~9 renders (~2.7 s) are
pure waste; a coalesced burst would cost ~0.4 s.

Findings read-out:

- **Finding 1 confirmed**: the discarded format-check parse is 18 % of
  per-keystroke cost at 1x (79 ms of 431 ms), scaling linearly.
- **Finding 2 confirmed**: burst renders == keystrokes, no coalescing;
  render CPU dominates every other component combined.
- **Finding 3/4 confirmed**: 16.3 MB of envelope JSON is parsed on the
  main thread per keystroke at 1x (134.5 MB at 8x) — the render
  envelope (transformed + untransformed AST) dwarfs everything else;
  `parseEnvelope` is ~3.3 % self-time in the 8x CPU profile even
  against the whole WASM pipeline.
- **Amplifier confirmed**: exactly 2 full-text VFS writes per keystroke
  (automerge sync + token alignment), 198 KB / 1.6 MB — plus the third
  crossing via `resolvePandocFormats` `contentOverride`.
- **Finding 8 (cold SASS)**: with the cache bridge disabled (no
  IndexedDB), every render recompiles the theme: render avg 620 ms vs
  302 ms warm at 1x, plus ~800 small importer envelope reads per render
  (8040 vs 60 per 10 keystrokes). Warm cache is the right per-keystroke
  baseline; a theme *change* pays ~320 ms once.

Caveats: Node proxy of the browser path (release WASM, real production
modules; browser cross-check deferred to phase regressions). Fixture
body is verbatim-duplicated sections. 8x BURST=10 exceeded a 10-minute
driver run (memory pressure from 10 concurrent 800-KB renders) — burst
at 8x recorded with BURST=5. The gauge wiring in the app bundle was
verified through unit/integration tests and this driver, **not** a live
browser session.

**Disabled-overhead verification (2026-10-29):** "free when off"
checked two ways. Microbench (bare vs gauge-wrapped, profiling off,
min of 5 batches): +0.005 µs/call on 21-B envelopes, +2.1 µs on 130-KB,
+9.4 µs on 16-MB, +2.0 µs on a 100-KB `vfs_add_file` — projected
≤ ~20 µs per keystroke at 1x, i.e. ~0.005 % of the 431-ms per-keystroke
wall. End-to-end A/B (same driver, pre-Phase-0 worktree vs instrumented
checkout, profiling off, interleaved runs): old walls 5580.8–5675.5 ms,
new 5575.8–5605.2 ms — identical within run-to-run spread.

Follow-up (same day, PR review): the wrappers were restructured to an
early-return fast path — disabled code is one `perfEnabled()` property
read, then the exact pre-instrumentation shape (no `try/finally`, no
gauge calls); the sink object is cached at module scope so
`perfEnabled()` itself is a single property read. Re-measured: +0.005 µs
on 21-B, +0.3 µs on 130-KB, and 16-MB/100-KB deltas lost in noise
(wrapped measures *faster* than bare) — payload-scaled overhead
eliminated, residual ≈ 5 ns per instrumented call. Both paths are pinned
by `perfGauges.test.ts` (enabled records, disabled is identical-result
and records nothing).

## Phase 1 — Latest-wins render coalescing (finding 2)

Smallest change, largest perceived-latency win: stop executing renders
whose result is already known-stale.

**Status: done 2026-10-29 (bd-r8iqmg2f).** Results below; per-keystroke
baseline is unchanged (Phase 1 removes wasted renders from a burst, it
does not make a single render cheaper).

- [x] Test first: a burst of N content updates while a render is in
  flight results in at most one additional render executing (the
  latest); the final preview reflects the last content; a rejection in a
  superseded render neither jams the queue nor surfaces. Added at the
  queue level in `ts-packages/preview-runtime/src/renderQueue.test.ts`
  (5 tests; flips the Phase 0 no-coalescing pin, plus cross-entry
  serialisation, first-render-after-idle, and both rejection shapes).
- [x] Implement latest-wins coalescing: `enqueueRender`
  (`wasmRenderer.ts`) is now a keyed pump — at most one waiting slot per
  entry point; a same-key enqueue supersedes the waiting slot's render
  (the slot keeps its FIFO position, so one entry point cannot starve
  another). The in-flight render is never interrupted and the first
  render after an idle queue still starts synchronously (no debounce
  regression). Found during implementation:
  **`renderPageInProjectWithAttribution` — the ReactPreview entry — was
  not queued at all**, so burst renders *overlapped* on the
  non-reentrant VFS state rather than queueing; it now routes through
  the queue (and `renderToHtmlInner` calls the raw, unqueued body to
  avoid self-deadlock). Superseded callers settle by rejecting with
  `RenderSupersededError`; `ReactPreview` / `Preview` swallow it
  name-matched (not `instanceof`) so partial
  `vi.mock('@quarto/preview-runtime')` doubles cannot break the check.
  New gauge: `render.superseded`.
- [x] `lastContentRef` stale-result guard (`ReactPreview.tsx`) stays as
  the second line of defense, with a comment noting it now fires only
  for the render that was in flight when the edit landed (superseded
  queued renders never execute; they arrive as the swallowed rejection
  instead).
- [x] Re-run Phase 0 burst measurement; before/after below.

### Phase 1 result (2026-10-29, Node driver, release WASM, warm SASS cache)

Phase B protocol, reconstructed driver (the Phase 0 driver did not
survive /tmp; same design: fire-and-forget burst, each "keystroke" a
full-document VFS write plus a `renderPageInProjectWithAttribution`
call, warm SASS cache via fake-indexeddb). "Before" is the same driver
run at HEAD (changes stashed); the Phase 0 recorded baseline (10/10 at
1x, 5/5 at 8x) reproduces.

| Scale | Burst | renders started/completed, before | after | superseded | wall before | wall after |
|---|---|---|---|---|---|---|
| 1x (~99 KB) | 10 | 10 / 10 | 2 / 2 | 8 | 3026 ms | 554 ms |
| 8x (~800 KB) | 5 | 5 / 5 | 2 / 2 | 3 | 12 367 ms | 4476 ms |

Envelope `JSON.parse` bytes per burst fell with the render count
(106.9 → 21.4 MB at 1x; 439.9 → 176.0 MB at 8x). The 2 renders that
remain are the in-flight one and the latest; freed from contending
with the rest of the burst, their per-render wall returns to solo cost
(~277 ms at 1x). The plan's Phase 0 estimate ("a coalesced burst would
cost ~0.4 s" at 1x) matches the measured 554 ms on this fixture.

Verification: preview-runtime unit 86/86, hub-client `test:ci`
437 passed / 28 skipped, `npm run build:all` green, no snapshot changes.
End-to-end: measured through the real WASM and production
preview-runtime sources under the Node driver (the Phase 0 proxy);
**no live-browser verification was done** — the component-level change
is two try/catch blocks and comment updates, but a dev-server typing
smoke is still owed before Phase 2 builds on this.

## Phase 2 — Parse-check-only format classification (finding 1)

The router pays a full parse + AST serialization + main-thread
`JSON.parse` per keystroke and keeps only `success`.

- [ ] Test first (Rust, `crates/wasm-quarto-hub-client`): a new export
  (working name `check_document_for_preview`) returns parseability plus
  the resolved format classification inputs without serializing an AST;
  assert classification parity with the `parse_qmd_to_ast` +
  `resolve_pandoc_formats` pair across format fixtures (default html,
  revealjs, pdf, docx, unknown key, mid-edit syntax error).
- [ ] Implement the Rust export (TDD: failing test, then implement).
  Reuse the metadata/front-matter path; no AST writer run.
- [ ] TS wrapper in `wasmRenderer.ts` + switch
  `PreviewRouter.tsx:134-168` to it. While here: skip the check entirely
  when the edit did not touch the front-matter block (cheap string
  compare of the YAML region), which also removes the
  `resolvePandocFormats` contentOverride crossing for body-only edits.
- [ ] Regression: keystroke-type through the format fixtures in the dev
  server, confirm no mode flicker; record the per-keystroke cost delta
  from the Phase 0 harness.

## Phase 3 — Single-parse AST transport (finding 3)

Today the AST is a JSON string inside the JSON response envelope: the
main thread parses the whole envelope to extract a string it reposts to
the iframe, which parses it again. Since bd-73lilznv (2026-10-26) the
sandbox iframe is cross-origin by default, so the iframe-side re-parse
runs in a separate process under site isolation; the main-thread costs
that remain are the envelope parse itself and the parent-side AST
consumers enumerated below.

- [ ] Test first: characterization test that the exact `astJson` string
  posted to the iframe is byte-identical before/after (golden fixtures
  in `goldenParity.wasm.test.ts` style), and that
  `untransformed_ast_json` is absent from the envelope unless requested.
- [ ] Rust side: change the render response so `ast_json` (and
  `untransformed_ast_json` when requested) travel as separate string
  out-params or a wasm-bindgen multi-return, not nested inside the
  parsed envelope. Gate `untransformed_ast_json` behind a flag the TS
  side sets only during rich-text editing sessions.
- [ ] TS side: `renderPageInProjectWithAttribution` and siblings stop
  parsing the AST; `ReactPreview` threads the opaque string through.
  Parent-side consumers that parse the AST as an object today (found
  2026-10-27; grep `.ast_json` / `astJson` / `JSON.parse(astJson)`
  outside iframe plumbing to catch any new ones), with scoping verdicts:
  - `buildProxyAssetManifest` (`proxyAssetManifest.ts:29`, run by
    `Q2SandboxedPreviewIframe` on every render) — **move iframe-side**
    (decided 2026-10-27; sub-items below).
  - `ReactRenderer.tsx:230` (`extractRenderComponentPaths`) — **stays**.
    Mounted by `ReactPreview.tsx:896`, the direct (non-sandboxed) React
    preview, which renders on the main thread by design; the opaque
    string applies to the sandboxed path only.
  - `useCursorToSlide.ts:29`, `useSlideThumbnails.tsx:94` — **stay**.
    Parent-side slide UI (Editor.tsx, OutlinePanel.tsx), independent of
    preview path. Optional follow-up, out of Phase 3 scope: have these
    share one parsed object instead of each re-parsing.
- [ ] Move `proxyAssetManifest.ts` (+ its test) into
  `hub-client/quarto-hub-sandboxed-preview/src/` and build the manifest
  in `updateAst`, dropping `assetManifest` from the UPDATE_AST payload
  and the parent `useMemo`. Rationale: the function is pure in
  `(astJson, currentFilePath)` — both already cross in UPDATE_AST — does
  no VFS reads, and its output is consumed only iframe-side via
  `AssetManifestContext` (`PreviewRoot.tsx:1603`). The move also fixes a
  reverse-import smell (parent code currently reaches into the iframe
  package for `pageRelativeUrlForVfsPath` via `../../../../`). The
  "Image never renders before its manifest entry" guarantee is
  preserved: the manifest is derived from the same UPDATE_AST string,
  synchronously before `root.render`. Do NOT unify with q2-preview's
  `buildAssetManifest` (`Q2PreviewIframe.tsx:314`): that one reads VFS
  bytes and mints origin-scoped blob URLs, so it must stay parent-side —
  cross-comment both files.
- [ ] Optional, only if iframe-side profiling cares: derive the manifest
  from the iframe's already-parsed AST instead of a second
  `JSON.parse(astJson)` in `updateAst`. Phase 3's target is the hub main
  thread; a same-string parse in the iframe process is off that thread
  and acceptable.
- [ ] Snapshot audit: this touches render-response shape — expect
  `.snap` updates; report counts and surprises per AGENTS.md.
- [ ] Measure: envelope `JSON.parse` bytes and AST parse count per
  render before/after, recorded in the plan.

## Phase 4 — Binary asset transfer without base64 (finding 6)

Every preview image/font currently does Rust base64 encode, JSON
envelope, main-thread parse, string postMessage, SW base64 decode.

- [ ] Test first: binary asset round-trip unit test (isBinaryPath +
  url_response content) and the existing chromium e2e smoke for proxied
  assets must pass unchanged on byte equality of served content.
- [ ] Add a binary VFS read that returns `Uint8Array` directly (no
  base64, no JSON envelope) on the Rust export; keep the text path
  as-is. (This touches `crates/wasm-quarto-hub-client` — the Conventions
  WASM rebuild chain applies to this phase.)
- [ ] Switch `Q2SandboxedPreviewIframe.tsx:182-221` and the SW
  `requestVFS` response to carry an ArrayBuffer, transferred (not
  cloned) via the postMessage transfer list. Cross-origin transfer is
  exercised by default since bd-73lilznv moved the sandbox origin to
  `quarto-dev.github.io/q2-sandbox` (2026-10-26) — the smoke test now
  covers the production topology, not a same-origin special case.
  (ArrayBuffer transfer across origins is spec-supported; verify in the
  smoke test regardless. Do not reach for SharedArrayBuffer — it
  requires COOP/COEP headers the sandbox host does not send.)
- [ ] File the strand as new work (bd-k9tfhsst is closed and did not
  cover wire format). Measure asset payload bytes and main-thread copies
  before/after on an image-heavy fixture.

## Phase 5 — Semantic tokens and the envelope tax (findings 4, 5)

Measurement-gated: start only with Phase 0 numbers showing these matter
relative to Phases 1-3.

- [ ] Baseline first: token-request frequency from Monaco during a
  keystroke burst, and synchronous tokenize ms per request at fixture
  scales. Check how Monaco's built-in ~300 ms semantic-tokens debounce
  interacts — and note that `refreshSemanticTokens()`
  (`useIntelligenceProviders.ts`, `services/monacoProviders.ts`)
  deliberately bypasses it via `schedule(0)` on mount and on WASM
  ready, so forced refreshes pay the synchronous cost immediately.
- [ ] Candidate fixes, cheapest first: (a) coalesce/debounce token
  requests in `monacoProviders` so a burst costs one synchronous run;
  (b) avoid the extra `vfsAddFile` full-text write when the VFS image is
  already current (dirty-check against the last write); (c) scoped
  serde-wasm-bindgen or typed returns for the three hottest envelopes
  (vfs write, semantic tokens, render response) instead of a blanket
  JSON.parse migration.
- [ ] Only if (a)-(c) are insufficient: evaluate moving LSP
  intelligence into a dedicated worker fed by the same sync callbacks
  (design note first; overlaps Phase 7).
- [ ] Record what was measured, which option was taken, and why.

## Phase 6 — Attribution off the main thread (finding 7)

Behind a toggle and debounced, so lowest urgency; still O(document
history) when on.

- [ ] Baseline: cold-build and incremental-update ms at history scales
  (the 2026-04-23 attribution profiling session is the template;
  `requestIdleCallback` override mandatory).
- [ ] If numbers warrant: move run-list replay and char-to-byte mapping
  into a worker (Automerge handles are transferable to a worker context
  via doc export/import; design the wire shape first), keeping the
  hook's payload contract unchanged.
- [ ] Regression: attribution overlay output identical on fixtures with
  mixed author/actor history; toggle on/off flicker-free.

## Phase 7 — Render-worker architecture evaluation (design only)

Not an implementation phase: produce a design note weighing the move of
`wasm-quarto-hub-client` (pipeline + VFS) into a dedicated worker, which
would take the whole render pipeline off the main thread.

- [ ] Design note in `claude-notes/designs/`: sync client pushes file
  changes into the worker VFS; render requests/responses as RPC;
  constraints enumerated (the SASS bridge calls JS synchronously from
  Rust; Monaco semantic tokens want synchronous answers; the serial
  queue moves with the VFS; memory duplication of the VFS during any
  transition).
- [ ] Decision record: adopt, defer, or reject — with the Phase 0/5
  numbers as the evidence base. If adopted, spawn an epic and re-plan.
