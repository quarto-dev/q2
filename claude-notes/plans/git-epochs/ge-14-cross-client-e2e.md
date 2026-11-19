---
title: 'GE-14: Cross-client e2e: the hub demo and the hub-less demo'
date: 2026-11-18
description: Automates both demos with Playwright, VS Code, headless peers, and the real binaries, decides which tests are permanent, and wires those into CI and cargo xtask verify.
status: done
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

- [x] Extend GE-13\'s fixture builder with `c.qmd`, `b.qmd`, and the PNG; unit-test blob determinism.
- [x] **Cross-implementation registry check** (the first plan that waits for both the Rust and TypeScript logic): the
  TypeScript package decodes the sidecar document GE-2 committed under `fixtures/golden/from-rust-*.bin` and a sidecar
  written by a real hub run, and the Rust crate decodes the doc GE-5 committed under `fixtures/golden/`; each pair
  yields the same logical entries, with generations as `int` and both string forms accepted. To dump the hub\'s
  sidecar, a small script connects a raw JS `Repo` to the running hub\'s `/ws`, reads the sidecar docId from the
  index\'s `epochs` key, fetches the doc, and writes `A.save(doc)` to a fixture file (the same raw-`Repo` path Gate 0
  item 1 proved). A mismatch is a contract bug: stop and bring it to Gordon.
- [x] Extend `q2-preview-spa/e2e/helpers/previewServer.ts` with `hubUrl` and `project` options (`--hub-url`,
  `--project`; `startPreviewServer` has neither today) and add a standalone-hub starter for L2 and L4 (GE-8\'s
  `helpers/hub.ts` hub, or `q2 hub`; the relay hub must need no sign-in). These helpers are not in GE-13.
- [x] Build the orchestrator: start the hub, a browser page, a headless peer or a VS Code window, and a standalone
  local hub; tear everything down; log the exact invocations.
- [x] Write H1, H2, and H3 and watch them fail; make them pass.
- [x] Write H6 and H7; then H5 and H8.
- [x] Write H4 with a real VS Code clone (exploratory).
- [x] Write L1 to L4; write L5 (exploratory).
- [x] Record the manual transcripts: the public server run and the ticket pair, or say they could not be run.
- [x] Record the permanent and exploratory lists with reasons; tag the specs.
- [x] Add the `test:e2e` script for the VS Code subset, the workflow changes, and the `cargo xtask verify` step in one
  commit.
- [x] Run the rebuild chain before the final runs: `cd hub-client && npm run build:wasm`,
  `cargo xtask build-q2-preview-spa`, `cargo xtask build-hub-client-embed`, `cargo xtask build-hub-mcp-bundle` (the
  client changed in GE-7), `cargo build --bin q2`, and the extension build.
- [x] `cargo xtask lint`; `q2 render --strict` on this file after editing its checkboxes.

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

**Session 1 (lane A, 2026-11-19).** Branch `ge/14-cross-client-e2e` off `feature/git-epochs` at `28150b043` (GE-13 landed
a moment before; GE-11 and GE-12 earlier). Everything ran on macOS arm64 under the Node in `.nvmrc`, with VS Code 1.101.2,
`target/debug/q2` built from this branch after the rebuild chain, chromium and `git` (`jj` was not used).

**What was built.**

- `q2-preview-spa/e2e/helpers/previewServer.ts`: `hubUrl` and `project` options (`--hub-url`, `--project`, no path
  argument; the combination with a local project, `targetFile`, `staticMode`, or `share` throws).
- `vscode-sync-experiment/e2e/helpers/cross-client.ts`: `Scene`, the orchestrator. It starts the tracking hub, the
  `--hub-url` preview, the in-process standalone hub, browser guests, headless mirrors, and headless clones (`git clone` of
  the hub repository plus the Join logic), prints one `[scene]` line per invocation, and tears everything down in reverse.
  `helpers/registry-dump.ts` reads the sidecar through a raw `Repo`. The scenarios live in `tests/cross-client/` and have
  their own config and script (`npm run test:e2e:cross`), because they need the embed, chromium, and a built `q2`; the
  existing `test:e2e` ignores that folder.
- The tests import GE-13\'s helpers (`gitRepo`, `editor`, `previewServer`) by relative path; they are ESM already. The
  `q2-preview-spa/e2e` files are now type-checked by the extension\'s e2e project, which uses `noUncheckedIndexedAccess`;
  three `!` assertions were needed in `previewServer.ts`.
- `from-hub-run.bin` and its manifest entry (one new file, one manifest entry, no snapshot touched).

**Detail differences from the plan.**

- The fixture builder needed no change: GE-13 already built `c.qmd`, `b.qmd`, and the PNG. The only addition is a
  determinism check (`git-epochs-helpers.spec.ts`: two builds give the same blob ids).
- The standalone hub for the hub-less demo is the in-process test hub (GE-8\'s `startHub`), not `q2 hub`; it needs no
  sign-in, and `q2 preview --hub-url` peers with it as it does with a real hub.
- **Editor-only (H2).** A headless peer has no editor buffers, so the editor-only half of H2 is its own test, **H2e**, in a
  real VS Code window on a `git clone` of the hub repository bound with `bindFolder`. It switches the hub, then the clone
  repository, and checks the window shows the feature text and the hub\'s `a.qmd` holds exactly the feature text.
- **H5 status bar.** The plan asks that a clone on the same blob says "your doc was superseded" in its status bar. That is a
  VS Code surface; the headless clone is checked on its disk (it stays in the retired doc and never overwrites the hub). The
  status-bar text is covered at the core level by GE-10 and is not driven here.
- **H5 decision kind.** The status route has no `reset` decision for this case; the test asserts `discarded >= 1` on any
  decision, as GE-13 S10 does.
- **H1 serialized.** The first version typed in the browser and appended to the clone and mirror disks at once. It failed 3
  of 3 repeats ("every edit reached the hub disk"): two disk appends race the sync write of the other peer\'s edit, which is
  a test race. The edits are now typed one after another, each waited for in every peer, and the browser keeps typing across
  the commit. It then passed 5 of 5 alone and 3 of 3 in the full run.

**Test-first record.** The scenarios were written against the built behavior, since the product already exists; the check
that they test something is the control run: `GE14_GIT=ignore npx ...` (the hub started with `--git ignore`) fails H1, H2,
H3, H5, H6, H7, and the registry check (7 of 7), all at "attach refused (no-repo-entry): This project was not shared from
a git repository" or "the index has no `epochs` key". That shows a scenario cannot pass by ordinary sync alone, but it fails
at the clone attach, not at the swap behavior itself. The swap-behavior discrimination (the notice never appears, no paused
indicator, no new tree, no `discarded`) is GE-13\'s control run of the same flows against `--git ignore`, recorded in its
notes. I did not repeat that without the clones here.

**Runs.**

- `npm run test:e2e:cross -w quarto-hub-vscode` (permanent subset: H1, H2, H2e, H3, H5, H6, H7, L1 to L4, registry check;
  9 tests): 9 of 9 pass, about 2 minutes. With `--repeat-each=3`, 24 of 27 passed, the 3 failures all the old H1 above; H1 was
  then fixed (5 of 5), and every other test passed 3 of 3. Exploratory (`GE14_ALL=1`): H4, H8, and L5 pass once each.
- `npm run test:e2e -w quarto-hub-vscode` (the extension flows of GE-8 to GE-12, cross-client folder ignored): 19 of 19.
- `npx playwright test --project=chromium --project=embed git-epochs-helpers.spec.ts git-epochs-viewer.spec.ts
  embed-git-epochs.spec.ts --workers=2` in `q2-preview-spa` (GE-13\'s scenarios, after the rebuild chain): 29 of 29. Firefox
  was not rerun.
- `npm run typecheck -w quarto-hub-vscode` (extension and `e2e/`) clean; `cargo clippy -p xtask --all-targets -- -D warnings`
  clean; `cargo nextest run -p xtask` 269 of 269; `cargo xtask lint` clean. The TypeScript registry test (32) and the Rust
  `golden` tests (6) pass with `from-hub-run.bin` in the manifest.
- **Not run:** `cargo xtask verify --e2e` (it needs Gordon\'s agreement), the Windows run (GE-17), Firefox, and the jj
  scenarios (GE-13 covers jj).

**Cross-implementation registry check.** The committed pairs (`from-rust-registry-ops.bin` decoded by TypeScript,
`ge05-registry-ops.bin` and the others decoded by Rust) were already tested by GE-2 and GE-5. This plan adds the missing
third document: the sidecar a real Rust hub wrote. `registry-crosscheck.e2e.ts` starts the hub, reads the sidecar through a
raw `Repo` on `/ws`, and checks one entry per tracked file, generations stored as `int` (value 0), blobs equal to
`git rev-parse HEAD:<path>`, nothing retired. `UPDATE_GOLDEN=1` wrote `from-hub-run.bin` (505 bytes, 5 entries) and its
manifest entry; the TypeScript package decodes it, and the Rust crate decodes it through the same manifest loop. No mismatch.

**The permanent subset, and why.** Permanent (`@permanent`; local binaries only, no network, stable in the repeat runs, about
2 minutes in all): H1, H2, H2e, H3, H5, H6, H7, L1 to L4 (one test, since L2 to L4 build on L1), and the registry check. H5
turned out stable (11 s, passed on every run). Exploratory (`@exploratory`, `GE14_ALL=1`): **H4** (a real VS Code clone: slower,
and its core is covered by GE-10), **H8** (the rebase timing depends on the hub\'s polling), **L5** (restarting the window
takes the startup rule through a second VS Code launch; the "stale until restart" gap is accepted), the public-server run, the
ticket join (needs the iroh relay), and the jj scenarios (GE-13). The Windows legs run the permanent subset in GE-17.

**CI and verify.** `.github/workflows/vscode-extension-e2e.yml` now builds what the `embed` leg of `q2-preview-spa-e2e.yml`
builds (Rust nightly, clang, wasm-pack, wasm-bindgen-cli, tree-sitter, binaryen, the WASM, trace-viewer, the SPA, the embed,
`q2`), installs chromium, and runs both scripts under `xvfb-run`. It stays `workflow_dispatch` only with
`continue-on-error: true`, as the plan says, until the permanent subset has passed on several manual runs; I could not run
the workflow itself here (it is a GitHub Actions run). `cargo xtask verify --e2e` has a new step 15 (`TOTAL_STEPS` is 15)
that runs `npm run test:e2e` and `npm run test:e2e:cross` in the extension workspace.

**End to end.** Both demos were run through the real binaries and the output inspected.

Hub demo, by hand (`target/debug/q2`, a temporary repository with `index.qmd` on `main` and `feature`):

```text
q2 preview --no-browser --git track --ui editor --allow-edit --data-dir <data> --port 39911 <repo>
  -> http://127.0.0.1:39911/#/share/3YQRDo49mNJpLQQJG7xbHyxHHiN3?server=%2Fws&file=index.qmd&name=repo&ephemeral=true
GET /api/preview/git-status          {"tracking":true,"paused":false,"pending":false,"markers":[],"decisions":[]}
git switch feature                   (in <repo>)
GET /api/preview/git-status          {"tracking":true,...,"decisions":[{"kind":"flipped","files":["index.qmd"],"step":"1","discarded":null}]}
tail -1 <repo>/index.qmd             FEATURE text
```

The browser side of the hub demo is H2 and H6 (a real chromium guest on the editor: the owner notice reads "project owner\'s
checked-out version changed", `b.qmd` appears and `c.qmd` leaves the tree). Hub-less demo, from the L1 to L4 run (inspected):

```text
[scene] standalone sync server (in-process test hub) at ws://127.0.0.1:51849/ws
[scene] modal: Share this folder on ws://127.0.0.1:51849/ws? | This publishes a copy of your project to that server. Anyone with access to that server and the project link can read and edit it. Project: ge14-origin-I2oc0M (git repo, branch main) Files: 5 tracked files, 1 KB Uncommitted: none
[scene] copied: q2 preview --hub-url ws://127.0.0.1:51849/ws --project 21H8j2wMse24UFck7gFgsHaSv4cQ --ui editor --allow-edit
[scene] browser guest on http://127.0.0.1:51861/#/share/21H8j2wMse24UFck7gFgsHaSv4cQ?server=%2Fws&file=index.qmd&name=q2%20preview&ephemeral=true (file a.qmd)
```

After that the test edits in the browser (it reaches the originator\'s disk), joins a clone (its five files attached by blob)
and a mirror, commits, runs `git switch feature` in the originating folder, and sees the browser, the mirror, and the tree
follow with the notice while the clone on `main` stays in its own doc.

**Manual runs not made.** The public-server run (`wss://sync.automerge.org`, the epic\'s Gate 0 item 4): publishing throwaway
content to a public service is Gordon\'s call; the modal\'s public-server wording is covered by GE-12\'s unit test. The real
`--share` ticket join: GE-13 S13 and GE-11\'s notes ran it; this plan did not repeat it.

**Defects found:** none in the product. One note for GE-17: the permanent scenarios use `fs.appendFile` on the clone and
mirror disks, which on Windows can race the sync writer more than on macOS (see the H1 note); H1 now waits between edits.

