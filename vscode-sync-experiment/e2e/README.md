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
it as the server. Users see no change; GE-11 replaces it with the real per-folder
binding.

## CI shape

`.github/workflows/vscode-extension-e2e.yml` runs the suite on Linux under
`xvfb-run`, with VS Code cached by version, and for now only on `workflow_dispatch`.
The intended pull-request trigger is a path filter on `vscode-sync-experiment/**`,
`ts-packages/quarto-automerge-schema/**`, `ts-packages/quarto-git-epochs/**`, and
`ts-packages/quarto-share-url/**` (GE-14 enables it).
