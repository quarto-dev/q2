# Research: Editable CustomNode slots in q2-preview

> **Superseded:** by Plans 7d / 7e (2026-05-29). The `CoarsenedEntry::CustomNodeSplice` variant proposed here is not needed under the algebraic dispatch — R3 with the CustomNode shell helpers added in Plan 7e covers the same case structurally. This note is preserved as historical context for the slot-editing design discussion.

**Date:** 2026-05-05
**Status:** Research / design sketch — out of scope for the current
q2-preview epic (Plans 1-8); captured here for findability if pursued.
**Context:** Generated from a conversation thread about whether the
q2-preview design "scales in" to support direct editing of CustomNode
slots in their respective React components (e.g., editing a Callout's
title and body content directly in the rendered preview, with edits
round-tripping back to source).

## Goal

Enable direct slot-level editing of non-atomic CustomNodes through React
components. Specifically: a Callout component lets the user edit its
title and body content inline; the edits round-trip to source qmd
preserving the wrapper's `:::` syntax, attribute choices, and any
between-slot whitespace.

This is distinct from the "atomic CustomNode" cases (`IncludeExpansion`,
`CrossrefResolvedRef`) which are deliberately read-only. It applies to
the structured-but-editable types: `Callout`, `Theorem`, `Proof`,
`FloatRefTarget`, `Equation`.

## Foundation in the current epic

The type-level groundwork is already in place after Plans 4-8:

1. **Slot content carries Original source_info pointing at sub-ranges
   of the wrapper's bytes.** Sugar transforms like `CalloutTransform`
   inherit source_info from their input Div: the title slot's Header
   points at `## Watch Out` bytes, the body slot's Para points at
   `Be careful!` bytes, the wrapper covers `::: … :::` whole-block
   bytes.

2. **`preimage_in` is depth-agnostic.** Plan 7's accessor walks any
   chain (Substring/Concat/Derived) and returns byte ranges in the
   target file. Slot children's source_info → walks → returns
   sub-range of the wrapper's bytes. No new infrastructure needed to
   find slot byte ranges.

3. **`is_atomic_custom_node` is type-keyed, not whole-class.** Callout,
   Theorem, Proof, etc. are NOT in the atomic registry. Plan 7's
   atomic-detection path correctly treats them as editable.

4. **`MaybeReadOnlyInline` (Plan 2) gates editability per inline.** The
   pattern extends to "per-slot setLocalAst forwarding" — Callout
   component can pass `setLocalAst` to its title and content slot
   dispatchers; CrossrefResolvedRef component refuses to.

5. **`By::user_edit` is reserved.** When the React layer constructs a
   fresh inline from a typed character, it tags it
   `Synthetic { by: By::user_edit() }`. The writer can distinguish
   user-typed text inside an editable slot from transform-emitted text.

## What would need to be added

Three places need new mechanism:

### (A) Reconciler slot-level recursion

Today's reconciler treats CustomNodes as opaque blocks. Looking at
`crates/pampa/src/writers/incremental.rs:178-180` and
`crates/quarto-ast-reconcile/src/`:

- `block_inlines()` returns `Some(&[Inline])` only for `Para`/`Plain`/
  `Header`. CustomNodes have multiple slots, no single inline-content
  region.
- The reconciler produces `BlockAlignment::RecurseIntoContainer` only
  for blocks `block_inlines()` knows about, plus block containers
  (Div/BlockQuote) handled by separate logic.

For CustomNode slot recursion, the reconciler needs a new alignment
shape (or a new variant inside `RecurseIntoContainer`) carrying
per-slot plans:

```rust
RecurseIntoCustomNode {
    before_idx: usize,
    after_idx: usize,
    slot_plans: HashMap<String, SlotPlan>,
    plain_data_changed: bool,
    attr_changed: bool,
}

enum SlotPlan {
    BlockSlot(BlockReconciliationPlan),
    InlineSlot(InlineReconciliationPlan),
}
```

The reconciler walks each slot independently, hashing children to
decide KeepBefore/UseAfter/RecurseIntoContainer per slot.

### (B) Writer slot splice

Mirroring today's `InlineSplice`, add a new `CoarsenedEntry::CustomNodeSplice`:

- Verbatim-copy the wrapper's bytes from the original.
- For each *unchanged* slot: keep the corresponding source bytes verbatim
  (already covered by the wrapper-Verbatim — slot bytes are sub-range
  of wrapper bytes).
- For each *changed* slot: identify its source range (via slot child
  source_info → `preimage_in`), splice in re-assembled content using
  the slot's plan.
- Wrapper-internal bytes that don't belong to any slot — the
  `::: {.callout-warning}` line, the closing `:::`, blank lines or
  comments between slots — stay verbatim by virtue of being
  wrapper-bytes outside any slot's range.

If `plain_data` or `attr` changed (user changed `.callout-warning` to
`.callout-tip`), fall through to whole-CustomNode Rewrite via the qmd
writer's CustomNode arm. That's a reasonable v1 cut: preserve
slot-edit fidelity, accept loss-of-formatting on type changes.

### (C) Per-CustomNode-type slot-position knowledge

The `:::` syntax doesn't encode slot identity by name; slot positions
are determined by document structure. For example, a Callout's title
slot is the first `## Header` immediately after the opening `:::`; if
the user adds a title to a Callout that didn't have one, the writer
needs to know "where in the wrapper bytes a title would go if there
were one."

This is solvable but requires per-CustomNode-type logic. Probably
belongs to the same place that lives the type-specific source-shape
contract for each CustomNode kind — the inverse of `CalloutTransform`,
`TheoremSugarTransform`, etc. (Or symmetric companions to them: a
`CalloutQmdShapeTransform` that knows the source shape.)

## Where it gets interesting

**Slot identity vs. position.** Adding a slot that didn't exist requires
the writer to know structural placement rules (where titles go,
where labels go). Removing a slot is straightforward (skip its source
range during splice).

**Sugar inversion isn't always a function.** `CalloutTransform` reads a
Div with class `.callout-warning` and produces
`CustomNode("Callout", plain_data: { type: "warning" })`. Going back, a
Callout with `type: "warning"` could re-emit as `:::` block syntax or
as a Div block. The original source tells you which the user wrote;
wrapper-Verbatim preserves it. For a fresh user-added Callout, the
writer picks a canonical syntax (same problem the regular qmd writer
already faces).

**Equation editing means math.** `Equation` CustomNode contains a `Math`
inline. Editing it well means editing LaTeX in a math editor (or a
specialized inline UI). Outside the writer's concern, but the surface
is interesting — the `Math` inline's text becomes the editable region;
the wrapper bytes (`$$ … $$ {#eq-foo}`) stay verbatim around it.

**Caption + body interaction in figures.** `FloatRefTarget` may have
both a body and a caption slot. Users edit captions independently
of body content. The slot-splice mechanism handles this naturally
since each slot has its own source range.

## Why it's "scale in" rather than "scale up"

The current epic (Plans 1-8) scales the design **outward** — more
atomic types, more pipeline transforms, more provenance kinds — without
much architectural change. Plan 1's pipeline list grows; Plan 4's `By`
kinds grow; Plan 7's atomic registry grows. Each addition is a new
entry in an extensible list.

Scaling **inward** (deeper recursion into CustomNode internals)
requires:
- Reconciler grows CustomNode-aware recursion (new alignment shape).
- Writer gains slot-level splice (new `CoarsenedEntry` variant).
- Per-CustomNode-type logic for slot placement (a new kind of registry
  with structural rules).

The first two are mechanical. The third is the real design pressure
point — it's where editor-flavored knowledge of the qmd source shape
lives, distinct from the parser's knowledge.

## Estimated scope (rough)

If pursued as a follow-up plan (call it "Plan 9"):

| Component | Lines (rough) |
|---|---|
| Reconciler `RecurseIntoCustomNode` alignment + slot plans | ~200 |
| Writer `CustomNodeSplice` coarsen variant + assemble | ~250 |
| Per-CustomNode-type slot-shape registry (initial 5 types) | ~300 |
| React component updates (Callout, Theorem, Proof, FloatRefTarget, Equation) — slot-level setLocalAst forwarding | ~200 |
| Tests | ~400 |
| **Total** | **~1350** |

Comparable in scope to Plan 7. Builds on these plans rather than
rewriting them.

## Realistic next step if pursued

This would land as **Plan 9** in this epic, after Plan 8. By then:
- Plans 4-7 have done the heavy lifting (provenance types, writer
  refactor, atomic-or-not infrastructure).
- Plan 8 has proven the writer can handle non-trivial CustomNode
  semantics.
- Plan 9 generalizes to "CustomNode with editable slots" by reusing
  the same machinery in a less restrictive mode.

## Specific UX questions left open

- **Caret behavior across slot boundaries.** What happens when the
  user types past the end of a title slot — does the caret move into
  the body slot, or get blocked at the slot boundary?
- **Drag-to-reorder across slots.** Probably out of scope; slot
  identity matters and reordering breaks the structural contract.
- **Undo granularity.** Slot-level edits should undo to slot-level
  states, not character-by-character (probably handled by Automerge's
  existing undo).
- **Read-only sub-regions inside editable slots.** A Callout's body
  slot might contain a shortcode-resolved Str (Derived). The slot is
  editable in general but that one inline isn't. The
  `MaybeReadOnlyInline` wrapper from Plan 2 covers this; the
  composition is straightforward.

## Pointers

- `crates/quarto-ast-reconcile/src/` — reconciler internals; the
  alignment-types module would extend here.
- `crates/pampa/src/writers/incremental.rs` — writer; `CustomNodeSplice`
  variant lands here.
- `crates/pampa/src/writers/qmd.rs` — qmd writer's CustomNode arm; the
  per-type slot-shape registry would either live here or in a sibling
  module.
- `crates/quarto-core/src/transforms/callout.rs` (and theorem.rs,
  proof.rs, etc.) — sugar transforms; their inverse logic is what
  the slot-shape registry encodes.
- `hub-client/src/components/render/` — Plan 2's React components;
  per-type slot-level editing UI lives here.

## Status

Not on the q2-preview roadmap. Surface this doc when:
- A concrete user request comes in for "edit my Callout's title in the
  preview without reloading the whole document."
- Or when the design conversation that produced this needs context.
