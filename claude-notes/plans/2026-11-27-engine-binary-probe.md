---
title: 'Shared engine binary probe (bd-s8xe0lac)'
date: 2026-11-27
description: 'Knitr and Jupyter report a runtime available only when its binary exits 0 on `--version`, through one cached, time-limited probe.'
status: in-progress
braid:
  strand: bd-s8xe0lac
---

## Overview

`KnitrEngine::is_available` is `self.rscript_path.is_some()`: a broken
`Rscript` (stale shim, deleted R install) reports available, so a render fails
inside execution instead of at the availability gate, and knitr-gated tests
hard-fail instead of skipping. `JupyterEngine` fixed the same defect in
bd-1eu34vpy with a private probe. Rather than add a second copy, this change
extracts that probe into one helper that every engine can use.

### What "available" means

This follows the existing design (`claude-notes/designs/engine-resolution.md`
section 10; `claude-notes/research/2026-07-03-plan10-check-installation-research.md`):

- **Availability** (`is_available()`): the engine's binary is found where it is
  configured (env var) or by default (PATH), *and* it runs: a version call
  exits 0. Cheap, cached, checked after resolution and before execute.
- **Capability** (packages such as knitr/rmarkdown, a registered kernel, a
  test render): not part of availability. It fails at execute time, and the
  rich diagnosis belongs to `q2 check` (bd-4qflzhwh, bd-elhgm6ff). `q2 check`
  is not implemented yet (`commands/check.rs` returns `NotImplemented`); the
  probe is an interface it can reuse as its first tier, and that integration
  is follow-up work, not part of this change.

The probe contract is **exit status only**: output is never parsed, because
version output differs per binary (multi-line for `jupyter`, stderr on old R).
Verified 2026-11-27 on Windows: `Rscript`, `jupyter`, `julia`, `node` and
`pandoc` all exit 0 on `--version`.

## Approaches considered

- **Copy the Jupyter probe into knitr** (what the strand proposed): smallest
  diff, but a third hand-rolled copy; engines would keep drifting.
- **Shared helper, trait stays `bool`** (chosen): one probe, cached per
  resolved binary; engines opt in; no trait or caller change.
- **Shared helper returning a status** (absent / broken with reason / ok):
  would let the render error name the broken binary, but changes the trait and
  the render error path. Deferred; the helper's shape should not preclude it.

## Design

- New module `crates/quarto-core/src/engine/binary_probe.rs`, native-only
  (`#[cfg(not(target_arch = "wasm32"))]`, like `knitr` and `jupyter`).
- One function that runs a resolved path with caller-given arguments and
  returns whether it exited 0. A spawn failure or a timeout counts as not
  runnable. Arguments come from the caller, so a future runtime that needs
  another flag passes its own.
- **Status only, bounded.** stdin, stdout and stderr all go to null, and the
  deadline applies to the version process alone, so a launcher that exits 0
  reports available even if a background child it started is still running.
  The existing `child_runner::run_probe` (`crates/quarto-core/src/child_runner.rs`,
  used by the pandoc and typst version probes) is *not* used as is: it pipes
  both streams and, after the child exits, waits for them to close
  (`run_child`'s drain loop), so an inherited pipe would turn an exit 0 into a
  timeout. Instead, add a status-only sibling in `child_runner` that reuses its
  deadline (`PROBE_DEADLINE`, 30 s) and kill behaviour, without changing
  `run_child`/`run_probe` for their current callers.
- Memoized process-wide, keyed by **(path, arguments)**: the registry is rebuilt
  per document (`claude-notes/plans/2026-05-22-engine-discovery-cache.md`), so
  an uncached probe would spawn per render. The lock is not held while the
  binary runs (same rule as the current Jupyter probe).
- **Cache guarantee:** once a result is cached, later calls reuse it and never
  spawn. Concurrent first calls (project renders run documents on a rayon
  pool) may each probe before the first result is stored; that is accepted
  because the probe is idempotent and the race happens at most once per key
  per process. No per-key initialization lock. **The first stored result
  wins:** a finished probe inserts only if the key is still vacant, and every
  caller returns the cached value, so a slow probe that times out cannot
  overwrite a success another worker already stored.
- `JupyterEngine::is_available` switches to the helper; its private
  `probe_jupyter`, `probe_jupyter_cached` and `JUPYTER_PROBE_CACHE` are
  removed. The verdict is unchanged (exit 0 means available, spawn failure
  means not; all three streams stay null), with one deliberate difference: a
  hung `jupyter --version` now times out after 30 s and reports unavailable
  instead of blocking forever. Its three stub tests stay unchanged as the
  regression guard.
- `KnitrEngine::is_available` becomes: path resolved and the helper reports
  `Rscript --version` exits 0.
- The stub-binary test helper (`write_jupyter_stub`: a `.bat` file on Windows, a
  `#!/bin/sh` script with mode 0o755 elsewhere) becomes a shared test helper
  so knitr and future engines reuse it.
- Docs: `engine-resolution.md` section 10 says availability is "is the owner's
  binary on PATH"; update it to "found where configured or by default, and
  runs". Update the `KnitrEngine::is_available` doc comment to match.

### Non-goals

- The Julia and Node probes (`ts_engine.rs` `julia_is_available`,
  `quarto-node-discovery`): they already run the binary but are uncached, and
  Julia's check fires inside `ensure_loaded` instead of `is_available`. They
  become follow-up strands, not part of this change.
- Changing `is_available` to return a status, or improving the generic render
  error (P2-12).
- R package checks, or test helpers that run `requireNamespace`: these are
  capability, which `q2 check` owns.
- Rscript discovery parity with quarto-cli (`R_HOME`, Windows registry): only
  `QUARTO_R` and PATH today. A separate strand if wanted.
- The duplicate `find_rscript()` call in `call_r` (`knitr/subprocess.rs`).

### Left to the implementer

Names and signatures are fixed by the implementation plan below. Function
bodies, doc-comment wording and test-message wording are the implementer's.

# Implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** `KnitrEngine::is_available` reports a broken `Rscript` as
unavailable, through a shared, cached, time-limited exit-status probe that
`JupyterEngine` also uses.

**Architecture:** A status-only bounded runner in `child_runner` (null stdio,
deadline, kill), a cached probe module `engine/binary_probe.rs` on top of it,
and two engine `is_available` impls switched onto the probe.

**Tech stack:** Rust, `quarto-core` crate only. Tests with `cargo nextest`,
`tempfile` stubs.

**Spec:** the design sections above in this file.

## Global constraints

- Work in this worktree on branch
  `braid/bd-s8xe0lac-knitrengineisavailable-trusts-path-presence`. Strand
  `bd-s8xe0lac`; mark it `in_progress` at start
  (`braid update bd-s8xe0lac --status in_progress`).
- Windows host. Bash tool is Git Bash. Use `cargo nextest run`, never
  `cargo test`; never pipe nextest output.
- Do NOT run `cargo build --workspace`, `cargo nextest run --workspace` or
  `cargo xtask verify`. Scope every build/test to `-p quarto-core` (and
  `--bin q2` for the end-to-end task).
- **Commit gate (exception to `AGENTS.md` commit-and-continue).** `AGENTS.md`
  and `claude-notes/instructions/review.md` let approved plan work commit
  without asking only when the full workspace suite is green; this machine's
  `CLAUDE.local.md` forbids running it without asking. For this plan the gate
  is instead: `cargo nextest run -p quarto-core` scoped as each task says, plus
  `cargo clippy -p quarto-core --all-targets -- -D warnings`, both clean.
  Rationale: the only new `pub` item is the additive
  `child_runner::run_status`; everything else is `pub(crate)` or private to
  `quarto-core`, so no downstream crate can break. CI runs the workspace
  suite. Approved 2026-11-27 for this plan.
- Complete the checklist in `claude-notes/instructions/review.md` before every
  commit (the new `HashMap` cache is lookup-only and never iterated).
- Before the first build: `cargo config get build.build-dir -Zunstable-options`
  must print a value; if unset, stop and report.
- Never `#[ignore]`, skip or `#[cfg(not(windows))]`-gate a test to make it
  pass. Platform-specific test setup uses `#[cfg(windows)]` /
  `#[cfg(not(windows))]` (or `unix`) helper pairs
  (`.claude/rules/cross-platform.md`).
- Do not change `run_child` or `run_probe` behaviour; their callers are
  pandoc and typst.
- Do not touch: `ts_engine.rs`, `quarto-node-discovery`, `find_rscript`,
  `call_r`, test-only `rscript_available()` helpers elsewhere.
- A `.rs` edit triggers `cargo fmt` via hook; don't fight it.
- Commits: subject `<scope>: <what> (bd-s8xe0lac)`; write the message to a
  file and `git commit -F <file>`. Stage explicit paths. Never push.

## Review focus

Inputs the spec implies but that need explicit tests (each is assigned to a
task below):

1. A path that does not exist (spawn fails) must report not runnable, not
   panic. Test in Task 2.
2. The same binary probed with different arguments must not share a cache
   entry. Test in Task 2.
3. A Windows `.bat` stub run through the status-only runner with null stdio
   must still execute and report its exit code (the counter-file test proves
   it ran). Tests in Tasks 2-4 all use `.bat` stubs on Windows.
4. A child that exits 0 while leaving a background grandchild running must
   return promptly with success (the reason the status-only runner exists).
   Test in Task 1.
5. A child that never exits must be killed at the deadline and report
   `TimedOut`. Test in Task 1.

---

### Task 1: Status-only bounded runner in `child_runner`

**Files:**

- Modify: `crates/quarto-core/src/child_runner.rs`
- Test: `crates/quarto-core/tests/integration/child_runner.rs`

**Interfaces:**

- Produces:
  - `pub fn run_status(cmd: Command, limits: ChildLimits) -> Result<ExitStatus, ChildError>`:
    stdin, stdout, stderr all `Stdio::null()`; same `process_group(0)` on
    unix when bounded; same poll loop (`POLL`), `limits.violation(start)`
    check and `kill(&mut child, bounded)` on violation as `run_child`; no
    drain threads.
  - `pub(crate) fn run_status_probe(cmd: Command) -> Option<ExitStatus>`:
    `run_status` with `ChildLimits { deadline: Some(PROBE_DEADLINE), cancellation: None }`,
    `.ok()`.
  - Extend the module doc comment to mention the status-only variant.

This is new code, not a bug fix: the tests below are contract tests, not a
RED/GREEN cycle.

- [x] **Step 1: Write the contract tests** in
  `tests/integration/child_runner.rs`, importing `run_status`. Reuse the
  file's `script`, `emit_cmd`, `sleep_cmd` helpers.
  - `run_status_reports_exit_code`: `run_status(emit_cmd("o", "e", 3), ChildLimits::default())`
    is `Ok(status)` with `status.code() == Some(3)`; with code 0,
    `status.success()`.
  - `run_status_spawn_failure_is_io_error`: a nonexistent program
    (`Command::new("definitely-not-a-real-binary-q2")`) gives
    `Err(ChildError::Io(_))`.
  - `run_status_times_out_and_kills`: `sleep_cmd(30)` with
    `deadline: Some(Duration::from_millis(300))` gives
    `Err(ChildError::TimedOut)` and returns in under 10 s.
    This mirrors the existing `times_out_and_kills`; `kill()` reaps the
    child with `child.wait()`, so no extra liveness check.
  - `run_status_ignores_background_grandchild`: a helper pair
    `background_child_then_exit0_cmd(marker: &Path) -> Command`. The launcher
    starts a background child that writes its own PID to `marker` and then
    sleeps 30 s; the launcher waits until `marker` exists (proving the child
    started), then exits 0.
    - Unix: `script("sh -c 'echo $$ > \"M\"; exec sleep 30' & while [ ! -s \"M\" ]; do sleep 0.05; done; exit 0")`
      with `M` the marker path. The inner shell writes its own PID and
      `exec`s `sleep`, so that PID is the sleeping process.
    - Windows: `script("$p = Start-Process -PassThru -NoNewWindow powershell -ArgumentList '-NoProfile','-Command','Start-Sleep 30'; Set-Content -Path 'M' -Value $p.Id; exit 0")`.
      If `Start-Process` fails, `$p` is null and the marker holds no PID, so
      the PID assertion fails instead of passing vacuously.
    - With `deadline: Some(Duration::from_secs(20))`, assert `run_status`
      returns `Ok(status)` with `status.success()` in under 10 s, and that
      `marker` exists with a PID in it. Then kill that PID (unix
      `Command::new("kill").arg(pid)`, Windows
      `Command::new("taskkill").args(["/PID", &pid, "/T", "/F"])`), and do the
      kill before the assertions run so a failing assertion still cleans up.
- [x] **Step 2: Implement** `run_status` and `run_status_probe` in
  `child_runner.rs`.
- [x] **Step 3: Run**
  `cargo nextest run -p quarto-core -E 'binary(integration) & test(child_runner::)'`.
  Expected: all `child_runner::` tests pass, old and new.
- [x] **Step 4: No commit yet.** `run_status_probe` has no caller until Task 2
  and `binary_runs` none until Task 3, so a commit here would fail
  `clippy -D warnings` on dead code. Tasks 1 to 3 are committed together at
  the end of Task 3.

### Task 2: Cached probe module `engine/binary_probe.rs`

**Files:**

- Create: `crates/quarto-core/src/engine/binary_probe.rs`
- Modify: `crates/quarto-core/src/engine/mod.rs` (declare the module next to
  the other native-only modules:
  `#[cfg(not(target_arch = "wasm32"))] pub(crate) mod binary_probe;`)

**Interfaces:**

- Consumes: `crate::child_runner::run_status_probe` (Task 1).
- Produces:
  - `pub(crate) fn binary_runs(path: &Path, args: &[&str]) -> bool`: true
    iff the binary, run with `args`, exits 0 within the probe deadline.
    Cached process-wide in
    `static PROBE_CACHE: LazyLock<Mutex<HashMap<(PathBuf, Vec<String>), bool>>>`.
    Algorithm: lock, return the cached value if present, unlock; run the probe
    without holding the lock; lock again and return
    `*cache.entry(key).or_insert(result)` (first stored result wins).
  - `#[cfg(test)] pub(crate) fn write_stub(dir: &Path, name: &str, body: &str) -> PathBuf`:
    moved from `write_jupyter_stub` (`jupyter/mod.rs`) with the file name as
    a parameter. Windows: `<name>.bat` containing `@echo off\r\n{body}\r\n`.
    Elsewhere: `<name>` containing `#!/bin/sh\n{body}\n`, mode `0o755`.
- Module doc comment: what availability means (found + exits 0, exit status
  only, output never read), why it's cached, the first-result-wins rule.
  Point at this plan file.

New code: contract tests, not RED/GREEN.

- [x] **Step 1: Write tests** in a `#[cfg(test)] mod tests` in
  `binary_probe.rs`. Each test uses its own `tempfile::tempdir()` so cache
  keys never collide across tests.
  - `binary_runs_false_for_failing_binary`: stub body `exit /b 1` (Windows) /
    `exit 1`; `binary_runs(&stub, &["--version"])` is false.
  - `binary_runs_true_for_working_binary`: body `echo 1.0\r\nexit /b 0` /
    `echo 1.0\nexit 0`; true.
  - `binary_runs_false_for_missing_path`: `tmp.path().join("no-such-binary")`;
    false.
  - `binary_runs_reuses_cached_result`: counter-file stub (copy the pattern
    from the Jupyter test `is_available_probe_runs_at_most_once_per_path`);
    call `binary_runs(&stub, &["--version"])` three times; counter file has
    exactly 1 line.
  - `binary_runs_caches_per_argument_list`: same counter stub; call with
    `&["--version"]` twice and `&["-V"]` once; counter has exactly 2 lines.
- [x] **Step 2: Implement** `binary_runs`, `write_stub` and the module
  declaration.
- [x] **Step 3: Run** `cargo nextest run -p quarto-core -E 'test(binary_probe::)'`.
  Expected: 5 passed.
- [x] **Step 4: No commit yet** (see Task 1 Step 4).

### Task 3: Knitr availability uses the probe (the bug fix, TDD)

**Files:**

- Modify: `crates/quarto-core/src/engine/knitr/mod.rs` (`is_available` at
  ~line 500, `KnitrEngine` doc comment at ~line 158, `mod tests` at ~line 654)

**Interfaces:**

- Consumes: `crate::engine::binary_probe::{binary_runs, write_stub}` (Task 2).

- [x] **Step 1: Write the failing tests** in knitr's `mod tests`, next to
  `test_knitr_engine_is_available_depends_on_rscript`. Build engines as
  `KnitrEngine { rscript_path: Some(stub) }`, stub name `"Rscript"`.
  - `is_available_false_for_broken_rscript`: body `exit /b 1` / `exit 1`;
    assert `!engine.is_available()`.
  - `is_available_true_for_working_rscript`: body
    `echo Rscript (R) version 4.6.2\r\nexit /b 0` / `... \nexit 0`; assert
    `engine.is_available()`.
  - `is_available_probe_reused_for_same_rscript`: counter stub; two engines
    sharing the path; `is_available()` three times total; counter has exactly
    1 line.
- [x] **Step 2: Run to see RED:**
  `cargo nextest run -p quarto-core -E 'test(is_available_) & test(rscript)'`.
  Expected: `is_available_false_for_broken_rscript` FAILS (returns true
  today) and `is_available_probe_reused_for_same_rscript` FAILS (counter has
  0 lines); `is_available_true_for_working_rscript` and the existing
  `test_knitr_engine_is_available_depends_on_rscript` (also matched by the
  filter) pass. Record the output.
- [x] **Step 3: Fix** `is_available`:
  `self.rscript_path.as_deref().is_some_and(|p| binary_runs(p, &["--version"]))`,
  with a one-line comment citing bd-s8xe0lac. Update the `KnitrEngine` doc
  comment's Requirements: `Rscript` must be found (`QUARTO_R` or `PATH`) and
  run; the knitr package is checked at execute time, not by `is_available`.
- [x] **Step 4: Run to see GREEN:** same command; 4 passed (the 3 new tests
  plus the existing one).
- [x] **Step 5: Gate:** `cargo nextest run -p quarto-core -E 'test(binary_probe::) | test(is_available) | binary(integration) & test(child_runner::)'`
  and `cargo clippy -p quarto-core --all-targets -- -D warnings`, both clean.
- [x] **Step 6: Commit Tasks 1 to 3 together**
  `knitr: probe that Rscript runs, via a shared binary probe (bd-s8xe0lac)`,
  staging `child_runner.rs`, `tests/integration/child_runner.rs`,
  `engine/binary_probe.rs`, `engine/mod.rs` and `engine/knitr/mod.rs`. The
  message body names the status-only runner, the cached probe and the knitr
  fix.

### Task 4: Jupyter onto the shared probe, design doc wording

**Files:**

- Modify: `crates/quarto-core/src/engine/jupyter/mod.rs` (lines ~94-135
  probe and cache, ~373-381 `is_available`, ~727-745 `write_jupyter_stub`
  and its three callers)
- Modify: `claude-notes/designs/engine-resolution.md` (~line 801)

**Interfaces:**

- Consumes: `binary_runs`, `write_stub` (Task 2).

- [x] **Step 1: Switch** `is_available` to
  `self.jupyter_path.as_deref().is_some_and(|p| binary_runs(p, &["--version"]))`,
  keeping the bd-1eu34vpy comment. Delete `JUPYTER_PROBE_CACHE`,
  `probe_jupyter`, `probe_jupyter_cached` and `write_jupyter_stub`; the
  three tests call `write_stub(dir, "jupyter", body)` instead, with bodies and
  assertions unchanged. Remove imports that become unused (`Stdio`, `Mutex`,
  `LazyLock`, `HashMap` only if nothing else uses them; let the compiler
  decide).
- [x] **Step 2: Run**
  `cargo nextest run -p quarto-core -E 'test(jupyter::tests::is_available) | test(find_jupyter_is_memoized)'`.
  Expected: 4 passed.
- [x] **Step 3: Update** `engine-resolution.md` line ~801: availability is
  "is the owner's binary found (configured env var or `PATH`) and does it run
  (a version call exits 0, `engine/binary_probe.rs`)". Keep "Cheap; checked
  after resolution, before execute (cases 1–2)". Run
  `q2 render --strict claude-notes/designs/engine-resolution.md` and confirm
  no error mentions that file (listing errors `Q-12-13` from a partial render
  are expected and unrelated).
- [x] **Step 4: Commit** `jupyter: use the shared binary probe (bd-s8xe0lac)`.

### Task 5: Lint, end-to-end check, follow-ups

**Files:** none in the repo, except this plan's checkboxes. Scratch files go
in a temp dir outside the repo.

- [x] **Step 1: Lint:** `cargo clippy -p quarto-core --all-targets -- -D warnings`.
  Expected: no warnings. Then
  `cargo nextest run -p quarto-core -E 'test(binary_probe::) | test(is_available) | test(child_runner::)'`;
  all pass.
- [x] **Step 2: Build** `cargo build --bin q2` (the `q2` on `PATH` is an
  installed build and does not include this change).
- [x] **Step 3: Broken-Rscript render.** In a temp dir, write `doc.qmd`:

  ````markdown
  ---
  engine: knitr
  ---

  ```{r}
  1 + 1
  ```
  ````

  Copy `C:\Windows\System32\where.exe` to `<tmp>\badr\Rscript.exe` (exits 1
  on `--version`). From PowerShell:
  `$env:QUARTO_R = "<tmp>\badr\Rscript.exe"; cargo run --bin q2 -- render <tmp>\doc.qmd`.
  Expected: render fails with
  `Engine 'knitr' is registered but its runtime is not available.` Record
  the command and the error line.
- [x] **Step 4: Working render.** New PowerShell process with `QUARTO_R`
  unset: `cargo run --bin q2 -- render <tmp>\doc.qmd`. Expected: success, and
  the output HTML contains `[1] 2`. Record the command and the grep result.
- [x] **Step 5: File follow-up strands** (both
  `--deps discovered-from:bd-s8xe0lac`, type `task`, priority 3, with a
  self-contained `-d`):
  - "Julia and Node availability probes onto engine::binary_probe":
    `julia_is_available` (`ts_engine.rs`) and `quarto-node-discovery` spawn
    uncached; Julia's check runs in `ensure_loaded`, not `is_available`.
  - "Rscript discovery parity with quarto-cli (R_HOME, Windows registry)":
    `find_rscript` checks only `QUARTO_R` and `PATH`.
- [x] **Step 6: Tick this plan's boxes**, run
  `q2 render --strict claude-notes/plans/2026-11-27-engine-binary-probe.md`
  (no error may mention this file), commit the plan
  `claude-notes: tick engine binary probe plan (bd-s8xe0lac)`, and add a
  `braid comment bd-s8xe0lac` summarising the commits and the end-to-end
  result. Do not close the strand or push; report back.

### Verification results (2026-11-27, Windows)

- Step 3: with `QUARTO_R` pointing at a copy of `where.exe` named
  `Rscript.exe`, `cargo run --bin q2 -- render doc.qmd` failed with
  `Engine 'knitr' is registered but its runtime is not available.`
- Step 4: with `QUARTO_R` unset, the same render succeeded; the output HTML
  was inspected and contains `[1] 2`.
- Step 5: follow-ups filed as bd-og2exj8x (Julia and Node probes) and
  bd-itbhfipu (Rscript discovery parity).

## Follow-up: probe in the execution directory

Review found that the probe ran from q2's launch directory while knitr runs
Rscript from `determine_working_dir` (the project directory, or the document
directory inside an active renv). A version-manager shim that picks R from
the working directory then failed the gate for a render launched from the
project's parent, and the failure was cached for the process.

- [x] `ExecutionEngine::is_available_for(document, project_dir)`, defaulting
  to `is_available()`; the `engine_execution` gate calls it.
- [x] `binary_runs` takes an optional `cwd`, part of the cache key. Knitr
  overrides `is_available_for` to probe from `determine_working_dir`. Jupyter
  passes `None`: its kernels start from the kernelspec argv, not from
  `jupyter`, so the probe directory has no execution counterpart.
- [x] Tests: `binary_runs_probes_and_caches_per_cwd`,
  `is_available_for_probes_in_the_execution_directory`.
- [x] End to end (Windows): an `Rscript.bat` first on `PATH` that exits 1
  unless `_quarto.yml` is in its working directory, else runs the real
  `Rscript.exe`. `q2 render proj\doc.qmd` from the parent failed before the
  change (`Engine 'knitr' is registered but its runtime is not available.`)
  and succeeds after it; the HTML was inspected and contains `[1] 2`. A
  `QUARTO_R` pointing at a copy of `where.exe` still fails at the gate.

## Follow-up: probe with the project environment

Execution adds the project `_environment` pairs (`env_for_subprocess` plus
`QUARTO_PROFILE`) to Rscript's environment; the probe did not, so a shim
pinned through `_environment` failed the gate although it ran at execute
time.

- [x] `engine_project_env(ctx)` builds those pairs once for both the gate
  and `ExecutionContext::with_project_env`. `is_available_for` and
  `binary_runs` take them, and they are part of the probe cache key. The
  per-document `execute_env` is left out: it differs per document and would
  turn the per-process cache into a probe per render.
- [x] Tests: `binary_runs_probes_and_caches_per_env`,
  `is_available_for_probes_with_the_project_env`, and
  `availability_gate_receives_the_project_env` (through
  `EngineExecutionStage::run`; it fails when the gate passes no
  environment).
- [x] End to end (Windows): an `Rscript.bat` first on `PATH` that exits 1
  unless `Q2_R_PIN` is set, else runs the real `Rscript.exe`, and a project
  `_environment` with `Q2_R_PIN=1`. `q2 render doc.qmd` inside the project
  failed before the change (`Engine 'knitr' is registered but its runtime
  is not available.`; the 0.48.0 nightly without the probe rendered it) and
  succeeds after it; the HTML was inspected and contains `[1] "1"`.
