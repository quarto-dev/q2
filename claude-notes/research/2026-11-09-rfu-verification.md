---
title: 'RFU-4 verification results'
date: 2026-11-09
description: 'Results of the RFU-4 checks: the decision 12 fail-soft re-check, the unhighlighted edited cell in q2 preview, the stale Julia server and missing Plots project reproductions, the RIU-3 browser items, and the hub verification.'
---

Binary: `target/debug/q2` built from `rfu/4-verification-and-small-fixes-2` (claim commit `b1e02ea16`). For the browser checks I rebuilt
the whole embed chain first (`npm run build:all` in `hub-client`, `cargo xtask build-q2-preview-spa`, `cargo xtask build-hub-client-embed`,
`cargo build --bin q2`), so the preview and editor bundles are not stale. Python and Jupyter are not installed here, so the checks use
`{r}` cells (R is installed) and Julia 1.12.7.

| Item | Result | Ran, read or not run |
|---|---|---|
| Hub verification | `q2 provide-hub` one-shot with `QUARTO_ENGINE_RECORD_WRITE=1` against a local no-auth hub: exit 0, "Done — collaborators will see the executed output."; a browser client opened the project and its preview frame shows both cell outputs. See "Hub verification". | ran (Playwright for the browser half) |
| Decision 12 (failed `record: true` copy fails soft) | Confirmed. | ran |
| Edited cell loses highlighting in `q2 preview` | Bug reproduced (re-checked on the fresh bundle in the first attempt). | ran (Playwright) |
| Stale Julia server (`startpath must be non-empty`) | Reproduced in an isolated `HOME`. | ran |
| Missing Plots project | Reproduced: exit 1 without `error: true`, exit 0 with it. | ran |
| `SYSTEM: caught exception ... Task notice` stream | Not reproduced; it came from a Manifest resolved by another Julia version and is not a q2 finding (phase A note). | not run |
| RIU-3: records-only preview shows output from a directory record | Output shown, no Run control. | ran (Playwright) |
| RIU-3: edited cell shows the Re-execute overlay | "Code has changed since the last capture." plus a Re-execute button appear. | ran (Playwright) |
| RIU-3: `positron,engine` runs the engine | With `preview.engine: auto`, an edit re-executes and the new output appears. With the default `manual` and no record, no output and no overlay. | ran (Playwright) |
| RIU-3: cache older than the document | A directory record older than an unchanged document is not flagged. The rule is for a Positron cache (unclaimed count), which I did not have. | ran (directory record only); Positron cache not run |
| RIU-3: "Clear results" clicked | Confirm step appears, outputs disappear, the status item goes. Other collaborators and restart not checked. | ran (Playwright, one browser) |
| RIU-3: `--share --allow-edit` with a guest, guest key outside the host list | Not run: it needs a second client over the iroh tunnel. | not run |
| RIU-3: real click or scroll sync | Not run; Monaco and preview scroll sync need a person watching. | not run |

## Decision 12

Invocation, in a scratch project whose `.quarto/execution-records` is a regular file, with `execute: {record: true}` and one `{r}` cell:

```bash
q2 render doc.qmd
```

Observed (output inspected): exit code 0; `Warning: Execution record not written` with the text "the execution record copy requested by
`record: true` could not be written: ... creating .../.quarto/execution-records: I/O error: File exists (os error 17)" and the line
"The render is unaffected. Check that `.quarto/execution-records` is a directory you can write to."; `doc.html` was produced and contains the
document text. Decision 12 holds.

## Julia: stale server and missing Plots project

### Stale Julia server

q2 shares one detached Julia server per `HOME` (`~/Library/Caches/quarto/julia/julia_transport.txt` names its pid and port). The server's
working directory is the directory of the render that started it. I reproduced the failure without touching the shared server, using a
scratch `HOME` (with `JULIA_DEPOT_PATH` and `JULIAUP_DEPOT_PATH` pointed at the real depot) and a copy of the Julia project in
`QUARTO_JULIA_PROJECT`:

```bash
cd stA && q2 render doc.qmd   # first run: "engine 'julia' timed out during execute" (cold start); the server stays up, cwd = stA
rm -rf stA
cd stB && q2 render doc.qmd   # exit 1
```

Observed (inspected): the server's cwd (via `lsof`) was `stA`; after deleting it, the second render fails with
``Execution failed in julia: Julia server returned error after receiving "run" command: ... ArgumentError: `startpath` must be non-empty``
plus a Julia stack trace. Nothing in the message names the server, its pid or the deleted directory. I killed my own isolated server
(pid 1023); the shared servers on this machine (other lanes) were left alone.

What q2 can say: the failure comes back from the engine as one string and is mapped in `crates/quarto-core/src/engine/ts_process.rs`
(`ExecutionError::execution_failed(&engine_name, message)`, near line 1249). A message that contains `startpath` must be non-empty is
a stale-server signature. There, q2 could append a hint ("The Julia server (pid N, started in a directory that no longer exists) is stale;
stop it with `kill N` or remove `julia_transport.txt` and render again") using the pid in the transport file. Nothing implemented; it
needs Gordon's say-so (it would be a judgment call to key on a Julia error string).

### Missing Plots project

A document with `engine: julia` and `using Plots` in a project that does not have Plots:

```bash
q2 render doc.qmd   # no error: true
```

Observed (inspected): exit 1 after about 54 s (the Julia startup); the error is
``ArgumentError: Package Plots not found in current path. - Run `import Pkg; Pkg.add("Plots")` to install the Plots package.`` followed by
about 45 stack frames of QuartoNotebookRunner internals. The text is accurate but buried in the trace and gives the wrong remedy for q2
(q2 takes the Julia project from the document's folder, not from `JULIA_PROJECT`, so the user must add Plots to *that* project).

With `execute: {error: true}` the same document exits 0 with no warning, and `doc.html` contains the `ArgumentError` as an error output of the
plot cell. That is the "looks like a working run" case: it is the documented effect of `error: true`, but a missing package is an
environment problem, not an error the author's code raised.

What q2 can say: in the same mapping site as above, recognize `Package X not found in current path` and print one line naming the
project directory q2 passed to Julia, and drop the stack trace. For the `error: true` case q2 would need to detect an `ArgumentError`
whose text matches in a returned error output, and emit a warning; that is a content check on outputs, so I would not do it
without a decision. Nothing implemented.

## RIU-3 browser checks

All with `q2 preview --no-browser` and Playwright (Chromium) against a scratch project with two `{r}` cells and a record copied to an
in-project directory `recs/`. Note the directory source must be written `./recs`; a bare `recs` is rejected as an unknown source.

- **Records-only, unchanged document.** `-M 'execute:{record-sources: ./recs}'`: both cell outputs (`OUT_ONE_3`, `OUT_TWO`) appear in the
  preview frame; the only button is "Refresh preview" (no Run control). Screenshot not kept.
- **Edit a cell.** After changing the second cell on disk the frame loses that output and the page shows "Code has changed since the last
  capture." with a "Re-execute" button, even with no `engine` source. I did not click Re-execute here (the plan says the guard refuses it; that I did not see).
- **`positron,engine`.** With `-M 'execute:{record-sources: "positron,engine"}'` and no Positron cache, the preview used the engine.
  Default `preview.engine: manual`: at first load the first run showed output from a previous run's cache; a fresh project would show inert
  source with no overlay (I saw this once, before the cache existed). With `-M 'preview:{engine: auto}'` added, an edit re-executed and the
  new output `OUT_TWO_EDITED` appeared within the wait.
- **Cache older than the document.** My first attempt flagged a stale page, but that run's record had been overwritten by an earlier engine
  run (it held `OUT_TWO_EDITED`), so the flag came from a real edit. After re-rendering a pristine record and setting its mtime to 2020
  with an unchanged document, the page was **not** flagged. The plan's "older than the document at boot" rule goes through the unclaimed
  count of a Positron import, so a directory record is not covered; I did not have a Positron-format cache to test the rule itself.
- **"Clear results".** In `--ui editor --allow-edit` (the editor takes the source list from the document key, not `-M`:
  `execute: {record-sources: ./recs}`), the status bar reads "Showing executed output" and "Clear results…". Clicking shows "Clear executed
  output? This removes it f..."; confirming removes the outputs from the preview and the status item. Not checked: a second collaborator, and
  that the record does not return until a restart. The `recs/doc.ipynb` file was still on disk afterward (the clear acts on the
  published record, not the directory).
- **Not run:** `--share --allow-edit` with a guest (and the Part B guest-key warning), real click or scroll sync.

## Hub verification

No earlier record of a hub run exists in the RIU plans (RIU-0\'s B12 scenario names it; RIU-6 says provide-hub to the hosted hub is not part of the demo). A first session could not run it, because the consent gate needs a terminal and the unattended flag was refused. Gordon then approved `--dangerously-accept-requests` for this test, and it ran (the flag was used only against a local hub on 127.0.0.1 and a scratch project, with a fixed one-shot document).

Invocations (all from the repo root; scratch project holds `_quarto.yml` and a `doc.qmd` with two `{r}` cells, `cat("HUB_OUT_MARKER\n")` and `1+1`):

```bash
target/debug/hub --project <scratch-project> --port 3037     # local no-auth hub; prints the index doc id
QUARTO_ENGINE_RECORD_WRITE=1 q2 provide-hub <index-doc-id> --file doc.qmd \
  --server ws://127.0.0.1:3037 --token dev --dangerously-accept-requests
(cd hub-client && npm run preview -- --port 5190)           # the built hub client
```

Observed (inspected):

- `provide-hub`: printed the unattended-execution warning, connected, listed `_quarto.yml` and `doc.qmd`, ran R (`processing file: doc.rmarkdown`), printed "Executed. Pushing the result to the hub…" and "Done — collaborators will see the executed output."; exit code 0.
- The result is not written to the project directory (the hub keeps it in its automerge document); no record file appeared on disk.
- Browser: Playwright (Chromium) opened `http://localhost:5190/#/share/<id>?file=doc.qmd&name=hubcheck&server=ws://127.0.0.1:3037` (the share link needs `name=`, or the page says the link is incomplete), clicked "Open project", and waited. The editor status bar read "Showing executed output" with "Clear results…". The `q2-preview.html` frame held two `cell-output cell-output-stdout` blocks, with text `HUB_OUT_MARKER` and `[1] 2`, and the code cells were highlighted (`sourceCode r`).
- Not seen by a person: I read the DOM and one screenshot of the invitation page, not the final editor view.
- A browser `pageerror` appeared twice while the editor loaded: "qmd: with groups, all characters should be matched in consecutive groups in rule: (unknown)". It did not stop the preview from showing the output; I did not chase it (see "Smaller observations").
- Not checked: `--watch` mode with a collaborator clicking Run, and a hub that requires auth.

## Findings for Gordon

### Edited cell is not highlighted in the records-only preview (RIU-3a)

Reproduction: render a project once with `execute: {record: true}` and an `{r}` cell (this fills `.quarto/execution-records/doc.ipynb`), edit
the cell source, then:

```bash
q2 preview --no-browser --port 18741 -M 'execute:{record-sources: .quarto/execution-records}' doc.qmd
```

(The directory entry needs a leading `./` in current builds.) A Playwright page load of `?page=doc.qmd` shows, inside the `q2-preview.html`
frame, the edited cell as `<pre class="{r} code-with-copy"><code>x &lt;- 1+2 ...` with no highlight spans. `q2 render -M execute:eval:false`
of the same cell gives `<pre class="sourceCode r">` with highlighting. So the unmatched cell reaches the writer still carrying the raw `{r}`
attribute class instead of the `sourceCode r` class. Not fixed here, as the plan says. In this session's fresh bundle the edited cell's output
was also dropped, as expected, and the Re-execute overlay appeared.

### Smaller observations

- Hub client page errors during the hub check: "WASM module not initialized. Call initWasm() first." (on the invitation page, before opening the project) and "qmd: with groups, all characters should be matched in consecutive groups in rule: (unknown)" (twice, on opening the project). The preview still showed output. Not investigated.
- A first render with a fresh `HOME` timed out ("engine 'julia' timed out during execute") while Julia started cold; the next run worked.
  It is a cold-start cost, not a bug I chased.
- The stale-server and missing-package messages above are candidates for a q2-side hint; see the sections for where.
