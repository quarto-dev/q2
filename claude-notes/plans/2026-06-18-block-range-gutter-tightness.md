# Block source-range tightness — blockquote gutters & list continuation

**Date:** 2026-06-18
**Branch:** feature/block-editing-improvements (worktree `.worktrees/block-editing`)
**Status:** **RESEARCH** — problem framed, options sketched, open questions listed.
NOT yet a development plan; do not implement from this. Convert to a dev plan
(TDD checklist) only after the open questions below are answered.
**Parent / prior art:** `2026-06-01-q2-preview-plan-7g-source-range-tiling.md`
(the provenance epic's source-range tiling work — **landed** on `feature/provenance`,
merged) and `claude-notes/designs/provenance-contract.md` (the P1–P4 contract).
**Motivating downstream bug:** G16 in `2026-06-18-block-editing-glitches-2.md`
(the nesting-cursor "down arrow caught in a blockquote loose list").

## Problem

A **block** node's `source_info` range absorbs the *next* line's leading
blockquote gutter / continuation prefix. Concretely, for

```
> 1.  oh
>
> 2.  dear
```

the `oh` `Plain`'s range is `[31,39]`, whose end byte (39) is the `2` of the
**next** item — i.e. the range reaches past `oh`'s own content, across the blank
`> ` continuation line, and into `dear`'s marker. The same happens for every
loose item inside a blockquote.

This is invisible to most consumers but breaks anything that needs the *lines a
block's content actually occupies*: the nesting-cursor down-navigation computed
the current block's trimmed span as **2 lines** instead of 1, so a step "down"
re-resolved the block onto itself ("caught"), and `surfaceAtLine` even resolved a
sibling's content line to the wrong block (overlap). See the G16 byte-level trace.

### Current downstream workaround (already shipped)

`ts-packages/preview-renderer/src/q2-preview/nestingNav.ts` `surfaceLineSpan` was
made **marker-aware**: it computes a surface's line span from lines that carry
visible content, treating a `>` gutter as non-content (`/[^\s>]/` per line). This
is a *heuristic* compensation in TS for a producer-side range that isn't tight.
It works, but it re-derives — imperfectly and per-consumer — something the parser
knows exactly. If the producer range were tight, this trim could be deleted.

## Relationship to Plan 7g (why this is a deliberate gap, not a regression)

7g established and **CI-enforces** a tight-range contract (P1 tight, P3 trim both
ends, P4 tiling). But it is scoped, by explicit decision, in two ways that leave
*this* case out:

1. **Inline-leaf only.** Tightness is checked at inline leaves; block-level
   assertions are non-overlap + parent⊇child containment, never trailing
   tightness. (Verified in `crates/pampa/tests/integration/tiling_phase3_tests.rs`
   — every tightness case is inline: code span, cite, quoted, raw inline, math,
   abbreviation; the one block case asserts containment only.)
2. **ASCII-whitespace only.** `tight_source_info_for_node`
   (`crates/pampa/src/pandoc/location.rs:265`) trims leading/trailing **ASCII
   whitespace**; a `>` gutter is not whitespace, so it is never trimmed. (Matches
   7g's "space/tab only" decision.)

And 7g's **scope boundary** names this exact territory as intentionally excluded:

> "Blank lines between blocks, **`> ` gutters**, and list indentation are
> legitimately unowned; BP tolerates them … Inventing nodes to own them is a
> larger, separate, probably-undesirable goal."

So our case is the consciously-deferred seam: a **block-leaf trailing** range over
a **`>` gutter** — outside both of 7g's scope limits. This plan is the candidate
*extension* of 7g, not a bug in it.

## Where the relevant code lives

- **Producer helpers** — `crates/pampa/src/pandoc/location.rs`:
  - `range_to_source_info_with_context` (:247)
  - `tight_source_info_for_node` (:265) → `SourceInfoOptions::trim_all()` (ASCII ws)
  - `leading_whitespace_source_info` (:280) — carves peeled ws into the `Space`
- **Block handlers** producing the over-wide ranges: the AST construction in
  `crates/pampa/src/pandoc/treesitter_utils/` (list items / blockquote / `postprocess.rs`).
  *(Exact emission site for list-item `Plain`/`Para` ranges TBD — see Q1.)*
- **The tiling auditor (CI property test)** — `crates/pampa/tests/integration/tiling_phase3_tests.rs`; finding type `TilingFinding` in `crates/pampa/src/writers/incremental.rs`.
- **The splice / round-trip consumer** — `crates/pampa/src/writers/incremental.rs`.
- **Downstream heuristic to retire** — `nestingNav.ts` `surfaceLineSpan`.

## Design options (to evaluate, not yet decide)

- **A — content extent field.** Keep the raw range (load-bearing for editing) and
  add a parser-computed *content* extent per block (first→last content byte,
  gutters/indent/blank-continuation excluded). Consumers split: navigation/measurement
  use content extent, editing uses raw. Most general; biggest schema/serialization
  change (`SourceInfo` payload + TS wire-format + `profile_version`-style bump).
- **B — tighten the trailing block range.** A new gutter-aware
  `SourceInfoOptions` trim ("trim trailing blockquote-continuation / blank-gutter
  lines": consume trailing `\n` + `[ \t]*>[ \t]*` runs) applied to block handlers'
  trailing extent so a block ends at its last content line. No new field; smallest
  surface. Creates **gaps** (the gutter owned by nobody) — which the 7g contract
  *already blesses* (P4 is non-overlap, not gap-free). Risk concentrated in the
  splice consumer (see Risk below).
- **C — content line-spans.** Emit each block's content `[startLine,endLine]`
  directly (pampa already tracks line/col). Exactly what the nav hot path wants;
  less fundamental than byte extents (presentation-coupled). Possible as a derived
  convenience on top of A/B.

Leaning **B** for the minimal fix that retires the heuristic, **A** if a second
consumer needs the content extent or if tightening the raw range proves unsafe for
the writer.

## Key risk

`incremental_write` / `apply_node_edit` may rely on sibling ranges **tiling**
(contiguously covering the source) for splicing. Tightening block ranges (option
B) introduces gaps; A avoids it by leaving raw ranges untouched. **This is the
gating question** — resolve Q2 before committing to B.

## Open research questions (answer before converting to a dev plan)

- [ ] **Q1 — Emission site.** Where exactly do loose list-item `Plain`/`Para`
  (and `<dd>`) ranges get their trailing extent? Is the absorption from the
  tree-sitter node range, or added in `postprocess.rs` / list handling? Map the
  producer path.
- [ ] **Q2 — Writer tiling dependency (GATING).** Does
  `crates/pampa/src/writers/incremental.rs` require contiguous tiling, or does it
  tolerate gaps? If it tolerates gaps, B is viable; if not, A (additive field) is
  forced. Construct a round-trip test (edit an item in a blockquote loose list)
  and check behaviour under a prototype tightening.
- [ ] **Q3 — Scope of absorption.** Is this only the trailing `>`-gutter case, or
  do leading gutters / nested `> >` / lazy continuation / tab indentation also
  over-absorb? (Up-nav works today, suggesting leading is currently fine —
  confirm.) Enumerate the shapes a fix must cover.
- [ ] **Q4 — Auditor extension.** What's the minimal change to extend the tiling
  auditor to **block-leaf trailing tightness** so the new invariant is CI-enforced
  like Phase 3? Does the boundary-byte predicate generalise, or does "content vs
  gutter" need a new classifier?
- [ ] **Q5 — `>` vs content disambiguation.** Confirm the parser can cleanly
  distinguish a blockquote-marker `>` from a content `>` at the producer layer
  (it should — token boundaries are known), so the fix is exact where the TS
  heuristic `/[^\s>]/` is approximate.
- [ ] **Q6 — Other consumers.** Besides `surfaceLineSpan`, which consumers
  re-derive content spans (caret geometry, breadcrumb, scroll-sync)? Quantify the
  heuristic-removal payoff and any behaviour that would *change* if ranges tighten.

## Definition of "research done"

Q1, Q2, Q3, Q5 answered with evidence; a recommendation between A and B with the
writer-safety verdict; a sketch of the auditor extension (Q4). Then convert to a
TDD development plan (producer fix + auditor check + `surfaceLineSpan`
simplification + round-trip regression), sequenced behind a `cargo xtask verify`.

## References

- `claude-notes/plans/2026-06-01-q2-preview-plan-7g-source-range-tiling.md` (parent epic)
- `claude-notes/designs/provenance-contract.md` (P1–P4)
- `claude-notes/plans/2026-06-18-block-editing-glitches-2.md` (G16 — the downstream symptom + the `surfaceLineSpan` workaround)
- Code: `crates/pampa/src/pandoc/location.rs`, `crates/pampa/tests/integration/tiling_phase3_tests.rs`, `crates/pampa/src/writers/incremental.rs`, `ts-packages/preview-renderer/src/q2-preview/nestingNav.ts`
