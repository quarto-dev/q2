# Item plane — research-level plan (DEFERRED)

**Date:** 2026-06-19
**Branch:** TBD (follow-on to `feature/block-editing-improvements`)
**Status:** RESEARCH / DEFERRED — not scheduled. Sibling of the **block plane**
(`2026-06-18-boundary-splice-edit-design.md` + `2026-06-19-boundary-splice-implementation.md`).

> This is a research stub, not an implementation plan. It captures the problem,
> the open design questions, and rough estimates so the work can be picked up
> cleanly. Do **not** start implementing from this file — it must first go
> through brainstorming → spec → implementation plan like the block plane did.

## What the item plane is

The block plane splices **`Block`s within a `Blocks` slice** (`Vec<Block>`). The
item plane splices the *other* element type that lists/tables/def-lists are built
from:

```rust
BulletList  { content: Vec<Vec<Block>> }                        // elements: ITEMS (Vec<Block>)
OrderedList { content: Vec<Vec<Block>>, .. }
DefinitionList { content: Vec<(Vec<Inline> /*term*/, Vec<Vec<Block>> /*bodies*/)> }
Table { ... rows of cells ... }                                 // elements: ROWS
```

Operations: **insert / move / delete an item** (list item, table row, def-list
entry) — splices on `Vec<Vec<Block>>` (or the table row vector), not on a
`Blocks` slice. "Add a kanban card", "drag a card to position 4", "add a row",
"add a definition" all live here.

## Why it was split out of the block plane (decided 2026-06-19)

- **Different element type.** A list item is `Vec<Block>`, not a `Block`. The
  block plane's `splice_range` operates on `Vec<Block>`; items are one level up.
- **Different content shape.** A new item's payload is "the blocks that make up
  the item" — and multi-item inserts need item delimiting. The block plane's
  `Content` (`md`/`ast` → blocks) does not express "these blocks form item k,
  those form item k+1".
- **It is the only thing that wanted a positional slot among sibling item-slices**
  — i.e. the positional `item`/`(def,body)` coordinates we deliberately kept out
  of the block plane's `ContainerRef`. Putting them here keeps the block plane
  fully SI-addressed and index-free.
- **No current consumer needs a primitive.** Today: typed list editing goes
  through the text channel (qmd reparse builds items for free); `kanban.tsx`
  reorders via `replaceNode`-the-whole-list. So the item plane is real future
  work, not blocking.

## How it composes with the block plane (additivity check)

The split is additive. The item plane adds its **own boundary family** over
item-arrays; it does not change the block plane's `Boundary`/`ContainerRef`. A
gesture that fills a freshly-inserted item is "insert item *with content*" (one
item-plane op), not "insert empty item then block-splice into it" — so the block
plane never needs `listItem`/`defBody` back.

Sketch (to be designed, not final):

```
ItemBoundary = beforeItem(listSi, k) | afterItem(listSi, k)
             | startOfList(listSi)   | endOfList(listSi)        // gap 0 / item-count
ItemContent  = md(...) parsed as items | items(blockGroups...)   // shape TBD
insertItemAt(listSi, k, itemContent)  // { from: beforeItem(listSi,k), to: ..., itemContent }
moveItem(listSi, from, to)            // delete + insert, or a dedicated op
```

`listSi` SI-matches the specific (possibly nested) list at any depth — depth is
absorbed by SI exactly as in the block plane; only *one* shallow index per list
level, never a root path.

## Open design questions (the research)

1. **Item content shape (the hard one).** How does `md('- a\n- b')` map to
   *items*? How does the `ast` form express "two items"? Single- vs multi-item
   payloads and delimiting. Does it reuse `Content` or get its own type?
2. **Tight vs loose lists.** Inserting into a tight list must stay tight — the
   item-level analog of the block plane's `preserve_leaf_variant` (Plain↔Para).
   How is tight/loose detected and preserved on insert?
3. **Ordered-list semantics.** Renumbering, `start`/`style`/`delim` interaction
   with the incremental writer when items are inserted/removed/reordered.
4. **Scope: which containers.** Bullet/ordered lists first? Then def-list entries
   (need a *term*, and bodies are `Vec<Vec<Block>>` — two coordinates) and table
   rows (fixed column count → row shape validation). Each is its own sub-design.
5. **Reconciler/writer behavior.** Confirm `compute_reconciliation` handles item
   count changes cleanly (the `list_*_produces_correct_length` tests in
   `quarto-ast-reconcile` suggest yes) and that the incremental writer's
   whole-list re-serialization is acceptable (it is the same nested-edit behavior
   the block plane already accepts).
6. **Move semantics.** Is "move item" a first-class op (so reconciliation can
   keep source fidelity of the moved item) or just delete+insert? Relevant to
   kanban drag.
7. **Frontend coordinates.** A list-level component (kanban) has `itemIndex`
   positionally; a component nested *inside* an item does not (item indices are
   render-time-local — confirmed 2026-06-19). Decide whether to expose item
   indices via context, or keep item-plane gestures list-level only.

## Rough estimates (ideal hours, assuming the block plane is in place)

| Scope | Planning | Engineering |
|---|---|---|
| Bullet/ordered lists | 3–5h | 8–14h |
| + def-list entries + table rows | +2–3h | +8–12h |

Biggest swing risks: the **item content shape** (Q1), **tight/loose + ordered
renumbering** (Q2/Q3), and whether migrating `kanban.tsx` from `replaceNode` to
item-moves surfaces reconciler quirks (Q5/Q6).

## Entry criteria

Pick this up when a real gesture needs a structural item primitive (e.g. kanban
drag wanting surgical item moves instead of whole-list `replaceNode`, or a
nesting-cursor "new list item" that we decide should be structural rather than
text-reparse). Until then it stays deferred. Start with brainstorming (the
content shape, Q1, deserves a real design pass), not with code.
