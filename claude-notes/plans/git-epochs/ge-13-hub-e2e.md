---
title: 'GE-13: Hub e2e with browser guests'
date: 2026-11-18
description: 'Adds git helpers to the q2-preview-spa e2e harness and writes the automated scenarios of the hub demo, with real git, a flagged preview hub, and two browser guests.'
status: approved
depends-on: [ge-4, ge-7]
lane: B
epic: ../2026-11-18-git-epochs-epic.md
---

**Epic:** [git epochs](../2026-11-18-git-epochs-epic.md) · **Waits for:** [GE-4](ge-04-hub-surface.md),
[GE-7](ge-07-client-integration.md) · **Unblocks:** [GE-14](ge-14-cross-client-e2e.md) ·
**Branch:** `ge/13-hub-e2e` off `feature/git-epochs`, merged by rebase and fast-forward; nothing is pushed.

## Goal

Prove the Part A story through the real binary and real browsers: a `q2 preview --git track` hub on a real git repo,
two browser guests, and the git operations that used to corrupt a live session. This is the automated deliverable of
the hub demo (the earlier plan\'s D22). The VS Code windows join in [GE-14](ge-14-cross-client-e2e.md); this plan
uses browsers only, so every scenario here runs without VS Code or a network.

## Decisions implemented

- **G30** (the harness half). The `q2-preview-spa` e2e harness gains git helpers; the permanent specs run against a
  local hub with no network.
- **G1, G2, G3, G5.** The scenarios observe the gate (a marker pauses sync), the trigger (including
  `git reset --hard`), and the classifier (a commit does not disrupt; a rewrite re-attaches).
- **G9, G13.** Arrival (attach, join live edits, and the `ReadOnly` variant), and unlist and relist of added and
  removed files.
- **G26, G33.** The client swap and its aggregate notice, and the paused indicator fed by the status route.

## Context

- The harness is `q2-preview-spa/e2e/helpers/previewServer.ts`. `StartOptions` (`:159`) already has `fixtureFiles`,
  `allowEdit`, `ui` (`'viewer'` or `'editor'`), and `share`; `startPreviewServer` (`:269`) writes the fixtures into a
  temp project, spawns `q2 preview --no-browser --data-dir ...` with those flags and the project directory
  (`:312-325`), and returns `projectDir`, `dataDir`, `url`, `serverLog()`, `joinTicket()`, and `stop()`.
  `startJoinGuest` (`:421`) and `isEmbedBuilt` (`:468`) exist. There is no git anywhere in it.
- Playwright projects (`q2-preview-spa/playwright.config.ts`): `chromium` and `firefox` run everything except
  `embed-*.spec.ts`; the `embed` project runs `embed-*.spec.ts` against a `q2` built with the hub-client editor
  embedded. A spec named `embed-*` is therefore an editor spec and must skip when `isEmbedBuilt` says the binary holds
  the placeholder (the pattern in `embed-import.spec.ts`, which also defines `treeItem`, `openProject`, and
  `editorText = page.locator('.monaco-editor .view-lines').first()` at `:88`).
- CI (`.github/workflows/q2-preview-spa-e2e.yml`): a matrix of `chromium`, `firefox`, and `embed`, each running
  `npx playwright test --project=${{ matrix.project }}` from `q2-preview-spa`; the `embed` leg builds
  `cargo xtask build-hub-client-embed` first. The `paths:` filter already covers `q2-preview-spa/e2e/**`,
  `crates/quarto-hub/**`, `crates/quarto-preview/**`, and `crates/quarto/**`; the jobs are `continue-on-error`
  (a trial workflow). `ubuntu-latest` has `git`; it has no `jj`.
- The `ci-test-suite-unwired` lint checks only npm workspace `test` scripts, so it does not see Playwright specs: the
  wiring is verified here by reading the workflow, not by the lint.
- `.claude/rules/hub-client-tests.md` governs these specs: wait on the state you assert with retrying assertions;
  no `page.waitForTimeout` as a synchronization point; presence is not delivery, so wait for the other guest to see a
  change before a step that depends on it.
- The hub\'s file watcher debounce is 500 ms (`build_hub_config`, `crates/quarto-preview/src/lib.rs:684`).
- Two browser guests need no tunnel: two Playwright contexts open the same loopback `handle.url`. The tunnel path
  (`share` plus `startJoinGuest`) needs iroh\'s relay and is used by one optional spec.

## Test specifications

The helpers get small unit-style checks first (they are test code, but a wrong helper makes every scenario lie):
`gitRepo.ts` creates a repo with a fixed author and no global config, `git switch`, `commit`, `rebase`, and
`reset --hard` helpers throw with git\'s stderr on failure, and `jjAvailable()` is false when `jj --version` fails.
Then the scenarios, one test each, seen failing before the code under test is exercised end to end (if GE-3 and GE-4
are correct they pass on first run; the failing run is the same spec against a hub started without `--git track`,
which must fail the swap assertions). Every scenario starts `--git track --ui <viewer|editor>` on a repo whose `main`
holds `_quarto.yml`, `index.qmd`, `a.qmd`, `c.qmd`, and `img/logo.png`, and whose `feature` branch changes `a.qmd`
and `img/logo.png`, removes `c.qmd`, and adds `b.qmd` (the fixture GE-14 reuses and extends).

| Id | Scenario | What must hold |
|---|---|---|
| S1 | An owner commit while two guests edit | No notice and no swap; both guests\' edits stay in both pages and on disk; a marker typed after the commit reaches the other guest; the status route lists a `moved-forward` decision within a few seconds although the guests keep typing (this plan has no sidecar reader: GE-13 does not wait for GE-5, so the registry itself is read in GE-14) |
| S2 | `git switch feature` | Both guests show the `feature` text; the hub\'s disk equals it; the aggregate notice appears once per guest (editor) or not at all (viewer, which re-renders instead) |
| S3 | A stranded edit | A guest edits against the old doc while the switch happens (offline, so the edit is unconfirmed when the flip arrives; GE-7\'s `unconfirmedLocalEdits` counts it); the notice also says the edit stayed with the previous version; the file on screen and on disk match `feature` |
| S4 | `git switch main` back | Editor and disk agree; under `--allow-edit` a stranded edit retained in the old doc may resurface, because arrival joins live edits (G9 step 4); the spec asserts agreement between editor and disk and records which outcome occurred |
| S5 | A rebase stopped for conflicts | `GET /api/preview/git-status` reports paused; the guests show the paused indicator and no conflict markers; after the rebase finishes, paused clears and exactly one decision is applied: one notice, editor equal to disk |
| S6 | jj `new` onto another commit (colocated), skipped when `jj` is absent | `jj new` writes the reflog, so the trigger fires. Arrival under jj is unsupported: guests follow the flip to a doc created from disk (the step 5 fallback, a `seeded` decision, nothing registered); no conflict marker reaches a doc. `jj edit` between sibling commits changes the tree with no reflog change and is a stated gap (G35), so no scenario uses it |
| S7 | Save then commit inside the 500 ms debounce | Treated as a commit: no swap, no notice, the new text reaches the guests by ordinary sync |
| S8 | `feature` adds `b.qmd` and removes `c.qmd` | After the switch the tree lists `b.qmd` and not `c.qmd`; after switching back the reverse |
| S9 | A tracked binary changes on `feature` | After each switch the hub\'s disk bytes equal the checked-out blob and the guests see the same image bytes; no cross-branch overwrite |
| S10 | `git reset --hard` after a dirty edit | The reflog trigger fires with HEAD unchanged; the pointer moves to a new doc equal to the HEAD content; the dirty text stays in the retired doc, and the decision appears in the status route with `discarded` of at least 1 |
| S11 | No `--allow-edit` (`ReadOnly`) | A switch follows without any browser edit reaching disk; arrival attaches only a doc equal to disk, otherwise a new doc from disk |
| S12 | Control: `--git ignore` | No paused indicator; the status route reports tracking false; today\'s behavior |
| S13 | Optional: a guest through `startJoinGuest` | Sees the S2 swap through the tunnel; skipped when the relay is unreachable |

## Checklist

- [ ] Read `.claude/rules/hub-client-tests.md` and the structure of `embed-import.spec.ts` (skips, `treeItem`,
  `openProject`, `editorText`). Record anything surprising.
- [ ] Add `q2-preview-spa/e2e/helpers/gitRepo.ts`: `GitRepo` bound to a directory with `init` (fixed
  `user.name` and `user.email`, `core.autocrlf=false`, `GIT_CONFIG_GLOBAL` and `GIT_CONFIG_SYSTEM` pointed at an empty
  file), `commitAll`, `switch`, `switchCreate`, `rebase` (returns without throwing when git stops for conflicts),
  `rebaseContinue`, `rebaseAbort`, `resetHard`, `writeFile`, `markerPath(name)` (via `git rev-parse --git-path`), and
  `jjAvailable()` plus `jjColocate`, `jjNew`, `jjEdit`. Use `execFileSync` with argument arrays, never a shell.
- [ ] Add `q2-preview-spa/e2e/helpers/editor.ts`: `openProject`, `treeItem`, `editorText`, `typeInEditor`,
  `noticeLocator` (the element with `role="status"` that GE-7 renders), and `pausedIndicator` (found by the text GE-7
  renders). Do not edit `embed-import.spec.ts`.
- [ ] Extend `StartOptions` with `git?: 'track' | 'ignore'` (passes `--git <value>`) and `gitRepo?: { branches }`
  (creates the repo, commits the fixtures on `main`, creates `feature` with its own commit, and returns to `main`
  before the server starts); expose the bound `GitRepo` as `handle.repo`. Existing callers are unchanged.
- [ ] Write the helper checks, then the scenario specs, seen failing against a `--git ignore` hub. Split them:
  `q2-preview-spa/e2e/git-epochs-viewer.spec.ts` (viewer UI: S1, S2, S4, S5, S8, S9, S10, S12; runs under the
  `chromium` and `firefox` projects) and `q2-preview-spa/e2e/embed-git-epochs.spec.ts` (editor UI: S1 to S12, with
  the notices, stranded edit, jj, and `ReadOnly`; runs under the `embed` project; skips when `isEmbedBuilt` is false).
- [ ] S3 stranded edit (detail chosen by the plan author; confirm): take guest A offline with
  `context.setOffline(true)`, type, switch branches on the hub, bring A online. If `setOffline` does not drop an
  open WebSocket, use `page.routeWebSocket` to close the socket and drop its frames instead.
- [ ] S5 rebase: build two branches that edit the same lines, start `rebase` so it stops, poll the status route for
  `paused`, assert the indicator and the absence of conflict markers in the editors, then `rebaseContinue` after
  resolving, and count notices.
- [ ] Wire and verify the CI legs by reading `.github/workflows/q2-preview-spa-e2e.yml`: the new viewer spec is picked
  up by `chromium` and `firefox`, the editor spec by `embed`, and nothing in the workflow needs a `git` or `jj`
  install (the specs skip jj). The `paths:` filter covers `q2-preview-spa/e2e/**`, `crates/quarto-hub/**`,
  `crates/quarto-preview/**`, and `crates/quarto/**` but not the client code these specs exercise: add
  `ts-packages/quarto-sync-client/**`, `ts-packages/quarto-git-epochs/**`, `ts-packages/preview-runtime/**`, and
  `hub-client/src/**`. Record this reading.
- [ ] Rebuild chain before the first run (from `AGENTS.md`): `cd hub-client && npm run build:wasm`,
  `cargo xtask build-q2-preview-spa`, `cargo xtask build-hub-client-embed`, `cargo build --bin q2`.
- [ ] Run the specs; fix defects in the specs, and report defects in product code as findings in the notes without
  changing product code (a defect in GE-3, GE-4, or GE-7 reopens that plan).
- [ ] Record the exact invocations and inspected output of each scenario in "Notes from the run"; update this
  checklist.

## Gates

```bash
cd q2-preview-spa
npx playwright test --project=chromium git-epochs-viewer.spec.ts
npx playwright test --project=firefox  git-epochs-viewer.spec.ts
npx playwright test --project=embed    embed-git-epochs.spec.ts
cd .. && npm run typecheck && cargo xtask lint
```

## End-to-end verification

The specs are the end-to-end verification: they drive the real `q2` binary, real git, and real browsers. In
addition, run one scenario by hand once (start `q2 preview --git track --ui editor --allow-edit` on a temp repo, open
two browser tabs, `git switch feature`) and note that the observed behavior matches the spec.

## Exit criteria

- S1 to S12 pass locally on the projects above; S6 and S13 skip with a stated reason when `jj` or the relay is
  absent. The same specs fail against `--git ignore` where the table says so.
- The CI wiring reading is recorded. Every product defect found is listed in the notes with its output.

## Out of scope

- VS Code windows and the hub-less demo (GE-14); the Windows runs (GE-15 to GE-17).
- Any product code change; a defect is reported, and the owning plan is reopened.
- Docs for the flag (GE-18).

## Notes from the run

Not started.
