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

## GE-14: the cross-client scenarios

`tests/cross-client/` holds the two demos of the git-epochs epic, run with real programs and nobody at the keyboard.
They have their own script and config: `npm run test:e2e:cross` (`e2e/playwright.cross-client.config.ts`). It runs the
tests tagged `@permanent`; set `GE14_ALL=1` to include the `@exploratory` ones (a `--grep` then picks among them).

- `hub-demo.e2e.ts` (H1 to H8): a real `q2 preview --git track --ui editor --allow-edit` hub on the GE-13 fixture
  repository, a chromium guest on its editor, a headless mirror, and headless clones, through a commit, a switch, a
  reset, and a switch back. H2e and H4 use a real VS Code window (editor-only and clone mode).
- `hubless-demo.e2e.ts` (L1 to L5): a real VS Code window runs **Share this folder** against the in-process test hub,
  `q2 preview --hub-url` shows the project in the browser, and second peers join as a clone and as a mirror.
- `registry-crosscheck.e2e.ts`: reads the sidecar a real hub wrote through a raw `Repo` and decodes it with the
  TypeScript registry. `UPDATE_GOLDEN=1` rewrites `ts-packages/quarto-git-epochs/fixtures/golden/from-hub-run.bin` and
  its manifest entry; the TypeScript and Rust golden tests decode that file.

What it needs, beyond the extension flows above: `target/debug/q2` built with the hub-client embed (run
`cd hub-client && npm run build:wasm`, `cargo xtask build-q2-preview-spa`, `cargo xtask build-hub-client-embed`, then
`cargo build --bin q2`), Playwright's chromium (`npx playwright install chromium`), and `git`. `jj` is not used here.
`Scene` (`helpers/cross-client.ts`) starts every process, logs the exact invocation of each (`[scene]` lines), and
tears everything down. `GE14_GIT=ignore` is the control run: the hub ignores git, and the scenarios must fail.
