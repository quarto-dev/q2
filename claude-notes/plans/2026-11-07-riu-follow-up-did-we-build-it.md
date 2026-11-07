---
title: 'Records in use: follow-up, did we build what we intended?'
date: 2026-11-07
description: 'The epic is done. This plan checks the result against the intent, one question at a time, starting with R through knitr (the export skips every knitr cell, so the demo cannot show R), then the loose ends the RIU-6 demo exposed. Each item says what was intended, how it will be checked, what counts as done, and who runs it.'
status: ready  # written 2026-11-07; reviewed and amended 2026-11-07 (see the review note); no item started except the ticked boxes
---

**Follows:** [the records-in-use epic](2026-11-02-records-in-use-epic.md) (done except the Windows run).
**Evidence so far:** [RIU-6 commands findings](../research/2026-11-07-riu-6-commands-findings.md),
[RIU-6 plan](2026-11-02-riu-06-positron-command-demo.md).
**Review of this plan:** [findings](../research/2026-11-07-riu-follow-up-review.md) (2026-11-07). Finding numbers
below refer to it.
**Related:** [knitr execution-records epic](2026-10-30-knitr-execution-records-epic.md).

> **For implementers:** tests first. Check with `target/debug/q2` built in the checkout, never the
> `q2` on `PATH`. Say which window steps were seen by a person, which ran under Playwright, and which
> were not run. Gates per crate: `cargo clippy -p <crate> --all-targets -- -D warnings` and
> `cargo nextest run -p <crate>`. Nothing is pushed without Gordon's say-so.
>
> **Who runs a box.** `[unattended]` can run without Gordon. `[Gordon]` needs a decision or a person in a
> Positron window. `[either]` is a Playwright run in the checkout that Gordon may repeat by eye.

## Overview

The epic intended this: a q2 execution record is the one place outputs live, and Positron can read
it, write it, and preview from it, for the languages q2 runs. RIU-6 showed that for Python and Julia.
It also showed that for R through knitr the loop is open on one side: the import direction handles
knitr-owned units (KNR-5), but `q2 record export-positron` skips every knitr cell with
"engine is not jupyter or julia" (`crates/quarto-core/src/record/positron_export.rs:23,33`,
`SkipReason::EngineNotExportable`; the owner list is `EXPORT_OWNERS`, `:101`). The epic's decision 11
left R out of the demo; with the knitr stack landed, that is now a gap to close or to state plainly.

## Order and dependencies

1. **Item 4 (second box) first, five minutes, `[unattended]`:** the uncovered-intent list is already in
   the review note; turn it into boxes (done below) so Item 1 does not spend effort before the gaps are known.
2. **Item 1 Phase A**, alone. Then **stop and ask Gordon** with the verdict (a Positron-side change is a
   real possible outcome). Phases B to D wait for the answer.
3. **Item 2** is independent of Item 1 Phases A to C. It contends with Item 1 Phase D only for the e2e
   test file in the fork (`quarto-render-and-push.test.ts`), so serialize those two, not the whole items.
   (The earlier claim that Item 2 decides whether the e2e tests can be trusted for R was wrong: R through
   knitr starts no Julia server.)
4. **Item 3** boxes marked "blocks" come before anything ships; the rest in Gordon's order.
5. **Item 5** (the two open boxes) and **Item 6** (Windows) are independent of everything else.
   Item 6 needs a Windows machine.
6. **Item 8** (`q2 convert` replaces `q2 record`) is independent of Item 1's gate changes; do it before Item 1
   Phase B. Strand `bd-ju7u8a73` holds the ipynb-to-qmd side.
7. **Item 9** (rename `--records` to `--execution-records`) goes before Item 8's implementation. It is
   mechanical and unreleased, so no alias.
8. **Item 7** (one presenter path) is a design decision for Gordon, independent of Item 1: Item 1 needs only
   the gate change. Do not start it without that decision.

## Before the next review session

Spikes that settle facts, not design, and should be done first (Gordon, 2026-11-07):

- [ ] Item 1 Phase A, including the raw-record experiment, the validity list and the Julia `#|` check, ending in
      the verdict note. The gate stays: report and wait.
- [ ] The presenter spikes in [the one-presenter-path epic](2026-11-07-one-presenter-path-epic.md), the ones
      Gordon approves.

## Item 1: R through knitr

**Intended:** a `.qmd` with `engine: knitr` renders once in q2, and its outputs can be pushed into an
open Positron editor and previewed from Positron's cache, as Python and Julia can.

**Seen (CLI, 2026-11-07):** a three-chunk knitr document. `q2 record export-positron` ran it through knitr,
produced a record, printed `{"cells":0, "skipped":[3 x "engine is not jupyter or julia"]}` and then
`error: the record has no cells Positron can display`. R 4.x and knitr 1.50 are installed.
**Seen (review, 2026-11-07, `execute: record: true` on the same shape of document):** all three cells are
*structured* code cells owned by `knitr`: a `stream`; a `display_data` with `image/png`; a labeled cell with two
`stream` outputs (message and data frame, both text). Each carries `quarto_hash_rule` and `quarto_header_keys`.
None is a fallback cell, so the owner rule is the only reason the export skips them. Not run: anything in a
Positron window, the hash comparison, Ark's cache shapes.

**Principle (Gordon, 2026-11-07).** q2 does nothing special for Positron. Where q2 produces a compatible
execution record, Positron can show it; where it does not, Positron will not. Designing Positron-side features
is out of scope. RIU-0 S7 found that a real q2 Jupyter Python record, **untransformed**, displayed the same as
the export (RIU-5, "What S7 and spike J showed"; not re-run on 2026-11-07). So the export should shrink to what a
*valid* cache file needs, and each rule that is a workaround for a Positron display behavior is a decision, not
a default. The kernel-equivalence declaration below is an **import** concern only: on import, Positron's outputs
are attributed to a q2 unit, and a unit owned by marimo or an extension engine may run something other than what
Positron's kernel ran. On export the recorded output is q2's own real output for that cell, true whichever engine
produced it. (This corrects the same day's earlier proposal of one declaration for both directions.)

### Phase A: characterize (spike, no product code) `[unattended]`

- [x] Dump a knitr record and say, per cell, structured or fallback (done in the review, above).
- [x] ~~Check whether the content hash equals the knitr record's `quarto_content_hash`.~~ **Settled by
      reading, 2026-11-07:** `quarto_content_hash` is computed in core for every engine from the author's block
      text, `#|` lines included (`crates/quarto-core/src/record/ids.rs:3-26`); the `structured` profile only
      shapes `quarto_exec_hash`. The R fixture's hashes already equal q2's
      (`every_cached_hash_equals_the_one_q2_computes_through_the_splice_path`, `positron_source.rs:142`).
      (The earlier version of this box rested on a reviewer's wrong premise; see the review note, finding 3.)
- [ ] **The raw-record experiment (replaces "dump and decide which cells to skip").** In the fork's e2e harness
      (Playwright against the Positron dev build), push **unfiltered** records, exactly as `q2 render` with
      `record: true` writes them, into open documents, and say what Positron shows: (a) a knitr R document with
      a plain chunk, a plot, a message, a data frame, an error chunk, an `asis` chunk, a `results: hide` chunk and
      a raw-HTML chunk; (b) a Julia document with a plot; (c) a Python document with an array-valued or
      JSON-valued output; (d) a document with an inline span. Anything that fails to display is a finding about
      Positron to report, not something q2 works around. Mark each case seen, Playwright, or not run.
- [ ] Read what Positron's R side (Ark) stores and shows for a `.qmd` (the same `quarto-inline-outputs` cache?),
      and compare shapes with the knitr cells: data frame as text versus HTML, the message stream, an empty
      `ename`, messages on stdout (RIU-3 lists these as KNR-5 presenter limits). This matters for the import
      direction. Fork worktree: `/Users/gordon/src/positron-riu6-a` (branch `spike/riu-6-phase-a`, tree clean at
      `99aa72fcb4`).
- [ ] `[unattended]` **Spike, before the next review.** What is left of the hash question is Positron-side only: does Positron's parser read `#|`
      lines the same way for a Julia cell as for Python and R? Check in the fork, with one Julia `#|` cell
      (the raw-record experiment can include it).
- [ ] **Validity list.** From Positron's loader and the conformance mirror, write down exactly what a cache file
      needs to avoid being deleted (notebook metadata such as `quarto_cache_version`, `outputs` always present,
      and whatever else the loader checks). Everything outside that list is a candidate for removal from the
      export.
- [ ] **Design, import side (agreed with Gordon 2026-11-07).** Replace the `outputs_for` owner rule
      (`jupyter`, `julia`, or `knitr` with token `r`) with one declared fact, **kernel equivalence**: for which
      languages is a unit this engine owns ordinary kernel code, so the same text gives the same outputs in a
      plain Positron kernel? Shape: a trait method on `ExecutionEngine` following `try_claims_language`
      (`engine/traits.rs:176-184`): `Option`, `None` means "would have to load", and **`None` is not admitted**
      (undeclared engines are excluded). Jupyter and Julia declare any plain language; knitr declares `r`. The
      data checks stay on the cell: no fallback marker, syntactic language. Engines outside today's list are
      admitted only by declaration, each with a test (Gordon: "we'll have to test"). Independent of Item 7.
- [ ] Write the answers into a findings note with a verdict on what the export must still do. Likeliest: write
      the record as a valid cache file and nothing else, with the image-representation rule an explicit decision.
- [ ] **`[Gordon]` Gate: report the verdict and wait** (the gate itself agreed with Gordon 2026-11-07). The
      raw-record experiment may show a Positron-side problem; that is a decision for Gordon, not a q2 workaround.

### Phase B: tests first (after the gate)

- [ ] Export tests for a knitr record: text output, a figure (one `image/png`), a message, a data frame, an
      error, a chunk with options. Each fails on the current skip (`EngineNotExportable`).
- [ ] A test that the export no longer consults the engine or language: a cell owned by any engine, in any
      language, appears in the output when the record has outputs for it (`marimo`-style `python.marimo` and
      a fallback cell included, if the raw-record experiment showed they do no harm).
- [ ] Import-side tests, one per engine in the table: a unit takes Positron outputs only when its engine
      declares kernel equivalence for its language; an undeclared engine gets nothing.
- [ ] A CLI test through the binary: `q2 record export-positron` on an `engine: knitr` fixture exits 0 and
      prints cells greater than 0. Keep the Python and Julia tests green.
- [x] **Image-representation rule: keep it for now, remove it when the fork fixes the view zone** (Gordon approved
      2026-11-07). The rule (RIU-5, "Output data (representations)": drop `application/pdf`, prefer PNG over SVG,
      drop a `text/html` that only repeats a data-URI image) works around a Positron display behavior (J8:
      Positron renders every item of a bundle, so Julia plots appeared twice with base64 text). It stays in the
      export, commented as temporary, until the fork's `quartoOutputViewZone.ts` chooses one representation.
- [ ] `[unattended]` Track the removal: when the fork's view zone picks one representation, delete the rule and
      its tests (a box here, not a strand: it belongs to this plan).

### Phase C: export

- [ ] Remove `EngineNotExportable`, the inline-unit, extension-language, fallback and duplicate-group skips
      and their `SkipReason` variants, as far as the raw-record experiment and the validity list allow; keep the
      validity wrapper. Do not add `knitr` to `EXPORT_OWNERS`. Update the subcommand help text and the RIU-5
      plan's rule list (that plan is landed; add a dated note there rather than rewriting its history).
- [ ] The import gate: implement the kernel-equivalence declaration from the Phase A design and delete
      `EXPORT_OWNERS`, `JUPYTER_ENGINE`, `JULIA_ENGINE`, `KNITR_ENGINE` and the `lang == "r"` literal as gate
      inputs.
- [ ] Gates on `quarto-core` and the `quarto` binary crate. Run the real binary on the fixture and read
      the JSON and the ipynb.

### Phase D: in a Positron window

- [ ] `[unattended]` An R session exists in the dev build (the e2e runs set `POSITRON_R_VER_SEL=4.3.2`; check
      it is set in the fork's e2e config and that the session starts).
- [ ] `[unattended]` An e2e test, R version of `quarto-render-and-push.test.ts`: command 2 on an R document,
      outputs appear; command 3 shows them from the cache with no R process started by the preview.
      Serialize with Item 2's edits to the same file.
- [ ] `[Gordon]` Someone watches it once. Mark each step seen, Playwright, or not run.

**Done when:** an R document shows its knitr outputs in the open editor after command 2, the preview
shows them from the cache, and the findings note says what Positron did with each kind of raw record and
which of those are Positron-side findings.

## Item 2: the Julia e2e tests are slow and flaky

**Intended:** the demo's tests pass on a clean run. **Seen:** on 2026-11-07 the Julia push tests passed
once (the five-output case and the preview), then timed out at 120 seconds on a second full run
(`.quarto-inline-output` count stayed 0 while waiting for 5). A first run after a Julia server restart
pays startup and precompile time.

- [x] Time the export cold and warm (CLI, 2026-11-07, Julia fixture, scratch directory): cold 20 s, warm
      10 to 12 s. The engine is Quarto 1's `julia-engine` extension: a detached server, a transport file in
      `~/Library/Caches/quarto/julia`, reused by the next render. The launcher starts it with
      `serve(; timeout = 300)` (`resources/extension-subtrees/julia-engine/_extensions/julia-engine/quartonotebookrunner.jl:48`),
      and the server log showed "Server stopped" mid-run, so a gap of more than five minutes means a cold start.
      `q2 call` has no `julia` subcommand (`crates/quarto/src/main.rs:1066-1084`: `test`, `build-ts-extension`
      and a hidden `inline-css`), so `status` and `stop` are not reachable from q2.
- [x] With a warm server both Julia tests pass (2 passed, 2.9 min). On a full-file run they twice timed
      out at the default 120 s with 0 outputs; the failing run's server log was overwritten before it was read,
      so the cause (cold server, versus a stale server) is likely but not proven. (A cold start measured at
      20 s does not alone explain 120 s with zero outputs; a first-ever precompile could.)
- [x] Change **committed** in the fork as `53bf9ca105` ("warm the Julia server, longer Julia timeout, clear the
      viewer and remove files the tests leave behind"): `warmJuliaServer` at
      `test/e2e/tests/quarto/quarto-render-and-push.test.ts:17`, `setTimeout(480000)` on the Julia tests. (An
      earlier version of this plan said "uncommitted"; that was wrong.)
- [ ] `[unattended]` Re-run the whole file three times and report pass counts. Keep each run's Julia server
      log (copy it before the next run overwrites it).
- [ ] `[unattended]` **Find the EPIPE closer, cheapest experiment first.** The Julia engine is a TS extension, so
      the client socket is held by a **Node child of q2**, not by q2; an EPIPE on the server's progress write
      means that Node child (or q2 above it) was killed. Candidates: the extension's `execFile` timeout
      (`extensions/q2-records-demo/src/exportCommand.ts`, default 300 s, SIGTERM, `q2.exportTimeoutSeconds`),
      Playwright test timeout or worker teardown (the commands findings already record a teardown timeout on
      every run), q2's own cancel or kill path (`crates/quarto-core/src/engine/ts_process.rs`). Experiment:
      (1) in a scratch directory run `q2 record export-positron` on the Julia fixture, SIGKILL q2 and its Node
      child a few seconds in, and read the server log: EPIPE there means a hung-up client is sufficient;
      (2) match the EPIPE timestamp in an e2e server log to the per-test end times: inside a test means a
      timeout kill, at the end means teardown; (3) check for a second q2 run (the warm helper, or an orphan from
      an earlier timed-out test) still in flight when a passing test tore down.

## Item 3: loose ends from RIU-6

**This is still a demo (Gordon, 2026-11-07).** "Ship" means the demo in the fork, so nothing here blocks a
release; each item is either fixed for the demo, or written down as a known issue in the demo's run guide
(the demo folder under `extensions/q2-records-demo`). Each "Gordon" box is a decision, not an assumption.

**Fix or document for the demo**

- [ ] `[unattended]` **Positron's cache location: prototype a Positron feature, idiomatically** (Gordon,
      2026-11-07: a Positron-side prototype is wanted here). Found so far, by a targeted grep of the fork, not
      an exhaustive search: the extension takes the sibling of `ctx.globalStorageUri`
      (`q2-records-demo/src/previewHelpers.ts:31-35`), which is profile-scoped, while Positron's cache service
      always writes under `defaultProfile.globalStorageHome`
      (`src/vs/workbench/contrib/positronQuarto/browser/quartoOutputCacheService.ts:133-136`). Positron's own
      docs extension notes the same profile scoping (`extHostDocsNode.ts:229-234`). No existing setting or API
      for the cache directory turned up. The precedent for an extension asking the workbench is the internal
      command `_positronQuarto.pushOutputsFromFile` (`quartoPushOutputs.ts:29`). First step: investigate how
      Positron exposes workbench-owned paths and settings to extensions (a public `positron.*` API, a
      contributed setting, or an internal command) and pick the most idiomatic; the cache service already holds
      `_cacheDir`, so a small command or API next to the push command is the likely shape. Then: tests first
      (a vitest for the extension's use of it, and a check with a non-default profile), replace
      `cacheDirFromGlobalStorage`, and make a missing answer an error that names the cause. Done when command 3
      and command 2 work under a non-default profile.
- [x] **Viewer big X leaves q2 running: document and ship** (Gordon, 2026-11-07). The extension already kills
      q2 on `onDidRequestStop` and `onDidDispose` (`previewCommand.ts:93-94`) and on deactivation (`:113`);
      closing the Viewer view evidently does not dispose the panel, which the findings attribute to Positron's
      Viewer. The leak is bounded by the Positron session; Stop works.
- [ ] `[unattended]` Write that limitation, and "use the Viewer's Stop", into the demo's run guide.
- [ ] `[unattended]` **Command 2: progress and cancel.** The engine may run the whole document with no
      progress or cancel. Confirm what the command shows today and write the gap down (a box if code is needed,
      otherwise a line in the run guide).
- [ ] `[unattended]` **Dirty document wording.** Epic decision 7 says a dirty document exports from disk and the
      push skips changed cells with a report; the commands findings say "exported as saved, with a note". Check
      the actual behavior and make the two statements agree.

**Should fix for the demo, or document**

- [ ] `[unattended]` **Stale Julia servers and a missing Plots project** produce confusing results (`startpath
      must be non-empty`; an `ArgumentError` exported as an error output that looks like a working run). Item 2
      covers timing and EPIPE only. Write down what q2 can say when it sees either.
- [x] The preview banner text "this page reconnects automatically" is wrong for a one-shot server (epic
      decision 15 says no banner). *Gordon, 2026-11-07: don't care; left as is.*
- [x] Does pushing over existing outputs ask first? *Gordon, 2026-11-07: no. Command 2 replaces the outputs of
      each matched cell with no prompt (its job); say so in the run guide.* The push is
      `_positronQuarto.pushOutputsFromFile` in the fork, which clears and re-adds a matched cell's outputs
      (epic S5; `pushCells` was not read).
- [ ] `[unattended]` **Not run in any window:** untitled document, "q2 too old" (exit 2), non-default profile,
      a dirty document. Wrong `q2.path` was run under Playwright and failed correctly (commands findings);
      change the box to say that. The first two can be added to the fork's e2e suite.

**Polish or documentation**

- [ ] `[either]` **An edited cell loses its syntax highlighting in the q2 preview** (commands findings, RIU-3a).
      Not a question about editability (the preview is not edited; the document is, in Positron's editor). The
      cell is edited, so it no longer matches its cached output and is shown with no output. What is not known is
      whether the missing highlighting is expected for a cell with no output (compare an unexecuted `{python}`
      cell in `q2 render` and in `q2 preview`) or a bug in how an unmatched cell is presented in a
      records-only preview. **`q2 render` comparison, 2026-11-07 (uv venv with ipykernel in the scratch
      directory, `PATH` and `JUPYTER_PATH` pointing at it, `target/debug/q2`):** a `{python}` cell with
      `#| eval: false` renders highlighted (`<pre class="sourceCode python">` with `hl-*` spans), as do an
      executed cell and a plain ` ```python ` fence. So an unexecuted cell is highlighted in `q2 render`, and the
      missing highlighting in the Positron preview is probably specific to how the records-only preview presents
      an unmatched cell. Still to do: a Playwright check against `q2 preview --records <dir>` with an edited cell;
      file a box if it is a bug.
- [ ] `[unattended]` **Command 1 on Julia and R spans.** Command 1 (`positronQuarto.runInlineExpressions`,
      `quartoRunInlineExpressions.ts` in the fork) is Positron-only and never calls q2. Its span detector takes the
      document's primary kernel language (`findInlineExprSpans(lines, language)`, `quartoInlineExpr.ts:35`; other
      languages' spans are ignored) and runs each span on the document's kernel; nothing in
      `quartoInlineExprExecutor.ts` is Python-specific. Only Python was run (42 and 3.0, also under
      Playwright), so Julia and R are untested, not known broken. Add a Julia and an R document to the fork's e2e
      for command 1. Independent of Item 1.
- [x] **Command 1 values do not persist, and that is the demo's honest state** (read 2026-11-07; Gordon: span
      outputs are not yet tied back to execution records). The values are editor decorations only
      (`createDecorationsCollection` with injected `after` text, the fork command,
      `quartoRunInlineExpressions.ts:62-82`); no file under the command writes the output cache, so they vanish on
      reload and command 3 cannot show them. Command 1 is an out-of-band implementation, in the way Q1 handles
      span execution, not a use of records. Say so in the run guide. **Eventual goal (a proposal to Positron, not
      demo work):** persist span outputs in the cache and carry them as inline units in execution records, so
      spans round-trip like cells. q2's side already has the shape: records hold spans as inline units with
      domain-separated hashes (`span_content_hash`, `crates/quarto-core/src/record/ids.rs:31-36`). See the
      "Eventual goal" section below.
- [ ] Command 2 does not change Positron's kernel state: a later cell that uses a variable from a pushed cell
      fails. Say so in the user-facing text before it ships.

## Item 4: does the epic match the tree

- [x] RIU-6 Phase E, done 2026-11-07: every plan re-read and its boxes checked against the tree (open boxes
      all, ticked boxes by sample); statuses, the epic's table, board and exit criteria updated; see the
      epic's "Closing reconciliation". Decision 11 (R) gets its outcome from Item 1.
- [x] Re-read the epic's stated intent once more against what the three commands do (done in miniature in the
      review note, "Intent the epic states that no checklist covers"). Each gap is now a box:
- [ ] `[unattended]` **Hub verification.** Exit criterion 1 says preview and hub work from records alone,
      "verified through the real binary and a browser". Find where the hub half was verified and cite it, or run
      it (`q2 provide-hub` with the writer gate on, a hub client).
- [ ] `[unattended]` **Embedded-docs refresh** (the epic's last step): `cargo xtask build-agents-docs`, then
      `cargo build --bin q2`, then `target/debug/q2 docs llms --embed-info` and `target/debug/q2 render docs/`.
      The 3b docs edits make the embed stale. Not a box in any plan today.
- [ ] `[either]` **RIU-3 browser items never seen:** `--records positron,engine` running the engine, a cache
      older than the document shown in a browser, `--share --allow-edit` with a guest, "Clear results" clicked,
      a real click or scroll sync.
- [x] **Stamped cells carry no `data-loc`**, so a click inside a stamped cell maps to no source line. This
      follows from decision 14 (stamped means not editable). *Gordon, 2026-11-07: fine.*
- [ ] `[unattended]` Decision 12 (a failed optional `record: true` copy fails soft) says "not re-verified
      against the code"; re-verify with the real binary.
- [x] Fix the epic's "Closing reconciliation" (`records-in-use-epic.md`), which said RIU-3b never started,
      against its Exit criteria, which mark 3b met. *Done 2026-11-07.*
- [x] Fix the stale Julia lines: the parent epic's decision 5 and its risk 2 said Julia was still presented by
      the TS `toMarkdown`, but `b4d827186` (2026-11-01) flipped it; `exr-08-julia.md` got a pointer. *Done
      2026-11-07.* `core_presents` (`engine/ts_record.rs:500`) is Julia with a record wanted and a non-empty
      unit list; the TS path still runs otherwise.
- [ ] `[unattended]` Fix the stale doc comment at `crates/quarto-core/src/presenter/options.rs:237` ("read
      under knitr's dotted names only"; the code reads the canonical dashed `fig-cap` for both front ends).

## Item 5: RIU-3b (landed 2026-11-07) and the 3a leftovers

**3b landed on `feature/riu-3b-stamping`** (`3f0c55a6d` here): the stamp post-walk in
`crates/quarto-core/src/stage/stages/record_splice.rs`, `jupyter-output` in `ATOMIC_KINDS`
(`ts-packages/preview-renderer/src/utils/sourceInfo.ts`), the stamp tests and the viewer regression test. See the
3b note in the RIU-3 plan. An earlier version of this section said it never started; that was written before the
merge.

- [x] Decide with Gordon whether 3b is wanted now, deferred, or dropped. *Decided 2026-11-07: wanted now. Done.*
- [x] Re-execute on an imported page is refused with no engine and runs the engine when it is listed:
      `re_execute_on_an_imported_page_is_refused_without_engine_and_runs_it_when_listed`
      (`crates/quarto-preview/src/capture_driver.rs:3194`).
- [ ] `[unattended]` **EXR-16 sweep.** One table-driven test in
      `crates/quarto-core/tests/integration/positron_source.rs` (or a sibling in the same binary) that, for each
      of the 21 `CASES` in `positron_conformance.rs`, builds a Positron-style cache from the case's fixture,
      imports, converts and splices, and asserts which units receive outputs. Cases where Positron and q2 diverge
      (`positron-expected.json`) assert the divergence. Today `positron_conformance.rs` tests only the generator
      and the loader; `positron_source.rs` covers three durable fixtures (python, hand-built julia, r). Indirect
      unit coverage already exists for `blockquote`, `crlf`, `duplicate_pair` and the marimo cases (hash,
      placement, matching, export), not through import.
- [ ] `[unattended]` **Labeled-cell `fig-cap` splice case.** A Python cell with `#| label: fig-x` and
      `#| fig-cap: ...` through import, convert and splice, asserting the output shows on the page.
      `knitr_positron_import.rs:179` and `:261` cover the hash and the record shape only.

## Item 6: the Windows run (deferred by the epic)

Carried over from the epic's unchecked box so it is not lost. Needs a Windows machine (`aws-lc-sys` blocks the
msvc cross-check from macOS). The review's targeted list, highest risk first:

- [ ] `[Gordon]` `crates/quarto-hub-provider/src/execute.rs:331` and `:527`: `canonicalize()` returns a `\\?\`
      verbatim path on Windows; check the "outside the document directory" comparison.
- [ ] `[Gordon]` `crates/quarto-core/src/positron_cache.rs:150` (`vscode_file_uri`) and the `%APPDATA%` rule:
      drive letter, `%3A` and non-ASCII are unverified.
- [x] The `--source-uri` default (`record.rs:139`, native path in a wire format): forward slashes, agreed; see Item 8.
- [ ] `[Gordon]` The CLI test file (`record_export_positron_cli.rs`, replaced by Item 8's test): `python3` at about `:250`, and
      the literal `/some/dir` at about `:113`.
- [ ] `[Gordon]` `crates/quarto-core/tests/integration/julia_engine_e2e.rs:1075-1098`: `kill` and `pgrep` with no
      `cfg(unix)` gate; confirm they cannot run on Windows with Julia installed.
- [ ] `[Gordon]` `crates/quarto-core/src/record/sources.rs:49,101-107,179-199`, and
      `crates/quarto-preview/src/capture_driver.rs:2932-2934`: native-separator display and a self-consistent URI test.
- [ ] `[Gordon]` CRLF: `positron-conformance/crlf.qmd` and `positron-cache/*.ipynb` under `autocrlf`.
- [ ] `[Gordon]` Then the epic's command, once: `cargo clippy -p quarto -p quarto-core -p quarto-preview
      -p quarto-hub -p quarto-hub-provider -p quarto-ast-reconcile --all-targets -- -D warnings` and
      `cargo nextest run` for the same crates.

## Item 7: one presenter path (now its own epic)

**Decided (Gordon, 2026-11-07): yes, make it an epic.** It lives in
[the one-presenter-path epic](2026-11-07-one-presenter-path-epic.md), which holds the intent, what is known, the
open design questions and the proposed spikes. The number of plans is not known yet: **settling it is the goal of
the next review session**, which reviews the presenter design with Gordon first. Nothing under this epic starts
before that review. Item 1 does not depend on it (the exporter reads raw outputs and never calls the presenter).

- [x] `[Gordon]` Decide whether to make this a plan. *Yes, an epic; structure first, plans after the review.*
- [ ] `[unattended]` The pre-review spikes listed in the epic, if Gordon approves them.

## Item 8: `q2 convert --to positron-ipynb` replaces `q2 record export-positron`

**Intended (Gordon, 2026-11-07):** remove `q2 record`, which is too specific, and use `q2 convert`, as Q1 had it,
with the output format explicit. Q1's command chose its direction from the input's extension
(`isJupyterNotebook`, `external-sources/quarto-cli/src/command/convert/cmd.ts`) and wrote beside the input,
overwriting silently. q2's `convert` is only a stub (`crates/quarto/src/commands/convert.rs` returns
`NotImplemented`; its `--output` flag is documented as a format, unlike Q1's `-o` path). `q2 record` is unreleased
(it landed in `d6954ccdf`, RIU-5 B2, on the epic branch), so removing it costs nothing. The ipynb-to-qmd and
plain qmd-to-ipynb sides are **not** part of this plan; they are strand `bd-ju7u8a73`.

**Design (agreed 2026-11-07):**

```
q2 convert <input> --to <format> [--from <format>] [-o <path>] [target options]
```

- `--to` is required and never inferred. `--from` is optional, inferred from the input extension (`.qmd` and
  `.md` are qmd, `.ipynb` is ipynb) and an error otherwise. An unsupported pair is an error that lists the
  supported pairs. Names and flags match `q2 render` (`--to`, `-o`).
- Supported in this plan: `--from qmd --to positron-ipynb`, the Positron cache notebook. Reserved by name, not
  implemented: `--to ipynb` and `--from ipynb`.
- Options for `positron-ipynb` only, rejected for other targets: `--execution-records q2,engine` (the preview and
  provide-hub grammar, only those two values accepted; default `q2,engine`; `--execution-records q2` forbids running the
  document) and `--source-uri`.
- `-o <path>` is required for `positron-ipynb`, because the file name belongs to the consumer (a hash of the
  document URI, computed by the extension). `-o -` writes the notebook to stdout, and the JSON report then goes
  to stderr. Otherwise the report is the one JSON line on stdout, as today.
- An existing output file is overwritten, with an info diagnostic naming it (Gordon's choice; Q1 overwrote
  silently).
- Unchanged: `export_for_positron` in `quarto-core`, the report fields, exit codes 10 to 13 (documented under
  the target), exit 2 for usage errors (so the extension's "q2 too old" detection still works: an older q2's
  `convert` has no `--to`). A `convert` that may run code is stated in its help text: Q1's never executed.
- Not `-M`: `-M` is document metadata in `q2 render`, `preview` and `provide-hub`.
- The `--source-uri` default is the document's path written with forward slashes
  (`quarto_util::to_forward_slashes`), per the cross-platform rule for paths in a wire format. **Agreed with
  Gordon 2026-11-07.** Today it is the native path (`commands/record.rs:138-139`). `normalize_location` in
  `positron_cache.rs` accepts both forms and Positron ignores the field on load. A test builds its path with
  `Path::join`. (q2 builds no `file://` URI: epic decision 13.)

**Order:** independent of Item 1's gate changes (the library function is untouched), but do it before Phase B of
Item 1 so the CLI test written there targets the new command. Item 1's references to `q2 record export-positron`
mean this command from here on.

- [ ] `[unattended]` Tests first, through the binary (`crates/quarto/tests/integration/`, replacing
      `record_export_positron_cli.rs`): `--to` missing is exit 2; an unsupported pair lists the supported ones;
      `--from` inferred from `.qmd`; an unknown extension errors; `--execution-records` values other than `q2` and `engine`
      are rejected; `--execution-records` and `--source-uri` are rejected for a target that does not take them; `-o -` puts
      the notebook on stdout and the report on stderr; an existing file is overwritten and an info line names it;
      exit codes 10 to 13 keep their meanings. Move, do not weaken, the existing cases.
- [ ] `[unattended]` Implement `commands/convert.rs` (replacing the stub), wire `Commands::Convert` in
      `main.rs` with `--to`, `--from`, `-o/--output` (a path), `--execution-records`, `--source-uri`, delete
      `commands/record.rs`, the `Record` subcommand, and the old test file name `record_export_positron_cli.rs`. Gates:
      `cargo clippy -p quarto --all-targets -- -D warnings` and `cargo nextest run -p quarto`.
- [ ] `[unattended]` Run the real binary end to end on a Python fixture and on the knitr fixture
      (`q2 convert d.qmd --to positron-ipynb -o out.ipynb --source-uri file:///x/d.qmd`), read the JSON and the
      ipynb, and record the invocation and output here.
- [ ] `[unattended]` Update the fork's extension: `q2Contract.ts:34-38` (`buildExportArgs`: `convert`, `--to
      positron-ipynb`, `-o`), its vitest, the comment at line 7, and `exportCommand.ts`. Keep the "too old"
      detection and add a vitest for it.
- [ ] `[unattended]` User-facing docs: a `docs/` page for `q2 convert` (usage only), then the embedded-docs
      refresh (`cargo xtask build-agents-docs`, `cargo build --bin q2`). Update the RIU-5 and RIU-6 plans' command
      references with a dated note rather than rewriting them.
- [ ] Windows: the forward-slash default is the only path logic here; its test is portable. The rest of the
      Windows list stays in Item 6.

## Item 9: rename `--records` to `--execution-records`

**Intended (Gordon, 2026-11-07):** the flag that names where a document's execution record comes from is spelled
`--execution-records`, matching the term used everywhere else ("execution records"). **Unreleased, so no alias and
no deprecation period:** neither `v0.42.0` (the latest tag) nor `main` has the flag (checked with
`git grep -e '--records' v0.42.0 -- crates/quarto/src/main.rs docs`: no matches). The grammar is unchanged:
a comma list or repeated flag; entries `cache`, `q2`, `positron`, `engine`, or a directory path; the defaults per
command; `cache` refused for `provide-hub` and `convert`; `convert` accepts only `q2` and `engine`. The setting
`preview.engine` is a different name and does not change.

**Where it appears** (a mechanical rename; found 2026-11-07 with `git grep -e '--records'`):

- CLI definitions, `crates/quarto/src/main.rs`: the `records` fields at lines 63 (`record export-positron`,
  removed by Item 8), 352 (`preview`) and 809 (`provide-hub`), the parse tests (about lines 1408-1430, 1583,
  1609) and the doc comments at 311 and 1574. Renaming the field to `execution_records` renames the long flag.
- Commands: `crates/quarto/src/commands/preview.rs` (4 places), `provide_hub.rs` (2) and `record.rs` (2, which
  Item 8 deletes).
- User-visible messages and their tests: `crates/quarto-core/src/engine/diagnostics.rs` (the page warning and its
  tests, 6 places), `crates/quarto-core/src/record/sources.rs` (3, including the `cache` refusal text),
  `crates/quarto-hub-provider/src/execute.rs` (4), `crates/quarto-preview/src/re_execute.rs` (2, one is a test
  assertion), `capture_driver.rs` (2, one assertion), `lib.rs`, `share.rs` and the two integration tests that
  mention it.
- User-facing docs: `docs/guides/authoring/execution-records.qmd` (4 places) and
  `docs/guides/projects/preview.qmd` (4), then the embedded-docs refresh.
- TypeScript: `q2-preview-spa/e2e/helpers/previewServer.ts` (3) and `q2-preview-spa/e2e/records-sources.spec.ts`
  (7).
- The fork's extension: `q2Contract.ts` (`buildPreviewArgs`, line 54, and its comments) and `previewCommand.ts`,
  with their vitests.
- Notes: 19 files under `claude-notes/` mention the old flag. They are history and are not rewritten; the epic
  gets one dated line saying the flag was renamed.

**Order:** do this before Item 8's implementation, so `q2 convert` is written with the new name from the start;
Item 8's tests use the new name.

- [ ] `[unattended]` Tests first: change the parse tests in `main.rs` and the assertions in `diagnostics.rs`,
      `re_execute.rs`, `capture_driver.rs`, the share and hub-provider integration tests to the new spelling;
      add a test that the old spelling is rejected as an unknown flag on `preview` and `provide-hub` (no alias).
      They fail first on the current name.
- [ ] `[unattended]` Rename the flag and every message, comment and doc string in the list above. Gates:
      `cargo clippy -p quarto -p quarto-core -p quarto-preview -p quarto-hub-provider --all-targets -- -D warnings`
      and `cargo nextest run` for the same crates.
- [ ] `[unattended]` Update `q2-preview-spa/e2e` (two files) and run the records-sources e2e if it can run here;
      say which ran.
- [ ] `[unattended]` Update the two docs pages, run `target/debug/q2 render docs/`, then the embedded-docs
      refresh (`cargo xtask build-agents-docs`, `cargo build --bin q2`).
- [ ] `[unattended]` Update the fork extension and its vitests (`q2Contract.ts`, `previewCommand.ts`).
- [ ] `[unattended]` A final `git grep -e '--records'` outside `claude-notes/` must come back empty; add the
      dated rename line to the epic.
- [ ] `[unattended]` Run the real binary: `q2 preview <doc> --execution-records positron,engine --no-browser`
      starts as before, and `q2 preview <doc> --records positron` fails as an unknown argument. Record the
      invocations and output here.

## Eventual goal: span outputs in execution records (not demo work)

Command 1 runs inline expressions out of band: it executes each span on the document's kernel and shows the value
as an editor decoration, much as Q1 handles span execution. Nothing ties those values back to execution records.
The eventual goal is that a span's output is a first-class record cell: Positron would persist it in the cache
(a Positron-side feature to propose, not to build in the demo) and q2 would export and import it as an inline
unit, the way cells already round-trip. Until then the export skips inline units (Positron skips them itself) and
command 3 cannot show span values. When this is planned, start from: q2's inline units and span hash
(`ids.rs:31-36`), the import and export skip rules for inline units, and Epic decision 6 (a Positron record is
display-only).
