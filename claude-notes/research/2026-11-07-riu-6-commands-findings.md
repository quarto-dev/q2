---
title: 'RIU-6: findings from Phases B and C (the three Positron commands)'
date: 2026-11-07
description: 'What each of the three Positron commands does, how each was checked (Gordon in the window, Playwright driving a real Positron window, or the command line), what broke along the way, what is not run, and a short script for running the demo by hand.'
---

**Plan:** [RIU-6: the Positron command demo](../plans/2026-11-02-riu-06-positron-command-demo.md)
**Earlier note:** [Phase 0 findings](2026-11-06-riu-6-positron-demo-findings.md)

Every check below is marked **seen** (Gordon watched it in the window), **Playwright** (an automated test
driving a real Positron window; nobody watched), **CLI** (run from the shell), or **not run**. A Playwright
pass is a real window run, but it is not the same as someone looking at the result, and the table says which
is which.

## What was built

All of it is in Gordon's Positron fork, worktree `/Users/gordon/src/positron-riu6-a`, branch
`spike/riu-6-phase-a` (local only, not pushed).

| Command | Id | Where | What it does |
|----|----|----|----|
| 1. Run Inline Expressions | `positronQuarto.runInlineExpressions` | workbench, `quartoRunInlineExpressions.ts` | Runs each `{lang} expr` span on the document's kernel (Interactive mode, no picker) and shows the value after the span. |
| 2. Render with q2 and push | `q2.renderAndPush` | extension `q2-spike`, `exportCommand.ts` | Runs `q2 record export-positron` on the saved document (with a timeout), then pushes the exported outputs into the open editor through the internal command `_positronQuarto.pushOutputsFromFile`, and shows a one-line summary. |
| 3. Preview from the Positron cache | `q2.previewFromCache` | extension `q2-spike`, `previewCommand.ts` | Runs `q2 preview <doc> --records <cache dir> --no-browser` as a child process and opens its page in the preview pane. The engine is not listed, so nothing executes. |

The push matches each exported cell to an open cell by content hash: it prefers the open cell whose index
equals the pushed cell's id, else the first cell with that hash. It can send two pushed cells to one open
cell when a document's duplicate cells changed; the demo accepts that.

## Results

| Check | How | Result |
|----|----|----|
| Command 3 shows cached outputs, no kernel starts | seen, 2026-11-06 | Works. A two-minute process watch saw only the q2 preview process appear. |
| Command 3: edit and Re-execute | seen | The overlay appears and Re-execute fails with 403 by design (the engine is not in `--records`). The edited cell also loses its syntax highlighting; a question for RIU-3a. |
| Command 3: Stop and the big X | seen | Stop ends q2. The Viewer's own Stop and Clear buttons leave the Viewer view open; the big X leaves q2 running. Fixing that is a change to Positron's Viewer, not taken. |
| Command 1 on a document with an undefined name | seen | No picker; the kernel's NameError shows for the span. |
| Command 1 on two spans | seen, and Playwright | 42 and 3.0 show after the spans; a second run replaces them. |
| Command 2, one-cell Python document | Playwright | The cell's output arrives in the open editor. Also run with a nonexistent q2 path: it fails as it should. |
| Command 2, Python plot, data frame, warning, message | Playwright | Four inline outputs; the image is visible; the data frame's columns and the warning and message text are present. |
| Command 2, Julia text, message, HTML, error, plot | Playwright | Five outputs; the plot is exactly one image (RIU-5 drops the other representations). |
| Command 3 after command 2, Python | Playwright | The preview pane shows the pushed output, read from the cache. |
| Command 3 after command 2, Julia | Playwright screenshot only | The pane showed the cell outputs, the vector and the plot. The automated test does not pass yet; see the Julia server problem below. |
| q2 export from the shell, Python and Julia | CLI | Exit 0, one JSON line, the stderr notice first; the Julia plot is one `image/png`. |

## Findings

**The cache write is debounced, and a fast preview found no cache.** Positron writes the output cache after a
short delay. Command 3, run straight after a push, failed with "record source ... does not exist". The push
command now flushes the cache before it returns (fork `c6319c1f38`). A person clicking by hand would rarely
hit it, but a script or a second command would.

**The cache directory is a guess that holds for one profile.** The extension takes the cache as the sibling of
its own `globalStorageUri` (`<User>/globalStorage/quarto-inline-outputs`). That is correct for the default
profile and for the e2e profile here. A non-default profile, or a change in where Positron writes the cache,
would break it, and the failure would be the "does not exist" message above. Shipping needs Positron to tell
the extension where its cache is.

**q2's Julia engine takes the project from the document's folder.** `JULIA_PROJECT` is not used. A document
whose folder has no project with Plots gets an `ArgumentError` for the plot cell, which with `error: true` is
exported as an error output and looks like a working run. The Julia test copies a `Project.toml` and
`Manifest.toml` next to the fixture.

**A leftover Julia server can break later runs.** q2's Julia engine leaves a server running after a render.
A server started from a directory that later disappears (the e2e workspace is deleted and recreated for each
run) makes the next export fail with `ArgumentError: startpath must be non-empty`, and nothing in the message
points at the stale server. The shared server directory is `~/Library/Caches/quarto/julia`, used by every
checkout on the machine, so one lane's test runs can break another's. This belongs to the Julia engine, not to
these commands. It is the reason the Julia preview test does not pass yet.

**The dev build has to be assembled with care.** Symlinking the supervisor extension's compiled `out/` from
another tree made the Positron Kernel Supervisor fail to activate ("MCP configuration providers must be
registered ... 'positron-sessions'"), which took Python, R and Connections down with it; a real compile of that
one extension fixed it. The kernel server binary (`resources/kallichore/kcserver`) also has to be present or
every kernel start fails with "Unknown error". A window that runs the wrong tree is easy to mistake for a bug:
`lsof -p <pid> -d cwd` shows which tree it runs.

**The Playwright harness works, with these needs.** Node 24.22.1 (the pre-commit hook runs Playwright's
config under whatever `node` is on `PATH`, and fails on another version), `POSITRON_E2E_ASSISTANT_PATH` set to
the extension to load, the four `POSITRON_*_VER_SEL` variables, a venv with ipykernel on `PATH` so q2 can
run Python, and the Plots project for Julia. Every run ends with a "worker teardown timeout"; the tests
themselves pass. In the preview pane the text of a cell appears twice (the code and its output), so a
text locator must match exactly. The preview page sits two iframes deep inside the Viewer's webview.

**One real limit of "q2 runs it, Positron shows it".** Command 2 runs the document in q2's own kernel, not
Positron's. The pushed outputs sit in the document and the cache, but Positron's kernel state is unchanged:
a following cell that uses a variable from a pushed cell would fail. The demo shows outputs, not state.

## What each command would need to ship

- **Command 1:** the span detector and the executor already exist and are tested; shipping needs Julia and R
  spans checked (only Python was run), and a decision on whether the values should persist.
- **Command 2:** Positron's own cache location from the API (see above); the document-is-dirty case decided
  (today a dirty document is exported as saved, with a note); a progress and cancel affordance, since the
  engine may run the whole document; a decision on whether pushing over existing outputs asks first.
- **Command 3:** the Viewer's close button ending q2; the banner text in q2's own preview page ("this page
  reconnects automatically") corrected or suppressed for a one-shot server; a way to pick a profile's cache.

## Not run

- Command 2 and command 3 watched by a person, other than command 3 on Python and the earlier checks marked
  seen.
- The Julia preview test passing (a stale Julia server, pid 86369, blocks it; see the plan).
- Positron producing a Julia cache itself, and hashing a Julia cell that has `#|` lines (no Julia kernel in the
  dev build).
- A wrong `q2.path`, an untitled or unsaved document, "q2 too old", and a non-default profile, for any command.
- R: out of this demo (epic decision 11).
- Command 1 on Julia and R spans.

## Run the demo by hand

1. Build q2 from the integration branch with the full chain, in a checkout that has the commits:
   `(cd hub-client && npm run build:wasm) && cargo xtask build-q2-preview-spa && cargo build --bin q2`.
2. Start Positron from the fork tree, not another tree:
   `cd /Users/gordon/src/positron-riu6-a && ./scripts/code.sh --extensionDevelopmentPath=$PWD/extensions/q2-spike`.
3. In settings, set `q2.path` to the `target/debug/q2` from step 1.
4. Open `test/e2e/test-files/workspaces/quarto_inline_output/inline_exprs.qmd`. Run **Quarto: Run Inline
   Expressions**. Expect `42` and `3.0` after the two spans and no picker; run it again and the values replace.
5. Open `render_and_push_python.qmd` in the same folder, with a Python that has ipykernel, matplotlib and
   pandas on `PATH`. Run **Render with q2 and push outputs**. Expect a notification saying q2 exported 4
   cells, and a plot, a data frame, a warning and a message under their cells.
6. Run **Preview with q2 from the Positron cache**. Expect the same outputs in the preview pane, and no new
   Python process. Press the Viewer's Stop to end q2 (the big X leaves it running).
7. For Julia, put a Julia project with Plots next to `render_and_push_julia.qmd` and repeat steps 5 and 6.
   Before a Julia run, make sure no Julia server from an earlier run is left (see the finding above).
