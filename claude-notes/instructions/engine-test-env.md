# Engine test environments: R and Jupyter

This note describes the local runtimes for tests that execute document
engines. The R and Python/Jupyter setups below were verified on Windows 11
on 2026-11-23. The dated commands, output, and test failures are in
`claude-notes/plans/2026-11-23-ci-engine-test-env-r-jupyter.md`.

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
  need. A test that needs a package belongs here. Without it, the test
  skips, its fixture is reported unavailable, or it fails an assertion:
  `capture_include_graphics_is_an_image_paths_piece` checks only for R and
  gets no image width without `png`. A green run can hide the first two.
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

Install the R version that `rproj.lock` names in its `r_version` field by
passing it to `rig add`. Then, from the repository root, sync the main and
fixture packages:

```sh
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
usual R, open a fresh shell without `.rvenv/bin` on PATH, rename the root
`.Renviron` (for example to `.Renviron.rig`) rather than deleting it, since
rig keeps settings you added outside its marked block there, and delete
`.rvenvlib/`, which a later `rig proj sync` recreates. Then check
`.libPaths()` from the checkout root before installing packages.

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

## Python / Jupyter

### Which tests need Jupyter

Jupyter-gated tests return normally when the `jupyter` runtime or a Python
kernelspec is missing, so a green summary can mean that coverage was skipped.
The selection used for the Windows verification includes the modules with
those gates and the mixed-engine modules that also exercise Jupyter:

```sh
cargo nextest run -p quarto-core --no-fail-fast -E 'test(jupyter) | test(engine_visibility) | test(engine_error_policy) | test(engine_output_parity) | test(capture_splice_engines) | test(nested_cell_mask_render)'
```

Use the smoke render below to prove that a real Python kernel executed.
Passing totals alone do not establish that.

### The committed uv project

The root `pyproject.toml` and `uv.lock` define the Python interpreter and
packages for the engine tests. Read them for the current requirements rather
than copying a package list into a second setup. This is a non-package uv
project: sync creates the machine-specific root `.venv/`, which is ignored.

From the repository root, install exactly the locked environment:

```sh
uv sync --locked
```

`--locked` fails if the manifest and lock disagree. Use `uv lock` only when
intentionally changing the committed requirements.

### Activate and register the kernel

Activation puts the environment's `jupyter` and `python` on PATH and sets
`VIRTUAL_ENV`, which the engine uses when selecting a kernel. Choose the form
for your shell.

**PowerShell:**

```powershell
.venv\Scripts\Activate.ps1
```

**POSIX shells:**

```sh
source .venv/bin/activate
```

**Command Prompt:**

```bat
.venv\Scripts\activate.bat
```

Then register `python3` inside the environment, not in the user Jupyter
directory:

```sh
python -m ipykernel install --sys-prefix --name python3
```

Do not substitute `--user`: that can replace a contributor's own `python3`
kernelspec, and the venv-first selection rule cannot identify a kernelspec
stored outside the environment.

Finally, point `JUPYTER_PATH` at the environment's absolute Jupyter data
directory. Quarto re-resolves the selected kernel by name at launch
(bd-6ecz7jt7); runtimelib searches this directory before the user kernelspecs.
It treats `JUPYTER_PATH` as one path, not a separator-split list.

**PowerShell:**

```powershell
$env:JUPYTER_PATH = (Resolve-Path .venv\share\jupyter).Path
```

**POSIX shells:**

```sh
export JUPYTER_PATH="$PWD/.venv/share/jupyter"
```

**Command Prompt:**

```bat
set JUPYTER_PATH=%CD%\.venv\share\jupyter
```

`jupyter kernelspec list` should name the `.venv` copy of `python3`. A user
copy with the same name may disappear from this listing because the
environment copy takes precedence; the setup does not modify the user copy.

### Acceptance: execute a smoke document

Create this document in a temporary directory:

````qmd
---
title: "Python engine smoke"
format: html
jupyter: python3
---

```{python}
import sys
print(f"Computed result: {6 * 7}")
print(f"Python executable: {sys.executable}")
```
````

Render and inspect the HTML with the activated environment and `JUPYTER_PATH`
set:

```sh
q2 render smoke.qmd
```

The output must contain `Computed result: 42` and a `Python executable:` path
inside the checkout's `.venv`. Run this check while a competing user
`python3` kernelspec exists, and compare `jupyter kernelspec list` in a fresh
shell afterwards: the user kernelspec must still be present and unchanged.
The Windows verification output is in the plan linked at the top of this
note; current failures are tracked as strands rather than copied here.
