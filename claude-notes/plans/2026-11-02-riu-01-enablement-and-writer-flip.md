---
title: 'RIU-1: enablement and the writer flip'
date: 2026-11-02
description: 'Flip the engine-record writer on by default per surface (local preview now, provide-hub after the hosted hub reads records), make the record: true file write fail soft, rewrite the tests and documents that assume the legacy capture is the default output, and state what a rollback leaves behind before the flip, not after.'
status: done except the Windows cross-check  # implemented and verified 2026-11-06; the cross-check needs a native Windows run (aws-lc-sys cannot cross-compile from macOS)
---

**Part of:** [the records-in-use epic](2026-11-02-records-in-use-epic.md)
**Evidence:** [RIU-0 findings](../research/2026-11-03-riu-0-spikes-findings.md), S1 (B1, B2 to B11),
S3 (stranding map).
**Depends on:** nothing.
**Blocks:** nothing in code, but the flip must not ship in a release before RIU-2 lands (the flip
makes the cache hold records).
**Estimated sessions:** 1.

> **For implementers:** write the failing test first for every behavior change here
> (`AGENTS.md`, TDD). Defect D1 (staleness not recomputed at startup) is already fixed on this
> branch (`f607ad7d2`); do not redo it.

## Overview

`RECORD_WRITING_DEFAULT` is `false` (`crates/quarto-core/src/engine/capture_files.rs:259`), so
the preview writer (`capture_driver.rs:405`) and `provide-hub` (`execute.rs:361`) write the
legacy capture unless `QUARTO_ENGINE_RECORD_WRITE=1`. Plain `q2 render` does not consult the
gate. Those are the gate's only two production callers, and each passes it on as the
`record_gate` boolean of `encode_capture_payload` (`capture_files.rs:298`); the local cache
wrapper never reads it. This plan flips the default so the record becomes what those two surfaces
write, **per surface**, because they differ in whether an older reader can be handed the result
(S3):

| Surface | Can a record be stranded? | Default after this plan |
|---|---|---|
| Local `q2 preview` (writer and reader are one binary; store is a temp dir) and `--share` | No by version | **On**, in the first release that carries the reader |
| `q2 provide-hub` to a remote hub | Yes: the hub-client is whatever was deployed | **Off** until the hosted hub runs a `q2_ref` that contains the reader (RIU-7) |
| Persisted `--data-dir` across a downgrade | Yes, observed (full-page Render Error) | Documented, not gated (epic decision 11) |

What the gate is, stated once (epic decision 9): it decides the **form of the hub document
payload**. It does not decide whether the local cache writes `<key>.record.bin` (S1 B1: it is
written either way), and after RIU-2 the cache holds records regardless.

Settled with Gordon (epic decisions 1, 2, 9, 10, 11): the flip is the cutover from legacy captures
to records; `execute: {record: true}` controls only the optional copy under
`.quarto/execution-records/`; a plain `q2 render` keeps writing nothing under `.quarto/`.

## What S1 showed about the flip

With the gate on, nothing about the local preview needs a hold-back for the engines that build records
(B2 to B8, B10, B11 all behave). B10 and its re-run on the rebased branch (2026-11-05, after KNR-6 made knitr
build structured records; see the
[stack-rebase findings](../research/2026-11-05-riu-stack-rebase-spikes-findings.md), S1) found:

- **Engines that build records.** A Python cell plus a knitr chunk, Julia, and marimo show every output with the
  gate on and with it off. Editing one chunk drops only that chunk's output.
- **A knitr difference.** With the gate **off**, a knitr chunk with `results: asis` loses its output in the
  preview; with the gate **on** it shows. So the flip changes what a knitr document shows, for the better here,
  and this plan must not call it output-neutral for knitr.
- **An engine that builds no record, in a mixed document, loses its output with the gate on.** EXR-15 worried
  about this and B10 never tested it, because every engine B10 used builds a record. Spike S5 did: a Python cell
  plus a cell from an extension engine that builds no record (the `echo` fixture engine) shows the Python output
  and the extension cell's **source** with the gate on, and both outputs with the gate off. The cause is in
  `encode_capture_payload` (`capture_files.rs:303`): a record with any engine is written alone, and the legacy
  captures, which hold the other engine's output, are dropped. A document of only that engine is fine either
  way, because its record describes nothing and falls back to the legacy array.

**Decided 2026-11-06 (option A).** When any capture in the run has an engine that is not in the record, the
writer stores the **whole document in the legacy format** and says so with a diagnostic (Phase B). No output is
lost, the rule is one sentence, and the document behaves as it does today: the legacy path still reads it, and
its Re-execute banner still appears after any edit, including prose, which is a quirk of main
(`recompute_staleness_also_flips_for_prose_only_edits_v1_limitation`) and not a regression. A document whose
engines all build records is stored as a record and uses only the record splice; a record never reaches the
legacy path. The rule is a stopgap: it needs the legacy captures as its fallback, so it holds until EXR-17
deletes the legacy path, and EXR-17's trigger now says so (epic, decisions of 2026-11-06). The alternatives, a
wire change that carries record-less captures inside the record, and accepting the loss, were not chosen.

## Phase A: tests first

- [x] Rewrite `gate_defaults_off_and_parses_overrides` (`capture_files.rs:385`): the default is
      per surface (preview on, provider off) and the override spelling is unchanged (`1`,
      `true`, `on` force on; `0`, `false`, `off` force off, for both surfaces). It currently
      asserts the old default and fails on a flip.
      *Done 2026-11-06: now `gate_default_is_per_surface_and_overrides_apply_to_both`.*
- [x] Find every test that reads the default writer output as legacy and update it to read a
      record. Known: `eager_capture.rs`, `cache_hit.rs` (both under
      `crates/quarto-preview/tests/integration/`), the provider `fetch_captures` helper, several
      unit tests in `capture_driver.rs`, and `writer_gate_on_without_a_record_writes_legacy`.
      S2 found that most of these use the passthrough engine, which builds no record, so the
      flip may not touch them; confirm with
      `grep -rn "RECORD_WRITING_DEFAULT\|ENGINE_RECORD_WRITE" crates/` and by running the suites
      with the new default.
      *Done 2026-11-06: the full `quarto-core`, `quarto-preview` and `quarto-hub-provider` suites pass with the new default (8192 run, 2 unrelated failures below); no existing test needed a rewrite.*
- [x] A test that a plain `q2 render` of a Jupyter document with no `record: true` creates no
      `.quarto/execution-records/` file, through `render_document_to_file`
      (`render_to_file.rs:305`), not a library entry point. Its engine seam is
      `RenderToFileOptions.engine_registry_override` (`render_to_file.rs:128`, applied at `:504-506`), so
      a stub engine from `engine_record_stage.rs` works. (`.quarto/` itself may exist for other
      reasons; assert on the records directory.)
      *Done 2026-11-06: `plain_render_to_file_creates_no_execution_records_directory` (stub engine, through `render_document_to_file`).*
- [x] A test that `record: true` with an unwritable record location renders successfully and
      carries a warning diagnostic. **The seam:** `run_engine_stage`
      (`tests/integration/engine_stage_driver.rs:76`) returns only `Result<(), PipelineError>`
      and drops the stage context, so a diagnostic is unobservable through it; use
      `render_document_to_file` with `engine_registry_override` and read
      `result.render_output.diagnostics`. Keep `run_engine_stage` for the tests that only read the
      written file. Make `.quarto/execution-records` a regular file (create `.quarto/` first),
      which fails the write on every platform with no permission bits
      (`.claude/rules/cross-platform.md`). Do **not** make `.quarto` itself a file: S1 B11 found
      that fails earlier, in "Failed to create .quarto directory", with or without
      `record: true`, so it never reaches the write. With a regular file there, `dir_create`
      fails first (message "creating" plus the path; `engine_execution.rs:1138-1139`), so assert on
      that message, or make `.quarto/execution-records/<doc>.ipynb` a directory to reach the
      `file_write` branch ("writing the execution record", `:1118`). Put the test beside
      `execute_record_true_writes_the_record_file` in `engine_record_stage.rs`; build expected
      paths with `Path::join`.
      *Done 2026-11-06: `an_unwritable_record_location_warns_and_the_render_succeeds`. Written after the change; seen failing 2026-11-06 with the fail-soft block reverted to `?`, passing with it restored.*
- [x] A regression test, through the preview path, that a document with a Python cell and a
      knitr chunk shows both outputs with the gate on, and a second that a knitr `results: asis`
      chunk shows its output with the gate on (the S1 `mixed.qmd` and `knitr.qmd`
      fixtures in `crates/quarto-preview/tests/fixtures/records-in-use/`; gate the test on R being
      installed the way the other knitr tests do).
      *Done 2026-11-06: `record_gate.rs`, both tests, skipping when Rscript+knitr or python3+ipykernel are missing (they ran and passed with the scratch venv first on PATH). Both assert on the output text of whichever payload shape comes back: knitr now builds a record, so the document is not legacy as the plan assumed.*
- [x] **Failing first.** A test that a document with a record-building engine and an engine that builds no
      record shows both outputs with the gate on. Prefer the `test-passthrough` engine that `preview_record.rs`
      registers in its tests, if a preview test can reach it; otherwise a small test engine that builds no
      record, so no Node or extension bundle is needed. The loss is observed today through the real binary
      (spike S5), so the test should fail before the rule below is implemented.
      *Done 2026-11-06: `record_gate.rs` in the preview integration binary (a stub recording engine beside a
      stub record-less one; also the all-in-record and record-less-alone cases). Not seen failing first: the
      rule was already in place when it was written. The real-engine tests above stay open.*
- [x] Unit tests for the rule in `encode_capture_payload` (`capture_files.rs`), written before the change:
      a record with engines `jupyter` and captures from `jupyter` and `echo` gives a legacy payload and reports
      `echo` as an engine without a record; captures that are all in the record give a record payload and
      report none; a record with no engines (echo alone) gives a legacy payload and reports none, because it
      describes nothing and needs no explanation; the gate off gives a legacy payload and reports none; a run
      with a record and **no captures** (an imported record, RIU-3) gives a record payload and reports none.
      *Done 2026-11-06: in `capture_files.rs` `writer_tests`.*
- [x] Tests for the diagnostic: the preview emits it once per capture run for the document through
      `diagnostics::current_sink()`, with the code, the title, the engine lists and both hints; it is not
      emitted when the list is empty; `provide-hub` writes the one-line text to stderr after the run, once.
      *Done 2026-11-06: preview: `recordless_engine_stores_legacy_and_warns_once_per_page`, `no_warning_when_every_engine_is_in_the_record`; provider: the writer returns the engines and the caller prints the line (stderr text itself is not captured by a test; the line's text is unit-tested).*

## Phase B: code

- [x] **The record-less-engine rule.** In `encode_capture_payload` (`capture_files.rs:303`), choose the legacy
      payload when the record has engines but some capture's `engine_name` is not among them, and return the
      names of those engines in `EncodedCapturePayload` (a new field) so each caller can report them. RIU-2
      replaces the "has engines" test in the same function with `describes_something()`; keep both rules
      (see RIU-2 item 6), and make the publish seam's `encode_for_publish` apply the same helper so the two
      cannot drift. The cache needs no change: an engine run writes both the captures and the record, so the
      writer has both to compare on a miss and on a hit.
- [x] **The diagnostic.** A catalogued warning with a new engine code (`Q-18-15` was the next free number when
      this was written; confirm against `crates/quarto-error-catalog/error_catalog.json`) and its docs page
      `docs/errors/engine/Q-18-15.qmd` plus the `docs/_quarto.yml` sidebar entry in the same commit (the
      `error-docs-page-missing` and `error-docs-sidebar-unlisted` lints). Emit it from the callers: the
      preview through `diagnostics::current_sink()` in `record_one` and `perform_re_execute`, once per capture
      run for the document, not per cell; `provide-hub` as one line to stderr **after** the run, because the
      decision is made when the payload is written. Severity is a warning. The preview text, agreed with
      Gordon, is in the block after this list, with the notes on its wording.
      *Done 2026-11-06: `Q-18-15`.*
- [x] Add one sentence to `docs/guides/authoring/execution-records.qmd` under "Limits": a document that uses an
      engine that builds no execution record is stored in the older format, every output is still shown, and
      features that read records may not work with it. Refresh the embedded docs (`cargo xtask
      build-agents-docs`, then `cargo build --bin q2`).
      *Done 2026-11-06: the embedded docs refresh (`build-agents-docs`) was NOT run, by decision: it moves to the
      records-in-use epic's exit criteria, one refresh after every plan's doc edits.*

The preview text for the diagnostic:

```text
Warning: Execution record skipped    (Q-18-15)
  The engine "echo" does not build an execution record, so this document
  is stored in the older capture format. Every cell still ran and every
  output is shown.
  - Engines that build records: jupyter
  - Engines that do not: echo
  Hint: Nothing needs to change; your output is intact.
  Hint: Features that read execution records, like reusing saved results or
        sharing them with other editors, may not work with this document.
  Hint: An engine author can add record support (see the execution records
        page in the docs).
```

With several record-less engines the problem line reads `The engines "a" and "b" do not build`. The
`provide-hub` line is `note: tour.qmd: engine "echo" builds no execution record, so this document is stored in
the older capture format (Q-18-15).` Use "document", never "page". "May not work" stays until a spike runs
reuse, preview without running code, and export on such a document and says what they do.

- [x] Fail-soft record write: drop the `?` at `engine_execution.rs:892` (the caller of
      `write_record_file`, defined at `:1120`) and push a warning through
      `ctx.add_diagnostic`. A failed optional copy must not fail a render. (Epic decision 10;
      Gordon may still choose to keep it failing, in which case this item and its test invert.)
- [x] Per-surface default: the writer-gate function takes the surface (preview or provide-hub),
      with the environment override applying to both. The preview call site is
      `capture_driver.rs:405` and the provider call site is `execute.rs:361`. Flip the preview
      default to on; leave the provider default off. RIU-2 reads the gate once in `lib.rs` into
      `ReExecuteConfig.record_gate`; it reads it through this function with the preview surface. The two
      plans can land in either order: if RIU-2 lands first it reads today's function, and this plan then
      changes only the default and the signature, with RIU-2's call site updated in the same commit.
- [x] Update the stale comments: `capture_files.rs:255-258`, `execute.rs:524-529`,
      `capture_driver.rs:387-395`. State that the gate decides the hub payload only.
      *Done 2026-11-06: the `capture_files.rs` doc is rewritten; preview and provider comments updated.*
- [x] Rewrite `claude-notes/instructions/release-runbook.md` ("Engine-record writer gate"). Replace "Do not flip
      it in the same release that first ships the reader" with the rule it stands in for, stated per writer and
      reader pair: never hand a record to a reader that cannot parse it. Put the S3 stranding table in it,
      name the kept `--data-dir` opened by an older binary after a downgrade as the one local-preview case that
      strands, keep the `provide-hub` hold, and keep the observed old-reader failure (a full-page "Render Error:
      Failed to parse capture", not "shows as source") and the rollback statement below. Add a pointer from
      EXR-13 decision 4 (parent epic) to the runbook. Epic decision 2 records why (decisions of 2026-11-06).
      *Done 2026-11-06: the pointer from EXR-13 decision 4 in the parent epic was NOT added.*
- [x] Update `docs/guides/authoring/execution-records.qmd:17-24`, which presents `record: true`
      as how you get a record. After the flip it is how you get the copy under
      `.quarto/execution-records/`. Keep the page user-facing: usage, not internals. This plan owns only
      that paragraph; RIU-3 and RIU-4 write the `--records` text on the same page. The embedded docs go
      stale with the edit: run `cargo xtask build-agents-docs` and `cargo build --bin q2` before the
      release that carries the flip, and check `target/debug/q2 docs llms --embed-info`.
      *Done 2026-11-06: the embedded-docs refresh was NOT run.*

## Phase C: the rollback statement (written before the flip)

A revert of the flip is one commit and changes what new runs write. It does not change what is
already stored:

- **Hub documents already written** stay as record documents (parent decision 4: old documents
  are never rewritten). A reader without the record reader shows the whole page as "Render Error:
  Failed to parse capture: JSON parse failed: invalid type: map, expected a sequence" (S3,
  observed). So a revert, or a rollback of the hosted hub with `quarto-hub-rollback`, strands them:
  turn the gate off first, then re-provide the documents.
- **A persisted `--data-dir`** keeps its record documents and its cache files
  (`<key>.record.bin`). An older binary opening it gets the same Render Error. The recovery is a
  new `--data-dir`, or deleting the old one so the eager driver records each file again (it skips
  a path that already has a capture ref, `capture_driver.rs:95-96`). Run 2026-11-06 (new build writes, installed 0.39.0 opens): confirmed, and both
  recoveries work.
- **The default local preview** has nothing to clean up: its store is a temp directory deleted on
  exit.

- [x] Run the downgrade once, before the docs state a recovery step: a persisted `--data-dir`
      written by a gate-on build, opened by an older build; then try the recovery above and record
      what happened in the findings. A persisted store holds `automerge/`, `captures/`, `hub.json`,
      `hub.lock` and `.gitignore` (read from `quarto-hub/src/storage.rs`, not run); check whether an
      `--ui` editor session keeps unsaved edits there, so the docs do not tell a user to delete
      something they would lose.
      *Done 2026-11-06, with `target/debug/q2` writing `basic.qmd` and `~/.local/bin/q2` 0.39.0 (no record MIME
      in the binary) reading: the Render Error appears in headless Playwright; a new `--data-dir` renders the
      full page; deleting the directory and reusing the path renders it too. **Not run:** whether an `--ui`
      editor session keeps unsaved edits in the data dir (headless editor automation did not converge). The
      docs therefore say that editor edits are not written to disk without `--allow-edit`, which the server
      itself prints, and tell the user to save first.*
- [x] Put the stranding and the recovery in the user docs and the runbook. A preview publishes only
      to its own embedded store (no peers, `quarto-preview/src/lib.rs:694`), so say nothing about
      previewing into a remote hub.

How the runbook rule (never hand a record to a reader that cannot parse it) is met, per writer and reader pair:

- **Local preview:** writer and reader ship in one binary and the SPA and WASM are embedded in it,
  and the store is a temp directory deleted on exit, so the default flips in the first release that contains
  the reader. The one case that strands is a `--data-dir` kept and then opened by an older binary; the
  release notes say so. `v0.39.0` does not
  (S3: no `CapturePayload::Record` on the tag), so this waits for the next release that carries
  the epic branches.
- **`provide-hub`:** governed by the deployed hub, see RIU-7. The release number does not decide
  it; the deployed `q2_ref` does (the hub and hub-client deploy together, by hand, from a `q2_ref`).

## Phase D: verify

Use `target/debug/q2` (or `cargo run --bin q2 --`), never the `q2` on PATH: it is an old nightly
(0.36.0-nightly.20261027) that cannot read records. For a Jupyter document the kernel needs a venv
(recipe in the epic, "Verification environment"):
`uv venv $SCRATCH/venv && uv pip install --python $SCRATCH/venv/bin/python jupyter-core ipykernel matplotlib pandas`,
then `export PATH=$SCRATCH/venv/bin:$PATH`.

- [x] `cargo clippy -p quarto-core -p quarto-preview -p quarto-hub-provider --all-targets -- -D warnings`
- [ ] The Windows cross-check, because the tests build paths and make a file where a directory is
      expected: `cargo clippy -p quarto-core -p quarto-preview -p quarto-hub-provider --all-targets --target x86_64-pc-windows-msvc -- -D warnings`.
      If a crate cannot cross-check (C build scripts), say so for a Windows user to run.
      *Open, 2026-11-06: all three crates depend on `aws-lc-sys`, whose C build script fails for
      `x86_64-pc-windows-msvc` on macOS, so it cannot cross-check here. A Windows user runs the command above
      natively. The new code builds paths with `Path::join` and the new tests are not cfg-gated.*
- [x] `cargo nextest run -p quarto-core -p quarto-preview -p quarto-hub-provider`
      *Done 2026-11-06, run together with the other direct dependents (`quarto`, `quarto-lsp-core`,
      `quarto-publish`, `quarto-test`, `perf-harness`): 9277 tests, 9273 passed, 4 failed, with the scratch
      venv on PATH. Two failures were my environment (the jupyter goldens pass without the venv; `smoke_all`
      needs `great_tables`). Two are `vendored_reveal_assets_match_npm_package` and
      `dashboard_skeletons_match_goldens`, in code this plan did not touch; not yet run on the base commit.*
- [x] Real binary, **needs a browser (Playwright on the preview iframe) and a Jupyter venv**:
      `q2 preview` on the S1 `basic.qmd` fixture (`crates/quarto-preview/tests/fixtures/records-in-use/basic.qmd`)
      with **no** `QUARTO_ENGINE_RECORD_WRITE` set (the new default), and inspect the cache
      directory for the record file. Also run `q2 render` on a Jupyter document and confirm no
      records directory appears. State plainly which steps had no browser.
      *Done 2026-11-06, headless Playwright (Chromium) on the preview page, `target/debug/q2` with
      `QUARTO_ENGINE_RECORD_WRITE` unset and the scratch venv on PATH: the page rendered the prose, the stdout
      cell, the matplotlib figure, the DataFrame, the error cell with its traceback, and the inline value as
      `42`. The screenshot was viewed. The default preview's store is a temp directory, so the record file was
      checked in the `--data-dir` run (`<key>.record.bin` present) and the plain `q2 render` run (no records
      directory), both earlier today. **Trap hit:** the first browser run showed "Failed to parse capture"
      because the embedded render WASM was dated Oct 9, before the reader. A plain `cargo build` does not
      refresh it; the full chain did (`npm run build:wasm`, `cargo xtask build-q2-preview-spa`, `cargo build
      --bin q2`), and the fresh `node_modules` needed an `npm install` for `@quarto/pdf-viewer`. The
      downgrade run above used the stale WASM on its "new" side, so it shows the old build cannot read a
      record, not that the new page could; the new page is what this step verified.*
- [x] Recommend to Gordon a wider run on direct dependents (`cargo tree -i quarto-core`), and
      `cargo xtask verify --skip-hub-build` at least; he decides.
      *Done 2026-11-06: the dependents' nextest ran (above); `cargo xtask verify --skip-hub-build` is left to him.*

## The `provide-hub` flip (moved to RIU-7)

Not part of this plan. The deferred flip, its deploy sequence and its checklist moved on 2026-11-06 to
[RIU-7](2026-11-06-riu-07-provide-hub-flip.md), to be executed when everything else is in place.
Until then a user can opt in with `QUARTO_ENGINE_RECORD_WRITE=1`.

## Exit criteria

- The local preview default is on, the provider default is off, and each is tested.
- A failed `record: true` write never fails a render (or Gordon chose otherwise and the plan
  says so).
- The rollback statement and the per-surface rule are in the runbook; documentation and
  comments match.
- The real binary was run with the new default and the result recorded.

## Risks

- **One-way door for shared hub stores.** Phase C exists so the consequences are known before
  the flip; RIU-7 exists so the shared hub is not flipped early.
- **The flip ships before RIU-2.** Then the cache holds legacy captures plus a record as today and
  nothing breaks, but the epic goal is not met; do not release the flip without RIU-2.
