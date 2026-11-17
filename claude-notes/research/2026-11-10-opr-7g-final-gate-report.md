---
title: 'OPR-7g: final parity gate run (box A)'
date: 2026-11-10
description: 'The full OPR parity gate after OPR-7f, 7g and 7h: 67 identical, 6 allowlisted, 6 unallowlisted differences, each attributed to a cause that already has an owner. ANSI is closed. OPR-7d stays blocked.'
---

**Plan:** [OPR-7g](../plans/2026-11-09-opr-7g-ansi-at-presentation.md), box A. **Earlier report:** [OPR-7c](2026-11-09-opr-7c-parity-report.md).

## Invocation (ran)

```bash
OPR_PARITY_REPORT=<file> cargo nextest run -p quarto-core \
  -E 'binary(integration) & test(opr_parity_runner::opr_parity_report)' --no-capture
```

Branch `opr/7g-ansi-at-presentation` at the commit that adds the ANSI work, on the integration branch tip
(`017216f2b`, nothing newer). R, Julia 1.12 with Plots, Python/uv and Node were present. The run took 123 s, not the
roughly 7 minutes the plan expected. No Julia server timeout occurred, so no document was re-run.

## The five counts

| Class | Cases |
|---|---:|
| identical | 67 |
| allowlisted (3 entries, all signed) | 6 |
| **unallowlisted (target: zero)** | **6** |
| rejected by q2 before any engine runs (same on both routes) | 253 |
| skipped | 0 |

Also: 2 documents where both routes fail the same way (`julia/engine-error`, expected; `knitr-corpus/engines/shell_chunk`,
no jupyter runtime) and 8 zip formats not compared. No route disagrees on success.

## ANSI: closed

`julia/engine-reordering`, the new `julia/ansi-output`, the new `jupyter/ansi-output` and
`knitr-corpus/markdown-stress/ansi_and_unicode` (R keeps its escape on both routes) are all identical.

## The six unallowlisted differences, by cause

None is an ANSI difference, and none is new. Each already has an owner.

| Document | Cause | Owner |
|---|---|---|
| `julia/capture-facts` | A multi-expression cell is split per expression on the recorded route (OPR-4d; 7d decision 4 accepts it as an improvement) | 7d: needs the whole-document allowlist form |
| `julia/presentation-attrs` | The same split: its print-then-figure cell | 7d, same form |
| `julia/records-plot` | The same split, plus the Plots script elements below | 7d, and 7h part A |
| `julia/docs-index-chunk` | Plain emits the `requirejs` and `jquery` script elements that Plots output needs; the recorded route has none | 7h part A: a proposal, no fix or entry |
| `julia/positron-cache` | The same three script lines, nothing else | 7h part A |
| `julia/fig-subcap` | No panel synthesis for `fig-subcap` groups (`bd-gbaykhth`; 7d decision 3 accepts wrong becoming unresolved) | 7d decision 3, strand |

I attributed these from the report's diff lines and the 7f and 7h notes. I did not re-render any of them narrowed to
confirm a cause.

## Other gates (ran)

- `cargo clippy -p quarto-core --all-targets -- -D warnings`: clean.
- `cargo nextest run -p quarto-core`: 8212 run, 8211 passed, 1 failed, 36 skipped. The failure is
  `revealjs::assemble::tests::vendored_reveal_assets_match_npm_package`: the vendored `reset.css` differs from this
  checkout's `node_modules/reveal.js`, which is dated Aug 25. This change touches no reveal file; I did not check it on the
  integration branch. `marimo_engine_e2e::` ran and passed. The knitr corpus control tests passed.
- `npm test -w ts-packages/quarto-api` (vitest): 402 passed, 1 skipped.
- `cd hub-client && npm run build:wasm`: succeeded.
- The Windows cross-clippy was not run: this machine cannot cross-check `aws-lc-sys` from macOS.
