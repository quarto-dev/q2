---
title: 'RFU-3: the EXR-16 sweep and the labeled-cell fig-cap splice test'
date: 2026-11-08
description: 'Two test plans for the Positron import path that RIU-3 left open. A table-driven sweep over the 21 conformance cases (build a Positron-style cache, import, convert and splice, and assert which units receive outputs), and a labeled Python cell with fig-cap carried through import and splice. Runs after the one-presenter loop merge so it pins the final splice behavior.'
status: complete
depends-on: [opr-1b]
lane: A
---

**Epic:** [RIU follow-up](2026-11-08-rfu-epic-riu-follow-up.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Branch:** `rfu/3-positron-splice-tests` · **Lane A** (`workspace-3`) · **Blocks:** nothing.
**Needs:** nothing beyond the repository (R-dependent cases skip without R).

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-3 && pwd
git switch -c rfu/3-positron-splice-tests feature/one-presenter
# claim: set status: in-progress in this file, commit, land (protocol, "Claiming a plan")
# ... work; commit often ...
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only rfu/3-positron-splice-tests
```

## Why this waits for OPR-1b

These tests present spliced output. OPR-1a and OPR-1b change the splice\'s forwarding and span handling on purpose, so
a test written before them would pin behavior that is about to change. Write them against the landed behavior.

## Goal

- **EXR-16 sweep.** One table-driven test in `crates/quarto-core/tests/integration/positron_source.rs` (or a sibling in
  the same binary) that, for each of the 21 `CASES` in `positron_conformance.rs`, builds a Positron-style cache from the
  case\'s fixture, imports, converts and splices, and asserts which units receive outputs; cases where Positron and q2
  diverge (`positron-expected.json`) assert the divergence. (read, 2026-11-07) Today `positron_conformance.rs` tests
  only the generator and the loader, and `positron_source.rs` covers three durable fixtures (python, hand-built
  julia, r).
- **Labeled-cell `fig-cap` splice case.** A Python cell with `#| label: fig-x` and `#| fig-cap: ...` through import,
  convert and splice, asserting the output shows on the page. `knitr_positron_import.rs:~179` and `:~261` cover the
  hash and the record shape only.

## Checklist (tests first; every box is `[unattended]`)

- [x] Read `positron_conformance.rs`, `positron_source.rs`, `knitr_positron_import.rs` and the fixtures under
      `crates/quarto-core/tests/fixtures/positron-conformance/` and `positron-cache/`.
- [x] Write the sweep as a table; one row per case with the expected set of units that get outputs; divergence rows cite
      `positron-expected.json`. Write it so it **fails** if a case silently stops receiving outputs (mutate a case to
      see it fail, then restore).
- [x] Write the labeled-cell case; assert the figure and its caption appear. Note that a multi-figure labeled cell with
      `fig-subcap` is a known gap on the Jupyter-shaped route (`bd-gbaykhth`); do not add that case here and do not file
      a new strand.
- [x] Register new files in `crates/quarto-core/tests/integration/main.rs`, alphabetically.
- [x] Gates: `cargo clippy -p quarto-core --all-targets -- -D warnings`; `cargo nextest run -p quarto-core`; the
      Windows cross-clippy for `quarto-core` (build any expected path or URI with `Path::join`).
- [x] If a case reveals a product bug, **do not fix it here**: record it in "Findings for Gordon" with a minimal
      fixture; a bug outside every plan may become a strand (Gordon decides).

## Exit criteria

- The sweep covers all 21 cases and the labeled-cell test exists; both pass; the sweep fails when a case regresses.
- Findings (if any) are written up.

## Notes from the run

**Landed.** Three tests in `crates/quarto-core/tests/integration/positron_source.rs` (no new file, so `main.rs` is unchanged):

- `each_conformance_case_shows_outputs_on_exactly_the_expected_units`: for each of the 21 cases it builds a Positron-style
  cache from `positron-expected.json` (one code cell per cell Positron found, with Positron\'s own id and hash, each with a
  `MARK-<case>-<n>` stream output), imports it through the real source inputs (the real registry\'s ownership), converts, and
  preview-renders with an empty registry, so every marker on the page came from the record. The `SWEEP` table lists, per case,
  which of Positron\'s cells show, with the reason. A parse-error case is an `Err` row.
- `the_sweep_table_covers_exactly_the_conformance_cases`: the table has 21 rows and matches the `.qmd` files on disk.
- `a_labeled_python_cell_with_fig_cap_shows_its_figure_and_caption`: `#| label: fig-x` and `#| fig-cap: ...` through import and
  splice gives a `FloatRefTarget` `fig-x`, the image file `doc_files/figure-html/cell-1-output-1.png`, and the caption text.

**Tests first.** These cover existing behavior, so they passed once the table matched what the product does. My first table
was wrong for four rows (below); each mismatch was read and explained before I changed the row. Mutation check (ran): changing
`plain.qmd` to `x = 2` made the sweep fail with `plain: want Ok([0]), got Ok([])`; restored.

**Table rows that were not what I first predicted** (the product is the evidence; I did not fix any):

- `two_cells`: only the `r` cell shows. A document with an `r` cell has knitr own `python` too (`owner_of("python")` is
  `knitr` there, `jupyter` in `plain`), so the python cell is claimed but gated out.
- `tilde_fence`: q2 rejects `~~~{python}` (curly braces reserved), so it is an `Err` row like the two `brace_label_*` rows.
- `marimo_spaced`: **shows** (Finding 2).
- `brace_id`: shows nothing (Finding 1).

**Findings for Gordon** (no strand opened; none belongs to a plan):

1. `{python #bar}` (case `brace_id`): imported Positron outputs never show, and the imported record reads stale. The masked
   document that the source inputs scan has the fence rewritten to ```` ```{#bar python} ````, which `scan_units` does not read as
   a cell, so Positron\'s cached cell is unclaimed (`UNCLAIMED_KEY` is 1). The conformance generator scans the raw text, where
   the unit exists with the matching hash, so `brace_id.q2.json` and the splice disagree. Minimal fixture:
   `tests/fixtures/positron-conformance/brace_id.qmd` plus a one-cell cache with hash `34bce5f775de97f5`. The test pins the
   current result; when the product is fixed the row becomes `Ok(&[0])`.
2. `{python .marimo}` (case `marimo_spaced`): q2 reads language `python` with class `marimo`, the owner is `jupyter`, so a
   Positron cache fills it and its outputs show, though marimo would run the cell. The row pins this; decide whether the
   class should gate out the import.

**Gates.** `cargo clippy -p quarto-core --all-targets -- -D warnings`: clean. `cargo nextest run -p quarto-core`: 8119 run, 8118 passed,
1 failed (the environmental `revealjs::assemble::tests::vendored_reveal_assets_match_npm_package`); the three new tests are among them. Windows cross-clippy: not runnable here (build scripts fail for the msvc target on macOS); the change builds
no path literal (the fixture directory is `Path::join`). WASM build: not run, because no presenter, record or splice source
changed (tests only). No snapshots or goldens added, changed or removed.
