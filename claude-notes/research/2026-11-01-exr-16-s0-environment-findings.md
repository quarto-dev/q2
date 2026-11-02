---
title: 'EXR-16 S0: Positron dev environment, findings so far'
date: 2026-11-01
description: 'Setup steps, timings and a Node version trap for a Positron worktree on Gordon''s fork. Install, daemons and a dev launch work; the Quarto extension is present but has no bundled Quarto CLI; cells and kernels still need a GUI check.'
---

**Question.** How hard is it to get a working Positron dev build with Python and R kernels, and
how long does it take? **Status: partial.** The worktree is installed, vitest runs, the
daemons build clean and the dev build launches. Running a cell and starting Python and R sessions
still need someone at the GUI.

**Worktree.** `/Users/gordon/src/positron-spike-s0`, branch `spike/q2-s0-env`, cut from
`upstream/main` at `d42d28a288` (2026-11-01). Local only. Other spike worktrees:
`positron-spike-s1` (S1, finished), `positron-spike-knr5` (another lane\'s; leave it alone).

## Steps and timings (macOS, arm64)

| Step | Result | Time |
|---|---|---|
| `git worktree add -b spike/q2-s0-env ../positron-spike-s0 upstream/main` | ok | seconds |
| `npm install` with system Node v24.5.0 | **fails in `preinstall`**: needs Node ≥ 24.22.1 (same major as `.nvmrc`) | 162 s wasted |
| Node 24.22.1 tarball from nodejs.org into `/Users/gordon/src/positron-node-24.22.1/`, SHA-256 checked against nodejs.org's `SHASUMS256.txt` | ok | seconds |
| `npm install` with that Node first on `PATH` | ok, exit 0 | 385 s |
| `npx vitest --run .../quartoDocumentModel.vitest.ts` | 41 passed | 3 s |
| `npm run build-start` (Node 24.22.1 on `PATH`) | daemons spawned | 1.9 s |
| `npm run build-check` | client: 0 errors; extensions: 0 errors; transpile 0 errors. `watch-e2e` reports 4 TS2307 (missing `archiver`, `ncp`, `canvas`, `resemblejs`), outside the client build | 28 s |
| `./scripts/code.sh --user-data-dir /tmp/s0-ud` | `Positron.app` from `.build/electron` starts; preLaunch downloads built-in extensions from the marketplace | about 1 min to a window |

## The Node trap

The earlier S1 install used `npm install --ignore-scripts`, which skips `preinstall`, so it ran
on v24.5.0 without complaint. A full install and the dev build need the pinned version. No
version manager is installed on this machine; the Homebrew `node` is v24.5.0. To use the local copy
in any shell:

```bash
export PATH=/Users/gordon/src/positron-node-24.22.1/node-v24.22.1-darwin-arm64/bin:$PATH
```

(Installing `fnm` would make this automatic; not done.)

## Notes

- `npm install` modifies the tracked `package-lock.json`. Do not commit it.
- npm prints `allowScripts` warnings for extension sub-installs (`keytar`, `@vscode/vsce-sign`,
  `esbuild`, `fsevents`). They did not stop the install; whether any matters for the dev build
  is unknown.
- **Quarto extension: present.** preLaunch downloads `quarto.quarto@1.139.0` (and the Jupyter
  extensions) from the marketplace, and the extension host activates it.
- **No bundled Quarto CLI in a dev build.** The extension logs `No pandoc executable found in
  Positron; expected one in <worktree>/quarto/bin/tools`, and `<worktree>/quarto` does not exist.
  Anything that shells out to `quarto` or pandoc will need one on `PATH` or in that directory.
  Cells and inline execution ran in the dev build in S3, S6 and S7 (2026-11-02), so cell
  execution does not need the bundled copy; whether a Quarto CLI was on `PATH` during those runs
  was not checked.
- **Ark: present.** `extensions/positron-r/resources/ark/ark` (30 MB) exists after install. System
  `R` is `/usr/local/bin/R`; `python3` is Homebrew\'s.
- **Short `--user-data-dir`.** A path under the Claude scratchpad overflowed the 103-character
  IPC socket limit (`listen EINVAL`). Use something like `/tmp/s0-ud`.
- Launch log noise that is not a failure: `engines` errors for three `positron-data-explorer-*`
  extensions, Copilot cloud-agent sign-in, a `lock()` request error.

## Not tested

Nothing on the S0 checklist. These stock-Positron checks were done by hand by Gordon: a `.qmd`
runs a cell with inline output, and Python and R (Ark) sessions start. None of them exercises q2
code. S0 only shows that S3, S6 and S2b have a working build to run in.

## Go/no-go

**Go**, in one session. Two things to know when reusing the build: inline output is off by default
and needs `"quarto.inlineOutput.enabled": true`, and a Python interpreter must be chosen
explicitly (uv\'s managed CPython 3.13.7 was killed with SIGKILL on this machine; a venv on
3.12.12 works).
