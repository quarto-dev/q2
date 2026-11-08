---
title: 'Records in use: follow-up, did we build what we intended?'
date: 2026-11-07
description: 'The records-in-use epic is done. This plan checks the result against the intent and closes what the RIU-6 demo exposed. R through knitr is characterized (Positron accepts a raw q2 knitr record; the export gate is the only blocker); what remains is gated on Gordon''s verdict, then a command rename, the export change, and a list of loose ends, each marked with who runs it.'
status: ready; Item 1 Phase A done and waiting at Gordon's gate; Items 8 and 9 designed, not started; rewritten 2026-11-07 after the Phase A and presenter spikes
---

**Follows:** [the records-in-use epic](2026-11-02-records-in-use-epic.md) (done except the Windows run).
**Evidence:** [RIU-6 commands findings](../research/2026-11-07-riu-6-commands-findings.md),
[RIU-6 plan](2026-11-02-riu-06-positron-command-demo.md),
[review of this plan](../research/2026-11-07-riu-follow-up-review.md),
[R through knitr, Phase A](../research/2026-11-07-r-through-knitr-phase-a.md),
[rewrite log](../research/2026-11-07-plan-rewrite-log.md) (what changed in this rewrite and why).
**Related:** [knitr execution-records epic](2026-10-30-knitr-execution-records-epic.md),
[one presenter path](2026-11-07-one-presenter-path-epic.md).

> **For implementers:** tests first. Check with `target/debug/q2` built in the checkout, never the `q2` on
> `PATH`. Say which window steps were seen by a person, which ran under Playwright, and which were not run.
> Gates per crate: `cargo clippy -p <crate> --all-targets -- -D warnings` and `cargo nextest run -p <crate>`.
> Nothing is pushed without Gordon's say-so.
>
> **Who runs a box.** `[unattended]` can run without Gordon. `[Gordon]` needs a decision or a person in a
> Positron window. `[either]` is a Playwright run in the checkout that Gordon may repeat by eye.

## What is established

- The epic intended that a q2 execution record is the one place outputs live, and that Positron can read it, write
  it, and preview from it. RIU-6 showed that for Python and Julia.
- **R through knitr (Phase A, 2026-11-07, Playwright):** Positron accepts a raw knitr record unfiltered (8 of 8
  cells matched by hash) and shows every output; its own hashes for R and Julia cells equal q2's. The only thing
  stopping `export-positron` on a knitr document is the owner rule (`EXPORT_OWNERS`,
  `crates/quarto-core/src/record/positron_export.rs:101`, `SkipReason::EngineNotExportable`). Positron shows what
  the record holds, so `results: hide` output, `asis` markdown and raw HTML appear as text; that is Positron's
  behavior, not a q2 workaround. Positron's validity rules are written down in the note. Nothing was watched by a
  person.
- The import direction already handles knitr-owned units (KNR-5).

## Gordon's decisions (recorded; not to be reopened here)

- **Principle:** q2 does nothing special for Positron; where q2 produces a compatible record Positron shows it.
  Positron-side feature design is out of scope.
- The export shrinks to what a valid cache file needs. Kernel equivalence is declared on the **import** side only.
- The image-representation rule stays, commented as temporary, until the fork's view zone
  (`quartoOutputViewZone.ts`) chooses one representation.
- The `--source-uri` default is the document path written with forward slashes (`quarto_util::to_forward_slashes`).
- `q2 convert --to positron-ipynb` replaces `q2 record` (Item 8; the ipynb and qmd side is strand `bd-ju7u8a73`).
- `--records` is renamed `--execution-records`, no alias (Item 9).
- This is still a demo. "Ship" means the demo in the fork; each loose end is fixed for the demo or written down
  as a known issue in the demo's run guide.
- Command 1 is live-only (never calls q2); persisting span outputs in records is an eventual goal, not demo work.
- The Viewer's big X leaves q2 running: document it (Stop works); do not fix.
- Command 2 replaces a matched cell's outputs with no prompt; the preview banner text is left as is; stamped
  cells carrying no `data-loc` is fine; RIU-3b was wanted and is done.
- The Phase A gate: report and wait.

## Open decisions (Gordon's)

- [ ] `[Gordon]` **The Phase A verdict.** Read the note, then say whether Item 1 proceeds as proposed (export
      shrinks to a valid cache file; the knitr cells export) and how to treat the Positron-side behaviors in the
      note: a `results: hide` output shown, `asis` and raw HTML shown as text, a Julia plot bundle shown
      several times (the image rule covers only the last). In particular: **should the export drop output the
      document says to hide (`results: hide`)?** The principle says no. This is the only decision that blocks
      Item 1 Phases B to D.
- [ ] `[Gordon]` **Command 2: progress and cancel, and the dirty-document wording: fix or document?** Facts read
      from the fork on 2026-11-07: the command shows an indeterminate notification with the title "q2 is running
      FILENAME. The first run can take a few minutes." and no cancel button; the only stop is the timeout
      (`q2.exportTimeoutSeconds`, default 300 s, SIGTERM). A dirty document gets the information note "q2 runs the
      saved file, so cells you have edited since saving will not show outputs", and the push then reports how
      many cells matched and how many were skipped. Epic decision 7 ("a dirty document exports from disk and the
      push skips changed cells with a report") agrees with that in substance, so the two statements need no
      reconciling; whether the wording is good enough, and whether cancel is wanted, is the decision.

## Order and dependencies

1. **Item 9, then Item 8** (`[unattended]`, mechanical, unreleased, no alias): both come before Item 1 Phase B so
   the tests written there target `q2 convert --execution-records`.
2. **Item 1 Phases B to D** wait for the verdict above and for Items 9 and 8. Phase D serializes with Item 2 only
   for the one e2e file in the fork (`quarto-render-and-push.test.ts`).
3. **Items 2, 3, 4, 5** are independent of Item 1. Item 3's boxes marked "blocks" come first among them.
4. **Item 6** (Windows) is independent and needs a Windows machine.
5. **Item 7** is its own epic; nothing under it starts before its review session. Item 1 does not depend on it:
   the exporter reads raw outputs and never calls the presenter.
6. **One embedded-docs refresh** (`cargo xtask build-agents-docs`, then `cargo build --bin q2`) serves Items 4, 8
   and 9 together; do it once after the last docs edit.

## Item 1: R through knitr

**Intended:** a `.qmd` with `engine: knitr` renders once in q2, and its outputs can be pushed into an open
Positron editor and previewed from Positron's cache, as Python and Julia can.

**Phase A: characterized (done 2026-11-07).** See [the note](../research/2026-11-07-r-through-knitr-phase-a.md).
The knitr record has three kinds of cell, all structured and owned by `knitr`; none is a fallback cell. The
content hash is computed in core for every engine from the block text including `#|` lines
(`crates/quarto-core/src/record/ids.rs:3-26`); it equals Positron's for R and Julia cells.

- [x] Dump a knitr record and say, per cell, structured or fallback.
- [x] The content hash question: settled by reading, then by Positron's own native run (identical hashes).
- [x] The raw-record experiment in the fork's e2e harness: knitr (8 chunk kinds), a Julia plot, Python array,
      JSON and multi-line output, an inline span. Playwright; see the note for the table.
- [x] Compare Ark's cache shapes with the knitr cells (the note has the per-chunk table).
- [x] Does Positron's parser read `#|` lines for a Julia cell as for Python and R? Yes: 2 of 2 matched.
- [x] The validity list from Positron's loader (in the note; the q2 conformance mirror was not compared).
- [x] The findings note with the verdict.
- [ ] `[unattended]` Extend the raw-record experiment to the two shapes it did not cover and that the export
      still skips for a stated reason: a **fallback cell** (a `text/x-quarto-markdown` output, from a knitr
      chunk the structured route cannot map, or a LaTeX target) and a **marimo** document. Phase B's "the export no
      longer consults the engine" test needs this answer first.
- [ ] `[Gordon]` The gate: the verdict (Open decisions above).

**Design, import side (agreed 2026-11-07; implemented in Phase C).** Replace the `outputs_for` owner rule
(`jupyter`, `julia`, or `knitr` with token `r`) with one declared fact, **kernel equivalence**: for which languages
is a unit this engine owns ordinary kernel code, so the same text gives the same outputs in a plain Positron
kernel? A trait method on `ExecutionEngine` following `try_claims_language` (`engine/traits.rs:176-184`): an
`Option`, `None` means "would have to load", and **`None` is not admitted** (undeclared engines are excluded).
Jupyter and Julia declare any plain language; knitr declares `r`. The data checks stay on the cell: no fallback
marker, syntactic language. Engines outside today's list are admitted only by declaration, each with a test.
Independent of the one-presenter epic. The Phase A table gives the import-side cost: for R, hash agrees and shape
differs, which RIU-3 already accepted (display only, epic decision 6).

### Phase B: tests first (after the gate and Items 9 and 8)

- [ ] `[unattended]` Export tests for a knitr record: text output, a figure (one `image/png`), a message, a data
      frame, an error, a chunk with options. Each fails on the current skip.
- [ ] `[unattended]` A test that the export no longer consults the engine or language: a cell owned by any
      engine, in any language, appears when the record has outputs for it. Marimo-style `python.marimo` and
      fallback cells are included only if the extended raw-record experiment (above) showed no harm.
- [ ] `[unattended]` Import-side tests, one per engine in the table: a unit takes Positron outputs only when its
      engine declares kernel equivalence for its language; an undeclared engine gets nothing.
- [ ] `[unattended]` A CLI test through the binary: `q2 convert <knitr fixture> --to positron-ipynb -o out.ipynb`
      exits 0 and reports cells greater than 0. Keep the Python and Julia tests green.
- [ ] `[unattended]` Track the removal of the image-representation rule: when the fork's view zone picks one
      representation, delete the rule and its tests (a box here, not a strand).

### Phase C: export

- [ ] `[unattended]` Remove `EngineNotExportable` and, as far as the verdict and the extended experiment allow,
      the inline-unit, extension-language, fallback and duplicate-group skips and their `SkipReason` variants.
      Keep the validity wrapper and the temporary image rule. Do not add `knitr` to `EXPORT_OWNERS`. Update the
      help text and add a dated note to the RIU-5 plan (landed; do not rewrite its history).
- [ ] `[unattended]` The import gate: implement kernel equivalence and delete `EXPORT_OWNERS`, `JUPYTER_ENGINE`,
      `JULIA_ENGINE`, `KNITR_ENGINE` and the `lang == "r"` literal as gate inputs.
- [ ] `[unattended]` Gates on `quarto-core` and `quarto`. Run the real binary on the fixture; read the JSON and
      the ipynb; record the invocation and output here.

### Phase D: in a Positron window

- [x] An R session exists in the dev build: it did in the Phase A run (`POSITRON_R_VER_SEL=4.3.2`, the `r`
      fixture; Playwright).
- [ ] `[unattended]` An e2e test, R version of `quarto-render-and-push.test.ts`: command 2 on an R document,
      outputs appear; command 3 shows them from the cache with no R process started by the preview. The Phase A
      spike (`test/e2e/tests/quarto/q2-raw-record-spike.test.ts`, untracked in the fork, not committed) is a
      starting point. Serialize with Item 2.
- [ ] `[Gordon]` Someone watches it once. Mark each step seen, Playwright, or not run.

**Done when:** an R document shows its knitr outputs in the open editor after command 2, the preview shows them
from the cache, and the findings say what Positron did with each kind of record (the Phase A note does, for the
raw record).

## Item 2: the Julia e2e tests are slow and flaky

**Intended:** the demo's tests pass on a clean run. **Seen:** the Julia push tests passed once, then timed out
at 120 s with 0 outputs on a second full run; cold start measured 20 s, warm 10 to 12 s. The Julia engine is Quarto
1's `julia-engine` extension: a detached server reused by the next render, started with `serve(; timeout = 300)`
(`resources/extension-subtrees/julia-engine/_extensions/julia-engine/quartonotebookrunner.jl:48`), so a gap of
over five minutes means a cold start. `q2 call` has no `julia` subcommand, so `status` and `stop` are not reachable
from q2.

- [x] Timing, cold and warm (CLI, 2026-11-07).
- [x] With a warm server both Julia tests pass. The cause of the 120 s runs (cold or stale server) is likely, not
      proven: a first-ever precompile could explain it, 20 s cold does not.
- [x] The fork change is committed as `53bf9ca105`: `warmJuliaServer` at
      `test/e2e/tests/quarto/quarto-render-and-push.test.ts:17` and `setTimeout(480000)` on the Julia tests
      (checked 2026-11-07: present, tree clean).
- [ ] `[unattended]` Re-run the whole file three times and report pass counts; keep each run's Julia server log
      (copy it before the next run overwrites it).
- [ ] `[unattended]` **Find the EPIPE closer, cheapest experiment first.** The client socket is held by a **Node
      child of q2**, not by q2, so an EPIPE on the server's progress write means that child (or q2 above it) was
      killed. Candidates: the extension's `execFile` timeout
      (`extensions/q2-records-demo/src/exportCommand.ts`), Playwright's test timeout or worker teardown, q2's own
      kill path (`crates/quarto-core/src/engine/ts_process.rs`). Experiment: (1) in a scratch directory run the
      export on the Julia fixture, SIGKILL q2 and its Node child a few seconds in, read the server log (EPIPE there
      means a hung-up client is sufficient); (2) match the EPIPE timestamp in an e2e server log to per-test end
      times (inside a test: a timeout kill; at the end: teardown); (3) look for a second q2 run (the warm helper,
      or an orphan) in flight when a passing test tore down.

## Item 3: loose ends from RIU-6

This is a demo: "ship" means the demo in the fork. Each box is fixed for the demo or written down as a known issue
in the demo's run guide.

- [ ] `[unattended]` **The run guide does not exist.** The RIU-6 plan and the commands findings cite
      `extensions/q2-records-demo/demo/README.md`, but at fork `99aa72fcb4` that file is in neither the commit nor
      the tree (the `demo/` folder holds the three `.qmd` documents only). Write it: the script from the commands
      findings note plus the known limits below. The RIU-6 plan has been corrected.
- [ ] `[unattended]` **Positron's cache location: prototype a Positron feature, idiomatically** (Gordon wants a
      Positron-side prototype here). The extension takes the sibling of `ctx.globalStorageUri`
      (`previewHelpers.ts:31-35`), which is profile-scoped, while the cache service always writes under
      `defaultProfile.globalStorageHome`
      (`src/vs/workbench/contrib/positronQuarto/browser/quartoOutputCacheService.ts:133-136`). First step:
      investigate how Positron exposes workbench-owned paths to extensions (a public `positron.*` API, a
      contributed setting, or an internal command like `_positronQuarto.pushOutputsFromFile`,
      `quartoPushOutputs.ts:29`) and pick the most idiomatic; the service already holds `_cacheDir`. Then tests
      first (a vitest for the extension, and a check with a non-default profile), replace
      `cacheDirFromGlobalStorage`, and make a missing answer an error that names the cause. Done when commands 2
      and 3 work under a non-default profile.
- [x] The Viewer's big X leaves q2 running: document it, not fix it (decided). The extension kills q2 on
      `onDidRequestStop` and `onDidDispose` (`previewCommand.ts:93-94`) and on deactivation (`:113`); closing the
      Viewer evidently does not dispose the panel (attributed to Positron's Viewer).
- [ ] `[unattended]` Write that limitation, and "use the Viewer's Stop", into the run guide.
- [ ] `[unattended]` Write into the run guide: command 2 replaces a matched cell's outputs with no prompt (decided
      2026-11-07); command 2 does not change Positron's kernel state, so a later cell that uses a variable from a
      pushed cell fails; command 1 values are editor decorations, gone on reload, and command 3 cannot show them.
- [ ] `[unattended]` **Stale Julia servers and a missing Plots project** give confusing results (`startpath must
      be non-empty`; an `ArgumentError` exported as an error output that looks like a working run). Write down what
      q2 can say when it sees either. (A related confusion appeared in the Phase A run: a Julia environment printed
      a stream of `SYSTEM: caught exception ... failed Task notice` into a plot cell's stderr; not investigated.)
- [ ] `[unattended]` **Not run in any window:** untitled document, "q2 too old" (exit 2), non-default profile, a
      dirty document. (A wrong `q2.path` was run under Playwright and failed correctly.) The first two can be added
      to the fork's e2e suite.
- [ ] `[either]` **An edited cell loses its syntax highlighting in the q2 preview** (RIU-3a). `q2 render`
      highlights an unexecuted `{python}` cell (checked 2026-11-07: `#| eval: false`, an executed cell and a plain
      fence all get `hl-*` spans), so the gap is probably in how the records-only preview presents an unmatched
      cell. Still to do: a Playwright check against `q2 preview --execution-records <dir>` with an edited cell; file
      a box if it is a bug.
- [ ] `[unattended]` **Command 1 on Julia and R spans.** Command 1 (`positronQuarto.runInlineExpressions`,
      `quartoRunInlineExpressions.ts` in the fork) never calls q2; its span detector takes the document's primary
      kernel language (`quartoInlineExpr.ts:35`) and nothing in `quartoInlineExprExecutor.ts` is
      Python-specific. Only Python was run (42 and 3.0, also under Playwright). Add a Julia and an R document to the
      fork's e2e for command 1.
- [x] The preview banner text ("this page reconnects automatically") is left as is (decided).
- [x] Pushing over existing outputs does not ask (decided).
- [x] Command 1 values do not persist; that is the demo's honest state (read 2026-11-07: editor decorations,
      `quartoRunInlineExpressions.ts:62-82`). See "Eventual goal" below.

## Item 4: does the epic match the tree

- [x] RIU-6 Phase E (2026-11-07): every plan re-read and checked against the tree; statuses, table and exit
      criteria updated. The epic's "Closing reconciliation" no longer says RIU-3b never started (checked); the
      stale Julia lines in the parent epic and `exr-08-julia.md` are fixed (checked: `b4d827186` is cited in both).
      This rewrite found two more stale statements and corrected them: the RIU-3 status line (it still said 3b
      never started) and the RIU-6 plan's claim that the demo README exists.
- [x] The epic's stated intent was re-read against what the three commands do (the review note, "Intent the epic
      states that no checklist covers"); each gap is a box below.
- [x] Stamped cells carry no `data-loc` (follows from decision 14; decided fine).
- [ ] `[unattended]` **Hub verification.** Exit criterion 1 says preview and hub work from records alone,
      "verified through the real binary and a browser". Find where the hub half was verified and cite it, or run
      it (`q2 provide-hub` with the writer gate on, a hub client).
- [ ] `[unattended]` **Embedded-docs refresh:** `cargo xtask build-agents-docs`, `cargo build --bin q2`,
      `target/debug/q2 docs llms --embed-info`, `target/debug/q2 render docs/`. The 3b docs edits made the embed
      stale; Items 8 and 9 stale it again, so do it once at the end.
- [ ] `[either]` **RIU-3 browser items never seen:** `--execution-records positron,engine` running the engine, a
      cache older than the document shown in a browser, `--share --allow-edit` with a guest, "Clear results"
      clicked, a real click or scroll sync.
- [ ] `[unattended]` Decision 12 (a failed optional `record: true` copy fails soft) says "not re-verified against
      the code"; re-verify with the real binary.
- [ ] `[unattended]` The stale doc comment at `crates/quarto-core/src/presenter/options.rs:237` ("read under
      knitr's dotted names only"; the code reads the canonical dashed `fig-cap` for both front ends). Checked still
      stale on 2026-11-07. The one-presenter epic touches this file; fix it in whichever lands first.

## Item 5: RIU-3b (landed 2026-11-07) and the 3a leftovers

3b landed on `feature/riu-3b-stamping` (`3f0c55a6d`): the stamp post-walk in
`crates/quarto-core/src/stage/stages/record_splice.rs`, `jupyter-output` in `ATOMIC_KINDS`
(`ts-packages/preview-renderer/src/utils/sourceInfo.ts`), the stamp tests and the viewer regression test.
Re-execute on an imported page is refused without `engine` and runs it when listed
(`re_execute_on_an_imported_page_is_refused_without_engine_and_runs_it_when_listed`,
`crates/quarto-preview/src/capture_driver.rs:3194`; checked present).

- [ ] `[unattended]` **EXR-16 sweep.** One table-driven test in
      `crates/quarto-core/tests/integration/positron_source.rs` (or a sibling in the same binary) that, for each of
      the 21 `CASES` in `positron_conformance.rs`, builds a Positron-style cache from the case's fixture, imports,
      converts and splices, and asserts which units receive outputs; cases where Positron and q2 diverge
      (`positron-expected.json`) assert the divergence. Today `positron_conformance.rs` tests only the generator and
      the loader; `positron_source.rs` covers three durable fixtures (python, hand-built julia, r).
- [ ] `[unattended]` **Labeled-cell `fig-cap` splice case.** A Python cell with `#| label: fig-x` and
      `#| fig-cap: ...` through import, convert and splice, asserting the output shows on the page.
      `knitr_positron_import.rs:179` and `:261` cover the hash and the record shape only.

## Item 6: the Windows run (deferred by the epic)

Needs a Windows machine (`aws-lc-sys` blocks the msvc cross-check from macOS). Highest risk first. Item 8 deletes
`commands/record.rs` and its CLI test file, so those entries move to the `convert` command and its test.

- [ ] `[Gordon]` `crates/quarto-hub-provider/src/execute.rs:331` and `:527`: `canonicalize()` returns a `\\?\`
      verbatim path on Windows; check the "outside the document directory" comparison.
- [ ] `[Gordon]` `crates/quarto-core/src/positron_cache.rs:150` (`vscode_file_uri`) and the `%APPDATA%` rule: drive
      letter, `%3A` and non-ASCII are unverified.
- [x] The `--source-uri` default (`record.rs:139`): forward slashes, agreed; implemented by Item 8.
- [ ] `[Gordon]` The CLI test file for the export command (`record_export_positron_cli.rs`, replaced by Item 8):
      `python3` at about `:250`, and the literal `/some/dir` at about `:113`.
- [ ] `[Gordon]` `crates/quarto-core/tests/integration/julia_engine_e2e.rs:1075-1098`: `kill` and `pgrep` with no
      `cfg(unix)` gate; confirm they cannot run on Windows with Julia installed.
- [ ] `[Gordon]` `crates/quarto-core/src/record/sources.rs:49,101-107,179-199` and
      `crates/quarto-preview/src/capture_driver.rs:2932-2934`: native-separator display and a self-consistent URI
      test.
- [ ] `[Gordon]` CRLF: `positron-conformance/crlf.qmd` and `positron-cache/*.ipynb` under `autocrlf`.
- [ ] `[Gordon]` Then the epic's command, once: `cargo clippy -p quarto -p quarto-core -p quarto-preview
      -p quarto-hub -p quarto-hub-provider -p quarto-ast-reconcile --all-targets -- -D warnings` and
      `cargo nextest run` for the same crates.

## Item 7: one presenter path

Decided (Gordon, 2026-11-07): its own epic, [one presenter path](2026-11-07-one-presenter-path-epic.md). The five
spikes are done; the epic holds what they found. Nothing under it starts before its review session. Item 1 does
not depend on it.

- [x] `[Gordon]` Decide whether to make this a plan: yes, an epic.
- [x] The pre-review spikes ([results](../research/2026-11-07-one-presenter-spikes.md)).

## Item 8: `q2 convert --to positron-ipynb` replaces `q2 record export-positron`

**Intended (Gordon, 2026-11-07):** remove `q2 record`, which is too specific, and use `q2 convert`, as Q1 had it,
with the output format explicit. Q1 chose its direction from the input's extension
(`external-sources/quarto-cli/src/command/convert/cmd.ts`) and wrote beside the input, overwriting silently.
q2's `convert` is a stub (`crates/quarto/src/commands/convert.rs` returns `NotImplemented`; its `--output` flag is
documented as a format, unlike Q1's `-o` path). `q2 record` is unreleased (it landed in `d6954ccdf`), so removing
it costs nothing. The ipynb-to-qmd and plain qmd-to-ipynb sides are **not** part of this plan: strand
`bd-ju7u8a73`.

**Design (agreed 2026-11-07):**

```
q2 convert <input> --to <format> [--from <format>] [-o <path>] [target options]
```

- `--to` is required and never inferred. `--from` is optional, inferred from the extension (`.qmd` and `.md` are
  qmd, `.ipynb` is ipynb) and an error otherwise. An unsupported pair is an error that lists the supported pairs.
  Names and flags match `q2 render` (`--to`, `-o`).
- Supported here: `--from qmd --to positron-ipynb`. Reserved by name, not implemented: `--to ipynb`,
  `--from ipynb`.
- Options for `positron-ipynb` only, rejected for other targets: `--execution-records q2,engine` (the preview and
  provide-hub grammar; only those two values; default `q2,engine`; `q2` alone forbids running the document) and
  `--source-uri`.
- `-o <path>` is required for `positron-ipynb`, because the file name belongs to the consumer (a hash of the
  document URI, computed by the extension). `-o -` writes the notebook to stdout and the JSON report to stderr.
  Otherwise the report is the one JSON line on stdout, as today.
- An existing output file is overwritten, with an info diagnostic naming it (Q1 overwrote silently).
- Unchanged: `export_for_positron` in `quarto-core`, the report fields, exit codes 10 to 13 (documented under the
  target), exit 2 for usage errors (so the extension's "q2 too old" detection still works: an older `convert` has
  no `--to`). A `convert` that may run code says so in its help text.
- Not `-M`: `-M` is document metadata in `q2 render`, `preview` and `provide-hub`.
- The `--source-uri` default is the document's path with forward slashes (`quarto_util::to_forward_slashes`).
  Today it is the native path (`commands/record.rs:138-139`). `normalize_location` in `positron_cache.rs` accepts
  both forms and Positron ignores the field on load. A test builds its path with `Path::join`.

**Order:** after Item 9, before Item 1 Phase B.

- [ ] `[unattended]` Tests first, through the binary (`crates/quarto/tests/integration/`, replacing
      `record_export_positron_cli.rs`): `--to` missing is exit 2; an unsupported pair lists the supported ones;
      `--from` inferred from `.qmd`; an unknown extension errors; `--execution-records` values other than `q2` and
      `engine` are rejected; `--execution-records` and `--source-uri` are rejected for a target that does not take
      them; `-o -` puts the notebook on stdout and the report on stderr; an existing file is overwritten and an
      info line names it; exit codes 10 to 13 keep their meanings. Move, do not weaken, the existing cases.
- [ ] `[unattended]` Implement `commands/convert.rs` (replacing the stub), wire `Commands::Convert` in `main.rs`
      with `--to`, `--from`, `-o/--output` (a path), `--execution-records`, `--source-uri`; delete
      `commands/record.rs`, the `Record` subcommand and `record_export_positron_cli.rs`. Gates:
      `cargo clippy -p quarto --all-targets -- -D warnings` and `cargo nextest run -p quarto`.
- [ ] `[unattended]` Run the real binary on a Python fixture and the knitr fixture
      (`q2 convert d.qmd --to positron-ipynb -o out.ipynb --source-uri file:///x/d.qmd`); read the JSON and the
      ipynb; record the invocation and output here.
- [ ] `[unattended]` Update the fork's extension: `q2Contract.ts:34-38` (`buildExportArgs`: `convert`, `--to
      positron-ipynb`, `-o`), its vitest, the comment at line 7, and `exportCommand.ts`. Keep the "too old"
      detection and add a vitest for it. The fork's e2e (`warmJuliaServer` at line 17 of
      `quarto-render-and-push.test.ts`) also calls `record export-positron` and needs the same change.
- [ ] `[unattended]` User-facing docs: a `docs/` page for `q2 convert` (usage only). Add dated notes, not
      rewrites, to the RIU-5 and RIU-6 plans' command references. The docs refresh is in the Order section.
- [ ] `[unattended]` Windows: the forward-slash default is the only path logic here; its test is portable.

## Item 9: rename `--records` to `--execution-records`

> **Update 2026-11-08 (Gordon):** this item is to be folded into a new consolidation plan in the
> [one-presenter epic](2026-11-07-one-presenter-path-epic.md): one shared definition of the flag's grammar and
> handling for `preview`, `provide-hub`, `convert` and the new `q2 render --execution-records`. The fold depends
> on the research into whether the source list could be a metadata key instead of a flag
> ([decisions note](../research/2026-11-08-one-presenter-review-decisions.md), R5). Until the epic's plan list is
> written, the boxes below stay as they are.

**Intended (Gordon, 2026-11-07):** the flag that names where a document's execution record comes from is spelled
`--execution-records`. **Unreleased, so no alias and no deprecation period:** neither `v0.42.0` nor `main` has the
flag (`git grep -e '--records' v0.42.0 -- crates/quarto/src/main.rs docs`: no matches). The grammar is unchanged: a
comma list or repeated flag; entries `cache`, `q2`, `positron`, `engine`, or a directory path; defaults per
command; `cache` refused for `provide-hub` and `convert`; `convert` accepts only `q2` and `engine`. The setting
`preview.engine` is a different name and does not change. At `8cfec0fc1`, `git grep -c -e '--records'
crates/quarto/src/main.rs` finds 9 lines.

**Where it appears** (found 2026-11-07 with `git grep -e '--records'`):

- CLI definitions, `crates/quarto/src/main.rs`: the `records` fields at about lines 63 (`record export-positron`,
  removed by Item 8), 352 (`preview`) and 809 (`provide-hub`), the parse tests (about 1408-1430, 1583, 1609) and
  doc comments at 311 and 1574. Renaming the field to `execution_records` renames the long flag.
- Commands: `crates/quarto/src/commands/preview.rs` (4 places), `provide_hub.rs` (2), `record.rs` (2, deleted by
  Item 8).
- Messages and their tests: `crates/quarto-core/src/engine/diagnostics.rs` (6), `record/sources.rs` (3, including
  the `cache` refusal text), `crates/quarto-hub-provider/src/execute.rs` (4),
  `crates/quarto-preview/src/re_execute.rs` (2), `capture_driver.rs` (2), `lib.rs`, `share.rs` and two
  integration tests.
- Docs: `docs/guides/authoring/execution-records.qmd` (4) and `docs/guides/projects/preview.qmd` (4).
- TypeScript: `q2-preview-spa/e2e/helpers/previewServer.ts` (3) and `q2-preview-spa/e2e/records-sources.spec.ts`
  (7).
- The fork's extension: `q2Contract.ts` (`buildPreviewArgs`, line 54) and `previewCommand.ts`, with vitests.
- Notes: 19 files under `claude-notes/` mention the old flag. They are history; the epic gets one dated line.

**Order:** before Item 8's implementation.

- [ ] `[unattended]` Tests first: change the parse tests in `main.rs` and the assertions in `diagnostics.rs`,
      `re_execute.rs`, `capture_driver.rs` and the share and hub-provider integration tests to the new spelling;
      add a test that the old spelling is rejected as an unknown flag on `preview` and `provide-hub`. They fail
      first on the current name.
- [ ] `[unattended]` Rename the flag and every message, comment and doc string above. Gates:
      `cargo clippy -p quarto -p quarto-core -p quarto-preview -p quarto-hub-provider --all-targets -- -D warnings`
      and `cargo nextest run` for the same crates.
- [ ] `[unattended]` Update `q2-preview-spa/e2e` (two files); run the records-sources e2e if it can run here and
      say which ran.
- [ ] `[unattended]` Update the two docs pages and run `target/debug/q2 render docs/`. (The embedded-docs refresh
      is in the Order section.)
- [ ] `[unattended]` Update the fork extension and its vitests.
- [ ] `[unattended]` A final `git grep -e '--records'` outside `claude-notes/` comes back empty; add the dated
      rename line to the epic.
- [ ] `[unattended]` Run the real binary: `q2 preview <doc> --execution-records positron,engine --no-browser`
      starts as before, and `q2 preview <doc> --records positron` fails as an unknown argument. Record the
      invocations and output here.

## Eventual goal: span outputs in execution records (not demo work)

Command 1 runs inline expressions out of band and shows the value as an editor decoration, much as Q1 handles
span execution. The eventual goal is that a span's output is a first-class record cell: Positron would persist it
in the cache (a Positron-side feature to propose, not to build here) and q2 would export and import it as an
inline unit, the way cells round-trip. Phase A found that Positron skips an inline unit by itself when pushed, so
the export's inline-unit skip is already unnecessary for Positron. Start from q2's inline units and span hash
(`ids.rs:31-36`), the import and export skip rules for inline units, and epic decision 6 (a Positron record is
display-only).
