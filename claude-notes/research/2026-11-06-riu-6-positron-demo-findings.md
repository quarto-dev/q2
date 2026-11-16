---
title: 'RIU-6: findings from Phase 0 (the q2 viewer in Positron''s preview pane)'
date: 2026-11-06
description: 'What was run and seen when q2 preview was opened in Positron''s preview pane with a throwaway spike command, what was not run, and the observations that bear on the RIU-6 commands.'
---

**Plan:** [RIU-6: the Positron command demo](../plans/2026-11-02-riu-06-positron-command-demo.md)
**Epic:** [the records-in-use epic](../plans/2026-11-02-records-in-use-epic.md)

Phase 0 asks one question: does q2's viewer, an iframe plus websockets plus a WASM module of roughly 36 MB,
work inside Positron's preview pane? Each step below is marked **seen** (Gordon watched it in the Positron
window), **measured** (run from the shell), or **not run**. Nothing here substitutes for the Positron window.

## What was built

A throwaway command `q2spike.previewPane` in the extension `extensions/q2-spike` of Gordon's Positron fork
(`gordonwoodhull/positron`, worktree `positron-spike-s3`, local branch `spike/riu-6-demo`, not pushed). It
spawns `q2 preview <active file> --no-browser` as a child process, reads the URL from the output, waits until
the port accepts connections, and opens the URL with `positron.window.previewUrl`. When the panel is disposed,
the command kills the child. It is a spike, not the Phase B command.

A second command, `q2spike.previewPanelStoppable`, does the same with `positron.window.createPreviewPanel`
instead: the panel's webview holds an iframe pointing at the URL, `stoppable` is `true`, and the stop button
kills the child and leaves the panel open. Both commands log `onDidDispose`, `onDidRequestStop` and view-state
changes to the output channel.

## Results

| Check | Result |
|----|----|
| The viewer renders in the pane | **Seen.** Page text and layout appear. |
| The WASM module loads under the pane's CSP | **Seen** in the console: `WASM module initialized successfully, template loaded`, then `Peer connected - online mode`. |
| An image present at startup | **Seen** on the prose fixture, after the fixture was corrected (see below). |
| Anchor link | **Seen.** The page scrolls a little; the fixture is short, so this is weak evidence about long pages. |
| External link | **Seen to fail.** Nothing happens. The console says `Blocked opening 'https://quarto.org/' in a new window because the request was made in a sandboxed frame whose 'allow-popups' permission is not set` (from `q2-preview-*.js:963`). See observation 7. |
| Live update of an edited line | **Seen.** The pane updated after saving. |
| Console errors from the preview webview | **Seen.** Apart from the blocked popup, nothing came from q2 or the SPA. The rest is Positron noise: an `allow-scripts` plus `allow-same-origin` sandbox warning, `Unrecognized feature: 'local-network-access'`, and unrelated extension errors (see observation 8). |
| Closing the `previewUrl` pane with the big X stops q2 | **Seen to fail.** The X hides the pane (`visible=false`) and does not fire `onDidDispose`; q2 kept running. See observation 6. |
| The Viewer's "clear" button on the `previewUrl` pane | **Not run.** Clear was tried only on the `createPreviewPanel` pane. |
| `createPreviewPanel` pane renders the document | **Seen.** |
| `createPreviewPanel`: the stop button | **Seen.** The button appears; clicking it fires `onDidRequestStop`, q2 exits with code 0, and the panel stays open with its toolbar gone. |
| `createPreviewPanel`: the Viewer's clear button before stop | **Seen.** It fires `onDidDispose` and the handler kills q2. |
| Quarto 1 preview in the same pane | **Not run.** Gordon declined, judging it close enough. |
| Click to source, scroll sync | **Seen absent.** Clicking in the preview does not move the editor, and the reverse; scrolling does not sync. Out of scope for this epic. Section links inside the page do scroll. |
| Storage and service worker in the webview origin, reopening the pane | **Not run.** |
| Remote workspace, `localhost` versus `127.0.0.1` | **Not run.** Only `127.0.0.1` was used. |

The SPA was rebuilt with the chain from `AGENTS.md` (`npm run build:wasm`, `cargo xtask build-q2-preview-spa`,
`cargo build --bin q2`; it exited 0 and `q2 preview --help` runs from the rebuilt `target/debug/q2`). After the
rebuild, the document renders in the pane and the external link is still blocked, both **seen**. The earlier
rows were observed before the rebuild; the embedded SPA then dated from Oct 16.

## Observations

**1. q2 prints the URL before it listens, and a client must wait.** `q2 preview` prints `→ http://...` before the
server binds (`crates/quarto/src/commands/preview.rs`, the comment calls it the print-before-accept contract).
q2's own browser open waits for the port (`spawn_browser_open_when_ready`), but a client that uses
`--no-browser` gets the URL early. The first spike version navigated at once and the pane stayed blank, with
`ERR_CONNECTION_REFUSED` in the Positron log. Waiting until the port accepts, or until the process exits,
fixed it; the wait measured 101 ms on one run. The race was not reproduced from the shell, so how long the
window can be is unknown. Quarto 1 prints its "Browse at" line after the initial render and before
`Deno.serve` (`external-sources/quarto-cli/src/command/preview/preview.ts`, lines 209, 282, 288), so it has the
same order but a gap that is not noticeable. **The RIU-6 commands must wait for the port.** The alternative is for q2
to print the URL only once it accepts connections, which removes the polling from every client; that is strand
`bd-qwwv9oh6`, with the design notes and the earlier decision that printing first was intended.

**2. The server does not wait for the project render.** On a file in a project of many documents, the port
accepted within 0.2 s of the URL and engine output kept appearing for at least 15 s afterward. An earlier
explanation in this session, that q2 rendered the whole project before serving, was wrong.

**3. A file created after startup is not indexed.** An image added while a preview runs stays broken in the
pane and in Chrome until q2 restarts; the hub watcher logs `File not in index, skipping`. This is q2 behavior,
not a pane problem, and it is outside every plan. Strand `bd-0lv550np`. The demo notes should tell the
presenter to add files before starting the preview.

**4. A plain `q2 preview` starts the engine.** The spike run on `s6-demo.qmd` showed `Q-PREVIEW-CAP-1`
("Engine capture failed ... Engine 'jupyter' is registered but its runtime is not available"), emitted by
`capture_driver.rs` when the eager driver's engine run fails. That is the existing path, not a regression. In
the epic, never executing is `preview.engine: off` or leaving `engine` out of `--records` (decision 5), and a
source ahead of `engine` that supplies a record means the engine is not reached (decision 4). The preview
default is `cache,q2,engine`, so a document with no record still ends at the engine. For command 3, start the
preview with `--records positron,...` and `preview.engine: off`. **Open question, not answered by the plans:**
when the engine cannot start at all (no Jupyter runtime in the extension host's environment), does a
`positron` source ahead of `engine` keep the warning from appearing? RIU-3 and RIU-4 have no test for it as far
as the plans show. This depends on RIU-2, RIU-3a and RIU-4, which are not on this branch, so it was not run.

**5. Results from Positron do not reach the pane yet.** The spike sends nothing to q2, so a document run in
Positron shows no results in the pane. That is command 3 of Phase B, which needs RIU-2, RIU-3a and RIU-5.
It is not a Phase 0 failure.

**6. Three ways to close a preview, three different outcomes.** After Gordon closed the `previewUrl` pane with the
big X in the corner, the spike's child process was still running (`ps` and `lsof`: the worktree's
`target/debug/q2 preview .../phase0/prose.qmd`, listening on 127.0.0.1:64020, with the extension host as its
parent). A second run with the logging added settled why: the X only hides the pane. The log showed
`viewState visible=false` and no `onDidDispose`. On the `createPreviewPanel` pane, **stop** killed q2, fired
`onDidRequestStop`, left the panel open, and removed the Viewer's toolbar (the API sets `stoppable` to
false after a stop). The Viewer's **clear** button, used before stop, fired `onDidDispose`, and the handler killed
q2. The type documentation says the same: clear fires `onDidDispose` and not `onDidRequestStop`. An open
question: a hidden pane keeps its server running and Gordon found no control that brings it back; the API has
`panel.reveal()`, but the spike never calls it, so whether a re-run could reuse the hidden pane was not tried.
For the commands: do not rely on `onDidDispose` alone to end `q2`; also stop it when the extension deactivates
and on stop, and decide what a hidden pane means. Leftover processes show with `pgrep -fl "q2 preview"`.

**7. External links are blocked by the sandbox.** The viewer opens a link in a new window, and the frame it runs
in has no `allow-popups`, so clicking an external link does nothing. Which frame lacks it (Positron's webview
iframe or the SPA's own) was not determined. A document with external links in the demo will show this. The
options are for the viewer to hand the URL to its parent with `postMessage` so the extension opens it, or a
Positron-side change; neither is in any plan, so decide before the demo script is written.

**8. Positron console noise that is not about q2.** The console also shows `property 'engines' is mandatory` for
four `positron-data-explorer-*` extensions, `Could not find a notebook editor for session ...`, `No pandoc
executable found in Positron`, and a long run of `Lookup notebook session for notebook URI ...: not found`. All
of these appear without the spike command and are not caused by it as far as can be told; none was investigated.

**9. Fixture mistakes, not product bugs.** The first prose fixture used a 1x1 image, which looked broken. A
fixture inside `~/docs/inline-output` is part of that directory's project, so the preview URL becomes
`?page=phase0/prose.qmd` and the project's other files are discovered; a fixture that should stand alone needs
its own `_quarto.yml`. An old fixture, `s3-spans.qmd`, fails to parse on an escaped backtick inside inline
code (line 57, column 77 of that file, an escaped `{python}` span); whether that syntax should parse was not
checked. The same construct also failed to parse when first written into this note.

**10. How Quarto 1's extension handles this (read from source, not run).** Read from `~/src/quarto` at
`216345ee` (2026-11-05), `apps/vscode/src`. Its preview server runs in a named integrated terminal, not a child
process; closing the terminal sends an HTTP terminate request. The panel comes from `createPreviewPanel` with
`retainContextWhenHidden: true`, and the extension keeps one panel per extension (`providers/webview.ts`). It
does not react to the pane being hidden. Running the preview command again on the same document reuses the
live server and calls `reveal()` on the existing panel (`canReuseRunningPreview`, `show`), which is how a
hidden pane comes back. `setStoppable(true)` is set when the server is up and `false` when the terminal closes;
on `onDidRequestStop` it sends the terminate request, hides the stop button, and **disposes the panel**
(`preview.ts`, lines 301 to 311), unlike the spike, which left the panel open. Its page is a wrapper webview
with its own toolbar (back, forward, reload, URL bar, zoom) around an iframe. Whether RIU-6 should reuse
the server per document the way Quarto 1 does is a design choice for riu-2-impl and Gordon; it is not decided
here.

**11. No editor and pane interaction.** In the pane, clicking in the preview does not move the editor, clicking
in the editor does not move the preview, and scrolling one side does not scroll the other. This was **seen**
on a long prose fixture (`phase0/prose.qmd`, seven sections of meteorology with four SVG diagrams). Links between
sections in the page do scroll. Gordon recorded this as out of scope for this epic.

## Not verified

Everything marked "not run" above (the Quarto 1 comparison, storage and the service worker, reopening the pane,
a remote workspace, `localhost` versus `127.0.0.1`, and the Viewer's clear button on the `previewUrl` pane), and
all of Phases A to E, which wait for RIU-2, RIU-3a and RIU-5. Observation 10 is a reading of source, not a run.
