---
title: 'GE-16a: Fixes for the defects the Windows runs found'
date: 2026-11-19
description: Fixes the product and test defects that Windows runs A and B found, so that run C can show the whole set green on the final code.
status: done
depends-on: [ge-15, ge-16]
lane: C
epic: ../2026-11-18-git-epochs-epic.md
---

**Epic:** [git epochs](../2026-11-18-git-epochs-epic.md) ·
**Waits for:** [GE-15](ge-15-windows-run-a.md) and [GE-16](ge-16-windows-run-b.md) (it fixes what they found) ·
**Unblocks:** [GE-17](ge-17-windows-run-c.md) ·
**Branch:** `ge/16a-windows-fixes` off `feature/git-epochs`, merged by rebase and fast-forward. Nothing is pushed.

## Goal

Fix every defect that GE-15 and GE-16 recorded and classified, in one plan, so that GE-17 can run all `a` and `b` items
once more on the final code and expect green. GE-15 and GE-16 changed no product code; this is where the product and test
changes land. The defects are numbered `A1` to `A2` (from GE-15\'s findings) and `B1` to `B5` (from GE-16\'s).

## Decisions implemented

- **G31:** the Windows runs cover only the new tests, and a defect becomes a fix plan. This is that plan.
- **G23, G4:** `safeJoin` and the probe keep their contracts; a fix tightens them for Windows path forms.
- **G24:** the temp-folder-then-rename step of `createProjectFolder` stays; only its Windows failure mode changes.

## Context

- **Evidence.** GE-15\'s "Findings and proposed fix plans" (F1 to F4) and GE-16\'s "Notes from the run", session 3, run
  `20261119-053817-GWWIN`, whose full `b4` log is on `windows/git-epochs` at
  `scripts/git-epochs-windows/results/logs/20261119-053817-GWWIN/b4-core.log`. Read both first.
- **No Windows machine here.** The agent can write and run the tests on macOS (most fail for the right reason there only
  when the Windows behavior is injected), and cannot observe Windows. The proof that a fix works on Windows is
  GE-17\'s `c3` rerun. Say so in the notes, per fix.
- **Cross-platform rules** apply: `.claude/rules/cross-platform.md` (directory rename with an open handle, paths,
  test-helper gates). A native check is `cargo clippy -p quarto-git-epochs --all-targets --target
  x86_64-pc-windows-msvc -- -D warnings`; if the crate cannot cross-check, say so.

## Fixes

Each fix starts with its test (red for the right reason), then the change, then green.

| Id | Source | Class | Fix |
|---|---|---|---|
| `A1` | GE-15 F1 | test helper, plus one product question | `support::temp_dir()` in `crates/quarto-git-epochs/tests/integration/support.rs` calls `canonicalize`, which yields `\\?\C:\...` on Windows, and Git for Windows rejects that as a worktree or clone destination. Return a plain path on Windows (strip the verbatim prefix, or canonicalize only on macOS, where `/var` to `/private/var` is the reason it exists). Product question to settle and record: does `SubprocessProbe` or the hub ever receive a verbatim project root from `canonicalize` in the real binary? `current_dir` accepts it, but a path argument does not. If it can, the probe hands git a plain path (a unit test on the stripping function, which runs on every OS). |
| `A2` | GE-15 F2 | test setup | With `GIT_EPOCHS_LARGE_TREE_REPO` set, both `large_tree::` tests share one repository and run in parallel; the size test\'s `git switch -q a` hit the gate test\'s `index.lock`. The size test must not switch branches (use `git ls-files` and `git diff` on refs, which take no lock) or each test copies the repo. |
| `B1` | GE-16 item 1 | product defect | `safeJoin` accepts the 8.3 alias `GIT~1` for `.git` on a volume with short names (`safe-path.test.ts` row "an 8.3 short name of .git is rejected" resolved to `...\root\GIT~1\hooks\pre-commit`). Files: `vscode-sync-experiment/src/core/safe-path.ts`, and the same rule in `ts-packages/quarto-git-epochs` if `safeJoin` is shared there. On win32, after the segment check, either compare the `realpath.native` of the deepest existing ancestor with the segment names (so a short name resolves to `.git` and is rejected), or reject a segment shaped like an 8.3 name. The existing row is the test; it must also have a macOS-runnable companion that injects the alias (a segment matcher unit test). |
| `B2` | GE-16 item 2 | machine plus test defect | The `b5` test `join.test.ts` "matches through a symlink, by real path" and the `b6` test `select.test.ts` "drops symlinks and submodule links" fail with `EPERM` creating a file symlink (Windows needs Developer Mode or an elevated token). Add one shared helper in the extension\'s test support, `trySymlink`, that returns false when `symlink` fails with `EPERM` on win32, and have both tests skip with a stated reason. Directory junction rows already run without privilege and stay as they are. The skipped behavior is checked once on a machine with Developer Mode on (GE-17). |
| `B3` | GE-16 item 3, four of six `b4` failures | probable product defect | `createProjectFolder` (`vscode-sync-experiment/src/core/create-folder.ts`, the `fs.rename(tmpRoot, root)` after `repo.shutdown()`) fails with `EPERM` on Windows while a handle is still open inside `tmpRoot`. Affects `editor-only.test.ts` "after a checkout it waits for the hub flip...", and `follower.test.ts` "one aggregate message per batch...", "a folder created before the mode existed starts as a mirror...", and "shows the mode in a persistent status item". Fix: await the storage adapter\'s close, not only `repo.shutdown()`, so no handle is held across the rename (`cross-platform.md`); retry the rename a few times on `EPERM` and `EBUSY` with a short backoff, as `record.ts` does for `git-epochs.json`; leave the temp folder in place and report the error if it still fails (never fall back to a copy that could half-write). Tests: the injected-error `vi.mock` pattern from `clone/record.test.ts` for the retry and the give-up, and a test that holds a handle inside `tmpRoot` and releases it after a delay. |
| `B4` | GE-16 item 3, two of six `b4` failures | unclassified | `project-sync.test.ts` "writes a file whose index entry appears..." (the file reached disk; `.quarto-hub/events.jsonl` had no `created` event) and `clone/clone-modes.test.ts` "each decision writes its event to events.jsonl" (timed out waiting for "edit reaching the doc" after the `reset` event had been seen). The log has no error text for either. Do not guess. First read the event writer and the disk-edit path for a Windows-specific cause (path separators or case in the event path, `appendFile` while another handle is open, the watcher\'s received path form versus the registry key), and write a test that fails on macOS if one is found. If none is found by reading, make the event writer surface its append errors (a product behavior worth having: a failed append is logged, not swallowed) and have the injected watcher record the paths it receives behind an env-gated debug flag, so that the `c3` rerun shows the cause. Record which branch was taken. A fix that waits on that evidence becomes a follow-up task at the end of GE-17. |
| `B5` | GE-16 item 4 | pre-existing, outside the epic, but it blocks `b7` | `ts-packages/quarto-hub-mcp/package.json` builds with `rm -rf dist && tsc && ...`, and `rm` does not exist in `cmd.exe`. Replace the `rm -rf dist` step with a node one-liner (`node -e "require('node:fs').rmSync('dist',{recursive:true,force:true})"`) or a `scripts/` helper, and chmod likewise if needed. Keep the change to that script. |

Carried to GE-17, not fixed here: GE-15 `F3` (16 `a6-nextest` failures with no Windows baseline of `main`) needs one
Windows run of `main` at its merge base before anyone can call them pre-existing, and `F4` (the machine needs
`core.autocrlf=true` and `core.longpaths=true`, which the instructions already say). GE-17\'s checklist asks Gordon for
the baseline run; any pre-existing failure becomes a braid strand, since it lies outside every plan.

## Test specifications

Write these first and run them red where macOS can show the failure.

| Fix | Test | Red for the right reason |
|---|---|---|
| `A1` | A unit test on the verbatim-prefix stripping function (runs everywhere); the existing `a1` tests on Windows | The function returns `C:\x` for `\\?\C:\x` and `\\?\UNC\h\s` correctly |
| `A2` | The two `large_tree::` tests run in parallel against one shared repo with no lock error | Reproducible on macOS by forcing both to switch branches; the old size test fails on `index.lock` |
| `B1` | A matcher test for 8.3 aliases of `.git` (`GIT~1`, `git~2`, with an extension), plus the existing volume row | `safeJoin` rejects each |
| `B2` | `trySymlink` returns false on an injected `EPERM`, and the two tests skip | The tests no longer fail when symlink creation is refused |
| `B3` | Injected `EPERM` on the first two renames, then success; injected permanent `EBUSY` leaves the temp folder and throws; a held handle released after 200 ms | The retry and the give-up behave as written |
| `B4` | Decided by reading (see the table) | Per the branch taken |
| `B5` | `npm run build -w @quarto/hub-mcp` on macOS still passes; the script contains no `rm` | A grep guard in the package test, if one exists |

## Checklist

- [x] Confirm GE-15 and GE-16 are `status: done`, and read GE-15\'s findings and GE-16\'s session 3 notes and the `b4` log.
- [x] Write the tests for `A1`, `A2`, `B1`, `B2`, `B3` and run them to see them fail where macOS can.
- [x] `A1`: fix the test helper; settle and record the product question; add the stripping function if the product needs it.
- [x] `A2`: fix the size test.
- [x] `B1`: fix `safeJoin`; check whether `ts-packages/quarto-git-epochs` has the same rule.
- [x] `B2`: add `trySymlink` and use it in both tests.
- [x] `B3`: fix `createProjectFolder`.
- [x] `B4`: read first; record the branch taken; implement it.
- [x] `B5`: replace `rm -rf dist` in `ts-packages/quarto-hub-mcp/package.json`.
- [x] Gates: `cargo clippy -p quarto-git-epochs --all-targets -- -D warnings` and `cargo nextest run -p quarto-git-epochs`; if
  the hub or probe changed, the same for `quarto-hub`. For the extension and packages: `npm run typecheck -w
  quarto-hub-vscode`, the vitest files touched, `npm test -w @quarto/quarto-git-epochs`, and `npm run build --if-present -w
  ts-packages/quarto-hub-mcp`. `cargo xtask lint`. The Windows clippy cross-check from `cross-platform.md` if the crate
  allows it.
- [x] Update GE-17\'s checklist with the baseline run of `main` (GE-15 `F3`), the Developer Mode symlink check (`B2`), and
  the follow-up for `B4` if it needs the `c3` evidence.
- [x] Fill "Notes from the run" (what ran, what did not, which branch `B4` took, the answer to the `A1` product question),
  reconcile the checklist, set `status: done`, render-check this file, rebase, and fast-forward.

## End-to-end verification

The proof for the Windows-only behavior is GE-17\'s `c3` run. On macOS, run the touched test files, and for `B3` run
`create folder` through the headless peer once (`npm exec -w quarto-hub-vscode -- vitest run src/headless`) and note the
output. For `B5`, run the hub-mcp build and the root `npm run build --if-present -w ts-packages/...` list once. The notes
say plainly that none of this was observed on Windows.

## Exit criteria

- Each of `A1` to `B5` is fixed, with its test, or recorded with the reason it could not be (`B4` may end as
  "diagnosed, fix waits for the `c3` evidence").
- Gates pass on macOS; no unrelated test was changed to pass.
- GE-17\'s checklist lists the three Windows steps this plan carries to it.

## Out of scope

- The 16 `a6-nextest` failures in code this epic did not change (GE-15 `F3`).
- Anything the GE-17 run finds later (it inserts its own fix plan).
- Running on Windows (Gordon, in GE-17).

## Notes from the run

Lane C, branch `ge/16a-windows-fixes`, one session. **None of this was observed on Windows**: the proof for each
Windows-only behavior is GE-17\'s `c3` rerun. Commits: `A1`/`A2` together, then one each for `B1` to `B5`.

- **`A1`.** `support::temp_dir()` now returns `strip_verbatim(canonicalize(..))`, a pure function with three unit tests
  (`\\?\C:\x` to `C:\x`, `\\?\UNC\h\s` to `\\h\s`, other paths unchanged) that run on every OS. The tests were
  written together with the function, so there was no red phase (a missing function is a compile error, not a failing
  test). **Product question, answered by reading:** no change needed. The hub canonicalizes its project root with
  `quarto_system_runtime::canonicalize`, which is `dunce::canonicalize` (plain `C:\...` whenever a plain spelling
  exists; verbatim only past `MAX_PATH` and similar). `SubprocessProbe` gives git the root only through
  `Command::current_dir`, never as a path argument, and `current_dir` accepts a verbatim path. So the real binary never
  hands git a verbatim path argument and no stripping function is needed in product code. The hub\'s own
  `git_epochs` test harness uses `std` `canonicalize` on temp dirs (`a5` passed on Windows, so it does not hit it).
  Clippy for `quarto-git-epochs --all-targets --target x86_64-pc-windows-msvc -- -D warnings` is clean.
- **`A2`.** `repo_dir(switch_to_a)`: only the gate test switches to `a`; the size test reads refs. New test
  `preparing_a_shared_repo_for_a_read_only_check_takes_no_lock` holds `.git/index.lock` by hand and prepares the repo
  without a switch. Seen red by flipping the call to the old always-switch behavior (`git switch` failed with
  `index.lock: File exists`), then green.
- **`B1`.** `isReservedShortAlias` (exported from `safe-path.ts`) flags `GIT~N` and `QUARTO~N` aliases (any case, optional
  extension) on every platform; `safeJoin` rejects them. On win32 it also compares the `realpathSync.native` of the
  deepest existing ancestor (which expands every 8.3 alias) against the ignore rule. Red first (the new rows failed on
  macOS), then green. `fs.promises.realpath` has no `.native`, so the sync form is used. `ts-packages/quarto-git-epochs`
  has no `safeJoin`, so nothing to change there. The existing win32-only row still needs the `c3` run to prove it.
- **`B2`.** `trySymlink`/`trySymlinkWith` in `test-support.ts` (platform and implementation injectable) with four tests;
  `join.test.ts` and `select.test.ts` skip with a stated reason when it returns false. The skipped behavior needs one
  run with Developer Mode on (carried to GE-17).
- **`B3`.** `renameFolderIntoPlace` in `create-folder.ts`: up to eight retries on `EPERM`/`EBUSY` (25 ms times attempt,
  about a second), then it throws and leaves the temp folder; no copy fallback (a half-written copy looks like a
  project). The plan\'s "await the storage adapter\'s close" has nothing to await: `NodeFSStorageAdapter` keeps no file
  open between calls and has no close, and `repo.shutdown()` settles its writes, so the holder can only be outside this
  process (a scanner or indexer), which is what the retry covers. Tests: two refusals then success, a handle released
  after 200 ms, a permanent `EBUSY` (temp folder intact, target absent), and a non-busy error not retried. Red first.
- **`B4`. Branch taken: both.** Reading found one cause and left one open.
  - `project-sync.test.ts` "writes a file whose index entry appears...": a **test race**. The code writes the file and
    then appends the `created` event, and the test waited for the file and read the log once. Fixed in the test (wait for
    the event, and for `deleted`). No product change needed for this one.
  - `clone-modes.test.ts` "each decision writes its event...": no cause found by reading (the edit is dropped silently
    when the gate is closed or the path is inert or removed). Added, as the plan\'s fallback says: `logEvent` now logs a
    failed append (`event log append failed ...`) instead of swallowing it, and `QUARTO_HUB_DEBUG=1` logs each disk
    event\'s path form, relative key, bound/tracked state, and clone gate state (`debug: disk ...`, to the log and
    stderr). Two tests, red first. **The fix for this one waits for the `c3` evidence** (GE-17 checklist updated).
- **`B5`.** `ts-packages/quarto-hub-mcp/package.json`: `build` and `clean` use a `node -e` `rmSync` instead of `rm -rf`
  (`clean` had the same flaw). `build-hygiene.test.ts` now pins the node form and rejects any script that calls `rm`
  (red against the old file, green now). `npm run build` on macOS passes and `node dist/index.js --help` runs.
- **Gates (macOS).** `cargo clippy -p quarto-git-epochs --all-targets -- -D warnings` clean; `cargo nextest run -p
  quarto-git-epochs` 90 of 90; `npm run typecheck -w quarto-hub-vscode` clean; the extension\'s whole vitest suite 336
  passed, 1 skipped; `npm test -w @quarto/quarto-git-epochs` 205 passed; hub-mcp build ok; `cargo xtask lint` clean. The
  hub and probe crates were not changed. The headless `create folder` run (`vitest run src/headless`) passed with the
  editor-only and follower files (27 tests).
