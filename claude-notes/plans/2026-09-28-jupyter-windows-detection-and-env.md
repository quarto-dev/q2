# Plan: Jupyter detection and test environment (q2)

## Overview

Failing test `quarto::integration jupyter_kernel_cleanup_e2e::render_leaves_no_kernel_behind`
on Windows (investigation 2026-09-28, bd-ce4fftg8) surfaced a chain of gaps in
how q2 detects and provisions Jupyter:

1. `JupyterEngine::is_available()` (`crates/quarto-core/src/engine/jupyter/mod.rs:234`)
   is `self.jupyter_path.is_some()` — binary *presence* only — so a broken
   jupyter (e.g. a stale shim trampoline pointing at a deleted env) reports
   "available", and jupyter-gated tests hard-fail instead of skipping.
2. Test guards check engine availability but not whether the specific
   kernelspec the fixture needs (`python3`) is registered.
3. `q2 check` is a stub (`crates/quarto/src/commands/check.rs` returns
   `NotImplemented`); nothing distinguishes absent / broken / no-kernel
   jupyter for users.
4. CI never provisions Python (bd-z09fx92g), so every jupyter-gated test
   skips forever there.

Tracked under epic **bd-t0wpbq59**. Machine-specific developer setup is
deliberately *not* in this file (it is per-machine, not project knowledge).

## Work split (mirrors braid relations)

| Strand | Scope | State |
|--------|-------|-------|
| bd-1eu34vpy | `is_available()` must probe a working jupyter | **Now** |
| bd-ce4fftg8 | cleanup-e2e guard: skip on missing `python3` kernel | **Now** |
| bd-zvf3e8n2 | capture-splice guard: same hardening | **Now** |
| bd-elhgm6ff | `q2 check jupyter` health check | Later — blocked by bd-4qflzhwh |
| bd-ek7njozm | CI provisioning strategy decision | Later — design/team discussion |
| bd-z09fx92g | CI python provisioning implementation | Later — blocked by bd-ek7njozm |

`braid ready` reflects this: the two Later implementation strands are gated by
their respective blockers.

## Phase 1 (now) — Honest detection, TDD

- [x] **1. Fix `JupyterEngine::is_available()` (bd-1eu34vpy)** to probe that
      jupyter actually runs (e.g. `jupyter --version` exits 0), not just that
      the name resolves on PATH. TDD: stub-binary tests (a jupyter stub that
      exits non-zero must report unavailable), mirroring the `.bat` stub
      pattern used elsewhere in the suite. The contract is exit status only:
      stdout is not parsed, because `jupyter --version` output differs
      across versions (a bare version string on older jupyter_core, a
      package table on newer) and the observed stale-shim failure already
      exits non-zero.
      Cache the probe result per path so repeated engine-registry calls
      don't re-spawn. Effect on any machine
      with a broken jupyter: engine reports unavailable → jupyter tests take
      their existing skip path instead of hard-failing.
- [x] **2. Harden test skip guards (bd-ce4fftg8, bd-zvf3e8n2)** to also probe
      the `python3` kernelspec (via `find_kernelspec`/`list_kernelspecs`) and
      `eprintln!`-skip on `KernelspecNotFound`. Still needed after item 1: a
      machine with working jupyter but only non-Python kernels (R/Julia-only
      users) is a legitimate setup that should skip, not fail.
      **Validated 2026-09-28 both directions** (Git Bash, post-PATH-refresh
      shell): real path — `render_leaves_no_kernel_behind` executes in 14.95s,
      `jupyter_capture_splices_into_preview_ast` in 5.44s, knitr leg 3.18s, no
      skip notes; skip path (stub jupyter `--version`-only + redirected
      `APPDATA`/`JUPYTER_PATH`) — both jupyter tests skip in <0.4s with
      `KernelspecNotFound` naming the redirected temp dirs and `available
      kernels: (none)`, knitr leg still runs for real (1.2s).

### Skip-path validation script (item 2)

Run from the repo root in **Git Bash** (a shell started after
2026-09-28). A stub jupyter passes the bd-1eu34vpy availability probe
(`--version` exits 0) but fails everything else, and `APPDATA` /
`JUPYTER_PATH` / `PROGRAMDATA` are redirected to empty dirs — the
three roots of runtimelib's static search on Windows (user, env,
system; `dirs.rs` `data_dirs`) — so it finds no kernels. The stub's
failing `--paths` adds none. Both jupyter-gated tests must
`eprintln!`-skip with `KernelspecNotFound`, not fail.

```bash
#!/usr/bin/env bash
set -euo pipefail

SIM="$(mktemp -d)"
trap 'rm -rf "$SIM"' EXIT
mkdir -p "$SIM/bin" "$SIM/appdata" "$SIM/jpath" "$SIM/programdata"

# Stub jupyter: answers `--version` (engine stays available via the
# bd-1eu34vpy probe), fails `--paths` and everything else.
cat > "$SIM/bin/jupyter.bat" <<'BAT'
@echo off
if "%~1"=="--version" (
  echo 5.7.0
  exit /b 0
)
exit /b 1
BAT

# MSYS converts $SIM/bin in PATH when spawning the Windows test
# binaries; APPDATA/JUPYTER_PATH/PROGRAMDATA are read by the Rust
# process directly, so they must be Windows-style paths. Git Bash
# spells the system var `ProgramData`; exporting `PROGRAMDATA`
# adds a second entry the child ignores, so reuse the existing name.
export PATH="$SIM/bin:$PATH"
export APPDATA="$(cygpath -w "$SIM/appdata")"
export JUPYTER_PATH="$(cygpath -w "$SIM/jpath")"
export ProgramData="$(cygpath -w "$SIM/programdata")"

cargo nextest run -p quarto --test integration jupyter_kernel_cleanup --no-capture
cargo nextest run -p quarto-core --test integration capture_splice_engines --no-capture
```

Expected: `render_leaves_no_kernel_behind` and
`jupyter_capture_splices_into_preview_ast` each pass in well under a
second with `Skipping test: kernelspec 'python3' not found` (searched
dirs show the redirected APPDATA, available kernels list the stubbed
environment's emptiness); the knitr leg of `capture_splice_engines`
still runs for real (~5s) and passes. Sanity check in the other
direction: the same two commands without the env overrides must
*execute* the jupyter legs (seconds, not milliseconds) on a machine
with working jupyter + `python3` kernelspec.

## Phase 2 (later) — `q2 check jupyter` (bd-elhgm6ff)

State of `q2 check` (verified 2026-09-28): the command **exists on main but is
a bare stub** — `crates/quarto/src/commands/check.rs` returns
`QuartoError::NotImplemented`, dispatched from `main.rs:1481`. Gordon's
bd-4qflzhwh (wire TS-engine `checkInstallation` into the engine protocol +
`q2 check`) is the strand that would create the real dispatch structure, but
it is still **open** and its branch is **not on the remote** — the "ready for
execution" note in bd-elhgm6ff appears stale. We are *adding to a feature
that is specified but not yet landed*, not creating a new command.

- [ ] **3. `q2 check jupyter`**: probe absent / broken / no-kernel distinctly,
      reusing Phase 1's probe logic. Sequencing decision at execution time:
      (a) if bd-4qflzhwh has landed, slot jupyter into its dispatch;
      (b) if not, coordinate with Gordon to execute his Plan 10 first, or
          build the minimal `q2 check` dispatch with jupyter as first consumer,
          structured per his frozen test-seam spec so checkInstallation slots
          in later. Never a parallel command.

## Phase 3 (later) — CI provisioning (bd-ek7njozm → bd-z09fx92g)

Findings (2026-09-28) that shape the design:

- The test-suite matrix is **ubuntu-latest + macos-latest only — no Windows
  leg**. Jupyter-gated tests currently skip on both.
- The Linux leg runs `endersonmenezes/free-disk-space` with
  **`remove_tool_cache: true`**, deleting `/opt/hostedtoolcache` — i.e. the
  runners\' preinstalled Pythons are deliberately wiped, justified by "no step
  in this job uses /opt/hostedtoolcache/". Any python provisioning must be
  ordered **after** this step (re-downloading what was removed), or the step
  must stop removing the tool cache.
- So "python already on the runner" is only half-true: present at job start,
  gone before tests run on Linux; present-but-jupyterless on macOS.

Design question (bd-ek7njozm, needs team input): pay the setup cost only for
the tests that need it. Options:

  (a) **uv step in the existing legs** — `astral-sh/setup-uv` (cached) +
      locked `uv pip install ipykernel jupyter` into a venv, after the
      disk-space step. Warm-cache cost ~10–20s per leg; simple, uniform,
      OS-agnostic (matters if a Windows leg ever lands, cf. bd-vvxo9ln2).
      Paid by every leg regardless of test selection.
  (b) **Dedicated jupyter job** — provisions python and runs only
      jupyter-gated tests via a nextest filter. Main legs pay nothing; the
      new job duplicates checkout/build (mitigated by Rust caches).
  (c) Status quo (skip everywhere) — rejected; that is bd-z09fx92g itself.

- [ ] **4a. Research/spike (bd-ek7njozm)**: measure option (a)\'s real cost on
      a PR leg (setup-uv cache warm vs cold, venv creation, kernelspec
      visibility to q2's search dirs on ubuntu + macOS); check how
      quarto-cli's CI handles this tradeoff; bring options to the team.
- [ ] **4b. Implement (bd-z09fx92g)** per the decision: uv-based, mirroring
      quarto-cli's `tests/` pattern (pyproject + uv.lock → venv with
      ipykernel → auto-registered `python3` kernelspec), ordered after the
      free-disk-space step on Linux. Verify a jupyter-gated test actually
      executes (not skips) in CI logs.

## Running jupyter tests locally (any developer)

The jupyter-gated tests skip unless the machine has a working `jupyter` and a
discoverable `python3` kernelspec. Minimal setup: install jupyter + ipykernel
into any Python on PATH (e.g. `pip install jupyter ipykernel`, or
`py -m pip install --user jupyter ipykernel` on Windows); ipykernel
auto-registers `python3` under the user Jupyter data dir, which q2's search
dirs cover. Verify with `jupyter kernelspec list`.

## Notes

- bd-db68qtuf (resolve kernel by language instead of hardcoded `python3`) is
  **out of scope** — separate track.
- bd-gdco7syg (runtimelib fork swallowing `ask_jupyter()` failures) is
  adjacent: Phase 2's check should surface what that fork hides.
