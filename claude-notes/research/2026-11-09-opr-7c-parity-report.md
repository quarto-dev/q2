---
title: 'OPR-7c: parity gate report for the plain-render switch'
date: 2026-11-09
description: 'The parity gate run on the final presenter (after OPR-2, 4c, 4d and 7b): 56 identical, 13 allowlisted by proposed entries, 7 differences left for Gordon. Classifies every difference and lists the blockers.'
---

**Plan:** [OPR-7c](../plans/2026-11-08-opr-7c-run-the-parity-gate.md) ·
**Baseline:** [OPR-0b baseline](2026-11-09-opr-parity-baseline.md).
Machine: macOS, R 4.3.2 with knitr, Julia 1.12.7 with Plots, `uv`. Marks: (ran) unless noted.

Command (ran, about 6.7 minutes):

```bash
OPR_PARITY_REPORT=report.md cargo nextest run -p quarto-core \
  -E 'binary(integration) & test(opr_parity_runner::opr_parity_report)' --no-capture
```

## Counts

First run (before this plan\'s two new allowlist rules), then the final state. The final state combines the second
full run with two narrowed re-runs (`OPR_PARITY_ONLY=chunk_in_blockquote` and `=docs-index-chunk`): the second full
run had `julia/docs-index-chunk` fail on a Julia server timeout (transient, the shared-server gotcha), and its
`chunk_in_blockquote` row predates a rule fix.

| Class | First run | Final |
|---|---:|---:|
| identical | 56 | 56 |
| allowlisted (proposed) | 12 | 13 |
| unallowlisted difference | 8 | 7 |
| rejected by q2 before any engine runs | 253 | 253 |
| both routes failed alike (one expected) | 2 | 2 |
| skipped | 0 | 0 |

`knitr/cache-mixed` is identical on a miss and on a hit, now that OPR-7b landed; its `pending` mark is removed and the
manifest note says so (identical is the expected result).
Moves since the baseline: the `julia/records-plot`-style Plots differences are unchanged; `julia/capture-facts` is new
(OPR-4d\'s case); the leading-newline difference is now allowlisted; `chunk_in_blockquote` is allowlisted.

## Allowlist changes (all `proposed`)

Two entries added, each with a rule and a harness test written with it:

- `knitr-adjacent-blockquotes` (`knitr` scope): R\'s markdown puts a blank line inside a quoted chunk, so the quote
  renders as two blockquotes; the presenter renders one. This is the page-level view of `structure/chunk_in_blockquote`,
  already a blocker-class entry in the knitr `allowlist.toml` (epic decision 23). Explains 1 case. My first rule
  called this an "empty shell"; the real difference is a split quote, and the rule was rewritten.
- `julia-code-leading-newline` (`julia/` scope): the engine route keeps a code block\'s blank first line, the
  presenter trims it. A browser drops a newline at the start of a `pre`. Explains 1 case (`julia/docs-index-chunk`).

The harness comment now says `signed`, matching the plan, not `signed-off`.

## Differences left for Gordon (7 documents, 5 causes)

None is allowlisted: each is larger than a small fix in `presenter/`, `record_splice.rs` or `engine/knitr/`, or is a
decision.

1. **Sub-figure group, `julia/fig-subcap`.** Plain builds the group float and names files by label; recorded writes plain
   figures and leaves the group references unresolved (`bd-gbaykhth`, `bd-au5gts6e`; OPR-7d decision 3). A blocker by the
   plan\'s own words.
2. **ANSI escapes in Julia output, `julia/engine-reordering`.** The `@test` result prints `Test Passed` on plain and with
   color escapes on recorded. The plain route strips ANSI (`stripAnsiCode` in `ts-packages/quarto-api/src/jupyter/to-markdown.ts`,
   on streams and `text/plain`); the presenter strips it only in error tracebacks (`strip_ansi_codes` in
   `presenter/cell.rs`). **The fix is not "strip everywhere":** the knitr corpus pins the opposite for R
   (`markdown-stress/ansi_and_unicode`: R keeps `ESC[31m` in stdout). So the presenter needs a rule for which outputs
   strip (for example, only when the cell has no resolved options, the D3 test), and that is a design choice across
   engines. This also affects the live Jupyter route through the presenter today (read: no stripping code on that path;
   not run). **Decision needed.**
3. **Plots scripts, `julia/records-plot`, `julia/positron-cache`, `julia/docs-index-chunk`.** Plain emits requirejs and
   jquery `script` elements (from the `text/html` bundle Plots returns) that the recorded route does not. For a static GR
   plot they are not needed, but I did not check what the recorded route does with an HTML bundle that does need them, so
   I cannot call this safe. **Needs a look at how the presenter treats `text/html` dependencies** (not done).
4. **`julia/plot-website`.** Plain writes `width="140" height="90"` on a bare PNG; recorded writes `class="img-fluid"`
   and no size. The presenter reads a size only from output metadata (`metadata_size`); I did not find out where the
   plain route gets 140 by 90 from, or whether the record carries it. Not investigated further.
5. **`julia/capture-facts` (new).** The OPR-4d case: interleaved output, exact `prompt`, numeric `echo` and
   `warning: false` by kind. Plain runs the whole cell as one block; recorded splits it per expression. This is the
   designed behavior of 4d (Julia never interleaved in Quarto 1), so it is a **behavior change at the flip**, not a
   regression. The harness has no whole-document allowlist form, so I left it unlisted; Gordon decides whether the change
   is accepted and how to record it.

## Candidates from the plan, re-checked

- **`id="cell-N"`, `data-quarto`, `data-execution_count`** (ran: grep of `crates/`, `ts-packages/`, `hub-client/src`,
  `resources/`, for the three names): no reader; the only hits are the unrelated `data-quarto-component`. Proposed, unchanged.
- **Generic `cell-output` class on a figure div:** proposed, unchanged (`julia/fig-png-bare`). The dashboard reader was
  not re-checked with a Julia figure in a dashboard.
- **Figure file names, bare PNG:** still the `julia-figure-names` proposed entry. The plan\'s other option (make the
  presenter count prose and use the label so the URLs stay) was **not** done: the presenter\'s record does not carry
  the position, as far as I found. Every Julia figure URL changes at the flip; Gordon should confirm.

## Not covered

253 knitr-corpus documents never reach a route (old-style chunk headers, Q-2-36); their parity stays at the engine level
(KNR-6). Eight docx and epub comparisons are not made (zip output). Windows was not run.

## Evidence notes

- (ran) two full gate runs, two narrowed runs, the 15 harness tests.
- (read) the ANSI, `metadata_size` and `plot_image_attrs` code; (not ran) any check of how plain gets the PNG size.
