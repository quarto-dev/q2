---
title: 'KNR-4: knitr inline expressions as recorded values'
date: 2026-10-30
description: 'Records each knitr inline expression as an inline unit of the engine execution record, aligned with the Jupyter execution-of-inline-expressions conventions, replacing the id-embedding mechanism of EXR-12 once the structured capture exists.'
status: draft  # outline written from code reading; mechanism answered by KNR-1 S9
---

**Part of:** [the epic](2026-10-30-knitr-execution-records-epic.md) (plan KNR-4).
**Depends on:** [KNR-3](2026-10-30-knr-03-record-adapter-hash-fallback.md) (adapter and
fallback), [EXR-12](2026-10-28-exr-12-knitr-k2-inline.md) (a draft on 2026-10-31: the current mechanism this plan replaces),
[EXR-2](2026-10-28-exr-02-unit-scanner.md) and [EXR-4](2026-10-28-exr-04-record-format.md)
(the stage\'s shared unit list, with the document-wide span counter and the ids). Soft:
[EXR-3](2026-10-28-exr-03-jupyter-iee.md) (the Jupyter conventions to match).
**Blocks:** KNR-6.
**Estimated sessions:** 2.

> **Status of this document.** Written 2026-10-30 from code reading, before KNR-1 runs, and
> revised on 2026-10-31 after the paper review and the parent epic\'s decisions 38 to 40.
> The knitr inline internals were read in the development snapshot of knitr; the
> value-formatting question is not verified, and it decides the design. KNR-1 has no inline
> spike yet; S9 (added to KNR-1) covers it.

## Goal

Every inline expression knitr evaluates becomes an inline unit of the record whose output
is the value, matched by id, so that preview and hub staleness work per span and replay
does not need R. Rendering is unchanged.

## Background

**What knitr does (read 2026-10-30 in the development snapshot, `block.R:634-665`).** For each span `inline_exec`
runs `hook_eval(code[i], envir)` (the `evaluate.inline` hook), turns `knit_asis` values into
text with `sew(res, inline = TRUE)`, coerces to character, and only `if (length(res))`
calls the `inline` hook, whose result replaces the span with `str_replace(input,
block$location, ans)`. So a `NULL` or invisible value never reaches the `inline` hook, and
the text group is rebuilt around the replaced spans.

**What q2 adds today (EXR-12 background; verify before relying).** A Rust pre-pass
(`engine/knitr/preprocess.rs`) rewrites the brace spelling `{r} expr` to
`r .QuartoInlineRender(expr)`. `.QuartoInlineRender` (`resources/rmd/execute.R:181-195`)
renders `NULL` as the text `NULL`, passes `AsIs` through, and backslash-escapes markdown
specials in character values. The classic spelling `r expr` is not wrapped, because
`I(NULL)` is an R error, so the two spellings render `NULL` differently on purpose. EXR-12
embeds a unit id as a leading statement, `.QuartoInlineId("3-1a2b3c4d-inline"); <expr>`
(the id is EXR-4\'s `{n}-{hash8}-inline`, from the stage\'s shared unit list; parent
decisions 38 and 40), and records `{id, inserted markdown}` through a wrapper on
`evaluate.inline` plus the `inline` hook.

**What the structured design changes.** EXR-12 records the *inserted markdown*. This epic's
records hold values as nbformat outputs, as Jupyter's spans do (EXR-3: a value output with a
text mime), and the Rust presenter inserts text. For R that raises the question below.

**The value-formatting question (not verified).** knitr's default markdown inline hook
formats numbers (rounding, scientific notation, a big mark) before insertion. A kernel
would return `repr` text for the same value. Whether to record the *value as R's formatted
text* (what knitr would insert, before q2's escaping) as `text/plain`, or a typed value,
decides how much formatting logic the Rust presenter needs. The cheap answer is the former:
record the text produced by knitr's inline formatting, un-escaped, and let the presenter
apply the spelling-specific escaping.

## Design decisions (from the epic; do not re-litigate)

1. A span is a unit, `quarto_kind: inline`, with the id format and domain-separated content
   hash of EXR-4 (`{n}-{hash8}-inline`, hash input `inline:<lang>\n<code>`). The span
   counter runs document-wide over all spans, owned or not (parent decision 38), and the
   shared span regex is the only definition of a span (decision 40), so a span another
   engine owns never shifts the ids of knitr\'s.
2. Statements and errors in spans are fatal and `error: true` is ignored, as in q1; `None`
   and `NULL` follow EXR-12 and EXR-3 (parent decisions 17 and 21).
3. Additive until KNR-6: the live output does not change.

## Provisional: decisions this plan must make

1. **Capture mechanism** *(answered by S1 and S9, 2026-10-31: wrap `evaluate.inline` and `inline`, one entry per span by position, text as knitr inserts it; see KNR-1 S9)*: keep EXR-12\'s `evaluate.inline` wrapper
   with an embedded id, or capture the span by position from the pre-pass. The new capture
   already patches knitr internals, so the id can ride in the unit list the Rust side hands
   to R (as chunk ids do in EXR-10) instead of being embedded in the expression.
2. **What is recorded** (text as knitr would insert vs typed value; see above), and where
   substitution happens. The venue is decided by KNR-6 (its skeleton decision, S7): R keeps
   substituting values in the live document, or markers carry them. This plan builds the
   record side and a presenter function that inserts a recorded value with the spelling\'s
   escaping; its tests do not assume the venue.
3. **The two spellings\' `NULL` rule**: keep the difference (empty versus the text `NULL`)
   as a presenter rule. Nothing records the spelling today (EXR-4 and EXR-12 have no
   spelling field), so the unit carries it as an explicit `quarto_*` cell key (EXR-4
   permits extra `quarto_*` keys; the name is decided here once S9 reports what is
   needed; proposed: `quarto_inline_spelling`).
4. **Empty and invisible values** (EXR-12 decision 4 behavior: an unserved pending id is
   recorded as empty): carry over, with no fallback to a bare counter (adjacent spans are
   evaluated once; `preprocess.rs:71`), and with ids taken from the shared list, never
   generated here.
5. **Spans in unusual places** (attribute values, link titles, nested display fences): the
   shared scanner and the mask decide; carry over EXR-12's corpus.

## Out of scope

- Jupyter spans (EXR-3), Julia spans (EXR-8); changing escaping rules or the adjacent-span
  limitation; Positron's lack of execution of inline expressions (EXR-16 S3).

## Phases (provisional)

- [ ] **Tests first.** Port EXR-12\'s tests: one unit per span in document order; both
  spellings; `NULL` and invisible spans keep their ids; attribute and link-title spans;
  nested display fence spans are not units; count mismatch yields no record plus a
  diagnostic with the span\'s location; the live output is identical with recording on and
  off after KNR-1\'s normalizer. New: ids are stable when another engine owns spans between
  knitr\'s (the document-wide counter), and the spelling key round-trips.
- [ ] **Mechanism.** The S1-based capture; the Rust-side id hand-off if chosen.
- [ ] **Adapter.** Inline units into the record via KNR-3; value output per the decision.
- [ ] **Presenter.** Insert the recorded value with the spelling's escaping; shadow test
  against live output on EXR-12's fixtures.
- [ ] **Retire EXR-12's mechanism** in KNR-6, once nothing reads the old `spans` field.

## Exit criteria

- Every span in the corpus has a unit; shadow equality with the live path; EXR-12's tests
  pass against the new mechanism; clippy and nextest green with counts reported.

## Risks

- **Number formatting drift** between R's formatting and any Rust re-implementation: avoid
  by recording R's formatted text.
- **Hook order.** User-defined `inline` or `evaluate.inline` hooks (same class of problem as
  user hooks in KNR-3): detect and fall back.

## References

- `external-sources/knitr/R/block.R:634-665`; EXR-12; EXR-3; EXR-4 (inline units);
  `crates/quarto-core/src/engine/knitr/{preprocess.rs,resources/rmd/execute.R}`
