---
title: 'GE-5: TypeScript package: pure logic, git probe, gate and quiescence controller'
date: 2026-11-18
description: 'Implements in the shared TypeScript package the same classifier, arrival rule, startup rule, registry operations, git probe, and gate controller that the Rust crate has, driven by the same fixtures and golden docs.'
status: approved
depends-on: [ge-1]
epic: ../2026-11-18-git-epochs-epic.md
---

**Epic:** [git epochs](../2026-11-18-git-epochs-epic.md) · **Waits for:** [GE-1](ge-01-contract-and-gate-0.md) ·
**Unblocks:** [GE-9](ge-09-extension-follower-modes.md) · **Branch:** `ge/05-ts-logic` off `feature/git-epochs`,
merged by rebase and fast-forward; nothing is pushed.
**Twin:** [GE-2](ge-02-rust-logic-probe-controller.md) builds the same modules in Rust and runs in parallel with this
plan. The two plans never wait for each other; they meet only through the shared fixtures, the golden docs, and the
contract spec that GE-1 leaves.

## Goal

Build the TypeScript half of the git-aware logic in `ts-packages/quarto-git-epochs`, with the same module names and the
same shape as the Rust crate, so that one set of fixtures drives both and the tests translate one for one. Nothing here
touches VS Code, the file system watcher, or an Automerge `Repo`: the extension (GE-9, GE-10, GE-12) and the headless
peer (GE-8) are the consumers.

## Decisions implemented

See the [epic decisions](../2026-11-18-git-epochs-epic.md#decisions). The ones this plan carries:

- **G2** classifier: content rule first, reflog subject as the tiebreaker (any non-commit entry in the reflog span
  means checkout-type); jj and reflog-less repos use content alone.
- **G3, G4** gate markers and subprocess hygiene: plumbing only, `GIT_OPTIONAL_LOCKS=0`, no `hash-object`, `cat-file
  --batch` for blob content, timeouts, an off switch, colocated jj lock, forward-slash paths.
- **G7, G8, G11, G12** registry (flat keys, a retired set, a narrow repair rule): canonical doc is the lowest `(generation, docId)` that is not retired; a commit moves the doc forward in one
  change; an unavailable canonical doc is treated as absent after 5 s; a doc is live under at most one path; a
  case-insensitive key collision falls back to ordinary sync; an unborn repo is refused.
- **G9** arrival, the five steps and the `reset --hard` retire step, text and binary (byte hash), `ReadOnly` and
  `WriteBack` variants, and the step 5 fallback under jj.
- **G10** the startup rule over a persisted `{head, blobs}` record and the detached set.
- **G14** per-file mode: tracked files take part, untracked files follow the pointer.
- **G29** two implementations, one fixture set; the pure core takes and returns plain JSON-able data.
- **G1** the controller\'s synchronous `check()`, the hold of every file whose disk differs from its last synced
  content, and the quiet window that ignores the peer\'s own writes.
- **G32** the numbers: quiet window 1 s, decision cap 30 s, stale lock 60 s, unavailable canonical doc 5 s, controller
  tick while a decision is pending 250 ms.

## Context

What GE-1 leaves (this plan reads it, never edits it):

- The package skeleton at `ts-packages/quarto-git-epochs`, a workspace member through the `ts-packages/*` glob in the
  root `package.json`, with scripts `test` (vitest), `typecheck`, and `build`, and its step in
  `.github/workflows/ts-test-suite.yml`. The lint `ci-test-suite-unwired` is already satisfied.
- `fixtures/`: the decision tables (classifier, arrival, startup, registry operations, each with the peer and write
  policy axes), the scenario step lists (JSON: git commands and file writes, with the expected classifier output after
  each step), and the golden binary docs (a sidecar before and after one operation).
- The registry and sidecar schema spec under `claude-notes/designs/`, and the V4 index helpers in
  `@quarto/quarto-automerge-schema` (`ts-packages/quarto-automerge-schema/src/index.ts`; `migrateIndexDocument` at
  `:107-133` leaves a newer-version doc untouched).

What exists today: nothing in this package beyond the skeleton. The extension\'s git-unaware path is `syncIndex`
(`vscode-sync-experiment/src/project.ts:322-330`); GE-9 and GE-10 replace it, not this plan.

Module map. The names are the contract\'s module map (GE-1), the same as the Rust crate\'s, one file each under `src/`:

| Module | Contents |
|---|---|
| `classify` | `classify(input)` returns a verdict, commit or checkout (G2) |
| `arrival` | `arrive(input)` returns one action for a file (G9, G14); text and binary; both policies |
| `startup` | `startupDecision(input)` returns the per-file plan for the three cases (G10) |
| `registry` | `canonical`, `nextGeneration`, `register`, `moveForward`, `retire`, `repair`, `entriesFor`, `keyCollision`, `liveElsewhere`; reads and writes the sidecar (flat keys) over `@automerge/automerge` docs |
| `probe` | the `GitProbe` interface and `createGitProbe` (the subprocess implementation) |
| `controller` | `createController(probe, options, clock)`, the tick-driven state machine, and `check()`, the synchronous entry every sync path calls first |

Boundaries. Pure functions take plain JSON-able data and return plain data; the action and field names are exactly the
ones in GE-1\'s fixtures and spec, and a rename is a change to the contract, not to this plan. The only Automerge use is
the registry document operations (`A.change` over a `Doc`). No `vscode` import anywhere, no timers except through the
injected clock, no file system except in the probe.

## Test specifications

Written first, seen failing for the right reason (a missing module, then a wrong answer), then implemented.

1. **Fixture runner.** Every file under `fixtures/` is loaded by one runner per module: each row is an input and an
   expected output. Rows marked for a role this module does not implement are skipped by name, never by position.
2. **Classifier rows:** a plain commit; amend; a merge commit and a cherry-pick commit (content rule handles them); a
   checkout; a reset with an unchanged blob; save-then-commit (a plain `commit` subject with a differing disk is the
   author\'s edit); a reflog span of `switch` then `commit` (checkout-type); `git stash` push; jj and no reflog (content only).
3. **Arrival rows,** both policies, text and binary: no registered doc; already attached; disk equals the doc; clean
   but drifted (join with a notice); dirty (detach); `ReadOnly` with a differing disk (create from disk); a doc live
   under another path (fork); an unavailable canonical doc (create after the timeout); already attached with S equal to B and T different (retire,
   create, register, flip); a doc another peer retired; every changed file under colocated jj (step 5 fallback).
4. **Registry rows and golden docs:** canonical by lowest `(generation, docId)`; a creation race on one key and on a new path (flat union); the first doc
   of a path gets generation 0 and a later creator gets the maximum over the path plus one (the maximum of an empty path is -1); the exported event
   kinds equal `fixtures/vocabulary.json`; retire skips the doc and repair never re-registers it; move forward is one change and carries the generation; two tips;
   case-insensitive collision; unborn repo refused. The package reads each golden doc and checks its logical entries;
   it applies the operation and checks that the result decodes to the golden \"after\" entries. Reading this package\'s
   docs with the Rust crate is not checked here; GE-14, the first plan that waits for both, does that.
5. **Startup rows:** HEAD unchanged (reconcile as today); HEAD changed with some blobs differing; no record (arrival for
   every file, never a merge); detached paths survive a restart.
6. **Controller with a fake probe and a fake clock, no sleeps:** gate closes on each marker; pending on a HEAD or
   reflog-tail change; hold of every file whose disk differs from its last synced content; `check()` sees a changed reflog tail before any
   tick; the peer\'s own writes do not restart the quiet window; decision only after 1 s of quiet and stable state; the 30 s cap; lock
   files stale after 60 s (resume with a warning); state markers pause indefinitely; an unresolvable HEAD means no
   change and keeps re-checking; resume; `off` when the probe reports git missing or refused.
7. **Probe against real temp repos** (the scenario step lists): HEAD and unborn HEAD; `--git-path` for a linked
   worktree; `ls-tree -r -z` with a prefix and with modes 100644 and 100755 only (symlinks and submodules filtered);
   `cat-file --batch` content with and without `core.autocrlf`; reflog entries since a recorded sha, including a sha
   that is absent and one that appears twice; the rebase markers; colocated jj, skipped when `jj` is absent.
8. **Windows-shaped inputs** run natively on every platform: keys always use `/`; fixture files with `\r\n`; drive
   letter and UNC prefixes in a path argument.

## Checklist

- [ ] Confirm GE-1 is complete: the skeleton, `fixtures/`, the spec, and the golden docs exist, and `npm test -w
  @quarto/quarto-git-epochs` passes its fixture-validation test. Write down the exact action and input names the
  fixtures use.
- [ ] Write the fixture runner for each module and the golden-doc test; run them and see them fail because the
  modules do not exist.
- [ ] `classify`: implement G2; the classifier rows pass.
- [ ] `registry`: implement canonical ordering (retired entries skipped), `nextGeneration` (maximum over the path plus one),
  `register` (put-if-absent), `moveForward` (one `A.change`), `retire`, `repair`, the
  collision check, and the live-elsewhere check; the registry rows and the golden-doc test pass; commit the doc this
  package writes as a fixture for the cross-implementation check in GE-14 (under `fixtures/golden/`, named for this
  plan); nothing in GE-2 reads it.
- [ ] `arrival`: implement the five steps for text, then binary (byte hash), then the `ReadOnly` variant, including
  the unavailable canonical doc and the live-elsewhere fork; the arrival rows pass.
- [ ] `startup`: implement the three cases; the startup rows pass.
- [ ] `probe` interface and a fake probe used by the controller tests. The interface also has the repo identity
  queries (G4): `rootCommits()` (`rev-list --max-parents=0 HEAD`), `isShallow()` (`rev-parse
  --is-shallow-repository`), and `showPrefix()`; GE-10 uses them for the attach check and GE-12 for the `repo` entry. The interface also has `cheapState()`, which
  stats the marker paths and the reflog file with no subprocess, for `check()`; a missing reflog file (no `.git/logs`
  directory when `core.logAllRefUpdates=false`) means "no reflog", not an error, and a test pins that.
  Real-repo tests: a repo with two root commits, and a `--depth 1` clone made through a `file://` URL (a plain local
  path ignores `--depth`).
- [ ] `controller`: write the fake-probe and fake-clock tests, see them fail, then implement the state machine with the
  numbers from G32, the synchronous `check()`, and an `isOwnWrite(path)` option for the quiet window; they pass with no real timers and no sleeps.
- [ ] `probe` subprocess implementation: spawn `git` without a shell, with `GIT_OPTIONAL_LOCKS=0`, a per-call timeout,
  a path that finds `git.exe` on Windows, `cat-file --batch`, `ls-tree -r -z` limited to the project prefix, markers
  resolved once with `--git-path`, jj\'s `.jj/working_copy/working_copy.lock`, and forward-slash output; a missing or
  refused git yields the off state with one message.
- [ ] Run the scenario step lists against real temp repos (jj skipped when absent) and compare the classifier output
  after every step; keep the repos under the OS temp directory and remove them in the test teardown.
- [ ] Add the `\r\n` and Windows-path rows; confirm they run on macOS and Linux.
- [ ] Export the public surface from `src/index.ts` (browser-safe: no `node:` import) and the probe from the
  node-only `./probe` entry that GE-1 declared, and write the package README: the module map, how to run the
  fixtures, and the rule that names come from the contract.
- [ ] Reconcile the fixtures: if a row cannot be satisfied, stop and raise it against GE-1\'s contract rather than
  editing the row (the Rust twin reads the same row).
- [ ] Update this plan\'s checkboxes and fill in the notes below.

## Gates

```bash
npm test -w @quarto/quarto-git-epochs
npm run typecheck -w @quarto/quarto-git-epochs
npm run build -w @quarto/quarto-git-epochs
cargo xtask lint
```

`cargo xtask lint` must stay green (in particular `ci-test-suite-unwired`, which GE-1 satisfied). Tests follow
`.claude/rules/hub-client-tests.md`: nothing started by a test outlives it, and every wait is on the state asserted.

## End-to-end verification

The package is a library with no binary of its own, so the end to end is the real-repo scenario run: run the scenario
step lists against real temp repos with the real `git`, and record in the notes the exact command, the number of
scenarios, and a snippet of the classifier output after a `git switch` step and a `git commit` step. Say that the
output was inspected. Nothing here runs under VS Code; that is GE-9.

## Exit criteria

- Every shared fixture row, the golden-doc test, the controller tests, and the scenario runs pass.
- The package has no `vscode` import and no real timer outside the injected clock.
- The doc this package writes is committed as a golden fixture; the check that the Rust crate decodes it is GE-14\'s.
- The gates above are green.

## Out of scope

- The extension wiring, the Binding, buffers, and notices (GE-9, GE-10, GE-12).
- The swap module and the aggregator ([GE-6](ge-06-swap-module.md)).
- The Rust crate and the hub ([GE-2](ge-02-rust-logic-probe-controller.md), [GE-3](ge-03-hub-integration.md)).
- Windows runs (GE-15, GE-16); this plan only makes the inputs Windows-shaped.

## Notes from the run

Not started.
