---
title: 'GE-1: Contract and Gate 0'
date: 2026-11-18
description: 'Writes the normative registry and index contract, adds the version 4 index schema in TypeScript and Rust, creates the shared fixtures and the quarto-git-epochs package skeleton, and runs the eleven Gate 0 checks that decide whether the epic proceeds.'
status: approved
depends-on: []
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

The plan also lands the small, risk-free code that the contract names: the version 4 index schema in TypeScript and
Rust, and the empty `@quarto/quarto-git-epochs` package with its CI step, so that GE-5 and GE-6 only add tests and
source files and never touch shared wiring.

## Decisions implemented

- **G6, G7, G8, G11, G12:** the registry schema and its operations, as a contract plus golden documents.
- **G9, G10, G2:** arrival, startup, and classifier cases as decision tables (the logic itself is GE-2 and GE-5).
- **G18:** index version 4, with the sidecar pointer, `owner`, and `repo` keys.
- **G25, G26, G32, G33:** the moved-message type, the swap events and notice strings, the numbers table, and the
  `git-status` route shape are written into the contract so both languages copy one source.
- **G29:** the shared fixtures live in `ts-packages/quarto-git-epochs/fixtures/`.

## Context

- `ts-packages/quarto-automerge-schema/src/index.ts:14` sets `CURRENT_SCHEMA_VERSION = 3`; `IndexDocument` is at
  `:79-89`; `migrateIndexDocument` is at `:107-133` and returns without changes for any version it does not know.
- The Rust hub reads and writes the index by named key (`crates/quarto-hub/src/index.rs:29` defines `FILES_KEY`;
  accessors at `:150`, `:174`, `:192`, `:205`, `:216`; capture writes at `:274`; creation at `:467`).
- `createNewProject` builds the index inline as `{ files: {}, version: CURRENT_SCHEMA_VERSION, identities: {}, folders: {} }`
  (`ts-packages/quarto-sync-client/src/client.ts:2443`).
- CI wires a package\'s tests with a step such as `npm test -w ts-packages/quarto-automerge-schema`
  (`.github/workflows/ts-test-suite.yml:383-385`); the lint `ci-test-suite-unwired`
  (`crates/xtask/src/lint/ci_test_wiring.rs`, `EXCUSED` is empty at `:56`) fails any workspace package with a `test`
  script that no workflow step runs.
- The sync client\'s test hub is `ts-packages/quarto-sync-client/src/test-hub.ts`; the share policy is
  `hub_share_policy()` at `crates/quarto-hub/src/context.rs:216`; `/health` returns `index_document_id` and an
  optional `project_root` (`crates/quarto-hub/src/server.rs:47-53`).

## Test specifications

Written first and seen failing for the right reason.

- **Schema (TypeScript):** `migrateIndexDocument` turns a version 3 doc into version 4 with no other change and
  reports `true`; a version 4 doc is untouched and reports `false`; a doc at version 5 is untouched and reports
  `false` (an older client leaves a newer doc alone). `createIndexDocumentContent()` returns exactly
  `files`, `version: 4`, `identities`, `folders` and nothing else. The new optional keys round-trip through an
  Automerge change.
- **Schema (Rust):** `IndexDocument` creation writes the same four keys as `createIndexDocumentContent()` (compared
  against `fixtures/golden/index-v4.json`). A permanent unit test builds an index with unknown top-level keys
  (the new keys and an invented `future_key`), runs every public mutator and accessor of `IndexDocument`, and asserts
  that the unknown keys are byte-for-byte unchanged afterward.
- **Fixtures:** a vitest file `fixtures.test.ts` loads every decision table, scenario, and golden document and
  validates its structure; it fails if a case lacks an id, a peer, a policy where the epic requires one, or an
  expected value.
- **Golden documents:** each golden binary document loads with `@automerge/automerge` and its decoded entries equal the
  expected JSON in `fixtures/golden/manifest.json`.

## Checklist

### Contract

- [ ] Write `claude-notes/designs/git-epoch-registry-contract.md` (qmd; render-check it), normative, with these
  sections. **Sidecar doc:** a `version` field (1) and `entries`, a map of path to blob id to docId to generation
  (an integer); the key set is a union, so concurrent adds both survive (detail chosen by the plan author; confirm).
  **Index version 4 keys:** the sidecar pointer (proposed name `epochs`, holding the sidecar docId), `owner` (the
  originating folder\'s random `ownerId`, absent for hub-owned projects), and `repo` (`roots`, a list of root commit
  hashes, and `prefix`, the project path inside the repo). **Operations:** register, move forward (add the new blob
  carrying the doc\'s generation and remove the old blob in one change), canonical selection (lowest generation, then
  docId), creation (generation is max seen plus one), the fork rule for the rename invariant, the case-collision
  fallback. **Arrival:** inputs (disk text or bytes, doc text or bytes, blob content, attached, policy, peer) and the
  outputs for the five steps and the ReadOnly variant. **Surfaces:** the response of
  `GET /api/preview/git-status`, `{ tracking, paused, pending }`; the swap events (`pending`, `done`, `unavailable`)
  and the notice strings; the moved message type; the numbers table from the epic (G32).
- [ ] Verify the epic\'s "What became of D1 to D36" table against the contract and fix any row that is wrong; record
  each change in Notes from the run.

### Schema, TypeScript

- [ ] Tests first (see Test specifications), then raise `CURRENT_SCHEMA_VERSION` to 4, add the V3 to V4 step to
  `migrateIndexDocument` as a pure version bump, add the optional `epochs`, `owner`, and `repo` fields and their types
  to `IndexDocument`, and add `createIndexDocumentContent()`.
- [ ] Do not change `createNewProject` here: adopting the helper touches `quarto-sync-client` and so the MCP bundle;
  GE-7 does it (detail chosen by the plan author; confirm).
- [ ] `npm test -w ts-packages/quarto-automerge-schema` and `npm run typecheck -w ts-packages/quarto-automerge-schema`.

### Schema, Rust

- [ ] Tests first. Read `index.rs` creation (`:467`) and make it write `files`, `version` 4, `identities`, and
  `folders`; add the key constants and typed accessors for `epochs`, `owner`, and `repo` (GE-3 uses them).
- [ ] The unknown-keys test enumerates every `pub fn` on `IndexDocument` that reads or writes the doc (`add_file`,
  `remove_file`, `has_file`, the capture setters, and any folder or identity function) and calls each one.
- [ ] `cargo clippy -p quarto-hub --all-targets -- -D warnings` and `cargo nextest run -p quarto-hub`.

### Package skeleton and CI

- [ ] Create `ts-packages/quarto-git-epochs` modeled on `quarto-automerge-schema`: `package.json` named
  `@quarto/quarto-git-epochs` (private, `type: module`, scripts `build`, `typecheck`, `test`), `tsconfig.json`,
  `vitest.config.ts`, `src/index.ts` exporting only a version constant, and the `fixtures.test.ts` above.
- [ ] Add a step `npm test -w ts-packages/quarto-git-epochs` to `.github/workflows/ts-test-suite.yml` next to the
  schema step, in the same commit as the package, and run `cargo xtask lint` to see `ci-test-suite-unwired` pass.
- [ ] Run `npm install` from the repository root (never from the package) and confirm the root `npm run typecheck`
  and `npm run build` still pass.

### Fixtures

- [ ] `fixtures/decision-tables/classify.json`: commit, amend, checkout, pull, `reset --hard` with an unchanged blob,
  a merge commit, a conflict-resolution commit, save-then-commit (plain `commit` reflog with a differing disk),
  colocated jj (content rule only).
- [ ] `fixtures/decision-tables/arrival.json`: the five steps for text and for binary, the already-attached case,
  the ReadOnly variant (attach only when disk equals the doc; otherwise seed from disk), the unavailable canonical
  doc, the rename-invariant fork, a case-insensitive collision. Each case carries `peer` (`hub` or `extension`) and,
  where it matters, `policy` (`ReadOnly` or `WriteBack`).
- [ ] `fixtures/decision-tables/startup.json` (the three cases) and `registry.json` (creation race with equal
  generations, an established doc beating a latecomer, move forward carrying the generation, two tips after
  `git add -p`, concurrent put and delete).
- [ ] `fixtures/scenarios/*.json`: step lists of git commands and file writes (no shell scripts, so Windows can run
  them), each step with an optional expected classifier verdict: commit, amend, switch and switch back, pull,
  rebase stopped for conflicts, `reset --hard`, save-then-commit, a file added and removed by a checkout, a binary
  changed by a checkout, `core.autocrlf` checkouts with `\r\n` content.
- [ ] `fixtures/golden/`: binary sidecar documents generated by `scripts/make-golden.mjs` and committed (before and
  after a move forward, a creation race merged from two actors, a concurrent put and delete), with
  `index-v4.json` and a `manifest.json` of the expected decoded entries.

### Gate 0

Each check is a scratch experiment outside the repository (use the session scratchpad); its result goes in Notes from
the run as ran, read, or not done. **Stop rule:** if any check fails, stop, write the result, do not start GE-2, GE-5,
GE-6, or GE-8, and bring it to Gordon (D21). Do not degrade silently.

- [ ] **1.** A Node script using the extension\'s own adapter (`BrowserWebSocketClientAdapter`) connects to a
  `q2 preview` hub\'s `/ws`, directly and through `q2 preview --join <ticket> --port N --no-browser`, and exchanges
  docs both ways under the announce-off policy: a doc created while connected reaches the hub and a second client
  can find it by id; a hub-created doc is fetchable by id; a sidecar-shaped doc travels both ways.
- [ ] **2.** The negative case: a doc created while disconnected cannot be found by a second client within 5 s (the
  hub cannot pull it); and the adapter reconnects and resumes after the `--join` proxy is killed and restarted.
- [ ] **3.** `GET /health` through the preview returns `index_document_id` and `project_root` and needs no credential.
- [ ] **4.** Publish a throwaway project to `wss://sync.automerge.org` with a JS client, run
  `q2 preview --hub-url wss://sync.automerge.org --project <id>` (this covers TLS), confirm it loads, then create a doc
  later and re-point a file at it from the JS client and confirm a client of the mirror follows.
- [ ] **5.** The Rust unknown-keys test passes (see Test specifications) and a read of `index.rs` finds no accessor that
  rebuilds the document.
- [ ] **6.** In Automerge (JS and Rust, two actors): a concurrent put and delete of one registry key leaves a
  deterministic result and the put survives; concurrent puts of different docIds under one blob both survive.
- [ ] **7.** In temp repos, record the reflog subjects of `cherry-pick`, `revert`, `commit --amend`, `reset --hard`,
  `pull --ff-only`, and a stopped `rebase`; whether `sequencer/` and `BISECT_LOG` appear; and the order of lock, file
  writes, and ref update for `reset --hard`, `pull`, and `rebase` on a 3,000-file tree. Record the git version.
- [ ] **8.** JS doc forking in automerge-repo `2.6.0-alpha.5`: try `repo.clone`; if absent, an Automerge clone imported
  with `repo.import`. The fork keeps history and syncs through the test hub.
- [ ] **9.** A registry doc for 5,000 and 15,000 entries with three moves each: saved size and load time in JS and
  Rust. Write the soft limit into the contract.
- [ ] **10.** `git` is on `PATH` where the extension runs, from a real extension host; the remote-development case is
  noted as not done unless a remote is available.
- [ ] **11.** Whether `q2 preview` without `--allow-edit` renders a doc edit made by a JS client in the browser (the
  preview may render from docs or from disk).

### Wrap-up

- [ ] Commit in logical pieces (contract; schema; package and CI; fixtures). The commit message of the fixtures
  commit lists the fixture files added. `cargo xtask lint` is green.

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

The contract is written and render-clean; the schema, package, CI step, and fixtures are committed with green gates;
all eleven Gate 0 results are recorded; none failed, or Gordon has decided what to do about each that did.

## Out of scope

The logic itself (GE-2, GE-5), the swap module (GE-6), adopting the helper in `createNewProject` (GE-7), and any
hub behavior change.

## Notes from the run

Not started.
