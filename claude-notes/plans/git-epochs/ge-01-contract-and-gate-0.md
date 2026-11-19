---
title: 'GE-1: Contract and Gate 0'
date: 2026-11-18
description: 'Runs the Gate 0 checks that the contract depends on, writes the normative registry and index contract, adds the version 4 index schema in TypeScript and Rust, creates the shared fixtures and the quarto-git-epochs package skeleton, and runs the remaining Gate 0 checks that decide how the epic proceeds.'
status: approved
depends-on: []
lane: A
epic: ../2026-11-18-git-epochs-epic.md
---

**Epic:** [git epochs](../2026-11-18-git-epochs-epic.md) · **Waits for:** nothing (the root of the graph) ·
**Unblocks:** [GE-2](ge-02-rust-logic-probe-controller.md), [GE-5](ge-05-ts-logic-probe-controller.md),
[GE-6](ge-06-swap-module.md), [GE-8](ge-08-extension-foundation.md) ·
**Branch:** `ge/01-contract-and-gate-0` off `feature/git-epochs`, merged by rebase and fast-forward, no push.

## Goal

Every later plan depends on three things written here: a normative contract for the registry sidecar and the new index
keys, shared fixtures that the Rust crate and the TypeScript package both run, and the Gate 0 answers to the questions the
review could not settle by reading code. If a Gate 0 check fails the epic stops and Gordon decides (D21); nothing
downstream starts on a guess.

The plan runs in this order because the contract and the fixtures encode Gate 0 answers: part one of Gate 0 (items 6 to
9: the registry in Automerge, the git ordering, forking, and registry size) runs first, then the contract, the package,
the schema, and the fixtures, then part two of Gate 0 (the checks that need a network, a VS Code download, or a running
hub). A part-one failure therefore invalidates nothing that was already written.

The plan also lands the small, risk-free code that the contract names: the version 4 index schema in TypeScript and
Rust, and the empty `@quarto/quarto-git-epochs` package with its CI and verify steps, so that GE-5 and GE-6 only add
tests and source files and never touch shared wiring.

## Decisions implemented

- **G6, G7, G8, G11, G12:** the registry schema (flat keys, the retired set, the repair rule) and its operations, as a
  contract plus golden documents.
- **G9, G10, G2:** arrival, startup, and classifier cases as decision tables (the logic itself is GE-2 and GE-5).
- **G18:** index version 4, with the sidecar pointer, `owner`, and `repo` keys.
- **G25, G26, G32, G33:** the moved-message shape, the swap events and the names of the notice constants, the numbers
  table, and the `git-status` route shape are written into the contract so both languages copy one source. The notice
  texts themselves live in GE-6 only.
- **G29:** the shared fixtures live in `ts-packages/quarto-git-epochs/fixtures/`.

## Context

- `ts-packages/quarto-automerge-schema/src/index.ts:14` sets `CURRENT_SCHEMA_VERSION = 3`; `IndexDocument` is at
  `:79-89`; `migrateIndexDocument` is at `:107-133` and returns without changes for any version it does not know.
- The Rust hub reads and writes the index by named key (`crates/quarto-hub/src/index.rs:29` defines `FILES_KEY`;
  accessors at `:150`, `:174`, `:192`, `:205`, `:216`; capture writes at `:274`; creation at `:467`). It reads a
  string value in either of two forms, because a JS client writes a string as a Text object and the Rust hub writes a
  scalar string (`index.rs:458`, bd-bm0vaetl). A JS client likewise reads a Rust-written scalar string as an
  `ImmutableString` (`ts-packages/quarto-sync-client/src/client.ts:97`, `:107`;
  `rust-binary-scalars.test.ts`). The second review reproduced both directions: a JS integer reads as `Int` in Rust,
  a NUL byte in a map key survives save and load, and a string put by JS arrives in Rust as `Text`.
- `createNewProject` builds the index inline as `{ files: {}, version: CURRENT_SCHEMA_VERSION, identities: {}, folders: {} }`
  (`ts-packages/quarto-sync-client/src/client.ts:2443`).
- CI wires a package\'s tests with a step such as `npm test -w ts-packages/quarto-automerge-schema`
  (`.github/workflows/ts-test-suite.yml:383-385`); the lint `ci-test-suite-unwired`
  (`crates/xtask/src/lint/ci_test_wiring.rs`, `EXCUSED` is empty at `:56`) fails any workspace package with a `test`
  script that no workflow step runs. `cargo xtask verify` steps 11 and 12 (`crates/xtask/src/verify.rs:785-830`) build
  and test a hardcoded list (the schema, the sync client, hub-mcp), and step 6 builds every `ts-packages/*` package, so
  a new package is built by verify but not tested by it.
- The sync client\'s test hub is `ts-packages/quarto-sync-client/src/test-hub.ts`; the share policy is
  `hub_share_policy()` at `crates/quarto-hub/src/context.rs:216`; `/health` returns `index_document_id` and an
  optional `project_root` (`crates/quarto-hub/src/server.rs:47-53`).
- `Repo.clone<T>(handle)` exists in automerge-repo `2.6.0-alpha.5` (`node_modules/@automerge/automerge-repo/dist/Repo.d.ts:82`);
  the second review ran it and got a new doc id with the history kept. Item 8 starts from it.

## Test specifications

Written first and seen failing for the right reason.

- **Schema (TypeScript):** `migrateIndexDocument` turns a version 3 doc into version 4 with no other change and
  reports `true`; a version 4 doc is untouched and reports `false`; a doc at version 5 is untouched and reports
  `false` (an older client leaves a newer doc alone). `createIndexDocumentContent()` returns exactly
  `files`, `version: 4`, `identities`, `folders` and nothing else, and equals `fixtures/golden/index-v4.json` (the
  TypeScript schema item writes that file). The new optional keys round-trip through an Automerge change.
- **Schema (Rust):** `IndexDocument` creation writes the same four keys as `createIndexDocumentContent()` (compared
  against `fixtures/golden/index-v4.json`). A permanent unit test builds an index with unknown top-level keys
  (the new keys and an invented `future_key`), runs every public mutator and accessor of `IndexDocument`, and asserts
  that the unknown keys are byte-for-byte unchanged afterward. Another test reads `epochs`, `owner`, and the `repo`
  fields written in the JS form (Text objects) and in the Rust form (scalar strings), and gets the same values.
- **Fixtures:** a vitest file `fixtures.test.ts` loads every decision table, scenario, and golden document and
  validates each row against the row schema in the contract; it fails if a case lacks an id, a peer, a policy where the epic requires one, or an
  expected value. It also checks `fixtures/vocabulary.json` (the event kinds and the swap events) against the contract
  table.
- **Golden documents:** each golden binary document loads with `@automerge/automerge` and its decoded entries equal the
  expected JSON in `fixtures/golden/manifest.json`.

## Checklist

### Gate 0, part one (before the contract)

Each check is a scratch experiment outside the repository (use the session scratchpad); its result goes in Notes from
the run as ran, read, or not done. **Stop rule:** if any check except 12 fails, stop, write the result, do not start GE-2,
GE-5, GE-6, or GE-8, and bring it to Gordon (D21). Do not degrade silently. Items 3, 11, and 13 are the exceptions that
record an answer instead of passing or failing (see part two).

- [x] **6.** In Automerge (JS and Rust, two actors), with the flat registry: concurrent creation of one key, and of
  different keys under a new path, both survive (the nested form loses one side; the second review reproduced it, so
  write these tests first and watch a nested variant fail); a concurrent delete and put of one key leaves the put; a
  retired entry is never removed. A NUL byte in a key and an integer generation survive a save and load in both
  languages (the second review saw both; this makes them permanent tests).
- [x] **7.** In temp repos, record the reflog subjects of `cherry-pick`, `revert`, `commit --amend`, `reset --hard`,
  `pull --ff-only`, `git stash` push, and a stopped `rebase`; whether `sequencer/` and `BISECT_LOG` appear; and the
  order of lock, file writes, and ref update for `switch`, `reset --hard`, `pull`, and `rebase` on a 3,000-file tree
  at millisecond resolution (a tight stat loop or `strace`/`dtruss`; the second review saw only 100 ms polling),
  including whether git releases `index.lock` before it writes the reflog; if it does, G1\'s 500 ms mtime skip
  applies. Record the git version. The second review found that `git stash` and a plain `git reset --hard` write the
  identical reflog subject (`reset: moving to HEAD`), that a repo with `core.logAllRefUpdates=false` has no `.git/logs`
  directory at all, and that `git rev-parse --git-path` gives per-worktree paths in a linked worktree; confirm each
  here and write them into the classify fixtures.
- [x] **8.** JS doc forking in automerge-repo `2.6.0-alpha.5`: `repo.clone(handle)` exists (`Repo.d.ts:82`) and the
  second review saw a new doc id with the history length kept on both sides; confirm that the fork keeps history, can
  diverge, and syncs through the test hub. If it does not sync, the fallback is a doc created from the content with no
  history; write that into the contract as the fork fallback (G12) and into the arrival fixtures.
- [x] **9.** A registry doc in the flat form for 5,000 and 15,000 entries with three moves each, and one for a repo
  with many large binary files: saved size and load time in JS and Rust. The second review measured about 110 KB and
  37 ms at 5,000 entries and 331 KB and 91 ms at 15,000 in JS, with no binary-heavy case; the binary-heavy case is the
  open part. Write the soft limit into the contract.

### Contract

- [ ] Write `claude-notes/designs/git-epoch-registry-contract.md` (qmd; render-check it), normative, with these
  sections. **Sidecar doc:** a `version` field (1); `entries`, a flat map from the key `path`, NUL, blob id, NUL, docId to
  the generation (an integer stored as Automerge `int`); and `retired`, a flat map with the same keys that only grows.
  Every entry is its own key, so concurrent adds union (G6; a nested map loses one side of a concurrent creation).
  **Scalar forms:** every string value the contract names (the sidecar pointer, `owner`, `repo.roots` items,
  `repo.prefix`) is read in either form, a scalar string or a Text object, because JS writes one and the Rust hub writes
  the other; writers write their platform\'s default form; every golden document in both forms is a fixture.
  **Index version 4 keys:** the sidecar pointer (proposed name `epochs`, holding the sidecar docId), `owner` (the
  originating folder\'s random `ownerId`, absent for hub-owned projects), and `repo` (`roots`, a list of root commit
  hashes, and `prefix`, the project path inside the repo). **Operations:** register, move forward (add the new blob
  carrying the doc\'s generation and remove the old blob in one change), canonical selection (lowest generation, then
  docId bytewise, skipping retired entries), creation (the maximum generation over the path on any blob, with the
  maximum of an empty path being -1, plus one, so the first doc of a path has generation 0, the same value that startup
  registration writes), register as put-if-absent, retire (the `reset --hard` step), the narrow repair rule
  (re-register a doc that has no entry on any blob and is not retired), the fork rule for the rename invariant and its
  fallback from item 8, the case-collision fallback (compare the lowercased path part of the key). **Arrival:** inputs
  (disk text or bytes, doc text or bytes, blob content, attached, policy, peer) and the outputs for the five steps, the
  `reset --hard` retire step, the hub\'s step 4 and step 5 behavior, the jj fallback, and the ReadOnly variant.
  **Surfaces:** the response of `GET /api/preview/git-status`,
  `{ tracking, paused, pending, markers, decisions }` (the marker names holding the gate, and the last 20 decisions as
  kind, time, files, G9 step, and `discarded`, the number of discarded edits a retire step set aside, zero otherwise);
  the event vocabulary shared by the hub\'s decision ring and the extension\'s `events.jsonl` (`flipped`, `switched`,
  `detached`, `reattached`, `held`, `reset`, `retired`, plus `moved-forward` for a commit-type decision, `joined` for the
  hub\'s step 4, and `seeded` for the hub\'s step 5 and the jj fallback); the swap events (`pending`, `done`,
  `unavailable`) and the names of the notice constants (their texts live in GE-6 only); the single moved message type
  (`{ type: 'moved', peerId }`, the same shape as the existing `leave` message in
  `hub-client/src/services/presenceService.ts:49-53`, with no reason field); the numbers table from the epic (G32).
  **Module map:** the six modules the Rust crate (GE-2) and the TypeScript package (GE-5) both build, with one line
  each and the entry-point function names of the pure core and its plain-data inputs and outputs: `classify`,
  `arrival`, `startup`, `registry`, `probe` (including the repo identity queries: root commits, shallow check, path
  prefix, and a cheap-state read in which a missing reflog file means "no reflog", not an error), and `controller`
  (the tick-driven machine and the synchronous `check()`). The two plans run in parallel and take their names from this
  map, not from each other. **Row schemas:** the input and expected fields of every decision-table row, the step
  format of a scenario, and the golden manifest, so that GE-2 and GE-5 parse the same rows.
- [ ] Verify the epic\'s "What became of D1 to D36" table against the contract and fix any row that is wrong; record
  each change in Notes from the run.

### Package skeleton and CI

- [ ] Create `ts-packages/quarto-git-epochs` modeled on `quarto-automerge-schema`: `package.json` named
  `@quarto/quarto-git-epochs` (private, `type: module`, scripts `build`, `typecheck`, `test`), `tsconfig.json`,
  `vitest.config.ts`, `src/index.ts` exporting only a version constant, an `exports` map with a browser-safe entry (`.`: the pure logic and
  the swap module) and a node-only `./probe` entry (the probe uses `node:child_process`; hub-client and the browser
  clients import only `.`), and the `fixtures.test.ts` above (red until the fixtures item).
- [ ] Add a step `npm test -w ts-packages/quarto-git-epochs` to `.github/workflows/ts-test-suite.yml` next to the
  schema step, in the same commit as the package, and add the package to the build step at
  `.github/workflows/ts-test-suite.yml:272` (it builds only the sync client and the schema, and GE-7 makes the sync
  client depend on this package). In the same commit add the counterpart to `cargo xtask verify` (`AGENTS.md`: verify
  mirrors CI): extend step 12 in `crates/xtask/src/verify.rs` so it also runs the tests of
  `quarto-automerge-schema` (CI-only today) and `quarto-git-epochs`. Run `cargo xtask lint` to see
  `ci-test-suite-unwired` pass.
- [ ] Run `npm install` from the repository root (never from the package) and confirm the root `npm run typecheck`
  and `npm run build` still pass.

### Schema, TypeScript

- [ ] Tests first (see Test specifications), then raise `CURRENT_SCHEMA_VERSION` to 4, add the V3 to V4 step to
  `migrateIndexDocument` as a pure version bump, add the optional `epochs`, `owner`, and `repo` fields and their types
  to `IndexDocument`, and add `createIndexDocumentContent()`. Write `fixtures/golden/index-v4.json` from it.
- [ ] Do not change `createNewProject` here: adopting the helper touches `quarto-sync-client` and so the MCP bundle;
  GE-7 does it (detail chosen by the plan author; confirm).
- [ ] `npm test -w ts-packages/quarto-automerge-schema` and `npm run typecheck -w ts-packages/quarto-automerge-schema`.

### Schema, Rust

- [ ] Tests first. Read `index.rs` creation (`:467`) and make it write `files`, `version` 4, `identities`, and
  `folders`; add the key constants and typed accessors for `epochs`, `owner`, and `repo` (GE-3 uses them); the
  accessors read both string forms (see Scalar forms in the contract).
- [ ] The unknown-keys test enumerates every `pub fn` on `IndexDocument` that reads or writes the doc (`add_file`,
  `remove_file`, `has_file`, the capture setters, and any folder or identity function) and calls each one.
- [ ] `cargo clippy -p quarto-hub --all-targets -- -D warnings` and `cargo nextest run -p quarto-hub`.

### Fixtures

- [ ] `fixtures/decision-tables/classify.json`: commit, amend, checkout, pull, `reset --hard` with an unchanged blob,
  a merge commit, a conflict-resolution commit, save-then-commit (plain `commit` reflog with a differing disk),
  colocated jj (content rule only), a reflog span of `switch` then `commit` (checkout-type), `git stash` push (a
  reflog entry with HEAD and blob unchanged, the same subject as `reset --hard`), and a repo with no reflog.
- [ ] `fixtures/decision-tables/arrival.json`: the steps for text and for binary (the already-attached case split into re-baseline and the `reset --hard` retire
  step),
  the ReadOnly variant (attach only when disk equals the doc; otherwise seed from disk), the unavailable canonical
  doc, the rename-invariant fork and its no-history fallback, a case-insensitive collision, a doc another peer retired, the hub at steps 4 and 5, and
  every changed file under colocated jj. Each case carries `peer` (`hub` or `extension`) and,
  where it matters, `policy` (`ReadOnly` or `WriteBack`).
- [ ] `fixtures/decision-tables/startup.json` (the three cases) and `registry.json` (creation race on one key and on
  different keys under a new path, equal generations, an established doc beating a latecomer, the first doc of a path
  getting generation 0 and the next getting the maximum plus one, move forward carrying the generation and with an
  absent old entry, two tips after `git add -p`, concurrent put and delete, retire, repair).
- [ ] `fixtures/vocabulary.json`: the event kinds and the swap events, the one list that the Rust enum (GE-2) and the
  TypeScript union (GE-5) are tested against.
- [ ] `fixtures/scenarios/*.json`: step lists of git commands and file writes (no shell scripts, so Windows can run
  them), each step with an optional expected classifier verdict: commit, amend, switch and switch back, pull,
  rebase stopped for conflicts, `reset --hard`, `git stash` and `stash pop`, save-then-commit, a file added and removed by a checkout, a binary
  changed by a checkout, `core.autocrlf` checkouts with `\r\n` content.
- [ ] `fixtures/golden/`: binary sidecar documents generated by `scripts/make-golden.mjs` and committed (before and
  after a move forward, a creation race merged from two actors on one key and on a new path, a concurrent put and delete, a retire, a repair), with
  `manifest.json` of the expected decoded entries. `index-v4.json` already exists from the schema item. GE-2 and GE-5
  each add one document written by their own implementation; GE-14 cross-checks them.

### Gate 0, part two

- [ ] **1.** A Node script using the extension\'s own adapter (`BrowserWebSocketClientAdapter`) connects to a
  `q2 preview` hub\'s `/ws`, directly and through `q2 preview --join <ticket> --port N --no-browser`, and exchanges
  docs both ways under the announce-off policy: a doc created while connected reaches the hub and a second client
  can find it by id; a hub-created doc is fetchable by id; a sidecar-shaped doc travels both ways. The script uses a
  raw `Repo`, as the extension does, and also records whether the client sees the hub\'s heads for its own docs
  (`getSyncInfo` and the `remote-heads` event), because GE-12\'s heads confirmation is built on that.
- [ ] **2.** The negative case: a doc created while disconnected cannot be found by a second client within 5 s (the
  hub cannot pull it); and the adapter reconnects and resumes after the `--join` proxy is killed and restarted.
- [ ] **3.** `GET /health` through the preview returns `index_document_id` and `project_root` and needs no credential.
  This one has a pass criterion: if it needs a credential, GE-11 changes and the epic stops for Gordon.
- [ ] **4.** Publish a throwaway project to `wss://sync.automerge.org` with a JS client, run
  `q2 preview --hub-url wss://sync.automerge.org --project <id>` (this covers TLS), confirm it loads, then create a doc
  later and re-point a file at it from the JS client and confirm a client of the mirror follows. Record also whether a
  raw `Repo` client sees remote heads from the public server (as in item 1).
- [ ] **5.** The Rust unknown-keys test passes (see Test specifications) and a read of `index.rs` finds no accessor that
  rebuilds the document.
- [ ] **10.** `git` is on `PATH` where the extension runs, from a real extension host (a scratch extension launched with
  `@vscode/test-electron`, since GE-8\'s harness does not exist yet; item 12 uses the same one); the
  remote-development case is noted as not done unless a remote is available.
- [ ] **11.** Whether `q2 preview` without `--allow-edit` renders a doc edit made by a JS client in the browser (the
  preview may render from docs or from disk). This item records an answer; either answer proceeds, and GE-13 S11 and the
  docs follow it.
- [ ] **12.** Editor-only timing: in a real VS Code with the scratch extension from item 10 (it logs
  `onDidChangeTextDocument` times), run `git switch` on a tree of a few thousand files in the open folder and measure
  the time from `index.lock` release to the last buffer reload event, on this host. This item has its own outcome: a
  reload later than the 1 s quiet window does not stop the epic; record it, and editor-only ships as experimental
  (G20). GE-17\'s `c2` runs GE-9\'s timing test on Windows.
- [ ] **13.** Whether a new `q2 preview` run on the same directory gets a new index doc id (the basis of G19): start
  it twice and compare `/health`. This item records an answer; if the id is the same, G19\'s reset-on-rebind still
  applies and the join prompt wording changes.

### Wrap-up

- [ ] Commit in logical pieces (Gate 0 part one notes; contract; package and CI; schema; fixtures). The commit message
  of the fixtures commit lists the fixture files added. `cargo xtask lint` is green.

## Gates

- `cargo clippy -p quarto-hub --all-targets -- -D warnings` and `cargo nextest run -p quarto-hub`.
- `npm test -w ts-packages/quarto-automerge-schema`, `npm test -w ts-packages/quarto-git-epochs`, and
  `npm run typecheck` for both packages.
- `cargo xtask lint`; `q2 render --strict claude-notes/designs/git-epoch-registry-contract.md` and this file.
- The Windows cross-clippy for `quarto-hub` where the build allows; if it cannot cross-check, say so in the notes.

## End-to-end verification

The product behavior is not user-visible in this plan. The Gate 0 experiments are the end-to-end part: they run
through the real `q2 preview`, `q2 preview --join`, and `q2 preview --hub-url` binaries, and their invocations and
inspected output go in Notes from the run.

## Exit criteria

The contract is written and render-clean; the schema, package, CI and verify steps, and fixtures are committed with
green gates; all thirteen Gate 0 results are recorded; none failed, or Gordon has decided what to do about each that did.

## Out of scope

The logic itself (GE-2, GE-5), the swap module and the notice texts (GE-6), adopting the helper in `createNewProject`
(GE-7), and any hub behavior change.

## Notes from the run

**Session 1 (lane A, 2026-11-18).**

- Step 0 done: `feature/git-epochs` rebased onto `origin/main` (9 commits replayed over 47 new ones, no conflicts;
  the only conflict-marker lines in the tree are quoted text in
  `claude-notes/research/2026-05-19-attribution-merge-briefing.md`, identical to `origin/main`). `cargo build
  --workspace` passed on the branch in workspace-7 (exit 0).
- Gate 0 item 7, partly done (git 2.55.0, temp repo): reflog subjects are `commit (amend): <msg>`,
  `revert: Revert "<subject>"`, `cherry-pick: <subject>`, `checkout: moving from X to Y` (for `switch`), and
  `reset: moving to HEAD` for both `git stash` and a plain `git reset --hard`, so the two are indistinguishable by
  subject, as the second review found. A repo with `core.logAllRefUpdates=false` has no `.git/logs`. In a linked
  worktree, `git rev-parse --git-path logs/HEAD` and `index.lock` resolve under `.git/worktrees/<name>/`.
  Remainder, same host: `pull --ff-only` logs `pull -q --ff-only: Fast-forward` (the subject carries the arguments
  as typed); a stopped `rebase` logs `rebase (start): checkout main` and leaves `.git/rebase-merge/`; a stopped
  single-commit `cherry-pick` leaves `CHERRY_PICK_HEAD` (a `sequencer/` directory is only expected for multi-commit
  picks, not checked); `git bisect start` leaves `BISECT_LOG` (and `BISECT_START`, `BISECT_TERMS`, and others). A
  stopped `revert` was not exercised (my test did not conflict). Millisecond ordering, 3,000-file tree, polling
  `stat` in a tight loop: `index.lock` appears first (about 10 ms), working files change next, then the lock is
  released and `.git/index` changes in the same millisecond, and the `logs/HEAD` write follows the lock release by
  0.7 to 1.3 ms (`switch`, `reset --hard`; `rebase` repeats this per step). So git does release the lock before it
  writes the reflog, and G1's 500 ms mtime skip applies. For `reset --hard` the branch ref write follows the reflog
  by about 0.1 ms; `switch` writes no branch ref (only HEAD). `pull --ff-only` was a near-instant no-op in my run, so
  its ordering is not measured.
- Gate 0 item 6, JS side, ran (`@automerge/automerge` 3.5.0, two actors, scratch script, 9 checks, all as expected):
  two docs for one path and blob both survive; identical keys from both sides merge to one; different keys under a new
  path both survive in the flat form while the nested form loses one side (demonstrated); a concurrent delete and put
  of one key leaves the put, in both merge orders; a retired key survives a concurrent unrelated delete; a NUL byte in
  a key and an `Int` generation survive save and load (an `Int` and a plain integer both read back as a JS number).
  **Contract consequence:** the `entries` and `retired` maps themselves must be created exactly once, by the writer
  that creates the sidecar doc (my first draft of the test created the base twice and reproduced the nested loss at
  the map level). The Rust side of item 6 becomes permanent tests in `quarto-hub` and the TypeScript side permanent
  tests in the package; neither is written yet, so item 6 is **half done** (JS ran, Rust not).
- Gate 0 item 8 ran (automerge-repo `2.6.0-alpha.5`, two client repos and a websocket hub in one process):
  `repo.clone(handle)` returns a new doc id and the full history (3 changes on both sides); the fork and the original
  diverge independently; a second client finds the fork over the network, sees its content and history, and an edit
  from that client reaches the first client's fork without touching the original. No fallback is needed.
- Gate 0 item 9 ran in JS only (`save`/`load`, random git blob ids, 50 directories, `.qmd` and `.png` paths): 5,000
  entries 231 KB (no moves), 648 KB after three moves each, save 30 ms, load 38 ms; 15,000 entries after three
  moves 1,947 KB, save 96 ms, load 98 ms (64-hex ids: 1,965 KB, 103 ms); 2,000 entries 259 KB, 13 ms. These are
  larger than the second review\'s figures (110 KB at 5,000) because my keys carry incompressible random ids. A
  repo with many large binaries costs the same as one with many small files, since a key holds the blob id and not
  the content; only the number of tracked paths matters. Proposed soft limit for the contract: 15,000 tracked paths
  (about 2 MB and 100 ms load); above it the sidecar warns and still works. Rust timing was not measured.
- Not started: Gate 0 part two; the contract, package, schemas, fixtures. No repository code changed yet.
