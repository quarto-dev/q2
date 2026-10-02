# D2(b) spike: one warm pandoc.wasm instance across renders (2026-10-02)

**Verdict: a warm instance works and is worth building for the PDF preview only.** It runs through the module's exported `hs_init_with_rtsopts` and `convert`
(not `_start`), is correct across renders and its memory is flat. It saves **55 ms in Chromium, 41 ms in Firefox and 187 ms in WebKit** per render, and the pandoc leg stays at
**about 105 ms** in all three (callouts.qmd to typst). It cannot get near the ~20 ms that would make typst.ts `incr_compile` worth revisiting. It costs a failure mode (a poisoned
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
  reactor-style instance: init once (24 ms Chromium, ~140 ms WebKit, i.e. the same as instantiation), then `convert(optionsJson)` any number of times.
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
| **callouts** | end-to-end | **160** | **105** | **291** | **104** |
| | instance creation | 24 | 0 (30 once) | 133 | 0 (165 once, incl. worker spawn) |
| | mount (252 files, fresh tree) | 0 | 0 | 1 | 0 |
| | pandoc run | 130 | 103 | 147 | 102 |
| | output read | 0 | 0 | 0 | 0 |
| | worker spawn + message cloning | 6.7 | 0.8 | 11 | 1 |
| **callouts-x10** | end-to-end | 600 | 561 | 684 | 508 |
| | pandoc run | 568 | 560 | 535 | 506 |
| **empty** (floor) | end-to-end | 104 | 60 | 251 | 59 |
| | pandoc run | 74 | 58 | 105 | 57 |

(Node/V8 without a worker: callouts 207 vs 141 ms, x10 836 vs 756 ms.) Outputs are byte-identical fresh vs warm in every run (SHA-256 prefix compared).

Reading it: warm removes instantiation (24 ms Chromium, ~133 ms WebKit, whose instantiate is slow) and the worker spawn, plus ~25 ms of per-run
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
| fresh path (today): ordinary render | ~150 ms | ~303 ms |
| warm path: new worker + instantiate + first `convert` | ~154 ms | ~310 ms |

So a policy that terminates the worker whenever a newer render supersedes it pays the cold cost on every superseded render and gets none of the warm benefit. H10b therefore does not
terminate on supersede: the old render keeps running, the newest request starts on a second warm worker, a pending request is replaced by a newer one (latest wins), and the pane applies
results in request order. A worker is terminated only on the wall timeout, by an adaptive grace rule for a superseded runaway, when poisoned or oversized, on a reboot, or when idle.
The preview pane debounces edits by 500 ms today (250 ms on the warm path if H10a Task 0(f) allows), so renders overlap only when the pandoc leg is longer than the debounce, which means large documents.

## 6. Net effect on the per-edit PDF preview

| | Chromium | WebKit |
|---|---:|---:|
| today: pandoc (fresh) + typst 2-5 ms | ~163-165 | ~294-296 |
| warm: pandoc (warm) + typst 2-5 ms | ~107-110 | ~106-109 |
| saving | ~55 ms (34%) | ~187 ms (63%) |

Only in WebKit is the saving large, and it comes from skipping its slow instantiation, which a pre-spawned spare fresh instance would also hide with no Lua preamble, no argv-to-defaults
translation and no poisoned-instance handling (H10a Task 0(c) measures that alternative). **`incr_compile` stays "no"**: its ~4 ms saving would be 4% of a ~105 ms pandoc leg, and the leg's floor is ~58 ms even for an empty
document. The H9 verdict in `2026-10-02-incr-compile-spike.md` is confirmed, not revisited.

## Firefox 157 (by hand, default browser; `d2b/serve-ff.mjs` + `ff.html`, raw JSON in `d2b/results/firefox-all.json`)

Same suite, median of 12 after a discarded run. callouts: **fresh 152 ms, warm 111 ms** (saving 41 ms); x10: 566 vs 529; empty (floor): 125 vs 63. Instantiation is cheap
here (8 ms), so warm only removes worker spawn/clone (13 ms) and per-run start-up. Correctness is identical to Chromium/WebKit: A,B,A,C,A,B byte-equal; the preamble env probe gives
`one|111`, `two|222`, `one|111`; recoverable errors leave the instance usable; `os.exit` throws but the instance survives; heap exhaustion (`-M100m`) poisons it for good (exit 1 forever;
fresh gives 251). Wasm memory flat over 100 renders (44 MB warm, 47 MB fresh; RSS not sampled). After a terminate, the next render is ~145 ms in both paths (no warm benefit).
So the saving is 41 ms (Firefox), 55 ms (Chromium) and 187 ms (WebKit), and the warm pandoc leg is 104-111 ms in all three browsers.

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
- The WebKit column above was taken in a different run from `d2b/results/webkit-all.json` (raw: callouts fresh 285, warm 99; empty fresh 236, warm 57; next render after terminate 285); H10a Task 0(e) replaces the differing figures.
