# VS Code end-to-end tests

These tests launch a real VS Code with the extension loaded and drive it with
Playwright's Electron support (`_electron.launch`). The hub is the in-process
test hub from `@quarto/quarto-sync-client/testing`; other collaborators are
headless peers (`src/headless/peer.ts`) or a second client (`Seeder`). Nothing
talks to the network except the one-time VS Code download.

The set here is the GE-8 smoke subset. GE-14 chooses the permanent subset and
turns the workflow on for pull requests.

## Run locally

From the repository root, under the Node in `.nvmrc`:

```bash
npm install
npm run build --if-present -w ts-packages/quarto-sync-client -w ts-packages/quarto-automerge-schema -w ts-packages/quarto-git-epochs -w ts-packages/quarto-share-url
npm run build -w quarto-hub-vscode          # the tests load dist/extension.js
npm run test:e2e -w quarto-hub-vscode       # needs a display (macOS, Windows, or a Linux desktop)
```

The first run downloads VS Code (about 130 MB) into `vscode-sync-experiment/.vscode-test/`.
A run takes about a minute. Pass Playwright arguments through, for example
`npm run test:e2e -w quarto-hub-vscode -- --grep "Create Folder"`.

`npm run test:e2e` goes through `e2e/run.mjs`, not `playwright test` directly. The
workspace packages the tests import publish only a `source` and an `import` entry,
and Playwright loads test files with Node's resolver, so the runner starts Playwright
with `--conditions=source`. `e2e/` and `src/` each have a `package.json` with
`"type": "module"` for the same reason: `@automerge/automerge` cannot be `require`d.
The extension bundle is unaffected (esbuild reads neither file).

## How a launch works

`helpers/launch.ts` downloads the pinned VS Code (`VSCODE_VERSION`; override with
`QUARTO_HUB_E2E_VSCODE_VERSION`) and starts it with:

- `--extensionDevelopmentPath` pointing at this folder;
- a throwaway `--user-data-dir` and `--extensions-dir` under `/tmp` (short paths,
  because macOS limits unix socket paths to about 100 characters);
- `--disable-workspace-trust`, `--skip-release-notes`, `--skip-welcome`,
  `--disable-updates`, `--disable-extensions` (every other extension), and, for
  containers, `--no-sandbox` and `--disable-gpu`;
- user settings with `"window.dialogStyle": "custom"`, because Playwright cannot
  click native modal dialogs and GE-11 and GE-12 use modal confirms.

**The VS Code version must ship Node 22 or later.** The extension's sync client uses
the global `WebSocket`. On VS Code 1.96 (Node 20) the extension activates but sync
fails with `WebSocket is not defined`, although `engines.vscode` says `^1.90.0`.

## The test seam

"Quarto Hub: Create Folder from Project" hard-wires the public sync server. For the
tests, the VS Code layer (not the core) reads `QUARTO_HUB_TEST_SYNC_SERVER` and passes
it as the server. Users see no change. Join Session (GE-11) takes its server from what
the user pastes; the seam remains for Create Folder and Attach Folder, and as the fallback
for a share link without `server=`.

## GE-11 tests

`tests/join.e2e.ts` runs Join Session and Leave Session against a real `q2 preview --allow-edit`
on loopback (`helpers/q2-hub.ts`; run `cargo build --bin q2` first, or set `Q2_BINARY`). The
ticket case sets `quartoHub.q2Path` to a stub `q2` (`src/core/stub-q2.ts`) that forwards TCP to
that hub. A real `--share` and `--join` pair needs the network and is exploratory only.

## GE-9 tests and the timing script

`tests/follower-modes.e2e.ts` runs the mirror and editor-only modes in a real window against the
test hub, with a headless owner (`src/headless/owner.ts`) standing in for the project owner. They
read the disk, the editor text, the status bar, and the notification toasts. Every test in it is a
candidate for the permanent subset (GE-14 decides).

`tests/checkout-reload.timing.ts` is the permanent form of Gate 0 item 12: after `git switch`
across a few thousand files, how long after `index.lock` is released does VS Code reload the last
open buffer? It has its own script, `npm run test:e2e:timing -w quarto-hub-vscode`, because it
fails by design when the machine is slow, and it prints the measured gap. It is not a permanent
subset candidate. `QUARTO_HUB_TIMING_FILES` changes the file count (default 3000).

Two more test-only environment variables, read by the VS Code layer only: `QUARTO_HUB_TEST_FLIP_WAIT_MS`
(editor-only wait for the hub's flip, default 10 000) and `QUARTO_HUB_TEST_SWAP_TIMEOUT_MS` (a follower
swap's load timeout, default 5000).

## CI shape

`.github/workflows/vscode-extension-e2e.yml` runs the suite on Linux under
`xvfb-run`, with VS Code cached by version, and for now only on `workflow_dispatch`.
The intended pull-request trigger is a path filter on `vscode-sync-experiment/**`,
`ts-packages/quarto-automerge-schema/**`, `ts-packages/quarto-git-epochs/**`, and
`ts-packages/quarto-share-url/**` (GE-14 enables it).
