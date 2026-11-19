---
title: 'GE-15: Windows run A: the Rust and hub items'
date: 2026-11-18
description: 'Runs only the new Rust and hub tests of the git epochs epic on a real Windows machine, from a temporary tooling branch that never lands, with specific items and a record of what each one proves.'
status: done
depends-on: [ge-4]
lane: B
epic: ../2026-11-18-git-epochs-epic.md
---

**Epic:** [git epochs](../2026-11-18-git-epochs-epic.md) · **Waits for:** [GE-4](ge-04-hub-surface.md) ·
**Unblocks:** [GE-16](ge-16-windows-run-b.md), [GE-17](ge-17-windows-run-c.md) ·
**Branches:** the plan\'s notes land through `ge/15-windows-run-a` off `feature/git-epochs` (rebase and fast-forward).
The tooling lives on a separate temporary branch, `windows/git-epochs`, that never lands. This plan is the epic\'s one
exception to "no pushes": it pushes that branch, only after Gordon says go.

## Goal

Run the epic\'s new Rust code on Windows, where it cannot be tested from macOS: the git probe, the gate, the
classifier and arrival comparisons under `core.autocrlf=true`, the path and key handling, and the hub gate with a real
repo. Windows is not in CI, and `cargo clippy --target x86_64-pc-windows-msvc` type-checks but cannot link
(the `aws-lc` build scripts), so only a Windows run shows locking, line-ending, and path behavior
(`.claude/rules/cross-platform.md`). The run covers only the new tests, and every item says what a pass proves and what
it does not.

## Why it waits

It runs once the Rust side is complete: the crate ([GE-2](ge-02-rust-logic-probe-controller.md)), the hub integration
([GE-3](ge-03-hub-integration.md)), and the surface (GE-4). Running earlier would test code that is still changing.

## Decisions implemented

- **G31.** Windows is tested from a temporary tooling branch that never lands, modeled on RFU-6, with the `windows/`
  prefix on the remote branch; each run covers only the new tests.
- **G3, G4, G5, G12, G6** are what the items exercise: the markers and probe, the gate in `sync_all`, the
  case-insensitive key collision rule, and the registry keys.

## Context

- **The model** is workspace-9\'s branch `rfu/6-windows-tooling` (read-only): `scripts/rfu6-windows/run.ps1`,
  `scripts/rfu6-windows/extract_failures.py`, and `claude-notes/plans/rfu-6-windows-run-instructions.md`, with the
  plan `2026-11-08-rfu-6-windows-run.md`. The tooling commits there are labeled "temporary, never lands"; results
  come back as committed JSON and markdown; logs are not committed.
- **What to keep from `run.ps1`:** PowerShell 5.1 compatibility; independent items (a failure never stops the
  rest); `Invoke-Logged`, which runs a command through `cmd.exe` and waits on that one process, because a leaked
  grandchild can hold a pipe open for hours; `Invoke-Nextest` with an `-E` filter and a `MinTests` guard so a filter
  that matches too little fails; `Add-Result` with `proves` and `does_not_prove` fields; `-Only`, `-SkipBuild`,
  `-WithTools` (downloads the pandoc and typst versions CI pins into `results/tools` and puts them on `PATH`),
  `-Publish`; the JSON and markdown summary; a `.gitignore` for `logs/`, `venv/`, and `tools/`.
- **Lessons from the RFU-6 runs:** the first two runs used a stale base, so record the base commit and check it is
  the current tip before a run; the machine had `core.autocrlf=input`, so the CRLF case was never exercised, so the
  script must read `core.autocrlf` and mark the CRLF items with a caveat when it is not `true`; the RFU-6 run found
  real defects (a `\\?\` verbatim path from `canonicalize`), which is the kind of finding to expect.
- **Filter form.** Integration tests are one binary per crate, so a filter reads
  `package(quarto-hub) & binary(integration) & test(<file>::)` (`.claude/rules/integration-tests.md`).
- **Module names are placeholders** below: GE-2 and GE-3 fix the real names. Update the filters before writing the
  script, and run each filter once on macOS (`cargo nextest list -E "<filter>"`) so it matches what it should.

## The items

Script item ids are named by run: `a1` to `a6` here, `b1` to `b7` in [GE-16](ge-16-windows-run-b.md), `c1` to `c3` in
[GE-17](ge-17-windows-run-c.md). Each item carries the catalog number (W1 to W10) of the epic's Windows list in parentheses.
The catalog is shared across the runs: this run holds W1 to W5 and W10, run B holds W6 and W7 and the TypeScript halves
of W3 and W4, run C holds W8 and W9.

- **`a1` (W1), the git probe on Git for Windows.** Filter: `package(quarto-git-epochs) & test(probe_real_repo::)`.
  - Proves: `rev-parse --git-path` output with a drive letter (`C:/...`) and from a linked worktree parses; `ls-tree -r
    -z` with a project prefix returns the right paths; symlink entries (mode 120000) are filtered; `cat-file --batch`
    frames binary content correctly; reflog subjects parse.
  - Does not prove: any Git build other than Git for Windows, or colocated jj.
- **`a2` (W2), the gate during a real `git switch`.** Generate a deterministic tree of 4,000 files with
  `uv run scripts/git-epochs-windows/make_large_tree.py`, then switch between two branches with real git while the
  controller test runs. Filter: `package(quarto-git-epochs) & binary(integration) & test(large_tree::)`.
  - Proves: the controller reports the gate closed for every real marker git creates during a switch of a large tree,
    and releases it afterward. (The skip-never-delete behavior for an unreadable file is a hub behavior, covered by
    `a5`.)
  - Does not prove: antivirus or OneDrive interplay, or timings on a cold cache.
- **`a3` (W3), `core.autocrlf=true`.** Filter: `package(quarto-git-epochs) & (test(classify::) | test(arrival::))`, run in a
  checkout where the script confirms `core.autocrlf` is `true`.
  - Proves: a CRLF-checked-out file compares equal to its blob content after normalization, through the classifier
    and the arrival comparison, for text; binaries compare by bytes.
  - Does not prove: `.gitattributes` filters (LFS, `ident`), or the CRLF case if `autocrlf` was not `true` (the
    script marks that caveat).
- **`a4` (W4), paths and keys.** Filters: `package(quarto-git-epochs) & test(registry::)` and
  `package(quarto-hub) & test(path_keys::)`.
  - Proves: registry keys use `/`; a case-insensitive collision of two keys falls back to ordinary sync; drive letters,
    UNC prefixes, and long paths (with `core.longpaths`) round-trip.
  - Does not prove: NTFS 8.3 short names or Unicode normalization differences.
- **`a5` (W5), hub gate and checkpoint integration.** Filter:
  `package(quarto-hub) & binary(integration) & test(git_epochs::)`, with a real temp repo under the Windows temp
  directory.
  - Proves: the gate inside `sync_all` holds with a real repo; the checkpoint is written before the pointer flips; no
    code path renames a directory while it holds a handle in it (`cross-platform.md`); the unreadable-file test of GE-3
    (an injected `PermissionDenied`, a skip and never a delete) passes on Windows. The injected error stands in for a
    real sharing violation; a real one needs a second process holding the file open.
  - Does not prove: network filesystems or Dev Drive behavior.
- **`a6` (W10), native clippy and nextest.** `cargo clippy -p quarto-git-epochs -p quarto-hub -p quarto-preview -p quarto
  --all-targets -- -D warnings`, then `cargo nextest run` for the same four crates.
  - Proves: the epic\'s own gate is green on the Windows host target.
  - Does not prove: pandoc and typst dependent tests unless `-WithTools` was given. Failures that appear on the
    macOS baseline are listed as known unrelated.

## Test specifications

The tooling is code, so it is tested before it ships, on macOS, in this order:

1. `make_large_tree.py`: run twice with one seed; the file count and a hash of all contents are identical; the repo
   it creates has two branches whose trees differ in a known number of files.
2. `extract_failures.py`: a captured nextest log with known failures (a fixture committed on the tooling branch)
   yields the expected grouping by first panic message and the per-module counts.
3. `run.ps1` parses (`pwsh -NoProfile` loads it as a script block, if `pwsh` is available; otherwise note that the
   syntax was reviewed only), and each item\'s filter lists at least its `MinTests` tests on macOS.

## Checklist

- [x] Confirm GE-2, GE-3, and GE-4 are complete; read their notes for the real module and test names. Replace the
  placeholders above and record the final filters in "Notes from the run".
- [x] Add the tests the filters need and no earlier plan wrote (tests only, no product code, on `ge/15-windows-run-a`
  before the tooling): a `large_tree` module in `quarto-git-epochs` (creates a few thousand files in a temp repo and
  switches branches while the controller runs), a `path_keys` module in `quarto-hub` (drive letters, UNC prefixes, long
  paths, case-insensitive key collisions), and a CRLF case in `arrival::` and `classify::`. Each must run on macOS too;
  list each filter with `cargo nextest list -E` to confirm it matches the `MinTests` guard.
- [x] Write the tooling tests first (items 1 to 3 above) and watch them fail.
- [x] Create the tooling branch and its worktree: `git worktree add -b windows/git-epochs
  .worktrees/windows-git-epochs feature/git-epochs`. Commit the tooling in small commits, each message ending
  "(temporary, never lands)".
- [x] Write `scripts/git-epochs-windows/run.ps1`, adapted from RFU-6 (not copied blindly): items `a1` to `a6`,
  a build item named `a0` (RFU-6 calls it `b0`, which would clash with run B), the preflight that prints the install
  command for anything missing (`uv` is required here, not optional as in RFU-6; NASM and CMake for the `aws-lc`
  crates; Git for Windows; Rust MSVC), `-WithTools`, the
  `core.autocrlf` check and caveat, the base-commit check, JSON and markdown results.
- [x] Write `scripts/git-epochs-windows/extract_failures.py` (standard library only, run with `uv run`),
  `scripts/git-epochs-windows/make_large_tree.py`, and the results `.gitignore`.
- [x] Write the instructions page `claude-notes/plans/git-epochs/windows-run-instructions.md`, on the tooling branch
  only: one-time setup (git, Rust MSVC, `cargo-nextest`, `git config --global core.longpaths true`, Node at `.nvmrc`,
  uv, the Visual Studio C++ build tools); cloning with `-c core.autocrlf=true`; the run command; sending results
  back; a table of what each item proves and does not prove; known failures to ignore; how to explain failures; and
  how to refresh the branch (merge `feature/git-epochs` into it; never rebase a pushed branch). Render-check it with
  `q2 render --strict`.
- [x] Run every item\'s command on macOS where it applies and list the failures there as known unrelated. Estimate the
  Windows cost (a cold build of four crates and their test binaries is plausibly 15 to 30 minutes and 10 to 20 GB;
  an estimate, not measured).
- [x] **Gordon:** say go. The agent pushes with an explicit refspec,
  `git push origin windows/git-epochs:windows/git-epochs`, never silenced, and confirms the remote tip with
  `git ls-remote origin windows/git-epochs` equal to `git rev-parse windows/git-epochs`.
- [x] **Gordon:** on the Windows machine, clone with `core.autocrlf=true`, switch to the branch, and run
  `powershell -ExecutionPolicy Bypass -File scripts\git-epochs-windows\run.ps1 -WithTools -Publish`.
- [x] **Gordon:** tell the agent "results are pushed".
- [x] The agent fetches the branch, parses the JSON, and records each item as PASS, FAIL, or SKIP in the notes with
  the base commit, the machine description, and the `core.autocrlf` value. If anything failed, run the extractor
  and add its summary.
- [x] List every finding with its output. Do not change product code in this plan: propose a fix plan for each
  defect, to be inserted after this plan.
- [x] Land the notes on `feature/git-epochs` through `ge/15-windows-run-a`; the tooling is not merged. Verify with
  `git log feature/git-epochs -- scripts/git-epochs-windows`, which must be empty.

## Gates

Before the push, the macOS baseline of every item\'s command passes or has its failures listed as known unrelated:

```bash
cargo nextest run -p quarto-git-epochs -p quarto-hub -p quarto-preview -p quarto
cargo clippy -p quarto-git-epochs -p quarto-hub -p quarto-preview -p quarto --all-targets -- -D warnings
```

## End-to-end verification

The run itself, on the Windows machine, is the verification. The recorded results file is the evidence: its path and
the items it reports go in the notes, and the notes say the output was read.

## Exit criteria

- Every item has a recorded result (ran on Windows with the base commit, machine, and `autocrlf` value, or not run
  with a reason); failures are listed with their output; findings have proposed fix plans.
- No product code changed; the tooling never reached `feature/git-epochs`.

## Out of scope

- Fixing what the run finds (inserted fix plans), the TypeScript and extension items (GE-16), and the scripted
  scenario and Playwright subset (GE-17).
- Adding Windows to CI.

## Notes from the run

**Session 1 (paused at the first Gordon step: say go before the push).**

**Final filters** (GE-2 and GE-3 fixed the real module names; the placeholders matched, so none changed). They live in
`scripts/git-epochs-windows/items.json` on the tooling branch, which `run.ps1` and `check_filters.sh` both read.
`check_filters.sh` lists each on macOS with the `-p` packages of the item (without `-p`, `nextest list` builds the
whole workspace); listed counts against the `min_tests` guard:

| Item | Filter | Listed | Guard |
|---|---|---:|---:|
| `a1` | `package(quarto-git-epochs) & test(probe_real_repo::)` | 21 | 10 |
| `a2` | `package(quarto-git-epochs) & binary(integration) & test(large_tree::)` | 2 | 2 |
| `a3` | `package(quarto-git-epochs) & (test(classify::) \| test(arrival::))` | 17 | 15 |
| `a4` | `(package(quarto-git-epochs) & test(registry::)) \| (package(quarto-hub) & test(path_keys::))` | 16 | 8 |
| `a5` | `package(quarto-hub) & binary(integration) & test(git_epochs::)` | 26 | 20 |

`a6` runs clippy, then nextest, for `quarto-git-epochs`, `quarto-hub`, `quarto-preview`, and `quarto`.

**Tests added on `ge/15-windows-run-a`** (tests only, no product code; commit `48bd44f15`). They passed on first run,
since the code under test already existed, so there was no red phase for these; each asserts something a wrong
implementation would break (the CRLF tests also assert that the raw, unnormalized comparison gives the other answer):

- `large_tree::` (crate `quarto-git-epochs`, 2 tests): builds a 4,000-file repository with branches `a` and `b`
  (1,200 files differ), or uses the one named by `GIT_EPOCHS_LARGE_TREE_REPO` (what `run.ps1` passes after
  `make_large_tree.py`). Six real `git switch` runs while the real probe and controller poll; asserts the gate was seen
  closed at least once, only for known marker names, then a decision for the new HEAD with a `checkout:` reflog entry,
  then open and idle. The claim is over the six runs, because one fast switch can finish between two polls.
- `crlf_checkout_text_attaches_silently_after_normalization`, `crlf_checkout_binary_compares_by_bytes` (`arrival::`) and
  `crlf_checkout_is_not_an_edit_after_normalization` (`classify::`): a real `git checkout` under `core.autocrlf=true`
  writes CRLF (asserted), and the comparison goes through `normalize_text`; a NUL-bearing file stays byte-identical.
  They set `autocrlf` in their own temp repos, so they run on macOS too; `run.ps1` still marks the `a3` caveat when the
  machine\'s own setting is not `true`.
- `sync::tests::path_keys::` (crate `quarto-hub`, 4 unit tests, because `contained_join` is private): drive-letter and
  UNC forms rejected on every platform, a native root joins to forward-slash keys, a path over 260 characters
  round-trips through disk, and a case pair is flagged by `registry::case_collisions`.
- `support::crlf_checkout` helper. `cargo clippy -p quarto-git-epochs --all-targets --target x86_64-pc-windows-msvc`
  is clean; `quarto-hub` cannot cross-check from macOS (`aws-lc-sys`).

**Tooling** (branch `windows/git-epochs`, worktree `.worktrees/windows-git-epochs` inside this lane worktree; commits
end "(temporary, never lands)"; the branch is local only, nothing pushed). Tooling tests were committed first and seen
failing (the scripts did not exist: three failures and one import error), then passed: `python3 -I -m unittest
discover -s scripts/git-epochs-windows/tests` (4 tests for `make_large_tree.py`: same seed gives identical files and
hash, the two branches differ in exactly `--differing` files, a different seed changes contents, a non-empty
destination is refused; 4 for `extract_failures.py` against a committed nextest-log fixture: grouping, per-module
counts, and the end-of-run repeat lines adding no blocks). `run.ps1` follows RFU-6 (`Invoke-Logged` through `cmd.exe`,
`MinTests` guard, `proves` and `does_not_prove`, JSON and markdown results) with the lessons built in: it records the
base commit and warns loudly when it is not the tip of `origin/windows/git-epochs`, reads `core.autocrlf` and
`core.longpaths` and prints a caveat, and requires `uv`, NASM, and CMake. `pwsh` is not installed on this host, so
**`run.ps1` was reviewed by reading only; it has never been parsed or run.** Expect a first-run syntax or logic fix
from the Windows machine. The instructions page `claude-notes/plans/git-epochs/windows-run-instructions.md` (tooling
branch only) renders with no error of its own under `q2 render --strict`; the only error is the pre-existing Q-2-7 in
`2026-09-21-book-projects-P0-number-sections.md`.

**macOS baseline** (this host, git 2.55.0, base `48bd44f15`): `cargo clippy -p quarto-git-epochs -p quarto-hub
-p quarto-preview -p quarto --all-targets -- -D warnings` clean; `cargo nextest run` for the same four crates: 1979 run,
1979 passed (1 slow), 3 skipped. No known-unrelated failures this time (the two Julia tests GE-4 saw failing passed).

**Windows cost estimate (not measured).** A cold build of the four crates and their test binaries: 15 to 30 minutes and
10 to 20 GB; the six-switch `a2` test adds a minute or two per switch on a slow disk; `a6` nextest is the longest part.

**Push.** After Gordon said go: `git push origin windows/git-epochs:windows/git-epochs`, remote tip equal to the local
tip (`b731fe058`). A second push (`d33c03064`) followed Gordon\'s first attempt: `nasm` was installed by winget but not on
`PATH`, so `run.ps1` now looks in the usual NASM and CMake folders and adds them for the run. Gordon also asked what
`-Publish` does: it commits only the run\'s `.json` and `.md` and pushes `HEAD` to `windows/git-epochs`.

## Windows results

Run `20261119-023206-GWWIN`, results commit `f83a4ec66` on `windows/git-epochs` (`scripts/git-epochs-windows/results/gea-20261119-023206-GWWIN.json` and `.md`; I read both). Base `d33c03064`, the current tip of the remote
branch. Machine: Microsoft Windows 11 Pro, `x86_64-pc-windows-msvc`, Git for Windows 2.47.1, **`core.autocrlf=input`**
(not `true`), `core.longpaths` unset. `run.ps1` ran on its first attempt with no syntax or logic error. Only the JSON
and `.md` came back (no logs, no `extract_failures` output), so the per-test detail below is what the output tails in
the JSON show.

| Item | Catalog | Result | What it shows |
|---|---|---|---|
| `a0` | | PASS | The four crates build on Windows (NASM and CMake present). |
| `a1` | W1 | **FAIL** (19 of 21 pass) | Two tests fail on a test-helper defect, finding F1. The probe parsing, `ls-tree`, symlink filter, `cat-file --batch`, reflog, markers, merge and rebase cases all pass on Git for Windows. |
| `a2` | W2 | **FAIL** (1 of 2 pass) | The gate test passed: six real switches of the 4,000-file tree, gate seen closed for real markers, released after. The size-and-branches test failed on a race in the test setup, finding F2. |
| `a3` | W3 | PASS (caveat) | 17 of 17. Caveat: the machine has `autocrlf=input`; the CRLF tests set `autocrlf=true` in their own temp repos, so the comparison was exercised, but not from the machine\'s own checkout. |
| `a4` | W4 | PASS (caveat) | 16 of 16, including the 260-character-path round trip. Caveat: `core.longpaths` unset (no git operation on a long path in these tests). |
| `a5` | W5 | PASS | 26 of 26: the hub gate with real repos, checkpoint ordering, the injected-`PermissionDenied` skip-never-delete test. |
| `a6-clippy` | W10 | PASS | `cargo clippy -D warnings` is clean for the four crates on the Windows host target. |
| `a6-nextest` | W10 | **FAIL** | 18 failing tests (list below). Two are F1. The other 16 are in code this epic did not change. |

**`a6-nextest` failures.** Two are the F1 tests above. The other 16: `quarto` binary
`render_dry_run::would_render_lines_are_project_relative`; `convert_positron_cli::the_source_uri_default_is_the_documents_path_with_forward_slashes`;
`lmd_snapshots` (four); `render_scripts_cli` (`explicit_interpreter_command_line_with_args`, `list_form_runs_scripts_in_order`);
`smoke_all::smoke_all`; `quarto-hub` `storage::tests::test_storage_manager_prevents_double_lock` and
`..._standalone_prevents_double_lock`; `quarto-preview` `route_tests` (two), `book_downloads::a_chapters_recorded_plot_reaches_the_book`,
`live_reexecute::the_cache_does_not_answer_a_re_execute_under_engine_only_sources`, `render_scripts_boot::pre_render_scripts_run_once_at_boot`.
`git diff` of the epic against its merge base with `main` shows it did not touch `storage.rs`, `render_dry_run`, the
smoke-all fixtures, or any of those test files; it added only the git-status route, the `--git` flag, and the
`PreviewConfig::git` field to `quarto-preview` and `quarto`. **There is no Windows baseline of `main`**, so I cannot
call these pre-existing, only unrelated by inspection of the diff. The `smoke_all` tail shows two causes: `great_tables`
is not installed in the Python environment (an environment gap), and the `brand-monospace-*` typst files fail
regex checks (cause not visible in the tail). On macOS the same four crates passed 1979 of 1979 at `48bd44f15`.

## Findings and proposed fix plans

No product code changed in this plan. Proposed, to be inserted after GE-15 (name them GE-15b and GE-15c when Gordon
approves):

- **F1 (test helper; a1).** `support::temp_dir()` canonicalizes the temp directory, which on Windows yields a
  `\\?\C:\...` verbatim path. Git for Windows rejects it as a worktree or clone destination: `git worktree add
  \\?\C:\...\wt` fails with "could not create leading directories of '//?/C:/.../wt/.git': Invalid argument", and
  `git clone ... \\?\C:\...\clone` fails with "could not create work tree dir ... Invalid argument". It is the same
  class of defect RFU-6 found in product code (`canonicalize` verbatim paths), but here only the test helper has it.
  Fix plan: in `crates/quarto-git-epochs/tests/integration/support.rs`, return the plain path on Windows (strip the
  `\\?\` prefix, or skip canonicalizing off macOS, where `/var` to `/private/var` is the only reason it exists);
  check the hub\'s `git_epochs` test harness for the same pattern (a5 passed, so it does not hit it); rerun with
  `run.ps1 -Only a1`. A product question to settle in that plan: does `SubprocessProbe` or the hub ever receive a
  verbatim project root from `canonicalize` in the real binary? If so, the probe should hand git a plain path, because
  `current_dir` accepts a verbatim path but any path argument does not.
- **F2 (test setup; a2).** With `GIT_EPOCHS_LARGE_TREE_REPO` set, both `large_tree::` tests use one shared repository
  and nextest runs them in parallel processes; the size test\'s `git switch -q a` hit the gate test\'s `index.lock`
  ("File exists"). Fix plan: the size test must not switch branches (use `git ls-files`/`git diff` on refs, which take no
  lock) or each test must copy the repo; then rerun `-Only a2`. The gate test\'s pass is real but ran beside the other
  test on the same repository, so the clean rerun is the proper W2 evidence.
- **F3 (not the epic; a6).** The 16 other failures, with no Windows baseline. Fix plan: run `main` at its merge base
  (`f9936b08d`) once with the same script (`-Only a6`) to separate pre-existing failures from new ones; then file strands
  for the pre-existing ones (they are outside the epic, so a strand is right), starting with the two
  `test_storage_manager_prevents_double_lock` tests and the `smoke_all` `brand-monospace` mismatches. Install
  `great_tables` in the run\'s Python environment, or gate those smoke tests on it.
- **F4 (tooling).** The run needs `autocrlf=true` and `core.longpaths=true` set for the full a3 and a4 evidence, and
  the machine had `input` and unset. The script reported both caveats correctly. Rerun with the clone made as the
  instructions say if full evidence is wanted; the CRLF tests themselves already ran against `autocrlf=true` temp repos.

## Reconciliation

Checklist reconciled against what landed: tests and tooling as listed above; the push, the Windows run, and the
results all happened as recorded. The tooling branch `windows/git-epochs` was never merged;
`git log feature/git-epochs -- scripts/git-epochs-windows` is empty.
