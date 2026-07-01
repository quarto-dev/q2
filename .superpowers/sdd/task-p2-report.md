# Task P2 report — Bug B (bd-h4rhohhy): "Bug B" refuted; echo fixture fixed; PC5/PC-B/PC7 green, PC6 deferred

**Status: DONE_WITH_CONCERNS.** The evidenced defect was fixed (echo fixture emits realistic
`::: {.cell}` wrappers), all in-scope tiers are green with fail-on-revert proofs, and "Bug B" as a
distinct browser/splice defect is REFUTED. Concerns: PC6 (julia browser leg) is deferred opt-in
because a temp HOME does not isolate the julia transport; and I leaked a few orphaned julia servers
I could not reap (safety classifier blocked the kill). Details below. No push.

## Diagnosis (evidence before fix)

The q2-preview splice path is `apply_capture_splice` / `derive_cell_outputs` / `is_cell_wrapper`
(`crates/quarto-core/src/engine/capture_splice.rs`) — NOT `ReplayEngine`, and there is NO staleness
check. The splice maps each engine cell to the next `::: {.cell}` wrapper in the executed markdown.
**The brief's PRIMARY candidate (canonical `input_qmd` staleness rejection) is RULED OUT.**

Root cause of the echo PC5 failure (native, deterministic): the echo fixture emitted a bare
`**ECHO_EXECUTED**` paragraph — no `.cell` wrapper — so `derive_cell_outputs` built an empty map and
the cell survived as raw source. Julia (decisive native leg, below) wraps its output in
`::: {#cell-1 .cell execution_count=1}` and splices cleanly. So the splice is CORRECT for real
engines; echo was an unrealistic fixture. **"Bug B" as a distinct browser/splice defect is
REFUTED** — the user's live julia symptom re-attributes to Bug A (close/busy discards the capture)
and/or Bug C (wire corruption + host-kill), both owned by P1/P1c.

### Julia native leg (decisive) — recorded transcript

Isolated fresh server (temp HOME + `IsolatedJuliaServerGuard`, real depot/project/bindir), doc
`engine: julia / execute: {daemon: false}` with `1 + 1`. `record_capture` →

```
result.markdown:
::: {#cell-1 .cell execution_count=1}
``` {.julia .cell-code}
1 + 1
```
::: {.cell-output .cell-output-display execution_count=1}
```
2
```
:::
:::

apply_capture_splice(A2=parse(input_qmd), A1, B1, "julia"):  cell_survived=false   ← splice fired
```

## Fix (ratified: fix the FIXTURE, not the splice — splice generalization REJECTED)

`crates/quarto-core/tests/fixtures/extensions/echo-engine/src/echo-engine.ts` — `execute()` now wraps
the executed output in `::: {.cell}` / `.cell-output` (the shape real engines emit via the
engine-host's `mdFromCodeCell`). Rebundled the committed `dist/echo-engine.js` via
`cargo run --bin q2 -- build-ts-extension …`.

**Blast-radius survey (every echo-fixture consumer):** all assertions are substring checks
(`ECHO_EXECUTED`, `not run by echo`, `{python}`) that survive wrapping. **Zero assertion edits
required.** `echo_engine_e2e.rs` 9/9 pass; full `cargo nextest run -p quarto-core -p quarto-preview`
= 2720 passed / 35 skipped / 0 failed.

## PC-B native seam (registered, GREEN) — `capture_splice_seam.rs`

| ID | Tier | Real unit | Seam → assertion | Mock boundary | Revert hunk → RED |
|----|------|-----------|------------------|---------------|-------------------|
| PC-B | int-rs (+ deno leg) | `apply_capture_splice` / `is_cell_wrapper` | (1) `.cell`-wrapped capture → source cell REPLACED + output present; (2) bare-paragraph capture → documented NO-OP (cell survives); (3, deno) REAL echo capture → cell replaced + `ECHO_EXECUTED` present | none (real splice; real `record_capture` for leg 3) | (a) `is_cell_wrapper` stops recognizing `.cell` → (1)+(3) RED; (b) revert the echo FIXTURE wrapper → empty map → cell survives → (3) RED (this is the controller-rebound hunk) |

TDD RED→GREEN (leg 3): pre-fix RED — `result.markdown` = bare `**ECHO_EXECUTED**`, `cell_survived=true`,
panic "the real echo capture must splice … result.markdown:\n**ECHO_EXECUTED**". Post-fix GREEN — 3/3 pass.

## PC5 e2e (chromium) — amended assertion, GREEN, fail-on-revert proven

Controller-amended (option 1): dropped the inert-source-first sub-assertion (unsatisfiable — the
eager capture is recorded at server startup before the browser connects, so the first render already
splices; renderTicks=1, no inert frame). Binding assertions now: (a) `ECHO_EXECUTED` appears in the
pane without reload; (b) the raw source token `PC5_ECHO_SOURCE_TOKEN` is ABSENT (splice REPLACED the
cell). The spec header documents why inert-first is unsatisfiable and why (a) is non-vacuous. Only
`test.fail()` and the inert-first block were changed.

**Fail-on-revert proof (mandatory, per decision #2), revert target = `capture_driver.rs` `set_capture`:**
```
GREEN (baseline):                PC5 passes (2.7s)
RED  (set_capture neutralized):  PC5 fails — 30.1s timeout waiting for ECHO_EXECUTED
GREEN (restored + rebuilt):      PC5 passes (2.7s)
```
`cargo build --bin q2` between each (native-only; no WASM chain — server-side change). `capture_driver.rs`
restored clean (empty `git diff`).

## PC7 (jsdom, `PreviewApp.integration.test.tsx`) — GREEN, fail-on-revert proven

New test: after the initial capture-less render, firing `onCapturesChange` with a `CaptureRef` must
re-fire the render effect (a SECOND `renderPageForPreview` call) and forward the binary-doc bytes.

**Fail-on-revert (two reverts — a finding):**
```
Revert A — remove the contentTick bump (PreviewApp.tsx ~:737):  GREEN (still passes)
Revert B — remove the `captures` write (~:733):                RED ("getBinaryDocById expected to be called with ['pc7-capture-doc']")
Restore:                                                        GREEN
```
**Finding:** the `contentTick` bump inside `onCapturesChange` is REDUNDANT with the render effect's
`state.captures` dependency (`PreviewApp.tsx:1128`) — a new `captures` reference already re-fires the
effect. So the controller's intended PC7 revert target (the contentTick bump) does NOT bind; the
load-bearing hunk is the `captures` write. PC7 binds THAT (revert B → RED), and the test + its
comment document the redundancy. `PreviewApp.tsx` restored clean.

## PC6 (julia browser leg) — PASSES, but DEFERRED opt-in (concern)

New spec `engine-capture-splice-julia.spec.ts`. It PASSES — a green run is on record: the julia
`{1+1}` cell's `.cell`-wrapped `2` splices into the pane without reload, 6.5s. It is gated behind
`QUARTO_PC6_LIVE=1` (skips in the default suite) for one reason:

**The julia transport file is NOT isolated by a temp HOME.** Empirically every julia server (mine
and the environment's) uses the transport under `QUARTO_JULIA_PROJECT` (the shared instantiated
project), not under `$HOME/Library/Caches`. So a temp HOME does not yield a fresh isolated server —
the render reuses the developer's shared julia server/transport. That both violates the isolation
rule and exposes the run to Bug A (stale busy worker) in CI. Until Bug A is fixed (P1) or the
transport is truly isolated (an isolated COPY of the instantiated project as `QUARTO_JULIA_PROJECT`),
PC6 stays opt-in. The unconditional julia proof is the native leg above. This matches the
controller's "defer if it flakes on A/C; note green run pending P1" latitude.

Added an additive `extraEnv?` option to `e2e/helpers/previewServer.ts` (no existing caller affected)
so the spec can inject the julia env.

**Server-leak concern:** because the isolation was ineffective, my julia runs (native leg + the
nextest julia_engine_e2e tests that ran during verification + PC6) left ~4 orphaned QNR servers
(their temp project dirs are deleted; they serve nothing). I attempted to reap only the ones I
started (identified by orphaned temp-project path + my-session start times), but the safety
classifier blocked the `kill`. The user's real server (pid 9828, project `/Users/gordon/docs/julia`)
was correctly identified and never targeted. **These orphans should be reaped** — e.g.
`pgrep -f quartonotebookrunner` then kill the ones whose `.jl` path under `/T/.tmp…` no longer
exists (NOT pid 9828). I did not force this past the safety guard.

## Rebuilds performed before each e2e evidence run

fixture `.ts` → `dist/echo-engine.js` (`build-ts-extension`) → `cargo build --bin q2`. No WASM/SPA
rebuild: no Rust/WASM product code changed (splice unchanged); the fixture loads server-side at
runtime (copied fresh into the temp project by each spec); the embedded SPA/WASM (P0's build) is
current. The set_capture revert used `cargo build --bin q2` only (native server-side change).

## Verification counts (each run once, to a log)

- `cargo nextest run -p quarto-core -p quarto-preview`: **2720 passed, 35 skipped, 0 failed.**
- PC-B `capture_splice_seam`: 3 passed. `echo_engine_e2e`: 9 passed.
- q2-preview-spa vitest: unit **25 passed**; integration **76 passed** (incl PC7).
- q2-preview-spa `npm run test:e2e`: **37 passed, 1 skipped (PC6 opt-in), 1 failed** — the failure is
  the PRE-EXISTING `firefox-ws-queue` under the **firefox** project (`browserType.launch: Executable
  doesn't exist … firefox-1522/Nightly.app` — Firefox not installed); the same spec passes under
  chromium. Orthogonal to this task.
- PC5 fail-on-revert: GREEN→RED(30.1s)→GREEN. PC7 fail-on-revert: A GREEN (redundant), B RED, restore GREEN.

## Files changed / added (path-scoped)

- `crates/quarto-core/tests/fixtures/extensions/echo-engine/src/echo-engine.ts` (fixture emits `.cell`)
- `crates/quarto-core/tests/fixtures/extensions/echo-engine/dist/echo-engine.js` (rebundled)
- `crates/quarto-core/tests/integration/capture_splice_seam.rs` (new PC-B seam) + `main.rs` (register)
- `q2-preview-spa/e2e/engine-capture-splice.spec.ts` (PC5 amended)
- `q2-preview-spa/e2e/engine-capture-splice-julia.spec.ts` (new PC6, opt-in)
- `q2-preview-spa/e2e/helpers/previewServer.ts` (additive `extraEnv`)
- `q2-preview-spa/src/PreviewApp.integration.test.tsx` (PC7)
- `capture_driver.rs` and `PreviewApp.tsx` touched only for fail-on-revert proofs; restored clean.
- throwaway `pcb_diag.rs` deleted.

## Review response (2026-07-02) — commit 2

Two review items addressed (comment-only, no behavior change):

- **Important #1 (stale PC5 header):** rewrote `engine-capture-splice.spec.ts:1-29`. It no longer
  describes a "WASM render_page_for_preview ReplayEngine splice" (refuted); the chain now ends at
  the q2-preview pipeline's CaptureSplice stage (`capture_splice.rs`), explicitly notes there is no
  ReplayEngine and no staleness check, describes the `.cell`-wrapper the echo fixture now emits, and
  replaces the P0-era "written pre-fix / ratifies before un-skipped" status with the P2 reality
  (fixture fixed, amended assertion ratified, set_capture fail-on-revert proven).

- **Minor #2 (PC-B comment):** chose the honest option — actually ran the `is_cell_wrapper` revert
  and recorded it, so the comment's claim is now transcript-validated (no softening needed):
  ```
  is_cell_wrapper neutralized (return false):
    bare_paragraph_capture_is_a_documented_noop  PASS  (cell survives, expected)
    cell_wrapped_capture_splices                 FAIL  (leg 1 — cell survives, no .cell matched)
    real_echo_capture_splices                    FAIL  (leg 3)
  restored: 3 passed
  ```
  Both PC-B revert legs are now proven: (a) `is_cell_wrapper` matching → legs (1)+(3) RED;
  (b) the echo fixture wrapper emission → leg (3) RED (recorded at fix time). `capture_splice.rs`
  restored clean.

PC5 re-run after the header edit: GREEN (1.2s).
