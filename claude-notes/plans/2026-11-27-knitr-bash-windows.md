---
title: 'Windows: knitr `{bash}` engine runs WSL bash.exe and fails (bd-xw86rvap)'
date: 2026-11-27
description: 'Two knitr tests use a bash chunk as their non-R engine; on windows-latest R resolves bash to the System32 WSL launcher, so the tests should use an engine that spawns no process.'
status: draft  # Investigation — pending design alignment with user; do not implement before the go-ahead
braid:
  strand: bd-xw86rvap
---

**Worktree:** `.worktrees/bd-xw86rvap-knitr-bash-windows` (branch `braid/bd-xw86rvap-knitr-bash-windows`, based on `main` @ `b538d9f61`)
**Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** The root cause is confirmed from R's source: Windows R resolves a bare
command through `SearchPath`, which searches System32 before `PATH`. This is a test problem, not
a q2 product gap. The one open choice is which engine replaces `bash` in the two tests.

## Issue context

Filed 2026-11-24 (bug, P2, labels `knitr`, `windows`). On the Windows dispatch of PR #1292
(run 45259769017, job 137991551865), two tests fail with `ExecutionFailedAtLines`:

- `engine::knitr::gating_tests::the_fallback_table` (case "non-R engine")
- `engine::knitr::capture_tests::capture_other_engine_is_a_fallback_piece`

knitr runs `system2("bash", "-c \"echo hi\"")`, which exits with status 1 and prints `W` followed
by NUL-separated blank lines. The strand inferred, without verifying it, that `bash` is the
System32 WSL launcher printing its UTF-16 "no installed distributions" message.

## Dependency graph

- **discovered-from**: bd-9kbjjeb6 (in progress). That strand provisions locked R and Jupyter for
  the engine tests (PR #1292). Before it landed, these tests failed earlier with
  `Missing package: rmarkdown`, so this failure only became visible once R was set up.
- No `blocks` or `related` edges.

## What the code looks like today

**Why PATH order does not help.** knitr's `eng_interpreted` calls
`system2(get_engine_path(options$engine.path, "bash"), code)`, so the command is a bare `bash`.
On Windows, R's `system2` reaches `expandcmd()` in `src/gnuwin32/run.c`, which tries
`bash.exe`, `.com`, `.cmd` and `.bat` through `SearchPath(NULL, ...)`. With a NULL path, Win32
searches the application directory, **System32**, the Windows directory and the current
directory before `PATH`. The test step runs under `shell: bash` (Git Bash, whose `/usr/bin` comes
first on `PATH`), but `C:\Windows\System32\bash.exe` still wins. That explains the strand's
inference, although nobody has looked at the runner's System32 directly.

**`Sys.which` is a faithful probe.** `do_syswhich` (same file) calls the same `expandcmd`, so
`Sys.which("bash")` on the runner reports exactly what `system2` will launch.

**Not reproducible on this dev machine.** There is no `C:\Windows\System32\bash.exe` here (WSL's
launcher lives in `WindowsApps`, which comes later through `PATH`). `Sys.which("bash")` returns
Rtools' `rtools\45\usr\bin\bash.exe`, and `system2("bash", "-c 'echo hi'")` prints `hi`. Placing a
`bash.exe` ahead of it would need admin rights to write to System32. The real failure (RED) is the
Windows CI dispatch.

**What the tests need from the chunk.** Only "a chunk whose engine is not R". `capture.R`
classifies by `capture_is_r_engine()` (`tolower(options$engine) == "r"`), and the `chunk` output
hook (`capture.R` ~498) records `class = "other"` for any other engine. Running `bash` adds
nothing to what the tests check, and it pulls in an external binary that sits in a different
place on each OS. No other test or fixture in `crates/` uses a `{bash}` chunk.

**Product side.** A Windows user whose `{bash}` chunk hits the WSL launcher gets the same failure
from knitr in Quarto 1. q2 calls knitr unchanged, so it has no product gap of its own here.

## Proposed phases (draft)

- Phase 0: RED is the existing Windows CI failure (run 45259769017). Optionally, confirm
  `Sys.which("bash")` and `Test-Path C:\Windows\System32\bash.exe` on the runner in the same
  dispatch that verifies the fix.
- Phase 1: in both tests, replace the `bash` chunk with a non-R engine that spawns no process
  (see Q1). Keep the expected `classes` assertion in line with the new engine name.
- Phase 2: run both tests locally
  (`cargo nextest run -p quarto-core -E 'test(the_fallback_table) | test(capture_other_engine_is_a_fallback_piece)'`),
  then a Windows dispatch with the same filter to show GREEN on the runner.

## Open design questions for the user

1. **Replacement engine.** Candidates:
   - `cat`: a built-in knitr engine that spawns no process. In a plain `knitr::knit` it produces
     no visible output (`knitr-bash-windows-investigation/cat_engine.R`). It is not yet confirmed
     that the `chunk` hook still records an `other` piece for it.
   - `Rscript` (`{Rscript}`): non-R by name, and its binary is guaranteed wherever these tests
     run. It still spawns a process, and its name reads like R, which blurs the "non-R" intent.
   - A test-registered engine (`knitr::knit_engines$set(...)`): fully hermetic, but it needs a
     setup hook in the test document.

   Is `cat` acceptable if the hook records it, with `Rscript` as the fallback?
2. **Keep bash anywhere?** Should the "non-R engine" row stay bash on unix through a cfg switch?
   The draft answer is no: the row tests classification, not bash, and a per-OS switch is the kind
   of gate CLAUDE.local.md warns against.
3. **Runner probe.** Is the one-line `Sys.which` diagnostic worth adding to the verification
   dispatch to turn the WSL inference into a fact? Or is the source-level explanation enough?

## Risks / tradeoffs (draft)

- `cat` might go through a different knitr output path (`engine_output` with empty output) and
  never reach the `chunk` hook. That would make the test fail for an unrelated reason, so try it
  first.
- Changing the engine name changes the asserted `classes` (`["bash"]` becomes `["cat"]`). This is
  a test-only change, and no snapshots are involved.
