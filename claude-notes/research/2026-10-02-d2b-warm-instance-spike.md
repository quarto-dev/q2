# D2(b) spike: one warm pandoc.wasm instance across renders (2026-10-02)

**Verdict: a warm instance works and is worth building for the PDF preview only.** It runs through the module's exported `hs_init_with_rtsopts` and `convert`
(not `_start`), is correct across renders and its memory is flat. It saves **55 ms in Chromium, 41 ms in Firefox and 186 ms in WebKit** per render, and the pandoc leg stays at
**99-111 ms (about 105)** in all three (callouts.qmd to typst). It cannot get near the ~20 ms that would make typst.ts `incr_compile` worth revisiting. It costs a failure mode (a poisoned
instance) and a semantic gap (Haskell-side `SOURCE_DATE_EPOCH`), which is why Download and parity stay on the fresh path. The build is planned as H10a (executor) and H10b (pool and preview);
the sections below say what each finding means for it. A pre-instantiated spare fresh instance (hides WebKit's slow instantiation, needs no Lua preamble or translator) is the deferred
alternative; H10a Task 0(c) measures it.

Setup: pandoc.wasm 3.11 (`resources/pandoc-wasm.json`), the R0 typst recordings (argv, env, share tree, input JSON) replayed;
Node 24.5 (V8), Chromium 148.0.7778.96 and WebKit 26.4 under Playwright 1.60, one Mac with other work running (read as magnitudes).
Module compiled once on the main thread and posted to workers, as `PandocRunner` does. "Fresh" = a short-lived module worker per
render running `_start` (the production shape, `execute.ts` re-implemented in `d2b/core.mjs`); "warm" = one persistent worker, `convert`
per render, mount rebuilt each render. Median of 12 after one discarded run, end-to-end from the main thread (worker spawn and message
cloning included). Sources: `claude-notes/research/2026-10-01-pandoc-wasm-spike/d2b/` (`node --experimental-wasm-exnref d2b/drive.mjs chromium|webkit all`;
raw JSON in `d2b/results/`). The Rust request builder is not in the loop: pampa's real argv/env come from the recordings, so these numbers are the pandoc leg only.
Fixtures: `callouts` (46 KB AST), `callouts-x10` (the same blocks repeated 10x, 188 KB: the "larger typst fixture"; synthetic, no real
large typst recording exists), `empty` (zero blocks: the fixed floor of the filter chain).

## 1. Does a warm instance work? (command vs reactor)

- **Command mode, `_start` twice on one instance: no.** The first run returns 0; the second traps `RuntimeError: unreachable`
  (GHC's `hs_main` cannot be re-entered). `proc_exit` is not what stops it; the RTS is simply already shut down.
- **The build also exports `__wasm_call_ctors`, `hs_init_with_rtsopts`, `convert`, `query`, `malloc`.** Calling them (no `_start`) gives a
  reactor-style instance: init once (24 ms Chromium, ~135 ms WebKit, i.e. the same as instantiation), then `convert(optionsJson)` any number of times.
  The options are pandoc *defaults-file* keys, not argv; the typst argv maps one-to-one (`from`, `to`, `data-dir`, `filters:[{type:lua,path}]`,
  `standalone`, `wrap`, `resource-path`, `template`, `variables`, `output-file`, `input-files`; `d2b/core.mjs: argvToOptions`).
  A production build needs a complete, tested argv-to-defaults translator (the request carries the whole argv, which has no RTS options).
- **RTS options are fixed at `hs_init`.** The old spike host passed `-H64m`; that makes the *first* `convert` cost ~1 s in V8 (Node: 1090 vs 172 ms).
  Production argv has no RTS options, and the warm numbers here use none.
- **`convert` returns no exit status**; failures show as a missing output file plus `ERROR: ...` on stderr, `os.exit(n)` as a thrown
  `Error: exit with exit code n`, and heap exhaustion as exit code 1 (fresh `_start` gives 251). The `(success, status, stderr)` classification would need rework.

## 2. Latency, warm vs fresh (ms, median of 12; min-max in the JSON)

| fixture | | Chromium fresh | Chromium warm | WebKit fresh | WebKit warm |
|---|---|---:|---:|---:|---:|
| **callouts** | end-to-end | **160** | **105** | **285** | **99** |
| | instance creation | 24 | 0 (30 once) | 131 | 0 (146 once, incl. worker spawn) |
| | mount (252 files, fresh tree) | 0 | 0 | 1 | 0 |
| | pandoc run | 130 | 103 | 146 | 97 |
| | output read | 0 | 0 | 0 | 0 |
| | worker spawn + message cloning | 6.7 | 0.8 | 11 | 1 |
| **callouts-x10** | end-to-end | 600 | 561 | 682 | 508 |
| | pandoc run | 568 | 560 | 532 | 505 |
| **empty** (floor) | end-to-end | 104 | 60 | 236 | 57 |
| | pandoc run | 74 | 58 | 98 | 55 |

(Node/V8 without a worker: callouts 207 vs 141 ms, x10 836 vs 756 ms.) Outputs are byte-identical fresh vs warm in every run (SHA-256 prefix compared).

Reading it: warm removes instantiation (24 ms Chromium, ~131 ms WebKit, whose instantiate is slow) and the worker spawn, plus ~25 ms of per-run
start-up in Chromium. **What remains is the work itself**: even an empty document takes ~58 ms (Lua state creation, Quarto's `init.lua` and `main.lua`
loading, template parse), and it grows with the document (x10: 560 ms, ~50 ms per callouts-sized chunk). None of that is cached by keeping the
instance: each filter gets a new Lua state per `convert`, the V8/JSC tier-up already persists in the resident `Module`.
The first `convert` on a new warm instance is as slow as a fresh run (Chromium ~120 ms first convert, WebKit ~130 ms).

## 3. Correctness across consecutive renders

All pass in Node, Chromium and WebKit (`q3.mjs`, `Browser.correctness`):
- A,B,A,C,A,B (callouts, tables, crossrefs) on one instance: every output byte-equal to its fresh-instance reference. No leaked Lua globals, VFS or temp state
  (the host rebuilds the tree each render; pandoc's own state did not leak in these fixtures).
- **Env delivery.** The WASI `environ` is read once: mutating the shim's env array after instantiation changes nothing (probe filter: still `one|111` after
  setting `two/222`). The proposed **Lua preamble does work for Lua**: prepending an `os.getenv` override to the datadir `init.lua` (which pandoc runs in *every*
  Lua state, before Quarto's own init, which itself calls `os.getenv` for `QUARTO_SHARE_PATH`, `QUARTO_FILTER_PARAMS`, `QUARTO_FILTER_DEPENDENCY_FILE`) delivers a different
  env per render: `one|111`, `two|222`, `one|111`, and the full Quarto chain renders byte-equal to fresh with the WASI env **empty**. A preamble must go there, not into a
  separate `-L` file, because each filter has its own Lua state. It means rewriting one share-tree file per render (the mount rules say a path may exist in only one place,
  so the host does the prepend rather than the request carrying two copies).
- **The gap: pandoc's Haskell side reads `SOURCE_DATE_EPOCH` from the cached WASI environ and the preamble cannot reach it.** docx `dcterms:created` with a fresh instance
  honours SDE (2001-09-09 for 1e9). On a warm instance, a mutated env array gives the *creation-time* value; a preamble-only instance gives the wall clock. So warm
  works for typst/PDF (pandoc's typst writer emits no timestamps; typst compiles dates on its own side), but the pinned-SDE reproducibility the parity net needs for
  docx/pptx/epub requires a fresh instance (or one warm instance per distinct SDE).

## 4. Memory and errors

- **100 renders, callouts:** wasm linear memory 42 -> 44 MB and flat from render 10 on, warm (Chromium and WebKit); fresh instances sit at 47 MB each. Node RSS after GC:
  warm -11 MB, fresh +82 MB over 100. Browser process RSS (noisy, other tabs/browsers present, not clean): Chromium warm 1454 -> 1526 MB (peak 1644), fresh 1528 -> 2579 MB (peak 2638),
  i.e. discarding 100 workers lets the browser retain ~1 GB until GC catches up; WebKit warm flat, fresh +66 MB. **Growth per warm render: ~0.** (Fresh looks worse in Chromium, but that was
  the case for H6's 20-render test too and it levels off.)
- **High-water mark is permanent.** Linear memory never shrinks: a filter that allocated 2 GB left a warm instance at 2129 MB for good (every later render fine, but the page
  keeps it). A warm host needs "recreate when `memory.buffer.byteLength` exceeds N MB". Fresh instances are freed by terminating the worker.
- **Recoverable errors** (Node, Chromium, WebKit): missing input, Lua syntax error, `error()`, nil index, missing filter, unknown format, bad JSON all leave the instance usable;
  the next render is byte-equal to the reference. `os.exit(3)` throws and the instance still works.
- **Heap exhaustion kills the instance for good.** With `+RTS -M100m` and a 6 MB input, fresh `_start` exits 251 (clean `oom` classification today); the warm `convert` throws
  `exit with exit code 1` and **every later `convert` throws the same, including a trivial good one**. A warm host must treat any trap or exit-1-without-stderr as "poisoned, discard and re-create".

## 5. Cancellation

A runaway filter (`while true do end` inside `convert`) blocks the worker; only `Worker.terminate()` stops it, exactly as today, and `terminate()` returns at once (0 ms). The instance is lost with the worker:

| after terminate, the next render costs | Chromium | WebKit |
|---|---:|---:|
| fresh path (today): ordinary render | ~150 ms | ~285 ms |
| warm path: new worker + instantiate + first `convert` | ~154 ms | ~280 ms |

So a policy that terminates the worker whenever a newer render supersedes it pays the cold cost on every superseded render and gets none of the warm benefit. H10b therefore does not
terminate on supersede: the old render keeps running, the newest request starts on a second warm worker, a pending request is replaced by a newer one (latest wins), and the pane applies
results in request order. A worker is terminated only on the wall timeout, by an adaptive grace rule for a superseded runaway, when poisoned or oversized, on a reboot, or when idle.
The preview pane debounces edits by 500 ms today (250 ms on the warm path if H10a Task 0(f) allows), so renders overlap only when the pandoc leg is longer than the debounce, which means large documents.

## 6. Net effect on the per-edit PDF preview

| | Chromium | WebKit |
|---|---:|---:|
| today: pandoc (fresh) + typst 2-5 ms | ~163-165 | ~287-290 |
| warm: pandoc (warm) + typst 2-5 ms | ~107-110 | ~101-104 |
| saving | ~55 ms (34%) | ~186 ms (65%) |

Only in WebKit is the saving large, and it comes from skipping its slow instantiation, which a pre-spawned spare fresh instance would also hide with no Lua preamble, no argv-to-defaults
translation and no poisoned-instance handling (H10a Task 0(c) measures that alternative). **`incr_compile` stays "no"**: its ~4 ms saving would be 4% of a ~105 ms pandoc leg, and the leg's floor is ~58 ms even for an empty
document. The H9 verdict in `2026-10-02-incr-compile-spike.md` is confirmed, not revisited.

## Firefox 157 (by hand, default browser; `d2b/serve-ff.mjs` + `ff.html`, raw JSON in `d2b/results/firefox-all.json`)

Same suite, median of 12 after a discarded run. callouts: **fresh 152 ms, warm 111 ms** (saving 41 ms); x10: 566 vs 529; empty (floor): 125 vs 63. Instantiation is cheap
here (8 ms), so warm only removes worker spawn/clone (13 ms) and per-run start-up. Correctness is identical to Chromium/WebKit: A,B,A,C,A,B byte-equal; the preamble env probe gives
`one|111`, `two|222`, `one|111`; recoverable errors leave the instance usable; `os.exit` throws but the instance survives; heap exhaustion (`-M100m`) poisons it for good (exit 1 forever;
fresh gives 251). Wasm memory flat over 100 renders (44 MB warm, 47 MB fresh; RSS not sampled). After a terminate, the next render is ~145 ms in both paths (no warm benefit).
So the saving is 41 ms (Firefox), 55 ms (Chromium) and 186 ms (WebKit), and the warm pandoc leg is 99-111 ms in all three browsers.

## What the build needs, and where it is handled
(1) an argv-to-defaults translator with a parity test per request shape: H10a Task 2, with a Rust allowlist guard and a fresh-path fallback; (2) the `init.lua` preamble transform in the host: H10a Task 3;
(3) poisoned-instance detection and recreate-on-memory-threshold: H10a Task 5; (4) a fresh instance for docx/pptx/epub because of `SOURCE_DATE_EPOCH`: Download and parity keep the fresh path; (5) a
supersede story: H10b's no-kill pool. Found after this spike: the warm `convert` discards WASI fd 1 and fd 2 in the spike harness, so Lua `io.stderr` warnings and the RTS's out-of-memory text must be captured
(H10a Task 4); pandoc's own warnings are written as JSON to `/warnings` and leave stderr empty; `--defaults` and a `defaults` key are rejected by `convert`; and `os.exit(1)` and heap exhaustion both
show as "exit 1", so only an fd-2 message or a trap marks out-of-memory.

## Measurements from the review of the build plan (2026-10-02)
- Cost against document size, one warm instance: callouts x10 0.56 s, x100 10.9 s.
- Warm memory by AST size: about 113 MB at a 1.5 MB AST, 317 MB at 6 MB, 1.2 GB at 24 MB.
- A filter's `os.exit(1)` throws `exit with exit code 1` with empty stderr and leaves the instance usable, the same signature as heap exhaustion except that the instance keeps working afterwards.
- `convert` writes pandoc warnings as JSON to `/warnings` (`[{type, verbosity, message, path, pretty}]`), whereas `_start` prints `[WARNING] Could not fetch resource missing.png: replacing image with description` on stderr.
- The WebKit column above was taken in a different run from `d2b/results/webkit-all.json` (raw: callouts fresh 285, warm 99; empty fresh 236, warm 57; next render after terminate 285); H10a Task 0(e) replaced the differing figures (done 2026-10-03: the WebKit column above, the verdict, section 6 and the Firefox summary now carry the raw-JSON figures; the evidence for each is `results/webkit-all.json`).


## H10a Task 0a: measurements before the build (2026-10-03)

Harness: `d2b/` extended with `drive0.mjs` (Chromium/WebKit driver), `ff0.html` + `ff0.mjs` + `serve-ff0.mjs` (the same suite for a browser Playwright cannot launch; Firefox is Gordon's),
a typst worker bundled from the host's `TypstSession` (`typst-worker.src.mjs`, `build-typst.mjs`; one fresh worker per compile, as `TypstRunner` does) fed from the hub-client assets
(`public/typst/*.gz`, `resources/typst-packages`), and the new modes `B.warmup`, `B.spare`, `B.stderrProbe`, `B.typstCompile`, `B.contention` in `page.mjs` (`core.mjs` gained `spareInstance` and fd-1/fd-2 capture).
Run: `node build-typst.mjs && N=12 node drive0.mjs chromium|webkit all`. Raw JSON: `d2b/results/h10a-0a-chromium.json`, `h10a-0a-webkit.json` (and `h10a-0a-firefox.json` once Gordon has run `serve-ff0.mjs`).
Conditions: callouts.qmd to typst through the R0 recording; median (min-max) in ms over 12 runs after one discarded cycle; Chromium 148 and WebKit 26.4 under Playwright 1.60 on a Mac whose load average was 19-26
on 12 cores (other sessions running), so absolute figures are about 15% slower than the spike's (steady warm 121 / 113 ms here against 105 / 99) and are read as magnitudes; every conclusion below is a same-run comparison.

**(a) Is a new instance hot after a warm-up? Yes.** `convert` after one warm-up is within 15 ms of the steady warm median in both browsers, with or without idle time, so **H10b warms every new worker by replaying the latest real request once** and the cold-spare fallback is not needed.

| | Chromium | WebKit |
|---|---:|---:|
| (a) new instance, first `convert` (cold) | 146 (138-159) | 170 (144-267) |
| (a) `convert` right after one warm-up | 126 (122-135) | 116 (111-163) |
| (a) steady warm `convert` (same run) | 121 (115-125) | 113 (106-122) |
| (a) difference, no idle gap | **+5.5** | **+3.0** |
| (a) difference, 500 ms idle before the timed `convert` | +4.4 | +4.0 |

The first `convert` on a new instance costs about 25 ms (Chromium) and 57 ms (WebKit) more than steady state, which is what the warm-up pays for.

**(b) Contention: no measurable cost. The gate passes.** Two warm workers; the older render starts, the newer starts 40 ms later on the other worker; both are timed from their own start and compared with the same render alone (`B.contention`).

| scenario | Chromium | WebKit |
|---|---|---|
| two pandoc workers (the gate) | alone 118 (112-123); older 116 (111-121); newer 118 (111-130); **older -2, newer +0** | alone 109 (102-114); older 107 (101-114); newer 109 (102-113); **older -2, newer +0** |
| plus a concurrent typst compile and a main-thread busy loop (10 ms every 20 ms) | alone 126 (112-127); older 119 (111-132); newer 126 (111-127); **older -7, newer -1** | alone 107 (103-118); older 116 (107-131); newer 114 (104-121); **older +9, newer +7** |
| Chromium 4x CPU throttle | alone 120 (116-124); older 122 (118-137); newer 122 (113-127); **older +2, newer +1** | not available |
| every core but one busy with spin workers (informational) | alone 169 (139-186); older 166 (130-197); newer 164 (117-185); **older -2, newer -5** | alone 135 (101-150); older 139 (122-152); newer 143 (112-159); **older +4, newer +8** |

Neither render is slowed by more than ~30 ms in the unthrottled run (it is 0 +/- 2 ms), so H10b proceeds without asking. Two observations: Chromium's CPU throttle did **not** slow the workers (alone: 120 ms
throttled against 118 unthrottled), as the plan suspected, so that row says nothing about a slow machine; the loaded-cores row does (alone rises from 118 to 169 ms in Chromium with 11 spinners) and the second render still costs nothing extra over it. The typst compile that ran beside the pandoc renders
took 90 ms (Chromium) and 126 ms (WebKit) as a fresh worker per compile (spawn, `TypstSession.create`, compile), against the 2-5 ms of the `incr_compile` note, which measured a warm resident compile: Task 0(f) measures the real split.

**(c) Pre-instantiating a fresh `_start` instance (informational, deferred alternative).** Argv, env and the tree can be supplied after instantiation (the shim reads them at `_start`); output is byte-equal to a fresh run.

| | Chromium | WebKit |
|---|---:|---:|
| spawn + instantiate the spare (off the critical path) | 41 (34-55) | 188 (166-273) |
| request to output on an idle spare | 139 (133-142) | 168 (155-181) |
| fresh end to end today (same run) | 173 (167-184) | 356 (332-413) |
| steady warm (row (a)) | 121 (115-125) | 113 (106-122) |

A spare hides instantiation and worker spawn (Chromium 173 to 139 ms, WebKit 356 to 168 ms) but not the cold per-run start-up the warm instance also avoids: it ends about 18 ms (Chromium) and 55 ms (WebKit) behind the warm instance. It needs no Lua preamble,
no translator and no poisoned-instance handling, which is its case; it never gates anything here.

**(d) Does Lua `io.stderr:write` land on WASI fd 2 in `convert` mode? Yes. The check passes in Chromium and WebKit** (Firefox pending). A filter that wrote `PROBE-IO-STDERR` and `WARNING (f.lua:1) quarto-style warning` to `io.stderr` and `PROBE-IO-STDOUT` to `io.stdout`
gave fd 2 = `PROBE-IO-STDERR\nWARNING (f.lua:1) quarto-style warning\n`, fd 1 = `PROBE-IO-STDOUT\n`, the `/stderr` file empty and `/warnings` = `[]`. So Task 4b captures fd 2 as written, and Lua warnings
(`quarto.warn`, the shim's `Q-20-5/6/7`) are not lost on the warm path.

**(e) WebKit reconciliation: done.** The note's WebKit column was taken in a different run from `results/webkit-all.json`; the raw JSON's figures (callouts fresh 285 / warm 99, empty 236 / 57, x10 682 / 508, instantiation 131, run 146 / 97, next render after a terminate 285 fresh / 280 warm)
replaced the differing ones in the verdict, the table, section 5, section 6 (saving 186 ms, 65%) and the Firefox summary, and in H10a's and H10b's "Why". The Chromium and Firefox end-to-end figures already matched their raw JSON.


## H10a Task 0b: the whole preview refresh (2026-10-03)

Harness: a new opt-in Playwright spec, `hub-client/e2e/pandoc-preview-measure.harness.spec.ts` (`Q2_MEASURE_F=1`, Chromium and WebKit projects), over a new E2E-only hook `pandoc.measurePdfRefresh` in `src/test-hooks.ts`. The hook builds the production
preview controller (`createPdfPreviewController`) and viewer (`mountPdfViewer`, `viewer.show`), the two objects `PdfPreviewPane` wires together, and stamps `performance.now()` at every controller stage change and when the viewer has drawn the new PDF.
Each run rewrites the document's last line (a real edit). Document: `callouts.qmd` of the R0 typst recording, format `pdf`. Median (min-max) in ms over 12 runs after one cold run, fresh path (today's production code, nothing warm in the app).
Raw JSON: `d2b/results/h10a-0b-app-chromium.json`, `h10a-0b-app-webkit.json` (stage marks of every run); the pandoc legs for the derivation come from the d2b harness run immediately after in the same browser:
`h10a-0b-d2b-chromium.json`, `h10a-0b-d2b-webkit.json` (`N=12 node drive.mjs <browser> latency`). Machine load was lower than for 0a (load average 7-18), so these figures are a little faster than 0a's.

| stage (fresh path) | Chromium | WebKit |
|---|---:|---:|
| `listFonts`: a typst worker spawn, `TypstSession.create`, font families (per run today) | 50 (49-57) | 65 (63-150) |
| Rust request build (main thread) | 5 (4-6) | 6 (5-11) |
| pandoc leg: runner spawn, mount, run, read, result message | 177 (167-197) | 320 (306-470) |
| &nbsp;&nbsp;of which spawn + instantiate + mount, before `running` | 31 (30-36) | 148 (142-163) |
| typst leg: a second worker spawn, init and compile | 66 (65-73) | 89 (84-101) |
| &nbsp;&nbsp;of which the compile itself, after `typst-compiling` | 26 (25-30) | 33 (29-41) |
| `viewer.show`: pdf.js iframe, pages laid out, first page painted | 98 (94-126) | 149 (133-165) |
| **whole refresh** | 400 (383-446) | 624 (599-780) |

The cold run (module fetch from cache and compile) takes 1275 ms in Chromium and 1646 ms in WebKit.

**Derived warm whole refresh** = fresh whole refresh minus the d2b fresh pandoc leg plus the d2b warm pandoc leg, all in the same browser and run:
Chromium 400 - 174 + 118 = **344 ms** (saving 56 ms, 14% of the refresh);
WebKit 624 - 309 + 109 = **424 ms** (saving 200 ms, 32%).
It is conservative by the per-run `listFonts` spawn that H10b's font memo removes (50 / 65 ms): without it the figures would be about 294 / 359 ms, still far above 200 ms.

**Consequence: the debounce stays 500 ms on the warm path.** The bar for 250 ms was a derived warm whole-refresh median of at most 200 ms in both browsers; it is 344 ms and 424 ms. H10b's `PDF_PREVIEW_WARM_DEBOUNCE_MS` is therefore 500, the same as today's.
What the measurement shows is that the pandoc leg is about half of the refresh in Chromium (177 of 400 ms) and half in WebKit (320 of 624 ms), and the typst leg (a second worker spawn and compile: 66 / 89 ms, of which the compile is 26 / 33 ms)
and pdf.js (98 / 149 ms) cost about three times what the warm instance saves in Chromium (164 ms against 56 ms). H10b's bar for the whole refresh (5c) therefore uses these savings: the warm whole-refresh median at most the same-run fresh median minus half of
56 ms (Chromium) or 200 ms (WebKit).

**Firefox 157 (run by Gordon's default browser through `serve-ff0.mjs`; raw JSON `d2b/results/h10a-0a-firefox.json`, 12 runs, same load caveat).**
- (a) New instance: first `convert` 138 (132-156); after one warm-up, no idle gap, 125 (120-130) against steady warm 112 (109-121): **+13.0 ms, a pass by 2 ms**. With a 500 ms idle gap before the timed `convert` the result is worse: first `convert` 119, after warm-up 141 (123-144) against steady 105: **+36 ms**, i.e. a warm-up followed by idle time did not leave the instance hot in Firefox (Chromium +4.4, WebKit +4.0 in the same variant). The plan's rule (no gap) passes; the idle case is recorded for H10b's 5c to watch.
- (b) The gate passes: plain alone 109 (101-122), older 108, newer 106 (older -1, newer -3 ms); with a concurrent typst compile and busy loop older +9, newer +2 (typst compile 75 ms); every core but one busy: alone 169, older -21, newer -1.
- (c) Spare: spawn + instantiate 18 ms, request to output 168 (132-173) against fresh 140 (135-166) in the same run: in Firefox a spare is not faster than fresh (instantiation is only 8 ms there), bytes equal.
- (d) Passes: fd 2 = `PROBE-IO-STDERR\nWARNING (f.lua:1) quarto-style warning\n`, fd 1 = `PROBE-IO-STDOUT\n`, `/stderr` empty, `/warnings` `[]`.
