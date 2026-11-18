---
title: 'GE-8: Extension foundation: workspace, ignore fixes, vscode-free core, headless peer, Playwright harness'
date: 2026-11-18
description: Prepares the VS Code extension for the rest of the epic with no change in behavior, by rejoining the npm workspaces, fixing the ignore rules, extracting a vscode-free core and a headless peer, and adding a Playwright harness that drives VS Code.
status: approved
depends-on: [ge-1]
epic: ../2026-11-18-git-epochs-epic.md
---

**Epic:** [git epochs](../2026-11-18-git-epochs-epic.md) ·
**Waits for:** [GE-1](ge-01-contract-and-gate-0.md) ·
**Unblocks:** [GE-9](ge-09-extension-follower-modes.md) ·
**Branch:** `ge/08-extension-foundation` off `feature/git-epochs`; rebase and fast-forward when done; never pushed.

## Goal

Every later extension plan needs four things the extension does not have: a clean place in the npm workspaces, a
correct ignore rule, a core that runs without VS Code so it can be tested and driven by a headless peer, and a way to
drive VS Code itself with Playwright. This plan builds them and changes no behavior a user can see. The three existing
flows (Connect to Document, Create Folder from Project, and auto-start of a folder that has
`.quarto-hub/project.json`) must work exactly as before, which tests written first against the core prove.

## Decisions implemented

- **G23** Ignore any path segment named `.git` or `.quarto-hub`, and skip a subfolder that has its own
  `.quarto-hub/project.json`. (Extends the earlier D35.)
- **G29** The shared pieces the extension needs: the share-URL helpers move to a shared package.
- **G30** A vscode-free core with injected watcher, buffer host, and notifier; a headless peer on the core; a
  Playwright harness that launches VS Code with the extension; the permanent subset is chosen later, in GE-14.
- **D34** (kept) The extension rejoins the root npm workspaces, its README is corrected, and its tests are wired into
  CI so the `ci-test-suite-unwired` lint holds.

## Context

- **Workspaces.** The root `package.json` lists `ts-packages/*`, `hub-client`, `trace-viewer`, `q2-preview-spa`, and
  `q2-demos/*`; the extension (`vscode-sync-experiment/`, package name `quarto-hub-vscode`) is not a member. Commit
  `8ae461f1b` removed it because every `npm install` rewrote `package-lock.json` to mark it extraneous; it also dropped
  `@types/vscode` from the lockfile, since only the extension used it. The root `build` and `typecheck` scripts run
  `--workspaces --if-present`, so rejoining adds the extension\'s `tsc --noEmit` and `node esbuild.mjs` to the root,
  to `cargo xtask verify`, and to CI. Today its `tsc` fails here with "Cannot find type definition file for
  `vscode`". The root `engines.node` is `^24.0.0`; the extension declares only `engines.vscode`.
- **The ignore rule.** `isIgnored` checks the first segment against `IGNORED_TOP_LEVEL` (`.quarto-hub`, `.vscode`,
  `.git`, `.quarto`, `_site`, `_book`, `_freeze`, `node_modules`, `.DS_Store`) and `startsWith('.quarto-hub')`
  (`vscode-sync-experiment/src/project.ts:42`, `:67-70`). A nested `.git` or `.quarto-hub` is not ignored, so a
  nested mirror folder would be swallowed by a parent that is itself synced. `relOf` and `listDiskFiles` already
  normalize to `/` (`project.ts:214`, `:267`).
- **What mixes `vscode` into the engine.** `ProjectSync` (`project.ts:153` on) creates the file watcher and listens to
  open and close events (`:188-197`), looks up open documents (`track`, `:219-231`, and `createFromDisk`,
  `:383-400`), and logs to an `OutputChannel`.
  `Binding` (`src/binding.ts`) holds a `vscode.TextDocument`, applies remote changes with `WorkspaceEdit`
  (`applyRemote`, `:74-103`), splices local edits (`onLocalEdit`, `:54-62`), and draws decorations (`render`, `:105`
  on). Its diff-and-splice logic is pure. `Presence` (`src/presence.ts`) depends only on automerge.
- **The server constant.** `makeRepo` and `createProjectFolder` use `SYNC_SERVER` (`project.ts:40`, `:63`, `:123`);
  `ProjectSync` dials `meta.syncServer` (`:183`). So a folder whose `project.json` names a local server already
  syncs against it; only creation is hard-wired.
- **Share-URL helpers.** `ts-packages/quarto-hub-mcp/src/share-url.ts` exports `buildShareUrl`, `parseProjectRef`,
  the `ProjectRef` and `ShareUrlParts` types, `DEFAULT_SERVER_URL`, and a server-equality helper; the fragment
  parsing is subtle (the data is after `#`), which is why it should be written once.
- **Test hubs.** `ts-packages/quarto-sync-client/src/test-hub.ts` is an in-process JS hub with `url`, the hub-side
  `repo`, `hubHasDoc`, and head-waiting helpers; `.claude/rules/hub-client-tests.md` ("Sync tests: presence is not
  delivery") says to wait for the creator\'s heads on the hub before a second client reads. `@playwright/test`
  `^1.50.0` is already a dev dependency of `q2-preview-spa`.
- **CI.** `.github/workflows/ts-test-suite.yml` has a "Workspace TS suites" job that runs `npm ci`, builds the
  ts-packages, and smoke-checks the MCP module graph. `crates/xtask/src/lint/ci_test_wiring.rs` fails the lint for
  any workspace package with a `test` script that no workflow `run:` step names (by path or package name).

## Test specifications

Written first, seen failing for the right reason, then the code that makes them pass.

1. **Ignore helper** (table-driven): `.git` at any depth (`a/b/.git/config`); `.quarto-hub` at any depth
   (`sub/.quarto-hub/automerge/x`); `.DS_Store` anywhere; each top-level entry in `IGNORED_TOP_LEVEL`; `rel === ''`; a
   subfolder containing its own `.quarto-hub/project.json` is skipped as a whole (the helper takes a set of known bound
   subfolders so it stays pure); a Windows-style input (`a\\b\\.git\\x`) is normalized first.
2. **Share-URL package.** The existing unit tests of `share-url.ts` move with the code; add round-trip tests of
   `buildShareUrl` through `parseProjectRef` and a case where the fragment carries `server=wss://…`.
3. **Characterization tests of today\'s sync engine**, run against the core with a fake `FileWatcher`, `BufferHost`,
   and `Notifier` and a real in-process test hub:
   - startup folds offline disk edits into the docs as merges, and removes index entries for files missing from disk
     (`reconcileDisk`, `project.ts:277-309`);
   - a created file becomes a doc and an index entry (`createFromDisk`, `:383-400`), text and binary, with the open
     buffer text winning over disk for an open file;
   - a deleted file or folder removes its entry (`onDiskDelete`, `:402-419`);
   - an index entry that appears, or whose doc id changes, is fetched and written to disk (`syncIndex`, `:313-341`;
     today\'s unconditional untrack, retrack, write, which GE-9 replaces: pin it, with a comment saying so);
   - `onDocChange` rewrites a missing file from its doc (`:343-352`: pin it; GE-10 gates it);
   - explicit folders and `events.jsonl` lines.
4. **Buffer sync core** (the pure part of `Binding`): the prefix and suffix diff that `applyRemote` computes; local
   edits spliced in descending-offset order; the echo guard; a keystroke during an apply triggers one more convergence
   pass.
5. **Headless peer:** peer A publishes three files to the test hub (waiting for the hub to hold A\'s heads); peer B
   joins by index id and sees them on its disk; an edit on A reaches B.
6. **Playwright smoke tests** (below).

## Checklist

- [ ] `git switch -c ge/08-extension-foundation feature/git-epochs`; from a clean root install record the baseline:
  `npm run typecheck -w quarto-hub-vscode` fails with the missing `vscode` type, `npm run build -w quarto-hub-vscode`
  result, and `cargo xtask lint`. Paste both into "Notes from the run".
- [ ] **Workspace rejoin.** Add `vscode-sync-experiment` to the root `workspaces`; run `npm install` from the repo root
  under the Node in `.nvmrc`; check that `package-lock.json` gains the workspace link and `@types/vscode` and that a
  second `npm install` leaves it unchanged (the failure that caused `8ae461f1b`); confirm `engine-strict` accepts the
  extension; `npm ci` succeeds from a clean checkout. Commit the lockfile on its own.
- [ ] Make the root `npm run typecheck` and `npm run build` pass (fix `tsconfig.json` types if needed so
  `@types/vscode` resolves).
- [ ] Fix the README: the "workspace member" line is true now (check each command in it runs from the root); leave
  the server and folder-placement text, which GE-11 changes.
- [ ] Write test spec 1, run it red, then add `src/core/ignore.ts` with the G23 rule and make `project.ts` use it.
- [ ] **Share-URL extraction.** Create a small package (`ts-packages/quarto-share-url`, name `@quarto/share-url`; detail
  chosen by the plan author; confirm), move `share-url.ts` and its unit tests into it, make
  `quarto-hub-mcp/src/share-url.ts` re-export it so every import and the MCP tests are unchanged, and add the package
  to the extension\'s dependencies. Run `npm run test -w @quarto/hub-mcp` (the share-URL and embed tests) and
  `cargo xtask build-hub-mcp-bundle` to prove the bundle still builds.
- [ ] Write test specs 3 and 4 as characterization tests against the current code (red against the not-yet-extracted
  core: the imports do not exist), then extract.
- [ ] **Extract the core.** Create `src/core/` with: `host.ts` (the interfaces below), `project-sync.ts` (the engine
  from `ProjectSync`, taking a `FileWatcher`, `BufferHost`, `Notifier`, and a logger), `meta.ts` (`ProjectMeta` and the
  read and write of `project.json`, unchanged fields), `create-folder.ts` (`createProjectFolder` without the
  `vscode` import), `text-binding.ts` (the pure diff, splice, and echo-guard logic from `Binding`), and `presence.ts`
  moved as is. Keep `src/extension.ts`, a VS Code `FileWatcher`, `BufferHost`, and `Notifier`, the decoration drawing
  of `Binding`, and the commands in a thin layer. Interfaces (detail chosen by the plan author; confirm):
  `FileWatcher` (`onChange`, `onCreate`, `onDelete`, each taking an absolute path, plus `dispose`), `BufferHost`
  (`openBuffers()`, `getText(path)`, `applyReplace(path, start, end, text)`, `onOpen`, `onClose`, `onEdit`), and
  `Notifier` (`info`, `warn`, `error` with optional actions that resolve to the chosen action, and a status item).
- [ ] The core imports nothing from `vscode`: add a test that fails if any file under `src/core/` contains
  `from 'vscode'`.
- [ ] Confirm the characterization tests and the existing flows pass unchanged on the extracted core; behavior diff
  is empty.
- [ ] **Headless peer.** Add `src/headless/peer.ts` (a library; an optional small CLI entry built by a second esbuild
  entry point; detail chosen by the plan author; confirm) that builds a `ProjectSync` on the core with a Node
  `FileWatcher` (`fs.watch` or a polling fallback), a `BufferHost` with no buffers, and a recording `Notifier`.
  It can `join(serverUrl, indexDocId, dir)` and `publish(dir, serverUrl)` (create the index and file docs, as
  `createNewProject` in `quarto-sync-client` does, using the helpers GE-1 added to the schema package).
  Write test spec 5 first.
- [ ] **Vitest for the extension.** Add `vitest` and a `test` script to `vscode-sync-experiment/package.json`; add the
  workflow step `npm run test -w quarto-hub-vscode` and a typecheck step to the "Workspace TS suites" job of
  `.github/workflows/ts-test-suite.yml`; run `cargo xtask lint` and confirm `ci-test-suite-unwired` is satisfied.
- [ ] **Playwright harness**, in `vscode-sync-experiment/e2e/`: `playwright.config.ts`; `helpers/launch.ts` that
  downloads a pinned VS Code with `@vscode/test-electron` (`downloadAndUnzipVSCode`, cached under `.vscode-test/`) and
  starts it with Playwright\'s `_electron.launch` using `--extensionDevelopmentPath`, throwaway `--user-data-dir` and
  `--extensions-dir`, `--disable-workspace-trust`, `--skip-release-notes`, `--disable-updates`, and a workspace
  folder (detail chosen by the plan author; confirm); `helpers/hub.ts` that starts the in-process test hub from
  `quarto-sync-client` (import by a relative path, or add a `./testing` export to that package; detail chosen by the
  plan author; confirm); `helpers/palette.ts` (run a command, fill an input box); fixtures for a published project and
  a temp git repo helper (shared with later plans).
- [ ] **Smoke test A** (hermetic): a folder whose `.quarto-hub/project.json` names `ws://127.0.0.1:PORT` of the test
  hub auto-starts sync (activation event `workspaceContains:.quarto-hub/project.json`); an edit published by the
  headless peer appears on disk and in the editor; a typed edit reaches the hub.
- [ ] **Smoke test B**: "Quarto Hub: Create Folder from Project" creates the folder, `project.json`, and the files.
  `createProjectFolder` hard-wires the public server, so this test needs a seam: the VS Code layer (not the core)
  reads an environment variable `QUARTO_HUB_TEST_SYNC_SERVER` and passes it as the server; users see no change
  (detail chosen by the plan author; confirm; GE-11 replaces it with the real per-folder binding).
- [ ] **Playwright CI design.** Write `vscode-sync-experiment/e2e/README.md` (how to run locally; the CI shape: Linux,
  `xvfb-run`, VS Code cached by version, trigger on paths under the extension and the schema and swap packages) and add
  `.github/workflows/vscode-extension-e2e.yml` with `workflow_dispatch` only, so it does not gate anything until
  GE-14 chooses the permanent subset (detail chosen by the plan author; confirm). Name the script `test:e2e` (the
  lint checks `test` scripts only).
- [ ] Reconcile this checklist with what landed, update "Notes from the run", and commit.

## Gates

```bash
npm install && npm ci                                   # from a clean root, under the .nvmrc Node
npm run typecheck && npm run build                       # root: must be green, extension included
npm run test -w quarto-hub-vscode                        # core, ignore, headless peer
npm run test -w @quarto/share-url && npm run test -w @quarto/hub-mcp
npm run test:e2e -w quarto-hub-vscode                    # the smoke tests, locally, with a display
cargo xtask build-hub-mcp-bundle && cargo build --bin q2 # the bundle still builds with the extracted helper
cargo xtask lint
```

## End-to-end verification

Smoke tests A and B drive a real VS Code through Playwright against the local test hub. Record the exact commands,
the VS Code version, and a snippet of the observed output (files on disk, editor text, `events.jsonl`) in "Notes
from the run", and say that the output was inspected. A human has not watched the VS Code window; say so.

## Exit criteria

- The extension is a root workspace member; the root typecheck and build are green; a second `npm install` does not
  change the lockfile.
- The ignore rule passes its table; nested `.git`, `.quarto-hub`, and bound subfolders are skipped.
- `src/core/` contains no `vscode` import (a test enforces it) and the characterization tests pass; the three
  existing flows behave as before.
- The headless peer joins and publishes against the test hub; the Playwright smoke tests pass locally.
- The share-URL helpers live in one package and the MCP tests and bundle are unchanged.
- The extension\'s tests are wired into CI and `cargo xtask lint` passes.

## Out of scope

- Any behavior change: modes, the pointer, the gate, attach, join, origination (GE-9 to GE-12).
- The server-URL binding and the unified Join command (GE-11); the interim environment variable is test-only.
- Choosing the permanent Playwright subset and enabling the workflow on pull requests (GE-14).
- Windows behavior of the new test helpers beyond `.claude/rules/cross-platform.md`; the Windows plans cover it.

## Notes from the run

Not started.
