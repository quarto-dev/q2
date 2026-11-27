---
title: 'Windows: knitr `{bash}` engine runs WSL bash.exe and fails (bd-xw86rvap)'
date: 2026-11-27
description: 'Two knitr tests ran a bash chunk as their non-R engine; on windows-latest R resolves bash to the System32 WSL launcher, so the chunks now use eval: false.'
status: implemented  # pending a Windows CI dispatch
braid:
  strand: bd-xw86rvap
---

**Worktree:** `.worktrees/bd-xw86rvap-knitr-bash-windows` (branch `braid/bd-xw86rvap-knitr-bash-windows`, based on `main` @ `b538d9f61`)

## Triage verdict

**Fixed in the tests.** The root cause is confirmed from R's source: Windows R resolves a bare
command through `SearchPath`, which searches System32 before `PATH`. This is a test problem, not
a q2 product gap. See § Decision.

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
first on `PATH`), but `C:\Windows\System32\bash.exe` still wins.

**The runner has that `bash.exe`.** The failing job ran on image `windows-2025-vs2026`. Its
documentation (actions/runner-images, `images/windows/Windows2025-Readme.md`) lists
"Windows Subsystem for Linux (WSLv1): Enabled", "WSLv2: 3.0.5.0", and
`wslbash.exe → C:\Windows\System32\bash.exe`. The WSL feature is on, but no distribution is
installed. The job log shows `message: "W\r\n\r\n\r\n\r\n\r\n"` followed by the
`system2(cmd, code, ...)` warning, which is a UTF-16 message read as 8-bit text. The exact text
of that message is still inferred, but the cause does not depend on it.

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

## Decision

Decided 2026-11-27: keep the `bash` chunks and add `#| eval: false` to both. Neither test
depends on bash running. Both were written for KNR-3's chunk-kinds row "Non-R engine chunk (bash,
SQL, reticulate Python, custom engines) → fallback, via the chunk hook"
(`2026-10-30-knr-03-record-adapter-hash-fallback.md`), and bash was chosen because the KNR-1 S5
spike happened to use it as its example.

Rejected: knitr's `cat` engine. It reaches the `chunk` hook, so `capture_other_engine_is_a_fallback_piece`
passed with it. `the_fallback_table` failed with `[true]` instead of `[true, false]`, because q2
counts a non-R chunk as a knitr unit only when `knitr_claims` claims its language
(`crates/quarto-core/src/engine/mod.rs:189`: `python | sql | bash | sh`). A `{cat}` chunk is a plain
code block to q2, so the row would stop testing a fallback unit. `sh` is claimed too, but it still
starts a process found on `PATH`. `sql` and `python` need a database connection or reticulate.

`eval: false` keeps the claimed `bash` unit and still goes through the `chunk` hook. It starts
no process: `knitr-bash-windows-investigation/bash_eval_false.R` points `engine.path` at a
missing binary. knitr prints `running: <path> -c "echo hi"` for the evaluated chunk and nothing
for the `eval=FALSE` one.

## Checklist

- [x] Phase 0: RED is the existing Windows CI failure (run 45259769017). It cannot be reproduced
  on this machine, which has no `System32\bash.exe`.
- [x] Phase 1: add `#| eval: false` to the bash chunk in both tests, with a comment explaining why.
- [x] Phase 2a: both tests pass locally with no `SKIP` line:
  `cargo nextest run -p quarto-core -E 'test(the_fallback_table) | test(capture_other_engine_is_a_fallback_piece)'`.
- [ ] Phase 2b: Windows dispatch with the same filter, to show GREEN on the runner (ask before
  dispatching).

## Lost coverage

No test now checks that a non-R chunk which really runs a process still ends up as a fallback
unit. The spec does not ask for that. If wanted, a separate test could cover it with a binary that
is known to be installed.
