---
title: 'RFU-6: the Windows run (Gordon, on a Windows machine)'
date: 2026-11-08
description: 'The Windows checks the records-in-use epic deferred, in highest-risk order. Needs a Windows machine because aws-lc-sys blocks the msvc cross-check from macOS, so no agent runs it. It waits for the plans that change the touched code, then Gordon runs it once and lands the results.'
status: in-progress
depends-on: [opr-6, rfu-1, rfu-2]
lane: none
---

**Epic:** [RIU follow-up](2026-11-08-rfu-epic-riu-follow-up.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (the landing steps apply to Gordon\'s results note too).
**Branch:** `rfu/6-windows-run` · **No lane: no agent starts this plan.** Gordon runs it on a Windows machine.

## Why it waits

It runs the epic\'s clippy and nextest commands once, after the plans that change the touched code have landed:
OPR-6 (`render` reads `execute: record-sources`, a per-document record seam, the moved staleness wrapper), RFU-1 (`q2 convert`
replaces `q2 record`) and RFU-2 (export and import gate). `cargo clippy ... --target x86_64-pc-windows-msvc` from macOS
type-checks Windows code but cannot link (`aws-lc-sys`), and it proves compilation only: file locking, line endings
and process signals need a Windows run.

## Branch and land (explicit; Gordon or an agent he directs, after he has the results)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-3 && pwd   # any lane workspace that is free
git switch -c rfu/6-windows-run feature/one-presenter
# record the results in this file and in one note; set status: complete
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only rfu/6-windows-run
```

## Checklist (highest risk first; every box is `[Gordon]`)

Item 8\'s deletion of `commands/record.rs` moved the old export-command entries to `convert` (RFU-1).

- [ ] `crates/quarto-hub-provider/src/execute.rs:~331` and `:~527`: `canonicalize()` returns a `\\?\` verbatim path on
      Windows; check the "outside the document directory" comparison.
- [ ] `crates/quarto-core/src/positron_cache.rs:~150` (`vscode_file_uri`) and the `%APPDATA%` rule: drive letter, `%3A`
      and non-ASCII are unverified.
- [x] The `--source-uri` default: forward slashes, agreed; implemented by RFU-1 (its test is portable).
- [x] The CLI test file that replaced `record_export_positron_cli.rs` (RFU-1): any `python3` use (the old file used it
      near line 250) and any literal `/some/dir` path (old line ~113).
  (Reconciled 2026-11-16: the `python3` call in `convert_positron_cli.rs` is a probe that skips when it fails; no literal directory path remains.)
- [x] `crates/quarto-core/tests/integration/julia_engine_e2e.rs:~1075-1098`: `kill` and `pgrep` with no `cfg(unix)` gate;
      confirm they cannot run on Windows with Julia installed.
  (Reconciled 2026-11-16: only an `#[ignore]`d test behind `QUARTO_PC4A_LIVE=1` with `if !cfg!(unix)` guards reaches `kill` and `pgrep`.)
- [ ] `crates/quarto-core/src/record/sources.rs:~49,~101-107,~179-199` and
      `crates/quarto-preview/src/capture_driver.rs:~2932-2934`: native-separator display and a self-consistent URI test.
- [ ] CRLF: `positron-conformance/crlf.qmd` and `positron-cache/*.ipynb` under `autocrlf`.
  (Reconciled 2026-11-16: `.gitattributes` now pins `positron-conformance/**` and `positron-cache/**` to `text eol=lf` and leaves `crlf.qmd` as committed (`-text`); `git add --renormalize` changes no fixture. A Windows run still has to confirm it, so the box stays open.)
- [ ] Then the epic\'s command, once: `cargo clippy -p quarto -p quarto-core -p quarto-preview -p quarto-hub
      -p quarto-hub-provider -p quarto-ast-reconcile --all-targets -- -D warnings` and `cargo nextest run` for the same
      crates. Also run the new `record-sources` (render, preview, provider) and `convert` integration tests (they build paths with
      `Path::join`) and say which passed.

## Exit criteria

- Each box has a result (ran on Windows, read, or not run) in "Notes from the run"; failures are listed with output.

## Notes from the run

**Two Windows runs on a stale base.** Run `20261116-121918-GWWIN` (no pandoc or typst) and run `20261116-235123-GWWIN`
(pandoc 3.12.1 and typst 0.15.1 installed) used an experiment branch cut from `feature/one-presenter` at `8f17e950d`,
which is 175 commits before the integration tip and 252 before `epic-closeout` (`d1e14d69d`). Neither run exercised the
finished epic. Treat the results as a map of Windows problems, not as results for the current code; no box is ticked
from them. The machine was Windows on x86\_64 with node 24, uv 0.13.7 and git 2.47.1, `core.autocrlf` set to `input`.
The run tooling is on the temporary branch `rfu/6-windows-tooling` and never lands.

| item | run 2 result | what it ran |
|---|---|---|
| build | PASS | `cargo build` of the touched crates |
| box 1, hub-provider `canonicalize` | FAIL | the probe test `a_figure_reported_under_the_materialized_root_is_captured` |
| box 2, `positron_cache` URI rule | PASS | 18 tests |
| box 4, `convert_positron_cli` | FAIL | 21 passed, 1 failed |
| box 5, `kill` and `pgrep` gates | PASS | both tests skip at their `cfg!(unix)` guard, so Windows runs no `kill` or `pgrep` |
| box 6, `record/sources` and `capture_driver` | FAIL | `capture_driver::` 55 passed, 1 failed |
| box 7, CRLF | PASS, partial | 26 tests passed with `autocrlf=input`, so the CRLF checkout was not exercised |
| box 8, clippy | PASS | clean on the host target |
| box 8, nextest | FAIL | 10181 run; 96 failed with the tools installed (337 without) |

**Findings that still apply to the current code.** Checked against `epic-closeout` by reading:

- **Box 1 (a real defect).** `crates/quarto-hub-provider/src/execute.rs` calls `.canonicalize()` at lines 349, 549 and 644.
  On Windows that returns a `\\?\` verbatim path, so the prefix comparison in the capture collector drops a figure the
  engine reported as `C:\...\doc_files`, with no warning. The probe test fails with `files: []`.
  `quarto_system_runtime::canonical::canonicalize` (dunce) returns the plain form.
- **Box 4 (a test defect).** `convert_positron_cli.rs:61` canonicalizes the temp directory in `doc()`, so the expected
  `--source-uri` becomes `//?/C:/...` while the product correctly wrote `C:/...`.
- **Box 6 (cause unknown).** `capture_driver::tests::a_record_only_cache_entry_is_published` panics with
  `Cannot start a runtime from within a runtime`. Not yet known whether `epic-closeout` still does it.

**Already fixed after the base was cut.** Four failures from the runs have fixes on `epic-closeout`: the three
`execute::` tests (`7830b6efc`, which also failed on macOS), the julia handshake test (`043cccfa4`), and the Jupyter
record goldens (`6b84fd6aa`).

**Where the 96 came from.** By the commit that first added each test, 14 trace to these epics (EXR 3, RIU 4, RFU 2,
OPR 5, KNR 0) and 82 predate them (books, glob, `metadata_layers`, `preview_out`, `route_tests`, `lmd_snapshots`,
`pandoc_request_*`, and others from August to mid-October). Of 285 suspect tests rerun on macOS, only the three
`execute::` failures failed. All other failures are Windows-specific, mostly `\\?\` against plain `C:\` paths,
Unix-shaped path expectations, and CRLF in captured script output. The 82 are out of scope here.

**Tooling observations.** Leaked Python kernels from two nextest `LEAK` tests held PowerShell's stdout pipe open and
hung the script for hours; waiting on the `cmd` process instead fixed it. The machine has no pandoc, typst, julia or
Rscript, and `python3` is the Microsoft Store stub.

**What remains.** Every box stays open. [The Windows fixes plan](2026-11-17-windows-epic-fixes.md) reruns the checks on
`epic-closeout` and fixes what the epics caused. Close this plan from that rerun.
