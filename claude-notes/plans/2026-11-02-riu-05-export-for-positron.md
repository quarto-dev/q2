---
title: 'RIU-5: export for Positron'
date: 2026-11-02
description: 'A Rust export_for_positron function that turns a q2 execution record into a cache file Positron accepts and displays: the S7 filter (a Jupyter Python record already loads unchanged), extended to Julia cells owned by the julia engine and with a rule that sends Positron one representation of an image instead of four, checked against the conformance mirror and real fixtures; and the q2 record export-positron subcommand that exposes it with defined exit codes, so the TypeScript commands do not reimplement it.'
status: ready  # nothing started; written from RIU-0 S7 and RIU-0b spike J, checked against the tree on 2026-11-03; the representation rule is designed from a window run that showed the problem, not yet run in the window
---

**Part of:** [the records-in-use epic](2026-11-02-records-in-use-epic.md)
**Evidence:** [RIU-0 findings](../research/2026-11-03-riu-0-spikes-findings.md), S7 (what Positron
shows), S5 (the push command that was used to see it); [RIU-0b findings](../research/2026-11-03-riu-0b-findings.md),
J1 to J8 (Julia).
**Depends on:** RIU-4 (the `--records` parsing the subcommand uses) and RIU-2 (the source list, and the
single path function for the `.quarto` copy that its Phase E exposes; the `q2` source reads through it
rather than recomputing the path). The
function itself needs neither and can be built and tested first.
**Blocks:** RIU-6 command 2.
**Estimated sessions:** 1 to 2.

> **For implementers:** tests first. The S7 spike is `spike/riu-s7-export` (commit `9156265db`,
> never merged): `record/positron_export.rs` and `examples/riu_s7_export.rs`. The spike J branch
> (`spike/riu-s8-julia`, commit `2fbd63449`, never merged) adds the `julia` engine to its keep rule
> and an `riu_s8_check` example. Port them, do not cherry-pick them.

**Lanes:** before starting a phase or landing work, read the epic's [lanes, landing and the ready board](2026-11-02-records-in-use-epic.md#lanes-landing-and-the-ready-board): lane branches, rebase and landing rules, and the board of which phases may start.

## Overview

The reverse of RIU-3: q2 has a record, and Positron needs a file it accepts. Positron deletes
invalid cache files (the conformance mirror's rule; not seen in a window), and rewrites a cache
file keeping only `quarto_cell_id`, `quarto_content_hash` and `quarto_label` per cell. The function
returns bytes; it never writes into the Positron directory (the extension does, RIU-6).

## What S7 and spike J showed

Two real Jupyter Python records (rendered through q2 with `record: true`) were pushed into open
documents in the Positron dev build with the S5 command. Everything displayed correctly: a stream, a
matplotlib `image/png`, a pandas `text/html` table, an error with its traceback and colors, a
labeled cell, and a pair of identical cells. The **record itself, untransformed**, displayed the
same as the export (5 cells, 4 matched and 1 skipped: the inline span; the export gave 4 and 0).
Positron ignores `quarto_source` on load. So for Jupyter Python the export is a filter.

Spike J added Julia (Gordon at the window, J7 and J8):

- A Julia cell is owned by the `julia` engine, not `jupyter` (J1, J2), so the S7 keep rule skipped
  every Julia cell (J4).
- Julia text, vector and HTML outputs matched by content hash, displayed and persisted (J7).
- A Julia plot arrives as **one bundle with four representations**: `application/pdf`, `image/png`,
  `image/svg+xml` and `text/html` (a data-URI `img`), and no `text/plain` (J5). Positron renders every
  item of a bundle, dropping only `text/plain` (`quartoOutputViewZone.ts:2366-2384`), and renders an
  unknown string MIME as text (`:2419`), so the window showed two plots and the base64 of one (J8).
  Matplotlib, which sends a PNG and `text/plain`, never showed this.

Still untested: array-valued and JSON-valued outputs (Julia produced none either), where the
transform changes bytes.

## The transform

- **Keep** a unit only if it is a code cell (not an inline unit), owned by `jupyter` **or `julia`**,
  in a plain language (no `.` in the language, which excludes extension languages such as marimo),
  and with no output carrying `text/x-quarto-markdown` (including knitr fallback units until KNR-8).
  The set of accepted owners is the same table RIU-3 uses for the import gate, in one place.
- **Duplicate-hash groups:** if any member of a group of cells with the same content hash is
  skipped, skip the whole group (Positron pairs by occurrence, so a partial group would pair the
  wrong output).
- **Output data (types):** a string stays; an array of strings is joined with no separator; any
  other value is serialized to compact JSON.
- **Output data (representations), new from spike J:** Positron shows every item, so send one
  representation per image.
  1. Drop `application/pdf` always (no renderer; it prints as base64).
  2. When both `image/png` and `image/svg+xml` are present, keep `image/png` and drop the SVG.
  3. Drop a `text/html` item whose whole body is a single `<img>` with a `data:image/` URI (the
     Plots case), because it repeats the image.
  A `text/html` that is anything else stays, so a table, a rendered widget or a Plotly figure with an
  image fallback keeps its HTML. Each drop is listed in the report with a reason. **Designed, not
  run:** J8 was run with all four representations. Confirm the rule in the window (Phase C).
- **Output metadata:** drop `quarto_format`; keep the rest (it carries image sizes and
  `quarto_webview_metadata`, which Positron reads).
- **Cell metadata:** reduce to `quarto_cell_id`, `quarto_content_hash` and, when present,
  `quarto_label`. Always emit `outputs`, and `execution_count: null`.
- **Notebook metadata:** `quarto_cache_version: 1`, `quarto_source`, `last_updated` (a millisecond
  timestamp the **caller** supplies, so the function reads no clock). Positron rejects a notebook
  without a numeric `quarto_cache_version`.
- **`quarto_source`** is what the caller supplies and is not interpreted. Positron ignores it on load
  and rewrites it when it saves (S5, S7; a file URI and a relative path both worked). So q2 builds no
  URI: the subcommand takes `--source-uri` and passes it through, the Positron command passes the
  exact `document.uri.toString()` (so Windows drive letters and non-ASCII paths are VS Code's own
  encoding, not ours), and without the option the value is the document's project-relative path.

What cannot be exported, and why: cells owned by an engine other than Jupyter or Julia (knitr R,
marimo), because Positron would have its own kernel produce those outputs or none; inline spans,
which are not cached cells in Positron; and rendered `text/x-quarto-markdown` outputs, a q2
construct Positron cannot display. Each is reported in the skipped list with a reason.

## Phase A: tests first

- [x] Fixtures, checked in as files so no kernel is needed at test time: the S1 `basic.qmd`
      **Done (2026-11-06):** `crates/quarto-core/tests/fixtures/records-export/` holds `basic`, `dup`, `julia-plot` and `julia-error` (each `.ipynb` record beside its `.qmd`), rendered with `target/debug/q2`. The expected exports are asserted in the tests, not stored as files.
      (`crates/quarto-preview/tests/fixtures/records-in-use/basic.qmd`) and the S1 `julia.qmd`
      records, each rendered once with `execute: {record: true}` in the front matter. **`dup.qmd`
      does not exist in that directory** (S7 made it in a scratchpad): create it (two identical cells
      and a labeled cell), render it the same way, and check in the record, not the page. Add a Julia
      plot record and a Julia record with an error cell (spike J's documents). Keep the expected
      export beside each.
- [x] One test per transform step: array join, object serialization, string passthrough, metadata
      reduction, `outputs` always present, version stamp, source passthrough, `last_updated` taken
      from the argument.
- [x] One test per representation rule: the PDF is dropped; PNG wins over SVG; an `img`-only data-URI
      `text/html` is dropped; a table's `text/html` stays; a Plotly-shaped bundle (a script-bearing
      `text/html` plus a PNG) keeps both. Assert the reason is reported.
- [x] One test per skip class (inline unit, non-Jupyter-non-Julia engine, extension language, rendered
      Partial and complete duplicate groups, `julia`-owned cells and an untagged cell (engine from ownership) are covered too. The "owner table admitting only `jupyter` exports 0" regression is not a test: the owner table is a constant, so there is nothing to switch.
      markdown, partial duplicate group), asserting the reason and that the other cells still
      export. S7 results to reproduce: `basic` exports 4 cells and skips 1 (`inline unit`); `dup`
      exports 3. Spike J results: the five-unit Julia record exports 4 and skips the span; with the
      owner table admitting only `jupyter` it exports 0 (the J4 behavior, kept as a regression).
- [x] A round trip through the `positron_loader` mirror in `positron_conformance.rs`: the export is
      accepted, and the mirror yields the outputs the record held. Where S7 (the real window) and
      the mirror disagree, the mirror is wrong; fix it in the same change and say so.
- [x] A negative test: an export the mirror would delete (for example missing `outputs`) cannot be
      produced. Build the bad case by mutating the output, not the function.
- [x] The inverse check with RIU-3: export then import (`record_from_positron`, with the real
      **Partial:** the import gate in this tree admits only `jupyter` and `knitr`; its `julia` widening is RIU-3's, so the Julia round trip passes owner `"jupyter"` as a stand-in. Re-point it at `"julia"` when RIU-3 lands.
      registry's owner) gives back the same outputs for the cells that exported, for a Python and a
      Julia document (spike J's `riu_s8_check` did this by hand: 4 of 4).
- [x] A test with an output whose data is an array of lines (the untested class), asserting the
      joined string.

## Phase B: code

- [x] `export_for_positron(&Record, &ExportOptions) -> Result<ExportedCache, ExportError>` where
      `record/positron_export.rs`, re-exported from `record`.
      `ExportOptions` carries the source string and `last_updated`. `ExportedCache` holds the bytes
      and the list of skipped units and dropped representations, each with a reason. No file writes,
      no environment or clock reads.
- [x] **`ExportError`**, with causes defined by checks the function makes on a record that
      `read_record` accepted: `MissingContentHash` (a cell that cannot be matched because its
      `quarto_content_hash` is empty) and `MalformedOutput` (an output that is not an object, or an
      `outputs` field that is not an array). Each names the cell id. (The spike had no failing path.)
      No exportable cells is **not** an `ExportError`: the function returns an empty cache and the
      subcommand turns that into its own exit code.
- [x] Nothing else reimplements this function; the subcommand (Phase B2) is a thin wrapper.
- [x] Place it next to `positron_import.rs`, target-neutral so a WASM caller is possible later; do
      not add a WASM export in this plan.

## Phase B2: `q2 record export-positron`

**Done (2026-11-07).** `crates/quarto/src/commands/record.rs`, wired as `Commands::Record` in `main.rs`; tests in `crates/quarto/tests/integration/record_export_positron_cli.rs`. A real-engine run on the S1 `basic.qmd` (no `record: true`, a scratch venv) exported 4 cells and skipped the inline unit, as S7 did. The tests were written before the code, but the first run was after it (I did not watch them fail). That run found one real bug the unit-level checks could not: `Runtime::block_on` around `record_capture_run` panics ("Cannot start a runtime from within a runtime"), so the command enters a runtime and drives the future with `pollster`. A later run on the `julia-error.qmd` fixture (`engine: julia`, `execute: error: true`; only `julia` on `PATH`, no Jupyter kernel) exited 0 with `{"cells":2,"dropped":[],"skipped":[]}`; the output file held an `execute_result` of `2` and an `error` output (`ErrorException`), inspected by hand. Not done here: the Julia plot document (it needs `Plots` added) and the real-window check.

Only what the Positron commands need; nothing speculative. Epic decision 13.

- [x] `q2 record export-positron <doc.qmd> --out <file> [--records <list>] [--source-uri <uri>]`
      wraps the function. Default source list `q2,engine`, and **only those two names are accepted**: the
      export reads the `.quarto` copy or runs the engine, and does not use the walk, the cache or any
      imported source (a Positron cache is the destination here, not a source). RIU-4's parser accepts
      the other names, so the subcommand rejects `cache`, `positron` and a directory entry with exit
      code 2 and a message saying which two are valid. Use the `.quarto` copy if there is one,
      else run the engine, so a document **does not need `record: true`** (S1 B9: the `-M` forms of
      `record: true` write nothing, only front matter does). No consent prompt: it is a command
      the user types on their own files, like `q2 render`. Before it runs code it prints one line
      to stderr (for example "no record found; running the document to produce one");
      `--records q2` forbids execution and fails with a clear message when no record exists.
- [x] The engine step uses the existing execution path:
      `quarto_core::engine::preview_record::record_capture_run`, whose observer asks for an
      execution record (`wants_execution_record`), the same hook preview uses. Do not add a second
      execution path. The `.quarto` copy is read with `read_record`; it is **deliberately not checked for
      staleness** here (epic decision 2 of 2026-11-05 covers `record_one` in the preview, not this command),
      so it can carry outputs for cells the document has since changed. Positron
      matches by content hash and skips those, which the export report should say.
- [x] **Exit codes**: a stable code per class, kept in one constant list and defined in the first
      test, per the table below (S6 showed `execFile` returns the numeric code and distinguishes
      `ENOENT`). A failing engine run is its own class: a cell that raises makes `record_capture_run` return an error
for both Python and Julia unless `error: true` is set (RIU-0c spike 3).

| Code | Class |
|---|---|
| 0 | success; one line of JSON on stdout |
| 2 | a bad argument or usage error (clap's own code; an old `q2` without the subcommand also lands here) |
| 10 | no record, and execution is forbidden (`--records q2`, or no `engine` in the list) |
| 11 | no exportable cells (the JSON line is still printed, with the skipped list) |
| 12 | the record is unreadable or invalid (`read_record` failed, or an `ExportError`) |
| 13 | the engine run failed: a cell raised and neither `#| error: true` nor `execute: error: true` was set (the message on stderr names the cell and the option); with `error: true` the run succeeds and the failing cell exports with its error output |

- [x] Stdout is exactly one line of JSON, `{"cells": n, "skipped": [{"id": ..., "reason": ...}],
      "dropped": [...]}`; diagnostics go to stderr; arguments are an array, never through a shell.
- [x] An older `q2` that lacks the subcommand exits non-zero with a usage error (code 2); the Positron
      command treats that as "q2 too old" (S6: the `q2` on PATH was an old nightly), so no capability
      probe is added.
- [x] (Partial: the Julia error document was run by hand, not as a test, and the Julia plot document was not run.) Tests first through the real argument parser, including that `--records cache`, `positron` and a
      directory entry exit 2 with a message naming the two valid names; then run it on the S1 `basic.qmd` fixture with
      no `record: true` and on a Julia document, and inspect the file and the JSON line. Add the
      Windows cross-check (Phase C).

## Phase C: verify

Use `target/debug/q2`, never the `q2` on PATH (an old nightly that cannot read records). A Jupyter
document needs a venv: `uv venv $SCRATCH/venv && uv pip install --python $SCRATCH/venv/bin/python jupyter-core ipykernel matplotlib pandas`,
then `export PATH=$SCRATCH/venv/bin:$PATH`. A Julia document needs `julia` (1.12.7 is installed);
a plot needs Plots in the document's Julia project (add it with `JULIA_PKG_OFFLINE=true julia
--project=. -e 'import Pkg; Pkg.add("Plots")'`).

- [x] `cargo clippy -p quarto-core -p quarto --all-targets -- -D warnings`: clean on the host for both crates (2026-11-07).
- [ ] The Windows cross-check **not run**: `cargo clippy --target x86_64-pc-windows-msvc` cannot build `aws-lc-sys` from macOS (the caveat in `.claude/rules/cross-platform.md`). The new tests build their one path with `Path::join`; a Windows user should run the native clippy. (the subcommand takes paths and writes a file):
      `cargo clippy -p quarto-core -p quarto --all-targets --target x86_64-pc-windows-msvc -- -D warnings`.
- [x] `cargo nextest run -p quarto-core -p quarto` 2026-11-07, `--no-fail-fast`: 8771 run, 8770 passed, 38 skipped, 1 failed. The failure is `revealjs::assemble::tests::vendored_reveal_assets_match_npm_package`: this worktree\'s `node_modules` reveal.js `reset.css` differs from the vendored copy. Not touched by this change; not run on the base to confirm. The 13 B2 tests passed with a scratch venv on `PATH`; the full run had none, so its two engine tests skipped.
- [ ] **NOT RUN (needs Gordon at the Positron dev build)** (the S7 method; the push command is the S5 spike or
      RIU-6 command 2): push an exported file for (a) a document with an array-valued output, and (b) the
      Julia plot document, and confirm the plot shows **once**, with no base64 text. The Julia plot
      document and its old, unfiltered export are in `~/docs/inline-output/` (`s8-julia.qmd`,
      `s8-julia-export.ipynb`). If no human is available, mark the step not run; the mirror test is not
      a substitute for seeing it.

## Exit criteria

- Export passes the mirror for every fixture and skips and drops what is listed above.
- The subcommand's contract, with its table of exit codes, is implemented and tested against this
  function.
- A Julia document exports and, in the window, shows each output once (or the notes say the step was
  not run).
- What Positron was seen to display is recorded; what was only checked against the mirror is
  labeled as such.

## Risks

- **The mirror is a model of Positron, not Positron.** Real-window confirmation is the proof;
  keep the S7 and J7 evidence linked.
- **The representation rule is narrow on purpose.** A different Julia plotting package may emit a
  bundle the three drops do not cover; the window run is the check, and the report lists what was
  dropped so a surprise is visible.
- **Rendered and fallback units** are skipped, so an R document run through knitr exports little
  until KNR-3 and KNR-5 land. That is expected; the demo does not include it.
- **Large records.** The S7 record was 30 KB; the Julia plot record is about 75 KB (a PDF, a PNG and
  an SVG); Positron has a 100 MB cache cap. Not tested at size.
