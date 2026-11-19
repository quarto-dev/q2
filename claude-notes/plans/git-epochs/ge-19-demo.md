---
title: 'GE-19: The demo and playground'
date: 2026-11-19
description: 'Builds a permanent, replayable demo of git-aware live share on three windows (the host editor, a real terminal, the guest editor), with scripted narrated playback and free play on the same real binaries, a mycology journal with live R and Python papers, continuous invariant checks, and a complete automated test suite.'
status: approved  # designed with Gordon on 2026-11-19, after the epic's e2e plans landed
depends-on: [ge-14]
lane: B
epic: ../2026-11-18-git-epochs-epic.md
---

**Epic:** [git epochs](../2026-11-18-git-epochs-epic.md) · **Waits for:** [GE-14](ge-14-cross-client-e2e.md) ·
**Unblocks:** [GE-18](ge-18-docs-and-closeout.md) · **Branch:** `ge/19-demo` off `feature/git-epochs`, merged by rebase
and fast-forward; nothing is pushed.

## Goal

Make the system visible. Today the demos exist only as automated scenarios (GE-13, GE-14): each test starts its own
hub, most peers are headless, and the git commands that cause everything run out of sight in `execFileSync`. This plan
builds one environment that puts three real windows on the screen:

- **upper left:** the host's editor, a browser on the hub's editor UI or a VS Code window;
- **lower left:** the host's **terminal**, a browser page running a terminal emulator backed by a real shell in the
  host's repository;
- **right:** the guest's editor, a browser or a VS Code window, with a guest terminal below it in presets where the
  guest has its own repository.

The terminal is the protagonist: the git commands happen there, and their effects show in the two editors. The same
environment runs in two ways:

- **Scripted playback.** A driver types each command into the terminal at a human pace, writes a caption before it,
  waits for the effect with retrying checks, and pauses. It is the recorded demo.
- **Free play.** A person types into the terminal (and into the editors). Every window is a real live-share or VS Code
  window on the real binaries, so this is a playground for trying scenarios by hand, including the ones the automated
  suites leave exploratory (H4, H8, L5, rebase conflicts). An observer narrates what the system did and checks its
  promises continuously.

Gordon arranges and records the screen; the launcher places the windows by default.

## Decisions of this plan

Settled with Gordon on 2026-11-19. They add to the epic and change none of its decisions.

| Id | Decision |
|---|---|
| DM1 | **One environment, two drivers.** A Node launcher (the Playwright library, not the test runner: it runs until Ctrl-C) starts the binaries and the windows. Scripted playback and free play run on the same environment; the scripted driver is a layer on top. The demo does not slow down or edit the permanent specs of GE-13 and GE-14; it reuses their helpers (`q2-preview-spa/e2e/helpers/*`, `vscode-sync-experiment/e2e/helpers/*`) |
| DM2 | **The terminal** is a page with xterm.js, connected over a WebSocket to a small Node server that runs a real shell through node-pty in the host's repository (a second one in the guest's repository when the preset has a guest terminal). The server binds to loopback only and requires a random token in the page URL. The shell gets the demo environment: the fixed git identity and empty global config that `GitRepo` uses, `R_LIBS_USER` (DM10), and, in scripted mode, `GIT_PAGER=cat` and a non-interactive editor |
| DM3 | **Captions and narration go in the terminal window, in italics** (dim italic for captions and narration, red for invariant warnings). Two options decide where, each `inline`, `strip`, `both`, or `off`: `--captions` (scripted step captions; default `inline`) and `--narration` (free-play observer narration; default `strip`). `inline` writes on the terminal display above the prompt, never into the PTY's input; in free play it is best-effort and may garble a half-typed line on screen (the shell still receives exactly what was typed). The strip is two or three lines at the bottom of the terminal window, below a thin rule, outside the shell's text. Invariant warnings follow `--narration` but are never suppressed: with `off` they go to the strip |
| DM4 | **Pacing:** `--pace auto` (after each step's check passes, hold for a dwell, for recording) or `--pace key` (advance on a key in the terminal, for presenting). A failed check stops the run, with the failure shown in the terminal. `--fast` runs the scripted story headless with no pacing and every check: the rot check |
| DM5 | **Layout.** Placed on the left (main) monitor by default, measured at start with `NSScreen` (the usable area there was 1920 × 1250 points from y = 30 on 2026-11-19). Two columns of 960 points; in each, editor and terminal split 3:1 (938 and 312 points). A guest without a terminal gets the whole right column. The rectangles live in one config table, with `--screen` to pick a monitor. Browsers are placed with CDP `Browser.setWindowBounds`, VS Code with `BrowserWindow.setBounds` through Playwright's Electron `app.evaluate` |
| DM6 | **Presets, built in this order:** (1) `hub-browser`: browser host, browser guest; (2) `hub-vscode-host`: VS Code editor-only host on the hub's folder, browser guest; (3) `hub-vscode-clone`: browser host, VS Code clone guest with its own terminal; (4) `hubless`: a real `q2 hub` on loopback started in the host terminal, a VS Code originator running Share this folder on camera (command palette, modal, copied link), and a browser guest whose terminal pastes the copied `q2 preview --hub-url ...` command |
| DM7 | **The scripted hub story,** one hub on one repository, four beats: (1) commit: Maurice types in his paper while Myra commits; nothing swaps and no notice appears; (2) `git switch feature` and back: the guest swaps with the notice, the corrigendum appears, the retracted pilot study leaves, the micrograph changes, the R paper re-executes; then all of it reverses; (3) `git reset --hard` after a guest edit: the doc is retired and the status reports the discarded edit; (4) a rebase onto `revision` that stops on the conflict, the paused indicator, the resolution, `git rebase --continue`, one swap. The hub itself is started on camera in the host terminal (`q2 preview --git track --ui editor --allow-edit`), in the background with its log in a file; the driver waits on `/health` |
| DM8 | **Free play** has continuous invariant checks, shown as red warnings: no conflict marker in any doc; when quiet, the hub's disk equals the doc its pointer names; a clone on another branch never has its disk rewritten; every follower shows the pointer's version. `demo reset` restores the starting state without closing the windows (DM12) |
| DM9 | **The content** is a journal issue, *Journal of Theoretical Mycodynamics*, vol. 17, issue 3, about *Stochastic Anastomosis Kinetics in Heterokaryotic Mycelial Networks Under Oscillating Substrate Humidity*. The authors, by nominative determinism: **Dr. Myra Celia** (the host; the R paper) and **Dr. Maurice Morel** (the guest; the Python paper); the retracted pilot study is by **Rott & Mildew**. Every page except `index.qmd` is a journal paper (title, authors and affiliations at the Institute for Applied Mycodynamics, a fake DOI, abstract, keywords, the usual sections, references); `index.qmd` is the issue's table of contents. Filenames use citation keys. Warm-up papers have no code; the R paper uses ggplot2 in live chunks; the Python paper uses plotnine in live chunks. The stats and charts are inscrutable and very scientific. The tests' fixture (GE-13) is unchanged |
| DM10 | **Engines.** Live chunks, executed by the hub, with `preview.engine: auto` in `_quarto.yml` so a change re-executes without a click. R packages come from an **rv** project in the parent of the journal (rv fixes its library at `rv/library` beside `rproject.toml`, with no setting to move it); the launcher runs `rv sync` there and starts the hub and the terminals with `R_LIBS_USER` set to the absolute library path (`rv library`). The rv repository is Posit Package Manager, for macOS binaries. Python comes from a uv project in the journal (`pyproject.toml`, `uv.lock`, a `.venv` that discovery skips as a hidden directory) with plotnine and ipykernel; the launcher runs `uv sync` and makes the venv's kernelspec findable. The launcher checks for R, rv, uv, and `q2`, and prints the exact install advice when one is missing; it installs nothing else itself |
| DM11 | **Platform:** macOS only, tested here. The pieces may work elsewhere; nothing promises it |
| DM12 | **Reset:** stop the hub (and `q2 hub` and the `--hub-url` preview in `hubless`); rebuild the journal from the fixture builder (and the guest clone); keep `rv/library` and `.venv`; start a fresh hub (a new project id, G19); navigate browsers to the new share URL; rejoin VS Code windows through Join session, accepting the G19 rebind confirm; clear the strip and the observer; print *the journal has been restored to its starting state* |
| DM13 | **Tests:** complete and automated (see Test specifications), run by a standalone `test:demo` script, not by CI and not by `cargo xtask verify`, because they need R, rv, uv, VS Code, and a display. They open real VS Code windows on the screen, as the existing VS Code e2e tests do |

## Context

Checked at the integration tip `32478b2b8` on 2026-11-19; re-find the code against the current tip.

- **What exists.** GE-13: `q2-preview-spa/e2e/embed-git-epochs.spec.ts` (S1 to S13), `git-epochs-viewer.spec.ts`,
  `git-epochs-helpers.spec.ts`, and the helpers `gitRepo.ts` (`GitRepo`, `buildFixtureRepo`, `MAIN_FILES`,
  `pngBytes`), `gitScenario.ts` (`startGitHub`, `openGuest`, `expectDisk`, `waitForDecision`), `editor.ts`
  (`openProject`, `openFile`, `typeInEditor`, `noticeLocator`, `pausedIndicator`, `shownText`, `gitStatus`), and
  `previewServer.ts` (`startPreviewServer` with `gitRepo`, `git`, `hubUrl`, `project`). GE-14: under
  `vscode-sync-experiment/e2e/`, `helpers/cross-client.ts` (`Scene`: `trackingHub`, `previewOver`, `standaloneHub`,
  `guest`, `mirror`, `clone`, one `[scene]` log line per invocation), `helpers/launch.ts` (`launchVscode` through
  `@vscode/test-electron` and `_electron.launch`, VS Code 1.101.2, settings including `window.dialogStyle: custom`),
  `helpers/registry-dump.ts`, and `tests/cross-client/*.e2e.ts`; `e2e/run.mjs` adds `--conditions=source`, which the
  launcher needs too.
- **Nothing paces today.** No spec or config uses headed mode, `slowMo`, or `test.step`; every wait is a retrying
  assertion (`.claude/rules/hub-client-tests.md`). Each scenario starts its own hub and repository.
- **Engines in the preview.** `preview.engine` in `_quarto.yml` is `manual` (default: eager capture, staleness on edit,
  a Re-execute button), `auto` (re-execute on every settled code-cell change), or `off`
  (`crates/quarto-preview/src/config.rs`, `EnginePolicy`); captures are written by `capture_driver.rs`
  (`record_eager_captures`, `recompute_staleness`), and G17 marks captures stale on a flip. The knitr engine runs
  `Rscript` without `--vanilla`, in the project directory unless the document's directory activates renv
  (`determine_working_dir`, `crates/quarto-core/src/engine/knitr/subprocess.rs`). Jupyter kernels are found through
  kernelspecs (`runtimelib`, `JUPYTER_PATH`; `crates/quarto-core/src/engine/jupyter/kernelspec.rs`, `daemon.rs`).
- **rv** (0.23.3 here): `rproject.toml` and `rv.lock`; packages in `rv/library/<r-version>/<arch>`; an `.Rprofile` that
  sources `rv/scripts/rvr.R` and `rv/scripts/activate.R`, which runs `rv info` in R's working directory and puts the
  library (made absolute) on `.libPaths()` and `R_LIBS_USER`. `rproject.toml` has no library setting. q2 and Quarto 1
  special-case renv only (strand **bd-er8e6nmq**, with the Quarto 1 check in its comments): an `rv/` directory inside a
  project is not skipped by discovery. That is why the rv project sits in the journal's parent (DM10); the strand is
  not this plan's work.
- **Installed here on 2026-11-19:** R 4.3.2 with ggplot2 4.0, knitr, rmarkdown, palmerpenguins, patchwork, viridisLite;
  rv 0.23.3 (`/opt/homebrew/bin/rv`); uv is expected on `PATH` (check).
- **The extension package** is `quarto-hub-vscode` (`vscode-sync-experiment/package.json`); it has no xterm, node-pty,
  or ws dependency today. `ci-test-suite-unwired` checks only a package's `test` script, so `test:demo` needs no CI
  wiring.

## Design

### Where it lives

`vscode-sync-experiment/demo/`, beside `e2e/`, because it imports both the preview helpers and the extension core, as
the cross-client scenarios do:

```text
vscode-sync-experiment/demo/
  launch.mjs            entry point: npm run demo -w quarto-hub-vscode -- --preset hub-browser [options]
  src/                  launcher, presets, layout, terminal server, observer, invariants, driver, reset
  terminal/             the xterm.js page (bundled with esbuild)
  stories/              the scripted stories, one per preset
  journal/              the fixture builder and the journal content (papers, R and Python setup, figures)
  tests/                unit tests (vitest) and end-to-end tests (Playwright, own config)
```

Scripts in the extension package: `demo` (the launcher) and `test:demo` (DM13).

### The runtime directory

```text
<demo-root>/              a temporary directory per launch (kept across demo reset)
  rproject.toml, rv.lock  the rv project (copied from demo/journal/)
  rv/library/...          installed by rv sync, outside the served project
  journal/                Myra's git repository, the project the hub serves
  maurice/                Maurice's clone (presets with a clone guest)
  logs/                   hub, q2 hub, and preview logs; the step and event log (JSONL)
```

### The journal

Citation-key filenames; `index.qmd` is the table of contents. Final titles, prose, and numbers are written in the
plan's content phase; the shape is fixed here.

| File | Paper | Author | Engine | Role |
|---|---|---|---|---|
| `index.qmd` | The issue's table of contents | none | none | The page the share link opens |
| `celia-morel-2026-anastomosis-kinetics.qmd` | The flagship paper, with the network micrograph (`figures/micrograph.png`) | Celia & Morel | none | Warm-up; the micrograph is the binary that changes on `feature` |
| `celia-2026-humidity-chamber-calibration.qmd` | A methods paper with a reagent table | Celia | none | Warm-up |
| `celia-2026-tip-extension-lmm.qmd` | Hyphal tip extension under three humidity regimes: a mixed-effects model, ridge plots, variance components, a log scale | Celia | R, ggplot2 | `feature` changes the model, so it re-executes; the rebase conflict is in its abstract (*p* = 0.049 against 0.051) |
| `morel-2026-fusion-spectra.qmd` | Fusion events as a point process: a periodogram, a fusion-competence heatmap, a bootstrap band | Morel | Python, plotnine | Maurice types here in the commit beat |
| `rott-mildew-2025-preliminary-fusion.qmd` | The pilot study | Rott & Mildew | none | Removed on `feature` (retracted) |
| `celia-2026-corrigendum-variance-components.qmd` | The corrigendum | Celia | none | Added on `feature` |

Branches: `main`; `feature` (Myra's reanalysis: the abstract rewritten, the micrograph recolored, the model changed,
the pilot study removed, the corrigendum added); `revision` (edits the same abstract line as `main`, for the conflict).
The builder is deterministic (fixed dates, identity, and bytes), as GE-14 requires of its fixture. Data in the papers is
simulated with fixed seeds. `_quarto.yml` sets `preview.engine: auto`.

### The windows and the terminal

The launcher measures the screens, computes the rectangles (DM5), opens one headed Chromium with a window per
browser surface, launches VS Code through `launchVscode` where the preset needs it, and places every window. The
terminal page is served by the terminal server; it renders xterm.js, the optional strip, and accepts input. Captions
and narration reach it over the same WebSocket as a separate message type, so they never pass through the PTY.

### The observer, the log, and the invariants

The observer polls `/api/preview/git-status`, reads the hub's disk and every peer folder, reads doc contents through a
raw `Repo` on the hub's `/ws` (as `registry-dump.ts` does), and receives the terminals' typed command lines. It writes
one JSON line per event (time, source, kind, summary, details) to `logs/events.jsonl`, and the scripted driver writes its
step lines (id, caption, command, check, result) to the same file. Narration text is a pure function of an event. The
invariant checker runs after each event and on a timer, and emits warning events.

### The scripted stories

`hub-browser`, `hub-vscode-host`, and `hub-vscode-clone` play DM7; in `hub-vscode-clone` the clone also runs its own
`git switch` in its terminal (the H4 story). `hubless` plays: start `q2 hub` in the host terminal, Share this folder in
VS Code with the modal on camera, the guest terminal pastes the `q2 preview --hub-url` command, the guest opens the
journal, Maurice edits, Myra runs `git switch feature`, the guest follows. Every step has a caption, a command or
action, and a check built from the GE-13 and GE-14 helpers.

## Research first (phase 0)

Each item is a short experiment whose answer goes in the notes before the design depends on it. If one fails, record it
and stop as blocked with a recommendation (the epic's stop conditions), unless the item says what to do instead.

1. **node-pty** installs and runs under the Node in `.nvmrc` on macOS arm64 (prebuilt or built with the Xcode command
   line tools), from the extension workspace. Fallback: a maintained prebuilt fork; record which.
2. **xterm.js** renders italic and dim text, and a message written to the display does not reach the PTY.
3. **Window placement:** CDP `Browser.setWindowBounds` places a headed Chromium window at given screen points on the
   main monitor; `app.evaluate` with `BrowserWindow.setBounds` places VS Code. Record any offset (title bar, menu bar).
4. **R under the hub through rv:** a `{r}` chunk with ggplot2 renders in the preview when the hub runs with
   `R_LIBS_USER` set to an rv library outside the project, and the R packages the paper needs install with `rv sync`.
5. **Python under the hub through uv:** a `{python}` chunk with plotnine renders in the preview using the uv `.venv`.
   Find what makes the kernelspec findable (`JUPYTER_PATH` pointing at `.venv/share/jupyter`, or an explicit kernel in
   the front matter) and record it.
6. **Engines across a flip:** with `preview.engine: auto`, after `git switch feature` changes the R paper's code, the
   guest shows the re-executed chart; record what the guest sees in between (stale overlay, old output, nothing) and
   how long it takes. This is the new interaction the playground exists to exercise: an unexpected result is a finding
   for Gordon, not a reason to drop live chunks.
7. **Live chunks in `hubless`:** with no project directory on either q2 process, the code papers probably show their
   code with no output. Confirm; if so, the `hubless` story uses the warm-up papers and the gap is stated in its
   captions.

## Test specifications

Written first; each is seen failing for the right reason before the piece it tests exists. The retrying-assertion
rules of `.claude/rules/hub-client-tests.md` apply to every end-to-end test; no fixed sleeps. The demo's own pacing
(DM4) is product behavior of the demo, not a synchronization point, and `--fast` turns it off.

**Unit tests (vitest), in `demo/tests/unit/`:**

| Piece | What must hold |
|---|---|
| Journal builder | Two builds give byte-identical repositories (same blob ids); `main`, `feature`, and `revision` hold exactly the specified files; rebasing `revision` onto `main` stops with a conflict in the R paper's abstract |
| Terminal server | Binds to loopback only; refuses a connection without the token; spawns the shell in the given directory with the demo environment; reports typed command lines; a display message is never written to the PTY |
| Captions and strip | `inline`, `strip`, `both`, and `off` route captions and narration as DM3 says; italic and dim codes are applied; warnings ignore `off`; the strip keeps its last lines |
| Observer | Recorded sequences of status-route bodies and disk states produce the expected events and narration (a flip of N files, a pause and its end, a retire with `discarded`, a removal and a re-listing) |
| Invariant checker | Each invariant passes on good states and fires on a violation: a conflict marker in a doc, a clone's disk rewritten, a follower on a stale doc, the hub's disk differing from its pointer when quiet |
| Driver | `auto` holds the dwell after a check passes and `key` waits for a key; `--fast` skips both; the first failed check stops the run and writes the failure to the terminal; the step log matches the story |
| Layout | The rectangles for both layouts from a screen description, including the main monitor's menu bar offset |
| Setup | The `rv sync`, `rv library`, and `uv sync` invocations; the advice printed when R, rv, uv, or `q2` is missing |
| Reset | The sequence of DM12 against fakes: what is stopped, rebuilt, kept, restarted, and renavigated |

**End-to-end tests (Playwright, own config), in `demo/tests/e2e/`, through the real launcher:**

| Id | Test | What must hold |
|---|---|---|
| E1 | Each preset's scripted story in `--fast` | Every step's check passes and the step log equals the story's steps, for `hub-browser`, `hub-vscode-host`, `hub-vscode-clone`, and `hubless` |
| E2 | Engines | After the switch the guest shows the R paper's re-executed chart and the Python paper's chart; the warm-up papers render |
| E3 | Free play | The test types `git switch feature` into the terminal page as a person would; it runs in the repository, the guest follows, and the observer's narration appears where `--narration` says |
| E4 | Fault injection | The test writes conflict markers into a doc through a raw `Repo`; the red warning appears. Each invariant gets one such case |
| E5 | Reset | After free-play commands, `demo reset` restores the repositories and a fresh hub; the windows stay open and follow the new project |
| E6 | Captions | One story run per value of `--captions`: the text appears inline, in the strip, in both, or nowhere |
| E7 | Layout | Each window reports the bounds the layout computed (browsers through CDP, VS Code through `getBounds()`) |
| E8 | Control | The `hub-browser` story against a hub started with `--git ignore` fails at the switch beat's check, so the checks discriminate |

## Checklist

**Phase 0: research**

- [ ] Run research items 1 to 7 and record each answer in the notes.

**Phase 1: tests first**

- [ ] Add the dependencies (xterm.js and its fit addon, node-pty, ws if needed) to `quarto-hub-vscode`; regenerate
  `package-lock.json` with `npm install` from the repository root.
- [ ] Write the unit tests of the table above and the end-to-end tests E1 to E8; see them fail (missing modules, then
  wrong behavior).

**Phase 2: the environment**

- [ ] The journal builder and content: the papers (prose, numbers, R and Python code with fixed seeds), the
  micrograph on both branches (generated once by an R script kept in `demo/journal/`, committed as PNG), the
  branches, `_quarto.yml`, the rv and uv project files and locks.
- [ ] Setup: `rv sync`, `uv sync`, the kernelspec, the checks and advice.
- [ ] The terminal server and page, captions and the strip.
- [ ] The layout and window placement.
- [ ] The launcher and preset `hub-browser`, free play only.
- [ ] The observer, the log, the invariant checker.
- [ ] `demo reset`.

**Phase 3: scripted playback**

- [ ] The driver: captions, typing, checks, pacing, `--fast`, stop on failure.
- [ ] The `hub-browser` story (DM7).
- [ ] Presets `hub-vscode-host` and `hub-vscode-clone` and their stories.
- [ ] Preset `hubless` and its story (DM6), adjusted by research item 7.

**Phase 4: finish**

- [ ] All unit and end-to-end tests pass; E1 three times in a row on every preset.
- [ ] A `demo/README.md`: requirements (R, rv, uv, VS Code download, the rebuild chain), how to launch each preset, the
  options, free play and `demo reset`, how an agent can follow `logs/events.jsonl`.
- [ ] End-to-end verification below, recorded in the notes.
- [ ] `npm run typecheck -w quarto-hub-vscode`, `cargo xtask lint`, `q2 render --strict` on this file.

## Gates

```bash
npm run typecheck -w quarto-hub-vscode
npm run test -w quarto-hub-vscode          # the existing unit tests and the demo's unit tests
npm run test:demo -w quarto-hub-vscode     # the demo's end-to-end tests (real windows)
cargo xtask lint
```

Before the first end-to-end run, the rebuild chain of `AGENTS.md`: `cd hub-client && npm run build:wasm`,
`cargo xtask build-q2-preview-spa`, `cargo xtask build-hub-client-embed`, `cargo build --bin q2`, and the extension
build. This worktree last built them for GE-13; the tip has moved since, so rebuild.

## End-to-end verification

Run every preset headed, once scripted with `--pace auto` and once in free play, on the left monitor. Take a
`screencapture` of the main display at the start, after the switch beat, and after the reset, and look at each image:
the three (or four) windows where the layout puts them, the captions in italics, the guest's swap visible. Record the
invocations, the event log excerpt for each beat, and the screenshots' paths in the notes, with an explicit note that
they were inspected.

## Exit criteria

- Every preset plays its story headed and in `--fast`, and free play works on each, with the invariant checks live.
- Every unit and end-to-end test passes; E1 passed three times in a row on every preset; E8 fails as it must.
- The research answers, the end-to-end record, and every finding about the product are in the notes.

## Out of scope

- CI wiring and `cargo xtask verify` (DM13); Windows and Linux (DM11).
- Recording, editing, or hosting the video, and arranging windows beyond the default layout: Gordon does these.
- Fixing bd-er8e6nmq; changing any product code. A product defect found here is recorded with its evidence; if it
  blocks a story beat, stop and bring it to Gordon, and the fix is a new plan.
- Changing the GE-13 and GE-14 specs, or the tests' fixture.

## Notes from the run

Not started.
