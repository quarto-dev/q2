---
title: 'OPR-7g plan review: the strip rule, the sites, the shared table, the overlap'
date: 2026-11-09
description: 'A light review of the OPR-7g plan against the code: where the strip-when-no-resolved-options rule is right, every site that writes output text, what the shared Rust and TS table must expect, and what OPR-7f and OPR-7h change underneath it.'
---

**Plan:** [OPR-7g](../plans/2026-11-09-opr-7g-ansi-at-presentation.md). **Method:** reading the code on
`feature/one-presenter` (head `1125ef116`) and the two running branches, plus one node run of the TS pattern. No
implementation. Marks: (read), (ran).

## Findings

1. **The strip rule holds for every record source (read).** Only the knitr adapter writes `quarto_resolved_options`
   (`engine/knitr/adapter.rs:123`; a fallback chunk writes none). Julia\'s mapping (`engine/ts_record.rs`) reads
   `output_kinds`, `output_anchors` and `expression_starts` and writes no resolved options, so OPR-4d is safe. The
   in-process Jupyter engine, ipynb replay, the splice stage, preview and replay all present through
   `present_cell_step`, so one flag covers them. The only case where the proxy and the intent differ is an Ark R cell in
   an imported Positron record: it gets stripped while knitr R keeps its escapes. Accepted; the plan says to note it.
2. **One real choke point, one missed site (read).** `render_cell_interleaved` and `render_cell_with` both end in
   `format_outputs_with`. The plan missed its `results: asis` branch, which writes stream and `text/plain` text bare,
   outside `fenced_output_div`. Added. Inline spans are excluded with a reason: the value is a `text/plain` result, and
   the TS route does not strip spans either, so there is no parity difference.
3. **The shared table was wrong in two places (read, ran).** The TS `ANSI_PATTERN` mangles OSC 8 hyperlinks: the
   final-byte class takes the `h` of `https`, leaving `ttps://...` in the output, for both BEL and ST terminators. The
   Rust function drops a lone ESC where TS keeps it, and Rust leaves `ename` unstripped where TS strips the joined
   body. Decision: one fixed pattern with an OSC branch on both sides, `stripAnsiCode` exported, rows pinned in the
   plan. The quarto-api vitest is in CI (`ts-test-suite.yml:372`).
4. **Overlap (read).** 7f rewrites `fenced_output_div`, `wrap_cell`, `push_figure` and every arm of
   `format_outputs_with` (and edits `to-markdown.ts` and the allowlist). 7h touches one arm (image size) and one test.
   The pin test asserts 8 entries and three signed ones; 7f drops five entries, so 7g must read it fresh, not assume 8.
   This is why 7g now depends on both and runs last.
5. **Added:** a Phase 0 box set, the two final boxes from Gordon (full gate, flip), mechanical exit criteria (a grep
   gate, two named test commands, the report with five counts), and the lane change to A in the plan and the epic
   table.

## Decisions I made (no question for Gordon)

- The TS pattern gets an OSC fix. It changes the plain route only for hyperlink escapes, which the corpus lacks.
- The strip happens before `trim_output` and `comment_out`.
- The flag rides on `CellOptions`, set where `resolved_options` is consumed, rather than a second metadata read.

## What I did not check

- Whether any Julia or Jupyter document in the parity corpus contains an OSC escape (assumed none; Phase 0 greps).
- The Positron fixtures\' actual escape content, and the exact shape of `render_expanded_unit`\'s option copy.
- 7h\'s plan for the parity harness (its branch has only the `cell.rs` change so far).
- No build or test was run; the review is reading plus one node snippet.
