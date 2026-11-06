---
title: 'RIU-6: the Positron command demo'
date: 2026-11-02
description: 'Three commands in Gordon fork of Positron that exercise both directions between Positron and q2 records: run inline expressions, render with q2 and push the outputs into the open document, and preview from the Positron cache with no execution. Mechanisms proven in RIU-0 S5 and S6; Python and Julia are in scope. Starts with an early spike that shows the default q2 preview viewer in the Positron preview pane, which can run before any other RIU plan. A spike in the fork, ending with a script, a capture, a findings note and the reconciliation of the epic checklists.'
status: ready  # nothing started; written from RIU-0 S5 to S7 and RIU-0b spike J; the Phase 0 spike needs nothing; the commands need RIU-3a, RIU-2 and RIU-5 (command 3 does not need RIU-4's hardening); revised after the fourth review (default viewer in the preview pane)
---

**Part of:** [the records-in-use epic](2026-11-02-records-in-use-epic.md)
**Evidence:** [RIU-0 findings](../research/2026-11-03-riu-0-spikes-findings.md), S3 (run spans),
S5 (push into an open document), S6 (running q2), S7 (what Positron shows);
[RIU-0b findings](../research/2026-11-03-riu-0b-findings.md), J1 to J8 (Julia).
**Depends on:** RIU-3a (the Positron source and the minimal `--records` flag) and RIU-2 (the execution guard)
for command 3, RIU-5 (the export and its Julia and representation rules) for command 2, and the RIU-1
preview flip. RIU-4 hardens the flag and is not needed by any command. The flip matters because the demo shows imported output through a preview
whose hub payload is a record document, which needs the writer gate on (the flip, or
`QUARTO_ENGINE_RECORD_WRITE=1` until it ships).
**Blocks:** nothing; it is the last plan.
**Estimated sessions:** 2, plus the Phase 0 spike (under one session).
**Origin:** the request that began this epic, which asked for exactly these three commands before
records were integrated. This plan comes last on purpose.

> **For implementers:** this is spike or demo quality, in the fork only. It is not how the
> commands would ship. Nothing is sent to anyone; nothing is pushed without Gordon asking.
> Work in the existing worktree `/Users/gordon/src/positron-spike-s3` on `spike/q2-s3-inline-exec`
> (or a new worktree cut from it, or the `spike/q2-s8-julia` branch there, which holds no new commit),
> never in `/Users/gordon/src/positron`. That branch already holds the S3 span command, the S5 push
> command (`6b0649ff1c`) and the S6 `q2-spike` extension (`afd8c75204`); start from them. Follow the
> Positron conventions: new files over edits, `// --- Start Positron ---` markers on edits to upstream
> files, vitest via `npm run test:positron`, no direct `tsc`, TypeScript only (the pre-commit hook
> rejects new `.js` files), a copyright header, and no non-ASCII characters in source. The dev build
> is `scripts/code.sh`.

**Lanes:** before starting a phase or landing work, read the epic's [lanes, landing and the ready board](2026-11-02-records-in-use-epic.md#lanes-landing-and-the-ready-board): lane branches, rebase and landing rules, and the board of which phases may start.

## Overview

```text
Positron document ----(1) run spans------------------> inline values (S3)
q2 record  --export (RIU-5)--> exported file --push (S5)--> inline outputs, persisted   (2)
Positron cache --q2 preview --records positron (RIU-3a, RIU-2)--> page, no execution (3)
```

## The mechanisms (all proven)

- **Pushing into an open document (S5):** `QuartoOutputContribution.spikePushCells(cells)`
  matches each cell by content hash (`findCellByContentHash`), clears its outputs, adds the new
  ones to the view zone and calls `saveOutput`, so they persist. A helper on the cache service
  (`spikeParseIpynb`) turns a cache-format ipynb into cells. Seen: outputs appear live, persist
  across close and reopen, survive a dirty document, and a cell whose code changed is skipped. No
  `reloadCachedOutputs` and no change to `_cachedOutputsLoaded` was needed. Spike J saw the same for
  a Julia document (Gordon, J7).
- **Running q2 (S6):** an extension runs `child_process.execFile` in the extension host and calls an
  internal workbench command (`_positronQuarto...`, via `vscode.commands.executeCommand`) with
  the result. A failed lookup comes back as `ENOENT`, distinct from a numeric exit code.
- **Finding q2 (S6):** `q2` is **not** found by name when Positron starts without the shell PATH.
  An absolute path in a setting (`q2spike.q2Path` in the spike) works and is read without a
  restart. The `q2` on PATH can be an old nightly, so the runner treats a usage error (exit code 2,
  RIU-5's table) from `q2 record export-positron` as "q2 too old".
- **Cache location (S5):** `.../Positron/User/globalStorage/quarto-inline-outputs/` for the release
  build, `.../code-oss-dev/User/globalStorage/quarto-inline-outputs/` for the dev build; a file is
  `sha1(document URI string)[0:16]` plus `.ipynb`. The extension never computes this: command 2 pushes
  through the cache service, and command 3 lets q2 find the file (RIU-3 Phase C).

## The three commands

1. **Run inline expressions.** The S3 command (`quartoInlineExprCommands.ts`), with the mode
   picker removed: always `Interactive`, the only mode that returns correct values without
   console or history side effects. Known side effect, to state in the demo: each span counts as
   a kernel execution.
2. **Render with q2, then push the outputs into the open document.** Runs
   `q2 record export-positron <doc> --out <temp file> --source-uri <document.uri.toString()>`
   (RIU-5) through the extension, reads the JSON result, and hands the file to the push method above.
   The command never writes into the live cache directory by hand; `saveOutput` does the persisting.
   Skipped cells (inline spans, non-Jupyter-non-Julia cells, a cell whose code changed) and dropped
   representations are reported in the notification, not hidden.
3. **Preview from the Positron cache, in Positron's preview pane.** Runs
   `q2 preview --records positron --no-browser` on the current document in a terminal (RIU-3a, RIU-2),
   reads the URL the preview prints (`  → http://127.0.0.1:<port>/...`, `commands/preview.rs:304`) and opens
   it with `positron.window.previewUrl`, so it appears in the preview pane as Positron's Quarto 1 preview
   does. The engine is omitted from the list, so nothing executes (RIU-2's guard refuses every re-execute
   path, the POST and the automatic one). It uses the **default viewer**; the editor UI and `--share` are
   product behavior checked in RIU-3 and RIU-4, not part of the demo. The locator is told which Positron
   cache directory to read, the running build's (a dev build writes under `code-oss-dev`, a release under
   `Positron`, and the locator tries the release path first), through its directory override. A stale page
   shows the viewer's Re-execute overlay and, with `engine` absent, a click is refused with the guard's
   message; the demo says so and does not hide it. A terminal, not `execFile`, because the
   preview is a long-running process that must be stoppable (`execFile` in S6 returns when the process
   exits, which suits the export in command 2 and not this). How Positron's Quarto extension does it
   (read 2026-11-05 from the bundled extension 1.139.0, minified): it runs `quarto preview <file>
   --no-browser --no-watch-inputs` in a terminal, matches `http://localhost|127.0.0.1:<port>` in the
   output, and opens it in a preview panel with a stop button; `positron-run-app` calls
   `positron.window.previewUrl(uri)` (`api.ts:1143`). q2 has no `--no-watch-inputs`, so the extension's own
   launch code cannot be reused as is; the command builds its own.

### What the commands do outside the happy path

S5 exercised one saved document in one editor with a Python kernel. The behavior outside that:

- **An untitled or unsaved document:** commands 2 and 3 **refuse with a message** ("Save the document
  first"). q2 reads a file on disk, and an untitled document has no file URI for the Positron cache
  name; Positron's own content-hash fallback exists for untitled documents but q2 does not use it.
  Command 1 does not need a file and is unchanged.
- **A dirty document** (saved, then edited): command 2 exports from the bytes **on disk**. A cell the
  edit changed no longer matches by content hash, and the push **skips it with a report** (S5 saw a
  changed cell skipped); this is not an error. Command 3 reads the Positron cache, which Positron wrote
  from the open model, and q2 matches it to the file on disk, so the same cells are skipped. S5 saw
  the push work on a dirty document; whether the cache and the disk disagree in other ways was not
  tested.
- **A remote or WSL workspace:** the document URI scheme is not `file`. Not tested; the commands say
  "unsupported" for a non-`file` scheme.
- **Not exercised, and listed as known limits:** split views of one document (two editors, one model),
  notebooks other than `.qmd`, and more than one window.
- **Languages:** Python and Julia (below). R through Ark and R through knitr are out.

## Decisions that shape this plan

From the epic's "Decisions recorded 2026-11-05", revised after the fourth review
([review 4](../research/2026-11-05-riu-plans-review-4.md)).

- [ ] **The demo route is the default viewer opened in Positron's preview pane**, like Positron's Quarto 1
      preview. `--ui editor` and live share are product behavior, tested in RIU-3 and RIU-4, and are not
      part of the demo. `provide-hub` to quarto-hub.com is not part of the demo either: the hosted hub
      predates the reader (epic "Direction: flipping `provide-hub`").
- [ ] **The acceptance cases are RIU-3's Python and Julia cases** (plot, data frame, warning, message plus
      printed output, error, labeled cell with figure options). K-S2 is the Ark R document and stays
      KNR-5's.

## Phase 0: the early spike (viewer inside the preview pane)

Everything in command 3 rests on one unproven thing: whether q2's viewer SPA works inside Positron's
preview pane. The SPA renders the document in an iframe and uses websockets for live updates; whether the
pane's webview allows that (frame rules, CSP, asset and websocket requests) is not known. This needs only
today's `q2 preview` and the dev build, no records and none of RIU-1 to RIU-5, so run it first, with Gordon
at the window.

- [x] **Setup.** (Done 2026-11-06: prose-only fixture used; the rebuild chain ran, exited 0, and `q2 preview --help` runs from
      the rebuilt `target/debug/q2`; the earlier rows in the findings note were seen on the older SPA, dated Oct 16.) Use a **prose-only** document, or a Python document with a Jupyter venv on PATH (the default
      list is `cache,q2,engine`, so a document with code cells runs its engine). Confirm that
      `target/debug/q2` embeds a current SPA: if the SPA or WASM changed, run the chain from `AGENTS.md`
      (`npm run build:wasm`, `cargo xtask build-q2-preview-spa`, `cargo build --bin q2`), and check
      `q2 preview --help` runs from the build you will use.
- [x] (Done 2026-11-06, with two differences: the command spawns `q2` as a child process, not in a terminal, and waits for
      the port to accept before opening the pane, since q2 prints the URL before it listens; `createPreviewPanel` was
      tried later in a second command, `q2spike.previewPanelStoppable`. Local commit `553b90b2ef` on `spike/riu-6-demo` in `positron-spike-s3`, not pushed.)
      A throwaway command in the spike worktree: start `target/debug/q2 preview <fixture> --no-browser` in a
      terminal (absolute path from a setting, as S6 found), read the printed URL, call
      `positron.window.previewUrl`. Try `createPreviewPanel` too if `previewUrl` shows a problem, since the
      Quarto extension uses it and it adds the stop button.
- [x] (Done 2026-11-06 as seen or not run; see the findings note for each item. Gordon chose to stop here. Seen: page loads,
      document renders (also in a `createPreviewPanel` pane), WASM initializes, live update, anchor link, console log, the
      stop button, and the close behaviors. Found: external links are blocked by the sandbox; the big X only hides the pane
      and q2 keeps running; no click-to-source or scroll sync (out of scope for this epic). Not run: Quarto 1 comparison
      (declined), storage and service worker, remote workspace and `localhost` versus `127.0.0.1`, reopening the pane.)
      Check and record, each as seen or not run: the page loads; the document renders in the SPA's iframe;
      live updates when the file is edited (the websocket); assets load, including the roughly 36 MB WASM
      (does it compile under the pane's content-security policy, `wasm-unsafe-eval`?); links and anchors;
      click-to-source and scroll sync if present (the postMessage path between the SPA and its iframe); the
      iframe sandbox attributes; whether the SPA's storage (local storage, service worker) works in a webview
      origin; the printed URL's host (`localhost` versus `127.0.0.1`, and whether it works in a remote
      workspace); the stop button or closing the pane ends the `q2` process; console errors from the preview
      webview (frame, CSP or mixed-content). Compare with Quarto 1's preview of the same file in the same
      pane.
- [x] (Findings note written 2026-11-06: `claude-notes/research/2026-11-06-riu-6-positron-demo-findings.md`; it has
      results, observations 1 to 11 and the not-run list.) Write the result into the findings note. If it fails, name the failing mechanism and the options
      (open the URL in the external browser, or a q2 change); do not fix q2 inside the spike.

## Phase A: tests first

- [ ] vitest tests for the pure TypeScript pieces: building the `q2` argument array (quoting paths
      with spaces, no shell), parsing the JSON result line, classifying a failure (`ENOENT`, each
      exit code in RIU-5's table, 2 meaning "too old"), the refusal for an untitled document, and
      matching exported cells to the open model (by content hash with the occurrence rule).
- [ ] A scripted check, runnable without a window, that the exported file satisfies the
      `positron_loader` mirror (RIU-5 owns the test; this plan only calls it).
- [ ] The rest is manual with Gordon at the window; write the script first so the run is
      repeatable (Phase D).

## Phase B: the commands

- [ ] Move the S3 command into a new file, remove the mode picker (always `Interactive`).
- [ ] The `q2` runner as a proper extension (the S6 `q2-spike` is the starting point): a `q2.path`
      setting read on each run, then `PATH`, with an error naming the setting when the lookup
      returns `ENOENT`; arguments as an array; a timeout; stderr tail on failure; no shell.
- [ ] Move the S5 push method out of the `spike` names and wrap edits to upstream files in Positron
      markers; keep the command surface minimal (one internal command taking a file path).
- [ ] Command 2 and command 3 as above (command 3 per Phase 0's result), with the refusals. Report q2 failures in a notification
      with the stderr tail; never swallow them.

## Phase C: languages

- [ ] **Python first** (S5 and S7 proved it).
- [ ] **Julia**, in scope (epic decision 12), with these specifics from spike J:
      - Command 2 needs RIU-5's keep rule (owners `jupyter` and `julia`) and its representation rule,
        or a Julia plot shows twice with base64 text (J8). Test it on a text and vector cell, an HTML
        value, an error cell and a plot cell.
      - Command 3 on a Julia document works from a cache that **command 2 wrote** (the push persists
        it through `saveOutput`), so it needs no Julia kernel in Positron, and RIU-3's gate admits the
        `julia` owner.
      - **Untested and named as such:** Positron producing a Julia cache itself by running a cell (no
        Julia kernel was run in Positron), and Positron hashing a Julia cell with `#|` option lines (the
        spike documents had none). If Gordon has a Julia kernel in the dev build, run one cell and see
        that command 3 shows it; otherwise say "not run".
      - q2 needs `julia` on PATH (1.12.7 here) and, for a plot, Plots in the document's project.
- [ ] R is out of this demo (epic decision 11: R through Ark is not in this epic, and importing an Ark R
      cache rides on KNR-5's gate widening; R through knitr waits on KNR-3 and KNR-5). Say so in the findings.

## Phase D: demo and findings

- [ ] A short script (what to open, what to click, what to expect) and a screen capture or
      screenshots. A step that no one watched is marked not run.
- [ ] A findings note, `claude-notes/research/2026-MM-DD-riu-6-positron-demo-findings.md`: what
      worked, what did not, what each command would need to ship. Render it, and every plan file Phase E
      edits, with `target/debug/q2 render --strict <files>` before any push.
- [ ] Push the spike branch to Gordon fork only when he asks.

## Phase E: reconcile the epic

This is the step the epic's last exit criterion names, and this plan owns it because it is the last
plan to finish. Whoever finishes RIU-6 does it (`CLAUDE.md`: re-read, verify each box against what
landed, correct, commit).

- [ ] Re-read the epic and RIU-0b to RIU-5; check each `- [ ]` and `- [x]` against the tree and the
      findings, not against memory; fix the wrong ones, update each plan's `status:` line, and commit
      the plan files.
- [ ] Update the epic's table (status, sessions actually spent) and the "Decisions for review" list
      with what Gordon decided since.

## Exit criteria

- The Phase 0 spike result is recorded: the viewer in Positron's preview pane works, or the findings say what
  does not.
- The three commands work on a Python document and a Julia document, or the findings note says
  exactly what blocked each; the untested Julia items above are named.
- The behavior for an untitled, a dirty and a non-`file` document is as listed, and the known limits
  are in the findings note.
- A script and a capture exist.
- Plan checklists across the epic are reconciled with what landed (Phase E).

## Risks

- **Needs Gordon at the window.** The dev build is slow; reuse the S3 worktree build.
- **Command 2 depends on an upstream change** to Positron (the S5 push method) that is not a
  public API; state this in the demo. It is a spike in the fork, not a proposal.
- **The `q2` a user has may be too old** to read records or lack the subcommand; the runner says
  so instead of failing obscurely.
- **Race between injection and the user typing** in the open document; matching by content hash
  makes a mismatch skip the cell, which the demo should show rather than hide (S5: a changed cell
  was skipped).
- **The TypeScript is not checked on Windows.** Path quoting in the runner is tested with vitest
  on one platform only.
