# Engine test environments: R and Jupyter

This note describes the local runtimes for tests that execute document
engines. The R setup below was verified on Windows 11 on 2026-11-23. The
Python/Jupyter section is only an outline until
Phase 3 is verified. The locked CI provisioning has not been added yet; the
existing test-suite step still installs Jupyter and registers a Python
kernel on Linux and macOS, and nothing installs the R packages.

Commands in this note quote arguments with single quotes. Run them from
PowerShell or a POSIX shell: Command Prompt does not group single-quoted
arguments.

## R / knitr

### Which tests need R

The main local selection is:

```sh
cargo nextest run -p quarto-core -E 'test(knitr)'
```

This selection also includes tests whose names contain `knitr` but which do
not execute R. Many R-dependent tests return normally when `Rscript` or a
required package is unavailable, so a green summary can mean that coverage
was skipped. Other gates check only for `Rscript` and then fail with
`Missing package: <name>` when a package is absent (bd-s8xe0lac).

Use the smoke render below to prove that R really executed. To see every
failure rather than stop at the first one, add `--no-fail-fast`:

```sh
cargo nextest run -p quarto-core -E 'test(knitr)' --no-fail-fast
```

### The committed rig project

The root `rproj.toml` and `rproj.lock` define the test environment; read
them for the current packages and R version:

- `[dependencies]` in `rproj.toml` holds what the knitr engine itself needs.
- `[dependency-groups.fixtures]` holds what corpus fixtures and other R tests
  need. A test that needs a package belongs here; without it, the test
  skips or its fixture is reported unavailable, and a green run hides the
  gap.
- `rproj.lock` pins the R version and every package for Windows, Linux,
  macOS, and source installs.

`rig proj` is experimental. This lockfile was generated and verified with
rig 0.11.1; check `rig --version` if a newer rig behaves differently. Do not
run `rig proj init` in the checkout: the manifest already exists, and init
would recreate local bootstrap files that this repository deliberately
ignores.

### Install rig

Use one of these official installation methods, then check `rig --version`.
The package-manager commands are not pinned to 0.11.1; the
[rig releases](https://github.com/r-lib/rig/releases) page has the versioned
assets when an exact version is needed.

**Windows, WinGet:**

```powershell
winget install posit.rig
```

**Windows, Scoop:**

```powershell
scoop bucket add r-bucket https://github.com/cderv/r-bucket.git
scoop install rig
```

**macOS, Homebrew:**

```sh
brew tap r-lib/rig
brew trust r-lib/rig
brew install --cask rig
```

**Linux, user-local tarball:**

```sh
mkdir -p ~/.local
curl -Ls https://github.com/r-lib/rig/releases/download/latest/rig-linux-$(arch)-latest.tar.gz |
  tar xz -C ~/.local
export PATH="$HOME/.local/bin:$PATH"
```

These commands come from the [official rig installation
guide](https://r-lib.github.io/rig/install.html).

### Sync and select the project R

From the repository root, install the R version named by the lock and sync
the main and fixture packages:

```sh
rig add <version>   # the r_version in rproj.lock
rig proj sync --locked --group fixtures
```

Omit `--group fixtures` to install only the main engine packages. `--locked`
fails if `rproj.toml` and `rproj.lock` disagree instead of silently resolving
a different environment.

The project library and R wrappers are machine-specific output under
`.rvenv/`. The root `.gitignore` also excludes `.rvenvlib/` and `.Renviron`:
committing those bootstrap files would make R in an unsynced checkout hide a
contributor's usual package library.

Select the wrappers for the shell that will render or run tests, and unset
`QUARTO_R`. The test gates find `Rscript` through PATH, while the engine gives
`QUARTO_R` precedence; leaving it set can test one R and render with another.

**PowerShell:**

```powershell
$env:PATH = "$PWD\.rvenv\bin;$env:PATH"
Remove-Item Env:QUARTO_R -ErrorAction SilentlyContinue
```

**POSIX shells:**

```sh
export PATH="$PWD/.rvenv/bin:$PATH"
unset QUARTO_R
```

**Command Prompt:**

```bat
set PATH=%CD%\.rvenv\bin;%PATH%
set QUARTO_R=
```

Confirm that the wrapper and project library are selected:

```sh
Rscript -e 'cat(R.version.string, "\n"); cat(.libPaths(), sep = "\n"); cat("knitr=", as.character(packageVersion("knitr")), "\n", sep = "")'
```

The first library should be the checkout's `.rvenv/lib`, and the knitr
version should match `rproj.lock`.

### Acceptance: execute a smoke document

Create this document in a temporary directory:

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

With the shim PATH and `QUARTO_R` unset, render and inspect the HTML:

```sh
q2 render smoke.qmd
```

The output must contain `Inline result: 42.`, the R version, and the
checkout's `.rvenv/lib` path. Rendering the same document in a fresh shell
without the shim is a useful baseline: it should still compute `42` if the
system R has the engine packages, but its library list should name the usual
user and system libraries instead of `.rvenv/lib`.

### Without rig

rig is not required. The tests and engine use the first `Rscript` on PATH.
Install the packages listed under `[dependencies]` in `rproj.toml` into your
usual R library with `install.packages()`, and those under
`[dependency-groups.fixtures]` as well when running corpus fixtures. Your
versions may differ from the lock, so a failure can come from a version
difference rather than from q2. Keep `QUARTO_R` unset or make it name the same R installation
that PATH selects.

If you synced the rig project earlier, its ignored root `.Renviron` still
applies to any R started from the checkout root. R reads `.Renviron` from
the working directory in place of `~/.Renviron`, so a personal `~/.Renviron`
is skipped there too. With `.rvenv` removed, that R warns
`Project is not synced` and loses your user library. To return to your
usual R, delete the root `.Renviron` and `.rvenvlib/` (a later `rig proj
sync` recreates them), and check `.libPaths()` from the checkout root before
installing packages.

Tests whose gates include their required packages skip when those packages
are missing. Gates that check only for `Rscript` can instead fail with
`Missing package: <name>`. Passing totals alone do not establish which case
occurred; run the smoke document and inspect its output.

### Known failures

Failures seen with the locked library are tracked as braid strands, not
listed here: Windows CRLF goldens (bd-qmp87pxy, bd-08jz7o8j) and the SQL
chunk `#|` refusal (bd-lnu81d4a). The Windows verification run, with its
commands and output, is in
`claude-notes/plans/2026-11-23-ci-engine-test-env-r-jupyter.md`.

## Python / Jupyter: outline, not yet verified

Phase 3 will fill in and verify this section. Its planned recipe is:

- Create a root non-package `pyproject.toml` and `uv.lock`, pinned to Python
  3.12, then use `uv sync --locked`.
- Activate `.venv` with the shell-specific form: `source .venv/bin/activate`
  on POSIX, `.venv\Scripts\Activate.ps1` in PowerShell, or
  `.venv\Scripts\activate.bat` in Command Prompt.
- Register the kernel inside the venv with
  `python -m ipykernel install --sys-prefix --name python3`.
- Set `JUPYTER_PATH` to the absolute `.venv/share/jupyter` path so launch by
  name cannot select a competing user kernelspec (bd-6ecz7jt7).
- Accept provisioning only after a render computes a Python value and shows
  `sys.executable` inside `.venv`, with a competing user `python3` kernelspec
  present and unchanged.

These files and CI steps do not exist on the branch yet; this outline is not
a runnable setup guide.
