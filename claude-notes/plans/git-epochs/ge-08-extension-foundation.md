---
title: 'GE-8: Extension foundation: workspace, ignore fixes, vscode-free core, headless peer, Playwright harness'
date: 2026-11-18
description: Prepares the VS Code extension for the rest of the epic with no change in behavior, by rejoining the npm workspaces, fixing the ignore rules, extracting a vscode-free core and a headless peer, and adding a Playwright harness that drives VS Code.
status: done
depends-on: [ge-1]
lane: C
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
- **G36** Every path that comes from a doc is validated by one `safeJoin` before the extension touches disk.
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
6. **`safeJoin`** (table-driven, G36): it accepts `a.qmd`, `dir/a.qmd`, and `dir/../a.qmd` inside the root; it rejects
   `../x`, `a/../../x`, `/abs`, `C:\x`, `\\server\share\x`, `a\b`, a path containing NUL, `.git/hooks/h`, `.GIT/x`,
   `sub/.quarto-hub/x`, `.vscode/tasks.json`, `.git.`, `.git ` (trailing space), and `.git::$INDEX_ALLOCATION`; a
   symlinked parent directory that points outside the root is rejected (the case is skipped where the platform cannot
   create symlinks), and so is a directory junction on Windows, which needs no privilege to create; `syncIndex` and `createProjectFolder`
   skip a rejected key, log it, and still apply the other keys.
7. **Playwright smoke tests** (below).

## Checklist

- [x] `git switch -c ge/08-extension-foundation feature/git-epochs`; from a clean root install record the baseline:
  `npm run typecheck -w quarto-hub-vscode` fails with the missing `vscode` type, `npm run build -w quarto-hub-vscode`
  result, and `cargo xtask lint`. Paste both into "Notes from the run".
- [x] **Workspace rejoin.** Add `vscode-sync-experiment` to the root `workspaces`; run `npm install` from the repo root
  under the Node in `.nvmrc`; check that `package-lock.json` gains the workspace link and `@types/vscode` and that a
  second `npm install` leaves it unchanged (the failure that caused `8ae461f1b`); confirm `engine-strict` accepts the
  extension; `npm ci` succeeds from a clean checkout. Commit the lockfile on its own.
- [x] Make the root `npm run typecheck` and `npm run build` pass (fix `tsconfig.json` types if needed so
  `@types/vscode` resolves).
- [x] Fix the README: the "workspace member" line is true now (check each command in it runs from the root); leave
  the server and folder-placement text, which GE-11 changes.
- [x] Write test spec 1, run it red, then add `src/core/ignore.ts` with the G23 rule and make `project.ts` use it.
- [x] **Test hub export (before any test that needs a hub).** Importing `quarto-sync-client`\'s `test-hub.ts` by a
  relative path fails the extension\'s `tsc --noEmit` with TS6059, because its `tsconfig.json` has `rootDir: ./src`
  (reproduced in the third review). Add a `./testing` export to `@quarto/quarto-sync-client` (types and source pointing
  at `src/test-hub.ts`, as its other entries do) and import it as `@quarto/quarto-sync-client/testing` from the
  extension\'s tests and the Playwright helpers. Check that the extension\'s `typecheck` and `build` still pass and
  that the esbuild bundle of the extension does not include it.
- [x] Write test spec 6 and see it fail. Add `src/core/safe-path.ts` with `safeJoin(root, rel)` (G36; it also rejects
  any segment that contains `:` or ends in a dot or a space) and route every
  doc-derived write, `rm`, `mkdir`, and `rmdir` through it: `writeFileDoc`, the removal and folder handling in
  `syncIndex` (`project.ts:313-341`), and `createProjectFolder`. A rejected key is skipped and logged (an event line
  and the output channel), never thrown. GE-16 `b2` runs the hostile-index fixture on Windows.
- [x] **Share-URL extraction.** Create a small package (`ts-packages/quarto-share-url`, name `@quarto/share-url`; detail
  chosen by the plan author; confirm), move `share-url.ts` and its unit tests into it, make
  `quarto-hub-mcp/src/share-url.ts` re-export it so every import and the MCP tests are unchanged, and add the package
  to the extension\'s dependencies. Run `npm run test -w @quarto/hub-mcp` (the share-URL and embed tests) and
  `cargo xtask build-hub-mcp-bundle` to prove the bundle still builds. In the same commit add a step
  `npm test -w ts-packages/quarto-share-url` to `.github/workflows/ts-test-suite.yml` (`ci-test-suite-unwired`), add
  the package to the build step at `:272`, because `quarto-hub-mcp` now depends on it, and add its tests to
  `cargo xtask verify` step 12 (`crates/xtask/src/verify.rs:785-830`; GE-1 extended that step).
- [x] Write test specs 3 and 4 as characterization tests against the current code (red against the not-yet-extracted
  core: the imports do not exist), then extract.
- [x] **Extract the core.** Create `src/core/` with: `host.ts` (the interfaces below), `project-sync.ts` (the engine
  from `ProjectSync`, taking a `FileWatcher`, `BufferHost`, `Notifier`, and a logger), `meta.ts` (`ProjectMeta` and the
  read and write of `project.json`, unchanged fields), `create-folder.ts` (`createProjectFolder` without the
  `vscode` import), `text-binding.ts` (the pure diff, splice, and echo-guard logic from `Binding`), and `presence.ts`
  moved as is. Keep `src/extension.ts`, a VS Code `FileWatcher`, `BufferHost`, and `Notifier`, the decoration drawing
  of `Binding`, and the commands in a thin layer. Interfaces (detail chosen by the plan author; confirm):
  `FileWatcher` (`onChange`, `onCreate`, `onDelete`, each taking an absolute path, plus `dispose`), `BufferHost`
  (`openBuffers()`, `getText(path)`, `applyReplace(path, start, end, text)`, `onOpen`, `onClose`, `onEdit`), and
  `Notifier` (`info`, `warn`, `error` with optional actions that resolve to the chosen action, and a status item).
- [x] The core imports nothing from `vscode`: add a test that fails if any file under `src/core/` contains
  `from 'vscode'`.
- [x] Confirm the characterization tests and the existing flows pass unchanged on the extracted core; behavior diff
  is empty.
- [x] **Headless peer.** Add `src/headless/peer.ts` (a library; an optional small CLI entry built by a second esbuild
  entry point; detail chosen by the plan author; confirm) that builds a `ProjectSync` on the core with a Node
  `FileWatcher` (`fs.watch` or a polling fallback), a `BufferHost` with no buffers, and a recording `Notifier`.
  It can `join(serverUrl, indexDocId, dir)` and `publish(dir, serverUrl)` (create the index and file docs, as
  `createNewProject` in `quarto-sync-client` does, using the helpers GE-1 added to the schema package; plain projects only, GE-10 adds the git-enabled variant).
  Write test spec 5 first.
- [x] **Vitest for the extension.** Add `vitest` and a `test` script to `vscode-sync-experiment/package.json`; add the
  workflow step `npm run test -w quarto-hub-vscode` and a typecheck step to the "Workspace TS suites" job of
  `.github/workflows/ts-test-suite.yml`, and in the same commit the `cargo xtask verify` counterpart: the extension is
  not under `ts-packages/*`, so rejoining the workspaces does not add it to verify (the step 6 build and step 12 tests
  enumerate `ts-packages` only); add its typecheck, build, and `npm run test -w quarto-hub-vscode` to
  `crates/xtask/src/verify.rs`. Run `cargo xtask lint` and confirm `ci-test-suite-unwired` is satisfied.
- [x] **Playwright harness**, in `vscode-sync-experiment/e2e/`: `playwright.config.ts`; `helpers/launch.ts` that
  downloads a pinned VS Code with `@vscode/test-electron` (`downloadAndUnzipVSCode`, cached under `.vscode-test/`) and
  starts it with Playwright\'s `_electron.launch` using `--extensionDevelopmentPath`, throwaway `--user-data-dir` and
  `--extensions-dir`, `--disable-workspace-trust`, `--skip-release-notes`, `--disable-updates`, `--no-sandbox` and
  `--disable-gpu` (containers), a short `--user-data-dir` path (macOS socket paths are limited to about 100
  characters), user settings with `"window.dialogStyle": "custom"` (Playwright cannot click native modal dialogs,
  and GE-11 and GE-12 use modal confirms), and a workspace folder (details chosen by the second reviewer; confirm); `helpers/hub.ts` that starts the in-process test hub through the `./testing` export added above; `helpers/palette.ts` (run a command, fill an input box); fixtures for a published project and
  a temp git repo helper (shared with later plans).
- [x] **Smoke test A** (hermetic): a folder whose `.quarto-hub/project.json` names `ws://127.0.0.1:PORT` of the test
  hub auto-starts sync (activation event `workspaceContains:.quarto-hub/project.json`); an edit published by the
  headless peer appears on disk and in the editor; a typed edit reaches the hub.
- [x] **Smoke test B**: "Quarto Hub: Create Folder from Project" creates the folder, `project.json`, and the files.
  `createProjectFolder` hard-wires the public server, so this test needs a seam: the VS Code layer (not the core)
  reads an environment variable `QUARTO_HUB_TEST_SYNC_SERVER` and passes it as the server; users see no change
  (detail chosen by the plan author; confirm; GE-11 replaces it with the real per-folder binding).
- [x] **Playwright CI design.** Write `vscode-sync-experiment/e2e/README.md` (how to run locally; the CI shape: Linux,
  `xvfb-run`, VS Code cached by version, trigger on paths under the extension and the schema and swap packages) and add
  `.github/workflows/vscode-extension-e2e.yml` with `workflow_dispatch` only, so it does not gate anything until
  GE-14 chooses the permanent subset (detail chosen by the plan author; confirm). Name the script `test:e2e` (the
  lint checks `test` scripts only).
- [x] Reconcile this checklist with what landed, update "Notes from the run", and commit.

## Gates

```bash
npm install && npm ci                                   # from a clean root, under the .nvmrc Node
npm run typecheck && npm run build                       # root: must be green, extension included (the root build also runs the hub-client and preview-SPA builds, which need the network for the pandoc fetch; where that is unavailable, build -w quarto-hub-vscode and the ts-packages instead)
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
- The ignore rule passes its table; nested `.git`, `.quarto-hub`, and bound subfolders are skipped; `safeJoin`
  passes its table, and a hostile index key is skipped and logged.
- `src/core/` contains no `vscode` import (a test enforces it) and the characterization tests pass; the three
  existing flows behave as before.
- The headless peer joins and publishes against the test hub, through the `./testing` export; the Playwright smoke
  tests pass locally. The extension\'s typecheck, build, and tests run in `cargo xtask verify` as well as in CI.
- The share-URL helpers live in one package and the MCP tests and bundle are unchanged.
- The extension\'s tests are wired into CI and `cargo xtask lint` passes.

## Out of scope

- Any behavior change: modes, the pointer, the gate, attach, join, origination (GE-9 to GE-12).
- The server-URL binding and the unified Join command (GE-11); the interim environment variable is test-only.
- Choosing the permanent Playwright subset and enabling the workflow on pull requests (GE-14).
- Windows behavior of the new test helpers beyond `.claude/rules/cross-platform.md`; the Windows plans cover it.

## Notes from the run

**Session 1 (lane C, 2026-11-18).** Branch `ge/08-extension-foundation` off `feature/git-epochs` at `aefd896b9`
(GE-1 landed).

- **Baseline (before any change).** From the root install, `npm run typecheck -w quarto-hub-vscode` failed with
  `error TS2688: Cannot find type definition file for 'vscode'`; `npm run build -w quarto-hub-vscode` passed
  (`dist/extension.js` 5.3 MB). I did not record a `cargo xtask lint` baseline before the first change. The first
  lint run after I added the extension's `test` script failed `ci-test-suite-unwired`, which the CI wiring commit
  fixed; the final run is clean (1,736 files).
- **Workspace rejoin.** `vscode-sync-experiment` is in the root `workspaces`. As GE-1 found, a plain `npm install` on
  this host rewrites `package-lock.json` and prunes the `lightningcss-*` platform entries (and does so again on a
  second install, with or without my change), so I never committed its output. I took the entries npm added for the
  extension (`@types/vscode`, the workspace link, and later `@vscode/test-electron` and its dependencies, vitest,
  `@quarto/share-url`) and merged only those into the committed lockfile, with a small script that adds keys and
  never removes any. Checks: `npm ci --dry-run` accepts it; a real `npm ci --ignore-scripts` from a `git archive` of
  the branch succeeded; in that clean copy the extension's typecheck, build, and 92 tests pass. **The "a second `npm
  install` leaves the lockfile unchanged" criterion cannot be met on this host** (the prune, which is independent of
  this plan); someone with a host that does not prune should confirm it. Root `npm run typecheck`: exit 0 (every
  workspace, the extension and its `e2e/` project included). Root `npm run build` was **not** run in full (it builds
  the hub-client and preview SPA, which need the WASM build); the extension\'s own build passes, as do the ts-package
  builds, so the checklist line for the root build is only partly checked.
- **`dist/` was tracked by accident.** Commit `251de0df0` moved the folder out of `hub-client`, where a `.gitignore`
  hid `dist/`, and committed the 19,000-line bundle. Every build would dirty it, and `verify` now builds the
  extension, so I untracked it and added `dist/`, `.vscode-test/`, and `test-results/` to the extension's
  `.gitignore`. Not in the plan; small and needed.
- **Ignore rule (G23).** `src/core/ignore.ts`, a pure `isIgnoredPath(rel, boundSubfolders)`: 26 table cases, red on the
  missing module first. A bound subfolder is detected in `ProjectSync.listDiskFiles` (a directory holding
  `.quarto-hub/project.json`) and covered by a characterization test.
- **`safeJoin` (G36).** `src/core/safe-path.ts`; async, because it resolves symlinks and junctions of the nearest
  existing ancestor with `fs.realpath`. 28 cases (all of the plan's, plus `x.git/y` and `.gitignore` accepted). The
  symlink cases use `junction` (which creates a plain symlink off Windows). A rejected key is skipped and logged:
  an output line `skipped index key: …` and, in `ProjectSync`, a `rejected` line in `events.jsonl` (new event
  name), once per key per run. `createProjectFolder` and `syncIndex` tests cover "still applies the other keys".
- **`./testing` export** added to `@quarto/quarto-sync-client`; the extension's `tsc` accepts the import (no TS6059)
  and the esbuild bundle does not contain it (grep for `startTestHub` is 0).
- **Share-URL package.** `ts-packages/quarto-share-url` (`@quarto/share-url`; the plan author's name taken). The
  MCP file re-exports it. `npm run test -w @quarto/hub-mcp`: 621 passed, 8 failed on the 5 s default timeout in a
  cold worktree (write-path tests); the same files pass at `--testTimeout=60000`, so it is first-run slowness, not
  this change. `cargo xtask build-hub-mcp-bundle` succeeds. Wired into `ts-test-suite.yml` (build line and test step)
  and `cargo xtask verify` step 12.
- **Core extraction.** `src/core/`: `host.ts`, `meta.ts`, `repo.ts`, `wasm.ts`, `create-folder.ts` (also exports
  `writeProjectFiles`, shared with the headless peer), `project-sync.ts`, `text-binding.ts`, `presence.ts` (moved,
  unchanged), `ignore.ts`, `safe-path.ts`, `recording.ts`. The VS Code layer is `binding.ts` (cursors, on
  `TextBindingCore`), `vscode-host.ts`, and `extension.ts`; `project.ts` is gone. Tests, written first and red on the
  missing modules: `project-sync.test.ts` (19, characterization of today's engine on fakes and an in-process hub),
  `text-binding.test.ts` (16), `no-vscode.test.ts` (1). Behavior pinned for later plans, with comments in the code and
  the tests: `syncIndex` untracks, retracks, and overwrites unconditionally (GE-9 replaces it) and `onDocChange`
  rewrites a file missing from disk (GE-10 gates it). The existing flows work as before: the same code, moved, and
  smoke tests A and B on a real VS Code.
  - **Deviation (interfaces; "confirm" item).** The plan's `BufferHost` had `applyReplace` and `onEdit`. Mine has
    `bind(absPath, handle, identity)` instead, because the VS Code binding owns presence and cursor decorations as well
    as the text sync; the shared text logic (diff, splice, echo guard, convergence pass) is `TextBindingCore`, which
    both the VS Code `Binding` and the test fakes use. `FileWatcher` and `Notifier` are as planned (`Notifier.status`
    takes an optional timeout). Also new: `ProjectSync.inventory()` (docs and heads, for the tests' hub waits).
  - **One real difference from the old behavior, on purpose:** none intended. `relOf` takes an absolute path (the old
    one a `vscode.Uri`) and the status-bar message is shown from inside `ProjectSync.start` through the notifier.
- **Headless peer.** `src/headless/peer.ts` (`HeadlessPeer.publish`/`join`, Node `fs.watch`, no buffers). `publish`
  writes an empty index to local storage and lets the startup reconciliation turn each file into a doc. 2 tests (A
  publishes three files; B joins and sees them; an edit and a new file on A reach B; ignored paths are not published),
  stable over three runs. macOS `fs.watch` reports many rewrites as `rename`, so an existing file's `rename` is
  reported as both created and changed. No command-line entry (the plan called it optional).
- **Vitest for the extension.** `vitest.config.ts` resolves workspace packages from `source` (also `ssr.resolve`), so
  the tests need no built `dist/` of the other packages. CI step "Typecheck, build, and test the VS Code extension"
  in the "Workspace TS suites" job; `cargo xtask verify` step 12 runs the same three commands. `cargo xtask lint`
  passes; `cargo clippy -p xtask --all-targets -- -D warnings` clean; `cargo nextest run -p xtask` 269 passed.
- **Playwright harness.** `e2e/` with the files the plan lists, plus `e2e/run.mjs`, `e2e/package.json` and
  `src/package.json` (both `"type": "module"`), and `helpers/git.ts` (the temp git repo helper). Two findings:
  1. **Playwright's CommonJS loader cannot load `@automerge/automerge`** (`require` reaches a `.js` file in a
     `"type": "module"` package) and the workspace packages export no `require` entry. So the tests run as ESM (the
     two `package.json` files) and `run.mjs` starts Playwright with `--conditions=source`. The extension bundle is
     unaffected (esbuild reads neither file; smoke tests ran on the rebuilt bundle).
  2. **Existing limitation, not fixed: sync fails on a VS Code with Node 20.** On VS Code 1.96.4 the extension
     activates but every connection attempt logs `ReferenceError: WebSocket is not defined` (the browser adapter
     uses the global `WebSocket`, which Node 22 has). `engines.vscode` says `^1.90.0`. I pinned the harness to
     1.101.2 (Node 22), with a comment. GE-9 or GE-11 should either raise `engines.vscode` or give the adapter a
     `ws` fallback; I left the product alone (no behavior change in this plan).
- **End-to-end (smoke tests A and B), VS Code 1.101.2 on macOS arm64, a real window, nobody watching it.**
  Invocation: `npm run test:e2e -w quarto-hub-vscode` from the repo root (after building the ts-packages and
  `npm run build -w quarto-hub-vscode`). Output:
  ```text
  hub doc text: "# Home\n\nfrom A, edited\ntyped in VS Code"
  ✓ A: a folder bound to the test hub auto-starts sync, in both directions (45.9s)
  ✓ B: "Quarto Hub: Create Folder from Project" creates the folder, project.json, and the files (2.4s)
  2 passed (49.6s)
  ```
  Test A asserts the edit made by peer A on the workspace's disk, in the editor's rendered text, and the typed edit
  in the hub's document; test B asserts `docs/guide.qmd`, `index.qmd`, and `project.json` (`syncServer` is the hub).
  I inspected the printed hub document text; the assertions read disk, editor, and hub, not a screenshot.
- **CI design.** `.github/workflows/vscode-extension-e2e.yml`, `workflow_dispatch` only, Linux under `xvfb-run`, VS
  Code cached by the version read from `launch.ts`. **Not run on GitHub**; the shape is untested there, and the
  `apt-get` package list is my guess for ubuntu-latest. The workflow has no gate until GE-14.
- **Details taken from the plan ("confirm" items):** package name `@quarto/share-url`; `FileWatcher`/`BufferHost`/
  `Notifier` shapes (with the `BufferHost` deviation above); headless peer as a library only; the
  `QUARTO_HUB_TEST_SYNC_SERVER` seam in the VS Code layer; the e2e workflow manual only; launch options as listed.
- **Not run:** a Windows cross-clippy (no Rust code beyond `xtask`\'s `verify.rs`, which uses no platform API); the
  smoke tests on Linux or Windows; `cargo xtask verify` (Gordon's call); the full hub-client build.
