---
title: "output-interleave: hold by default, interleave on request, for every engine"
date: 2026-11-13
status: in-progress
---

## Overview

Make held output (all of a cell's outputs after its source) the presenter's default for
every engine, add the engine-neutral option `output-interleave` to opt into interleaving,
and let knitr declare that it interleaves by default (Quarto 1 parity). Julia goes back to
Quarto 1's held output, and interleaving becomes opt-in. Also lift knitr's partial holds
(`results: hold`, `fig.show: hold`, `fig.beforecode`) out of the knitr-only branch, so they
work on any record that carries anchors.

Research: `claude-notes/research/2026-11-13-results-hold-across-engines.md`.

## Decisions (Gordon, 2026-11-13)

1. **Julia holds by default**, the same as Quarto 1. Authors opt into interleaving. This
   reverses OPR-9d decision 4 (interleaving on by default).
2. **Core holds by default.** An engine entry that does not declare interleaving (Julia,
   Jupyter, any future engine, a record with no entry for the cell's engine) presents held.
   knitr declares that it interleaves.
3. **Name: `output-interleave: true | false`**, valid in a cell `#|` header and under
   `execute:`. The cell beats the document scope, and the scope beats the engine default.
4. **knitr hash version bump.** Old knitr records lack the declaration and would present
   held. Bumping `KNITR_HASH_VERSION` makes them stale, so they re-run. OPR-5 decision 6
   already treats record compatibility as free.

## Design

- `EngineEntry` gains `interleaves_output: bool` (serde default `false`, skipped when false),
  set through `EngineEntryInput`. The knitr adapter and `chunk_records` set it to `true`.
- The decision is made in `present_cell_step` from the Quarto options (scope plus `#|`),
  never from knitr's `resolved_options`. The R layer forwards only specific `execute:` keys,
  and a knitr cell does not layer the scope (D3). Resolution: explicit
  `output-interleave`, else `engine.interleaves_output`, else `false`. When the result is
  `false`, the anchors are dropped before rendering, so the cell renders whole.
  `prompt` and numeric `echo` still use the expression starts.
- The holds and `fig.beforecode` run on any cell with usable anchors while interleaving.
  Their flags come from `CellOptions` (`results`, `fig.show`, `fig.beforecode`), not the
  raw knitr JSON.
- `output-interleave` is never forwarded as a cell attribute: it is skipped for Jupyter
  (reader key) and for knitr (`from_knitr_known`).
- It is display-only for every engine: Jupyter `DISPLAY_ONLY_KEYS`, knitr
  `STRUCTURED_PRESENTATION_OPTIONS` and the `execute:` exclusions, and Julia `record-wire.ts`
  (in the fork).

## Status and handoff (2026-11-13, end of the first session)

The work is in the WIP commit on branch `explore/results-hold-all-engines` (workspace-1).
Phase 1 is written and was confirmed red. Phase 2's product edits are all applied. The last
`cargo build -p quarto-core --tests` **compiled without Rust errors but failed at the link
step** of the `quarto_core` lib test binary (`linking with cc failed`). The specific `ld`
error was cut off in the captured log. The disk was not full (279 GiB free), and the same
build linked fine during the red run earlier in the session. Rerun the build, capturing the
whole log, and read the `ld:` lines before assuming anything. Then run the Phase 1 tests and
fix the fallout below.

**Already applied in the WIP commit:**

- `record/types.rs` and `record/builder.rs`: `EngineEntry.interleaves_output` (serde default
  `false`, skipped when false) and `EngineEntryInput.interleaves_output`, passed through by
  the builder. These compiled during the red run.
- `presenter/options.rs`: new `CellOptions` fields and accessors: `fig_show_hold`,
  `fig_beforecode`, `output_interleave` (`Option<bool>`) and `results_hold()`, all read in
  `from_lookup`. Also `ENGINE_NEUTRAL_KEYS = ["output-interleave"]`, skipped by
  `from_knitr_known`, and `fig.beforecode` added to `KNITR_ONLY` in
  `every_key_the_reader_reads_is_declared_display_only_for_jupyter`.
- `presenter/knitr_inputs.rs`: `present_with_inputs` takes `interleave: bool` (before
  `fig`). When it is false the anchors are dropped, and the expression starts are kept. The
  `fig.beforecode` and hold block no longer sits inside the knitr-only `merged` branch. It
  reads `options.fig_beforecode()`, `options.fig_show_hold()` and `options.results_hold()`.
  The `merged` variable is gone. Both test helpers (`present_knitr_cell`, `present_inputs`)
  pass `true`.
- `presenter/cell_step.rs`: `interleave` is
  `options.output_interleave().unwrap_or_else(|| engine.is_some_and(|e| e.interleaves_output))`,
  computed from the Quarto options before they move into the fallback.

- `engine/knitr/adapter.rs` and `engine/knitr/chunk_records.rs`: `interleaves_output: true`.
- `engine/knitr/hash_rule.rs`: `KNITR_HASH_VERSION` is 4; `"output-interleave"` ends
  `STRUCTURED_PRESENTATION_OPTIONS`; `presentation_execute_keys` is
  `["output-interleave", "record-sources"]` for the default declaration and the structured
  profile.
- `engine/jupyter/mod.rs`: `"output-interleave"` ends `DISPLAY_ONLY_KEYS`.

**Then expect fallout in existing tests** that assume interleaving by default, without the
engine declaring it. Fix each by giving the engine entry `interleaves_output: true` when it
stands for knitr, or by setting `output-interleave: true` when it stands for Julia. Never
weaken an assertion. Likely places:

- `presenter/knitr_inputs.rs` `fixture::assert_rich`, used by the three presenter paths
  (`present_document`, the splice stage, replay). Their knitr engine entries need the flag.
- The OPR-4d Julia tests in `engine/ts_record.rs`, and any test of
  `anchors_alone_interleave_source_and_output`. Check the Julia ones go through the step.
- `presenter/jupyter_option_sweep.rs`: `every_declared_display_only_key_is_swept` will fail
  until `output-interleave` gets a row. The row is `Inert`, with an `INERT_REASONS` entry:
  Jupyter records carry no anchors, so every cell already renders held.
- `stage/stages/record_splice/tests/option_sweep.rs`: add `output-interleave` as `Presents`
  for knitr, and check its list of keys.
- The knitr corpus (`tests/fixtures/knitr-corpus/`, especially `s1/04_results`,
  `s1/05_fig_show_hold`, `s1/16_fig_show`), and record fixtures pinned to knitr hash
  version 3 (`tests/fixtures/records/golden.ipynb`,
  `tests/fixtures/records-export/knitr.ipynb`). The version bump may need these regenerated
  or updated; read how each is produced before touching it.
- One behavior change for Jupyter: `fig.beforecode` is now a reader key, so it is no longer
  forwarded as a `fig.beforecode='true'` cell attribute.

Gate for Phase 2: `cargo clippy -p quarto-core --all-targets -- -D warnings` and
`cargo nextest run -p quarto-core`.

## Checklist

### Phase 1: tests first (each fails for the stated reason)

All written in `presenter/cell_step.rs` (section `output-interleave`), unless noted. Red was
confirmed for the stated reasons (9 failures). The serde test and the opt-in test passed
already, as expected.

- [x] A Julia-shaped cell (anchors and starts, no resolved options, an entry that does not
      interleave) renders whole.
- [x] The same cell with `#| output-interleave: true` interleaves.
- [x] The same cell with `execute: output-interleave: true` interleaves, and a cell
      `#| output-interleave: false` beats it.
- [x] Interleaving plus `results: hold` holds stdout; plus `fig.show: hold` holds plots. This
      fails today because the holds are knitr-only.
- [x] A knitr record (resolved options, an entry that interleaves) still interleaves;
      `#| output-interleave: false` renders it whole; `execute: output-interleave: false`
      renders it whole.
- [x] A cell with anchors and no engine entry renders whole.
- [x] `EngineEntry` without the field deserializes as `false`; the knitr entry serializes
      `true`.
- [x] `output-interleave` is never written as a cell attribute (Jupyter and knitr).
- [x] The hash: the key is display-only for Jupyter and for structured knitr, both as a cell
      option and under `execute:` (`jupyter/mod.rs` and `knitr/hash_rule.rs` tests updated).

### Phase 2: implement

- [x] `EngineEntry.interleaves_output` and `EngineEntryInput`.
- [x] knitr sets `interleaves_output: true` (adapter and `chunk_records`).
- [x] Bump `KNITR_HASH_VERSION` to 4.
- [x] Read `output-interleave` in `from_lookup`; resolve it in `present_cell_step`.
- [x] Lift the holds and `fig.beforecode` in `present_with_inputs`.
- [x] Skip the key in `from_knitr_known`.
- [x] Declare the key display-only: Jupyter, knitr (cell and `execute:`).
- [x] Compile; Phase 1 tests green. (The link failure was stale `target/debug/incremental/quarto_core-*`
      artifacts, undefined `_anon.*.llvm.*` symbols; clearing them fixed it, no code change. Phase 1's
      `present_ab` helper also had a leading blank line in the fence when `options` was empty.)
- [x] Update the existing tests that expect Julia to interleave by default
      (`knitr_inputs`, `cell_step`, `ts_record`, OPR-4d), so they set the option or expect held
      output. Do not weaken them: each keeps its assertion under the explicit option.
- [x] Sweep rows: `jupyter_option_sweep.rs` (Inert, with the reason that Jupyter records
      carry no anchors), `record_splice/tests/option_sweep.rs` (Inert: its fixture has no anchors, so the cell renders held either way; cell_step tests cover the effect). Move
      `fig.beforecode` into `KNITR_ONLY` in the reverse sweep if `from_lookup` now reads it.

### Phase 3: Julia declaration and the parity harness

- [ ] Julia `record-wire.ts`: `output-interleave` is added to `DISPLAY_ONLY_KEYS` in the fork
      (`q2/node-port`, commit `4892bf5`, pushed with Gordon\'s OK; the first push reported an
      HTTP 500 but `ls-remote` and a fresh fetch showed the tip at `4892bf5`). Hash version stays
      at 2 (Gordon: everything here is very new). **Pending, left out of this branch on
      purpose:** sync with `cargo xtask pull-extension-subtree julia-engine` (that also brings in
      the earlier `executeEnv` commit `c8af0a4`) and move the `subtree-julia-engine` pin in
      `resources/vendored.toml` to `4892bf5`. Until then, editing `output-interleave` in a Julia
      cell re-runs the cell; the default (held) does not depend on the sync.
- [x] Parity harness: nothing to remove. The allowlist is already empty (OPR-9f); the
      `julia-interleaved-cell` normalizer and the `capture-facts` pin are unit-tested mechanism
      over static fixtures. The live gate (`OPR_PARITY_REPORT`, Julia installed, measured with the synced subtree) passes: 93
      identical, 0 allowlisted, 0 blockers, 0 route disagreements, 253 rejected by q2 before any
      engine runs (same on both routes).

### Phase 4: end to end and docs

- [x] `target/debug/q2 render` of a Julia document (record route): held by default,
      interleaved with the option. See "End-to-end record" below.
- [x] `target/debug/q2 render` of a knitr document: interleaved by default, held with
      `output-interleave: false` (document and cell scope), and `results: hold` unchanged.
- [x] User docs: "Where a cell\'s output appears" in `docs/guides/authoring/computations.qmd`,
      with a migration callout. Rendered with `q2 render docs/guides/authoring/computations.qmd`;
      no warnings name the page (the 153 warnings are other pages\' missing markdown companions
      in a partial render). The embedded docs (`cargo xtask build-agents-docs`) are not rebuilt.
- [ ] Gates: `cargo clippy -p quarto-core --all-targets -- -D warnings` (clean) and
      `cargo nextest run -p quarto-core` (8403 passed) are done. The Windows cross-clippy cannot
      run from macOS here: the `wasmtime` and `aws-lc-sys` build scripts need an MSVC sysroot
      (the caveat in `.claude/rules/cross-platform.md`). The change adds no platform APIs, cfg
      gates or path assertions; a Windows user can run
      `cargo clippy -p quarto-core --all-targets -- -D warnings` natively.
      `q2 render --strict` on this plan: the file renders; the 6 `Q-12-13` listing-preview
      errors belong to other plans\' pages.

### End-to-end record (2026-11-13, inspected)

Scratch documents live outside the repo. Each cell is the same code:
`println("first")`, `x = 1`, `println("second")`, `x + 1` (Julia), and the `cat`/`<-` equivalent
for R. Each render was `target/debug/q2 render <file>.qmd`; the HTML was read for the number
of `cell-code` blocks and the order of source and output.

| Document | Header | Result |
|---|---|---|
| Julia, record route | `execute: {record: true}` | 1 code block, then `first`, `second`, `2` |
| Julia, record route | `execute: {record: true, output-interleave: true}` | 3 code blocks; `first` between block 1 and `x = 1`, `second` between `x = 1` and `x + 1`, `2` last |
| Julia, default route | none | 1 code block, outputs after |
| knitr | none | 3 code blocks, output after each statement |
| knitr | `execute: {output-interleave: false}` | 1 code block, outputs after |
| knitr | cell `#| output-interleave: false` | 1 code block, outputs after |
| knitr | cell `#| results: hold` | 1 code block; `first` and `second` in one stream, then `[1] 2` |

The live parity gate also passed after the Julia sync (93 identical, 0 blockers).

## Out of scope

- Jupyter interleaving through an IPython AST-transformer marker (research note). It needs
  a spike first, and until then the option is inert for Jupyter.
- Dashed canonical names for `results` and `fig.show`, and display-only declarations for
  the knitr options that already present for Jupyter.
