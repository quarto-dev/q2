# The git-epochs demo and playground

A scripted, repeatable demonstration of git epochs for a screen recording, and a playground you can drive by hand. It
opens real windows (browsers, VS Code) on one monitor, runs a real hub and real `q2 preview` processes, and watches
what happens. Plan: `claude-notes/plans/git-epochs/ge-19-demo.md`.

The content is a fictional journal issue, *Journal of Theoretical Mycodynamics* (vol. 17, issue 3). Myra Celia (the host)
owns the R paper; Maurice Morel (the guest) owns the Python paper. Both papers have live chunks, so a branch switch
re-executes real code.

## Requirements

macOS only (nothing promises other platforms).

- **R**, **rv**, and **uv**. The launcher checks for them and prints the exact install advice when one is missing:
  `brew install --cask r`, the rv install script from <https://a2-ai.github.io/rv-docs/>, and `brew install uv`. It runs
  `rv sync` and `uv sync` itself, and keeps `rv/library` and `.venv` across resets.
- **`q2` on `PATH`**, built from this checkout (`cargo build --bin q2`, then put `target/debug` on `PATH`).
- **Node 24** (`.nvmrc`) and `npm install` from the repository root.
- **VS Code**, downloaded by the existing VS Code end-to-end tests into `vscode-sync-experiment/.vscode-test`. Run
  `npm run test:e2e -w quarto-hub-vscode` once, or copy the cache from another worktree.
- The built extension and the `ts-packages` outputs it imports (`npm run build -w quarto-hub-vscode`).

### The rebuild chain

`q2 preview` embeds its client and WASM, so Rust changes do not reach the demo until you rebuild them:

```bash
cd hub-client && npm run build:wasm
cd .. && cargo xtask build-q2-preview-spa
cargo xtask build-hub-client-embed
cargo build --bin q2
```

## Launching

From the repository root:

```bash
npm run demo -w quarto-hub-vscode -- --preset <preset> [options]
```

| Preset | What it shows |
|---|---|
| `hub-browser` (default) | Myra edits in a browser on a tracking hub; Maurice follows in another browser. The full story: edit, commit, switch branch, reset, rebase with a conflict. |
| `hub-vscode-host` | Myra edits in an editor-only VS Code window on the hub's own folder; Maurice follows in a browser. |
| `hub-vscode-clone` | Myra uses a browser on the hub; Maurice has his own VS Code clone with its own terminal. |
| `hubless` | A standalone `q2 hub` with no git tracking: Myra shares from VS Code (the dialog is on camera), and Maurice joins with the copied command. |

Without `--play` the demo starts in **free play**: the windows open, the hub runs, and you type in the terminal window
as Myra (or Maurice, in the presets that have his terminal). With `--play` it runs the preset's **scripted story**.

### Options

| Option | Meaning |
|---|---|
| `--play` | Run the scripted story instead of free play. |
| `--pace auto\|key` | `auto` waits a dwell after each beat; `key` waits for a key press in the host terminal. Default `auto`. |
| `--dwell <ms>` | The dwell of `auto` (default 2500). |
| `--captions inline\|strip\|both\|off` | Where the story's narration captions go (default `inline`, in italics in the terminal). |
| `--narration inline\|strip\|both\|off` | Where the observer's one-line descriptions of what the hub did go (default `strip`). Warnings ignore `off`. |
| `--fast` | Headless, no pacing, no captions; runs every check. This is what the tests use, and it implies `--play`. |
| `--seed <n>` | Seed of the typing tempo (default 1). One seed gives the same rhythm every run. |
| `--screen <n>` | The monitor to place windows on (default 0, the main one). |
| `--root <dir>` | The demo root (default a fresh temporary directory). |

The typed commands and the editor text appear at a human tempo: a base of 40 ms per character, slower after spaces and
punctuation, with an occasional hesitation and a pause before the first character and before Enter.

## Free play and reset

Type any `git` command in the host terminal; the hub follows and the observer narrates what it did. Invariant checks run
continuously and show a red warning in the strip if one breaks: a conflict marker in a doc; the hub's disk differing
from the doc its pointer names once the doc has held still for 8 seconds (the hub writes to disk every 5 seconds); a
clone on another branch whose disk was rewritten.

To restore the starting state, type `reset` and Enter in the shell that runs `npm run demo`. The launcher stops the hub
(and the standalone hub in `hubless`), rebuilds the journal and the clone, starts a fresh hub with a new project id, and
points the open windows at it. The windows and terminals stay open.

## The event log, for agents

Everything the demo does and sees is appended to `<root>/logs/events.jsonl`, one JSON object per line:

```json
{"time":"2026-11-19T18:53:03.310Z","source":"observer","kind":"decision:moved-forward","summary":"The hub moved forward without swapping (...).","details":{}}
```

`source` is `story` (`pass` or `fail` per step), `terminal` (the commands typed), `observer` (what the hub did),
`invariant` (a violated check, with the first difference in `details`), or `demo` (reset and similar). A failed story
step stops the run and writes each terminal's last lines. An agent can follow the log with `tail -f` and judge a run by
the `pass` lines and the absence of `invariant` lines.

## Tests

```bash
npm run test -w quarto-hub-vscode        # unit tests, including demo/tests/unit
npm run test:demo -w quarto-hub-vscode   # end-to-end tests E1 to E8 (real windows; about 4 minutes)
```

`test:demo` is not part of CI or `cargo xtask verify`: it needs R, rv, uv, VS Code, and a display.
