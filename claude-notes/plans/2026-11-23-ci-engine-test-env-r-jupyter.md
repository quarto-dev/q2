---
title: 'CI and local engine test environment: R via `rig proj`, Jupyter via uv (bd-9kbjjeb6, bd-yhl7tb3g)'
date: 2026-11-23
description: 'Provision R (rig, with packages from a committed rig project) and Python/Jupyter (uv) on every test-suite leg including Windows, and document the same setup for running the engine tests locally.'
status: in-progress
braid:
  strand: bd-9kbjjeb6
  priority: P2
  labels: [ci, knitr, windows]
---

**Branch:** `braid/bd-9kbjjeb6-windows-ci-r-and-jupyter`, stacked on PR #1275 (`ci/windows-test-suite`), which adds the opt-in `runners` input.
**Strands:** bd-9kbjjeb6 (R), bd-yhl7tb3g (Jupyter on Windows). Related: bd-z09fx92g (no Python/Jupyter test provisioning), bd-6ecz7jt7 (kernel launched by name), bd-55jhd5m9 (silent-skip sites), bd-ek7njozm (Jupyter provisioning question), bd-s8xe0lac (knitr availability trusts R on PATH).

## Overview

The engine tests skip themselves when their runtime is missing. Today CI
runs knitr on **no** platform:

- Linux and macOS have no R step. On the last green `main` run (run
  44820808343), R-executing tests such as
  `knitr_html_dependency::html_dependency_document_renders` finish in about
  9 ms on both legs: they skip.
- The Windows image has R but not rmarkdown. Some gates check only for
  `Rscript`, so in the PR #1275 dispatch run (job 135885897700) 81 tests fail
  with `Missing package: rmarkdown` instead of skipping.
- Jupyter is provisioned on Linux and macOS (uv venv in `$RUNNER_TEMP`), but
  the step hardcodes the POSIX venv layout (`bin/`), so PR #1275 skips it on
  Windows.

Decided 2026-11-23: provision R on every leg, not only Windows. Turning
on real knitr coverage is the point; Windows-only would leave Linux and macOS
skipping silently.

No contributor doc says how to get R or Jupyter ready for the engine tests
locally. This plan writes one, and CI follows the same recipe.

## Approach

### R: a committed `rig proj` project

`rig proj` (experimental in rig 0.11) manages an R project through an
`rproj.toml` manifest and an `rproj.lock` lockfile. `rig proj sync` installs
the locked packages into the project library `.rvenv/lib`, and writes
wrapper `R`/`Rscript` binaries into `.rvenv/bin` that point R at that
library and the locked repositories.

Putting `.rvenv/bin` on `PATH` gives every `Rscript` the project library,
whatever directory it starts in. This mirrors the uv venv's `bin/` on PATH,
and needs no test or engine change: both the tests' `Command::new("Rscript")`
gates and the engine's `which::which("Rscript")` (`engine/knitr/subprocess.rs`)
resolve the wrapper.

Spike, 2026-11-23, Windows, rig 0.11.1, R 4.6.2: `rig proj init` +
`rig proj add knitr rmarkdown jsonlite` in a scratch directory. On Windows,
`.rvenv/bin` holds real `R.exe` / `Rscript.exe` shims, not `.bat` files, so
Rust's PATH lookup finds them. With `.rvenv/bin` prepended to PATH and the
working directory set to `%TEMP%`, the tests' gate expression
(`requireNamespace` on knitr, rmarkdown, jsonlite) exits 0, and
`.libPaths()` lists `.rvenv/lib` first.

**Placement: the repo root.** rig finds a project by looking for
`rproj.toml` in the current directory and its parents, the same model as
uv's `pyproject.toml`. `.rvenv/` is the `.venv` counterpart: machine-specific
and ignored.

**Commit only `rproj.toml` and `rproj.lock`.** `rig proj sync` regenerates
the rest when it is missing (checked 2026-11-23: a directory holding only
those two files gets `.Renviron`, `.rvenvlib/`, `.gitignore` and `.rvenv/`
from `rig proj sync --frozen`). Ignore `/.rvenv/`, `/.rvenvlib/` and
`/.Renviron` in the root `.gitignore`.

The `.Renviron` boilerplate must not be committed. Checked 2026-11-23,
Windows, R 4.6.2: in a directory with the boilerplate but no synced
`.rvenv`, plain `Rscript` prints `! Project is not synced. Run: rig proj sync`
and its `.libPaths()` holds only the system library, so `knitr` is not
found. The same R started in `%TEMP%` sees the user library
(`win-library/4.6`) and `knitr`. Committed, the boilerplate would take
contributors who do not use rig away from their own packages whenever R
starts at the repo root. The boilerplate comment says the `rvenv` package
restores `R_LIBS_USER` when the project has no library; that did not happen
here (cause unverified, possibly `R_LIBS_USER` being unset when R reads
`.Renviron` on Windows; worth an upstream report).

To check during Phase 1: `rig proj sync` merges a marked block into an
existing `.gitignore`. Verify it leaves the tracked root `.gitignore`
unchanged once that block is committed, or a sync would dirty every rig
user's tree.

`QUARTO_R` wins over `PATH` in the engine (`engine/knitr/subprocess.rs`,
line 133), while the tests' gates call `Rscript` from `PATH`. A
contributor with `QUARTO_R` set would pass the gates through the shim and
render with another R. The setup doc says to unset `QUARTO_R` (or point it
at `.rvenv/bin`) in the shell that runs the tests.

The lock pins the R version per target (`r_version` in `rproj.lock`) and,
by default, solves for `aarch64-apple-darwin`, `x86_64-w64-mingw32`, glibc
`x86_64-unknown-linux-gnu` and a `source` fallback, so one lock serves all
three CI runners. CI installs the R version the lock names, not
`release`, and syncs with `rig proj sync --locked`, which fails when
`rproj.lock` is out of date with `rproj.toml` (`--frozen` does not read the
manifest, so a PR adding a dependency without relocking would pass).

### Without rig

The tests and the engine use whatever `Rscript` is first on `PATH`; rig is
not required. A contributor without rig installs the packages listed in
`rproj.toml`'s `[dependencies]` (and the `fixtures` group, if wanted) into
their usual library, for example with `pak::pkg_install()` or
`install.packages()`. Missing packages make the gated tests skip, except the
gates that check only for `Rscript` (bd-s8xe0lac), which fail with
`Missing package: <name>`. The setup doc says so and points to rig as the
way to get exactly what CI runs. `rig proj export` (writes a `DESCRIPTION`)
exists, but a committed second manifest would drift, so it is not used.

### Jupyter: setup-uv `activate-environment`

One recipe for CI and local setup: a root `pyproject.toml` + `uv.lock` with
`requires-python` pinned to 3.12, installed by `uv sync --locked` (never
`uv pip install`, which ignores the lock). In CI, `astral-sh/setup-uv` with
`python-version: 3.12` and `activate-environment: true` puts `.venv/Scripts`
(Windows) or `.venv/bin` on PATH and sets `VIRTUAL_ENV`; the
`runner.os != 'Windows'` conditions go.

Register the kernel inside the venv:
`python -m ipykernel install --sys-prefix --name python3`. The venv-first
rule in `kernel_select.rs` (lines 116-131) picks the kernelspec whose *path*
contains `VIRTUAL_ENV`. A `--user` kernelspec lives in the user Jupyter
directory, never matches, and locally would replace a contributor's own
`python3` kernelspec.

Selection is not enough on its own. The daemon resolves the chosen kernel
again *by name* at launch (`daemon.rs`, line 331, through runtimelib's
`find_kernelspec_with_jupyter_paths`). In the pinned runtimelib,
`data_dirs()` searches `JUPYTER_PATH`, then the user data directory, then
the system directories, and only then the `jupyter --paths` entries where
the venv's kernels are (`dirs.rs`, lines 102-127). A contributor's own
`python3` kernelspec therefore wins at launch even when the venv's was
selected. That launch-by-name mismatch is a product bug, filed as bd-6ecz7jt7.
For the test environment, set `JUPYTER_PATH` to the absolute
`.venv/share/jupyter` directory (runtimelib appends `kernels` itself, and
reads `JUPYTER_PATH` as a single path, not a separator-split list) in CI
and in the documented local shell. Acceptance is a render with a competing
user `python3` kernelspec present: the kernel's `sys.executable` is inside
`.venv`, and the user kernelspec is left unchanged.

## Decisions (2026-11-23)

- **R project at the repo root**, committing only `rproj.toml` and
  `rproj.lock` (see Approach).
- **Packages:** `main` holds the engine minimum (knitr, rmarkdown,
  jsonlite). A `fixtures` dependency group holds what the corpus fixtures
  use (ggplot2, gt, flextable, bslib, bsicons, shiny, dplyr, readr, xml2,
  …). Whether CI syncs `--group fixtures` is decided on measurements:
  install time and test time separately, since R being present turns on
  `knitr_corpus_control.rs`, which reruns the corpus. Fixtures whose
  packages are missing skip through the corpus runner's `Missing package`
  mapping.
- **`rig proj` is experimental:** pin the rig version in CI; the fallback,
  if the lockfile format changes, is a plain `rig add` plus
  `pak::pkg_install()` over the `rproj.toml` package list.
- **Python gets the same treatment:** the uv package list moves from the
  workflow into a root `pyproject.toml` + `uv.lock` (a non-package project,
  `[tool.uv] package = false`), with `.venv` at the root. CI and local setup
  both run `uv sync`.

## Checklist

### Phase 1: R project and local setup

- [ ] Create the rig project at the repo root (`main` + `fixtures` groups), commit only `rproj.toml` + `rproj.lock`, and ignore `/.rvenv/`, `/.rvenvlib/`, `/.Renviron`.
- [ ] Check that `rig proj sync` leaves the tracked root `.gitignore` unchanged once rig's block is committed.
- [ ] Check that the root files do not disturb anything else that starts R in the checkout (a plain R session at the root without `.Renviron` should be unaffected).
- [ ] Locally (Windows): `rig proj sync`, put `.rvenv/bin` on PATH, and render the knitr smoke document (Phase 2 acceptance) with and without the shim, against a clean baseline. The output must hold the computed values and the shim's `.libPaths()`. Then run `cargo nextest run -p quarto-core -E 'test(knitr)'` to see which tests now fail for real; pass/skip counts alone prove nothing.

### Phase 2: CI R step (all legs)

- [ ] Install a pinned rig version per OS (one `RIG_VERSION` env value, like `TYPST_VERSION`): Windows via `choco install rig --version`, or the release installer; Linux and macOS from the versioned release assets (commands to verify against the rig install docs).
- [ ] `rig add <r_version from rproj.lock>`, then `rig proj sync --locked`, and append `.rvenv/bin` to `$GITHUB_PATH`.
- [ ] Order on Linux: after "Free disk space", which removes the tool cache.
- [ ] Cache the project library keyed on `rproj.lock` and the runner OS.
- [ ] Dispatch on all three runners with `filter='test(knitr) | test(engine_visibility) | test(docs_knitr)'` and triage what real knitr coverage turns up on Linux and macOS.
- [ ] Acceptance: pass/skip counts cannot prove execution (the gates return normally when R is absent). On each runner, render a one-cell knitr smoke document (for example `` `r 6 * 7` `` and a chunk printing `R.version.string`) and check the output holds the computed values.
- [ ] Measure package install time and test time separately (main group vs `fixtures`) and record the `fixtures` decision here.

### Phase 3: Jupyter on Windows (bd-yhl7tb3g)

- [ ] Move the package list into a root `pyproject.toml` + `uv.lock` (non-package project); check it does not collide with `packaging/pypi/`.
- [ ] CI: setup-uv with `activate-environment`, `uv sync --locked`, `python -m ipykernel install --sys-prefix --name python3`, `JUPYTER_PATH` set to the absolute `.venv/share/jupyter`; drop the Windows conditions.
- [ ] Validate on all three runners (the recipe changes Linux and macOS too): the PR's own CI covers those two; dispatch Windows with the jupyter tests plus `test(lazy_execution_boots_inert_and_executes_on_first_view)` (preview_static_e2e). Provisioning is accepted when the smoke render below passes on every runner. Test outcomes are recorded, not required green: a `lazy_execution` failure goes on bd-ymd9up1v, and this phase does not try to detect an orphaned kernel (bd-iy2zbos7 owns that).
- [ ] Acceptance: render a one-cell Python smoke document on each runner and check the output holds the computed value and a `sys.executable` inside `.venv`.

### Phase 4: Documentation

- [ ] New `claude-notes/instructions/engine-test-env.md`: which tests need R or Jupyter, how they gate (skip when missing), and per-OS setup mirroring the CI steps: rig (scoop/winget/brew/tarball) + `rig proj sync` + `.rvenv/bin` on PATH, with `QUARTO_R` unset in that shell; `uv sync --locked` + `ipykernel install --sys-prefix` + `JUPYTER_PATH`, and venv activation labeled per shell (POSIX `source .venv/bin/activate`, PowerShell `.venv\Scripts\Activate.ps1`, Command Prompt `.venv\Scripts\activate.bat`). A "without rig" section: install the `rproj.toml` packages into your usual library, and what fails or skips if they are missing.
- [ ] Link it from the AGENTS.md testing instructions and from the CI section of `claude-notes/instructions/windows-dev.md`.
- [ ] Fix the POSIX-only `source .venv/bin/activate` in `docs/guides/authoring/jupyter-kernels.qmd` (user-facing): add labeled PowerShell (`.venv\Scripts\Activate.ps1`) and Command Prompt (`.venv\Scripts\activate.bat`) forms.

### Follow-ups (separate strands)

- [ ] Under `QUARTO_CI=1`, make a missing R or kernel fail instead of skip (the Node pattern in `ts_process.rs`), so a broken provisioning step cannot silently revert coverage (bd-55jhd5m9).
- [ ] Gates that check only for `Rscript` should check for the engine packages (bd-s8xe0lac).
