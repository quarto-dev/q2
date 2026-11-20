---
title: 'RIU: findings from the spikes after the stack rebase'
date: 2026-11-05
description: 'Results of spikes S0 to S5 run on feature/records-in-use after it was rebased onto the execution-records and knitr branches, plus two reviews of what those branches changed, and the four contradictions as Gordon answered them.'
---

**Plan:** [RIU: spikes after rebasing onto the execution-records and knitr stack](../plans/2026-11-05-riu-stack-rebase-spikes.md)
**Epic:** [the records-in-use epic](../plans/2026-11-02-records-in-use-epic.md)

## How these were run

- **Tree.** `feature/records-in-use` rebased onto `feature/knitr-structured-records` (34 commits, no conflicts).
  Nothing was pushed.
- **Binary.** `target/debug/q2` (0.41.0) built cold in this worktree: `cargo build --bin q2`, 4 minutes 54
  seconds. For anything that serves a preview page, the full refresh chain was run first
  (`npm run build:wasm`, `cargo xtask build-q2-preview-spa`, `cargo build --bin q2`), because the WASM and SPA
  in the worktree dated from 2026-11-03, before the knitr work.
- **Environment.** A scratch uv venv with `jupyter-core`, `ipykernel`, `matplotlib` and `pandas`, its kernelspec
  installed under the venv prefix, and its `bin` first on `PATH`. Without it the first preview run showed
  `Q-PREVIEW-CAP-1`, "Engine 'jupyter' is registered but its runtime is not available": an environment gap, not
  a defect. R through knitr was the system `Rscript`.
- **Browser.** Headless Chromium through Playwright, reading the preview iframe's text, as RIU-0 S1 did. The
  drivers live in the session scratchpad and are not committed. The fixtures were copied into the scratchpad,
  so the runs wrote nothing under the repository.
- **Side effect cleaned up.** The WASM build modified the tracked file
  `crates/wasm-quarto-hub-client/Cargo.lock`; it was restored with `git checkout` and is not in any commit.

## Two reviews that preceded the spikes

Two read-only sonnet reviewers examined what the stack changed. Every claim acted on below was checked by
reading the code or the file myself, and two of the first reviewer's line numbers were wrong (see the cite table).

## S0: the rebased branch builds and its tests pass

- `cargo build --bin q2`: finished.
- `cargo nextest run -p quarto-preview -p quarto-hub-provider --no-fail-fast`: **372 run, 372 passed, 5 skipped**
  (21 seconds).
- `cargo nextest run -p quarto-core --no-fail-fast` filtered to `record`, `capture_files`, `positron` and
  `engineless`: **568 run, 568 passed, 7275 skipped** (40 seconds).
- **Result.** The rebase is sound, including `capture_driver.rs`, which both the RIU commit and the stack changed.
  This is a filtered run, not the workspace suite, so a break in a crate outside those filters would not show.

## S1: mixed documents with the gate on and off

Fixtures: the committed `mixed.qmd` (a Python cell and an R chunk), `knitr.qmd`, and a richer copy with a
matplotlib plot, `head(mtcars, 3)`, an R plot, a `results: asis` chunk and an inline `r x + 2`. Each ran under
`q2 preview --no-browser --data-dir <fresh>` with `QUARTO_ENGINE_RECORD_WRITE` set to `1` or `0`.

| Document | Gate on | Gate off |
|---|---|---|
| `mixed.qmd` | `python cell` and `[1] 2` shown | identical |
| `knitr.qmd` | `[1] 2` shown | identical |
| rich fixture | both images, the data frame, **`asis text`** | both images, the data frame, **no `asis text`** |

- The `asis` difference reproduced with the file names and the gate settings swapped, so it is the gate and
  not the run. The inline `r x + 2` showed as raw text in both states.
- **Edit test (gate on).** Editing the R chunk of `mixed.qmd` from `1 + 1` to `2 + 2` on disk left
  `python cell` and its output, dropped the R chunk's output, and the host showed "Code has changed since the
  last capture. Re-execute".
- **Result.** RIU-0 B10 stands after KNR-6 **for engines that build records**: no output is lost in a mixed
  document with the gate on, and none differently with it off, except one new difference. B10 and this spike
  never mixed in an engine that builds no record; S5 below does. With the gate off, knitr's `results: asis` output is
  lost in the preview; with it on, it shows. Which side is "right" is not in question (on shows it); what
  matters is that the flip changes knitr documents and RIU-1 now says so. The cause is not investigated here.
  It is plausibly the gate-off legacy capture path discussed in KNR-6 (its plan says the legacy path reads
  `result.markdown` and "6B states which it holds while the record-writing gate exists"), but that is a
  reading of the plan, not an observation.

## S2: a knitr record written in one format, read in another

- `q2 render rec-knitr.qmd --to docx` with `execute: {record: true}` wrote
  `.quarto/execution-records/rec-knitr.ipynb`. Every knitr cell carries `quarto-output-format` set to `docx`
  and `quarto_hash_rule`; the engine entry's hash definition lists the `structured` profile. The Jupyter
  control record carries no format tag.
- `q2 render --to html` afterwards re-ran knitr (`processing file: rec-knitr.rmarkdown`) and **rewrote** the
  record with `html` on both cells. So `render` does not read a record, as expected, and a second render
  overwrites the first (the epic already notes this for `record: true`).
- `record_is_stale` (`record/staleness.rs:56-165`) hashes a profiled unit under `recorded_format(record)`,
  the format the record was recorded for, not the format being served.
- **Result.** A format mismatch does not by itself make a knitr record stale. Whether docx-shaped structured
  output presents correctly in an HTML page was **not** observed: no code reads a `q2` record yet. RIU-4 now
  has a test that pins this and says the first real run answers the presentation question.

## S3: the Positron gate for R, and what a fallback unit is

- The gate is `outputs_for` at `positron_import.rs:146`: owner `jupyter` with a plain language, or owner
  `knitr` with the token exactly `r`, and never a unit whose `fallback` argument is true. KNR-5 merged it in
  `5db29f6bc`. It does not admit the owner `julia`, which RIU-3 adds.
- A knitr record of plain, labeled (`#| label: lbl-one`), `asis`, `echo: false` and plot R chunks plus one
  bash chunk, rendered with `record: true`:
  - every R chunk is a structured unit (`quarto_hash_rule` set) with `stream` outputs, or `display_data`
    `image/png` for the plot, and every one has a `quarto_cell_id`, including the labeled one;
  - the bash chunk is a **fallback unit**: `display_data` with `text/x-quarto-markdown`, no hash rule.
- **Result.** `record_from_positron` has no recorded output, so it passes `fallback = false`; that is safe
  because the language test already excludes bash. RIU-3 says so.

### S3b: a real Ark cache (added 2026-11-06, after Gordon ran the document in Positron)

My first check found no Positron cache for an R document: I looked only in the release folder, and Gordon had run
`~/docs/inline-output/ks2-ark.qmd` in the **dev build**, which writes under
`~/Library/Application Support/code-oss-dev/User/globalStorage/quarto-inline-outputs/` (RIU-3's locator table
already lists both folders). The file is `b9e16dd5a1d7e6fb.ipynb`, written 2026-11-06 08:04.

- **The file name rule holds.** `printf '%s' file:///Users/gordon/docs/inline-output/ks2-ark.qmd | shasum -a 1`
  starts with `b9e16dd5a1d7e6fb`: the name is the first 16 hex digits of the sha1 of the document URI.
  `quarto_source` in the file is that URI. It has six cells, each with `quarto_cell_id` and
  `quarto_content_hash`; the labeled chunk is recorded as `unlabeled`, as KNR-5 said.
- **Run.** `KS2_ARK=<copy of the cache> KS2_QMD=~/docs/inline-output/ks2-ark.qmd cargo nextest run -p quarto-core
  --no-capture -E 'test(a_real_ark_cache_presents_through_the_knitr_layer)'`: **1 passed**.
- **All six knitr-owned `r` units imported** through the widened gate (`imported=true` for each), including the
  chunk with `#| label: fig-hist`, `fig-width` and `fig-height`: the content hash matched even though the cell
  text carries option lines. The presented HTML-target markdown carried `fig-width='4' fig-height='3'` and the
  figure at `width=384 height=288`.
- **One output is lost: the first plot.** `plot(cars)` presented as the text `[Image output]` and wrote no PNG;
  only `hist` produced a file (`cell-6-output-1.png`, 32463 bytes). The cause is confirmed: that cell's
  `image/png` string is 103794 characters long, 2 more than a multiple of 4, so it is unpadded base64, and the
  strict decoder rejects it. The `hist` string is padded. This is braid strand bd-wzz90981 (closed), fixed in
  `c666e10cc` on `braid/bd-yt2o2ovt-ark-r-kernel` (workspace-7), **unpushed and not in this stack**.
- **Two behaviors KNR-5 owns, seen here and not RIU's to fix.** The error cell shows `Error:` and
  `! deliberate` twice. The `message("a message")` output presents as stdout, not stderr.
- **Result.** The importer and the gate work on a real Ark cache. For real Ark R plots, RIU depends on the
  base64 fix reaching the stack. It does not affect the Python and Julia acceptance cases (ipykernel pads).

## S4: the engineless record with the gate on

- Two inline copies of the "has engines" rule exist: `capture_files.rs:303` and `cache.rs:285`. Nothing else
  tests `quarto_engines.is_empty()`. RIU-2 item 1 already covers the cache site.
- `gate_on_with_an_engineless_record_falls_back_to_legacy` (`capture_files.rs:442`) clears both engines and
  cells, so under `describes_something` it still describes nothing and still falls back. It does **not** flip,
  contrary to the review's worry. The case RIU needs is an engineless record **with cells**, now named in RIU-2.

## S5: an engine that builds no record, beside one that does (added 2026-11-06)

EXR-15's actual worry, which B10 and S1 had not tested because every engine they used builds a record.

- **Setup.** The `echo` fixture engine from `crates/quarto-core/tests/fixtures/extensions/echo-engine`: a
  TypeScript extension engine that returns markdown and builds no record. Bundled with
  `esbuild src/echo-engine.ts --bundle --format=esm --platform=node` into a scratch project's
  `_extensions/echo-engine/dist/`. A plain `q2 render` of the mixed document showed both outputs.
- **Documents.** `mixed-echo.qmd` (a Jupyter `print("python cell")` cell and an `echo` cell) and `echo-only.qmd`
  (the `echo` cell alone), each previewed with `QUARTO_ENGINE_RECORD_WRITE` on and off.

| Document | Gate on | Gate off |
|---|---|---|
| mixed | `python cell` and the echo cell's **source**, `Hello from echo!`; the engine's output `ECHO_EXECUTED` is **missing** | `python cell` and `ECHO_EXECUTED` |
| echo only | `ECHO_EXECUTED` | `ECHO_EXECUTED` |

- It reproduced with the files and gates swapped. The echo-only control is fine because its record has no
  engines and no cells, so `encode_capture_payload` falls back to the legacy array.
- **Cause (code reading plus the observation).** `encode_capture_payload` (`capture_files.rs:303`) writes only
  the record when `quarto_engines` is non-empty, and the legacy captures, which hold the echo engine's output,
  are dropped.
- **Result.** EXR-15's worry is **real for an engine that builds no record**, and it is reachable by anyone with
  an extension engine. Epic decision 2's claim "no special rule for mixed documents" holds only for engines that
  build records. The writer therefore needs a rule for such documents, decided on 2026-11-06 as option A (RIU-1). This corrects my earlier statement that S1 supported the epic on this point.
- **Not tried.** A Julia-engine or marimo document with a record-less engine, the hub path (`provide-hub`), and
  any of the alternatives to the recommended fix.

## Drift against the plans' cites (checked, then fixed)

Shifted and fixed: `engine_execution.rs` 864 to 892, 1079 to 1108, 1091 to 1120, 1109-1110 to 1138-1139,
989 to 1018, 281-287 to 306-312; `matching.rs` 61-95 to 131-163; `record_splice.rs` 140-160 to 135-169 (the
fallback literal at 161-167) and 248 to 257; `positron_import.rs` 147 to 146; `recompute_staleness` 262 to 252.

The reviewer also reported `staleness.rs:51` as `:48-50` and `capture_files.rs:385` as `:386`; both are wrong, and
the plans' cites (`:51`, `:385`) were already correct and were left alone. Cites in RIU-0 findings and in the
RIU-0 plan record where things were when the spikes ran and were not changed.

Other semantic drift, now in the plans: `UnitKey` gained `alt_exec_hashes` and `header_keys`; imported cells
carry no `quarto_header_keys`, so the splice's header-key demotion is a no-op for them; `record_is_stale` is
unchanged in signature.

## The four contradictions, answered by Gordon on 2026-11-06

Gordon agreed with each recommendation. The plans were edited in place; the edits are in the epic's "Decisions
recorded 2026-11-06" and in the plans named below.

1. **When the preview's writer flips.** The rule is stated per writer and reader pair, not per release. Epic
   decision 2 is rewritten; RIU-1 rewrites the runbook rule and names the one local case that strands (a kept
   `--data-dir` opened by an older binary after a downgrade); the parent epic's decision 4 points to it.
2. **Mixed documents.** Settled as option A after S5: a document with an engine that builds no record is stored
   whole in the legacy format, with a catalogued warning (RIU-1). It is a stopgap until EXR-17, whose trigger now
   treats the diagnostic's engine list as the inventory. Legacy-format documents keep main's Re-execute banner
   after any edit (observed: a prose edit shows it with the gate off and not with it on).
3. **Engineless records.** A clarification. The rule is extended (a record with cells and no engines counts as
   describing something) and not reversed; the stack's own test passes unchanged. RIU-2 and EXR-15 say so.
4. **Reuse of a record file.** Epic decision 3 is rewritten to name the new, bounded kind of reuse. `q2` stays
   in the preview's default list. The parent epic's decision 8 points to it. `provide-hub` still defaults to
   `engine`.

## Not verified

- Whether the `asis` loss with the gate off is a stack defect or an intended gate-off limit.
- Docx-recorded knitr output presented in an HTML preview (needs RIU-4's `q2` source).
- Real Ark R plots through q2 are blocked on the unpadded-base64 fix, which is not in this stack (S3b); the same
  fix was not tried here.
- The hosted hub and any `provide-hub` run: not run here, per the standing rules.
- The workspace test suite: only the filtered runs above were done.
- Whether a Julia-engine run of `python` or `r` cells is owned by `julia` and so exported or imported under
  RIU's `jupyter` and `julia` rule (the second review could not determine it).
