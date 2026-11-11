---
title: 'OPR-9f gate report: plain-render parity gate before the flip'
date: 2026-11-11
description: 'Result of running the whole parity harness on opr/9f-run-the-gate with no flip: counts, per-entry fire counts, the not-compared list, and the caption and reference lines of the three fig-subcap fixtures.'
---

**Plan:** [OPR-9f](../plans/2026-11-10-opr-9f-run-the-gate.md) · **Branch:** `opr/9f-run-the-gate` (on top of 9e, `0c84cc337`).
**Run:** `OPR_PARITY_REPORT=<scratchpad>/full-report.md cargo nextest run -p quarto-core -E 'binary(integration) & test(opr_parity_runner::opr_parity_report)' --no-capture`
(R 4.3.2 with knitr 1.50, Julia 1.12 with Plots, Node; 138 s; the test passed). (ran)

## Counts

| Class | Cases |
|---|---:|
| identical | 81 |
| allowlisted (proposed or signed entries) | 12 |
| unallowlisted difference (blocker) | **0** |
| document rejected by q2 before any engine runs (same on both routes) | 253 |
| both routes failed at execution, same message | 2 |
| routes disagree on success, or failed differently | 0 |
| skipped | 0 |
| formats not compared (zip output) | 8 (see below) |

The two "both failed" cases are `julia/engine-error` (expected) and `knitr-corpus/engines/shell_chunk` (the Jupyter runtime
is absent on this machine; same message on both routes, flagged "NOT expected" by the report). The 253 rejected documents are
121 parse errors and 132 Q-2-36 old-style chunk options in the knitr corpus; they never reach a route.

## Entries and fire counts

Fire counts are per side (plain, recorded). The interleaving normalizer must fire on the recorded side only.

| Entry | Status | Plain | Recorded | Cases |
|---|---|---:|---:|---|
| knitr empty cell wrapper | signed | 5 | 0 | 5 knitr html |
| knitr split blockquote | signed | 1 | 0 | `chunk_in_blockquote` |
| julia code block leading newline | signed | 1 | 0 | `julia/docs-index-chunk` |
| julia epub interleaved cell (pin, 9e) | proposed | 0 | 0 | `julia/targets-epub` |
| julia presentation-attrs interleaved cell | proposed | 0 | 1 | `julia/presentation-attrs` |
| julia records-plot interleaved cell | proposed | 0 | 1 | `julia/records-plot` |
| julia fig-subcap interleaved cell | proposed | 0 | 1 | `julia/fig-subcap` |
| julia capture-facts interleaved cell | proposed | 0 | 3 | `julia/capture-facts` |
| julia capture-facts pinned difference | proposed | 0 | 0 | `julia/capture-facts` |

Every interleaving entry is needed (none stale) and fires on the recorded side only. A pinned entry reports 0 and 0 by design.

**Entries added by 9f (all `proposed`):** the four per-document normalizer entries and one pinned entry (the plan said "four";
`julia/capture-facts` needs both the normalizer and the pin, as its unit tests in `opr_parity_harness.rs` already assume, so
there are five rows). The pin is `pins/julia-capture-facts.txt` (25 lines).

**The pin differs from the unit-test fixture.** I first copied `opr-parity-normalizer/capture-facts.pin.txt` (written in 9d); the
live document no longer matches it, because that fixture predates 7h and 7i (the recorded page now has one block per expression
and the code-copy scaffold around each stderr output). I wrote the pin from the live run, after the normalizer, and read every
line: the differences are the four designed kinds (the `>` prompt of `#| prompt: true`, numeric `echo: [2]`, `warning: false`
keeping `keepme` and the raw stderr write while dropping the warning, and the per-expression split), plus the order note. The
unit-test fixture still passes its own tests; it is a frozen sample, not the live pin. Gordon should read
`pins/julia-capture-facts.txt` before signing.

## Not compared

Eight knitr cases, all `docx` and `epub` of four documents:
`rich-output/kable`, `rich-output/df_print_default`, `rich-output/knit_asis`, `rich-output/include_graphics`.
The knitr loop of `opr_parity_runner.rs` still filters on the text-only `COMPARABLE` list; 9d left it unchanged on purpose
("changing that is not 9d's"). The harness has the zip extractor (it compares the Julia `epub`, `docx` and `pptx` cases).
To measure them, I changed that one line to the `EXTRACTABLE` list **locally, without committing**, and ran
`OPR_PARITY_ONLY=rich-output/`: six of the eight cases were identical, and the two `include_graphics` cases were among the
documents q2 rejects before any engine (the same document is rejected for html). No blocker. (The narrowed run exits
nonzero by design, because the knitr entries are then stale in a 20-case subset; the counts are what matter.)
This is a stop condition of the plan on its face (a zip format not compared), so I stopped and asked.

## The three `fig-subcap` fixtures: caption and reference lines

Before the flip = the plain route (`q2 render`); after = the recorded route (`execute: record: true`), which is what the flip
makes the plain route. `julia/fig-subcap`, `knitr/fig-subcap` and `jupyter/fig-subcap`, html, rendered by `target/debug/q2`
with the fixture's own document. The figure captions and the reference sentence are identical in all six renders:

```text
Figure 1: One figure.
Figure 2: First.
Figure 3: Second.
Figure 4: A group.
Table 1: A table.
References: Figure 1, Figure 4, Figure 2, Figure 3, Table 1.
```

The group caption, the panel ids (`fig-two-1`, `fig-two-2`) and the references resolve on both routes. The numbers (the group
counted after its panels) are the known wrong numbering that is out of scope (epic D13 and D14, `bd-au5gts6e`,
`bd-gbaykhth`). The Julia render needed the default Plots environment's `Project.toml` and `Manifest.toml` copied beside the
document, as the harness does. (ran; I inspected the extracted text, not the HTML as a browser shows it.)

## Differences not covered by an entry

None.
