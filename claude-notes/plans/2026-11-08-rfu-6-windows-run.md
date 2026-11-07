---
title: 'RFU-6: the Windows run (Gordon, on a Windows machine)'
date: 2026-11-08
description: 'The Windows checks the records-in-use epic deferred, in highest-risk order. Needs a Windows machine because aws-lc-sys blocks the msvc cross-check from macOS, so no agent runs it. It waits for the plans that change the touched code, then Gordon runs it once and lands the results.'
status: blocked-on-gordon
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
- [ ] The CLI test file that replaced `record_export_positron_cli.rs` (RFU-1): any `python3` use (the old file used it
      near line 250) and any literal `/some/dir` path (old line ~113).
- [ ] `crates/quarto-core/tests/integration/julia_engine_e2e.rs:~1075-1098`: `kill` and `pgrep` with no `cfg(unix)` gate;
      confirm they cannot run on Windows with Julia installed.
- [ ] `crates/quarto-core/src/record/sources.rs:~49,~101-107,~179-199` and
      `crates/quarto-preview/src/capture_driver.rs:~2932-2934`: native-separator display and a self-consistent URI test.
- [ ] CRLF: `positron-conformance/crlf.qmd` and `positron-cache/*.ipynb` under `autocrlf`.
- [ ] Then the epic\'s command, once: `cargo clippy -p quarto -p quarto-core -p quarto-preview -p quarto-hub
      -p quarto-hub-provider -p quarto-ast-reconcile --all-targets -- -D warnings` and `cargo nextest run` for the same
      crates. Also run the new `record-sources` (render, preview, provider) and `convert` integration tests (they build paths with
      `Path::join`) and say which passed.

## Exit criteria

- Each box has a result (ran on Windows, read, or not run) in "Notes from the run"; failures are listed with output.

## Notes from the run

(Gordon fills in.)
