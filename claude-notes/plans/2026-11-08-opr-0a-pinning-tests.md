---
title: 'OPR-0a: pinning tests for the presenter loops, scope, spans and entry paths'
date: 2026-11-08
description: 'Tests first for the one-presenter epic. Pins what the two presenter loops, the document execute scope, the span routines and the knitr and Jupyter entry paths do today, so OPR-1a and OPR-1b can prove they changed only what the epic decided. No product code changes.'
status: in-progress
depends-on: []
lane: A
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Branch:** `opr/0a-pinning-tests` · **Lane A** (`workspace-3`) · **Blocks:** OPR-1a.
**Needs:** nothing beyond the repository; R 4.3 with knitr for the knitr tests (they skip when R is absent).

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-3 && pwd
git switch -c opr/0a-pinning-tests feature/one-presenter
# claim: set status: in-progress in this file, commit, land (protocol, "Claiming a plan")
# ... work; commit often ...
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only opr/0a-pinning-tests
```

## Goal

Pin today\'s behavior with tests that pass on today\'s code, so the later plans change behavior only where the epic
says. This plan changes **no product code**. Where a test pins behavior that a later plan will deliberately change,
mark it with a comment `// OPR-1a:` or `// OPR-1b:` naming the plan and the intended new expectation.

## Background (read before writing tests)

(read unless marked) The per-cell step exists twice. `present_document` (`crates/quarto-core/src/presenter/document.rs:104`)
serves render: five non-test callers (`engine/skeleton.rs:217` knitr, `engine/replay.rs:178`, `engine/ipynb_replay.rs:238`,
`engine/jupyter/text_execute.rs:658`, `engine/ts_record.rs:542` Julia). The splice stage
(`crates/quarto-core/src/stage/stages/record_splice.rs`, loop near line 231, `present_cell` near line 431,
`present_span` near line 505) serves preview, the hub and a supplied record. Both end in `render_cell_with` and
`render_cell_whole` (`presenter/cell.rs`). Differences found by reading both (all to be pinned here):

| Difference | `present_document` | splice |
|---|---|---|
| Unknown `#|` options on a Jupyter cell | forwarded as cell attributes (`.forwarding(cell_only)`, `document.rs:~168`; `CellOptions::forwarding`, `options.rs:~313`) | not forwarded |
| Errors in cell options | `PresentError::InvalidCellOptions`, render fails | swallowed (`.ok()`), source kept |
| Stored markdown for another format | skipped with `PresentNote::MarkdownFormatMismatch` | skipped silently |
| Span value choice | `classify_inline_result` (`presenter/inline.rs:250`): an error wins; else the **last** text-bearing `execute_result` or `display_data` | `present_span`: the **first** `execute_result` or `display_data` with any data; errors ignored |
| Front-matter span | value presented, YAML-escaped; a newline in it is an error | skipped |
| Engine entry used for a cell | one entry for the whole document | per cell (`engine_entry_for`, by `quarto_engine`) |
| Matching | `match_cells` (by id, then content hash) | `match_units` (exec hash, then content hash; staleness; lost header keys) |

(relied, from a review agent, 2026-11-08) Julia\'s hash declaration lists every option as hashed and its capture
enforces only a cell\'s own `warning`; the knitr hash lists `include` and `output` as hashed while the presenter
enforces them. Those belong to OPR-2, not this plan.

The throwaway branch `spike/one-presenter-s1-s3` (commit `62afa5177`, local only) holds three measurement tests
from the spikes. Read it with `git show`; **never merge or push it**. Port what you need as proper tests.

## Checklist (tests first; every box is `[unattended]`)

- [ ] Find the existing oracle and sweeps and read them: `crates/quarto-core/tests/integration/jupyter_markdown_goldens.rs`
      (`stored_records_present_to_the_goldens`), `tests/fixtures/jupyter-markdown-goldens/` (13 shadow goldens and six
      `ipynb-*` stored-replay goldens), `crates/quarto-core/src/presenter/jupyter_option_sweep.rs`, and the knitr
      option sweep (`git ls-files | grep option_sweep`).
- [ ] **Document-scope fixtures (Jupyter).** Add fixtures whose front matter sets `execute:` keys: `echo: false`,
      `output: false`, `include: false`, `warning: false` (one fixture per key, each with a stored record that has
      stderr and stdout outputs). Present them through `present_document` **and** through the splice. Pin today\'s
      bytes. Decision D3 applies: a cell **without** resolved options is layered with the document scope.
- [ ] **Document-scope test (knitr), R required.** A knitr document with `execute: {record: true, output: false}`:
      assert the record\'s cell keeps its outputs and its `quarto_resolved_options` hold `results: "hide"` and
      `fig.show: "hide"` (R translates `output: false` in `crates/quarto-core/src/engine/knitr/resources/rmd/execute.R:348-353`),
      and the rendered HTML has no `cell-output` divs. Skip cleanly without R. Follow knitr epic decision 17 (R tests
      are CI-ready, CI stays disabled).
- [ ] **Loop differences.** One test per row of the table above, each pinning **both** sides:
      forwarding (`#| data-testid: hero-plot` appears as a cell attribute through `present_document`, not through the
      splice; `// OPR-1a:` the splice will forward); invalid cell options; stored-markdown format mismatch (note
      versus silence); the span rows of the next box; the mixed-engine row of the box after.
- [ ] **Span cases.** A table-driven test over both span routines with these records, each with its expected result
      today: one text value; stdout then a value; two text values (last versus first); an error then a value; an
      image-only bundle then a text value; an empty result; a span in front matter (value with and without a newline).
      Mark every row where the two differ with `// OPR-1b:` and the decided result (`classify_inline_result` rule).
- [ ] **Mixed-engine document.** A record with two engines in one document (knitr plus Python): pin which engine
      entry\'s `known_options` reach a knitr cell through each loop. `// OPR-1a:` per-cell lookup everywhere.
- [ ] **Entry-path equivalence (ports of S1 and S2).** Port the front-end diff (`from_config` against `from_knitr`
      over the union of option keys; today 55 of 65 identical and the 10 differences are knitr\'s dotted aliases) and
      the goldens-through-the-knitr-entry-path test (add the marker `quarto_resolved_options: {}` to every record
      cell; today 12 of 13 shadow goldens are byte-identical and `warning-false-stderr` differs because
      `knitr_visibility` does not read `warning`). **Add the six `ipynb-*` goldens**, which S2 did not run, and record
      what they do. These pin the equivalence OPR-1a must keep and the one difference OPR-2 will remove.
- [ ] **Julia route.** Keep and extend `only_julia_with_units_and_a_wanted_record_is_presented_by_core`
      (`engine/ts_record.rs`, near line 1622) only if it needs a companion; do not change `core_presents`.
- [ ] Register new integration test files in `crates/quarto-core/tests/integration/main.rs`, alphabetically.
- [ ] Gates: `cargo clippy -p quarto-core --all-targets -- -D warnings`, `cargo nextest run -p quarto-core`, and the
      Windows cross-clippy for `quarto-core` (protocol).
- [ ] Write "Handoff to OPR-1a and OPR-1b" in this file: a table of every test marked `// OPR-1a:` or `// OPR-1b:`,
      with the intended new expectation. This table is the checklist those plans use to prove their changes.

## Exit criteria

- All new tests pass on today\'s code (no product file under `crates/*/src` changed, except test modules).
- The Handoff table exists and matches the `// OPR-1a:` and `// OPR-1b:` comments in the tree
  (`git grep -n 'OPR-1[ab]:'` and the table list the same tests).
- The six `ipynb-*` results and the span-case table are recorded in "Notes from the run".

## Unknowns this plan answers

Whether the six `ipynb-*` goldens differ through the knitr entry path; whether the two substitution routines produce
the same text for the same cell (compare their handling of blank lines around a cell); whether the span cases differ
as read.

## Notes from the run

(Fill in at landing.)
