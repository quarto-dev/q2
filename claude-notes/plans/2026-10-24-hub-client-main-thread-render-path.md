---
title: 'hub-client main-thread render path: audit fixes'
date: 2026-10-24
description: 'Phased fixes from the main-thread render-path audit: render coalescing, resolver-only format classification, untransformed-AST capture elimination and gating, binary asset transfer, then measurement-gated worker work.'
status: in-progress
---

## Overview

The hub-client render path runs the entire Rust WASM pipeline on the browser
main thread: VFS access, qmd parsing, transforms, AST production, theme CSS
compilation, and LSP intelligence. Only pandoc.wasm, typst, and Monaco
language workers run off that thread (architecture map:
`claude-notes/designs/pandoc-wasm-architecture.md`). General work on the main
thread therefore appears as typing latency and preview lag.

This plan addresses eight findings in cheapest-first order, with measurements
gating the larger changes per
`claude-notes/instructions/performance-profiling.md`. The original findings
came from static analysis and code-path reading; the completed phases below
record which costs the measurements confirmed. File:line references locate
the audit sites, but should be re-located before editing.

The sandboxed preview is genuinely cross-origin at
`quarto-dev.github.io/q2-sandbox`, which affects findings 3 and 6 and Phases
3-4. TS-side performance instrumentation lives in
`hub-client/src/utils/perfCounters.ts`.

Phases 0-2 and 3A are complete. **Phase 3B1 is the current implementation
checkpoint and completes bd-wpmqf039. Phases 3B2 and 3C remain deferred.**
Phase 3B1 preserves eager capture whenever comments are visible or editing is
enabled; it does not redesign comment interactions. This keeps the capture
optimization independent of ongoing comment work. The comment-intent UX in
Phase 3B2 and the transport changes in Phase 3C require fresh measurements
before activation.

## Findings from the original audit

| # | Finding | Primary site | Cost class |
|---|---------|--------------|------------|
| 1 | Per-keystroke full parse whose AST is discarded (format classification) | `hub-client/src/components/render/PreviewRouter.tsx:134` | wasted work per keystroke |
| 2 | Renders are debounce-free; stale queued renders still execute | `hub-client/src/components/render/ReactPreview.tsx:768`, `ts-packages/preview-runtime/src/wasmRenderer.ts:494` | N full pipelines per N-keystroke burst |
| 3 | Untransformed AST captured on every preview render and captured twice on the project path; transformed AST nested as a JSON string in the response envelope | `crates/quarto-core/src/pipeline.rs:2083`, `crates/wasm-quarto-hub-client/src/lib.rs:2947`, `ts-packages/preview-runtime/src/wasmRenderer.ts:835` | redundant QMD parse + serialization; multi-MB JSON scanning |
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
render itself reads it. Phase 0 measured each crossing as sub-millisecond;
Phase 2's final design explains why the third crossing is not worth removing.
The amplifier is folded into Phase 5 rather than tracked separately.

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
  `hub-client/`. Phases touching `crates/wasm-quarto-hub-client` or the
  quarto-core code it compiles in (2 via `formats.rs`, 3, 4)
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
- File a braid strand per independently implemented phase or subphase when
  work begins; reference this plan.
  Any snapshot-file changes get the AGENTS.md snapshot treatment: counts,
  summary, and surprises called out.

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

The final wrappers use an early-return fast path: disabled code performs one
`perfEnabled()` property read, then follows the exact pre-instrumentation
shape with no `try/finally` or gauge calls. The sink object is cached at
module scope, so `perfEnabled()` itself is a single property read.
Re-measurement found +0.005 µs on 21-B and +0.3 µs on 130-KB envelopes, with
16-MB/100-KB deltas lost in noise. Payload-scaled overhead is eliminated,
leaving approximately 5 ns per instrumented call. `perfGauges.test.ts` pins
both enabled recording and disabled identical-result behavior.

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
End-to-end verification used the real WASM and production preview-runtime
sources under the Node driver, followed by the browser smoke below.

Additional queue invariants:

- The dead `isRenderSupersededError` export was removed — the
  components deliberately inline the name check so partial
  `vi.mock('@quarto/preview-runtime')` doubles cannot break it, which
  left the helper with no possible consumers.
- The one-slot-per-key invariant is now structural: the waiting-slots
  store is a `Map<key, slot>` (was array + `find`); insertion-ordered
  iteration is the FIFO the pump drains in, and supersede mutates the
  slot in place so it keeps its position. Behavior-preserved; the
  characterization tests pinned it.
- `renderPrintable` — the last unqueued render entry with a live
  caller (`openPrintableDocument`) — now serializes through the queue
  under a fresh coalesce key per call (bd-gj6fk5hc): prints never
  coalesce, since each is an explicit user action. A print fired
  mid-render previously overlapped the non-reentrant VFS state.
  `renderQmd` stays unqueued (exported but caller-less).
  Re-verification: preview-runtime 88/88, hub-client `test:ci`
  437 passed / 28 skipped.

**Browser smoke:** A throwaway Playwright spec (deleted after the run, same
precedent as the Phase 0 driver) drove the **dev server** end-to-end:
vite dev with `VITE_E2E=1 VITE_DEFAULT_SYNC_SERVER=/ws
VITE_HUB_SERVER=http://localhost:3031
VITE_Q2_SANDBOXED_PREVIEW_URL=q2-sandboxed-preview/index.html`, the
functional suite's globalSetup hub on 3031, headless chromium; recipe
`cd hub-client && npx playwright test --config
playwright.smoke.config.ts`. Scenario: seed a project with a ~300 KB
qmd via the e2e projectFactory, wait for the initial render in the
q2-preview iframe (`iframe[title="q2-preview Renderer"]` — NOT the
sandboxed-preview title), type a 10-keystroke burst (`BURSTMARK!`)
paced at 250 ms, click the print button immediately after the burst.
Two consecutive runs passed, observed gauges:
`render.started=5 completed=5 superseded=6→7 discarded-stale=1
effect.react-preview-rerender=10–11` — a 10-keystroke burst ran ~3
burst renders instead of ~10, the in-flight render's stale result was
discarded once, the printable popup contained the burst text (proving
the print serialized FIFO behind the burst renders), the preview
converged to the last content, and no `RenderSupersededError` reached
console/pageerror. Recipe gotchas, for the next driver: ~99 KB renders
too fast (~150 ms warm) for CDP keystroke cadence — nothing queues, so
nothing coalesces; `keyboard.type(..., { delay: 0 })` lets React batch
keystrokes into as few as 3 effect fires, same symptom; the 250 ms
pace makes overlap deterministic.

## Phase 2 — Resolver-only format classification (finding 1)

Before this phase, the router paid a full parse per keystroke and read only
`success` (`PreviewRouter.tsx:147`). That parse was `parse_qmd_to_ast`: the
whole tree-sitter body parse, the engine pass-through, `MetadataMergeStage`,
and the AST writer — a larger discard than "parse + serialize" suggests.

**Status: done 2026-11-09 (bd-luoo7qgu).** The router uses the resolver
directly, without parsing the document or retaining the old parse guard. A
one-line consistency fix in the Rust resolver makes unknown `q2-*` keys
classify as `neither`, which allows both deletions. The other per-keystroke
components (render, tokens, VFS) are unchanged; this phase removes only the
discarded parse and its envelope.

### Classification evidence

A probe ran `parse_qmd_to_ast` and `resolve_pandoc_formats` on each input,
then applied `classifyPreviewMode` as the router did. It omitted
`format: dashboard`, which did not affect the classification constraints
below.

| Input | parse `success` | resolver `class` / `pseudoFormat` | former router mode |
|---|---|---|---|
| no front matter; `format: html`, `revealjs`, `q2-slides` | true | preview / set | react |
| `format: q2-html-render` | true | preview / null | dom |
| `format: typst`, `docx`; a `docx, html` map | true | download / null | download or pdf |
| `format: latex`, `r`, `reveal`, `html5` | **false** | neither / null | neither |
| `format: pdf` | true | neither / null | neither |
| `format: acm-html`, extension not installed | true | preview / null | dom |
| `format: q2-s`, `q2-bogus` | **false** | **neither / the key itself** | react — **held back by the guard** |
| malformed YAML; unterminated front matter | true | html → q2-preview | react |
| body: unclosed div, unclosed fence, bad cell options, unknown shortcode | true | from the front matter | unchanged |
| body: `[text{.cls}`, `[x](` | **false** (parse error / Q-2-31) | from the front matter | held |
| runtime `format: revealjs` + document `format: bogus` | **false** | revealjs / revealjs | **stuck on the previous mode** |

Timing, 75 KB document, warm, min of 5: `parse_qmd_to_ast` 67.1 ms,
`resolve_pandoc_formats` 0.26 ms. The Phase 0 baseline agrees: 79 ms vs
0.48 ms at 1x, 708 ms vs 1.5 ms at 8x.

The table establishes three design constraints:

1. **`success` is not front-matter-determined.** Unbalanced inline
   spans and malformed links hard-fail the parse (pampa raises a parse
   error / Q-2-31). A parse-free export cannot reproduce `success`.
   This does not matter for classification: the mode is a function of
   the front matter and the project only, so "hold the previous mode on
   a body error" and "reclassify from the unchanged front matter" are
   the same mode. Parity is asserted on the **mode**, never on
   `success`.
2. **The guard is live in exactly one real case**: a `q2-*` key the
   pipeline cannot build — the partial `q2-s` while typing `q2-slides`,
   or a typo. The resolver is inconsistent there: `format_class` says
   `neither` (it gates on `Format::from_format_string`) while
   `preview_pseudo_format` returns the key for anything starting with
   `q2-`, and `classifyPreviewMode` checks `pseudoFormat` before
   `class`, so the mode would be `react` with an unbuildable format.
   The parse gate was masking that. Once `preview_pseudo_format`
   applies the same `from_format_string` gate, an unknown key *always*
   classifies `neither`, which the guard already lets through — the
   guard is provably vacuous and goes with the parse. (Every other mode
   implies a key `from_format_string` accepts: `pseudoFormat` set ⇒
   html / dashboard / revealjs / a buildable `q2-*`; `q2-html-render`
   is builtin; class `preview` is gated on it; class `download` ⇒ one
   of the five download-table keys, all known identifiers.)
3. **The guard causes a bug.** `front_matter_format` reads the document
   layer only, while the resolver — and the real render, via
   `resolve_entry_format` — are project-aware and honor the runtime
   scalar `format` (`q2 preview --to`). With a requested format, a
   document whose own key was unknown could never reclassify (last row).
   Deleting the guard fixes it; the resolver is the one that is
   right.

### Final design

- **No new export.** `resolve_pandoc_formats` already returns everything
  the router needs, synchronously, in a small envelope.
- **No skip logic.** Skipping the resolver would save only 0.26 ms in the
  probe and 0.48 / 1.5 ms at 1x / 8x in the baseline. The "third full-text
  crossing" is one buffer copy into WASM memory, sub-millisecond at 8x. A
  TS front-matter extractor would have to match the Rust YAML-block
  boundary exactly, and a VFS-write counter would touch three wrappers.
  Neither justifies the invalidation risk; the amplifier stays with Phase 5.
- **Accepted behavior change.** While a `q2-*` format name is being typed,
  the pane shows the `neither` view for the partial key instead of holding
  the previous preview, then mounts the deck when the key completes. This
  matches typing `revealjs` or `docx` letter by letter. The preview already
  shows a render error for the partial key because rendering resolves the
  format itself.
- **Out of scope, pre-existing**: `format: q2-foo-html` parses as extension
  `q2-foo` of `html`, gets a `pseudoFormat`, and mounts the React preview on
  a non-preview pipeline. This phase leaves that behavior unchanged.

### Checklist

- [x] File the strand (`braid create "Phase 2: resolver-only format
  classification" -t task -p 2 -d "<this plan>"`), mark it
  `in_progress`, branch `braid/<id>-resolver-only-format-check` off
  `main`. → bd-luoo7qgu, branch `braid/bd-luoo7qgu-resolver-only-format-check`.
- [x] **Test first (TS, wasm suite).** In
  `previewModeClassification.wasm.test.ts`, under the editor-edge
  `describe`, add two pins that must **fail** on the current artifact:
  `format: q2-s` and `format: q2-bogus` → `{ mode: 'neither', formatKey:
  'q2-s' }` / `'q2-bogus'` (pre-fix: `react` with the partial key). Add one
  regression pin that passes already and documents runtime-format behavior:
  `wasm.vfs_set_runtime_metadata('format: revealjs')` then a buffer with
  `format: bogus` → `{ mode: 'react', format: 'revealjs' }` (the
  `beforeEach` already resets the runtime metadata). Fix the stale
  comment on the unterminated-front-matter pin: the parse succeeds there
  too, the guard never suppressed anything, and the transient
  `q2-preview` reclassify is real (benign only when the prior mode was
  already `q2-preview`). → Red confirmed: `q2-s` classified `react`;
  regression pin green from the start.
- [x] **Test first (Rust).** Extend
  `the_preview_pseudo_format_follows_the_react_rule` in
  `crates/quarto-core/src/pandoc_request/formats.rs`: `q2-s`, `q2-bogus`
  → `None`; `q2-preview`, `q2-debug`, `q2-sandboxed-preview`,
  `q2-slides`, `q2-dashboard` → `Some(key)`; `q2-html-render` → `None`
  (unchanged). `cargo nextest run -p quarto-core formats` — the two new
  `None` assertions fail. → Red confirmed (`q2-s` → `Some("q2-s")`).
- [x] **Implement the Rust fix**: in `preview_pseudo_format`, the `q2-*`
  arm returns `Some(k)` only when `Format::from_format_string(k).is_ok()`
  — the identical gate `format_class` uses a few lines up (`Format` is
  already imported). One line plus a two-line comment: a `q2-*` key
  previews as itself only when the pipeline can build it; a partial or
  misspelt key is `neither`, like any unknown key. Then
  `cargo clippy -p quarto-core --all-targets -- -D warnings`.
  → 53/53 formats tests green, clippy clean.
- [x] **Rebuild the WASM** so the TS pins see the fix: `cd hub-client &&
  npm run build:wasm`, then `npm run test:wasm`. The `q2-s` / `q2-bogus`
  pins flip to green; the rest of the classification suite (typst and
  its `output-ext` variants, extension formats, multi-format maps,
  project-level and book formats, the editor-edge pins) is unchanged —
  that *is* the mode-parity assertion across the existing fixture set.
  → 17/17 classification pins green; full wasm suite 445 passed / 28
  skipped.
- [x] **Router** (`PreviewRouter.tsx`, the `checkFormat` effect):
  - delete the `parseQmdToAst` call, the `result.success || …` guard
    and the `async` wrapper. The effect becomes synchronous: early
    return when `!isWasmReady()`, build `deps`, `mode =
    classifyPreviewMode(path, deps)`, then the existing
    `setPreviewMode(prev => same ? prev : mode)` / `onFormatChange` /
    `onPreviewModeChange` tail unconditionally. Keep `try/catch/finally`
    with `setCheckedPath` in `finally` — the `initialChecking`
    placeholder depends on it;
  - the inner `isWasmReady() ? … : null` in `deps.resolve` is redundant
    after the early return — drop it; `lastStableFormatRef` is written
    and never read — delete it;
  - gauges: keep `router.format-check` and `.classify`, delete `.parse`;
  - delete the stale comments ("a mid-edit syntax error keeps the
    previous mode"; "a format the parser's metadata merge does not know
    (`latex`) fails the parse") and the "Finding 1" marker — the code
    they describe is gone. Replace with one line: the mode is the
    resolver's answer, re-read on every content change
    (sub-millisecond; see this phase);
  - `PreviewRouter.integration.test.tsx`: drop the `parseQmdToAst` entry
    from the `vi.mock('@quarto/preview-runtime')` factory and rewrite the
    header's "the WASM probe is mocked to return an AST…" sentence; the
    three routing cases stay as they are. → 3/3 green.
- [x] **preview-runtime cleanup** (`wasmRenderer.ts`): `parseQmdToAst`
  loses its last caller (`ReactPreview` uses
  `parseQmdToAstWithAttribution` for the q2-debug / q2-slides render;
  the SPA and the sandboxed preview never imported it). Delete the
  wrapper, per the Phase 1 precedent for dead exports, and point the two
  comments that name it (`useAttribution.ts:20`, `pipelineKind.ts:12`)
  at the attribution variant. Remove the "Finding 1" comment inside
  `parseQmdToAstWithAttribution`; the `parse.qmd-to-ast` gauge stays —
  it now measures only real q2-debug / q2-slides renders. Leave the Rust
  `parse_qmd_to_ast` export alone.
- [x] **Verify**: `npm run test:ci` and `npm run build:all` from
  `hub-client/`; `cargo nextest run -p quarto-core`; `cargo xtask verify`
  (full — quarto-core feeds the WASM). No snapshot changes expected; if
  any `.snap` moves, stop and report per AGENTS.md.
  → Full `cargo xtask verify` green (lint, workspace build + strict
  claude-notes render + noop-rebuild check, ts-packages builds, hub-client
  build:all incl. WASM + SPA, hub-client test:ci = 1746 unit + integration
  + 445/28 wasm). Workspace nextest 19194/19194 with two exclusions, both
  R-package version skew on this machine, both reproduced with the branch
  diff reverted and filed as bd-qwscag8b:
  `capture_classifies_pieces` (newer knitr classes the chunk error
  `rlang_error`, test pins `simpleError`) and
  `a_sql_chunk_with_hash_pipe_options_records_as_it_runs_live` (newer
  knitr refuses `#|` options on SQL chunks, so the live run errors). Zero
  `.snap` files touched. Side effect: `cargo xtask verify` exposed a
  stale `node_modules` (`@types/jsdom` missing) — `npm install` at the
  repo root before the rerun.
- [x] **Measure** with the Node driver (recipe: perf playbook § "Driving
  the real render path"; same fixture scales as Phase 0). Per keystroke,
  `router.format-check` drops from parse-dominated to ≤ ~1.5 ms and
  `parse.qmd-to-ast` no longer fires from the router. Expected
  per-keystroke wall: 431 → ~350 ms at 1x, 3410 → ~2700 ms at 8x.
  Record before/after in a "Phase 2 result" table under this checklist.
- [x] **End-to-end in the dev server** (`npm run dev`, a project with a
  `_quarto.yml`): (a) type in the body of a `revealjs` document — the
  deck stays mounted, no flicker; (b) in the front matter type
  `format: q2-slides` letter by letter — the pane shows the `neither`
  view for the partial keys and mounts the deck at `q2-slides`; (c)
  change `format:` in `_quarto.yml` for a front-matter-less chapter — the
  open chapter reclassifies on its next keystroke (the
  VFS-generation-keyed `cached_project`, `lib.rs:2360`, still does this
  with no skip in the way); (d) with `QUARTO_PERF_STATS` on, read
  `router.format-check` from the gauge table. Record the invocation and
  what was observed here.

  Done as a throwaway Playwright spec (deleted after the run; precedent:
  the Phase 0 driver, the Phase 1 smoke): vite dev
  (`VITE_E2E=1 VITE_DEFAULT_SYNC_SERVER=/ws
  VITE_HUB_SERVER=http://localhost:3031
  VITE_Q2_SANDBOXED_PREVIEW_URL=q2-sandboxed-preview/index.html`) against
  the functional suite's globalSetup hub, headless chromium; invocation
  `cd hub-client && npx playwright test --config
  playwright.phase2-smoke.config.ts` — **2 passed**. Observed:
  (a) a 9-character body burst in a `format: revealjs` document kept the
  deck mounted at every keystroke and the deck re-rendered with the new
  text; (b) `format: q2-`/`q2-s`/`q2-sl` each showed the `neither` pane
  *naming the partial key* (the accepted behavior change), and
  `q2-slides` mounted the slide preview — **which is `SlideAst`, the
  in-page generic slide surface, not the q2-preview iframe**: `revealjs`
  converged onto the iframe (bd-vwp4y5ku), `q2-slides` did not
  (`ReactRenderer.tsx`\'s fall-through arm); (c) after adding
  `format: revealjs` to `_quarto.yml` and polling the VFS until the
  sibling edit landed (the router classifies only on the chapter's next
  content change, so a fixed sleep races the sync), one keystroke in the
  open front-matter-less chapter flipped it to the reveal deck;
  (d) browser gauges: `router.format-check` 17 runs avg **1.0 ms**
  (resolver + classify; the deleted parse was 80–700+ ms), no
  `router.format-check.parse`, and `parse.qmd-to-ast` entirely absent
  before the `q2-slides` completion — then present after, from the one
  remaining consumer (SlideAst's AST-only q2-slides render).
  Gotchas for the next driver: Monaco auto-indents on Enter (an
  unguarded `format:` line landed nested under `project:` — the
  resolver was right to keep saying html; two Node probes confirmed the
  resolver reclassifies correctly at the WASM level), and position the
  cursor by clicking the target `.view-line` + `End`, not Meta+Arrow
  chords (no-ops in headless chromium here).

### Phase 2 result (2026-11-09, Node driver, release WASM, warm SASS cache)

Reconstructed driver (the Phase 0/1 drivers did not survive /tmp; same
design plus one flag): sequential 10-keystroke phase-A protocol — VFS
write, format check, model-aligned semantic tokens, full render — against
the production `wasmRenderer`/`intelligenceService`/`getQ2Format` sources
and the real WASM. `ROUTER_PARSE=1` re-inserts the deleted per-keystroke
parse (`parseQmdToAstWithAttribution(content, null)`, exactly the old
router call) for the "before" leg, so both legs run the same fixture on
the same build. Fixture rebuilt to the Phase 0 spec (mid-size project
document, ~99 KB at 1x, images, theme; 1x/2x/4x/8x body duplication) —
its composition differs from Phase 0's, so absolute render cost differs;
only same-driver before/after is compared. Invocation:
`QUARTO_PERF_STATS=1 node --import tsx/esm driver.mjs` (± `ROUTER_PARSE=1`),
run on an idle machine.

Per-keystroke wall (10 sequential keystrokes, ms):

| Scale | before (parse) | after (resolver-only) | delta | % |
|---|---|---|---|---|
| 1x (99 KB) | 526.9 | 423.7 | −103.2 | −19.6 % |
| 2x (199 KB) | 1030.5 | 831.3 | −199.2 | −19.3 % |
| 4x (396 KB) | 2003.1 | 1644.2 | −358.9 | −17.9 % |
| 8x (792 KB) | 3987.3 | 3299.6 | −687.7 | −17.2 % |

The deleted parse was ~19 % of per-keystroke cost at 1x scaling linearly
(delta 6.7× from 1x to 8x) — the Phase 0 baseline measured 18 % (79.4 ms
of 431 ms) on its own fixture. The format check itself (resolver +
classify, driver-timed) is 0.38–0.87 ms per call at every scale — under
the ≤ ~1.5 ms target — and `parse.qmd-to-ast` no longer fires from the
router path (0 runs in the after leg; it now measures only real
q2-debug / q2-slides renders). Envelope `JSON.parse` per keystroke fell
from 6 to 5 and envelope bytes −31 % (162.4 → 111.8 MB per 10 keystrokes
at 1x, 1338.2 → 919.8 MB at 8x) — the discarded AST envelope is gone.
The per-keystroke wall numbers are therefore a slight *under*statement of
the win on the Phase 0 fixture (whose baseline wall included a cheaper
parse): relative to the Phase 0 baseline's own components, deleting the
parse takes the 1x wall from 431 ms to ~350 ms and the 8x wall from
3410 ms to ~2700 ms, matching this phase's prediction.
- [x] hub-client changelog entry (two-commit workflow: the change, then
  the hash). Close the strand with the measurement in the comment.
  → 641acc274 + changelog efcad13be; bd-luoo7qgu closed with the
  measurement.

## Phase 3 — Skip and gate the untransformed AST capture (finding 3)

Phases 3A and 3B1 are two implementation checkpoints under one strand.
Phase 3A removes the duplicate project-side capture without changing
behavior or the public WASM API. Phase 3B1 makes capture optional with a
conservative eager-comment policy and preserves capture-on defaults for
existing callers.

**Complete Phase 3B1 now, then close bd-wpmqf039 after verification and
measurement. Keep Phases 3B2 and 3C deferred.** Phase 3B1 does not optimize
or redesign comment interactions, so it does not depend on comments becoming
a stable design. Its only comment-UI change is the baseline-less read-only
guard below. The host-side **Enable comments** action and request correlation
in Phase 3B2 remain measurement-gated product work; Phase 3C's transport
changes wait for evidence that envelope parsing still justifies their
protocol complexity.

The original capture inventory established these constraints:

- `render_qmd_to_preview_ast` captures the untransformed AST with a fresh,
  parent-less `qmd_to_pandoc` parse and an
  `include_inline_locations: true` JSON write before running the main
  pipeline (`crates/quarto-core/src/pipeline.rs:2077`). Single-document
  preview renders therefore parse the QMD twice: once for this capture and
  once for the transformed preview AST.
- Before Phase 3A, the multi-file project renderer called the same function
  but dropped `preview_output.untransformed_ast_json` when it constructed
  `WasmPassTwoOutput` (`crates/quarto-core/src/project/pass2_renderer.rs:1251`,
  `:1341`). The project response builder then reparsed the active document
  to recreate the same payload (`crates/wasm-quarto-hub-client/src/lib.rs:2943`).
  Armed project renders therefore parsed the QMD three times; after Phase 3A
  they parse twice.
- The two project captures are not interchangeable. Pass 2 names the source
  with canonical `doc_info.input`; the response names it with the original
  `active_path`. Hub calls can use relative paths such as `index.qmd`, while
  the canonical path is `/project/index.qmd`. The name is serialized into
  the AST context, so moving the pass-2 capture into the response would not
  preserve bytes for those calls.
- The untransformed AST is not only an `applyNodeEdit` input. The iframe
  parses it to build `sourceIndex` (`PreviewRoot.tsx:1507`), which both
  content editing and comment add/resolve use to find their source nodes.
  An unarmed render therefore cannot support comment edits: the comment
  affordance and handlers need `sourceIndex` before they create a payload.

Phase 2 measured a comparable parse and serialization at about 19 % of
per-keystroke wall on its fixture. This makes gating worth measuring, not a
guaranteed win on every path. The unarmed cases are a hub preview with
editing off and comments hidden, and the default read-only q2-preview SPA
with `allowEdit=false`. Fresh-default hub sessions remain armed. Use the
same-driver before/after and same-build flag-on/flag-off measurements below
as the evidence.

### Design constraints

- **Keep the separate parent-less parse.** It protects the source-pool and
  provenance contract used by incremental writes (`pipeline.rs:5262`). Do
  not derive a baseline from the transformed pipeline AST.
- **The project response keeps ownership of its capture.** Phase 3A skips
  the discarded pass-2 capture; it does not carry that capture into
  `WasmPassTwoOutput`. The response continues to capture with the original
  `active_path`, preserving both bytes and source-name provenance.
- **The capture flag is per render and defaults to on.** Use the internal
  capture-aware core implementation from Phase 3A and retain
  `render_qmd_to_preview_ast` as a capture-on wrapper for native callers.
  Add `captureUntransformedAst` to the existing
  `render_page_for_preview` options object and a trailing options object to
  `render_page_in_project_with_attribution`. At both WASM boundaries, an
  omitted options object or flag means capture-on, preserving the existing
  raw-WASM contract. Only the hub and SPA hot callers need to pass the flag
  explicitly; unrelated raw-WASM callers and tests need no mechanical
  edits. Do not add module-global WASM state, which would race with the
  Phase 1 queue.
- **Phases 3A and 3B1 share one strand.** Land them as two green,
  independently revertable implementation checkpoints: first the
  behavior-preserving project-capture deletion, then per-render gating.
  Hub-client changelog entries still use their required separate hash
  commits.
- **Phase 3B1 keeps capture eager for active comment and edit surfaces.**
  In hub-client, arm capture for
  `previewEditing || (commentsMode ?? 'show') !== 'hide'`. In
  the q2-preview SPA, arm it for `allowEdit`, which governs both content
  editing and commenting. The unarmed hub case is therefore editing off
  plus comments hidden. Fresh hub defaults have `previewEditing=true`, so
  they remain armed in every Phase 3 variant.
- **Baseline-less comments are read-only in Phase 3B1.** Hidden-to-visible
  and editing-transition renders temporarily have no new baseline, so this
  gate cannot wait for Phase 3B2. Derive readiness from
  `PreviewContext.sourceIndex`; do not add a readiness prop or iframe
  protocol. `CommentWrapper` must include a missing source index in its
  read-only gate, suppress resolve controls and new-comment inputs, and
  hide an input that is already open when the baseline disappears. Existing
  comments remain readable and no comment text or edit is queued.
- **Nesting buffers have a separate editing gate and memo dependency.** Hub
  computes them only for `previewEditing && (unlockNestingCursor ?? true)`
  and includes `previewEditing` in the memo dependencies. The SPA uses
  `allowEdit && nestingCursor` and includes `allowEdit`. A comment-armed
  read-mode render must skip `regenerate_nested_buffers`, and an editing
  toggle must invalidate any previously computed table even when the AST
  strings are unchanged.
- **Phase 3B1 adds no capture-only side channel.** Do not add a dedicated
  WASM export, `REQUEST_COMMENT_CAPTURE` message, generation transport,
  baseline patching, in-flight top-up, or comment-edit queue.

### Phase 3A — Skip the discarded project capture

No response-shape, TypeScript-call-site, or comment/editing behavior
change. An armed project render drops from three QMD parses to two;
single-document rendering remains at two until Phase 3B1. This is the
first implementation checkpoint in the shared Phase 3A/3B1 strand.

**Status: complete 2026-11-09 (bd-wpmqf039, implementation `3010beb04`).**
Phase 3B1 continues under the same strand; Phases 3B2 and 3C are outside the
current implementation scope.

- [x] File one Phase 3A/3B1 strand, mark it `in_progress`, and record the
  current project-path `render.wasm-call` baseline at 1x/8x with the Node
  driver. Do not block this checkpoint on a new Rust timer: the cited
  `std::env::var_os("QUARTO_PERF_STATS")` pattern is not available as a
  browser-WASM profiling mechanism. The same-driver before/after wall-time
  comparison below measures the deleted capture directly enough.
  → bd-wpmqf039. Unmodified release WASM, 10 warm-cache project renders per
  scale (`QUARTO_PERF_STATS=1 ENTRY=project CAPTURE=default SCALES=1,8
  RENDERS=10 node --import tsx/esm driver.mjs`): **356.940 ms/render at 1x**
  (99 KB; envelope parse 107,168,994 bytes) and **2811.396 ms/render at 8x**
  (792 KB; 882,771,214 bytes).
- [x] Test first, as behavior-preserving characterization: add a focused
  `hub-client/src/services/previewCapture.wasm.test.ts` test through the
  raw WASM project entry. Record the current SHA-256 hashes of `ast_json`
  and `untransformed_ast_json` for a fixed multi-file project so the test
  pins their exact bytes before the refactor and must remain green after
  it. Use a relative active path and assert that the original source name,
  not canonical `/project/...`, remains in the serialized AST context.
  Keep the existing `applyNodeEdit.wasm.test.ts` round trips unchanged as
  the proof that the response baseline still matches the retained
  `renderedContent` bytes.
  → Green on the unmodified build: transformed
  `c98fd4c8a875b4f8a8cb5e910153b0ba8b3e6cb1502c0d8bbbe15cd3021afeab`,
  untransformed
  `9b22dd9a3495eba60a3ae6b3efecbfbd254e0761125a73ba8587eaa8a2003733`;
  source name `index.qmd`.
- [x] Add an internal capture-aware implementation of
  `render_qmd_to_preview_ast`, retaining the existing public function as a
  capture-on wrapper. Project pass 2 calls the internal implementation
  with capture off so its discarded parse and serialization never happen.
  Leave `render_project_active_page_to_response` and
  `render_single_doc_to_response` unchanged in this checkpoint. Do not add
  `untransformed_ast_json` to `WasmPassTwoOutput`.
- [x] Run focused Rust and raw-WASM regression tests, then the full
  verification chain in Conventions. No `.snap` changes are expected from
  this internal plumbing; audit and report any that occur rather than
  assuming a snapshot round.
  → Seven focused core tests and strict clippy passed; the rebuilt raw-WASM
  characterization plus unchanged `applyNodeEdit` round trips passed 27/27.
  `cargo xtask verify --skip-rust-tests` passed every leg. The final selected
  workspace run passed **19,193/19,193** tests, excluding the two known
  R-package-version cases in bd-qwscag8b (`capture_classifies_pieces` and
  `a_sql_chunk_with_hash_pipe_options_records_as_it_runs_live`) and the
  load-sensitive `socket_group_relaxes_permissions`. The latter passed in
  isolation. `head_peek_timeout_closes` also failed once by 136 microseconds
  under load, then passed in isolation and in the following full run. The
  unfiltered `cargo xtask verify` is therefore not green on this machine:
  its first Rust leg stops at the documented `rlang_error`/`simpleError`
  skew; CI's pinned R environment remains the backstop. No `.snap` files
  changed.
- [x] Measure the same project fixture at 1x/8x and record
  `render.wasm-call` before/after. `wasm-envelope.parse` bytes should be
  unchanged because the response still carries the same capture; the win
  is the removed parse and serialization inside the WASM call.
  → Same driver, release WASM, 10 renders/scale: 1x **356.940 → 284.122 ms**
  (−72.818 ms, **−20.4%**); 8x **2811.396 → 2211.554 ms** (−599.842 ms,
  **−21.3%**). Envelope bytes are exactly unchanged at both scales
  (107,168,994 and 882,771,214).
- [x] Browser-smoke a multi-file hub project: a rich-text content edit and
  a comment add/resolve with editing off both land in the QMD. Record the
  invocation and observed result, or explicitly record the browser debt.
  Commit this green checkpoint, following the hub-client changelog
  two-commit workflow when the commits touch `hub-client/`; keep the
  strand open for Phase 3B1.
  → Built with `cd hub-client && VITE_E2E=1 VITE_DEFAULT_SYNC_SERVER=/ws
  npm run build`, then drove headless Chromium with the project factory's
  hub on port 3031. A throwaway multi-file spec plus the existing inline-edit
  control passed 2/2: a paragraph edit reached Automerge, and a comment added
  and resolved with editing off appeared in and disappeared from `main.qmd`.
  The throwaway spec was deleted after the run. The existing real-ProseMirror
  control (`q2-preview-richtext-bold-content-loss.spec.ts`, "bolding a
  paragraph preserves its content") also passed 1/1 and produced
  `**Hello world paragraph.**` in Automerge. The output was inspected through
  the test hooks, not inferred from successful renders alone.

### Phase 3B1 — Gate capture without changing visible-comment behavior

This is the current implementation checkpoint and completes the shared strand.
An unarmed single-document render drops from two QMD parses to one. An
unarmed project render drops from two to one relative to Phase 3A (from
the original three to one overall); an armed project render remains at
two. The flag-off response omits `untransformed_ast_json`; `ast_json`
remains byte-identical. Default comments-visible hub renders remain armed
unless the deferred Phase 3B2 is later activated, and fresh-default
editing-enabled renders remain armed in every variant.

- [ ] Continue the Phase 3A/3B1 strand and write the Rust and raw-WASM
  tests first. Cover both single-document and multi-file project paths:
  flag off omits `untransformed_ast_json`; flag on returns bytes identical
  to Phase 3A; transformed `ast_json` is identical in both modes. Also pin
  omitted options and an omitted flag to the same capture-on bytes as an
  explicit flag-on call. Confirm the new flag-off assertions fail on the
  unmodified build. Leave existing raw-WASM callers that consume the
  baseline, including `applyNodeEdit.wasm.test.ts`, unchanged.
- [ ] Write the TypeScript behavior tests before implementation:
  - hub `previewEditing=true` arms capture;
  - hub editing off with `commentsMode='show'` or `'expand'` also arms it,
    and real `PreviewRoot` read-mode comment add/resolve still works;
  - hub editing off with `commentsMode='hide'` leaves it unarmed;
  - entering editing and changing comments from hidden to visible each
    cause exactly one armed render through the existing effect, with no
    additional retry render;
  - SPA `allowEdit=true` arms the first render, while `allowEdit=false`
    leaves capture, content editing, commenting, and nested regeneration
    off;
  - nested buffers are regenerated only in editing mode, never merely
    because a read-mode render was armed for visible comments, and an
    editing toggle invalidates a previously computed table even when the
    AST strings are unchanged.
- [ ] Extend the real `CommentBlock.readMode.integration.test.tsx` tests
  before the UI implementation. With no baseline there is no new-node `+`,
  and existing comments have no input or resolve buttons. Cover both an
  initial baseline-less mount and an armed-to-unarmed rerender with an
  existing-comment input already open; the input must be suppressed rather
  than remaining available for a null-source no-op. Once an armed render
  supplies the baseline, normal block and span add/resolve UI appears
  unchanged. Existing unsupported-node and read-only-host cases remain
  unchanged.
- [ ] Thread `capture_untransformed_ast` through both hot WASM render
  paths. Single-document rendering passes it to the internal core
  implementation from Phase 3A. Project pass 2 remains capture-off; the
  project response keeps or skips its existing original-`active_path`
  capture according to the flag. When false, skip the capture entirely —
  no parse, serialization, or envelope field. Preserve the public core
  wrapper and both public WASM entries as capture-on when their options or
  flag are omitted, so unrelated native and raw-WASM tests need no
  mechanical edits.
- [ ] Add `captureUntransformedAst` to the two public wrapper option shapes.
  Use `previewEditing || (commentsMode ?? 'show') !== 'hide'` for hub and
  `allowEdit` for the SPA. Add those arming inputs to the existing render
  callback dependencies so the current effect performs the rerender. Do
  not add pending edits, lazy retries, or a separate entering-editing
  effect.
- [ ] Make baseline-less comments read-only in `CommentWrapper` by deriving
  readiness from `PreviewContext.sourceIndex`. Include a missing source
  index in the existing read-only gate and suppress the inline input even
  when it was already open. Add no readiness prop, iframe message, or
  comment-edit queue.
- [ ] Make the nested-buffer conditions and memo dependencies explicit in
  `computeNestedEditBuffers` call sites: hub uses
  `previewEditing && (unlockNestingCursor ?? true)` with `previewEditing`
  in the dependency list; the SPA uses `allowEdit && nestingCursor` with
  `allowEdit` in its dependency list. Update the existing SPA nesting
  fixtures that currently set `allow_edit: false` while expecting
  regeneration to use `allow_edit: true`, and add a read-only
  no-regeneration case. Keep comment-only armed renders from paying the
  `regenerate_nested_buffers` call.
- [ ] Run focused Rust, preview-runtime, preview-renderer, hub-client, and
  SPA tests, then the full verification chain in Conventions. Audit
  `.snap` files and report counts and surprises if any change; no snapshot
  churn is assumed from the optional response field.
- [ ] Measure same-build flag-on/flag-off runs at 1x/8x for both
  single-document and project paths. Record `render.wasm-call`,
  `wasm-envelope.parse` bytes, and the arming mode used. Flag-on is the
  Phase 3A comparison point; flag-off isolates the gating win. The hub
  unarmed measurement must explicitly use `previewEditing=false` and
  `commentsMode='hide'`. Include editing-on and comments-visible armed
  controls; do not present either as an unarmed win.
- [ ] End-to-end smoke both hosts. In hub, verify content editing after
  entering edit mode, comment add/resolve with editing off and comments
  visible, and comments hidden/visible transitions, including the
  baseline-less interval before the armed render completes. In the SPA,
  verify a read-only session and an `--allow-edit` session. Record
  invocations and observed behavior, then finish the hub-client changelog
  two-commit workflow and close the strand with the measurements.

### Phase 3B2 — Deferred: one armed render on comment intent

Do not file an implementation strand with Phases 3A/3B1. Phase 3B1 leaves
read-mode, comments-visible renders armed; this deferred phase targets
only that non-default configuration. Fresh-default hub sessions have
`previewEditing=true` and remain armed. Reconsider activation only when
the Phase 3B1 measurements show that the ordinary-render saving justifies
a new host action and its race handling.

- [ ] Measurement gate only: after Phase 3B1, measure its target
  configuration at 1x/8x with `previewEditing=false` and
  `commentsMode='show'`. Record the potential ordinary-render saving and
  an editing-enabled armed control. Decide explicitly whether to activate
  this phase; otherwise leave it deferred.

If a later review activates the phase, use these design constraints rather
than treating them as current implementation work:

- **Direct iframe only.** A host-side **Enable comments** action sets a
  path-keyed `commentCaptureArmedForPath` React state value. Arm hub
  capture with
  `previewEditing || format === 'q2-sandboxed-preview' || commentCaptureArmedForPath === currentFile?.path`.
  Thus the direct iframe can opt in on comment intent while sandbox
  capture stays eager. The SPA condition remains `allowEdit`.
- **Sandbox work is separate.** Do not deploy baseline-less sandbox
  renders in this phase. A future sandbox strand must handle old or cached
  iframe deployment and the existing `editingDisabled` transport gap
  before changing its eager-capture policy. No compatibility handshake is
  needed for the direct-only activation.
- **Reuse the full-render path.** The first state change deliberately uses
  the existing render effect and serial/latest-wins queue, increasing
  `render.started` exactly once. Repeated activation deduplicates;
  subsequent natural renders for that path stay armed. Retry invokes the
  same armed render. Do not use a ref latch, a separate entering-editing
  effect, or an effect-order-dependent file reset.
- **Correlate armed and unarmed requests.** Extend the compound `rendered`
  state with `sourcePath`, use it for the matching `ReactRenderer` props,
  and key accepted-render staleness by path + content + capture-armed
  state. Path + content prevents a same-content file switch from accepting
  the old path's result; the armed bit prevents an identical-content
  unarmed render already in flight from owning comment-intent pending,
  failure, Retry, or readiness. The queued armed render publishes the
  final comment-ready state without a top-up. Add race tests for both
  cases; no numeric generation protocol is needed.
- **Reuse the Phase 3B1 read-only gate.** Existing comments remain
  readable without a baseline, while add/resolve controls and inputs stay
  suppressed. The Enable comments action belongs to `ReactPreview`, not
  `PreviewRoot`. Keep it pending until the matching armed baseline is
  published. No comment text or edit is queued.
- **Keep editing work separate.** None of these paths enables nested
  regeneration in read mode; the Phase 3B1 editing guards and memo
  dependencies remain unchanged.
- **Add no capture-only side channel.** Reuse the Phase 3B1 flag and
  existing queue. No dedicated capture export, `REQUEST_COMMENT_CAPTURE`
  message, iframe pending/error transport, baseline patching, or in-flight
  top-up is part of this design.
- **Verify only if activated.** Write the `ReactPreview` behavior and race
  tests first, run the full verification chain, and browser-smoke the
  direct iframe: no capture before intent, one armed render on activation,
  block and span add/resolve, subsequent armed natural renders,
  failure/Retry, and same-content path and armed-state races. Measure
  ordinary no-intent cost, first-intent full-render latency, and exactly
  one `render.started` increase against Phase 3B1. Keep sandbox regression
  coverage on its eager path.

If the measured first-intent full-render latency is unacceptable, stop and
write a separate capture-only design justified by those numbers. That
design must cover original-source-name byte parity, request/generation
correlation, stale top-up revalidation and failure, direct and sandbox
`event.source` checks, a real paint-yielding mechanism, and old or cached
iframe deployment. Do not implement that side channel as part of this
phase.

### Phase 3C — Deferred: AST transport and proxy-manifest relocation

Do not implement this with Phases 3A/3B. Reconsider it only if post-3B
profiles still show significant main-thread envelope parsing or
parent-side AST work; it may then fit better with Phase 5's scoped
envelope work. It is independent of Phase 4: binary asset transfer changes
the VFS read and service-worker response paths, not the AST response shape.

The activation design must account for the full current consumer and
deployment inventory:

- `ReactRenderer.tsx:231` parses `astJson` for custom-component paths for
  `q2-sandboxed-preview` as well as the direct iframe formats. Moving only
  the proxy manifest therefore does not eliminate parent AST parsing on
  the sandboxed path.
- `ReactAstSlideRenderer.tsx:77`, `useCursorToSlide.ts:29`, and
  `useSlideThumbnails.tsx:94` also parse on the parent side. The SPA has
  sibling parses for custom components and the document title. Any claim
  of "single-parse AST transport" must say exactly which parses it removes.
- A wasm-bindgen out-param or multi-return changes success/error
  representation, generated and hand-written declarations, wrappers, and
  direct raw-WASM callers. Design that contract separately rather than
  treating it as a small addition to the capture-option change.
- Moving `buildProxyAssetManifest` into `updateAst` remains a plausible
  follow-up because it is pure in `(astJson, currentFilePath)` and needs no
  VFS reads. Preserve the current ordering guarantee by deriving the
  manifest synchronously before `root.render`. The VFS-reading,
  origin-scoped blob manifest in `Q2PreviewIframe.tsx` stays parent-side.
- The sandbox iframe is deployed separately. Roll out a dual-mode iframe
  that derives a manifest when absent and honors a supplied manifest first;
  remove the parent field only after that deployment. Add real iframe-side
  tests and a test runner for `quarto-hub-sandboxed-preview` rather than
  moving the existing test outside hub-client discovery.
- Sharing the already-parsed AST from `PreviewRoot` is not a free optional
  cleanup: that parsed object currently lives inside `PreviewRoot`, while
  `updateAst` runs before `root.render`. Profile before restructuring it.
- Cross-origin iframes do not guarantee a separate OS process under every
  browser and local deployment. Measure the actual production-like
  topology rather than assuming every iframe parse is off the parent event
  loop.

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
