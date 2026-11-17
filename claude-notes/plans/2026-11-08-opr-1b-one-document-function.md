---
title: 'OPR-1b: one document function that returns outcomes'
date: 2026-11-08
description: 'Merges the two document-level loops (present_document and the splice stage loop) into one function that takes already-matched cells and returns, per unit, a replacement, a skip with a reason, or an error, plus one shared apply_edits step. Callers keep matching, error policy, reporting, stamping and the AST reparse. Applies the span rule from classify_inline_result to both surfaces.'
status: complete
depends-on: [opr-1a]
lane: A
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Branch:** `opr/1b-one-document-function` · **Lane A** (`workspace-3`) · **Blocks:** OPR-2, OPR-4d, OPR-6, OPR-7a.
**Needs:** R 4.3 with knitr (some tests skip without it); `hub-client` build tooling for the WASM gate.

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-3 && pwd
git switch -c opr/1b-one-document-function feature/one-presenter
# claim: set status: in-progress in this file, commit, land (protocol, "Claiming a plan")
# ... work; commit often ...
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only opr/1b-one-document-function
```

## Goal (decisions D1, D9)

After OPR-1a both loops call one per-cell function. This plan makes the loop itself one function.

- **`present_units`** takes the document text, the units, the **already-matched** (unit, cell) pairs, the scope, the
  target format, the record\'s engine list and a figure sink. It returns, per unit, an outcome:
  a replacement (the byte range from `replacement(unit, text, markdown)` plus the markdown), a **skip with a reason**
  (stored markdown for another format; a front-matter span; a span or cell with no usable value; no match), or an
  **error** (invalid cell options, an invalid front-matter value).
- **`apply_edits`** applies replacements to the text and can report where each landed (the splice needs that to
  stamp output). Both loops already use the same `replacement` helper; today `present_document` builds an output
  string sequentially and the splice collects edits and applies them in reverse on masked text.
- **No policy flags.** What a skip or an error means is the caller\'s: render fails on an error and turns skips into
  notes and warnings; the splice keeps the source on both and ignores most notes.
- **Callers keep:** matching (`match_cells` by id for render, `match_units` by exec hash with staleness for the
  splice), error policy, reporting, output stamping, the AST reparse and the orphan report.

## Decisions that shape this plan

- **Spans (D9):** both surfaces use `classify_inline_result` (`presenter/inline.rs:250`): an error wins; else the
  **last** text-bearing `execute_result` or `display_data`; streams dropped; no value is a skip with a reason.
  Today the splice\'s `present_span` (`record_splice.rs:~505`) takes the **first** output with any data and ignores
  errors; that is a deliberate change to preview and hub. A front-matter span is a skip reason: render applies it
  (YAML-escaped; a newline is an error, as `inline_text` does today at `document.rs:316`); the splice ignores it.
  `present_span` is deleted.
- **Format mismatch:** a skip with a reason on both; render keeps its `PresentNote::MarkdownFormatMismatch`; the
  splice may surface it too (OPR-6 uses it for the `render` `record-sources` warning).
- **Substitution details:** `present_document` inserts a blank line before a cell when the preceding text does not end
  with one; the splice\'s edit path may not. OPR-0a pinned both. Keep render\'s text byte-identical; where the
  splice differs, follow render and say so in the landing report.

## Checklist (tests first; every box is `[unattended]`)

- [x] Start from OPR-0a\'s Handoff table. Every `// OPR-1b:` test is on your list.
- [x] **New tests first:** (1) both callers produce today\'s render and splice text for every OPR-0a fixture, except
      the rows the Handoff table marks as changing; (2) the outcome type: each skip reason and each error is returned,
      not swallowed or raised, by the shared function; (3) `apply_edits` reports spans that match the text it
      produced, on masked text with several edits; (4) the span table: the `classify_inline_result` result on every
      row, through both callers.
- [x] Add `present_units` and `apply_edits` (suggested home: `presenter/document.rs`; the splice stage imports it).
- [x] Convert `present_document` and its five callers (`engine/skeleton.rs:217`, `engine/replay.rs:178`,
      `engine/ipynb_replay.rs:238`, `engine/jupyter/text_execute.rs:658`, `engine/ts_record.rs:542`) to it.
- [x] Convert the splice loop (`record_splice.rs`, near line 231) to it, keeping matching, stamping (`StampSpan`),
      the unmask pass, the AST reparse and the report.
- [x] Delete the dead code (the old loops, `present_span`, any helper only they used).
- [x] Regenerate the preview and hub fixtures that change **only** through the span rule; review each diff and
      document counts and reasons in the commit message and landing report.
- [x] Gates: `cargo clippy -p quarto-core --all-targets -- -D warnings`; `cargo nextest run -p quarto-core`; the
      Windows cross-clippy for `quarto-core`; `cd hub-client && npm run build:wasm` and the hub-client record
      tests (`recordPayload.wasm.test.ts`, `captureSpliceHtml.wasm.test.ts`); `cargo xtask build-q2-preview-spa`
      then `cargo build --bin q2` if you will check preview by running it (AGENTS.md, "Verifying Rust changes in
      `q2 preview`").
- [x] **End-to-end:** run `target/debug/q2 render` on a Jupyter-record document, a knitr document with
      `execute: record: true`, and a Julia document with `execute: record: true`; read the HTML; record the
      invocations and what you saw in "Notes from the run".

## Exit criteria

- One document-level presenter function and one edit-application function exist; `git grep -n 'fn present_span'`
  and the old loop bodies are gone.
- OPR-0a\'s `// OPR-1b:` tests pass with their new expectations; all other OPR-0a tests pass unchanged.
- Render output is byte-identical to before on the goldens and fixtures (the 13 shadow goldens, six `ipynb-*`);
  preview and hub output differ only by the span rule (and OPR-1a\'s forwarding, already landed).
- WASM build, Windows cross-clippy and the end-to-end runs are green or reported.

## Risks

If the substitution routines produce different text for the same cell in ways OPR-0a did not predict, stop and
report with a minimal fixture; do not change render bytes silently. If this plan proves larger than one session,
report; do not land half of it.

## Notes from the run

**Landed:** `present_units` and `apply_edits` in `presenter/document.rs`. `present_units(&PresentInput, &[(unit index, &RecordCell)],
fig)` takes already-matched pairs and returns one `UnitReport` per pair: `UnitOutcome::Replace(Edit)`, `Skip(SkipReason)`
(`MarkdownFormatMismatch`, `NoUsableValue`, `EmptyValue`) or `Error(PresentError)`, plus the image failures the cell\'s figures
raised. `present_document` (render) and the splice loop in `record_splice.rs` both call it; the old loops, `present_cell`,
`present_span` and `stored_result_for_cell` are gone. `apply_edits(text, edits)` sorts, inserts the blank line before a block edit,
and returns each edit\'s range in the new text; the splice turns those ranges into `StampSpan`s and keeps the unmask pass.
`PresentInput` carries the author texts, because the splice\'s come from the sugared-block snapshot and render\'s from the scan.

**Deviations from the plan\'s wording.**

- A front-matter span is not a skip reason: it is a `Replace` whose `Edit` has `front_matter: true`. Render applies it, the
  splice drops it. A skip reason cannot carry the escaped value render needs, and a flag keeps "no policy flags" true of the
  function (the caller decides).
- The splice\'s `quarto_location == Metadata` filter stays in the splice, before the call (it is part of what the splice
  matches).

**Where the splice now follows render** (render\'s bytes did not change; each is a splice change):

- The blank line before a cell when the text before it ends in one newline: the splice reparses, so the AST is the same.
- A cell\'s own trailing newlines (render\'s rule) instead of `replacement`\'s single one; an inline value is inserted as
  rendered, not trimmed of trailing newlines.
- A span with stored markdown (knitr\'s verbatim span, `text/x-quarto-markdown`) is now placed by the splice when the format
  matches, as render does; before, the splice looked only for `text/plain`-family mimes and kept the source.
- A stored-markdown cell for another format does not advance the figure counter (render\'s rule); the splice used to.
  Only a document with such a cell followed by figures changes names.
No existing test exercised these four; if one of them matters to Gordon, it is a deliberate-looking one-line change in `present_units`.

**Tests.** First, `span_cases_through_both_routines` was flipped to the three rows the OPR-0a handoff table lists and run: red
(splice `1`, `7`, none). Then green after the change. New in `presenter/document.rs`: `a_cell_and_a_span_each_return_a_replacement`,
`each_skip_reason_is_returned_not_swallowed`, `invalid_cell_options_and_a_front_matter_newline_are_returned_as_errors`,
`apply_edits_reports_the_range_each_replacement_occupies`, `apply_edits_with_no_edits_returns_the_text` (all written against a new
function, so they passed once my own fixture mistakes were fixed). The first-run failures were mine: a fixture I called
image-only carried a `text/plain` value, and I miscounted byte offsets. Item (1) of the checklist is the existing OPR-0a suite,
the 13 shadow goldens and the six `ipynb-*` goldens, all unchanged and green.

**Gates.** `cargo clippy -p quarto-core --all-targets -- -D warnings`: clean. `cargo nextest run -p quarto-core`: 8061 run
before the five new tests, 8054 passed, 7 failed (a rerun of the affected tests after the new tests: 108 of 108 passed).
The seven: `revealjs::assemble::tests::vendored_reveal_assets_match_npm_package` (environmental, as in OPR-1a) and the six
`julia_engine_e2e::j1..j6` tests. These six passed in OPR-1a\'s run and fail now inside the installed QuartoNotebookRunner
(`ArgumentError: startpath must be non-empty` in `relpath`, from `evaluate_raw_cells!`), before any presenter code; this diff touches
only the presenter and the splice, and a by-hand `q2 render` of a Julia document with `execute: record: true` worked (below).
I did not run the six on the integration branch, so "not caused by this plan" is reasoned from the stack, not measured; a lane B
agent (RFU-5 is the Julia e2e flake) should look. Windows cross-clippy: fails in the `aws-lc-sys` build script on macOS, as in
OPR-1a; no platform API or path literal was added. `cd hub-client && npm run build:wasm`: green; `vitest run --config
vitest.wasm.config.ts recordPayload.wasm.test.ts captureSpliceHtml.wasm.test.ts`: 8 of 8 pass.

**End-to-end** (`target/debug/q2 render <file>`, output HTML read). Knitr, `engine: knitr`, `execute: record: true`, a span
and a cell with `fig-cap`: `Value 2 here.`, then `div.cell` with the source, `[1] 1 2 3`, the plot call, the image and
"A plot". Julia, `engine: julia`, `execute: {record: true, daemon: false}`, `1 + 1`: `div.cell` with the source and a
`cell-output-display` holding `2`. Jupyter: **not run**, there is no `jupyter` runtime on this machine; the ipynb and shadow
goldens cover the render path. Preview and hub were not run by hand beyond the WASM record tests.

**Snapshots and goldens.** None added, changed or removed. Preview and hub output change only by the span rule and the four
splice-follows-render points above; no existing fixture showed a difference.

**Strands.** None opened.
