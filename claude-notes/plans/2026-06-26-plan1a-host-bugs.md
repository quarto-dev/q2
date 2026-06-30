# Plan 1a-host bugs — q2-introduced defects in the landed engine host

**Status:** ready to execute. **Created:** 2026-06-26 (carved out of `plan1a-return-to-q1`).
**Branch:** `feature/ts-engine-extensions`. **Touches:** `crates/quarto-core/src/engine/ts_process.rs`.
**Sequence:** **independent — no dependencies** (not on Item A, 1b, 1c). 1a-host-layer maintenance on
already-landed code; can land **first / in parallel with everything**. Low conflict risk with Item A
(different functions: `stderr_loop`/`extract` vs `launch_engine`/`Init`).

**Merge target:** the `feature/ts-engine-extensions` integration line — **not** `main`. `ts_process.rs`
does not yet exist on `main` (the whole file is new on this branch), so there is no standalone-to-`main`
option. "Independent / land first" means independent of Items A/1b/1c, which all share this integration
line; these two fixes merge into it like every other sub-task.

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
hide the culprit if it wasn't the engine you waited on).

**The defect is in the error *contract*, not in rendered output:** `ProcessCrashed { engine, stderr }`
(`error.rs:112-122`) pairs a specific `engine` name with a stderr tail that may have originated from a
*different* concurrent engine — a false attribution that can send a debugging user down the wrong path.
The fix makes the shared-ness explicit so no single engine is falsely blamed. So:

- [x] **Honest crash label:** when `>1` slot is in flight, prefix the snapshot with "recent subprocess
  stderr (shared across in-flight engines: [...]):" + a sorted, deduplicated roster; when exactly one
  slot is in flight, emit the bare ring join (no header) — unchanged from today.
- [x] **Ring hygiene:** `stderr_loop` must **not** push `[INFO]` lines into `recent_stderr` (trace
  them only); ring keeps `[WARN]`/`[ERROR]`/unprefixed — so an env-enabled INFO toggle never degrades
  the crash diagnostic.
- [x] **Seam-enabling refactor (must land with the test):** `stderr_loop(stderr: ChildStderr, …)` →
  `stderr_loop(reader: impl BufRead, …)`. The concrete `ChildStderr` (~L931) forces a subprocess
  harness and makes the ring un-assertable in a unit; `impl BufRead` lets the test feed a
  `Cursor<&[u8]>`. The `BufReader` wrap currently lives *inside* `stderr_loop` (`let reader =
  BufReader::new(stderr)`, ~L932); the refactor moves that one wrap to the call site
  (`stderr_loop(BufReader::new(stderr), recent_stderr)`, ~L539) and drops it from the body — a
  one-line, behavior-identical production change. `BufRead`/`BufReader` are already imported (L26).

*Residual pollution (accepted):* by default only WARN/ERROR reach stderr, so a chatty notebook's
**warnings** from engine B can still appear in engine A's crash error — notebook-author hygiene,
rare. INFO is off unless env-enabled (and now never rings).

*Implementation note — roster ordering in `handle_crash` (~L892):* the in-flight engine names are only
known after `pending` is drained (currently ~L918, *after* the snapshot at ~L912). **Drain first**, then
build the roster from the drained `PendingSlot.engine` fields (`PendingSlot { engine: String, tx: … }`,
L363-367), then assemble the label, then send. Reordering drain-before-snapshot is safe (no dependency).
Roster is `sort_unstable` + `dedup` so two in-flight requests to the *same* engine collapse to one entry
(H1-a uses two distinct names, so the dedup/order is an explicit decision the seam doesn't pin down).

*Implementation note — drain-grace sleep (~L909):* `handle_crash` sleeps ~250 ms unconditionally to let
the stderr thread drain. H1-a/H1-b call `handle_crash` directly and each pay it (~0.5 s total). Accepted:
the existing `test_crash_broadcast_on_mock_eof` already eats this once under a 15 s watchdog. Do **not**
parameterize the grace to `Duration::ZERO` for tests unless it becomes a real friction point — keeping
the production path untouched is worth the half-second.

### Test Seam Spec — HOST-1

All three are pure-logic Rust units (ring manipulation + label formatting) — jsdom-free, no
subprocess/browser tier. `Cursor<&[u8]>` is `impl BufRead`, so H1-c drives the real post-refactor
`stderr_loop` with no child process.

| # | Test | Tier | Real unit | Seam | Named revert → RED |
|---|---|---|---|---|---|
| H1-a | crash names shared roster (>1 in flight) | Rust unit | `handle_crash` | pre-fill `recent_stderr` with **engine-name-free** `"[WARN] glitch"`; `pending` = 2 slots (`alpha`,`beta`, each `sync_channel(1)`); `child=None`; call → recv on **both** slots → assert each `ProcessCrashed.stderr` contains `"shared"`, `"alpha"`, **and** `"glitch"` | remove the **`slots.len()>1`** label branch → both names vanish from `stderr` → `stderr.contains("alpha")` RED (raw ring is `"glitch"`; names live only in the roster). *Discriminators = `alpha`/`shared` (absent from the ring); `glitch` is the path-exercised assertion — proves the snapshot is still attached.* |
| H1-b | single in-flight keeps clean attribution | Rust unit | `handle_crash` | 1 slot (`solo`); ring `"[WARN] x"`; assert stderr **does not** contain `"shared"` **and does** contain `"x"` | drop the **`slots.len()>1`** guard (label unconditional) → `!stderr.contains("shared across")` RED. *The `contains("x")` co-assertion is the path-exercised check — it reddens a fix that returns empty/contentless stderr for the solo path (which would otherwise pass the negative-only assertion).* |
| H1-c | INFO not ringed; WARN/ERROR/bare are | Rust unit | `stderr_loop` (post-refactor, `impl BufRead`) | `stderr_loop(Cursor::new(b"[INFO] hello\n[WARN] careful\n[ERROR] boom\nbare\n"), ring)`; after EOF assert ring **excludes** `hello`, **includes** `careful`/`boom`/`bare` | **(1)** remove the `[INFO]`-skip guard → `!ring.contains("hello")` RED. **(2)** broaden the guard to skip *all* `[`-prefixed lines → `ring.contains("careful")` RED. **(3)** key the skip on info-level *routing* rather than the literal `[INFO]` prefix (so bare lines — which also `info!` — get dropped) → `ring.contains("bare")` RED. |

*Accepted-untested (HOST-1):*
- INFO still routed to `tracing` — the diff only relocates the ring `push`; the `info!` arm is untouched.
- **L539 caller wrap** `stderr_loop(BufReader::new(stderr), …)` — exercised only by real-subprocess
  runs (mock transport passes `stderr=None`, L537; `test_crash_broadcast_on_mock_eof` seeds the ring
  directly and never spawns `stderr_loop`). Compilation pins the type; the `BufReader` wrap is merely
  relocated, behavior-identical — so no unit binds it and that is accepted.
- **Roster dedup/sort** — `sort_unstable`+`dedup` guards cosmetic roster tidiness (duplicate in-flight
  requests to one engine collapse to one entry); H1-a uses distinct names and does not bind it.

---

## HOST-2 — bundle-extraction failures are cached permanently

**Severity:** Low (CLI) / Low-Med (long-running `q2 preview`/hub). **Verified 2026-06-26.**

`static EXTRACTED_BUNDLE_PATH: Mutex<Option<Result<PathBuf, String>>>` (~L63) caches `Err`
permanently (the comment says "until the process restarts", `extracted_bundle_path` ~L114-143). A
transient disk-full / missing-runtime-dir blip then **permanently disables all TS engines** in a
long-running process. Fix = cache successes only.

- [x] Change the static to `Mutex<Option<PathBuf>>`; factor a testable
  `cached_extract(cache: &Mutex<Option<PathBuf>>, extract: impl FnOnce() -> Result<PathBuf,
  ExecutionError>)`. **The double-checked-lock ordering is load-bearing** and must preserve what
  `extracted_bundle_path` already does (L116-142): (1) fast path — lock, return a clone if `Some`,
  drop the guard; (2) call `extract()` with the **lock released**, `?`-returning early on `Err` so it
  is **never** cached; (3) re-acquire the lock and `get_or_insert(path)` the *already-computed*
  `PathBuf` — the **eager** `get_or_insert`, **not** `get_or_insert_with` (which would run I/O under
  the guard). Race-safe because `extract_bundle_to` is idempotent write-if-absent and the final insert
  is double-checked. Sketch:

  ```rust
  fn cached_extract(
      cache: &Mutex<Option<PathBuf>>,
      extract: impl FnOnce() -> Result<PathBuf, ExecutionError>,
  ) -> Result<PathBuf, ExecutionError> {
      {
          let guard = cache.lock().unwrap();
          if let Some(ref path) = *guard {
              return Ok(path.clone());
          }
      }                                    // guard dropped — lock released for extract()
      let path = extract()?;               // Err returns early, never cached
      let mut guard = cache.lock().unwrap();
      Ok(guard.get_or_insert(path).clone())
  }
  ```
- [x] Delete the "errors are also cached … until restart" doc-comment.

### Test Seam Spec — HOST-2

Pure-logic Rust unit — the injected `extract` closure removes all disk/runtime-dir dependence, so no
filesystem or subprocess tier is needed.

| # | Test | Tier | Real unit | Seam | Named revert → RED |
|---|---|---|---|---|---|
| H2 | Err not cached; Ok is; hit skips extractor | Rust unit | `cached_extract` (new) | local `Mutex<Option<PathBuf>>` + `AtomicUsize` (closure increments per call). call 1 → closure `Err` → assert `r1.is_err()` **and** `cache.lock()` is `None`; call 2 → closure `Ok("/x")` → assert `Ok("/x")` **and** `count==2`; call 3 → closure `Ok("/other")` → assert `Ok("/x")` (cache served, **not** `/other`) **and** `count==2` (extractor skipped) | revert to caching `Err` (drop the `?`-early-return; insert on `Err` too) → after call 1 the cache holds `Some(Err)`, so call 2 returns the cached Err and never runs the extractor → `r2.is_err()` **and** `count==1` RED. *Discriminators: call 2's `count==2` (Err forced a re-extract) and call 3's `/x`≠`/other` + `count==2` (success cached, extractor skipped).* |

*Accepted-untested (HOST-2):*
- `get_or_insert` two-thread race — nondeterministic; relies on the std `Mutex` guarantee + idempotent
  `extract_bundle_to` (write-if-absent).
- **`extracted_bundle_path` → `cached_extract` wiring** — the static type change
  (`Mutex<Option<Result<…>>>` → `Mutex<Option<PathBuf>>`) plus the extraction closure. The
  error-not-cached *logic* is fully bound in H2 via the injected closure + local mutex; the real static
  path hits `quarto_runtime_dir()` + disk I/O whose transient-failure injection is impractical, and is
  covered by compilation + the existing engine-launch integration tests.

---

## Verification

Per CLAUDE.md: write each seam, confirm RED, fix, confirm GREEN, then `cargo nextest run --workspace`
(monorepo — `ts_process.rs` is in `quarto-core`, depended on by `wasm-quarto-hub-client`; run
`cargo xtask verify --skip-hub-build` at minimum).

---

## Status: COMPLETE (2026-06-30)

Both tasks landed on `feature/ts-engine-extensions` via subagent-driven-development
(sonnet implementers + sonnet per-task reviews; opus final whole-branch review).

| Task | Commit | Review |
|---|---|---|
| HOST-1 (honest crash label + INFO ring hygiene + `stderr_loop` `impl BufRead` refactor) | `e043783a4` | Spec ✅ / Quality Approved |
| HOST-2 (cache only successful bundle extraction) | `fc56e8c31` | Spec ✅ / Quality Approved |
| Fix wave (Minors: dup comment line; direct `Ordering` import) | `23a0d9e5e` | from final review |

**Final whole-branch review (opus, `f62c2cd89..fc56e8c31`): READY TO MERGE — yes.**
No Critical/Important. Confirmed: `cached_extract` lock ordering (no lock held across
`extract()`, Err never cached, eager `get_or_insert`); crash-path drain-before-snapshot
is dependency-free; all four seam tests bind their named reverts (non-vacuous). Noted
`ts_process.rs` is doubly `cfg(not(wasm32))`-gated, so the new `std::thread`/`BufRead`
usage never reaches a WASM build.

Seam tests added (all RED→GREEN demonstrated via named revert):
`test_crash_shared_roster_label` (H1-a), `test_crash_single_slot_no_shared_label` (H1-b),
`test_stderr_loop_info_not_ringed` (H1-c), `test_cached_extract_err_not_cached_ok_is_and_hit_skips` (H2).

**Pre-existing flaky test observed (NOT a regression):**
`quarto-core engine::ts_engine::tests::test_race_free_instance_exclusive` hit its 15 s
watchdog once during a full concurrent `nextest --workspace` run, but passes deterministically
in isolation (5/5 at ~0.29 s each). It uses the `with_transport` mock path (`stderr=None`,
no bundle extraction), which this plan's diff does not touch. Load-induced timing flakiness
in a polling-watcher race test — a candidate digression/strand for the epic, out of scope here.

Not pushed (awaiting user permission per GIT PUSH POLICY).
