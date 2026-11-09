---
title: 'hub-client main-thread render path: audit fixes'
date: 2026-10-24
description: 'Phased fixes from the 2026-10-24 main-thread render-path audit: render coalescing, resolver-only format classification, single-parse AST transport, binary asset transfer, then measurement-gated worker work.'
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

Revised 2026-11-06: first Phase 2 review folded in (new export plus a
two-signal skip).

Revised 2026-11-09: Phase 2 rewritten for implementation after a second
probe against the real WASM corrected the first review — `success` is
*not* front-matter-determined (inline parse errors fail it), the
router's guard is live only for unknown `q2-*` keys and causes a bug
under a runtime `format`, and the new-export design is replaced by a
one-line resolver fix plus deletions. The skip is dropped: the call it
would save costs well under a millisecond.

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
render itself reads it. Measured in Phase 0 (sub-millisecond each; the
Phase 2 decisions record why the third crossing is not worth removing)
and folded into Phase 5 rather than tracked separately.

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
live-browser verification was initially owed and landed 2026-10-30 —
see "Live-browser smoke" under the review follow-ups below.

Review follow-ups (2026-10-30, PR review of bd-r8iqmg2f; three commits
on the same branch):

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

Live-browser smoke (2026-10-30; closes the verification debt recorded
above). A throwaway Playwright spec (deleted after the run, same
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

The router pays a full parse per keystroke and reads only `success`
(`PreviewRouter.tsx:147`). That parse is `parse_qmd_to_ast`: the whole
tree-sitter body parse, the engine pass-through, `MetadataMergeStage`
and the AST writer — the discard is larger than "parse + serialize"
says. Nothing merged since the audit touches this path.

**Status: done 2026-11-09 (bd-luoo7qgu).** The first
review (2026-11-06) designed a new export, `check_document_for_preview`,
returning parseability plus the classification in one envelope, with a
two-signal skip in front of it. A second probe against the real WASM
(below) shows none of that is needed: the router can drop the parse
*and* its guard outright once a one-line inconsistency in the Rust
resolver is fixed. The design is now one Rust line, deletions in the
router, and no skip. Results below; the per-keystroke baseline's other
components (render, tokens, VFS) are unchanged — the phase removes the
discarded parse and its envelope, nothing else.

### What the probe showed (2026-11-09, local WASM artifact of 2026-11-03)

`parse_qmd_to_ast` + `resolve_pandoc_formats` on each input, then
`classifyPreviewMode` as the router runs it. The artifact predates the
dashboard commit (c5a29dcd0), so `format: dashboard` is omitted; nothing
else relevant to this phase changed between the artifact and `main`
(the only other touch, a0080ff54, is the single-file `_metadata.yml`
walk).

| Input | parse `success` | resolver `class` / `pseudoFormat` | router mode today |
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

Three corrections to the 2026-11-06 premise follow from the table:

1. **`success` is not front-matter-determined.** Unbalanced inline
   spans and malformed links hard-fail the parse (pampa raises a parse
   error / Q-2-31). A parse-free export cannot reproduce `success`, and
   the `success`-parity probes the first review specified would fail.
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
   document whose own key is unknown never reclassifies today (last
   row). Deleting the guard fixes it; the resolver is the one that is
   right.

### Decisions

- **No new export.** `resolve_pandoc_formats` already returns everything
  the router needs, synchronously, in a small envelope.
- **No skip logic.** The two-signal skip of the first review
  (YAML-region compare + VFS-write counter, per-path keying) would save
  the resolver call: 0.26 ms here, 0.48 / 1.5 ms at 1x / 8x in the
  baseline. The "third full-text crossing" of the amplifier is one
  buffer copy into WASM memory, sub-millisecond at 8x. Against that, a
  TS front-matter extractor would have to match the Rust YAML-block
  boundary exactly (the unterminated case shows how subtle that is), and
  the counter touches three VFS wrappers. Not worth the code or the
  invalidation risk; the amplifier stays with Phase 5.
- **Accepted behavior change.** While a `q2-*` format name is being
  typed, the pane shows the `neither` view ("nothing can show `q2-s`")
  for the partial key instead of holding the previous preview, then
  mounts the deck when the key completes. That is exactly what typing
  `revealjs` or `docx` does letter by letter today, and the preview
  already shows a render error for the partial key (the render resolves
  the format itself). Rejected alternative, for the record: a
  `formatKnown` boolean on the resolver response computed from the same
  gate, with the guard kept — same cost, but it keeps a special case for
  one key family.
- **Out of scope, pre-existing**: `format: q2-foo-html` parses as
  extension `q2-foo` of `html`, gets a `pseudoFormat`, and mounts the
  React preview on a non-preview pipeline. Unchanged by this phase; file
  a strand if it ever bites.

### Checklist

- [x] File the strand (`braid create "Phase 2: resolver-only format
  classification" -t task -p 2 -d "<this plan>"`), mark it
  `in_progress`, branch `braid/<id>-resolver-only-format-check` off
  `main`. → bd-luoo7qgu, branch `braid/bd-luoo7qgu-resolver-only-format-check`.
- [x] **Test first (TS, wasm suite).** In
  `previewModeClassification.wasm.test.ts`, under the editor-edge
  `describe`, add two pins that must **fail** on the current artifact:
  `format: q2-s` and `format: q2-bogus` → `{ mode: 'neither', formatKey:
  'q2-s' }` / `'q2-bogus'` (today: `react` with the partial key). Add one
  regression pin that passes already and documents correction 3:
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
