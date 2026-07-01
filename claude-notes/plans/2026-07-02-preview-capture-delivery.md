# Preview engine-capture delivery: julia close/busy failure + browser splice e2e

**Status:** plan — created 2026-07-02 from a live user repro. Seams prevalidated same day.
**Strand:** bd-h4rhohhy (P1) carries the delivery-bug evidence; the close/busy defect is item
**Bug A** below (tracked in this plan; noted on the strand).
**Scope:** OUT of Plan 4c's scope (marimo continues separately). This is a focused
debug-and-fix of `q2 preview`'s engine-capture path plus the browser-tier e2e coverage it
never had (Plan-4 4J's honest limitation).
**Repos:** q2 worktree `feature/ts-engine-extensions` + upstream `~/src/quarto-julia-engine`
(engine-side fix on a NEW local branch `q2-close-busy-fix` off main; never push; mirrors the
marimo `q2-bare-sql-interop` precedent).

## The two bugs (evidence gathered 2026-07-02, controller session ts-engines-impl-10)

**Bug A — oneShot close/busy kills successful/fresh captures (`Q-PREVIEW-CAP-1`).**
User repro: `daemon: false` julia doc under `q2 preview` → `Engine capture failed …
Julia server returned error after receiving "close" command … worker is busy`.
Root-cause chain (verified on the live machine):
1. A prior session's detached daemon julia server (bd-m1jeqhhz) holds the shared transport
   file (`~/Library/Caches/quarto/julia/julia_transport.txt`).
2. `startOrReuseJuliaServer` (julia-engine.ts:330-448) reuses ANY existing transport file —
   the check never consults `oneShot`; `daemon: false` governs per-file worker close only.
3. The server log shows an earlier run's client vanished mid-response (EPIPE at QNR
   socket.jl:455) leaving the file's worker BUSY.
4. The new render's **pre-run** close (`executeJulia` julia-engine.ts:703-718 sends
   `isopen`→`close`) hits "worker is busy"; there is NO busy handling anywhere in the engine
   (full-file grep: no wait/retry); the error propagates → whole capture discarded.
Also latent: the **post-run** close (julia-engine.ts:742-749) can fail the same way — which
would discard a capture whose run SUCCEEDED (cleanup failure ≠ execution failure).

**Bug B — recorded captures never reach the browser pane (bd-h4rhohhy).**
With the daemon-mode session, the eager capture WAS recorded (verified: 84KB gzip
EngineCapture in the server data_dir, `result.markdown` contains the base64 figure), but the
pane never updates. Delivery chain (all verified file:line):
server `record_one` → `write_capture_doc` + `ctx.index().set_capture`
(capture_driver.rs:184-205, info log "recorded engine capture(s)") → samod sync →
sync-client capture-sidecar diff → `onCapturesChange` (quarto-sync-client client.ts:359-364,
781, 1353) → PreviewApp.tsx:729-738 (`setState` captures + `contentTick+1`) → render effect
:1005-1030 (`getBinaryDocById` → `renderPageForPreview(path, undefined, captureGzJson)`) →
WASM `render_page_for_preview` ReplayEngine splice (gunzip happens WASM-side).
The break is somewhere in that chain — root cause UNKNOWN; P0 reproduces it deterministically
before any fix. Candidate suspects (check in order): sidecar write vs sync delivery
(server info-log + client-side log), the render effect's `state.activeFile` key vs the
sidecar's rel_path key, WASM replay rejecting on canonical `input_qmd` mismatch (staleness),
`contentTick` effect not re-firing.

## Bug C candidate — wire-frame corruption on the engine-host stdout (evidence 2026-07-02 ~17:11-17:14, user's live preview; RECORDED, NOT DIAGNOSED)

Two `ERROR quarto_core::engine::ts_process: engine-host protocol error: non-JSON line on
stdout` events from the same session:
1. An ANSI-colored **julia server log line** (`[ Info: Log started at 2026-07-02T13:11:20.379`)
   arrived on the ENGINE-HOST's stdout — i.e. a child julia process's output leaked into the
   Deno host's wire channel. Suspect: julia-engine.ts spawning the julia server (or a worker)
   with inherited stdout in some path.
2. A **legitimate `{"id":3,"msg":{"type":"executeResult",...}}` frame** — with the full
   document markdown and pages of base64 — was REJECTED as a non-JSON line. Suspects:
   (a) reader-side line splitting on very long frames (buffer cap / partial read treated as a
   full line in ts_process's stdout reader); (b) child-output bytes interleaved INTO the JSON
   line mid-frame (same leak as #1 corrupting an otherwise-valid frame). Symptom-consistent
   with the user "sometimes seeing no result": when the executeResult frame is dropped, the
   capture silently never completes.
This may be the true root of Bug B or an independent third defect. P0 must check the
ts_process reader's framing under large frames (multi-MB base64) + child-stdout inheritance
BEFORE assuming the delivery chain's samod/SPA links are guilty. A targeted unit seam
(mock transport feeding a >1MB single-line frame; and a frame with interleaved noise) can
bind whichever defect is confirmed — add it to the seam table at diagnosis time (additive,
controller sign-off).

## Facts the implementer must not rediscover

- **Playwright infra EXISTS for exactly this**: `q2-preview-spa/e2e/` (14 specs) spawns the
  real `q2 preview --no-browser --data-dir <tmp>` binary via
  `e2e/helpers/previewServer.ts:203-296` (`Q2_BINARY = target/debug/q2`; globalSetup asserts
  it exists). Run: `npm run test:e2e` in q2-preview-spa/ (xtask verify --e2e step 14,
  verify.rs:533-564). Chromium install is the developer's responsibility.
- Error paths have ZERO coverage today: no test references Q-PREVIEW-CAP-1
  (capture_driver.rs:128-131, soft-fail + continue) or Q-PREVIEW-RE-1 (re_execute.rs:272-279,
  + sidecar `CaptureState::Error`); no forced-failing-engine test exists.
- `capture_binary_doc_round_trips_through_samod` (capture_driver.rs:885-923) is the happy-path
  sidecar test to pattern-match.
- Eager captures and `/api/preview/re-execute` bottom out in the SAME
  `preview_record::record_capture`; re-execute adds Running/Error sidecar states + IN_FLIGHT.
- hub-client does NOT consume captures at all — everything browser-side is
  q2-preview-spa/src/PreviewApp.tsx atop ts-packages/preview-runtime + quarto-sync-client.
- Rebuild rules: engine-host TS → `cargo xtask build-engine-host-bundle` + commit dist;
  julia fixture engine → rebundle `julia-engine.js` (layout workaround: compat log §4/§8);
  SPA (PreviewApp/preview-runtime/sync-client) → `cargo xtask build-q2-preview-spa` +
  `cargo build --bin q2` to re-embed (CLAUDE.md preview-staleness section); WASM
  (quarto-core replay path) → `cd hub-client && npm run build:wasm` first.

## Work items

### P0 — reproduce both bugs deterministically (no fixes yet)
- [x] **PC4a harness (Bug A repro)**: julia+deno-gated integration test (julia_engine_e2e.rs
      or a preview-crate test): doc with a sleeping `{julia}` cell; start `record_capture`,
      abort/drop its client mid-run (simulating the EPIPE abandonment), then run a fresh
      `record_capture` of the same doc through the same shared server → observe the
      close/busy failure. EXPECTED RED-shaped result pre-fix (this IS the reproduction the
      user asked for). Record verbatim. Its post-fix assertion is frozen at fix time (see P1).
- [x] **PC5 harness (Bug B repro)**: the playwright spec below, written pre-fix; if Bug B
      reproduces, PC5 fails pre-fix → that failure output IS the diagnosis entry point
      (grab server log + browser console + sync-client state from the failure).
- [x] Write both failure transcripts into the strand (bd-h4rhohhy) + this plan.

### P0 results (2026-07-02, commit 2931d7692 — full transcripts in .superpowers/sdd/task-p0-report.md and strand comment c-eps49gsq)

All three reproduced deterministically:
- **Bug A**: verbatim user failure via two concurrent oneShot renders of a `sleep(25)` doc
  sharing one julia server under an isolated HOME — render B: `Julia server returned error
  after receiving "close" command: … Tried to close file … but the corresponding worker is
  busy.` Harness `pc4a_shared_server_busy_close` (julia_engine_e2e.rs, `#[ignore]` +
  `QUARTO_PC4A_LIVE=1`).
- **Bug B**: reproduced in **chromium** (not Firefox-specific) via
  `q2-preview-spa/e2e/engine-capture-splice.spec.ts` (`test.fail()` while unfixed): capture
  recorded + sidecar written server-side; SPA syncs ("Peer connected - online mode"); pane
  shows only the inert source; NO console error — a silent browser-side break. **Bug B is
  NOT Bug C** (echo engine: no julia child, no ts_process error, capture recorded).
  **Fix-wave re-ranking (ae4191d8f, sync-client state harvested)**: the browser-side chain
  WORKS end to end — `onCapturesChange` fired (keys `["index.qmd"]`), activeFile key
  matches, `getBinaryDocById` returned the capture bytes (567 B), render effect re-fired
  (renderTicks=1) — yet the pane stayed inert. The break is INSIDE WASM
  `render_page_for_preview`'s ReplayEngine splice; PRIMARY candidate: the canonical
  `input_qmd` staleness rejection (the "accepted-untested" item — P0 now implicates it;
  P2 adds its seam on confirmation). RULED OUT: sidecar-not-delivered, key mismatch,
  getBinaryDocById failure, contentTick not re-firing.
- **Bug C**: large-frame suspect RULED OUT (`ts_process_framing_probe.rs`: >1 MB single-line
  frame parses `Ok`; no reader size cap). Both live symptoms share one root: a **foreign
  writer on the engine-host's stdout fd** (candidate: `start_quartonotebookrunner_detached.jl`
  runs `run(detach(cmd), wait=false)` with no stdio redirection → QNR banner inherits stdout).
  **Escalation defect**: one `Malformed` line makes `reader_loop` (ts_process.rs:930-954)
  broadcast an error to EVERY pending slot and kill the whole Deno host — one stray banner
  discards every in-flight capture.

### Controller ratification (2026-07-02, pre-P1/P2)

- **PC1 shape ratified as proposed**: post-run close failure after a successful run is
  non-fatal (warn + return run result).
- **PC2/PC4 decision rule ratified** (amended after P0 review): the rule applies to the
  **abandoned-worker** scenario (client vanished mid-run, worker stuck busy — the user's
  bug). P1 first confirms whether the QNR socket `close` command accepts a force flag
  (julia-engine.ts CLI already calls `closeWorker(file, force)` ~:1002-1003). If YES →
  recovery: pre-run close falls back to forced close on busy; frozen PC4 assertion = the
  fresh `record_capture` SUCCEEDS. If NO → actionable error naming the stale-server/
  transport-file remedy; frozen PC4 assertion = error contains the frozen remedy substring.
  Either way, never the bare "worker is busy" protocol error. **Scenario caveat (review
  Important #1)**: a worker busy serving a LIVE concurrent render is a different case where
  force-close would kill legitimate work — that is the plan's documented-not-gold-plated
  oneShot-reuse design question, to be documented for the upstream PR, not silently folded
  into the busy-recovery path.
  **COUNTERSIGNED (controller, 2026-07-02, post-P1)**: decision gate answered YES with
  file:line evidence (ServerCommand `forceclose` union member, `closeWorker(file, force)`,
  live smoke test green) → recovery branch selected; frozen PC4 assertion = fresh
  `record_capture` SUCCEEDS and its `result.markdown` contains `cell-output`
  (non-vacuous execution proof). RED→GREEN proven against pre-fix vs fixed bundles.
- **Bug C engine-side root fix ratified** (P1, upstream branch): the detached QNR launcher
  must redirect child stdout/stderr (log file or devnull), never inherit the engine-host fd.
- **Bug C reader-side resilience ratified WITH constraints** (quarto-core, new task P1c):
  bounded log-and-skip for stray non-protocol lines instead of Malformed→kill-all; MUST be
  bounded (cap on consecutive stray lines, beyond which the existing kill-channel behavior
  is preserved); MUST NOT introduce indefinite hangs for an in-flight request whose frame
  was consumed/corrupted (implementer investigates the existing timeout story and documents);
  the deliberate-contract comment at reader_loop:930-935 must be updated to describe the
  new policy. Seam row PC-C added below.
- **Bug B**: P2 proceeds diagnosis-first (SPA instrumentation per rebuild rules), then a
  minimal fix on the broken link; frozen post-fix assertion = PC5 as written (remove
  `test.fail()`), plus PC6 (julia leg) and PC7 (jsdom).

### P1 — Bug A fix (engine-side, upstream branch `q2-close-busy-fix`)
Fix-shape principles (final shape needs controller ratification before freezing PC4's
assertion — J3-correction precedent):
- A **post-run** close failure after a successful run must be NON-FATAL (warn + return the
  run result). Seam **PC1**.
- A **pre-run** close-busy must not produce a bare protocol error: either recover (QNR may
  offer a forceful close — investigate its API) or fail with an ACTIONABLE message naming the
  stale-server/transport-file remedy. Seam **PC2**.
- Consider (document, don't gold-plate): should `oneShot` renders refuse to reuse a
  daemon-started server at all? Upstream-behavior question — document for the upstream PR,
  implement only if the fix requires it.
- [x] PC1 + PC2 TDD upstream (deno tests, socket/command-writer mocked); rebundle the q2
      julia fixture from the branch; compat log + migration guide addenda (julia-engine.ts is
      no longer zero-changes — UPDATE THE HEADLINE claims in both docs honestly).
      *(Upstream `q2-close-busy-fix` @ 93bce7b. Decision gate = YES: QNR exposes `forceclose`;
      PC2 = force-close recovery. New pure `src/worker-close.ts` (preRunClose/postRunClose over
      an injectable writer); 6 deno unit tests RED→GREEN. Fixture rebundled `82bff64…`. Compat
      log §15 + migration-guide headline updated. See .superpowers/sdd/task-p1-report.md.)*
- [x] Bug C engine-side root fix on the same upstream branch: detached QNR launcher stdio
      redirection (no fd inheritance) — see ratification above.
      *(`start_quartonotebookrunner_detached.jl` → `run(pipeline(detach(cmd), stdout=devnull,
      stderr=devnull), wait=false)`. Not deno-mock-testable (Julia subprocess/OS-fd property);
      covered by PC-C framing probes (GREEN at P0) + PC4a live GREEN end-to-end.)*
- [x] Freeze PC4 post-fix assertion (controller sign-off) and prove RED→GREEN.
      *(Frozen: fresh `record_capture` SUCCEEDS with a real capture (YES branch). RED vs pre-fix
      bundle `d9d5120…` → "worker is busy" Err; GREEN vs `82bff64…` → recovers. Controller
      countersign pending from this report's decision-gate evidence.)*

### P1c — Bug C reader-side resilience (quarto-core, ratified with constraints above)
- [x] **PC-C resilience leg** TDD: bounded log-and-skip in `reader_loop` for stray
      non-protocol lines; kill-channel preserved beyond the bound; no indefinite hangs;
      update the :930-935 contract comment. *(Commit a373e6902; MAX_CONSECUTIVE_MALFORMED_LINES=5,
      reset on any valid frame; beyond-bound escalation structurally unchanged; real-reader
      TDD RED→GREEN incl. unrelated-slot survival; framing probes untouched GREEN; review
      Approved. Documented residual: explicit `timeout: false` + own-frame-corrupted can
      hang — see task-p1c-report.md no-hang story.)*

### P2 — Bug B diagnosis + fix (location unknown until P0)
- [x] Diagnose from PC5's failure; fix minimally on whichever side breaks. *(Outcome: NOT a
      product defect — "Bug B" refuted. The splice is correct for real engines (julia
      confirmed natively); the echo FIXTURE emitted no `::: {.cell}` wrapper. Fix =
      fixture-side (echo-engine.ts emits real-engine `.cell` shape, dist rebundled), ratified;
      splice generalization rejected. User's live symptom re-attributed to Bug A/C. New PC-B
      seam binds the splice contract both ways, both revert legs transcript-validated.)*
- [x] **PC7** jsdom-tier binding of the SPA handler (existing
      PreviewApp.integration.test.tsx mock pattern at :549). *(GREEN + fail-on-revert; binds
      the captures state write — see seam-table note on contentTick redundancy.)*
- [x] PC5 GREEN (echo leg, amended assertion, set_capture fail-on-revert proven) + **PC6**
      GREEN (julia leg — green run recorded, opt-in `QUARTO_PC6_LIVE=1` pending P3
      un-deferral).
      *(Commits 75cf526fc + 652ae9ae4; report .superpowers/sdd/task-p2-report.md; review
      Approved, task-p2-review.md.)*

### P3 — error-path coverage + regression
- [x] **PC3**: forced-failing engine → Q-PREVIEW-CAP-1 emitted AND the eager loop still
      records the NEXT doc's capture. *(capture_driver.rs `pc3_failing_engine_does_not_block_next_doc_capture`:
      registry with a `test-failing` FailingTestEngine (doc A, `a-` prefix) + real
      `test-passthrough` PassthroughTestEngine (doc B, `b-` prefix — `qmd_files` is
      sorted so A always runs first); GREEN against current code (both behaviors
      already existed, undertested). Fail-on-revert proven both ways: (a)
      neutralizing `sink.emit(...)` reddens the diagnostic assertion; (b)
      short-circuiting the loop to `return Err(e)` on first failure reddens doc
      B's capture assertion. No product change.)*
- [x] **PC8**: forced re-execute failure → sidecar `CaptureState::Error` + Q-PREVIEW-RE-1.
      *(re_execute.rs `pc8_re_execute_failure_sets_error_state_and_emits_diagnostic`:
      a re-execute-time-only `FailingReExecuteEngine` sharing the seeded doc's engine
      name, registry override applied only at re-execute — a separate cache dir is
      required since `record_capture_cached` keys purely on content hash. GREEN
      against current code. Fail-on-revert proven: neutralizing the
      `CaptureState::Error`/`last_error` write hunk sticks the sidecar at `Running`
      forever, reddening the test. No product change.)*
- [x] **PC6 + PC4a shared-transport isolation**: isolate `QUARTO_JULIA_PROJECT` (temp copy)
      in BOTH live-julia harnesses (PC6 playwright leg; PC4a whose temp-HOME isolation was
      proven imperfect at P1 — it spawned QNR servers on the shared transport, see
      task-p1-report.md §7 and compat log §15), then drop the `QUARTO_PC6_LIVE` gate
      (or record explicitly why it stays opt-in). Relates to bd-l9jhy5u0.
      *(`isolate_julia_project()` (julia_engine_e2e.rs) / `isolateJuliaProject()`
      (engine-capture-splice-julia.spec.ts) copy the ambient `QUARTO_JULIA_PROJECT`'s
      `Project.toml`+`Manifest.toml` into a per-test temp dir and re-point the env
      var at the copy — on top of, not instead of, the existing temp-`HOME`
      override (which governs the transport file; confirmed by a standalone
      `quarto_util::quarto_runtime_dir()` + `Deno.env.get("HOME")` check, both
      honor an overridden `HOME`). Live-verified TWICE: `pc4a_abandoned_worker_close_busy`
      (78.8s, `QUARTO_PC4A_LIVE=1 ... --run-ignored all`) and PC6
      (`QUARTO_PC6_LIVE=1 npx playwright test engine-capture-splice-julia`, 8.6s) —
      both embed a `SharedTransportSentinel`/`captureSharedTransportMtime()`
      assertion that the shared `~/Library/Caches/quarto/julia/julia_transport.txt`
      existence+mtime is unchanged across the run; both PASSED, independently
      confirmed via `stat`/`ls` before and after (Project.toml/Manifest.toml
      mtimes byte-identical; `julia_transport.txt` absent both times); the
      `IsolatedJuliaServerGuard` reaped every process either run spawned (no new
      pids after either run). Found, but did NOT touch: ~28 pre-existing
      julia/QuartoNotebookRunner processes on the shared transport from
      unrelated (non-isolated, by-design daemon-reuse) test activity spanning the
      day — this is the pre-existing bd-l9jhy5u0 leak, out of scope here, left
      alone per the "never touch processes you didn't start" constraint.
      `QUARTO_PC6_LIVE` gate KEPT opt-in — isolation is no longer the reason
      (proven safe above); the remaining reason is environmental/speed, mirroring
      PC4a's `#[ignore]` gate (real julia+deno dependency, network-instantiated
      project, multi-second server boot) — documented in the spec's file header.)*
- [x] julia/echo suites + `cargo nextest run -p quarto-preview -p quarto-core` green;
      q2-preview-spa `npm run test:e2e` green (all 14 existing specs + new ones);
      full `cargo xtask verify` at the end (WASM leg matters if quarto-core changed).
      *(nextest -p pair: 2723 passed / 35 skipped / 0 failed. e2e: 37 passed / 1 skipped
      (PC6 opt-in) / 1 failed = known environmental firefox-not-installed spec only;
      PC5 passing. Full `cargo xtask verify` GREEN on run 3 — 10589/10589 Rust tests,
      all hub legs — after serializing `julia_engine_e2e` via nextest test-group
      (156b290ec): runs 1–2 failed with rotating "Incorrect HMAC digest" victims
      (j1, then j3+j4 on a clean machine after the user-approved orphan reap),
      diagnosing an intra-suite race on the single shared ambient transport file —
      pre-existing test-infra property, not this branch's diff; hermetic follow-up
      filed as bd-wsh4ybhc. Full story: task-p3-report.md §3; review Approved,
      task-p3-review.md.)*

### P4 — closure
- [x] Update bd-h4rhohhy (close if B fixed; else record state); compat log/migration guide
      reconciled; plan checkboxes reconciled.
      *(Strand → in_review with full outcome (comments c-eps49gsq, c-5ep9kmp7, c-9fvojb24):
      "Bug B" refuted as a delivery defect; user symptom re-attributed to Bug A + Bug C,
      both fixed. Left OPEN pending the user's merge-back decision + acceptance run of the
      real ~/docs/julia doc (which also needs its project _extensions/julia-engine updated
      from the upstream q2-close-busy-fix branch). Compat log §15 + migration guide
      reconciled at P1, forward-note re upstream tip's errorRunClose added at the final fix
      wave (b67cb48a3). Final whole-branch review: "With fixes", all fixes applied
      (5635aa3ec, fb95f8148, b67cb48a3); no correctness issues; merge risk LOW
      (.superpowers/sdd/final-review.md). Full verify green run 3 (10589/10589).
      Orthogonality evidence: PC6 green against the PRE-fix engine — q2-side machinery
      works with an unmodified engine on a healthy server; the engine fix is robustness
      for the degraded states. Follow-ups: bd-wsh4ybhc (hermetic julia e2e); testing-
      strategy research claude-notes/research/2026-07-03-preview-testing-tiers.md.)*

## Test Seam Spec (frozen — prevalidated 2026-07-02)

Tiers: unit-ts (upstream deno, socket mocked) · int-rs (q2, real capture driver + failing
engine, no browser) · e2e-pw (q2-preview-spa playwright, REAL `q2 preview` binary + embedded
SPA + real chromium; env-gated: deno for echo rows, julia+deno for julia rows). Once green,
harness + assertions FROZEN.

| ID | Tier | Real unit | Seam → assertion | Mock boundary | Revert hunk → RED |
|----|------|-----------|------------------|---------------|-------------------|
| PC1 | unit-ts | `executeJulia` post-run close block (julia-engine.ts:742-749) | run succeeds, post-run close returns "worker is busy" error → executeJulia RESOLVES with the run result + a warning logged | QNR socket/`writeJuliaCommand` | revert the new non-fatal handling → executeJulia rejects → RED |
| PC2 | unit-ts | pre-run close path (:703-718) | pre-run close-busy → error message contains the actionable remedy substring (exact string frozen at fix time) | same | revert the message/recovery hunk → bare protocol error → RED (assert substring absent→present) |
| PC3 | int-rs | `record_eager_captures` error branch (capture_driver.rs:116-140) | registry with failing-engine doc A + echo doc B → Q-PREVIEW-CAP-1 emitted for A (test sink) AND B's capture recorded | failing engine only (real driver, real samod ctx) | (a) revert sink emission → diagnostic absent → RED; (b) make the loop return on first Err → B's capture absent → RED |
| PC4 | int-rs, julia-gated | shared-server busy-worker lifecycle | P0 harness; post-fix assertion FROZEN AT FIX TIME with controller sign-off (fresh capture succeeds via recovery, or fails with the PC2 message — whichever the ratified fix specifies) | none (real QNR) | the ratified fix hunk → pre-fix failure shape returns → RED |
| PC5 | e2e-pw, deno-gated | full delivery chain: set_capture → samod → onCapturesChange → getBinaryDocById → WASM splice | temp project w/ echo-engine doc; real `q2 preview` via previewServer.ts; page open; WAIT (no reload) for `ECHO_EXECUTED` in the pane AND assert the inert source token ABSENT from the final pane (splice replaced, not appended). *Amended at P2 (controller-ratified): the original inert→executed transition guard is unsatisfiable — the eager capture is recorded at server startup before the browser connects, so the first SPA render already splices; the stale-full-render vacuity it targeted doesn't exist (q2 preview serves the client-side SPA; ECHO_EXECUTED exists only in the capture bytes).* | none (real binary, real chromium) | capture_driver.rs:192-194 set_capture → RED-by-timeout (revert-PROVEN at P2). The contentTick-bump hunk (PreviewApp.tsx:729-738) is REBOUND to PC7 (jsdom, its explicit revert target); PC6's julia timing observes the live post-connect update path e2e. |
| PC6 | e2e-pw, julia-gated, **opt-in `QUARTO_PC6_LIVE=1`** | same chain, real julia | julia minimal doc (`daemon: false`); assert executed `2` appears in the pane without reload. *Green run recorded 2026-07-02 (6.5s). **P3 update:** isolation is CLOSED — `isolateJuliaProject()` now copies `QUARTO_JULIA_PROJECT`'s `Project.toml`/`Manifest.toml` into a per-test temp dir on top of the pre-existing temp-`HOME` transport override, live-verified (shared `julia_transport.txt` existence/mtime unchanged across the run). Opt-in is KEPT, but the reason changed: no longer an isolation gap, now purely environmental/speed — a real julia spawn (network-installed julia, multi-second server boot) — mirroring PC4a's `#[ignore]` gate. See task-p3-report.md.* | none | same set_capture hunk as PC5 (julia leg is the real-engine evidence row; PC5 is the fast CI guard) |
| PC7 | jsdom (vitest, q2-preview-spa) | PreviewApp onCapturesChange handler + render effect re-fire | integration-test mock pattern (PreviewApp.integration.test.tsx:549): fire onCapturesChange after initial render with a CaptureRef → assert renderPageForPreview called AGAIN with the binary doc's bytes | sync client + wasm renderer (existing mocks) | the `captures` state write in the onCapturesChange handler → no second render call → RED. *(P2 finding: the contentTick bump is REDUNDANT — the render effect already depends on `state.captures`; reverting contentTick alone stays GREEN. PC7 binds the load-bearing captures write; the redundancy is documented in the test and listed for final-review triage.)* |
| PC8 | int-rs | `perform_re_execute` failure branch (re_execute.rs:253-279) | failing engine → sidecar `CaptureState::Error` + `last_error` set + Q-PREVIEW-RE-1 emitted | failing engine | revert the error-write hunk → sidecar lacks Error state → RED |
| PC-C | int-rs deno-gated (framing) + resilience tier chosen at fix time | `StdioReadHalf::recv` framing + `reader_loop` Malformed arm (ts_process.rs) | (framing, GREEN at P0) >1MB frame → `Ok`; foreign/interleaved line → `Malformed`. (resilience, post-fix) a stray non-JSON line does NOT kill the host and does NOT fail an unrelated in-flight request; a following valid frame is still delivered; beyond the stray-line bound the kill-channel behavior is preserved | none (real pipe) for framing; mock read-half for resilience | revert the log-and-skip resilience hunk → one stray line kills all pending → RED |

**Vacuity notes:** PC5's binding is the ECHO_EXECUTED-only-in-capture property plus the
revert-proven set_capture hunk (the original inert-first transition guard was amended away
at P2 — see the PC5 row; PC7 carries the contentTick binding). PC3
needs doc B's positive assertion or the continue-on-error revert is unbound. PC1's mock must
fail ONLY the close (a mock failing the run too would pass a wrong implementation that
swallows run errors).

**Accepted-untested (logged):** canonical-input staleness rejection in WASM replay (add a
seam only if P0 diagnosis proves it's Bug B's cause); `getBinaryDocById` mime-type gating
(production JS never checks the mime — note for upstream polish, not a defect); hub-client
(does not consume captures at all).
