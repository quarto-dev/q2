---
title: 'RFU-6 Windows run: getting started'
date: 2026-11-09
description: 'What Gordon does on the Windows machine: get the branch, run one script, send the results back through the branch. Also what each check proves and the failures to ignore.'
---

**Plan:** [RFU-6](2026-11-08-rfu-6-windows-run.md). This page and the scripts under `scripts/rfu6-windows/` exist only
while the run is in progress; the final commit before landing deletes them, and only the plan\'s "Notes from the run"
stays.

## 1. One-time setup on the Windows machine

You need git, Rust (`rustup`, MSVC host), `cargo-nextest`, and the Visual Studio C++ Build Tools. The script checks all of
this first and prints the install command for anything missing, so you can also just run it and read the top of its
output.

```powershell
winget install Git.Git
winget install Rustlang.Rustup
rustup default stable-x86_64-pc-windows-msvc
cargo install cargo-nextest --locked
git config --global core.longpaths true
```

Optional: Julia and R (their tests skip cleanly without them), `uv` (you have it; `-WithPython` uses it), NASM and
CMake (only if the build fails inside `aws-lc-sys`), Node at the major in `.nvmrc`.

## 2. Get the branch

The macOS agent pushes `rfu/6-windows-tooling` to `origin` as `feature/rfu-6-windows-tooling` after you say go. Clone with
`core.autocrlf=true` (the Git for Windows default, and the setting the CRLF check needs):

```powershell
git clone -c core.autocrlf=true https://github.com/quarto-dev/q2.git
cd q2
git switch -c rfu/6-windows-tooling origin/feature/rfu-6-windows-tooling
```

If you already have a clone, run `git fetch origin feature/rfu-6-windows-tooling` and then the `git switch` line above.
A later fix from the macOS side arrives with `git pull origin feature/rfu-6-windows-tooling`.

## 3. Run it

From the repository root, in Windows PowerShell 5.1 or PowerShell 7:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\rfu6-windows\run.ps1 -WithPython -WithTools
```

`-WithTools` downloads pandoc and typst (the versions CI pins) as zips under `scripts\rfu6-windows\results\tools` and puts them on `PATH` for the run. Without them about 240 tests fail for lack of those tools.

`-WithPython` makes a venv with `ipykernel` under `scripts\rfu6-windows\results\venv`, gives it a `python3.exe`, and
registers a `python3` kernelspec for your user, so the Python engine tests run instead of skipping. Leave it off if you
do not want that kernelspec registered (item `i4` then reports a caveat).

Other switches: `-Only i1,i5` runs just those items, `-SkipBuild` skips the up-front build, `-PositronDoc C:\path\x.qmd`
adds item `i2b` (see below), `-Publish` commits and pushes the results at the end.

Expected cost (estimates; nothing here has been run on Windows): a cold build of the six crates and their test binaries
is the long part, plausibly 20 to 40 minutes and 15 to 25 GB under `target\`. A Dev Drive helps. The tests themselves
take longer than the macOS run because Windows spawns a process per test; plan on 10 to 20 minutes. Items are
independent, so a failure never stops the rest.

## 4. Send the results back

The script writes, under `scripts\rfu6-windows\results\`:

- `rfu6-<host>-<time>.json`, the machine-readable file the macOS side parses;
- `rfu6-<host>-<time>.md`, a short summary with the output tail of every failure;
- `logs\`, full logs (not committed).

Either rerun with `-Publish`, or do it by hand (the script prints the exact commands):

```powershell
git add scripts/rfu6-windows/results/*.json scripts/rfu6-windows/results/*.md
git commit -m "RFU-6: Windows results"
git push origin HEAD:feature/rfu-6-windows-tooling
```

Then tell the macOS session "results are pushed". It fetches the branch and runs the parser. If pushing is awkward, paste
the `.md` summary into the session instead; the `.json` is what ticks the boxes, so attach or paste that too.

## 5. What each item checks

| Item | How it is tested | What a PASS proves | What it does not prove |
|---|---|---|---|
| `i1` | New probe test `a_figure_reported_under_the_materialized_root_is_captured` plus the existing provider and `capture_files` tests | A figure an engine reports under the canonicalized root (on Windows a `\\?\` path) is captured with a forward-slash path. On macOS the probe fails if `canonicalize()` is removed. | What knitr or Jupyter report on Windows. |
| `i2` | The `positron_cache::` unit tests | URI encoding (drive letter, `%3A`, spaces, non-ASCII) and the `%APPDATA%` rules agree with their own port of `uri.ts`. | That real Positron computes the same file name: use `i2b`. |
| `i2b` | Optional. `-PositronDoc` names a document you ran in Positron on this machine | The file name q2 expects exists in Positron\'s cache folder. | Run the document in Positron first, or the item fails for the wrong reason. |
| `i4` | `convert_positron_cli::` tests | The `q2 convert` CLI works on Windows; with `-WithPython` the Python engine tests really run. | Without `-WithPython` the engine tests skip and the item reports a caveat. No literal `/some/dir` path remains (read). |
| `i5` | Runs the two Julia tests that call `kill` and `pgrep` | Both return at their `cfg!(unix)` guard before starting anything (two `SKIP` lines in the log). | That the helpers work (they compile only). |
| `i6` | `record::sources::` and `capture_driver::` unit tests | Source-list parsing, `~` expansion and the self-consistent URI test pass on Windows paths. | That a printed path reads well; optionally render with `-M "execute.record-sources=C:\nope"` and check the message. |
| `i7` | `positron_conformance::`, `positron_source::`, `knitr_positron_import::` under your `autocrlf` | The fixtures hash correctly as checked out; the script prints which fixtures contain CR bytes. | Anything under `autocrlf=false` or `input`. |
| `i8` | `cargo clippy` with `-D warnings`, then `cargo nextest run`, for the six crates; reports the record-sources and convert groups | The epic\'s own command is green on Windows. | The `-D warnings` run lints Windows code only. |

## 6. Known failures to ignore

- The clippy finding at `positron_cache.rs:360`. The script marks a clippy failure whose only location is that line as
  known unrelated.
- `marimo_records` timeouts under load. If they are the only failures, the script marks the nextest item as known
  unrelated; rerun with `-Only i8-nextest` or `-Only i8` once the machine is idle if you want them gone.
- On macOS at this commit, three `quarto-hub-provider` tests fail with the new probe absent too:
  `a_guest_key_may_narrow_a_wider_host_list`, `execute_document_with_an_imported_source_publishes_without_consent_or_engine`
  and `publishing_an_imported_record_needs_no_consent_and_runs_no_engine`. Their cause is not yet diagnosed. If they also
  fail on Windows, leave them in the results; do not investigate.

Anything else that fails is a finding: it goes into the plan\'s notes with its output, and the macOS session proposes a
plan or strand for it. Product code is not changed in this plan.

## Explaining the failures

If the run reports failures, run this once from the repository root. It reads the logs the run already wrote, so it does not rerun any test:

```powershell
uv run scripts/rfu6-windows/extract_failures.py
git add scripts/rfu6-windows/results
git commit -m "RFU-6: failure details"
git push origin HEAD:feature/rfu-6-windows-tooling
```

It writes `results/failure-details-<run>.md`: which tools are on `PATH` (pandoc, typst, and so on), every failing test grouped by its first panic message, a failure count per test module, and the full output of the failing tests for items 1, 4, and 6. Add `--show NAME` to include the full output of any failing test whose name contains `NAME`.
