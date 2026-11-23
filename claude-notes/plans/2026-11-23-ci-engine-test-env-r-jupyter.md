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
  and other R tests use (ggplot2, gt, flextable, bslib, bsicons, shiny,
  dplyr, readr, xml2, plotly, DBI, RSQLite, png). Whether CI syncs `--group fixtures` is decided on measurements:
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

## Phase 1 verification (2026-11-23, Windows)

### Project creation and sync

Ran with rig 0.11.1 and R 4.6.2 in PowerShell 7.6.10:

```powershell
rig proj init
rig proj add knitr rmarkdown jsonlite
rig proj lock
rig proj sync --locked --group fixtures
```

`rig proj add` has a `--dev` shortcut but no arbitrary `--group` option in
this version, so the nine fixture packages were declared directly under
`[dependency-groups.fixtures]` before `rig proj lock`. The generated project
name was changed from the worktree directory name to `q2`. The main add
installed 25 packages; adding the fixtures group installed the remaining 73.
Commit `30b73f99b` contains `rproj.toml`, `rproj.lock`, the root ignore rules,
and this plan's status change. It does not contain `.Renviron` or
`.rvenvlib/`.

After that commit, two consecutive runs of

```powershell
rig proj sync --locked --group fixtures
```

both reported `rproj.lock is up to date`, `0 of 98 packages to install`, and
`Everything is up to date`. A following `git diff --exit-code` produced no
output: in particular, sync left the committed rig marker block in the root
`.gitignore` unchanged.

For the ordinary-R check, `.Renviron` was moved aside and restored in a
`finally` block, then this command was run at the repo root:

```powershell
Rscript -e 'cat(R.version.string, "\n"); cat(.libPaths(), sep = "\n"); cat("knitr available: ", requireNamespace("knitr", quietly = TRUE), "\n", sep = "")'
```

The executable was `C:\Program Files\R\bin\Rscript.exe`; output included:

```text
R version 4.6.2 (2026-10-29 ucrt)
C:/Users/chris/AppData/Local/R/win-library/4.6
C:/Program Files/R/R-4.6.2/library
knitr available: TRUE
```

With no root `.Renviron`, plain R therefore still sees the contributor's
user library.

### Smoke renders

The same document was placed in separate `with-shim` and `without-shim`
temporary directories:

````qmd
---
title: "R engine smoke"
format: html
engine: knitr
---

Inline result: `r 6 * 7`.

```{r}
cat(R.version.string, "\n")
cat(.libPaths(), sep = "\n")
```
````

For the shim run, `.rvenv\bin` was prepended to PATH and `QUARTO_R` was
removed before rendering:

```powershell
$shim = (Resolve-Path .rvenv\bin).Path
$env:PATH = "$shim;$env:PATH"
Remove-Item Env:QUARTO_R -ErrorAction SilentlyContinue
q2 render .tmp-phase1-r-smoke/with-shim/smoke.qmd
```

A fresh PowerShell without that PATH change ran:

```powershell
Remove-Item Env:QUARTO_R -ErrorAction SilentlyContinue
q2 render .tmp-phase1-r-smoke/without-shim/smoke.qmd
```

`Get-Command Rscript` resolved to `.rvenv\bin\Rscript.exe` in the first run
and `C:\Program Files\R\bin\Rscript.exe` in the second. Inspection of both
generated HTML files found `<p>Inline result: 42.</p>` and
`R version 4.6.2 (2026-10-29 ucrt)`. The library output was:

```text
# with the shim
C:/Users/chris/orca/workspaces/windows-ci-r-and-jupyter/.rvenv/lib
C:/Users/chris/AppData/Roaming/rig/data/r/4.6.2/library

# without the shim
C:/Users/chris/AppData/Local/R/win-library/4.6
C:/Program Files/R/R-4.6.2/library
```

### Knitr tests and baseline

The requested command was run with the shim PATH and `QUARTO_R` unset:

```powershell
cargo nextest run -p quarto-core -E 'test(knitr)'
```

Nextest stopped on the first failure, an LF-versus-CRLF golden comparison,
after 22 tests. To avoid confusing fail-fast truncation with the full R
result, both environments were then run to completion:

```powershell
cargo nextest run -p quarto-core -E 'test(knitr)' --no-fail-fast
```

The shim run finished in 941.958 seconds: **543 run, 538 passed, 5 failed**.
The fresh-shell, non-shim baseline finished in 1001.169 seconds: **543 run,
538 passed, 5 failed**. No Rust source changed between the runs. Real R
execution is established by the smoke render and by R-dependent behavior,
not by the totals: for example, the two `knitr_corpus_control` tests each ran
for more than 833 seconds in the shim run.

| Shim failure | Baseline | Diagnosis | Strand |
|---|---|---|---|
| `engine::content_processors::spin::tests::golden_knitr_delim` | Same failure | Windows CRLF: otherwise identical LF and CRLF golden strings | bd-qmp87pxy |
| `engine::knitr::capture_tests::capture_include_graphics_is_an_image_paths_piece` | Passes | R dependency gap: `[None]` versus `[Some(200.0)]`; the locked library lacks `png` | bd-b0o2lxnv |
| `positron_source::the_r_fixture_converts_through_the_knitr_owner` | Same failure | Windows CRLF: two cached cells paired instead of three; temporarily converting only `r.qmd` to LF made the exact test pass | bd-08jz7o8j |
| `presenter_pinning::ipynb_goldens_through_the_knitr_entry_path` | Same failure | Windows CRLF golden difference | bd-qmp87pxy |
| `presenter_pinning::shadow_goldens_through_the_knitr_entry_path` | Same failure | Windows CRLF golden difference | bd-qmp87pxy |

The image-width cause was checked directly with HTML output and `dpi = 96`:
`knitr:::raster_dpi_width()` returned `NULL` in the locked library, which has
no `png`, and `200` in the user library, which has `png` 0.1.9. The approved
package list was not changed during this phase; the missing dependency is
recorded in bd-b0o2lxnv.

The baseline had one additional, different failure in place of the shim's
image-width failure:
`knitr_replay_equals_live::a_sql_chunk_with_hash_pipe_options_records_as_it_runs_live`.
The user library's knitr 1.52.3 rejects `#|` options for its SQL chunk
(bd-lnu81d4a). The shim run does not show that locked knitr 1.52 accepts
them: the locked library has no DBI or RSQLite, so the test returns before
its assertion and nextest counts it as passed. The version comparison needs
a run with both packages available.

The locked library also lacked plotly, so the corpus fixtures
`s2/03_htmlwidget`, `s2/09_widget_filters` and `s9/03_inline_block_widget`
were unavailable, as was `s5/01_odd_chunks` (DBI, RSQLite).

Decided 2026-11-23: packages that tests need belong in the `fixtures`
group, so plotly, DBI, RSQLite and png were added and the lock regenerated
with rig 0.11.1. The relock only added packages (420 platform entries
against 388; no existing version changed) and kept R 4.6.2 on all four
targets; `rig proj sync` left `.gitignore` unchanged. With the shim:

```powershell
cargo nextest run -p quarto-core --no-fail-fast -E "test(a_sql_chunk_with_hash_pipe_options_records_as_it_runs_live) | test(capture_include_graphics_is_an_image_paths_piece)"
```

`capture_include_graphics_is_an_image_paths_piece` now passes (bd-b0o2lxnv).
The SQL test now executes and fails under locked knitr 1.52 too:
`Error: The chunk options should start with '--| ' instead of '#| '.`, the
same failure as knitr 1.52.3 (bd-lnu81d4a). The plotly fixtures were not
rerun; the corpus control tests take over 833 seconds each.

## Phase 3 local verification (2026-11-23, Windows)

### uv project and PyPI isolation

Created the root non-package `pyproject.toml` from the dependency list in the
existing workflow, generated `uv.lock` with uv 0.13.9, and ran:

```powershell
uv sync --locked
.venv\Scripts\python.exe --version
.venv\Scripts\python.exe -m ipykernel install --sys-prefix --name python3
python3 -I -m unittest discover -s packaging/pypi/tests
```

The sync selected CPython 3.12.13, resolved 55 packages, and created the root
`.venv`. The kernel installer wrote only
`.venv\share\jupyter\kernels\python3`. The existing PyPI packaging suite
still passed: 26 tests run, 25 passed and one skipped (its usual skip), with
exit status 0. The root `/.venv/` is now ignored; `packaging/pypi/` keeps its
own build-system-only `pyproject.toml`.

Before registration, `jupyter kernelspec list` included a competing user
kernel:

```text
Available kernels:
  ark           C:\Users\chris\AppData\Roaming\jupyter\kernels\ark
  ggsql         C:\Users\chris\AppData\Roaming\jupyter\kernels\ggsql
  ir            C:\Users\chris\AppData\Roaming\jupyter\kernels\ir
  julia-1.11    C:\Users\chris\AppData\Roaming\jupyter\kernels\julia-1.11
  julia-1.12    C:\Users\chris\AppData\Roaming\jupyter\kernels\julia-1.12
  python3       C:\Users\chris\AppData\Roaming\jupyter\kernels\python3
```

Its `kernel.json` selected the user Python 3.14 installation and had SHA-256
`D97B535C8627F666DFC9EC012A874AA0463D92DAB9B92C45DEF698F8E2965614`. The
hash and argv were identical immediately after `--sys-prefix` registration.

### Python smoke render

Used the one-cell document in `engine-test-env.md` and ran:

```powershell
$venv = (Resolve-Path .venv).Path
$env:VIRTUAL_ENV = $venv
$env:PATH = "$venv\Scripts;$env:PATH"
$env:JUPYTER_PATH = "$venv\share\jupyter"
jupyter kernelspec list
q2 render .tmp-phase3-jupyter/smoke.qmd
```

The kernelspec listing selected
`.venv\share\jupyter\kernels\python3`. Inspection of the generated HTML
found this actual cell output:

```text
Computed result: 42
Python executable: C:\Users\chris\orca\workspaces\windows-ci-r-and-jupyter\.venv\Scripts\python.exe
```

A fresh shell afterwards still listed the user kernel at
`C:\Users\chris\AppData\Roaming\jupyter\kernels\python3`, with the same
hash and argv. A psutil check for processes with an `ipykernel_launcher`
argument returned `[]` after the render and again after the test runs.

### Jupyter-gated tests

Grepped the quarto-core runtime and kernelspec gates, then ran their modules
and the mixed-engine modules with the venv and rig shims on PATH:

```powershell
cargo nextest run -p quarto-core --no-fail-fast -E 'test(jupyter) | test(engine_visibility) | test(engine_error_policy) | test(engine_output_parity) | test(capture_splice_engines) | test(nested_cell_mask_render)'
```

The run finished in 213.542 seconds: **365 run, 359 passed, 6 failed**.
The failures were real executions, not missing-runtime skips.

| Failure | Classification | Strand |
|---|---|---|
| `engine::jupyter::fake_kernel::tests::test_fake_kernel_losing_a_port_reports_a_port_conflict` | Product bug: `JupyterError::is_port_conflict` matches English Winsock prose but not this machine's French error; isolated rerun failed with the stable `(os error 10048)` marker | bd-leshm0xt |
| `engine::jupyter::presenter_goldens::presenter_goldens` | Windows CRLF golden comparisons; passed after temporary LF conversion | bd-umkxc8xb |
| `jupyter_markdown_goldens::stored_records_present_to_the_goldens` | Windows CRLF golden/source comparisons | bd-umkxc8xb |
| `record_preview_splice::record_splice_matches_legacy_splice_on_the_jupyter_fixtures` | Windows CRLF golden comparison; passed after temporary LF conversion | bd-umkxc8xb |
| `jupyter_markdown_goldens::legacy_capture_payload_is_byte_identical_after_flip` | Windows CRLF golden/source comparisons; passed twice isolated after temporary LF conversion | bd-umkxc8xb |
| `jupyter_markdown_goldens::jupyter_markdown_matches_goldens` | Windows CRLF golden/source comparisons | bd-umkxc8xb |

`xxd` confirmed CRLF bytes in the checked-out goldens. Diagnostic conversions
were limited to the fixture `.md`, `.json`, and `.qmd` files and were restored
with `git checkout` after each run; no fixture changes remain. One intermediate
parallel LF diagnostic also showed an extra empty output for `twenty-cells`,
but the isolated LF legacy-capture test then passed twice, so that transient
does not change the classification of the original six failures.

The preview-level `lazy_execution_boots_inert_and_executes_on_first_view` test
is outside this quarto-core-only local selection; it remains for the Windows
dispatch under bd-ymd9up1v.

## CI implementation (2026-11-23, awaiting dispatch)

`.github/workflows/test-suite.yml` now contains the Phase 2 and Phase 3
provisioning and both smoke renders, but no hosted runner has executed it.
The CI-dependent checklist items therefore remain unchecked.

The rig install commands follow the [official rig installation
guide](https://rig.r-lib.org/install.html): versioned Linux and macOS tarballs
unpacked into a runner-temporary prefix, and Chocolatey on Windows.
`gh release view v0.11.1 --repo r-lib/rig --json tagName,assets` verified the
tag and exact asset names; `choco search rig --exact --all-versions
--limit-output` verified that Chocolatey has 0.11.1. The shared setup reads
the first `r_version` from `rproj.lock`, selects rig user mode on the
disposable runner, installs that R, and syncs `--locked --group fixtures`
until the planned timing measurement decides otherwise. The `.rvenv/` cache
has no fallback key and includes the OS, lock hash, and `main-fixtures`
groups. The absolute `.rvenv/bin` directory goes on `$GITHUB_PATH`.

The Jupyter step keeps setup-uv 0.13.7, adds `python-version: 3.12` and
`activate-environment: true`, syncs the root lock, registers the sys-prefix
kernel, and exports the absolute `.venv/share/jupyter` as `JUPYTER_PATH`.
The Windows exclusions and POSIX-only venv paths are gone.

Both smoke steps run through `target/ci/q2` after the test build, including
after a red test result, and grep the rendered HTML for the computed values
and the expected environment path. The new cache step uses
`actions/cache@v6`; its v6.1.0 `action.yml` declares Node 24.

Validation on 2026-11-23: `actionlint
.github/workflows/test-suite.yml` reported only the pre-existing
`actions/checkout@v4` Node 20 deprecation in the WASM job. Re-running with
`-ignore 'runtime "node20" is deprecated'` exited 0. `git diff --check`
passed. Hosted provisioning, smoke renders, timings, and test outcomes are
not yet verified.

## Checklist

### Phase 1: R project and local setup

- [x] Create the rig project at the repo root (`main` + `fixtures` groups), commit only `rproj.toml` + `rproj.lock`, and ignore `/.rvenv/`, `/.rvenvlib/`, `/.Renviron`.
- [x] Check that `rig proj sync` leaves the tracked root `.gitignore` unchanged once rig's block is committed.
- [x] Check that the root files do not disturb anything else that starts R in the checkout (a plain R session at the root without `.Renviron` should be unaffected).
- [x] Locally (Windows): `rig proj sync`, put `.rvenv/bin` on PATH, and render the knitr smoke document (Phase 2 acceptance) with and without the shim, against a clean baseline. The output must hold the computed values and the shim's `.libPaths()`. Then run `cargo nextest run -p quarto-core -E 'test(knitr)'` to see which tests now fail for real; pass/skip counts alone prove nothing.

### Phase 2: CI R step (all legs)

- [ ] Install a pinned rig version per OS (one `RIG_VERSION` env value, like `TYPST_VERSION`): Windows via `choco install rig --version`, or the release installer; Linux and macOS from the versioned release assets (commands to verify against the rig install docs).
- [ ] `rig add <r_version from rproj.lock>`, then `rig proj sync --locked`, and append the absolute `$GITHUB_WORKSPACE/.rvenv/bin` to `$GITHUB_PATH` (a relative entry would not resolve when nextest runs tests from a crate directory).
- [ ] Order on Linux: after "Free disk space", which removes the tool cache.
- [ ] Cache the project library keyed on `rproj.lock`, the runner OS and the synced dependency groups, so a main-only cache is never restored for a fixtures install.
- [ ] Dispatch on all three runners with `filter='test(knitr) | test(engine_visibility) | test(docs_knitr)'` and triage what real knitr coverage turns up on Linux and macOS.
- [ ] Acceptance: pass/skip counts cannot prove execution (the gates return normally when R is absent). On each runner, render a one-cell knitr smoke document (for example `` `r 6 * 7` `` and a chunk printing `R.version.string`) and check the output holds the computed values.
- [ ] Measure package install time and test time separately (main group vs `fixtures`) and record the `fixtures` decision here.

### Phase 3: Jupyter on Windows (bd-yhl7tb3g)

- [x] Move the package list into a root `pyproject.toml` + `uv.lock` (non-package project); check it does not collide with `packaging/pypi/`.
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
