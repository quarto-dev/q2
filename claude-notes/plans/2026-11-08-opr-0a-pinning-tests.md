---
title: 'OPR-0a: pinning tests for the presenter loops, scope, spans and entry paths'
date: 2026-11-08
description: 'Tests first for the one-presenter epic. Pins what the two presenter loops, the document execute scope, the span routines and the knitr and Jupyter entry paths do today, so OPR-1a and OPR-1b can prove they changed only what the epic decided. No product code changes.'
status: complete
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

- [x] Read the oracle and sweeps: `jupyter_markdown_goldens.rs`, the 13 shadow and six `ipynb-*` goldens, `presenter/jupyter_option_sweep.rs`, and the knitr `record_splice/tests/option_sweep.rs`.
- [x] **Document-scope fixtures (Jupyter).** `a_jupyter_cell_with_no_resolved_options_is_layered_with_the_document_scope`
      (`record_splice/tests/opr_pinning.rs`): one source per key (`echo`, `output`, `include`, `warning`), a record with stdout
      and stderr, through `present_document` and the splice. Both loops agree today. In-code sources, not fixture files.
- [x] **Document-scope test (knitr), R required.** `knitr_bakes_the_document_output_scope_into_the_record`
      (`tests/integration/presenter_pinning.rs`): `results: hide` and `fig.show: hide` in the resolved options, outputs kept,
      no `cell-output` in the HTML. Skips without Rscript and knitr. Ran here (R present).
- [x] **Loop differences.** One test per row: forwarding, invalid options, stored-markdown mismatch, spans (below),
      mixed engine (below), all in `record_splice/tests/opr_pinning.rs`, each pinning both sides.
- [x] **Span cases.** `span_cases_through_both_routines` (table-driven), plus the notes test and the two front-matter tests.
- [x] **Mixed-engine document.** `the_engine_entry_a_knitr_cell_sees_differs_between_the_loops`.
- [x] **Entry-path equivalence.** `presenter::options::opr_pinning` (S1 front-end diff: 55 of 65 equal, ten dotted aliases
      differ), `shadow_goldens_through_the_knitr_entry_path` (S2) and `ipynb_goldens_through_the_knitr_entry_path` (the six
      `ipynb-*` goldens).
- [x] **Julia route.** Not needed: `only_julia_with_units_and_a_wanted_record_is_presented_by_core` needs no companion; `core_presents` untouched.
- [x] `presenter_pinning` registered in `tests/integration/main.rs`, alphabetically.
- [x] Gates: `cargo clippy -p quarto-core --all-targets -- -D warnings` (green), `cargo nextest run -p quarto-core` (see Notes). The Windows cross-clippy could not run: `aws-lc-sys` compiles C for the target and fails on macOS (the known caveat); the new tests use no platform API and no path literals.
- [x] Handoff table (below).

## Handoff to OPR-1a and OPR-1b

Every comment tagged `OPR-1a:` or `OPR-1b:` (`git grep -n 'OPR-1[ab]:'`), with what the plan must change it to.

| Test | Tag | Today | Intended new expectation |
|---|---|---|---|
| `unknown_cell_option_is_forwarded_by_render_but_not_by_the_splice` | 1a | splice emits `::: {.cell}` with no `data-testid` | splice forwards: `::: {.cell data-testid='hero-plot'}` (D9) |
| `the_engine_entry_a_knitr_cell_sees_differs_between_the_loops` | 1a | render uses the one entry it is handed; handed Jupyter's, `bar.unknown` is not forwarded | entry looked up per cell: forwarded whichever entry the caller passes |
| `span_cases_through_both_routines`, "two text values" | 1b | splice `1` (first), render `2` (last) | both `2` |
| same, "an error then a value" | 1b | splice `7`, render keeps the span | both keep the span source |
| same, "an image-only bundle then a text value" | 1b | splice keeps the span, render `5` | both `5` |
| `shadow_goldens_through_the_knitr_entry_path` | 1a (keep) | 12 of 13 identical; `warning-false-stderr` differs (stderr shown) | unchanged until OPR-2 moves `warning` enforcement |
| `ipynb_goldens_through_the_knitr_entry_path` | 1a (keep) | all six identical | unchanged |

Pinned and **not** tagged (no epic decision changes them): invalid options (render fails, splice keeps source, D1 keeps
the error policy per caller); stored-markdown mismatch (render notes it, splice silent); front-matter spans (render
presents, splice skips); the Jupyter document scope (both loops identical); the S1 option-reader diff (ten dotted aliases).

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

**Ran:** `cargo clippy -p quarto-core --all-targets -- -D warnings` (clean); `cargo nextest run -p quarto-core`: 8054 run,
8053 passed, 1 failed. The failure is `revealjs::assemble::tests::vendored_reveal_assets_match_npm_package`: this
workspace\'s `node_modules/reveal.js` has a `reset.css` that differs from the vendored copy (property order). It is
environmental, unrelated to this plan, and no product file changed. The 13 new tests (9 in
`record_splice/tests/opr_pinning.rs`, 1 in `presenter/options.rs`, 3 in `tests/integration/presenter_pinning.rs`)
all pass, with R and knitr present. **Not run:** the Windows cross-clippy (`aws-lc-sys`
fails to compile C for `x86_64-pc-windows-msvc` on macOS); no new test uses a platform API or a path literal.

**Findings.**

- *Six `ipynb-*` goldens through the knitr entry path:* all six are byte-identical to their goldens. The 13 shadow
  goldens: 12 identical, `warning-false-stderr` differs (stderr is shown), as the S2 spike found. Each run has a control
  without the marker that reproduces the golden, so the stand-in engines are faithful.
- *S1:* 55 of 65 option rows equal; the ten differences are knitr\'s dotted aliases (`fig.align`, `fig.env`, `fig.cap`,
  `fig.scap`, `fig.subcap`, `fig.pos`, `out.width`, `out.height`, `out.extra`, `fig.link`).
- *Span cases (render, splice):* one value (42, 42); stdout then value (42, 42); two text values (last `2`, first `1`);
  error then value (span kept, `7`); image-only then text (`5`, span kept); empty (kept, kept). Front matter: render
  writes the escaped value and fails on a newline; the splice skips. All as read in the plan.
- *Document scope (Jupyter):* both loops agree for all four keys (`echo`: no code; `output`: code only; `include`: nothing;
  `warning`: stdout but no stderr). The loops differ in none of them today.
- *Same text for the same cell (blank lines):* not measured beyond the wrapper cases already pinned in
  `jupyter_markdown_goldens`; the two loops produced equal text in every scope case above.
- *Invalid options:* the record builder rejects invalid `#|` YAML, so the test builds a record from a valid twin and
  re-keys the cell\'s id and content hash to the invalid source (the splice matches it by content hash).
- *Knitr scope:* R\'s record has `results: hide` and `fig.show: hide`, keeps the stdout output, and the rendered HTML has no
  `cell-output` and no `[1] 2`. (The echoed code still contains the text of the `cat` call, so the assertion is on the
  output value.)
- Document-scope fixtures are in-code sources, not files, so no snapshot or golden file was added, changed or removed.
