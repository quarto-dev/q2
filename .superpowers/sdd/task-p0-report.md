# Task P0 report — reproduce Bug A / Bug B / Bug C (bd-h4rhohhy)

**Status: DONE.** All three defects reproduced deterministically on this machine
(2026-07-02). Harnesses committed (diagnosis-only; zero product changes). Fix
SHAPES proposed below for the controller checkpoint — **no fixes implemented.**

Commit: `2931d7692 test(preview-capture): P0 repro harnesses for Bug A/B/C (bd-h4rhohhy)`

Files:
- `crates/quarto-core/tests/integration/ts_process_framing_probe.rs` (Bug C, new)
- `crates/quarto-core/tests/integration/main.rs` (register, +1 line)
- `crates/quarto-core/tests/integration/julia_engine_e2e.rs` (PC4a / Bug A)
- `q2-preview-spa/e2e/engine-capture-splice.spec.ts` (PC5 / Bug B, new)

---

## Verdict table

| Bug | Reproduced? | Deterministic? | Root cause status |
|-----|-------------|----------------|-------------------|
| A (close/busy) | YES (verbatim below) | YES | Root-caused (plan + confirmed live) |
| B (capture → pane) | YES, in **chromium** | YES (fails by timeout every run) | Localized to delivery chain; NOT Bug C. Precise link = P2 |
| C (wire framing) | Reader framing triaged | YES (3 probes) | Reader escalation confirmed; leak source engine-side (P1) |

**Does Bug C explain Bug B? NO — definitively.** PC5 reproduces Bug B with the
**echo** engine, which spawns no julia child, emits no `ts_process` error, and
the capture IS recorded server-side. Bug C and Bug B are independent defects
(see §Bug B and §Bug C).

---

## Bug A — oneShot close hits a busy worker → capture discarded

**Reproduced: YES, verbatim.** Deterministic via two concurrent oneShot renders
of one sleeping-cell doc sharing one julia server (the second render's pre-run
`close` collides with the first's still-running worker). Proven live via the
`q2` binary under an isolated HOME (never touching the user's real server
pid 9828); codified as `pc4a_shared_server_busy_close` (in-process,
`#[ignore]` + `QUARTO_PC4A_LIVE=1`, isolated HOME).

### Repro transcript (binary probe, 2026-07-02)

Isolation env (protects the user's server + reuses the pre-instantiated depot):
```
HOME=<temp>                 JULIA_DEPOT_PATH=~/.julia
QUARTO_JULIA_PROJECT=~/Library/Caches/quarto/julia
PATH=<real julia-1.11.7 bin>:$PATH     # NOT the juliaup shim (drifts under temp HOME)
```
Doc `sleepy.qmd`: `engine: julia`, `execute: {daemon: false}`, cell `sleep(25)\n1 + 1`.
Render A (background) starts the isolated server (transport up in 4s), worker
busy on `sleep(25)`; render B (same file, +6s) → **verbatim**:

```
Rendering single file: …/sleepy.qmd
Error: Execution failed in julia: Julia server returned error after receiving "close" command:

Failed to close notebook: …/sleepy.qmd

The underlying Julia error was:

Tried to close file "…/sleepy.qmd" but the corresponding worker is busy.

1 error
```

This is the exact user-reported failure. The pre-run close is
`executeJulia` julia-engine.ts:703-718 (`isopen`→`close`); there is no
busy handling anywhere in the engine (plan grep). The error propagates through
`render_to_file` and the whole render/capture is discarded.

### Root cause (verified file:line, per plan + confirmed live)
`~/src/quarto-julia-engine/src/julia-engine.ts`:
- pre-run close: `:703-718` (oneShot/restart → `isopen`→`close`, no busy guard)
- post-run close: `:742-749` (oneShot → `close`, no busy guard — latent: can
  discard a capture whose run SUCCEEDED)
- `startOrReuseJuliaServer` `:330-448` reuses ANY existing transport file
  regardless of `oneShot` (`:440-446`), so a busy/orphaned shared worker is
  reached by a fresh oneShot render.

### Proposed fix SHAPE (needs controller ratification before PC4 freeze)
- **PC1** (post-run close, `:742-749`): a post-run close failure after a
  successful run must be **non-fatal** — warn + return the run result.
- **PC2** (pre-run close, `:703-718`): a pre-run close-busy must NOT surface a
  bare protocol error. Either **recover** or **fail with an actionable message**
  naming the stale-server/transport remedy.
  - **Concrete recovery lead for P1:** julia-engine.ts already exposes a
    forceful close — the CLI `close` command calls `closeWorker(file, force)`
    with a `--force` option (`:~1002-1003`). QNR therefore supports a forced
    close; the pre-run close could pass `force: true` (or fall back to it on
    busy). P1 should confirm the QNR socket-command surface for `close` accepts
    a force flag before committing to recovery-vs-actionable-message.
- **Frozen PC4 post-fix assertion (controller signs off at fix time):** render B
  either SUCCEEDS via forced close, or FAILS with the PC2 actionable-remedy
  substring — never the bare `"worker is busy"` protocol error. (My harness's
  pre-fix assertion is `msg.contains("worker is busy")`; the fix flips it.)

---

## Bug B — recorded capture never reaches the browser pane

**Reproduced: YES, in chromium** (the user saw it in Firefox; it reproduces in
chromium too — a notable finding, it is NOT browser-specific). Deterministic:
`engine-capture-splice.spec.ts` (PC5) fails by timeout on every run.

### Repro transcript (PC5, `test.fail()` temporarily disabled to harvest evidence)

Real `q2 preview` + chromium, temp project with the committed **echo** engine
and `index.qmd` containing one `{echo}` cell with source `PC5_ECHO_SOURCE_TOKEN`.
The pane renders the INERT source, then times out (15s) waiting for the executed
marker `ECHO_EXECUTED`:

```
Error: pane must show the executed echo marker after the capture splices in; pane text was:

  PC5 echo capturePC5 headingPC5_ECHO_SOURCE_TOKEN

console:
[log] WASM module initialized successfully, template loaded
[log] Waiting for peer connection...
[log] Peer connected - online mode
[warning] An iframe which has both allow-scripts and allow-same-origin for its sandbox attribute can escape its sandboxing.

Expected: true
Received: false
```

Server side (independent manual `q2 preview` run, `RUST_LOG=quarto_preview=debug`)
— the capture IS recorded and the sidecar written:
```
INFO quarto_preview::capture_driver: recorded engine capture(s) rel_path=index.qmd engines=echo
INFO quarto_preview::capture_driver: recorded engine captures count=1
# data-dir/captures/<sha>.bin written (gzip EngineCapture)
```

### Boundary evidence / where it breaks
- **Server**: capture recorded + `IndexDocument::set_capture` writes the sidecar
  (`capture_driver.rs:184-205`). WORKING.
- **Browser**: SPA WASM initialized, **"Peer connected - online mode"** (samod
  sync is up), doc rendered — but the pane shows only the inert source, and
  there is **no capture-related console log and no error**. The executed marker
  never splices in.
- Because the eager capture is recorded at server startup (before the browser
  connects), the SPA should receive it via the **initial** `onCapturesChange`
  (`quarto-sync-client/src/client.ts:779-781` / `:1351-1353`, fired off the
  IndexDocument's `captures` map). Captures ride on the IndexDocument
  (`getCapturesFromIndex`, `:339-364`); the SPA is synced to that doc (it
  rendered), so the sidecar entry should be visible on first fire.

**Conclusion: Bug B is an independent delivery-chain defect, NOT Bug C.** Root
cause NOT determined at P0 (that is P2's job); it is localized to the
`set_capture → samod → onCapturesChange → PreviewApp → getBinaryDocById → WASM
splice` chain, browser-side of "Peer connected". Candidate links for P2, in
descending suspicion:
1. The **capture BINARY doc** (a separate samod doc referenced by
   `captureDocId`, written by `write_capture_doc` capture_driver.rs:326) is not
   synced/resolvable to the SPA — `getBinaryDocById` (client.ts:~1007-1019)
   returns nothing and the splice silently no-ops (consistent with "no error,
   no log").
2. Initial `onCapturesChange` fires before the render is ready and the
   `contentTick` bump (PreviewApp.tsx:729-738) is lost / the render effect
   (`:1005-1030`) does not re-fire.
3. `state.activeFile` key vs the sidecar `rel_path` key mismatch in the render
   effect (plan candidate).

### Proposed fix SHAPE
Deferred to P2 by design (plan §P2: "location unknown until P0"). P2's entry
point = PC5's failure + this boundary evidence; recommended first step is
targeted SPA instrumentation (console.log at `onCapturesChange`,
`getBinaryDocById`, and the render effect — **rebuild the SPA per the binding
rules**, revert before commit) to identify the silent link, then a minimal fix
on that link. **Frozen post-fix assertion = PC5 as written** (`ECHO_EXECUTED`
appears in the pane without reload; remove `test.fail()`), plus the julia leg
PC6 and the jsdom-tier PC7.

---

## Bug C — engine-host stdout wire-frame corruption

**Reader framing triaged (the P0 mandate); leak source is engine-side (P1).**
Three deterministic probes (`ts_process_framing_probe.rs`, deno-gated), all GREEN:

```
PASS ts_process_framing_probe::pc_c_a_large_single_line_frame_parses
PASS ts_process_framing_probe::pc_c_b_foreign_line_is_malformed
PASS ts_process_framing_probe::pc_c_b_prime_interleaved_bytes_corrupt_frame
3 tests run: 3 passed
```

### Findings
- **(a) Large-frame suspect RULED OUT.** A >1 MB single-line frame parses to
  `Ok(Response)` — `BufRead::read_line` has no size cap and loops the 8 KB
  BufReader buffer to the terminating `\n`. The reader does NOT truncate or
  mis-split large frames. So the "legit executeResult frame rejected" symptom
  is NOT a large-frame reader bug.
- **(b) Foreign / interleaved bytes → `RecvError::Malformed`.** A stray ANSI
  julia log line on the wire (symptom #1) and foreign bytes spliced into a
  frame's middle (symptom #2) both fail framing at `StdioReadHalf::recv`
  (ts_process.rs:292-308). Both live symptoms are the **same** root cause: a
  foreign writer on the engine-host's stdout fd.
- **Catastrophic escalation (by reading, ts_process.rs:930-954):** a single
  `Malformed` makes `reader_loop` set `shutting_down`, **broadcast an error to
  EVERY pending slot, and kill the whole Deno subprocess.** One stray line
  destroys the entire engine host and every in-flight capture. This is why the
  user "sometimes sees no result" — the executeResult is dropped AND the host
  dies.

### Leak source (engine-side; candidate, not the P0 mandate)
- **Ruled out for the live session:** the `Deno.stdout.writeSync` sites in
  julia-engine.ts (`:1035` `logStatus`, `:1056` `printJuliaServerLog`) are
  **Cliffy CLI subcommand handlers**, not the engine-host module path — they do
  not fire when julia-engine.ts is imported as an engine.
- **Candidate:** `start_quartonotebookrunner_detached.jl` runs
  `run(detach(cmd), wait = false)` with **no stdio redirection**, so the
  detached QNR server inherits fds; its startup banner (`[ Info: Log started
  at …`) can land on an inherited stdout. Precise mechanism is engine-side
  forensics owned by P1 — not required to close the P0 reader triage.

### Proposed additive seam row (controller sign-off required)

| ID | Tier | Real unit | Seam → assertion | Mock boundary | Revert hunk → RED |
|----|------|-----------|------------------|---------------|-------------------|
| PC-C | int-rs, deno-gated (framing) + unit-ts (demux resilience) | `StdioReadHalf::recv` framing + `reader_loop` Malformed arm | (framing, DONE) >1MB frame → `Ok`; foreign/interleaved line → `Malformed`. (resilience, POST-FIX) a stray non-JSON line does NOT kill the host and does NOT fail an unrelated in-flight request; a following valid frame is still delivered | none (real pipe) for framing; MockReadHalf for resilience | revert the log-and-skip resilience hunk → one stray line kills all pending → RED |

### Proposed fix SHAPE (two independent, both need sign-off)
1. **Engine-side (P1/upstream) — the root fix:** ensure NO child process
   inherits the engine-host's stdout fd. The detached julia launcher must
   redirect the server's stdout/stderr to its log file / devnull rather than
   inherit, so a QNR banner can never reach the protocol channel.
2. **Reader-side defense-in-depth (quarto-core, `reader_loop:930-954`) —
   POLICY CHANGE, explicit sign-off:** make the reader resilient to a stray
   non-protocol line (bounded log-and-skip) instead of `Malformed → kill-all`,
   so one leaked banner does not discard every in-flight capture. This changes
   the current "compromised channel ⇒ kill subprocess" contract; the comment at
   `:930-935` (finding #7, "one terminal error per exit") documents the
   intent, so a change here must be deliberate.

---

## Ruled-out hypotheses (evidence, not assumption)
- **"Large executeResult frame is rejected by the reader."** RULED OUT — probe
  `pc_c_a` shows a 2 MB single-line frame parses to `Ok`.
- **"Bug C is the root of Bug B."** RULED OUT — PC5 reproduces Bug B with echo
  (no julia child, no `ts_process` error, capture recorded server-side).
- **"Bug B is Firefox-specific."** RULED OUT as a necessary condition — it
  reproduces in chromium.
- **"The julia-engine.ts `Deno.stdout.writeSync` calls corrupt the wire in the
  live session."** RULED OUT — they are CLI-only, not on the engine module path.

## Constraints honored
- No product code changed (harnesses/probes only; `git status` clean but for the
  4 committed files). Never pushed.
- PC4a ran only under an isolated temp HOME; its julia server (pid 1478) was
  killed after the probe. The user's server (pid 9828) and the pre-existing
  leaked workers (bd-l9jhy5u0; they use the user's transport file / TempDir
  projects, not my isolated HOME) were left untouched.
- PC5 uses `test.fail()`; PC4a uses `#[ignore]` + `QUARTO_PC4A_LIVE` opt-in;
  the Bug C probe is deno-gated and GREEN — none break the default suites.
- SPA + WASM + q2 binary were rebuilt (per the binding rebuild rules) before the
  browser-tier PC5 run; the first run had shown the placeholder SPA.

---

# Fix wave (review response — task-p0-review.md, 2026-07-02)

Addresses the four Important findings. Still P0 (harnesses/diagnosis only; the
one product-file touch — PreviewApp.tsx instrumentation — was quarantined and
reverted; `git status` shows no product changes).

## Fix #1 + #2 — PC4a rewritten to the specified scenario, with cleanup

`pc4a_shared_server_busy_close` → **`pc4a_abandoned_worker_close_busy`**. Now
matches the plan §P0 spec exactly:
- Drives **`record_capture`** (`quarto_core::engine::preview_record::record_capture`),
  not `render_to_file` — the soft-fail caller path the bug actually lives on.
- The first run's client is **ABANDONED mid-run**: its Deno engine-host is
  killed (identified by the isolated-HOME bundle path in its cmdline) while the
  worker is provably executing, closing the QNR socket → EPIPE → the worker is
  left **orphaned-busy** (QNR does not cancel the task). Then a **fresh**
  `record_capture` of the same doc → oneShot pre-run close hits the abandoned
  worker.
- **Real signal, not a timer** (addresses the Minor): the cell writes a sentinel
  file before sleeping; the harness abandons the client only after the sentinel
  appears (worker provably mid-run). The old flat `sleep(6)` guess is gone.
- **Cleanup (Fix #2):** an `IsolatedJuliaServerGuard` Drop kills the detached
  server's **process group** (pid read from the isolated transport file) on
  scope exit, **even on panic** — so no server leaks per run. `#[cfg(unix)]`
  gated (process groups); `#[ignore]` + `QUARTO_PC4A_LIVE=1` opt-in unchanged.

### New verbatim failure (record_capture #2, live 2026-07-02)
```
Stage 'engine-execution' failed: Execution failed in julia: Julia server returned error after receiving "close" command:

Failed to close notebook: /var/folders/…/T/.tmpncK0gc/sleepy.qmd

The underlying Julia error was:

Tried to close file "/var/folders/…/T/.tmpncK0gc/sleepy.qmd" but the corresponding worker is busy.
```
Note the entry point is now `Stage 'engine-execution' failed` (the
`record_capture` pipeline), not a CLI render — the correct soft-fail path.

### Cleanup verified
`pgrep -f quartonotebookrunner.jl | wc -l` = **4 before, 4 after** the run; the
only survivors use the user's transport file (pre-existing bd-l9jhy5u0 workers).
The isolated server the harness started was killed by the guard — no net leak.

### Fix-shape update (scenario distinction, per review #1)
The **PC4 frozen assertion targets the ABANDONED-worker scenario**: the fresh
`record_capture` either RECOVERS via a forceful close (→ succeeds) or fails with
the actionable PC2 remedy — never the bare `"worker is busy"`. Force-close is
correct here **because the worker is abandoned**. The **concurrent-live-render**
case (where force-close would harm a legitimate in-flight execution) is
explicitly OUT of PC4's scope — it is the plan's documented-not-gold-plated
`oneShot`-server-reuse design question for the upstream PR (plan §P1). P1 must
not let a force-close recovery kill a *live* peer's worker; scoping the recovery
to detectably-abandoned workers (or to the actionable-message path) is the safe
default.

## Fix #3 — PC5 sync-client state harvested (re-ranks the P2 candidates)

Quarantined instrumentation (reverted before commit) stashed every
`onCapturesChange` payload on `window.__pc5CaptureLog` and logged the render
effect's capture lookup; `window.__renderTicks` (a production counter) gave the
render count. Harvested from the PC5 failure:

```
sync-client state:
{
  "captureLog": [
    { "keys": ["index.qmd"],
      "captures": { "index.qmd": { "captureDocId": {"val":"31WYwMd3ZW8Xv92aSghTL6oT1xLo"},
                                   "staleness": false } } }
  ],
  "renderTicks": 1
}
console:
  PC5-DIAG onCapturesChange ["index.qmd"]
  PC5-DIAG renderEffect {"activeFile":"index.qmd","captureKeys":["index.qmd"],
                         "hasRef":true,"docId":"31WYwMd3ZW8…","gotBinary":true,"bytes":567}
```

**This CHANGES the P2 candidate ranking decisively.** The entire delivery chain
is confirmed WORKING, end to end:
- `onCapturesChange` fired with the entry keyed `index.qmd` (sidecar synced to
  the SPA). ⇒ RULES OUT "sidecar not delivered".
- `activeFile == "index.qmd" == sidecar key` ⇒ RULES OUT the activeFile-vs-rel_path
  key-mismatch candidate.
- `hasRef: true` ⇒ the capture ref resolved in state.
- `gotBinary: true, bytes: 567` ⇒ `getBinaryDocById` SUCCEEDED and the gzipped
  capture bytes were fetched ⇒ RULES OUT "capture binary doc not synced/resolvable"
  (my prior #1 candidate).
- `renderTicks: 1` ⇒ the render effect fired (once, WITH the capture present) ⇒
  RULES OUT "contentTick effect not re-firing".

`renderPageForPreview("index.qmd", undefined, captureGzJson[567])` was therefore
called **with** the capture bytes, yet the pane rendered the **inert** source
(no `ECHO_EXECUTED`). **The break is inside the WASM `render_page_for_preview`
ReplayEngine splice**, downstream of everything the delivery chain does.

### Revised P2 candidate ranking (Bug B)
1. **PRIMARY — WASM ReplayEngine splice rejects/ignores the capture.** Strongest
   sub-candidate: the **canonical `input_qmd` staleness check in WASM replay**
   (the plan's explicitly "accepted-untested" item) rejects the capture on a
   byte mismatch between the recorded `input_qmd` and what the WASM recomputes,
   silently falling back to the default markdown engine → inert render. (Note the
   sidecar's own `staleness:false` is the SERVER's flag; the WASM replay applies
   its OWN canonical-input check — they are independent.) The 567 bytes reaching
   WASM but no splice is exactly this signature.
2. Secondary — ReplayEngine construction from `captureGzJson` (gunzip/parse)
   fails silently WASM-side.

Bug B's fix therefore most likely lands in **quarto-core's WASM replay path**
(`render_page_for_preview` / `ReplayEngine`), requiring a hub WASM rebuild
(plan §P2). This retires the plan's "add a seam only if P0 diagnosis proves it's
Bug B's cause" condition on the canonical-input staleness seam: **P0 now
implicates it** — P2 should add that seam.


## Fix #4 — full `npm run test:e2e` proves the new spec doesn't break the suite

Ran the entire suite once (`q2` re-embedded from the reverted/clean SPA source,
verified `strings target/debug/q2 | grep -c PC5-DIAG` = 0):

```
37 passed (28.9s)
1 failed
```

- **PC5 (`engine-capture-splice.spec.ts`, #30) is among the 37 passed** — its
  `test.fail()` records the pre-fix RED as an EXPECTED failure; it is NOT in the
  failures detail section.
- The **1 failed is pre-existing and orthogonal**: `firefox-ws-queue.spec.ts`
  under the **firefox** project fails to launch because Firefox is not installed
  on this machine (`browserType.launch: Executable doesn't exist … firefox-1522/
  firefox/Nightly.app`). The SAME spec **passes under chromium** (#31 ✓). My
  changes touch nothing in that spec. The brief provisioned chromium only.

Net: the new PC5 spec does not break the default e2e suite.

## Files (fix wave)
- `crates/quarto-core/tests/integration/julia_engine_e2e.rs` — PC4a rewritten
  (`pc4a_abandoned_worker_close_busy`): `record_capture` + client-abandonment +
  sentinel signal + `IsolatedJuliaServerGuard` cleanup.
- `q2-preview-spa/e2e/engine-capture-splice.spec.ts` — PC5 now also harvests
  `renderTicks` (+ `__pc5CaptureLog` when diagnostic instrumentation is present).
- `q2-preview-spa/src/PreviewApp.tsx` — instrumentation was added, harvested,
  and **reverted** (no net change; confirmed clean in the binary).
