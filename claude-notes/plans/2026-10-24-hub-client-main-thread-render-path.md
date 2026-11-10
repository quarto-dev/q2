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

Phases 0-2, 3A, and 3B1 are complete. **Phase 3B1 completed on 2026-11-10
under bd-wpmqf039. Phases 3B2 and 3C remain deferred.** Phase 3B1 preserves
eager capture whenever comments are visible or editing is enabled; it does not
redesign comment interactions. This keeps the capture optimization independent
of ongoing comment work. The comment-intent UX in Phase 3B2 and the transport
changes in Phase 3C require fresh measurements before activation.

Phase 5 implementation and measurement are complete under bd-lxq4oac3:
identical-request sharing plus a model-quiet window reduced each measured
Monaco burst to one tokenization. Its commit awaits approval because the full
workspace run has two unrelated local knitr failures; all other verification
legs pass. See the Phase 5 evidence below.

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

**Status: complete 2026-11-10 (bd-wpmqf039, implementation `c7bc73610`).** This checkpoint completes the
shared strand. An unarmed single-document render drops from two QMD parses to one. An
unarmed project render drops from two to one relative to Phase 3A (from
the original three to one overall); an armed project render remains at
two. The flag-off response omits `untransformed_ast_json`; `ast_json`
remains byte-identical. Default comments-visible hub renders remain armed
unless the deferred Phase 3B2 is later activated, and fresh-default
editing-enabled renders remain armed in every variant.

- [x] Continue the Phase 3A/3B1 strand and write the Rust and raw-WASM
  tests first. Cover both single-document and multi-file project paths:
  flag off omits `untransformed_ast_json`; flag on returns bytes identical
  to Phase 3A; transformed `ast_json` is identical in both modes. Also pin
  omitted options and an omitted flag to the same capture-on bytes as an
  explicit flag-on call. Confirm the new flag-off assertions fail on the
  unmodified build. Leave existing raw-WASM callers that consume the
  baseline, including `applyNodeEdit.wasm.test.ts`, unchanged.
  → Native gate characterization passed. On the unmodified WASM, both new
  flag-off cases failed because the baseline was still present; all three
  default-on/byte-characterization controls passed. `applyNodeEdit` is unchanged.
  An isolated Phase 3A build additionally pinned single-document transformed
  `8599765d7edeaabd48916d0c4629010262189443614f440d867e8798f6e0314c`,
  the same untransformed hash as the project path, and source `index.qmd`.
- [x] Write the TypeScript behavior tests before implementation:
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
  → Before implementation, the eight hub arming/rerender/nesting cases and
  five SPA option/read-only cases failed as intended; 67 SPA controls passed.
  The three real `PreviewRoot` read-mode add/resolve controls also passed.
- [x] Extend the real `CommentBlock.readMode.integration.test.tsx` tests
  before the UI implementation. With no baseline there is no new-node `+`,
  and existing comments have no input or resolve buttons. Cover both an
  initial baseline-less mount and an armed-to-unarmed rerender with an
  existing-comment input already open; the input must be suppressed rather
  than remaining available for a null-source no-op. Once an armed render
  supplies the baseline, normal block and span add/resolve UI appears
  unchanged. Existing unsupported-node and read-only-host cases remain
  unchanged.
  → The existing-comment initial-mount and armed-to-unarmed cases failed on
  the unsuppressed input before implementation. The new-node and all six
  existing read-mode/host controls passed.
- [x] Thread `capture_untransformed_ast` through both hot WASM render
  paths. Single-document rendering passes it to the internal core
  implementation from Phase 3A. Project pass 2 remains capture-off; the
  project response keeps or skips its existing original-`active_path`
  capture according to the flag. When false, skip the capture entirely —
  no parse, serialization, or envelope field. Preserve the public core
  wrapper and both public WASM entries as capture-on when their options or
  flag are omitted, so unrelated native and raw-WASM tests need no
  mechanical edits.
  → Release WASM rebuilt successfully. The five capture characterization /
  gating tests and all 26 unchanged `applyNodeEdit` round trips passed 31/31.
- [x] Add `captureUntransformedAst` to the two public wrapper option shapes.
  Use `previewEditing || (commentsMode ?? 'show') !== 'hide'` for hub and
  `allowEdit` for the SPA. Add those arming inputs to the existing render
  callback dependencies so the current effect performs the rerender. Do
  not add pending edits, lazy retries, or a separate entering-editing
  effect.
  → Both perf branches of the runtime-forwarding test pass 2/2 (8/8 file
  total). All hub arming and one-render transition cases pass; all SPA capture
  and option-compatibility cases pass. Only the separate nested-gate items
  remain red at this checkpoint.
- [x] Make baseline-less comments read-only in `CommentWrapper` by deriving
  readiness from `PreviewContext.sourceIndex`. Include a missing source
  index in the existing read-only gate and suppress the inline input even
  when it was already open. Add no readiness prop, iframe message, or
  comment-edit queue.
  → All eight focused comment files pass 57/57, including real `PreviewRoot`
  block/span add/resolve, baseline loss/restoration, unsupported nodes, and
  read-only hosts. Writable direct-context fixtures now declare readiness
  explicitly with a non-null source index.
- [x] Make the nested-buffer conditions and memo dependencies explicit in
  `computeNestedEditBuffers` call sites: hub uses
  `previewEditing && (unlockNestingCursor ?? true)` with `previewEditing`
  in the dependency list; the SPA uses `allowEdit && nestingCursor` with
  `allowEdit` in its dependency list. Update the existing SPA nesting
  fixtures that currently set `allow_edit: false` while expecting
  regeneration to use `allow_edit: true`, and add a read-only
  no-regeneration case. Keep comment-only armed renders from paying the
  `regenerate_nested_buffers` call.
  → Hub 17/17 and SPA 72/72 focused integration tests pass. The positive SPA
  fixtures use the production `allowEdit: true` key (the former snake-case key
  was ignored); the read-only case verifies no regeneration. The hub editing
  toggle recomputes with unchanged AST strings; comment-only read mode does not.
- [x] Run focused Rust, preview-runtime, preview-renderer, hub-client, and
  SPA tests, then the full verification chain in Conventions. Audit
  `.snap` files and report counts and surprises if any change; no snapshot
  churn is assumed from the optional response field.
  → Focused core 8/8 and native clippy passed; raw-WASM 31/31; preview-runtime
  92/92; preview-renderer 643 unit + 723 integration; SPA 95 unit + 97
  integration plus production build. `cargo xtask verify --skip-rust-tests`
  passed all 14 legs, including hub-client 1746 unit + 240 integration + 478
  WASM tests. The selected workspace run passed **19,193/19,193**, excluding
  the three Phase 3A environment cases and newly filed bd-wwmnuqxr. Standalone
  WASM clippy reaches pre-existing lints tracked as bd-wn2pd12d. No `.snap`
  files changed.
- [x] Measure same-build flag-on/flag-off runs at 1x/8x for both
  single-document and project paths. Record `render.wasm-call`,
  `wasm-envelope.parse` bytes, and the arming mode used. Flag-on is the
  Phase 3A comparison point; flag-off isolates the gating win. The hub
  unarmed measurement must explicitly use `previewEditing=false` and
  `commentsMode='hide'`. Include editing-on and comments-visible armed
  controls; do not present either as an unarmed win.
  → Same release WASM `bf2bc87eb6fa`, production wrapper/init path, warm SASS
  cache, 10 renders/scale. Ran `QUARTO_PERF_STATS=1 ENTRY=<single|project>
  CAPTURE=<on|off> SCALES=1,8 RENDERS=10 node --import tsx/esm --import
  <scratch>/phase3b1-perf/register.mjs <scratch>/phase3b1-perf/driver.mjs`.
  Single (`allowEdit=true → false`): 1x **164.935 → 86.728 ms** (−47.4%);
  8x **1305.639 → 712.920 ms** (−45.4%). Envelope bytes: 1x 89,356,660 →
  45,229,180; 8x 737,121,840 → 372,181,160.
  Project (`previewEditing=true, commentsMode=show` →
  `previewEditing=false, commentsMode=hide`): 1x **170.617 → 90.004 ms**
  (−47.2%); 8x **1345.331 → 712.938 ms** (−47.0%). Envelope bytes: 1x
  89,366,940 → 45,252,910; 8x 737,319,610 → 372,392,800.
  Armed project controls at 1x/8x: editing-on (`previewEditing=true,
  commentsMode=hide`) **165.160 / 1299.047 ms**; comments-visible
  (`previewEditing=false, commentsMode=show`) **162.329 / 1294.363 ms**.
  Both have exactly the flag-on envelope bytes above; neither is presented as
  an unarmed win.
- [x] End-to-end smoke both hosts. In hub, verify content editing after
  entering edit mode, comment add/resolve with editing off and comments
  visible, and comments hidden/visible transitions, including the
  baseline-less interval before the armed render completes. In the SPA,
  verify a read-only session and an `--allow-edit` session. Record
  invocations and observed behavior, then finish the hub-client changelog
  two-commit workflow and close the strand with the measurements.
  → Hub built with `VITE_E2E=1 VITE_DEFAULT_SYNC_SERVER=/ws
  VITE_Q2_SANDBOXED_PREVIEW_URL=q2-sandboxed-preview/index.html npm
  --workspace hub-client run build`; `npm --workspace hub-client exec --
  playwright test e2e/phase3b1-smoke.spec.ts --project=chromium --workers=1
  --retries=0` passed **2/2**. Observed `Edited after the armed render.` in
  Automerge after entering editing from the unarmed state; comment add/resolve
  changed the QMD; a gated `show` + null-baseline update kept existing comments
  readable with no input, resolve, or new-comment `+`, and the released armed
  update restored controls. SPA refreshed with `cargo xtask
  build-q2-preview-spa && cargo build --bin q2`; `npm --workspace q2-preview-spa
  exec -- playwright test e2e/phase3b1-smoke.spec.ts --project=chromium
  --workers=1 --retries=0` passed **2/2**. The default session rendered
  read-only with no edit/comment mutation controls; the `--allow-edit` session
  wrote `Edited through the allowEdit session.` to `index.qmd` on disk. Outputs
  were inspected through Automerge/disk and browser locators, not inferred from
  render success. Throwaway specs are preserved in scratch, not committed.

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
relative to Phases 1-3. **Implementation and verification complete under
bd-lxq4oac3 except for two unrelated local knitr failures; commit awaits
user approval per the dirty-phase review policy.**

- [x] Baseline first: token-request frequency from Monaco during a
  keystroke burst, and synchronous tokenize ms per request at fixture
  scales. Check how Monaco's built-in ~300 ms semantic-tokens debounce
  interacts — and note that `refreshSemanticTokens()`
  (`useIntelligenceProviders.ts`, `services/monacoProviders.ts`)
  deliberately bypasses it via `schedule(0)` on mount and on WASM
  ready, so forced refreshes pay the synchronous cost immediately.
  → Fresh Node and real-Chromium baselines below.
- [x] Candidate fixes, cheapest first: (a) coalesce/debounce token
  requests in `monacoProviders` so a burst costs one synchronous run;
  (b) avoid the extra `vfsAddFile` full-text write when the VFS image is
  already current (dirty-check against the last write); (c) scoped
  serde-wasm-bindgen or typed returns for the three hottest envelopes
  (vfs write, semantic tokens, render response) instead of a blanket
  JSON.parse migration.
  → Took (a). The measured sub-1% token-path headroom does not justify (b)
  or semantic-token typed returns; render transport stays scoped to
  deferred Phase 3C.
- [x] Only if (a)-(c) are insufficient: evaluate moving LSP
  intelligence into a dedicated worker fed by the same sync callbacks
  (design note first; overlaps Phase 7).
  → Not activated: option (a) reduced each measured burst to one run.
- [x] Record what was measured, which option was taken, and why.
  → Baseline, rejected alternatives, TDD evidence, and final browser
  measurements below.

### Phase 5 baseline (2026-11-10, release WASM, production modules)

The throwaway harness uses the real `wasmRenderer` and
`intelligenceService`, with only Node module-resolution shims. The fixture
is `docs/guides/authoring/computations.qmd` without its front matter,
repeated to about 100 KB at 1x and scaled geometrically. Each token scale
ran one warm-up and ten content-changing requests. Invocation:

```sh
QUARTO_PERF_STATS=1 node --cpu-prof --cpu-prof-interval=200 \
  --import tsx/esm --import <scratch>/phase5-perf/register.mjs \
  <scratch>/phase5-perf/driver.mjs
```

| Scale | Source, UTF-16 units | Tokens | Tokenize avg | Semantic JSON parse, isolated avg | VFS write avg | Semantic envelope |
|---|---:|---:|---:|---:|---:|---:|
| 1x | 102,325 | 4,361 | 55.350 ms | 0.558 ms | 0.181 ms | 296,835 units |
| 2x | 204,589 | 8,713 | 108.951 ms | 1.056 ms | 0.249 ms | 594,539 units |
| 4x | 409,117 | 17,417 | 216.662 ms | 2.066 ms | 0.436 ms | 1,193,695 units |
| 8x | 818,173 | 34,825 | 434.742 ms | 4.103 ms | 0.809 ms | 2,401,919 units |

Tokenization scales linearly (7.9x from 1x to 8x). The isolated semantic
JSON parse is 1.0% of tokenization at 1x and 0.9% at 8x; the VFS write is
0.3% and 0.2%. The CPU profile agrees: `parseSemanticTokensResponse` is
0.8% of total self-time, while WASM frames dominate. A semantic-token
typed return or a duplicate-write check therefore cannot materially reduce
the synchronous token cost on its own.

The render comparison used the same fixture, an unarmed
`captureUntransformedAst: false` project render, warm SASS cache, one
warm-up, and five measured renders per scale:

| Scale | Render avg | Outer-envelope parse avg | Envelope units/render | Parse share |
|---|---:|---:|---:|---:|
| 1x | 137.972 ms | 6.107 ms | 5,497,091 | 4.4% |
| 8x | 1749.030 ms | 46.906 ms | 45,388,428 | 2.7% |

This keeps the render-envelope question relevant to Phase 3C, but the
cheapest first experiment is token-request elimination rather than a broad
transport change.

Real Monaco was measured in the production hub bundle through a throwaway
Playwright config and spec. The bundle was built with `VITE_E2E=1`,
`VITE_DEFAULT_SYNC_SERVER=/ws`, and the local sandbox URL. The driver ran
`npx playwright test --config=<scratch>/phase5-perf/playwright.config.ts
--project=chromium --workers=1 --retries=0`; both scenarios passed in
36.9s, and the final `BURSTMARK!` text was inspected in Monaco. Gauges were
snapshotted after mount/WASM-ready tokenization settled and again after a
10-character, 100-ms-paced burst plus trailing requests:

| Scale | Mount token runs | Mount token total | Burst token runs | Burst token total | Burst renders |
|---|---:|---:|---:|---:|---:|
| 1x | 2 | 135.5 ms | 2 | 84.7 ms | 2 |
| 8x | 2 | 771.2 ms | 3 | 1054.0 ms | 2 |

Monaco's adaptive debounce already collapses many edit requests, but does
not make this burst one synchronous run; the mount/WASM-ready path also
runs twice. Render coalescing remains intact at two renders per burst.
These measurements activate option (a) as the first implementation
experiment. Options (b) and the semantic-token half of (c) have sub-1%
headroom; the render half of (c) stays coupled to Phase 3C's transport
constraints rather than becoming a blanket JSON migration.

### Phase 5 implementation and result (2026-11-10)

`monacoProviders.ts` now uses two bounded mechanisms:

- A single latest `(path, content)` request shares concurrent and completed
  nonempty token results. This removes duplicate mount/WASM-ready work and
  lets Monaco's trailing request reuse the result from an earlier pending
  provider call. Empty results are not retained because the service also
  uses them for transient failures; disposal clears the entry.
- A 300 ms model-quiet window resets directly from Monaco content-change
  events. Content and version are snapshotted only after that window, and
  cancellation before the snapshot prevents VFS/WASM work. Monarch base
  highlighting remains immediate; semantic enrichment is the layer that
  waits for the quiet window.

TDD preceded both changes. The identical-request test failed with two
service calls instead of one. The quiet-window tests failed because the
provider tokenized `initial` immediately and cancellation did not prevent
that call. A 50 ms implementation passed the unit tests but produced mixed
Chromium results (2 then 3 runs at 1x, versus 2 at baseline), so it was
rejected rather than tuned blindly. Updating the test to require a 300 ms
quiet window failed at 50 ms, then passed at 300 ms. The final focused
suite has 17 passing tests, including latest-content, cancellation,
nonempty-cache, empty-retry, and disposal cases.

The same production-browser scenario used for the baseline was rerun with
render-readiness included in both snapshots. Two scenarios passed in 44.7s;
the final `BURSTMARK!` text was inspected in Monaco and all started renders
completed. The structural run-count changes are the primary result;
millisecond totals are same-scenario observations, not a general latency
budget.

| Scale | Mount runs, before → after | Burst runs, before → after | Burst token total, before → after | Change |
|---|---:|---:|---:|---:|
| 1x | 2 → 1 | 2 → 1 | 84.7 → 45.0 ms | −46.9% |
| 8x | 2 → 1 | 3 → 1 | 1054.0 → 367.9 ms | −65.1% |

Option (a) is therefore sufficient for this phase. No VFS dirty-check,
serde-wasm-bindgen dependency, typed render response, or intelligence
worker was added. The render envelope measurements remain available for
Phase 3C's separately deferred transport decision.

### Phase 5 verification

- `npm run build:all -w hub-client` passed, including release WASM, the
  sandbox bundle, production TypeScript/Vite build, and service-worker
  precache check.
- `npm run test:ci -w hub-client` passed: 1,751 unit, 240 integration, and
  478 WASM tests; preview/render DOM parity passed 109/109 opted-in
  fixtures. The focused provider suite passed 17/17 and the intelligence
  lifecycle integration suite passed 6/6.
- `cargo xtask verify --skip-rust-tests --skip-hub-build` passed every
  enabled leg, including custom lints, clippy, formatting, strict
  `claude-notes` render, ts-packages, hub-client tests, and preview SPA.
  Scoped ESLint passed for all three changed TypeScript files. Full
  hub-client ESLint reports 190 errors in unrelated files and none in this
  change set.
- The real-browser end-to-end invocation and output inspection are recorded
  above: two scenarios passed in 44.7s, the final edited text was visible,
  and each measured burst produced one actual tokenization.
- The full Rust workspace run used an official scratch pandoc 3.13 because
  the system binary is 3.12.1. It completed all 19,203 tests: 19,201 passed,
  201 skipped, and two failed. `capture_classifies_pieces` is the known
  `rlang_error`/`simpleError` environment issue (bd-54odm3rz,
  bd-qwscag8b). `knitr_replay_equals_live::a_sql_chunk_with_hash_pipe_options_records_as_it_runs_live`
  rejects `#|` in the live engine and reproduces in isolation; it is filed
  as bd-b95dt1ia. No Rust source changed in this phase, so these unrelated
  failures remain open rather than being patched or hidden here.
- No snapshot files changed.

## Phase 6 — Attribution production main-thread cost (finding 7)

The preview overlay is session-only and defaults off; its incremental updates
are debounced, but cold builds start immediately. DOCX/PPTX downloads also
build attribution on demand, independently of the toggle. Cold replay is
history-dependent; incremental updates currently traverse full history again
for bookkeeping, run updates may shift or splice the run array, and payload
construction is O(source length + runs).

### Phase 6a — Cold-build edit race (correctness)

A correctness bug, independent of the performance gate; it gets its own
strand and lands before the 6b baseline so the baseline measures correct
behavior.

`buildRunListAttribution` snapshots history and the document when it starts.
An edit that arrives during the build is not replayed, and both callers then
pair the stale runs with newer text: the hook schedules no incremental update
while `stateRef.current` is null and finalises through the latest
`sourceTextRef.current`; the download reads `handle.doc().text` after the
await (`downloadAttribution.ts`). `runsCharToByteOffsets` hides out-of-range
offsets behind its `??` fallback.

- [x] Failing regression tests first, one per caller: an edit during a cold
  build yields a payload whose runs match the final text. Cover multiple
  edits, disable or file-switch during the build, and late completion.
- [x] Fix in the producer, not the hook lifecycle: before
  `buildRunListAttribution` returns, replay `getChanges(workDoc, handle.doc())`
  until it is empty, so the returned state is at the handle's current heads.
  One change covers both callers. If the hook's `sourceText` lags the handle,
  the existing debounced effect corrects it, because `stateRef` is then set.
- [x] Verify in a real browser: type during a cold build on a long-history
  document with attribution enabled, and inspect the overlay.

**Status: implementation and browser verification complete on 2026-11-10
under bd-oul15aql.** The producer now replays the live delta without another
yield and returns `getHeads(workDoc)`. The hook and download callers are
unchanged.

### Phase 6a verification

- Before the fix, the hook, file-switch, and download regressions all failed
  with only the initial 501-character run; the edits at times 2 and 3 were
  missing. The disable/late-completion guard already passed. After the fix,
  all 29 focused attribution tests passed.
- `npm run build:all -w hub-client` passed, including release WASM, the
  sandbox bundle, production TypeScript/Vite build, and precache check.
- `npm run test:ci -w hub-client` passed: 1,755 unit, 240 integration, and
  478 WASM tests; preview/render DOM parity passed 109/109 opted-in fixtures.
- Scoped ESLint, `npm run typecheck -w hub-client`, `cargo fmt --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, and
  `cargo xtask lint` passed. The strict `claude-notes` render passed all 1,702
  files. No snapshot files changed.
- The full Rust workspace run used an official scratch pandoc 3.13 because
  the system binary is 3.12.1. It completed all 19,203 tests: 19,201 passed,
  201 skipped, and the same two unrelated local knitr failures recorded for
  Phase 5: `capture_classifies_pieces` (bd-54odm3rz, bd-qwscag8b) and
  `knitr_replay_equals_live::a_sql_chunk_with_hash_pipe_options_records_as_it_runs_live`
  (bd-b95dt1ia). No Rust source changed in this phase.
- Real Chromium verification used a throwaway Playwright spec and config in
  the scratch directory (not committed). The spec generated 501 real edits,
  gated the cold build at its first idle callback, typed into Monaco, proved
  the sentinel was in Automerge before releasing the callback, and inspected
  the resulting attribution wrapper and hover badge. Invocation:

  ```bash
  npm exec -w hub-client -- playwright test \
    --config=/tmp/pa-502/928e87e0-3b75-4558-9782-1fe213fb5f2b/playwright.attribution.config.ts \
    --project=chromium --workers=1 --retries=0
  ```

  Observed output:

  ```text
  COLD_BUILD_HISTORY 502
  COLD_BUILD_EDIT_CONFIRMED_BEFORE_RELEASE
  COLD_BUILD_OVERLAY {"typedText":"COLD_BUILD_SENTINEL","typedActor":"e2e7e1f02a30000000000000000007e1","badge":"Gentle Koala just now"}
  1 passed
  ```

  The screenshot was inspected: `COLD_BUILD_SENTINEL` has the author color,
  and its visible hover badge reads "Gentle Koala just now".

### Phase 6b — Main-thread cost

Measure before choosing between a cheaper algorithmic fix and a worker.

- [x] Baseline the real producer using the 2026-04-23 attribution profiling
  session as the template; the `requestIdleCallback` override is mandatory.
  Use geometric history, source-length, and run-count scales. Cover cold
  builds, debounced incremental updates, and DOCX/PPTX download attribution.
  Record main-thread CPU, longest synchronous slice, and elapsed time;
  distinguish CPU from idle scheduling. Measure the unchunked cold-build
  prefix separately: `history()`, `getAllChanges`, and a `decodeChange` of
  every change for the hash index all run before the first yield.
- [x] Remove dead bookkeeping: `updateRunListAttribution` calls
  `handle.history()` (a full `topoHistoryTraversal`) after every nonempty
  delta solely to set `processedHistoryIndex`. Neither that field nor
  `processedHeads` has a consumer; delete both.
- [x] Try unifying the cold build with the incremental path: replay
  `getAllChanges(doc)` in order from `init()`, chunked, instead of walking
  `history()`. This drops the traversal, the hash index, the second
  `decodeChange` per change, and `decodeHeads`, and leaves one replay loop
  (which also subsumes the 6a catch-up). It depends on `getAllChanges`
  returning a causally valid order; confirm that first, then require the
  existing `attribution-runs.test.ts` invariant suite to pass unchanged.
- [x] Regression: payload identical before and after on fixtures with
  concurrent mixed author/actor histories, Unicode offsets, and incremental
  updates; toggle on/off flicker-free.
- [x] Re-measure. Only if the numbers still warrant a worker, write a design
  section before any implementation (wire shape, ongoing change feed,
  staleness and cancellation, failure fallback, download path), keeping the
  `{ runs, identities }` payload contract unchanged. Note that `DocHandle`
  objects do not transfer via export/import: `Repo.export()` serializes the
  full document on the calling thread, and `Repo.import()` sets up no sync.
- [x] Verify end-to-end in a real browser with attribution enabled, including
  an edit burst and toggle cycle. Record the invocation, inspected overlay
  output, and before/after main-thread measurements.

### Phase 6b baseline

Baseline completed on 2026-11-10 under bd-kckdpt8j, before producer changes.
The throwaway driver in
`/tmp/pa-502/6007c0d1-1233-4599-abe4-7193998e72be/attribution-perf/`
loads the real `attribution-runs.ts`, `buildAttributionPayload`, and
`attributionJsonFor` through `tsx`. A resolve-hook shim supplies only
`getFileHandle`; Automerge, the producer, payload builder, and DOCX/PPTX
attribution helper are the production implementations. It does not measure the
subsequent pandoc document conversion.

The driver installs the mandatory `requestIdleCallback` override before
importing production code. Immediate callbacks measure synchronous work; a
separate 2 ms delayed run prices idle scheduling. `main ms` below is the sum of
main-thread synchronous slices, not whole-process CPU (which includes `tsx`
loader threads). Each matrix entry is the median of three runs. `longest ms` is
the longest synchronous slice, including the unchunked prefix and first replay
chunk. Cold builds are measured before an eight-edit burst; incremental updates
apply that burst in one debounced update. The history column recorded by the
raw JSON therefore includes the eight later changes. Invocation from the repo
root:

```bash
QUARTO_PERF_STATS=1 node --enable-source-maps --import tsx/esm \
  --import "$PA_SCRATCH_DIR/attribution-perf/register.mjs" \
  "$PA_SCRATCH_DIR/attribution-perf/driver.mjs"
```

Raw output: `attribution-perf/baseline-main-thread.json`. Every measured cold,
incremental, DOCX, and PPTX payload agreed.

| History scale | Cold main ms | Longest ms | Elapsed with 2 ms idle delay | Incremental main ms | DOCX main ms | PPTX main ms |
|---:|---:|---:|---:|---:|---:|---:|
| 250 | 510.6 | 510.6 | 538.9 | 41.1 | 540.5 | 539.9 |
| 500 | 530.1 | 530.1 | 575.4 | 42.1 | 561.2 | 559.0 |
| 1000 | 603.2 | 533.2 | 651.5 | 45.9 | 640.2 | 634.6 |
| 2000 | 866.5 | 547.6 | 910.5 | 51.2 | 903.1 | 902.2 |
| 4000 | 1865.4 | 568.0 | 1932.2 | 62.7 | 1903.4 | 1919.9 |

The delayed run is separate and includes ordinary run-to-run noise. At 4000
changes there are seven nominal 2 ms idle waits, against 1.87 seconds of
synchronous work: scheduling is not the bottleneck. DOCX and PPTX downloads pay
the same cold-build cost as enabling the overlay.

| Source chars at 1000 changes | Runs after burst | Cold main ms | Longest ms | Incremental main ms | Payload main ms |
|---:|---:|---:|---:|---:|---:|
| 25,364 | 217 | 726.4 | 660.0 | 54.8 | 0.08 |
| 50,364 | 217 | 1343.9 | 1275.6 | 103.7 | 0.16 |
| 100,364 | 217 | 2567.7 | 2498.6 | 199.3 | 0.21 |
| 200,364 | 217 | 5076.8 | 5005.9 | 392.4 | 0.39 |
| 400,364 | 217 | 9968.0 | 9899.4 | 773.6 | 0.77 |

| Text-edit scale at 2000 changes | Runs after burst | Cold main ms | Longest ms | Incremental main ms |
|---:|---:|---:|---:|---:|
| 50 | 117 | 680.6 | 344.4 | 52.0 |
| 100 | 217 | 881.5 | 554.7 | 51.8 |
| 200 | 417 | 1282.4 | 979.0 | 52.8 |
| 400 | 812 | 2117.1 | 1864.5 | 54.7 |
| 800 | 1594 | 3973.5 | 2347.1 | 58.3 |

The separate prefix probe calls the same three operations as the current cold
build. `decode + index` includes its own `getAllChanges` call, matching the
producer loop; the middle column isolates retrieval for comparison.

| Fixture | `history()` ms | `getAllChanges` ms | `decode + index` ms |
|---|---:|---:|---:|
| 258 changes, 20k chars | 6.8 | 5.5 | 35.3 |
| 4008 changes, 20k chars | 28.8 | 11.8 | 53.6 |
| 1008 changes, 400k chars | 107.5 | 103.2 | 650.1 |
| 2008 changes, 1594 runs | 18.0 | 9.0 | 46.5 |

The longest cold slice is larger than this prefix because it also includes the
first replay chunk. In particular, replay around the 400k initial text dominates
the 9.90-second slice; the prefix is still an unchunked 0.76 seconds and the
incremental path repeats `history()` solely for dead bookkeeping. Payload
construction is not a useful optimization target.

A separate V8 CPU profile used `--cpu-prof-interval=200` and was analyzed with
`hub-client/scripts/perf/analyze-cpuprofile.mjs`. Its 164.8 seconds of samples
include fixture generation and the whole matrix. Automerge WASM frames
(`OpIter`, `BatchApply`, and `Untangler`) dominate self-time; only 173 ms is
attributed directly to `attribution-runs.ts` frames and 5 ms to the payload
builder. This confirms that the replay itself, not TypeScript payload plumbing,
is the cost to evaluate after removing redundant traversal and decoding.

### Phase 6b implementation and re-measurement

The producer no longer stores `processedHeads` or `processedHistoryIndex`, and
incremental updates no longer call `history()`. Cold builds replay
`getAllChanges(doc)` from `init()` through the same `replayChange` function as
incremental updates. The topology traversal, hash index, second decode per
change, and head decoding are removed. The 6a live-delta catch-up loop remains
unchanged. Test handles no longer implement `history()`, so an accidental cold
or incremental dependency fails instead of silently restoring the traversal.

A fail-first regression observed one incremental `history()` call before the
fix and zero after it. The existing invariant suite is unchanged and passes. A
new concurrent fixture with legacy actors, an author-bearing session, a seq-2
change, Unicode offsets, and an incremental merge proves every
`getAllChanges` entry is immediately applicable in order and pins the exact
cold/incremental `{ runs, identities }` payload.

The identical after driver wrote
`attribution-perf/after-main-thread.json`; all cold, incremental, DOCX, and
PPTX payload comparisons passed again. Values below are before → after
main-thread milliseconds from the same three-run medians.

| History scale | Cold main ms | Longest slice ms | Incremental main ms |
|---:|---:|---:|---:|
| 250 | 510.6 → 474.3 | 510.6 → 474.3 | 41.1 → 34.5 |
| 500 | 530.1 → 491.1 | 530.1 → 491.1 | 42.1 → 33.9 |
| 1000 | 603.2 → 560.7 | 533.2 → 492.8 | 45.9 → 35.9 |
| 2000 | 866.5 → 808.4 | 547.6 → 494.0 | 51.2 → 34.8 |
| 4000 | 1865.4 → 1807.9 | 568.0 → 505.9 | 62.7 → 34.8 |

| Source chars | Cold main ms | Longest slice ms | Incremental main ms |
|---:|---:|---:|---:|
| 25,364 | 726.4 → 675.8 | 660.0 → 609.8 | 54.8 → 43.4 |
| 50,364 | 1343.9 → 1255.1 | 1275.6 → 1189.4 | 103.7 → 84.5 |
| 100,364 | 2567.7 → 2418.1 | 2498.6 → 2351.6 | 199.3 → 169.5 |
| 200,364 | 5076.8 → 4750.2 | 5005.9 → 4682.8 | 392.4 → 338.3 |
| 400,364 | 9968.0 → 9326.5 | 9899.4 → 9260.9 | 773.6 → 663.2 |

| Text-edit scale | Cold main ms | Incremental main ms |
|---:|---:|---:|
| 50 | 680.6 → 614.3 | 52.0 → 34.3 |
| 100 | 881.5 → 807.9 | 51.8 → 34.4 |
| 200 | 1282.4 → 1211.7 | 52.8 → 35.5 |
| 400 | 2117.1 → 2048.4 | 54.7 → 37.2 |
| 800 | 3973.5 → 3891.6 | 58.3 → 40.4 |

| History scale | DOCX main ms | PPTX main ms |
|---:|---:|---:|
| 250 | 540.5 → 509.8 | 539.9 → 509.7 |
| 500 | 561.2 → 535.9 | 559.0 → 535.8 |
| 1000 | 640.2 → 592.6 | 634.6 → 593.6 |
| 2000 | 903.1 → 849.5 | 902.2 → 851.6 |
| 4000 | 1903.4 → 1823.6 | 1919.9 → 1826.7 |

Cold cost falls by 3–10 percent; incremental cost falls by 14–45 percent.
Those are worthwhile constant-factor wins, but they do not change the
complexity or the user-blocking result. The 400k fixture still has a 9.26-second
synchronous slice and a 663 ms eight-edit incremental update. Further
main-thread micro-tuning is therefore rejected and the worker design below is
required before any worker implementation.

### Phase 6b attribution-worker design

#### Scope and shared replay core

Implementation follow-up: bd-arh0obkp. Use a dedicated attribution worker,
separate from the Phase 7 render-worker evaluation. Move Automerge document
replay, run-list mutation, and the
actor-to-author map off the main thread. Keep UTF-16-to-UTF-8 conversion,
identity resolution, and JSON serialization on the main thread: the 400k
payload measurement is only 0.77 ms. The wire payload remains exactly
`{ runs, identities }`, so the Rust renderer and DOCX/PPTX consumers do not
change.

Extract the existing replay operations into one internal replay-session core
used by both the worker and the main-thread fallback. It owns `_workDoc`,
`runs`, and `_authorByActor`; worker and fallback must not grow separate replay
implementations. The public hook/download behavior stays the same even though
incremental worker updates become asynchronous.

#### Request and response wire

Do not transfer `DocHandle` objects. `Repo.export()` would serialize the whole
document on the calling thread, and `Repo.import()` would not establish sync.
Send encoded Automerge changes instead:

```ts
type AttributionWorkerRequest =
  | {
      type: 'init';
      sessionId: number;
      requestId: number;
      textFieldName: string;
      changes: Uint8Array[];
      heads: string[];
    }
  | {
      type: 'append';
      sessionId: number;
      requestId: number;
      textFieldName: string;
      baseHeads: string[];
      heads: string[];
      changes: Uint8Array[];
    }
  | { type: 'cancel'; sessionId: number };

type AttributionWorkerResponse =
  | {
      type: 'runs';
      sessionId: number;
      requestId: number;
      heads: string[];
      runs: AttributionRun[];
    }
  | {
      type: 'error';
      sessionId: number;
      requestId: number;
      message: string;
    };
```

Start with structured-cloned `Uint8Array` values, which cannot detach buffers
owned by Automerge. Transfer only freshly owned copies if profiling later
shows clone cost matters. Worker `heads` and `baseHeads` are synchronization
metadata, not a reason to restore the deleted public bookkeeping fields.

#### Cold bootstrap and ongoing change feed

For a cold session, the main thread snapshots `doc`, calls
`getAllChanges(doc)`, and posts those changes with `getHeads(doc)`. The worker
replays from `init()`. This leaves one measured main-thread extraction cost:
103 ms for the 400k/1000-change fixture after this phase, rather than 9.33
seconds of replay. Do not claim worker cold starts are zero-cost. If that
residual becomes a measured gate, evaluate reading the persisted change log in
the worker as a separate design; do not move `Repo.export()` onto the main
thread as a shortcut.

The client retains `lastSentDoc`. On each debounced update it computes
`getChanges(lastSentDoc, handle.doc())`, posts the encoded delta and new heads,
then advances `lastSentDoc`. This keeps change extraction on the main thread
but moves all `applyChanges`, patch generation, and run-list work to the worker.
Messages for one session are ordered. The worker checks `baseHeads` against its
own heads before applying an append and queues an append that arrives while a
cold replay chunk is in progress.

Before any cold or incremental result is finalised, the client repeats the
catch-up delta until `getChanges(lastSentDoc, handle.doc())` is empty. This is
the worker equivalent of the 6a producer loop and prevents pairing stale runs
with newer text. The hook awaits the latest response while retaining the
previous payload and `generating: true`; identities-only changes can reuse the
worker runs and rebuild the payload without replay.

#### Staleness, cancellation, and lifecycle

Use one monotonically increasing session ID per enabled file and a request ID
per build/update. File switches, disable, and component cleanup abort the main
request, post `cancel`, and terminate a worker that is no longer shared. Every
response is checked against the current session and request IDs before state is
updated, so a late completion cannot revive an old file or disabled overlay.
The worker checks cancellation at replay-chunk boundaries and yields to its
own event loop often enough to receive cancel messages; these yields are not
main-thread scheduling costs.

#### Failure fallback

Keep the current in-main producer as the correctness fallback for unsupported
workers, worker startup failure, or a worker error. Log the failure, terminate
the failed session, and run the shared replay core on the main thread once.
Retain the previous valid payload until the fallback completes; never install
partial runs. A fallback failure follows current behavior: the overlay reports
the build failure and stops generating, while a download proceeds without
attribution rather than failing the file conversion.

#### DOCX/PPTX downloads

Route `attributionJsonFor` through the same worker-client registry keyed by
project and file. If the overlay already owns a warm session, a download sends
the latest delta and reuses it. With the overlay off, it creates an ephemeral
session, catches up to the handle's current heads, and terminates it after the
request. Both paths call the existing main-thread `buildAttributionPayload`
with the current identities, preserving the exact DOCX/PPTX transport and
best-effort behavior.

Moving pandoc conversion or the render pipeline is outside this design; Phase
7 evaluates that separately.

### Phase 6b browser verification

Real Chromium verification used the throwaway spec and config in
`attribution-perf/attribution-e2e/` (not committed). It seeded 501 real
Automerge edits, reloaded through the real sync/VFS path, enabled Authors,
typed `BURSTMARK!` into Monaco, checked the marker in Automerge and the preview,
inspected the attribution wrapper and hover badge, then toggled Authors off and
on. Invocation:

```bash
VITE_E2E=1 VITE_DEFAULT_SYNC_SERVER=/ws \
  VITE_Q2_SANDBOXED_PREVIEW_URL=q2-sandboxed-preview/index.html \
  npm run build -w hub-client
npm exec -w hub-client -- playwright test \
  --config="$PA_SCRATCH_DIR/attribution-perf/attribution-e2e/playwright.config.ts" \
  --project=chromium --workers=1 --retries=0
```

Observed output (the long `restoredText` value is abridged here):

```text
ATTRIBUTION_HISTORY 502
ATTRIBUTION_EDIT {"marker":"BURSTMARK!","actor":"e2e7e1f02a30000000000000000007e1","color":"#607D8B","badge":"Noble Wolf just now"}
ATTRIBUTION_TOGGLE {"offWrappers":0,"restoredActor":"e2e7e1f02a30000000000000000007e1","restoredText":"Start…BURSTMARK!"}
1 passed (7.6s)
```

Both `attribution-edit.png` and `attribution-toggle.png` were inspected. The
Monaco marker is present, the preview has the author-colored wrapper, and the
visible badge reads `Noble Wolf just now`. Off removes every attribution
wrapper; re-enabling restores the same actor and marker with no stale payload.
The before/after main-thread measurements are recorded in the tables above.

### Phase 6b verification

- The fail-first incremental-history regression failed with one call before the
  fix. After the fix, 31 focused attribution tests passed, including cold-build
  races, exact payload characterization, and both download formats.
- `npm run preflight -w hub-client` passed, including release WASM, the sandbox
  bundle, and TypeScript typechecking. Scoped ESLint passed for every changed
  TypeScript file.
- `npm run test:ci -w hub-client` passed: 1,757 unit, 240 integration, and 478
  WASM tests; preview/render DOM parity passed 109/109 opted-in fixtures.
- `npm run build:all -w hub-client` passed, including release WASM, the sandbox
  bundle, production TypeScript/Vite build, and service-worker precache check.
- `cargo xtask verify --skip-rust-tests --skip-hub-build` passed every enabled
  leg, including custom lints, clippy, formatting, all 1,702 strict
  `claude-notes` files, ts-packages, hub-client tests, and preview SPA.
- The full Rust workspace run used an official scratch pandoc 3.13 because the
  system binary is 3.12.1. With `--no-fail-fast`, it completed 19,203 tests:
  19,186 passed, 201 skipped, and 17 failed. Two are the same local knitr
  failures recorded for Phases 5 and 6a: `capture_classifies_pieces`
  (bd-54odm3rz, bd-qwscag8b) and
  `knitr_replay_equals_live::a_sql_chunk_with_hash_pipe_options_records_as_it_runs_live`
  (bd-b95dt1ia). The other 15 are Marimo tests whose engine stage timed out
  fetching `https://pypi.org/simple/marimo/`; they are unrelated network
  failures and no Rust source changed. Commit awaits user approval because the
  full workspace run is not green.
- No snapshot files changed.

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
