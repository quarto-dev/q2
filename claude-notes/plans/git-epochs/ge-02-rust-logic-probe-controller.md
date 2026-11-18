---
title: 'GE-2: Rust crate: pure logic, git probe, gate and quiescence controller'
date: 2026-11-18
description: 'Creates the quarto-git-epochs Rust crate holding the classifier, arrival rule, startup rule, registry operations, a subprocess git probe, and a tick-driven gate controller, all tested against the shared fixtures and real temporary repositories.'
status: approved
depends-on: [ge-1]
epic: ../2026-11-18-git-epochs-epic.md
---

**Epic:** [git epochs](../2026-11-18-git-epochs-epic.md) · **Waits for:** [GE-1](ge-01-contract-and-gate-0.md) ·
**Unblocks:** [GE-3](ge-03-hub-integration.md) ·
**Branch:** `ge/02-rust-logic-probe-controller` off `feature/git-epochs`, merged by rebase and fast-forward, no push.

## Goal

Build the Rust half of the shared logic as a small crate with no samod and no hub dependency, so its tests are light
and the hub (GE-3) can adopt it. The crate has the same module shape as the TypeScript package that GE-5 writes:
`classify`, `arrival`, `startup`, `registry`, `probe`, `controller`. The pure modules take and return plain data. The
controller is a state machine driven by explicit ticks over a `GitProbe` trait; it returns actions and never touches
Automerge or the hub.

Two behaviors must be proven against real git, not fakes: the probe never runs repository-configured programs (the
review found that `git hash-object --path` runs a clean filter), and the controller sees a `git reset --hard` that
leaves HEAD and the blob unchanged.

## Decisions implemented

- **G1:** the trigger is a reflog-tail change; decide on the first quiet tick (1 s, cap 30 s); changed files are held.
- **G2:** the classifier: content rule first, reflog subject as tiebreaker, jj and no-reflog use content alone.
- **G3, G4:** the markers, resolved once with `git rev-parse --git-path`; lock files stale after 60 s, state markers
  pause indefinitely; plumbing only, `GIT_OPTIONAL_LOCKS=0`, `cat-file --batch`, timeouts, an off-switch, colocated jj.
- **G7, G8, G11, G12:** canonical selection, generation, move forward, the unavailable rule, the rename-invariant fork,
  the case-collision fallback, the unborn-HEAD refusal.
- **G9, G10:** arrival (text and binary, ReadOnly and WriteBack variants) and the startup rule.
- **G29:** two implementations of one module shape, fixtures shared by relative path.

## Context

- The workspace takes every `crates/*` directory as a member (`Cargo.toml`, `members = ["crates/*", ...]`), so a new
  directory under `crates/` joins automatically; `AGENTS.md` lists the crates by hand and needs a line.
- Crates already include files from outside their own root by relative path: `crates/quarto-wasm-embed/src/lib.rs:153`
  includes `../../../ts-packages/quarto-hub-mcp/scripts/wasm-host-entry.mjs`, and
  `crates/quarto-core/tests/integration/toc_js_pipeline.rs:33` includes `../../../../resources/js/quarto-toc/quarto-toc.js`.
  From `crates/quarto-git-epochs/tests/integration/x.rs` the repository root is `../../../../`.
- `quarto_util::to_forward_slashes` is at `crates/quarto-util/src/path.rs:23`.
- The fixtures, the golden documents, and the contract come from GE-1:
  `ts-packages/quarto-git-epochs/fixtures/` and `claude-notes/designs/git-epoch-registry-contract.md`.
- Integration tests go in `crates/<crate>/tests/integration/<name>.rs` with one `main.rs`; never a top-level
  `tests/<name>.rs` (`.claude/rules/integration-tests.md`). Platform rules: `.claude/rules/cross-platform.md`.

## Test specifications

All written first and seen failing for the right reason.

- **`classify`:** every case in `fixtures/decision-tables/classify.json`, including `reset --hard` with an unchanged
  blob (reflog tail changed, HEAD the same), a merge commit, and save-then-commit (plain `commit` reflog, disk differs
  from the last synced content, verdict: author edit, move forward).
- **`arrival`:** every case in `arrival.json` for both peers and both policies; the ReadOnly case where disk differs
  from the doc yields seed-from-disk; the already-attached case yields a re-baseline and no merge; binary cases compare
  byte hashes.
- **`startup`:** the three cases in `startup.json` (HEAD unchanged; HEAD changed; no record, which never merges).
- **`registry`:** every case in `registry.json`; and the golden documents: reading each golden document yields the
  expected entries, and applying the crate\'s operation to the "before" document gives a document whose decoded entries
  equal the "after" document (logical equality; bytes may differ).
- **`controller` with a fake probe, no sleeps:** a tick sequence for each of: gate closes on `index.lock`, a lock that
  outlasts 60 s resumes with a warning, a state marker (`rebase-merge`) pauses indefinitely until removed, HEAD moves
  then the decision waits for 1 s of quiet, the 30 s cap forces a decision, an unresolvable HEAD means no change and is
  rechecked each tick, files whose blob changed are held from the trigger to the decision.
- **Real repositories:** a scenario runner reads `fixtures/scenarios/*.json`, builds a temp repo, runs each git step and
  file write, and asserts the expected verdicts. Plus specific probe tests: unborn HEAD (`rev-parse --verify -q HEAD`
  exits 1, no change); `ls-tree -r -z` limited to a subdirectory prefix; a linked worktree (marker paths resolve per
  worktree); a symlink (mode 120000) and a submodule (160000) are filtered out; a stopped rebase shows the marker.
- **No repository programs run:** configure a clean filter and a `core.fsmonitor` in the temp repo that touch a marker
  file, run the whole probe, and assert the marker file does not exist.
- **jj:** colocated scenarios run when `jj` is on `PATH` and skip with a message otherwise.

## Checklist

### Crate and pure modules

- [ ] Create `crates/quarto-git-epochs` (edition and workspace fields as its siblings), depending only on `automerge`,
  `serde`/`serde_json`, `thiserror`, `tracing`, and `quarto-util`. Add its line to the crate list in `AGENTS.md`.
- [ ] Tests first: `tests/integration/main.rs` with one module per area (`classify`, `arrival`, `startup`, `registry`,
  `golden`, `controller`, `probe_real_repo`, `scenarios`), each including its fixture with `include_str!` or
  `include_bytes!` by the relative path above. Run the audit grep from `integration-tests.md` on the new files.
- [ ] Implement `classify` (G2).
- [ ] Implement `arrival` (G9): an input struct with the disk content, the doc content, the blob content, the attached
  state, the policy, and the content kind; an action enum with create, attach silently, re-baseline, join with notice,
  detach, and seed from disk.
- [ ] Implement `startup` (G10).
- [ ] Implement `registry`: the plain-data operations (canonical, register, move forward, fork-needed, case
  collision, unavailable rule) and, in a `doc` submodule, reading and writing the sidecar schema over
  `automerge::AutoCommit` exactly as the contract says (G7, G8, G11, G12).

### Probe

- [ ] Define `trait GitProbe` (HEAD, markers, reflog since a recorded HEAD, blob map for a prefix, blob content) and
  implement `SubprocessProbe`: `GIT_OPTIONAL_LOCKS=0`, `rev-parse --verify -q HEAD`, markers (`index.lock`,
  `rebase-merge`, `rebase-apply`, `MERGE_HEAD`, `CHERRY_PICK_HEAD`, `REVERT_HEAD`, `sequencer`, `BISECT_LOG`, and jj\'s
  `.jj/working_copy/working_copy.lock`) resolved once with `rev-parse --git-path`, `ls-tree -r -z HEAD` limited to the
  prefix from `rev-parse --show-prefix` with modes 100644 and 100755 only, `cat-file --batch` for content, no
  `hash-object`, no filters.
- [ ] A timeout per call (use a helper the workspace already depends on; check `Cargo.lock` before adding one)
  (detail chosen by the plan author; confirm). A missing git or a refused repository turns the probe off with one log
  line and the controller reports "off".
- [ ] Convert paths with `quarto_util::to_forward_slashes` at the boundary; registry keys are always `/` separated.

### Controller

- [ ] Implement the tick-driven state machine (gate, pending, hold, quiescence, cap, stale lock, indefinite marker
  pause, unresolvable HEAD, resume) returning actions: hold these paths, release these paths, decide with this verdict
  and these blob maps, report paused or off.
- [ ] Constants for the numbers in G32 live in one module and are asserted by a test, so GE-3 and the status route
  read them from one place.

### Verification

- [ ] All fake-probe tests pass with no sleeps; the real-repo and scenario tests pass; the no-repository-programs test
  passes; jj scenarios pass or skip.
- [ ] `cargo clippy -p quarto-git-epochs --all-targets --target x86_64-pc-windows-msvc -- -D warnings` is clean, and
  any helper used only by unix-gated tests follows the pairing rule in `cross-platform.md`. Symlink tests need
  `core.symlinks`; gate that one setup helper with `#[cfg(unix)]` and a `#[cfg(not(unix))]` variant that skips
  (detail chosen by the plan author; confirm).

## Gates

- `cargo clippy -p quarto-git-epochs --all-targets -- -D warnings`
- `cargo nextest run -p quarto-git-epochs`
- `cargo clippy -p quarto-git-epochs --all-targets --target x86_64-pc-windows-msvc -- -D warnings`
- `cargo xtask lint`; `q2 render --strict` on this file.
- The crate has no dependents yet, so no wider run is needed before GE-3.

## End-to-end verification

Not user-visible: the crate has no binary. The real-repository tests are the end-to-end part, and the scenario runner
exercises the probe, controller, and classifier together against actual git. Their output is recorded in Notes from the
run.

## Exit criteria

The crate builds and its tests pass on this host; the Windows cross-clippy is clean (or the limitation is noted); the
shared fixtures and golden documents pass; the probe provably runs no repository-configured program; `AGENTS.md` lists
the crate.

## Out of scope

Any use of Automerge docs beyond the registry schema, samod, the hub, the CLI flag, and the TypeScript twin (GE-5).

## Notes from the run

Not started.
