# Plan 1a-host bugs — q2-introduced defects in the landed engine host

**Status:** ready to execute. **Created:** 2026-06-26 (carved out of `plan1a-return-to-q1`).
**Branch:** `feature/ts-engine-extensions`. **Touches:** `crates/quarto-core/src/engine/ts_process.rs`.
**Sequence:** **independent — no dependencies** (not on Item A, 1b, 1c). 1a-host-layer maintenance on
already-landed code; can land **first / in parallel with everything**. Low conflict risk with Item A
(different functions: `stderr_loop`/`extract` vs `launch_engine`/`Init`).

These are **q2-introduced bugs, not Q1 regressions** (no Q1 analogue) — which is why they live in
their own plan rather than the return-to-Q1 plan. Both are **code + test**, with frozen named-revert
seams (TDD: write the seam, see RED, fix, GREEN, run the workspace suite).

---

## HOST-1 — crash stderr is shared; honest labeling + ring hygiene (not partitioning)

**Severity:** Low-Med. **Verified 2026-06-26 against `ts_process.rs`.**

stderr is consulted on **exactly one event — a whole-subprocess crash** (`handle_crash` ~L877-908).
Per-request failures return `FromEngine::Error{message,stack}` routed by id to one slot
(`reader_loop` ~L820-828) and **never touch the ring** — so a single engine's failure already sees
only its own structured error. A crash is global (every in-flight request fails at once); today each
waiter gets an identical copy of the whole `recent_stderr` ring (~L395) stamped with its own engine
name. Per-engine *partitioning* is impossible (lines are untagged) **and undesirable** (it would
hide the culprit if it wasn't the engine you waited on). So:

- [ ] **Honest crash label:** when `>1` slot is in flight, label the snapshot "recent subprocess
  stderr (shared across in-flight engines: [...])" + roster; keep today's single-engine framing when
  exactly one is in flight.
- [ ] **Ring hygiene:** `stderr_loop` must **not** push `[INFO]` lines into `recent_stderr` (trace
  them only); ring keeps `[WARN]`/`[ERROR]`/unprefixed — so an env-enabled INFO toggle never degrades
  the crash diagnostic.
- [ ] **Seam-enabling refactor (must land with the test):** `stderr_loop(stderr: ChildStderr, …)` →
  `stderr_loop(reader: impl BufRead, …)`. The concrete `ChildStderr` (~L917) forces a subprocess
  harness and makes the ring un-assertable in a unit; `impl BufRead` lets the test feed a
  `Cursor<&[u8]>`. The live caller already wraps `BufReader::new(child_stderr)` — production unchanged.

*Residual pollution (accepted):* by default only WARN/ERROR reach stderr, so a chatty notebook's
**warnings** from engine B can still appear in engine A's crash error — notebook-author hygiene,
rare. INFO is off unless env-enabled (and now never rings).

### Test Seam Spec — HOST-1

| # | Test | Tier | Real unit | Seam | Named revert → RED |
|---|---|---|---|---|---|
| H1-a | crash names shared roster (>1 in flight) | Rust unit | `handle_crash` | pre-fill `recent_stderr` with `"[WARN] from beta"`; `pending` = 2 slots (`alpha`,`beta`, each `sync_channel(1)`); `child=None`; call → recv both → assert each `ProcessCrashed.stderr` contains `"shared"` + both `alpha`/`beta` | revert the `engines.len()>1` branch → alpha-slot `stderr.contains("alpha")` RED (raw ring names only `beta`) |
| H1-b | single in-flight keeps clean attribution | Rust unit | `handle_crash` | 1 slot (`solo`); ring `"[WARN] x"`; assert stderr does **not** contain `"shared"` | make `>1` framing unconditional → `!contains("shared across")` RED |
| H1-c | INFO not ringed; WARN/ERROR/bare are | Rust unit | `stderr_loop` (post-refactor, `impl BufRead`) | `stderr_loop(Cursor::new(b"[INFO] hello\n[WARN] careful\n[ERROR] boom\nbare\n"), ring)`; after EOF assert ring **excludes** `hello`, **includes** `careful`/`boom`/`bare` | revert "skip INFO" guard → `!ring.contains("hello")` RED (include-asserts guard the over-broad "drop all prefixed" fix) |

*Accepted-untested:* INFO still routed to `tracing` (diff only relocates the ring `push`; the `info!` arm is untouched).

---

## HOST-2 — bundle-extraction failures are cached permanently

**Severity:** Low (CLI) / Low-Med (long-running `q2 preview`/hub). **Verified 2026-06-26.**

`static EXTRACTED_BUNDLE_PATH: Mutex<Option<Result<PathBuf, String>>>` (~L63) caches `Err`
permanently (the comment says "until the process restarts", `extracted_bundle_path` ~L114-143). A
transient disk-full / missing-runtime-dir blip then **permanently disables all TS engines** in a
long-running process. Fix = cache successes only.

- [ ] Change the static to `Mutex<Option<PathBuf>>`; factor a testable
  `cached_extract(cache: &Mutex<Option<PathBuf>>, extract: impl FnOnce() -> Result<PathBuf,
  ExecutionError>)` that returns early on `Err` (never caches it), `get_or_insert`s on `Ok`
  (race-safe: `extract_bundle_to` is idempotent write-if-absent), and releases the lock during
  extraction.
- [ ] Delete the "errors are also cached … until restart" doc-comment.

### Test Seam Spec — HOST-2

| # | Test | Tier | Real unit | Seam | Named revert → RED |
|---|---|---|---|---|---|
| H2 | Err not cached; Ok is; hit skips extractor | Rust unit | `cached_extract` (new) | local `Mutex<Option<PathBuf>>` + `AtomicUsize`. call 1 `Err` → `is_err()` **and cache `None`**; call 2 `Ok("/x")` → `Ok("/x")` **and count==2**; call 3 `Ok("/other")` → `Ok("/x")` **and count==2** (hit) | revert to caching `Err` → after call 1, call 2 returns cached Err & never runs extractor → `r2.is_err()` **and count==1** RED |

*Accepted-untested:* `get_or_insert` two-thread race (nondeterministic; std guarantee + idempotent extract).

---

## Verification

Per CLAUDE.md: write each seam, confirm RED, fix, confirm GREEN, then `cargo nextest run --workspace`
(monorepo — `ts_process.rs` is in `quarto-core`, depended on by `wasm-quarto-hub-client`; run
`cargo xtask verify --skip-hub-build` at minimum).
