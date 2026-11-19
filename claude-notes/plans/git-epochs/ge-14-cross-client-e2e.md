---
title: 'GE-14: Cross-client e2e: the hub demo and the hub-less demo'
date: 2026-11-18
description: Automates both demos with Playwright, VS Code, headless peers, and the real binaries, decides which tests are permanent, and wires those into CI and cargo xtask verify.
status: approved
depends-on: [ge-11, ge-12, ge-13]
lane: A
epic: ../2026-11-18-git-epochs-epic.md
---

**Epic:** [git epochs](../2026-11-18-git-epochs-epic.md) · **Waits for:** [GE-11](ge-11-join-command.md),
[GE-12](ge-12-origination.md), [GE-13](ge-13-hub-e2e.md) · **Unblocks:** [GE-17](ge-17-windows-run-c.md) ·
**Branch:** `ge/14-cross-client-e2e` off `feature/git-epochs`, merged by rebase and fast-forward, no push.

## Goal

Prove the whole system with real programs and no one at the keyboard. The **hub demo**: a `--git track` preview hub, a
browser guest, and VS Code peers in each mode, through a branch switch, a commit, a reset, and a switch back. The
**hub-less demo**: a VS Code window originates a project to a plain sync server, `q2 preview --hub-url` shows it in a
browser, and a second peer joins. Decide which of these tests are permanent, and wire those into CI.

## Decisions implemented

- **G30:** the headless peer and Playwright for VS Code; the permanent subset runs against a local hub with no
  network on Linux CI with a display.
- **G22, G24, G16:** the join and origination flows exercised end to end, with the owner writing the pointer.
- **G9, G13, G17, G20, G25, G26:** arrival, unlist and relist, the hub as a registry peer, editor-only and mirror
  following, the moved message, and the follower swap with its notice, observed from outside.
- **G33:** `--git track`, the status route, and `--hub-url` working together.
- The plan\'s record requirement: the exact invocations and inspected output of each demo (`AGENTS.md`, "End-to-end
  verification").

## Context

- The preview e2e harness starts a real preview with `startPreviewServer(opts)`, can start a join guest with
  `startJoinGuest(ticket)`, and finds the binary at `target/debug/q2` (`q2-preview-spa/e2e/helpers/previewServer.ts:269`,
  `:421`, `:80`). GE-13 added git helpers and a fixture repo builder there; this plan extends the builder.
- GE-8 provided the vscode-free core, the headless peer, and the Playwright VS Code harness (a real VS Code launched
  with the extension and a local hub); GE-9 to GE-12 added the modes, the Join command, and origination.
- `--hub-url` and `--project` are in `crates/quarto/src/main.rs` (lines 440 to 455); the preview that uses them runs a
  standalone hub with no project directory and must be pointed at a hub that needs no sign-in
  (`claude-notes/plans/2026-11-13-preview-against-remote-hub.md`).
- CI today: `.github/workflows/q2-preview-spa-e2e.yml` is a trial workflow (`continue-on-error: true`, a matrix of the
  Playwright projects `chromium`, `firefox`, `embed`) that builds the WASM, trace-viewer, the SPA, the hub-client embed
  (`cargo xtask build-hub-client-embed`), and `q2`, installs pandoc, typst, and browsers, and runs
  `npx playwright test --project=…`. `cargo xtask verify --e2e` runs step 14, `npm run test:e2e` in `q2-preview-spa`
  (`crates/xtask/src/verify.rs:869-894`; the step list is in the header at `:27`).
- `ci-test-suite-unwired` sees only a package\'s `test` script (`crates/xtask/src/lint/ci_test_wiring.rs`), so a
  `test:e2e` script is wired by hand, and `AGENTS.md` requires the matching `cargo xtask verify` step in the same
  commit.

## The fixture and the peers

**The fixture repo** (built deterministically, so blob ids repeat across runs): on `main`, `_quarto.yml`, `index.qmd`,
`a.qmd`, `c.qmd`, and `img/logo.png` (a small generated PNG). Branch `feature` changes `a.qmd`, adds `b.qmd`, removes
`c.qmd`, and changes `logo.png`.

**Peers.** The hub is `q2 preview --git track --ui editor --allow-edit` on the repo. The permanent tests omit
`--share`, since a ticket needs the network; joins use the loopback URL. A browser guest is a Playwright page on the
hub\'s editor UI. One scenario uses a real VS Code window for the mode under test; the other modes are headless peers
running the same core (detail chosen by the plan author; confirm), built with GE-9\'s headless owner and GE-10\'s
`joinAsClone` and `joinAsMirror` helpers. A clone peer is a separate clone of the fixture
repo attached through the Join logic; a mirror is a folder the Join logic created.

## Scenarios

Every assertion waits on the state it asserts, with retrying assertions, never a fixed sleep
(`.claude/rules/hub-client-tests.md`).

**The hub demo**

| Id | Action | Expected |
|---|---|---|
| H1 | The hub commits while all peers are typing | No swap and no notice anywhere. Every edit survives in every peer. The registry shows the doc moved forward: an entry under the new blob, none under the old (read with GE-5\'s registry operations through a raw `Repo`, as in the dump script above) |
| H2 | The hub runs `git switch feature` | The browser swaps with the owner notice and shows the feature text; an edit typed just before the switch is counted as stayed with the previous version. The mirror\'s disk equals the feature file. Editor only suspends, waits for the flip, rebinds, and shows the feature text, writing nothing. The clone on `main` stays in its own doc and its disk is untouched. A second clone already on `feature` joins the hub\'s new doc |
| H3 | A clone on `main`, same as the hub, types | The edit reaches the browser and the hub\'s disk; a browser edit reaches the clone\'s disk |
| H4 | The clone runs `git switch feature` (one real VS Code run) | Arrival attaches the doc for that blob, or creates one; peers still in the old doc see the moved message; a clean file with live edits joins with the notice and the "use my checkout instead" action; the hub\'s pointer does not move |
| H5 | The hub runs `git reset --hard` after a browser edit | No guest sees a revert merged in as an author edit; the pointer moves to a new doc equal to disk; the browser edit stays in the retired doc and the decision\'s `discarded` count is at least 1; a clone on the same blob stays in the retired doc, and its status bar says "your doc was superseded" |
| H6 | The hub switches to `feature` and back | `c.qmd` leaves the browser tree, the mirror\'s disk loses it, the clone on `main` keeps its own; `b.qmd` appears in the tree and in the mirror; on return `c.qmd` is listed again at its old doc and `b.qmd` is gone |
| H7 | The hub switches and back, with `logo.png` differing | The browser shows the new image; the mirror\'s bytes equal the feature file; the clone on `main` keeps its own bytes (never overwritten) |
| H8 | The hub starts a rebase that stops for conflicts, then continues | The status route reports paused; guests keep editing; no conflict marker reaches any doc; after the rebase one decision runs and guests swap once |

**The hub-less demo**

| Id | Action | Expected |
|---|---|---|
| L1 | A real VS Code window runs **Share this folder** against a standalone hub on loopback | The modal text matches the mock in GE-12; the panel appears only after the server holds the heads; the three forms are copyable |
| L2 | `q2 preview --hub-url ws://127.0.0.1:S/ws --project <id> --ui editor --allow-edit` | The browser shows the project; a browser edit reaches the originator\'s disk |
| L3 | A second peer joins as a clone (passes the identity check) and as a mirror | Both sync; the clone attaches by blob |
| L4 | The originator runs `git switch feature` | The browser follows the flip through the relay with the notice; the mirror follows; the clone on `main` stays in its own doc |
| L5 | The originator goes offline, checks out another branch, and restarts | The startup rule re-points the paths; until then the browser is stale (the accepted gap) |

**Manual only.** The same hub-less run against `wss://sync.automerge.org` with throwaway content (the epic\'s Gate 0
item 4, recorded as a transcript), and a real `q2 preview --share` with a ticket join (the network).

## The permanent subset

A test is permanent when it needs only local binaries and no network, is stable under CI load, and finishes in about
two minutes. Proposed permanent: H1, H2, H3, H6, H7, L1, L2, L4, and H5 if stable, with headless peers standing in
for modes that are not under test. Exploratory: H4 with a real VS Code clone, H8 (rebase timing), L5, the manual
runs, jj scenarios, and anything needing the network. The final list and the reason for each exclusion go in the
notes (detail chosen by the plan author; confirm).

## CI and verify

- Extend the workflow `.github/workflows/vscode-extension-e2e.yml` that GE-8 created (`workflow_dispatch` only),
  modeled on `q2-preview-spa-e2e.yml`: Rust nightly,
  clang, wasm-pack, wasm-bindgen-cli, Node 24, `npm ci`, the WASM, trace-viewer, SPA, and hub-client embed builds,
  `cargo build --bin q2`, the extension build, the VS Code download step of the GE-8 harness (cached), and the
  Playwright run under a virtual display (`xvfb-run`), with a report upload. The workflow rebuilds Rust, the WASM, and
  the embeds, which the existing 75-minute `q2-preview-spa-e2e` legs already do, so it stays `workflow_dispatch` only
  (non-gating, `continue-on-error: true` like its model) until the permanent subset has passed on several manual runs
  under the virtual display; then Gordon decides whether to add path triggers (the extension, the hub crates,
  `quarto-sync-client`, `preview-runtime`, the schema and git epochs packages, and the workflow itself) and whether to
  gate it.
- A new `cargo xtask verify` step (step 15, under `--e2e`) running the permanent subset; update `TOTAL_STEPS` and the
  header list in `verify.rs`. GE-1 and GE-8 already added the unit-test steps for the new packages and the extension.
- GE-17 runs the permanent subset on Windows, plus GE-9\'s timing test, which is not in the permanent subset.

## Test specifications

The scenarios are the tests. Written first, each seen failing for the right reason against a stub of the behavior it
checks. Harness unit tests too: the fixture builder gives the same blob ids on every run; teardown kills every
process and removes every temporary directory, and no async work outlives a test.

## Checklist

- [ ] Extend GE-13\'s fixture builder with `c.qmd`, `b.qmd`, and the PNG; unit-test blob determinism.
- [ ] **Cross-implementation registry check** (the first plan that waits for both the Rust and TypeScript logic): the
  TypeScript package decodes the sidecar document GE-2 committed under `fixtures/golden/from-rust-*.bin` and a sidecar
  written by a real hub run, and the Rust crate decodes the doc GE-5 committed under `fixtures/golden/`; each pair
  yields the same logical entries, with generations as `int` and both string forms accepted. To dump the hub\'s
  sidecar, a small script connects a raw JS `Repo` to the running hub\'s `/ws`, reads the sidecar docId from the
  index\'s `epochs` key, fetches the doc, and writes `A.save(doc)` to a fixture file (the same raw-`Repo` path Gate 0
  item 1 proved). A mismatch is a contract bug: stop and bring it to Gordon.
- [ ] Extend `q2-preview-spa/e2e/helpers/previewServer.ts` with `hubUrl` and `project` options (`--hub-url`,
  `--project`; `startPreviewServer` has neither today) and add a standalone-hub starter for L2 and L4 (GE-8\'s
  `helpers/hub.ts` hub, or `q2 hub`; the relay hub must need no sign-in). These helpers are not in GE-13.
- [ ] Build the orchestrator: start the hub, a browser page, a headless peer or a VS Code window, and a standalone
  local hub; tear everything down; log the exact invocations.
- [ ] Write H1, H2, and H3 and watch them fail; make them pass.
- [ ] Write H6 and H7; then H5 and H8.
- [ ] Write H4 with a real VS Code clone (exploratory).
- [ ] Write L1 to L4; write L5 (exploratory).
- [ ] Record the manual transcripts: the public server run and the ticket pair, or say they could not be run.
- [ ] Record the permanent and exploratory lists with reasons; tag the specs.
- [ ] Add the `test:e2e` script for the VS Code subset, the workflow changes, and the `cargo xtask verify` step in one
  commit.
- [ ] Run the rebuild chain before the final runs: `cd hub-client && npm run build:wasm`,
  `cargo xtask build-q2-preview-spa`, `cargo xtask build-hub-client-embed`, `cargo xtask build-hub-mcp-bundle` (the
  client changed in GE-7), `cargo build --bin q2`, and the extension build.
- [ ] `cargo xtask lint`; `q2 render --strict` on this file after editing its checkboxes.

## Gates

- After the rebuild chain: `npm run test:e2e -w q2-preview-spa` (GE-13\'s scenarios still pass) and the VS Code
  Playwright run script from GE-8 (the permanent subset).
- `npm run typecheck` and `npm run test` for any package touched.
- `cargo xtask lint`, and one `cargo xtask verify --e2e` at the end.

## End-to-end verification

The scenarios are the verification, but each demo is also run once through the real binaries by hand where the
harness hides it: the hub demo with a real browser, and the hub-less demo with `q2 preview --hub-url`. Record the exact
commands, a snippet of the output that shows each expected effect, and an explicit note that the output was inspected,
in the notes below.

## Exit criteria

- H1 to H3, H6, H7, and L1 to L4 pass on a clean machine; the rest are recorded as exploratory with reasons.
- The transcripts of both demos are in the notes.
- The workflow and the verify step exist and agree.

## Out of scope

Windows runs (GE-17), jj beyond a skipped scenario, authenticated hubs, any change to the logic under test, and fixing
product bugs found here: a defect is a new plan.

## Notes from the run

Not started.
